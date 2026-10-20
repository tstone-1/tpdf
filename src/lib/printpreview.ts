/**
 * What the print preview shows, worked out apart from the dialog that shows it.
 *
 * The preview is of the print job itself: the bytes the platform's dialog is
 * handed next, with the reader's marks, crops, deleted pages and turn already
 * in them, drawn by a worker over those bytes (`commands::print::Previews`).
 * So a sheet here is a page of the job, not a page of the open document.
 *
 * macOS has no preview of tpdf's: its print panel draws the job itself.
 */

import type { PageSize } from "./ipc";
import { PAGE_SIZES } from "./pagesizes";

/**
 * The widest picture the backend draws. `PREVIEW_MAX_WIDTH` in
 * `commands/print.rs`; `printpreview.test.ts` holds the two together.
 */
export const PREVIEW_MAX_WIDTH = 1600;

/** Space between two sheets, in CSS pixels. */
export const SHEET_GAP = 16;

/** How close a page must be to a named size to be called by its name, in points. */
const NAMED_WITHIN = 3;

/**
 * A sheet's paper, as a reader names it: "A4", "A3 landscape", "US Letter",
 * or its size in millimetres when it is none of those.
 */
export function paperName(size: PageSize): string {
  const { width_pt: width, height_pt: height } = size;
  for (const named of Object.values(PAGE_SIZES)) {
    if (Math.abs(width - named.width) <= NAMED_WITHIN && Math.abs(height - named.height) <= NAMED_WITHIN) {
      return named.title;
    }
    if (Math.abs(width - named.height) <= NAMED_WITHIN && Math.abs(height - named.width) <= NAMED_WITHIN) {
      return `${named.title} landscape`;
    }
  }
  const mm = (points: number) => Math.round((points / 72) * 25.4);
  return `${mm(width)} x ${mm(height)} mm`;
}

/**
 * The line under the preview's heading: how many sheets, and on what paper.
 * Up to three different papers are named; more than that is "mixed sizes".
 */
export function previewSummary(pages: readonly PageSize[]): string {
  const count = pages.length === 1 ? "1 page" : `${pages.length} pages`;
  const names = [...new Set(pages.map(paperName))];
  if (names.length === 0) return count;
  if (names.length === 1) return pages.length === 1 ? `${count}, ${names[0]}` : `${count}, all ${names[0]}`;
  if (names.length <= 3) return `${count}: ${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return `${count} in mixed sizes`;
}

/**
 * A sheet's size on screen, in CSS pixels: as large as fits `maxWidth` by
 * `maxHeight` with its own proportions. A landscape page fills the width, a
 * portrait one the height, so every sheet is as readable as the box allows.
 */
export function fitSheet(size: PageSize, maxWidth: number, maxHeight: number): { width: number; height: number } {
  const { width_pt: width, height_pt: height } = size;
  if (!(width > 0 && height > 0 && maxWidth > 0 && maxHeight > 0)) return { width: 0, height: 0 };
  const scale = Math.min(maxWidth / width, maxHeight / height);
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}

/**
 * How many pixels wide to ask the backend for a sheet shown `cssWidth`
 * pixels wide: sharp on the screen's own pixels, never past
 * {@link PREVIEW_MAX_WIDTH}, and at least one.
 */
export function pictureWidth(cssWidth: number, devicePixelRatio: number): number {
  const ratio = devicePixelRatio > 0 ? devicePixelRatio : 1;
  return Math.max(1, Math.min(PREVIEW_MAX_WIDTH, Math.round(cssWidth * ratio)));
}

/**
 * The sheets at least partly inside the scrolled view, as `[first, last]`, or
 * `null` when there are none. `heights` are the sheets' heights in order, each
 * followed by {@link SHEET_GAP}.
 */
export function visibleSheets(
  heights: readonly number[],
  scrollTop: number,
  viewHeight: number,
): [number, number] | null {
  let top = 0;
  let first = -1;
  let last = -1;
  const bottom = scrollTop + viewHeight;
  for (let index = 0; index < heights.length; index += 1) {
    const end = top + (heights[index] ?? 0);
    if (end > scrollTop && top < bottom) {
      if (first < 0) first = index;
      last = index;
    } else if (top >= bottom) {
      break;
    }
    top = end + SHEET_GAP;
  }
  return first < 0 ? null : [first, last];
}

/**
 * The next sheet to draw: the first visible one not yet asked for, then the
 * one after the view, so scrolling down finds it drawn. `null` when everything
 * near the view has been asked for.
 */
export function nextToDraw(
  visible: [number, number] | null,
  count: number,
  asked: ReadonlySet<number>,
): number | null {
  if (!visible) return null;
  const [first, last] = visible;
  for (let index = first; index <= Math.min(last + 1, count - 1); index += 1) {
    if (!asked.has(index)) return index;
  }
  return null;
}
