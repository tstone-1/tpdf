/**
 * Drawing a box: the gesture, and the space the rectangle ends up in.
 *
 * Two things are being tested and only one of them is the drag. `drag.test.ts`
 * covers the lifecycle --- capture, cancel, a second pointer --- against no
 * viewer at all. What is left here is everything about *where* the rectangle
 * goes, and that is the half with a defect already in it: `commentAt` took a
 * point in the page's laid-out space and handed it to the model, which holds
 * the file's, so a comment dropped on a turned or cropped page landed somewhere
 * else. The box travels the same route and would have inherited it.
 *
 * So the rotated cases below are not thoroughness. On an unrotated, uncropped
 * page the two spaces are the same four numbers and every assertion here passes
 * under either implementation --- which is exactly the fixture that cannot tell
 * them apart, and the reason a quarter-turned page is in every describe block.
 *
 * Every test was checked by mutating `viewer.ts`, `markband.ts` or `text.ts`
 * and confirming it went red; `scripts/mutate_frontend.py` holds the mutations.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MIN_BOX } from "./markband";
import type { MarkKind, PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer, type Drawn } from "./viewer";
import { INK_SAMPLE } from "./markband";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

let dom: FakeDom;
/** Every box the viewer reported, in order. */
let drawn: { kind: MarkKind; page: number; shape: Drawn }[];

beforeEach(() => {
  dom = installFakeDom();
  drawn = [];
  core.invoke.mockResolvedValue(null);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

/** A page that an edit has turned by `turns` quarter turns. */
function turnedPage(turns: number): PageView[] {
  return [{ id: 1, source: 0, turns }];
}

/** A one-page document, 600 by 800 points, optionally turned. */
function build(pages?: PageView[]): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
    onDrawn: (kind, page, shape) => drawn.push({ kind, page, shape }),
  });
  if (pages) viewer.setPages(pages);
  return viewer;
}

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

/** The four numbers of one box that was drawn. */
function corners(at = -1): [number, number, number, number] {
  const box = drawn.at(at);
  if (!box) throw new Error("nothing was drawn");
  const [left, top, right, bottom] = box.shape.quads;
  return [left ?? 0, top ?? 0, right ?? 0, bottom ?? 0];
}

describe("arming the tool", () => {
  it("does nothing to a document until a drag happens", async () => {
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    expect(viewer.drawArmed).toBe("square");
    expect(drawn).toEqual([]);
    viewer.destroy();
  });

  it("draws nothing when a drag happens with no tool armed", async () => {
    // The control for every test below. A viewer that reported a box on any
    // drag would pass all of them, and this is the only assertion that says
    // arming is what decides.
    const viewer = build();
    await settle();

    drag({ x: 100, y: 100 }, { x: 300, y: 200 });

    expect(drawn).toEqual([]);
    viewer.destroy();
  });

  it("is spent by one box", async () => {
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 });
    expect(viewer.drawArmed).toBe(null);

    drag({ x: 100, y: 300 }, { x: 300, y: 400 });
    expect(drawn).toHaveLength(1);
    viewer.destroy();
  });

  it("is dropped by Escape, with nothing drawn", async () => {
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    escape();

    expect(viewer.drawArmed).toBe(null);
    drag({ x: 100, y: 100 }, { x: 300, y: 200 });
    expect(drawn).toEqual([]);
    viewer.destroy();
  });

  it("is dropped by Escape mid-drag, without committing the box", async () => {
    // Escape during the drag rather than before it: the drag is live, its
    // preview is on screen, and the rectangle must not reach the model.
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 }, false);
    expect(viewer.drawPreview).not.toBe(null);

    escape();
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 300, clientY: 200 });

    expect(drawn).toEqual([]);
    expect(viewer.drawPreview).toBe(null);
    viewer.destroy();
  });
});

describe("the rectangle that reaches the model", () => {
  it("carries the armed kind and the page's id", async () => {
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 });

    expect(drawn).toHaveLength(1);
    expect(drawn[0]?.kind).toBe("square");
    // The **id**, which `unedited` allocates from 1, not the slot, which is 0.
    // A viewer sending the slot works on every document until a page is
    // deleted, and then every mark lands on the wrong page.
    expect(drawn[0]?.page).toBe(1);
    viewer.destroy();
  });

  it("orders the corners whichever way the drag went", async () => {
    // Four drags between the same two points. A rectangle built by subtracting
    // in arrival order comes out inside out for three of them, and an
    // inside-out rectangle simply does not draw --- which reads as a tool that
    // works downhill and not uphill.
    const viewer = build();
    await settle();

    const pairs: { x: number; y: number }[][] = [
      [
        { x: 100, y: 100 },
        { x: 300, y: 200 },
      ],
      [
        { x: 300, y: 200 },
        { x: 100, y: 100 },
      ],
      [
        { x: 300, y: 100 },
        { x: 100, y: 200 },
      ],
      [
        { x: 100, y: 200 },
        { x: 300, y: 100 },
      ],
    ];
    for (const pair of pairs) {
      viewer.armDraw("square");
      drag(pair[0] ?? { x: 0, y: 0 }, pair[1] ?? { x: 0, y: 0 });
    }

    expect(drawn).toHaveLength(4);
    const shapes = drawn.map((_box, at) => {
      const [left, top, right, bottom] = corners(at);
      return [right - left, bottom - top];
    });
    expect(shapes[0]?.[0]).toBeGreaterThan(0);
    expect(shapes[0]?.[1]).toBeGreaterThan(0);
    for (const shape of shapes) expect(shape).toEqual(shapes[0]);
    viewer.destroy();
  });

  it("refuses a click, and keeps the tool armed", async () => {
    // A press with no drag is a rectangle of no size, and one saved is an
    // annotation nothing draws and nobody can find again to remove. Keeping the
    // tool armed is what makes it a non-event rather than an error: the reader
    // tries again.
    //
    // The press and the release land on the same point rather than one short of
    // MIN_BOX, and the difference matters: MIN_BOX is in **points** and a drag
    // is in **client pixels**, which the zoom separates. A test written on the
    // constant is really a test of whatever zoom the fixture happened to pick;
    // the bound itself is `markband.test.ts`'s, on both sides, in one unit.
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 100, y: 100 });

    expect(drawn).toEqual([]);
    expect(viewer.drawArmed).toBe("square");
    viewer.destroy();
  });

  it("takes a real drag, so the refusal above is not a refusal of everything", async () => {
    // The control for it. Comfortably over the bound at any zoom the fixture
    // could produce, since what this says is only that *something* gets through.
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 100 + MIN_BOX * 20, y: 100 + MIN_BOX * 20 });

    expect(drawn).toHaveLength(1);
    viewer.destroy();
  });
});

describe("a page an edit has turned", () => {
  it("puts a box drawn at the screen's top-left in the corner the turn implies", async () => {
    // **The load-bearing test, and its first draft passed for the wrong
    // reason.** That draft drew the same screen rectangle on an unturned page
    // and a quarter-turned one and asserted the two answers differed --- which
    // a viewer reporting laid-out coordinates *also* satisfies, because the
    // turned page is 800 points wide where the flat one is 600 and therefore
    // lays out at a different zoom. Two different numbers, from the defect.
    // The mutation said so: it was caught by the two tests below and not by
    // this one.
    //
    // So the assertion is the corner instead. The same drag near the screen's
    // top-left is a different corner of the *sheet* at each turn, and it walks
    // round it: top-left, bottom-left, bottom-right, top-right. A viewer that
    // skips the inverse reports the top-left every time, whatever the zoom.
    const corner: [boolean, boolean][] = [];
    for (const turns of [0, 1, 2, 3]) {
      const viewer = build(turnedPage(turns));
      await settle();
      viewer.armDraw("square");
      drag({ x: 40, y: 40 }, { x: 140, y: 120 });
      viewer.destroy();

      const [left, top, right, bottom] = corners();
      // The file's own page, 600 by 800, whichever way the reader is holding it.
      corner.push([(left + right) / 2 < 300, (top + bottom) / 2 < 400]);
    }

    expect(corner).toEqual([
      [true, true], // top-left
      [true, false], // bottom-left
      [false, false], // bottom-right
      [false, true], // top-right
    ]);
  });

  it("transposes the rectangle's own proportions", async () => {
    // Concretely, and it is what separates a correct inverse from a translation
    // that merely moves the box: a drag twice as wide as it is tall, on a page
    // turned a quarter, is a rectangle twice as *tall* as it is wide on the
    // sheet. A viewer that undid the turn with the wrong page dimensions gets a
    // plausible rectangle in the wrong place with the right proportions.
    const viewer = build(turnedPage(1));
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 });
    const [left, top, right, bottom] = corners();

    // The **ratio**, not the two lengths: client pixels reach the page through
    // the zoom, so the absolute numbers are a fact about how wide the fixture's
    // window is. The transpose is not --- a 2:1 drag is a 1:2 rectangle at a
    // quarter turn whatever the zoom, and a viewer that skipped the turn
    // reports 2:1 here.
    expect((bottom - top) / (right - left)).toBeCloseTo(2, 3);
    viewer.destroy();
  });

  it("stays inside the page at every turn", async () => {
    // A drag that leaves the page is one flick of the wrist at the edge, and an
    // unclamped one writes a /Rect running past the page box -- which `save.rs`
    // maps without complaint, because it maps quads and does not police them.
    // Asserted at all four turns because the clamp is applied in the laid-out
    // space and this assertion is about the file's, so a clamp against the
    // wrong pair of dimensions passes at 0 and 2 and fails at 1 and 3.
    for (const turns of [0, 1, 2, 3]) {
      const viewer = build(turnedPage(turns));
      await settle();

      viewer.armDraw("square");
      drag({ x: 50, y: 50 }, { x: 5000, y: 5000 });
      viewer.destroy();

      const [left, top, right, bottom] = corners();
      expect(left, "turns " + turns + " left").toBeGreaterThanOrEqual(0);
      expect(top, "turns " + turns + " top").toBeGreaterThanOrEqual(0);
      // The **file's** page, 600 by 800, whatever the reader is looking at.
      expect(right, "turns " + turns + " right").toBeLessThanOrEqual(600.001);
      expect(bottom, "turns " + turns + " bottom").toBeLessThanOrEqual(800.001);
    }
  });
});

describe("a drag that wanders", () => {
  it("keeps the box on the page it started from", async () => {
    // A box belongs to one page --- a PDF annotation cannot span two --- so a
    // drag that runs off the bottom of the first page is clamped to it rather
    // than silently moving to the second. Re-reading the page under the pointer
    // would also measure the starting corner on a page it is no longer on,
    // which puts the box somewhere neither end of the drag was.
    //
    // Needs two pages, and that is the whole point: a one-page fixture cannot
    // tell a viewer that re-reads the page from one that does not.
    const viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 2,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onDrawn: (kind, page, shape) => drawn.push({ kind, page, shape }),
    });
    viewer.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 2, source: 1, turns: 0 },
    ]);
    await settle();

    viewer.armDraw("square");
    // Far past the bottom of the first page, at any zoom this fixture picks.
    drag({ x: 100, y: 100 }, { x: 300, y: 9000 });
    viewer.destroy();

    expect(drawn).toHaveLength(1);
    expect(drawn[0]?.page).toBe(1);
    // And clamped to that page rather than running past its bottom edge.
    const [, , , bottom] = corners();
    expect(bottom).toBeLessThanOrEqual(800.001);
  });
});

describe("which path a press takes", () => {
  it("goes to the drag when a tool is armed and not when one is not", async () => {
    // **The mode itself, and the only observable in this fixture that can see
    // it.** The obvious assertion --- that an armed press starts no selection ---
    // cannot fail here: the fake DOM extracts no text, so a press-drag produces
    // no selection either way, and a viewer with the interception deleted would
    // pass it. That is the trap about a property that holds by construction, so
    // it is not asserted. What genuinely differs is where the press *goes*, and
    // a live preview is what says it went to the drag.
    const viewer = build();
    await settle();

    drag({ x: 100, y: 100 }, { x: 300, y: 200 }, false);
    const unarmed = viewer.drawPreview;
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 300, clientY: 200 });

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 }, false);
    const armed = viewer.drawPreview;
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 300, clientY: 200 });

    expect(unarmed).toBe(null);
    expect(armed).not.toBe(null);
    expect(armed?.slot).toBe(0);
    viewer.destroy();
  });

  it("follows the page rather than the window while the drag is live", async () => {
    // The preview is held in the page's own space, so a scroll under a still
    // pointer leaves the rectangle on the paper. Held in client coordinates it
    // would slide up the page as the reader scrolled down, which looks like a
    // box being dragged by something nobody is touching.
    const viewer = build();
    await settle();

    viewer.armDraw("square");
    drag({ x: 100, y: 100 }, { x: 300, y: 200 }, false);
    const before = viewer.drawPreview;
    expect(before).not.toBe(null);
    const corner = { ...(before?.from ?? { x: 0, y: 0 }) };

    // The private scroll, exactly as `viewermarks.test.ts` reaches it: there is
    // no public setter, and a wheel event would also move the pointer's meaning.
    (viewer as unknown as { scrollTo(top: number): void }).scrollTo(120);
    await settle();

    expect(viewer.drawPreview?.from).toEqual(corner);
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 300, clientY: 200 });
    viewer.destroy();
  });
});

/** Presses, drags through every point in turn, and releases. */
function scribble(points: { x: number; y: number }[]): void {
  const [first, ...rest] = points;
  if (!first) throw new Error("a scribble needs a starting point");
  dom.root.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: first.x,
    clientY: first.y,
    target: dom.root,
  });
  for (const at of rest) {
    dom.root.dispatch("pointermove", {
      pointerId: 1,
      clientX: at.x,
      clientY: at.y,
    });
  }
  const last = points[points.length - 1] ?? first;
  dom.root.dispatch("pointerup", {
    pointerId: 1,
    clientX: last.x,
    clientY: last.y,
  });
}

/** An Enter, which is what finishes a drawing. */
function enter(): void {
  dom.root.dispatch("keydown", {
    key: "Enter",
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    target: dom.root,
  });
}

/** The stroke at `which` of the drawing at `at`, as `x y x y ...`. */
function stroke(at = -1, which = 0): number[] {
  const made = drawn.at(at);
  if (!made) throw new Error("nothing was drawn");
  const only = made.shape.strokes[which];
  if (!only) throw new Error(`the drawing carried no stroke ${which}`);
  return only;
}

describe("drawing freehand", () => {
  it("commits strokes and no rectangle", async () => {
    // **Both halves, and the second is the one that can go wrong quietly.** A
    // drawing that also carried a quad would be refused by the model rather
    // than drawn wrong, so the assertion that it is empty is what says the
    // viewer knows which kind it is sending rather than getting away with it.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 200, y: 150 },
      { x: 300, y: 100 },
    ]);
    // Nothing yet: a drawing is finished by Enter, not by letting go.
    expect(drawn).toEqual([]);
    enter();

    expect(drawn).toHaveLength(1);
    expect(drawn[0]?.kind).toBe("ink");
    expect(drawn[0]?.shape.quads).toEqual([]);
    expect(drawn[0]?.shape.strokes).toHaveLength(1);
    // Three points, six numbers: the press, one kept move, and the release.
    expect(stroke()).toHaveLength(6);
    viewer.destroy();
  });

  it("keeps the tool armed when the pointer never moved", async () => {
    // A press with no movement is a reader who has not started, not a mark of
    // no length --- the same answer `boxQuad`'s minimum gives a box, and for
    // the same reason: spending the tool on it would cost them the command
    // with nothing on screen to say why.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    scribble([{ x: 100, y: 100 }]);

    expect(drawn).toEqual([]);
    expect(viewer.drawArmed).toBe("ink");
    // **And nothing was kept**, which is the assertion the tool-stays-armed
    // change made load-bearing. Before it, a stroke of one point would have
    // spent the tool and `drawArmed` was the reading that saw it; now the tool
    // is armed either way, so only the stroke count can tell a refused press
    // from a kept one --- a mutation deleting the bound survived until this
    // line existed. Zero rather than null: the tool is armed, and nothing was
    // kept, which is exactly the pair this has to tell apart.
    expect(viewer.drawnStrokes).toBe(0);
    viewer.destroy();
  });

  it("samples the pointer rather than keeping every event", async () => {
    // Twenty moves inside a tenth of a point, which a hand resting on a
    // trackpad produces in a fraction of a second. Without the sample every one
    // of them is a point in the file and a pair of numbers over the IPC
    // boundary, and none of them changes the line.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    const jitter = Array.from({ length: 20 }, (_, at) => ({
      x: 100 + at * 0.005,
      y: 100,
    }));
    scribble([...jitter, { x: 300, y: 100 }]);
    enter();

    // The press, the far point, and nothing from the jitter: 21 moves in and
    // two points out. A viewer that kept them all reports 21 or 22.
    expect(stroke()).toHaveLength(4);
    viewer.destroy();
  });

  it("keeps the point the pointer was released on", async () => {
    // **The sample can drop the last move, and this is the case where that
    // matters.** A stroke whose final movement is shorter than the sample would
    // otherwise end at the previous kept point, so every line is up to one
    // sample short --- invisible on a long sweep and the whole of a tick or the
    // crossing of a t.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 100 },
      // Well inside INK_SAMPLE of the point before it, so the sample drops it
      // and only the unconditional push at the end can put it back.
      { x: 300.05, y: 100 },
    ]);
    enter();

    const points = stroke();
    // Three points, and without the unconditional push it would be two: the
    // final move is inside the sample and is dropped on its way through.
    expect(points).toHaveLength(6);
    // **And the kept point is nearer than the sample would ever allow**, which
    // is what says it arrived by the push rather than by the sample. Compared
    // against its own neighbour rather than against the client coordinate: these
    // are the file's display space, and the numbers that went in are the root's
    // --- a comparison across those two is what the first draft of this line got
    // wrong, and it read as a mapping defect.
    const gap = (points[4] ?? 0) - (points[2] ?? 0);
    expect(gap).toBeGreaterThan(0);
    expect(gap).toBeLessThan(INK_SAMPLE);
    viewer.destroy();
  });

  it("stays armed so a drawing can be several strokes", async () => {
    // **The whole point of the increment, and the opposite of the box.**
    // `/InkList` is a list of lists so that one annotation holds several
    // strokes, and a drawing normally is several. Before this the window could
    // make exactly one, so `annot-probe --mode strokes` --- which sends two ---
    // was creating a document no reader of tpdf could.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 120 },
    ]);
    expect(viewer.drawArmed).toBe("ink");
    expect(drawn).toEqual([]);

    scribble([
      { x: 100, y: 300 },
      { x: 300, y: 320 },
    ]);
    expect(drawn).toEqual([]);

    enter();
    expect(drawn).toHaveLength(1);
    expect(drawn[0]?.shape.strokes).toHaveLength(2);
    // Both strokes, and each its own: flattened into one they would be four
    // points in a single entry, which is the join `/InkList` exists to prevent.
    expect(stroke(-1, 0)).toHaveLength(4);
    expect(stroke(-1, 1)).toHaveLength(4);
    expect(viewer.drawArmed).toBe(null);
    viewer.destroy();
  });

  it("throws the whole drawing away on Escape", async () => {
    // Escape has meant abandon since the box, so it means abandon here --- which
    // is exactly why finishing had to be a different key. A mode a reader can
    // only leave by discarding what they made is one they use once.
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 120 },
    ]);
    scribble([
      { x: 100, y: 300 },
      { x: 300, y: 320 },
    ]);
    expect(viewer.drawnStrokes).toBe(2);

    escape();
    expect(drawn).toEqual([]);
    expect(viewer.drawArmed).toBe(null);
    expect(viewer.drawnStrokes).toBe(null);
    viewer.destroy();
  });

  it("reports the drawing in the status, so the mode is visible", async () => {
    // Every other tool here is one-shot and there is nothing to be stuck in.
    // This one can be, so the window has to be able to say so.
    //
    // **Read through the accessor, which is what `ViewerStatus.drawing` is.**
    // The status field was its own expression until a mutation that emptied it
    // survived every test here: the tests asked the viewer, the window reads the
    // status, and the two were a copy of one another. They are one accessor now,
    // so this reading is the window's reading rather than a proxy for it --- and
    // driving the frame loop to observe `onStatus` in these tests would need the
    // tile IPC this file deliberately does not stub.
    const viewer = build();
    await settle();

    expect(viewer.drawnStrokes).toBe(null);
    // Zero, not null: armed, nothing drawn. The window says "press and drag"
    // for this and names the strokes for anything above it.
    viewer.armDraw("ink");
    expect(viewer.drawnStrokes).toBe(0);

    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 120 },
    ]);
    expect(viewer.drawnStrokes).toBe(1);

    enter();
    expect(viewer.drawnStrokes).toBe(null);
    viewer.destroy();
  });

  it("refuses a stroke that starts on another page", async () => {
    // An annotation belongs to one page, so a second stroke elsewhere is not
    // part of this drawing. Refused rather than dragged onto the first page,
    // which would put ink where nobody drew it.
    const viewer = build();
    viewer.setPages([
      { id: 1, source: 0, turns: 0 },
      { id: 2, source: 1, turns: 0 },
    ]);
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 120 },
    ]);
    expect(viewer.drawnStrokes).toBe(1);

    // Far enough down to land on the second page in this layout.
    scribble([
      { x: 100, y: 3000 },
      { x: 300, y: 3020 },
    ]);
    expect(viewer.drawnStrokes).toBe(1);

    enter();
    expect(drawn[0]?.shape.strokes).toHaveLength(1);
    viewer.destroy();
  });

  it("does nothing on Enter with no drawing in progress", async () => {
    // So the key falls through to whatever else claims it --- a focused link,
    // most of all, which the keyboard walk can leave behind and which `armDraw`
    // does not clear.
    const viewer = build();
    await settle();

    enter();
    expect(drawn).toEqual([]);
    viewer.destroy();
  });

  it("commits nothing when Escape ends the drag", async () => {
    const viewer = build();
    await settle();

    viewer.armDraw("ink");
    dom.root.dispatch("pointerdown", {
      button: 0,
      pointerId: 1,
      clientX: 100,
      clientY: 100,
      target: dom.root,
    });
    dom.root.dispatch("pointermove", { pointerId: 1, clientX: 300, clientY: 200 });
    escape();
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: 300, clientY: 200 });

    expect(drawn).toEqual([]);
    expect(viewer.drawArmed).toBe(null);
    viewer.destroy();
  });

  it("puts the points back in the file's space on a turned page", async () => {
    // The same assertion the box's own turn test makes, and it needs its own:
    // a box goes through `fileRectOn` once for its two corners and a drawing
    // goes through it once per point, so a mapping applied to the rectangle and
    // forgotten for the path is a defect no box could show.
    const viewer = build();
    viewer.setPages([{ id: 1, source: 0, turns: 1 }]);
    await settle();

    viewer.armDraw("ink");
    scribble([
      { x: 100, y: 100 },
      { x: 300, y: 100 },
    ]);
    enter();

    const turned = stroke();
    expect(turned).toHaveLength(4);
    // Drawn along a constant client y, so on a quarter-turned page the points
    // must share an x in the file's space and differ in y. An unturned viewer
    // reports the opposite, which is what makes this an assertion rather than a
    // restatement of the input.
    expect(turned[0]).toBeCloseTo(turned[2] ?? 0, 3);
    expect(turned[1]).not.toBeCloseTo(turned[3] ?? 0, 1);
    viewer.destroy();
  });
});
