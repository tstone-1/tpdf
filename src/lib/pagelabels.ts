/**
 * The page number printed on a page, where it is not the page's position.
 *
 * Every page number a reader types into tpdf is a position: `5` in the page
 * range of *Extract pages* is the fifth page of the file, as `pageranges.ts`
 * reads it. A report with a cover and roman-numbered front matter prints a
 * different number on that page, and a reader who extracts "pages 5 to 9" from
 * the table of contents gets the wrong pages with nothing to say so. Until
 * 2026-11-13 tpdf did not read `/PageLabels` at all.
 *
 * So the number is shown, not accepted: the page counter, the go-to-page
 * preview and the range previews show the printed number beside the position
 * wherever the two differ, and the reader keeps typing positions. Accepting
 * printed numbers as well would make `5` ambiguous on exactly the documents
 * this is for.
 *
 * The printed number is the document's own statement, `/PageLabels`, which
 * PDFium reads with the outline (`outline.rs`). A document that prints numbers
 * on its pages but states no labels has nothing here to read.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    printedAs: (printed: string) => `printed as ${printed}`,
  },
  "de-DE": {
    printedAs: (printed: string) => `gedruckt als ${printed}`,
  },
});

/** At most this many runs are spelled out in a preview; the rest are `…`. */
const MAX_RUNS = 4;

/**
 * The label of the page at `position` (one-based), when it says something the
 * position does not.
 *
 * `source` is the page of the opened file the position shows, zero-based, or
 * `undefined` for a page that is not one: a blank page tpdf made, or a page
 * from another file. A label belongs to the page of the file and not to the
 * position, so after a deletion the page printed `2` sits at position 1 and is
 * shown as printed `2`.
 */
export function printedLabel(labels: readonly string[], source: number | undefined, position: number): string | null {
  if (source === undefined) return null;
  const label = labels[source];
  if (!label || label === String(position)) return null;
  return label;
}

/**
 * `printed as i–iii, 6` for runs of consecutive slots, or `""` when no page in
 * them is printed with a number other than its position.
 *
 * `labelOf` is {@link printedLabel} for a zero-based slot. A run is named by
 * its first and last page, each by its label or, where it has none, by its
 * position, with an en dash because a label may hold a hyphen (`App-A`), so a run that crosses from the front matter into the body reads
 * `ii–3`. Past {@link MAX_RUNS} the rest is `…`: the preview is one line.
 */
export function printedRuns(runs: readonly (readonly number[])[], labelOf: (slot: number) => string | null): string {
  if (!runs.some((run) => run.some((slot) => labelOf(slot) !== null))) return "";
  const name = (slot: number) => labelOf(slot) ?? String(slot + 1);
  const parts = runs.slice(0, MAX_RUNS).map((run) => {
    const first = run[0];
    const last = run[run.length - 1];
    if (first === undefined || last === undefined) return "";
    return first === last ? name(first) : `${name(first)}–${name(last)}`;
  });
  if (runs.length > MAX_RUNS) parts.push("…");
  return TEXT().printedAs(parts.filter((part) => part !== "").join(", "));
}

/** Sorted slots as runs of consecutive slots: `[0,1,2,5]` is `[[0,1,2],[5]]`. */
export function runsOf(slots: readonly number[]): number[][] {
  const runs: number[][] = [];
  for (const slot of slots) {
    const run = runs[runs.length - 1];
    if (run && run[run.length - 1] === slot - 1) run.push(slot);
    else runs.push([slot]);
  }
  return runs;
}

/** `preview · printed as …`, or `preview` alone when there is nothing printed to add. */
export function withPrinted(preview: string, printed: string): string {
  if (printed === "") return preview;
  return preview === "" ? printed : `${preview} · ${printed}`;
}
