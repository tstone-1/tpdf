import { describe, expect, it } from "vitest";
import { FURTHER_VIEWS, NO_VIEW, ViewIds, isFurther, viewOf } from "./views";

describe("the id of a view", () => {
  it("is the document's handle for its first view", () => {
    expect(viewOf(7)).toBe(7);
    expect(isFurther(viewOf(7))).toBe(false);
    expect(isFurther(NO_VIEW)).toBe(false);
  });

  it("is a number no handle has for a further view, and a new one each time", () => {
    const ids = new ViewIds();
    const first = ids.another();
    const second = ids.another();
    expect(first).toBe(FURTHER_VIEWS);
    expect(second).not.toBe(first);
    expect(isFurther(first) && isFurther(second)).toBe(true);
    // A handle is a 32-bit count in the backend.
    expect(FURTHER_VIEWS).toBeGreaterThan(2 ** 32);
    expect(Number.isSafeInteger(FURTHER_VIEWS + 1_000_000)).toBe(true);
  });
});
