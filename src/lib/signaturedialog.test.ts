import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeElement, installFakeDom, settle, type FakeDom } from "./testdom";

// The three awaits in the dialog are the store and the image decoder, and each
// test decides when its answer lands. A promise the test holds open is the only
// way to put a cancel or a new stroke *between* a request and its reply.
const store = vi.hoisted(() => ({
  remember: vi.fn(),
  load: vi.fn(),
  decode: vi.fn(),
}));
vi.mock("./signaturestore", () => ({
  prepareSignatureStorage: () => Promise.resolve(),
  loadSignature: store.load,
  rememberSignature: store.remember,
  forgetSignature: () => Promise.resolve(),
}));
vi.mock("./signature", async (actual) => ({
  ...(await actual<typeof import("./signature")>()),
  decodeSignature: store.decode,
  // `ImageData` does not exist here, and what the dialog draws is not the question.
  signatureCanvas: (image: { width: number; height: number }) => {
    const canvas = document.createElement("canvas");
    canvas.width = image.width; canvas.height = image.height;
    return canvas;
  },
}));

const { SignatureDialog } = await import("./signaturedialog");

function deferred<T>(): { promise: Promise<T>; resolve(value: T): void; reject(error: unknown): void } {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

/** A 2D context that records what is drawn and reports one inked pixel. */
function context() {
  return {
    drawn: 0,
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {}, arc() {}, fill() {},
    putImageData() {},
    drawImage() { this.drawn++; },
    getImageData(_x: number, _y: number, width: number, height: number) {
      const data = new Uint8ClampedArray(width * height * 4);
      data.set([23, 36, 58, 255]);
      return { data, width, height };
    },
  };
}

type Node = FakeElement & Record<string, unknown>;
let dom: FakeDom;
let canvases: { node: Node; ctx: ReturnType<typeof context> }[];

beforeEach(() => {
  dom = installFakeDom();
  canvases = [];
  const create = document.createElement.bind(document);
  vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
    const node = create(tag) as unknown as Node;
    if (tag === "canvas") {
      const ctx = context();
      canvases.push({ node, ctx });
      Object.assign(node, { getContext: () => ctx });
    }
    if (tag === "dialog") {
      node.open = false;
      node.showModal = () => { node.open = true; };
      node.close = () => { node.open = false; };
    }
    if (tag === "input") { node.checked = false; node.click = () => {}; }
    // A label is handed its caption as a string, which the fake tree has no node for.
    if (tag === "label")
      Object.assign(node, {
        append: (...kids: unknown[]) => {
          for (const kid of kids) if (typeof kid !== "string") node.appendChild(kid as FakeElement);
        },
      });
    return node as unknown as HTMLElement;
  }) as typeof document.createElement);
});
afterEach(() => { vi.restoreAllMocks(); store.remember.mockReset(); store.load.mockReset(); store.decode.mockReset(); dom.restore(); });

function all(root: FakeElement): FakeElement[] {
  return root.children.flatMap((child) => [child, ...all(child)]);
}

function mount() {
  const dialog = new SignatureDialog(dom.root as unknown as HTMLElement);
  const nodes = () => all(dom.root) as Node[];
  const button = (text: string) => nodes().find((node) => node.tagName === "button" && node.textContent === text)!;
  const inputs = nodes().filter((node) => node.tagName === "input");
  const file = inputs.find((node) => node.type === "file")!;
  const remember = inputs.filter((node) => node.type === "checkbox")[1]!;
  const message = nodes().find((node) => node.tagName === "p" && node.attributes.get("role") === "status")!;
  const main = canvases[0]!;
  const draw = () => main.node.dispatch("pointerdown", { button: 0, clientX: 5, clientY: 5, pointerId: 1 });
  const answers: unknown[] = [];
  const ask = () => { void dialog.ask().then((image) => answers.push(image)); };
  return { dialog, button, file, remember, message, main, draw, answers, ask };
}

describe("signature dialog: an answer that arrives after the reader moved on", () => {
  it("places the signature once the remember finishes, when nothing changed meanwhile", async () => {
    const saving = deferred<void>();
    store.remember.mockReturnValue(saving.promise);
    const ui = mount();
    ui.ask(); ui.draw(); ui.remember.checked = true;
    ui.button("Place signature image").dispatch("click", {});
    await settle();
    expect(ui.answers).toEqual([]);
    saving.resolve();
    await settle();
    expect(ui.answers).toHaveLength(1);
    expect(ui.answers[0]).toMatchObject({ width: 1, height: 1 });
  });

  it("does not hand a late-remembered signature to the next time the dialog is asked", async () => {
    const saving = deferred<void>();
    store.remember.mockReturnValue(saving.promise);
    const ui = mount();
    ui.ask(); ui.draw(); ui.remember.checked = true;
    ui.button("Place signature image").dispatch("click", {});
    await settle();
    ui.button("Cancel").dispatch("click", {});
    await settle();
    expect(ui.answers).toEqual([null]);
    // Asked again before the save lands: this is a different placement.
    ui.ask();
    saving.resolve();
    await settle();
    expect(ui.answers).toEqual([null]);
    expect(ui.message.textContent).toMatch(/saved on this device but not placed/);
  });

  it("says the earlier signature was kept when the reader drew again while it was being saved", async () => {
    const saving = deferred<void>();
    store.remember.mockReturnValue(saving.promise);
    const ui = mount();
    ui.ask(); ui.draw(); ui.remember.checked = true;
    ui.button("Place signature image").dispatch("click", {});
    await settle();
    ui.draw();
    saving.resolve();
    await settle();
    expect(ui.answers).toEqual([]);
    expect(ui.message.textContent).toMatch(/saved on this device but not placed/);
  });

  it("ignores a saved signature that arrives after a new stroke", async () => {
    const loading = deferred<null>();
    store.load.mockReturnValue(loading.promise);
    const ui = mount();
    ui.ask();
    ui.button("Use saved signature").dispatch("click", {});
    ui.draw();
    loading.resolve(null);
    await settle();
    expect(ui.message.textContent).toBe("");
  });

  it("reports an empty store when the load is still the current request", async () => {
    store.load.mockResolvedValue(null);
    const ui = mount();
    ui.ask();
    ui.button("Use saved signature").dispatch("click", {});
    await settle();
    expect(ui.message.textContent).toBe("No signature has been saved on this device.");
  });

  it("does not report a failed import that a new stroke replaced", async () => {
    const decoding = deferred<never>();
    store.decode.mockReturnValue(decoding.promise);
    const ui = mount();
    ui.ask();
    ui.file.files = [{ type: "image/png", size: 10 }];
    ui.file.dispatch("change", {});
    ui.draw();
    decoding.reject(new Error("This image could not be decoded."));
    await settle();
    expect(ui.message.textContent).toBe("");
  });

  it("reports a failed import that is still the current request", async () => {
    store.decode.mockRejectedValue(new Error("This image could not be decoded."));
    const ui = mount();
    ui.ask();
    ui.file.files = [{ type: "image/png", size: 10 }];
    ui.file.dispatch("change", {});
    await settle();
    expect(ui.message.textContent).toBe("This image could not be decoded.");
  });

  it("does not draw an imported image over a stroke made while it was decoding", async () => {
    const decoding = deferred<{ width: number; height: number; close(): void }>();
    store.decode.mockReturnValue(decoding.promise);
    const ui = mount();
    ui.ask();
    ui.file.files = [{ type: "image/png", size: 10 }];
    ui.file.dispatch("change", {});
    ui.draw();
    const close = vi.fn();
    decoding.resolve({ width: 4, height: 4, close });
    await settle();
    expect(ui.main.ctx.drawn).toBe(0);
    expect(close).toHaveBeenCalledOnce();
  });

  it("draws an imported image that is still the current request", async () => {
    store.decode.mockResolvedValue({ width: 4, height: 4, close: () => {} });
    const ui = mount();
    ui.ask();
    ui.file.files = [{ type: "image/png", size: 10 }];
    ui.file.dispatch("change", {});
    await settle();
    expect(ui.main.ctx.drawn).toBe(1);
  });
});
