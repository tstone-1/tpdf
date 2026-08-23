import { describe, expect, it, vi } from "vitest";

import type { OpenRefusal } from "./ipc";
import { openWithPassword } from "./unlock";

/** A refusal as Tauri delivers one: a plain object, never an `Error`. */
function refusal(over: Partial<OpenRefusal> = {}): OpenRefusal {
  return { reason: "This document is locked, and needs a password.", locked: true, ...over };
}

/**
 * An `open` that refuses until it is handed `password`, then succeeds.
 *
 * The wording of the second refusal differs from the first, exactly as
 * `worker_child::unlock` words it: PDFium reports the same error either way, so
 * the sentence is the only thing that carries "you already tried one".
 */
function backend(password: string): {
  open: (given: string | undefined) => Promise<string>;
  tried: (string | undefined)[];
} {
  const tried: (string | undefined)[] = [];
  return {
    tried,
    open: (given) => {
      tried.push(given);
      if (given === password) return Promise.resolve("opened");
      return Promise.reject(
        refusal(
          given === undefined
            ? {}
            : { reason: "That password did not open this document." },
        ),
      );
    },
  };
}

describe("openWithPassword", () => {
  it("does not ask when the document opens", async () => {
    // Resolves `null` even though this test asserts it is never called: a
    // mutation that *does* reach it must then end the loop and fail an
    // assertion, rather than spinning forever on a refusal that never changes.
    // `vi.fn()` alone resolves `undefined`, which is neither a password nor a
    // decline, and the mutation proving this test hung the runner instead.
    const ask = vi.fn().mockResolvedValue(null);
    await expect(openWithPassword(() => Promise.resolve("opened"), ask)).resolves.toBe(
      "opened",
    );
    // The control for every test below: a dialog in front of an ordinary
    // document would be the most visible defect this module could have.
    expect(ask).not.toHaveBeenCalled();
  });

  it("asks once and opens with what was typed", async () => {
    const { open, tried } = backend("swordfish");
    const ask = vi.fn().mockResolvedValue("swordfish");
    await expect(openWithPassword(open, ask)).resolves.toBe("opened");
    expect(tried).toEqual([undefined, "swordfish"]);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it("asks again after a wrong password, showing the backend's second wording", async () => {
    const { open, tried } = backend("swordfish");
    const ask = vi
      .fn()
      .mockResolvedValueOnce("wrong")
      .mockResolvedValueOnce("also wrong")
      .mockResolvedValueOnce("swordfish");
    await expect(openWithPassword(open, ask)).resolves.toBe("opened");
    expect(tried).toEqual([undefined, "wrong", "also wrong", "swordfish"]);
    // The first prompt says the document is locked; every one after it says the
    // password did not open it. A loop that reused the first refusal's text
    // would tell a reader who just mistyped something they already knew.
    expect(ask.mock.calls.map((c) => c[0])).toEqual([
      "This document is locked, and needs a password.",
      "That password did not open this document.",
      "That password did not open this document.",
    ]);
  });

  it("rethrows the refusal when the reader declines", async () => {
    const { open, tried } = backend("swordfish");
    const ask = vi.fn().mockResolvedValue(null);
    // The refusal itself, not a sentinel: the caller's existing `catch` puts the
    // reader back where they were, and what it shows is true.
    await expect(openWithPassword(open, ask)).rejects.toEqual(refusal());
    expect(tried).toEqual([undefined]);
  });

  it("does not ask about a refusal that is not the answerable one", async () => {
    const broken = refusal({ reason: "This file is not a PDF.", locked: false });
    // Resolves `null` even though this test asserts it is never called: a
    // mutation that *does* reach it must then end the loop and fail an
    // assertion, rather than spinning forever on a refusal that never changes.
    // `vi.fn()` alone resolves `undefined`, which is neither a password nor a
    // decline, and the mutation proving this test hung the runner instead.
    const ask = vi.fn().mockResolvedValue(null);
    await expect(openWithPassword(() => Promise.reject(broken), ask)).rejects.toEqual(
      broken,
    );
    // A password dialog in front of a corrupt file asks a reader for something
    // that cannot help, and no answer ends it.
    expect(ask).not.toHaveBeenCalled();
  });

  it("does not ask about an ordinary error", async () => {
    const boom = new Error("the render thread stopped");
    // Resolves `null` even though this test asserts it is never called: a
    // mutation that *does* reach it must then end the loop and fail an
    // assertion, rather than spinning forever on a refusal that never changes.
    // `vi.fn()` alone resolves `undefined`, which is neither a password nor a
    // decline, and the mutation proving this test hung the runner instead.
    const ask = vi.fn().mockResolvedValue(null);
    await expect(openWithPassword(() => Promise.reject(boom), ask)).rejects.toBe(boom);
    expect(ask).not.toHaveBeenCalled();
  });

  it("rethrows rather than prompting when there is no dialog", async () => {
    const { open, tried } = backend("swordfish");
    // Every spike entry point opens documents before the shell is mounted. A
    // prompt there would hang a headless run at a dialog nobody can see.
    await expect(openWithPassword(open, null)).rejects.toEqual(refusal());
    expect(tried).toEqual([undefined]);
  });
});
