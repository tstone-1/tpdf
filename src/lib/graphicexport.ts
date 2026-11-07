/**
 * *Copy a graphic* and *Save a graphic...*: the pictures and drawings lying
 * wholly inside a dragged region, on their own (`graphic.rs`).
 *
 * *Copy a region as a picture* gives everything in the rectangle at the
 * screen's sharpness, the page's white included. A logo is wanted without
 * that: alone, on a transparent background, sharp at any size, and as the
 * file it was if it is a picture. So the backend takes the page objects inside
 * the region and renders them alone at 600 dpi, or hands back a stored JPEG as
 * it is, or writes a drawing as SVG paths.
 *
 * This module holds what the window decides: the request a region becomes,
 * which formats the save panel offers, which one the chosen name asks for,
 * the name it suggests, and what the window says afterwards.
 */

import { catalog } from "./i18n";
import { regionRequest, type LaidQuad } from "./regionshot";
import type { TileSource } from "./tiles";

const TEXT = catalog({
  "en-US": {
    graphic: "graphic",
    panelTitle: "Save a graphic",
    fileName: (stem: string, page: number, ending: string) => `${stem} page ${page} graphic.${ending}`,
    svgDrawing: "SVG drawing",
    pngPicture: "PNG picture",
    storedJpeg: "JPEG picture, as stored",
    copied: (width: number, height: number) =>
      `Copied the graphic, ${width} by ${height} pixels on a transparent background.`,
    savedSvg: (name: string) => `Saved the graphic as ${name}, a drawing that stays sharp at any size.`,
    savedPng: (name: string, width: number, height: number) =>
      `Saved the graphic as ${name}, ${width} by ${height} pixels on a transparent background.`,
    savedJpeg: (name: string) => `Saved the picture as ${name}, as the document stores it.`,
    wrongEnding: (name: string, endings: string) =>
      `${name} was not saved: choose a name ending in ${endings}.`,
  },
  "de-DE": {
    graphic: "Grafik",
    panelTitle: "Grafik speichern",
    fileName: (stem: string, page: number, ending: string) => `${stem} Seite ${page} Grafik.${ending}`,
    svgDrawing: "SVG-Zeichnung",
    pngPicture: "PNG-Bild",
    storedJpeg: "JPEG-Bild, wie gespeichert",
    copied: (width: number, height: number) =>
      `Grafik kopiert, ${width} × ${height} Pixel auf transparentem Hintergrund.`,
    savedSvg: (name: string) => `Grafik als ${name} gespeichert, eine Zeichnung, die in jeder Größe scharf bleibt.`,
    savedPng: (name: string, width: number, height: number) =>
      `Grafik als ${name} gespeichert, ${width} × ${height} Pixel auf transparentem Hintergrund.`,
    savedJpeg: (name: string) => `Bild als ${name} gespeichert, so wie das Dokument es enthält.`,
    wrongEnding: (name: string, endings: string) =>
      `${name} wurde nicht gespeichert: Der Name muss auf ${endings} enden.`,
  },
});

/** What the worker is asked for: `GraphicFormat` in `graphic.rs`. */
export type GraphicFormat = "describe" | "png" | "original" | "svg";

/** A region, as `GraphicRequest` in `graphic.rs` takes it. */
export interface GraphicRequest {
  page: number;
  crop?: readonly [number, number, number, number];
  turns: number;
  /** Pixels a point of the tile `rect` is measured in. */
  scale: number;
  /** x, y, width, height in that tile's pixels. */
  rect: [number, number, number, number];
  format: GraphicFormat;
}

/** What a region holds: `GraphicFound` in `graphic.rs`. */
export interface GraphicFound {
  objects: number;
  /** `jpg` or `png` when the region holds one picture and nothing else. */
  original: "jpg" | "png" | null;
  /** Why no SVG can be made, or `null` when one can. */
  no_svg: string | null;
  width: number;
  height: number;
  /** The ending of the bytes made; empty for a description. */
  extension: string;
}

/**
 * The document and the request for the objects inside `quad`.
 *
 * The rectangle is measured in the pixels of the tile *Copy a region* would
 * render, so both commands read a drag the same way; the backend turns it
 * back into points with the page's own geometry.
 */
export function graphicRequest(
  source: TileSource,
  quad: LaidQuad,
  screenScale: number,
  format: GraphicFormat,
): { doc: number; request: GraphicRequest } {
  const tile = regionRequest(source, quad, screenScale);
  const request: GraphicRequest = {
    page: tile.page,
    turns: tile.turns ?? 0,
    scale: tile.scale,
    rect: [tile.x, tile.y, tile.width, tile.height],
    format,
  };
  if (tile.crop) request.crop = tile.crop;
  return { doc: tile.doc, request };
}

/** One format the save panel offers. */
export interface GraphicChoice {
  format: Exclude<GraphicFormat, "describe">;
  name: string;
  extensions: string[];
}

/**
 * The formats a region can be saved as, the one to suggest first.
 *
 * SVG first when the drawing can be one, because it stays sharp at any size;
 * then PNG, which every graphic can be; and the stored JPEG when the region
 * is one. A picture stored any other way is already given as it is by the
 * PNG, so it has no third choice.
 */
export function graphicChoices(found: GraphicFound): GraphicChoice[] {
  const choices: GraphicChoice[] = [];
  if (found.no_svg === null) choices.push({ format: "svg", name: TEXT().svgDrawing, extensions: ["svg"] });
  if (found.original === "jpg") choices.push({ format: "original", name: TEXT().storedJpeg, extensions: ["jpg", "jpeg"] });
  choices.push({ format: "png", name: TEXT().pngPicture, extensions: ["png"] });
  return choices;
}

/** The format a chosen file name asks for, among `choices`; `null` for none of them. */
export function formatFor(path: string, choices: readonly GraphicChoice[]): GraphicChoice["format"] | null {
  const ending = /\.([^./\\]+)$/.exec(path)?.[1]?.toLowerCase();
  if (!ending) return null;
  return choices.find((choice) => choice.extensions.includes(ending))?.format ?? null;
}

/** The refusal for a name none of `choices` ends in. */
export function wrongEnding(name: string, choices: readonly GraphicChoice[]): string {
  const endings = choices.flatMap((choice) => choice.extensions.map((ending) => `.${ending}`));
  return TEXT().wrongEnding(name, endings.join(", "));
}

/** The name the save panel offers: the document's, the page, and the first choice's ending. */
export function graphicName(documentName: string, pageNumber: number, choice: GraphicChoice): string {
  const stem = documentName.replace(/\.pdf$/i, "") || TEXT().graphic;
  return TEXT().fileName(stem, pageNumber, choice.extensions[0]!);
}

/** The save panel's title. */
export function saveGraphicTitle(): string {
  return TEXT().panelTitle;
}

/** What the window says once a graphic is copied, or saved as `saved`. */
export function graphicSaid(found: GraphicFound, saved?: string): string {
  if (saved === undefined) return TEXT().copied(found.width, found.height);
  if (found.extension === "svg") return TEXT().savedSvg(saved);
  if (found.extension === "jpg") return TEXT().savedJpeg(saved);
  return TEXT().savedPng(saved, found.width, found.height);
}
