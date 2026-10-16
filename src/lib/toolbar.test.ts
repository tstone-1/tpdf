import { describe, expect, it } from "vitest";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { HEADER_COMMANDS, POPUP_MARGIN, TOOL_ACTIONS, TOOL_GROUPS, WINDOW_MENU, popupShift, styleOptions, toolbarState, type ArmedTool } from "./toolbar";
import { BINDINGS } from "./keys";
import app from "../App.svelte?raw";
import { missingFrom, withoutComments } from "./sourcetext";
import { MARK_MENU, PAGE_MENU, REDACTION_MENU, SELECTION_MENU } from "./contextmenu";
import { cssColor, MARK_COLORS, PALETTE, swatchBackground } from "./markcolors";
import { MENU_LAYOUT, SEPARATOR } from "./menubar";

/**
 * Every button that is live, by command id.
 *
 * The line round a new text field is a stored preference and not a fact about
 * a document, so one of its two commands is live in an empty window too.
 */
function live(snapshot: ReturnType<typeof toolbarState>): string[] {
  // The tool row's. The window menu's commands are about the application and
  // its settings, and most of them are live with nothing open.
  const elsewhere = new Set(WINDOW_MENU.flatMap((section) => section.items));
  return Object.keys(snapshot).filter((id) => snapshot[id]!.enabled && !elsewhere.has(id));
}

function harness() {
  const state = { open: false, selected: false, dirty: false, undo: false, redo: false, matches: 0, border: false };
  // Only guards are called here. Missing action methods must fail if the
  // snapshot starts executing commands instead of describing them.
  const actions = {
    // Not a viewer: the guards ask only whether there is one.
    viewer: () => state.open ? ({ spread: { columns: 1, coverAlone: false, rightToLeft: false, continuous: true } } as never) : null,
    hasSelection: () => state.selected,
    isDirty: () => state.dirty,
    canUndo: () => state.undo,
    canRedo: () => state.redo,
    matchCount: () => state.matches,
    fieldBorder: () => state.border,
    savedFields: () => 0,
    formEditing: () => false,
    canOrderTabs: () => false,
    canCloseTabs: () => false,
    canReopenClosedTab: () => false,
    hasOutline: () => false,
    isFavourite: () => false,
    canStepFavourite: () => false,
    fieldPicked: () => false,
    signableFields: () => 0,
    // What the window menu's guards read, in the state a first launch is in.
    commandLineToolOffered: () => false,
    explorerOffered: () => false,
    automaticUpdates: () => true,
    updateReady: () => false,
    updateAvailable: () => false,
    diskChangeMode: () => "ask" as const,
    restoreTabs: () => false,
    syncScrolling: () => false,
    tabLabels: () => ({ canGrow: true, canShrink: true, isDefault: true }),
    documentCount: () => 1,
    sides: () => ({ split: false, focused: "left" as const, others: [] }),
    recentDocuments: () => 0,
    tabsToReopen: () => 0,
  } satisfies Partial<AppActions> as unknown as AppActions;
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

  it("lists under Document commands of the File menu, in its order", () => {
    // On Windows this menu is the File menu. Same order, so that a reader who
    // knows one finds their way in the other, and so that where a new item
    // goes is decided once.
    const menu = MENU_LAYOUT.find((section) => section.title === "File")!.items;
    const group = TOOL_GROUPS.find((group) => group.id === "document")!;
    const places = group.items.map((item) => menu.indexOf(item.id));
    expect(places.length).toBeGreaterThan(8);
    expect(places.filter((place) => place < 0)).toEqual([]);
    expect(places).toEqual([...places].sort((a, b) => a - b));
    // The three that had no route but the palette on Windows, and no route
    // under the word a reader types for them anywhere.
    for (const id of ["file.compress", "file.protect", "file.recogniseText"]) {
      expect(group.items.map((item) => item.id)).toContain(id);
    }
  });

  it("draws a line in the Redact menu before what reviews and before what writes", () => {
    // Marking, reviewing and writing a file are three kinds, and the last of
    // them holds the one item here that overwrites the document.
    const group = TOOL_GROUPS.find((group) => group.id === "redact")!;
    expect(group.items.filter((item) => item.rule).map((item) => item.id))
      .toEqual(["view.showRedactions", "file.redactRasterCopy"]);
    const writes = group.items.findIndex((item) => item.id.startsWith("file."));
    expect(group.items.slice(writes).every((item) => item.id.startsWith("file."))).toBe(true);
  });

  it("words a label with the words of the command's title", () => {
    // A label may be shorter than the title, since its menu says the rest. It
    // may not say something else: "Move page earlier" stood over the command
    // the palette calls "Move page up", and five more like it.
    const { registry } = harness();
    const words = (text: string) => text.toLowerCase().split(/[^a-z]+/).filter(Boolean);
    const groups = TOOL_GROUPS.filter((group) => group.id !== "width");
    const items = [...TOOL_ACTIONS, ...groups.flatMap((group) => group.items)];
    expect(items.length).toBeGreaterThan(50);
    const foreign = items.flatMap((item) => {
      const title = words(registry.find(item.id)?.title ?? "");
      return words(item.label).filter((word) => !title.includes(word)).map((word) => `${item.label}: ${word}`);
    });
    expect(foreign).toEqual([]);
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
      ["region", false, false],
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

describe("where there is no menu bar", () => {
  /*
   * On Windows the window is all there is: no menu bar lists the commands, so
   * one that is on no button, in no menu here and under no key is reached by
   * typing its name into the palette and by nothing else. Measured on
   * 2026-10-13, that was 61 commands, "About tpdf" and "Save a smaller
   * copy..." among them. `menubar.test.ts` asks the same of the macOS menu bar.
   */
  const NOT_ON_SCREEN: { prefix: string; reason: string }[] = [
    { prefix: "edit.align", reason: "a button on the arrange bar, shown while marks are picked" },
    { prefix: "edit.distribute", reason: "a button on the arrange bar, shown while marks are picked" },
    { prefix: "edit.same", reason: "a button on the arrange bar, shown while marks are picked" },
    { prefix: "edit.centerOnPage", reason: "a button on the arrange bar, shown while a mark is picked" },
    { prefix: "edit.middleOnPage", reason: "a button on the arrange bar, shown while a mark is picked" },
    { prefix: "edit.editForeignMark", reason: "a button in the popup of the comment it edits" },
    { prefix: "edit.deleteComment", reason: "a button in the popup of the comment it deletes" },
    { prefix: "edit.replyToComment", reason: "a button in the popup of the comment it answers" },
    { prefix: "edit.insertPage.", reason: "Insert blank page is in the Pages menu; these choose a paper size for it" },
    { prefix: "edit.insertPages.range", reason: "the question Insert pages from file... asks, while a file waits" },
    { prefix: "file.recogniseTextLanguage.choice", reason: "the question Recognise text: language... asks" },
    { prefix: "file.saveAttachment.choice", reason: "the question Save an attachment... asks when there are several" },
    { prefix: "view.showOutline", reason: "a tab at the head of the sidebar, which has a button in the header" },
    { prefix: "view.showMarks", reason: "a tab at the head of the sidebar, which has a button in the header" },
    {
      prefix: "file.addCertificateFile",
      reason:
        "Linux only: the palette offers it, and the sentence a reader is shown when no certificate can sign names it",
    },
  ];

  it("gives every command a control, a key, or a written reason", () => {
    const { registry } = harness();
    const shown = new Set<string>([
      ...HEADER_COMMANDS,
      ...TOOL_ACTIONS.map((item) => item.id),
      ...TOOL_GROUPS.flatMap((group) => group.items.map((item) => item.id)),
      ...WINDOW_MENU.flatMap((section) => section.items),
      ...[PAGE_MENU, MARK_MENU, REDACTION_MENU, SELECTION_MENU].flat(),
      ...Object.keys(BINDINGS),
    ]);
    const all = registry.all().map((command) => command.id);
    expect(all.length).toBeGreaterThan(100);
    const nowhere = all
      .filter((id) => !shown.has(id))
      .filter((id) => !NOT_ON_SCREEN.some((rule) => id.startsWith(rule.prefix)));
    expect(nowhere).toEqual([]);
  });

  it("excuses only commands that exist and are not shown", () => {
    // A reason outlives its command, and then excuses whatever takes the id.
    const { registry } = harness();
    const all = registry.all().map((command) => command.id);
    const idle = NOT_ON_SCREEN.filter((rule) => !all.some((id) => id.startsWith(rule.prefix)));
    expect(idle).toEqual([]);
    const listed = WINDOW_MENU.flatMap((section) => section.items);
    expect(listed.filter((id) => NOT_ON_SCREEN.some((rule) => id.startsWith(rule.prefix)))).toEqual([]);
  });

  it("lists each command of the window menu once, and none the row's menus have", () => {
    const listed = WINDOW_MENU.flatMap((section) => section.items);
    expect(listed.length).toBeGreaterThan(20);
    expect(new Set(listed).size).toBe(listed.length);
    const row = new Set([...TOOL_ACTIONS, ...TOOL_GROUPS.flatMap((group) => group.items)].map((item) => item.id));
    expect(listed.filter((id) => row.has(id))).toEqual([]);
  });

  it("has a title and a state for each of them, which is all its buttons show", () => {
    const { registry } = harness();
    const snapshot = toolbarState(registry);
    const listed = WINDOW_MENU.flatMap((section) => section.items);
    expect(listed.filter((id) => !snapshot[id] || snapshot[id]!.title === "Unavailable")).toEqual([]);
    expect(snapshot["app.about"]).toEqual({ enabled: true, title: "About tpdf" });
  });
});

describe("the first snapshot of the window", () => {
  /*
   * `App.svelte` declares its state down six thousand lines, and a snapshot
   * runs every listed command's guard. Taken while the component is still
   * being built, it reads whatever a guard reads before that is declared, and
   * the window comes up empty. No test builds the component, so the two lines
   * that keep it from happening are held as text, with the comments taken out
   * (`sourcetext.ts`): a line replaced and left behind in a comment then reads
   * as missing.
   */
  const code = withoutComments(app);

  it("is not taken while the component is being built", () => {
    expect(code).not.toMatch(/\$state\(\s*toolbarState\(/);
    expect(missingFrom(app, ["let toolState = $state<ReturnType<typeof toolbarState>>({});"])).toEqual([]);
  });

  it("is taken once the component is mounted", () => {
    expect(missingFrom(app, ["onMount(() => refreshMenu());"])).toEqual([]);
    // The one place that takes it, so that there is one rule for when.
    expect(code.split("toolbarState(commands)").length - 1).toBe(1);
  });
});
