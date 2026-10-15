/**
 * Where a page's content is, read from its rendered pixels.
 *
 * Fit content needs a box round what is printed on the page. Measured on the
 * tier-1 placeholder, the 150 px bitmap the viewer already holds for every page
 * it has shown, rather than asked of the worker: a scanned page has no text or
 * drawing objects to measure, and the pixels are the same answer for every kind
 * of page. At 150 px the box is good to about 4 points on an A4 sheet, which
 * the fit's margin covers.
 */

/** A box as fractions of the bitmap, 0 at the left and top, 1 at the right and bottom. */
export interface InkBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * How far a channel may differ from the paper and still be paper.
 *
 * A JPEG scan's white is not one value: measured paper in a scan wanders by
 * ten or so per channel. Anti-aliased text edges fade below this, which costs
 * less than a pixel of the box.
 */
export const PAPER_TOLERANCE = 32;

/**
 * The box round every pixel that is not paper, or `null` for a blank page.
 *
 * `rgba` is `ImageData.data`: four bytes a pixel, row by row. The paper is the
 * colour of the top-left pixel, not white, so an inverted page (white on black)
 * and a tinted sheet are measured the same way. The box is grown by one pixel on
 * each side, inside the bitmap, because the edge pixel of a stroke is partly
 * paper and can fall under the tolerance.
 */
export function inkBounds(rgba: ArrayLike<number>, width: number, height: number): InkBox | null {
  if (width <= 0 || height <= 0 || rgba.length < width * height * 4) return null;
  const paper = [rgba[0]!, rgba[1]!, rgba[2]!];
  let left = width;
  let right = -1;
  let top = height;
  let bottom = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = (y * width + x) * 4;
      if (
        Math.abs(rgba[at]! - paper[0]!) <= PAPER_TOLERANCE &&
        Math.abs(rgba[at + 1]! - paper[1]!) <= PAPER_TOLERANCE &&
        Math.abs(rgba[at + 2]! - paper[2]!) <= PAPER_TOLERANCE
      ) continue;
      if (x < left) left = x;
      if (x > right) right = x;
      if (y < top) top = y;
      if (y > bottom) bottom = y;
    }
  }
  if (right < 0) return null;
  return {
    left: Math.max(0, left - 1) / width,
    top: Math.max(0, top - 1) / height,
    right: Math.min(width, right + 2) / width,
    bottom: Math.min(height, bottom + 2) / height,
  };
}

/** A page of a row as displayed, in points, with its content box if it has one. */
export interface RowPage {
  width_pt: number;
  height_pt: number;
  ink: InkBox | null;
}

/** The content of a row, as fit content fits it and lands on it. */
export interface RowContent {
  /** The span from the first content to the last, in points, gaps excluded. */
  width_pt: number;
  height_pt: number;
  /** How many gaps between pages the span crosses: 0, or 1 with two pages. */
  gaps: number;
  /** The page, by its place in `pages`, the content starts on. */
  first: number;
  /** Where the content starts on that page, in points from its left. */
  left_pt: number;
  /** Where the content starts, in points from the top of the row. */
  top_pt: number;
}

/**
 * The box round the content of a row's pages, or `null` when all are blank.
 *
 * `pages` is in the order they sit on screen, left to right. Facing pages fit
 * as one box, SumatraPDF's rule: the outer margins go and the inner ones stay,
 * so the gap between the pages is inside the box. A blank page beside a printed
 * one adds nothing. Pages in a row are top-aligned, so their tops share one
 * scale.
 */
export function rowContent(pages: readonly RowPage[]): RowContent | null {
  const inked = pages.flatMap((page, at) => (page.ink ? [{ page, ink: page.ink, at }] : []));
  const first = inked[0];
  const last = inked[inked.length - 1];
  if (!first || !last) return null;
  const top = Math.min(...inked.map(({ page, ink }) => ink.top * page.height_pt));
  const bottom = Math.max(...inked.map(({ page, ink }) => ink.bottom * page.height_pt));
  const left = first.ink.left * first.page.width_pt;
  const width = first === last
    ? (first.ink.right - first.ink.left) * first.page.width_pt
    : first.page.width_pt - left + last.ink.right * last.page.width_pt;
  return {
    width_pt: width,
    height_pt: bottom - top,
    gaps: last.at - first.at,
    first: first.at,
    left_pt: left,
    top_pt: top,
  };
}
