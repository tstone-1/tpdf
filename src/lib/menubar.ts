/**
 * The native menu bar, built from the command registry.
 *
 * ## Why this exists
 *
 * Every command tpdf has was reachable through the palette and through a
 * keyboard chord, and through nothing else. On macOS that left the menu bar
 * holding Tauri's default --- About, Hide, Quit, the web view's Cut/Copy/Paste,
 * Window --- with **no tpdf command in it at all**, so a reader who did not
 * already know ⌘\ or ⌘K had no route to the page strip, and therefore none to
 * deleting or reordering a page. `AGENTS.md` lists discoverability second among
 * three non-negotiables; a Mac reader looks in the menu bar first, and found
 * nothing there.
 *
 * ## One list, three renderings
 *
 * The menu is *generated from* {@link CommandRegistry}: a title comes from the
 * command, an accelerator from `keys.ts`, and enablement from the command's own
 * `enabled` guard. Nothing here restates what a command is or does. That is the
 * same argument `keys.ts` was extracted for --- the palette's shortcut labels
 * were hand-written beside their handlers, and ⌘O was advertised while reaching
 * no handler at all --- applied to a third reader of the same table.
 *
 * What this file *does* own is the **layout**: which command sits in which menu,
 * in what order, with separators. That is genuinely new information and has
 * nowhere else to live. {@link NOT_IN_MENU} is the other half of it, and the two
 * together are checked for completeness in `menubar.test.ts`: a command must be
 * in the layout exactly once or excluded with a reason, so adding a command and
 * forgetting the menu is a red test rather than an omission nobody sees.
 *
 * ## The menu bar takes a key before the web view does
 *
 * This is the constraint that shaped everything else. An accelerator registered
 * on a menu item is claimed by AppKit before the key reaches the page, so a menu
 * item is not a passive label --- it *moves* a shortcut out of whatever has
 * focus. Two families are therefore listed with **no** accelerator:
 *
 *  - **Bindings with no ⌘**, refused in `keys.ts`'s {@link accelerator} itself.
 *    `nav.nextPage` is bare `n`; giving that to the menu would take the letter
 *    out of the find field and out of every text input the application grows.
 *  - **Chords a text field claims anyway**, listed in {@link NO_ACCELERATOR}
 *    below. ⌘Z is the case that proves the rule: `handleWindowKey` carries an
 *    explicit `inTextField` guard on undo *because* stealing it would mean a
 *    reader correcting a typo in the find field silently undid a page rotation.
 *    A menu accelerator cannot see focus, so it would undo exactly that guard.
 *
 * Both keep working as they do today, through handlers that can see what has
 * focus. They appear in the menu for discovery, without a shortcut beside them.
 *
 * ## macOS only
 *
 * On macOS the menu bar lives outside the window and costs the reader no space,
 * which is why its emptiness was a defect. On Windows a menu bar is chrome
 * *inside* the window, and this application exists in part because the
 * alternatives put a ribbon there. The palette is that platform's route, as it
 * has been.
 */

import type { Command, CommandRegistry } from "./commands";
import { accelerator, BINDINGS, type BoundCommand } from "./keys";

/** A gap between groups of items. */
export const SEPARATOR = "---";

/** One entry in a menu: a command id, or a separator. */
export type LayoutEntry = string | typeof SEPARATOR;

/** One menu in the bar. */
export interface LayoutSection {
  /** What the menu is called. Ignored for the application menu. */
  title: string;
  items: LayoutEntry[];
  /**
   * Whether this is the application menu --- the leftmost one, named after the
   * application by the platform rather than by us.
   *
   * Its items are placed between About and Services, which is where macOS puts
   * "Check for Updates" in every application that has one. The predefined items
   * around them are the platform's and are added by the Rust side.
   */
  app?: boolean;
}

/**
 * Every menu, in bar order.
 *
 * Find is a menu of its own rather than a submenu of Edit, which is the other
 * convention. It has seven commands and this is a reader before it is an
 * editor: burying the feature people open the application for, one level down,
 * inside a menu named after something else, is the discoverability failure this
 * whole file is fixing, in miniature.
 */
export const MENU_LAYOUT: LayoutSection[] = [
  {
    title: "tpdf",
    app: true,
    items: ["app.checkForUpdates", "app.installUpdate"],
  },
  {
    title: "File",
    items: [
      "file.open",
      "file.reload",
      SEPARATOR,
      "file.saveCopy",
      "file.extractPages",
      SEPARATOR,
      "file.print",
    ],
  },
  {
    title: "Edit",
    items: [
      "edit.undo",
      "edit.redo",
      SEPARATOR,
      "edit.copy",
      SEPARATOR,
      // The three marks together, then the removal, then the selection items.
      // Grouped rather than listed in one run because "Remove mark" is the
      // opposite of the three above it and reads as a fourth kind when it sits
      // flush against them.
      "edit.highlightSelection",
      "edit.underlineSelection",
      "edit.strikeoutSelection",
      "edit.removeMark",
      SEPARATOR,
      "edit.selectAll",
      "edit.clearSelection",
    ],
  },
  {
    // A menu of its own, and the reason is the question that started this: the
    // page operations were the least reachable thing in the application, being
    // available only inside a sidebar that itself had no visible route in.
    title: "Page",
    items: [
      "edit.rotatePageClockwise",
      "edit.rotatePageCounterClockwise",
      SEPARATOR,
      "edit.movePageUp",
      "edit.movePageDown",
      SEPARATOR,
      "edit.deletePage",
    ],
  },
  {
    title: "View",
    items: [
      "view.zoomIn",
      "view.zoomOut",
      "view.zoomTo",
      SEPARATOR,
      "view.fitWidth",
      "view.fitPage",
      "view.actualSize",
      SEPARATOR,
      "view.rotateClockwise",
      "view.rotateCounterClockwise",
      SEPARATOR,
      "view.toggleSidebar",
      "view.showOutline",
      "view.showThumbnails",
      SEPARATOR,
      "view.invertPages",
    ],
  },
  {
    title: "Go",
    items: [
      "nav.nextPage",
      "nav.previousPage",
      SEPARATOR,
      "nav.firstPage",
      "nav.lastPage",
      "nav.goToPage",
      SEPARATOR,
      "nav.back",
      "nav.forward",
      SEPARATOR,
      "nav.nextLink",
      "nav.previousLink",
    ],
  },
  {
    title: "Find",
    items: [
      "find.open",
      "find.next",
      "find.previous",
      SEPARATOR,
      "find.matchCase",
      "find.wholeWord",
      "find.regex",
      "find.inSelection",
    ],
  },
];

/**
 * Commands that are deliberately not in the menu, by id prefix, with the reason.
 *
 * An exclusion has to be written down for the same reason `viewer_sweep.py`
 * makes one: a list that quietly omits things is indistinguishable from a list
 * that has fallen behind. Matched by prefix because the one entry here names a
 * group whose members are created and destroyed at runtime.
 */
export const NOT_IN_MENU: { prefix: string; reason: string }[] = [
  {
    prefix: "recent.",
    reason:
      "the recent-document list is rebuilt whenever a file is opened, and a " +
      "menu that follows it needs rebuilding with it --- File > Open Recent is " +
      "worth having and is its own piece of work",
  },
];

/**
 * Commands listed without a shortcut, and why each one.
 *
 * Every entry is a chord that a text field claims. See the note at the top: an
 * accelerator here would be taken by the menu bar before the find field could
 * see it, and each of these means something different inside a field than
 * outside one. The bindings themselves are untouched --- `handleWindowKey` and
 * the surface still match them, and they still know what has focus.
 */
export const NO_ACCELERATOR: Record<string, string> = {
  "edit.undo": "⌘Z is text undo inside the find field; handleWindowKey guards it",
  "edit.redo": "⇧⌘Z, the same guard as undo",
  "edit.copy": "⌘C copies the find field's text when the field has focus",
  "edit.selectAll": "⌘A selects the find field's text when the field has focus",
  "edit.clearSelection":
    "Esc closes the find bar, and a menu accelerator on it would fire first",
};

/** One item as the Rust side wants it. */
export type ItemSpec =
  | { kind: "separator" }
  | {
      kind: "command";
      id: string;
      title: string;
      /** Null means the item shows no shortcut. */
      accelerator: string | null;
      enabled: boolean;
    };

/** One menu as the Rust side wants it. */
export interface SectionSpec {
  title: string;
  app: boolean;
  items: ItemSpec[];
}

/**
 * Reads the registry into the shape the Rust side builds a menu from.
 *
 * A command in the layout that the registry does not have is **skipped**, not
 * substituted: a menu item that names nothing would sit there looking live and
 * do nothing when chosen. The completeness test is what turns that into a
 * failure at the time it is introduced; here it degrades to an absence.
 */
export function buildMenu(registry: CommandRegistry): SectionSpec[] {
  return MENU_LAYOUT.map((section) => ({
    title: section.title,
    app: section.app ?? false,
    items: section.items.flatMap((entry): ItemSpec[] => {
      if (entry === SEPARATOR) return [{ kind: "separator" }];
      const command = registry.find(entry);
      if (!command) return [];
      return [
        {
          kind: "command",
          id: command.id,
          title: command.title,
          accelerator: acceleratorFor(command.id),
          enabled: command.enabled?.() ?? true,
        },
      ];
    }),
  }));
}

/**
 * The accelerator a menu item may claim for a command, or null.
 *
 * Two independent refusals, and keeping them apart is the point: `keys.ts`
 * refuses a binding that holds no ⌘, which is a fact about the *binding*, and
 * {@link NO_ACCELERATOR} refuses four that do, which is a fact about what else
 * on the platform wants that chord.
 */
export function acceleratorFor(id: string): string | null {
  if (id in NO_ACCELERATOR) return null;
  const binding = BINDINGS[id as BoundCommand] as
    | (typeof BINDINGS)[BoundCommand]
    | undefined;
  return binding ? accelerator(binding) : null;
}

/** The current enabled state of every command the menu shows, by id. */
export function menuEnablement(registry: CommandRegistry): Record<string, boolean> {
  const state: Record<string, boolean> = {};
  for (const section of MENU_LAYOUT) {
    for (const entry of section.items) {
      if (entry === SEPARATOR) continue;
      const command = registry.find(entry);
      if (command) state[command.id] = command.enabled?.() ?? true;
    }
  }
  return state;
}

/** What {@link runMenuCommand} needs from the palette. */
export interface MenuPalette {
  /** Open the palette already committed to `id`, waiting for its value. */
  askFor(id: string): void;
}

/**
 * Runs what a menu item asks for.
 *
 * A command that takes an argument cannot be run from a menu --- there is
 * nowhere to type the value --- so it opens the palette in argument mode, which
 * is exactly what ⌥⌘G does for "Go to page". The menu item's title already ends
 * in an ellipsis, which is the platform's own promise that choosing it will ask
 * for something rather than act.
 *
 * Returns whether anything was reached, so a caller can tell a stale menu from a
 * command that declined to run.
 */
export function runMenuCommand(
  registry: CommandRegistry,
  palette: MenuPalette | null,
  id: string,
): boolean {
  const command: Command | undefined = registry.find(id);
  if (!command) return false;
  if (!(command.enabled?.() ?? true)) return false;
  if (command.argument) {
    if (!palette) return false;
    palette.askFor(id);
    return true;
  }
  return registry.run(id);
}
