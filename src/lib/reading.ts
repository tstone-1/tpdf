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
 * ## Unless the document says, in which case believe it
 *
 * A tagged PDF carries its reading order explicitly, and `structure.rs` reads it
 * into runs of the same character indices used here. Where those runs cover the
 * page, they are the order and everything below is skipped --- inferring an order
 * from boxes when the producer has stated one is guessing at an answer that is
 * written down. {@link usableRuns} is the whole of that decision, and
 * {@link readingLines} is the only place either route is chosen, so the
 * accessibility tree and the copy path cannot disagree about which one ran.
 *
 * The tags decide the *order of the blocks* and the geometry still decides where
 * the lines inside one fall. A tagged run is a paragraph, and a screen reader is
 * handed lines, so the two answer different questions and both are needed.
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
  type TaggedRun,
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
 * A group of lines meant to be read together, and what the document calls it.
 *
 * The unit the *tags* work in --- a paragraph, a heading, a table cell --- where
 * {@link ReadingLine} is the unit a reader moves through. Both are needed and
 * they are different questions, which is why this exists rather than a `tag` on
 * the line: a consumer that wants "the heading" wants one element, and a consumer
 * that wants "the next line" wants ten.
 */
export interface ReadingBlock {
  /**
   * The element's type as the document spells it, or `null` where there is none.
   *
   * `null` is not "unknown", it is **"inferred"**: the block came out of the
   * geometry, so its boundaries are this file's guess rather than the producer's
   * statement. A consumer that treats an inferred boundary as a real one is
   * asserting something nobody claimed --- which is why `a11y.ts` reads a tagged
   * block as one element and an inferred one line by line.
   */
  tag: string | null;
  lines: ReadingLine[];
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

/**
 * How short a box has to be, against the one it touches, to be a mark on it.
 *
 * Half. A comma is about a third of a line's height and a space PDFium reports
 * as a hundredth; a line of text is never half the height of the line beside it,
 * because it is set in the same type.
 */
const SHORT_MARK = 0.5;

/**
 * Whether two boxes share a line.
 *
 * Mostly "they overlap by more than half the shorter one's extent", which is the
 * right test for two boxes of comparable height --- and it is wrong in one
 * direction that matters: a **comma**. PDFium reports `,` as a box that starts
 * inside the line and drops below its baseline, roughly a third of the line's
 * height, so it overlaps by well under half of *itself*. Measured on
 * `testdata/tagged.pdf`: letters banded at 227.41--236.13 and the comma at
 * 234.80--237.69, an overlap of 46%.
 *
 * The consequence was not a slightly wrong box. The comma opened a band of its
 * own, and every space on the line --- which PDFium reports 0.01 pt tall, sitting
 * on the baseline --- then matched *that* band rather than the letters', because
 * a space overlaps anything it touches by 100% of itself. So one line came back
 * as two: `inthemaincolumnandclosesthesection`, and a second "line" holding a
 * comma, a full stop and six spaces. Read aloud, and copied, exactly like that.
 *
 * So a box too short to be a line of text joins the line it touches. That is a
 * statement about type rather than a tuned constant: a mark a third the height of
 * the letters beside it is a mark on their line, and nothing set in the same type
 * is half the height of the line above it.
 *
 * Found by the tagged fixture and **not caused by the tags** --- the geometric
 * path produced the same two lines, on a fixture that has existed for one day.
 * The corpus that was going to catch this was always going to be a new one:
 * every other one is generated from words with no punctuation in them.
 */
function sameBand(a: Extents, b: Extents): boolean {
  const overlap = Math.min(a.crossEnd, b.crossEnd) - Math.max(a.crossStart, b.crossStart);
  if (overlap <= 0) return false;
  const heights = [a.crossEnd - a.crossStart, b.crossEnd - b.crossStart];
  const shorter = Math.min(...heights);
  if (shorter <= 0) return false;
  if (shorter < Math.max(...heights) * SHORT_MARK) return true;
  return overlap / shorter > 0.5;
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
 * A nonspacing or enclosing combining mark: `\p{Mn}` or `\p{Me}`.
 *
 * Not a character in its own right. It has no advance width, it is drawn over
 * the character before it, and the two are one grapheme --- so the reader who
 * typed `resumé` typed one thing, however many code points the producer stored.
 */
const COMBINING = /^[\p{Mn}\p{Me}]$/u;

/**
 * Whether a code is a mark that belongs to the character before it.
 *
 * The alternative was a geometric rule, and it cannot work. An acute accent sits
 * **above the x-height**, so on a word with no ascender its box does not touch
 * the band it belongs to at all: measured on `testdata/multilingual.pdf`, U+0301
 * at 718.64--721.30 against an `e` at 707.80--717.68, which is a 0.96 pt gap and
 * no overlap. {@link sameBand} requires overlap before it will consider anything,
 * and it is right to --- the short-mark clause exists for a comma that *dips into*
 * the line, and loosening it to bridge a gap would start joining a mark to the
 * line above it in tightly leaded text.
 *
 * So `resumé` decomposed came back as three lines: `resume`, the accent alone,
 * and the rest. Read aloud, and copied, exactly like that. `café` did **not**,
 * which is why this needed a second fixture line: the `f` reaches up to 721.30
 * and drags the band into contact with the accent, so a word with an ascender
 * hides the defect completely.
 *
 * Unicode already answers the question the geometry cannot, and it answers it
 * about the *character* rather than about where the producer drew it.
 */
function combining(code: number): boolean {
  return COMBINING.test(String.fromCodePoint(code));
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
    // A mark is attached the same way an unplaced character is --- it stays in
    // the ranges, so nothing is dropped --- and its box is folded into the
    // character it decorates, so the line's box still covers the accent. A mark
    // with nothing before it has no base to join and keeps its own band, which
    // is the honest answer for a document that starts a page with one.
    const mark = last >= 0 && combining(text.codes[index] ?? 0);
    if (!placed(box) || mark) {
      const at = trailing.get(last) ?? [];
      at.push(index);
      trailing.set(last, at);
      if (mark && placed(box)) {
        const base = items[items.length - 1];
        if (base) {
          absorb(base.box, box);
          base.extents = extentsOf(base.box, axes);
        }
      }
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
  return readingBlocks(text).flatMap((block) => block.lines);
}

/**
 * The page's blocks, in reading order, each carrying its lines.
 *
 * Where {@link readingLines} flattens. The two share every line of logic, so
 * there is no second ordering to drift --- and the split is what lets one
 * consumer take the page as a sequence of lines and another as a sequence of
 * paragraphs without either of them re-deriving the order.
 */
export function readingBlocks(text: PageText): ReadingBlock[] {
  const axes = axesFor(text.quarter_turns);
  const gap = cutWidth(text, axes);
  const fragments = fragmentsOf(text, axes, gap);
  const tagged = usableRuns(text);
  if (tagged) {
    return ownership(text, tagged).map((owned, at) => ({
      tag: tagged[at]?.tag ?? null,
      lines: linesOf(within(text, fragments, owned), axes),
    }));
  }
  return blocksOf(fragments, axes, gap).map((block) => ({
    tag: null,
    lines: linesOf(block, axes),
  }));
}

/**
 * One block's fragments as lines, in the order they are read.
 *
 * Shared by both routes, which is the point: a block from the geometry and a
 * block from the tags are the same thing --- a set of fragments meant to be read
 * together --- so the *ordering within* one is decided in one place. Fragments in
 * the same band are one line, because a line broken by a wide word space is not
 * two lines.
 */
function linesOf(block: readonly Fragment[], axes: Axes): ReadingLine[] {
  const ordered = [...block].sort((a, b) => {
    const [ea, eb] = [extentsOf(a.box, axes), extentsOf(b.box, axes)];
    return ea.crossStart - eb.crossStart || ea.alongStart - eb.alongStart;
  });
  const lines: ReadingLine[] = [];
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
  return lines;
}

/**
 * The document's own reading order, if it covers the page well enough to use.
 *
 * The whole of the decision between the tags and the geometry, in one place and
 * exported so a check can assert *which route ran* rather than infer it from an
 * order that both routes might agree on.
 *
 * The condition is that every **visible** character is claimed by some run.
 * Not every character: PDFium synthesises a separator between two text objects
 * and one falling between two elements belongs to neither, so requiring all of
 * them would reject every real tagged document. And not "most": a producer that
 * tagged three paragraphs of four would otherwise have the fourth silently
 * disappear from what a screen reader reads, which is worse than ignoring its
 * tags altogether --- the geometry at least orders *all* the text, and is only
 * wrong where the tags disagree with it.
 *
 * Returns `null` rather than an empty array for "use the geometry", so that a
 * caller cannot accidentally treat the untagged case as a page with no text.
 */
export function usableRuns(text: PageText): TaggedRun[] | null {
  const runs = text.runs;
  if (!runs || runs.length === 0) return null;

  const claimed = new Uint8Array(text.codes.length);
  for (const run of runs) {
    const to = Math.min(run.end, text.codes.length);
    for (let index = Math.max(0, run.start); index < to; index++) claimed[index] = 1;
  }
  for (let index = 0; index < text.codes.length; index++) {
    if (claimed[index]) continue;
    const code = text.codes[index] ?? 0;
    // A character with no box is one PDFium placed nowhere, which is invisible
    // whatever its code says.
    if (isVisible(code) && placed(charQuad(text, index))) return null;
  }
  return runs;
}

/** Whether a code is something a reader would see, rather than space. */
function isVisible(code: number): boolean {
  const char = String.fromCodePoint(code);
  return char.trim().length > 0;
}

/**
 * Which run every character belongs to, as one entry per run.
 *
 * The runs claim ranges of characters and they do not claim all of them: PDFium
 * synthesises a separator between two text objects, and one falling *between*
 * two elements belongs to neither. {@link usableRuns} tolerates that, correctly
 * --- an unclaimed space is not a hole in the reading order --- but a consumer
 * that then emitted only the claimed characters would **drop** those separators,
 * and dropping the one between two paragraphs is how a page comes back six
 * characters shorter than the page.
 *
 * So every character is given an owner: its own run where it has one, and
 * otherwise the run of the nearest character before it, which is the same rule
 * `fragmentsOf` uses to re-attach a character PDFium placed nowhere. A leading
 * unclaimed character has nothing before it and takes the first owner that
 * follows.
 *
 * The result is a partition, and that is the invariant worth stating: the tagged
 * order is a **permutation of every character index**, exactly as the geometric
 * one is. A reading order that quietly holds less than the page is worse than one
 * that puts the page in a poorer order.
 *
 * Returned as **one list of indices per run**, not as one owner per character
 * with the run named by position. The leaner shape was written and reverted: it
 * couples the owner array to the run list by index, and a mutation that sorted
 * the runs before mapping over them then changed *nothing at all*, because the
 * callback used the positional index and never the run. A wrong edit that
 * compiles to a no-op is indistinguishable from a test that cannot fail --- so
 * the coupling is carried in the value instead, where there is no order to keep
 * in step, and the `Set` per run is the price.
 */
function ownership(text: PageText, runs: readonly TaggedRun[]): number[][] {
  const owner = new Int32Array(text.codes.length).fill(-1);
  runs.forEach((run, at) => {
    const to = Math.min(run.end, owner.length);
    for (let index = Math.max(0, run.start); index < to; index++) owner[index] = at;
  });
  let last = -1;
  for (let index = 0; index < owner.length; index++) {
    if (owner[index] === -1) owner[index] = last;
    else last = owner[index] as number;
  }
  // Backwards for anything before the first claimed character, which the forward
  // pass could only leave at -1.
  let next = runs.length > 0 ? 0 : -1;
  for (let index = owner.length - 1; index >= 0; index--) {
    if (owner[index] === -1) owner[index] = next;
    else next = owner[index] as number;
  }
  const owned: number[][] = runs.map(() => []);
  for (let index = 0; index < owner.length; index++) {
    const at = owner[index] as number;
    if (at >= 0) owned[at]?.push(index);
  }
  return owned;
}

/**
 * The fragments of one run, clipped to the characters {@link ownership} gave it.
 *
 * A fragment is a geometric run of characters and a run is a set of indices, and
 * nothing makes them nest: a fragment can straddle a boundary between two
 * elements --- two tagged words side by side on one line are one fragment --- so
 * it is clipped rather than assigned to whichever element its first character
 * falls in. The box is rebuilt from the characters that survive, because the
 * original covers text that is now in a different block and a line's band would
 * be measured from it.
 */
function within(
  text: PageText,
  fragments: readonly Fragment[],
  owned: readonly number[],
): Fragment[] {
  const mine = new Set(owned);
  const out: Fragment[] = [];
  for (const fragment of fragments) {
    const indices: number[] = [];
    let box: Quad | null = null;
    for (const range of fragment.ranges) {
      for (let index = range.from; index < range.to; index++) {
        if (!mine.has(index)) continue;
        indices.push(index);
        const at = charQuad(text, index);
        if (!placed(at)) continue;
        if (box) absorb(box, at);
        else box = { ...at };
      }
    }
    if (indices.length === 0) continue;
    // Every character of this piece unplaced: it carries text and no geometry,
    // so it keeps its place in the run and takes a degenerate box, exactly as
    // `fragmentsOf` does for the same case.
    out.push({ ranges: rangesOf(indices), box: box ?? emptyBox() });
  }
  // Ordered along the run's own index range, so that a block whose fragments
  // came out of the page-wide banding in a different order reads in the order
  // its characters were written. `linesOf` re-orders by position within the
  // block, which is what decides its lines; this only makes that input stable.
  out.sort((a, b) => (a.ranges[0]?.from ?? 0) - (b.ranges[0]?.from ?? 0));
  return out;
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
