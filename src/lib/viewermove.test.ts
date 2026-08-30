/**
 * Dragging one of the reader's own marks to somewhere else on its page.
 *
 * **Reported from use, and the report was about a comment**: *"edit -> add
 * comment adds a speech bubble - but I can't drag it to move it."* Nothing could
 * be moved --- the model had no command for it, and a press on a mark opened its
 * note and did nothing else.
 *
 * Two halves are tested here and only one of them is the gesture. The other is
 * *which* marks the drag is offered on, which is a product rule rather than an
 * arithmetic one: `isMovable` says a highlight is made of the words under it and
 * a comment is not, and a fixture with only one kind in it cannot tell a viewer
 * that asks from one that moves anything it is pressed on.
 *
 * The offsets that leave are in the page's **display** space, where the gesture
 * is in the laid-out space, so a turned page is in every describe block for
 * `viewerdraw.test.ts`'s reason: upright the two spaces are the same numbers and
 * every assertion passes under either implementation.
 *
 * Every test below was checked by mutating `viewer.ts` or `markband.ts` and
 * confirming it went red; `scripts/mutate_frontend.py` holds the mutations.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pageId, type MarkKind, type MarkView, type PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";
import { INK_WIDTH } from "./markband";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

let dom: FakeDom;
/** Every move the viewer reported, in order. */
let moved: { id: number; dx: number; dy: number }[];

beforeEach(() => {
  dom = installFakeDom();
  moved = [];
  core.invoke.mockResolvedValue(null);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

/**
 * One mark of `kind`, 100 points square, well inside a 600 by 800 page.
 *
 * Square and central on purpose: the clamp is what keeps a mark on its page, and
 * a fixture pressed against an edge would have every drag cut short by it, which
 * is a different test and would make these ones pass for the wrong reason.
 */
function mark(kind: MarkKind, id = 42): MarkView[] {
  return [
    {
      id,
      kind,
      stamp: null,
      page: pageId(1),
      quads: [200, 300, 300, 400],
      strokes: kind === "ink" ? [[210, 310, 290, 390]] : [],
      color: [0.85, 0.15, 0.15],
      width: INK_WIDTH,
      note: "",
      lines: [],
    },
  ];
}

/** A one-page document, 600 by 800 points, optionally turned by an edit. */
function build(turns = 0): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
    onMarkMoved: (id, dx, dy) => moved.push({ id, dx, dy }),
  });
  if (turns !== 0) {
    const pages: PageView[] = [{ id: pageId(1), source: { baseline: 0 }, turns }];
    viewer.setPages(pages);
  }
  return viewer;
}

/** Where the mark's own rectangle is on screen, so a press can land on it. */
function onMark(viewer: Viewer, id = 42): { x: number; y: number } {
  const anchor = viewer.markAnchor(id);
  if (!anchor) throw new Error("the mark is not laid out");
  return {
    x: (anchor.left + anchor.right) / 2,
    y: (anchor.top + anchor.bottom) / 2,
  };
}

/** Presses, drags and releases, in the root's client coordinates. */
function drag(
  from: { x: number; y: number },
  by: { x: number; y: number },
  release = true,
): void {
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: from.x,
    clientY: from.y,
    target: dom.root,
  });
  dom.root.dispatch("pointermove", {
    pointerId: 1,
    clientX: from.x + by.x,
    clientY: from.y + by.y,
  });
  if (release) {
    dom.root.dispatch("pointerup", {
      pointerId: 1,
      clientX: from.x + by.x,
      clientY: from.y + by.y,
    });
  }
}

describe("which marks a drag is offered on", () => {
  it("moves a comment, which is what nobody could do", async () => {
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    drag(onMark(viewer), { x: 60, y: 40 });

    expect(moved).toHaveLength(1);
    expect(moved[0]?.id).toBe(42);
    // In points, so the client pixels the hand moved are divided by the zoom.
    // Positive both ways, which is the least a passing test can say: a viewer
    // that reported the offset negated would move the mark the other way and
    // still satisfy a check on the magnitude.
    expect(moved[0]?.dx).toBeGreaterThan(0);
    expect(moved[0]?.dy).toBeGreaterThan(0);
    viewer.destroy();
  });

  it("moves a box, an ellipse, a text box and a drawing", async () => {
    // The other four kinds a reader places. Asserted together rather than one
    // test each, because what would ship is a predicate that named the comment
    // alone --- the kind that was reported --- and any one of these would catch
    // that.
    for (const kind of ["square", "ellipse", "textbox", "ink"] as const) {
      moved = [];
      const viewer = build();
      viewer.setMarks(mark(kind));
      await settle();

      drag(onMark(viewer), { x: 60, y: 40 });

      expect(moved, kind).toHaveLength(1);
      viewer.destroy();
    }
  });

  it("does not move a mark that is made of the words under it", async () => {
    // **The control, and every test above is worth nothing without it.** A
    // viewer that moved whatever it was pressed on would pass all of them. A
    // highlight's rectangle came off a text selection, so dragging one leaves a
    // wash over a line it does not mark and nothing to snap it back to.
    for (const kind of ["highlight", "underline", "strikeout", "squiggly"] as const) {
      moved = [];
      const viewer = build();
      viewer.setMarks(mark(kind));
      await settle();

      drag(onMark(viewer), { x: 60, y: 40 });

      expect(moved, kind).toEqual([]);
      viewer.destroy();
    }
  });
});

describe("what the gesture reports", () => {
  it("opens the note on the press, and moves on the drag", async () => {
    // Both, from one press. The note opens on the way down --- which is what a
    // reader clicking a mark asks for and is how it worked before this --- and
    // the drag is what happens if the pointer then moves. A viewer that had to
    // wait for the button to come up before deciding would make every note feel
    // slow.
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    drag(onMark(viewer), { x: 60, y: 40 });

    expect(viewer.markOpen).toBe(42);
    expect(moved).toHaveLength(1);
    viewer.destroy();
  });

  it("reports nothing for a press that did not move", async () => {
    // A click is how a reader opens a note. Reporting a zero offset would put a
    // command in the journal for it, and undo would then step back through every
    // note that was ever opened before it reached anything the reader changed.
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    const at = onMark(viewer);
    dom.root.dispatch("pointerdown", {
      button: 0,
      pointerId: 1,
      clientX: at.x,
      clientY: at.y,
      target: dom.root,
    });
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: at.x, clientY: at.y });

    expect(moved).toEqual([]);
    expect(viewer.markOpen).toBe(42);
    viewer.destroy();
  });

  it("reports once for a drag, not once per pointer event", async () => {
    // One command per gesture, so one undo puts the mark back where it started
    // rather than walking it home a pointer event at a time. Same rule the
    // eraser's sweep follows.
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    const at = onMark(viewer);
    dom.root.dispatch("pointerdown", {
      button: 0,
      pointerId: 1,
      clientX: at.x,
      clientY: at.y,
      target: dom.root,
    });
    for (const step of [10, 20, 30, 40]) {
      dom.root.dispatch("pointermove", {
        pointerId: 1,
        clientX: at.x + step,
        clientY: at.y + step,
      });
    }
    dom.root.dispatch("pointerup", {
      pointerId: 1,
      clientX: at.x + 40,
      clientY: at.y + 40,
    });

    expect(moved).toHaveLength(1);
    viewer.destroy();
  });

  it("throws the move away on Escape", async () => {
    // The offset lives in the viewer until the pointer comes up, so abandoning
    // one costs nothing and undoes nothing: there is no command to take back.
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    const at = onMark(viewer);
    drag(at, { x: 60, y: 40 }, false);
    dom.root.dispatch("keydown", {
      key: "Escape",
      shiftKey: false,
      altKey: false,
      metaKey: false,
      ctrlKey: false,
      target: dom.root,
    });
    dom.root.dispatch("pointerup", {
      pointerId: 1,
      clientX: at.x + 60,
      clientY: at.y + 40,
    });

    expect(moved).toEqual([]);
    viewer.destroy();
  });
});

describe("keeping a mark on its page", () => {
  it("cuts the offset short at the page's edge", async () => {
    // A mark dragged past the paper would be written with a `/Rect` outside the
    // page box, which other readers clip, draw half of, or place somewhere of
    // their own choosing --- `iconQuad` clamps for exactly this reason and this
    // is the same rule for a gesture rather than for a placement.
    //
    // The mark's left edge is at 200 points, so a drag of a whole page width to
    // the left can move it by 200 and no further.
    const viewer = build();
    viewer.setMarks(mark("note"));
    await settle();

    drag(onMark(viewer), { x: -4000, y: -4000 });

    expect(moved).toHaveLength(1);
    expect(moved[0]?.dx).toBeCloseTo(-200, 3);
    expect(moved[0]?.dy).toBeCloseTo(-300, 3);
    viewer.destroy();
  });
});

describe("a page an edit has turned", () => {
  it("reports the offset in the file's space, not the reader's", async () => {
    // **The fixture that can tell the two apart.** A mark is stored in the
    // page's display space and the reader is looking at it turned, so a viewer
    // that sent the offset it measured on screen would move the mark sideways
    // when the hand went down. Upright the two are the same two numbers and this
    // assertion passes either way, which is why the turn is here.
    //
    // A quarter turn clockwise puts the page's own left edge along the top of
    // the window, so the direction that was rightwards on the paper is now
    // downwards on screen: a drag of (0, +d) must come out as (+d, 0).
    //
    // **The zoom is pinned to 1 so that the length can be asserted at all.**
    // Client pixels reach the page through it, and a turned page is 800 by 600
    // where the upright one is 600 by 800 --- so fit-width picks a different zoom
    // for each, and the same 80-pixel drag is a different distance on the paper.
    // Comparing the two fixtures without this reports 76.19 against 57.14, which
    // is the ratio of the page's own sides and not a defect. At 1 the drag is 80
    // points on both.
    const upright = build();
    upright.setZoomFixed(1);
    upright.setMarks(mark("note"));
    await settle();
    drag(onMark(upright), { x: 0, y: 80 });
    upright.destroy();
    expect(moved).toHaveLength(1);
    expect(moved[0]?.dx).toBeCloseTo(0, 3);
    expect(moved[0]?.dy).toBeCloseTo(80, 3);

    moved = [];
    const viewer = build(1);
    viewer.setZoomFixed(1);
    viewer.setMarks(mark("note"));
    await settle();

    drag(onMark(viewer), { x: 0, y: 80 });

    expect(moved).toHaveLength(1);
    const { dx, dy } = moved[0] ?? { dx: 0, dy: 0 };
    expect(dy).toBeCloseTo(0, 3);
    expect(dx).toBeCloseTo(80, 3);
    viewer.destroy();
  });
});
