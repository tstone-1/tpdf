/**
 * Tests for the turn a page carries after an edit, as against the view's.
 *
 * A rectangle the backend sends --- a comment's, a link's, a mark's --- is in
 * the page's display space, and placing one on screen needs every turn in force:
 * the reader's rotation *and* the quarter turns an edit applied to that page.
 * Six places in `viewer.ts` used the reader's alone, so on a page turned with
 * Rotate Right a comment was painted in one place and found in another, and a
 * destination scrolled down an axis that was no longer vertical.
 *
 * **The two placement tests compare subsystems rather than arithmetic.** Each
 * scans a grid of presses across the page and collects the points where the
 * thing under test opens, then asserts a comment's region and a link's region
 * are the region a *mark* with the identical rectangle produces. The mark path
 * was already right and has checks against a real window behind it, so this
 * says "these three agree about one rectangle" without the test recomputing the
 * geometry the code computes --- a writer agreeing with its own reader, which
 * `docs/TRAPS.md` records as unable to fail. Each is run twice, unturned and
 * turned, so a region that never moves cannot pass either.
 */

import { describe, expect, it, vi } from "vitest";

import { installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";
import type { Comment } from "./comments";
import type { Link } from "./links";
import type { PageText } from "./text";
import { pageId } from "./pages";
import { INK_WIDTH } from "./markband";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

/** The one rectangle every subject below is placed at, in display space. */
const RECT: [number, number, number, number] = [100, 50, 140, 90];

/** A 600x800 page, upright, as `page_text` reports it. */
function pageText(): PageText {
  return {
    codes: [97, 98],
    boxes: [10, 10, 20, 22, 20, 10, 30, 22],
    width_pt: 600,
    height_pt: 800,
    quarter_turns: 0,
    extract_ms: 0,
  };
}

function comment(): Comment {
  return {
    id: 7,
    page: 0,
    kind: "text",
    author: "a",
    body: "b",
    subject: "",
    date: null,
    rect: RECT,
    quads: [],
    object: null,
    reply_to: null,
    hidden: false,
    color: null,
  };
}

function link(): Link {
  // Not cast. A cast here hid `top` for `top_pt` on the first draft, which the
  // hit tests never read and the type would have refused on sight.
  return {
    id: 9,
    page: 0,
    rect: RECT,
    target: { kind: "page", page: 2, top_pt: null },
  };
}

/** The private geometry a press has to be built from, and nothing else. */
interface Placing {
  scroller: {
    pageOrigin(page: number): { left: number; top: number };
    pageSize(page: number): { width_pt: number; height_pt: number };
    knowsPageSize(page: number): boolean;
    pageTopOf(page: number): number;
  };
  scrollTo(top: number): void;
  zoom: number;
  scrollTop: number;
}

function build(dom: FakeDom, pageCount = 3): Viewer {
  core.invoke.mockImplementation((command: string) =>
    command === "page_text" ? Promise.resolve(pageText()) : Promise.resolve(null),
  );
  tiles.fetchTile.mockRejectedValue(new Error("no tile"));
  let rid = 0;
  tiles.nextRequestId.mockImplementation(() => ++rid);
  return new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount,
    pages: [{ width_pt: 600, height_pt: 800 }],
  });
}

/** Presses page 0 at a point in its own points, and hands back what opened. */
function pressAt(
  dom: FakeDom,
  viewer: Viewer,
  x: number,
  y: number,
  read: () => number,
): number {
  const v = viewer as unknown as Placing;
  const origin = v.scroller.pageOrigin(0);
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    target: dom.root,
    clientX: origin.left + x * v.zoom,
    clientY: origin.top + y * v.zoom - v.scrollTop,
    preventDefault() {},
  });
  const opened = read();
  dom.root.dispatch("pointerup", { pointerId: 1, target: dom.root });
  return opened;
}

/** Every grid point on page 0 at which `read` reports something open. */
function region(dom: FakeDom, viewer: Viewer, read: () => number, clear: () => void) {
  const found: string[] = [];
  for (let x = 0; x <= 800; x += 20) {
    for (let y = 0; y <= 800; y += 20) {
      if (pressAt(dom, viewer, x, y, read) !== -1) found.push(`${x},${y}`);
      clear();
    }
  }
  return found;
}

/**
 * Asserts a region is inside the page it belongs to, which no differential can.
 *
 * The eight comparisons below say the three subsystems agree, and since they
 * were collapsed onto one `turnsOn` they agree *by construction* --- so a fault
 * in the primitive itself moves all three together and every comparison stays
 * green. That is the shape `docs/TRAPS.md` records as a property holding by
 * construction, and it arrived here as a consequence of the fix.
 *
 * This is the absolute half. The bound is the page's laid-out pitch, read off
 * the scroller rather than computed from the rectangle: a page turned once is
 * 600 pt tall where the document's page is 800, so turning a rectangle at
 * y=50..90 one quarter too far puts it at y~730 --- off the bottom of the page
 * it is supposed to be on, wherever the other two subsystems think it is.
 */
function withinPage(viewer: Viewer, found: string[]): void {
  const v = viewer as unknown as Placing;
  const pitch = v.scroller.pageTopOf(1) - v.scroller.pageTopOf(0);
  const below = found.filter((point) => Number(point.split(",")[1]) > pitch);
  expect({ below, pitch }).toEqual({ below: [], pitch });
}

describe("a rectangle on a page an edit turned", () => {
  it("places the focus ring at the same position after a click and a keyboard step", async () => {
    const dom = installFakeDom();
    const viewer = build(dom);
    try {
      viewer.setZoomFixed(1);
      viewer.setPageTurns(0, 1);
      await settle();
      viewer.setLinks([{ ...link(), target: { kind: "none" } }]);
      expect(viewer.stepLink(1)).toBe(true);
      const ring = (viewer as unknown as { ring: HTMLElement }).ring;
      const position = () => [ring.style.left, ring.style.top];
      const keyboard = position();
      viewer.clearLinkFocus();
      expect(pressAt(dom, viewer, 730, 120, () => viewer.linkFocus)).toBe(9);
      expect(position()).toEqual(keyboard);
    } finally {
      viewer.destroy();
      dom.restore();
    }
  });

  for (const turn of [0, 1, 2, 3]) {
    it(`places a comment where a mark with the same rectangle is, at ${turn} turns`, async () => {
      const dom = installFakeDom();
      const viewer = build(dom);
      viewer.setPageTurns(0, turn);
      await settle();

      viewer.setMarks([{
        id: 3,
        kind: "highlight",
        stamp: null,
        page: pageId(1),
        quads: [...RECT],
        strokes: [],
        color: [1, 1, 0],
        width: INK_WIDTH,
        note: "",
        lines: [],
      }]);
      const marks = region(
        dom,
        viewer,
        () => viewer.markOpen,
        () => viewer.closeMark(),
      );
      viewer.setMarks([]);

      viewer.setComments([comment()]);
      const comments = region(
        dom,
        viewer,
        () => viewer.commentOpen,
        () => viewer.closeComment(),
      );

      expect(marks.length).toBeGreaterThan(0);
      withinPage(viewer, marks);
      expect(comments).toEqual(marks);
      viewer.destroy();
    });

    it(`places a link where a mark with the same rectangle is, at ${turn} turns`, async () => {
      const dom = installFakeDom();
      const viewer = build(dom);
      viewer.setPageTurns(0, turn);
      await settle();

      viewer.setMarks([{
        id: 3,
        kind: "highlight",
        stamp: null,
        page: pageId(1),
        quads: [...RECT],
        strokes: [],
        color: [1, 1, 0],
        width: INK_WIDTH,
        note: "",
        lines: [],
      }]);
      const marks = region(
        dom,
        viewer,
        () => viewer.markOpen,
        () => viewer.closeMark(),
      );
      viewer.setMarks([]);

      viewer.setLinks([link()]);
      const links = region(
        dom,
        viewer,
        () => viewer.linkFocus,
        () => viewer.clearLinkFocus(),
      );

      expect(marks.length).toBeGreaterThan(0);
      withinPage(viewer, marks);
      expect(links).toEqual(marks);
      viewer.destroy();
    });
  }

  it.each([false, true])("keeps the accessible phrase linked under a quarter turn (crop %s)", async (crop) => {
    const dom = installFakeDom();
    const viewer = build(dom, 1);
    core.invoke.mockImplementation(async (command) => {
      if (command === "page_geometry") return { width_pt: 500, height_pt: 700, left: 100, top: 100 };
      if (command === "document_mapping") return [];
      if (command === "page_text") return { ...pageText(), boxes: crop
        ? [10, 10, 20, 22, 20, 10, 30, 22] : [110, 110, 120, 122, 120, 110, 130, 122] };
      return null;
    });
    try {
      viewer.setPages([{ id: pageId(1), source: { baseline: 0 }, turns: 1,
        ...(crop ? { crop: [100, 0, 600, 700] as [number, number, number, number] } : {}) }]);
      await settle();
      viewer.setLinks([{ ...link(), rect: [109, 109, 119, 123] }]);
      await viewer.loadText(0);
      (viewer as unknown as { syncAccessibleText(visible: number[]): void }).syncAccessibleText([0]);
      const span = viewer.accessibleText.elementFor(0)?.querySelector("span");
      expect(span?.getAttribute("role")).toBe("link");
      expect(span?.textContent).toBe("a");
    } finally { viewer.destroy(); dom.restore(); }
  });

  it.each([true, false])("routes a press on a link overlapped by a comment (hidden %s)", async (hidden) => {
    const dom = installFakeDom();
    const viewer = build(dom);
    try {
      viewer.setComments([{ ...comment(), hidden }]);
      viewer.setLinks([link()]);
      await settle();
      const selected = pressAt(dom, viewer, 120, 70, () => hidden ? viewer.linkFocus : viewer.commentOpen);
      expect(selected).toBe(hidden ? 9 : 7);
      expect(hidden ? viewer.commentOpen : viewer.linkFocus).toBe(-1);
    } finally {
      viewer.destroy();
      dom.restore();
    }
  });

  it("moves comment and link hit boxes with a top crop and with its undo", async () => {
    const dom = installFakeDom();
    const viewer = build(dom, 1);
    const base = { id: pageId(1), source: { baseline: 0 } as const, turns: 0 };
    const cropped = { ...base, crop: [0, 0, 600, 775] as [number, number, number, number] };
    core.invoke.mockImplementation(async (command, args) => command === "page_geometry"
      ? { width_pt: 600, height_pt: args.crop ? 775 : 800, left: 0, top: args.crop ? 25 : 0 }
      : command === "page_text" ? pageText() : null);
    viewer.setComments([comment()]);
    viewer.setLinks([{ ...link(), rect: [200, 50, 240, 90] }]);
    try {
      for (const state of [base, cropped, base]) {
        viewer.setPages([state]);
        await settle();
        for (const y of [32, 85]) {
          const hit = state === cropped ? y === 32 : y === 85;
          expect(pressAt(dom, viewer, 120, y, () => viewer.commentOpen)).toBe(hit ? 7 : -1);
          viewer.closeComment();
          expect(pressAt(dom, viewer, 220, y, () => viewer.linkFocus)).toBe(hit ? 9 : -1);
          viewer.clearLinkFocus();
        }
      }
    } finally { viewer.destroy(); dom.restore(); }
  });

  it("puts the rectangle somewhere else once the page is turned", async () => {
    // The control for the eight above: a placement that ignored every turn
    // would satisfy them all, because all three subsystems would ignore it
    // together. Here the region has to *move*.
    const dom = installFakeDom();
    const viewer = build(dom);
    viewer.setComments([comment()]);
    await settle();
    const upright = region(
      dom,
      viewer,
      () => viewer.commentOpen,
      () => viewer.closeComment(),
    );
    viewer.setPageTurns(0, 1);
    await settle();
    const turned = region(
      dom,
      viewer,
      () => viewer.commentOpen,
      () => viewer.closeComment(),
    );
    expect(upright.length).toBeGreaterThan(0);
    expect(turned.length).toBeGreaterThan(0);
    expect(turned).not.toEqual(upright);
    viewer.destroy();
  });
});

it("restores text geometry when a second view adopts moved and turned pages", async () => {
  const viewer = build(installFakeDom(), 2);
  try {
    viewer.setPages([
      { id: pageId(2), source: { baseline: 1 }, turns: 1 },
      { id: pageId(1), source: { baseline: 0 }, turns: 0 },
    ]);
    expect(viewer.pageExtraTurns(0)).toBe(1);
    expect((await viewer.loadText(0))?.width_pt).toBe(800);
    expect((await viewer.loadText(1))?.width_pt).toBe(600);
  } finally {
    viewer.destroy();
  }
});

describe("what a turned page does to a place in it", () => {
  it("does not serve a link's old rectangle out of the cache after a turn", async () => {
    // One press per turn, and never off page 0. The grid scans above cannot see
    // the memo at all: they walk down past the page they are testing, so a
    // lookup for page 1 evicts the poisoned page-0 entry before anything reads
    // it back --- which is why the two mutations of the key survived them.
    //
    // The sequence is what makes both halves reachable. Warming at 0 and then
    // reading at 1 catches a lookup by the view's turn, which hits when it
    // should miss; going back to 0 catches a *store* by the view's turn, which
    // leaves the turned rectangles under a key the untuned lookup matches.
    const at = (turn: number): string => {
      const dom = installFakeDom();
      const viewer = build(dom);
      viewer.setPageTurns(0, turn);
      viewer.setMarks([{
        id: 3,
        kind: "highlight",
        stamp: null,
        page: pageId(1),
        quads: [...RECT],
        strokes: [],
        color: [1, 1, 0],
        width: INK_WIDTH,
        note: "",
        lines: [],
      }]);
      const found = region(
        dom,
        viewer,
        () => viewer.markOpen,
        () => viewer.closeMark(),
      );
      viewer.destroy();
      expect(found.length).toBeGreaterThan(0);
      return found[0] ?? "";
    };
    const [upright, turned] = [at(0), at(1)];
    expect(turned).not.toEqual(upright);

    const dom = installFakeDom();
    const viewer = build(dom);
    viewer.setLinks([link()]);
    await settle();
    const press = (point: string): number => {
      const [x, y] = point.split(",").map(Number);
      const opened = pressAt(dom, viewer, x ?? 0, y ?? 0, () => viewer.linkFocus);
      viewer.clearLinkFocus();
      return opened;
    };

    expect(press(upright)).toBe(9);
    viewer.setPageTurns(0, 1);
    await settle();
    expect(press(turned)).toBe(9);
    viewer.setPageTurns(0, 0);
    await settle();
    expect(press(upright)).toBe(9);
    viewer.destroy();
  });

  it("projects destinations identically for equivalent combined turns", async () => {
    const dom = installFakeDom();
    const viewer = build(dom);
    const edges = { width: 600, height: 800, left: 40, right: 250, bottom: 300 };
    try {
      await settle();
      viewer.rotateBy(-1);
      for (const [pageTurns, expected] of [[1, 120], [2, 40], [3, 500]] as const) {
        viewer.setPageTurns(0, pageTurns);
        expect(viewer.fileDestinationTop(0, 120, edges)).toBe(expected);
      }
    } finally {
      viewer.destroy();
      dom.restore();
    }
  });

  it("lands a destination on a turned page rather than partway down it", async () => {
    // The same rule a rotated *view* already follows, and for the same reason:
    // at a quarter turn the destination's axis is the screen's horizontal one.
    // The two cases are compared directly, so this cannot pass by both being
    // wrong in the same way.
    const dom = installFakeDom();
    const viewer = build(dom);
    const v = viewer as unknown as Placing;
    await settle();

    viewer.rotateBy(1);
    viewer.goToDestination(0, 400);
    const underView = (v.scrollTop - v.scroller.pageTopOf(0)) / v.zoom;
    viewer.rotateBy(-1);

    viewer.setPageTurns(0, 1);
    await settle();
    viewer.goToDestination(0, 400);
    const underEdit = (v.scrollTop - v.scroller.pageTopOf(0)) / v.zoom;

    expect(underView).toBe(0);
    expect(underEdit).toBe(underView);
    viewer.destroy();
  });

  it("reports no offset within a page an edit turned", async () => {
    // `position` feeds the history and the session, so an offset measured down
    // an axis that is no longer vertical is what Back and a restart land on.
    const dom = installFakeDom();
    const viewer = build(dom);
    const v = viewer as unknown as Placing;
    await settle();

    v.scrollTo(v.scroller.pageTopOf(0) + 100 * v.zoom);
    viewer.rotateBy(1);
    const underView = viewer.position.top;
    viewer.rotateBy(-1);

    viewer.setPageTurns(0, 1);
    await settle();
    v.scrollTo(v.scroller.pageTopOf(0) + 100 * v.zoom);
    expect(underView).toBe(0);
    expect(viewer.position.top).toBe(underView);
    viewer.destroy();
  });

  it("learns a page's size in the document's space, not the turned view's", async () => {
    // `TextCache.peek` answers in view space, which includes the page's own
    // edit turn, so a page turned before it was ever on screen learned its size
    // transposed --- and kept it, since a size is learned once.
    const dom = installFakeDom();
    const viewer = build(dom);
    const v = viewer as unknown as Placing;
    viewer.setPageTurns(1, 1);
    await settle();
    v.scrollTo(v.scroller.pageTopOf(1) + 10);
    for (let frame = 0; frame < 12; frame++) {
      dom.runFrames();
      await settle();
    }
    expect(v.scroller.knowsPageSize(1)).toBe(true);
    expect(v.scroller.pageSize(1)).toEqual({ width_pt: 600, height_pt: 800 });
    viewer.destroy();
  });
});
