/**
 * What one side's frame does to the other with scrolling together on
 * (`syncfollow.ts`). Where the partner belongs is `syncscroll.ts`'s and has its
 * own tests; these are the moves, the flag that keeps a move from coming back,
 * and the Alt wheel.
 */

import { describe, expect, it } from "vitest";

import { ALONE_MS, SyncFollow, type Follower } from "./syncfollow";
import { ScrollLock, type Reading } from "./syncscroll";

/** A viewer that is where it is told, and can report a frame from inside a move. */
class Fake implements Follower {
  reading: Reading;
  currentZoom: number;
  moves: Reading[] = [];
  zooms: number[] = [];
  /** Called inside `followTo`, as a real viewer draws a frame while it is moved. */
  onMove: () => void = () => {};

  constructor(page: number, zoom = 1) {
    this.reading = { page, fraction: 0 };
    this.currentZoom = zoom;
  }

  setZoomFixed(zoom: number): void {
    this.currentZoom = zoom;
    this.zooms.push(zoom);
  }

  followTo(page: number, fraction: number): void {
    this.reading = { page, fraction };
    this.moves.push(this.reading);
    this.onMove();
  }
}

function pair(now: () => number = () => 0) {
  const left = new Fake(2), right = new Fake(5);
  const follow = new SyncFollow<number, Fake>(new ScrollLock<number>(), now);
  follow.hold({ id: 1, viewer: left }, { id: 2, viewer: right });
  return { left, right, follow };
}

describe("SyncFollow", () => {
  it("moves the other side to keep the offset it was locked with", () => {
    const { left, right, follow } = pair();
    left.reading = { page: 4, fraction: 0.5 };
    follow.step(1, left, false);
    expect(right.moves).toEqual([{ page: 7, fraction: 0.5 }]);
    // Control: a frame that moved nothing moves nothing.
    follow.step(1, left, false);
    expect(right.moves).toHaveLength(1);
  });

  it("does not send a move back to the side it came from", () => {
    // The partner draws a frame while it is moved, and that frame is answered
    // with nothing. Mutation: leave the flag down during the move -> red.
    const { left, right, follow } = pair();
    right.onMove = () => follow.step(2, right, false);
    left.reading = { page: 3, fraction: 0 };
    follow.step(1, left, false);
    expect(left.moves).toEqual([]);
    expect(right.moves).toEqual([{ page: 6, fraction: 0 }]);
  });

  it("passes a zoom the reader set across, and not one the window made", () => {
    // Mutation: pass `false` for `fitted` to the lock -> red.
    const { left, right, follow } = pair();
    left.currentZoom = 2;
    follow.step(1, left, false);
    expect(right.zooms).toEqual([2]);
    left.currentZoom = 3;
    follow.step(1, left, true);
    expect(right.zooms).toEqual([2]);
  });

  it("scrolls one side alone for a moment after a wheel with Alt", () => {
    // Mutation: `aloneFor` not setting the clock, or `<` turned to `>` -> red.
    let clock = 1_000;
    const { left, right, follow } = pair(() => clock);
    follow.aloneFor();
    left.reading = { page: 3, fraction: 0 };
    follow.step(1, left, false);
    expect(right.moves).toEqual([]);
    // Past the moment, the new offset holds: the right was three pages ahead
    // and is now two.
    clock += ALONE_MS;
    left.reading = { page: 4, fraction: 0 };
    follow.step(1, left, false);
    expect(right.moves).toEqual([{ page: 6, fraction: 0 }]);
  });

  it("moves nothing once released, or for a document it does not hold", () => {
    const { left, right, follow } = pair();
    left.reading = { page: 9, fraction: 0 };
    follow.step(3, left, false);
    expect(right.moves).toEqual([]);
    follow.release();
    follow.step(1, left, false);
    expect(right.moves).toEqual([]);
  });
});
