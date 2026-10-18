/**
 * The dialog for *Save a copy with page numbers...* and *Save a copy with a
 * watermark...*: the text, where it goes, how big, and on which pages.
 *
 * What is acceptable is decided by `judge` in `addtext.ts`, and a refusal is
 * shown here with the dialog left open, so a typing mistake costs one field and
 * not the whole command. Every string goes in through `textContent`; the text a
 * reader types is a value of an input, never markup.
 */

import { defaults, judge, POSITION_TITLES, POSITIONS, type AddedText, type Kind, type Position } from "./addtext";
import { button, footer, paragraph, row, section } from "./dialogparts";

/** Class on the dialog, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-add-text";

/** What each kind asks, beside the text and the position. */
const WORDING: Record<Kind, { title: string; intro: string; extra: string; save: string }> = {
  numbers: {
    title: "Save a copy with page numbers",
    intro: "{page} is each page's number and {pages} how many pages are numbered. Add words around them for a header or a footer.",
    extra: "First number",
    save: "Choose where to save...",
  },
  watermark: {
    title: "Save a copy with a watermark",
    intro: "The text is written across each page, turned and see-through, over what is on it.",
    extra: "Opacity, percent",
    save: "Choose where to save...",
  },
};

export class AddTextDialog {
  private readonly dialog: HTMLDialogElement;
  private readonly heading: HTMLElement;
  private readonly intro: HTMLElement;
  private readonly text: HTMLInputElement;
  private readonly position: HTMLSelectElement;
  private readonly size: HTMLInputElement;
  private readonly extra: HTMLInputElement;
  private readonly extraLabel: HTMLElement;
  private readonly pages: HTMLInputElement;
  private readonly problem: HTMLElement;
  private readonly save: HTMLButtonElement;
  private kind: Kind = "numbers";
  private count = 1;
  private pending: ((added: AddedText | null) => void) | null = null;
  private returnFocus: HTMLElement | null = null;

  constructor(host: HTMLElement) {
    this.dialog = document.createElement("dialog");
    this.dialog.className = `${DIALOG_CLASS} tpdf-dialog`;
    this.heading = document.createElement("h2");
    this.intro = paragraph("tpdf-dialog-intro", "");

    this.text = this.input("text", "Text");
    const words = section("Text");
    words.append(this.text);

    this.position = document.createElement("select");
    this.position.setAttribute("aria-label", "Position");
    for (const position of POSITIONS) {
      const option = document.createElement("option");
      option.value = position;
      option.textContent = POSITION_TITLES[position];
      this.position.append(option);
    }
    this.size = this.input("text", "Size, points");
    this.size.inputMode = "decimal";
    this.size.style.width = "5em";
    this.extra = this.input("text", "");
    this.extra.inputMode = "decimal";
    this.extra.style.width = "5em";
    this.extraLabel = document.createElement("span");
    const place = section("Placing");
    const line = row();
    line.append(this.labelled("Position", this.position), this.labelled("Size, points", this.size));
    const second = document.createElement("label");
    second.append(this.extraLabel, this.extra);
    second.style.cssText = "display:inline-flex;gap:6px;align-items:center;";
    line.append(second);
    place.append(line);

    this.pages = this.input("text", "Pages");
    this.pages.placeholder = "Every page";
    const which = section("Pages");
    which.append(this.pages, paragraph("tpdf-dialog-hint", "Leave empty for every page, or type 2-10 or 1,3,5."));

    this.problem = paragraph("tpdf-dialog-hint", "");
    this.problem.setAttribute("role", "alert");
    this.problem.style.minHeight = "1.45em";

    const cancel = button("Cancel");
    this.save = button("", true);
    cancel.addEventListener("click", () => this.settle(null));
    this.save.addEventListener("click", () => this.submit());

    this.dialog.append(this.heading, this.intro, words, place, which, this.problem, footer(cancel, this.save));
    this.dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.settle(null); });
    this.dialog.addEventListener("keydown", (event) => {
      // The window's key handler binds Enter and Escape; both are this dialog's.
      event.stopPropagation();
      // Handled here as well as by the dialog's own `cancel`, which a key
      // event the page sends does not raise.
      if (event.key === "Escape") {
        event.preventDefault();
        this.settle(null);
        return;
      }
      if (event.key === "Enter" && event.target !== this.position) {
        event.preventDefault();
        this.submit();
      }
    });
    for (const field of [this.text, this.size, this.extra, this.pages]) {
      field.addEventListener("input", () => { this.problem.textContent = ""; });
    }
    host.append(this.dialog);
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.pending !== null;
  }

  /**
   * Asks what to write on a copy of a document of `pageCount` pages, or
   * `null` when dismissed. Starts from the defaults every time.
   */
  ask(kind: Kind, pageCount: number): Promise<AddedText | null> {
    this.settle(null);
    this.kind = kind;
    this.count = pageCount;
    const start = defaults(kind);
    const wording = WORDING[kind];
    this.heading.textContent = wording.title;
    this.dialog.setAttribute("aria-label", wording.title);
    this.intro.textContent = wording.intro;
    this.text.value = start.text;
    this.position.value = start.position;
    this.size.value = String(start.size);
    this.extraLabel.textContent = wording.extra;
    this.extra.setAttribute("aria-label", wording.extra);
    this.extra.value = kind === "numbers" ? String(start.first_number) : String(Math.round(start.opacity * 100));
    this.pages.value = "";
    this.problem.textContent = "";
    this.save.textContent = wording.save;
    const active = document.activeElement as { focus?: () => void } | null;
    this.returnFocus = typeof active?.focus === "function" ? (active as HTMLElement) : null;
    this.dialog.showModal();
    this.text.focus();
    this.text.select();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer. */
  close(): void {
    this.settle(null);
  }

  private submit(): void {
    const judged = judge(this.kind, {
      text: this.text.value,
      position: this.position.value as Position,
      size: this.size.value,
      extra: this.extra.value,
      pages: this.pages.value,
    }, this.count);
    if (typeof judged === "string") {
      this.problem.textContent = judged;
      return;
    }
    this.settle(judged);
  }

  private settle(added: AddedText | null): void {
    const pending = this.pending;
    this.pending = null;
    if (this.dialog.open) this.dialog.close();
    if (pending) {
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(added);
  }

  private input(type: string, label: string): HTMLInputElement {
    const field = document.createElement("input");
    field.type = type;
    if (label) field.setAttribute("aria-label", label);
    field.spellcheck = false;
    return field;
  }

  private labelled(label: string, control: HTMLElement): HTMLLabelElement {
    const wrap = document.createElement("label");
    wrap.style.cssText = "display:inline-flex;gap:6px;align-items:center;";
    const words = document.createElement("span");
    words.textContent = label;
    wrap.append(words, control);
    return wrap;
  }
}
