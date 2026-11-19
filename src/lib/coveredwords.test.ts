/**
 * The words each mark covers, as the marks panel reads them. What
 * `App.svelte` does with them is held as text in `documentopen.test.ts` and
 * `livedocument.test.ts`.
 */

import { describe, expect, it } from "vitest";

import { COVERED_CHARS, CoveredWords } from "./coveredwords";

describe("the words each mark covers", () => {
  it("are kept by mark id and read back, and nothing for a mark with none", () => {
    const covered = new CoveredWords();
    covered.note(7, "quartz and feldspar");
    expect(covered.of(7)).toBe("quartz and feldspar");
    expect(covered.of(8)).toBe("");
    expect(covered.size).toBe(1);
  });

  it("are cut to the bound, so a hundred highlighted pages are not held whole", () => {
    const covered = new CoveredWords();
    covered.note(1, "x".repeat(COVERED_CHARS + 50));
    expect(covered.of(1)).toHaveLength(COVERED_CHARS);
  });

  it("are kept by a tab as a copy the live one does not reach", () => {
    // The live list goes on being written while another tab is in front, and
    // a kept list that followed it would come back with that tab's words.
    const live = new CoveredWords();
    live.note(1, "first");
    const kept = live.copy();
    live.note(2, "second");
    live.note(1, "rewritten");
    expect(kept.of(1)).toBe("first");
    expect(kept.size).toBe(1);
  });

  it("are restored in place, so a view sharing the list sees them", () => {
    // Mark ids start again with each model: what was there before is gone,
    // and a list shared with the document's other view is the same object.
    const live = new CoveredWords();
    live.note(9, "another document's");
    const shared = live;
    const kept = new CoveredWords();
    kept.note(1, "kept");
    live.restore(kept);
    expect(shared.of(1)).toBe("kept");
    expect(shared.of(9)).toBe("");
    // And the kept copy stays the tab's own.
    live.note(2, "later");
    expect(kept.size).toBe(1);
  });

  it("restored from themselves are left as they are", () => {
    const covered = new CoveredWords();
    covered.note(1, "kept");
    covered.restore(covered);
    expect(covered.of(1)).toBe("kept");
  });

  it("compare by what they hold, so a test of a kept tab sees a different list", () => {
    const one = new CoveredWords();
    const two = new CoveredWords();
    one.note(1, "a");
    two.note(1, "b");
    expect(one).not.toEqual(two);
    two.note(1, "a");
    expect(one).toEqual(two);
  });
});
