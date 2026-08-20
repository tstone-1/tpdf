/**
 * The comments tab: every note, highlight and reply in the document.
 *
 * `docs/PLAN.md` §8 asked for this as the fourth sidebar tab and it is the last
 * of the four to exist. What makes it worth a tab rather than leaving the marks
 * on the page to speak for themselves: PDFium draws a sticky note as a
 * 24-point icon and a highlight as a wash, and neither shows a single word of
 * what was written. Until this panel, a document somebody had reviewed opened in
 * tpdf as a document with coloured boxes in it.
 *
 * ## A list, not a tree, with one indent
 *
 * A thread is a root and its replies, and `comments.ts` flattens it to rows with
 * a depth of 0 or 1. That is a `listbox` rather than the outline's `tree`,
 * because the nesting is one deep by construction and a screen reader announcing
 * "level 1 of 1" on every row is noise. Replies carry `aria-describedby`
 * pointing at the row they answer, which is what makes the relationship audible
 * rather than merely visual.
 *
 * ## Rows are bounded by the backend, not here
 *
 * `annots.rs` caps the scan at 5,000 comments and reports when it did, so every
 * row here is a real element and there is no windowing --- the same trade the
 * outline makes, for the same reason, and the cap is stated in the panel rather
 * than applied silently.
 *
 * ## Nothing here builds markup from a document string
 *
 * Bodies, authors and subjects are set with `textContent` only. That is the
 * frontend half of `docs/THREAT-MODEL.md` T8, and `scripts/check_webview_sinks.py`
 * is the gate that keeps it true --- this file is the largest body of
 * document-chosen text tpdf puts on screen, so it is where that would break
 * first.
 */

import {
  bylineOf,
  noticeFor,
  rowsOf,
  summaryOf,
  type Comments,
  type CommentRow,
} from "./comments";

/** Indent of a reply, in CSS pixels. */
const INDENT = 16;

export interface CommentListOptions {
  /** Called when a row is activated, with the comment's id. */
  onPick: (id: number) => void;
}

/** The comments panel: a status line, then one row per comment. */
export class CommentList {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: CommentListOptions;

  private comments: Comments | null = null;
  /** Whether an answer has arrived, as distinct from an empty one having. */
  private loaded = false;
  private rows: CommentRow[] = [];
  private readonly elements = new Map<number, HTMLElement>();
  /** Id of the row the roving tabindex is on, or null. */
  private focused: number | null = null;
  /** Id of the row shown as selected, or null. */
  private selected: number | null = null;
  /** What the notice last said, so an unchanged paint writes nothing. */
  private said = "";

  constructor(host: HTMLElement, opts: CommentListOptions) {
    this.opts = opts;

    // A live region, like the outline's cap notice and the results panel's
    // status: it appears after the reader is already in the panel.
    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText =
      "flex:none;padding:0.3rem 0.7rem;opacity:0.7;display:none;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.setAttribute("aria-label", "Comments");
    this.list.style.cssText = "flex:1;min-height:0;overflow-y:auto;";
    this.list.addEventListener("keydown", this.onKeyDown);
    // Focus can arrive without going through `focus(id)` --- a Tab into the
    // list, a click the browser handled --- and a roving tabindex that does not
    // follow it aims every later key at the wrong row. The outline learned this
    // the expensive way; see the trap about a mirror of the DOM's focus.
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
   * whether or not a single element was built. It was `this.rows.length` here
   * until 2026-08-20, when the identical getter in `marklist.ts` --- copied from
   * this one --- let a mutation that drew one row and stopped survive the window
   * check written to catch exactly that. "the sidebar lists every comment" was
   * unable to fail the same way, so it is fixed here too rather than left as the
   * one panel where the finding does not apply. Same trap as `rowText` below,
   * which is why that one already read the DOM.
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
   * Read back rather than reported from the comment it was built from, for the
   * reason `results.ts` gives: a getter returning the source would agree with
   * itself whatever the row actually contains.
   */
  rowText(id: number): { body: string; byline: string } {
    const row = this.elements.get(id);
    if (!row) return { body: "", byline: "" };
    // Walked by position rather than found by attribute selector, and read off
    // the leaves rather than their container. `results.ts` records why both
    // matter: the fake DOM the unit tests run against matches selectors by tag
    // name only and computes no aggregate `textContent`, so a selector-based
    // reader returns "" there --- which is exactly what an empty row returns.
    const [, text] = [...row.children] as HTMLElement[];
    const [body, byline] = [...(text?.children ?? [])] as HTMLElement[];
    return {
      body: body?.textContent ?? "",
      byline: byline?.textContent ?? "",
    };
  }

  /**
   * Replaces the comments shown.
   *
   * Three states saying three different things, as the outline's `setOutline`
   * does: not called yet is "still reading", an empty list is "this document has
   * none", and `null` is "this document's comments could not be read".
   * Collapsing the first two makes a slow document look like an unannotated one
   * for exactly as long as somebody would be looking at it.
   */
  setComments(comments: Comments | null): void {
    this.loaded = true;
    this.comments = comments;
    this.focused = null;
    this.selected = null;
    this.paint();
  }

  /** Marks one comment as the selected row and scrolls it into view. */
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
    this.rows = [];

    const limits = this.comments?.limits;
    const notice = limits ? noticeFor(limits) : null;
    this.say(notice ?? "");

    if (!this.loaded) {
      this.list.appendChild(placeholder("Reading the comments…"));
      return;
    }
    if (!this.comments) {
      this.list.appendChild(placeholder("The comments could not be read."));
      return;
    }
    if (this.comments.items.length === 0) {
      this.list.appendChild(placeholder("This document has no comments."));
      return;
    }

    this.rows = rowsOf(this.comments.items);
    // The roving tabindex has to land somewhere before the first Tab.
    if (this.focused === null || !this.rows.some((row) => row.comment.id === this.focused)) {
      this.focused = this.rows[0]?.comment.id ?? null;
    }
    for (const row of this.rows) {
      const element = this.build(row);
      this.elements.set(row.comment.id, element);
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

  private build(row: CommentRow): HTMLElement {
    const { comment, depth } = row;
    const element = document.createElement("div");
    element.setAttribute("role", "option");
    element.setAttribute("aria-selected", "false");
    element.id = rowId(comment.id);
    element.dataset.id = String(comment.id);
    element.tabIndex = comment.id === this.focused ? 0 : -1;
    element.style.cssText =
      "display:flex;gap:0.5rem;align-items:baseline;cursor:default;" +
      `padding:0.3rem 0.7rem 0.3rem ${10 + depth * INDENT}px;` +
      (depth > 0 ? "border-left:2px solid color-mix(in srgb, currentColor 20%, transparent);" : "");
    if (comment.reply_to !== null && this.elements.has(comment.reply_to)) {
      // Points at the row this answers, so the relationship is audible and not
      // only an indent. Only when that row is present: a reply whose parent was
      // cut is drawn as a root, and naming an element that is not there tells a
      // screen reader to read nothing.
      element.setAttribute("aria-describedby", rowId(comment.reply_to));
    }

    const page = document.createElement("span");
    page.textContent = String(comment.page + 1);
    page.style.cssText =
      "flex:none;min-width:3ch;text-align:right;opacity:0.5;" +
      "font-variant-numeric:tabular-nums;";

    const text = document.createElement("div");
    text.style.cssText = "flex:1;min-width:0;";

    const body = document.createElement("div");
    body.dataset.part = "body";
    body.textContent = summaryOf(comment);
    body.style.cssText =
      "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;" +
      (comment.body.trim() ? "" : "opacity:0.6;font-style:italic;");

    const byline = document.createElement("div");
    byline.dataset.part = "byline";
    byline.textContent = bylineOf(comment);
    byline.style.cssText =
      "opacity:0.6;font-size:0.85em;overflow:hidden;text-overflow:ellipsis;" +
      "white-space:nowrap;";

    text.append(body, byline);
    element.append(page, text);

    if (comment.hidden) {
      // A comment the producer marked hidden is listed --- somebody wrote it ---
      // and marked, because it is not on the page and a reader looking for it
      // there would not find it.
      const flag = document.createElement("span");
      flag.textContent = "hidden";
      flag.style.cssText = "flex:none;opacity:0.6;font-size:0.85em;";
      element.appendChild(flag);
    }

    // `pointerdown`, not `click`, for the reason `results.ts` gives: whatever
    // had focus is blurred first, and that can move the viewport under the row.
    element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.focus(comment.id);
      this.opts.onPick(comment.id);
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
    const at = this.rows.findIndex((row) => row.comment.id === this.focused);
    const next = Math.max(
      0,
      Math.min((at < 0 ? 0 : at) + delta, this.rows.length - 1),
    );
    const target = this.rows[next];
    if (!target) return;
    this.focus(target.comment.id);
    this.elements.get(target.comment.id)?.focus();
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    // The event's target is authoritative and the mirror is the fallback, which
    // is the reconciliation the outline needed: a window without system focus
    // moves `activeElement` without delivering `focusin`, and every key then
    // operates on a row the reader is not on.
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
        if (from !== null) this.opts.onPick(from);
        break;
      default:
        return;
    }

    event.preventDefault();
    // The viewer underneath scrolls on arrows and Home/End; a list that let
    // them through would move the page as well as the selection.
    event.stopPropagation();
  };
}

/** The DOM id of a row, so a reply can name the row it answers. */
function rowId(id: number): string {
  return `tpdf-comment-${id}`;
}

/** The comment id a row element carries, or `null` for anything else. */
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
