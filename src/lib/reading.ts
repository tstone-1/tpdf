/**
 * Recovering the order a page is meant to be read in.
 *
 * A PDF carries no reading order. It carries glyphs at positions, in whatever
 * sequence its producer chose to emit them, and PDFium hands them back in that
 * sequence --- which `text.ts` has always treated as the order of the page.
 * On a single column it is. On two columns it is whatever the producer did:
 * `testdata/make_columns_pdf.py` builds two pages that look identical and
 * extract as `alpha one`, `alpha two`, ... and as `alpha one beta one`,
 * `alpha two beta two`, ... --- the second being what lands on the clipboard and
 * what a screen reader reads aloud.
 *
 * So the order has to be recovered from the geometry, which is the only thing
 * that describes the page rather than its producer.
 *
 * ## Recursive XY-cut, over fragments rather than characters
 *
 * The page is split at the widest band of whitespace that crosses it, and each
 * half is split again. A gutter between columns and a rule of space under a
 * heading are the same operation on different axes, which is why one algorithm
 * handles the heading case that defeats clustering by x position.
 *
 * Two rules make it behave on real pages:
 *
 * - **A cut has to be free.** The band must contain no fragment at all, not
 *   merely few --- a "mostly empty" cut severs a line that straddles it.
 * - **A column cut wins over a row cut whenever one exists.** Cutting rows first
 *   is what produces `alpha one beta one`: split the page into bands of
 *   whitespace and every band holds one line from each column. Columns are
 *   therefore separated first, and rows only inside them.
 *
 * The consequence, stated because it is a real limit rather than a detail: a
 * spanning heading is told from the body by the whitespace under it being wider
 * than the body's line leading. Where those are equal the page is genuinely
 * ambiguous --- nothing in the geometry says whether the top line spans or is
 * the first line of column one --- and what this produces there is locally
 * sensible and globally wrong, in the way described at {@link blocksOf}.
 *
 * ## Which way is along, and which way is down
 *
 * Every rule here is stated over two axes: *along* a line, and *across* from
 * one line to the next. Which screen axis each is, and which direction each
 * runs, is decided by the page's rotation --- see {@link axesFor}. The rest of
 * the file never mentions x or y, so a rotated page takes the same path as an
 * upright one rather than a parallel one that gets tested half as often.
 */

import {
  charQuad,
  type IndexRange,
  type PageText,
  type Quad,
} from "./text";

/**
 * How much wider than the typical character a gap must be to cut the page.
 *
 * Expressed in characters rather than points so it scales with the type: the
 * same document set in 9pt and 14pt has to cut in the same places. Three is
 * comfortably wider than the widest word space justification produces and
 * narrower than any gutter, which is the whole range it has to separate.
 */
const CUT_CHARS = 3;

/** How deep the recursion may go before a block is taken as it is. */
const MAX_DEPTH = 12;

/** A run of characters read consecutively, and where it sits. */
export interface Fragment {
  /** Index ranges, in reading order within the fragment. */
  ranges: IndexRange[];
  /** The bounding box of every character in it. */
  box: Quad;
}

/** One line of the page, in reading order. */
export interface ReadingLine {
  /** Index ranges, in reading order. Usually one; more where a line is split. */
  ranges: IndexRange[];
  box: Quad;
}

/**
 * Which screen axis is along a line and which is across, and which way each runs.
 *
 * Derived from `to_device` in `src-tauri/src/text.rs`, which maps a character
 * box out of the page's own space into the displayed one. Text advances along
 * `+x` in page space and lines advance along `-y`; the four rotations send that
 * pair to the four combinations below.
 *
 * The signs are the part that is easy to leave out and impossible to see: an
 * ordering that ignores them is right at 0 and 1 and exactly reversed at 2 and
 * 3, which reads as a document whose paragraphs are in the wrong order rather
 * than as a rotation bug. `viewer_check.py` pins it without reproducing this
 * table --- the same document viewed at all four rotations has to give the same
 * reading order, which only the right signs satisfy.
 */
export function axesFor(turns: number): {
  sideways: boolean;
  alongSign: 1 | -1;
  crossSign: 1 | -1;
} {
  const at = ((turns % 4) + 4) % 4;
  return {
    sideways: at % 2 === 1,
    alongSign: at === 2 || at === 3 ? -1 : 1,
    crossSign: at === 1 || at === 2 ? -1 : 1,
  };
}

/** Which way a page reads. */
export type Axes = ReturnType<typeof axesFor>;

/** A box's extent along a line, and across the lines, oriented for reading. */
interface Extents {
  alongStart: number;
  alongEnd: number;
  crossStart: number;
  crossEnd: number;
}

/**
 * A box in reading coordinates: both axes increasing in the reading direction.
 *
 * Negating rather than branching everywhere: once a box is in this frame, every
 * comparison in the file is a plain `<`, and there is one place where a
 * rotation can be got wrong instead of a dozen.
 */
function extentsOf(box: Quad, axes: Axes): Extents {
  const along0 = axes.sideways ? box.top : box.left;
  const along1 = axes.sideways ? box.bottom : box.right;
  const cross0 = axes.sideways ? box.left : box.top;
  const cross1 = axes.sideways ? box.right : box.bottom;
  return {
    alongStart: axes.alongSign === 1 ? along0 : -along1,
    alongEnd: axes.alongSign === 1 ? along1 : -along0,
    crossStart: axes.crossSign === 1 ? cross0 : -cross1,
    crossEnd: axes.crossSign === 1 ? cross1 : -cross0,
  };
}

/** Whether two boxes share most of their extent across the lines. */
function sameBand(a: Extents, b: Extents): boolean {
  const overlap = Math.min(a.crossEnd, b.crossEnd) - Math.max(a.crossStart, b.crossStart);
  const shorter = Math.min(a.crossEnd - a.crossStart, b.crossEnd - b.crossStart);
  return shorter > 0 && overlap / shorter > 0.5;
}

/** Grows `into` to contain `add`. */
function absorb(into: Quad, add: Quad): void {
  into.left = Math.min(into.left, add.left);
  into.right = Math.max(into.right, add.right);
  into.top = Math.min(into.top, add.top);
  into.bottom = Math.max(into.bottom, add.bottom);
}

/** Whether a box has any extent, i.e. PDFium placed the character. */
function placed(box: Quad): boolean {
  return box.right > box.left || box.bottom > box.top;
}

/** One character, and where it is. */
interface Placed {
  index: number;
  box: Quad;
  extents: Extents;
}

/**
 * The page's characters as fragments: runs that sit together on one line.
 *
 * Built by position rather than by index, which is the whole difference from
 * `linesOf` in `text.ts`: characters are banded by their cross extent wherever
 * they came from in the file, then ordered along the line, then split wherever
 * the gap between them is wide enough to be a gutter rather than a space.
 *
 * A character PDFium placed nowhere --- the separators it synthesises between
 * text objects --- joins the fragment before it, so nothing is dropped and the
 * ranges stay usable as ranges. On a page whose first character has no box
 * there is nothing before it, and it starts a fragment of its own with a
 * degenerate box; {@link blocksOf} then leaves it where it is.
 */
export function fragmentsOf(text: PageText, axes: Axes, gap: number): Fragment[] {
  const items: Placed[] = [];
  /** Where each unplaced character should be re-attached, by the index before it. */
  const trailing = new Map<number, number[]>();
  let last = -1;
  for (let index = 0; index < text.codes.length; index++) {
    const box = charQuad(text, index);
    if (!placed(box)) {
      const at = trailing.get(last) ?? [];
      at.push(index);
      trailing.set(last, at);
      continue;
    }
    items.push({ index, box, extents: extentsOf(box, axes) });
    last = index;
  }
  if (items.length === 0) {
    return text.codes.length > 0
      ? [{ ranges: [{ from: 0, to: text.codes.length }], box: emptyBox() }]
      : [];
  }

  // Banded by cross position, so a line is a line wherever its characters came
  // from. Sorted by the *start* of the extent rather than the middle: a
  // superscript shares a band with the text it hangs off, and its middle does
  // not.
  const byCross = [...items].sort((a, b) => a.extents.crossStart - b.extents.crossStart);
  const bands: Placed[][] = [];
  let band: Placed[] = [];
  let bandExtents: Extents | null = null;
  for (const item of byCross) {
    if (bandExtents && sameBand(bandExtents, item.extents)) {
      band.push(item);
      bandExtents.crossEnd = Math.max(bandExtents.crossEnd, item.extents.crossEnd);
      continue;
    }
    if (band.length > 0) bands.push(band);
    band = [item];
    bandExtents = { ...item.extents };
  }
  if (band.length > 0) bands.push(band);

  const fragments: Fragment[] = [];
  for (const members of bands) {
    members.sort((a, b) => a.extents.alongStart - b.extents.alongStart);
    let current: Placed[] = [];
    let reach = -Infinity;
    for (const item of members) {
      if (current.length > 0 && item.extents.alongStart - reach > gap) {
        fragments.push(fragmentOf(current, trailing));
        current = [];
      }
      current.push(item);
      reach = Math.max(reach, item.extents.alongEnd);
    }
    if (current.length > 0) fragments.push(fragmentOf(current, trailing));
  }
  return fragments;
}

/** A box no character occupies, for a page whose characters have no boxes. */
function emptyBox(): Quad {
  return { left: 0, top: 0, right: 0, bottom: 0 };
}

/** Builds a fragment from the characters on it, re-attaching what follows them. */
function fragmentOf(members: Placed[], trailing: Map<number, number[]>): Fragment {
  const indices: number[] = [];
  const box: Quad = { ...(members[0] as Placed).box };
  for (const item of members) {
    indices.push(item.index, ...(trailing.get(item.index) ?? []));
    absorb(box, item.box);
  }
  return { ranges: rangesOf(indices), box };
}

/**
 * The gap that counts as a cut, in points.
 *
 * Taken from the median **character**'s extent along the line, so it follows the
 * type size: the same document set in 9pt and 14pt cuts in the same places.
 *
 * Measured once, over characters, and handed to every stage --- including the
 * one that separates columns, which is the reason it is a parameter rather than
 * something each stage works out for itself. Derived from the *fragments* there
 * instead, the threshold would be three times the median line's length, and a
 * gutter would have to be wider than half the page to be found at all. Written
 * that way first, and the two-column fixture passed it by 20 points.
 *
 * The median rather than the mean: a page with one dropped capital, or with a
 * full-width rule that extracts as a character, would otherwise raise the
 * threshold for everything else on it.
 */
export function cutWidth(text: PageText, axes: Axes): number {
  const widths: number[] = [];
  for (let index = 0; index < text.codes.length; index++) {
    const box = charQuad(text, index);
    if (!placed(box)) continue;
    const extents = extentsOf(box, axes);
    const width = extents.alongEnd - extents.alongStart;
    if (width > 0) widths.push(width);
  }
  widths.sort((a, b) => a - b);
  const median = widths[Math.floor(widths.length / 2)] ?? 0;
  return median * CUT_CHARS;
}

/** Merges a sorted list of indices into half-open ranges. */
function rangesOf(indices: number[]): IndexRange[] {
  const sorted = [...indices].sort((a, b) => a - b);
  const ranges: IndexRange[] = [];
  for (const index of sorted) {
    const last = ranges[ranges.length - 1];
    if (last && last.to === index) last.to = index + 1;
    else ranges.push({ from: index, to: index + 1 });
  }
  return ranges;
}

/**
 * Splits fragments into blocks, in reading order, by recursive XY-cut.
 *
 * A cut is a band of whitespace that no fragment touches, with fragments on
 * both sides of it. Column cuts are taken first and all at once; row cuts are
 * taken one at a time, at the widest gap, because the widest one is the
 * structural boundary and the rest are line leading.
 *
 * Where the widest row gap is *not* the structural boundary --- a heading set
 * with no more air under it than the body has between its lines --- the cut
 * lands in the body instead, and the result is each part of the page ordered
 * correctly within itself and the parts interleaved with each other. That is
 * the failure this degrades to, and it is worth knowing it is not "the page
 * comes out backwards".
 */
export function blocksOf(
  fragments: Fragment[],
  axes: Axes,
  gap: number,
  depth = 0,
): Fragment[][] {
  if (fragments.length < 2 || depth >= MAX_DEPTH) return [fragments];

  const spans = fragments.map((fragment) => ({
    fragment,
    extents: extentsOf(fragment.box, axes),
  }));

  const columns = split(spans, (s) => [s.extents.alongStart, s.extents.alongEnd], gap);
  if (columns.length > 1) {
    return columns.flatMap((group) =>
      blocksOf(
        group.map((s) => s.fragment),
        axes,
        gap,
        depth + 1,
      ),
    );
  }

  const rows = splitOnce(spans, (s) => [s.extents.crossStart, s.extents.crossEnd]);
  if (rows.length > 1) {
    return rows.flatMap((group) =>
      blocksOf(
        group.map((s) => s.fragment),
        axes,
        gap,
        depth + 1,
      ),
    );
  }
  return [fragments];
}

/** A fragment paired with its reading-frame extents. */
type Span = { fragment: Fragment; extents: Extents };

/**
 * Splits at every free band wider than `gap`, in order.
 *
 * Used for columns, where every gutter is a real boundary and taking them one
 * at a time would only cost recursion.
 */
function split(spans: Span[], of: (s: Span) => [number, number], gap: number): Span[][] {
  const sorted = [...spans].sort((a, b) => of(a)[0] - of(b)[0]);
  const groups: Span[][] = [];
  let group: Span[] = [];
  let reach = -Infinity;
  for (const span of sorted) {
    const [start, end] = of(span);
    if (group.length > 0 && start - reach > gap) {
      groups.push(group);
      group = [];
    }
    group.push(span);
    reach = Math.max(reach, end);
  }
  if (group.length > 0) groups.push(group);
  return groups;
}

/**
 * Splits at the single widest free band, in order.
 *
 * Used for rows. Splitting at every free band is what turns a two-column page
 * into `alpha one beta one`: every band of whitespace between two lines crosses
 * the whole page, so cutting them all leaves each band holding one line from
 * each column, and no column boundary is ever found. Taking only the widest and
 * recursing gives the column cut a chance to happen first inside each half.
 */
function splitOnce(spans: Span[], of: (s: Span) => [number, number]): Span[][] {
  const sorted = [...spans].sort((a, b) => of(a)[0] - of(b)[0]);
  let best = { at: -1, size: 0 };
  let reach = -Infinity;
  for (let index = 0; index < sorted.length; index++) {
    const [start, end] = of(sorted[index] as Span);
    if (index > 0 && start - reach > best.size) best = { at: index, size: start - reach };
    reach = Math.max(reach, end);
  }
  if (best.at < 0) return [sorted];
  return [sorted.slice(0, best.at), sorted.slice(best.at)];
}

/**
 * The page's lines, in the order they are meant to be read.
 *
 * The replacement for `linesOf` wherever order matters. Fragments that end up
 * in the same block and share a band are one line --- a line broken by a wide
 * word space is not two lines --- and the blocks are already ordered, so the
 * lines are.
 */
export function readingLines(text: PageText): ReadingLine[] {
  const axes = axesFor(text.quarter_turns);
  const gap = cutWidth(text, axes);
  const lines: ReadingLine[] = [];

  for (const block of blocksOf(fragmentsOf(text, axes, gap), axes, gap)) {
    const ordered = [...block].sort((a, b) => {
      const [ea, eb] = [extentsOf(a.box, axes), extentsOf(b.box, axes)];
      return ea.crossStart - eb.crossStart || ea.alongStart - eb.alongStart;
    });
    let current: ReadingLine | null = null;
    for (const fragment of ordered) {
      if (current && sameBand(extentsOf(current.box, axes), extentsOf(fragment.box, axes))) {
        current.ranges.push(...fragment.ranges);
        absorb(current.box, fragment.box);
        continue;
      }
      current = { ranges: [...fragment.ranges], box: { ...fragment.box } };
      lines.push(current);
    }
  }
  return lines;
}

/**
 * Every character index, in reading order.
 *
 * A permutation of `0..codes.length`, so a caller that has a set of selected
 * indices can emit them in the order the page reads without knowing anything
 * about how that order was arrived at.
 */
export function readingOrder(text: PageText): number[] {
  const order: number[] = [];
  for (const line of readingLines(text)) {
    for (const range of line.ranges) {
      for (let index = range.from; index < range.to; index++) order.push(index);
    }
  }
  return order;
}

/** The characters of `ranges`, in the order they are given. */
export function textOfRanges(text: PageText, ranges: readonly IndexRange[]): string {
  let out = "";
  for (const range of ranges) {
    const end = Math.min(range.to, text.codes.length);
    for (let at = Math.max(0, range.from); at < end; at += 4096) {
      out += String.fromCodePoint(...text.codes.slice(at, Math.min(at + 4096, end)));
    }
  }
  return out;
}

/**
 * The characters whose index falls in `[from, to)`, emitted in reading order.
 *
 * The copy path. The *selection* is still a range of character indices --- which
 * is a range in the order the file was written, not in the order the page reads
 * --- so on a page whose producer interleaved its columns, a drag across the
 * gutter still takes in more than was dragged over. What this fixes is the
 * order of whatever it did take in, and the case that matters most: select-all
 * on a two-column page now copies the columns one after the other instead of
 * one line from each in turn.
 *
 * Making the *drag* select the region dragged over means carets that carry a
 * reading position rather than a character index, which is a change to the
 * selection model rather than to this function. See `docs/PLAN.md`.
 */
export function readingTextOf(text: PageText, from: number, to: number): string {
  const start = Math.max(0, from);
  const end = Math.min(to, text.codes.length);
  const wanted = readingOrder(text).filter((index) => index >= start && index < end);
  let out = "";
  for (let at = 0; at < wanted.length; at += 4096) {
    out += String.fromCodePoint(
      ...wanted.slice(at, at + 4096).map((index) => text.codes[index] ?? 0),
    );
  }
  return out;
}

/**
 * Whether any two of the page's lines sit beside each other.
 *
 * Which is what "this page has more than one column" means in the only terms
 * the geometry offers: lines of a single column never share a band, because two
 * runs that did would have been merged into one line.
 *
 * Exists for `viewer_check.py`, whose drag-ordering check compares text taken
 * from high on the page against text from lower down and expects the first to
 * come earlier. That premise is false on **any** multi-column page --- the top
 * of column two is read after the bottom of column one --- whether or not the
 * file was written in a sensible order, so the check has to stand aside rather
 * than report the layout as a defect.
 */
export function hasSideBySideLines(text: PageText): boolean {
  const axes = axesFor(text.quarter_turns);
  const bands = readingLines(text).map((line) => extentsOf(line.box, axes));
  for (let a = 0; a < bands.length; a++) {
    for (let b = a + 1; b < bands.length; b++) {
      if (sameBand(bands[a] as Extents, bands[b] as Extents)) return true;
    }
  }
  return false;
}
