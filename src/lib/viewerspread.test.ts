/**
 * Facing pages and book view in the viewer: the fit, the place a reader is
 * kept at, next and previous page, and which page a press lands on.
 *
 * `scrollerspread.test.ts` owns where each page goes. This owns what the
 * viewer does with that: the row of two is one sheet to fit, a step is a row,
 * and a hit test has to ask which page of a row the pointer is over.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type MarkView } from "./pages";
import type { Spread } from "./scroller";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer, type ViewerStatus } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

const FACING: Spread = { columns: 2, coverAlone: false, rightToLeft: false };
const BOOK: Spread = { columns: 2, coverAlone: true, rightToLeft: false };

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
  core.invoke.mockResolvedValue(null);
  tiles.fetchTile.mockReturnValue(new Promise(() => {}));
  tiles.nextRequestId.mockReturnValue(1);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

/** A box on page `page` (one-based), near its top-left. */
function box(id: number, page: number): MarkView {
  return {
    id, kind: "square", stamp: null, page: pageId(page), quads: [40, 40, 140, 90], strokes: [],
    color: [0.85, 0.15, 0.15], width: 1, note: "", lines: [],
  };
}

/**
 * Twelve 300 x 400 pt pages at fit width, with a box on each of the first two.
 * Twelve so that every row a test steps to can reach the top of the window:
 * near the end of a document the view stops at the end instead.
 */
function build(onStatus?: (status: ViewerStatus) => void): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    ...(onStatus ? { onStatus } : {}),
    doc: 1,
    pageCount: 12,
    pages: Array.from({ length: 12 }, () => ({ width_pt: 300, height_pt: 400 })) as [
      { width_pt: number; height_pt: number },
    ],
  });
  viewer.setMarks([box(1, 1), box(2, 2)]);
  viewer.setFit("width");
  return viewer;
}

/** Presses the middle of a mark, as a reader picking it does. */
function press(viewer: Viewer, id: number): void {
  const anchor = viewer.markAnchor(id);
  if (!anchor) throw new Error(`mark ${id} is not laid out`);
  const at = { clientX: (anchor.left + anchor.right) / 2, clientY: (anchor.top + anchor.bottom) / 2 };
  dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, timeStamp: 5000 + id * 1000, ...at });
  dom.root.dispatch("pointerup", { pointerId: 1, ...at });
}

describe("Facing pages in the viewer", () => {
  it("fits the row of two to the width, less the gap between them", () => {
    const viewer = build();
    const single = viewer.currentZoom;
    viewer.setSpread(FACING);
    // Both fill the same width: one page of 300 pt, or two and a 16 px gap.
    expect(viewer.currentZoom * 600 + 16).toBeCloseTo(single * 300, 6);
  });

  it("keeps the reader on the page they were reading", () => {
    const viewer = build();
    viewer.goToPage(4);
    expect(viewer.position.page).toBe(4);
    viewer.setSpread(FACING);
    expect(viewer.position.page).toBe(4);
    viewer.setSpread(BOOK);
    // Under book view the fifth page shares a row with the fourth, and the
    // first page of that row is the one at the top.
    expect(viewer.position.page).toBe(3);
  });

  it("steps a row at a time with next and previous page", () => {
    const viewer = build();
    viewer.setSpread(FACING);
    viewer.nextPage();
    expect(viewer.position.page).toBe(2);
    viewer.nextPage();
    expect(viewer.position.page).toBe(4);
    viewer.previousPage();
    expect(viewer.position.page).toBe(2);
    viewer.previousPage();
    expect(viewer.position.page).toBe(0);
  });

  it("steps from the cover alone to the first pair under book view", () => {
    const viewer = build();
    viewer.setSpread(BOOK);
    viewer.nextPage();
    expect(viewer.position.page).toBe(1);
    viewer.nextPage();
    expect(viewer.position.page).toBe(3);
    viewer.previousPage();
    expect(viewer.position.page).toBe(1);
    viewer.previousPage();
    expect(viewer.position.page).toBe(0);
  });

  it("picks a mark on either page of a row", async () => {
    const viewer = build();
    viewer.setSpread(FACING);
    await settle();
    press(viewer, 1);
    expect(viewer.pickedMarks()).toEqual([1]);
    press(viewer, 2);
    expect(viewer.pickedMarks()).toEqual([2]);
  });

  it("restores the layout a place names, and one page to a row for one naming none", () => {
    const viewer = build();
    viewer.restore({ page: 2, top_pt: 0, zoom: 1, fit: "width", turns: 0, spread: BOOK });
    expect(viewer.spread).toEqual(BOOK);
    // Under book view the third page shares a row with the second.
    expect(viewer.position.page).toBe(1);
    viewer.restore({ page: 2, top_pt: 0, zoom: 1, fit: "width", turns: 0 });
    expect(viewer.spread.columns).toBe(1);
    expect(viewer.position.page).toBe(2);
  });

  it("reports a change of layout when nothing else moved", async () => {
    // The status is what the window writes the place from, and it is sent only
    // when its summary changes. A fixed zoom and the first row keep everything
    // else where it was, so only the layout can make this one arrive.
    const statuses: ViewerStatus[] = [];
    const viewer = build((status) => statuses.push(status));
    viewer.setZoomFixed(1);
    for (let round = 0; round < 5; round++) {
      dom.runFrames();
      await settle();
    }
    expect(statuses.at(-1)?.spread.columns).toBe(1);
    viewer.setSpread(FACING);
    for (let round = 0; round < 5; round++) {
      dom.runFrames();
      await settle();
    }
    expect(statuses.at(-1)?.spread).toEqual(FACING);
  });

  it("goes back to one page to a row", () => {
    const viewer = build();
    viewer.setSpread(FACING);
    viewer.setSpread({ columns: 1, coverAlone: false, rightToLeft: false });
    viewer.nextPage();
    expect(viewer.position.page).toBe(1);
  });
});
