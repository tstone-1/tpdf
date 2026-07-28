<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/autobench";
  import { runScrollBenchIfRequested } from "./lib/scrollbench";
  import { runStartupTimelineIfRequested } from "./lib/startup";
  import { runViewerCheckIfRequested } from "./lib/viewercheck";
  import { CommandRegistry } from "./lib/commands";
  import { label, matches } from "./lib/keys";
  import { Palette } from "./lib/palette";
  import { Sidebar, type Tab } from "./lib/sidebar";
  import type { Outline } from "./lib/outline";
  import { clampPlace, loadSession, SessionWriter, type Session } from "./lib/session";
  import { runSessionCheckIfRequested } from "./lib/sessioncheck";
  import { runOpenCheckIfRequested } from "./lib/opencheck";
  import { Viewer, type ViewerStatus } from "./lib/viewer";

  interface PageSize {
    width_pt: number;
    height_pt: number;
  }
  interface DocumentInfo {
    id: number;
    pages: PageSize[];
    page_count: number;
    lazy_geometry: boolean;
    open_ms: number;
    at_ms: number;
  }

  let surface = $state<HTMLDivElement | null>(null);
  let sidebarHost = $state<HTMLDivElement | null>(null);
  let title = $state("");
  let error = $state<string | null>(null);
  let opening = $state(false);
  let status = $state<ViewerStatus | null>(null);
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
   * Every open is chained onto this, so no two bodies ever interleave and the
   * document singletons above are only ever mutated by one of them.
   */
  let openChain: Promise<void> = Promise.resolve();

  /**
   * Every command the application has, in one place.
   *
   * Built once and outliving any document, so the palette works before a file
   * is open --- "Open document" is the command someone reaches for first. The
   * rest guard on `viewer`, which is read at call time rather than captured, so
   * closing and opening a document does not need the registry rebuilt.
   *
   * The `keys` strings are labels the palette displays, and they are **derived**
   * from the same table `viewer.ts`'s key handler and `onWindowKey` below match
   * against --- see `keys.ts`. They were hand-written beside those handlers with
   * nothing checking the two agreed, and the gap was not hypothetical: ⌘O was
   * advertised and reached no handler at all, and ⌘P turned the page as well as
   * printing, because the viewer's `p` arm tested the key without the modifier.
   */
  const commands = new CommandRegistry();
  const withDocument = () => viewer !== null;

  commands.register(
    { id: "file.open", title: "Open document", keys: label("file.open"), run: () => void pickAndOpen() },
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
      run: () => void printDocument(),
    },
    {
      id: "find.open",
      title: "Find in document",
      keys: label("find.open"),
      enabled: withDocument,
      run: () => focusFind(),
    },
    {
      id: "find.next",
      title: "Find next",
      keys: label("find.next"),
      enabled: withDocument,
      run: () => viewer?.nextMatch(),
    },
    {
      id: "find.previous",
      title: "Find previous",
      keys: label("find.previous"),
      enabled: withDocument,
      run: () => viewer?.prevMatch(),
    },
    {
      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
      enabled: withDocument,
      run: () => viewer?.zoomStep(1),
    },
    {
      id: "view.zoomOut",
      title: "Zoom out",
      keys: label("view.zoomOut"),
      enabled: withDocument,
      run: () => viewer?.zoomStep(-1),
    },
    {
      id: "view.fitWidth",
      title: "Fit width",
      keys: label("view.fitWidth"),
      enabled: withDocument,
      run: () => viewer?.fitWidth(),
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
      run: () => viewer?.rotateBy(1),
    },
    {
      id: "view.rotateCounterClockwise",
      title: "Rotate view anticlockwise",
      keys: label("view.rotateCounterClockwise"),
      enabled: withDocument,
      run: () => viewer?.rotateBy(-1),
    },
    {
      id: "nav.nextPage",
      title: "Next page",
      keys: label("nav.nextPage"),
      enabled: withDocument,
      run: () => viewer?.nextPage(),
    },
    {
      id: "nav.previousPage",
      title: "Previous page",
      keys: label("nav.previousPage"),
      enabled: withDocument,
      run: () => viewer?.previousPage(),
    },
    {
      id: "nav.firstPage",
      title: "Go to start",
      keys: label("nav.firstPage"),
      enabled: withDocument,
      run: () => viewer?.goToStart(),
    },
    {
      id: "nav.lastPage",
      title: "Go to end",
      keys: label("nav.lastPage"),
      enabled: withDocument,
      run: () => viewer?.goToEnd(),
    },
    {
      id: "edit.selectAll",
      title: "Select all on page",
      keys: label("edit.selectAll"),
      enabled: withDocument,
      run: () => viewer?.selectPage(),
    },
    {
      id: "edit.copy",
      title: "Copy selection",
      keys: label("edit.copy"),
      enabled: withDocument,
      run: () => void viewer?.copySelection(),
    },
    {
      id: "edit.clearSelection",
      title: "Clear selection",
      keys: label("edit.clearSelection"),
      enabled: withDocument,
      run: () => viewer?.clearSelection(),
    },
    {
      // Named for both things a reader might type. One command with one
      // binding rather than two commands sharing one, which would show the
      // same shortcut twice in the palette and teach that it does two things.
      id: "view.toggleSidebar",
      title: "Toggle sidebar",
      keys: label("view.toggleSidebar"),
      enabled: withDocument,
      run: () => toggleSidebar(),
    },
    {
      // Two commands rather than one "switch tab", because the palette is how a
      // command is *found*: someone looking for thumbnails types "thumb", and a
      // command called "Switch sidebar tab" is not what they would type.
      id: "view.showOutline",
      title: "Show outline",
      enabled: withDocument,
      run: () => showTab("outline"),
    },
    {
      id: "view.showThumbnails",
      title: "Show page thumbnails",
      enabled: withDocument,
      run: () => showTab("pages"),
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
      run: () => toggleInvert(),
    },
  );

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
   * Records where the reader is, for the next launch.
   *
   * Called from both `onStatus` and `onPosition` because neither is enough on
   * its own: the status fires when something a reader would notice changed and
   * so misses scrolling *within* a page, and the position fires every frame and
   * carries no zoom or rotation. The writer collapses the overlap.
   */
  function notePlace() {
    if (!viewer || !openPathName) return;
    const where = viewer.position;
    places.note({
      path: openPathName,
      page: where.page,
      top_pt: where.top,
      zoom: viewer.currentZoom,
      fitting: viewer.isFitting,
      turns: viewer.rotation,
      sidebar: sidebarShown,
      page_count: openPageCount,
    });
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
   * Matched through `keys.ts`, which is where the palette's labels come from
   * too --- see the note there. ⌘K is the one chord not in that table: it opens
   * the palette rather than being listed in it, so there is no label for it to
   * disagree with.
   */
  function onWindowKey(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      // Toggling rather than reopening: Cmd-K on an open palette is a request
      // to get rid of it, not to clear the query someone is halfway through.
      if (palette?.isOpen) palette.close();
      else palette?.open();
    } else if (matches("file.open", event)) {
      // ⌘O was advertised in the palette and reached nothing at all: the label
      // was written by hand and no handler was ever added for it, which is the
      // exact disagreement the shared table exists to make impossible.
      //
      // Prevented whether or not an open is already running, but only issued
      // when one is not --- the same guard the Open button carries as
      // `disabled`. Without it the keyboard is the one path that can stack file
      // dialogs, and the second chooser's document then waits behind the first
      // on `openChain` for no reason anyone asked for.
      event.preventDefault();
      if (!opening) void pickAndOpen();
    } else if (matches("find.open", event) && title) {
      event.preventDefault();
      focusFind();
    } else if (matches("file.print", event)) {
      // Prevented whether or not a document is open --- note the missing
      // `&& title` that every other binding here has. WKWebView's own Cmd-P
      // prints the *page*: the chrome, the toolbar, and a scaled-down
      // screenshot of whatever tiles happen to be painted. On the empty state
      // that is a picture of the words "Open a PDF, or drop one here."
      event.preventDefault();
      void printDocument();
    } else if (matches("view.toggleSidebar", event) && title) {
      event.preventDefault();
      toggleSidebar();
    } else if (matches("view.invertPages", event) && title) {
      event.preventDefault();
      toggleInvert();
    }
  }

  /** What the find field's counter says. */
  const findLabel = $derived.by(() => {
    const search = status?.search;
    if (!search || !search.query) return "";
    if (search.textless) {
      // Distinct from "no matches" on purpose: the query was never tested
      // against anything, and saying so is the difference between a working
      // search and a broken one from the reader's side.
      return search.running ? "no text yet" : "no text to search";
    }
    if (search.total === 0) return search.running ? "searching" : "no matches";
    return `${search.index} of ${search.total}${search.running ? "+" : ""}`;
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
      if (await runOpenCheckIfRequested({ path: () => openPathName })) return;

      await runSessionCheckIfRequested({
        open: (path) => openPath(path),
        viewer: () => viewer,
        root: () => surface,
        path: () => openPathName,
        sidebarShown: () => sidebarShown,
        toggleSidebar,
        flush: () => places.flush(),
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
  function openPath(path: string, resuming = false): Promise<void> {
    // Both arms, so one document failing to open does not stop the next.
    openChain = openChain.then(
      () => openDocument(path, resuming),
      () => openDocument(path, resuming),
    );
    return openChain;
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
  async function openDocument(path: string, resuming = false) {
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
      title = path.split("/").pop() ?? path;
      openPathName = path;
      openPageCount = doc.page_count;

      // Fitted to the document as it is now, not as it was: the file may have
      // been rebuilt shorter since, and a viewer scrolled past its own last page
      // is a worse answer than the wrong page.
      const remembered = session.places.find((kept) => kept.path === path);
      const resume = remembered ? clampPlace(remembered, doc.page_count) : null;
      sidebarShown = resume ? resume.sidebar : sidebarShown;

      // The host element does not exist until the viewer section is in the
      // DOM, and it is not while the empty-state placeholder is showing.
      await new Promise(requestAnimationFrame);
      if (!surface || !sidebarHost) throw new Error("no surface to mount into");

      query = "";
      sidebar = new Sidebar(sidebarHost, {
        onNavigate: (target, top) => {
          viewer?.goToDestination(target, top);
          viewer?.focus();
        },
        pages: {
          doc: doc.id,
          pageCount: doc.page_count,
          page,
          // The viewer is created below, so the strip reaches it lazily rather
          // than being handed a reference that does not exist yet.
          tier1: { placeholderFor: (at) => viewer?.placeholderFor(at) ?? null },
          onNavigate: (at) => {
            viewer?.goToPage(at);
            viewer?.focus();
          },
        },
      });
      sidebar.setVisible(sidebarShown);

      viewer = new Viewer(surface, {
        doc: doc.id,
        pageCount: doc.page_count,
        page,
        onStatus: (next) => {
          status = next;
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
          if (result && openDoc === wanted) sidebar?.setOutline(result);
        })
        .catch(() => {
          if (openDoc === wanted) sidebar?.setOutline(null);
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
   * docs/PLAN.md section 9 records that closing the A0 vector page against
   * "never below the tier-1 placeholder" left the user owed a degraded state:
   * that page is legitimately blurry for seconds at a time, and a viewer that
   * says nothing about it is indistinguishable from one that is broken. The two
   * failures are different and are reported as different --- `any` is whether
   * there is a page at all, `sharp` is whether it can be read.
   *
   * The thresholds are just short of 1 rather than at it because coverage is a
   * ratio of areas: a tile boundary that lands a rounding step inside the
   * viewport leaves a fraction of a percent uncovered on a page that is fully
   * rendered, and a status line that flickers on that is worse than none.
   */
  const degraded = $derived.by(() => {
    if (!status) return null;
    // First, because it is the one state waiting does not fix. "preparing page"
    // in front of a renderer that is erroring on every request is a lie by
    // omission --- the honest failures here were previously invisible, since
    // every `catch` in the scroller discarded them whole.
    if (status.failed > 0) return "some pages could not be drawn";
    if (status.any < 0.999) return "preparing page";
    if (status.sharp < 0.999) return "sharpening";
    return status.pending > 0 ? "loading ahead" : null;
  });
</script>

<svelte:window onkeydown={onWindowKey} />

<main>
  <header>
    <button onclick={pickAndOpen} disabled={opening}>Open</button>
    <span class="title">{title}</span>
    <span class="spacer"></span>
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
      {#if findLabel}<span class="stat">{findLabel}</span>{/if}
    {/if}
    {#if status}
      {#if degraded}
        <span class="degraded"
          >{degraded} — {Math.round(status.sharp * 100)}% sharp</span
        >
      {/if}
      {#if status.selected > 0}
        <span class="stat">{status.selected} selected</span>
      {/if}
      <span class="stat">{status.page} / {status.pageCount}</span>
      <span class="stat">{Math.round(status.zoom * 100)}%</span>
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
  .title {
    font-weight: 600;
  }
  .find {
    font: inherit;
    width: 14ch;
    padding: 0.15rem 0.5rem;
  }
  .spacer {
    flex: 1;
  }
  .stat,
  .degraded {
    font-variant-numeric: tabular-nums;
    opacity: 0.65;
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
