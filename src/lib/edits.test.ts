/**
 * The frontend's half of the edit model: what it caches, and what it asks for.
 *
 * There is deliberately very little logic here to test, which is the design ---
 * the journal, the undo replay and the arithmetic of composing a turn all live in
 * Rust, and a second implementation of any of them on this side is the thing
 * `edits.ts` exists to avoid. So what these assert is the two things this side
 * genuinely owns: that a command names a page by the *identity* the model gave
 * it, and that a reply is adopted whole rather than merged.
 *
 * The third is {@link Edits.map}, which is the translation the viewer lays the
 * document out through --- `pages.ts` has its own tests, and what is asserted
 * here is that the state a reply carried is what the map is built from.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  Edits,
  NOTHING_OPEN,
  type EditState,
  type MarkView,
} from "./edits";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

/** A state of `count` pages, ids from 1, with `turns` applied where given. */
function state(
  count: number,
  turns: Record<number, number> = {},
  marks: MarkView[] = [],
): EditState {
  return {
    pages: Array.from({ length: count }, (_, at) => ({
      id: at + 1,
      source: at,
      turns: turns[at] ?? 0,
    })),
    marks,
    can_undo: Object.keys(turns).length > 0,
    can_redo: false,
    dirty: Object.keys(turns).length > 0,
  };
}

/** One highlight on the page with id `page`. */
function mark(id: number, page: number): MarkView {
  return {
    id,
    kind: "highlight",
    page,
    quads: [72, 100, 300, 118],
    strokes: [],
    color: [1, 0.9, 0.2],
    note: "",
    lines: [],
  };
}

describe("Edits", () => {
  beforeEach(() => {
    core.invoke.mockReset();
  });

  it("starts with nothing, so a command before the first reply is refused here", async () => {
    const edits = new Edits(3);
    expect(edits.state).toEqual(NOTHING_OPEN);
    expect(edits.dirty).toBe(false);

    // No page 0 in the cache, so no id to send. The model would refuse it too;
    // refusing here says the same thing without a round trip, and the assertion
    // is that nothing was sent at all.
    await edits.rotate(0, 1);
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("sends the page's identity, not its position", async () => {
    const opened = state(3);
    const page = opened.pages[2];
    if (!page) throw new Error("fixture");
    page.id = 4242;
    core.invoke.mockResolvedValueOnce(opened);
    const edits = new Edits(7);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(3, { 2: 1 }));
    await edits.rotate(2, 1);

    expect(core.invoke).toHaveBeenLastCalledWith("page_rotate", {
      doc: 7,
      page: 4242,
      turns: 1,
    });
  });

  it("adopts a reply whole", async () => {
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(1);
    await edits.refresh();
    expect(edits.turnsOf(1)).toBe(0);

    core.invoke.mockResolvedValueOnce(state(3, { 1: 2 }));
    await edits.rotate(1, 2);
    expect(edits.turnsOf(1)).toBe(2);
    expect(edits.dirty).toBe(true);
  });

  it("deletes by identity, not by position", async () => {
    const opened = state(3);
    const page = opened.pages[1];
    if (!page) throw new Error("fixture");
    page.id = 77;
    core.invoke.mockResolvedValueOnce(opened);
    const edits = new Edits(2);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce({
      ...state(2),
      pages: [opened.pages[0], opened.pages[2]],
    });
    await edits.delete(1);

    expect(core.invoke).toHaveBeenLastCalledWith("page_delete", {
      doc: 2,
      page: 77,
    });
  });

  it("does not send a delete for a slot the model has never mentioned", async () => {
    const edits = new Edits(3);
    await edits.delete(0);
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("does not decide for itself whether the last page may go", async () => {
    // The rule is the model's. A copy of it here would be a second rule, able to
    // disagree with the first about a document a command in flight has already
    // changed --- so the command goes, and the refusal comes back.
    core.invoke.mockResolvedValueOnce(state(1));
    const edits = new Edits(4);
    await edits.refresh();

    core.invoke.mockRejectedValueOnce("a document must keep at least one page");
    await expect(edits.delete(0)).rejects.toBe(
      "a document must keep at least one page",
    );
    expect(core.invoke).toHaveBeenLastCalledWith("page_delete", {
      doc: 4,
      page: 1,
    });
  });

  it("turns a destination slot into the neighbour the model accepts", async () => {
    const opened = state(4);
    core.invoke.mockResolvedValueOnce(opened);
    const edits = new Edits(9);
    await edits.refresh();

    // The first page to the last slot. The anchor is read out of the order
    // *without* it: the page that ends up in front of it is the old page 4, id
    // 4 --- reading the order that still holds it would name id 3 and land the
    // page one slot short of where the reader put it.
    core.invoke.mockResolvedValueOnce(opened);
    await edits.move(0, 3);
    expect(core.invoke).toHaveBeenLastCalledWith("page_move", {
      doc: 9,
      page: 1,
      after: 4,
    });
  });

  it("sends no anchor for a move to the front", async () => {
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(3));
    await edits.move(2, 0);
    expect(core.invoke).toHaveBeenLastCalledWith("page_move", {
      doc: 9,
      page: 3,
      after: null,
    });
  });

  it("names the page after the one being moved for a single step back", async () => {
    // The case the naive arithmetic breaks loudly on rather than quietly: it
    // would name the moved page as its own anchor, which the model refuses.
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(3));
    await edits.move(0, 1);
    expect(core.invoke).toHaveBeenLastCalledWith("page_move", {
      doc: 9,
      page: 1,
      after: 2,
    });
  });

  it("names the page before the destination for a step towards the front", async () => {
    core.invoke.mockResolvedValueOnce(state(4));
    const edits = new Edits(9);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(4));
    await edits.move(3, 2);
    expect(core.invoke).toHaveBeenLastCalledWith("page_move", {
      doc: 9,
      page: 4,
      after: 2,
    });
  });

  it("sends nothing for a move that changes no order", async () => {
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();
    core.invoke.mockReset();

    // Onto its own slot, and — after the clamp — onto the end it is already at.
    // A journal entry for a move that moves nothing costs the reader an undo
    // that does nothing visible.
    await edits.move(1, 1);
    await edits.move(0, -4);
    await edits.move(2, 9);
    expect(core.invoke).not.toHaveBeenCalled();

    // The control: the same clamp on a page that is *not* already at that end
    // is a real move, so the three above are silent because they change nothing
    // rather than because a destination past the end is dropped.
    core.invoke.mockResolvedValueOnce(state(3));
    await edits.move(0, 9);
    expect(core.invoke).toHaveBeenLastCalledWith("page_move", {
      doc: 9,
      page: 1,
      after: 3,
    });
  });

  it("does not send a move for a slot the model has never mentioned", async () => {
    const edits = new Edits(9);
    await edits.move(0, 1);
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("builds the map from the pages the last reply carried", async () => {
    const opened = state(3);
    core.invoke.mockResolvedValueOnce(opened);
    const edits = new Edits(6);
    await edits.refresh();
    expect(edits.map.sources()).toEqual([0, 1, 2]);

    // Page 2 deleted: the reply is the whole order, and the map is a reading of
    // it rather than an edit applied to the previous one.
    core.invoke.mockResolvedValueOnce({
      ...opened,
      pages: [opened.pages[0], opened.pages[2]],
    });
    await edits.delete(1);
    expect(edits.map.sources()).toEqual([0, 2]);
    expect(edits.map.slotOf(1)).toBeUndefined();
  });

  it("reports no turn for a slot the model does not have", () => {
    // Rather than `undefined` flowing into the layout arithmetic, which is the
    // failure `ipc.ts` describes: a page laid out at NaN rather than an error.
    expect(new Edits(1).turnsOf(9)).toBe(0);
  });

  it("carries the document handle so a caller cannot pass the wrong one", async () => {
    core.invoke.mockResolvedValue(state(1));
    const edits = new Edits(12);
    await edits.refresh();
    await edits.undo();
    await edits.redo();
    for (const call of core.invoke.mock.calls) {
      expect(call[1]).toMatchObject({ doc: 12 });
    }
  });

  it("names the open document and no destination when it saves in place", async () => {
    // The absence of a `path` is the whole of it. A save in place that carried
    // one would be `save_copy` under another name, and the backend would take
    // whatever it was given --- which on a mistyped call is a second file
    // written while the reader is told their document was saved.
    core.invoke.mockResolvedValueOnce(state(2, { 0: 1 }));
    const edits = new Edits(5);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(undefined);
    await edits.save("/in.pdf");
    expect(core.invoke).toHaveBeenLastCalledWith("save_document", {
      doc: 5,
      source: "/in.pdf",
    });
  });

  it("does not clear dirty when a copy is written", async () => {
    // The journal is still the journal. Reporting the document as clean after a
    // copy would claim the *open* file matches what is on disk, which it does
    // not --- a copy was written somewhere else.
    core.invoke.mockResolvedValueOnce(state(2, { 0: 1 }));
    const edits = new Edits(5);
    await edits.refresh();
    expect(edits.dirty).toBe(true);

    core.invoke.mockResolvedValueOnce(undefined);
    await edits.saveCopy("/in.pdf", "/out.pdf");
    expect(edits.dirty).toBe(true);
    expect(core.invoke).toHaveBeenLastCalledWith("save_copy", {
      doc: 5,
      source: "/in.pdf",
      path: "/out.pdf",
    });
  });

  it("sends the page's id rather than its slot when a mark is made", async () => {
    // The same reason `rotate` sends an id, one degree sharper: a mark carries
    // *coordinates*, so a stale slot would put a reader's highlight on a
    // different page at the place the words used to be.
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(3, {}, [mark(1, 3)]));
    const after = await edits.mark("highlight", 2, [10, 20, 30, 40], [], "a note");
    expect(core.invoke).toHaveBeenLastCalledWith("annot_mark", {
      doc: 9,
      mark: {
        kind: "highlight",
        page: 3,
        quads: [10, 20, 30, 40],
        strokes: [],
        color: [1, 0.9, 0.2],
        author: "",
        note: "a note",
      },
    });
    expect(after.marks).toHaveLength(1);
    expect(edits.state.marks[0]?.id).toBe(1);
  });

  it("sends each kind with its own colour", async () => {
    // The colour is the one thing this side of the boundary decides, and the
    // two lines are deliberately not the wash's yellow: a 1.3 pt yellow rule on
    // white paper is close to invisible where the same yellow over a whole line
    // of text is right. Asserted as a set of three rather than one at a time,
    // so a table that gave every kind the same colour cannot pass.
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();

    const sent: Record<string, [number, number, number]> = {};
    for (const kind of ["highlight", "underline", "strikeout"] as const) {
      core.invoke.mockResolvedValueOnce(state(3, {}, []));
      await edits.mark(kind, 2, [10, 20, 30, 40]);
      const call = core.invoke.mock.lastCall as [string, { mark: { color: [number, number, number] } }];
      sent[kind] = call[1].mark.color;
    }
    expect(sent.highlight).toEqual([1, 0.9, 0.2]);
    expect(sent.underline).not.toEqual(sent.highlight);
    expect(sent.strikeout).toEqual(sent.underline);
  });

  it("sends the reader's colour for every kind, once one is chosen", async () => {
    // The other half of the table above: a chosen colour replaces the kind's
    // own, for *all* of them. Three kinds rather than one, because a rule
    // applied to the highlight alone would leave a reader who picked green with
    // a red underline and nothing saying why.
    core.invoke.mockResolvedValueOnce(state(3));
    const edits = new Edits(9);
    await edits.refresh();

    const green: [number, number, number] = [0.35, 0.8, 0.35];
    for (const kind of ["highlight", "underline", "note"] as const) {
      core.invoke.mockResolvedValueOnce(state(3, {}, []));
      await edits.mark(kind, 2, [10, 20, 30, 40], [], "", green);
      const call = core.invoke.mock.lastCall as [
        string,
        { mark: { color: [number, number, number] } },
      ];
      expect(call[1].mark.color).toEqual(green);
    }
  });

  it("does not send a mark for a slot the model has never mentioned", async () => {
    core.invoke.mockResolvedValueOnce(state(2));
    const edits = new Edits(9);
    await edits.refresh();
    core.invoke.mockClear();

    await edits.mark("highlight", 7, [10, 20, 30, 40]);
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("sends the mark's own id when one is removed", async () => {
    // A mark is addressed by identity all the way through: there is no slot
    // that names one, and its position in `marks` moves whenever an earlier
    // mark is removed.
    core.invoke.mockResolvedValueOnce(state(2, {}, [mark(4, 1), mark(5, 2)]));
    const edits = new Edits(3);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(2, {}, [mark(5, 2)]));
    await edits.unmark(4);
    expect(core.invoke).toHaveBeenLastCalledWith("annot_remove", {
      doc: 3,
      mark: 4,
    });
    expect(edits.state.marks.map((m) => m.id)).toEqual([5]);
  });

  it("sends the mark's own id and the whole note when one is typed", async () => {
    // Addressed by identity, like the removal above -- and the *whole* note,
    // because the model takes a version rather than an edit to one. A caller
    // that sent only what changed would need the model to hold a cursor.
    core.invoke.mockResolvedValueOnce(state(2, {}, [mark(4, 1)]));
    const edits = new Edits(3);
    await edits.refresh();

    core.invoke.mockResolvedValueOnce(state(2, {}, [mark(4, 1)]));
    await edits.renote(4, "ask about this");
    expect(core.invoke).toHaveBeenLastCalledWith("annot_note", {
      doc: 3,
      mark: 4,
      note: "ask about this",
    });
  });

  it("carries the marks a reply brought, and drops the ones it did not", async () => {
    // The cache is replaced by each answer rather than merged into. A merge
    // would leave an undone mark on screen, which is the one failure undo
    // exists to prevent.
    core.invoke.mockResolvedValueOnce(state(1, {}, [mark(1, 1), mark(2, 1)]));
    const edits = new Edits(1);
    await edits.refresh();
    expect(edits.state.marks).toHaveLength(2);

    core.invoke.mockResolvedValueOnce(state(1));
    await edits.undo();
    expect(edits.state.marks).toEqual([]);
  });
});
