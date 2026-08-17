<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/autobench";
  import { runScrollBenchIfRequested } from "./lib/scrollbench";
  import { runStartupTimelineIfRequested } from "./lib/startup";
  import { runViewerCheckIfRequested } from "./lib/viewercheck";
  import { handleWindowKey, registerAppCommands, type AppActions } from "./lib/appcommands";
  import { CommandRegistry } from "./lib/commands";
  import { Edits, type EditState } from "./lib/edits";
  import type { DocumentInfo, PageSize } from "./lib/ipc";
  import { label } from "./lib/keys";
  import { Palette } from "./lib/palette";
  import { basename } from "./lib/paths";
  import { Sidebar, type Tab } from "./lib/sidebar";
  import type { Comments } from "./lib/comments";
  import { noticeFor as linkNotice, type Link, type Links } from "./lib/links";
  import type { Outline } from "./lib/outline";
  import { commentsIn, linksIn, NO_PAGES, outlineIn } from "./lib/pages";
  import { labelsFor, MAX_RECENTS, recentCommandId, RECENT_PREFIX } from "./lib/recents";
  import {
    clampPlace,
    loadSession,
    SessionWriter,
    type Place,
    type Session,
  } from "./lib/session";
  import { runSessionCheckIfRequested } from "./lib/sessioncheck";
  import { runOpenCheckIfRequested } from "./lib/opencheck";
  import { Serial } from "./lib/serial";
  import { DegradedLabel } from "./lib/degraded";
  import { Updates, updateLabel, type UpdateState } from "./lib/update";
  import { Viewer, type ViewerStatus } from "./lib/viewer";
  import { describeFit, percentOf } from "./lib/zoom";

  let surface = $state<HTMLDivElement | null>(null);
  let sidebarHost = $state<HTMLDivElement | null>(null);
  let title = $state("");
  let error = $state<string | null>(null);
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
    checkForUpdates: () => void updates.check(),
    applyUpdate: () => void updates.install(),
    updateAvailable: () => updates.state.kind === "available",
    updateReady: () => updates.state.kind === "ready",
    rotatePage: (delta) => void rotatePage(delta),
    deletePage: () => void deletePage(),
    movePage: (delta) => void movePage(delta),
    undoEdit: () => void applyEdit((e) => e.undo()),
    redoEdit: () => void applyEdit((e) => e.redo()),
    canUndo: () => edits?.state.can_undo ?? false,
    canRedo: () => edits?.state.can_redo ?? false,
    saveCopy: () => void saveCopy(),
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
   * Runs one edit and moves the viewer to the state it produced.
   *
   * Every route in goes through here, which is the same reasoning that put the
   * history recording inside `goToDestination` rather than at its four callers:
   * the fifth caller is the one that forgets. What it must not become is a place
   * where the *next* state is computed --- it is handed one, and its whole job
   * is to redraw what differs.
   */
  async function applyEdit(
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
      dirty = after.dirty;
    } catch (e) {
      // Shown rather than logged. A refusal here is about the document --- a page
      // that is gone, a handle that is not open --- and a rotate command that
      // silently does nothing reads as a broken application.
      error = String(e);
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
    }
    if (rawOutline) {
      sidebar?.setOutline({
        ...rawOutline,
        items: outlineIn(rawOutline.items, pages),
      });
    }
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
      await edits.saveCopy(openPathName, chosen);
    } catch (e) {
      error = String(e);
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
      // Relaunching is the shell's job rather than the state machine's: it ends
      // the process, which is not something a module with unit tests should be
      // able to do. Deferred to the reader's next launch instead of forced ---
      // see `update.ts` on why nothing here swaps the binary under an open
      // document.
    },
  );

  const commands = new CommandRegistry();
  registerAppCommands(commands, appActions);

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
      error = String(e);
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
  function currentPlace(): Place | null {
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
      page: edits?.map.sourceOf(where.page) ?? where.page,
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
    void openPath(path, false, currentPlace());
  }

  /** Opens the sidebar if it is closed, on the tab asked for. */
  function showTab(tab: Tab) {
    if (!sidebarShown) toggleSidebar();
    sidebar?.selectTab(tab);
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
   * The shortcuts that belong to the window rather than to the surface.
   *
   * The routing is `appcommands.ts`'s, for the reason the registration above
   * gives: ⌘K was unreachable by any check while it lived in this file.
   */
  function onWindowKey(event: KeyboardEvent) {
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

      // After the spike entry points, which exit the process: none of them mount
      // the shell, and a palette attached to `document.body` would outlive it.
      palette = new Palette(commands);

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
    error = null;
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
      const doc = await invoke<DocumentInfo>("open_document", { path });

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
        },
      });
      sidebar.setVisible(sidebarShown);

      // Before the viewer, so that a rotate arriving on the first frame has a
      // model to ask. `refresh` is not awaited: it reads a `HashMap` in the
      // backend, and holding the first page behind it would put an IPC round
      // trip on the startup path for an answer that is "nothing is edited".
      edits = new Edits(doc.id, doc.page_count);
      dirty = false;
      void edits.refresh().then(
        (state) => {
          dirty = state.dirty;
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
        },
        onPosition: (at, top) => {
          sidebar?.setPosition(at, top);
          notePlace();
        },
        // Shown, for the same reason a failed print is: this fires only for a
        // command the reader typed and is waiting on --- a copy that could not
        // read every page it spans, or a clipboard that refused the write.
        onError: (message) => {
          error = message;
        },
        // The one message here nobody asked for. It fires while someone is
        // reading, because a process outside the application shortened the file
        // underneath them --- so it goes to the same surface as the errors they
        // did ask for, which is the only one this window has. The pages already
        // painted stay painted; what this adds is the reason the rest never
        // arrive.
        onGone: (message) => {
          error = message;
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
      if (!resuming) error = String(e);
    } finally {
      opening = false;
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
    <pre class="error">{error}</pre>
  {/if}

  {#if title}
    <div class="body">
      <div class="panel" bind:this={sidebarHost}></div>
      <div class="surface" bind:this={surface}></div>
    </div>
  {:else}
    <div class="empty"><p>Open a PDF, or drop one here.</p></div>
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
  button {
    font: inherit;
    padding: 0.2rem 0.8rem;
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
