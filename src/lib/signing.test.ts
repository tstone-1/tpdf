import { describe, expect, it, vi } from "vitest";

import type { PageId } from "./pages";
import type { SignatureImage } from "./signature";
import type { Appearance } from "./signappearance";
import {
  UNSAVED,
  afterSigning,
  askIdentity,
  choiceLabel,
  nothingToChoose,
  signDocument,
  signedName,
  type Choice,
  type Choices,
  type Placement,
  type Signed,
  type SigningShell,
} from "./signing";
import { FakeElement, installFakeDom } from "./testdom";

const choice: Choice = {
  id: "abc",
  subject: "A. Signer",
  issuer: "Example CA",
  expires: "2026-01-01 00:00:00 UTC",
  method: "ECDSA P-256",
};

/** The reader's saved visual signature, as the store answers it. */
const saved: SignatureImage = { width: 1, height: 1, rgba: [0, 0, 0, 255] };

/** What the reader chose in the appearance panel. */
const chosenLook: Appearance = {
  image: saved,
  options: { label: false, name: true, date: true, reason: "Approved", location: "Hamburg" },
};

/** Where the reader dragged, on the page with id 7. */
const dragged = { page: 7 as PageId, rect: [20, 30, 170, 90] as [number, number, number, number] };

/**
 * A shell that records every question asked of it, in order, and every
 * placement `sign` was handed.
 */
function shell(overrides: Partial<SigningShell> = {}) {
  const asked: string[] = [];
  const placements: (Placement | null)[] = [];
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
      const identity = choices[0]?.id;
      return identity === undefined ? null : { identity, visible: false };
    },
    savedImage: async () => {
      asked.push("savedImage");
      return saved;
    },
    appearance: async (identity, image) => {
      asked.push(`appearance:${identity}:${image === saved ? "saved" : "none"}`);
      return chosenLook;
    },
    place: async () => {
      asked.push("place");
      return dragged;
    },
    saveAs: async (suggested) => {
      asked.push(`saveAs:${suggested}`);
      return "/docs/report-signed.pdf";
    },
    sign: async (identity, path, placement) => {
      asked.push(`sign:${identity}:${path}`);
      placements.push(placement);
      return signed;
    },
  };
  return { asked, placements, shell: { ...base, ...overrides } };
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

  it("signs invisibly by default, asking nothing about a page or an image", async () => {
    const { asked, placements, shell: s } = shell();
    await signDocument(s);
    expect(asked).not.toContain("place");
    expect(asked).not.toContain("savedImage");
    expect(placements).toEqual([null]);
  });

  it("asks what a visible signature shows, then where, then the file, then signs", async () => {
    const { asked, placements, shell: s } = shell({
      choose: async (choices) => {
        asked.push(`choose:${choices.map((c) => c.id).join("+")}`);
        return { identity: "abc", visible: true };
      },
    });
    await signDocument(s);
    expect(asked).toEqual([
      "list",
      "choose:abc",
      "savedImage",
      "appearance:abc:saved",
      "place",
      "saveAs:report-signed.pdf",
      "sign:abc:/docs/report-signed.pdf",
    ]);
    // The panel's image and options, not the saved image the panel was offered.
    expect(placements).toEqual([
      { page: 7, rect: [20, 30, 170, 90], image: saved, options: chosenLook.options },
    ]);
  });

  it("hands the panel no image when none is saved, and signs what the panel answers", async () => {
    const offered: (SignatureImage | null)[] = [];
    const { placements, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: true }),
      savedImage: async () => null,
      appearance: async (_identity, image) => {
        offered.push(image);
        return { image: null, options: { ...chosenLook.options, reason: "" } };
      },
    });
    await signDocument(s);
    expect(offered).toEqual([null]);
    expect(placements).toEqual([
      {
        page: 7,
        rect: [20, 30, 170, 90],
        image: null,
        options: { ...chosenLook.options, reason: "" },
      },
    ]);
  });

  it("stops without a word, and asks nothing more, when the reader cancels the panel", async () => {
    const { asked, placements, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: true }),
      appearance: async () => {
        asked.push("appearance");
        return null;
      },
    });
    expect(await signDocument(s)).toBeNull();
    expect(asked).toEqual(["list", "savedImage", "appearance"]);
    expect(placements).toEqual([]);
  });

  it("stops without a word when the reader escapes the placement", async () => {
    const { asked, placements, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: true }),
      place: async () => {
        asked.push("place");
        return null;
      },
    });
    expect(await signDocument(s)).toBeNull();
    expect(asked).toEqual(["list", "savedImage", "appearance:abc:saved", "place"]);
    expect(placements).toEqual([]);
  });

  it("stops without a word when the reader cancels the save panel after placing", async () => {
    const { asked, placements, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: true }),
      saveAs: async (suggested) => {
        asked.push(`saveAs:${suggested}`);
        return null;
      },
    });
    expect(await signDocument(s)).toBeNull();
    expect(asked).toEqual([
      "list",
      "savedImage",
      "appearance:abc:saved",
      "place",
      "saveAs:report-signed.pdf",
    ]);
    expect(placements).toEqual([]);
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

describe("the chooser", () => {
  /** Opens the chooser on a fake DOM and hands back its controls. */
  function open(choices: Choice[]) {
    const dom = installFakeDom();
    const body = new FakeElement("body");
    Object.assign(globalThis.document, { body, activeElement: null });
    const create = document.createElement.bind(document);
    const spy = vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
      const node = create(tag) as unknown as FakeElement & Record<string, unknown>;
      if (tag === "dialog") Object.assign(node, { showModal: () => {}, close: () => {} });
      if (tag === "input") Object.assign(node, { checked: false, focus: () => {} });
      // A label is handed its caption as a string, which the fake tree has no node for.
      if (tag === "label")
        Object.assign(node, {
          append: (...kids: unknown[]) => {
            for (const kid of kids) if (typeof kid !== "string") node.appendChild(kid as FakeElement);
          },
        });
      return node as unknown as HTMLElement;
    }) as typeof document.createElement);
    const answer = askIdentity(choices);
    const nodes = (root: FakeElement): FakeElement[] =>
      root.children.flatMap((child) => [child, ...nodes(child)]);
    const all = nodes(body) as (FakeElement & { checked: boolean; value: string; name: string })[];
    const radio = (name: string, value: string) =>
      all.find((node) => node.tagName === "input" && node.name === name && node.value === value)!;
    const button = (text: string) => all.find((node) => node.tagName === "button" && node.textContent === text)!;
    const done = () => {
      spy.mockRestore();
      dom.restore();
    };
    return { answer, radio, button, done };
  }

  it("answers an invisible signature unless the reader picks a visible one", async () => {
    const first = open([choice]);
    expect(first.radio("sign-appearance", "invisible").checked).toBe(true);
    expect(first.radio("sign-appearance", "visible").checked).toBe(false);
    first.button("Sign…").dispatch("click", {});
    expect(await first.answer).toEqual({ identity: "abc", visible: false });
    first.done();

    const second = open([choice]);
    second.radio("sign-appearance", "invisible").checked = false;
    second.radio("sign-appearance", "visible").checked = true;
    second.button("Sign…").dispatch("click", {});
    expect(await second.answer).toEqual({ identity: "abc", visible: true });
    second.done();
  });
});
