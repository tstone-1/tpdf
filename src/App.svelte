<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/autobench";
  import { runScrollBenchIfRequested } from "./lib/scrollbench";
  import { runStartupTimelineIfRequested } from "./lib/startup";
  import { runViewerCheckIfRequested } from "./lib/viewercheck";
  import { CommandRegistry } from "./lib/commands";
  import { Palette } from "./lib/palette";
  import { Sidebar, type Tab } from "./lib/sidebar";
  import type { Outline } from "./lib/outline";
  import { clampPlace, loadSession, SessionWriter, type Session } from "./lib/session";
  import { runSessionCheckIfRequested } from "./lib/sessioncheck";
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
  /** Document the sidebar belongs to, so a late outline for an old one is dropped. */
  let openDoc = -1;

  /** Path of the open document, which is what a remembered place is keyed on. */
  let openPathName = "";
  /** Its page count, so a place can record what the document had when written. */
  let openPageCount = 0;
  /** Places read at launch. Read once: the file is ours and nothing else writes it. */
  let session: Session = { places: [] };
  /** Collapses a scroll's worth of positions into at most one write per second. */
  const places = new SessionWriter();

  /**
   * Every command the application has, in one place.
   *
   * Built once and outliving any document, so the palette works before a file
   * is open --- "Open document" is the command someone reaches for first. The
   * rest guard on `viewer`, which is read at call time rather than captured, so
   * closing and opening a document does not need the registry rebuilt.
   *
   * The `keys` strings are labels the palette displays; the bindings themselves
   * live in `viewer.ts`'s key handler and in `onWindowKey` below. Nothing checks
   * that the two agree, which is a real gap and a small one --- a wrong label
   * teaches a wrong shortcut, it does not break a command.
   */
  const commands = new CommandRegistry();
  const withDocument = () => viewer !== null;

  commands.register(
    { id: "file.open", title: "Open document", keys: "⌘O", run: () => void pickAndOpen() },
    {
      id: "find.open",
      title: "Find in document",
      keys: "⌘F",
      enabled: withDocument,
      run: () => focusFind(),
    },
    {
      id: "find.next",
      title: "Find next",
      keys: "⌘G",
      enabled: withDocument,
      run: () => viewer?.nextMatch(),
    },
    {
      id: "find.previous",
      title: "Find previous",
      keys: "⇧⌘G",
      enabled: withDocument,
      run: () => viewer?.prevMatch(),
    },
    {
      id: "view.zoomIn",
      title: "Zoom in",
      keys: "⌘+",
      enabled: withDocument,
      run: () => viewer?.zoomStep(1),
    },
    {
      id: "view.zoomOut",
      title: "Zoom out",
      keys: "⌘−",
      enabled: withDocument,
      run: () => viewer?.zoomStep(-1),
    },
    {
      id: "view.fitWidth",
      title: "Fit width",
      keys: "⌘0",
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
      keys: "⌘R",
      enabled: withDocument,
      run: () => viewer?.rotateBy(1),
    },
    {
      id: "view.rotateCounterClockwise",
      title: "Rotate view anticlockwise",
      keys: "⌘L",
      enabled: withDocument,
      run: () => viewer?.rotateBy(-1),
    },
    {
      id: "nav.nextPage",
      title: "Next page",
      keys: "n",
      enabled: withDocument,
      run: () => viewer?.nextPage(),
    },
    {
      id: "nav.previousPage",
      title: "Previous page",
      keys: "p",
      enabled: withDocument,
      run: () => viewer?.previousPage(),
    },
    {
      id: "nav.firstPage",
      title: "Go to start",
      keys: "Home",
      enabled: withDocument,
      run: () => viewer?.goToStart(),
    },
    {
      id: "nav.lastPage",
      title: "Go to end",
      keys: "End",
      enabled: withDocument,
      run: () => viewer?.goToEnd(),
    },
    {
      id: "edit.selectAll",
      title: "Select all on page",
      keys: "⌘A",
      enabled: withDocument,
      run: () => viewer?.selectPage(),
    },
    {
      id: "edit.copy",
      title: "Copy selection",
      keys: "⌘C",
      enabled: withDocument,
      run: () => void viewer?.copySelection(),
    },
    {
      id: "edit.clearSelection",
      title: "Clear selection",
      keys: "Esc",
      enabled: withDocument,
      run: () => viewer?.clearSelection(),
    },
    {
      // Named for both things a reader might type. One command with one
      // binding rather than two commands sharing one, which would show the
      // same shortcut twice in the palette and teach that it does two things.
      id: "view.toggleSidebar",
      title: "Toggle sidebar",
      keys: "⌘\\",
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
  );

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

  /** The two shortcuts that belong to the window rather than to the surface. */
  function onWindowKey(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey)) return;
    if (event.key === "k") {
      event.preventDefault();
      // Toggling rather than reopening: Cmd-K on an open palette is a request
      // to get rid of it, not to clear the query someone is halfway through.
      if (palette?.isOpen) palette.close();
      else palette?.open();
    } else if (event.key === "f" && title) {
      event.preventDefault();
      focusFind();
    } else if (event.key === "\\" && title) {
      event.preventDefault();
      toggleSidebar();
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
      session = await loadSession();
      const resume = session.places[0];
      if (resume) await openPath(resume.path, true);

      // After the restore, not instead of it: what this checks is what the boot
      // above just did. Unlike the other harnesses it does not replace the
      // application --- see `sessioncheck.ts` for why it cannot.
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
   * Opens a document, putting the reader back where they left it.
   *
   * `resuming` is set only by the launch restore, and changes one thing: a
   * document that no longer opens is not an error to report. Someone who chose
   * a file and cannot have it needs to be told; someone who launched the app and
   * whose last document has since been deleted or unmounted needs an empty
   * window, not a dialog about a file they did not ask for.
   */
  async function openPath(path: string, resuming = false) {
    error = null;
    opening = true;
    try {
      const doc = await invoke<DocumentInfo>("open_document", { path });
      const page = doc.pages[0];
      if (!page) throw new Error("document reports no pages");

      // Whatever the outgoing document was owed, before its path is replaced.
      places.flush();
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
          notePlace();
        },
        onPosition: (at, top) => {
          sidebar?.setPosition(at, top);
          notePlace();
        },
      });
      // Before the first paint, so the reader sees their page rather than page
      // one and then a jump --- and before `focus`, which does not move the view
      // but would make the jump look like something they did.
      if (resume) viewer.restore(resume);
      viewer.focus();

      // After the viewer, deliberately not awaited, and deliberately not asked
      // for until the first screen is up.
      //
      // Not awaiting it was always right --- the outline shares the render
      // thread with tiles and a document that opens instantly should not wait
      // for its table of contents. Waiting for the first paint is newer, and is
      // there because the walk stopped being free: resolving a destination on a
      // page carrying `/Rotate` needs the page's rotation, `FPDFPage_GetRotation`
      // needs the page loaded, and that measured 0.17 ms -> 7.5 ms on a
      // twelve-page fixture, about 1 ms per distinct page named. On a book with
      // a three-hundred-entry table of contents that is a third of a second of
      // render thread, and the render thread is FIFO --- so asked for at open it
      // would sit in front of the tiles for the page someone is looking at.
      const wanted = doc.id;
      openDoc = wanted;
      await firstPaint();
      void invoke<Outline>("document_outline", { doc: wanted })
        .then((result) => {
          // Another document may have been opened while this was in flight, in
          // which case this outline belongs to a file nobody is looking at.
          if (openDoc === wanted) sidebar?.setOutline(result);
        })
        .catch(() => {
          if (openDoc === wanted) sidebar?.setOutline(null);
        });
    } catch (e) {
      openPathName = "";
      openPageCount = 0;
      title = "";
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
