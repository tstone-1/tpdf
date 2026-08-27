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

/**
 * The smallest region worth marking for removal, in PDF points.
 *
 * A run can come back with no width --- a selection that ends exactly where a
 * line does contributes an empty run at the line's end --- and a region with no
 * area contains no glyph's centre, so it would remove nothing. Dropping it is
 * not tidiness: every region a reader is shown is a row they have to read and
 * certify, and a row that will never remove anything makes the list overstate
 * what the removal is going to do.
 *
 * Half a point rather than zero, because a run one hundredth of a point wide is
 * the same nothing arrived at by rounding.
 */
export const MIN_REDACTION_SIDE = 0.5;

/**
 * Turns one page's selection runs into regions to mark for removal.
 *
 * The viewer hands over a **flat** array --- four numbers per run, `left`,
 * `top`, `right`, `bottom`, already out of the crop and in the file's own space
 * --- because that is the shape `Edits.mark` takes. `Edits.redact` documents
 * itself as taking a region in exactly that space, so the two consumers agree
 * by what is written down rather than by anyone's reading of the geometry.
 *
 * **One region per run, not one box around the page's runs.** A bounding box
 * over a selection that spans lines covers everything between them, which on a
 * two-column page is the other column and on any page is whatever sits in the
 * margin. Route B removes a whole text-showing operation, so the *lines* that
 * go are the same either way; what differs is everything else the rectangle
 * swallowed, and a region a reader did not draw is a region they cannot check.
 *
 * The sides are ordered rather than trusted. Nothing in the viewer produces a
 * run with `right` left of `left`, and a region is a claim about what will be
 * destroyed, so it costs two comparisons to stop depending on that.
 *
 * A trailing group of fewer than four numbers is dropped. It cannot arrive from
 * `selectionQuadsByPage`, which builds the array four at a time --- which is
 * exactly why the loop must not read past the end on the day something else
 * calls this.
 */
export function areasFrom(
  quads: readonly number[],
): [number, number, number, number][] {
  const areas: [number, number, number, number][] = [];
  for (let at = 0; at < quads.length; at += 4) {
    const run = quads.slice(at, at + 4);
    // **One mechanism, deliberately.** The obvious form bounds the loop at
    // `at + 4 <= quads.length` *and* checks here, and then neither can be
    // tested: whichever one a mutation removes, the other still rescues the
    // partial run, so both report SURVIVED and the property is unguarded the
    // day somebody deletes the remaining one. See `docs/TRAPS.md` on two
    // mechanisms with the same limit.
    if (run.length < 4) continue;
    // `NaN` rather than `0` as the default `noUncheckedIndexedAccess` asks
    // for. It is unreachable past the check above, and if it ever is reached a
    // `NaN` propagates into a region that equals nothing, where a `0` would be
    // a plausible coordinate at the page's corner.
    const [a = NaN, b = NaN, c = NaN, d = NaN] = run;
    const left = Math.min(a, c);
    const right = Math.max(a, c);
    const top = Math.min(b, d);
    const bottom = Math.max(b, d);
    if (right - left < MIN_REDACTION_SIDE) continue;
    if (bottom - top < MIN_REDACTION_SIDE) continue;
    areas.push([left, top, right, bottom]);
  }
  return areas;
}
