/**
 * Text recognition from the window: the name it suggests, the line it shows
 * while it reads, and the sentence it ends on.
 *
 * `App.svelte` keeps the dialogs and the `invoke`; what is decided is here, so
 * it has tests. The reading, the writing and the read-back of the copy are
 * `src-tauri/src/commands/ocr.rs`.
 */

import { fellBack } from "./ocrlanguage";
import { basename } from "./paths";

/**
 * The event that says which page is being read.
 *
 * `PROGRESS_EVENT` in `commands/ocr.rs`; `recognise.test.ts` holds the two
 * spellings together.
 */
export const PROGRESS_EVENT = "tpdf://ocr-progress";

/** The page being read, counted from 1, and how many there are. */
export interface Progress {
  page: number;
  of: number;
}

/** What became of every page. `Recognised` in `commands/ocr.rs`; pages count from 1. */
export interface Recognised {
  /** The pages given a text layer. */
  pages: { page: number; words: number }[];
  /** Pages with text of their own, left as they are. */
  alreadyText: number[];
  /** Pages the engine found no words on. */
  nothingRead: number[];
  /** Pages whose picture the engine would not read. */
  refused: number[];
  /** Pages too large to render finely enough to read. */
  tooLarge: number[];
  engine: string;
  /**
   * The language the reader had chosen, when the machine no longer offers it
   * and the engine chose instead. Absent otherwise.
   */
  languageUnavailable?: string;
}

/** What a document with unsaved changes is told, before any name is asked for. */
export const SAVE_FIRST =
  "Save your changes first. Text is recognised on the pages of the saved file.";

/**
 * What a refused page usually is. `REFUSED_MEANS` in `ocr_layer.rs`, which has
 * the measurements; `recognise.test.ts` holds the two spellings together.
 */
export const REFUSED_MEANS =
  "which usually means a script it cannot read or a scan too unclear to tell the script";

/** The line shown before the first page has been reported. */
export const STARTING = "Recognising text...";

/** `scan.pdf` gives `scan searchable.pdf`. */
export function suggestedName(path: string): string {
  return `${basename(path).replace(/\.pdf$/i, "")} searchable.pdf`;
}

/** The line shown while a page is being read. */
export function progressLine(at: Progress): string {
  return `Recognising text: page ${at.page} of ${at.of}...`;
}

/**
 * What to say once the copy is written.
 *
 * Not silent, for `afterMerge`'s reason: the window is about to show a
 * different file from the one the reader was looking at, and which pages can
 * now be searched is the thing they cannot see by looking.
 */
export function afterRecognition(read: Recognised, name: string): string {
  const words = read.pages.reduce((sum, page) => sum + page.words, 0);
  const total =
    read.pages.length +
    read.alreadyText.length +
    read.nothingRead.length +
    read.refused.length +
    read.tooLarge.length;
  const said = [
    `Saved ${name}. Text was added to ${read.pages.length} of ${count(total, "page")} ` +
      `(${count(words, "word")}).`,
  ];
  if (read.alreadyText.length > 0) {
    said.push(`Already had text: ${pages(read.alreadyText)}.`);
  }
  if (read.nothingRead.length > 0) {
    said.push(`No text was recognised on ${pages(read.nothingRead)}.`);
  }
  if (read.refused.length > 0) {
    said.push(`The recogniser refused ${pages(read.refused)}, ${REFUSED_MEANS}.`);
  }
  if (read.tooLarge.length > 0) {
    said.push(`Too large to read: ${pages(read.tooLarge)}.`);
  }
  if (read.languageUnavailable) said.push(fellBack(read.languageUnavailable));
  return said.join(" ");
}

/** `1 page` or `4 pages`. */
function count(many: number, noun: string): string {
  return many === 1 ? `1 ${noun}` : `${many} ${noun}s`;
}

/** `page 2` or `pages 2, 5 and 3 more`: a long document must not fill the window. */
function pages(numbers: number[]): string {
  const SHOWN = 8;
  const head = numbers.slice(0, SHOWN).join(", ");
  const rest = numbers.length - SHOWN;
  if (numbers.length === 1) return `page ${head}`;
  return rest > 0 ? `pages ${head} and ${rest} more` : `pages ${head}`;
}
