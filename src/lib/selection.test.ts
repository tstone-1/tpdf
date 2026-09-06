/**
 * `areasFrom`, which is the seam between a text selection and a redaction.
 *
 * The loop that calls it lives in `App.svelte`, which no unit test imports and
 * no harness constructs --- so the decision that can be wrong is here on
 * purpose rather than there. See `docs/TRAPS.md` on a feature that is inert in
 * the application while three layers of tests pass.
 */
import { describe, expect, it } from "vitest";
import { MIN_REDACTION_SIDE, Selection, areasFrom } from "./selection";
import type { PageText } from "./text";

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


describe("Selection.hasText", () => {
  /** A page of `chars` letters, each with a box, laid out left to right. */
  function page(chars: number): PageText {
    const codes: number[] = [];
    const boxes: number[] = [];
    for (let at = 0; at < chars; at++) {
      codes.push(97 + (at % 26));
      boxes.push(at * 10, 0, at * 10 + 10, 12);
    }
    return {
      codes,
      boxes,
      width_pt: 600,
      height_pt: 800,
      quarter_turns: 0,
      extract_ms: 0,
    };
  }

  /** A selection from one caret to another. */
  function from(
    start: { page: number; index: number },
    end: { page: number; index: number },
  ): Selection {
    const selection = new Selection(start);
    selection.focus = end;
    return selection;
  }

  it("agrees with the string it stands in for", () => {
    // The whole contract: this exists to answer `text(look) !== ""` without
    // building the string, so the string is the oracle rather than a set of
    // expectations that would have to be right twice.
    const look = (): PageText => page(20);
    for (const [start, end] of [
      [
        { page: 0, index: 0 },
        { page: 0, index: 0 },
      ],
      [
        { page: 0, index: 0 },
        { page: 0, index: 5 },
      ],
      [
        { page: 0, index: 5 },
        { page: 0, index: 0 },
      ],
      [
        { page: 0, index: 3 },
        { page: 2, index: 4 },
      ],
    ] as const) {
      const selection = from(start, end);
      expect(selection.hasText(look)).toBe(selection.text(look) !== "");
    }
  });

  it("says nothing is selected when no page has arrived", () => {
    // A selection over pages the cache does not hold contributes nothing, which
    // is what `text` answers too --- and the guard that reads this must not
    // offer a scope over text nobody can see.
    const selection = from({ page: 0, index: 0 }, { page: 3, index: 4 });
    expect(selection.hasText(() => null)).toBe(false);
  });

  it("stops at the first page that contributes", () => {
    // The point of it. A select-all over a long document built every page's
    // text in reading order, joined the lot and compared it with "" --- on the
    // frame loop, because a menu guard reads it.
    let asked = 0;
    const selection = from({ page: 0, index: 0 }, { page: 400, index: 4 });
    selection.hasText(() => {
      asked++;
      return page(20);
    });
    expect(asked).toBe(1);
  });

  it("counts the separator two empty pages would be joined by", () => {
    // The odd case, kept rather than tidied away: a selection running from the
    // end of one page to the start of the next holds no letters, and `text`
    // returns the newline between them. Whether that should enable the scope
    // toggle is a real question and a separate one --- what is asserted here is
    // that making the guard cheap did not silently answer it.
    const selection = from({ page: 0, index: 20 }, { page: 1, index: 0 });
    const look = (): PageText => page(20);
    expect(selection.text(look)).toBe("\n");
    expect(selection.hasText(look)).toBe(true);
  });
});
