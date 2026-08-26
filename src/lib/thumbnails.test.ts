import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installFakeDom, settle, type FakeDom } from "./testdom";
import {
  insertionGap,
  landingSlot,
  nextWanted,
  rowHeightFor,
  stripWindow,
  Thumbnails,
  type ThumbnailOptions,
} from "./thumbnails";

const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("./tiles", () => tiles);

/** A window of rows, as `stripWindow` reports one. */
function window(first: number, last: number): { first: number; last: number } {
  return { first, last };
}

/** A `have` predicate from a list of pages already rendered. */
function rendered(...pages: number[]): (page: number) => boolean {
  const set = new Set(pages);
  return (page) => set.has(page);
}

/** A 40-page strip on the fake root, with nothing to borrow unless asked. */
function makeStrip(dom: FakeDom, opts: Partial<ThumbnailOptions> = {}): Thumbnails {
  return new Thumbnails(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 40,
    page: { width_pt: 600, height_pt: 800 },
    tier1: { placeholderFor: () => null },
    onNavigate: () => {},
    ...opts,
  });
}

/** A settled render, carrying a bitmap whose disposal a test can watch. */
function render(close: () => void): unknown {
  return { bitmap: { close } as unknown as ImageBitmap, bytes: 1, renderUs: 1, decodeMs: 1 };
}

describe("stripWindow", () => {
  // 100 px rows, a 350 px panel, no overscan: rows 0..3 are on screen.
  it("covers every row the panel shows", () => {
    expect(stripWindow(0, 350, 100, 50, 0)).toEqual(window(0, 3));
  });

  it("includes a row that is only partly visible", () => {
    // Visible span [50, 430): row 4 spans 400..500, so 30 px of it is on
    // screen and dropping it would leave a visible strip of blank.
    expect(stripWindow(50, 380, 100, 50, 0)).toEqual(window(0, 4));
  });

  it("excludes a row whose top edge is exactly the bottom of the panel", () => {
    // The boundary the test above nearly asserted by accident. Span [50, 400):
    // row 4 begins at 400 and is not visible, so building it would be one more
    // Pdfium render call than the screen can show --- 1.5 s of one, on the A0
    // sheet.
    expect(stripWindow(50, 350, 100, 50, 0)).toEqual(window(0, 3));
  });

  it("adds the overscan on both sides", () => {
    expect(stripWindow(1000, 350, 100, 50, 3)).toEqual(window(7, 16));
  });

  it("does not run off either end of the document", () => {
    expect(stripWindow(0, 350, 100, 50, 3)).toEqual(window(0, 6));
    expect(stripWindow(4700, 350, 100, 50, 3)).toEqual(window(44, 49));
  });

  it("gives one row when the panel has not been laid out yet", () => {
    // Height is zero until the layout reaches the panel. An empty window here
    // would mean nothing is ever built, and nothing would then resize it.
    expect(stripWindow(0, 0, 100, 50, 0)).toEqual(window(0, 0));
  });

  it("gives nothing for a document with no pages", () => {
    expect(stripWindow(0, 350, 100, 0, 3)).toEqual(window(0, -1));
  });

  it("gives nothing rather than dividing by a zero row height", () => {
    expect(stripWindow(0, 350, 0, 50, 3)).toEqual(window(0, -1));
  });
});

describe("nextWanted", () => {
  it("names the page at the centre when it has no thumbnail", () => {
    expect(nextWanted(window(0, 9), 4, rendered())).toBe(4);
  });

  it("works outwards from the centre, not from the top", () => {
    // The whole point of the ordering. On the A0 sheet a thumbnail costs 1.5 s,
    // so a strip that started at page 0 for a reader on page 400 would render
    // hundreds of pictures nobody asked for before reaching the one they can
    // see. Written as `for (page = first; ...)` this test is what fails.
    expect(nextWanted(window(0, 9), 5, rendered(5))).toBe(4);
    expect(nextWanted(window(0, 9), 5, rendered(4, 5))).toBe(6);
    expect(nextWanted(window(0, 9), 5, rendered(4, 5, 6))).toBe(3);
  });

  it("prefers the row above when both are equally far", () => {
    // Not arbitrary: rows above the centre are the ones a reader scrolling
    // down has just passed and is most likely to scroll back to. Asserted so
    // the tie is a decision rather than an accident of loop order.
    expect(nextWanted(window(0, 9), 5, rendered(5))).toBe(4);
  });

  it("stays inside the window even when the reader is outside it", () => {
    // The strip can be scrolled away from the page being read. What is worth
    // rendering is what is on screen; the centre only orders it.
    expect(nextWanted(window(20, 25), 0, rendered())).toBe(20);
    expect(nextWanted(window(20, 25), 99, rendered())).toBe(25);
  });

  it("names nothing when every row in the window is drawn", () => {
    expect(nextWanted(window(2, 5), 3, rendered(2, 3, 4, 5))).toBeNull();
  });

  it("reaches the last row of the window", () => {
    // The loop bound is the width of the window, and an off-by-one there leaves
    // one row permanently blank at whichever end is furthest from the centre.
    expect(nextWanted(window(0, 9), 0, rendered(0, 1, 2, 3, 4, 5, 6, 7, 8))).toBe(9);
    expect(nextWanted(window(0, 9), 9, rendered(1, 2, 3, 4, 5, 6, 7, 8, 9))).toBe(0);
  });

  it("names nothing for an empty window", () => {
    expect(nextWanted(window(0, -1), 0, rendered())).toBeNull();
  });
});

describe("insertionGap", () => {
  // 100 px rows, 10 pages: gaps 0..10, and gap g is the line at y = 100g.
  it("names the gap above the row the pointer is in the top half of", () => {
    expect(insertionGap(240, 100, 10)).toBe(2);
  });

  it("names the gap below the row the pointer is in the bottom half of", () => {
    expect(insertionGap(260, 100, 10)).toBe(3);
  });

  it("has one more gap than there are rows", () => {
    // The whole reason this answers a gap rather than a row: dropping after the
    // last page has to be sayable, and a row index cannot say it.
    expect(insertionGap(1000, 100, 10)).toBe(10);
  });

  it("does not run off either end", () => {
    expect(insertionGap(-500, 100, 10)).toBe(0);
    expect(insertionGap(99999, 100, 10)).toBe(10);
  });

  it("gives the first gap rather than dividing by a zero row height", () => {
    // Same guard as `stripWindow`, and for the same reason: a strip whose panel
    // has not been laid out yet has no row height, and NaN would propagate into
    // a slot index and out to the model.
    expect(insertionGap(300, 0, 10)).toBe(0);
  });
});

describe("landingSlot", () => {
  it("leaves a gap above the page where it is", () => {
    // Nothing above slot 5 moves when the page at 5 leaves, so the gap and the
    // landing are the same number.
    expect(landingSlot(5, 2)).toBe(2);
  });

  it("takes one off a gap below the page, because the page has left it", () => {
    // Gap 8 is read against an order that still contains the page at 5.
    // Removing it first pulls everything below up by one, so the page lands at
    // 7 -- and a version without this reads as "a drag towards the back always
    // stops one short", which is the shape the frontend mutation reproduces.
    expect(landingSlot(5, 8)).toBe(7);
  });

  it("calls both gaps either side of the page itself a no-op", () => {
    // The property that makes a drag that goes nowhere do nothing. Gap 5 is
    // already where the page is; gap 6 is the other side of the same page, and
    // it has to come back as 5 rather than as 6 -- otherwise releasing the
    // pointer a pixel below where it was pressed moves the page one slot down.
    expect(landingSlot(5, 5)).toBe(5);
    expect(landingSlot(5, 6)).toBe(5);
  });

  it("moves a page to the very front and to the very back", () => {
    expect(landingSlot(3, 0)).toBe(0);
    // Ten pages, so gap 10 is past the last one and the landing is slot 9.
    expect(landingSlot(3, 10)).toBe(9);
  });
});

describe("rowHeightFor", () => {
  it("keeps the page's aspect ratio", () => {
    const portrait = rowHeightFor({ width_pt: 612, height_pt: 792 });
    const landscape = rowHeightFor({ width_pt: 792, height_pt: 612 });
    expect(portrait).toBeGreaterThan(landscape);
  });

  it("leaves room for the page number under the picture", () => {
    // A row exactly as tall as its thumbnail would overlap the number with the
    // next page's picture.
    const page = { width_pt: 100, height_pt: 100 };
    expect(rowHeightFor(page)).toBeGreaterThan(116);
  });

  it("does not divide by a zero page width", () => {
    expect(Number.isFinite(rowHeightFor({ width_pt: 0, height_pt: 792 }))).toBe(true);
  });

  it("measures the page as the view shows it, not as the file has it", () => {
    // A portrait page rotated a quarter turn is a landscape row, and a strip
    // that sized its rows from the file would leave a gap under every one of
    // them --- while the borrowed bitmap, which *is* rotated, overflowed.
    const portrait = { width_pt: 612, height_pt: 792 };
    const landscape = { width_pt: 792, height_pt: 612 };
    expect(rowHeightFor(portrait, 1)).toBe(rowHeightFor(landscape, 0));
    expect(rowHeightFor(portrait, 3)).toBe(rowHeightFor(landscape, 0));
  });

  it("is unchanged by a half turn", () => {
    // The control for the test above: a defect that swapped the dimensions on
    // *every* non-zero rotation would pass it, and fails here.
    const portrait = { width_pt: 612, height_pt: 792 };
    expect(rowHeightFor(portrait, 2)).toBe(rowHeightFor(portrait, 0));
    expect(rowHeightFor(portrait, 4)).toBe(rowHeightFor(portrait, 0));
  });
});

/**
 * What the strip does once it has been torn down.
 *
 * The renderer is one FIFO thread shared with the page the reader is looking
 * at, so a strip that keeps asking after its document is gone is not merely
 * wasteful --- every doomed thumbnail is 1.5 s of that thread on the A0 sheet,
 * in front of the tiles for the document that replaced it.
 */
describe("Thumbnails lifetime", () => {
  let dom: FakeDom;
  /** Settles the one outstanding render, whenever the test wants it to. */
  let deliver: (result: unknown) => void;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    deliver = () => {};
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
    );
  });

  afterEach(() => {
    dom.restore();
  });

  function strip(): Thumbnails {
    return makeStrip(dom);
  }

  it("asks for nothing more once it has been destroyed", async () => {
    // `pump` refuses to issue anything while the strip is inactive, and a
    // teardown that left it active kept that door open: the request settling
    // pumps the next page, whose reply pumps the one after it.
    const pages = strip();
    pages.setActive(true);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(1);

    pages.destroy();
    deliver(null);
    await settle();

    expect(tiles.fetchTile).toHaveBeenCalledTimes(1);
  });

  it("throws its thumbnails away when the order changes and the count does not", async () => {
    // The case a deletion cannot produce and a move always does. This method
    // returned early on a matching count, which is right for the operation it
    // was written for and leaves the strip showing the old order for the one it
    // was not: same rows, same captions, the wrong pictures under them.
    const closed: number[] = [];
    const pages = strip();
    pages.setActive(true);
    deliver(render(() => closed.push(1)));
    await settle();
    // The control: nothing has been discarded yet.
    expect(closed).toEqual([]);

    // The same count it already has, which is what a move reports.
    pages.setPages(40);
    await settle();
    expect(closed).toEqual([1]);
  });

  it("releases a borrowed copy that lands after it was destroyed", async () => {
    // The strip's *other* arrival, and it was the one this fixture could not
    // reach: `placeholderFor` returns null above, so nothing is ever borrowed
    // and a mutation removing this disposal survived the whole suite.
    //
    // Worth stating why the copy needs releasing at all when the scroller owns
    // the original: `createImageBitmap` produces a second GPU-backed bitmap, and
    // `destroy` does not clear `borrowing` --- so the copy passes the staleness
    // test inside the continuation and is kept in a map that was just emptied.
    const close = vi.fn();
    let finish: (bitmap: ImageBitmap) => void = () => {};
    const created = globalThis.createImageBitmap;
    globalThis.createImageBitmap = (() =>
      new Promise<ImageBitmap>((resolve) => {
        finish = resolve;
      })) as typeof globalThis.createImageBitmap;

    const borrowed = { close: vi.fn() } as unknown as ImageBitmap;
    const pages = makeStrip(dom, { tier1: { placeholderFor: () => borrowed } });
    pages.setActive(true);
    // Borrowing, not rendering: no request should have gone out at all, which
    // is also what says this test is exercising the path it claims to.
    expect(tiles.fetchTile).not.toHaveBeenCalled();

    pages.destroy();
    expect(close).not.toHaveBeenCalled();

    finish({ close } as unknown as ImageBitmap);
    await settle();
    expect(close).toHaveBeenCalledTimes(1);

    globalThis.createImageBitmap = created;
  });

  it("releases a thumbnail that lands after it was destroyed", async () => {
    // The other half of the teardown, and the half a `pump` guard cannot cover:
    // `keep` puts the bitmap in a map that `destroy` has already emptied, so
    // nothing will ever close it. Refusing to *pump* afterwards is not the same
    // as refusing to *keep*, and the strip did the first only.
    const close = vi.fn();
    const bitmap = { close } as unknown as ImageBitmap;

    const pages = strip();
    pages.setActive(true);
    pages.destroy();
    // The control: the teardown itself closed every bitmap it held, and this one
    // was not among them, so a `close` after this line is the delivery's doing.
    expect(close).not.toHaveBeenCalled();

    deliver({ bitmap, bytes: 1, renderUs: 1, decodeMs: 1 });
    await settle();

    expect(close).toHaveBeenCalledTimes(1);
  });

  it("still keeps a thumbnail that lands while it is alive", async () => {
    // The control for the test above: a strip that closed every arrival would
    // pass it while rendering an empty column.
    const close = vi.fn();
    const bitmap = { close } as unknown as ImageBitmap;

    const pages = strip();
    pages.setActive(true);
    deliver({ bitmap, bytes: 1, renderUs: 1, decodeMs: 1 });
    await settle();

    expect(close).not.toHaveBeenCalled();
    pages.destroy();
    // And the teardown is what releases it, which is the pair of facts that
    // says the bitmap was genuinely kept rather than quietly dropped.
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("names a thumbnail that failed, once", async () => {
    // `tiles.ts` builds an error naming the page and every catch here dropped
    // it, so a strip with a blank row said nothing anywhere about why. Once per
    // page is all there is to say --- a failed page is never retried for the
    // life of this orientation.
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    let reject: (reason: Error) => void = () => {};
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((_resolve, fail) => {
          reject = fail;
        }),
    );
    const pages = strip();
    pages.setActive(true);
    reject(new Error("page is broken"));
    await settle();

    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain("page is broken");
    pages.destroy();
    warn.mockRestore();
  });

  it("asks for the next one while it is alive", async () => {
    // The control. A strip that stopped pumping on every settling reply would
    // pass the test above and never draw a second thumbnail.
    const pages = strip();
    pages.setActive(true);
    deliver(null);
    await settle();

    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);
    pages.destroy();
  });
});

/**
 * What the strip does with a render that beat its own withdrawal.
 *
 * `setTurns` and `setInvert` drop every bitmap and withdraw whatever is
 * outstanding --- but a withdrawal only marks a request that is still queued or
 * running, and one that has already finished comes back a *full result*. Kept,
 * that is a picture of the previous orientation which `have()` then reports as
 * drawn, so the page is never rendered again for the life of the document: one
 * sideways or one un-inverted thumbnail, permanently, in a strip where every
 * other row obeyed the reader.
 *
 * The other two rendering paths guard exactly this --- the borrow path with
 * `borrowing`, the scroller with its placeholder generation --- and this was the
 * one of the three without an epoch.
 */
describe("Thumbnails orientation", () => {
  let dom: FakeDom;
  /** Settles the one outstanding render, whenever the test wants it to. */
  let deliver: (result: unknown) => void;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    deliver = () => {};
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
    );
  });

  afterEach(() => {
    dom.restore();
  });

  /** What the strip last asked the renderer for. */
  function lastRequest(): { page: number; turns: number; invert: boolean } {
    const calls = tiles.fetchTile.mock.calls;
    return calls[calls.length - 1]?.[0] as { page: number; turns: number; invert: boolean };
  }

  it("drops a thumbnail that finished before a rotation withdrew it", async () => {
    const stale = vi.fn();
    const fresh = vi.fn();
    const pages = makeStrip(dom);
    pages.setActive(true);
    const asked = lastRequest();
    expect(asked.turns).toBe(0);

    pages.setTurns(1);
    deliver(render(stale));
    await settle();

    expect(stale).toHaveBeenCalledTimes(1);
    expect(pages.rendered).not.toContain(asked.page);
    // Dropped is only half of it: a page left undrawn and not asked for again
    // is the same blank row by another route, and `have()` reads a kept bitmap
    // as "done". The re-render must be in the orientation the reader is now in.
    expect(lastRequest()).toMatchObject({ page: asked.page, turns: 1 });

    // The control, in the same test rather than beside it, because the mutation
    // it guards against is "close every result": that would satisfy every
    // assertion above and leave the strip permanently empty.
    deliver(render(fresh));
    await settle();
    expect(fresh).not.toHaveBeenCalled();
    expect(pages.rendered).toContain(asked.page);

    pages.destroy();
  });

  it("drops one that finished before an inversion withdrew it", async () => {
    // The same race on the other epoch. Two paths bump it, and a guard that
    // followed only the rotation would pass the test above and leave a strip of
    // white thumbnails behind a reader who asked for dark ones.
    const stale = vi.fn();
    const fresh = vi.fn();
    const pages = makeStrip(dom);
    pages.setActive(true);
    const asked = lastRequest();
    expect(asked.invert).toBe(false);

    pages.setInvert(true);
    deliver(render(stale));
    await settle();

    expect(stale).toHaveBeenCalledTimes(1);
    expect(pages.rendered).not.toContain(asked.page);
    expect(lastRequest()).toMatchObject({ page: asked.page, invert: true });

    deliver(render(fresh));
    await settle();
    expect(fresh).not.toHaveBeenCalled();
    expect(pages.rendered).toContain(asked.page);

    pages.destroy();
  });

  it("counts a withdrawal made for the viewer, and not one made for a rotation", async () => {
    // `yieldCount` is read by `viewercheck.ts` as a *contention* metric --- it
    // decides whether the strip got out of the way within two frames of the
    // viewer wanting tiles. A rotation withdraws too, and for an unrelated
    // reason: the picture asked for is no longer the one wanted. Counted
    // together, the number says less the more the reader rotates.
    const pages = makeStrip(dom);
    pages.setActive(true);
    expect(pages.yieldCount).toBe(0);

    pages.setTurns(1);
    expect(pages.yieldCount).toBe(0);

    // Settled first: a request already withdrawn is withdrawn only once, so
    // without this the viewer's own withdrawal below would find nothing to do
    // and the assertion would hold for the wrong reason.
    deliver(null);
    await settle();
    expect(pages.outstanding).toBe(true);

    pages.setViewerBusy(true);
    expect(pages.yieldCount).toBe(1);

    pages.destroy();
  });
});

describe("Thumbnails keyboard activation", () => {
  let dom: FakeDom;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
  });

  afterEach(() => {
    dom.restore();
  });

  /**
   * A strip with rows actually built. The panel it makes for itself has no
   * height under the fake DOM, and a strip that cannot measure its panel builds
   * a single row -- so every row past the first has to be laid out on purpose.
   */
  function stripWithRows(navigated: number[]): Thumbnails {
    const pages = makeStrip(dom, { onNavigate: (page: number) => navigated.push(page) });
    const host = dom.root.children[dom.root.children.length - 1]!;
    host.clientHeight = 700;
    host.dispatch("scroll", {});
    return pages;
  }

  it("activates the row the key reached, not the one it last tracked", () => {
    const navigated: number[] = [];
    const pages = stripWithRows(navigated);
    const row = pages.elementFor(3) as unknown as (typeof dom.root | null);
    expect(row).not.toBeNull();

    // Focus reached the row without this class's `focusin` listener ever
    // running. That is not a contrived state: a document without system focus
    // moves `activeElement` and does not deliver the focus event, so the row
    // this class believes is focused is still page 0 while the key event lands
    // on page 3. Activating the tracked row sends the reader to page 1, which
    // is what `viewer_check.py` caught once on `vector-multi` and what reads in
    // a transcript as a navigation that did nothing.
    const list = row!.parent!.parent!;
    list.dispatch("keydown", { key: "Enter", target: row });

    expect(navigated).toEqual([3]);
    pages.destroy();
  });

  it("steps from the row the key reached, not the one it last tracked", () => {
    // Enter had the reconciliation above and the arrows did not, so the same
    // stale mirror that would have sent Enter to page 0 stepped ArrowDown from
    // page 0 as well: the reader on page 3 pressed Down and the roving tabindex
    // went to page 1. Three of the four roving lists in this frontend fixed
    // this; this was the fourth, and the explanation was already in this file
    // attached to Enter alone.
    const navigated: number[] = [];
    const pages = stripWithRows(navigated);
    const row = pages.elementFor(3) as unknown as (typeof dom.root | null);
    expect(row).not.toBeNull();

    const list = row!.parent!.parent!;
    list.dispatch("keydown", { key: "ArrowDown", target: row });

    expect(pages.elementFor(4)?.tabIndex).toBe(0);
    expect(pages.elementFor(3)?.tabIndex).toBe(-1);
    // The row the defect moved to. Asserted by name rather than left to the
    // two above, because "4 is tabbable" is also satisfied by a class that
    // makes every row tabbable, and 1 is the wrong answer this is about.
    expect(pages.elementFor(1)?.tabIndex).toBe(-1);
    pages.destroy();
  });

  it("falls back to the tracked row when the key did not come from one", () => {
    // The control on the assertion above: the fallback still has to work, or
    // "use the event's row" would be satisfied by a class that never activates
    // anything. A key on the list itself carries no page.
    const navigated: number[] = [];
    const pages = stripWithRows(navigated);
    const list = (pages.elementFor(0) as unknown as typeof dom.root)!.parent!.parent!;
    list.dispatch("keydown", { key: "Enter", target: list });

    expect(navigated).toEqual([0]);
    pages.destroy();
  });
});

/**
 * Dragging a row to reorder the document.
 *
 * The arithmetic is tested above as two pure functions, because that is where
 * the off-by-one lives. What is tested here is everything the pure functions
 * cannot see: that a press is not a drag until it has travelled, that a drop
 * calls the handler once with the slots the pointer actually described, and
 * that the three ways a drag ends without a drop really do end it.
 */
describe("Thumbnails dragging", () => {
  let dom: FakeDom;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
  });

  afterEach(() => {
    dom.restore();
  });

  /** The strip's own scrolling panel, which is where the pointer lands. */
  function panelOf(): FakeDom["root"] {
    return dom.root.children[dom.root.children.length - 1]!;
  }

  /**
   * A strip whose rows exist, with the row height forced to a round number.
   *
   * `rowHeightFor` derives it from the page geometry, so a test that assumed
   * 100 would be asserting against whatever that arithmetic currently gives.
   * A 600x800 page at 116 px wide is 155 px of picture plus the label and the
   * padding, and the drags below are written in multiples of that rather than
   * of a number this file chose.
   */
  function stripForDrag(moves: [number, number][]): {
    pages: Thumbnails;
    panel: FakeDom["root"];
    rowHeight: number;
  } {
    const pages = makeStrip(dom, {
      onReorder: (from: number, to: number) => moves.push([from, to]),
    });
    const panel = panelOf();
    panel.clientHeight = 700;
    panel.dispatch("scroll", {});
    return { pages, panel, rowHeight: rowHeightFor({ width_pt: 600, height_pt: 800 }) };
  }

  /** Presses a row, at the top of its own box. */
  function press(pages: Thumbnails, page: number, y: number): void {
    const row = pages.elementFor(page) as unknown as FakeDom["root"];
    row.dispatch("pointerdown", { pointerId: 1, clientY: y, preventDefault: () => {} });
  }

  it("does not reorder anything when the pointer barely moves", () => {
    // The control the whole threshold exists for: every click on a thumbnail is
    // a press followed by a release, and a strip that treated that as a drag
    // would reorder the document every time a reader looked at a page.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 2, 2 * rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 2 * rowHeight + 3 });
    expect(pages.dragging).toBe(false);
    panel.dispatch("pointerup", { pointerId: 1, clientY: 2 * rowHeight + 3 });

    expect(moves).toEqual([]);
    expect(pages.dropCount).toBe(0);
    pages.destroy();
  });

  it("moves a page to the slot the pointer was over when it was released", () => {
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    // Up to the very top of the strip: gap 0, which is also landing 0.
    panel.dispatch("pointermove", { pointerId: 1, clientY: 2 });
    expect(pages.dragging).toBe(true);
    panel.dispatch("pointerup", { pointerId: 1, clientY: 2 });

    expect(moves).toEqual([[3, 0]]);
    expect(pages.dropCount).toBe(1);
    pages.destroy();
  });

  it("takes the gap from where the pointer was, not from where it ended up", () => {
    // A drop is allowed to happen outside the panel -- the pointer is captured,
    // so a release anywhere is delivered here. The gap the indicator last showed
    // is what the reader agreed to, so it is what the drop uses; recomputing it
    // from the release coordinates would move the page somewhere nothing ever
    // pointed at.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 0, 0);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 2 * rowHeight });
    panel.dispatch("pointerup", { pointerId: 1, clientY: -9999 });

    expect(moves).toEqual([[0, 1]]);
    pages.destroy();
  });

  it("ignores a pointer that is not the one being dragged", () => {
    // A second finger on a touchpad, or a second pointer on a trackpad-and-mouse
    // machine. Without the id check its movement aims the drag.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    panel.dispatch("pointermove", { pointerId: 9, clientY: 0 });
    expect(pages.dragging).toBe(false);
    panel.dispatch("pointerup", { pointerId: 9, clientY: 0 });

    expect(moves).toEqual([]);
    // And the drag the other pointer started is still live, which is what says
    // the guard ignored the stray pointer rather than swallowing the drag.
    panel.dispatch("pointermove", { pointerId: 1, clientY: 0 });
    expect(pages.dragging).toBe(true);
    pages.destroy();
  });

  it("abandons the drag on Escape rather than dropping it", () => {
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 0 });
    expect(pages.dragging).toBe(true);

    const list = (pages.elementFor(0) as unknown as FakeDom["root"]).parent!.parent!;
    list.dispatch("keydown", { key: "Escape", preventDefault: () => {} });
    expect(pages.dragging).toBe(false);
    // The release still arrives -- the pointer is a real pointer and the reader
    // still lets go of it -- and it must not become the drop Escape refused.
    panel.dispatch("pointerup", { pointerId: 1, clientY: 0 });

    expect(moves).toEqual([]);
    pages.destroy();
  });

  it("abandons the drag when a pointercancel arrives", () => {
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 0 });
    panel.dispatch("pointercancel", { pointerId: 1, clientY: 0 });

    expect(pages.dragging).toBe(false);
    expect(moves).toEqual([]);
    pages.destroy();
  });

  it("releases the pointer it captured", () => {
    // Not cosmetic: a captured pointer sends every subsequent move and release
    // to this element, so a capture that is never released leaves the strip
    // swallowing pointer events for the rest of the session.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    expect(panel.captured.has(1)).toBe(true);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 0 });
    panel.dispatch("pointerup", { pointerId: 1, clientY: 0 });

    expect(panel.captured.has(1)).toBe(false);
    pages.destroy();
  });

  it("runs no edit when the document is rebuilt under a live drag", () => {
    // `setPages` is what the drop's own edit comes back through, so completing
    // a drag there would apply the reader's move twice. It is also what a
    // deletion from anywhere else calls, and then the slots the drag was aimed
    // at have stopped meaning what they meant.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 3, 3 * rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 0 });
    expect(pages.dragging).toBe(true);

    pages.setPages(40);
    expect(pages.dragging).toBe(false);
    panel.dispatch("pointerup", { pointerId: 1, clientY: 0 });

    expect(moves).toEqual([]);
    pages.destroy();
  });

  it("does not drag at all when nothing is listening", () => {
    // The strip is also driven by harnesses and by documents nobody can edit.
    // Without a handler there is no capture to take, and taking one anyway
    // would swallow pointer events for a gesture that can never do anything.
    const pages = makeStrip(dom);
    const panel = panelOf();
    panel.clientHeight = 700;
    panel.dispatch("scroll", {});
    const row = pages.elementFor(2) as unknown as FakeDom["root"];
    row.dispatch("pointerdown", { pointerId: 1, clientY: 0, preventDefault: () => {} });

    expect(panel.captured.size).toBe(0);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 900 });
    expect(pages.dragging).toBe(false);
    pages.destroy();
  });

  it("does not follow the page being read while a pointer is down on a row", () => {
    // Pressing a row navigates, and navigating comes back as "show the page
    // being read". Acting on that mid-press slides the content out from under a
    // pointer that has not moved, so the drop lands on a gap nobody pointed at.
    // The corpus sweep caught this on four of fourteen documents; the ten that
    // passed had strips short enough for the scroll to clamp, which is a fixture
    // hiding a defect rather than a defect that only sometimes happens.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    // The strip only follows the page when it is the tab on show, which is what
    // `setCurrentPage` guards on -- so without this the control below asserts
    // against a strip that was never going to scroll for a second reason.
    pages.setActive(true);
    // A control: with no pointer down, the strip does follow the page. Without
    // this, "does not scroll" is satisfied by a strip that never scrolls.
    pages.setCurrentPage(20);
    const followed = panel.scrollTop;
    expect(followed).toBeGreaterThan(0);

    // A row that is still built after that scroll -- rows 0 and 1 left the
    // window with it, and a press on an element that is no longer in the
    // document is not the gesture under test.
    press(pages, 20, 20 * rowHeight);
    pages.setCurrentPage(0);
    expect(panel.scrollTop).toBe(followed);
    pages.destroy();
  });

  it("scrolls the strip while the pointer rests against the bottom edge", () => {
    // The reason this is a frame loop and not a step per move: a reader who has
    // already reached the edge stops moving the pointer, and without the loop
    // the strip stops with them.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 1, rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 690 });
    // Nothing yet: the move schedules the loop rather than scrolling, so that
    // the speed is per frame rather than per event -- a trackpad that reports
    // at 120 Hz and a mouse that reports at 60 must not scroll at two speeds.
    expect(panel.scrollTop).toBe(0);
    dom.runFrames();
    const first = panel.scrollTop;
    expect(first).toBeGreaterThan(0);

    // No further pointer movement at all, which is the whole point.
    dom.runFrames();
    expect(panel.scrollTop).toBeGreaterThan(first);
    pages.destroy();
  });

  it("stops the edge loop when the strip can scroll no further", () => {
    // Otherwise it reschedules itself for the life of the drag, which on a
    // document already at its end is a frame callback per frame doing nothing.
    const moves: [number, number][] = [];
    const { pages, panel, rowHeight } = stripForDrag(moves);
    press(pages, 1, rowHeight);
    panel.dispatch("pointermove", { pointerId: 1, clientY: 2 });
    // At the top already, so the first frame cannot move it.
    expect(panel.scrollTop).toBe(0);
    dom.reset();
    dom.runFrames();

    expect(dom.scheduledFrames()).toBe(0);
    pages.destroy();
  });
});
