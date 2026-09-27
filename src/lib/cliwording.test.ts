/**
 * The command-line tool says what the application says, word for word.
 *
 * `tpdf verify` and `tpdf sign` print the properties dialog's Integrity and
 * Trust rows and the signing panel's closing sentence. The tool is Rust and has
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
import { integrityRow, trustRow, type Integrity, type Trust } from "./integrity";
import { afterSigning, type Signed } from "./signing";

describe("the command-line tool's wording", () => {
  it("covers every verdict, standing and closing sentence", () => {
    // 5 verdicts x 3 shapes, the unchecked one with 7 reasons more, each with
    // and without an append and a trust row; 5 standings x 3 stores x 8
    // reasons x 2 date pairs; 7 signing reports.
    expect(wording.integrity.length).toBe((5 * 3 + 7) * 2 * 2);
    expect(wording.trust.length).toBe(5 * 3 * 8 * 2);
    expect(wording.after_signing.length).toBe(7);
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

  it("closes a signing as the signing panel does", () => {
    for (const c of wording.after_signing) {
      expect(afterSigning(c.signed as Signed), JSON.stringify(c)).toBe(c.sentence);
    }
  });
});
