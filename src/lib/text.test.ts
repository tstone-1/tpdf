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
import {
  linesOf,
  linesRunSideways,
  textOf,
  turnedView,
  turnQuad,
  type PageText,
} from "./text";

/** Builds a page from `(character, [left, top, right, bottom])` pairs. */
function page(
  chars: [string, [number, number, number, number] | null][],
  quarter_turns = 0,
): PageText {
  const codes: number[] = [];
  const boxes: number[] = [];
  for (const [char, box] of chars) {
    codes.push(char.codePointAt(0) ?? 0);
    boxes.push(...(box ?? [0, 0, 0, 0]));
  }
  return { codes, boxes, width_pt: 100, height_pt: 100, quarter_turns, extract_ms: 0 };
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

describe("linesOf on a rotated page", () => {
  /**
   * The same two lines, turned a quarter clockwise.
   *
   * Text runs *down* the screen and successive lines advance sideways, which is
   * what `/Rotate 90` displays --- and what a scanner emits. Grouping by
   * vertical overlap puts each of these five characters on a line of its own,
   * so a screen reader reads the page letter by letter. It did.
   */
  function turned(): PageText {
    return page(
      [
        ["a", [10, 10, 22, 20]],
        ["b", [10, 20, 22, 30]],
        ["c", [10, 30, 22, 40]],
        ["d", [30, 10, 42, 20]],
        ["e", [30, 20, 42, 30]],
      ],
      1,
    );
  }

  it("knows which axis separates lines", () => {
    expect(linesRunSideways(turned())).toBe(true);
    expect(linesRunSideways(twoLines())).toBe(false);
  });

  it("groups characters that run down the screen into one line", () => {
    expect(linesOf(turned())).toEqual([
      { from: 0, to: 3 },
      { from: 3, to: 5 },
    ]);
  });

  it("does not group the same boxes when the page is upright", () => {
    // The control, and the reason this fixture is a quarter turn of the other
    // rather than a fresh one: the *only* difference between them is the
    // rotation, so a `linesOf` that ignored it would give both the same answer.
    // Read as an upright page these five characters are five separate lines.
    const upright = { ...turned(), quarter_turns: 0 };
    expect(linesOf(upright)).toEqual([
      { from: 0, to: 1 },
      { from: 1, to: 2 },
      { from: 2, to: 3 },
      { from: 3, to: 4 },
      { from: 4, to: 5 },
    ]);
  });

  it("covers a rotated page exactly too", () => {
    const source = turned();
    const rebuilt = linesOf(source)
      .map((line) => textOf(source, line.from, line.to))
      .join("");
    expect(rebuilt).toBe(textOf(source, 0, source.codes.length));
  });

  it("treats a half turn as upright, since lines still stack vertically", () => {
    // 180 degrees reverses the reading order but not the axis, so the vertical
    // rule is still the right one. Written as "any rotation is sideways" this
    // is what fails.
    expect(linesRunSideways({ ...twoLines(), quarter_turns: 2 })).toBe(false);
    expect(linesOf({ ...twoLines(), quarter_turns: 2 })).toEqual(linesOf(twoLines()));
  });

  it("treats a three-quarter turn as sideways", () => {
    expect(linesRunSideways({ ...turned(), quarter_turns: 3 })).toBe(true);
  });
});

describe("turnQuad", () => {
  // A 100x200 page, and a box near its top-left: 10..30 across, 20..60 down.
  // Asymmetric in both axes on purpose --- a square box in the middle maps to
  // something plausible under every turn and can distinguish none of them.
  const page = { width: 100, height: 200 };
  const box = { left: 10, top: 20, right: 30, bottom: 60 };

  it("leaves an unrotated box alone", () => {
    expect(turnQuad(box, 0, page.width, page.height)).toEqual(box);
  });

  it("sends the top-left corner clockwise round the page", () => {
    // The load-bearing one. A box in the top-left must appear top-right,
    // bottom-right and bottom-left in turn, and this is the only assertion here
    // that can tell a quarter turn from a three-quarter one --- both swap the
    // page's dimensions identically.
    const corners = [1, 2, 3].map((turns) => {
      const [width, height] =
        turns % 2 === 1 ? [page.height, page.width] : [page.width, page.height];
      const turned = turnQuad(box, turns, page.width, page.height);
      return [turned.left < width / 2, turned.top < height / 2];
    });
    expect(corners).toEqual([
      [false, true], // top-right
      [false, false], // bottom-right
      [true, false], // bottom-left
    ]);
  });

  it("swaps the box's own proportions on a quarter turn", () => {
    // 20 wide by 40 tall becomes 40 wide by 20 tall. Without this, a turn that
    // put the corner in the right place but transposed the extents would pass
    // the check above.
    const turned = turnQuad(box, 1, page.width, page.height);
    expect(turned.right - turned.left).toBe(40);
    expect(turned.bottom - turned.top).toBe(20);
  });

  it("never returns a box inside out", () => {
    // A highlight with a negative width simply does not draw, which reads as
    // "selection is broken" rather than as a coordinate bug.
    for (const turns of [0, 1, 2, 3]) {
      const turned = turnQuad(box, turns, page.width, page.height);
      expect(turned.left).toBeLessThanOrEqual(turned.right);
      expect(turned.top).toBeLessThanOrEqual(turned.bottom);
    }
  });

  it("comes back to where it started after four turns", () => {
    // Composition rather than a table: four quarter turns are the identity, and
    // getting there requires every intermediate step to be self-consistent.
    let quad = box;
    let [width, height] = [page.width, page.height];
    for (let step = 0; step < 4; step++) {
      quad = turnQuad(quad, 1, width, height);
      [width, height] = [height, width];
    }
    expect(quad).toEqual(box);
  });

  it("treats a rotation beyond a full turn as the turn it means", () => {
    expect(turnQuad(box, 4, page.width, page.height)).toEqual(box);
    expect(turnQuad(box, -1, page.width, page.height)).toEqual(
      turnQuad(box, 3, page.width, page.height),
    );
  });
});

describe("turnedView", () => {
  /** Two lines of two characters on a 100x100 page, reading left to right. */
  function twoShortLines(): PageText {
    return page([
      ["a", [10, 10, 20, 20]],
      ["b", [20, 10, 30, 20]],
      ["c", [10, 40, 20, 50]],
      ["d", [20, 40, 30, 50]],
    ]);
  }

  it("swaps the page's dimensions on a quarter turn", () => {
    const text: PageText = { ...twoShortLines(), width_pt: 100, height_pt: 200 };
    const turned = turnedView(text, 1);
    expect(turned.width_pt).toBe(200);
    expect(turned.height_pt).toBe(100);
    expect(turnedView(text, 2).width_pt).toBe(100);
  });

  it("accumulates onto the page's own rotation", () => {
    // What keeps `linesRunSideways` right: an upright page looked at sideways
    // reads down the screen exactly as a /Rotate 90 page does.
    const upright = twoShortLines();
    expect(linesRunSideways(upright)).toBe(false);
    expect(linesRunSideways(turnedView(upright, 1))).toBe(true);

    const scanned: PageText = { ...upright, quarter_turns: 1 };
    expect(linesRunSideways(turnedView(scanned, 1))).toBe(false);
    expect(turnedView(scanned, 3).quarter_turns).toBe(0);
  });

  it("keeps the same characters on the same lines", () => {
    // A rotation is an isometry, so the grouping cannot change --- which is why
    // a screen reader still hears a rotated page in its own order. The check
    // that matters is that this holds after the *axis* has swapped too, i.e.
    // that both halves of the turn were applied and not just one.
    const upright = twoShortLines();
    for (const turns of [1, 2, 3]) {
      expect(linesOf(turnedView(upright, turns))).toEqual(linesOf(upright));
    }
  });

  it("leaves a character PDFium gave no box alone", () => {
    // Four zeroes means "no box". Turning it would invent one in a corner, and
    // `isPlaced` would then believe it --- putting a phantom character into a
    // line, and into the text a screen reader reads.
    const text = page([
      ["a", [10, 10, 20, 20]],
      [" ", null],
    ]);
    expect(turnedView(text, 1).boxes.slice(4)).toEqual([0, 0, 0, 0]);
  });

  it("returns the page itself when nothing is rotated", () => {
    const text = twoShortLines();
    expect(turnedView(text, 0)).toBe(text);
  });
});
