import { describe, expect, it } from "vitest";
import { messageLines } from "./messagelines";
import { afterRasterRedaction, afterRedaction, afterRedactionCopy } from "./recovery";
import { reviewGroups, reviewLines } from "./redactreview";

const why = [
  "page 1: object 7 is an image",
  "Smith is still in the file, on page 4",
  "the file is encrypted",
  "page 2 has a QR code that still carries \"Smith\", which the redaction removed; redact the code too",
];
const kinds = ["pictures", "found", "unchecked", "found"];

describe("reviewGroups", () => {
  it("groups the reasons by kind, what the reader can fix first, in arrival order within a kind", () => {
    expect(reviewGroups(why, kinds)).toEqual([
      { kind: "found", reasons: [why[1], why[3]] },
      { kind: "pictures", reasons: [why[0]] },
      { kind: "unchecked", reasons: [why[2]] },
    ]);
  });

  it("leaves out a kind no reason has", () => {
    expect(reviewGroups([why[2]!], ["unchecked"])).toEqual([{ kind: "unchecked", reasons: [why[2]] }]);
  });

  it("answers null for kinds that do not pair one for one with the reasons", () => {
    expect(reviewGroups(why, undefined)).toBeNull();
    expect(reviewGroups(why, kinds.slice(1))).toBeNull();
    expect(reviewGroups(why, [...kinds.slice(0, 3), "elsewhere"])).toBeNull();
    expect(reviewGroups([], [])).toBeNull();
  });
});

describe("reviewLines", () => {
  it("is one labelled line a group, the advice before the reasons", () => {
    const lines = reviewLines(reviewGroups(why, kinds)!, "en-US");
    expect(lines).toHaveLength(3);
    const laid = messageLines(["head", ...lines].join("\n"));
    expect(laid.map((line) => line.label)).toEqual([
      null, "Still in the file", "Not read in pictures", "Could not be checked",
    ]);
    expect(laid[1]!.value).toBe("mark it where named and redact the original again");
    expect(laid[1]!.detail).toBe(`${why[1]}; ${why[3]}`);
  });

  it("keeps a reason with a dash in it whole", () => {
    const dashed = "Seite 1 — ein Bild";
    const [line] = reviewLines([{ kind: "pictures", reasons: [dashed] }], "de-DE");
    expect(messageLines(`head\n${line}`)[1]!.detail).toBe(dashed);
  });

  it("speaks formally in the formal locale", () => {
    const [line] = reviewLines([{ kind: "found", reasons: ["x"] }], "de-DE-formal");
    expect(line).toContain("markieren Sie");
  });
});

describe("the verdict sentences", () => {
  const applied = { regions: 2, shows: 3, verified: false, why, kinds, changed: false };

  it("lay an unverified redaction out by group", () => {
    const said = afterRedaction(applied);
    const laid = messageLines(said);
    expect(laid[0]!.value).toMatch(/^Redaction not verified\. Redacted 2 regions, 3 removals/);
    expect(laid.slice(1).map((line) => line.label)).toEqual([
      "Still in the file", "Not read in pictures", "Could not be checked",
    ]);
    for (const reason of why) expect(said).toContain(reason);
  });

  it("put a changed original on the first line", () => {
    const said = afterRedaction({ ...applied, changed: true });
    expect(said.split("\n")[0]).toContain("The original also changed on disk");
  });

  it("say what they always said for a reply without kinds", () => {
    const { kinds: _, ...without } = applied;
    const said = afterRedaction(without);
    expect(said).not.toContain("\n");
    expect(said).toContain(`Checks before adding the fill found: ${why.join("; ")}.`);
  });

  it("still offer the image-only copy beside the groups", () => {
    expect(afterRedactionCopy(applied).offers).toEqual(["rasterCopy"]);
  });

  it("lay the image-only copy's reasons out by group too", () => {
    const made = afterRasterRedaction({ regions: 1, verified: false, why, kinds }, "out.pdf");
    expect(made.success).toBe(false);
    const laid = messageLines(made.message);
    expect(laid[0]!.value).toMatch(/^Saved out\.pdf\..*The image-only copy is not verified/);
    expect(laid).toHaveLength(4);
  });
});
