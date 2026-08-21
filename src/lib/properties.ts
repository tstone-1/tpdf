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
 * are actually chosen. tpdf now *parses* certificates --- it reads the subject,
 * the issuer, the serial and the validity dates out of the PKCS#7 blob --- and
 * that is a smaller thing than it sounds. It has **no trust store**, does not
 * build a chain, does not check a revocation list, and never tests the signature
 * against the bytes it covers. So it knows what the document claims, what the
 * certificate claims, and two structural facts it checked itself.
 *
 * Reading a certificate is not verifying one, and the gap between those is
 * exactly where a reader would be misled. The vocabulary carries it: a signer's
 * name, reason, location and date are introduced as claimed, and so is
 * everything the certificate says. The only unhedged sentences in the section
 * are the byte-range one and `self_issued`, which are the two things measured.
 * [`NOT_CHECKED`] is shown whenever a signature is, and
 * `properties.test.ts` asserts that no rendered line ever uses a word that would
 * read as a verdict.
 */

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
  certification: number;
  certificate: Certificate | null;
  timestamp: Timestamp | null;
}

/**
 * What a timestamp authority attested, as `docinfo::Timestamp` reports it.
 *
 * A signature's own date is whatever the signer's computer clock read. This is
 * a different party's statement, and it is still unverified --- see
 * [`NOT_CHECKED`].
 */
export interface Timestamp {
  when: string;
  authority: Certificate | null;
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
}

/**
 * The sentence shown wherever a signature is.
 *
 * Stated once and reused, so the honest disclaimer cannot drift out of one place
 * it is needed while staying in another.
 */
export const NOT_CHECKED =
  "tpdf reads what the signature and its certificate say. It does not check " +
  "the signature against the bytes it covers, build a chain to an issuer it " +
  "trusts, look for a revocation, or ask whether the certificate was in date " +
  "when it was used. What a certificate states its key is for is the issuer's " +
  "own word, unchecked for the same reason. Nor is a timestamp checked: its " +
  "own signature, the authority behind it, and whether it covers this " +
  "signature at all are all unexamined. Nothing here means the signature is " +
  "valid.";

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

/** The one thing about a signature that was checked rather than claimed. */
export function coverageOf(signature: Signature, bytes: number): Row {
  if (signature.covered_bytes === 0) {
    return {
      name: "Covers",
      value: "no byte range stated, so nothing could be checked",
      warn: true,
    };
  }
  if (signature.covers_whole_file) {
    return { name: "Covers", value: "the whole file" };
  }
  const short = Math.max(0, bytes - signature.covered_bytes);
  return {
    name: "Covers",
    value: `not the whole file --- ${formatBytes(short)} lie outside the signed range`,
    warn: true,
  };
}

/**
 * The lines that come out of the signing certificate.
 *
 * Empty when the blob carried none, which is a fact about the document; that it
 * could not be *read* is a fact about tpdf and is reported through
 * `limits.certificates_unread` instead, in the notice at the foot of the dialog.
 */
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
      value: `${xmp.conformance.join(", ")} --- the document's own claim, which tpdf does not check`,
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
      value: "itself --- self-issued, so no other party vouched for this name",
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
  // constraint mean anything. tpdf builds no chain, which NOT_CHECKED says.
  const usage = certificate.key_usage;
  rows.push({
    name: "Key is for",
    value:
      usage === null
        ? "not stated --- the certificate places no limit on what the key is used for"
        : usage.length > 0
          ? usage.join(", ")
          : "nothing --- the certificate names no use for its own key",
    warn: usage !== null && usage.length === 0,
  });

  const purposes = certificate.extended_usage;
  if (purposes !== null) {
    rows.push({
      name: "Issued for",
      value:
        purposes.length > 0
          ? purposes.join(", ")
          : "nothing --- the certificate names no purpose",
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
      value: `${certificate.chain}, of which this is the signer's`,
    });
  }
  if (!certificate.matched_signer) {
    rows.push({
      name: "Certificates present",
      value:
        "one, and the signature does not point at it --- shown because there " +
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

/** Every line of one signature. */
export function signatureRows(signature: Signature, bytes: number): Row[] {
  if (!signature.signed) {
    return [{ name: "Status", value: "a signature field, not yet signed" }];
  }

  const rows: Row[] = [];
  const claimed = (name: string, value: string): void => {
    if (value) rows.push({ name, value });
  };

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
  // number a reader has no way to weigh.
  const stamp = signature.timestamp;
  if (stamp?.when) {
    const by =
      stamp.authority?.subject_cn || stamp.authority?.subject || "an unnamed authority";
    rows.push({
      name: "Timestamped",
      value: `${stamp.when} by ${by} --- a separate party's claim, which tpdf does not check`,
    });
  }
  rows.push(coverageOf(signature, bytes));

  const level = certificationOf(signature.certification);
  if (level) rows.push({ name: "Certification", value: level });

  const how = [signature.handler, signature.kind].filter(Boolean).join(" / ");
  claimed("Format", how);

  return rows;
}

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
            "all, so its properties, signatures and structure are not missing --- " +
            "they were never seen.",
        },
      ]
    : [];

  const signatures: Section[] = properties.signatures.map((signature) => {
    const title = signature.field ? `Signature --- ${signature.field}` : "Signature";
    const rows = signatureRows(signature, properties.bytes);
    // The disclaimer goes on a signature that exists, and not on an empty field
    // waiting for one --- there is nothing there to be wrong about.
    return signature.signed ? { title, rows, note: NOT_CHECKED } : { title, rows };
  });

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
        "ignore them --- they are a request, not an enforcement.",
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
        ? "yes --- the document states its own reading order"
        : "no --- reading order is inferred from the layout",
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
  return [...locked, ...signatures, ...described, ...security, file, ...cut];
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
