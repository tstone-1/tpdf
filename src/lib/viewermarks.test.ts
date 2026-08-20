/**
 * Reaching the reader's own marks from the keyboard, and not from the page.
 *
 * Two halves of one increment, and they are the same fact seen from either side.
 * The walk puts a reader in the note box without a pointer; the guard is what
 * makes that box safe to be in. Before it, every key this viewer handles fired
 * while the reader was typing --- measured, not inferred: "n" turned the page,
 * Home jumped to the start, the space bar scrolled, ⌘R rotated the view. The
 * handler predates the only text field that has ever been inside its root.
 *
 * The guard's tests all come in pairs. A key delivered from a text field must do
 * nothing **and** the same key delivered from the page must do the thing --- a
 * guard tested only on its refusal is satisfied by a viewer that ignores every
 * key, which is the trap this repository records as a control that cannot fail.
 *
 * Every test below was checked by mutating `viewer.ts`, `pages.ts` or `keys.ts`
 * and confirming it went red; `scripts/mutate_frontend.py` holds the mutations.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { markWalk, PageMap, unedited, type MarkView, type PageView } from "./pages";
import { installFakeDom, settle, type FakeDom } from "./testdom";
import { Viewer } from "./viewer";

/** Reaches the viewer's private scroll, exactly as `viewerturns.test.ts` does. */
interface Placing {
  scrollTo(top: number): void;
  marks: readonly MarkView[];
  anchorForMark(mark: MarkView): {
    top: number;
    bottom: number;
  } | null;
  viewportSize(): { width: number; height: number };
}

/**
 * Where a mark's note would hang, in the window.
 *
 * The viewer's own placement rather than a second computation here: a rectangle
 * derived independently would be a second implementation of the same turn, which
 * is the drift this repository has a trap about. It can still fail --- `showMark`
 * scrolls *by* a target and this asserts the *result*, so a wrong target leaves
 * the anchor off screen exactly as no scroll at all would.
 */
function noteAnchor(
  viewer: Viewer,
  id: number,
): { top: number; bottom: number } | null {
  const inner = viewer as unknown as Placing;
  const mark = inner.marks.find((item) => item.id === id);
  return mark ? inner.anchorForMark(mark) : null;
}

/** Whether the note field has the keyboard, as the fake DOM records it. */
function noteFocused(viewer: Viewer): boolean {
  return (viewer.markNoteField as unknown as { focused: boolean }).focused;
}

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
const tiles = vi.hoisted(() => ({
  fetchTile: vi.fn(),
  cancelTile: vi.fn(),
  nextRequestId: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => core);
vi.mock("./tiles", () => tiles);

let dom: FakeDom;
let errors: string[];

beforeEach(() => {
  dom = installFakeDom();
  errors = [];
  core.invoke.mockResolvedValue(null);
});

afterEach(() => {
  dom.restore();
  vi.clearAllMocks();
});

/** A four-page document with whatever marks a test wants on it. */
function build(marks: MarkView[] = [], pages?: PageView[]): Viewer {
  const viewer = new Viewer(dom.root as unknown as HTMLElement, {
    doc: 1,
    pageCount: 4,
    pages: [{ width_pt: 600, height_pt: 800 }],
    onError: (message) => errors.push(message),
  });
  if (pages) viewer.setPages(pages);
  viewer.setMarks(marks);
  return viewer;
}

/** One highlight, on the page with `page` as its **id**, at `top` points down. */
function mark(id: number, page: number, top: number): MarkView {
  return {
    id,
    kind: "highlight",
    page,
    quads: [100, top, 300, top + 14],
    strokes: [],
    color: [1, 0.9, 0.2],
    note: "",
  };
}

/**
 * A key event shaped the way `matches` reads one.
 *
 * The four modifier fields are not padding. `matches` tests shift and alt in
 * *both* directions, so an event that omits them has `undefined !== false` and
 * matches no chorded binding at all --- which made the first version of this
 * probe report that ⌘R and ⌘+ were already guarded when nothing was.
 */
function key(
  k: string,
  from: "page" | "field",
  extra: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    key: k,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    ...extra,
    target:
      from === "field"
        ? { tagName: "TEXTAREA", isContentEditable: false }
        : dom.root,
  };
}

describe("recolouring the mark whose note is open", () => {
  /** The viewer, with a recorder for what it asks the model to do. */
  function withRecorder(marks: MarkView[]): {
    viewer: Viewer;
    asked: string[];
  } {
    const asked: string[] = [];
    const viewer = new Viewer(dom.root as unknown as HTMLElement, {
      doc: 1,
      pageCount: 4,
      pages: [{ width_pt: 600, height_pt: 800 }],
      onError: (message) => errors.push(message),
      onMarkRecolor: (id, color) => asked.push(`${id}:${color.join(",")}`),
    });
    viewer.setMarks(marks);
    return { viewer, asked };
  }

  it("asks for the colour, naming the open mark and no other", async () => {
    const { viewer, asked } = withRecorder([mark(4, 1, 100), mark(5, 1, 300)]);
    await settle();

    viewer.showMark(5);
    expect(viewer.recolorOpenMark([0.35, 0.8, 0.35])).toBe(true);
    expect(asked).toEqual(["5:0.35,0.8,0.35"]);
    viewer.destroy();
  });

  it("asks for nothing when no note is open", async () => {
    // The pair, and it is what makes the check above an assertion about *which*
    // mark: a viewer that recoloured the first mark it held would pass that one
    // whenever the open mark happened to be first, and this one never.
    const { viewer, asked } = withRecorder([mark(4, 1, 100)]);
    await settle();

    expect(viewer.recolorOpenMark([0.35, 0.8, 0.35])).toBe(false);
    expect(asked).toEqual([]);
    viewer.destroy();
  });

  it("asks for nothing when the mark is already that colour", async () => {
    // The comparison the swatch row also makes, made again here because this is
    // the palette's route in and it reaches no row. An undo step that changes
    // nothing is worse than no command at all.
    const { viewer, asked } = withRecorder([mark(4, 1, 100)]);
    await settle();

    viewer.showMark(4);
    expect(viewer.recolorOpenMark([1, 0.9, 0.2])).toBe(false);
    expect(asked).toEqual([]);
    viewer.destroy();
  });

  it("reads the default swatch as the open mark's own kind's colour", async () => {
    // `null` is "each kind's own", and the kind is the *mark's*. A red
    // underline is the case that discriminates: resolving it against the
    // highlight's yellow --- or against any one colour --- would recolour it,
    // and a fixture of highlights alone could not tell.
    const underline: MarkView = { ...mark(4, 1, 100), kind: "underline" };
    const { viewer, asked } = withRecorder([
      { ...underline, color: [0.85, 0.15, 0.15] },
    ]);
    await settle();

    viewer.showMark(4);
    expect(viewer.recolorOpenMark(null)).toBe(false);
    expect(asked).toEqual([]);

    // And it does act when the mark is *not* its kind's colour, or the check
    // above would be satisfied by a viewer that refuses every default.
    viewer.setMarks([{ ...underline, color: [0.35, 0.8, 0.35] }]);
    viewer.showMark(4);
    expect(viewer.recolorOpenMark(null)).toBe(true);
    expect(asked).toEqual(["4:0.85,0.15,0.15"]);
    viewer.destroy();
  });
});

describe("keys that went to a note", () => {
  it("scroll the page when they came from the page and not when they did not", async () => {
    const viewer = build();
    await settle();

    (viewer as unknown as Placing).scrollTo(2000);
    dom.root.dispatch("keydown", key("ArrowDown", "field"));
    const typedInto = viewer.offset;
    dom.root.dispatch("keydown", key("ArrowDown", "page"));
    const pressedOn = viewer.offset;

    expect(typedInto).toBe(2000);
    expect(pressedOn).toBeGreaterThan(2000);
    viewer.destroy();
  });

  it("do not jump to the start of the document", async () => {
    const viewer = build();
    await settle();

    (viewer as unknown as Placing).scrollTo(2000);
    dom.root.dispatch("keydown", key("Home", "field"));
    const typedInto = viewer.offset;
    dom.root.dispatch("keydown", key("Home", "page"));

    expect(typedInto).toBe(2000);
    expect(viewer.offset).toBe(0);
    viewer.destroy();
  });

  it("do not turn the view", async () => {
    const viewer = build();
    await settle();

    dom.root.dispatch("keydown", key("r", "field", { metaKey: true }));
    const typedInto = viewer.rotation;
    dom.root.dispatch("keydown", key("r", "page", { metaKey: true }));

    expect(typedInto).toBe(0);
    expect(viewer.rotation).toBe(1);
    viewer.destroy();
  });

  it("are refused by what a field is, not by what the key is", async () => {
    // A contenteditable and a `<select>` are text fields too, and neither is a
    // TEXTAREA. The guard reads `keys.ts`'s answer rather than a tag list of
    // its own, and this is what says so from the viewer's side.
    const viewer = build();
    await settle();

    (viewer as unknown as Placing).scrollTo(2000);
    dom.root.dispatch("keydown", {
      ...key("ArrowDown", "page"),
      target: { tagName: "DIV", isContentEditable: true },
    });
    expect(viewer.offset).toBe(2000);

    dom.root.dispatch("keydown", {
      ...key("ArrowDown", "page"),
      target: { tagName: "SELECT", isContentEditable: false },
    });
    expect(viewer.offset).toBe(2000);
    viewer.destroy();
  });
});

describe("markWalk", () => {
  it("orders marks by the slot their page is in, not by its id", () => {
    // The whole reason this is not `[...marks].sort()`. Page id 3 has been moved
    // to the front, so its mark comes first -- and nothing about either mark
    // changed. Ids ascend the other way, so a walk that read `mark.page` as a
    // position would produce exactly the reverse.
    const moved = new PageMap([
      { id: 3, source: 2, turns: 0 },
      { id: 1, source: 0, turns: 0 },
      { id: 2, source: 1, turns: 0 },
    ]);
    const walk = markWalk([mark(10, 1, 100), mark(11, 3, 100)], moved);
    expect(walk.map((item) => item.id)).toEqual([11, 10]);
    expect(walk.map((item) => item.page)).toEqual([0, 1]);
  });

  it("orders two marks on one page down the page", () => {
    const walk = markWalk(
      [mark(10, 1, 400), mark(11, 1, 100)],
      unedited(2),
    );
    expect(walk.map((item) => item.id)).toEqual([11, 10]);
  });

  it("is a total order for two marks with the same top edge", () => {
    // A reader marking one line twice. Without the id tiebreak, which is "next"
    // depends on the sort's stability rather than on a rule.
    const walk = markWalk(
      [mark(11, 1, 100), mark(10, 1, 100)],
      unedited(2),
    );
    expect(walk.map((item) => item.id)).toEqual([10, 11]);
  });

  it("takes the union of a mark's rectangles, not its first one", () => {
    // A highlight across three lines. Its place in the walk is its topmost
    // edge, and a walk that read `quads[1]` alone would be right only when the
    // model happened to emit the lines in order.
    const across: MarkView = {
      ...mark(10, 1, 0),
      quads: [100, 300, 300, 314, 100, 100, 300, 114],
    };
    const walk = markWalk([across], unedited(2));
    expect(walk[0]?.rect).toEqual([100, 100, 300, 314]);
  });

  it("leaves out a mark whose page is gone", () => {
    const walk = markWalk([mark(10, 1, 100), mark(11, 9, 100)], unedited(2));
    expect(walk.map((item) => item.id)).toEqual([10]);
  });
});

describe("stepMark", () => {
  it("opens the next mark's note without taking the keyboard off the page", async () => {
    const viewer = build([mark(10, 1, 100), mark(11, 1, 400)]);
    await settle();

    expect(viewer.stepMark(1)).toBe(true);
    expect(viewer.markOpen).toBe(10);
    // The keyboard stays on the page, which is what lets the next press step
    // again -- the guard above would send it to the field instead.
    expect(noteFocused(viewer)).toBe(false);

    expect(viewer.stepMark(1)).toBe(true);
    expect(viewer.markOpen).toBe(11);
    viewer.destroy();
  });

  it("stops at each end rather than wrapping, and says so", async () => {
    const viewer = build([mark(10, 1, 100), mark(11, 1, 400)]);
    await settle();

    viewer.stepMark(1);
    viewer.stepMark(1);
    expect(viewer.stepMark(1)).toBe(false);
    expect(viewer.markOpen).toBe(11);
    expect(errors).toEqual(["No further marks."]);
    viewer.destroy();
  });

  it("says so on a document the reader has not marked", async () => {
    const viewer = build([]);
    await settle();

    expect(viewer.stepMark(1)).toBe(false);
    expect(errors).toEqual(["You have not marked anything in this document."]);
    viewer.destroy();
  });

  it("starts from where the reader is looking, not from the first mark", async () => {
    // The property `stepAlong` has and a plain index walk does not: a reader who
    // scrolled to page 3 and pressed the key means the next mark *there*.
    const viewer = build([mark(10, 1, 100), mark(11, 4, 100)]);
    await settle();

    (viewer as unknown as Placing).scrollTo(viewer.pageTopCss(3));
    expect(viewer.stepMark(1)).toBe(true);
    expect(viewer.markOpen).toBe(11);
    viewer.destroy();
  });

  it("scrolls to a mark that is off screen", async () => {
    const viewer = build([mark(10, 4, 100)]);
    await settle();

    const before = viewer.offset;
    expect(viewer.stepMark(1)).toBe(true);
    expect(viewer.offset).toBeGreaterThan(before);
    // On screen once it arrives: the note box anchors to the mark, and a box
    // anchored below the window clamps itself into view and points at nothing.
    const at = noteAnchor(viewer, 10);
    const height = (viewer as unknown as Placing).viewportSize().height;
    expect(at).not.toBeNull();
    expect(at?.top).toBeLessThan(height);
    expect(at?.bottom).toBeGreaterThan(0);
    viewer.destroy();
  });
});

describe("Enter with a note open", () => {
  it("moves the keyboard into the note", async () => {
    const viewer = build([mark(10, 1, 100)]);
    await settle();

    viewer.stepMark(1);
    expect(noteFocused(viewer)).toBe(false);
    dom.root.dispatch("keydown", key("Enter", "page"));
    expect(noteFocused(viewer)).toBe(true);
    viewer.destroy();
  });

  it("still reaches the focused link when no note is open", async () => {
    // The control for the arm's guard, and it needs a link rather than an
    // absence: with no note open, "did nothing" is what a viewer that focused a
    // hidden field looks like *and* what a correct one looks like, because the
    // popup's own guard refuses that focus. Two mechanisms with one limit, which
    // makes either untestable on its own. Enter reaching the link arm is the
    // observable that separates them.
    const viewer = build([mark(10, 1, 100)]);
    viewer.setLinks([
      {
        id: 1,
        page: 0,
        rect: [100, 100, 300, 120],
        target: { kind: "page", page: 3, top_pt: null },
      },
    ]);
    await settle();

    viewer.stepLink(1);
    dom.root.dispatch("keydown", key("Enter", "page"));
    await settle();
    expect(viewer.position.page).toBe(3);
    viewer.destroy();
  });
});
