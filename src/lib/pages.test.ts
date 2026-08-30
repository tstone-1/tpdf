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
  pageId,
  unedited,
  type PageView,
} from "./pages";

/** A working document built by hand, as a state reply would describe it. */
function map(...views: [id: number, source: number, turns?: number][]): PageMap {
  return new PageMap(
    views.map(([id, source, turns]) => ({
      id: pageId(id),
      source: { baseline: source },
      turns: turns ?? 0,
    })),
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
    expect(pages.slotOfId(pageId(3))).toBe(0);
    expect(pages.slotOfId(pageId(1))).toBe(1);
    expect(pages.slotOfId(pageId(4))).toBe(2);
    // An id that is not on screen -- a deleted page's, or one nobody issued --
    // is nowhere rather than slot 0, which would draw its marks on whatever
    // page happens to be first.
    expect(pages.slotOfId(pageId(2))).toBeUndefined();
    expect(pages.slotOfId(pageId(99))).toBeUndefined();
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
    expect(pages.at(1)).toEqual({
      id: 3,
      source: { baseline: 2 },
      turns: 3,
    });
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
    function comment(
      id: number,
      page: number,
      object: [number, number] | null = null,
    ): Comment {
      return {
        id,
        page,
        kind: "text",
        author: "a",
        body: "b",
        subject: "",
        date: null,
        rect: [0, 0, 10, 10],
        quads: [],
        object,
        reply_to: null,
        hidden: false,
      };
    }

    /** One rewrite of `object`, saying `body`, dated `shown`. */
    function edited(
      object: [number, number],
      body: string,
      shown: string | null = "2026-08-29 12:00",
    ) {
      return { object, page: pageId(1), body, made: "D:20260829120000Z", shown };
    }

    it("moves a comment to the slot its page is in and drops the rest", () => {
      const pages = map([1, 0], [3, 2]);
      expect(commentsIn([comment(1, 2), comment(2, 1)], pages)).toEqual([
        { ...comment(1, 2), page: 1 },
      ]);
    });

    it("writes a reader's rewrite over the comment it names, and no other", () => {
      // Two comments with objects, one rewrite. The second is the control and
      // it is what makes this about *matching* rather than about applying: a
      // join that ignored the object would change both, and a join that ignored
      // the edit would change neither.
      const pages = map([1, 0]);
      const items = [comment(1, 0, [12, 0]), comment(2, 0, [34, 0])];
      const [first, second] = commentsIn(items, pages, [
        edited([34, 0], "what I think"),
      ]);
      expect(first?.body).toBe("b");
      expect(first?.date).toBeNull();
      expect(second?.body).toBe("what I think");
      // The date moves with the body, or a reader sees their own words over
      // somebody else's timestamp.
      expect(second?.date).toBe("2026-08-29 12:00");
    });

    it("matches on the object, never on the id or the generation", () => {
      // The whole reason `Comment.object` exists. Every comment here has id 1
      // and sits on the same page, so an id-matching join would rewrite the
      // wrong one -- and the generation is the half a `[0]`-only comparison
      // would miss.
      const pages = map([1, 0]);
      const items = [comment(1, 0, [12, 0]), comment(1, 0, [12, 1])];
      const [zero, one] = commentsIn(items, pages, [
        edited([12, 1], "the later generation"),
      ]);
      expect(zero?.body).toBe("b");
      expect(one?.body).toBe("the later generation");
    });

    it("leaves a comment with no object of its own alone", () => {
      // A direct dictionary inside `/Annots`. It has no name an incremental
      // update could override, so nothing can be addressed to it -- and a join
      // that treated `null` as a wildcard would rewrite every one of them.
      const pages = map([1, 0]);
      const items = [comment(1, 0, null), comment(2, 0, null)];
      const joined = commentsIn(items, pages, [edited([12, 0], "not for you")]);
      expect(joined.map((one) => one.body)).toEqual(["b", "b"]);
    });

    it("is the scan unchanged when nothing has been rewritten", () => {
      // The default. A document opened and not edited must read exactly as the
      // file does, which is the case every other consumer assumes.
      const pages = map([1, 0]);
      expect(commentsIn([comment(1, 0, [12, 0])], pages)).toEqual([
        { ...comment(1, 0, [12, 0]), page: 0 },
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

  describe("a page tpdf made", () => {
    /** The map above, with a made page of `size` at `slot`. */
    function withMade(slot: number, width = 200, height = 400): PageMap {
      const views: PageView[] = [
        { id: pageId(1), source: { baseline: 0 }, turns: 0 },
        { id: pageId(2), source: { baseline: 1 }, turns: 0 },
      ];
      views.splice(slot, 0, {
        id: pageId(9),
        source: { blank: { width, height } },
        turns: 0,
      });
      return new PageMap(views);
    }

    it("has no page of the file behind it", () => {
      const pages = withMade(1);
      expect(pages.sourceOf(1)).toBeUndefined();
      // The control: the pages around it still answer, so this is about the made
      // page rather than about the map having stopped translating.
      expect(pages.sourceOf(0)).toBe(0);
      expect(pages.sourceOf(2)).toBe(1);
    });

    it("carries its own size, which a page of the file does not", () => {
      const pages = withMade(1, 200, 400);
      expect(pages.madeSizeOf(1)).toEqual({ width: 200, height: 400 });
      expect(pages.madeSizeOf(0)).toBeUndefined();
    });

    it("is in no direction of the file-page translation", () => {
      const pages = withMade(1);
      // Both file pages still resolve, and to the slots they are actually in ---
      // which is the half that would break if the made page took a place in the
      // map keyed by an index.
      expect(pages.slotOf(0)).toBe(0);
      expect(pages.slotOf(1)).toBe(2);
    });

    it("answers with the page before it when a file page is what is needed", () => {
      const pages = withMade(1);
      expect(pages.nearestSourceAt(1)).toBe(0);
      // Not a walk that always goes back: a slot showing a file page answers
      // with its own.
      expect(pages.nearestSourceAt(2)).toBe(1);
      // Nothing before it at all, which is a made page at the very front.
      expect(withMade(0).nearestSourceAt(0)).toBeUndefined();
    });

    it("reports itself in a source list rather than being left out of it", () => {
      // A list one entry short would misalign every slot after the made page,
      // which is what a `filter` here would produce.
      expect(withMade(1).sources()).toEqual([0, undefined, 1]);
    });
  });

  describe("unedited", () => {
    it("numbers ids from one, as the model's baseline does", () => {
      const pages = unedited(3);
      expect(pages.pages).toEqual<PageView[]>([
        { id: pageId(1), source: { baseline: 0 }, turns: 0 },
        { id: pageId(2), source: { baseline: 1 }, turns: 0 },
        { id: pageId(3), source: { baseline: 2 }, turns: 0 },
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
