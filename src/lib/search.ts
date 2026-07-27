/**
 * Walking a document for a query, one page at a time.
 *
 * The matching itself is in Rust (`src-tauri/src/search.rs`), over the same
 * character codes selection reads, so a hit arrives as a range of character
 * indices and highlighting it is the selection code with a different colour.
 * What is here is the walk: which page to ask about next, when to stop, and how
 * to abandon a scan the moment the query changes.
 *
 * ## One page per request, sequentially
 *
 * The render thread is FIFO and shared with tiles. A single request that scanned
 * the whole document would hold it for a second and a half on the 775-page
 * corpus and every tile queued behind it would wait, so the unit is a page. They
 * are asked for one at a time rather than in parallel for the same reason there
 * is one render thread at all --- concurrent requests would not be served
 * concurrently, they would only make the queue longer and cancellation slower.
 *
 * ## Cancellation is not asking again
 *
 * There is nothing to withdraw: a page search is milliseconds, so a superseded
 * scan is abandoned by ignoring its remaining pages rather than by cancelling
 * anything. The generation counter is what makes that safe --- a reply that
 * arrives after the query changed belongs to a search nobody is running any
 * more, and adding its matches to the current one is the bug this exists to
 * prevent.
 */

import { invoke } from "@tauri-apps/api/core";

/** A hit, as half-open character indices into the page's own text. */
export interface Match {
  page: number;
  start: number;
  end: number;
}

/** What one page contributed. */
interface PageMatches {
  page: number;
  matches: Match[];
  /** Characters the page has at all --- see {@link Search.textless}. */
  chars: number;
}

/** Scans a document for a query, accumulating matches as they are found. */
export class Search {
  private readonly doc: number;
  private readonly pageCount: number;
  private readonly onChange: () => void;
  /** Bumped by every `run` and `cancel`; replies from an older one are dropped. */
  private generation = 0;

  /** The query being scanned for, or "". */
  query = "";
  /**
   * Matches found so far, in the order they were found --- which is reading
   * order starting from the page the search began on, wrapping at the end.
   *
   * That is the order Enter should step through, so it is the order they are
   * kept in rather than sorted by page: a reader searching from page 400 wants
   * the next hit after page 400, not the first hit in the document.
   */
  matches: Match[] = [];
  /** Pages asked about so far, out of `pageCount`. */
  scanned = 0;
  /** Characters seen across those pages. */
  charsSeen = 0;
  /** Whether a scan is in progress. */
  running = false;
  /**
   * Wall time of the last scan that ran to completion, in milliseconds.
   *
   * A whole-document scan is the one cost here worth a number, and this is the
   * only place it can be taken from end to end. The 1 ms clock clamp the webview
   * applies is irrelevant at this scale --- see `clock.ts` for where it is not.
   */
  elapsedMs = 0;
  private startedAt = 0;

  constructor(doc: number, pageCount: number, onChange: () => void) {
    this.doc = doc;
    this.pageCount = pageCount;
    this.onChange = onChange;
  }

  /**
   * Whether every page looked at so far had no extractable text.
   *
   * A scan of a scanned document finds nothing, and reporting that as "no
   * matches" is a lie of omission --- the query was never tested against
   * anything. docs/PLAN.md section 9 measured the A0 sheet at zero characters,
   * which is the correct answer for it and the case this distinguishes.
   */
  get textless(): boolean {
    return this.scanned > 0 && this.charsSeen === 0;
  }

  /** Abandons any scan in progress. */
  cancel(): void {
    this.generation++;
    this.running = false;
  }

  /** Clears the query and everything found for it. */
  clear(): void {
    this.cancel();
    this.query = "";
    this.matches = [];
    this.scanned = 0;
    this.charsSeen = 0;
    this.onChange();
  }

  /**
   * Scans the whole document, starting at `from` and wrapping.
   *
   * Resolves when the scan finishes or is superseded. Matches appear in
   * `matches` as they are found, and `onChange` fires for every page --- a
   * search over a long document has to be visibly working, not silently
   * pending.
   */
  async run(query: string, from: number): Promise<void> {
    this.cancel();
    const generation = ++this.generation;

    this.query = query;
    this.matches = [];
    this.scanned = 0;
    this.charsSeen = 0;
    if (!query) {
      this.onChange();
      return;
    }

    this.running = true;
    this.startedAt = performance.now();
    this.onChange();

    const start = Math.max(0, Math.min(from, this.pageCount - 1));
    for (let step = 0; step < this.pageCount; step++) {
      const page = (start + step) % this.pageCount;
      let result: PageMatches | null = null;
      try {
        result = await invoke<PageMatches>("search_page", {
          doc: this.doc,
          page,
          query,
        });
      } catch {
        // A page that cannot be read is skipped rather than abandoning the
        // scan: one damaged page should not hide the hits on the other 774.
        result = null;
      }

      // Superseded while that request was outstanding. Nothing here belongs to
      // the search that is running now.
      if (generation !== this.generation) return;

      this.scanned++;
      if (result) {
        this.charsSeen += result.chars;
        if (result.matches.length > 0) this.matches.push(...result.matches);
      }
      this.onChange();
    }

    this.running = false;
    this.elapsedMs = performance.now() - this.startedAt;
    this.onChange();
  }
}
