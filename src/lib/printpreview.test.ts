/**
 * The print preview's arithmetic, and the one number it shares with
 * `commands/print.rs`.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/commands/print.rs?raw";
import {
  PREVIEW_MAX_WIDTH,
  SHEET_GAP,
  fitSheet,
  nextToDraw,
  paperName,
  pictureWidth,
  previewSummary,
  visibleSheets,
} from "./printpreview";

const A4 = { width_pt: 595.276, height_pt: 841.89 };
const A4_LANDSCAPE = { width_pt: 841.89, height_pt: 595.276 };
const LETTER = { width_pt: 612, height_pt: 792 };
const A0 = { width_pt: 2384, height_pt: 3370 };

describe("the widest picture is commands/print.rs's", () => {
  it("has the same value", () => {
    expect(backend).toContain(`pub const PREVIEW_MAX_WIDTH: u32 = ${PREVIEW_MAX_WIDTH};`);
  });
});

describe("paperName", () => {
  it("names the sizes a reader knows, either way round", () => {
    expect(paperName(A4)).toBe("A4");
    expect(paperName(A4_LANDSCAPE)).toBe("A4 landscape");
    expect(paperName(LETTER)).toBe("US Letter");
    // A whole-point A4, as many producers write it.
    expect(paperName({ width_pt: 595, height_pt: 842 })).toBe("A4");
  });

  it("gives any other size in millimetres", () => {
    expect(paperName(A0)).toBe("841 x 1189 mm");
    expect(paperName({ width_pt: 200, height_pt: 100 })).toBe("71 x 35 mm");
  });
});

describe("previewSummary", () => {
  it("counts the sheets and names one paper", () => {
    expect(previewSummary([A4])).toBe("1 page, A4");
    expect(previewSummary([A4, A4, A4])).toBe("3 pages, all A4");
  });

  it("names up to three papers, and calls more than that mixed", () => {
    expect(previewSummary([A4, A4_LANDSCAPE])).toBe("2 pages: A4 and A4 landscape");
    expect(previewSummary([A4, LETTER, A0])).toBe("3 pages: A4, US Letter and 841 x 1189 mm");
    expect(previewSummary([A4, LETTER, A0, A4_LANDSCAPE])).toBe("4 pages in mixed sizes");
  });
});

describe("fitSheet", () => {
  it("fills the height with a portrait page and the width with a landscape one", () => {
    expect(fitSheet(A4, 600, 600)).toEqual({ width: 424, height: 600 });
    expect(fitSheet(A4_LANDSCAPE, 600, 600)).toEqual({ width: 600, height: 424 });
  });

  it("draws nothing for a page or a box with no area", () => {
    expect(fitSheet({ width_pt: 0, height_pt: 10 }, 600, 600)).toEqual({ width: 0, height: 0 });
    expect(fitSheet(A4, 0, 600)).toEqual({ width: 0, height: 0 });
  });
});

describe("pictureWidth", () => {
  it("asks for the screen's own pixels, within the backend's limit", () => {
    expect(pictureWidth(424, 1)).toBe(424);
    expect(pictureWidth(424, 2)).toBe(848);
    expect(pictureWidth(1200, 2)).toBe(PREVIEW_MAX_WIDTH);
    expect(pictureWidth(0, 2)).toBe(1);
    // A ratio the window could not read is taken as 1.
    expect(pictureWidth(300, 0)).toBe(300);
  });
});

describe("visibleSheets", () => {
  const heights = [100, 100, 100, 100];

  it("finds the sheets the view overlaps, gaps included", () => {
    expect(visibleSheets(heights, 0, 150)).toEqual([0, 1]);
    // The view starts inside the gap after sheet 0.
    expect(visibleSheets(heights, 100 + SHEET_GAP / 2, 50)).toEqual([1, 1]);
    expect(visibleSheets(heights, 3 * (100 + SHEET_GAP), 500)).toEqual([3, 3]);
  });

  it("finds none past the end or in a list with nothing in it", () => {
    expect(visibleSheets(heights, 10_000, 100)).toBeNull();
    expect(visibleSheets([], 0, 100)).toBeNull();
  });
});

describe("nextToDraw", () => {
  it("draws what is in view first, then the sheet after it", () => {
    expect(nextToDraw([1, 2], 5, new Set())).toBe(1);
    expect(nextToDraw([1, 2], 5, new Set([1]))).toBe(2);
    expect(nextToDraw([1, 2], 5, new Set([1, 2]))).toBe(3);
    expect(nextToDraw([1, 2], 5, new Set([1, 2, 3]))).toBeNull();
  });

  it("does not ask for a sheet past the last", () => {
    expect(nextToDraw([3, 4], 5, new Set([3, 4]))).toBeNull();
    expect(nextToDraw(null, 5, new Set())).toBeNull();
  });
});
