/**
 * What can be copied, how a copied mark is sent back, and what the clipboard holds.
 */

import { describe, expect, it } from "vitest";

import { describe as describeMark, MarkClipboard, notCopied, shapeOf } from "./markclipboard";
import { pageId, type MarkKind, type MarkView } from "./pages";

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
  it("copies the marks a reader places, and not those made of words, fields or signatures", () => {
    for (const kind of ["note", "square", "ellipse", "textbox", "ink", "line", "arrow", "polygon", "polyline", "stamp"] as const) {
      expect(notCopied(kind)).toBeNull();
    }
    for (const kind of ["highlight", "underline", "strikeout", "squiggly", "field", "signature"] as const) {
      expect(notCopied(kind)).toEqual(expect.any(String));
    }
  });
});

describe("a copied mark", () => {
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

describe("MarkClipboard", () => {
  it("holds what was copied and says why the rest was not", () => {
    const clipboard = new MarkClipboard();
    const result = clipboard.copy([mark("square", 1), mark("highlight", 2)], { doc: 1, page: pageId(3) });
    expect(result.copied).toBe(1);
    expect(result.refused).toHaveLength(1);
    expect(clipboard.marks.map((one) => one.kind)).toEqual(["square"]);
  });

  it("keeps what it held when nothing new can be copied", () => {
    const clipboard = new MarkClipboard();
    clipboard.copy([mark("square")], { doc: 1, page: pageId(3) });
    clipboard.copy([mark("highlight")], { doc: 2, page: pageId(1) });
    expect(clipboard.marks.map((one) => one.kind)).toEqual(["square"]);
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(true);
  });

  it("knows the page the marks came from, in that document only", () => {
    const clipboard = new MarkClipboard();
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(false);
    clipboard.copy([mark("ellipse")], { doc: 1, page: pageId(3) });
    expect(clipboard.onItsOwnPage(1, pageId(3))).toBe(true);
    expect(clipboard.onItsOwnPage(1, pageId(4))).toBe(false);
    expect(clipboard.onItsOwnPage(2, pageId(3))).toBe(false);
  });
});
