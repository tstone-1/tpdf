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
import { pageId, type MarkView, type PageView } from "./pages";
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
  cropped = [];
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
    onCropped: (page, rect) => cropped.push({ page, rect }),
  });
}

/** Every crop the reader dragged out, in order. */
let cropped: { page: number; rect: [number, number, number, number] }[] = [];

/** Presses, drags and releases, in the root's client coordinates. */
function drag(
  from: { x: number; y: number },
  to: { x: number; y: number },
  release = true,
): void {
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: from.x,
    clientY: from.y,
    target: dom.root,
  });
  dom.root.dispatch("pointermove", { pointerId: 1, clientX: to.x, clientY: to.y });
  if (release) {
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: to.x, clientY: to.y });
  }
}

/** An Escape, shaped the way `matches` reads a key event. */
function escape(): void {
  dom.root.dispatch("keydown", {
    key: "Escape",
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    target: dom.root,
  });
}

/** The pages of a two-page document, with page 1 cropped or not. */
function pages(crop: boolean): PageView[] {
  return [
    crop ? { id: pageId(1), source: 0, turns: 0, crop: CROP } : { id: pageId(1), source: 0, turns: 0 },
    { id: pageId(2), source: 1, turns: 0 },
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
    stamp: null,
    page: pageId(1),
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

describe("cropping by dragging", () => {
  it("reports nothing until the tool is armed", async () => {
    // The control for every test below. A viewer that reported a crop on any
    // drag would pass all of them, and this is the only assertion saying that
    // arming is what decides --- which matters more here than for a mark,
    // because a stray crop removes something the reader can see.
    const viewer = build();
    await settle();
    viewer.setPages(pages(false));
    await settle();

    drag({ x: 120, y: 140 }, { x: 320, y: 340 });

    expect(cropped).toEqual([]);
    expect(viewer.cropArmed).toBe(false);
    viewer.destroy();
  });

  it("is spent by one rectangle", async () => {
    const viewer = build();
    await settle();
    viewer.setPages(pages(false));
    await settle();

    viewer.armCrop();
    expect(viewer.cropArmed).toBe(true);
    drag({ x: 120, y: 140 }, { x: 320, y: 340 });
    expect(viewer.cropArmed).toBe(false);

    drag({ x: 130, y: 150 }, { x: 330, y: 350 });
    expect(cropped).toHaveLength(1);
    viewer.destroy();
  });

  it("is dropped by Escape mid-drag, without cropping", async () => {
    const viewer = build();
    await settle();
    viewer.setPages(pages(false));
    await settle();

    viewer.armCrop();
    drag({ x: 120, y: 140 }, { x: 320, y: 340 }, false);
    expect(viewer.cropPreview).not.toBe(null);

    escape();
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 320, clientY: 340 });

    expect(cropped).toEqual([]);
    expect(viewer.cropPreview).toBe(null);
    expect(viewer.cropArmed).toBe(false);
    viewer.destroy();
  });

  it("keeps the tool armed when the reader clicks instead of dragging", async () => {
    // A press that never moved is a reader who has not started, not a crop of
    // no size. Spending the tool here would cost them the command with nothing
    // on screen to say why --- `boxQuad`'s `MIN_BOX` is what decides.
    const viewer = build();
    await settle();
    viewer.setPages(pages(false));
    await settle();

    viewer.armCrop();
    drag({ x: 120, y: 140 }, { x: 121, y: 141 });

    expect(cropped).toEqual([]);
    expect(viewer.cropArmed).toBe(true);
    viewer.destroy();
  });

  it("reports the rectangle in the file's space and not the page's", async () => {
    // **The test this gesture exists to pass.** A crop is dragged out on the
    // page the reader can see, which on an already-cropped page is inset from
    // the file's own corner by `GEOMETRY.left`/`top` --- and the crop box the
    // model holds is absolute. Reporting the laid-out rectangle gives a crop
    // that walks further into the page every time it is used, which on an
    // uncropped page is indistinguishable from the right answer.
    //
    // **Read against the preview, which is a second observable of the same
    // gesture.** The preview comes off `cropDrawing` untouched and the report
    // goes through `fileRectOn`, so neither is derived from the other; comparing
    // them says the translation happened, and comparing the report against a
    // number written here would only say it equals a number written here.
    const viewer = build();
    await settle();
    viewer.setPages(pages(true));
    await settle();

    viewer.armCrop();
    drag({ x: 200, y: 100 }, { x: 500, y: 400 }, false);
    const preview = viewer.cropPreview;
    expect(preview).not.toBe(null);
    const laidLeft = Math.min(preview?.from.x ?? 0, preview?.to.x ?? 0);
    const laidTop = Math.min(preview?.from.y ?? 0, preview?.to.y ?? 0);
    const laidRight = Math.max(preview?.from.x ?? 0, preview?.to.x ?? 0);
    const laidBottom = Math.max(preview?.from.y ?? 0, preview?.to.y ?? 0);
    // **A precondition, asserted rather than assumed.** These client
    // coordinates are chosen so that the drag lands wholly inside the laid-out
    // page and `boxQuad`'s clamp does not fire --- a clamped corner would make
    // the comparison below about the clamp instead of about the translation.
    // Where the page sits on screen is the fake DOM's business and could move,
    // so this fails loudly and says which half went wrong rather than turning
    // the real assertion into a statement about clamping.
    expect(laidLeft).toBeGreaterThanOrEqual(0);
    expect(laidTop).toBeGreaterThanOrEqual(0);
    expect(laidRight).toBeLessThanOrEqual(GEOMETRY.width_pt);
    expect(laidBottom).toBeLessThanOrEqual(GEOMETRY.height_pt);
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 500, clientY: 400 });

    const onCropped = cropped.at(-1);
    expect(onCropped).toBeDefined();
    expect(onCropped?.rect[0]).toBeCloseTo(laidLeft + GEOMETRY.left, 5);
    expect(onCropped?.rect[1]).toBeCloseTo(laidTop + GEOMETRY.top, 5);
    // The control, and it is the defect rather than a restatement: the reported
    // corner must **not** be the one the reader dragged to on screen. Without it
    // both assertions above pass on a page whose crop happens to sit at the
    // origin, which is every page nobody has cropped.
    expect(onCropped?.rect[0]).not.toBeCloseTo(laidLeft, 5);
    expect(onCropped?.rect[1]).not.toBeCloseTo(laidTop, 5);
    viewer.destroy();
  });

  it("orders the corners whichever way the drag went", async () => {
    // Four drags between the same two points. A rectangle built by subtracting
    // in arrival order is inside out for three of them, and an inside-out crop
    // box is one the model refuses --- so a reader who drags up and to the left
    // gets an error rather than a crop.
    const viewer = build();
    await settle();
    viewer.setPages(pages(false));
    await settle();

    const pairs = [
      [
        { x: 120, y: 140 },
        { x: 320, y: 340 },
      ],
      [
        { x: 320, y: 340 },
        { x: 120, y: 140 },
      ],
      [
        { x: 320, y: 140 },
        { x: 120, y: 340 },
      ],
      [
        { x: 120, y: 340 },
        { x: 320, y: 140 },
      ],
    ];
    for (const pair of pairs) {
      viewer.armCrop();
      drag(pair[0] ?? { x: 0, y: 0 }, pair[1] ?? { x: 0, y: 0 });
    }

    expect(cropped).toHaveLength(4);
    const shapes = cropped.map((one) => [
      one.rect[2] - one.rect[0],
      one.rect[3] - one.rect[1],
    ]);
    expect(shapes[0]?.[0]).toBeGreaterThan(0);
    expect(shapes[0]?.[1]).toBeGreaterThan(0);
    for (const shape of shapes) expect(shape).toEqual(shapes[0]);
    viewer.destroy();
  });

  it("puts the drawing tool away, and the drawing tool puts it away", async () => {
    // Both directions in one test, because either alone passes with the code
    // half right --- and a viewer with two tools armed has a press that has to
    // ask which was meant, which is the state the `erasing` flag already exists
    // to keep impossible.
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    viewer.armCrop();
    expect(viewer.drawArmed).toBe(null);
    expect(viewer.cropArmed).toBe(true);

    viewer.armDraw("square");
    expect(viewer.cropArmed).toBe(false);
    expect(viewer.drawArmed).toBe("square");
    viewer.destroy();
  });
});
