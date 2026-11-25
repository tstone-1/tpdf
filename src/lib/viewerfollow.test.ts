/**
 * A viewer led by another: where it says it is, and being moved there.
 *
 * `syncscroll.ts` owns the offset between two documents. This owns the two
 * things it needs of a viewer: a place that means the same at another zoom,
 * and a move that is drawn in the frame it is asked for.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SyncFollow } from "./syncfollow";
import { ScrollLock } from "./syncscroll";
import { FakeElement, installFakeDom, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
  core.invoke.mockResolvedValue(null);
  tiles.fetchTile.mockResolvedValue(null);
  tiles.nextRequestId.mockReturnValue(1);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

const PAGES = 6;
const SIZE = { width_pt: 600, height_pt: 800 };

/** Six 600x800 pt pages, and every position the viewer reported. */
function build(zoom = 1, root = dom.root) {
  const reported: number[] = [];
  const viewer = new Viewer(root as unknown as HTMLElement, {
    doc: 1,
    pageCount: PAGES,
    pages: [SIZE, ...Array.from({ length: PAGES - 1 }, () => SIZE)],
    onPosition: (page) => { reported.push(page); },
  });
  viewer.setZoomFixed(zoom);
  dom.runFrames();
  reported.length = 0;
  return { viewer, reported };
}

describe("where a viewer says it is", () => {
  it("is the top of the first page to begin with", () => {
    expect(build().viewer.reading).toEqual({ page: 0, fraction: 0 });
  });

  it("is the top of a page after a jump to it", () => {
    const { viewer } = build();
    viewer.goToPage(2);
    expect(viewer.reading).toEqual({ page: 2, fraction: 0 });
  });
});

describe("a viewer moved by another", () => {
  it("arrives at the page and the share of it that was asked for", () => {
    const { viewer } = build();
    viewer.followTo(3, 0.5);
    expect(viewer.reading.page).toBe(3);
    expect(viewer.reading.fraction).toBeCloseTo(0.5, 6);
  });

  it.each([
    { coverAlone: false, first: 2 },
    { coverAlone: true, first: 1 },
  ])("scrolls forward and back through a paired row with coverAlone=$coverAlone", ({ coverAlone, first }) => {
    const single = build().viewer;
    const root = new FakeElement("div");
    root.clientWidth = dom.root.clientWidth;
    root.clientHeight = dom.root.clientHeight;
    const paired = build(1, root).viewer;
    paired.setSpread({ columns: 2, coverAlone, continuous: true, rightToLeft: false });
    paired.goToPage(first + 2);
    const nextRow = paired.offset;
    paired.goToPage(first);
    const rowTop = paired.offset;
    const pitch = nextRow - rowTop;
    expect(pitch).toBeGreaterThan(0);
    single.goToPage(first);
    const follow = new SyncFollow<number, Viewer>(new ScrollLock());
    follow.hold({ id: 1, viewer: single }, { id: 2, viewer: paired });
    single.followTo(first, 0.9);
    follow.step(1, single, false);
    expect(paired.offset).toBeCloseTo(rowTop + 0.45 * pitch);
    single.goToPage(first + 1);
    follow.step(1, single, false);
    expect(paired.offset).toBeCloseTo(rowTop + 0.5 * pitch);
    // The paired viewer now leads backwards by a quarter of its physical row.
    root.dispatch("wheel", { deltaX: 0, deltaY: -pitch / 4, deltaMode: 0, preventDefault() {} });
    follow.step(2, paired, false);
    expect(single.reading).toEqual({ page: first, fraction: 0.5 });
  });

  it("arrives at the same share of the page at another zoom", () => {
    const { viewer } = build(2);
    viewer.followTo(3, 0.5);
    expect(viewer.reading.page).toBe(3);
    expect(viewer.reading.fraction).toBeCloseTo(0.5, 6);
  });

  it("draws the move before returning, with no animation frame run", () => {
    const { viewer, reported } = build();
    viewer.followTo(2, 0.25);
    expect(reported).toEqual([2]);
  });

  it("draws nothing for the place it is already in", () => {
    const { viewer, reported } = build();
    viewer.followTo(2, 0.25);
    reported.length = 0;
    viewer.followTo(2, 0.25);
    expect(reported).toEqual([]);
  });

  it("records nothing for Back, where a jump does", () => {
    const { viewer } = build();
    viewer.followTo(4, 0);
    expect(viewer.canGoBack).toBe(false);
    viewer.goToDestination(1, 0);
    expect(viewer.canGoBack).toBe(true);
  });

  it("stops at the end for a page past the last one", () => {
    const { viewer } = build();
    viewer.followTo(PAGES + 3, 0.4);
    const end = viewer.reading;
    viewer.goToEnd();
    expect(viewer.reading).toEqual(end);
    expect(end.page).toBe(PAGES - 1);
  });

  it("holds a share outside the page to the page", () => {
    const { viewer } = build();
    viewer.followTo(1, -3);
    expect(viewer.reading).toEqual({ page: 1, fraction: 0 });
  });

  it("does nothing once destroyed", () => {
    const { viewer, reported } = build();
    viewer.destroy();
    viewer.followTo(3, 0.5);
    expect(reported).toEqual([]);
  });
});
