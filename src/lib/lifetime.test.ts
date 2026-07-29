import { describe, expect, it, vi } from "vitest";

import { Lifetime } from "./lifetime";

describe("Lifetime", () => {
  it("runs the live arm before it ends", () => {
    const life = new Lifetime();
    const live = vi.fn();
    const dispose = vi.fn();

    life.claim(live, dispose)("tile");

    // Both halves. Without the second, a `claim` that ran *neither* arm would
    // pass the first --- and running neither is the leak, not the fix.
    expect(live).toHaveBeenCalledWith("tile");
    expect(dispose).not.toHaveBeenCalled();
  });

  it("runs the dispose arm after it ends, with the value", () => {
    const life = new Lifetime();
    const live = vi.fn();
    const dispose = vi.fn();
    const guarded = life.claim(live, dispose);

    life.end();
    guarded("tile");

    expect(live).not.toHaveBeenCalled();
    // The value has to reach the disposal, or there is nothing to release ---
    // which is the whole difference between this and an early return.
    expect(dispose).toHaveBeenCalledWith("tile");
  });

  it("decides at call time, not when the guard was made", () => {
    // The guard is built when the request goes out and called when it lands, so
    // an implementation that read `ended` while wrapping would answer with the
    // state at request time --- always live, since a destroyed object issues
    // nothing. That is the failure this exists to prevent, spelled backwards.
    const life = new Lifetime();
    const live = vi.fn();
    const dispose = vi.fn();
    const guarded = life.claim(live, dispose);

    guarded("first");
    life.end();
    guarded("second");

    expect(live.mock.calls).toEqual([["first"]]);
    expect(dispose.mock.calls).toEqual([["second"]]);
  });

  it("reports whether it has ended", () => {
    const life = new Lifetime();
    expect(life.ended).toBe(false);
    life.end();
    expect(life.ended).toBe(true);
  });

  it("stays ended", () => {
    // There is no revival, and nothing should invent one: a second `end` is what
    // a double teardown produces, and it must not read as a reset.
    const life = new Lifetime();
    life.end();
    life.end();
    expect(life.ended).toBe(true);
  });
});
