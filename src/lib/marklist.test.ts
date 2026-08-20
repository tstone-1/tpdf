import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { MarkList } from "./marklist";
import { markRows, PageMap, unedited, type MarkKind, type MarkView } from "./pages";
import { installFakeDom, type FakeDom } from "./testdom";

/** One mark, with the fields a test is not about filled in plausibly. */
function mark(over: Partial<MarkView> & { id: number }): MarkView {
  return {
    kind: "highlight",
    page: 1,
    quads: [100, 100, 300, 114],
    strokes: [],
    color: [1, 0.9, 0.2],
    note: "",
    lines: [],
    ...over,
  };
}

describe("markRows", () => {
  it("lists marks in the walk's order, not the order they were made", () => {
    // The whole reason this wraps `markWalk` rather than sorting again. Page id
    // 3 has been moved to the front, so its mark comes first --- and the panel
    // and the keyboard walk have to agree about that, because a reader uses
    // both in the same minute.
    const moved = new PageMap([
      { id: 3, source: 2, turns: 0 },
      { id: 1, source: 0, turns: 0 },
    ]);
    const rows = markRows([mark({ id: 10, page: 1 }), mark({ id: 11, page: 3 })], moved);
    expect(rows.map((row) => row.mark.id)).toEqual([11, 10]);
    expect(rows.map((row) => row.page)).toEqual([0, 1]);
  });

  it("keeps a mark the walk could not place, with no page against it", () => {
    // Built by hand, because the model refuses to make one: `annotate` rejects a
    // mark that covers nothing and `snapshot` walks the live pages, so this is
    // the malformed fixture a guard with no reachable input needs. What it
    // guards against is a panel that silently drops a row, which tells a reader
    // their mark is gone.
    const rows = markRows(
      [mark({ id: 10 }), mark({ id: 11, quads: [] })],
      unedited(2),
    );
    expect(rows.map((row) => row.mark.id)).toEqual([10, 11]);
    expect(rows.map((row) => row.page)).toEqual([0, null]);
  });
});

describe("MarkList", () => {
  let dom: FakeDom;
  let picked: number[];

  beforeEach(() => {
    dom = installFakeDom();
    picked = [];
  });

  afterEach(() => {
    dom.restore();
  });

  function panel(): MarkList {
    return new MarkList(dom.root as unknown as HTMLElement, {
      onPick: (id) => picked.push(id),
    });
  }

  function show(list: MarkList, marks: MarkView[], pages = unedited(3)): void {
    list.setMarks(markRows(marks, pages));
  }

  it("says the reader has marked nothing, before anything is pushed at it", () => {
    // One state rather than the comments panel's three. The model is in this
    // process and answers every time, so there is nothing to say "still
    // reading" about --- and a placeholder that said it would be a lie a reader
    // could sit and watch.
    const list = panel();
    expect(list.rowCount).toBe(0);
    const text = (dom.root.children[1]?.children[0]?.textContent ?? "") as string;
    expect(text).toContain("not marked anything");
  });

  it("shows the note, the kind and the page", () => {
    const list = panel();
    show(list, [mark({ id: 7, page: 3, kind: "strikeout", note: "wrong figure" })]);
    expect(list.rowText(7)).toEqual({
      note: "wrong figure",
      kind: "Strikeout",
      page: "3",
    });
  });

  it("calls each kind what the note box calls it", () => {
    // One table, in `markpopup.ts`. Two would agree until somebody renamed one,
    // and the failure is a mark called an Ellipse in the panel and a Circle in
    // the box that opens when you press it. These three are the kinds whose
    // reader's word is neither the PDF subtype nor the serde name, so they are
    // the ones a second table would get wrong.
    const list = panel();
    const kinds: MarkKind[] = ["note", "square", "ink"];
    show(
      list,
      kinds.map((kind, at) => mark({ id: at, kind })),
    );
    expect(kinds.map((_, at) => list.rowText(at).kind)).toEqual([
      "Comment",
      "Box",
      "Drawing",
    ]);
  });

  it("says a mark has no note rather than drawing an empty row", () => {
    const list = panel();
    show(list, [mark({ id: 7, note: "   " })]);
    expect(list.rowText(7).note).toBe("No note");
  });

  it("flattens a text box's own lines into the one the row has", () => {
    // A text box's note *is* the mark, so it is the one kind whose note
    // routinely has newlines in it. A row is one line high.
    const list = panel();
    show(list, [mark({ id: 7, kind: "textbox", note: "first\nsecond\n\nthird" })]);
    expect(list.rowText(7).note).toBe("first second third");
  });

  it("is one tab stop", () => {
    const list = panel();
    show(list, [mark({ id: 0 }), mark({ id: 1 }), mark({ id: 2 })]);
    const tabbable = [0, 1, 2].filter((id) => list.elementFor(id)?.tabIndex === 0);
    expect(tabbable).toEqual([0]);
  });

  it("moves the roving tabindex with the arrow keys", () => {
    const list = panel();
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] }), mark({ id: 1 })]);
    dom.root.children[1]?.dispatch("keydown", { key: "ArrowDown" });
    expect(list.focusedId).toBe(1);
    expect(list.elementFor(1)?.tabIndex).toBe(0);
    expect(list.elementFor(0)?.tabIndex).toBe(-1);
  });

  it("activates the row the key landed on, not the one it remembered", () => {
    // The stale-focus-mirror trap: a window without system focus moves
    // `activeElement` without delivering `focusin`, so a handler reading its own
    // mirror aims Enter at a row the reader is not on.
    const list = panel();
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] }), mark({ id: 1 })]);
    dom.root.children[1]?.dispatch("keydown", {
      key: "Enter",
      target: list.elementFor(1),
    });
    expect(picked).toEqual([1]);
  });

  it("reports a press on a row", () => {
    const list = panel();
    show(list, [mark({ id: 3 })]);
    (
      list.elementFor(3) as unknown as { dispatch: (t: string, e: object) => void }
    )?.dispatch("pointerdown", {});
    expect(picked).toEqual([3]);
  });

  it("marks one row as selected at a time", () => {
    const list = panel();
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] }), mark({ id: 1 })]);
    list.select(0);
    expect(list.selectedId).toBe(0);
    expect(list.elementFor(0)?.getAttribute("aria-selected")).toBe("true");
    list.select(1);
    expect(list.elementFor(0)?.getAttribute("aria-selected")).toBe("false");
    expect(list.elementFor(1)?.getAttribute("aria-selected")).toBe("true");
    list.select(null);
    expect(list.selectedId).toBe(-1);
    expect(list.elementFor(1)?.getAttribute("aria-selected")).toBe("false");
  });

  it("drops the selection when the mark it was on goes", () => {
    // An undo, or the reader taking the mark off from the box. Ids are handed
    // out by the model and a repainted list that kept this would mark whichever
    // row inherited one.
    const list = panel();
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] }), mark({ id: 1 })]);
    list.select(1);
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] })]);
    expect(list.selectedId).toBe(-1);
    // The control: a repaint that still holds the mark keeps it selected, so
    // the clause above is not simply clearing it on every push.
    list.select(0);
    show(list, [mark({ id: 0, quads: [10, 10, 20, 20] })]);
    expect(list.selectedId).toBe(0);
    expect(list.elementFor(0)?.getAttribute("aria-selected")).toBe("true");
  });

  it("says when a mark is on no page, and says nothing when they all are", () => {
    const list = panel();
    show(list, [mark({ id: 0 })]);
    expect(list.status).toBe("");
    show(list, [mark({ id: 0 }), mark({ id: 1, quads: [] })]);
    expect(list.status).toBe("1 mark is not on any page.");
  });

  it("refuses to navigate to a mark that is on no page, by either route", () => {
    // Two listeners, so two refusals --- and the keyboard one is the easy half
    // to forget: the row is drawn, so it can be arrowed to and pressed. A row
    // that answered would scroll the reader nowhere and look broken.
    const list = panel();
    show(list, [mark({ id: 0, quads: [] })]);
    expect(list.elementFor(0)?.getAttribute("aria-disabled")).toBe("true");
    (
      list.elementFor(0) as unknown as { dispatch: (t: string, e: object) => void }
    )?.dispatch("pointerdown", {});
    dom.root.children[1]?.dispatch("keydown", {
      key: "Enter",
      target: list.elementFor(0),
    });
    expect(picked).toEqual([]);
  });
});
