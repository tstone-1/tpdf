import { describe, expect, it } from "vitest";

import {
  certificateRows,
  certificationOf,
  conformanceRows,
  appendixRow,
  coverageOf,
  formatBytes,
  limitRows,
  NOT_CHECKED,
  sections,
  signatureRows,
  VERDICT_WORDS,
  type Appendix,
  type Certificate,
  type Properties,
  type Signature,
} from "./properties";

/** A document that states nothing, so each test adds only what it is about. */
function blank(): Properties {
  return {
    version: "1.7",
    bytes: 1024,
    pages: 1,
    revisions: 1,
    fields: [],
    encryption: null,
    signatures: [],
    tagged: null,
    language: "",
    attachments: null,
    xmp: null,
    limits: {
      locked: false,
      fields_dropped: 0,
      values_clipped: 0,
      signatures_dropped: 0,
      unreadable: 0,
      certificates_unread: 0,
    },
    scan_ms: 0.1,
  };
}

/** A signature that claims everything, so a test can take pieces away. */
function signed(): Signature {
  return {
    field: "Signature1",
    signed: true,
    handler: "Adobe.PPKLite",
    kind: "adbe.pkcs7.detached",
    name: "A. Signer",
    reason: "SGS officially issued document",
    location: "EUW",
    when: "2026-05-25 08:05:07 UTC",
    covers_whole_file: true,
    covered_bytes: 1024,
    appended_bytes: 0,
    appendix: null,
    certification: 0,
    certificate: null,
    timestamp: null,
  };
}

/** A certificate that states everything, so a test can take pieces away. */
function certificate(): Certificate {
  return {
    subject: "CN=A. Signer, O=SGS",
    subject_cn: "A. Signer",
    issuer: "CN=SGS Issuing CA, O=SGS",
    issuer_cn: "SGS Issuing CA",
    serial: "085398B6930734A2C5F6F74C89AACE579C0EE11B",
    from: "2026-01-01 00:00:00 UTC",
    until: "2030-01-01 00:00:00 UTC",
    self_issued: false,
    chain: 2,
    matched_signer: true,
    key_usage: ["Digital signature", "Non-repudiation"],
    extended_usage: ["Email protection"],
    authority: false,
    extensions_unread: 0,
  };
}

describe("formatBytes", () => {
  it("gives a plain count below a kilobyte, with no rounded size beside it", () => {
    // "0.9 KB (923 bytes)" is two numbers for a quantity nobody needs rounded.
    expect(formatBytes(923)).toBe("923 bytes");
  });

  it("keeps one decimal below ten and drops it above", () => {
    expect(formatBytes(9_123_456)).toBe("8.7 MB (9,123,456 bytes)");
    expect(formatBytes(894_280)).toBe("873 KB (894,280 bytes)");
  });

  it("says so rather than printing NaN for a size it was not given", () => {
    // The refusal matters because the alternative reads as a real measurement:
    // `NaN bytes` and `0 bytes` are both statements about the file, and neither
    // is true of a document whose size never arrived.
    expect(formatBytes(Number.NaN)).toBe("unknown");
    expect(formatBytes(-1)).toBe("unknown");
  });
});

describe("coverageOf", () => {
  it("names the container it cannot cover, rather than claiming the whole file", () => {
    // It said "the whole file" until 2026-08-25, which is false of every
    // signature there is: the `/Contents` hex string holding the hash cannot be
    // inside what was hashed. Saying it once here is what stops the appended
    // case below reading as though the container were the reader's problem.
    expect(coverageOf(signed(), 1024)).toEqual({
      name: "Covers",
      value: "the whole file, except the signature container it cannot cover",
    });
  });

  it("counts what was appended after signing, not what is uncovered", () => {
    // The defect a reader reported, with the real numbers from the DocuSign
    // contract that produced it. 74,637 bytes are uncovered, and the row said
    // so; 65,536 of them are the container, which every signed PDF has, and the
    // 9,101 that were appended after signing are the whole of what is worth
    // reading. A row that leads with 73 KB is corrected by anyone technical and
    // alarms everyone else.
    const appended = {
      ...signed(),
      covers_whole_file: false,
      covered_bytes: 51_996,
      appended_bytes: 9_101,
    };
    expect(coverageOf(appended, 126_633)).toEqual({
      name: "Covers",
      value:
        "everything up to the signature, and 8.9 KB (9,101 bytes) were appended afterwards",
      warn: true,
    });
  });

  it("does not let the container reach the number a reader is shown", () => {
    // The control for the sentence above, and the one that could not exist
    // while the row was a subtraction: the container grows by 56 KB and the
    // reading does not move, because it is not a fact about the container.
    const small = {
      ...signed(),
      covers_whole_file: false,
      covered_bytes: 51_996,
      appended_bytes: 9_101,
    };
    const large = { ...small, covered_bytes: 51_996 };
    expect(coverageOf(large, 126_633 + 57_344)).toEqual(coverageOf(small, 126_633));
  });

  it("names how much lies outside a range that leaves the head of the file", () => {
    // Not an append: the range reaches the last byte and does not start at the
    // first, so the unsigned bytes are a prologue. Rare, and it wants different
    // suspicion from an appendix -- content that was there before the signature
    // and was left out of it.
    const short = {
      ...signed(),
      covers_whole_file: false,
      covered_bytes: 700,
      appended_bytes: 0,
    };
    expect(coverageOf(short, 1024)).toEqual({
      name: "Covers",
      value: "not the whole file — 324 bytes lie outside the signed range",
      warn: true,
    });
  });

  it("refuses to answer at all when there is no byte range", () => {
    // "Covers nothing" would be a measurement. There was no range to measure,
    // which is a different statement and the only honest one.
    // `appended_bytes` is set to something loud on purpose: with no range there
    // is no end to measure from, so this branch must answer before it is read.
    const none = {
      ...signed(),
      covers_whole_file: false,
      covered_bytes: 0,
      appended_bytes: 4_096,
    };
    expect(coverageOf(none, 1024)).toEqual({
      name: "Covers",
      value: "no byte range stated, so nothing could be checked",
      warn: true,
    });
  });
});

describe("appendixRow", () => {
  /** An LTV append, with the shape measured on a real DocuSign contract. */
  function validationData(): Appendix {
    return {
      added: 15,
      replaced: 1,
      kinds: ["Catalog", "DSS", "VRI", "stream", "untyped", "value"],
      catalog_gained: ["DSS"],
      pages_touched: 0,
      unread: false,
    };
  }

  /** A second signature, with the shape measured on `incr-two-signers.pdf`. */
  function secondSignature(): Appendix {
    return {
      added: 5,
      replaced: 3,
      kinds: ["Annot/Widget", "FontDescriptor", "Page", "Sig", "stream", "untyped"],
      catalog_gained: [],
      pages_touched: 1,
      unread: false,
    };
  }

  it("says nothing at all when nothing was appended", () => {
    // `null`, not an empty row. A signature nobody has appended to should not
    // grow a row saying so -- the Covers row above it already reads "the whole
    // file, except the signature container", and a second line adding "and
    // nothing after it" is the same fact twice.
    expect(appendixRow(signed())).toBeNull();
  });

  it("names validation data as what it is, and says no page moved", () => {
    // The row the whole feature exists for. 9 KB of LTV data and 9 KB of new
    // page content are the same size, and the size is all a reader had.
    const signature = { ...signed(), appendix: validationData() };
    expect(appendixRow(signature)).toEqual({
      name: "Appended",
      value:
        "the certificates and revocation records a signature needs to be checked later, " +
        "and no page was rewritten",
      warn: true,
    });
  });

  it("distinguishes a second signature from validation data", () => {
    // The discrimination, on two inputs that a byte count cannot tell apart.
    // Both appended about nine kilobytes; only one of them touched a page.
    const signature = { ...signed(), appendix: secondSignature() };
    expect(appendixRow(signature)).toEqual({
      name: "Appended",
      value: "another signature, and 1 page was rewritten",
      warn: true,
    });
  });

  it("reads DSS from the catalog rather than from the object list", () => {
    // The two are not the same test, and the catalog is the one that means
    // something: a `/DSS` object among fifteen could be anything the file
    // happens to hold, while the catalog gaining the key is what an LTV append
    // *is*. Here the object is present and the catalog is not -- and the row
    // must fall through rather than claim validation data.
    const ambiguous = { ...validationData(), catalog_gained: [] };
    const value = appendixRow({ ...signed(), appendix: ambiguous })?.value ?? "";
    // Against the phrase the DSS branch actually produces. It said "validation
    // data" until the wording moved off that word to keep clear of
    // `VERDICT_WORDS`, and an assertion naming a string nothing can produce is
    // one that cannot fail.
    expect(value).not.toContain("revocation records");
    expect(value).toContain("16 objects");
  });

  it("falls through to the file's own names when it has no better word", () => {
    const other: Appendix = {
      added: 2,
      replaced: 0,
      kinds: ["Metadata", "StructTreeRoot"],
      catalog_gained: ["StructTreeRoot"],
      pages_touched: 0,
      unread: false,
    };
    expect(appendixRow({ ...signed(), appendix: other })?.value).toBe(
      "2 objects: Metadata, StructTreeRoot, and no page was rewritten",
    );
  });

  it("reports an appendix it could not read as unread, never as empty", () => {
    // The reassuring failure this exists to prevent: an appendix that could not
    // be decomposed has no objects in it, and "no objects" renders as an append
    // that changed nothing.
    const unread: Appendix = {
      added: 0,
      replaced: 0,
      kinds: [],
      catalog_gained: [],
      pages_touched: 0,
      unread: true,
    };
    expect(appendixRow({ ...signed(), appendix: unread })).toEqual({
      name: "Appended",
      value: "something, but its contents could not be read",
      warn: true,
    });
  });

  it("counts pages in the plural where there are several", () => {
    const many = { ...secondSignature(), pages_touched: 4 };
    expect(appendixRow({ ...signed(), appendix: many })?.value).toContain(
      "4 pages were rewritten",
    );
  });

  it("sits directly under the Covers row it completes", () => {
    // Position is the point: Covers says how much, this says what, and a reader
    // takes them as one statement. Anywhere else and the two numbers are a
    // puzzle to assemble.
    const signature = {
      ...signed(),
      covers_whole_file: false,
      covered_bytes: 51_996,
      appended_bytes: 9_101,
      appendix: validationData(),
    };
    const names = signatureRows(signature, 126_633).map((row) => row.name);
    const covers = names.indexOf("Covers");
    expect(covers).toBeGreaterThanOrEqual(0);
    expect(names[covers + 1]).toBe("Appended");
  });

  it("carries no verdict word, as nothing in this panel may", () => {
    // The standing rule, applied to the one row that describes a change rather
    // than reporting a field. `VERDICT_WORDS` is the same list the rest of the
    // panel is held to.
    for (const appendix of [validationData(), secondSignature()]) {
      const value = appendixRow({ ...signed(), appendix })?.value.toLowerCase() ?? "";
      for (const word of VERDICT_WORDS) {
        expect(value, `"${word}" is a verdict`).not.toContain(word);
      }
    }
  });
});

describe("signatureRows", () => {
  it("introduces every claimed field as claimed", () => {
    const rows = signatureRows(signed(), 1024);
    const names = rows.map((row) => row.name);
    // Each of these four is a string the signer wrote and nothing checked, and
    // the label is the only thing saying so.
    expect(names).toContain("Signer typed");
    expect(names).toContain("Reason given");
    expect(names).toContain("Location given");
    expect(names).toContain("Date given");
  });

  it("leaves out a field the document does not state", () => {
    // An empty row reads as a signature that gave no reason, which is true, and
    // as a reason that is blank, which is not the same thing to look at.
    const quiet = { ...signed(), reason: "", location: "" };
    const names = signatureRows(quiet, 1024).map((row) => row.name);
    expect(names).not.toContain("Reason given");
    expect(names).not.toContain("Location given");
    expect(names).toContain("Signer typed");
  });

  it("reports an unsigned field as a field, not as an absent signature", () => {
    const empty = { ...signed(), signed: false };
    expect(signatureRows(empty, 1024)).toEqual([
      { name: "Status", value: "a signature field, not yet signed" },
    ]);
  });

  it("names the certification level when there is one", () => {
    const certified = { ...signed(), certification: 1 };
    const rows = signatureRows(certified, 1024);
    expect(rows).toContainEqual({
      name: "Certification",
      value: "certified, no changes permitted",
    });
  });
});

describe("certificationOf", () => {
  it("describes the three levels the specification defines", () => {
    expect(certificationOf(1)).toBe("certified, no changes permitted");
    expect(certificationOf(2)).toBe("certified, form filling permitted");
    expect(certificationOf(3)).toBe(
      "certified, form filling and comments permitted",
    );
  });

  it("says nothing for a level the specification does not define", () => {
    // Including zero, which is an ordinary approval signature rather than a
    // certification --- describing it as one would be a claim about what the
    // signer intended.
    expect(certificationOf(0)).toBe("");
    expect(certificationOf(4)).toBe("");
  });
});

describe("sections", () => {
  it("puts a signature above the file's own statistics", () => {
    // Order is the decision: somebody who opens this dialog on a signed
    // document opened it about the signature.
    const properties = { ...blank(), signatures: [signed()] };
    const titles = sections(properties).map((section) => section.title);
    expect(titles.indexOf("Signature — Signature1")).toBeLessThan(
      titles.indexOf("File"),
    );
  });

  it("carries the disclaimer on every signed signature", () => {
    const properties = { ...blank(), signatures: [signed(), signed()] };
    const notes = sections(properties)
      .filter((section) => section.title.startsWith("Signature"))
      .map((section) => section.note);
    expect(notes).toEqual([NOT_CHECKED, NOT_CHECKED]);
  });

  it("does not disclaim an unsigned field, which claims nothing", () => {
    const properties = { ...blank(), signatures: [{ ...signed(), signed: false }] };
    expect(sections(properties)[0]?.note).toBeUndefined();
  });

  it("never renders a word that would read as a verdict", () => {
    // The broadest check here, and the one that survives somebody editing a
    // template string later: it reads what is actually rendered rather than
    // what the source appears to say, so a phrase interpolated in from a
    // document's own /Reason is caught by this and by nothing else.
    const hostile = {
      ...signed(),
      reason: "This document is valid and verified",
      name: "Authentic Signing Ltd",
    };
    const properties = { ...blank(), signatures: [hostile] };
    const rendered = sections(properties)
      .flatMap((section) => [
        section.title,
        section.note ?? "",
        ...section.rows.flatMap((row) => [row.name, row.value]),
      ])
      // The disclaimer is exempt by identity, not by pattern: denying validity
      // is the one place the word belongs.
      .filter((line) => line !== NOT_CHECKED)
      .join(" ")
      .toLowerCase();

    // The document's own words are shown --- they are what it says. What must
    // not happen is tpdf saying them.
    const ours = rendered.replace(hostile.reason.toLowerCase(), "").replace(
      hostile.name.toLowerCase(),
      "",
    );
    for (const word of VERDICT_WORDS) {
      expect(ours).not.toContain(word);
    }
  });

  it("says what a locked document is, rather than showing it as empty", () => {
    const locked = { ...blank(), limits: { ...blank().limits, locked: true } };
    const first = sections(locked)[0];
    expect(first?.title).toBe("Locked");
    // The distinction the whole section exists for: absent and never-looked-at
    // are different, and only one of them is what a blank readout means.
    expect(first?.note).toContain("never seen");
  });

  it("omits the tagged line when the question could not be asked", () => {
    // `null` is not `false`. Reporting a locked document as untagged is a
    // confident false statement, which is the failure mode of every optional
    // field here.
    const unknown = { ...blank(), tagged: null };
    const file = sections(unknown).find((section) => section.title === "File");
    expect(file?.rows.map((row) => row.name)).not.toContain("Tagged");

    const known = { ...blank(), tagged: false };
    const said = sections(known).find((section) => section.title === "File");
    expect(said?.rows.map((row) => row.name)).toContain("Tagged");
  });

  it("shows the revision count only when there is more than one", () => {
    const one = sections(blank()).find((section) => section.title === "File");
    expect(one?.rows.map((row) => row.name)).not.toContain("Revisions");

    const two = sections({ ...blank(), revisions: 2 }).find(
      (section) => section.title === "File",
    );
    expect(two?.rows.map((row) => row.name)).toContain("Revisions");
  });

  it("marks a forbidden permission so it is visible at a glance", () => {
    const properties = {
      ...blank(),
      encryption: {
        method: "RC4 40-bit",
        revision: 2,
        opened_without_password: true,
        permissions: [
          { what: "Print", allowed: true },
          { what: "Copy text and graphics", allowed: false },
        ],
      },
    };
    const security = sections(properties).find(
      (section) => section.title === "Security",
    );
    expect(security?.rows).toContainEqual({
      name: "Copy text and graphics",
      value: "not allowed",
      warn: true,
    });
    // A restriction the document states is not a restriction anything enforces,
    // and a readout that did not say so would read as a guarantee.
    expect(security?.note).toContain("a request, not an enforcement");
  });

  it("renames the two date keys and leaves a custom key as written", () => {
    const properties = {
      ...blank(),
      fields: [
        { name: "CreationDate", value: "2026-05-25", standard: true },
        { name: "SourceModified", value: "D:20260525", standard: false },
      ],
    };
    const described = sections(properties).find(
      (section) => section.title === "Described as",
    );
    expect(described?.rows.map((row) => row.name)).toEqual([
      "Created",
      "SourceModified",
    ]);
  });

  it("says which part is missing rather than looking whole", () => {
    const cut = {
      ...blank(),
      limits: { ...blank().limits, signatures_dropped: 2, unreadable: 1 },
    };
    const partial = sections(cut).find(
      (section) => section.title === "Not fully read",
    );
    expect(partial?.rows).toEqual([
      { name: "Signature fields", value: "2 were not read", warn: true },
      { name: "Entries", value: "1 could not be read at all", warn: true },
    ]);
  });

  it("adds no such section when nothing was cut", () => {
    expect(limitRows(blank().limits)).toEqual([]);
    const titles = sections(blank()).map((section) => section.title);
    expect(titles).not.toContain("Not fully read");
  });
});

describe("the signing certificate", () => {
  it("names the signer above what the signer typed", () => {
    const sig = signed();
    sig.certificate = certificate();
    const names = signatureRows(sig, 1024).map((r) => r.name);

    expect(names).toContain("Certificate names");
    expect(names).toContain("Signer typed");
    // Order is the decision under test: a reader asks who signed this, and the
    // first answer they meet should not be the one anybody could type.
    expect(names.indexOf("Certificate names")).toBeLessThan(names.indexOf("Signer typed"));
  });

  it("says a self-issued certificate was vouched for by nobody", () => {
    const cert = certificate();
    cert.self_issued = true;
    cert.issuer_cn = cert.subject_cn;
    const sig = signed();
    sig.certificate = cert;

    const issued = certificateRows(sig).find((r) => r.name === "Issued by");
    expect(issued?.value).toContain("self-issued");
    // Not a warning. Every root in every trust store is self-issued, and tpdf
    // has no trust store with which to tell those apart from an unvouched one.
    expect(issued?.warn).toBeFalsy();
  });

  it("points out two names for one signer that disagree", () => {
    const cert = certificate();
    cert.subject_cn = "B. Different";
    const sig = signed();
    sig.name = "A. Signer";
    sig.certificate = cert;

    const row = certificateRows(sig).find((r) => r.name === "Names disagree");
    expect(row).toBeDefined();
    expect(row?.warn).toBe(true);
    expect(row?.value).toContain("B. Different");
    expect(row?.value).toContain("A. Signer");
  });

  it("does not call a difference of case a disagreement", () => {
    const cert = certificate();
    cert.subject_cn = "a. signer";
    const sig = signed();
    sig.name = "A. Signer";
    sig.certificate = cert;

    expect(certificateRows(sig).find((r) => r.name === "Names disagree")).toBeUndefined();
  });

  it("says nothing at all when there is no certificate", () => {
    const sig = signed();
    sig.certificate = null;
    expect(certificateRows(sig)).toEqual([]);
  });

  it("warns when the signature does not point at the certificate shown", () => {
    const cert = certificate();
    cert.matched_signer = false;
    cert.chain = 1;
    const sig = signed();
    sig.certificate = cert;

    const row = certificateRows(sig).find((r) => r.name === "Certificates present");
    expect(row?.warn).toBe(true);
  });

  it("tells a certificate that states no use apart from one that states none", () => {
    // Opposite claims. Absent places no limit on the key; empty limits it to
    // nothing -- and the row a reader sees has to say which, because the
    // direction a collapse would fall is the reassuring one.
    const silent = certificate();
    silent.key_usage = null;
    const stated = certificate();
    stated.key_usage = [];

    const rowFor = (cert: Certificate) => {
      const sig = signed();
      sig.certificate = cert;
      return certificateRows(sig).find((r) => r.name === "Key is for");
    };

    expect(rowFor(silent)?.value).toContain("not stated");
    expect(rowFor(silent)?.warn).toBeFalsy();
    expect(rowFor(stated)?.value).toContain("nothing");
    expect(rowFor(stated)?.warn).toBe(true);
    expect(rowFor(certificate())?.value).toBe("Digital signature, Non-repudiation");
  });

  it("omits the purpose row when the certificate names no purposes at all", () => {
    // The asymmetry with the row above is deliberate. An absent key usage is
    // information -- the issuer placed no limit -- while an absent extended key
    // usage is the ordinary case for a signing certificate and a row saying so
    // on nearly every document is noise.
    const cert = certificate();
    cert.extended_usage = null;
    const sig = signed();
    sig.certificate = cert;

    expect(certificateRows(sig).find((r) => r.name === "Issued for")).toBeUndefined();
    expect(certificateRows(sig).find((r) => r.name === "Key is for")).toBeDefined();
  });

  it("says so when the signer's own certificate claims to issue others", () => {
    const cert = certificate();
    cert.authority = true;
    const sig = signed();
    sig.certificate = cert;

    const row = certificateRows(sig).find((r) => r.name === "Also an authority");
    expect(row?.warn).toBe(true);

    // And says nothing in the two ordinary cases, which read the same way.
    for (const ordinary of [false, null] as const) {
      const plain = certificate();
      plain.authority = ordinary;
      const other = signed();
      other.certificate = plain;
      expect(
        certificateRows(other).find((r) => r.name === "Also an authority"),
      ).toBeUndefined();
    }
  });

  it("reports an extension it could not read rather than one that said nothing", () => {
    const cert = certificate();
    cert.key_usage = null;
    cert.extensions_unread = 2;
    const sig = signed();
    sig.certificate = cert;

    const row = certificateRows(sig).find((r) => r.name === "Extensions");
    expect(row?.value).toContain("2");
    expect(row?.warn).toBe(true);
  });

  it("puts no verdict word in any line a certificate produces", () => {
    // The hostile case: a document chooses its own certificate subject, so a
    // signer can name themselves anything they like and it is rendered as data.
    const cert = certificate();
    cert.subject_cn = "This certificate is valid and verified";
    cert.issuer_cn = "Genuine Authentic Root";
    const sig = signed();
    sig.name = "";
    sig.certificate = cert;

    for (const row of certificateRows(sig)) {
      for (const word of VERDICT_WORDS) {
        // The value may quote the document; the label may not, because the
        // label is the only half tpdf wrote.
        expect(row.name.toLowerCase()).not.toContain(word);
      }
    }
  });
});

describe("a timestamp on a signature", () => {
  it("names the authority beside the time, and says it is unchecked", () => {
    const authority = certificate();
    authority.subject_cn = "Acme Time Authority";
    const sig = signed();
    sig.timestamp = { when: "2026-08-21 12:00:00 UTC", authority };

    const row = signatureRows(sig, 1024).find((r) => r.name === "Timestamped");
    expect(row?.value).toContain("2026-08-21 12:00:00 UTC");
    expect(row?.value).toContain("Acme Time Authority");
    // A time with no attester named is a number a reader cannot weigh, and a
    // time presented without the disclaimer reads as tpdf agreeing.
    expect(row?.value).toContain("does not check");
  });

  it("puts the attested time under the signer's own date, not over it", () => {
    // Two answers to one question from two places, and the order is the claim
    // about which is which: the signer's clock first because it is the one the
    // document itself states, the authority's under it as the second source.
    const sig = signed();
    sig.when = "2026-08-21 16:58:20 +02:00";
    sig.timestamp = { when: "2026-08-21 12:00:00 UTC", authority: null };

    const names = signatureRows(sig, 1024).map((r) => r.name);
    // Both must be present before the comparison means anything: `indexOf`
    // answers -1 for a row that is not there, and -1 is less than every real
    // index -- so an ordering assertion on its own passes most loudly when the
    // row it is about has been deleted.
    expect(names).toContain("Date given");
    expect(names).toContain("Timestamped");
    expect(names.indexOf("Date given")).toBeLessThan(names.indexOf("Timestamped"));
  });

  it("still reports a token that names no authority", () => {
    // The authority's certificate is optional in a token. A time with no name
    // beside it is worth less and is not worth nothing, and dropping the row
    // would report the signature as untimestamped.
    const sig = signed();
    sig.timestamp = { when: "2026-08-21 12:00:00 UTC", authority: null };

    const row = signatureRows(sig, 1024).find((r) => r.name === "Timestamped");
    expect(row?.value).toContain("an unnamed authority");
  });

  it("says nothing for a signature nobody timestamped", () => {
    // The common case -- 1 of 10 signed documents to hand carries a token --
    // and this is the control that stops the row appearing on all of them.
    const sig = signed();
    sig.timestamp = null;

    expect(
      signatureRows(sig, 1024).find((r) => r.name === "Timestamped"),
    ).toBeUndefined();
  });

  it("puts no verdict word in a line built from an authority's own name", () => {
    // A timestamp token is chosen by whoever signed the document, so the
    // authority's name is attacker-controlled exactly as the signer's is.
    const authority = certificate();
    authority.subject_cn = "Verified Genuine Timestamps Ltd";
    const sig = signed();
    sig.timestamp = { when: "2026-08-21 12:00:00 UTC", authority };

    for (const row of signatureRows(sig, 1024)) {
      for (const word of VERDICT_WORDS) {
        expect(row.name.toLowerCase()).not.toContain(word);
      }
    }
  });
});

describe("what a document claims to conform to", () => {
  it("shows a claim as a claim", () => {
    const doc = blank();
    doc.xmp = { bytes: 1718, conformance: ["PDF/A-3B"], unread: false };

    const row = conformanceRows(doc.xmp)[0];
    expect(row?.name).toBe("States conformance");
    expect(row?.value).toContain("PDF/A-3B");
    // The whole posture of this readout: nothing here validates anything, and
    // a row that read "PDF/A-3B" alone would be taken as tpdf agreeing.
    expect(row?.value).toContain("does not check");
  });

  it("lists every standard a document claims", () => {
    const doc = blank();
    doc.xmp = { bytes: 900, conformance: ["PDF/A-2A", "PDF/UA-1"], unread: false };

    expect(conformanceRows(doc.xmp)[0]?.value).toContain("PDF/A-2A, PDF/UA-1");
  });

  it("says nothing for a document that claims nothing", () => {
    // Most documents. A row on every one of them is noise, and the absence of
    // a claim is not a finding -- so the silence here is deliberate and this is
    // what stops it being restored by somebody tidying up.
    const doc = blank();
    doc.xmp = { bytes: 3000, conformance: [], unread: false };
    expect(conformanceRows(doc.xmp)).toEqual([]);

    doc.xmp = null;
    expect(conformanceRows(doc.xmp)).toEqual([]);
  });

  it("speaks up when a packet is there and could not be read", () => {
    // The one case that always speaks: this is tpdf failing, not the document
    // declining to say anything, and those must not read the same.
    const doc = blank();
    doc.xmp = { bytes: 2_000_000, conformance: [], unread: true };

    const row = conformanceRows(doc.xmp)[0];
    expect(row?.warn).toBe(true);
    expect(row?.value).toContain("unknown");
  });

  it("puts a claim in the file section of the readout", () => {
    const doc = blank();
    doc.xmp = { bytes: 1718, conformance: ["PDF/UA-1"], unread: false };

    const file = sections(doc).find((s) => s.title === "File");
    expect(file?.rows.some((r) => r.value.includes("PDF/UA-1"))).toBe(true);
  });

  it("puts no verdict word in a line built from a document's own claim", () => {
    // The hostile case, and it is a real one: the conformance string is copied
    // out of the packet, so a document can write anything it likes there.
    const doc = blank();
    doc.xmp = {
      bytes: 900,
      conformance: ["PDF/A-1B, valid and verified, genuine"],
      unread: false,
    };

    for (const row of conformanceRows(doc.xmp)) {
      for (const word of VERDICT_WORDS) {
        expect(row.name.toLowerCase()).not.toContain(word);
      }
    }
  });
});

describe("a certificate that could not be read", () => {
  it("is reported as tpdf's failure and not as the document's silence", () => {
    const doc = blank();
    doc.limits.certificates_unread = 1;

    const row = limitRows(doc.limits).find((r) => r.name === "Certificates");
    expect(row).toBeDefined();
    expect(row?.warn).toBe(true);
    // "present but could not be read" and "absent" are the two readings that
    // must not collapse; the first is about tpdf, the second about the file.
    expect(row?.value).toContain("present");
  });
});
