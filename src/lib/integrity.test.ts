import { describe, expect, it } from "vitest";

import {
  integrityRow,
  TRUST_NOT_CHECKED,
  WHY,
  type Integrity,
  type Verdict,
  type Why,
} from "./integrity";
import { signatureRows, VERDICT_WORDS, type Signature } from "./properties";

function verdict(v: Verdict, why: Why | null = null): Integrity {
  return { verdict: v, why, digest: "SHA-256", method: "ECDSA P-256" };
}

const EVERY_WHY = Object.keys(WHY) as Why[];

/** Every row this module can render, for the assertions that range over all. */
function everyRow() {
  const rows = [
    ...(["intact", "weak", "altered", "broken"] as const).flatMap((v) => [
      integrityRow(verdict(v), 0),
      integrityRow(verdict(v), 9_101),
    ]),
    ...EVERY_WHY.map((why) => integrityRow(verdict("unchecked", why), 0)),
  ];
  return rows.map((row) => {
    if (!row) throw new Error("every verdict renders a row");
    return row;
  });
}

describe("integrityRow", () => {
  it("leads each answer with its own word, so none reads as another", () => {
    const lead = (v: Verdict, why: Why | null = null) =>
      integrityRow(verdict(v, why), 0)?.value.split(" — ")[0];
    expect(lead("intact")).toBe("intact");
    expect(lead("weak")).toBe("unchanged under SHA-1 only");
    expect(lead("altered")).toBe("altered");
    expect(lead("broken")).toBe("broken");
    expect(lead("unchecked", "range")).toBe("not checked");
  });

  it("says after every answer that could mean trust that trust was not checked", () => {
    for (const v of ["intact", "weak"] as const) {
      expect(integrityRow(verdict(v), 0)?.value).toContain(TRUST_NOT_CHECKED);
      expect(integrityRow(verdict(v), 5)?.value).toContain(TRUST_NOT_CHECKED);
    }
  });

  it("never calls a SHA-1 match intact", () => {
    const value = integrityRow({ ...verdict("weak"), digest: "SHA-1" }, 0)?.value ?? "";
    expect(value).not.toMatch(/\bintact\b/);
    expect(value).toContain("SHA-1");
  });

  it("marks everything but an intact signature for a reader's attention", () => {
    expect(integrityRow(verdict("intact"), 0)?.warn).toBeUndefined();
    for (const v of ["weak", "altered", "broken"] as const) {
      expect(integrityRow(verdict(v), 0)?.warn).toBe(true);
    }
    for (const why of EVERY_WHY) {
      expect(integrityRow(verdict("unchecked", why), 0)?.warn).toBe(true);
    }
  });

  it("gives each reason its own sentence, and says an unchecked one means nothing", () => {
    const sentences = new Set(Object.values(WHY));
    expect(sentences.size).toBe(EVERY_WHY.length);
    for (const why of EVERY_WHY) {
      const value = integrityRow(verdict("unchecked", why), 0)?.value ?? "";
      expect(value).toContain(WHY[why]);
      expect(value).toContain("says nothing either way");
    }
  });

  it("says an intact signature does not cover what was appended after it", () => {
    // A signature can be intact AND have a later revision --- the ordinary
    // shape of a document signed twice, or given validation data. Both are
    // stated; the reader is not left to reconcile this row with Covers.
    const appended = integrityRow(verdict("intact"), 9_101)?.value ?? "";
    expect(appended).toContain("what was appended afterwards is not part of it");
    const whole = integrityRow(verdict("intact"), 0)?.value ?? "";
    expect(whole).not.toContain("appended");
  });

  it("names the digest and the method it checked with", () => {
    expect(integrityRow(verdict("intact"), 0)?.value).toContain("(SHA-256, ECDSA P-256)");
    // A refusal that stopped before choosing either names neither.
    const bare = integrityRow({ verdict: "unchecked", why: "format", digest: "", method: "" }, 0);
    expect(bare?.value).not.toContain("()");
  });

  it("renders nothing for a field nobody signed", () => {
    expect(integrityRow(null, 0)).toBeNull();
  });

  it("puts no verdict word about the signer into any answer", () => {
    for (const row of everyRow()) {
      const text = `${row.name} ${row.value}`.toLowerCase();
      for (const word of VERDICT_WORDS) {
        expect(text).not.toContain(word);
      }
    }
  });
});

describe("the integrity row among the others", () => {
  it("comes first, above who the signature says signed it", () => {
    const signature: Signature = {
      field: "Signature1",
      signed: true,
      handler: "Adobe.PPKLite",
      kind: "adbe.pkcs7.detached",
      name: "A. Signer",
      reason: "",
      location: "",
      when: "",
      covers_whole_file: false,
      covered_bytes: 1000,
      appended_bytes: 24,
      appendix: null,
      certification: 0,
      certificate: null,
      timestamp: null,
      integrity: verdict("altered"),
    };
    const rows = signatureRows(signature, 1024);
    expect(rows[0]?.name).toBe("Integrity");
    expect(rows[0]?.value.startsWith("altered")).toBe(true);
    // And the appended fact is still stated beside it, in its own row.
    expect(rows.some((row) => row.name === "Covers" && row.value.includes("appended"))).toBe(
      true,
    );
  });
});
