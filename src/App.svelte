<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { runAutobenchIfRequested } from "./lib/autobench";
  import { runScrollBenchIfRequested } from "./lib/scrollbench";
  import { runStartupTimelineIfRequested } from "./lib/startup";
  import { runViewerCheckIfRequested } from "./lib/viewercheck";
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
  let title = $state("");
  let error = $state<string | null>(null);
  let opening = $state(false);
  let status = $state<ViewerStatus | null>(null);
  let query = $state("");
  let findField = $state<HTMLInputElement | null>(null);

  let viewer: Viewer | null = null;
  let findTimer = 0;

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

  /** Cmd-F from anywhere puts the caret in the find field. */
  function onWindowKey(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || event.key !== "f") return;
    if (!title) return;
    event.preventDefault();
    findField?.focus();
    findField?.select();
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

      await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const [path] = event.payload.paths;
        if (path) void openPath(path);
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

  async function openPath(path: string) {
    error = null;
    opening = true;
    try {
      const doc = await invoke<DocumentInfo>("open_document", { path });
      const page = doc.pages[0];
      if (!page) throw new Error("document reports no pages");

      viewer?.destroy();
      viewer = null;
      status = null;
      title = path.split("/").pop() ?? path;

      // The host element does not exist until the viewer section is in the
      // DOM, and it is not while the empty-state placeholder is showing.
      await new Promise(requestAnimationFrame);
      if (!surface) throw new Error("no surface to mount into");

      query = "";
      viewer = new Viewer(surface, {
        doc: doc.id,
        pageCount: doc.page_count,
        page,
        onStatus: (next) => (status = next),
      });
      viewer.focus();
    } catch (e) {
      error = String(e);
      title = "";
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
    <div class="surface" bind:this={surface}></div>
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
  .surface {
    flex: 1;
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
