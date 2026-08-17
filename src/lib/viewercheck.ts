/**
 * A functional check of the reading surface, run without a human.
 *
 * Everything else in `src/lib` that runs unattended is a *measurement*. This is
 * not: it asserts that the viewer does what it claims, because the alternative
 * is a class whose behaviour is only ever confirmed by someone looking at a
 * window --- and the repository has already paid twice for a result that was
 * produced by nothing happening (a crash test the optimizer deleted, a scroll
 * benchmark that issued no requests). Both looked like passes.
 *
 * Input is delivered as real `WheelEvent`s and `KeyboardEvent`s dispatched at
 * the viewer's own root, so what is exercised is the handler a trackpad
 * reaches, not a parallel path written for the test.
 *
 * Two checks carry their own control, for the same reason:
 *
 *  - **idling** is asserted in both directions. "Is idle after settling" passes
 *    trivially if the loop never starts, so it is paired with "is not idle
 *    while work is outstanding".
 *  - **coverage after a zoom** is asserted against a *drop* first. Recovering
 *    to 100% proves nothing if the zoom never invalidated anything.
 *  - **search** is asserted against a query that is not in the document, and its
 *    jump is asserted against not having been on the target page already.
 *
 * Run it with `TPDF_VIEWERCHECK=<file.pdf>`; it prints a table and exits
 * non-zero on the first failure.
 */

import { invoke } from "@tauri-apps/api/core";
import {
  handleWindowKey,
  registerAppCommands,
  type AppActions,
  type WindowKeyDeps,
} from "./appcommands";
import { frame, Report, settle as settleFor } from "./checkreport";
import { MULTI_CLICK_SLOP_PX } from "./clicks";
import { CommandRegistry } from "./commands";
import type { DocumentInfo, PageSize } from "./ipc";
import type { Comment, Comments } from "./comments";
import type { Link, Links } from "./links";
import { allRows, isNavigable, type Outline, type Row } from "./outline";
import { Palette } from "./palette";
import { MAX_RESULT_ROWS } from "./results";
import { PLAIN_SEARCH } from "./search";
import {
  hasSideBySideLines,
  readingLines,
  textOfRanges,
  usableRuns,
} from "./reading";
import { TextCache, type PageText } from "./text";
import { Sidebar } from "./sidebar";
import { fetchRequiredTile, tileUrl } from "./tiles";
import { OVERSCAN, rowHeightFor, type Thumbnails } from "./thumbnails";
import { SCROLLBAR_WIDTH, Viewer, type ViewerStatus } from "./viewer";

/**
 * Tabs the sidebar has: outline, pages, results, comments.
 *
 * Spelled out here rather than read from the sidebar, which would make the check
 * agree with whatever the sidebar happens to build. It went red on its own when
 * the results tab landed and again when the comments tab did, which is the check
 * working --- twice.
 */
const SIDEBAR_TABS = 4;

/** Size of the surface the check mounts, in CSS pixels. */
const WIDTH = 900;
const HEIGHT = 700;

/** How long any single wait may take before the check gives up. */
const TIMEOUT_MS = 30_000;

/**
 * Where every verdict below lands, and where every line is printed from.
 *
 * Shared with `sessioncheck.ts` and `opencheck.ts` rather than reimplemented
 * here. This file carried its own copy of the printing chain --- the same
 * `[OK]  `/`[FAIL]`/`[SKIP]` labels, the same chained `spike_print`, the same
 * summary arithmetic --- and by the time anyone compared them the two had
 * already drifted, which is precisely what the shared module exists to make
 * impossible.
 *
 * **The check names are untouched**, and they are the cross-platform invariant
 * `BUILD.md` records. Three cosmetic things about the transcript do change, none
 * of which any parser reads: the detail column moves from 40 to 46, a skip's
 * reason is joined with `---` rather than an em dash (so the whole line is
 * cp1252-safe), and the summary's tail is `, N not applicable` without the
 * trailing "to this document" --- which is how every other harness here already
 * words it, and how `BUILD.md` already quotes it.
 *
 * What the shared module encodes, and the reason it is not a local convenience:
 * results are printed **as they are recorded**, chained rather than awaited, so
 * a run that stops partway names where it got to instead of printing nothing.
 * An empty transcript is what a passing run looks like from outside the webview,
 * and telling those two apart cost an afternoon once already.
 */
const report = new Report();

const check = (name: string, ok: boolean, detail: string): void =>
  report.check(name, ok, detail);

/**
 * Records a check that this document cannot exercise.
 *
 * Printed rather than omitted. A control that quietly disappears on some inputs
 * is indistinguishable from one that ran, and the whole point of a control is
 * to know whether it did.
 */
const skip = (name: string, why: string): void => report.skip(name, why);

/**
 * Records a check that is satisfied by waiting, failing it on a timeout.
 *
 * A timeout is a failure of the thing being checked rather than a slow machine:
 * every wait here is for something that either happens within a second or is
 * broken. It is recorded rather than thrown so one broken behaviour does not
 * hide the state of the rest.
 */
async function eventually(
  name: string,
  predicate: () => boolean,
  detail: () => string,
): Promise<boolean> {
  const deadline = performance.now() + TIMEOUT_MS;
  while (!predicate()) {
    if (performance.now() > deadline) {
      check(name, false, `timed out — ${detail()}`);
      return false;
    }
    await frame();
  }
  check(name, true, detail());
  return true;
}

/**
 * Waits for a precondition of a later check, without recording one.
 *
 * The shared helper's verdict is deliberately dropped: the deadline expiring is
 * reported by the check that follows, which asserts the state this was waiting
 * for and fails naming it. Recording a second failure here would double-count
 * one broken behaviour in a summary two Python harnesses read arithmetic from.
 */
async function settle(predicate: () => boolean): Promise<void> {
  await settleFor(predicate, TIMEOUT_MS);
}

/** Dispatches a wheel event the way a trackpad would. */
function wheel(root: HTMLElement, deltaY: number, zoomGesture = false): void {
  root.dispatchEvent(
    new WheelEvent("wheel", {
      deltaY,
      deltaMode: 0,
      ctrlKey: zoomGesture,
      bubbles: true,
      cancelable: true,
    }),
  );
}

/** Dispatches a pointer event the way a trackpad or mouse would. */
function pointer(
  root: HTMLElement,
  type: "pointerdown" | "pointermove" | "pointerup",
  x: number,
  y: number,
): void {
  root.dispatchEvent(
    new PointerEvent(type, {
      clientX: x,
      clientY: y,
      button: 0,
      buttons: type === "pointerup" ? 0 : 1,
      pointerId: 1,
      isPrimary: true,
      bubbles: true,
      cancelable: true,
    }),
  );
}

/** Drags from one point to another, in a few steps as a real drag would. */
function drag(
  root: HTMLElement,
  from: [number, number],
  to: [number, number],
): void {
  pointer(root, "pointerdown", from[0], from[1]);
  for (let step = 1; step <= 4; step++) {
    const t = step / 4;
    pointer(
      root,
      "pointermove",
      from[0] + (to[0] - from[0]) * t,
      from[1] + (to[1] - from[1]) * t,
    );
  }
  pointer(root, "pointerup", to[0], to[1]);
}

/**
 * Clicks a point `times` in a row, as one double- or triple-click gesture.
 *
 * Each click is a full press and release at the same point, which is what the
 * counter in `clicks.ts` is watching for. No delay between them: they are
 * dispatched synchronously, so `performance.now()` barely advances and the run
 * is well inside the multi-click window.
 *
 * **It breaks any run already in progress first**, and that is the whole
 * reason this is a function rather than two lines at the call site. The counter
 * does not know a check has moved on to the next gesture: dispatched back to
 * back, a click, then a double-click, then a triple-click at one point is
 * *six consecutive clicks* at that point, and the counter --- correctly ---
 * reads them as one run cycling 1,2,3,1,2,3. Every reading is then off by
 * however many clicks the previous check made.
 *
 * It cost all three granularity checks on the first run, including the control:
 * the "single" click reported seven characters selected, because the drag in
 * the check before it had pressed at the same point a few milliseconds earlier
 * and this was that run's *second* click. Nothing was wrong with the viewer.
 * The trap of a control contaminated by the phase before it, arriving through a
 * counter that neither phase mentions.
 *
 * Breaking the run by distance rather than by waiting keeps the check
 * deterministic --- a sleep long enough to be safe is a sleep in every run, and
 * one just short of the window is a flake that only appears on a loaded
 * machine.
 */
function click(root: HTMLElement, x: number, y: number, times: number): void {
  const away = y + 4 * MULTI_CLICK_SLOP_PX;
  pointer(root, "pointerdown", x, away);
  pointer(root, "pointerup", x, away);
  for (let n = 0; n < times; n++) {
    pointer(root, "pointerdown", x, y);
    pointer(root, "pointerup", x, y);
  }
}

/** Dispatches a keydown the way the window would. */
function key(root: HTMLElement, k: string, accel = false): void {
  root.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: k,
      metaKey: accel,
      bubbles: true,
      cancelable: true,
    }),
  );
}

/**
 * Runs the check if `TPDF_VIEWERCHECK` is set, then exits the process.
 *
 * Returns `false` when it was not requested, so the caller carries on into the
 * real application.
 */
export async function runViewerCheckIfRequested(): Promise<boolean> {
  const path = await invoke<string | null>("viewercheck_path");
  if (!path) return false;

  try {
    await run(path);
  } catch (e) {
    check("run completed", false, String(e));
  }

  // Prints the `N/M checks passed` summary, waits for the last line to land,
  // and exits with the verdict's code.
  await report.finish();
  return true;
}

async function run(path: string): Promise<void> {
  const doc = await invoke<DocumentInfo>("open_document", { path });
  const page = doc.pages[0];
  if (!page) throw new Error("document reports no pages");

  const root = document.createElement("div");
  root.style.cssText = `position:fixed;left:0;top:0;width:${WIDTH}px;height:${HEIGHT}px;`;
  document.body.replaceChildren(root);

  // `updates` counts deliveries, and exists because `status` is a *mirror*: a
  // check that waits for the viewer to go idle and then reads this object can
  // be reading the state from before the thing it just did. Counting lets a
  // wait say "a status has arrived since" without waiting on the value being
  // asserted, which would make the assertion unable to fail. See the trap.
  const seen: { status: ViewerStatus | null; updates: number } = {
    status: null,
    updates: 0,
  };
  const sharp = () => seen.status?.sharp ?? 0;
  const covered = () => sharp() >= 0.999;
  const pct = () => `sharp=${(sharp() * 100).toFixed(1)}%`;

  // Mounted outside the viewer's root, and wired through `onPosition` exactly
  // as `App.svelte` wires it --- so what is checked below is the connection the
  // application has, not a second one written for the check.
  //
  // Given a real height, beside the viewer rather than on top of it. A zero-size
  // panel was enough for the outline, and is not for the page strip: the strip
  // builds the rows its panel can show, so in a box of no height it would build
  // one row and every assertion about windowing would pass on nothing.
  // Every message the viewer reported through `onError`, in order.
  const problems: string[] = [];
  /** Every reorder the page strip asked for, in order. */
  const drags: [number, number][] = [];
  const panel = document.createElement("div");
  panel.style.cssText = `position:fixed;left:${WIDTH}px;top:0;width:300px;height:${HEIGHT}px;`;
  document.body.appendChild(panel);
  const sidebar = new Sidebar(panel, {
    onNavigate: (target, top) => viewer.goToDestination(target, top),
    results: { onPick: (index) => viewer.showMatch(index) },
    comments: { onPick: (id) => viewer.showComment(id) },
    pages: {
      doc: doc.id,
      pageCount: doc.page_count,
      page,
      tier1: { placeholderFor: (at) => viewer.placeholderFor(at) },
      onNavigate: (at) => viewer.goToPage(at),
      // Recorded rather than applied, and that is the deliberate half of this
      // wiring. `App.svelte` runs an edit here; running one from the harness
      // would be a *second* implementation of that handler, and the two seams
      // it would exercise -- `Edits.move`'s arithmetic and the `page_move`
      // round trip -- already have checks of their own that do not need a
      // pointer. What no unit test can reach is whether the gesture works in a
      // real webview at all, so that is what this leaves for the window: real
      // pointer capture, real row geometry, real event delivery.
      onReorder: (from, to) => drags.push([from, to]),
    },
  });

  const viewer = new Viewer(root, {
    doc: doc.id,
    pageCount: doc.page_count,
    // The whole table the open carried, exactly as `App.svelte` hands it over.
    // On a lazy open that is page 1 alone and the viewer learns the rest.
    pages: [page, ...doc.pages.slice(1)],
    onStatus: (next) => {
      seen.status = next;
      seen.updates += 1;
      // The wiring the yield depends on, taken from `App.svelte` rather than
      // invented here: without this line the strip never learns the viewer is
      // busy and the check below would be testing a mechanism nothing drives.
      sidebar.setViewerBusy(next.pending > 0);
      // Likewise: the strip learns about a rotation through the status, and
      // omitting this line is how "the page strip turns with the view" first
      // went red --- against a viewer that had rotated perfectly.
      sidebar.setTurns(next.turns);
    },
    onPosition: (at, top) => sidebar.setPosition(at, top),
    // Recorded rather than shown. `App.svelte` puts this on the header; here it
    // is what makes "a refused link said so" an assertion instead of an
    // observation about a press that may simply have missed.
    onError: (message) => problems.push(message),
  });

  // The constructor sizes itself against a root the layout has not reached, so
  // the real fit-width arrives through the ResizeObserver. Nothing below means
  // anything until that has happened.
  if (
    !(await eventually(
      "fits the page to the window",
      () => {
        const ratio = (page.width_pt * viewer.currentZoom) / WIDTH;
        return ratio > 0.85 && ratio < 1.0;
      },
      () =>
        `page spans ${((page.width_pt * viewer.currentZoom) / WIDTH) * 100}% of the width`,
    ))
  ) {
    return;
  }

  // Idling, in the direction that can fail. Work is outstanding from the first
  // frame, so a loop that had never started would report idle here and the
  // check below would pass on nothing.
  check("runs a frame loop while working", !viewer.idle, `idle=${viewer.idle}`);

  await eventually("covers the first screen", covered, pct);
  await eventually(
    "stops the frame loop when settled",
    () => viewer.idle,
    () => "loop stopped",
  );

  const before = viewer.offset;
  wheel(root, 400);
  check(
    "a wheel notch scrolls, and wakes the loop",
    viewer.offset > before && !viewer.idle,
    `offset ${before.toFixed(0)} -> ${viewer.offset.toFixed(0)}, idle=${viewer.idle}`,
  );

  const leftFrom = viewer.offset;
  key(root, "End");
  check(
    "End reaches the end of the document",
    Math.abs(viewer.offset - viewer.maxOffset) < 1,
    `offset=${viewer.offset.toFixed(0)} max=${viewer.maxOffset.toFixed(0)}`,
  );
  // The control, and it is not decoration: written without it, the check below
  // waited for full coverage that the *first* screen had already established
  // and returned before the jump had rendered anything. It passed, and its own
  // detail line gave it away by reporting page 1 of 775.
  //
  // It only means something when the jump left the screen behind. On a
  // one-page sheet End moves a few hundred pixels and the tiles already on
  // screen stay valid, which is correct behaviour and not something to assert
  // against.
  //
  // **How far this jump travelled, not how long the document is.** Those are
  // the same number only from a standing start, and the wheel notch above has
  // already moved 400 px. A document 750 px longer than its window satisfies
  // "longer than a window" and then travels 350, so the previous screen's
  // tiles stay valid and the check fails on a viewer that discarded exactly
  // what it should have. Measured on `links-rotated.pdf`, which is short
  // enough for the two quantities to come apart.
  await frame();
  const travelled = viewer.offset - leftFrom;
  if (travelled > HEIGHT) {
    check(
      "a jump discards what it leaves behind",
      !covered(),
      `${pct()} one frame after the jump`,
    );
  } else {
    skip(
      "a jump discards what it leaves behind",
      `End travelled ${travelled.toFixed(0)} px, less than the ${HEIGHT} px window, ` +
        `so the screen it left is still on screen`,
    );
  }
  await eventually(
    "covers the last page",
    () => covered() && seen.status?.page === doc.page_count,
    () => `${pct()} on page ${seen.status?.page}/${doc.page_count}`,
  );

  key(root, "Home");
  check(
    "Home returns to the top",
    viewer.offset === 0,
    `offset=${viewer.offset}`,
  );

  // Zoom, with its own control: recovering coverage proves nothing unless the
  // zoom threw the tiles away first.
  await settle(() => viewer.idle && covered());
  const fitZoom = viewer.currentZoom;
  key(root, "+", true);
  await frame();
  check(
    "a zoom step changes the scale",
    viewer.currentZoom > fitZoom,
    `${fitZoom.toFixed(3)} -> ${viewer.currentZoom.toFixed(3)}`,
  );
  check(
    "a zoom step discards what it invalidates",
    !covered(),
    `${pct()} one frame later`,
  );
  await eventually("recovers coverage after a zoom", covered, pct);

  // A pinch is a wheel event carrying ctrlKey, and is the only zoom path not on
  // the ladder, so it is exercised separately.
  const pinchFrom = viewer.currentZoom;
  wheel(root, -100, true);
  check(
    "a pinch gesture zooms",
    viewer.currentZoom > pinchFrom,
    `${pinchFrom.toFixed(3)} -> ${viewer.currentZoom.toFixed(3)}`,
  );

  key(root, "0", true);
  await eventually(
    "Cmd-0 returns to fit width",
    () => Math.abs(viewer.currentZoom - fitZoom) < 1e-6,
    () => `zoom=${viewer.currentZoom.toFixed(3)}`,
  );

  // Resizing. Tier-2 tiles survive it -- only where they are drawn changes --
  // so the assertion is that the fit follows the window and coverage holds.
  await settle(() => viewer.idle);
  const narrow = WIDTH - 200;
  root.style.width = `${narrow}px`;
  await eventually(
    "follows a window resize",
    () => {
      const ratio = (page.width_pt * viewer.currentZoom) / narrow;
      return ratio > 0.85 && ratio < 1.0 && covered();
    },
    () => `zoom=${viewer.currentZoom.toFixed(3)}, ${pct()}`,
  );

  await readingChecks(doc);
  await fitChecks(root, viewer);
  await selectionChecks(root, viewer, doc);
  await searchChecks(root, viewer, doc, seen);
  await resultsChecks(viewer, sidebar, doc, seen);
  await mappingChecks(viewer, sidebar, doc, seen);
  await crossPageChecks(viewer, doc, seen);
  await scopedSearchChecks(root, viewer, seen);
  await paletteChecks(viewer, doc.page_count);
  await appCommandChecks(viewer, doc);
  await accessibilityChecks(root, viewer, doc, seen);
  await outlineChecks(viewer, sidebar, doc);
  // Before `rotationChecks`, which leaves the view turned: `screenPoint` maps a
  // page-space point without the view's own rotation, so a press aimed through
  // it lands somewhere else once the reader has turned the page. That is a
  // property of the harness rather than of the viewer --- `Viewer.anchorFor`
  // does apply the turn --- and running here is cheaper than a second mapping
  // nobody else needs.
  await commentChecks(root, viewer, sidebar, doc);
  await linkChecks(root, viewer, doc, problems);
  await thumbnailChecks(root, viewer, sidebar, doc, page, drags);
  await rotationChecks(root, viewer, sidebar, doc, page, seen);
  await pageRotationChecks(root, viewer, doc, seen);
  await pageDeletionChecks(root, viewer, doc, seen);
  await pageMoveChecks(root, viewer, doc, seen);
  await invertChecks(viewer, doc, page, seen);
  // Last of the checks that drive the surface, because it is the only one that
  // deliberately leaves the view somewhere else: it scrolls the whole document
  // to make every page's size known, and refits on the widest page. Everything
  // after it talks to the backend rather than to the viewer.
  await geometryChecks(viewer, doc, seen);
  await printChecks(path, doc);
  await releaseChecks(path);

  sidebar.destroy();
  panel.remove();
  viewer.destroy();
  check("destroys cleanly", viewer.idle, "frame loop stopped");
}

/**
 * Text selection, on whatever the current page happens to be.
 *
 * The load-bearing assertion is the **ordering** one: text dragged near the top
 * of the page must come from earlier in the page's text than text dragged near
 * the bottom. That is the only check here that ties a screen position to
 * specific characters, and so the only one that can see a coordinate error.
 *
 * It replaced a substring check --- "the dragged text appears in the whole
 * page's text" --- which sounded like it tested the same thing and could not
 * fail. A selection is a contiguous range of character *indices*, so its text is
 * a substring of the page's text no matter where the boxes claim the characters
 * are. Inverting the y-flip in `text.rs` passed all twenty checks, and the drag
 * even returned real words; it was simply the wrong words. Nothing about a
 * property that holds by construction can be evidence of anything.
 */
/** What a fixture's generator says each of its pages should read as. */
interface ReadingManifest {
  pages: { page: number; name: string; lines: string[] }[];
  /** Searches the generator states the outcome of. Only some fixtures have any. */
  queries?: { name: string; query: string; hits: number }[];
}

/**
 * Reading order, against expectations this process did not write.
 *
 * The one check in this file whose answer comes from outside: the fixture's
 * generator states the lines of each page, in the order they are meant to be
 * read, and that file is passed in by `viewer_check.py`. Comparing against it
 * is therefore not the writer agreeing with its own reader --- which
 * `docs/TRAPS.md` records as the shape that passes on output that is wrong.
 *
 * The strongest assertion is the differential one underneath it, and it needs
 * no manifest at all: `columns.pdf` carries two pages laid out identically and
 * emitted in opposite orders, so a correct implementation cannot tell them
 * apart. That one cannot be satisfied by any amount of self-consistency,
 * because self-consistency is exactly what produces two different answers.
 */
async function readingChecks(doc: DocumentInfo): Promise<void> {
  const raw = await invoke<string | null>("reading_manifest");
  if (!raw) {
    skip(
      "a page reads in the order its generator laid it out",
      "no manifest for this fixture",
    );
    skip(
      "two pages laid out alike read alike, whatever their order in the file",
      "no manifest",
    );
    return;
  }

  let manifest: ReadingManifest;
  try {
    manifest = JSON.parse(raw) as ReadingManifest;
  } catch (e) {
    check(
      "a page reads in the order its generator laid it out",
      false,
      `unreadable manifest: ${e}`,
    );
    return;
  }

  // A manifest of the wrong *shape*, which is not the same as an unreadable
  // one and used to crash the whole run: `viewer_check.py` binds any
  // `<fixture>-manifest.json` to this variable, so the suffix alone enrols a
  // fixture in this check. `comments-corpus.json` was called
  // `comments-manifest.json` for one commit, and the loop below threw
  // `{} is not iterable` sixteen checks in --- taking the other 155 with it,
  // since an exception here ends the run rather than reddening a row. Reported
  // as a failure rather than a skip: nothing is *inapplicable* here, a fixture
  // has claimed a name that means something it does not mean.
  if (!Array.isArray(manifest.pages)) {
    check(
      "a page reads in the order its generator laid it out",
      false,
      "the manifest bound to TPDF_READING_MANIFEST has no `pages` array -- " +
        "rename the sidecar so it does not end in `-manifest.json`",
    );
    skip(
      "two pages laid out alike read alike, whatever their order in the file",
      "the manifest is not a reading manifest",
    );
    return;
  }

  const cache = new TextCache(doc.id);
  /** A page's non-empty lines, trimmed, in reading order. */
  const read = async (at: number): Promise<string[]> => {
    const text = await cache.load(at);
    if (!text) return [];
    return readingLines(text)
      .map((line) =>
        line.ranges
          .flatMap((range) =>
            Array.from({ length: range.to - range.from }, (_, i) =>
              String.fromCodePoint(text.codes[range.from + i] ?? 0),
            ),
          )
          .join("")
          .trim(),
      )
      .filter((line) => line.length > 0);
  };

  const wrong: string[] = [];
  for (const expected of manifest.pages) {
    const got = await read(expected.page);
    if (got.join("\n") !== expected.lines.join("\n")) {
      const at = got.findIndex((line, i) => line !== expected.lines[i]);
      wrong.push(
        `${expected.name}: line ${at} is "${got[at] ?? "(missing)"}", ` +
          `wanted "${expected.lines[at] ?? "(nothing)"}"`,
      );
    }
  }
  check(
    "a page reads in the order its generator laid it out",
    wrong.length === 0,
    wrong.length === 0
      ? `${manifest.pages.length} pages, ${manifest.pages[0]?.lines.length ?? 0} lines each`
      : wrong.join("; "),
  );

  // Named rather than found by index: a differential check whose two sides are
  // whichever pages happened to be first would compare a page with itself the
  // day the fixture gains one.
  const natural = manifest.pages.find((p) => p.name === "natural");
  const shuffled = manifest.pages.find((p) => p.name === "interleaved");
  if (!natural || !shuffled) {
    skip(
      "two pages laid out alike read alike, whatever their order in the file",
      "the manifest has no natural/interleaved pair",
    );
    return;
  }
  const [first, second] = [await read(natural.page), await read(shuffled.page)];
  check(
    "two pages laid out alike read alike, whatever their order in the file",
    first.length > 0 && first.join("\n") === second.join("\n"),
    `${first.length} lines vs ${second.length}: ` +
      `"${first[0] ?? ""}..." vs "${second[0] ?? ""}..."`,
  );
}

/**
 * Fitting the page, and the two ways a fit stops.
 *
 * `zoom.test.ts` proves the arithmetic against numbers. What it cannot say is
 * that the numbers reach the layout: every assertion here reads the page box
 * the scroller actually laid out, against the element's real height, so a fit
 * computed correctly and applied to nothing fails.
 *
 * The state is put back to fit-width at the end, and that is not tidiness ---
 * `rotationChecks` derives its expected zoom from the page's aspect ratio,
 * which is only the answer while the width is what is being fitted.
 */
async function fitChecks(root: HTMLElement, viewer: Viewer): Promise<void> {
  /** How much of the window the laid-out page covers, on each axis. */
  const box = (): { width: number; height: number } => viewer.pageBoxCss;
  /**
   * The width a page is fitted into, which is not the element's.
   *
   * The scrollbar sits in a gutter over the right-hand edge, so a page as wide
   * as `clientWidth` has its last 12 px underneath it. Written as the element's
   * own width first, and the mutation that deletes the refit on rotation then
   * *passed*: an upright A4 fitted by its height is 700 px wide when turned, and
   * 700 is exactly `clientWidth` --- so the check was reading a page that
   * overflowed the readable area as one that fitted.
   */
  const usable = (): number => root.clientWidth - SCROLLBAR_WIDTH;
  // A pixel of slack on each bound: the box is a float and `clientHeight` is a
  // rounded integer, so an exact fit can land a fraction over. It cannot hide
  // what is being tested --- a page fitted to the wrong axis overshoots by
  // hundreds of pixels.
  const fits = (): boolean =>
    box().height <= root.clientHeight + 1 && box().width <= usable() + 1;

  key(root, "0", true);
  await settle(() => viewer.idle);
  const wide = box();

  key(root, "9", true);
  await frame();
  check(
    "Cmd-9 fits the whole page in the window",
    viewer.fitMode === "page" && fits(),
    `${viewer.fitMode}: page ${box().width.toFixed(0)}x${box().height.toFixed(0)} ` +
      `in ${root.clientWidth}x${root.clientHeight}`,
  );

  // The control. On a page short enough to fit the window at fit-width already,
  // the check above is satisfied by doing nothing at all, and a silently
  // vacuous check is the thing this repository keeps finding.
  if (wide.height <= root.clientHeight) {
    skip(
      "fitting the page shows less of it than fitting the width",
      `the page already fitted the window at fit width (${wide.height.toFixed(0)}px ` +
        `in ${root.clientHeight}px)`,
    );
  } else {
    check(
      "fitting the page shows less of it than fitting the width",
      box().height < wide.height,
      `${wide.height.toFixed(0)}px tall at fit width, ${box().height.toFixed(0)}px at fit page`,
    );
  }

  // A rotation changes the page's aspect, so the fit computed a moment ago is
  // the wrong one now. This is the assertion that fails if `rotateBy` stops
  // re-applying the fit --- under fit-width that shows up as a zoom, but under
  // fit-page it is the difference between the turned page and a third of it.
  key(root, "r", true);
  await frame();
  check(
    "a fitted page is refitted when the view is rotated",
    viewer.rotation === 1 && viewer.fitMode === "page" && fits(),
    `turns=${viewer.rotation}, page ${box().width.toFixed(0)}x${box().height.toFixed(0)} ` +
      `in ${usable()}x${root.clientHeight}`,
  );
  key(root, "l", true);
  await frame();

  const fitted = viewer.currentZoom;
  key(root, "+", true);
  await frame();
  check(
    "a zoom step stops the zoom following the window",
    viewer.fitMode === "none" && viewer.currentZoom > fitted,
    `${viewer.fitMode} at ${viewer.currentZoom.toFixed(3)}, was fitted at ${fitted.toFixed(3)}`,
  );

  key(root, "1", true);
  await frame();
  check(
    "Cmd-1 is actual size, and follows nothing",
    viewer.fitMode === "none" && Math.abs(viewer.currentZoom - 1) < 1e-9,
    `${viewer.fitMode} at ${viewer.currentZoom.toFixed(3)}`,
  );

  key(root, "0", true);
  await settle(() => viewer.idle);
  check(
    "Cmd-0 puts the zoom back to following the width",
    // Compared against the box measured at the top of this function rather than
    // against a zoom: the window has not changed size in between, so fit-width
    // has to lay the page out at exactly the width it did then.
    viewer.fitMode === "width" && Math.abs(box().width - wide.width) < 1,
    `${viewer.fitMode}, page ${box().width.toFixed(0)}px wide, was ${wide.width.toFixed(0)}px`,
  );
}

async function selectionChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle);

  // Select-all first, for a known whole to locate the drags within. The
  // character count comes from a second, independent extraction rather than
  // from the viewer's own cache, so "selects the page's text" cannot be
  // satisfied by the viewer agreeing with itself.
  const extracted = await invoke<{ codes: number[]; quarter_turns: number }>(
    "page_text",
    { doc: doc.id, page: 0 },
  ).catch(() => null);

  if (extracted && extracted.codes.length === 0) {
    // A scan, or the A0 sheet. "Selected 0 of 0" is a true statement and a
    // misleading [OK] beside a check named for selecting text.
    // All four, not just the two that need text to succeed. The other two are
    // *controls* --- Escape clearing a selection, a zero-length drag selecting
    // nothing --- and a control that quietly disappears on some inputs is
    // indistinguishable from one that ran. Found by counting the names in the
    // two runs and getting 43 and 41.
    skip("Cmd-A selects the page's text", "the page has no extractable text");
    skip("Escape clears the selection", "there is no text to select and clear");
    skip(
      "dragging nowhere selects nothing",
      "there is no text a drag could select",
    );
    skip(
      "a drag selects text from where it was dragged",
      "the page has no extractable text",
    );
    skipGranularity("the page has no extractable text");
    return;
  }

  key(root, "a", true);
  const ok = await eventually(
    "Cmd-A selects the page's text",
    () => [...viewer.selectedText].length === (extracted?.codes.length ?? -1),
    () =>
      `${[...viewer.selectedText].length} code points, extraction says ${
        extracted?.codes.length ?? "unavailable"
      }`,
  );
  const whole = viewer.selectedText;

  key(root, "Escape");
  check(
    "Escape clears the selection",
    viewer.selectedText === "",
    "nothing selected",
  );

  // A degenerate drag, as the control: press and release without moving. If
  // this selected something, every assertion below would pass on a selection
  // model that simply always selects.
  drag(root, [MID_X, HIGH_Y], [MID_X, HIGH_Y]);
  check(
    "dragging nowhere selects nothing",
    viewer.selectedText === "",
    `selected ${viewer.selectedText.length} characters`,
  );

  granularityChecks(root, viewer, whole);

  if (!ok || !whole) {
    skip(
      "a drag selects text from where it was dragged",
      "the page has no extractable text",
    );
    return;
  }

  // The ordering assertion below reads a *horizontal* drag high on the page
  // against one lower down, which assumes the page reads left-to-right and its
  // lines advance downwards. On a page carrying `/Rotate 90` --- what a scanner
  // emits --- the text runs down the screen and lines advance sideways, so a
  // horizontal drag crosses every line at once and the comparison means
  // nothing. Skipped with the reason rather than rewritten for four rotations
  // nothing here would exercise: `text-probe --mode align` checks that mapping
  // per rotation, against pixels, with a control for each wrong turn.
  if ((extracted?.quarter_turns ?? 0) % 2 === 1) {
    skip(
      "a drag selects text from where it was dragged",
      "this page's lines advance sideways, so a horizontal drag crosses all of them",
    );
    return;
  }

  // The same assertion has a second premise, and `columns.pdf` is the fixture
  // that falsifies it: it compares *character indices*, so it assumes the page
  // was written in the order it is read. A producer that emitted its two
  // columns line by line breaks that --- the text at the top of column two has
  // a higher index than the text at the bottom of column one --- and the check
  // then reports "the page reads bottom to top", which is a true statement
  // about the file and not about the drag.
  //
  // Tested here from the *boxes*, not from `reading.ts`: a precondition
  // computed by the code under test is one a defect in that code can switch
  // off, and `docs/TRAPS.md` has that entry. This asks the question directly ---
  // does the extraction advance down the page? -- and needs nothing but the
  // geometry the backend already sent.
  // The same assertion has a second premise, and `columns.pdf` falsifies it: on
  // more than one column the top of column two is read *after* the bottom of
  // column one, so "higher on the page" does not mean "earlier in the text" for
  // any two-column layout, however sensibly its producer wrote it.
  //
  // Measured first as "does the extraction travel back up the page", which was
  // the wrong instrument twice over --- it read PDFium's glyph boxes, whose tops
  // move with every ascender, and reported 45% on a strictly top-to-bottom
  // single-column document. It survived only because the fraction was printed.
  // This asks the structural question instead, and has no threshold to mis-set.
  //
  // It does come from `reading.ts`, which a defect there could therefore switch
  // off --- the trap of that name. It is guarded rather than ignored: two checks
  // above assert reading order directly against a manifest this process did not
  // write, and fourteen unit tests cover the same code, so a `reading.ts` broken
  // enough to silence this one does not get to be silent.
  const shown = viewer.textOn(0);
  if (shown && hasSideBySideLines(shown)) {
    skip(
      "a drag selects text from where it was dragged",
      "this page has lines side by side, so being higher up does not mean being read sooner",
    );
    return;
  }

  drag(root, [MID_X, HIGH_Y], [MID_X + 240, HIGH_Y]);
  const high = viewer.selectedText;
  const highAt = whole.indexOf(high);

  drag(root, [MID_X, LOW_Y], [MID_X + 240, LOW_Y]);
  const low = viewer.selectedText;
  const lowAt = whole.indexOf(low);

  // A drag that selected nothing is a fact about where this page's text *is*,
  // not about the ordering being wrong, and reporting it as a failure blames the
  // code for the fixture. `multilingual.pdf` is where that showed up: four pages
  // of three to six lines spread down an A4 sheet, and y=620 falls in a gap
  // between two of them --- so the check reported "selected 20 and 0 characters"
  // on a viewer that was working perfectly.
  //
  // A precondition rather than a widened assertion, because the assertion is the
  // valuable part: with nothing selected at one of the two heights there is no
  // ordering to compare, and any verdict would be invented.
  if (high.length === 0 || low.length === 0) {
    skip(
      "a drag selects text from where it was dragged",
      `this page has no text at y=${high.length === 0 ? HIGH_Y : LOW_Y}, so there is nothing to order`,
    );
    return;
  }
  // A selection too short to be located is the same class of problem as an
  // empty one and was not guarded until 2026-08-16: `indexOf` finds the *first*
  // occurrence, so a one-character selection resolves to wherever that letter
  // happens to appear first --- which on `comments.pdf` was a `g` from an early
  // line, reported as the page reading bottom to top. The verdict was invented
  // from a position that meant nothing.
  const shortest = Math.min(high.length, low.length);
  if (shortest < 3) {
    skip(
      "a drag selects text from where it was dragged",
      `one drag selected ${shortest} character(s), which cannot be located in the page's text`,
    );
    return;
  }
  const located = highAt >= 0 && lowAt >= 0;
  check(
    "a drag selects text from where it was dragged",
    located && highAt < lowAt,
    !located
      ? `selected ${high.length} and ${low.length} characters, not both located in the page's text`
      : `y=${HIGH_Y} gave "${preview(high)}" at ${highAt}; ` +
          `y=${LOW_Y} gave "${preview(low)}" at ${lowAt}` +
          (highAt < lowAt
            ? ""
            : " -- the page reads bottom to top, which it does not"),
  );
}

/** The names {@link granularityChecks} reports, so a skip can list them all. */
const GRANULARITY_CHECKS = [
  "a double-click selects a word rather than a character",
  "a triple-click selects the line the word is on",
  "a drag after a double-click ends on a word boundary",
] as const;

function skipGranularity(why: string): void {
  for (const name of GRANULARITY_CHECKS) skip(name, why);
}

/**
 * Double- and triple-click selection.
 *
 * Deliberately relational rather than literal: every assertion here compares
 * one selection against another taken from the same point, so none of them
 * needs to know a word the fixture happens to contain. A check that named
 * "Hello" would pass on one corpus and skip on five.
 *
 * These run on every page including a rotated one, unlike the ordering check
 * above. Line grouping follows the page's own reading axis, so a triple-click
 * on a `/Rotate 90` page selects the line that runs down the screen --- there
 * is nothing here that assumes lines advance downwards.
 */
function granularityChecks(
  root: HTMLElement,
  viewer: Viewer,
  whole: string,
): void {
  const at: [number, number] = [MID_X, HIGH_Y];

  // The control, and it has to come first: if a single click already selected
  // a word, every assertion below would pass against a viewer with no notion
  // of granularity at all.
  click(root, at[0], at[1], 1);
  const single = viewer.selectedText;

  click(root, at[0], at[1], 2);
  const word = viewer.selectedText;

  check(
    GRANULARITY_CHECKS[0],
    single === "" && word.length > 1 && !/\s/u.test(word),
    single !== ""
      ? `a single click already selected ${single.length} characters, so this proves nothing`
      : `"${preview(word)}" (${word.length} characters)` +
          (/\s/u.test(word)
            ? " -- which contains whitespace, so it is not one word"
            : ""),
  );

  click(root, at[0], at[1], 3);
  const line = viewer.selectedText;

  if (word && line === word) {
    // Honest rather than green: on a line holding a single word the two
    // selections are legitimately identical and the check cannot discriminate.
    skip(
      GRANULARITY_CHECKS[1],
      "the line under the pointer holds a single word",
    );
  } else {
    check(
      GRANULARITY_CHECKS[1],
      word !== "" && line.includes(word) && line.length > word.length,
      `"${preview(line)}" (${line.length}) against the word "${preview(word)}" (${word.length})`,
    );
  }

  // A drag that begins with a double-click must extend by whole words.
  //
  // The precondition is *found*, not assumed, and that is the whole design of
  // this check. Written first as a fixed 240 px drag asserting the result ends
  // on a word boundary, it passed with the granular branch of `onSelectMove`
  // mutated to `false` --- because 240 px happens to land on a boundary in this
  // fixture, so a character drag and a word drag return the identical string.
  // An outcome two mechanisms can produce cannot test either one.
  //
  // So a distance is searched for whose *character*-granular end falls inside a
  // word. At that distance the two mechanisms must differ: the character drag
  // stops mid-word and the word drag rounds outwards to the word's end.
  const clean = (edge: string) => edge === "" || !WORD_CHARACTER.test(edge);
  const edgesOf = (text: string) => {
    const foundAt = whole.indexOf(text);
    return {
      foundAt,
      before: foundAt > 0 ? (whole[foundAt - 1] ?? "") : "",
      after: foundAt >= 0 ? (whole[foundAt + text.length] ?? "") : "",
    };
  };

  // Short distances first, and they are the ones that work. The list began at
  // 240 px --- what the ordering check drags --- and every candidate near it
  // ended on a boundary for a reason that has nothing to do with words: from
  // x=300 a drag that long runs off the end of the line, so the selection ends
  // at its last character however far past it the pointer goes.
  let midWordAt = 0;
  let charDrag = "";
  for (const candidate of [
    40, 48, 56, 64, 72, 80, 88, 96, 120, 160, 200, 240,
  ]) {
    dragAfterClicks(root, at, candidate, 1);
    const text = viewer.selectedText;
    const edges = edgesOf(text);
    if (text.length > 0 && edges.foundAt >= 0 && !clean(edges.after)) {
      midWordAt = candidate;
      charDrag = text;
      break;
    }
  }

  if (!midWordAt) {
    // Honest rather than green: with no distance that ends inside a word, the
    // two mechanisms agree and the check would be asserting nothing.
    skip(
      GRANULARITY_CHECKS[2],
      "no drag distance tried ends inside a word on this page",
    );
    return;
  }

  dragAfterClicks(root, at, midWordAt, 2);
  const extended = viewer.selectedText;
  const edges = edgesOf(extended);

  check(
    GRANULARITY_CHECKS[2],
    extended.length > charDrag.length &&
      edges.foundAt >= 0 &&
      clean(edges.before) &&
      clean(edges.after),
    edges.foundAt < 0
      ? `"${preview(extended)}" was not found in the page's own text`
      : `"${preview(extended)}" against the same drag without the double-click, ` +
          `"${preview(charDrag)}", which stopped before "${edgesOf(charDrag).after}"` +
          (extended.length > charDrag.length
            ? ""
            : " -- the double-click changed nothing") +
          (clean(edges.before) ? "" : " -- it began inside a word") +
          (clean(edges.after) ? "" : " -- it stopped inside a word"),
  );
}

/**
 * Presses `times` at a point, then drags right without releasing the last one.
 *
 * The run is broken first, for the reason {@link click} gives. Spelled out
 * rather than built on `click` because the final press must stay down.
 */
function dragAfterClicks(
  root: HTMLElement,
  at: [number, number],
  dx: number,
  times: number,
): void {
  const away = at[1] + 4 * MULTI_CLICK_SLOP_PX;
  pointer(root, "pointerdown", at[0], away);
  pointer(root, "pointerup", at[0], away);
  for (let n = 0; n < times - 1; n++) {
    pointer(root, "pointerdown", at[0], at[1]);
    pointer(root, "pointerup", at[0], at[1]);
  }
  pointer(root, "pointerdown", at[0], at[1]);
  for (let step = 1; step <= 4; step++) {
    pointer(root, "pointermove", at[0] + (dx * step) / 4, at[1]);
  }
  pointer(root, "pointerup", at[0] + dx, at[1]);
}

/** Letters, digits, marks and the underscore --- the word class in `text.ts`. */
const WORD_CHARACTER = /[\p{L}\p{N}\p{M}_]/u;

/** Where the selection drags run, in viewport CSS pixels. */
const MID_X = 300;
/** Near the top of the page, below its margin. */
const HIGH_Y = 140;
/**
 * Further down the same page.
 *
 * Both must land on the page that `Home` put at the top of the viewport, and
 * both must land on *text* --- a drag into a margin still selects the nearest
 * character, but which one is then a question about the margin rather than
 * about the mapping.
 */
const LOW_Y = 620;

/**
 * Find in document.
 *
 * The load-bearing assertion, and the counterpart to the ordering check above,
 * is that the characters a match *covers* are the characters that were searched
 * for. Everything else about search --- that it finds something, that it counts
 * hits, that Cmd-G moves --- passes just as well when the returned indices are
 * off by one, or in a different index space from the boxes, and an off-by-one
 * highlight is exactly the bug this whole layer was designed to make impossible.
 *
 * The needle is taken from the document rather than hardcoded, so this runs
 * against any fixture; the negative control is that needle with letters glued to
 * the front, which cannot be in a document that the real one was drawn from.
 */
async function searchChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  // Widened for `searchOptionChecks`, which needs the delivery counter and not
  // only the mirrored value. Every other consumer of `seen` takes the narrower
  // shape and still accepts this object.
  seen: { status: ViewerStatus | null; updates: number },
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle);

  const first = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const needle = first ? pickNeedle(first.codes) : null;

  // A document with no extractable text still has something to check, and it is
  // the one thing that matters for it: that the viewer says so instead of
  // reporting no matches for a query it never tested against anything.
  const probe = needle ?? "the";
  viewer.search(probe);
  await settle(() => (seen.status?.search.scanned ?? 0) >= doc.page_count);
  check(
    "tells no text apart from no matches",
    seen.status?.search.textless === (first?.codes.length === 0),
    `document reports textless=${seen.status?.search.textless}, page 1 has ` +
      `${first?.codes.length ?? "unknown"} characters`,
  );

  if (!needle) {
    // The reason has to distinguish the two ways this happens, because they call
    // for opposite responses. An empty page is a fixture that cannot exercise
    // search; a page full of text that `pickNeedle` could not read a word out of
    // is a **harness** that cannot exercise search, and printing the first when
    // the second is true is how seventeen checks stayed silently unexercised on a
    // Japanese document while claiming the page had no text.
    const why =
      (first?.codes.length ?? 0) === 0
        ? "page 1 has no extractable text"
        : `no word could be read out of page 1's ${first?.codes.length} characters`;
    skip("finds a word taken from the document", why);
    skip("a match covers the characters searched for", why);
    skip("case is ignored", why);
    skip("a word that is not there is not found", why);
    skip("searches forward from the page being read", why);
    skipSearchOptions(why);
    skip("finds something from the end of the document", why);
    skip("counts more than the matches on one page", why);
    skip("Cmd-G moves to a match on another page", why);
    return;
  }

  viewer.search(needle);
  const found = await eventually(
    "finds a word taken from the document",
    () => viewer.searchMatches.length > 0,
    () => `"${needle}" -> ${viewer.searchMatches.length} matches so far`,
  );

  const firstHit = viewer.searchMatches[0];
  if (found && firstHit) {
    // The assertion that ties an index range to specific content. The text is
    // re-extracted here rather than read out of the viewer's cache, so a match
    // cannot be confirmed by the viewer agreeing with itself.
    const page = await invoke<{ codes: number[] }>("page_text", {
      doc: doc.id,
      page: firstHit.page,
    }).catch(() => null);
    const covered = page
      ? String.fromCodePoint(...page.codes.slice(firstHit.start, firstHit.end))
      : "";
    check(
      "a match covers the characters searched for",
      covered.toLowerCase() === needle.toLowerCase(),
      `page ${firstHit.page} [${firstHit.start}, ${firstHit.end}) is "${preview(covered)}", ` +
        `searched for "${needle}"`,
    );
  } else {
    skip(
      "a match covers the characters searched for",
      "nothing was found to check",
    );
  }

  // Same first match, not merely some match: both scans start from the same
  // page and walk the same way, so anything other than the identical hit means
  // the two queries did not match the same text.
  viewer.search(needle.toUpperCase());
  await eventually(
    "case is ignored",
    () => {
      const upper = viewer.searchMatches[0];
      return (
        !!upper &&
        !!firstHit &&
        upper.page === firstHit.page &&
        upper.start === firstHit.start &&
        upper.end === firstHit.end
      );
    },
    () => {
      const upper = viewer.searchMatches[0];
      return (
        `"${needle.toUpperCase()}" first hit ${upper ? `${upper.page}:${upper.start}` : "none"}, ` +
        `"${needle}" first hit ${firstHit ? `${firstHit.page}:${firstHit.start}` : "none"}`
      );
    },
  );

  // The negative control. Without it, every check above is satisfied by a search
  // that returns matches for anything at all.
  const absent = `qxzj${needle}`;
  viewer.search(absent);
  // Waiting for `scanned` to reach the page count rather than for the scan to
  // stop running. They are the same thing only while cancellation works: a
  // mutation that removed the generation guard let an *older* scan finish and
  // clear the running flag, and this check then read its result at a moment when
  // neither scan had put anything in the list. It passed, for a reason that had
  // nothing to do with the query being absent.
  await settle(() => (seen.status?.search.scanned ?? 0) >= doc.page_count);
  check(
    "a word that is not there is not found",
    viewer.searchMatches.length === 0,
    `"${absent}" -> ${viewer.searchMatches.length} matches over ` +
      `${seen.status?.search.scanned ?? 0} pages in ` +
      `${viewer.searchElapsedMs.toFixed(0)} ms`,
  );

  await searchOptionChecks(viewer, seen, needle, firstHit, doc.page_count);
  await searchesFromHere(root, viewer, seen, needle, doc.page_count);
  await stepToAnotherPage(root, viewer, seen, needle, doc.page_count);
}

/** What {@link resultsChecks} records, for its own skip path. */
const RESULTS_CHECKS = [
  "the results tab lists a row per hit",
  "a row shows the words around its hit, with the hit emboldened",
  "picking a row moves the document to that hit",
  "the results list is replaced when the query changes",
] as const;

/**
 * The search-results sidebar tab, against a real DOM.
 *
 * The unit tests cover the state machine --- appending rather than rebuilding,
 * the row cap, the status line --- against `testdom.ts`, which has no text
 * layout and no real elements. What only a running webview can answer is whether
 * a row actually *says* what the match found and whether pressing one moves the
 * document, so those are here and nothing else is.
 *
 * The snippet check is the load-bearing one, and it is the same shape as the
 * search check above it: a row is tied to *specific content* by comparing what
 * it displays against the page's own text, re-extracted independently. A check
 * that a row is non-empty passes for a row describing the wrong hit.
 */
async function resultsChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const first = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const needle = first ? pickNeedle(first.codes) : null;
  if (!needle) {
    // Same distinction as in `searchesFromHere`: "no text" and "no word this
    // harness could read" are different facts and only one of them is about the
    // fixture.
    const why =
      (first?.codes.length ?? 0) === 0
        ? "page 1 has no extractable text"
        : `no word could be read out of page 1's ${first?.codes.length} characters`;
    for (const name of RESULTS_CHECKS) skip(name, why);
    return;
  }

  sidebar.selectTab("results");
  const results = sidebar.results;
  const feed = (): void =>
    results.update(
      viewer.searchMatches,
      viewer.matchIndex,
      seen.status?.search.query ?? "",
      viewer.searching,
    );

  viewer.search(needle);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );
  feed();

  const total = viewer.searchMatches.length;
  check(
    RESULTS_CHECKS[0],
    results.rowCount === Math.min(total, MAX_RESULT_ROWS) &&
      results.rowCount > 0,
    `${results.rowCount} rows for ${total} matches --- "${results.status}"`,
  );

  const hit = viewer.searchMatches[0];
  const page = hit
    ? await invoke<{ codes: number[] }>("page_text", {
        doc: doc.id,
        page: hit.page,
      }).catch(() => null)
    : null;
  if (hit && page) {
    // What the row displays, read back out of the DOM, against what the page
    // says at the indices the match reported. Both halves matter: the bold run
    // has to be the hit, and the row has to be about the right place.
    const row = results.rowText(0);
    const onPage = String.fromCodePoint(
      ...page.codes.slice(hit.start, hit.end),
    );
    check(
      RESULTS_CHECKS[1],
      row.bold === onPage &&
        row.page === String(hit.page + 1) &&
        row.whole.includes(onPage),
      `row 1 reads "${preview(row.whole)}" with "${row.bold}" bold on page ${row.page}; ` +
        `the document has "${preview(onPage)}" at ${hit.page}:${hit.start}`,
    );
  } else {
    skip(RESULTS_CHECKS[1], "nothing was found to compare against the page");
  }

  // A row other than the one the scan already jumped to, and on a different
  // page, so the assertion is that the document *moved* rather than that it was
  // already there. `viewer.search` auto-shows the first hit, so without the
  // first condition this picks the row the viewer is on and asserts nothing.
  const shown = viewer.matchIndex;
  const shownPage = viewer.searchMatches[shown]?.page ?? -1;
  const away = viewer.searchMatches.findIndex(
    (m, i) => i !== shown && i < MAX_RESULT_ROWS && m.page !== shownPage,
  );
  if (away < 0) {
    skip(
      RESULTS_CHECKS[2],
      `all ${total} hits are on page ${shownPage + 1}, which is the one already shown, ` +
        "so a jump would not be observable",
    );
  } else {
    const target = viewer.searchMatches[away];
    const from = viewer.offset;
    // Through the row's own listener rather than by calling `onPick`, so what is
    // tested is the wiring a reader's finger goes through.
    results
      .rowAt(away)
      ?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await settle(() => viewer.matchIndex === away && viewer.offset !== from);
    // **Not** `position.page`, which is the page at the *top edge*: `goToMatch`
    // deliberately leaves a third of a screen above the hit, so a match near the
    // top of page 2 is shown with page 1 still at the top edge and the top-edge
    // page never changes. It is also 0 on a rotated document. The scroll offset
    // is the quantity that actually says the document moved.
    check(
      RESULTS_CHECKS[2],
      viewer.matchIndex === away && viewer.offset !== from,
      `row ${away + 1} is on page ${(target?.page ?? -1) + 1}; current match went ` +
        `${shown} -> ${viewer.matchIndex}, offset ${from.toFixed(0)} -> ` +
        `${viewer.offset.toFixed(0)}`,
    );
  }

  const absent = `qxzj${needle}`;
  viewer.search(absent);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );
  feed();
  check(
    RESULTS_CHECKS[3],
    results.rowCount === 0,
    `"${absent}" -> ${results.rowCount} rows, "${results.status}"`,
  );

  viewer.clearSearch();
  feed();
  sidebar.selectTab("outline");
}

/** The page name a generator gives a page whose fonts state no mapping. */
const UNMAPPED_PAGE = "no-mapping";
/** The manifest query that is on such a page and cannot be found. */
const UNMAPPED_QUERY = "unmapped-text-is-not-found";

/** The two checks {@link mappingChecks} records. Both run on every fixture. */
const MAPPING_CHECKS = [
  "the pages whose text is a guess reach the frontend",
  "a reader is told when a page could not be searched",
] as const;

/**
 * The third state: a page that has text, in the right places, meaning nothing.
 *
 * `encoding.rs` decides it, and until this ran the whole path from that decision
 * to the line a reader reads had never executed in a window on any platform ---
 * six hops, each typechecked and unit-tested, which is exactly the arrangement
 * that passes while the wiring between two of them is missing.
 *
 * **The expectation comes from the fixture's generator, not from the subject.**
 * `encodings.pdf` names its first page `no-mapping`, and that name is written by
 * a program that has never heard of `encoding.rs`. Waiting for
 * `unsearchablePages` to go positive instead --- the obvious shape --- would make
 * a backend that always answered zero pass on all ten corpora, including the one
 * built so that it must not.
 *
 * **Both checks run everywhere**, and the nine fixtures with nothing to report
 * are the control: the same query, the same panel, and the line must be absent.
 * A one-sided check here would be satisfied by a frontend that said it about
 * every document.
 *
 * The wait is on `mappingKnown` rather than on the count, because on those nine
 * the count starts at the value being asserted --- see the getter's own note.
 */
async function mappingChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const manifest = await manifestOf();
  const unreadable = (manifest?.pages ?? []).filter(
    (page) => page.name === UNMAPPED_PAGE,
  ).length;
  // A query the generator says is on the page and cannot be found, so what a
  // reader sees is the whole defect: a word plainly visible, "No matches.", and
  // now the sentence saying why. Elsewhere any absent query does, since what is
  // being asserted there is that the sentence stays away.
  const query =
    manifest?.queries?.find((entry) => entry.name === UNMAPPED_QUERY)?.query ??
    "qxzjabsentfromeverything";

  sidebar.selectTab("results");
  const results = sidebar.results;

  viewer.search(query);
  await settle(
    () =>
      !viewer.searching &&
      (seen.status?.search.scanned ?? 0) >= doc.page_count &&
      (seen.status?.search.mappingKnown ?? false),
  );

  const known = seen.status?.search.mappingKnown ?? false;
  const reported = seen.status?.search.unsearchablePages ?? 0;
  check(
    MAPPING_CHECKS[0],
    known && reported === unreadable,
    known
      ? `"${query}" -> ${viewer.searchMatches.length} matches; the backend reports ` +
          `${reported} unsearchable page(s) and the generator wrote ${unreadable}`
      : "the backend never answered which pages store unreadable text",
  );

  // Fed from the status the viewer emitted, which is the object `App.svelte`
  // reads at its own call site -- not from a number computed here, which would
  // be a second implementation agreeing with the first.
  results.update(
    viewer.searchMatches,
    viewer.matchIndex,
    seen.status?.search.query ?? "",
    viewer.searching,
    reported,
  );

  const said = results.status;
  const names = said.includes("could not be searched");
  const counts =
    unreadable === 0 ||
    said.includes(unreadable === 1 ? "1 page" : `${unreadable} pages`);
  check(
    MAPPING_CHECKS[1],
    names === unreadable > 0 && counts,
    `${unreadable} page(s) unreadable, the panel says "${said}"`,
  );

  viewer.clearSearch();
  results.update(viewer.searchMatches, viewer.matchIndex, "", false, 0);
  sidebar.selectTab("outline");
}

/**
 * The fixture's manifest, or null when it has none.
 *
 * {@link readingChecks} parses it separately and on purpose: a manifest that
 * does not parse is a broken fixture and it fails there, loudly and once,
 * rather than in every phase that reads one.
 */
async function manifestOf(): Promise<ReadingManifest | null> {
  const raw = await invoke<string | null>("reading_manifest").catch(() => null);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as ReadingManifest;
  } catch {
    return null;
  }
}

/** The three checks {@link searchOptionChecks} records, for the skip path. */
const SEARCH_OPTION_CHECKS = [
  "matching case rejects the hit that ignoring it accepted",
  "whole words rejects a hit inside a longer word",
  "turning the options off finds the hit again",
  "a pattern finds what the same text as a literal does not",
  "a pattern that does not compile says so instead of finding nothing",
] as const;

function skipSearchOptions(why: string): void {
  for (const name of SEARCH_OPTION_CHECKS) skip(name, why);
}

/**
 * That the two matching options reach the matcher and change what it accepts.
 *
 * Each is asserted against **one known occurrence** --- the first hit of the
 * plain search, whose position and spelling are both already established ---
 * rather than against a count. A count is the tempting assertion and it cannot
 * fail honestly: whole-word narrows a search on most documents and widens it on
 * none, so "fewer than before" passes for a fixture that simply has fewer of
 * something, and it passes for an option that was dropped on the way to the
 * worker whenever the document happens to agree.
 *
 * The waits are on `searching` **and** `scanned`, where the checks above wait on
 * `scanned` alone. `Search.run` sets `running` synchronously before its first
 * await, so a wait that includes it cannot be satisfied by the *previous* scan's
 * finished state --- which is a race the `scanned`-only form has on entry. The
 * `scanned` half is kept because it is what notices a missing generation guard,
 * for the reason the negative control above spells out.
 */
async function searchOptionChecks(
  viewer: Viewer,
  seen: { status: ViewerStatus | null; updates: number },
  needle: string,
  firstHit: { page: number; start: number } | undefined,
  pageCount: number,
): Promise<void> {
  const done = (): boolean =>
    !viewer.searching && (seen.status?.search.scanned ?? 0) >= pageCount;
  const hitAtFirst = (): boolean =>
    !!firstHit &&
    viewer.searchMatches.some(
      (m) => m.page === firstHit.page && m.start === firstHit.start,
    );

  if (!firstHit) {
    skipSearchOptions("the plain search found nothing to reason about");
    return;
  }

  const shouted = needle.toUpperCase();
  if (shouted === needle) {
    skip(
      SEARCH_OPTION_CHECKS[0],
      `"${needle}" is already upper case, so there is no spelling of it that ` +
        "matching case would reject",
    );
  } else {
    // `case is ignored` has just established that this exact query finds this
    // exact occurrence. Turning the option on must stop it, because the text
    // there is spelled the other way.
    viewer.setSearchOptions({ ...PLAIN_SEARCH, matchCase: true });
    viewer.search(shouted);
    await settle(done);
    check(
      SEARCH_OPTION_CHECKS[0],
      !hitAtFirst(),
      `"${shouted}" with match-case -> ${viewer.searchMatches.length} matches, none at ` +
        `${firstHit.page}:${firstHit.start} where "${needle}" is`,
    );
  }

  // A proper prefix of the needle occurs at the needle's own position and is
  // followed there by a letter, so it is never a whole word there. `pickNeedle`
  // returns five letters or more, so the prefix is at least four.
  const stem = needle.slice(0, -1);
  viewer.setSearchOptions({ ...PLAIN_SEARCH, wholeWord: true });
  viewer.search(stem);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[1],
    !hitAtFirst(),
    `"${stem}" with whole-word -> ${viewer.searchMatches.length} matches, none at ` +
      `${firstHit.page}:${firstHit.start} where it is followed by ` +
      `"${needle.slice(-1)}"`,
  );

  // The control, and the one that says the two above rejected something rather
  // than the search having stopped working. Same query, options off, hit back.
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.search(stem);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[2],
    hitAtFirst(),
    `"${stem}" unrestricted -> ${viewer.searchMatches.length} matches, including ` +
      `${firstHit.page}:${firstHit.start}`,
  );

  // A pattern built from the needle by replacing its second character with a
  // dot. As a pattern it must find the needle where the needle is; as a literal
  // the same string contains a dot the page does not have there, so it finds
  // nothing --- which is the control that says the option is what did it rather
  // than the query happening to match twice.
  const pattern = `${needle[0] ?? ""}.${needle.slice(2)}`;
  viewer.setSearchOptions({ ...PLAIN_SEARCH, regex: true });
  viewer.search(pattern);
  await settle(done);
  const asPattern = viewer.searchMatches.length;
  const foundHere = hitAtFirst();
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.search(pattern);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[3],
    foundHere && asPattern > 0 && viewer.searchMatches.length === 0,
    `"${pattern}" as a pattern -> ${asPattern} matches including ` +
      `${firstHit.page}:${firstHit.start}; as a literal -> ` +
      `${viewer.searchMatches.length}`,
  );

  // An unclosed group. The reader has to be told the pattern is broken, because
  // "no matches" for it is a statement about the document instead.
  //
  // Both reads below wait for a status DELIVERED AFTER THE SCAN STOPPED, and
  // the emphasis is the whole of a fix made on 2026-08-12. `seen` is a mirror
  // the viewer fills through `onStatus`; `viewer.searching` is a live read of
  // the searcher. Those are different clocks.
  //
  // The wait used to be `!viewer.searching && seen.updates > beforeBroken`, and
  // it flaked --- twice in about seven runs on the day it was fixed, against a
  // comment here that recorded once in three.
  //
  // `Search.run` emits a status at the *start* of a scan, on purpose, because a
  // search over 775 pages has to be visibly working. But `onSearchProgress`
  // only calls `wake()`, so delivery waits for the next frame --- and a broken
  // pattern is rejected so fast that the start and the completion normally both
  // land before that frame. The mirror then sees one status carrying the final
  // state, and the check passes. That is the usual run, and it is why this
  // looked fine for weeks.
  //
  // Occasionally a frame lands *between* them. Now the first status delivered
  // after `beforeBroken` is the start one: `running: true`, `problem: ""`. The
  // counter half of the old wait was satisfied by exactly that status --- the
  // event it existed to exclude --- and its other half, `!viewer.searching`, is
  // a live read of the searcher that went false the moment the invoke resolved.
  // Both true, mirror holding the start state, `problem` read as empty.
  //
  // Reading `running` OUT OF THE MIRROR fixes it by construction rather than by
  // timing: whichever of the two statuses is delivered, only one taken after
  // the scan stopped satisfies this. The counter stays and is still doing work
  // --- it excludes the idle status from *before* the search, which also reads
  // `running: false`.
  //
  // Waiting on `problem` itself would be the obvious fix and the wrong one: it
  // is the value being asserted, so the check could then only pass or hang.
  //
  // A control asserting "the start status is always delivered first" was
  // written before this and is deliberately **not** here: it went red on the
  // first run, which is what corrected the account above. The start status is
  // usually coalesced away, so that control asserted the race rather than the
  // behaviour --- a check whose truth depends on the timing it is meant to
  // remove. There is no deterministic control for this from inside the harness;
  // what stands behind the fix is the construction argument plus repetition.
  const settledScan = (since: number) =>
    seen.updates > since && seen.status?.search.running === false;

  const broken = `${needle}(`;
  viewer.setSearchOptions({ ...PLAIN_SEARCH, regex: true });
  const beforeBroken = seen.updates;
  viewer.search(broken);
  await settle(() => settledScan(beforeBroken));
  const problem = seen.status?.search.problem ?? "";
  // The control: the same characters as a literal are a perfectly ordinary
  // query, so a `problem` there would mean the reporting is about the text
  // rather than about the pattern. It needs the same wait for the opposite
  // reason --- a stale mirror here still holds the problem set just above, so
  // reading too early fails the control rather than passing it.
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  const beforeLiteral = seen.updates;
  viewer.search(broken);
  // `done()` reads the live `viewer.searching` and the mirror's `scanned`, so
  // it carries the same defect as the wait above did; `settledScan` is what
  // makes the delivered status the post-scan one. Both, because this one also
  // needs the whole document scanned rather than merely stopped.
  await settle(() => settledScan(beforeLiteral) && done());
  check(
    SEARCH_OPTION_CHECKS[4],
    problem !== "" && (seen.status?.search.problem ?? "") === "",
    `"${broken}" as a pattern -> "${preview(problem)}"; as a literal -> ` +
      `"${preview(seen.status?.search.problem ?? "")}", ` +
      `${viewer.searchMatches.length} matches`,
  );
  viewer.setSearchOptions({ ...PLAIN_SEARCH });
  viewer.clearSearch();
}

/**
 * The scan starts at the page being read, not at page 1.
 *
 * A reader on page 700 who searches for a common word wants the next one, not a
 * jump to the beginning of the document. Nothing else here would notice if that
 * were wrong: every other search check runs from `Home`, where "starts at the
 * current page" and "starts at page 1" are the same thing.
 */
async function searchesFromHere(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
  needle: string,
  pageCount: number,
): Promise<void> {
  const name = "searches forward from the page being read";
  // Both names, on every path out. This function records *two* checks, and its
  // early returns skipped only the first --- so on a one-page document the
  // second did not fail, did not skip, and did not appear at all. The name set
  // is the invariant here (86 across every corpus but one, which is how it was
  // found); a check that evaporates is invisible in a way a red one is not.
  const scanned = "finds something from the end of the document";
  if (pageCount < 2) {
    skip(name, "the document has one page");
    skip(scanned, "the document has one page");
    return;
  }

  key(root, "End");
  await settle(() => viewer.idle);
  const from = (seen.status?.page ?? 1) - 1;
  if (from === 0) {
    skip(name, "the whole document fits on one screen");
    skip(scanned, "the whole document fits on one screen");
    return;
  }

  viewer.search(needle);
  const found = await eventually(
    scanned,
    () => viewer.searchMatches.length > 0,
    () => `${viewer.searchMatches.length} matches so far`,
  );
  const first = viewer.searchMatches[0];
  // A needle with nothing at or after the page being read cannot distinguish the
  // two answers: the scan is *supposed* to wrap when there is nothing ahead, so
  // a first hit before `from` is correct there and a defect anywhere else. The
  // check reported that as a failure on the first two-page fixture it met ---
  // the needle comes from page 1 and this document does not repeat it --- which
  // is a check that cannot tell its own subject from its precondition.
  if (found && !viewer.searchMatches.some((m) => m.page >= from)) {
    skip(
      name,
      `no match at or after page ${from + 1}, so wrapping to the start is correct`,
    );
    return;
  }
  check(
    name,
    found && !!first && first.page >= from,
    `reading page ${from + 1}, first hit on page ${first ? first.page + 1 : "none"}` +
      (first && first.page < from
        ? " -- the scan restarted at the beginning"
        : ""),
  );
}

/**
 * Steps through matches with Cmd-G until one is on a different page.
 *
 * The check is that the viewport followed, and it needs the control beside it:
 * "the viewport is on the match's page" is trivially true if the match was on
 * the page already, which is how a jump check on a 775-page document once passed
 * without the jump having rendered anything. So the target is the first match
 * on a page other than the one being read, the precondition that we are *not*
 * there is asserted first, and a document that cannot offer one is skipped with
 * its reason printed rather than silently dropped.
 */
async function stepToAnotherPage(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
  needle: string,
  pageCount: number,
): Promise<void> {
  const name = "Cmd-G moves to a match on another page";
  const spread = "counts more than the matches on one page";
  if (pageCount < 2) {
    skip(spread, "the document has one page");
    skip(name, "the document has one page");
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle);
  viewer.search(needle);
  // Waited out to the *end of the search*, not until matches appear on two
  // pages. Written the second way first, and it is a wait for a condition that
  // may never hold: on a document whose needle sits on one page it spent its
  // whole bound and then reported a failure, when what it had established is
  // that this fixture cannot exercise the check below. That is a precondition
  // rather than a claim about searching, so it says so.
  await settle(() => !viewer.searching);
  const pages = new Set(viewer.searchMatches.map((m) => m.page));
  const detail = `${viewer.searchMatches.length} matches across ${pages.size} pages`;
  if (pages.size < 2) {
    skip(spread, detail);
    skip(name, detail);
    return;
  }
  check(spread, true, detail);

  const start = viewer.searchMatches[0]?.page ?? 0;
  const target = viewer.searchMatches.findIndex((m) => m.page !== start);
  if (target < 0) {
    skip(name, `every match is on page ${start + 1}`);
    return;
  }

  const goal = viewer.searchMatches[target];
  const before = (seen.status?.page ?? 0) - 1;
  if (!goal || before === goal.page) {
    skip(name, `already on page ${goal ? goal.page + 1 : "?"} before stepping`);
    return;
  }

  // One press per step, each awaited: `goToMatch` has to load the target page's
  // text before it knows where to scroll, so pressing faster than that races the
  // reply it is waiting for.
  for (let step = 0; step < target; step++) {
    const at = seen.status?.search.index ?? 0;
    key(root, "g", true);
    await settle(() => (seen.status?.search.index ?? 0) !== at);
  }
  await settle(() => seen.status?.page === goal.page + 1);

  check(
    name,
    seen.status?.page === goal.page + 1,
    `${target} presses from page ${before + 1} to page ${seen.status?.page}, ` +
      `match ${target + 1} is on page ${goal.page + 1}`,
  );
}

/**
 * What a screen reader would find.
 *
 * Four claims, and only the first is the obvious one. That the text is *there*;
 * that it is the page's own text and not some other page's; that a page which
 * stays on screen keeps the **same element**, so a reading cursor survives a
 * scroll; and that the canvas is out of the tree so the text is not doubled by a
 * large empty region.
 *
 * The third is the one the architecture exists for --- `docs/PLAN.md` §8 says
 * recycling containers destroys focus --- and it is asserted by putting real
 * focus in the element and checking it is still there afterwards, with a control
 * that the page was still visible. "The element is still in the DOM" would pass
 * on a rebuilt element that had thrown the cursor away.
 *
 * What is **not** checked, and is not claimed: that any of this is *pleasant* to
 * listen to. That needs a screen reader and a person.
 */
/** The check that a guessed page is not read aloud, named once for its skips. */
const GUESSED_TEXT_CHECK =
  "a page whose characters mean nothing is not read out";

async function accessibilityChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle);
  await eventually(
    "the visible page reaches the accessibility tree",
    () => viewer.accessibleText.elementFor(0) !== null,
    () =>
      `pages present: ${viewer.accessibleText.present.join(", ") || "none"}`,
  );

  const extracted = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const spoken = spokenText(viewer.accessibleText.elementFor(0));

  // Three states, not two, and the third is why this branch was restructured on
  // 2026-08-02: a page can have text of the right length that means nothing.
  // `a11y.ts` withholds those characters and gives the reason instead, so the
  // check below --- "what is read out is the page's own text" --- is *false by
  // design* there and failed the first time `encodings.pdf` was ever opened in a
  // window. Which page that is comes from the fixture's manifest, never from
  // `unreadablePage`: branching on the subject's own answer would make a layer
  // that had stopped withholding skip the check instead of failing it.
  const manifest = await manifestOf();
  const guessed = new Set(
    (manifest?.pages ?? [])
      .filter((page) => page.name === UNMAPPED_PAGE)
      .map((page) => page.page),
  );

  if (!extracted || extracted.codes.length === 0) {
    check(
      "a page with no text says so rather than falling silent",
      spoken.includes("no extractable text"),
      `reads "${preview(spoken)}"`,
    );
    skip(
      "the text read out is the page's own text",
      "the page has no extractable text",
    );
    skip(GUESSED_TEXT_CHECK, "the page has no extractable text");
  } else if (guessed.has(0)) {
    skip(
      "a page with no text says so rather than falling silent",
      "this page has text",
    );
    skip(
      "the text read out is the page's own text",
      "this page's characters are a guess, so what is read out is deliberately " +
        "not them",
    );
    const characters = flatten(String.fromCodePoint(...extracted.codes));
    // The second half is the load-bearing one. An element holding the notice
    // *and* the characters would be read out in full, which is the outcome
    // being avoided, and a check for the notice alone passes on it.
    const sample = characters.slice(0, 8);
    check(
      GUESSED_TEXT_CHECK,
      spoken.includes("cannot be read") &&
        sample.length > 0 &&
        !spoken.includes(sample),
      `reads "${preview(spoken)}"; the page extracts as "${preview(characters)}"`,
    );
  } else {
    skip(
      "a page with no text says so rather than falling silent",
      "this page has text",
    );
    skip(GUESSED_TEXT_CHECK, "this page states what its characters mean");
    // Compared against an independent extraction, not against the viewer's
    // cache, so the layer cannot be confirmed by agreeing with itself.
    const expected = flatten(String.fromCodePoint(...extracted.codes));
    // Compared as a *multiset*, not as a string, and that is a real weakening
    // made for a real reason: the tree is now built in reading order, and on
    // `rotated-90.pdf` PDFium's extraction runs the other way --- so exact
    // equality started failing against a layer that had just been made correct.
    //
    // What survives is what this check is named for: the text read out is this
    // page's, entire, and not invented. What it can no longer say is anything
    // about the order, which is asserted instead by the two reading-order checks
    // against the fixture's own manifest and by `reading.test.ts`.
    const sorted = (text: string): string => [...text].sort().join("");
    const same = sorted(spoken) === sorted(expected);
    const moved = [...spoken].filter(
      (char, at) => char !== expected[at],
    ).length;
    check(
      "the text read out is the page's own text",
      same,
      same
        ? `${spoken.length} characters match the extraction, ${moved} in another position`
        : `reads ${spoken.length} characters, extraction has ${expected.length}: ` +
            `"${preview(spoken)}" vs "${preview(expected)}"`,
    );
  }

  await linkAnnouncementChecks(viewer);

  await structureChecks(doc, viewer.accessibleText.elementFor(0));

  // Hidden visually, present in the tree. `display:none` and
  // `visibility:hidden` both remove an element from the accessibility tree, so
  // either would make the whole layer do nothing while every other check here
  // still passed --- textContent reads the same from a hidden element.
  const host = viewer.accessibleText.elementFor(0)?.parentElement ?? null;
  const style = host ? getComputedStyle(host) : null;
  const box = host?.getBoundingClientRect();
  check(
    "the text is hidden visually but not from the tree",
    !!style &&
      style.display !== "none" &&
      style.visibility !== "hidden" &&
      !!box &&
      box.width <= 2 &&
      box.height <= 2,
    `display=${style?.display}, visibility=${style?.visibility}, ` +
      `box=${box?.width.toFixed(0)}x${box?.height.toFixed(0)}`,
  );

  check(
    "the canvas is hidden from the accessibility tree",
    [...root.querySelectorAll("canvas, div")].every(
      (element) =>
        !(element instanceof HTMLCanvasElement) ||
        element.closest("[aria-hidden='true']") !== null,
    ),
    `${root.querySelectorAll("canvas").length} canvases, all inside aria-hidden`,
  );

  // Focus survives a scroll that keeps the page on screen. The control is the
  // `if` below: on a document where a small scroll leaves the page, this proves
  // nothing and says so.
  const article = viewer.accessibleText.elementFor(0);
  article?.focus();
  const focused = document.activeElement === article;
  wheel(root, 60);
  await frame();
  const stillVisible = viewer.accessibleText.present.includes(0);

  if (!focused) {
    skip("focus in the text survives a scroll", "the element never took focus");
  } else if (!stillVisible) {
    skip(
      "focus in the text survives a scroll",
      "the scroll left the page entirely",
    );
  } else {
    check(
      "focus in the text survives a scroll",
      document.activeElement === article &&
        viewer.accessibleText.elementFor(0) === article,
      `same element=${viewer.accessibleText.elementFor(0) === article}, ` +
        `still focused=${document.activeElement === article}`,
    );
  }

  // And the other half: a page that leaves the screen leaves the tree, or a long
  // document would accumulate every page it had ever shown.
  const before = viewer.accessibleText.present.includes(0);
  key(root, "End");
  await settle(() => viewer.idle);
  const last = (seen.status?.page ?? 1) - 1;
  if (!before || doc.page_count < 2) {
    skip(
      "a page that leaves the screen leaves the tree",
      "the document has one screen",
    );
  } else {
    await eventually(
      "a page that leaves the screen leaves the tree",
      () =>
        !viewer.accessibleText.present.includes(0) &&
        viewer.accessibleText.present.includes(last),
      () =>
        `after End: pages ${viewer.accessibleText.present.join(", ") || "none"}`,
    );
  }
}

/**
 * What the page's blocks say, joined the way they are announced.
 *
 * Not `textContent`, which concatenates block elements with **nothing** between
 * them --- so a page of 56 lines read that way is 56 characters short of the
 * extraction it is being compared against, and the check reports a content
 * mismatch when the content is identical and only the separators are structural.
 * Joining the blocks is what makes the comparison about the text.
 */
function spokenText(article: HTMLElement | null): string {
  if (!article) return "";
  // Every child, not `querySelectorAll("p")`. A tagged page announces its
  // headings as `h1`--`h6`, and a selector naming one element silently stops
  // reading the page the day the layer gains another --- which would have shown
  // up here as the *page's text* being short by a heading, i.e. as a defect in
  // extraction rather than in this line.
  return flatten(
    [...article.children].map((child) => child.textContent ?? "").join(" "),
  );
}

/**
 * The document's own reading order, where it reaches the tree.
 *
 * `structure_probe` already asserts that the tags are read correctly, against a
 * fixture a different program wrote. What it cannot say is that anything
 * *consumes* them: the runs travel on `PageText` and the decision to use them is
 * `usableRuns`, and a wiring mistake anywhere along that path leaves a viewer
 * that reads the tree perfectly and shows the geometry's answer.
 *
 * So the assertion is differential and it is made twice. The same page is laid
 * out through `readingLines` with the runs and with them stripped, which is the
 * geometric answer by construction --- and the first check is that those two
 * *differ*, because on a fixture where they agree the second one is vacuous and
 * would pass whether or not a single tag was read. The second is that the
 * paragraphs actually in the accessibility tree are the tagged answer.
 *
 * Reading the DOM rather than calling `readingLines` again is the point of the
 * second one: it is the only thing here that can see the tree being built from
 * something else.
 */
async function structureChecks(
  doc: DocumentInfo,
  article: HTMLElement | null,
): Promise<void> {
  const names = [
    "a tagged page's reading order is not the one its geometry gives",
    "the accessibility tree is built in the order the tags give",
    "a heading is announced as a heading, at the document's own level",
    "nothing the document did not call a heading becomes one",
  ] as const;
  const text = await new TextCache(doc.id).load(0);
  const runs = text ? usableRuns(text) : null;
  if (!text || !runs) {
    const why = !text
      ? "the page's text did not arrive"
      : text.runs && text.runs.length > 0
        ? "this page's tags do not cover all of its visible text"
        : "this document carries no structure tree";
    for (const name of names) skip(name, why);
    return;
  }

  /** A page's non-empty lines, in whatever order the text it is given implies. */
  const lines = (of: PageText): string[] =>
    readingLines(of)
      .map((line) => flatten(textOfRanges(of, line.ranges)))
      .filter((line) => line.length > 0);

  const tagged = lines(text);
  // Stripped rather than a separate code path: the geometric answer is exactly
  // what this file's own layer produces for a document with no tags, so there is
  // no second implementation here to be wrong in its own way.
  const geometric = lines({ ...text, runs: [] });
  check(
    names[0],
    tagged.length > 0 && tagged.join("\n") !== geometric.join("\n"),
    `${runs.length} runs, ${tagged.length} lines; tags start "${preview(tagged[0] ?? "")}", ` +
      `geometry starts "${preview(geometric[0] ?? "")}"`,
  );

  const spoken = spokenText(article);
  check(
    names[1],
    spoken === flatten(tagged.join(" ")) &&
      spoken !== flatten(geometric.join(" ")),
    `tree reads "${preview(spoken)}"`,
  );

  // Headings, which are the reason to read the types at all: "jump to the next
  // heading" is how a screen-reader user skims, and it works on `h1`--`h6` and on
  // nothing else. Asserted against the *document's* types rather than a count, so
  // a page with no heading skips instead of passing vacuously.
  const wanted = runs.filter((run) => /^H[1-6]?$/.test(run.tag));
  const headings = [...(article?.children ?? [])].filter((child) =>
    /^H[1-6]$/.test(child.tagName),
  );
  const levels = headings
    .map((child) => child.tagName.toLowerCase())
    .join(", ");
  if (wanted.length === 0) {
    skip(names[2], "this page's tags contain no heading");
  } else {
    check(
      names[2],
      headings.length === wanted.length &&
        wanted.every(
          (run, at) =>
            headings[at]?.tagName.toLowerCase() ===
            (run.tag === "H" ? "h2" : run.tag.toLowerCase()),
        ),
      `tags say [${wanted.map((r) => r.tag).join(", ")}], tree has [${levels}]`,
    );
  }

  // And the other half, over *every* block rather than the headings: a layer that
  // emitted `h1` for everything would pass the check above, which only asks that
  // the headings it wanted are present. Each block carries the document's own word
  // for it in `data-tag`, so the whole mapping is checkable in one pass, and
  // `data-tag` exists for exactly this --- a type flattened to `p` with nothing
  // recording it cannot be told from a type nobody handled.
  const expected = (tag: string | null): string => {
    if (tag === null) return "p";
    const level = /^H([1-6])$/.exec(tag);
    if (level) return `h${level[1]}`;
    return tag === "H" ? "h2" : "p";
  };
  const wrong = [...(article?.children ?? [])]
    .map((child) => ({
      tag: child.getAttribute("data-tag"),
      is: child.tagName.toLowerCase(),
    }))
    .filter((block) => block.is !== expected(block.tag));
  check(
    names[3],
    wrong.length === 0,
    wrong.length === 0
      ? `${article?.children.length ?? 0} blocks, each the element its type asks for`
      : wrong.map((b) => `${b.tag} became <${b.is}>`).join(", "),
  );
}

/** Collapses whitespace, so a line break is not a difference in content. */
function flatten(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * A search confined to what the reader selected.
 *
 * The discriminating part is not that a scoped search finds fewer things --- a
 * search that found nothing would do that too. It is that the *same* query,
 * unscoped, finds strictly more, and that everything the scoped one found lies
 * inside the range that was drawn. Both directions, because a scope that
 * excluded everything and a scope that excluded nothing are the two ways this
 * can be wrong and each looks fine from the other side.
 */
async function scopedSearchChecks(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const NAMES = [
    "a scoped search looks only inside the selection",
    "and the same query unscoped finds more",
  ] as const;
  const skipBoth = (why: string): void => {
    for (const name of NAMES) skip(name, why);
  };

  viewer.goToStart();
  viewer.clearSelection();
  await settle(() => viewer.idle);

  const done = (): boolean =>
    !viewer.searching &&
    (seen.status?.search.scanned ?? 0) >= (seen.status?.search.toScan ?? 1);

  // A drag that starts part-way along a line and ends part-way down the page,
  // so the scope is a *range within* a page. Dragging the whole page would
  // leave both of the range's ends untested: a mutation relaxing the start
  // bound survived exactly that, because the selection began at character 0 and
  // "at or after 0" is true of everything.
  drag(root, [WIDTH / 2, HEIGHT / 5], [WIDTH - 120, HEIGHT * 0.55]);
  await settle(() => viewer.idle);
  const selected = viewer.selectedText;
  if (selected === "") {
    skipBoth("dragging over the page selected nothing");
    return;
  }

  // The query comes out of the selection, so it is certain to be inside the
  // scope. Taken from the page instead it might not be, and a scoped search
  // that finds nothing satisfies "fewer than before" perfectly.
  const needle = pickNeedle([...selected].map((ch) => ch.codePointAt(0) ?? 0));
  if (!needle) {
    skipBoth(
      `the ${selected.length} selected characters yielded no word to search for`,
    );
    return;
  }

  viewer.search(needle);
  await settle(done);
  const whole = viewer.searchMatches.length;
  // Hits on the selection's own page, which is the number that makes this a
  // check on the *range*. A scoped scan only asks about the pages in the scope,
  // so comparing against the whole document proves the page list and says
  // nothing about the two ends of it --- and two mutations relaxing exactly
  // those ends survived while this compared against `whole`.
  const onFirstPage = viewer.searchMatches.filter((m) => m.page === 0);
  if (whole < 2) {
    skipBoth(
      `"${needle}" occurs ${whole} time(s) in the document, so a narrower search proves nothing`,
    );
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }

  if (!viewer.scopeSearchToSelection()) {
    skipBoth("the selection could not be scoped to");
    return;
  }
  if (onFirstPage.length < 2) {
    skipBoth(
      `"${needle}" occurs ${onFirstPage.length} time(s) on page 1, so clipping the range proves nothing`,
    );
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }
  await settle(done);
  const scoped = viewer.searchMatches.slice();
  const outside = scoped.filter((m) => m.page !== 0 || m.endPage !== undefined);

  // Both ends of the range, and both measured against the **scope** rather than
  // against the matches that came back. Two mutations relaxing one bound each
  // survived a version that used the results: widening a bound widens the
  // numbers the precondition is computed from too, so the check turned itself
  // into a `[SKIP]` instead of going red --- a defect that switches off the
  // check that would have caught it.
  const range = viewer.searchScopeRanges?.[0];
  const droppedBefore = range
    ? onFirstPage.filter((m) => m.start < range.from).length
    : 0;
  const droppedAfter = range
    ? onFirstPage.filter((m) => m.end > range.to).length
    : 0;
  if (!range || droppedBefore === 0 || droppedAfter === 0) {
    skipBoth(
      `"${needle}": the scope is ${range ? `[${range.from}, ${range.to})` : "absent"}, with ` +
        `${droppedBefore} of page 1's hits before it and ${droppedAfter} after it --- ` +
        `both ends need something to drop`,
    );
    viewer.clearSearchScope();
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }
  check(
    NAMES[0],
    viewer.searchScoped &&
      outside.length === 0 &&
      scoped.length > 0 &&
      scoped.length === onFirstPage.length - droppedBefore - droppedAfter,
    `"${needle}": ${scoped.length} matches in [${range.from}, ${range.to}), and page 1 has ` +
      `${onFirstPage.length} with ${droppedBefore} before that range and ${droppedAfter} ` +
      `after it (${whole} in the document); ${outside.length} outside the scope`,
  );

  viewer.clearSearchScope();
  await settle(done);
  check(
    NAMES[1],
    !viewer.searchScoped &&
      viewer.searchMatches.length === whole &&
      whole > scoped.length,
    `unscoped -> ${viewer.searchMatches.length}, was ${whole} before scoping and ` +
      `${scoped.length} while scoped`,
  );

  viewer.clearSearch();
  viewer.clearSelection();
  await settle(() => viewer.idle);
}

/**
 * A phrase that runs over a page break.
 *
 * The query is built from the document rather than written here: the last word
 * on page 1 and the first word on page 2, which by construction occur in that
 * order with nothing but the break between them. A matcher that looks at one
 * page at a time cannot find it, which is what makes the check discriminating
 * without needing a fixture built for it.
 *
 * Two things are asserted, and the second is the one that matters. That
 * *something* was found is weak --- a hit anywhere would satisfy it. So each
 * half is resolved against the page it claims to be on, through a fresh
 * extraction rather than through the matcher's own snippet, and has to be the
 * word that page really ends or begins with.
 */
async function crossPageChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const NAMES = [
    "a phrase is found across a page break",
    "each half of it lands on the page it names",
  ] as const;
  const skipBoth = (why: string): void => {
    for (const name of NAMES) skip(name, why);
  };

  if (doc.page_count < 2) {
    skipBoth("a break needs two pages");
    return;
  }
  // The **codes**, not a string, and that distinction is the whole reason this
  // helper does not return one. A match's `start` and `end` are code point indices,
  // and `String.prototype.slice` counts UTF-16 code units --- so on a page holding a
  // character above the BMP the two spaces differ by one per such character, and a
  // slice comes back one short. `encodings.pdf` is where it showed: two broken
  // `/ToUnicode` entries decode to one astral character, and the right-hand half of
  // the phrase lost its last letter while the comment below claimed to be checking
  // the index spaces against the pages.
  const codesOf = async (page: number): Promise<number[] | null> => {
    const got = await invoke<{ codes: number[] }>("page_text", {
      doc: doc.id,
      page,
    }).catch(() => null);
    return got ? got.codes : null;
  };
  const textOf = async (page: number): Promise<string | null> => {
    const codes = await codesOf(page);
    return codes ? String.fromCodePoint(...codes) : null;
  };
  /** A page's text between two **code point** indices. */
  const sliceOf = async (
    page: number,
    from: number,
    to?: number,
  ): Promise<string> => {
    const codes = await codesOf(page);
    return codes ? String.fromCodePoint(...codes.slice(from, to)) : "";
  };
  const first = await textOf(0);
  const second = await textOf(1);
  const last = /(\S+)\s*$/.exec(first ?? "")?.[1];
  const head = /^\s*(\S+)/.exec(second ?? "")?.[1];
  if (!last || !head) {
    skipBoth("page 1 or page 2 has no extractable text");
    return;
  }
  const query = `${last} ${head}`;
  if (query.length > 128) {
    skipBoth(`"${preview(query)}" is longer than a break is looked across`);
    return;
  }

  viewer.goToStart();
  await settle(() => viewer.idle);
  viewer.search(query);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );

  const across = viewer.searchMatches.find((m) => m.endPage !== undefined);
  check(
    NAMES[0],
    across !== undefined,
    `"${preview(query)}" -> ${viewer.searchMatches.length} matches, ` +
      `${viewer.searchMatches.filter((m) => m.endPage !== undefined).length} of them across a break`,
  );

  if (!across) {
    skip(NAMES[1], "nothing was found to resolve");
    viewer.clearSearch();
    return;
  }
  // Re-extracted, so the two index spaces are checked against the pages rather
  // than against the reply that reported them --- and sliced by code point, which
  // is the space the indices are in. See `sliceOf`.
  const left = await sliceOf(across.page, across.start);
  const right = await sliceOf(across.endPage ?? -1, 0, across.end);
  check(
    NAMES[1],
    across.page === 0 &&
      across.endPage === 1 &&
      left.trim() === last &&
      right.trim() === head,
    `page ${across.page} from ${across.start} is "${preview(left)}" (wanted "${preview(last)}"), ` +
      `page ${across.endPage} to ${across.end} is "${preview(right)}" (wanted "${preview(head)}")`,
  );
  viewer.clearSearch();
}

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
async function paletteChecks(viewer: Viewer, pageCount: number): Promise<void> {
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
async function appCommandChecks(
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
    viewer: () => viewer,
    pageCount: () => doc.page_count,
    openDocument: () => fired.push("openDocument"),
    reloadDocument: () => fired.push("reloadDocument"),
    busyOpening: () => busy,
    printDocument: () => fired.push("printDocument"),
    focusFind: () => fired.push("focusFind"),
    toggleSearchOption: (which) => fired.push(`toggleSearchOption:${which}`),
    toggleSearchScope: () => fired.push("toggleSearchScope"),
    toggleSidebar: () => fired.push("toggleSidebar"),
    showTab: (tab) => fired.push(`showTab:${tab}`),
    toggleInvert: () => fired.push("toggleInvert"),
    // Recorded like the rest, though neither command is driven here --- both are
    // in `undriven` above. A recorder rather than a throw, so that the day one
    // of them *is* driven the failure is a missing entry in `fired` rather than
    // an exception from a helper nobody was looking at.
    checkForUpdates: () => fired.push("checkForUpdates"),
    applyUpdate: () => fired.push("applyUpdate"),
    // False both, so the install command's `enabled` guard is exercised in the
    // direction the check can assert: it must not appear in the palette on a
    // run where nothing has been found.
    updateReady: () => false,
    updateAvailable: () => false,
    // The page operations. Recorders like the rest of the shell half: the model
    // that decides what a page's turn becomes is in the backend, so what these
    // commands can be checked for here is that they reach the right action ---
    // and the `enabled` guards below are what needs the two booleans to be
    // separately settable.
    rotatePage: (delta) => fired.push(`rotatePage:${delta}`),
    deletePage: () => fired.push("deletePage"),
    movePage: (delta) => fired.push(`movePage:${delta}`),
    undoEdit: () => fired.push("undoEdit"),
    redoEdit: () => fired.push("redoEdit"),
    canUndo: () => canUndo,
    canRedo: () => canRedo,
    saveCopy: () => fired.push("saveCopy"),
    extractPages: (slots: number[]) => fired.push(`extractPages:${slots.join("+")}`),
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
    /** Puts the viewer somewhere the command can be seen to move it from. */
    from?: () => void;
    /** Reads what the command is supposed to change. */
    read: () => string;
    /** Whether the reading moved as the command promises. */
    moved: (before: string, after: string) => boolean;
    /** Why this document cannot exercise it, or null. */
    unless?: () => string | null;
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
    {
      id: "file.print",
      ...shell("printDocument"),
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
    "find.next": "needs a live search with more than one match",
    "find.previous": "needs a live search with more than one match",
    "edit.copy": "its outcome is the system clipboard",
    // Driving it would reopen the document, which replaces the viewer, the
    // text cache and the tile state that every later check reads --- so it
    // cannot run here without ending the run. Its wiring is covered by
    // `appcommands.test.ts` instead: that it reaches its own action and no
    // other, is withheld with no document, and ranks first for its own name.
    "file.reload": "it reopens the document, discarding the state later checks read",
    // Driving either would reach the network from a check that is otherwise
    // entirely offline, and the install one would replace the running binary
    // mid-run. Their wiring is covered by `appcommands.test.ts` and their
    // behaviour by `update.test.ts`, which fakes the plugin; what neither
    // covers is a real endpoint and a real signature, and `BUILD.md` schedules
    // that as a manual step because it needs a published release to check
    // against.
    "app.checkForUpdates": "it would reach the network from an offline check",
    "app.installUpdate": "it would replace the running binary mid-run",
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
    probe.from?.();
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
  const NEEDS_MORE_THAN_A_DOCUMENT = ["app.installUpdate", "find.inSelection"];

  // And the other direction, declared for the same reason. This list was the
  // literal `file.open` until the updater landed, and the check went red ---
  // correctly, because "needs no document" is a claim each command has to earn
  // rather than a fact about how many there are. Two earn it: opening one is
  // how a reader gets a document at all, and checking for updates has nothing
  // to do with documents. Installing one is NOT here: it is guarded on a check
  // having found something, so it belongs above.
  const NEEDS_NO_DOCUMENT = ["file.open", "app.checkForUpdates"];

  viewer.clearSelection();
  await settle(() => viewer.idle);

  const detached = new CommandRegistry();
  registerAppCommands(detached, { ...actions, viewer: () => null });
  const withoutDocument = detached
    .search("")
    .map((ranked) => ranked.command.title);

  palette.open();
  const withDocument = palette.visible.slice();
  palette.close();
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

/**
 * The comments: the panel that lists them, and the note that opens on the page.
 *
 * Most of the corpus has no annotations at all, so nearly everything here is
 * conditional --- and every condition is a `skip` with its reason rather than a
 * missing row, for the reason this whole file is arranged around.
 *
 * Two assertions carry the weight and each carries its own control:
 *
 * - **Pressing a mark opens its note**, asserted after checking that no note was
 *   open beforehand. Without that, a check that opened one for its own
 *   convenience would pass on a press that did nothing.
 * - **The note says what the file says**, compared against the body the backend
 *   returned rather than against "some text is showing". A popup rendering an
 *   empty string is a popup, and the check that only asks whether one is open
 *   cannot tell the two apart.
 *
 * The press lands on the *centre* of a comment's rectangle. That is not a
 * convenience either: `hitTest` allows a few points of slack around the edge, so
 * a press aimed at a corner would pass whether or not the rectangle is where the
 * scan says it is.
 */
async function commentChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
): Promise<void> {
  const names = [
    "the sidebar lists every comment",
    "a reply is drawn under the comment it answers",
    "pressing a mark on the page opens its note",
    "the note shows what the comment says",
    "a reply appears in the note with its own author",
    "pressing away from a mark closes the note",
    "activating a row opens that comment's note",
  ];

  let comments: Comments;
  try {
    comments = await invoke<Comments>("document_comments", { doc: doc.id });
  } catch (e) {
    check("reads the document's comments", false, String(e));
    for (const name of names) skip(name, "the comments could not be read");
    return;
  }
  check(
    "reads the document's comments",
    Array.isArray(comments.items),
    `${comments.items.length} comments in ${comments.scan_ms.toFixed(2)} ms`,
  );

  sidebar.setComments(comments);
  viewer.setComments(comments.items);

  if (comments.items.length === 0) {
    for (const name of names) skip(name, "the document has no comments");
    return;
  }

  check(
    "the sidebar lists every comment",
    sidebar.comments.rowCount === comments.items.length,
    `${sidebar.comments.rowCount} rows for ${comments.items.length} comments`,
  );

  const reply = comments.items.find(
    (item) => item.reply_to !== null && comments.items.some((other) => other.id === item.reply_to),
  );
  if (!reply) {
    skip("a reply is drawn under the comment it answers", "no comment is a reply");
  } else {
    const row = sidebar.comments.elementFor(reply.id);
    const describes = row?.getAttribute("aria-describedby") ?? "";
    check(
      "a reply is drawn under the comment it answers",
      describes === `tpdf-comment-${reply.reply_to}`,
      `describedby=${describes || "(none)"} for a reply to #${reply.reply_to}`,
    );
  }

  // A mark with a real rectangle that the document shows. A hidden one is
  // deliberately not clickable, and one with no area was never placed.
  const mark = comments.items.find(
    (item) =>
      !item.hidden && item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
  );
  if (!mark) {
    for (const name of names.slice(2)) {
      skip(name, "no comment has a rectangle on the page");
    }
    return;
  }

  viewer.goToPage(mark.page);
  await frame();
  await frame();
  await frame();
  const centre = viewer.screenPoint(
    mark.page,
    (mark.rect[0] + mark.rect[2]) / 2,
    (mark.rect[1] + mark.rect[3]) / 2,
  );
  // The control: nothing may be open before the press, or "a note is open"
  // afterwards says nothing about the press.
  const wasOpen = viewer.commentOpen;
  pointer(root, "pointerdown", centre.x, centre.y);
  pointer(root, "pointerup", centre.x, centre.y);
  check(
    "pressing a mark on the page opens its note",
    wasOpen === -1 && viewer.commentOpen === mark.id,
    `open=${viewer.commentOpen} for #${mark.id} at (${centre.x.toFixed(0)}, ${centre.y.toFixed(0)}), ` +
      `before=${wasOpen}`,
  );

  const said = viewer.commentText;
  const wanted = mark.body.trim() || mark.author.trim();
  check(
    "the note shows what the comment says",
    wanted.length > 0 ? said.includes(wanted) : said.length > 0,
    `note says ${JSON.stringify(said.slice(0, 60))}, wanted ${JSON.stringify(wanted.slice(0, 60))}`,
  );

  const answered = comments.items.filter((item) => item.reply_to === mark.id);
  if (answered.length === 0) {
    skip(
      "a reply appears in the note with its own author",
      "nothing replies to the comment under the pointer",
    );
  } else {
    const first = answered[0] as Comment;
    check(
      "a reply appears in the note with its own author",
      said.includes(first.body.trim()) &&
        (first.author.trim() === "" || said.includes(first.author.trim())),
      `note carries ${answered.length} repl${answered.length === 1 ? "y" : "ies"}`,
    );
  }

  // Away from every mark on this page: the far corner of the viewport, which no
  // rectangle in the corpus reaches. Asserted as a *change*, so a note that had
  // already closed itself does not read as this press closing it.
  const openBefore = viewer.commentOpen;
  pointer(root, "pointerdown", WIDTH - 20, HEIGHT - 20);
  pointer(root, "pointerup", WIDTH - 20, HEIGHT - 20);
  check(
    "pressing away from a mark closes the note",
    openBefore !== -1 && viewer.commentOpen === -1,
    `open ${openBefore} -> ${viewer.commentOpen}`,
  );

  // The *last* comment with a rectangle, and the viewer is sent back to the
  // top first: activating a row has to take the reader to the comment, and a
  // row for something already on screen cannot tell that from a popup that
  // opened where it stood. On a one-page document the two are the same comment
  // and the movement half is vacuous, which is why the detail prints both pages.
  const far = [...comments.items]
    .reverse()
    .find(
      (item) =>
        !item.hidden && item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
    );
  const row = far ? sidebar.comments.elementFor(far.id) : null;
  if (!far || !row) {
    skip("activating a row opens that comment's note", "the comment has no row");
    return;
  }
  viewer.goToStart();
  await settle(() => viewer.idle);
  row.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await frame();
  await frame();
  check(
    "activating a row opens that comment's note",
    viewer.commentOpen === far.id && viewer.position.page === far.page,
    `open=${viewer.commentOpen} for #${far.id}, viewer on page ${viewer.position.page + 1} ` +
      `of ${doc.page_count}, comment on ${far.page + 1}`,
  );
  // Left closed, so nothing after this runs with a note over the page.
  viewer.closeComment();
}

/**
 * Whether a screen reader is told that a cross-reference is a link.
 *
 * The half of the link work a sighted reader never sees, and the half that is
 * hardest to notice missing: the words are announced either way, so a table of
 * contents read as prose is indistinguishable from one read correctly unless
 * somebody is listening.
 *
 * Asserted against the **rendered tree** rather than against the links the
 * backend returned, and the two controls are what make it mean something: a
 * marked-up run must carry the role, and the page's ordinary text must not ---
 * a layer that put the role on every element would announce a page of links.
 */
async function linkAnnouncementChecks(viewer: Viewer): Promise<void> {
  const names = [
    "a link in the page is announced as a link",
    "the text around it is not",
    "no element that could carry a URL is created",
  ];

  // Whichever page the links live on, since most of the corpus has none at all.
  const first = viewer.firstLinkPage;
  if (first === -1) {
    for (const name of names) skip(name, "the document has no links");
    return;
  }
  viewer.goToPage(first);
  await settle(() => viewer.idle);

  const page = viewer.accessibleText.elementFor(first);
  if (!page) {
    for (const name of names) skip(name, `page ${first + 1} is not in the tree yet`);
    return;
  }

  const marked = [...page.querySelectorAll('[role="link"]')];
  check(
    names[0] as string,
    marked.length > 0,
    `${marked.length} announced on page ${first + 1}, ` +
      `first ${JSON.stringify((marked[0]?.textContent ?? "").slice(0, 40))}`,
  );

  // The control. Everything the page says, minus what the roles cover, has to
  // be non-empty --- otherwise the role is on the page element itself and the
  // check above passes for a layer that marks everything.
  const all = (page.textContent ?? "").trim();
  const inside = marked.map((node) => node.textContent ?? "").join("");
  check(
    names[1] as string,
    all.length > inside.length && inside.length > 0,
    `${all.length} characters on the page, ${inside.length} inside links`,
  );

  // T8, asserted on what was built rather than by reading the source. The gate
  // forbids creating these; this is the same claim from the other end.
  const urlBearing = page.querySelectorAll("a, iframe, img, embed, object").length;
  check(
    names[2] as string,
    urlBearing === 0,
    `${urlBearing} URL-bearing element(s) in the page's accessible text`,
  );
}

/**
 * Links: following one, being refused one, and getting back.
 *
 * The load-bearing assertion is **where the viewer ended up**, not that a click
 * was accepted: a link that scrolls nowhere and a link that scrolls to the wrong
 * page both look like a press the viewer took. So the target page is read out of
 * the link's own resolved destination and compared against `position.page`, and
 * the viewer is sent somewhere else first --- a jump to a page it is already on
 * cannot fail.
 *
 * The refusal has its own control, and it is the one worth stating. A refused
 * link must *not* move the reader, and "did not move" is exactly what a press
 * that missed the rectangle also produces. So the check asserts two things at
 * once: the position is unchanged **and** the refusal was reported. Without the
 * second half it would pass for a press that landed on blank paper.
 */
async function linkChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  /** Everything the viewer has reported through `onError`, in order. */
  problems: readonly string[],
): Promise<void> {
  const names = [
    "pressing a link goes where it points",
    "going back returns to where the link was pressed",
    "going forward returns to the destination",
    "a refused link says so and moves nobody",
    "the pointer changes over a link",
    "the keyboard reaches a link, and draws a ring on it",
    "Enter follows the link the keyboard is on",
    "Escape takes the keyboard off the link",
    "walking off the end says so rather than wrapping",
  ];

  let links: Links;
  try {
    links = await invoke<Links>("document_links", { doc: doc.id });
  } catch (e) {
    check("reads the document's links", false, String(e));
    for (const name of names) skip(name, "the links could not be read");
    return;
  }
  check(
    "reads the document's links",
    Array.isArray(links.items),
    `${links.items.length} links in ${links.scan_ms.toFixed(2)} ms`,
  );

  viewer.setLinks(links.items);
  check(
    "the viewer takes the links",
    viewer.linkCount === links.items.length,
    `${viewer.linkCount} held for ${links.items.length} read`,
  );

  if (links.items.length === 0) {
    for (const name of names) skip(name, "the document has no links");
    return;
  }

  /** Presses the centre of a link's rectangle, having scrolled to its page. */
  const pressCentre = async (link: Link): Promise<{ x: number; y: number }> => {
    viewer.goToPage(link.page);
    await settle(() => viewer.idle);
    const at = viewer.screenPoint(
      link.page,
      (link.rect[0] + link.rect[2]) / 2,
      (link.rect[1] + link.rect[3]) / 2,
    );
    pointer(root, "pointerdown", at.x, at.y);
    pointer(root, "pointerup", at.x, at.y);
    await settle(() => viewer.idle);
    return at;
  };

  // Which destination pages can actually become the page being read, measured
  // by going to each one rather than inferred from a page count.
  //
  // The last page of a document shorter than the window can never reach the
  // top of the viewport, so `position.page` never names it and every "the
  // link went where it points" assertion fails on a viewer that did exactly
  // what it was told. `nav.goToPage` and its two siblings already carry a
  // guard for this and it was never carried across to the links --- which is
  // the second half of the trap that guard came from: when one check's
  // precondition is strengthened, its siblings have the same one.
  //
  // Probed up front, in one pass over the distinct destination pages, so that
  // no check below has to move the viewer to find out. A probe run inside the
  // Enter check would leave the reader on the page Enter is meant to take
  // them to, and a jump to a page you are already on cannot fail.
  //
  // **Both ends of a link, not just the destination.** Back has to return to
  // the page the link was pressed *on*, so an unobservable origin fails a
  // check whose subject is the history rather than the page --- measured on
  // `links-rotated.pdf`, where the only link sits on the last page and the
  // check reported `back taken to page 1, wanted 2` against a viewer that had
  // never been able to be on page 2.
  const reachable = new Map<number, boolean>();
  const probe = async (page: number): Promise<void> => {
    if (reachable.has(page)) return;
    viewer.goToPage(page);
    await settle(() => viewer.idle);
    reachable.set(page, viewer.position.page === page);
  };
  for (const item of links.items) {
    await probe(item.page);
    if (item.target.kind === "page") await probe(item.target.page);
  }
  /** Why a page cannot be landed on observably, or `null` if it can. */
  const unreachable = (page: number): string | null =>
    reachable.get(page)
      ? null
      : `page ${page + 1} of ${doc.page_count} cannot reach the top of the ` +
        `viewport, so landing on it is not observable`;

  // A link that goes to a page the viewer will not already be on, so "it moved"
  // is observable. Its own page is excluded for the same reason.
  const candidates = links.items.filter(
    (item) =>
      item.target.kind === "page" &&
      item.target.page !== item.page &&
      item.rect[2] - item.rect[0] > 2 &&
      item.rect[3] - item.rect[1] > 2,
  );
  const goes = candidates.find(
    (item) =>
      item.target.kind === "page" &&
      reachable.get(item.page) === true &&
      reachable.get(item.target.page) === true,
  );

  if (!goes || goes.target.kind !== "page") {
    const other = candidates[0];
    const why =
      other && other.target.kind === "page"
        ? (unreachable(other.page) ??
          unreachable(other.target.page) ??
          "no link points at a different page")
        : "no link points at a different page";
    for (const name of names.slice(0, 3)) skip(name, why);
  } else {
    const from = goes.page;
    await pressCentre(goes);
    const landed = viewer.position.page;
    check(
      names[0] as string,
      landed === goes.target.page,
      `pressed on page ${from + 1}, landed on ${landed + 1}, wanted ${goes.target.page + 1}`,
    );

    // Back, then forward. Asserted as *positions*, because a history that
    // recorded the wrong end would still change the page and still look right
    // from a single assertion.
    const before = viewer.historyDepths;
    const went = viewer.goBack();
    await settle(() => viewer.idle);
    check(
      names[1] as string,
      went && viewer.position.page === from,
      `back ${went ? "taken" : "refused"} to page ${viewer.position.page + 1}, wanted ${from + 1}` +
        ` (stack was ${before.back})`,
    );

    const forward = viewer.goForward();
    await settle(() => viewer.idle);
    check(
      names[2] as string,
      forward && viewer.position.page === goes.target.page,
      `forward ${forward ? "taken" : "refused"} to page ${viewer.position.page + 1}, ` +
        `wanted ${goes.target.page + 1}`,
    );
  }

  // A refused link. `problems` is the harness's own record of what `onError`
  // reported, which is what makes "did not move" mean something.
  const refused = links.items.find(
    (item) =>
      item.target.kind === "refused" &&
      item.rect[2] - item.rect[0] > 2 &&
      item.rect[3] - item.rect[1] > 2,
  );
  if (!refused) {
    skip(names[3] as string, "no link is refused");
  } else {
    const saidBefore = problems.length;
    await pressCentre(refused);
    const stayed = viewer.position.page === refused.page;
    const said = problems.length > saidBefore;
    check(
      names[3] as string,
      stayed && said,
      `on page ${viewer.position.page + 1} of ${doc.page_count}, ` +
        `${problems.length - saidBefore} message(s): ${JSON.stringify(problems.at(-1) ?? "")}`,
    );
  }

  // The pointer. It is the only thing that tells a reader a run of text can be
  // pressed, so an invisible link with no cursor is a feature nobody finds.
  // Asserted against the cursor *off* a link as well, or a surface that always
  // says "pointer" would pass.
  const any = links.items.find(
    (item) => item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
  );
  if (!any) {
    skip(names[4] as string, "no link has a rectangle on the page");
  } else {
    viewer.goToPage(any.page);
    await settle(() => viewer.idle);
    const at = viewer.screenPoint(
      any.page,
      (any.rect[0] + any.rect[2]) / 2,
      (any.rect[1] + any.rect[3]) / 2,
    );
    pointer(root, "pointermove", at.x, at.y);
    await frame();
    const over = viewer.cursorName;
    // The far corner, which no rectangle in the corpus reaches.
    pointer(root, "pointermove", WIDTH - 20, HEIGHT - 20);
    await frame();
    const off = viewer.cursorName;
    check(
      names[4] as string,
      over === "pointer" && off !== "pointer",
      `over a link ${JSON.stringify(over)}, off it ${JSON.stringify(off)}`,
    );
  }

  // --- the keyboard, which is the only way to reach a link without a pointer.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);

  const before = viewer.linkFocus;
  const stepped = viewer.stepLink(1);
  await settle(() => viewer.idle);
  check(
    names[5] as string,
    stepped && before === -1 && viewer.linkFocus !== -1 && viewer.linkRingShown,
    `focus ${before} -> ${viewer.linkFocus}, ring ${viewer.linkRingShown ? "drawn" : "absent"}`,
  );

  // Enter, on whichever link the keyboard reached. Asserted against the link's
  // own destination rather than "the page changed": a link that points at its
  // own page is legal, and a check reading only the page would call that a
  // failure.
  const onIt = links.items.find((item) => item.id === viewer.linkFocus);
  if (!onIt || onIt.target.kind !== "page") {
    skip(
      names[6] as string,
      onIt ? "the first link is not a page destination" : "the keyboard reached no link",
    );
  } else if (unreachable(onIt.target.page) !== null) {
    skip(names[6] as string, unreachable(onIt.target.page) ?? "");
  } else {
    const wanted = onIt.target.page;
    const key = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    root.dispatchEvent(key);
    await settle(() => viewer.idle);
    check(
      names[6] as string,
      viewer.position.page === wanted,
      `Enter on #${onIt.id} landed on page ${viewer.position.page + 1}, wanted ${wanted + 1}`,
    );
  }

  // Escape. The control is that something was focused first, or "nothing is
  // focused afterwards" says nothing about the key.
  //
  // **Back to the top before stepping**, which is the control's precondition
  // rather than tidiness. Stepping starts from the viewport and reports
  // running out rather than wrapping, both deliberate --- so on a document
  // whose only link sits above where the Enter check left the reader, the step
  // correctly focuses nothing and the control cannot be established. That is
  // exactly `links-cropped.pdf`: one link, pointing at its own page, and the
  // check before this one follows it. The first keyboard check already does
  // this; the two below had inherited whatever position they were handed.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);
  viewer.stepLink(1);
  const focused = viewer.linkFocus;
  root.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  if (focused === -1) {
    skip(names[7] as string, "stepping from the top of the document focused no link");
  } else {
    check(
      names[7] as string,
      viewer.linkFocus === -1 && !viewer.linkRingShown,
      `focus ${focused} -> ${viewer.linkFocus}, ` +
        `ring ${viewer.linkRingShown ? "drawn" : "absent"}`,
    );
  }

  // The end of the walk. Stepped forward past every link, which must report
  // rather than wrap --- and the assertion is that it *said* so, since a viewer
  // that silently did nothing produces the same final position.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);
  const saidBefore = problems.length;
  for (let step = 0; step <= links.items.length; step += 1) viewer.stepLink(1);
  await settle(() => viewer.idle);
  const last = viewer.linkFocus;
  check(
    names[8] as string,
    last !== -1 && problems.length > saidBefore,
    `stopped on #${last} after ${links.items.length + 1} steps, ` +
      `${problems.length - saidBefore} message(s): ${JSON.stringify(problems.at(-1) ?? "")}`,
  );
  viewer.clearLinkFocus();
}

/**
 * The outline, and the sidebar showing it.
 *
 * Most of the corpus has no outline at all, so nearly everything here is
 * conditional --- and each condition is a `skip` with its reason rather than a
 * silently absent row. A check that vanishes on some documents cannot be told
 * apart from one that ran, which this file has already been caught doing once.
 *
 * Two assertions are the load-bearing ones and both carry their own control:
 *
 * - **Activating a row moves the viewer**, asserted against *not having been on
 *   that page already*. The target is chosen to be a page the viewer is not on.
 * - **A destination's y is respected**, asserted by comparing two entries on the
 *   *same* page. That is what can see a y-flip: both land on the right page
 *   either way, and only the distance from its top edge inverts. Comparing an
 *   entry against a different page's would pass under the flip.
 */
async function outlineChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
): Promise<void> {
  let outline: Outline;
  try {
    outline = await invoke<Outline>("document_outline", { doc: doc.id });
  } catch (e) {
    check("reads the document's outline", false, String(e));
    return;
  }

  check(
    "reads the document's outline",
    outline.total === countItems(outline),
    `${outline.total} entries in ${outline.walk_ms.toFixed(2)} ms`,
  );

  sidebar.setOutline(outline);
  const rows = allRows(outline.items);

  if (rows.length === 0) {
    // Listed one by one rather than collapsed into a single "no outline" line:
    // the count of checks has to be the same whichever document is used, or a
    // check that silently stopped existing looks exactly like one that passed.
    const why = "the document has no outline";
    for (const name of [
      "the sidebar draws a row per entry",
      "the rows are a tree, not a list",
      "the whole tree is one tab stop",
      "collapsing a row hides its children",
      "expanding it again brings them back",
      "activating a row goes to its page",
      "a destination's y is measured from the page top",
      "scrolling moves the highlight to the right entry",
      "a refused action is drawn but does nothing",
    ]) {
      skip(name, why);
    }
    return;
  }

  // Top-level rows plus the children of whatever the producer left open.
  const shown = sidebar.visible.length;
  check(
    "the sidebar draws a row per entry",
    shown > 0 && shown <= rows.length,
    `${shown} rows drawn of ${rows.length} entries`,
  );

  const treeitems = document.querySelectorAll(
    '.tpdf-sidebar [role="treeitem"]',
  );
  check(
    "the rows are a tree, not a list",
    treeitems.length === shown &&
      [...treeitems].every((row) => row.hasAttribute("aria-level")),
    `${treeitems.length} treeitems, all levelled=${[...treeitems].every((r) => r.hasAttribute("aria-level"))}`,
  );

  const tabbable = [...treeitems].filter(
    (row) => row instanceof HTMLElement && row.tabIndex === 0,
  );
  check(
    "the whole tree is one tab stop",
    tabbable.length === 1,
    `${tabbable.length} of ${treeitems.length} rows are tabbable`,
  );

  // Collapsing, with the control built in: a parent whose children are shown
  // now and gone afterwards. A parent that was already collapsed would satisfy
  // "the children are absent" without anything having happened.
  const parent = findExpandedParent(sidebar);
  if (!parent) {
    const why = "no row in this outline has visible children";
    skip("collapsing a row hides its children", why);
    skip("expanding it again brings them back", why);
  } else {
    const before = sidebar.visible.length;
    sidebar.elementFor(parent)?.focus();
    key(sidebar.elementFor(parent)!, "ArrowLeft");
    const after = sidebar.visible.length;
    check(
      "collapsing a row hides its children",
      after < before,
      `${before} rows -> ${after} after collapsing "${parent}"`,
    );
    key(sidebar.elementFor(parent)!, "ArrowRight");
    check(
      "expanding it again brings them back",
      sidebar.visible.length === before,
      `${after} rows -> ${sidebar.visible.length}`,
    );
  }

  // Navigation. The target is deliberately a page the viewer is not on.
  viewer.goToStart();
  await settle(() => viewer.idle);
  const here = viewer.position.page;
  const elsewhere = rows.find(
    (row) => isNavigable(row.target) && row.target.page !== here,
  );
  if (!elsewhere || !isNavigable(elsewhere.target)) {
    skip(
      "activating a row goes to its page",
      `every entry points at page ${here + 1}, which is where we are`,
    );
  } else {
    sidebar.reveal(elsewhere.id);
    const element = sidebar.elementFor(elsewhere.id);
    if (!element) {
      check(
        "activating a row goes to its page",
        false,
        `no row for ${elsewhere.id}`,
      );
    } else {
      element.focus();
      key(element, "Enter");
      await frame();
      check(
        "activating a row goes to its page",
        viewer.position.page === elsewhere.target.page,
        `"${preview(elsewhere.title)}" from page ${here + 1} to ` +
          `${viewer.position.page + 1}, wanted ${elsewhere.target.page + 1}`,
      );
    }
  }

  await destinationOffsetCheck(viewer, sidebar, rows);
  await highlightCheck(viewer, sidebar, rows);
  refusalCheck(viewer, sidebar, rows);
}

/** Total entries in the tree, counted independently of the walk's own tally. */
function countItems(outline: Outline): number {
  return allRows(outline.items).length;
}

/** A visible row that currently shows children, or `null`. */
function findExpandedParent(sidebar: Sidebar): string | null {
  const rows = document.querySelectorAll<HTMLElement>(
    '.tpdf-sidebar [role="treeitem"][aria-expanded="true"]',
  );
  for (const row of rows) {
    const id = row.dataset.id;
    if (id && sidebar.elementFor(id)) return id;
  }
  return null;
}

/**
 * Two entries on one page: the lower one must land further down.
 *
 * This is the only check here a y-flip fails. Both entries resolve to the same
 * page under either convention, so page-level assertions are blind to it; what
 * inverts is which of the two is nearer the top.
 */
async function destinationOffsetCheck(
  viewer: Viewer,
  sidebar: Sidebar,
  rows: Row[],
): Promise<void> {
  const name = "a destination's y is measured from the page top";
  const byPage = new Map<number, Row[]>();
  for (const row of rows) {
    if (!isNavigable(row.target)) continue;
    const list = byPage.get(row.target.page) ?? [];
    list.push(row);
    byPage.set(row.target.page, list);
  }

  for (const [, group] of byPage) {
    const tops = group
      .map((row) => (isNavigable(row.target) ? row.target : null))
      .filter((target) => target !== null)
      .sort((a, b) => (a.top_pt ?? 0) - (b.top_pt ?? 0));
    // The highest and lowest on the page, whatever they are. Written first as
    // "one at the very top and one below 50 pt", which skipped on the only
    // fixture that has the pair --- its two entries sit at 240 and 440 pt, so
    // neither is at the top and the check reported itself inapplicable.
    const high = tops[0];
    const low = tops[tops.length - 1];
    if (!high || !low || (low.top_pt ?? 0) - (high.top_pt ?? 0) < 50) continue;

    sidebar.reveal(group[0]!.id);
    viewer.goToDestination(high.page, high.top_pt);
    await frame();
    const upper = viewer.offset;
    viewer.goToDestination(low.page, low.top_pt);
    await frame();
    const lower = viewer.offset;

    // A page at the very end of the document clamps both to `maxOffset`, which
    // would pass this by accident in one direction and fail it in the other.
    if (upper >= viewer.maxOffset - 1) {
      skip(name, "the shared page is the last one and both jumps clamp");
      return;
    }
    check(
      name,
      lower > upper,
      `page ${high.page + 1}: y=${high.top_pt ?? 0} lands at ${upper.toFixed(0)}, ` +
        `y=${low.top_pt} at ${lower.toFixed(0)}`,
    );
    return;
  }

  skip(name, "no page in this outline has two entries at different heights");
}

/** Scrolling to an entry's page must move the highlight onto that entry. */
async function highlightCheck(
  viewer: Viewer,
  sidebar: Sidebar,
  rows: Row[],
): Promise<void> {
  const name = "scrolling moves the highlight to the right entry";
  const targets = rows.filter((row) => isNavigable(row.target));
  const first = targets[0];
  const later = targets.find(
    (row) =>
      isNavigable(row.target) &&
      isNavigable(first!.target) &&
      row.target.page > first!.target.page,
  );
  if (!first || !later || !isNavigable(later.target)) {
    skip(name, "this outline has no two entries on different pages");
    return;
  }

  viewer.goToStart();
  await settle(() => viewer.idle);
  const before = sidebar.currentRow;

  viewer.goToDestination(later.target.page, later.target.top_pt);
  await settle(() => viewer.idle);
  // The control: if the highlight was already on the target row, arriving there
  // proves nothing about whether it follows the scroll.
  if (before === later.id) {
    skip(name, `the highlight was already on "${preview(later.title)}"`);
    return;
  }
  check(
    name,
    sidebar.currentRow === later.id,
    `"${before}" -> "${sidebar.currentRow}", wanted "${later.id}" ` +
      `(${preview(later.title)})`,
  );
}

/** An entry carrying a refused action is drawn, marked, and inert. */
function refusalCheck(viewer: Viewer, sidebar: Sidebar, rows: Row[]): void {
  const name = "a refused action is drawn but does nothing";
  const refused = rows.find((row) => row.target.kind === "refused");
  if (!refused) {
    skip(name, "this outline has no /Launch, /URI or /GoToR entry");
    return;
  }

  sidebar.reveal(refused.id);
  const element = sidebar.elementFor(refused.id);
  const before = viewer.offset;
  element?.focus();
  if (element) key(element, "Enter");
  check(
    name,
    element !== null &&
      element.getAttribute("aria-disabled") === "true" &&
      viewer.offset === before,
    `"${preview(refused.title)}" disabled=${element?.getAttribute("aria-disabled")}, ` +
      `offset ${before.toFixed(0)} -> ${viewer.offset.toFixed(0)}`,
  );
}

/**
 * The page strip, and the rule that keeps it out of the viewer's way.
 *
 * Most of this is ordinary --- rows exist, a row goes to its page --- and one
 * check is the reason the strip is written the way it is: **it withdraws its
 * work the moment the viewer needs the renderer.** A thumbnail is a Pdfium
 * render call, 1.5 s of one on the A0 sheet, on the single thread that also
 * draws the page in front of the reader.
 *
 * That check cannot be written without a control, and the control is not the
 * usual "were we already there". It is *was there anything to withdraw*: on the
 * text corpus a thumbnail takes about a millisecond, so a strip asked to yield
 * has almost certainly finished already, and "nothing is outstanding" would be
 * true of a strip that had never heard of the viewer. So the run first waits to
 * catch a request in flight, and says so and skips when it cannot --- which is
 * the honest answer for a document whose thumbnails are too cheap to collide
 * with anything. `vector-multi.pdf` exists to be the document where it can.
 */
async function thumbnailChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  page: PageSize,
  drags: [number, number][],
): Promise<void> {
  const strip = sidebar.thumbnails;
  if (!strip) {
    check("the sidebar has a tab for pages", false, "no strip was built");
    return;
  }

  const tabs = panelTabs();
  const selected = tabs.filter(
    (t) => t.getAttribute("aria-selected") === "true",
  ).length;
  check(
    "the sidebar has a tab for pages",
    tabs.length === SIDEBAR_TABS && selected === 1,
    `${tabs.length} tabs (${tabs.map((t) => t.textContent).join(", ")}), ${selected} selected`,
  );

  viewer.goToStart();
  await settle(() => viewer.idle);
  sidebar.selectTab("pages");
  check(
    "showing the pages tab hides the outline",
    sidebar.tab === "pages" && !panelShown("outline") && panelShown("pages"),
    `outline shown=${panelShown("outline")}, pages shown=${panelShown("pages")}`,
  );

  await eventually(
    "a thumbnail arrives for the page being read",
    () => strip.rendered.includes(0),
    () => `rendered pages: ${strip.rendered.slice(0, 8).join(", ") || "none"}`,
  );

  // The page on screen already has a tier-1 placeholder, which is the same
  // 150 px bitmap at the same scale. Rendering it again would be a second
  // second-and-a-half on the A0 sheet for a picture we already have.
  //
  // Bounded above as well as below, and the upper bound is the half that found
  // a defect: a borrow completes in a microtask, so without a guard the same
  // page is borrowed again on every scroll, resize and position change that
  // lands in between. It read as twelve borrows on a twelve-page document with
  // seven rows on screen --- true, useless, and invisible to "more than zero".
  check(
    "the page already rendered is not rendered twice",
    strip.borrowCount > 0 && strip.borrowCount <= strip.rendered.length,
    `${strip.borrowCount} thumbnails borrowed from the viewer's tier 1, ` +
      `${strip.rendered.length} drawn`,
  );

  const mounted = strip.mounted;
  const windowName = "the strip builds only the rows on screen";
  // The most rows a panel this tall could want, from the row height and the
  // overscan --- not from what the strip actually built. Two earlier versions
  // of this were wrong in opposite directions, and the second is the one worth
  // remembering: written as `mounted.length < pageCount`, with the skip taken
  // when it is not, a mutation that builds *every* row does not fail the check,
  // it makes the check report itself inapplicable. An assertion whose
  // precondition the defect can switch off is not an assertion.
  const maxRows = Math.ceil(HEIGHT / rowHeightFor(page)) + 1 + 2 * OVERSCAN;
  if (doc.page_count <= maxRows) {
    skip(windowName, `all ${doc.page_count} rows fit in a ${HEIGHT} px panel`);
  } else {
    check(
      windowName,
      mounted.length > 0 && mounted.length <= maxRows,
      `${mounted.length} rows built of ${doc.page_count} pages, at most ` +
        `${maxRows} can be on screen`,
    );
  }

  // Windowing is invisible to a screen reader unless each row says where it
  // sits in the whole document; without this it is told the document has as
  // many pages as happen to be mounted.
  const rows = mounted
    .map((page) => strip.elementFor(page))
    .filter((element) => element !== null);
  check(
    "a row says where it is in the whole document",
    rows.length === mounted.length &&
      rows.every(
        (element, at) =>
          element.getAttribute("aria-setsize") === String(doc.page_count) &&
          element.getAttribute("aria-posinset") === String(mounted[at]! + 1),
      ),
    `${rows.length} rows, setsize=${rows[0]?.getAttribute("aria-setsize")} ` +
      `of ${doc.page_count}, first posinset=${rows[0]?.getAttribute("aria-posinset")}`,
  );

  await navigateFromStrip(viewer, strip, doc);
  await dragFromStrip(strip, doc, drags);
  await yieldChecks(root, viewer, sidebar, doc.page_count, strip);
}

/**
 * Dragging a thumbnail, in a real webview.
 *
 * Deliberately the narrowest thing worth putting here. The slot arithmetic is
 * two pure functions with unit tests; the gesture's state machine has nine
 * mutations behind it against a fake DOM; the edit a drop runs is covered by
 * `moveCommandChecks` and by `edits.test.ts`. None of those can answer the one
 * question a window can: does a press on a row in WKWebView capture the
 * pointer, keep receiving moves after the pointer has left the row, and land on
 * a gap computed from geometry the browser actually laid out?
 *
 * So the strip's handler here only *records*, and the document is never
 * touched --- which also means this phase leaves nothing for the later ones to
 * trip over.
 *
 * Two names, and the second is the control. "A drag moves the page" is
 * satisfied by a strip that reorders on every click, and that is the defect a
 * reader meets first: it would rearrange their document every time they looked
 * at a page.
 */
async function dragFromStrip(
  strip: Thumbnails,
  doc: DocumentInfo,
  drags: [number, number][],
): Promise<void> {
  const moved = "dragging a thumbnail asks for the slot it was dropped on";
  const still = "a press that does not travel asks for nothing";

  // A row past the first two, so that dragging it to the top is a move rather
  // than a no-op, and so that the landing is unambiguous.
  if (doc.page_count < 3 || strip.mounted.find((slot: number) => slot >= 2) === undefined) {
    const why =
      doc.page_count < 3
        ? `the document has ${doc.page_count} page(s)`
        : `rows ${strip.mounted.join(",")} are built; none past slot 1`;
    skip(moved, why);
    skip(still, why);
    return;
  }

  /**
   * The row to drag, the row to drop it above, and where to press each.
   *
   * **Measured immediately before each gesture, never carried across one.**
   * Pressing a row navigates, and navigating scrolls the strip to the page it
   * moved to --- 400 px on `outline-simple`, two whole rows. A first version
   * read these once at the top and reused them after the control press, so the
   * real drag pressed 500 px below where its row had moved to and released
   * against a row element the windowing had already replaced. It reported a
   * drop on a gap two off, which reads as broken arithmetic in the strip; the
   * arithmetic was right and the check was aiming at a layout that no longer
   * existed. Re-reading costs nothing and the staleness is unbounded.
   */
  const aim = (): {
    row: HTMLElement;
    top: HTMLElement;
    x: number;
    from: number;
    to: number;
    landing: number;
  } | null => {
    const source = strip.mounted.find((slot: number) => slot >= 2);
    const landing = strip.mounted[0] ?? 0;
    const row = source === undefined ? null : strip.elementFor(source);
    const top = strip.elementFor(landing);
    if (!row || !top || source === undefined) return null;
    const box = row.getBoundingClientRect();
    return {
      row,
      top,
      x: box.left + box.width / 2,
      from: box.top + box.height / 2,
      to: top.getBoundingClientRect().top + 1,
      landing,
    };
  };

  const first = aim();
  if (!first) {
    const why = `rows ${strip.mounted.join(",")} are built; none usable`;
    skip(moved, why);
    skip(still, why);
    return;
  }

  // A click: pressed, moved by less than the threshold, released. Dispatched on
  // the row, because that is where the strip listens for the press; the moves
  // and the release bubble to the panel, which is where it listens for those.
  drags.length = 0;
  pointer(first.row, "pointerdown", first.x, first.from);
  pointer(first.row, "pointermove", first.x, first.from + 2);
  // **Before the release, and the first version of this check was after it.**
  // Both of that version's clauses were incapable of failing, which the
  // mutation that removes the threshold proved by surviving. `dragging` is
  // false once the pointer is up whatever happened in between, so reading it
  // there asks whether a drag is *still* running rather than whether one ever
  // started. And the other clause can never fire either: the press is at the
  // row's centre, so the gap either side of it is the page's own slot, and
  // `landingSlot` calls both of those a no-op by design --- so a press that
  // wrongly became a drag asks for no reorder anyway. The one position that
  // reads as the natural place to press is the one position where the defect
  // has no effect.
  const started = strip.dragging;
  pointer(first.row, "pointerup", first.x, first.from + 2);
  await frame();
  check(
    still,
    !started && drags.length === 0,
    `dragging=${started} after a 2 px press, ${drags.length} reorder(s) asked for`,
  );

  // The strip has very likely moved by now --- that press navigated. Everything
  // below is measured against where things are, not where they were.
  const second = aim();
  if (!second) {
    skip(moved, `rows ${strip.mounted.join(",")} are built; none usable`);
    return;
  }

  const source = strip.mounted.find((slot: number) => slot >= 2) ?? 2;
  drags.length = 0;
  pointer(second.row, "pointerdown", second.x, second.from);
  for (let step = 1; step <= 4; step++) {
    pointer(
      second.row,
      "pointermove",
      second.x,
      second.from + ((second.to - second.from) * step) / 4,
    );
  }
  const recognised = strip.dragging;
  pointer(second.row, "pointerup", second.x, second.to);
  await frame();
  check(
    moved,
    recognised &&
      drags.length === 1 &&
      drags[0]?.[0] === source &&
      drags[0]?.[1] === second.landing,
    `dragged slot ${source} to slot ${second.landing}: recognised=${recognised}, ` +
      `asked ${drags.map((d) => `${d[0]}->${d[1]}`).join(",") || "nothing"}; ` +
      `released over gap ${strip.releasedOver}, rows ${strip.rowPitch.toFixed(0)}px ` +
      `at y ${second.from.toFixed(0)} -> ${second.to.toFixed(0)}, ` +
      `panel ${strip.panelAt.scrollTop.toFixed(0)}@${strip.panelAt.top.toFixed(0)}, ` +
      `mounted ${strip.mounted.join(",")}`,
  );
}


/**
 * Rotating the view.
 *
 * The renderer's half of this is checked against pixels by `text-probe --mode
 * align --view-turns N`, per rotation, with a control for each wrong turn. What
 * is left for here is what only the assembled application can answer: that the
 * layout turns with the render, that the reader keeps their page, that the text
 * layer turns with both, and that the strip follows.
 *
 * The text assertion is the one worth reading. "The dragged text is part of the
 * page's text" holds by construction --- a selection is a contiguous range of
 * character indices and its string is built from those indices --- so it cannot
 * see a rotation applied backwards, or not at all. What discriminates is tying a
 * *screen position* to *specific content*: two named lines are dragged out
 * before the rotation and the same two after it, from wherever the rotation
 * should have put them, and the text has to come back identical.
 *
 * Stated that way rather than as "text nearer the start of the page has a lower
 * character index", which was the first version and is a claim about PDFium
 * rather than about us --- on `rotated-90.pdf` its extraction order is not the
 * page's line order, and that check went red against a rotation that was
 * correct. This one holds whatever order the characters arrive in.
 */
async function rotationChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  page: { width_pt: number; height_pt: number },
  seen: { status: ViewerStatus | null },
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const uprightZoom = viewer.currentZoom;
  const uprightHeight = viewer.maxOffset;
  const startedOn = viewer.position.page;
  const uprightBox = viewer.pageBoxCss;

  // Sampled before the rotation, so the comparison afterwards is against this
  // document rather than against an expectation written for one.
  const before = await sampleLines(root, viewer);

  key(root, "r", true);
  await frame();

  check(
    "Cmd-R rotates the view a quarter turn clockwise",
    viewer.rotation === 1,
    `turns=${viewer.rotation}`,
  );

  // The layout turned, not merely the pixels. At fit width the *displayed*
  // width fills the window, so the zoom has to move by exactly the page's aspect
  // ratio --- a check a rotation that only reached the renderer would fail, and
  // one that reached neither would fail differently.
  const wanted = (uprightZoom * page.width_pt) / page.height_pt;
  check(
    "the page is laid out sideways",
    Math.abs(viewer.currentZoom - wanted) / wanted < 0.02,
    `zoom ${uprightZoom.toFixed(4)} -> ${viewer.currentZoom.toFixed(4)}, ` +
      `wanted ${wanted.toFixed(4)} for a ${page.width_pt.toFixed(0)}x${page.height_pt.toFixed(0)} page`,
  );

  // And the scroller's own geometry, which the zoom above does not cover: the
  // viewer and the scroller each keep a rotation and either can be wrong alone.
  // Written with only the zoom check, a scroller that laid every page out
  // upright survived the mutation entirely -- the page came out narrow inside a
  // correctly-refitted window, and nothing said so.
  const turnedBox = viewer.pageBoxCss;
  const uprightAspect = uprightBox.width / uprightBox.height;
  const turnedAspect = turnedBox.width / turnedBox.height;
  check(
    "the page on screen has the turned page's shape",
    Math.abs(turnedAspect * uprightAspect - 1) < 0.02,
    `${uprightBox.width.toFixed(0)}x${uprightBox.height.toFixed(0)} -> ` +
      `${turnedBox.width.toFixed(0)}x${turnedBox.height.toFixed(0)}, ` +
      `aspect ${uprightAspect.toFixed(3)} then ${turnedAspect.toFixed(3)}`,
  );

  // The control for the coverage check below: a rotation that kept its tiles
  // would recover instantly, and "recovers" would pass having waited for
  // nothing. Tier 1 goes too, so this is stricter than the zoom equivalent.
  check(
    "a rotation discards both tiers",
    (seen.status?.any ?? 1) < 0.999,
    `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );
  await eventually(
    "recovers coverage after a rotation",
    () => (seen.status?.sharp ?? 0) >= 0.999,
    () => `sharp=${((seen.status?.sharp ?? 0) * 100).toFixed(1)}%`,
  );

  check(
    "the reader keeps their page across a rotation",
    viewer.position.page === startedOn,
    `page ${startedOn + 1} -> ${viewer.position.page + 1}`,
  );

  await rotatedTextLayerCheck(viewer, doc);
  await rotatedDragCheck(root, viewer, before);
  rotatedStripCheck(sidebar, page);
  await rotatedTileCheck(doc, page);

  // Four quarter turns are the identity, and the document's length coming back
  // is what says the geometry went round rather than accumulating.
  for (let step = 0; step < 3; step++) {
    key(root, "r", true);
    await frame();
  }
  check(
    "four quarter turns come back to where they started",
    viewer.rotation === 0 &&
      Math.abs(viewer.currentZoom - uprightZoom) < 1e-6 &&
      Math.abs(viewer.maxOffset - uprightHeight) < 1,
    `turns=${viewer.rotation}, zoom=${viewer.currentZoom.toFixed(4)}, ` +
      `length ${uprightHeight.toFixed(0)} -> ${viewer.maxOffset.toFixed(0)}`,
  );

  key(root, "l", true);
  await frame();
  check(
    "Cmd-L rotates the other way",
    viewer.rotation === 3,
    `turns=${viewer.rotation}`,
  );
  key(root, "r", true);
  await frame();
  await settle(() => viewer.idle);
}

/** Two lines of a page, dragged out at named fractions through its text. */
interface LineSample {
  /** Fractions through the lines, in reading order: the first and the second. */
  early: string;
  late: string;
  page: number;
}

/**
 * Drags two lines out of the page on screen, in reading order.
 *
 * "In reading order" is the whole trick, and it is what makes the same call
 * usable before and after a rotation: the two positions are named as fractions
 * through the *lines*, and turning those into screen points needs the total
 * rotation --- which decides both which screen axis separates lines and which
 * end of it the first line is at.
 *
 * Two tables, and they are different pairs. Across the lines, the first line is
 * at the low end of the axis at no rotation and at three quarter turns, because
 * a quarter turn sends the page's y to the display's x *decreasing* while three
 * send it to x increasing. Along a line, reading runs in the increasing
 * direction at no rotation and at one quarter turn. Getting either wrong makes
 * the samples disagree, which is what the comparison then reports.
 */
async function sampleLines(
  root: HTMLElement,
  viewer: Viewer,
): Promise<LineSample | null> {
  const at = viewer.position.page;
  const shown = viewer.textOn(at);
  const bounds = shown && inkBounds(shown);
  if (!shown || !bounds) return null;

  // Derived from the text's own extent rather than from two fixed screen rows.
  // Written the other way it read the fixture's margins: `rotated-90.pdf` puts
  // its twelve lines in the top third of the page, so at a half turn the upper
  // drag landed in two-thirds of blank and selected three characters from an
  // edge, which is not a sample of anything.
  const total = shown.quarter_turns % 4;
  const sideways = total % 2 === 1;
  const across = sideways
    ? [bounds.left, bounds.right]
    : [bounds.top, bounds.bottom];
  const along = sideways
    ? [bounds.top, bounds.bottom]
    : [bounds.left, bounds.right];
  const lerp = (range: number[], t: number): number =>
    range[0]! + (range[1]! - range[0]!) * t;

  const acrossForwards = total === 0 || total === 3;
  const alongForwards = total === 0 || total === 1;
  // From the start of a line to its middle. Starting mid-line is not enough on
  // `rotated-90.pdf`, whose six words per line come from a rotating list, so a
  // run out of the middle recurs on several lines and names none of them.
  const lineStart = alongForwards ? 0.02 : 0.98;

  const sample = (through: number): string => {
    // A fifth in from each end, so neither lands on the first or last line,
    // where a drag that overshoots has nowhere to go and clamps.
    const line = lerp(across, acrossForwards ? through : 1 - through);
    const from = sideways
      ? viewer.screenPoint(at, line, lerp(along, lineStart))
      : viewer.screenPoint(at, lerp(along, lineStart), line);
    const to = sideways
      ? viewer.screenPoint(at, line, lerp(along, 0.5))
      : viewer.screenPoint(at, lerp(along, 0.5), line);
    drag(root, [from.x, from.y], [to.x, to.y]);
    return viewer.selectedText;
  };

  const early = sample(0.2);
  const late = sample(0.8);
  key(root, "Escape");
  return { early, late, page: at };
}

/**
 * The same two lines come back out of a rotated page.
 *
 * Not "the text nearer the start of the page has the lower character index",
 * which was the first version of this: PDFium's extraction order is not the
 * page's line order on `rotated-90.pdf`, so that check went red against a
 * rotation that was correct, and would have been quietly accepted as a defect.
 * Comparing a before with an after assumes nothing about the order --- and a
 * rotation applied backwards returns the *other* line, so it still fails.
 */
async function rotatedDragCheck(
  root: HTMLElement,
  viewer: Viewer,
  before: LineSample | null,
): Promise<void> {
  const name = "the same lines come back out of a rotated page";

  // The bound `sameLine` actually needs, derived from it rather than guessed
  // beside it: a sample shorter than this has no comparable core, and the check
  // below would call that a failed rotation.
  const enough = CORE_CHARS + 4;
  if (!before || before.early.length < enough || before.late.length < enough) {
    skip(
      name,
      `the upright page yielded no two lines of ${enough} characters to drag out of it ` +
        `(got ${before?.early.length ?? 0} and ${before?.late.length ?? 0})`,
    );
    return;
  }
  if (before.early === before.late) {
    // A one-line page, or a drag that clamped to the same line twice. Either
    // way the comparison could not tell a rotation from a mirror.
    skip(
      name,
      "both samples came from the same line, which cannot distinguish a turn",
    );
    return;
  }

  const after = await sampleLines(root, viewer);
  if (!after || after.page !== before.page) {
    skip(
      name,
      `the rotation left page ${(after?.page ?? -1) + 1}, not ${before.page + 1}`,
    );
    return;
  }

  const held =
    sameLine(after.early, before.early) && sameLine(after.late, before.late);
  const swapped =
    sameLine(after.early, before.late) && sameLine(after.late, before.early);
  check(
    name,
    held,
    `"${preview(before.early)}" then "${preview(before.late)}" upright; ` +
      `"${preview(after.early)}" then "${preview(after.late)}" turned` +
      (held
        ? ""
        : swapped
          ? " -- the two swapped, i.e. the turn went the wrong way"
          : ""),
  );
}

/**
 * The text layer knows the view turned.
 *
 * Direct, and it has to be. "The same lines come back" derives its drag
 * positions from the viewer's own turned boxes and then asks the caret about
 * them, so a text layer that was never told about the rotation is wrong
 * *consistently* --- the sample and the caret agree, the same lines come back,
 * and the check passes against a selection that is ninety degrees out from the
 * page on screen. It survived exactly that mutation.
 *
 * This asserts the wiring rather than the geometry: what the page reports as its
 * own rotation, and its dimensions, must have moved.
 */
async function rotatedTextLayerCheck(
  viewer: Viewer,
  doc: DocumentInfo,
): Promise<void> {
  const name = "the text layer turns with the view";
  const at = viewer.position.page;
  const shown = viewer.textOn(at);
  if (!shown || shown.codes.length === 0) {
    skip(name, "the page has no extractable text");
    return;
  }

  // From the backend, so the comparison is against the document rather than
  // against the same cache being checked.
  const raw = await invoke<{ quarter_turns: number; width_pt: number }>(
    "page_text",
    {
      doc: doc.id,
      page: at,
    },
  ).catch(() => null);
  if (!raw) {
    skip(name, "the page's text could not be fetched a second time");
    return;
  }

  const wanted = (raw.quarter_turns + viewer.rotation) % 4;
  const swapped = viewer.rotation % 2 === 1;
  check(
    name,
    shown.quarter_turns === wanted &&
      (swapped
        ? shown.width_pt !== raw.width_pt
        : shown.width_pt === raw.width_pt),
    `page is /Rotate ${raw.quarter_turns * 90}, view turned ${viewer.rotation * 90}: ` +
      `the text layer reports /Rotate ${shown.quarter_turns * 90} ` +
      `(wanted ${wanted * 90}) on a ${shown.width_pt.toFixed(0)} pt wide page ` +
      `(the document says ${raw.width_pt.toFixed(0)})`,
  );
}

/**
 * Turning one page of the document, which is the edit a save writes.
 *
 * The whole difficulty is telling this apart from a view rotation, and the
 * assertions are chosen for exactly that. A defect that turned the *view*
 * instead would satisfy every statement about the page that was turned --- it
 * would be the right shape, its tiles would be gone, its text would run
 * sideways --- so the checks that carry the weight are the two negative ones: a
 * page nobody touched must keep its shape, and `viewer.rotation` must not have
 * moved. Written with only the positive half, a `setPageTurns` that called
 * `rotateBy` would pass.
 *
 * Runs after {@link rotationChecks}, which leaves the view upright, and puts the
 * page back before returning --- every phase after this one inherits whatever
 * state it is left in, and the file's own history has eight assertions going red
 * across three phases for exactly that reason.
 */
async function pageRotationChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "a page turn is recorded against that page",
    "the turned page has the turned shape",
    "a page nobody turned keeps its shape",
    "turning a page does not rotate the view",
    "a page turn discards that page's pixels",
    "recovers coverage after a page turn",
    "the text layer turns with the page",
    "a page nobody turned keeps its text upright",
    "turning a page back restores the layout",
    "a half turn discards the pixels its box did not move",
  ];

  if (doc.page_count < 2) {
    for (const name of names) skip(name, "the document has one page");
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const target = 0;
  const other = 1;
  const uprightTarget = viewer.pageBoxCssOf(target);
  const uprightOther = viewer.pageBoxCssOf(other);
  const uprightRotation = viewer.rotation;

  // A square page cannot report a quarter turn through its shape, and neither
  // can a page whose box is within the tolerance of square. Skipping is the
  // honest answer: the alternative is three assertions that hold whatever the
  // code does.
  const aspect = uprightTarget.width / uprightTarget.height;
  if (Math.abs(aspect - 1) < 0.05) {
    for (const name of names) {
      skip(name, `page 1 is square (${aspect.toFixed(3)}), so a turn is invisible`);
    }
    return;
  }

  // Read before the turn, so the text assertions compare against this document.
  const otherTextBefore = viewer.textOn(other);

  viewer.setPageTurns(target, 1);
  await frame();

  check(
    names[0] ?? "",
    viewer.pageExtraTurns(target) === 1 && viewer.pageExtraTurns(other) === 0,
    `page 1 turned ${viewer.pageExtraTurns(target) * 90}, ` +
      `page 2 turned ${viewer.pageExtraTurns(other) * 90}`,
  );

  const turnedTarget = viewer.pageBoxCssOf(target);
  const turnedAspect = turnedTarget.width / turnedTarget.height;
  check(
    names[1] ?? "",
    Math.abs(turnedAspect * aspect - 1) < 0.02,
    `${uprightTarget.width.toFixed(0)}x${uprightTarget.height.toFixed(0)} -> ` +
      `${turnedTarget.width.toFixed(0)}x${turnedTarget.height.toFixed(0)}, ` +
      `aspect ${aspect.toFixed(3)} then ${turnedAspect.toFixed(3)}`,
  );

  // The assertion that separates a page turn from a view rotation. A viewer that
  // turned everything would pass every check above this line.
  //
  // **Proportions, not pixels**, and the difference is a whole corpus. Fit-width
  // sizes the layout to the widest page, so turning page 1 to landscape makes it
  // the widest and every other page is legitimately rescaled: on `text-heavy` the
  // neighbour went 640x828 to 495x640 --- 22% smaller, and the *same* shape to
  // three decimals. Comparing absolute boxes called that a defect. Comparing the
  // ratio still catches what this check exists for, because a page that was
  // turned reports the reciprocal, which no rescale can produce.
  //
  // Found by the first sweep across all fourteen corpora, having been written and
  // watched pass on one. That is what the sweep is for.
  const turnedOther = viewer.pageBoxCssOf(other);
  const otherAspect = uprightOther.width / uprightOther.height;
  const turnedOtherAspect = turnedOther.width / turnedOther.height;
  const rescaled = Math.abs(turnedOther.width - uprightOther.width) >= 1;
  check(
    names[2] ?? "",
    Math.abs(turnedOtherAspect / otherAspect - 1) < 0.02,
    `page 2 was ${uprightOther.width.toFixed(0)}x${uprightOther.height.toFixed(0)}, ` +
      `now ${turnedOther.width.toFixed(0)}x${turnedOther.height.toFixed(0)} ` +
      `-- aspect ${otherAspect.toFixed(3)} then ${turnedOtherAspect.toFixed(3)}` +
      (rescaled ? ", rescaled by the wider page" : ", unmoved"),
  );

  check(
    names[3] ?? "",
    viewer.rotation === uprightRotation,
    `view turns ${uprightRotation} -> ${viewer.rotation}`,
  );

  // The control for the recovery check below: a turn that kept its tiles would
  // recover instantly, and "recovers" would have waited for nothing.
  check(
    names[4] ?? "",
    (seen.status?.any ?? 1) < 0.999,
    `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );
  await eventually(
    names[5] ?? "",
    () => (seen.status?.sharp ?? 0) >= 0.999,
    () => `sharp=${((seen.status?.sharp ?? 0) * 100).toFixed(1)}%`,
  );

  await pageTurnTextChecks(viewer, doc, target, other, otherTextBefore, names);

  // Back to where the phase found it, and asserted rather than assumed --- a
  // restore that silently did nothing would leave every later phase reading a
  // document with a sideways first page.
  viewer.setPageTurns(target, 0);
  await frame();
  const restored = viewer.pageBoxCssOf(target);
  check(
    names[8] ?? "",
    viewer.pageExtraTurns(target) === 0 &&
      Math.abs(restored.width - uprightTarget.width) < 1 &&
      Math.abs(restored.height - uprightTarget.height) < 1,
    `${turnedTarget.width.toFixed(0)}x${turnedTarget.height.toFixed(0)} -> ` +
      `${restored.width.toFixed(0)}x${restored.height.toFixed(0)}, ` +
      `wanted ${uprightTarget.width.toFixed(0)}x${uprightTarget.height.toFixed(0)}`,
  );
  await settle(() => viewer.idle);

  // The half turn, and it is the only thing that tests the ordering inside
  // `Scroller.setPageTurns`. That method discards the page's tiles *before* it
  // touches the geometry, because `applySizes` invalidates a page only when its
  // box dimensions change --- and 180 degrees swaps width and height twice, so
  // the box is identical and `applySizes` sees nothing.
  //
  // A quarter turn cannot show this: the box does change, so the geometry pass
  // invalidates the page whether the explicit call is there or not. Found by a
  // mutation that deleted the call and survived every check above, which is the
  // harness reporting a hole in the corpus rather than a hole in the code. The
  // bug it would have let through is plain to a reader: turn a page twice, and
  // the tiles on screen are still the ones from before the turn.
  // Full coverage FIRST, or the assertion below cannot fail. `settle(idle)` is
  // not the same thing: the viewer goes idle while tiles are still arriving, so
  // `any` was already under the threshold when the half turn happened and the
  // check passed whether the invalidation ran or not. The mutation survived a
  // whole run saying exactly that.
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const beforeHalf = viewer.pageBoxCssOf(target);
  viewer.setPageTurns(target, 2);
  await frame();
  const afterHalf = viewer.pageBoxCssOf(target);
  const boxHeld =
    Math.abs(afterHalf.width - beforeHalf.width) < 1 &&
    Math.abs(afterHalf.height - beforeHalf.height) < 1;
  check(
    names[9] ?? "",
    boxHeld && (seen.status?.any ?? 1) < 0.999,
    `box ${boxHeld ? "unmoved" : "MOVED"} at ` +
      `${afterHalf.width.toFixed(0)}x${afterHalf.height.toFixed(0)}, ` +
      `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );

  // Back to upright again, for the phase after this one.
  viewer.setPageTurns(target, 0);
  await settle(() => viewer.idle);
}

/**
 * Removing a page from the document, which is the edit that moves every other.
 *
 * **What makes this hard to check is the same thing that makes it worth
 * checking**: a page count that went down by one is equally true of a viewer
 * that dropped the *wrong* page, or the last one, or that renumbered without
 * moving anything. So the assertion that carries the weight is the identity one
 * --- the slot below the gap must now be showing the page that was under it ---
 * and the only thing on this side that can tell one page from another is its
 * text. Where a document has none, the check says so and skips rather than
 * asserting a count that cannot fail.
 *
 * The order is driven directly rather than through the model. That is
 * deliberate and is the same seam {@link pageRotationChecks} uses: the journal,
 * the refusals and the undo replay are in Rust and have 437 tests there, and
 * what no unit test can reach is a real layout with real tiles rearranging
 * itself. The wire between the two --- `App.svelte` asking the backend and
 * handing the answer here --- is covered by neither, and `docs/PLAN.md` says so.
 *
 * Puts the document back before returning, asserted rather than assumed, because
 * every phase after this one inherits what it leaves.
 */
/**
 * A page's text as a short comparable string, or `""` if it has none.
 *
 * What makes a page identifiable to a check that can only see the layout. A
 * count cannot tell "page 2 was removed" from "page 3 was removed and everything
 * renumbered", and neither can it tell a page that moved from a page that stayed
 * --- both phases below rest on this, and both skip rather than pass when two
 * pages come back with the same string.
 */
function fingerprint(
  text: { codes: number[]; width_pt: number } | null,
): string {
  return !text || text.codes.length === 0
    ? ""
    : `${text.codes.length}@${text.width_pt.toFixed(1)}:${text.codes
        .slice(0, 32)
        .join(",")}`;
}

async function pageDeletionChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "deleting a page leaves the document one page shorter",
    "the page below the deleted one moves up into its slot",
    "the page above the deleted one does not move",
    "the last slot is gone rather than left empty",
    "deleting a page does not rotate the view",
    "deleting a page discards what was painted",
    "recovers coverage after a page is deleted",
    "putting the page back restores the document",
    "the page that came back is the page that went",
    "the reader stays on the page they were reading",
    "the backend takes the page out of the working document",
    "the backend refuses a page it has already deleted",
    "undo puts the page back in the backend's answer",
  ];

  if (doc.page_count < 3) {
    // Only the viewer's half needs a middle page to delete. The three that ask
    // the backend need two pages and are left to say so for themselves --- on
    // `tagged.pdf` they are the only checks here that can run at all, and
    // skipping them with everything else would be a skip for a reason that is
    // not theirs.
    for (const name of names.slice(0, 10)) {
      skip(name, `the document has ${doc.page_count} page(s), and this deletes a middle one`);
    }
    await deleteCommandChecks(doc, names);
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const target = 1;
  // Below the page that is about to go, so that "the reader follows their page"
  // has a direction. Where they end up is read back rather than assumed --- on a
  // short document the last page cannot reach the top of the viewport, which is
  // a trap this file has paid for once.
  viewer.goToPage(target + 1);
  await settle(() => viewer.idle);
  const readingAt = viewer.position.page;

  const before = viewer.pageOrder;
  const uprightRotation = viewer.rotation;
  const above = viewer.pageBoxCssOf(0);
  // Read while the pages are still where they were, so what the check compares
  // against is this document rather than the state it is asserting.
  const belowText = viewer.textOn(target + 1);
  const aboveText = viewer.textOn(0);
  const targetText = viewer.textOn(target);
  const lastSlot = before.length - 1;

  // Whether the two pages this phase swaps around can be told apart at all. On a
  // corpus whose pages carry the same text, "the page below moved up" and "the
  // page below did not move" produce the same reading --- a fixture where the
  // right rule and the wrong rule agree, which `AGENTS.md` says to skip rather
  // than to report as a pass.
  const targetPrint = fingerprint(targetText);
  const belowPrint = fingerprint(belowText);
  const distinct =
    targetPrint !== "" && belowPrint !== "" && targetPrint !== belowPrint;

  const without = before.filter((_unused, slot) => slot !== target);
  viewer.setPages(without);
  await frame();

  check(
    names[0] ?? "",
    (seen.status?.pageCount ?? -1) === before.length - 1,
    `${before.length} pages, now ${seen.status?.pageCount ?? -1}`,
  );

  const movedUp = fingerprint(viewer.textOn(target));
  if (!distinct) {
    skip(
      names[1] ?? "",
      belowPrint === "" || targetPrint === ""
        ? "the pages have no extractable text to tell them apart"
        : "pages 2 and 3 read alike, so moving one up is invisible here",
    );
  } else {
    // Identity by content. A count cannot tell "page 2 was removed" from "page 3
    // was removed and the pages renumbered", and those are different documents.
    check(
      names[1] ?? "",
      movedUp === belowPrint,
      `slot ${target + 1} held [${belowPrint.slice(0, 40)}], slot ${target} now ` +
        `holds [${movedUp.slice(0, 40)}]`,
    );
  }

  const aboveNow = viewer.pageBoxCssOf(0);
  const aboveTextNow = viewer.textOn(0);
  const shapeHeld =
    Math.abs(aboveNow.width / aboveNow.height - above.width / above.height) <
    0.02;
  const textHeld =
    !aboveText || !aboveTextNow
      ? true
      : aboveTextNow.codes.length === aboveText.codes.length;
  check(
    names[2] ?? "",
    shapeHeld && textHeld,
    `page 1 was ${above.width.toFixed(0)}x${above.height.toFixed(0)}, now ` +
      `${aboveNow.width.toFixed(0)}x${aboveNow.height.toFixed(0)}` +
      (aboveText && aboveTextNow
        ? `, text ${aboveText.codes.length} -> ${aboveTextNow.codes.length}`
        : ", no text to compare"),
  );

  const past = viewer.pageBoxCssOf(lastSlot);
  check(
    names[3] ?? "",
    past.width === 0 && past.height === 0,
    `slot ${lastSlot} reports ${past.width.toFixed(0)}x${past.height.toFixed(0)}`,
  );

  // The negative one, and the reason it is here: every statement above is also
  // true of a viewer that rebuilt itself from scratch, and a rebuild that lost
  // the reader's rotation is a defect they would meet immediately.
  check(
    names[4] ?? "",
    viewer.rotation === uprightRotation,
    `view turns ${uprightRotation} -> ${viewer.rotation}`,
  );

  // The control for the recovery below: pixels kept across a deletion would make
  // "recovers" a wait for something that had never stopped being true.
  check(
    names[5] ?? "",
    (seen.status?.any ?? 1) < 0.999,
    `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );
  await eventually(
    names[6] ?? "",
    () => (seen.status?.sharp ?? 0) >= 0.999,
    () => `sharp=${((seen.status?.sharp ?? 0) * 100).toFixed(1)}%`,
  );

  // Where the reader is, after the page they were on has moved up one slot.
  // Inside this deletion rather than in one of its own: every `setPages` throws
  // both tiers away, and on the twelve A0 pages of `vector-multi` a tier-1
  // render alone costs about a second and a half --- a phase that deleted and
  // restored twice put the whole sweep past its timeout.
  if (readingAt <= target) {
    skip(
      names[9] ?? "",
      `the reader is on slot ${readingAt}, at or above the page being deleted, ` +
        "so following it moves them nowhere",
    );
  } else {
    check(
      names[9] ?? "",
      viewer.position.page === readingAt - 1,
      `was reading slot ${readingAt}, now on ${viewer.position.page} ` +
        `(wanted ${readingAt - 1})`,
    );
  }

  // Putting the whole order back, which is exactly what an undo produces: the
  // same pages, the same identities, the same slots.
  viewer.setPages(before);
  await frame();
  check(
    names[7] ?? "",
    (seen.status?.pageCount ?? -1) === before.length,
    `${seen.status?.pageCount ?? -1} pages, wanted ${before.length}`,
  );

  const restored = fingerprint(viewer.textOn(target));
  if (!distinct) {
    skip(
      names[8] ?? "",
      belowPrint === "" || targetPrint === ""
        ? "the pages have no extractable text to tell them apart"
        : "pages 2 and 3 read alike, so which one came back is invisible here",
    );
  } else {
    // The page in the target slot must be the one that was there before, not the
    // one that had moved up into it --- the failure a page count cannot see, and
    // the reason the restore is checked at all rather than assumed from the
    // count going back up.
    check(
      names[8] ?? "",
      restored === targetPrint,
      `slot ${target} holds [${restored.slice(0, 40)}], wanted ` +
        `[${targetPrint.slice(0, 40)}] and not [${belowPrint.slice(0, 40)}]`,
    );
  }
  await settle(() => viewer.idle);

  await deleteCommandChecks(doc, names);
}

/**
 * The `page_delete` command itself, against the model in the backend.
 *
 * The rest of the phase drives `Viewer.setPages` directly, which is the seam
 * that lets a check watch a real layout rearrange itself --- and says nothing
 * about the command a reader actually runs. This half asks the backend, so the
 * round trip is covered: the command is registered, it names a page by the
 * identity a state reply gave it, and it answers with a document one page
 * shorter.
 *
 * What neither half covers is `App.svelte`, which carries the answer from one to
 * the other. The harness runs instead of the shell.
 *
 * The model is left as it was found. The undo is asserted rather than assumed,
 * because every phase after this one reads a document that this could otherwise
 * have left a page short.
 */
async function deleteCommandChecks(
  doc: DocumentInfo,
  names: string[],
): Promise<void> {
  interface State {
    pages: { id: number; source: number; turns: number }[];
    can_undo: boolean;
  }

  const state = await invoke<State>("edit_state", { doc: doc.id }).catch(
    () => null,
  );
  const doomed = state?.pages[1];
  if (!state || !doomed) {
    for (const name of names.slice(10)) {
      skip(name, "the backend has no edit model with a page to spare");
    }
    return;
  }

  const after = await invoke<State>("page_delete", {
    doc: doc.id,
    page: doomed.id,
  }).catch((e: unknown) => String(e));
  if (typeof after === "string") {
    check(names[10] ?? "", false, `the command failed: ${preview(after)}`);
    for (const name of names.slice(11)) skip(name, "the deletion did not happen");
    return;
  }

  check(
    names[10] ?? "",
    after.pages.length === state.pages.length - 1 &&
      !after.pages.some((page) => page.id === doomed.id),
    `${state.pages.length} pages, now ${after.pages.length}; the deleted id is ` +
      (after.pages.some((page) => page.id === doomed.id) ? "still there" : "gone"),
  );

  // The tombstone, which is the distinction a frontend one state behind needs:
  // an id that never existed and an id that was deleted are different answers.
  const again = await invoke<State>("page_delete", {
    doc: doc.id,
    page: doomed.id,
  }).then(
    () => "",
    (e: unknown) => String(e),
  );
  check(
    names[11] ?? "",
    again.includes("deleted"),
    again ? preview(again) : "it accepted the second deletion",
  );

  const undone = await invoke<State>("edit_undo", { doc: doc.id }).catch(
    () => null,
  );
  check(
    names[12] ?? "",
    undone?.pages.length === state.pages.length &&
      undone.pages.some((page) => page.id === doomed.id),
    `${after.pages.length} pages, now ${undone?.pages.length ?? -1}, wanted ` +
      `${state.pages.length} with the deleted page among them`,
  );
}

/**
 * Moving a page: the layout rearranges, and nothing is lost on the way.
 *
 * The sibling of {@link pageDeletionChecks}, and it exists separately because a
 * move fails in a way a deletion cannot: **the page count does not move**. Every
 * observable a deletion is caught by --- one page shorter, an empty last slot,
 * coverage dropping --- reads the same for a move that worked and a move that
 * did nothing at all. So what is asserted here is identity, by the text on each
 * page, and the length is asserted to have *stayed*.
 *
 * A later page is moved to the **front** rather than an early one to the back,
 * which is not arbitrary: the reader stands on the page that moves, and only a
 * page that can reach the top of the viewport can be checked for having been
 * followed. The last page of a short document cannot, which is a trap this file
 * has already paid for.
 *
 * Two `setPages` calls and no coverage wait, deliberately. Each one throws both
 * tiers away and on the twelve A0 pages of `vector-multi` a tier-1 render costs
 * about a second and a half; that the pixels are discarded and come back is
 * {@link pageDeletionChecks}'s to assert, and asserting it twice would buy a
 * minute of sweep for nothing.
 *
 * Puts the document back before returning, asserted rather than assumed.
 */
async function pageMoveChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "moving a page leaves the document exactly as long",
    "the moved page is in the slot it was moved to",
    "the page that was there moves down rather than away",
    "the reader follows the page they were reading",
    "the moved page and the one it displaced keep their sizes",
    "putting the order back restores the document",
    "the backend moves the page in the working document",
    "the backend refuses a page moved behind itself",
    "undo puts a moved page back where it started",
  ];

  if (doc.page_count < 3) {
    for (const name of names.slice(0, 6)) {
      skip(
        name,
        `the document has ${doc.page_count} page(s), and this moves the third one`,
      );
    }
    await moveCommandChecks(doc, names);
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle);

  const target = 2;
  viewer.goToPage(target);
  await settle(() => viewer.idle);
  const readingAt = viewer.position.page;

  const before = viewer.pageOrder;
  const movedText = fingerprint(viewer.textOn(target));
  const frontText = fingerprint(viewer.textOn(0));
  const distinct =
    movedText !== "" && frontText !== "" && movedText !== frontText;

  // Both pages have been laid out by now --- the sweep started at Home and the
  // reader has just scrolled to `target` --- so these are measured sizes rather
  // than the estimate a page nobody has seen is given.
  const movedBox = viewer.pageBoxCssOf(target);
  const frontBox = viewer.pageBoxCssOf(0);
  const shape = (box: { width: number; height: number }): number =>
    box.height === 0 ? 0 : box.width / box.height;
  const shapesDiffer = Math.abs(shape(movedBox) - shape(frontBox)) > 0.02;

  const after = [...before];
  const [moved] = after.splice(target, 1);
  if (!moved) return;
  after.unshift(moved);
  viewer.setPages(after);
  await frame();

  check(
    names[0] ?? "",
    (seen.status?.pageCount ?? -1) === before.length,
    `${before.length} pages, now ${seen.status?.pageCount ?? -1}`,
  );

  if (!distinct) {
    const why =
      movedText === "" || frontText === ""
        ? "the pages have no extractable text to tell them apart"
        : `pages 1 and ${target + 1} read alike, so a move between them is invisible here`;
    skip(names[1] ?? "", why);
    skip(names[2] ?? "", why);
  } else {
    const atFront = fingerprint(viewer.textOn(0));
    check(
      names[1] ?? "",
      atFront === movedText,
      `slot 0 holds [${atFront.slice(0, 40)}], wanted [${movedText.slice(0, 40)}]`,
    );
    // The other direction, and it is what separates a move from a copy: the page
    // that was at the front is still in the document, one slot further down.
    const displaced = fingerprint(viewer.textOn(1));
    check(
      names[2] ?? "",
      displaced === frontText,
      `slot 1 holds [${displaced.slice(0, 40)}], wanted [${frontText.slice(0, 40)}]`,
    );
  }

  if (readingAt !== target) {
    skip(
      names[3] ?? "",
      `the reader is on slot ${readingAt} rather than the page being moved`,
    );
  } else {
    // By identity, which is the whole reason `Viewer.setPages` takes an order
    // rather than a count: a reader looking at a page that moved is still
    // looking at that page.
    check(
      names[3] ?? "",
      viewer.position.page === 0,
      `was reading slot ${readingAt}, now on ${viewer.position.page} (wanted 0)`,
    );
  }

  // What the layout carries by *identity* rather than by slot. A scroller that
  // re-indexed its learned sizes by position would give the moved page the size
  // of whatever used to be at the front. Only observable where the two pages are
  // different shapes, and `mixed.pdf` is the one corpus that is.
  //
  // **Both slots are asserted, and one of them would not be enough.** The other
  // way to lose a size is to lose them all --- the carry-forward returning
  // nothing rather than the wrong thing --- and then every page falls back to
  // the same estimate. A comparison against one measured shape cannot see that,
  // because the estimate is free to land within tolerance of it, and on this
  // corpus it does: the mutation that drops the carry-forward entirely left this
  // check green while reddening a deletion check that reads absolute boxes.
  // Comparing *both* closes it by arithmetic rather than by luck. If every page
  // shares one estimate, that estimate would have to be within 0.02 of two
  // shapes that `shapesDiffer` has just established are further apart than that,
  // so at least one half must fail whatever the estimate happens to be.
  //
  // The comparison is of shape rather than of size because fit-width rescales
  // every page when the widest one changes slot --- which is a trap of its own,
  // and here the moved page really does land at twice the width it was measured
  // at while being the same page.
  if (!shapesDiffer) {
    skip(
      names[4] ?? "",
      `pages 1 and ${target + 1} are the same shape, so a size that moved to the ` +
        "wrong page reads exactly like one that travelled",
    );
  } else {
    const landed = viewer.pageBoxCssOf(0);
    const displacedBox = viewer.pageBoxCssOf(1);
    check(
      names[4] ?? "",
      Math.abs(shape(landed) - shape(movedBox)) <= 0.02 &&
        Math.abs(shape(displacedBox) - shape(frontBox)) <= 0.02,
      `the page was ${movedBox.width.toFixed(0)}x${movedBox.height.toFixed(0)} ` +
        `and landed as ${landed.width.toFixed(0)}x${landed.height.toFixed(0)}; ` +
        `the page it displaced was ${frontBox.width.toFixed(0)}x${frontBox.height.toFixed(0)} ` +
        `and is now ${displacedBox.width.toFixed(0)}x${displacedBox.height.toFixed(0)}`,
    );
  }

  viewer.setPages(before);
  await frame();
  const restored = fingerprint(viewer.textOn(target));
  check(
    names[5] ?? "",
    (seen.status?.pageCount ?? -1) === before.length &&
      (!distinct || restored === movedText),
    `${seen.status?.pageCount ?? -1} pages, slot ${target} holds ` +
      (distinct ? `[${restored.slice(0, 40)}]` : "text nothing can tell apart"),
  );
  await settle(() => viewer.idle);

  await moveCommandChecks(doc, names);
}

/**
 * The `page_move` command itself, against the model in the backend.
 *
 * The same seam {@link deleteCommandChecks} covers, for the same reason: the
 * rest of the phase drives `Viewer.setPages` directly and says nothing about the
 * command a reader runs. Here the round trip is the subject --- the command is
 * registered, it takes two identities rather than a destination index, and the
 * order it answers with is the one asked for.
 *
 * The model is left as it was found, asserted rather than assumed.
 */
async function moveCommandChecks(
  doc: DocumentInfo,
  names: string[],
): Promise<void> {
  interface State {
    pages: { id: number; source: number; turns: number }[];
    can_undo: boolean;
  }

  const state = await invoke<State>("edit_state", { doc: doc.id }).catch(
    () => null,
  );
  const first = state?.pages[0];
  const last = state?.pages[state.pages.length - 1];
  if (!state || !first || !last || state.pages.length < 2) {
    for (const name of names.slice(6)) {
      skip(name, "the backend has no edit model with two pages to swap");
    }
    return;
  }

  const wanted = [...state.pages.slice(1), first].map((page) => page.id);
  const after = await invoke<State>("page_move", {
    doc: doc.id,
    page: first.id,
    after: last.id,
  }).catch((e: unknown) => String(e));
  if (typeof after === "string") {
    check(names[6] ?? "", false, `the command failed: ${preview(after)}`);
    for (const name of names.slice(7)) skip(name, "the move did not happen");
    return;
  }

  const got = after.pages.map((page) => page.id);
  check(
    names[6] ?? "",
    got.length === wanted.length && got.every((id, at) => id === wanted[at]),
    `ids ${preview(got.join(","))}, wanted ${preview(wanted.join(","))}`,
  );

  // A page cannot be its own anchor: that names no position, and answering it
  // with "nothing happened" would leave a reader's undo holding a command that
  // did nothing.
  const itself = await invoke<State>("page_move", {
    doc: doc.id,
    page: first.id,
    after: first.id,
  }).then(
    () => "",
    (e: unknown) => String(e),
  );
  check(
    names[7] ?? "",
    itself.includes("after itself"),
    itself ? preview(itself) : "it accepted a page as its own anchor",
  );

  const undone = await invoke<State>("edit_undo", { doc: doc.id }).catch(
    () => null,
  );
  const back = undone?.pages.map((page) => page.id) ?? [];
  const original = state.pages.map((page) => page.id);
  check(
    names[8] ?? "",
    back.length === original.length && back.every((id, at) => id === original[at]),
    `ids ${preview(back.join(","))}, wanted ${preview(original.join(","))}`,
  );
}

/**
 * The text layer's half of a page turn, on the page and on its neighbour.
 *
 * Split out so that the two text assertions skip together and for one stated
 * reason: half the corpus has no extractable text, and a text check on such a
 * document is not a failure, it is a question the document cannot answer.
 */
async function pageTurnTextChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  target: number,
  other: number,
  otherBefore: { quarter_turns: number; width_pt: number } | null,
  names: string[],
): Promise<void> {
  const turned = names[6] ?? "";
  const untouched = names[7] ?? "";
  const shown = viewer.textOn(target);
  if (!shown || shown.codes.length === 0) {
    skip(turned, "the page has no extractable text");
    skip(untouched, "the page has no extractable text");
    return;
  }

  // From the backend, so the comparison is against the document rather than
  // against the same cache being checked.
  const raw = await invoke<{ quarter_turns: number; width_pt: number }>(
    "page_text",
    { doc: doc.id, page: target },
  ).catch(() => null);
  if (!raw) {
    skip(turned, "the page's text could not be fetched a second time");
    skip(untouched, "the page's text could not be fetched a second time");
    return;
  }

  const wanted = (raw.quarter_turns + viewer.pageExtraTurns(target)) % 4;
  check(
    turned,
    shown.quarter_turns === wanted && shown.width_pt !== raw.width_pt,
    `page is /Rotate ${raw.quarter_turns * 90}, turned ${viewer.pageExtraTurns(target) * 90}: ` +
      `the text layer reports /Rotate ${shown.quarter_turns * 90} ` +
      `(wanted ${wanted * 90}) on a ${shown.width_pt.toFixed(0)} pt wide page ` +
      `(the document says ${raw.width_pt.toFixed(0)})`,
  );

  const otherNow = viewer.textOn(other);
  if (!otherBefore || !otherNow) {
    skip(untouched, "the neighbouring page has no text layer to compare");
    return;
  }
  check(
    untouched,
    otherNow.quarter_turns === otherBefore.quarter_turns &&
      otherNow.width_pt === otherBefore.width_pt,
    `page 2 was /Rotate ${otherBefore.quarter_turns * 90} at ` +
      `${otherBefore.width_pt.toFixed(0)} pt wide, now /Rotate ` +
      `${otherNow.quarter_turns * 90} at ${otherNow.width_pt.toFixed(0)} pt`,
  );
}

/**
 * Inverting the page, at both ends of the path.
 *
 * Two very different assertions, because two very different things can be
 * wrong. The renderer might not invert; or it might invert perfectly while
 * nothing on screen changes, because the flag never reached a tile request.
 *
 * The first is answered exactly. Lightness inversion has a closed form --- every
 * channel moves by `255 - max - min` --- so the inverted tile is not merely
 * "different", it is a value this check can compute for itself and compare byte
 * for byte. Re-deriving the formula here would only duplicate whatever the Rust
 * got wrong, so the *independent* half is the pair of properties beside it: the
 * transform must actually change the tile, and it must be its own inverse.
 *
 * The second is answered on the composited canvas, which is the last thing
 * before the compositor and the only pixels in this file that a reader would
 * actually see.
 */
async function invertChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  page: { width_pt: number; height_pt: number },
  seen: { status: ViewerStatus | null },
): Promise<void> {
  await rendererInvertCheck(doc, page);
  await screenInvertCheck(viewer, seen);
}

/**
 * The bytes the renderer sent, read off the wire rather than through a canvas.
 *
 * A canvas round trip cannot be used here, and finding that out cost a run. An
 * `ImageBitmap` drawn onto a canvas is **premultiplied**, so every pixel with
 * alpha 0 reads back as `[0,0,0,0]` whatever colour the renderer put there ---
 * and a square tile of a portrait page is about a sixth transparent. The oracle
 * then "wanted" white in the margins, the renderer had genuinely produced white
 * in the margins, and the comparison failed on a difference neither side had.
 *
 * The claim under test is about what the renderer returns, so the wire is the
 * right place to read it. It also removes the decode from a comparison that was
 * never about the decode.
 */
async function tileBytes(
  req: Parameters<typeof tileUrl>[0],
): Promise<Uint8ClampedArray | null> {
  const response = await fetch(tileUrl(req));
  if (!response.ok || response.status === 204) return null;
  return new Uint8ClampedArray(await response.arrayBuffer());
}

/** The lightness inversion, as an oracle for what the renderer should return. */
function invertedCopy(rgba: Uint8ClampedArray): Uint8ClampedArray {
  const out = new Uint8ClampedArray(rgba);
  for (let at = 0; at + 3 < out.length; at += 4) {
    const r = out[at] ?? 0;
    const g = out[at + 1] ?? 0;
    const b = out[at + 2] ?? 0;
    const offset = 255 - Math.max(r, g, b) - Math.min(r, g, b);
    out[at] = r + offset;
    out[at + 1] = g + offset;
    out[at + 2] = b + offset;
  }
  return out;
}

/** Mean lightness, `(max + min) / 2` per pixel, over an RGBA buffer. */
function meanLightness(rgba: Uint8ClampedArray): number {
  let total = 0;
  let count = 0;
  for (let at = 0; at + 3 < rgba.length; at += 4) {
    const r = rgba[at] ?? 0;
    const g = rgba[at + 1] ?? 0;
    const b = rgba[at + 2] ?? 0;
    const high = Math.max(r, g, b);
    const low = Math.min(r, g, b);
    total += (high + low) / 2;
    count += 1;
  }
  return count === 0 ? 0 : total / count / 255;
}

async function rendererInvertCheck(
  doc: DocumentInfo,
  page: { width_pt: number; height_pt: number },
): Promise<void> {
  const exact = "an inverted tile is the exact inversion of the plain one";
  const moved = "inverting a tile changes it";
  const edge = 150;
  const request = (invert: boolean) =>
    tileBytes({
      doc: doc.id,
      page: 0,
      // A whole small page rather than a tile of one, for the reason the
      // rotation check gives: a fixed-offset tile can be blank, and two blank
      // tiles agree under every transform there is.
      scale: edge / Math.max(page.width_pt, page.height_pt),
      x: 0,
      y: 0,
      width: edge,
      height: edge,
      invert,
      format: "raw",
    });

  const [plainPixels, darkPixels] = await Promise.all([
    request(false),
    request(true),
  ]).catch(() => [null, null]);
  if (!plainPixels || !darkPixels || plainPixels.length !== darkPixels.length) {
    skip(exact, "the tile requests did not complete");
    skip(moved, "the tile requests did not complete");
    return;
  }

  // The control, and it comes first because it is what stops the exact check
  // passing on a page the transform happens to fix. Every pixel of a uniformly
  // mid-grey tile is its own inversion, and on such a tile "the renderer
  // returned the exact inversion" is satisfied by a renderer that did nothing.
  let differences = 0;
  for (let at = 0; at < plainPixels.length; at++) {
    if (plainPixels[at] !== darkPixels[at]) differences += 1;
  }
  check(
    moved,
    differences > 0,
    differences > 0
      ? `${differences} of ${plainPixels.length} bytes differ`
      : "the inverted tile is byte-identical, so the exact check below proves nothing",
  );
  if (differences === 0) {
    skip(
      exact,
      "the two tiles are identical, so there is nothing to compare against",
    );
    return;
  }

  const wanted = invertedCopy(plainPixels);
  let wrong = 0;
  let firstWrong = -1;
  for (let at = 0; at < wanted.length; at++) {
    if (wanted[at] !== darkPixels[at]) {
      wrong += 1;
      if (firstWrong < 0) firstWrong = at;
    }
  }
  check(
    exact,
    wrong === 0,
    wrong === 0
      ? `${plainPixels.length / 4} pixels match the closed form exactly`
      : `${wrong} bytes differ; pixel ${Math.floor(firstWrong / 4)} was ` +
          `[${[0, 1, 2, 3].map((c) => plainPixels[(firstWrong & ~3) + c]).join(",")}], ` +
          `wanted [${[0, 1, 2, 3].map((c) => wanted[(firstWrong & ~3) + c]).join(",")}], ` +
          `got [${[0, 1, 2, 3].map((c) => darkPixels[(firstWrong & ~3) + c]).join(",")}]`,
  );
}

async function screenInvertCheck(
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const darker = "the page on screen goes dark when it is inverted";
  const back = "and light again when it is turned off";
  const dropped = "inverting discards what it invalidates";
  const reported = "the status says the page is inverted";

  const surface = viewer.compositedSurface;
  if (!surface) {
    for (const name of [darker, back, dropped, reported]) {
      skip(
        name,
        "this layout composites per tile, so there is no single surface",
      );
    }
    return;
  }

  /**
   * Mean lightness of the middle of the viewport.
   *
   * The centre rather than the whole canvas, because the surround is painted
   * into it too and does not invert --- and at fit-width the middle is inside
   * the page on every corpus here.
   */
  const middle = (): number | null => {
    const ctx = surface.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;
    const w = Math.floor(surface.width / 3);
    const h = Math.floor(surface.height / 3);
    if (w < 1 || h < 1) return null;
    return meanLightness(ctx.getImageData(w, h, w, h).data);
  };

  await settle(() => viewer.idle);
  const before = middle();
  if (before === null) {
    for (const name of [darker, back, dropped, reported]) {
      skip(name, "the surface could not be read back");
    }
    return;
  }
  // A corpus with no bright paper cannot show "it got darker" --- and saying so
  // is the point: a check that silently passed on such a document would be
  // measuring nothing. `vector-heavy` is dense linework and lands here.
  if (before < 0.6) {
    for (const name of [darker, back, dropped]) {
      skip(
        name,
        `the page is already dark at ${(before * 100).toFixed(0)}% lightness`,
      );
    }
    await invertReportedCheck(viewer, seen, reported);
    return;
  }

  viewer.setInverted(true);
  // The control this repository keeps having to relearn: waiting for a good
  // state proves nothing unless the state was first shown to be bad. One frame
  // after the toggle every tile must be gone, or "it went dark" could be
  // satisfied by tiles that never changed at all.
  await frame();
  const midSharp = seen.status?.sharp ?? 1;
  check(
    dropped,
    midSharp < 0.999,
    midSharp < 0.999
      ? `sharp coverage fell to ${(midSharp * 100).toFixed(1)}% one frame after the toggle`
      : `sharp coverage was still ${(midSharp * 100).toFixed(1)}% a frame after the toggle, ` +
          `so nothing was discarded and "it went dark" could not have been earned`,
  );

  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);
  const after = middle();
  check(
    darker,
    after !== null && after < 0.4,
    `middle of the viewport went from ${(before * 100).toFixed(0)}% to ` +
      `${((after ?? 0) * 100).toFixed(0)}% lightness`,
  );

  await invertReportedCheck(viewer, seen, reported);

  viewer.setInverted(false);
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);
  const restored = middle();
  check(
    back,
    restored !== null && restored >= 0.6,
    `back to ${((restored ?? 0) * 100).toFixed(0)}% lightness, from ${(before * 100).toFixed(0)}%`,
  );
}

/** The status carries the mode, which is how the page strip learns about it. */
async function invertReportedCheck(
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
  name: string,
): Promise<void> {
  await settle(() => seen.status?.invert === viewer.inverted);
  check(
    name,
    seen.status?.invert === viewer.inverted,
    `viewer says ${viewer.inverted}, status says ${seen.status?.invert}`,
  );
}

/**
 * The renderer is actually asked for the rotation.
 *
 * The gap every other check here leaves. Nothing above looks at a pixel: if the
 * `turns` parameter were dropped on its way into the URL, the boxes would still
 * turn, the layout would still turn, the same lines would still come back out of
 * the drag --- and the page on screen would be upright underneath all of it.
 *
 * So this fetches the same tile twice and asserts the two differ, with the
 * control that matters beside it: fetching it twice at the *same* rotation must
 * give identical bytes, or "they differ" would be satisfied by a renderer that
 * is merely non-deterministic.
 */
async function rotatedTileCheck(
  doc: DocumentInfo,
  page: { width_pt: number; height_pt: number },
): Promise<void> {
  const name = "the renderer is asked for the rotation";
  const edge = 150;
  const request = (turns: number) =>
    fetchRequiredTile({
      doc: doc.id,
      page: 0,
      // A whole small page, not a tile of one: a tile at a fixed offset can miss
      // the content entirely once the page turns, and two blank tiles differ in
      // no pixel at all.
      scale: edge / Math.max(page.width_pt, page.height_pt),
      turns,
      x: 0,
      y: 0,
      width: edge,
      height: edge,
      format: "raw",
    });

  const [upright, again, turned] = await Promise.all([
    request(0),
    request(0),
    request(1),
  ]).catch(() => [null, null, null]);
  if (!upright || !again || !turned) {
    skip(name, "the tile requests did not complete");
    return;
  }

  const stable = await identical(upright.bitmap, again.bitmap);
  const differs = !(await identical(upright.bitmap, turned.bitmap));
  check(
    name,
    stable && differs,
    !stable
      ? "the same request rendered differently twice, so a difference proves nothing"
      : differs
        ? "turns=0 and turns=1 render different pixels, and turns=0 is reproducible"
        : "turns=1 rendered exactly the same pixels as turns=0",
  );
}

/** Whether two bitmaps are pixel-identical. */
async function identical(a: ImageBitmap, b: ImageBitmap): Promise<boolean> {
  if (a.width !== b.width || a.height !== b.height) return false;
  const read = (bitmap: ImageBitmap): Uint8ClampedArray | null => {
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx?.drawImage(bitmap, 0, 0);
    return ctx?.getImageData(0, 0, bitmap.width, bitmap.height).data ?? null;
  };
  const [left, right] = [read(a), read(b)];
  if (!left || !right || left.length !== right.length) return false;
  for (let at = 0; at < left.length; at++) {
    if (left[at] !== right[at]) return false;
  }
  return true;
}

/**
 * Whether two selections came off the same line of text.
 *
 * Not string equality. Rotating refits the page to the window, so the zoom
 * changes and the two drag endpoints land a character or so further in or out
 * --- which is benign, and produced "ine 03 charlie delta ech" against "Line 03
 * charlie delta ec" on a page that had rotated perfectly. The core of the
 * shorter one has to appear in the other, which tolerates an edge and not a
 * line: a one-line error returns a different line number entirely.
 */
function sameLine(a: string, b: string): boolean {
  return (
    core(a).length >= CORE_CHARS && (b.includes(core(a)) || a.includes(core(b)))
  );
}

/** The comparable middle of a sample, with an edge character trimmed each end. */
function core(text: string): string {
  return text.slice(2, -2);
}

/**
 * How much of a sample has to survive trimming for a comparison to mean anything.
 *
 * Shared with the precondition that guards it, which is the point: written as a
 * bare `8` here and a bare `8` there, the guard admitted a nine-character sample
 * that `sameLine` then rejected for having a five-character core --- so a
 * fixture with short lines reported a **failed** rotation rather than a sample
 * it could not use. `columns.pdf` is that fixture, and the symptom was a check
 * whose own detail line showed the two strings being identical.
 */
const CORE_CHARS = 8;

/** The box enclosing every character that has one, in the view's own space. */
function inkBounds(text: {
  boxes: number[];
}): { left: number; top: number; right: number; bottom: number } | null {
  let left = Infinity;
  let top = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (let at = 0; at < text.boxes.length; at += 4) {
    const quad = text.boxes.slice(at, at + 4) as number[];
    // Four zeroes is "PDFium gave this character no box", and taking it as a
    // corner would drag the extent to the page's origin.
    if (quad[2]! <= quad[0]! || quad[3]! <= quad[1]!) continue;
    left = Math.min(left, quad[0]!);
    top = Math.min(top, quad[1]!);
    right = Math.max(right, quad[2]!);
    bottom = Math.max(bottom, quad[3]!);
  }
  return right > left && bottom > top ? { left, top, right, bottom } : null;
}

/** The page strip's rows change shape with the rotation. */
function rotatedStripCheck(
  sidebar: Sidebar,
  page: { width_pt: number; height_pt: number },
): void {
  const name = "the page strip turns with the view";
  const strip = sidebar.thumbnails;
  const built = strip?.mounted[0];
  const row = built === undefined ? null : strip?.elementFor(built);
  if (!row) {
    skip(name, "no thumbnail row is currently built");
    return;
  }
  if (Math.abs(page.width_pt - page.height_pt) < 1) {
    skip(name, "the page is square, so a quarter turn changes no row height");
    return;
  }

  // Against `rowHeightFor`, which is unit-tested against the aspect ratio, so
  // this is the wiring rather than the arithmetic: a strip that never heard
  // about the rotation keeps the upright height and goes red.
  const wanted = rowHeightFor(page, 1);
  const actual = Math.round(parseFloat(row.style.height));
  check(
    name,
    actual === wanted,
    `row is ${actual} px, a turned page wants ${wanted} (upright is ${rowHeightFor(page, 0)})`,
  );
}

/** Every tab button in the sidebar, in order. */
function panelTabs(): HTMLElement[] {
  return [
    ...document.querySelectorAll<HTMLElement>('.tpdf-sidebar [role="tab"]'),
  ];
}

/** Whether a panel is displayed. */
function panelShown(tab: string): boolean {
  const panel = document.getElementById(`tpdf-panel-${tab}`);
  return !!panel && getComputedStyle(panel).display !== "none";
}

/** Activating a row moves the viewer, asserted against not being there already. */
async function navigateFromStrip(
  viewer: Viewer,
  strip: { mounted: number[]; elementFor(page: number): HTMLElement | null },
  doc: DocumentInfo,
): Promise<void> {
  // Not "activating a row", which is what the outline's equivalent is called:
  // two different checks under one name make the transcript ambiguous, and the
  // name is how a check that stopped existing is noticed.
  const name = "activating a thumbnail goes to its page";
  if (doc.page_count < 2) {
    skip(name, "the document has one page");
    return;
  }

  viewer.goToStart();
  await settle(() => viewer.idle);
  const here = viewer.position.page;
  // The furthest *built* row that is neither the one we are on nor the last
  // page of the document. Two mistakes are being avoided at once. Written as
  // "the last page" it skipped on every document long enough for windowing to
  // matter; written without excluding the final page it *failed* on a
  // four-page document, because scrolling to the last page clamps at
  // `maxScroll` and leaves the page before it still at the top of the viewport
  // --- the same clamp the outline's destination check already guards against.
  const candidates = strip.mounted.filter((page) => page !== here);
  const target =
    candidates.filter((page) => page < doc.page_count - 1).pop() ??
    candidates.pop();
  if (target === undefined) {
    skip(name, `no row other than page ${here + 1} is currently built`);
    return;
  }

  // The fallback above hands back exactly the page the filter exists to avoid,
  // whenever it is the only one there is --- which on a two-page document it
  // always is. So the preference is checked rather than trusted: go to the
  // chosen page and see whether the viewer can report being on it. A page that
  // clamps at `maxScroll` never becomes `position.page`, and the check then
  // fails on a strip that did its job.
  viewer.goToPage(target);
  await settle(() => viewer.idle);
  const canLand = viewer.position.page === target;
  viewer.goToStart();
  await settle(() => viewer.idle);
  if (!canLand) {
    skip(
      name,
      `page ${target + 1} of ${doc.page_count} cannot reach the top of the viewport`,
    );
    return;
  }

  const element = strip.elementFor(target);
  if (!element) {
    skip(name, `no row other than page ${here + 1} is currently built`);
    return;
  }

  element.focus();
  // Focus is this check's *precondition*, not part of what it asserts: the
  // strip navigates to its own focused row, so a `focus()` that did not land
  // leaves that row at page 0 and Enter goes to page 1 --- which prints as
  // "from page 1 to 1" and reads as a broken navigation rather than as lost
  // focus. Recorded rather than asserted, because the two failures want
  // different fixes and the detail line is what says which one happened.
  // Three observables, because "went nowhere" has three causes that print
  // identically. `landed` is the DOM's own focus; `roved` is the strip's, since
  // the roving tabindex is set by the same `focus(page)` the `focusin` listener
  // calls, so it says whether that listener ever ran; `hasFocus` is there
  // because a document without system focus is exactly where the two can
  // disagree -- the element becomes `activeElement` and the focus event is not
  // delivered.
  const landed = document.activeElement === element;
  const roved = element.tabIndex === 0;
  const framed = document.hasFocus();
  key(element, "Enter");
  await settle(() => viewer.position.page === target);
  check(
    name,
    viewer.position.page === target,
    `from page ${here + 1} to ${viewer.position.page + 1}, wanted ${target + 1}` +
      (viewer.position.page === target
        ? ""
        : `, activeElement=${landed}, strip followed=${roved}, document focused=${framed}`),
  );
}

/**
 * The yield, and the hidden strip --- both with the same control.
 *
 * Neither can be tested on a document whose thumbnails finish faster than a
 * frame: "nothing is outstanding" is then true for reasons that have nothing to
 * do with the mechanism. So both wait to catch a request in flight first, and
 * both skip with that reason when they cannot.
 */
async function yieldChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  pageCount: number,
  strip: {
    outstanding: boolean;
    yieldCount: number;
    rendered: number[];
  },
): Promise<void> {
  const yields =
    "the strip withdraws its work when the viewer needs the renderer";
  const quiet = "a hidden strip asks for nothing";
  const resumes = "and starts again when it is shown";

  /** Waits to catch a thumbnail in flight, which every check here needs. */
  const caught = async (): Promise<boolean> => {
    const deadline = performance.now() + 3000;
    while (!strip.outstanding && performance.now() < deadline) await frame();
    return strip.outstanding;
  };

  const why =
    "no thumbnail stayed in flight long enough to collide with anything";
  if (!(await caught())) {
    skip(yields, why);
    skip(quiet, why);
    skip(resumes, why);
    return;
  }

  // Three outcomes, not two, and the third is why this is written out rather
  // than folded into one boolean. A thumbnail that *finished* before the viewer
  // asked for anything leaves nothing outstanding --- which reads exactly like a
  // successful withdrawal and is no evidence at all. On the text corpus a
  // thumbnail takes about a millisecond, so that is the common case there, and
  // an earlier version of this check reported it as a failure. It is neither.
  const before = strip.yieldCount;
  // No `await` between the test and the press: the fetch can only settle in a
  // microtask, so this is the one place the two are known to be simultaneous.
  const inFlight = strip.outstanding;
  // A zoom step throws away every tier-2 tile, so the viewer wants a screenful
  // immediately. This is the collision the whole design is about.
  key(root, "+", true);
  await frame();
  await frame();

  if (!inFlight) {
    skip(yields, `${why} — it settled between catching it and the zoom`);
  } else if (strip.yieldCount > before) {
    check(
      yields,
      !strip.outstanding,
      `withdrew ${strip.yieldCount - before} request(s) within two frames of ` +
        `the viewer wanting tiles; outstanding=${strip.outstanding}`,
    );
  } else if (strip.outstanding) {
    check(
      yields,
      false,
      "the thumbnail is still rendering with the viewer waiting behind it",
    );
  } else {
    skip(yields, "the thumbnail finished before the viewer asked for anything");
  }

  // And the other half. Hiding the whole panel rather than switching tabs:
  // they are the same thing to the strip, and the panel is what a reader
  // closes.
  //
  // Not written as "catch a request and watch it disappear", which is what the
  // check above does and what this one did first: it skipped on every corpus,
  // because by the time the zoom had settled the few rows on screen were all
  // drawn and there was nothing left in flight to catch. The property does not
  // need a request in flight --- it needs *work remaining*, which is a fact
  // about the document rather than about timing.
  key(root, "0", true);
  await settle(() => viewer.idle);

  // Somewhere the strip has not drawn yet. "Any page without a thumbnail" is
  // not enough --- the strip only renders the rows in its own window, so on the
  // 775-page corpus it had 763 pages left and nothing to do, and the control
  // below failed for exactly the right reason. What has to move is the window.
  const already = new Set(strip.rendered);
  let far = -1;
  for (let page = pageCount - 1; page >= 0; page--) {
    if (!already.has(page)) {
      far = page;
      break;
    }
  }
  if (far < 0) {
    const done = "every page of this document already has a thumbnail";
    skip(quiet, done);
    skip(resumes, done);
    return;
  }

  sidebar.setVisible(false);
  const stopped = !strip.outstanding;
  // Moved while hidden, so the strip's window lands on undrawn rows the moment
  // it is shown again. It follows the reader either way; doing it here is what
  // makes "it starts again" a claim about being shown rather than about the
  // scroll that happened to accompany it.
  viewer.goToPage(far);
  await settle(() => viewer.idle);

  const drawn = strip.rendered.length;
  // Long enough that a strip still working would have finished something: a
  // thumbnail costs about a millisecond on a text page and a second and a half
  // on the A0 sheet, and this waits three seconds.
  const until = performance.now() + 3000;
  while (performance.now() < until) await frame();

  check(
    quiet,
    stopped && strip.rendered.length === drawn,
    `withdrew on hide=${stopped}, ${drawn} of ${pageCount} thumbnails before ` +
      `and ${strip.rendered.length} after three seconds hidden on page ${far + 1}`,
  );

  // The control, and it is the half that makes the check above mean anything:
  // a strip with nothing left to draw would also draw nothing while hidden.
  sidebar.setVisible(true);
  await eventually(
    resumes,
    () => strip.rendered.length > drawn,
    () => `${drawn} thumbnails while hidden, ${strip.rendered.length} after`,
  );
}

/**
 * A word from a page's characters, long enough to be worth searching for.
 *
 * Taken from the document so the check is not tied to one fixture's vocabulary.
 * Letters only: a needle containing a space would exercise the whitespace fold,
 * which is a different claim and has its own test in `search.rs`.
 *
 * Deliberately **not** the page's first word. The check downstream asserts that
 * a match's index range covers the characters searched for, and a first match at
 * index 0 is the one value an implementation that had lost track of its indices
 * would be most likely to return anyway.
 */
function pickNeedle(codes: number[]): string | null {
  const text = String.fromCodePoint(...codes.slice(0, 4096));
  // Latin words first, five characters or more, which is what this looked for
  // exclusively until 2026-08-01. On `multilingual.pdf` it found none --- Kanji are
  // not `[A-Za-z]` --- so `pickNeedle` returned null and **seventeen** search
  // checks skipped, every one of them saying "the page has no extractable text"
  // about a page with forty-nine characters on it. The checks did not run and the
  // reason printed was false, which is the worse half: a skip is read as "this
  // fixture cannot exercise it" rather than "the harness cannot read this script".
  const latin = text.match(/[A-Za-z]{5,}/g);
  if (latin?.length) {
    const lead = latin[0]?.toLowerCase();
    return (
      latin.find((word) => word.toLowerCase() !== lead) ?? latin[0] ?? null
    );
  }
  // Any script's letters, and **two** of them is a word in Chinese or Japanese.
  // Five would be a sentence, and the run this picks has to be short enough that
  // the whole-word and in-selection checks have something longer around it.
  const letters = text.match(/[\p{L}\p{N}]{2,}/gu);
  if (!letters?.length) return null;
  const longest = [...letters].sort((a, b) => b.length - a.length)[0] ?? "";
  // A slice from the middle rather than the whole run: a needle that *is* the run
  // makes "whole words rejects a hit inside a longer word" vacuous, and on a
  // script with no spaces the run is the whole line.
  return longest.length >= 4 ? longest.slice(1, 3) : longest;
}

/** A short, single-line form of a string, for a detail column. */
function preview(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > 40 ? `${flat.slice(0, 40)}...` : flat;
}

/**
 * The print command, up to but not including the panel.
 *
 * Everything about printing that a test can reach is in Rust and covered there.
 * What is **not** covered anywhere else is the wiring: a mistyped command name
 * or a parameter Tauri cannot deserialise compiles perfectly and fails the first
 * time somebody presses Cmd-P. Nothing in the gate would notice, because the
 * two sides never meet until then.
 *
 * So these ask for jobs the backend must *refuse*, and assert on the reason it
 * gives. A refusal proves the command exists, the arguments arrived, and
 * `print::build` ran --- and stops before anything platform-specific, which is
 * the half that would need a person and a sheet of paper.
 */
/**
 * That a document can be released, through the command the app really calls.
 *
 * `backend-probe` pins what releasing *does* --- the process dies, the id is
 * refused afterwards and never handed out again, and the descriptors come back.
 * What it cannot see is this: whether the Tauri command exists under that name
 * and takes an argument called `doc`. A wrong name there fails only at runtime,
 * only when someone opens a second file, and only as a warning in a console
 * nobody is watching --- so the leak comes back with every check still green.
 *
 * Opened here rather than reusing the document under test, which the viewer is
 * still reading from.
 */
async function releaseChecks(path: string): Promise<void> {
  const extra = await invoke<DocumentInfo>("open_document", { path });
  const release = async (doc: number): Promise<string> => {
    try {
      await invoke("close_document", { doc });
      return "";
    } catch (e) {
      return String(e);
    }
  };

  const first = await release(extra.id);
  check(
    "a document can be released through the command",
    first === "",
    first ? preview(first) : `released document ${extra.id}`,
  );

  // The control, and it is the half that pins the argument. "It returned no
  // error" is equally true of a command that ignored `doc` entirely, or of one
  // that quietly succeeded on an id it had never seen; a second release of the
  // same id has to be refused, and refused *by that id*.
  const again = await release(extra.id);
  check(
    "releasing the same document twice is refused",
    again.includes(`document ${extra.id}`) && again.includes("closed"),
    again ? preview(again) : "it accepted the second release",
  );
}

/** What a fixture's generator says each of its pages is, and where its ink is. */
interface GeometryManifest {
  /** Page 1's size, i.e. the one a uniform layout would use for everything. */
  first_page: { width_pt: number; height_pt: number };
  pages: {
    page: number;
    name: string;
    width_pt: number;
    height_pt: number;
    markers: { text: string; x: number; y: number; width_pt: number }[];
  }[];
}

const LAID_OUT = "every page is laid out at its own size";
const OFFSETS = "page offsets accumulate each page's own height";
const DRAWN = "an oversized page is drawn past page 1's width";

/**
 * How tall a band around a marker's baseline is sampled for ink, in points.
 *
 * `make_mixed_pdf.py` sets its markers in 12-point Helvetica and does not record
 * the size, so this is deliberately generous rather than exact. Generous is the
 * safe direction: extra rows are blank page, which adds white and can never
 * invent ink, so a band too tall weakens nothing.
 */
const MARKER_BAND_PT = 20;

/** Fraction of a sampled region that has to be page rather than surround. */
const PAGE_WHITE = 0.3;
/** Fraction that has to be ink, which one line of 12-point type easily clears. */
const PAGE_INK = 0.005;

/**
 * The layout, against a description of the document that this process did not
 * write.
 *
 * The second check in this file whose answer comes from outside, and the reason
 * it needs to be: everything the viewer knows about a page's size it learned
 * from the same backend it renders through, so a check comparing the layout
 * against `doc.pages` would be the writer agreeing with its own reader. The
 * fixture's generator states every page's size and where every marker was drawn,
 * `viewer_check.py` passes that file in, and this compares against it.
 *
 * All three skip on a document with no sidecar, which is every fixture but
 * `mixed.pdf` --- and on one whose pages are all the same size, which is what
 * they would silently pass on. A property with one value present is the same as
 * none: an assertion that offsets follow each page's own height is satisfied by
 * multiplying page 1's when every page *is* page 1's size.
 *
 * The pages are visited before anything is asserted, because the open is lazy:
 * page 1's geometry is all that arrives and the rest is learned from the text
 * extraction of each page as it comes on screen. A check that asserted straight
 * away would be asserting against the estimate.
 */
async function geometryChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [LAID_OUT, OFFSETS, DRAWN];
  const raw = await invoke<string | null>("geometry_manifest").catch(
    () => null,
  );
  if (!raw) {
    for (const name of names)
      skip(name, "no geometry sidecar for this fixture");
    return;
  }

  let manifest: GeometryManifest;
  try {
    manifest = JSON.parse(raw) as GeometryManifest;
  } catch (e) {
    for (const name of names) skip(name, `unreadable geometry sidecar: ${e}`);
    return;
  }

  const stated = manifest.pages.filter((entry) => entry.page < doc.page_count);
  const widths = new Set(stated.map((entry) => entry.width_pt));
  const heights = new Set(stated.map((entry) => entry.height_pt));
  if (widths.size < 2 && heights.size < 2) {
    for (const name of names) {
      skip(name, "every page of this fixture is the same size");
    }
    return;
  }

  // Every page visited, so every size is learned rather than estimated. The
  // wait is per page and its verdict is dropped: the checks below fail naming
  // the page that never resolved, which is more use than a second failure here.
  for (const entry of stated) {
    if (viewer.knowsPageSize(entry.page)) continue;
    viewer.goToPage(entry.page);
    await settle(() => viewer.knowsPageSize(entry.page));
  }

  const zoom = viewer.currentZoom;
  const unknown = stated.filter((entry) => !viewer.knowsPageSize(entry.page));
  const wrongBox = stated.filter((entry) => {
    const box = viewer.pageBoxCssOf(entry.page);
    return (
      Math.abs(box.width - entry.width_pt * zoom) > 1 ||
      Math.abs(box.height - entry.height_pt * zoom) > 1
    );
  });
  check(
    LAID_OUT,
    unknown.length === 0 && wrongBox.length === 0,
    unknown.length > 0
      ? `page ${(unknown[0]?.page ?? 0) + 1} never reported its size`
      : wrongBox.length === 0
        ? `${stated.length} pages, ${widths.size} widths and ${heights.size} heights`
        : `page ${(wrongBox[0]?.page ?? 0) + 1} is ` +
          `${viewer.pageBoxCssOf(wrongBox[0]?.page ?? 0).width.toFixed(0)} px wide, ` +
          `${((wrongBox[0]?.width_pt ?? 0) * zoom).toFixed(0)} wanted`,
  );

  // The gap between pages is the scroller's own constant and nothing out here
  // should be pinning the number; what is pinned is that the *same* gap
  // separates every pair and that the height between them is each page's own.
  // Derived from the first pair, so it is the later pages that are the claim ---
  // which is where a layout multiplying page 1's height and one accumulating
  // per page first disagree.
  const first = stated[0];
  const second = stated[1];
  if (!first || !second || stated.length < 3) {
    skip(OFFSETS, "fewer than three pages, so no offset can have accumulated");
  } else {
    const gap = viewer.pageTopCss(second.page) - first.height_pt * zoom;
    let expected = viewer.pageTopCss(second.page);
    const wrongTop: { page: number; got: number; want: number }[] = [];
    for (let index = 2; index < stated.length; index++) {
      const previous = stated[index - 1];
      const entry = stated[index];
      if (!previous || !entry) continue;
      expected += previous.height_pt * zoom + gap;
      const got = viewer.pageTopCss(entry.page);
      if (Math.abs(got - expected) > 1.5) {
        wrongTop.push({ page: entry.page, got, want: expected });
      }
    }
    const worst = wrongTop[0];
    check(
      OFFSETS,
      wrongTop.length === 0,
      worst
        ? `page ${worst.page + 1} starts at ${worst.got.toFixed(0)} px, ` +
            `${worst.want.toFixed(0)} wanted`
        : `${stated.length} pages, gap ${gap.toFixed(0)} px`,
    );
  }

  await drawnPastFirstPageCheck(viewer, manifest, stated, seen);
}

/**
 * That the widest page's pixels reach past page 1's width.
 *
 * The half the offsets cannot see, and the one that loses content rather than
 * misplacing it: the tile grid comes from the page's own width, so a page laid
 * out at page 1's is never *asked for* past it and is drawn cropped, silently.
 * There is no error to look for, which is why this reads pixels.
 *
 * `make_mixed_pdf.py` places a marker a few points past page 1's right edge
 * precisely so that a layout which is generous rather than correct --- one that
 * rounds a column up, or adds a tile of slack --- still misses something. The
 * page's own far-right markers sit 500 points beyond A4 and would be lost by any
 * wrong answer at all, which makes them the easy case.
 *
 * Two conditions, and the second is the instrument's control. The marker's box
 * must be mostly *page* (so what is on screen there is paper rather than the
 * surround the crop would leave) and must contain ink (so the paper is not
 * merely blank). A third sample, taken to the left of the page where there can
 * be no paper, is what says "white" means page at all --- without it, a canvas
 * read back as uniform white would satisfy both.
 */
async function drawnPastFirstPageCheck(
  viewer: Viewer,
  manifest: GeometryManifest,
  stated: GeometryManifest["pages"],
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const widest = stated.reduce((a, b) => (b.width_pt > a.width_pt ? b : a));
  if (widest.width_pt <= manifest.first_page.width_pt) {
    skip(DRAWN, "no page of this fixture is wider than page 1");
    return;
  }
  const beyond = widest.markers
    .filter((marker) => marker.x > manifest.first_page.width_pt)
    .sort((a, b) => a.x - b.x)[0];
  if (!beyond) {
    skip(DRAWN, "the widest page has no ink past page 1's width");
    return;
  }

  viewer.goToPage(widest.page);
  await settle(() => viewer.idle);
  // Refitted here rather than relying on whatever the last fit left behind: the
  // sample has to be on screen, and a wide page at the previous page's scale
  // runs off the side of the window.
  viewer.setFit("width");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const surface = viewer.compositedSurface;
  const ctx = surface?.getContext("2d", { willReadFrequently: true }) ?? null;
  if (!surface || !ctx) {
    skip(
      DRAWN,
      "this layout composites per tile, so there is no single surface",
    );
    return;
  }

  const dpr = window.devicePixelRatio || 1;
  /** Fractions of white and of ink in a box given in the page's own points. */
  const sample = (
    x: number,
    width: number,
  ): { white: number; ink: number } | null => {
    // The marker's `y` is a baseline measured up from the page's bottom edge, as
    // PDF coordinates are; the viewer works down from the page's top.
    const top = widest.height_pt - beyond.y - MARKER_BAND_PT + 4;
    const a = viewer.screenPoint(widest.page, x, top);
    const b = viewer.screenPoint(widest.page, x + width, top + MARKER_BAND_PT);
    const left = Math.max(0, Math.round(Math.min(a.x, b.x) * dpr));
    const upper = Math.max(0, Math.round(Math.min(a.y, b.y) * dpr));
    const right = Math.min(surface.width, Math.round(Math.max(a.x, b.x) * dpr));
    const lower = Math.min(
      surface.height,
      Math.round(Math.max(a.y, b.y) * dpr),
    );
    if (right - left < 2 || lower - upper < 2) return null;
    const { data } = ctx.getImageData(left, upper, right - left, lower - upper);
    let white = 0;
    let ink = 0;
    for (let at = 0; at < data.length; at += 4) {
      const lightness =
        ((data[at] ?? 0) + (data[at + 1] ?? 0) + (data[at + 2] ?? 0)) /
        (3 * 255);
      if (lightness > 0.85) white++;
      if (lightness < 0.4) ink++;
    }
    const pixels = data.length / 4;
    return { white: white / pixels, ink: ink / pixels };
  };

  const marker = sample(beyond.x, beyond.width_pt);
  // Left of the page's own left edge, where there is nothing but surround. Same
  // band, same size, so the only difference is where it is.
  const outside = sample(-beyond.width_pt - 8, beyond.width_pt);
  if (!marker || !outside) {
    skip(DRAWN, "the sampled region fell outside the composited surface");
    return;
  }

  check(
    DRAWN,
    marker.white > PAGE_WHITE &&
      marker.ink > PAGE_INK &&
      outside.white < PAGE_WHITE,
    `"${beyond.text}" at ${beyond.x.toFixed(0)} pt: ` +
      `${(marker.white * 100).toFixed(0)}% page, ${(marker.ink * 100).toFixed(1)}% ink ` +
      `(off the page: ${(outside.white * 100).toFixed(0)}% page)`,
  );
}

async function printChecks(path: string, doc: DocumentInfo): Promise<void> {
  const print = async (
    pages: number[] | null,
    turns: number,
  ): Promise<string> => {
    try {
      await invoke("print_document", { path, pages, turns });
      return "";
    } catch (e) {
      return String(e);
    }
  };

  // A page past the end. Refused by `resolve`, before any platform code.
  const beyond = await print([doc.page_count + 1], 0);
  check(
    "the print command refuses a page the document does not have",
    beyond.includes("is not in this document"),
    beyond ? preview(beyond) : "it accepted the job",
  );

  // The control, and the half that actually pins the wiring: the message above
  // could be produced by a backend that refuses everything. An empty selection
  // is refused by a *different* branch, so the two together show the argument
  // was read rather than that some error was reached.
  const empty = await print([], 0);
  check(
    "the print command refuses an empty selection",
    empty.includes("no pages selected"),
    empty ? preview(empty) : "it accepted the job",
  );

  // And a turn count is a `u8` in Rust: sending a number outside it proves the
  // parameter is typed rather than ignored. `resolve` never runs here.
  const turned = await print(null, 4096);
  check(
    "the print command's rotation is a typed parameter",
    turned.length > 0 && !turned.includes("is not in this document"),
    turned ? preview(turned) : "it accepted 4096 quarter-turns",
  );
}
