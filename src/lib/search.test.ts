/**
 * The parts of `search.ts` that are not the walk.
 *
 * The walk itself needs `invoke`, and what it does --- one page at a time, in
 * order, abandoning a superseded scan --- is asserted against a running app by
 * `viewercheck.ts`, where a fake backend would only prove the fake. What is
 * testable here is the option comparison, which decides whether a toggle
 * rescans.
 */

import { describe, expect, it } from "vitest";

import {
  MAX_MATCHES_TO_MARK,
  PLAIN_SEARCH,
  RUN_PAGES,
  runAnswers,
  runFrom,
  sameOptions,
  tooManyMatchesToMark,
  type PageMatches,
  type ScopeRange,
  type SearchOptions,
} from "./search";
import type { FilePage } from "./pages";

describe("sameOptions", () => {
  it("is true only when both options agree", () => {
    const both: SearchOptions = { matchCase: true, wholeWord: true, regex: true };
    expect(sameOptions(PLAIN_SEARCH, PLAIN_SEARCH)).toBe(true);
    expect(sameOptions(both, { ...both })).toBe(true);
    // One field each way, because a comparison that reads only the first is
    // true for every pair the others distinguish.
    expect(sameOptions(PLAIN_SEARCH, { ...PLAIN_SEARCH, matchCase: true })).toBe(false);
    expect(sameOptions(PLAIN_SEARCH, { ...PLAIN_SEARCH, wholeWord: true })).toBe(false);
    expect(sameOptions(PLAIN_SEARCH, { ...PLAIN_SEARCH, regex: true })).toBe(false);
  });

  it("describes the plain search as neither option", () => {
    // The default the backend also defaults to. If these drift, a first search
    // is matched one way and labelled the other.
    expect(PLAIN_SEARCH).toEqual({ matchCase: false, wholeWord: false, regex: false });
  });
});

describe("tooManyMatchesToMark", () => {
  /**
   * Both sides of the bound, so the check fails in both directions.
   *
   * A bound tested only from above passes for an implementation that refuses
   * everything, which is the same command missing.
   */
  it("permits the bound itself and refuses one more", () => {
    expect(tooManyMatchesToMark(MAX_MATCHES_TO_MARK)).toBeNull();
    expect(tooManyMatchesToMark(MAX_MATCHES_TO_MARK + 1)).not.toBeNull();
  });

  /**
   * The bound's **value**, in absolute numbers.
   *
   * Every other check here is written against `MAX_MATCHES_TO_MARK` itself, so
   * its expectation moves with the constant and none of them can see the
   * constant move --- the trap of a check that measures along the axis it is
   * policing. This is the one that pins the number, and both figures in it are
   * measurements rather than taste: 123 is the most a six-digit-or-longer
   * number matched in any of 41 real PDFs, and 722 is the *median* count for
   * the single letter `e` across the same corpus.
   */
  it("sits above every realistic pattern and far below the pathological one", () => {
    expect(MAX_MATCHES_TO_MARK).toBeGreaterThan(123);
    expect(MAX_MATCHES_TO_MARK).toBeLessThan(722);
  });

  /** The realistic sizes this bound was measured against are nowhere near it. */
  it("permits the counts a real redaction pattern produces", () => {
    // An email address matched a median of 2 times across 41 real PDFs and at
    // most 31; a six-digit-or-longer number, at most 123.
    for (const count of [0, 1, 2, 3, 31, 123]) {
      expect(tooManyMatchesToMark(count)).toBeNull();
    }
  });

  /**
   * The refusal says the number, and says what to do.
   *
   * A refusal a reader cannot act on is a dead end --- and the number is what
   * they can check against the results panel in front of them, which is why it
   * is the count of matches rather than of the regions they would become.
   */
  it("names the count and asks for a narrower search", () => {
    const said = tooManyMatchesToMark(85337) ?? "";
    expect(said).toContain("85337");
    expect(said).toContain(String(MAX_MATCHES_TO_MARK));
    expect(said.toLowerCase()).toContain("narrow");
  });
});

describe("runFrom", () => {
  /** A plan over slots `from`..`from + count - 1`, each scoped to the whole page. */
  const plan = (from: number, count: number): ScopeRange[] =>
    Array.from({ length: count }, (_unused, step) => ({
      page: from + step,
      from: 0,
      to: Infinity,
    }));

  /** The identity mapping: an unedited document, where a slot is its own page. */
  const same = (slot: number) => slot as unknown as FilePage;

  it("takes a whole run of consecutive pages, bounded by RUN_PAGES", () => {
    // Longer than the bound, so the bound is what stops it rather than the plan
    // running out --- which is the same answer for the wrong reason.
    const run = runFrom(plan(0, RUN_PAGES + 5), 0, same);
    expect(run.length).toBe(RUN_PAGES);
    expect(run[0]).toBe(0);
    expect(run[RUN_PAGES - 1]).toBe(RUN_PAGES - 1);
  });

  it("starts where it is asked to and not at the beginning", () => {
    expect(runFrom(plan(0, 40), 30, same).slice(0, 3)).toEqual([30, 31, 32]);
  });

  it("stops at a gap in the slots", () => {
    // The wrap an unscoped scan makes: page 774 is followed by page 0, and
    // stitching those two would report a phrase spanning the end of the
    // document.
    //
    // **The file pages are deliberately consecutive across the wrap**, which is
    // what makes this a test of the slot guard rather than of the file-page one
    // beside it. With the identity mapping both guards fire on the same entry,
    // so deleting either leaves the other to catch it and neither can be shown
    // to do anything --- two mechanisms producing one outcome, which
    // `docs/TRAPS.md` records as making both unfalsifiable. Measured: with
    // `same` here, removing the slot guard survived.
    const wrapped: ScopeRange[] = [
      { page: 8, from: 0, to: Infinity },
      { page: 9, from: 0, to: Infinity },
      { page: 0, from: 0, to: Infinity },
      { page: 1, from: 0, to: Infinity },
    ];
    const consecutive = new Map([
      [8, 0],
      [9, 1],
      [0, 2],
      [1, 3],
    ]);
    const of = (slot: number) => consecutive.get(slot) as unknown as FilePage;
    expect(runFrom(wrapped, 0, of)).toEqual([8, 9]);
  });

  /**
   * The second vocabulary, and the one an unedited document cannot show.
   *
   * Slots and file pages are the same sequence until a page is deleted or
   * moved. The backend chains the carry between pages *it* sees as neighbours,
   * so a run that was contiguous in slots alone would ask it to stitch two
   * pages of the file that do not touch --- and the phrase it then reported
   * would span a break that is not in the document.
   */
  it("stops where the file pages are not consecutive either", () => {
    // Slots 0,1,2 draw file pages 4,5,9: page 6 was deleted.
    const sources = [4, 5, 9];
    const of = (slot: number) => sources[slot] as unknown as FilePage;
    expect(runFrom(plan(0, 3), 0, of)).toEqual([0, 1]);
  });

  it("stops at a slot with no page behind it", () => {
    const of = (slot: number) =>
      (slot === 2 ? undefined : slot) as unknown as FilePage | undefined;
    expect(runFrom(plan(0, 5), 0, of)).toEqual([0, 1]);
  });

  /**
   * The same guard where nothing else can stand in for it.
   *
   * In the case above the file-page guard fires on the very next entry, so
   * deleting this one leaves that one to catch it --- measured, and it survived.
   * At the *first* entry there is no previous file page to compare against, so
   * this guard is the only thing between a missing slot and a request asking the
   * backend about a page number that means something else now.
   */
  it("stops at once when the first slot has no page behind it", () => {
    const of = () => undefined as unknown as FilePage | undefined;
    expect(runFrom(plan(0, 5), 0, of)).toEqual([]);
  });

  it("answers a single entry with a run of one, which is the per-page path", () => {
    expect(runFrom(plan(3, 1), 0, same)).toEqual([3]);
    expect(runFrom([], 0, same)).toEqual([]);
  });
});

describe("runAnswers", () => {
  /** One page's answer, with `count` hits nobody looks at. */
  const answer = (page: number, count = 0): PageMatches => ({
    page,
    matches: Array.from({ length: count }, () => ({
      page,
      start: 0,
      end: 1,
      before: "",
      hit: "x",
      after: "",
    })),
    chars: 100,
  });

  it("pairs a full run with the slots it was asked about", () => {
    const reply = { ...answer(10), more: [answer(11), answer(12)] };
    expect(runAnswers([4, 5, 6], reply).map((a) => [a.slot, a.answer.page])).toEqual([
      [4, 10],
      [5, 11],
      [6, 12],
    ]);
  });

  /**
   * The bounded reply, and the failure it prevents.
   *
   * The backend stops a run when its answers reach the byte budget, so asking
   * about sixteen pages can come back with four. A walk that filed one answer
   * per slot regardless would put page 3's hits on slot 6 and count the twelve
   * pages in between as searched --- both plausible, and both wrong.
   */
  it("files only the pages that came back", () => {
    const reply = { ...answer(10), more: [answer(11)] };
    const paired = runAnswers([4, 5, 6, 7], reply);
    expect(paired.length).toBe(2);
    expect(paired.map((a) => a.slot)).toEqual([4, 5]);
  });

  /** A reply with no `more` at all is the single-page shape, and is one pair. */
  it("treats a reply with no run in it as one page", () => {
    expect(runAnswers([4, 5, 6], answer(10)).map((a) => a.slot)).toEqual([4]);
  });

  /**
   * The other direction: more answers than slots is a backend defect, and the
   * extra ones have no slot to belong to. Dropped rather than filed against
   * `undefined`, which would report a hit on a page the reader does not have.
   */
  it("drops answers it has no slot for", () => {
    const reply = { ...answer(10), more: [answer(11), answer(12)] };
    expect(runAnswers([4], reply).map((a) => a.slot)).toEqual([4]);
  });

  it("answers nothing for a run of no slots", () => {
    expect(runAnswers([], answer(10))).toEqual([]);
  });
});
