import { describe, expect, it } from "vitest";

import { LOCK_KEY, LOCK_LABEL, readLock, writeLock } from "./signlock";

/** A storage double holding one value, or throwing. */
function holding(value: string | null, throws = false) {
  let kept = value;
  return () => {
    if (throws) throw new Error("no storage");
    return {
      getItem: (key: string) => (key === LOCK_KEY ? kept : null),
      setItem: (key: string, next: string) => {
        if (key === LOCK_KEY) kept = next;
      },
    };
  };
}

describe("whether a signing locks the document", () => {
  it("is a lock for a reader who never chose, and for storage that cannot be read", () => {
    expect(readLock(holding(null))).toBe(true);
    expect(readLock(holding(null, true))).toBe(true);
  });

  it("is no lock only for the exact value tpdf writes for one", () => {
    expect(readLock(holding("false"))).toBe(false);
    for (const other of ["true", "", "0", "no", "False", "null"]) {
      expect(readLock(holding(other)), other).toBe(true);
    }
  });

  it("reads back what was written, and says when storage refused", () => {
    const storage = holding(null);
    expect(writeLock(false, storage)).toBe(true);
    expect(readLock(storage)).toBe(false);
    expect(writeLock(true, storage)).toBe(true);
    expect(readLock(storage)).toBe(true);
    expect(writeLock(false, holding(null, true))).toBe(false);
  });

  it("says in the chooser what a lock does and when to untick it", () => {
    expect(LOCK_LABEL).toContain("Lock the document after signing");
    expect(LOCK_LABEL).toContain("a locked document takes no further signature");
  });
});
