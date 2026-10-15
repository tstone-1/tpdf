/**
 * The three layouts by name, and a layout's round trip through the session.
 */

import { describe, expect, it } from "vitest";

import { SINGLE, type Spread } from "./scroller";
import { kindOf, placeOfSpread, SPREAD_KINDS, spreadOfPlace, withKind } from "./spreads";

const FACING: Spread = { columns: 2, coverAlone: false, rightToLeft: false, continuous: true };

describe("The layouts by name", () => {
  it("names each spread", () => {
    expect(kindOf(SINGLE)).toBe("single");
    expect(kindOf(FACING)).toBe("facing");
    expect(kindOf({ ...FACING, coverAlone: true })).toBe("book");
  });

  it("changes to each layout and back to its own name", () => {
    for (const kind of SPREAD_KINDS) {
      expect(kindOf(withKind(SINGLE, kind)), kind).toBe(kind);
    }
  });

  it("keeps right to left through a change of layout", () => {
    const manga = { ...FACING, rightToLeft: true };
    expect(withKind(manga, "book").rightToLeft).toBe(true);
    expect(withKind(withKind(manga, "single"), "facing").rightToLeft).toBe(true);
    // The control: a spread that was not right to left does not become one.
    expect(withKind(FACING, "book").rightToLeft).toBe(false);
  });
});

describe("A layout in the session", () => {
  it("keeps one page at a time through a change of layout", () => {
    expect(withKind({ ...SINGLE, continuous: false }, "facing").continuous).toBe(false);
    expect(withKind(SINGLE, "book").continuous).toBe(true);
  });

  it("comes back as it was written", () => {
    for (const spread of [
      SINGLE,
      FACING,
      { ...FACING, coverAlone: true },
      { ...FACING, rightToLeft: true },
      { ...SINGLE, continuous: false },
      { ...FACING, continuous: false },
    ]) {
      expect(spreadOfPlace(placeOfSpread(spread))).toEqual(spread);
    }
  });

  it("is one page to a row for a place written before facing pages", () => {
    expect(spreadOfPlace({})).toEqual(SINGLE);
  });

  it("is one page to a row for a column count other than two", () => {
    expect(spreadOfPlace({ columns: 3, cover_alone: true })).toEqual(SINGLE);
    expect(spreadOfPlace({ columns: 0 })).toEqual(SINGLE);
  });
});
