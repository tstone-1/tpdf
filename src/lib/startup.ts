/**
 * The startup timeline (spike 0.2), run without a human.
 *
 * docs/PLAN.md commits to "cold start to first page painted under 300 ms". This
 * measures that end to end and, more usefully, breaks it into named milestones
 * so a miss can be attributed rather than merely observed.
 *
 * With `TPDF_STARTUP=<file.pdf>` set, the app opens the document, renders the
 * region of page 1 that the window would actually show, waits for the
 * compositor to present it, prints the timeline and exits. One shell command per
 * launch, so cold and warm runs are the same measurement repeated.
 *
 * Two things this deliberately does not do:
 *
 *  - It does not measure under `tauri dev`. The frontend there is served by a
 *    Vite dev server over HTTP, so the numbers would describe Vite. Use a built
 *    app.
 *  - It does not treat "the canvas has been drawn into" as painted. A
 *    `drawImage` call returns long before the compositor has put anything on
 *    screen, and the user's 300 ms ends at the glass.
 */

import { calibrateProcessClock } from "./clock";
import { call } from "./ipc";
import { fetchRequiredTile } from "./tiles";

/** Largest tile dimension to ask for, per the spike 0.1 tile-size finding. */
const MAX_TILE = 4096;

function pad(text: string, width: number, right = false): string {
  return right ? text.padStart(width) : text.padEnd(width);
}

/**
 * Resolves once the compositor has presented the frame containing whatever was
 * just drawn.
 *
 * A single `requestAnimationFrame` fires *before* the frame it belongs to is
 * painted, so it is the wrong signal. The second callback runs at the start of
 * the following frame, by which time the previous one has been committed --
 * this is the closest a webview gets to a presentation timestamp, and it is an
 * upper bound: the true present happened somewhere between the two callbacks.
 */
function afterPresentation(): Promise<number> {
  return new Promise((resolve, reject) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve(performance.now()));
    });
    // A webview whose window is occluded or whose display has slept stops
    // firing requestAnimationFrame entirely, and the run then hangs with no
    // output rather than failing. That is the worst shape a measurement can
    // fail in, so it is turned into an error. setTimeout keeps running.
    setTimeout(
      () => reject(new Error("no frame presented in 10 s -- is the display asleep?")),
      10_000,
    );
  });
}

/** First contentful paint of the shell, as the compositor reported it. */
function firstContentfulPaint(): number | null {
  const entry = performance
    .getEntriesByType("paint")
    .find((e) => e.name === "first-contentful-paint");
  return entry ? entry.startTime : null;
}

/**
 * Milestones the webview recorded for itself, read late.
 *
 * Read at the end of the run rather than when they happen: `loadEventEnd` is
 * zero until the load event has fired, and the interesting question is whether
 * the interval after the first script is our module graph or the page still
 * loading. Reading late gets both, with historical timestamps either way.
 *
 * The module entry is looked up by suffix rather than by name because Vite
 * hashes it.
 */
export function navigationMarks(): [string, number][] {
  const marks: [string, number][] = [];
  const [navigation] = performance.getEntriesByType(
    "navigation",
  ) as PerformanceNavigationTiming[];

  if (navigation) {
    if (navigation.responseEnd > 0) marks.push(["html received", navigation.responseEnd]);
    if (navigation.domContentLoadedEventEnd > 0) {
      marks.push(["dom content loaded", navigation.domContentLoadedEventEnd]);
    }
    if (navigation.loadEventEnd > 0) marks.push(["window load", navigation.loadEventEnd]);
  }

  const script = (performance.getEntriesByType("resource") as PerformanceResourceTiming[])
    .filter((entry) => entry.name.endsWith(".js"))
    .sort((a, b) => a.responseEnd - b.responseEnd)
    .pop();
  if (script) {
    marks.push(["module requested", script.startTime]);
    marks.push(["module received", script.responseEnd]);
  }

  return marks;
}

/**
 * Runs the startup timeline if `TPDF_STARTUP` is set, then exits the process.
 *
 * Returns false when no run was requested, so the normal spike UI carries on.
 */
export async function runStartupTimelineIfRequested(): Promise<boolean> {
  const path = await call("startup_path");
  // Stamped rather than marked: the mapping that turns it into a process
  // timestamp does not exist yet. The first IPC of a launch is much dearer than
  // the rest, and it is charged to whichever variant happens to make it first,
  // so it needs its own line rather than hiding inside framework boot.
  const firstIpcReturned = performance.now();
  if (!path) return false;

  const lines: string[] = [];
  const log = (line = "") => lines.push(line);

  try {
    const clock = await calibrateProcessClock();
    const calibrated = performance.now();
    const globals = window as unknown as Record<string, number | undefined>;

    /** Records a webview-observed milestone, converted onto the process timeline. */
    const mark = (name: string, perfNow: number) =>
      call("startup_mark", { name, atMs: clock.toProcessMs(perfNow) });

    const scriptStart = globals.__tpdfWebviewScriptStart;
    if (scriptStart !== undefined) await mark("webview script start", scriptStart);

    const fcp = firstContentfulPaint();
    if (fcp !== null) await mark("webview first paint", fcp);

    const appMounted = globals.__tpdfAppMounted;
    if (appMounted !== undefined) await mark("app mounted", appMounted);

    await mark("first ipc returned", firstIpcReturned);
    await mark("clock calibrated", calibrated);

    await mark("document open requested", performance.now());
    const info = await call("open_document", { path });

    const page = info.pages[0];
    if (!page) throw new Error("document has no pages");

    // The preview is the region the window would actually show at fit-width, in
    // device pixels -- not a thumbnail. A thumbnail would be a cheaper number
    // that does not correspond to anything the user sees.
    const dpr = window.devicePixelRatio || 1;
    const viewportWidth = Math.round(window.innerWidth * dpr);
    const viewportHeight = Math.round(window.innerHeight * dpr);
    const scale = viewportWidth / page.width_pt;
    const fullHeight = Math.round(page.height_pt * scale);

    const width = Math.min(viewportWidth, MAX_TILE);
    const height = Math.min(viewportHeight, fullHeight, MAX_TILE);

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    canvas.style.cssText = `position:fixed;inset:0;width:${width / dpr}px;height:${height / dpr}px;z-index:9999`;
    document.body.appendChild(canvas);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("no 2d context");

    const tile = await fetchRequiredTile({
      doc: info.id,
      page: 0,
      scale,
      x: 0,
      y: 0,
      width,
      height,
      format: "raw",
    });
    await mark("first preview bitmap ready", performance.now());

    context.drawImage(tile.bitmap, 0, 0);
    tile.bitmap.close();
    await mark("first page presented", await afterPresentation());

    for (const [name, at] of navigationMarks()) await mark(name, at);

    const preMain = await call("startup_pre_main_ms");
    const marks = await call("startup_timeline");

    log(`file            ${path}`);
    log(`variant         ${info.lazy_geometry ? "lazy geometry" : "full geometry"}`);
    log(`pages           ${info.page_count}`);
    log(`page 1          ${page.width_pt.toFixed(0)} x ${page.height_pt.toFixed(0)} pt`);
    log(`preview         ${width} x ${height} device px at ${scale.toFixed(3)}x (dpr ${dpr})`);
    log(
      `clock mapping   +/- ${clock.uncertaintyMs.toFixed(2)} ms ` +
        `(best ipc round trip ${clock.roundTripMs.toFixed(2)} ms)`,
    );
    log(
      preMain === null
        ? "timeline origin main entry -- pre-main interval NOT measurable on this platform"
        : `timeline origin process exec (${preMain.toFixed(1)} ms of it before main)`,
    );
    log();

    log([pad("milestone", 30), pad("ms", 9, true), pad("delta", 9, true)].join(" "));
    log("-".repeat(50));

    let previous = 0;
    for (const [name, at] of marks) {
      log(
        [pad(name, 30), pad(at.toFixed(1), 9, true), pad((at - previous).toFixed(1), 9, true)].join(
          " ",
        ),
      );
      previous = at;
    }

    log();
    log(`TIMELINE-JSON ${JSON.stringify({ preMainMs: preMain, marks })}`);

    await call("spike_print", { text: lines.join("\n") });
    await call("spike_exit", { code: 0 });
  } catch (error) {
    await call("spike_print", {
      text: `[ERROR] startup: ${error instanceof Error ? error.message : String(error)}`,
    });
    await call("spike_exit", { code: 1 });
  }

  return true;
}
