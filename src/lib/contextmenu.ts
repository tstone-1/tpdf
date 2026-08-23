/**
 * The right-click menu, built from the command registry.
 *
 * ## Why this exists
 *
 * Right-clicking a page thumbnail offered **Reload**. Not tpdf's menu --- there
 * was no `contextmenu` listener anywhere in the application --- but WKWebView's
 * own, whose one useful entry reloads the frontend and throws away the view of
 * the document the reader was pointing at. Reported from use, and the same
 * finding as the empty menu bar one increment earlier: the page operations were
 * reachable from the palette and from a chord, and from nowhere a reader would
 * look.
 *
 * Right-clicking a thumbnail is the gesture for *"do something to this page"*.
 * It is the reason the strip is open.
 *
 * ## Derived, not written
 *
 * As with `menubar.ts`: an entry is a command id, and its title, shortcut and
 * enabled guard come from {@link CommandRegistry}. What this file owns is which
 * commands belong to which surface, which is genuinely new information.
 *
 * Unlike the menu bar there is **no completeness rule** here, and that is a
 * difference in kind rather than an omission: a menu bar covers everything the
 * application does, and a context menu is a *selection* of what makes sense for
 * the thing under the pointer. What is asserted instead is that every id in a
 * surface exists and that a surface is not empty.
 *
 * ## In the DOM, not native
 *
 * The menu bar is AppKit's because a menu bar is; this is a `<div>`. Three
 * reasons, and the third is what settled it: it works on both platforms with one
 * implementation, it can show the same shortcut labels the palette does, and the
 * window harness can drive it --- a native popup is outside the page and
 * `viewer_check.py` could not see it at all.
 */

import type { Command, CommandRegistry } from "./commands";

/** A gap between groups. */
export const SEPARATOR = "---";

/** One entry: a command id, or a separator. */
export type Entry = string | typeof SEPARATOR;

/**
 * The page operations, for a right-click on a thumbnail in the page strip.
 *
 * Rotate before move before delete: least destructive first, which is the order
 * the Page menu uses and the order a reader scans in.
 *
 * `file.extractPages` is deliberately absent. It takes a value, so choosing it
 * would open the palette --- correct behaviour, and a strange thing for a menu
 * that appeared *because* the reader pointed at one specific page. Extracting
 * the page under the pointer is worth having and is a command that does not
 * exist yet.
 */
export const PAGE_MENU: Entry[] = [
  "edit.rotatePageClockwise",
  "edit.rotatePageCounterClockwise",
  SEPARATOR,
  "edit.movePageUp",
  "edit.movePageDown",
  SEPARATOR,
  "edit.deletePage",
];

/**
 * What a right-click on one of the reader's own marks offers.
 *
 * A right-click on a highlight used to offer {@link SELECTION_MENU} --- copy,
 * the three marks, find --- which is a menu about the *selection*, and on a
 * mark there usually is not one. So the only way to take a mark off was to
 * left-press it, wait for its note box, and choose Remove mark from a menu the
 * reader had to know was elsewhere. Reported from use.
 *
 * **One entry, and the other two things a mark can be asked are not missing.**
 * The kind cannot be changed --- there is no command for it. The note and the
 * colour are both edited in the box that opens *with* this menu: the swatch row
 * is the first thing in it, so listing the seven `edit.color.*` commands here
 * would put a second surface under the reader's pointer for a choice already
 * under their eyes, and make this the padded-out menu the one it replaced was.
 * (That sentence read "one entry and that is the whole of what a mark can be
 * asked to do" until the swatch row existed, which is why it is spelled out
 * rather than shortened: the claim was true and stopped being.)
 *
 * **How it knows which mark.** It does not address one: `App.svelte` opens the
 * mark's note first and then shows this, so `edit.removeMark` acts on the mark
 * whose note is open, exactly as it does from the palette and the menu bar.
 * That is the arrangement the page strip already uses --- a right-click on a
 * thumbnail navigates to that page and then offers the page operations --- and
 * it is the reason there is still no second way to name a mark, which
 * `edit.removeMark`'s own note in `appcommands.ts` argues against at length.
 */
export const MARK_MENU: Entry[] = ["edit.removeMark"];

/**
 * What a right-click on the document surface offers.
 *
 * Copy first, because that is what a right-click on selected text is for
 * everywhere else, and the three marks straight after it: those are the things
 * a reader does with a run of text they have just dragged across, and this menu
 * is the shortest route to them, since none of the three has a chord. The find
 * entry is here rather than in a menu of its own because searching *within what
 * you just selected* is the one search option whose subject is the selection.
 *
 * All three rather than the highlight alone. A right-click menu earns its
 * length by being the place a reader does not have to go looking, and offering
 * one of three kinds here sends them to the menu bar for the other two --- which
 * is the trip this menu exists to save.
 *
 * An entry whose command is not available right now is left out rather than
 * greyed, so with nothing selected this menu is Select all and Find --- the
 * marks simply are not offered.
 */
export const SELECTION_MENU: Entry[] = [
  // First, and the only entry here that does not need a selection --- so on
  // white paper with nothing selected this menu is Add comment, Select all and
  // Find. That is deliberate: dropping a comment is the one thing a reader
  // wants from a right-click on an empty part of the page, and before it
  // existed such a click produced a menu offering nothing that applied.
  "edit.addComment",
  // Beside the comment, because the two are the pair that need no selection ---
  // so a right-click on white paper offers both of the things a reader can do
  // to blank space. It is also the only route to the tool a reader will find
  // without being told, since it has no chord and the menu bar is macOS-only.
  "edit.drawBox",
  // With the box, for its reason exactly: this is also a thing a reader can do
  // to blank paper, and also has no chord, so a right-click is the only route
  // to it that anyone will find without being told.
  "edit.draw",
  // And the eraser with them: a reader right-clicks on the mark they want rid
  // of, which is the gesture that puts the pointer where the tool is needed ---
  // and since the nib takes any mark rather than only a drawing's strokes, the
  // pointer is already on the thing the next sweep will take.
  "edit.erase",
  "edit.copy",
  "edit.highlightSelection",
  "edit.underlineSelection",
  "edit.strikeoutSelection",
  "edit.selectAll",
  SEPARATOR,
  "find.inSelection",
  SEPARATOR,
  "edit.clearSelection",
];

/**
 * Which menu a right-click on the document surface gets.
 *
 * A function rather than an `if` in `App.svelte`, because that file has no
 * tests and the window harness builds its own DOM without a toolbar or a
 * context-menu host --- so a rule written there is reachable by nothing, which
 * is the shape this repository's traps call for a seam rather than a harness.
 * What stays untested is only that the handler asks the viewer and passes the
 * answer along.
 *
 * The mark wins when there is one. A reader who right-clicks a highlight is
 * asking about that highlight, and a selection they made earlier is not what
 * the pointer is on.
 */
export function menuForSurface(markUnderPointer: number | null): Entry[] {
  return markUnderPointer === null ? SELECTION_MENU : MARK_MENU;
}

/** Where a menu was asked for, in client coordinates. */
export interface At {
  x: number;
  y: number;
}

/** One row as the menu draws it. */
interface Row {
  command: Command;
  element: HTMLElement;
}

/**
 * A right-click menu over a command registry.
 *
 * One instance serves every surface: {@link open} takes the entries, so the
 * strip's menu and the document's menu are the same object with different
 * lists. Two instances would be two things to close when the other opens.
 */
export class ContextMenu {
  private readonly root: HTMLElement;
  private rows: Row[] = [];
  private at = -1;
  /** Where the open menu was asked for, for {@link onRun}. */
  private openedAt: At | null = null;
  private open_ = false;
  /**
   * Run after a command is chosen, with where the menu was opened.
   *
   * The point is here because the menu is the only thing that knows it, and one
   * command needs it: `edit.addComment` places a bubble, and *where* is exactly
   * what a right-click contributes over the palette. Passing it is the same
   * arrangement the page strip uses --- a right-click establishes what the
   * command acts on --- stated as an argument rather than left in application
   * state that a later palette invocation could read stale.
   *
   * Every other command ignores it. A menu opened by the keyboard, if one ever
   * is, would pass `null` and get the palette's behaviour, which is right.
   */
  private readonly onRun: (id: string, at: At | null) => void;

  constructor(
    private readonly host: HTMLElement,
    private readonly registry: CommandRegistry,
    onRun: (id: string, at: At | null) => void,
  ) {
    this.onRun = onRun;
    this.root = document.createElement("div");
    this.root.setAttribute("role", "menu");
    this.root.className = "context-menu";
    // Inline, as `palette.ts` styles itself and for the same reason: this
    // element is appended to `document.body`, where a Svelte component's scoped
    // styles do not reach it. The system colour keywords are what make it follow
    // the desktop's light and dark themes with nothing to switch.
    this.root.style.cssText =
      "position:fixed;z-index:60;min-width:14rem;padding:0.25rem 0;display:none;" +
      "border-radius:8px;background:Canvas;color:CanvasText;" +
      // A hairline ring and a short shadow, not a long dark one. This was
      // `0 8px 32px rgba(0,0,0,0.32)` --- heavier than the comment popups' at
      // `0 6px 24px / 0.25` while being the smallest and shortest-lived surface
      // in the application, and reported from use on Windows as "VERY dark and
      // large and shadowy". A context menu is not elevated the way the palette
      // is; it is attached to the thing under the pointer, and what makes it
      // read as a menu at that elevation is the ring rather than the shadow.
      // The ring is `currentColor`-derived, as the page strip's selection is,
      // so it follows both themes without a second literal to keep in step.
      "box-shadow:0 4px 12px rgba(0,0,0,0.16)," +
      "0 0 0 1px color-mix(in srgb, currentColor 15%, transparent);" +
      "font:13px/1.5 system-ui,-apple-system,sans-serif;";
    this.host.appendChild(this.root);
    // On the host rather than on each row: a pointer routinely leaves the row
    // it went down on, and the menu is rebuilt on every open.
    this.root.addEventListener("mousedown", (event) => event.preventDefault());
  }

  /** Whether a menu is on screen. */
  get isOpen(): boolean {
    return this.open_;
  }

  /** The command ids currently offered, in order. For tests and the harness. */
  get offered(): string[] {
    return this.rows.map((row) => row.command.id);
  }

  /** The index of the highlighted row, or -1. */
  get highlighted(): number {
    return this.at;
  }

  /**
   * Shows `entries` at `at`, dropping anything the registry does not have.
   *
   * **A command whose guard is closed is left out entirely rather than greyed.**
   * That is the opposite of the menu bar's choice, and deliberate: a menu bar is
   * a stable map of the application, so an item vanishing from it would make the
   * map move under the reader. A context menu is built fresh for one click and
   * has no such continuity to protect --- and a short menu of things that work
   * is easier to use than a long one that is mostly grey.
   *
   * Returns whether anything was shown. An empty menu is not opened: a menu with
   * no entries reads as the application being broken rather than as there being
   * nothing to do.
   */
  show(entries: Entry[], at: At): boolean {
    this.close();
    const rows: Row[] = [];
    let pendingSeparator = false;
    for (const entry of entries) {
      if (entry === SEPARATOR) {
        // Recorded rather than drawn, so a separator whose whole group was
        // withheld does not leave a rule with nothing under it.
        pendingSeparator = rows.length > 0;
        continue;
      }
      const command = this.registry.find(entry);
      if (!command || !(command.enabled?.() ?? true)) continue;
      if (pendingSeparator) {
        this.root.appendChild(this.rule());
        pendingSeparator = false;
      }
      const element = this.row(command, rows.length);
      this.root.appendChild(element);
      rows.push({ command, element });
    }
    if (rows.length === 0) return false;

    this.rows = rows;
    this.open_ = true;
    this.openedAt = at;
    this.root.style.display = "block";
    this.place(at);
    this.highlight(-1);
    return true;
  }

  /** Hides the menu and forgets what it was showing. */
  close(): void {
    this.root.style.display = "none";
    this.root.replaceChildren();
    this.rows = [];
    this.at = -1;
    this.open_ = false;
    // Cleared with everything else, so a command run from the palette after a
    // menu has been dismissed cannot read where that menu once was.
    this.openedAt = null;
  }

  /**
   * Handles a key while the menu is open, answering whether it was consumed.
   *
   * Taken as a method rather than a listener of its own, so the caller decides
   * where in its own key handling this sits --- the menu must see Escape before
   * the find bar does, and must not see anything at all when it is closed.
   */
  handleKey(event: KeyboardEvent): boolean {
    if (!this.open_) return false;
    if (event.key === "Escape") {
      this.close();
      return true;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      const count = this.rows.length;
      // From -1, Down lands on the first and Up on the last, which is what a
      // menu opened by keyboard does everywhere else.
      const next = this.at < 0 ? (step > 0 ? 0 : count - 1) : (this.at + step + count) % count;
      this.highlight(next);
      return true;
    }
    if (event.key === "Enter" && this.at >= 0) {
      this.choose(this.at);
      return true;
    }
    return false;
  }

  /** Runs the row at `index` and closes. */
  choose(index: number): void {
    const row = this.rows[index];
    const opened = this.openedAt;
    this.close();
    if (!row) return;
    // Read before `close`, which clears it --- and `close` runs first above so
    // that a command opening something of its own does not fight a menu that is
    // still on screen.
    this.onRun(row.command.id, opened);
  }

  /** Drops the element. The menu is gone with its host, but say so. */
  destroy(): void {
    this.close();
    this.root.remove();
  }

  private row(command: Command, index: number): HTMLElement {
    const element = document.createElement("div");
    element.setAttribute("role", "menuitem");
    element.className = "context-menu-item";
    element.style.cssText =
      "display:flex;align-items:center;gap:1.5rem;padding:0.3rem 0.9rem;" +
      "cursor:default;white-space:nowrap;";
    const title = document.createElement("span");
    title.style.flex = "1";
    // The command's own title. Not a copy of it -- see the note at the top.
    title.textContent = command.title;
    element.appendChild(title);
    if (command.keys !== undefined) {
      const keys = document.createElement("span");
      keys.className = "context-menu-keys";
      keys.style.cssText = "opacity:0.55;font-variant-numeric:tabular-nums;";
      keys.textContent = command.keys;
      element.appendChild(keys);
    }
    element.addEventListener("mouseenter", () => this.highlight(index));
    element.addEventListener("click", () => this.choose(index));
    return element;
  }

  private rule(): HTMLElement {
    const rule = document.createElement("div");
    rule.className = "context-menu-rule";
    rule.setAttribute("role", "separator");
    rule.style.cssText =
      "margin:0.25rem 0.5rem;height:1px;" +
      "background:color-mix(in srgb, currentColor 18%, transparent);";
    return rule;
  }

  private highlight(index: number): void {
    this.at = index;
    this.rows.forEach((row, i) => {
      const on = i === index;
      row.element.classList.toggle("is-highlighted", on);
      row.element.style.background = on
        ? "color-mix(in srgb, currentColor 12%, transparent)"
        : "";
    });
  }

  /**
   * Puts the menu where the pointer is, inside the window.
   *
   * Measured after it is displayed, because a hidden element has no size --- and
   * flipped rather than clamped when it would overflow, so a menu opened near
   * the bottom of the strip does not cover the row it was opened on.
   */
  private place(at: At): void {
    const box = this.root.getBoundingClientRect();
    const room = { w: window.innerWidth, h: window.innerHeight };
    const x = at.x + box.width > room.w ? Math.max(0, at.x - box.width) : at.x;
    const y = at.y + box.height > room.h ? Math.max(0, at.y - box.height) : at.y;
    this.root.style.left = `${x}px`;
    this.root.style.top = `${y}px`;
  }
}
