import { describe, expect, it } from "vitest";

import { describeRange, namePages, parsePageRange } from "./pageranges";

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
