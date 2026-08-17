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
 * {@link changedSlots} is the exception and gets the most attention, because it
 * is real logic: it decides which pages the viewer redraws, and a defect there
 * is a page left painted the way it used to be.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { changedSlots, Edits, NOTHING_OPEN, type EditState } from "./edits";

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

describe("changedSlots", () => {
  it("reports nothing when the two states agree", () => {
    expect(changedSlots(state(4), state(4))).toEqual([]);
  });

  it("reports the slot whose turn moved, and only that one", () => {
    expect(changedSlots(state(4), state(4, { 2: 1 }))).toEqual([2]);
  });

  it("reports every slot that moved", () => {
    expect(changedSlots(state(4), state(4, { 0: 3, 3: 2 }))).toEqual([0, 3]);
  });

  it("reports a turn that went back to upright", () => {
    // The direction a comparison against zero would miss: undoing a rotation
    // leaves `turns` at 0, which is what an unedited page reports, so a diff
    // that looked for "is it turned" rather than "did it change" would leave the
    // page painted sideways.
    expect(changedSlots(state(4, { 1: 1 }), state(4))).toEqual([1]);
  });

  it("reports a slot whose page changed identity even at the same turn", () => {
    const before = state(3);
    const after = state(3);
    const page = after.pages[1];
    if (!page) throw new Error("fixture");
    page.id = 99;
    expect(changedSlots(before, after)).toEqual([1]);
  });

  it("reports a slot whose source changed", () => {
    const before = state(3);
    const after = state(3);
    const page = after.pages[2];
    if (!page) throw new Error("fixture");
    page.source = 0;
    expect(changedSlots(before, after)).toEqual([2]);
  });

  it("reports every slot in the longer state when the page count moved", () => {
    // Not a shortcut. When a page appears or disappears, every slot from the
    // change onwards holds a different page, and comparing turns slot by slot
    // would report "nothing moved" for a document whose pages had all shifted.
    expect(changedSlots(state(3), state(4))).toEqual([0, 1, 2, 3]);
    expect(changedSlots(state(4), state(3))).toEqual([0, 1, 2, 3]);
  });

  it("reports nothing for two empty states", () => {
    expect(changedSlots(NOTHING_OPEN, NOTHING_OPEN)).toEqual([]);
  });
});

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
