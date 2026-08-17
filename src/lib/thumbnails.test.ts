import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installFakeDom, settle, type FakeDom } from "./testdom";
import {
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
