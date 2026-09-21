/**
 * Checking for a new tpdf, and applying one.
 *
 * The application talks to the network in exactly one place and this is it.
 * `docs/THREAT-MODEL.md` §T9 carries what that costs and what bounds it; the
 * short version is that the payload's signature is checked against the public
 * key compiled into the binary *before* anything is unpacked, so the archive
 * parsers the plugin brings in never see bytes an attacker chose.
 *
 * ## Why this is notify-and-apply rather than silent
 *
 * Swapping the binary under somebody who has a document open is rude, and for a
 * viewer an update is never urgent. So a check reports, and the reader decides.
 * The check runs once per launch unless the reader disables it.
 *
 * ## Why the Tauri API is injected
 *
 * `@tauri-apps/plugin-updater` only answers inside a Tauri webview, so a module
 * that imported it directly could be tested for nothing but its type
 * signatures. The four states that actually matter --- a second check while one
 * is running, a check that fails, a download that reports progress, and a
 * download that never finishes --- are all reachable here with a fake, and each
 * of them is a way this turns into either a busy loop or a lie in the header.
 */

/** What the updater is doing, as the header shows it. */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  /** Checked, and this is the newest there is. */
  | { kind: "current" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; version: string; percent: number | null }
  /** Downloaded and applied; takes effect on relaunch. */
  | { kind: "ready"; version: string }
  | { kind: "failed"; message: string };

/** The progress events the plugin emits while downloading. */
export type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

/** One available update, as the plugin describes it. */
export interface UpdateHandle {
  version: string;
  downloadAndInstall(onEvent: (e: DownloadEvent) => void): Promise<void>;
}

/** The half of `@tauri-apps/plugin-updater` this module uses. */
export interface UpdaterApi {
  /** Resolves to the update, or null when the running version is newest. */
  check(): Promise<UpdateHandle | null>;
}

/**
 * Turns a byte count into a percentage, or `null` when there is no total.
 *
 * `null` rather than 0, and the difference is the whole point: a server that
 * sends no `Content-Length` leaves the total unknown, and a progress bar that
 * reads 0% forever is a worse answer than one that admits it cannot say. The
 * header shows a spinner-ish "downloading" for `null` and a number otherwise.
 */
export function percentOfDownload(received: number, total: number | null): number | null {
  if (total === null || total <= 0) return null;
  // Clamped because the sum of chunk lengths can overshoot a stale
  // `Content-Length`, and a header reading 103% looks like a defect in the
  // download rather than in the arithmetic.
  return Math.min(100, Math.max(0, Math.round((received / total) * 100)));
}

/**
 * Holds the update state, and refuses to run two operations at once.
 *
 * The in-flight guard is a field rather than a check on {@link state}, because
 * the states it must exclude are not the same shape: a second `check()` during
 * a download has to be refused too, and `downloading` is not `checking`. A
 * guard written as "only start if idle" would also refuse the perfectly
 * ordinary case of checking again after a previous check said `current`.
 */
export class Updates {
  #state: UpdateState = { kind: "idle" };
  #busy = false;
  #handle: UpdateHandle | null = null;
  #onChange: (s: UpdateState) => void;
  #automatic = false;
  #launchChecked = false;

  constructor(
    private api: UpdaterApi,
    onChange: (s: UpdateState) => void = () => {},
    private storage: () => Pick<Storage, "getItem" | "setItem"> = () => window.localStorage,
  ) {
    this.#onChange = onChange;
    try {
      const saved = this.storage().getItem("tpdf.automaticUpdates");
      // Missing means the existing default; unreadable or malformed means offline.
      this.#automatic = saved === null || saved === "true";
    } catch { /* A failed preference read must not enable network traffic. */ }
  }

  get automatic(): boolean { return this.#automatic; }

  /** Disabling takes effect even if persistence fails; enabling requires a saved choice. */
  setAutomatic(enabled: boolean): void {
    if (!enabled) this.#automatic = false;
    this.storage().setItem("tpdf.automaticUpdates", String(enabled));
    this.#automatic = enabled;
  }

  /** Separate from the manual check, which always remains available. */
  async checkOnLaunch(): Promise<UpdateState> {
    if (this.#launchChecked) return this.#state;
    this.#launchChecked = true;
    return this.#automatic ? this.check() : this.#state;
  }

  get state(): UpdateState {
    return this.#state;
  }

  /** Whether a check or a download is running right now. */
  get busy(): boolean {
    return this.#busy;
  }

  #set(next: UpdateState): void {
    this.#state = next;
    this.#onChange(next);
  }

  /**
   * Asks whether there is a newer version.
   *
   * Returns the state it settled on, so a caller can act without re-reading.
   * A check while something is already running is a no-op that returns the
   * current state rather than an error --- the launch check and a reader
   * typing "check for updates" can genuinely race, and neither is a mistake.
   */
  async check(): Promise<UpdateState> {
    if (this.#busy) return this.#state;
    this.#busy = true;
    this.#set({ kind: "checking" });
    try {
      const found = await this.api.check();
      this.#handle = found;
      this.#set(found ? { kind: "available", version: found.version } : { kind: "current" });
    } catch (e) {
      // A failed check is reported and then forgotten. Nothing retries it: the
      // reader can ask again from the palette, and a viewer that keeps dialling
      // out after being told no is the behaviour this whole file is trying not
      // to have.
      this.#handle = null;
      this.#set({ kind: "failed", message: String(e) });
    } finally {
      this.#busy = false;
    }
    return this.#state;
  }

  /**
   * Downloads and applies the update found by the last {@link check}.
   *
   * Does nothing without one, which is not merely defensive: `available` is the
   * only state where a handle exists, and applying an update the reader was
   * never shown is exactly the silent swap this design rejects.
   */
  async install(): Promise<UpdateState> {
    const handle = this.#handle;
    if (this.#busy || !handle) return this.#state;
    this.#busy = true;
    let total: number | null = null;
    let received = 0;
    this.#set({ kind: "downloading", version: handle.version, percent: null });
    try {
      await handle.downloadAndInstall((e) => {
        if (e.event === "Started") {
          total = e.data.contentLength ?? null;
        } else if (e.event === "Progress") {
          received += e.data.chunkLength;
          this.#set({
            kind: "downloading",
            version: handle.version,
            percent: percentOfDownload(received, total),
          });
        }
        // `Finished` deliberately does not settle the state. The promise
        // resolving is what says the install completed, and those are different
        // moments: the event fires when the bytes have arrived, the promise when
        // they have been verified and written. Settling on the event would
        // report "ready" for an update that then failed its signature check.
      });
      this.#set({ kind: "ready", version: handle.version });
    } catch (e) {
      this.#set({ kind: "failed", message: String(e) });
    } finally {
      this.#busy = false;
    }
    return this.#state;
  }
}

/** What the header says, or `null` when it should show nothing. */
export function updateLabel(state: UpdateState): string | null {
  switch (state.kind) {
    case "idle":
    case "checking":
    case "current":
      // None of these are worth a word in a document viewer's header. A reader
      // who asked explicitly gets the answer from {@link updateNotice} instead,
      // which is what the palette command sets.
      //
      // That sentence was a promise with nothing behind it until 2026-08-19:
      // it said the palette answered, and "Check for updates" landed on
      // `current`, which returns null here, so pressing it did visibly nothing.
      // A comment describing a mechanism that does not exist reads exactly like
      // one describing a mechanism that does.
      return null;
    case "available":
      return `Update to ${state.version}`;
    case "downloading":
      return state.percent === null
        ? "Downloading update"
        : `Downloading update — ${state.percent}%`;
    case "ready":
      // The same words as the command that does it, deliberately: this is a
      // button now rather than a sign, and a reader who reads "Restart to
      // finish update" in the header and then types the same thing into the
      // palette must find one action rather than two names for it.
      //
      // It read `Update ready — restart to finish` until 26.9.17, beside a
      // button that was `disabled` in exactly this state. That is a label
      // promising something the control does not perform, which is the shape
      // `docs/TRAPS.md` keeps recording: the words were true of the *world* and
      // false of the thing they were printed on.
      return "Restart to finish update";
    case "failed":
      return null;
  }
}

/**
 * Whether applying the update ends this process, which differs by platform.
 *
 * **Read off `tauri-plugin-updater` 2.11 rather than assumed, because the two
 * platforms are not symmetric and the asymmetry decides whether unsaved work is
 * at stake.**
 *
 * - **Windows**: `Update::install_inner` hands the MSI or the NSIS setup to
 *   `ShellExecuteW` and then calls `std::process::exit(0)`. So the install
 *   *is* the shutdown: `downloadAndInstall` never resolves, {@link
 *   UpdateState} never reaches `ready`, and whatever was open goes with the
 *   process. `restart_after_install` defaults to true, so the installer starts
 *   tpdf again afterwards.
 * - **macOS**: the same function unpacks the `.app.tar.gz`, moves the running
 *   bundle aside and renames the new one into its place. The process keeps
 *   running the code it already mapped, the promise resolves, and `ready` is
 *   where a reader lands. Nothing restarts on its own.
 *
 * The consequence that is easy to get backwards: on macOS the *install* costs
 * nothing and the *restart* is the step that ends things, while on Windows the
 * install is the only step there is. One flag, read once, rather than two
 * copies of that sentence in the two call sites.
 */
export function installEndsProcess(mac: boolean): boolean {
  return !mac;
}

/** Which half of the update a {@link finishUpdate} call is running. */
export type FinishStep = "install" | "restart";

/** What a reader is asked before a step that would discard their work. */
export interface FinishPrompt {
  title: string;
  message: string;
  okLabel: string;
  cancelLabel: string;
}

/**
 * What to ask before a step that ends the process, or `null` to just do it.
 *
 * Shaped after the dialog the close button already puts up, and that is the
 * point rather than a coincidence: a relaunch discards exactly what a quit
 * discards, so a reader must meet the same question with the same words. One
 * document is named, because a reader with one open knows which it is and a
 * count tells them less than the name does; several are counted, because a list
 * of names in a modal is a wall.
 */
export function finishPrompt(step: FinishStep, unsaved: string[]): FinishPrompt | null {
  if (unsaved.length === 0) return null;
  const what = unsaved.length === 1
    ? `unsaved changes to ${unsaved[0]}`
    : `unsaved changes in ${unsaved.length} open documents`;
  return step === "restart"
    ? {
      title: "Restart tpdf",
      message: `Discard ${what} and restart to finish updating?`,
      okLabel: "Discard and restart",
      cancelLabel: "Keep open",
    }
    : {
      title: "Install update",
      message: `Installing closes tpdf. Discard ${what}?`,
      okLabel: "Discard and install",
      cancelLabel: "Keep open",
    };
}

/**
 * How a {@link finishUpdate} call ended.
 *
 * `withheld` and `cancelled` are kept apart although both mean "nothing
 * happened": the first is the state machine refusing a step that does not
 * belong in this state, the second is the reader saying no. A caller that
 * collapsed them could not tell a stuck update from a respected decision.
 */
export type FinishOutcome = "acted" | "cancelled" | "withheld" | "failed";

/** The shell's half: reading the tabs, asking, acting, reporting. */
export interface FinishEffects {
  /**
   * Names of the open documents with unsaved edits.
   *
   * Async because the caller has to settle pending edits first --- a popup
   * still open and an edit still in flight both change the answer, and reading
   * `dirty` before they land is reading it early.
   */
  unsaved(): Promise<string[]>;
  /** Asks the reader. `true` goes ahead. */
  confirm(prompt: FinishPrompt): Promise<boolean>;
  /**
   * Does the thing.
   *
   * It need not return: on success the process may already be gone. A rejection
   * is a failure the reader is told about; a resolution is success.
   */
  act(): Promise<void>;
  /** Reports a failure. */
  say(message: string): void;
}

/**
 * Runs one half of an update, asking first if work would be lost.
 *
 * **The gate is here rather than in the shell because a relaunch does not go
 * through the window's close handler.** `AppHandle::request_restart` sends
 * `ExitRequested` straight at the run-event loop with an explicit exit code; no
 * window is asked to close, so `onCloseRequested` --- which is where tpdf's
 * "discard unsaved changes in N documents" question lives --- never runs. A
 * button wired directly to `relaunch()` would therefore throw a reader's edits
 * away without a word, which is the one thing a viewer must not do with a
 * single press.
 *
 * The guard on {@link UpdateState} is part of the same job. `install` belongs
 * only to `available` and `restart` only to `ready`; a second press while a
 * confirm dialog is up, or a command reached from a menu one frame behind the
 * state, must do nothing rather than act twice.
 *
 * Cancelling changes no state at all --- which is what leaves the update ready
 * and the button still offered, so a reader who saves and comes back finds the
 * same thing waiting.
 *
 * @param ends Whether this step ends the process; see {@link installEndsProcess}.
 *             When it does not, there is nothing to discard and nothing to ask.
 */
export async function finishUpdate(
  step: FinishStep,
  state: UpdateState,
  ends: boolean,
  effects: FinishEffects,
): Promise<FinishOutcome> {
  const wanted = step === "install" ? "available" : "ready";
  if (state.kind !== wanted) return "withheld";
  if (ends) {
    const prompt = finishPrompt(step, await effects.unsaved());
    if (prompt && !await effects.confirm(prompt)) return "cancelled";
  }
  try {
    await effects.act();
  } catch (e) {
    effects.say(String(e));
    return "failed";
  }
  return "acted";
}

/**
 * What to tell a reader who asked, in so many words.
 *
 * Separate from {@link updateLabel} because the two answer different questions.
 * That one decides what the header shows on its own initiative, where silence is
 * the right answer for every state but three --- an element arriving unbidden
 * moves what somebody is aiming at. This one answers a question that was asked,
 * where silence is never the right answer: a command that appears to do nothing
 * is indistinguishable from one that is broken.
 *
 * `version` is the running version rather than anything the updater reports, so
 * the answer to "which one am I on" does not depend on a network call having
 * succeeded --- which is the case a reader is most likely to be asking in.
 */
export function updateNotice(state: UpdateState, version: string): string {
  switch (state.kind) {
    case "idle":
    case "checking":
      return `tpdf ${version} — checking for updates`;
    case "current":
      return `tpdf ${version} is the latest version`;
    case "available":
      return `tpdf ${version} — version ${state.version} is available`;
    case "downloading":
      return `Downloading version ${state.version}`;
    case "ready":
      // "installed", not "ready": the bytes are already on disk under the
      // running process, and the only thing outstanding is the restart. A
      // reader who reads "ready" reasonably asks what is left to download.
      return `Version ${state.version} is installed — restart tpdf to finish`;
    case "failed":
      // The version still leads, because the reader asked two questions with one
      // press and only one of them failed.
      return `tpdf ${version} — could not check for updates: ${state.message}`;
  }
}
