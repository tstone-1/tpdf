/**
 * The redactions tab: every region the reader has marked for removal.
 *
 * `docs/PLAN.md` §6's workflow is mark, review, apply, verify, and this is step
 * 2 --- *"every mark listed with page, extracted text and thumbnail. The last
 * chance to catch an over- or under-selection."* Until it existed the marking
 * step had nowhere to be reviewed: a pending region is a red wash on a page, so
 * checking six of them across a forty-page document meant scrolling the document
 * and trusting your eyes about which words were under each one.
 *
 * ## Two things this panel says that no other panel has to
 *
 * **Nothing has been removed yet.** That is the standing fact about every row
 * here, it is the difference between this list and a redacted file, and §6's
 * thesis is that a redaction which looks done and is not is worse than none. So
 * it is written in the panel rather than left to be inferred from the tab being
 * called *Redactions*.
 *
 * **A row with no words under it is a finding, not a quiet row.** For a mark it
 * means nothing much --- a square drawn on a photograph covers no text and that
 * is what a reader wanted. For a region it means the removal will take no text
 * out of that rectangle, so whatever is under it is a picture, a vector drawing,
 * or nothing, and `redact.rs` reports every one of those as *unhandled* rather
 * than removing it. The row therefore distinguishes three states where
 * `marklist.ts` has two --- four, counting a page that could not be read at all
 * --- because an empty answer that might still be arriving is the reading a
 * review panel must never offer.
 *
 * ## What the words are, and what they are not
 *
 * They come from {@link touchedText}, which takes every character the rectangle
 * touches rather than every character whose centre it contains --- the rule
 * `text.ts` argues at length, and the short version is that a highlight's
 * rectangle is drawn generously around its words while a dragged region is a
 * claim about what disappears.
 *
 * They are **what the region covers, not what applying would remove**. Route B
 * deletes a whole text-showing operation when any of its glyphs is inside, so an
 * apply takes at least these words and commonly the rest of the line. The panel
 * is a review of the selection, which is what step 2 asks for; the report of
 * what a removal would actually take belongs to step 3, where the plan is
 * computed against the page's own objects.
 *
 * ## Nothing here builds markup from a string
 *
 * Every word on a row is the document's, lifted off a page by
 * {@link touchedText}. `textContent` and nothing else, `docs/THREAT-MODEL.md`
 * T8, and `scripts/check_webview_sinks.py` is the gate.
 */

import { placeholder } from "./panelrow";
import { flatten } from "./rowline";
import type { RedactionRow, RegionPlan } from "./pages";

/** Side of the swatch standing for the region, in CSS pixels. */
const SWATCH = 9;

/**
 * The wash a pending region is drawn in, as the panel's swatch.
 *
 * The same red `viewer.ts` paints, written here as its own constant rather than
 * imported: that one is a canvas fill string built for `globalAlpha`, this is a
 * CSS colour on a 9-pixel square, and a shared value would have one of the two
 * change for the other's reasons. What has to match is the reader's impression
 * that the row and the wash are the same thing, which a hex pair does.
 */
const SWATCH_COLOR = "rgba(190, 30, 45, 0.55)";

/** What a row says while the page it is on has not been read yet. */
const READING = "Reading the page…";

/**
 * What a row says once the page has been read and the region covers no words.
 *
 * Not *"No text"*, which reads as a property of the row. The region is the
 * subject: the reader dragged a rectangle and there are no words in it, which is
 * either exactly what they meant or the reason nothing will come out of it.
 */
const NO_TEXT = "No text in this region";

/** What a row says when the page it is on could not be read at all. */
const UNREADABLE = "Could not read the page";

/**
 * The panel's standing line above the rows, or "" when there are none.
 *
 * Two sentences at most, and the second only when it applies. The first is the
 * count and the fact that binds every row --- see the module note on why that is
 * written down rather than implied. The second names regions nothing could
 * place, which `pages.ts` explains cannot happen from the model as it stands and
 * which this panel lists anyway.
 */
export function noticeFor(rows: readonly RedactionRow[]): string {
  if (rows.length === 0) return "";
  const many = rows.length === 1 ? "1 region marked" : `${rows.length} regions marked`;
  const lost = rows.filter((row) => row.page === null).length;
  const said = `${many}. Nothing has been removed yet.`;
  if (lost === 0) return said;
  return lost === 1
    ? `${said} 1 is not on any page.`
    : `${said} ${lost} are not on any page.`;
}

/**
 * What a row's line says, and whether it is the document's own words.
 *
 * **Four answers, because there are four things that can be true**, and the
 * three that are not words say different things to a reader deciding whether to
 * destroy something:
 *
 * - `undefined` --- the page has not been read yet. Still arriving.
 * - `null` --- the page could not be read. Nothing is known about this region.
 * - `""` --- the page was read and the rectangle holds no words. A finding: a
 *   removal will take no text out of it.
 * - anything else --- the words the region covers.
 *
 * Collapsing any pair of the first three is the obvious simplification and is
 * what `marklist.ts` does, correctly, because there an absent note is not
 * alarming. Here the whole panel exists so that a reader can tell *nothing is
 * there* from *I do not know yet*, and §6 forbids reporting the second as the
 * first anywhere in this subsystem.
 */
export function rowLineFor(words: string | null | undefined): {
  text: string;
  own: boolean;
} {
  if (words === undefined) return { text: READING, own: false };
  if (words === null) return { text: UNREADABLE, own: false };
  const said = flatten(words);
  if (said) return { text: said, own: true };
  return { text: NO_TEXT, own: false };
}

/** What a redaction row needs from whoever owns the document. */
export interface RedactListOptions {
  /** Called when a row is activated, with the redaction's id. */
  onPick: (id: number) => void;
  /**
   * Called when a row's remove control is used, with the redaction's id.
   *
   * This panel is the **only** way to take a pending region off other than
   * undo, and undo is chronological: a reader who drew six regions and wants
   * the second one back cannot get there by undoing. That is why the control is
   * on every row, including a row nothing could place --- see
   * {@link RedactionRow.page}.
   */
  onRemove: (id: number) => void;
  /**
   * The words a region covers, by id. See {@link rowLineFor} for the four
   * answers and what each one means.
   *
   * A lookup rather than a field on {@link RedactionRow} for `marklist.ts`'s
   * reason: nothing in the model holds this and no byte of a file carries it.
   * It is read out of the page as it is now, and the scheduling --- one page at
   * a time, only while somebody is looking --- belongs to the caller, exactly as
   * it does for the comments panel's covered words.
   */
  wordsFor: (id: number) => string | null | undefined;
  /**
   * What a removal would take from a region, or `undefined` before the backend
   * has been asked.
   *
   * Separate from {@link wordsFor} because they answer different questions and
   * arrive by different routes: the words are geometry the frontend already
   * holds, and this is a reading of the page's content stream that only the
   * worker can do. What it is *for* is the second line of a row --- the objects
   * a removal cannot take, which is the one thing a reader must know before
   * they destroy anything.
   */
  planFor: (id: number) => RegionPlan | undefined;
}

/**
 * The row's second line: what a removal of this region would not take.
 *
 * `""` when there is nothing to say, which is the ordinary case and includes
 * every region whose plan has not arrived yet. **Silence is the right default
 * here and is not the right default for the first line**: a row with no words
 * says so because a reader is waiting for them, and a row with no warning is
 * simply a row with no warning.
 *
 * Grouped by kind and counted, because a page with three pictures on it reports
 * three findings and a reader reading three identical sentences cannot tell
 * that from one printed thrice. `redact::Unhandled` carries the position for
 * exactly that reason; this panel has no room for it and the count says the
 * same thing.
 */
export function warningFor(plan: RegionPlan | undefined): string {
  if (!plan || plan.unhandled.length === 0) return "";
  const kinds = new Map<string, number>();
  for (const object of plan.unhandled) {
    kinds.set(object.kind, (kinds.get(object.kind) ?? 0) + 1);
  }
  const said = [...kinds.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    // `an image`, `a path`. A rule rather than a table, because the kinds are
    // PDFium's words and a table here would be a second list to keep in step
    // with `objects.ts` --- which is the drift this panel has already avoided
    // once by taking the kind rather than a sentence.
    .map(([kind, many]) =>
      many === 1 ? `${/^[aeiou]/.test(kind) ? "an" : "a"} ${kind}` : `${many} ${kind}s`,
    )
    .join(" and ");
  return `Also covers ${said}, which a removal cannot take`;
}

/** The redactions panel: a row per pending region, in page order. */
export class RedactList {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: RedactListOptions;

  private rows: RedactionRow[] = [];
  private readonly elements = new Map<number, HTMLElement>();
  /** Id of the row the roving tabindex is on, or null. */
  private focused: number | null = null;
  /** What the notice last said, so an unchanged paint writes nothing. */
  private said = "";

  constructor(host: HTMLElement, opts: RedactListOptions) {
    this.opts = opts;

    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText =
      "flex:none;padding:0.3rem 0.7rem;opacity:0.7;display:none;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.setAttribute("aria-label", "Regions marked for removal");
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
   * Counted out of the list element rather than off `this.rows`, for the reason
   * `marklist.ts` gives: the obvious version answers with the rows the panel was
   * *given*, which is the same number whether or not one element was built.
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
   * Read back rather than reported from the row it was built from, for the
   * reason every sibling panel gives: a getter answering from the source agrees
   * with itself whatever the row actually contains.
   */
  rowText(id: number): {
    words: string;
    /**
     * The second line, or `null` where the row has none.
     *
     * **`null` rather than `""`**, because an empty warning element and no
     * warning element are different rows and read the same through a string: a
     * row that draws an empty line has a line of chrome nobody can see, and a
     * check that could not tell them apart let exactly that ship.
     */
    warning: string | null;
    page: string;
    own: boolean;
  } {
    const row = this.elements.get(id);
    if (!row) return { words: "", warning: null, page: "", own: false };
    const [, page, text] = [...row.children] as HTMLElement[];
    const parts = [...(text?.children ?? [])] as HTMLElement[];
    const words = parts.find((part) => part.dataset?.part === "words");
    const warning = parts.find((part) => part.dataset?.part === "warning");
    return {
      words: words?.textContent ?? "",
      warning: warning ? (warning.textContent ?? "") : null,
      page: page?.textContent ?? "",
      own: words?.dataset?.own === "yes",
    };
  }

  /**
   * Replaces the regions shown.
   *
   * One state rather than the comments panel's three, for `marklist.ts`'s
   * reason: the model is in this process and answers every time, so an empty
   * list means the reader has marked nothing rather than that an answer is
   * outstanding. The *words* on a row are the part that can still be arriving,
   * which is why they have three states and this does not.
   */
  setRedactions(rows: readonly RedactionRow[]): void {
    this.rows = [...rows];
    this.paint();
  }

  /**
   * Redraws the rows against whatever {@link RedactListOptions.wordsFor} now
   * says, without changing which regions are listed.
   *
   * Separate from {@link setRedactions} because the two change for different
   * reasons and at different rates: the list changes when the reader marks or
   * unmarks something, the words when a page finishes being extracted. A caller
   * with only the first would have to invent a list change to show a word.
   */
  setWords(): void {
    this.paint();
  }

  private paint(): void {
    this.list.replaceChildren();
    this.elements.clear();

    this.say(noticeFor(this.rows));

    if (this.rows.length === 0) {
      this.list.appendChild(
        placeholder("You have not marked anything for removal in this document."),
      );
      this.focused = null;
      return;
    }

    // The roving tabindex has to land somewhere before the first Tab.
    if (
      this.focused === null ||
      !this.rows.some((row) => row.redaction.id === this.focused)
    ) {
      this.focused = this.rows[0]?.redaction.id ?? null;
    }
    for (const row of this.rows) {
      const element = this.build(row);
      this.elements.set(row.redaction.id, element);
      this.list.appendChild(element);
    }
  }

  private say(text: string): void {
    if (text === this.said) return;
    this.said = text;
    this.notice.textContent = text;
    this.notice.style.display = text ? "block" : "none";
  }

  private build(row: RedactionRow): HTMLElement {
    const { redaction } = row;
    const element = document.createElement("div");
    element.setAttribute("role", "option");
    element.setAttribute("aria-selected", "false");
    element.dataset.id = String(redaction.id);
    element.tabIndex = redaction.id === this.focused ? 0 : -1;
    element.style.cssText =
      "display:flex;gap:0.5rem;align-items:baseline;cursor:default;" +
      "padding:0.3rem 0.7rem 0.3rem 10px;";

    // The wash the region is drawn in, so the row and the rectangle on the page
    // read as one thing. Nothing is carried by it alone: every row in this
    // panel is the same kind, which is the whole point of the panel being
    // separate from the marks one.
    const swatch = document.createElement("span");
    swatch.setAttribute("aria-hidden", "true");
    swatch.style.cssText =
      `flex:none;width:${SWATCH}px;height:${SWATCH}px;border-radius:2px;` +
      `background:${SWATCH_COLOR};` +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);";

    const page = document.createElement("span");
    // A region nothing could place has no page to name, and an em dash is what
    // that looks like next to a column of numbers.
    page.textContent = row.page === null ? "—" : String(row.page + 1);
    page.style.cssText =
      "flex:none;min-width:3ch;text-align:right;opacity:0.5;" +
      "font-variant-numeric:tabular-nums;";

    // A column rather than a line, because a row can have two things to say.
    const text = document.createElement("div");
    text.style.cssText = "flex:1;min-width:0;";

    const line = rowLineFor(this.opts.wordsFor(redaction.id));
    const words = document.createElement("div");
    words.dataset.part = "words";
    // Whether the line is the document's own words, said rather than left to be
    // read off the styling. Both come from `line.own`, so they cannot disagree.
    words.dataset.own = line.own ? "yes" : "no";
    words.textContent = line.text;
    // **The words are drawn plainly and only the fallbacks are dimmed**, which
    // inverts `marklist.ts`. There, dimmed-and-italic separates a sentence a
    // person typed from words the document supplied, and both appear. Here
    // nobody typed anything, so dimming the document's words would dim every
    // row that has any and leave the styling saying nothing at all.
    words.style.cssText =
      "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;" +
      (line.own ? "" : "opacity:0.6;font-style:italic;");
    text.append(words);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.dataset.part = "remove";
    // The name literal stays on the call's own line: `check_webview_sinks.py`
    // reads these a line at a time and flags a call whose arguments it cannot
    // parse.
    remove.setAttribute("aria-label", "Remove region");
    remove.textContent = "×";
    remove.style.cssText =
      "flex:none;border:0;background:none;cursor:pointer;padding:0 0.2rem;" +
      "font-size:1.1em;line-height:1;opacity:0.55;color:inherit;";
    // Two listeners, and the split is `marklist.ts`'s: `click` carries Enter and
    // Space on a button for free, and `pointerdown` only stops the row's own
    // listener below from navigating away from the region being taken off.
    remove.addEventListener("pointerdown", (event) => event.stopPropagation());
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      this.opts.onRemove(redaction.id);
    });

    const warning = warningFor(this.opts.planFor(redaction.id));
    if (warning) {
      // Under the words rather than beside them, and dimmed rather than red:
      // this is a fact about what will survive, not an error, and a row that
      // shouts is a row a reader stops reading after the third one. The
      // sentence is what carries the weight.
      const said = document.createElement("div");
      said.dataset.part = "warning";
      said.textContent = warning;
      said.style.cssText =
        "opacity:0.6;font-size:0.85em;overflow:hidden;text-overflow:ellipsis;" +
        "white-space:nowrap;";
      text.append(said);
    }

    element.append(swatch, page, text, remove);

    if (row.page === null) {
      // Listed rather than dropped, and marked rather than left looking
      // ordinary: a row that cannot be pressed and does not say so reads as a
      // broken panel.
      element.setAttribute("aria-disabled", "true");
      element.style.opacity = "0.55";
      return element;
    }

    // `pointerdown`, not `click`: whatever had focus is blurred first, and that
    // can move the viewport under the row.
    element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.focus(redaction.id);
      this.opts.onPick(redaction.id);
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
    const at = this.rows.findIndex((row) => row.redaction.id === this.focused);
    const next = Math.max(
      0,
      Math.min((at < 0 ? 0 : at) + delta, this.rows.length - 1),
    );
    const target = this.rows[next];
    if (!target) return;
    this.focus(target.redaction.id);
    this.elements.get(target.redaction.id)?.focus();
  }

  /** Whether a row can be activated at all. */
  private placed(id: number): boolean {
    return this.rows.some((row) => row.redaction.id === id && row.page !== null);
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    // A key on the remove control is the control's. Without this, Enter there
    // reads as Enter on the row and the panel navigates instead of removing.
    const part = (event.target as HTMLElement | null)?.dataset?.part;
    if (part === "remove") return;

    // The event's target is authoritative and the mirror is the fallback: a
    // window without system focus moves `activeElement` without delivering
    // `focusin`, and every key then operates on a row the reader is not on.
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
        // The same refusal the pointer gets, written twice because the two
        // routes in are two listeners. A row with nowhere to go that answered
        // the keyboard would scroll the reader nowhere.
        if (from !== null && this.placed(from)) this.opts.onPick(from);
        break;
      case "Delete":
      case "Backspace":
        // Deliberately NOT guarded on `placed`, for `marklist.ts`'s reason:
        // this is the one thing an unplaceable row can still do.
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

/** The redaction id a row element carries, or `null` for anything else. */
function idOf(target: EventTarget | null): number | null {
  const raw = (target as HTMLElement | null)?.dataset?.id;
  if (raw === undefined) return null;
  const id = Number(raw);
  return Number.isFinite(id) ? id : null;
}
