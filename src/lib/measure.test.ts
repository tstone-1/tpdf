import { describe, expect, it } from "vitest";
import {
  distance,
  endTicks,
  formatLength,
  keepOnPage,
  Measurements,
  parseKnownLength,
  straighten,
} from "./measure";
import { pageId } from "./pages";

describe("a typed length", () => {
  it.each([
    ["195 cm", 195, "cm", 0],
    ["195cm", 195, "cm", 0],
    ["  1.95 m ", 1.95, "m", 2],
    ["1,95 m", 1.95, "m", 2],
    [",5 km", 0.5, "km", 1],
    ["12 Fuß", 12, "Fuß", 0],
    ['6"', 6, '"', 0],
    ["3'", 3, "'", 0],
  ])("%s is understood", (raw, value, unit, decimals) => {
    expect(parseKnownLength(raw)).toEqual({ value, unit, decimals });
  });

  it.each(["", "195", "cm", "0 cm", "0,0 cm", "-5 cm", "1.234,5 cm", "1 2 cm", "5 averyverylongunit", "1e3 cm", "Infinity m"])(
    "%j is refused",
    (raw) => {
      expect(parseKnownLength(raw)).toBe(null);
    },
  );

  it("refuses a number too long to be finite rather than scaling by infinity", () => {
    expect(parseKnownLength(`${"9".repeat(400)} cm`)).toBe(null);
  });

  it("caps the decimals it keeps, so a long fraction cannot reach Intl's limit", () => {
    expect(parseKnownLength(`1.${"1".repeat(40)} m`)?.decimals).toBe(6);
  });
});

describe("a length as text", () => {
  it("rounds to the decimals asked for, and drops a trailing zero", () => {
    expect(formatLength(82.44, "cm", 1, "en-US")).toBe("82.4 cm");
    expect(formatLength(195, "cm", 1, "en-US")).toBe("195 cm");
  });

  it("writes the German comma in both forms of address", () => {
    expect(formatLength(82.44, "cm", 1, "de-DE")).toBe("82,4 cm");
    expect(formatLength(82.44, "cm", 1, "de-DE-formal")).toBe("82,4 cm");
  });

  it("puts no space before a foot or inch sign", () => {
    expect(formatLength(6, '"', 1, "en-US")).toBe('6"');
  });
});

describe("the geometry", () => {
  it("leaves the end alone without Shift", () => {
    expect(straighten({ x: 0, y: 0 }, { x: 30, y: 40 }, false)).toEqual({ x: 30, y: 40 });
  });

  it("with Shift, snaps to the axis the line is nearer to", () => {
    expect(straighten({ x: 10, y: 10 }, { x: 50, y: 20 }, true)).toEqual({ x: 50, y: 10 });
    expect(straighten({ x: 10, y: 10 }, { x: 20, y: 50 }, true)).toEqual({ x: 10, y: 50 });
  });

  it("keeps a point on the page, on all four sides", () => {
    const page = { width: 100, height: 200 };
    expect(keepOnPage({ x: -5, y: 300 }, page)).toEqual({ x: 0, y: 200 });
    expect(keepOnPage({ x: 105, y: -1 }, page)).toEqual({ x: 100, y: 0 });
    expect(keepOnPage({ x: 5, y: 6 }, page)).toEqual({ x: 5, y: 6 });
  });

  it("draws the end ticks across the line, at both ends", () => {
    expect(endTicks({ x: 0, y: 0 }, { x: 10, y: 0 }, 2)).toEqual([
      [{ x: 0, y: -2 }, { x: 0, y: 2 }],
      [{ x: 10, y: -2 }, { x: 10, y: 2 }],
    ]);
  });

  it("has no ticks for a line of no length, rather than dividing by zero", () => {
    expect(endTicks({ x: 3, y: 3 }, { x: 3, y: 3 }, 2)).toEqual([]);
  });
});

describe("the measurements of a document", () => {
  const one = pageId(1);
  const two = pageId(2);
  const anywhere = () => true;

  /** The example this tool was asked for: a line that is 195 cm, then another. */
  function scaled(): { measures: Measurements; first: ReturnType<Measurements["add"]> } {
    const measures = new Measurements();
    const first = measures.add(one, { x: 0, y: 0 }, { x: 390, y: 0 });
    expect(measures.setScale(first, { value: 195, unit: "cm", decimals: 0 })).toBe(true);
    return { measures, first };
  }

  it("reads in points until the page has a scale", () => {
    const measures = new Measurements();
    const line = measures.add(one, { x: 0, y: 0 }, { x: 30, y: 40 });
    expect(measures.label(line, "en-US")).toBe("50 pt");
    expect(measures.scaled(one)).toBe(false);
  });

  it("reads a second line in the unit of the first", () => {
    const { measures } = scaled();
    const second = measures.add(one, { x: 10, y: 10 }, { x: 10, y: 174.8 });
    expect(measures.label(second, "en-US")).toBe("82.4 cm");
  });

  it("names the line the scale came from", () => {
    const { measures, first } = scaled();
    expect(measures.label(first, "en-US")).toBe("195 cm (scale)");
    expect(measures.label(first, "de-DE")).toBe("195 cm (Maßstab)");
  });

  it("keeps a page's scale to that page", () => {
    const { measures } = scaled();
    const elsewhere = measures.add(two, { x: 0, y: 0 }, { x: 100, y: 0 });
    expect(measures.label(elsewhere, "en-US")).toBe("100 pt");
  });

  it("shows later lines as precisely as the reader typed the scale", () => {
    const measures = new Measurements();
    const first = measures.add(one, { x: 0, y: 0 }, { x: 100, y: 0 });
    measures.setScale(first, { value: 1.95, unit: "m", decimals: 2 });
    const second = measures.add(one, { x: 0, y: 0 }, { x: 42, y: 0 });
    expect(measures.label(second, "en-US")).toBe("0.82 m");
  });

  it("replaces a page's scale, and only one line is then the reference", () => {
    const { measures, first } = scaled();
    const second = measures.add(one, { x: 0, y: 0 }, { x: 100, y: 0 });
    measures.setScale(second, { value: 50, unit: "mm", decimals: 0 });
    expect(measures.label(second, "en-US")).toBe("50 mm (scale)");
    expect(measures.label(first, "en-US")).toBe("195 mm");
  });

  it("refuses a scale from a line of no length", () => {
    const measures = new Measurements();
    const dot = measures.add(one, { x: 5, y: 5 }, { x: 5, y: 5 });
    expect(measures.setScale(dot, { value: 1, unit: "m", decimals: 0 })).toBe(false);
    expect(measures.scaled(one)).toBe(false);
  });

  it("takes the newest line as the one a scale refers to", () => {
    const measures = new Measurements();
    measures.add(one, { x: 0, y: 0 }, { x: 1, y: 0 });
    const newest = measures.add(two, { x: 0, y: 0 }, { x: 2, y: 0 });
    expect(measures.latest(anywhere)).toBe(newest);
  });

  it("passes over a line whose page has been deleted", () => {
    const measures = new Measurements();
    const kept = measures.add(one, { x: 0, y: 0 }, { x: 1, y: 0 });
    measures.add(two, { x: 0, y: 0 }, { x: 2, y: 0 });
    expect(measures.latest((page) => page !== two)).toBe(kept);
    expect(measures.latest(() => false)).toBe(null);
  });

  it("clears the scales with the lines, so no invisible scale stays behind", () => {
    const { measures } = scaled();
    measures.clear();
    expect(measures.count).toBe(0);
    expect(measures.scaled(one)).toBe(false);
    const again = measures.add(one, { x: 0, y: 0 }, { x: 10, y: 0 });
    expect(measures.label(again, "en-US")).toBe("10 pt");
  });

  it("measures the same at any turn, since a turn moves a line and stretches nothing", () => {
    expect(distance({ x: 0, y: 0 }, { x: 30, y: 40 })).toBe(distance({ x: 0, y: 0 }, { x: 40, y: 30 }));
  });
});
