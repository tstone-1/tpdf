/**
 * Turning what a reader types into the pages they meant.
 *
 * `1-3,5` and `2, 4` and `7` are the forms people already use in every print
 * dialog they have ever seen, so this parses those and nothing more inventive.
 * It exists as its own module for the reason `pages.ts` does: it is arithmetic
 * with an answer that can be *wrong* rather than merely ugly, and it is reached
 * from a command argument where the only other option is to test it through a
 * window.
 *
 * **The numbers a reader types are one-based** --- they are what is printed on
 * the page and what the toolbar says --- and everything downstream addresses
 * pages by zero-based slot. The conversion happens here, once, for the same
 * reason `nav.goToPage` does it next to the text saying "of {pageCount}".
 *
 * Three decisions that are not obvious, all of them refusals or normalisations
 * a reader could reasonably expect to go the other way:
 *
 *  - **A reversed range is refused, not corrected.** `5-3` is a typo, and
 *    quietly reading it as `3-5` hides it. Same argument as `nav.goToPage`
 *    refusing 900 in a 775-page document rather than clamping to the last page.
 *  - **Overlaps are not refused**, they are merged. `1-3,2` names three pages,
 *    and a reader who writes it has made no mistake --- a subset is a set, and
 *    asking for a page twice cannot mean anything else.
 *  - **The result is always in document order**, whatever order it was typed
 *    in. `5,1` extracts pages 1 and 5. Extract produces a *subset*; putting
 *    pages in a different order is what `edit.movePageUp` and dragging a
 *    thumbnail are for, and one operation that silently does both would make
 *    `5,1` mean something no reader could predict from the other.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    pagesToExtract: (count: number) => `Pages to extract, 1 to ${count}`,
    emptyPart: (typed: string) => `"${typed}" has an empty part`,
    backwards: (from: number, to: number) => `${from}-${to} runs backwards`,
    numberMissing: "a page number is missing",
    notANumber: (typed: string) => `"${typed}" is not a page number`,
    documentHas: (count: number) => `This document has ${count} page${count === 1 ? "" : "s"}`,
    fileHas: (name: string, count: number) => `${name} has ${count} page${count === 1 ? "" : "s"}`,
    pagesToCutAfter: (last: number) => `Pages to cut after, 1 to ${last}`,
    onePage: "A one-page document cannot be split",
    notARange: (typed: string) => `Split takes the pages to cut after, not a range like "${typed}"`,
    lastPage: (page: number) => `Page ${page} is the last page, so cutting after it makes nothing`,
    namedTwice: (page: number) => `Page ${page} is named twice`,
    split: (files: number, sizes: string) => `${files} files: ${sizes} pages`,
    extractOne: (page: number) => `Extract page ${page}`,
    extractMany: (count: number) => `Extract ${count} pages`,
    noPages: "no pages",
    named: (count: number, parts: string) => `${count === 1 ? "page" : "pages"} ${parts}`,
  },
  "de-DE": {
    pagesToExtract: (count: number) => `Zu extrahierende Seiten, 1 bis ${count}`,
    emptyPart: (typed: string) => `„${typed}“ hat einen leeren Teil`,
    backwards: (from: number, to: number) => `${from}-${to} läuft rückwärts`,
    numberMissing: "eine Seitenzahl fehlt",
    notANumber: (typed: string) => `„${typed}“ ist keine Seitenzahl`,
    documentHas: (count: number) => `Dieses Dokument hat ${count} ${count === 1 ? "Seite" : "Seiten"}`,
    fileHas: (name: string, count: number) => `${name} hat ${count} ${count === 1 ? "Seite" : "Seiten"}`,
    pagesToCutAfter: (last: number) => `Seiten, nach denen geteilt wird, 1 bis ${last}`,
    onePage: "Ein einseitiges Dokument lässt sich nicht aufteilen",
    notARange: (typed: string) => `Aufteilen erwartet die Seiten, nach denen geteilt wird, keinen Bereich wie „${typed}“`,
    lastPage: (page: number) => `Seite ${page} ist die letzte Seite; danach zu teilen ergibt nichts`,
    namedTwice: (page: number) => `Seite ${page} ist doppelt angegeben`,
    split: (files: number, sizes: string) => `${files} Dateien: ${sizes} Seiten`,
    extractOne: (page: number) => `Seite ${page} extrahieren`,
    extractMany: (count: number) => `${count} Seiten extrahieren`,
    noPages: "keine Seiten",
    named: (count: number, parts: string) => `${count === 1 ? "Seite" : "Seiten"} ${parts}`,
  },
});

/** Pages a reader named, as zero-based slots, or why the text is not usable. */
export type PageRange =
  | { slots: number[]; problem?: undefined }
  | { slots?: undefined; problem: string };

/**
 * Reads `raw` as a page selection against a document of `pageCount` pages.
 *
 * The slots come back sorted, deduplicated, and zero-based. Every failing path
 * returns a message written for the reader rather than a code: this feeds the
 * palette's `problem` callback, which shows it under the input as they type.
 *
 * `file` names the document the pages are of when it is not the one on
 * screen, so a page past the end is said to be past that file's end.
 */
export function parsePageRange(raw: string, pageCount: number, file?: string): PageRange {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { problem: TEXT().pagesToExtract(pageCount) };
  }

  const slots = new Set<number>();
  // Split on commas only. A range's hyphen is handled per part, so `1-3,5`
  // and `1 - 3, 5` both work and `1-3-5` is refused by the part that cannot
  // read it rather than by a grammar written here.
  for (const part of trimmed.split(",")) {
    const piece = part.trim();
    if (piece === "") {
      // A trailing comma is the common case and reads as unfinished typing, so
      // the message names the shape rather than complaining about a character.
      return { problem: TEXT().emptyPart(trimmed) };
    }

    const hyphen = piece.indexOf("-");
    if (hyphen === -1) {
      const one = readPage(piece, pageCount, file);
      if (typeof one === "string") return { problem: one };
      slots.add(one - 1);
      continue;
    }

    const from = readPage(piece.slice(0, hyphen).trim(), pageCount, file);
    if (typeof from === "string") return { problem: from };
    const to = readPage(piece.slice(hyphen + 1).trim(), pageCount, file);
    if (typeof to === "string") return { problem: to };
    if (from > to) {
      return { problem: TEXT().backwards(from, to) };
    }
    for (let page = from; page <= to; page += 1) slots.add(page - 1);
  }

  // Sorted numerically. The default sort is lexicographic, which puts page 10
  // before page 2 and would hand `write_copy` a plan in an order no reader
  // asked for --- and one that still writes a valid PDF, so nothing downstream
  // could report it.
  return { slots: [...slots].sort((a, b) => a - b) };
}

/** One page number, one-based, or why it is not one. */
function readPage(text: string, pageCount: number, file?: string): number | string {
  if (text === "") return TEXT().numberMissing;
  // Digits only, deliberately: `+2`, `2.0` and `1e1` are all things a reader
  // could type and none of them is a page number, while `Number()` accepts
  // every one of them.
  if (!/^[0-9]+$/.test(text)) return TEXT().notANumber(text);
  const page = Number(text);
  if (page < 1 || page > pageCount) {
    return file === undefined ? TEXT().documentHas(pageCount) : TEXT().fileHas(file, pageCount);
  }
  return page;
}

/** Consecutive runs of pages a split would write, or why the text is not usable. */
export type SplitPoints =
  | { groups: number[][]; problem?: undefined }
  | { groups?: undefined; problem: string };

/**
 * Reads `raw` as the pages a split cuts *after*, against a document of
 * `pageCount` pages.
 *
 * `3,7` on ten pages is three files: 1-3, 4-7, 8-10. The numbers are the last
 * page of a file rather than the first page of the next one, because that is
 * how a reader describes a document --- "the report ends on page 7" --- and
 * because it makes the first file's boundary sayable, which "first page of the
 * next" cannot do without naming page 1 and meaning nothing.
 *
 * **Not the same grammar as {@link parsePageRange}, deliberately.** Extract
 * takes a *set* and split takes *cuts*, so `1-3` has no meaning here and is
 * refused rather than quietly read as two boundaries. Two commands whose
 * argument boxes look identical and read the same text differently is worse
 * than one of them being narrower.
 *
 * **"Every N pages" is not this and is not built.** It was the other candidate
 * grammar and it collides: a bare `3` would have to mean either "cut after page
 * 3" or "files of three pages", and only the first composes with a list. A
 * reader who wants fives on a twenty-page document writes `5,10,15`.
 */
export function parseSplitPoints(raw: string, pageCount: number): SplitPoints {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { problem: TEXT().pagesToCutAfter(pageCount - 1) };
  }
  if (pageCount < 2) {
    return { problem: TEXT().onePage };
  }

  const cuts = new Set<number>();
  for (const part of trimmed.split(",")) {
    const piece = part.trim();
    if (piece === "") return { problem: TEXT().emptyPart(trimmed) };
    // A hyphen is refused by name rather than by `readPage` failing on it. The
    // reader has almost certainly typed an extract range into the wrong box,
    // and `"1-3" is not a page number` sends them to look for a typo that is
    // not there.
    if (piece.includes("-")) {
      return { problem: TEXT().notARange(piece) };
    }
    const page = readPage(piece, pageCount);
    if (typeof page === "string") return { problem: page };
    // The last page is not a cut. Allowing it writes a final file of no pages,
    // and the reader who typed it meant the document they already have.
    if (page === pageCount) {
      return { problem: TEXT().lastPage(pageCount) };
    }
    if (cuts.has(page)) return { problem: TEXT().namedTwice(page) };
    cuts.add(page);
  }

  // Sorted, for `parsePageRange`'s reason and with the same limit: `7,3` and
  // `3,7` name the same set of cuts, so ordering them is not a correction. A
  // repeated cut *is* refused above, because that one cannot mean anything.
  const ordered = [...cuts].sort((a, b) => a - b);
  const groups: number[][] = [];
  let from = 0;
  for (const cut of [...ordered, pageCount]) {
    const run: number[] = [];
    for (let slot = from; slot < cut; slot += 1) run.push(slot);
    groups.push(run);
    from = cut;
  }
  return { groups };
}

/** What the palette shows under a valid split argument. */
export function describeSplit(groups: number[][]): string {
  const sizes = groups.map((group) => group.length);
  return TEXT().split(groups.length, sizes.join(" + "));
}

/**
 * What the palette shows under the input while the text is still valid.
 *
 * Counts rather than lists, because a selection can be most of a 775-page
 * document and the preview line is one line.
 */
export function describeRange(slots: number[]): string {
  const only = slots.length === 1 ? slots[0] : undefined;
  if (only !== undefined) return TEXT().extractOne(only + 1);
  return TEXT().extractMany(slots.length);
}

/**
 * Names a selection the way a reader wrote it, for a suggested filename.
 *
 * The parser's inverse, and it lives here so the two forms stay one idea: runs
 * are collapsed back to `1-3` rather than spelled `1,2,3`, because that is what
 * was typed and a filename is not a place to expand a selection.
 *
 * Slots in, printed page numbers out. `namePages` and `parsePageRange` are not
 * asserted to round-trip in general --- `5,1` comes back as `pages 1,5`, which
 * is the same *set* and deliberately not the same string, since document order
 * is what extract produces.
 */
export function namePages(slots: number[]): string {
  if (slots.length === 0) return TEXT().noPages;
  const sorted = [...new Set(slots)].sort((a, b) => a - b);
  const parts: string[] = [];
  let run = 0;
  while (run < sorted.length) {
    const from = sorted[run] as number;
    let last = run;
    while (last + 1 < sorted.length && (sorted[last + 1] as number) === (sorted[last] as number) + 1) {
      last += 1;
    }
    const to = sorted[last] as number;
    // A run of two is written out rather than hyphenated: `1-2` is no shorter
    // than `1,2` and reads as a range where there is none worth naming.
    parts.push(to - from >= 2 ? `${from + 1}-${to + 1}` : rangeOfTwo(from, to));
    run = last + 1;
  }
  return TEXT().named(sorted.length, parts.join(","));
}

/** One or two adjacent pages, spelled out. */
function rangeOfTwo(from: number, to: number): string {
  return from === to ? `${from + 1}` : `${from + 1},${to + 1}`;
}
