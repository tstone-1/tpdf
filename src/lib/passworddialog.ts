/**
 * The prompt a locked document gets instead of an error.
 *
 * ## Why this exists at all
 *
 * Until now `open_document` answered an encrypted document with a sentence
 * saying tpdf could not ask for a password, and there was nowhere to type one.
 * A whole class of file --- every PDF behind a user password --- could be chosen
 * from the file dialog and then not opened, by any route. The backend had
 * diagnosed it correctly since the day `open_failure` was written; what was
 * missing was the half that lets a reader answer.
 *
 * ## A locked document is not a broken one
 *
 * That distinction is carried the whole way rather than reconstructed here:
 * `progressive::Refusal` has a `locked` field, `Response` has one beside
 * `abandoned`, and the Tauri command serialises `{reason, locked}`. So this
 * dialog is shown on a *flag*, never on a string match against a message ---
 * which is what would rot the first time the wording changed.
 *
 * ## The password never becomes state
 *
 * It lives in the input element and in the promise this resolves with, and the
 * field is cleared on every close, whether the reader submitted or dismissed.
 * Nothing here stores it, and nothing logs it. The one place it is *held* is
 * `Held::password` in the app process, which needs it to build the document's
 * later workers --- see that field's comment for what that costs.
 *
 * ## Document text does not reach here
 *
 * The only strings shown are the file's base name, which the reader chose, and
 * a refusal worded in `progressive.rs`. Neither comes out of the document, so
 * `docs/THREAT-MODEL.md` T8 is not being leaned on --- but everything still
 * arrives through `textContent`, and no element built here carries a
 * URL-bearing attribute, because the `sinks` gate is what enforces that rather
 * than this paragraph.
 */

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-password";

/**
 * A modal asking for a document's password.
 *
 * Built once and reused, like the palette and the properties dialog. One
 * question is outstanding at a time by construction: a second `ask` while one is
 * open dismisses the first, because two prompts for one document would leave a
 * promise nobody resolves.
 */
export class PasswordDialog {
  private readonly backdrop: HTMLElement;
  private readonly panel: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly note: HTMLElement;
  private readonly field: HTMLInputElement;
  /** What to focus when it closes, so Escape does not lose the page. */
  private returnFocus: HTMLElement | null = null;
  /** Settles the outstanding `ask`, exactly once. */
  private pending: ((password: string | null) => void) | null = null;
  /**
   * Whether it is on screen, held here rather than read back off the element.
   *
   * `propertiesdialog.ts` answers the same question with
   * `backdrop.style.display !== "none"`, and that is the shape `docs/TRAPS.md`
   * records under *reading a decision back out of the DOM makes the test double
   * part of the logic* --- right in the browser, wrong under test, which is the
   * worse direction. It set `display` through `cssText`, so a double that stores
   * assignments verbatim reports `undefined` and every question reads as open.
   * The state is ours, so it is kept as ours; `display` is still set, because
   * that is what the browser draws.
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
    this.panel.setAttribute("aria-label", "Password required");
    this.panel.style.cssText =
      "margin-top:14vh;width:min(420px,92vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0 0 0.35rem;font-size:15px;font-weight:600;";

    this.note = document.createElement("p");
    this.note.style.cssText = "margin:0 0 0.75rem;opacity:0.72;";

    this.field = document.createElement("input");
    // Through the property rather than `setAttribute`, which is how everything
    // else in this file sets one, and which the `sinks` gate is happier with.
    this.field.type = "password";
    this.field.setAttribute("aria-label", "Password");
    this.field.style.cssText =
      "width:100%;box-sizing:border-box;padding:0.4rem 0.5rem;font:inherit;" +
      "border-radius:6px;border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:Field;color:FieldText;";

    const buttons = document.createElement("div");
    buttons.style.cssText =
      "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.85rem;";
    const cancel = this.button("Cancel", () => this.settle(null));
    const unlock = this.button("Unlock", () => this.submit());
    unlock.style.fontWeight = "600";
    buttons.append(cancel, unlock);

    this.panel.append(this.heading, this.note, this.field, buttons);
    this.backdrop.append(this.panel);
    host.append(this.backdrop);

    // A click on the backdrop itself, never one that bubbled out of the panel.
    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.settle(null);
    });
    this.backdrop.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.settle(null);
        return;
      }
      // Enter submits from the field, which is where the reader's hands are.
      // Stopped rather than merely defaulted-away: the window key handler binds
      // Enter, and a submit that also drove a command would be a second action
      // the reader did not ask for.
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        this.submit();
      }
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.shown;
  }

  /**
   * Asks for `name`'s password, resolving with what was typed or `null`.
   *
   * `problem` is the backend's own wording --- which sentence it is says whether
   * a password has already been tried, and that is a fact only the worker that
   * tried it has.
   *
   * An empty answer resolves as `null` rather than as `""`. They are different
   * to PDFium --- an empty *user* password is what most permission-restricted
   * documents carry, and it opens with no prompt --- but a reader who presses
   * Unlock on an empty field has not supplied one, and sending `""` would retry
   * exactly the attempt that already failed.
   */
  ask(name: string, problem: string): Promise<string | null> {
    // A second question dismisses the first rather than stacking on it. Nothing
    // issues two today; what this rules out is a promise nobody settles.
    this.settle(null);

    this.heading.textContent = name;
    this.note.textContent = problem;
    this.field.value = "";

    if (!this.shown) {
      // Duck-typed rather than `instanceof HTMLElement`, which is how
      // `palette.ts` and `propertiesdialog.ts` write it. That form is correct in
      // a browser and *throws* where the constructor does not exist ---
      // `docs/TRAPS.md`: *`instanceof` against a constructor the runner does not
      // have throws, it does not answer no*. Neither of those files is reached
      // by a test through this line; this one is, and the first run of that test
      // is what found it.
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus =
        typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    this.field.focus();

    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(null);
  }

  private submit(): void {
    this.settle(this.field.value || null);
  }

  /**
   * Resolves the outstanding promise once and hides the dialog.
   *
   * `pending` is cleared *before* the resolve, not after: a handler waiting on
   * it may call `ask` again synchronously, and settling into a promise this has
   * already forgotten is what keeps that from resolving the new one.
   */
  private settle(password: string | null): void {
    const pending = this.pending;
    this.pending = null;
    // Cleared whichever way this went. The value is the secret, and leaving it
    // in a detached-but-live input is the one place it would outlive its use.
    this.field.value = "";
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(password);
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
