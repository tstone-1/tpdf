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
  sameOptions,
  tooManyMatchesToMark,
  type SearchOptions,
} from "./search";

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
