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
}

/** Whether lines on this page are separated horizontally rather than vertically. */
export function linesRunSideways(text: PageText): boolean {
  return text.quarter_turns % 2 === 1;
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
 * Per-page text, fetched once.
 *
 * Extraction measured 1.4 ms on a dense page and the page's own load dominates
 * on a complex one (`text-probe --mode extract`), so the cache is about avoiding
 * an IPC round trip during a drag, not about avoiding PDFium. It is unbounded:
 * a page's characters are a few hundred kilobytes at worst and a reader visits
 * tens of pages, not thousands.
 *
 * Search does not change that --- the matching happens in Rust and only the hits
 * cross, so a whole-document scan never touches this. What can still grow it is
 * a reader stepping through hits on a thousand different pages, since each jump
 * loads the page it lands on to know where to scroll. That wants a bound and
 * does not have one.
 */
export class TextCache {
  private readonly doc: number;
  private readonly pages = new Map<number, PageText>();
  private readonly pending = new Map<number, Promise<PageText | null>>();

  constructor(doc: number) {
    this.doc = doc;
  }

  /** A page's text if it has already arrived, without asking for it. */
  peek(page: number): PageText | null {
    return this.pages.get(page) ?? null;
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
    if (cached) return cached;

    const existing = this.pending.get(page);
    if (existing) return existing;

    const request = invoke<PageText>("page_text", { doc: this.doc, page })
      .then((text) => {
        this.pages.set(page, text);
        return text;
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
