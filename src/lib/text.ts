/**
 * A page's characters, cached, and the geometry that turns them into a
 * selection.
 *
 * The backend (`src-tauri/src/text.rs`) sends one Unicode scalar per PDFium
 * character index plus four numbers per box, and deliberately does not send a
 * string --- `FPDFText_GetText` extracts UCS-2 and drops characters it cannot
 * represent, which desynchronises the string from the indices the boxes are
 * keyed by, on exactly the documents where nobody would notice. Everything here
 * therefore works in character indices, and a string is built only at the moment
 * one is handed to the clipboard.
 *
 * Boxes arrive in PDF points, y downwards from the page's top-left corner ---
 * the flip happens once, in Rust, against the same page height the renderer maps
 * a tile with. `bin/text-probe --mode align` checks that convention against
 * pixels rather than against reasoning, and carries a control that fails the run
 * if the *wrong* convention would also pass.
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * One run of text the document's own tags claim, as half-open character indices.
 *
 * Mirrors `structure::TaggedRun`. See `src-tauri/src/structure.rs` for why a
 * character index is the right currency here rather than a second index space.
 */
export interface TaggedRun {
  /** The element's type as the document spells it: `P`, `H1`, `Note`, `TD`. */
  tag: string;
  /** The element's ancestry, deepest last: `["Document", "Sect", "P"]`. */
  path: string[];
  start: number;
  end: number;
}

/** A page's characters and where they sit, in PDF points from the top-left. */
export interface PageText {
  /** One Unicode scalar per character index. */
  codes: number[];
  /** Four values per character: left, top, right, bottom. */
  boxes: number[];
  width_pt: number;
  height_pt: number;
  /**
   * Quarter-turns clockwise the page is displayed rotated by: 0, 1, 2 or 3.
   *
   * The boxes have already been turned in Rust, so nothing here has to rotate
   * anything. What this decides is which axis *separates lines*: on a page read
   * left-to-right that is the vertical one, and on a `/Rotate 90` page --- what a
   * scanner emits --- the text runs down the screen and lines advance sideways,
   * so grouping by vertical overlap puts every character on a line of its own.
   * A screen reader then reads the page letter by letter, which is what it did.
   */
  quarter_turns: number;
  extract_ms: number;
  /**
   * The document's own reading order, where it says one.
   *
   * Absent or empty on an untagged page, and absent when the structure walk was
   * bounded --- so a non-empty value is a *complete* claim about the page, which
   * is what lets `reading.ts` decide between it and the geometry on one
   * condition. Optional because the backend omits the field when it is empty,
   * which is most pages.
   */
  runs?: TaggedRun[];
}

/** Whether lines on this page are separated horizontally rather than vertically. */
export function linesRunSideways(text: PageText): boolean {
  return text.quarter_turns % 2 === 1;
}

/**
 * The same page seen through a view rotated `turns` quarter-turns clockwise.
 *
 * Rotating the view is not an operation on the document --- the file is
 * untouched --- so the boxes are turned here rather than re-extracted, and a
 * rotation costs no round trip.
 *
 * `quarter_turns` accumulates, which is what keeps {@link linesRunSideways}
 * right: an upright page looked at sideways reads down the screen exactly as a
 * `/Rotate 90` page does, and the line-grouping axis has to follow the view
 * rather than the document. Note the *ranges* {@link linesOf} returns are
 * unchanged by this --- a rotation is an isometry, so the grouping is invariant
 * --- which is why a screen reader still hears the page in its own order.
 *
 * The mirror of `text::turn_device` in Rust, where the composition rule the two
 * of them share is asserted against the already-verified page mapping.
 */
export function turnedView(text: PageText, turns: number): PageText {
  const quarters = ((turns % 4) + 4) % 4;
  if (quarters === 0) return text;

  const { width_pt: width, height_pt: height } = text;
  const boxes = new Array<number>(text.boxes.length);
  for (let at = 0; at < text.boxes.length; at += 4) {
    const quad = {
      left: text.boxes[at] ?? 0,
      top: text.boxes[at + 1] ?? 0,
      right: text.boxes[at + 2] ?? 0,
      bottom: text.boxes[at + 3] ?? 0,
    };
    // Four zeroes means "PDFium gave this character no box", and turning that
    // would invent one in a corner --- which `isPlaced` would then believe.
    const turned = isPlaced(quad) ? turnQuad(quad, quarters, width, height) : quad;
    boxes[at] = turned.left;
    boxes[at + 1] = turned.top;
    boxes[at + 2] = turned.right;
    boxes[at + 3] = turned.bottom;
  }

  const sideways = quarters % 2 === 1;
  const view: PageText = {
    codes: text.codes,
    boxes,
    width_pt: sideways ? height : width,
    height_pt: sideways ? width : height,
    quarter_turns: (text.quarter_turns + quarters) % 4,
    extract_ms: text.extract_ms,
  };
  // The tags come through untouched: a run is a range of character indices and
  // a rotation renumbers no characters. Dropping them here is invisible rather
  // than broken, which is what makes it worth stating --- `reading.ts`'s
  // `usableRuns` reads an absent value as "this page is untagged" and falls
  // back to the XY-cut, so one quarter turn would demote a tagged page to
  // geometric order for the screen reader and for `Selection.text` both.
  // Assigned rather than spread because the field is absent on most pages and
  // `exactOptionalPropertyTypes` distinguishes absent from `undefined`.
  if (text.runs) view.runs = text.runs;
  return view;
}

/**
 * Turns one device-space box by `turns` quarter-turns clockwise.
 *
 * `width`/`height` are the page's displayed size *before* the turn; a quarter
 * turn swaps them, so the caller's page box swaps with it.
 */
export function turnQuad(quad: Quad, turns: number, width: number, height: number): Quad {
  switch (((turns % 4) + 4) % 4) {
    case 1:
      return { left: height - quad.bottom, top: quad.left, right: height - quad.top, bottom: quad.right };
    case 2:
      return {
        left: width - quad.right,
        top: height - quad.bottom,
        right: width - quad.left,
        bottom: height - quad.top,
      };
    case 3:
      return { left: quad.top, top: width - quad.right, right: quad.bottom, bottom: width - quad.left };
    default:
      return quad;
  }
}

/**
 * Undoes {@link turnQuad}: a quad in the turned page, back in the page's own.
 *
 * `width`/`height` are the **turned** page's size --- the one the quad is
 * measured in --- which is the opposite convention to `turnQuad`'s and is what
 * makes the pair compose to the identity. A quarter turn swaps the page's
 * sides, so the caller that turned a 612x792 page by one and got a 792x612 one
 * passes 792 and 612 back here.
 *
 * It exists because every rectangle in this application travels one way ---
 * from the file, through the crop, through the turn, onto the screen --- and a
 * reader who *draws* one travels the other. There is no third direction, and a
 * second hand-written inverse at the one call site that needs it is how the two
 * would come to disagree at a quarter turn only, which no unrotated corpus can
 * see.
 */
export function unturnQuad(
  quad: Quad,
  turns: number,
  width: number,
  height: number,
): Quad {
  return turnQuad(quad, -turns, width, height);
}

/** A position between two characters, which is what a caret is. */
export interface Caret {
  page: number;
  /** 0 to `codes.length`, inclusive at both ends. */
  index: number;
}

/** A rectangle in PDF points, y downwards from the page's top-left. */
export interface Quad {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Characters held across every cached page before the least recently used are
 * dropped.
 *
 * A character costs about 40 bytes here --- one code point plus four box
 * coordinates, each a JavaScript number --- so this is roughly 16 MB. A page of
 * ordinary prose is two to three thousand characters, which makes the bound
 * something like 150 pages of a book and rather more of a technical manual.
 *
 * Counted in characters rather than in pages because characters are what the
 * memory actually tracks, and page size varies by three orders of magnitude
 * across this repository's own corpus: `text-base14` has 177 characters a page
 * and `vector-heavy` has none at all. A bound of "128 pages" would mean sixteen
 * megabytes on one document and a few kilobytes on another.
 */
export const TEXT_CACHE_CHARS = 400_000;

/**
 * Pages kept whatever they cost, so the bound cannot evict what is on screen.
 *
 * Without it a single page larger than the whole budget would empty the cache
 * on arrival and then be dropped itself, and a viewport spanning two pages would
 * re-fetch both on every frame --- a bound that turns a memory concern into an
 * IPC storm. Eight covers a viewport and its overscan.
 */
export const TEXT_CACHE_FLOOR = 8;

/**
 * Per-page text, fetched once and dropped when the cache outgrows its bound.
 *
 * Extraction measured 1.4 ms on a dense page and the page's own load dominates
 * on a complex one (`text-probe --mode extract`), so the cache is about avoiding
 * an IPC round trip during a drag, not about avoiding PDFium. Re-fetching an
 * evicted page is therefore cheap; holding every page a reader ever visits is
 * what is not.
 *
 * Search is what made a bound necessary, and not in the obvious way. The
 * matching happens in Rust and only the hits cross, so a whole-document scan
 * never touches this at all. What grows it is a reader **stepping through** the
 * hits: each jump loads the page it lands on to know where to scroll, so 5,712
 * matches over 775 pages is 775 pages of characters retained by a reader holding
 * down Cmd-G.
 *
 * Eviction is least-recently-used, with `peek` counting as a use --- it is what
 * the paint path calls, so the pages on screen are continuously the youngest and
 * are the last things that could be dropped. The newest page is never evicted
 * while any other remains, because the scan starts at the old end and a page
 * just inserted is at the young one.
 *
 * **Every `page` below is a page of the *file*, not a slot on screen.** The two
 * were the same number until a page could be deleted, and the distinction is the
 * subject of `pages.ts`. It belongs this way round: a page's text is a property
 * of the document, so a deletion above it must not make it be fetched again ---
 * and `viewer.ts` translates at the four call sites rather than this class
 * holding a copy of the order.
 */
export class TextCache {
  private readonly doc: number;
  /**
   * Cached pages, in **least-recently-used order**.
   *
   * A `Map` iterates in insertion order and re-inserting a key moves it to the
   * end, which is the whole of the LRU bookkeeping: {@link touch} deletes and
   * re-sets, and the eviction scan takes keys from the front.
   */
  private readonly pages = new Map<number, PageText>();
  /**
   * Each page's crop box, sent with every extraction that has one.
   *
   * Held here rather than passed to {@link load}, because the cache's whole job
   * is to answer without asking --- a caller that had to supply the crop could
   * supply a stale one and be served a cached answer measured under a different
   * box, which is the one thing the cache must not do.
   */
  private readonly crops = new Map<
    number,
    readonly [number, number, number, number]
  >();
  private readonly pending = new Map<number, Promise<PageText | null>>();
  /** Characters across `pages`, maintained rather than recomputed. */
  private chars = 0;
  /**
   * Pages already turned into the current view, so a drag does not re-turn a
   * few thousand boxes on every pointer move. Dropped whole when the view
   * rotates, which is the only thing that can invalidate it.
   */
  private readonly turned = new Map<number, PageText>();
  private turns = 0;

  /**
   * Quarter-turns an edit has applied to a page, for the pages that have one.
   *
   * A map rather than an array because it is empty for every document nobody
   * has edited, which is almost all of them, and because it is keyed by the same
   * page number the two caches above are.
   */
  private readonly extra = new Map<number, number>();

  constructor(doc: number) {
    this.doc = doc;
  }

  /**
   * Rotates the view every reader of this cache sees.
   *
   * Everything downstream --- the caret, the highlight runs, the accessibility
   * lines --- works in the space the pointer and the tiles are in, so the turn
   * belongs here rather than at each of those call sites. What is cached is the
   * document's own text; what is handed out is the view of it.
   */
  setTurns(turns: number): void {
    const next = ((turns % 4) + 4) % 4;
    if (next === this.turns) return;
    this.turns = next;
    this.turned.clear();
  }

  /**
   * Records that an edit turned one page.
   *
   * Composed with the view's rotation in {@link view}, so the caret, the
   * highlight runs and the accessibility lines land on a page-turned page for
   * the same reason they land on a view-turned one --- there is one turn in
   * force and it is the sum. Without this the tiles would be turned and the text
   * over them would not, which does not look like a bug in the text layer: it
   * looks like selection being slightly wrong on one page.
   *
   * Drops **this page's** turned view rather than all of them, which is the
   * difference from {@link setTurns}: a view rotation invalidates every page and
   * an edit invalidates one.
   */
  setPageTurns(page: number, turns: number): void {
    const next = ((turns % 4) + 4) % 4;
    if ((this.extra.get(page) ?? 0) === next) return;
    if (next === 0) this.extra.delete(page);
    else this.extra.set(page, next);
    this.turned.delete(page);
  }

  /**
   * Records a page's crop box, dropping what was extracted under the old one.
   *
   * **Character boxes are measured from the displayed page's corner**, and a
   * crop moves that corner, so an extraction made under a different box is not
   * stale in the usual sense --- it is measured in another space. Dropped rather
   * than translated: the backend re-extracts under the new box and the answer is
   * then the renderer's own, which is what everything else here is built on.
   *
   * Drops **this page's** entry, the same scope {@link setPageTurns} uses and
   * for the same reason: a crop is one page's, unlike a view rotation.
   */
  setPageCrop(
    page: number,
    crop: readonly [number, number, number, number] | undefined,
  ): void {
    const was = this.crops.get(page);
    const same =
      was === crop ||
      (was !== undefined &&
        crop !== undefined &&
        was.every((value, at) => value === crop[at]));
    if (same) return;
    if (crop === undefined) this.crops.delete(page);
    else this.crops.set(page, crop);
    this.forget(page);
  }

  /** Drops one page's extraction, and everything derived from it. */
  private forget(page: number): void {
    const held = this.pages.get(page);
    if (held) {
      this.chars -= held.codes.length;
      this.pages.delete(page);
    }
    this.turned.delete(page);
  }

  /** Pages held. For the check harness and the tests. */
  get cachedPages(): number {
    return this.pages.size;
  }

  /** Characters held across those pages. For the check harness and the tests. */
  get cachedChars(): number {
    return this.chars;
  }

  /**
   * Turned views held. For the tests.
   *
   * Exposed because it is the only way to see this half of the cache at all: a
   * turned view left behind by an eviction is unreachable through {@link peek},
   * which asks `pages` first and never reaches `turned` for a page that is gone.
   * The entry would sit there for the life of the document, invisible, which is
   * exactly a leak that no behavioural assertion can fail on.
   */
  get retainedViews(): number {
    return this.turned.size;
  }

  /** A page's text if it has already arrived, without asking for it. */
  peek(page: number): PageText | null {
    const text = this.pages.get(page);
    // Counted as a use. This is the paint path, so what is on screen stays the
    // youngest entry and cannot be evicted out from under the frame drawing it.
    if (text !== undefined) this.touch(page);
    return this.view(page, text);
  }

  /**
   * A page's text as the *document* displays it, before any turn a reader added.
   *
   * {@link peek} answers in view space: the page's own `/Rotate`, plus the
   * reader's rotation, plus whatever quarter turns an edit has applied. That is
   * right for drawing and wrong for anything that has to outlive the view --- a
   * highlight is stored against the page, not against how somebody was holding
   * it, so the quads it is built from must come from here.
   *
   * The alternative was to take the view's quads and turn them back, which is a
   * second inverse of {@link turnedView} living in the caller. Reading the
   * untuned boxes instead means there is nothing to invert: the character
   * *indices* are the same in both, because turning a page renumbers nothing.
   *
   * Counts as a use, as {@link peek} does, so a page a mark is being made on
   * cannot be evicted between the two calls.
   */
  peekUnturned(page: number): PageText | null {
    const text = this.pages.get(page);
    if (text === undefined) return null;
    this.touch(page);
    return text;
  }

  /** Moves a page to the young end of the eviction order. */
  private touch(page: number): void {
    const text = this.pages.get(page);
    if (text === undefined) return;
    this.pages.delete(page);
    this.pages.set(page, text);
  }

  /**
   * Takes a newly arrived page into the cache and drops the oldest until it fits.
   *
   * Only ever called for a page the cache does **not** hold: {@link load}
   * returns from `pages` before it issues a request, and `pending` dedupes two
   * callers racing for the same one. A `chars` correction for an entry being
   * replaced was written here first and no mutation of it could go red, which is
   * what says it was unreachable rather than merely untested.
   */
  private remember(page: number, text: PageText): void {
    this.pages.set(page, text);
    this.chars += text.codes.length;

    // Deleting during iteration is defined for a Map, and iterating from the
    // front is what makes this least-recently-used rather than arbitrary.
    for (const oldest of this.pages.keys()) {
      if (this.chars <= TEXT_CACHE_CHARS || this.pages.size <= TEXT_CACHE_FLOOR) break;
      this.chars -= this.pages.get(oldest)?.codes.length ?? 0;
      this.pages.delete(oldest);
      // The turned view is derived from the page and keyed the same way. Leaving
      // it behind would move the leak rather than fix it, and on a rotated
      // document it is the *larger* of the two maps.
      this.turned.delete(oldest);
    }
  }

  /** The cached view of a page, turning and memoising it on first use. */
  private view(page: number, text: PageText | undefined): PageText | null {
    if (!text) return null;
    const turns = this.turns + (this.extra.get(page) ?? 0);
    if (turns % 4 === 0) return text;

    const already = this.turned.get(page);
    if (already) return already;
    const turned = turnedView(text, turns);
    this.turned.set(page, turned);
    return turned;
  }

  /**
   * Fetches a page's text, or returns the cached copy.
   *
   * Concurrent callers for the same page share one request --- a drag crossing a
   * page boundary asks on every frame until the answer lands, and without this
   * that is one extraction per frame queued behind the tiles.
   */
  async load(page: number): Promise<PageText | null> {
    const cached = this.pages.get(page);
    if (cached) {
      this.touch(page);
      return this.view(page, cached);
    }

    const existing = this.pending.get(page);
    if (existing) return existing;

    const request = invoke<PageText>("page_text", {
      doc: this.doc,
      page,
      crop: this.crops.get(page) ?? null,
    })
      .then((text) => {
        this.remember(page, text);
        // Turned on the way out rather than on the way in, so a rotation that
        // lands while the request is in flight is still honoured: the view is
        // read at the moment it is asked for, not at the moment it was sent.
        return this.view(page, text);
      })
      .catch(() => null)
      .finally(() => this.pending.delete(page));

    this.pending.set(page, request);
    return request;
  }
}

/** The box of one character. */
export function charQuad(text: PageText, index: number): Quad {
  const at = index * 4;
  return {
    left: text.boxes[at] ?? 0,
    top: text.boxes[at + 1] ?? 0,
    right: text.boxes[at + 2] ?? 0,
    bottom: text.boxes[at + 3] ?? 0,
  };
}

/** Whether a character has no box, which PDFium reports as four zeroes. */
function isPlaced(quad: Quad): boolean {
  return quad.right > quad.left && quad.bottom > quad.top;
}

/**
 * The caret position nearest a point, in PDF points from the page's top-left.
 *
 * Nearest rather than containing: a click in the margin, in the leading between
 * two lines, or past the end of a line has to land somewhere, and landing
 * nowhere is what makes a selection feel broken. Distance is measured to the
 * box, so a point inside one is at distance zero and wins outright.
 *
 * The vertical term is weighted, because a page is much wider than a line is
 * tall: without it, a click below the last word of a long line is closer to a
 * character at the far end of the *next* line than to the one directly above it.
 */
export function caretAt(text: PageText, x: number, y: number): number {
  const best = nearestChar(text, x, y);
  const sideways = linesRunSideways(text);

  if (best < 0) return 0;
  const quad = charQuad(text, best);
  // Past the middle of the glyph, along the direction the text reads, means the
  // caret belongs after it --- which is what makes a drag include the character
  // under the pointer.
  return sideways
    ? y > (quad.top + quad.bottom) / 2
      ? best + 1
      : best
    : x > (quad.left + quad.right) / 2
      ? best + 1
      : best;
}

/**
 * The index of the character nearest a point, or -1 if the page places none.
 *
 * Split out of {@link caretAt} rather than duplicated, because a caret and a
 * character are different answers to the same search and only one of them is
 * right per question. A caret is a position *between* characters, so
 * double-clicking the last glyph of a word yields the caret after it --- which
 * names the space, and a word selection built on that selects the gap rather
 * than the word. Selecting by unit therefore asks which character was clicked;
 * dragging a caret asks where the pointer fell between two.
 */
export function nearestChar(text: PageText, x: number, y: number): number {
  let best = -1;
  let bestDistance = Infinity;
  // The weight belongs on the axis that separates lines, not on `y`. On a
  // rotated page those are different axes, and weighting the wrong one makes a
  // click in the margin land a line away --- the same failure the weight exists
  // to prevent, moved ninety degrees.
  const sideways = linesRunSideways(text);

  for (let index = 0; index < text.codes.length; index++) {
    const quad = charQuad(text, index);
    if (!isPlaced(quad)) continue;

    const dx = Math.max(quad.left - x, 0, x - quad.right);
    const dy = Math.max(quad.top - y, 0, y - quad.bottom);
    const [along, across] = sideways ? [dy, dx] : [dx, dy];
    const distance = along * along + (across * ACROSS_LINE_WEIGHT) ** 2;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  }

  return best;
}

/**
 * How much more a point of across-the-lines distance counts than along one.
 *
 * Untuned, and chosen to be obviously large enough that a neighbouring line
 * never wins over the line the pointer is on. Lower it and clicks in the margin
 * start landing a line away.
 */
const ACROSS_LINE_WEIGHT = 8;

/**
 * Merges a character range into one rectangle per run of text on a line.
 *
 * A quad per character would be thousands of rectangles for a page and would
 * show the gaps between glyphs as stripes through the highlight. Characters join
 * a run while they overlap it vertically; anything else --- a new line, a
 * superscript, a rotated run --- starts a new one.
 *
 * Runs are also unioned horizontally, so an inter-word space with no box of its
 * own does not split the highlight in two.
 */
export function runsFor(text: PageText, from: number, to: number): Quad[] {
  const runs: Quad[] = [];
  let current: Quad | null = null;
  const sideways = linesRunSideways(text);

  for (let index = Math.max(0, from); index < Math.min(to, text.codes.length); index++) {
    const quad = charQuad(text, index);
    if (!isPlaced(quad)) continue;

    if (current && onSameLine(current, quad, sideways)) {
      current.left = Math.min(current.left, quad.left);
      current.right = Math.max(current.right, quad.right);
      current.top = Math.min(current.top, quad.top);
      current.bottom = Math.max(current.bottom, quad.bottom);
      continue;
    }
    current = { ...quad };
    runs.push(current);
  }

  return runs;
}

/**
 * Splits a page's characters into lines, as index ranges.
 *
 * The same vertical-overlap rule {@link runsFor} uses, applied to the whole page
 * instead of to a selection --- a screen reader given one 2,700-character blob
 * per page has no way to move by line, which is most of how a document is read.
 *
 * Ranges are half-open, contiguous and in index order, so concatenating their
 * text reproduces the page. Characters with no box --- the trailing space of a
 * line, typically --- extend the run they follow rather than starting one, which
 * is what keeps the ranges contiguous.
 */
export function linesOf(text: PageText): { from: number; to: number }[] {
  const lines: { from: number; to: number }[] = [];
  let current: Quad | null = null;
  const sideways = linesRunSideways(text);

  for (let index = 0; index < text.codes.length; index++) {
    const quad = charQuad(text, index);
    const last = lines[lines.length - 1];

    if (!isPlaced(quad)) {
      if (last) last.to = index + 1;
      else lines.push({ from: index, to: index + 1 });
      continue;
    }

    if (current && last && onSameLine(current, quad, sideways)) {
      current.top = Math.min(current.top, quad.top);
      current.bottom = Math.max(current.bottom, quad.bottom);
      current.left = Math.min(current.left, quad.left);
      current.right = Math.max(current.right, quad.right);
      last.to = index + 1;
      continue;
    }
    current = { ...quad };
    lines.push({ from: index, to: index + 1 });
  }

  return lines;
}

/**
 * Whether two boxes share most of their extent across the lines, i.e. are on one.
 *
 * `sideways` picks the axis. It is the page's rotation rather than anything
 * inferred from the boxes, which is a real limitation and worth stating: a
 * *rotated run* inside an otherwise upright page --- a sideways table header ---
 * is still split character by character. That was true before and is unchanged;
 * what is fixed is the whole-page case, which is what a scanner produces.
 */
function onSameLine(a: Quad, b: Quad, sideways: boolean): boolean {
  const [aStart, aEnd, bStart, bEnd] = sideways
    ? [a.left, a.right, b.left, b.right]
    : [a.top, a.bottom, b.top, b.bottom];
  const overlap = Math.min(aEnd, bEnd) - Math.max(aStart, bStart);
  const shorter = Math.min(aEnd - aStart, bEnd - bStart);
  return shorter > 0 && overlap / shorter > 0.5;
}

/** A half-open range of character indices. */
export interface IndexRange {
  from: number;
  /** Exclusive. */
  to: number;
}

/**
 * What class of character this is, for the purpose of finding a word's edges.
 *
 * Three classes rather than two, because the run a double-click should select
 * depends on which one was hit: letters and digits run together into a word,
 * whitespace runs together into a gap, and a punctuation mark is its own unit.
 * Collapsing the last two would make double-clicking a full stop select the
 * sentence's trailing space with it.
 */
type CharClass = "word" | "space" | "mark";

/** Letters, digits, combining marks and the underscore. */
const WORD_CHARACTER = /[\p{L}\p{N}\p{M}_]/u;
const WHITESPACE = /\s/u;

function classOf(code: number): CharClass {
  const char = String.fromCodePoint(code);
  if (WORD_CHARACTER.test(char)) return "word";
  if (WHITESPACE.test(char)) return "space";
  return "mark";
}

/**
 * The range of the word containing a character, for a double-click.
 *
 * Takes a *character* index, not a caret --- see {@link nearestChar} for why the
 * distinction is load-bearing here rather than pedantic.
 *
 * **Runs of letters, not dictionary words**, and the difference matters on
 * exactly one family of scripts. Word edges are found by walking outwards while
 * the character class does not change, which is correct wherever words are
 * separated by something: spaces, punctuation, or a change of class. It is not
 * correct for Chinese, Japanese or Thai, where a run of Han or Thai characters
 * is a whole clause and a double-click will select all of it. `Intl.Segmenter`
 * knows better and is deliberately not used: it segments a *string*, and this
 * module works in code-point indices precisely because `FPDFText_GetText` drops
 * characters and desynchronises the two spaces --- so adopting it would mean
 * reintroducing the index mapping that the module docs exist to warn about, to
 * fix a case no fixture currently covers. Stated rather than hidden; if it
 * becomes worth doing, the mapping is the work, not the call.
 */
export function wordAt(text: PageText, index: number): IndexRange {
  const codes = text.codes;
  if (codes.length === 0) return { from: 0, to: 0 };

  const at = Math.min(Math.max(index, 0), codes.length - 1);
  const kind = classOf(codes[at] ?? 0);
  if (kind === "mark") return { from: at, to: at + 1 };

  let from = at;
  while (from > 0 && classOf(codes[from - 1] ?? 0) === kind) from--;
  let to = at + 1;
  while (to < codes.length && classOf(codes[to] ?? 0) === kind) to++;
  return { from, to };
}

/**
 * The range of the line containing a character, for a triple-click.
 *
 * Built on {@link linesOf} rather than on a second grouping rule, so a
 * triple-click and a screen reader agree about what a line is --- two rules
 * would eventually disagree, and the one a user can see is not the one that
 * gets tested.
 */
export function lineAt(text: PageText, index: number): IndexRange {
  const at = Math.min(Math.max(index, 0), Math.max(text.codes.length - 1, 0));
  for (const line of linesOf(text)) {
    if (at >= line.from && at < line.to) return line;
  }
  return { from: 0, to: text.codes.length };
}

/**
 * The text of a character range.
 *
 * Built from code points rather than sliced out of a string, because there is no
 * string to slice --- see the module docs. `fromCodePoint` is applied in chunks
 * so a page-sized selection cannot overflow the argument limit.
 */
export function textOf(text: PageText, from: number, to: number): string {
  const start = Math.max(0, from);
  const end = Math.min(to, text.codes.length);
  let out = "";
  for (let at = start; at < end; at += 4096) {
    out += String.fromCodePoint(...text.codes.slice(at, Math.min(at + 4096, end)));
  }
  return out;
}
