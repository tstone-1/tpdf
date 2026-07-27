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
  extract_ms: number;
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

  for (let index = 0; index < text.codes.length; index++) {
    const quad = charQuad(text, index);
    if (!isPlaced(quad)) continue;

    const dx = Math.max(quad.left - x, 0, x - quad.right);
    const dy = Math.max(quad.top - y, 0, y - quad.bottom);
    const distance = dx * dx + (dy * VERTICAL_WEIGHT) ** 2;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  }

  if (best < 0) return 0;
  const quad = charQuad(text, best);
  // Past the middle of the glyph means the caret belongs after it, which is what
  // makes dragging left-to-right include the character under the pointer.
  return x > (quad.left + quad.right) / 2 ? best + 1 : best;
}

/**
 * How much more a point of vertical distance counts than a horizontal one.
 *
 * Untuned, and chosen to be obviously large enough that a neighbouring line
 * never wins over the line the pointer is on. Lower it and clicks in the margin
 * start landing a line away.
 */
const VERTICAL_WEIGHT = 8;

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

  for (let index = Math.max(0, from); index < Math.min(to, text.codes.length); index++) {
    const quad = charQuad(text, index);
    if (!isPlaced(quad)) continue;

    if (current && overlapsVertically(current, quad)) {
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

/** Whether two boxes share most of their vertical extent, i.e. are on a line. */
function overlapsVertically(a: Quad, b: Quad): boolean {
  const overlap = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
  const shorter = Math.min(a.bottom - a.top, b.bottom - b.top);
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
