/**
 * The content box read from a page's pixels, for fit content.
 */

import { describe, expect, it } from "vitest";

import { inkBounds, PAPER_TOLERANCE, rowContent } from "./inkbounds";

/** A `width` x `height` bitmap of `paper`, with `ink` painted on the listed pixels. */
function bitmap(width: number, height: number, paper: number, ink: number, at: [number, number][]): Uint8ClampedArray {
  const data = new Uint8ClampedArray(width * height * 4).fill(paper);
  for (const [x, y] of at) data.fill(ink, (y * width + x) * 4, (y * width + x) * 4 + 3);
  return data;
}

describe("inkBounds", () => {
  it("boxes the printed pixels, one pixel wider on each side", () => {
    const box = inkBounds(bitmap(10, 20, 255, 0, [[3, 5], [6, 14]]), 10, 20);
    expect(box).toEqual({ left: 2 / 10, top: 4 / 20, right: 8 / 10, bottom: 16 / 20 });
  });

  it("stays inside the bitmap for ink at its edges", () => {
    const box = inkBounds(bitmap(10, 20, 255, 0, [[0, 0], [9, 19]]), 10, 20);
    expect(box).toEqual({ left: 0, top: 0, right: 1, bottom: 1 });
  });

  it("finds nothing on a blank page", () => {
    expect(inkBounds(bitmap(10, 20, 255, 0, []), 10, 20)).toBeNull();
  });

  it("takes the paper's colour from the page, so an inverted page is measured too", () => {
    // White on black, as the viewer draws a page with inverting on.
    const box = inkBounds(bitmap(10, 20, 0, 255, [[4, 8]]), 10, 20);
    expect(box).toEqual({ left: 3 / 10, top: 7 / 20, right: 6 / 10, bottom: 10 / 20 });
  });

  it("reads paper that wanders a little, as a scan's does, as paper", () => {
    const grain = bitmap(10, 20, 255, 255 - PAPER_TOLERANCE, [[1, 1], [8, 18]]);
    expect(inkBounds(grain, 10, 20)).toBeNull();
    const mark = bitmap(10, 20, 255, 255 - PAPER_TOLERANCE - 1, [[5, 5]]);
    expect(inkBounds(mark, 10, 20)).not.toBeNull();
  });

  it("answers nothing for a bitmap shorter than its size says", () => {
    expect(inkBounds(new Uint8ClampedArray(12), 10, 20)).toBeNull();
  });
});

describe("rowContent", () => {
  const box = (left: number, top: number, right: number, bottom: number) => ({ left, top, right, bottom });

  it("is the page's content box on a page alone", () => {
    expect(rowContent([{ width_pt: 600, height_pt: 800, ink: box(0.1, 0.25, 0.6, 0.75) }])).toEqual({
      width_pt: 300, height_pt: 400, gaps: 0, first: 0, left_pt: 60, top_pt: 200,
    });
  });

  it("spans facing pages from the first content to the last, gap included", () => {
    const content = rowContent([
      { width_pt: 600, height_pt: 800, ink: box(0.1, 0.1, 0.9, 0.5) },
      { width_pt: 400, height_pt: 1000, ink: box(0.2, 0.05, 0.5, 0.9) },
    ]);
    // 540 points left of the gap, 200 right of it; tops 80 and 50, bottoms 400 and 900.
    expect(content).toEqual({ width_pt: 740, height_pt: 850, gaps: 1, first: 0, left_pt: 60, top_pt: 50 });
  });

  it("leaves out a blank page beside a printed one", () => {
    const content = rowContent([
      { width_pt: 600, height_pt: 800, ink: null },
      { width_pt: 600, height_pt: 800, ink: box(0.5, 0.5, 1, 1) },
    ]);
    expect(content).toEqual({ width_pt: 300, height_pt: 400, gaps: 0, first: 1, left_pt: 300, top_pt: 400 });
  });

  it("has nothing to fit on blank pages", () => {
    expect(rowContent([{ width_pt: 600, height_pt: 800, ink: null }])).toBeNull();
  });
});
