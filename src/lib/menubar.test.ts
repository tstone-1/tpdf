/**
 * Tests for the menu bar's layout and the rules that decide its accelerators.
 *
 * Three different things can be wrong here and only the first is obvious. A
 * command can be missing from the menu, which is what this whole feature exists
 * to prevent recurring. An accelerator can be *claimed* that something else on
 * the platform needs --- a menu item takes a chord before the web view sees it,
 * so a wrong one here silently breaks typing in the find field rather than
 * breaking the menu. And two items can claim the same chord, where the second
 * one simply never fires and nothing says so.
 *
 * None of that is visible from `viewercheck.ts`, which drives a real window: the
 * menu bar is AppKit's, outside the page, and a synthetic key event in the web
 * view never goes near it.
 */

import { describe, expect, it } from "vitest";

import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import { BINDINGS, accelerator, type BoundCommand } from "./keys";
import {
  acceleratorFor,
  buildMenu,
  MENU_LAYOUT,
  menuEnablement,
  NOT_IN_MENU,
  NO_ACCELERATOR,
  runMenuCommand,
  SEPARATOR,
} from "./menubar";

/**
 * A registry holding every application command.
 *
 * The actions are a Proxy over the handful of readers the `enabled` guards
 * actually consult; everything else answers with a no-op function. That split is
 * the honest one for this file: nothing here *runs* a command's action --- that
 * is `appcommands.test.ts`, with a stub that records --- while the guards' inputs
 * are exactly what the menu's enabled flags are made of, so they are named and
 * given deliberate values rather than left undefined.
 *
 * A document is open and nothing else is: no update, an empty journal, no
 * selection. That is the state a launch reaches after opening a file, and it
 * gives both directions --- most items live, four withheld --- from one registry.
 */
function registry(overrides: Record<string, () => unknown> = {}): CommandRegistry {
  const answers: Record<string, () => unknown> = {
    // The two fields `find.inSelection`'s guard reads. Not a Viewer; a stub with
    // its real surface would suggest the commands' actions are exercised here.
    viewer: () => ({ searchScoped: false, selectedText: "" }),
    pageCount: () => 3,
    canUndo: () => false,
    canRedo: () => false,
    updateAvailable: () => false,
    updateReady: () => false,
    busyOpening: () => false,
    ...overrides,
  };
  const actions = new Proxy(
    {},
    {
      get: (_target, key: string) => answers[key] ?? (() => undefined),
    },
  ) as AppActions;
  const commands = new CommandRegistry();
  registerAppCommands(commands, actions);
  return commands;
}

/** Every command id the layout names, separators dropped. */
function laidOut(): string[] {
  return MENU_LAYOUT.flatMap((section) =>
    section.items.filter((item): item is string => item !== SEPARATOR),
  );
}

describe("the layout covers the registry", () => {
  it("finds commands at all", () => {
    // The control for every assertion below. All three of them are satisfied by
    // an empty registry and an empty layout, so a `registerAppCommands` that
    // silently registered nothing would leave this file entirely green.
    expect(registry().all().length).toBeGreaterThan(30);
    expect(laidOut().length).toBeGreaterThan(30);
  });

  it("gives every registered command a menu or a written reason", () => {
    // The assertion this file was written for. A command added to the palette
    // and not to a menu is exactly the state the whole application was in, and
    // nothing said so --- the menu bar simply had less in it than the palette.
    const placed = new Set(laidOut());
    const missing = registry()
      .all()
      .map((command) => command.id)
      .filter((id) => !placed.has(id))
      .filter((id) => !NOT_IN_MENU.some((rule) => id.startsWith(rule.prefix)));
    expect(missing).toEqual([]);
  });

  it("names no command the registry does not have", () => {
    // The other direction, and it fails differently: a layout entry for an id
    // that no longer exists is skipped by `buildMenu`, so the menu comes out one
    // item short and looks perfectly normal.
    const known = new Set(registry().all().map((command) => command.id));
    expect(laidOut().filter((id) => !known.has(id))).toEqual([]);
  });

  it("places each command once", () => {
    const seen = laidOut();
    expect(seen.length).toBe(new Set(seen).size);
  });

  it("gives every exclusion a reason worth reading", () => {
    // An exclusion list whose entries can be empty strings is a blanket
    // permission with extra steps.
    for (const rule of NOT_IN_MENU) {
      expect(rule.prefix.length).toBeGreaterThan(0);
      expect(rule.reason.length).toBeGreaterThan(20);
    }
  });
});

describe("accelerators the menu may claim", () => {
  it("renders a plain chord", () => {
    expect(acceleratorFor("file.open")).toBe("CmdOrCtrl+O");
  });

  it("withholds a punctuation chord, because position is not character", () => {
    // Measured on the running application rather than reasoned about. An
    // accelerator names a physical key; the key handler reads the character
    // that key produced. On the German layout `Backslash` is the `#` key and
    // `BracketLeft` is `ö`, so an earlier version of this menu advertised ⌘#
    // and ⌘Ö beside commands whose palette entry reads ⌘\ and ⌘[ --- and ⌘\
    // itself did nothing, which it had never done on this keyboard.
    expect(acceleratorFor("nav.back")).toBeNull();
    expect(acceleratorFor("view.zoomOut")).toBeNull();
    expect(acceleratorFor("view.zoomIn")).toBeNull();
    // ...and the exception, which is what `Binding.code` is for: a binding that
    // names its physical key can be claimed as that key, and the menu then
    // shows whatever the layout prints on it -- ⌘# here, ⌘\\ on a US keyboard,
    // and correct on both because the handler matches the same position.
    expect(acceleratorFor("view.toggleSidebar")).toBe("CmdOrCtrl+Backslash");
  });

  it("carries Shift and Option", () => {
    expect(acceleratorFor("edit.rotatePageClockwise")).toBe(
      "Shift+CmdOrCtrl+R",
    );
    expect(acceleratorFor("find.matchCase")).toBe("Alt+CmdOrCtrl+C");
    expect(acceleratorFor("nav.previousLink")).toBe("Alt+Shift+CmdOrCtrl+L");
  });

  it("withholds a chord a text field claims", () => {
    // The load-bearing case. `handleWindowKey` guards undo with `inTextField`
    // precisely so that ⌘Z in the find bar is text undo; a menu accelerator
    // fires before the page sees the key and would undo that guard.
    for (const id of Object.keys(NO_ACCELERATOR)) {
      expect(acceleratorFor(id)).toBeNull();
    }
    // ...and the control: each of them *has* a binding, so the null above is a
    // decision rather than a command that simply has no chord.
    expect(BINDINGS["edit.undo" as BoundCommand]).toBeDefined();
    expect(accelerator(BINDINGS["edit.undo" as BoundCommand])).toBe(
      "CmdOrCtrl+Z",
    );
  });

  it("withholds an unmodified key", () => {
    // `n` and `p` turn pages. As a menu accelerator either would take the letter
    // out of every text field in the application.
    expect(acceleratorFor("nav.nextPage")).toBeNull();
    expect(acceleratorFor("nav.previousPage")).toBeNull();
    expect(acceleratorFor("nav.firstPage")).toBeNull();
  });

  it("gives no accelerator to a command that has no binding", () => {
    expect(acceleratorFor("view.showThumbnails")).toBeNull();
    expect(acceleratorFor("file.reload")).toBeNull();
  });

  it("claims no chord twice", () => {
    // Two items on one accelerator is not a build error: AppKit takes the first
    // and the second is dead, which looks exactly like a command that does not
    // work rather than like a menu that is wrong.
    const claimed = buildMenu(registry())
      .flatMap((section) => section.items)
      .flatMap((item) =>
        item.kind === "command" && item.accelerator ? [item.accelerator] : [],
      );
    expect(claimed.length).toBeGreaterThan(10);
    expect(claimed.length).toBe(new Set(claimed).size);
  });
});

describe("buildMenu", () => {
  it("carries the command's own title rather than a copy", () => {
    const commands = registry();
    const file = buildMenu(commands).find((section) => section.title === "File");
    const open = file?.items.find(
      (item) => item.kind === "command" && item.id === "file.open",
    );
    expect(open).toMatchObject({
      kind: "command",
      title: commands.find("file.open")?.title,
    });
  });

  it("keeps separators where the layout puts them", () => {
    const file = buildMenu(registry()).find(
      (section) => section.title === "File",
    );
    // Pinned as a whole rather than derived from `MENU_LAYOUT`, deliberately:
    // a check that reads the layout to decide what the layout should produce
    // agrees with itself whatever either says. The cost is that adding a
    // command to the File menu edits this line, which is the point --- the
    // menu's shape is a decision, and a decision should not change silently.
    expect(file?.items.map((item) => item.kind)).toEqual([
      "command",
      "command",
      "separator",
      // Save, Save a copy, Redact and save as, Extract, Split, Merge --- the six
      // that write a file. Split joined them on 2026-08-26 and the redaction
      // later the same day.
      "command",
      "command",
      "command",
      "command",
      "command",
      "command",
      "separator",
      "command",
      "separator",
      "command",
    ]);
  });

  it("marks exactly one section as the application menu", () => {
    const app = buildMenu(registry()).filter((section) => section.app);
    expect(app.length).toBe(1);
  });

  it("reads enablement from the command rather than assuming it", () => {
    // `app.installUpdate` is withheld until there is an update to install, and
    // the Proxy answers false for both halves of its guard, so this is the
    // disabled direction. Without it every item would ship enabled and the menu
    // would offer commands the palette refuses.
    const spec = buildMenu(registry())
      .flatMap((section) => section.items)
      .filter((item) => item.kind === "command");
    const install = spec.find((item) => item.id === "app.installUpdate");
    expect(install).toMatchObject({ enabled: false });
    expect(spec.some((item) => item.enabled)).toBe(true);
  });
});

describe("menuEnablement", () => {
  it("answers for every command in the layout and nothing else", () => {
    const state = menuEnablement(registry());
    expect(Object.keys(state).sort()).toEqual([...laidOut()].sort());
  });
});

describe("runMenuCommand", () => {
  /** A palette that records what it was asked for. */
  function palette() {
    const asked: string[] = [];
    return { asked, palette: { askFor: (id: string) => asked.push(id) } };
  }

  it("runs a plain command through the registry", () => {
    const commands = registry();
    const { palette: p } = palette();
    expect(runMenuCommand(commands, p, "file.open")).toBe(true);
    // Through `run`, which is what records a recent --- the observable that
    // distinguishes it from a menu that called the action directly.
    expect(commands.recents()[0]).toBe("file.open");
  });

  it("opens the palette for a command that takes a value", () => {
    // A menu has nowhere to type "1-3,5", so choosing Extract must ask rather
    // than act. Running it through `registry.run` would be refused for want of
    // an argument and the item would look broken.
    const { asked, palette: p } = palette();
    expect(runMenuCommand(registry(), p, "file.extractPages")).toBe(true);
    expect(asked).toEqual(["file.extractPages"]);
  });

  it("refuses a command whose guard is closed", () => {
    // True, and it cannot fail: `registry.run` checks `enabled` too, so
    // deleting the guard in `runMenuCommand` leaves this exactly as green --
    // the mutation survived it. Kept because the behaviour is worth pinning,
    // and followed by the case that can actually see the guard.
    const commands = registry();
    const { palette: p } = palette();
    expect(runMenuCommand(commands, p, "app.installUpdate")).toBe(false);
    expect(commands.recents()).toEqual([]);
  });

  it("refuses a withheld command that would have opened the palette", () => {
    // The branch where the guard is the only thing there is. An argument
    // command never reaches `registry.run` --- it opens the palette instead ---
    // so without the guard, choosing "Extract pages..." with no document open
    // would put an input on screen for a command that cannot run. Reachable:
    // the enablement push is a round trip, so the bar is briefly one step
    // behind every close.
    const closed = registry({ viewer: () => null });
    const { asked, palette: p } = palette();
    expect(closed.find("file.extractPages")?.enabled?.()).toBe(false);
    expect(runMenuCommand(closed, p, "file.extractPages")).toBe(false);
    expect(asked).toEqual([]);
  });

  it("refuses an id the registry does not have", () => {
    const { palette: p } = palette();
    expect(runMenuCommand(registry(), p, "no.such.command")).toBe(false);
  });

  it("refuses an argument command with no palette to ask in", () => {
    // Reachable: the menu is installed after the palette is built, but a menu
    // event can arrive while the shell is being torn down and rebuilt for a new
    // document. Asking a null palette would throw inside an event listener,
    // where nothing is watching.
    expect(runMenuCommand(registry(), null, "file.extractPages")).toBe(false);
  });
});
