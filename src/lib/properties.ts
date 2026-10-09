/**
 * What a document says about itself, turned into lines a reader can read.
 *
 * The backend half is `docinfo.rs`, which reads the object graph. Everything
 * here is presentation, and it is a module of its own rather than markup inside
 * the dialog for one reason: **most of the decisions in a properties readout are
 * decisions, not layout.** Whether a permission bit means what it says under
 * revision 2, whether a byte range covers the file, whether an absent field is
 * omitted or shown empty --- each has a right answer and a wrong one, and a
 * wrong one here is a confident false statement about a document somebody is
 * about to rely on.
 *
 * So the dialog receives [`Section`]s and prints them. Nothing it does can be
 * wrong; everything that can be wrong is a pure function with a test.
 *
 * ## Nothing here may say a signature is valid
 *
 * `docs/TRAPS.md` is explicit, and the reason is worth restating where the words
 * are actually chosen. tpdf *parses* certificates --- it reads the subject, the
 * issuer, the serial and the validity dates out of the PKCS#7 blob --- and since
 * 2026-09-26 it also tests each signature against the bytes it covers, in the
 * worker, with the answer in `integrity.ts`; since 2026-09-27 it asks the
 * operating system's own trust store whether the signer's certificate chains
 * to a root it trusts; since 2026-09-28 it reads the revocation data the
 * document itself carries, and judges the signer at the time a trusted
 * timestamp attests. That is still a smaller thing than "valid". It fetches
 * nothing, consults no list but the OS's own, and without such a timestamp
 * cannot tell whether a certificate was in date when it was used --- so the
 * trust row names the store, and the revocation row says, when the document
 * carries no revocation data, that nothing was checked.
 *
 * The vocabulary carries it: a signer's name, reason, location and date are
 * introduced as claimed, and so is everything the certificate says. The
 * unhedged sentences in the section are the byte-range one, `self_issued`, and
 * the integrity row, which are the things measured --- and the integrity row
 * says, every time it could be read as more, that the key's owner was not
 * checked. [`NOT_CHECKED`] is shown whenever a signature is, and
 * `properties.test.ts` asserts that no rendered line ever uses a word that would
 * read as a verdict about the signer.
 */

import {
  authorityRow,
  padesRow,
  chainRow,
  integrityRow,
  revocationRow,
  timestampRow,
  trustRow,
  type Chain,
  type Integrity,
  type PadesLevel,
  type Revocation,
  type Trust,
} from "./integrity";

/** One `/Info` entry, as `docinfo.rs` reports it. */
export interface Field {
  name: string;
  value: string;
  standard: boolean;
}

/** One permission, named rather than a bit. */
export interface Permission {
  what: string;
  allowed: boolean;
}

/** The document's encryption, as `docinfo.rs` reports it. */
export interface Encryption {
  method: string;
  revision: number;
  opened_without_password: boolean;
  permissions: Permission[];
}

/**
 * What an appended revision changed. Mirrors `docinfo::Appendix`.
 *
 * Nothing here is a verdict, and the rendering below has to keep it that way:
 * naming `/DSS` as validation data is translation, in the same voice
 * {@link certificationOf} turns a DocMDP level into words. Calling an append
 * harmless would not be.
 */
export interface Appendix {
  added: number;
  replaced: number;
  /**
   * Objects the signed revision had that a later cross-reference section
   * marks free, or that the document no longer has. Nothing is written for
   * a removal, so it is in neither count above.
   */
  removed: number;
  kinds: string[];
  catalog_gained: string[];
  pages_touched: number;
  /**
   * The touched pages rewritten only to list a new signature or timestamp
   * field, with nothing the page draws from added or changed: its content
   * streams, its resources and the annotations it had. `timestamp` says every
   * field the page gained holds a document timestamp.
   */
  pages_listing: { page: number; timestamp: boolean }[];
  unread: boolean;
}

/** One signature field, as `docinfo.rs` reports it. */
export interface Signature {
  field: string;
  signed: boolean;
  handler: string;
  kind: string;
  name: string;
  reason: string;
  location: string;
  when: string;
  covers_whole_file: boolean;
  covered_bytes: number;
  appended_bytes: number;
  appendix: Appendix | null;
  certification: number;
  certificate: Certificate | null;
  timestamp: Timestamp | null;
  /** Whether it still covers what it was made over; `null` when unsigned. */
  integrity: Integrity | null;
  /**
   * Whether the signer's certificate chains to a root the OS store trusts;
   * `null` unless `integrity` is intact or weak.
   */
  trust: Trust | null;
  /**
   * What the document's own revocation data says about the signer's
   * certificate (a document timestamp's: its authority's); `null` exactly
   * when `trust` is.
   */
  revocation: Revocation | null;
  /**
   * The same, for every certificate from the signer's up to its root, and
   * which one decides it; `null` exactly when `revocation` is.
   */
  revocation_chain: Chain | null;
  /**
   * The PAdES baseline level this signature has the parts of, or `null`.
   * Optional so a reply from before 2026-10-02 still reads.
   */
  pades?: PadesLevel | null;
}

/**
 * What a timestamp authority attested, as `docinfo::Timestamp` reports it.
 *
 * A signature's own date is whatever the signer's computer clock read. This is
 * a different party's statement, checked in the worker since 2026-09-28:
 * `integrity` says whether the token is sound and covers this signature,
 * `trust` whether the store vouches for the authority, and `attested` whether
 * `when` is anything more than what the token states.
 */
export interface Timestamp {
  /** The time the token states. Attested only when `attested` is true. */
  when: string;
  authority: Certificate | null;
  /** The token's verdict; `null` only for a token nothing checked. */
  integrity: Integrity | null;
  /** The authority's standing, for timestamping; `null` unless `attested`. */
  trust: Trust | null;
  /** Whether the verdict is intact or weak, decided in the worker. */
  attested: boolean;
  /** The authority's revocation, at the time the token states; `null` unless `trust`. */
  revocation: Revocation | null;
  /** The same, for the authority's whole chain; `null` exactly when `revocation` is. */
  revocation_chain: Chain | null;
}

/**
 * What the signing certificate says.
 *
 * Mirrors `docinfo::Certificate`. Nothing here is verified --- see
 * [`NOT_CHECKED`], which is shown wherever these rows are.
 */
export interface Certificate {
  subject: string;
  subject_cn: string;
  issuer: string;
  issuer_cn: string;
  serial: string;
  from: string;
  until: string;
  self_issued: boolean;
  chain: number;
  matched_signer: boolean;
  /**
   * What the issuer says the key is for. `null` is a certificate carrying no
   * key usage extension, which places no limit; an empty array is one that
   * limits it to nothing. Different claims, kept different.
   */
  key_usage: string[] | null;
  /** Extended key usage, named where known and given as an OID where not. */
  extended_usage: string[] | null;
  /** Whether it says it may issue other certificates. `null` when unstated. */
  authority: boolean | null;
  /** Extensions present but not decodable. */
  extensions_unread: number;
}

/** What could not be read. */
export interface Limits {
  locked: boolean;
  fields_dropped: number;
  values_clipped: number;
  signatures_dropped: number;
  unreadable: number;
  certificates_unread: number;
}

/** Everything a document says about itself. Mirrors `docinfo::Properties`. */
export interface Properties {
  version: string;
  bytes: number;
  pages: number;
  revisions: number;
  fields: Field[];
  encryption: Encryption | null;
  signatures: Signature[];
  tagged: boolean | null;
  language: string;
  attachments: number | null;
  xmp: Xmp | null;
  limits: Limits;
  scan_ms: number;
}

/**
 * The XMP metadata packet, as `xmp.rs` reports it.
 *
 * `null` is a document with no packet. `unread` on a packet that is there is
 * tpdf's failure, not the document's silence, and the two never share a row.
 */
export interface Xmp {
  bytes: number;
  conformance: string[];
  unread: boolean;
}

/** One line of the readout. */
export interface Row {
  name: string;
  value: string;
  /** Set on a row that reports something the reader should notice. */
  warn?: boolean;
}

/** One block of the readout, with its heading. */
export interface Section {
  title: string;
  rows: Row[];
  /** Shown under the rows, in smaller type, when the block needs a caveat. */
  note?: string;
  /**
   * Set on each signature when the document holds more than one: the block is
   * then a card that opens, and this is what its closed face says. See
   * {@link briefly}.
   */
  card?: Brief[];
}

/** One phrase on a closed card's face; `warn` as on a {@link Row}. */
export interface Brief {
  text: string;
  warn?: boolean;
}

/**
 * The sentence shown wherever a signature is.
 *
 * Stated once and reused, so the honest disclaimer cannot drift out of one place
 * it is needed while staying in another.
 */
export const NOT_CHECKED =
  "tpdf checks that the bytes a signature covers are unchanged, that the " +
  "signature matches the key in its certificate, and whether that certificate " +
  "chains to a root this computer's own trust store trusts, which is not " +
  "Adobe's list most signed PDFs are made against. It fetches nothing to check a signature: " +
  "revocation is judged only from data the document itself carries, a document " +
  "carrying none is not checked for it, and no missing certificate is fetched. " +
  "What a certificate states its key is for is the issuer's own word. A " +
  "timestamp's own signature and whether it covers this signature are checked, " +
  "and its authority is asked about as the signer is; only when the timestamp is " +
  "intact and its authority trusted is the signer's certificate judged at the " +
  "time it attests rather than now. Nothing here means the signature is valid.";

/**
 * Words that would read as a verdict on a signature.
 *
 * Asserted against every rendered line, which is a weaker check than reading the
 * source and a stronger one than trusting it: a phrase added later to a template
 * string, or interpolated in from a document's own `/Reason`, is caught by this
 * and by nothing else.
 *
 * `"trusted"` is absent on purpose --- it occurs in [`NOT_CHECKED`], which is
 * the one place denying it is the point. The test exempts that string by
 * identity rather than by pattern.
 */
export const VERDICT_WORDS = ["valid", "verified", "authentic", "genuine"];

/** A byte count, with a rounded size beside it once it is worth having. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown";
  const exact = `${Math.round(bytes).toLocaleString("en-US")} bytes`;
  if (bytes < 1024) return exact;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal below ten, none above: 8.7 MB is worth the digit and 873 KB is
  // not, and "873.4 KB" beside an exact byte count is two spurious digits.
  const rounded = value < 10 ? value.toFixed(1) : Math.round(value).toString();
  return `${rounded} ${units[unit]} (${exact})`;
}

/**
 * What a DocMDP level permits, in the specification's own terms.
 *
 * Reported and never acted on --- `docs/TRAPS.md` records that a validator
 * rejects edits every one of these levels permits, so this describes the
 * document's intent rather than what would actually survive.
 */
export function certificationOf(level: number): string {
  switch (level) {
    case 1:
      return "certified, no changes permitted";
    case 2:
      return "certified, form filling permitted";
    case 3:
      return "certified, form filling and comments permitted";
    default:
      return "";
  }
}

/**
 * The one thing about a signature that was checked rather than claimed.
 *
 * ## Why this does not report `bytes - covered_bytes`
 *
 * It did until 2026-08-25, and the number was arithmetically right and read as
 * an accusation. A detached signature cannot cover its own `/Contents` hex
 * string -- the value would have to contain its own hash -- so **every signed
 * PDF ever written has that hole**, and it is large: on the DocuSign contract
 * this was reported from, 65,536 of the 74,637 uncovered bytes were the
 * container, and the part actually written after signing was 9,101 bytes of
 * LTV validation data. The row said *73 KB lie outside the signed range*, which
 * a technical reader corrects and a non-technical one is alarmed by, and
 * neither reaction is the right one.
 *
 * So the headline is {@link Signature.appended_bytes}: what was added **after**
 * the signature was made. That is the number that distinguishes one document
 * from another, and the container is named rather than counted.
 *
 * ## What it still does not say
 *
 * **What was appended.** Nine kilobytes of validation data and nine kilobytes
 * of new page content are the same row here, and they are not the same fact.
 * Telling them apart means parsing the appended revision's objects and deciding
 * which additions a signature tolerates -- a real piece of work, and one that
 * would be making a *verdict*, which nothing in this panel does yet. Until then
 * the wording says when, not what, and does not imply the answer.
 */
export function coverageOf(signature: Signature, bytes: number): Row {
  if (signature.covered_bytes === 0) {
    return {
      name: "Covers",
      value: "no byte range stated, so nothing could be checked",
      warn: true,
    };
  }
  if (signature.covers_whole_file) {
    // Not "the whole file", which was the old wording and is false of every
    // signature: the container is excluded and always will be. Saying so once,
    // here, is what stops the appended case below reading as though the
    // container were the reader's problem too.
    return {
      name: "Covers",
      value: "the whole file, except the signature container it cannot cover",
    };
  }
  if (signature.appended_bytes > 0) {
    return {
      name: "Covers",
      value:
        `everything up to the signature, and ${formatBytes(signature.appended_bytes)} ` +
        "were appended afterwards",
      warn: true,
    };
  }
  // The range stops at the last byte and still does not begin at the first, so
  // the hole is at the front of the file rather than after it. Rare, and worth
  // its own sentence: an unsigned prologue is not an appendix and the two want
  // different suspicion.
  const short = Math.max(0, bytes - signature.covered_bytes);
  return {
    name: "Covers",
    value: `not the whole file — ${formatBytes(short)} lie outside the signed range`,
    warn: true,
  };
}

/**
 * What the XMP packet claims about the standards the document conforms to.
 *
 * Nothing else in a PDF says this: PDF/A, PDF/UA and PDF/X are declared in XMP
 * or not at all. Measured at 8 of 41 real documents, seven PDF/UA-1 and one
 * PDF/A-3B, which is why this is the one thing read out of the packet.
 *
 * **A claim, not a verdict**, and the wording carries that: tpdf does not
 * validate a document against PDF/A, and a file stating PDF/A-3B may break it
 * in twenty ways. The row says *states*, in the same voice as the signature
 * rows and for the same reason.
 *
 * Silence is deliberate for the common case. Most documents claim nothing, and
 * most carry no packet at all; a row saying so on every document is noise. The
 * one thing that always speaks is a packet that could not be read, because that
 * is tpdf failing rather than the document declining to say anything.
 */
export function conformanceRows(xmp: Xmp | null): Row[] {
  if (!xmp) return [];

  const rows: Row[] = [];
  if (xmp.conformance.length > 0) {
    rows.push({
      name: "States conformance",
      value: `${xmp.conformance.join(", ")} — the document's own claim, which tpdf does not check`,
    });
  }
  if (xmp.unread) {
    rows.push({
      name: "Metadata",
      value:
        "the document carries an XMP packet that could not be read, so what it " +
        "states about itself is unknown",
      warn: true,
    });
  }
  return rows;
}

/**
 * The lines that come out of the signing certificate.
 *
 * Empty when the blob carried none, which is a fact about the document; that it
 * could not be *read* is a fact about tpdf and is reported through
 * `limits.certificates_unread` instead, in the notice at the foot of the dialog.
 */
export function certificateRows(signature: Signature): Row[] {
  const certificate = signature.certificate;
  if (!certificate) return [];

  const rows: Row[] = [];
  const named = certificate.subject_cn || certificate.subject;
  rows.push({
    name: "Certificate names",
    value: named || "a certificate with no name in it",
    warn: !named,
  });

  if (certificate.self_issued) {
    rows.push({
      name: "Issued by",
      value: "itself — self-issued, so no other party vouched for this name",
    });
  } else {
    const by = certificate.issuer_cn || certificate.issuer;
    if (by) rows.push({ name: "Issued by", value: by });
  }

  if (certificate.from && certificate.until) {
    rows.push({
      name: "Certificate runs",
      value: `${certificate.from} to ${certificate.until}`,
    });
  }
  if (certificate.serial) {
    rows.push({ name: "Serial", value: certificate.serial });
  }

  // What the certificate says its key is for. Shown even when nothing is
  // stated, because *nothing stated* is itself the issuer placing no limit ---
  // and a reader who sees no row cannot tell that from a row that was dropped.
  //
  // Not a verdict, and the wording is what keeps it one: the extension
  // constrains the key, and only a chain built to a trusted issuer makes that
  // constraint mean anything. The trust row is where that chain is reported;
  // this row stays the issuer's word, as NOT_CHECKED says.
  const usage = certificate.key_usage;
  rows.push({
    name: "Key is for",
    value:
      usage === null
        ? "not stated — the certificate places no limit on what the key is used for"
        : usage.length > 0
          ? usage.join(", ")
          : "nothing — the certificate names no use for its own key",
    warn: usage !== null && usage.length === 0,
  });

  const purposes = certificate.extended_usage;
  if (purposes !== null) {
    rows.push({
      name: "Issued for",
      value:
        purposes.length > 0
          ? purposes.join(", ")
          : "nothing — the certificate names no purpose",
      warn: purposes.length === 0,
    });
  }

  // Only when it claims to be one. `false` and *unstated* are the ordinary
  // cases and both read the same way, so a row saying so on every document
  // would be noise; a signer that is also an authority is worth a line.
  if (certificate.authority === true) {
    rows.push({
      name: "Also an authority",
      value: "this certificate says it may issue other certificates",
      warn: true,
    });
  }

  if (certificate.extensions_unread > 0) {
    rows.push({
      name: "Extensions",
      value: `${certificate.extensions_unread} could not be read, so what they state is unknown`,
      warn: true,
    });
  }
  if (certificate.chain > 1) {
    rows.push({
      name: "Certificates present",
      // A document timestamp's own certificate is its authority's.
      value: `${certificate.chain}, of which this is the ${
        signature.kind === DOCUMENT_TIMESTAMP ? "authority's" : "signer's"
      }`,
    });
  }
  if (!certificate.matched_signer) {
    rows.push({
      name: "Certificates present",
      value:
        "one, and the signature does not point at it — shown because there " +
        "is nothing else it could be",
      warn: true,
    });
  }

  // Two names for one signer, from two places, that disagree. Neither is
  // checked, so this is not an accusation --- it is the one thing a reader
  // could not work out from the rows above without comparing them by eye.
  const typed = signature.name.trim();
  const inCert = (certificate.subject_cn || certificate.subject).trim();
  if (typed && inCert && typed.toLowerCase() !== inCert.toLowerCase()) {
    rows.push({
      name: "Names disagree",
      value: `the certificate says ${inCert}, the document says ${typed}`,
      warn: true,
    });
  }

  return rows;
}

/**
 * What was appended after a signature, in words rather than in bytes.
 *
 * ## Why this row exists
 *
 * {@link coverageOf} says *when* --- so many bytes were written after this
 * signature --- and stops there, deliberately, because until 2026-08-25 nothing
 * could say more. The gap is not small. Two documents measured, both appending
 * about nine kilobytes:
 *
 * - a DocuSign contract: `/DSS`, a `/VRI` and seven streams, and the catalog
 *   gained `/DSS`. That is **validation data** --- the certificates and
 *   revocation responses a signature needs to still verify in ten years' time.
 *   Adobe does not treat it as a modification.
 * - `incr-two-signers.pdf`: a `/Sig`, an `/Annot/Widget` and a rewritten
 *   `/Page`. That is **a second person signing**.
 *
 * Same size, and a reader shown only the size would read the first as the
 * second. That is exactly what happened: the row was quoted in a letter as
 * evidence of unsigned content when what it described was the signature's own
 * container plus routine LTV data.
 *
 * ## Where the line is drawn
 *
 * Translating `/DSS` into "signature validation data" is the same act as
 * {@link certificationOf} turning a DocMDP level into a sentence: the file's own
 * vocabulary, in words a reader has. What this must not do is conclude. It never
 * says an append was harmless, permitted, or a modification --- those are
 * verdicts, and `docinfo::Appendix`'s own note is that nothing in it makes one.
 *
 * The page count is the one thing separated out, because it is the difference a
 * reader can act on. It is still stated as what it is: a page object was written
 * again. A page can be rewritten for reasons that change nothing on screen, and
 * the wording does not claim otherwise.
 */
export function appendixRow(signature: Signature): Row | null {
  const appendix = signature.appendix;
  if (!appendix) return null;
  if (appendix.unread) {
    // A fact about tpdf, in the voice `NOT_CHECKED` uses for the rest of this
    // panel: what could not be read is never reported as what is not there.
    return {
      name: "Appended",
      value: "something, but its contents could not be read",
      warn: true,
    };
  }

  const what = describeAppendix(appendix);
  const pages = describePages(appendix);
  const arrived = appendix.added + appendix.replaced;
  const named = appendix.catalog_gained.includes("DSS") || appendix.kinds.includes("Sig");
  let value: string;
  if (appendix.removed === 0) {
    value = `${what}, and ${pages}`;
  } else if (arrived === 0 && !named) {
    // Nothing arrived and something went: the removal is the whole of it,
    // and "0 objects" in front of it would be noise.
    const removed =
      appendix.removed === 1 ? "1 object was removed" : `${appendix.removed} objects were removed`;
    value = `${removed}, and ${pages}`;
  } else {
    const removed =
      appendix.removed === 1 ? "1 object removed" : `${appendix.removed} objects removed`;
    value = `${what}, with ${removed}, and ${pages}`;
  }
  return { name: "Appended", value, warn: true };
}

/**
 * What the append did to pages.
 *
 * "1 page was rewritten" under a signature reads as a change to the page, and
 * the commonest reason a page object is written again changes nothing on it: a
 * signature or timestamp field has to be listed in the page's annotations. So
 * when the worker proved that is all that happened to every touched page ---
 * `docinfo::read_appendix` holds the conditions --- the row says that, and
 * names the page. **Every other rewrite keeps the bare wording**, including a
 * mix of the two, so that sentence still means what it meant: a page object
 * changed and tpdf does not know why.
 */
function describePages(appendix: Appendix): string {
  const touched = appendix.pages_touched;
  if (touched === 0) return "no page was rewritten";
  const listing = appendix.pages_listing;
  if (listing.length !== touched) {
    return touched === 1 ? "1 page was rewritten" : `${touched} pages were rewritten`;
  }
  const field = listing.every((entry) => entry.timestamp)
    ? "timestamp field"
    : listing.some((entry) => entry.timestamp)
      ? "signature or timestamp field"
      : "signature field";
  if (touched === 1) {
    return (
      `a ${field} was added to page ${listing[0]!.page}'s annotations ` +
      `(the page's content is unchanged)`
    );
  }
  const numbers = listing.map((entry) => `${entry.page}`);
  const pages = `${numbers.slice(0, -1).join(", ")} and ${numbers.at(-1)}`;
  return (
    `a ${field} was added to the annotations of pages ${pages} ` +
    `(their content is unchanged)`
  );
}

/**
 * The appendix in a phrase, most specific reading first.
 *
 * Ordered rather than combined, because the readings are not equally
 * informative and a reader takes the first clause. `/DSS` arriving in the
 * catalog is the one unambiguous signal here --- it is what an LTV append *is*,
 * one key rather than a guess over fifteen objects --- so it leads. A `/Sig`
 * among the objects is next, and everything else falls through to the file's
 * own names, which is honest about having no better word for it.
 */
function describeAppendix(appendix: Appendix): string {
  if (appendix.catalog_gained.includes("DSS")) {
    return "the certificates and revocation records a signature needs to be checked later";
  }
  if (appendix.kinds.includes("Sig")) {
    return "another signature";
  }
  const objects =
    appendix.added + appendix.replaced === 1
      ? "1 object"
      : `${appendix.added + appendix.replaced} objects`;
  if (appendix.kinds.length === 0) return objects;
  // The file's own vocabulary, unglossed. A reader who does not know what a
  // `/StructTreeRoot` is learns nothing from this, and a reader who does learns
  // everything -- which beats inventing a phrase for a shape nobody measured.
  return `${objects}: ${appendix.kinds.join(", ")}`;
}

/**
 * The `/SubFilter` of a document timestamp (PDF 2.0 §12.8.5): a signature field
 * whose value is a timestamp token over the document, and nobody's signature.
 */
const DOCUMENT_TIMESTAMP = "ETSI.RFC3161";

/** Every line of one signature. */
export function signatureRows(signature: Signature, bytes: number): Row[] {
  if (!signature.signed) {
    return [{ name: "Status", value: "a signature field, not yet signed" }];
  }

  const rows: Row[] = [];
  const claimed = (name: string, value: string): void => {
    if (value) rows.push({ name, value });
  };

  // The answer first, because it is what a reader opening this about a signed
  // document came to ask: has it been changed. Everything below is who the
  // signature says signed it, which only matters once this is known.
  //
  // Then whose key it is, as far as this computer's trust store can say ---
  // directly under, because it is the question the first answer leaves open.
  //
  // A document timestamp's certificate is its authority's and names no person,
  // so its standing is said as an authority's, in that row's own words.
  const document = signature.kind === DOCUMENT_TIMESTAMP;
  const trusted = (document ? authorityRow : trustRow)(
    signature.trust,
    signature.certificate?.from,
    signature.certificate?.until,
  );
  const verdict = integrityRow(signature.integrity, signature.appended_bytes, !!trusted);
  if (verdict) rows.push(verdict);
  if (trusted) rows.push(trusted);
  // Then whether its issuer has withdrawn it, as far as the document's own
  // revocation data says. A document timestamp's signer is its authority.
  const withdrawn = revocationRow(signature.revocation, document);
  if (withdrawn) rows.push(withdrawn);
  // Then the certificates above it, which it stands or falls with: shown only
  // when there is one, or the chain ran past what tpdf follows.
  const above = chainRow(signature.revocation_chain ?? null, document);
  if (above) rows.push(above);

  // The certificate goes above what the signer typed, because a reader opening
  // this asks who signed it and these are two different answers to that. Which
  // is worth more depends on `self_issued` and is not ours to rank: a name in a
  // self-issued certificate is exactly as self-asserted as `/Name` is.
  rows.push(...certificateRows(signature));

  claimed("Signer typed", signature.name);
  claimed("Reason given", signature.reason);
  claimed("Location given", signature.location);
  claimed("Date given", signature.when);

  // Directly under the signer's own date, because the two answer the same
  // question from different places and the labels are what tell them apart:
  // `/M` is written by the machine doing the signing and nothing checks it,
  // while a token is a third party's statement. Naming the authority is the
  // whole value of the row --- an attested time with no attester named is a
  // number a reader has no way to weigh --- and the verdict is in the same
  // sentence, because a time and whether it is attested are one answer.
  const stamp = signature.timestamp;
  if (stamp?.when) {
    const by = stamp.authority?.subject_cn || stamp.authority?.subject || "";
    rows.push(timestampRow(stamp.when, by, stamp.integrity, document));
    // A document timestamp's authority is the field's own signer: its standing
    // and its revocation are the rows above, and are not said twice.
    const authority = document
      ? null
      : authorityRow(stamp.trust, stamp.authority?.from, stamp.authority?.until);
    if (authority) rows.push(authority);
    const lapsed = document ? null : revocationRow(stamp.revocation ?? null, true);
    if (lapsed) rows.push(lapsed);
    const lapsedAbove = document ? null : chainRow(stamp.revocation_chain ?? null, true);
    if (lapsedAbove) rows.push(lapsedAbove);
  }
  rows.push(coverageOf(signature, bytes));
  // Directly under Covers, which is the row it completes: that one says how much
  // was appended and this one says what it was.
  const appended = appendixRow(signature);
  if (appended) rows.push(appended);

  const level = certificationOf(signature.certification);
  if (level) rows.push({ name: "Certification", value: level });

  const how = [signature.handler, signature.kind].filter(Boolean).join(" / ");
  claimed("Format", how);
  // Under Format, which names the encoding: this names what that encoding,
  // with the timestamp and revocation rows above, adds up to.
  const pades = padesRow(signature.pades);
  if (pades) rows.push(pades);

  return rows;
}

/**
 * How many signatures, document timestamps and empty fields, counted apart.
 *
 * Apart, because a document timestamp is a signature field in the file and
 * nobody's signature: "2 signatures" over one of each says a second party
 * signed. `cli/verify.rs`'s `counted` is the command line's half.
 */
export function countedSignatures(signatures: Signature[]): string {
  const stamps = signatures.filter((s) => s.signed && s.kind === DOCUMENT_TIMESTAMP).length;
  const empty = signatures.filter((s) => !s.signed).length;
  const signed = signatures.length - stamps - empty;
  const parts = [
    signed === 1 ? "1 signature" : signed > 1 ? `${signed} signatures` : "",
    stamps === 1 ? "1 document timestamp" : stamps > 1 ? `${stamps} document timestamps` : "",
    empty === 1 ? "1 empty signature field" : empty > 1 ? `${empty} empty signature fields` : "",
  ].filter(Boolean);
  return parts.length > 1
    ? `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}`
    : (parts[0] ?? "");
}

/**
 * What a closed card says: whose certificate, then the leading word of each
 * verdict row.
 *
 * A closed card hides its rows, so every row that carries a verdict has its
 * verdict on the face, and a warning there stays a warning. The words are cut
 * from the rows rather than written again: a second wording of "intact" is a
 * second place for it to be wrong.
 */
export function briefly(signature: Signature, rows: Row[]): Brief[] {
  if (!signature.signed) return [{ text: "not yet signed" }];
  const who = signature.certificate?.subject_cn || signature.certificate?.subject || "";
  const out: Brief[] = who ? [{ text: who }] : [];
  // Its own certificate's rows only. A signature's timestamp has an authority
  // with a standing too, and "trusted · trusted" on a face says nothing about
  // which is whose; that row is one click away.
  const own = signature.kind === DOCUMENT_TIMESTAMP ? AUTHORITY_VERDICTS : SIGNER_VERDICTS;
  for (const row of rows) {
    if (row.name !== "Integrity" && !own.has(row.name)) continue;
    // Up to the dash that separates a verdict from its reason; a row with no
    // dash is short enough to be said whole.
    const text = row.value.split(" — ")[0]!.replace(/\.$/, "");
    out.push(row.warn ? { text, warn: true } : { text });
  }
  // Last, and the level alone: a reader asked "is this B-LTA?" should not have
  // to open the card and scroll to its last row. The row says what it rests on.
  if (signature.pades) out.push({ text: signature.pades });
  return out;
}

/** The rows about a signer's certificate whose leading words are a verdict. */
const SIGNER_VERDICTS = new Set(["Trust", "Revocation", "Chain revocation"]);
/** The same rows as a document timestamp names them. */
const AUTHORITY_VERDICTS = new Set([
  "Timestamp authority",
  "Authority revocation",
  "Authority chain revocation",
]);

/**
 * The whole readout, in the order a reader wants it.
 *
 * Order is a decision and not a small one: what a reader opens this dialog to
 * find is at the top. Signatures come before the file's own statistics because
 * a document that is signed is a document whose signature is the reason anybody
 * asked, and encryption comes before both when it is the thing stopping them.
 */
export function sections(properties: Properties): Section[] {
  const locked: Section[] = properties.limits.locked
    ? [
        {
          title: "Locked",
          rows: [
            {
              name: "Contents",
              value: "encrypted, and no password has been given",
              warn: true,
            },
          ],
          note:
            "Everything below comes from the file's structure, which is readable " +
            "without the password. Nothing inside the document could be read at " +
            "all, so its properties, signatures and structure are not missing — " +
            "they were never seen.",
        },
      ]
    : [];

  // More than one, and each is a card under a line that counts them: two
  // blocks of twenty rows under two small headings read as one long signature.
  const several = properties.signatures.length > 1;
  const signatures: Section[] = properties.signatures.map((signature) => {
    // Headed as what it is: a document timestamp under "Signature" reads as a
    // second party having signed, and nobody did.
    const what = signature.kind === DOCUMENT_TIMESTAMP ? "Document timestamp" : "Signature";
    const title = signature.field ? `${what} — ${signature.field}` : what;
    const rows = signatureRows(signature, properties.bytes);
    // The disclaimer goes on a signature that exists, and not on an empty field
    // waiting for one --- there is nothing there to be wrong about.
    return signature.signed ? { title, rows, note: NOT_CHECKED } : { title, rows };
  });
  if (several) {
    for (const [at, section] of signatures.entries()) {
      section.card = briefly(properties.signatures[at]!, section.rows);
    }
  }
  const counted: Section[] = several
    ? [{ title: countedSignatures(properties.signatures), rows: [] }]
    : [];

  const named = properties.fields.map((field) => ({
    name: labelFor(field.name),
    value: field.value,
  }));
  const described: Section[] =
    named.length > 0 ? [{ title: "Described as", rows: named }] : [];

  const security: Section[] = [];
  if (properties.encryption) {
    const stated = properties.encryption;
    const rows: Row[] = [
      { name: "Method", value: `${stated.method}, revision ${stated.revision}` },
    ];
    for (const permission of stated.permissions) {
      rows.push({
        name: permission.what,
        value: permission.allowed ? "allowed" : "not allowed",
        warn: !permission.allowed,
      });
    }
    security.push({
      title: "Security",
      rows,
      note:
        "These are the document's stated restrictions. Any application may " +
        "ignore them — they are a request, not an enforcement.",
    });
  }

  const rows: Row[] = [
    { name: "Pages", value: properties.pages.toLocaleString("en-US") },
    { name: "Size", value: formatBytes(properties.bytes) },
    { name: "PDF version", value: properties.version || "not stated" },
  ];
  if (properties.revisions > 1) {
    rows.push({ name: "Revisions", value: `${properties.revisions}` });
  }
  if (properties.language) {
    rows.push({ name: "Language", value: properties.language });
  }
  if (properties.tagged !== null) {
    rows.push({
      name: "Tagged",
      value: properties.tagged
        ? "yes — the document states its own reading order"
        : "no — reading order is inferred from the layout",
    });
  }
  if (properties.attachments !== null && properties.attachments > 0) {
    rows.push({
      name: "Attachments",
      value: `${properties.attachments} embedded file${properties.attachments === 1 ? "" : "s"}`,
    });
  }
  rows.push(...conformanceRows(properties.xmp));
  const file: Section = { title: "File", rows };

  const missed = limitRows(properties.limits);
  const cut: Section[] =
    missed.length > 0
      ? [
          {
            title: "Not fully read",
            rows: missed,
            note:
              "What is shown above is correct. It is not complete, and this says " +
              "which part is missing rather than leaving the readout looking whole.",
          },
        ]
      : [];

  // The order, in one line, on purpose. It was eight `push` calls spread over
  // ninety, and an ordering spread over ninety lines is one nothing can be
  // aimed at: the mutation written to prove the order assertion could fail
  // *removed* the signature section instead of moving it, so the disclaimer
  // test went red and the order test never ran. A single expression is both
  // readable and mutable --- swapping two of these is one edit.
  return [...locked, ...counted, ...signatures, ...described, ...security, file, ...cut];
}

/** A reader-facing label for an `/Info` key, which is written for a machine. */
function labelFor(key: string): string {
  switch (key) {
    case "CreationDate":
      return "Created";
    case "ModDate":
      return "Modified";
    default:
      // A custom key is the document's own word and is shown as written ---
      // splitting it on case would turn `/SourceModified` into something the
      // document does not say.
      return key;
  }
}

/** What was cut, one line each, so a partial readout says so. */
export function limitRows(limits: Limits): Row[] {
  const rows: Row[] = [];
  const say = (name: string, count: number, what: string): void => {
    if (count > 0) rows.push({ name, value: `${count} ${what}`, warn: true });
  };
  say("Properties", limits.fields_dropped, "were not read");
  say("Values", limits.values_clipped, "were shortened");
  say("Signature fields", limits.signatures_dropped, "were not read");
  say("Entries", limits.unreadable, "could not be read at all");
  // Phrased as what tpdf could not do, not as something the document lacks ---
  // a signature whose certificate went unread must not read like one that has
  // none, which is the whole reason this is counted separately.
  say(
    "Certificates",
    limits.certificates_unread,
    "were present but could not be read",
  );
  return rows;
}
