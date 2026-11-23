/**
 * What the window says about a comparison, and the shapes this side shares with
 * `compare.rs` by spelling.
 *
 * The changes are the backend's and are sent, so there is no second copy of the
 * diff here to hold equal. What is held is what this side adds around it: that
 * no result is shown without saying only the words were compared, that the
 * pages go in slot order with empty pages kept, and where each change is.
 */

import { pageId } from "./pages";
import { describe, expect, it } from "vitest";
import { dropAbandoned } from "./documentopen";
import { DocumentTabs } from "./documenttabs";

import backend from "../../src-tauri/src/compare.rs?raw";
import {
  ComparisonState,
  gather,
  MAX_ROWS,
  NOTHING_COMPARED,
  paintedOn,
  SAME_AS_FAR_AS_READ,
  placeOf,
  refusal,
  reopenedViews,
  rowLabel,
  sentences,
  step,
  TEXT_ONLY,
  type Change,
  type Compared,
  type Comparison,
} from "./comparison";
import type { PageText } from "./text";

function change(over: Partial<Change> = {}): Change {
  return {
    kind: "replaced",
    left: [{ page: 0, start: 12, end: 14 }],
    right: [{ page: 0, start: 12, end: 14 }],
    left_at: { page: 0, index: 12 },
    right_at: { page: 0, index: 12 },
    removed: "30",
    added: "45",
    before: "The term is",
    after: "days.",
    ...over,
  };
}

function result(over: Partial<Comparison> = {}): Comparison {
  return {
    left_words: 5,
    right_words: 5,
    removed_words: 1,
    added_words: 1,
    changes: [change()],
    total_changes: 1,
    unaligned: 0,
    left_unread: 0,
    right_unread: 0,
    left_without_words: [],
    right_without_words: [],
    ...over,
  };
}

function compared(over: Partial<Comparison> = {}): Compared {
  return { result: result(over), names: { left: "v1.pdf", right: "v2.pdf" } };
}

/** The `pub` fields of a struct in `compare.rs`, in order. */
function rustFields(name: string): string[] {
  const body = new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`).exec(backend)?.[1] ?? "";
  return [...body.matchAll(/pub (\w+):/g)].map((m) => m[1] ?? "");
}

describe("the shapes are compare.rs's", () => {
  it("names every field the backend sends, and no other", () => {
    expect(rustFields("Change")).toEqual(Object.keys(change()));
    expect(rustFields("Comparison")).toEqual(Object.keys(result()));
    expect(rustFields("Span")).toEqual(["page", "start", "end"]);
    expect(rustFields("Place")).toEqual(["page", "index"]);
    expect(rustFields("PageCodes")).toEqual(["page", "codes"]);
  });

  it("spells the kinds as serde does", () => {
    expect(backend).toContain('#[serde(rename_all = "lowercase")]\npub enum Kind {');
    const variants = [...(/pub enum Kind \{([\s\S]*?)\n\}/.exec(backend)?.[1] ?? "").matchAll(/^\s{4}(\w+),$/gm)]
      .map((m) => (m[1] ?? "").toLowerCase());
    const kinds: Change["kind"][] = ["removed", "added", "replaced"];
    expect(variants).toEqual(kinds);
  });
});

describe("sentences", () => {
  it("says the count, which side is which and, last, that only words were compared", () => {
    const said = sentences(compared({ total_changes: 3, removed_words: 4, added_words: 1 }));
    expect(said[0]).toBe("3 changes: 4 words removed and 1 word added.");
    expect(said[1]).toBe("From v1.pdf, in red, to v2.pdf, in green.");
    expect(said.at(-1)).toBe(TEXT_ONLY);
  });

  it("does not let the same words stand for the same document", () => {
    const said = sentences(compared({ changes: [], total_changes: 0, removed_words: 0, added_words: 0 }));
    expect(said[0]).toBe("The two documents have the same words.");
    expect(said).toContain(TEXT_ONLY);
  });

  it("does not say the same words when a page was not read", () => {
    const none = { changes: [], total_changes: 0, removed_words: 0, added_words: 0 };
    const partly = sentences(compared({ ...none, right_without_words: [1] }));
    expect(partly[0]).toBe(SAME_AS_FAR_AS_READ);
    expect(partly).toContain("v2.pdf has 1 page with no text, which was not compared. A scanned page has to be recognized first.");
    expect(partly).not.toContain("The two documents have the same words.");
    const scans = sentences(compared({ ...none, left_words: 0, right_words: 0, left_without_words: [0, 1, 2], right_without_words: [0, 1, 2] }));
    expect(scans[0]).toBe(NOTHING_COMPARED);
    expect(scans).not.toContain("The two documents have the same words.");
    // Words on neither side is nothing compared, even with no page counted as empty.
    expect(sentences(compared({ ...none, left_words: 0, right_words: 0 }))[0]).toBe(NOTHING_COMPARED);
  });

  it("says which words were not compared because they face a page with no text", () => {
    const none = { changes: [], total_changes: 0, removed_words: 0, added_words: 0 };
    const said = sentences(compared({ ...none, left_unread: 12, right_without_words: [1] }));
    expect(said[0]).toBe(SAME_AS_FAR_AS_READ);
    expect(sentences(compared({ ...none, right_unread: 1 }))[0]).toBe(SAME_AS_FAR_AS_READ);
    expect(said).toContain(
      "12 words of v1.pdf stand where v2.pdf has a page with no text, and were not compared: the change may be on that page.",
    );
    expect(sentences(compared({ ...none, right_unread: 1 }))).toContain(
      "1 word of v2.pdf stands where v1.pdf has a page with no text, and was not compared: the change may be on that page.",
    );
    expect(sentences(compared()).some((line) => line.includes("not compared:"))).toBe(false);
  });

  it("says when the rows were cut, and how many there were", () => {
    const many = Array.from({ length: MAX_ROWS + 1 }, () => change());
    const said = sentences(compared({ changes: many, total_changes: 12_000 }));
    expect(said).toContain(`Showing the first 2,000 of 12,000.`);
    expect(sentences(compared()).some((line) => line.startsWith("Showing"))).toBe(false);
  });

  it("says when passages could not be matched word by word", () => {
    const limited = compared({ unaligned: 1, total_changes: MAX_ROWS + 1 });
    expect(sentences(limited)).toContain(
      "1 passage could not be matched word by word. Each counts as one replacement.",
    );
  });

  it("names a side with pages that have no text", () => {
    expect(sentences(compared({ right_without_words: [1, 2] }))).toContain(
      "v2.pdf has 2 pages with no text, which were not compared. A scanned page has to be recognized first.",
    );
  });
});

describe("refusal", () => {
  const names = { left: "v1.pdf", right: "scan.pdf" };

  it("refuses a side none of whose pages has text", () => {
    const said = refusal(names, result({ right_words: 0 }));
    expect(said).toBe(
      "scan.pdf: none of its pages has text, so there are no words to compare. A scan has to be recognized first.",
    );
    expect(refusal(names, result({ left_words: 0 }))).toMatch(/^v1\.pdf: none of its pages has text/);
  });

  it("compares when every side has text on some page", () => {
    expect(refusal(names, result())).toBeNull();
  });
});

describe("rows and places", () => {
  it("labels a change by its pages and what happened", () => {
    expect(rowLabel(change())).toBe("Page 1, changed");
    expect(rowLabel(change({ kind: "added", right_at: { page: 2, index: 0 } }))).toBe("Page 1 → page 3, added");
  });

  it("places a change at its first span, or where its words would stand", () => {
    const added = change({ kind: "added", left: [], left_at: { page: 1, index: 40 }, right: [{ page: 2, start: 5, end: 9 }] });
    expect(placeOf(added, "left")).toEqual({ page: 1, index: 40 });
    expect(placeOf(added, "right")).toEqual({ page: 2, index: 5 });
  });

  it("gives each painted span the change it belongs to", () => {
    const two = result({
      changes: [change(), change({ left: [{ page: 0, start: 1, end: 2 }, { page: 1, start: 0, end: 3 }] })],
    });
    expect(paintedOn(two, "left")).toEqual([
      { page: 0, start: 12, end: 14, change: 0 },
      { page: 0, start: 1, end: 2, change: 1 },
      { page: 1, start: 0, end: 3, change: 1 },
    ]);
    const moved = result({ changes: [change({ right: [{ page: 2, start: 5, end: 9 }] })] });
    expect(paintedOn(moved, "right")).toEqual([{ page: 2, start: 5, end: 9, change: 0 }]);
  });

  it("steps through the changes and wraps", () => {
    expect(step(-1, 3, 1)).toBe(0);
    expect(step(-1, 3, -1)).toBe(2);
    expect(step(2, 3, 1)).toBe(0);
    expect(step(0, 3, -1)).toBe(2);
    expect(step(0, 0, 1)).toBe(-1);
  });
});

describe("gather", () => {
  it("counts comparable words page by page before requesting the rest", async () => {
    const asked: number[] = [];
    const load = async (slot: number) => {
      asked.push(slot);
      return { codes: [0xad, 97, 0xad, 98, 0x85, 0xad, 0xd800], boxes: [],
        width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 };
    };
    expect(await gather("one.pdf", 1, load, 100, 1)).toHaveLength(1);
    asked.length = 0;
    await expect(gather("two.pdf", 3, load, 100, 1)).rejects.toThrow("word limit");
    expect(asked).toEqual([0, 1]);
  });

  it("counts a discretionary wrap as one word, and an ordinary break as two", async () => {
    const load = (word: string) => async () => ({
      codes: Array.from(word, c => c.codePointAt(0)!), boxes: [], width_pt: 1, height_pt: 1,
      quarter_turns: 0, extract_ms: 0,
    });
    await expect(gather("wrapped", 1, load("discre\u00ad\r\ntionary"), 100, 1)).resolves.toHaveLength(1);
    await expect(gather("separate", 1, load("discre\r\ntionary"), 100, 1)).rejects.toThrow("word limit");
  });

  it("continues a discretionary word on the following page only", async () => {
    const load = (parts: string[]) => async (slot: number) => ({
      codes: Array.from(parts[slot]!, c => c.codePointAt(0)!), boxes: [], width_pt: 1, height_pt: 1,
      quarter_turns: 0, extract_ms: 0,
    });
    await expect(gather("continued", 3, load(["dis\u00ad\r\n", "cre\u00ad", "tionary"]), 100, 1)).resolves.toHaveLength(3);
    await expect(gather("blank", 3, load(["dis\u00ad", "", "tinct"]), 100, 1)).rejects.toThrow("word limit");
  });

  it("stops requesting pages when a side exceeds the character budget", async () => {
    const asked: number[] = [];
    const load = async (slot: number) => {
      asked.push(slot);
      return { codes: [97, 98], boxes: [], width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 };
    };
    expect(await gather("short.pdf", 1, load, 2)).toHaveLength(1);
    asked.length = 0;
    await expect(gather("long.pdf", 3, load, 2)).rejects.toThrow("long.pdf");
    expect(asked).toEqual([0, 1]);
  });

  it("sends every slot in order, a page with no text as empty", async () => {
    const texts: (PageText | null)[] = [
      { codes: [97], boxes: [], width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 },
      null,
      { codes: [], boxes: [], width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 },
    ];
    const asked: number[] = [];
    const got = await gather("draft.pdf", 3, (slot) => {
      asked.push(slot);
      return Promise.resolve(texts[slot] ?? null);
    });
    expect(asked).toEqual([0, 1, 2]);
    expect(got).toEqual([
      { page: 0, codes: [97] },
      { page: 1, codes: [] },
      { page: 2, codes: [] },
    ]);
  });
});

describe("ComparisonState", () => {
  it("keeps compared page identities until their own side is reloaded", () => {
    const state = new ComparisonState<string>();
    state.start("a", "b", compared(), { left: [10, 20].map(pageId), right: [30, 40].map(pageId) });
    state.invalidate("a");
    expect([5, 10, 20].indexOf(state.pageAt("left", 1)!)).toBe(2);
    expect(state.pageAt("right", 1)).toBe(40);
    state.reopened([["a", "replacement"]]);
    expect(state.pageAt("left", 1)).toBeUndefined();
    expect(state.pageAt("right", 1)).toBe(40);
  });

  const two = (): ComparisonState<number> => {
    const state = new ComparisonState<number>();
    state.start(7, 9, compared({ changes: [change(), change()], total_changes: 2 }), { left: [pageId(1)], right: [pageId(1)] });
    return state;
  };

  it("knows its two views and paints each as its side", () => {
    const state = two();
    expect(state.sideOf(7)).toBe("left");
    expect(state.sideOf(9)).toBe("right");
    expect(state.sideOf(8)).toBeNull();
    expect(state.paintFor(7)?.side).toBe("removed");
    expect(state.paintFor(9)?.side).toBe("added");
    expect(state.paintFor(8)).toBeNull();
  });

  it("steps and picks within its changes", () => {
    const state = two();
    expect(state.step(1)).toBe(0);
    expect(state.step(1)).toBe(1);
    expect(state.step(1)).toBe(0);
    expect(state.pick(2)).toBe(false);
    expect(state.pick(1)).toBe(true);
    expect(state.paintFor(9)?.current).toBe(1);
  });

  it("goes out of date on an edit to either side, once, and paints nothing then", () => {
    const state = two();
    expect(state.invalidate(8)).toBe(false);
    expect(state.invalidate(9)).toBe(true);
    expect(state.invalidate(7)).toBe(false);
    expect(state.stale).toBe(true);
    expect(state.paintFor(7)).toBeNull();
    // The rows are kept: the reader can still go to a change.
    expect(state.count).toBe(2);
  });

  it("counts and picks only the rows it draws", () => {
    const state = new ComparisonState<number>();
    const many = Array.from({ length: MAX_ROWS + 1 }, () => change());
    state.start(7, 9, compared({ changes: many, total_changes: MAX_ROWS + 1 }), { left: [pageId(1)], right: [pageId(1)] });
    expect(state.count).toBe(MAX_ROWS);
    expect(state.pick(MAX_ROWS)).toBe(false);
    expect(state.step(-1)).toBe(MAX_ROWS - 1);
  });

  it("clears the surviving view when its partner is abandoned during reopen", () => {
    const state = two();
    const tabs = new DocumentTabs();
    tabs.keep({ view: 7, path: "old.pdf" });
    tabs.keep({ view: 9, path: "new.pdf" });
    state.step(1);
    expect(state.paintFor(7)?.current).toBe(0);
    dropAbandoned({ closed() {} }, tabs, { own: undefined, replaced: true, twins: [{ view: 9 }] },
      () => {}, (id) => state.end(id));
    expect(tabs.all.map((tab) => tab.view)).toEqual([7]);
    expect(state.compared).toBeNull();
    expect(state.paintFor(7)).toBeNull();
    expect(state.step(1)).toBe(-1);
  });

  it("ends when either view is closed, and not for another", () => {
    const state = two();
    expect(state.end(8)).toBeNull();
    expect(state.end(9)).toEqual({ left: 7, right: 9 });
    expect(state.compared).toBeNull();
    expect(state.count).toBe(0);
  });

  it("follows a document read again to its new view, and goes out of date", () => {
    // A reload of the left gives its tab view 17. Before, the comparison
    // still named 7: the right went on painting a current comparison and the
    // reloaded left was no side of it. Mutation: drop `held.stale = true` or
    // the move of the left -> red.
    const state = two();
    expect(state.reopened([[7, 17]])).toBe(true);
    expect(state.sideOf(17)).toBe("left");
    expect(state.sideOf(7)).toBeNull();
    expect(state.stale).toBe(true);
    expect(state.paintFor(9)).toBeNull();
    expect(state.count).toBe(2);
    // The other document's own view keeps its side.
    expect(state.sideOf(9)).toBe("right");
  });

  it("goes out of date when a second view of a document read again is a side", () => {
    // The tab the reload ran in gets a new view; its other views keep theirs,
    // and are named to themselves. Mutation: compare only the first pair -> red.
    const state = two();
    expect(state.reopened([[3, 30], [9, 9]])).toBe(true);
    expect(state.views).toEqual({ left: 7, right: 9 });
    expect(state.stale).toBe(true);
  });

  it("is left alone by a document it is not about", () => {
    // Control for both tests above.
    const state = two();
    expect(state.reopened([[8, 18]])).toBe(false);
    expect(state.stale).toBe(false);
    expect(state.views).toEqual({ left: 7, right: 9 });
    expect(new ComparisonState<number>().reopened([[7, 17]])).toBe(false);
  });

  it("names the replaced view to the new one and each other view to itself", () => {
    expect(reopenedViews(7, 17, [{ view: 3 }, { view: 4 }])).toEqual([[7, 17], [3, 3], [4, 4]]);
  });
});
