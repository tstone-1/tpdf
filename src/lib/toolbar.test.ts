import { describe, expect, it } from "vitest";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { POPUP_MARGIN, TOOL_ACTIONS, TOOL_GROUPS, popupShift, styleOptions, toolbarState, type ArmedTool } from "./toolbar";
import { cssColor, MARK_COLORS, PALETTE, swatchBackground } from "./markcolors";
import { MENU_LAYOUT, SEPARATOR } from "./menubar";

/**
 * Every button that is live, by command id.
 *
 * The line round a new text field is a stored preference and not a fact about
 * a document, so one of its two commands is live in an empty window too.
 */
function live(snapshot: ReturnType<typeof toolbarState>): string[] {
  return Object.keys(snapshot).filter((id) => snapshot[id]!.enabled);
}

function harness() {
  const state = { open: false, selected: false, dirty: false, undo: false, redo: false, matches: 0, border: false };
  // Only guards are called here. Missing action methods must fail if the
  // snapshot starts executing commands instead of describing them.
  const actions = {
    viewer: () => state.open ? {} : null,
    hasSelection: () => state.selected,
    isDirty: () => state.dirty,
    canUndo: () => state.undo,
    canRedo: () => state.redo,
    matchCount: () => state.matches,
    fieldBorder: () => state.border,
    savedFields: () => 0,
    formEditing: () => false,
    canOrderTabs: () => false,
    fieldPicked: () => false,
  } as AppActions;
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  return { state, registry };
}

describe("toolbar command surface", () => {
  it("offers a visible swatch for every color using the annotation palette", () => {
    const group = TOOL_GROUPS.find((group) => group.id === "color")!;
    expect(group.items.length).toBe(PALETTE.length);
    expect(new Set(TOOL_GROUPS.map((group) => group.id)).size).toBe(TOOL_GROUPS.length);
    for (const entry of PALETTE) {
      expect(group.items.find((item) => item.id === `edit.color.${entry.id}`)?.swatch).toBe(entry);
      if (entry.rgb !== null) expect(swatchBackground(entry)).toBe(cssColor(entry.rgb));
      else {
        expect(swatchBackground(entry)).toContain("linear-gradient");
        expect(swatchBackground(entry)).toContain(cssColor(MARK_COLORS.highlight));
        expect(swatchBackground(entry)).toContain(cssColor(MARK_COLORS.ink));
      }
    }
  });

  it("lists under Form what the Form menu lists, in its order", () => {
    const menu = MENU_LAYOUT.find((section) => section.title === "Form")!;
    const commands = menu.items.filter((entry) => entry !== SEPARATOR);
    expect(commands.length).toBe(13);
    const group = TOOL_GROUPS.find((group) => group.id === "form")!;
    expect(group.items.map((item) => item.id)).toEqual(commands);
  });

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
    expect(live(toolbarState(registry))).toEqual(["edit.fieldBorderOn"]);
    state.border = true;
    expect(live(toolbarState(registry))).toEqual(["edit.fieldBorderOff"]);
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
    expect(live(toolbarState(registry))).toEqual(["edit.fieldBorderOff"]);
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

describe("when the colour and width controls are offered", () => {
  const idle = { armed: null as ArmedTool, drawing: false, erasing: false };

  it("offers neither with nothing armed", () => {
    expect(styleOptions(idle)).toEqual({ color: false, width: false });
  });

  it("offers both while the pen is armed, which the viewer reports as drawing", () => {
    expect(styleOptions({ ...idle, drawing: true })).toEqual({ color: true, width: true });
  });

  it("offers each tool what it uses", () => {
    const cases: [ArmedTool, boolean, boolean][] = [
      ["note", true, false],
      ["textbox", true, false],
      ["square", true, true],
      ["ellipse", true, true],
      ["ink", true, true],
      ["stamp", false, false],
      ["signature", false, false],
      ["crop", false, false],
      ["redact", false, false],
      ["place", false, false],
    ];
    for (const [armed, color, width] of cases) {
      expect(styleOptions({ ...idle, armed }), String(armed)).toEqual({ color, width });
    }
  });

  it("offers neither while erasing, even if the pen is still reported", () => {
    expect(styleOptions({ ...idle, erasing: true, drawing: true })).toEqual({ color: false, width: false });
  });
});

describe("keeping an open menu inside the window", () => {
  it("leaves a menu that fits where the stylesheet put it", () => {
    expect(popupShift(200, 510, 1200)).toBe(0);
    // Touching the margin on either side is still inside.
    expect(popupShift(POPUP_MARGIN, 300, 1200)).toBe(0);
    expect(popupShift(900, 1200 - POPUP_MARGIN, 1200)).toBe(0);
  });

  it("moves a menu that starts left of the window to the left margin", () => {
    // The More menu at 1,000 px: aligned to a button at the start of a row.
    expect(popupShift(-235, 75, 1000)).toBe(POPUP_MARGIN + 235);
    expect(popupShift(0, 310, 1000)).toBe(POPUP_MARGIN);
  });

  it("moves a menu that ends right of the window back by the overhang", () => {
    // The Redact menu at the end of the tool row.
    expect(popupShift(1050, 1290, 1200)).toBe(1200 - POPUP_MARGIN - 1290);
    expect(popupShift(1050, 1193, 1200)).toBe(-1);
  });

  it("starts a menu wider than the window at the left margin, not the right", () => {
    expect(popupShift(100, 500, 300)).toBe(POPUP_MARGIN - 100);
  });
});
