/**
 * The dialog for a smaller copy.
 *
 * Four ready choices and the reader's own numbers. Each ready choice is shown
 * with the size it comes to, worked out when the dialog opens and filled in
 * as each answer arrives; the selected one also shows a part of a page as it
 * is and as the copy would draw it. The reader's own numbers are worked out
 * when a field is left or a box is ticked, not on every keystroke: an
 * estimate reads every picture in the document.
 *
 * Saving is offered only for a choice whose estimate says the copy is
 * smaller. What is asked of the backend and what is said about the answer
 * are `compress.ts`'s; every string goes in through `textContent`, and the
 * two pictures are `data:` URLs the backend made.
 */

import {
  CHOICES,
  CUSTOM_START,
  caption,
  custom,
  isSmaller,
  outcome,
  summary,
  type Pictures,
  type Shrinkage,
} from "./compress";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-compress";

/** Works out what a copy made this way comes to. `null` changes no picture. */
export type Estimator = (pictures: Pictures | null) => Promise<Shrinkage>;

/** What the reader chose, and the estimate it was chosen on. */
export interface Chosen {
  pictures: Pictures | null;
  shrinkage: Shrinkage;
}

/**
 * Whether a string is what the backend sends for a sample: a PNG as a `data:`
 * URL in base64. Anything else is not put in an `img`.
 */
export function isPicture(url: string): boolean {
  return /^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(url);
}

/** The id of the row for the reader's own numbers. */
const CUSTOM = "custom";

const WORKING = "Working it out...";
const FAILED = "Could not be worked out";

/** One row's state: not asked yet, asked, answered, or failed with a reason. */
type Known = { shrinkage: Shrinkage } | { problem: string } | "working" | null;

/** A modal offering the ways to make a copy smaller. Built once and reused. */
export class CompressDialog {
  private readonly backdrop: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly radios = new Map<string, HTMLInputElement>();
  private readonly outcomes = new Map<string, HTMLElement>();
  private readonly dpi: HTMLInputElement;
  private readonly quality: HTMLInputElement;
  private readonly jpeg: HTMLInputElement;
  private readonly said: HTMLElement;
  private readonly preview: HTMLElement;
  private readonly before: HTMLElement;
  private readonly after: HTMLElement;
  private readonly under: HTMLElement;
  private readonly problem: HTMLElement;
  private readonly save: HTMLButtonElement;
  private returnFocus: HTMLElement | null = null;
  private pending: ((chosen: Chosen | null) => void) | null = null;
  private estimate: Estimator | null = null;
  private shown = false;
  private selected = "keep";
  private known = new Map<string, Known>();
  /** The numbers the custom row's estimate is for. */
  private customFor: Pictures | null = null;
  /** Counts every opening and every custom estimate, so a late answer is dropped. */
  private run = 0;
  /** One estimate at a time: each reads every picture in the document. */
  private queue: Promise<void> = Promise.resolve();

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;overflow:auto;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("aria-label", "Save a smaller copy");
    panel.style.cssText =
      "margin:6vh 0;width:min(720px,94vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0 0 0.5rem;font-size:15px;font-weight:600;";

    const list = document.createElement("div");
    list.setAttribute("role", "radiogroup");
    list.setAttribute("aria-label", "How much smaller");
    for (const choice of CHOICES) {
      list.append(this.row(choice.id, choice.title, choice.note));
    }
    this.dpi = this.number("Pixels an inch", String(CUSTOM_START.dpi), "5.5em");
    this.quality = this.number("JPEG quality, 1 to 100", String(CUSTOM_START.quality), "4.5em");
    this.jpeg = document.createElement("input");
    this.jpeg.type = "checkbox";
    this.jpeg.checked = CUSTOM_START.jpeg;
    this.jpeg.setAttribute("aria-label", "Store photographs as JPEG");
    const fields = document.createElement("span");
    fields.style.cssText = "display:inline-flex;gap:0.6rem;align-items:center;flex-wrap:wrap;";
    fields.append(
      this.labelled("Pixels an inch", this.dpi),
      this.labelled("JPEG quality", this.quality),
      this.labelled("Store photographs as JPEG", this.jpeg, true),
    );
    const own = this.row(CUSTOM, "Your own numbers", "");
    own.append(fields);
    list.append(own);

    this.said = document.createElement("p");
    this.said.style.cssText = "margin:0.7rem 0 0;font-weight:600;";

    this.preview = document.createElement("div");
    this.preview.style.cssText = "display:none;margin-top:0.6rem;";
    const pair = document.createElement("div");
    pair.style.cssText = "display:flex;gap:8px;";
    this.before = this.picture("The page as it is now");
    this.after = this.picture("The page as the copy draws it");
    pair.append(this.before, this.after);
    this.under = document.createElement("p");
    this.under.style.cssText = "margin:0.35rem 0 0;opacity:0.72;";
    this.preview.append(pair, this.under);

    this.problem = document.createElement("p");
    this.problem.setAttribute("role", "alert");
    this.problem.style.cssText = "margin:0.6rem 0 0;min-height:1.55em;";

    const buttons = document.createElement("div");
    buttons.style.cssText =
      "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.6rem;";
    const cancel = this.button("Cancel", () => this.settle(null));
    this.save = this.button("Choose where to save...", () => this.submit());
    this.save.style.fontWeight = "600";
    buttons.append(cancel, this.save);

    panel.append(this.heading, list, this.said, this.preview, this.problem, buttons);
    this.backdrop.append(panel);
    host.append(this.backdrop);

    for (const field of [this.dpi, this.quality, this.jpeg]) {
      field.addEventListener("change", () => {
        this.select(CUSTOM);
        this.askCustom();
      });
    }
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

  /**
   * Offers the ways to make `name` smaller, each with what `estimate` says it
   * comes to. Resolves with the choice, or `null` when dismissed.
   */
  ask(name: string, estimate: Estimator): Promise<Chosen | null> {
    this.settle(null);
    this.run += 1;
    this.estimate = estimate;
    this.known = new Map();
    this.customFor = null;
    // A fresh line: an estimate still running for the opening before this one
    // must not hold these back, and its answer is dropped by `run`.
    this.queue = Promise.resolve();
    this.heading.textContent = `A smaller copy of ${name}`;
    this.dpi.value = String(CUSTOM_START.dpi);
    this.quality.value = String(CUSTOM_START.quality);
    this.jpeg.checked = CUSTOM_START.jpeg;
    if (!this.shown) {
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus =
        typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    this.select("keep");
    for (const choice of CHOICES) this.work(choice.id, choice.pictures);
    this.radios.get("keep")?.focus();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(null);
  }

  /** Asks for one row's estimate, after the ones already asked for. */
  private work(id: string, pictures: Pictures | null): void {
    const estimate = this.estimate;
    if (!estimate) return;
    const run = this.run;
    this.known.set(id, "working");
    this.show();
    this.queue = this.queue.then(async () => {
      // Closed, opened again, or the numbers changed while this waited.
      if (run !== this.run || !this.shown) return;
      let answer: Known;
      try {
        answer = { shrinkage: await estimate(pictures) };
      } catch (error) {
        answer = { problem: String(error) };
      }
      if (run !== this.run || !this.shown) return;
      this.known.set(id, answer);
      this.show();
    });
  }

  /** Works out the reader's own numbers, when they are numbers. */
  private askCustom(): void {
    const read = custom(this.dpi.value, this.quality.value, this.jpeg.checked);
    if ("problem" in read) {
      this.customFor = null;
      this.known.set(CUSTOM, { problem: read.problem });
      this.show();
      return;
    }
    const same =
      this.customFor !== null &&
      this.customFor.dpi === read.pictures.dpi &&
      this.customFor.quality === read.pictures.quality &&
      this.customFor.jpeg === read.pictures.jpeg;
    if (same && this.known.get(CUSTOM)) return;
    this.customFor = read.pictures;
    // A new run for this row only: the ready rows keep their answers, and an
    // answer for the numbers before these is dropped when it arrives.
    const estimate = this.estimate;
    if (!estimate) return;
    const pictures = read.pictures;
    this.known.set(CUSTOM, "working");
    this.show();
    const run = this.run;
    this.queue = this.queue.then(async () => {
      if (run !== this.run || !this.shown || this.customFor !== pictures) return;
      let answer: Known;
      try {
        answer = { shrinkage: await estimate(pictures) };
      } catch (error) {
        answer = { problem: String(error) };
      }
      if (run !== this.run || !this.shown || this.customFor !== pictures) return;
      this.known.set(CUSTOM, answer);
      this.show();
    });
  }

  private select(id: string): void {
    this.selected = id;
    for (const [row, radio] of this.radios) radio.checked = row === id;
    if (id === CUSTOM && !this.known.get(CUSTOM)) this.askCustom();
    this.show();
  }

  /** Puts what is known on screen: each row's outcome, and the selected row's detail. */
  private show(): void {
    for (const [id, line] of this.outcomes) {
      const known = this.known.get(id) ?? null;
      line.textContent =
        known === null
          ? ""
          : known === "working"
            ? WORKING
            : "problem" in known
              ? FAILED
              : outcome(known.shrinkage);
    }
    const known = this.known.get(this.selected) ?? null;
    const shrinkage = known && known !== "working" && "shrinkage" in known ? known.shrinkage : null;
    this.said.textContent =
      known === "working" ? WORKING : shrinkage ? summary(shrinkage) : "";
    this.problem.textContent =
      known && known !== "working" && "problem" in known ? known.problem : "";
    const sample = shrinkage?.sample ?? null;
    if (sample && isPicture(sample.before) && isPicture(sample.after)) {
      // webview-sink-ok: a PNG `data:` URL the backend encoded from pixels it rendered, checked by `isPicture`; never a document URL.
      this.before.setAttribute("src", sample.before);
      // webview-sink-ok: as the line above.
      this.after.setAttribute("src", sample.after);
      this.under.textContent = caption(sample);
      this.preview.style.display = "block";
    } else {
      this.before.removeAttribute("src");
      this.after.removeAttribute("src");
      this.under.textContent = "";
      this.preview.style.display = "none";
    }
    this.save.disabled = !(shrinkage && isSmaller(shrinkage));
  }

  private chosen(): Chosen | null {
    const known = this.known.get(this.selected) ?? null;
    if (!known || known === "working" || !("shrinkage" in known)) return null;
    if (!isSmaller(known.shrinkage)) return null;
    if (this.selected === CUSTOM) {
      return this.customFor ? { pictures: this.customFor, shrinkage: known.shrinkage } : null;
    }
    const choice = CHOICES.find((choice) => choice.id === this.selected);
    return choice ? { pictures: choice.pictures, shrinkage: known.shrinkage } : null;
  }

  private submit(): void {
    const chosen = this.chosen();
    if (chosen) this.settle(chosen);
  }

  /** Resolves once and hides the dialog. */
  private settle(chosen: Chosen | null): void {
    const pending = this.pending;
    this.pending = null;
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      // The pictures are of the reader's document: not kept once it is closed.
      this.known = new Map();
      this.before.removeAttribute("src");
      this.after.removeAttribute("src");
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(chosen);
  }

  /** One choice: a radio, its title, what it comes to, and a note under it. */
  private row(id: string, title: string, note: string): HTMLElement {
    const row = document.createElement("label");
    row.style.cssText =
      "display:grid;grid-template-columns:auto 1fr auto;column-gap:0.5rem;" +
      "align-items:baseline;padding:0.25rem 0;cursor:pointer;";
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "tpdf-compress-choice";
    radio.value = id;
    radio.addEventListener("change", () => this.select(id));
    const name = document.createElement("span");
    name.textContent = title;
    const comes = document.createElement("span");
    comes.style.cssText = "opacity:0.8;font-variant-numeric:tabular-nums;";
    row.append(radio, name, comes);
    if (note) {
      const under = document.createElement("span");
      under.textContent = note;
      under.style.cssText = "grid-column:2 / span 2;opacity:0.72;";
      row.append(under);
    }
    this.radios.set(id, radio);
    this.outcomes.set(id, comes);
    return row;
  }

  private number(label: string, value: string, width: string): HTMLInputElement {
    const field = document.createElement("input");
    field.type = "text";
    field.inputMode = "numeric";
    field.value = value;
    field.setAttribute("aria-label", label);
    field.style.cssText =
      `width:${width};box-sizing:border-box;padding:0.2rem 0.4rem;font:inherit;` +
      "border-radius:6px;border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:Field;color:FieldText;";
    return field;
  }

  private labelled(text: string, field: HTMLElement, after = false): HTMLElement {
    const label = document.createElement("label");
    label.style.cssText = "display:inline-flex;gap:0.3rem;align-items:center;";
    const words = document.createElement("span");
    words.textContent = text;
    label.append(...(after ? [field, words] : [words, field]));
    return label;
  }

  private picture(label: string): HTMLElement {
    // webview-sink-ok: its `src` is only ever a PNG `data:` URL; see `show`.
    const picture = document.createElement("img");
    picture.setAttribute("alt", label);
    picture.style.cssText =
      "display:block;width:calc(50% - 4px);height:auto;image-rendering:auto;" +
      "border:1px solid color-mix(in srgb, CanvasText 18%, transparent);";
    return picture;
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
