/**
 * What can be copied, how a copied mark is sent back, and what the clipboard holds.
 */

import { describe, expect, it } from "vitest";

import { describe as describeMark, MarkClipboard, notCopied, shapeOf, withMarkDraft } from "./markclipboard";
import { pageId, type MarkKind, type MarkView } from "./pages";

const frame = { turns: 0, width_pt: 600, height_pt: 800, crop: { width_pt: 600, height_pt: 800, left: 0, top: 0 } };

function mark(kind: MarkKind, id = 1): MarkView {
  return {
    id,
    kind,
    stamp: kind === "stamp" ? "draft" : null,
    page: pageId(3),
    quads: [10, 20, 110, 70],
    strokes: kind === "line" ? [[12, 22, 108, 68]] : [],
    color: [0.85, 0.15, 0.15],
    width: 2.5,
    note: "a note",
    lines: [],
  };
}

describe("notCopied", () => {
  it("copies the marks a reader places, and not those made of words or fields", () => {
    for (const kind of ["note", "square", "ellipse", "textbox", "ink", "line", "arrow", "polygon", "polyline", "stamp",
      "link", "signature", "picture"] as const) {
      expect(notCopied(kind)).toBeNull();
    }
    for (const kind of ["highlight", "underline", "strikeout", "squiggly", "field"] as const) {
      expect(notCopied(kind)).toEqual(expect.any(String));
    }
  });
});

describe("a copied mark", () => {
  it("refuses a cut containing a threaded reply and retains the previous clipboard", () => {
    const clipboard = new MarkClipboard();
    const source = { frame, doc: 1, unit: 1, page: pageId(3) };
    clipboard.copy([mark("stamp")], source);
    const result = clipboard.copy([mark("square"), { ...mark("note", 2), is_reply: true }], source, new Map(), true);
    expect(result.copied).toBe(0);
    expect(result.refused[0]).toContain("reply");
    expect(clipboard.marks.map(m => m.kind)).toEqual(["stamp"]);
    expect(clipboard.copy([mark("note")], source).copied).toBe(1);
  });

  it("copies the open editor's text and formatting together", () => {
    const clipboard = new MarkClipboard();
    const original = mark("square");
    const source = { frame, doc: 1, unit: 1, page: original.page };
    for (const [note, start] of [["a note", 2], ["a longer note", 9]] as const) {
      const draft = { id: original.id, note, styles: [[start, start + 4, 1] as [number, number, number]] };
      clipboard.copy([withMarkDraft(original, draft)], source);
      expect(clipboard.marks[0]).toMatchObject({ note, styles: draft.styles });
    }
    expect(withMarkDraft(original, { id: 99, note: "other mark" })).toBe(original);
  });

  it("carries the kind, the place, the note, the colour, the width and the stamp", () => {
    expect(describeMark(mark("stamp"))).toEqual({
      kind: "stamp", quads: [10, 20, 110, 70], strokes: [], note: "a note",
      color: [0.85, 0.15, 0.15], width: 2.5, stamp: "draft",
    });
  });

  it("is its own copy, so a later change to the mark does not reach it", () => {
    const original = mark("line");
    const copied = describeMark(original);
    original.quads[0] = 999;
    original.strokes[0]![0] = 999;
    expect(typeof copied !== "string" && copied.quads[0]).toBe(10);
    expect(typeof copied !== "string" && copied.strokes[0]![0]).toBe(12);
  });

  it("goes back as strokes for a kind drawn from strokes, and as its rectangle otherwise", () => {
    const line = describeMark(mark("line"));
    const box = describeMark(mark("square"));
    if (typeof line === "string" || typeof box === "string") throw new Error("not copied");
    expect(shapeOf(line)).toEqual({ quads: [], strokes: [[12, 22, 108, 68]] });
    expect(shapeOf(box)).toEqual({ quads: [10, 20, 110, 70], strokes: [] });
  });
});

describe("a copied picture", () => {
  it("carries its pixels, and is not copied without them", () => {
    const pixels = { width: 1, height: 1, rgba: new Uint8Array([1, 2, 3, 255]) };
    const picture = { ...mark("picture"), picture: { width: 1, height: 1 } };
    expect(describeMark(picture, pixels)).toMatchObject({ kind: "picture", picture: pixels });
    expect(typeof describeMark(picture)).toBe("string");
    const clipboard = new MarkClipboard();
    expect(clipboard.copy([picture], { frame, doc: 1, unit: 1, page: pageId(3) }).copied).toBe(0);
    expect(clipboard.copy([picture], { frame, doc: 1, unit: 1, page: pageId(3) }, new Map([[1, pixels]])).copied).toBe(1);
    expect(clipboard.marks[0]?.picture).toBe(pixels);
  });

  it("is a signature's with the pixels its view carries", () => {
    const image = { width: 1, height: 1, rgba: [1, 2, 3, 255] };
    expect(describeMark({ ...mark("signature"), image })).toMatchObject({ kind: "signature", image });
  });
});

describe("MarkClipboard", () => {
  it("holds what was copied and says why the rest was not", () => {
    const clipboard = new MarkClipboard();
    const result = clipboard.copy([mark("square", 1), mark("highlight", 2)], { frame, doc: 1, unit: 1, page: pageId(3) });
    expect(result.copied).toBe(1);
    expect(result.refused).toHaveLength(1);
    expect(clipboard.marks.map((one) => one.kind)).toEqual(["square"]);
  });

  it("cancels a pending cut after reselection and preserves the clipboard on an unreadable picture", () => {
    const clipboard = new MarkClipboard();
    expect(clipboard.sameSelection([7], [8])).toBe(false);
    expect(clipboard.sameSelection([7], [])).toBe(false);
    expect(clipboard.sameSelection([7, 8], [8, 7])).toBe(true);
    const from = { frame, doc: 1, unit: 1, page: pageId(2) };
    clipboard.copy([mark("ellipse")], from);
    const result = clipboard.copy([mark("square"), mark("picture")], from, new Map(), true);
    expect(result.copied).toBe(0);
    expect(clipboard.marks.map((item) => item.kind)).toEqual(["ellipse"]);
  });

  it("keeps what it held when nothing new can be copied", () => {
    const clipboard = new MarkClipboard();
    clipboard.copy([mark("square")], { frame, doc: 1, unit: 1, page: pageId(3) });
    clipboard.copy([mark("highlight")], { frame, doc: 2, unit: 1, page: pageId(1) });
    expect(clipboard.marks.map((one) => one.kind)).toEqual(["square"]);
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(true);
  });

  it("knows the page the marks came from, in that document only", () => {
    const clipboard = new MarkClipboard();
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(false);
    clipboard.copy([mark("ellipse")], { frame, doc: 1, unit: 1, page: pageId(3) });
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(true);
    expect(clipboard.onItsOwnPage(1, pageId(4))).toBe(false);
    expect(clipboard.onItsOwnPage(2, pageId(3))).toBe(false);
  });
});

it("preserves physical rectangles, stroke points and widths across page units", () => {
  const clipboard = new MarkClipboard();
  for (const [from, to] of [[1, 2], [2, 1]] as const) {
    clipboard.copy([mark("square"), mark("line", 2)], { frame, doc: 1, unit: from, page: pageId(3) });
    const [box, line] = clipboard.forPaste(to, 0);
    expect(box!.quads.map(n => n * to)).toEqual(mark("square").quads.map(n => n * from));
    expect(line!.strokes[0]!.map(n => n * to)).toEqual(mark("line").strokes[0]!.map(n => n * from));
    expect(line!.width).toBe(mark("line").width * from);
    expect(clipboard.forPaste(from, 0)[1]!.width).toBe(mark("line").width * from);
    expect(clipboard.forPaste(1000, 0)[1]!.width).toBe(mark("line").width * from);
  }
});

it("pastes the copy in progress and ignores an older copy that finishes later", async () => {
  const clipboard = new MarkClipboard();
  const source = { frame, doc: 1, unit: 2, page: pageId(3) };
  let finishFirst!: () => void, finishSecond!: () => void;
  const first = new Promise<void>(resolve => { finishFirst = resolve; });
  const second = new Promise<void>(resolve => { finishSecond = resolve; });
  const copy = (kind: MarkKind, ready: Promise<void>) => clipboard.prepare(async current => {
    await ready;
    if (current()) clipboard.copy([mark(kind)], source);
  });
  const older = copy("square", first);
  expect(clipboard.canPaste).toBe(true);
  const olderPaste = clipboard.readyMarks();
  const newer = copy("line", second);
  const newerPaste = clipboard.readyMarks();
  finishSecond();
  expect((await newerPaste).map(mark => mark.kind)).toEqual(["line"]);
  finishFirst();
  await Promise.all([older, newer]);
  expect(await olderPaste).toEqual([]);
  expect(clipboard.marks.map(mark => mark.kind)).toEqual(["line"]);
});

it("pastes a turned page's picture and stroke in their displayed orientation", () => {
  const clipboard = new MarkClipboard();
  const stored = { width: 1, height: 2, rgba: new Uint8Array([10, 0, 0, 255, 20, 0, 0, 255]) };
  clipboard.copy([mark("picture"), mark("line", 2)], { doc: 1, unit: 1, page: pageId(3), frame: { ...frame, turns: 1 } }, new Map([[1, stored]]));
  const [picture, line] = clipboard.forPaste(1, 0);
  expect(picture!.quads).toEqual([730, 10, 780, 110]);
  expect(picture!.picture?.width).toBe(2);
  expect([...(picture!.picture?.rgba ?? [])]).toEqual([20, 0, 0, 255, 10, 0, 0, 255]);
  expect(line!.strokes).toEqual([[778, 12, 732, 108]]);
  expect(clipboard.forPaste(1, 1)[0]!.picture).toBe(stored);
});
