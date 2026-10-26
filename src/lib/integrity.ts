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
 *
 * ## Two languages, and one of them is a contract
 *
 * The American English sentences are restated in Rust (`words.rs`) for the
 * command-line tool, and `cliwording.test.ts` holds the two equal word for
 * word: change one here and the Rust half must change too. That is also why
 * `authorise` keeps its British spelling in the American half. The German
 * half is the window's alone. German needs the grammatical case a phrase is
 * used in, so its sentences are built from what they are about (whose
 * certificate, which name) rather than from an English phrase slotted in.
 */

import type { Row } from "./properties";
import { catalog } from "./i18n";

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
export type Store = "mac" | "windows" | "linux";

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

/** A PAdES baseline level, as the worker names it; see `pades.rs`. */
export type PadesLevel = "B-B" | "B-T" | "B-LT" | "B-LTA";

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

/** What a trust or authority sentence is worded from. */
interface TrustWords {
  standing: Standing;
  /** The computer whose store answered, as {@link COMPUTER} names it. */
  computer: string;
  /** The doubt, already worded, or the no-reason clause. */
  why: string;
  attestedAt: string;
  from: string;
  until: string;
}

/** What a revocation sentence is worded from. */
interface RevocationWords {
  revocation: Revocation;
  authority: boolean;
  /** The certificate the sentence is about, as {@link leafHolder} or {@link issuingCertificate} names it. */
  whose: string;
  /** {@link momentPhrase}'s answer. */
  moment: string;
  /** The reason, already worded, or `""` for none. */
  reason: string;
  /** The gap, already worded, or the no-reason clause. */
  gap: string;
}

// English helpers, shared by the sentences of the American half.
const enWhose = (authority: boolean) => (authority ? "the authority's" : "the signer's");
const enLeaf = (authority: boolean) =>
  authority ? "the authority's certificate" : "the signer's certificate";
const enIssuing = (name: string) =>
  name ? `the issuing certificate ${name}` : "an issuing certificate with no readable name";
const enCertificates = (n: number) => (n === 1 ? "1 certificate" : `${n} certificates`);

// German helpers. A certificate's holder in the genitive ("des
// Unterzeichners"), the certificate as a nominative or accusative phrase
// (one form for a neuter noun) and as a dative one.
const deGen = (authority: boolean) => (authority ? "des Zeitstempeldienstes" : "des Unterzeichners");
const deLeaf = (authority: boolean) => `das Zertifikat ${deGen(authority)}`;
const deLeafDat = (authority: boolean) => `dem Zertifikat ${deGen(authority)}`;
const deIssuing = (name: string) =>
  name ? `das ausstellende Zertifikat ${name}` : "ein ausstellendes Zertifikat ohne lesbaren Namen";
const deIssuingDat = (name: string) =>
  name ? `dem ausstellenden Zertifikat ${name}` : "einem ausstellenden Zertifikat ohne lesbaren Namen";
const deCertificates = (n: number) => (n === 1 ? "1 Zertifikat" : `${n} Zertifikate`);

const TEXT = catalog({
  "en-US": {
    trustNotChecked: "Whether that key belongs to the person the certificate names was not checked.",
    noReason: "no reason was given",
    why: (why: Why): string => {
      switch (why) {
        case "format":
          return "tpdf does not check signatures in this format";
        case "range":
          return (
            "the signed range does not leave out exactly this signature's own value, " +
            "so the signature does not protect the document the way it should"
          );
        case "unreadable":
          return "the signature's data could not be read";
        case "certificate":
          return "the certificate the signature names is not in it, or its key could not be read";
        case "algorithm":
          return "it uses an algorithm tpdf does not implement";
        case "attributes":
          return "its signed attributes are not in the form the CMS standard requires";
        case "budget":
          return "the document's signatures together cover more data than tpdf checks at once";
        case "binding":
          return (
            "it does not name the certificate it was made with, as a timestamp must, or it " +
            "names another one"
          );
      }
    },
    computer: (store: Store | null): string =>
      store === "mac" ? "this Mac" : store === "windows" ? "this PC" : "this computer",
    doubt: (doubt: Doubt, computer: string, authority: boolean): string => {
      const whose = enWhose(authority);
      switch (doubt) {
        case "incomplete":
          return (
            `a certificate between ${whose} and a root is in neither the signature ` +
            `nor on ${computer}, and tpdf does not look it up`
          );
        case "root":
          return (
            `its chain ends at a root ${computer} does not trust. A certificate somebody ` +
            `issued to themselves reads this way, and so does one whose root only ` +
            `Adobe's trust list carries`
          );
        case "dates":
          return `a certificate above ${whose} is outside its dates`;
        case "purpose":
          return "the signer's certificate was issued for something other than signing documents";
        case "rejected":
          return `${computer} refused its chain`;
        case "certificate":
          return "the signature's certificates could not be prepared for the check";
        case "unavailable":
          return "the operating system's trust check could not be run";
        case "timestamping":
          return "the authority's certificate was not issued for timestamping";
        case "not_in_force":
          // Only ever said with the moment in the verdict's own words, which is
          // where "then" points: the moment is stated once, as the verdict's.
          return `${whose} certificate was not in force then`;
      }
    },
    trustName: "Trust",
    trust: (w: TrustWords): string => {
      const chained = `the signer's certificate chains to a root ${w.computer} trusts`;
      // The moment is the whole verdict's, so it goes beside the verdict's word
      // and nowhere else: after the reason it read as the reason's last clause.
      const judged = w.attestedAt ? `, judged at ${w.attestedAt}, the time the timestamp attests` : "";
      switch (w.standing) {
        case "trusted":
          return (
            `trusted — ${chained}, so an issuer ${w.computer} trusts vouches that the ` +
            `key belongs to the person the certificate names.`
          );
        case "trusted_at_timestamp":
          return (
            `trusted at the timestamp — the signer's certificate chained to a root ` +
            `${w.computer} trusts at ${w.attestedAt}, the time a timestamp from an ` +
            `authority ${w.computer} trusts attests, so an issuer ${w.computer} trusts ` +
            `vouched that the key belonged to the person the certificate names when ` +
            `the signature was made. Whether the certificate has run out since does ` +
            `not change that.`
          );
        case "expired":
          return (
            `expired — ${chained}, and it ran out${w.until ? ` on ${w.until}` : ""}. The ` +
            `date a signature gives is the signer's own claim, and no timestamp from ` +
            `an authority ${w.computer} trusts attests when it was made, so tpdf cannot ` +
            `tell whether it was made before then.`
          );
        case "not_yet_valid":
          return (
            `not yet in force — ${chained}, but it only comes into force` +
            `${w.from ? ` on ${w.from}` : " later"}.`
          );
        case "untrusted":
          return (
            `not trusted${judged} — ${w.why}. So nothing establishes that the key ` +
            `belongs to the person the certificate names.`
          );
        case "unchecked":
          return `not checked — ${w.why}. This says nothing either way about who holds the key.`;
      }
    },
    integrityName: "Integrity",
    integrity: (verdict: Verdict, how: string, appended: boolean, owner: string, why: string): string => {
      switch (verdict) {
        case "intact": {
          const later = appended
            ? " It covers the document as it was when signed; what was appended " +
              "afterwards is not part of it."
            : "";
          return (
            `intact — the signed bytes are unchanged and the signature checks out ` +
            `under the key in its certificate${how}.${later}${owner}`
          );
        }
        case "weak":
          return (
            `unchanged under SHA-1 only — the digest and the signature match` +
            `${how}, but SHA-1 collisions can be manufactured, so this ` +
            `does not show the bytes are the ones signed.${owner}`
          );
        case "altered":
          return (
            `altered — the bytes this signature covers have changed since it was ` +
            `made${how}. The signature itself checks out, so what ` +
            `changed is the document.`
          );
        case "broken":
          return (
            `broken — the signature does not check out under its certificate's ` +
            `key${how}, so nothing it states can be relied on, ` +
            `including what it covers.`
          );
        case "unchecked":
          return `not checked — ${why}. This says nothing either way about whether the document changed.`;
      }
    },
    timestampName: "Timestamped",
    timestamp: (
      verdict: Verdict | null,
      when: string,
      by: string,
      document: boolean,
      how: string,
      why: string,
    ): string => {
      const authority = by || "an unnamed authority";
      const subject = document ? "the signed bytes of this document" : "this signature";
      switch (verdict) {
        case null:
          return `${when} by ${authority} — a separate party's claim, which tpdf did not check.`;
        case "intact":
          return (
            `${when}, attested by ${authority} — the timestamp checks out under the key ` +
            `in its certificate and covers ${subject}${how}.`
          );
        case "weak":
          return (
            `${when}, by ${authority}, under SHA-1 only — the timestamp checks out and ` +
            `covers ${subject}${how}, but SHA-1 collisions can be manufactured, ` +
            `so this does not show the time belongs to ${subject}.`
          );
        case "altered":
          return (
            `not attested — a timestamp by ${authority} states ${when} and checks out, ` +
            `but it covers something other than ${subject}${how}, so it ` +
            `attests nothing about ${subject}.`
          );
        case "broken":
          return (
            `not attested — a timestamp naming ${authority} states ${when}, but its own ` +
            `signature does not check out${how}, so nothing it states can be ` +
            `relied on, the time included.`
          );
        case "unchecked":
          return `not attested — a timestamp naming ${authority} states ${when}, and was not checked: ${why}.`;
      }
    },
    padesName: "PAdES level",
    pades: (level: PadesLevel): string => {
      const parts: Record<PadesLevel, string> = {
        "B-B": "a CAdES signature with no timestamp, so its only date is the signer's own",
        "B-T":
          "signed and timestamped; the document does not carry revocation data that answers for every certificate",
        "B-LT":
          "signed, timestamped, and carrying revocation data that answers for every certificate; no document timestamp seals that data",
        "B-LTA":
          "signed, timestamped, carrying revocation data that answers for every certificate, and sealed by a document timestamp",
      };
      return `${level} by its parts — ${parts[level]}. Conformance to the standard is not tested.`;
    },
    authorityName: "Timestamp authority",
    authority: (w: TrustWords): string => {
      const chained = `the authority's certificate chains to a root ${w.computer} trusts`;
      // Judged at the moment an archive timestamp later in the document attests
      // (PAdES B-LTA), which then vouches for the time --- said beside the
      // verdict's word, as the signer's is, because the moment is the verdict's.
      const judged = w.attestedAt
        ? `, judged at ${w.attestedAt}, the time an archive timestamp later in this document attests`
        : "";
      switch (w.standing) {
        case "trusted":
          return (
            `trusted — ${chained} and is issued for timestamping. It is judged at the ` +
            `present moment, not at the time it attests.`
          );
        case "trusted_at_timestamp":
          return `trusted${judged} — ${chained} and is issued for timestamping.`;
        case "expired":
          return (
            `expired — ${chained}, and it ran out${w.until ? ` on ${w.until}` : ""}. tpdf ` +
            `judges it at the present moment, so it cannot tell whether it was in force ` +
            `when the timestamp was made.`
          );
        case "not_yet_valid":
          return (
            `not yet in force — ${chained}, but it only comes into force` +
            `${w.from ? ` on ${w.from}` : " later"}.`
          );
        case "untrusted":
          return `not trusted${judged} — ${w.why}. So nothing establishes who attests this time.`;
        case "unchecked":
          return `not checked — ${w.why}. This says nothing either way about who attests this time.`;
      }
    },
    reason: (reason: Reason): string => {
      switch (reason) {
        case "unspecified":
          return "no stated reason";
        case "key_compromise":
          return "key compromise";
        case "ca_compromise":
          return "compromise of its issuer";
        case "affiliation_changed":
          return "a change of affiliation";
        case "superseded":
          return "being superseded";
        case "cessation_of_operation":
          return "cessation of operation";
        case "certificate_hold":
          return "a hold, which may be lifted";
        case "remove_from_crl":
          return "removal from a list";
        case "privilege_withdrawn":
          return "privilege withdrawn";
        case "aa_compromise":
          return "compromise of an attribute authority";
      }
    },
    gap: (gap: Gap, moment: string): string => {
      switch (gap) {
        case "unreadable":
          return "some of the document's revocation data could not be read";
        case "bound":
          return "the document carries more revocation data than tpdf reads";
        case "issuer":
          return (
            "the certificate that issued it is not in the document, so the data about it " +
            "cannot be checked"
          );
        case "signature":
          return "the data's own signature does not check out";
        case "unauthorised":
          return "the data is signed by a party its issuer did not authorise to answer for it";
        case "algorithm":
          return "the data uses an algorithm tpdf does not implement";
        case "unsupported":
          return "the data is in a form tpdf does not interpret";
        case "stale":
          return `the latest data about it does not reach ${moment}`;
        case "expired":
          return (
            "the data was issued after the certificate expired, and does not say that it keeps " +
            "expired certificates"
          );
        case "dates":
          return "the data's own dates do not hang together";
        case "budget":
          return "the document's signatures together cover more data than tpdf checks at once";
      }
    },
    moment: (basis: Basis, at: string): string => {
      switch (basis) {
        case "attested":
          return `${at}, the time the timestamp attests`;
        case "stated":
          return `${at}, the time the timestamp states`;
        case "claimed":
          return `${at}, the signing date the signer gave, which is their own claim`;
        case "now":
          return "the present moment";
      }
    },
    leaf: (authority: boolean): string => enLeaf(authority),
    issuing: (name: string): string => enIssuing(name),
    revocationName: (authority: boolean): string => (authority ? "Authority revocation" : "Revocation"),
    revocation: (w: RevocationWords): string => {
      const r = w.revocation;
      const made = w.authority ? "the timestamp" : "the signature";
      const source =
        r.source === "ocsp"
          ? "an OCSP response in the document, signed by its issuer or a responder its issuer " +
            "authorised,"
          : r.source === "crl"
            ? "a revocation list in the document, signed by its issuer,"
            : "data in the document";
      const next = r.next ? ` and meant to hold until ${r.next}` : "";
      const issued = `issued ${r.issued}${next}`;
      const why = w.reason ? `, for ${w.reason}` : "";
      switch (r.standing) {
        case "good":
          return (
            `not revoked — ${source} ${issued}, says ${w.whose} had not been revoked, and it ` +
            `reaches ${w.moment}.`
          );
        case "revoked":
          if (r.after_moment) {
            return (
              `revoked after the timestamp — ${source} ${issued}, says ${w.whose} was revoked ` +
              `on ${r.revoked}${why}, after ${w.moment}. A revocation after that time ` +
              `does not undo ${made}, which was made before it.`
            );
          }
          if (r.basis === "attested") {
            return (
              `revoked — ${source} ${issued}, says ${w.whose} was revoked on ` +
              `${r.revoked}${why}, at or before ${w.moment}, so it was already ` +
              `withdrawn when ${made} was made.`
            );
          }
          return (
            `revoked — ${source} ${issued}, says ${w.whose} was revoked on ` +
            `${r.revoked}${why}. Nothing tpdf trusts attests when ${made} was made, ` +
            `so it cannot tell whether that was before then.`
          );
        case "unknown":
          return `unknown — ${source} ${issued}, says its responder does not know ${w.whose}.`;
        case "none":
          return (
            `not checked — the document carries no revocation data for ${w.whose}, and tpdf ` +
            `does not fetch any, so a certificate its issuer has since withdrawn reads the ` +
            `same as one it has not.`
          );
        case "unchecked":
          return `not checked — ${w.gap}. This says nothing either way about whether ${w.whose} was revoked.`;
      }
    },
    chainName: (authority: boolean): string => (authority ? "Authority chain revocation" : "Chain revocation"),
    chainUnjudged: (dropped: number): string =>
      ` ${enCertificates(dropped)} further up ${dropped === 1 ? "was" : "were"} not judged.`,
    chainGoodAlone: (authority: boolean): string =>
      `not revoked — the same as the row above: no certificate stands between ${enLeaf(authority)} and a root.`,
    chainGoodOne: (authority: boolean, top: string, moment: string): string =>
      `not revoked — the document's revocation data says neither ${enLeaf(authority)} nor ` +
      `${enIssuing(top)} had been revoked, and it reaches ${moment}.`,
    chainGoodMany: (authority: boolean, count: number, top: string, moment: string): string =>
      `not revoked — the document's revocation data says none of the ` +
      `${enCertificates(count)} from ${enLeaf(authority)} up to ${enIssuing(top)} had been ` +
      `revoked, and it reaches ${moment}.`,
    chainTooLong: (authority: boolean, max: number, dropped: number): string =>
      `not checked — the chain from ${enLeaf(authority)} to its root is longer than the ${max} ` +
      `certificates tpdf follows, so ${enCertificates(dropped)} further up ` +
      `${dropped === 1 ? "was not judged" : "were not judged"}, and ` +
      `${dropped === 1 ? "it" : "any of them"} might have been revoked.`,
    chainHead: (standing: RevocationStatus, afterMoment: boolean): string => {
      switch (standing) {
        case "revoked":
          return afterMoment ? "revoked after the timestamp" : "revoked";
        case "unknown":
          return "unknown";
        case "good":
          return "not revoked";
        case "none":
        case "unchecked":
          return "not checked";
      }
    },
    chainSameAbove: (head: string, authority: boolean, atRoot: boolean, unjudged: string): string => {
      const why = atRoot
        ? `no certificate stands between ${enLeaf(authority)} and a root`
        : `the certificate that issued ${enLeaf(authority)} is not in the document`;
      return `${head} — the same as the row above: ${why}.${unjudged}`;
    },
    chainByLeaf: (head: string, authority: boolean, above: number, unjudged: string): string => {
      const those = above === 1 ? "the certificate" : `the ${enCertificates(above)}`;
      return (
        `${head} — decided by ${enLeaf(authority)} itself, in the row above; nothing about ` +
        `${those} above it reads worse.${unjudged}`
      );
    },
  },
  "de-DE": {
    trustNotChecked: "Ob dieser Schlüssel der Person gehört, die das Zertifikat nennt, wurde nicht geprüft.",
    noReason: "es wurde kein Grund angegeben",
    why: (why: Why): string => {
      switch (why) {
        case "format":
          return "tpdf prüft keine Signaturen in diesem Format";
        case "range":
          return (
            "der signierte Bereich spart nicht genau den eigenen Wert dieser Signatur aus, " +
            "daher schützt die Signatur das Dokument nicht so, wie sie sollte"
          );
        case "unreadable":
          return "die Daten der Signatur konnten nicht gelesen werden";
        case "certificate":
          return (
            "das Zertifikat, das die Signatur nennt, ist nicht in ihr enthalten, oder sein " +
            "Schlüssel konnte nicht gelesen werden"
          );
        case "algorithm":
          return "sie verwendet einen Algorithmus, den tpdf nicht implementiert";
        case "attributes":
          return "ihre signierten Attribute haben nicht die Form, die der CMS-Standard verlangt";
        case "budget":
          return "die Signaturen des Dokuments erfassen zusammen mehr Daten, als tpdf auf einmal prüft";
        case "binding":
          return (
            "sie nennt nicht das Zertifikat, mit dem sie erstellt wurde, wie es ein Zeitstempel " +
            "muss, oder sie nennt ein anderes"
          );
      }
    },
    computer: (store: Store | null): string =>
      store === "mac" ? "dieser Mac" : store === "windows" ? "dieser PC" : "dieser Computer",
    doubt: (doubt: Doubt, computer: string, authority: boolean): string => {
      const whose = deGen(authority);
      switch (doubt) {
        case "incomplete":
          return (
            `ein Zertifikat zwischen dem Zertifikat ${whose} und einem Stammzertifikat fehlt: ` +
            `Die Signatur enthält es nicht, ${computer} kennt es nicht, und tpdf sucht nicht danach`
          );
        case "root":
          return (
            `seine Kette endet bei einem Stammzertifikat, dem ${computer} nicht vertraut. So ` +
            `erscheint ein Zertifikat, das sich jemand selbst ausgestellt hat, und ebenso eines, ` +
            `dessen Stammzertifikat nur in Adobes Vertrauensliste steht`
          );
        case "dates":
          return `ein Zertifikat oberhalb des Zertifikats ${whose} liegt außerhalb seiner Laufzeit`;
        case "purpose":
          return (
            "das Zertifikat des Unterzeichners wurde für einen anderen Zweck als das Signieren " +
            "von Dokumenten ausgestellt"
          );
        case "rejected":
          return `${computer} hat seine Kette abgelehnt`;
        case "certificate":
          return "die Zertifikate der Signatur konnten nicht für die Prüfung vorbereitet werden";
        case "unavailable":
          return "die Vertrauensprüfung des Betriebssystems konnte nicht ausgeführt werden";
        case "timestamping":
          return "das Zertifikat des Zeitstempeldienstes wurde nicht für Zeitstempel ausgestellt";
        case "not_in_force":
          return `das Zertifikat ${whose} war zu diesem Zeitpunkt nicht in Kraft`;
      }
    },
    trustName: "Vertrauen",
    trust: (w: TrustWords): string => {
      const chained =
        `das Zertifikat des Unterzeichners führt in seiner Kette zu einem Stammzertifikat, ` +
        `dem ${w.computer} vertraut`;
      const judged = w.attestedAt
        ? `, beurteilt zum Zeitpunkt ${w.attestedAt}, den der Zeitstempel bestätigt`
        : "";
      switch (w.standing) {
        case "trusted":
          return (
            `vertrauenswürdig — ${chained}, also bürgt ein Aussteller, dem ${w.computer} vertraut, ` +
            `dafür, dass der Schlüssel der Person gehört, die das Zertifikat nennt.`
          );
        case "trusted_at_timestamp":
          return (
            `vertrauenswürdig zum Zeitpunkt des Zeitstempels — das Zertifikat des Unterzeichners ` +
            `führte zum Zeitpunkt ${w.attestedAt} zu einem Stammzertifikat, dem ${w.computer} ` +
            `vertraut; diesen Zeitpunkt bestätigt ein Zeitstempel eines Zeitstempeldienstes, dem ` +
            `${w.computer} vertraut. Also bürgte ein Aussteller, dem ${w.computer} vertraut, dafür, ` +
            `dass der Schlüssel der Person gehörte, die das Zertifikat nennt, als die Signatur ` +
            `erstellt wurde. Ob das Zertifikat seitdem abgelaufen ist, ändert daran nichts.`
          );
        case "expired":
          return (
            `abgelaufen — ${chained}, und es ist${w.until ? ` am ${w.until}` : ""} abgelaufen. ` +
            `Das Datum, das eine Signatur angibt, ist die eigene Behauptung des Unterzeichners, ` +
            `und kein Zeitstempel eines Zeitstempeldienstes, dem ${w.computer} vertraut, bestätigt, ` +
            `wann sie erstellt wurde. Daher kann tpdf nicht feststellen, ob sie vorher erstellt wurde.`
          );
        case "not_yet_valid":
          return (
            `noch nicht in Kraft — ${chained}, aber es tritt erst` +
            `${w.from ? ` am ${w.from}` : " später"} in Kraft.`
          );
        case "untrusted":
          return (
            `nicht vertrauenswürdig${judged} — ${w.why}. Damit ist nicht belegt, dass der ` +
            `Schlüssel der Person gehört, die das Zertifikat nennt.`
          );
        case "unchecked":
          return `nicht geprüft — ${w.why}. Daraus folgt nichts darüber, wer den Schlüssel besitzt.`;
      }
    },
    integrityName: "Integrität",
    integrity: (verdict: Verdict, how: string, appended: boolean, owner: string, why: string): string => {
      switch (verdict) {
        case "intact": {
          const later = appended
            ? " Sie erfasst das Dokument so, wie es beim Signieren war; was danach angehängt " +
              "wurde, gehört nicht dazu."
            : "";
          return (
            `intakt — die signierten Bytes sind unverändert, und die Signatur passt zum ` +
            `Schlüssel in ihrem Zertifikat${how}.${later}${owner}`
          );
        }
        case "weak":
          return (
            `unverändert nur unter SHA-1 — Hashwert und Signatur stimmen überein${how}, aber ` +
            `SHA-1-Kollisionen lassen sich gezielt herstellen, daher zeigt das nicht, dass es ` +
            `die signierten Bytes sind.${owner}`
          );
        case "altered":
          return (
            `verändert — die Bytes, die diese Signatur erfasst, haben sich seit ihrer Erstellung ` +
            `geändert${how}. Die Signatur selbst passt zum Schlüssel, also hat sich das Dokument ` +
            `geändert.`
          );
        case "broken":
          return (
            `fehlerhaft — die Signatur passt nicht zum Schlüssel ihres Zertifikats${how}, daher ` +
            `kann man sich auf nichts verlassen, was sie angibt, auch nicht darauf, was sie erfasst.`
          );
        case "unchecked":
          return `nicht geprüft — ${why}. Daraus folgt nichts darüber, ob sich das Dokument geändert hat.`;
      }
    },
    timestampName: "Zeitstempel",
    timestamp: (
      verdict: Verdict | null,
      when: string,
      by: string,
      document: boolean,
      how: string,
      why: string,
    ): string => {
      const from = by || "einem nicht genannten Zeitstempeldienst";
      // "Naming" an authority, which a token whose signature fails only claims.
      const naming = by
        ? `ein Zeitstempel, der ${by} als Aussteller nennt,`
        : "ein Zeitstempel ohne genannten Aussteller";
      const subject = document ? "die signierten Bytes dieses Dokuments" : "diese Signatur";
      switch (verdict) {
        case null:
          return `${when} von ${from} — die Behauptung einer anderen Stelle, die tpdf nicht geprüft hat.`;
        case "intact":
          return (
            `${when}, bestätigt von ${from} — der Zeitstempel passt zum Schlüssel in seinem ` +
            `Zertifikat und erfasst ${subject}${how}.`
          );
        case "weak":
          return (
            `${when}, von ${from}, nur unter SHA-1 — der Zeitstempel passt zum Schlüssel und ` +
            `erfasst ${subject}${how}, aber SHA-1-Kollisionen lassen sich gezielt herstellen, ` +
            `daher zeigt das nicht, dass sich die Zeit auf ${subject} bezieht.`
          );
        case "altered":
          return (
            `nicht bestätigt — ein Zeitstempel von ${from} gibt ${when} an und passt zum ` +
            `Schlüssel, erfasst aber etwas anderes als ${subject}${how}, daher bestätigt er ` +
            `nichts über ${subject}.`
          );
        case "broken":
          return (
            `nicht bestätigt — ${naming} gibt ${when} an, aber seine eigene Signatur passt ` +
            `nicht zum Schlüssel${how}, daher kann man sich auf nichts verlassen, was er ` +
            `angibt, auch nicht auf die Zeit.`
          );
        case "unchecked":
          return `nicht bestätigt — ${naming} gibt ${when} an und wurde nicht geprüft: ${why}.`;
      }
    },
    padesName: "PAdES-Stufe",
    pades: (level: PadesLevel): string => {
      const parts: Record<PadesLevel, string> = {
        "B-B": "eine CAdES-Signatur ohne Zeitstempel, daher ist ihr einziges Datum das eigene des Unterzeichners",
        "B-T":
          "signiert und mit Zeitstempel; das Dokument enthält keine Widerrufsdaten, die für jedes Zertifikat Auskunft geben",
        "B-LT":
          "signiert, mit Zeitstempel und mit Widerrufsdaten, die für jedes Zertifikat Auskunft geben; kein Dokumentzeitstempel versiegelt diese Daten",
        "B-LTA":
          "signiert, mit Zeitstempel, mit Widerrufsdaten, die für jedes Zertifikat Auskunft geben, und versiegelt durch einen Dokumentzeitstempel",
      };
      return `${level} nach seinen Bestandteilen — ${parts[level]}. Die Konformität mit dem Standard wird nicht geprüft.`;
    },
    authorityName: "Zeitstempeldienst",
    authority: (w: TrustWords): string => {
      const chained =
        `das Zertifikat des Zeitstempeldienstes führt in seiner Kette zu einem Stammzertifikat, ` +
        `dem ${w.computer} vertraut,`;
      const judged = w.attestedAt
        ? `, beurteilt zum Zeitpunkt ${w.attestedAt}, den ein späterer Archivzeitstempel in diesem Dokument bestätigt`
        : "";
      switch (w.standing) {
        case "trusted":
          return (
            `vertrauenswürdig — ${chained} und ist für Zeitstempel ausgestellt. Es wird zum ` +
            `gegenwärtigen Zeitpunkt beurteilt, nicht zu dem Zeitpunkt, den der Zeitstempel bestätigt.`
          );
        case "trusted_at_timestamp":
          return `vertrauenswürdig${judged} — ${chained} und ist für Zeitstempel ausgestellt.`;
        case "expired":
          return (
            `abgelaufen — ${chained} und ist${w.until ? ` am ${w.until}` : ""} abgelaufen. tpdf ` +
            `beurteilt es zum gegenwärtigen Zeitpunkt und kann daher nicht feststellen, ob es in ` +
            `Kraft war, als der Zeitstempel erstellt wurde.`
          );
        case "not_yet_valid":
          return (
            `noch nicht in Kraft — ${chained} tritt aber erst` +
            `${w.from ? ` am ${w.from}` : " später"} in Kraft.`
          );
        case "untrusted":
          return `nicht vertrauenswürdig${judged} — ${w.why}. Damit ist nicht belegt, wer diese Zeit bestätigt.`;
        case "unchecked":
          return `nicht geprüft — ${w.why}. Daraus folgt nichts darüber, wer diese Zeit bestätigt.`;
      }
    },
    reason: (reason: Reason): string => {
      switch (reason) {
        case "unspecified":
          return "kein angegebener Grund";
        case "key_compromise":
          return "Kompromittierung des Schlüssels";
        case "ca_compromise":
          return "Kompromittierung seines Ausstellers";
        case "affiliation_changed":
          return "geänderte Zugehörigkeit";
        case "superseded":
          return "ersetzt";
        case "cessation_of_operation":
          return "Einstellung des Betriebs";
        case "certificate_hold":
          return "vorläufige Sperre, die aufgehoben werden kann";
        case "remove_from_crl":
          return "Entfernung aus einer Liste";
        case "privilege_withdrawn":
          return "Berechtigung entzogen";
        case "aa_compromise":
          return "Kompromittierung einer Attributstelle";
      }
    },
    gap: (gap: Gap, moment: string): string => {
      switch (gap) {
        case "unreadable":
          return "einige Widerrufsdaten des Dokuments konnten nicht gelesen werden";
        case "bound":
          return "das Dokument enthält mehr Widerrufsdaten, als tpdf liest";
        case "issuer":
          return (
            "das Zertifikat, das es ausgestellt hat, ist nicht im Dokument, daher können die " +
            "Daten dazu nicht geprüft werden"
          );
        case "signature":
          return "die eigene Signatur der Daten passt nicht zum Schlüssel";
        case "unauthorised":
          return (
            "die Daten sind von einer Stelle signiert, die sein Aussteller nicht ermächtigt hat, " +
            "dafür Auskunft zu geben"
          );
        case "algorithm":
          return "die Daten verwenden einen Algorithmus, den tpdf nicht implementiert";
        case "unsupported":
          return "die Daten liegen in einer Form vor, die tpdf nicht auswertet";
        case "stale":
          return `die neuesten Daten dazu reichen nicht bis ${moment}`;
        case "expired":
          return (
            "die Daten wurden ausgestellt, nachdem das Zertifikat abgelaufen war, und sagen nicht, " +
            "dass ihr Aussteller abgelaufene Zertifikate weiter führt"
          );
        case "dates":
          return "die eigenen Datumsangaben der Daten passen nicht zusammen";
        case "budget":
          return "die Signaturen des Dokuments erfassen zusammen mehr Daten, als tpdf auf einmal prüft";
      }
    },
    // Each follows "bis", "nach" or "spätestens", so it carries no case: the
    // time, then what it is in parentheses.
    moment: (basis: Basis, at: string): string => {
      switch (basis) {
        case "attested":
          return `${at} (der Zeitpunkt, den der Zeitstempel bestätigt)`;
        case "stated":
          return `${at} (der Zeitpunkt, den der Zeitstempel angibt)`;
        case "claimed":
          return `${at} (das Signaturdatum, das der Unterzeichner angegeben hat, also seine eigene Behauptung)`;
        case "now":
          return "jetzt";
      }
    },
    leaf: (authority: boolean): string => deLeaf(authority),
    issuing: (name: string): string => deIssuing(name),
    revocationName: (authority: boolean): string =>
      authority ? "Widerruf des Zeitstempeldienstes" : "Widerruf",
    revocation: (w: RevocationWords): string => {
      const r = w.revocation;
      const madeNom = w.authority ? "der Zeitstempel" : "die Signatur";
      const madeAcc = w.authority ? "den Zeitstempel" : "die Signatur";
      const madeRel = w.authority ? "der" : "die";
      const source =
        r.source === "ocsp"
          ? "eine OCSP-Antwort im Dokument, signiert von seinem Aussteller oder einem Responder, " +
            "den sein Aussteller dazu ermächtigt hat"
          : r.source === "crl"
            ? "eine Widerrufsliste im Dokument, signiert von seinem Aussteller"
            : "Daten im Dokument";
      const next = r.next ? ` und vorgesehen bis ${r.next}` : "";
      const lead = `${source}, ausgestellt am ${r.issued}${next},`;
      const says = r.source === null ? "besagen" : "besagt";
      const reach = r.source === null ? "reichen" : "reicht";
      const why = w.reason ? ` (Grund: ${w.reason})` : "";
      switch (r.standing) {
        case "good":
          return (
            `nicht widerrufen — ${lead} ${says}, dass ${w.whose} nicht widerrufen war, und ` +
            `${reach} bis ${w.moment}.`
          );
        case "revoked":
          if (r.after_moment) {
            return (
              `nach dem Zeitstempel widerrufen — ${lead} ${says}, dass ${w.whose} am ` +
              `${r.revoked} widerrufen wurde${why}, nach ${w.moment}. Ein Widerruf nach diesem ` +
              `Zeitpunkt macht ${madeAcc} nicht unwirksam, ${madeRel} davor erstellt wurde.`
            );
          }
          if (r.basis === "attested") {
            return (
              `widerrufen — ${lead} ${says}, dass ${w.whose} am ${r.revoked} widerrufen ` +
              `wurde${why}, spätestens ${w.moment}; es war also bereits zurückgezogen, als ` +
              `${madeNom} erstellt wurde.`
            );
          }
          return (
            `widerrufen — ${lead} ${says}, dass ${w.whose} am ${r.revoked} widerrufen ` +
            `wurde${why}. Nichts, dem tpdf vertraut, bestätigt, wann ${madeNom} erstellt wurde, ` +
            `daher kann tpdf nicht feststellen, ob das davor war.`
          );
        case "unknown":
          return `unbekannt — ${lead} ${says}, dass der Responder ${w.whose} nicht kennt.`;
        case "none":
          return (
            `nicht geprüft — das Dokument enthält keine Widerrufsdaten für ${w.whose}, und tpdf ` +
            `ruft keine ab; ein Zertifikat, das sein Aussteller inzwischen zurückgezogen hat, ` +
            `erscheint daher genauso wie eines, bei dem das nicht der Fall ist.`
          );
        case "unchecked":
          return `nicht geprüft — ${w.gap}. Daraus folgt nichts darüber, ob ${w.whose} widerrufen wurde.`;
      }
    },
    chainName: (authority: boolean): string =>
      authority ? "Widerruf der Kette des Zeitstempeldienstes" : "Widerruf der Kette",
    chainUnjudged: (dropped: number): string =>
      dropped === 1
        ? " 1 Zertifikat weiter oben wurde nicht beurteilt."
        : ` ${dropped} Zertifikate weiter oben wurden nicht beurteilt.`,
    chainGoodAlone: (authority: boolean): string =>
      `nicht widerrufen — wie in der Zeile darüber: Zwischen ${deLeafDat(authority)} und einem ` +
      `Stammzertifikat steht kein weiteres Zertifikat.`,
    chainGoodOne: (authority: boolean, top: string, moment: string): string =>
      `nicht widerrufen — laut den Widerrufsdaten des Dokuments war weder ${deLeaf(authority)} ` +
      `noch ${deIssuing(top)} widerrufen, und sie reichen bis ${moment}.`,
    chainGoodMany: (authority: boolean, count: number, top: string, moment: string): string =>
      `nicht widerrufen — laut den Widerrufsdaten des Dokuments war keines der ${count} ` +
      `Zertifikate von ${deLeafDat(authority)} bis zu ${deIssuingDat(top)} widerrufen, und sie ` +
      `reichen bis ${moment}.`,
    chainTooLong: (authority: boolean, max: number, dropped: number): string =>
      `nicht geprüft — die Kette von ${deLeafDat(authority)} bis zu ihrem Stammzertifikat ist ` +
      `länger als die ${max} Zertifikate, denen tpdf folgt; daher ` +
      (dropped === 1
        ? "wurde 1 Zertifikat weiter oben nicht beurteilt, und es könnte widerrufen worden sein."
        : `wurden ${deCertificates(dropped)} weiter oben nicht beurteilt, und jedes davon könnte ` +
          `widerrufen worden sein.`),
    chainHead: (standing: RevocationStatus, afterMoment: boolean): string => {
      switch (standing) {
        case "revoked":
          return afterMoment ? "nach dem Zeitstempel widerrufen" : "widerrufen";
        case "unknown":
          return "unbekannt";
        case "good":
          return "nicht widerrufen";
        case "none":
        case "unchecked":
          return "nicht geprüft";
      }
    },
    chainSameAbove: (head: string, authority: boolean, atRoot: boolean, unjudged: string): string => {
      const why = atRoot
        ? `Zwischen ${deLeafDat(authority)} und einem Stammzertifikat steht kein weiteres Zertifikat`
        : `Das Zertifikat, das ${deLeaf(authority)} ausgestellt hat, ist nicht im Dokument`;
      return `${head} — wie in der Zeile darüber: ${why}.${unjudged}`;
    },
    chainByLeaf: (head: string, authority: boolean, above: number, unjudged: string): string => {
      const those = above === 1 ? "das Zertifikat" : `die ${deCertificates(above)}`;
      return (
        `${head} — entschieden durch ${deLeaf(authority)} selbst, in der Zeile darüber; für ` +
        `${those} darüber ergibt sich nichts Schlechteres.${unjudged}`
      );
    },
  },
});

/** Every key of a union, to build a record of its words. */
function wordsFor<K extends string, V>(keys: readonly K[], word: (key: K) => V): Record<K, V> {
  return Object.fromEntries(keys.map((key) => [key, word(key)])) as Record<K, V>;
}

const WHYS: readonly Why[] = [
  "format", "range", "unreadable", "certificate", "algorithm", "attributes", "budget", "binding",
];
const DOUBTS: readonly Doubt[] = [
  "incomplete", "root", "dates", "purpose", "rejected", "certificate", "unavailable",
  "timestamping", "not_in_force",
];
const STORES: readonly Store[] = ["mac", "windows", "linux"];
const REASONS: readonly Reason[] = [
  "unspecified", "key_compromise", "ca_compromise", "affiliation_changed", "superseded",
  "cessation_of_operation", "certificate_hold", "remove_from_crl", "privilege_withdrawn",
  "aa_compromise",
];
const GAPS: readonly Gap[] = [
  "unreadable", "bound", "issuer", "signature", "unauthorised", "algorithm", "unsupported",
  "stale", "expired", "dates", "budget",
];

/**
 * Said after every answer that could be read as "this signer is who they say".
 *
 * Stated once and reused, so the one sentence that keeps "intact" from meaning
 * "trustworthy" cannot drift out of one branch while staying in another.
 */
export const TRUST_NOT_CHECKED = TEXT().trustNotChecked;

/** Why a signature was not checked, as a clause that follows "not checked —". */
export const WHY: Record<Why, string> = wordsFor(WHYS, (why) => TEXT().why(why));

/** The computer whose store answered, as the sentence names it. */
export const COMPUTER: Record<Store, string> = wordsFor(STORES, (store) => TEXT().computer(store));

/**
 * Why the store does not vouch, as a clause; `computer` names the machine and
 * `authority` says the chain starts from a timestamp authority's certificate
 * rather than the signer's.
 */
export const DOUBT: Record<Doubt, (computer: string, authority?: boolean) => string> = wordsFor(
  DOUBTS,
  (doubt) => (computer: string, authority = false) => TEXT().doubt(doubt, computer, authority),
);

/**
 * The names of the rows this module words, in the window's language: what
 * `properties.ts` picks a closed card's verdicts by.
 */
export function rowNames() {
  const said = TEXT();
  return {
    integrity: said.integrityName,
    trust: said.trustName,
    revocation: said.revocationName(false),
    chain: said.chainName(false),
    authority: said.authorityName,
    authorityRevocation: said.revocationName(true),
    authorityChain: said.chainName(true),
  };
}

/** A row, with `warn` only where it is set. */
function row(name: string, value: string, warn: boolean): Row {
  return warn ? { name, value, warn: true } : { name, value };
}

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
  const said = TEXT();
  const computer = said.computer(trust.store);
  const why = trust.why ? DOUBT[trust.why](computer) : said.noReason;
  const value = said.trust({
    standing: trust.standing,
    computer,
    why,
    attestedAt: trust.attested_at,
    from,
    until,
  });
  const fine = trust.standing === "trusted" || trust.standing === "trusted_at_timestamp";
  return row(said.trustName, value, !fine);
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
  const said = TEXT();
  const owner = trustFollows ? "" : ` ${said.trustNotChecked}`;
  const why = integrity.why ? WHY[integrity.why] : said.noReason;
  const value = said.integrity(integrity.verdict, how(integrity), appended > 0, owner, why);
  return row(said.integrityName, value, integrity.verdict !== "intact");
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
  const said = TEXT();
  const why = integrity?.why ? WHY[integrity.why] : said.noReason;
  const value = said.timestamp(
    integrity?.verdict ?? null,
    when,
    by,
    document,
    integrity ? how(integrity) : "",
    why,
  );
  return row(said.timestampName, value, integrity?.verdict !== "intact");
}

/**
 * The row that names the PAdES level a signature has the parts of.
 *
 * `null` when the worker named none: a document timestamp, a signature that
 * is not CAdES, and one that does not hold. The level is decided in the worker
 * (`pades.rs`) and only worded here.
 *
 * **Every answer ends by saying what was not done.** The level is read from
 * the parts the document carries. A validator's conformance test is a larger
 * thing, and "B-LTA" with no qualification would be taken for it.
 */
export function padesRow(level: PadesLevel | null | undefined): Row | null {
  if (!level) return null;
  const said = TEXT();
  return { name: said.padesName, value: said.pades(level) };
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
  const said = TEXT();
  const computer = said.computer(trust.store);
  const why = trust.why ? DOUBT[trust.why](computer, true) : said.noReason;
  const value = said.authority({
    standing: trust.standing,
    computer,
    why,
    attestedAt: trust.attested_at,
    from,
    until,
  });
  const fine = trust.standing === "trusted" || trust.standing === "trusted_at_timestamp";
  return row(said.authorityName, value, !fine);
}

/** A revocation reason, as a phrase. */
export const REASON: Record<Reason, string> = wordsFor(REASONS, (reason) => TEXT().reason(reason));

/** Why revocation data led to no conclusion; `moment` as {@link momentPhrase}. */
export const GAP: Record<Gap, (moment: string) => string> = wordsFor(
  GAPS,
  (gap) => (moment: string) => TEXT().gap(gap, moment),
);

/** The moment a revocation was judged at, and whose clock it is. */
export function momentPhrase(revocation: Revocation): string {
  return TEXT().moment(revocation.basis, revocation.moment);
}

/** The signer's or the authority's certificate, as the sentences name it. */
function leafHolder(authority: boolean): string {
  return TEXT().leaf(authority);
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
  const said = TEXT();
  const moment = momentPhrase(revocation);
  const value = said.revocation({
    revocation,
    authority,
    whose,
    moment,
    reason: revocation.reason ? REASON[revocation.reason] : "",
    gap: revocation.why ? GAP[revocation.why](moment) : said.noReason,
  });
  const fine =
    revocation.standing === "good" ||
    (revocation.standing === "revoked" && revocation.after_moment);
  return row(said.revocationName(authority), value, !fine);
}

/** A certificate above the leaf, as the sentences name it. */
export function issuingCertificate(judged: Judged): string {
  return TEXT().issuing(judged.subject_cn || judged.subject);
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
  const warn = !(chain.standing === "good" || (chain.standing === "revoked" && chain.after_moment));
  return row(TEXT().chainName(authority), chainSentence(chain, authority), warn);
}

/**
 * {@link chainRow}'s value, for every chain: the command-line tool's JSON
 * carries a sentence even where the dialog shows no row.
 */
export function chainSentence(chain: Chain, authority = false): string {
  const said = TEXT();
  const above = Math.max(chain.certificates.length - 1, 0);
  const unjudged = chain.dropped === 0 ? "" : said.chainUnjudged(chain.dropped);
  const decided = chain.decided_by;
  if (decided === null) {
    if (chain.standing === "good") {
      const first = chain.certificates[0];
      const moment = first ? momentPhrase(first.revocation) : "";
      const last = chain.certificates[chain.certificates.length - 1];
      const top = last ? last.subject_cn || last.subject : "";
      if (above === 0) return said.chainGoodAlone(authority);
      if (above === 1) return said.chainGoodOne(authority, top, moment);
      return said.chainGoodMany(authority, above + 1, top, moment);
    }
    return said.chainTooLong(authority, MAX_CHAIN, chain.dropped);
  }
  const head = said.chainHead(chain.standing, chain.after_moment);
  if (decided === 0) {
    if (above === 0) return said.chainSameAbove(head, authority, chain.end === "root", unjudged);
    return said.chainByLeaf(head, authority, above, unjudged);
  }
  const judged = chain.certificates[decided];
  if (!judged) return "";
  const shown = revocationRow(judged.revocation, authority, issuingCertificate(judged));
  return `${shown?.value ?? ""}${unjudged}`;
}
