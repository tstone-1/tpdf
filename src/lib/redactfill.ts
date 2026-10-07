/**
 * The colour of the boxes a redaction draws over what it took.
 *
 * Black unless the reader chooses another, and the choice is kept between
 * sessions. `redaction_fill.rs` draws the boxes and holds the same three
 * names; the window sends the name and nothing else.
 *
 * **White is offered and is never the default.** On white paper a reader of
 * the copy cannot see that anything was taken out. That is sometimes what is
 * wanted, so it is not refused; the panel says what it means while it is the
 * choice, so it does not happen by accident.
 */

/** The three colours, by the names `redaction_fill::Fill` takes. */
export type Fill = "black" | "white" | "red";

/** Every colour, in the order the panel shows them. The first is the default. */
export const FILLS: readonly { fill: Fill; label: string; css: string }[] = [
  { fill: "black", label: "Black", css: "#000" },
  { fill: "white", label: "White", css: "#fff" },
  // The same red `redaction_fill.rs` writes: 0.83, 0.16, 0.16.
  { fill: "red", label: "Red", css: "#d42929" },
];

/** What a redaction is filled with when nothing was chosen. */
export const DEFAULT_FILL: Fill = "black";

/** Where the choice is kept. */
const FILL_KEY = "tpdf.redaction-fill";

type Store = Pick<Storage, "getItem" | "setItem">;

/** Whether a stored or received string is one of the three names. */
export function isFill(value: unknown): value is Fill {
  return FILLS.some((one) => one.fill === value);
}

/**
 * The colour the reader last chose, or black.
 *
 * Storage that throws, or that holds anything tpdf did not write, reads as
 * black: a redaction must not turn white because a stored value was damaged.
 */
export function readFill(storage: () => Store = () => window.localStorage): Fill {
  try {
    const kept = storage().getItem(FILL_KEY);
    return isFill(kept) ? kept : DEFAULT_FILL;
  } catch {
    return DEFAULT_FILL;
  }
}

/** Keeps the choice; `false` when storage refused, which changes nothing now. */
export function writeFill(fill: Fill, storage: () => Store = () => window.localStorage): boolean {
  try {
    storage().setItem(FILL_KEY, fill);
    return true;
  } catch {
    return false;
  }
}

/**
 * What the panel says under the swatches while `fill` is the choice, or the
 * empty string for nothing.
 */
export function fillNote(fill: Fill): string {
  return fill === "white"
    ? "White boxes cannot be seen on white paper: a reader of the copy will not see that anything was removed."
    : "";
}

/** What a {@link FillPicker} needs from whoever keeps the choice. */
export interface FillPickerOptions {
  /** The colour chosen when the panel opens. */
  current: Fill;
  /** Called with the new colour when the reader picks one. */
  onChange: (fill: Fill) => void;
}

/**
 * The three swatches, as one radio group, and the note under them.
 *
 * A radio group and not three buttons: exactly one is chosen at any time, and
 * a screen reader says "Black, 1 of 3, selected" for nothing.
 */
export class FillPicker {
  /** The element to put in a panel. */
  readonly element: HTMLElement;
  private readonly note: HTMLElement;
  private readonly radios = new Map<Fill, HTMLInputElement>();
  private chosen: Fill;

  constructor(private readonly opts: FillPickerOptions) {
    this.chosen = opts.current;
    this.element = document.createElement("div");
    this.element.style.cssText =
      "flex:none;padding:0.45rem 0.7rem;border-top:1px solid color-mix(in srgb, CanvasText 18%, transparent);";

    const group = document.createElement("div");
    group.setAttribute("role", "radiogroup");
    group.setAttribute("aria-label", "Colour of the boxes drawn over what is removed");
    group.style.cssText = "display:flex;align-items:center;gap:0.7rem;flex-wrap:wrap;";
    const caption = document.createElement("span");
    caption.textContent = "Box colour";
    caption.style.cssText = "opacity:0.75;";
    group.append(caption);

    for (const one of FILLS) {
      const label = document.createElement("label");
      label.style.cssText = "display:inline-flex;align-items:center;gap:0.3rem;cursor:pointer;";
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "tpdf-redaction-fill";
      radio.value = one.fill;
      radio.checked = one.fill === this.chosen;
      radio.addEventListener("change", () => {
        if (radio.checked) this.choose(one.fill);
      });
      const swatch = document.createElement("span");
      swatch.style.cssText =
        `width:0.95em;height:0.95em;border-radius:2px;background:${one.css};` +
        "border:1px solid color-mix(in srgb, CanvasText 45%, transparent);";
      const name = document.createElement("span");
      name.textContent = one.label;
      label.append(radio, swatch, name);
      group.append(label);
      this.radios.set(one.fill, radio);
    }

    this.note = document.createElement("div");
    this.note.setAttribute("role", "status");
    this.note.style.cssText = "margin-top:0.35rem;opacity:0.8;";
    this.element.append(group, this.note);
    this.paint();
  }

  /** The colour chosen now. */
  get fill(): Fill {
    return this.chosen;
  }

  /** What the note says now. For the tests. */
  get said(): string {
    return this.note.textContent ?? "";
  }

  /** Picks a colour as a click on its swatch does, and tells the owner. */
  choose(fill: Fill): void {
    if (fill === this.chosen) return;
    this.chosen = fill;
    this.paint();
    this.opts.onChange(fill);
  }

  private paint(): void {
    for (const [fill, radio] of this.radios) radio.checked = fill === this.chosen;
    const note = fillNote(this.chosen);
    this.note.textContent = note;
    this.note.style.display = note ? "" : "none";
  }
}
