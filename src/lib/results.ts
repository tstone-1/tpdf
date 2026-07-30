/**
 * The search-results tab: every hit in the document, one row each.
 *
 * `docs/PLAN.md` §8 asks for this as the third sidebar tab, and the reason it is
 * worth building rather than leaving the find bar's counter to stand for it is
 * the same reason the palette exists: `12 of 5712` tells a reader how much there
 * is and nothing about what is in it. A list of snippets is the difference
 * between stepping through five thousand hits and picking the one you wanted.
 *
 * ## The snippets come from the backend, and that is not an optimisation
 *
 * A row shows the words around its hit. Those words are on the *page*, and the
 * frontend does not have the page --- `search.rs` extracts the text, matches
 * against it, and drops it again, so only the hits cross. Building snippets here
 * would mean re-fetching every page a hit is on, which on a 775-page document is
 * the whole document's text in order to show a screenful of it. So a `Match`
 * carries `before`, `hit` and `after`, built where the characters already are.
 *
 * ## It is bounded, in rows and not in matches
 *
 * A one-letter query on the dense corpus finds tens of thousands of hits. The
 * *count* stays exact --- a reader has to be able to tell "many" from "a few" ---
 * and only the rows are capped, with the cap stated in the panel rather than
 * silently applied. This is the outline's arrangement (`sidebar.ts`) and for the
 * same reason: bounding the input honestly beats a windowing implementation that
 * does not exist yet.
 *
 * ## Rows are not rebuilt on every reply
 *
 * The scan reports after every page, which on 775 pages is 775 notifications. A
 * panel that rebuilt its list each time would rebuild it 775 times, and the last
 * of those would be the only one anybody saw. Rows are appended for matches that
 * have arrived since the last paint, and the whole list is rebuilt only when the
 * query changes underneath it.
 */

import type { Match } from "./search";

/** Rows drawn at once. Beyond this the panel says how many it is not showing. */
export const MAX_RESULT_ROWS = 2000;

export interface ResultsOptions {
  /** Called when a row is activated, with its index into the match list. */
  onPick: (index: number) => void;
}

/** Rows for the hits found so far, appended as the scan walks. */
export class Results {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: ResultsOptions;

  /** Rows built, which is `min(matches.length, MAX_RESULT_ROWS)`. */
  private built = 0;
  /** Matches the built rows describe, so a replaced list is detected. */
  private shown: readonly Match[] = [];
  /** Index of the highlighted row, or -1. */
  private currentIndex = -1;
  private readonly rows: HTMLElement[] = [];
  /** What the notice last said, so an unchanged frame writes nothing. */
  private said = "";

  constructor(host: HTMLElement, opts: ResultsOptions) {
    this.opts = opts;

    // A live region, like the outline's cap notice: it changes while a reader is
    // already in the panel, and "no matches" arriving silently is the case this
    // whole tab exists to make visible.
    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText = "flex:none;padding:0.3rem 0.7rem;opacity:0.7;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.setAttribute("aria-label", "Search results");
    this.list.style.cssText = "flex:1;min-height:0;overflow-y:auto;";

    host.append(this.notice, this.list);
  }

  /** Rows currently built. For the check harness and the tests. */
  get rowCount(): number {
    return this.built;
  }

  /** What the panel says above the list. For the check harness and the tests. */
  get status(): string {
    return this.said;
  }


  /** Index of the highlighted row, or -1. For the tests. */
  get highlighted(): number {
    return this.currentIndex;
  }

  /** A built row, so the check harness can press it. */
  rowAt(index: number): HTMLElement | null {
    return this.rows[index] ?? null;
  }

  /**
   * What a built row displays, read back out of the DOM. For the check harness.
   *
   * Read back rather than reported from the match it was built from: the claim
   * is that the row *says* what the document says, and a getter returning the
   * match would agree with itself whatever the row actually contains.
   */
  rowText(index: number): { page: string; bold: string; whole: string } {
    const row = this.rows[index];
    if (!row) return { page: "", bold: "", whole: "" };
    const [page, text] = [...row.children] as HTMLElement[];
    const parts = text ? ([...text.children] as HTMLElement[]) : [];
    return {
      page: page?.textContent ?? "",
      bold: text?.querySelector("strong")?.textContent ?? "",
      // Joined from the three parts rather than read off their container. A
      // container's `textContent` is an aggregate the real DOM computes and the
      // fake one in `testdom.ts` does not, so reading it works in the check
      // harness and silently returns "" in a unit test --- which is the same
      // answer an empty row gives.
      whole: parts.map((part) => part.textContent ?? "").join(""),
    };
  }

  /**
   * Redraws for the matches found so far.
   *
   * `query` is passed so that an empty one can be told from a scan that has
   * found nothing yet: they read identically from the match list and mean
   * opposite things to a reader.
   */
  update(
    matches: readonly Match[],
    current: number,
    query: string,
    running: boolean,
  ): void {
    // A different array, or one that has been emptied, means a new query: the
    // rows describe matches that no longer exist and appending to them would
    // interleave two searches. Identity is the test rather than length, because
    // a new query on the same page count can find the same number of hits.
    if (matches !== this.shown) {
      this.list.replaceChildren();
      this.rows.length = 0;
      this.built = 0;
      this.shown = matches;
      this.currentIndex = -1;
    }

    for (let i = this.built; i < matches.length && i < MAX_RESULT_ROWS; i++) {
      const match = matches[i];
      if (!match) break;
      this.list.appendChild(this.row(match, i));
    }
    this.built = Math.min(matches.length, MAX_RESULT_ROWS);

    this.highlight(current);
    this.say(matches.length, query, running);
  }

  /** Moves the highlight, scrolling the row into view if it is off screen. */
  private highlight(index: number): void {
    if (index === this.currentIndex) return;
    this.paintRow(this.currentIndex, false);
    this.currentIndex = index;
    this.paintRow(index, true);
    // Only a row that exists, and only when it is out of view: a panel that
    // scrolled on every step would fight a reader scrolling it themselves.
    this.rows[index]?.scrollIntoView({ block: "nearest" });
  }

  private paintRow(index: number, on: boolean): void {
    const row = this.rows[index];
    if (!row) return;
    row.setAttribute("aria-selected", String(on));
    row.style.background = on
      ? "color-mix(in srgb, currentColor 12%, transparent)"
      : "";
  }

  /** Writes the line above the list, if it has changed. */
  private say(total: number, query: string, running: boolean): void {
    const text = statusFor(total, query, running);
    if (text === this.said) return;
    this.said = text;
    this.notice.textContent = text;
    this.notice.style.display = text ? "block" : "none";
  }

  /** One row: the page number, then the snippet with the hit emboldened. */
  private row(match: Match, index: number): HTMLElement {
    const row = document.createElement("div");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", "false");
    row.dataset.index = String(index);
    row.style.cssText =
      "display:flex;gap:0.5rem;padding:0.25rem 0.7rem;cursor:default;" +
      "align-items:baseline;";

    const page = document.createElement("span");
    page.textContent = String(match.page + 1);
    page.style.cssText =
      "flex:none;min-width:3ch;text-align:right;opacity:0.5;" +
      "font-variant-numeric:tabular-nums;";

    const text = document.createElement("span");
    text.style.cssText =
      "flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;";
    // Three elements rather than two text nodes around a `<strong>`, which is
    // what this was first: they render identically, and spans are what the check
    // harness and the fake DOM in `testdom.ts` can both see. The concatenation
    // is the snippet and nothing here computes an offset into it --- see the
    // note on `Match` in `search.rs` about why that matters.
    text.append(span(match.before), strong(match.hit), span(match.after));

    row.append(page, text);
    // `pointerdown`, not `click`: the find field usually has focus, and a click
    // blurs it first, which on this platform can move the viewport underneath
    // the row before the row is notified.
    row.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.opts.onPick(index);
    });
    this.rows.push(row);
    return row;
  }
}

/** A plain run of snippet text. */
function span(text: string): HTMLElement {
  const element = document.createElement("span");
  element.textContent = text;
  return element;
}

/** The matched run, which is the only emboldened part of a row. */
function strong(text: string): HTMLElement {
  const element = document.createElement("strong");
  element.textContent = text;
  return element;
}

/**
 * The line above the list.
 *
 * Exported for its own test: it is the only part of this panel that is a
 * decision rather than DOM, and it has four cases that a reader reads as four
 * different situations.
 */
export function statusFor(total: number, query: string, running: boolean): string {
  if (!query) return "Type in the find field to search.";
  if (total === 0) return running ? "Searching…" : "No matches.";
  const found = `${total} match${total === 1 ? "" : "es"}`;
  // The cap is stated, never silently applied: a list that stopped at 2,000 rows
  // without saying so is a document that appears to have 2,000 hits in it.
  const capped =
    total > MAX_RESULT_ROWS ? `, showing the first ${MAX_RESULT_ROWS}` : "";
  return running ? `${found}${capped}, still searching…` : `${found}${capped}`;
}
