/**
 * Middle-button panning: the gesture, and the axis it exists for.
 *
 * The vertical half of this could be asserted on any document. The horizontal
 * half only does anything past the width of the window --- `Scroller.maxPan` is
 * zero at every fit zoom --- so every test here zooms in first, and a fixture
 * that did not would pass under an implementation that never panned sideways at
 * all. `scroller.test.ts` owns what the pan *is*; this owns that the gesture
 * drives it, which is the separate claim.
 *
 * The observable is {@link Viewer.markAnchor}, deliberately rather than a
 * getter added for the tests: it is where the note box hangs, so it is what a
 * reader sees, and it carries both axes through the same `pageOrigin` the tiles
 * are placed from. A check reading a number this file invented could not see
 * the pages failing to move.
 *
 * Every test was checked by mutating `viewer.ts` and confirming it went red;
 * `scripts/mutate_frontend.py` holds the mutations.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type MarkView } from "./pages";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";
import { Viewer } from "./viewer";
import type { Anchor } from "./popup";

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

/** One mark, near the top-left of the page, to read a screen position off. */
const MARK: MarkView[] = [
  {
    id: 7,
    kind: "highlight",
    stamp: null,
    page: pageId(1),
    quads: [40, 40, 200, 60],
    strokes: [],
    color: [0.85, 0.15, 0.15],
    width: 1,
    note: "",
    lines: [],
  },
];

/**
 * A viewer on a 600x800 pt page, zoomed until the page overflows the window.
 *
 * The window is 900 CSS px (`installFakeDom`'s default) and the page is 600 pt,
 * so it fits at zoom 1 and there is nothing to pan. At zoom 2 it is 1200 px
 * wide, which leaves 300 px of overhang --- enough that the drags below move
 * inside the bound rather than against it, since a clamped drag would agree
 * with an implementation that never moved at all.
 */
function build(zoom = 2): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
  });
  viewer.setZoomFixed(zoom);
  viewer.setMarks(MARK);
  return viewer;
}

/** Where the mark is on screen now. */
function anchor(viewer: Viewer): Anchor {
  const at = viewer.markAnchor(7);
  if (!at) throw new Error("the fixture's mark must be placed");
  return at;
}

/** Presses `button` at `from`, moves through `via`, and optionally releases. */
function drag(
  button: number,
  from: { x: number; y: number },
  via: { x: number; y: number }[],
  end: "up" | "cancel" | "held" = "up",
): void {
  dom.root.dispatch("pointerdown", {
    button,
    pointerId: 1,
    clientX: from.x,
    clientY: from.y,
    target: dom.root,
  });
  for (const at of via) {
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      clientX: at.x,
      clientY: at.y,
    });
  }
  const last = via[via.length - 1] ?? from;
  if (end === "held") return;
  dom.root.dispatch(end === "up" ? "pointerup" : "pointercancel", {
    pointerId: 1,
    clientX: last.x,
    clientY: last.y,
  });
}

describe("Middle-button panning", () => {
  it("moves the view with the pointer, on both axes", () => {
    const viewer = build();
    const before = anchor(viewer);

    // Up and to the left. Both directions are the ones with room: the view
    // starts at the top-left of the document, so dragging the other way would
    // clamp at zero on both axes and pass under an implementation that panned
    // nothing.
    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }]);

    const after = anchor(viewer);
    // **The content follows the pointer**, so the mark moves by exactly the
    // pointer's displacement. Asserting the displacement rather than a
    // direction is what pins the hand-tool convention: a view that followed the
    // pointer instead would move the mark +40 and +50.
    expect(after.left - before.left).toBe(-40);
    expect(after.top - before.top).toBe(-50);
  });

  it("is the middle button and no other", () => {
    for (const button of [0, 2]) {
      const viewer = build();
      const before = anchor(viewer);
      drag(button, { x: 500, y: 400 }, [{ x: 460, y: 350 }]);
      const after = anchor(viewer);
      expect(after.left).toBe(before.left);
      expect(after.top).toBe(before.top);
    }

    // The control, through the same door: without it the loop above is
    // satisfied by a viewer in which no button pans.
    const viewer = build();
    const before = anchor(viewer);
    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }]);
    expect(anchor(viewer).left).toBe(before.left - 40);
  });

  it("applies each move as a displacement from the press, not from the last move", () => {
    // Out past the bound and back to where the press started. A pan that
    // accumulated deltas would spend the overshoot on the way out -- the applied
    // pan clamps at the bound and the excess is lost -- and the return would
    // then carry the view past the start, to the left edge. Taking every move
    // from the press instead makes the return exact, whatever happened in
    // between.
    //
    // ⚠ **No mutation of `viewer.ts` can redden this, and it is registered in
    // no mutation table for that reason.** The property holds *by
    // construction*: while every move is computed from `panFrom`, returning the
    // pointer to the press necessarily returns the pan to what it was, and no
    // one-line edit turns that handler into an accumulating one. It is kept as
    // a tripwire for the rewrite that would --- the same treatment this
    // repository gives a guard the type system already makes unexpressible ---
    // and it is labelled so that nobody reads it as a check that has been shown
    // to fail.
    const viewer = build();
    const home = anchor(viewer);

    drag(
      1,
      { x: 500, y: 400 },
      [
        // Far enough left to run out of page: the overhang is 300 px.
        { x: -600, y: 400 },
        // And back to the press.
        { x: 500, y: 400 },
      ],
    );

    expect(anchor(viewer).left).toBe(home.left);
  });

  it("says a pan is happening, and stops saying so", () => {
    const viewer = build();
    const resting = viewer.cursorName;

    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }], "held");
    expect(viewer.cursorName).toBe("grabbing");

    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 460, clientY: 350 });
    expect(viewer.cursorName).toBe(resting);
  });

  it("outranks an armed tool's cursor while the button is down", () => {
    // The ordering `showCursor` states: a pan is a gesture happening now and an
    // armed tool is a statement about the next primary press, so a crosshair
    // through a middle drag would describe a gesture nobody is making.
    const viewer = build();
    viewer.armDraw("square");
    expect(viewer.cursorName).toBe("crosshair");

    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }], "held");
    expect(viewer.cursorName).toBe("grabbing");

    // And the tool is still armed afterwards: a pan borrows the cursor, it does
    // not disarm anything.
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 460, clientY: 350 });
    expect(viewer.cursorName).toBe("crosshair");
  });

  it("ends on a cancelled gesture, and moves nothing afterwards", () => {
    const viewer = build();
    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }], "cancel");
    const settled = anchor(viewer);
    expect(viewer.cursorName).not.toBe("grabbing");

    // A move with the gesture gone must do nothing.
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      clientX: 100,
      clientY: 100,
    });
    expect(anchor(viewer).left).toBe(settled.left);
  });

  it("takes its listeners back off, which no drag can notice", () => {
    // **An accounting observable, and it is here because a behavioural one
    // cannot work.** `onPanEnd` clears `panFrom` *and* removes the three
    // listeners, and either alone stops a stray move panning --- two mechanisms
    // for one rule, so a mutation deleting the removals survived every
    // assertion above. What it costs is a set of listeners per drag, on an
    // element that lives as long as the document, and nothing a reader does can
    // see that.
    // Built for its effect and not read: the constructor is what puts the
    // listeners on the root, and this test is about the three a *pan* adds and
    // takes away again.
    build();
    const counts = (): number[] =>
      ["pointermove", "pointerup", "pointercancel"].map(
        (name) => dom.root.listeners.get(name)?.size ?? 0,
      );
    const resting = counts();

    drag(1, { x: 500, y: 400 }, [{ x: 460, y: 350 }], "held");
    // The control: the drag really did add them, or the equality below is
    // satisfied by a gesture that attached nothing.
    expect(counts()).not.toEqual(resting);

    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 460, clientY: 350 });
    expect(counts()).toEqual(resting);

    // And again, because one drag leaking one set is invisible in a count that
    // is only ever compared against the reading before it.
    drag(1, { x: 400, y: 300 }, [{ x: 380, y: 280 }]);
    expect(counts()).toEqual(resting);
    // The viewer never registered `pointercancel` outside a pan, so its resting
    // count is zero -- which is what makes the comparison above a real one
    // rather than two identical non-zero numbers.
    expect(resting[2]).toBe(0);
  });

  it("leaves a press on the scrollbar to the scrollbar", () => {
    const viewer = build();
    const before = anchor(viewer);

    // The track is the child pinned to the right-hand edge. Asserted to be
    // exactly one, because a finder that matched nothing would make every
    // assertion below pass without the guard being exercised at all.
    const tracks = (dom.root.children as FakeElement[]).filter((child) =>
      (child.style.cssText ?? "").includes("bottom:0"),
    );
    expect(tracks).toHaveLength(1);

    dom.root.dispatch("pointerdown", {
      button: 1,
      pointerId: 1,
      clientX: 890,
      clientY: 400,
      target: tracks[0],
    });
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      clientX: 700,
      clientY: 200,
    });
    expect(anchor(viewer).left).toBe(before.left);
  });
});
