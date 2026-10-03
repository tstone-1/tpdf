import { describe, expect, it } from "vitest";

import {
  CHOICES,
  CUSTOM_START,
  DPI_MAX,
  DPI_MIN,
  afterCompress,
  caption,
  custom,
  isSmaller,
  outcome,
  savedPercent,
  size,
  suggestedName,
  summary,
  type SampleView,
  type Shrinkage,
} from "./compress";

const shrinkage = (before: number, after: number, extra: Partial<Shrinkage> = {}): Shrinkage => ({
  bytesBefore: before,
  bytesAfter: after,
  pictures: 0,
  picturesChanged: 0,
  sample: null,
  ...extra,
});

const sample: SampleView = {
  width: 320,
  height: 320,
  before: "data:image/png;base64,AAAA",
  after: "data:image/png;base64,BBBB",
  page: 3,
  zoomPercent: 200,
  dpiBefore: 520,
  dpiAfter: 110,
};

describe("the ways to make a copy smaller", () => {
  it("offers four ready choices, from losing nothing to the smallest", () => {
    expect(CHOICES.map((choice) => choice.id)).toEqual(["keep", "print", "balanced", "screen"]);
    expect(CHOICES[0]?.pictures).toBeNull();
    // The numbers are the backend's presets: 300/85, 150/75 and 110/60.
    expect(CHOICES.slice(1).map((choice) => choice.pictures)).toEqual([
      { dpi: 300, quality: 85, jpeg: true },
      { dpi: 150, quality: 75, jpeg: true },
      { dpi: 110, quality: 60, jpeg: true },
    ]);
    // Each says the resolution it leaves, in the reader's words.
    for (const choice of CHOICES.slice(1)) {
      expect(choice.note).toContain(`${choice.pictures?.dpi} pixels an inch`);
    }
    expect(CUSTOM_START).toEqual(CHOICES[2]?.pictures);
  });

  it("reads the reader's own numbers, and says what is wrong with them", () => {
    expect(custom("96", "40", false)).toEqual({ pictures: { dpi: 96, quality: 40, jpeg: false } });
    expect(custom(" 150 ", " 75 ", true)).toEqual({
      pictures: { dpi: 150, quality: 75, jpeg: true },
    });
    expect(custom(String(DPI_MIN), "1", true)).toHaveProperty("pictures");
    expect(custom(String(DPI_MAX), "100", true)).toHaveProperty("pictures");
    const resolution = { problem: "The resolution is a whole number from 20 to 1200." };
    for (const dpi of ["", "19", "1201", "150.5", "1e2", "-150", "abc"]) {
      expect(custom(dpi, "75", true), dpi).toEqual(resolution);
    }
    const quality = { problem: "The JPEG quality is a whole number from 1 to 100." };
    for (const q of ["", "0", "101", "7.5", "high"]) {
      expect(custom("150", q, true), q).toEqual(quality);
    }
  });
});

describe("what a smaller copy comes to, in words", () => {
  it("says a size the way a reader does", () => {
    expect(size(512)).toBe("512 bytes");
    expect(size(1024)).toBe("1.0 KB");
    expect(size(894_000)).toBe("873 KB");
    expect(size(9_122_611)).toBe("8.7 MB");
    expect(size(95_103_456)).toBe("91 MB");
    expect(size(-1)).toBe("unknown");
    expect(size(Number.NaN)).toBe("unknown");
  });

  it("rounds the share saved down and never below nothing", () => {
    expect(savedPercent(1000, 400)).toBe(60);
    expect(savedPercent(1000, 989)).toBe(1);
    // 1.5% is one percent, not two: the sentence never claims more than was saved.
    expect(savedPercent(1000, 985)).toBe(1);
    expect(savedPercent(1000, 999)).toBe(0);
    expect(savedPercent(1000, 1000)).toBe(0);
    expect(savedPercent(1000, 2000)).toBe(0);
    expect(savedPercent(0, 0)).toBe(0);
  });

  it("puts the size and the share beside a choice, or says it is not smaller", () => {
    expect(outcome(shrinkage(95_103_456, 4_094_847))).toBe("3.9 MB, 95% smaller");
    expect(outcome(shrinkage(1000, 1000))).toBe("Not smaller");
    expect(outcome(shrinkage(1000, 1200))).toBe("Not smaller");
    expect(isSmaller(shrinkage(1000, 999))).toBe(true);
    expect(isSmaller(shrinkage(1000, 1000))).toBe(false);
  });

  it("says under the choices where the size goes and how many pictures change", () => {
    expect(summary(shrinkage(95_103_456, 4_094_847, { pictures: 36, picturesChanged: 25 }))).toBe(
      "From 91 MB to about 3.9 MB. 25 of 36 pictures are stored smaller.",
    );
    expect(summary(shrinkage(2_000_000, 1_900_000, { pictures: 4 }))).toBe(
      "From 1.9 MB to about 1.8 MB. No picture changes.",
    );
    expect(summary(shrinkage(2_000_000, 1_900_000))).toBe("From 1.9 MB to about 1.8 MB.");
    expect(summary(shrinkage(2_000_000, 2_000_000))).toBe(
      "The document is 1.9 MB, and a copy made this way would not be smaller.",
    );
  });

  it("says what the two pictures are and how far the picture is reduced", () => {
    expect(caption(sample)).toBe(
      "Part of page 3 at 200%: now on the left, the copy on the right. " +
        "Its largest picture goes from 520 to 110 pixels an inch.",
    );
    // A picture only stored differently has no resolution to report.
    const same = "Part of page 3 at 200%: now on the left, the copy on the right.";
    expect(caption({ ...sample, dpiAfter: 520 })).toBe(same);
    expect(caption({ ...sample, dpiBefore: 0, dpiAfter: 0 })).toBe(same);
  });

  it("suggests a name and says the size once the copy is written", () => {
    expect(suggestedName("/home/me/Report.PDF")).toBe("Report smaller.pdf");
    expect(suggestedName("notes")).toBe("notes smaller.pdf");
    expect(afterCompress({}, "/out/Report smaller.pdf", shrinkage(95_103_456, 4_094_847))).toBe(
      "Saved Report smaller.pdf, about 3.9 MB where the document is 91 MB.",
    );
    expect(afterCompress({}, "/out/a.pdf", null)).toBe("Saved a.pdf.");
    expect(afterCompress({ changed: true }, "/out/a.pdf", null)).toMatch(
      /^Saved a\.pdf\. The file was written, but the original changed on disk/,
    );
  });
});
