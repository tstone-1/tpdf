/**
 * Which released press asks for the graphic under it (`isGraphicClick` in
 * `viewer.ts`). The graphic found is `tests/cli/graphic.rs`.
 */

import { describe, expect, it } from "vitest";

import { isGraphicClick } from "./viewer";

const PRESS = { x: 100, y: 100, text: false };
const HERE = { clientX: 101, clientY: 102 };

describe("isGraphicClick", () => {
  it("takes a still, single press on paper with no tool and nothing selected", () => {
    expect(isGraphicClick(PRESS, HERE, "none", "char", false)).toBe(true);
  });

  it("leaves a drag, text, a tool, a double click and a selection alone", () => {
    // Mutation: any one condition dropped -> one of these goes red.
    expect(isGraphicClick(PRESS, { clientX: 110, clientY: 100 }, "none", "char", false)).toBe(false);
    expect(isGraphicClick({ ...PRESS, text: true }, HERE, "none", "char", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "region", "char", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "none", "word", false)).toBe(false);
    expect(isGraphicClick(PRESS, HERE, "none", "char", true)).toBe(false);
  });
});
