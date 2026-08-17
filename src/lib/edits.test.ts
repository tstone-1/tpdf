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

import { Edits, NOTHING_OPEN, type EditState } from "./edits";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

/** A state of `count` pages, ids from 1, with `turns` applied where given. */
function state(count: number, turns: Record<number, number> = {}): EditState {
  return {
    pages: Array.from({ length: count }, (_, at) => ({
      id: at + 1,
      source: at,
      turns: turns[at] ?? 0,
    })),
    can_undo: Object.keys(turns).length > 0,
    can_redo: false,
    dirty: Object.keys(turns).length > 0,
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
});
