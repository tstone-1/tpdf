import { describe, expect, it } from "vitest";

import type { Comment } from "./comments";
import type { Link } from "./links";
import type { OutlineItem } from "./outline";
import {
  commentsIn,
  linksIn,
  NO_PAGES,
  outlineIn,
  PageMap,
  unedited,
  type PageView,
} from "./pages";

/** A working document built by hand, as a state reply would describe it. */
function map(...views: [id: number, source: number, turns?: number][]): PageMap {
  return new PageMap(
    views.map(([id, source, turns]) => ({ id, source, turns: turns ?? 0 })),
  );
}

describe("PageMap", () => {
  it("translates a slot to the page of the file it draws", () => {
    // Page 2 of a four-page document deleted: slot 1 now draws source 2.
    const pages = map([1, 0], [3, 2], [4, 3]);
    expect(pages.length).toBe(3);
    expect(pages.sources()).toEqual([0, 2, 3]);
    expect(pages.sourceOf(1)).toBe(2);
    expect(pages.sourceOf(2)).toBe(3);
  });

  it("translates a page of the file back to the slot showing it", () => {
    const pages = map([1, 0], [3, 2], [4, 3]);
    expect(pages.slotOf(0)).toBe(0);
    expect(pages.slotOf(2)).toBe(1);
    expect(pages.slotOf(3)).toBe(2);
  });

  it("finds the slot a page identity is showing in", () => {
    // The direction a *mark* needs, and the one where a slot cannot stand in
    // for an identity: the model keys a highlight by the page's id, and the
    // overlay draws in slots. Built with the pages out of order, because on a
    // document nobody rearranged every id equals its slot plus one and any
    // answer at all looks right.
    const pages = map([3, 2], [1, 0], [4, 3]);
    expect(pages.slotOfId(3)).toBe(0);
    expect(pages.slotOfId(1)).toBe(1);
    expect(pages.slotOfId(4)).toBe(2);
    // An id that is not on screen -- a deleted page's, or one nobody issued --
    // is nowhere rather than slot 0, which would draw its marks on whatever
    // page happens to be first.
    expect(pages.slotOfId(2)).toBeUndefined();
    expect(pages.slotOfId(99)).toBeUndefined();
  });

  it("says a deleted page is nowhere rather than answering with a slot", () => {
    const pages = map([1, 0], [3, 2], [4, 3]);
    // Source 1 is the page that was deleted. A link pointing at it, a comment on
    // it and an outline entry naming it all have to be told there is nowhere to
    // go --- an answer of 1 would scroll the reader to a different page.
    expect(pages.slotOf(1)).toBeUndefined();
  });

  it("says a slot past the end is nowhere rather than falling back to itself", () => {
    const pages = map([1, 0], [3, 2]);
    expect(pages.sourceOf(2)).toBeUndefined();
    expect(pages.sourceOf(-1)).toBeUndefined();
    // The fallback this refuses is `?? slot`, which is right for every unedited
    // document and asks for the wrong page in exactly the case the class is for.
    expect(pages.turnsOf(9)).toBe(0);
    expect(pages.idOf(9)).toBeUndefined();
  });

  it("carries a page's turns and its identity", () => {
    const pages = map([1, 0], [3, 2, 3]);
    expect(pages.turnsOf(1)).toBe(3);
    expect(pages.idOf(1)).toBe(3);
    expect(pages.at(1)).toEqual({ id: 3, source: 2, turns: 3 });
  });

  describe("sameOrder", () => {
    it("is true for the same pages in the same slots, whatever their turns", () => {
      expect(map([1, 0], [2, 1]).sameOrder(map([1, 0], [2, 1, 2]))).toBe(true);
    });

    it("is false when a page has gone", () => {
      expect(map([1, 0], [2, 1]).sameOrder(map([1, 0]))).toBe(false);
    });

    it("is false when the same sources arrive under different identities", () => {
      // The case a comparison of `source` alone would miss: an undo that
      // restored a page, or a reopen, can put the same file page in the same
      // slot under a different page. Identity is what says whether the thing in
      // the slot is the thing that was there.
      expect(map([1, 0], [2, 1]).sameOrder(map([1, 0], [7, 1]))).toBe(false);
    });
  });

  describe("slotFrom", () => {
    it("follows a page that moved up when an earlier one was deleted", () => {
      const before = map([1, 0], [2, 1], [3, 2]);
      const after = map([1, 0], [3, 2]);
      expect(after.slotFrom(before, 2)).toBe(1);
      expect(after.slotFrom(before, 0)).toBe(0);
    });

    it("says nothing for a page that is no longer there", () => {
      const before = map([1, 0], [2, 1], [3, 2]);
      const after = map([1, 0], [3, 2]);
      expect(after.slotFrom(before, 1)).toBeUndefined();
      expect(after.slotFrom(before, 9)).toBeUndefined();
    });
  });

  describe("linksIn", () => {
    /** A link on `page` pointing at `to`, both pages of the file. */
    function link(id: number, page: number, to: number): Link {
      return {
        id,
        page,
        rect: [0, 0, 10, 10],
        target: { kind: "page", page: to, top_pt: 100 },
      };
    }

    // Page 2 of four deleted.
    const pages = map([1, 0], [3, 2], [4, 3]);

    it("puts a link's rectangle in the slot its page is now in", () => {
      const [moved] = linksIn([link(1, 3, 0)], pages);
      expect(moved?.page).toBe(2);
    });

    it("leaves out a link on a page that is gone", () => {
      // Nowhere to draw it, and a link kept at its old page number would be
      // drawn over whichever page moved into that slot.
      expect(linksIn([link(1, 1, 0)], pages)).toEqual([]);
    });

    it("keeps a link whose destination is gone, and calls it broken", () => {
      const [dead] = linksIn([link(1, 0, 1)], pages);
      expect(dead?.page).toBe(0);
      expect(dead?.target).toEqual({ kind: "broken" });
    });

    it("moves a destination that survived to the slot it is in now", () => {
      const [live] = linksIn([link(1, 0, 3)], pages);
      expect(live?.target).toEqual({ kind: "page", page: 2, top_pt: 100 });
    });

    it("leaves a target that never named a page alone", () => {
      const refused: Link = {
        ...link(1, 0, 0),
        target: { kind: "refused", action: "Launch" },
      };
      expect(linksIn([refused], pages)[0]?.target).toEqual({
        kind: "refused",
        action: "Launch",
      });
    });
  });

  describe("commentsIn", () => {
    function comment(id: number, page: number): Comment {
      return {
        id,
        page,
        kind: "text",
        author: "a",
        body: "b",
        subject: "",
        date: null,
        rect: [0, 0, 10, 10],
        reply_to: null,
        hidden: false,
      };
    }

    it("moves a comment to the slot its page is in and drops the rest", () => {
      const pages = map([1, 0], [3, 2]);
      expect(commentsIn([comment(1, 2), comment(2, 1)], pages)).toEqual([
        { ...comment(1, 2), page: 1 },
      ]);
    });
  });

  describe("outlineIn", () => {
    function entry(title: string, page: number, children: OutlineItem[] = []) {
      return {
        title,
        open: true,
        target: { kind: "page" as const, page, top_pt: null },
        children,
      };
    }

    it("keeps the tree whole and marks the entries whose page has gone", () => {
      const pages = map([1, 0], [3, 2]);
      const mapped = outlineIn(
        [entry("chapter", 1, [entry("section", 2), entry("gone", 1)])],
        pages,
      );

      // The chapter's own page was deleted and its subsections were not. An
      // outline that dropped it would take the whole chapter out of the table of
      // contents because its title page went.
      expect(mapped[0]?.target).toEqual({ kind: "broken" });
      expect(mapped[0]?.children.length).toBe(2);
      expect(mapped[0]?.children[0]?.target).toEqual({
        kind: "page",
        page: 1,
        top_pt: null,
      });
      expect(mapped[0]?.children[1]?.target).toEqual({ kind: "broken" });
    });
  });

  describe("unedited", () => {
    it("numbers ids from one, as the model's baseline does", () => {
      const pages = unedited(3);
      expect(pages.pages).toEqual<PageView[]>([
        { id: 1, source: 0, turns: 0 },
        { id: 2, source: 1, turns: 0 },
        { id: 3, source: 2, turns: 0 },
      ]);
    });

    it("has no pages for a document with none", () => {
      expect(unedited(0).length).toBe(0);
      expect(unedited(-1).length).toBe(0);
      expect(NO_PAGES.length).toBe(0);
      expect(NO_PAGES.sourceOf(0)).toBeUndefined();
    });
  });
});
