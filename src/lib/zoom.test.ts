/**
 * The zoom arithmetic.
 *
 * Every function here has an answer that can be wrong rather than merely ugly,
 * which is the bar this repository sets for adding a test at all. The two that
 * carry the feature are {@link fitZoom} --- fit-page is the *smaller* of two
 * fits, and taking the wrong one cuts the page off at the side on every wide
 * window --- and {@link parseZoomPercent}, which stands between a text field and
 * the renderer's scale.
 *
 * Every test below was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import {
  clampZoom,
  describeFit,
  FIT_MARGIN,
  fitZoom,
  MAX_ZOOM,
  MIN_ZOOM,
  nextStop,
  parseZoomPercent,
  percentOf,
  ZOOM_STEPS,
} from "./zoom";

/** A4 in points, which is what most of these are fitted against. */
const A4 = { width_pt: 595, height_pt: 842 };

describe("clampZoom", () => {
  it("holds the range the session file is sanitized against", () => {
    expect(clampZoom(0.001)).toBe(MIN_ZOOM);
    expect(clampZoom(1000)).toBe(MAX_ZOOM);
    expect(clampZoom(1.5)).toBe(1.5);
  });

  it("turns a zoom that is not a number into the smallest one", () => {
    // NaN survives both `Math.max` and `Math.min` unchanged, so a clamp written
    // as the obvious pair of calls passes it straight through to the scroller,
    // which then lays out a page of NaN pixels and draws nothing at all. The
    // viewport really is 1px wide during construction, so this arrives.
    expect(clampZoom(NaN)).toBe(MIN_ZOOM);
    expect(clampZoom(Infinity)).toBe(MIN_ZOOM);
  });
});

describe("fitZoom", () => {
  it("leaves a margin either side when fitting the width", () => {
    // Exact, not approximate: the margin is the whole difference between this
    // and dividing by the viewport, and a fit that quietly dropped it would
    // still look like a fit.
    expect(fitZoom("width", { width: 1000, height: 400 }, A4)).toBe(
      (1000 - FIT_MARGIN * 2) / A4.width_pt,
    );
  });

  it("ignores the viewport height when fitting the width", () => {
    // The distinguishing property of fit-width: a page taller than the window
    // is *meant* to run off the bottom and be scrolled.
    const wide = fitZoom("width", { width: 1000, height: 400 }, A4);
    expect(fitZoom("width", { width: 1000, height: 4000 }, A4)).toBe(wide);
  });

  it("fits a page by its height when the window is wide", () => {
    // A landscape window on a portrait page: the height binds, and no margin is
    // taken off it, so the page is exactly as tall as the viewport.
    const zoom = fitZoom("page", { width: 3000, height: 900 }, A4);
    expect(zoom).toBe(900 / A4.height_pt);
  });

  it("fits a page by its width when the window is tall", () => {
    // The other side of the same `min`. Fitting only the height here would
    // magnify the page until it ran off both edges, which is the failure this
    // whole function exists to not have.
    const zoom = fitZoom("page", { width: 600, height: 4000 }, A4);
    expect(zoom).toBe((600 - FIT_MARGIN * 2) / A4.width_pt);
  });

  it("never magnifies a page past either edge of the viewport", () => {
    // The property, stated over both orientations rather than over the two
    // cases above: whichever bound is tighter, the drawn page fits inside the
    // viewport on both axes at once.
    for (const page of [A4, { width_pt: 842, height_pt: 595 }]) {
      for (const viewport of [
        { width: 1200, height: 800 },
        { width: 400, height: 1600 },
        { width: 900, height: 900 },
      ]) {
        const zoom = fitZoom("page", viewport, page);
        // A nanometre of slack, because `595 * (800 / 595)` is 800.0000000000001
        // and the property is about the page fitting, not about float
        // associativity. It cannot hide the failure being tested for: taking
        // the larger fit instead of the smaller overshoots by hundreds of
        // pixels, not by one part in 10^13.
        expect(page.width_pt * zoom).toBeLessThanOrEqual(viewport.width + 1e-9);
        expect(page.height_pt * zoom).toBeLessThanOrEqual(viewport.height + 1e-9);
      }
    }
  });

  it("clamps a fit computed against a viewport that has no layout yet", () => {
    // The constructor computes a fit before the element has been laid out, so
    // the viewport is one pixel wide. Without the clamp that is a negative zoom
    // --- the margin is wider than the window --- and a negative scale is not a
    // small page, it is a mirrored one.
    expect(fitZoom("width", { width: 1, height: 1 }, A4)).toBe(MIN_ZOOM);
    expect(fitZoom("page", { width: 1, height: 1 }, A4)).toBe(MIN_ZOOM);
  });
});

describe("nextStop", () => {
  it("steps to the neighbouring stop in each direction", () => {
    expect(nextStop(1, 1)).toBe(1.25);
    expect(nextStop(1, -1)).toBe(0.8);
  });

  it("does not find the stop it is standing on", () => {
    // Floating point: a fitted zoom that lands within noise of a stop would
    // otherwise step to itself, and a keypress that does nothing reads as a
    // broken binding rather than as a rounding error.
    for (const stop of ZOOM_STEPS) {
      expect(nextStop(stop, 1)).not.toBe(stop);
      expect(nextStop(stop, -1)).not.toBe(stop);
    }
  });

  it("starts from a fitted zoom that is not a stop at all", () => {
    // The usual case, since a fit-width zoom is whatever the window is wide.
    expect(nextStop(0.9, 1)).toBe(1);
    expect(nextStop(0.9, -1)).toBe(0.8);
  });

  it("says there is no next stop rather than returning the last one again", () => {
    // The caller pins the fit mode off when it steps. Returning the end stop
    // again would turn fitting off on a keypress that changed nothing, so the
    // next resize would stop moving the zoom for no visible reason.
    expect(nextStop(ZOOM_STEPS[ZOOM_STEPS.length - 1] ?? 8, 1)).toBeNull();
    expect(nextStop(ZOOM_STEPS[0] ?? 0.25, -1)).toBeNull();
  });
});

describe("parseZoomPercent", () => {
  it("reads a percentage with or without the sign", () => {
    expect(parseZoomPercent("150")).toBe(1.5);
    expect(parseZoomPercent("150%")).toBe(1.5);
    expect(parseZoomPercent("  150 %  ")).toBe(1.5);
    expect(parseZoomPercent("12.5")).toBe(0.125);
  });

  it("refuses what `Number` would have accepted", () => {
    // The reason for the regular expression rather than `Number(raw)`, which
    // reads all four of these as numbers --- and the empty string as zero.
    expect(parseZoomPercent("1e3")).toBeNull();
    expect(parseZoomPercent("0x64")).toBeNull();
    expect(parseZoomPercent("Infinity")).toBeNull();
    expect(parseZoomPercent("")).toBeNull();
    expect(parseZoomPercent("   ")).toBeNull();
  });

  it("refuses text, signs and a bare percent", () => {
    expect(parseZoomPercent("wide")).toBeNull();
    expect(parseZoomPercent("-50")).toBeNull();
    expect(parseZoomPercent("+50")).toBeNull();
    expect(parseZoomPercent("%")).toBeNull();
    expect(parseZoomPercent("15 0")).toBeNull();
  });

  it("refuses a zoom outside the range rather than clamping it", () => {
    // Clamping would answer a mistyped 5000 with a perfectly good 1600%, and
    // the reader would never learn that the document cannot do what they asked.
    expect(parseZoomPercent("4")).toBeNull();
    expect(parseZoomPercent("1601")).toBeNull();
    // The ends themselves are inside.
    expect(parseZoomPercent("5")).toBe(MIN_ZOOM);
    expect(parseZoomPercent("1600")).toBe(MAX_ZOOM);
  });
});

describe("describeFit", () => {
  it("gives each mode its own words", () => {
    const said = ["none", "width", "page"].map((mode) =>
      describeFit(mode as "none" | "width" | "page"),
    );
    expect(new Set(said).size).toBe(3);
    expect(describeFit("page")).toBe("Fit page");
  });
});

describe("percentOf", () => {
  it("rounds to whole percent", () => {
    expect(percentOf(1)).toBe(100);
    // Not 1.005, which is 100.49999999999999 as a double and rounds *down* ---
    // an expectation of 101 there tests the float literal, not the rounding.
    expect(percentOf(1.006)).toBe(101);
    expect(percentOf(0.05)).toBe(5);
  });
});
