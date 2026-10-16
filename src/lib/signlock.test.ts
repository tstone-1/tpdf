import { describe, expect, it } from "vitest";

import { LOCK_ANSWERS, LOCK_QUESTION, LOCK_UNANSWERED, lockOf } from "./signlock";

describe("whether a signing locks the document", () => {
  it("locks only for the reader who says nobody else signs", () => {
    expect(lockOf("last")).toBe(true);
    expect(lockOf("others")).toBe(false);
  });

  it("has no answer while none is chosen or the value is not one of ours", () => {
    for (const other of [undefined, "", "true", "false", "Last", "lock"]) {
      expect(lockOf(other), String(other)).toBeNull();
    }
  });

  it("asks about the people, and says in the answers what a lock does", () => {
    expect(LOCK_QUESTION).toBe("Does anyone else still need to sign this document?");
    expect(LOCK_ANSWERS.map((one) => one.value)).toEqual(["others", "last"]);
    const last = LOCK_ANSWERS.find((one) => one.lock)!;
    expect(last.text).toContain("I am the last to sign");
    expect(last.hint).toContain("no further signature can be added");
    expect(LOCK_ANSWERS.find((one) => !one.lock)!.text).toContain("leave it open for their signatures");
    expect(LOCK_UNANSWERED).toContain("a locked document takes no further signature");
  });
});
