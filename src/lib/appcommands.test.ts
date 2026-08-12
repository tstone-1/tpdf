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

import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";

/**
 * A registry with every application command in it, and a record of what fired.
 *
 * `viewer` decides whether a document is open, which is what the `enabled`
 * guards read --- so the same builder serves both directions and the
 * with-document case cannot quietly become the only one tested.
 */
function harness(hasDocument = true, update: { available?: boolean; ready?: boolean } = {}) {
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
    const REACHES_THE_VIEWER = [
      "view.",
      "nav.",
      "edit.",
      "find.next",
      "find.previous",
    ];
    // Built with an update on offer, because otherwise `app.installUpdate` is
    // correctly disabled and this sweep would read a working guard as a
    // no-op command. The sweep asks "does every command reach an action",
    // which presumes each is in a state where it is allowed to run; the guard
    // itself is asserted four tests above, in both directions.
    const { registry, fired } = harness(true, { available: true });
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
