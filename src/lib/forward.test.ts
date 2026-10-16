import { describe, expect, it } from "vitest";
import { forwardSaid, parseForward } from "./forward";

describe("A --forward-search value", () => {
  it("is a file and a line, split at the last colon", () => {
    expect(parseForward("/proj/ch/intro.tex:12")).toEqual({ source: "/proj/ch/intro.tex", line: 12 });
    expect(parseForward(" C:\\proj\\intro.tex:7 ")).toEqual({ source: "C:\\proj\\intro.tex", line: 7 });
  });

  it("refuses a value that is not a file and a line", () => {
    expect(parseForward("intro.tex")).toHaveProperty("problem");
    expect(parseForward(":12")).toHaveProperty("problem");
    expect(parseForward("intro.tex:twelve")).toHaveProperty("problem");
    expect(parseForward("intro.tex:")).toHaveProperty("problem");
    expect(parseForward("intro.tex:0")).toEqual({
      problem: 'tpdf was asked to --forward-search "intro.tex:0", and lines count from 1',
    });
  });

  it("says nothing when the line asked for is the one shown, and which line otherwise", () => {
    const found = { page: 1, line: 6, boxes: [] };
    expect(forwardSaid(found, 6)).toBeNull();
    expect(forwardSaid(found, 5)).toBe("Line 5 made nothing on the page; showing line 6, the nearest that did.");
  });
});
