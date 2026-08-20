/**
 * Tests for the right-click menu.
 *
 * Three things can be wrong and only the first is loud. The menu can offer a
 * command that cannot run, which is the state the web view's own menu was in ---
 * *Reload*, on a document the reader was pointing at. It can offer nothing and
 * open anyway, which reads as a broken application rather than as a surface with
 * nothing to do. And it can leave a separator with no group under it, which is
 * the cosmetic one and the only one a screenshot would catch.
 *
 * The window harness exercises the same menu through a real right-click; this is
 * where the rules that decide what goes in it live.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CommandRegistry, type Command } from "./commands";
import {
  ContextMenu,
  MARK_MENU,
  menuForSurface,
  PAGE_MENU,
  SELECTION_MENU,
  SEPARATOR,
  type Entry,
} from "./contextmenu";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

let dom: FakeDom;
beforeEach(() => {
  dom = installFakeDom();
});
afterEach(() => dom.restore());

/** A registry of commands whose enablement a test decides. */
function registry(open: Record<string, boolean> = {}): {
  commands: CommandRegistry;
  fired: string[];
} {
  const fired: string[] = [];
  const commands = new CommandRegistry();
  // `keys` is spread rather than assigned, because `exactOptionalPropertyTypes`
  // makes an explicit `undefined` a different thing from an absent property ---
  // and "this command has no shortcut" is the absent one, which is exactly the
  // case a row with no key cell is drawn for.
  const make = (id: string, title: string, keys?: string): Command => ({
    id,
    title,
    ...(keys === undefined ? {} : { keys }),
    enabled: () => open[id] ?? true,
    run: () => void fired.push(id),
  });
  commands.register(
    make("edit.rotatePageClockwise", "Rotate page clockwise", "⇧⌘R"),
    make("edit.rotatePageCounterClockwise", "Rotate page anticlockwise", "⇧⌘L"),
    make("edit.movePageUp", "Move page up"),
    make("edit.movePageDown", "Move page down"),
    make("edit.deletePage", "Delete page"),
    make("edit.copy", "Copy selection", "⌘C"),
    make("edit.highlightSelection", "Highlight selection"),
    make("edit.underlineSelection", "Underline selection"),
    make("edit.strikeoutSelection", "Strike out selection"),
    make("edit.selectAll", "Select all on page", "⌘A"),
    make("find.inSelection", "Find: in selection on or off", "⌥⌘S"),
    make("edit.clearSelection", "Clear selection", "Esc"),
    make("edit.removeMark", "Remove mark"),
    make("edit.addComment", "Add comment"),
    make("edit.drawBox", "Draw a box..."),
    make("edit.draw", "Draw freehand..."),
    make("edit.erase", "Erase drawing..."),
  );
  return { commands, fired };
}

function menuOf(open: Record<string, boolean> = {}) {
  const { commands, fired } = registry(open);
  const chosen: string[] = [];
  const host = dom.root;
  const menu = new ContextMenu(host as never, commands, (id) => {
    chosen.push(id);
    commands.run(id);
  });
  return { menu, commands, fired, chosen, host };
}

/** The menu's own children: its rows and rules, in order. */
function drawn(host: unknown): FakeElement[] {
  return ((host as FakeElement).children[0]?.children ?? []) as FakeElement[];
}

/** Every entry in a surface list that is a command id. */
function ids(entries: Entry[]): string[] {
  return entries.filter((entry): entry is string => entry !== SEPARATOR);
}

describe("the surface lists", () => {
  it("name only commands the application has", () => {
    // The control for the whole file. A surface naming an id that no longer
    // exists is silently dropped by `show`, so the menu comes out one entry
    // short and looks perfectly normal.
    const { commands } = registry();
    const known = new Set(commands.all().map((command) => command.id));
    for (const list of [PAGE_MENU, SELECTION_MENU, MARK_MENU]) {
      expect(ids(list).filter((id) => !known.has(id))).toEqual([]);
    }
  });

  it("are not empty", () => {
    expect(ids(PAGE_MENU).length).toBeGreaterThan(3);
    expect(ids(SELECTION_MENU).length).toBeGreaterThan(2);
    // One entry, and asserted rather than left out of this sweep: a list that
    // is exempt from the emptiness check is a list that can quietly become
    // empty. What a mark can be asked to do is exactly this, for now.
    expect(ids(MARK_MENU).length).toBeGreaterThan(0);
  });

  it("offer no command that takes a value", () => {
    // A command with an argument opens the palette rather than acting, which is
    // right for a menu bar and strange for a menu that appeared because the
    // reader pointed at one specific thing.
    const { commands } = registry();
    for (const id of [...ids(PAGE_MENU), ...ids(SELECTION_MENU), ...ids(MARK_MENU)]) {
      expect(commands.find(id)?.argument, id).toBeUndefined();
    }
  });
});

describe("which menu a right-click on the page gets", () => {
  it("offers the mark's menu when the pointer is on a mark", () => {
    expect(menuForSurface(7)).toBe(MARK_MENU);
  });

  it("offers the selection's menu when it is not", () => {
    expect(menuForSurface(null)).toBe(SELECTION_MENU);
  });

  it("treats mark 0 as a mark", () => {
    // The one that would go wrong written as a truthiness test. Mark ids come
    // from the model and there is nothing stopping the first one being 0, at
    // which point a right-click on it silently offers the selection menu ---
    // the exact defect this replaced, back again for one mark in the document.
    expect(menuForSurface(0)).toBe(MARK_MENU);
  });

  it("gives the two cases different menus", () => {
    // The discrimination itself. A rule returning one list for both satisfies
    // neither of the first two on its own, and this says so in one assertion
    // that cannot be passed by a constant.
    expect(menuForSurface(1)).not.toBe(menuForSurface(null));
  });
});

describe("what a menu shows", () => {
  it("offers every command whose guard is open", () => {
    const { menu } = menuOf();
    expect(menu.show(PAGE_MENU, { x: 10, y: 10 })).toBe(true);
    expect(menu.offered).toEqual(ids(PAGE_MENU));
    expect(menu.isOpen).toBe(true);
  });

  it("leaves out a command that cannot run, rather than greying it", () => {
    // The opposite of the menu bar's choice, and deliberate --- see the note in
    // `contextmenu.ts`. A menu built fresh for one click has no continuity to
    // protect, and a short menu of things that work beats a long grey one.
    const { menu } = menuOf({ "edit.deletePage": false });
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    expect(menu.offered).not.toContain("edit.deletePage");
    expect(menu.offered).toContain("edit.movePageUp");
  });

  it("does not open when nothing in it can run", () => {
    // A menu with no entries reads as the application being broken. No menu is
    // the correct answer to a right-click on something with nothing to offer.
    const closed = Object.fromEntries(ids(PAGE_MENU).map((id) => [id, false]));
    const { menu } = menuOf(closed);
    expect(menu.show(PAGE_MENU, { x: 10, y: 10 })).toBe(false);
    expect(menu.isOpen).toBe(false);
  });

  it("draws no separator with nothing under it", () => {
    // Withhold the whole last group. The rule before it would otherwise sit at
    // the bottom of the menu with no rows after it.
    const { menu, host } = menuOf({ "edit.deletePage": false });
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    const roles = drawn(host).map((child) => child.attributes.get("role"));
    expect(roles.at(-1)).toBe("menuitem");
    // ...and the control: a separator is drawn where a group *does* follow.
    expect(roles).toContain("separator");
  });

  it("draws no separator before the first row either", () => {
    // A list whose first group is entirely withheld would otherwise open with a
    // rule across the top.
    const { menu, host } = menuOf({
      "edit.rotatePageClockwise": false,
      "edit.rotatePageCounterClockwise": false,
    });
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    expect(drawn(host)[0]?.attributes.get("role")).toBe("menuitem");
  });

  it("shows each command's own shortcut, and none where there is none", () => {
    const { menu, host } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    const rows = drawn(host).filter((child) => child.children.length > 0);
    expect(rows[0]?.children.map((span) => span.textContent)).toEqual([
      "Rotate page clockwise",
      "⇧⌘R",
    ]);
    // `edit.movePageUp` has no binding, so its row is the title alone -- not a
    // title beside an empty box, which is what rendering an absent key gives.
    const move = rows.find((row) => row.children[0]?.textContent === "Move page up");
    expect(move?.children.length).toBe(1);
  });
});

/** The one field {@link ContextMenu.handleKey} reads. */
const key = (name: string): KeyboardEvent =>
  ({ key: name }) as unknown as KeyboardEvent;

describe("keyboard", () => {
  it("consumes nothing while closed", () => {
    // The reason this is a method the shell calls rather than a listener of its
    // own: a closed menu must not eat Escape from the find bar.
    const { menu } = menuOf();
    expect(menu.handleKey(key("Escape"))).toBe(false);
    expect(menu.handleKey(key("ArrowDown"))).toBe(false);
  });

  it("closes on Escape", () => {
    const { menu } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    expect(menu.handleKey(key("Escape"))).toBe(true);
    expect(menu.isOpen).toBe(false);
  });

  it("walks down from nothing to the first row and up to the last", () => {
    const { menu } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    expect(menu.highlighted).toBe(-1);
    menu.handleKey(key("ArrowDown"));
    expect(menu.highlighted).toBe(0);
    menu.close();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    menu.handleKey(key("ArrowUp"));
    expect(menu.highlighted).toBe(menu.offered.length - 1);
  });

  it("wraps at both ends", () => {
    const { menu } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    const last = menu.offered.length - 1;
    menu.handleKey(key("ArrowUp"));
    expect(menu.highlighted).toBe(last);
    menu.handleKey(key("ArrowDown"));
    expect(menu.highlighted).toBe(0);
  });

  it("runs the highlighted row on Enter and closes", () => {
    const { menu, chosen, fired } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    menu.handleKey(key("ArrowDown"));
    expect(menu.handleKey(key("Enter"))).toBe(true);
    expect(chosen).toEqual(["edit.rotatePageClockwise"]);
    // Through the registry, which is the observable that separates this from a
    // menu holding its own copy of what each command does.
    expect(fired).toEqual(["edit.rotatePageClockwise"]);
    expect(menu.isOpen).toBe(false);
  });

  it("does nothing on Enter with no row highlighted", () => {
    // Reachable: the menu opens with nothing highlighted, which is what a menu
    // opened by pointer should look like.
    const { menu, chosen } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    expect(menu.handleKey(key("Enter"))).toBe(false);
    expect(chosen).toEqual([]);
    expect(menu.isOpen).toBe(true);
  });
});

describe("reopening", () => {
  it("forgets the previous menu's rows", () => {
    // One instance serves every surface, so a second `show` must not leave the
    // first surface's commands behind it.
    const { menu } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    menu.show(SELECTION_MENU, { x: 10, y: 10 });
    expect(menu.offered).toEqual(ids(SELECTION_MENU));
  });

  it("forgets the highlight too", () => {
    const { menu } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    menu.handleKey(key("ArrowDown"));
    menu.show(SELECTION_MENU, { x: 10, y: 10 });
    expect(menu.highlighted).toBe(-1);
  });
});

describe("choose", () => {
  it("ignores an index the menu does not have", () => {
    // Reachable through the harness and through a stale click: the rows are
    // rebuilt on every open, and an index is not a row.
    const { menu, chosen } = menuOf();
    menu.show(PAGE_MENU, { x: 10, y: 10 });
    menu.choose(99);
    expect(chosen).toEqual([]);
    // ...and it still closes, because the press happened.
    expect(menu.isOpen).toBe(false);
  });
});
