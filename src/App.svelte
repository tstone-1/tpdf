<script lang="ts">
  import { TextEditor } from "./lib/textedit";
  import { FormLayer } from "./lib/forms";
  import { tick } from "svelte";
  import {
    DiskWatch,
    onDiskChange,
    readDiskChangeMode,
    writeDiskChangeMode,
    type DiskChangeMode,
  } from "./lib/diskwatch";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    DocumentTabs, DocumentTasks, freshState, keepScope, keepState, oneEach, partnersOf, restore, restoredState,
    restoredWith, scopeToRestore, twinsOf,
    type DocumentTab, type FreshState, type Restore,
  } from "./lib/documenttabs";
  import { TabLabelSize } from "./lib/tablabels";
  import { sidewaysBy } from "./lib/tabwheel";
  import { readFill, writeFill, type Fill } from "./lib/redactfill";
  import Toolbar from "./Toolbar.svelte";
  import { toolbarState } from "./lib/toolbar";
  import { icon } from "./lib/icons";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    confirm as confirmDialog,
    open as openDialog,
    save as saveDialog,
  } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/harness";
  import { runScrollBenchIfRequested } from "./lib/harness";
  import { runStartupTimelineIfRequested } from "./lib/harness";
  import { runViewerCheckIfRequested } from "./lib/harness";
  import type { Drawn, ScreenPoint } from "./lib/viewer";
  import {
    handleWindowKey,
    registerAppCommands,
    relabelCommands,
    togglePalette,
    type AppActions,
  } from "./lib/appcommands";
  import { CommandRegistry } from "./lib/commands";
  import { areasFrom } from "./lib/selection";
  import { tooManyMatchesToMark } from "./lib/search";
  import {
    ContextMenu,
    menuForSurface,
    PAGE_MENU,
  } from "./lib/contextmenu";
  import { contentBox, cropBox } from "./lib/crop";
  import { Edits, markMade, type EditState } from "./lib/edits";
  import {
    DEFAULT_SWATCH,
    swatch,
    type Swatch,
  } from "./lib/markcolors";
  import { DEFAULT_NIB, nib, type Nib } from "./lib/marknibs";
  import {
    afterRecognition,
    PROGRESS_EVENT,
    progressLine,
    SAVE_FIRST,
    STARTING,
    suggestedName,
    type Progress,
  } from "./lib/recognise";
  import { RecognitionLanguage } from "./lib/ocrlanguage";
  import {
    PAGE_GONE,
    PROGRESS_EVENT as HIDDEN_PROGRESS_EVENT,
    Runs as HiddenRuns,
    STARTING as HIDDEN_STARTING,
    showPassage,
    type Passage,
    type Progress as HiddenProgress,
  } from "./lib/hiddentext";
  import { CommandLineTool } from "./lib/clitoolstate";
  import {
    afterCopy,
    afterRedaction,
    afterRedactionCopy,
    afterRasterRedaction,
    afterSplit,
    afterMerge,
    afterRefusal,
    refusalOf,
    reloadAsked,
    beforeRedactingInPlace,
    type Offer,
  } from "./lib/recovery";
  import { releaseOrphans } from "./lib/orphans";
  import type { DocumentInfo, PageSize } from "./lib/ipc";
  import { call } from "./lib/ipc";
  import * as signing from "./lib/signing";
  import { emptySignatureFields, signTarget, type SignTarget } from "./lib/signfield";
  import { openWithPassword } from "./lib/unlock";
  import { isMac, label, setPrintedKeys } from "./lib/keys";
  import { buildMenu, menuEnablement, runMenuCommand } from "./lib/menubar";
  import { namePages } from "./lib/pageranges";
  import { Palette } from "./lib/palette";
  import { PendingImports } from "./lib/pendingimport";
  import { confirmSignatureSave, askSignatureSave, SaveCancelled } from "./lib/signedsave";
  import { APPEARANCE_WORDS, SignatureDialog } from "./lib/signaturedialog";
  import { askAppearance, PREVIEW_SIZE } from "./lib/signappearance";
  import { loadSignature } from "./lib/signaturestore";
  import { SaveAnswers } from "./lib/saveanswer";
  import { PropertiesDialog } from "./lib/propertiesdialog";
  import { PasswordDialog } from "./lib/passworddialog";
  import { NewPasswordDialog } from "./lib/newpassworddialog";
  import { FieldPropertiesDialog, changeProperties, pickedField, type PropertiesDeps } from "./lib/fieldprops";
  import { duplicate, type DuplicateDeps } from "./lib/duplicate";
  import { EXTENSIONS as PICTURE_EXTENSIONS, afterPictures, suggestedName as pictureName } from "./lib/pictures";
  import { afterProtect, suggestedName as protectedName } from "./lib/protect";
  import { afterCompress, suggestedName as smallerName } from "./lib/compress";
  import { CompressDialog } from "./lib/compressdialog";
  import {
    WebLinkDialog,
    confirmAndOpen,
    type WebLinkSource,
  } from "./lib/weblinkdialog";
  import type { Properties } from "./lib/properties";
  import { basename } from "./lib/paths";
  import { Sidebar, type SidebarOptions, type Tab } from "./lib/sidebar";
  import {
    pagesNeedingWords,
    wantingWordsOn,
    wordsForPage,
    type Comments,
  } from "./lib/comments";
  import { fillRedactionRegions } from "./lib/redactlist";
  import { noticeFor as linkNotice, type Link } from "./lib/links";
  import { ImportedLinks } from "./lib/importedlinks";
  import type { Outline, WebTarget } from "./lib/outline";
  import {
    addressOf,
    allLinksIn,
    commentsIn,
    markRows,
    NO_PAGES,
    outlineIn,
    pageId,
    redactionRows,
    slotOfIdIn,
    type MarkKind,
    type FieldKind,
    type StampName,
    type PageId,
    type RegionPlan,
  } from "./lib/pages";
  import { dimensionsOf, type PageSizeName } from "./lib/pagesizes";
  import { nameOf } from "./lib/markpopup";
  import { labelsFor, RECENT_PREFIX } from "./lib/recents";
  import {
    behindWrites, focusAfterRemoval, recentCommands, StartPage, startMove, type StartRow,
  } from "./lib/startpage";
  import {
    loadSession,
    SessionWriter,
    type Place,
    type Session,
  } from "./lib/session";
  import {
    canOrderTabs, placing, readFieldBorder, writeFieldBorder,
  } from "./lib/fieldnames";
  import { noticeAfterPick } from "./lib/arrange";
  import { barCommand } from "./lib/arrangebar";
  import {
    arrangeBoth, asMarks, isSaved, moved as fieldMoved, placed as fieldPlaced,
    controlRect, removal, renamed as fieldRenamed,
  } from "./lib/savedfields";
  import type { Form } from "./lib/forms";
  import {
    TabRecorder, afterReopen, launchPlan, openBehind, sidesToRestore, tabsToReopen, type TabHost,
  } from "./lib/tabrestore";
  import { runMarkCheckIfRequested } from "./lib/harness";
  import { runSessionCheckIfRequested } from "./lib/harness";
  import { runOpenCheckIfRequested } from "./lib/harness";
  import { Serial } from "./lib/serial";
  import { DegradedLabel } from "./lib/degraded";
  import {
    finishUpdate, stepEndsProcess, stepFor, Updates, updateLabel, updateNotice,
    type FinishStep, type UpdateState,
  } from "./lib/update";
  import { Viewer, type ViewerOptions, type ViewerStatus } from "./lib/viewer";
  import { Stage, blankLive, scoped, type LiveDocument, type Slots } from "./lib/livedocument";
  import {
    dropAbandoned, openFailure, pageTable, placeOnSide, placeToResume, readerIsIn, repoint, sharedByTwin, tabToKeep,
    toldAfterFallback,
    type OpenFound,
  } from "./lib/documentopen";
  import { Panes, otherSide, type Side, type Slot } from "./lib/panes";
  import { TabDrag, type Carried } from "./lib/tabdrag";
  import { NO_VIEW, ViewIds, viewOf, type ViewId } from "./lib/views";
  import { ScrollLock } from "./lib/syncscroll";
  import { describeFit, percentOf } from "./lib/zoom";

  /**
   * The window's two page areas. The second is used when two documents are
   * side by side; which side each one shows is `panes.ts`'s answer.
   */
  let areaHosts = $state<(HTMLDivElement | null)[]>([null, null]);
  /** The page area the mounted document's viewer is in. */
  let surface: HTMLDivElement | null = null;
  const panes = new Panes<ViewId>();
  const furtherViews = new ViewIds();
  /**
   * What the markup needs of {@link panes}, which is not reactive: for each
   * page area, whether it shows nothing and where it is drawn. Written by
   * {@link refreshTabs}, which every change to the tabs already ends in.
   */
  let paneLayout = $state({
    split: false, unused: [false, true], order: [0, 2], focused: 0, front: [-1, -1],
  });
  /**
   * Whether a document is mounted that the reader is not working in. The body
   * is drawn while this is set, as well as while the document in the variables
   * has a title.
   */
  let bodyHeld = $state(false);
  /** The share of the width the left side has, with two sides. */
  let paneShare = $state(0.5);
  let panesHost = $state<HTMLDivElement | null>(null);
  /** Whether the two sides scroll together. The offset is `syncscroll.ts`'s. */
  let syncScroll = $state(false);
  const scrollLock = new ScrollLock<ViewId>();
  /** The viewers of the two locked documents, by handle. See `lockSides`. */
  const lockedViewers = new Map<ViewId, Viewer>();
  /** Set while one document is being moved to keep up with the other. */
  let following = false;
  /** Until when a wheel turned with Alt held scrolls its own side alone. */
  let aloneUntil = 0;

  function dividerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  /** Follows the pointer while the divider holds it, within a fifth of either edge. */
  function dividerMove(event: PointerEvent): void {
    const divider = event.currentTarget as HTMLElement;
    if (!divider.hasPointerCapture(event.pointerId) || !panesHost) return;
    const box = panesHost.getBoundingClientRect();
    if (box.width <= 0) return;
    paneShare = Math.min(0.8, Math.max(0.2, (event.clientX - box.left) / box.width));
  }

  const tabDrag = new TabDrag<ViewId>();
  /** The tab being dragged and the side it is over, for the markup. */
  let tabCarried = $state<Carried<ViewId> | null>(null);
  /** Set from the end of a drag until the click the same release sends has passed. */
  let tabDragEnded = false;

  /** What a drop is decided against, read when the pointer moves. */
  function dropLayout(id: ViewId) {
    const box = panesHost?.getBoundingClientRect();
    return {
      area: box ?? { left: 0, top: 0, width: 0, height: 0 },
      split: panes.split, share: paneShare, from: panes.sideOf(id), tabs: tabs.all.length,
    };
  }

  /**
   * A press on a tab, which is a drag onto a side once the pointer has left
   * the tab's place. The pointer is followed on the window, so the drag goes
   * on over the pages, where the tab's own row does not reach.
   */
  function tabPress(event: PointerEvent, id: ViewId): void {
    if (event.button !== 0 || opening || documentBusy || tabDrag.pressed) return;
    tabDrag.press(id, event.clientX, event.clientY);
    const move = (moved: PointerEvent) => {
      tabCarried = tabDrag.move(moved.clientX, moved.clientY, dropLayout(id));
    };
    const end = (drop: boolean) => {
      for (const [name, listener] of listeners) window.removeEventListener(name, listener, true);
      const carried = drop ? tabDrag.release() : null;
      tabDrag.cancel();
      if (tabCarried) {
        // The release is followed by a click when it is over the tab pressed.
        tabDragEnded = true;
        setTimeout(() => { tabDragEnded = false; });
      }
      tabCarried = null;
      if (carried?.side) void dropTab(carried.id, carried.side);
    };
    const listeners: [string, (event: Event) => void][] = [
      ["pointermove", (moved) => move(moved as PointerEvent)],
      ["pointerup", () => end(true)],
      ["pointercancel", () => end(false)],
      ["blur", () => end(false)],
      ["keydown", (key) => {
        if ((key as KeyboardEvent).key !== "Escape" || !tabCarried) return;
        key.preventDefault();
        key.stopPropagation();
        end(false);
      }],
    ];
    for (const [name, listener] of listeners) window.addEventListener(name, listener, true);
  }

  /**
   * Puts a dragged tab on `side`. With one side showing, the tab dropped on
   * the left half stays on the left and every other tab goes to the right.
   */
  async function dropTab(id: ViewId, side: Side): Promise<void> {
    if (panes.split) return moveTab(id, side);
    await moveTab(id, "right");
    if (side === "left" && panes.split && panes.sideOf(id) === "right") switchSides();
  }
  let sidebarHost = $state<HTMLDivElement | null>(null);
  let title = $state("");
  let error = $state<string | null>(null);
  /**
   * What the reader can press about {@link error}, in the order shown.
   *
   * Cleared everywhere `error` is, and set only from `recovery.ts` --- the rules
   * that decide these are in that module because nothing renders this component,
   * so a decision written here is one no check can reach.
   */
  let offers = $state<Offer[]>([]);
  let opening = $state(false);
  let documentBusy = $state(false);
  const documentTasks = new DocumentTasks((busy) => {
    documentBusy = busy;
    formLayer?.setBusy(busy);
    textEditor?.setBusy(busy);
    // The commands are guarded by this, and a task's own `refreshMenu` runs
    // while it is still set. A copy that failed opened no tab, so nothing
    // asked again, and Print, Comment and the rest stayed grey until the
    // reader switched tabs.
    refreshMenu();
  });
  const tabs = new DocumentTabs<DocumentTab>();
  let tabRows = $state<{ id: ViewId; path: string; dirty: boolean; slot: number }[]>([]);
  const tabLabelSize = new TabLabelSize();
  let tabLabelPx = $state(tabLabelSize.px);
  let activeTab = $state<ViewId>(NO_VIEW);
  /**
   * Each tab's label, by the tab. Worked out over the files and not the tabs:
   * a file shown in two tabs is one name, and two equal paths would lengthen
   * each other to the whole path.
   */
  const tabLabelOf = $derived.by(() => {
    const paths = [...new Set(tabRows.map((tab) => tab.path))];
    const labels = labelsFor(paths);
    return new Map(tabRows.map((tab) => [tab.id, labels[paths.indexOf(tab.path)] ?? ""]));
  });
  let committingPopup = false;
  let formLayer: FormLayer | null = null;
  /** The names in the open document's form, for naming a field placed in it. */
  let formNames: string[] = [];
  /** The open document's form as the worker scanned it, for changing its fields. */
  let scannedForm: Form | null = null;
  /** Whether the document's own fields are shown as rectangles to move, rename and remove. */
  let formEditing = false;
  /** The marks the viewer draws: the reader's, and the saved fields while they are being changed. */
  const shownMarks = (state: EditState) =>
    formEditing && scannedForm ? [...state.marks, ...asMarks(scannedForm, state)] : state.marks;
  /** A change to one saved field, or nothing when the viewer named none. */
  const changeField = (target: ReturnType<typeof fieldMoved>) => {
    if (target) void applyEdit((e) => e.refield([target]));
  };
  /**
   * Removes what the viewer names by `id`, as one step of the gesture `sweep`.
   * Which of the two kinds of thing the id stands for is `removal`'s answer.
   */
  const removeNamed = (id: number, sweep: number) => {
    const what = removal(scannedForm, edits?.state ?? null, id);
    if (!what) return;
    void applyEdit((e) => "mark" in what ? e.unmark(what.mark, sweep) : e.refield([what.field], sweep));
  };
  /** What the field properties panel is given; the join is `fieldprops.ts`. */
  const propertiesDeps: PropertiesDeps = {
    model: () => edits,
    picked: () => viewer?.pickedMarks() ?? [],
    form: () => (formEditing ? scannedForm : null),
    state: () => edits?.state ?? null,
    ask: (now) => fieldPropertiesDialog?.ask(now) ?? Promise.resolve(null),
    refield: (targets) => void applyEdit((e) => e.refield(targets, targets.length > 1 ? viewer?.gesture() ?? 0 : 0)),
    refit: (mark, props) => void applyEdit((e) => e.refit(mark, props)),
    say: (message) => say(message),
  };
  /** What duplicating the picked rectangles is given; the join is `duplicate.ts`. */
  const duplicateDeps: DuplicateDeps = {
    copies: () => viewer?.copiesOfPicked() ?? null,
    state: () => edits?.state ?? null,
    form: () => scannedForm,
    formNames: () => formNames,
    border: () => fieldBorder,
    make: (copy, sweep) =>
      applyEdit((e) =>
        e.mark(copy.kind, copy.page, copy.quads, [], copy.note, copy.color, null, null, copy.width, undefined, copy.field, sweep),
      ),
    pick: (ids) => viewer?.pick(ids),
    say: (message) => say(message),
  };
  /** Which kind of form field the armed tool places. */
  let armedField: FieldKind = "text";
  /** The group the radio buttons placed next belong to. */
  let armedGroup = "";
  /** The choices of the dropdown the next drag places. Empty for any other kind. */
  let armedOptions: string[] = [];
  /** Whether a text field placed from now on gets a line round it. */
  let fieldBorder = readFieldBorder();
  let textEditor: TextEditor | null = null;
  let textEditorGeneration = 0;

  function commitPopups(): void {
    committingPopup = true;
    try {
      formLayer?.commit();
      textEditor?.commit();
      viewer?.closeMark();
      viewer?.closeComment();
    } finally { committingPopup = false; }
  }

  function refreshTabs(): void {
    const leftIn = panes.slotOf("left");
    paneLayout = {
      split: panes.split,
      unused: [0, 1].map((slot) => !panes.split && slot !== leftIn),
      // The divider is drawn between them, at 1.
      order: [0, 1].map((slot) => (slot === leftIn ? 0 : 2)),
      focused: panes.slotOf(panes.focused),
      front: ([0, 1] as Slot[]).map((slot) => panes.front(panes.sideIn(slot))),
    };
    const changed = activeTab !== tabs.active;
    activeTab = tabs.active;
    tabRows = tabs.all.map((tab) => ({
      id: tab.view, path: tab.path, dirty: tab.edits.state.dirty,
      slot: panes.slotOf(panes.sideOf(tab.view)),
    }));
    tabRecorder.note(tabs.all.map((tab) => tab.path), tabs.find(tabs.active)?.path ?? null, panes.split ? {
      right: tabs.all.filter((tab) => panes.sideOf(tab.view) === "right").map((tab) => tab.path),
      beside: tabs.find(panes.front(otherSide(panes.focused)))?.path ?? null,
      share: Math.round(paneShare * 1000) / 1000,
    } : null);
    if (changed) void tick().then(() => {
      document.getElementById(`document-tab-${activeTab}`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
  }

  /**
   * Commits what the reader is still typing and waits until the model has it.
   *
   * **The first line of everything that writes a file or reads `dirty` to
   * decide whether it may**, and one function so that the next such flow
   * cannot leave a line of it out: it was written out by hand in six of them
   * and missing from the seventh, which signed the file on disk without the
   * form answer on screen. A form answer commits when its control loses the
   * keyboard and a note when its box closes, so {@link commitPopups} does
   * both; the edits that makes are queued, so they are waited for; and the
   * form layer and the text editor each hold replies of their own.
   *
   * **Called before `copyTaskBusy` is set, never after.** {@link applyEdit}
   * lets a commit made in here past a running document task and past nothing
   * else, so a caller that raised its own flag first would have the draft
   * refused in silence. The commit is made in the call itself, before the
   * first wait, which is what lets a copy task call this, raise its flag and
   * only then wait: `const settled = settleDrafts(); copyTaskBusy = true;` and
   * `await settled` inside its `try`.
   *
   * Rejects when a draft cannot be committed, with the sentence to show.
   */
  async function settleDrafts(): Promise<void> {
    commitPopups();
    // The document on the other side too, when there is one: a note left open
    // there is as unsaved as one here, and closing the window or every tab
    // reads both documents' state. Committed before the first wait, like the
    // line above, and each as its own document.
    const beside = stage.parked.map((id) => asDocument(id, () => {
      commitPopups();
      return Promise.all([pendingEdit, formLayer?.settle(), textEditor?.settle()]);
    }));
    // Named before the first wait: after it the variables may hold the other
    // document, whose drafts are the ones waited for just below.
    const pending = pendingEdit, form = formLayer, editor = textEditor;
    await pending;
    await form?.settle();
    await editor?.settle();
    await Promise.all(beside);
  }

  async function settleDocument(): Promise<void> {
    await settleDrafts();
    notePlace();
    places.flush();
  }

  function keepActiveTab(): void {
    const tab = tabs.find(openView);
    if (!tab || !viewer || !edits) return;
    keepState(tab, {
      edits, place: currentPlace(false), covered: new Map(covered), query,
      findShown, searchOptions: viewer.searchOptionsNow,
      searchScope: keepScope(viewer.searchScopeRanges, edits.state.pages), sidebarTab: sidebar?.tab ?? "outline",
      error, offers, notice, redactedCopyPath,
    });
    refreshTabs();
  }

  /**
   * {@link keepActiveTab}'s other half: what restoring each kept field means in
   * this window. `documenttabs.ts` decides when in an open each one is applied
   * and that none is left out; an entry missing here does not compile.
   */
  const restoring: Restore = {
    query: (value) => { query = value; },
    findShown: (value) => { findShown = value; },
    // The message and its buttons are one fact, shown by one call.
    error: (value, kept) => say(value, kept.offers),
    offers: restoredWith("error"),
    notice: (value) => { notice = value; },
    redactedCopyPath: (value) => { redactedCopyPath = value; },
    // Mark ids start again with the model, so an entry kept from the last
    // document would put its words on this one's first highlight.
    covered: (value) => {
      covered.clear();
      for (const [id, words] of value) covered.set(id, words);
    },
    // A search is its words, how they are matched and where, restored by one
    // call so that a search confined to a selection is not widened on the way.
    // The scope only while the pages are the ones it was kept with: the
    // document's other view may have deleted or moved one since.
    searchOptions: (value, kept) => viewer?.restoreSearch(kept.query, value,
      scopeToRestore(kept.searchScope, edits?.state.pages ?? [])),
    searchScope: restoredWith("searchOptions"),
    sidebarTab: (value) => sidebar?.selectTab(value),
  };

  function activateTab(id: ViewId): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      await showTabNow(id);
    }));
  }

  /**
   * Brings a tab to the front of the side it is on, with the reader working
   * there. Inside {@link opens}, like every caller of `openDocument`.
   */
  async function showTabNow(id: ViewId): Promise<void> {
    const tab = tabs.find(id);
    if (!tab) return;
    const side = panes.sideOf(id);
    if (side !== panes.focused && !focusSide(side)) return;
    if (id === openView) return;
    await openDocument(tab.path, false, null, tab);
  }

  /**
   * Makes `side` the one the reader works in: its document's values go into
   * the variables and the other document's wait in `stage`.
   *
   * Not asynchronous, and it must not become so. It is called from a press
   * inside the other side, and the viewer there handles the same press right
   * after: by then the variables have to be that document's. What the reader
   * was typing on the side they leave is committed first; the edit that makes
   * is finished by {@link runEdit} as the document it was made in.
   *
   * Refused while a document is opening or a task is running, as a tab switch
   * is, and for a side that shows nothing.
   */
  function focusSide(side: Side): boolean {
    const id = panes.front(side);
    if (id === openView) return true;
    if (opening || documentBusy || copyTaskBusy || !stage.parked.includes(id)) return false;
    commitPopups();
    // A search still waiting out its pause is this document's. Run now, it
    // reaches the viewer it was typed for.
    if (findTimer) {
      clearTimeout(findTimer);
      findTimer = 0;
      viewer?.search(query);
    }
    sidebar?.setVisible(false);
    stage.focus(id);
    panes.focus(side);
    tabs.active = id;
    sidebar?.setVisible(sidebarShown);
    formLayer?.setBusy(documentBusy);
    textEditor?.setBusy(documentBusy);
    refreshTabs();
    refreshMenu();
    return true;
  }

  /**
   * Mounts and tears down viewers until the window shows what {@link panes}
   * says, and leaves the reader in the side it names. Inside {@link opens}.
   *
   * A document is torn down as itself, whichever side the reader is in. One is
   * mounted into blank variables: the document in them is parked first, and
   * `openDocument` then does what it does for the first document of a window.
   *
   * `failed` is what an open that could not finish said, when this is the
   * window going back to the tabs it had. Each mount below puts back the
   * message its tab kept, so the reason is said again at the end.
   */
  async function showPanes(failed: string | null = null): Promise<void> {
    const plan = panes.plan();
    for (const { id } of plan.unmount) {
      asDocument(id, () => { commitPopups(); keepActiveTab(); unmountDocument(); });
    }
    let refused: string | null = null;
    // The page area a document is about to be mounted in is hidden until the
    // window is drawn with two sides. A viewer built in a hidden area measures
    // nothing and restores its place against that.
    if (plan.mount.length) {
      refreshTabs();
      await tick();
    }
    for (const { id } of plan.mount) {
      const tab = tabs.find(id);
      if (!tab) continue;
      if (openDoc >= 0) {
        sidebar?.setVisible(false);
        // The body is drawn while a document has a title, and the variables
        // are about to have none: this keeps the page areas, and the viewer
        // in one of them, through that.
        bodyHeld = true;
        stage.park();
      }
      panes.fronted(id);
      await openDocument(tab.path, false, null, tab);
      // A document that would not mount left its reason in variables that the
      // next lines replace.
      if (openView !== id) refused = error;
    }
    // Mounting a document puts the reader in its side, and the last one
    // mounted is not always the side the plan named.
    if (stage.holds(plan.focus) && panes.front(panes.sideOf(plan.focus)) === plan.focus) {
      panes.focus(panes.sideOf(plan.focus));
    }
    const wanted = panes.plan().focus;
    if (openView !== wanted && stage.parked.includes(wanted)) {
      sidebar?.setVisible(false);
      stage.focus(wanted);
    } else if (openDoc < 0) {
      // The side the reader was to be in shows nothing. Any document that is
      // mounted is a better answer than an empty window beside one.
      const any = stage.parked[0];
      if (any !== undefined && stage.focus(any)) panes.fronted(any);
    }
    bodyHeld = stage.parked.length > 0;
    if (openDoc >= 0) tabs.active = openView;
    sidebar?.setVisible(sidebarShown);
    formLayer?.setBusy(documentBusy);
    textEditor?.setBusy(documentBusy);
    const told = toldAfterFallback(failed, refused);
    if (told !== null) say(told);
    if (syncScroll) setSyncScroll(true);
    refreshTabs();
    refreshMenu();
    if (plan.mount.length || plan.unmount.length) viewer?.focus();
  }

  /** Moves a tab to `to`, which starts the split when nothing is on the right. */
  function moveTab(id: ViewId, to: Side): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      if (!tabs.find(id) || tabs.all.length < 2) return;
      opening = true;
      try {
        await settleDocument();
        const before = panes.split ? null : readingsNow();
        panes.move(id, to, tabs.all.map((entry) => entry.view));
        await showPanes();
        if (before) fitNewSplit(before);
      } catch (e) { say(String(e)); }
      finally { opening = false; refreshMenu(); }
    }));
  }

  /** Where each mounted document's view begins, by view. */
  function readingsNow(): Map<ViewId, { page: number; fraction: number }> {
    const readings = new Map<ViewId, { page: number; fraction: number }>();
    for (const view of [openView, ...stage.parked]) {
      const at = asDocument(view, () => viewer?.reading);
      if (at) readings.set(view, at);
    }
    return readings;
  }

  /**
   * Half the window at the zoom the whole window had shows half a page width,
   * so a split starts with each side fitted to its own width.
   *
   * `before` is {@link readingsNow} from before the window was divided. A
   * view that was on screen then begins where it began: its area has halved
   * since, and a view that follows its width keeps the middle of the page in
   * place through that and through the fit here, which at half the width is
   * most of a page further up. A view mounted since begins where it was put.
   */
  function fitNewSplit(before: ReadonlyMap<ViewId, { page: number; fraction: number }>): void {
    if (!panes.split) return;
    for (const side of ["left", "right"] as const) {
      const view = panes.front(side);
      asDocument(view, () => {
        if (!viewer) return;
        const at = before.get(view) ?? viewer.reading;
        viewer.setFit("width");
        viewer.followTo(at.page, at.fraction);
      });
    }
  }

  /**
   * Shows the document the reader is in on the other side as well: a second
   * view of it, with a tab of its own, on the same handle and the same edit
   * model. A document that already has a second view gets that one brought to
   * the front of the other side.
   *
   * The second view starts where the first is, with no search and no message
   * of its own, and the reader is left in it.
   */
  function showOnBothSides(): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      const tab = tabs.find(openView);
      if (!tab || !viewer) return;
      opening = true;
      try {
        await settleDocument();
        keepActiveTab();
        const across = otherSide(panes.focused);
        const before = panes.split ? null : readingsNow();
        let twin = twinsOf(tabs.all, tab)[0];
        if (!twin) {
          twin = { ...tab, ...freshState(), view: furtherViews.another(), sidebarTab: tab.sidebarTab };
          tabs.addAfter(twin, tab.view);
        }
        panes.move(twin.view, across, tabs.all.map((entry) => entry.view));
        await showPanes();
        if (before) fitNewSplit(before);
      } catch (e) { say(String(e)); }
      finally { opening = false; refreshMenu(); }
    }));
  }

  /**
   * Puts the tabs back on the two sides a launch read, once they are open.
   * `recorded` is what the session held; what is done with it is decided
   * against the tabs that did open (`sidesToRestore`).
   */
  function restoreSides(recorded: Parameters<typeof sidesToRestore>[0]): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      const sides = sidesToRestore(recorded, tabs.all.map((tab) => tab.path), tabs.find(openView)?.path ?? null);
      if (!sides || panes.split) return;
      const idOf = (path: string | null) => (path === null ? NO_VIEW : tabs.forPath(path)?.view ?? NO_VIEW);
      opening = true;
      try {
        await settleDocument();
        paneShare = sides.share;
        panes.arrange(sides.right.map(idOf), idOf(sides.beside), openView, tabs.all.map((entry) => entry.view));
        await showPanes();
      } catch (e) { say(String(e)); }
      finally { opening = false; refreshMenu(); }
    }));
  }

  /**
   * Turns scrolling together on or off. On, the two documents are locked at
   * the places they are in; with one side, or a side that shows nothing, it
   * stays off.
   */
  function setSyncScroll(on: boolean): void {
    syncScroll = on && panes.split;
    if (!lockSides()) syncScroll = false;
    refreshMenu();
  }

  /**
   * Locks the two documents in front at the places they are in, when the
   * reader has scrolling together on and both are mounted; holds nothing
   * otherwise. False when nothing is locked.
   *
   * The two viewers are kept beside the lock. {@link keepInStep} runs in every
   * frame of either document, and asking the stage for the other one's viewer
   * there would lend the variables out and back once a frame for an answer
   * that changes only when a document is mounted or torn down.
   */
  function lockSides(): boolean {
    const read = (id: ViewId) => asDocument(id, () =>
      viewer ? { id, at: viewer.reading, zoom: viewer.currentZoom, viewer } : null);
    const first = syncScroll && panes.split ? read(panes.front("left")) : null;
    const second = first ? read(panes.front("right")) : null;
    lockedViewers.clear();
    if (!first || !second) {
      scrollLock.release();
      return false;
    }
    scrollLock.lock(first, second);
    lockedViewers.set(first.id, first.viewer);
    lockedViewers.set(second.id, second.viewer);
    return true;
  }

  /**
   * Moves the other document to where this one's frame says it belongs.
   * Called from every frame of a mounted document, as that document, and it
   * runs inside that frame: the other viewer draws before this returns, so the
   * two sides are never a frame apart.
   */
  function keepInStep(): void {
    if (!syncScroll || !viewer) return;
    const leader = viewer;
    const partnerId = scrollLock.partnerOf(openView);
    const partner = lockedViewers.get(partnerId);
    if (!partner) return;
    const factor = scrollLock.zoomed(openView, leader.currentZoom,
      { following, fitted: leader.fitMode !== "none" });
    const moved = scrollLock.moved(openView, leader.reading,
      { following, alone: performance.now() < aloneUntil, partnerAt: partner.reading });
    if (factor === null && moved === null) return;
    following = true;
    try {
      // The zoom first: it shifts the partner's own place, and the place asked
      // for after it is then the one it ends in.
      if (factor !== null) partner.setZoomFixed(partner.currentZoom * factor);
      const to = moved ?? scrollLock.aim(openView, leader.reading);
      if (to) partner.followTo(to.page, to.fraction);
      scrollLock.seen(partnerId, partner.reading, partner.currentZoom);
    } finally { following = false; }
  }

  function switchSides(): void {
    panes.swap(tabs.all.map((entry) => entry.view));
    refreshTabs();
    refreshMenu();
  }

  function closeTab(id: ViewId): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      const tab = tabs.find(id);
      if (!tab) return;
      // The tab in front of the other side is closed as the document the
      // reader is in, which it becomes first.
      if (id !== openView && panes.front(panes.sideOf(id)) === id && !focusSide(panes.sideOf(id))) return;
      opening = true;
      try {
        await settleDocument();
        // One of two views of a document closes without a question and without
        // releasing anything: the document, and its unsaved work, stay open in
        // the other.
        const lastView = twinsOf(tabs.all, tab).length === 0;
        if (lastView && tab.edits.state.dirty && !await confirmDialog(
          `Discard unsaved changes to ${basename(tab.path)}?`,
          { title: "Close tab", kind: "warning", okLabel: "Discard changes", cancelLabel: "Keep open" },
        )) return;
        if (id === openView) clearActiveDocument();
        panes.closed(id, tabs.all.map((entry) => entry.view));
        tabs.remove(id);
        refreshTabs();
        // A teardown refusal must not prevent the next document from mounting.
        if (lastView) await call("close_document", { doc: tab.doc.id }).catch((e) => {
          console.warn(`could not release document ${tab.doc.id}: ${e}`);
        });
        // The side's next tab comes to the front, or the split ends and the
        // other side's document is the one the reader is in.
        if (tabs.all.length) await showPanes();
        else void refreshStartPage();
      } catch (e) { say(String(e)); }
      finally { opening = false; refreshMenu(); }
    }));
  }

  /**
   * Closes every tab, asking once when any of them has unsaved work -- the
   * same settle-then-read the update's unsaved check does, since reading
   * `dirty` before a pending edit lands reports work as saved.
   */
  function closeAllTabs(): Promise<void> {
    return documentTasks.idle().then(() => opens.run(async () => {
      if (!tabs.all.length) return;
      opening = true;
      try {
        await settleDocument();
        const unsaved = oneEach(tabs.all).filter((tab) => tab.edits.state.dirty).map((tab) => basename(tab.path));
        if (unsaved.length && !await confirmDialog(
          unsaved.length === 1
            ? `Discard unsaved changes to ${unsaved[0]}?`
            : `Discard unsaved changes in ${unsaved.length} documents?`,
          { title: "Close all tabs", kind: "warning", okLabel: "Discard changes", cancelLabel: "Keep open" },
        )) return;
        const ids = tabs.all.map((tab) => tab.view);
        const handles = oneEach(tabs.all).map((tab) => tab.doc.id);
        for (const beside of stage.parked) asDocument(beside, () => unmountDocument());
        bodyHeld = false;
        setSyncScroll(false);
        clearActiveDocument();
        for (const id of ids) tabs.remove(id);
        panes.cleared();
        refreshTabs();
        for (const id of handles) {
          // A teardown refusal must not keep the other handles open.
          await call("close_document", { doc: id }).catch((e) => {
            console.warn(`could not release document ${id}: ${e}`);
          });
        }
        void refreshStartPage();
      } catch (e) { say(String(e)); }
      finally { opening = false; refreshMenu(); }
    }));
  }

  /** Middle-click on a tab closes it, as in a browser. */
  function tabAuxClick(event: MouseEvent, id: ViewId): void {
    if (event.button !== 1) return;
    event.preventDefault();
    if (!opening && !documentBusy) void closeTab(id);
  }

  function clearActiveDocument(): void {
    unmountDocument();
  }

  function tabKey(event: KeyboardEvent, id: ViewId): void {
    const index = tabRows.findIndex((tab) => tab.id === id);
    let next: number;
    if (event.key === "ArrowRight") next = (index + 1) % tabRows.length;
    else if (event.key === "ArrowLeft") next = (index + tabRows.length - 1) % tabRows.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = tabRows.length - 1;
    else return;
    event.preventDefault();
    const target = tabRows[next];
    if (target) void activateTab(target.id).then(() => {
      document.getElementById(`document-tab-${target.id}`)?.focus();
    });
  }
  let copyTaskBusy = $state(false);
  let redactedCopyPath = $state<string | null>(null);
  let blockingTask = $state<string | null>(null);
  // Whether the blocking task is a text recognition, which is the one of them
  // that reports its pages and can be stopped.
  let recognising = $state(false);
  // The number of the recognition Stop is for. Counted from 1, because 0 is
  // the backend's "nothing was asked to stop".
  let recognitionRun = 0;
  // The other blocking task that reports its pages and can be stopped: the
  // check for text the pages do not show. Which run Stop is for is
  // `hiddenRuns`'; the flag is its `running`, held here so the header redraws.
  const hiddenRuns = new HiddenRuns();
  let findingHidden = $state(false);
  /**
   * The open document's page edits, or null when there is none.
   *
   * Holds the model's last answer; the model itself is in Rust. Replaced
   * wholesale on every open, so a document cannot inherit the previous one's
   * journal --- the backend does the same thing under the same handle, and the
   * two have to agree about which document a command is for.
   */
  let edits: Edits | null = null;
  /**
   * Whether the document differs from the file on disk.
   *
   * `$state` because the header shows it, unlike the rest of the edit state,
   * which is only read when a command runs.
   */
  let dirty = $state(false);
  let status = $state<ViewerStatus | null>(null);
  /**
   * The colour the reader picked for marks, or {@link DEFAULT_SWATCH}.
   *
   * Here rather than on {@link Edits}, which is built per document: a reader who
   * picks green, closes the file and opens another has not gone back to yellow.
   * The whole swatch rather than its three floats, because the status line names
   * it and `null` --- the default's colour --- has no name of its own.
   */
  let markColor = $state<Swatch>(DEFAULT_SWATCH);
  /**
   * The nib the reader has picked, for the status line and the menu's tick.
   *
   * **Not what a drawing is made with**, which is the viewer's own `nib` and
   * arrives back on `Drawn.width`. This is the label, and it is deliberately
   * downstream of the setting rather than beside it: `chooseNib` sets the
   * viewer and then records what it set, so a state that drifted would show a
   * wrong word rather than draw a wrong line.
   */
  let markNib = $state<Nib>(DEFAULT_NIB);
  /**
   * The words each mark covers, by the model's id for it.
   *
   * **Not `$state`, and not in the model.** The panel is imperative DOM that is
   * repainted by hand, so nothing here has to be reactive; and the model holds
   * what the document will become, which this is not --- a saved PDF has no
   * entry for the text a highlight sits on, so this could never be read back
   * and would be a field `save.rs` had to remember to ignore.
   *
   * Filled by {@link markSelection}, which is the only way a mark that covers
   * words is made. Kept until the document changes rather than pruned against
   * the live marks: an undone mark comes back under the same id --- the id is
   * in the journalled command --- so a map pruned on undo would have redo show
   * the reader "No note" for a highlight whose words it had just been
   * displaying. The cost of that choice is an entry per mark removed and not
   * redone, each capped at {@link COVERED_CHARS}.
   */
  let covered = new Map<number, string>();
  /**
   * Longest covered text kept per mark, in characters.
   *
   * The row is one line and the CSS ellipsis cuts it far shorter than this, so
   * nothing visible is lost. What it bounds is a reader who selects a hundred
   * dense pages and highlights them: the whole of every page would otherwise be
   * held here, beside the copy `TextCache` is already holding under its own
   * bound.
   */
  const COVERED_CHARS = 200;

  /**
   * What the status line calls the tool that is armed.
   *
   * `nameOf` is the one table of reader-facing kind names --- the note box and
   * the marks panel read it too --- so this adds a verb rather than a second
   * spelling: "Box" is what the thing is called and "Box — click and drag" is
   * what a reader who armed it needs told. The two shape tools and the text box
   * are dragged out; a comment is one press, which is the difference the whole
   * placement change is about, so it is the one entry whose verb differs.
   *
   * `ink` cannot reach here --- `viewer.ts` reports a drawing through the field
   * that counts strokes --- and it is left out of the table rather than given an
   * unreachable entry, so the fallback is the honest one if that ever changes.
   */
  function armedLabel(kind: MarkKind | "crop" | "redact" | "place"): string {
    if (kind === "crop") return "Crop — drag out what to keep";
    if (kind === "place") return "Signature — drag out where it appears; Esc cancels";
    // **Says what goes, where the crop's says what stays**, because the two are
    // the same drag and this is the only place the reader is told which one
    // they armed. "Nothing is removed yet" is the other half: marking is
    // reversible and applying is not, and a reader who thinks the first line
    // has already destroyed something will not review the list.
    if (kind === "redact")
      return "Redact — drag across words or blank paper to mark; Esc when done; nothing is removed yet";
    return kind === "note"
      ? `${nameOf(kind)} — click to place`
      : `${nameOf(kind)} — click and drag`;
  }
  /**
   * The degraded-state words currently on screen, or `null` for none.
   *
   * State rather than a `$derived`, because the decision reads the clock: a
   * transient state has to have *lasted* before it is worth showing, and a
   * derived that sampled `performance.now()` would be recomputing a different
   * answer every time anything else in the header changed. `degradedGate` holds
   * the episode clock; see `degraded.ts` for why there is one.
   */
  let degraded = $state<string | null>(null);
  let degradedGate = new DegradedLabel();
  let query = $state("");
  let findField = $state<HTMLInputElement | null>(null);
  let findShown = $state(false);
  let zoomMenu = $state<HTMLDetailsElement | null>(null);

  function closeZoomOutside(event: PointerEvent): void {
    if (event.target instanceof Node && !zoomMenu?.contains(event.target)) {
      zoomMenu?.removeAttribute("open");
    }
  }

  function closeZoomOnEscape(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !zoomMenu?.open) return;
    zoomMenu.open = false;
    zoomMenu.querySelector("summary")?.focus();
    event.preventDefault();
    event.stopPropagation();
  }
  let sidebarShown = $state(false);

  let viewer: Viewer | null = null;
  let palette: Palette | null = null;
  let sidebar: Sidebar | null = null;
  let signatureDialog: SignatureDialog | null = null;
  // The signing's save panel, answered by the checks build's signing phase,
  // which cannot drive a native panel (`signingcheck.ts`). `null` in a normal
  // build, where the panel call below compiles to the panel alone.
  const signSaves = __TPDF_CHECKS__ ? new SaveAnswers() : null;
  /** The pictures the next open panel answers with, in a checks build. */
  let queuedPictures: string[] | null = null;
  let propertiesDialog: PropertiesDialog | null = null;
  let passwordDialog: PasswordDialog | null = null;
  let newPasswordDialog: NewPasswordDialog | null = null;
  let fieldPropertiesDialog: FieldPropertiesDialog | null = null;
  let compressDialog: CompressDialog | null = null;
  let webLinkDialog: WebLinkDialog | null = null;

  /**
   * Asks about a web link and opens it if the reader says so.
   *
   * The whole of what this adds over `confirmAndOpen` is the three things only
   * the component knows: which document is open, which dialog to ask with, and
   * where an error goes. The decisions --- ask first, do not report a
   * cancellation, use the backend's wording, and *which* scan's list a token
   * indexes when the link came from a file whose pages were inserted --- are in
   * `weblinkdialog.ts` where a test can reach them.
   *
   * A missing dialog is a refusal rather than a silent open. It cannot happen
   * once the component is mounted; what it rules out is the ordering where a
   * link is somehow activated before `onMount`, opening an address nobody was
   * shown.
   */
  async function followWebLink(
    source: WebLinkSource,
    target: WebTarget,
  ): Promise<void> {
    const dialog = webLinkDialog;
    const doc = openDoc;
    if (!dialog || doc < 0) return;
    await confirmAndOpen(doc, source, target, {
      ask: (address) => dialog.ask(address),
      open: (id, from, token) =>
        call("open_web_link", { doc: id, source: from, token }),
      onError: (message) => say(message),
    });
  }
  /**
   * What the document says about itself, once anybody has asked.
   *
   * Cached here as well as in the backend, and the reason is what a reader sees
   * rather than what it costs: the backend's own cache makes a second read
   * nearly free, but the answer still arrives through an `await`, so a dialog
   * reopened on the same document would flash "Reading the document..." every
   * time. Cleared with the document, since it is an answer about a file.
   */
  let properties: Properties | null = null;
  /**
   * The search waiting out its pause after the last key in the find field, or
   * 0. One of the document's variables: it is for the viewer the words were
   * typed for, and tearing down the document on the other side must not
   * cancel it.
   */
  let findTimer = 0;
  /**
   * The document the backend is holding for this window, or -1 for none.
   *
   * Two jobs, and the second is why it is set the instant the open returns: a
   * late outline for a document nobody is looking at is dropped by comparing
   * against it, and it is also what the *next* open releases. A document the
   * backend has open and the frontend has forgotten is a leaked process.
   */
  let openDoc = -1;
  /**
   * Which view of {@link openDoc} the variables hold, or `NO_VIEW`. The stage,
   * the sides and the tabs know a mounted document by this: a document shown
   * on both sides is one handle and two views (`views.ts`).
   */
  let openView: ViewId = NO_VIEW;

  /**
   * The document's links, comments and outline exactly as the backend sent them.
   *
   * Kept because they are answers about the *file*, and what the viewer and the
   * panels need is the working document: every page number in them is a page of
   * the file, and after a deletion that is no longer the slot it is drawn in.
   * {@link applyPageOrder} is the one place that translates, and it re-translates
   * from these rather than from what it pushed last time --- a second pass over
   * an already-translated list would move every page twice.
   */
  let rawLinks: readonly Link[] = [];
  /**
   * The links of the other files the document's pages were inserted from, one
   * scan per file, asked for when its first page arrives. See `importedlinks.ts`.
   */
  let importedLinks = new ImportedLinks();
  let rawComments: Comments | null = null;
  /**
   * Comments whose covered words have been asked for, so none is asked twice.
   *
   * **Comment ids, and until 2026-08-26 this held page slots.** The comment
   * that justified that was half right and the half it got wrong decided the
   * key: one extraction does answer every comment on a page, and a page that
   * came back with nothing is a page that was asked --- both true, and neither
   * says the *set* may be keyed by page. A page number here is a slot, and
   * deleting a page renumbers every slot after it, so a walk that had answered
   * slot 5 would then refuse the page that moved into slot 5 and its comments
   * would read *no comment* for the rest of the session. Measured before it was
   * changed: two bare highlights, one answered, one page deleted, and the walk
   * returned nothing with a comment still wanting words.
   *
   * A comment id is what the annotation carries and it does not move. The
   * redaction walk below reaches the same conclusion from the other end and
   * goes one step further: it counts what has been answered by *region* id, so
   * that a second region drawn on a page already read is still asked about.
   * Cleared with the document, since an id means nothing across two files.
   */
  let wordsAsked = new Set<number>();
  /**
   * The words each comment covers, for the ones that have been looked up.
   *
   * **Held here as well as in the panel, and that is not belt and braces.**
   * `applyPageOrder` re-supplies the whole comment list whenever a page is
   * deleted or moved, and the panel drops its words with every `setComments` ---
   * ids are per document, so keeping them there across a list it did not compute
   * would be the one way a sentence lands on the wrong row. Without this map the
   * rows fell back to "Highlight, no comment" on the first page deletion and
   * stayed there for the rest of the session, because {@link wordsAsked} had
   * already recorded every page as asked.
   *
   * Found by a mutation that survived: clearing the panel's map on each answer
   * changed nothing observable, which is what made it worth reading the code one
   * layer out.
   */
  let commentWords = new Map<number, string>();
  /** Whether {@link fillCommentWords} is walking, so a second call stands down. */
  let fillingWords = false;
  /**
   * The words each pending region covers, by redaction id.
   *
   * `null` for a region whose page could not be read, which is a different
   * thing to tell a reader than an empty string --- `redactlist.ts` states the
   * four answers and why none of them may be collapsed. A region with no entry
   * has not been looked at yet.
   */
  let redactionWords = new Map<number, string | null>();
  /**
   * What a removal would take from each pending region, by redaction id.
   *
   * Separate from {@link redactionWords} because they answer different
   * questions and come from different places: the words a region *covers* are
   * geometry this process already holds, and this is a reading of the page's
   * content stream that only a worker can do. What it is for is the row's
   * second line --- the objects a removal cannot take.
   */
  let redactionPlans = new Map<number, RegionPlan>();
  /**
   * The colour of the boxes a redaction draws: black unless the reader chose
   * another in the redactions panel. `redactfill.ts` keeps it between sessions.
   */
  let redactionFill: Fill = readFill();
  /** Whether {@link fillRedactionWords} is walking, so a second call stands down. */
  let fillingRedactionWords = false;
  let rawOutline: Outline | null = null;

  /** Path of the open document, which is what a remembered place is keyed on. */
  let openPathName = "";
  /** Its page count, so a place can record what the document had when written. */
  let openPageCount = 0;
  /**
   * The session read at launch. Its tabs and preferences are that launch's for
   * the whole run; its places are replaced whenever the recent documents are
   * re-read, so a document opened from that list lands where the list said.
   */
  let session: Session = { places: [] };
  /** The rows of the blank page. The state behind them is `startpage.ts`'s. */
  let startRows = $state<StartRow[]>([]);
  const startPage = new StartPage((rows) => { startRows = rows; });
  let startHost = $state<HTMLDivElement | null>(null);
  /**
   * Whether pages are shown inverted.
   *
   * Held here rather than read from the viewer, because it has to survive the
   * viewer: closing one document and opening another must not quietly turn the
   * mode back off.
   */
  let invertPages = false;
  /** The language text is recognised in, and the list held while it is asked. */
  const recognitionLanguage = new RecognitionLanguage();
  /** Collapses a scroll's worth of positions into at most one write per second. */
  const places = new SessionWriter();
  const tabRecorder = new TabRecorder();
  let restoreTabs = false;
  /**
   * The variables that hold the mounted document, one entry for each.
   *
   * Every one of them is a value of the document the reader is working in.
   * `livedocument.ts` moves a document in and out of them as a whole, and an
   * entry missing here does not compile. A variable added to this file that is
   * about one document belongs in `LiveDocument` and in this table; one that is
   * about the window (a tool the reader armed, a colour they picked) does not.
   */
  const liveSlots: Slots<LiveDocument> = {
    openView: { get: () => openView, set: (value) => { openView = value; } },
    openDoc: { get: () => openDoc, set: (value) => { openDoc = value; } },
    openPathName: { get: () => openPathName, set: (value) => { openPathName = value; } },
    openPageCount: { get: () => openPageCount, set: (value) => { openPageCount = value; } },
    title: { get: () => title, set: (value) => { title = value; } },
    surface: { get: () => surface, set: (value) => { surface = value; } },
    viewer: { get: () => viewer, set: (value) => { viewer = value; } },
    sidebar: { get: () => sidebar, set: (value) => { sidebar = value; } },
    textEditor: { get: () => textEditor, set: (value) => { textEditor = value; } },
    formLayer: { get: () => formLayer, set: (value) => { formLayer = value; } },
    edits: { get: () => edits, set: (value) => { edits = value; } },
    pendingEdit: { get: () => pendingEdit, set: (value) => { pendingEdit = value; } },
    status: { get: () => status, set: (value) => { status = value; } },
    dirty: { get: () => dirty, set: (value) => { dirty = value; } },
    degraded: { get: () => degraded, set: (value) => { degraded = value; } },
    degradedGate: { get: () => degradedGate, set: (value) => { degradedGate = value; } },
    query: { get: () => query, set: (value) => { query = value; } },
    findShown: { get: () => findShown, set: (value) => { findShown = value; } },
    findTimer: { get: () => findTimer, set: (value) => { findTimer = value; } },
    error: { get: () => error, set: (value) => { error = value; } },
    offers: { get: () => offers, set: (value) => { offers = value; } },
    notice: { get: () => notice, set: (value) => { notice = value; } },
    redactedCopyPath: { get: () => redactedCopyPath, set: (value) => { redactedCopyPath = value; } },
    properties: { get: () => properties, set: (value) => { properties = value; } },
    rawLinks: { get: () => rawLinks, set: (value) => { rawLinks = value; } },
    importedLinks: { get: () => importedLinks, set: (value) => { importedLinks = value; } },
    rawComments: { get: () => rawComments, set: (value) => { rawComments = value; } },
    rawOutline: { get: () => rawOutline, set: (value) => { rawOutline = value; } },
    formNames: { get: () => formNames, set: (value) => { formNames = value; } },
    scannedForm: { get: () => scannedForm, set: (value) => { scannedForm = value; } },
    formEditing: { get: () => formEditing, set: (value) => { formEditing = value; } },
    covered: { get: () => covered, set: (value) => { covered = value; } },
    wordsAsked: { get: () => wordsAsked, set: (value) => { wordsAsked = value; } },
    commentWords: { get: () => commentWords, set: (value) => { commentWords = value; } },
    redactionWords: { get: () => redactionWords, set: (value) => { redactionWords = value; } },
    redactionPlans: { get: () => redactionPlans, set: (value) => { redactionPlans = value; } },
    fillingWords: { get: () => fillingWords, set: (value) => { fillingWords = value; } },
    fillingRedactionWords: { get: () => fillingRedactionWords, set: (value) => { fillingRedactionWords = value; } },
  };
  const stage = new Stage<LiveDocument, ViewId>(liveSlots, blankLive, () => openView);

  /**
   * Runs `work` with document `id` in the variables above, when it is still
   * mounted, and answers `undefined` when it is not.
   *
   * For everything that happens to a document without the reader pressing
   * anything in it: a frame its viewer drew, a reply that arrived for it. With
   * two documents side by side those happen to the one the reader is *not*
   * working in as well, and the code that handles them reads the variables.
   * `work` must not wait: what follows an `await` goes through here again.
   */
  function asDocument<R>(id: ViewId, work: () => R): R | undefined {
    const done = stage.within(id, work);
    return done.ran ? done.value : undefined;
  }

  /**
   * Whether the reader is still working in `view`, and it still shows `model`
   * (and, when one is named, still in the viewer `mounted`).
   *
   * The guard after an `await` for what must stop when the reader has left:
   * a question the palette is about to ask, a tool about to be armed. Work
   * that should finish in its own document whichever side the reader is in
   * goes through {@link asDocument} and needs no guard. One function, because
   * the comparisons were written by hand at each wait and the older ones
   * compared the handle, which two views of a document share.
   */
  function stillIn(view: ViewId, model: Edits | null, mounted?: Viewer | null): boolean {
    return view !== NO_VIEW && openView === view && edits === model
      && (mounted === undefined || viewer === mounted);
  }

  /**
   * Tears down what is built around the mounted document and leaves every
   * variable of it as it is with no document.
   *
   * The generation is counted up first and is not one of the variables: a
   * text editor still being built compares against it, and a count that went
   * back to zero would match an editor asked for in an earlier document.
   */
  function unmountDocument(): void {
    // The debounced find is keyed to the viewer destroyed below: left armed, it
    // fires a scan at the next document for a query the field no longer shows.
    clearTimeout(findTimer);
    findTimer = 0;
    textEditorGeneration++;
    textEditor?.destroy();
    formLayer?.destroy();
    viewer?.destroy();
    sidebar?.destroy();
    panes.unmounted(openView);
    // A lock on a document that is going is a lock on nothing. Whoever mounts
    // what replaces it locks again, at the new pair's places.
    if (scrollLock.locks(openView)) { scrollLock.release(); lockedViewers.clear(); }
    stage.clear();
  }

  /**
   * Serialises document opens. See {@link openPath}.
   *
   * Every open is queued on this, so no two bodies ever interleave and the
   * document singletons above are only ever mutated by one of them. The queue
   * itself lives in `serial.ts`, where its properties have tests that can fail
   * --- the end-to-end check that exercises this through the running app is a
   * race, and a race is a smoke test rather than a gate.
   */
  const opens = new Serial();
  /**
   * Serialises edit commands. See {@link applyEdit}.
   *
   * The same instrument as {@link opens} and for a narrower version of the same
   * reason. Every edit is its own `invoke`, and the Rust side takes the model
   * lock per command --- so the *model* is ordered and the **replies are not**.
   * Two edits issued close together (a thumbnail drag while a popup commit is
   * still in flight, an undo followed at once by a mark) can be answered out of
   * order, and the later-arriving older state is the one the window adopts:
   * `Edits.adopt` and the `setPages`/`setMarks`/`dirty` calls below take
   * whatever reply reaches them last. The document on disk stays right, because
   * the plan is read out of the model; what goes wrong is what the reader sees,
   * until the next edit happens to correct it.
   *
   * A chain rather than a flag, for the reason `serial.ts` gives about opens: a
   * flag makes the second edit a no-op, which loses the edit the reader just
   * made.
   */
  const editing = new Serial();

  /** The documents that can be shown beside this one; the rule is `partnersOf`. */
  const besidePartners = () =>
    partnersOf(tabs.all, openView, (tab) => panes.sideOf(tab.view) !== panes.focused);

  /**
   * Every command the application has, built in `appcommands.ts`.
   *
   * The list lived here, and `docs/PLAN.md` recorded what that cost: the check
   * harness runs *instead of* this component, so nothing covered either the
   * commands or the ⌘K that opens them. Moving them to a module the harness can
   * import is the same move `viewer.ts` and `palette.ts` already made.
   *
   * What stays here is the half that is genuinely this component: the actions
   * the commands reach for.
   */
  const appActions: AppActions = {
    viewer: () => viewer,
    fillForm: () => formLayer?.focus(),
    pageCount: () => status?.pageCount ?? 0,
    openDocument: () => void pickAndOpen(),
    reloadDocument: () => void reloadDocument(),
    diskChangeMode: () => diskChangeMode,
    setDiskChangeMode: (mode) => setDiskChangeMode(mode),
    closeDocument: () => void closeTab(openView),
    closeAllDocuments: () => void closeAllTabs(),
    sides: () => ({
      split: panes.split,
      focused: panes.focused,
      others: besidePartners().map((partner) => partner.name),
    }),
    showBeside: (index) => {
      const partner = besidePartners()[index]?.tab;
      if (partner) void moveTab(partner.view, otherSide(panes.focused));
    },
    moveToOtherSide: () => void moveTab(openView, otherSide(panes.focused)),
    switchSides: () => switchSides(),
    showOnBothSides: () => void showOnBothSides(),
    syncScrolling: () => syncScroll,
    toggleSyncScrolling: () => setSyncScroll(!syncScroll),
    focusOtherSide: () => { if (focusSide(otherSide(panes.focused))) viewer?.focus(); },
    restoreTabs: () => restoreTabs,
    setRestoreTabs: (restore) => setRestoreTabs(restore),
    tabsToReopen: () => tabsToReopen(session, isOpenTab).length,
    reopenLastTabs: () => void reopenLastTabs(),
    recentDocuments: () => startRows.length,
    clearRecentDocuments: () => void clearRecents(),
    tabLabels: () => tabLabelSize,
    resizeTabLabels: (direction) => { tabLabelPx = tabLabelSize.step(direction); refreshMenu(); },
    nextDocument: (delta) => { const next = tabs.neighbour(delta); if (next) void activateTab(next.view); },
    documentCount: () => tabRows.length,
    busyOpening: () => opening || copyTaskBusy || documentBusy,
    busyDocument: () => copyTaskBusy || opening || documentBusy,
    printDocument: () => void printDocument(),
    focusFind: () => focusFind(),
    toggleSearchOption: (which) => toggleSearchOption(which),
    toggleSearchScope: () => toggleSearchScope(),
    toggleSidebar: () => toggleSidebar(),
    showTab: (tab) => showTab(tab),
    toggleInvert: () => toggleInvert(),
    about: () => {
      notice = `tpdf ${appVersion}`;
    },
    // The backend reads the filesystem back after the change and answers with
    // the sentence; a refusal --- somebody else's file at the path, a cancelled
    // administrator prompt --- arrives as the error and is shown the same way.
    // `clitoolstate.ts` shows it and then asks what is there now.
    commandLineTool: (install) => void commandLineTool.run(install),
    commandLineToolOffered: (install) => commandLineTool.offered(install),
    makeDefaultPdfApp: () => {
      void call("default_pdf_app").then(
        (said) => tell(said),
        (why: unknown) => tell(String(why)),
      );
    },
    // Wrapped rather than passed straight through, because a check that lands on
    // `current` shows nothing in the header by design -- so before this, pressing
    // "Check for updates" and being up to date was indistinguishable from a
    // command that did not run.
    checkForUpdates: () => void checkAndSay(),
    automaticUpdates: () => updates.automatic,
    setAutomaticUpdates: (enabled) => setAutomaticUpdates(enabled),
    applyUpdate: () => void finishUpdateStep("install"),
    restartForUpdate: () => void finishUpdateStep("restart"),
    updateAvailable: () => updates.state.kind === "available",
    updateReady: () => updates.state.kind === "ready",
    rotatePage: (delta) => void rotatePage(delta),
    deletePage: () => void deletePage(),
    insertBlankPage: () => void insertBlankPage(),
    insertSizedPage: (name) => void insertSizedPage(name),
    importPages: () => void importPages(),
    pendingImport: () => pendingImports.current(edits?.doc ?? null),
    insertChosenPages: (pages) => void insertChosenPages(pages),
    dropImport: () => pendingImports.drop(),
    cropPage: (to) => void cropPage(to),
    redactRegion: () => viewer?.armRedact(),
    redactSelection: () => void redactSelectionAndStayArmed(),
    redactMatches: () => void redactMatches(),
    matchCount: () => viewer?.matchCount ?? 0,
    movePage: (delta) => void movePage(delta),
    undoEdit: () => void applyEdit((e) => e.undo()),
    redoEdit: () => void applyEdit((e) => e.redo()),
    canUndo: () => edits?.state.can_undo ?? false,
    canRedo: () => edits?.state.can_redo ?? false,
    markSelection: (kind) => void markSelection(kind),
    addComment: (at) => void addComment(at),
    drawBox: () => viewer?.armDraw("square"),
    drawEllipse: () => viewer?.armDraw("ellipse"),
    stamp: (name) => viewer?.armDraw("stamp", name),
    drawTextBox: () => viewer?.armDraw("textbox"),
    drawField: (kind, options = []) => {
      armedField = kind;
      armedOptions = options;
      viewer?.armDraw("field");
    },
    drawRadio: (group) => {
      armedField = "radio";
      armedOptions = [];
      armedGroup = group;
      viewer?.armDraw("field");
    },
    savedFields: () => scannedForm?.widgets.length ?? 0,
    formEditing: () => formEditing,
    setFormEditing: (on) => {
      formEditing = on;
      if (edits) viewer?.setMarks(shownMarks(edits.state));
      formLayer?.layout();
      notice = on
        ? "The document's fields can be dragged, resized by a corner, renamed and removed. Finish from the Edit menu."
        : "";
      refreshMenu();
    },
    fieldPicked: () => pickedField(propertiesDeps) !== null,
    fieldProperties: () => void changeProperties(propertiesDeps),
    canOrderTabs: () => canOrderTabs(edits?.state ?? null, scannedForm),
    orderTabs: () => {
      const view = openView;
      void applyEdit((e) => e.orderTabs()).then(() => {
        asDocument(view, () => { notice = "Saving will put the form's fields in reading order for the Tab key."; });
        refreshMenu();
      });
    },
    pickedMarks: () => viewer?.pickedCount ?? 0,
    duplicatePicked: () => void duplicate(duplicateDeps),
    arrange: (how) => {
      // `false` is too few picked or nothing to move; the command is only
      // offered with enough picked, so what is left to say is the second.
      if (viewer && !viewer.arrangePicked(how)) notice = "They are already arranged that way.";
    },
    fieldBorder: () => fieldBorder,
    setFieldBorder: (border) => {
      fieldBorder = border;
      const kept = writeFieldBorder(border);
      notice = (border
        ? "Text fields placed from now on are drawn with a line round them."
        : "Text fields placed from now on draw nothing until they are filled.")
        + (kept ? "" : " The choice could not be saved and lasts until tpdf closes.");
      refreshMenu();
    },
    signature: () => void addSignature(),
    editText: () => void editExistingText(),
    draw: () => viewer?.armDraw("ink"),
    erase: () => viewer?.armErase(),
    hasSelection: () => (status?.selected ?? 0) > 0,
    removeMark: () => removeMark(),
    hasOpenMark: () => viewer?.canRemoveMark ?? false,
    removeRedaction: () => removeRedaction(),
    hasPickedRedaction: () => (viewer?.redactionPicked ?? -1) >= 0,
    canEditComment: () => viewer?.commentEditable ?? false,
    editComment: () => viewer?.editComment(),
    canReplyToComment: () => viewer?.commentReplyable ?? false,
    replyToComment: () => viewer?.replyToComment(),
    canDeleteComment: () => viewer?.commentDeletable ?? false,
    deleteComment: () => viewer?.deleteComment(),
    setMarkColor: (id) => chooseMarkColor(id),
    setNib: (id) => chooseNib(id),
    markColor: () => markColor.id,
    saveDocument: () => void saveDocument(),
    isDirty: () => dirty,
    saveCopy: () => void saveCopy(),
    redactCopy: () => void redactCopy(),
    redactRasterCopy: () => void redactRasterCopy(),
    recogniseText: () => void recogniseText(),
    findHiddenText: () => void findHiddenText(),
    chooseRecognitionLanguage: () => void chooseRecognitionLanguage(),
    recognitionLanguages: () => {
      const offered = recognitionLanguage.question();
      return offered ? { offered, current: recognitionLanguage.language } : null;
    },
    setRecognitionLanguage: (raw) => setRecognitionLanguage(raw),
    dropRecognitionLanguages: () => recognitionLanguage.drop(),
    protectCopy: () => void protectCopy(true),
    unprotectCopy: () => void protectCopy(false),
    compressCopy: () => void compressCopy(),
    redactDocument: () => redactDocument(),
    extractPages: (slots) => void extractPages(slots),
    splitDocument: (groups) => void splitDocument(groups),
    mergeDocuments: () => void mergeDocuments(),
    fromPictures: () => void fromPictures(),
    signDocument: () => void signDocument(),
    signableFields: () => emptySignatureFields(scannedForm).length,
    signField: () => {
      const first = emptySignatureFields(scannedForm)[0];
      const target = first && edits ? signTarget(first, edits.state.pages) : null;
      if (target) void signDocument(target);
    },
    showProperties: () => void showProperties(),
  };

  /**
   * Turns the page the reader is on, in the document rather than in the view.
   *
   * The *page* comes from the viewer and the *turn* comes from the model: the
   * reader points at a page and asks for a quarter more, and what that page's
   * rotation becomes is the journal's arithmetic, replayed on undo. Nothing here
   * adds one to anything.
   */
  async function rotatePage(delta: number): Promise<void> {
    const at = viewer?.position.page;
    if (at === undefined) return;
    await applyEdit((e) => e.rotate(at, delta));
  }

  /**
   * Marks the selected text, as an annotation on the document.
   *
   * The *rectangles* come from the viewer and everything else comes from the
   * model: whether the mark is accepted, what its identity is, and what undo
   * does with it. Nothing here decides any of that --- see `edits.rs`.
   *
   * A selection can span pages, and each page is a mark of its own: a
   * `/QuadPoints` array addresses one page, so a highlight running from page 3
   * to page 4 is two annotations however it is presented. They are applied in
   * order, so undo takes them off one page at a time --- which is the honest
   * behaviour rather than a pleasant one, and the alternative is a journal
   * entry that groups commands, which the model does not have.
   *
   * **One function for all three kinds**, taking the kind rather than three
   * near-copies of the loop above. The per-page split, the ordering and the
   * refusal are the same for a highlight, an underline and a strikeout; only
   * the subtype the writer puts in the file differs.
   */
  async function markSelection(kind: MarkKind): Promise<void> {
    // The document the marks are for. Each one is a wait, and the reader may
    // press in the other side during it: what follows a wait is done as this
    // document and never as "the one in the variables".
    const view = openView;
    const marks = viewer?.selectionQuadsByPage() ?? [];
    for (const { page, quads, text } of marks) {
      // The mark that appears is identified by difference, in `markMade` ---
      // `addComment` below gives the argument for asking it that way rather
      // than taking the last mark or the highest id.
      const made = await markMade(
        (run) => applyEdit(run, view),
        (e) => e.mark(kind, page, quads, [], "", markColor.rgb),
      );
      // Absent when the model refused. Nothing to record and nothing to say ---
      // `applyEdit` has already shown the refusal.
      if (made) asDocument(view, () => covered.set(made.id, text.slice(0, COVERED_CHARS)));
    }
    // `applyEdit` painted the panel already --- but it painted it *before* this
    // loop knew which id to file the words under, so every row it drew says
    // nothing was typed on the mark. One repaint here rather than one inside
    // the loop after each `covered.set`: a selection over four pages makes four
    // marks, and the three intermediate paints would each be replaced within
    // the millisecond by the next `applyEdit`.
    if (marks.length > 0) asDocument(view, () => {
      if (edits) sidebar?.setMarks(markRows(edits.state.marks, edits.map));
    });
  }

  /**
   * Marks the selected text for removal, one region per line it covers.
   *
   * **`markSelection`'s shape and a different destination.** Both take their
   * geometry from `selectionQuadsByPage`, which is what makes the two agree
   * about space without either of them reasoning about it: those quads are what
   * `Edits.mark` takes, and `Edits.redact` documents itself as taking a region
   * in exactly that space. A selection spanning pages becomes regions on each,
   * because a region belongs to one page the way `/QuadPoints` does.
   *
   * One region per run rather than one box per page --- see {@link areasFrom},
   * where that decision lives and can be tested. Nothing here decides anything:
   * the model accepts or refuses, and `applyEdit` has already said so.
   *
   * **No `covered` bookkeeping**, which is the one line of `markSelection` that
   * is missing rather than moved. That map exists so a mark's row can show the
   * words it sits on; a redaction's row is about what will be *removed*, and
   * the panel gets that from the backend's own plan rather than from what the
   * reader had selected when they asked. The two would be the same string today
   * and would part company the moment route B took a whole line.
   */
  async function redactSelection(): Promise<void> {
    const view = openView;
    const selected = viewer?.selectionQuadsByPage() ?? [];
    for (const { page, quads } of selected) {
      for (const area of areasFrom(quads)) {
        await applyEdit((e) => e.redact(page, area), view);
      }
    }
  }

  /**
   * {@link redactSelection}, and the tool left armed in the document it was
   * asked in: the next word the reader double-clicks there is marked without
   * coming back to the command. Reported from use: choosing it again for every
   * passage was the tedious part. Esc puts the tool away.
   */
  async function redactSelectionAndStayArmed(): Promise<void> {
    const view = openView;
    await redactSelection();
    asDocument(view, () => {
      viewer?.clearSelection();
      viewer?.armRedact();
    });
  }

  /**
   * Marks every search match for removal.
   *
   * {@link redactSelection}'s shape over a different set of quads, and two
   * things are genuinely different rather than copied.
   *
   * **It can refuse before it starts.** Above {@link MAX_MATCHES_TO_MARK} the
   * reader is told the number and asked to narrow the search, because the review
   * list is this subsystem's whole safety mechanism and a list nobody can read
   * is the same as no list. Marking the first five hundred and reporting success
   * would leave them reviewing a list that *understates* their own search.
   *
   * **A page that could not be read stops everything.** `matchQuadsByPage`
   * answers `null` rather than a shorter list, and this says so rather than
   * marking what it did get: a partial mark becomes a partial removal that a
   * reader is then told is clean, which is the one thing §6 forbids.
   */
  async function redactMatches(): Promise<void> {
    const view = openView, searched = viewer;
    if (!searched) return;
    const refusal = tooManyMatchesToMark(searched.matchCount);
    if (refusal) {
      say(refusal);
      return;
    }
    const found = await searched.matchQuadsByPage();
    if (found === null) {
      asDocument(view, () => say(
        "Some of the pages with matches on them could not be read, so nothing " +
          "was marked. Nothing has been changed.",
      ));
      return;
    }
    for (const { page, quads } of found) {
      for (const area of areasFrom(quads)) {
        await applyEdit((e) => e.redact(page, area), view);
      }
    }
  }

  /**
   * Drops a comment on the page and opens its note ready to be typed in.
   *
   * **The one thing here that is not `markSelection`'s shape**, and the
   * difference is the whole of what a comment is: the other three take their
   * geometry from a selection, so they can make several marks at once --- one
   * per page a selection crosses --- and refuse when there is no selection.
   * This one takes a *point* and always makes exactly one, because a comment is
   * something a reader puts somewhere rather than something they apply to words.
   *
   * The note opens straight after, with the keyboard in it. A bubble a reader
   * has to find and click before they can say anything is a bubble that gets
   * dropped and abandoned --- and the box is also the only thing on screen that
   * tells them the comment is theirs to type in rather than the document's.
   */
  async function addComment(at: ScreenPoint | null): Promise<void> {
    // **With no point, this arms rather than places.** It used to place, at
    // `commentAt`'s no-pointer answer --- the top-left of the visible page ---
    // which is a defensible spot and reads as a command that ignored where the
    // reader was looking. Reported from use: *"I would expect the cursor to
    // become a speech bubble to place it, instead of adding it always to the top
    // left."* So the palette and the menu bar now arm the tool, the next press
    // on a page drops the bubble there, and the viewer paints a ghost of it
    // under the pointer meanwhile --- and Enter still places it at the old spot,
    // so a reader who reached the command from the keyboard is not left in a
    // mode they cannot finish. See `Viewer.paintCommentGhost` and `placeComment`.
    //
    // A right-click keeps placing immediately. It already names a point, so
    // arming would ask a reader who has just said *here* to say it again.
    if (!at) {
      viewer?.armDraw("note");
      return;
    }
    const view = openView;
    const where = viewer?.commentAt(at);
    if (!where) return;
    // Which ids existed before, so the one that appears can be identified by
    // difference rather than by guessing. "The last mark in the list" and "the
    // highest id" are both inferences about how the model numbers and orders
    // things, and neither is written down anywhere as a promise --- a set
    // difference needs no promise at all. `markMade` in `edits.ts` takes it.
    const made = await markMade(
      (run) => applyEdit(run, view),
      (e) => e.mark("note", where.page, where.quads, [], "", markColor.rgb),
    );
    // Absent if the model refused --- an empty quad, a page that is gone. The
    // refusal has already been shown by `applyEdit`, so there is nothing to say
    // here beyond not opening a note on a mark that was never made. The note
    // opens in the document the comment was put in, wherever the reader is now.
    if (made) asDocument(view, () => viewer?.showMark(made.id));
  }

  /**
   * Records a mark the reader drew, and opens its note.
   *
   * The same shape as {@link addComment} above once the gesture is over: make
   * it, find it by set difference, open the box. Written out rather than shared
   * with that function, because the two differ in the one line that matters ---
   * where the geometry comes from --- and a helper taking a callback would hide
   * exactly the distinction the two exist to draw.
   *
   * The note opens for the comment's reason: a mark a reader cannot immediately
   * say anything about is one they draw and abandon, and the box is also the
   * only thing on screen saying the rectangle is theirs rather than the
   * document's.
   */
  async function addSignature(): Promise<void> {
    const target = viewer, doc = openDoc, view = openView, model = edits;
    if (!target || doc < 0 || !signatureDialog || documentBusy || opening || copyTaskBusy) return;
    const image = await signatureDialog.ask();
    if (image && stillIn(view, model, target)) target.armSignature(image);
  }

  async function drawn(
    kind: MarkKind,
    page: PageId,
    shape: Drawn,
    stamp: StampName | null,
  ): Promise<void> {
    // Called by the viewer the shape was drawn in, as its document.
    const view = openView;
    // A field has to have a name the moment it exists. See `fieldnames.ts`.
    const placed = kind === "field"
      ? placing(
        { kind: armedField, options: armedOptions, group: armedGroup },
        fieldBorder, formNames, edits?.state.marks ?? [], scannedForm,
      )
      : undefined;
    const field = placed?.field;
    const name = placed?.name ?? "";
    const made = await markMade((run) => applyEdit(run, view), (e) =>
      e.mark(
        kind,
        page,
        shape.quads,
        shape.strokes,
        name,
        markColor.rgb,
        stamp,
        // Never a reply: this is a mark a reader dragged out, and a reply is
        // made from the panel beside the comment it answers.
        null,
        // **From the shape, not from `markNib`.** The viewer holds the armed nib
        // and painted the preview with it, so taking it from here is what makes
        // the line a reader watched and the line they get one number --- see
        // `Drawn.width`. Reading the state below instead would work today and
        // would be a second copy of it.
        shape.width,
        shape.image,
        field,
      ),
    );
    // A group is several buttons, so the tool stays armed for the next one
    // and no name box opens over the place it would go; Escape puts it down.
    asDocument(view, () => {
      if (field?.kind === "radio") viewer?.armDraw("field");
      else if (made) viewer?.showMark(made.id);
    });
  }

  /**
   * Takes the mark whose note is open off the page it is on.
   *
   * *Which* mark is the viewer's answer, because the open note is where a
   * reader says which one they mean --- there is no selected-mark concept
   * beside it, and two ways to name the subject of a command is how they come
   * to disagree. So this hands the question straight back to the viewer, which
   * answers it the same way for the button inside the note; both arrive at
   * `onMarkRemove` below, and the removal itself is the model's.
   */
  function removeMark(): void {
    viewer?.removeMarks();
  }

  /**
   * Takes the picked region out of the list of what is to be removed.
   *
   * {@link removeMark}'s twin, and *which* region is the viewer's answer for
   * that function's reason exactly: the pick is where a reader says which one
   * they mean, and a second way to name it is how the two come to disagree.
   *
   * `applyEdit` and `unredact` are the same path the review panel's remove
   * control takes, so this journals, undoes and refreshes the panel exactly as
   * that does --- and the pick clears itself when the model stops listing the
   * region, which `Viewer.setRedactions` does rather than this.
   */
  function removeRedaction(): void {
    const id = viewer?.redactionPicked ?? -1;
    if (id < 0) return;
    void applyEdit((e) => e.unredact(id));
  }

  /**
   * Picks the colour marks are drawn in.
   *
   * **One gesture, both meanings**, which is the whole of the rule
   * `markcolors.ts` states: it sets what the next mark will be, and if a mark's
   * note is open it draws that mark in it too. A reader who has just made a
   * highlight and wants it green means the second; a reader who has not made one
   * yet means the first; and asking which would be asking them to know that
   * there are two.
   *
   * *Which* mark is the viewer's answer for {@link removeMark}'s reason --- the
   * open note is where a reader says which one they mean --- and it also drops
   * a press that would recolour a mark to the colour it already is.
   */
  function chooseMarkColor(id: string): void {
    const chosen = swatch(id);
    // No such swatch means a command id that named one, which cannot happen from
    // the registry: every `edit.color.*` command is built from `PALETTE`.
    if (!chosen) return;
    markColor = chosen;
    viewer?.recolorOpenMark(chosen.rgb);
  }

  /**
   * Picks how thick the next drawing is.
   *
   * **One direction, unlike {@link chooseMarkColor} above.** A colour applies to
   * the mark whose note is open as well as to the next one; a nib applies only
   * to the next, because changing an existing drawing's width has to rebuild its
   * rectangle --- `Mark::width` says what that would cost. So there is nothing
   * here about an open mark, and that absence is a decision rather than a gap.
   *
   * The viewer is set first and the label second, so the two cannot disagree
   * about a drawing: if this ever drifts the reader sees the wrong word beside a
   * line drawn at the right weight, rather than the reverse.
   */
  function chooseNib(id: string): void {
    const chosen = nib(id);
    // No such nib means a command id that named one, which cannot happen from
    // the registry: every `edit.nib.*` command is built from `NIBS`.
    if (!chosen) return;
    // The nib is the window's, so the viewer on the other side takes it too.
    stage.each(() => viewer?.setNib(chosen.pt));
    markNib = chosen;
  }

  /**
   * Removes the page the reader is on from the document.
   *
   * The page comes from the viewer and the rule comes from the model: a document
   * must keep at least one page, and that refusal arrives as a message rather
   * than being predicted here --- see `edits.rs`. Undo puts the page back where
   * it was, which is why this asks nothing before doing it.
   */
  async function deletePage(): Promise<void> {
    const at = viewer?.position.page;
    if (at === undefined) return;
    await applyEdit((e) => e.delete(at));
  }

  /**
   * Puts a blank page after the one the reader is on.
   *
   * **The size is the page they are looking at**, unturned, which is what makes
   * a blank page in an A4 document A4 and one in a US Letter document Letter.
   * `pageSize` rather than `displayedSize`: a `/MediaBox` is the page's own
   * size, and a reader who has turned the view has not asked for a page in
   * landscape.
   *
   * An estimated size is used rather than refused. `knowsPageSize` is false only
   * before a page has ever rendered, and a blank page the size of the running
   * mean of a document's pages is a better answer than a refusal a reader would
   * read as the command being broken --- the estimate is exact for the great
   * majority of documents, where every page is the same size.
   */
  async function insertBlankPage(): Promise<void> {
    const at = viewer?.position.page;
    const size = at === undefined ? undefined : viewer?.pageSize(at);
    if (at === undefined || !size) return;
    await applyEdit((e) => e.insertPage(at, [size.width_pt, size.height_pt]));
  }

  /**
   * The insert that is waiting for the reader to say which pages. See
   * `pendingimport.ts`.
   *
   * The release is posted rather than awaited, and its answer dropped: the
   * backend answers `false` for an import it no longer waits on, which is the
   * ordinary case after a commit, and there is nobody to tell about a cancel
   * that failed --- the pool goes at the document's close regardless.
   */
  const pendingImports = new PendingImports((doc, pending) => {
    void call("page_import_cancel", { doc, pending }).catch(() => {});
  });

  /**
   * Opens a file the reader picks and asks which of its pages to insert after
   * the one they are on.
   *
   * The dialog first and the edit last, and not inside `applyEdit`: the edit
   * queue is what every later command waits behind, and a reader looking at a
   * file dialog --- or at the range question --- would hold it for as long as
   * they looked. The page is read again at the commit for the same reason:
   * they may have scrolled.
   *
   * The palette is closed before anything else, which dismisses a range
   * question still open from an earlier file and so releases that file. Left
   * open, the question for this file would be asked over it.
   */
  async function importPages(): Promise<void> {
    if (opening || !edits) return;
    const model = edits, view = openView;
    palette?.close();
    const picked = await openDialog({
      multiple: false,
      directory: false,
      title: "Choose a document to insert pages from",
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    const path = typeof picked === "string" ? picked : null;
    if (!path || !stillIn(view, model)) return;
    await importPagesFrom(path);
  }

  /**
   * {@link importPages} once the file is chosen: the backend opens and checks
   * it, and the palette asks for the pages. Its own function so the window
   * check can reach the whole path but the dialog, which no harness can answer.
   *
   * A file that will not open, is encrypted or cannot be read is refused here,
   * before the question is asked, in the backend's own sentence.
   */
  async function importPagesFrom(path: string): Promise<void> {
    const model = edits, view = openView;
    if (!model) return;
    let prepared;
    try {
      prepared = await model.prepareImport(path);
    } catch (error) {
      asDocument(view, () => { if (edits === model) say(String(error)); });
      return;
    }
    // The document went away while the file opened: its close released the
    // file already, and there is nothing to ask about. A reader who has gone
    // to the other side is not asked either: the question is put to the
    // document they are in, and the file is released when this one closes.
    if (!stillIn(view, model)) return;
    pendingImports.hold(model.doc, prepared);
    palette?.askFor("edit.insertPages.range");
  }

  /**
   * Inserts the pages the reader named of the waiting file, after the page
   * they are on now. `edits.ts`'s `importPages` releases the file if that page
   * has gone; the backend releases it if the model refuses.
   */
  async function insertChosenPages(pages: number[]): Promise<void> {
    const model = edits;
    if (!model) return;
    const waiting = pendingImports.take(model.doc);
    if (!waiting) return;
    const release = () =>
      void call("page_import_cancel", { doc: waiting.doc, pending: waiting.pending }).catch(() => {});
    const at = viewer?.position.page;
    if (at === undefined) return release();
    await applyEdit((e) => {
      // The queue runs this later, against whichever document is open then.
      // The file was prepared for `model`, so another one releases it.
      if (e !== model) {
        release();
        return Promise.resolve(e.state);
      }
      return e.importPages(at, waiting.pending, pages);
    });
  }

  /**
   * Inserts a blank page of a named size after the one the reader is on.
   *
   * The pair comes from `dimensionsOf` rather than being read off the table
   * here, and that is the whole reason that function exists: this file is
   * reached by no unit test, so a width and a height transposed at this call
   * site would be checked by nothing. There it has a test and a mutation.
   */
  async function insertSizedPage(name: PageSizeName): Promise<void> {
    const at = viewer?.position.page;
    if (at === undefined) return;
    await applyEdit((e) => e.insertPage(at, dimensionsOf(name)));
  }

  /**
   * Crops the page the reader is on to its ink, or puts the file's box back.
   *
   * The measurement is the backend's and names a page of the **file**, while the
   * command names a page of the **model** --- the two vocabularies meet here, as
   * they do for every other page operation, and the source index comes out of
   * the state reply rather than being assumed equal to the slot.
   *
   * A page with no ink is left alone and said so: cropping a blank page to
   * nothing is not what "crop to content" means, and silence would read as the
   * command being broken.
   */
  async function cropPage(to: "content" | "reset" | "drag"): Promise<void> {
    const at = viewer?.position.page;
    // The model and the view are named once, here. The measurement below is a
    // wait, and an edit sent after it as "the document in the variables" would
    // crop a page of the other side that happens to have the same id.
    const model = edits, view = openView;
    if (at === undefined || !model) return;
    if (to === "drag") {
      // Arms and returns: the rest of this gesture happens when the reader
      // lets go, in `cropTo`. Nothing is measured and no page is named here,
      // because the page is whichever one they press on --- which need not be
      // the one they are scrolled to.
      viewer?.armCrop();
      return;
    }
    if (to === "reset") {
      // The slot is resolved inside the edit, for the reason `cropTo` gives
      // below: `applyEdit` queues, so this callback can run after a deletion
      // above this page has landed, and `at` would then name whichever page had
      // moved into the slot --- putting the file's box back on a page nobody
      // asked about. An id cannot move.
      const page = model.state.pages[at]?.id;
      if (page === undefined) return;
      await applyEdit((e) => {
        const slot = slotOfIdIn(e.state.pages, page);
        return slot === undefined ? Promise.resolve(e.state) : e.crop(slot, null);
      }, view);
      return;
    }
    const shown = model.state.pages[at];
    // Where the page is drawn from, which for a page inserted from another file
    // is that file: its ink is measured by the worker that renders it.
    const source = shown === undefined ? undefined : addressOf(shown, model.doc);
    if (shown === undefined || source === undefined) {
      // Two ways to get here and only one of them is worth a message: a slot
      // that is not in the document is a stale press, and a page tpdf made is a
      // reader asking a reasonable question about a page with nothing on it.
      if (shown !== undefined) say("A blank page has nothing to crop to.");
      return;
    }
    // The page's identity, taken before the measurement goes out. `at` is a
    // slot and it is about to stop naming this page: the round trip below can
    // land after a deletion above it, and a crop sent against the old slot
    // crops whichever page has moved into it. `cropTo` next door does the same
    // for the same reason; a `source` is safe to read early and a slot is not.
    const page = shown.id;
    const box = await contentBox(source.doc, source.page).catch(() => null);
    if (!box) {
      asDocument(view, () => say("There is nothing on this page to crop to."));
      return;
    }
    await applyEdit((e) => {
      const slot = slotOfIdIn(e.state.pages, page);
      return slot === undefined ? Promise.resolve(e.state) : e.crop(slot, box);
    }, view);
  }

  /**
   * Crops a page to the rectangle the reader dragged out on it.
   *
   * **Two round trips, and the first one is the whole reason this is not one
   * line.** The viewer hands back a rectangle in the file's display space,
   * because that is the space every rectangle in the frontend is in; a crop box
   * is in the page's own unrotated space, and turning between them needs the
   * page's `/Rotate`, which this side is deliberately never told. So the
   * backend is asked, and only then is the edit made.
   *
   * **The page arrives as a model id and three different numbers name a page
   * here**, which is the trap `docs/TRAPS.md` records as an id and a slot both
   * being `number`. The mapping needs a page of the *file* (`source`), the model
   * command takes a *slot*, and what the viewer hands over is an *id*. Each is
   * read from the id rather than assumed equal to it.
   *
   * The slot is read **inside** the edit and not beside `source`, and that is
   * the one non-obvious line here: the mapping above is a round trip, and a page
   * can be moved or deleted while it is in flight, which changes every slot
   * after it. A `source` cannot move --- it names the page of the file this page
   * came from --- so reading that early is safe and reading the slot early is
   * not.
   *
   * A failure is said out loud rather than swallowed. Every other gesture here
   * either succeeds or leaves the tool armed, and this is the only one that can
   * fail *after* the reader has finished dragging --- silence would read as a
   * crop that did nothing.
   */
  async function cropTo(
    id: number,
    rect: [number, number, number, number],
  ): Promise<void> {
    // Called by the viewer the rectangle was dragged in, as its document, and
    // named here for the edit that follows the wait below.
    const model = edits, view = openView;
    if (!model) return;
    // What the viewer hands over is an id, spelt as a plain `number` because
    // that is what the callback's type says; named as one here so that the
    // slot lookup below cannot be handed the wrong kind of page number.
    const page = pageId(id);
    const shown = model.state.pages.find((p) => p.id === page);
    // For a page inserted from another file, the box is in *that* file's page
    // space and turned by its `/Rotate`, so it is that file's worker that is
    // asked --- the model writes the box onto the page object it imports.
    const source = shown === undefined ? undefined : addressOf(shown, model.doc);
    if (source === undefined) {
      // `cropToContent`'s reasoning, and here the refusal is the model's as
      // well: a crop box is measured against a page of the file, and there is
      // none behind a page tpdf made --- see `Refusal::CropOnMadePage`.
      // This return is why that refusal has never reached a reader: the model
      // has one and nothing gets that far. Its sibling for a redaction has no
      // such guard, which is how the shared message it used to carry --- about
      // marking --- was shown to somebody dragging a region.
      if (shown !== undefined) say("A blank page cannot be cropped.");
      return;
    }
    const box = await cropBox(source.doc, source.page, rect).catch(() => null);
    if (!box) {
      asDocument(view, () => say("That rectangle could not be turned into a crop."));
      return;
    }
    await applyEdit((e) => {
      const slot = slotOfIdIn(e.state.pages, page);
      return slot === undefined ? Promise.resolve(e.state) : e.crop(slot, box);
    }, view);
  }

  /**
   * Moves the page the reader is on by one slot.
   *
   * A destination slot rather than a direction, because that is what `edits.ts`
   * inverts into the neighbour the model wants --- and because the day the page
   * strip can be dragged, the destination is what a drag produces and this is
   * already the call it makes. Off either end is a no-op, decided there rather
   * than guarded here.
   */
  async function movePage(delta: number): Promise<void> {
    const at = viewer?.position.page;
    if (at === undefined) return;
    await applyEdit((e) => e.move(at, at + delta));
  }

  /**
   * The edit that has been asked for and not yet answered.
   *
   * Recorded because one caller has to wait for it and the twenty that fire
   * `void applyEdit(...)` must not: {@link saveDocument} writes the model's own
   * answer to disk, and a note the reader typed a moment ago is a `renote` still
   * in flight. Saving over it would put a highlight in the file with an empty
   * note while the box on screen shows the words. Every other caller is a
   * redraw, and a redraw that waits for the last one is a slower redraw.
   */
  let pendingEdit: Promise<unknown> = Promise.resolve();

  /**
   * Runs one edit and moves the viewer to the state it produced.
   *
   * Every route in goes through here, which is the same reasoning that put the
   * history recording inside `goToDestination` rather than at its four callers:
   * the fifth caller is the one that forgets. What it must not become is a place
   * where the *next* state is computed --- it is handed one, and its whole job
   * is to redraw what differs.
   *
   * Resolves with the state `view` adopted, or with null when it adopted none:
   * the edit was refused here or by the model, or the view has gone. A caller
   * with something to do afterwards reads that answer and does it through
   * `asDocument(view, ...)`. It does not read `edits` or `viewer` again, which
   * are the other document's once the reader has pressed in the other side.
   *
   * `view` is the document the edit is for. Left out, it is the one the reader
   * is in now, which is right for a caller that has not waited for anything; a
   * caller that has waited passes the view it took when it was asked.
   */
  function applyEdit(
    run: (edits: Edits) => Promise<EditState>,
    view: ViewId = openView,
  ): Promise<EditState | null> {
    if (copyTaskBusy || (documentBusy && !committingPopup)) return Promise.resolve(null);
    // Queued rather than started, so a reply can never be adopted after a
    // later one. See {@link editing}.
    //
    // The document is named here, when the edit is asked for. The queue runs
    // it later, and the reader may be working in the other side by then.
    const done = editing.run(() => runEdit(view, run));
    // What a save waits for is the edit of its own document.
    if (view === openView) pendingEdit = done;
    else asDocument(view, () => { pendingEdit = done; });
    return done;
  }

  async function editExistingText(): Promise<void> {
    const model = edits, mounted = viewer, host = surface, view = openView;
    if (!model || !mounted || !host || documentBusy) return;
    const generation = ++textEditorGeneration;
    // Asked again after each wait. A reader who has gone to the other side has
    // left this, as one who switched tabs has.
    const wanted = () => generation === textEditorGeneration && stillIn(view, model, mounted);
    try {
      await settleDocument();
      if (!wanted()) return;
      if (model.state.redactions.length) throw new Error("Finish or remove pending redactions before editing text.");
      const page = model.state.pages[(asDocument(view, () => status?.page) ?? 1) - 1];
      if (!page) return;
      const runs = await call("document_text_runs", { doc: model.doc, page: page.id });
      if (!wanted()) return;
      if (!runs.runs.length) throw new Error("This page has no supported text to edit.");
      asDocument(view, () => textEditor?.destroy());
      const editor: TextEditor = new TextEditor(host, page.id, runs,
        // By the page's identity, not by `runs.page`: that number is a page
        // number of whichever document this page is drawn from, and for an
        // inserted page it names a different page of the opened file.
        (run) => mounted.textAnchor(page.id, run.display_rect),
        async (change) => {
          let result: EditState | undefined;
          let failure: unknown;
          await applyEdit(async (current) => {
            if (current !== model) throw new Error("The document changed before the text was applied.");
            try { result = await current.replaceText(page.id, change); return result; }
            catch (error) { failure = error; throw error; }
          }, view);
          if (!result) throw failure ?? new Error("Text editing is currently unavailable.");
          return result;
        }, () => { editor.destroy(); asDocument(view, () => { if (textEditor === editor) textEditor = null; }); },
        (change) => call("document_text_runs", { doc: model.doc, page: page.id, change }),
        // No draft: the runs as the pending edits leave them, for the outlines.
        () => call("document_text_runs", { doc: model.doc, page: page.id }));
      asDocument(view, () => { textEditor = editor; });
      editor.update(model.state); editor.setBusy(documentBusy);
    } catch (error) {
      if (generation === textEditorGeneration) asDocument(view, () => {
        if (edits === model) say(`Cannot edit this text: ${error instanceof Error ? error.message : String(error)}`);
      });
    }
  }

  async function runEdit(
    id: ViewId,
    run: (edits: Edits) => Promise<EditState>,
  ): Promise<EditState | null> {
    const model = asDocument(id, () => (viewer ? edits : null));
    if (!model) return null;
    try {
      const after = await run(model);
      // Everything below reads and writes the variables of the document the
      // edit was made in, so it runs as that document.
      const adopted = asDocument(id, () => adoptEdit(model, after)) === true;
      // A second view of the document draws the same state. An edit is the
      // document's, whichever view it was made in.
      for (const twin of tabs.all) {
        if (twin.edits === model && twin.view !== id) asDocument(twin.view, () => adoptEdit(model, after));
      }
      return adopted ? after : null;
    } catch (e) {
      // Shown rather than logged. A refusal here is about the document --- a page
      // that is gone, a handle that is not open --- and a rotate command that
      // silently does nothing reads as a broken application.
      asDocument(id, () => say(String(e)));
      return null;
    }
  }

  /**
   * Moves the mounted document's viewer and panels to the state an edit
   * produced. False, and nothing moved, when the variables hold another model.
   */
  function adoptEdit(model: Edits, after: EditState): boolean {
    if (edits !== model) return false;
    // Only when the pages moved, and the viewer is what answers that: every
    // call below throws work away --- the strip's thumbnails, the panels' rows
    // --- and a turn moves no page, so doing it unconditionally would make
    // rotating a page cost a re-render of the whole strip.
    if (viewer?.setPages(after.pages)) {
      applyPageOrder();
      // The strip, which is the one consumer that cannot work out for itself
      // that anything happened: its thumbnails are held under the row they
      // were rendered for, and a *move* leaves the row count exactly as it
      // was. Called here rather than in `applyPageOrder`, which also runs
      // when a late outline or a late set of comments arrives and has no
      // business throwing away a strip somebody is looking at.
      sidebar?.thumbnails?.setPages(after.pages.length);
    }
    // Any other file whose first pages just arrived has links nobody has read.
    void fetchImportedLinks(model);
    viewer?.setMarks(shownMarks(after));
    formLayer?.update(after);
    formLayer?.layout();
    if (viewer?.setTextEdits(after.text_edits ?? [])) sidebar?.thumbnails?.setPages(after.pages.length);
    if (viewer?.setFieldEdits(scannedForm?.widgets ?? [], after.fields ?? [])) sidebar?.thumbnails?.setPages(after.pages.length);
    textEditor?.update(after);
    // The pending redactions arrive on the same reply and are pushed the same
    // way. Not through `setMarks`: they are a separate list for the reason
    // `docmodel.rs` states, and one setter taking both would be the first
    // place that distinction could be lost.
    viewer?.setRedactions(after.redactions);
    // Beside the viewer's own copy rather than in `applyPageOrder`: the marks
    // arrive with this answer, where the links, comments and outline are
    // answers about the *file* that this reconciles against a new page order.
    sidebar?.setMarks(markRows(after.marks, model.map));
    // Two calls rather than one, because the rows and the words on them
    // change for different reasons: marking a region changes the list at
    // once, and the words under it arrive a page-extraction later. The
    // scheduler is a no-op when every page in the list has already been read,
    // which is every edit after the first on a given page.
    sidebar?.setRedactions(redactionRows(after.redactions, model.map));
    void fillRedactionWords();
    dirty = after.dirty;
    refreshTabs();
    // Undo and Redo are the two menu items whose enablement moves on every
    // edit, which is why this is here rather than only at the ends of an open.
    refreshMenu();
    return true;
  }

  /**
   * Reads the links of each other file the document's pages come from, once.
   *
   * Through the other file's own handle, because its pages are its own: the
   * opened document's scan has nothing to say about them. Quiet on failure for
   * the reason the opened document's scan is --- a file with no readable links
   * is the common case --- and `ImportedLinks` records the handle as asked
   * either way, so a failing scan is not retried on every edit.
   */
  async function fetchImportedLinks(model: Edits): Promise<void> {
    // The list is this document's own, held while the scans are in flight:
    // the variable is another document's when the reader changes sides.
    const mine = importedLinks, view = openView;
    for (const doc of mine.wanted(model.map)) {
      try {
        const result = await call("document_links", { doc });
        const recorded = asDocument(view, () => {
          if (edits !== model) return false;
          importedLinks.record(doc, result.items);
          applyPageOrder();
          return true;
        });
        if (!recorded) return;
      } catch {
        // Deliberately quiet; see above.
      }
    }
  }

  /**
   * Re-reads the document's links, comments and outline against the page order.
   *
   * Every page number the backend sent is a page of the *file*, and after a
   * deletion that is no longer the slot it is drawn in --- so a link would be
   * hit-tested over the wrong page, a comment would open against one, and an
   * outline row would scroll somewhere nobody asked for. `pages.ts` holds the
   * rules; this is the one place they are applied, and it re-reads the answers
   * the backend sent rather than the ones it pushed last time.
   *
   * Called on an order change and after each answer arrives, since those two
   * races: a document whose links land *after* a page was deleted would
   * otherwise be translated by nobody.
   */
  function applyPageOrder(): void {
    const pages = edits?.map ?? NO_PAGES;
    viewer?.setLinks(allLinksIn(rawLinks, importedLinks.all, pages));
    // An answer that has not arrived is left alone rather than pushed as
    // `null`: to these panels `null` means "this document's comments could not
    // be read", which is a different thing to tell a reader than "not yet". The
    // failing path still says it, from the `catch` that knows.
    if (rawComments) {
      // The scan, with the reader's own rewrites over it. `refreshTargets` runs
      // after every state reply, which is what makes an edited comment reach
      // the panel and the popup at all --- the scan is a reading of the file on
      // disk and knows nothing of what has been typed since.
      const items = commentsIn(
        rawComments.items,
        pages,
        edits?.state.notes,
        edits?.state.discards,
      );
      viewer?.setComments(items);
      sidebar?.setComments({ ...rawComments, items });
      // After, because `setComments` is a rebuild and drops what the panel knew.
      // The words are about a comment rather than about a page order, so they
      // survive a page being deleted or moved --- and re-asking for them is not
      // an option, since the pages they came from are recorded as asked.
      if (commentWords.size > 0) sidebar?.setCommentWords(commentWords);
    }
    if (rawOutline) {
      sidebar?.setOutline({
        ...rawOutline,
        items: outlineIn(rawOutline.items, pages),
      });
    }
  }

  /**
   * Fills in the words each bare highlight covers, a page at a time.
   *
   * **Why it exists.** A reviewer's highlight with nothing typed on it is a
   * rectangle and no text, so the panel listed nine of them as nine rows all
   * reading "Highlight, no comment" --- a list that says a document was marked
   * up and not one word about what was marked. The words are in the page, under
   * the rectangle, and `wordsForPage` is what reads them out.
   *
   * **Why a page at a time, awaited.** Each page is a `page_text` extraction in
   * the backend, and the pool that answers it is the pool drawing tiles. Firing
   * them all at once puts a document's worth of extractions in front of the page
   * the reader is looking at; awaiting each means at most one is ever queued,
   * and the panel fills in from the front of the document while they scroll.
   *
   * **Why it can be called again.** {@link CommentList.setWords} merges, and
   * `asked` stops the same page being fetched twice, so a reader flipping to the
   * comments tab repeatedly costs one pass. `running` is what makes a second
   * call during the first a no-op rather than a second interleaved walk.
   *
   * The words are read against the page **as it is now** while the comments were
   * scanned when the document opened. That is the same footing the rectangles
   * are already on --- `applyPageOrder` re-slots a comment's page and leaves its
   * geometry alone --- so a page an edit has rotated moves its highlight and its
   * words together, or neither.
   */
  async function fillCommentWords(): Promise<void> {
    if (fillingWords) return;
    const source = rawComments;
    if (!source) return;
    fillingWords = true;
    // The walk waits for a page at a time, and the reader may change sides
    // while it does. Each round's reading and each round's writing runs as the
    // document the walk was started in, and so does lowering the flag.
    const id = openView;
    try {
      for (;;) {
        const round = asDocument(id, () => {
          // **Re-slotted every round, not once before the loop.** A page number
          // here is a slot, and a reader who deletes a page mid-walk renumbers
          // every slot after it --- so a list captured up front would read slot 4's
          // text and hand it to the comment that used to be there. Wrong words on
          // a real row, which is the failure that looks entirely plausible.
          const items = commentsIn(
            source.items,
            edits?.map ?? NO_PAGES,
            edits?.state.notes,
          );
          const page = pagesNeedingWords(items, wordsAsked)[0];
          if (page === undefined) return null;
          // The document that was open when this page was asked for. A second file
          // opened mid-walk replaces `rawComments`, and writing this one's
          // sentences onto its rows has the same shape as the slot problem above.
          if (rawComments !== source) return null;
          // The comments this round will answer, recorded before the await so a
          // page that cannot be read is not asked for again on the next edit.
          // **Their ids, not the page**: a page number here is a slot, and a
          // deletion renumbers every slot after it --- see `pagesNeedingWords`,
          // which now takes this set and states what went wrong when it held
          // slots.
          for (const comment of wantingWordsOn(items, page)) wordsAsked.add(comment.id);
          return { items, page, reader: viewer };
        });
        if (!round) return;
        const { items, page, reader } = round;
        const words = await wordsForPage(items, page, (at) =>
          reader ? reader.unturnedText(at) : Promise.resolve(null),
        );
        const kept = asDocument(id, () => {
          if (rawComments !== source) return false;
          if (words.size > 0) {
            for (const [comment, said] of words) commentWords.set(comment, said);
            sidebar?.setCommentWords(words);
          }
          return true;
        });
        if (!kept) return;
      }
    } finally {
      asDocument(id, () => { fillingWords = false; });
    }
  }

  /**
   * Fills in the words each pending region covers, a page at a time.
   *
   * **Why it exists.** `docs/PLAN.md` §6 step 2 is a review, and a review of six
   * red rectangles listed as *page 3, page 3, page 7* is not one. The words are
   * in the page under the rectangle, and `touchedText` is what reads them out.
   *
   * **Why a page at a time, awaited.** {@link fillCommentWords}'s reason, which
   * is the same reason: each page is a `page_text` extraction answered by the
   * pool that draws tiles, and firing them all at once puts a document's worth
   * of extractions in front of the page the reader is looking at.
   *
   * **Why every region on the page is answered at once.** The extraction is the
   * cost and it is per page; two regions on one page are two rectangles over one
   * `PageText`. Answering only the one that prompted the walk would read the
   * same page again for its neighbour.
   *
   * **Why a page that could not be read is still answered.** Otherwise the walk
   * asks for it again on the next edit, forever, and the row it belongs to says
   * *reading* for the rest of the session. Every region on it is recorded as
   * `null` instead, which is a state the row draws as what it is.
   *
   * **What has been answered is counted by region and not by page.** See
   * `nextUnreadRegion` in `redactlist.ts`: a page set is the obvious bookkeeping here, since
   * the extraction is per page, and it is wrong in one direction --- a region
   * drawn on a page already read is never selected, so its row says *reading*
   * for the session and no plan is ever computed for it.
   *
   * **Why the walk re-reads the slot after every wait.** See
   * `fillRedactionRegions`: a page deleted above this one mid-walk turned the
   * slot into a different file page, and the plan was stored for the wrong page.
   */
  async function fillRedactionWords(): Promise<void> {
    if (fillingRedactionWords) return;
    const model = edits;
    if (!model || !viewer) return;
    fillingRedactionWords = true;
    // Held for the walk, as `fillCommentWords` holds its own: the variables
    // are another document's once the reader changes sides.
    const id = openView, reader = viewer, panel = sidebar;
    const words = redactionWords, planned = redactionPlans;
    try {
      // The walk is `redactlist.ts`, where a test can reach it; what stays here
      // is the wiring. Every lookup reads `model.map` at the moment it is asked,
      // because an edit replaces the page order in place during a wait.
      await fillRedactionRegions({
        // The model that was open when this walk started. A second document
        // replaces `edits` mid-walk, and writing this one's words onto its rows
        // is the same failure `fillCommentWords` guards against.
        current: () => asDocument(id, () => edits === model) === true,
        regions: () => model.state.redactions,
        slotOf: (page) => model.map.slotOfId(page),
        sourceOf: (slot) => model.map.sourceOf(slot),
        text: async (slot) => (await reader.unturnedText(slot)) ?? null,
        plans: (page, regions) => call("redaction_plans", { doc: model.doc, page, regions }),
        words,
        planned,
        answered: () => panel?.setRedactionWords(),
        // Not raised to the reader. The rows keep saying what they said,
        // which is nothing about what a removal would take --- and the
        // command that actually redacts asks again and reports its own
        // failures, so a reader is never left acting on this silence.
        failed: (e) => console.warn(`could not read what a removal would take: ${e}`),
      });
    } finally {
      asDocument(id, () => { fillingRedactionWords = false; });
    }
  }

  /**
   * Opens `path` again in the place of a document a write has just closed,
   * with the reader put back at `place`.
   *
   * A save in place and a redaction in place close the document as part of
   * the write, whether the write went through or was refused past the point
   * of return. The handle is forgotten first, so that the open does not try
   * to release one the backend already released. One function, because the
   * two lines were written out at each of the four places that reopen.
   */
  async function reopenAfterWrite(path: string, place: Place | null): Promise<void> {
    openDoc = -1; openView = NO_VIEW;
    await openPath(path, false, place);
  }

  /**
   * Writes the working document over the file the reader opened, and reopens it.
   *
   * **Four steps, and the first two are why this is not one `await`.** The note
   * a reader is typing commits when its box closes, so the box is closed first;
   * the edit that closing it journals may still be in flight, so
   * {@link pendingEdit} is waited for. Only then does the model hold what the
   * reader is looking at. Skipping either leaves a highlight in the file with an
   * empty note.
   *
   * **The reopen is the rebase.** `save_document` closes the document as part of
   * the save --- `docs/PLAN.md` §5 --- so there is nothing left to keep: every
   * object identity in the file has changed and the journal is spent. Opening
   * the path again is what gives the reader a document, and `openDoc` is cleared
   * first so the open does not try to release a handle the save already
   * released. {@link reopenAfterWrite} is both lines.
   *
   * **The place is expressed in slots**, which is the one argument here that
   * could quietly be wrong --- see {@link currentPlace}. Captured before the
   * save rather than after, because the viewer is torn down by the reopen.
   *
   * A failure that says `reopen` is one the document did not survive; anything
   * else left the reader exactly as they were, and is only a message.
   */
  async function saveDocument(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || !viewer) return;
      await settleDrafts();
      const path = openPathName;
      const place = currentPlace(false);
      try {
        await edits.save(path);
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        // Read through the seam rather than cast here, so that what a rejection
        // is understood to carry has one definition. Three catches wrote this by
        // hand and the fourth -- printing -- wrote none of it.
        const failure = refusalOf(e);
        // The message and the buttons come from one call, so they cannot disagree
        // about what happened. A refusal that names Save a copy now arrives with
        // Save a copy beside it -- which it did not until 2026-08-19, and worse,
        // Save a copy was refused by the same guard, so the advice named a door
        // that was locked.
        const prompt = afterRefusal(failure);
        if (!failure.reopen) {
          say(prompt.message, prompt.offers);
          return;
        }
        // The document is closed and the file is the one it always was. Reopening
        // is what gives the reader something to look at; their unsaved commands
        // are gone with the model, which is what the message says.
        await reopenAfterWrite(path, place);
        // **After the reopen, and that ordering is the whole of it.** `openPath`
        // clears the message area on its way in, so saying this before the reopen
        // showed it for zero frames: the one refusal a reader can do nothing about
        // -- their document closed, their edits spent -- was the one they were
        // never told about. It has been that way since `save_document` landed, and
        // the fingerprint work is what made the path reachable often enough to
        // notice.
        //
        // Only when the reopen had nothing of its own to report. A file that also
        // failed to reopen is the more urgent fact, and it is already on screen.
        if (!error) say(prompt.message, prompt.offers);
        return;
      }
      await reopenAfterWrite(path, place);
    });
  }

  /**
   * Asks for a name and writes the working document to it.
   *
   * A copy, never the open file. `save.rs` refuses the source path outright, so
   * a reader who types the open document's own name is told rather than left
   * with a file whose baseline no longer matches the journal replaying against
   * it --- see `docs/PLAN.md` §5 on saving in place.
   */
  async function saveCopy(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName) return;
      const suggested = basename(openPathName).replace(/\.pdf$/i, "");
      try {
        const chosen = await saveDialog({
          title: "Save a copy",
          defaultPath: `${suggested} copy.pdf`,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      // Cancelled. Deliberately not an error and deliberately not a message:
      // the reader closed the panel, which is an answer.
      if (!chosen) return;
      // Success is silent, which is deliberate and is the same answer Preview
      // gives: the panel closing and the file appearing where the reader put it
      // is the acknowledgement, and a banner over the page they are reading is
      // not. A failure is not silent --- `save.rs` refuses an encrypted
      // document, a file that changed under the open one and a write over the
      // source, and each of those is something the reader has to act on.
      // Not silent when the source had changed. The file is written and is the
      // best tpdf can produce, and which document it was built from is the one
      // thing a reader cannot be left to discover for themselves.
      const said = afterCopy(await edits.saveCopy(openPathName, chosen));
      if (said) say(said);
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      say(String(e));
    }
    });
  }

  /**
   * Asks for a name and writes a redacted copy of the document to it.
   *
   * `saveCopy`'s shape and one deliberate difference: **success is not silent**.
   * A copy that worked says so by appearing where the reader put it; a redaction
   * has destroyed content on the strength of a claim, and `docs/PLAN.md` §6 step
   * 4 says the claim is reported either way. `afterRedaction` is the sentence.
   *
   * The open document is untouched --- the regions stay pending and nothing is
   * journalled --- so a reader who does not like the result still has their
   * marks and can try again somewhere else.
   */
  async function redactCopy(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName) return;
      const suggested = basename(openPathName).replace(/\.pdf$/i, "");
      try {
        const chosen = await saveDialog({
          title: "Redact and save as",
          defaultPath: `${suggested} redacted.pdf`,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      // Cancelled, which is an answer rather than an error.
      if (!chosen) return;
      const result = afterRedactionCopy(await edits.redactCopy(openPathName, chosen, redactionFill));
      say(result.message, result.offers);
      redactedCopyPath = chosen;
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      // Every refusal reaches here, and the one worth the room is the region
      // that covers something a removal cannot take: `lib.rs` refuses before
      // writing anything and names what and where.
      say(String(e));
    }
    });
  }

  /**
   * Writes the marked regions into a fresh image-only PDF.
   *
   * The confirmation is mandatory because the output discards every interactive
   * document feature even though the original stays untouched. Once confirmed,
   * the task blocks document commands: its worker reads the current model while
   * it renders every page, so another edit or open cannot be allowed to change
   * what that model means halfway through the copy.
   */
  async function redactRasterCopy(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || !viewer || copyTaskBusy) return;
      // Closing the note journals its last text synchronously. Do this before the
      // busy guard starts refusing new edits, then wait for that queued edit below.
      const settled = settleDrafts();
      copyTaskBusy = true;
      refreshMenu();
      try {
        await settled;
        const proceed = await confirmDialog(
          "Creates an image-only PDF. Text will no longer be selectable; links, forms and signatures will not remain interactive/valid. Original unchanged.",
          {
            title: "Redact to image-only copy",
            kind: "warning",
            okLabel: "Create image-only copy",
            cancelLabel: "Cancel",
          },
        );
        if (!proceed || !edits || !openPathName) return;
        const source = openPathName;
        const suggested = basename(source).replace(/\.pdf$/i, "");
        const chosen = await saveDialog({
          title: "Redact to image-only copy",
          defaultPath: `${suggested} redacted image-only.pdf`,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      if (!chosen) return;

      blockingTask = "Creating image-only copy...";
      // Give the progress line one render before the long native call occupies
      // this command. Without the tick, a fast state change followed by IPC can
      // leave the only visible acknowledgement until after the file exists.
      await tick();
      say(
        afterRasterRedaction(
          await edits.redactRasterCopy(source, chosen, redactionFill),
          basename(chosen),
        ),
      );
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      say(`Image-only redaction failed. No verified copy was saved. ${String(e)}`);
    } finally {
      blockingTask = null;
      copyTaskBusy = false;
      refreshMenu();
    }
    });
  }

  /**
   * Offers the ways to make a copy smaller, and writes the one chosen.
   *
   * `saveCopy`'s shape. The dialog comes before the name, as the password
   * does in `protectCopy`, and it is handed the estimate to ask: each choice
   * is shown with the size it comes to, and saving is offered only for one
   * that is smaller. The copy is not opened; the sentence says its size.
   */
  async function compressCopy(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || copyTaskBusy) return;
      const settled = settleDrafts();
      copyTaskBusy = true;
      refreshMenu();
      try {
        await settled;
        if (!edits || !openPathName) return;
        const source = openPathName;
        const working = edits;
        // The choices are shown with what each comes to, so the dialog is
        // handed the question and asks it itself.
        const chosen = await compressDialog?.ask(basename(source), (pictures) =>
          working.compressEstimate(source, pictures),
        ) ?? null;
        if (!chosen || !edits) return;
        const suggested = smallerName(source);
        const panel = () =>
          saveDialog({
            title: "Save a smaller copy",
            defaultPath: suggested,
            filters: [{ name: "PDF", extensions: ["pdf"] }],
          });
        // The checks build answers the panel; see `saveanswer.ts`.
        const path = __TPDF_CHECKS__ && signSaves
          ? await signSaves.ask(suggested, panel)
          : await panel();
        if (!path || !edits) return;
        blockingTask = "Saving the smaller copy...";
        await tick();
        say(afterCompress(
          await edits.compressCopy(source, path, chosen.pictures),
          path,
          chosen.shrinkage,
        ));
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        say(String(e));
      } finally {
        blockingTask = null;
        copyTaskBusy = false;
        refreshMenu();
      }
    });
  }

  /**
   * Writes a copy that needs a new password, or one that needs none.
   *
   * `saveCopy`'s shape, with the task blocking document commands while the
   * copy is written. The password is asked for before the name, so a reader
   * who dismisses the first dialog is not shown the second. The copy is not
   * opened: the window keeps the document the reader has, and the sentence
   * says what the file on disk now needs.
   */
  async function protectCopy(set: boolean): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || copyTaskBusy) return;
      const settled = settleDrafts();
      copyTaskBusy = true;
      refreshMenu();
      try {
        await settled;
        if (!edits || !openPathName) return;
        const source = openPathName;
        const password = set
          ? await newPasswordDialog?.ask(basename(source)) ?? null
          : null;
        if (set && password === null) return;
        const suggested = protectedName(source, set);
        const panel = () =>
          saveDialog({
            title: set ? "Save a copy with a password" : "Save a copy without its password",
            defaultPath: suggested,
            filters: [{ name: "PDF", extensions: ["pdf"] }],
          });
        // The checks build answers the panel; see `saveanswer.ts`.
        const chosen = __TPDF_CHECKS__ && signSaves
          ? await signSaves.ask(suggested, panel)
          : await panel();
        if (!chosen || !edits) return;
        blockingTask = "Saving the copy...";
        await tick();
        say(afterProtect(await edits.protectCopy(source, chosen, password), chosen, set));
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        say(String(e));
      } finally {
        blockingTask = null;
        copyTaskBusy = false;
        refreshMenu();
      }
    });
  }

  /**
   * Writes a copy in which the scanned pages can be searched, and opens it.
   *
   * `redactRasterCopy`'s shape: the task blocks document commands, because the
   * backend reads the pages of the open document while it runs. Two things
   * differ. The copy is opened when it is written, since searching it is what
   * the reader asked for; and the open happens after the task has ended,
   * because `openPath` waits for document tasks to be idle and this is one.
   *
   * A document with unsaved changes is told to save first, before a name is
   * asked for. The backend refuses it too (`UNSAVED` in `commands/ocr.rs`); the
   * check here only spares the reader a dialog whose answer would be thrown away.
   */
  async function recogniseText(): Promise<void> {
    if (opening) return;
    const done: { path?: string; said?: string } = {};
    await documentTasks.run(async () => {
      if (!edits || !openPathName || copyTaskBusy) return;
      const settled = settleDrafts();
      copyTaskBusy = true;
      refreshMenu();
      try {
        await settled;
        if (!edits || !openPathName) return;
        if (edits.dirty) {
          say(SAVE_FIRST);
          return;
        }
        const source = openPathName;
        const suggested = suggestedName(source);
        const panel = () =>
          saveDialog({
            title: "Recognise text and save as",
            defaultPath: suggested,
            filters: [{ name: "PDF", extensions: ["pdf"] }],
          });
        // The checks build answers the panel, as it does for a signing: no
        // phase can drive a native one. See `saveanswer.ts`.
        const chosen = __TPDF_CHECKS__ && signSaves
          ? await signSaves.ask(suggested, panel)
          : await panel();
        if (!chosen) return;
        // Numbered before Stop is shown, so a press at any moment names this run.
        recognitionRun += 1;
        blockingTask = STARTING;
        recognising = true;
        await tick();
        done.said = afterRecognition(
          await edits.ocrCopy(source, chosen, recognitionRun, recognitionLanguage.language),
          basename(chosen),
        );
        done.path = chosen;
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        say(String(e));
      } finally {
        recognising = false;
        blockingTask = null;
        copyTaskBusy = false;
        refreshMenu();
      }
    });
    await openWritten(done);
  }

  /**
   * Opens the file a task wrote, and says what the task has to say about it.
   *
   * Said twice on purpose. The first is for a file that then fails to open:
   * it is on disk, and the reader has to be told so on the tab they are on.
   * The second is for the document once it is open, since opening one clears
   * the message area; it is said in that document, which is the one the
   * reader is left in.
   */
  async function openWritten(done: { path?: string; said?: string }): Promise<void> {
    const { path, said } = done;
    if (!path || !said) return;
    say(said);
    await openPath(path);
    const opened = tabs.forPath(path, tabs.active);
    if (opened) asDocument(opened.view, () => say(said));
  }

  /**
   * Compares the saved file's text with its pages and lists what is not shown.
   *
   * `recogniseText`'s shape without a file to write: the task blocks document
   * commands, because the backend reads the pages of the open document while
   * it runs, and it reports its pages and can be stopped. The answer goes to
   * the sidebar's tab, which is opened. A stopped or failed check leaves
   * whatever the tab showed before: it has no result of its own.
   *
   * Unsaved changes do not refuse it. The file is what is checked, and the
   * result says so (`UNSAVED` in `hiddentext.ts`).
   */
  async function findHiddenText(): Promise<void> {
    if (opening) return;
    await documentTasks.run(async () => {
      if (!edits || copyTaskBusy) return;
      const settled = settleDrafts();
      copyTaskBusy = true;
      refreshMenu();
      try {
        await settled;
        if (!edits) return;
        const asked = edits;
        // The order, the run's number and the dropped answer are `Runs.check`'s.
        await hiddenRuns.check({
          begun: async () => {
            blockingTask = HIDDEN_STARTING;
            findingHidden = true;
            await tick();
          },
          ask: (run) => call("hidden_text", { doc: asked.doc, run }),
          // The document the answer is about, not whichever is open when it lands.
          current: () => edits === asked,
          show: (checked) => {
            sidebar?.setHiddenText(checked);
            // The ring over a passage of the result this one replaces.
            viewer?.clearRegion();
            showTab("hidden");
          },
        });
      } catch (e) {
        say(String(e));
      } finally {
        findingHidden = false;
        blockingTask = null;
        copyTaskBusy = false;
        refreshMenu();
      }
    });
  }

  /**
   * Goes to a passage the check found and rings it.
   *
   * Focus stays in the panel, for the results list's reason: a reader working
   * down this list is comparing passages. A passage outside the page has no
   * place on it to ring, so its page is what is shown.
   */
  function showHidden(passage: Passage): void {
    const model = edits, shown = viewer;
    if (!model || !shown) return;
    showPassage(passage, (page) => model.map.slotOf(page), {
      ring: (slot, rect) => shown.showRegion(slot, rect),
      // No ring left from the row before.
      page: (slot) => { shown.clearRegion(); shown.goToDestination(slot, null); },
      gone: () => say(PAGE_GONE),
    });
  }

  /**
   * Asks which language text is recognised in.
   *
   * The list is the machine's and is asked for first, so the palette opens
   * with it in hand; `ocrlanguage.ts` holds it, decides what an answer means
   * and has every sentence. A list that cannot be fetched is said and nothing
   * is asked.
   */
  async function chooseRecognitionLanguage(): Promise<void> {
    await recognitionLanguage.choose(
      () => call("ocr_languages"),
      () => palette?.askFor("file.recogniseTextLanguage.choice"),
      say,
    );
  }

  /**
   * Takes the palette's answer, says what was chosen and has the session
   * remember it. A failed write is not said, for `toggleInvert`'s reason: the
   * choice holds for this launch either way.
   */
  function setRecognitionLanguage(raw: string): void {
    const said = recognitionLanguage.answer(raw);
    if (said === null) return;
    // A notice and not `say`: that line is the failure's, and is set in red.
    notice = said;
    void call("session_set_ocr_language", { language: recognitionLanguage.language }).catch(
      () => {},
    );
  }

  /**
   * Removes every marked region from the file the reader opened, and reopens it.
   *
   * {@link redactCopy} and {@link saveDocument} joined, and it inherits a
   * precondition from each. From the save: the note a reader is typing commits
   * when its box closes, so the box is closed and the edit it journals is waited
   * for --- otherwise a highlight lands in the file with an empty note. From the
   * redaction: nothing is silent, ever, because §6 step 4 says the claim is
   * reported either way.
   *
   * **The warning comes first and the second press is what confirms.** There is
   * no undo across this and no original afterwards, which is more than Reload
   * spends and Reload already asks. `beforeRedactingInPlace` is the sentence and
   * carries Save a copy beside it, which is the only way left to keep an
   * unredacted one.
   */
  function redactDocument(): void {
    if (!edits || !openPathName || !viewer) return;
    const prompt = beforeRedactingInPlace(basename(openPathName));
    say(prompt.message, prompt.offers);
  }

  /**
   * Redacts the open file, warning already given.
   *
   * {@link reloadAnyway}'s shape and its reason: a confirmation that re-enters
   * the guard that produced it is a loop.
   *
   * **The reopen is the rebase**, exactly as {@link saveDocument} describes it,
   * and here it is also `docs/PLAN.md` §6's truncation --- the journal is spent,
   * so the regions that were pending are gone along with every command before
   * them, and there is no undo that reaches back across the removal.
   *
   * The report is said **after** the reopen, for the reason `saveDocument`
   * states about its own message: `openPath` clears the message area on the way
   * in, so a verdict said before it would show for zero frames --- and this
   * verdict is the one thing a reader must not miss, because content is gone on
   * the strength of it.
   */
  async function redactAnyway(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || !viewer) return;
      await settleDrafts();
      const path = openPathName;
      const place = currentPlace(false);
      say(null);
      let said: string;
      try {
        said = afterRedaction(await edits.redactDocument(path, redactionFill));
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        const failure = refusalOf(e);
        const prompt = afterRefusal(failure);
        // Nothing happened: the file is the file and the reader still has their
        // document and their marks. A message is all there is to do.
        if (!failure.reopen) {
          say(prompt.message, prompt.offers);
          return;
        }
        await reopenAfterWrite(path, place);
        if (!error) say(prompt.message, prompt.offers);
        return;
      }
      await reopenAfterWrite(path, place);
      if (!error) say(said);
    });
  }

  /**
   * Writes the pages a reader named to a second file.
   *
   * `saveCopy` with a selection, and deliberately the same shape: the same
   * dialog, the same silence on success, the same single `error` on failure.
   * A reader who has used one has used the other.
   *
   * The suggested name says which pages, because the one thing a reader cannot
   * tell from a file called "report copy.pdf" is which three pages of the
   * report are in it. Ranges are collapsed back for the name --- `1-3` rather
   * than `1,2,3` --- since that is what they typed and a name is not a place
   * to expand a selection.
   */
  async function extractPages(slots: number[]): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || slots.length === 0) return;
      const suggested = basename(openPathName).replace(/\.pdf$/i, "");
      try {
        const chosen = await saveDialog({
          title: "Extract pages",
          defaultPath: `${suggested} ${namePages(slots)}.pdf`,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      if (!chosen) return;
      // The same report `saveCopy` gives, and it was missing until 2026-08-24
      // while `lib.rs`'s comment on `extract_pages` said "the reader is told the
      // same way". An extract from a file that changed underneath is built from
      // the newer version exactly as a copy is; saying nothing left that to be
      // discovered.
      const said = afterCopy(
        await edits.extractPages(openPathName, chosen, slots),
      );
      if (said) say(said);
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      say(String(e));
    }
    });
  }

  /**
   * Writes the document to several files, one per group of pages.
   *
   * `extractPages`' shape, with one difference that decides the dialog: the
   * reader picks a **stem** and gets numbered siblings, because `split_paths`
   * derives `name-1.pdf`, `name-2.pdf` and never writes the chosen name itself.
   * So the default offered here has no page numbers in it, where an extract's
   * carries `namePages` --- naming a range in a stem would put it in every part.
   *
   * **The report is not optional**, unlike an extract's. `afterSplit` always
   * says something, because the file the reader named is not one of the files
   * that appeared, and silence would send them looking for it.
   *
   * Nothing happens to the open document: no `applyEdit`, no state to adopt.
   */
  async function splitDocument(groups: number[][]): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || groups.length < 2) return;
      const suggested = basename(openPathName).replace(/\.pdf$/i, "");
      try {
        const chosen = await saveDialog({
          title: "Split document",
          defaultPath: `${suggested}.pdf`,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      if (!chosen) return;
      say(afterSplit(await edits.splitDocument(openPathName, chosen, groups)));
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      say(String(e));
    }
    });
  }

  /**
   * Combines this document with others into a new file.
   *
   * Two dialogs, in this order: what to merge in, then where to write it. The
   * order is the reader's sentence rather than a convenience --- the default
   * name offered by the second one could depend on what was picked in the first,
   * and asking for a destination before knowing what goes in it is a question
   * out of order.
   *
   * **Nothing happens to the open document**, which is why there is no
   * `applyEdit` here and no state to adopt. It is `extractPages`' shape: a read
   * of the working document, producing a file somewhere else.
   *
   * A cancelled dialog returns without a word, at either step. `openDialog` with
   * `multiple` answers an array, a bare string or `null` depending on the
   * platform and on what was chosen, so all three are handled rather than the
   * one this machine happens to give.
   */
  async function mergeDocuments(): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName) return;
      try {
        const picked = await openDialog({
          multiple: true,
          directory: false,
          title: "Choose documents to merge into this one",
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
      const others =
        typeof picked === "string" ? [picked] : (picked ?? []);
      if (others.length === 0) return;
      const suggested = basename(openPathName).replace(/\.pdf$/i, "");
      const chosen = await saveDialog({
        title: "Save the merged document",
        defaultPath: `${suggested} merged.pdf`,
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (!chosen) return;
      say(afterMerge(await edits.mergeDocuments(openPathName, chosen, others)));
    } catch (e) {
        if (e instanceof SaveCancelled) return;
      say(String(e));
    }
    });
  }

  /**
   * Makes a document from pictures the reader chooses, one page each, and
   * opens it.
   *
   * Needs no open document. The pictures are chosen in the platform's panel
   * and go in in the order it returns them; the name is asked for next; the
   * document is opened after the task has ended, as a recognised copy is.
   */
  async function fromPictures(): Promise<void> {
    if (opening) return;
    const done: { path?: string; said?: string } = {};
    await documentTasks.run(async () => {
      if (copyTaskBusy) return;
      copyTaskBusy = true;
      refreshMenu();
      try {
        const queued = __TPDF_CHECKS__ ? queuedPictures : null;
        queuedPictures = null;
        const picked = queued ?? await openDialog({
          multiple: true,
          directory: false,
          title: "Choose pictures, one for each page",
          filters: [{ name: "Pictures", extensions: PICTURE_EXTENSIONS }],
        });
        const images = typeof picked === "string" ? [picked] : (picked ?? []);
        const first = images[0];
        if (first === undefined) return;
        const suggested = pictureName(first);
        const panel = () =>
          saveDialog({
            title: "Save the new document",
            defaultPath: suggested,
            filters: [{ name: "PDF", extensions: ["pdf"] }],
          });
        const chosen = __TPDF_CHECKS__ && signSaves
          ? await signSaves.ask(suggested, panel)
          : await panel();
        if (!chosen) return;
        blockingTask = "Making the document...";
        await tick();
        done.said = afterPictures(await call("images_to_pdf", { images, path: chosen }), chosen);
        done.path = chosen;
      } catch (e) {
        if (e instanceof SaveCancelled) return;
        say(String(e));
      } finally {
        blockingTask = null;
        copyTaskBusy = false;
        refreshMenu();
      }
    });
    await openWritten(done);
  }

  /**
   * Signs the document with a certificate the reader already has, into a new
   * file. The sequence, its refusals and every sentence are `signing.ts`'s; this
   * supplies the chooser, the save panel, the command and the message area.
   */
  async function signDocument(field: SignTarget | null = null): Promise<void> {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!edits || !openPathName || openDoc < 0) return;
      const doc = openDoc;
      const source = openPathName;
      try {
        const said = await signing.signDocument({
          // Inside the task and before anything of this flow's own is raised,
          // which is where the six other flows that write call it and where
          // `applyEdit` still takes what it commits.
          settle: settleDrafts,
          dirty: () => dirty,
          openPath: source,
          list: () => call("sign_identities"),
          choose: (choices, into) => signing.askIdentity(choices, undefined, into),
          savedImage: () => loadSignature(),
          // The preview is drawn in this document's worker by the signing's
          // own code; the drawing dialog is Phase 4's, named for this use.
          appearance: (identity, saved) =>
            askAppearance({
              saved,
              preview: (image, options) =>
                call("sign_preview", { doc, identity, size: PREVIEW_SIZE, image, options }),
              draw: () => signatureDialog?.ask(APPEARANCE_WORDS) ?? Promise.resolve(null),
            }),
          // The viewer that is showing the document now: a tab switch while
          // this waits destroys it, which answers `null` and ends the signing.
          place: async () => {
            if (!viewer) return null;
            say(signing.PLACE);
            return await viewer.armPlacement();
          },
          saveAs: async (suggested) => {
            const panel = () =>
              saveDialog({
                title: "Save the signed document",
                defaultPath: suggested,
                filters: [{ name: "PDF", extensions: ["pdf"] }],
              });
            return __TPDF_CHECKS__ && signSaves
              ? await signSaves.ask(suggested, panel)
              : await panel();
          },
          sign: (identity, path, placement, timestamp, longTerm, into) =>
            call("sign_document", { doc, source, identity, path, placement, timestamp, longTerm, field: into }),
          // A timestamp or long-term data that did not come: the question, then
          // the signature the backend is holding written or dropped. The OS is
          // not asked again.
          stampFailed: (why) => signing.askAfterStampFailed(why),
          longTermFailed: (why) => signing.askAfterLongTermFailed(why),
          resume: (pending, timestamp, longTerm) =>
            call("sign_resume", { pending, timestamp, longTerm }),
          discard: (pending) => call("sign_discard", { pending }),
        }, field);
        if (said) say(said);
      } catch (e) {
        say(String(e));
      }
    });
  }

  /**
   * Shows what the document says about itself.
   *
   * Opens first and fills in second, deliberately. The `lopdf` parse behind this
   * is the one nothing on the reading path ever needs, so it has not run when
   * the dialog is asked for --- on the 337 MB fixture that is around twelve
   * milliseconds and on an ordinary document a fraction of one, but a command
   * that appears to do nothing for even a moment reads as broken. The dialog
   * says it is reading, and replaces that with the answer.
   *
   * The `openDoc` guard is the one every document-level fetch here carries: a
   * reader who closes one file and opens another before the answer lands must
   * not be shown the first file's properties under the second file's name.
   */
  async function showProperties(): Promise<void> {
    if (!propertiesDialog || openDoc < 0) return;
    if (properties) {
      propertiesDialog.show(properties, "");
      return;
    }

    const wanted = openDoc, view = openView, model = edits;
    propertiesDialog.show(null, "");
    try {
      const answer = await call("document_properties", { doc: wanted });
      if (!stillIn(view, model) || !propertiesDialog.isOpen) return;
      asDocument(view, () => { properties = answer; });
      propertiesDialog.show(answer, "");
    } catch (e) {
      if (!stillIn(view, model) || !propertiesDialog.isOpen) return;
      // Shown in the dialog rather than in the status line, because the dialog
      // is what the reader is looking at and an empty one beside a message
      // somewhere else reads as a document that states nothing.
      propertiesDialog.show(null, String(e));
    }
  }

  /**
   * The updater, and the one place this application uses the network.
   *
   * The Tauri plugin is reached through a thin adapter rather than imported by
   * `update.ts`, so the state machine stays testable outside a webview --- the
   * plugin answers only inside one. See `update.ts` and `docs/THREAT-MODEL.md`
   * §T9.
   */
  let updateState = $state<UpdateState>({ kind: "idle" });

  /**
   * The running version, read once from the backend at boot.
   *
   * Empty until that lands, which is why every reader of it below tolerates the
   * empty string rather than asserting. It is one `invoke` during setup and
   * nothing waits on it.
   */
  let appVersion = $state("");

  /**
   * The answer to a question the reader asked, or null.
   *
   * Distinct from `status`, which reports what the document is doing and is
   * present the whole time a document is open. This is set only by a command --
   * "About tpdf", "Check for updates" -- and cleared when a document opens, so
   * it is never something that arrived on its own. See `updateNotice`.
   */
  let notice = $state<string | null>(null);

  /**
   * Puts the answer to something the reader asked in front of them, where
   * they are when it arrives: the document they are working in by then, or
   * the window with none. {@link say}'s counterpart for what is not a failure.
   * For an answer that comes back after a wait and is about the window, a
   * preference saved or a list cleared; one about a document is put in that
   * document, through `asDocument`.
   */
  function tell(text: string | null): void {
    notice = text;
  }
  const updates = new Updates(
    {
      check: async () => {
        const { check } = await import("@tauri-apps/plugin-updater");
        const found = await check();
        if (!found) return null;
        return {
          version: found.version,
          downloadAndInstall: (onEvent) => found.downloadAndInstall(onEvent),
        };
      },
    },
    (s) => {
      updateState = s;
      // "Install update and restart" is withheld until there is one and
      // withdrawn once it is applied, so its menu item moves with this.
      refreshMenu();
      // Relaunching is the shell's job rather than the state machine's: it ends
      // the process, which is not something a module with unit tests should be
      // able to do. Deferred to the reader's next launch instead of forced ---
      // see `update.ts` on why nothing here swaps the binary under an open
      // document.
    },
  );

  // Which of the command-line tool's two commands has something to do. The
  // backend is asked; this holds the answer and greys by it, and the menu is
  // re-read each time the answer moves.
  const commandLineTool = new CommandLineTool({
    read: () => call("command_line_tool_state"),
    apply: (install) => call("command_line_tool", { install }),
    say: (text) => (notice = text),
    changed: () => refreshMenu(),
  });

  const commands = new CommandRegistry();
  registerAppCommands(commands, appActions);
  let toolState = $state(toolbarState(commands));
  let toolStateKey = "";

  function runToolbarCommand(id: string): void {
    // Focus before dispatch: a command opening a dialog must keep its focus.
    viewer?.focus();
    runMenuCommand(commands, palette, id);
    refreshMenu();
  }

  function finishToolbarDrawing(): void {
    viewer?.finishDrawing();
    viewer?.focus();
  }

  function cancelToolbarTool(): void {
    viewer?.cancelDraw();
    viewer?.focus();
  }

  /**
   * The right-click menu, or null before the shell is built.
   *
   * One instance for every surface --- see `contextmenu.ts`. It lives on
   * `document.body` rather than inside the panel it was opened from, so a menu
   * opened on the last row of the strip is not clipped by the panel's scroll
   * box.
   */
  let contextMenu: ContextMenu | null = null;

  /**
   * Shows the right-click menu, if any of its commands can run.
   *
   * Returns nothing and swallows the empty case deliberately: a menu with no
   * entries is not opened, and no menu appearing is the correct answer to a
   * right-click on something with nothing to offer.
   */
  function openContextMenu(entries: string[], at: { x: number; y: number }) {
    contextMenu?.show(entries, at);
  }

  function tabContextMenu(event: MouseEvent, id: ViewId): void {
    event.preventDefault();
    event.stopPropagation();
    const tab = tabs.find(id);
    if (!tab) return;
    const across = otherSide(panes.sideOf(id));
    contextMenu?.show(["tab.reveal", "tab.copyPath", "tab.copyName", "---", "tab.moveStart", "tab.moveEnd", "tab.moveSide", "---", "tab.close", "file.closeAll"], { x: event.clientX, y: event.clientY }, [{
      id: "tab.moveStart", title: "Move to start",
      enabled: () => tabs.find(id) === tab && tabs.all.length > 1,
      run: () => { tabs.moveTo(id, "start"); refreshTabs(); },
    }, {
      id: "tab.moveEnd", title: "Move to end",
      enabled: () => tabs.find(id) === tab && tabs.all.length > 1,
      run: () => { tabs.moveTo(id, "end"); refreshTabs(); },
    }, {
      // Named for where the tab goes. With one side, the right one is made.
      id: "tab.moveSide", title: `Move to ${across} side`,
      enabled: () => tabs.find(id) === tab && tabs.all.length > 1 && !opening && !documentBusy,
      run: () => moveTab(id, across),
    }, {
      id: "tab.reveal",
      title: isMac() ? "Show in Finder" : "Show in Explorer",
      enabled: () => tabs.find(id) === tab,
      run: async () => {
        try { await call("reveal_file", { path: tab.path }); }
        catch (error) { say(String(error)); }
      },
    }, {
      id: "tab.copyPath", title: "Copy file path",
      enabled: () => tabs.find(id) === tab,
      run: async () => {
        try { await navigator.clipboard.writeText(tab.path); }
        catch (error) { say(`Could not copy the file path: ${String(error)}`); }
      },
    }, {
      id: "tab.copyName", title: "Copy file name",
      enabled: () => tabs.find(id) === tab,
      run: async () => {
        try { await navigator.clipboard.writeText(basename(tab.path)); }
        catch (error) { say(`Could not copy the file name: ${String(error)}`); }
      },
    }, {
      id: "tab.close", title: "Close",
      enabled: () => tabs.find(id) === tab && !opening && !documentBusy,
      run: () => closeTab(id),
    }]);
  }

  /**
   * Whether a native menu bar exists to keep up to date.
   *
   * False on every platform but macOS, and false before the install has
   * answered. Read rather than a platform test of our own: the answer comes
   * from the side that actually builds the menu, so there is one statement of
   * where a menu bar belongs rather than two that can disagree.
   */
  let menuInstalled = false;

  /**
   * The enablement last pushed to the native menu, as its own JSON.
   *
   * Not a rune: nothing renders from it. It exists so that `refreshMenu` can be
   * called from the frame loop without sending a message per frame --- see
   * there for why it has to be.
   */
  let menuPushed = "";

  /**
   * Builds the menu once the commands are registered.
   *
   * Called after the spike entry points have returned --- every one of them
   * exits the process --- so no check or benchmark run ever installs a menu.
   */
  async function installMenu(): Promise<void> {
    try {
      // The event name comes back with the answer rather than being written
      // here as well as in Rust --- see `set_menu`. A menu that is built,
      // enabled and inert is what a drifted constant would look like.
      const event = await call("set_menu", {
        sections: buildMenu(commands),
      });
      if (!event) return;
      menuInstalled = true;
      await listen<string>(event, (chosen) => {
        runMenuCommand(commands, palette, chosen.payload);
      });
    } catch (e) {
      // Shown, not swallowed. A menu that failed to build is invisible by
      // definition: the bar simply keeps the platform's default, which is
      // exactly what it looked like before any of this existed.
      say(String(e));
    }
  }

  /**
   * Pushes each command's `enabled` guard into its menu item.
   *
   * Called from an edit, an update-state transition, the end of an open **and
   * the frame loop** --- rather than from an effect, because the guards read
   * `viewer` and `edits`, which are plain variables rather than runes and so are
   * not tracked.
   *
   * The frame loop is the correction, and the reasoning it replaces was half
   * right. This used to be called from the first three alone, on the argument
   * that a missed call leaves an item *live* that the palette would withhold ---
   * refused by `runMenuCommand`, so the cost is a stale grey. The direction that
   * actually bit is the other one, and a stale grey is not a cosmetic cost: it
   * is a route the reader cannot take. `edit.highlightSelection`'s guard reads
   * the selection, which moves through none of those three, so the menu bar
   * offered it greyed at exactly the moment there was something to highlight.
   * `docs/TRAPS.md` has the entry; the shape is that a *pushed* enablement is a
   * cache, and every guard reading state that changes outside the push sites is
   * wrong between them.
   */
  function refreshMenu(): void {
    // The menu bar and the toolbar describe the document the reader is working
    // in. Asked from a frame of the other one, this would describe that one,
    // and the next frame of this one would push it all back.
    if (stage.lent) return;
    // The toolbar also runs on Windows, where there is no native menu.
    const nextTools = toolbarState(commands);
    const nextKey = JSON.stringify(nextTools);
    if (nextKey !== toolStateKey) {
      toolStateKey = nextKey;
      toolState = nextTools;
    }
    if (!menuInstalled) return;
    const state = menuEnablement(commands);
    // Compared rather than pushed, because this is now called from the frame
    // loop: the enablement of twenty commands is twenty closures reading local
    // variables, and the message across the boundary is the expensive half.
    const key = JSON.stringify(state);
    if (key === menuPushed) return;
    menuPushed = key;
    void call("set_menu_enabled", { state }).catch(() => {
      // Quiet, unlike the install. This runs after every edit, and a menu
      // whose greying is one step behind is not worth putting a red line in
      // front of a reader for.
      //
      // Forgotten as well as unreported: a failed push left the menu saying
      // something else, and remembering it as sent would withhold the next
      // identical attempt --- which is the one that would have corrected it.
      menuPushed = "";
    });
  }

  function toggleInvert() {
    if (!viewer) return;
    invertPages = !viewer.inverted;
    // The colours are the window's and each viewer holds its own copy, so the
    // document on the other side is set too.
    stage.each(() => viewer?.setInverted(invertPages));
    // Written directly rather than through the place writer. The writer skips a
    // place identical to the last one it sent, and inverting the page moves
    // nothing --- so routed that way, a reader who inverts and quits without
    // scrolling would find the preference forgotten.
    void call("session_set_invert_pages", { invert: invertPages }).catch(() => {
      // Same posture as a failed place write: losing the preference is worth
      // less than a dialog saying so.
    });
  }

  /**
   * Hands the open document to the platform print dialog.
   *
   * The view rotation goes with it, because the reader asked to see the page
   * that way and printing it the other way round would be a surprise. Nothing
   * else about the view does: zoom, inversion and the scroll position are
   * properties of a screen, and a printed page that came out inverted because
   * the room was dark would be a genuinely expensive mistake.
   *
   * Resolves when the panel has been *asked for*, not when it closes --- the
   * backend cannot tell a cancel from a failure (see `print_macos::present`),
   * so there is no outcome here worth waiting for.
   */
  async function printDocument() {
    if (opening) return;
    return documentTasks.run(async () => {
      if (!openPathName || !viewer) return;
      try {
        await call("print_document", {
          path: openPathName,
          // The edits go with it: which pages are left, and how each is turned.
          // Read from the model rather than sent from here --- the frontend's copy
          // is a cache, and a print job built from a stale one would put a page on
          // paper that the reader has deleted.
          doc: openDoc,
          pages: null,
          turns: viewer.rotation,
        });
    } catch (e) {
      // Shown, unlike a failed place write. This one the reader is standing
      // there waiting for: a print command that silently does nothing reads as
      // a broken application, and there are several ways for this one to
      // refuse -- an encrypted document, a job PDFKit will not read back, a
      // page count that does not match what was asked for, and a file that has
      // changed on disk since it was opened.
      //
      // The last of those is the reason this goes through the rules rather than
      // straight to `say`. It said `String(e)` until the refusal grew fields,
      // and a sentence naming a fallback has to arrive with the fallback beside
      // it: the reader's unsaved edits are what the job would have been built
      // from, so Save a copy keeps them and Reload starts again from the file
      // underneath. A print closes nothing, so `reopen` never arrives here and
      // the window has nothing to reopen; `afterRefusal` reads that from the
      // flags rather than from which command it was called by.
      const prompt = afterRefusal(refusalOf(e));
      say(prompt.message, prompt.offers);
    }
    });
  }

  function toggleSidebar() {
    sidebarShown = !sidebarShown;
    // The viewer's own ResizeObserver notices the width it just lost or got
    // back, so nothing here has to tell it.
    sidebar?.setVisible(sidebarShown);
    notePlace();
  }

  /**
   * Where the reader is, right now, in the shape the session keeps.
   *
   * Extracted so {@link notePlace} and {@link reloadDocument} cannot drift: a
   * reload that rebuilt this object by hand would be a second definition of
   * "the reader's place", and the two would disagree the first time a field was
   * added to `Place`.
   */
  function currentPlace(inFile = true): Place | null {
    if (!viewer || !openPathName) return null;
    const where = viewer.position;
    return {
      path: openPathName,
      // The page of the *file*, not the slot. A place outlives the edits that
      // are not saved with it: the reader deletes page 2, quits, and reopens the
      // file as it is on disk --- where the slot they were on names a different
      // page, and the page they were reading is still where it was. Read back as
      // a slot by `restore`, which is the same number on a document that has
      // just been opened and is where a session that carried edits would have to
      // translate instead.
      // `inFile` is false for exactly one caller, and it is not a nuance: after
      // a save in place the file's pages **are** the reader's order, so the
      // translation below --- which asks which baseline page a slot came from ---
      // would send them to whichever page used to be there. On a document with a
      // deletion in it the two answers differ by the deletion.
      // `nearestSourceAt` rather than `sourceOf`, because this has to answer
      // with a page of the *file* and a reader can be looking at a page tpdf
      // made --- which is in no file and cannot be restored. The page before it
      // is where they would have been had they not inserted one.
      page: inFile
        ? (edits?.map.nearestSourceAt(where.page) ?? where.page)
        : where.page,
      top_pt: where.top,
      zoom: viewer.currentZoom,
      fit: viewer.fitMode,
      turns: viewer.rotation,
      sidebar: sidebarShown,
      page_count: openPageCount,
    };
  }

  /**
   * Records where the reader is, for the next launch.
   *
   * Called from both `onStatus` and `onPosition` because neither is enough on
   * its own: the status fires when something a reader would notice changed and
   * so misses scrolling *within* a page, and the position fires every frame and
   * carries no zoom or rotation. The writer collapses the overlap.
   */
  function notePlace() {
    const where = currentPlace();
    if (where) places.note(where);
  }

  /**
   * Shows a message, and the buttons that go with it.
   *
   * One function because the two are one fact. Setting `error` and `offers`
   * separately is a pair that drifts, and the way it drifts is a stale Reload
   * button surviving next to an unrelated message --- which is a button that
   * discards the reader's work, offered for a reason that has gone. `say(null)`
   * clears both.
   */
  function say(message: string | null, next: Offer[] = []) {
    redactedCopyPath = null;
    error = message;
    offers = message === null ? [] : next;
  }

  /**
   * Opens the current document's path again.
   *
   * The place is captured here and handed to the open, rather than left to the
   * lookup that a launch restore uses. `session` holds the places as they were
   * when the recent documents were last re-read --- `places.note` writes over
   * IPC to Rust --- so reopening the current path would put the reader back
   * where they were then, which on a long session is nowhere near where they
   * are now.
   *
   * Nothing is guarded on the file having changed. Reload is also what someone
   * reaches for when they know they have changed it, and a command that
   * refuses because the app has not noticed yet is worse than one that always
   * does what it says.
   */
  async function reloadDocument(): Promise<void> {
    const path = openPathName, view = openView, model = edits;
    if (!path) return;
    // Reload reopens the file, which closes the document and spends the journal.
    // On an unedited document that costs nothing; on an edited one it is the
    // reader's work, and until 2026-08-19 it went without a word -- the command
    // was written before there was anything to lose, and nothing revisited it
    // when there was. The second press is what confirms: `reloadAnyway` is the
    // offer this prompt carries.
    const prompt = await reloadAsked({
      settle: settleDrafts,
      dirty: () => asDocument(view, () => dirty) ?? false,
    });
    // The wait let another document in: the answer is about one that has gone,
    // or about the side the reader has left.
    if (!stillIn(view, model)) return;
    if (prompt) {
      say(prompt.message, prompt.offers);
      return;
    }
    reloadAnyway();
  }

  /**
   * What the reader chose to have happen when the open file changes on disk.
   * The rules, the watch and the storage are `diskwatch.ts`'s; this is the
   * current value and the two places it is used.
   */
  let diskChangeMode: DiskChangeMode = readDiskChangeMode();
  const diskWatch = new DiskWatch(
    {
      stamp: (doc, path) => call("document_stamp", { doc, path }),
      differs: (doc, path) => call("document_differs", { doc, path }),
    },
    (doc) => {
      // Not now, rather than not at all: the watch reports again on its next
      // check. A save's own write is the case that matters --- it changes the
      // file and then reopens it, and must not be answered with a reload.
      const current = () => doc === openDoc && !opening && !documentBusy;
      if (!current()) return false;
      // What happens, and the settling that comes before a reload nobody is
      // asked about, are `onDiskChange`'s.
      return onDiskChange({
        mode: () => diskChangeMode,
        name: () => title,
        settle: settleDrafts,
        dirty: () => dirty,
        current,
        reload: reloadAnyway,
        say: (prompt) => say(prompt.message, prompt.offers),
      });
    },
  );

  /** One look at the open document's file. Called on a timer and on focus. */
  function checkDisk() {
    if (diskChangeMode === "ignore" || openDoc < 0 || !openPathName) return;
    if (opening || documentBusy || document.visibilityState !== "visible") return;
    void diskWatch.check(openDoc, openPathName);
  }

  function setDiskChangeMode(mode: DiskChangeMode): void {
    diskChangeMode = mode;
    const kept = writeDiskChangeMode(mode);
    notice = {
      ask: "tpdf will ask before reloading a file that changed on disk.",
      reload: "tpdf will reload a file that changed on disk, unless it has unsaved edits.",
      ignore: "tpdf will not check whether the open file changed on disk.",
    }[mode] + (kept ? "" : " The choice could not be saved and lasts until tpdf closes.");
    refreshMenu();
  }

  /**
   * Reloads whatever {@link reloadDocument} was about to, warning or not.
   *
   * Separate so that the Reload button on a warning does not have to re-enter
   * the guard that produced the warning --- a confirmation that asks the same
   * question again is a loop, and this is the one place where "the reader has
   * already been told" is true.
   */
  function reloadAnyway() {
    const path = openPathName;
    if (!path) return;
    say(null);
    void openPath(path, false, currentPlace());
  }

  /** Opens the sidebar if it is closed, on the tab asked for. */
  function showTab(tab: Tab) {
    if (!sidebarShown) toggleSidebar();
    sidebar?.selectTab(tab);
  }

  /**
   * Scrolls to a pending region, named by its redaction id.
   *
   * The region's own top edge rather than the page's, so a reader checking the
   * fourth region on a long page lands on it. `goToDestination` is what turns
   * that into a scroll position: it owns the margin above a destination, and it
   * owns the rule that a turned page has no vertical offset worth scrolling to
   * --- both of which would be a second copy of a hard-won answer if this
   * scrolled by itself.
   *
   * Silent when the region is gone or its page is in no slot. There is nowhere
   * to go, and `redactlist.ts` refuses to activate such a row from either the
   * pointer or the keyboard, so reaching here means the model changed under the
   * press rather than that a reader needs telling.
   */
  function showRedaction(id: number): void {
    const region = edits?.state.redactions.find((row) => row.id === id);
    if (!region || !edits) return;
    const slot = edits.map.slotOfId(region.page);
    if (slot === undefined) return;
    viewer?.goToDestination(slot, region.area[1]);
  }

  /**
   * Resolves once the viewer has something on screen, or after a short grace.
   *
   * The grace matters more than the signal: a document whose first page is slow
   * --- the A0 sheet takes seconds --- must still get its outline, so this is a
   * scheduling preference rather than a dependency. Anything that waits on the
   * viewer without a way out is a feature that silently never arrives.
   */
  function firstPaint(id: ViewId = openView): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(done, 1000);
      const started = performance.now();
      function done() {
        clearTimeout(timer);
        resolve();
      }
      function poll() {
        // The document's own status, which is not the one in the header when
        // the reader is working in the other side. A document that has gone
        // answers nothing, and the wait ends on its clock.
        const drawn = asDocument(id, () => status?.any ?? 0) ?? 0;
        if (drawn >= 0.999 || performance.now() - started > 1000) done();
        else requestAnimationFrame(poll);
      }
      requestAnimationFrame(poll);
    });
  }

  /**
   * Registers one command per recently-read document.
   *
   * The list is `session.rs`'s, which is already most-recent-first, deduplicated
   * by path and truncated --- so nothing here decides an order, and the ordering
   * rule lives in exactly one place. Reaching the second entry has simply never
   * been possible until now.
   *
   * Nothing checks that the files still exist. That would be one filesystem call
   * per entry on a path a keystroke waits behind, to prevent an error message
   * that `openPath` already produces correctly --- and a document on a volume
   * that is not mounted right now is one a reader may well want offered.
   */
  function offerRecents(from: Session) {
    // The places an open restores from are the ones the rows were written
    // from, or a row saying "page 12" would open on whatever page the document
    // was on when the application started.
    session = { ...session, places: from.places };
    // The rows first and the commands from them: `startpage.ts` has why.
    commands.replace(
      RECENT_PREFIX,
      recentCommands(startPage.offer(from), (path) => void openPath(path)),
    );
  }

  /** Rebuilds the recent-document commands from disk, then re-ranks. */
  async function refreshRecents() {
    offerRecents(await loadSession());
    // The palette may have been opened while this was in flight, or closed
    // again, or moved into argument mode. `reload` is a no-op in the last two.
    palette?.reload();
    refreshMenu();
  }

  /**
   * Re-reads the list for the blank page a closing document leaves behind.
   *
   * Behind that document's last place, which `settleDocument` has issued and
   * the store may not have answered: read before it, the document just closed
   * would be missing from the top of the list it belongs at the top of.
   */
  async function refreshStartPage() {
    await behindWrites(places, refreshRecents);
  }

  /** Runs what a control on the blank page names, as the palette would. */
  function runStartCommand(id: string) {
    commands.run(id);
    refreshMenu();
  }

  /** Puts the focus on a row of the blank page, or on its button for -1. */
  function focusStartRow(index: number) {
    const target = index < 0
      ? startHost?.querySelector<HTMLElement>(".start-open")
      : startHost?.querySelectorAll<HTMLElement>(".recent-open")[index];
    target?.focus();
  }

  /** Takes one document off the list, and keeps the keyboard in it. */
  async function forgetRecent(row: StartRow, index: number) {
    const said = await behindWrites(places, () => call("session_forget", { path: row.path }));
    if (said) say(said);
    await refreshRecents();
    await tick();
    focusStartRow(focusAfterRemoval(index, startRows.length));
  }

  /**
   * Forgets every remembered document but the one on screen.
   *
   * That one is being read, so it is noted again at once: the writer is told
   * the store dropped it, or the note would compare equal to the last one
   * written and be suppressed. The blank page is not showing while a document
   * is, so nothing on screen goes and comes back.
   */
  async function clearRecents() {
    const said = await behindWrites(places, () => call("session_clear_places"));
    if (said) say(said);
    places.forgotten();
    notePlace();
    await refreshStartPage();
    if (!said) tell("Recent documents cleared.");
  }

  /**
   * The blank page's keys. `at` is the focused row, or -1 for none.
   *
   * @returns whether the key was this page's.
   */
  function startPageKey(event: KeyboardEvent, at: number): boolean {
    const move = startMove(event, at, startRows.length);
    if (!move) return false;
    event.preventDefault();
    if ("focus" in move) focusStartRow(move.focus);
    else {
      const row = startRows[move.remove];
      if (row) void forgetRecent(row, move.remove);
    }
    return true;
  }

  /** Which row of the blank page an event came from, or -1. */
  function startRowOf(event: Event): number {
    const item = (event.target as HTMLElement | null)?.closest<HTMLElement>("[data-start-row]");
    return item ? Number(item.dataset.startRow) : -1;
  }

  function focusFind() {
    findShown = true;
    void tick().then(() => {
      findField?.focus();
      findField?.select();
    });
  }

  /**
   * How long typing has to pause before a scan starts, in milliseconds.
   *
   * Every keystroke supersedes the scan before it, so the cost of typing
   * without this is bounded by the pages each attempt got through --- but on a
   * 775-page document that is still a queue of page requests in front of the
   * tiles for no result anyone will read.
   */
  const FIND_DEBOUNCE_MS = 150;

  /**
   * Flips one matching option and rescans if there is a query.
   *
   * The viewer owns the setting rather than this component, because it is the
   * viewer that has to rescan when it changes and because the check harness
   * mounts the viewer without any of this. What is here is the toggle.
   */
  function toggleSearchOption(which: "matchCase" | "wholeWord" | "regex") {
    const now = viewer?.searchOptionsNow;
    if (!now) return;
    viewer?.setSearchOptions({ ...now, [which]: !now[which] });
  }

  /**
   * Confines the search to the selection, or releases it.
   *
   * Nothing to say when there is no selection: the command is disabled without
   * one and the toolbar button is too, so this is only reachable with something
   * to scope to or something to release.
   */
  function toggleSearchScope() {
    if (!viewer) return;
    if (viewer.searchScoped) viewer.clearSearchScope();
    else viewer.scopeSearchToSelection();
  }

  /**
   * The toolbar button's version, which also puts the caret back.
   *
   * Clicking a button takes focus, and a reader who flips whole-word mid-search
   * is still typing a query. Not folded into {@link toggleSearchOption}: the
   * keyboard route reaches the same toggle from the document, and yanking focus
   * into the find field there would be a shortcut that moves the caret.
   *
   * `focus()` without `select()`, unlike `focusFind`: the query is not being
   * replaced, it is being refined.
   */
  function toggleSearchOptionFromToolbar(which: "matchCase" | "wholeWord" | "regex") {
    toggleSearchOption(which);
    findField?.focus();
  }

  function onFindInput() {
    clearTimeout(findTimer);
    const wanted = query, view = openView;
    findTimer = setTimeout(
      () => asDocument(view, () => { findTimer = 0; viewer?.search(wanted); }),
      FIND_DEBOUNCE_MS,
    );
  }

  function onFindKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      // Enter before the debounce has fired should search, not step through the
      // nothing it has found so far.
      clearTimeout(findTimer);
      if (status && status.search.query !== query) viewer?.search(query);
      else if (event.shiftKey) viewer?.prevMatch();
      else viewer?.nextMatch();
    } else if (event.key === "Escape") {
      event.preventDefault();
      clearTimeout(findTimer);
      query = "";
      viewer?.clearSearch();
      findShown = false;
      viewer?.focus();
    }
  }

  /**
   * The toolbar's route into the palette.
   *
   * Through `togglePalette` rather than `palette?.open()`, so the button and ⌘K
   * are one implementation --- opening without the recents refresh beside it is
   * exactly the kind of half-copy that leaves the button's list stale while the
   * chord's is current, with nothing to say so.
   */
  function openPaletteFromToolbar() {
    togglePalette({
      palette: () => palette,
      refreshRecents: () => void refreshRecents(),
    });
  }

  /**
   * The shortcuts that belong to the window rather than to the surface.
   *
   * The routing is `appcommands.ts`'s, for the reason the registration above
   * gives: ⌘K was unreachable by any check while it lived in this file.
   */
  function onWindowKey(event: KeyboardEvent) {
    // First, and it has to be: Escape closes the menu rather than the find bar,
    // and the arrows walk its rows rather than scrolling the document under it.
    // A closed menu consumes nothing, so this costs one boolean per keystroke.
    if (contextMenu?.handleKey(event)) {
      event.preventDefault();
      return;
    }
    // The first Down on a blank page enters its list. Only with nothing
    // focused: a key pressed inside the page is the page's own handler's, and
    // one pressed in the palette or a dialog is not this page's at all.
    const focused = document.activeElement;
    if (!title && (focused === null || focused === document.body) && startPageKey(event, -1)) return;
    handleWindowKey(event, {
      actions: appActions,
      palette: () => palette,
      hasDocument: () => title !== "",
      refreshRecents: () => void refreshRecents(),
    });
  }

  /** What the find field's counter says. */
  const findLabel = $derived.by(() => {
    const search = status?.search;
    if (!search || !search.query) return "";
    // Before every other answer: a pattern that did not compile was never run,
    // so "no matches" would be a statement about the document rather than about
    // the query. Only a pattern can produce one.
    if (search.problem) return search.problem;
    if (search.textless) {
      // Distinct from "no matches" on purpose: the query was never tested
      // against anything, and saying so is the difference between a working
      // search and a broken one from the reader's side.
      return search.running ? "no text yet" : "no text to search";
    }
    // "in selection" rides on every answer below it, the empty one included: a
    // reader who is told "no matches" while the search can only see three lines
    // has been told something false about the document.
    const where = search.scoped ? " in selection" : "";
    if (search.total === 0) {
      return (search.running ? "searching" : "no matches") + where;
    }
    return `${search.index} of ${search.total}${search.running ? "+" : ""}${where}`;
  });

  $effect(() => {
    // Named for `scripts/check_view_after_await.py`, which lists it by name.
    void (async function boot() {
      // Automated spike runs, if their env var is set. Each exits the process
      // when done, so nothing below runs.
      if (await runStartupTimelineIfRequested()) return;
      if (await runAutobenchIfRequested()) return;
      if (await runScrollBenchIfRequested()) return;
      if (await runViewerCheckIfRequested()) return;

      // Anything a previous webview left the backend holding. This page holds no
      // document id yet, so every id the backend has is one nobody can name ---
      // see `orphans.ts` and `release_documents` in `lib.rs` for the whole
      // argument, including what it assumes about there being one window.
      //
      // After the spike entry points and before anything opens a document. Both
      // matter: a check harness returns above this and must not have its own
      // documents released mid-run, and releasing after an open would drop the
      // document this page had just been given.
      //
      // Not awaited. A reader who has just started the application is waiting for
      // a page, not for housekeeping, and `releaseOrphans` never rejects.
      void releaseOrphans(
        () => call("release_documents"),
        (line) => console.info(`[open] ${line}`),
      );

      // After the spike entry points, which exit the process: none of them mount
      // the shell, and a palette attached to `document.body` would outlive it.
      palette = new Palette(commands);

      // On `document.body` rather than inside the viewer, for the reason the
      // context menu is: a modal that lives in a scroll box is clipped by it.
      signatureDialog = new SignatureDialog(document.body);
      propertiesDialog = new PropertiesDialog(document.body);

      // Beside it, and for its reason. A locked document is asked about
      // rather than reported, which is the whole of what this adds --- see
      // `passworddialog.ts`.
      passwordDialog = new PasswordDialog(document.body);
      newPasswordDialog = new NewPasswordDialog(document.body);
      fieldPropertiesDialog = new FieldPropertiesDialog(document.body);
      compressDialog = new CompressDialog(document.body);

      // And beside that. A web link is asked about rather than followed, and
      // the dialog is the whole of the confirmation --- see
      // `weblinkdialog.ts`, which is where the reasoning about what a reader
      // may be shown lives.
      webLinkDialog = new WebLinkDialog(document.body);

      // On `document.body`, not inside a panel: a menu opened on the last row
      // of the page strip would otherwise be clipped by that panel's scroll
      // box. Runs a chosen command through the registry, exactly as the palette
      // and the menu bar do.
      contextMenu = new ContextMenu(document.body, commands, (id, at) => {
        // One command reads where the menu was opened, because *where* is the
        // whole of what a right-click adds over the palette. It is routed
        // rather than run through the registry for the same reason the strip's
        // right-click navigates first: the point has to reach the placement,
        // and a command signature that carried one would put a pointer
        // coordinate into every route that has no pointer.
        if (id === "edit.addComment" && at) {
          void addComment({ clientX: at.x, clientY: at.y });
          return;
        }
        commands.run(id);
      });
      // Any press outside the menu dismisses it, on the way down rather than on
      // click, so a press that lands on the document does not both dismiss the
      // menu and do whatever it was going to do -- the menu is gone by the time
      // the click arrives.
      window.addEventListener("pointerdown", (event) => {
        if (!contextMenu?.isOpen) return;
        const inside = (event.target as HTMLElement | null)?.closest?.(
          ".context-menu",
        );
        if (inside) return;
        contextMenu.close();
        // The pick goes with the menu it was made for. It is not a selection a
        // reader can build on --- nothing else reads it, and leaving a region
        // drawn heavy after the menu is gone would say the application is still
        // pointing at it. A press *inside* the menu is the one that chooses a
        // row, so the pick has to outlive that one.
        viewer?.pickRedaction(null);
      });
      // The web view's own menu, everywhere it is not replaced. Its one entry
      // reloads the frontend, which drops the reader's view of the document --
      // a developer affordance that has been shipping to readers.
      window.addEventListener("contextmenu", (event) => {
        event.preventDefault();
        // On the document surface, offer what a selection can do. Elsewhere --
        // the toolbar, the panel's chrome -- nothing is offered, and nothing is
        // the right answer rather than a menu of commands about somewhere else.
        const onSurface = (event.target as HTMLElement | null)?.closest?.(
          ".surface",
        );
        if (onSurface) {
          const at = { x: event.clientX, y: event.clientY };
          // A mark under the pointer wins, because a right-click on a highlight
          // is a request to do something to *that highlight* --- the same
          // argument `contextmenu.ts` makes for the page strip. Before this it
          // offered the selection menu, so the only route to taking a mark off
          // was to left-press it for its note box first.
          //
          // The note is opened rather than the mark being passed to the menu,
          // and that is deliberate: it is how every other route names the mark
          // it means, so `edit.removeMark` needs no second way to be told which
          // one. The strip does the same thing by navigating to the page.
          const own = viewer?.markAt(event) ?? null;
          // A pending region wins over a mark, for the reason `menuForSurface`
          // gives: it is drawn over every mark, and the thing on top is the
          // thing the right-click is about. Picked rather than passed to the
          // menu, which is how the mark below names itself and how the page
          // strip names a page --- see `edit.removeRedaction`, which has no
          // second way to be told which region it means.
          const region = viewer?.redactionAt(event) ?? null;
          viewer?.pickRedaction(region);
          // Without the keyboard: the menu is what the reader is about to arrow
          // through, and `showMark`'s default puts the caret in the note's text
          // field, which would eat every key the menu needs.
          if (region === null && own !== null) viewer?.showMark(own, false);
          openContextMenu(menuForSurface(own, region), at);
        }
      });

      // After the palette, and it has to be: a menu item for a command that
      // takes an argument opens the palette rather than running anything, so
      // installing the menu first would put a live item in the bar with nowhere
      // for its value to be typed.
      // Before the menu, and before anything reads a shortcut label. The
      // platform is asked what this keyboard prints on the keys a binding can
      // name by position --- `keylayout.rs` has why it has to be asked at all ---
      // and the labels are re-rendered from the answer. A failure here is quiet:
      // the labels stay as the characters their bindings declare, which is what
      // the palette showed before any of this existed.
      try {
        setPrintedKeys(
          await call("keyboard_positions"),
        );
        relabelCommands(commands);
      } catch {
        // Deliberately silent. Nothing is broken -- a shortcut still works and
        // is still advertised, under the spelling a US keyboard would use.
      }

      await installMenu();
      await getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault();
        await documentTasks.idle();
        await opens.run(async () => {
          opening = true;
          try {
            await settleDocument();
            const unsaved = oneEach(tabs.all).filter((tab) => tab.edits.state.dirty);
            if (unsaved.length && !await confirmDialog(
              `Discard unsaved changes in ${unsaved.length} open document(s)?`,
              { title: "Close tpdf", kind: "warning", okLabel: "Discard changes", cancelLabel: "Keep open" },
            )) return;
            await getCurrentWindow().destroy();
          } finally { opening = false; }
        });
      });

      // Early benchmark/check entry points return above this line. Full-shell
      // checks below can reach it too; the saved preference applies to both.
      // Do not await the endpoint: a slow check must not delay opening a PDF.
      void updates.checkOnLaunch();

      await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        for (const path of event.payload.paths) void openPath(path);
      });

      // A last chance to record the position for a reader who quits inside the
      // writer's interval. Best effort by construction --- the write is an async
      // IPC call and the process need not outlive it --- which is why the
      // interval is a second rather than something that leans on this.
      window.addEventListener("pagehide", () => places.flush());

      // Whether another program rewrote the open file. Once a second is a
      // `stat` and nothing else until one differs; on focus as well, so a
      // reader coming back from the program that wrote it is not kept waiting.
      window.setInterval(checkDisk, 1000);
      window.addEventListener("focus", checkDisk);
      // The link, or the PATH, can be changed from a terminal while tpdf is
      // open, and a greyed command cannot be run to find that out.
      window.addEventListener("focus", () => void commandLineTool.refresh());

      // Reopening the last document is the whole of the feature: a reader that
      // starts empty every morning is not the one someone reaches for. It runs
      // after the spike entry points above, all of which exit the process, so no
      // measurement ever opens a document it was not pointed at.
      // Documents handed over from outside: a double-click, "Open With", or a
      // path on the command line. Drained before anything is restored, because
      // a person who double-clicked a file is asking for *that* file and would
      // read yesterday's document appearing instead as the association being
      // broken. Anything arriving later --- a second double-click while tpdf is
      // already running --- comes in on the event below.
      //
      // The name comes from Rust rather than being agreed in two places: a
      // constant that drifts fails by silence, the app simply ceasing to notice
      // documents opened while it is already running. And the listener is
      // registered *before* the queue is drained, because a path delivered
      // between the two would be emitted to nobody.
      // Read once, and nothing waits on it: every reader tolerates the empty
      // string it starts as. It is baked into the binary at compile time from
      // `CARGO_PKG_VERSION`, so this call can fail in no interesting way.
      appVersion = await call("app_version");

      const openEvent = await call("launch_open_event");
      await listen<string>(openEvent, (event) => void openPath(event.payload));
      // Guarded, because an event can land after the command has answered and
      // would otherwise leave its line on screen with nothing running.
      await listen<Progress>(PROGRESS_EVENT, (event) => {
        if (recognising) blockingTask = progressLine(event.payload);
      });
      // The same guard, kept in `hiddentext.ts`.
      await listen<HiddenProgress>(HIDDEN_PROGRESS_EVENT, (event) => {
        hiddenRuns.report(event.payload, (line) => { blockingTask = line; });
      });
      // Together rather than one after the other. Neither answer feeds the
      // other --- one is what the launcher handed over, the other is what was on
      // disk from last time --- and both are round trips on the path between the
      // window appearing and the first page being asked for, which is the part
      // of startup a reader is watching.
      const [handed, restored] = await Promise.all([
        call("take_launch_paths"),
        loadSession(),
      ]);
      session = restored;
      // From the session already in hand, so opening the palette on the first
      // keystroke costs nothing. Refreshed from disk after that -- see
      // `refreshRecents`.
      offerRecents(session);
      // Read before any document opens, so the first tiles of the first page are
      // requested in the polarity the reader left the application in.
      invertPages = session.invert_pages ?? false;
      recognitionLanguage.restore(session.ocr_language);
      restoreTabs = session.restore_tabs ?? false;
      const plan = launchPlan(session, handed);
      // Held until the last tab is back, so that quitting halfway through does
      // not record the half that had opened as the whole list.
      if (plan.behind.length) tabRecorder.hold();
      for (const path of plan.show) await openPath(path, plan.resuming);
      // After the document the reader is waiting for has been asked for, and
      // not awaited: until it answers, both of the tool's commands are offered.
      void commandLineTool.refresh();
      // After the first page, not before it: the tabs behind are not what the
      // reader is waiting for, and each one is an open the first page would
      // otherwise queue behind.
      if (plan.behind.length) {
        void firstPaint()
          .then(() => openBehind(plan.behind, plan.order, tabHost(false)))
          // Before the recorder is released: a quit between the two would
          // otherwise record the tabs that came back as one side.
          .then(() => restoreSides(plan.sides))
          .finally(() => { tabRecorder.release(); refreshTabs(); refreshMenu(); });
      }

      // Both of these observe the boot rather than replacing it, for the same
      // reason --- see `sessioncheck.ts`. The open check goes first because its
      // `arrives` phase asserts that *nothing* opened, which the session check
      // would have to have finished with to be true.
      if (
        await runOpenCheckIfRequested({
          path: () => openPathName,
          // Through `openPath`, never `openDocument`: the chain is the thing
          // the `race` phase exists to exercise, and a check that went around
          // it would be testing a second implementation of the open.
          open: (path) => openPath(path),
          hasViewer: () => viewer !== null,
          tabs: () => tabRows,
          status: () => status,
          beside: () => asDocument(panes.front(otherSide(panes.focused)), () => viewer) ?? null,
          viewer: () => viewer,
          edits: () => edits,
          apply: async (run) => { await applyEdit(run); },
          // The check names a tab by the id `tabs` below gave it.
          activate: (id) => activateTab(id as ViewId),
          close: (id) => closeTab(id as ViewId),
          run: (id, argument) => { commands.run(id, argument); },
          importPages: (path) => importPagesFrom(path),
          answerSave: (path) => signSaves?.queue(path),
          answerPictures: (paths) => { queuedPictures = paths; },
          saveSuggestions: () => signSaves?.asked ?? [],
          pendingImport: () => pendingImports.current(edits?.doc ?? null),
          idle: async () => { await pendingEdit; await formLayer?.settle(); await textEditor?.settle(); await documentTasks.idle(); await tick(); },
        })
      )
        return;

      // Before the session check, which ends the process, and after the open
      // check for the same reason that one goes first: this needs a document
      // open and that one asserts that nothing is.
      //
      // **Everything it is handed is a handle the application itself uses**, and
      // that is the whole design --- see `markcheck.ts`. What the defect it was
      // written for lived in is the object literal a few lines above this one,
      // where the viewer's callbacks are bound to the functions that reach the
      // model, so a check that reconstructed any of those would have rebuilt the
      // very thing it is meant to observe.
      if (
        await runMarkCheckIfRequested({
          // Through the registry, not the actions behind it: a reader reaches a
          // command by its id, and the enablement guard is part of the chain.
          run: (id) => commands.run(id),
          viewer: () => viewer,
          root: () => surface,
          // The **model's** marks and pages, which is the independent end of
          // every assertion the check makes: they came back over the IPC
          // boundary from Rust, not from the viewer that produced the gesture.
          marks: () => edits?.state.marks ?? [],
          pages: () => edits?.state.pages ?? [],
          path: () => openPathName,
        })
      )
        return;

      await runSessionCheckIfRequested({
        open: (path) => openPath(path),
        viewer: () => viewer,
        root: () => surface,
        path: () => openPathName,
        pageCount: () => status?.pageCount ?? 0,
        sidebarShown: () => sidebarShown,
        toggleSidebar,
        flush: () => places.flush(),
        tabs: () => tabs.all.map((tab) => tab.path),
        setRestoreTabs,
        reopenLastTabs,
        tabsSettled: () => tabRecorder.settled(),
        moveToOtherSide: () => moveTab(openView, otherSide(panes.focused)),
        recentCommands: () =>
          commands
            .all()
            .filter((command) => command.id.startsWith(RECENT_PREFIX))
            .map((command) => command.title),
      });
    })();
  });

  async function pickAndOpen() {
    const chosen = await openDialog({
      multiple: true,
      directory: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    for (const path of typeof chosen === "string" ? [chosen] : chosen ?? [])
      await openPath(path);
  }

  /**
   * Checks, then says what was found -- including "nothing", which is the point.
   *
   * The launch check deliberately does not come through here: an answer nobody
   * asked for is exactly the element-arriving-on-its-own that the header's own
   * silence is designed to avoid.
   */
  async function checkAndSay(): Promise<void> {
    notice = updateNotice({ kind: "checking" }, appVersion);
    notice = updateNotice(await updates.check(), appVersion);
  }

  /**
   * Runs one half of the update, with the same unsaved-work question a close asks.
   *
   * **Why this exists at all**: a relaunch is not a window close. Tauri's
   * `request_restart` sends `ExitRequested` with its own exit code straight at
   * the run-event loop, so no window is asked to close and the
   * `onCloseRequested` handler in `setup` --- the one that counts dirty tabs
   * and asks --- never runs. Wiring the button to `relaunch()` would throw a
   * reader's edits away on one press.
   *
   * The install is gated by the same call, on the platform where it ends the
   * process: on Windows `downloadAndInstall` hands over to the installer and
   * calls `exit(0)`, so it is a quit wearing another name. That direction had
   * no question in front of it before 26.9.17 either, and the answer had to be
   * one function rather than two copies of a dialog.
   *
   * The settle before reading `dirty` mirrors the close path exactly:
   * `documentTasks.idle()`, then `settleDocument()`, which commits open popups
   * and lets a pending edit land. Reading `dirty` before those is reading it
   * early, and reading it early is how a document with unsaved work reports
   * that it has none.
   */
  async function finishUpdateStep(step: FinishStep): Promise<void> {
    const ends = stepEndsProcess(step, isMac());
    await finishUpdate(step, updates.state, ends, {
      unsaved: async () => {
        await documentTasks.idle();
        await settleDocument();
        // One name for each document: one on both sides is two tabs.
        return oneEach(tabs.all)
          .filter((tab) => tab.edits.state.dirty)
          .map((tab) => basename(tab.path));
      },
      confirm: (prompt) => confirmDialog(prompt.message, {
        title: prompt.title,
        kind: "warning",
        okLabel: prompt.okLabel,
        cancelLabel: prompt.cancelLabel,
      }),
      act: async () => {
        // `install` reports its own failures through the state machine, which
        // is what puts `failed` in the header; it never rejects, so `say`
        // below is not a second channel for the same news.
        if (step === "install") {
          await updates.install();
          return;
        }
        // The place is issued by `settleDocument` above and **awaited** here,
        // which `flush()` on its own does not do. The process ends inside the
        // next call, so an unanswered write is a lost reading position ---
        // restoring after an update has to land where an ordinary restart
        // lands, and this is the only step in the path that can wait.
        await places.settled();
        await tabRecorder.settled();
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      },
      say: (message) => say(message),
    });
    refreshMenu();
  }

  function setAutomaticUpdates(enabled: boolean): void {
    try {
      updates.setAutomatic(enabled);
      notice = enabled
        ? "Automatic update checks enabled for future launches."
        : "Automatic update checks disabled. You can still use Check for updates. A check already started may finish.";
    } catch {
      notice = enabled
        ? "Could not save the update preference. Automatic checks remain disabled."
        : "Automatic checks disabled for this launch, but the preference could not be saved for future launches.";
    }
    refreshMenu();
  }

  /**
   * Opens a document, one at a time.
   *
   * **Serialised, and it has to be.** The body below suspends three times --- on
   * the open, on a frame, and on the outline --- while mutating `openDoc`,
   * `viewer`, `sidebar` and `openPathName`, none of which can be half-updated.
   * Two of the six callers fire it without awaiting anything (`onDragDropEvent`
   * and the `OPEN_EVENT` listener), so two opens genuinely interleaved: each
   * read the *other's* freshly-set `openDoc` as its `outgoing` and released the
   * document the other was about to build a viewer on, and the second
   * `new Viewer` overwrote the first without destroying it --- leaving two
   * viewers with live `wheel`, `keydown` and `pointerdown` listeners on the same
   * element, and two sidebars in the DOM, since `Sidebar` appends rather than
   * replacing. Combined with a tile request that could not stop failing, that
   * was a pegged core for the life of the process.
   *
   * A chain rather than a generation counter, because the invariant is "one
   * document at a time" and a chain says exactly that. The cost is that a second
   * double-click waits for the first open, which is why the body no longer waits
   * on `firstPaint()` --- see the outline note at the end of it.
   */
  function openPath(
    path: string,
    resuming = false,
    resume: Place | null = null,
  ): Promise<void> {
    // A save's own reopen must not wait for the save that requested it.
    const ready = resume ? Promise.resolve() : documentTasks.idle();
    return ready.then(() => opens.run(async () => {
      const existing = resume ? undefined : tabs.forPath(path);
      // A document already open is shown where it is, on its own side.
      if (existing) { await showTabNow(existing.view); viewer?.focus(); return; }
      const failed = await openDocument(path, resuming, resume);
      // A second view of a document that was saved or reloaded went with the
      // old handle. It is mounted again on the new one, where it was. So is
      // the tab of a reload that failed, on the handle it had, and the reader
      // is then told why in the document that is shown.
      if (resume && panes.plan().mount.length) {
        opening = true;
        try { await showPanes(failed); }
        finally { opening = false; refreshMenu(); }
      }
    }));
  }

  /** The edit model of a document, whether or not it is the one on screen. */
  function editsFor(doc: DocumentInfo): Edits {
    return new Edits(doc.id, doc.page_count, async (merging = false) => {
      await settleDrafts();
      await confirmSignatureSave(() => call("document_properties", { doc: doc.id }),
        askSignatureSave, merging);
    });
  }

  const isOpenTab = (path: string) => tabs.forPath(path) !== undefined;

  /**
   * What `openBehind` in `tabrestore.ts` drives: a tab with a handle and a
   * model and no viewer, which `activateTab` mounts like any tab switched to.
   *
   * `asking` is whether a document behind a password may prompt for it. A
   * launch does not: a dialog about a tab the reader cannot see, before they
   * have done anything, is not one they can place. The command does.
   */
  function tabHost(asking: boolean): TabHost {
    return {
      openBehind: (path) => opens.run(async () => {
        if (isOpenTab(path)) return;
        const doc = asking ? await openOrAsk(path) : await openWithPassword(
          (password) => call("open_document", { path, password }), null);
        if (!doc.pages[0]) {
          await call("close_document", { doc: doc.id }).catch(console.warn);
          throw new Error("document reports no pages");
        }
        tabs.add({ view: viewOf(doc.id), doc, path, edits: editsFor(doc), place: null, ...freshState() });
        panes.joined(viewOf(doc.id));
        refreshTabs();
      }),
      arrange: (order) => { tabs.arrange(order); refreshTabs(); },
      showing: () => openDoc >= 0,
      showFirst: async () => {
        const first = tabs.all[0];
        if (first) await activateTab(first.view);
      },
    };
  }

  async function reopenLastTabs(): Promise<void> {
    await documentTasks.idle();
    const refused = await openBehind(tabsToReopen(session, isOpenTab), [], tabHost(true));
    refreshMenu();
    const said = afterReopen(refused, basename);
    if (said) say(said);
  }

  function setRestoreTabs(restore: boolean): void {
    restoreTabs = restore;
    refreshMenu();
    void call("session_set_restore_tabs", { restore }).catch(() => {
      tell("Could not save that choice. It holds until tpdf is closed.");
    });
  }

  /**
   * Opens `path`, asking the reader for a password if it turns out to need one.
   *
   * The decision is in `unlock.ts` so that it can be tested; what stays here is
   * the `invoke` and the dialog, which are the two things this component is the
   * right place for. A refusal that is not about the password, and a reader who
   * declines to answer one, both arrive at the caller's `catch` as the refusal.
   */
  async function openOrAsk(path: string): Promise<DocumentInfo> {
    // Captured once, so the closure cannot see it become null between the
    // check and the call --- and so the type-checker can see that too.
    const dialog = passwordDialog;
    return openWithPassword(
      (password) => call("open_document", { path, password }),
      dialog ? (problem) => dialog.ask(basename(path), problem) : null,
    );
  }

  /**
   * The runner `scoped` wraps a document's callbacks with. Every callback the
   * viewer and the panels are built with runs as this document, whichever one
   * the reader is working in when it fires.
   */
  function runningAs(view: ViewId): <R>(work: () => R) => R | undefined {
    return (work) => asDocument(view, work);
  }

  /**
   * Whether `view` is still mounted with the viewer `mounted`. Asked as the
   * document, so the answer is the same whichever side the reader is working
   * in when a reply lands.
   */
  function stillMounted(view: ViewId, mounted: Viewer): boolean {
    return asDocument(view, () => viewer === mounted) === true;
  }

  /** Closes the two panels that are about the document that is going. */
  function closeDocumentPanels(): void {
    propertiesDialog?.close();
    // A panel about one field of the document that is closing. Left open, its
    // Save would look the field up by an id that starts at 1 in every
    // document; `changeProperties` refuses that too, and closing it is what
    // the reader sees.
    fieldPropertiesDialog?.close();
  }

  /**
   * The sidebar of the document `openDocument` is mounting as `view`, built in
   * `host`. `page` is the first page's size, which the strip lays every row
   * out at.
   *
   * One options literal, and kept as one: the `wiring` gate reads the `pages`
   * block of it as text.
   */
  function buildSidebar(host: HTMLElement, view: ViewId, doc: DocumentInfo, page: PageSize): Sidebar {
    return new Sidebar(host, scoped<SidebarOptions>({
      onNavigate: (target, top) => {
        viewer?.goToDestination(target, top);
        viewer?.focus();
      },
      // `outline`, not `links`: the two scans number their tokens
      // independently, so the wrong word here opens a different address
      // rather than failing. See `webopen::Source`.
      onWebLink: (target) => void followWebLink("outline", target),
      results: {
        // Focus stays where it was, unlike an outline row. A reader picking
        // hits off this list is comparing them, and taking focus to the page
        // after each one means clicking back into the panel to try the next.
        onPick: (index) => viewer?.showMatch(index),
      },
      comments: {
        // Focus moves into the note, which is the opposite of the results
        // list above and for the reason that distinguishes them: a hit is
        // something to look at on the page, and a comment is something to
        // *read* in the note that opens --- so the keyboard belongs there.
        onPick: (id) => viewer?.showComment(id),
      },
      marks: {
        // The comments row's reasoning, one step stronger: a reader who picks
        // one of their own marks out of a list is reaching for the box that
        // edits it, so the keyboard goes into the field. The keyboard walk is
        // the route that deliberately does not --- there the reader is
        // stepping rather than writing, and taking focus would strand them.
        onPick: (id) => viewer?.showMark(id),
        // The mark named by id, which nothing else in this file does --- see
        // `removeMark` above for the rule this breaks and `marklist.ts` for
        // why it has to. A mark the model could not place is listed here and
        // nowhere else, so the open note cannot name it and this is its only
        // way off. `applyEdit` is the same path every other edit takes, so it
        // journals, undoes and refreshes the panel exactly as they do.
        onRemove: (id) => void applyEdit((e) => e.unmark(id)),
        // What the selection said when the mark was made, or "" --- see
        // `covered` above for why this is held here and not in the model.
        coveredFor: (id) => covered.get(id) ?? "",
      },
      redactions: {
        fill: {
          current: redactionFill,
          onChange: (fill) => {
            redactionFill = fill;
            writeFill(fill);
          },
        },
        // Focus stays in the panel, which is the results list's arrangement
        // rather than the marks list's, and for the results list's reason: a
        // reader working down this list is *comparing* regions --- is that
        // the right box, is that one too wide --- and taking the keyboard to
        // the page after each row means clicking back to reach the next.
        onPick: (id) => showRedaction(id),
        // The only route off a pending region other than undo, and undo is
        // chronological --- a reader who dragged six and wants the second one
        // back cannot get there by undoing. `applyEdit` is the path every
        // other edit takes, so this journals and undoes like the rest.
        onRemove: (id) => void applyEdit((e) => e.unredact(id)),
        // Four answers, and the map deliberately holds no entry for a region
        // nobody has looked at yet: `Map.get` answering `undefined` is what
        // separates *not read* from a page read and found to hold nothing.
        wordsFor: (id) => redactionWords.get(id),
        // Absent until a worker has answered for the region's page, which is
        // why the row draws nothing rather than "no objects": a warning that
        // has not arrived and a region with nothing to warn about must not
        // look alike, and the way they are told apart here is that only one
        // of them ever produces a line.
        planFor: (id) => redactionPlans.get(id),
      },
      hidden: { onPick: (passage) => showHidden(passage) },
      pages: {
        doc: doc.id,
        pageCount: doc.page_count,
        // Page 1 alone, and the strip lays every row out at it. Deliberately
        // left on the uniform assumption the viewer has just stopped making,
        // and it is a *known* gap rather than a proof of harmlessness: see
        // `thumbnails.ts`, which states what a mixed-size document costs there
        // and why the fix is a separate piece of work from this one.
        page,
        // The viewer is created below, so the strip reaches it lazily rather
        // than being handed a reference that does not exist yet.
        tier1: { placeholderFor: (at) => viewer?.placeholderFor(at) ?? null },
        // A row is a slot and a tile request names a page of a document --- this
        // one's, or another file's for a page inserted from it; see `pages.ts`.
        addressOf: (slot) => edits?.map.addressOf(slot, doc.id),
        onNavigate: (at) => {
          viewer?.goToPage(at);
          viewer?.focus();
        },
        // The same call `movePage` makes, and deliberately so: a drag and the
        // two palette commands are one operation reached two ways, and the
        // slot arithmetic that turns a drop into a destination is the strip's
        // because the strip is what knows where the pointer was.
        onReorder: (from, to) => {
          void applyEdit((e) => e.move(from, to));
        },
        // Right-clicking a thumbnail goes to that page first, and then offers
        // the page operations. Navigating on a right-click is unusual and it
        // is the honest arrangement here: every one of these commands acts on
        // the page the viewer is on, so the alternative is a second way to
        // address a page --- and a reader who rotates a page wants to see it
        // turn, which means being on it anyway.
        onContextMenu: (slot, at) => {
          viewer?.goToPage(slot);
          openContextMenu(PAGE_MENU, at);
        },
      },
      // The comments panel lists a bare highlight by the words it covers, and
      // finding those words is one text extraction per page carrying one. So
      // it is paid for by a reader who opens that tab, and by nobody else ---
      // a document opened, read and closed on the outline costs none of it.
      onTab: (tab) => {
        if (tab === "comments") void fillCommentWords();
        // The same bargain for the same reason: the words under a region are
        // a text extraction per page carrying one, and a reader who never
        // opens this tab pays none of it. Unlike the comments walk this one
        // is *also* driven from `runEdit`, because a region the reader has
        // just dragged wants its words while they are looking at the panel.
        if (tab === "redactions") void fillRedactionWords();
        // The ring over a passage belongs to the list that drew it.
        if (tab !== "hidden") viewer?.clearRegion();
      },
    }, runningAs(view)));
  }

  /**
   * Asks the model of the document mounting as `view` what is already on it,
   * and shows the answer when it comes. Not waited for: see {@link adoptModel}.
   */
  function readEditState(view: ViewId, model: Edits): void {
    void model.refresh().then(
      (state) => asDocument(view, () => {
        // The model this reply belongs to, not whichever one is open when it
        // lands. A second document opened inside the round trip replaces
        // `edits` and the panels with it, and this would then translate one
        // document's marks through another's page order and list the result.
        // `dirty` had the same hazard and the same one-line fix.
        if (edits !== model) return;
        dirty = state.dirty;
        refreshTabs();
        // A document opened with edits already on it --- which is the model's
        // to answer, not this file's to assume. Every later change comes
        // through `runEdit` above.
        sidebar?.setMarks(markRows(state.marks, model.map));
        sidebar?.setRedactions(redactionRows(state.redactions, model.map));
      }),
      (e) => {
        // Not raised to the reader. Nothing is wrong with their document ---
        // the edit commands will refuse until this succeeds, which is the
        // right failure, and an error banner over a page that opened fine is
        // not.
        console.warn(`could not read the edit state: ${e}`);
      },
    );
  }

  /**
   * Gives the document mounting as `view` its edit model, its tab and its
   * side, and answers the model.
   *
   * Before the viewer, so that a rotate arriving on the first frame has a
   * model to ask. `refresh` is not awaited: it reads a `HashMap` in the
   * backend, and holding the first page behind it would put an IPC round
   * trip on the startup path for an answer that is "nothing is edited".
   *
   * `from` is what the open found: the tab being returned to, the tab a save
   * or a reload opens the file again in and the handle that tab had, its
   * other views, the place to resume at and what the tab left behind.
   */
  function adoptModel(view: ViewId, doc: DocumentInfo, path: string, from: OpenFound): Edits {
    const { retained, replacing, replaceId, twins, kept } = from;
    const model = retained?.edits ?? editsFor(doc);
    edits = model;
    dirty = model.state.dirty;
    restore(kept, "model", restoring);
    tabs.keep(tabToKeep(view, doc, path, model, from), replacing?.view);
    repoint(twins, doc, model);
    // Two views of one document read one list of what its marks cover.
    const shared = sharedByTwin(tabs.all, tabs.find(view), (twin) => asDocument(twin.view, () => covered));
    if (shared) covered = shared;
    placeOnSide(panes, view, replacing?.view, retained !== undefined);
    refreshTabs();
    if (replaceId !== undefined && replaceId !== doc.id)
      void call("close_document", { doc: replaceId }).catch(console.warn);
    readEditState(view, model);
    return model;
  }

  /**
   * The viewer of the document `openDocument` is mounting as `view`, built in
   * the page area `area`.
   *
   * One options literal, and kept as one: the `wiring` gate reads it as text
   * for every callback `ViewerOptions` declares.
   */
  function buildViewer(
    area: HTMLDivElement,
    view: ViewId,
    doc: DocumentInfo,
    pages: [PageSize, ...PageSize[]],
  ): Viewer {
    return new Viewer(area, scoped<ViewerOptions>({
      doc: doc.id,
      pageCount: doc.page_count,
      pages,
      // The panel's selection follows the page, so a note opened by clicking
      // a mark highlights its row --- and the two can never disagree about
      // which comment is being read, which is the whole reason this is a
      // callback rather than each side tracking its own idea of it.
      onComment: (id) => sidebar?.comments.select(id),
      // The same arrangement for the reader's own marks: pressing one on the
      // page selects its row, so the panel and the box can never disagree
      // about which mark is being read. `markpopup.ts` fires it, because the
      // box is closed by five different things.
      onMark: (id) => sidebar?.marks.select(id),
      // The note the reader typed on one of their own marks, committed when
      // its box closed. A command like any other: it lands in the journal, so
      // undo steps over it and the document is dirty until it is saved.
      //
      // The edit's promise is handed back, in both branches: the page keeps
      // drawing what was typed until it settles, which is after `setMarks`
      // has the model's answer. See `markdraft.ts`.
      onMarkNote: (mark, note) => {
        if (!isSaved(mark)) return applyEdit((e) => e.renote(mark, note));
        const to = scannedForm && edits ? fieldRenamed(scannedForm, edits.state, mark, note) : null;
        if (typeof to === "string") return say(to);
        return to ? applyEdit((e) => e.refield([to])) : undefined;
      },
      // The lines for a text box's words while they are being typed, from
      // the function the model wraps with. Nothing is stored by it.
      onMarkDraft: (note, left, right) => call("annot_draft_lines", { note, left, right }),
      // Somebody else's comment, rewritten. The one edit command addressed by
      // the **object** the file gave the annotation rather than by an id this
      // application issued --- `Comment.id` is a position in one scan, and a
      // save crosses a process boundary.
      //
      // The page is this viewer's slot and the model wants an identity, so
      // the translation happens here, where the map is. A comment on a page
      // the model no longer has translates to nothing and is dropped: it is
      // the same guard `Edits.mark` states, and the reader cannot have got
      // here anyway --- `setComments` closes a popup whose comment has gone.
      onCommentEdit: (comment, body) => {
        const object = comment.object;
        const page = edits?.map.idOf(comment.page);
        if (!object || page === undefined) return;
        void applyEdit((e) => e.rewrite(object, page, body));
      },
      // The same two lookups as the rewrite above and for its reasons: a
      // comment the file wrote as a direct dictionary has no object to name,
      // and one on a page the model no longer has translates to nothing. A
      // comment a reply of the reader's own answers is refused by the model
      // rather than predicted here --- the refusal names the order to do the
      // two in, which is more than this side could say.
      onCommentDelete: (comment) => {
        const object = comment.object;
        const page = edits?.map.idOf(comment.page);
        if (!object || page === undefined) return;
        void applyEdit((e) => e.discard(object, page));
      },
      // The reply's own icon goes on the parent's rectangle, which is why
      // this callback needs a third thing off the comment where the one above
      // needs two. `comment.page` is a slot and `idOf` turns it into the page
      // identity the model addresses --- the translation that exists because
      // an id and a slot are both `number`, which this repository has paid
      // for once.
      onCommentReply: (comment, body) => {
        const object = comment.object;
        const page = edits?.map.idOf(comment.page);
        if (!object || page === undefined) return;
        void applyEdit((e) => e.reply(page, object, comment.rect, body));
      },
      onMarkRemove: (mark, sweep) => removeNamed(mark, sweep),
      // A colour picked in the swatch row, or by a `Colour:` command with a
      // note open. A command like the note above it, and undone the same way.
      // Not for a saved field, which is drawn in one colour to say what it
      // is and has none of its own to change: its id is not a mark's.
      onMarkRecolor: (mark, color) => {
        if (!isSaved(mark)) void applyEdit((e) => e.recolor(mark, color));
      },
      // A box or a drawing the reader finished. The page id and the shape are
      // already in the file's space --- `Viewer.fileRectOn` does that, because
      // the crop and both rotations are the viewer's and nothing here could
      // undo them --- so this is the same one-line journal entry a highlight
      // is, and undo steps over it identically. The shape is handed straight
      // through: which of its two halves is filled is the viewer's answer and
      // the model's rule, and restating it here would be a third copy.
      onDrawn: (kind, page, shape, stamp) =>
        void drawn(kind, page, shape, stamp),
      // The rectangle a reader dragged out to crop to. It arrives in the
      // file's *display* space, like every other gesture's, and the crop the
      // model holds is one turn further in --- so unlike the three callbacks
      // around it this one cannot go straight to an edit. See `cropTo`.
      onCropped: (page, rect) => void cropTo(page, rect),
      // Straight through, where a crop goes via `cropTo` and an IPC round
      // trip: a crop box is in the page's own unrotated space and the
      // rectangle a drag produces is not, so that one has to be converted.
      // A pending redaction is held in exactly the space handed here.
      onRedacted: (page, area) => void applyEdit((e) => e.redact(page, area)),
      onRedactSelection: () =>
        void redactSelection().then(() => asDocument(view, () => viewer?.clearSelection())),
      onMarkMoved: (id, dx, dy) =>
        isSaved(id)
          ? changeField(scannedForm && edits ? fieldMoved(scannedForm, edits.state, id, dx, dy) : null)
          : void applyEdit((e) => e.displace(id, dx, dy)),
      onSignatureResize: (id, width) => void applyEdit((e) => e.resizeSignature(id, width)),
      onMarkResized: (id, rect) =>
        isSaved(id)
          ? changeField(scannedForm && edits ? fieldPlaced(scannedForm, edits.state, id, rect) : null)
          : void applyEdit((e) => e.resize(id, rect)),
      onMarksArranged: (moves, sweep) =>
        void applyEdit((e) => arrangeBoth(scannedForm, e.state, e, moves, sweep)),
      // The Arrange commands are offered by how many are picked, and a menu
      // item's enablement is pushed, so every change is pushed too.
      onPicked: (count, more) => {
        notice = noticeAfterPick(count, more, notice);
        refreshMenu();
      },
      // The bar beside several picked marks asks the registry for each of
      // its buttons, so a button and its menu item are one command.
      onArrangeCommand: (id) => barCommand(commands, id),
      onErased: (mark, remove, sweep) =>
        void applyEdit((e) => e.erase(mark, remove, sweep)),
      // The same sweep's other half: a mark with no parts to lose goes whole.
      // `unmark` is what the mark panel's own Remove already calls, so a mark
      // taken by the nib and one taken from the list are one command and one
      // undo, however the reader asked. The same function as `onMarkRemove`
      // for that reason, and because a saved field under the nib is removed
      // as a field there and has to be here.
      onUnmarked: (mark, sweep) => removeNamed(mark, sweep),
      // **Back and Forward grey when there is nowhere to go, and this is what
      // keeps that honest.** A menu item's enablement is a *pushed* map, so a
      // guard reading state that moves outside the push sites is wrong
      // between them --- which is the trap `refreshMenu` already carries. The
      // history moves on a jump, on a step back and on a new document, and
      // none of those is an edit; the frame loop's push covers the ones that
      // also move the page, and this covers the ones that do not, including a
      // link to somewhere on the page the reader is already looking at.
      onNavigate: () => refreshMenu(),
      onStatus: (next) => {
        status = next;
        formLayer?.layout();
        textEditor?.layout();
        // Here rather than in a `$derived`, because this is the only moment
        // the coverage actually changes, and the gate wants one reading of
        // the clock per change rather than one per render.
        degraded = degradedGate.update(next, performance.now());
        // What keeps thumbnails out of the way of the page: the strip stops
        // asking, and withdraws what it asked for, whenever the viewer has
        // work outstanding. See `thumbnails.ts`.
        sidebar?.setViewerBusy(next.pending > 0);
        // Through the status rather than from the rotate command, so the
        // strip follows however the rotation was reached --- the palette, the
        // keyboard, or anything later that rotates without going via here.
        sidebar?.setTurns(next.turns);
        // Same reasoning as the rotation above: the strip follows the view
        // however the inversion was reached, rather than only via the command.
        sidebar?.setInvert(next.invert);
        // Same reasoning again, and it is the whole wiring for the results
        // tab: the panel follows the scan through the status, so it is fed
        // whether the search came from the find field, the palette, or a
        // toggle rescanning what was already there.
        if (viewer) {
          sidebar?.results.update(
            viewer.searchMatches,
            viewer.matchIndex,
            next.search.query,
            next.search.running,
            next.search.unsearchablePages,
          );
        }
        notePlace();
        // Every frame, and almost always a no-op: the guards that move
        // without an edit --- a selection appearing, a mark's note opening ---
        // have no event of their own, and `refreshMenu` pushes nothing when
        // the answers have not changed. Without this the menu bar's Highlight
        // selection is greyed at exactly the moment there is a selection,
        // because the last thing to refresh it was an edit.
        refreshMenu();
      },
      onPosition: (at, top) => {
        sidebar?.setPosition(at, top);
        // A form's controls and the text editor sit over the page, and
        // `onStatus` does not fire for a scroll within a page that brings no
        // new tile: the status has no place in it. Laid out there alone, a
        // control stayed where it was while its field moved under it.
        formLayer?.layout();
        textEditor?.layout();
        notePlace();
        keepInStep();
      },
      // Shown, for the same reason a failed print is: this fires only for a
      // command the reader typed and is waiting on --- a copy that could not
      // read every page it spans, or a clipboard that refused the write.
      onError: (message) => {
        say(message);
      },
      // `links`, not `outline`: a target from a page rectangle is numbered by
      // the links scan, and the two scans number independently --- the wrong
      // word here opens a different address rather than failing.
      onWebLink: (target) => void followWebLink("links", target),
      // The one message here nobody asked for. It fires while someone is
      // reading, because a process outside the application shortened the file
      // underneath them --- so it goes to the same surface as the errors they
      // did ask for, which is the only one this window has. The pages already
      // painted stay painted; what this adds is the reason the rest never
      // arrive.
      onGone: (message) => {
        say(message);
      },
    }, runningAs(view)));
  }

  /**
   * Puts the model's pages and marks, the reader's place and the window's
   * settings into a viewer and its sidebar that have just been built.
   *
   * `kept` is what a tab being returned to left behind, and null for a
   * document opened for the first time.
   */
  function showModel(
    shown: Viewer,
    panel: Sidebar,
    model: Edits,
    resume: Place | null,
    kept: FreshState | null,
  ): void {
    // Before the first paint, so the reader sees their page rather than page
    // one and then a jump --- and before `focus`, which does not move the view
    // but would make the jump look like something they did.
    shown.setPages(model.state.pages);
    // A kept tab can hold pages of other files, whose links were cleared with
    // the rest when the outgoing document was torn down.
    void fetchImportedLinks(model);
    shown.setTextEdits(model.state.text_edits ?? []);
    shown.setFieldEdits([], model.state.fields ?? []);
    shown.setMarks(shownMarks(model.state));
    shown.setRedactions(model.state.redactions);
    panel.thumbnails?.setPages(model.state.pages.length);
    if (resume) shown.restore(resume);
    // Only for a tab being returned to. A document opened for the first time
    // has no search to put back and opens on the tab the sidebar starts on.
    if (kept) restore(kept, "mounted", restoring);
    shown.setNib(markNib.pt);
    // After `restore`, which does not touch the colours, and before `focus`,
    // so the first tiles requested are already the right polarity rather than
    // being rendered light and immediately thrown away.
    shown.setInverted(invertPages);
    shown.focus();
    // A side that shows a different tab is a different pair of documents.
    // Nothing is locked while the other side is still to be mounted.
    if (syncScroll) lockSides();
  }

  /**
   * The controls over a form's fields, for the document mounted as `view` in
   * the viewer `mounted`.
   */
  function buildFormLayer(host: HTMLDivElement, view: ViewId, form: Form, mounted: Viewer): FormLayer {
    // A control sits where its field now is, and nowhere while the fields
    // are being changed: a press on one then picks it and does not type.
    //
    // Read as this document. The frames and the edits that lay the controls
    // out arrive as it, and a control that lays itself out from its own key
    // handler does not: the reader may be in the other side then, and the
    // variables are that document's.
    const anchored = (widget: Form["widgets"][number]) => {
      const rect = asDocument(view, () => controlRect(widget, formEditing, edits?.state ?? null)) ?? null;
      return rect ? mounted.formAnchor({ page: widget.page, display_rect: rect }) : null;
    };
    return new FormLayer(host, form, anchored,
      // For this document by name: a control commits when it loses the
      // keyboard, and the press that took it may have been in the other side.
      async (object, value) => { await applyEdit((model) => model.fill(object, value), view); },
      (widget) => mounted.showForm(widget),
      // What a control has to say, and a press on a place for a signature,
      // are this document's for the same reason.
      (message) => { asDocument(view, () => say(message)); },
      (widget) => {
        // Signing reads the focused document, and it starts a moment after
        // it is asked for. So the reader is taken to this document first,
        // and where that is refused nothing is signed.
        if (!readerIsIn(view, openView, () => focusSide(panes.sideOf(view)))) {
          asDocument(view, () => say("Wait for the other document to finish, then press the field again"));
          return;
        }
        void signDocument(edits ? signTarget(widget, edits.state.pages) : null);
      });
  }

  /**
   * Reads the document's form once its first screen is up, and builds the
   * controls over its fields.
   *
   * The form, its names and whether its fields are being changed were
   * reset when the outgoing document was torn down and are read again here:
   * these three are not in `DocumentTab` on purpose. The form and its names
   * are facts about the file, which this scan reads on every open, a tab
   * being returned to included, because the controls it builds belong to the
   * viewer and went with the last one. Changing the fields is a mode of the
   * document on screen, and it does not wait for the reader in a tab they left.
   */
  function readForm(view: ViewId, wanted: number, mounted: Viewer): void {
    void firstPaint(view).then(() => {
      if (!stillMounted(view, mounted)) return null;
      return call("document_form", { doc: wanted });
    }).then((form) => asDocument(view, () => {
      if (!form || !surface || viewer !== mounted) return;
      formNames = form.widgets.map((widget) => widget.name);
      scannedForm = form;
      formLayer = buildFormLayer(surface, view, form, mounted);
      if (edits) formLayer.update(edits.state);
      formLayer.setBusy(documentBusy);
    })).catch((error) => asDocument(view, () => { if (viewer === mounted) say(String(error)); }));
  }

  /** Reads the document's outline once its first screen is up. */
  function readOutline(view: ViewId, wanted: number, mounted: Viewer): void {
    void firstPaint(view)
      .then(() => {
        // Checked before asking as well as after. The wait is up to a second
        // and is no longer inside the open, so another document can arrive
        // during it --- and an outline walk for a file nobody is looking at is
        // not merely wasted, it is a third of a second of the FIFO render
        // thread in front of the tiles for the file they *are* looking at.
        if (!stillMounted(view, mounted)) return null;
        return call("document_outline", { doc: wanted });
      })
      .then((result) => asDocument(view, () => {
        // And again, because another document may have been opened while the
        // walk itself was in flight.
        if (!result || viewer !== mounted) return;
        rawOutline = result;
        applyPageOrder();
      }))
      .catch(() => asDocument(view, () => sidebar?.setOutline(null)));
  }

  /** Reads the comments in the document once its first screen is up. */
  function readComments(view: ViewId, wanted: number, mounted: Viewer): void {
    // The comments, on the same terms and for a different reason. They cost
    // an `lopdf` parse of the whole file --- 0.1 ms small, 11.9 ms on the
    // 337 MB scan --- rather than render-thread time, so what this waits for
    // is not the render queue but the first paint: warm startup has ~25 ms of
    // margin against its 300 ms target, and this is off that path entirely.
    // A separate chain rather than a link in the one above, so a document
    // whose outline cannot be read still gets its comments and the reverse.
    void firstPaint(view)
      .then(() => {
        if (!stillMounted(view, mounted)) return null;
        return call("document_comments", { doc: wanted });
      })
      .then((result) => asDocument(view, () => {
        if (!result || viewer !== mounted) return;
        // Both the panel that lists them and the viewer that makes the mark on
        // the page openable, and both through the translation --- see
        // `applyPageOrder`, which is also what re-runs this if a page is
        // deleted later.
        rawComments = result;
        applyPageOrder();
        // A reader already on the comments tab when the scan lands would
        // otherwise sit looking at rows reading "Highlight, no comment": the
        // tab callback fired before there was anything to fill in, and it does
        // not fire again for a tab that is already showing.
        if (sidebar?.tab === "comments") void fillCommentWords();
      }))
      .catch(() => asDocument(view, () => sidebar?.setComments(null)));
  }

  /** Reads the document's links once its first screen is up. */
  function readLinks(view: ViewId, wanted: number, mounted: Viewer): void {
    // The links, on the same terms again --- a third chain rather than a link
    // in either above, so one failing does not take the others with it.
    //
    // Where this differs from the comments: nobody opens a panel before
    // clicking a cross-reference, so waiting for demand would mean the first
    // click on any document goes nowhere. It waits for first paint for the
    // same reason they do, and for nothing else.
    void firstPaint(view)
      .then(() => {
        if (!stillMounted(view, mounted)) return null;
        return call("document_links", { doc: wanted });
      })
      .then((result) => asDocument(view, () => {
        if (!result || viewer !== mounted) return;
        rawLinks = result.items;
        applyPageOrder();
        // A cut list is worth saying out loud, for the reason every bound in
        // this application reports itself: a document whose cross-references
        // half work is worse to use than one whose links are all dead, and
        // silence makes the two indistinguishable.
        const said = linkNotice(result.limits);
        if (said) error = said;
      }))
      .catch(() => {
        // Deliberately quiet. A document with no readable links is the common
        // case --- most PDFs have none --- and there is nothing the reader
        // would do about it, so this is not the `onError` contract.
      });
  }

  /**
   * Asks for the answers about the file `wanted`, mounted as `view` in the
   * viewer `mounted`: its form, its outline, its comments and its links.
   *
   * After the viewer, deliberately not awaited, and deliberately not asked
   * for until the first screen is up.
   *
   * Not awaiting the *outline* was always right: it shares the render thread
   * with tiles and a document that opens instantly should not wait for its
   * table of contents. Waiting for the first paint before *asking* is there
   * because the walk stopped being free: resolving a destination on a page
   * carrying `/Rotate` needs the page's rotation, `FPDFPage_GetRotation`
   * needs the page loaded, and that measured 0.17 ms -> 7.5 ms on a
   * twelve-page fixture, about 1 ms per distinct page named. On a book with
   * a three-hundred-entry table of contents that is a third of a second of
   * render thread, and the render thread is FIFO --- so asked for at open it
   * would sit in front of the tiles for the page someone is looking at.
   *
   * What changed is that `openPath` is now a chain, and `firstPaint` waits
   * up to a second: awaiting it here would hold the *next* document's open
   * behind a delay that has nothing to do with it. Both halves are already
   * guarded by the viewer still being the one that was mounted, so letting the whole tail run detached
   * costs nothing --- an outline for a document nobody is looking at is
   * dropped exactly as it was before.
   */
  function readAfterFirstPaint(view: ViewId, wanted: number, mounted: Viewer): void {
    readForm(view, wanted, mounted);
    readOutline(view, wanted, mounted);
    readComments(view, wanted, mounted);
    readLinks(view, wanted, mounted);
  }

  /**
   * Takes back what an open that failed had got as far as changing.
   *
   * `acquired` is the handle the open was given, or -1 when it was a tab's
   * own; `replaced` is whether the outgoing document had been torn down, and
   * `twins` the other views that went with it.
   *
   * What may be cleared depends on how far the open got, and the two cases
   * are opposites. A failure *before* the outgoing document was torn down ---
   * an `open_document` that threw, which is the common one --- has touched
   * nothing: the reader still has their document on screen, and clearing
   * `title` there unmounts the body out from under a live viewer and sidebar
   * while the backend still holds the file. A failure *after* it has no
   * document left to keep, and leaving the singletons set would advertise one
   * that is gone.
   */
  function abandonOpen(acquired: number, replaced: boolean, twins: readonly DocumentTab[]): void {
    // Which views go, and that the row is drawn after the last of them has
    // gone, is `dropAbandoned`'s. On every path, the ones that drop nothing
    // included: a row drawn once too often is the same row.
    dropAbandoned(panes, tabs,
      { own: acquired >= 0 ? viewOf(acquired) : undefined, replaced, twins }, refreshTabs);
    if (acquired >= 0) void call("close_document", { doc: acquired }).catch(console.warn);
    if (replaced) {
      // Whatever half-built state got as far as existing. A viewer left alive
      // while `title` is empty runs its frame loop against a detached surface
      // and keeps writing `status`, which the header renders --- a page count
      // and a zoom for a document with no body under them.
      //
      // `title` and `status` go together, always: `title` gates the body
      // and `status` feeds the header, so one outliving the other is a header
      // describing a document that is no longer on screen. The degraded
      // label's clock goes with them, because a stale one would show the
      // next document's first blurry frame at once.
      unmountDocument();
      // With no document left the page areas go too, so the window shows
      // the start page and not two empty areas. `showPanes` sets this when
      // it mounts something, and a failed open with nothing to go back to
      // never gets there.
      bodyHeld = stage.parked.length > 0;
    }
  }

  /**
   * Opens a document, putting the reader back where they left it.
   *
   * Never called directly --- {@link openPath} is the entry point, and going
   * around it reintroduces the interleaving described there.
   *
   * `resuming` is set only by the launch restore, and changes one thing: a
   * document that no longer opens is not an error to report. Someone who chose
   * a file and cannot have it needs to be told; someone who launched the app and
   * whose last document has since been deleted or unmounted needs an empty
   * window, not a dialog about a file they did not ask for.
   *
   * A sequence of steps, and the ones that are functions of this file
   * ({@link buildSidebar}, {@link adoptModel}, {@link buildViewer},
   * {@link showModel}) are called from here and nowhere else: they read the
   * document's variables after this function's waits, which is right under
   * `opening` and only there. What the open decides on the way is in
   * `documentopen.ts`.
   *
   * Answers what a failed open told the reader, and null for an open that
   * worked or failed without a word. The message is in the variables too, and
   * a caller that mounts another tab next replaces it there.
   */
  async function openDocument(
    path: string,
    resuming = false,
    override: Place | null = null,
    retained?: DocumentTab,
  ): Promise<string | null> {
    let failed: string | null = null;
    opening = true;
    /** Whether this body has already torn the outgoing document down: see {@link abandonOpen}. */
    let replaced = false;
    let acquired = -1;
    let twins: DocumentTab[] = [];
    try {
      const doc = retained?.doc ?? await openOrAsk(path);
      acquired = retained ? -1 : doc.id;
      await settleDocument();
      keepActiveTab();
      // A save or a reload opens the file again in the tab it is shown in.
      // The tab the reader is in when the file is shown in two.
      const replacing = override ? tabs.forPath(path, tabs.active) : undefined;
      // Its other views are on the handle that is going. They are torn down
      // with it, and whoever asked for this open mounts them again.
      twins = replacing ? twinsOf(tabs.all, replacing) : [];
      const replaceId = replacing?.doc.id;
      // The first view of a document is named by its handle, and a tab that
      // is returned to keeps the name it has.
      const view = retained?.view ?? viewOf(doc.id);
      const pages = pageTable(doc);

      // Whatever the outgoing document was owed, before its path is replaced.
      places.flush();
      for (const twin of twins) {
        asDocument(twin.view, () => { commitPopups(); keepActiveTab(); unmountDocument(); });
      }
      replaced = true;
      // Everything about the outgoing document goes here, the answers about
      // its file and the lookups keyed by its ids included: a page number or a
      // mark id kept would be read as the next file's.
      unmountDocument();
      openDoc = doc.id;
      openView = view;
      title = basename(path);
      openPathName = path;
      openPageCount = doc.page_count;
      startPage.opened(path);

      const resume = placeToResume(retained?.place, override, session.places, path,
        retained?.edits.state.pages.length ?? doc.page_count);
      sidebarShown = resume ? resume.sidebar : sidebarShown;

      // The host element does not exist until the viewer section is in the
      // DOM, and it is not while the empty-state placeholder is showing.
      await new Promise(requestAnimationFrame);
      const slot = panes.slotOf(panes.focused);
      const area = areaHosts[slot] ?? null;
      if (!area || !sidebarHost) throw new Error("no surface to mount into");
      surface = area;

      // One record for every restore below, fresh for a document never kept,
      // and applied through `restoring` at each of the three points.
      const kept = restoredState(retained);
      restore(kept, "unmounted", restoring);
      closeDocumentPanels();
      sidebar = buildSidebar(sidebarHost, view, doc, pages[0]);
      sidebar.setVisible(sidebarShown);

      const opening = adoptModel(view, doc, path, { retained, replacing, replaceId, twins, resume, kept });
      viewer = buildViewer(area, view, doc, pages);
      panes.mounted(view, slot);
      showModel(viewer, sidebar, opening, resume, retained ? kept : null);
      readAfterFirstPaint(view, doc.id, viewer);
    } catch (e) {
      abandonOpen(acquired, replaced, twins);
      const said = openFailure(e, resuming);
      if (said !== null) error = said;
      failed = said;
      // Whether or not it is reported: a row for a document that would not
      // open says so from here on, and keeps its control for removing it.
      startPage.failed(path, e);
    } finally {
      opening = false;
      // In the `finally`, so that a failed open greys the menu back out. Every
      // command but four is withheld without a document, and an open that threw
      // leaves `viewer` null with the menu still saying otherwise.
      refreshMenu();
    }
    return failed;
  }

  /**
   * What the surface is doing, when it is not simply showing the document.
   *
   * The classification and the delay in front of it both live in
   * `degraded.ts` --- docs/PLAN.md section 9 for why the state is owed at all,
   * and that module's own comment for why it is not shown the instant it
   * becomes true.
   */
</script>

<svelte:window onkeydown={onWindowKey} onkeydowncapture={closeZoomOnEscape} onpointerdown={closeZoomOutside} />

<main>
  <header>
    <button title="Open a PDF" aria-label="Open a PDF" onclick={pickAndOpen} disabled={opening || copyTaskBusy || documentBusy}><span class="icon" use:icon={"open"}></span></button>
    {#if title}
      <button class="sidebar-toggle" aria-pressed={sidebarShown} title="Toggle sidebar" aria-label="Toggle sidebar" onclick={toggleSidebar}><span class="icon" use:icon={"sidebar"}></span></button>
      <button title={toolState['file.save']?.title} aria-label="Save" disabled={!toolState['file.save']?.enabled}
        onclick={() => runToolbarCommand('file.save')}><span class="icon" use:icon={"save"}></span></button>
      <!-- Save a copy is in the Document menu, one row down, and no longer here too. -->
      <button class="secondary-file" title={toolState['file.print']?.title} aria-label="Print"
        disabled={!toolState['file.print']?.enabled}
        onclick={() => runToolbarCommand('file.print')}><span class="icon" use:icon={"print"}></span></button>
    {/if}
    <span class="document-name">
      {#if !title}<span class="title">tpdf</span>{/if}
      {#if dirty}<span class="edited">Edited</span>{/if}
      {#if degraded && status}<span class="degraded">{degraded}</span>{/if}
      {#if blockingTask}<span class="notice" data-testid="blocking-task">{blockingTask}</span>
        {#if recognising}<button data-testid="stop-recognition" title="Stop recognising text"
          onclick={() => void call("ocr_cancel", { run: recognitionRun })}>Stop</button>{/if}
        {#if findingHidden}<button data-testid="stop-hidden-text" title="Stop comparing text with the pages"
          onclick={() => hiddenRuns.stop((run) => void call("hidden_text_cancel", { run }))}>Stop</button>{/if}
      {:else if notice}<span class="notice" data-testid="notice">{notice}</span>{/if}
    </span>
    {#if status}
      <div class="navigation" aria-label="Page navigation">
        <button aria-label="Previous page" title={toolState['nav.previousPage']?.title} disabled={!toolState['nav.previousPage']?.enabled}
          onclick={() => runToolbarCommand('nav.previousPage')}><span class="icon" use:icon={"previous"}></span></button>
        <button title="Go to page" onclick={() => runToolbarCommand('nav.goToPage')}>
          {status.page} / {status.pageCount}</button>
        <button aria-label="Next page" title={toolState['nav.nextPage']?.title} disabled={!toolState['nav.nextPage']?.enabled}
          onclick={() => runToolbarCommand('nav.nextPage')}><span class="icon" use:icon={"next"}></span></button>
      </div>
      <details class="zoom-menu" bind:this={zoomMenu}>
        <summary title={describeFit(status.fit)}>{percentOf(status.zoom)}%</summary>
        <div class="zoom-options">
          {#each ['view.fitWidth', 'view.fitPage', 'view.actualSize', 'view.zoomIn', 'view.zoomOut', 'view.zoomTo'] as id}
            <button disabled={!toolState[id]?.enabled} onclick={(event) => {
              event.currentTarget.closest('details')?.removeAttribute('open');
              runToolbarCommand(id);
            }}>{commands.find(id)?.title}</button>
          {/each}
        </div>
      </details>
      <button aria-pressed={findShown} title="Find in document" aria-label="Find in document" onclick={focusFind}><span class="icon" use:icon={"find"}></span></button>
    {/if}
    <button title="All commands ({label('app.palette')})" onclick={openPaletteFromToolbar}>Commands</button>
    {#if updateLabel(updateState)}
      <button
        class="update"
        class:ready={updateState.kind === "ready"}
        disabled={updates.busy}
        title={updateState.kind === "ready"
          ? "Restart tpdf now to finish installing the update"
          : "Download and apply this update"}
        onclick={() => void finishUpdateStep(stepFor(updateState))}>{updateLabel(updateState)}</button
      >
    {/if}
  </header>
  {#if title}
    <Toolbar state={toolState}
      active={status?.armed ? armedLabel(status.armed) : null} armed={status?.armed ?? null}
      drawing={status?.drawing ?? null} erasing={status?.erasing != null}
      colorId={markColor.id} widthLabel={markNib.name}
      selected={status?.selected ?? 0} run={runToolbarCommand}
      finish={finishToolbarDrawing} cancel={cancelToolbarTool} />
  {/if}
    {#if title && findShown}
      <section class="find-panel" aria-label="Find in document">
      <input
        aria-label="Find in document"
        class="find"
        type="search"
        placeholder="Find"
        bind:value={query}
        bind:this={findField}
        oninput={onFindInput}
        onkeydown={onFindKey}
      />
      <button
        class="toggle"
        class:on={status?.search.options.matchCase}
        aria-pressed={status?.search.options.matchCase ?? false}
        title="Match case ({label('find.matchCase')})"
        onclick={() => toggleSearchOptionFromToolbar("matchCase")}>Aa</button
      >
      <button
        class="toggle"
        class:on={status?.search.options.wholeWord}
        aria-pressed={status?.search.options.wholeWord ?? false}
        title="Whole words ({label('find.wholeWord')})"
        onclick={() => toggleSearchOptionFromToolbar("wholeWord")}>|ab|</button
      >
      <button
        class="toggle"
        class:on={status?.search.options.regex}
        aria-pressed={status?.search.options.regex ?? false}
        title="Regular expression ({label('find.regex')})"
        onclick={() => toggleSearchOptionFromToolbar("regex")}>.*</button
      >
      <button
        class="toggle"
        class:on={status?.search.scoped}
        aria-pressed={status?.search.scoped ?? false}
        disabled={!status?.search.scoped && (status?.selected ?? 0) === 0}
        title="Search the selection ({label('find.inSelection')})"
        onclick={() => {
          toggleSearchScope();
          findField?.focus();
        }}>[ab]</button
      >
      {#if findLabel}<span class="stat" class:problem={status?.search.problem}
          >{findLabel}</span
        >{/if}
      <button title="Previous match" onclick={() => viewer?.prevMatch()}>Previous</button>
      <button title="Next match" onclick={() => viewer?.nextMatch()}>Next</button>
      <button onclick={() => { findShown = false; viewer?.focus(); }}>Close</button>
      </section>
    {/if}


  {#if error}
    <div class="problem-bar" data-testid="problem">
      {#if redactedCopyPath}
        <p>Saved {basename(redactedCopyPath)}. You are still viewing the original with its pending redaction marks.</p>
        <button data-testid="open-redacted-copy" disabled={copyTaskBusy}
          onclick={async () => {
            const path = redactedCopyPath;
            const verdict = error;
            if (!path) return;
            await openPath(path);
            if (openPathName === path) say(verdict);
          }}>Open saved copy</button>
      {/if}
      {#if offers.includes("rasterCopy")}
        <p class="error">Redaction not verified. Create an image-only copy to remove everything inside the marked regions.</p>
        <details><summary>Technical details</summary><p class="error">{error}</p></details>
      {:else}
        <p class="error">{error}</p>
      {/if}
      <!--
        The buttons a message carries, and never more than the message earns:
        `recovery.ts` decides which appear, because a Reload offered beside the
        wrong message discards the reader's work for a reason that has gone.
        Rendered from the list rather than written twice, so the order the rules
        return is the order they appear in -- Save a copy leads, and that is not
        cosmetic, since the one beside it is the one that spends the journal.

        **Every variant has its own arm and there is no `{:else}`**, which is a
        decision rather than a style. Until 2026-08-27 `saveCopy` was matched and
        an `{:else}` drew Reload for everything else -- correct while there were
        two, and one new variant away from putting a button that discards the
        reader's work under a prompt about destroying their file. With no
        catch-all a variant nobody wired here draws nothing, which is a prompt
        with no button: visible, harmless, and the direction to fail in.
        `recovery.ts`'s `Offer` says the same thing from the other end, and
        `recovery.test.ts` pins the set these rules can return -- nothing here is
        reachable from a unit test, so that is where a new variant goes red.
      -->
      {#if offers.length > 0}
        <div class="offers">
          {#each offers as offer (offer)}
            {#if offer === "saveCopy"}
              <button data-testid="offer-saveCopy" disabled={copyTaskBusy}
                onclick={() => void saveCopy()}
                >Save a copy…</button
              >
            {:else if offer === "reload"}
              <button data-testid="offer-reload" disabled={copyTaskBusy}
                onclick={() => reloadAnyway()}
                >Reload from disk</button
              >
            {:else if offer === "rasterCopy"}
              <button data-testid="offer-rasterCopy" disabled={copyTaskBusy}
                onclick={() => void redactRasterCopy()}
                >Create image-only copy...</button>
            {:else if offer === "redact"}
              <button data-testid="offer-redact" disabled={copyTaskBusy}
                onclick={() => void redactAnyway()}
                >Redact this file</button
              >
            {/if}
          {/each}
        </div>
      {/if}
      <!-- Takes the message away and decides nothing: it runs no offer, and
           Reload from disk stays in the Document menu and the palette. -->
      <button class="dismiss" data-testid="problem-dismiss" title="Dismiss"
        aria-label="Dismiss this message"
        onclick={() => { say(null); viewer?.focus(); }}
        ><span class="icon" use:icon={"close"}></span></button>
    </div>
  {/if}

  <!-- One row of tabs. With one side it is every tab; with two, each side
       draws its own above its pages. `front` is the tab that row shows. -->
  {#snippet tabStrip(rows: typeof tabRows, front: number, label: string)}
    <div class="document-tabs" role="tablist" aria-label={label}
      style:--tab-label-size={`${tabLabelPx}px`}
      onwheel={(event) => { event.currentTarget.scrollLeft += sidewaysBy(event, event.currentTarget.clientWidth); }}>
      {#each rows as tab (tab.id)}
        <!-- The middle button's mousedown would start autoscroll on Windows. -->
        <div class="document-tab" class:active={tab.id === front} class:carried={tabCarried?.id === tab.id}
          role="presentation"
          onmousedown={(event) => { if (event.button === 1) event.preventDefault(); }}
          onauxclick={(event) => tabAuxClick(event, tab.id)}>
          <button id={`document-tab-${tab.id}`} role="tab"
            aria-selected={tab.id === front} aria-controls="document-panel"
            tabindex={tab.id === front ? 0 : -1}
            title={tab.path} disabled={opening || documentBusy}
            oncontextmenu={(event) => tabContextMenu(event, tab.id)}
            onpointerdown={(event) => tabPress(event, tab.id)}
            onclick={() => { if (!tabDragEnded) void activateTab(tab.id); }} onkeydown={(event) => tabKey(event, tab.id)}>
            <span class="tab-name">{tabLabelOf.get(tab.id)}</span>
            {#if tab.id === activeTab ? dirty : tab.dirty}<span aria-label="Unsaved changes">*</span>{/if}
          </button>
          <button class="tab-close" title={`Close ${tabLabelOf.get(tab.id)}`}
            aria-label={`Close ${tabLabelOf.get(tab.id)}`} disabled={opening || documentBusy}
            onclick={() => void closeTab(tab.id)}><span class="icon" use:icon={"close"}></span></button>
        </div>
      {/each}
      <button class="tab-open" title="Open PDFs in new tabs" aria-label="Open PDFs in new tabs"
        disabled={opening || documentBusy} onclick={pickAndOpen}><span class="icon" use:icon={"add"}></span></button>
    </div>
  {/snippet}
  {#if tabRows.length && !paneLayout.split}
    {@render tabStrip(tabRows, activeTab, "Open documents")}
  {/if}
  {#if title || bodyHeld}
    <div class="body" id="document-panel" role="tabpanel" aria-labelledby={`document-tab-${activeTab}`}>

      <div class="panel" bind:this={sidebarHost}></div>
      <!-- Both page areas exist for as long as any document is open, used
           or not: a viewer is mounted into one and stays there, so an area
           made and removed with the split would take its viewer with it. -->
      <div class="panes" bind:this={panesHost}>
        {#each [0, 1] as slot (slot)}
          <!-- A press or the keyboard arriving in a side makes it the one the
               reader works in, before anything inside it handles the event. -->
          <div class="pane" class:unused={paneLayout.unused[slot]}
            class:focused={paneLayout.split && paneLayout.focused === slot}
            data-side={paneLayout.order[slot] === 0 ? "left" : "right"}
            style:order={paneLayout.order[slot]}
            style:flex-grow={paneLayout.split ? (paneLayout.order[slot] === 0 ? paneShare : 1 - paneShare) : 1}
            role="presentation"
            onwheelcapture={(event) => { if (event.altKey) aloneUntil = performance.now() + 250; }}
            onpointerdowncapture={() => { focusSide(panes.sideIn(slot as Slot)); }}
            onfocusin={() => { focusSide(panes.sideIn(slot as Slot)); }}>
            {#if paneLayout.split}
              {@render tabStrip(tabRows.filter((tab) => tab.slot === slot), paneLayout.front[slot] ?? -1,
                paneLayout.order[slot] === 0 ? "Documents on the left side" : "Documents on the right side")}
            {/if}
            <div class="surface" bind:this={areaHosts[slot]}></div>
          </div>
        {/each}
        {#if paneLayout.split}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div class="pane-divider" role="separator" aria-orientation="vertical"
            aria-label="Divider between the two sides" title="Drag to resize. Double-click for equal halves."
            aria-valuemin="20" aria-valuemax="80" aria-valuenow={Math.round(paneShare * 100)}
            onpointerdown={dividerDown} onpointermove={dividerMove}
            onpointerup={refreshTabs} onpointercancel={refreshTabs}
            ondblclick={() => { paneShare = 0.5; refreshTabs(); }}>
            <!-- On the divider because it is about both sides and belongs to
                 neither. A press on it is not the start of a resize. -->
            <button class="sync-scroll" class:on={syncScroll} aria-pressed={syncScroll}
              data-testid="sync-scroll"
              title={syncScroll
                ? "The two sides scroll together. Hold Alt to scroll one side alone."
                : "Scroll both sides together"}
              aria-label="Scroll both sides together"
              onpointerdown={(event) => event.stopPropagation()}
              ondblclick={(event) => event.stopPropagation()}
              onclick={() => setSyncScroll(!syncScroll)}><span></span><span></span></button>
          </div>
        {/if}
        {#if tabCarried?.side}
          {@const edge = (paneLayout.split ? paneShare : 0.5) * 100}
          <div class="drop-side" data-testid="drop-side" data-side={tabCarried.side}
            style:left={tabCarried.side === "left" ? "0" : `${edge}%`}
            style:width={`${tabCarried.side === "left" ? edge : 100 - edge}%`}></div>
        {/if}
      </div>
    </div>
  {:else}
    <!--
      The blank page. Laid out from the top rather than centred, so that the
      rows arriving a moment after the window does push nothing: the sentence
      and the button are above them, and the version is pinned to the bottom.
      What the rows are and what each control runs is `startpage.ts`'s.
    -->
    <div class="empty">
      <div class="start" role="presentation" bind:this={startHost}
        onkeydown={(event) => startPageKey(event, startRowOf(event))}>
      <p class="invite">Open a PDF, or drop one here.</p>
      <button class="start-open" disabled={opening || copyTaskBusy || documentBusy}
        onclick={() => runStartCommand("file.open")}
        >Open a PDF… <span class="start-keys">{label("file.open")}</span></button>
      {#if startRows.length}
        <ul class="recent" aria-label="Recently opened">
          {#each startRows as row, index (row.path)}
            <li class:unopened={row.trouble !== ""} data-start-row={index}>
              <button class="recent-open" title={row.path}
                onclick={() => runStartCommand(row.command)}>
                <span class="recent-name">{row.label}</span>
                <span class="recent-where"><span class="recent-folder">{row.folder}</span>{#if row.page}<span class="recent-page">{row.page}</span>{/if}{#if row.trouble}<span class="recent-trouble">{row.trouble}</span>{/if}</span>
              </button>
              <button class="recent-remove" title={row.removing} aria-label={row.removing}
                onclick={() => void forgetRecent(row, index)}><span class="icon" use:icon={"close"}></span></button>
            </li>
          {/each}
        </ul>
      {/if}
      </div>
      <!--
        The version, where there is room for it and nothing to cover. A reader
        asking "which one am I on" is usually asking because something is wrong,
        and an empty window is the state they are most often in when they ask.
        The palette's "About tpdf" answers the same question with a document
        open; this costs no chrome at all.
      -->
      {#if appVersion}<p class="version" data-testid="version">tpdf {appVersion}</p>{/if}
    </div>
  {/if}
</main>

<style>
  .document-tabs { display:flex; flex-shrink:0; overflow-x:auto; gap:3px; padding:4px 8px 0; border-bottom:1px solid color-mix(in srgb, CanvasText 20%, transparent); }
  /* No bar under the tabs: `tabwheel.ts` says why, and what a mouse wheel does instead. */
  .document-tabs { scrollbar-width:none; }
  .document-tabs::-webkit-scrollbar { display:none; }
  .document-tab { display:flex; min-width:100px; max-width:240px; flex-shrink:0; border:1px solid transparent; border-radius:6px 6px 0 0; font-size:var(--tab-label-size); }
  .document-tab button { min-height:0; }
  .document-tab.active { background:color-mix(in srgb, Highlight 12%, Canvas); border-color:color-mix(in srgb, Highlight 50%, Canvas); border-bottom:2px solid Highlight; }
  .document-tab button { border:0; background:transparent; color:inherit; border-radius:4px; }
  .document-tab [role="tab"] { display:flex; flex:1 1 auto; align-items:center; gap:6px; min-width:0; padding:0.4em 0.8em; }
  .tab-name { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .document-tab [aria-label="Unsaved changes"] { flex:none; }
  .document-tab .tab-close { display:flex; align-items:center; padding:0.25em 0.4em; margin:3px; }
  .document-tab .tab-close:hover { background:color-mix(in srgb, CanvasText 14%, transparent); }
  .tab-open { align-self:center; flex-shrink:0; }

  :global(body) {
    margin: 0;
    background: Canvas;
    color: CanvasText;
    color-scheme: light dark;
  }
  /* The area around the page. Not derivable from `Canvas` by a single formula:
     it has to be *darker* than the paper in a light window and darker again in
     a dark one, where any symmetric mix of Canvas and CanvasText goes the wrong
     way and lights it up. Two literals, one per theme. */
  :global(:root) {
    --tpdf-surround: #666;
    /* The colour of a message that reports a failure. One literal per theme for
       the surround's reason: the light one is 3.4 to 1 on a dark window, which
       is under what small text needs. */
    --tpdf-problem: #c0392b;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --tpdf-surround: #2b2b2b;
      --tpdf-problem: #ff8a7a;
    }
  }
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    font: 13px/1.5 system-ui, -apple-system, sans-serif;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.4rem 0.7rem;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    flex: none;
  }
  /* The chrome is not a document, and the web view treats it as one by default.
     Dragging across the toolbar highlighted the button faces, and ⌘A with the
     keyboard anywhere outside the page selected every label in the bar together
     with the find field's contents --- reported from use as "the app behaves
     like a browser", which is exactly what it was doing. `appcommands.ts` holds
     the other half: ⌘A now reaches the page's own selection instead of falling
     through to the web view's select-all.

     The page is untouched by this. Its selection is drawn on a canvas overlay
     and copied out of extracted text, so it is not the web view's selection at
     all and cannot be widened by a rule here.

     Prefixed as well as not: unprefixed `user-select` is Safari 17.4 and later,
     and WKWebView's version follows the OS. A macOS old enough to want the
     prefix is not one this can be tested on from here, and the prefix costs a
     line. */
  header,
  .find-panel,
  .panel,
  .empty {
    -webkit-user-select: none;
    user-select: none;
  }
  /* The one element in the chrome whose text a reader edits. Not belt and
     braces: a field under a `user-select: none` ancestor cannot have its own
     contents selected with the mouse, so without this the fix would take
     double-click-to-select-a-word out of the find bar. */
  .find {
    -webkit-user-select: text;
    user-select: text;
  }
  button {
    font: inherit;
    padding: 0.3rem 0.6rem;
    min-height: 34px;
    color: inherit;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 5px;
    white-space: nowrap;
    flex: none;
    /* Centres a button whose whole face is an icon; one holding text is
       unchanged by it. */
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }
  button:hover:not(:disabled), summary:hover {
    background: color-mix(in srgb, CanvasText 8%, Canvas);
  }
  button:disabled { opacity: 0.4; }
  button:focus-visible, summary:focus-visible {
    outline: 2px solid Highlight;
    outline-offset: 1px;
  }
  button[aria-pressed="true"] {
    background: color-mix(in srgb, Highlight 16%, Canvas);
  }
  .document-name {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    min-width: 0;
    padding-inline: 0.7rem;
    overflow: hidden;
    white-space: nowrap;
  }
  .navigation { display: flex; align-items: center; flex: none; }
  .zoom-menu { position: relative; flex: none; }
  summary { cursor: pointer; padding: 0.3rem 0.5rem; border-radius: 5px; }
  .zoom-options {
    position: absolute;
    right: 0;
    top: 100%;
    z-index: 50;
    min-width: 170px;
    padding: 5px;
    display: grid;
    background: Canvas;
    border: 1px solid color-mix(in srgb, CanvasText 20%, Canvas);
    border-radius: 6px;
    box-shadow: 0 5px 20px #0002;
  }
  .zoom-options button { text-align: left; }
  .find-panel {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0.35rem 0.7rem;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    flex: none;
  }
  @media (max-width: 900px) {
    .secondary-file { display: none; }
  }
  @media (max-width: 600px) {
    .sidebar-toggle, .navigation { display: none; }
    .document-name { padding-inline: 0.15rem; }
    header { padding-inline: 0.3rem; gap: 0; }
    header button { padding-inline: 0.4rem; }
  }
  /* Flex items shrink by default, so without a shrink discipline here the
     header has no fixed shape: whichever element appears last steals width from
     whatever happens to be beside it. The title is the one thing that may give
     way, because it is the only item a reader can still identify from half of
     it --- a search field at 6ch and a button reading "A" cannot be used. */
  .title {
    font-weight: 600;
    min-width: 3ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .find {
    font: inherit;
    width: 14ch;
    padding: 0.15rem 0.5rem;
    flex: none;
  }
  .toggle {
    font: inherit;
    font-size: 0.85em;
    padding: 0.1rem 0.35rem;
    opacity: 0.6;
    flex: none;
  }
  .toggle.on {
    /* The pressed state has to survive both themes and both platforms' native
       button chrome, so it is drawn rather than left to `:active`-like colours
       that a dark appearance inverts out of existence. */
    opacity: 1;
    font-weight: 700;
    box-shadow: inset 0 0 0 2px currentColor;
  }
  .stat,
  .degraded {
    font-variant-numeric: tabular-nums;
    opacity: 0.65;
  }
  /* Truncates rather than wraps or pushes: it is the least important thing in
     the bar, and the one item here whose text changes while nobody has touched
     anything. `tabular-nums` above keeps the percentage from jittering as the
     digits change; this keeps a narrow window from turning it into a shove. */
  .degraded {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .edited {
    flex: none;
    opacity: 0.65;
  }
  .stat {
    flex: none;
  }
  /* Drawn as a real button, unlike the zoom readout, because this one asks the
     reader to decide something rather than reporting a number they can also
     change. `flex: none` for the same reason as the toolbar: it appears by
     itself, and nothing that appears by itself may be squeezed. */
  .update {
    flex: none;
    font: inherit;
    font-size: 0.9em;
    padding: 0.1rem 0.5rem;
    border-radius: 4px;
  }
  .update.ready {
    font-weight: 600;
  }
  .update:disabled {
    opacity: 0.65;
  }
  /* A pattern that did not compile is not a quieter version of a count. It sits
     where the counter sits, because that is where a reader is already looking,
     and at full strength because it is the only thing in the bar that is asking
     to be fixed. */
  .stat.problem {
    opacity: 1;
    color: color-mix(in srgb, currentColor 40%, var(--tpdf-problem));
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .panel {
    /* `Sidebar` creates the real panel and owns its width and visibility, so
       the host must not be a box of its own --- an empty one would reserve
       space while the sidebar is hidden. */
    display: contents;
  }
  .panes {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
  }
  .pane {
    flex: 1 1 0;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pane.unused { display: none; }
  /* Where a dragged tab lands if it is let go now. Over the pages and under
     the pointer, which it does not take: the drag is followed on the window. */
  .panes { position: relative; }
  .drop-side {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 20;
    pointer-events: none;
    background: color-mix(in srgb, Highlight 22%, transparent);
    outline: 2px solid Highlight;
    outline-offset: -2px;
  }
  .document-tab.carried { opacity: 0.55; }
  /* The side the reader is working in, said on its row of tabs: the toolbar
     and the sidebar act on it, and nothing else on screen says which it is. */
  .pane.focused :global(.document-tabs) { box-shadow: inset 0 2px 0 Highlight; }
  .pane-divider {
    order: 1;
    flex: none;
    width: 5px;
    cursor: col-resize;
    background: color-mix(in srgb, currentColor 15%, transparent);
    touch-action: none;
  }
  .pane-divider:hover { background: color-mix(in srgb, currentColor 35%, transparent); }
  .pane-divider { position: relative; }
  /* Two bars, apart when the sides scroll separately and joined when they
     scroll together. Wider than the divider it sits on, so it can be pressed. */
  .sync-scroll {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 5;
    width: 22px;
    height: 26px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 5px;
    background: Canvas;
    color: CanvasText;
    cursor: pointer;
  }
  .sync-scroll span { width: 2px; height: 14px; border-radius: 1px; background: currentColor; }
  .sync-scroll.on { gap: 0; background: Highlight; color: HighlightText; border-color: Highlight; }
  .sync-scroll.on span { width: 3px; }
  .surface {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .empty {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  /* A quarter of the way down, which is where the sentence sat when it was one
     of two centred lines; from the top, so nothing above the rows moves when
     they arrive. */
  .empty::before { content: ""; flex: 0 1 22%; }
  .start {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: min(520px, calc(100% - 2rem));
    flex: none;
  }
  .invite, .version { margin: 0; opacity: 0.5; }
  .version { margin-top: auto; padding: 1.2rem 0 0.9rem; flex: none; }
  .start-open {
    margin-top: 0.9rem;
    padding: 0.4rem 0.9rem;
    border-color: color-mix(in srgb, CanvasText 25%, transparent);
  }
  .start-keys, .recent-where { opacity: 0.6; }
  .recent {
    list-style: none;
    margin: 1.4rem 0 0;
    padding: 0;
    align-self: stretch;
  }
  .recent li { display: flex; align-items: center; gap: 2px; border-radius: 6px; }
  .recent li:hover, .recent li:focus-within { background: color-mix(in srgb, CanvasText 6%, Canvas); }
  .recent-open {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 44px;
    padding: 0.35rem 0.7rem;
    flex-direction: column;
    align-items: stretch;
    justify-content: center;
    gap: 0;
    text-align: left;
  }
  .recent-open:hover:not(:disabled) { background: transparent; }
  .recent-name, .recent-folder {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The folder gives way and the page and the trouble beside it do not: in one
     clipped line a long folder took "page 5" out with it. `startpage.ts` cuts
     a long folder at its start, so this clips only in a narrow window. */
  .recent-where { display: flex; min-width: 0; white-space: nowrap; }
  .recent-folder { flex: 0 1 auto; }
  .recent-page, .recent-trouble { flex: none; }
  .recent-name { font-weight: 600; }
  .recent-where { font-size: 0.92em; }
  .recent-page::before, .recent-trouble::before { content: " · "; }
  .unopened .recent-name { font-weight: 400; opacity: 0.6; }
  .recent-trouble { color: var(--tpdf-problem); }
  /* There for a pointer over the row and for the keyboard inside it, and
     otherwise out of the way. Hidden by opacity rather than `display`, so it
     keeps its place in the tab order and the row does not change width. */
  .recent-remove { opacity: 0; margin-right: 4px; padding: 0.25rem 0.4rem; }
  .recent li:hover .recent-remove, .recent li:focus-within .recent-remove { opacity: 0.75; }
  .recent li .recent-remove:hover, .recent li .recent-remove:focus-visible { opacity: 1; }
  /* The bar a message and its buttons share, laid out as the find bar is: one
     row, the buttons straight after the message they answer, wrapping under
     it in a narrow window. The tint and the edge say it is a message;
     the text is the window's own colour and face, because most of what appears
     here is a sentence to read and not a failure to decode. */
  .problem-bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.35rem 0.7rem;
    padding: 0.35rem 0.7rem;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    border-left: 3px solid var(--tpdf-problem);
    background: color-mix(in srgb, var(--tpdf-problem) 9%, Canvas);
    flex: none;
  }
  .problem-bar p { margin: 0; }
  /* Opens on a row of its own, under the message and the buttons. */
  .problem-bar details { flex: 1 1 100%; order: 1; }
  /* A button here is the answer to a question, so it is drawn as one. The
     chrome's buttons have no edge because a toolbar gives them their place;
     alone under a sentence, a borderless one read as a second line of text. */
  .problem-bar button {
    background: Canvas;
    border-color: color-mix(in srgb, CanvasText 30%, Canvas);
  }
  .offers { display: flex; flex-wrap: wrap; gap: 0.35rem; }
  /* At the far end and without an edge, as a tab's close is: it is not one of
     the answers, and drawn like them it would read as one. */
  .problem-bar .dismiss {
    margin-left: auto;
    padding: 0.25em 0.4em;
    background: transparent;
    border-color: transparent;
  }
  .error {
    flex: 0 1 auto;
    min-width: 0;
    white-space: pre-wrap;
  }
  .error, :global([role="alert"]) {
    -webkit-user-select: text;
    user-select: text;
    cursor: text;
  }
</style>
