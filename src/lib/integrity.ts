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
 * out since is `expired` rather than trusted or not --- **unless** the signature
 * carries an intact timestamp from an authority this computer trusts, when it
 * is evaluated at the time that timestamp attests instead
 * (`trusted_at_timestamp`, since 2026-09-28).
 *
 * ## The revocation row, and what `none` means
 *
 * {@link revocationRow} is the third, from `revocation.rs`: what the revocation
 * data **the document itself carries** says about the certificate. tpdf never
 * fetches any, so a document carrying none --- the ordinary case --- reads
 * *not checked*, never as reassurance, and says so in its own sentence.
 *
 * ## A timestamp, and the time it is allowed to call attested
 *
 * {@link timestampRow} and {@link authorityRow} say the same two things about
 * an RFC 3161 timestamp that the rows above say about a signature: whether the
 * token checks out and covers this signature (`integrity/token.rs`), and
 * whether the store vouches for its authority, for timestamping. The time is
 * called attested only when the token is intact or weak; every other sentence
 * begins "not attested" and gives the time as what the token states.
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
  | "budget"
  | "binding";

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
  binding:
    "it does not name the certificate it was made with, as a timestamp must, or it " +
    "names another one",
};

/** How the signer's certificate stands in the OS store. Mirrors `trust::Standing`. */
export type Standing =
  | "unchecked"
  | "untrusted"
  | "not_yet_valid"
  | "expired"
  | "trusted"
  | "trusted_at_timestamp";

/** Why it is not trusted, or was not asked. Mirrors `trust::Doubt`. */
export type Doubt =
  | "incomplete"
  | "root"
  | "dates"
  | "purpose"
  | "rejected"
  | "certificate"
  | "unavailable"
  | "timestamping"
  | "not_in_force";

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
  /** The attested moment the chain was judged at; empty when judged now. */
  attested_at: string;
}

/** The computer whose store answered, as the sentence names it. */
export const COMPUTER: Record<Store, string> = {
  mac: "this Mac",
  windows: "this PC",
};

/**
 * Why the store does not vouch, as a clause; `computer` names the machine and
 * `whose` the certificate the chain starts from --- the signer's, or a
 * timestamp authority's.
 */
export const DOUBT: Record<Doubt, (computer: string, whose?: string) => string> = {
  incomplete: (computer, whose = "the signer's") =>
    `a certificate between ${whose} and a root is in neither the signature ` +
    `nor on ${computer}, and tpdf does not look it up`,
  root: (computer) =>
    `its chain ends at a root ${computer} does not trust. A certificate somebody ` +
    `issued to themselves reads this way, and so does one whose root only ` +
    `Adobe's trust list carries`,
  dates: (_computer, whose = "the signer's") => `a certificate above ${whose} is outside its dates`,
  purpose: () =>
    "the signer's certificate was issued for something other than signing documents",
  rejected: (computer) => `${computer} refused its chain`,
  certificate: () => "the signature's certificates could not be prepared for the check",
  unavailable: () => "the operating system's trust check could not be run",
  timestamping: () => "the authority's certificate was not issued for timestamping",
  not_in_force: () =>
    "the signer's certificate was not in force at the time the timestamp attests",
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
  const judged = trust.attested_at
    ? `, judged at ${trust.attested_at}, the time the timestamp attests`
    : "";
  switch (trust.standing) {
    case "trusted":
      return {
        name,
        value:
          `trusted — ${chained}, so an issuer ${computer} trusts vouches that the ` +
          `key belongs to the person the certificate names.`,
      };
    case "trusted_at_timestamp":
      return {
        name,
        value:
          `trusted at the timestamp — the signer's certificate chained to a root ` +
          `${computer} trusts at ${trust.attested_at}, the time a timestamp from an ` +
          `authority ${computer} trusts attests, so an issuer ${computer} trusts ` +
          `vouched that the key belonged to the person the certificate names when ` +
          `the signature was made. Whether the certificate has run out since does ` +
          `not change that.`,
      };
    case "expired":
      return {
        name,
        value:
          `expired — ${chained}, and it ran out${until ? ` on ${until}` : ""}. The ` +
          `date a signature gives is the signer's own claim, and no timestamp from ` +
          `an authority ${computer} trusts attests when it was made, so tpdf cannot ` +
          `tell whether it was made before then.`,
        warn: true,
      };
    case "not_yet_valid":
      return {
        name,
        value:
          `not yet in force — ${chained}, but it only comes into force` +
          `${from ? ` on ${from}` : " later"}.`,
        warn: true,
      };
    case "untrusted":
      return {
        name,
        value:
          `not trusted — ${why}${judged}. So nothing establishes that the key ` +
          `belongs to the person the certificate names.`,
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

/**
 * The row that says when a timestamp authority attests the signature existed,
 * and whether that attestation holds.
 *
 * The backend half is `integrity/token.rs`. **The time is called attested only
 * for `intact` and `weak`**, the rule `docinfo::Timestamp::attested` decides
 * in the worker; every other answer names the time as what the token states,
 * and says it is not attested --- a broken token's time is the token's word,
 * and nothing more. `integrity` is `null` only for a token nothing checked,
 * which the scan never produces; the sentence then says so rather than
 * implying a check. `document` says the token is a document timestamp, whose
 * imprint covers the signed bytes rather than a signature.
 */
export function timestampRow(
  when: string,
  by: string,
  integrity: Integrity | null,
  document: boolean,
): Row {
  const name = "Timestamped";
  const authority = by || "an unnamed authority";
  const subject = document ? "the signed bytes of this document" : "this signature";
  if (!integrity) {
    return {
      name,
      value: `${when} by ${authority} — a separate party's claim, which tpdf did not check.`,
      warn: true,
    };
  }
  switch (integrity.verdict) {
    case "intact":
      return {
        name,
        value:
          `${when}, attested by ${authority} — the timestamp checks out under the key ` +
          `in its certificate and covers ${subject}${how(integrity)}.`,
      };
    case "weak":
      return {
        name,
        value:
          `${when}, by ${authority}, under SHA-1 only — the timestamp checks out and ` +
          `covers ${subject}${how(integrity)}, but SHA-1 collisions can be manufactured, ` +
          `so this does not show the time belongs to ${subject}.`,
        warn: true,
      };
    case "altered":
      return {
        name,
        value:
          `not attested — a timestamp by ${authority} states ${when} and checks out, ` +
          `but it covers something other than ${subject}${how(integrity)}, so it ` +
          `attests nothing about ${subject}.`,
        warn: true,
      };
    case "broken":
      return {
        name,
        value:
          `not attested — a timestamp naming ${authority} states ${when}, but its own ` +
          `signature does not check out${how(integrity)}, so nothing it states can be ` +
          `relied on, the time included.`,
        warn: true,
      };
    case "unchecked":
      return {
        name,
        value:
          `not attested — a timestamp naming ${authority} states ${when}, and was not ` +
          `checked: ${integrity.why ? WHY[integrity.why] : "no reason was given"}.`,
        warn: true,
      };
  }
}

/**
 * The row that says whether the operating system's store vouches for the
 * timestamp authority, for timestamping.
 *
 * `null` when there is no standing, which is every token whose verdict is not
 * intact or weak. Evaluated at the present moment, like the signer's: the time
 * the token attests is the authority's own statement, so judging the authority
 * at it would let the party under question choose when it is judged.
 */
export function authorityRow(trust: Trust | null, from = "", until = ""): Row | null {
  if (!trust) return null;
  const name = "Timestamp authority";
  const computer = trust.store ? COMPUTER[trust.store] : "this computer";
  const why = trust.why ? DOUBT[trust.why](computer, "the authority's") : "no reason was given";
  const chained = `the authority's certificate chains to a root ${computer} trusts`;
  switch (trust.standing) {
    case "trusted":
      return {
        name,
        value:
          `trusted — ${chained} and is issued for timestamping. It is judged at the ` +
          `present moment, not at the time it attests.`,
      };
    // An authority judged at the moment an archive timestamp later in the
    // document attests (PAdES B-LTA), which then vouches for the time.
    case "trusted_at_timestamp":
      return {
        name,
        value:
          `trusted — ${chained} and is issued for timestamping, judged at ` +
          `${trust.attested_at}, the time an archive timestamp later in this document attests.`,
      };
    case "expired":
      return {
        name,
        value:
          `expired — ${chained}, and it ran out${until ? ` on ${until}` : ""}. tpdf ` +
          `judges it at the present moment, so it cannot tell whether it was in force ` +
          `when the timestamp was made.`,
        warn: true,
      };
    case "not_yet_valid":
      return {
        name,
        value:
          `not yet in force — ${chained}, but it only comes into force` +
          `${from ? ` on ${from}` : " later"}.`,
        warn: true,
      };
    case "untrusted":
      return {
        name,
        value: `not trusted — ${why}. So nothing establishes who attests this time.`,
        warn: true,
      };
    case "unchecked":
      return {
        name,
        value: `not checked — ${why}. This says nothing either way about who attests this time.`,
        warn: true,
      };
  }
}

/** The answer about revocation. Mirrors `revocation::Status`. */
export type RevocationStatus = "none" | "unchecked" | "unknown" | "revoked" | "good";

/** Why revocation data led to no conclusion. Mirrors `revocation::Gap`. */
export type Gap =
  | "unreadable"
  | "bound"
  | "issuer"
  | "signature"
  | "unauthorised"
  | "algorithm"
  | "unsupported"
  | "stale"
  | "expired"
  | "dates"
  | "budget";

/** A revocation's stated reason. Mirrors `revocation::Reason`. */
export type Reason =
  | "unspecified"
  | "key_compromise"
  | "ca_compromise"
  | "affiliation_changed"
  | "superseded"
  | "cessation_of_operation"
  | "certificate_hold"
  | "remove_from_crl"
  | "privilege_withdrawn"
  | "aa_compromise";

/** Whose clock the moment judged is. Mirrors `revocation::Basis`. */
export type Basis = "attested" | "stated" | "claimed" | "now";

/**
 * What the document's own revocation data says about one certificate.
 * Mirrors `revocation::Revocation`; present exactly where a {@link Trust} is.
 */
export interface Revocation {
  standing: RevocationStatus;
  why: Gap | null;
  source: "ocsp" | "crl" | null;
  issued: string;
  next: string;
  revoked: string;
  reason: Reason | null;
  basis: Basis;
  moment: string;
  after_moment: boolean;
}

/** A revocation reason, as a phrase. */
export const REASON: Record<Reason, string> = {
  unspecified: "no stated reason",
  key_compromise: "key compromise",
  ca_compromise: "compromise of its issuer",
  affiliation_changed: "a change of affiliation",
  superseded: "being superseded",
  cessation_of_operation: "cessation of operation",
  certificate_hold: "a hold, which may be lifted",
  remove_from_crl: "removal from a list",
  privilege_withdrawn: "privilege withdrawn",
  aa_compromise: "compromise of an attribute authority",
};

/** Why revocation data led to no conclusion; `moment` as {@link momentPhrase}. */
export const GAP: Record<Gap, (moment: string) => string> = {
  unreadable: () => "some of the document's revocation data could not be read",
  bound: () => "the document carries more revocation data than tpdf reads",
  issuer: () =>
    "the certificate that issued it is not in the document, so the data about it " +
    "cannot be checked",
  signature: () => "the data's own signature does not check out",
  unauthorised: () =>
    "the data is signed by a party its issuer did not authorise to answer for it",
  algorithm: () => "the data uses an algorithm tpdf does not implement",
  unsupported: () => "the data is in a form tpdf does not interpret",
  stale: (moment) => `the latest data about it does not reach ${moment}`,
  expired: () =>
    "the data was issued after the certificate expired, and does not say that it keeps " +
    "expired certificates",
  dates: () => "the data's own dates do not hang together",
  budget: () => "the document's signatures together cover more data than tpdf checks at once",
};

/** The moment a revocation was judged at, and whose clock it is. */
export function momentPhrase(revocation: Revocation): string {
  const at = revocation.moment;
  switch (revocation.basis) {
    case "attested":
      return `${at}, the time the timestamp attests`;
    case "stated":
      return `${at}, the time the timestamp states`;
    case "claimed":
      return `${at}, the signing date the signer gave, which is their own claim`;
    case "now":
      return "the present moment";
  }
}

/** The signer's or the authority's certificate, as the sentences name it. */
function leafHolder(authority: boolean): string {
  return authority ? "the authority's certificate" : "the signer's certificate";
}

/**
 * The row that says what the document's own revocation data says about a
 * certificate: the signer's, or a timestamp authority's (`authority`) --- or,
 * named by `whose`, a certificate above either on its chain.
 *
 * `null` when there is no answer, which is wherever there is no trust
 * standing. **`none` is worded as not checked**: tpdf fetches no revocation
 * data, so a document that carries none --- most of them --- has had
 * nothing checked, and a sentence that read as reassurance there would be a
 * confident false statement. Only `good` and a revocation after an attested
 * moment are shown without a warning.
 */
export function revocationRow(
  revocation: Revocation | null,
  authority = false,
  whose: string = leafHolder(authority),
): Row | null {
  if (!revocation) return null;
  const name = authority ? "Authority revocation" : "Revocation";
  const made = authority ? "the timestamp" : "the signature";
  const moment = momentPhrase(revocation);
  const source =
    revocation.source === "ocsp"
      ? "an OCSP response in the document, signed by its issuer or a responder its issuer " +
        "authorised,"
      : revocation.source === "crl"
        ? "a revocation list in the document, signed by its issuer,"
        : "data in the document";
  const next = revocation.next ? ` and meant to hold until ${revocation.next}` : "";
  const issued = `issued ${revocation.issued}${next}`;
  const why = revocation.reason ? `, for ${REASON[revocation.reason]}` : "";
  switch (revocation.standing) {
    case "good":
      return {
        name,
        value:
          `not revoked — ${source} ${issued}, says ${whose} had not been revoked, and it ` +
          `reaches ${moment}.`,
      };
    case "revoked":
      if (revocation.after_moment) {
        return {
          name,
          value:
            `revoked after the timestamp — ${source} ${issued}, says ${whose} was revoked ` +
            `on ${revocation.revoked}${why}, after ${moment}. A revocation after that time ` +
            `does not undo ${made}, which was made before it.`,
        };
      }
      if (revocation.basis === "attested") {
        return {
          name,
          value:
            `revoked — ${source} ${issued}, says ${whose} was revoked on ` +
            `${revocation.revoked}${why}, at or before ${moment}, so it was already ` +
            `withdrawn when ${made} was made.`,
          warn: true,
        };
      }
      return {
        name,
        value:
          `revoked — ${source} ${issued}, says ${whose} was revoked on ` +
          `${revocation.revoked}${why}. Nothing tpdf trusts attests when ${made} was made, ` +
          `so it cannot tell whether that was before then.`,
        warn: true,
      };
    case "unknown":
      return {
        name,
        value: `unknown — ${source} ${issued}, says its responder does not know ${whose}.`,
        warn: true,
      };
    case "none":
      return {
        name,
        value:
          `not checked — the document carries no revocation data for ${whose}, and tpdf ` +
          `does not fetch any, so a certificate its issuer has since withdrawn reads the ` +
          `same as one it has not.`,
        warn: true,
      };
    case "unchecked":
      return {
        name,
        value:
          `not checked — ${revocation.why ? GAP[revocation.why](moment) : "no reason was given"}. ` +
          `This says nothing either way about whether ${whose} was revoked.`,
        warn: true,
      };
  }
}

/** How a chain's walk ended. Mirrors `revocation::chain::End`. */
export type ChainEnd = "root" | "no_issuer" | "loop";

/** One certificate on a chain. Mirrors `revocation::chain::Judged`. */
export interface Judged {
  subject: string;
  subject_cn: string;
  serial: string;
  revocation: Revocation;
}

/**
 * What the document's revocation data says about a whole chain, the leaf
 * first. Mirrors `revocation::chain::Chain`; present exactly where the leaf's
 * {@link Revocation} is.
 */
export interface Chain {
  certificates: Judged[];
  standing: RevocationStatus;
  after_moment: boolean;
  decided_by: number | null;
  dropped: number;
  end: ChainEnd;
}

/** The most certificates tpdf judges on one chain: `revocation::chain::MAX_CHAIN`. */
export const MAX_CHAIN = 8;

/** A certificate above the leaf, as the sentences name it. */
export function issuingCertificate(judged: Judged): string {
  const name = judged.subject_cn || judged.subject;
  return name ? `the issuing certificate ${name}` : "an issuing certificate with no readable name";
}

/** "1 certificate", "3 certificates". */
function certificates(n: number): string {
  return n === 1 ? "1 certificate" : `${n} certificates`;
}

/** The first words of a chain's answer, when the leaf's own decides it. */
function chainHead(chain: Chain): string {
  switch (chain.standing) {
    case "revoked":
      return chain.after_moment ? "revoked after the timestamp" : "revoked";
    case "unknown":
      return "unknown";
    case "good":
      return "not revoked";
    case "none":
    case "unchecked":
      return "not checked";
  }
}

/**
 * The row that says what the document's revocation data says about the whole
 * chain from the signer's certificate --- or a timestamp authority's --- up to
 * its root, naming the certificate that decides it (`revocation/chain.rs`).
 *
 * `null` when there is no chain, and when the chain holds nothing above the
 * leaf and nothing past the bound: the leaf's own row then says all of it.
 * A revocation of an issuing certificate before the moment is worded as the
 * leaf's would be, with the certificate named, because it undoes the signature
 * exactly as the leaf's own would.
 */
export function chainRow(chain: Chain | null, authority = false): Row | null {
  if (!chain) return null;
  if (chain.certificates.length <= 1 && chain.dropped === 0) return null;
  const name = authority ? "Authority chain revocation" : "Chain revocation";
  const warn = !(chain.standing === "good" || (chain.standing === "revoked" && chain.after_moment));
  const row = (value: string): Row => (warn ? { name, value, warn } : { name, value });
  return row(chainSentence(chain, authority));
}

/**
 * {@link chainRow}'s value, for every chain: the command-line tool's JSON
 * carries a sentence even where the dialog shows no row.
 */
export function chainSentence(chain: Chain, authority = false): string {
  const whose = leafHolder(authority);
  const above = Math.max(chain.certificates.length - 1, 0);
  const unjudged =
    chain.dropped === 0
      ? ""
      : ` ${certificates(chain.dropped)} further up ${chain.dropped === 1 ? "was" : "were"} not judged.`;
  const decided = chain.decided_by;
  if (decided === null) {
    if (chain.standing === "good") {
      const first = chain.certificates[0];
      const moment = first ? momentPhrase(first.revocation) : "";
      const last = chain.certificates[chain.certificates.length - 1];
      const top = last ? issuingCertificate(last) : "";
      if (above === 0) {
        return `not revoked — the same as the row above: no certificate stands between ${whose} and a root.`;
      }
      if (above === 1) {
        return (
          `not revoked — the document's revocation data says neither ${whose} nor ${top} ` +
          `had been revoked, and it reaches ${moment}.`
        );
      }
      return (
        `not revoked — the document's revocation data says none of the ` +
        `${certificates(above + 1)} from ${whose} up to ${top} had been revoked, and it ` +
        `reaches ${moment}.`
      );
    }
    return (
      `not checked — the chain from ${whose} to its root is longer than the ${MAX_CHAIN} ` +
      `certificates tpdf follows, so ${certificates(chain.dropped)} further up ` +
      `${chain.dropped === 1 ? "was not judged" : "were not judged"}, and ` +
      `${chain.dropped === 1 ? "it" : "any of them"} might have been revoked.`
    );
  }
  if (decided === 0) {
    if (above === 0) {
      const why =
        chain.end === "root"
          ? `no certificate stands between ${whose} and a root`
          : `the certificate that issued ${whose} is not in the document`;
      return `${chainHead(chain)} — the same as the row above: ${why}.${unjudged}`;
    }
    const those = above === 1 ? "the certificate" : `the ${certificates(above)}`;
    return (
      `${chainHead(chain)} — decided by ${whose} itself, in the row above; nothing about ` +
      `${those} above it reads worse.${unjudged}`
    );
  }
  const judged = chain.certificates[decided];
  if (!judged) return "";
  const said = revocationRow(judged.revocation, authority, issuingCertificate(judged));
  return `${said?.value ?? ""}${unjudged}`;
}
