/**
 * The command-line tool says what the application says, word for word.
 *
 * `tpdf verify` and `tpdf sign` print the properties dialog's Integrity and
 * Trust rows and the signing panel's closing sentence, and `tpdf redact` the
 * sentence the window shows after a redaction --- the one place a redaction's
 * verdict is worded, so the tool can never say *verified* more strongly than
 * the window does. The tool is Rust and has
 * no webview to ask, so `src-tauri/src/cli/words.rs` restates the functions
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
  integrityRow,
  timestampRow,
  trustRow,
  type Integrity,
  type Trust,
} from "./integrity";
import { afterRedaction } from "./recovery";
import { afterSigning, type Signed } from "./signing";

describe("the command-line tool's wording", () => {
  it("covers every verdict, standing and closing sentence", () => {
    // 5 verdicts x 3 shapes, the unchecked one with 8 reasons more, each with
    // and without an append and a trust row; 5 standings x 3 stores x 9
    // reasons x 2 date pairs; the signing reports below.
    expect(wording.integrity.length).toBe((5 * 3 + 8) * 2 * 2);
    expect(wording.trust.length).toBe(5 * 3 * 9 * 2);
    // A timestamp: no verdict, or 5 verdicts x 2 shapes and the unchecked one
    // with 8 reasons, each named and unnamed, on a signature and a document;
    // its authority as the trust rows are.
    expect(wording.timestamp.length).toBe((1 + 5 * 2 + 8) * 2 * 2);
    expect(wording.authority.length).toBe(5 * 3 * 9 * 2);
    // 7 signing reports, each with no timestamp and with four: sound, named
    // and trusted; sound from an authority nobody vouches for; sound and
    // unnamed; and one that does not check out.
    expect(wording.after_signing.length).toBe(7 * 5);
    expect(
      wording.after_signing.filter((c) => c.sentence.includes(" Timestamp: ")).length,
    ).toBeGreaterThan(0);
    // 3 count pairs x 3 reason lists x changed or not.
    expect(wording.after_redaction.length).toBe(3 * 3 * 2);
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
