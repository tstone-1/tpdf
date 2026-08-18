/**
 * Placing things inside a crop, and taking them back out.
 *
 * Two pure functions and one property, and the property is the point: a mark is
 * **stored** in the file's display space and **drawn** in the cropped one, so
 * whatever moves a rectangle in must move it back out exactly. A pair that
 * disagreed by a point would move every highlight a little every time the reader
 * cropped, which is a drift no single screenshot catches.
 *
 * Every test below was checked by mutating `crop.ts` and confirming it went red;
 * `scripts/mutate_frontend.py` holds the mutations.
 */

import { describe, expect, it } from "vitest";

import { intoCrop, outOfCrop, uncropped, type CropGeometry } from "./crop";

/** A crop 30 points in from the left of the file's page and 40 from its top. */
const at: CropGeometry = { width_pt: 400, height_pt: 500, left: 30, top: 40 };

describe("intoCrop", () => {
  it("moves a rectangle by the crop's corner, both edges of each axis", () => {
    // Both edges, because a version that moved only the origin would leave every
    // rectangle the wrong size rather than in the wrong place -- and a highlight
    // that grows as the page is cropped reads as a rendering bug.
    expect(intoCrop([100, 200, 150, 220], at)).toEqual([70, 160, 120, 180]);
  });

  it("moves a rectangle nowhere on a page nobody cropped", () => {
    // The control. Without it, a version that subtracted a constant would pass
    // the test above and shift every rectangle on every uncropped document.
    //
    // Named apart from its twin below, which asserts the same property of
    // `outOfCrop`: `mutate_frontend.py` reads failing test names as a **set**,
    // so two tests sharing one name make its count disagree with the suite's
    // and the run is refused as unreadable rather than reported as a survivor.
    const none = uncropped(595, 842);
    expect(intoCrop([100, 200, 150, 220], none)).toEqual([100, 200, 150, 220]);
  });

  it("takes a rectangle above the crop to a negative coordinate", () => {
    // A comment in the margin the reader has just cropped away. Negative rather
    // than clamped: it is genuinely off the visible page, and clamping would
    // stack every such note along the top edge as though they were on it.
    expect(intoCrop([0, 0, 10, 10], at)).toEqual([-30, -40, -20, -30]);
  });
});

describe("outOfCrop", () => {
  it("is the inverse of intoCrop, quad by quad", () => {
    // The property the model depends on. `outOfCrop` works over the flat
    // four-per-rectangle array a mark carries, so this asserts across a pair of
    // quads rather than one -- an implementation that used the first rectangle's
    // offset for all of them passes on a single-line highlight.
    const quads = [70, 160, 120, 180, 70, 190, 200, 210];
    const back = outOfCrop(quads, at);
    expect(back).toEqual([100, 200, 150, 220, 100, 230, 230, 250]);
    expect(intoCrop([back[0], back[1], back[2], back[3]] as [
      number,
      number,
      number,
      number,
    ], at)).toEqual([70, 160, 120, 180]);
  });

  it("alternates the two offsets rather than applying one of them", () => {
    // x, y, x, y and not x, x, y, y: a quad is `[left, top, right, bottom]`, so
    // the offsets alternate by index parity. Given a crop whose two offsets
    // differ -- which is why `at` is 30 and 40 and not 30 and 30 -- an
    // implementation that used `left` for all four is off by ten on two of them.
    expect(outOfCrop([0, 0, 0, 0], at)).toEqual([30, 40, 30, 40]);
  });

  it("moves quads nowhere on a page nobody cropped", () => {
    expect(outOfCrop([1, 2, 3, 4], uncropped(595, 842))).toEqual([1, 2, 3, 4]);
  });
});

describe("uncropped", () => {
  it("is the page itself, at the origin", () => {
    expect(uncropped(595, 842)).toEqual({
      width_pt: 595,
      height_pt: 842,
      left: 0,
      top: 0,
    });
  });
});
