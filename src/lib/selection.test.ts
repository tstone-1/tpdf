/**
 * `areasFrom`, which is the seam between a text selection and a redaction.
 *
 * The loop that calls it lives in `App.svelte`, which no unit test imports and
 * no harness constructs --- so the decision that can be wrong is here on
 * purpose rather than there. See `docs/TRAPS.md` on a feature that is inert in
 * the application while three layers of tests pass.
 */
import { describe, expect, it } from "vitest";
import { MIN_REDACTION_SIDE, areasFrom } from "./selection";

describe("areasFrom", () => {
  it("turns one run into one region", () => {
    expect(areasFrom([10, 20, 110, 32])).toEqual([[10, 20, 110, 32]]);
  });

  /**
   * Three runs, three regions, in the order they arrived.
   *
   * **Not one box around them**, which is the decision this asserts: a bounding
   * box over these three covers everything between the lines, and on a
   * two-column page that is the other column. The three lines are deliberately
   * at different left edges, so a bounding box is a different answer rather
   * than the same one arrived at differently.
   */
  it("turns three runs into three regions, and not into one box around them", () => {
    const areas = areasFrom([10, 20, 110, 32, 40, 40, 90, 52, 10, 60, 200, 72]);
    expect(areas).toEqual([
      [10, 20, 110, 32],
      [40, 40, 90, 52],
      [10, 60, 200, 72],
    ]);
    // The box the wrong answer would have produced, stated so the two cannot
    // agree by accident.
    expect(areas).not.toContainEqual([10, 20, 200, 72]);
  });

  /**
   * A run with no width contributes nothing.
   *
   * A selection that ends exactly where a line does gives an empty run at that
   * line's end. A region with no area holds no glyph's centre, so it can only
   * ever remove nothing --- and a row in the review list that will never remove
   * anything makes the list overstate what is about to happen.
   *
   * **Two runs, so the check discriminates.** With only the empty one, an
   * implementation that returned nothing at all would pass.
   */
  it("drops a run with no width and keeps the one beside it", () => {
    expect(areasFrom([10, 20, 10, 32, 10, 40, 110, 52])).toEqual([
      [10, 40, 110, 52],
    ]);
  });

  /** The other side of the same rule, which a width-only check cannot see. */
  it("drops a run with no height and keeps the one beside it", () => {
    expect(areasFrom([10, 20, 110, 20, 10, 40, 110, 52])).toEqual([
      [10, 40, 110, 52],
    ]);
  });

  /**
   * The bound is on the side, not on the area.
   *
   * A run a hundredth of a point wide and two hundred long has a larger area
   * than many real words and is still nothing. Just under and just over, so the
   * check fails in both directions rather than only when the bound is deleted.
   */
  it("measures the bound against each side rather than against the area", () => {
    const thin = MIN_REDACTION_SIDE / 2;
    expect(areasFrom([10, 20, 10 + thin, 220])).toEqual([]);
    const wide = MIN_REDACTION_SIDE * 2;
    expect(areasFrom([10, 20, 10 + wide, 220])).toEqual([[10, 20, 10 + wide, 220]]);
  });

  /**
   * The sides come out ordered whichever way they went in.
   *
   * Nothing in the viewer produces a run with its right edge left of its left
   * one. A region is a claim about what will be destroyed, so it costs two
   * comparisons to stop depending on that --- and an unordered region would be
   * measured as negative width and dropped by the bound above, which is a
   * silent nothing rather than a visible wrong.
   */
  it("orders the sides rather than trusting them", () => {
    expect(areasFrom([110, 32, 10, 20])).toEqual([[10, 20, 110, 32]]);
  });

  /** No runs, no regions --- and no region invented to stand for them. */
  it("makes nothing out of nothing", () => {
    expect(areasFrom([])).toEqual([]);
  });

  /**
   * A trailing group of fewer than four numbers is dropped rather than read.
   *
   * It cannot arrive from `selectionQuadsByPage`, which builds the array four
   * at a time. That is exactly why the loop must not read past the end on the
   * day something else calls this: the bound is what stops a `NaN` reaching a
   * region, and a `NaN` region is not equal to itself.
   */
  it("drops a trailing group that is not a whole run", () => {
    expect(areasFrom([10, 20, 110, 32, 10, 40])).toEqual([[10, 20, 110, 32]]);
  });
});
