import { describe, expect, it } from "vitest";

import {
  History,
  LINK_SLACK_PT,
  MAX_HISTORY,
  linkAt,
  linkRunsIn,
  noticeFor,
  onPage,
  orderedLinks,
  refusalFor,
  stepLink,
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
    pages_missed: 0,
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

describe("orderedLinks", () => {
  it("orders by page first", () => {
    const items = [
      link({ id: 0, page: 3, rect: [100, 100, 200, 120] }),
      link({ id: 1, page: 1, rect: [100, 700, 200, 720] }),
    ];
    expect(orderedLinks(items).map((item) => item.id)).toEqual([1, 0]);
  });

  it("orders down the page within one page", () => {
    const items = [
      link({ id: 0, rect: [100, 500, 200, 520] }),
      link({ id: 1, rect: [100, 100, 200, 120] }),
    ];
    expect(orderedLinks(items).map((item) => item.id)).toEqual([1, 0]);
  });

  it("orders across the page for two links on one line", () => {
    // The case a top-then-left sort gets wrong when the right-hand link's box
    // starts a point higher, which is ordinary: it would come first.
    const items = [
      link({ id: 0, rect: [300, 99, 400, 119] }),
      link({ id: 1, rect: [100, 100, 200, 120] }),
    ];
    expect(orderedLinks(items).map((item) => item.id)).toEqual([1, 0]);
  });

  it("treats boxes that barely overlap as different lines", () => {
    // Overlap of 2 points against a height of 20 is 10%, under the half the
    // rule wants --- so this is two lines and the higher one comes first.
    const items = [
      link({ id: 0, rect: [300, 118, 400, 138] }),
      link({ id: 1, rect: [100, 100, 200, 120] }),
    ];
    expect(orderedLinks(items).map((item) => item.id)).toEqual([1, 0]);
  });

  it("keeps a footnote marker on the line it sits in", () => {
    // A superscript is shorter than the sentence around it *and sits higher*,
    // and the second half is what makes this discriminate. The marker's top is
    // above the sentence's, so a rule that separates them onto two lines orders
    // the marker first --- while the proportional rule bands them and orders
    // them across the page, sentence first.
    //
    // The first version of this fixture put the marker's top *below* the
    // sentence's, where both rules give the same answer and the mutation
    // `band lines by absolute overlap` survived. Overlap here is 6 points on a
    // 10-point marker: 60% of the shorter box, and under any constant tuned for
    // 20-point body text.
    const marker = link({ id: 0, rect: [300, 96, 306, 106] });
    const sentence = link({ id: 1, rect: [100, 100, 280, 120] });
    expect(orderedLinks([marker, sentence]).map((item) => item.id)).toEqual([1, 0]);
  });

  it("is a total order even for identical rectangles", () => {
    // Pathological and it still has to be a function: "the next one" cannot
    // depend on which of two equal links the sort happened to see first.
    const items = [link({ id: 5 }), link({ id: 2 })];
    expect(orderedLinks(items).map((item) => item.id)).toEqual([2, 5]);
  });

  it("does not modify the array it is given", () => {
    const items = [link({ id: 0, rect: [100, 500, 200, 520] }), link({ id: 1 })];
    orderedLinks(items);
    expect(items.map((item) => item.id)).toEqual([0, 1]);
  });
});

describe("stepLink", () => {
  const page0 = [
    link({ id: 0, rect: [100, 100, 200, 120] }),
    link({ id: 1, rect: [100, 300, 200, 320] }),
    link({ id: 2, rect: [100, 500, 200, 520] }),
  ];
  const ordered = orderedLinks(page0);
  const at = (top: number) => ({ page: 0, top });

  it("walks forward from a focused link", () => {
    expect(stepLink(ordered, page0[0] ?? null, at(0), 1)?.id).toBe(1);
    expect(stepLink(ordered, page0[1] ?? null, at(0), 1)?.id).toBe(2);
  });

  it("walks backward from a focused link", () => {
    expect(stepLink(ordered, page0[2] ?? null, at(0), -1)?.id).toBe(1);
  });

  it("stops at each end rather than wrapping", () => {
    expect(stepLink(ordered, page0[2] ?? null, at(0), 1)).toBeNull();
    expect(stepLink(ordered, page0[0] ?? null, at(0), -1)).toBeNull();
  });

  it("starts from the viewport when nothing is focused", () => {
    // Not from the top of the document: a reader who has scrolled to page 400
    // and presses "next link" means the next one they can see.
    expect(stepLink(ordered, null, at(250), 1)?.id).toBe(1);
    expect(stepLink(ordered, null, at(0), 1)?.id).toBe(0);
  });

  it("goes back to the link before the viewport, not the one level with it", () => {
    // The control that says the two predicates differ. At exactly 300 the
    // middle link is neither ahead nor behind; treating it as behind would make
    // Previous land on the link Next just arrived at.
    expect(stepLink(ordered, null, at(300), -1)?.id).toBe(0);
    expect(stepLink(ordered, null, at(301), -1)?.id).toBe(1);
  });

  it("falls back to the viewport when the focused link is gone", () => {
    const stale = link({ id: 99, rect: [100, 100, 200, 120] });
    expect(stepLink(ordered, stale, at(250), 1)?.id).toBe(1);
  });

  it("answers nothing for a document with no links", () => {
    expect(stepLink([], null, at(0), 1)).toBeNull();
    expect(stepLink([], null, at(0), -1)).toBeNull();
  });

  it("crosses pages in both directions", () => {
    const across = orderedLinks([
      link({ id: 0, page: 0, rect: [100, 700, 200, 720] }),
      link({ id: 1, page: 2, rect: [100, 100, 200, 120] }),
    ]);
    expect(stepLink(across, null, { page: 1, top: 400 }, 1)?.id).toBe(1);
    expect(stepLink(across, null, { page: 1, top: 400 }, -1)?.id).toBe(0);
  });
});

describe("linkRunsIn", () => {
  // Five characters on one line: "GO ON", with boxes 10 points wide.
  const boxes = [
    10, 100, 20, 112, 20, 100, 30, 112, 30, 100, 36, 112, 36, 100, 46, 112, 46,
    100, 56, 112,
  ];
  const whole = [{ from: 0, to: 5 }];
  const over = (rect: [number, number, number, number], id = 0) =>
    link({ id, rect });

  /** The runs as `[link id or null, character count]`, which is what a test reads. */
  const shape = (runs: ReturnType<typeof linkRunsIn>) =>
    runs.map((run) => [
      run.link?.id ?? null,
      run.ranges.reduce((sum, range) => sum + (range.to - range.from), 0),
    ]);

  it("splits a line into the link's characters and the rest", () => {
    const runs = linkRunsIn(whole, boxes, [over([8, 98, 32, 114])]);
    expect(shape(runs)).toEqual([
      [0, 2],
      [null, 3],
    ]);
  });

  it("marks nothing when no link covers the line", () => {
    expect(shape(linkRunsIn(whole, boxes, []))).toEqual([[null, 5]]);
    // A link on the same page but elsewhere on it, which is the control that
    // says the rectangle is consulted rather than merely its presence.
    expect(shape(linkRunsIn(whole, boxes, [over([200, 300, 260, 320])]))).toEqual([
      [null, 5],
    ]);
  });

  it("takes a character by its centre, not by its box overlapping", () => {
    // The rectangle reaches 22, so it covers all of the first character and two
    // points of the second. By overlap the second belongs to the link; by centre
    // it does not --- and annotation rectangles are drawn generously around
    // their text, so overlap makes a link claim the word next door.
    const runs = linkRunsIn(whole, boxes, [over([8, 98, 22, 114])]);
    expect(shape(runs)).toEqual([
      [0, 1],
      [null, 4],
    ]);
  });

  it("keeps two links apart even where they point at the same page", () => {
    // Adjacent runs are merged when they are the *same link*, not when they have
    // the same destination: two cross-references to one chapter are two links,
    // and merging them would announce them as a single one.
    const runs = linkRunsIn(whole, boxes, [
      over([8, 98, 22, 114], 0),
      over([22, 98, 32, 114], 1),
    ]);
    expect(shape(runs)).toEqual([
      [0, 1],
      [1, 1],
      [null, 3],
    ]);
  });

  it("finds a link on a band boundary", () => {
    // The lookup buckets by 12-point bands and a link spans every band it
    // touches. A character whose centre sits in a band the link only partly
    // covers must still find it --- which a single-band index would miss.
    const tall = over([8, 60, 32, 200]);
    expect(shape(linkRunsIn(whole, boxes, [tall]))).toEqual([
      [0, 2],
      [null, 3],
    ]);
  });

  it("handles a range that runs past the boxes it has", () => {
    // A range longer than the page's characters is what a bounded extraction
    // produces, and reading past the array yields `undefined` for each edge.
    //
    // **The link has to reach the origin for this to discriminate**, and the
    // first version of this test did not: coercing the missing edges to 0 puts
    // the phantom character at (0, 0), which an ordinary link does not contain,
    // so the mutation and the fix gave the same answer and it survived. With a
    // rectangle whose corner is the origin, the coercion marks four characters
    // that do not exist as being inside it.
    const runs = linkRunsIn([{ from: 0, to: 9 }], boxes, [over([0, 0, 32, 114])]);
    expect(shape(runs)).toEqual([
      [0, 2],
      [null, 7],
    ]);
  });

  it("ignores a link whose rectangle has no height", () => {
    // A degenerate rectangle contains exactly the points on its own line, so the
    // character this uses is centred *on* it --- boxes 94 to 106, centre 100.
    // Against a character centred anywhere else the guard changes nothing and
    // the test cannot fail.
    //
    // `links.rs` drops a zero-area rectangle at scan time, so nothing from the
    // backend arrives like this; the guard is what makes that a property of this
    // function rather than of its caller.
    const onTheLine = [10, 94, 20, 106];
    expect(
      shape(linkRunsIn([{ from: 0, to: 1 }], onTheLine, [over([8, 100, 32, 100])])),
    ).toEqual([[null, 1]]);
    // The control: the same character and a rectangle with height is a hit, so
    // the assertion above is about the degeneracy rather than about the position.
    expect(
      shape(linkRunsIn([{ from: 0, to: 1 }], onTheLine, [over([8, 98, 32, 114])])),
    ).toEqual([[0, 1]]);
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
    // The one that means "the scan could not look" rather than "it cut
    // something", which is the difference between an incomplete list and a list
    // of nothing presented as complete.
    expect(noticeFor(limits({ pages_missed: 1 }))).toContain(
      "one page could not be read at all",
    );
    expect(noticeFor(limits({ pages_missed: 5 }))).toContain("5 pages");
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
