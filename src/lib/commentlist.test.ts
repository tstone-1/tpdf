import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CommentList } from "./commentlist";
import type { Comment, Comments } from "./comments";
import { installFakeDom, type FakeDom } from "./testdom";

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

function comments(items: Comment[], over: Partial<Comments> = {}): Comments {
  return {
    items,
    limits: {
      crowded_pages: 0,
      over_budget: false,
      bodies_clipped: 0,
      unknown_kinds: 0,
      unreadable: 0,
      cycles: 0,
    },
    scan_ms: 1,
    ...over,
  };
}

describe("CommentList", () => {
  let dom: FakeDom;
  let picked: number[];

  beforeEach(() => {
    dom = installFakeDom();
    picked = [];
  });

  afterEach(() => {
    dom.restore();
  });

  function panel(): CommentList {
    return new CommentList(dom.root as unknown as HTMLElement, {
      onPick: (id) => picked.push(id),
    });
  }

  it("says it is still reading before an answer arrives", () => {
    // Three states saying three different things. Collapsing "not yet" into
    // "none" makes a slow document look like an unannotated one for exactly as
    // long as somebody would be looking at it.
    const list = panel();
    expect(list.rowCount).toBe(0);
    const text = (dom.root.children[1]?.children[0]?.textContent ?? "") as string;
    expect(text).toContain("Reading");
  });

  it("distinguishes a document with none from one that could not be read", () => {
    const list = panel();
    list.setComments(comments([]));
    const none = dom.root.children[1]?.children[0]?.textContent ?? "";
    list.setComments(null);
    const broken = dom.root.children[1]?.children[0]?.textContent ?? "";
    expect(none).toContain("no comments");
    expect(broken).toContain("could not be read");
    expect(none).not.toBe(broken);
  });

  it("draws a row per comment, replies included", () => {
    const list = panel();
    list.setComments(
      comments([comment({ id: 0 }), comment({ id: 1, reply_to: 0 }), comment({ id: 2 })]),
    );
    expect(list.rowCount).toBe(3);
  });

  it("shows what the comment says and who said it", () => {
    const list = panel();
    list.setComments(
      comments([comment({ id: 0, body: "Check this figure.", author: "Timo" })]),
    );
    expect(list.rowText(0)).toEqual({
      body: "Check this figure.",
      byline: "Timo · 2026-08-12 10:15",
    });
  });

  it("names the row a reply answers, and only when that row is there", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0 }), comment({ id: 1, reply_to: 0 })]));
    expect(list.elementFor(1)?.getAttribute("aria-describedby")).toBe("tpdf-comment-0");
    // The control: a reply whose parent is not in the list is drawn as a root,
    // and naming an element that is not there tells a screen reader to read
    // nothing.
    list.setComments(comments([comment({ id: 5, reply_to: 4 })]));
    expect(list.elementFor(5)?.getAttribute("aria-describedby")).toBeNull();
  });

  it("is one tab stop", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0 }), comment({ id: 1 }), comment({ id: 2 })]));
    const tabbable = [0, 1, 2].filter((id) => list.elementFor(id)?.tabIndex === 0);
    expect(tabbable).toEqual([0]);
  });

  it("moves the roving tabindex with the arrow keys", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0 }), comment({ id: 1 })]));
    const listbox = dom.root.children[1];
    listbox?.dispatch("keydown", { key: "ArrowDown" });
    expect(list.focusedId).toBe(1);
    expect(list.elementFor(1)?.tabIndex).toBe(0);
    expect(list.elementFor(0)?.tabIndex).toBe(-1);
  });

  it("activates the row the key landed on, not the one it remembered", () => {
    // The stale-focus-mirror trap the outline paid for: a window without system
    // focus moves `activeElement` without delivering `focusin`, so a handler
    // reading its own mirror aims Enter at a row the reader is not on.
    const list = panel();
    list.setComments(comments([comment({ id: 0 }), comment({ id: 1 })]));
    const listbox = dom.root.children[1];
    listbox?.dispatch("keydown", { key: "Enter", target: list.elementFor(1) });
    expect(picked).toEqual([1]);
  });

  it("reports a press on a row", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 3 })]));
    (list.elementFor(3) as unknown as { dispatch: (t: string, e: object) => void })?.dispatch(
      "pointerdown",
      {},
    );
    expect(picked).toEqual([3]);
  });

  it("marks one row as selected at a time", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0 }), comment({ id: 1 })]));
    list.select(0);
    expect(list.selectedId).toBe(0);
    expect(list.elementFor(0)?.getAttribute("aria-selected")).toBe("true");
    list.select(1);
    expect(list.elementFor(0)?.getAttribute("aria-selected")).toBe("false");
    expect(list.elementFor(1)?.getAttribute("aria-selected")).toBe("true");
    list.select(null);
    expect(list.selectedId).toBe(-1);
    expect(list.elementFor(1)?.getAttribute("aria-selected")).toBe("false");
  });

  it("says when the scan cut something, and says nothing when it did not", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0 })]));
    expect(list.status).toBe("");
    list.setComments(
      comments([comment({ id: 0 })], {
        limits: {
          crowded_pages: 1,
          over_budget: false,
          bodies_clipped: 0,
          unknown_kinds: 0,
          unreadable: 0,
          cycles: 0,
        },
      }),
    );
    expect(list.status).toContain("incomplete");
  });

  it("marks a hidden comment as hidden", () => {
    const list = panel();
    list.setComments(comments([comment({ id: 0, hidden: true })]));
    const row = list.elementFor(0);
    const children = [...(row?.children ?? [])] as HTMLElement[];
    expect(children[children.length - 1]?.textContent).toBe("hidden");
  });
});
