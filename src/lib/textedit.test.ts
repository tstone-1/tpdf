import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { changedTextPages, replacementError, TextEditor, type TextChange, type TextRuns } from "./textedit";
import { NOTHING_OPEN, type EditState } from "./edits";
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
    node.select = () => {};
    node.querySelectorAll = () => {
      const walk = (root: FakeElement): FakeElement[] => root.children.flatMap((child) => [child, ...walk(child)]);
      return walk(node).filter((child) => child.tagName === "button" || child.tagName === "input");
    };
    return node as unknown as HTMLElement;
  }) as typeof document.createElement);
});
afterEach(() => { vi.restoreAllMocks(); dom.restore(); });

function mount(write: (change: TextChange) => Promise<EditState> = async (value) => ({ ...state, text_edits: [value] })) {
  const close = vi.fn();
  const editor = new TextEditor(dom.root as unknown as HTMLElement, 1, runs,
    () => ({ left: 40, top: 48, right: 120, bottom: 63, clip: "inset(0px)" }), write, close);
  editor.update(state);
  const root = dom.root.children[0]!;
  const hit = root.children[0]!;
  const form = root.children.find((node) => node.classList.contains("text-edit-popup"))!;
  const field = form.children[0]!.children[0]! as FakeElement & { value: string; disabled: boolean };
  hit.dispatch("click", {});
  return { editor, root, form, field, close, hit };
}

describe("existing text editing", () => {
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
    for (const value of ["x".repeat(4097), "a\nb", "\u03b1", "\u0000", "\u007f"]) expect(replacementError(value)).not.toBeNull();
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
    for (const value of ["€", "a\u0308", "日本語", "\x80", "\x9f"]) expect(replacementError(value)).not.toBeNull();
  });
  it("keeps invalid and refused drafts from passing the save drain", async () => {
    const write = vi.fn(async () => { throw new Error("replacement exceeds the original width"); });
    const { editor, field } = mount(write);
    field.value = "\u03b1"; editor.commit(); await expect(editor.settle()).rejects.toThrow("printable");
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
    form.dispatch("keydown", { key: "Enter", isComposing: true });
    expect(write).not.toHaveBeenCalled();
    form.dispatch("keydown", { key: "Enter", isComposing: false }); await editor.settle();
    expect(write).toHaveBeenCalledExactlyOnceWith(change);
    field.value = "ACME";
    form.children.find((node) => node.classList.contains("text-edit-apply"))!.dispatch("click", {});
    await editor.settle(); expect(write).toHaveBeenLastCalledWith({ ...change, replacement: "ACME" });
  });
  it("cancel discards a draft and its refusal without writing", async () => {
    const write = vi.fn(async () => state);
    const { editor, form, field } = mount(write);
    field.value = "\u03b1"; editor.commit();
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
