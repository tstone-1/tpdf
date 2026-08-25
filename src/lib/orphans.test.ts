/**
 * What `releaseOrphans` says, and what it refuses to do to the reader.
 *
 * Every case here was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it, vi } from "vitest";

import { releaseOrphans } from "./orphans";

describe("releaseOrphans", () => {
  it("says nothing when there was nothing to release", async () => {
    // The ordinary case, on every single start. A line here would be printed
    // more often than any other line in the log, and would say that nothing
    // happened -- which is how a reader learns to skip past the one that matters.
    const note = vi.fn();
    await expect(releaseOrphans(async () => 0, note)).resolves.toBe(0);
    expect(note).not.toHaveBeenCalled();
  });

  it("says what it released when a previous page left something behind", async () => {
    // The whole point of the count coming back: nothing else in the running
    // system reports that a webview reloaded, and this is the moment it is
    // knowable.
    const note = vi.fn();
    await expect(releaseOrphans(async () => 3, note)).resolves.toBe(3);
    expect(note).toHaveBeenCalledTimes(1);
    expect(note.mock.calls[0]?.[0]).toContain("3 document(s)");
  });

  it("never rejects, so a start is not blocked by housekeeping", async () => {
    // The reader has just opened the application and is waiting for a page. A
    // rejection here would surface as an error about a thing they did not ask
    // for, in place of a leak they cannot see -- which is a worse trade, since
    // the documents stay held either way.
    const note = vi.fn();
    await expect(
      releaseOrphans(async () => {
        throw new Error("the render service is not answering");
      }, note),
    ).resolves.toBe(-1);
    expect(note).toHaveBeenCalledTimes(1);
    expect(note.mock.calls[0]?.[0]).toContain("not answering");
  });

  it("distinguishes a failure from a start with nothing to do", async () => {
    // `0` and `-1` are different facts and the reader of a transcript needs
    // both: one says the application started, the other says the question could
    // not be asked. Collapsing them would make a broken backend look like a
    // clean start, which is the reassuring direction.
    const quiet = vi.fn();
    const broken = vi.fn();
    const clean = await releaseOrphans(async () => 0, quiet);
    const failed = await releaseOrphans(async () => Promise.reject(new Error("x")), broken);
    expect(clean).not.toBe(failed);
    expect(quiet).not.toHaveBeenCalled();
    expect(broken).toHaveBeenCalledTimes(1);
  });
});
