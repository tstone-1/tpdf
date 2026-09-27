import { afterEach, describe, expect, it, vi } from "vitest";

import type { SignatureImage } from "./signature";
import {
  AppearancePanel,
  DEFAULT_PREFERENCE,
  MAX_NOTE_CHARS,
  PREFERENCE_KEY,
  askAppearance,
  readPreference,
  writePreference,
  type AppearanceOptions,
  type PanelShell,
  type Preference,
  type SignaturePreview,
} from "./signappearance";
import { FakeElement, installFakeDom, settle } from "./testdom";

/** The reader's saved visual signature. */
const saved: SignatureImage = { width: 1, height: 1, rgba: [0, 0, 0, 255] };
/** One drawn in the panel. */
const fresh: SignatureImage = { width: 2, height: 1, rgba: [9, 9, 9, 255, 0, 0, 0, 0] };
/** What a preview answers. */
const picture: SignaturePreview = { png: [137, 80, 78, 71], width: 480, height: 160 };

/** Storage that is a map, and can be told to throw. */
function memory(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  let broken = false;
  const store = {
    getItem: (key: string) => {
      if (broken) throw new Error("storage is unavailable");
      return items.get(key) ?? null;
    },
    setItem: (key: string, value: string) => {
      if (broken) throw new Error("storage is unavailable");
      items.set(key, value);
    },
  };
  return { items, store: () => store, breakIt: () => (broken = true) };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

/** A panel whose previews and paints are recorded. */
function panelWith(overrides: Partial<PanelShell> = {}, preference?: Preference) {
  const asked: { image: SignatureImage | null; options: AppearanceOptions }[] = [];
  const shown: SignaturePreview[] = [];
  const shell: PanelShell = {
    saved,
    preview: async (image, options) => {
      asked.push({ image, options });
      return picture;
    },
    show: (preview) => void shown.push(preview),
    ...overrides,
  };
  return { panel: new AppearancePanel(shell, preference), asked, shown };
}

describe("the choices a panel opens with", () => {
  it("opens on the three lines and the saved image when nothing is remembered", () => {
    const { panel } = panelWith({ storage: memory().store });
    expect(panel.source).toBe("saved");
    expect(panel.image()).toBe(saved);
    expect(panel.options).toEqual(DEFAULT_PREFERENCE.options);
  });

  it("opens on no image when the saved one is wanted and there is none", () => {
    const { panel } = panelWith({ saved: null, storage: memory().store });
    expect(panel.source).toBe("none");
    expect(panel.image()).toBeNull();
  });

  it("refuses a source with nothing in it, and keeps the one it had", () => {
    const { panel } = panelWith({ saved: null, storage: memory().store });
    expect(panel.setSource("saved")).toBe(false);
    expect(panel.setSource("drawn")).toBe(false);
    expect(panel.source).toBe("none");
    panel.useDrawn(fresh);
    expect(panel.source).toBe("drawn");
    expect(panel.image()).toBe(fresh);
    expect(panel.setSource("none")).toBe(true);
    expect(panel.image()).toBeNull();
  });
});

describe("remembering the choices", () => {
  it("keeps them when asked and opens on them next time", () => {
    const storage = memory();
    const first = panelWith({ storage: storage.store }).panel;
    first.setLine("label", false);
    first.setLine("date", false);
    first.setReason("Approved");
    first.setLocation("Hamburg");
    first.setSource("none");
    first.remember = true;
    first.finish();

    const again = panelWith({ storage: storage.store }).panel;
    expect(again.options).toEqual({
      label: false,
      name: true,
      date: false,
      reason: "Approved",
      location: "Hamburg",
    });
    expect(again.source).toBe("none");
    // The pixels are not part of it: only the choice between saved and none.
    expect(JSON.parse(storage.items.get(PREFERENCE_KEY) ?? "")).toEqual({
      image: "none",
      options: again.options,
    });
  });

  it("remembers an image drawn in the panel as the saved one, never its pixels", () => {
    const storage = memory();
    const first = panelWith({ storage: storage.store }).panel;
    first.useDrawn(fresh);
    first.remember = true;
    first.finish();
    const kept = storage.items.get(PREFERENCE_KEY) ?? "";
    expect(JSON.parse(kept).image).toBe("saved");
    expect(kept).not.toContain("rgba");
    expect(panelWith({ storage: storage.store }).panel.source).toBe("saved");
  });

  it("keeps nothing unless asked, and nothing on Cancel", () => {
    const storage = memory();
    const unasked = panelWith({ storage: storage.store }).panel;
    unasked.setLine("name", false);
    unasked.finish();
    const cancelled = panelWith({ storage: storage.store }).panel;
    cancelled.setLine("name", false);
    cancelled.remember = true;
    cancelled.cancel();
    expect(storage.items.has(PREFERENCE_KEY)).toBe(false);
  });

  it("answers the choices even when storage refuses to keep them", () => {
    const storage = memory();
    const panel = panelWith({ storage: storage.store }).panel;
    storage.breakIt();
    panel.setReason("Approved");
    panel.remember = true;
    expect(panel.finish()).toEqual({
      image: saved,
      options: { ...DEFAULT_PREFERENCE.options, reason: "Approved" },
    });
    expect(writePreference(DEFAULT_PREFERENCE, storage.store)).toBe(false);
  });

  it("falls back to the defaults, whole, for anything tpdf did not write", () => {
    const good = { image: "none", options: { ...DEFAULT_PREFERENCE.options, label: false } };
    const corrupt = [
      "not json",
      "null",
      "[]",
      "7",
      JSON.stringify({ ...good, image: "drawn" }),
      JSON.stringify({ image: "none" }),
      JSON.stringify({ ...good, options: null }),
      JSON.stringify({ ...good, options: { ...good.options, label: "yes" } }),
      JSON.stringify({ ...good, options: { ...good.options, date: undefined } }),
      JSON.stringify({ ...good, options: { ...good.options, reason: 3 } }),
      JSON.stringify({ ...good, options: { ...good.options, location: "x".repeat(MAX_NOTE_CHARS + 1) } }),
    ];
    for (const raw of corrupt) {
      const storage = memory({ [PREFERENCE_KEY]: raw });
      expect(readPreference(storage.store), raw).toEqual(DEFAULT_PREFERENCE);
    }
    // Storage that throws, and storage with nothing in it.
    const broken = memory();
    broken.breakIt();
    expect(readPreference(broken.store)).toEqual(DEFAULT_PREFERENCE);
    expect(readPreference(memory().store)).toEqual(DEFAULT_PREFERENCE);
    // The control: what tpdf writes is read back.
    const kept = memory({ [PREFERENCE_KEY]: JSON.stringify(good) });
    expect(readPreference(kept.store)).toEqual(good);
  });

  it("hands out a copy of the defaults, so changing one changes nothing shared", () => {
    const first = readPreference(memory().store);
    first.options.label = false;
    expect(readPreference(memory().store).options.label).toBe(true);
    expect(DEFAULT_PREFERENCE.options.label).toBe(true);
  });
});

describe("the preview", () => {
  it("is asked for the image and options chosen now", async () => {
    const { panel, asked, shown } = panelWith({ storage: memory().store });
    panel.setLine("label", false);
    panel.setReason("Approved");
    await panel.refresh();
    expect(asked).toEqual([
      { image: saved, options: { ...DEFAULT_PREFERENCE.options, label: false, reason: "Approved" } },
    ]);
    expect(shown).toEqual([picture]);
    expect(panel.state).toEqual({ kind: "shown" });
    panel.setSource("none");
    await panel.refresh();
    expect(asked[1]?.image).toBeNull();
    panel.useDrawn(fresh);
    await panel.refresh();
    expect(asked[2]?.image).toBe(fresh);
  });

  it("shows only the latest, whichever order the answers land in", async () => {
    const early = deferred<SignaturePreview>();
    const late = deferred<SignaturePreview>();
    const answers = [early, late];
    const { panel, shown } = panelWith({
      storage: memory().store,
      preview: () => answers.shift()!.promise,
    });
    const first = panel.refresh();
    const second = panel.refresh();
    const newest = { ...picture, width: 2 };
    late.resolve(newest);
    await second;
    early.resolve(picture);
    await first;
    expect(shown).toEqual([newest]);
    expect(panel.state).toEqual({ kind: "shown" });
  });

  it("holds Place on page while the choices are refused, with the signing's words", async () => {
    let refuse = true;
    const { panel } = panelWith({
      storage: memory().store,
      preview: async () => {
        if (refuse) throw new Error("a visible signature has to show something");
        return picture;
      },
    });
    await panel.refresh();
    expect(panel.state).toEqual({ kind: "refused", why: "a visible signature has to show something" });
    expect(panel.canContinue()).toBe(false);
    refuse = false;
    await panel.refresh();
    expect(panel.canContinue()).toBe(true);
  });

  it("paints nothing and says nothing once the panel is cancelled", async () => {
    const pending = deferred<SignaturePreview>();
    const { panel, shown } = panelWith({ storage: memory().store, preview: () => pending.promise });
    const drawing = panel.refresh();
    panel.cancel();
    pending.resolve(picture);
    await drawing;
    expect(shown).toEqual([]);
    expect(panel.canContinue()).toBe(false);
  });
});

// ---------------------------------------------------------------- the dialog

type Node = FakeElement & Record<string, unknown>;

/** Opens the panel on a fake DOM and hands back its controls. */
function open(overrides: Partial<Parameters<typeof askAppearance>[0]> = {}) {
  const dom = installFakeDom();
  const body = new FakeElement("body");
  Object.assign(globalThis.document, { body, activeElement: null });
  const create = document.createElement.bind(document);
  const spy = vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
    const node = create(tag) as unknown as Node;
    if (tag === "dialog") Object.assign(node, { showModal: () => {}, close: () => {} });
    if (tag === "input") Object.assign(node, { checked: false, value: "", focus: () => {} });
    if (tag === "label")
      Object.assign(node, {
        append: (...kids: unknown[]) => {
          for (const kid of kids) if (typeof kid !== "string") node.appendChild(kid as FakeElement);
        },
      });
    return node as unknown as HTMLElement;
  }) as typeof document.createElement);
  const storage = memory();
  const previews: { image: SignatureImage | null; options: AppearanceOptions }[] = [];
  const painted: SignaturePreview[] = [];
  const answer = askAppearance({
    saved,
    storage: storage.store,
    preview: async (image, options) => {
      previews.push({ image, options });
      return picture;
    },
    draw: async () => fresh,
    paint: (_canvas, preview) => void painted.push(preview),
    typingDelay: 0,
    ...overrides,
  });
  const nodes = (): Node[] => {
    const walk = (root: FakeElement): FakeElement[] =>
      root.children.flatMap((child) => [child, ...walk(child)]);
    return walk(body) as Node[];
  };
  const input = (name: string, value?: string) =>
    nodes().find((n) => n.tagName === "input" && n.name === name && (value === undefined || n.value === value))!;
  const button = (text: string) => nodes().find((n) => n.tagName === "button" && n.textContent === text)!;
  const status = () => nodes().find((n) => n.attributes.get("role") === "status")!.textContent;
  const done = () => {
    spy.mockRestore();
    dom.restore();
  };
  return { answer, body, input, button, status, previews, painted, storage, done };
}

describe("the panel", () => {
  afterEach(() => vi.useRealTimers());

  it("previews on opening, and answers what the controls say on Place on page", async () => {
    vi.useFakeTimers();
    const ui = open();
    await settle();
    expect(ui.previews).toHaveLength(1);
    expect(ui.painted).toEqual([picture]);
    expect(ui.input("sign-appearance-image", "saved").checked).toBe(true);

    const label = ui.input("sign-appearance-label");
    label.checked = false;
    label.dispatch("change", {});
    const reason = ui.input("sign-appearance-reason");
    reason.value = "Approved";
    reason.dispatch("input", {});
    vi.runAllTimers();
    await settle();
    expect(ui.previews.at(-1)?.options).toMatchObject({ label: false, reason: "Approved" });

    // Typed and not yet drawn is still what was chosen.
    const location = ui.input("sign-appearance-location");
    location.value = "Hamburg";
    location.dispatch("input", {});
    ui.button("Place on page…").dispatch("click", {});
    expect(await ui.answer).toEqual({
      image: saved,
      options: { label: false, name: true, date: true, reason: "Approved", location: "Hamburg" },
    });
    expect(ui.body.children).toEqual([]);
    expect(ui.storage.items.has(PREFERENCE_KEY)).toBe(false);
    ui.done();
  });

  it("keeps the choices when Remember is ticked", async () => {
    const ui = open();
    await settle();
    const none = ui.input("sign-appearance-image", "none");
    none.checked = true;
    none.dispatch("change", {});
    const remember = ui.input("sign-appearance-remember");
    remember.checked = true;
    remember.dispatch("change", {});
    ui.button("Place on page…").dispatch("click", {});
    expect(await ui.answer).toMatchObject({ image: null });
    expect(JSON.parse(ui.storage.items.get(PREFERENCE_KEY) ?? "").image).toBe("none");
    ui.done();
  });

  it("uses an image drawn in the Phase 4 dialog, and keeps its choice when that is cancelled", async () => {
    const drawn = open();
    await settle();
    drawn.button("Draw or import…").dispatch("click", {});
    await settle();
    expect(drawn.previews.at(-1)?.image).toBe(fresh);
    expect(drawn.input("sign-appearance-image", "drawn").checked).toBe(true);
    drawn.button("Place on page…").dispatch("click", {});
    expect(await drawn.answer).toMatchObject({ image: fresh });
    drawn.done();

    const cancelled = open({ draw: async () => null });
    await settle();
    cancelled.button("Draw or import…").dispatch("click", {});
    await settle();
    expect(cancelled.input("sign-appearance-image", "saved").checked).toBe(true);
    cancelled.button("Place on page…").dispatch("click", {});
    expect(await cancelled.answer).toMatchObject({ image: saved });
    cancelled.done();
  });

  it("answers nothing for Cancel or Escape, and paints nothing still on its way", async () => {
    const pending = deferred<SignaturePreview>();
    const ui = open({ preview: () => pending.promise });
    ui.button("Cancel").dispatch("click", {});
    expect(await ui.answer).toBeNull();
    pending.resolve(picture);
    await settle();
    expect(ui.painted).toEqual([]);
    expect(ui.body.children).toEqual([]);
    ui.done();

    const escaped = open();
    const dialog = escaped.body.children[0]!;
    dialog.dispatch("cancel", {});
    expect(await escaped.answer).toBeNull();
    escaped.done();
  });

  it("will not place what the signing would refuse, and says why", async () => {
    const ui = open({
      preview: async () => {
        throw new Error("the reason, 审核, has characters tpdf cannot draw");
      },
    });
    await settle();
    expect(ui.status()).toBe("the reason, 审核, has characters tpdf cannot draw");
    expect(ui.button("Place on page…").disabled).toBe(true);
    ui.button("Place on page…").dispatch("click", {});
    await settle();
    let settled = false;
    void ui.answer.then(() => (settled = true));
    await settle();
    expect(settled).toBe(false);
    ui.button("Cancel").dispatch("click", {});
    expect(await ui.answer).toBeNull();
    ui.done();
  });
});
