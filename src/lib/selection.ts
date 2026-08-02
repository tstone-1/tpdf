/**
 * A text selection, which may span pages.
 *
 * Kept apart from the viewer because it is entirely arithmetic --- two carets,
 * an ordering, and a per-page character range derived from them --- and because
 * the interesting failures are all at the ends: a selection that starts on the
 * page it ends on, one that ends exactly where it started, one dragged upwards
 * so the focus precedes the anchor. Those are much easier to get wrong than to
 * test, and testing them should not require a webview.
 *
 * There is no `isComplete` either, and it is worth saying why it went. It
 * answered "does the cache hold every page this selection touches" --- a
 * question the copy path asked twice, before and after loading --- and the
 * answer became worthless the moment a selection outgrew the cache: the cache
 * evicts, so the second reading was a fact about the bound rather than about
 * the document. A caller that must have every page now holds each reply as it
 * lands ({@link Selection.textFrom}), which cannot be undone by an eviction.
 *
 * There is no `isEmpty`. There was, and every call site guarded on it before
 * doing anything --- and a mutation making it return `false` unconditionally
 * changed no observable behaviour at all, because an anchor equal to the focus
 * already yields an empty range, which yields no text, no quads and no count.
 * Unreachable defence reads as load-bearing and can quietly become wrong, so it
 * went the way of `queue.rs`'s zero guard. A caller that needs the question
 * answered can ask whether the selected text is empty, which is pinned.
 *
 * The pages *between* the ends are deliberately not enumerated. A selection from
 * page 3 to page 700 has no business materialising 697 page ranges, and the two
 * things anyone asks --- what does page N contribute, and what is the whole
 * string --- are both answerable without it.
 */

import { readingTextOf } from "./reading";
import { runsFor, type Caret, type PageText, type Quad, type TextCache } from "./text";

/** Which of two carets comes first in reading order. */
function precedes(a: Caret, b: Caret): boolean {
  return a.page < b.page || (a.page === b.page && a.index < b.index);
}

/** A half-open character range within one page. */
export interface PageRange {
  from: number;
  /** Exclusive. `Infinity` means "to the end of this page". */
  to: number;
}

export class Selection {
  /** Where the drag started. Stays put while the focus moves. */
  anchor: Caret;
  /** Where the pointer is now. */
  focus: Caret;

  constructor(at: Caret) {
    this.anchor = at;
    this.focus = at;
  }

  /** The two ends in reading order, whichever direction the drag went. */
  get ordered(): { start: Caret; end: Caret } {
    return precedes(this.focus, this.anchor)
      ? { start: this.focus, end: this.anchor }
      : { start: this.anchor, end: this.focus };
  }

  /**
   * What a page contributes, or `null` if it contributes nothing.
   *
   * `Infinity` rather than a character count for the open end: this class does
   * not know how long a page is, and asking would mean holding a reference to
   * the text of every page it spans. The consumers clamp.
   */
  rangeOn(page: number): PageRange | null {
    const { start, end } = this.ordered;
    if (page < start.page || page > end.page) return null;
    return {
      from: page === start.page ? start.index : 0,
      to: page === end.page ? end.index : Infinity,
    };
  }

  /** Highlight rectangles for a page, in PDF points from its top-left. */
  quadsOn(page: number, cache: TextCache): Quad[] {
    const range = this.rangeOn(page);
    const text = range && cache.peek(page);
    if (!range || !text) return [];
    return runsFor(text, range.from, range.to);
  }

  /**
   * The selected text, pages joined by a newline.
   *
   * Returns what is *available*: a page whose text has not arrived contributes
   * nothing rather than blocking. That is the right trade for a status line and
   * for the check harness --- but it means a caller must not treat the result
   * as proof the whole selection was included. A caller that needs that has to
   * hold each page's reply and see them all, which is what
   * {@link textFrom} is for and what `Viewer.selectionText` does.
   */
  text(cache: TextCache): string {
    const { start, end } = this.ordered;
    const parts: string[] = [];
    for (let page = start.page; page <= end.page; page++) {
      const text = cache.peek(page);
      if (!text) continue;
      const part = this.textFrom(page, text);
      if (part !== null) parts.push(part);
    }
    return parts.join("\n");
  }

  /**
   * What one page contributes, given that page's own text.
   *
   * Takes the text rather than the cache, which is what lets a caller holding a
   * page's reply take its contribution *there and then* instead of asking the
   * cache for it again later. `TextCache` is bounded, so a large selection is
   * partly evicted by the time its last page arrives, and a caller that went
   * back to the cache would find the front of its own selection gone --- see
   * `Viewer.selectionText`, where that was a copy that could never succeed.
   *
   * `null` for a page the selection does not touch, which {@link pages} never
   * names.
   */
  textFrom(page: number, text: PageText): string | null {
    const range = this.rangeOn(page);
    if (!range) return null;
    // In the order the page reads rather than the order the file was written
    // in --- see `readingTextOf`. The two differ only where a producer
    // interleaved its columns, and there the difference is the whole point.
    return readingTextOf(text, range.from, range.to);
  }

  /** Pages the selection touches, for a caller that must load them all. */
  pages(): number[] {
    const { start, end } = this.ordered;
    const pages: number[] = [];
    for (let page = start.page; page <= end.page; page++) pages.push(page);
    return pages;
  }
}
