/**
 * What the window says about a comparison, and the shapes this side shares with
 * `compare.rs` by spelling.
 *
 * The changes are the backend's and are sent, so there is no second copy of the
 * diff here to hold equal. What is held is what this side adds around it: that
 * no result is shown without saying only the words were compared, that the
 * pages go in slot order with empty pages kept, and where each change is.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/compare.rs?raw";
import {
  ComparisonState,
  gather,
  MAX_ROWS,
  paintedOn,
  placeOf,
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
    ...over,
  };
}

function compared(over: Partial<Comparison> = {}, withoutText = { left: 0, right: 0 }): Compared {
  return { result: result(over), names: { left: "v1.pdf", right: "v2.pdf" }, withoutText };
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

  it("says when the rows were cut, and how many there were", () => {
    const many = Array.from({ length: MAX_ROWS + 1 }, () => change());
    const said = sentences(compared({ changes: many, total_changes: 12_000 }));
    expect(said).toContain(`Showing the first 2,000 of 12,000.`);
    expect(sentences(compared()).some((line) => line.startsWith("Showing"))).toBe(false);
  });

  it("says when passages could not be matched word by word", () => {
    expect(sentences(compared({ unaligned: 1 }))).toContain(
      "1 passage was too different to match word by word, and each is listed as one replacement.",
    );
  });

  it("names a side with pages that have no text", () => {
    expect(sentences(compared({}, { left: 0, right: 2 }))).toContain(
      "v2.pdf has 2 pages with no text, which were not compared. A scanned page has to be recognised first.",
    );
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
  it("sends every slot in order, a page with no text as empty", async () => {
    const texts: (PageText | null)[] = [
      { codes: [97], boxes: [], width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 },
      null,
      { codes: [], boxes: [], width_pt: 1, height_pt: 1, quarter_turns: 0, extract_ms: 0 },
    ];
    const asked: number[] = [];
    const got = await gather(3, (slot) => {
      asked.push(slot);
      return Promise.resolve(texts[slot] ?? null);
    });
    expect(asked).toEqual([0, 1, 2]);
    expect(got.pages).toEqual([
      { page: 0, codes: [97] },
      { page: 1, codes: [] },
      { page: 2, codes: [] },
    ]);
    expect(got.withoutText).toBe(2);
  });
});

describe("ComparisonState", () => {
  const two = (): ComparisonState<number> => {
    const state = new ComparisonState<number>();
    state.start(7, 9, compared({ changes: [change(), change()], total_changes: 2 }));
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

  it("ends when either view is closed, and not for another", () => {
    const state = two();
    expect(state.end(8)).toBeNull();
    expect(state.end(9)).toEqual({ left: 7, right: 9 });
    expect(state.compared).toBeNull();
    expect(state.count).toBe(0);
  });
});
