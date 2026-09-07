import { describe, expect, it } from "vitest";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { TOOL_ACTIONS, TOOL_GROUPS, toolbarState } from "./toolbar";

function harness() {
  const state = { open: false, selected: false, dirty: false, undo: false, redo: false, matches: 0 };
  // Only guards are called here. Missing action methods must fail if the
  // snapshot starts executing commands instead of describing them.
  const actions = {
    viewer: () => state.open ? {} : null,
    hasSelection: () => state.selected,
    isDirty: () => state.dirty,
    canUndo: () => state.undo,
    canRedo: () => state.redo,
    matchCount: () => state.matches,
  } as AppActions;
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  return { state, registry };
}

describe("toolbar command surface", () => {
  it("resolves every visible action against the real application registry", () => {
    const { registry } = harness();
    const snapshot = toolbarState(registry);
    expect(Object.keys(snapshot).length).toBeGreaterThan(40);
    for (const id of Object.keys(snapshot)) {
      expect(registry.find(id), id).toBeDefined();
      expect(snapshot[id]!.title, id).not.toBe("Unavailable");
    }
    const visible = [...TOOL_ACTIONS, ...TOOL_GROUPS.flatMap((group) => group.items)];
    expect(new Set(visible.map((item) => item.id)).size).toBe(visible.length);
  });

  it("updates selection, save, undo, redo and search guards in both directions", () => {
    const { state, registry } = harness();
    expect(Object.values(toolbarState(registry)).every((item) => !item.enabled)).toBe(true);
    state.open = true;
    let snapshot = toolbarState(registry);
    expect(snapshot["edit.addComment"]!.enabled).toBe(true);
    expect(snapshot["file.saveCopy"]!.enabled).toBe(true);
    const guarded = ["edit.highlightSelection", "edit.redactSelection", "file.save", "edit.undo", "edit.redo", "edit.redactMatches"];
    for (const id of guarded) expect(snapshot[id]!.enabled, id).toBe(false);
    Object.assign(state, { selected: true, dirty: true, undo: true, redo: true, matches: 2 });
    snapshot = toolbarState(registry);
    for (const id of guarded) expect(snapshot[id]!.enabled, id).toBe(true);
    Object.assign(state, { selected: false, dirty: false, undo: false, redo: false, matches: 0 });
    snapshot = toolbarState(registry);
    for (const id of guarded) expect(snapshot[id]!.enabled, id).toBe(false);
    state.open = false;
    expect(Object.values(toolbarState(registry)).every((item) => !item.enabled)).toBe(true);
  });

  it("uses current command titles and refuses commands absent from a registry", () => {
    const registry = new CommandRegistry();
    registry.register({ id: "file.save", title: "Save", keys: "Ctrl+S", run: () => {} });
    expect(toolbarState(registry)["file.save"]).toEqual({ enabled: true, title: "Save (Ctrl+S)" });
    registry.find("file.save")!.keys = "Cmd+S";
    expect(toolbarState(registry)["file.save"]!.title).toBe("Save (Cmd+S)");
    expect(toolbarState(registry)["edit.draw"]).toEqual({ enabled: false, title: "Unavailable" });
  });
});
