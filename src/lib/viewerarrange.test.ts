/**
 * Picking several placed rectangles and arranging them.
 *
 * The arithmetic is `arrange.test.ts`'s. What is tested here is the join: which
 * press picks, what a pick does not do, and that an arrangement worked out in
 * the laid-out page leaves in the file's space, as one gesture.
 *
 * A turned page is in here for `viewermove.test.ts`'s reason: upright the two
 * spaces are the same numbers, and a viewer that skipped the mapping would
 * pass every upright assertion.
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

type Move = { mark: number; rect: [number, number, number, number] };

let dom: FakeDom;
let arranged: { moves: Move[]; sweep: number }[];
let counts: number[];
let moved: number[];

beforeEach(() => {
  dom = installFakeDom();
  arranged = [];
  counts = [];
  moved = [];
  core.invoke.mockResolvedValue(null);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

function one(id: number, kind: MarkKind, quads: number[]): MarkView {
  return {
    id, kind, stamp: null, page: pageId(1), quads, strokes: [],
    color: [0.85, 0.15, 0.15], width: INK_WIDTH, note: "", lines: [],
  };
}

/** Three rectangles of different sizes, none lined up with another, and a highlight. */
const MARKS: MarkView[] = [
  one(1, "field", [100, 100, 200, 120]),
  one(2, "square", [250, 300, 450, 340]),
  one(3, "textbox", [40, 500, 70, 510]),
  one(4, "highlight", [300, 600, 400, 612]),
];

function build(turns = 0): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 1,
    pages: [{ width_pt: 600, height_pt: 800 }],
    onMarksArranged: (moves, sweep) => arranged.push({ moves, sweep }),
    onPicked: (count) => counts.push(count),
    onMarkMoved: (id) => moved.push(id),
  });
  if (turns !== 0) {
    const pages: PageView[] = [{ id: pageId(1), source: { baseline: 0 }, turns }];
    viewer.setPages(pages);
  }
  viewer.setMarks(MARKS);
  return viewer;
}

function press(viewer: Viewer, id: number, shiftKey = false): void {
  const anchor = viewer.markAnchor(id);
  if (!anchor) throw new Error(`mark ${id} is not laid out`);
  const at = { clientX: (anchor.left + anchor.right) / 2, clientY: (anchor.top + anchor.bottom) / 2 };
  dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, shiftKey, ...at });
  dom.root.dispatch("pointerup", { pointerId: 1, ...at });
}

function pressPaper(): void {
  const at = { clientX: 5, clientY: 5 };
  dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, ...at });
  dom.root.dispatch("pointerup", { pointerId: 1, ...at });
}

describe("picking marks to arrange", () => {
  it("picks one on a plain press and adds others with Shift, in the order pressed", async () => {
    const viewer = build();
    await settle();
    press(viewer, 2);
    expect(viewer.pickedMarks()).toEqual([2]);
    press(viewer, 1, true);
    press(viewer, 3, true);
    expect(viewer.pickedMarks()).toEqual([2, 1, 3]);
    expect(viewer.pickedCount).toBe(3);
    expect(counts).toEqual([1, 2, 3]);
    // Shift on one already picked takes it out.
    press(viewer, 1, true);
    expect(viewer.pickedMarks()).toEqual([2, 3]);
  });

  it("opens no note and moves nothing on a press with Shift", async () => {
    const viewer = build();
    await settle();
    press(viewer, 1, true);
    expect(viewer.markOpen).toBe(-1);
    // The control: the same press without Shift opens the mark's note.
    press(viewer, 1);
    expect(viewer.markOpen).toBe(1);
    expect(moved).toEqual([]);
  });

  it("does not pick a mark that is made of the words under it", async () => {
    const viewer = build();
    await settle();
    press(viewer, 1);
    press(viewer, 4, true);
    expect(viewer.pickedMarks()).toEqual([]);
  });

  it("keeps several picked when one of them is pressed, and drops them on a press elsewhere", async () => {
    const viewer = build();
    await settle();
    press(viewer, 1);
    press(viewer, 2, true);
    press(viewer, 2);
    expect(viewer.pickedMarks()).toEqual([1, 2]);
    // One that is not among them replaces them.
    press(viewer, 3);
    expect(viewer.pickedMarks()).toEqual([3]);
    press(viewer, 1, true);
    pressPaper();
    expect(viewer.pickedMarks()).toEqual([]);
    expect(counts.at(-1)).toBe(0);
  });

  it("forgets a picked mark the model no longer has", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2, 3]);
    viewer.setMarks(MARKS.filter((mark) => mark.id !== 2));
    expect(viewer.pickedMarks()).toEqual([1, 3]);
    expect(counts.at(-1)).toBe(2);
  });
});

describe("arranging the picked marks", () => {
  it("reports each mark that moved, with its new rectangle, as one named gesture", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2, 3]);
    expect(viewer.arrangePicked("left")).toBe(true);
    expect(arranged).toHaveLength(1);
    // The first picked is where it was and is not among them.
    expect(arranged[0]?.moves).toEqual([
      { mark: 2, rect: [100, 300, 300, 340] },
      { mark: 3, rect: [100, 500, 130, 510] },
    ]);
    expect(arranged[0]?.sweep).toBeGreaterThan(0);
    // A second arrangement is a second gesture.
    expect(viewer.arrangePicked("top")).toBe(true);
    expect(arranged[1]?.sweep).not.toBe(arranged[0]?.sweep);
  });

  it("follows the first one picked", async () => {
    const viewer = build();
    await settle();
    viewer.pick([2, 1]);
    viewer.arrangePicked("left");
    expect(arranged[0]?.moves).toEqual([{ mark: 1, rect: [250, 100, 350, 120] }]);
  });

  it("says nothing was done when too few are picked or they are already in place", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1]);
    expect(viewer.arrangePicked("left")).toBe(false);
    viewer.pick([1, 2]);
    expect(viewer.arrangePicked("distributeAcross")).toBe(false);
    viewer.setMarks([one(1, "field", [100, 100, 200, 120]), one(2, "square", [100, 300, 300, 340])]);
    viewer.pick([1, 2]);
    expect(viewer.arrangePicked("left")).toBe(false);
    expect(arranged).toEqual([]);
  });

  it("aligns what the reader sees as left on a turned page, and reports it in the file's space", async () => {
    const viewer = build(1);
    await settle();
    viewer.pick([1, 2]);
    expect(viewer.arrangePicked("left")).toBe(true);
    const rect = arranged[0]?.moves[0]?.rect ?? [0, 0, 0, 0];
    // The page is turned a quarter, so the edge on the reader's left is the
    // file's bottom edge: mark 2 keeps its columns and its size, and its
    // bottom goes to mark 1's.
    expect(rect).toEqual([250, 80, 450, 120]);
    // And the page it is centred on is the page as it lies: 800 across.
    viewer.pick([1]);
    expect(viewer.arrangePicked("pageCenter")).toBe(true);
    expect(arranged[1]?.moves).toEqual([{ mark: 1, rect: [100, 390, 200, 410] }]);
  });
});
