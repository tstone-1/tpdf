/**
 * A place a list asks the viewer to show: the ring over a passage of text the
 * page does not show.
 *
 * The ring is all a reader gets for such a passage, since nothing on the page
 * marks it. So what is held is that it is drawn where the page's own
 * coordinates say the rectangle is --- asked of `screenPoint`, which reaches
 * the screen by another route than the ring's --- and that it follows its page
 * when the pages above it change.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
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
  // Frames are run here, and a frame asks for tiles: none ever arrives.
  tiles.fetchTile.mockImplementation(() => new Promise(() => {}));
  tiles.nextRequestId.mockImplementation(() => 1);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

const RECT: [number, number, number, number] = [100, 300, 260, 314];

/** Four pages of the file, ids 1 to 4 in slots 0 to 3. */
const PAGES: PageView[] = [1, 2, 3, 4].map((id, at) => ({
  id: pageId(id),
  source: { baseline: at },
  turns: 0,
}));

function build(pages: PageView[] = PAGES): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 4,
    pages: [{ width_pt: 600, height_pt: 800 }],
  });
  viewer.setPages(pages);
  return viewer;
}

/** Where `RECT` is on screen for the page in `slot`, by the page's own route. */
function expected(viewer: Viewer, slot: number) {
  const from = viewer.screenPoint(slot, RECT[0], RECT[1]);
  const to = viewer.screenPoint(slot, RECT[2], RECT[3]);
  return {
    left: Math.round(from.x),
    top: Math.round(from.y),
    width: Math.round(to.x - from.x),
    height: Math.round(to.y - from.y),
  };
}

describe("a place a list asks to be shown", () => {
  it("is scrolled to and ringed where the page has it", async () => {
    const viewer = build();
    await settle();
    expect(viewer.regionRingBox).toBeNull();
    expect(viewer.position.page).toBe(0);

    expect(viewer.showRegion(2, RECT)).toBe(true);
    await settle();
    expect(viewer.position.page).toBe(2);
    const box = viewer.regionRingBox;
    expect(box).toEqual(expected(viewer, 2));
    // And it is a rectangle of the passage's size, not a point: 160 by 14
    // points at whatever the zoom is.
    expect(box!.width / box!.height).toBeCloseTo(160 / 14, 0);
    expect(box!.width).toBeGreaterThan(20);
    // On screen: the jump went to the passage and not only to its page.
    expect(box!.top).toBeGreaterThanOrEqual(0);
    expect(box!.top).toBeLessThan(200);
    // A jump Back undoes, like every other jump from a list.
    expect(viewer.canGoBack).toBe(true);
    viewer.destroy();
  });

  it("stays on its page when a page above it is deleted, and goes with its own", async () => {
    const viewer = build();
    await settle();
    viewer.showRegion(2, RECT);
    await settle();

    // Page id 1 deleted: the page the ring is on moves from slot 2 to slot 1.
    viewer.setPages(PAGES.filter((page) => page.id !== pageId(1)));
    viewer.wake();
    dom.runFrames();
    expect(viewer.regionRingBox).toEqual(expected(viewer, 1));
    // The control: slot 2 now holds another page, somewhere else on screen.
    expect(viewer.regionRingBox).not.toEqual(expected(viewer, 2));

    // Its own page deleted: there is nothing left to ring.
    viewer.setPages(PAGES.filter((page) => page.id !== pageId(1) && page.id !== pageId(3)));
    viewer.wake();
    dom.runFrames();
    expect(viewer.regionRingBox).toBeNull();
    viewer.destroy();
  });

  it("is taken away when asked, and does not come back on the next frame", async () => {
    const viewer = build();
    await settle();
    viewer.showRegion(1, RECT);
    await settle();
    expect(viewer.regionRingBox).not.toBeNull();

    viewer.clearRegion();
    expect(viewer.regionRingBox).toBeNull();
    viewer.wake();
    dom.runFrames();
    expect(viewer.regionRingBox).toBeNull();
    viewer.destroy();
  });

  it("refuses a slot that holds no page, and moves nothing", async () => {
    const viewer = build();
    await settle();
    expect(viewer.showRegion(9, RECT)).toBe(false);
    await settle();
    expect(viewer.regionRingBox).toBeNull();
    expect(viewer.position.page).toBe(0);
    expect(viewer.canGoBack).toBe(false);
    viewer.destroy();
  });
});
