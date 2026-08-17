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
  registerAppCommands,
  type AppActions,
} from "./appcommands";
import { CommandRegistry } from "./commands";

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
) {
  const fired: string[] = [];
  const actions: AppActions = {
    // Not a real viewer: nothing here calls a method on it, and the guards ask
    // only whether it is null. A cast is honest about that; a stub with a dozen
    // methods would suggest they are exercised.
    viewer: () => (hasDocument ? ({} as never) : null),
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
    checkForUpdates: () => fired.push("checkForUpdates"),
    applyUpdate: () => fired.push("applyUpdate"),
    // Default false, so a test that says nothing about updates exercises the
    // state a launch actually starts in rather than the convenient one.
    updateAvailable: () => update.available ?? false,
    updateReady: () => update.ready ?? false,
    rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
    deletePage: () => fired.push("deletePage"),
    undoEdit: () => fired.push("undoEdit"),
    redoEdit: () => fired.push("redoEdit"),
    // Default false for the same reason the update pair is: an empty journal is
    // the state every document opens in, and defaulting to true would make the
    // guards' with-nothing-to-undo direction the untested one.
    canUndo: () => journal.undo ?? false,
    canRedo: () => journal.redo ?? false,
    saveCopy: () => fired.push("saveCopy"),
  };
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  return { registry, fired };
}

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
    const { registry } = harness(false);
    const offered = registry.search("").map((ranked) => ranked.command.id);
    expect(offered).toEqual(["file.open", "app.checkForUpdates"]);
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
    // Built with an update on offer and a journal in both directions, because
    // otherwise `app.installUpdate`, `edit.undo` and `edit.redo` are correctly
    // disabled and this sweep would read a working guard as a no-op command.
    // The sweep asks "does every command reach an action", which presumes each
    // is in a state where it is allowed to run; the guards themselves are
    // asserted above, in both directions.
    const { registry, fired } = harness(
      true,
      { available: true },
      { undo: true, redo: true },
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
    modifiers: { shift?: boolean; alt?: boolean } = {},
    target: { tagName?: string; isContentEditable?: boolean } | null = null,
    journal: { undo?: boolean; redo?: boolean } = { undo: true, redo: true },
  ) {
    const { fired, actions } = keyHarness(journal);
    let prevented = 0;
    const event = {
      key,
      metaKey: true,
      ctrlKey: false,
      shiftKey: modifiers.shift ?? false,
      altKey: modifiers.alt ?? false,
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

  function keyHarness(journal: { undo?: boolean; redo?: boolean }) {
    // Its own recorders rather than the palette harness's. The two routes are
    // separate mechanisms --- a command can be registered correctly and bound to
    // nothing, which is the disagreement `keys.ts` exists to make impossible ---
    // and the point of this block is the one the palette does not cover.
    const fired: string[] = [];
    const actions: AppActions = {
      viewer: () => ({}) as never,
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
      checkForUpdates: () => fired.push("checkForUpdates"),
      applyUpdate: () => fired.push("applyUpdate"),
      updateAvailable: () => false,
      updateReady: () => false,
      rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
      deletePage: () => fired.push("deletePage"),
      undoEdit: () => fired.push("undoEdit"),
      redoEdit: () => fired.push("redoEdit"),
      canUndo: () => journal.undo ?? false,
      canRedo: () => journal.redo ?? false,
      saveCopy: () => fired.push("saveCopy"),
    };
    return { fired, actions };
  }

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

  it("saves a copy on Shift-Cmd-S", () => {
    expect(press("S", { shift: true }).fired).toEqual(["saveCopy"]);
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
