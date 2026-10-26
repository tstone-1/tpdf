/**
 * Saving a smaller copy: the choices, and the words for what each comes to.
 *
 * The work is the backend's (`compress.rs`). This module is what the dialog
 * offers and what it says: four ready choices for a reader who wants a
 * smaller file and no numbers, and the two numbers behind them for one who
 * wants to set them. Every choice is shown with the size it comes to and, when
 * it changes a picture, with a part of a page before and after --- a
 * percentage does not say how a picture will look.
 */

import { catalog } from "./i18n";
import { basename } from "./paths";
import { afterCopy } from "./recovery";

const TEXT = catalog({
  "en-US": {
    keepTitle: "Keep every picture as it is",
    keepNote: "Nothing you can see changes. Saves little unless the file was stored loosely.",
    printTitle: "For printing",
    balancedTitle: "For screen and office printing",
    screenTitle: "Smallest, for reading on a screen",
    atMost: (dpi: number) => `Pictures at most ${dpi} pixels an inch.`,
    soft: "Soft when zoomed in.",
    resolutionRange: (min: number, max: number) => `The resolution is a whole number from ${min} to ${max}.`,
    qualityRange: "The JPEG quality is a whole number from 1 to 100.",
    unknown: "unknown",
    bytes: (bytes: number) => `${bytes} bytes`,
    decimal: (value: number) => value.toFixed(1),
    notSmaller: "Not smaller",
    outcome: (size: string, percent: number) => `${size}, ${percent}% smaller`,
    wouldNotShrink: (now: string) => `The document is ${now}, and a copy made this way would not be smaller.`,
    noPictureChanges: "No picture changes.",
    picturesSmaller: (changed: number, of: number) => `${changed} of ${of} pictures are stored smaller.`,
    fromTo: (now: string, after: string) => `From ${now} to about ${after}.`,
    sampleWhere: (page: number, zoom: number) =>
      `Part of page ${page} at ${zoom}%: now on the left, the copy on the right.`,
    sampleDpi: (before: number, after: number) => `Its largest picture goes from ${before} to ${after} pixels an inch.`,
    smallerName: (stem: string) => `${stem} smaller.pdf`,
    savedSmaller: (name: string, after: string, before: string) =>
      `Saved ${name}, about ${after} where the document is ${before}.`,
    saved: (name: string) => `Saved ${name}.`,
  },
  "de-DE": {
    keepTitle: "Alle Bilder unverändert lassen",
    keepNote: "Nichts Sichtbares ändert sich. Spart wenig, außer die Datei wurde ineffizient gespeichert.",
    printTitle: "Für den Druck",
    balancedTitle: "Für Bildschirm und Bürodruck",
    screenTitle: "Am kleinsten, zum Lesen am Bildschirm",
    atMost: (dpi: number) => `Bilder mit höchstens ${dpi} Pixel pro Zoll.`,
    soft: "Unscharf beim Hineinzoomen.",
    resolutionRange: (min: number, max: number) => `Die Auflösung ist eine ganze Zahl von ${min} bis ${max}.`,
    qualityRange: "Die JPEG-Qualität ist eine ganze Zahl von 1 bis 100.",
    unknown: "unbekannt",
    bytes: (bytes: number) => `${bytes} Byte`,
    decimal: (value: number) => value.toFixed(1).replace(".", ","),
    notSmaller: "Nicht kleiner",
    outcome: (size: string, percent: number) => `${size}, ${percent} % kleiner`,
    wouldNotShrink: (now: string) => `Das Dokument hat ${now}, und eine so erstellte Kopie wäre nicht kleiner.`,
    noPictureChanges: "Kein Bild ändert sich.",
    picturesSmaller: (changed: number, of: number) => `${changed} von ${of} Bildern werden kleiner gespeichert.`,
    fromTo: (now: string, after: string) => `Von ${now} auf etwa ${after}.`,
    sampleWhere: (page: number, zoom: number) =>
      `Ausschnitt von Seite ${page} bei ${zoom} %: links wie jetzt, rechts die Kopie.`,
    sampleDpi: (before: number, after: number) =>
      `Das größte Bild darin geht von ${before} auf ${after} Pixel pro Zoll.`,
    smallerName: (stem: string) => `${stem} kleiner.pdf`,
    savedSmaller: (name: string, after: string, before: string) =>
      `${name} gespeichert: etwa ${after} statt ${before}.`,
    saved: (name: string) => `${name} gespeichert.`,
  },
});

/** How pictures are shrunk. `Pictures` in `compress.rs`. */
export interface Pictures {
  /** The resolution a finer picture is scaled down to, in pixels an inch. */
  dpi: number;
  /** The JPEG quality, 1 to 100. */
  quality: number;
  /** Whether a picture stored without loss may be stored as JPEG. */
  jpeg: boolean;
}

/** One part of one page before and after. `SampleView` in `commands/compress.rs`. */
export interface SampleView {
  width: number;
  height: number;
  /** A PNG as a `data:` URL. */
  before: string;
  after: string;
  /** Counted from 1. */
  page: number;
  zoomPercent: number;
  dpiBefore: number;
  dpiAfter: number;
}

/** What a smaller copy would come to. `Shrinkage` in `commands/compress.rs`. */
export interface Shrinkage {
  bytesBefore: number;
  bytesAfter: number;
  pictures: number;
  picturesChanged: number;
  sample: SampleView | null;
}

/** The lowest and highest resolution offered. `DPI_RANGE` in `compress.rs`. */
export const DPI_MIN = 20;
export const DPI_MAX = 1200;

/** One ready choice. `pictures` is `null` for the one that changes no picture. */
export interface Choice {
  id: "keep" | "print" | "balanced" | "screen";
  title: string;
  /** What it does, in a line. */
  note: string;
  pictures: Pictures | null;
}

/**
 * The ready choices, from the one that loses nothing to the smallest.
 *
 * The numbers are `Preset`'s in `compress.rs`, and a test holds the two
 * together through the sample the backend writes.
 */
export const CHOICES: readonly Choice[] = [
  {
    id: "keep",
    title: TEXT().keepTitle,
    note: TEXT().keepNote,
    pictures: null,
  },
  {
    id: "print",
    title: TEXT().printTitle,
    note: TEXT().atMost(300),
    pictures: { dpi: 300, quality: 85, jpeg: true },
  },
  {
    id: "balanced",
    title: TEXT().balancedTitle,
    note: TEXT().atMost(150),
    pictures: { dpi: 150, quality: 75, jpeg: true },
  },
  {
    id: "screen",
    title: TEXT().screenTitle,
    note: `${TEXT().atMost(110)} ${TEXT().soft}`,
    pictures: { dpi: 110, quality: 60, jpeg: true },
  },
];

/** Where the reader's own numbers start: the middle choice. */
export const CUSTOM_START: Pictures = { dpi: 150, quality: 75, jpeg: true };

/**
 * The reader's own numbers, or the sentence about what is wrong with them.
 *
 * Text and not numbers, because that is what a field holds: `1e2` and `150.5`
 * are numbers to `Number` and not what anybody means by a resolution.
 */
export function custom(
  dpi: string,
  quality: string,
  jpeg: boolean,
): { pictures: Pictures } | { problem: string } {
  const whole = (text: string): number | null =>
    /^\d{1,5}$/.test(text.trim()) ? Number(text.trim()) : null;
  const d = whole(dpi);
  if (d === null || d < DPI_MIN || d > DPI_MAX) {
    return { problem: TEXT().resolutionRange(DPI_MIN, DPI_MAX) };
  }
  const q = whole(quality);
  if (q === null || q < 1 || q > 100) {
    return { problem: TEXT().qualityRange };
  }
  return { pictures: { dpi: d, quality: q, jpeg } };
}

/** A size as a reader says it: `873 KB`, `8.7 MB`. */
export function size(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return TEXT().unknown;
  if (bytes < 1024) return TEXT().bytes(Math.round(bytes));
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? TEXT().decimal(value) : Math.round(value)} ${units[unit]}`;
}

/** The share saved in whole percent, rounded down so it never overstates. */
export function savedPercent(before: number, after: number): number {
  if (!(before > 0) || !(after < before)) return 0;
  return Math.floor(((before - after) * 100) / before);
}

/** Whether a copy of this size is worth writing. */
export function isSmaller(shrinkage: Shrinkage): boolean {
  return shrinkage.bytesAfter < shrinkage.bytesBefore;
}

/** What one choice comes to, for the line beside it. */
export function outcome(shrinkage: Shrinkage): string {
  if (!isSmaller(shrinkage)) return TEXT().notSmaller;
  return TEXT().outcome(size(shrinkage.bytesAfter), savedPercent(shrinkage.bytesBefore, shrinkage.bytesAfter));
}

/** The sentence under the choices, about the one that is selected. */
export function summary(shrinkage: Shrinkage): string {
  const now = size(shrinkage.bytesBefore);
  if (!isSmaller(shrinkage)) {
    return TEXT().wouldNotShrink(now);
  }
  const pictures =
    shrinkage.pictures === 0
      ? ""
      : shrinkage.picturesChanged === 0
        ? ` ${TEXT().noPictureChanges}`
        : ` ${TEXT().picturesSmaller(shrinkage.picturesChanged, shrinkage.pictures)}`;
  return `${TEXT().fromTo(now, size(shrinkage.bytesAfter))}${pictures}`;
}

/** What the two pictures are, for the line under them. */
export function caption(sample: SampleView): string {
  const where = TEXT().sampleWhere(sample.page, sample.zoomPercent);
  if (sample.dpiBefore > 0 && sample.dpiAfter > 0 && sample.dpiAfter < sample.dpiBefore) {
    return `${where} ${TEXT().sampleDpi(sample.dpiBefore, sample.dpiAfter)}`;
  }
  return where;
}

/** `report.pdf` gives `report smaller.pdf`. */
export function suggestedName(path: string): string {
  return TEXT().smallerName(basename(path).replace(/\.pdf$/i, ""));
}

/**
 * What to say once the copy is written.
 *
 * Never silent, where an ordinary copy is: the window still shows the
 * document it had, and the size of the file on disk is the point.
 */
export function afterCompress(
  copied: { changed?: boolean },
  path: string,
  shrinkage: Shrinkage | null,
): string {
  const said =
    shrinkage && isSmaller(shrinkage)
      ? TEXT().savedSmaller(basename(path), size(shrinkage.bytesAfter), size(shrinkage.bytesBefore))
      : TEXT().saved(basename(path));
  const changed = afterCopy(copied);
  return changed ? `${said} ${changed}` : said;
}
