/**
 * Facing pages and book view: which pages share a row, and where each sits.
 *
 * Read off {@link Scroller.pageOrigin}, which is what tiles, marks, links and
 * the hit tests all place themselves from, so a number here is a number on
 * screen. The pages are 300 x 400 pt at zoom 1 in a window 900 px wide: a row
 * of two is 616 px (300, the 16 px gap, 300), centred at 142 px.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { rowsOf, Scroller, SINGLE, type PageSize, type ScrollerOptions, type Spread } from "./scroller";
import { installFakeDom, type FakeDom } from "./testdom";

const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("./tiles", () => tiles);

const FACING: Spread = { columns: 2, coverAlone: false, rightToLeft: false, continuous: true };
const BOOK: Spread = { columns: 2, coverAlone: true, rightToLeft: false, continuous: true };
const PAGE: PageSize = { width_pt: 300, height_pt: 400 };

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
  tiles.fetchTile.mockReturnValue(new Promise(() => {}));
  tiles.nextRequestId.mockReturnValue(1);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

function build(spread: Spread, pages: PageSize[] = [PAGE, PAGE, PAGE, PAGE, PAGE], width = 900): Scroller {
  const options: ScrollerOptions = {
    doc: 1,
    pageCount: pages.length,
    pages: pages as [PageSize, ...PageSize[]],
    zoom: 1,
    turns: 0,
    invert: false,
    spread,
    layout: "viewport",
    tilePx: 4096,
    dpr: 1,
    viewport: { width, height: 900 },
    prefetchScreens: 0,
    cacheTiles: 48,
    maxInFlight: 4,
    cancel: false,
  };
  return new Scroller(dom.root as unknown as HTMLElement, options);
}

/** Every page's `[left, top]` on screen. */
function origins(scroller: Scroller, count = 5): [number, number][] {
  return Array.from({ length: count }, (_, page) => {
    const { left, top } = scroller.pageOrigin(page);
    return [left, top];
  });
}

describe("rowsOf", () => {
  it("puts one page in each row, two facing, or the cover alone and then pairs", () => {
    expect(rowsOf(3, SINGLE)).toEqual([[0], [1], [2]]);
    expect(rowsOf(5, FACING)).toEqual([[0, 1], [2, 3], [4]]);
    expect(rowsOf(5, BOOK)).toEqual([[0], [1, 2], [3, 4]]);
    expect(rowsOf(0, BOOK)).toEqual([]);
  });
});

describe("Pages beside each other", () => {
  it("lays out one page to a row, centred, as before", () => {
    // The control for every test below: the same pages, one to a row.
    expect(origins(build(SINGLE), 3)).toEqual([
      [300, 0],
      [300, 416],
      [300, 832],
    ]);
  });

  it("puts two pages side by side under facing, centred as a pair", () => {
    expect(origins(build(FACING))).toEqual([
      [142, 0],
      [458, 0],
      [142, 416],
      [458, 416],
      // The last page has no partner and sits on the left, where it would in a book.
      [142, 832],
    ]);
  });

  it("puts the cover alone on the right under book view", () => {
    expect(origins(build(BOOK))).toEqual([
      [458, 0],
      [142, 416],
      [458, 416],
      [142, 832],
      [458, 832],
    ]);
  });

  it("reads a row from the right with right to left", () => {
    expect(origins(build({ ...FACING, rightToLeft: true }), 3)).toEqual([
      [458, 0],
      [142, 0],
      [458, 416],
    ]);
  });

  it("makes a row as tall as its taller page", () => {
    const tall: PageSize = { width_pt: 300, height_pt: 600 };
    const scroller = build(FACING, [PAGE, tall, PAGE]);
    expect(scroller.pageOrigin(2).top).toBe(616);
    expect(scroller.pagePitchOf(0)).toBe(616);
  });

  it("pans as far as the row is wider than the window", () => {
    expect(build(FACING, undefined, 500).maxPan).toBe(116);
    // The control: one page to a row is narrower than the window.
    expect(build(SINGLE, undefined, 500).maxPan).toBe(0);
  });
});

describe("Finding the page under a point", () => {
  it("answers a height with the first page of its row", () => {
    const scroller = build(FACING);
    expect(scroller.pageAt(10)).toBe(0);
    expect(scroller.pageAt(420)).toBe(2);
    expect(scroller.pageAt(900)).toBe(4);
  });

  it("picks the page of the row the point is over", () => {
    const scroller = build(FACING);
    expect(scroller.pageAtPoint(200, 10)).toBe(0);
    expect(scroller.pageAtPoint(500, 10)).toBe(1);
    expect(scroller.pageAtPoint(500, 420)).toBe(3);
    // In the gap, nearer the right-hand page.
    expect(scroller.pageAtPoint(455, 10)).toBe(1);
    // Beyond the row on either side.
    expect(scroller.pageAtPoint(5, 10)).toBe(0);
    expect(scroller.pageAtPoint(890, 10)).toBe(1);
  });

  it("finds the right-hand page of a row read from the right on the left", () => {
    const scroller = build({ ...FACING, rightToLeft: true });
    expect(scroller.pageAtPoint(200, 10)).toBe(1);
    expect(scroller.pageAtPoint(500, 10)).toBe(0);
  });

  it("answers by height alone with one page to a row", () => {
    const scroller = build(SINGLE);
    expect(scroller.pageAtPoint(5, 420)).toBe(1);
  });
});

describe("Changing the spread", () => {
  it("relays out on a change and reports none for the same spread", () => {
    const scroller = build(SINGLE);
    const tall = scroller.documentHeight;
    expect(scroller.setSpread(SINGLE)).toBe(false);
    expect(scroller.setSpread(FACING)).toBe(true);
    expect(scroller.spread).toEqual(FACING);
    expect(scroller.documentHeight).toBeLessThan(tall);
    expect(scroller.pageOrigin(1).top).toBe(0);
    expect(scroller.setSpread({ ...FACING, coverAlone: true })).toBe(true);
    expect(scroller.pageOrigin(1).top).toBe(416);
  });
});

describe("One row at a time", () => {
  const ONE_AT_A_TIME: Spread = { ...SINGLE, continuous: false };

  it("gives each row a slot one window tall, so no other row is in view", () => {
    // The window is 900 px tall and the pages 400: each slot is the window
    // and the gap.
    expect(origins(build(ONE_AT_A_TIME), 3)).toEqual([
      [300, 0],
      [300, 916],
      [300, 1832],
    ]);
    expect(build(ONE_AT_A_TIME).rowSpan(950)).toEqual({ top: 916, bottom: 916 });
  });

  it("scrolls within a row as far as it is taller than the window", () => {
    const tall: PageSize = { width_pt: 300, height_pt: 1200 };
    const scroller = build(ONE_AT_A_TIME, [tall, tall]);
    expect(scroller.pageOrigin(1).top).toBe(1216);
    expect(scroller.rowSpan(10)).toEqual({ top: 0, bottom: 300 });
  });

  it("sizes the slots again when the window changes height", () => {
    const scroller = build(ONE_AT_A_TIME);
    scroller.resize({ width: 900, height: 600 });
    expect(scroller.pageOrigin(1).top).toBe(616);
  });
});
