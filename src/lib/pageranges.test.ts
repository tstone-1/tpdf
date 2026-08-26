import { describe, expect, it } from "vitest";

import {
  describeRange,
  describeSplit,
  namePages,
  parsePageRange,
  parseSplitPoints,
} from "./pageranges";

describe("parseSplitPoints", () => {
  it("cuts after the page named, so the number is a file's last page", () => {
    expect(parseSplitPoints("3", 10).groups).toEqual([
      [0, 1, 2],
      [3, 4, 5, 6, 7, 8, 9],
    ]);
  });

  it("makes one file more than there are cuts", () => {
    expect(parseSplitPoints("3,7", 10).groups).toEqual([
      [0, 1, 2],
      [3, 4, 5, 6],
      [7, 8, 9],
    ]);
  });

  it("covers every page exactly once, whatever the cuts", () => {
    // The property the group arithmetic exists to hold, and the one a
    // boundary mistake breaks in silence: a reader who loses page 6 to an
    // off-by-one still gets files, and they still open.
    const groups = parseSplitPoints("1,2,9", 10).groups ?? [];
    expect(groups.flat()).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it("cuts after the first page, which is the smallest split there is", () => {
    expect(parseSplitPoints("1", 3).groups).toEqual([[0], [1, 2]]);
  });

  it("cuts before the last page, which is the other end of the same bound", () => {
    expect(parseSplitPoints("2", 3).groups).toEqual([[0, 1], [2]]);
  });

  it("orders the cuts however they were typed", () => {
    expect(parseSplitPoints("7,3", 10).groups).toEqual(parseSplitPoints("3,7", 10).groups);
  });

  it("ignores the spaces people type around the separators", () => {
    expect(parseSplitPoints(" 3 , 7 ", 10).groups).toEqual(parseSplitPoints("3,7", 10).groups);
  });

  it("refuses the last page, because cutting after it makes an empty file", () => {
    expect(parseSplitPoints("10", 10).problem).toBe(
      "Page 10 is the last page, so cutting after it makes nothing",
    );
  });

  it("refuses a repeated cut rather than merging it, because it makes an empty file", () => {
    // The one place this is stricter than `parsePageRange`, which merges an
    // overlap. A set can contain a page twice and mean one page; a cut list
    // cannot, because the second cut names a file with no pages in it.
    expect(parseSplitPoints("3,3", 10).problem).toBe("Page 3 is named twice");
  });

  it("refuses a range, naming the box the reader meant", () => {
    expect(parseSplitPoints("1-3", 10).problem).toBe(
      'Split takes the pages to cut after, not a range like "1-3"',
    );
  });

  it("refuses a one-page document, which has nowhere to cut", () => {
    expect(parseSplitPoints("1", 1).problem).toBe("A one-page document cannot be split");
  });

  it("asks for input rather than complaining when the text is empty", () => {
    expect(parseSplitPoints("", 10).problem).toBe("Pages to cut after, 1 to 9");
  });

  it("refuses an empty part, which is what a trailing comma is", () => {
    expect(parseSplitPoints("3,", 10).problem).toBe('"3," has an empty part');
  });

  it("refuses a page past the end, naming the document's length", () => {
    expect(parseSplitPoints("11", 10).problem).toBe("This document has 10 pages");
  });

  it("refuses page zero, because the numbers are the printed ones", () => {
    expect(parseSplitPoints("0", 10).problem).toBe("This document has 10 pages");
  });
});

describe("describeSplit", () => {
  it("counts the files and the pages in each", () => {
    const groups = parseSplitPoints("3,7", 10).groups ?? [];
    expect(describeSplit(groups)).toBe("3 files: 3 + 4 + 3 pages");
  });
});

describe("parsePageRange", () => {
  it("reads a single page as one zero-based slot", () => {
    expect(parsePageRange("3", 10).slots).toEqual([2]);
  });

  it("reads a range inclusively at both ends", () => {
    expect(parsePageRange("2-4", 10).slots).toEqual([1, 2, 3]);
  });

  it("reads a list of ranges and singles together", () => {
    expect(parsePageRange("1-3,5", 10).slots).toEqual([0, 1, 2, 4]);
  });

  it("ignores the spaces people type around the separators", () => {
    expect(parsePageRange(" 1 - 3 , 5 ", 10).slots).toEqual([0, 1, 2, 4]);
  });

  it("merges an overlap rather than repeating the page", () => {
    // A subset is a set. Asking for page 2 twice cannot mean anything else,
    // so this is normalised where a reversed range is refused.
    expect(parsePageRange("1-3,2", 10).slots).toEqual([0, 1, 2]);
  });

  it("returns document order however the pages were typed", () => {
    // The property that keeps extract a *subset* rather than a reorder. If
    // this ever returns [4, 0], `write_copy` writes a two-page document with
    // the pages swapped and every downstream check still passes.
    expect(parsePageRange("5,1", 10).slots).toEqual([0, 4]);
  });

  it("sorts numerically, so page 11 comes after page 3", () => {
    // Written against slots 2 and 10, and the numbers matter: the first
    // version of this test used pages 1, 10 and 2, whose slots are 0, 9 and 1
    // -- and a lexicographic sort puts those in the same order a numeric one
    // does, because they are single digits. The mutation that removes the
    // comparator SURVIVED it. Slots 2 and 10 are the smallest pair that
    // discriminates: as strings "10" sorts before "2".
    expect(parsePageRange("11,3", 20).slots).toEqual([2, 10]);
  });

  it("refuses a range that runs backwards instead of correcting it", () => {
    expect(parsePageRange("5-3", 10).problem).toBe("5-3 runs backwards");
  });

  it("accepts a range whose ends are equal", () => {
    expect(parsePageRange("4-4", 10).slots).toEqual([3]);
  });

  it("refuses a page past the end, naming the document's length", () => {
    expect(parsePageRange("11", 10).problem).toBe("This document has 10 pages");
  });

  it("refuses page zero, because the numbers are the printed ones", () => {
    expect(parsePageRange("0", 10).problem).toBe("This document has 10 pages");
  });

  it("says pages in the singular for a one-page document", () => {
    expect(parsePageRange("2", 1).problem).toBe("This document has 1 page");
  });

  it("asks for input rather than complaining when the text is empty", () => {
    expect(parsePageRange("   ", 10).problem).toBe("Pages to extract, 1 to 10");
  });

  it("refuses an empty part, which is what a trailing comma is", () => {
    expect(parsePageRange("1,", 10).problem).toBe('"1," has an empty part');
  });

  it("refuses a missing end, which is what a trailing hyphen is", () => {
    expect(parsePageRange("2-", 10).problem).toBe("a page number is missing");
  });

  it.each(["+2", "2.0", "1e1", "two", "-3"])(
    "refuses %s, which Number() would have accepted",
    (text) => {
      expect(parsePageRange(text, 10).problem).toBeTruthy();
    },
  );

  it("refuses a second hyphen rather than reading past it", () => {
    expect(parsePageRange("1-3-5", 10).problem).toBeTruthy();
  });
});

describe("describeRange", () => {
  it("names the page when there is exactly one", () => {
    expect(describeRange([2])).toBe("Extract page 3");
  });

  it("counts when there is more than one", () => {
    expect(describeRange([0, 1, 4])).toBe("Extract 3 pages");
  });
});

describe("namePages", () => {
  it("names a single page in the singular", () => {
    expect(namePages([2])).toBe("page 3");
  });

  it("collapses a run back to the range that was typed", () => {
    expect(namePages([0, 1, 2])).toBe("pages 1-3");
  });

  it("writes a pair out rather than hyphenating it", () => {
    // `1-2` is no shorter than `1,2` and reads as a range where there is none
    // worth naming.
    expect(namePages([0, 1])).toBe("pages 1,2");
  });

  it("separates runs from singles", () => {
    expect(namePages([0, 1, 2, 4])).toBe("pages 1-3,5");
  });

  it("sorts and deduplicates what it is given", () => {
    expect(namePages([4, 0, 4])).toBe("pages 1,5");
  });

  it("survives an empty selection rather than producing a bare word", () => {
    expect(namePages([])).toBe("no pages");
  });

  it("round-trips a name back to the slots it came from", () => {
    // Not a general property -- `5,1` comes back in document order -- but for a
    // sorted selection the two functions must agree, and this is the pair a
    // suggested filename actually goes through.
    const slots = [0, 1, 2, 4, 7];
    const text = namePages(slots).replace(/^pages? /, "");
    expect(parsePageRange(text, 10).slots).toEqual(slots);
  });
});
