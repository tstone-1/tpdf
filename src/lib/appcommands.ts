/**
 * The command list the application actually has, and the window shortcuts.
 *
 * This was inside `App.svelte`, and `docs/PLAN.md` recorded the consequence as a
 * gap: *"Cmd-K itself and the command list `App.svelte` registers are covered by
 * nothing."* The palette check built its own four-command registry, so what it
 * proved was that the palette works --- not that any command a reader can type
 * is wired to anything. The same sentence `palette.ts` opens with says why: a
 * component that only exists inside `App.svelte` is a component the check cannot
 * reach, because `viewercheck.ts` runs *instead of* the shell booting.
 *
 * So the list moved out to where the check can reach it. Nothing about it
 * changed on the way --- the commands, their titles, their `enabled` guards and
 * their argument validators are the ones that were there, and `App.svelte` calls
 * {@link registerAppCommands} where it used to call `register` itself.
 *
 * ## What the seam is, and what it costs
 *
 * A command's `run` reaches one of two places. Most reach the {@link Viewer},
 * which the check owns a real one of, so those are end-to-end: type "fit width",
 * press Enter, and the viewer the reader would be looking at changes. The rest
 * reach the shell --- a file dialog, a print panel, a Svelte `$state` flag for
 * the sidebar --- and there is no shell in the check. Those arrive through
 * {@link AppActions}, which the check implements with recorders.
 *
 * Be precise about what that buys. For a shell action the check asserts the
 * *wiring* --- that this command, found by the name a reader types, reaches that
 * action exactly once --- and says nothing about what the action then does.
 * `toggleInvert` writing the preference to the session is not covered here and
 * is not claimed to be. The wiring is the half that was covered by nothing.
 */

import type { CommandRegistry } from "./commands";
import { BINDINGS, label, matches, type BoundCommand } from "./keys";
import { describeRange, parsePageRange } from "./pageranges";
import type { Tab } from "./sidebar";
import type { Viewer } from "./viewer";
import { MAX_ZOOM, MIN_ZOOM, parseZoomPercent, percentOf } from "./zoom";

/**
 * Everything a command needs that is not the viewer.
 *
 * Deliberately not "the app": each member is one verb, so a check can record
 * which of them a command reached. A single `app` object with the whole
 * component behind it would make the seam untestable again.
 */
export interface AppActions {
  /**
   * The open document's surface, or null.
   *
   * A function rather than a value because the registry outlives any document:
   * it is read at call time, so closing and opening one does not need the
   * commands rebuilt.
   */
  viewer(): Viewer | null;
  /** How many pages the open document has, for the "Go to page" validator. */
  pageCount(): number;
  /** Ask for a file and open it. */
  openDocument(): void;
  /** Open the current document's path again, keeping the reader's place. */
  reloadDocument(): void;
  /**
   * Whether an open is already in flight.
   *
   * Only the keyboard route consults it, which is how it was before this moved:
   * the Open *button* carries the same guard as `disabled`, and the palette row
   * carries none. Preserved rather than unified --- making the palette guard too
   * would be a behaviour change nobody asked for while moving code.
   */
  busyOpening(): boolean;
  /** Hand the document to the platform print panel. */
  printDocument(): void;
  /** Put the caret in the find field. */
  focusFind(): void;
  /** Flip one matching option and rescan. */
  toggleSearchOption(which: "matchCase" | "wholeWord" | "regex"): void;
  /** Confine the search to the selection, or let it see the document again. */
  toggleSearchScope(): void;
  /** Show or hide the sidebar. */
  toggleSidebar(): void;
  /** Show one of the sidebar's tabs. */
  showTab(tab: Tab): void;
  /** Invert the page colours, and remember it. */
  toggleInvert(): void;
  /** Ask the update endpoint whether there is a newer tpdf. */
  checkForUpdates(): void;
  /** Download and apply the update the last check found. */
  applyUpdate(): void;
  /**
   * Whether an update is downloaded and waiting for a relaunch.
   *
   * Read by the two update commands' `enabled` guards, so that "Install update"
   * is offered only when there is one and is withdrawn once it is applied ---
   * the two states are different and a single "is there an update" flag would
   * leave the command live after it had already run.
   */
  updateReady(): boolean;
  /** Whether the last check found an update that has not been applied. */
  updateAvailable(): boolean;
  /**
   * Turn the page the reader is on by `delta` quarter-turns, in the document.
   *
   * A shell action rather than a viewer method, because the model that decides
   * what a page's turn becomes is in the backend: the viewer is *told* the
   * answer. See `edits.ts`.
   */
  rotatePage(delta: number): void;
  deletePage(): void;
  /**
   * Move the page the reader is on `delta` slots along, in the document.
   *
   * A signed step rather than a pair of commands, so that the two palette
   * entries are one action with two arguments --- the same shape `rotatePage`
   * has, and for the same reason: two entry points into one call is where the
   * second one drifts.
   */
  movePage(delta: number): void;
  /** Step the edit journal back one command. */
  undoEdit(): void;
  /** Step the edit journal forward one command. */
  redoEdit(): void;
  /** Whether there is an edit to undo. */
  canUndo(): boolean;
  /** Whether there is an edit to redo. */
  canRedo(): boolean;
  /** Ask for a name and write the working document to it. */
  saveCopy(): void;
  /** Ask for a name and write the pages at `slots` to it, as a second file. */
  extractPages(slots: number[]): void;
}

/**
 * Registers every command the application has.
 *
 * Appends to whatever the registry already holds, so the recent-document group
 * that `recents.ts` maintains is unaffected by the order these are added in.
 */
export function registerAppCommands(
  registry: CommandRegistry,
  actions: AppActions,
): void {
  /**
   * Every command the application has, in one place.
   *
   * Built once and outliving any document, so the palette works before a file
   * is open --- "Open document" is the command someone reaches for first. The
   * rest guard on `viewer`, which is read at call time rather than captured, so
   * closing and opening a document does not need the registry rebuilt.
   *
   * The `keys` strings are labels the palette displays, and they are **derived**
   * from the same table `viewer.ts`'s key handler and {@link handleWindowKey} match
   * against --- see `keys.ts`. They were hand-written beside those handlers with
   * nothing checking the two agreed, and the gap was not hypothetical: ⌘O was
   * advertised and reached no handler at all, and ⌘P turned the page as well as
   * printing, because the viewer's `p` arm tested the key without the modifier.
   */
  const withDocument = () => actions.viewer() !== null;

  registry.register(
    {
      id: "file.open",
      title: "Open document",
      keys: label("file.open"),
      run: () => actions.openDocument(),
    },
    {
      // No keyboard binding, and not an oversight. ⌘R is already the rotate
      // chord, and moving a binding a reader already has is worse than a
      // command reaching the palette alone --- which is the shape this
      // application is built around. "Show outline" and "Show page thumbnails"
      // have none either.
      //
      // It exists because the backend now tells a reader whose file was
      // truncated to open it again, and until this there was no way to do that
      // except ⌘O and re-picking the file. It is useful either way: a document
      // rewritten in the background is not picked up, so this is how you ask for
      // the new one.
      id: "file.reload",
      title: "Reload from disk",
      enabled: withDocument,
      run: () => actions.reloadDocument(),
    },
    {
      // No binding, same reasoning as the two above: a command nobody presses
      // by accident belongs in the palette. Always enabled, deliberately ---
      // asking again after a failed check is the only way back, and a guard
      // that withheld it while `failed` would strand the reader on a launch
      // that happened to have no network.
      id: "app.checkForUpdates",
      title: "Check for updates",
      run: () => actions.checkForUpdates(),
    },
    {
      // Two guards rather than one, because "there is an update" and "it is
      // already applied" are different states and this must be withdrawn in the
      // second. Applying twice is not harmful, but a command that stays live
      // after it has run says the first run did not work.
      id: "app.installUpdate",
      title: "Install update and restart",
      enabled: () => actions.updateAvailable() && !actions.updateReady(),
      run: () => actions.applyUpdate(),
    },
    {
      // No page-range field of our own, deliberately: the system panel has one,
      // and its numbers refer to the document we hand over --- which is every
      // page, so they mean what the reader thinks they mean. `print::build`
      // takes a range because thumbnail-selection printing will need it, not
      // because anything asks for one today.
      id: "file.print",
      title: "Print",
      keys: label("file.print"),
      enabled: withDocument,
      run: () => actions.printDocument(),
    },
    {
      id: "find.open",
      title: "Find in document",
      keys: label("find.open"),
      enabled: withDocument,
      run: () => actions.focusFind(),
    },
    {
      id: "find.next",
      title: "Find next",
      keys: label("find.next"),
      enabled: withDocument,
      run: () => actions.viewer()?.nextMatch(),
    },
    {
      id: "find.previous",
      title: "Find previous",
      keys: label("find.previous"),
      enabled: withDocument,
      run: () => actions.viewer()?.prevMatch(),
    },
    {
      // Titled by what pressing it does, not by what the setting is called: a
      // palette lists verbs, and "Match case" beside a checkbox that is already
      // on reads as a description of the state rather than as a command.
      id: "find.matchCase",
      title: "Find: match case on or off",
      keys: label("find.matchCase"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("matchCase"),
    },
    {
      id: "find.wholeWord",
      title: "Find: whole words on or off",
      keys: label("find.wholeWord"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("wholeWord"),
    },
    {
      // Enabled on having something to scope *to*, which is what makes a
      // silently-does-nothing command unnecessary: a reader with no selection
      // sees it greyed, with the reason being that nothing is selected. It
      // stays enabled while scoped so there is a way back out.
      id: "find.inSelection",
      title: "Find: in selection on or off",
      keys: label("find.inSelection"),
      enabled: () => {
        const viewer = actions.viewer();
        return (
          viewer !== null && (viewer.searchScoped || viewer.selectedText !== "")
        );
      },
      run: () => actions.toggleSearchScope(),
    },
    {
      // Titled by what it turns on rather than by the word "regex", which is
      // what a reader who wants it would type and not what a reader who does
      // not would understand. Both spellings are in the title so the palette
      // finds it either way.
      id: "find.regex",
      title: "Find: regular expression on or off",
      keys: label("find.regex"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("regex"),
    },
    {
      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
      enabled: withDocument,
      run: () => actions.viewer()?.zoomStep(1),
    },
    {
      id: "view.zoomOut",
      title: "Zoom out",
      keys: label("view.zoomOut"),
      enabled: withDocument,
      run: () => actions.viewer()?.zoomStep(-1),
    },
    {
      id: "view.fitWidth",
      title: "Fit width",
      keys: label("view.fitWidth"),
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("width"),
    },
    {
      id: "view.fitPage",
      title: "Fit page",
      keys: label("view.fitPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("page"),
    },
    {
      // 100% means one CSS pixel per PDF point, which is not the same as one
      // inch on the desk --- that would need the display's physical size, which
      // no browser reports honestly. Every other reader calls this number
      // "actual size" and means the same thing by it.
      id: "view.actualSize",
      title: "Actual size",
      keys: label("view.actualSize"),
      enabled: withDocument,
      run: () => actions.viewer()?.setZoomFixed(1),
    },
    {
      // The second command that takes a value. The zoom ladder is deliberately
      // coarse --- each stop throws away every tile --- so a reader who wants
      // 175% cannot step to it, and before this there was nothing to type it
      // into either.
      id: "view.zoomTo",
      title: "Zoom to…",
      keys: label("view.zoomTo"),
      enabled: withDocument,
      argument: {
        placeholder: "Zoom, in percent",
        problem: (raw: string) => {
          if (raw.trim() === "")
            return `Zoom, ${percentOf(MIN_ZOOM)} to ${percentOf(MAX_ZOOM)}%`;
          if (parseZoomPercent(raw) === null) {
            return `"${raw.trim()}" is not a zoom between ${percentOf(MIN_ZOOM)}% and ${percentOf(MAX_ZOOM)}%`;
          }
          return null;
        },
        preview: (raw: string) =>
          `Zoom to ${percentOf(parseZoomPercent(raw) ?? 1)}%`,
        run: (raw: string) => {
          const zoom = parseZoomPercent(raw);
          if (zoom !== null) actions.viewer()?.setZoomFixed(zoom);
        },
      },
    },
    {
      // Preview's bindings, not Acrobat's: Acrobat rotates on Shift-Cmd-+ and
      // Shift-Cmd-−, and on this keyboard those produce the same `key` as the
      // zoom shortcuts they would then collide with.
      //
      // Rotating the *view* only. The file is untouched; rotating pages in the
      // document is a page operation and belongs with the ones that write.
      id: "view.rotateClockwise",
      title: "Rotate view clockwise",
      keys: label("view.rotateClockwise"),
      enabled: withDocument,
      run: () => actions.viewer()?.rotateBy(1),
    },
    {
      id: "view.rotateCounterClockwise",
      title: "Rotate view anticlockwise",
      keys: label("view.rotateCounterClockwise"),
      enabled: withDocument,
      run: () => actions.viewer()?.rotateBy(-1),
    },
    {
      // ⇧⌘R beside ⌘R, because these two are the same gesture on two different
      // subjects and a reader who knows one should be able to guess the other.
      // The titles carry the distinction that matters --- "view" against
      // "page" --- since the shortcut cannot.
      id: "edit.rotatePageClockwise",
      title: "Rotate page clockwise",
      keys: label("edit.rotatePageClockwise"),
      enabled: withDocument,
      run: () => actions.rotatePage(1),
    },
    {
      id: "edit.rotatePageCounterClockwise",
      title: "Rotate page anticlockwise",
      keys: label("edit.rotatePageCounterClockwise"),
      enabled: withDocument,
      run: () => actions.rotatePage(-1),
    },
    {
      // **No keyboard binding, and that is the decision rather than an
      // omission.** Every other page operation has one, and this is the only
      // command in the application that removes something a reader can see. It
      // is undoable, which is the argument for a chord --- and a mis-pressed
      // chord that silently removes a page from a document somebody is halfway
      // through reading is a worse first experience than one extra keystroke. It
      // is two keystrokes away in the palette, which is what `docs/PLAN.md` asks
      // of every command.
      id: "edit.deletePage",
      title: "Delete page",
      enabled: withDocument,
      run: () => actions.deletePage(),
    },
    {
      // No binding either, and for a different reason than the deletion above:
      // there is no chord left that reads as "move a page" rather than as "move
      // the view", and a reader who wants to rearrange a document is going to
      // want the page strip anyway. `docs/PLAN.md` has dragging as the half this
      // increment did not build; these two are the command it will call.
      //
      // Off either end does nothing rather than wrapping. A reader who holds a
      // key down expects a page to stop at the end of a document, and a page
      // that reappeared at the other end would be a deletion as far as the eye
      // is concerned.
      id: "edit.movePageUp",
      title: "Move page up",
      enabled: withDocument,
      run: () => actions.movePage(-1),
    },
    {
      id: "edit.movePageDown",
      title: "Move page down",
      enabled: withDocument,
      run: () => actions.movePage(1),
    },
    {
      // Guarded on there being something to undo rather than merely on a
      // document being open. A palette that offers Undo with an empty journal
      // teaches a reader that the command does nothing, which is the same
      // lesson a broken one teaches.
      id: "edit.undo",
      title: "Undo",
      keys: label("edit.undo"),
      enabled: () => actions.viewer() !== null && actions.canUndo(),
      run: () => actions.undoEdit(),
    },
    {
      id: "edit.redo",
      title: "Redo",
      keys: label("edit.redo"),
      enabled: () => actions.viewer() !== null && actions.canRedo(),
      run: () => actions.redoEdit(),
    },
    {
      // Offered on any open document, not only an edited one. Saving an
      // unedited copy is a thing readers do --- it is how you get a file out of
      // a downloads folder --- and a command that appears only after an edit is
      // one nobody finds.
      id: "file.saveCopy",
      title: "Save a copy...",
      keys: label("file.saveCopy"),
      enabled: withDocument,
      run: () => actions.saveCopy(),
    },
    {
      // Takes a selection where `file.saveCopy` takes none, and is otherwise
      // the same operation over fewer pages --- it shares the whole write path,
      // so an encrypted document is refused here for the reason it is refused
      // there.
      //
      // The parse runs twice, in `problem` and again in `run`, and that is
      // deliberate: `parsePageRange` is pure and cheap, and the alternative is
      // a validated value cached between two callbacks that the palette is free
      // to invoke in either order. Two calls to one function cannot disagree;
      // a cache and its writer can.
      id: "file.extractPages",
      title: "Extract pages...",
      // No shortcut, for the reason the two move commands have none: there is
      // no chord left that reads as "extract", and this is a command a reader
      // reaches deliberately rather than by muscle memory.
      enabled: withDocument,
      argument: {
        placeholder: "Pages, e.g. 1-3,5",
        problem: (raw: string) =>
          parsePageRange(raw, actions.pageCount()).problem ?? null,
        preview: (raw: string) => {
          const range = parsePageRange(raw, actions.pageCount());
          return range.slots ? describeRange(range.slots) : "";
        },
        run: (raw: string) => {
          const range = parsePageRange(raw, actions.pageCount());
          // Cannot be reached through the palette, which refuses to run a
          // command whose `problem` answered. Written as a guard rather than a
          // `!` because the two callbacks are independent entry points and this
          // one is what actually writes a file.
          if (!range.slots) return;
          actions.extractPages(range.slots);
        },
      },
    },
    {
      id: "nav.nextPage",
      title: "Next page",
      keys: label("nav.nextPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.nextPage(),
    },
    {
      id: "nav.previousPage",
      title: "Previous page",
      keys: label("nav.previousPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.previousPage(),
    },
    {
      // Back exists because following a cross-reference without one is a trap:
      // it moves a reader into the middle of something and leaves them to
      // remember the page number they came from. It records positions rather
      // than links, so a jump from the outline or a search result is on the
      // same stack --- which is what a reader who has used a browser expects.
      id: "nav.back",
      title: "Back",
      keys: label("nav.back"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.goBack();
      },
    },
    {
      id: "nav.forward",
      title: "Forward",
      keys: label("nav.forward"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.goForward();
      },
    },
    {
      // The only way to reach a link without a pointer. Until this existed a
      // reader on the keyboard could move by page, heading and search hit and
      // could not follow a cross-reference at all -- which on a document whose
      // table of contents is its navigation is most of the document.
      id: "nav.nextLink",
      title: "Next link",
      keys: label("nav.nextLink"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepLink(1);
      },
    },
    {
      id: "nav.previousLink",
      title: "Previous link",
      keys: label("nav.previousLink"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepLink(-1);
      },
    },
    {
      id: "nav.firstPage",
      title: "Go to start",
      keys: label("nav.firstPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.goToStart(),
    },
    {
      id: "nav.lastPage",
      title: "Go to end",
      keys: label("nav.lastPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.goToEnd(),
    },
    {
      // The first command that takes a value. On a 775-page document there was
      // no way to reach page 400 at all: Home, End and one page at a time.
      //
      // Numbers here are the ones printed on the page --- one-based --- and
      // `goToPage` is zero-based, so the conversion happens once, here, next to
      // the text that says "of {pageCount}". `problem` refuses out of range
      // rather than clamping: a reader who types 900 in a 775-page document has
      // made a mistake, and silently going to the last page hides it.
      id: "nav.goToPage",
      title: "Go to page…",
      keys: label("nav.goToPage"),
      enabled: withDocument,
      argument: {
        placeholder: "Page number",
        problem: (raw: string) => {
          const pages = actions.pageCount();
          const trimmed = raw.trim();
          if (trimmed === "") return `Page number, 1 to ${pages}`;
          if (!/^[0-9]+$/.test(trimmed))
            return `"${trimmed}" is not a page number`;
          const page = Number(trimmed);
          if (page < 1 || page > pages) {
            return `This document has ${pages} page${pages === 1 ? "" : "s"}`;
          }
          return null;
        },
        preview: (raw: string) =>
          `Go to page ${Number(raw.trim())} of ${actions.pageCount()}`,
        run: (raw: string) =>
          actions.viewer()?.goToPage(Number(raw.trim()) - 1),
      },
    },
    {
      id: "edit.selectAll",
      title: "Select all on page",
      keys: label("edit.selectAll"),
      enabled: withDocument,
      run: () => actions.viewer()?.selectPage(),
    },
    {
      id: "edit.copy",
      title: "Copy selection",
      keys: label("edit.copy"),
      enabled: withDocument,
      run: () => void actions.viewer()?.copySelection(),
    },
    {
      id: "edit.clearSelection",
      title: "Clear selection",
      keys: label("edit.clearSelection"),
      enabled: withDocument,
      run: () => actions.viewer()?.clearSelection(),
    },
    {
      // Named for both things a reader might type. One command with one
      // binding rather than two commands sharing one, which would show the
      // same shortcut twice in the palette and teach that it does two things.
      id: "view.toggleSidebar",
      title: "Toggle sidebar",
      keys: label("view.toggleSidebar"),
      enabled: withDocument,
      run: () => actions.toggleSidebar(),
    },
    {
      // Two commands rather than one "switch tab", because the palette is how a
      // command is *found*: someone looking for thumbnails types "thumb", and a
      // command called "Switch sidebar tab" is not what they would type.
      id: "view.showOutline",
      title: "Show outline",
      enabled: withDocument,
      run: () => actions.showTab("outline"),
    },
    {
      id: "view.showThumbnails",
      title: "Show page thumbnails",
      enabled: withDocument,
      run: () => actions.showTab("pages"),
    },
    {
      // "Invert page colours", not "Dark mode". The chrome is already dark when
      // the desktop is, so a command called dark mode would appear to do nothing
      // for the reader who most expects it to --- and what this actually does is
      // change how the document looks, which is worth saying out loud.
      id: "view.invertPages",
      title: "Invert page colours",
      keys: label("view.invertPages"),
      enabled: withDocument,
      run: () => actions.toggleInvert(),
    },
  );
}

/** The part of the palette the window shortcuts drive. */
export interface PaletteLike {
  readonly isOpen: boolean;
  open(): void;
  close(): void;
  askFor(id: string): void;
}

/**
 * Re-renders every command's advertised shortcut from the binding table.
 *
 * Called once, after the platform has said what this keyboard prints --- see
 * `keys.ts`'s {@link setPrintedKeys} and `src-tauri/src/keylayout.rs`. A command
 * registers with the label that can be rendered synchronously, which for a
 * binding naming a physical key is the character it declares; this replaces that
 * with the key the reader can actually see on the keyboard in front of them.
 *
 * Mutating the registered commands rather than rebuilding the registry, because
 * a rebuild would drop the recent-command list and re-run every registration for
 * a change that touches one string. A command with no binding is left alone: its
 * `keys` is undefined and the palette shows no shortcut, which is right.
 */
export function relabelCommands(registry: CommandRegistry): void {
  for (const command of registry.all()) {
    if (command.id in BINDINGS) command.keys = label(command.id as BoundCommand);
  }
}

/** What {@link handleWindowKey} reaches for. */
export interface WindowKeyDeps {
  actions: AppActions;
  /** The palette, or null before the shell has built one. */
  palette(): PaletteLike | null;
  /**
   * Whether a document is open.
   *
   * `App.svelte` answered this with the window title, and every binding but two
   * is gated on it. Kept distinct from `actions.viewer() !== null`, which is
   * what a command's `enabled` asks: they agree in practice and they are not the
   * same question, and collapsing them here would be a change disguised as a
   * move.
   */
  hasDocument(): boolean;
  /** Re-read the recent-document list behind an opening palette. */
  refreshRecents(): void;
}

/**
 * The shortcuts that belong to the window rather than to the surface.
 *
 * Matched through `keys.ts`, which is where the palette's labels come from
 * too --- see the note there. ⌘K is the one chord not in that table: it opens
 * the palette rather than being listed in it, so there is no label for it to
 * disagree with.
 */
export function handleWindowKey(
  event: KeyboardEvent,
  deps: WindowKeyDeps,
): void {
  const palette = deps.palette();
  const { actions } = deps;
  const title = deps.hasDocument();

  if ((event.metaKey || event.ctrlKey) && event.key === "k") {
    event.preventDefault();
    // Toggling rather than reopening: Cmd-K on an open palette is a request
    // to get rid of it, not to clear the query someone is halfway through.
    if (palette?.isOpen) palette.close();
    else {
      palette?.open();
      // Opened first and refreshed behind it. The list only changes when a
      // document is opened, so it is almost always already right, and blocking
      // a keystroke on a file read to cover the case where it is not would
      // make every use of the palette pay for it.
      deps.refreshRecents();
    }
  } else if (matches("nav.goToPage", event) && title) {
    // Straight into the palette's argument mode. The shortcut and the palette
    // row reach the same code, which is the point of `askFor` -- a second way
    // to ask for a page number is a second thing to keep right.
    event.preventDefault();
    palette?.askFor("nav.goToPage");
  } else if (matches("view.zoomTo", event) && title) {
    event.preventDefault();
    palette?.askFor("view.zoomTo");
  } else if (matches("file.open", event)) {
    // ⌘O was advertised in the palette and reached nothing at all: the label
    // was written by hand and no handler was ever added for it, which is the
    // exact disagreement the shared table exists to make impossible.
    //
    // Prevented whether or not an open is already running, but only issued
    // when one is not --- the same guard the Open button carries as
    // `disabled`. Without it the keyboard is the one path that can stack file
    // dialogs, and the second chooser's document then waits behind the first
    // in `opens` for no reason anyone asked for.
    event.preventDefault();
    if (!actions.busyOpening()) actions.openDocument();
  } else if (matches("find.open", event) && title) {
    event.preventDefault();
    actions.focusFind();
  } else if (matches("file.print", event)) {
    // Prevented whether or not a document is open --- note the missing
    // `&& title` that every other binding here has. WKWebView's own Cmd-P
    // prints the *page*: the chrome, the toolbar, and a scaled-down
    // screenshot of whatever tiles happen to be painted. On the empty state
    // that is a picture of the words "Open a PDF, or drop one here."
    event.preventDefault();
    actions.printDocument();
  } else if (matches("view.toggleSidebar", event) && title) {
    event.preventDefault();
    actions.toggleSidebar();
  } else if (matches("view.invertPages", event) && title) {
    event.preventDefault();
    actions.toggleInvert();
  } else if (matches("find.matchCase", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("matchCase");
  } else if (matches("find.wholeWord", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("wholeWord");
  } else if (matches("find.regex", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("regex");
  } else if (matches("find.inSelection", event) && title) {
    event.preventDefault();
    actions.toggleSearchScope();
  } else if (matches("edit.rotatePageClockwise", event) && title) {
    event.preventDefault();
    actions.rotatePage(1);
  } else if (matches("edit.rotatePageCounterClockwise", event) && title) {
    event.preventDefault();
    actions.rotatePage(-1);
  } else if (matches("file.saveCopy", event) && title) {
    event.preventDefault();
    actions.saveCopy();
  } else if (matches("edit.undo", event) && title && !inTextField(event)) {
    // The `inTextField` guard is on these two and on nothing else here, and the
    // asymmetry is deliberate rather than an oversight. Every other binding
    // above is a chord no text field claims, so taking it from the find bar is
    // what a reader wants. Cmd-Z is the exception: it is *the* text-undo chord
    // on both platforms, and stealing it would mean a reader correcting a typo
    // in the find field silently undid a page rotation instead.
    event.preventDefault();
    if (actions.canUndo()) actions.undoEdit();
  } else if (matches("edit.redo", event) && title && !inTextField(event)) {
    event.preventDefault();
    if (actions.canRedo()) actions.redoEdit();
  }
}

/**
 * Whether the key went to something a reader is typing into.
 *
 * Duck-typed rather than `instanceof HTMLElement`, so that the guard is
 * exercised by the same tests that exercise everything else here.
 *
 * Measured rather than assumed, because the guess was wrong: the test runner has
 * no DOM at all, `globalThis.HTMLElement` is `undefined`, and
 * `target instanceof HTMLElement` there **throws** ---
 * `TypeError: Right-hand side of 'instanceof' is not an object`. So it does not
 * quietly answer "not a text field"; it takes the whole handler down, and the
 * only way to test the guard would be to stand up a DOM for it. Duck-typing
 * costs three field reads and needs neither.
 */
function inTextField(event: KeyboardEvent): boolean {
  const target = event.target as {
    tagName?: string;
    isContentEditable?: boolean;
  } | null;
  if (!target) return false;
  if (target.isContentEditable === true) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}
