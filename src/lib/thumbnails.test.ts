import { describe, expect, it } from "vitest";

import { nextWanted, rowHeightFor, stripWindow } from "./thumbnails";

/** A window of rows, as `stripWindow` reports one. */
function window(first: number, last: number): { first: number; last: number } {
  return { first, last };
}

/** A `have` predicate from a list of pages already rendered. */
function rendered(...pages: number[]): (page: number) => boolean {
  const set = new Set(pages);
  return (page) => set.has(page);
}

describe("stripWindow", () => {
  // 100 px rows, a 350 px panel, no overscan: rows 0..3 are on screen.
  it("covers every row the panel shows", () => {
    expect(stripWindow(0, 350, 100, 50, 0)).toEqual(window(0, 3));
  });

  it("includes a row that is only partly visible", () => {
    // Visible span [50, 430): row 4 spans 400..500, so 30 px of it is on
    // screen and dropping it would leave a visible strip of blank.
    expect(stripWindow(50, 380, 100, 50, 0)).toEqual(window(0, 4));
  });

  it("excludes a row whose top edge is exactly the bottom of the panel", () => {
    // The boundary the test above nearly asserted by accident. Span [50, 400):
    // row 4 begins at 400 and is not visible, so building it would be one more
    // Pdfium render call than the screen can show --- 1.5 s of one, on the A0
    // sheet.
    expect(stripWindow(50, 350, 100, 50, 0)).toEqual(window(0, 3));
  });

  it("adds the overscan on both sides", () => {
    expect(stripWindow(1000, 350, 100, 50, 3)).toEqual(window(7, 16));
  });

  it("does not run off either end of the document", () => {
    expect(stripWindow(0, 350, 100, 50, 3)).toEqual(window(0, 6));
    expect(stripWindow(4700, 350, 100, 50, 3)).toEqual(window(44, 49));
  });

  it("gives one row when the panel has not been laid out yet", () => {
    // Height is zero until the layout reaches the panel. An empty window here
    // would mean nothing is ever built, and nothing would then resize it.
    expect(stripWindow(0, 0, 100, 50, 0)).toEqual(window(0, 0));
  });

  it("gives nothing for a document with no pages", () => {
    expect(stripWindow(0, 350, 100, 0, 3)).toEqual(window(0, -1));
  });

  it("gives nothing rather than dividing by a zero row height", () => {
    expect(stripWindow(0, 350, 0, 50, 3)).toEqual(window(0, -1));
  });
});

describe("nextWanted", () => {
  it("names the page at the centre when it has no thumbnail", () => {
    expect(nextWanted(window(0, 9), 4, rendered())).toBe(4);
  });

  it("works outwards from the centre, not from the top", () => {
    // The whole point of the ordering. On the A0 sheet a thumbnail costs 1.5 s,
    // so a strip that started at page 0 for a reader on page 400 would render
    // hundreds of pictures nobody asked for before reaching the one they can
    // see. Written as `for (page = first; ...)` this test is what fails.
    expect(nextWanted(window(0, 9), 5, rendered(5))).toBe(4);
    expect(nextWanted(window(0, 9), 5, rendered(4, 5))).toBe(6);
    expect(nextWanted(window(0, 9), 5, rendered(4, 5, 6))).toBe(3);
  });

  it("prefers the row above when both are equally far", () => {
    // Not arbitrary: rows above the centre are the ones a reader scrolling
    // down has just passed and is most likely to scroll back to. Asserted so
    // the tie is a decision rather than an accident of loop order.
    expect(nextWanted(window(0, 9), 5, rendered(5))).toBe(4);
  });

  it("stays inside the window even when the reader is outside it", () => {
    // The strip can be scrolled away from the page being read. What is worth
    // rendering is what is on screen; the centre only orders it.
    expect(nextWanted(window(20, 25), 0, rendered())).toBe(20);
    expect(nextWanted(window(20, 25), 99, rendered())).toBe(25);
  });

  it("names nothing when every row in the window is drawn", () => {
    expect(nextWanted(window(2, 5), 3, rendered(2, 3, 4, 5))).toBeNull();
  });

  it("reaches the last row of the window", () => {
    // The loop bound is the width of the window, and an off-by-one there leaves
    // one row permanently blank at whichever end is furthest from the centre.
    expect(nextWanted(window(0, 9), 0, rendered(0, 1, 2, 3, 4, 5, 6, 7, 8))).toBe(9);
    expect(nextWanted(window(0, 9), 9, rendered(1, 2, 3, 4, 5, 6, 7, 8, 9))).toBe(0);
  });

  it("names nothing for an empty window", () => {
    expect(nextWanted(window(0, -1), 0, rendered())).toBeNull();
  });
});

describe("rowHeightFor", () => {
  it("keeps the page's aspect ratio", () => {
    const portrait = rowHeightFor({ width_pt: 612, height_pt: 792 });
    const landscape = rowHeightFor({ width_pt: 792, height_pt: 612 });
    expect(portrait).toBeGreaterThan(landscape);
  });

  it("leaves room for the page number under the picture", () => {
    // A row exactly as tall as its thumbnail would overlap the number with the
    // next page's picture.
    const page = { width_pt: 100, height_pt: 100 };
    expect(rowHeightFor(page)).toBeGreaterThan(116);
  });

  it("does not divide by a zero page width", () => {
    expect(Number.isFinite(rowHeightFor({ width_pt: 0, height_pt: 792 }))).toBe(true);
  });

  it("measures the page as the view shows it, not as the file has it", () => {
    // A portrait page rotated a quarter turn is a landscape row, and a strip
    // that sized its rows from the file would leave a gap under every one of
    // them --- while the borrowed bitmap, which *is* rotated, overflowed.
    const portrait = { width_pt: 612, height_pt: 792 };
    const landscape = { width_pt: 792, height_pt: 612 };
    expect(rowHeightFor(portrait, 1)).toBe(rowHeightFor(landscape, 0));
    expect(rowHeightFor(portrait, 3)).toBe(rowHeightFor(landscape, 0));
  });

  it("is unchanged by a half turn", () => {
    // The control for the test above: a defect that swapped the dimensions on
    // *every* non-zero rotation would pass it, and fails here.
    const portrait = { width_pt: 612, height_pt: 792 };
    expect(rowHeightFor(portrait, 2)).toBe(rowHeightFor(portrait, 0));
    expect(rowHeightFor(portrait, 4)).toBe(rowHeightFor(portrait, 0));
  });
});
