/**
 * Tests for the retry wait behind every tile request.
 *
 * The behaviour being pinned is not "requests are delayed" --- it is the three
 * ways this can turn back into the busy loop it exists to remove: a wait that
 * restarts instead of doubling, a due entry reported as a wake, and a clear on
 * something that is not a reader's action. Each of those looks like a working
 * backoff from every other angle and costs a core for as long as the document
 * stays open.
 *
 * The clock is a plain number here for the same reason the class takes one: a
 * test that has to wait 250 ms to observe a 250 ms wait is a slow test that
 * still cannot say which of the two readings the code used.
 *
 * Every test below was checked by mutating `backoff.ts` and confirming it went
 * red; the integration with the scroller is covered separately, in
 * `scroller.test.ts`.
 */

import { describe, expect, it } from "vitest";

import { Backoff, RETRY_BASE_MS, RETRY_MAX_MS } from "./backoff";

describe("Backoff", () => {
  it("does not reissue a request before its wait has elapsed", () => {
    const backoff = new Backoff();
    backoff.note("a", 1000);
    expect(backoff.blocked("a", 1000 + RETRY_BASE_MS - 1)).toBe(true);
  });

  it("reissues once the wait has elapsed", () => {
    // The other half of the test above, and the one that fails if a wait is
    // ever treated as permanent: a transient failure has to recover.
    const backoff = new Backoff();
    backoff.note("a", 1000);
    expect(backoff.blocked("a", 1000 + RETRY_BASE_MS)).toBe(false);
  });

  it("knows nothing about a key that has not failed", () => {
    expect(new Backoff().blocked("a", 0)).toBe(false);
  });

  it("doubles the wait on each further failure", () => {
    const backoff = new Backoff();
    backoff.note("a", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_BASE_MS);
    backoff.note("a", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_BASE_MS * 2);
    backoff.note("a", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_BASE_MS * 4);
  });

  it("keeps doubling across an entry that has come due", () => {
    // The reason a due entry is kept in the map rather than deleted. Deleting
    // it would make every retry start from 250 ms again, so a document that
    // fails every time settles into a fixed four-per-second retry rate --- less
    // pathological than the per-frame loop, and just as permanent.
    const backoff = new Backoff();
    backoff.note("a", 0);
    const due = RETRY_BASE_MS;
    expect(backoff.blocked("a", due)).toBe(false);
    backoff.note("a", due);
    expect(backoff.nextWaitMs(due)).toBe(RETRY_BASE_MS * 2);
  });

  it("stops doubling at the ceiling", () => {
    const backoff = new Backoff();
    for (let tries = 0; tries < 20; tries++) backoff.note("a", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_MAX_MS);
  });

  it("reports no wake for a request that is already due", () => {
    // The busy-loop guard. A due request is issued by the next frame anyway, so
    // reporting it here would have the caller schedule an immediate wake, which
    // runs a frame, which finds the request due, which schedules another wake.
    const backoff = new Backoff();
    backoff.note("a", 0);
    expect(backoff.nextWaitMs(RETRY_BASE_MS)).toBeNull();
    expect(backoff.nextWaitMs(RETRY_BASE_MS + 1000)).toBeNull();
  });

  it("reports no wake when nothing has failed", () => {
    expect(new Backoff().nextWaitMs(0)).toBeNull();
  });

  it("names the soonest of several waiting requests", () => {
    // A wake is one timer, so it has to be armed for the earliest entry: armed
    // for a later one, every request between now and then waits behind it.
    const backoff = new Backoff();
    backoff.note("late", 0);
    backoff.note("late", 0);
    backoff.note("soon", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_BASE_MS);
  });

  it("counts a due entry out of the soonest, not into it", () => {
    // The control for the test above: the minimum has to be taken over the
    // entries that are still waiting, and a defect that took it over all of
    // them would return a negative wait --- a timer that fires immediately, on
    // a request the next frame was going to issue anyway.
    const backoff = new Backoff();
    backoff.note("due", 0);
    backoff.note("waiting", 0);
    backoff.note("waiting", 0);
    const after = RETRY_BASE_MS;
    expect(backoff.nextWaitMs(after)).toBe(RETRY_BASE_MS * 2 - after);
  });

  it("forgets a key that succeeded", () => {
    const backoff = new Backoff();
    backoff.note("a", 0);
    backoff.clear("a");
    expect(backoff.blocked("a", 0)).toBe(false);
    expect(backoff.nextWaitMs(0)).toBeNull();
  });

  it("starts a key that succeeded and failed again from the base wait", () => {
    // A success is evidence the request can be served, so the next failure is a
    // first failure. Without this a page that mostly works would drift towards
    // the eight-second ceiling on nothing but bad luck.
    const backoff = new Backoff();
    backoff.note("a", 0);
    backoff.note("a", 0);
    backoff.clear("a");
    backoff.note("a", 0);
    expect(backoff.nextWaitMs(0)).toBe(RETRY_BASE_MS);
  });

  it("counts failures per key", () => {
    // The count is what makes a failure reason worth printing once rather than
    // on every retry, so a key must not inherit another key's tries.
    const backoff = new Backoff();
    expect(backoff.note("a", 0)).toBe(1);
    expect(backoff.note("b", 0)).toBe(1);
    expect(backoff.note("a", 0)).toBe(2);
  });

  it("forgets everything at once", () => {
    const backoff = new Backoff();
    backoff.note("a", 0);
    backoff.note("b", 0);
    backoff.clearAll();
    expect(backoff.blocked("a", 0)).toBe(false);
    expect(backoff.blocked("b", 0)).toBe(false);
    expect(backoff.nextWaitMs(0)).toBeNull();
  });
});
