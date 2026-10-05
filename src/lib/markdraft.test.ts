/**
 * The drafts on their own: which text and which lines the page is told to draw.
 *
 * `viewerdraft.test.ts` holds the join, from a keystroke in the box to what the
 * overlay paints. Here every decision is asked directly, with the wrap as a
 * list of questions the test answers in whatever order it likes.
 */

import { beforeEach, describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import { drafted, MarkDrafts } from "./markdraft";
import { INK_WIDTH } from "./markband";
import { pageId, type MarkKind, type MarkView } from "./pages";
import { settle } from "./testdom";

function mark(id: number, kind: MarkKind, over: Partial<MarkView> = {}): MarkView {
  return {
    id, kind, stamp: null, page: pageId(1), quads: [100, 100, 300, 160], strokes: [],
    color: [0.85, 0.15, 0.15], width: INK_WIDTH, note: "", lines: [], ...over,
  };
}

/** One question the drafts asked, and the two ways to answer it. */
interface Question {
  note: string;
  left: number;
  right: number;
  answer: (lines: string[]) => void;
  refuse: () => void;
}

let asked: Question[];
let changes: number;
let drafts: MarkDrafts;

beforeEach(() => {
  asked = [];
  changes = 0;
  drafts = new MarkDrafts({
    wrap: (note, left, right) =>
      new Promise<string[]>((answer, refuse) => {
        asked.push({ note, left, right, answer, refuse: () => refuse(new Error("no")) });
      }),
    changed: () => {
      changes += 1;
    },
  });
});

const BOX = mark(1, "textbox", { note: "old", lines: ["old"] });
const FIELD = mark(2, "field", { note: "Name" });

describe("which kinds have a draft", () => {
  it("is the two whose note the page draws, and each in its own way", () => {
    const every: MarkKind[] = [
      "highlight", "underline", "strikeout", "squiggly", "note", "square",
      "ellipse", "textbox", "field", "stamp", "signature", "ink",
    ];
    expect(every.filter((kind) => drafted(kind) !== null)).toEqual(["textbox", "field"]);
    expect(drafted("textbox")).toBe("lines");
    expect(drafted("field")).toBe("name");
  });

  it("keeps nothing and asks nothing for a kind whose note is not on the page", () => {
    const highlight = mark(3, "highlight", { note: "was", lines: [] });
    drafts.opened(3);
    drafts.typed(highlight, "a remark");
    expect(asked).toEqual([]);
    expect(changes).toBe(0);
    expect(drafts.noteOf(highlight)).toBe("was");
    expect(drafts.linesOf(highlight)).toEqual([]);
  });
});

describe("a text box's draft", () => {
  it("asks for the lines of what was typed, for the box's own edges", () => {
    drafts.opened(1);
    drafts.typed(BOX, "hello there");
    expect(asked.map(({ note, left, right }) => ({ note, left, right }))).toEqual([
      { note: "hello there", left: 100, right: 300 },
    ]);
  });

  it("draws the model's lines until an answer arrives, and the answer after", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "hello there");
    expect(drafts.linesOf(BOX)).toEqual(["old"]);
    const before = changes;
    asked[0]!.answer(["hello", "there"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hello", "there"]);
    expect(changes).toBe(before + 1);
  });

  it("takes only the answer to the newest question, whatever order they arrive in", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "h");
    drafts.typed(BOX, "he");
    drafts.typed(BOX, "hel");
    // The newest first, then the two before it, late.
    asked[2]!.answer(["hel"]);
    await settle();
    asked[0]!.answer(["h"]);
    asked[1]!.answer(["he"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hel"]);
  });

  it("does not draw an older answer while the newest is still out", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "h");
    drafts.typed(BOX, "he");
    asked[0]!.answer(["h"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["old"]);
  });

  it("keeps the last lines it drew while the next are asked for", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "h");
    asked[0]!.answer(["h"]);
    await settle();
    drafts.typed(BOX, "he");
    expect(drafts.linesOf(BOX)).toEqual(["h"]);
  });

  it("keeps what it showed when the wrap fails, and goes on with the next", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "h");
    asked[0]!.answer(["h"]);
    await settle();
    drafts.typed(BOX, "he");
    asked[1]!.refuse();
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["h"]);
    drafts.typed(BOX, "hel");
    asked[2]!.answer(["hel"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hel"]);
  });

  it("draws the model's lines when there is nobody to ask", () => {
    const alone = new MarkDrafts({ wrap: () => undefined, changed: () => {} });
    alone.opened(1);
    alone.typed(BOX, "hello");
    expect(alone.linesOf(BOX)).toEqual(["old"]);
  });

  it("drops an answer that arrives after the box closed", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "hello");
    drafts.opened(null);
    const before = changes;
    asked[0]!.answer(["hello"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["old"]);
    // And nothing is redrawn for it: there is nothing it could have changed.
    expect(changes).toBe(before);
  });

  it("drops an answer for a mark the box has left", async () => {
    const other = mark(4, "textbox", { note: "", lines: [] });
    drafts.opened(1);
    drafts.typed(BOX, "hello");
    drafts.opened(4);
    drafts.typed(other, "x");
    asked[0]!.answer(["hello"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["old"]);
    expect(drafts.linesOf(other)).toEqual([]);
    asked[1]!.answer(["x"]);
    await settle();
    expect(drafts.linesOf(other)).toEqual(["x"]);
  });

  it("asks again when the box is resized under it, and not when it is only sent again", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "hello there");
    asked[0]!.answer(["hello there"]);
    await settle();
    drafts.refit(BOX);
    expect(asked).toHaveLength(1);
    drafts.refit({ ...BOX, quads: [100, 100, 140, 160] });
    expect(asked).toHaveLength(2);
    expect(asked[1]).toMatchObject({ note: "hello there", left: 100, right: 140 });
    asked[1]!.answer(["hello", "there"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hello", "there"]);
    // Another mark's news is not this draft's.
    drafts.refit(mark(9, "textbox", { quads: [0, 0, 10, 10] }));
    expect(asked).toHaveLength(2);
  });
});

describe("a field's draft", () => {
  it("is the name as typed, at once and with nothing asked", () => {
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    expect(drafts.noteOf(FIELD)).toBe("Surname");
    expect(asked).toEqual([]);
    expect(changes).toBe(1);
    // An emptied box is an empty name, not the model's.
    drafts.typed(FIELD, "");
    expect(drafts.noteOf(FIELD)).toBe("");
    // Nor when the field is resized under its box: a name is not wrapped.
    drafts.refit({ ...FIELD, quads: [100, 100, 140, 160] });
    expect(asked).toEqual([]);
  });

  it("is dropped when the box closes, and when another mark takes it", () => {
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    const before = changes;
    drafts.opened(null);
    expect(drafts.noteOf(FIELD)).toBe("Name");
    expect(changes).toBe(before + 1);

    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    drafts.opened(1);
    expect(drafts.noteOf(FIELD)).toBe("Name");
  });

  it("is kept when the box reports the mark it is already on", () => {
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    drafts.opened(2);
    expect(drafts.noteOf(FIELD)).toBe("Surname");
  });
});

describe("a commit", () => {
  it("keeps the draft drawn until the edit settles, so the old words are never drawn between", async () => {
    let settleEdit: () => void = () => {};
    const sent = new Promise<void>((done) => {
      settleEdit = done;
    });
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    drafts.committed(2, "Surname", sent);
    drafts.opened(null);
    await settle();
    expect(drafts.noteOf(FIELD)).toBe("Surname");
    const before = changes;
    settleEdit();
    await settle();
    // The model's own, whatever it now is.
    expect(drafts.noteOf({ ...FIELD, note: "Surname!" })).toBe("Surname!");
    expect(changes).toBe(before + 1);
  });

  it("lets go when the edit is refused, and the page shows what the model has", async () => {
    drafts.opened(2);
    drafts.typed(FIELD, "a.b");
    drafts.committed(2, "a.b", Promise.reject(new Error("refused")));
    drafts.opened(null);
    await settle();
    expect(drafts.noteOf(FIELD)).toBe("Name");
  });

  it("lets go at once when nothing says when the model has answered", async () => {
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    drafts.committed(2, "Surname", undefined);
    await settle();
    expect(drafts.noteOf(FIELD)).toBe("Name");
  });

  it("takes the newest answer for a text box that closed before it arrived", async () => {
    let settleEdit: () => void = () => {};
    drafts.opened(1);
    drafts.typed(BOX, "hello");
    drafts.committed(1, "hello", new Promise<void>((done) => {
      settleEdit = done;
    }));
    drafts.opened(null);
    asked[0]!.answer(["hello"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hello"]);
    settleEdit();
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["old"]);
  });

  it("does not hold a draft whose text is not what was sent", async () => {
    drafts.opened(2);
    drafts.typed(FIELD, "Surname");
    drafts.committed(2, "something else", new Promise<void>(() => {}));
    await settle();
    expect(drafts.noteOf(FIELD)).toBe("Name");
  });

  it("holds nothing for a mark that had no draft", async () => {
    drafts.committed(2, "Surname", new Promise<void>(() => {}));
    await settle();
    expect(drafts.noteOf(FIELD)).toBe("Name");
    expect(changes).toBe(0);
  });

  it("holds nothing for a mark other than the one being typed for", async () => {
    drafts.opened(1);
    drafts.typed(BOX, "hello");
    drafts.committed(2, "hello", new Promise<void>(() => {}));
    expect(drafts.noteOf(FIELD)).toBe("Name");
    // And the box's own draft is still the one being typed.
    asked[0]!.answer(["hello"]);
    await settle();
    expect(drafts.linesOf(BOX)).toEqual(["hello"]);
  });

  it("draws what is typed next over what is held, and a first edit settling does not drop a second", async () => {
    const settles: (() => void)[] = [];
    const edit = (): Promise<void> => new Promise<void>((done) => settles.push(done));
    drafts.opened(2);
    drafts.typed(FIELD, "One");
    drafts.committed(2, "One", edit());
    drafts.opened(null);
    drafts.opened(2);
    drafts.typed(FIELD, "Two");
    expect(drafts.noteOf(FIELD)).toBe("Two");
    drafts.committed(2, "Two", edit());
    drafts.opened(null);
    settles[0]!();
    await settle();
    expect(drafts.noteOf({ ...FIELD, note: "One" })).toBe("Two");
    settles[1]!();
    await settle();
    expect(drafts.noteOf({ ...FIELD, note: "Two!" })).toBe("Two!");
  });
});

describe("the drafts' wiring in App.svelte", () => {
  // Source-level, because `App.svelte` is the join and nothing imports it.
  it("asks the command that wraps as the model does, with the box's edges", () => {
    expect(app).toContain(
      'onMarkDraft: (note, left, right) => call("annot_draft_lines", { note, left, right }),',
    );
  });

  it("hands the edit's promise back for a note, so a draft is held until the model answers", () => {
    expect(app).toContain("if (!isSaved(mark)) return applyEdit((e) => e.renote(mark, note));");
    expect(app).toContain("return to ? applyEdit((e) => e.refield([to])) : undefined;");
  });
});
