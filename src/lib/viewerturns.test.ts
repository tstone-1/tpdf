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
