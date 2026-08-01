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

/**
 * A hit, as half-open character indices into the page's own text, plus the
 * words around it.
 *
 * The snippet is three strings rather than a string and two offsets, because an
 * offset into it would be a third index space beside the page's code points and
 * JavaScript's UTF-16 --- see `src-tauri/src/search.rs`, which builds them where
 * the page's characters already are. Concatenating them is the snippet.
 */
export interface Match {
  page: number;
  start: number;
  /** Exclusive end, on {@link endPage} when there is one and on `page` otherwise. */
  end: number;
  /**
   * The page the hit finishes on, when that is not the one it starts on.
   *
   * A phrase can run over a page break, and a reader who types it does not know
   * there is a break in it. The hit is anchored on the page it starts on ---
   * that is where the search should take them --- and highlighted on both.
   */
  endPage?: number;
  /** Text immediately before the hit, whitespace collapsed. */
  before: string;
  /** The matched text, exactly as the page spells it. */
  hit: string;
  /** Text immediately after the hit, whitespace collapsed. */
  after: string;
}

/**
 * How a query is matched. Mirrors `search::Options` in Rust, which is where the
 * matching happens and where what each one means is written down.
 *
 * Both off is the plain search, and is what a reader who has never touched the
 * toggles gets.
 */
export interface SearchOptions {
  matchCase: boolean;
  wholeWord: boolean;
  /**
   * Read the query as a regular expression.
   *
   * The pattern is matched against the *folded* text --- one space for a run of
   * whitespace, no soft hyphens, case decided by `matchCase` rather than by an
   * inline flag --- which is the same haystack a literal query gets. Two
   * consequences a reader would not guess and `search.rs` spells out: `\n`
   * never occurs, and `^` anchors to the page rather than to a printed line.
   */
  regex: boolean;
}

/** No option set. */
export const PLAIN_SEARCH: SearchOptions = {
  matchCase: false,
  wholeWord: false,
  regex: false,
};

/** Whether two option sets ask for the same thing. */
export function sameOptions(a: SearchOptions, b: SearchOptions): boolean {
  return (
    a.matchCase === b.matchCase &&
    a.wholeWord === b.wholeWord &&
    a.regex === b.regex
  );
}

/**
 * A half-open character range on one page. `to` may be `Infinity`.
 *
 * The same shape `Selection.rangeOn` produces, deliberately: a scope is a
 * snapshot of a selection and converting between two spellings of it would be a
 * place for the two to disagree.
 */
export interface ScopeRange {
  page: number;
  from: number;
  to: number;
}

/**
 * Where a scan may look, or `null` for the whole document.
 *
 * ## It is filtered here rather than in the backend
 *
 * `search.rs` could take a range and refuse to look outside it, and the result
 * would be identical --- a hit is in scope when it lies entirely inside the
 * range, and that test needs nothing the frontend does not have. What the
 * backend *would* change is two things that should not change: the whole-word
 * boundary is decided by the characters either side of a hit on the **page**,
 * so a selection cutting through the middle of a word must not make that half a
 * whole word; and a snippet's context is the page's text around the hit, which
 * is what a reader wants to see even when the words either side were not
 * selected. Both are right by default when the scope is applied to the results
 * and wrong by default when it is applied to the haystack.
 *
 * The scan cost is unchanged: the pages outside the scope are never asked
 * about, which is the part that would have mattered, and that is a decision
 * about the walk rather than about the matching.
 *
 * ## It is a snapshot, not a live reading of the selection
 *
 * Captured when the reader scopes the search and held until they unscope it. A
 * scope that read the selection live would silently widen to the whole document
 * the moment they clicked on the page --- and clicking on the page is how you
 * dismiss a selection, so the search would quietly stop meaning what its own
 * label said.
 */
export type SearchScope = ScopeRange[];

/**
 * A page's last characters, handed to the request about the next page.
 *
 * Opaque here: it is produced by `search.rs` and handed back to it unread. The
 * walk's only job is to know *when* two requests are about adjacent pages, which
 * is a fact about the walk and not about the characters.
 */
interface Carry {
  page: number;
  from: number;
  codes: number[];
}

/** What one page contributed. */
interface PageMatches {
  page: number;
  matches: Match[];
  /** Characters the page has at all --- see {@link Search.textless}. */
  chars: number;
  /** Why the query could not be run --- see {@link Search.problem}. */
  problem?: string;
  /** This page's tail, absent when the query cannot span a break. */
  tail?: Carry;
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
  /** Pages asked about so far, out of {@link toScan}. */
  scanned = 0;
  /**
   * Pages this scan will ask about in total.
   *
   * The document's page count for an unscoped scan and the selection's pages
   * for a scoped one. Separate from `pageCount` because "the scan has finished"
   * is what every caller actually wants, and comparing against the document
   * would leave a scoped scan permanently unfinished --- a wait for a condition
   * that cannot hold, which is a trap this repository has paid for once.
   */
  toScan = 0;
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
  /**
   * Why the query could not be run at all, or "".
   *
   * Only a pattern can fail to be a query, and the reason it is surfaced rather
   * than folded into "no matches" is that a reader typing one *expects* to get
   * it wrong. `foo(` finds nothing, and a counter reading "no matches" says the
   * document does not contain it --- which is a different, false, statement.
   *
   * The scan stops on the first one. Every page would report the same problem,
   * since it is a property of the query, and walking 775 pages to be told 775
   * times is a queue in front of the tiles for no information.
   */
  problem = "";
  /**
   * Where the current scan was allowed to look, or null for everywhere.
   *
   * Kept so the counter can say "in selection": a count of three that silently
   * ignored the rest of the document is worse than no count.
   */
  scope: SearchScope | null = null;
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
    this.problem = "";
    this.onChange();
  }

  /**
   * Scans the whole document, starting at `from` and wrapping.
   *
   * `options` is passed to the backend rather than applied to the results here:
   * a whole-word filter over hits the backend already found would need the
   * page's text on this side to see what is next to them, which is the whole of
   * a 775-page document's characters to answer a question about a dozen hits.
   *
   * Resolves when the scan finishes or is superseded. Matches appear in
   * `matches` as they are found, and `onChange` fires for every page --- a
   * search over a long document has to be visibly working, not silently
   * pending.
   */
  async run(
    query: string,
    from: number,
    options: SearchOptions = PLAIN_SEARCH,
    scope: SearchScope | null = null,
  ): Promise<void> {
    this.cancel();
    const generation = ++this.generation;

    this.query = query;
    this.matches = [];
    this.scanned = 0;
    this.charsSeen = 0;
    this.problem = "";
    this.scope = scope;
    this.toScan = scope ? scope.length : this.pageCount;
    if (!query) {
      this.onChange();
      return;
    }

    this.running = true;
    this.startedAt = performance.now();
    this.onChange();

    // Two walks, and the difference is not only which pages. The unscoped one
    // starts where the reader is and **wraps**, so the first hit shown is the
    // next one after them rather than the first in the document. A scoped one
    // does not wrap: the pages are the selection's, in reading order, and
    // starting in the middle of a selection the reader just made would be
    // answering a question nobody asked.
    const plan: ScopeRange[] =
      scope ??
      Array.from({ length: this.pageCount }, (_unused, step) => ({
        page:
          (Math.max(0, Math.min(from, this.pageCount - 1)) + step) %
          this.pageCount,
        from: 0,
        to: Infinity,
      }));

    /**
     * The tail of the page scanned last, and which page that was.
     *
     * Handed to the next request when --- and only when --- the two pages are
     * adjacent, which is what lets `search.rs` find a phrase that runs over the
     * break between them. The walk is the only thing that knows: an unscoped
     * scan wraps, so the page after 774 is 0, and stitching those two together
     * would invent a phrase spanning the end of the document.
     */
    let carried: { page: number; carry: Carry } | null = null;

    /** Where each page's scope ends, for clipping a hit that spans a break. */
    const limitOn = new Map(plan.map((entry) => [entry.page, entry]));

    /**
     * Asks about one page and files what comes back. Returns "keep going".
     *
     * `joinsOnly` is for the second look at the page the walk began on: that
     * page has already been scanned and counted, so only the hits that cross
     * into it from the page before are new.
     */
    const visit = async (page: number, joinsOnly = false): Promise<boolean> => {
      const carry = carried?.page === page - 1 ? carried.carry : undefined;
      let result: PageMatches | null = null;
      try {
        result = await invoke<PageMatches>("search_page", {
          doc: this.doc,
          page,
          query,
          options,
          carry,
        });
      } catch {
        // A page that cannot be read is skipped rather than abandoning the
        // scan: one damaged page should not hide the hits on the other 774.
        result = null;
      }

      // Superseded while that request was outstanding. Nothing here belongs to
      // the search that is running now.
      if (generation !== this.generation) return false;

      if (result?.problem) {
        // A property of the query, not of this page, so there is nothing to be
        // learned from asking about the other 774.
        this.problem = result.problem;
        this.running = false;
        this.elapsedMs = performance.now() - this.startedAt;
        this.onChange();
        return false;
      }

      carried = result?.tail ? { page, carry: result.tail } : null;

      if (!joinsOnly) this.scanned++;
      if (result) {
        if (!joinsOnly) this.charsSeen += result.chars;
        // Entirely inside, not merely overlapping. A hit half of which the
        // reader did not select is not a hit in their selection, and
        // highlighting it would paint outside the range they drew.
        //
        // A hit that spans a break is measured against *two* entries: its start
        // against the page it starts on and its end against the page it ends
        // on. Reading both against this page would compare an index on one page
        // with a limit belonging to another, which is the kind of arithmetic
        // that produces a plausible answer and a wrong one.
        const inScope = result.matches.filter((m) => {
          if (joinsOnly && m.endPage === undefined) return false;
          const startsIn = limitOn.get(m.page);
          const endsIn = limitOn.get(m.endPage ?? m.page);
          return (
            startsIn !== undefined &&
            endsIn !== undefined &&
            m.start >= startsIn.from &&
            m.end <= endsIn.to
          );
        });
        if (inScope.length > 0) this.matches.push(...inScope);
      }
      this.onChange();
      return true;
    };

    for (const { page } of plan) {
      if (!(await visit(page))) return;
    }

    // The one break a wrapped walk never looked across. Starting at page 400
    // means 399 is scanned last, so the join between 399 and 400 --- the one
    // immediately behind the reader --- is the only one with no request after
    // it. One more request closes it, and only cross-page hits are taken from
    // the reply: page 400's own hits are already in the list from the first
    // step, and adding them again would double them.
    const first = plan[0];
    const wrapped = scope === null && plan.length > 1 && first !== undefined && first.page > 0;
    if (wrapped && carried !== null) {
      if (!(await visit(first.page, true))) return;
    }

    this.running = false;
    this.elapsedMs = performance.now() - this.startedAt;
    this.onChange();
  }
}
