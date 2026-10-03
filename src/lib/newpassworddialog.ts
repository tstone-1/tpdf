/**
 * The prompt for a new password, typed twice.
 *
 * `passworddialog.ts` asks for the password a document has; this asks for the
 * one a copy is about to get. What is acceptable is decided by `judge` in
 * `protect.ts`, and a refusal is shown here with the dialog left open, so a
 * mistyped repeat costs one field and not the whole command.
 *
 * The password lives in the two inputs and in the promise this resolves with.
 * Both fields are cleared on every close. Every string shown is this module's
 * or `protect.ts`'s, and the file's own name; all of it goes in through
 * `textContent`.
 */

import { ADVICE, NOT_ASCII, beyondAscii, judge } from "./protect";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-new-password";

/** A modal asking for a new password. Built once and reused. */
export class NewPasswordDialog {
  private readonly backdrop: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly note: HTMLElement;
  private readonly problem: HTMLElement;
  private readonly first: HTMLInputElement;
  private readonly second: HTMLInputElement;
  private returnFocus: HTMLElement | null = null;
  private pending: ((password: string | null) => void) | null = null;
  /** Held here rather than read back off the element; see `passworddialog.ts`. */
  private shown = false;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("aria-label", "New password");
    panel.style.cssText =
      "margin-top:14vh;width:min(420px,92vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0 0 0.35rem;font-size:15px;font-weight:600;";

    this.note = document.createElement("p");
    this.note.style.cssText = "margin:0 0 0.75rem;opacity:0.72;";

    this.first = this.field("New password");
    this.second = this.field("New password, again");
    this.second.style.marginTop = "0.5rem";

    this.problem = document.createElement("p");
    this.problem.setAttribute("role", "alert");
    this.problem.style.cssText = "margin:0.6rem 0 0;min-height:1.55em;";

    const buttons = document.createElement("div");
    buttons.style.cssText =
      "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.85rem;";
    const cancel = this.button("Cancel", () => this.settle(null));
    const save = this.button("Choose where to save...", () => this.submit());
    save.style.fontWeight = "600";
    buttons.append(cancel, save);

    panel.append(this.heading, this.note, this.first, this.second, this.problem, buttons);
    this.backdrop.append(panel);
    host.append(this.backdrop);

    this.first.addEventListener("input", () => this.hint());
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
      // Stopped as well as defaulted away: the window's key handler binds Enter.
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

  /** Asks for the password `name`'s copy will have, or `null` when dismissed. */
  ask(name: string): Promise<string | null> {
    this.settle(null);
    this.heading.textContent = `Password for a copy of ${name}`;
    this.note.textContent = ADVICE;
    this.problem.textContent = "";
    if (!this.shown) {
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus =
        typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    this.first.focus();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(null);
  }

  /** The note about Preview, while it applies and nothing worse is shown. */
  private hint(): void {
    this.problem.textContent = beyondAscii(this.first.value) ? NOT_ASCII : "";
  }

  private submit(): void {
    const why = judge(this.first.value, this.second.value);
    if (why) {
      this.problem.textContent = why;
      return;
    }
    this.settle(this.first.value);
  }

  /** Resolves once, clears both fields and hides the dialog. */
  private settle(password: string | null): void {
    const pending = this.pending;
    this.pending = null;
    this.first.value = "";
    this.second.value = "";
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(password);
  }

  private field(label: string): HTMLInputElement {
    const field = document.createElement("input");
    field.type = "password";
    field.setAttribute("aria-label", label);
    field.placeholder = label;
    field.style.cssText =
      "display:block;width:100%;box-sizing:border-box;padding:0.4rem 0.5rem;font:inherit;" +
      "border-radius:6px;border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:Field;color:FieldText;";
    return field;
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
