import { describe, expect, it } from "vitest";

import {
  certificationOf,
  coverageOf,
  formatBytes,
  limitRows,
  NOT_CHECKED,
  sections,
  signatureRows,
  VERDICT_WORDS,
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
    limits: {
      locked: false,
      fields_dropped: 0,
      values_clipped: 0,
      signatures_dropped: 0,
      unreadable: 0,
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
    certification: 0,
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
  it("says the whole file when the range reaches the last byte", () => {
    expect(coverageOf(signed(), 1024)).toEqual({
      name: "Covers",
      value: "the whole file",
    });
  });

  it("names how much lies outside the range, and warns", () => {
    // The real failure this exists for: a document signed and then appended to.
    // The number is what makes it actionable --- "not the whole file" alone
    // does not distinguish a stray newline from a second document bolted on.
    const short = { ...signed(), covers_whole_file: false, covered_bytes: 700 };
    expect(coverageOf(short, 1024)).toEqual({
      name: "Covers",
      value: "not the whole file --- 324 bytes lie outside the signed range",
      warn: true,
    });
  });

  it("refuses to answer at all when there is no byte range", () => {
    // "Covers nothing" would be a measurement. There was no range to measure,
    // which is a different statement and the only honest one.
    const none = { ...signed(), covers_whole_file: false, covered_bytes: 0 };
    expect(coverageOf(none, 1024)).toEqual({
      name: "Covers",
      value: "no byte range stated, so nothing could be checked",
      warn: true,
    });
  });
});

describe("signatureRows", () => {
  it("introduces every claimed field as claimed", () => {
    const rows = signatureRows(signed(), 1024);
    const names = rows.map((row) => row.name);
    // Each of these four is a string the signer wrote and nothing checked, and
    // the label is the only thing saying so.
    expect(names).toContain("Signer says");
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
    expect(names).toContain("Signer says");
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
    expect(titles.indexOf("Signature --- Signature1")).toBeLessThan(
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
