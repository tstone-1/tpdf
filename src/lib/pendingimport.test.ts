import { describe, expect, it } from "vitest";
import {
  chosenPages,
  PendingImports,
  rangePlaceholder,
  rangePreview,
  type PreparedImport,
} from "./pendingimport";

const report: PreparedImport = { pending: 4, pages: 8, name: "report.pdf" };

/** A waiting slot, and every release it asked for, as `doc:pending`. */
function slot() {
  const released: string[] = [];
  const pending = new PendingImports((doc, id) => released.push(`${doc}:${id}`));
  return { pending, released };
}

describe("the import waiting for its pages", () => {
  it("is offered only for the document it was prepared for", () => {
    const { pending, released } = slot();
    expect(pending.current(9)).toBeNull();
    pending.hold(9, report);
    expect(pending.current(9)).toEqual({ doc: 9, ...report });
    expect(pending.current(3)).toBeNull();
    expect(pending.current(null)).toBeNull();
    expect(released).toEqual([]);
  });

  it("releases the file a second prepare replaces", () => {
    const { pending, released } = slot();
    pending.hold(9, report);
    pending.hold(3, { ...report, pending: 5 });
    expect(released).toEqual(["9:4"]);
    expect(pending.current(3)?.pending).toBe(5);
    expect(pending.current(9)).toBeNull();
  });

  it("hands the import over to be committed, and does not release it", () => {
    const { pending, released } = slot();
    pending.hold(9, report);
    expect(pending.take(9)).toEqual({ doc: 9, ...report });
    expect(released).toEqual([]);
    expect(pending.current(9)).toBeNull();
    expect(pending.take(9)).toBeNull();
  });

  it("releases rather than hands over an import prepared for another document", () => {
    const { pending, released } = slot();
    pending.hold(9, report);
    expect(pending.take(3)).toBeNull();
    expect(released).toEqual(["9:4"]);
    expect(pending.current(9)).toBeNull();
  });

  it("releases the file once when the question is dropped", () => {
    const { pending, released } = slot();
    pending.drop();
    expect(released).toEqual([]);
    pending.hold(9, report);
    pending.drop();
    pending.drop();
    expect(released).toEqual(["9:4"]);
    expect(pending.current(9)).toBeNull();
  });
});

describe("the pages a reader names of the other file", () => {
  it("reads a blank answer as every page, in the file's order", () => {
    expect(chosenPages("", report)).toEqual({ slots: [0, 1, 2, 3, 4, 5, 6, 7] });
    expect(chosenPages("   ", report).slots).toHaveLength(8);
  });

  it("reads a range against the other file's count, zero-based", () => {
    expect(chosenPages("2-4, 8", report)).toEqual({ slots: [1, 2, 3, 7] });
    expect(chosenPages("8", { ...report, pages: 7 }).problem).toBeDefined();
  });

  it("names the file, not the document, when a page is past its end", () => {
    const refused = chosenPages("9", report);
    expect(refused.slots).toBeUndefined();
    expect(refused.problem).toBe("report.pdf has 8 pages");
  });

  it("keeps every other refusal as the range parser words it", () => {
    expect(chosenPages("5-3", report).problem).toBe("5-3 runs backwards");
    expect(chosenPages("2,", report).problem).toContain("empty part");
  });

  it("states the file and its count before anything is typed", () => {
    expect(rangePlaceholder(report)).toBe("Pages of report.pdf (1-8); blank for all");
  });

  it("previews every page, some pages, or nothing while the answer is unusable", () => {
    expect(rangePreview("", report)).toBe("Insert all 8 pages of report.pdf");
    expect(rangePreview("", { ...report, pages: 1 })).toBe("Insert the page of report.pdf");
    expect(rangePreview("2-8", report)).toBe("Insert pages 2-8 of report.pdf");
    expect(rangePreview("3", report)).toBe("Insert page 3 of report.pdf");
    expect(rangePreview("1-8", report)).toBe("Insert all 8 pages of report.pdf");
    expect(rangePreview("9", report)).toBe("");
  });
});
