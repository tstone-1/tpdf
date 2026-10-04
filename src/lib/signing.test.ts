import { describe, expect, it, vi } from "vitest";

import type { PageId } from "./pages";
import type { SignatureImage } from "./signature";
import type { Appearance } from "./signappearance";
import {
  NOT_WRITTEN,
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
  type SignOutcome,
  type Signed,
  type SigningShell,
} from "./signing";
import { CHOICE_KEY, LONG_TERM_KEY } from "./signtimestamp";
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
      return identity === undefined ? null : { identity, visible: false, timestamp: null };
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
    sign: async (identity, path, placement, timestamp) => {
      asked.push(`sign:${identity}:${path}${timestamp === null ? "" : `:${timestamp}`}`);
      placements.push(placement);
      return { signed, unstamped: null };
    },
    stampFailed: async (why) => {
      asked.push(`stampFailed:${why}`);
      return null;
    },
    longTermFailed: async (why) => {
      asked.push(`longTermFailed:${why}`);
      return null;
    },
    resume: async (pending, timestamp, longTerm) => {
      asked.push(`resume:${pending}:${timestamp}${longTerm ? ":long-term" : ""}`);
      return { signed, unstamped: null };
    },
    discard: async (pending) => {
      asked.push(`discard:${pending}`);
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
        return { identity: "abc", visible: true, timestamp: null };
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
      choose: async () => ({ identity: "abc", visible: true, timestamp: null }),
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
      choose: async () => ({ identity: "abc", visible: true, timestamp: null }),
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
      choose: async () => ({ identity: "abc", visible: true, timestamp: null }),
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
      choose: async () => ({ identity: "abc", visible: true, timestamp: null }),
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

describe("signing a signature field the document has", () => {
  const field = { name: "Approved", page: 9 as PageId, rect: [40, 50, 240, 110] as [number, number, number, number] };
  /** A shell that also records the field each question was asked about. */
  function inField(visible: boolean) {
    const into: (string | null)[] = [];
    const made = shell({
      choose: async (_choices, name) => {
        into.push(name);
        return { identity: "abc", visible, timestamp: null };
      },
      sign: async (_identity, _path, placement, _timestamp, _longTerm, name) => {
        into.push(name);
        made.placements.push(placement);
        return { signed: { path: "/docs/report-signed.pdf", field: "Approved", signatures: [] }, unstamped: null };
      },
    });
    return { ...made, into };
  }

  it("draws a visible one in the field, where the field is, and drags nothing", async () => {
    const { asked, placements, into, shell: s } = inField(true);
    await signDocument(s, field);
    expect(asked).toEqual(["list", "savedImage", "appearance:abc:saved", "saveAs:report-signed.pdf"]);
    expect(placements).toEqual([{ page: 9, rect: [40, 50, 240, 110], image: saved, options: chosenLook.options }]);
    // The chooser is told which field, and so is the signing.
    expect(into).toEqual(["Approved", "Approved"]);
  });

  it("writes an invisible one into the field all the same", async () => {
    const { asked, placements, into, shell: s } = inField(false);
    await signDocument(s, field);
    expect(asked).toEqual(["list", "saveAs:report-signed.pdf"]);
    expect(placements).toEqual([null]);
    expect(into).toEqual(["Approved", "Approved"]);
  });

  it("names no field, and asks for the drag, when none was given", async () => {
    const { asked, into, shell: s } = inField(true);
    await signDocument(s);
    expect(asked).toContain("place");
    expect(into).toEqual([null, null]);
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
  function open(choices: Choice[], storage?: () => Pick<Storage, "getItem" | "setItem">, into: string | null = null) {
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
    const answer = askIdentity(choices, storage, into);
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
    const field = (label: string) =>
      all.find((node) => node.tagName === "input" && node.attributes.get("aria-label") === label)!;
    const alert = () => all.find((node) => node.attributes.get("role") === "alert")!;
    return { answer, radio, button, field, alert, done };
  }

  it("offers a visible signature first when a signature field is being signed", async () => {
    const pressed = open([choice], undefined, "Approved");
    expect(pressed.radio("sign-appearance", "visible").checked).toBe(true);
    expect(pressed.radio("sign-appearance", "invisible").checked).toBe(false);
    pressed.button("Sign…").dispatch("click", {});
    expect(await pressed.answer).toEqual({ identity: "abc", visible: true, timestamp: null, longTerm: false });
    pressed.done();
  });

  it("answers an invisible signature unless the reader picks a visible one", async () => {
    const first = open([choice]);
    expect(first.radio("sign-appearance", "invisible").checked).toBe(true);
    expect(first.radio("sign-appearance", "visible").checked).toBe(false);
    first.button("Sign…").dispatch("click", {});
    expect(await first.answer).toEqual({ identity: "abc", visible: false, timestamp: null, longTerm: false });
    first.done();

    const second = open([choice]);
    second.radio("sign-appearance", "invisible").checked = false;
    second.radio("sign-appearance", "visible").checked = true;
    second.button("Sign…").dispatch("click", {});
    expect(await second.answer).toEqual({ identity: "abc", visible: true, timestamp: null, longTerm: false });
    second.done();
  });
});

/** A storage double: what was written, and what reads back. */
function memory(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  return {
    store,
    storage: () => ({
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    }),
  };
}

describe("a timestamp", () => {
  const stampedBack: Signed = {
    path: "/docs/report-signed.pdf",
    field: "Signature1",
    signatures: [
      {
        field: "Signature1",
        integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "ECDSA P-256" },
        ours: true,
        timestamp: {
          when: "2026-09-28 10:11:12 UTC",
          authority: { subject: "CN=DigiCert", subject_cn: "DigiCert SHA256 RSA4096 Timestamp Responder 2025 1" } as never,
          integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "RSA" },
          trust: null,
          attested: true,
          revocation: null,
          revocation_chain: null,
        },
      },
    ],
  };

  it("is asked of the chosen authority, and the closing sentence says what it attests", async () => {
    const { asked, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: "http://timestamp.digicert.com" }),
      sign: async (identity, path, _placement, timestamp) => {
        asked.push(`sign:${identity}:${path}:${timestamp}`);
        return { signed: stampedBack, unstamped: null };
      },
    });
    const said = await signDocument(s);
    expect(asked).toContain("sign:abc:/docs/report-signed.pdf:http://timestamp.digicert.com");
    expect(said).toBe(
      "Signed as Signature1 and saved to report-signed.pdf. Read back after writing, the " +
        "signature is intact. Timestamp: 2026-09-28 10:11:12 UTC, attested by DigiCert SHA256 " +
        "RSA4096 Timestamp Responder 2025 1 — the timestamp checks out under the key in its " +
        "certificate and covers this signature (SHA-256, RSA).",
    );
  });

  it("asks nobody when none was chosen", async () => {
    const { asked, shell: s } = shell();
    await signDocument(s);
    expect(asked).toContain("sign:abc:/docs/report-signed.pdf");
    expect(asked.some((a) => a.startsWith("resume") || a.startsWith("stampFailed"))).toBe(false);
  });

  /** A shell whose first signing answers that the timestamp did not come. */
  function failing(answer: "retry" | "without" | null, second: SignOutcome) {
    const url = "https://tsa.example/";
    return shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: url }),
      sign: async () => ({
        signed: null,
        unstamped: { why: "tpdf could not reach tsa.example", pending: 41 },
      }),
      stampFailed: async () => answer,
      resume: async (pending, timestamp) => {
        return pending === 41 && (timestamp === url || timestamp === null)
          ? second
          : { signed: null, unstamped: { why: `resumed ${pending} with ${timestamp}`, pending: 0 } };
      },
    });
  }

  it("writes nothing when it does not come, until the reader chooses to sign without one", async () => {
    const { asked, shell: s } = failing("without", {
      signed: { ...stampedBack, signatures: [{ ...stampedBack.signatures[0]!, timestamp: null }] },
      unstamped: null,
    });
    const calls: string[] = [];
    const recording: SigningShell = {
      ...s,
      stampFailed: async (why) => {
        calls.push(`stampFailed:${why}`);
        return "without";
      },
      resume: async (pending, timestamp) => {
        calls.push(`resume:${pending}:${timestamp}`);
        return {
          signed: { ...stampedBack, signatures: [{ ...stampedBack.signatures[0]!, timestamp: null }] },
          unstamped: null,
        };
      },
    };
    const said = await signDocument(recording);
    expect(calls).toEqual(["stampFailed:tpdf could not reach tsa.example", "resume:41:null"]);
    expect(said).toBe(
      "Signed as Signature1 and saved to report-signed.pdf. Read back after writing, the signature is intact.",
    );
    expect(asked).not.toContain("discard:41");
  });

  it("asks the same authority again when the reader tries again", async () => {
    const calls: string[] = [];
    const { shell: s } = failing("retry", { signed: stampedBack, unstamped: null });
    let round = 0;
    const said = await signDocument({
      ...s,
      stampFailed: async () => {
        round += 1;
        return round === 1 ? "retry" : "without";
      },
      resume: async (pending, timestamp) => {
        calls.push(`resume:${pending}:${timestamp}`);
        return round === 1
          ? { signed: null, unstamped: { why: "still down", pending } }
          : { signed: stampedBack, unstamped: null };
      },
    });
    expect(calls).toEqual(["resume:41:https://tsa.example/", "resume:41:null"]);
    expect(said).toContain("Timestamp:");
  });

  it("drops the signature and says nothing was written when the reader cancels", async () => {
    const { asked, shell: s } = failing(null, { signed: stampedBack, unstamped: null });
    expect(await signDocument(s)).toBe(NOT_WRITTEN);
    expect(asked).toContain("discard:41");
    expect(asked.some((a) => a.startsWith("resume"))).toBe(false);
  });
});

describe("the chooser's timestamp", () => {
  function open(choices: Choice[], storage: () => Pick<Storage, "getItem" | "setItem">) {
    const dom = installFakeDom();
    const body = new FakeElement("body");
    Object.assign(globalThis.document, { body, activeElement: null });
    const create = document.createElement.bind(document);
    const spy = vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
      const node = create(tag) as unknown as FakeElement & Record<string, unknown>;
      if (tag === "dialog") Object.assign(node, { showModal: () => {}, close: () => {} });
      if (tag === "input") Object.assign(node, { checked: false, focus: () => {}, value: "" });
      if (tag === "label")
        Object.assign(node, {
          append: (...kids: unknown[]) => {
            for (const kid of kids) if (typeof kid !== "string") node.appendChild(kid as FakeElement);
          },
        });
      return node as unknown as HTMLElement;
    }) as typeof document.createElement);
    const answer = askIdentity(choices, storage);
    const nodes = (root: FakeElement): FakeElement[] =>
      root.children.flatMap((child) => [child, ...nodes(child)]);
    const all = nodes(body) as (FakeElement & { checked: boolean; value: string; name: string })[];
    const stamps = () => all.filter((node) => node.tagName === "input" && node.name === "sign-timestamp");
    const radio = (value: string) => stamps().find((node) => node.value === value)!;
    const url = all.find((node) => node.attributes.get("aria-label") === "Timestamp authority address")!;
    const alert = all.find((node) => node.attributes.get("role") === "alert")!;
    const sign = all.find((node) => node.tagName === "button" && node.textContent === "Sign…")!;
    const done = () => {
      spy.mockRestore();
      dom.restore();
    };
    return { answer, stamps, radio, url, alert, sign, done };
  }

  it("preselects no authority for a reader who has never chosen, and asks nobody", async () => {
    const { storage } = memory();
    const c = open([choice], storage);
    expect(c.stamps().filter((r) => r.checked).map((r) => r.value)).toEqual(["none"]);
    c.sign.dispatch("click", {});
    expect(await c.answer).toEqual({ identity: "abc", visible: false, timestamp: null, longTerm: false });
    c.done();
  });

  it("remembers the reader's choice and offers it next time", async () => {
    const { store, storage } = memory();
    const first = open([choice], storage);
    first.radio("none").checked = false;
    first.radio("sectigo").checked = true;
    first.sign.dispatch("click", {});
    expect(await first.answer).toEqual({
      identity: "abc",
      visible: false,
      timestamp: "https://timestamp.sectigo.com",
      longTerm: false,
    });
    first.done();
    expect(JSON.parse(store.get(CHOICE_KEY)!)).toEqual({ server: "sectigo", url: "" });

    const second = open([choice], storage);
    expect(second.stamps().filter((r) => r.checked).map((r) => r.value)).toEqual(["sectigo"]);
    second.done();
  });

  it("holds the chooser open on another authority's address it would not ask", async () => {
    const { store, storage } = memory();
    const c = open([choice], storage);
    c.radio("none").checked = false;
    c.radio("other").checked = true;
    c.url.value = "ftp://tsa.example/";
    c.sign.dispatch("click", {});
    expect(c.alert.textContent).toContain("http:// or https://");
    expect(store.has(CHOICE_KEY)).toBe(false);
    c.url.value = " https://tsa.example/rfc3161 ";
    c.sign.dispatch("click", {});
    expect(await c.answer).toEqual({
      identity: "abc",
      visible: false,
      timestamp: "https://tsa.example/rfc3161",
      longTerm: false,
    });
    c.done();
  });
});

describe("long-term validation data", () => {
  const url = "https://tsa.example/";
  const good = {
    standing: "good",
    why: null,
    source: "ocsp",
    issued: "2026-09-28 09:00:00 UTC",
    next: "2026-09-28 21:00:00 UTC",
    revoked: "",
    reason: null,
    basis: "claimed",
    moment: "2026-09-28 10:11:12 UTC",
    after_moment: false,
  } as const;
  const withData: Signed = {
    path: "/docs/report-signed.pdf",
    field: "Signature1",
    signatures: [
      {
        field: "Signature1",
        integrity: { verdict: "intact", why: null, digest: "SHA-256", method: "ECDSA P-256" },
        ours: true,
        revocation: good,
      },
    ],
  };

  it("is asked for only with a timestamp, and said when it reads back good", async () => {
    const calls: string[] = [];
    const { shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: url, longTerm: true }),
      sign: async (_identity, _path, _placement, timestamp, longTerm) => {
        calls.push(`sign:${timestamp}:${longTerm}`);
        return { signed: withData, unstamped: null };
      },
    });
    const said = await signDocument(s);
    expect(calls).toEqual([`sign:${url}:true`]);
    expect(said).toContain(" Revocation: not revoked — an OCSP response in the document");

    const without: string[] = [];
    const { shell: t } = shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: null, longTerm: true }),
      sign: async (_identity, _path, _placement, timestamp, longTerm) => {
        without.push(`sign:${timestamp}:${longTerm}`);
        return { signed: withData, unstamped: null };
      },
    });
    await signDocument(t);
    expect(without).toEqual(["sign:null:false"]);
  });

  /** A shell whose first signing answers that the long-term data did not come. */
  function failing(answers: AfterStampAnswer[]) {
    const calls: string[] = [];
    const { asked, shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: url, longTerm: true }),
      sign: async () => ({
        signed: null,
        unstamped: { why: "no revocation data", pending: 52, stage: "long_term" },
      }),
      stampFailed: async (why) => {
        calls.push(`stampFailed:${why}`);
        return null;
      },
      longTermFailed: async (why) => {
        calls.push(`longTermFailed:${why}`);
        return answers.shift() ?? null;
      },
      resume: async (pending, timestamp, longTerm) => {
        calls.push(`resume:${pending}:${timestamp}:${longTerm}`);
        return answers.length > 0
          ? { signed: null, unstamped: { why: "still none", pending, stage: "long_term" } }
          : { signed: withData, unstamped: null };
      },
    });
    return { calls, asked, s };
  }
  type AfterStampAnswer = "retry" | "without" | null;

  it("asks its own question when it does not come, and signs without it only when told", async () => {
    const { calls, s } = failing(["without"]);
    await signDocument(s);
    expect(calls).toEqual(["longTermFailed:no revocation data", `resume:52:${url}:false`]);
  });

  it("gathers it again, keeping the timestamp, when the reader tries again", async () => {
    const { calls, s } = failing(["retry", "without"]);
    await signDocument(s);
    expect(calls).toEqual([
      "longTermFailed:no revocation data",
      `resume:52:${url}:true`,
      "longTermFailed:still none",
      `resume:52:${url}:false`,
    ]);
  });

  it("is asked again with a timestamp tried again, and never with none", async () => {
    const calls: string[] = [];
    let round = 0;
    const { shell: s } = shell({
      choose: async () => ({ identity: "abc", visible: false, timestamp: url, longTerm: true }),
      sign: async () => ({
        signed: null,
        unstamped: { why: "tsa down", pending: 60, stage: "timestamp" },
      }),
      stampFailed: async () => {
        round += 1;
        return round === 1 ? "retry" : "without";
      },
      resume: async (pending, timestamp, longTerm) => {
        calls.push(`resume:${pending}:${timestamp}:${longTerm}`);
        return round === 1
          ? { signed: null, unstamped: { why: "still down", pending, stage: "timestamp" } }
          : { signed: withData, unstamped: null };
      },
    });
    await signDocument(s);
    expect(calls).toEqual([`resume:60:${url}:true`, "resume:60:null:false"]);
  });

  it("drops the signature when the reader cancels", async () => {
    const { asked, s } = failing([null]);
    expect(await signDocument(s)).toBe(NOT_WRITTEN);
    expect(asked).toContain("discard:52");
  });
});

describe("the chooser's long-term checkbox", () => {
  function open(storage: () => Pick<Storage, "getItem" | "setItem">) {
    const dom = installFakeDom();
    const body = new FakeElement("body");
    Object.assign(globalThis.document, { body, activeElement: null });
    const create = document.createElement.bind(document);
    const spy = vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
      const node = create(tag) as unknown as FakeElement & Record<string, unknown>;
      if (tag === "dialog") Object.assign(node, { showModal: () => {}, close: () => {} });
      if (tag === "input")
        Object.assign(node, { checked: false, disabled: false, focus: () => {}, value: "" });
      if (tag === "label")
        Object.assign(node, {
          append: (...kids: unknown[]) => {
            for (const kid of kids) if (typeof kid !== "string") node.appendChild(kid as FakeElement);
          },
        });
      return node as unknown as HTMLElement;
    }) as typeof document.createElement);
    const answer = askIdentity([choice], storage);
    const nodes = (root: FakeElement): FakeElement[] =>
      root.children.flatMap((child) => [child, ...nodes(child)]);
    const all = nodes(body) as (FakeElement & {
      checked: boolean;
      disabled: boolean;
      value: string;
      name: string;
    })[];
    const radio = (value: string) =>
      all.find((n) => n.tagName === "input" && n.name === "sign-timestamp" && n.value === value)!;
    const box = all.find((n) => n.tagName === "input" && n.name === "sign-long-term")!;
    const sign = all.find((n) => n.tagName === "button" && n.textContent === "Sign…")!;
    const done = () => {
      spy.mockRestore();
      dom.restore();
    };
    return { answer, radio, box, sign, done };
  }

  it("is unticked, and cannot be ticked, until an authority is chosen", async () => {
    const { storage } = memory();
    const c = open(storage);
    expect(c.box.checked).toBe(false);
    expect(c.box.disabled).toBe(true);
    c.radio("none").checked = false;
    c.radio("digicert").checked = true;
    c.radio("digicert").dispatch("change", {});
    expect(c.box.disabled).toBe(false);
    c.sign.dispatch("click", {});
    expect((await c.answer)?.longTerm).toBe(false);
    c.done();
  });

  it("is remembered, and asked for only with a timestamp", async () => {
    const { store, storage } = memory();
    const first = open(storage);
    first.radio("none").checked = false;
    first.radio("sectigo").checked = true;
    first.box.checked = true;
    first.sign.dispatch("click", {});
    expect(await first.answer).toEqual({
      identity: "abc",
      visible: false,
      timestamp: "https://timestamp.sectigo.com",
      longTerm: true,
    });
    first.done();
    expect(store.get(LONG_TERM_KEY)).toBe("true");

    const second = open(storage);
    expect(second.box.checked).toBe(true);
    // Ticked, and no authority: nothing more is asked of anybody.
    second.radio("sectigo").checked = false;
    second.radio("none").checked = true;
    second.sign.dispatch("click", {});
    expect((await second.answer)?.longTerm).toBe(false);
    second.done();
  });
});
