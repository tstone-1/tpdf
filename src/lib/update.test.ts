/**
 * Tests for the update check and the download that follows it.
 *
 * What is being pinned is not "updates work" --- that needs a real endpoint and
 * a real signature, and `BUILD.md` schedules it as a manual step. It is the five
 * ways this turns into either a busy loop, a silent swap, or a header that lies:
 * a second operation starting on top of one already running, an install with no
 * check behind it, a `Finished` event treated as success, a missing
 * `Content-Length` reported as 0%, and a failed check that leaves a stale handle
 * behind for `install` to use.
 *
 * The last two are the quiet ones. A progress bar stuck at 0% reads as a slow
 * network, and an install running off a handle from a check that *failed* is the
 * one path here that could apply something the reader was never shown.
 *
 * Every test below was checked by mutating `update.ts` and confirming it went
 * red; the mutations are named on each block.
 */

import { describe, expect, it, vi } from "vitest";
import app from "../App.svelte?raw";

import {
  finishPrompt,
  finishUpdate,
  installEndsProcess,
  percentOfDownload,
  Updates,
  updateLabel,
  updateNotice,
  type DownloadEvent,
  type FinishEffects,
  type FinishPrompt,
  type UpdateHandle,
  type UpdaterApi,
} from "./update";

/** An update the fake updater will offer. */
function handle(version = "26.8.2", events: DownloadEvent[] = []): UpdateHandle {
  return {
    version,
    downloadAndInstall: async (onEvent) => {
      for (const e of events) onEvent(e);
    },
  };
}

/** An updater that answers with whatever it is given. */
function api(result: UpdateHandle | null | Error): UpdaterApi {
  return {
    check: async () => {
      if (result instanceof Error) throw result;
      return result;
    },
  };
}

describe("automatic update preference", () => {
  function stored(initial: string | null = null) {
    let value = initial;
    return {
      getItem: vi.fn(() => value),
      setItem: vi.fn((_key: string, next: string) => { value = next; }),
    };
  }

  it("checks once on a new installation without downloading anything", async () => {
    const storage = stored();
    const downloadAndInstall = vi.fn();
    const check = vi.fn(async () => ({ version: "99.1.0", downloadAndInstall }));
    const updates = new Updates({ check }, undefined, () => storage);
    await updates.checkOnLaunch(); await updates.checkOnLaunch();
    expect(check).toHaveBeenCalledTimes(1);
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(storage.setItem).not.toHaveBeenCalled();
  });

  it("remembers disabling across restart and preserves the manual check", async () => {
    const storage = stored();
    const check = vi.fn(async () => null);
    new Updates({ check }, undefined, () => storage).setAutomatic(false);
    const restarted = new Updates({ check }, undefined, () => storage);
    expect(restarted.automatic).toBe(false);
    expect(await restarted.checkOnLaunch()).toEqual({ kind: "idle" });
    expect(check).not.toHaveBeenCalled();
    expect(await restarted.check()).toEqual({ kind: "current" });
    expect(check).toHaveBeenCalledTimes(1);
    expect(storage.getItem).toHaveBeenCalledWith("tpdf.automaticUpdates");
    expect(storage.setItem).toHaveBeenCalledWith("tpdf.automaticUpdates", "false");
  });

  it("enables future launches without starting a request when the choice changes", async () => {
    const storage = stored("false"), check = vi.fn(async () => null);
    const updates = new Updates({ check }, undefined, () => storage);
    await updates.checkOnLaunch();
    updates.setAutomatic(true);
    await updates.checkOnLaunch();
    expect(check).not.toHaveBeenCalled();
    await new Updates({ check }, undefined, () => storage).checkOnLaunch();
    expect(check).toHaveBeenCalledTimes(1);
  });

  it("skips automatic traffic for unreadable and malformed preferences", async () => {
    const check = vi.fn(async () => null);
    for (const storage of [stored(""), stored("broken"), stored("false"), {
      getItem: () => { throw new Error("unreadable"); }, setItem: vi.fn(),
    }]) {
      const updates = new Updates({ check }, undefined, () => storage);
      await updates.checkOnLaunch(); expect(updates.automatic).toBe(false);
    }
    await new Updates({ check }, undefined, () => { throw new Error("unavailable"); }).checkOnLaunch();
    expect(check).not.toHaveBeenCalled();
  });

  it("reports persistence errors, disables immediately and refuses an unsaved enable", async () => {
    const storage = stored(), check = vi.fn(async () => null);
    storage.setItem.mockImplementation(() => { throw new Error("full"); });
    const updates = new Updates({ check }, undefined, () => storage);
    expect(() => updates.setAutomatic(false)).toThrow("full");
    expect(updates.automatic).toBe(false);
    expect(() => updates.setAutomatic(true)).toThrow("full");
    expect(updates.automatic).toBe(false);
    await updates.checkOnLaunch(); expect(check).not.toHaveBeenCalled();
  });

  it("does not retry a failed automatic check", async () => {
    const check = vi.fn(async () => { throw new Error("offline"); });
    const updates = new Updates({ check }, undefined, () => stored());
    await updates.checkOnLaunch(); await updates.checkOnLaunch();
    expect(check).toHaveBeenCalledTimes(1);
  });

  it("wires startup through the preference and confines direct checking to the manual action", () => {
    // The module tests cannot see a caller that bypasses the preference entirely.
    expect(app.match(/updates\.checkOnLaunch\(\)/g)).toHaveLength(1);
    expect(app.match(/updates\.check\(\)/g)).toHaveLength(1);
    expect(app).toMatch(/async function checkAndSay\(\)[\s\S]*?notice = updateNotice\(await updates\.check\(\), appVersion\);\s*}/);
  });
});

describe("percentOfDownload", () => {
  it("reports a percentage when the total is known", () => {
    expect(percentOfDownload(50, 200)).toBe(25);
  });

  it("says nothing rather than zero when there is no total", () => {
    // Mutation: return 0 instead of null -> red. A bar reading 0% forever is
    // indistinguishable from a stalled download; "unknown" is the honest answer.
    expect(percentOfDownload(50, null)).toBeNull();
    expect(percentOfDownload(50, 0)).toBeNull();
  });

  it("clamps an overshoot, because a stale Content-Length is not our bug", () => {
    // Mutation: drop the Math.min -> red.
    expect(percentOfDownload(300, 200)).toBe(100);
  });
});

describe("Updates.check", () => {
  it("reports an available update with its version", async () => {
    const u = new Updates(api(handle("26.9.0")));
    expect(await u.check()).toEqual({ kind: "available", version: "26.9.0" });
  });

  it("reports being current when there is nothing newer", async () => {
    // The control for the test above: without it, a check that always claimed
    // an update would pass everything else here.
    const u = new Updates(api(null));
    expect(await u.check()).toEqual({ kind: "current" });
  });

  it("reports a failed check instead of throwing", async () => {
    const u = new Updates(api(new Error("no route to host")));
    const s = await u.check();
    expect(s.kind).toBe("failed");
    expect(s.kind === "failed" && s.message).toContain("no route to host");
  });

  it("does not start a second check while one is running", async () => {
    // Mutation: drop the #busy guard -> red. The launch check and a reader
    // typing "check for updates" genuinely race.
    let calls = 0;
    let release!: (v: UpdateHandle | null) => void;
    const slow: UpdaterApi = {
      check: () => {
        calls++;
        return new Promise((r) => (release = r));
      },
    };
    const u = new Updates(slow);
    const first = u.check();
    await u.check();
    expect(calls).toBe(1);
    release(null);
    await first;
    expect(calls).toBe(1);
  });

  it("clears the previous handle when a later check fails", async () => {
    // Mutation: leave #handle alone in the catch -> red. This is the path that
    // could install something the reader was never shown: an update found, then
    // a failed re-check, then install running off the stale handle.
    const u = new Updates({
      check: vi
        .fn<UpdaterApi["check"]>()
        .mockResolvedValueOnce(handle("26.9.0"))
        .mockRejectedValueOnce(new Error("offline")),
    });
    await u.check();
    await u.check();
    expect((await u.install()).kind).toBe("failed");
  });
});

describe("Updates.install", () => {
  it("does nothing without a check behind it", async () => {
    // Mutation: drop the !handle guard -> red (it throws instead).
    const u = new Updates(api(handle()));
    expect(await u.install()).toEqual({ kind: "idle" });
  });

  it("becomes ready only when the promise resolves, not on Finished", async () => {
    // Mutation: settle on the Finished event -> red. The event fires when the
    // bytes arrived; the promise resolves when they were verified and written,
    // and an update that fails its signature check fails between the two.
    const u = new Updates({
      check: async () => ({
        version: "26.9.0",
        downloadAndInstall: async (onEvent) => {
          onEvent({ event: "Finished" });
          throw new Error("signature did not verify");
        },
      }),
    });
    await u.check();
    const s = await u.install();
    expect(s.kind).toBe("failed");
    expect(s.kind === "failed" && s.message).toContain("signature");
  });

  it("reports progress against a known total", async () => {
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(
        handle("26.9.0", [
          { event: "Started", data: { contentLength: 100 } },
          { event: "Progress", data: { chunkLength: 25 } },
          { event: "Progress", data: { chunkLength: 25 } },
        ]),
      ),
      (s) => {
        if (s.kind === "downloading") seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([null, 25, 50]);
  });

  it("accumulates chunks rather than reporting the last one", async () => {
    // Mutation: assign instead of += -> red. With equal chunks the two are
    // indistinguishable, so the chunks here are deliberately different sizes.
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(
        handle("26.9.0", [
          { event: "Started", data: { contentLength: 100 } },
          { event: "Progress", data: { chunkLength: 10 } },
          { event: "Progress", data: { chunkLength: 30 } },
        ]),
      ),
      (s) => {
        if (s.kind === "downloading" && s.percent !== null) seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([10, 40]);
  });

  it("reports an unknown total as unknown throughout", async () => {
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(handle("26.9.0", [{ event: "Started", data: {} }, { event: "Progress", data: { chunkLength: 10 } }])),
      (s) => {
        if (s.kind === "downloading") seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([null, null]);
  });
});

describe("updateLabel", () => {
  it("says nothing for the three states a reader did not ask about", () => {
    // Mutation: return a string for `checking` or `current` -> red. A viewer
    // that announces "you are up to date" on every launch is noise.
    expect(updateLabel({ kind: "idle" })).toBeNull();
    expect(updateLabel({ kind: "checking" })).toBeNull();
    expect(updateLabel({ kind: "current" })).toBeNull();
  });

  it("says nothing about a failed check", () => {
    // Deliberate: a launch with no network must not put an error in the header
    // of a document viewer. The palette reports it to whoever asked.
    expect(updateLabel({ kind: "failed", message: "offline" })).toBeNull();
  });

  it("names the version on offer, and the restart when one is waiting", () => {
    expect(updateLabel({ kind: "available", version: "26.9.0" })).toBe("Update to 26.9.0");
    expect(updateLabel({ kind: "ready", version: "26.9.0" })).toBe("Restart to finish update");
  });

  it("leaves the ready state pressable, rather than labelling a disabled button", () => {
    // **The defect this was written for, asserted at the one layer nothing else
    // reaches.** The header button carried `updateState.kind === "ready"` inside
    // its own `disabled`, so the single state whose label asks the reader to do
    // something was the single state offering nothing to press: the only way
    // out was quitting the application by hand.
    //
    // Source-level, like the three assertions above it, because `App.svelte` is
    // the join and no test imports it --- `AGENTS.md` records that seam as the
    // one three green layers do not cover.
    const button = app.match(/<button\s+class="update"[\s\S]*?<\/button/);
    expect(button, "the update button moved or was renamed").not.toBeNull();
    expect(button![0]).toContain("disabled={updates.busy}");
    expect(button![0]).not.toMatch(/disabled=\{[^}]*kind === "ready"/);
    // And it runs the guarded step rather than `relaunch()` directly, which is
    // what puts the unsaved-work question in front of it.
    expect(button![0]).toMatch(
      /finishUpdateStep\(\s*updateState\.kind === "ready" \? "restart" : "install",?\s*\)/,
    );
  });

  it("distinguishes a download with a total from one without", () => {
    expect(updateLabel({ kind: "downloading", version: "1", percent: null })).toBe(
      "Downloading update",
    );
    expect(updateLabel({ kind: "downloading", version: "1", percent: 40 })).toContain("40%");
  });
});

describe("updateNotice", () => {
  // The state the whole function exists for. `updateLabel` returns null here by
  // design, so before this a reader who pressed "Check for updates" and was
  // already up to date saw nothing at all -- indistinguishable from a command
  // that did not run.
  it("says the running version is the latest, rather than saying nothing", () => {
    const said = updateNotice({ kind: "current" }, "26.8.5");
    expect(said).toContain("26.8.5");
    expect(said).toContain("latest");
  });

  it("names the running version even when the check could not be made", () => {
    // Two questions, one press, and only one of them failed. A reader offline is
    // exactly the reader who needs to know which version they are on.
    const said = updateNotice({ kind: "failed", message: "no network" }, "26.8.5");
    expect(said).toContain("26.8.5");
    expect(said).toContain("no network");
  });

  it("names both versions when there is an update, so they can be told apart", () => {
    const said = updateNotice({ kind: "available", version: "26.9.0" }, "26.8.5");
    expect(said).toContain("26.8.5");
    expect(said).toContain("26.9.0");
  });

  // Every state answers. A `null` anywhere here would put back the silence this
  // replaced, and a switch that grew an eighth state would fail to compile
  // rather than fall through -- but nothing checks that the ANSWER is useful,
  // which is what this does.
  it("answers in every state, and never with an empty string", () => {
    const states = [
      { kind: "idle" },
      { kind: "checking" },
      { kind: "current" },
      { kind: "available", version: "1" },
      { kind: "downloading", version: "1", percent: null },
      { kind: "ready", version: "1" },
      { kind: "failed", message: "x" },
    ] as const;
    for (const state of states) {
      expect(updateNotice(state, "26.8.5").length).toBeGreaterThan(0);
    }
  });
});

describe("which step ends the process", () => {
  // Read off `tauri-plugin-updater` 2.11's own source rather than inferred from
  // symmetry, because the platforms are not symmetric and the whole design
  // downstream of this hangs on which one is which. Windows'
  // `Update::install_inner` ends with `std::process::exit(0)`; macOS' renames
  // the new `.app` into place and returns.
  it("ends on Windows, where installing is the shutdown", () => {
    expect(installEndsProcess(false)).toBe(true);
  });

  it("does not end on macOS, where the restart is a separate step", () => {
    // Mutation: `return true` -> red here. Getting this backwards would put the
    // "discard unsaved changes" question in front of a macOS install, which
    // discards nothing, and take it away from the Windows one, which discards
    // everything.
    expect(installEndsProcess(true)).toBe(false);
  });
});

describe("what a reader is asked before work is lost", () => {
  it("asks nothing when nothing is unsaved", () => {
    // The state most presses are made in. A modal in front of a reader with no
    // unsaved work is a question with one answer, which trains them to dismiss
    // the one that matters.
    expect(finishPrompt("restart", [])).toBeNull();
    expect(finishPrompt("install", [])).toBeNull();
  });

  it("names the one document, and counts several", () => {
    // A reader with one open knows which it is, and the name tells them more
    // than "1 open document" does; a list of eight in a modal is a wall.
    const one = finishPrompt("restart", ["notes.pdf"]);
    expect(one?.message).toContain("notes.pdf");
    const many = finishPrompt("restart", ["notes.pdf", "report.pdf"]);
    expect(many?.message).toContain("2 open documents");
    expect(many?.message).not.toContain("notes.pdf");
  });

  it("says which act is about to happen, and the two are not the same act", () => {
    // Mutation: return the restart prompt for both -> red. A Windows reader
    // pressing Install is told tpdf closes; a macOS reader pressing Restart is
    // told it restarts. One wording for both would be wrong for one of them,
    // and the wrong one reads as reassurance.
    const restart = finishPrompt("restart", ["a.pdf"]);
    const install = finishPrompt("install", ["a.pdf"]);
    expect(restart?.okLabel).toBe("Discard and restart");
    expect(install?.okLabel).toBe("Discard and install");
    expect(install?.message).toContain("closes tpdf");
    expect(restart?.message).toContain("restart");
    // The way out is spelled the same as the close dialog's, because it is the
    // same decision.
    expect(restart?.cancelLabel).toBe("Keep open");
    expect(install?.cancelLabel).toBe("Keep open");
  });
});

describe("finishing an update", () => {
  const ready = { kind: "ready", version: "26.9.17" } as const;
  const available = { kind: "available", version: "26.9.17" } as const;

  /** Effects that record what they were asked, and answer as told. */
  function effects(options: { unsaved?: string[]; agree?: boolean; fail?: string } = {}) {
    const log: string[] = [];
    const prompts: FinishPrompt[] = [];
    return {
      log,
      prompts,
      effects: {
        unsaved: async () => { log.push("unsaved"); return options.unsaved ?? []; },
        confirm: async (prompt: FinishPrompt) => {
          log.push("confirm");
          prompts.push(prompt);
          return options.agree ?? true;
        },
        act: async () => {
          log.push("act");
          if (options.fail) throw new Error(options.fail);
        },
        say: (message: string) => log.push(`say:${message}`),
      } satisfies FinishEffects,
    };
  }

  it("restarts from the ready state with nothing unsaved, asking nobody", () => {
    // The ordinary press, and the one the whole feature exists for.
    const { log, effects: e } = effects();
    return expect(finishUpdate("restart", ready, true, e)).resolves.toBe("acted")
      .then(() => expect(log).toEqual(["unsaved", "act"]));
  });

  it("asks before discarding unsaved work, and acts when the reader agrees", async () => {
    const { log, prompts, effects: e } = effects({ unsaved: ["notes.pdf"], agree: true });
    expect(await finishUpdate("restart", ready, true, e)).toBe("acted");
    expect(log).toEqual(["unsaved", "confirm", "act"]);
    expect(prompts[0]?.message).toContain("notes.pdf");
  });

  it("leaves the update ready when the reader says no", async () => {
    // **The property the button depends on.** Cancelling must change no state
    // at all, so the header still reads "Restart to finish update" and the
    // command is still offered to a reader who saves and comes back. It is
    // asserted as "act was never called" plus the state being the one handed
    // in, because this function holds no state of its own --- and that is the
    // design rather than an accident.
    const { log, effects: e } = effects({ unsaved: ["notes.pdf"], agree: false });
    expect(await finishUpdate("restart", ready, true, e)).toBe("cancelled");
    expect(log).toEqual(["unsaved", "confirm"]);
    expect(ready.kind).toBe("ready");
    // And the step is still offered afterwards: the same call with the same
    // state reaches `act` once the reader agrees.
    const again = effects({ unsaved: ["notes.pdf"], agree: true });
    expect(await finishUpdate("restart", ready, true, again.effects)).toBe("acted");
    expect(again.log).toContain("act");
  });

  it("leaves the update on offer when a Windows install is refused", () => {
    // The same code and the other step, because the two are reached from
    // different commands and only one of them was exercised above. A "no" here
    // must leave `available` intact, so the install is still offered --- and on
    // Windows this is the press that would otherwise close the application with
    // a document unsaved.
    const { log, prompts, effects: e } = effects({ unsaved: ["a.pdf", "b.pdf"], agree: false });
    return expect(finishUpdate("install", available, true, e)).resolves.toBe("cancelled")
      .then(() => {
        expect(log).toEqual(["unsaved", "confirm"]);
        expect(prompts[0]?.message).toContain("closes tpdf");
      });
  });

  it("refuses a restart before the update is applied", async () => {
    // Mutation: drop the state guard -> red. A menu item one frame behind the
    // state, or a second press while the dialog is up, must do nothing rather
    // than end the process with nothing installed.
    for (const state of [available, { kind: "idle" } as const, { kind: "current" } as const]) {
      const { log, effects: e } = effects();
      expect(await finishUpdate("restart", state, true, e)).toBe("withheld");
      expect(log).toEqual([]);
    }
  });

  it("refuses an install that is already applied, and one never found", async () => {
    // The mirror, and the half a single "is there an update" flag would lose:
    // `available` is not the only thing that has to be true.
    for (const state of [ready, { kind: "idle" } as const]) {
      const { log, effects: e } = effects();
      expect(await finishUpdate("install", state, true, e)).toBe("withheld");
      expect(log).toEqual([]);
    }
  });

  it("asks nothing for a step that does not end the process", async () => {
    // macOS installing. The bundle is replaced under a process that keeps
    // running, so there is nothing to discard --- and a modal here would be the
    // question with one answer that the prompt tests above refuse to ask.
    // Mutation: ignore `ends` -> red, because `unsaved` would be consulted.
    const { log, effects: e } = effects({ unsaved: ["notes.pdf"], agree: false });
    expect(await finishUpdate("install", available, false, e)).toBe("acted");
    expect(log).toEqual(["act"]);
  });

  it("asks about unsaved work before a restart, whatever installing does here", () => {
    // **`installEndsProcess` answers for the INSTALL, and only for it.** A
    // restart ends the process on both platforms by definition, so the shell
    // has to OR the two. Reading the platform alone would take the question
    // away from every macOS restart --- which is the one platform where the
    // restart is a step a reader presses at all.
    //
    // Source-level, because `App.svelte` is the join and nothing imports it.
    expect(app).toMatch(
      /const ends = step === "restart" \|\| installEndsProcess\(isMac\(\)\);/,
    );
  });

  it("waits for the reading position to land before ending the process", () => {
    // `settleDocument` *issues* the place write and returns; the relaunch is
    // the very next call and the process is gone a moment later. Without the
    // wait, restoring after an update lands a position or two behind where an
    // ordinary restart lands, and the difference is invisible in every layer
    // that has tests.
    expect(app).toMatch(
      /await places\.settled\(\);[\s\S]*?const \{ relaunch \} = await import\("@tauri-apps\/plugin-process"\);/,
    );
  });

  it("tells the reader when the step itself fails, rather than failing silently", async () => {
    // A relaunch that cannot start the new binary leaves the reader looking at
    // a window that did nothing. The outcome is distinct from `cancelled`
    // because only one of the two is a fault.
    const { log, effects: e } = effects({ fail: "no such binary" });
    expect(await finishUpdate("restart", ready, true, e)).toBe("failed");
    expect(log).toEqual(["unsaved", "act", "say:Error: no such binary"]);
  });
});
