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

import { basename } from "./paths";
import { afterCopy } from "./recovery";

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
    title: "Keep every picture as it is",
    note: "Nothing you can see changes. Saves little unless the file was stored loosely.",
    pictures: null,
  },
  {
    id: "print",
    title: "For printing",
    note: "Pictures at most 300 pixels an inch.",
    pictures: { dpi: 300, quality: 85, jpeg: true },
  },
  {
    id: "balanced",
    title: "For screen and office printing",
    note: "Pictures at most 150 pixels an inch.",
    pictures: { dpi: 150, quality: 75, jpeg: true },
  },
  {
    id: "screen",
    title: "Smallest, for reading on a screen",
    note: "Pictures at most 110 pixels an inch. Soft when zoomed in.",
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
    return { problem: `The resolution is a whole number from ${DPI_MIN} to ${DPI_MAX}.` };
  }
  const q = whole(quality);
  if (q === null || q < 1 || q > 100) {
    return { problem: "The JPEG quality is a whole number from 1 to 100." };
  }
  return { pictures: { dpi: d, quality: q, jpeg } };
}

/** A size as a reader says it: `873 KB`, `8.7 MB`. */
export function size(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  if (bytes < 1024) return `${Math.round(bytes)} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
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
  if (!isSmaller(shrinkage)) return "Not smaller";
  return `${size(shrinkage.bytesAfter)}, ${savedPercent(shrinkage.bytesBefore, shrinkage.bytesAfter)}% smaller`;
}

/** The sentence under the choices, about the one that is selected. */
export function summary(shrinkage: Shrinkage): string {
  const now = size(shrinkage.bytesBefore);
  if (!isSmaller(shrinkage)) {
    return `The document is ${now}, and a copy made this way would not be smaller.`;
  }
  const pictures =
    shrinkage.pictures === 0
      ? ""
      : shrinkage.picturesChanged === 0
        ? " No picture changes."
        : ` ${shrinkage.picturesChanged} of ${shrinkage.pictures} pictures are stored smaller.`;
  return `From ${now} to about ${size(shrinkage.bytesAfter)}.${pictures}`;
}

/** What the two pictures are, for the line under them. */
export function caption(sample: SampleView): string {
  const where = `Part of page ${sample.page} at ${sample.zoomPercent}%: now on the left, the copy on the right.`;
  if (sample.dpiBefore > 0 && sample.dpiAfter > 0 && sample.dpiAfter < sample.dpiBefore) {
    return `${where} Its largest picture goes from ${sample.dpiBefore} to ${sample.dpiAfter} pixels an inch.`;
  }
  return where;
}

/** `report.pdf` gives `report smaller.pdf`. */
export function suggestedName(path: string): string {
  return `${basename(path).replace(/\.pdf$/i, "")} smaller.pdf`;
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
      ? `Saved ${basename(path)}, about ${size(shrinkage.bytesAfter)} where the document is ${size(shrinkage.bytesBefore)}.`
      : `Saved ${basename(path)}.`;
  const changed = afterCopy(copied);
  return changed ? `${said} ${changed}` : said;
}
