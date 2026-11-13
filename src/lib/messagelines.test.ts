import { describe, expect, it } from "vitest";
import { announcement, joinedLine, messageLines, resultTone } from "./messagelines";
import { afterSigning, type Signed } from "./signing";

const intact = { verdict: "intact" as const, why: null, digest: "SHA-256", method: "RSA" };

/** A timestamped signing, with an earlier signature, as `afterSigning` words it. */
function sentence(): string {
  const signed: Signed = {
    path: "/docs/a-signed.pdf",
    field: "Signature2",
    signatures: [
      { field: "Signature1", integrity: intact, ours: false },
      {
        field: "Signature2",
        integrity: intact,
        ours: true,
        timestamp: {
          when: "2026-09-28 10:11:12 UTC",
          authority: null,
          integrity: intact,
          trust: null,
          attested: true,
          revocation: null,
          revocation_chain: null,
        },
      },
    ],
  };
  return afterSigning(signed);
}

describe("resultTone", () => {
  it("draws a result green only when it succeeded", () => {
    expect(resultTone(true)).toBe("success");
    expect(resultTone(false)).toBe("problem");
  });
});

describe("announcement", () => {
  it("interrupts for a problem and waits for a result or an instruction", () => {
    expect(announcement("The file is damaged.", "problem")).toEqual({ alert: "The file is damaged.", status: "" });
    expect(announcement("Saved.", "success")).toEqual({ alert: "", status: "Saved." });
    expect(announcement("Drag a rectangle.", "attention")).toEqual({ alert: "", status: "Drag a rectangle." });
  });

  it("empties both regions when the bar is dismissed", () => {
    expect(announcement(null, "problem")).toEqual({ alert: "", status: "" });
    expect(announcement(null, "success")).toEqual({ alert: "", status: "" });
  });
});

describe("messageLines", () => {
  it("leaves a one-line message as it is", () => {
    const one = "Save your changes first: a signature covers the file as it is on disk.";
    expect(messageLines(one)).toEqual([{ label: null, value: one, detail: null }]);
  });

  it("splits the sentence after signing into the lead and labelled rows", () => {
    const lines = messageLines(sentence());
    expect(lines.map((l) => l.label)).toEqual([null, "Timestamp", "Earlier signature"]);
    // The lead keeps its colon-free reading even though a later sentence of a
    // locked signing has one: only lines after the first carry a label.
    expect(lines[0]!.value).toBe(
      "Signed as Signature2 and saved to a-signed.pdf. Read back after writing, the signature is intact.",
    );
    expect(lines[1]!.value).toBe("2026-09-28 10:11:12 UTC, attested by an unnamed authority");
    expect(lines[1]!.detail).toMatch(/^the timestamp checks out/);
    expect(lines[2]).toEqual({ label: "Earlier signature", value: "Signature1 intact.", detail: null });
  });

  it("puts every line back together character for character", () => {
    // The window checks compare the bar's text with the sentence.
    const said = sentence();
    expect(messageLines(said).map(joinedLine).join("\n")).toBe(said);
    const odd = "Lead: with a colon\nno label here — but a dash\nA colon this far into a line belongs to a sentence: x";
    const lines = messageLines(odd);
    expect(lines.map(joinedLine).join("\n")).toBe(odd);
    expect(lines.map((l) => l.label)).toEqual([null, null, null]);
    expect(lines[1]!.detail).toBe("but a dash");
  });
});
