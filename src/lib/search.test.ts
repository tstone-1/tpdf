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

import { PLAIN_SEARCH, sameOptions, type SearchOptions } from "./search";

describe("sameOptions", () => {
  it("is true only when both options agree", () => {
    const both: SearchOptions = { matchCase: true, wholeWord: true };
    expect(sameOptions(PLAIN_SEARCH, PLAIN_SEARCH)).toBe(true);
    expect(sameOptions(both, { ...both })).toBe(true);
    // One field each way, because a comparison that reads only the first is
    // true for every pair the second one distinguishes.
    expect(sameOptions(PLAIN_SEARCH, { matchCase: true, wholeWord: false })).toBe(false);
    expect(sameOptions(PLAIN_SEARCH, { matchCase: false, wholeWord: true })).toBe(false);
  });

  it("describes the plain search as neither option", () => {
    // The default the backend also defaults to. If these drift, a first search
    // is matched one way and labelled the other.
    expect(PLAIN_SEARCH).toEqual({ matchCase: false, wholeWord: false });
  });
});
