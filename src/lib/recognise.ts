/**
 * Text recognition from the window: the name it suggests, the line it shows
 * while it reads, and the sentence it ends on.
 *
 * `App.svelte` keeps the dialogs and the `invoke`; what is decided is here, so
 * it has tests. The reading, the writing and the read-back of the copy are
 * `src-tauri/src/commands/ocr.rs`.
 */

import { catalog, type UiLocale } from "./i18n";
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

/**
 * What a refused page usually is. `REFUSED_MEANS` in `ocr_layer.rs`, which has
 * the measurements; `recognise.test.ts` holds the two spellings together. The
 * English the window says; the German half of {@link TEXT} has its own.
 */
export const REFUSED_MEANS =
  "which usually means a script it cannot read or a scan too unclear to tell the script";

const TEXT = catalog({
  "en-US": {
    saveFirst: "Save your changes first. Text is recognized on the pages of the saved file.",
    starting: "Recognizing text...",
    searchable: (stem: string) => `${stem} searchable.pdf`,
    progress: (page: number, of: number) => `Recognizing text: page ${page} of ${of}...`,
    saved: (name: string, added: number, total: number, words: number) =>
      `Saved ${name}. Text was added to ${added} of ${total === 1 ? "1 page" : `${total} pages`} ` +
      `(${words === 1 ? "1 word" : `${words} words`}).`,
    alreadyText: (pages: string) => `Already had text: ${pages}.`,
    nothingRead: (pages: string) => `No text was recognized on ${pages}.`,
    refused: (pages: string) => `The recognizer refused ${pages}, ${REFUSED_MEANS}.`,
    tooLarge: (pages: string) => `Too large to read: ${pages}.`,
    pages: (shown: string, count: number, rest: number) =>
      count === 1 ? `page ${shown}` : rest > 0 ? `pages ${shown} and ${rest} more` : `pages ${shown}`,
  },
  "en-GB": {
    saveFirst: "Save your changes first. Text is recognised on the pages of the saved file.",
    starting: "Recognising text...",
    progress: (page: number, of: number) => `Recognising text: page ${page} of ${of}...`,
    nothingRead: (pages: string) => `No text was recognised on ${pages}.`,
    refused: (pages: string) => `The recogniser refused ${pages}, ${REFUSED_MEANS}.`,
  },
  "de-DE": {
    saveFirst: "Speichere zuerst deine Änderungen. Der Text wird auf den Seiten der gespeicherten Datei erkannt.",
    starting: "Text wird erkannt...",
    searchable: (stem: string) => `${stem} durchsuchbar.pdf`,
    progress: (page: number, of: number) => `Text wird erkannt: Seite ${page} von ${of}...`,
    saved: (name: string, added: number, total: number, words: number) =>
      `${name} gespeichert. Auf ${added} von ${total === 1 ? "1 Seite" : `${total} Seiten`} wurde Text ` +
      `hinzugefügt (${words === 1 ? "1 Wort" : `${words} Wörter`}).`,
    alreadyText: (pages: string) => `Hatte bereits Text: ${pages}.`,
    nothingRead: (pages: string) => `Kein Text erkannt: ${pages}.`,
    refused: (pages: string) =>
      `Die Texterkennung hat ${pages} abgelehnt; meist ist das eine Schrift, die sie nicht lesen kann, ` +
      `oder ein Scan, der zu unscharf ist, um die Schrift zu bestimmen.`,
    tooLarge: (pages: string) => `Zu groß für die Erkennung: ${pages}.`,
    pages: (shown: string, count: number, rest: number) =>
      count === 1 ? `Seite ${shown}` : rest > 0 ? `Seiten ${shown} und ${rest} weitere` : `Seiten ${shown}`,
  },
});

/**
 * What a document with unsaved changes is told, before any name is asked for.
 * The British sentence is `UNSAVED` in `commands/ocr.rs`, which says it when
 * the backend refuses; `recognise.test.ts` holds the two together.
 */
export function saveFirst(locale?: UiLocale): string {
  return TEXT(locale).saveFirst;
}

/** {@link saveFirst} in the window's language. */
export const SAVE_FIRST = saveFirst();

/** The line shown before the first page has been reported. */
export const STARTING = TEXT().starting;

/** `scan.pdf` gives `scan searchable.pdf`. */
export function suggestedName(path: string): string {
  return TEXT().searchable(basename(path).replace(/\.pdf$/i, ""));
}

/** The line shown while a page is being read. */
export function progressLine(at: Progress): string {
  return TEXT().progress(at.page, at.of);
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
  const said = [TEXT().saved(name, read.pages.length, total, words)];
  if (read.alreadyText.length > 0) {
    said.push(TEXT().alreadyText(pages(read.alreadyText)));
  }
  if (read.nothingRead.length > 0) {
    said.push(TEXT().nothingRead(pages(read.nothingRead)));
  }
  if (read.refused.length > 0) {
    said.push(TEXT().refused(pages(read.refused)));
  }
  if (read.tooLarge.length > 0) {
    said.push(TEXT().tooLarge(pages(read.tooLarge)));
  }
  if (read.languageUnavailable) said.push(fellBack(read.languageUnavailable));
  return said.join(" ");
}

/** `page 2` or `pages 2, 5 and 3 more`: a long document must not fill the window. */
function pages(numbers: number[]): string {
  const SHOWN = 8;
  const head = numbers.slice(0, SHOWN).join(", ");
  return TEXT().pages(head, numbers.length, numbers.length - SHOWN);
}
