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
 * are actually chosen: this application has no certificate parser and no trust
 * store, so it does not know whether a signature verifies. What it knows is what
 * the document *claims* and one structural fact it can check itself.
 *
 * The vocabulary carries that. A signer's name, reason, location and date are
 * introduced as claimed; the only unhedged sentence in the whole section is the
 * byte-range one, which is the only thing that was measured.
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
}

/** What could not be read. */
export interface Limits {
  locked: boolean;
  fields_dropped: number;
  values_clipped: number;
  signatures_dropped: number;
  unreadable: number;
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
  limits: Limits;
  scan_ms: number;
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
  "tpdf reads what the signature says. It does not verify the certificate, " +
  "check who issued it, or look for a revocation, so nothing here means the " +
  "signature is valid.";

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

/** Every line of one signature. */
export function signatureRows(signature: Signature, bytes: number): Row[] {
  if (!signature.signed) {
    return [{ name: "Status", value: "a signature field, not yet signed" }];
  }

  const rows: Row[] = [];
  const claimed = (name: string, value: string): void => {
    if (value) rows.push({ name, value });
  };

  claimed("Signer says", signature.name);
  claimed("Reason given", signature.reason);
  claimed("Location given", signature.location);
  claimed("Date given", signature.when);
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
  return rows;
}
