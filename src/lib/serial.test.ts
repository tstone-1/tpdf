import { describe, expect, it } from "vitest";

import { Serial } from "./serial";
import { settle } from "./testdom";

/** A promise with its settle handles, so a test decides when a body finishes. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("Serial", () => {
  it("does not start a body while another is running", async () => {
    const serial = new Serial();
    const first = deferred<void>();
    const started: string[] = [];

    const a = serial.run(async () => {
      started.push("a");
      await first.promise;
    });
    const b = serial.run(async () => {
      started.push("b");
    });

    // A queued body starts on a microtask, not synchronously, so this has to
    // drain them --- and it is the whole assertion: `a` has run, `b` has not.
    // The first half is the control. Without it, "b has not started" would be
    // just as true of a `Serial` that never ran anything at all.
    await settle();
    expect(started).toEqual(["a"]);

    first.resolve();
    await Promise.all([a, b]);
    expect(started).toEqual(["a", "b"]);
  });

  it("starts bodies in the order they were queued", async () => {
    const serial = new Serial();
    const order: number[] = [];
    await Promise.all(
      [1, 2, 3, 4].map((n) =>
        serial.run(async () => {
          order.push(n);
        }),
      ),
    );
    expect(order).toEqual([1, 2, 3, 4]);
  });

  it("runs the next body after one rejects", async () => {
    const serial = new Serial();
    const ran: string[] = [];

    const failed = serial.run(async () => {
      ran.push("doomed");
      throw new Error("could not open");
    });
    const after = serial.run(async () => {
      ran.push("after");
    });

    // The caller of the failing body is told; the chain is not stopped by it.
    await expect(failed).rejects.toThrow("could not open");
    await after;
    expect(ran).toEqual(["doomed", "after"]);
  });

  it("a body that rejects still holds the queue until it settles", async () => {
    // The failure arm has to serialise exactly like the success arm. A chain
    // that only awaited its predecessor on success would let the next body run
    // beside a failing one --- which is the interleaving, arriving through the
    // path taken by every document that does not open.
    const serial = new Serial();
    const first = deferred<void>();
    const started: string[] = [];

    const doomed = serial.run(async () => {
      started.push("doomed");
      await first.promise;
    });
    const next = serial.run(async () => {
      started.push("next");
    });

    await settle();
    expect(started).toEqual(["doomed"]);
    first.reject(new Error("no"));
    await expect(doomed).rejects.toThrow("no");
    await next;
    expect(started).toEqual(["doomed", "next"]);
  });

  it("resolves with the body's own value", async () => {
    const serial = new Serial();
    await expect(serial.run(async () => 42)).resolves.toBe(42);
  });
});
