/**
 * The print preview's arithmetic, and the one number it shares with
 * `commands/print.rs`.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/commands/print.rs?raw";
import layout from "../../src-tauri/src/print_layout.rs?raw";
import {
  type PageChoice,
  choosePages,
  readKept,
  turnedPicture,
  withKept,
  writeKept,
  MOST_PERCENT,
  PER_SHEET,
  PREVIEW_MAX_WIDTH,
  SHEET_GAP,
  defaultSettings,
  fitSheet,
  nextPicture,
  pageSheets,
  paperName,
  parsePages,
  pictureWidth,
  placementBox,
  previewSummary,
  printableBox,
  sheetSummary,
  suggestLandscape,
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

describe("nextPicture", () => {
  const sheets = pageSheets([A4, A4, A4, A4, A4]);
  const at = (width: number) => () => width;

  it("draws what is in view first, then the sheet after it", () => {
    expect(nextPicture(sheets, [1, 2], at(400), new Map())?.page).toBe(1);
    expect(nextPicture(sheets, [1, 2], at(400), new Map([[1, 400]]))?.page).toBe(2);
    expect(nextPicture(sheets, [1, 2], at(400), new Map([[1, 400], [2, 400]]))?.page).toBe(3);
    expect(nextPicture(sheets, [1, 2], at(400), new Map([[1, 400], [2, 400], [3, 400]]))).toBeNull();
  });

  it("does not ask for a sheet past the last", () => {
    expect(nextPicture(sheets, [3, 4], at(400), new Map([[3, 400], [4, 400]]))).toBeNull();
    expect(nextPicture(sheets, null, at(400), new Map())).toBeNull();
  });

  it("draws a page again when it is shown much larger, and not for a few pixels", () => {
    expect(nextPicture(sheets, [0, 0], at(400), new Map([[0, 300], [1, 999]]))).toEqual({
      sheet: 0,
      page: 0,
      width: 400,
    });
    expect(nextPicture(sheets, [0, 0], at(400), new Map([[0, 380], [1, 999]]))).toBeNull();
  });

  it("asks for each page sharing a sheet", () => {
    const shared = [
      {
        paper: pageSheets([A4])[0]!.paper,
        placements: [
          { page: 0, x: 0, y: 0, width: 100, height: 100, turned: false },
          { page: 1, x: 100, y: 0, width: 100, height: 100, turned: false },
        ],
      },
    ];
    expect(nextPicture(shared, [0, 0], at(200), new Map([[0, 200]]))?.page).toBe(1);
  });
});

describe("parsePages", () => {
  it("reads pages and ranges in the order typed, zero based", () => {
    expect(parsePages("1-3, 5", 6)).toEqual([0, 1, 2, 4]);
    expect(parsePages(" 4 ,2", 6)).toEqual([3, 1]);
    expect(parsePages("2 - 2", 6)).toEqual([1]);
  });

  it("takes nothing typed as every page", () => {
    expect(parsePages("", 6)).toBeNull();
    expect(parsePages(" , ", 6)).toBeNull();
  });

  it("refuses a page the job does not have, a backwards range and words, in the reader's terms", () => {
    expect(parsePages("7", 6)).toBe("This job has pages 1 to 6, not 7.");
    expect(parsePages("0", 1)).toBe("This job has 1 page, not 0.");
    expect(parsePages("4-2", 6)).toBe('"4-2" runs backwards.');
    expect(parsePages("all", 6)).toBe('"all" is not a page or a range of pages.');
  });
});

describe("suggestLandscape", () => {
  it("turns the sheet for two or six portrait pages, and follows the page otherwise", () => {
    expect(suggestLandscape(A4, 1)).toBe(false);
    expect(suggestLandscape(A4_LANDSCAPE, 1)).toBe(true);
    expect(suggestLandscape(A4, 2)).toBe(true);
    expect(suggestLandscape(A4_LANDSCAPE, 2)).toBe(false);
    expect(suggestLandscape(A4, 6)).toBe(true);
    expect(suggestLandscape(A4, 4)).toBe(false);
    expect(suggestLandscape(undefined, 1)).toBe(false);
  });
});

describe("defaultSettings", () => {
  it("opens on the default printer, one page to a sheet, nothing scaled up", () => {
    expect(defaultSettings([A4_LANDSCAPE, A4])).toEqual({
      printer: null,
      paper: null,
      landscape: true,
      copies: 1,
      twoSided: null,
      colour: null,
      perSheet: 1,
      scale: { kind: "shrink" },
      rotate: true,
      marks: true,
      pages: null,
    });
  });
});

describe("choosePages", () => {
  const choice = (over: Partial<PageChoice>): PageChoice => ({
    range: "all",
    text: "",
    current: 2,
    subset: "all",
    reverse: false,
    ...over,
  });

  it("answers every page in order as null, which the backend reads as all", () => {
    expect(choosePages(choice({}), 5)).toBeNull();
    expect(choosePages(choice({ range: "custom", text: "" }), 5)).toBeNull();
    expect(choosePages(choice({ range: "custom", text: "1-5" }), 5)).toBeNull();
  });

  it("prints the current page, and refuses when there is none", () => {
    expect(choosePages(choice({ range: "current" }), 5)).toEqual([2]);
    expect(choosePages(choice({ range: "current", current: -1 }), 5)).toBe("There is no current page in this job.");
  });

  it("takes the odd or even pages by the number a reader counts", () => {
    expect(choosePages(choice({ subset: "odd" }), 5)).toEqual([0, 2, 4]);
    expect(choosePages(choice({ subset: "even" }), 5)).toEqual([1, 3]);
    expect(choosePages(choice({ range: "custom", text: "2-4", subset: "odd" }), 5)).toEqual([2]);
    expect(choosePages(choice({ range: "current", subset: "even" }), 5)).toBe(
      "None of these pages has an even number.",
    );
  });

  it("prints last page first, after the subset", () => {
    expect(choosePages(choice({ reverse: true }), 3)).toEqual([2, 1, 0]);
    expect(choosePages(choice({ subset: "even", reverse: true }), 5)).toEqual([3, 1]);
  });

  it("passes on what parsePages refuses", () => {
    expect(choosePages(choice({ range: "custom", text: "9" }), 5)).toBe("This job has pages 1 to 5, not 9.");
  });
});

describe("the settings kept between prints", () => {
  const settings = {
    ...defaultSettings([A4]),
    printer: "Brother",
    paper: 9,
    twoSided: "long" as const,
    colour: false,
    perSheet: 2,
    scale: { kind: "poster" as const, percent: 200 },
    rotate: false,
    marks: false,
    copies: 3,
    pages: [0],
  };

  function memory(): { getItem(key: string): string | null; setItem(key: string, value: string): void } {
    const held = new Map<string, string>();
    return { getItem: (key) => held.get(key) ?? null, setItem: (key, value) => void held.set(key, value) };
  }

  it("keeps the printer and how it prints, and not the pages or the copies", () => {
    const store = memory();
    expect(writeKept(settings, () => store)).toBe(true);
    const kept = readKept(() => store);
    expect(kept).toEqual({
      printer: "Brother",
      paper: 9,
      twoSided: "long",
      colour: false,
      perSheet: 2,
      scale: { kind: "poster", percent: 200 },
      rotate: false,
      marks: false,
    });
    const next = withKept(defaultSettings([A4, A4]), kept, A4);
    expect(next.copies).toBe(1);
    expect(next.pages).toBeNull();
    // Two portrait pages to a sheet turn the sheet, as choosing 2 does.
    expect(next.landscape).toBe(true);
  });

  it("reads nothing from storage that refuses or holds anything else", () => {
    const refusing = () => {
      throw new Error("no storage");
    };
    expect(readKept(refusing)).toBeNull();
    expect(writeKept(settings, refusing)).toBe(false);
    const store = memory();
    store.setItem("tpdf.print-settings", "{not json");
    expect(readKept(() => store)).toBeNull();
    store.setItem("tpdf.print-settings", JSON.stringify({ ...settings, perSheet: 3 }));
    expect(readKept(() => store)).toBeNull();
    store.setItem("tpdf.print-settings", JSON.stringify({ ...settings, scale: { kind: "percent", percent: 0 } }));
    expect(readKept(() => store)).toBeNull();
    expect(withKept(defaultSettings([A4]), null, A4)).toEqual(defaultSettings([A4]));
  });

  it("keeps nothing until a printer is known", () => {
    const store = memory();
    expect(writeKept({ ...settings, printer: null }, () => store)).toBe(false);
    expect(readKept(() => store)).toBeNull();
  });
});

describe("turnedPicture", () => {
  it("sizes the upright picture so a quarter turn about its middle fills the placement", () => {
    // A landscape page turned onto a portrait sheet lands 100 wide, 200 tall;
    // its upright picture is 200 wide and 100 tall, centred on the same middle.
    expect(turnedPicture({ page: 0, x: 0, y: 0, width: 100, height: 200, turned: true })).toEqual({
      left: -50,
      top: 25,
      width: 200,
      height: 50,
    });
    expect(turnedPicture({ page: 0, x: 0, y: 0, width: 100, height: 200, turned: false })).toBeNull();
  });
});

describe("the boxes a sheet is drawn with", () => {
  const paper = {
    width: 600,
    height: 800,
    left: 30,
    top: 40,
    printableWidth: 540,
    printableHeight: 720,
  };

  it("puts the printable area where the printer can reach", () => {
    expect(printableBox(paper)).toEqual({ left: 5, top: 5, width: 90, height: 90 });
  });

  it("places a page in the printable area, past its edge when it overhangs", () => {
    expect(placementBox({ page: 0, x: 54, y: -72, width: 270, height: 864, turned: false }, paper)).toEqual({
      left: 10,
      top: -10,
      width: 50,
      height: 120,
    });
  });
});

describe("sheetSummary", () => {
  it("counts sheets of paper, and the pages when they differ", () => {
    expect(sheetSummary(1, 1, 1)).toBe("1 sheet of paper");
    expect(sheetSummary(3, 3, 1)).toBe("3 sheets of paper");
    expect(sheetSummary(2, 5, 1)).toBe("2 sheets of paper for 5 pages");
    expect(sheetSummary(2, 5, 3)).toBe("2 sheets of paper for 5 pages, 3 copies");
  });
});

describe("the choices are print_layout.rs's", () => {
  it("offers the same pages to a sheet and the same largest percentage", () => {
    expect(layout).toContain(`pub const PER_SHEET: [u8; ${PER_SHEET.length}] = [${PER_SHEET.join(", ")}];`);
    expect(layout).toContain(`pub const MOST_PERCENT: u16 = ${MOST_PERCENT};`);
  });
});
