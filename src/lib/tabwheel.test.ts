import { describe, expect, it } from "vitest";
import { sidewaysBy } from "./tabwheel";

describe("a wheel over the tab strip", () => {
  it("turns a vertical turn into the same distance sideways", () => {
    expect(sidewaysBy({ deltaX: 0, deltaY: 120, deltaMode: 0 }, 800)).toBe(120);
    expect(sidewaysBy({ deltaX: 0, deltaY: -120, deltaMode: 0 }, 800)).toBe(-120);
  });

  it("leaves a sideways swipe to the strip itself", () => {
    // A trackpad's two-finger swipe, and a wheel turned with Shift held, both
    // arrive as a sideways delta the strip already scrolls for.
    expect(sidewaysBy({ deltaX: 90, deltaY: 4, deltaMode: 0 }, 800)).toBe(0);
    expect(sidewaysBy({ deltaX: -90, deltaY: 90, deltaMode: 0 }, 800)).toBe(0);
    expect(sidewaysBy({ deltaX: 0, deltaY: 0, deltaMode: 0 }, 800)).toBe(0);
  });

  it("counts lines and pages in pixels", () => {
    // Firefox reports a mouse wheel in lines, and three lines is not three
    // pixels. The two modes are told apart by widths that differ from a line.
    expect(sidewaysBy({ deltaX: 0, deltaY: 3, deltaMode: 1 }, 800)).toBe(120);
    expect(sidewaysBy({ deltaX: 0, deltaY: -1, deltaMode: 2 }, 800)).toBe(-800);
  });
});
