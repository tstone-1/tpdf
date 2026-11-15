import { describe, expect, it } from "vitest";
import { positionsFor, printedLabel, printedRuns, runsOf, withPrinted } from "./pagelabels";

/** The labels `outline-labels.pdf` states: roman front matter, a body, an appendix. */
const LABELS = ["i", "ii", "1", "2", "3", "4", "App-A", "App-B"];

/** Slot to label on an unedited document. */
const unedited = (labels: readonly string[]) => (slot: number) => printedLabel(labels, slot, slot + 1);

describe("printedLabel", () => {
  it("names a page whose printed number is not its position", () => {
    expect(printedLabel(LABELS, 0, 1)).toBe("i");
    expect(printedLabel(LABELS, 4, 5)).toBe("3");
    expect(printedLabel(LABELS, 7, 8)).toBe("App-B");
  });

  it("says nothing where the printed number is the position", () => {
    expect(printedLabel(["1", "2", "3"], 1, 2)).toBeNull();
  });

  it("says nothing for a document without labels, a page without one, or a page not of the file", () => {
    expect(printedLabel([], 0, 1)).toBeNull();
    expect(printedLabel(["i", "", "1"], 1, 2)).toBeNull();
    expect(printedLabel(LABELS, undefined, 3)).toBeNull();
  });

  it("follows the page of the file, not the position, after an edit", () => {
    // The first page deleted: the page printed "2" now sits at position 1.
    expect(printedLabel(["1", "2", "3"], 1, 1)).toBe("2");
  });
});

describe("runsOf", () => {
  it("groups consecutive slots", () => {
    expect(runsOf([0, 1, 2, 5, 7, 8])).toEqual([[0, 1, 2], [5], [7, 8]]);
    expect(runsOf([])).toEqual([]);
  });
});

describe("printedRuns", () => {
  it("names each run by the labels of its first and last page", () => {
    expect(printedRuns(runsOf([0, 1, 2, 5]), unedited(LABELS))).toBe("printed as i–1, 4");
  });

  it("falls back to the position for a page without a differing label", () => {
    const labels = ["i", "ii", "3", "4"];
    expect(printedRuns([[1, 2, 3]], unedited(labels))).toBe("printed as ii–4");
  });

  it("is empty when every page is printed with its position", () => {
    expect(printedRuns([[0, 1, 2]], unedited(["1", "2", "3"]))).toBe("");
    expect(printedRuns([[0, 1]], unedited([]))).toBe("");
  });

  it("spells out four runs and shortens the rest", () => {
    expect(printedRuns(runsOf([0, 2, 4, 6, 7]), unedited(LABELS))).toBe("printed as i, 1, 3, App-A–App-B");
    expect(printedRuns(runsOf([0, 2, 4, 6]).concat([[7]]), unedited(LABELS))).toBe("printed as i, 1, 3, App-A, …");
  });
});

describe("withPrinted", () => {
  it("adds the printed numbers to a preview only when there are any", () => {
    expect(withPrinted("Extract 3 pages", "")).toBe("Extract 3 pages");
    expect(withPrinted("Extract 3 pages", "printed as i–1")).toBe("Extract 3 pages · printed as i–1");
    expect(withPrinted("", "printed as i")).toBe("printed as i");
  });
});

describe("positionsFor", () => {
  const of = (raw: string, labels: readonly (string | null)[] = LABELS) =>
    positionsFor(raw, labels.length, (slot) => labels[slot] ?? null);

  it("names the positions of printed numbers, singly, in ranges and with a hyphen in a label", () => {
    expect(of("ii")).toBe("Printed as ii: type 2");
    expect(of("i-ii, App-A")).toBe("Printed as i-ii, App-A: type 1-2,7");
    expect(of("App-A-App-B")).toBe("Printed as App-A-App-B: type 7-8");
    expect(of("ii-4, 8")).toBe("Printed as ii-4, 8: type 2-4,8");
  });

  it("names nothing for a value with no label in it, or with a word that is none", () => {
    // Numbers alone are positions, and the parser decides about them.
    expect(of("1-3")).toBeNull();
    expect(of("ii, appendix")).toBeNull();
    expect(of("")).toBeNull();
  });

  it("names nothing for a label printed on two pages, or in a document without labels", () => {
    expect(of("ii", ["i", "ii", "ii"])).toBeNull();
    expect(of("ii", [null, null])).toBeNull();
  });
});
