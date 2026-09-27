import { describe, expect, it } from "vitest";

import {
  UNSAVED,
  afterSigning,
  choiceLabel,
  nothingToChoose,
  signDocument,
  signedName,
  type Choice,
  type Choices,
  type Signed,
  type SigningShell,
} from "./signing";

const choice: Choice = {
  id: "abc",
  subject: "A. Signer",
  issuer: "Example CA",
  expires: "2026-01-01 00:00:00 UTC",
  method: "ECDSA P-256",
};

/** A shell that records every question asked of it, in order. */
function shell(overrides: Partial<SigningShell> = {}) {
  const asked: string[] = [];
  const signed: Signed = {
    path: "/docs/report-signed.pdf",
    field: "Signature1",
    signatures: [
      {
        field: "Signature1",
        integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "ECDSA P-256" },
        ours: true,
      },
    ],
  };
  const base: SigningShell = {
    dirty: () => false,
    openPath: "/docs/report.pdf",
    list: async () => {
      asked.push("list");
      return { usable: [choice], skipped: [] };
    },
    choose: async (choices) => {
      asked.push(`choose:${choices.map((c) => c.id).join("+")}`);
      return choices[0]?.id ?? null;
    },
    saveAs: async (suggested) => {
      asked.push(`saveAs:${suggested}`);
      return "/docs/report-signed.pdf";
    },
    sign: async (identity, path) => {
      asked.push(`sign:${identity}:${path}`);
      return signed;
    },
  };
  return { asked, shell: { ...base, ...overrides } };
}

describe("the order the questions are asked in", () => {
  it("lists, chooses, names and signs, and reports what was read back", async () => {
    const { asked, shell: s } = shell();
    const said = await signDocument(s);
    expect(asked).toEqual([
      "list",
      "choose:abc",
      "saveAs:report-signed.pdf",
      "sign:abc:/docs/report-signed.pdf",
    ]);
    expect(said).toBe(
      "Signed as Signature1 and saved to report-signed.pdf. Read back after writing, the signature is intact.",
    );
  });

  it("refuses unsaved edits before asking the OS anything", async () => {
    const { asked, shell: s } = shell({ dirty: () => true });
    expect(await signDocument(s)).toBe(UNSAVED);
    expect(asked).toEqual([]);
  });

  it("stops without a word when the reader cancels either question", async () => {
    const chooser = shell({ choose: async () => null });
    expect(await signDocument(chooser.shell)).toBeNull();
    expect(chooser.asked).toEqual(["list"]);

    const panel = shell({ saveAs: async () => null });
    expect(await signDocument(panel.shell)).toBeNull();
    expect(panel.asked).toEqual(["list", "choose:abc"]);
  });

  it("says what was found when nothing can sign, and asks nothing more", async () => {
    const found: Choices = {
      usable: [],
      skipped: [{ subject: "Old Signer", why: "it has expired" }],
    };
    const { asked, shell: s } = shell({
      list: async () => {
        asked.push("list");
        return found;
      },
    });
    expect(await signDocument(s)).toBe(nothingToChoose(found));
    expect(asked).toEqual(["list"]);
  });

  it("passes a refusal from the backend on rather than reporting success", async () => {
    const { shell: s } = shell({
      sign: async () => {
        throw new Error("This document is certified with no changes permitted");
      },
    });
    await expect(signDocument(s)).rejects.toThrow("no changes permitted");
  });
});

describe("the words", () => {
  it("suggests a -signed name beside the original, whatever its case", () => {
    expect(signedName("/docs/report.pdf")).toBe("report-signed.pdf");
    expect(signedName("C:\\docs\\Report.PDF")).toBe("Report-signed.pdf");
    expect(signedName("/docs/notes")).toBe("notes-signed.pdf");
  });

  it("labels a certificate by who, from whom, until when and how", () => {
    expect(choiceLabel(choice)).toBe(
      "A. Signer — issued by Example CA, expires 2026-01-01 (ECDSA P-256)",
    );
  });

  it("names what was found but not offered, and says nothing more when nothing was", () => {
    expect(nothingToChoose({ usable: [], skipped: [] })).toBe(
      "No certificate that can sign was found in your keychain or certificate store.",
    );
    expect(
      nothingToChoose({
        usable: [],
        skipped: [
          { subject: "Old", why: "it has expired" },
          { subject: "Mail", why: "it is not issued for signing" },
        ],
      }),
    ).toBe(
      "No certificate that can sign was found in your keychain or certificate store. " +
        "Found but not offered — Old: it has expired; Mail: it is not issued for signing.",
    );
  });

  it("lists every earlier signature after the new one", () => {
    const said = afterSigning({
      path: "/docs/a-signed.pdf",
      field: "Signature3",
      signatures: [
        {
          field: "Signature1",
          integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "RSA" },
          ours: false,
        },
        {
          field: "Signature2",
          integrity: { verdict: "unchecked", why: "format", digest: "", method: "" },
          ours: false,
        },
        {
          field: "Signature3",
          integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "RSA" },
          ours: true,
        },
      ],
    });
    expect(said).toBe(
      "Signed as Signature3 and saved to a-signed.pdf. Read back after writing, the signature is intact. " +
        "Earlier signatures: Signature1 intact, Signature2 not checked " +
        "(tpdf does not check signatures in this format).",
    );
  });

  it("puts a new signature that did not read back intact first, and warns", () => {
    for (const verdict of ["broken", "altered", "weak", "unchecked"] as const) {
      const said = afterSigning({
        path: "/docs/a-signed.pdf",
        field: "Signature1",
        signatures: [
          {
            field: "Signature1",
            integrity: { verdict, why: verdict === "unchecked" ? "range" : null, digest: "", method: "" },
            ours: true,
          },
        ],
      });
      expect(said.startsWith("a-signed.pdf was written, but reading it back did not find"), verdict).toBe(true);
      expect(said).toContain("Do not rely on that copy.");
      expect(said).not.toContain("Signed as");
    }
    // And a file in which the new field is missing altogether.
    const missing = afterSigning({ path: "/x.pdf", field: "Signature1", signatures: [] });
    expect(missing).toContain("it is not in the file");
  });

  it("never calls a signature valid, verified, authentic or genuine", () => {
    // `properties.test.ts`'s rule, for the same reason: the check is about the
    // bytes and the key, never about who holds the key.
    const said = afterSigning({
      path: "/docs/a-signed.pdf",
      field: "Signature1",
      signatures: [
        {
          field: "Signature1",
          integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "RSA" },
          ours: true,
        },
      ],
    });
    for (const word of ["valid", "verified", "authentic", "genuine"]) {
      expect(said.toLowerCase()).not.toContain(word);
    }
  });
});
