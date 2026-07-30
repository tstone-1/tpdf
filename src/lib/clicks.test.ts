/**
 * Tests for counting a run of clicks.
 *
 * All of it is boundaries, which is why it is a module rather than four lines
 * inside a pointer handler: one millisecond too late, one pixel too far, and
 * what a fourth click means.
 *
 * Every test below was checked by mutating `ClickCounter` and confirming it
 * went red.
 */

import { describe, expect, it } from "vitest";

import { ClickCounter, MULTI_CLICK_MS, MULTI_CLICK_SLOP_PX } from "./clicks";

describe("ClickCounter", () => {
  it("counts a lone click as the first of a run", () => {
    expect(new ClickCounter().press(10, 10, 0)).toBe(1);
  });

  it("counts clicks in the same place in quick succession", () => {
    const clicks = new ClickCounter();
    expect(clicks.press(10, 10, 0)).toBe(1);
    expect(clicks.press(10, 10, 100)).toBe(2);
    expect(clicks.press(10, 10, 200)).toBe(3);
  });

  it("starts again when the gap is too long", () => {
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    expect(clicks.press(10, 10, MULTI_CLICK_MS + 1)).toBe(1);
  });

  it("counts a click at exactly the deadline as part of the run", () => {
    // Both sides of the boundary, because a comparison that is wrong by one
    // passes any test that only ever looks at one of them.
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    expect(clicks.press(10, 10, MULTI_CLICK_MS)).toBe(2);
  });

  it("starts again when the pointer has moved too far", () => {
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    expect(clicks.press(10 + MULTI_CLICK_SLOP_PX + 1, 10, 50)).toBe(1);
  });

  it("tolerates the hand moving a pixel between press and release", () => {
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    expect(clicks.press(10 + MULTI_CLICK_SLOP_PX, 10, 50)).toBe(2);
  });

  it("measures the slop on both axes", () => {
    // Written after noticing that a check of `x` alone passes every test above:
    // they all hold y fixed, so an implementation that ignored y entirely would
    // be green. The hazard is a vertical move onto the line below, which is
    // exactly the case a word selection must not extend into.
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    expect(clicks.press(10, 10 + MULTI_CLICK_SLOP_PX + 1, 50)).toBe(1);
  });

  it("wraps back to a single click after the third", () => {
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    clicks.press(10, 10, 100);
    clicks.press(10, 10, 200);
    expect(clicks.press(10, 10, 300)).toBe(1);
    expect(clicks.press(10, 10, 400)).toBe(2);
  });

  it("measures the gap from the last click, not from the first", () => {
    // A triple-click taken slowly: each gap is inside the deadline but the run
    // is longer than it. Comparing against the run's start would break it.
    const clicks = new ClickCounter();
    clicks.press(10, 10, 0);
    clicks.press(10, 10, MULTI_CLICK_MS - 10);
    expect(clicks.press(10, 10, 2 * (MULTI_CLICK_MS - 10))).toBe(3);
  });

  it("measures the distance from the last click, not from where the run began", () => {
    // The counterpart to the test above, and it has to assert the whole
    // sequence rather than its last value. Written that way first and it could
    // not fail: a pointer creeping one slop-width per click gives 1 at the
    // seventh press under *either* rule --- the correct one cycles 1,2,3,1,2,3,1
    // and the origin-anchored one alternates 1,2,1,2,1,2,1. They differ at the
    // third press and agree at the seventh, which is the one a spot check looks
    // at.
    const clicks = new ClickCounter();
    let at = 10;
    const counts = [];
    for (let i = 0; i < 7; i++) counts.push(clicks.press((at += MULTI_CLICK_SLOP_PX), 10, i * 50));
    expect(counts).toEqual([1, 2, 3, 1, 2, 3, 1]);
  });
});
