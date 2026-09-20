import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { changedTextPages, replacementError, TextEditor, type TextChange, type TextRuns } from "./textedit";
import { NOTHING_OPEN, type EditState } from "./edits";
import { defaultTextLayout } from "./textlayout";
import { pageId } from "./pages";
import { FakeElement, installFakeDom, type FakeDom } from "./testdom";

const change: TextChange = { page: 0, revision: [1], operator: 3, original: "ACME original", replacement: "ACME edit" };
const runs: TextRuns = { page: 0, revision: [1], runs: [{ operator: 3, text: change.original, font: "F1", size: 12, advance: 80, matrix: [1,0,0,1,40,180], display_rect: [40,48,120,63] }] };
const state: EditState = { ...NOTHING_OPEN, pages: [{ id: pageId(1), source: { baseline: 0 }, turns: 0 }] };
let dom: FakeDom;
beforeEach(() => {
  dom = installFakeDom();
  const create = document.createElement.bind(document);
  vi.spyOn(document, "createElement").mockImplementation(((tag: string) => {
    const node = create(tag) as unknown as FakeElement & { select(): void; querySelectorAll(): FakeElement[] };
    node.clientWidth = 800; node.clientHeight = 600;
    node.select = () => {};
    node.querySelectorAll = () => {
      const walk = (root: FakeElement): FakeElement[] => root.children.flatMap((child) => [child, ...walk(child)]);
      return walk(node).filter((child) => ["button", "input", "textarea", "select"].includes(child.tagName));
    };
    return node as unknown as HTMLElement;
  }) as typeof document.createElement);
});
afterEach(() => { vi.restoreAllMocks(); dom.restore(); });

function mount(write: (change: TextChange) => Promise<EditState> = async (value) => ({ ...state, text_edits: [value] }), source = runs, anchor: ConstructorParameters<typeof TextEditor>[3] = () => ({ left: 40, top: 48, right: 120, bottom: 63, clip: "inset(0px)" }), preview?: ConstructorParameters<typeof TextEditor>[6]) {
  const close = vi.fn();
  const editor = new TextEditor(dom.root as unknown as HTMLElement, 1, source,
    anchor, write, close, preview);
  editor.update(state);
  const root = dom.root.children[0]!;
  const hit = root.children[0]!;
  const form = root.children.find((node) => node.classList.contains("text-edit-popup"))!;
  const field = form.children[0]!.children[0]! as FakeElement & { value: string; disabled: boolean };
  hit.dispatch("click", {});
  return { editor, root, form, field, close, hit };
}

describe("existing text editing", () => {
  it("keeps the validated descender height when sizing and rounding the default box", () => {
    for (const matrix of [[1, 0, 0, 1, 0, 0], [0, 1, -1, 0, 0, 0], [-1, 0, 0, -1, 0, 0]] as const) {
      const run = { ...runs.runs[0]!, size: 12.0001, minimum_height: 18.00015, matrix: [...matrix] as [number, number, number, number, number, number] };
      const box = defaultTextLayout(run);
      expect(box.size).toBe(12.001);
      expect(box.height).toBe(18.002);
      expect(box.height).toBeGreaterThanOrEqual(box.size * 1.5);
    }
  });
  // The box the editor opens carries `grow`, which is what lets the worker size
  // it to the typed text up to the room after the line. The flag is not about
  // the layout as a whole: it is about the width, and only the width control
  // clears it -- a reader who picks a font or ticks wrapping has still not said
  // how wide they want the box.
  it("opens the box with room to grow and gives it up only when a width is typed", async () => {
    const box = defaultTextLayout(runs.runs[0]!);
    expect(box).toMatchObject({ width: 80, grow: true });
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, root, field } = mount(write);
    const control = (name: string) => (root as FakeElement & { querySelectorAll(): FakeElement[] }).querySelectorAll().find((node) => node.getAttribute("aria-label") === name)! as FakeElement & { value: string; checked: boolean };
    field.value = "ACME edited"; field.dispatch("input", {});
    control("Font").value = "noto_sans_bold"; control("Font").dispatch("change", {});
    control("Wrap within box").checked = true; control("Wrap within box").dispatch("change", {});
    control("Height (pt)").value = "40"; control("Height (pt)").dispatch("input", {});
    editor.commit(); await editor.settle();
    expect(write.mock.calls[0]![0].layout).toMatchObject({ grow: true, height: 40, wrap: true, font: "noto_sans_bold" });
    control("Width (pt)").value = "150"; control("Width (pt)").dispatch("input", {});
    editor.commit(); await editor.settle();
    expect(write.mock.calls[1]![0].layout).toMatchObject({ grow: false, width: 150 });
    editor.destroy();
  });
  // Reopening a run takes the answer from the layout the journal kept, rather
  // than assuming one: a box a reader sized once stays sized.
  it("reopens a sized box as sized and an untouched one with room to grow", async () => {
    const sized = { ...defaultTextLayout(runs.runs[0]!), width: 150, grow: false };
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    // The application always supplies a preview callback, and that is what
    // makes every change carry a layout; without one the flag has no carrier.
    const { editor, field } = mount(write, runs, undefined, vi.fn(async () => runs));
    editor.update({ ...state, text_edits: [{ ...change, replacement: "ACME edit", layout: sized }] });
    field.value = "ACME edited"; field.dispatch("input", {});
    editor.commit(); await editor.settle();
    expect(write.mock.calls[0]![0].layout).toMatchObject({ grow: false, width: 150 });
    editor.update({ ...state, text_edits: [] });
    field.value = "ACME again"; field.dispatch("input", {});
    editor.commit(); await editor.settle();
    expect(write.mock.calls[1]![0].layout).toMatchObject({ grow: true, width: 80 });
    editor.destroy();
  });
  it("offers CJK fonts and sends the selected style to the writer", async () => {
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, root, field } = mount(write);
    const font = (root as FakeElement & { querySelectorAll(): FakeElement[] }).querySelectorAll()
      .find((node) => node.getAttribute("aria-label") === "Font")! as FakeElement & { value: string };
    expect(font.children.map((node) => (node as FakeElement & { value: string }).value))
      .toEqual(expect.arrayContaining(["noto_sans_cjk_sc", "noto_sans_cjk_sc_bold"]));
    field.value = "\u65b0\u5b57";
    font.value = "noto_sans_cjk_sc_bold"; font.dispatch("change", {});
    editor.commit(); await editor.settle();
    expect(write).toHaveBeenLastCalledWith(expect.objectContaining({ replacement: "\u65b0\u5b57",
      layout: expect.objectContaining({ font: "noto_sans_cjk_sc_bold" }) }));
    editor.destroy();
  });
  it("applies layout-only changes and wraps on Ctrl+Enter while Enter adds a line", async () => {
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, root, form, field } = mount(write);
    const control = (name: string) => (root as FakeElement & { querySelectorAll(): FakeElement[] }).querySelectorAll().find((node) => node.getAttribute("aria-label") === name)! as FakeElement & { value: string; checked: boolean };
    control("Width (pt)").value = "150";
    control("Width (pt)").dispatch("input", {});
    control("Font").value = "noto_sans_bold";
    control("Font").dispatch("change", {});
    editor.commit(); await editor.settle();
    expect(write).toHaveBeenLastCalledWith({ ...change, replacement: change.original,
      layout: { width: 150, height: 15, size: 12, wrap: false, font: "noto_sans_bold", grow: false } });
    control("Wrap within box").checked = true; control("Wrap within box").dispatch("change", {});
    field.value = "ACME\nSECOND";
    form.dispatch("keydown", { target: field, key: "Enter" });
    expect(write).toHaveBeenCalledTimes(1);
    form.dispatch("keydown", { target: field, key: "Enter", ctrlKey: true }); await editor.settle();
    expect(write.mock.calls[1]![0]).toMatchObject({ replacement: "ACME\nSECOND", layout: { wrap: true } });
    control("Width (pt)").value = "0"; editor.commit();
    await expect(editor.settle()).rejects.toThrow("box");
    expect(write).toHaveBeenCalledTimes(2);
    editor.destroy();
  });
  it("debounces previews, discards stale replies and never writes a cancelled preview", async () => {
    vi.useFakeTimers();
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:preview");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const finish: Array<(runs: TextRuns) => void> = [];
    const preview = vi.fn(() => new Promise<TextRuns>((resolve) => finish.push(resolve)));
    const write = vi.fn(async () => state);
    const { editor, field, form } = mount(write, runs, undefined, preview);
    const reply: TextRuns = { ...runs, preview: { png: [1,2,3], font: "Noto Sans", lines: 2, rect: [40,48,180,100] } };
    field.value = "FIRST"; field.dispatch("input", {}); await vi.advanceTimersByTimeAsync(250);
    field.value = "SECOND"; field.dispatch("input", {}); await vi.advanceTimersByTimeAsync(250);
    expect(preview).toHaveBeenCalledTimes(2);
    expect(preview.mock.calls[0]).toEqual([{ ...change, replacement: "FIRST", layout: { width: 80, height: 15, size: 12, wrap: false, font: "auto", grow: true } }]);
    finish[0]!(reply); await Promise.resolve(); expect(create).not.toHaveBeenCalled();
    finish[1]!(reply); await Promise.resolve(); expect(create).toHaveBeenCalledOnce();
    form.dispatch("keydown", { key: "Escape" }); expect(revoke).toHaveBeenCalledWith("blob:preview");
    expect(write).not.toHaveBeenCalled();
    editor.destroy(); vi.useRealTimers();
  });
  it("Done waits for the draft before closing and returning focus to the viewer", async () => {
    for (const outcome of ["accepted", "refused", "disposed"] as const) {
      let finish!: (value: EditState) => void;
      let refuse!: (error: Error) => void;
      const write = vi.fn(() => new Promise<EditState>((resolve, reject) => { finish = resolve; refuse = reject; }));
      const { editor, root, field, close } = mount(write);
      const done = root.children.flatMap((node) => node.children).find((node) => node.getAttribute("aria-label") === "Done")!;
      field.value = "ACME edit";
      const focus = vi.spyOn(FakeElement.prototype, "focus");
      close.mockImplementation(() => editor.destroy());
      done.dispatch("click", {});
      expect(write).toHaveBeenCalledExactlyOnceWith(change);
      expect(close).not.toHaveBeenCalled();
      expect(focus).not.toHaveBeenCalled();
      if (outcome === "disposed") editor.destroy();
      if (outcome === "refused") refuse(new Error("replacement refused"));
      else finish({ ...state, text_edits: [change] });
      if (outcome === "refused") await expect(editor.settle()).rejects.toThrow("replacement refused");
      else await editor.settle();
      if (outcome === "accepted") {
        expect(close).toHaveBeenCalledOnce();
        expect(dom.root.children).toHaveLength(0);
        expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
        expect(focus.mock.contexts).toEqual([dom.root]);
        expect(close.mock.invocationCallOrder[0]).toBeLessThan(focus.mock.invocationCallOrder[0]!);
      } else {
        expect(close).not.toHaveBeenCalled();
        expect(focus).not.toHaveBeenCalled();
        if (outcome === "refused") expect(dom.root.children).toEqual([root]);
      }
      editor.destroy(); focus.mockRestore();
    }
  });
  it("returns focus to the selected text target when cancelling without scrolling", async () => {
    for (const escape of [true, false]) {
      const focus = vi.spyOn(FakeElement.prototype, "focus");
      const write = vi.fn(async () => state);
      const second = { ...runs.runs[0]!, operator: 9, text: "SECOND" };
      const { editor, root, form, field } = mount(write, { ...runs, runs: [...runs.runs, second] });
      const target = root.children[1]!;
      target.dispatch("click", {});
      field.value = "discard this draft";
      focus.mockClear();
      if (escape) form.dispatch("keydown", { key: "Escape" });
      else form.children.find((node) => node.getAttribute("aria-label") === "Cancel")!.dispatch("click", {});
      await editor.settle();
      expect(form).toHaveProperty("hidden", true);
      expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
      expect(focus.mock.contexts).toEqual([target]);
      expect(write).not.toHaveBeenCalled();
      target.dispatch("click", {});
      expect(field.value).toBe("SECOND");
      editor.destroy(); focus.mockRestore();
    }
  });
  it("returns focus to Done when the selected text has scrolled out of view", async () => {
    for (const escape of [true, false]) {
      let offset = 0;
      const write = vi.fn(async () => state);
      const { editor, root, form, field, hit } = mount(write, runs, () => ({ left: 40, top: 48 + offset, right: 120, bottom: 63 + offset, clip: "inset(0px)" }));
      const done = root.children.flatMap((node) => node.children).find((node) => node.getAttribute("aria-label") === "Done")!;
      field.value = "discard this draft";
      offset = 700; editor.layout();
      expect(hit).toHaveProperty("hidden", true);
      const focus = vi.spyOn(FakeElement.prototype, "focus");
      if (escape) form.dispatch("keydown", { key: "Escape" });
      else form.children.find((node) => node.getAttribute("aria-label") === "Cancel")!.dispatch("click", {});
      await editor.settle();
      expect(form).toHaveProperty("hidden", true);
      expect(focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
      expect(focus.mock.contexts).toEqual([done]);
      expect(write).not.toHaveBeenCalled();
      offset = 0; editor.layout(); hit.dispatch("click", {});
      expect(field.value).toBe(change.original);
      editor.destroy(); focus.mockRestore();
    }
  });
  it("removes fully offscreen targets from keyboard navigation and restores them on return", () => {
    let box: ReturnType<ConstructorParameters<typeof TextEditor>[3]> = null;
    const { editor, hit } = mount(undefined, runs, () => box);
    expect(hit).toHaveProperty("hidden", true);
    for (const [left, top, right, bottom, hidden] of [
      [-20, 48, 0, 63, true], [800, 48, 820, 63, true],
      [40, -20, 120, 0, true], [40, 600, 120, 620, true],
      [-20, 48, 1, 63, false], [799, 48, 820, 63, false],
      [40, -20, 120, 1, false], [40, 599, 120, 620, false],
      [40, 48, 120, 63, false],
    ] as const) {
      box = { left, top, right, bottom, clip: "inset(0px)" };
      editor.layout();
      expect(hit).toHaveProperty("hidden", hidden);
    }
    editor.destroy();
  });
  it("starts keyboard navigation at the first visible target or Done", () => {
    for (const visible of [true, false]) {
      const focus = vi.spyOn(FakeElement.prototype, "focus");
      const second = { ...runs.runs[0]!, operator: 9, text: "SECOND" };
      const { editor, root } = mount(undefined, { ...runs, runs: [...runs.runs, second] }, (run) => ({
        left: 40, right: 120, top: visible && run.operator === 9 ? 48 : 700,
        bottom: visible && run.operator === 9 ? 63 : 720, clip: "inset(0px)",
      }));
      const done = root.children.flatMap((node) => node.children).find((node) => node.getAttribute("aria-label") === "Done")!;
      expect(focus.mock.contexts[0]).toBe(visible ? root.children[1] : done);
      expect(focus.mock.calls[0]).toEqual([{ preventScroll: true }]);
      editor.destroy(); focus.mockRestore();
    }
  });
  it("focuses targets and the input without scrolling their overlay", () => {
    const focus = vi.spyOn(FakeElement.prototype, "focus");
    mount();
    expect(focus.mock.calls.length).toBeGreaterThanOrEqual(2);
    for (const args of focus.mock.calls) expect(args).toEqual([{ preventScroll: true }]);
  });
  it("invalidates added, changed and undone pages without invalidating reordered changes", () => {
    expect(changedTextPages([], [change])).toEqual([0]);
    expect(changedTextPages([change], [])).toEqual([0]);
    expect(changedTextPages([change], [{ ...change, replacement: "ACME" }])).toEqual([0]);
    const second = { ...change, page: 2 };
    expect(changedTextPages([change, second], [second, change])).toEqual([]);
  });
  it("bounds draft characters while allowing deletion", () => {
    expect(replacementError("")).toBeNull();
    expect(replacementError("x".repeat(4096))).toBeNull();
    for (const value of ["x".repeat(4097), "a\nb", "\ud800", "\u0000", "\u007f"]) expect(replacementError(value)).not.toBeNull();
    for (const value of ["\u03b1", "€", "a\u0308", "日本語", "\u{20000}".repeat(4096)]) expect(replacementError(value)).toBeNull();
  });
  it("starts a drain immediately and waits for the backend before accepting it", async () => {
    let finish!: (value: EditState) => void;
    const write = vi.fn(() => new Promise<EditState>((resolve) => { finish = resolve; }));
    const { editor, field } = mount(write);
    field.value = "ACME edit"; editor.commit();
    expect(write).toHaveBeenCalledExactlyOnceWith(change);
    expect(field.disabled).toBe(true);
    let settled = false; const waiting = editor.settle().then(() => { settled = true; });
    await Promise.resolve(); expect(settled).toBe(false);
    finish({ ...state, text_edits: [change] }); await waiting;
    expect(field.disabled).toBe(false);
    editor.commit(); expect(write).toHaveBeenCalledTimes(1);
  });
  it("sends accented replacements unchanged through the save drain", async () => {
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, field } = mount(write);
    field.value = "GEPRÜFT ß"; editor.commit(); await editor.settle();
    expect(write).toHaveBeenCalledExactlyOnceWith({ ...change, replacement: "GEPRÜFT ß" });
    expect(replacementError("ä".repeat(4096))).toBeNull();
    expect(replacementError("ä".repeat(4097))).not.toBeNull();
    for (let code = 160; code <= 255; code++) expect(replacementError(String.fromCharCode(code))).toBeNull();
    for (const value of ["\x80", "\x9f"]) expect(replacementError(value)).not.toBeNull();
  });
  it("sends punctuation unchanged and refuses controls", async () => {
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, field } = mount(write);
    field.value = "A\u2013\u2018\u2019\u2212B"; editor.commit(); await editor.settle();
    expect(write).toHaveBeenCalledExactlyOnceWith({ ...change, replacement: "A\u2013\u2018\u2019\u2212B" });
    expect(replacementError("\u2013".repeat(4096))).toBeNull();
    for (const value of ["\u2013".repeat(4097), "\u0096", "\u2028", "\u2029", "\u0091", "\u0080"])
      expect(replacementError(value)).not.toBeNull();
  });
  it("keeps invalid and refused drafts from passing the save drain", async () => {
    const write = vi.fn(async () => { throw new Error("replacement exceeds the original width"); });
    const { editor, field } = mount(write);
    field.value = "a\nb"; editor.commit(); await expect(editor.settle()).rejects.toThrow("control");
    expect(write).not.toHaveBeenCalled();
    field.value = "ACME"; field.dispatch("input", {}); editor.commit();
    await expect(editor.settle()).rejects.toThrow("original width");
    await expect(editor.settle()).rejects.toThrow("original width");
    expect(field.value).toBe("ACME");
  });
  it("Enter and Apply commit without a navigable form; composition does not commit", async () => {
    const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
    const { editor, form, field } = mount(write);
    expect(form.tagName).toBe("div");
    field.value = "ACME edit";
    form.dispatch("keydown", { target: field, key: "Enter", isComposing: true });
    expect(write).not.toHaveBeenCalled();
    form.dispatch("keydown", { target: field, key: "Enter", isComposing: false }); await editor.settle();
    expect(write).toHaveBeenCalledExactlyOnceWith(change);
    field.value = "ACME";
    form.children.find((node) => node.classList.contains("text-edit-apply"))!.dispatch("click", {});
    await editor.settle(); expect(write).toHaveBeenLastCalledWith({ ...change, replacement: "ACME" });
  });
  it("Enter on popup buttons leaves their native activation in charge", async () => {
    for (const action of ["Cancel", "Apply"]) {
      const write = vi.fn(async (value: TextChange) => ({ ...state, text_edits: [value] }));
      const { editor, form, field, hit } = mount(write);
      field.value = "ACME edit";
      const button = form.children.find((node) => node.textContent === action)!;
      const preventDefault = vi.fn();
      const stopPropagation = vi.fn();
      // The fake DOM does not bubble or perform browser default actions.
      form.dispatch("keydown", { target: button, key: "Enter", preventDefault, stopPropagation });
      await editor.settle();
      expect(write).not.toHaveBeenCalled();
      expect(preventDefault).not.toHaveBeenCalled();
      expect(stopPropagation).toHaveBeenCalledOnce();
      button.dispatch("click", {});
      await editor.settle();
      if (action === "Cancel") {
        expect(write).not.toHaveBeenCalled();
        expect(form).toHaveProperty("hidden", true);
        hit.dispatch("click", {});
        expect(field.value).toBe(change.original);
      } else {
        expect(write).toHaveBeenCalledExactlyOnceWith(change);
        expect(form).toHaveProperty("hidden", false);
      }
      editor.destroy();
    }
  });
  it("cancel discards a draft and its refusal without writing", async () => {
    const write = vi.fn(async () => state);
    const { editor, form, field } = mount(write);
    field.value = "a\nb"; editor.commit();
    form.dispatch("keydown", { key: "Escape" });
    await expect(editor.settle()).resolves.toBeUndefined();
    editor.commit(); expect(write).not.toHaveBeenCalled();
  });
  it("undo refreshes a clean input and restoration keeps the original source address", async () => {
    const write = vi.fn(async () => state);
    const { editor, field, hit } = mount(write);
    editor.update({ ...state, text_edits: [change] });
    expect(field.value).toBe("ACME edit");
    expect(hit.getAttribute("aria-label")).toBe("Edit: ACME edit");
    editor.update(state); expect(field.value).toBe("ACME original");
    editor.update({ ...state, text_edits: [change] });
    field.value = "ACME original"; editor.commit(); await editor.settle();
    expect(write).toHaveBeenCalledExactlyOnceWith({ ...change, replacement: change.original });
  });
  it("ignores late completion after destruction and closes on page removal", async () => {
    let finish!: (value: EditState) => void;
    const { editor, field, close } = mount(() => new Promise((resolve) => { finish = resolve; }));
    field.value = "ACME"; editor.commit(); editor.destroy();
    finish({ ...state, text_edits: [change] }); await editor.settle();
    expect(dom.root.children).toHaveLength(0);
    const next = mount(); next.editor.update(NOTHING_OPEN); expect(next.close).toHaveBeenCalledOnce();
    expect(close).not.toHaveBeenCalled();
  });
});
