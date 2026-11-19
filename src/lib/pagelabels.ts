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
    typePositions: (printed: string, positions: string) => `Printed as ${printed}: type ${positions}`,
  },
  "de-DE": {
    printedAs: (printed: string) => `gedruckt als ${printed}`,
    typePositions: (printed: string, positions: string) => `Gedruckt als ${printed}: ${positions} eingeben`,
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

/**
 * What to type instead of printed numbers, for a value the page-number parser
 * refused: `Printed as ii, App-A: type 2,7`, or `null` when the value is not
 * made of this document's labels.
 *
 * The parser still refuses the value, for the reason at the top of this file:
 * accepting labels would make `5` ambiguous. But a reader asked for "pages ii
 * and App-A" was told `"ii" is not a page number` and had to find the
 * positions alone (measured with NVDA, 2026-11-15). Only a refused value gets
 * here, so a number is never reinterpreted.
 *
 * Each comma part is a position, a label, or two of them joined by `-`. A label
 * may hold a hyphen itself (`App-A`), so every `-` is tried as the join. A
 * label printed on more than one page names no position, and the value is then
 * left to the parser's own refusal. `labelOf` is {@link printedLabel} for a
 * zero-based slot.
 */
export function positionsFor(
  raw: string,
  pageCount: number,
  labelOf: (slot: number) => string | null,
): string | null {
  const slotsOf = new Map<string, number[]>();
  for (let slot = 0; slot < pageCount; slot++) {
    const label = labelOf(slot);
    if (label === null) continue;
    // Pushed, not copied: a document whose every page prints the same label
    // would otherwise copy a growing list once per page.
    const slots = slotsOf.get(label);
    if (slots) slots.push(slot);
    else slotsOf.set(label, [slot]);
  }
  if (slotsOf.size === 0) return null;
  let usedLabel = false;
  const one = (text: string): string | null => {
    if (/^[0-9]+$/.test(text)) return text;
    const slots = slotsOf.get(text);
    if (slots?.length !== 1) return null;
    usedLabel = true;
    return String(slots[0]! + 1);
  };
  const parts = raw.split(",").map((part) => part.trim());
  const typed: string[] = [];
  for (const part of parts) {
    const whole = one(part);
    if (whole !== null) {
      typed.push(whole);
      continue;
    }
    let joined: string | null = null;
    for (let at = part.indexOf("-"); at > 0 && joined === null; at = part.indexOf("-", at + 1)) {
      const from = one(part.slice(0, at).trim());
      const to = from === null ? null : one(part.slice(at + 1).trim());
      if (from !== null && to !== null) joined = `${from}-${to}`;
    }
    if (joined === null) return null;
    typed.push(joined);
  }
  return usedLabel ? TEXT().typePositions(parts.join(", "), typed.join(",")) : null;
}
