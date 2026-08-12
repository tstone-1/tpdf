/**
 * Tests for the delay in front of the degraded-state label.
 *
 * The behaviour being pinned is not "the label is delayed" --- it is the four
 * ways this turns back into either the flicker it removes or a viewer that goes
 * quiet about a real problem: a delay applied to a failure, an episode clock
 * that restarts when the wording changes, a clock that never restarts between
 * episodes, and a label that is held after the page is readable.
 *
 * Each of those looks like a working delay from every other angle. The first
 * and the last are the expensive ones, because both fail *silently* --- a
 * suppressed failure and a stale warning are each indistinguishable from the
 * indicator working, right up to the moment somebody is misled by it.
 *
 * The clock is a plain number here for the reason `backoff.test.ts` gives.
 * Every test below was checked by mutating `degraded.ts` and confirming it went
 * red; the mutations and their outcomes are named on each block.
 */

import { describe, expect, it } from "vitest";

import { DegradedLabel, describeDegraded, SHOW_AFTER_MS, type Coverage } from "./degraded";

/** Sharp, complete, idle: the state that says nothing at all. */
const clean: Coverage = { failed: 0, any: 1, sharp: 1, pending: 0 };

describe("describeDegraded", () => {
  it("says nothing about a page that is sharp and settled", () => {
    // The control for every test below. Without it, a classifier that returned
    // a label unconditionally would pass all of them.
    expect(describeDegraded(clean)).toBeNull();
  });

  it("reports a failure ahead of any slowness, because waiting does not fix it", () => {
    // Mutation: move the `failed` test below the `any` test -> red. A document
    // erroring on every request also has no coverage, so the two are true at
    // once and only the order distinguishes them.
    const both = { ...clean, failed: 1, any: 0, sharp: 0 };
    expect(describeDegraded(both)).toEqual({
      text: "some pages could not be drawn",
      urgent: true,
    });
  });

  it("distinguishes no page at all from a page that cannot be read", () => {
    // Mutation: report `sharp` for both -> red. These are different failures
    // and PLAN.md section 9 requires they be reported as different.
    expect(describeDegraded({ ...clean, any: 0.5, sharp: 0.5 })?.text).toBe("preparing page");
    expect(describeDegraded({ ...clean, sharp: 0.5 })?.text).toBe("sharpening");
  });

  it("treats a rounding step short of full coverage as full", () => {
    // Mutation: compare against 1 rather than 0.999 -> red. This is the
    // original flicker guard and it has to keep working.
    expect(describeDegraded({ ...clean, any: 0.9995, sharp: 0.9995 })).toBeNull();
  });

  it("reports work in flight over a page that is already readable", () => {
    expect(describeDegraded({ ...clean, pending: 3 })?.text).toBe("loading ahead");
  });

  it("has nothing to say before a document is open", () => {
    expect(describeDegraded(null)).toBeNull();
  });
});

describe("DegradedLabel", () => {
  const blurry: Coverage = { ...clean, sharp: 0.4 };

  it("says nothing while a transient state is younger than the delay", () => {
    // The whole point: a scroll that resolves within a few frames is silent.
    const label = new DegradedLabel();
    expect(label.update(blurry, 1000)).toBeNull();
    expect(label.update(blurry, 1000 + SHOW_AFTER_MS - 1)).toBeNull();
  });

  it("speaks once the transient state has lasted the delay", () => {
    // The other half of the test above, and the one that fails if the delay is
    // ever treated as permanent: a genuinely slow page has to report itself.
    const label = new DegradedLabel();
    label.update(blurry, 1000);
    expect(label.update(blurry, 1000 + SHOW_AFTER_MS)).toBe("sharpening");
  });

  it("reports a failure immediately, with no delay at all", () => {
    // Mutation: drop the `urgent` early return so failures are timed like
    // everything else -> red. `failed` can arrive with the frame loop already
    // idle, so a delayed failure is not postponed, it is never shown.
    const label = new DegradedLabel();
    expect(label.update({ ...clean, failed: 1 }, 1000)).toBe("some pages could not be drawn");
  });

  it("times the episode, not the wording", () => {
    // Mutation: restart `#since` when the text changes -> red. A page going
    // from "preparing page" to "sharpening" never became readable, so the
    // reader has been waiting the whole time.
    const label = new DegradedLabel();
    label.update({ ...clean, any: 0.2, sharp: 0 }, 1000);
    expect(label.update(blurry, 1000 + SHOW_AFTER_MS)).toBe("sharpening");
  });

  it("restarts the clock for a new episode after the page becomes readable", () => {
    // Mutation: leave `#since` alone when the state clears -> red, and this is
    // the mutation that reinstates the original flicker: a second brief dip
    // would then be shown instantly because the first episode's clock is still
    // running.
    const label = new DegradedLabel();
    label.update(blurry, 1000);
    label.update(blurry, 1000 + SHOW_AFTER_MS);
    expect(label.update(clean, 2000)).toBeNull();
    expect(label.update(blurry, 2001)).toBeNull();
  });

  it("stops speaking the moment the page is readable, even mid-episode", () => {
    // No minimum display time, deliberately: the caller has no timer with which
    // to keep one, so a hold would leave the label stuck. Mutation: hold the
    // last text for any nonzero time -> red.
    const label = new DegradedLabel();
    label.update(blurry, 1000);
    expect(label.update(blurry, 1000 + SHOW_AFTER_MS)).toBe("sharpening");
    expect(label.update(clean, 1000 + SHOW_AFTER_MS + 1)).toBeNull();
  });
});
