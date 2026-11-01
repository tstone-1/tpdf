import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import sample from "../../src-tauri/testdata/replies/Properties.json";
import { rootMayBeMissing } from "./integrity";
import type { Properties } from "./properties";
import { PropertiesDialog } from "./propertiesdialog";
import { installFakeDom, settle, type FakeDom, type FakeElement } from "./testdom";

type Trust = NonNullable<Properties["signatures"][number]["trust"]>;

const missing: Trust = { standing: "untrusted", why: "root", store: "windows", attested_at: "" };
const trusted: Trust = { standing: "trusted", why: null, store: "windows", attested_at: "" };

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
});

afterEach(() => {
  dom.restore();
});

/** The committed reply, with every signature's and timestamp's trust replaced. */
function withTrust(trust: Trust, encrypted = false): Properties {
  const base = structuredClone(sample) as unknown as Properties;
  for (const signature of base.signatures) {
    signature.trust = trust;
    if (signature.timestamp) signature.timestamp.trust = trust;
  }
  if (!encrypted) base.encryption = null;
  return base;
}

function everything(under: FakeElement): FakeElement[] {
  return [under, ...under.children.flatMap(everything)];
}

function offer(): (FakeElement & { disabled: boolean }) | undefined {
  return everything(dom.root).find((e) => e.tagName === "button") as
    | (FakeElement & { disabled: boolean })
    | undefined;
}

function said(text: string): boolean {
  return everything(dom.root).some((e) => e.textContent === text);
}

function dialog(ask: () => Promise<Properties>): PropertiesDialog {
  return new PropertiesDialog(dom.root as unknown as HTMLElement, ask);
}

describe("rootMayBeMissing", () => {
  it("is only a root or an incomplete chain, not trusted, on Windows", () => {
    expect(rootMayBeMissing(missing)).toBe(true);
    expect(rootMayBeMissing({ ...missing, why: "incomplete" })).toBe(true);
    expect(rootMayBeMissing({ ...missing, why: "dates" })).toBe(false);
    expect(rootMayBeMissing({ ...missing, store: "mac" })).toBe(false);
    expect(rootMayBeMissing({ ...missing, store: "linux" })).toBe(false);
    expect(rootMayBeMissing(trusted)).toBe(false);
    expect(rootMayBeMissing(null)).toBe(false);
  });
});

describe("the offer to ask Windows for a root", () => {
  it("asks nobody until it is pressed", () => {
    const ask = vi.fn(() => Promise.resolve(withTrust(trusted)));
    dialog(ask).show(withTrust(missing), "");
    expect(offer()).toBeDefined();
    expect(ask).not.toHaveBeenCalled();
  });

  it("is not offered where no root may be missing, or on an encrypted document", () => {
    const shown = dialog(() => Promise.resolve(withTrust(trusted)));
    shown.show(withTrust({ ...missing, store: "mac" }), "");
    expect(offer()).toBeUndefined();
    shown.show(withTrust(trusted), "");
    expect(offer()).toBeUndefined();
    expect(sample.encryption).not.toBeNull();
    shown.show(withTrust(missing, true), "");
    expect(offer()).toBeUndefined();
  });

  it("is offered for a timestamp's authority alone", () => {
    const properties = withTrust(trusted);
    const stamped = properties.signatures.find((s) => s.timestamp);
    if (!stamped?.timestamp) throw new Error("the sample has no timestamp");
    stamped.timestamp.trust = missing;
    dialog(() => Promise.resolve(properties)).show(properties, "");
    expect(offer()).toBeDefined();
  });

  it("shows the document as read again, and goes once the root came", async () => {
    dialog(() => Promise.resolve(withTrust(trusted))).show(withTrust(missing), "");
    offer()!.dispatch("click", {});
    expect(offer()!.disabled).toBe(true);
    await settle();
    expect(offer()).toBeUndefined();
  });

  it("says so when the answer did not change, and a failure keeps the first answer", async () => {
    let answer = () => Promise.resolve(withTrust(missing));
    dialog(() => answer()).show(withTrust(missing), "");
    offer()!.dispatch("click", {});
    await settle();
    expect(said("Windows did not fetch a root that changes this answer.")).toBe(true);

    answer = () => Promise.reject("the file on disk changed");
    offer()!.dispatch("click", {});
    await settle();
    expect(said("the file on disk changed")).toBe(true);
    expect(offer()).toBeDefined();
    expect(offer()!.disabled).toBeFalsy();
  });
});
