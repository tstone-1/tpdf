import { describe, expect, it } from "vitest";

import {
  boxQuad,
  ICON_SIZE,
  LINE_FRACTION,
  MIN_BOX,
  iconQuad,
  isIcon,
  isOutline,
  isWash,
  markBand,
  type Quad,
} from "./markband";

/**
 * A line of 12 pt text, in the overlay's top-down coordinates.
 *
 * Deliberately not square and deliberately not at the origin: a band at the
 * wrong edge of a square quad centred at zero is hard to tell from the right
 * one, and this repository has a trap about exactly that fixture.
 */
const LINE: Quad = { left: 100, top: 200, right: 340, bottom: 212 };

describe("which kinds are a wash", () => {
  it("is the highlight, and only the highlight", () => {
    // `is_wash` in `save.rs`. All three named rather than the one, because the
    // defect this file exists to stop was every kind being treated alike --- an
    // assertion about the highlight alone is satisfied by that too.
    expect(isWash("highlight")).toBe(true);
    expect(isWash("underline")).toBe(false);
    expect(isWash("strikeout")).toBe(false);
  });
});

describe("which kinds are an icon", () => {
  it("is the comment, and only the comment", () => {
    // `is_note` in `save.rs`, and all four named for the reason the wash sweep
    // above names three: a predicate answering true for everything satisfies a
    // one-kind assertion perfectly.
    expect(isIcon("note")).toBe(true);
    expect(isIcon("highlight")).toBe(false);
    expect(isIcon("underline")).toBe(false);
    expect(isIcon("strikeout")).toBe(false);
  });

  it("is not a wash, so the two questions do not collapse", () => {
    // The pair that matters. A comment is neither ink over words nor a rule
    // across them, and a `isWash` that answered true for it would draw the
    // bubble multiplied into the paper.
    expect(isWash("note")).toBe(false);
    expect(isIcon("highlight")).toBe(false);
  });
});

describe("where a comment's icon lands", () => {
  const A4 = { width: 595, height: 842 };

  it("puts the icon's top-left where the reader pointed", () => {
    expect(iconQuad(100, 200, A4)).toEqual({
      left: 100,
      top: 200,
      right: 100 + ICON_SIZE,
      bottom: 200 + ICON_SIZE,
    });
  });

  it("keeps a comment dropped at the far edge inside the page", () => {
    // The clamp. Without it `save.rs` writes a `/Rect` running past the page
    // box --- it maps quads, it does not police them --- and other readers then
    // clip the icon, draw half of it, or place it somewhere of their own.
    const corner = iconQuad(A4.width - 2, A4.height - 2, A4);
    expect(corner.right).toBe(A4.width);
    expect(corner.bottom).toBe(A4.height);
    expect(corner.left).toBe(A4.width - ICON_SIZE);
    expect(corner.top).toBe(A4.height - ICON_SIZE);
  });

  it("keeps one dropped past the top-left inside too", () => {
    // The other direction, which a one-sided clamp would fail. A negative
    // coordinate is reachable: `pageAndPoint` maps a press in the grey area
    // beside a page into that page's space.
    const off = iconQuad(-50, -50, A4);
    expect(off.left).toBe(0);
    expect(off.top).toBe(0);
  });

  it("is square, and the size other readers draw theirs at", () => {
    const quad = iconQuad(10, 10, A4);
    expect(quad.right - quad.left).toBe(ICON_SIZE);
    expect(quad.bottom - quad.top).toBe(ICON_SIZE);
  });

  it("gives a comment its whole quad, which the bubble is drawn inside", () => {
    // The one kind where `markBand` returning the quad unchanged is right ---
    // and it is the same answer the shipped defect gave every kind, so this
    // assertion is the reason `tells the two line kinds apart` above has to
    // exist beside it.
    const quad = iconQuad(10, 10, A4);
    expect(markBand("note", quad)).toEqual(quad);
  });
});

describe("where a mark's ink goes", () => {
  it("gives a highlight the whole quad", () => {
    expect(markBand("highlight", LINE)).toEqual(LINE);
  });

  it("sits an underline on the bottom edge", () => {
    const band = markBand("underline", LINE);
    // 12 pt of text at 7% is 0.84 pt of rule, against the bottom.
    expect(band.bottom).toBe(212);
    expect(band.top).toBeCloseTo(212 - 0.84, 10);
    // The horizontal extent is the text's, untouched. Worth asserting: a band
    // that got the height right and the width wrong would still look plausible
    // on the one line a screenshot shows.
    expect(band.left).toBe(100);
    expect(band.right).toBe(340);
  });

  it("centres a strikeout on the text", () => {
    const band = markBand("strikeout", LINE);
    const middle = 206;
    expect((band.top + band.bottom) / 2).toBeCloseTo(middle, 10);
    expect(band.bottom - band.top).toBeCloseTo(0.84, 10);
  });

  it("keeps both lines inside the quad they mark", () => {
    // The reason `save.rs` states for expressing the two separately: the saved
    // file's `/BBox` is the bounds of the quads, so anything drawn outside is
    // clipped. An underline centred *on* the bottom edge would lose half its
    // thickness there and sit half a line low here, and neither would look
    // like a bug --- it would look like a thinner line.
    for (const kind of ["underline", "strikeout"] as const) {
      const band = markBand(kind, LINE);
      expect(band.top, kind).toBeGreaterThanOrEqual(LINE.top);
      expect(band.bottom, kind).toBeLessThanOrEqual(LINE.bottom);
    }
  });

  it("tells the two line kinds apart", () => {
    // The discrimination itself, and it is the assertion that would have caught
    // the shipped defect: a rule that returned the same band for both, or the
    // whole quad for both, satisfies every bound above.
    expect(markBand("underline", LINE)).not.toEqual(markBand("strikeout", LINE));
    expect(markBand("underline", LINE)).not.toEqual(LINE);
    expect(markBand("strikeout", LINE)).not.toEqual(LINE);
  });

  it("scales the line with the text rather than fixing it", () => {
    // `LINE_FRACTION`'s whole reason: a fixed thickness is a hairline across a
    // heading. Measured on a 36 pt line against the 12 pt one, so a constant
    // would make the two equal.
    const heading: Quad = { left: 0, top: 0, right: 200, bottom: 36 };
    const thick = markBand("underline", heading);
    const thin = markBand("underline", LINE);
    expect(thick.bottom - thick.top).toBeCloseTo(36 * LINE_FRACTION, 10);
    expect(thick.bottom - thick.top).toBeGreaterThan(thin.bottom - thin.top);
  });
});

describe("which kinds are drawn how", () => {
  it("says a box is drawn as an outline and the others are not", () => {
    // The three questions this file answers about a kind are separate on
    // purpose --- `save.rs` splits them the same way --- and each needs the
    // other four kinds beside it or it is satisfied by a predicate that is
    // always true.
    expect(isOutline("square")).toBe(true);
    for (const kind of ["highlight", "underline", "strikeout", "note"] as const) {
      expect(isOutline(kind), kind).toBe(false);
    }
  });

  it("keeps the three questions apart", () => {
    // A box is not a wash and not an icon; a comment is an icon and neither of
    // the others. Written out because collapsing any two of the three is the
    // change that looks harmless: it was one predicate doing two jobs until the
    // box arrived, and the box is what separated them.
    expect(isWash("square")).toBe(false);
    expect(isIcon("square")).toBe(false);
    expect(isOutline("note")).toBe(false);
    expect(isIcon("note")).toBe(true);
  });

  it("gives a box the whole quad, because the quad is the mark", () => {
    const box: Quad = { left: 100, top: 200, right: 340, bottom: 260 };
    expect(markBand("square", box)).toEqual(box);
  });
});

describe("the rectangle a drag makes", () => {
  /** A page big enough that nothing below is clamped unless it says so. */
  const PAGE = { width: 600, height: 800 };

  it("takes an ordinary drag as it was made", () => {
    expect(boxQuad({ x: 100, y: 200 }, { x: 340, y: 260 }, PAGE)).toEqual({
      left: 100,
      top: 200,
      right: 340,
      bottom: 260,
    });
  });

  it("normalises the corners whichever way the drag went", () => {
    // All four directions between the same two points, which must give one
    // rectangle. Subtracting in arrival order gives an inside-out one for
    // three of them, and an inside-out rectangle does not draw at all.
    const want = { left: 100, top: 200, right: 340, bottom: 260 };
    expect(boxQuad({ x: 340, y: 260 }, { x: 100, y: 200 }, PAGE)).toEqual(want);
    expect(boxQuad({ x: 340, y: 200 }, { x: 100, y: 260 }, PAGE)).toEqual(want);
    expect(boxQuad({ x: 100, y: 260 }, { x: 340, y: 200 }, PAGE)).toEqual(want);
  });

  it("clamps a drag that left the page, in both directions", () => {
    // One flick of the wrist at the edge. `save.rs` maps quads and does not
    // police them, so an unclamped drag writes a /Rect past the page box.
    expect(boxQuad({ x: -50, y: -80 }, { x: 900, y: 1200 }, PAGE)).toEqual({
      left: 0,
      top: 0,
      right: 600,
      bottom: 800,
    });
  });

  it("refuses a click", () => {
    expect(boxQuad({ x: 100, y: 200 }, { x: 100, y: 200 }, PAGE)).toBe(null);
  });

  it("refuses a box that is tall and thin", () => {
    expect(
      boxQuad({ x: 100, y: 200 }, { x: 100 + MIN_BOX - 0.1, y: 400 }, PAGE),
    ).toBe(null);
  });

  it("refuses a box that is wide and flat", () => {
    // The other dimension. Apart from its twin above this is satisfied by a
    // bound applied to one side only, which is a tool that refuses a thin
    // column and accepts a flat sliver.
    expect(
      boxQuad({ x: 100, y: 200 }, { x: 400, y: 200 + MIN_BOX - 0.1 }, PAGE),
    ).toBe(null);
  });

  it("takes a box exactly on the minimum", () => {
    // The control for all three refusals: a rule that returned null for
    // everything satisfies them and nothing else here.
    expect(
      boxQuad({ x: 100, y: 200 }, { x: 100 + MIN_BOX, y: 200 + MIN_BOX }, PAGE),
    ).not.toBe(null);
  });

  it("measures the minimum after the clamp, not before it", () => {
    // A drag that starts off the page and ends just inside it. Measured before
    // the clamp it looks large and is accepted, and what reaches the file is a
    // sliver against the edge -- which is the refusal working on a number the
    // reader never drew.
    expect(boxQuad({ x: -200, y: -200 }, { x: 1, y: 1 }, PAGE)).toBe(null);
  });
});
