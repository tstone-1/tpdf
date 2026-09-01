/**
 * Sustained-scroll frame rate (spike 0.8).
 *
 * Phase 0's exit criterion has two halves. The first --- first page presented
 * under 300 ms warm --- was answered by spike 0.2. This is the second: "no
 * dropped frames on sustained scroll", at 100% and 400%.
 *
 * Three things have to be established before a frame number means anything,
 * and each of them is measured here rather than assumed:
 *
 *  - **The display's cadence.** "60 fps" is a guess on a ProMotion panel, and a
 *    drop threshold derived from a guess reports drops that are not drops. An
 *    idle animation loop is timed first and everything is stated against it.
 *  - **The clock's resolution.** `performance.now()` is clamped in this webview
 *    (docs/PLAN.md section 3 saw it at 1 ms). At a 8.3 ms frame that quantises
 *    every interval, so the resolution is probed and printed, and no claim
 *    finer than it is made.
 *  - **Whether anything was actually painted.** A scroller that draws nothing
 *    holds any frame rate you like. Coverage --- the visible page area backed by
 *    a sharp tile --- is reported next to the frame times, and a smooth run with
 *    low coverage is a failure, not a pass.
 *
 * Two limitations to state rather than bury. The scroll is driven from
 * JavaScript, one step per animation frame; a real trackpad flick can be
 * handled on the compositor thread without waking script at all, so this is the
 * pessimistic case rather than the typical one. And each round scrolls over
 * tiles that have not been rendered yet, because tier 2 is cleared between
 * rounds --- a second pass over cached tiles is a different, easier question.
 */

import { call, type DocumentInfo, type PageSize } from "./ipc";
import { Scroller, type Layout } from "./scroller";

export interface ScrollBenchConfig {
  path: string;
  rounds: number;
  frames: number;
  warmup_frames: number;
  px_per_frame: number;
  tile_px: number;
  zooms: number[];
  layouts: string[];
  cache_tiles: number;
  max_in_flight: number;
  prefetch_screens: number;
  cancels: number[];
}

/** One variant: a layout at a zoom, with its own persistent scroller. */
interface Variant {
  label: string;
  layout: Layout;
  zoom: number;
  scroller: Scroller;
  host: HTMLDivElement;
}

/** What one round of one variant measured. */
interface Round {
  label: string;
  round: number;
  /** Frame-to-frame intervals in ms, from the animation callback's timestamp. */
  intervals: number[];
  /** Time spent inside our own per-frame work, in ms. */
  callbacks: number[];
  /** Mean fraction of the visible page area backed by a sharp tile. */
  coverage: number;
  /** Mean fraction backed by anything, tier-1 placeholder included. */
  anyCoverage: number;
  /**
   * Worst single frame's `any` coverage.
   *
   * The criterion says the page is *never* below its tier-1 placeholder, which
   * is a claim about the minimum. A mean that rounds to 100% is consistent with
   * a frame that showed nothing, so reporting only the mean would let the
   * criterion be declared met by a statistic that cannot test it.
   */
  anyFloor: number;
  /** Frames the warm-up took, and what it had reached when it stopped. */
  warmupFrames: number;
  warmupCoverage: number;
  /** Wall time of the whole timed section, for a quantisation-free rate. */
  wallMs: number;
  delivered: number;
  discarded: number;
  /** Withdrawn before the renderer finished them. */
  abandoned: number;
  requested: number;
  megabytes: number;
  renderMs: number;
  decodeMs: number;
}

/** How long a variant may take to drain before the next one starts anyway. */
const SETTLE_FRAMES = 900;

function nextFrame(): Promise<number> {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

/**
 * Waits for a variant's outstanding requests to leave the shared renderer.
 *
 * Every variant queues into the same single render thread, and a backlog
 * outlives the round that created it: on the A0 corpus, whichever variant ran
 * first reached full tier-1 coverage and whichever ran second reached 75%,
 * whatever the variants actually were. Swapping them swapped the result, which
 * is how it was found. Bounded rather than unbounded, and it reports what it
 * gave up on, because a variant that withdraws nothing can genuinely take four
 * seconds a tile to finish draining.
 */
async function settle(variant: Variant, frames: number): Promise<number> {
  for (let frame = 0; frame < frames; frame++) {
    if (variant.scroller.outstanding === 0) return frame;
    await nextFrame();
  }
  return frames;
}

function quantile(values: number[], q: number): number {
  if (values.length === 0) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.round(q * (sorted.length - 1))),
  );
  return sorted[index] ?? NaN;
}

function mean(values: number[]): number {
  if (values.length === 0) return NaN;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function pad(text: string, width: number, right = false): string {
  return right ? text.padStart(width) : text.padEnd(width);
}

/**
 * Smallest non-zero step this clock can report.
 *
 * Spin until the value changes, several times over. A clamped clock returns the
 * clamp; an unclamped one returns something near its true tick, and either way
 * the number below which no claim can be made is printed rather than guessed.
 */
function clockResolutionMs(): number {
  let smallest = Infinity;
  for (let sample = 0; sample < 32; sample++) {
    const start = performance.now();
    let now = start;
    while (now === start) now = performance.now();
    smallest = Math.min(smallest, now - start);
  }
  return smallest;
}

/**
 * Times an idle animation loop to find the presentation cadence.
 *
 * Doing nothing is the point: whatever this reports is the best the display and
 * the compositor will do for us, and every frame time later is stated as a
 * multiple of it.
 */
async function calibrateCadence(frames: number): Promise<number[]> {
  const intervals: number[] = [];
  let previous = await nextFrame();
  for (let frame = 0; frame < frames; frame++) {
    const now = await nextFrame();
    intervals.push(now - previous);
    previous = now;
  }
  return intervals;
}

/**
 * Scrolls for `frames` frames, recording each one.
 *
 * Direction reverses at both ends of the document rather than stopping. The
 * vector corpus is a single page and runs out of scroll in well under a round;
 * a run that stopped there would report the frame rate of a stationary
 * viewport, which is not what is being asked.
 */
async function scrollRound(
  variant: Variant,
  config: ScrollBenchConfig,
  round: number,
): Promise<Round> {
  const { scroller } = variant;

  // Cleared once, here. An earlier version cleared again after the warm-up, to
  // be sure the timed section scrolled over unrendered content --- and that
  // left the warm-up's requests outstanding but invalidated, so with four
  // in flight against a one-second page the timed section could not issue a
  // single new request and reported a flawless 60 fps over nothing at all.
  // Clearing between rounds is enough: everything below the first screen is
  // unrendered either way, which is the case the criterion is about.
  scroller.clearTiles();
  scroller.resetStats();

  // Untimed: let the first screen fill and the layer tree settle, so the timed
  // section measures scrolling rather than the first paint after a variant is
  // switched in. Bounded by frames as well as by coverage, because on a page
  // whose every tile costs a second the first screen may never fill --- and a
  // warm-up that waited for it would hang instead of reporting that.
  let warmupFrames = 0;
  for (; warmupFrames < config.warmup_frames; warmupFrames++) {
    await nextFrame();
    scroller.frame(0);
    if (scroller.coverage().sharp >= 0.999) break;
  }
  const warmupCoverage = scroller.coverage();

  scroller.resetStats();

  const intervals: number[] = [];
  const callbacks: number[] = [];
  const coverages: number[] = [];
  const anyCoverages: number[] = [];

  let scrollTop = 0;
  let direction = 1;
  let previous = await nextFrame();
  const wallStart = previous;

  for (let frame = 0; frame < config.frames; frame++) {
    const timestamp = await nextFrame();
    intervals.push(timestamp - previous);
    previous = timestamp;

    scrollTop += direction * config.px_per_frame;
    if (scrollTop >= scroller.maxScroll) {
      scrollTop = scroller.maxScroll;
      direction = -1;
    } else if (scrollTop <= 0) {
      scrollTop = 0;
      direction = 1;
    }

    const start = performance.now();
    const stats = scroller.frame(scrollTop);
    callbacks.push(performance.now() - start);
    coverages.push(stats.sharp);
    anyCoverages.push(stats.any);
  }

  return {
    label: variant.label,
    round,
    intervals,
    callbacks,
    coverage: mean(coverages),
    anyCoverage: mean(anyCoverages),
    anyFloor: Math.min(...anyCoverages),
    warmupFrames,
    warmupCoverage: warmupCoverage.sharp,
    wallMs: previous - wallStart,
    delivered: scroller.stats.delivered,
    discarded: scroller.stats.discarded,
    abandoned: scroller.stats.abandoned,
    requested: scroller.stats.requested,
    megabytes: scroller.stats.bytes / (1024 * 1024),
    renderMs: scroller.stats.renderMs,
    decodeMs: scroller.stats.decodeMs,
  };
}

/** Builds one host element and scroller per variant, all but one hidden. */
function buildVariants(
  stage: HTMLElement,
  config: ScrollBenchConfig,
  doc: DocumentInfo,
  page: PageSize,
  viewport: { width: number; height: number },
): Variant[] {
  const variants: Variant[] = [];

  for (const layout of config.layouts) {
    for (const zoom of config.zooms) {
      for (const cancel of config.cancels) {
        const host = document.createElement("div");
        host.style.display = "none";
        stage.appendChild(host);

        variants.push({
          // Only labelled when there is more than one to tell apart, so the
          // ordinary single-variant run keeps its established row names.
          label:
            config.cancels.length > 1
              ? `${layout}/${zoom.toFixed(0)}x/${cancel ? "cancel" : "plain"}`
              : `${layout}/${zoom.toFixed(0)}x`,
          layout: layout as Layout,
          zoom,
          host,
          scroller: new Scroller(host, {
            doc: doc.id,
            pageCount: doc.page_count,
            // Page 1 alone, and nothing here learns the rest. The benchmark
            // opens one corpus at a time and every one of them is uniform, so a
            // learning channel would add a variable to a measurement rather than
            // correctness to it --- and the frame cost this measures is the same
            // whichever size the pages are.
            pages: [page],
            zoom,
            // Upright: the benchmark measures scrolling, and a rotation would
            // add a dimension to a table that already has three.
            turns: 0,
      // The benchmark measures the light path. Inversion is a per-pixel pass
      // over the tile in the renderer, so it would be a variant dimension of
      // its own rather than a constant here, and nothing has asked for that
      // number yet.
      invert: false,
            layout: layout as Layout,
            tilePx: config.tile_px,
            dpr: window.devicePixelRatio,
            viewport,
            prefetchScreens: config.prefetch_screens,
            cacheTiles: config.cache_tiles,
            maxInFlight: config.max_in_flight,
            cancel: cancel !== 0,
          }),
        });
      }
    }
  }

  return variants;
}

function report(
  config: ScrollBenchConfig,
  doc: DocumentInfo,
  page: PageSize,
  viewport: { width: number; height: number },
  baselineMs: number,
  idle: number[],
  resolutionMs: number,
  rounds: Round[],
  log: (line?: string) => void,
): void {
  const labels = [...new Set(rounds.map((r) => r.label))];

  log(`file            ${config.path}`);
  log(`pages           ${doc.page_count}`);
  log(
    `page 1          ${page.width_pt.toFixed(0)} x ${page.height_pt.toFixed(0)} pt`,
  );
  log(
    `viewport        ${viewport.width} x ${viewport.height} css px @ dpr ${window.devicePixelRatio}`,
  );
  log(
    `tile            ${config.tile_px}² device px, ${config.cache_tiles} resident, ${config.max_in_flight} in flight`,
  );
  log(
    `withdrawal      ${config.cancels.map((c) => (c ? "on" : "off")).join(", ")}`,
  );
  log(
    `scroll          ${config.px_per_frame} css px/frame, ${config.frames} frames x ${config.rounds} rounds`,
  );
  log();
  log(
    `clock step      ${resolutionMs.toFixed(3)} ms  (no claim below this is supportable)`,
  );
  // Both are controls on the cadence below. WebKit throttles a page whose
  // window is not visible and can slow one that is not focused, either of
  // which would read as a platform ceiling rather than as a benchmark that was
  // not looking at the screen.
  log(
    `window          ${document.visibilityState}, ` +
      `focus ${document.hasFocus() ? "yes" : "NO --- cadence below may be throttled"}`,
  );
  log(
    `idle cadence    ${quantile(idle, 0.5).toFixed(2)} ms median ` +
      `(${(1000 / quantile(idle, 0.5)).toFixed(0)} Hz), ` +
      `min ${Math.min(...idle).toFixed(2)}, max ${Math.max(...idle).toFixed(2)}`,
  );
  log(
    `drop threshold  > ${(baselineMs * 1.5).toFixed(2)} ms; stall > ${(baselineMs * 2.5).toFixed(2)} ms`,
  );
  log();

  const header = [
    pad("variant", 16),
    pad("fps", 7, true),
    pad("med", 7, true),
    pad("p95", 7, true),
    pad("p99", 7, true),
    pad("max", 8, true),
    pad("drops", 7, true),
    pad("stalls", 7, true),
    pad("cb mean", 8, true),
    pad("cb max", 7, true),
    pad("sharp", 7, true),
    pad("any", 6, true),
    pad("floor", 6, true),
    pad("tiles", 7, true),
    pad("cut", 6, true),
    pad("abd", 6, true),
    pad("MB/s", 7, true),
  ].join(" ");
  log(header);
  log("-".repeat(header.length));

  for (const label of labels) {
    const mine = rounds.filter((r) => r.label === label);
    const intervals = mine.flatMap((r) => r.intervals);
    const callbacks = mine.flatMap((r) => r.callbacks);
    const wallMs = mine.reduce((sum, r) => sum + r.wallMs, 0);
    const frames = mine.reduce((sum, r) => sum + r.intervals.length, 0);
    const drops = intervals.filter((ms) => ms > baselineMs * 1.5).length;
    const stalls = intervals.filter((ms) => ms > baselineMs * 2.5).length;
    const megabytes = mine.reduce((sum, r) => sum + r.megabytes, 0);

    log(
      [
        pad(label, 16),
        // From the whole timed section rather than from the median interval:
        // it is one subtraction over seconds, so the clock's clamp cannot
        // reach it.
        pad(((frames / wallMs) * 1000).toFixed(1), 7, true),
        pad(quantile(intervals, 0.5).toFixed(1), 7, true),
        pad(quantile(intervals, 0.95).toFixed(1), 7, true),
        pad(quantile(intervals, 0.99).toFixed(1), 7, true),
        pad(Math.max(...intervals).toFixed(1), 8, true),
        pad(drops.toString(), 7, true),
        pad(stalls.toString(), 7, true),
        // Mean rather than median: a per-frame callback here costs a fraction
        // of the clock's 1 ms step, so every individual sample is 0 or 1 and
        // the median is only ever one of those. Averaging hundreds of them
        // recovers a usable figure from a clock that cannot resolve one.
        pad(mean(callbacks).toFixed(2), 8, true),
        pad(Math.max(...callbacks).toFixed(2), 7, true),
        pad(
          `${(mean(mine.map((r) => r.coverage)) * 100).toFixed(0)}%`,
          7,
          true,
        ),
        pad(
          `${(mean(mine.map((r) => r.anyCoverage)) * 100).toFixed(0)}%`,
          6,
          true,
        ),
        // The worst frame of the worst round, not the mean of the minima: one
        // blank frame anywhere fails the criterion, and averaging them would
        // hide it behind the rounds that were fine.
        pad(
          `${(Math.min(...mine.map((r) => r.anyFloor)) * 100).toFixed(0)}%`,
          6,
          true,
        ),
        pad(
          (mine.reduce((sum, r) => sum + r.delivered, 0) / mine.length).toFixed(
            0,
          ),
          7,
          true,
        ),
        pad(
          (mine.reduce((sum, r) => sum + r.discarded, 0) / mine.length).toFixed(
            0,
          ),
          6,
          true,
        ),
        // `cut` is work that was paid for and thrown away; `abd` is work that
        // was withdrawn before it finished. The second rising as the first
        // falls is the whole point of the queue being cancellable, and neither
        // number alone shows it.
        pad(
          (mine.reduce((sum, r) => sum + r.abandoned, 0) / mine.length).toFixed(
            0,
          ),
          6,
          true,
        ),
        pad(((megabytes / wallMs) * 1000).toFixed(0), 7, true),
      ].join(" "),
    );
  }

  log();
  log("per-round drops, to separate a warm-up outlier from a steady state:");
  for (const label of labels) {
    const series = rounds
      .filter((r) => r.label === label)
      .sort((a, b) => a.round - b.round)
      .map((r) =>
        r.intervals
          .filter((ms) => ms > baselineMs * 1.5)
          .length.toString()
          .padStart(3),
      )
      .join(" ");
    log(`  ${pad(label, 16)} ${series}`);
  }

  log();
  log("server-side render and client-side decode per round, ms:");
  for (const label of labels) {
    const mine = rounds.filter((r) => r.label === label);
    log(
      `  ${pad(label, 16)} render ${pad(mean(mine.map((r) => r.renderMs)).toFixed(0), 7, true)}` +
        `   decode ${pad(mean(mine.map((r) => r.decodeMs)).toFixed(0), 7, true)}` +
        `   requested ${pad(mean(mine.map((r) => r.requested)).toFixed(0), 5, true)}`,
    );
  }

  log();
  log("warm-up before each timed section (frames, and the coverage reached):");
  for (const label of labels) {
    const mine = rounds.filter((r) => r.label === label);
    log(
      `  ${pad(label, 16)} ${pad(mean(mine.map((r) => r.warmupFrames)).toFixed(0), 4, true)} frames` +
        `   ${pad(`${(mean(mine.map((r) => r.warmupCoverage)) * 100).toFixed(0)}%`, 5, true)} sharp` +
        // A timed section that began without a full first screen was measured
        // from a state no user would have waited through, and its frame times
        // describe a scroller that is still catching up rather than one that
        // is keeping up.
        (mean(mine.map((r) => r.warmupCoverage)) < 0.999
          ? "   (never filled)"
          : ""),
    );
  }
}

/**
 * Runs the scroll benchmark if `TPDF_SCROLLBENCH` is set, then exits.
 *
 * Returns false when none was requested, so the spike UI carries on.
 */
export async function runScrollBenchIfRequested(): Promise<boolean> {
  const config = await call("scrollbench_config");
  if (!config) return false;

  const lines: string[] = [];
  const log = (line = "") => lines.push(line);

  try {
    const resolutionMs = clockResolutionMs();

    const doc = await call("open_document", {
      path: config.path,
    });
    const page = doc.pages[0];
    if (!page) throw new Error("document has no pages");

    // The whole window, minus nothing: the stage covers the page so no other
    // element is competing for the compositor.
    const stage = document.createElement("div");
    stage.style.cssText = "position:fixed;inset:0;margin:0;background:#222;";
    document.body.replaceChildren(stage);
    const viewport = { width: window.innerWidth, height: window.innerHeight };

    const idle = await calibrateCadence(120);
    const baselineMs = quantile(idle, 0.5);

    const variants = buildVariants(stage, config, doc, page, viewport);
    const rounds: Round[] = [];

    let unsettled = 0;
    for (let round = 0; round < config.rounds; round++) {
      for (const variant of variants) {
        for (const other of variants) other.host.style.display = "none";
        variant.host.style.display = "block";
        rounds.push(await scrollRound(variant, config, round));
        // After the round, not before: the backlog to drain is the one this
        // round just created, and draining it here is what makes the next
        // variant's numbers its own.
        if ((await settle(variant, SETTLE_FRAMES)) >= SETTLE_FRAMES)
          unsettled++;
      }
    }

    report(
      config,
      doc,
      page,
      viewport,
      baselineMs,
      idle,
      resolutionMs,
      rounds,
      log,
    );

    await call("spike_print", { text: lines.join("\n") });
    await call("spike_exit", { code: 0 });
  } catch (error) {
    await call("spike_print", {
      text: `[ERROR] scrollbench: ${error instanceof Error ? error.message : String(error)}`,
    });
    await call("spike_exit", { code: 1 });
  }

  return true;
}
