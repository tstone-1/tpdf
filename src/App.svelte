<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { fetchTile, type TileFormat } from "./lib/tiles";
  import { interleaved, type BenchResult } from "./lib/bench";
  import { runAutobenchIfRequested } from "./lib/autobench";

  interface PageSize {
    width_pt: number;
    height_pt: number;
  }
  interface DocumentInfo {
    id: number;
    pages: PageSize[];
    open_ms: number;
    at_ms: number;
  }

  let path = $state("");
  let doc = $state<DocumentInfo | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let log = $state<string[]>([]);
  let bench = $state<BenchResult | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);

  // Startup timeline (spike 0.2). The webview-side origin is stamped in
  // index.html before any module loads, so framework boot is not hidden inside
  // the first measurable interval.
  let startup = $state<Record<string, number>>({});

  $effect(() => {
    void (async () => {
      // Headless transfer benchmark, if TPDF_AUTOBENCH is set. Exits the
      // process when done, so nothing below runs.
      if (await runAutobenchIfRequested()) return;

      const elapsed = await invoke<number>("process_elapsed_ms");
      const scriptStart =
        (window as unknown as Record<string, number | undefined>)
          .__tpdfWebviewScriptStart ?? 0;
      startup = {
        "webview script start (ms into process)": elapsed - performance.now() + scriptStart,
        "app mounted (ms into process)": elapsed,
      };
    })();
  });

  function note(line: string) {
    log = [...log, line];
  }

  async function open() {
    error = null;
    busy = true;
    try {
      doc = await invoke<DocumentInfo>("open_document", { path });
      note(
        `opened ${doc.pages.length} pages in ${doc.open_ms.toFixed(1)} ms ` +
          `(at ${doc.at_ms.toFixed(0)} ms into process)`,
      );
      await drawFirstPage();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  /** Renders page 0 at fit-width into the canvas, tile by tile. */
  async function drawFirstPage() {
    if (!doc || !canvas) return;
    const page = doc.pages[0];
    if (!page) return;

    const scale = 1.5;
    const width = Math.round(page.width_pt * scale);
    const height = Math.round(page.height_pt * scale);
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const t0 = performance.now();
    const tile = 512;
    for (let y = 0; y < height; y += tile) {
      for (let x = 0; x < width; x += tile) {
        const w = Math.min(tile, width - x);
        const h = Math.min(tile, height - y);
        const result = await fetchTile({
          doc: doc.id,
          page: 0,
          scale,
          x,
          y,
          width: w,
          height: h,
          format: "raw",
        });
        ctx.drawImage(result.bitmap, x, y);
        result.bitmap.close();
      }
    }
    note(`first page painted in ${(performance.now() - t0).toFixed(1)} ms`);
  }

  /**
   * Interleaved A/B of the two transfer formats at two tile sizes.
   * Renders a full page per sample so the comparison is over realistic work.
   */
  async function runBench() {
    if (!doc) return;
    busy = true;
    bench = null;
    try {
      const page = doc.pages[0];
      if (!page) return;
      const scale = 2.0;
      const width = Math.round(page.width_pt * scale);
      const height = Math.round(page.height_pt * scale);

      const sweep = (format: TileFormat, tile: number) => async () => {
        let bytes = 0;
        let renderUs = 0;
        let decodeMs = 0;
        let count = 0;
        for (let y = 0; y < height; y += tile) {
          for (let x = 0; x < width; x += tile) {
            const result = await fetchTile({
              doc: doc!.id,
              page: 0,
              scale,
              x,
              y,
              width: Math.min(tile, width - x),
              height: Math.min(tile, height - y),
              format,
            });
            bytes += result.bytes;
            renderUs += result.renderUs;
            decodeMs += result.decodeMs;
            count++;
            result.bitmap.close();
          }
        }
        return { bytes, renderUs, decodeMs, tiles: count };
      };

      bench = await interleaved(
        {
          "raw/512": sweep("raw", 512),
          "png/512": sweep("png", 512),
          "raw/1024": sweep("raw", 1024),
          "png/1024": sweep("png", 1024),
        },
        5,
      );
      note("benchmark complete");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<main>
  <h1>tpdf — Phase 0 spike</h1>

  <section class="controls">
    <input
      bind:value={path}
      placeholder="/absolute/path/to/file.pdf"
      spellcheck="false"
      onkeydown={(e) => e.key === "Enter" && open()}
    />
    <button onclick={open} disabled={busy || !path}>Open</button>
    <button onclick={runBench} disabled={busy || !doc}>Run A/B benchmark</button>
  </section>

  {#if error}
    <pre class="error">{error}</pre>
  {/if}

  <section class="timeline">
    <h2>Startup</h2>
    <table>
      <tbody>
        {#each Object.entries(startup) as [label, value] (label)}
          <tr><td>{label}</td><td class="num">{value.toFixed(1)}</td></tr>
        {/each}
      </tbody>
    </table>
  </section>

  {#if bench}
    <section>
      <h2>Interleaved A/B — 5 rounds, full page at 2.0x</h2>
      <table>
        <thead>
          <tr><th>variant</th><th>median ms</th><th>min</th><th>max</th><th>tiles</th><th>KB</th><th>render ms</th><th>decode ms</th></tr>
        </thead>
        <tbody>
          {#each bench.stats as s (s.variant)}
            <tr>
              <td>{s.variant}</td>
              <td class="num">{s.median.toFixed(1)}</td>
              <td class="num">{s.min.toFixed(1)}</td>
              <td class="num">{s.max.toFixed(1)}</td>
              <td class="num">{(s.counters.tiles ?? 0).toFixed(0)}</td>
              <td class="num">{((s.counters.bytes ?? 0) / 1024).toFixed(0)}</td>
              <td class="num">{((s.counters.renderUs ?? 0) / 1000).toFixed(1)}</td>
              <td class="num">{(s.counters.decodeMs ?? 0).toFixed(1)}</td>
            </tr>
          {/each}
        </tbody>
      </table>

      <h3>Pairwise vs {bench.pairwise[0]?.a ?? "baseline"} (per round; &lt;1 is faster)</h3>
      <table>
        <tbody>
          {#each bench.pairwise as p (p.b)}
            <tr>
              <td>{p.b}</td>
              <td class="num">{p.medianRatio.toFixed(3)}</td>
              <td class="ratios">{p.ratios.map((r) => r.toFixed(3)).join("  ")}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/if}

  <section>
    <h2>Log</h2>
    <pre>{log.join("\n")}</pre>
  </section>

  <section>
    <h2>Page 1</h2>
    <canvas bind:this={canvas}></canvas>
  </section>
</main>

<style>
  main {
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
    padding: 1.5rem;
    max-width: 1100px;
    margin: 0 auto;
  }
  h1 { font-size: 1.1rem; }
  h2 { font-size: 0.95rem; margin-top: 1.5rem; }
  h3 { font-size: 0.85rem; font-weight: 600; }
  .controls { display: flex; gap: 0.5rem; margin: 1rem 0; }
  input { flex: 1; padding: 0.4rem 0.6rem; font: inherit; }
  button { padding: 0.4rem 0.9rem; font: inherit; }
  table { border-collapse: collapse; width: 100%; }
  td, th { padding: 0.15rem 0.5rem; text-align: left; border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent); }
  .num { text-align: right; font-variant-numeric: tabular-nums; }
  .ratios { opacity: 0.6; }
  .error { color: #c0392b; white-space: pre-wrap; }
  pre { white-space: pre-wrap; }
  canvas { max-width: 100%; border: 1px solid color-mix(in srgb, currentColor 20%, transparent); }
</style>
