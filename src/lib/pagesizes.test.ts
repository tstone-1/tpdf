import { describe, expect, it } from "vitest";

import {
  PAGE_SIZES,
  PAGE_SIZE_NAMES,
  dimensionsOf,
  type PageSizeName,
} from "./pagesizes";

/**
 * The sizes, checked against facts that are not the table.
 *
 * **A test that reads each number back out of the table it came from is the
 * writer agreeing with its own reader**, which `docs/TRAPS.md` records as one of
 * the ways an assertion cannot fail. So nothing below transcribes a constant.
 * What it asserts instead are the relations the standards define --- the A
 * series halves along its long edge, so each size's width is the next larger
 * one's height, and every sheet has the same aspect ratio of root two --- and
 * the property that separates the two families: US sizes are defined in inches,
 * so at 72 points to the inch they are whole numbers, while no A size is.
 *
 * A transposed pair, which is the mistake this module exists to make
 * impossible at the call site, fails the portrait check *and* the ratio one.
 */
describe("the page sizes a reader can insert", () => {
  it("lists every name in the table, and nothing else", () => {
    expect(PAGE_SIZE_NAMES).toEqual(Object.keys(PAGE_SIZES));
    expect(PAGE_SIZE_NAMES.length).toBeGreaterThan(1);
  });

  it("is portrait throughout, so turning is what makes a landscape page", () => {
    for (const name of PAGE_SIZE_NAMES) {
      const size = PAGE_SIZES[name];
      expect(
        size.height,
        `${name} must be taller than it is wide`,
      ).toBeGreaterThan(size.width);
    }
  });

  it("gives every A size the ratio the series is defined by", () => {
    // Root two, which is what makes halving a sheet along its long edge give
    // the next size down with the same shape.
    //
    // **Within half a percent, and it cannot be tighter, which is a fact about
    // ISO 216 rather than a weakened assertion.** The series is defined by the
    // ratio and then every size is *rounded to whole millimetres*, so no real
    // sheet has it exactly: A5 is 148 x 210, which is 1.41892 against 1.41421,
    // out by a third of a percent. A tolerance of a thousandth was written here
    // first and failed on A5 --- correctly, and about the standard rather than
    // about the table. Half a percent still refuses a transposed pair, which
    // reads 0.705.
    const a: PageSizeName[] = ["a3", "a4", "a5"];
    for (const name of a) {
      const size = PAGE_SIZES[name];
      const ratio = size.height / size.width;
      expect(
        Math.abs(ratio - Math.SQRT2) / Math.SQRT2,
        `${name} is ${ratio}`,
      ).toBeLessThan(0.005);
    }
  });

  it("halves each A size into the next one down", () => {
    // A4's height is A3's width, and A5's height is A4's width. Two links, and
    // both are needed: one alone is satisfied by a table in which the other
    // size is anything at all.
    expect(PAGE_SIZES.a4.height).toBeCloseTo(PAGE_SIZES.a3.width, 2);
    expect(PAGE_SIZES.a5.height).toBeCloseTo(PAGE_SIZES.a4.width, 2);
  });

  it("puts the US sizes on whole points and the A sizes on none", () => {
    // The discriminating property between the two families, and the reason it
    // is worth an assertion: a US size that has drifted off a whole point has
    // been mistyped, because 8.5 x 11 inches at 72 points to the inch is exact.
    for (const name of ["letter", "legal"] as PageSizeName[]) {
      const size = PAGE_SIZES[name];
      expect(Number.isInteger(size.width), name).toBe(true);
      expect(Number.isInteger(size.height), name).toBe(true);
    }
    // And the control, without which the assertion above is satisfied by a
    // table of whole numbers throughout: no A size lands on one.
    for (const name of ["a3", "a4", "a5"] as PageSizeName[]) {
      const size = PAGE_SIZES[name];
      expect(
        Number.isInteger(size.width) && Number.isInteger(size.height),
        `${name} is a conversion from millimetres and cannot be whole`,
      ).toBe(false);
    }
  });

  it("answers width first, then height", () => {
    // The order the model's `Command::Insert` takes, and the transposition this
    // module exists to keep out of `App.svelte`. Asserted against the portrait
    // property rather than against the numbers: the first of the pair is the
    // smaller one for every size in the table, which no transposed answer is.
    for (const name of PAGE_SIZE_NAMES) {
      const [width, height] = dimensionsOf(name);
      expect(width, `${name} width`).toBeLessThan(height);
      expect(width).toBe(PAGE_SIZES[name].width);
      expect(height).toBe(PAGE_SIZES[name].height);
    }
  });
});
