import { describe, expect, it } from "vitest";

import type { MarkKind } from "./pages";

import {
  boxQuad,
  ERASER_RADIUS,
  ICON_SIZE,
  LINE_FRACTION,
  MIN_BOX,
  iconQuad,
  isEllipse,
  isIcon,
  isOutline,
  isText,
  isWave,
  isWash,
  markBand,
  paintOf,
  quadSwept,
  strokeSwept,
  strokeTouches,
  sweepLabel,
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
    // **`ellipse` is the one that matters in this list**, and the reason it is
    // named rather than left to the loop's original four: it is the only kind
    // that could plausibly answer `true` here, being the box's sibling and
    // stroked exactly like it. The overlay branches on this before it reaches
    // {@link isEllipse}, so an outline that claimed the ellipse would draw a
    // rectangle over every ring a reader made -- and every other assertion
    // about an ellipse would still pass.
    for (const kind of [
      "highlight",
      "underline",
      "strikeout",
      "note",
      "ellipse",
      "ink",
    ] as const) {
      expect(isOutline(kind), kind).toBe(false);
    }
  });

  it("says an ellipse is drawn as one and the others are not", () => {
    // The box's control, in the other direction. Both halves are needed for the
    // reason above: the overlay asks these two in order, so a predicate that
    // was always true here would swallow the box.
    expect(isEllipse("ellipse")).toBe(true);
    for (const kind of [
      "highlight",
      "underline",
      "strikeout",
      "note",
      "square",
      "ink",
    ] as const) {
      expect(isEllipse(kind), kind).toBe(false);
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

  it("says a squiggle is drawn as a wave and the others are not", () => {
    // The predicate that stops a squiggle being painted as a filled band. Its
    // near-twin is the underline, which is the one that matters in this list:
    // both sit at the bottom of the quad, and a `true` here for an underline
    // would draw a wave where a reader asked for a rule.
    expect(isWave("squiggly")).toBe(true);
    for (const kind of [
      "highlight",
      "underline",
      "strikeout",
      "note",
      "square",
      "ellipse",
      "ink",
    ] as const) {
      expect(isWave(kind), kind).toBe(false);
    }
  });

  it("says a text box is drawn as words and the others are not", () => {
    // Without this the overlay falls through to `fillRect` and paints a solid
    // block over the words, while the file drawn by `Paint::Text` has them.
    expect(isText("textbox")).toBe(true);
    for (const kind of [
      "highlight",
      "underline",
      "strikeout",
      "squiggly",
      "note",
      "square",
      "ellipse",
      "ink",
    ] as const) {
      expect(isText(kind), kind).toBe(false);
    }
  });

  it("gives a squiggle a band taller than an underline's rule", () => {
    // **The property every check that tells the two kinds apart rests on.**
    // Asserted as a comparison rather than against a number, so the two
    // constants cannot drift into agreement without this failing -- at which
    // point a squiggle and an underline would be the same mark drawn twice, and
    // nothing else in the frontend would say so.
    const quad: Quad = { left: 100, top: 200, right: 340, bottom: 260 };
    const rule = markBand("underline", quad);
    const wave = markBand("squiggly", quad);

    expect(wave.bottom - wave.top).toBeGreaterThan(
      (rule.bottom - rule.top) * 2,
    );
    // Both sit on the same edge, which is what makes the difference a strip
    // above the rule rather than two bands in unrelated places.
    expect(wave.bottom).toBe(rule.bottom);
    expect(wave.bottom).toBe(quad.bottom);
  });

  it("gives an ellipse the whole quad, which is what it is inscribed in", () => {
    // The rectangle, not the curve. Everything downstream of this wants the
    // bounding box -- the popup anchor, `/Rect`, the hit test -- and the curve
    // exists only where something paints. A band inside the quad here would
    // shrink the popup's anchor and the mark's own rectangle to the middle
    // half of what the reader dragged.
    const box: Quad = { left: 100, top: 200, right: 340, bottom: 260 };
    expect(markBand("ellipse", box)).toEqual(box);
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

describe("what the eraser's nib touches", () => {
  /** A stroke across the page with one corner in it. */
  const BENT = [
    { x: 100, y: 100 },
    { x: 300, y: 100 },
    { x: 300, y: 260 },
  ];

  it("takes a stroke the nib is on", () => {
    expect(strokeTouches(BENT, { x: 200, y: 100 }, ERASER_RADIUS)).toBe(true);
  });

  it("leaves one the nib misses", () => {
    // The control. Without it, a predicate that answered `true` for everything
    // would pass every other assertion in this block.
    expect(strokeTouches(BENT, { x: 200, y: 180 }, ERASER_RADIUS)).toBe(false);
  });

  it("measures to the nearest segment, not to the nearest recorded point", () => {
    // The whole reason this is not a point distance. A fast hand leaves points
    // 200 pt apart; the midpoint of that segment is nowhere near either of
    // them, and a nearest-point test lets the eraser pass straight through the
    // middle of a long stroke. Measured: 100 pt from each end, 0 from the line.
    const long = [
      { x: 100, y: 100 },
      { x: 300, y: 100 },
    ];
    expect(strokeTouches(long, { x: 200, y: 101 }, 2)).toBe(true);
    // And the same point against the two endpoints alone, which is what a
    // nearest-point implementation would be comparing.
    expect(Math.hypot(200 - 100, 101 - 100)).toBeGreaterThan(ERASER_RADIUS);
  });

  it("does not reach along the line the segment sits on", () => {
    // The clamp. Unclamped, the distance from a point beyond the end of a
    // segment is measured to the infinite line through it -- which here is
    // zero, so the eraser would take a stroke it passed a hundred points clear
    // of, in the stroke's own direction.
    expect(strokeTouches(BENT, { x: 500, y: 100 }, ERASER_RADIUS)).toBe(false);
    expect(strokeTouches(BENT, { x: 302, y: 100 }, ERASER_RADIUS)).toBe(true);
  });

  it("reaches the corner between two segments", () => {
    expect(strokeTouches(BENT, { x: 301, y: 101 }, ERASER_RADIUS)).toBe(true);
  });

  it("answers for a stroke of one point rather than refusing it", () => {
    // The model will not keep such a stroke, so nothing should ever ask -- and
    // a geometry helper that is wrong on a degenerate input is one somebody
    // calls from somewhere else later. Both directions, so "always false" does
    // not pass.
    expect(strokeTouches([{ x: 50, y: 50 }], { x: 52, y: 50 }, ERASER_RADIUS)).toBe(true);
    expect(strokeTouches([{ x: 50, y: 50 }], { x: 90, y: 50 }, ERASER_RADIUS)).toBe(false);
  });

  it("takes a stroke it crosses in the middle, with every end far away", () => {
    // The case no endpoint distance can see: an X of two long segments is at
    // distance zero and all four ends are a hundred points apart. Without the
    // crossing test the nib goes straight through the stroke it is aimed at.
    const across = [
      { x: 100, y: 300 },
      { x: 500, y: 300 },
    ];
    expect(strokeSwept(across, { x: 300, y: 100 }, { x: 300, y: 500 }, 1)).toBe(true);
    // The control, and it is the same two segments moved apart rather than a
    // different fixture: parallel, so they never cross and no end is near.
    expect(strokeSwept(across, { x: 300, y: 100 }, { x: 300, y: 280 }, 1)).toBe(false);
  });

  it("is empty-handed about an empty stroke", () => {
    expect(strokeTouches([], { x: 0, y: 0 }, ERASER_RADIUS)).toBe(false);
  });

  it("uses the radius it is given", () => {
    // The radius is a parameter because the viewer divides it by the zoom. A
    // constant baked in here would make that division a no-op and nothing would
    // say so.
    const off = { x: 200, y: 120 };
    expect(strokeTouches(BENT, off, 10)).toBe(false);
    expect(strokeTouches(BENT, off, 25)).toBe(true);
  });
});

describe("what the eraser's nib touches that is not a drawing", () => {
  /** A mark's rectangle, 200 by 100, well away from the origin. */
  const RECT: Quad = { left: 100, top: 200, right: 300, bottom: 300 };

  /** A press: a sweep of no length, which is what `begin` produces. */
  function on(x: number, y: number): boolean {
    return quadSwept(RECT, { x, y }, { x, y }, ERASER_RADIUS);
  }

  it("takes one the nib is pressed inside", () => {
    expect(on(200, 250)).toBe(true);
  });

  it("leaves one the nib is pressed clear of", () => {
    // The control. Without it a predicate answering `true` for everything would
    // satisfy every other assertion here.
    expect(on(200, 500)).toBe(false);
  });

  it("counts the edge itself, and the nib's width around it", () => {
    // Inside, on the line, and just outside it: the last is what says the
    // radius is read at all, and it is measured in the caller's own units.
    expect(on(100, 250)).toBe(true);
    expect(on(100 - ERASER_RADIUS + 1, 250)).toBe(true);
    expect(on(100 - ERASER_RADIUS - 1, 250)).toBe(false);
  });

  it("does not reach the corner diagonally further than the nib", () => {
    // A point off the corner at (100, 200) by the radius in *both* directions
    // is further away than the radius, because distance is not the larger of
    // the two offsets. A rule written as two independent inequalities -- the
    // obvious way to inflate a rectangle -- would take this one.
    const off = ERASER_RADIUS - 1;
    expect(on(100 - off, 200 - off)).toBe(false);
    expect(Math.hypot(off, off)).toBeGreaterThan(ERASER_RADIUS);
  });

  it("takes one the nib crosses without stopping inside it", () => {
    // Right through and out the other side, with neither end contained. This
    // is the case the containment test cannot answer and the boundary can.
    expect(
      quadSwept(RECT, { x: 0, y: 250 }, { x: 600, y: 250 }, ERASER_RADIUS),
    ).toBe(true);
  });

  it("takes one the nib travels into", () => {
    // `from` outside and `to` inside, which is why `to` needs no containment
    // test of its own: the segment crossed an edge to get there.
    expect(
      quadSwept(RECT, { x: 0, y: 250 }, { x: 200, y: 250 }, ERASER_RADIUS),
    ).toBe(true);
  });

  it("leaves one a long sweep passes clear of", () => {
    // The travel's own control: a segment as long as the one above, missing.
    expect(
      quadSwept(RECT, { x: 0, y: 500 }, { x: 600, y: 500 }, ERASER_RADIUS),
    ).toBe(false);
  });

  it("answers for a rectangle of no size rather than refusing it", () => {
    // A geometry helper wrong on a degenerate case is one somebody will call
    // from somewhere else. Nothing in the application sends one -- a mark's
    // quad has area -- so this is about the helper, not about the eraser.
    const point: Quad = { left: 100, top: 200, right: 100, bottom: 200 };
    expect(quadSwept(point, { x: 102, y: 200 }, { x: 102, y: 200 }, ERASER_RADIUS)).toBe(
      true,
    );
    expect(quadSwept(point, { x: 130, y: 200 }, { x: 130, y: 200 }, ERASER_RADIUS)).toBe(
      false,
    );
  });
});

describe("what the status line says while the eraser is armed", () => {
  it("names the gesture when nothing has been taken", () => {
    // "a mark", not "a drawing": the nib takes every kind, and a reader told to
    // drag across a drawing would not try it on the highlight they want gone.
    expect(sweepLabel({ strokes: 0, marks: 0 })).toBe("Erasing — drag across a mark");
  });

  it("counts one of each thing in the singular", () => {
    expect(sweepLabel({ strokes: 1, marks: 0 })).toBe("Erasing: 1 stroke");
    expect(sweepLabel({ strokes: 0, marks: 1 })).toBe("Erasing: 1 mark");
  });

  it("counts several in the plural", () => {
    expect(sweepLabel({ strokes: 4, marks: 0 })).toBe("Erasing: 4 strokes");
    expect(sweepLabel({ strokes: 0, marks: 3 })).toBe("Erasing: 3 marks");
  });

  it("says both when a sweep took both, and neither when it took neither", () => {
    // The reason there are two numbers: "3 strokes" would be a lie about the
    // highlight that went with them.
    expect(sweepLabel({ strokes: 3, marks: 2 })).toBe("Erasing: 3 strokes, 2 marks");
  });

  it("leaves out the half that is zero", () => {
    // Not "1 stroke, 0 marks". The common case is one kind of thing, and a
    // clause reporting nothing is a clause a reader has to read past.
    expect(sweepLabel({ strokes: 1, marks: 0 })).not.toContain("mark");
    expect(sweepLabel({ strokes: 0, marks: 1 })).not.toContain("stroke");
  });
});

describe("how the overlay decides to draw a kind", () => {
  it("answers for every kind there is", () => {
    // The population, written out. A `MarkKind[]` the test builds from the
    // classifier itself would be the check reading its own answer back ---
    // `docs/TRAPS.md` records that shape --- so the list is here and a tenth
    // kind fails to type-check against `MarkKind` rather than being skipped.
    const kinds: MarkKind[] = [
      "highlight",
      "underline",
      "strikeout",
      "squiggly",
      "note",
      "square",
      "ellipse",
      "textbox",
      "ink",
      "stamp",
    ];
    for (const kind of kinds) {
      expect(paintOf(kind), kind).toBeTruthy();
    }
  });

  it("agrees with save.rs arm for arm, which is where the pair can drift", () => {
    // **`paint()` in `save.rs`, transcribed.** The overlay and the writer draw
    // every mark and this is the decision they must agree on; a differential
    // between them is what found a defect neither side's own tests could. The
    // one deliberate difference is the highlight: `Paint::Wash` there against
    // "fill" here, because the multiply is set per mark on the context rather
    // than being a property of the shape.
    expect(paintOf("highlight")).toBe("fill");
    expect(paintOf("underline")).toBe("line");
    expect(paintOf("strikeout")).toBe("line");
    expect(paintOf("squiggly")).toBe("wave");
    expect(paintOf("square")).toBe("outline");
    expect(paintOf("ellipse")).toBe("ellipse");
    expect(paintOf("textbox")).toBe("text");
    expect(paintOf("ink")).toBe("path");
    expect(paintOf("note")).toBe("icon");
    expect(paintOf("stamp")).toBe("stamp");
  });

  it("gives the two kinds that used to fall through a style of their own", () => {
    // The painter's chain ended in a bare `else` that filled the quad, so any
    // kind without a branch was drawn as a filled rectangle rather than not at
    // all. Only a highlight may reach that arm now, and these two are the ones
    // whose wrong answer would have looked most like a real mark.
    expect(paintOf("stamp")).not.toBe("fill");
    expect(paintOf("note")).not.toBe("fill");
  });
});
