/**
 * The command-line tool says what the application says, word for word.
 *
 * `tpdf verify` and `tpdf sign` print the properties dialog's Integrity and
 * Trust rows and the signing panel's closing sentence, and `tpdf redact` the
 * sentence the window shows after a redaction --- the one place a redaction's
 * verdict is worded, so the tool can never say *verified* more strongly than
 * the window does. The tool is Rust and has
 * no webview to ask, so `src-tauri/src/words.rs` restates the functions
 * below --- and a restatement is a second copy, which drifts. So Rust writes
 * every case it can produce to `src-tauri/testdata/cli/wording.json`
 * (`TPDF_CLI_SAMPLES=write`), and this file asks the originals the same
 * questions and compares. A word changed on either side is a red test here.
 *
 * The emptiness control is the counts: a sample that lost its cases would
 * otherwise compare nothing and pass.
 */

import { describe, expect, it } from "vitest";

import wording from "../../src-tauri/testdata/cli/wording.json";
import {
  authorityRow,
  chainRow,
  chainSentence,
  integrityRow,
  issuingCertificate,
  padesRow,
  revocationRow,
  timestampRow,
  trustRow,
  type Chain,
  type Integrity,
  type PadesLevel,
  type Revocation,
  type Trust,
} from "./integrity";
import { afterRedaction } from "./recovery";
import { afterSigning, type Signed } from "./signing";

describe("the command-line tool's wording", () => {
  it("covers every verdict, standing and closing sentence", () => {
    // 5 verdicts x 3 shapes, the unchecked one with 8 reasons more, each with
    // and without an append and a trust row; 6 standings x 3 stores x 10
    // reasons x 2 date pairs x judged now or at an attested moment; the
    // signing reports below.
    expect(wording.integrity.length).toBe((5 * 3 + 8) * 2 * 2);
    expect(wording.trust.length).toBe(6 * 3 * 10 * 2 * 2);
    expect(wording.trust.some((c) => c.trust.attested_at !== "")).toBe(true);
    // A timestamp: no verdict, or 5 verdicts x 2 shapes and the unchecked one
    // with 8 reasons, each named and unnamed, on a signature and a document;
    // its authority as the trust rows are.
    expect(wording.timestamp.length).toBe((1 + 5 * 2 + 8) * 2 * 2);
    expect(wording.authority.length).toBe(6 * 3 * 10 * 2 * 2);
    // A revocation row, for 4 bases: none, unchecked with no reason and 11,
    // and for each of 2 sources good and unknown with and without a next
    // update and revoked with 11 reasons before and after the moment; each
    // for the signer and for an authority.
    expect(wording.revocation.length).toBe(4 * (1 + 12 + 2 * (2 * 2 + 11 * 2)) * 2);
    // A chain row: each of those revocation shapes on the certificate above
    // the leaf; five answers on the leaf, above a good issuer, alone at a
    // root and alone short of one; a lone good leaf; a longer good chain;
    // three past the bound; two unnamed issuers two ways --- each for the
    // signer and for an authority.
    expect(wording.chain.length).toBe((4 * (1 + 12 + 2 * (2 * 2 + 11 * 2)) + 5 * 3 + 5 + 4) * 2);
    expect(wording.chain.some((c) => c.chain.dropped > 0)).toBe(true);
    // 7 signing reports, each with no timestamp and with four: sound, named
    // and trusted; sound from an authority nobody vouches for; sound and
    // unnamed; and one that does not check out.
    // And two with an archive timestamp after the new signature.
    expect(wording.after_signing.length).toBe(7 * 5 + 2);
    expect(
      wording.after_signing.filter((c) => c.sentence.includes(" Timestamp: ")).length,
    ).toBeGreaterThan(0);
    // 3 count pairs x 3 reason lists x changed or not, and a clean verdict
    // with one note and with two, changed or not.
    expect(wording.after_redaction.length).toBe(3 * 3 * 2 + 2 * 2);
    expect(wording.after_redaction.filter((c) => c.sentence.includes(" Note: ")).length).toBe(4);
    expect(wording.after_redaction.some((c) => c.applied.verified)).toBe(true);
    expect(wording.after_redaction.some((c) => !c.applied.verified)).toBe(true);
  });

  it("says each integrity verdict as the properties dialog does", () => {
    for (const c of wording.integrity) {
      const row = integrityRow(c.integrity as Integrity, c.appended, c.trust_follows);
      expect(row?.value, JSON.stringify(c)).toBe(c.sentence);
    }
  });

  it("says each trust standing as the properties dialog does", () => {
    for (const c of wording.trust) {
      const row = trustRow(c.trust as Trust, c.from, c.until);
      expect(row?.value, JSON.stringify(c)).toBe(c.sentence);
    }
  });

  it("says each timestamp as the properties dialog does", () => {
    for (const c of wording.timestamp) {
      const row = timestampRow(c.when, c.by, c.integrity as Integrity | null, c.document);
      expect(row.value, JSON.stringify(c)).toBe(c.sentence);
    }
  });

  it("says each timestamp authority's standing as the properties dialog does", () => {
    for (const c of wording.authority) {
      const row = authorityRow(c.trust as Trust, c.from, c.until);
      expect(row?.value, JSON.stringify(c)).toBe(c.sentence);
    }
  });

  it("says each revocation answer as the properties dialog does", () => {
    for (const c of wording.revocation) {
      const row = revocationRow(c.revocation as Revocation, c.authority);
      expect(row?.value, JSON.stringify(c)).toBe(c.sentence);
    }
  });

  it("says each chain's answer as the properties dialog does", () => {
    let shown = 0;
    for (const c of wording.chain) {
      const chain = c.chain as Chain;
      expect(chainSentence(chain, c.authority), JSON.stringify(c)).toBe(c.sentence);
      const row = chainRow(chain, c.authority);
      if (row) {
        shown += 1;
        expect(row.value).toBe(c.sentence);
      }
      // And each certificate above the leaf, as the JSON words it.
      chain.certificates.slice(1).forEach((judged, at) => {
        const said = revocationRow(judged.revocation, c.authority, issuingCertificate(judged));
        expect(said?.value, JSON.stringify(judged)).toBe(c.issuers[at]);
      });
    }
    expect(shown).toBeGreaterThan(0);
  });

  it("names each PAdES level as the properties dialog does", () => {
    expect(wording.pades.map((c) => c.level)).toEqual(["B-B", "B-T", "B-LT", "B-LTA"]);
    for (const c of wording.pades) {
      expect(padesRow(c.level as PadesLevel)?.value, c.level).toBe(c.sentence);
      expect(c.sentence.endsWith("Conformance to the standard is not tested.")).toBe(true);
    }
    expect(padesRow(null)).toBeNull();
  });

  it("reports a redaction as the window does", () => {
    for (const c of wording.after_redaction) {
      expect(afterRedaction(c.applied), JSON.stringify(c.applied)).toBe(c.sentence);
    }
  });

  it("closes a signing as the signing panel does", () => {
    for (const c of wording.after_signing) {
      expect(afterSigning(c.signed as Signed), JSON.stringify(c)).toBe(c.sentence);
    }
  });
});
