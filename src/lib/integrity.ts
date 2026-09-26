/**
 * Whether a signature still covers the bytes it was made over, in words.
 *
 * The backend half is `integrity.rs`, which computes the answer in the worker;
 * this module only chooses the sentence. It is a module of its own, rather
 * than a branch inside `properties.ts`, because it is the **one verdict** the
 * properties dialog gives, and the words are where a verdict goes wrong: a
 * bare "valid" beside a certificate somebody made for themselves five minutes
 * ago is a confident false statement about who signed.
 *
 * ## What each answer claims, and the sentence that stops it claiming more
 *
 * - `intact`: the covered bytes hash to the digest the signature signed, and
 *   the signature checks out under the key in the certificate it names. Always
 *   followed by {@link TRUST_NOT_CHECKED}, because nothing here asks whether
 *   that key belongs to the person the certificate names.
 * - `weak`: the same, under SHA-1, which no longer proves the bytes are the
 *   ones signed. Never phrased as intact.
 * - `altered`: the signature checks out, and the covered bytes no longer hash
 *   to what it signed --- the document changed inside the signed range.
 * - `broken`: the signature does not check out, so nothing it states can be
 *   relied on, including what the bytes should hash to.
 * - `unchecked`: nothing was concluded, with the reason, and the sentence
 *   says it means nothing either way. Never the reassuring branch.
 *
 * A signature whose range stops short of the end of the file can be intact
 * **and** have been appended to afterwards. The intact sentence says so in
 * that case rather than leaving the reader to reconcile it with the
 * *Covers* row below it.
 *
 * `properties.test.ts`'s `VERDICT_WORDS` rule still holds here: none of these
 * sentences says valid, verified, authentic or genuine.
 */

import type { Row } from "./properties";

/** The answer. Mirrors `integrity::Verdict`. */
export type Verdict = "unchecked" | "broken" | "altered" | "weak" | "intact";

/** Why nothing was concluded. Mirrors `integrity::Why`. */
export type Why =
  | "format"
  | "range"
  | "unreadable"
  | "certificate"
  | "algorithm"
  | "attributes"
  | "budget";

/** What checking a signature found. Mirrors `integrity::Integrity`. */
export interface Integrity {
  verdict: Verdict;
  why: Why | null;
  digest: string;
  method: string;
}

/**
 * Said after every answer that could be read as "this signer is who they say".
 *
 * Stated once and reused, so the one sentence that keeps "intact" from meaning
 * "trustworthy" cannot drift out of one branch while staying in another.
 */
export const TRUST_NOT_CHECKED =
  "Whether that key belongs to the person the certificate names was not checked.";

/** Why a signature was not checked, as a clause that follows "not checked —". */
export const WHY: Record<Why, string> = {
  format: "tpdf does not check signatures in this format",
  range:
    "the signed range does not leave out exactly this signature's own value, " +
    "so the signature does not protect the document the way it should",
  unreadable: "the signature's data could not be read",
  certificate:
    "the certificate the signature names is not in it, or its key could not be read",
  algorithm: "it uses an algorithm tpdf does not implement",
  attributes: "its signed attributes are not in the form the CMS standard requires",
  budget:
    "the document's signatures together cover more data than tpdf checks at once",
};

/** `(SHA-256, RSA)`, or nothing when the check stopped before either. */
function how(integrity: Integrity): string {
  const parts = [integrity.digest, integrity.method].filter(Boolean);
  return parts.length > 0 ? ` (${parts.join(", ")})` : "";
}

/**
 * The row that says whether the signature still covers what it was made over.
 *
 * `null` for a signature with no verdict, which is a field nobody signed.
 * `appended` is how many bytes were written after the signed range ends.
 */
export function integrityRow(integrity: Integrity | null, appended: number): Row | null {
  if (!integrity) return null;
  const name = "Integrity";
  switch (integrity.verdict) {
    case "intact": {
      const later =
        appended > 0
          ? " It covers the document as it was when signed; what was appended " +
            "afterwards is not part of it."
          : "";
      return {
        name,
        value:
          `intact — the signed bytes are unchanged and the signature checks out ` +
          `under the key in its certificate${how(integrity)}.${later} ${TRUST_NOT_CHECKED}`,
      };
    }
    case "weak":
      return {
        name,
        value:
          `unchanged under SHA-1 only — the digest and the signature match` +
          `${how(integrity)}, but SHA-1 collisions can be manufactured, so this ` +
          `does not show the bytes are the ones signed. ${TRUST_NOT_CHECKED}`,
        warn: true,
      };
    case "altered":
      return {
        name,
        value:
          `altered — the bytes this signature covers have changed since it was ` +
          `made${how(integrity)}. The signature itself checks out, so what ` +
          `changed is the document.`,
        warn: true,
      };
    case "broken":
      return {
        name,
        value:
          `broken — the signature does not check out under its certificate's ` +
          `key${how(integrity)}, so nothing it states can be relied on, ` +
          `including what it covers.`,
        warn: true,
      };
    case "unchecked":
      return {
        name,
        value:
          `not checked — ${integrity.why ? WHY[integrity.why] : "no reason was given"}. ` +
          `This says nothing either way about whether the document changed.`,
        warn: true,
      };
  }
}
