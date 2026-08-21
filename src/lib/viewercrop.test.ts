/**
 * Where a rectangle lands once the reader has cropped the page.
 *
 * The differential this repository has learnt to write: a comment, a link and
 * one of the reader's own marks all arrive measured from the **file's**
 * displayed corner, and all three must be drawn at the same place on a cropped
 * page. One subsystem left out of the crop is a mark that no longer sits on its
 * words --- and it looks like a rendering bug rather than a missing translation,
 * which is exactly the failure the page-turn increment already paid for once.
 *
 * The control beside it is that the placement must **move** when the crop is
 * applied. Three subsystems agreeing about an unchanged number agree by
 * construction; what says the crop is honoured is that the agreed number is not
 * the uncropped one.
 *
 * Every test below was checked by mutating `viewer.ts`, `crop.ts` or
 * `scroller.ts` and confirming it went red; `scripts/mutate_frontend.py` holds
 * the mutations.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Comment } from "./comments";
import type { CropGeometry } from "./crop";
import type { MarkView, PageView } from "./pages";
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

/** The page's own box, and the crop the reader takes out of the middle of it. */
const PAGE = { width_pt: 600, height_pt: 800 };
const CROP: readonly [number, number, number, number] = [100, 150, 500, 650];
/**
 * What the backend answers for {@link CROP}.
 *
 * Written out rather than derived, because deriving it here would be a second
 * implementation of the arithmetic `render.rs` does --- and one that agreed with
 * a wrong one. On an unrotated 600x800 page the crop is 400x500 and its top-left
 * sits 100 points in and 150 down from the file's own corner.
 */
const GEOMETRY: CropGeometry = { width_pt: 400, height_pt: 500, left: 100, top: 150 };

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
  core.invoke.mockImplementation((command: string) =>
    command === "page_geometry"
      ? Promise.resolve(GEOMETRY)
      : Promise.resolve(null),
  );
  tiles.fetchTile.mockRejectedValue(new Error("no tile"));
  let rid = 0;
  tiles.nextRequestId.mockImplementation(() => ++rid);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

function build(): Viewer {
  return new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 2,
    pages: [PAGE],
  });
}

/** The pages of a two-page document, with page 1 cropped or not. */
function pages(crop: boolean): PageView[] {
  return [
    crop ? { id: 1, source: 0, turns: 0, crop: CROP } : { id: 1, source: 0, turns: 0 },
    { id: 2, source: 1, turns: 0 },
  ];
}

/** One comment on page 0, at a rectangle in the file's display space. */
function comment(rect: [number, number, number, number]): Comment {
  return {
    id: 1,
    page: 0,
    kind: "text",
    author: "",
    body: "",
    subject: "",
    date: null,
    rect,
    quads: [],
    reply_to: null,
    hidden: false,
  };
}

/** One mark on page id 1, over the same rectangle. */
function mark(rect: [number, number, number, number]): MarkView {
  return {
    id: 10,
    kind: "highlight",
    page: 1,
    quads: [...rect],
    strokes: [],
    color: [1, 0.9, 0.2],
    note: "",
    lines: [],
  };
}

/** What the viewer keeps private, for reading a placement back. */
interface Placing {
  anchorFor(comment: Comment): { left: number; top: number };
  anchorForMark(mark: MarkView): { left: number; top: number } | null;
  viewQuadsOf(mark: MarkView): { slot: number; quads: { left: number; top: number }[] } | null;
  cropAt(page: number): CropGeometry;
}

/**
 * Where a comment and a mark over one rectangle each say they are.
 *
 * `note` and `anchor` are both in the **window**, and they reach it by different
 * routes --- `anchorOn` for a comment, `anchorForMark` for a mark --- which is
 * what makes comparing them a differential rather than a tautology. `drawn` is
 * the same mark in the page's own points, which is where a movement is visible:
 * a window coordinate also moves when the page is re-centred, and on a crop
 * inset from the left by half its width reduction the two cancel exactly.
 */
function placed(viewer: Viewer, rect: [number, number, number, number]) {
  const inner = viewer as unknown as Placing;
  return {
    note: inner.anchorFor(comment(rect)),
    lines: [],
    anchor: inner.anchorForMark(mark(rect)),
    drawn: inner.viewQuadsOf(mark(rect))?.quads[0],
  };
}

describe("a cropped page", () => {
  it("places a comment and a mark at the same point, and not where they were", async () => {
    const rect: [number, number, number, number] = [200, 300, 260, 320];
    const viewer = build();
    await settle();

    viewer.setPages(pages(false));
    await settle();
    const before = placed(viewer, rect);

    viewer.setPages(pages(true));
    await settle();
    const after = placed(viewer, rect);

    // Both subsystems moved by the crop's corner, and by the same amount.
    expect(after.drawn?.left).toBeCloseTo(rect[0] - GEOMETRY.left, 5);
    expect(after.drawn?.top).toBeCloseTo(rect[1] - GEOMETRY.top, 5);
    // The differential: the comment subsystem and the mark subsystem put one
    // rectangle in one place. Two routes to the window, and a crop applied by
    // only one of them separates them.
    expect(after.anchor?.left).toBeCloseTo(after.note.left, 5);
    expect(after.anchor?.top).toBeCloseTo(after.note.top, 5);
    // The control: the placement moved at all. Three subsystems agreeing about
    // an unchanged number agree by construction.
    expect(after.drawn?.left).not.toBeCloseTo(before.drawn?.left ?? 0, 5);
    viewer.destroy();
  });

  it("leaves an uncropped page alone", async () => {
    // The other control, and the one that catches a crop applied to every page:
    // page 1 of this document is not cropped and its rectangles must not move.
    const viewer = build();
    await settle();
    viewer.setPages(pages(true));
    await settle();

    const inner = viewer as unknown as Placing;
    // The offset, not the size: an uncropped page whose size nothing has learnt
    // is laid out from the scroller's estimate, which the cropped page beside it
    // moves. What must be zero is the shift, since that is what places every
    // rectangle on it.
    expect(inner.cropAt(1).left).toBe(0);
    expect(inner.cropAt(1).top).toBe(0);
    expect(inner.cropAt(0).left).toBe(GEOMETRY.left);
    viewer.destroy();
  });

  it("takes the geometry back off when the crop is cleared", async () => {
    // A crop the reader undoes. Without this the map keeps the old geometry and
    // every rectangle stays shifted on a page that is no longer cropped --- the
    // failure that looks least like the thing that caused it.
    const viewer = build();
    await settle();
    viewer.setPages(pages(true));
    await settle();
    expect((viewer as unknown as Placing).cropAt(0).left).toBe(GEOMETRY.left);

    viewer.setPages(pages(false));
    await settle();
    expect((viewer as unknown as Placing).cropAt(0).left).toBe(0);
    viewer.destroy();
  });

  it("lays the page out at the size the backend reported", async () => {
    const viewer = build();
    await settle();
    viewer.setPages(pages(true));
    await settle();
    // Not the crop rectangle's own width: the two agree on an unrotated page and
    // differ at every quarter turn, and this is the one the layout must use.
    expect(viewer.pageSizeOf(0)).toEqual({
      width_pt: GEOMETRY.width_pt,
      height_pt: GEOMETRY.height_pt,
    });
    viewer.destroy();
  });
});
