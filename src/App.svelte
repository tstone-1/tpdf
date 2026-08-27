<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/autobench";
  import { runScrollBenchIfRequested } from "./lib/scrollbench";
  import { runStartupTimelineIfRequested } from "./lib/startup";
  import { runViewerCheckIfRequested } from "./lib/viewercheck";
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
  import { Edits, type EditState } from "./lib/edits";
  import {
    DEFAULT_SWATCH,
    swatch,
    type Swatch,
  } from "./lib/markcolors";
  import {
    afterCopy,
    afterRedaction,
    afterSplit,
    afterMerge,
    afterFailedSave,
    beforeReload,
    beforeRedactingInPlace,
    type Offer,
  } from "./lib/recovery";
  import { releaseOrphans } from "./lib/orphans";
  import type { DocumentInfo, PageSize } from "./lib/ipc";
  import { isOpenRefusal } from "./lib/ipc";
  import { openWithPassword } from "./lib/unlock";
  import { label, setPrintedKeys } from "./lib/keys";
  import { buildMenu, menuEnablement, runMenuCommand } from "./lib/menubar";
  import { namePages } from "./lib/pageranges";
  import { Palette } from "./lib/palette";
  import { PropertiesDialog } from "./lib/propertiesdialog";
  import { PasswordDialog } from "./lib/passworddialog";
  import type { Properties } from "./lib/properties";
  import { basename } from "./lib/paths";
  import { Sidebar, type Tab } from "./lib/sidebar";
  import {
    pagesNeedingWords,
    wantingWordsOn,
    wordsForPage,
    type Comments,
  } from "./lib/comments";
  import { touchedText } from "./lib/reading";
  import { noticeFor as linkNotice, type Link, type Links } from "./lib/links";
  import type { Outline } from "./lib/outline";
  import {
    commentsIn,
    linksIn,
    markRows,
    NO_PAGES,
    outlineIn,
    pairPlans,
    redactionRows,
    type MarkKind,
    type StampName,
    type PageId,
    type RegionPlan,
  } from "./lib/pages";
  import { nameOf } from "./lib/markpopup";
  import { sweepLabel } from "./lib/markband";
  import { labelsFor, MAX_RECENTS, recentCommandId, RECENT_PREFIX } from "./lib/recents";
  import {
    clampPlace,
    loadSession,
    SessionWriter,
    type Place,
    type Session,
  } from "./lib/session";
  import { runMarkCheckIfRequested } from "./lib/markcheck";
  import { runSessionCheckIfRequested } from "./lib/sessioncheck";
  import { runOpenCheckIfRequested } from "./lib/opencheck";
  import { Serial } from "./lib/serial";
  import { DegradedLabel } from "./lib/degraded";
  import { Updates, updateLabel, updateNotice, type UpdateState } from "./lib/update";
  import { Viewer, type ViewerStatus } from "./lib/viewer";
  import { describeFit, percentOf } from "./lib/zoom";

  let surface = $state<HTMLDivElement | null>(null);
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
  const covered = new Map<number, string>();
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
  function armedLabel(kind: MarkKind | "crop" | "redact"): string {
    if (kind === "crop") return "Crop — drag out what to keep";
    // **Says what goes, where the crop's says what stays**, because the two are
    // the same drag and this is the only place the reader is told which one
    // they armed. "Nothing is removed yet" is the other half: marking is
    // reversible and applying is not, and a reader who thinks the first line
    // has already destroyed something will not review the list.
    if (kind === "redact")
      return "Redact — drag out what to remove; nothing is removed yet";
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
  const degradedGate = new DegradedLabel();
  let query = $state("");
  let findField = $state<HTMLInputElement | null>(null);
  let sidebarShown = $state(false);

  let viewer: Viewer | null = null;
  let palette: Palette | null = null;
  let sidebar: Sidebar | null = null;
  let propertiesDialog: PropertiesDialog | null = null;
  let passwordDialog: PasswordDialog | null = null;
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
   * A comment id is what the annotation carries and it does not move ---
   * `redactionPagesRead` below reaches the same conclusion from the other end,
   * keying on the page *id* rather than the slot for regions that carry one.
   * Cleared with the document, since an id means nothing across two files.
   */
  const wordsAsked = new Set<number>();
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
  const commentWords = new Map<number, string>();
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
  const redactionWords = new Map<number, string | null>();
  /**
   * Pages whose text has been read for the regions on them, **by page id**.
   *
   * Not by slot, which is what {@link wordsAsked} does one field up, and the
   * difference is a defect rather than a preference: a slot is renumbered by
   * every deletion, so a set of slots says page 4 has been read and then a
   * deletion moves an unread page into slot 4, where it is never asked again.
   * A `PageId` is what the region itself carries and it does not move.
   */
  const redactionPagesRead = new Set<number>();
  /**
   * What a removal would take from each pending region, by redaction id.
   *
   * Separate from {@link redactionWords} because they answer different
   * questions and come from different places: the words a region *covers* are
   * geometry this process already holds, and this is a reading of the page's
   * content stream that only a worker can do. What it is for is the row's
   * second line --- the objects a removal cannot take.
   */
  const redactionPlans = new Map<number, RegionPlan>();
  /** Whether {@link fillRedactionWords} is walking, so a second call stands down. */
  let fillingRedactionWords = false;
  let rawOutline: Outline | null = null;

  /** Path of the open document, which is what a remembered place is keyed on. */
  let openPathName = "";
  /** Its page count, so a place can record what the document had when written. */
  let openPageCount = 0;
  /** Places read at launch. Read once: the file is ours and nothing else writes it. */
  let session: Session = { places: [] };
  /**
   * Whether pages are shown inverted.
   *
   * Held here rather than read from the viewer, because it has to survive the
   * viewer: closing one document and opening another must not quietly turn the
   * mode back off.
   */
  let invertPages = false;
  /** Collapses a scroll's worth of positions into at most one write per second. */
  const places = new SessionWriter();
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
    pageCount: () => status?.pageCount ?? 0,
    openDocument: () => void pickAndOpen(),
    reloadDocument: () => reloadDocument(),
    busyOpening: () => opening,
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
    // Wrapped rather than passed straight through, because a check that lands on
    // `current` shows nothing in the header by design -- so before this, pressing
    // "Check for updates" and being up to date was indistinguishable from a
    // command that did not run.
    checkForUpdates: () => void checkAndSay(),
    applyUpdate: () => void updates.install(),
    updateAvailable: () => updates.state.kind === "available",
    updateReady: () => updates.state.kind === "ready",
    rotatePage: (delta) => void rotatePage(delta),
    deletePage: () => void deletePage(),
    cropPage: (to) => void cropPage(to),
    redactRegion: () => viewer?.armRedact(),
    redactSelection: () => void redactSelection(),
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
    draw: () => viewer?.armDraw("ink"),
    erase: () => viewer?.armErase(),
    hasSelection: () => (status?.selected ?? 0) > 0,
    removeMark: () => removeMark(),
    hasOpenMark: () => (viewer?.markOpen ?? -1) >= 0,
    setMarkColor: (id) => chooseMarkColor(id),
    markColor: () => markColor.id,
    saveDocument: () => void saveDocument(),
    isDirty: () => dirty,
    saveCopy: () => void saveCopy(),
    redactCopy: () => void redactCopy(),
    redactDocument: () => redactDocument(),
    extractPages: (slots) => void extractPages(slots),
    splitDocument: (groups) => void splitDocument(groups),
    mergeDocuments: () => void mergeDocuments(),
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
    const marks = viewer?.selectionQuadsByPage() ?? [];
    for (const { page, quads, text } of marks) {
      // Which ids existed before, so the one that appears can be identified by
      // difference --- `addComment` below gives the argument for asking it this
      // way rather than taking the last mark or the highest id.
      const before = new Set((edits?.state.marks ?? []).map((mark) => mark.id));
      await applyEdit((e) => e.mark(kind, page, quads, [], "", markColor.rgb));
      const made = (edits?.state.marks ?? []).find(
        (mark) => !before.has(mark.id),
      );
      // Absent when the model refused. Nothing to record and nothing to say ---
      // `applyEdit` has already shown the refusal.
      if (made) covered.set(made.id, text.slice(0, COVERED_CHARS));
    }
    // `applyEdit` painted the panel already --- but it painted it *before* this
    // loop knew which id to file the words under, so every row it drew says
    // nothing was typed on the mark. One repaint here rather than one inside
    // the loop after each `covered.set`: a selection over four pages makes four
    // marks, and the three intermediate paints would each be replaced within
    // the millisecond by the next `applyEdit`.
    if (marks.length > 0 && edits) {
      sidebar?.setMarks(markRows(edits.state.marks, edits.map));
    }
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
    for (const { page, quads } of viewer?.selectionQuadsByPage() ?? []) {
      for (const area of areasFrom(quads)) {
        await applyEdit((e) => e.redact(page, area));
      }
    }
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
    if (!viewer) return;
    const refusal = tooManyMatchesToMark(viewer.matchCount);
    if (refusal) {
      say(refusal);
      return;
    }
    const found = await viewer.matchQuadsByPage();
    if (found === null) {
      say(
        "Some of the pages with matches on them could not be read, so nothing " +
          "was marked. Nothing has been changed.",
      );
      return;
    }
    for (const { page, quads } of found) {
      for (const area of areasFrom(quads)) {
        await applyEdit((e) => e.redact(page, area));
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
    const where = viewer?.commentAt(at);
    if (!where) return;
    // Which ids existed before, so the one that appears can be identified by
    // difference rather than by guessing. "The last mark in the list" and "the
    // highest id" are both inferences about how the model numbers and orders
    // things, and neither is written down anywhere as a promise --- a set
    // difference needs no promise at all.
    const before = new Set((edits?.state.marks ?? []).map((mark) => mark.id));
    await applyEdit((e) =>
      e.mark("note", where.page, where.quads, [], "", markColor.rgb),
    );
    const made = (edits?.state.marks ?? []).find((mark) => !before.has(mark.id));
    // Absent if the model refused --- an empty quad, a page that is gone. The
    // refusal has already been shown by `applyEdit`, so there is nothing to say
    // here beyond not opening a note on a mark that was never made.
    if (made) viewer?.showMark(made.id);
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
  async function drawn(
    kind: MarkKind,
    page: PageId,
    shape: Drawn,
    stamp: StampName | null,
  ): Promise<void> {
    const before = new Set((edits?.state.marks ?? []).map((mark) => mark.id));
    await applyEdit((e) =>
      e.mark(kind, page, shape.quads, shape.strokes, "", markColor.rgb, stamp),
    );
    const made = (edits?.state.marks ?? []).find((mark) => !before.has(mark.id));
    if (made) viewer?.showMark(made.id);
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
    viewer?.removeOpenMark();
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
    if (at === undefined || !edits) return;
    if (to === "drag") {
      // Arms and returns: the rest of this gesture happens when the reader
      // lets go, in `cropTo`. Nothing is measured and no page is named here,
      // because the page is whichever one they press on --- which need not be
      // the one they are scrolled to.
      viewer?.armCrop();
      return;
    }
    if (to === "reset") {
      await applyEdit((e) => e.crop(at, null));
      return;
    }
    const source = edits.state.pages[at]?.source;
    if (source === undefined) return;
    const box = await contentBox(edits.doc, source).catch(() => null);
    if (!box) {
      say("There is nothing on this page to crop to.");
      return;
    }
    await applyEdit((e) => e.crop(at, box));
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
    page: number,
    rect: [number, number, number, number],
  ): Promise<void> {
    if (!edits) return;
    const source = edits.state.pages.find((p) => p.id === page)?.source;
    if (source === undefined) return;
    const box = await cropBox(edits.doc, source, rect).catch(() => null);
    if (!box) {
      say("That rectangle could not be turned into a crop.");
      return;
    }
    await applyEdit((e) => {
      const slot = e.state.pages.findIndex((one) => one.id === page);
      return slot < 0 ? Promise.resolve(e.state) : e.crop(slot, box);
    });
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
  let pendingEdit: Promise<void> = Promise.resolve();

  /**
   * Runs one edit and moves the viewer to the state it produced.
   *
   * Every route in goes through here, which is the same reasoning that put the
   * history recording inside `goToDestination` rather than at its four callers:
   * the fifth caller is the one that forgets. What it must not become is a place
   * where the *next* state is computed --- it is handed one, and its whole job
   * is to redraw what differs.
   */
  function applyEdit(run: (edits: Edits) => Promise<EditState>): Promise<void> {
    // Queued rather than started, so a reply can never be adopted after a
    // later one. See {@link editing}.
    pendingEdit = editing.run(() => runEdit(run));
    return pendingEdit;
  }

  async function runEdit(
    run: (edits: Edits) => Promise<EditState>,
  ): Promise<void> {
    const model = edits;
    if (!model || !viewer) return;
    try {
      const after = await run(model);
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
      viewer?.setMarks(after.marks);
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
      // Undo and Redo are the two menu items whose enablement moves on every
      // edit, which is why this is here rather than only at the ends of an open.
      refreshMenu();
    } catch (e) {
      // Shown rather than logged. A refusal here is about the document --- a page
      // that is gone, a handle that is not open --- and a rotate command that
      // silently does nothing reads as a broken application.
      say(String(e));
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
    viewer?.setLinks(linksIn(rawLinks, pages));
    // An answer that has not arrived is left alone rather than pushed as
    // `null`: to these panels `null` means "this document's comments could not
    // be read", which is a different thing to tell a reader than "not yet". The
    // failing path still says it, from the `catch` that knows.
    if (rawComments) {
      const items = commentsIn(rawComments.items, pages);
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
    try {
      for (;;) {
        // **Re-slotted every round, not once before the loop.** A page number
        // here is a slot, and a reader who deletes a page mid-walk renumbers
        // every slot after it --- so a list captured up front would read slot 4's
        // text and hand it to the comment that used to be there. Wrong words on
        // a real row, which is the failure that looks entirely plausible.
        const items = commentsIn(source.items, edits?.map ?? NO_PAGES);
        const page = pagesNeedingWords(items, wordsAsked)[0];
        if (page === undefined) return;
        // The document that was open when this page was asked for. A second file
        // opened mid-walk replaces `rawComments`, and writing this one's
        // sentences onto its rows has the same shape as the slot problem above.
        if (rawComments !== source) return;
        // The comments this round will answer, recorded before the await so a
        // page that cannot be read is not asked for again on the next edit.
        // **Their ids, not the page**: a page number here is a slot, and a
        // deletion renumbers every slot after it --- see `pagesNeedingWords`,
        // which now takes this set and states what went wrong when it held
        // slots.
        for (const comment of wantingWordsOn(items, page)) wordsAsked.add(comment.id);
        const words = await wordsForPage(items, page, (at) =>
          viewer ? viewer.unturnedText(at) : Promise.resolve(null),
        );
        if (rawComments !== source) return;
        if (words.size > 0) {
          for (const [id, said] of words) commentWords.set(id, said);
          sidebar?.setCommentWords(words);
        }
      }
    } finally {
      fillingWords = false;
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
   * **Why a page that could not be read is recorded as read.** Otherwise the
   * walk asks for it again on the next edit, forever, and the row it belongs to
   * says *reading* for the rest of the session. It is recorded as `null`
   * instead, which is a state the row draws as what it is.
   */
  async function fillRedactionWords(): Promise<void> {
    if (fillingRedactionWords) return;
    const model = edits;
    if (!model || !viewer) return;
    fillingRedactionWords = true;
    try {
      for (;;) {
        // The model that was open when this round started. A second document
        // replaces `edits` mid-walk, and writing this one's words onto its rows
        // is the same failure `fillCommentWords` guards against.
        if (edits !== model) return;
        const next = model.state.redactions.find(
          (region) => !redactionPagesRead.has(region.page),
        );
        if (!next) return;
        redactionPagesRead.add(next.page);
        const slot = model.map.slotOfId(next.page);
        // A region whose page is in no slot. Unreachable from the model as it
        // stands --- see `RedactionRow.page` --- and answered rather than
        // skipped, because a row left with no entry says "reading" for ever.
        const text =
          slot === undefined ? null : ((await viewer?.unturnedText(slot)) ?? null);
        if (edits !== model) return;
        const asked = model.state.redactions.filter(
          (region) => region.page === next.page,
        );
        for (const region of asked) {
          redactionWords.set(
            region.id,
            text === null ? null : touchedText(text, region.area),
          );
        }
        // The page of the *file*, which is what the backend means by a page
        // number: `slot` is a position in the document as the reader has it,
        // and a deletion above this page makes the two different numbers.
        const source = slot === undefined ? undefined : model.map.sourceOf(slot);
        if (source !== undefined) {
          try {
            const plans = await invoke<RegionPlan[]>("redaction_plans", {
              doc: model.doc,
              page: source,
              regions: asked.map((region) => region.area),
            });
            if (edits !== model) return;
            for (const [id, plan] of pairPlans(asked, plans)) {
              redactionPlans.set(id, plan);
            }
          } catch (e) {
            // Not raised to the reader. The rows keep saying what they said,
            // which is nothing about what a removal would take --- and the
            // command that actually redacts asks again and reports its own
            // failures, so a reader is never left acting on this silence.
            console.warn(`could not read what a removal would take: ${e}`);
          }
        }
        sidebar?.setRedactionWords();
      }
    } finally {
      fillingRedactionWords = false;
    }
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
   * released.
   *
   * **The place is expressed in slots**, which is the one argument here that
   * could quietly be wrong --- see {@link currentPlace}. Captured before the
   * save rather than after, because the viewer is torn down by the reopen.
   *
   * A failure that says `reopen` is one the document did not survive; anything
   * else left the reader exactly as they were, and is only a message.
   */
  async function saveDocument(): Promise<void> {
    if (!edits || !openPathName || !viewer) return;
    viewer.closeMark();
    await pendingEdit;
    const path = openPathName;
    const place = currentPlace(false);
    try {
      await edits.save(path);
    } catch (e) {
      const failure = e as {
        message?: string;
        reopen?: boolean;
        changed?: boolean;
      };
      // The message and the buttons come from one call, so they cannot disagree
      // about what happened. A refusal that names Save a copy now arrives with
      // Save a copy beside it -- which it did not until 2026-08-19, and worse,
      // Save a copy was refused by the same guard, so the advice named a door
      // that was locked.
      const prompt = afterFailedSave({
        message: failure.message ?? String(e),
        reopen: failure.reopen,
        changed: failure.changed,
      });
      if (!failure.reopen) {
        say(prompt.message, prompt.offers);
        return;
      }
      // The document is closed and the file is the one it always was. Reopening
      // is what gives the reader something to look at; their unsaved commands
      // are gone with the model, which is what the message says.
      openDoc = -1;
      await openPath(path, false, place);
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
    openDoc = -1;
    await openPath(path, false, place);
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
      say(String(e));
    }
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
      say(afterRedaction(await edits.redactCopy(openPathName, chosen)));
    } catch (e) {
      // Every refusal reaches here, and the one worth the room is the region
      // that covers something a removal cannot take: `lib.rs` refuses before
      // writing anything and names what and where.
      say(String(e));
    }
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
    if (!edits || !openPathName || !viewer) return;
    viewer.closeMark();
    await pendingEdit;
    const path = openPathName;
    const place = currentPlace(false);
    say(null);
    let said: string;
    try {
      said = afterRedaction(await edits.redactDocument(path));
    } catch (e) {
      const failure = e as { message?: string; reopen?: boolean; changed?: boolean };
      const prompt = afterFailedSave({
        message: failure.message ?? String(e),
        reopen: failure.reopen,
        changed: failure.changed,
      });
      // Nothing happened: the file is the file and the reader still has their
      // document and their marks. A message is all there is to do.
      if (!failure.reopen) {
        say(prompt.message, prompt.offers);
        return;
      }
      openDoc = -1;
      await openPath(path, false, place);
      if (!error) say(prompt.message, prompt.offers);
      return;
    }
    openDoc = -1;
    await openPath(path, false, place);
    if (!error) say(said);
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
      say(String(e));
    }
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
      say(String(e));
    }
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
      say(String(e));
    }
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

    const wanted = openDoc;
    propertiesDialog.show(null, "");
    try {
      const answer = await invoke<Properties>("document_properties", { doc: wanted });
      if (openDoc !== wanted || !propertiesDialog.isOpen) return;
      properties = answer;
      propertiesDialog.show(answer, "");
    } catch (e) {
      if (openDoc !== wanted || !propertiesDialog.isOpen) return;
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

  const commands = new CommandRegistry();
  registerAppCommands(commands, appActions);

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
      const event = await invoke<string | null>("set_menu", {
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
    if (!menuInstalled) return;
    const state = menuEnablement(commands);
    // Compared rather than pushed, because this is now called from the frame
    // loop: the enablement of twenty commands is twenty closures reading local
    // variables, and the message across the boundary is the expensive half.
    const key = JSON.stringify(state);
    if (key === menuPushed) return;
    menuPushed = key;
    void invoke("set_menu_enabled", { state }).catch(() => {
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
    viewer.setInverted(invertPages);
    // Written directly rather than through the place writer. The writer skips a
    // place identical to the last one it sent, and inverting the page moves
    // nothing --- so routed that way, a reader who inverts and quits without
    // scrolling would find the preference forgotten.
    void invoke("session_set_invert_pages", { invert: invertPages }).catch(() => {
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
    if (!openPathName || !viewer) return;
    try {
      await invoke("print_document", {
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
      // a broken application, and on Windows it *will* fail, by design.
      say(String(e));
    }
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
      page: inFile ? (edits?.map.sourceOf(where.page) ?? where.page) : where.page,
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
    error = message;
    offers = message === null ? [] : next;
  }

  /**
   * Opens the current document's path again.
   *
   * The place is captured here and handed to the open, rather than left to the
   * lookup that a launch restore uses. `session` is the snapshot loaded at
   * startup and is never updated --- `places.note` writes over IPC to Rust ---
   * so reopening the current path would put the reader back where they were
   * when the *application* started, which on a long session is nowhere near
   * where they are now.
   *
   * Nothing is guarded on the file having changed. Reload is also what someone
   * reaches for when they know they have changed it, and a command that
   * refuses because the app has not noticed yet is worse than one that always
   * does what it says.
   */
  function reloadDocument() {
    const path = openPathName;
    if (!path) return;
    // Reload reopens the file, which closes the document and spends the journal.
    // On an unedited document that costs nothing; on an edited one it is the
    // reader's work, and until 2026-08-19 it went without a word -- the command
    // was written before there was anything to lose, and nothing revisited it
    // when there was. The second press is what confirms: `reloadAnyway` is the
    // offer this prompt carries.
    const prompt = beforeReload(dirty);
    if (prompt) {
      say(prompt.message, prompt.offers);
      return;
    }
    reloadAnyway();
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
  function firstPaint(): Promise<void> {
    return new Promise((resolve) => {
      const timer = setTimeout(done, 1000);
      const started = performance.now();
      function done() {
        clearTimeout(timer);
        resolve();
      }
      function poll() {
        if ((status && status.any >= 0.999) || performance.now() - started > 1000) done();
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
    const paths = from.places.slice(0, MAX_RECENTS).map((place) => place.path);
    const labels = labelsFor(paths);
    commands.replace(
      RECENT_PREFIX,
      paths.map((path, index) => ({
        id: recentCommandId(index),
        // Prefixed with the verb so the row reads as a command next to "Zoom
        // in" rather than as a stray filename. Ranking is subsequence matching,
        // so typing part of the name still finds it.
        title: `Open ${labels[index] ?? path}`,
        run: () => void openPath(path),
      })),
    );
  }

  /** Rebuilds the recent-document commands from disk, then re-ranks. */
  async function refreshRecents() {
    offerRecents(await loadSession());
    // The palette may have been opened while this was in flight, or closed
    // again, or moved into argument mode. `reload` is a no-op in the last two.
    palette?.reload();
  }

  function focusFind() {
    findField?.focus();
    findField?.select();
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
    const wanted = query;
    findTimer = setTimeout(() => viewer?.search(wanted), FIND_DEBOUNCE_MS);
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
    void (async () => {
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
        () => invoke<number>("release_documents"),
        (line) => console.info(`[open] ${line}`),
      );

      // After the spike entry points, which exit the process: none of them mount
      // the shell, and a palette attached to `document.body` would outlive it.
      palette = new Palette(commands);

      // On `document.body` rather than inside the viewer, for the reason the
      // context menu is: a modal that lives in a scroll box is clipped by it.
      propertiesDialog = new PropertiesDialog(document.body);

      // Beside it, and for its reason. A locked document is asked about
      // rather than reported, which is the whole of what this adds --- see
      // `passworddialog.ts`.
      passwordDialog = new PasswordDialog(document.body);

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
        if (!inside) contextMenu.close();
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
          // Without the keyboard: the menu is what the reader is about to arrow
          // through, and `showMark`'s default puts the caret in the note's text
          // field, which would eat every key the menu needs.
          if (own !== null) viewer?.showMark(own, false);
          openContextMenu(menuForSurface(own), at);
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
          await invoke<Record<string, string>>("keyboard_positions"),
        );
        relabelCommands(commands);
      } catch {
        // Deliberately silent. Nothing is broken -- a shortcut still works and
        // is still advertised, under the spelling a US keyboard would use.
      }

      await installMenu();

      // The launch check, and its position here is the whole of what keeps every
      // spike, benchmark and check run offline: all of them return above this
      // line. `void` rather than `await` because nothing downstream depends on
      // the answer and a slow endpoint must not delay a document opening --- the
      // reader is here to read, and an update is never urgent.
      void updates.check();

      await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const [path] = event.payload.paths;
        if (path) void openPath(path);
      });

      // A last chance to record the position for a reader who quits inside the
      // writer's interval. Best effort by construction --- the write is an async
      // IPC call and the process need not outlive it --- which is why the
      // interval is a second rather than something that leans on this.
      window.addEventListener("pagehide", () => places.flush());

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
      appVersion = await invoke<string>("app_version");

      const openEvent = await invoke<string>("launch_open_event");
      await listen<string>(openEvent, (event) => void openPath(event.payload));
      const handed = await invoke<string[]>("take_launch_paths");

      session = await loadSession();
      // From the session already in hand, so opening the palette on the first
      // keystroke costs nothing. Refreshed from disk after that -- see
      // `refreshRecents`.
      offerRecents(session);
      // Read before any document opens, so the first tiles of the first page are
      // requested in the polarity the reader left the application in.
      invertPages = session.invert_pages ?? false;
      const [first] = handed;
      if (first) {
        // One window, so one document. Selecting several in the Finder opens
        // the first; the rest are dropped rather than silently replacing each
        // other, which is what opening them in turn would look like.
        await openPath(first);
      } else {
        const resume = session.places[0];
        if (resume) await openPath(resume.path, true);
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
      multiple: false,
      directory: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof chosen === "string") await openPath(chosen);
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
    return opens.run(() => openDocument(path, resuming, resume));
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
      (password) => invoke<DocumentInfo>("open_document", { path, password }),
      dialog ? (problem) => dialog.ask(basename(path), problem) : null,
    );
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
   */
  async function openDocument(
    path: string,
    resuming = false,
    override: Place | null = null,
  ) {
    say(null);
    notice = null;
    opening = true;
    /**
     * Whether this body has already torn the outgoing document down.
     *
     * What the `catch` is allowed to clear depends on how far the body got, and
     * the two cases are opposites. A failure *before* this point --- an
     * `open_document` that threw, which is the common one --- has touched
     * nothing: the reader still has their document on screen, and clearing
     * `title` there unmounts the body out from under a live viewer and sidebar
     * while the backend still holds the file. A failure *after* it has no
     * document left to keep, and leaving the singletons set would advertise one
     * that is gone.
     */
    let replaced = false;
    try {
      const doc = await openOrAsk(path);

      // Released only once the replacement exists, so a file that turns out not
      // to open leaves the reader with what they had --- and recorded here,
      // before anything else can throw, because from this line on the backend is
      // holding this document whether or not it ever reaches the screen.
      //
      // Under the worker backend this is a process rather than an allocation:
      // without it a session that opens a dozen files holds a dozen sandboxed
      // children. Not awaited --- the render thread is FIFO, so the close is
      // already behind everything the outgoing document had outstanding, and
      // making the reader wait for a process teardown on the way to their first
      // page would be paying for it twice.
      const outgoing = openDoc;
      openDoc = doc.id;
      if (outgoing >= 0 && outgoing !== doc.id) {
        void invoke("close_document", { doc: outgoing }).catch((e) => {
          // Not raised to the reader: the file they asked for is open and fine,
          // and this is a leak rather than anything they can act on.
          console.warn(`could not release document ${outgoing}: ${e}`);
        });
      }

      const page = doc.pages[0];
      if (!page) throw new Error("document reports no pages");
      // The whole table the open carried, not only its first entry. On a lazy
      // open --- the default, because collecting every page's size costs 86 ms
      // on a long document --- that *is* only the first entry, and the viewer
      // estimates the rest and corrects them as it reads. What it must not do is
      // discard sizes the backend already sent, which is what handing over
      // `pages[0]` alone did: with `TPDF_EAGER_GEOMETRY` set the whole document's
      // geometry arrived and every page after the first was still laid out at
      // page 1's.
      const pages: [PageSize, ...PageSize[]] = [page, ...doc.pages.slice(1)];

      // Whatever the outgoing document was owed, before its path is replaced.
      places.flush();
      // The debounced find is keyed to the viewer being destroyed on the next
      // line: left armed, it fires a scan at the *new* document for a query the
      // field no longer shows, because `query` is cleared below.
      clearTimeout(findTimer);
      replaced = true;
      viewer?.destroy();
      viewer = null;
      sidebar?.destroy();
      sidebar = null;
      status = null;
      degraded = degradedGate.update(null, performance.now());
      title = basename(path);
      openPathName = path;
      openPageCount = doc.page_count;

      // Fitted to the document as it is now, not as it was: the file may have
      // been rebuilt shorter since, and a viewer scrolled past its own last page
      // is a worse answer than the wrong page.
      // A caller that already knows where the reader is wins over the startup
      // snapshot --- see `reloadDocument`, which is the only one that does.
      const remembered =
        override ?? session.places.find((kept) => kept.path === path);
      const resume = remembered ? clampPlace(remembered, doc.page_count) : null;
      sidebarShown = resume ? resume.sidebar : sidebarShown;

      // The host element does not exist until the viewer section is in the
      // DOM, and it is not while the empty-state placeholder is showing.
      await new Promise(requestAnimationFrame);
      if (!surface || !sidebarHost) throw new Error("no surface to mount into");

      query = "";
      // Before the panels are built, so nothing carries over from the document
      // that was open a moment ago --- these are answers about a file, and the
      // file has changed.
      rawLinks = [];
      rawComments = null;
      // A page number is a slot in the document that is closing, so an entry
      // kept would tell the next file's page 3 that its words are already known.
      wordsAsked.clear();
      commentWords.clear();
      redactionWords.clear();
      redactionPlans.clear();
      redactionPagesRead.clear();
      properties = null;
      propertiesDialog?.close();
      rawOutline = null;
      sidebar = new Sidebar(sidebarHost, {
        onNavigate: (target, top) => {
          viewer?.goToDestination(target, top);
          viewer?.focus();
        },
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
          // A row is a slot and a tile request names a page of the file. The two
          // are the same number until a page is deleted; see `pages.ts`.
          sourceOf: (slot) => edits?.map.sourceOf(slot),
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
        },
      });
      sidebar.setVisible(sidebarShown);

      // Before the viewer, so that a rotate arriving on the first frame has a
      // model to ask. `refresh` is not awaited: it reads a `HashMap` in the
      // backend, and holding the first page behind it would put an IPC round
      // trip on the startup path for an answer that is "nothing is edited".
      const opening = new Edits(doc.id, doc.page_count);
      edits = opening;
      dirty = false;
      // Mark ids start again with the model, so an entry kept from the last
      // document would put its words on this one's first highlight.
      covered.clear();
      void opening.refresh().then(
        (state) => {
          // The model this reply belongs to, not whichever one is open when it
          // lands. A second document opened inside the round trip replaces
          // `edits` and the panels with it, and this would then translate one
          // document's marks through another's page order and list the result.
          // `dirty` had the same hazard and the same one-line fix.
          if (edits !== opening) return;
          dirty = state.dirty;
          // A document opened with edits already on it --- which is the model's
          // to answer, not this file's to assume. Every later change comes
          // through `runEdit` above.
          sidebar?.setMarks(markRows(state.marks, opening.map));
          sidebar?.setRedactions(redactionRows(state.redactions, opening.map));
        },
        (e) => {
          // Not raised to the reader. Nothing is wrong with their document ---
          // the edit commands will refuse until this succeeds, which is the
          // right failure, and an error banner over a page that opened fine is
          // not.
          console.warn(`could not read the edit state: ${e}`);
        },
      );

      viewer = new Viewer(surface, {
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
        onMarkNote: (mark, note) => void applyEdit((e) => e.renote(mark, note)),
        onMarkRemove: (mark) => void applyEdit((e) => e.unmark(mark)),
        // A colour picked in the swatch row, or by a `Colour:` command with a
        // note open. A command like the note above it, and undone the same way.
        onMarkRecolor: (mark, color) =>
          void applyEdit((e) => e.recolor(mark, color)),
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
        onMarkMoved: (id, dx, dy) => void applyEdit((e) => e.displace(id, dx, dy)),
        onErased: (mark, remove) => void applyEdit((e) => e.erase(mark, remove)),
        // The same sweep's other half: a mark with no parts to lose goes whole.
        // `unmark` is what the mark panel's own Remove already calls, so a mark
        // taken by the nib and one taken from the list are one command and one
        // undo, however the reader asked.
        onUnmarked: (mark) => void applyEdit((e) => e.unmark(mark)),
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
          notePlace();
        },
        // Shown, for the same reason a failed print is: this fires only for a
        // command the reader typed and is waiting on --- a copy that could not
        // read every page it spans, or a clipboard that refused the write.
        onError: (message) => {
          say(message);
        },
        // The one message here nobody asked for. It fires while someone is
        // reading, because a process outside the application shortened the file
        // underneath them --- so it goes to the same surface as the errors they
        // did ask for, which is the only one this window has. The pages already
        // painted stay painted; what this adds is the reason the rest never
        // arrive.
        onGone: (message) => {
          say(message);
        },
      });
      // Before the first paint, so the reader sees their page rather than page
      // one and then a jump --- and before `focus`, which does not move the view
      // but would make the jump look like something they did.
      if (resume) viewer.restore(resume);
      // After `restore`, which does not touch the colours, and before `focus`,
      // so the first tiles requested are already the right polarity rather than
      // being rendered light and immediately thrown away.
      viewer.setInverted(invertPages);
      viewer.focus();

      // After the viewer, deliberately not awaited, and deliberately not asked
      // for until the first screen is up.
      //
      // Not awaiting the *outline* was always right: it shares the render thread
      // with tiles and a document that opens instantly should not wait for its
      // table of contents. Waiting for the first paint before *asking* is there
      // because the walk stopped being free: resolving a destination on a page
      // carrying `/Rotate` needs the page's rotation, `FPDFPage_GetRotation`
      // needs the page loaded, and that measured 0.17 ms -> 7.5 ms on a
      // twelve-page fixture, about 1 ms per distinct page named. On a book with
      // a three-hundred-entry table of contents that is a third of a second of
      // render thread, and the render thread is FIFO --- so asked for at open it
      // would sit in front of the tiles for the page someone is looking at.
      //
      // What changed is that `openPath` is now a chain, and `firstPaint` waits
      // up to a second: awaiting it here would hold the *next* document's open
      // behind a delay that has nothing to do with it. Both halves are already
      // guarded by `openDoc === wanted`, so letting the whole tail run detached
      // costs nothing --- an outline for a document nobody is looking at is
      // dropped exactly as it was before.
      const wanted = doc.id;
      void firstPaint()
        .then(() => {
          // Checked before asking as well as after. The wait is up to a second
          // and is no longer inside the open, so another document can arrive
          // during it --- and an outline walk for a file nobody is looking at is
          // not merely wasted, it is a third of a second of the FIFO render
          // thread in front of the tiles for the file they *are* looking at.
          if (openDoc !== wanted) return null;
          return invoke<Outline>("document_outline", { doc: wanted });
        })
        .then((result) => {
          // And again, because another document may have been opened while the
          // walk itself was in flight.
          if (!result || openDoc !== wanted) return;
          rawOutline = result;
          applyPageOrder();
        })
        .catch(() => {
          if (openDoc === wanted) sidebar?.setOutline(null);
        });

      // The comments, on the same terms and for a different reason. They cost
      // an `lopdf` parse of the whole file --- 0.1 ms small, 11.9 ms on the
      // 337 MB scan --- rather than render-thread time, so what this waits for
      // is not the render queue but the first paint: warm startup has ~25 ms of
      // margin against its 300 ms target, and this is off that path entirely.
      // A separate chain rather than a link in the one above, so a document
      // whose outline cannot be read still gets its comments and the reverse.
      void firstPaint()
        .then(() => {
          if (openDoc !== wanted) return null;
          return invoke<Comments>("document_comments", { doc: wanted });
        })
        .then((result) => {
          if (!result || openDoc !== wanted) return;
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
        })
        .catch(() => {
          if (openDoc === wanted) sidebar?.setComments(null);
        });

      // The links, on the same terms again --- a third chain rather than a link
      // in either above, so one failing does not take the others with it.
      //
      // Where this differs from the comments: nobody opens a panel before
      // clicking a cross-reference, so waiting for demand would mean the first
      // click on any document goes nowhere. It waits for first paint for the
      // same reason they do, and for nothing else.
      void firstPaint()
        .then(() => {
          if (openDoc !== wanted) return null;
          return invoke<Links>("document_links", { doc: wanted });
        })
        .then((result) => {
          if (!result || openDoc !== wanted) return;
          rawLinks = result.items;
          applyPageOrder();
          // A cut list is worth saying out loud, for the reason every bound in
          // this application reports itself: a document whose cross-references
          // half work is worse to use than one whose links are all dead, and
          // silence makes the two indistinguishable.
          const said = linkNotice(result.limits);
          if (said) error = said;
        })
        .catch(() => {
          // Deliberately quiet. A document with no readable links is the common
          // case --- most PDFs have none --- and there is nothing the reader
          // would do about it, so this is not the `onError` contract.
        });
    } catch (e) {
      if (replaced) {
        // Whatever half-built state got as far as existing. A viewer left alive
        // while `title` is empty runs its frame loop against a detached surface
        // and keeps writing `status`, which the header renders --- a page count
        // and a zoom for a document with no body under them.
        viewer?.destroy();
        viewer = null;
        sidebar?.destroy();
        sidebar = null;
        // Cleared together with `title`, always: `title` gates the body and
        // `status` feeds the header, so one outliving the other is a header
        // describing a document that is no longer on screen.
        status = null;
        // Cleared through the gate rather than by assignment, so the episode
        // clock is reset too: a stale `#since` would show the next document's
        // first blurry frame instantly, which is the flicker this removes.
        degraded = degradedGate.update(null, performance.now());
        title = "";
        openPathName = "";
        openPageCount = 0;
      }
      // A document that was open last time and is not there now is not a
      // failure the reader caused, so the window simply comes up empty.
      if (!resuming) error = isOpenRefusal(e) ? e.reason : String(e);
    } finally {
      opening = false;
      // In the `finally`, so that a failed open greys the menu back out. Every
      // command but four is withheld without a document, and an open that threw
      // leaves `viewer` null with the menu still saying otherwise.
      refreshMenu();
    }
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

<svelte:window onkeydown={onWindowKey} />

<main>
  <header>
    <!--
      Leftmost, and first in the tab order, because between them these two are
      the whole of this application's mouse-reachable command surface on
      Windows. `menu.rs` puts the native menu bar behind
      `#[cfg(target_os = "macos")]` deliberately --- a menu bar there costs the
      reader no window, and on Windows it would be chrome inside the window,
      which is the ribbon this application exists to not be. The consequence
      went unnoticed until it was reported from use: of 54 commands, the
      toolbar and the two right-click menus reach about 19, the palette reaches
      all of them, and nothing on screen said the palette existed. A reader who
      did not already know ⌘K could not open the sidebar at all.

      Drawn in the toolbar's own idiom --- a small flat glyph like the find
      toggles, not a raised button --- for the reason the zoom readout is: a row
      of buttons drawn as buttons is the ribbon again.
    -->
    <button
      class="chord"
      title="All commands ({label('app.palette')})"
      aria-label="All commands"
      onclick={openPaletteFromToolbar}>≡</button
    >
    {#if title}
      <button
        class="chord"
        class:on={sidebarShown}
        aria-pressed={sidebarShown}
        title="Sidebar ({label('view.toggleSidebar')})"
        aria-label="Toggle sidebar"
        onclick={toggleSidebar}>▤</button
      >
    {/if}
    <button onclick={pickAndOpen} disabled={opening}>Open</button>
    <span class="title">{title}</span>
    <!--
      Left of the spacer, where the degraded label already establishes that an
      element appearing and disappearing costs nothing: the slack absorbs it, and
      the find controls to the right of the spacer do not move. Unlike that one
      it changes at most once per reader action rather than while nobody is
      touching anything, so it needs no episode gate.

      It says "Edited", not "Unsaved". The distinction is real and the shorter
      word is the wrong one: `Save a copy` writes another file and leaves this
      document exactly as edited as it was, so a marker that cleared on a save
      would be claiming the open file had been written.
    -->
    {#if dirty}
      <span class="edited">Edited</span>
    {/if}
    <!--
      Beside the document's name, and deliberately not among the find controls
      where it used to sit. The header is one flex row, so an element that comes
      and goes on the right displaces everything to its left: a fast scroll made
      the whole find toolbar step sideways, and squeezed the search field, every
      time coverage dipped. Here it grows into the slack the spacer was holding,
      so appearing and vanishing moves nothing at all. The delay in
      `degraded.ts` stops it strobing; this stops it shoving.
    -->
    {#if degraded && status}
      <span class="degraded">{degraded} — {Math.round(status.sharp * 100)}% sharp</span>
    {/if}
    <span class="spacer"></span>
    <!--
      Left of the find controls and inside the slack, for the reason the
      degraded label is: this appears without anybody touching the window, and
      an element that arrives on its own must not move what a reader is aiming
      at. `updateLabel` returns null for idle, checking, current and failed, so
      on an ordinary launch nothing is added to the row at all.
    -->
    {#if notice}
      <span class="notice" data-testid="notice">{notice}</span>
    {/if}
    {#if updateLabel(updateState)}
      <button
        class="update"
        class:ready={updateState.kind === "ready"}
        disabled={updates.busy || updateState.kind === "ready"}
        title={updateState.kind === "ready"
          ? "Quit and open tpdf again to finish updating"
          : "Download and apply this update"}
        onclick={() => void updates.install()}>{updateLabel(updateState)}</button
      >
    {/if}
    {#if title}
      <input
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
    {/if}
    {#if status}
      <!--
        **The one mode in this application, said out loud.** Every other tool is
        one-shot and there is nothing to be stuck in; a drawing is several
        strokes, so a reader can be in a state where the next press draws rather
        than selects. `viewer.ts` argues that a mode a reader cannot recognise is
        worse than one they asked for, and this line is what pays that off ---
        it names both keys, because Escape alone would mean the only way out is
        to discard the work.
      -->
      {#if status.drawing !== null}
        <span class="stat" data-testid="drawing">
          {status.drawing === 0
            ? "Drawing — press and drag"
            : `Drawing: ${status.drawing} stroke${status.drawing === 1 ? "" : "s"}`}
          — Enter to finish, Esc to discard
        </span>
      {/if}
      <!--
        A tool armed and waiting for the gesture that spends it.
        **Not a mode a reader can be stuck in --- one they can be lost in.** The
        next press draws a box, or drops a comment, instead of selecting text,
        and until this line existed the only sign of that was a crosshair. It
        names Escape because a reader who has forgotten what they armed needs
        the way out more than the reader who armed it deliberately.
        `viewer.ts` keeps ink out of this field, so it cannot collide with the
        drawing line above.
      -->
      {#if status.armed !== null}
        <span class="stat" data-testid="armed">
          {armedLabel(status.armed)} — Esc to cancel
        </span>
      {/if}
      <!--
        The eraser's twin of the line above, and it names one key rather than
        two: a sweep commits when the reader lifts the pointer, so there is
        nothing waiting to be finished and Escape is the only way out of the
        mode.
      -->
      {#if status.erasing !== null}
        <span class="stat" data-testid="erasing">
          {sweepLabel(status.erasing)}
          — Esc to stop
        </span>
      {/if}
      <!--
        What the next mark will be drawn in, and only once a reader has chosen:
        with nothing picked each kind keeps its own colour, which is what the
        application has always done and is not worth a line of chrome. Once green
        is armed it is a mode like the two above --- it outlives the gesture, and
        the reader who set it three documents ago is exactly the one who needs
        telling.
      -->
      {#if markColor.rgb !== null}
        <span class="stat" data-testid="markcolor">
          Marking in {markColor.name}
        </span>
      {/if}
      {#if status.selected > 0}
        <span class="stat">{status.selected} selected</span>
      {/if}
      <span class="stat">{status.page} / {status.pageCount}</span>
      <!--
        A button rather than a readout, because the number is exactly what a
        reader wants to change when they look at it --- and it opens the same
        palette argument the shortcut does, so there is one implementation of
        "ask for a zoom" rather than a second one nobody checks.
      -->
      <button
        class="stat zoom"
        title="{describeFit(status.fit)} — click to set ({label('view.zoomTo')})"
        onclick={() => palette?.askFor("view.zoomTo")}
        >{percentOf(status.zoom)}%</button
      >
    {/if}
  </header>

  {#if error}
    <div class="problem" data-testid="problem">
      <pre class="error">{error}</pre>
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
              <button data-testid="offer-saveCopy" onclick={() => void saveCopy()}
                >Save a copy…</button
              >
            {:else if offer === "reload"}
              <button data-testid="offer-reload" onclick={() => reloadAnyway()}
                >Reload from disk</button
              >
            {:else if offer === "redact"}
              <button data-testid="offer-redact" onclick={() => void redactAnyway()}
                >Redact this file</button
              >
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if title}
    <div class="body">
      <div class="panel" bind:this={sidebarHost}></div>
      <div class="surface" bind:this={surface}></div>
    </div>
  {:else}
    <div class="empty">
      <p>Open a PDF, or drop one here.</p>
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
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --tpdf-surround: #2b2b2b;
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
    gap: 0.75rem;
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
    padding: 0.2rem 0.8rem;
  }
  /* The two commands that are a route rather than an action, drawn in the
     toolbar's own idiom --- flat, like the find toggles and the zoom readout,
     rather than raised like Open. `.toggle` is deliberately not reused: these
     two sit left of the title and are not part of the find group, and sharing a
     class would mean a change aimed at the find toggles moved these as well. */
  .chord {
    font: inherit;
    font-size: 1.05em;
    line-height: 1;
    background: none;
    border: none;
    color: inherit;
    opacity: 0.6;
    padding: 0.15rem 0.35rem;
    border-radius: 4px;
    flex: none;
    cursor: default;
  }
  .chord:hover {
    opacity: 1;
    background: color-mix(in srgb, CanvasText 10%, transparent);
  }
  /* Same drawn pressed state as `.toggle.on`, and for the same reason: a
     native `:active`-like colour disappears under a dark appearance. */
  .chord.on {
    opacity: 1;
    box-shadow: inset 0 0 0 2px currentColor;
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
  .spacer {
    flex: 1;
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
    color: color-mix(in srgb, currentColor 40%, #c0392b);
  }
  /* Reads as the other stats do until it is pointed at: a toolbar of buttons
     drawn as buttons is the ribbon this application exists to not be. */
  .zoom {
    font: inherit;
    font-variant-numeric: tabular-nums;
    background: none;
    border: none;
    padding: 0.1rem 0.2rem;
    color: inherit;
    cursor: default;
  }
  .zoom:hover {
    opacity: 1;
    background: color-mix(in srgb, CanvasText 10%, transparent);
    border-radius: 4px;
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
  .surface {
    flex: 1;
    min-width: 0;
    min-height: 0;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    opacity: 0.5;
  }
  .error {
    margin: 0;
    padding: 0.5rem 0.7rem;
    color: #c0392b;
    white-space: pre-wrap;
  }
</style>
