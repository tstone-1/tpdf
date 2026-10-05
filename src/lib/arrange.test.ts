import { describe, expect, it } from "vitest";
import { ARRANGEMENTS, Picked, arrange, differs, needs, together } from "./arrange";
import type { Quad } from "./markband";

const q = (left: number, top: number, right: number, bottom: number): Quad => ({ left, top, right, bottom });
const PAGE = { width: 600, height: 800 };
// The first is the one the others follow: 100 wide, 20 high, at (100, 100).
const first = q(100, 100, 200, 120);
const wide = q(250, 300, 450, 340);
const small = q(40, 500, 70, 510);
const three = [first, wide, small];

describe("arrange", () => {
  it("lines an edge of every mark up with the first mark's, and keeps each size", () => {
    expect(arrange(three, "left", PAGE)).toEqual([first, q(100, 300, 300, 340), q(100, 500, 130, 510)]);
    expect(arrange(three, "right", PAGE)).toEqual([first, q(0, 300, 200, 340), q(170, 500, 200, 510)]);
    expect(arrange(three, "top", PAGE)).toEqual([first, q(250, 100, 450, 140), q(40, 100, 70, 110)]);
    expect(arrange(three, "bottom", PAGE)).toEqual([first, q(250, 80, 450, 120), q(40, 110, 70, 120)]);
  });

  it("lines the centres up with the first mark's", () => {
    // The first mark's centre is at x 150 and y 110.
    expect(arrange(three, "center", PAGE)).toEqual([first, q(50, 300, 250, 340), q(135, 500, 165, 510)]);
    expect(arrange(three, "middle", PAGE)).toEqual([first, q(250, 90, 450, 130), q(40, 105, 70, 115)]);
  });

  it("follows whichever mark was picked first", () => {
    expect(arrange([wide, first], "left", PAGE)).toEqual([wide, q(250, 100, 350, 120)]);
  });

  it("gives every mark the first mark's width, height or both, from its own corner", () => {
    expect(arrange(three, "sameWidth", PAGE)).toEqual([first, q(250, 300, 350, 340), q(40, 500, 140, 510)]);
    expect(arrange(three, "sameHeight", PAGE)).toEqual([first, q(250, 300, 450, 320), q(40, 500, 70, 520)]);
    expect(arrange(three, "sameSize", PAGE)).toEqual([first, q(250, 300, 350, 320), q(40, 500, 140, 520)]);
  });

  it("spaces the marks between the outer two evenly, whatever order they were picked in", () => {
    // Widths 20, 40 and 60 between x 0 and 300: 180 of space, 90 a gap.
    const a = q(0, 0, 20, 10);
    const b = q(30, 50, 70, 60);
    const c = q(240, 90, 300, 100);
    expect(arrange([c, a, b], "distributeAcross", PAGE)).toEqual([c, a, q(110, 50, 150, 60)]);
    // And down: heights 10 each between y 0 and 100, 35 a gap.
    expect(arrange([b, c, a], "distributeDown", PAGE)).toEqual([q(30, 45, 70, 55), c, a]);
    // Four, so there are two to place: 160 of space in three gaps. By centre
    // the order is a, b, d, c, so b starts at 73.3 and d at 166.7.
    const d = q(100, 0, 120, 10);
    const lefts = arrange([a, d, b, c], "distributeAcross", PAGE).map((quad) => quad.left);
    expect(lefts[0]).toBe(0);
    expect(lefts[1]).toBeCloseTo(166.667, 2);
    expect(lefts[2]).toBeCloseTo(73.333, 2);
    expect(lefts[3]).toBeCloseTo(240, 6);
  });

  it("centres the picked marks on the page as one block", () => {
    // Together they span x 40 to 450 and y 100 to 510.
    expect(arrange(three, "pageCenter", PAGE)).toEqual(three.map((quad) => q(quad.left + 55, quad.top, quad.right + 55, quad.bottom)));
    expect(arrange(three, "pageMiddle", PAGE)).toEqual(three.map((quad) => q(quad.left, quad.top + 95, quad.right, quad.bottom + 95)));
    // One mark is enough.
    expect(arrange([first], "pageCenter", PAGE)).toEqual([q(250, 100, 350, 120)]);
  });

  it("keeps every mark on the page", () => {
    // A wide mark whose centre is lined up with one near the left edge.
    const edge = q(10, 10, 30, 30);
    expect(arrange([edge, q(300, 100, 500, 120)], "center", PAGE)).toEqual([edge, q(0, 100, 200, 120)]);
    // And one grown past the right and the bottom edge.
    const big = q(100, 100, 500, 700);
    expect(arrange([big, q(400, 300, 420, 320)], "sameSize", PAGE)).toEqual([big, q(200, 200, 600, 800)]);
  });

  it("changes nothing with fewer marks than the arrangement needs, or one that is not finite", () => {
    expect(arrange([first], "left", PAGE)).toEqual([first]);
    expect(arrange([first, wide], "distributeAcross", PAGE)).toEqual([first, wide]);
    expect(arrange([], "pageCenter", PAGE)).toEqual([]);
    const broken = [first, q(Number.NaN, 0, 10, 10)];
    expect(arrange(broken, "left", PAGE)).toEqual(broken);
  });

  it("says how many marks each arrangement needs", () => {
    expect(ARRANGEMENTS.map((how) => [how, needs(how)])).toEqual([
      ["left", 2], ["center", 2], ["right", 2], ["top", 2], ["middle", 2], ["bottom", 2],
      ["distributeAcross", 3], ["distributeDown", 3],
      ["sameWidth", 2], ["sameHeight", 2], ["sameSize", 2],
      ["pageCenter", 1], ["pageMiddle", 1],
    ]);
  });

  it("tells a moved rectangle from one within rounding of where it was", () => {
    expect(differs(first, first)).toBe(false);
    expect(differs(first, q(100.005, 100, 200, 120))).toBe(false);
    for (const other of [q(100.1, 100, 200, 120), q(100, 100.1, 200, 120), q(100, 100, 200.1, 120), q(100, 100, 200, 120.1)]) {
      expect(differs(first, other)).toBe(true);
    }
  });
});

describe("Picked", () => {
  it("keeps the order marks were picked in, and takes one out when it is picked again", () => {
    const picked = new Picked();
    picked.only(4, 0);
    picked.toggle(9, 0);
    picked.toggle(2, 0);
    expect(picked.list()).toEqual([4, 9, 2]);
    expect(picked.count).toBe(3);
    picked.toggle(9, 0);
    expect(picked.list()).toEqual([4, 2]);
    expect(picked.has(9)).toBe(false);
    expect(picked.has(2)).toBe(true);
  });

  it("starts again on another page", () => {
    const picked = new Picked();
    picked.toggle(4, 0);
    picked.toggle(9, 0);
    picked.toggle(7, 1);
    expect(picked.list()).toEqual([7]);
    // And a page emptied by toggling is no page: the next pick starts there.
    picked.toggle(7, 1);
    picked.toggle(4, 0);
    picked.toggle(9, 0);
    expect(picked.list()).toEqual([4, 9]);
  });

  it("picks one alone, clears, and drops marks that are gone", () => {
    const picked = new Picked();
    picked.toggle(4, 0);
    picked.toggle(9, 0);
    picked.only(2, 0);
    expect(picked.list()).toEqual([2]);
    picked.toggle(5, 0);
    picked.keep((id) => id !== 2);
    expect(picked.list()).toEqual([5]);
    picked.keep(() => false);
    picked.toggle(8, 3);
    picked.toggle(6, 3);
    expect(picked.list()).toEqual([8, 6]);
    picked.clear();
    expect(picked.count).toBe(0);
  });

  it("follows its page to another slot, and goes with a page that is gone", () => {
    const picked = new Picked();
    picked.toggle(4, 2);
    picked.toggle(9, 2);
    // The page moved up one: the marks stay picked, and the next one is added
    // from the slot the page is in now and not from the one it left.
    picked.repage((slot) => slot - 1);
    expect(picked.list()).toEqual([4, 9]);
    picked.toggle(7, 1);
    expect(picked.list()).toEqual([4, 9, 7]);
    picked.toggle(3, 2);
    expect(picked.list()).toEqual([3]);
    // The page left the document.
    picked.repage(() => undefined);
    expect(picked.count).toBe(0);
    // Nothing picked is on no page, and is not asked about.
    let asked = 0;
    picked.repage(() => { asked += 1; return 0; });
    expect(asked).toBe(0);
  });
});

describe("moving several together", () => {
  const page = { width: 600, height: 800 };
  const two: Quad[] = [
    { left: 100, top: 100, right: 200, bottom: 120 },
    { left: 40, top: 500, right: 70, bottom: 510 },
  ];

  it("moves by what was asked while every one stays on the page", () => {
    expect(together(two, { dx: 10, dy: -20 }, page)).toEqual({ dx: 10, dy: -20 });
    expect(together(two, { dx: 0, dy: 0 }, page)).toEqual({ dx: 0, dy: 0 });
  });

  it("stops all of them where the one nearest an edge stops, on each of the four sides", () => {
    // The second is 40 from the left; the first is 100 from the top, 400 from
    // the right; the second 290 from the bottom.
    expect(together(two, { dx: -90, dy: 0 }, page)).toEqual({ dx: -40, dy: 0 });
    expect(together(two, { dx: 0, dy: -150 }, page)).toEqual({ dx: 0, dy: -100 });
    expect(together(two, { dx: 900, dy: 0 }, page)).toEqual({ dx: 400, dy: 0 });
    expect(together(two, { dx: 0, dy: 900 }, page)).toEqual({ dx: 0, dy: 290 });
    // Exactly to the edge is allowed.
    expect(together(two, { dx: -40, dy: 290 }, page)).toEqual({ dx: -40, dy: 290 });
  });

  it("moves nothing for no rectangles or an offset that is not a number", () => {
    expect(together([], { dx: 5, dy: 5 }, page)).toEqual({ dx: 0, dy: 0 });
    expect(together(two, { dx: Number.NaN, dy: 5 }, page)).toEqual({ dx: 0, dy: 0 });
    expect(together(two, { dx: 5, dy: Number.POSITIVE_INFINITY }, page)).toEqual({ dx: 0, dy: 0 });
  });
});
