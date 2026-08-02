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

/**
 * One page's verdict on whether its text means anything.
 *
 * Mirrors `encoding::PageMapping`. `guessing > 0` means a font on that page
 * declares no character mapping, so PDFium reads glyph ids as character codes
 * and returns text of the right length that means nothing --- see `encoding.rs`.
 */
interface PageMapping {
  composite: number;
  guessing: number;
  truncated: boolean;
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

  /**
   * Per-page character-mapping verdicts, or null until the backend is asked.
   *
   * Asked for at most once per document. **Not once per search**, and an earlier
   * version of this comment claimed a reader who finds hits never pays for it,
   * which stopped being true the moment the accessibility layer became the
   * second consumer: `Viewer.syncAccessibleText` calls `ensureMapping` every
   * frame, so in practice the fetch happens on the first frame after open, for
   * any document that renders text.
   *
   * That is affordable because it was measured rather than assumed --- 0.1 ms to
   * 11.9 ms across the fixtures, tracking object count rather than file size ---
   * and because it is off the startup path, which is where the ~25 ms of margin
   * against the 300 ms target lives.
   */
  private mapping: PageMapping[] | null = null;

  /** Whether the backend has already been asked, so it is asked at most once. */
  private mappingAsked = false;

  /** Whether the backend has *answered*, either way. */
  private mappingSettled = false;

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

  /**
   * How many pages store text no search can read.
   *
   * Zero until a completed scan has found nothing and the backend has answered,
   * so a reader who finds hits is never told about it. Zero also when the
   * question could not be settled --- an unreadable page and an *unknown* page
   * are different, and only the first is worth interrupting a reader over. The
   * unknown case is deliberately silent rather than cautious: a warning on every
   * encrypted document, which `lopdf` cannot paginate, would be a false alarm
   * far more often than a true one.
   */
  get unsearchablePages(): number {
    if (this.mapping === null) return 0;
    return this.mapping.filter((page) => page.guessing > 0).length;
  }

  /**
   * Whether this page's text is PDFium's guess rather than the document's.
   *
   * False until the mapping has been fetched, and false for a page nobody could
   * judge --- unknown is not unreadable. Used by the accessibility layer, which
   * would otherwise read the guess aloud as though it were the page.
   */
  unreadablePage(page: number): boolean {
    const entry = this.mapping?.[page];
    return entry !== undefined && entry.guessing > 0;
  }

  /**
   * Whether the backend has answered, either way.
   *
   * For the check harness, and it is what makes that check able to fail. Almost
   * every document has no unreadable page, so the assertion there is a negative
   * one --- "the reader is told nothing" --- and a negative assertion made before
   * the answer has arrived is satisfied by the answer never arriving. Waiting on
   * `unsearchablePages` instead cannot serve: on those documents the value it
   * would wait for is the value it starts at.
   *
   * True after a failed fetch as well as a successful one. The distinction this
   * draws is asked-and-answered against still-in-flight; what the answer *was*
   * is `unsearchablePages`, which reports zero for "clean" and for "nobody
   * knows" alike, deliberately.
   */
  get mappingKnown(): boolean {
    return this.mappingSettled;
  }

  /**
   * Fetches the mapping if it has not been fetched, and repaints if it changed.
   *
   * Public because there are two consumers and only one of them is a search.
   * The accessibility layer needs it for every document it renders text for, not
   * only for a search that found nothing --- a screen-reader user who never
   * searches would otherwise be read the guess aloud, which is the worst of the
   * three symptoms and the one whose reader can least easily tell.
   *
   * Safe to call every frame: the fetch happens at most once per document.
   */
  ensureMapping(): void {
    void this.learnMapping(this.generation);
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
    // The mapping is a property of the *document*, not of the query, so it
    // deliberately survives a clear: asking again would repeat a parse whose
    // answer cannot have changed.
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

    // The scan is over and found nothing, which is the one moment "No matches."
    // can be a lie: a page whose fonts state no character mapping was never
    // searchable, and it looks exactly like a page that simply does not contain
    // the query.
    //
    // **This call is redundant today and is kept deliberately.** The frame loop
    // calls `ensureMapping` for the accessibility layer, so by the time any scan
    // finishes the answer is already here -- a mutation deleting the condition
    // below survives every test, and that is the correct result rather than a
    // gap. It stays because the alternative is for this module's correctness to
    // depend silently on another module's frame-loop policy: if the
    // accessibility layer ever becomes conditional, search would stop learning
    // the fact and nothing here would fail. `AGENTS.md` records that exact
    // shape -- an impossibility enforced in a different module is not a guard to
    // delete.
    if (this.matches.length === 0) await this.learnMapping(generation);
  }

  /**
   * Asks the backend which pages store unreadable text, at most once.
   *
   * `generation` guards the repaint, not the fetch: a newer scan may have
   * started while this was in flight, and repainting then would show the older
   * scan's conclusion over the newer scan's results. The *answer* is still worth
   * keeping, because it describes the document rather than the query.
   */
  private async learnMapping(generation: number): Promise<void> {
    if (this.mappingAsked) return;
    this.mappingAsked = true;
    try {
      this.mapping = await invoke<PageMapping[]>("document_mapping", {
        doc: this.doc,
      });
    } catch {
      // Unknown, not clean. `unsearchablePages` reports 0 either way, and the
      // reader is told nothing rather than told something false.
      this.mapping = null;
    }
    this.mappingSettled = true;
    if (generation === this.generation) this.onChange();
  }
}
