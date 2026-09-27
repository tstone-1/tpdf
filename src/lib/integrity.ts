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
 *   the signature checks out under the key in the certificate it names. Nothing
 *   in this sentence says whose key that is: that is {@link trustRow}'s, which
 *   follows it whenever the backend asked the operating system's trust store,
 *   and {@link TRUST_NOT_CHECKED} ends the sentence whenever it did not.
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
 * ## The trust row, and the one thing it never claims
 *
 * {@link trustRow} is the second verdict, from `trust.rs`: whether the signer's
 * certificate chains to a root **this computer's own trust store** trusts. It
 * names the store ("a root this Mac trusts"), because a signature made against
 * Adobe's list alone reads here as ending at a root this computer does not
 * trust. It is evaluated at the present moment, so a certificate that has run
 * out since is `expired` rather than trusted or not. And every standing that
 * reached a trusted root ends with {@link REVOCATION_NOT_CHECKED}: tpdf does not
 * go online, so revocation is never asked.
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

/** How the signer's certificate stands in the OS store. Mirrors `trust::Standing`. */
export type Standing = "unchecked" | "untrusted" | "not_yet_valid" | "expired" | "trusted";

/** Why it is not trusted, or was not asked. Mirrors `trust::Doubt`. */
export type Doubt =
  | "incomplete"
  | "root"
  | "dates"
  | "purpose"
  | "rejected"
  | "certificate"
  | "unavailable";

/** The store that answered. Mirrors `trust::Store`. */
export type Store = "mac" | "windows";

/**
 * What the operating system's trust store says about the signer's certificate.
 * Mirrors `trust::Trust`. Present only beside an intact or weak verdict.
 */
export interface Trust {
  standing: Standing;
  why: Doubt | null;
  store: Store | null;
}

/** The computer whose store answered, as the sentence names it. */
export const COMPUTER: Record<Store, string> = {
  mac: "this Mac",
  windows: "this PC",
};

/**
 * Said after every standing that says the chain reached a trusted root.
 *
 * tpdf does not go online, so no revocation list or OCSP responder is asked.
 * Stated once and reused, for the reason {@link TRUST_NOT_CHECKED} is.
 */
export const REVOCATION_NOT_CHECKED =
  "Revocation was not checked: tpdf does not go online, so a certificate its " +
  "issuer has since withdrawn reads the same as one it has not.";

/** Why the store does not vouch, as a clause; `computer` names the machine. */
export const DOUBT: Record<Doubt, (computer: string) => string> = {
  incomplete: (computer) =>
    `a certificate between the signer's and a root is in neither the signature ` +
    `nor on ${computer}, and tpdf does not look it up`,
  root: (computer) =>
    `its chain ends at a root ${computer} does not trust. A certificate somebody ` +
    `issued to themselves reads this way, and so does one whose root only ` +
    `Adobe's trust list carries`,
  dates: () => "a certificate above the signer's is outside its dates",
  purpose: () =>
    "the signer's certificate was issued for something other than signing documents",
  rejected: (computer) => `${computer} refused its chain`,
  certificate: () => "the signature's certificates could not be prepared for the check",
  unavailable: () => "the operating system's trust check could not be run",
};

/**
 * The row that says whether an issuer the OS trusts vouches for the signer.
 *
 * `null` when there is no standing, which is every signature whose integrity
 * verdict is not intact or weak --- there is nothing to attribute. `from` and
 * `until` are the signer's certificate's dates, as the certificate rows show
 * them, for the two standings that are about time.
 */
export function trustRow(
  trust: Trust | null,
  from = "",
  until = "",
): Row | null {
  if (!trust) return null;
  const name = "Trust";
  const computer = trust.store ? COMPUTER[trust.store] : "this computer";
  const why = trust.why ? DOUBT[trust.why](computer) : "no reason was given";
  const chained = `the signer's certificate chains to a root ${computer} trusts`;
  switch (trust.standing) {
    case "trusted":
      return {
        name,
        value:
          `trusted — ${chained}, so an issuer ${computer} trusts vouches that the ` +
          `key belongs to the person the certificate names. ${REVOCATION_NOT_CHECKED}`,
      };
    case "expired":
      return {
        name,
        value:
          `expired — ${chained}, and it ran out${until ? ` on ${until}` : ""}. The ` +
          `date a signature gives is the signer's own claim, so tpdf cannot tell ` +
          `whether it was made before then. ${REVOCATION_NOT_CHECKED}`,
        warn: true,
      };
    case "not_yet_valid":
      return {
        name,
        value:
          `not yet in force — ${chained}, but it only comes into force` +
          `${from ? ` on ${from}` : " later"}. ${REVOCATION_NOT_CHECKED}`,
        warn: true,
      };
    case "untrusted":
      return {
        name,
        value:
          `not trusted — ${why}. So nothing establishes that the key belongs to ` +
          `the person the certificate names.`,
        warn: true,
      };
    case "unchecked":
      return {
        name,
        value:
          `not checked — ${why}. This says nothing either way about who holds ` +
          `the key.`,
        warn: true,
      };
  }
}

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
 * `trustFollows` says a {@link trustRow} follows this one: the owner of the key is
 * then that row's subject, and {@link TRUST_NOT_CHECKED} would contradict it.
 */
export function integrityRow(
  integrity: Integrity | null,
  appended: number,
  trustFollows = false,
): Row | null {
  if (!integrity) return null;
  const name = "Integrity";
  const owner = trustFollows ? "" : ` ${TRUST_NOT_CHECKED}`;
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
          `under the key in its certificate${how(integrity)}.${later}${owner}`,
      };
    }
    case "weak":
      return {
        name,
        value:
          `unchanged under SHA-1 only — the digest and the signature match` +
          `${how(integrity)}, but SHA-1 collisions can be manufactured, so this ` +
          `does not show the bytes are the ones signed.${owner}`,
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
