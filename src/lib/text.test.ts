/**
 * Tests for splitting a page's characters into lines.
 *
 * This is the geometry the screen-reader text depends on (`a11y.ts`): a page
 * delivered as one 2,700-character blob is present and unusable, because moving
 * by line is most of how a document is read. It is also the one piece of that
 * layer with an answer that can be *wrong* --- whether VoiceOver announces the
 * result well is not something a unit test can say, and no claim is made about
 * it here.
 *
 * Every test below was checked by mutating `linesOf` and confirming it went red.
 */

import { describe, expect, it } from "vitest";
import { linesOf, textOf, type PageText } from "./text";

/** Builds a page from `(character, [left, top, right, bottom])` pairs. */
function page(chars: [string, [number, number, number, number] | null][]): PageText {
  const codes: number[] = [];
  const boxes: number[] = [];
  for (const [char, box] of chars) {
    codes.push(char.codePointAt(0) ?? 0);
    boxes.push(...(box ?? [0, 0, 0, 0]));
  }
  return { codes, boxes, width_pt: 100, height_pt: 100, extract_ms: 0 };
}

/** Two words on one line, then two on the line below it. */
function twoLines(): PageText {
  return page([
    ["a", [10, 10, 20, 22]],
    ["b", [20, 10, 30, 22]],
    ["c", [30, 10, 40, 22]],
    ["d", [10, 30, 20, 42]],
    ["e", [20, 30, 30, 42]],
  ]);
}

describe("linesOf", () => {
  it("splits where the text moves to the next line", () => {
    expect(linesOf(twoLines())).toEqual([
      { from: 0, to: 3 },
      { from: 3, to: 5 },
    ]);
  });

  it("keeps a character on its line when the boxes overlap vertically", () => {
    // A superscript: higher and smaller, but sharing most of its height with
    // the line it belongs to.
    const superscripted = page([
      ["a", [10, 10, 20, 22]],
      ["2", [20, 8, 26, 16]],
      ["b", [26, 10, 36, 22]],
    ]);
    expect(linesOf(superscripted)).toEqual([{ from: 0, to: 3 }]);
  });

  it("grows a line to fit its tallest character", () => {
    // The line starts on a short letter, then an ascender makes it taller, and
    // a superscript overlaps the ascender but not the short letter. Comparing
    // against the line's accumulated extent keeps all three together; comparing
    // against the character that started it splits the last one off.
    const ascender = page([
      ["o", [10, 12, 18, 22]],
      ["l", [18, 6, 24, 22]],
      ["2", [24, 4, 30, 12]],
    ]);
    expect(linesOf(ascender)).toEqual([{ from: 0, to: 3 }]);
  });

  it("attaches a character with no box to the line before it", () => {
    // The trailing space of a line: PDFium gives it no box, and starting a new
    // line on it would put a stray blank between every pair of lines.
    const trailing = page([
      ["a", [10, 10, 20, 22]],
      [" ", null],
      ["b", [10, 30, 20, 42]],
    ]);
    expect(linesOf(trailing)).toEqual([
      { from: 0, to: 2 },
      { from: 2, to: 3 },
    ]);
  });

  it("does not drop a character with no box at the very start", () => {
    const leading = page([
      [" ", null],
      ["a", [10, 10, 20, 22]],
    ]);
    expect(linesOf(leading)).toEqual([
      { from: 0, to: 1 },
      { from: 1, to: 2 },
    ]);
  });

  it("covers the page exactly, so the lines rebuild its text", () => {
    // The property the whole split rests on: contiguous, in order, nothing
    // dropped and nothing repeated. A test on any single line would not see a
    // gap; this does.
    //
    // The fixture has a one-character line and a character with no box on
    // purpose. Run against `twoLines()` it missed a mutation that closed each
    // line one character short, because with two or more characters per line
    // the *next* character reopens the range and hides it.
    const source = page([
      ["a", [10, 10, 20, 22]],
      ["b", [20, 10, 30, 22]],
      [" ", null],
      ["c", [10, 30, 20, 42]],
      ["d", [10, 50, 20, 62]],
      ["e", [20, 50, 30, 62]],
    ]);
    const rebuilt = linesOf(source)
      .map((line) => textOf(source, line.from, line.to))
      .join("");
    expect(rebuilt).toBe(textOf(source, 0, source.codes.length));
  });

  it("has no lines on a page with no characters", () => {
    expect(linesOf(page([]))).toEqual([]);
  });
});
