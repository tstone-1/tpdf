import { describe, expect, it } from "vitest";

import {
  MAX_BYTES,
  afterProtect,
  beyondAscii,
  judge,
  suggestedName,
} from "./protect";

describe("judge", () => {
  it("accepts a password typed the same way twice", () => {
    expect(judge("tr0ub4dor", "tr0ub4dor")).toBeNull();
    expect(judge("pässword", "pässword")).toBeNull();
  });

  it("refuses an empty one before comparing anything", () => {
    expect(judge("", "")).toBe("Type a password.");
    expect(judge("", "x")).toBe("Type a password.");
  });

  it("refuses two that differ, by case as well", () => {
    expect(judge("tr0ub4dor", "tr0ub4dor ")).toMatch(/not the same/);
    expect(judge("Secret", "secret")).toMatch(/not the same/);
  });

  it("counts the length in bytes, which is what the file format reads", () => {
    const fits = "x".repeat(MAX_BYTES);
    expect(judge(fits, fits)).toBeNull();
    const over = "x".repeat(MAX_BYTES + 1);
    expect(judge(over, over)).toMatch(/at most 127 bytes, and this one is 128/);
    // 64 characters, 128 bytes: short by count and too long by what is written.
    const wide = "ä".repeat(64);
    expect(judge(wide, wide)).toMatch(/this one is 128/);
    expect(judge("ä".repeat(63), "ä".repeat(63))).toBeNull();
  });

  it.each(["a\tb", "a\nb", "a\u007fb", "a\u0085b"])(
    "refuses a character nobody can type back: %j",
    (password) => {
      expect(judge(password, password)).toMatch(/cannot be typed back/);
    },
  );
});

describe("beyondAscii", () => {
  it("is about characters Preview does not accept, not about length", () => {
    expect(beyondAscii("tr0ub4dor-!~ ")).toBe(false);
    expect(beyondAscii("pässword")).toBe(true);
    expect(beyondAscii("密码")).toBe(true);
  });
});

describe("suggestedName", () => {
  it("names the copy after what was done to it", () => {
    expect(suggestedName("/home/me/Report.PDF", true)).toBe("Report protected.pdf");
    expect(suggestedName("/home/me/report.pdf", false)).toBe("report unprotected.pdf");
  });
});

describe("afterProtect", () => {
  it("says whether the copy needs a password, by its name", () => {
    expect(afterProtect({ changed: false }, "/out/a protected.pdf", true)).toBe(
      "Saved a protected.pdf. It needs the new password to open.",
    );
    expect(afterProtect({}, "/out/a unprotected.pdf", false)).toBe(
      "Saved a unprotected.pdf. It opens without a password.",
    );
  });

  it("adds what a copy from a changed source is told", () => {
    const said = afterProtect({ changed: true }, "/out/a.pdf", true);
    expect(said).toMatch(/^Saved a\.pdf\. It needs the new password to open\. The file was written, but/);
  });
});
