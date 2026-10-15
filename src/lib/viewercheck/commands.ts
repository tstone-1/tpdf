/**
 * The command palette, commands that take an argument, and every registered command of the application.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import {
  type AppActions,
  registerAppCommands,
  type WindowKeyDeps,
  handleWindowKey,
} from "../appcommands";
import { PALETTE } from "../markcolors";
import { NIBS } from "../marknibs";
import { PAGE_SIZE_NAMES } from "../pagesizes";
import { CommandRegistry } from "../commands";
import type { DocumentInfo } from "../ipc";
import { Palette } from "../palette";
import { isLinux } from "../keys";
import type { StampName } from "../pages";
import { Viewer } from "../viewer";
import { check, settle, skip } from "./common";
import { syntheticMark, syntheticRedaction } from "./marks";
import { pickNeedle } from "./invert";

/**
 * The command palette, driven through its own DOM.
 *
 * The registry is four commands built here, not the application's, so what this
 * covers is the palette *mechanism* --- filtering, highlighting, Escape, running
 * the selected row --- against a list small enough to state the expected result
 * of every keystroke. The ranking underneath is `commands.test.ts`.
 *
 * The application's real command list and the ⌘K that opens it were covered by
 * nothing at all, which `docs/PLAN.md` recorded as a gap; they are
 * `appCommandChecks` below now. Kept separate rather than merged: a mechanism
 * check wants a list it controls, and a wiring check wants the list a reader
 * actually gets.
 *
 * The load-bearing assertion is that Enter *ran* something: a palette that
 * filters beautifully and does nothing passes every other check here. It carries
 * the control this repository keeps needing --- the viewer is asserted not to be
 * at the end of the document before the command that takes it there.
 */
export async function paletteChecks(viewer: Viewer, pageCount: number): Promise<void> {
  const registry = new CommandRegistry();
  registry.register(
    {
      id: "view.fitWidth",
      title: "Fit width",
      keys: "⌘0",
      run: () => viewer.setFit("width"),
    },
    {
      id: "nav.lastPage",
      title: "Go to end",
      keys: "End",
      run: () => viewer.goToEnd(),
    },
    {
      id: "nav.firstPage",
      title: "Go to start",
      keys: "Home",
      run: () => viewer.goToStart(),
    },
    {
      id: "edit.copy",
      title: "Copy selection",
      enabled: () => false,
      run: () => {},
    },
  );
  const palette = new Palette(registry);

  const field = (): HTMLInputElement | null =>
    document.querySelector<HTMLInputElement>(".tpdf-palette input");

  /** Types into the palette's real input, through its real listener. */
  const type = (text: string): void => {
    const input = field();
    if (!input) return;
    input.value = text;
    input.dispatchEvent(new InputEvent("input", { bubbles: true }));
  };

  const press = (k: string): void => {
    field()?.dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
  };

  viewer.goToStart();
  await settle(() => viewer.idle);

  palette.open();
  check(
    "the palette opens on every enabled command",
    palette.isOpen && palette.visible.length === 3,
    `open=${palette.isOpen}, lists ${palette.visible.length} of 4 registered`,
  );
  check(
    "a disabled command is not offered",
    !palette.visible.includes("Copy selection"),
    `lists ${palette.visible.join(", ")}`,
  );

  type("fw");
  check(
    "typing an abbreviation finds the command",
    palette.highlighted === "Fit width",
    `"fw" highlights "${palette.highlighted}" of ${palette.visible.length} shown`,
  );

  type("zzzz");
  check(
    "a query matching nothing lists nothing",
    palette.visible.length === 0,
    `"zzzz" lists ${palette.visible.length}`,
  );

  type("go to");
  const before = palette.highlighted;
  press("ArrowDown");
  check(
    "the arrow keys move the highlight",
    palette.highlighted !== before && palette.visible.includes(before),
    `"${before}" -> "${palette.highlighted}"`,
  );

  // Escape must not run what was highlighted. Asserted against the viewer's
  // position, since "the palette closed" is true either way.
  type("go to end");
  const offsetBeforeEscape = viewer.offset;
  press("Escape");
  check(
    "Escape closes without running the command",
    !palette.isOpen && viewer.offset === offsetBeforeEscape,
    `open=${palette.isOpen}, offset ${offsetBeforeEscape.toFixed(0)} -> ${viewer.offset.toFixed(0)}`,
  );

  // The control: at the top of the document, so "went to the end" cannot be
  // satisfied by having been there already. A one-screen document cannot offer
  // that, and says so rather than passing on nothing.
  palette.open();
  type("go to end");
  const start = viewer.offset;
  if (viewer.maxOffset <= 1) {
    skip("Enter runs the highlighted command", "the document does not scroll");
  } else if (start >= viewer.maxOffset - 1) {
    skip(
      "Enter runs the highlighted command",
      "already at the end before running it",
    );
  } else {
    press("Enter");
    check(
      "Enter runs the highlighted command",
      !palette.isOpen && viewer.offset > start,
      `"${"go to end"}" from offset ${start.toFixed(0)} to ${viewer.offset.toFixed(0)} ` +
        `of ${viewer.maxOffset.toFixed(0)}`,
    );
  }

  await argumentChecks(viewer, palette, registry, pageCount, type, press);

  palette.destroy();
}

/**
 * A command that asks for a value before it runs.
 *
 * The page jump is the reason this exists --- a 775-page document had no way to
 * reach page 400 --- so every assertion here ties the typed value to where the
 * viewer ends up, not merely to the palette's own state. A prompt that opened,
 * validated and closed while jumping nowhere would pass a check on the panel.
 */
async function argumentChecks(
  viewer: Viewer,
  palette: Palette,
  registry: CommandRegistry,
  pages: number,
  type: (text: string) => void,
  press: (key: string) => void,
): Promise<void> {
  let went = -1;
  registry.register({
    id: "nav.goToPage",
    title: "Go to page…",
    keys: "⌥⌘G",
    argument: {
      placeholder: "Page number",
      problem: (raw) => {
        const trimmed = raw.trim();
        if (trimmed === "") return `Page number, 1 to ${pages}`;
        if (!/^[0-9]+$/.test(trimmed))
          return `"${trimmed}" is not a page number`;
        const page = Number(trimmed);
        return page < 1 || page > pages
          ? `This document has ${pages} pages`
          : null;
      },
      preview: (raw) => `Go to page ${Number(raw.trim())} of ${pages}`,
      run: (raw) => {
        went = Number(raw.trim()) - 1;
        viewer.goToPage(went);
      },
    },
  });

  palette.open();
  type("go to page");
  press("Enter");
  check(
    "choosing a command that takes a value asks for it",
    palette.isOpen && palette.isAsking && palette.prompt === "Page number",
    `open=${palette.isOpen}, asking=${palette.isAsking}, prompt="${palette.prompt}"`,
  );

  type("not a number");
  press("Enter");
  check(
    "a value the command refuses does not run it",
    palette.isOpen && palette.isAsking && went === -1,
    // Every term reported, not just the one that usually fails. Written as
    // "still asking" when `went === -1` and it lied: with the palette's own
    // validation mutated away the *registry* still refused the value, so `went`
    // stayed -1 while the panel closed --- the check failed correctly and its
    // detail described a state that was not the case.
    `asking=${palette.isAsking}, open=${palette.isOpen}, ` +
      (went === -1 ? "did not run" : `ran with page ${went + 1}`),
  );

  press("Escape");
  check(
    "Escape leaves the value, not the palette",
    palette.isOpen && !palette.isAsking,
    `open=${palette.isOpen}, asking=${palette.isAsking}`,
  );

  // The jump itself, and its control. A target near the end so "it moved" is
  // not satisfied by where the document already was --- and a document with too
  // few pages to have a distant one says so rather than passing on nothing.
  const target = Math.min(pages, 8);
  if (pages < 2) {
    skip("a typed page number goes to that page", "the document has one page");
  } else {
    viewer.goToStart();
    await settle(() => viewer.idle);
    const from = viewer.position.page;
    palette.open();
    type("go to page");
    press("Enter");
    type(String(target));
    press("Enter");
    await settle(() => viewer.idle);
    // `position.page` is the page at the *top edge*, and the last page of a
    // short document cannot get there: the scroller stops at maximum scroll
    // with earlier pages still above it. Found on `rotated-90`, which has four
    // landscape pages, so the target was the last one and the viewer honestly
    // showed page 3 for a correct jump to page 4. Excused only in exactly that
    // case --- the target is the final page and the document is scrolled as far
    // as it goes --- rather than by weakening the assertion everywhere.
    const atTop = viewer.position.page === target - 1;
    const pinned = target === pages && viewer.offset >= viewer.maxOffset - 1;
    check(
      "a typed page number goes to that page",
      !palette.isOpen &&
        went === target - 1 &&
        from !== target - 1 &&
        (atTop || pinned),
      from === target - 1
        ? `already on page ${target} before the jump, so this proves nothing`
        : `typed ${target}, ran with page ${went + 1}, viewer shows page ${viewer.position.page + 1}` +
            ` (from ${from + 1})` +
            (atTop || !pinned
              ? ""
              : ", scrolled to the end since it is the last page"),
    );
  }

  // Out of range is refused rather than clamped: a reader who types 900 into a
  // 775-page document has made a mistake, and silently landing on the last page
  // hides it. Asserted against the viewer, because "the palette stayed open" is
  // also true of a prompt that jumped and forgot to close.
  const settled = viewer.position.page;
  went = -1;
  palette.open();
  type("go to page");
  press("Enter");
  type(String(pages + 1));
  press("Enter");
  await settle(() => viewer.idle);
  check(
    "a page number past the end is refused, not clamped",
    palette.isAsking && went === -1 && viewer.position.page === settled,
    `asked for page ${pages + 1} of ${pages}: ran=${went + 1}, ` +
      `page ${settled + 1} -> ${viewer.position.page + 1}`,
  );
  press("Escape");
  palette.close();
}

/**
 * The command list the application really has, driven the way a reader drives
 * it, and the ⌘K that opens it.
 *
 * `paletteChecks` above builds a four-command registry of its own, and said so:
 * what it proves is that the palette works, **not** that any command a reader
 * can type reaches anything. `docs/PLAN.md` recorded that as a gap and it was
 * one --- the list `App.svelte` registered was covered by nothing, and neither
 * was ⌘K. Both now come from `appcommands.ts`, so this can import the real ones.
 *
 * ## Every command is driven, or says why not
 *
 * The audit below is the part worth keeping. A table classifies every id, the
 * check asserts the table and the registry are the *same set*, and it prints the
 * population it found --- so a command added tomorrow turns this red until
 * somebody decides how it is covered, and a command renamed turns it red from
 * the other side. A tally would not do that: this file has already been caught
 * counting checks that had stopped existing, and `AGENTS.md` says to diff the
 * names rather than compare the totals.
 *
 * ## Two kinds of coverage, and the difference is stated
 *
 * A command reaching the {@link Viewer} is asserted against a real viewer: the
 * zoom, the page, the rotation or the selection has to move, and each has a
 * control establishing it was not already where the command would take it. A
 * command reaching the shell --- a file dialog, a print panel, a Svelte flag ---
 * is asserted to reach *that action, once*, and nothing is claimed about what
 * the action then does. There is no shell here to do it in.
 */
export async function appCommandChecks(
  viewer: Viewer,
  doc: DocumentInfo,
): Promise<void> {
  /** Shell actions that fired, in order, since the last clearing. */
  let fired: string[] = [];
  let busy = false;
  let recents = 0;
  let hasDocument = true;
  // Both false to begin with, which is the state a launch is really in, and both
  // movable so that Undo and Redo can be checked in each direction. A guard
  // pinned to one answer is a guard with no failing case.
  let canUndo = false;
  let canRedo = false;

  const actions: AppActions = {
    editText: () => { fired.push("editText"); },
    signature: () => { fired.push("signature"); },
    fillForm: () => {},
    viewer: () => viewer,
    pageCount: () => doc.page_count,
    openDocument: () => fired.push("openDocument"),
    closeDocument: () => fired.push("closeDocument"),
    closeAllDocuments: () => fired.push("closeAllDocuments"),
    sides: () => ({ split: false, focused: "left" as const, others: [] as string[] }),
    showBeside: (index: number) => fired.push(`showBeside:${index}`),
    moveToOtherSide: () => fired.push("moveToOtherSide"),
    switchSides: () => fired.push("switchSides"),
    showOnBothSides: () => fired.push("showOnBothSides"),
    focusOtherSide: () => fired.push("focusOtherSide"),
    syncScrolling: () => false,
    toggleSyncScrolling: () => fired.push("toggleSyncScrolling"),
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
    busyOpening: () => busy,
    busyDocument: () => false,
    printDocument: () => fired.push("printDocument"),
    focusFind: () => fired.push("focusFind"),
    toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
    toggleSearchScope: () => fired.push("toggleSearchScope"),
    toggleSidebar: () => fired.push("toggleSidebar"),
    addComment: (at) => fired.push(`addComment:${at === null ? "here" : "at"}`),
    drawBox: () => fired.push("drawBox"),
    drawEllipse: () => fired.push("drawEllipse"),
    stamp: (name: StampName) => fired.push(`stamp:${name}`),
    drawTextBox: () => fired.push("drawTextBox"),
    drawField: (kind) => fired.push(`drawField:${kind}`),
    drawRadio: (group) => fired.push(`drawRadio:${group}`),
    fieldBorder: () => true,
    setFieldBorder: (border) => fired.push(`setFieldBorder:${border}`),
    pickedMarks: () => viewer.pickedCount,
    duplicatePicked: () => fired.push("duplicatePicked"),
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
    showTab: (tab) => fired.push(`showTab:${tab}`),
    toggleInvert: () => fired.push("toggleInvert"),
    // Recorded like the rest, though neither command is driven here --- both are
    // in `undriven` above. A recorder rather than a throw, so that the day one
    // of them *is* driven the failure is a missing entry in `fired` rather than
    // an exception from a helper nobody was looking at.
    about: () => fired.push("about"),
    checkForUpdates: () => fired.push("checkForUpdates"),
    commandLineTool: (install) => fired.push(`commandLineTool:${install}`),
    // Not known, which is what a check that asks no backend has: both offered.
    commandLineToolOffered: () => true,
    makeDefaultPdfApp: () => fired.push("makeDefaultPdfApp"),
    automaticUpdates: () => true,
    setAutomaticUpdates: (enabled) => fired.push(`setAutomaticUpdates:${enabled}`),
    applyUpdate: () => fired.push("applyUpdate"),
    restartForUpdate: () => fired.push("restartForUpdate"),
    // False both, so the install command's `enabled` guard is exercised in the
    // direction the check can assert: it must not appear in the palette on a
    // run where nothing has been found. `updateReady` false also withholds the
    // restart, which is the only direction a check that must not end its own
    // process can assert.
    updateReady: () => false,
    updateAvailable: () => false,
    // The page operations. Recorders like the rest of the shell half: the model
    // that decides what a page's turn becomes is in the backend, so what these
    // commands can be checked for here is that they reach the right action ---
    // and the `enabled` guards below are what needs the two booleans to be
    // separately settable.
    rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
    deletePage: () => fired.push("deletePage"),
    insertBlankPage: () => fired.push("insertBlankPage"),
    insertSizedPage: (name) => fired.push(`insertSizedPage:${name}`),
    // A recorder like its neighbours, which is also what keeps the sweep from
    // opening a file dialog: the dialog and the backend are `App.svelte`'s, and
    // `tabs_check.py --phase import` drives those against a real file.
    importPages: () => fired.push("importPages"),
    // Null: nothing is waiting in this harness, so the range question is not
    // offered, which is the direction `undriven` below names.
    pendingImport: () => null,
    insertChosenPages: (pages) => fired.push(`insertChosenPages:${pages.join("+")}`),
    dropImport: () => fired.push("dropImport"),
    cropPage: (to) => fired.push(`cropPage:${to}`),
    redactRegion: () => fired.push("redactRegion"),
    redactSelection: () => fired.push("redactSelection"),
    redactMatches: () => fired.push("redactMatches"),
    matchCount: () => viewer.matchCount,
    movePage: (delta) => fired.push(`movePage:${delta}`),
    undoEdit: () => fired.push("undoEdit"),
    redoEdit: () => fired.push("redoEdit"),
    canUndo: () => canUndo,
    canRedo: () => canRedo,
    // The mark half. `hasSelection` is read from the viewer rather than pinned,
    // because the command's `enabled` guard turns on it -- a fixed `true` would
    // make the guard untestable, and a fixed `false` would take the command out
    // of the palette and make the probe below unrunnable.
    markSelection: (kind) => fired.push(`markSelection:${kind}`),
    hasSelection: () => viewer.selectedText.length > 0,
    // And the same for the other guard that reads the viewer: which mark to
    // remove *is* the open note, so pinning this true would take the only
    // interesting thing about the command out of the check.
    removeMark: () => fired.push("removeMark"),
    // And the same again for the third viewer-read guard: which region to
    // remove *is* the pick, so pinning this true would take the only
    // interesting thing about the command out of the check.
    removeRedaction: () => fired.push("removeRedaction"),
    setMarkColor: (id: string) => fired.push(`setMarkColor:${id}`),
    setNib: (id: string) => fired.push(`setNib:${id}`),
    markColor: () => "default",
    hasOpenMark: () => viewer.markOpen >= 0,
    hasPickedRedaction: () => viewer.redactionPicked >= 0,
    canEditComment: () => viewer.commentEditable,
    editComment: () => {
      viewer.editComment();
      return 1;
    },
    canReplyToComment: () => viewer.commentReplyable,
    replyToComment: () => {
      viewer.replyToComment();
      return 1;
    },
    canDeleteComment: () => viewer.commentDeletable,
    deleteComment: () => {
      viewer.deleteComment();
      return 1;
    },
    // Recorded rather than driven, like the update pair: `file.save` is in
    // `undriven` below, because a ⌘S here would write the corpus fixture that
    // every other check in the run is reading. False so the command stays out
    // of the palette, which is the only direction this harness can assert ---
    // the other is `appcommands.test.ts`'s.
    saveDocument: () => fired.push("saveDocument"),
    isDirty: () => false,
    saveCopy: () => fired.push("saveCopy"),
    redactCopy: () => fired.push("redactCopy"),
    redactRasterCopy: () => fired.push("redactRasterCopy"),
    recogniseText: () => fired.push("recogniseText"),
    findHiddenText: () => fired.push("findHiddenText"),
    chooseRecognitionLanguage: () => fired.push("chooseRecognitionLanguage"),
    recognitionLanguages: () => null,
    setRecognitionLanguage: (raw) => fired.push(`setRecognitionLanguage:${raw}`),
    dropRecognitionLanguages: () => fired.push("dropRecognitionLanguages"),
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
    addCertificateFile: () => fired.push("addCertificateFile"),
    addValidationData: () => fired.push("addValidationData"),
    signableFields: () => 1,
    signField: () => fired.push("signField"),
    showProperties: () => fired.push("showProperties"),
  };

  // Where the viewer was on arrival, so it can be put back. Every phase after
  // this one inherits whatever state it is left in, and the first run of these
  // checks turned eight later assertions red across three phases --- the last
  // probe rotates, and outlines, thumbnails and the rotation checks themselves
  // all read a viewer that was three quarter-turns from where they expected it.
  // `AGENTS.md` has this as "a control can be contaminated by the phase that ran
  // before it"; the restoration is asserted below rather than assumed.
  const entry = {
    rotation: viewer.rotation,
    fit: viewer.fitMode,
    zoom: viewer.currentZoom,
  };

  const registry = new CommandRegistry();
  registerAppCommands(registry, actions);
  const palette = new Palette(registry);
  const deps: WindowKeyDeps = {
    actions,
    palette: () => palette,
    hasDocument: () => hasDocument,
    refreshRecents: () => {
      recents += 1;
    },
  };

  const field = (): HTMLInputElement | null =>
    document.querySelector<HTMLInputElement>(".tpdf-palette input");
  const type = (text: string): void => {
    const input = field();
    if (!input) return;
    input.value = text;
    input.dispatchEvent(new InputEvent("input", { bubbles: true }));
  };
  const press = (k: string): void => {
    field()?.dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
  };
  /** A window chord, through the real routing rather than through a call. */
  const chord = (
    key: string,
    mods: { accel?: boolean; shift?: boolean; alt?: boolean } = {},
  ): boolean => {
    const event = new KeyboardEvent("keydown", {
      key,
      metaKey: mods.accel ?? false,
      shiftKey: mods.shift ?? false,
      altKey: mods.alt ?? false,
      bubbles: true,
      cancelable: true,
    });
    handleWindowKey(event, deps);
    return event.defaultPrevented;
  };

  // ⌘K, which is the one chord that is not a command and so had nothing
  // advertising it to disagree with. Asserted closed first: "the palette is
  // open" is true of a palette that was already open, which is the shape of
  // pass this file keeps having to guard against.
  check(
    "the palette is closed before Cmd-K",
    !palette.isOpen,
    `open=${palette.isOpen}`,
  );
  const bareK = chord("k");
  check(
    "a bare k does not open the palette",
    !palette.isOpen && !bareK,
    `open=${palette.isOpen}, prevented=${bareK}`,
  );
  const opened = chord("k", { accel: true });
  check(
    "Cmd-K opens the palette",
    palette.isOpen && opened && recents === 1,
    `open=${palette.isOpen}, prevented=${opened}, recents refreshed ${recents}x`,
  );
  chord("k", { accel: true });
  check(
    "Cmd-K again closes it, and does not re-read the recents",
    !palette.isOpen && recents === 1,
    `open=${palette.isOpen}, recents refreshed ${recents}x`,
  );

  // ⌘P has no `&& title` guard, deliberately, because WKWebView's own ⌘P would
  // otherwise print a screenshot of the chrome. ⌘F has one. Driving both with
  // no document is what tells those two arms apart.
  hasDocument = false;
  fired = [];
  chord("p", { accel: true });
  const printedEmpty = fired.slice();
  chord("f", { accel: true });
  const findEmpty = fired.slice();
  hasDocument = true;
  check(
    "Cmd-P prints with no document open, and Cmd-F does not reach find",
    printedEmpty.join() === "printDocument" &&
      findEmpty.join() === "printDocument",
    `after Cmd-P: [${printedEmpty.join(", ")}], after Cmd-F: [${findEmpty.join(", ")}]`,
  );

  // The guard the Open button carries as `disabled`, which the keyboard route
  // shares and the palette row deliberately does not.
  busy = true;
  fired = [];
  chord("o", { accel: true });
  const whileBusy = fired.slice();
  busy = false;
  chord("o", { accel: true });
  check(
    "Cmd-O opens one dialog at a time",
    whileBusy.length === 0 && fired.join() === "openDocument",
    `busy: [${whileBusy.join(", ")}], idle: [${fired.join(", ")}]`,
  );

  /**
   * Runs a command the way a reader does: open, type its title, Enter.
   *
   * Returns why it could not, or "". The title has to rank *first* --- pressing
   * Enter on whatever happened to be highlighted would run some other command
   * and assert against it, which is the failure mode a harness cannot see.
   */
  const runByTitle = (title: string, argument?: string): string => {
    palette.open();
    type(title);
    if (palette.highlighted !== title) {
      const top = palette.highlighted;
      palette.close();
      return `"${title}" highlighted "${top}" of ${palette.visible.length}`;
    }
    press("Enter");
    if (argument !== undefined) {
      if (!palette.isAsking) {
        palette.close();
        return `"${title}" did not ask for a value`;
      }
      type(argument);
      press("Enter");
    }
    if (palette.isOpen) palette.close();
    return "";
  };

  const titleOf = (id: string): string => registry.find(id)?.title ?? "";

  /** One command, and what has to be different afterwards. */
  interface Probe {
    id: string;
    /** Typed into the argument prompt, for the two commands that ask. */
    argument?: string;
    /**
     * Puts the viewer somewhere the command can be seen to move it from.
     *
     * May return a promise, and `edit.redactMatches` is why: its precondition
     * is a completed search, and `viewer.idle` cannot be waited on for that ---
     * it reports the frame loop, not the scan. Every other `from` here is
     * synchronous and unaffected.
     */
    from?: () => void | Promise<void>;
    /** Reads what the command is supposed to change. */
    read: () => string;
    /** Whether the reading moved as the command promises. */
    moved: (before: string, after: string) => boolean;
    /** Why this document cannot exercise it, or null. */
    unless?: () => string | null;
    /**
     * Puts back whatever `from` changed about the document.
     *
     * Only the two mark probes need it, and they need it because `from` puts
     * marks on a document that has none: every probe after them would otherwise
     * run against a page carrying two highlights nobody made, and so would every
     * phase after this one.
     */
    then?: () => void;
  }

  // Half the corpus is one page, and two of it have no extractable text. A
  // probe that cannot move on such a document must say so: "next page" on a
  // one-page document is a check whose before and after agree whatever the
  // command does, which is decoration, and "select all" would simply go red for
  // a reason that is not a defect.
  const firstPage = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const hasText = (firstPage?.codes.length ?? 0) > 0;
  /**
   * Whether a page *after* the first can become the page being read.
   *
   * Stronger than "more than one page", and it has to be: the last page cannot
   * reach the top of the viewport, so on a two-page document stepping forward
   * lands nowhere and the position stays 0. `nav.goToPage` already carried this
   * guard; `nav.nextPage` and `nav.previousPage` carried `manyPages` and had
   * never met a two-page fixture.
   */
  const reachablePage = () =>
    doc.page_count > 2
      ? null
      : `page 2 is the last of ${doc.page_count} and cannot reach the top`;
  const withText = () => (hasText ? null : "page 1 has no extractable text");
  // `1-2` needs a page 2 to name. Two of the fourteen corpora are single-page
  // documents, and the probe's first version asserted in a comment that none
  // was --- an unchecked claim, which the sweep answered by going red on
  // `vector-heavy` and `links-cropped`. Skipped rather than narrowed to `1`:
  // a single slot reads the same whether the parser produced one page or
  // dropped one, so a weaker check under the same name would be worse than an
  // honest skip.
  const twoPages = () =>
    doc.page_count > 1 ? null : `a range needs two pages and this has ${doc.page_count}`;
  // Most of the corpus has no links at all, and one fixture has exactly one ---
  // so "step to the previous link" needs a second one to have anywhere to go,
  // and saying so is what keeps a skip from looking like a pass.
  const withLinks = () =>
    viewer.linkCount > 0 ? null : "the document has no links";
  const twoLinks = () =>
    viewer.linkCount > 1
      ? null
      : `the document has ${viewer.linkCount} link, so there is no earlier one`;

  const zoom = () => viewer.currentZoom.toFixed(3);
  const page = () => String(viewer.position.page);
  const turn = () => String(viewer.rotation);
  const fit = () => viewer.fitMode;
  const selection = () => String(viewer.selectedText.length);
  const shell = (expected: string) => ({
    read: () => fired.join(","),
    moved: (_before: string, after: string) => after === expected,
  });

  const last = doc.page_count - 1;
  /** The page a Back/Forward probe jumped to, recorded by its own `from`. */
  let wentTo = -1;
  const probes: Probe[] = [
    { id: "file.open", ...shell("openDocument"), read: () => fired.join(",") },
    // Driven rather than excused. Its two neighbours in the tpdf menu cannot be
    // -- one reaches the network, the other replaces the running binary -- and
    // this one asks nothing and replaces nothing, so an exclusion would have
    // been a habit rather than a reason. The comment beside NEEDS_NO_DOCUMENT
    // said "the sweep drives it" for a day while nothing did, which is the same
    // empty promise `update.ts`'s own comment was making about this command.
    { id: "app.about", ...shell("about"), read: () => fired.join(",") },
    // Two of the three disk-change choices, aimed separately so that a `map`
    // passing one mode to all three would be seen. The third is the mode this
    // fixture is in, so it is withheld: see `undriven`.
    ...(["reload", "ignore"] as const).map((mode) => ({
      id: `file.onDiskChange.${mode}`,
      ...shell(`setDiskChangeMode:${mode}`),
      read: () => fired.join(","),
    })),
    {
      id: "file.print",
      ...shell("printDocument"),
      read: () => fired.join(","),
    },
    // Driven rather than excused, for the reason `app.about` is: it asks
    // nothing of the network, writes nothing, and replaces nothing. What this
    // half covers is that the command reaches its action; that the dialog then
    // shows a document's real properties is `propertiesChecks` below, which
    // drives the whole path through the backend.
    {
      id: "file.properties",
      ...shell("showProperties"),
      read: () => fired.join(","),
    },
    { id: "find.open", ...shell("focusFind"), read: () => fired.join(",") },
    {
      id: "find.matchCase",
      ...shell("toggleSearchOption:matchCase"),
      read: () => fired.join(","),
    },
    {
      id: "find.wholeWord",
      ...shell("toggleSearchOption:wholeWord"),
      read: () => fired.join(","),
    },
    {
      id: "find.regex",
      ...shell("toggleSearchOption:regex"),
      read: () => fired.join(","),
    },
    {
      id: "find.inSelection",
      ...shell("toggleSearchScope"),
      read: () => fired.join(","),
      // Only offered when there is something to scope to, so the probe has to
      // make one --- and on a page with no text there is nothing to select.
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      id: "view.toggleSidebar",
      ...shell("toggleSidebar"),
      read: () => fired.join(","),
    },
    {
      id: "view.showOutline",
      ...shell("showTab:outline"),
      read: () => fired.join(","),
    },
    {
      id: "view.showThumbnails",
      ...shell("showTab:pages"),
      read: () => fired.join(","),
    },
    {
      id: "view.showMarks",
      ...shell("showTab:marks"),
      read: () => fired.join(","),
    },
    {
      id: "view.invertPages",
      ...shell("toggleInvert"),
      read: () => fired.join(","),
    },
    {
      id: "view.zoomIn",
      from: () => viewer.setZoomFixed(1),
      read: zoom,
      moved: (b, a) => Number(a) > Number(b),
    },
    {
      id: "view.zoomOut",
      from: () => viewer.setZoomFixed(1),
      read: zoom,
      moved: (b, a) => Number(a) < Number(b),
    },
    {
      id: "view.actualSize",
      from: () => viewer.setZoomFixed(2),
      read: zoom,
      moved: (b, a) => b !== "1.000" && a === "1.000",
    },
    {
      id: "view.zoomTo",
      argument: "175",
      from: () => viewer.setZoomFixed(1),
      read: zoom,
      moved: (b, a) => b !== "1.750" && a === "1.750",
    },
    {
      id: "view.fitWidth",
      from: () => viewer.setFit("page"),
      read: fit,
      moved: (b, a) => b !== "width" && a === "width",
    },
    {
      id: "view.fitPage",
      from: () => viewer.setFit("width"),
      read: fit,
      moved: (b, a) => b !== "page" && a === "page",
    },
    {
      id: "view.rotateClockwise",
      from: () => viewer.rotateBy(-viewer.rotation),
      read: turn,
      moved: (b, a) => b === "0" && a === "1",
    },
    {
      id: "view.rotateCounterClockwise",
      from: () => viewer.rotateBy(-viewer.rotation),
      read: turn,
      moved: (b, a) => b === "0" && a === "3",
    },
    {
      id: "nav.nextPage",
      from: () => viewer.goToStart(),
      read: page,
      moved: (b, a) => b === "0" && Number(a) > 0,
      unless: reachablePage,
    },
    {
      id: "nav.previousPage",
      from: () => viewer.goToPage(Math.min(1, last)),
      read: page,
      moved: (b, a) => Number(a) < Number(b),
      unless: reachablePage,
    },
    {
      id: "nav.lastPage",
      from: () => viewer.goToStart(),
      read: () => `${viewer.offset.toFixed(0)}/${viewer.maxOffset.toFixed(0)}`,
      moved: (b, a) => b.startsWith("0/") && !a.startsWith("0/"),
      unless: () =>
        viewer.maxOffset > 0 ? null : "the whole document fits on screen",
    },
    {
      id: "nav.firstPage",
      from: () => viewer.goToEnd(),
      read: () => viewer.offset.toFixed(0),
      moved: (b, a) => b !== "0" && a === "0",
      unless: () =>
        viewer.maxOffset > 0 ? null : "the whole document fits on screen",
    },
    {
      // Page 2, and only when it is not the last one. `AGENTS.md` has "the
      // last page cannot reach the top of the viewport": asking a three-page
      // document for its third page scrolls as far as it can and reports the
      // page above it, which reads as the command missing by one.
      id: "nav.goToPage",
      argument: "2",
      from: () => viewer.goToStart(),
      read: page,
      moved: (b, a) => b === "0" && a === "1",
      unless: () =>
        doc.page_count > 2
          ? null
          : `page 2 is the last of ${doc.page_count} and cannot reach the top`,
    },
    {
      // Back and Forward are driven as a pair from a jump this probe makes
      // itself, because the history is empty on arrival and a Back with nothing
      // on the stack is a check that cannot move. `from` does the jump; the
      // command has to undo it.
      //
      // **The page reached is recorded rather than predicted.** Asserting
      // `String(last)` was wrong and the run said so: `goToDestination` leaves a
      // margin above its target, so landing on the last page reports the page
      // above it. Predicting the number tested the margin; recording it tests
      // the round trip, which is what Back is for.
      id: "nav.back",
      from: () => {
        viewer.goToDestination(0, 0);
        viewer.goToDestination(last, 0);
        wentTo = viewer.position.page;
      },
      read: page,
      moved: (before, after) =>
        wentTo > 0 && before === String(wentTo) && after === "0",
      unless: reachablePage,
    },
    {
      id: "nav.forward",
      from: () => {
        viewer.goToDestination(0, 0);
        viewer.goToDestination(last, 0);
        wentTo = viewer.position.page;
        viewer.goBack();
      },
      read: page,
      // Exactly where Back left, not merely "somewhere later". A history that
      // pushed the wrong end still moves the reader forward, and only an exact
      // comparison can tell that from returning to where they were going.
      moved: (before, after) =>
        wentTo > 0 && before === "0" && after === String(wentTo),
      unless: reachablePage,
    },
    {
      // The keyboard's position on the page, which is what these move --- not
      // the scroll: a link already on screen is stepped onto without the view
      // going anywhere, so asserting the page would fail for a working command.
      id: "nav.nextLink",
      from: () => viewer.clearLinkFocus(),
      read: () => String(viewer.linkFocus),
      moved: (before, after) => before === "-1" && after !== "-1",
      unless: withLinks,
    },
    {
      id: "nav.previousLink",
      // Stepped forward twice first, so there is an earlier link to reach ---
      // from the first link of the document Previous correctly does nothing,
      // and a probe set up that way would report a working command as broken.
      from: () => {
        viewer.clearLinkFocus();
        viewer.stepLink(1);
        viewer.stepLink(1);
      },
      read: () => String(viewer.linkFocus),
      moved: (before, after) => before !== "-1" && after !== before && after !== "-1",
      unless: twoLinks,
    },
    {
      // Marks put there by the probe, because no fixture carries one: these are
      // the reader's own, and a corpus of files nobody has opened in tpdf has
      // none by construction. That makes this the one probe pair with no
      // `unless` --- it runs on all fourteen corpora.
      id: "nav.nextMark",
      from: () => {
        // Back to the top, which the link probes above do not need and this
        // does: a link walk starts from the focused link, and a mark walk with
        // no note open starts from *where the reader is*. Left where the
        // previous probe put them --- below the two marks this one plants near
        // the top of page 1 --- "next mark" correctly finds nothing, and the
        // probe read a working command as a dead one.
        viewer.goToStart();
        viewer.closeMark();
        viewer.setMarks([
          syntheticMark(viewer, 4246, "", 0.06),
          syntheticMark(viewer, 4247, "", 0.2),
        ]);
      },
      read: () => String(viewer.markOpen),
      moved: (before, after) => before === "-1" && after !== "-1",
      then: () => {
        viewer.closeMark();
        viewer.setMarks([]);
      },
    },
    {
      id: "nav.previousMark",
      // Walked forward to the second mark first, so there is an earlier one to
      // reach --- from the first mark Previous correctly does nothing, and a
      // probe set up that way would report a working command as broken. The
      // same shape as `nav.previousLink` above, for the same reason.
      from: () => {
        viewer.goToStart();
        viewer.closeMark();
        viewer.setMarks([
          syntheticMark(viewer, 4246, "", 0.06),
          syntheticMark(viewer, 4247, "", 0.2),
        ]);
        viewer.stepMark(1);
        viewer.stepMark(1);
      },
      read: () => String(viewer.markOpen),
      moved: (before, after) =>
        before !== "-1" && after !== before && after !== "-1",
      then: () => {
        viewer.closeMark();
        viewer.setMarks([]);
      },
    },
    {
      id: "edit.selectAll",
      from: () => viewer.clearSelection(),
      read: selection,
      moved: (b, a) => b === "0" && Number(a) > 0,
      unless: withText,
    },
    {
      id: "edit.clearSelection",
      from: () => viewer.selectPage(),
      read: selection,
      moved: (b, a) => Number(b) > 0 && a === "0",
      unless: withText,
    },
    {
      // The page operations. Shell probes, because the model that decides what
      // a page's turn becomes lives in the backend --- what is asserted here is
      // that the command a reader types reaches the right action with the right
      // sign, which is the half that was covered by nothing. That a turn then
      // reaches the tiles and the text layer is asserted separately, against a
      // real viewer, by `pageRotationChecks`.
      id: "edit.rotatePageClockwise",
      ...shell("rotatePage:1"),
      read: () => fired.join(","),
    },
    {
      id: "edit.rotatePageCounterClockwise",
      ...shell("rotatePage:-1"),
      read: () => fired.join(","),
    },
    {
      // The only command with no keyboard binding, so the palette is not one of
      // two routes to it --- it is the route. Driving it from there is therefore
      // the whole of its wiring, which is why it is here rather than left to
      // `appcommands.test.ts`.
      id: "edit.deletePage",
      ...shell("deletePage"),
      read: () => fired.join(","),
    },
    {
      // Palette-only as well, for the reason `edit.deletePage` is, so the same
      // argument puts it here.
      id: "edit.insertBlankPage",
      ...shell("insertBlankPage"),
      read: () => fired.join(","),
    },
    // Every named size, aimed separately, for the colours' and the stamps'
    // reason exactly: five commands built by one `map`, all calling one action
    // with an argument, so a copy-and-paste that left every entry passing "a4"
    // would give a reader five commands that all insert A4 and no single probe
    // could see it. `insertSizedPage:<name>` is what says each reaches its own.
    // Built from `PAGE_SIZE_NAMES` so a size added to the table and left
    // unclassified here reddens the sweep.
    ...PAGE_SIZE_NAMES.map((name) => ({
      id: `edit.insertPage.${name}`,
      ...shell(`insertSizedPage:${name}`),
      read: () => fired.join(","),
    })),
    {
      // Palette-only, for `edit.insertBlankPage`'s reason. What this reaches is
      // the action; the dialog, the backend's open of a second file and the
      // pages that arrive are `tabs_check.py --phase import`'s.
      id: "edit.insertPages",
      ...shell("importPages"),
      read: () => fired.join(","),
    },
    {
      // Needs a selection, like `find.inSelection` above, and for a sharper
      // reason: its `enabled` guard is the only one in the application that
      // reads the *viewer's* state rather than the shell's. A run with nothing
      // selected does not fail here -- the command is simply not in the palette,
      // and `runByTitle` would report it missing, which is why the selection is
      // made in `from` rather than assumed from whatever the last phase left.
      id: "edit.highlightSelection",
      ...shell("markSelection:highlight"),
      read: () => fired.join(","),
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      // No `from` and no `unless`, which is the whole of what separates it from
      // the three above: a comment is dropped on the page rather than made out
      // of a selection, so it needs no text and there is no corpus on which it
      // is inapplicable. The recorded value carries `here` --- the palette
      // passes no point --- which is what says the palette route reached the
      // no-pointer branch rather than a stale one.
      id: "edit.addComment",
      ...shell("addComment:here"),
      read: () => fired.join(","),
    },
    {
      // Reaches the action, which *arms* the tool rather than drawing anything
      // --- so this says the command is registered and wired and nothing about
      // the gesture. The drag itself is covered by `drag.test.ts` and by the
      // pointer phase below, which is the right split: what a real window adds
      // here is that the command exists in the palette at all, and that is
      // exactly what the shell probe reads.
      id: "edit.drawBox",
      ...shell("drawBox"),
      read: () => fired.join(","),
    },
    { id: "edit.editText", ...shell("editText"), read: () => fired.join(",") },
    { id: "edit.addSignature", ...shell("signature"), read: () => fired.join(",") },
    {
      // **The one crop command that can be driven here**, and that is why it is
      // a probe while its two neighbours are excused: it arms and returns, so
      // its outcome is a field on the viewer rather than a chain of IPC replies
      // the probe framework's frame-loop settle cannot wait for.
      //
      // What it reads is the same half every tool command's probe reads --- the
      // command exists in the palette and reaches its own action. The gesture
      // it arms is covered by `viewercrop.test.ts` against a fake DOM, and the
      // mapping it ends in by `cropCommandChecks` against this backend.
      id: "edit.cropToDrag",
      ...shell("cropPage:drag"),
      read: () => fired.join(","),
    },
    {
      // The crop's twin, and aimed separately for the reason the three drawing
      // tools above are aimed separately and more so: these two arm the same
      // drag with opposite outcomes, so a copy-and-paste that left this one
      // calling `cropPage` would crop a page a reader asked to redact --- and
      // the palette entry would look correct throughout. What it reads is that
      // the command exists and reaches its *own* action; that the action arms
      // the redaction tool rather than the crop is `viewercrop.test.ts`'s,
      // which can build a viewer and read `redactArmed` back.
      id: "edit.redactRegion",
      ...shell("redactRegion"),
      read: () => fired.join(","),
    },
    {
      // Aimed separately from the drag above for that entry's own reason, and
      // one more that is this command's: it is the only redaction command with
      // a `hasSelection` guard, so a copy-and-paste that left it calling
      // `redactRegion` would arm a drag on a reader who had already said which
      // words they meant --- and the palette entry would look correct.
      id: "edit.redactSelection",
      ...shell("redactSelection"),
      read: () => fired.join(","),
      // Makes its own selection and skips a page that has none to make, exactly
      // as the three marking commands above do --- and for the reason
      // `edit.highlightSelection`'s comment already gives: a selection left by
      // an earlier phase is not a precondition, it is a coincidence.
      //
      // It had neither, and inherited `find.inSelection`'s. That works wherever
      // page 1 has text and nowhere else: on `vector-heavy` the neighbour is
      // *skipped* by its own `unless`, so its `from` never runs, nothing is
      // selected, the command is withheld, and the check reads `"Redact
      // selection" highlighted "" of 0` --- which looks like a command that was
      // never registered. Found by the step that sweeps the release bundle over
      // both documents, since no runner in the mutation table opens a corpus
      // with no text.
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      // The third of the three, aimed separately for the same reason as the
      // second: all three are one word apart in the source, and the one that
      // sweeps the whole document is the one whose mis-wiring costs most.
      id: "edit.redactMatches",
      ...shell("redactMatches"),
      read: () => fired.join(","),
      // Guarded on there being matches to mark, and nothing before this point
      // has searched --- the find phase runs *after* this loop. So the probe
      // makes its own precondition, as `find.inSelection` does for a selection.
      //
      // Awaited, which is why `from` may return a promise: a search completes
      // off the frame loop, so the loop's `settle(() => viewer.idle)` would let
      // the palette be opened against a document with no matches yet. That is
      // exactly what shipped --- the command was withheld, the palette matched
      // nothing, and the check read `highlighted "" of 0`, which looks like a
      // missing registration rather than a missing precondition.
      from: async () => {
        const needle = firstPage ? pickNeedle(firstPage.codes) : null;
        if (!needle) return;
        viewer.search(needle);
        await settle(() => !viewer.searching && viewer.searchMatches.length > 0);
      },
      unless: withText,
      // Put back, for the reason the two mark probes have one: every probe
      // after this and every phase after this one would otherwise run against a
      // document carrying a search nobody asked for.
      then: () => viewer.clearSearch(),
    },
    {
      // The ellipse, aimed separately for the reason the note below it gives
      // about the freehand tool: it arms the same primitive as the box with a
      // different argument, which is exactly the shape where a copy-and-paste
      // leaves both arming a `square`. What this reads is that the command
      // exists in the palette and reaches its *own* action --- the argument is
      // chosen in `App.svelte`, which no window check can reach, and the
      // `wiring` gate is what covers that half.
      id: "edit.drawEllipse",
      ...shell("drawEllipse"),
      read: () => fired.join(","),
    },
    {
      // The third drag tool, aimed separately for the reason the two above it
      // are: three commands now arm the same primitive with three different
      // arguments, which is exactly where a copy-and-paste leaves two of them
      // placing a `square`.
      id: "edit.addTextBox",
      ...shell("drawTextBox"),
      read: () => fired.join(","),
    },
    // The three form field commands arm one drag with three kinds, which is
    // where a copy leaves two of them placing the same field.
    { id: "edit.addTextField", ...shell("drawField:text"), read: () => fired.join(",") },
    { id: "edit.addMultilineField", ...shell("drawField:multiline"), read: () => fired.join(",") },
    { id: "edit.addCheckbox", ...shell("drawField:checkbox"), read: () => fired.join(",") },
    { id: "edit.addDropdown", argument: "Yes; No", ...shell("drawField:dropdown"), read: () => fired.join(",") },
    { id: "edit.addRadio", argument: "Payment", ...shell("drawRadio:Payment"), read: () => fired.join(",") },
    { id: "edit.addSignatureField", ...shell("drawField:signature"), read: () => fired.join(",") },
    {
      // The freehand tool, aimed separately for the reason the three mark kinds
      // below are: two commands that arm the same primitive with a different
      // argument is exactly the shape where a copy-and-paste leaves both arming
      // the box, and a probe that only checked "a tool was armed" could not see
      // it. `App.svelte` is where the argument is chosen, which this cannot
      // reach --- so what it says is that the command exists and reaches its own
      // action, which is the defect that shipped with the box.
      id: "edit.draw",
      ...shell("draw"),
      read: () => fired.join(","),
    },
    {
      // The eraser, aimed separately for the reason above it: three commands
      // now arm a tool and the whole family is where a copy-and-paste leaves
      // two of them arming the same one. What this says is that the command
      // exists and reaches its own action; that the action arms the *eraser*
      // rather than the pen is `viewerdraw.test.ts`'s, which can build a viewer
      // and read `eraseArmed` back.
      id: "edit.erase",
      ...shell("erase"),
      read: () => fired.join(","),
    },
    // Every colour, aimed separately, for the reason the three selection marks
    // below are aimed separately and more so: these are seven commands built by
    // one `map`, so the copy-and-paste this file worries about would be a whole
    // family taking the wrong argument at once --- and `setMarkColor:<id>` is
    // what says each reaches its own. Built from `PALETTE` because a colour
    // added there and unclassified here reddens the sweep, which is what the
    // classification is for.
    ...PALETTE.map((entry) => ({
      id: `edit.color.${entry.id}`,
      ...shell(`setMarkColor:${entry.id}`),
      read: () => fired.join(","),
    })),
    // Every nib, aimed separately, for the colours' reason exactly: four
    // commands built by one `map`, all calling one action with an argument, so
    // a copy-and-paste leaving every entry passing "medium" would give a reader
    // four commands that all draw at the default and no single probe could see
    // it. Built from `NIBS` so a nib added there and unclassified here reddens
    // the sweep.
    ...NIBS.map((entry) => ({
      id: `edit.nib.${entry.id}`,
      ...shell(`setNib:${entry.id}`),
      read: () => fired.join(","),
    })),
    // Every stamp, aimed separately, for the colours' reason exactly: four
    // commands built by one `map`, all calling one action with an argument, so
    // a copy-and-paste that left every entry passing "approved" would give a
    // reader four commands that all stamp APPROVED and no single probe could
    // see it. `stamp:<name>` is what says each reaches its own.
    ...(["approved", "confidential", "draft", "final"] as const).map((name) => ({
      id: `edit.stamp.${name}`,
      ...shell(`stamp:${name}`),
      read: () => fired.join(","),
    })),
    {
      // The two other kinds, aimed separately. One action taking an argument,
      // which is the shape this file's own note about `movePage` warns about:
      // a copy-and-paste that left all three passing "highlight" gives a reader
      // a Strike out that highlights, and no single probe can see it. The
      // `moved` expectation carries the argument, so each names its own.
      id: "edit.underlineSelection",
      ...shell("markSelection:underline"),
      read: () => fired.join(","),
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      id: "edit.strikeoutSelection",
      ...shell("markSelection:strikeout"),
      read: () => fired.join(","),
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      // The fourth, and the one most exposed to the copy-and-paste the note
      // above describes: it sits directly beneath the underline in the menu, in
      // the palette and in this list, and an entry left passing "underline"
      // gives a reader a Squiggly that draws a rule. The expectation carries
      // the argument, so this names its own.
      id: "edit.squigglySelection",
      ...shell("markSelection:squiggly"),
      read: () => fired.join(","),
      from: () => viewer.selectPage(),
      unless: withText,
    },
    {
      // The other viewer-guarded command, and its `from` has to *make* the
      // state the guard reads: a mark, and its note open. Nothing else in this
      // phase leaves one open, which is deliberate --- the sweep below asserts
      // that a document alone does not offer this, and a note left open by an
      // earlier probe would make that read as a broken guard.
      id: "edit.removeMark",
      ...shell("removeMark"),
      read: () => fired.join(","),
      from: () => {
        // On the page the mark is on, because the frame loop closes a note
        // whose page has scrolled out of sight -- and the probes before this
        // one leave the viewer wherever they finished.
        viewer.goToPage(0);
        const mark = syntheticMark(viewer, 4243, "");
        viewer.setMarks([mark]);
        viewer.showMark(mark.id);
      },
    },
    {
      // The third viewer-guarded command, and its `from` makes its own state
      // for `edit.removeMark`'s reason exactly: a region, and that region
      // picked. Nothing else in this phase leaves a pick, which the sweep below
      // depends on --- a pick left by an earlier probe would make "a document
      // alone does not offer this" read as a broken guard.
      //
      // The region is put into the overlay directly rather than dragged out,
      // because a drag is the *tool's* behaviour and `viewercrop.test.ts` owns
      // that; what this reads is that the palette entry reaches this action and
      // no other. Aimed separately from the three marking commands above for
      // the reason they are aimed separately from each other: this one is the
      // way back off, and a copy-and-paste that left it calling `redactRegion`
      // would arm a drag on a reader asking to undo one.
      id: "edit.removeRedaction",
      ...shell("removeRedaction"),
      read: () => fired.join(","),
      from: () => {
        viewer.goToPage(0);
        const region = syntheticRedaction(viewer, 4244);
        viewer.setRedactions([region]);
        viewer.pickRedaction(region.id);
      },
      // The stub records the command; it does not remove its synthetic region.
      then: () => viewer.setRedactions([]),
    },
    {
      // Palette-only as well, and the two are worth aiming at separately: they
      // are one action taking a sign, so a copy-and-paste that left both at -1
      // gives a reader a "move down" that moves up, which is not a wiring
      // failure any single probe can see.
      id: "edit.movePageUp",
      ...shell("movePage:-1"),
      read: () => fired.join(","),
    },
    {
      id: "edit.movePageDown",
      ...shell("movePage:1"),
      read: () => fired.join(","),
    },
    {
      id: "file.saveCopy",
      ...shell("saveCopy"),
      read: () => fired.join(","),
    },
    {
      // The two redaction writes, driven for the reason `file.saveCopy` is:
      // every action in this harness is a recorder, so what runs is a push onto
      // `fired` and no file is written. `file.save` is undriven despite that,
      // and the difference is its guard rather than its danger --- it is
      // withheld on an unedited document, so there is nothing in the palette to
      // press.
      //
      // Aimed separately, because they are one word apart in the source and the
      // one that writes over the reader's own file is the costlier to mis-wire
      // --- the same argument the three ways of marking a redaction carry above.
      id: "file.redactCopy",
      ...shell("redactCopy"),
      read: () => fired.join(","),
    },
    {
      id: "file.redactRasterCopy",
      ...shell("redactRasterCopy"),
      read: () => fired.join(","),
    },
    {
      id: "file.recogniseText",
      ...shell("recogniseText"),
      read: () => fired.join(","),
    },
    {
      id: "file.recogniseTextLanguage",
      ...shell("chooseRecognitionLanguage"),
      read: () => fired.join(","),
    },
    {
      id: "file.findHiddenText",
      ...shell("findHiddenText"),
      read: () => fired.join(","),
    },
    {
      id: "file.compress",
      ...shell("compressCopy"),
      read: () => fired.join(","),
    },
    {
      id: "file.protect",
      ...shell("protectCopy"),
      read: () => fired.join(","),
    },
    {
      id: "file.unprotect",
      ...shell("unprotectCopy"),
      read: () => fired.join(","),
    },
    {
      id: "file.redactDocument",
      ...shell("redactDocument"),
      read: () => fired.join(","),
    },
    {
      // The redactions tab, driven like the four tabs above it. It reaches
      // `showTab` with its own argument, which is the half a window check can
      // read; that the panel then lists the pending regions is
      // `redactlist.test.ts`'s.
      id: "view.showRedactions",
      ...shell("showTab:redactions"),
      read: () => fired.join(","),
    },
    {
      // Driven with a real argument, because the value is where this command's
      // work is: `1-2` has to survive the palette's own input, reach
      // `parsePageRange` against this document's page count, and arrive as the
      // two slots the action is handed. A probe with no argument would check
      // that the command is registered and nothing else.
      //
      // `1-2` rather than `1`, so the joined form is `0+1` --- a single slot
      // would read the same whether the parser produced one page or dropped
      // one. That costs a skip on the two single-page corpora, which is the
      // trade `twoPages` states.
      id: "file.extractPages",
      argument: "1-2",
      ...shell("extractPages:0+1"),
      read: () => fired.join(","),
      unless: twoPages,
    },
    {
      // `1` is the only cut every multi-page corpus can make: cutting after the
      // first page works on two pages and on 775, where `3` is out of range on
      // anything shorter than four. The expected value is the *groups*, so a
      // boundary off by one changes it -- `0|1+2` and `0+1|2` are both two
      // groups covering three pages, and only the grouping tells them apart.
      //
      // Shares `twoPages` with extract above rather than declaring its own: a
      // one-page document has nowhere to cut, which is the same fact the range
      // needs and the same skip reason a reader should see.
      id: "file.splitDocument",
      argument: "1",
      // The whole expected value, derived from *this document's* length rather
      // than pinned: cutting after page 1 of an N-page corpus gives `0` and
      // then every remaining slot, which is `1+2+3` on a four-page fixture and
      // 774 numbers on the long one. Derived from `doc.page_count`, which is
      // the document's own report and not the parser's --- the same source
      // `twoPages` reads, and the reason this is an assertion rather than a
      // restatement of what `parseSplitPoints` did.
      ...shell(
        `splitDocument:0|${Array.from({ length: doc.page_count - 1 }, (_, at) => at + 1).join("+")}`,
      ),
      read: () => fired.join(","),
      unless: twoPages,
    },
    {
      // Driven rather than excused, for `file.saveCopy`'s reason: the action is
      // a recorder here, so the platform's file chooser is never opened and
      // nothing is written. What this covers is that the command reaches its
      // action --- which is the half that shipped broken once already, when a
      // callback was declared, fired and never wired into the object literal
      // that joins the two.
      //
      // **No argument**, unlike its neighbour above, and that is the command
      // rather than a shortcut: a merge is given files by a dialog, so there is
      // no value for the palette to carry and nothing for a probe to put in it.
      id: "file.mergeDocuments",
      ...shell("mergeDocuments"),
      read: () => fired.join(","),
    },
    {
      id: "file.fromPictures",
      ...shell("fromPictures"),
      read: () => fired.join(","),
    },
    {
      // Driven for `file.mergeDocuments`' reason: the action is a recorder, so
      // no chooser opens, no OS key is asked and nothing is written. What this
      // covers is the palette reaching the action; the sequence behind it is
      // `signing.test.ts`, and the real window is `tabs_check.py`'s to drive.
      id: "file.signDocument",
      ...shell("signDocument"),
      read: () => fired.join(","),
    },
    {
      // The same, into a signature field: reaching the action is what is
      // covered here, and the field it signs is `signfield.test.ts`'s.
      id: "file.signField",
      ...shell("signField"),
      read: () => fired.join(","),
    },
    {
      // Reaching the action is what is covered, as for signing; no file dialog
      // opens. Linux only, where the command is enabled.
      id: "file.addCertificateFile",
      ...shell("addCertificateFile"),
      read: () => fired.join(","),
      unless: () => (isLinux() ? null : "certificate files are added to tpdf on Linux only"),
    },
    {
      // Driven for `file.signDocument`'s reason: the action is a recorder, so
      // no dialog opens, nobody is asked for anything and nothing is written.
      // What this covers is the palette reaching the action; the sequence
      // behind it is `validationdata.test.ts`.
      id: "file.addValidationData",
      ...shell("addValidationData"),
      read: () => fired.join(","),
    },
    {
      // Both journal commands are withheld unless there is something to act on,
      // so `from` has to grant it --- and granting it is what makes the probe a
      // statement about the wiring rather than about the guard. The guard's own
      // other direction is in `appcommands.test.ts`, where an empty journal
      // keeps them out of the palette entirely.
      id: "edit.undo",
      from: () => {
        canUndo = true;
      },
      ...shell("undoEdit"),
      read: () => fired.join(","),
    },
    {
      id: "edit.redo",
      from: () => {
        canRedo = true;
      },
      ...shell("redoEdit"),
      read: () => fired.join(","),
    },
  ];

  // The ones the list above cannot drive, each with the reason it cannot, so
  // that "not covered" is a decision in the table rather than an absence. Not
  // counted in this comment: the count belongs to the entries, and saying it
  // here went stale the first time one was added.
  const undriven: Record<string, string> = {
    "edit.fillForm": "Driven against the real application by tabs_check.py --phase forms.",
    "file.close": "tab ownership is exercised by the open check's tabs phase",
    "file.closeAll": "closes every tab, which would end the run; no automated check drives it yet",
    "edit.fieldBorderOn": "a stored preference; fieldnames.test.ts and appcommands.test.ts cover the pair",
    "edit.fieldBorderOff": "a stored preference; tabs_check.py --phase fields drives it",
    "edit.formEditOn": "needs a document with a form; appcommands.test.ts and savedfields.test.ts",
    "edit.formEditOff": "needs the mode on; appcommands.test.ts",
    "edit.fieldProperties": "needs a field of the file picked; appcommands.test.ts, fieldprops.test.ts, and the fields phase",
    "edit.duplicate": "needs marks picked; duplicate.test.ts, and the fields phase",
    "edit.tabOrder": "needs a form field; appcommands.test.ts, save/tests.rs, and the fields phase",
    "edit.alignLeft": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.alignCenter": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.alignRight": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.alignTop": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.alignMiddle": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.alignBottom": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.distributeAcross": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.distributeDown": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.sameWidth": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.sameHeight": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.sameSize": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.centerOnPage": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "edit.middleOnPage": "needs marks picked; viewerarrange.test.ts, and the fields phase",
    "file.reopenLastTabs": "needs a previous launch; the session check's tabs phase drives it",
    "file.reopenTabsAtLaunch": "a stored preference; the session check's tabs phase covers the launch",
    "file.reopenLastDocumentAtLaunch": "a stored preference; appcommands.test.ts covers the pair",
    "file.clearRecents": "empties the session's list, and this check must not; appcommands.test.ts and session.rs",
    "view.tabLabelsLarger": "a stored display preference; tablabels.test.ts covers stepping and storage",
    "view.tabLabelsSmaller": "a stored display preference; tablabels.test.ts covers stepping and storage",
    "view.tabLabelsDefault": "a stored display preference; tablabels.test.ts covers stepping and storage",
    "view.nextTab": "tab ownership is exercised by the open check's tabs phase",
    "view.previousTab": "tab ownership is exercised by the open check's tabs phase",
    "view.sideBySide": "needs a second open document; the open check's sides phase drives it",
    "view.moveToOtherSide": "needs a second open document; the open check's sides phase drives it",
    "view.bothSides": "mounts a second view of the document; the open check's views phase drives it",
    "view.switchSides": "needs two documents side by side; the open check's sides phase drives it",
    "view.focusOtherSide": "needs two documents side by side; the open check's sides phase drives it",
    "view.syncScrolling": "needs two documents side by side; the open check's sides phase drives it",
    "find.next": "needs a live search with more than one match",
    "find.previous": "needs a live search with more than one match",
    "edit.copy": "its outcome is the system clipboard",
    // Driving it would reopen the document, which replaces the viewer, the
    // text cache and the tile state that every later check reads --- so it
    // cannot run here without ending the run. Its wiring is covered by
    // `appcommands.test.ts` instead: that it reaches its own action and no
    // other, is withheld with no document, and ranks first for its own name.
    "file.reload": "it reopens the document, discarding the state later checks read",
    // Withheld here because this fixture's mode is `ask`, and each choice is
    // offered only while it is not the current one. `appcommands.test.ts`
    // drives all three from every starting mode.
    "file.onDiskChange.ask": "it is the fixture's current mode, so it is withheld",
    // Driving it would write the working document over `testdata/<corpus>.pdf`
    // --- the file this run and every other run of this harness is reading ---
    // and then reopen it, which `file.reload` above already rules out on its
    // own. Copying the fixture first would make the phase a statement about a
    // document no other check has seen. Its wiring is covered by
    // `appcommands.test.ts`, the write itself by `save.rs`'s tests, and the
    // ordering the two halves need by the mutations named there.
    "file.save": "it would write over the corpus fixture the run is reading",
    // Driving either would reach the network from a check that is otherwise
    // entirely offline, and the install one would replace the running binary
    // mid-run. Their wiring is covered by `appcommands.test.ts` and their
    // behaviour by `update.test.ts`, which fakes the plugin; what neither
    // covers is a real endpoint and a real signature, and `BUILD.md` schedules
    // that as a manual step because it needs a published release to check
    // against.
    "app.checkForUpdates": "it would reach the network from an offline check",
    "app.disableAutomaticUpdates": "the persisted setting is covered by update tests",
    "app.enableAutomaticUpdates": "the persisted setting is covered by update tests",
    "app.installUpdate": "it would replace the running binary mid-run",
    // And this one would end the process the check is running in, which is not
    // a failure the harness could report: the transcript it is writing would
    // stop mid-sentence and `viewer_check.py` would read that as a crash. Its
    // guard is asserted in both directions in `appcommands.test.ts`, and what
    // it does once pressed --- settle, count the dirty tabs, ask, respect a no
    // --- is `update.test.ts`, which fakes the relaunch.
    "app.restartForUpdate": "it would end the process this check is writing its transcript from",
    // Both write outside the application --- a link in `/usr/local/bin` --- and
    // on a fresh Mac ask for an administrator password, which a check must not
    // do to whoever is at the machine. The decision is `clitool.rs`'s tests,
    // over a scratch directory; the wiring is `appcommands.test.ts`.
    "app.installCommandLineTool": "it writes /usr/local/bin and can raise an administrator prompt",
    "app.uninstallCommandLineTool": "it removes /usr/local/bin/tpdf and can raise an administrator prompt",
    // It would change which application opens PDFs on the machine running the
    // check. The sentence is `defaultapp.rs`'s test; the wiring is `appcommands.test.ts`.
    "app.makeDefaultPdfApp": "it changes the machine's default application for PDFs",
    // Driving either from the palette would time the layout against a chain of
    // two IPC round trips --- measure the ink, then ask what size the page
    // becomes --- and the probe framework's settle is a frame-loop wait, not a
    // reply wait. Their wiring is covered by `appcommands.test.ts`'s sweep over
    // every registered command, and the commands underneath them by
    // `cropCommandChecks` above, which drives all three against this backend.
    "edit.cropToContent": "its outcome is two IPC replies the probe cannot wait for",
    "edit.resetCrop": "its outcome is an IPC reply the probe cannot wait for",
    // **The three foreign-comment commands, and one reason for all three.**
    // Each needs a comment popup open on a comment that has an object of its
    // own, and this phase runs before the comments phase and on corpora that
    // may have no comments at all. A probe here would pass wherever the
    // comments happened to exist and say nothing where they did not, which is
    // the shape `docs/TRAPS.md` records as a check borrowing its neighbour's
    // precondition. Both directions of each guard are asserted in
    // `appcommands.test.ts`, which needs no corpus.
    //
    // ⚠ **The first two were registered on 2026-08-29 and left out of this
    // table**, so the classification check below was red from that day and
    // nothing said so --- this harness needs a screen and runs by hand. That is
    // what `scripts/check_classified.py` now catches on every commit.
    "edit.editForeignMark": "needs an open comment popup, which this phase has no corpus for",
    "edit.replyToComment": "needs an open comment popup, which this phase has no corpus for",
    "edit.deleteComment": "needs an open comment popup, which this phase has no corpus for",
    // Offered only while a file chosen by `edit.insertPages` is open and
    // waiting, which needs the open dialog this harness cannot answer.
    // `tabs_check.py --phase import` drives it past the dialog, through the
    // palette, answered and dismissed; `appcommands.test.ts` covers the guard
    // and the argument in both directions.
    "edit.insertPages.range": "needs a file waiting to be inserted; the import window phase drives it",
    // Offered only while `file.recogniseTextLanguage` holds the machine's list
    // of languages, which these recording actions never fetch.
    // `appcommands.test.ts` covers the guard and the argument in both
    // directions, and `ocrlanguage.test.ts` what an answer means.
    "file.recogniseTextLanguage.choice":
      "needs this machine's list of languages to be held; no window phase drives it yet",
  };

  const registered = registry.all().map((command) => command.id);
  const classified = new Set([
    ...probes.map((p) => p.id),
    ...Object.keys(undriven),
  ]);
  const unclassified = registered.filter((id) => !classified.has(id));
  const stale = [...classified].filter((id) => !registered.includes(id));
  check(
    "every registered command is classified, and every classification is registered",
    registered.length > 0 && unclassified.length === 0 && stale.length === 0,
    `${registered.length} registered, ${probes.length} driven, ` +
      `${Object.keys(undriven).length} not driven; ` +
      `unclassified [${unclassified.join(", ")}], stale [${stale.join(", ")}]`,
  );

  for (const probe of probes) {
    const title = titleOf(probe.id);
    if (!title) {
      check(`${probe.id} runs from the palette`, false, "not registered");
      continue;
    }
    const cannot = probe.unless?.();
    if (cannot) {
      skip(`${probe.id} runs from the palette`, cannot);
      continue;
    }
    await probe.from?.();
    await settle(() => viewer.idle);
    fired = [];
    const before = probe.read();
    const why = runByTitle(title, probe.argument);
    await settle(() => viewer.idle);
    const after = probe.read();
    check(
      `${probe.id} runs from the palette`,
      why === "" && probe.moved(before, after),
      why === "" ? `"${title}": ${before} -> ${after}` : why,
    );
    probe.then?.();
  }

  // Find-next and find-previous, once there is something to step through. The
  // needle comes from the document rather than from here, and the two are
  // skipped together with the reason when it does not yield two matches.
  viewer.goToStart();
  await settle(() => viewer.idle);
  const needle = firstPage ? pickNeedle(firstPage.codes) : null;
  if (needle) {
    viewer.search(needle);
    // Guarded, because the predicate cannot hold on a document with no text and
    // an unguarded wait would then spend the full 30 s timeout doing nothing ---
    // the trap `AGENTS.md` records as a wait for a condition that cannot hold.
    await settle(() => !viewer.searching && viewer.searchMatches.length > 1);
  }
  if (!needle || viewer.searchMatches.length < 2) {
    const why = `${undriven["find.next"]} ("${needle ?? "no needle"}" found ${viewer.searchMatches.length})`;
    skip("find.next runs from the palette", why);
    skip("find.previous runs from the palette", why);
  } else {
    viewer.showMatch(0);
    await settle(() => viewer.idle);
    const atZero = viewer.matchIndex;
    const nextWhy = runByTitle(titleOf("find.next"));
    await settle(() => viewer.idle);
    const afterNext = viewer.matchIndex;
    check(
      "find.next runs from the palette",
      nextWhy === "" && atZero === 0 && afterNext === 1,
      nextWhy === ""
        ? `match ${atZero} -> ${afterNext} of ${viewer.searchMatches.length}`
        : nextWhy,
    );
    const prevWhy = runByTitle(titleOf("find.previous"));
    await settle(() => viewer.idle);
    check(
      "find.previous runs from the palette",
      prevWhy === "" && viewer.matchIndex === 0,
      prevWhy === "" ? `match ${afterNext} -> ${viewer.matchIndex}` : prevWhy,
    );
    viewer.clearSearch();
  }

  // Copy. The OS clipboard cannot be read back here, so what is asserted is
  // that the command reaches the copy path carrying the selection --- the write
  // itself is intercepted, and this says nothing about the system clipboard.
  const clipboard: { writeText?: (text: string) => Promise<void> } =
    navigator.clipboard ?? {};
  const realWrite = clipboard.writeText?.bind(navigator.clipboard);
  let written: string | null = null;
  let installed = false;
  try {
    clipboard.writeText = (text: string) => {
      written = text;
      return Promise.resolve();
    };
    installed = clipboard.writeText !== realWrite;
  } catch {
    installed = false;
  }
  if (!installed || !hasText) {
    skip(
      "edit.copy runs from the palette",
      installed
        ? "page 1 has no extractable text"
        : "the clipboard write could not be intercepted",
    );
  } else {
    viewer.goToStart();
    viewer.selectPage();
    await settle(() => viewer.idle);
    const selected = viewer.selectedText;
    const copyWhy = runByTitle(titleOf("edit.copy"));
    await settle(() => written !== null);
    check(
      "edit.copy runs from the palette",
      copyWhy === "" && selected.length > 0 && written === selected,
      copyWhy === ""
        ? `selected ${selected.length} characters, copied ${written === null ? "nothing" : String(written).length}`
        : copyWhy,
    );
    viewer.clearSelection();
    if (realWrite) clipboard.writeText = realWrite;
  }

  // The guard every command but "Open document" carries: without a document,
  // only "Open document" may be offered, and with one, everything that needs
  // nothing further must come back. The second half is the control --- a
  // palette that listed nothing at all would satisfy the first.
  //
  // A **second registry** rather than taking the document away from this one.
  // The obvious version holds the viewer in a `let` the actions close over and
  // sets it to null for the duration, and that version produced a reading this
  // file could not explain: `attached === null` and `actions.viewer() === null`
  // disagreed inside one expression, and which way round they disagreed changed
  // between runs of the same binary. Nothing static accounts for it --- one
  // declaration, one closure, one call site, and the compiled bundle reads
  // correctly --- so rather than ship a check whose own mechanism is not
  // understood, the mechanism is gone: a registry built with a viewer that is
  // null *by construction* has nothing to observe at the wrong moment. See
  // `docs/TRAPS.md`.
  //
  // Commands that a document alone does not enable are declared rather than
  // subtracted, so the next one of them turns this red instead of being
  // absorbed by a count.
  //
  // `edit.highlightSelection` joined the list on 2026-08-18 and turned this
  // check red on the way, which is the arrangement working: it needs a
  // selection, exactly as `find.inSelection` does, and a check that subtracted
  // a count would have absorbed it in silence.
  const NEEDS_MORE_THAN_A_DOCUMENT = [
    // No tabs from an earlier launch to reopen, and reopening all of them is
    // this fixture's launch preference, so its other half is not offered. Both
    // joined the registry with the tab restore and not this list; the first
    // run of this check after them, on 2026-10-04, found it.
    "file.reopenLastTabs",
    "file.reopenLastDocumentAtLaunch",
    // This fixture remembers no documents, so there is nothing to clear.
    "file.clearRecents",
    // This fixture has no other document to put beside the open one, and so
    // no two sides to switch or to go between. Declared with the commands.
    "view.sideBySide",
    "view.moveToOtherSide",
    "view.switchSides",
    "view.focusOtherSide",
    "view.syncScrolling",
    // This fixture's disk-change mode is `ask`, so that choice is the one not
    // offered. In registry order, as the whole list is.
    "file.onDiskChange.ask",
    // This fixture has automatic checking enabled, so only its disable command
    // is offered. Opening a document does not change that preference.
    "app.enableAutomaticUpdates",
    "app.installUpdate",
    // Needs an update already applied, which is a state a check run can never
    // be in: nothing here installs one. In registry order, immediately after
    // the install it follows.
    "app.restartForUpdate",
    "find.inSelection",
    // New text fields get a line round them here, so turning that on is not
    // offered. Found missing with the two above.
    "edit.fieldBorderOn",
    // The fixture's fake says the document has no form, and no field is being
    // changed.
    "edit.formEditOn",
    "edit.formEditOff",
    "edit.tabOrder",
    "edit.fieldProperties",
    "edit.duplicate",
    // Nothing is picked.
    "edit.alignLeft",
    "edit.alignCenter",
    "edit.alignRight",
    "edit.alignTop",
    "edit.alignMiddle",
    "edit.alignBottom",
    "edit.distributeAcross",
    "edit.distributeDown",
    "edit.sameWidth",
    "edit.sameHeight",
    "edit.sameSize",
    "edit.centerOnPage",
    "edit.middleOnPage",
    "edit.highlightSelection",
    "edit.underlineSelection",
    "edit.strikeoutSelection",
    "edit.squigglySelection",
    // The three a comment has to be OPEN for, guarded on the popup rather than
    // on a selection: a reader names the comment they mean by opening it, and a
    // document nobody has clicked into has none open. All three landed on
    // 2026-08-29 and 2026-08-30 without joining this list, which turned the
    // check red on a clean tree -- the third time in this file's history, and
    // the two comments below already say why it goes unread. **In registry
    // order**, which the check compares by `join`.
    "edit.editForeignMark",
    "edit.deleteComment",
    "edit.replyToComment",
    // Guarded on a note being open, which is how a reader names the mark they
    // mean. A document with no marks in it offers nothing to remove.
    //
    // **In registry order, which the check below compares by `join`.** The
    // three entries here were written in the order somebody thought of them and
    // put `edit.removeMark` last, where the registry has it first --- so the
    // check was red on a clean tree, and stayed red unread because the harness
    // that runs this baseline prints only its first three failures.
    "edit.removeMark",
    // Joined on 2026-08-27 with the command itself, rather than after the
    // harness went red for it. Nothing runs this harness automatically -- it
    // needs a real window, so it is not a gate -- and the note above records
    // what that costs: a red that goes unread until somebody happens to look.
    // The declaration is part of adding a guarded command, not a repair.
    "edit.redactSelection",
    // Guarded on there being search matches, which a document nobody has
    // searched has none of. Declared with the command, as the one above was.
    "edit.redactMatches",
    // Guarded on a region being picked, which is how a reader names the region
    // they mean --- a document nobody has right-clicked a region on has none.
    // Declared with the command, as the two above were, rather than after the
    // harness went red for it.
    "edit.removeRedaction",
    // Guarded on a file waiting for its pages to be named, which only
    // `edit.insertPages` leaves behind. It shipped without its entry here and
    // the Windows run was the first to read the red, on both corpora.
    "edit.insertPages.range",
    // Guarded on the document being edited, which an untouched one is not. It
    // joined this list late: the guard landed with "Save over the file the
    // reader opened" and turned this check red, and the red went unread because
    // nothing runs this harness automatically --- it needs a real window, so it
    // is not a gate. The check itself behaved exactly as its comment above
    // promises; `file.saveCopy` is deliberately not here, because a copy of an
    // unedited document is a file a reader wants.
    "file.save",
    // Linux only (certificate files are added to tpdf there), so withheld
    // everywhere else, document or not; on Linux it needs none and is in
    // NEEDS_NO_DOCUMENT instead.
    ...(isLinux() ? [] : ["file.addCertificateFile"]),
    // Guarded on the list of languages `file.recogniseTextLanguage` fetches.
    // Declared with the command, one place too early: the
    // registry lists it after `file.save`, and this list is compared in order.
    "file.recogniseTextLanguage.choice",
    // Guarded on the jump history rather than on a document, which is why the
    // phase clears it below: both are correct to offer after a jump, and this
    // check is about what an open document alone earns. `nav.forward` joined
    // this list on 2026-08-23 with the guard that greys them, and `nav.back`
    // joined at the same moment for a reason the red line did not show --- it
    // was *offered* on this corpus, because the search phase had jumped to a
    // hit before this phase ran, and would have been withheld on a corpus with
    // no text for the search phase to find. A check whose expected answer
    // depends on which fixture it is looking at is one that fails somewhere
    // else later.
    "nav.back",
    "nav.forward",
  ];

  // And the other direction, declared for the same reason. This list was the
  // literal `file.open` until the updater landed, and the check went red ---
  // correctly, because "needs no document" is a claim each command has to earn
  // rather than a fact about how many there are. Two earn it: opening one is
  // how a reader gets a document at all, and checking for updates has nothing
  // to do with documents. Installing one is NOT here: it is guarded on a check
  // having found something, so it belongs above.
  // `app.about` belongs here rather than in the skip table above: it asks the
  // network nothing and replaces no binary, so unlike its two neighbours it can
  // actually be driven, and the sweep drives it.
  //
  // **In the registry's own ranking order, not alphabetical and not the order
  // they were written.** The comparison below is an ordered `join()` rather than
  // a set difference, so a correct list in the wrong order is red --- which is
  // what `app.about` did on the day it was added, reporting all three commands
  // present and the check failing anyway. Worth reading the message rather than
  // the verdict there: the two lists differ by a swap, not by a member.
  // The command-line tool pair needs no document on either platform: on macOS
  // it adds or removes the link, on Windows it says where the installer put the
  // tool. It joined the registry this cycle without joining this list, which the
  // 26.9.21 release run of this check found.
  const NEEDS_NO_DOCUMENT = [
    // A preference, registered with the tab commands, which come first.
    "file.reopenTabsAtLaunch",
    "file.open",
    // Makes a document, so it needs none. Missing here from the day it was
    // registered until this check next ran, 2026-10-04.
    "file.fromPictures",
    // A preference rather than something done to a document; `ask` is the
    // fixture's mode and so is not among them.
    "file.onDiskChange.reload",
    "file.onDiskChange.ignore",
    "app.about",
    "app.checkForUpdates",
    "app.disableAutomaticUpdates",
    "app.installCommandLineTool",
    "app.uninstallCommandLineTool",
    "app.makeDefaultPdfApp",
    "edit.fieldBorderOff",
    // Linux only, and a document is not involved: the file is remembered for
    // every signing after. Missing here from the day it was registered until
    // the first Linux run of this check, 2026-10-14.
    ...(isLinux() ? ["file.addCertificateFile"] : []),
    // A preference, set before or after the first scan is opened. Missing here
    // from 2026-10-05, when it was registered, until this check next ran.
    "file.recogniseTextLanguage",
  ];

  viewer.clearSelection();
  // And the mark the removal probe made, for the same reason the selection is
  // cleared: `edit.removeMark` is offered while its note is open, and the sweep
  // below asks what a document *alone* offers.
  viewer.closeMark();
  viewer.setMarks([]);
  await settle(() => viewer.idle);

  const detached = new CommandRegistry();
  registerAppCommands(detached, { ...actions, viewer: () => null });
  const withoutDocument = detached
    .search("")
    .map((ranked) => ranked.command.title);

  // **From the registry, not from `palette.visible`, and the difference is a
  // rendering cap.** This read the palette's rows until 2026-08-23, when the
  // four stamp commands took the enabled count past `palette.ts`'s
  // `.slice(0, 64)` and three commands fell off the end --- so
  // `view.showThumbnails`, `view.showMarks` and `view.invertPages` were reported
  // as *withheld from a reader* when they were merely off the bottom of a list.
  // The question this check asks is which commands are offered, and a list
  // truncated for display cannot answer it. It had been one command away from
  // saying so for some time.
  //
  // The palette is still opened and closed, because the phase after this one
  // asserts the viewer was left as it was found and an unbalanced open would
  // break it somewhere else.
  palette.open();
  palette.close();
  // To a known point, so the two nav commands above have one answer on every
  // corpus. Not restored afterwards, and that is deliberate rather than an
  // oversight: `linkChecks` is the next thing that reads the history and opens
  // by calling `setLinks`, which clears it too.
  viewer.clearHistory();
  // And the search, for exactly the same reason and found the same way. The
  // find-next phase above leaves one running --- it has to, since it is stepping
  // through matches --- and `edit.redactMatches` is guarded on there being
  // matches. So this check's answer depended on whether *that* phase's needle
  // hit: withheld on `outline-simple`, offered on `tagged`, from one hand-written
  // list that cannot be right for both. The comment beside `nav.back` above
  // names this exact failure a phase earlier, and it did not transfer.
  //
  // The check asks what an open document alone earns, and a leftover search is
  // not part of "an open document" --- so clearing it is what makes the question
  // the one the name asks.
  viewer.clearSearch();
  await settle(() => viewer.idle && viewer.searchMatches.length === 0);
  const withDocument = registry.search("").map((ranked) => ranked.command.title);
  const missing = registered.filter(
    (id) => !withDocument.includes(titleOf(id)),
  );
  check(
    "with no document only the commands needing none are offered",
    withoutDocument.join() === NEEDS_NO_DOCUMENT.map(titleOf).join() &&
      missing.join() === NEEDS_MORE_THAN_A_DOCUMENT.join(),
    `no document: [${withoutDocument.join(", ")}]; with one and nothing selected: ` +
      `${withDocument.length} of ${registered.length}, withheld [${missing.join(", ")}]`,
  );

  palette.destroy();

  // Put it back, and say so. Restoring silently would be enough to stop the
  // contamination and would not stop the *next* probe from reintroducing it: a
  // phase that hands on a viewer it did not leave as it found it is worth one
  // line of output.
  viewer.clearSearch();
  viewer.clearSelection();
  viewer.rotateBy(entry.rotation - viewer.rotation);
  if (entry.fit === "none") viewer.setZoomFixed(entry.zoom);
  else viewer.setFit(entry.fit);
  viewer.goToStart();
  await settle(() => viewer.idle);
  check(
    "leaves the viewer as the phase before it did",
    viewer.rotation === entry.rotation &&
      viewer.fitMode === entry.fit &&
      viewer.selectedText === "" &&
      viewer.searchMatches.length === 0,
    `turns ${entry.rotation}, fit ${entry.fit}, ` +
      `${viewer.selectedText.length} characters selected, ` +
      `${viewer.searchMatches.length} matches held`,
  );
}
