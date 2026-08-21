/**
 * The marks tab: everything the reader has marked in this document.
 *
 * `docs/PLAN.md` §8 asked for four sidebar tabs and got them; this is a fifth,
 * and the reason it is not one of the four is that when they were written the
 * reader could not mark anything. Nine kinds later they can, and until this
 * panel existed there was no way to see what they had done: a highlight is a
 * wash, a note is a 24-point icon, and neither shows a word of what was typed
 * on it. The same argument `commentlist.ts` makes about somebody else's
 * annotations, made about the reader's own.
 *
 * ## It is not the comments panel with a different source
 *
 * Two differences decide almost everything here.
 *
 * **This one is about live state, not about a file that was read once.**
 * `annots.rs` scans a document and the answer stands until it is reopened, so
 * that panel has three states --- reading, none, unreadable. Marks come from the
 * model in this process, which answers immediately and cannot fail, so there are
 * two: some, or none. There is nothing to say "still reading" about, and a
 * placeholder that said it would be a lie a reader could sit and watch.
 *
 * **A row's order has an owner already.** `markWalk` in `pages.ts` decides which
 * mark the keyboard walk meets next, and this list must agree with it, so
 * `markRows` wraps that function rather than sorting again. The panel is one of
 * two ways to the same marks and a reader will use both in the same minute.
 *
 * ## The nine kinds are named once
 *
 * `nameOf` comes from `markpopup.ts`, which is where the reader's word for each
 * kind is chosen and argued for. A table here would be a second one, and the
 * failure would be a mark called an Ellipse in the panel and a Circle in the box
 * that opens when you press it.
 *
 * ## Nothing here builds markup from a string somebody typed
 *
 * A note is the reader's own until the document is saved and reopened, at which
 * point it is whatever was in the file --- so it is treated as document text
 * throughout: `textContent` and nothing else. `docs/THREAT-MODEL.md` T8, and
 * `scripts/check_webview_sinks.py` is the gate.
 *
 * The words a mark *covers* need no such argument: they are the document's from
 * the first frame, lifted off a page by `selectionQuadsByPage`. They take the
 * same route as the note and for the same reason, which is why {@link rowLine}
 * hands both to one `textContent` rather than treating one as trusted.
 */

import { cssColor } from "./markcolors";
import { nameOf } from "./markpopup";
import type { MarkRow } from "./pages";

export interface MarkListOptions {
  /** Called when a row is activated, with the mark's id. */
  onPick: (id: number) => void;
  /**
   * Called when a row's remove control is used, with the mark's id.
   *
   * **This is the one place a mark is named by anything but the open note**,
   * and `App.svelte`'s `removeMark` says why that rule exists: two ways to say
   * which mark a command means is how they come to disagree. The exception is
   * not a convenience, it is the only route for part of this list. A mark whose
   * `page` is `null` cannot be opened --- there is no page to show it on --- so
   * every existing removal path, which goes through the box the note opens in,
   * cannot reach it. Listing a mark that can never be taken off is worse than
   * not listing it.
   */
  onRemove: (id: number) => void;
  /**
   * The words a mark covers, by id, or `""` where none were recorded.
   *
   * A lookup rather than a field on {@link MarkRow}, because this is not part
   * of the mark: nothing in the model holds it and no byte of a saved file
   * carries it. It is what the selection said at the moment the mark was made,
   * kept by whoever made it --- see `App.svelte`'s `markSelection`.
   *
   * Empty for every kind that does not cover words, and empty for a mark read
   * back out of a file. Neither is an error and neither is announced: the row
   * falls back to saying nothing was typed on it, which is what it said before
   * this existed.
   */
  coveredFor: (id: number) => string;
}

/** Side of the colour swatch, in CSS pixels. */
const SWATCH = 9;

/** The panel's line for a row nobody could place, or "" when they all were. */
export function noticeFor(rows: readonly MarkRow[]): string {
  const lost = rows.filter((row) => row.page === null).length;
  if (lost === 0) return "";
  return lost === 1
    ? "1 mark is not on any page."
    : `${lost} marks are not on any page.`;
}

/**
 * One line of whatever was handed in.
 *
 * A row is one line high and a text box's note has real newlines in it --- the
 * same treatment `comments.ts` gives a comment body, for the same reason. Both
 * of {@link rowLine}'s candidates go through it: the words a mark covers run
 * over the lines of the page they were taken from, so they arrive with exactly
 * the same problem.
 */
function flatten(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * What a row's first line says, and whether the reader wrote it.
 *
 * Three cases in one order, and the order is the argument. **What the reader
 * typed wins**, always: a note is the one thing on a row they chose, and a
 * highlight noted "check this against §4" must not be listed by the sentence it
 * sits on. Where they typed nothing, the words the mark covers are what they
 * would recognise it by --- which is the whole point, because the alternative is
 * a column of nine rows all reading "No note". And where there are neither, the
 * row says so as it always did.
 *
 * `own` is false for both fallbacks, and the row draws it the way it already
 * drew "No note": dimmed and italic. That is not decoration --- it is the only
 * thing separating a sentence the reader wrote from a sentence the document
 * did, in a panel whose subject is what the reader has done.
 */
export function rowLine(
  note: string,
  covered: string,
): { text: string; own: boolean } {
  const typed = flatten(note);
  if (typed) return { text: typed, own: true };
  const words = flatten(covered);
  if (words) return { text: words, own: false };
  return { text: "No note", own: false };
}

/** The marks panel: a row per mark the reader made, in walk order. */
export class MarkList {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: MarkListOptions;

  private rows: MarkRow[] = [];
  private readonly elements = new Map<number, HTMLElement>();
  /** Id of the row the roving tabindex is on, or null. */
  private focused: number | null = null;
  /** Id of the row shown as selected, or null. */
  private selected: number | null = null;
  /** What the notice last said, so an unchanged paint writes nothing. */
  private said = "";

  constructor(host: HTMLElement, opts: MarkListOptions) {
    this.opts = opts;

    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText =
      "flex:none;padding:0.3rem 0.7rem;opacity:0.7;display:none;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.setAttribute("aria-label", "Marks");
    this.list.style.cssText = "flex:1;min-height:0;overflow-y:auto;";
    this.list.addEventListener("keydown", this.onKeyDown);
    // Focus can arrive without going through `focus(id)`, and a roving tabindex
    // that does not follow it aims every later key at the wrong row. See the
    // trap about a mirror of the DOM's focus going stale.
    this.list.addEventListener("focusin", (event) => {
      const id = idOf(event.target);
      if (id !== null) this.focus(id);
    });

    host.append(this.notice, this.list);
    this.paint();
  }

  /**
   * Rows currently drawn. For the check harness and the tests.
   *
   * **Counted out of the list element, not off `this.rows`.** The obvious
   * version returns the rows the panel was *given*, which is the same number
   * whether or not a single element was built --- a mutation that drew one row
   * and stopped survived the window check written to catch exactly that, and it
   * survived because this getter agreed with the input rather than with the
   * page. Same trap as `rowText` below, which is why that one reads the DOM.
   */
  get rowCount(): number {
    return [...this.list.children].filter(
      (child) => (child as HTMLElement).dataset?.id !== undefined,
    ).length;
  }

  /** What the panel says above the list. For the check harness and the tests. */
  get status(): string {
    return this.said;
  }

  /** Id of the selected row, or -1. For the check harness and the tests. */
  get selectedId(): number {
    return this.selected ?? -1;
  }

  /** Id of the row holding the roving tabindex, or -1. For the tests. */
  get focusedId(): number {
    return this.focused ?? -1;
  }

  /** A row element, so the check harness can press it. */
  elementFor(id: number): HTMLElement | null {
    return this.elements.get(id) ?? null;
  }

  /**
   * What a row displays, read back out of the DOM. For the check harness.
   *
   * Read back rather than reported from the mark it was built from, for the
   * reason `commentlist.ts` and `results.ts` both give: a getter answering from
   * the source agrees with itself whatever the row actually contains.
   *
   * Walked by position rather than by selector, because the fake DOM the unit
   * tests run against matches selectors by tag name only and computes no
   * aggregate `textContent` --- a selector-based reader returns "" there, which
   * is exactly what an empty row returns.
   */
  rowText(id: number): {
    note: string;
    kind: string;
    page: string;
    own: boolean;
  } {
    const row = this.elements.get(id);
    if (!row) return { note: "", kind: "", page: "", own: false };
    const [, page, text] = [...row.children] as HTMLElement[];
    const [note, kind] = [...(text?.children ?? [])] as HTMLElement[];
    return {
      note: note?.textContent ?? "",
      kind: kind?.textContent ?? "",
      page: page?.textContent ?? "",
      // Whether the first line is what the reader typed --- see {@link rowLine}.
      own: note?.dataset?.own === "yes",
    };
  }

  /**
   * Replaces the marks shown.
   *
   * One state rather than the comments panel's three: the model is in this
   * process and answers every time, so an empty list means the reader has
   * marked nothing rather than that an answer is outstanding.
   */
  setMarks(rows: readonly MarkRow[]): void {
    this.rows = [...rows];
    if (this.selected !== null && !this.rows.some((row) => row.mark.id === this.selected)) {
      // The mark the box was open on has been removed or undone away. Dropping
      // the selection here rather than waiting to be told is what keeps a
      // repainted list from marking whichever row inherits the id.
      this.selected = null;
    }
    this.paint();
  }

  /** Marks one mark as the selected row and scrolls it into view. */
  select(id: number | null): void {
    if (this.selected === id) return;
    if (this.selected !== null) this.mark(this.selected, false);
    this.selected = id;
    if (id === null) return;
    this.mark(id, true);
    this.focus(id);
    this.elements.get(id)?.scrollIntoView({ block: "nearest" });
  }

  private mark(id: number, on: boolean): void {
    const element = this.elements.get(id);
    if (!element) return;
    element.setAttribute("aria-selected", String(on));
    element.style.background = on
      ? "color-mix(in srgb, currentColor 12%, transparent)"
      : "";
  }

  private paint(): void {
    this.list.replaceChildren();
    this.elements.clear();

    this.say(noticeFor(this.rows));

    if (this.rows.length === 0) {
      this.list.appendChild(
        placeholder("You have not marked anything in this document."),
      );
      this.focused = null;
      return;
    }

    // The roving tabindex has to land somewhere before the first Tab.
    if (
      this.focused === null ||
      !this.rows.some((row) => row.mark.id === this.focused)
    ) {
      this.focused = this.rows[0]?.mark.id ?? null;
    }
    for (const row of this.rows) {
      const element = this.build(row);
      this.elements.set(row.mark.id, element);
      this.list.appendChild(element);
    }
    if (this.selected !== null) this.mark(this.selected, true);
  }

  private say(text: string): void {
    if (text === this.said) return;
    this.said = text;
    this.notice.textContent = text;
    this.notice.style.display = text ? "block" : "none";
  }

  private build(row: MarkRow): HTMLElement {
    const { mark } = row;
    const element = document.createElement("div");
    element.setAttribute("role", "option");
    element.setAttribute("aria-selected", "false");
    element.dataset.id = String(mark.id);
    element.tabIndex = mark.id === this.focused ? 0 : -1;
    element.style.cssText =
      "display:flex;gap:0.5rem;align-items:baseline;cursor:default;" +
      "padding:0.3rem 0.7rem 0.3rem 10px;";

    // The mark's own colour, and no more than that: the kind is spelled out in
    // the second line, so nothing here is carried by colour alone.
    const swatch = document.createElement("span");
    swatch.setAttribute("aria-hidden", "true");
    swatch.style.cssText =
      `flex:none;width:${SWATCH}px;height:${SWATCH}px;border-radius:2px;` +
      `background:${cssColor(mark.color)};` +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);";

    const page = document.createElement("span");
    // A mark nothing could place has no page to name, and an em dash is what
    // that looks like next to a column of numbers.
    page.textContent = row.page === null ? "—" : String(row.page + 1);
    page.style.cssText =
      "flex:none;min-width:3ch;text-align:right;opacity:0.5;" +
      "font-variant-numeric:tabular-nums;";

    const text = document.createElement("div");
    text.style.cssText = "flex:1;min-width:0;";

    const line = rowLine(mark.note, this.opts.coveredFor(mark.id));
    const note = document.createElement("div");
    note.dataset.part = "note";
    // Whose words these are, said rather than left to be inferred from the
    // styling below. Both come off `line.own`, so they cannot disagree, and a
    // reader of this row --- the check harness, a screen reader's user with a
    // stylesheet of their own --- can ask the question without parsing CSS.
    note.dataset.own = line.own ? "yes" : "no";
    note.textContent = line.text;
    note.style.cssText =
      "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;" +
      (line.own ? "" : "opacity:0.6;font-style:italic;");

    const kind = document.createElement("div");
    kind.dataset.part = "kind";
    kind.textContent = nameOf(mark.kind);
    kind.style.cssText =
      "opacity:0.6;font-size:0.85em;overflow:hidden;text-overflow:ellipsis;" +
      "white-space:nowrap;";

    text.append(note, kind);

    // Named for the kind rather than "Remove": a screen reader walking the list
    // hears "Remove highlight", which says which row it is on. `nameOf` is the
    // same source the second line uses, so the two cannot drift -- and lowered
    // the way `markpopup.ts` lowers it for its own Remove button, so the two
    // ways a mark comes off are one phrase rather than two spellings of one.
    const remove = document.createElement("button");
    remove.type = "button";
    remove.dataset.part = "remove";
    // The name literal stays on the call's own line: `check_webview_sinks.py`
    // reads these a line at a time and flags a call whose arguments it cannot
    // parse, which is the right behaviour and which wrapping this defeated.
    const label = `Remove ${nameOf(mark.kind).toLowerCase()}`;
    remove.setAttribute("aria-label", label);
    remove.textContent = "\u00d7";
    remove.style.cssText =
      "flex:none;border:0;background:none;cursor:pointer;padding:0 0.2rem;" +
      "font-size:1.1em;line-height:1;opacity:0.55;color:inherit;";
    // Two listeners, and the split is deliberate. `click` carries Enter and
    // Space on a button for free, which is what makes this reachable from the
    // keyboard without a third route. `pointerdown` only stops the event: the
    // row's own `pointerdown` below would otherwise fire first and open the
    // note of the mark being taken off, which is a flash and, worse, an edit
    // aimed through a box that is closing.
    remove.addEventListener("pointerdown", (event) => event.stopPropagation());
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      this.opts.onRemove(mark.id);
    });

    element.append(swatch, page, text, remove);

    if (row.page === null) {
      // Listed rather than dropped, and marked rather than left looking
      // ordinary: a row that cannot be pressed and does not say so reads as a
      // broken panel. Same treatment as an outline row whose destination
      // resolves to no page.
      element.setAttribute("aria-disabled", "true");
      element.style.opacity = "0.55";
      return element;
    }

    // `pointerdown`, not `click`: whatever had focus is blurred first, and that
    // can move the viewport under the row.
    element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.focus(mark.id);
      this.opts.onPick(mark.id);
    });

    return element;
  }

  private focus(id: number): void {
    if (this.focused === id) return;
    if (this.focused !== null) {
      const previous = this.elements.get(this.focused);
      if (previous) previous.tabIndex = -1;
    }
    this.focused = id;
    const element = this.elements.get(id);
    if (element) element.tabIndex = 0;
  }

  /** Moves focus by `delta` rows and puts the keyboard there. */
  private move(delta: number): void {
    if (this.rows.length === 0) return;
    const at = this.rows.findIndex((row) => row.mark.id === this.focused);
    const next = Math.max(
      0,
      Math.min((at < 0 ? 0 : at) + delta, this.rows.length - 1),
    );
    const target = this.rows[next];
    if (!target) return;
    this.focus(target.mark.id);
    this.elements.get(target.mark.id)?.focus();
  }

  /** Whether a row can be activated at all. */
  private placed(id: number): boolean {
    return this.rows.some((row) => row.mark.id === id && row.page !== null);
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    // The event's target is authoritative and the mirror is the fallback: a
    // window without system focus moves `activeElement` without delivering
    // `focusin`, and every key then operates on a row the reader is not on.
    // A key on the remove control is the control's. Without this, Enter there
    // reads as Enter on the row -- `idOf` finds no id on the button, so the
    // fallback hands it the focused row and the note opens instead of the mark
    // coming off.
    const part = (event.target as HTMLElement | null)?.dataset?.part;
    if (part === "remove") return;

    const from = idOf(event.target) ?? this.focused;
    if (from !== null && from !== this.focused && this.elements.has(from)) {
      this.focus(from);
    }

    switch (event.key) {
      case "ArrowDown":
        this.move(1);
        break;
      case "ArrowUp":
        this.move(-1);
        break;
      case "Home":
        this.move(-this.rows.length);
        break;
      case "End":
        this.move(this.rows.length);
        break;
      case "Enter":
      case " ":
        // The same refusal the pointer gets, and it has to be written twice
        // because the two routes in are two listeners. A row with nowhere to go
        // that answered the keyboard would scroll the reader nowhere.
        if (from !== null && this.placed(from)) this.opts.onPick(from);
        break;
      case "Delete":
      case "Backspace":
        // Deliberately NOT guarded on `placed`. Enter refuses an unplaced row
        // because there is nowhere to scroll to; this is the one thing such a
        // row can still do, and the reason `onRemove` exists at all.
        if (from !== null) this.opts.onRemove(from);
        break;
      default:
        return;
    }

    event.preventDefault();
    // The viewer underneath scrolls on arrows and Home/End; a list that let them
    // through would move the page as well as the selection.
    event.stopPropagation();
  };
}

/** The mark id a row element carries, or `null` for anything else. */
function idOf(target: EventTarget | null): number | null {
  const raw = (target as HTMLElement | null)?.dataset?.id;
  if (raw === undefined) return null;
  const id = Number(raw);
  return Number.isFinite(id) ? id : null;
}

function placeholder(text: string): HTMLElement {
  const element = document.createElement("div");
  element.style.cssText = "padding:0.5rem 0.7rem;opacity:0.55;";
  element.textContent = text;
  return element;
}
