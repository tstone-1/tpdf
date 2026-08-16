import { describe, expect, it } from "vitest";

import {
  bylineOf,
  hitTest,
  labelFor,
  noticeFor,
  onPage,
  rowsOf,
  summaryOf,
  turnedFor,
  viewRect,
  type Comment,
  type CommentLimits,
} from "./comments";

/** One comment, with the fields a test is not about filled in plausibly. */
function comment(over: Partial<Comment> & { id: number }): Comment {
  return {
    page: 0,
    kind: "text",
    author: "Timo",
    body: "A note.",
    subject: "",
    date: "2026-08-12 10:15",
    rect: [100, 100, 124, 124],
    reply_to: null,
    hidden: false,
    ...over,
  };
}

const NOTHING_CUT: CommentLimits = {
  crowded_pages: 0,
  over_budget: false,
  bodies_clipped: 0,
  unknown_kinds: 0,
  unreadable: 0,
  cycles: 0,
  pages_missed: 0,
};

describe("rowsOf", () => {
  it("puts a reply under the comment it answers", () => {
    const rows = rowsOf([
      comment({ id: 0 }),
      comment({ id: 1 }),
      comment({ id: 2, reply_to: 0 }),
    ]);
    expect(rows.map((row) => [row.comment.id, row.depth])).toEqual([
      [0, 0],
      [2, 1],
      [1, 0],
    ]);
  });

  it("indents a reply to a reply once, not twice", () => {
    // A panel 260 pixels wide has room for one indent, and a reply to a reply
    // is still an answer in the same conversation.
    const rows = rowsOf([
      comment({ id: 0 }),
      comment({ id: 1, reply_to: 0 }),
      comment({ id: 2, reply_to: 1 }),
    ]);
    expect(rows.map((row) => row.depth)).toEqual([0, 1, 1]);
    expect(rows.map((row) => row.comment.id)).toEqual([0, 1, 2]);
  });

  it("shows a reply whose parent is missing rather than dropping it", () => {
    // The parent was cut by a bound, or is on a page that was not read. A
    // comment nobody can see is a comment lost.
    const rows = rowsOf([comment({ id: 7, reply_to: 4 })]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.depth).toBe(0);
  });

  it("lists every comment exactly once", () => {
    // The property the panel's row count is asserted against in the window
    // harness: a thread walk that visited a comment twice would pass a
    // "everything is listed" check and show duplicates.
    const items = [
      comment({ id: 0 }),
      comment({ id: 1, reply_to: 0 }),
      comment({ id: 2, reply_to: 0 }),
      comment({ id: 3 }),
    ];
    const ids = rowsOf(items).map((row) => row.comment.id);
    expect([...ids].sort()).toEqual([0, 1, 2, 3]);
  });
});

describe("hitTest", () => {
  const note = comment({ id: 1, rect: [100, 100, 124, 124] });
  const big = comment({ id: 2, rect: [50, 50, 300, 300] });

  it("finds a comment under the point", () => {
    expect(hitTest([note], 0, 112, 112)?.id).toBe(1);
  });

  it("finds nothing beside it", () => {
    // The control. Without it, a hit test that answered with the first comment
    // whatever it was handed would pass the test above.
    expect(hitTest([note], 0, 400, 400)).toBeNull();
  });

  it("allows a few points of slack around the edge", () => {
    expect(hitTest([note], 0, 98, 112)?.id).toBe(1);
    expect(hitTest([note], 0, 90, 112)).toBeNull();
  });

  it("prefers the smaller of two marks the point is inside", () => {
    // A note icon dropped inside a square annotation is inside both, and the
    // one a reader is pointing at is the small one.
    expect(hitTest([note, big], 0, 112, 112)?.id).toBe(1);
    expect(hitTest([big, note], 0, 112, 112)?.id).toBe(1);
    // And the big one is still reachable where the small one is not.
    expect(hitTest([note, big], 0, 280, 280)?.id).toBe(2);
  });

  it("ignores a comment on another page", () => {
    expect(hitTest([comment({ id: 3, page: 4 })], 0, 112, 112)).toBeNull();
  });

  it("ignores a hidden comment", () => {
    // `/F` bit 2 means the page does not show it, so there is no mark under the
    // pointer to have been pressed. It is still listed in the panel.
    expect(hitTest([comment({ id: 5, hidden: true })], 0, 112, 112)).toBeNull();
  });

  it("ignores a rectangle with no area", () => {
    // What `annots.rs` reports for a `/Rect` it could not read. Treating it as
    // a hit would put an invisible target in the page's top-left corner.
    expect(hitTest([comment({ id: 6, rect: [0, 0, 0, 0] })], 0, 0, 0)).toBeNull();
  });
});

describe("viewRect and turnedFor", () => {
  it("turns a rectangle with the view", () => {
    // A quarter turn clockwise on a 600 x 800 page sends the display's y to its
    // x. The numbers are asymmetric on purpose: a square rectangle maps to
    // itself and cannot tell a turn from an identity.
    const quad = viewRect([10, 20, 30, 60], 1, 600, 800);
    expect(quad).toEqual({ left: 800 - 60, top: 10, right: 800 - 20, bottom: 30 });
  });

  it("leaves a rectangle alone when the view is not turned", () => {
    expect(viewRect([10, 20, 30, 60], 0, 600, 800)).toEqual({
      left: 10,
      top: 20,
      right: 30,
      bottom: 60,
    });
  });

  it("turns every comment and keeps the rest of each one", () => {
    const turned = turnedFor([comment({ id: 1, rect: [10, 20, 30, 60] })], 1, 600, 800);
    expect(turned[0]?.rect).toEqual([740, 10, 780, 30]);
    expect(turned[0]?.body).toBe("A note.");
  });

  it("copies the list rather than turning it in place", () => {
    // The viewer calls this on every pointer press, over the array it holds
    // permanently. Mutating that array would compose the turn with itself once
    // per press, and the marks would walk off the page.
    const items = [comment({ id: 1, rect: [10, 20, 30, 60] })];
    turnedFor(items, 1, 600, 800);
    expect(items[0]?.rect).toEqual([10, 20, 30, 60]);
  });
});

describe("onPage", () => {
  it("keeps only the page asked for, in order", () => {
    const items = [
      comment({ id: 0, page: 0 }),
      comment({ id: 1, page: 2 }),
      comment({ id: 2, page: 2 }),
    ];
    expect(onPage(items, 2).map((item) => item.id)).toEqual([1, 2]);
    expect(onPage(items, 9)).toEqual([]);
  });
});

describe("noticeFor", () => {
  it("says nothing when nothing was cut", () => {
    expect(noticeFor(NOTHING_CUT)).toBeNull();
  });

  it("names each bound that fired", () => {
    const text = noticeFor({ ...NOTHING_CUT, crowded_pages: 2, cycles: 1 });
    expect(text).toContain("2 pages");
    expect(text).toContain("circle");
  });

  it("distinguishes a kind it does not read from an entry it could not read", () => {
    // Different diagnoses: one says there are marks tpdf does not understand,
    // the other that there are entries nothing could read. A reader chasing a
    // missing comment needs to know which.
    expect(noticeFor({ ...NOTHING_CUT, unknown_kinds: 1 })).toContain("kind");
    expect(noticeFor({ ...NOTHING_CUT, unreadable: 1 })).toContain("could not be read");
  });

  it("reads as a sentence with one, two or three parts", () => {
    expect(noticeFor({ ...NOTHING_CUT, cycles: 1 })).not.toContain(" and ");
    expect(noticeFor({ ...NOTHING_CUT, cycles: 1, unreadable: 1 })).toContain(" and ");
    const three = noticeFor({
      ...NOTHING_CUT,
      cycles: 1,
      unreadable: 1,
      bodies_clipped: 1,
    });
    expect(three).toContain(", ");
    expect(three).toContain(" and ");
  });
});

describe("summaryOf and bylineOf", () => {
  it("flattens a body to one line", () => {
    expect(summaryOf(comment({ id: 1, body: "Two things:\n\nfirst, and second." }))).toBe(
      "Two things: first, and second.",
    );
  });

  it("says what kind of mark it is when there is no body", () => {
    // A row showing nothing looks broken; "somebody highlighted this" is what
    // actually happened.
    expect(summaryOf(comment({ id: 1, kind: "highlight", body: "  " }))).toBe(
      "Highlight, no comment",
    );
  });

  it("names an author with no name", () => {
    expect(bylineOf(comment({ id: 1, author: "", date: null }))).toBe("Unknown");
  });

  it("puts the date after the author when there is one", () => {
    expect(bylineOf(comment({ id: 1, author: "Timo", date: "2026-08-12 10:15" }))).toBe(
      "Timo · 2026-08-12 10:15",
    );
  });
});

describe("labelFor", () => {
  it("gives every kind a word of ours", () => {
    // The kinds the backend can send, from `annots.rs`'s `Kind`. A label built
    // from the document's own `/Subtype` string is what this exists to avoid.
    const kinds = [
      "text",
      "freetext",
      "highlight",
      "underline",
      "squiggly",
      "strikeout",
      "square",
      "circle",
      "line",
      "polygon",
      "polyline",
      "ink",
      "stamp",
      "caret",
      "fileattachment",
      "sound",
      "redact",
    ] as const;
    for (const kind of kinds) {
      expect(labelFor(kind)).toMatch(/^[A-Z]/);
    }
    expect(new Set(kinds.map(labelFor)).size).toBe(kinds.length);
  });
});
