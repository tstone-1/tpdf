import { describe, expect, it } from "vitest";

import { flatten, rowLine } from "./rowline";

describe("rowLine", () => {
  it("returns what was typed, flattened, as somebody's own", () => {
    expect(rowLine("two\nlines", "covered", "nothing")).toEqual({
      text: "two lines",
      own: true,
    });
  });

  it("falls back to the covered words, flattened, as not their own", () => {
    // Flattened for the same reason a note is: the words a mark covers run over
    // the lines of the page they came off, and a row is one line high.
    expect(rowLine("   ", "over\ntwo   lines", "nothing")).toEqual({
      text: "over two lines",
      own: false,
    });
  });

  it("uses the caller's own fallback when there is neither", () => {
    // Two callers, two fallbacks, and this is the whole reason it is a
    // parameter: "No note" is right for a mark the reader made and wrong for
    // somebody else's highlight, which has to name its kind.
    expect(rowLine("", "", "No note")).toEqual({ text: "No note", own: false });
    expect(rowLine(" \n ", "\t", "Highlight, no comment")).toEqual({
      text: "Highlight, no comment",
      own: false,
    });
  });

  it("collapses every run of whitespace, not only newlines", () => {
    expect(flatten("  a \t\n  b  ")).toBe("a b");
  });
});
