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
import { BAR_HEIGHT, barSpot } from "./arrangebar";
import type { FakeElement } from "./testdom";

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
/** Beside each count: whether the page had another mark to pick with those. */
let mores: boolean[];
let moved: number[];
let resized: number[];
let removed: [number, number][];

beforeEach(() => {
  dom = installFakeDom();
  arranged = [];
  counts = [];
  mores = [];
  moved = [];
  resized = [];
  removed = [];
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
    onPicked: (count, more) => {
      counts.push(count);
      mores.push(more);
    },
    onMarkMoved: (id) => moved.push(id),
    onMarkResized: (id) => resized.push(id),
    onMarkRemove: (id, sweep) => removed.push([id, sweep]),
  });
  if (turns !== 0) {
    const pages: PageView[] = [{ id: pageId(1), source: { baseline: 0 }, turns }];
    viewer.setPages(pages);
  }
  viewer.setMarks(MARKS);
  return viewer;
}

/** When the next press happens, in milliseconds: each press a second after the last unless told otherwise. */
let clock = 1000;

function press(viewer: Viewer, id: number, shiftKey = false, after = 1000): void {
  const anchor = viewer.markAnchor(id);
  if (!anchor) throw new Error(`mark ${id} is not laid out`);
  const at = { clientX: (anchor.left + anchor.right) / 2, clientY: (anchor.top + anchor.bottom) / 2 };
  clock += after;
  dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, shiftKey, timeStamp: clock, ...at });
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

  it("says with each change whether the page has another mark to pick with those", async () => {
    const viewer = build();
    await settle();
    press(viewer, 2);
    press(viewer, 1, true);
    press(viewer, 3, true);
    // Two left to add, then one, then none: every rectangle of the page is picked.
    expect(mores).toEqual([true, true, false]);
    pressPaper();
    // Nothing picked has no page to look for another on.
    expect(counts.at(-1)).toBe(0);
    expect(mores.at(-1)).toBe(false);
  });

  it("does not count a mark on another page as one to pick with these", async () => {
    const seen: boolean[] = [];
    const viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 2,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onPicked: (_count, more) => seen.push(more),
    });
    const [first, second] = viewer.pageOrder;
    viewer.setMarks([
      { ...one(1, "field", [100, 100, 200, 120]), page: first!.id },
      { ...one(2, "field", [100, 200, 200, 220]), page: second!.id },
    ]);
    await settle();
    press(viewer, 1);
    expect(viewer.pickedMarks()).toEqual([1]);
    // Shift adds from the picked mark's own page, and that page has no other.
    expect(seen).toEqual([false]);
  });

  it("opens no note and moves nothing on a press with Shift", async () => {
    const viewer = build();
    await settle();
    press(viewer, 1, true);
    expect(viewer.markOpen).toBe(-1);
    // Nor do two of them one soon after the other: a press with Shift is
    // never the first or the second of the pair that opens a box.
    press(viewer, 1, true, 100);
    expect(viewer.markOpen).toBe(-1);
    // The control: two presses without Shift, one soon after the other, open it.
    press(viewer, 1);
    press(viewer, 1, false, 200);
    expect(viewer.markOpen).toBe(1);
    expect(moved).toEqual([]);
  });

  it("picks a rectangle on one press and opens its box on a second soon after", async () => {
    const viewer = build();
    await settle();
    press(viewer, 1);
    expect(viewer.pickedMarks()).toEqual([1]);
    expect(viewer.markOpen).toBe(-1);
    // A second press long after is another first press.
    press(viewer, 1, false, 451);
    expect(viewer.markOpen).toBe(-1);
    // And one on another rectangle is the first on that one.
    press(viewer, 2, false, 100);
    expect(viewer.markOpen).toBe(-1);
    press(viewer, 2, false, 450);
    expect(viewer.markOpen).toBe(2);
    // A press with Shift between the two is not the first of a pair.
    const other = build();
    await settle();
    press(other, 1, true);
    press(other, 1, true, 100);
    expect(other.markOpen).toBe(-1);
  });

  it("opens a mark made of the words under it on one press, as before", async () => {
    const viewer = build();
    await settle();
    press(viewer, 4);
    expect(viewer.markOpen).toBe(4);
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

  /**
   * Three pages, two fields on the second and one on the third, and the first
   * page deleted after `picked` was pressed: the second page is in slot 0 and
   * the third in the slot the second had.
   */
  async function afterDeletionAbove(picked: number): Promise<Viewer> {
    const viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 3,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onPicked: (count) => counts.push(count),
    });
    const [, second, third] = viewer.pageOrder;
    viewer.setMarks([
      { ...one(1, "field", [100, 100, 200, 120]), page: second!.id },
      { ...one(2, "field", [100, 200, 200, 220]), page: second!.id },
      { ...one(3, "field", [100, 100, 200, 120]), page: third!.id },
    ]);
    await settle();
    viewer.goToPage(1);
    press(viewer, picked);
    expect(viewer.pickedMarks()).toEqual([picked]);
    expect(viewer.setPages([second!, third!])).toBe(true);
    return viewer;
  }

  it("keeps_what_is_picked_on_its_page_when_a_page_above_is_deleted", async () => {
    // Shift adds a mark on the page the picked ones are on, and that page is
    // remembered as a slot. With the slot left at its old number, the page
    // that took it is the one Shift adds from.
    const viewer = await afterDeletionAbove(1);
    expect(viewer.pickedMarks()).toEqual([1]);
    viewer.goToPage(0);
    press(viewer, 2, true);
    expect(viewer.pickedMarks()).toEqual([1, 2]);
  });

  it("does_not_add_a_mark_from_the_page_that_took_the_picked_page's_slot", async () => {
    const viewer = await afterDeletionAbove(1);
    viewer.goToPage(1);
    press(viewer, 3, true);
    // Another page's mark replaces the picked ones, as it does in a document
    // nobody has edited. Both together would be marks of two pages picked.
    expect(viewer.pickedMarks()).toEqual([3]);
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

/** A press on a mark, a move of the pointer, and a release. */
function drag(viewer: Viewer, id: number, by: { x: number; y: number }): void {
  const anchor = viewer.markAnchor(id);
  if (!anchor) throw new Error(`mark ${id} is not laid out`);
  const from = { x: (anchor.left + anchor.right) / 2, y: (anchor.top + anchor.bottom) / 2 };
  dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, clientX: from.x, clientY: from.y });
  dom.root.dispatch("pointermove", { pointerId: 1, clientX: from.x + by.x, clientY: from.y + by.y });
  dom.root.dispatch("pointerup", { pointerId: 1, clientX: from.x + by.x, clientY: from.y + by.y });
}

function key(name: string, shiftKey = false, extra: Record<string, unknown> = {}): ReturnType<typeof vi.fn> {
  const preventDefault = vi.fn();
  dom.root.dispatch("keydown", {
    key: name, shiftKey, ctrlKey: false, metaKey: false, altKey: false, target: dom.root, preventDefault, ...extra,
  });
  return preventDefault;
}

const rectOf = (id: number) => MARKS.find((mark) => mark.id === id)!.quads;
const offsets = (moves: Move[]) =>
  moves.map((move) => [move.mark, +(move.rect[0] - rectOf(move.mark)[0]!).toFixed(3), +(move.rect[1] - rectOf(move.mark)[1]!).toFixed(3)]);

describe("dragging several picked marks", () => {
  it("moves all of them by what the one under the hand moved, as one gesture", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2, 3]);
    drag(viewer, 2, { x: 30, y: 20 });
    expect(arranged).toHaveLength(1);
    const moves = arranged[0]!.moves;
    expect(moves.map((move) => move.mark)).toEqual([2, 1, 3]);
    const [first, ...rest] = offsets(moves);
    expect(first![1]).toBeGreaterThan(0);
    expect(first![2]).toBeGreaterThan(0);
    for (const other of rest) expect(other.slice(1)).toEqual(first!.slice(1));
    // Each keeps its size.
    for (const move of moves) {
      const was = rectOf(move.mark);
      expect(move.rect[2] - move.rect[0]).toBeCloseTo(was[2]! - was[0]!, 6);
      expect(move.rect[3] - move.rect[1]).toBeCloseTo(was[3]! - was[1]!, 6);
    }
    // The move is the arrangement's report and not also a move of one mark.
    expect(moved).toEqual([]);
    expect(viewer.pickedMarks()).toEqual([1, 2, 3]);
  });

  it("stops all of them when one reaches the page's edge", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 3]);
    // Mark 3 is 40 points from the left edge; far more is asked.
    drag(viewer, 1, { x: -2000, y: 0 });
    expect(offsets(arranged[0]!.moves)).toEqual([[1, -40, 0], [3, -40, 0]]);
  });

  it("moves one mark alone when it is the only one picked, as a move and not an arrangement", async () => {
    const viewer = build();
    await settle();
    drag(viewer, 1, { x: 30, y: 20 });
    expect(moved).toEqual([1]);
    expect(arranged).toEqual([]);
  });

  it("resizes the one whose corner is taken and leaves the others", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2]);
    const anchor = viewer.markAnchor(2)!;
    const from = { x: anchor.right - 1, y: anchor.bottom - 1 };
    dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, clientX: from.x, clientY: from.y });
    dom.root.dispatch("pointermove", { pointerId: 1, clientX: from.x + 20, clientY: from.y + 10 });
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: from.x + 20, clientY: from.y + 10 });
    expect(resized).toEqual([2]);
    expect(arranged).toEqual([]);
  });

  it("journals nothing for a press on one of several that does not move", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2]);
    press(viewer, 2);
    expect(arranged).toEqual([]);
    expect(moved).toEqual([]);
  });
});

describe("the arrow keys with marks picked", () => {
  it("move them a point, or ten with Shift, as one gesture each", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2]);
    const taken = key("ArrowRight");
    expect(taken).toHaveBeenCalledTimes(1);
    expect(offsets(arranged[0]!.moves)).toEqual([[1, 1, 0], [2, 1, 0]]);
    key("ArrowUp", true);
    expect(offsets(arranged[1]!.moves)).toEqual([[1, 0, -10], [2, 0, -10]]);
    key("ArrowLeft");
    key("ArrowDown");
    expect(offsets(arranged[2]!.moves)).toEqual([[1, -1, 0], [2, -1, 0]]);
    expect(offsets(arranged[3]!.moves)).toEqual([[1, 0, 1], [2, 0, 1]]);
    expect(new Set(arranged.map((one) => one.sweep)).size).toBe(4);
  });

  it("move one mark picked alone", async () => {
    const viewer = build();
    await settle();
    viewer.pick([3]);
    key("ArrowRight", true);
    expect(arranged[0]?.moves).toEqual([{ mark: 3, rect: [50, 500, 80, 510] }]);
  });

  it("stop at the page's edge, and are still taken there", async () => {
    const viewer = build();
    await settle();
    viewer.setMarks([one(3, "field", [4, 500, 34, 510])]);
    viewer.pick([3]);
    key("ArrowLeft", true);
    expect(arranged[0]?.moves).toEqual([{ mark: 3, rect: [0, 500, 30, 510] }]);
    viewer.setMarks([one(3, "field", [0, 500, 30, 510])]);
    const taken = key("ArrowLeft");
    expect(arranged).toHaveLength(1);
    expect(taken).toHaveBeenCalledTimes(1);
  });

  it("are left to the page with nothing picked, and with a modifier held", async () => {
    const viewer = build();
    await settle();
    // Not taken: with nothing picked an arrow is the page's, to scroll with.
    expect(key("ArrowRight")).not.toHaveBeenCalled();
    viewer.pick([1]);
    key("ArrowRight", false, { metaKey: true });
    key("ArrowRight", false, { ctrlKey: true });
    key("ArrowRight", false, { altKey: true });
    key("a");
    expect(arranged).toEqual([]);
  });

  it("move what the reader sees as right on a turned page", async () => {
    const viewer = build(1);
    await settle();
    viewer.pick([1]);
    expect(viewer.nudgePicked(5, 0)).toBe(true);
    const rect = arranged[0]!.moves[0]!.rect;
    // A quarter turn: the reader's right is along the file's other axis.
    expect(rect[0]).toBe(100);
    expect(Math.abs(rect[1] - 100)).toBe(5);
  });
});

describe("where copies of the picked marks go", () => {
  it("is a step down and to the right of each, under a gesture of its own", async () => {
    const viewer = build();
    await settle();
    expect(viewer.copiesOfPicked()).toBeNull();
    viewer.pick([1, 2]);
    const got = viewer.copiesOfPicked()!;
    expect(got.rects).toEqual([
      { mark: 1, rect: [112, 112, 212, 132] },
      { mark: 2, rect: [262, 312, 462, 352] },
    ]);
    expect(got.sweep).toBeGreaterThan(0);
    expect(viewer.copiesOfPicked()!.sweep).not.toBe(got.sweep);
  });

  it("is as far as the page lets all of them go", async () => {
    const viewer = build();
    await settle();
    viewer.setMarks([one(1, "field", [100, 100, 200, 120]), one(2, "square", [500, 760, 595, 795])]);
    viewer.pick([1, 2]);
    expect(viewer.copiesOfPicked()!.rects).toEqual([
      { mark: 1, rect: [105, 105, 205, 125] },
      { mark: 2, rect: [505, 765, 600, 800] },
    ]);
  });
});

describe("removing what is picked", () => {
  it("takes several off under one gesture, and one alone under none", async () => {
    const viewer = build();
    await settle();
    expect(viewer.canRemoveMark).toBe(false);
    viewer.pick([1, 2, 3]);
    expect(viewer.canRemoveMark).toBe(true);
    expect(viewer.removeMarks()).toBe(true);
    expect(removed.map(([id]) => id)).toEqual([1, 2, 3]);
    expect(removed[0]![1]).toBeGreaterThan(0);
    expect(new Set(removed.map(([, sweep]) => sweep)).size).toBe(1);
    expect(viewer.pickedMarks()).toEqual([]);
    expect(counts.at(-1)).toBe(0);
    removed.length = 0;
    viewer.pick([2]);
    viewer.removeMarks();
    expect(removed).toEqual([[2, 0]]);
  });

  it("takes the mark whose box is open and leaves the picked ones", async () => {
    const viewer = build();
    await settle();
    press(viewer, 4);
    expect(viewer.canRemoveMark).toBe(true);
    expect(viewer.removeMarks()).toBe(true);
    expect(removed).toEqual([[4, 0]]);
    expect(viewer.markOpen).toBe(-1);
  });

  it("does nothing with none picked and no box open", async () => {
    const viewer = build();
    await settle();
    expect(viewer.removeMarks()).toBe(false);
    expect(removed).toEqual([]);
  });

  it("is what Delete and Backspace do with rectangles picked, and not otherwise", async () => {
    const viewer = build();
    await settle();
    expect(key("Delete")).not.toHaveBeenCalled();
    expect(key("Backspace")).not.toHaveBeenCalled();
    viewer.pick([1]);
    expect(key("Delete")).toHaveBeenCalledTimes(1);
    expect(removed).toEqual([[1, 0]]);
    viewer.pick([2]);
    key("Backspace");
    expect(removed.map(([id]) => id)).toEqual([1, 2]);
  });
});

/**
 * These two draw frames, which nothing above does, and a frame asks for
 * tiles. An answer that never comes is all they need: no pixel is read.
 */
function framesWithoutTiles(): void {
  let request = 0;
  tiles.nextRequestId.mockImplementation(() => ++request);
  tiles.fetchTile.mockReturnValue(new Promise(() => {}));
}

describe("the arrange bar in the viewer", () => {
  beforeEach(framesWithoutTiles);

  /** A viewer whose bar is answered by commands that record and align left. */
  function wired(marks: MarkView[] = MARKS): { viewer: Viewer; ran: string[] } {
    const ran: string[] = [];
    const viewer: Viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 1,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onMarksArranged: (moves, sweep) => arranged.push({ moves, sweep }),
      onPicked: (count) => counts.push(count),
      onArrangeCommand: (id) => ({
        title: `Arrange: ${id}`,
        enabled: viewer.pickedCount >= 2,
        why: null,
        run: () => {
          ran.push(id);
          viewer.arrangePicked("left");
        },
      }),
    });
    viewer.setMarks(marks);
    return { viewer, ran };
  }

  const bar = (viewer: Viewer): FakeElement => viewer.arrangeBarNode as unknown as FakeElement;
  const shown = (viewer: Viewer): boolean => bar(viewer).style.display === "flex";
  const at = (viewer: Viewer): [number, number] => [
    Number.parseFloat(bar(viewer).style.left ?? ""),
    Number.parseFloat(bar(viewer).style.top ?? ""),
  ];
  /** Where the bar belongs for these marks, from their own anchors. */
  function expected(viewer: Viewer, ids: number[]): [number, number] {
    const anchors = ids.map((id) => viewer.markAnchor(id)!);
    const spot = barSpot(
      { width: dom.root.clientWidth, height: dom.root.clientHeight },
      {
        left: Math.min(...anchors.map((one) => one.left)),
        top: Math.min(...anchors.map((one) => one.top)),
        right: Math.max(...anchors.map((one) => one.right)),
        bottom: Math.max(...anchors.map((one) => one.bottom)),
      },
    );
    if (!spot) throw new Error("the fixture leaves the bar no place");
    return [Math.round(spot.left), Math.round(spot.top)];
  }

  it("shows it beside the marks once two are picked, and takes it away at one", async () => {
    const { viewer } = wired();
    await settle();
    dom.runFrames();
    expect(shown(viewer)).toBe(false);
    press(viewer, 1);
    dom.runFrames();
    expect(shown(viewer)).toBe(false);
    press(viewer, 2, true);
    dom.runFrames();
    expect(shown(viewer)).toBe(true);
    expect(at(viewer)).toEqual(expected(viewer, [1, 2]));
    // Above both of them, clear of the upper one.
    expect(at(viewer)[1] + BAR_HEIGHT).toBeLessThan(viewer.markAnchor(1)!.top);
    // A third joins and the bar is round all three.
    press(viewer, 3, true);
    dom.runFrames();
    expect(at(viewer)).toEqual(expected(viewer, [1, 2, 3]));
    // Shift on two of them takes them out again: one left, no bar.
    press(viewer, 3, true);
    press(viewer, 2, true);
    dom.runFrames();
    expect(viewer.pickedMarks()).toEqual([1]);
    expect(shown(viewer)).toBe(false);
    // And none at all after a press on the paper.
    viewer.pick([1, 2]);
    dom.runFrames();
    expect(shown(viewer)).toBe(true);
    pressPaper();
    dom.runFrames();
    expect(shown(viewer)).toBe(false);
  });

  it("runs the command a button stands for and leaves the marks picked", async () => {
    const { viewer, ran } = wired();
    await settle();
    viewer.pick([2, 1]);
    dom.runFrames();
    const left = viewer.arrangeBarButtons.find((one) => one.id === "edit.alignLeft")!;
    expect(viewer.arrangeBarButtons).toHaveLength(10);
    const before = counts.length;
    (left.button as unknown as FakeElement).dispatch("pointerdown", { button: 0 });
    expect(ran).toEqual(["edit.alignLeft"]);
    expect(arranged).toHaveLength(1);
    expect(arranged[0]!.moves.map((move) => move.mark)).toEqual([1]);
    expect(viewer.pickedMarks()).toEqual([2, 1]);
    expect(counts.length).toBe(before);
    // The keyboard was not given to the button, and the arrow keys still
    // move what is picked.
    expect((left.button as unknown as FakeElement).focused).toBe(false);
    key("ArrowRight");
    expect(arranged).toHaveLength(2);
    expect(viewer.pickedMarks()).toEqual([2, 1]);
  });

  it("has no bar when the application answers for no command", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2]);
    dom.runFrames();
    expect(shown(viewer)).toBe(false);
  });

  it("steps aside while the box of one of the picked marks is open", async () => {
    const { viewer } = wired();
    await settle();
    viewer.pick([1, 2]);
    dom.runFrames();
    expect(shown(viewer)).toBe(true);
    // Two presses soon after each other on one of several open its box and
    // keep the others picked: the one way a box and a bar would meet.
    press(viewer, 1);
    press(viewer, 1, false, 200);
    expect(viewer.markOpen).toBe(1);
    expect(viewer.pickedMarks()).toEqual([1, 2]);
    dom.runFrames();
    expect(shown(viewer)).toBe(false);
    viewer.closeMark();
    dom.runFrames();
    expect(shown(viewer)).toBe(true);
  });

  it("follows marks that are dragged together, the ones that are not under the hand too", async () => {
    const { viewer } = wired();
    await settle();
    viewer.pick([1, 2]);
    dom.runFrames();
    const was = { bar: at(viewer), one: viewer.markAnchor(1)!, two: viewer.markAnchor(2)! };
    const from = { x: (was.two.left + was.two.right) / 2, y: (was.two.top + was.two.bottom) / 2 };
    dom.root.dispatch("pointerdown", { button: 0, pointerId: 1, target: dom.root, clientX: from.x, clientY: from.y });
    dom.root.dispatch("pointermove", { pointerId: 1, clientX: from.x + 30, clientY: from.y + 40 });
    dom.runFrames();
    const now = { one: viewer.markAnchor(1)!, two: viewer.markAnchor(2)! };
    expect(now.two.top - was.two.top).toBeGreaterThan(0);
    // The mark dragged along is where it is drawn, by the same offset.
    expect(now.one.left - was.one.left).toBeCloseTo(now.two.left - was.two.left, 6);
    expect(now.one.top - was.one.top).toBeCloseTo(now.two.top - was.two.top, 6);
    expect(at(viewer)).toEqual(expected(viewer, [1, 2]));
    expect(at(viewer)).not.toEqual(was.bar);
    dom.root.dispatch("pointerup", { pointerId: 1, clientX: from.x + 30, clientY: from.y + 40 });
  });

  it("goes with the viewer", async () => {
    const { viewer } = wired();
    await settle();
    viewer.pick([1, 2]);
    dom.runFrames();
    expect(bar(viewer).parent).toBe(dom.root);
    viewer.destroy();
    expect(bar(viewer).parent).toBeNull();
  });
});

describe("the line about Shift in a mark's box", () => {
  beforeEach(framesWithoutTiles);

  const line = (viewer: Viewer): boolean => !(viewer.markTogetherLine as unknown as { hidden: boolean }).hidden;

  /** Opens a placed rectangle's box as a reader does: two presses soon after each other. */
  function open(viewer: Viewer, id: number): void {
    press(viewer, id);
    press(viewer, id, false, 200);
    expect(viewer.markOpen).toBe(id);
  }

  it("is shown for a rectangle picked alone on a page that has another", async () => {
    const viewer = build();
    await settle();
    open(viewer, 1);
    expect(viewer.pickedMarks()).toEqual([1]);
    expect(line(viewer)).toBe(true);
  });

  it("is not shown for a mark made of the words under it", async () => {
    const viewer = build();
    await settle();
    press(viewer, 4);
    expect(viewer.markOpen).toBe(4);
    expect(line(viewer)).toBe(false);
  });

  it("is not shown when the page has no other rectangle to pick", async () => {
    const viewer = build();
    await settle();
    // The field, and a highlight that a press with Shift does not pick.
    viewer.setMarks([MARKS[0]!, MARKS[3]!]);
    open(viewer, 1);
    expect(line(viewer)).toBe(false);
    // Nor when the only other rectangle is on another page.
    viewer.setMarks([MARKS[0]!, { ...MARKS[1]!, page: pageId(2) }]);
    viewer.closeMark();
    open(viewer, 1);
    expect(line(viewer)).toBe(false);
    // The control: the same rectangle on this page, and it is.
    viewer.setMarks([MARKS[0]!, MARKS[1]!]);
    viewer.closeMark();
    open(viewer, 1);
    expect(line(viewer)).toBe(true);
  });

  it("is not shown with several picked, or with this one not picked", async () => {
    const viewer = build();
    await settle();
    viewer.pick([1, 2]);
    open(viewer, 1);
    expect(viewer.pickedMarks()).toEqual([1, 2]);
    expect(line(viewer)).toBe(false);
    // Opened from the keyboard walk with another mark picked.
    viewer.closeMark();
    viewer.pick([2]);
    viewer.showMark(1, false);
    expect(viewer.markOpen).toBe(1);
    expect(line(viewer)).toBe(false);
    // And with nothing picked at all.
    viewer.closeMark();
    viewer.pick([]);
    viewer.showMark(1, false);
    expect(line(viewer)).toBe(false);
  });

  it("goes when it stops being true under the open box, and comes back", async () => {
    const viewer = build();
    await settle();
    open(viewer, 1);
    expect(line(viewer)).toBe(true);
    // Shift on the open mark lets it go; the box stays.
    press(viewer, 1, true);
    expect(viewer.markOpen).toBe(1);
    expect(viewer.pickedMarks()).toEqual([]);
    dom.runFrames();
    expect(line(viewer)).toBe(false);
    press(viewer, 1, true);
    dom.runFrames();
    expect(line(viewer)).toBe(true);
    // The other rectangles are removed under it.
    viewer.setMarks([MARKS[0]!, MARKS[3]!]);
    dom.runFrames();
    expect(viewer.markOpen).toBe(1);
    expect(line(viewer)).toBe(false);
  });
});
