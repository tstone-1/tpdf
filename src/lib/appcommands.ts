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
import { BINDINGS, inTextField, label, matches, type BoundCommand } from "./keys";
import { describeRange, parsePageRange } from "./pageranges";
import { PALETTE } from "./markcolors";
import type { MarkKind } from "./pages";
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
  /** Say which version this is, without asking the network anything. */
  about(): void;
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
   * Crop the page the reader is on to the box its ink occupies, or put the
   * file's own box back.
   *
   * One action with two arguments rather than two actions, which is the shape
   * `rotatePage` has and for the same reason: two entry points into one call is
   * where the second one drifts.
   */
  cropPage(to: "content" | "reset"): void;
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
  /**
   * Highlight the selected text, as a real annotation on the document.
   *
   * A shell action rather than a viewer method, for the reason `rotatePage`
   * gives: the *selection* is the viewer's and the *mark* is the model's, and
   * what a highlight becomes --- its id, whether it is accepted at all --- is
   * the journal's answer, replayed on undo.
   */
  markSelection(kind: MarkKind): void;
  /**
   * Drops a comment on the page, at a point or wherever the shell decides.
   *
   * Separate from {@link markSelection} rather than a fourth kind passed to it,
   * even though the model *does* treat it as a fourth kind. The two take
   * different things --- one takes a selection, this takes a point --- and a
   * `markSelection("note")` would silently make a comment the size of whatever
   * happened to be selected, which is the shape of mistake a type should make
   * unsayable rather than a comment warn about.
   *
   * The point is `null` from the palette and the menu bar, which have no
   * pointer. Where that lands is the viewer's answer; see `commentAt`.
   */
  addComment(at: { clientX: number; clientY: number } | null): void;
  /**
   * Arms the box tool: the reader's next drag on a page draws one.
   *
   * **Arms rather than draws**, which is why it takes no rectangle and why it
   * is not `markSelection("square")`. The other three take a selection and this
   * one takes a gesture the reader has not made yet --- so unlike every command
   * beside it, running this changes nothing about the document and everything
   * about what the next press means. See `Viewer.armDraw` for why a mode is
   * unavoidable here and why it is one-shot.
   */
  drawBox(): void;
  /**
   * Arms the ellipse tool: the reader's next drag on a page draws one.
   *
   * A second command rather than an argument to {@link drawBox}, for the reason
   * {@link draw} gives below: a reader who wants a ring wants it in one press,
   * and a command that asked which shape afterwards would be two presses to say
   * one thing. The gesture it reads is the box's exactly --- two corners --- and
   * that is what makes them separate commands rather than separate modes.
   */
  drawEllipse(): void;
  /**
   * Arms the text box tool: the reader's next drag on a page draws one.
   *
   * The same gesture as {@link drawBox} and {@link drawEllipse}, and a third
   * command for their reason. What differs is what happens *after* the drag: the
   * other two are finished when the reader lets go, and this one has only just
   * started --- the box is empty until they type into its note.
   */
  drawTextBox(): void;
  /**
   * Arms the freehand tool for one drawing.
   *
   * The same shape as {@link drawBox} and for the same reasons --- a mode,
   * one-shot, no chord --- and it is a second command rather than an argument to
   * the first because a reader who wants to draw wants it in one press. What
   * differs is only the gesture the armed tool reads: a box is two corners and
   * this is every point between them.
   */
  draw(): void;
  /**
   * Arms the eraser.
   *
   * {@link draw}'s counterpart, and the third command here that arms rather
   * than acts. It takes whole strokes out of freehand drawings; a sweep over a
   * highlight does nothing, and removing a whole mark of any kind is
   * {@link removeMark}, which says so in its name.
   */
  erase(): void;
  /** Takes the mark whose note is open off the page it is on. */
  removeMark(): void;
  /** Whether a mark's note is open, which is what names the mark to remove. */
  hasOpenMark(): boolean;
  /**
   * Picks the colour marks are drawn in, by a `markcolors.ts` swatch id.
   *
   * One call for both meanings --- what the next mark will be, and what the mark
   * whose note is open becomes. See `App.svelte`, where the rule is written out;
   * stating it here as well would be the second copy of it.
   */
  setMarkColor(id: string): void;
  /** Which swatch is picked. For the check harness. */
  markColor(): string;
  /** Whether there is a selection to mark. */
  hasSelection(): boolean;
  /** Write the working document over the file it was opened from. */
  saveDocument(): void;
  /** Whether anything differs from the file on disk. */
  isDirty(): boolean;
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
      // Always enabled and asking nothing of the network, which is the whole
      // difference from the command below it. "Which version am I running" is a
      // question a reader has *because* something is wrong, and an answer that
      // needs a working endpoint is no answer in exactly that case.
      //
      // It exists because nothing in the application answered it at all until
      // 2026-08-19, and the cost was a bug report: a reader on `26.8.4` hit the
      // Windows defect where an app with no console could open no document, and
      // could not tell whether the release fixing it was the one they had.
      id: "app.about",
      title: "About tpdf",
      run: () => actions.about(),
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
      // **No keyboard binding, deliberately, and for the opposite reason to
      // the deletion below.** ⌘H is the macOS shortcut for hiding the
      // application and cannot be taken; ⌘⇧H is Acrobat's and is free, but a
      // chord for a command that only ever applies to a selection teaches
      // itself badly --- a reader who presses it with nothing selected gets
      // nothing and no explanation. It is offered in the palette, in the Edit
      // menu, and in the right-click menu over a selection, which is where a
      // reader who has just dragged across a line is already looking.
      // **The only mark command with no `hasSelection` guard**, because a
      // comment is not made *of* a selection --- it is dropped on the page. It
      // is offered on any open document, which is also what makes it reachable
      // from the palette and the menu bar, where there is no pointer and
      // therefore no place a selection-shaped command could act.
      //
      // "Add comment" rather than "Add note": the note is the *text* inside it,
      // and every mark has one of those. `markpopup.ts` uses the same word for
      // the same reason.
      id: "edit.addComment",
      title: "Add comment",
      enabled: withDocument,
      run: () => actions.addComment(null),
    },
    {
      // **"Draw a box" rather than "Box selection"**, and the verb is the whole
      // difference: the three below act on what is already selected, and this
      // one asks the reader to do something next. The ellipsis says so in the
      // way the platform's own menus do --- `runMenuCommand` uses one for a
      // command that opens the palette to ask for a value, and this is the same
      // promise about a different kind of asking.
      //
      // No `hasSelection` guard, for the comment's reason above: a box is not
      // made *of* a selection. It is offered on any open document, which is
      // also what makes it reachable from the palette and the menu bar, where
      // there is no pointer at all --- arming from there is exactly as useful,
      // since the gesture comes afterwards either way.
      //
      // No chord. A one-shot mode a reader can enter by accident and then not
      // recognise is worse than one they have to ask for, and every letter that
      // would suit is taken by navigation.
      id: "edit.drawBox",
      title: "Draw a box...",
      enabled: withDocument,
      run: () => actions.drawBox(),
    },
    {
      // Beside the box, because it is the box's gesture and a reader choosing
      // between the two is choosing a shape rather than a tool. Everything the
      // entry above says about arming a mode, about having no `hasSelection`
      // guard and about having no chord applies here unchanged.
      //
      // "an ellipse", not "a circle": `/Circle` is the file's word and one a
      // reader drags out is almost never circular, which is `markpopup.ts`'s
      // argument for the label and the same one `edit.drawBox` makes about
      // "square".
      id: "edit.drawEllipse",
      title: "Draw an ellipse...",
      enabled: withDocument,
      run: () => actions.drawEllipse(),
    },
    {
      // The third tool that reads a drag, after the two shapes. Everything the
      // box's entry says about arming a mode applies unchanged.
      //
      // "Add a text box...", not "Draw...": the other two are finished by the
      // gesture and this one is not --- the drag places an empty box and the
      // words come afterwards, so a verb about drawing would describe the wrong
      // half of it.
      id: "edit.addTextBox",
      title: "Add a text box...",
      enabled: withDocument,
      run: () => actions.drawTextBox(),
    },
    {
      // Beside the box, and everything the entry above says about arming a mode
      // applies here unchanged. No chord for the same reason: a one-shot mode a
      // reader enters by accident and does not recognise is worse than one they
      // have to ask for.
      //
      // "Draw freehand", not "Ink": `/Ink` is the file's word and `ink` is the
      // wire's, and inside this codebase "ink" already names how a mark is laid
      // down rather than which mark it is. "Freehand" is also what separates it
      // from the box above at a glance, which is what a menu is for.
      id: "edit.draw",
      title: "Draw freehand...",
      enabled: withDocument,
      run: () => actions.draw(),
    },
    {
      // Beside the pen, because it is the other tool that stays armed and a
      // reader who has just drawn something is the reader who wants it.
      //
      // "Erase drawing", not "Erase": it takes whole strokes out of freehand
      // drawings and nothing else, and a bare "Erase" beside "Remove mark"
      // would read as a second, blunter way to delete anything. The ellipsis
      // matches the two tools above it and says the same thing they do --- the
      // command arms something rather than acting now.
      id: "edit.erase",
      title: "Erase drawing...",
      enabled: withDocument,
      run: () => actions.erase(),
    },
    {
      id: "edit.highlightSelection",
      title: "Highlight selection",
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("highlight"),
    },
    {
      // The same shape as the highlight above, and deliberately three entries
      // rather than one command that asks which kind: a reader who wants a
      // strikeout wants it in one press, and a palette that answered "Mark
      // selection" and then asked again would cost two. The chords are left
      // free for the same reason the highlight's is --- a chord for something
      // that only ever applies to a selection teaches itself badly.
      id: "edit.underlineSelection",
      title: "Underline selection",
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("underline"),
    },
    {
      id: "edit.strikeoutSelection",
      title: "Strike out selection",
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("strikeout"),
    },
    {
      // The fourth and last of them: PDF 32000-1 lists exactly four subtypes
      // that carry `/QuadPoints`, and with this one tpdf writes all four. A
      // fourth entry rather than an argument, for the reason the note above
      // gives about the other three.
      //
      // "Squiggly underline", not "Squiggly": the word alone names a shape
      // rather than an action, and it sits directly under "Underline selection"
      // where a reader is choosing between two lines under the same words.
      id: "edit.squigglySelection",
      title: "Squiggly underline selection",
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("squiggly"),
    },
    // Built from `PALETTE` rather than written out, because a colour added there
    // and not here would be in the swatch row and reachable from nowhere else.
    // Seven separate commands rather than one that asks which colour, for the
    // reason the three selection marks above are three: a reader who wants green
    // wants it in one press, and a palette that answered "Mark colour" and then
    // asked again would cost two.
    //
    // No chords. Every letter that would suit is taken by navigation, and a
    // colour is not a thing anybody reaches for often enough to spend one on.
    ...PALETTE.map((entry) => ({
      id: `edit.color.${entry.id}`,
      // "Colour:" leads for `find.matchCase`'s reason --- it groups the run in a
      // list sorted by title, and it is what a reader searches for when they do
      // not know this application calls the colour "pink". The colour itself is
      // lowercase because it is a word in a sentence rather than a name.
      title: `Colour: ${entry.name}`,
      // Enabled on any open document, with or without a mark open: with one it
      // recolours, without one it arms the next mark. Guarding on `hasOpenMark`
      // would grey out exactly the case a reader uses to choose before marking.
      enabled: withDocument,
      run: () => actions.setMarkColor(entry.id),
    })),
    {
      // Takes the mark whose note is open, because that is the one the reader
      // has named. There is no "the mark under the pointer" here: a menu item
      // is chosen with the pointer somewhere else entirely, and the open note
      // is the application's own record of which mark is being worked on.
      id: "edit.removeMark",
      // Not "Remove highlight", which was right while a highlight was the only
      // mark there was. A menu item naming one of three kinds is wrong twice
      // out of three times, and is chosen with the pointer somewhere else, so
      // it cannot say which mark it means the way the note box can.
      title: "Remove mark",
      enabled: () => withDocument() && actions.hasOpenMark(),
      run: () => actions.removeMark(),
    },
    {
      // **Crop to content**, and there is still no crop-by-dragging --- but the
      // reason has changed and is worth correcting rather than leaving. It used
      // to be that a rectangle a reader draws needs a drag mode this
      // application did not have; `drag.ts` and `edit.drawBox` are that mode
      // now, so what is missing is only a second caller of it. Measuring the
      // ink remains the better answer for the case a reader actually wants --- a
      // scan, or an article whose margins are wider than its column --- which is
      // why it is still the one that exists.
      //
      // Measured from a low-resolution render rather than from the page's
      // objects, which is what makes it work on a scan at all: see `content.rs`,
      // where a scanned page is one image object covering the sheet and the
      // object union is therefore the sheet.
      id: "edit.cropToContent",
      title: "Crop page to content",
      enabled: withDocument,
      run: () => actions.cropPage("content"),
    },
    {
      // Offered whether or not this page is cropped, like Undo and unlike
      // "Remove mark": whether a crop is in force is a property of whichever
      // page the reader has scrolled to, and a command that came and went as
      // they scrolled would be worse than one that sometimes does nothing.
      id: "edit.resetCrop",
      title: "Reset page crop",
      enabled: withDocument,
      run: () => actions.cropPage("reset"),
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
      // Guarded on there being something to save, for the reason `edit.undo`
      // is guarded on there being something to undo. It is the opposite of the
      // guard on `file.saveCopy` below, and deliberately: a copy of an unedited
      // document is a file a reader wants, and rewriting the open file to say
      // exactly what it already says is not.
      id: "file.save",
      title: "Save",
      keys: label("file.save"),
      enabled: () => actions.viewer() !== null && actions.isDirty(),
      run: () => actions.saveDocument(),
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
      // The only way to reach a mark of the reader's own without a pointer.
      // Until this existed a highlight's note could not be read, changed or
      // taken off from the keyboard at all --- the whole subsystem was reachable
      // by one input device.
      //
      // `nav` rather than `edit`, and that is not filing: it *moves* the reader.
      // Editing is what the note box and "Remove mark" then do, and both are
      // already commands of their own.
      id: "nav.nextMark",
      title: "Next mark",
      keys: label("nav.nextMark"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepMark(1);
      },
    },
    {
      id: "nav.previousMark",
      title: "Previous mark",
      keys: label("nav.previousMark"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepMark(-1);
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
      // "Show my marks", not "Show marks": the panel lists what *this* reader
      // made, and the document's own annotations are in the comments tab
      // beside it. A reader with both open needs the two titles to say which
      // is which, and "Annotations" would name either.
      id: "view.showMarks",
      title: "Show my marks",
      enabled: withDocument,
      run: () => actions.showTab("marks"),
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
 * Opens the palette, or closes an open one.
 *
 * Extracted from ⌘K's arm in {@link handleWindowKey} so the toolbar's palette
 * button reaches the same code rather than a second copy of it. That button is
 * not a convenience: `menu.rs` puts the native menu bar behind
 * `#[cfg(target_os = "macos")]` on purpose, so on Windows the palette is the
 * only route to most of this application's commands, and it was advertised
 * nowhere on screen --- a reader had to already know ⌘K to find anything. A
 * second implementation here would be a second place to forget the recents
 * refresh, which is the disagreement `keys.ts` exists to make impossible for
 * labels and this exists to make impossible for one command.
 */
export function togglePalette(
  deps: Pick<WindowKeyDeps, "palette" | "refreshRecents">,
): void {
  const palette = deps.palette();
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
}

/**
 * The shortcuts that belong to the window rather than to the surface.
 *
 * Matched through `keys.ts`, which is where the palette's labels come from
 * too --- see the note there, and note that ⌘K is now in that table as well.
 * It was the one exception, on the reasoning that the palette is not a row in
 * itself so no label could disagree with it; the toolbar button added for
 * Windows renders a label for it, which ended the exception.
 */
export function handleWindowKey(
  event: KeyboardEvent,
  deps: WindowKeyDeps,
): void {
  const palette = deps.palette();
  const { actions } = deps;
  const title = deps.hasDocument();

  if (matches("app.palette", event)) {
    event.preventDefault();
    togglePalette(deps);
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
  } else if (matches("file.save", event) && title) {
    // Guarded on there being something to save, exactly as ⌘Z is guarded on
    // there being something to undo. ⌘S is the chord readers press by reflex on
    // a document they have not touched, and the backend answers that with a
    // refusal --- correctly, since it is what a stale frontend deserves --- but
    // a message about nothing having happened is not what a reflex wants back.
    event.preventDefault();
    if (actions.isDirty()) actions.saveDocument();
  } else if (matches("file.saveCopy", event) && title) {
    event.preventDefault();
    actions.saveCopy();
  } else if (
    matches("edit.selectAll", event) &&
    title &&
    !inTextField(event) &&
    !event.defaultPrevented
  ) {
    // ⌘A and ⌘C belong to the surface while the surface has the keyboard: its
    // own handler matches both and prevents the default, which is what
    // `defaultPrevented` reads here. This is the first arm in this function that
    // needs such a guard --- every other chord above is one the viewer does not
    // claim, so the two lists have never overlapped before.
    //
    // What the viewer's handler cannot see is a reader whose focus is in the
    // chrome. Click the document's name in the toolbar and the event never
    // reaches the viewer's root at all, so before this arm ⌘A fell through to
    // the web view, whose select-all takes the *toolbar* --- the Open button,
    // every find toggle, and the find field's contents. Reported from use on
    // Windows, where there is no menu bar to claim the chord first.
    //
    // Guarded on the field for the reason `NO_ACCELERATOR` in `menubar.ts`
    // gives: inside the find bar ⌘A means *this field*, and taking it there
    // would stop a reader replacing a query they had half typed.
    event.preventDefault();
    actions.viewer()?.selectPage();
  } else if (
    matches("edit.copy", event) &&
    title &&
    !inTextField(event) &&
    !event.defaultPrevented
  ) {
    // The same defect as ⌘A above and the same three guards. Left out of a fix
    // for select-all alone it would be the quieter half: ⌘A on the chrome did
    // something visibly wrong, ⌘C there did nothing at all, which reads as a
    // copy that silently failed.
    event.preventDefault();
    void actions.viewer()?.copySelection();
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
