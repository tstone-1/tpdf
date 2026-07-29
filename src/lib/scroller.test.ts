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

import { displayedSize, Scroller, type ScrollerOptions } from "./scroller";
import { installFakeDom, settle, type FakeDom } from "./testdom";

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
    page: { width_pt: 600, height_pt: 800 },
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

describe("Scroller teardown", () => {
  let dom: FakeDom;
  let scroller: Scroller;

  /** A tile reply whose bitmap says whether anyone released it. */
  function delivery() {
    const bitmap = { close: vi.fn(), width: 64, height: 64 };
    return {
      bitmap: bitmap as unknown as ImageBitmap,
      close: bitmap.close,
      result: { bitmap: bitmap as unknown as ImageBitmap, bytes: 1, renderUs: 1, decodeMs: 1 },
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
      () => new Promise((resolve) => late.push(resolve as (value: unknown) => void)),
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
    for (const arrival of arrivals) expect(arrival.close).not.toHaveBeenCalled();

    late[0]?.(arrivals[0]!.result);
    late[1]?.(arrivals[1]!.result);
    await settle();
    for (const arrival of arrivals) expect(arrival.close).toHaveBeenCalledTimes(1);
  });

  it("still keeps a tile that lands while it is alive", async () => {
    // The control for the test above, and not a formality: a scroller that
    // closed every arrival would pass that one perfectly while drawing nothing.
    const late: Array<(value: unknown) => void> = [];
    tiles.fetchTile.mockImplementation(
      () => new Promise((resolve) => late.push(resolve as (value: unknown) => void)),
    );

    scroller.frame(0, performance.now());
    const tile = delivery();
    late[0]?.(tile.result);
    await settle();

    expect(tile.close).not.toHaveBeenCalled();
    expect(scroller.stats.bytes).toBe(1);
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
