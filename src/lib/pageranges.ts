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
 */
export function parsePageRange(raw: string, pageCount: number): PageRange {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { problem: `Pages to extract, 1 to ${pageCount}` };
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
      return { problem: `"${trimmed}" has an empty part` };
    }

    const hyphen = piece.indexOf("-");
    if (hyphen === -1) {
      const one = readPage(piece, pageCount);
      if (typeof one === "string") return { problem: one };
      slots.add(one - 1);
      continue;
    }

    const from = readPage(piece.slice(0, hyphen).trim(), pageCount);
    if (typeof from === "string") return { problem: from };
    const to = readPage(piece.slice(hyphen + 1).trim(), pageCount);
    if (typeof to === "string") return { problem: to };
    if (from > to) {
      return { problem: `${from}-${to} runs backwards` };
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
function readPage(text: string, pageCount: number): number | string {
  if (text === "") return `a page number is missing`;
  // Digits only, deliberately: `+2`, `2.0` and `1e1` are all things a reader
  // could type and none of them is a page number, while `Number()` accepts
  // every one of them.
  if (!/^[0-9]+$/.test(text)) return `"${text}" is not a page number`;
  const page = Number(text);
  if (page < 1 || page > pageCount) {
    return `This document has ${pageCount} page${pageCount === 1 ? "" : "s"}`;
  }
  return page;
}

/**
 * What the palette shows under the input while the text is still valid.
 *
 * Counts rather than lists, because a selection can be most of a 775-page
 * document and the preview line is one line.
 */
export function describeRange(slots: number[]): string {
  const only = slots.length === 1 ? slots[0] : undefined;
  if (only !== undefined) return `Extract page ${only + 1}`;
  return `Extract ${slots.length} pages`;
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
  if (slots.length === 0) return "no pages";
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
  const word = sorted.length === 1 ? "page" : "pages";
  return `${word} ${parts.join(",")}`;
}

/** One or two adjacent pages, spelled out. */
function rangeOfTwo(from: number, to: number): string {
  return from === to ? `${from + 1}` : `${from + 1},${to + 1}`;
}
