/**
 * The reader's crop: asking the backend for it, and placing things inside it.
 *
 * ## Everything stays in the file's display space, and one pair of numbers is
 * why
 *
 * A comment, a link and one of the reader's own marks all arrive measured from
 * the **file's** displayed corner --- `annots.rs` and `links.rs` read the file,
 * and a mark is stored against the page the file describes. A crop moves that
 * corner. Rather than restating all three in a space that changes whenever the
 * reader crops, they stay where they are and are drawn at `rect - (left, top)`:
 * {@link CropGeometry} carries the pair, {@link intoCrop} applies it and
 * {@link outOfCrop} undoes it.
 *
 * That direction matters as much as the offset. A mark is **sent** in the file's
 * space, so a highlight made while cropped and saved after the crop changed is
 * still written where the words are rather than where the crop was.
 *
 * ## The geometry cannot be computed here, so it is asked for
 *
 * A crop box is in the page's own space and a layout is in display space, and
 * the turn between them is the page's `/Rotate` --- which the frontend is never
 * told, deliberately, because the renderer already composes it and a second copy
 * would be a second thing to keep right. So the size a cropped page lays out at,
 * and where the crop sits inside the file's page, come from
 * {@link pageGeometry}, which asks PDFium.
 */

import { invoke } from "@tauri-apps/api/core";

/** A crop box's size and where it sits inside the page the file describes. */
export interface CropGeometry {
  /** The cropped page's displayed width in points. */
  width_pt: number;
  /** The cropped page's displayed height in points. */
  height_pt: number;
  /** The crop's left edge in the file's display space, points from its corner. */
  left: number;
  /** The crop's top edge in the file's display space, points from its corner. */
  top: number;
}

/** The geometry of a page nobody has cropped: itself, at the origin. */
export function uncropped(width_pt: number, height_pt: number): CropGeometry {
  return { width_pt, height_pt, left: 0, top: 0 };
}

/**
 * A rectangle from the file, moved into the cropped page's display space.
 *
 * `[left, top, right, bottom]` in, the same out. Pure, and separate from every
 * caller for the reason every mapping in this repository is separate: the
 * failure mode is a plausible rectangle in the wrong place, which no amount of
 * looking at a render reliably catches.
 */
export function intoCrop(
  rect: readonly [number, number, number, number],
  at: CropGeometry,
): [number, number, number, number] {
  return [
    rect[0] - at.left,
    rect[1] - at.top,
    rect[2] - at.left,
    rect[3] - at.top,
  ];
}

/**
 * Quads from the cropped page's display space, back into the file's.
 *
 * The inverse of {@link intoCrop}, over the flat four-per-rectangle array a
 * mark carries. It exists for one caller and that caller is the reason the model
 * holds one space: a mark made while the page is cropped has to be stored where
 * the words are, or a later crop moves it.
 */
export function outOfCrop(quads: readonly number[], at: CropGeometry): number[] {
  const moved: number[] = [];
  for (let index = 0; index < quads.length; index += 1) {
    moved.push((quads[index] ?? 0) + (index % 2 === 0 ? at.left : at.top));
  }
  return moved;
}

/**
 * The box a page's ink occupies, in the page's own space, or `null` if blank.
 *
 * `page` is a position in the **baseline file**, never a slot and never a page
 * id: this asks PDFium about the document on disk, which knows nothing about the
 * model's identities.
 */
export async function contentBox(
  doc: number,
  page: number,
): Promise<[number, number, number, number] | null> {
  return await invoke<[number, number, number, number] | null>(
    "page_content_box",
    { doc, page },
  );
}

/** Where a crop box lands inside the file's own page, and how big it is. */
export async function pageGeometry(
  doc: number,
  page: number,
  crop: readonly [number, number, number, number] | null,
): Promise<CropGeometry> {
  return await invoke<CropGeometry>("page_geometry", { doc, page, crop });
}
