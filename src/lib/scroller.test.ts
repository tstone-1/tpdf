/**
 * Tests for the scroller's use of the retry wait, and for the geometry it
 * shares with the page strip.
 *
 * `backoff.test.ts` pins what the wait *is*. This pins that the scroller
 * actually consults it, which is a separate claim and the one that failed
 * before: `request()` runs on every frame and re-issues anything that is not
 * cached and not in flight, so a failure that records nothing is a tile
 * requested at display cadence --- and under the worker backend, a sandboxed
 * process forked and killed at display cadence --- for as long as the document
 * stays open. A test of the class alone cannot see that.
 *
 * The clock is passed in rather than waited for. `frame()` takes the reading it
 * should use, which is also the fix for a request coming due between two
 * separate readings, so a test can step 250 ms without spending 250 ms.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  displayedSize,
  Scroller,
  type PageSize,
  type ScrollerOptions,
} from "./scroller";
import { installFakeDom, settle, type FakeDom } from "./testdom";
// The real class, not a stand-in supplied by the mock factory above: it
// lives outside the mocked module precisely so `instanceof` means the same
// thing here as in production. `tiles.test.ts` owns the other half of the
// chain --- that a 410 is what produces one.
import { DocumentGone } from "./tilestatus";

const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("./tiles", () => tiles);

/** One page, one tile, no prefetch --- the smallest thing that requests. */
function options(): ScrollerOptions {
  return {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
    zoom: 1,
    turns: 0,
    invert: false,
    layout: "viewport",
    // Larger than the page, so there is exactly one tier-2 tile to reason about
    // beside the tier-1 placeholder.
    tilePx: 4096,
    dpr: 1,
    viewport: { width: 900, height: 900 },
    prefetchScreens: 0,
    cacheTiles: 48,
    maxInFlight: 4,
    cancel: false,
  };
}

describe("Scroller retries", () => {
  let dom: FakeDom;
  let scroller: Scroller;
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    // Every request fails, which is the case the wait exists for.
    tiles.fetchTile.mockImplementation(() => Promise.reject(new Error("boom")));
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    scroller = new Scroller(dom.root as unknown as HTMLElement, options());
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  it("does not re-issue a failed request on the next frame", async () => {
    // The whole of it. Two requests go out --- the tier-1 placeholder and the
    // one tile --- both fail, and the very next frame must ask for neither.
    const t0 = performance.now();
    scroller.frame(0, t0);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);

    await settle();
    scroller.frame(0, t0 + 1);
    scroller.frame(0, t0 + 2);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);
  });

  it("re-issues once the wait has elapsed", async () => {
    // The control for the test above, and not a formality: a scroller that
    // recorded a failure and never cleared it would pass that test perfectly
    // while leaving a tile that failed once permanently blank.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    scroller.frame(0, t0 + 10_000);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(4);
  });

  it("counts a failure rather than swallowing it", async () => {
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(scroller.stats.failed).toBe(2);
  });

  it("says why a tile failed once, not on every retry", async () => {
    // The reason `tiles.ts` builds is the same on every attempt, and the
    // attempts go on for as long as the document is open --- so a warning per
    // attempt is a console nobody reads, which is where this started.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(warn).toHaveBeenCalledTimes(2);
    expect(String(warn.mock.calls[0]?.[0])).toContain("boom");

    scroller.frame(0, t0 + 10_000);
    await settle();
    expect(tiles.fetchTile).toHaveBeenCalledTimes(4);
    expect(warn).toHaveBeenCalledTimes(2);
  });

  it("asks for a wake against the frame's own clock reading", async () => {
    // A second reading here is the defect: an entry that came due between the
    // frame and this call is not issued by the frame and gets no wake either.
    // Asked with the frame's own reading, a request that failed during it is
    // always still in the future.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    const wait = scroller.nextRetryMs(t0);
    expect(wait).not.toBeNull();
    expect(wait).toBeGreaterThan(0);
  });

  it("wants no wake when nothing has failed", async () => {
    tiles.fetchTile.mockImplementation(() => Promise.resolve(null));
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(scroller.nextRetryMs(t0)).toBeNull();
  });

  it("gives a reader who changes the view a fresh attempt", async () => {
    // Backoff is dropped by a zoom, a rotation or an inversion and by nothing
    // on the frame path --- someone who has just asked for a different picture
    // is owed an immediate try rather than the tail of a wait they cannot see.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    scroller.setZoom(2);
    scroller.frame(0, t0 + 1);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(4);
  });
});

describe("Scroller when the document's file is gone", () => {
  let dom: FakeDom;
  let scroller: Scroller;
  let warn: ReturnType<typeof vi.spyOn>;
  let gone: string[];

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() =>
      Promise.reject(new DocumentGone("this file changed on disk")),
    );
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    gone = [];
    scroller = new Scroller(dom.root as unknown as HTMLElement, {
      ...options(),
      onGone: (message) => gone.push(message),
    });
  });

  afterEach(() => {
    warn.mockRestore();
    dom.restore();
  });

  it("stops asking altogether rather than backing off", async () => {
    // The distinction this whole mechanism exists for, and the reason the
    // ordinary backoff is not good enough: a wait *schedules a retry*, and every
    // retry of a vanished document is another refusal for as long as the reader
    // leaves the window open. The control is the neighbouring suite, where the
    // same two tiles do come back after 10 s.
    const t0 = performance.now();
    scroller.frame(0, t0);
    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);

    await settle();
    scroller.frame(0, t0 + 10_000);
    scroller.frame(0, t0 + 100_000);
    await settle();
    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);
  });

  it("wants no wake, because there is nothing to wake for", async () => {
    // A scroller that latched but still asked for a timer would wake the window
    // forever to do nothing --- which no coverage of the request count can see.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(scroller.nextRetryMs(t0)).toBeNull();
  });

  it("reports once, however many tiles were in flight", async () => {
    // Two tiles fail here, and they are one piece of news: the file is gone. A
    // report per tile is a stack of identical messages at whatever the document
    // had outstanding, which on a real page is a dozen.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(tiles.fetchTile).toHaveBeenCalledTimes(2);
    expect(gone).toEqual(["this file changed on disk"]);
  });

  it("says nothing to the console, having said it to the reader", async () => {
    // The per-tile warning is for a failure nobody is told about. This one is
    // reported to the window, so the same text in the console is noise --- and
    // it is the ordinary-failure path's warning, which would mean the latch had
    // not been taken.
    const t0 = performance.now();
    scroller.frame(0, t0);
    await settle();
    expect(warn).not.toHaveBeenCalled();
  });

  it("keeps what is already painted", async () => {
    // Not politeness. Those tiles are the last true picture of the document
    // there will be, and a scroller that cleared on the way to reporting would
    // replace something correct with a blank page.
    const t0 = performance.now();
    tiles.fetchTile.mockImplementationOnce(() => Promise.resolve(null));
    scroller.frame(0, t0);
    await settle();
    expect(scroller.stats.failed).toBeGreaterThan(0);
    expect(dom.root.children.length).toBeGreaterThan(0);
  });
});

describe("Scroller teardown", () => {
  let dom: FakeDom;
  let scroller: Scroller;

  /** A tile reply whose bitmap says whether anyone released it. */
  function delivery() {
    const bitmap = { close: vi.fn(), width: 64, height: 64 };
    return {
      bitmap: bitmap as unknown as ImageBitmap,
      close: bitmap.close,
      result: {
        bitmap: bitmap as unknown as ImageBitmap,
        bytes: 1,
        renderUs: 1,
        decodeMs: 1,
      },
    };
  }

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    scroller = new Scroller(dom.root as unknown as HTMLElement, options());
  });

  afterEach(() => {
    dom.restore();
  });

  it("releases a tile that lands after it was destroyed", async () => {
    // Withdrawal races the renderer: `destroy` cancels everything outstanding,
    // but a tile that had already finished still arrives, and the queue it used
    // to be pushed onto is drained by a frame loop that no longer runs. An
    // `ImageBitmap` is GPU-backed and freed only by `close`, so a continuation
    // that returned early here would leak exactly as much as one that queued it.
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) =>
          late.push(resolve as (value: unknown) => void),
        ),
    );

    scroller.frame(0, performance.now());
    // Both of them, because the two arrivals are separate call sites --- the
    // tier-1 placeholder and the tier-2 tile --- and guarding one is what a
    // half-done fix looks like. Which goes first is not worth depending on.
    expect(late).toHaveLength(2);

    const arrivals = [delivery(), delivery()];
    scroller.destroy();
    // The control for the assertions below: nothing has released these *yet*,
    // so a `close` afterwards is the delivery's doing, not the teardown's.
    for (const arrival of arrivals)
      expect(arrival.close).not.toHaveBeenCalled();

    late[0]?.(arrivals[0]!.result);
    late[1]?.(arrivals[1]!.result);
    await settle();
    for (const arrival of arrivals)
      expect(arrival.close).toHaveBeenCalledTimes(1);
  });

  it("still keeps a tile that lands while it is alive", async () => {
    // The control for the test above, and not a formality: a scroller that
    // closed every arrival would pass that one perfectly while drawing nothing.
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) =>
          late.push(resolve as (value: unknown) => void),
        ),
    );

    scroller.frame(0, performance.now());
    const tile = delivery();
    late[0]?.(tile.result);
    await settle();

    expect(tile.close).not.toHaveBeenCalled();
    expect(scroller.stats.bytes).toBe(1);
  });
});

/**
 * The three page sizes `testdata/mixed.pdf` is built from, in points.
 *
 * The same three, and for the same reasons its generator gives: A3 landscape
 * differs from A4 on the width axis **only**, so a failure there is the tile
 * grid and cannot be an offset; A5 is shorter, which is the axis that moves
 * every later page. A property with one value present is the same as none.
 */
const A4 = { width_pt: 595, height_pt: 842 };
const A3_LANDSCAPE = { width_pt: 1191, height_pt: 842 };
const A5 = { width_pt: 420, height_pt: 595 };

/** A tile request as the scroller issued it. */
interface Issued {
  page: number;
  scale: number;
  x: number;
  width: number;
}

/**
 * The tier-2 requests issued so far, in order.
 *
 * Tier-1 placeholders are filtered out by their scale: they are rendered to a
 * fixed 150 px, so their scale is `150 / width_pt` and never the view's own.
 * Counting them here would make "how many columns was this page asked for in"
 * answer a different question on every page size.
 */
function issuedTiles(zoom = 1): Issued[] {
  return tiles.fetchTile.mock.calls
    .map((call) => call[0] as Issued)
    .filter((request) => request.scale === zoom);
}

/** The device-pixel column the requests for a page reach, and how many. */
function reach(page: number): { right: number; columns: number } {
  const forPage = issuedTiles().filter((request) => request.page === page);
  return {
    right: Math.max(0, ...forPage.map((r) => r.x + r.width)),
    columns: new Set(forPage.map((r) => r.x)).size,
  };
}

describe("Scroller geometry on a mixed-size document", () => {
  let dom: FakeDom;

  /** Options laying out `pages` with every page of a `count`-page document. */
  function mixed(
    pages: [PageSize, ...PageSize[]],
    count = pages.length,
  ): ScrollerOptions {
    return {
      ...options(),
      pageCount: count,
      pages,
      // Small enough that the widest page needs three columns and the narrowest
      // two: a tile larger than every page cannot tell one grid from another.
      tilePx: 512,
      // Tall and wide enough to hold the whole document, so every page is in the
      // band and the assertions below are about the layout rather than about
      // which pages happened to be on screen.
      viewport: { width: 1400, height: 4000 },
      maxInFlight: 64,
    };
  }

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => Promise.resolve(null));
  });

  afterEach(() => {
    dom.restore();
  });

  it("accumulates each page's own height into the next page's offset", () => {
    // The four-page shape of the fixture: A4, A3 landscape (same height), A5
    // (shorter), A4 again. The last one is what makes this discriminate --- a
    // layout that multiplies page 1's height by the index agrees about the first
    // three offsets in a document whose first three pages are 842 points tall.
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4, A3_LANDSCAPE, A5, A4]),
    );

    // Read rather than asserted: `PAGE_GAP` is the scroller's own constant and
    // nothing outside it should be pinning the number. What is pinned is that
    // the *same* gap separates every pair, and that the heights between them are
    // each page's own.
    const gap = scroller.pageTopOf(1) - A4.height_pt;
    expect(gap).toBeGreaterThan(0);

    expect(scroller.pageTopOf(0)).toBe(0);
    expect(scroller.pageTopOf(2)).toBe(2 * (A4.height_pt + gap));
    expect(scroller.pageTopOf(3)).toBe(
      2 * (A4.height_pt + gap) + A5.height_pt + gap,
    );
    expect(scroller.documentHeight).toBe(scroller.pageTopOf(3) + A4.height_pt);
  });

  it("asks for an oversized page as far as its own right edge", () => {
    // The half the offsets cannot see, and the one that loses content: the tile
    // grid comes from the page's width, so a page laid out at page 1's width is
    // never *asked for* past it and is drawn cropped, with no error anywhere.
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4, A3_LANDSCAPE]),
    );
    scroller.frame(0, performance.now());

    // The control first, and it is the half that says the reach is a property of
    // the page rather than of the document: page 1 must *not* be asked for past
    // its own edge either.
    expect(reach(0)).toEqual({ right: A4.width_pt, columns: 2 });
    expect(reach(1)).toEqual({ right: A3_LANDSCAPE.width_pt, columns: 3 });
  });

  it("lays an unknown page out at the mean of the sizes it knows", () => {
    // A lazy open carries page 1 alone, so this is the state every document
    // starts in. One known size makes the mean page 1's size, which is what
    // section 4 relies on: page sizes within a document are overwhelmingly
    // uniform, so the estimate is usually exact immediately.
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4], 3),
    );
    expect(scroller.knowsPageSize(1)).toBe(false);
    expect(scroller.pageSize(1)).toEqual(A4);

    expect(scroller.notePageSize(1, A3_LANDSCAPE)).toBe(true);
    expect(scroller.knowsPageSize(1)).toBe(true);
    // And the estimate for what is *still* unknown follows what has been seen,
    // rather than staying pinned to page 1 for the life of the document.
    expect(scroller.pageSize(2).width_pt).toBe(
      (A4.width_pt + A3_LANDSCAPE.width_pt) / 2,
    );
  });

  it("reports no change when a learned size is the one already assumed", () => {
    // The control for the test above, and the case that matters for cost: every
    // page of every uniform document arrives here, and a scroller that relaid
    // out and threw its tiles away each time would repaint the screen once per
    // page on a document being read straight through.
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4], 3),
    );
    const height = scroller.documentHeight;
    expect(scroller.notePageSize(1, { ...A4 })).toBe(false);
    expect(scroller.notePageSize(1, { ...A4 })).toBe(false);
    expect(scroller.documentHeight).toBe(height);
  });

  it("drops a tile rendered before its page was corrected, and keeps its neighbour's", async () => {
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) =>
          late.push(resolve as (value: unknown) => void),
        ),
    );
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4], 2),
    );
    scroller.frame(0, performance.now());

    // Which resolver belongs to which page, taken from the requests themselves.
    // A withdrawal cannot reach a render that has already finished, so this is
    // the race the epoch exists for and the tile has to arrive to exercise it.
    const calls = tiles.fetchTile.mock.calls.map((call) => call[0] as Issued);
    const stale = calls.findIndex((r) => r.page === 1 && r.scale === 1);
    const neighbour = calls.findIndex((r) => r.page === 0 && r.scale === 1);
    expect(stale).toBeGreaterThanOrEqual(0);
    expect(neighbour).toBeGreaterThanOrEqual(0);

    expect(scroller.notePageSize(1, A3_LANDSCAPE)).toBe(true);

    const bitmapFor = () => {
      const bitmap = { close: vi.fn(), width: 64, height: 64 };
      return {
        close: bitmap.close,
        result: {
          bitmap: bitmap as unknown as ImageBitmap,
          bytes: 1,
          renderUs: 1,
          decodeMs: 1,
        },
      };
    };
    const dropped = bitmapFor();
    const kept = bitmapFor();
    late[stale]?.(dropped.result);
    late[neighbour]?.(kept.result);
    await settle();
    scroller.frame(0, performance.now());

    expect(dropped.close).toHaveBeenCalledTimes(1);
    // The control, and not a formality: a scroller that closed every arrival
    // would pass the line above perfectly while drawing nothing at all.
    expect(kept.close).not.toHaveBeenCalled();
    // Counted as neither delivered nor discarded --- nothing about the scroll
    // invalidated it, so reporting it as a superseded tile would be a queue
    // failure that did not happen.
    expect(scroller.stats.discarded).toBe(0);
  });

  it("asks for the corrected page again, at its new width", async () => {
    const scroller = new Scroller(
      dom.root as unknown as HTMLElement,
      mixed([A4], 2),
    );
    scroller.frame(0, performance.now());
    expect(reach(1)).toEqual({ right: A4.width_pt, columns: 2 });

    scroller.notePageSize(1, A3_LANDSCAPE);
    // Waited for on purpose. A withdrawal is a request to stop rather than proof
    // of having stopped, so the entries stay in flight until their replies land
    // and `request` will not issue a duplicate for one that is still on its way
    // --- a frame taken here would see the one genuinely new column and nothing
    // else, and report that as the whole answer.
    await settle();
    tiles.fetchTile.mockClear();
    scroller.frame(0, performance.now());
    expect(reach(1)).toEqual({ right: A3_LANDSCAPE.width_pt, columns: 3 });
  });
});

/**
 * A page turned by an *edit*, which is one page rather than the whole view.
 *
 * The assertions that matter are the negative ones. Every statement about the
 * page that was turned --- its box, its tiles, the turn it reports --- is also
 * true of a defect that turned the view instead, so what separates the two is a
 * neighbour that must not have moved and a `turns` option that must not have.
 */
describe("Scroller page turns", () => {
  let dom: FakeDom;
  let scroller: Scroller;

  /** Two portrait pages, so a turn on one is visible against the other. */
  function twoPages(): ScrollerOptions {
    return {
      ...options(),
      pageCount: 2,
      pages: [
        { width_pt: 600, height_pt: 800 },
        { width_pt: 600, height_pt: 800 },
      ],
    };
  }

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
    scroller = new Scroller(dom.root as unknown as HTMLElement, twoPages());
  });

  afterEach(() => {
    dom.restore();
  });

  it("reports no turn on a document nobody has edited", () => {
    expect(scroller.pageExtraTurns(0)).toBe(0);
    expect(scroller.pageExtraTurns(1)).toBe(0);
  });

  it("lays the turned page out sideways and leaves its neighbour alone", () => {
    const otherBefore = scroller.pageBoxCssOf(1);
    const turnedBefore = scroller.pageBoxCssOf(0);

    scroller.setPageTurns(0, 1);

    const turned = scroller.pageBoxCssOf(0);
    expect(turned.width / turned.height).toBeCloseTo(
      turnedBefore.height / turnedBefore.width,
      3,
    );
    expect(scroller.pageBoxCssOf(1)).toEqual(otherBefore);
  });

  it("normalises the turn, so a negative one is three quarters clockwise", () => {
    scroller.setPageTurns(0, -1);
    expect(scroller.pageExtraTurns(0)).toBe(3);
    scroller.setPageTurns(0, 5);
    expect(scroller.pageExtraTurns(0)).toBe(1);
  });

  it("asks the renderer for the page's turn composed with the view's", () => {
    scroller.setTurns(1);
    scroller.setPageTurns(1, 1);
    tiles.fetchTile.mockClear();
    scroller.frame(0, performance.now());

    const turns = new Map<number, number>();
    for (const [request] of tiles.fetchTile.mock.calls) {
      turns.set(request.page, request.turns);
    }
    // Page 0 carries the view's turn alone; page 1 carries both, reduced. A
    // request that sent five quarter-turns would be refused by the server
    // rather than reduced there, which is why this is normalised here.
    expect(turns.get(0)).toBe(1);
    expect(turns.get(1)).toBe(2);
  });

  it("does not touch the view's own rotation", () => {
    // The control for every assertion above: a `setPageTurns` implemented by
    // rotating the view would satisfy all of them on a one-page document, and
    // this is what it could not satisfy.
    const before = twoPages().turns;
    scroller.setPageTurns(0, 2);
    tiles.fetchTile.mockClear();
    scroller.frame(0, performance.now());
    const forOther = tiles.fetchTile.mock.calls
      .map(([request]) => request)
      .filter((request: { page: number }) => request.page === 1);
    expect(forOther.length).toBeGreaterThan(0);
    for (const request of forOther) expect(request.turns).toBe(before);
  });

  it("discards a page's pixels even when its box does not move", async () => {
    // A half turn leaves the box exactly as it was, so a geometry comparison
    // sees nothing move --- and the page is upside down. Written without this,
    // an implementation that let `applySizes` decide what to invalidate would
    // leave the old pixels on screen at 180 degrees and pass every other test
    // in this block.
    //
    // The tiles have to have *landed* first, which is the whole precondition:
    // a request still in flight is not re-issued while it is outstanding, so a
    // version of this that turned the page mid-flight would see no new request
    // and read as a defect in the invalidation rather than in the fixture.
    const closes: Array<() => void> = [];
    tiles.fetchTile.mockImplementation(() => {
      const bitmap = { close: vi.fn(), width: 64, height: 64 };
      closes.push(bitmap.close);
      return Promise.resolve({
        bitmap: bitmap as unknown as ImageBitmap,
        bytes: 1,
        renderUs: 1,
        decodeMs: 1,
      });
    });
    scroller.frame(0, performance.now());
    await Promise.resolve();
    await Promise.resolve();
    scroller.frame(0, performance.now());
    const landed = closes.length;
    expect(landed).toBeGreaterThan(0);

    const boxBefore = scroller.pageBoxCssOf(0);
    tiles.fetchTile.mockClear();

    scroller.setPageTurns(0, 2);
    scroller.frame(0, performance.now());

    expect(scroller.pageBoxCssOf(0)).toEqual(boxBefore);
    const asked = tiles.fetchTile.mock.calls
      .map(([request]) => request)
      .filter((request: { page: number }) => request.page === 0);
    expect(asked.length).toBeGreaterThan(0);
    for (const request of asked) expect(request.turns).toBe(2);
  });

  it("ignores a page that is not in the document", () => {
    expect(scroller.setPageTurns(-1, 1)).toBe(false);
    expect(scroller.setPageTurns(2, 1)).toBe(false);
    expect(scroller.pageExtraTurns(0)).toBe(0);
  });

  it("ignores a turn a page already has", () => {
    scroller.setPageTurns(0, 1);
    expect(scroller.setPageTurns(0, 1)).toBe(false);
  });
});

describe("Scroller when the page order changes", () => {
  let dom: FakeDom;
  let scroller: Scroller;

  /** Three pages of different heights, so a size carried to the wrong slot shows. */
  function threePages(): ScrollerOptions {
    return {
      ...options(),
      pageCount: 3,
      pages: [
        { width_pt: 600, height_pt: 800 },
        { width_pt: 600, height_pt: 400 },
        { width_pt: 600, height_pt: 1200 },
      ],
      order: [
        { id: 1, source: 0, turns: 0 },
        { id: 2, source: 1, turns: 0 },
        { id: 3, source: 2, turns: 0 },
      ],
    };
  }

  beforeEach(() => {
    dom = installFakeDom();
    tiles.fetchTile.mockReset();
    tiles.cancelTile.mockReset();
    let rid = 0;
    tiles.nextRequestId.mockImplementation(() => ++rid);
    tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
    scroller = new Scroller(dom.root as unknown as HTMLElement, threePages());
  });

  afterEach(() => {
    dom.restore();
  });

  it("asks for the page of the file a slot draws, not for the slot", () => {
    // The whole reason the order is here. Slot 1 draws page 2 of the file once
    // page 2 is deleted, and a request naming the slot asks for a picture of the
    // wrong page --- which looks like a rendering defect, not a bookkeeping one.
    scroller.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 3, source: 2, turns: 0 },
    ]);
    tiles.fetchTile.mockClear();
    scroller.frame(0, performance.now());

    const asked = tiles.fetchTile.mock.calls
      .map(([request]) => request as { page: number })
      .map((request) => request.page);
    expect(asked.length).toBeGreaterThan(0);
    expect(asked).not.toContain(1);
    expect(new Set(asked)).toEqual(new Set([0, 2]));
  });

  it("carries a learned size to wherever the page moved to", () => {
    // Sizes belong to the page, not to the position. Carried by slot instead,
    // every page below the gap is laid out at the size of the page that used to
    // be there --- invisible on a document whose pages are all the same size,
    // which is why this fixture's are not.
    const tallBefore = scroller.pageBoxCssOf(2);
    scroller.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 3, source: 2, turns: 0 },
    ]);

    const moved = scroller.pageBoxCssOf(1);
    expect(moved.height).toBeCloseTo(tallBefore.height, 5);
    expect(scroller.knowsPageSize(1)).toBe(true);
  });

  it("carries a page's own turn with it", () => {
    scroller.setPageTurns(2, 1);
    scroller.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 3, source: 2, turns: 1 },
    ]);
    expect(scroller.pageExtraTurns(1)).toBe(1);
    expect(scroller.pageExtraTurns(0)).toBe(0);
  });

  it("drops a tile that was rendering when the order changed", async () => {
    // Including one for a page that did **not** move, which is the half a check
    // of the deleted page cannot see. A render already finished when the order
    // changed still arrives; a withdrawal cannot reach it, and the slot it was
    // requested for may now hold another page.
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) => late.push(resolve as (value: unknown) => void)),
    );
    const held = new Scroller(dom.root as unknown as HTMLElement, threePages());
    held.frame(0, performance.now());

    const issued = tiles.fetchTile.mock.calls.map(
      ([request]) => request as { page: number; scale: number },
    );
    const unmoved = issued.findIndex(
      (request) => request.page === 0 && request.scale === 1,
    );
    expect(unmoved).toBeGreaterThanOrEqual(0);

    held.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 3, source: 2, turns: 0 },
    ]);

    const bitmap = { close: vi.fn(), width: 64, height: 64 };
    late[unmoved]?.({
      bitmap: bitmap as unknown as ImageBitmap,
      bytes: 1,
      renderUs: 1,
      decodeMs: 1,
    });
    await settle();
    held.frame(0, performance.now());
    expect(bitmap.close).toHaveBeenCalled();
    held.destroy();
  });

  it("keeps a tile that arrives while the order is unchanged", async () => {
    // The control, and it is what makes the test above able to fail: a scroller
    // that closed every arrival would pass that one perfectly while drawing
    // nothing at all.
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () =>
        new Promise((resolve) => late.push(resolve as (value: unknown) => void)),
    );
    const held = new Scroller(dom.root as unknown as HTMLElement, threePages());
    held.frame(0, performance.now());

    const issued = tiles.fetchTile.mock.calls.map(
      ([request]) => request as { page: number; scale: number },
    );
    const first = issued.findIndex(
      (request) => request.page === 0 && request.scale === 1,
    );
    expect(first).toBeGreaterThanOrEqual(0);

    const bitmap = { close: vi.fn(), width: 64, height: 64 };
    late[first]?.({
      bitmap: bitmap as unknown as ImageBitmap,
      bytes: 1,
      renderUs: 1,
      decodeMs: 1,
    });
    await settle();
    held.frame(0, performance.now());
    expect(bitmap.close).not.toHaveBeenCalled();
    held.destroy();
  });

  it("reports a document that got shorter", () => {
    const before = scroller.documentHeight;
    scroller.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 3, source: 2, turns: 0 },
    ]);
    expect(scroller.documentHeight).toBeLessThan(before);
    expect(scroller.pageBoxCssOf(2)).toEqual({ width: 0, height: 0 });
  });
});

describe("displayedSize", () => {
  const portrait = { width_pt: 612, height_pt: 792 };
  const landscape = { width_pt: 792, height_pt: 612 };

  it("leaves an upright page alone", () => {
    expect(displayedSize(portrait, 0)).toEqual(portrait);
  });

  it("swaps the axes on a quarter turn", () => {
    expect(displayedSize(portrait, 1)).toEqual(landscape);
    expect(displayedSize(portrait, 3)).toEqual(landscape);
  });

  it("leaves the axes alone on a half turn", () => {
    // The control: a defect that swapped on every non-zero rotation passes the
    // test above and fails here.
    expect(displayedSize(portrait, 2)).toEqual(portrait);
    expect(displayedSize(portrait, 4)).toEqual(portrait);
  });

  it("normalises a negative turn", () => {
    // `rotateBy(-1)` reaches here. Worth stating what this does *not* pin:
    // dropping one of the two `% 4` reductions passes every test here, because
    // JavaScript's remainder keeps the sign and a parity test is unmoved by it
    // --- the mutation is an identity, not a surviving defect. What these three
    // catch is a swap keyed on the turn's value rather than its parity, which
    // is the form the arithmetic actually invites.
    expect(displayedSize(portrait, -1)).toEqual(landscape);
    expect(displayedSize(portrait, -2)).toEqual(portrait);
    expect(displayedSize(portrait, -3)).toEqual(landscape);
  });
});
