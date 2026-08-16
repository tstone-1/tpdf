import { describe, expect, it } from "vitest";

import {
  History,
  LINK_SLACK_PT,
  MAX_HISTORY,
  linkAt,
  noticeFor,
  onPage,
  refusalFor,
  samePlace,
  turnedFor,
  type Link,
  type LinkLimits,
} from "./links";

/** One link, with the fields a test is not about filled in plausibly. */
function link(over: Partial<Link> & { id: number }): Link {
  return {
    page: 0,
    rect: [100, 100, 200, 120],
    target: { kind: "page", page: 3, top_pt: 200 },
    ...over,
  };
}

function limits(over: Partial<LinkLimits> = {}): LinkLimits {
  return {
    crowded_pages: 0,
    over_budget: false,
    unreadable: 0,
    unresolved_names: 0,
    ...over,
  };
}

describe("linkAt", () => {
  it("finds a link under the point", () => {
    const items = [link({ id: 0 })];
    expect(linkAt(items, 0, 150, 110)?.id).toBe(0);
  });

  it("answers nothing outside it", () => {
    const items = [link({ id: 0 })];
    expect(linkAt(items, 0, 400, 110)).toBeNull();
    expect(linkAt(items, 0, 150, 400)).toBeNull();
  });

  it("does not reach onto another page", () => {
    // The control for the test above: the same coordinates, a different page.
    const items = [link({ id: 0, page: 2 })];
    expect(linkAt(items, 2, 150, 110)?.id).toBe(0);
    expect(linkAt(items, 0, 150, 110)).toBeNull();
  });

  it("takes the smallest of two overlapping links", () => {
    // A producer wrapping a paragraph in one link and a phrase inside it in
    // another: the phrase is what the reader aimed at.
    const paragraph = link({ id: 0, rect: [100, 100, 400, 200] });
    const phrase = link({ id: 1, rect: [150, 110, 200, 125] });
    expect(linkAt([paragraph, phrase], 0, 170, 118)?.id).toBe(1);
    // And outside the phrase, the paragraph still answers.
    expect(linkAt([paragraph, phrase], 0, 350, 180)?.id).toBe(0);
  });

  it("ignores a rectangle with no area", () => {
    const flat = link({ id: 0, rect: [100, 100, 100, 120] });
    expect(linkAt([flat], 0, 100, 110)).toBeNull();
  });

  it("allows a point of slack, and no more", () => {
    const items = [link({ id: 0 })];
    // Just inside the slack, and just outside it. Both sides asserted, or the
    // test passes for a slack of any size at all.
    expect(linkAt(items, 0, 100 - LINK_SLACK_PT + 0.1, 110)?.id).toBe(0);
    expect(linkAt(items, 0, 100 - LINK_SLACK_PT - 0.1, 110)).toBeNull();
  });

  it("keeps neighbouring links apart, which is why the slack is small", () => {
    // Two links two points apart, which is what a wrapped sentence looks like.
    // A comment's three points of slack would make the gap between them belong
    // to both, and the second one listed would win.
    const first = link({ id: 0, rect: [100, 100, 200, 120] });
    const second = link({ id: 1, rect: [202, 100, 300, 120] });
    expect(linkAt([first, second], 0, 199, 110)?.id).toBe(0);
    expect(linkAt([first, second], 0, 203, 110)?.id).toBe(1);
  });
});

describe("onPage and turnedFor", () => {
  it("keeps only the page asked for", () => {
    const items = [link({ id: 0, page: 1 }), link({ id: 1, page: 2 })];
    expect(onPage(items, 2).map((item) => item.id)).toEqual([1]);
  });

  it("returns rectangles unchanged at no rotation", () => {
    const items = [link({ id: 0 })];
    expect(turnedFor(items, 0, 612, 792)[0]?.rect).toEqual([100, 100, 200, 120]);
  });

  it("turns the rectangle and keeps the target", () => {
    const items = [link({ id: 0 })];
    const turned = turnedFor(items, 1, 612, 792)[0];
    expect(turned).toBeDefined();
    expect(turned?.rect).not.toEqual([100, 100, 200, 120]);
    // The destination is not geometry and must survive the turn untouched --- a
    // spread that dropped it would leave every rotated link pointing nowhere.
    expect(turned?.target).toEqual({ kind: "page", page: 3, top_pt: 200 });
  });
});

describe("refusalFor", () => {
  it("says nothing about a link that works", () => {
    expect(refusalFor({ kind: "page", page: 1, top_pt: null })).toBeNull();
  });

  it("uses the outline's words for a refused action", () => {
    const said = refusalFor({ kind: "refused", action: "uri" });
    expect(said).toContain("web link");
    expect(said).toContain("not followed");
  });

  it("names a broken destination as the document's fault", () => {
    expect(refusalFor({ kind: "broken" })).toContain(
      "points at a page this document does not have",
    );
  });

  it("says nothing for a link that names no destination", () => {
    // `reasonFor` answers "no destination", which is true of an outline heading
    // and is not something to tell a reader about a rectangle they clicked.
    expect(refusalFor({ kind: "none" })).toContain("no destination");
  });
});

describe("noticeFor", () => {
  it("says nothing when nothing was cut", () => {
    expect(noticeFor(limits())).toBeNull();
  });

  it("names each bound separately", () => {
    expect(noticeFor(limits({ over_budget: true }))).toContain("too many links");
    expect(noticeFor(limits({ crowded_pages: 3 }))).toContain("3 pages");
    expect(noticeFor(limits({ unreadable: 1 }))).toContain("one annotation");
    expect(noticeFor(limits({ unresolved_names: 2 }))).toContain(
      "2 named destinations",
    );
  });

  it("joins several into one sentence", () => {
    const said = noticeFor(limits({ over_budget: true, unreadable: 4 }));
    expect(said).toContain("too many links");
    expect(said).toContain("4 annotations");
  });
});

describe("History", () => {
  const at = (page: number, top = 0) => ({ page, top });

  it("starts with nowhere to go", () => {
    const history = new History();
    expect(history.canGoBack).toBe(false);
    expect(history.canGoForward).toBe(false);
    expect(history.back(at(5))).toBeNull();
    expect(history.forward(at(5))).toBeNull();
  });

  it("goes back to where the jump started", () => {
    const history = new History();
    history.push(at(2, 100));
    expect(history.canGoBack).toBe(true);
    expect(history.back(at(9))).toEqual(at(2, 100));
    expect(history.canGoBack).toBe(false);
  });

  it("goes forward to where going back left", () => {
    // The distinction that makes Forward useful: it returns to the destination,
    // not to the origin. Getting it round the other way gives two buttons that
    // bounce between one pair of positions.
    const history = new History();
    history.push(at(2));
    history.back(at(9));
    expect(history.canGoForward).toBe(true);
    expect(history.forward(at(2))).toEqual(at(9));
  });

  it("walks a chain of jumps back in order", () => {
    const history = new History();
    history.push(at(1));
    history.push(at(4));
    history.push(at(7));
    expect(history.back(at(9))).toEqual(at(7));
    expect(history.back(at(7))).toEqual(at(4));
    expect(history.back(at(4))).toEqual(at(1));
    expect(history.canGoBack).toBe(false);
  });

  it("drops the forward branch on a new jump", () => {
    const history = new History();
    history.push(at(1));
    history.back(at(5));
    expect(history.canGoForward).toBe(true);
    history.push(at(8));
    expect(history.canGoForward).toBe(false);
  });

  it("does not record a jump that lands where the reader already was", () => {
    // Pressing the same cross-reference twice is one place to come back to, not
    // two --- otherwise Back has to be pressed as many times as the link was.
    const history = new History();
    history.push(at(2, 100));
    history.push(at(2, 110));
    expect(history.depths.back).toBe(1);
    // The control: far enough apart on the same page and it is a real jump.
    history.push(at(2, 600));
    expect(history.depths.back).toBe(2);
  });

  it("drops the oldest entry rather than refusing a new one", () => {
    const history = new History();
    for (let page = 0; page < MAX_HISTORY + 10; page += 1) {
      history.push(at(page * 10));
    }
    expect(history.depths.back).toBe(MAX_HISTORY);
    // The most recent is still there, which is what "drops the oldest" means
    // and what a length check alone cannot tell you.
    expect(history.back(at(0))).toEqual(at((MAX_HISTORY + 9) * 10));
  });

  it("forgets everything for a new document", () => {
    const history = new History();
    history.push(at(3));
    history.back(at(8));
    history.clear();
    expect(history.canGoBack).toBe(false);
    expect(history.canGoForward).toBe(false);
  });
});

describe("samePlace", () => {
  it("is false across pages however close the offsets", () => {
    expect(samePlace({ page: 1, top: 100 }, { page: 2, top: 100 })).toBe(false);
  });

  it("is true within half a page and false beyond it", () => {
    expect(samePlace({ page: 1, top: 100 }, { page: 1, top: 200 })).toBe(true);
    expect(samePlace({ page: 1, top: 100 }, { page: 1, top: 700 })).toBe(false);
  });
});
