/**
 * A region of a page as a picture: copied to the clipboard as a PNG
 * (`edit.copyRegionAsPicture`) or saved as one (`file.saveRegionAsPicture`).
 *
 * **The region is one tile**, rendered by a worker as every tile is: the page
 * as the file has it, under the crop and the turn on screen, and dark if the
 * page is shown dark. A mark not yet saved is drawn by the overlay, not by the
 * worker, so it is not in the picture.
 *
 * **The resolution is the screen's**, the zoom times the device pixel ratio,
 * so the picture is as sharp as what the reader saw; never less than
 * {@link REGION_FLOOR} pixels a point, so a region copied from a page zoomed
 * out to fit is still readable; and never more than the tile mapping holds,
 * {@link REGION_PIXELS}, so a large region at a high zoom comes out at the
 * largest scale that fits rather than not at all.
 */

import { catalog } from "./i18n";
import type { TileRequest, TileSource } from "./tiles";

const TEXT = catalog({
  "en-US": {
    region: "region",
    fileName: (stem: string, page: number) => `${stem} page ${page}.png`,
    copied: (width: number, height: number) => `Copied the region as a picture, ${width} by ${height} pixels.`,
    saved: (name: string, width: number, height: number) =>
      `Saved the region as ${name}, ${width} by ${height} pixels.`,
  },
  "de-DE": {
    region: "Bereich",
    fileName: (stem: string, page: number) => `${stem} Seite ${page}.png`,
    copied: (width: number, height: number) => `Bereich als Bild kopiert, ${width} × ${height} Pixel.`,
    saved: (name: string, width: number, height: number) =>
      `Bereich als ${name} gespeichert, ${width} × ${height} Pixel.`,
  },
});

/** A rectangle on a page as it is laid out, in points: `boxQuad`'s shape. */
export interface LaidQuad {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** Pixels a point a region is copied at, at least: 144 pixels an inch. */
export const REGION_FLOOR = 2;

/** Most pixels in a region: the tile mapping's 16 MiB, four bytes a pixel (`TILE_CAPACITY`). */
export const REGION_PIXELS = (16 * 1024 * 1024) / 4;

/** Most pixels a scale may name: `MAX_SCALE_THOUSANDTHS` in `protocol.rs`. */
const MAX_SCALE = 256;

/** A tile's side is sixteen bits on the wire. */
const MAX_SIDE = 65_535;

/**
 * The tile that renders `quad` of the page `source` names, as a PNG.
 *
 * Worked in whole thousandths of a pixel a point, the unit the scale travels
 * in, so the rectangle the worker renders is the one computed here. The
 * rectangle covers the quad, whole pixels outwards. A region past the mapping
 * is shrunk by what it is over, which can land a little under, and is then
 * stepped back up to the largest scale that fits.
 */
export function regionRequest(source: TileSource, quad: LaidQuad, screenScale: number): TileRequest {
  const wanted = Math.floor(Math.min(Math.max(screenScale, REGION_FLOOR), MAX_SCALE) * 1000);
  let at = wanted;
  while (at > 1 && !fits(pixelsOf(quad, at / 1000))) {
    const rect = pixelsOf(quad, at / 1000);
    const over = Math.max(
      Math.sqrt((rect.width * rect.height) / REGION_PIXELS),
      rect.width / MAX_SIDE,
      rect.height / MAX_SIDE,
    );
    at = Math.max(1, Math.min(Math.floor(at / over), at - 1));
  }
  while (at < wanted && fits(pixelsOf(quad, (at + 1) / 1000))) at++;
  const scale = at / 1000;
  return { ...source, scale, ...pixelsOf(quad, scale), format: "png" };
}

function fits(rect: { width: number; height: number }): boolean {
  return rect.width * rect.height <= REGION_PIXELS && rect.width <= MAX_SIDE && rect.height <= MAX_SIDE;
}

function pixelsOf(quad: LaidQuad, scale: number): { x: number; y: number; width: number; height: number } {
  const x = Math.floor(quad.left * scale);
  const y = Math.floor(quad.top * scale);
  return {
    x,
    y,
    width: Math.max(1, Math.ceil(quad.right * scale) - x),
    height: Math.max(1, Math.ceil(quad.bottom * scale) - y),
  };
}

/**
 * The name the save panel offers: the document's, the page, and `.png`, so a
 * folder of regions from one report says where each came from.
 */
export function regionName(documentName: string, pageNumber: number): string {
  const stem = documentName.replace(/\.pdf$/i, "") || TEXT().region;
  return TEXT().fileName(stem, pageNumber);
}

/** What the window says once a region is copied or saved. */
export function regionSaid(request: Pick<TileRequest, "width" | "height">, saved?: string): string {
  return saved === undefined
    ? TEXT().copied(request.width, request.height)
    : TEXT().saved(saved, request.width, request.height);
}
