import { describe, expect, it } from "vitest";

import {
  authorityRow,
  chainRow,
  COMPUTER,
  DOUBT,
  integrityRow,
  GAP,
  REASON,
  revocationRow,
  TRUST_NOT_CHECKED,
  trustRow,
  WHY,
  type Basis,
  type Chain,
  type Doubt,
  type Gap,
  type Reason,
  type Revocation,
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
      revocation: null,
      revocation_chain: null,
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

function standing(
  s: Standing,
  why: Doubt | null = null,
  store: Store | null = "mac",
  attested_at = "",
): Trust {
  return { standing: s, why, store, attested_at };
}

const EVERY_DOUBT = Object.keys(DOUBT) as Doubt[];

/** Every trust row this module can render, on both stores. */
function everyTrustRow() {
  const rows = (["mac", "windows"] as const).flatMap((store) => [
    trustRow(standing("trusted", null, store)),
    trustRow(standing("trusted_at_timestamp", null, store, "2026-08-21 12:00:00 UTC")),
    trustRow(standing("expired", null, store), "2020-01-01", "2021-01-01"),
    trustRow(standing("not_yet_valid", null, store), "the first day", "the last day"),
    ...EVERY_DOUBT.map((why) => trustRow(standing("untrusted", why, store))),
    ...EVERY_DOUBT.map((why) =>
      trustRow(standing("untrusted", why, store, "2026-08-21 12:00:00 UTC")),
    ),
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
    expect(lead(standing("trusted_at_timestamp", null, "mac", "then"))).toBe(
      "trusted at the timestamp",
    );
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

  it("leaves revocation to its own row, and no longer speaks for it", () => {
    // Since 2026-09-28 revocation is a row of its own, which says exactly
    // what the document's data did and did not establish.
    for (const row of everyTrustRow()) {
      expect(row.value.toLowerCase()).not.toContain("revocation");
      expect(row.value.toLowerCase()).not.toContain("revoked");
    }
  });

  it("says whose moment an attested judgement is, and that expiry since does not undo it", () => {
    const at = "2026-08-21 12:00:00 UTC";
    const row = trustRow(standing("trusted_at_timestamp", null, "mac", at));
    expect(row?.value.split(" — ")[0]).toBe("trusted at the timestamp");
    expect(row?.value).toContain(`at ${at}, the time a timestamp from an authority this Mac`);
    expect(row?.value).toContain("run out since does not change that");
    expect(row?.warn).toBeUndefined();
    // An untrusted chain judged at that moment says so; one judged now does not.
    expect(trustRow(standing("untrusted", "root", "mac", at))?.value).toContain(
      `judged at ${at}, the time the timestamp attests`,
    );
    expect(trustRow(standing("untrusted", "root"))?.value).not.toContain("judged at");
    expect(trustRow(standing("untrusted", "not_in_force", "mac", at))?.value).toContain(
      "the signer's certificate was not in force then",
    );
  });

  it("states the attested moment once, as the whole verdict's and not the reason's", () => {
    // `tpdf verify`, 2026-09-28: after the reason, "judged at ..." read as the
    // last clause of the Adobe sentence rather than as the verdict's moment.
    const at = "2026-08-21 12:00:00 UTC";
    const moment = `judged at ${at}, the time the timestamp attests`;
    for (const why of EVERY_DOUBT) {
      const value = trustRow(standing("untrusted", why, "mac", at))?.value ?? "";
      const [lead, ...rest] = value.split(" — ");
      expect(lead).toBe(`not trusted, ${moment}`);
      expect(rest.join(" — ")).not.toContain(at);
      expect(rest.join(" — ")).not.toContain("the time the timestamp attests");
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
    expect(trustRow(standing("trusted_at_timestamp", null, "mac", "then"))?.warn).toBeUndefined();
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
  function signature(
    integrity: Integrity,
    trust: Trust | null,
    revocation: Revocation | null = null,
    kind = "ETSI.CAdES.detached",
  ): Signature {
    return {
      field: "Signature1",
      signed: true,
      handler: "Adobe.PPKLite",
      kind,
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
      revocation,
      revocation_chain: null,
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

  it("puts the revocation row under the trust row, the authority's for a document timestamp", () => {
    const none = revocation("none");
    const rows = signatureRows(signature(verdict("intact"), standing("trusted"), none), 1000);
    expect(rows.map((row) => row.name).slice(0, 3)).toEqual([
      "Integrity",
      "Trust",
      "Revocation",
    ]);
    const stamped = signatureRows(
      signature(verdict("intact"), standing("trusted"), none, "ETSI.RFC3161"),
      1000,
    );
    expect(stamped[2]?.name).toBe("Authority revocation");
    expect(stamped[2]?.value).toContain("the authority's certificate");
  });

  it("dates an expired standing from the certificate the dialog shows", () => {
    const rows = signatureRows(signature(verdict("intact"), standing("expired")), 1000);
    const row = rows.find((r) => r.name === "Trust");
    expect(row?.value).toContain("ran out on 2025-01-01 00:00:00 UTC");
  });
});

const EVERY_GAP = Object.keys(GAP) as Gap[];
const EVERY_REASON = Object.keys(REASON) as Reason[];
const EVERY_BASIS: Basis[] = ["attested", "stated", "claimed", "now"];

function revocation(
  standing: Revocation["standing"],
  more: Partial<Revocation> = {},
): Revocation {
  return {
    standing,
    why: null,
    source: standing === "good" || standing === "revoked" || standing === "unknown" ? "ocsp" : null,
    issued: standing === "none" || standing === "unchecked" ? "" : "2026-08-20 09:00:00 UTC",
    next: standing === "none" || standing === "unchecked" ? "" : "2026-08-27 09:00:00 UTC",
    revoked: standing === "revoked" ? "2026-08-01 00:00:00 UTC" : "",
    reason: null,
    basis: "attested",
    moment: "2026-08-21 12:00:00 UTC",
    after_moment: false,
    ...more,
  };
}

/** Every revocation row this module can render, for both certificates. */
function everyRevocationRow() {
  const shapes: Revocation[] = EVERY_BASIS.flatMap((basis) => [
    revocation("none", { basis }),
    revocation("good", { basis }),
    revocation("good", { basis, source: "crl", next: "" }),
    revocation("unknown", { basis }),
    ...EVERY_GAP.map((why) => revocation("unchecked", { basis, why })),
    ...EVERY_REASON.flatMap((reason) => [
      revocation("revoked", { basis, reason }),
      revocation("revoked", { basis, reason, after_moment: true, source: "crl" }),
    ]),
  ]);
  return shapes.flatMap((shape) =>
    [false, true].map((authority) => {
      const row = revocationRow(shape, authority);
      if (!row) throw new Error("every answer renders a row");
      return { shape, row };
    }),
  );
}

describe("revocationRow", () => {
  it("says a document carrying no revocation data was not checked, never that it is fine", () => {
    for (const authority of [false, true]) {
      const row = revocationRow(revocation("none"), authority);
      expect(row?.value.split(" — ")[0]).toBe("not checked");
      expect(row?.value).toContain("carries no revocation data");
      expect(row?.value).toContain("does not fetch any");
      expect(row?.value).not.toContain("not revoked");
      expect(row?.warn).toBe(true);
    }
  });

  it("leads each answer with its own words, so none reads as another", () => {
    const lead = (r: Revocation) => revocationRow(r)?.value.split(" — ")[0];
    expect(lead(revocation("good"))).toBe("not revoked");
    expect(lead(revocation("revoked"))).toBe("revoked");
    expect(lead(revocation("revoked", { after_moment: true }))).toBe("revoked after the timestamp");
    expect(lead(revocation("unknown"))).toBe("unknown");
    expect(lead(revocation("unchecked", { why: "stale" }))).toBe("not checked");
    expect(lead(revocation("none"))).toBe("not checked");
  });

  it("says a revocation after an attested moment does not undo the signature, and only then", () => {
    const after = revocationRow(revocation("revoked", { after_moment: true }))?.value ?? "";
    expect(after).toContain("does not undo the signature");
    const before = revocationRow(revocation("revoked"))?.value ?? "";
    expect(before).toContain("already withdrawn when the signature was made");
    for (const basis of ["stated", "claimed", "now"] as const) {
      const value = revocationRow(revocation("revoked", { basis }))?.value ?? "";
      expect(value, basis).toContain("cannot tell whether that was before then");
    }
  });

  it("names whose clock the moment is", () => {
    const at = "2026-08-21 12:00:00 UTC";
    const said = (basis: Basis) => revocationRow(revocation("good", { basis }))?.value ?? "";
    expect(said("attested")).toContain(`${at}, the time the timestamp attests`);
    expect(said("stated")).toContain(`${at}, the time the timestamp states`);
    expect(said("claimed")).toContain("which is their own claim");
    expect(said("now")).toContain("the present moment");
  });

  it("gives each gap and each reason its own words", () => {
    const gaps = new Set(EVERY_GAP.map((why) => GAP[why]("then")));
    expect(gaps.size).toBe(EVERY_GAP.length);
    const reasons = new Set(Object.values(REASON));
    expect(reasons.size).toBe(EVERY_REASON.length);
    for (const reason of EVERY_REASON) {
      expect(revocationRow(revocation("revoked", { reason }))?.value).toContain(
        `, for ${REASON[reason]}`,
      );
    }
  });

  it("marks everything but a good answer and a later revocation for a reader's attention", () => {
    for (const { shape, row } of everyRevocationRow()) {
      const calm =
        shape.standing === "good" || (shape.standing === "revoked" && shape.after_moment);
      expect(row.warn, JSON.stringify(shape)).toBe(calm ? undefined : true);
    }
  });

  it("puts no verdict word into any answer", () => {
    for (const { row } of everyRevocationRow()) {
      const text = `${row.name} ${row.value}`.toLowerCase();
      for (const word of VERDICT_WORDS) {
        expect(text).not.toContain(word);
      }
    }
  });

  it("renders nothing when there is no answer", () => {
    expect(revocationRow(null)).toBeNull();
  });
});

describe("chainRow", () => {
  /** The signer's chain: the leaf's answer, then each above it, combined as `revocation::chain::combine` would. */
  function chain(
    answers: Revocation[],
    decided_by: number | null,
    more: Partial<Chain> = {},
  ): Chain {
    const certificates = answers.map((r, at) => ({
      subject: at === 0 ? "CN=A. Signer" : `CN=Issuing CA ${at}`,
      subject_cn: at === 0 ? "A. Signer" : `Issuing CA ${at}`,
      serial: "07",
      revocation: r,
    }));
    const decider = decided_by === null ? null : answers[decided_by];
    return {
      certificates,
      standing: decider?.standing ?? "good",
      after_moment: decider?.after_moment ?? false,
      decided_by,
      dropped: 0,
      end: "root",
      ...more,
    };
  }

  it("is not shown when nothing stands above the leaf, which its own row then says", () => {
    expect(chainRow(null)).toBeNull();
    expect(chainRow(chain([revocation("good")], null))).toBeNull();
    expect(chainRow(chain([revocation("none")], 0, { end: "no_issuer" }))).toBeNull();
    // The control: one certificate above it, and the row is there.
    expect(chainRow(chain([revocation("good"), revocation("good")], null))).not.toBeNull();
  });

  it("names the issuing certificate a revocation is about, and warns", () => {
    for (const authority of [false, true]) {
      const row = chainRow(chain([revocation("good"), revocation("revoked")], 1), authority);
      expect(row?.name).toBe(authority ? "Authority chain revocation" : "Chain revocation");
      expect(row?.value).toContain("says the issuing certificate Issuing CA 1 was revoked on");
      expect(row?.value).toContain("already withdrawn when");
      expect(row?.warn).toBe(true);
    }
  });

  it("never calls a chain with a certificate nothing was said about good", () => {
    const row = chainRow(chain([revocation("good"), revocation("none")], 1));
    expect(row?.value.split(" — ")[0]).toBe("not checked");
    expect(row?.value).toContain("no revocation data for the issuing certificate Issuing CA 1");
    expect(row?.warn).toBe(true);
  });

  it("says a whole good chain is good, calmly, and names its top", () => {
    const row = chainRow(
      chain([revocation("good"), revocation("good"), revocation("good")], null),
    );
    expect(row?.value).toContain(
      "none of the 3 certificates from the signer's certificate up to the issuing certificate Issuing CA 2",
    );
    expect(row?.warn).toBeUndefined();
  });

  it("says a chain past the bound was not followed, with no certificate named", () => {
    const row = chainRow(
      chain([revocation("good"), revocation("good")], null, { standing: "unchecked", dropped: 2 }),
    );
    expect(row?.value).toContain("longer than the 8 certificates tpdf follows");
    expect(row?.value).toContain("2 certificates further up were not judged");
    expect(row?.warn).toBe(true);
  });

  it("points back at the leaf's own row when the leaf decides", () => {
    const row = chainRow(chain([revocation("unknown"), revocation("good")], 0));
    expect(row?.value).toBe(
      "unknown — decided by the signer's certificate itself, in the row above; nothing about " +
        "the certificate above it reads worse.",
    );
    expect(row?.warn).toBe(true);
  });

  it("lets a revocation after the attested moment stand calmly, as the leaf's row does", () => {
    const row = chainRow(
      chain([revocation("good"), revocation("revoked", { after_moment: true })], 1),
    );
    expect(row?.value).toContain("does not undo the signature");
    expect(row?.warn).toBeUndefined();
  });
});

describe("authorityRow", () => {
  const archive = "the time an archive timestamp later in this document attests";

  it("states the archive's moment once, beside the verdict's word", () => {
    const at = "2026-08-21 12:00:00 UTC";
    const trusted = authorityRow(standing("trusted_at_timestamp", null, "mac", at))?.value ?? "";
    expect(trusted.split(" — ")[0]).toBe(`trusted, judged at ${at}, ${archive}`);
    expect(trusted.split(at).length).toBe(2);
    for (const why of EVERY_DOUBT) {
      const value = authorityRow(standing("untrusted", why, "mac", at))?.value ?? "";
      const [lead, ...rest] = value.split(" — ");
      expect(lead).toBe(`not trusted, judged at ${at}, ${archive}`);
      expect(rest.join(" — ")).not.toContain(at);
    }
    // Judged now, it names no moment.
    expect(authorityRow(standing("untrusted", "root"))?.value).not.toContain("judged at");
  });

  it("says an authority out of its dates is the authority's certificate, not the signer's", () => {
    const value =
      authorityRow(standing("untrusted", "not_in_force", "mac", "2026-08-21 12:00:00 UTC"))
        ?.value ?? "";
    expect(value).toContain("the authority's certificate was not in force then");
    expect(value).not.toContain("signer");
  });
});
