/**
 * Tests for the commands the application registers, as opposed to the registry.
 *
 * `commands.test.ts` pins what the registry *does* with a command --- ranking,
 * recents, argument validation. This pins the registrations themselves: that a
 * command exists, reaches the action it names, and is withheld when it cannot
 * work. Those are three different ways to be wrong and none of them is visible
 * from the registry's own tests.
 *
 * `viewercheck.ts` exercises the same commands through a real palette in a real
 * window, which is the right tool for "does typing `fw` and pressing Enter fit
 * the width". It is scheduled by hand and needs an unlocked screen, so it is the
 * wrong tool for "is this command wired to that function" --- which is what is
 * here, and runs on every commit.
 */

import { ARRANGEMENTS } from "./arrange";
import { describe, expect, it } from "vitest";

import {
  handleWindowKey,
  togglePalette,
  registerAppCommands,
  type AppActions,
} from "./appcommands";
import { CommandRegistry } from "./commands";
import { DISK_CHANGE_MODES, type DiskChangeMode } from "./diskwatch";
import { PALETTE } from "./markcolors";
import { NIBS } from "./marknibs";
import type { StampName } from "./pages";
import { PAGE_SIZE_NAMES } from "./pagesizes";
import type { PreparedImport } from "./pendingimport";
import { updateLabel } from "./update";

/**
 * A registry with every application command in it, and a record of what fired.
 *
 * `viewer` decides whether a document is open, which is what the `enabled`
 * guards read --- so the same builder serves both directions and the
 * with-document case cannot quietly become the only one tested.
 */
function harness(
  hasDocument = true,
  update: {
    available?: boolean; ready?: boolean; automatic?: boolean; disk?: DiskChangeMode;
    restoreTabs?: boolean; reopenable?: number; recents?: number; fieldBorder?: boolean; picked?: number; fieldPicked?: boolean; formEditing?: boolean; savedFields?: number; canOrderTabs?: boolean; signable?: number;
  } = {},
  journal: { undo?: boolean; redo?: boolean } = {},
  selected = false,
  markOpen = false,
  dirty = false,
  history: { back?: boolean; forward?: boolean } = {},
  matched = false,
  // Default false for `markOpen`'s reason: a document opens with no comment
  // popup on show, so the withheld direction is what a test that says nothing
  // about comments exercises.
  commentEditable = false,
  // **Its own flag rather than sharing `commentEditable`.** The two conditions
  // agree today --- both ask for an open popup whose comment has an object ---
  // and one flag driving both would make a mutation that swaps the two commands'
  // guards survive, because every harness reading would move together.
  commentReplyable = false,
  // A third flag for the second one's reason exactly, and the argument gets
  // stronger with each: three guards that agree today, and one flag driving all
  // three would make a mutation that swaps any two of them survive.
  commentDeletable = false,
  // A fourth flag, for the third's reason: a picked region and an open note are
  // different states of different subsystems, and one flag driving both would
  // let a mutation that swapped `edit.removeMark`'s guard for this one survive.
  // Default false because a document opens with nothing picked.
  redactionPicked = false,
  busyDocument = false,
  // Null by default, which is the state every document is in unless the
  // reader has just chosen a file to insert from: the range question is then
  // withheld, and a test that says nothing about an import exercises that.
  waiting: PreparedImport | null = null,
) {
  const fired: string[] = [];
  let automatic = update.automatic ?? true;
  let restoring = update.restoreTabs ?? false;
  let framing = update.fieldBorder ?? true;
  let formEditing = update.formEditing ?? false;
  let diskMode: DiskChangeMode = update.disk ?? "ask";
  const actions: AppActions = {
    editText: () => { fired.push("editText"); }, signature: () => { fired.push("signature"); },
    fillForm: () => { fired.push("fillForm"); },
    // Not a real viewer: the guards ask whether it is null and, since
    // 2026-08-23, whether the history has anywhere to go. A cast is honest
    // about the rest; a stub with a dozen methods would suggest they are
    // exercised.
    //
    // **Both default to `false`**, which is the state a document actually opens
    // in --- nobody has jumped yet --- rather than the convenient one. That is
    // the same argument the update flags below make, and it is what makes the
    // greyed direction the one every other test in this file exercises.
    viewer: () =>
      hasDocument
        ? ({
            canGoBack: history.back ?? false,
            canGoForward: history.forward ?? false,
          } as never)
        : null,
    pageCount: () => 3,
    openDocument: () => fired.push("openDocument"),
    closeDocument: () => fired.push("closeDocument"),
    closeAllDocuments: () => fired.push("closeAllDocuments"),
    restoreTabs: () => restoring,
    setRestoreTabs: (restore) => { restoring = restore; fired.push(`setRestoreTabs:${restore}`); },
    tabsToReopen: () => update.reopenable ?? 0,
    reopenLastTabs: () => fired.push("reopenLastTabs"),
    recentDocuments: () => update.recents ?? 0,
    clearRecentDocuments: () => fired.push("clearRecentDocuments"),
    tabLabels: () => ({ canGrow: true, canShrink: true, isDefault: false }),
    resizeTabLabels: (direction: -1 | 0 | 1) => fired.push(`resizeTabLabels:${direction}`),
    nextDocument: (delta) => fired.push(`nextDocument:${delta}`),
    documentCount: () => 2,
    reloadDocument: () => fired.push("reloadDocument"),
    diskChangeMode: () => diskMode,
    setDiskChangeMode: (mode) => { diskMode = mode; fired.push(`setDiskChangeMode:${mode}`); },
    busyOpening: () => false,
    busyDocument: () => busyDocument,
    printDocument: () => fired.push("printDocument"),
    focusFind: () => fired.push("focusFind"),
    toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
    toggleSearchScope: () => fired.push("toggleSearchScope"),
    toggleSidebar: () => fired.push("toggleSidebar"),
    showTab: (tab) => fired.push(`showTab:${tab}`),
    toggleInvert: () => fired.push("toggleInvert"),
    about: () => fired.push("about"),
    checkForUpdates: () => fired.push("checkForUpdates"),
    commandLineTool: (install) => fired.push(`commandLineTool:${install}`),
    makeDefaultPdfApp: () => fired.push("makeDefaultPdfApp"),
    automaticUpdates: () => automatic,
    setAutomaticUpdates: (enabled) => { automatic = enabled; fired.push(`setAutomaticUpdates:${enabled}`); },
    applyUpdate: () => fired.push("applyUpdate"),
    restartForUpdate: () => fired.push("restartForUpdate"),
    // Default false, so a test that says nothing about updates exercises the
    // state a launch actually starts in rather than the convenient one.
    updateAvailable: () => update.available ?? false,
    updateReady: () => update.ready ?? false,
    rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
    deletePage: () => fired.push("deletePage"),
    insertBlankPage: () => fired.push("insertBlankPage"),
    insertSizedPage: (name) => fired.push(`insertSizedPage:${name}`),
    importPages: () => fired.push("importPages"),
    pendingImport: () => waiting,
    insertChosenPages: (pages) => fired.push(`insertChosenPages:${pages.join("+")}`),
    dropImport: () => fired.push("dropImport"),
    cropPage: (to) => fired.push(`cropPage:${to}`),
    redactRegion: () => fired.push("redactRegion"),
    redactSelection: () => fired.push("redactSelection"),
    redactMatches: () => fired.push("redactMatches"),
    matchCount: () => (matched ? 3 : 0),
    movePage: (delta) => fired.push(`movePage:${delta}`),
    undoEdit: () => fired.push("undoEdit"),
    redoEdit: () => fired.push("redoEdit"),
    // Default false for the same reason the update pair is: an empty journal is
    // the state every document opens in, and defaulting to true would make the
    // guards' with-nothing-to-undo direction the untested one.
    canUndo: () => journal.undo ?? false,
    canRedo: () => journal.redo ?? false,
    // Default false, like the two pairs above: a document opens with nothing
    // selected, so the highlight command's withheld direction is the one a test
    // that says nothing about a selection exercises.
    markSelection: (kind) => fired.push(`markSelection:${kind}`),
    // Records whether a point came with it, because that is the whole
    // difference between the two routes into a comment and the thing a test
    // about the palette needs to be able to see.
    addComment: (at) => fired.push(`addComment:${at === null ? "here" : "at"}`),
    drawBox: () => fired.push("drawBox"),
    drawEllipse: () => fired.push("drawEllipse"),
    stamp: (name: StampName) => fired.push(`stamp:${name}`),
    drawTextBox: () => fired.push("drawTextBox"),
    drawField: (kind, options) => fired.push(`drawField:${kind}${options ? `:${options.join("|")}` : ""}`),
    drawRadio: (group) => fired.push(`drawRadio:${group}`),
    fieldBorder: () => framing,
    setFieldBorder: (border) => { framing = border; fired.push(`setFieldBorder:${border}`); },
    pickedMarks: () => update.picked ?? 0,
    duplicatePicked: () => fired.push("duplicatePicked"),
    canOrderTabs: () => update.canOrderTabs ?? false,
    orderTabs: () => fired.push("orderTabs"),
    fieldPicked: () => update.fieldPicked ?? false,
    fieldProperties: () => fired.push("fieldProperties"),
    arrange: (how) => fired.push(`arrange:${how}`),
    savedFields: () => update.savedFields ?? 0,
    signableFields: () => update.signable ?? 0,
    signField: () => fired.push("signField"),
    formEditing: () => formEditing,
    setFormEditing: (on) => { formEditing = on; fired.push(`setFormEditing:${on}`); },
    draw: () => fired.push("draw"),
    erase: () => fired.push("erase"),
    hasSelection: () => selected,
    // Default false, on the same reasoning: a document opens with no note open,
    // so the withheld direction is what a test that says nothing about a mark
    // exercises.
    removeMark: () => fired.push("removeMark"),
    removeRedaction: () => fired.push("removeRedaction"),
    setMarkColor: (id: string) => fired.push(`setMarkColor:${id}`),
    setNib: (id: string) => fired.push(`setNib:${id}`),
    markColor: () => "default",
    hasOpenMark: () => markOpen,
    hasPickedRedaction: () => redactionPicked,
    canEditComment: () => commentEditable,
    editComment: () => fired.push("editComment"),
    canReplyToComment: () => commentReplyable,
    replyToComment: () => fired.push("replyToComment"),
    canDeleteComment: () => commentDeletable,
    deleteComment: () => fired.push("deleteComment"),
    // Default false, on the reasoning the journal pair above states: a document
    // opens with nothing to save, so a test that says nothing about edits
    // exercises the direction where Save is withheld.
    saveDocument: () => fired.push("saveDocument"),
    isDirty: () => dirty,
    saveCopy: () => fired.push("saveCopy"),
    redactCopy: () => fired.push("redactCopy"),
    redactRasterCopy: () => fired.push("redactRasterCopy"),
    recogniseText: () => fired.push("recogniseText"),
    protectCopy: () => fired.push("protectCopy"),
    unprotectCopy: () => fired.push("unprotectCopy"),
    compressCopy: () => fired.push("compressCopy"),
    redactDocument: () => fired.push("redactDocument"),
    extractPages: (slots: number[]) => fired.push(`extractPages:${slots.join("+")}`),
    splitDocument: (groups: number[][]) =>
      fired.push(`splitDocument:${groups.map((g) => g.join("+")).join("|")}`),
    mergeDocuments: () => fired.push("mergeDocuments"),
    fromPictures: () => fired.push("fromPictures"),
    signDocument: () => fired.push("signDocument"),
    showProperties: () => fired.push("showProperties"),
  };
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  return { registry, fired };
}

describe("the nib commands", () => {
  it("every nib command asks for its own nib", () => {
    // Four commands out of one `map`, which is the colours' shape below and
    // carries their warning: a wrong argument is wrong four times at once and
    // every one still runs, so a check that only asserted each reaches `setNib`
    // would pass where every entry passed "medium". Asserted from `NIBS` rather
    // than from a list written twice.
    for (const entry of NIBS) {
      const { registry, fired } = harness();
      expect(registry.run(`edit.nib.${entry.id}`)).toBe(true);
      expect(fired).toEqual([`setNib:${entry.id}`]);
    }
  });

  it("gives the nibs titles that are nobody's prefix", () => {
    // A title that is a strict prefix of another ties in the ranking, and
    // registration order then decides which one typing the full name runs ---
    // which this repository has paid for once, with one command running its
    // neighbour. `rank`'s whole-title bonus is what settles the tie now, so this
    // is a second line rather than the only one; it is worth having because a
    // family built from a table is where a name that swallows another arrives
    // without anybody looking at the other three.
    //
    // `marknibs.ts` asserts it of the words; this asserts it of the titles a
    // reader actually types, which is what the ranking looks at.
    const { registry } = harness();
    const titles = NIBS.map(
      (entry) => registry.find(`edit.nib.${entry.id}`)?.title ?? "",
    );
    expect(titles.filter((title) => title.length > 0)).toHaveLength(NIBS.length);
    for (const one of titles) {
      for (const other of titles) {
        if (one === other) continue;
        expect(
          other.startsWith(one),
          `"${one}" is a strict prefix of "${other}"`,
        ).toBe(false);
      }
    }
  });

  it("is offered with a document open whether or not a pen is armed", () => {
    // The guard that is deliberately not "a drawing tool is armed". A reader
    // picks the nib and then takes the pen out at least as often as the
    // reverse, and greying the choice until a tool is armed would make the
    // commoner order impossible. The harness arms nothing, which is what makes
    // this the case under test.
    const { registry } = harness();
    const offered = registry.search("").map((ranked) => ranked.command.id);
    for (const entry of NIBS) expect(offered).toContain(`edit.nib.${entry.id}`);
  });
});

describe("the colour commands", () => {
  it("every colour command asks for its own colour", () => {
    // Seven commands out of one `map`, which is the shape this file's own note
    // about `movePage` warns about: a wrong argument is wrong seven times at
    // once and every one of them still runs, so a check that only asserted each
    // reaches `setMarkColor` would pass on a palette where every swatch is
    // yellow. Asserted from `PALETTE` rather than a list written twice.
    for (const entry of PALETTE) {
      const { registry, fired } = harness();
      expect(registry.run(`edit.color.${entry.id}`)).toBe(true);
      expect(fired).toEqual([`setMarkColor:${entry.id}`]);
    }
  });

  it("is offered with a document open whether or not a mark is", () => {
    // The guard that is deliberately not `hasOpenMark`: choosing a colour
    // before marking is the commoner of the two things this command does, and
    // greying it out until a note is open would refuse exactly that.
    const { registry } = harness();
    const offered = registry.search("").map((ranked) => ranked.command.id);
    for (const entry of PALETTE) expect(offered).toContain(`edit.color.${entry.id}`);
  });
});

describe("Reload from disk", () => {
  it("runs the reload action and nothing else", () => {
    // "And nothing else" is the half that can fail quietly: a command wired to
    // the wrong action still returns true from `run`, and a test asserting only
    // that would pass with `openDocument` in its place --- which would throw
    // away the reader's document and put a file dialog in front of them.
    const { registry, fired } = harness();
    expect(registry.run("file.reload")).toBe(true);
    expect(fired).toEqual(["reloadDocument"]);
  });

  it("is offered when a document is open", () => {
    const { registry } = harness();
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).toContain("file.reload");
  });

  it("is withheld, and refuses to run, with no document", () => {
    // Both halves, because they are separate mechanisms. The palette filters on
    // `enabled`, but a keybinding or a stale palette row can still reach `run`
    // --- and there is nothing to reload before a file is open, so the guard has
    // to hold on the path that does not consult the list.
    const { registry, fired } = harness(false);
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).not.toContain("file.reload");
    expect(registry.run("file.reload")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("is findable by typing, which is the only way to reach it", () => {
    // It has no keyboard binding on purpose --- ⌘R is the rotate chord --- so
    // the palette is not one route among several, it is the route. A command
    // that ranks below the fold for its own name is unreachable in practice.
    const { registry } = harness();
    const top = registry.search("reload")[0];
    expect(top?.command.id).toBe("file.reload");
  });

  it("advertises no shortcut", () => {
    // The palette renders whatever `keys` holds. An id absent from the bindings
    // table has none, and a label invented here would advertise a chord no
    // handler matches --- the exact gap `keys.ts` was written to close.
    const { registry } = harness();
    expect(registry.find("file.reload")?.keys).toBeUndefined();
  });
});

/**
 * What happens when the open file changes on disk: three commands, one choice.
 *
 * Each is offered while its mode is not the current one, so the palette shows
 * the two changes available and never a command that would do nothing.
 */
describe("the disk-change commands", () => {
  it("offers every mode but the current one, and sets the one chosen", () => {
    for (const current of DISK_CHANGE_MODES) {
      const { registry, fired } = harness(false, { disk: current });
      expect(registry.run(`file.onDiskChange.${current}`)).toBe(false);
      expect(fired).toEqual([]);
      const other = DISK_CHANGE_MODES.find((mode) => mode !== current)!;
      expect(registry.run(`file.onDiskChange.${other}`)).toBe(true);
      expect(fired).toEqual([`setDiskChangeMode:${other}`]);
      // The choice took: the one just chosen is withdrawn, the old one is back.
      expect(registry.run(`file.onDiskChange.${other}`)).toBe(false);
      expect(registry.run(`file.onDiskChange.${current}`)).toBe(true);
    }
  });

  it("names each choice so the palette finds all three by one phrase", () => {
    const { registry } = harness(false, { disk: "ignore" });
    const found = registry.search("changes on disk").map((ranked) => ranked.command.id);
    expect(found).toEqual(["file.onDiskChange.ask", "file.onDiskChange.reload"]);
  });
});

/**
 * The two update commands.
 *
 * `viewercheck.ts` classifies both as `undriven` --- one would reach the network
 * from a check that is otherwise entirely offline, the other would replace the
 * running binary mid-run --- so this is the only place their wiring is asserted
 * at all. What matters is the pair of guards on the install command, because
 * they encode a distinction a single "is there an update" flag would lose.
 */
describe("the form field commands", () => {
  it("arm one drag with the kind each names", () => {
    for (const [id, kind] of [
      ["edit.addTextField", "text"],
      ["edit.addMultilineField", "multiline"],
      ["edit.addCheckbox", "checkbox"],
    ] as const) {
      const { registry, fired } = harness(true);
      expect(registry.run(id), id).toBe(true);
      expect(fired, id).toEqual([`drawField:${kind}`]);
    }
  });

  it("need a document to place a field on", () => {
    const { registry, fired } = harness(false);
    for (const id of ["edit.addTextField", "edit.addMultilineField", "edit.addCheckbox"]) {
      expect(registry.run(id), id).toBe(false);
    }
    expect(fired).toEqual([]);
  });
});

describe("the border of a new text field", () => {
  it("offers only the choice that is not the current one, with or without a document", () => {
    for (const open of [true, false]) {
      for (const framing of [true, false]) {
        const { registry, fired } = harness(open, { fieldBorder: framing });
        const available = framing ? "edit.fieldBorderOff" : "edit.fieldBorderOn";
        const unavailable = framing ? "edit.fieldBorderOn" : "edit.fieldBorderOff";
        expect(registry.run(unavailable)).toBe(false);
        expect(fired).toEqual([]);
        expect(registry.run(available)).toBe(true);
        expect(fired).toEqual([`setFieldBorder:${!framing}`]);
        expect(registry.run(available)).toBe(false);
      }
    }
  });
});

describe("the commands that bring tabs back", () => {
  it("offers only the launch choice that is not the current one", () => {
    for (const restoring of [true, false]) {
      const { registry, fired } = harness(false, { restoreTabs: restoring });
      const available = restoring ? "file.reopenLastDocumentAtLaunch" : "file.reopenTabsAtLaunch";
      const unavailable = restoring ? "file.reopenTabsAtLaunch" : "file.reopenLastDocumentAtLaunch";
      expect(registry.run(unavailable)).toBe(false);
      expect(fired).toEqual([]);
      expect(registry.run(available)).toBe(true);
      expect(fired).toEqual([`setRestoreTabs:${!restoring}`]);
      expect(registry.run(available)).toBe(false);
      expect(registry.run(unavailable)).toBe(true);
    }
  });

  it("reopens last time's tabs when some are not open, with or without a document", () => {
    for (const open of [true, false]) {
      const none = harness(open);
      expect(none.registry.run("file.reopenLastTabs"), String(open)).toBe(false);
      const some = harness(open, { reopenable: 2 });
      expect(some.registry.run("file.reopenLastTabs"), String(open)).toBe(true);
      expect(some.fired).toEqual(["reopenLastTabs"]);
    }
  });

  it("withholds the reopening while a document is busy, but not the preference", () => {
    const busy = harness(
      true, { reopenable: 2 }, {}, false, false, false, {}, false, false, false, false, false, true,
    );
    expect(busy.registry.run("file.reopenLastTabs")).toBe(false);
    expect(busy.registry.run("file.reopenTabsAtLaunch")).toBe(true);
  });
});

describe("clearing the recent documents", () => {
  it("is offered while there are some, with or without a document", () => {
    for (const open of [true, false]) {
      const none = harness(open);
      expect(none.registry.run("file.clearRecents"), String(open)).toBe(false);
      expect(none.fired).toEqual([]);
      const some = harness(open, { recents: 1 });
      expect(some.registry.run("file.clearRecents"), String(open)).toBe(true);
      expect(some.fired).toEqual(["clearRecentDocuments"]);
    }
  });

  it("is not withheld while a document is busy", () => {
    // It changes a list and no document, as the launch preference beside it.
    const busy = harness(
      true, { recents: 2 }, {}, false, false, false, {}, false, false, false, false, false, true,
    );
    expect(busy.registry.run("file.reopenLastTabs")).toBe(false);
    expect(busy.registry.run("file.clearRecents")).toBe(true);
  });
});

describe("the update commands", () => {
  it("offers only the applicable preference change and keeps manual checking available", () => {
    for (const automatic of [true, false]) {
      const { registry, fired } = harness(false, { automatic });
      const available = automatic ? "app.disableAutomaticUpdates" : "app.enableAutomaticUpdates";
      const unavailable = automatic ? "app.enableAutomaticUpdates" : "app.disableAutomaticUpdates";
      expect(registry.run(unavailable)).toBe(false);
      expect(fired).toEqual([]);
      expect(registry.run(available)).toBe(true);
      expect(fired).toEqual([`setAutomaticUpdates:${!automatic}`]);
      expect(registry.run(available)).toBe(false);
      expect(registry.run(unavailable)).toBe(true);
      expect(registry.run("app.checkForUpdates")).toBe(true);
      expect(fired.at(-1)).toBe("checkForUpdates");
    }
  });

  it("checks for updates, reaching that action and no other", () => {
    const { registry, fired } = harness();
    expect(registry.run("app.checkForUpdates")).toBe(true);
    expect(fired).toEqual(["checkForUpdates"]);
  });

  it("offers the check on every launch, including one with no document", () => {
    // Deliberately unguarded: asking again is the only way back from a failed
    // check, and a launch with no network is exactly when a reader would.
    const offered = harness(false).registry.search("").map((r) => r.command.id);
    expect(offered).toContain("app.checkForUpdates");
  });

  it("withholds the install until a check has found something", () => {
    // The control for the test below. Without it, a guard that was always true
    // would pass the "offered" case and nothing would notice.
    const { registry, fired } = harness(true, { available: false });
    expect(registry.search("").map((r) => r.command.id)).not.toContain("app.installUpdate");
    expect(registry.run("app.installUpdate")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("offers the install once an update has been found", () => {
    const { registry, fired } = harness(true, { available: true });
    expect(registry.search("").map((r) => r.command.id)).toContain("app.installUpdate");
    expect(registry.run("app.installUpdate")).toBe(true);
    expect(fired).toEqual(["applyUpdate"]);
  });

  it("withdraws the install once the update is applied and waiting on a restart", () => {
    // The half a single flag would lose: `available` is still true here, and
    // the command must be gone anyway. A command that stays live after it has
    // run tells the reader the first run did not work.
    const { registry, fired } = harness(true, { available: true, ready: true });
    expect(registry.search("").map((r) => r.command.id)).not.toContain("app.installUpdate");
    expect(registry.run("app.installUpdate")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("withholds the restart until the update is actually applied", () => {
    // The control for the pair below. `available` alone is not enough: an
    // update found and not yet installed has nothing for a restart to finish,
    // and offering one would end the process for no gain.
    for (const update of [{}, { available: true }]) {
      const { registry, fired } = harness(true, update);
      expect(registry.search("").map((r) => r.command.id)).not.toContain("app.restartForUpdate");
      expect(registry.run("app.restartForUpdate")).toBe(false);
      expect(fired).toEqual([]);
    }
  });

  it("offers the restart exactly where the install is withdrawn", () => {
    // **The two guards are not mirror images and this is where that shows.**
    // `install` needs an update found *and* not yet applied; `restart` needs it
    // applied. In the one state a reader is left in after installing, exactly
    // one of the two must be live --- before 26.9.17 neither was, and the
    // header said "restart to finish" over a disabled button.
    const { registry, fired } = harness(true, { available: true, ready: true });
    const offered = registry.search("").map((r) => r.command.id);
    expect(offered).not.toContain("app.installUpdate");
    expect(offered).toContain("app.restartForUpdate");
    expect(registry.run("app.restartForUpdate")).toBe(true);
    expect(fired).toEqual(["restartForUpdate"]);
  });

  it("gives the header and the palette one name for the restart", () => {
    // A button and a palette row that read differently are two actions to a
    // reader. Compared against `updateLabel` rather than against a literal, so
    // changing either side alone is what goes red.
    const { registry } = harness(true, { available: true, ready: true });
    expect(registry.find("app.restartForUpdate")?.title).toBe(
      updateLabel({ kind: "ready", version: "26.9.17" }),
    );
  });

  it("promises no restart from the command that may not perform one", () => {
    // On macOS installing replaces the bundle and leaves this process running
    // the old code, so "Install update and restart" --- which is what this said
    // until 26.9.17 --- was a title claiming a step the command does not take.
    // A title may promise less than a command performs; never more.
    const { registry } = harness(true, { available: true });
    expect(registry.find("app.installUpdate")?.title).toBe("Install update");
  });

  it("finds both by typing, which is the only way to reach either", () => {
    // Neither has a binding, so palette rank is not one route among several.
    expect(harness().registry.search("check for updates")[0]?.command.id).toBe(
      "app.checkForUpdates",
    );
    expect(harness(true, { available: true }).registry.search("install update")[0]?.command.id).toBe(
      "app.installUpdate",
    );
  });
});

describe("the commands a document is needed for", () => {
  it("leaves only the commands that genuinely need no document", () => {
    // The control for the withholding test above: it proves the guard is
    // `withDocument` rather than something that hides every command, and it
    // fails if a *new* command is registered without one --- which is the
    // mistake this catches that a check on `file.reload` alone cannot.
    //
    // It is an exact list rather than a `toContain`, and that is the whole
    // value: this went red when the update check was added, which is correct,
    // because "no document needed" is a claim each command has to earn. Two
    // earn it. Opening one is how you get a document at all, and checking for
    // updates has nothing to do with documents --- a reader whose launch found
    // no network has no document open either, and asking again is their only
    // route back. Installing an update is *not* here: it is guarded on having
    // found one, which the default harness has not.
    //
    // `app.about` joined on 2026-08-19 and turned this red on the way, which is
    // the arrangement working exactly as the paragraph above describes. It earns
    // the claim more plainly than either of the others: it reads a string the
    // binary was compiled with, so there is nothing for a document to be needed
    // for -- and the reader most likely to ask which version they are running is
    // the one looking at an empty window because a document would not open.
    //
    // The command-line tool's pair joined on 2026-09-27 on the same footing: it
    // links or removes a file outside any document, and the obvious moment to
    // install it is right after installing the application, with nothing open.
    const { registry } = harness(false);
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).toEqual([
      // A preference, and off by default, so this is the one of its pair on
      // offer. First because the tab commands are registered first.
      "file.reopenTabsAtLaunch",
      "file.open",
      // It begins with pictures, not with a document.
      "file.fromPictures",
      // A preference, not something done to a document, and the default
      // harness is in `ask` --- so the other two are what is on offer.
      "file.onDiskChange.reload",
      "file.onDiskChange.ignore",
      "app.about",
      "app.checkForUpdates",
      "app.disableAutomaticUpdates",
      "app.installCommandLineTool",
      "app.uninstallCommandLineTool",
      "app.makeDefaultPdfApp",
      // A preference about fields placed next, on by default, so turning it
      // off is the one of its pair on offer.
      "edit.fieldBorderOff",
    ]);
  });

  it("asks radio buttons for their group and arms the drag with it", () => {
    const { registry, fired } = harness(true);
    const argument = registry.all().find((c) => c.id === "edit.addRadio")?.argument;
    expect(argument?.problem("  ")).toContain("name of the group");
    expect(argument?.problem("a.b")).toContain("period");
    expect(argument?.problem("Payment")).toBeNull();
    expect(argument?.preview?.(" Payment ")).toContain("a button of Payment");
    expect(argument?.preview?.("")).toBe("");
    registry.run("edit.addRadio", " Payment ");
    expect(fired.at(-1)).toBe("drawRadio:Payment");
    const before = fired.length;
    registry.run("edit.addRadio", "a.b");
    expect(fired.length).toBe(before);
    expect(harness(false).registry.all().find((c) => c.id === "edit.addRadio")?.enabled?.()).toBe(false);
  });

  it("asks a dropdown for its choices and arms the drag with them", () => {
    const { registry, fired } = harness(true);
    const command = registry.all().find((c) => c.id === "edit.addDropdown");
    const argument = command?.argument;
    expect(argument).toBeDefined();
    expect(argument?.problem("")).toContain("semicolon");
    expect(argument?.problem("Yes; ; No")).toContain("empty choice");
    expect(argument?.problem("Yes; No; Yes")).toBe('"Yes" is there twice');
    expect(argument?.problem("Yes; No")).toBeNull();
    expect(argument?.preview?.("Yes; No")).toBe("2 choices. Then drag the field onto the page.");
    expect(argument?.preview?.("Yes")).toBe("1 choice. Then drag the field onto the page.");
    expect(argument?.preview?.("Yes;;")).toBe("");
    registry.run("edit.addDropdown", " Yes ; No, by post;");
    expect(fired.at(-1)).toBe("drawField:dropdown:Yes|No, by post");
    // Choices that cannot be a list arm nothing.
    const before = fired.length;
    registry.run("edit.addDropdown", "Yes; Yes");
    expect(fired.length).toBe(before);
    expect(harness(false).registry.all().find((c) => c.id === "edit.addDropdown")?.enabled?.()).toBe(false);
  });

  it("offers changing the document's fields when it has some, and finishing while that is on", () => {
    const offered = (open: boolean, update: { formEditing?: boolean; savedFields?: number }) => {
      const { registry } = harness(open, update);
      return ["edit.formEditOn", "edit.formEditOff"].filter(
        (id) => registry.all().find((c) => c.id === id)?.enabled?.() !== false,
      );
    };
    expect(offered(true, { savedFields: 0 })).toEqual([]);
    expect(offered(true, { savedFields: 3 })).toEqual(["edit.formEditOn"]);
    expect(offered(true, { savedFields: 3, formEditing: true })).toEqual(["edit.formEditOff"]);
    expect(offered(false, { savedFields: 3 })).toEqual([]);
    expect(offered(false, { savedFields: 3, formEditing: true })).toEqual([]);
    const { registry, fired } = harness(true, { savedFields: 3 });
    registry.run("edit.formEditOn");
    expect(fired.at(-1)).toBe("setFormEditing:true");
    registry.run("edit.formEditOff");
    expect(fired.at(-1)).toBe("setFormEditing:false");
  });

  it("offers the tab order while there is a field to order and it is not asked for", () => {
    const offered = (open: boolean, canOrderTabs: boolean) =>
      harness(open, { canOrderTabs }).registry.all().find((c) => c.id === "edit.tabOrder")?.enabled?.();
    expect(offered(true, true)).toBe(true);
    expect(offered(true, false)).toBe(false);
    expect(offered(false, true)).toBe(false);
    const { registry, fired } = harness(true, { canOrderTabs: true });
    registry.run("edit.tabOrder");
    expect(fired.at(-1)).toBe("orderTabs");
  });

  it("offers duplicating while a rectangle is picked", () => {
    const offered = (open: boolean, picked: number) =>
      harness(open, { picked }).registry.all().find((c) => c.id === "edit.duplicate")?.enabled?.();
    expect(offered(true, 1)).toBe(true);
    expect(offered(true, 3)).toBe(true);
    expect(offered(true, 0)).toBe(false);
    expect(offered(false, 1)).toBe(false);
    const { registry, fired } = harness(true, { picked: 1 });
    registry.run("edit.duplicate");
    expect(fired.at(-1)).toBe("duplicatePicked");
  });

  it("offers a field's properties while one field is picked", () => {
    const offered = (open: boolean, update: { formEditing?: boolean; fieldPicked?: boolean }) =>
      harness(open, { savedFields: 3, ...update }).registry.all().find((c) => c.id === "edit.fieldProperties")?.enabled?.();
    expect(offered(true, { formEditing: true, fieldPicked: true })).toBe(true);
    expect(offered(true, { formEditing: true })).toBe(false);
    // A field placed in this session is picked with the mode off.
    expect(offered(true, { fieldPicked: true })).toBe(true);
    expect(offered(true, {})).toBe(false);
    expect(offered(false, { formEditing: true, fieldPicked: true })).toBe(false);
    const { registry, fired } = harness(true, { savedFields: 3, formEditing: true, fieldPicked: true });
    registry.run("edit.fieldProperties");
    expect(fired.at(-1)).toBe("fieldProperties");
  });

  it("offers each arrangement once enough marks are picked for it, and runs that one", () => {
    const ARRANGE_COMMANDS = [
      ["left", "edit.alignLeft"],
      ["center", "edit.alignCenter"],
      ["right", "edit.alignRight"],
      ["top", "edit.alignTop"],
      ["middle", "edit.alignMiddle"],
      ["bottom", "edit.alignBottom"],
      ["distributeAcross", "edit.distributeAcross"],
      ["distributeDown", "edit.distributeDown"],
      ["sameWidth", "edit.sameWidth"],
      ["sameHeight", "edit.sameHeight"],
      ["sameSize", "edit.sameSize"],
      ["pageCenter", "edit.centerOnPage"],
      ["pageMiddle", "edit.middleOnPage"],
    ] as const;
    const ids = ARRANGE_COMMANDS.map(([, id]) => id);
    const offered = (picked: number) => {
      const { registry } = harness(true, { picked });
      return ids.filter((id) => registry.all().find((c) => c.id === id)?.enabled?.() !== false);
    };
    expect(offered(0)).toEqual([]);
    // One mark can be centred on the page and nothing else.
    expect(offered(1)).toEqual(["edit.centerOnPage", "edit.middleOnPage"]);
    // Two can be aligned and sized; spacing needs a third between them.
    expect(offered(2)).toEqual(ids.filter((id) => !id.startsWith("edit.distribute")));
    expect(offered(3)).toEqual(ids);
    // And none without a document, however many a stale count says.
    const closed = harness(false, { picked: 3 });
    expect(ids.filter((id) => closed.registry.all().find((c) => c.id === id)?.enabled?.() !== false)).toEqual([]);
    // What is missing is said by the command that is not offered, and only
    // about how many are picked: a closed document has nothing to add.
    const why = (picked: number, id: string, open = true) =>
      harness(open, { picked }).registry.find(id)?.why?.();
    expect(why(2, "edit.distributeAcross")).toBe("needs three picked");
    expect(why(1, "edit.alignLeft")).toBe("needs two picked");
    expect(why(2, "edit.alignLeft")).toBeNull();
    expect(why(3, "edit.distributeAcross")).toBeNull();
    expect(why(3, "edit.alignLeft", false)).toBeNull();
    // Each runs its own arrangement.
    const { registry, fired } = harness(true, { picked: 3 });
    for (const [how, id] of ARRANGE_COMMANDS) {
      const command = registry.all().find((c) => c.id === id);
      expect(command, id).toBeDefined();
      if (command && command.argument === undefined) void command.run();
      expect(fired.at(-1)).toBe(`arrange:${how}`);
    }
    // Thirteen commands, thirteen arrangements, each once.
    expect(ARRANGE_COMMANDS.map(([how]) => how)).toEqual([...ARRANGEMENTS]);
    expect(new Set(ids).size).toBe(ARRANGEMENTS.length);
  });

  it("makes tpdf the default only when the command is run", () => {
    const { registry, fired } = harness(false);
    expect(fired).toEqual([]);
    expect(registry.run("app.makeDefaultPdfApp")).toBe(true);
    expect(fired).toEqual(["makeDefaultPdfApp"]);
  });

  it("asks the backend to install or remove the command-line tool, and nothing else", () => {
    const { registry, fired } = harness(false);
    expect(registry.run("app.installCommandLineTool")).toBe(true);
    expect(registry.run("app.uninstallCommandLineTool")).toBe(true);
    expect(fired).toEqual(["commandLineTool:true", "commandLineTool:false"]);
  });

  it("offers the rest once one is open", () => {
    const { registry } = harness();
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).toContain("file.open");
    expect(offered.length).toBeGreaterThan(1);
  });
});

describe("every registered command", () => {
  it("reaches an action rather than doing nothing", () => {
    // A command whose `run` is a no-op is indistinguishable from a working one
    // in the palette: it appears, it is selectable, it closes the palette. The
    // ones taking an argument or touching the viewer are excluded by name
    // rather than by a filter on behaviour, so adding one to that list is a
    // deliberate act with a reason beside it.
    //
    // The three selection commands are named in full rather than excluded by an
    // `edit.` prefix, and that is a correction rather than a style. The prefix
    // was right while every `edit.` command reached the viewer, and it silently
    // stopped covering the page operations the day they were added under the
    // same prefix --- an exclusion that grows on its own is not an exclusion.
    const REACHES_THE_VIEWER = [
      "view.",
      "nav.",
      "edit.selectAll",
      "edit.copy",
      "edit.clearSelection",
      "find.next",
      "find.previous",
    ];
    // Built with an update on offer, a journal in both directions, a live
    // selection, an open note and unsaved changes, because otherwise
    // `app.installUpdate`, `edit.undo`, `edit.redo`, `edit.highlightSelection`,
    // `edit.removeMark`, `edit.editForeignMark`, `edit.replyToComment` and
    // `file.save` are correctly disabled and this sweep
    // would read a working guard as a no-op command. The sweep asks "does every
    // command reach an action", which presumes each is in a state where it is
    // allowed to run; the guards themselves are asserted above, in both
    // directions.
    //
    // **Two harnesses, because the update flow has two states and no single one
    // offers both of its commands.** `app.installUpdate` needs an update found
    // and not yet applied; `app.restartForUpdate` needs it applied. Run in one
    // state, the sweep reads whichever is correctly withheld as a no-op
    // command. Excluding one by name would be the wrong repair --- an exclusion
    // list is exactly where a genuine no-op hides --- so each command has to
    // reach an action in *at least one* of the two, and the two guards
    // themselves are asserted above in both directions.
    const built = (update: {
      available?: boolean; ready?: boolean; disk?: DiskChangeMode;
      restoreTabs?: boolean; reopenable?: number; recents?: number; fieldBorder?: boolean; picked?: number; fieldPicked?: boolean; formEditing?: boolean; savedFields?: number; canOrderTabs?: boolean; signable?: number;
    }) => harness(
      true,
      update,
      { undo: true, redo: true },
      true,
      true,
      true,
      { back: true, forward: true },
      // A search with matches, so `edit.redactMatches` is in a state where it
      // is allowed to run.
      true,
      // An editable comment on show, so `edit.editForeignMark` is too --- the
      // comment above is why each of them is here rather than the guard being
      // subtracted.
      true,
      // And a replyable one, so `edit.replyToComment` is. Its own argument for
      // the reason the harness gives: one flag for both would let a swap of the
      // two commands' guards pass unnoticed.
      true,
      // And a deletable one, for `edit.deleteComment`. Third flag, same reason.
      true,
      // And a picked region, so `edit.removeRedaction` is allowed to run.
      // Fourth flag, same reason again.
      true,
      false,
      // And a file waiting to be inserted, so `edit.insertPages.range` is.
      { pending: 1, pages: 3, name: "other.pdf" },
    );
    // With tabs from last time to reopen, or that command is withheld in both.
    // And three marks picked, so every arrangement is allowed to run.
    // And a form with fields, so changing them is on offer.
    // And an empty signature field, so signing in it is.
    // And a remembered document, so clearing the list has something to clear.
    const found = built({ available: true, reopenable: 1, recents: 1, picked: 3, savedFields: 2, signable: 1 });
    // The second state also holds the other disk-change mode, for the reason
    // the update pair needs two: each `file.onDiskChange.*` command is withheld
    // while its mode is the current one, so no single state offers all three.
    // And the launch pair, which is the update pair's shape again.
    const applied = built({
      available: true, ready: true, disk: "reload", restoreTabs: true, fieldBorder: false,
      formEditing: true, fieldPicked: true, savedFields: 2, canOrderTabs: true,
    });
    const states = [found, applied];
    const shell = found.registry
      .all()
      .filter(
        (command) => !REACHES_THE_VIEWER.some((p) => command.id.startsWith(p)),
      );
    for (const command of shell) {
      const reached = states.some(({ registry, fired }) => {
        const before = fired.length;
        registry.run(command.id, command.argument ? "1" : undefined);
        return fired.length > before;
      });
      expect(reached, `${command.id} fired nothing in either update state`).toBe(true);
    }
    expect(shell.length).toBeGreaterThan(3);
    // The control for the arrangement above: without it, a second state that
    // happened to offer nothing new would still let the sweep pass, and the
    // two-state shape would be decoration. Each update command must be the one
    // its own state reaches, and neither state reaches both.
    expect(found.fired).toContain("applyUpdate");
    expect(found.fired).not.toContain("restartForUpdate");
    expect(applied.fired).toContain("restartForUpdate");
    expect(applied.fired).not.toContain("applyUpdate");
  });
});

describe("the pages of another file to insert", () => {
  const waiting = { pending: 4, pages: 8, name: "report.pdf" };
  /** A harness with a document open and `file` waiting, or nothing. */
  const withFile = (file: typeof waiting | null) =>
    harness(true, {}, {}, false, false, false, {}, false, false, false, false, false, false, file);
  const range = (registry: CommandRegistry) =>
    registry.find("edit.insertPages.range");

  it("is not offered unless a file is waiting", () => {
    // The one command in the registry that exists only mid-flow: listed at
    // any other time it would be a row that asks for the pages of nothing.
    const idle = withFile(null);
    expect(idle.registry.search("insert pages").map((r) => r.command.id)).not.toContain(
      "edit.insertPages.range",
    );
    expect(idle.registry.run("edit.insertPages.range", "1")).toBe(false);
    expect(idle.fired).toEqual([]);

    const asking = withFile(waiting);
    expect(asking.registry.search("insert pages").map((r) => r.command.id)).toContain(
      "edit.insertPages.range",
    );
  });

  it("is not offered with no document open, even with a file waiting", () => {
    const { registry } = harness(
      false, {}, {}, false, false, false, {}, false, false, false, false, false, false, waiting,
    );
    expect(registry.run("edit.insertPages.range", "1")).toBe(false);
  });

  it("names the file and its pages in the question", () => {
    const command = range(withFile(waiting).registry);
    expect(command?.title).toBe("Insert pages from report.pdf");
    expect(command?.argument?.placeholder).toBe("Pages of report.pdf (1-8); blank for all");
  });

  it("inserts every page for a blank answer", () => {
    const { registry, fired } = withFile(waiting);
    expect(registry.run("edit.insertPages.range", "")).toBe(true);
    expect(fired).toEqual(["insertChosenPages:0+1+2+3+4+5+6+7"]);
  });

  it("inserts the pages named, as pages of the other file in its order", () => {
    // `8,2-3` rather than something already sorted, so an answer handed over
    // in the order typed is a different answer.
    const { registry, fired } = withFile(waiting);
    expect(registry.run("edit.insertPages.range", "8,2-3")).toBe(true);
    expect(fired).toEqual(["insertChosenPages:1+2+7"]);
  });

  it("reads a range against the other file's count, not the document's", () => {
    // The harness document has three pages and the file eight: page 5 is past
    // one and inside the other, which is the pair that says which count asked.
    const command = range(withFile(waiting).registry);
    expect(command?.argument?.problem("5")).toBeNull();
    expect(command?.argument?.problem("9")).toBe("report.pdf has 8 pages");
    expect(command?.argument?.preview("5")).toBe("Insert page 5 of report.pdf");
    expect(command?.argument?.preview("")).toBe("Insert all 8 pages of report.pdf");
  });

  it("releases the file when the question is dismissed", () => {
    const { registry, fired } = withFile(waiting);
    range(registry)?.argument?.dismissed?.();
    expect(fired).toEqual(["dropImport"]);
  });

  it("inserts nothing for an answer that does not parse", () => {
    // The registry refuses it first; this is the guard behind that, for
    // `file.extractPages`' reason.
    const { registry, fired } = withFile(waiting);
    range(registry)?.argument?.run("nonsense");
    expect(fired).toEqual([]);
  });
});

describe("the page operations", () => {
  it("rotate the page with the sign the reader asked for", () => {
    // The sign is the half that fails quietly. A command wired to the wrong
    // direction reaches the right action, returns true, and turns the page the
    // other way --- which reads as a viewer that ignores which key was pressed.
    const { registry, fired } = harness();
    expect(registry.run("edit.rotatePageClockwise")).toBe(true);
    expect(registry.run("edit.rotatePageCounterClockwise")).toBe(true);
    expect(fired).toEqual(["rotatePage:1", "rotatePage:-1"]);
  });

  it("insert a page of the size the command names, and not of another", () => {
    // The pairing is what fails quietly. Five commands built in a `map` all
    // reach the same action, so one wired to the wrong entry of the table --- a
    // closure capturing the loop's last name, which is the classic way a `map`
    // of handlers goes wrong --- returns true, inserts a page, and inserts the
    // wrong one. Every name is run, and the answer has to be every name in the
    // table's own order.
    const { registry, fired } = harness();
    for (const name of PAGE_SIZE_NAMES) {
      expect(registry.run(`edit.insertPage.${name}`), name).toBe(true);
    }
    expect(fired).toEqual(
      PAGE_SIZE_NAMES.map((name) => `insertSizedPage:${name}`),
    );
  });

  it("give the sized inserts titles that are nobody's prefix", () => {
    // `docs/TRAPS.md` records a strict-prefix pair shipping: two titles tie on
    // every term `rank` can see, and registration order decides, so typing one
    // command's full title ran the other. A family of five sharing "Insert
    // blank" is exactly where that arrives, and the sweep in `commands.test.ts`
    // covers the registry as a whole --- this is the local statement, so a
    // rename of one title fails beside the table it came from.
    const { registry } = harness();
    const titles = registry
      .all()
      .map((command) => command.title)
      .filter((title) => title.startsWith("Insert blank"));
    expect(titles.length).toBe(PAGE_SIZE_NAMES.length + 1);
    for (const one of titles) {
      for (const other of titles) {
        if (one === other) continue;
        expect(
          other.startsWith(one),
          `"${one}" is a strict prefix of "${other}"`,
        ).toBe(false);
      }
    }
  });

  it("are withheld with no document", () => {
    const { registry, fired } = harness(false);
    expect(registry.run("edit.rotatePageClockwise")).toBe(false);
    expect(registry.run("edit.deletePage")).toBe(false);
    expect(registry.run("file.saveCopy")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("offer deleting a page in the palette and on no chord", () => {
    // The one command here with no keyboard binding, deliberately: it is the
    // only one that removes something a reader can see, and a mis-pressed chord
    // that does that silently is worse than a second keystroke. The assertion is
    // that it is reachable and that it advertises nothing --- a binding added
    // later without reading `appcommands.ts` turns this red.
    const { registry, fired } = harness();
    const found = registry
      .search("delete")
      .map((ranked) => ranked.command)
      .find((command) => command.id === "edit.deletePage");
    expect(found?.title).toBe("Delete page");
    expect(found?.keys).toBeUndefined();
    expect(registry.run("edit.deletePage")).toBe(true);
    expect(fired).toEqual(["deletePage"]);
  });

  it("say page rather than view, since both are offered at once", () => {
    // Both rotations are in the palette on any open document, so the titles are
    // the only thing telling a reader which one turns the file. A search for
    // "rotate" that could not distinguish them would be a list of four rows
    // where two do something permanent.
    const { registry } = harness();
    const titles = registry
      .search("rotate")
      .map((ranked) => ranked.command.title);
    expect(titles).toContain("Rotate page clockwise");
    expect(titles).toContain("Rotate view clockwise");
  });

  it("extract the pages a reader named, as slots", () => {
    const { registry, fired } = harness();
    // The harness document has three pages, so the selection is written
    // against that rather than against a longer one -- a range past the end is
    // refused, and this test would then be asserting the refusal.
    expect(registry.run("file.extractPages", "1,3")).toBe(true);
    expect(fired).toEqual(["extractPages:0+2"]);
  });

  it("split at the cuts a reader named, as groups of slots", () => {
    // The harness document has three pages. Cutting after page 1 is the
    // smallest split there is, and it is the one that discriminates: a
    // boundary off by one gives `0|1+2` reversed into `0+1|2`, and both are
    // two groups of the right total.
    const { registry, fired } = harness();
    expect(registry.run("file.splitDocument", "1")).toBe(true);
    expect(fired).toEqual(["splitDocument:0|1+2"]);
  });

  it("refuse to split what does not parse, and reach no action", () => {
    // `file.extractPages`' second line of defence, for its reason: the
    // registry refuses a value its `problem` rejected, so a test going through
    // `registry.run` never executes this guard at all.
    const { registry, fired } = harness();
    const command = registry.all().find((c) => c.id === "file.splitDocument");
    command?.argument?.run("nonsense");
    expect(fired).toEqual([]);
  });

  it("report a problem for a cut this document cannot make", () => {
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.splitDocument");
    expect(command?.argument?.problem("3")).toBe(
      "Page 3 is the last page, so cutting after it makes nothing",
    );
  });

  it("report no problem for a cut this document has", () => {
    // The other direction, and the one that goes missing silently: a `problem`
    // answering for everything makes the command unrunnable while every
    // refusal test above still passes.
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.splitDocument");
    expect(command?.argument?.problem("2")).toBeNull();
  });

  it("preview a split as the files it would write", () => {
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.splitDocument");
    expect(command?.argument?.preview("1")).toBe("2 files: 1 + 2 pages");
  });

  it("merge documents through the command, with no value to carry", () => {
    // Registered, guarded on a document, and reaching its action. The last of
    // those is the half that shipped inert once before, when a callback was
    // declared and fired and never wired into the literal that joins the viewer
    // to the model.
    const { registry, fired } = harness();
    expect(registry.run("file.mergeDocuments")).toBe(true);
    expect(fired).toEqual(["mergeDocuments"]);
  });

  it("signs a signature field through its own command, offered only while the document has an empty one", () => {
    const none = harness();
    expect(none.registry.run("file.signField")).toBe(false);
    expect(none.fired).toEqual([]);
    const one = harness(true, { signable: 1 });
    expect(one.registry.all().find((entry) => entry.id === "file.signField")?.title).toBe("Sign in the signature field\u2026");
    expect(one.registry.run("file.signField")).toBe(true);
    expect(one.fired).toEqual(["signField"]);
    expect(harness(false, { signable: 1 }).registry.run("file.signField")).toBe(false);
  });

  it("signs the document through its own command, offered with or without edits", () => {
    // Reaching its action, and only its own: a sign command wired to Save a
    // copy would write an unsigned file and pass every other check here.
    const { registry, fired } = harness();
    const command = registry.all().find((entry) => entry.id === "file.signDocument");
    expect(command?.title).toBe("Sign document\u2026");
    expect(registry.run("file.signDocument")).toBe(true);
    expect(fired).toEqual(["signDocument"]);
    // Offered on an edited document too: the refusal to sign unsaved edits is
    // `signing.ts`'s sentence, and a command that vanished instead would leave
    // the reader looking for it.
    const edited = harness(true, {}, {}, false, false, true);
    expect(edited.registry.run("file.signDocument")).toBe(true);
    // And withheld with no document at all.
    const closed = harness(false);
    expect(closed.registry.run("file.signDocument")).toBe(false);
    expect(closed.fired).toEqual([]);
  });

  it("redact the open file through the command, with no value to carry", () => {
    // Registered, guarded on a document, and reaching its action --- the last of
    // those being the half that shipped inert once before.
    //
    // The pair is what makes this worth its own test rather than leaving it to
    // the sweep: `file.redactCopy` and `file.redactDocument` differ only in
    // where the result goes, and the sweep asks whether each has *an* action
    // rather than whether it has its own. A destructive command wired to its
    // safe twin would pass everything except this.
    const { registry, fired } = harness();
    expect(registry.run("file.redactDocument")).toBe(true);
    expect(fired).toEqual(["redactDocument"]);
  });

  it("creates the image-only fallback through its own command", () => {
    const { registry, fired } = harness();
    const command = registry.all().find((entry) => entry.id === "file.redactRasterCopy");
    expect(command?.title).toBe("Redact to image-only copy...");
    expect(registry.run("file.redactRasterCopy")).toBe(true);
    expect(fired).toEqual(["redactRasterCopy"]);
  });

  it("recognises text through its own command, on any open document", () => {
    const { registry, fired } = harness();
    const command = registry.all().find((entry) => entry.id === "file.recogniseText");
    expect(command?.title).toBe("Recognise text and save as...");
    expect(registry.run("file.recogniseText")).toBe(true);
    expect(fired).toEqual(["recogniseText"]);
    const closed = harness(false);
    expect(closed.registry.run("file.recogniseText")).toBe(false);
    expect(closed.fired).toEqual([]);
  });

  it("offers a smaller copy on any open document", () => {
    const { registry, fired } = harness();
    const titles = new Map(registry.all().map((entry) => [entry.id, entry.title]));
    expect(titles.get("file.compress")).toBe("Save a smaller copy...");
    expect(registry.run("file.compress")).toBe(true);
    expect(fired).toEqual(["compressCopy"]);
    const closed = harness(false);
    expect(closed.registry.run("file.compress")).toBe(false);
    expect(closed.fired).toEqual([]);
  });

  it("sets and removes a password through two commands, on any open document", () => {
    const { registry, fired } = harness();
    const titles = new Map(registry.all().map((entry) => [entry.id, entry.title]));
    expect(titles.get("file.protect")).toBe("Save a copy with a password...");
    expect(titles.get("file.unprotect")).toBe("Save a copy without its password...");
    expect(registry.run("file.protect")).toBe(true);
    expect(registry.run("file.unprotect")).toBe(true);
    expect(fired).toEqual(["protectCopy", "unprotectCopy"]);
    const closed = harness(false);
    expect(closed.registry.run("file.protect")).toBe(false);
    expect(closed.registry.run("file.unprotect")).toBe(false);
    expect(closed.fired).toEqual([]);
  });

  it("withholds document commands while an image-only copy is being made", () => {
    const { registry, fired } = harness(
      true, {}, { undo: true, redo: true }, true, false, true, {}, false,
      false, false, false, false, true,
    );
    expect(registry.run("file.redactRasterCopy")).toBe(false);
    expect(registry.run("file.recogniseText")).toBe(false);
    expect(registry.run("file.protect")).toBe(false);
    expect(registry.run("file.unprotect")).toBe(false);
    expect(registry.run("edit.redactRegion")).toBe(false);
    expect(registry.run("file.save")).toBe(false);
    expect(registry.run("edit.undo")).toBe(false);
    expect(registry.run("file.open")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("offers all three redaction outputs together", () => {
    // The palette is where a reader chooses between destroying their file and
    // writing a new one, so both have to be there to choose from. A registry
    // holding one of them reads as a complete feature.
    const { registry } = harness();
    const ids = registry.all().map((command) => command.id);
    expect(ids).toContain("file.redactRasterCopy");
    expect(ids).toContain("file.redactCopy");
    expect(ids).toContain("file.redactDocument");
  });

  it("reaches the selection's own action rather than the drag's", () => {
    // The two are one keystroke apart in the palette and one word apart in the
    // source, so a copy-and-paste that left this one calling `redactRegion`
    // would arm a drag on a reader who had already said which words they meant
    // --- and the entry would look correct throughout. The registry sweep above
    // cannot see it: `redactRegion` is an action, so the command reaches one.
    const { registry, fired } = harness(true, {}, {}, true);
    expect(registry.run("edit.redactSelection")).toBe(true);
    expect(fired).toEqual(["redactSelection"]);
  });

  it("withholds redacting a selection when there is none", () => {
    // With nothing selected it would mark nothing, and a command that runs and
    // does nothing reads as a broken command. The control is the same registry
    // built with a selection, where it is offered --- without which a registry
    // that withheld it always would pass.
    expect(
      harness(true, {}, {}, false)
        .registry.all()
        .find((c) => c.id === "edit.redactSelection")
        ?.enabled?.(),
    ).toBe(false);
    expect(
      harness(true, {}, {}, true)
        .registry.all()
        .find((c) => c.id === "edit.redactSelection")
        ?.enabled?.(),
    ).toBe(true);
  });

  it("withholds editing a comment when none is on show that can be", () => {
    // The guard is the popup's, and this asserts the command reads it. With no
    // comment open --- or one the file wrote without an object of its own ---
    // there is nothing to name, and a command that runs and does nothing reads
    // as a broken command. The control is the same registry built with an
    // editable comment, without which a registry that withheld it always would
    // pass.
    const editable = (commentEditable: boolean) =>
      harness(true, {}, {}, false, false, false, {}, false, commentEditable)
        .registry.all()
        .find((c) => c.id === "edit.editForeignMark")
        ?.enabled?.();
    expect(editable(false)).toBe(false);
    expect(editable(true)).toBe(true);
  });

  it("withholds replying to a comment when none is on show that can be", () => {
    // The reply command's own pair, and not a second assertion in the test
    // above: the two commands read two different actions, and one test driving
    // both through one flag would pass with either guard reading the other's.
    const replyable = (commentReplyable: boolean) =>
      harness(true, {}, {}, false, false, false, {}, false, false, commentReplyable)
        .registry.all()
        .find((c) => c.id === "edit.replyToComment")
        ?.enabled?.();
    expect(replyable(false)).toBe(false);
    expect(replyable(true)).toBe(true);
  });

  it("withholds replying to a comment with no document, however replyable", () => {
    // The other half of this command's conjunction, for the reason the edit
    // command's own case one line down gives.
    expect(
      harness(false, {}, {}, false, false, false, {}, false, false, true)
        .registry.all()
        .find((c) => c.id === "edit.replyToComment")
        ?.enabled?.(),
    ).toBe(false);
  });

  it("offers deleting a comment exactly when the popup says it can be deleted", () => {
    // The third guard, and its own flag for the second one's reason: three
    // conditions that agree today, and one flag driving all three would let a
    // swap of any two of the commands' guards pass unnoticed.
    const deletable = (commentDeletable: boolean) =>
      harness(true, {}, {}, false, false, false, {}, false, false, false, commentDeletable)
        .registry.all()
        .find((c) => c.id === "edit.deleteComment")
        ?.enabled?.();
    expect(deletable(false)).toBe(false);
    expect(deletable(true)).toBe(true);
  });

  it("withholds deleting a comment with no document, however deletable", () => {
    // The other half of this command's conjunction, for the reason the two
    // cases below give: a test that only ever varies one operand of an `&&`
    // covers the whole and is unfalsifiable in each half.
    expect(
      harness(false, {}, {}, false, false, false, {}, false, false, false, true)
        .registry.all()
        .find((c) => c.id === "edit.deleteComment")
        ?.enabled?.(),
    ).toBe(false);
  });

  it("withholds editing a comment with no document, however editable", () => {
    // The other half of the conjunction, and it needs its own case for the
    // reason the trap index gives: a test that only ever varies one operand of
    // an `&&` covers the whole and is unfalsifiable in each half. Here the
    // popup claims a comment is editable while no document is open, which is
    // not a state the application reaches -- and is exactly what a defect in
    // the ordering would produce.
    expect(
      harness(false, {}, {}, false, false, false, {}, false, true)
        .registry.all()
        .find((c) => c.id === "edit.editForeignMark")
        ?.enabled?.(),
    ).toBe(false);
  });

  it("reaches the pattern command's own action rather than a sibling's", () => {
    // Three commands one word apart in the source, and this is the one whose
    // mis-wiring costs most: it sweeps the whole document. The registry sweep
    // cannot see it, because every one of the three reaches an action.
    const { registry, fired } = harness(
      true,
      {},
      {},
      false,
      false,
      false,
      {},
      true,
    );
    expect(registry.run("edit.redactMatches")).toBe(true);
    expect(fired).toEqual(["redactMatches"]);
  });

  it("withholds redacting every result when the search found none", () => {
    // With no matches it would mark nothing. The control is the same registry
    // built with matches, without which a registry that withheld it always
    // would pass.
    const guard = (matched: boolean) =>
      harness(true, {}, {}, false, false, false, {}, matched)
        .registry.all()
        .find((c) => c.id === "edit.redactMatches")
        ?.enabled?.();
    expect(guard(false)).toBe(false);
    expect(guard(true)).toBe(true);
  });

  it("gives the destructive redaction no keyboard shortcut", () => {
    // Every chord that reads as this command is a save. A slip between Save and
    // a command that destroys content with no undo is the one slip this
    // application must not make cheap --- `edit.deletePage` has the same rule
    // for a smaller loss.
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.redactDocument");
    expect(command?.keys).toBeUndefined();
  });

  it("take no argument for a merge, because a dialog supplies the files", () => {
    // The distinction from `file.extractPages`, which is otherwise its twin.
    // An `argument` here would put a palette text field in front of a command
    // whose input is a list of paths --- and the palette would then refuse to
    // run it until something was typed.
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.mergeDocuments");
    expect(command).toBeDefined();
    expect(command?.argument).toBeUndefined();
  });

  it("refuse to extract what does not parse, and reach no action", () => {
    // Reached through the command's own `run` rather than through
    // `registry.run`, and that is the whole test. The registry refuses a value
    // its `problem` rejected, so going through it means the guard under test is
    // never executed --- the mutation that deletes the guard SURVIVED against
    // exactly that, which is the trap about a test whose precondition is
    // already satisfied.
    //
    // This is the second line of defence, and it is the one that decides
    // whether a defect writes a file: a caller that skipped validation is what
    // it exists for.
    const { registry, fired } = harness();
    const command = registry.all().find((c) => c.id === "file.extractPages");
    command?.argument?.run("nonsense");
    expect(fired).toEqual([]);
  });

  it("report a problem for a range that runs backwards", () => {
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.extractPages");
    expect(command?.argument?.problem("3-1")).toBe("3-1 runs backwards");
  });

  it("report no problem for a range this document has", () => {
    // The other direction, and the one that would go missing silently: a
    // `problem` that answered for everything would make the command
    // unrunnable while every refusal test above still passed.
    const { registry } = harness();
    const command = registry.all().find((c) => c.id === "file.extractPages");
    expect(command?.argument?.problem("1-2")).toBeNull();
  });

  it("offer Save only once there is something to save", () => {
    // Both halves, for the reason the journal pair states: the palette filters
    // on `enabled`, and a keybinding reaches `run` without consulting the list,
    // so a guard that only hid the row would leave ⌘S rewriting every object id
    // in a file the reader has not changed.
    const clean = harness();
    expect(clean.registry.run("file.save")).toBe(false);
    expect(clean.registry.all().find((c) => c.id === "file.save")?.enabled?.()).toBe(false);
    expect(clean.fired).toEqual([]);

    const edited = harness(true, {}, {}, false, false, true);
    expect(edited.registry.run("file.save")).toBe(true);
    expect(edited.fired).toEqual(["saveDocument"]);
  });

  it("withholds Save with no document, however dirty the model claims to be", () => {
    // The two guards are separate questions and this is the one that is easy to
    // drop: `dirty` survives a document being closed in any implementation that
    // reads it off a variable, so a guard on `dirty` alone would offer Save with
    // nothing open.
    const { registry, fired } = harness(false, {}, {}, false, false, true);
    expect(registry.run("file.save")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("offer a copy of any open document, edited or not", () => {
    // Deliberately not guarded on the journal. Saving an unedited copy is how a
    // reader gets a file out of a downloads folder, and a command that appears
    // only after an edit is one nobody finds.
    const { registry, fired } = harness();
    expect(registry.run("file.saveCopy")).toBe(true);
    expect(fired).toEqual(["saveCopy"]);
  });
});

describe("Undo and Redo", () => {
  it("are withheld while the journal is empty", () => {
    // Both halves. The palette filters on `enabled`, and a keybinding reaches
    // `run` without consulting the list --- so a guard that only hid the row
    // would leave ⌘Z reaching an action with nothing to undo.
    const { registry, fired } = harness();
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).not.toContain("edit.undo");
    expect(offered).not.toContain("edit.redo");
    expect(registry.run("edit.undo")).toBe(false);
    expect(registry.run("edit.redo")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("are offered separately, each on its own half of the journal", () => {
    // One flag each, not one "has been edited" flag. A document with an edit
    // and no undone command has something to undo and nothing to redo, and a
    // single flag would offer both.
    const undoable = harness(true, {}, { undo: true });
    const undoOffered = undoable.registry
      .search("")
      .map((ranked) => ranked.command.id);
    expect(undoOffered).toContain("edit.undo");
    expect(undoOffered).not.toContain("edit.redo");

    const redoable = harness(true, {}, { redo: true });
    const redoOffered = redoable.registry
      .search("")
      .map((ranked) => ranked.command.id);
    expect(redoOffered).toContain("edit.redo");
    expect(redoOffered).not.toContain("edit.undo");
  });

  it("reach their own action and no other", () => {
    const { registry, fired } = harness(true, {}, { undo: true, redo: true });
    expect(registry.run("edit.undo")).toBe(true);
    expect(registry.run("edit.redo")).toBe(true);
    expect(fired).toEqual(["undoEdit", "redoEdit"]);
  });

  it("are withheld with no document even when the journal says otherwise", () => {
    // The journal belongs to a document. A state that outlived its document is
    // not a state a reader can act on, and the guard has to say so --- both
    // conditions, not either.
    const { registry, fired } = harness(false, {}, { undo: true, redo: true });
    expect(registry.run("edit.undo")).toBe(false);
    expect(registry.run("edit.redo")).toBe(false);
    expect(fired).toEqual([]);
  });
});

/**
 * The palette's own chord, and the toolbar route that shares its code.
 *
 * ⌘K lived outside the bindings table until the toolbar grew a button for it,
 * matched by a hand-written `(metaKey || ctrlKey) && key === "k"`. That is the
 * spelling this table exists to replace, and the tests below are what say the
 * replacement is not merely tidier.
 */
describe("opening the palette", () => {
  function fakePalette() {
    const events: string[] = [];
    let open = false;
    return {
      events,
      handle: {
        get isOpen() {
          return open;
        },
        open: () => {
          open = true;
          events.push("open");
        },
        close: () => {
          open = false;
          events.push("close");
        },
        askFor: () => {},
      },
    };
  }

  function pressK(modifiers: { shift?: boolean; alt?: boolean } = {}) {
    const { events, handle } = fakePalette();
    let refreshed = 0;
    const event = {
      key: modifiers.shift === true ? "K" : "k",
      metaKey: true,
      ctrlKey: false,
      shiftKey: modifiers.shift ?? false,
      altKey: modifiers.alt ?? false,
      defaultPrevented: false,
      target: null,
      preventDefault: () => {},
    } as unknown as KeyboardEvent;
    handleWindowKey(event, {
      actions: {} as unknown as AppActions,
      palette: () => handle as never,
      hasDocument: () => false,
      refreshRecents: () => {
        refreshed++;
      },
    });
    return { events, refreshed };
  }

  it("opens on Cmd-K and closes on the next one", () => {
    const { events, handle } = fakePalette();
    const deps = { palette: () => handle as never, refreshRecents: () => {} };
    togglePalette(deps);
    togglePalette(deps);
    expect(events).toEqual(["open", "close"]);
  });

  it("refreshes the recent list when it opens and not when it closes", () => {
    // The reason the toolbar button goes through `togglePalette` rather than
    // calling `open()` itself: a second copy would be a second place to forget
    // this, and a stale recents list looks exactly like a correct one.
    const { handle } = fakePalette();
    let refreshed = 0;
    const deps = {
      palette: () => handle as never,
      refreshRecents: () => {
        refreshed++;
      },
    };
    togglePalette(deps);
    expect(refreshed).toBe(1);
    togglePalette(deps);
    expect(refreshed).toBe(1);
  });

  it("does not open on Shift-Cmd-K or Option-Cmd-K", () => {
    // Both of these opened it before the chord moved into the bindings table:
    // the hand-written test read `metaKey` and the letter and nothing else, so
    // every chord built on ⌘K was ⌘K. `matches` tests Shift and Option in both
    // directions, which is the whole reason ⌥⌘G could stop being find-next.
    expect(pressK().events).toEqual(["open"]);
    expect(pressK({ shift: true }).events).toEqual([]);
    expect(pressK({ alt: true }).events).toEqual([]);
  });
});

/**
 * The window shortcuts for the page operations.
 *
 * Driven with a plain object rather than a real `KeyboardEvent`, which is what
 * the handler is written for: it reads five fields and `preventDefault`, and
 * `inTextField` duck-types its target for exactly this reason. Measured here
 * rather than assumed --- this runner has no DOM, `globalThis.HTMLElement` is
 * `undefined`, and `x instanceof HTMLElement` throws
 * `TypeError: Right-hand side of 'instanceof' is not an object`, so the
 * conventional spelling of that guard could not be tested from this file at all.
 */
describe("the window shortcuts for editing", () => {
  function press(
    key: string,
    modifiers: { shift?: boolean; alt?: boolean; handled?: boolean; picked?: number } = {},
    target: { tagName?: string; isContentEditable?: boolean; ownerDocument?: { getSelection(): { type: string } | null } } | null = null,
    journal: { undo?: boolean; redo?: boolean } = { undo: true, redo: true },
    dirty = true,
  ) {
    const { fired, actions } = keyHarness(journal, dirty);
    actions.pickedMarks = () => modifiers.picked ?? 0;
    let prevented = 0;
    const event = {
      key,
      metaKey: true,
      ctrlKey: false,
      shiftKey: modifiers.shift ?? false,
      altKey: modifiers.alt ?? false,
      // What the surface leaves behind when it has already claimed the chord.
      // `false` rather than absent so that the guard reading it is exercised in
      // both directions --- an undefined field is falsy, so a harness that never
      // set it would pass whether the guard existed or not.
      defaultPrevented: modifiers.handled ?? false,
      target,
      preventDefault: () => {
        prevented++;
      },
    } as unknown as KeyboardEvent;
    handleWindowKey(event, {
      actions,
      palette: () => null,
      hasDocument: () => true,
      refreshRecents: () => {},
    });
    return { fired, prevented };
  }

  function keyHarness(
    journal: { undo?: boolean; redo?: boolean },
    dirty = false,
  ) {
    // Its own recorders rather than the palette harness's. The two routes are
    // separate mechanisms --- a command can be registered correctly and bound to
    // nothing, which is the disagreement `keys.ts` exists to make impossible ---
    // and the point of this block is the one the palette does not cover.
    const fired: string[] = [];
    const actions: AppActions = {
    editText: () => { fired.push("editText"); }, signature: () => { fired.push("signature"); },
    fillForm: () => { fired.push("fillForm"); },
      // Two real methods rather than `{}`, because ⌘A and ⌘C are the only
      // window chords that reach *through* `viewer()` instead of an action of
      // their own --- an empty object would make them throw, and a throw here
      // reads as a broken handler rather than a harness that was never told
      // about them.
      viewer: () =>
        ({
          selectPage: () => fired.push("selectPage"),
          copySelection: () => {
            fired.push("copySelection");
            return Promise.resolve();
          },
        }) as never,
      pageCount: () => 3,
      openDocument: () => fired.push("openDocument"),
    closeDocument: () => fired.push("closeDocument"),
    closeAllDocuments: () => fired.push("closeAllDocuments"),
    restoreTabs: () => false,
    setRestoreTabs: (restore) => fired.push(`setRestoreTabs:${restore}`),
    tabsToReopen: () => 0,
    reopenLastTabs: () => fired.push("reopenLastTabs"),
    recentDocuments: () => 0,
    clearRecentDocuments: () => fired.push("clearRecentDocuments"),
    tabLabels: () => ({ canGrow: true, canShrink: true, isDefault: false }),
    resizeTabLabels: (direction: -1 | 0 | 1) => fired.push(`resizeTabLabels:${direction}`),
    nextDocument: (delta) => fired.push(`nextDocument:${delta}`),
    documentCount: () => 2,
      reloadDocument: () => fired.push("reloadDocument"),
      diskChangeMode: () => "ask",
      setDiskChangeMode: (mode) => fired.push(`setDiskChangeMode:${mode}`),
      busyOpening: () => false,
      busyDocument: () => false,
      printDocument: () => fired.push("printDocument"),
      focusFind: () => fired.push("focusFind"),
      toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
      toggleSearchScope: () => fired.push("toggleSearchScope"),
      toggleSidebar: () => fired.push("toggleSidebar"),
      showTab: (tab) => fired.push(`showTab:${tab}`),
      toggleInvert: () => fired.push("toggleInvert"),
      about: () => fired.push("about"),
      checkForUpdates: () => fired.push("checkForUpdates"),
      commandLineTool: (install) => fired.push(`commandLineTool:${install}`),
    makeDefaultPdfApp: () => fired.push("makeDefaultPdfApp"),
      automaticUpdates: () => true,
      setAutomaticUpdates: (enabled) => fired.push(`setAutomaticUpdates:${enabled}`),
      applyUpdate: () => fired.push("applyUpdate"),
      restartForUpdate: () => fired.push("restartForUpdate"),
      updateAvailable: () => false,
      updateReady: () => false,
      rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
      deletePage: () => fired.push("deletePage"),
      insertBlankPage: () => fired.push("insertBlankPage"),
      insertSizedPage: (name) => fired.push(`insertSizedPage:${name}`),
      importPages: () => fired.push("importPages"),
      pendingImport: () => null,
      insertChosenPages: (pages) => fired.push(`insertChosenPages:${pages.join("+")}`),
      dropImport: () => fired.push("dropImport"),
      cropPage: (to) => fired.push(`cropPage:${to}`),
      redactRegion: () => fired.push("redactRegion"),
      redactSelection: () => fired.push("redactSelection"),
      redactMatches: () => fired.push("redactMatches"),
      // This harness is about key handling, and no chord reaches a redaction.
      matchCount: () => 0,
      movePage: (delta) => fired.push(`movePage:${delta}`),
      undoEdit: () => fired.push("undoEdit"),
      redoEdit: () => fired.push("redoEdit"),
      canUndo: () => journal.undo ?? false,
      canRedo: () => journal.redo ?? false,
      markSelection: (kind) => fired.push(`markSelection:${kind}`),
      addComment: (at) => fired.push(`addComment:${at === null ? "here" : "at"}`),
      drawBox: () => fired.push("drawBox"),
      drawEllipse: () => fired.push("drawEllipse"),
      stamp: (name: StampName) => fired.push(`stamp:${name}`),
      drawTextBox: () => fired.push("drawTextBox"),
      drawField: (kind) => fired.push(`drawField:${kind}`),
      drawRadio: (group) => fired.push(`drawRadio:${group}`),
      fieldBorder: () => true,
      setFieldBorder: (border) => fired.push(`setFieldBorder:${border}`),
      pickedMarks: () => 0,
      duplicatePicked: () => fired.push("duplicatePicked"),
      signableFields: () => 0,
      signField: () => fired.push("signField"),
      canOrderTabs: () => false,
      orderTabs: () => fired.push("orderTabs"),
      fieldPicked: () => false,
      fieldProperties: () => fired.push("fieldProperties"),
      arrange: (how) => fired.push(`arrange:${how}`),
      savedFields: () => 0,
      formEditing: () => false,
      setFormEditing: (on) => fired.push(`setFormEditing:${on}`),
    draw: () => fired.push("draw"),
    erase: () => fired.push("erase"),
      hasSelection: () => false,
      removeMark: () => fired.push("removeMark"),
      removeRedaction: () => fired.push("removeRedaction"),
      setMarkColor: (id: string) => fired.push(`setMarkColor:${id}`),
      setNib: (id: string) => fired.push(`setNib:${id}`),
      markColor: () => "default",
      hasOpenMark: () => false,
      hasPickedRedaction: () => false,
      canEditComment: () => false,
      editComment: () => 0,
      canReplyToComment: () => false,
      replyToComment: () => 0,
      canDeleteComment: () => false,
      deleteComment: () => 0,
      saveDocument: () => fired.push("saveDocument"),
      isDirty: () => dirty,
      saveCopy: () => fired.push("saveCopy"),
      redactCopy: () => fired.push("redactCopy"),
      redactRasterCopy: () => fired.push("redactRasterCopy"),
      recogniseText: () => fired.push("recogniseText"),
      protectCopy: () => fired.push("protectCopy"),
      unprotectCopy: () => fired.push("unprotectCopy"),
      compressCopy: () => fired.push("compressCopy"),
    redactDocument: () => fired.push("redactDocument"),
    extractPages: (slots: number[]) => fired.push(`extractPages:${slots.join("+")}`),
    splitDocument: (groups: number[][]) =>
      fired.push(`splitDocument:${groups.map((g) => g.join("+")).join("|")}`),
    mergeDocuments: () => fired.push("mergeDocuments"),
    fromPictures: () => fired.push("fromPictures"),
    signDocument: () => fired.push("signDocument"),
    showProperties: () => fired.push("showProperties"),
    };
    return { fired, actions };
  }

  it("selects the page on Cmd-A and copies on Cmd-C from the chrome", () => {
    // The defect these two arms were added for: a reader clicks the document's
    // name in the toolbar, presses ⌘A, and the web view selects the toolbar
    // --- the Open button, the find toggles and the field's contents --- because
    // the event never reaches the viewer's own handler. Reported from use.
    expect(press("a").fired).toEqual(["selectPage"]);
    expect(press("c").fired).toEqual(["copySelection"]);
  });

  it("claims both chords, so the web view never sees them", () => {
    // Separate from the assertion above on purpose. Reaching the action and
    // taking the key from the web view are two things, and it is the second one
    // that stops the toolbar being selected: an arm that ran `selectPage` and
    // let the default through would select the page *and* the chrome.
    expect(press("a").prevented).toBe(1);
    expect(press("c").prevented).toBe(1);
  });

  it("copies selected error text itself, but still copies PDF text without it", () => {
    for (const type of ["Range", "Caret", "None"]) {
      const selection = { type, toString: () => (type === "Range" ? "Cannot edit this text" : "") };
      const target = { tagName: "PRE", ownerDocument: { getSelection: () => selection } };
      const result = press("c", {}, target);
      expect(result.fired).toEqual(type === "Range" ? [] : ["copySelection"]);
      // Taken either way: the page's copy is this handler's, and the selected
      // words are written out here because the web view does not copy them.
      expect(result.prevented).toBe(1);
      expect(press("s", {}, target).fired).toEqual(["saveDocument"]);
    }
  });

  it("leaves both to the surface when the surface has already taken them", () => {
    // The viewer's own handler matches ⌘A and ⌘C and prevents the default. The
    // event still bubbles to the window, so without this guard both would run
    // twice --- harmless for select-all and two clipboard writes for copy.
    expect(press("a", { handled: true }).fired).toEqual([]);
    expect(press("c", { handled: true }).fired).toEqual([]);
  });

  it("leaves both to the find field when the find field has them", () => {
    // `menubar.ts` gives the reason these two carry no menu accelerator: inside
    // a text field ⌘A means *this field*. Taking it there would stop a reader
    // replacing a query they had half typed.
    const field = { tagName: "INPUT" };
    expect(press("a", {}, field).fired).toEqual([]);
    expect(press("c", {}, field).fired).toEqual([]);
    // And the key is not claimed either, which is the half that matters: a
    // prevented default here would leave the field unable to select its own
    // text at all.
    expect(press("a", {}, field).prevented).toBe(0);
    expect(press("c", {}, field).prevented).toBe(0);
  });

  it("turns the page on Shift-Cmd-R and the other way on Shift-Cmd-L", () => {
    expect(press("R", { shift: true }).fired).toEqual(["rotatePage:1"]);
    expect(press("L", { shift: true }).fired).toEqual(["rotatePage:-1"]);
  });

  it("duplicates what is picked on Cmd-D, and leaves the chord alone with nothing picked", () => {
    expect(press("d", { picked: 2 })).toMatchObject({ fired: ["duplicatePicked"], prevented: 1 });
    expect(press("d", { picked: 0 })).toMatchObject({ fired: [], prevented: 0 });
    expect(press("d", { picked: 1, shift: true }).fired).toEqual([]);
  });

  it("leaves the unshifted chords to the view, which owns them", () => {
    // ⌘R and ⌘L rotate the *view*, and the viewer's own key handler has them.
    // A window handler that matched them too would turn both at once.
    expect(press("r").fired).toEqual([]);
    expect(press("l").fired).toEqual([]);
  });

  it("saves on Cmd-S and saves a copy on Shift-Cmd-S", () => {
    // The pair together, because the failure they guard against is that one
    // chord reaches the other's action --- and ⌘S reaching "save a copy" would
    // put a file dialog in front of a reader who asked for nothing of the kind,
    // while ⇧⌘S reaching Save would replace the file they meant to keep.
    expect(press("s").fired).toEqual(["saveDocument"]);
    expect(press("S", { shift: true }).fired).toEqual(["saveCopy"]);
  });

  it("does nothing on Cmd-S with nothing to save", () => {
    // Silent rather than a refusal from the backend: ⌘S is the chord a reader
    // presses by reflex on a document they have not touched.
    const { fired, prevented } = press("s", {}, null, undefined, false);
    expect(fired).toEqual([]);
    // The key is still claimed --- letting it through would hand ⌘S to the web
    // view, whose own answer to it is a browser save dialog.
    expect(prevented).toBe(1);
  });

  it("undoes on Cmd-Z and redoes on Shift-Cmd-Z", () => {
    expect(press("z").fired).toEqual(["undoEdit"]);
    expect(press("Z", { shift: true }).fired).toEqual(["redoEdit"]);
  });

  it("does nothing on Cmd-Z with an empty journal", () => {
    const { fired } = press("z", {}, null, {});
    expect(fired).toEqual([]);
  });

  it("leaves Cmd-Z to the text field a reader is typing in", () => {
    // The failure this prevents is not subtle in effect and is invisible in
    // cause: a reader correcting a typo in the find field silently undoes a
    // page rotation instead, and nothing on screen connects the two.
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
      const { fired, prevented } = press("z", {}, { tagName });
      expect(fired, tagName).toEqual([]);
      expect(prevented, `${tagName} kept its own undo`).toBe(0);
    }
    const editable = press("z", {}, { tagName: "DIV", isContentEditable: true });
    expect(editable.fired).toEqual([]);
    expect(editable.prevented).toBe(0);
  });

  it("takes Cmd-Z outside a text field, and says so by preventing the default", () => {
    // The control for the test above: without it, a handler that never fired at
    // all would satisfy every "leaves it alone" assertion.
    const { fired, prevented } = press("z", {}, { tagName: "DIV" });
    expect(fired).toEqual(["undoEdit"]);
    expect(prevented).toBe(1);
  });

  it("still takes Shift-Cmd-R from inside a text field", () => {
    // The asymmetry, asserted rather than left to the comment: only the two
    // journal chords yield to a text field, because only they collide with one.
    expect(press("R", { shift: true }, { tagName: "INPUT" }).fired).toEqual([
      "rotatePage:1",
    ]);
  });
});

describe("Highlight selection", () => {
  it("is withheld with nothing selected, and offered once there is", () => {
    // The guard reads two things and both have to bite. A document with no
    // selection is the state every open starts in, so a command offered there
    // does nothing when chosen -- which is the failure a palette exists to
    // prevent.
    //
    // **All three kinds, not the highlight alone.** They are three near-copies
    // of one entry, which is exactly the shape where a guard gets dropped from
    // the second and third without anything noticing --- and each carries a
    // different argument, so a copy that kept the guard and forgot to change
    // the argument gives a reader a Strike out that highlights.
    for (const [id, kind] of [
      ["edit.highlightSelection", "highlight"],
      ["edit.underlineSelection", "underline"],
      ["edit.strikeoutSelection", "strikeout"],
    ] as const) {
      const { registry: idle } = harness(true);
      expect(idle.run(id), `${id} with nothing selected`).toBe(false);

      const { registry, fired } = harness(true, {}, {}, true);
      expect(registry.run(id), `${id} with a selection`).toBe(true);
      expect(fired).toEqual([`markSelection:${kind}`]);
    }
  });

  it("is withheld with no document even when something is selected", () => {
    // Not reachable through the application -- there is nothing to select
    // without a document -- and asserted because the guard is an `&&` of two
    // conditions, and a test for only one of them passes for either.
    const { registry } = harness(false, {}, {}, true);
    expect(registry.run("edit.highlightSelection")).toBe(false);
  });

  it("has no keyboard binding", () => {
    // Deliberate, and stated in the command's own note: a chord that does
    // nothing whenever there is no selection teaches itself badly. Asserted so
    // that adding one is a decision rather than a diff nobody reads.
    const { registry } = harness(true, {}, {}, true);
    const command = registry
      .all()
      .find((entry: { id: string }) => entry.id === "edit.highlightSelection");
    expect(command?.keys).toBeUndefined();
  });
});

/**
 * Back and Forward, which grey when there is nowhere to go.
 *
 * They were guarded on "a document is open" alone until 2026-08-23, so the menu
 * offered Back on a document nobody had jumped in and the press did nothing.
 * The viewer's `onNavigate` callback had been declared for exactly this and was
 * consumed by nothing, which the wiring gate carried as its one exemption.
 */
describe("moving back and forward through jumps", () => {
  /**
   * Whether the registry would offer a command right now.
   *
   * `enabled` is optional on a `Command` --- most have no guard --- so asking
   * for it through `?.` types as possibly-undefined and an unguarded call does
   * not compile. Throwing rather than defaulting: a command that has lost its
   * guard is the defect these tests exist to catch, and `?? true` would report
   * it as working.
   */
  function offers(registry: CommandRegistry, id: string): boolean {
    const command = registry.find(id);
    if (!command) throw new Error(`${id} is not registered`);
    if (!command.enabled) throw new Error(`${id} has no guard`);
    return command.enabled();
  }

  it("withholds both on a document nobody has jumped in", () => {
    // The state a document opens in, and the one every other test in this file
    // exercises by default: the stack is empty in both directions.
    const { registry, fired } = harness();
    expect(registry.run("nav.back")).toBe(false);
    expect(registry.run("nav.forward")).toBe(false);
    expect(fired).toEqual([]);
  });

  it("offers Back once there is somewhere to go, and still withholds Forward", () => {
    // The two are asked separately rather than through one "has a history"
    // predicate, which is what a stack popped in one direction needs: after a
    // jump Back is live and Forward is not, and a single flag cannot say that.
    const { registry } = harness(true, {}, {}, false, false, false, { back: true });
    expect(offers(registry, "nav.back")).toBe(true);
    expect(offers(registry, "nav.forward")).toBe(false);
  });

  it("offers Forward once Back has been pressed, and still offers Back", () => {
    // The mirror. Both live is the ordinary state in the middle of a stack, and
    // a guard that answered one question for both would have to pick one.
    const { registry } = harness(true, {}, {}, false, false, false, {
      back: true,
      forward: true,
    });
    expect(offers(registry, "nav.back")).toBe(true);
    expect(offers(registry, "nav.forward")).toBe(true);
  });

  it("withholds both with no document, whatever a stale history would say", () => {
    // The control on the `&&`: with no viewer there is nothing to ask, and the
    // guard must not reach through a null to a remembered answer.
    const { registry } = harness(false, {}, {}, false, false, false, {
      back: true,
      forward: true,
    });
    expect(offers(registry, "nav.back")).toBe(false);
    expect(offers(registry, "nav.forward")).toBe(false);
  });
});
