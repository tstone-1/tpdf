import { describe, expect, it } from "vitest";

import {
  COMPUTER,
  DOUBT,
  integrityRow,
  REVOCATION_NOT_CHECKED,
  TRUST_NOT_CHECKED,
  trustRow,
  WHY,
  type Doubt,
  type Integrity,
  type Standing,
  type Store,
  type Trust,
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
      trust: null,
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

function standing(s: Standing, why: Doubt | null = null, store: Store | null = "mac"): Trust {
  return { standing: s, why, store };
}

const EVERY_DOUBT = Object.keys(DOUBT) as Doubt[];

/** Every trust row this module can render, on both stores. */
function everyTrustRow() {
  const rows = (["mac", "windows"] as const).flatMap((store) => [
    trustRow(standing("trusted", null, store)),
    trustRow(standing("expired", null, store), "2020-01-01", "2021-01-01"),
    trustRow(standing("not_yet_valid", null, store), "the first day", "the last day"),
    ...EVERY_DOUBT.map((why) => trustRow(standing("untrusted", why, store))),
    ...EVERY_DOUBT.map((why) => trustRow(standing("unchecked", why, null))),
  ]);
  return rows.map((row) => {
    if (!row) throw new Error("every standing renders a row");
    return row;
  });
}

describe("trustRow", () => {
  it("leads each standing with its own word, so none reads as another", () => {
    const lead = (t: Trust) => trustRow(t)?.value.split(" — ")[0];
    expect(lead(standing("trusted"))).toBe("trusted");
    expect(lead(standing("expired"))).toBe("expired");
    expect(lead(standing("not_yet_valid"))).toBe("not yet in force");
    expect(lead(standing("untrusted", "root"))).toBe("not trusted");
    expect(lead(standing("unchecked", "unavailable", null))).toBe("not checked");
  });

  it("names the store that answered", () => {
    // Trust is relative to a store, and a reader whose colleague sees
    // "trusted" in Acrobat has to be able to tell why this one says otherwise.
    for (const store of ["mac", "windows"] as const) {
      for (const s of ["trusted", "expired", "not_yet_valid"] as const) {
        expect(trustRow(standing(s, null, store))?.value).toContain(
          `a root ${COMPUTER[store]} trusts`,
        );
      }
      expect(trustRow(standing("untrusted", "root", store))?.value).toContain(
        `a root ${COMPUTER[store]} does not trust`,
      );
    }
    expect(COMPUTER.mac).toBe("this Mac");
    expect(COMPUTER.windows).toBe("this PC");
  });

  it("says revocation was not checked wherever the chain reached a trusted root", () => {
    for (const s of ["trusted", "expired", "not_yet_valid"] as const) {
      expect(trustRow(standing(s))?.value).toContain(REVOCATION_NOT_CHECKED);
    }
  });

  it("says an expired certificate may have been in force when used, and cannot tell", () => {
    const value = trustRow(standing("expired"), "2020-01-01", "2021-06-30 UTC")?.value ?? "";
    expect(value).toContain("ran out on 2021-06-30 UTC");
    expect(value).toContain("cannot tell whether it was made before then");
    expect(value).not.toMatch(/^trusted/);
    expect(value).not.toMatch(/^not trusted/);
  });

  it("says a root only Adobe's list carries reads as untrusted here", () => {
    expect(trustRow(standing("untrusted", "root"))?.value).toContain("Adobe's trust list");
  });

  it("gives each doubt its own sentence", () => {
    const sentences = new Set(EVERY_DOUBT.map((why) => DOUBT[why]("this Mac")));
    expect(sentences.size).toBe(EVERY_DOUBT.length);
    for (const why of EVERY_DOUBT) {
      expect(trustRow(standing("untrusted", why))?.value).toContain(DOUBT[why]("this Mac"));
    }
  });

  it("marks everything but a trusted chain for a reader's attention", () => {
    expect(trustRow(standing("trusted"))?.warn).toBeUndefined();
    const rest = everyTrustRow().filter((row) => !row.value.startsWith("trusted"));
    expect(rest.length).toBeGreaterThan(0);
    for (const row of rest) expect(row.warn).toBe(true);
  });

  it("renders nothing when no standing was given", () => {
    expect(trustRow(null)).toBeNull();
  });

  it("puts no verdict word about the signer into any standing", () => {
    for (const row of everyTrustRow()) {
      const text = `${row.name} ${row.value}`.toLowerCase();
      for (const word of VERDICT_WORDS) {
        expect(text).not.toContain(word);
      }
    }
  });
});

describe("the trust row among the others", () => {
  function signature(integrity: Integrity, trust: Trust | null): Signature {
    return {
      field: "Signature1",
      signed: true,
      handler: "Adobe.PPKLite",
      kind: "ETSI.CAdES.detached",
      name: "",
      reason: "",
      location: "",
      when: "",
      covers_whole_file: true,
      covered_bytes: 1000,
      appended_bytes: 0,
      appendix: null,
      certification: 0,
      certificate: {
        subject: "CN=A. Signer",
        subject_cn: "A. Signer",
        issuer: "CN=Issuer",
        issuer_cn: "Issuer",
        serial: "01",
        from: "2024-01-01 00:00:00 UTC",
        until: "2025-01-01 00:00:00 UTC",
        self_issued: false,
        chain: 2,
        matched_signer: true,
        key_usage: null,
        extended_usage: null,
        authority: false,
        extensions_unread: 0,
      },
      timestamp: null,
      integrity,
      trust,
    };
  }

  it("follows the integrity row, and replaces its not-checked sentence", () => {
    const rows = signatureRows(signature(verdict("intact"), standing("trusted")), 1000);
    expect(rows.map((row) => row.name).slice(0, 2)).toEqual(["Integrity", "Trust"]);
    // The sentence saying ownership was not checked would now contradict the
    // row under it, which checked.
    expect(rows[0]?.value).not.toContain(TRUST_NOT_CHECKED);
  });

  it("keeps the not-checked sentence when there is no standing to show", () => {
    const rows = signatureRows(signature(verdict("intact"), null), 1000);
    expect(rows.some((row) => row.name === "Trust")).toBe(false);
    expect(rows[0]?.value).toContain(TRUST_NOT_CHECKED);
  });

  it("dates an expired standing from the certificate the dialog shows", () => {
    const rows = signatureRows(signature(verdict("intact"), standing("expired")), 1000);
    const row = rows.find((r) => r.name === "Trust");
    expect(row?.value).toContain("ran out on 2025-01-01 00:00:00 UTC");
  });
});
