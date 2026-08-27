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

import { describe, expect, it } from "vitest";

import {
  handleWindowKey,
  togglePalette,
  registerAppCommands,
  type AppActions,
} from "./appcommands";
import { CommandRegistry } from "./commands";
import { PALETTE } from "./markcolors";
import type { StampName } from "./pages";

/**
 * A registry with every application command in it, and a record of what fired.
 *
 * `viewer` decides whether a document is open, which is what the `enabled`
 * guards read --- so the same builder serves both directions and the
 * with-document case cannot quietly become the only one tested.
 */
function harness(
  hasDocument = true,
  update: { available?: boolean; ready?: boolean } = {},
  journal: { undo?: boolean; redo?: boolean } = {},
  selected = false,
  markOpen = false,
  dirty = false,
  history: { back?: boolean; forward?: boolean } = {},
) {
  const fired: string[] = [];
  const actions: AppActions = {
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
    reloadDocument: () => fired.push("reloadDocument"),
    busyOpening: () => false,
    printDocument: () => fired.push("printDocument"),
    focusFind: () => fired.push("focusFind"),
    toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
    toggleSearchScope: () => fired.push("toggleSearchScope"),
    toggleSidebar: () => fired.push("toggleSidebar"),
    showTab: (tab) => fired.push(`showTab:${tab}`),
    toggleInvert: () => fired.push("toggleInvert"),
    about: () => fired.push("about"),
    checkForUpdates: () => fired.push("checkForUpdates"),
    applyUpdate: () => fired.push("applyUpdate"),
    // Default false, so a test that says nothing about updates exercises the
    // state a launch actually starts in rather than the convenient one.
    updateAvailable: () => update.available ?? false,
    updateReady: () => update.ready ?? false,
    rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
    deletePage: () => fired.push("deletePage"),
    cropPage: (to) => fired.push(`cropPage:${to}`),
    redactRegion: () => fired.push("redactRegion"),
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
    draw: () => fired.push("draw"),
    erase: () => fired.push("erase"),
    hasSelection: () => selected,
    // Default false, on the same reasoning: a document opens with no note open,
    // so the withheld direction is what a test that says nothing about a mark
    // exercises.
    removeMark: () => fired.push("removeMark"),
    setMarkColor: (id: string) => fired.push(`setMarkColor:${id}`),
    markColor: () => "default",
    hasOpenMark: () => markOpen,
    // Default false, on the reasoning the journal pair above states: a document
    // opens with nothing to save, so a test that says nothing about edits
    // exercises the direction where Save is withheld.
    saveDocument: () => fired.push("saveDocument"),
    isDirty: () => dirty,
    saveCopy: () => fired.push("saveCopy"),
    redactCopy: () => fired.push("redactCopy"),
    redactDocument: () => fired.push("redactDocument"),
    extractPages: (slots: number[]) => fired.push(`extractPages:${slots.join("+")}`),
    splitDocument: (groups: number[][]) =>
      fired.push(`splitDocument:${groups.map((g) => g.join("+")).join("|")}`),
    mergeDocuments: () => fired.push("mergeDocuments"),
    showProperties: () => fired.push("showProperties"),
  };
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  return { registry, fired };
}

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
 * The two update commands.
 *
 * `viewercheck.ts` classifies both as `undriven` --- one would reach the network
 * from a check that is otherwise entirely offline, the other would replace the
 * running binary mid-run --- so this is the only place their wiring is asserted
 * at all. What matters is the pair of guards on the install command, because
 * they encode a distinction a single "is there an update" flag would lose.
 */
describe("the update commands", () => {
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
    const { registry } = harness(false);
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).toEqual(["file.open", "app.about", "app.checkForUpdates"]);
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
    // `edit.removeMark` and `file.save` are correctly disabled and this sweep
    // would read a working guard as a no-op command. The sweep asks "does every
    // command reach an action", which presumes each is in a state where it is
    // allowed to run; the guards themselves are asserted above, in both
    // directions.
    const { registry, fired } = harness(
      true,
      { available: true },
      { undo: true, redo: true },
      true,
      true,
      true,
    );
    const shell = registry
      .all()
      .filter(
        (command) => !REACHES_THE_VIEWER.some((p) => command.id.startsWith(p)),
      );
    for (const command of shell) {
      const before = fired.length;
      const argument = command.argument ? "1" : undefined;
      registry.run(command.id, argument);
      expect(fired.length, `${command.id} fired nothing`).toBeGreaterThan(
        before,
      );
    }
    expect(shell.length).toBeGreaterThan(3);
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

  it("offers both ways to redact, and never one instead of the other", () => {
    // The palette is where a reader chooses between destroying their file and
    // writing a new one, so both have to be there to choose from. A registry
    // holding one of them reads as a complete feature.
    const { registry } = harness();
    const ids = registry.all().map((command) => command.id);
    expect(ids).toContain("file.redactCopy");
    expect(ids).toContain("file.redactDocument");
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
    modifiers: { shift?: boolean; alt?: boolean; handled?: boolean } = {},
    target: { tagName?: string; isContentEditable?: boolean } | null = null,
    journal: { undo?: boolean; redo?: boolean } = { undo: true, redo: true },
    dirty = true,
  ) {
    const { fired, actions } = keyHarness(journal, dirty);
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
      reloadDocument: () => fired.push("reloadDocument"),
      busyOpening: () => false,
      printDocument: () => fired.push("printDocument"),
      focusFind: () => fired.push("focusFind"),
      toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
      toggleSearchScope: () => fired.push("toggleSearchScope"),
      toggleSidebar: () => fired.push("toggleSidebar"),
      showTab: (tab) => fired.push(`showTab:${tab}`),
      toggleInvert: () => fired.push("toggleInvert"),
      about: () => fired.push("about"),
      checkForUpdates: () => fired.push("checkForUpdates"),
      applyUpdate: () => fired.push("applyUpdate"),
      updateAvailable: () => false,
      updateReady: () => false,
      rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
      deletePage: () => fired.push("deletePage"),
      cropPage: (to) => fired.push(`cropPage:${to}`),
    redactRegion: () => fired.push("redactRegion"),
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
    draw: () => fired.push("draw"),
    erase: () => fired.push("erase"),
      hasSelection: () => false,
      removeMark: () => fired.push("removeMark"),
      setMarkColor: (id: string) => fired.push(`setMarkColor:${id}`),
      markColor: () => "default",
      hasOpenMark: () => false,
      saveDocument: () => fired.push("saveDocument"),
      isDirty: () => dirty,
      saveCopy: () => fired.push("saveCopy"),
      redactCopy: () => fired.push("redactCopy"),
    redactDocument: () => fired.push("redactDocument"),
    extractPages: (slots: number[]) => fired.push(`extractPages:${slots.join("+")}`),
    splitDocument: (groups: number[][]) =>
      fired.push(`splitDocument:${groups.map((g) => g.join("+")).join("|")}`),
    mergeDocuments: () => fired.push("mergeDocuments"),
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
