/**
 * *Compare with*, from the window: the pages it sends, what it says when the
 * answer is back, and where each change is on each side.
 *
 * `App.svelte` keeps the `invoke` and the two viewers; what is decided is here,
 * so it has tests. The comparison itself is `src-tauri/src/compare.rs`, which
 * `tpdf compare` runs too, so the window and the command line list the same
 * changes. The list is `changelist.ts`.
 *
 * **Text only.** What is compared is the words the text extraction reads out
 * of each page --- the characters search and selection read. A changed picture,
 * drawing, colour or layout is no change of words, and {@link sentences}
 * always says so: "no changes" is a statement about the words.
 *
 * **Pages are the window's slots.** Each document is sent in the order the
 * reader sees its pages, so a document with pages moved, inserted or deleted
 * is compared as it is shown and every span comes back on the slot it is
 * painted on.
 */

import type { PageText } from "./text";

/** One page's characters, as `compare_texts` takes them. `PageCodes` in `compare.rs`. */
export interface PageCodes {
  /** The slot, counted from 0. */
  page: number;
  codes: number[];
}

/** A run of changed characters on one page. `Span` in `compare.rs`. */
export interface Span {
  page: number;
  start: number;
  end: number;
}

/** Where a change sits on a side that has none of its words. `Place` in `compare.rs`. */
export interface Place {
  page: number;
  index: number;
}

export type ChangeKind = "removed" | "added" | "replaced";

/** One change, from the left document to the right. `Change` in `compare.rs`. */
export interface Change {
  kind: ChangeKind;
  left: Span[];
  right: Span[];
  left_at: Place;
  right_at: Place;
  removed: string;
  added: string;
  before: string;
  after: string;
}

/** What `compare_texts` answers. `Comparison` in `compare.rs`. */
export interface Comparison {
  left_words: number;
  right_words: number;
  removed_words: number;
  added_words: number;
  changes: Change[];
  total_changes: number;
  unaligned: number;
  /** Left words not compared: they stand where the right has a page with no text. */
  left_unread: number;
  /** Right words not compared, against a page of the left. */
  right_unread: number;
}

/** Which of the two documents. The left is the older: what was removed is on it. */
export type Side = "left" | "right";

/** A finished comparison, with what the panel says about the two documents. */
export interface Compared {
  result: Comparison;
  /** The two file names, as their tabs show them. */
  names: Record<Side, string>;
  /** How many of each document's pages held no characters. */
  withoutText: Record<Side, number>;
}

/** The line shown while the pages are being read. */
export const STARTING = "Reading both documents...";

/** The line shown while the words are being compared. */
export const COMPARING = "Comparing the words...";

/** What the panel says before a comparison has been made. */
export const NOT_COMPARED =
  "Not compared yet. “Compare with...” lists what changed in the words between " +
  "this document and another open one.";

/** Said under every result, because a result is about the words alone. */
export const TEXT_ONLY =
  "Only the words were compared. Pictures, drawings, colors, fonts and the position of text were not.";

/** What the panel says once either document has changed since it was compared. */
export const OUT_OF_DATE =
  "One of the documents has changed since it was compared. Compare them again to see what differs now.";

/** Rows drawn at once. Beyond this the panel says how many it is not showing. */
export const MAX_ROWS = 2000;

/**
 * Every page's characters, in slot order, read through `load`.
 *
 * `load` is the viewer's: it answers `null` for a page tpdf made, which has no
 * characters, and that page is sent as empty rather than left out, so the slot
 * numbers stay those of the document shown.
 */
export async function gather(
  slots: number,
  load: (slot: number) => Promise<PageText | null>,
): Promise<{ pages: PageCodes[]; withoutText: number }> {
  const pages: PageCodes[] = [];
  let withoutText = 0;
  for (let slot = 0; slot < slots; slot++) {
    const text = await load(slot);
    const codes = text?.codes ?? [];
    if (codes.length === 0) withoutText++;
    pages.push({ page: slot, codes });
  }
  return { pages, withoutText };
}

/**
 * Why the two documents are not compared, or `null` when they are: a document
 * none of whose pages has text has no words to compare, and comparing it would
 * list every word of the other as removed or added. `tpdf compare` refuses the
 * same pair with exit 3.
 */
export function refusal(
  names: Record<Side, string>,
  gathered: Record<Side, { pages: PageCodes[]; withoutText: number }>,
): string | null {
  for (const side of ["left", "right"] as const) {
    if (gathered[side].withoutText === gathered[side].pages.length) {
      return `${names[side]}: none of its pages has text, so there are no words to compare. A scan has to be recognised first.`;
    }
  }
  return null;
}

function plural(count: number, one: string, many: string): string {
  return `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;
}

/** The first line when neither document has a word to compare. */
export const NOTHING_COMPARED = "Neither document has any text, so nothing was compared.";

/** The first line when no word changed but some pages were not read. */
export const SAME_AS_FAR_AS_READ =
  "Incomplete: no change in the words that were read, but some pages have no text and were not compared.";

/**
 * What the panel says above its rows, a sentence to a line.
 *
 * The count first, then which document is which, then the bounds that were
 * reached, then pages with no words, and last, always, {@link TEXT_ONLY}.
 *
 * "The same words" is said only when every page of both had text. With a page
 * of either unread, a result with no changes leads with that instead, so two
 * scans never open with a sentence a reader takes for "identical".
 */
export function sentences(compared: Compared): string[] {
  const { result, names, withoutText } = compared;
  const said: string[] = [];
  if (result.total_changes === 0) {
    if (result.left_words === 0 && result.right_words === 0) {
      said.push(NOTHING_COMPARED);
    } else if (withoutText.left + withoutText.right > 0) {
      said.push(SAME_AS_FAR_AS_READ);
    } else {
      said.push("The two documents have the same words.");
    }
  } else {
    said.push(
      `${plural(result.total_changes, "change", "changes")}: ` +
        `${plural(result.removed_words, "word", "words")} removed and ` +
        `${plural(result.added_words, "word", "words")} added.`,
    );
  }
  said.push(`From ${names.left}, in red, to ${names.right}, in green.`);
  const rows = Math.min(result.changes.length, MAX_ROWS);
  if (result.total_changes > rows) {
    said.push(`Showing the first ${rows.toLocaleString("en-US")} of ${result.total_changes.toLocaleString("en-US")}.`);
  }
  if (result.unaligned > 0) {
    said.push(
      `${plural(result.unaligned, "passage was", "passages were")} too different to match word ` +
        "by word, and each is listed as one replacement.",
    );
  }
  for (const side of ["left", "right"] as const) {
    const empty = withoutText[side];
    if (empty > 0) {
      said.push(
        `${names[side]} has ${plural(empty, "page", "pages")} with no text, which ` +
          `${empty === 1 ? "was" : "were"} not compared. A scanned page has to be recognised first.`,
      );
    }
  }
  for (const [side, other] of [["left", "right"], ["right", "left"]] as const) {
    const unread = side === "left" ? result.left_unread : result.right_unread;
    if (unread > 0) {
      said.push(
        `${plural(unread, "word", "words")} of ${names[side]} ${unread === 1 ? "stands" : "stand"} where ` +
          `${names[other]} has a page with no text, and ${unread === 1 ? "was" : "were"} not compared: ` +
          "the change may be on that page.",
      );
    }
  }
  said.push(TEXT_ONLY);
  return said;
}

/** What a row says before the words: `Page 2, changed`, or `Page 2 → page 3, added`. */
export function rowLabel(change: Change): string {
  const left = change.left_at.page + 1;
  const right = change.right_at.page + 1;
  const pages = left === right ? `Page ${left}` : `Page ${left} → page ${right}`;
  const what = { removed: "removed", added: "added", replaced: "changed" }[change.kind];
  return `${pages}, ${what}`;
}

/**
 * Where `change` is on `side`, for scrolling there: the start of its first
 * span, or the place its words would stand when that side has none of them.
 */
export function placeOf(change: Change, side: Side): Place {
  const first = (side === "left" ? change.left : change.right)[0];
  if (first) return { page: first.page, index: first.start };
  return side === "left" ? change.left_at : change.right_at;
}

/** One side's spans, each with the index of the change it belongs to. */
export interface Painted extends Span {
  change: number;
}

/** Every span of `side`, for the viewer to paint. */
export function paintedOn(result: Comparison, side: Side): Painted[] {
  const out: Painted[] = [];
  result.changes.forEach((change, index) => {
    for (const span of side === "left" ? change.left : change.right) {
      out.push({ ...span, change: index });
    }
  });
  return out;
}

/**
 * The change after (`1`) or before (`-1`) `current`, wrapping; the first or the
 * last when none is current. `-1` when there are none.
 */
export function step(current: number, count: number, direction: 1 | -1): number {
  if (count <= 0) return -1;
  if (current < 0 || current >= count) return direction === 1 ? 0 : count - 1;
  return (current + direction + count) % count;
}

/**
 * The comparison on screen: which two views it is about, the change the reader
 * is on, and whether either document has changed since.
 *
 * One for the window, not one per document: a comparison is about two of them.
 * `V` is the view id; a view that is closed ends it, and an edit to either
 * makes it out of date, which takes the paint off both pages (the spans name
 * characters an edit can have moved) and keeps the rows.
 */
export class ComparisonState<V> {
  private held: { left: V; right: V; compared: Compared; current: number; stale: boolean } | null = null;

  /** Holds a new comparison, `left` being the older document's view. */
  start(left: V, right: V, compared: Compared): void {
    this.held = { left, right, compared, current: -1, stale: false };
  }

  /** The comparison, or `null` with none. */
  get compared(): Compared | null {
    return this.held?.compared ?? null;
  }

  /** How many changes it lists; 0 with none. */
  get count(): number {
    return Math.min(this.held?.compared.result.changes.length ?? 0, MAX_ROWS);
  }

  /** The change the reader is on, or -1. */
  get current(): number {
    return this.held?.current ?? -1;
  }

  /** Whether either document has changed since it was compared. */
  get stale(): boolean {
    return this.held?.stale ?? false;
  }

  /** The two views, or `null` with no comparison. */
  get views(): { left: V; right: V } | null {
    return this.held ? { left: this.held.left, right: this.held.right } : null;
  }

  /** Which side `view` is, or `null` when the comparison is not about it. */
  sideOf(view: V): Side | null {
    if (!this.held) return null;
    if (this.held.left === view) return "left";
    if (this.held.right === view) return "right";
    return null;
  }

  /** Makes `index` current, when it is a change; answers whether it is. */
  pick(index: number): boolean {
    if (!this.held || index < 0 || index >= this.count) return false;
    this.held.current = index;
    return true;
  }

  /** Moves to the next or previous change and answers it, or -1 with none. */
  step(direction: 1 | -1): number {
    const next = step(this.current, this.count, direction);
    if (next >= 0 && this.held) this.held.current = next;
    return next;
  }

  /** Marks it out of date when it is about `view`; answers whether it was just marked. */
  invalidate(view: V): boolean {
    if (!this.held || this.sideOf(view) === null || this.held.stale) return false;
    this.held.stale = true;
    return true;
  }

  /** Ends it when it is about `view`, and answers the two views it was about. */
  end(view: V): { left: V; right: V } | null {
    if (!this.held || this.sideOf(view) === null) return null;
    const views = this.views;
    this.held = null;
    return views;
  }

  /**
   * What `view`'s viewer paints: its side's spans, as removed or added, and the
   * current change; `null` when it paints nothing, which an out-of-date
   * comparison is.
   */
  paintFor(view: V): { spans: Painted[]; side: "removed" | "added"; current: number } | null {
    const side = this.sideOf(view);
    if (!this.held || side === null || this.held.stale) return null;
    return {
      spans: paintedOn(this.held.compared.result, side),
      side: side === "left" ? "removed" : "added",
      current: this.held.current,
    };
  }
}
