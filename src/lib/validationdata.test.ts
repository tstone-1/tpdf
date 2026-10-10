import { describe, expect, it, vi } from "vitest";

import type { Properties } from "./properties";
import { CHOICE_KEY, SERVERS } from "./signtimestamp";
import { FakeElement, installFakeDom } from "./testdom";
import {
  AUTHORITY_KEY,
  CHOOSE,
  NOTHING_SIGNED,
  UNSAVED,
  WAITING,
  addValidationData,
  askAuthority,
  hasSignature,
  keptName,
  type ValidationShell,
} from "./validationdata";

/** A document's properties with these signature fields, signed or empty. */
function properties(...signed: boolean[]): Properties {
  return { signatures: signed.map((is) => ({ signed: is })) } as unknown as Properties;
}

const SAID =
  "Added long-term validation data for the signature Signature1 and the archive timestamp " +
  "Signature2, saved to contract-long-term.pdf.";

/** A shell that records every question asked of it, in order. */
function shell(overrides: Partial<ValidationShell> = {}) {
  const asked: string[] = [];
  const s: ValidationShell = {
    settle: async () => void asked.push("settle"),
    dirty: () => (asked.push("dirty"), false),
    openPath: "/docs/contract.pdf",
    properties: async () => (asked.push("properties"), properties(true)),
    chooseAuthority: async () => (asked.push("authority"), "https://timestamp.sectigo.com"),
    saveAs: async (suggested) => (asked.push(`save:${suggested}`), "/docs/contract-long-term.pdf"),
    add: async (path, timestamp) => (asked.push(`add:${path}:${timestamp}`), SAID),
    waiting: (message) => void asked.push(message === null ? "waiting:over" : `waiting:${message}`),
    ...overrides,
  };
  return { asked, s };
}

describe("adding long-term validation data", () => {
  it("asks in order, and answers the backend's sentence", async () => {
    const { asked, s } = shell();
    expect(await addValidationData(s)).toBe(SAID);
    expect(asked).toEqual([
      "settle",
      "dirty",
      "properties",
      "authority",
      "save:contract-long-term.pdf",
      `waiting:${WAITING}`,
      "add:/docs/contract-long-term.pdf:https://timestamp.sectigo.com",
      "waiting:over",
    ]);
  });

  it("says it is waiting only while the backend works, and stops when it refuses", async () => {
    const { asked, s } = shell({
      add: async () => {
        asked.push("add");
        throw new Error("nothing was written");
      },
    });
    await expect(addValidationData(s)).rejects.toThrow("nothing was written");
    expect(asked.slice(-3)).toEqual([`waiting:${WAITING}`, "add", "waiting:over"]);
    // Nothing is waited for when the reader cancelled before the backend was asked.
    const cancelled = shell({ saveAs: async () => null });
    await addValidationData(cancelled.s);
    expect(cancelled.asked.some((step) => step.startsWith("waiting:"))).toBe(false);
  });

  it("refuses unsaved edits before anything is read or asked", async () => {
    const { asked, s } = shell({ dirty: () => true });
    expect(await addValidationData(s)).toBe(UNSAVED);
    expect(asked).toEqual(["settle"]);
  });

  it("settles drafts before it reads dirty", async () => {
    let dirty = false;
    const { s } = shell({
      settle: async () => {
        dirty = true;
      },
      dirty: () => dirty,
    });
    expect(await addValidationData(s)).toBe(UNSAVED);
  });

  it("tells a document with no signature so, before any dialog", async () => {
    for (const fields of [[], [false]]) {
      const { asked, s } = shell({
        properties: async () => (asked.push("properties"), properties(...fields)),
      });
      expect(await addValidationData(s)).toBe(NOTHING_SIGNED);
      expect(asked).toEqual(["settle", "dirty", "properties"]);
    }
  });

  it("is cancelled by the dialog and by the save panel, and adds nothing", async () => {
    const dialog = shell({ chooseAuthority: async () => null });
    expect(await addValidationData(dialog.s)).toBeNull();
    expect(dialog.asked).not.toContain("save:contract-long-term.pdf");

    const panel = shell({ saveAs: async () => null });
    expect(await addValidationData(panel.s)).toBeNull();
    expect(panel.asked.some((step) => step.startsWith("add:"))).toBe(false);
  });

  it("throws the backend's refusal on", async () => {
    const { s } = shell({
      add: async () => {
        throw new Error("the signer of Signature1 is not trusted — nothing was written");
      },
    });
    await expect(addValidationData(s)).rejects.toThrow("nothing was written");
  });

  it("suggests a name beside the original", () => {
    expect(keptName("/docs/contract.pdf")).toBe("contract-long-term.pdf");
    expect(keptName("C:\\docs\\Vertrag.PDF")).toBe("Vertrag-long-term.pdf");
  });

  it("counts a signed field and not an empty one", () => {
    expect(hasSignature(properties(false, true))).toBe(true);
    expect(hasSignature(properties(false))).toBe(false);
    expect(hasSignature(properties())).toBe(false);
  });
});

/** Storage a test can read back. */
function memory(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  const storage = () => ({
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  });
  return { store, storage };
}

describe("the authority dialog", () => {
  function open(storage: () => Pick<Storage, "getItem" | "setItem">) {
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
    const answer = askAuthority(storage);
    const nodes = (root: FakeElement): FakeElement[] =>
      root.children.flatMap((child) => [child, ...nodes(child)]);
    const all = nodes(body) as (FakeElement & { checked: boolean; value: string; name: string })[];
    const radios = all.filter((n) => n.tagName === "input" && n.name === "validation-authority");
    const radio = (value: string) => radios.find((n) => n.value === value)!;
    const button = (text: string) => all.find((n) => n.tagName === "button" && n.textContent === text)!;
    const address = all.find((n) => n.attributes.get("aria-label") === "Timestamp authority address")!;
    const alert = all.find((n) => n.attributes.get("role") === "alert")!;
    const dialog = all.find((n) => n.tagName === "dialog")!;
    const done = () => {
      spy.mockRestore();
      dom.restore();
    };
    return { answer, radios, radio, button, address, alert, dialog, done };
  }

  /** Whether a promise has settled by the next turn of the event loop. */
  async function settled(promise: Promise<unknown>): Promise<boolean> {
    let is = false;
    void promise.then(() => {
      is = true;
    });
    await Promise.resolve();
    await Promise.resolve();
    return is;
  }

  it("preselects nothing the first time, and holds Add until an authority is chosen", async () => {
    const { store, storage } = memory();
    const d = open(storage);
    expect(d.radios.map((r) => r.value)).toEqual([...SERVERS.map((s) => s.name), "other"]);
    expect(d.radios.some((r) => r.checked)).toBe(false);
    d.button("Add…").dispatch("click", {});
    expect(await settled(d.answer)).toBe(false);
    expect(d.alert.textContent).toBe(CHOOSE);
    expect(store.size).toBe(0);

    d.radio("digicert").checked = true;
    d.button("Add…").dispatch("click", {});
    expect(await d.answer).toBe("http://timestamp.digicert.com");
    d.done();
  });

  it("remembers the choice under its own key, and never a signing's", async () => {
    const { store, storage } = memory({ [CHOICE_KEY]: JSON.stringify({ server: "sectigo", url: "" }) });
    const first = open(storage);
    // A signing's remembered authority is not this dialog's.
    expect(first.radios.some((r) => r.checked)).toBe(false);
    first.radio("globalsign").checked = true;
    first.button("Add…").dispatch("click", {});
    await first.answer;
    first.done();
    expect(JSON.parse(store.get(AUTHORITY_KEY) ?? "null")).toEqual({ server: "globalsign", url: "" });
    expect(JSON.parse(store.get(CHOICE_KEY) ?? "null")).toEqual({ server: "sectigo", url: "" });

    const second = open(storage);
    expect(second.radio("globalsign").checked).toBe(true);
    second.done();
  });

  it("holds another authority's address to what tpdf asks", async () => {
    const { store, storage } = memory();
    const d = open(storage);
    d.radio("other").checked = true;
    d.address.value = "ftp://example.com/tsa";
    d.button("Add…").dispatch("click", {});
    expect(await settled(d.answer)).toBe(false);
    expect(d.alert.textContent).toContain("http:// or https://");
    expect(store.size).toBe(0);

    d.address.value = " https://tsa.example.com/stamp ";
    d.button("Add…").dispatch("click", {});
    expect(await d.answer).toBe("https://tsa.example.com/stamp");
    d.done();
  });

  it("answers null for Cancel and for Escape, and remembers nothing", async () => {
    const { store, storage } = memory();
    const cancelled = open(storage);
    cancelled.radio("digicert").checked = true;
    cancelled.button("Cancel").dispatch("click", {});
    expect(await cancelled.answer).toBeNull();
    cancelled.done();

    const escaped = open(storage);
    escaped.radio("digicert").checked = true;
    let prevented = false;
    escaped.dialog.dispatch("cancel", { preventDefault: () => void (prevented = true) });
    expect(await escaped.answer).toBeNull();
    expect(prevented).toBe(true);
    escaped.done();
    expect(store.size).toBe(0);
  });
});
