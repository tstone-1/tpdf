/**
 * The confirmation a web link gets before anything opens.
 *
 * ## The safe-looking version is the dangerous one
 *
 * `docs/PLAN.md` §11 is explicit that this dialog *is* the risk the feature
 * carries: the address is a string a stranger wrote, and a prompt built out of
 * it is a phishing surface with tpdf's own chrome around it. *"Open
 * https://your-bank.example.com∕verify?"* is convincing and is entirely
 * attacker-chosen, with a division slash doing the work. So three rules, and
 * each is here because leaving it out produces a dialog that looks fine:
 *
 * 1. **The host is shown as punycode.** `xn--80ak6aa92e.com`, never the Unicode
 *    rendering, which is the homoglyph attack drawn by us on the attacker's
 *    behalf. That decision is made in `weburl.rs` and cannot be un-made here:
 *    what arrives is already ASCII, and this file has no decoder.
 * 2. **The host is the emphasised element and the path is not.** The path is
 *    where a stranger writes something reassuring ---
 *    `/secure/your-bank.example.com/login` is a path --- so it is secondary,
 *    already truncated by the backend, and never the thing the eye lands on.
 * 3. **Nothing here parses markup or builds a link.** Every string goes in
 *    through `textContent`, and no `<a>` is created --- enforced by
 *    `scripts/check_webview_sinks.py` rather than by this paragraph.
 *
 * ## Per link, every time
 *
 * There is no "always allow this site". A domain grant established from a
 * document a stranger sent turns one careless click into a standing capability,
 * and it is the one shape here that would let a reader be attacked by a link
 * they never saw. A reader working through a specification confirms each one;
 * that is tedious and it is the price, stated in the plan when the decision was
 * taken rather than discovered afterwards.
 *
 * ## Cancel is what has focus
 *
 * Deliberate, and the opposite of what an ordinary dialog does. The
 * confirmation exists to interrupt a reflex --- a reader who clicked expecting
 * an internal cross-reference --- and a dialog whose affirmative button answers
 * the Enter that reflex is about to press has not interrupted anything. So
 * Enter and Escape both cancel, and opening takes a click or a deliberate Tab.
 * The cost is one extra action per link and it falls on the reader who *wants*
 * to go; the alternative puts the cost on the reader who did not.
 */

import type { Target, WebTarget } from "./outline";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-weblink";

/** What a reader is shown about an address, mirroring `Target::Web`. */
export interface WebAddress {
  /** ASCII, punycode where the document wrote an internationalised name. */
  host: string;
  /** Path, query and fragment, already cut to length by `weburl.rs`. */
  rest: string;
}

/** The address in a web target, or `null` for any other kind. */
export function addressOf(target: Target): WebAddress | null {
  return target.kind === "web" ? { host: target.host, rest: target.rest } : null;
}

/**
 * Which scan numbered a token, mirroring `webopen::Source`.
 *
 * The two lists are independent, so a token from one names a different address
 * in the other --- passing the wrong one here opens the wrong link rather than
 * failing, which is why it is a named type and not a boolean.
 */
export type WebLinkSource = "links" | "outline";

/** What {@link confirmAndOpen} needs from the world around it. */
export interface WebLinkDeps {
  /** Puts the question to the reader. */
  ask(address: WebAddress): Promise<boolean>;
  /** Invokes `open_web_link`. Rejects with the backend's own wording. */
  open(doc: number, source: WebLinkSource, token: number): Promise<void>;
  /** Reports a failure to the reader, in the status line. */
  onError(message: string): void;
}

/**
 * Asks, and opens only on a yes. Resolves with whether anything opened.
 *
 * **Separate from the dialog and from `App.svelte`, deliberately.** It is the
 * join between three things --- a question, a command, and what happens when
 * the command fails --- and every one of those has an answer that can be wrong:
 * opening without asking, asking and then opening the other list's token,
 * reporting a cancellation as an error. `AGENTS.md` records that logic living
 * only in `App.svelte` is logic no gate reaches, so it starts here with tests
 * rather than being extracted after something ships broken.
 *
 * A cancellation is **not** an error and says nothing. The reader dismissed a
 * dialog they raised; a status line reading "could not open" after that would
 * report their own decision back to them as a failure.
 */
export async function confirmAndOpen(
  doc: number,
  source: WebLinkSource,
  target: WebTarget,
  deps: WebLinkDeps,
): Promise<boolean> {
  const wanted = await deps.ask({ host: target.host, rest: target.rest });
  if (!wanted) return false;
  try {
    await deps.open(doc, source, target.token);
    return true;
  } catch (e) {
    // The backend's wording, not ours: it is the side that knows whether the
    // token named nothing or the operating system declined, and a sentence
    // composed here would have to guess between them. `String(e)` alone is
    // `[object Object]` for a structured refusal --- the trap of that name ---
    // so a `message` is preferred where there is one.
    const said =
      e && typeof e === "object" && "message" in e
        ? String((e as { message: unknown }).message)
        : String(e);
    deps.onError(said || "this link could not be opened");
    return false;
  }
}

/**
 * A modal asking whether to open an address.
 *
 * Built once and reused, like {@link import("./passworddialog").PasswordDialog},
 * and for the same reason: one question is outstanding at a time by
 * construction, so there is no promise left unsettled by a second `ask`.
 */
export class WebLinkDialog {
  private readonly backdrop: HTMLElement;
  private readonly panel: HTMLElement;
  private readonly host: HTMLElement;
  private readonly rest: HTMLElement;
  private readonly cancel: HTMLButtonElement;
  /** What to focus when it closes, so Escape does not lose the page. */
  private returnFocus: HTMLElement | null = null;
  /** Settles the outstanding `ask`, exactly once. */
  private pending: ((open: boolean) => void) | null = null;
  /**
   * Whether it is on screen, held here rather than read back off the element.
   *
   * The reason is `passworddialog.ts`'s, and the trap is in `docs/TRAPS.md`
   * under *reading a decision back out of the DOM makes the test double part of
   * the logic*.
   */
  private shown = false;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    this.panel = document.createElement("div");
    this.panel.setAttribute("role", "dialog");
    this.panel.setAttribute("aria-modal", "true");
    this.panel.setAttribute("aria-label", "Open this link?");
    this.panel.style.cssText =
      "margin-top:14vh;width:min(460px,92vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    const heading = document.createElement("h2");
    heading.style.cssText = "margin:0 0 0.6rem;font-size:15px;font-weight:600;";
    heading.textContent = "Open this link?";

    // The host, and it is the only thing in the panel drawn at size. Word
    // breaking is on because a long host must wrap rather than push the panel
    // wider than the reader's screen, where its end would be off the edge --- a
    // host whose tail cannot be seen is exactly the one worth reading.
    this.host = document.createElement("div");
    this.host.style.cssText =
      "font-size:15px;font-weight:600;overflow-wrap:anywhere;" +
      "font-family:ui-monospace,SFMono-Regular,Menlo,monospace;";

    this.rest = document.createElement("div");
    this.rest.style.cssText =
      "margin-top:0.15rem;opacity:0.66;overflow-wrap:anywhere;" +
      "font-family:ui-monospace,SFMono-Regular,Menlo,monospace;";

    const note = document.createElement("p");
    note.style.cssText = "margin:0.75rem 0 0;opacity:0.72;";
    // Ours, and fixed. Nothing the document said appears in this sentence.
    note.textContent =
      "This address is written in the document. It opens in your browser.";

    const buttons = document.createElement("div");
    buttons.style.cssText =
      "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.85rem;";
    this.cancel = this.button("Cancel", () => this.settle(false));
    const open = this.button("Open", () => this.settle(true));
    buttons.append(this.cancel, open);

    this.panel.append(heading, this.host, this.rest, note, buttons);
    this.backdrop.append(this.panel);
    host.append(this.backdrop);

    // A click on the backdrop itself, never one that bubbled out of the panel.
    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.settle(false);
    });
    this.backdrop.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.settle(false);
        return;
      }
      // Enter cancels, which is this dialog's whole posture --- see the header.
      // Stopped as well as defaulted-away: the window key handler binds Enter,
      // and a dismissal that also drove a command would be a second action the
      // reader did not ask for.
      //
      // The exception is Enter on a button, which is a reader who has tabbed to
      // it pressing the thing they are pointing at. That is not a reflex, and
      // intercepting it would make Open unreachable from the keyboard. Left to
      // the browser, which turns it into that button's own click.
      //
      // Duck-typed on `tagName`, not `instanceof HTMLElement`: the trap two
      // paragraphs above applies here too, and writing it correctly in `ask`
      // and wrongly here is how one of a pair rots.
      //
      // **Compared case-insensitively, and that is not defensiveness.** A
      // browser's `tagName` is upper case for an HTML element; `testdom.ts`
      // stores whatever `createElement` was passed, which is lower case here.
      // So `=== "BUTTON"` is right in the application and unreachable under
      // test, and `=== "button"` is the reverse --- either one gives a green
      // suite over a branch the tests never take. The first draft was the
      // former and the test caught it.
      if (event.key === "Enter") {
        const tag = (event.target as { tagName?: string } | null)?.tagName;
        if (tag?.toLowerCase() === "button") return;
        event.preventDefault();
        event.stopPropagation();
        this.settle(false);
      }
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.shown;
  }

  /**
   * Asks whether to open `address`, resolving `true` only on Open.
   *
   * Every other way out --- Cancel, Escape, a click on the backdrop, a second
   * `ask` --- resolves `false`. There is no third answer, because there is
   * nothing between opening and not.
   */
  ask(address: WebAddress): Promise<boolean> {
    // A second question dismisses the first rather than stacking on it.
    this.settle(false);

    this.host.textContent = address.host;
    this.rest.textContent = address.rest;

    if (!this.shown) {
      // Duck-typed rather than `instanceof HTMLElement`, for the reason
      // `passworddialog.ts` records: that form throws where the constructor
      // does not exist, rather than answering no.
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus =
        typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    // Cancel, not Open. See the header.
    this.cancel.focus();

    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding as a refusal. */
  close(): void {
    this.settle(false);
  }

  /**
   * Resolves the outstanding promise once and hides the dialog.
   *
   * `pending` is cleared *before* the resolve for `passworddialog.ts`'s reason:
   * a handler may call `ask` again synchronously, and settling into a promise
   * this has already forgotten is what keeps that from resolving the new one.
   */
  private settle(open: boolean): void {
    const pending = this.pending;
    this.pending = null;
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(open);
  }

  private button(label: string, onClick: () => void): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.style.cssText =
      "padding:0.35rem 0.9rem;font:inherit;border-radius:6px;" +
      "border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:ButtonFace;color:ButtonText;cursor:pointer;";
    button.addEventListener("click", onClick);
    return button;
  }
}
