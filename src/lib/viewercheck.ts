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
import { MULTI_CLICK_SLOP_PX } from "./clicks";
import { CommandRegistry } from "./commands";
import { allRows, isNavigable, type Outline, type Row } from "./outline";
import { Palette } from "./palette";
import { Sidebar } from "./sidebar";
import { fetchRequiredTile, tileUrl } from "./tiles";
import { OVERSCAN, rowHeightFor } from "./thumbnails";
import { Viewer, type ViewerStatus } from "./viewer";

/** Size of the surface the check mounts, in CSS pixels. */
const WIDTH = 900;
const HEIGHT = 700;

/** How long any single wait may take before the check gives up. */
const TIMEOUT_MS = 30_000;

interface PageSize {
  width_pt: number;
  height_pt: number;
}

interface DocumentInfo {
  id: number;
  pages: PageSize[];
  page_count: number;
}

type Outcome = "ok" | "fail" | "skip";

const results: { name: string; outcome: Outcome; detail: string }[] = [];

const LABEL: Record<Outcome, string> = {
  ok: "[OK]  ",
  fail: "[FAIL]",
  skip: "[SKIP]",
};

/**
 * Lines already handed to the process, in order.
 *
 * Each result is printed as it is recorded rather than in one block at the end,
 * because a run that never reaches the end prints *nothing* otherwise --- and
 * an empty transcript is exactly what a passing run looks like from outside the
 * webview. This cost an afternoon: a check that stopped partway was
 * indistinguishable from one that had not started, and the only fact available
 * was that the process was alive. Now the last line printed names where it got
 * to.
 *
 * Chained rather than awaited at the call site so `check` stays synchronous:
 * `invoke` resolves out of order under load, and a transcript whose lines are
 * shuffled is worse than one that arrives late.
 */
let printing: Promise<unknown> = Promise.resolve();

function emit(line: string): void {
  printing = printing.then(() => invoke("spike_print", { text: line }));
}

function record(name: string, outcome: Outcome, detail: string): void {
  results.push({ name, outcome, detail });
  emit(`${LABEL[outcome]} ${name.padEnd(40)} ${detail}`);
}

function check(name: string, ok: boolean, detail: string): void {
  record(name, ok ? "ok" : "fail", detail);
}

/**
 * Records a check that this document cannot exercise.
 *
 * Printed rather than omitted. A control that quietly disappears on some inputs
 * is indistinguishable from one that ran, and the whole point of a control is
 * to know whether it did.
 */
function skip(name: string, why: string): void {
  record(name, "skip", `not applicable — ${why}`);
}

function frame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

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

/** Waits for a precondition of a later check, without recording one. */
async function settle(predicate: () => boolean): Promise<void> {
  const deadline = performance.now() + TIMEOUT_MS;
  while (!predicate() && performance.now() < deadline) await frame();
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
function drag(root: HTMLElement, from: [number, number], to: [number, number]): void {
  pointer(root, "pointerdown", from[0], from[1]);
  for (let step = 1; step <= 4; step++) {
    const t = step / 4;
    pointer(root, "pointermove", from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t);
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

  const failed = results.filter((r) => r.outcome === "fail").length;
  const skipped = results.filter((r) => r.outcome === "skip").length;
  const ran = results.length - skipped;

  emit(
    "\n" +
      `${ran - failed}/${ran} checks passed` +
      (skipped ? `, ${skipped} not applicable to this document` : ""),
  );
  // The lines went out one at a time as they were recorded; this is where the
  // last of them is known to have landed.
  await printing;
  await invoke("spike_exit", { code: failed === 0 ? 0 : 1 });
  return true;
}

async function run(path: string): Promise<void> {
  const doc = await invoke<DocumentInfo>("open_document", { path });
  const page = doc.pages[0];
  if (!page) throw new Error("document reports no pages");

  const root = document.createElement("div");
  root.style.cssText = `position:fixed;left:0;top:0;width:${WIDTH}px;height:${HEIGHT}px;`;
  document.body.replaceChildren(root);

  const seen: { status: ViewerStatus | null } = { status: null };
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
  const panel = document.createElement("div");
  panel.style.cssText =
    `position:fixed;left:${WIDTH}px;top:0;width:300px;height:${HEIGHT}px;`;
  document.body.appendChild(panel);
  const sidebar = new Sidebar(panel, {
    onNavigate: (target, top) => viewer.goToDestination(target, top),
    pages: {
      doc: doc.id,
      pageCount: doc.page_count,
      page,
      tier1: { placeholderFor: (at) => viewer.placeholderFor(at) },
      onNavigate: (at) => viewer.goToPage(at),
    },
  });

  const viewer = new Viewer(root, {
    doc: doc.id,
    pageCount: doc.page_count,
    page,
    onStatus: (next) => {
      seen.status = next;
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
      () => `page spans ${((page.width_pt * viewer.currentZoom) / WIDTH) * 100}% of the width`,
    ))
  ) {
    return;
  }

  // Idling, in the direction that can fail. Work is outstanding from the first
  // frame, so a loop that had never started would report idle here and the
  // check below would pass on nothing.
  check("runs a frame loop while working", !viewer.idle, `idle=${viewer.idle}`);

  await eventually("covers the first screen", covered, pct);
  await eventually("stops the frame loop when settled", () => viewer.idle, () => "loop stopped");

  const before = viewer.offset;
  wheel(root, 400);
  check(
    "a wheel notch scrolls, and wakes the loop",
    viewer.offset > before && !viewer.idle,
    `offset ${before.toFixed(0)} -> ${viewer.offset.toFixed(0)}, idle=${viewer.idle}`,
  );

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
  // It only means something on a document long enough to jump out of the
  // window. On a one-page sheet End moves a few hundred pixels and the tiles
  // already on screen stay valid, which is correct behaviour and not something
  // to assert against.
  await frame();
  const jumped = viewer.maxOffset > HEIGHT;
  if (jumped) {
    check(
      "a jump discards what it leaves behind",
      !covered(),
      `${pct()} one frame after the jump`,
    );
  } else {
    skip(
      "a jump discards what it leaves behind",
      `the document is ${viewer.maxOffset.toFixed(0)} px longer than the window`,
    );
  }
  await eventually(
    "covers the last page",
    () => covered() && seen.status?.page === doc.page_count,
    () => `${pct()} on page ${seen.status?.page}/${doc.page_count}`,
  );

  key(root, "Home");
  check("Home returns to the top", viewer.offset === 0, `offset=${viewer.offset}`);

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
  check("a zoom step discards what it invalidates", !covered(), `${pct()} one frame later`);
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

  await selectionChecks(root, viewer, doc);
  await searchChecks(root, viewer, doc, seen);
  await paletteChecks(viewer, doc.page_count);
  await accessibilityChecks(root, viewer, doc, seen);
  await outlineChecks(viewer, sidebar, doc);
  await thumbnailChecks(root, viewer, sidebar, doc, page);
  await rotationChecks(root, viewer, sidebar, doc, page, seen);
  await invertChecks(viewer, doc, page, seen);
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
    skip("dragging nowhere selects nothing", "there is no text a drag could select");
    skip("a drag selects text from where it was dragged", "the page has no extractable text");
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
  check("Escape clears the selection", viewer.selectedText === "", "nothing selected");

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
    skip("a drag selects text from where it was dragged", "the page has no extractable text");
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

  drag(root, [MID_X, HIGH_Y], [MID_X + 240, HIGH_Y]);
  const high = viewer.selectedText;
  const highAt = whole.indexOf(high);

  drag(root, [MID_X, LOW_Y], [MID_X + 240, LOW_Y]);
  const low = viewer.selectedText;
  const lowAt = whole.indexOf(low);

  const located = high.length > 0 && low.length > 0 && highAt >= 0 && lowAt >= 0;
  check(
    "a drag selects text from where it was dragged",
    located && highAt < lowAt,
    !located
      ? `selected ${high.length} and ${low.length} characters, not both located`
      : `y=${HIGH_Y} gave "${preview(high)}" at ${highAt}; ` +
        `y=${LOW_Y} gave "${preview(low)}" at ${lowAt}` +
        (highAt < lowAt ? "" : " -- the page reads bottom to top, which it does not"),
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
function granularityChecks(root: HTMLElement, viewer: Viewer, whole: string): void {
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
        (/\s/u.test(word) ? " -- which contains whitespace, so it is not one word" : ""),
  );

  click(root, at[0], at[1], 3);
  const line = viewer.selectedText;

  if (word && line === word) {
    // Honest rather than green: on a line holding a single word the two
    // selections are legitimately identical and the check cannot discriminate.
    skip(GRANULARITY_CHECKS[1], "the line under the pointer holds a single word");
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
  for (const candidate of [40, 48, 56, 64, 72, 80, 88, 96, 120, 160, 200, 240]) {
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
    skip(GRANULARITY_CHECKS[2], "no drag distance tried ends inside a word on this page");
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
        (extended.length > charDrag.length ? "" : " -- the double-click changed nothing") +
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
  seen: { status: ViewerStatus | null },
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
    skip("finds a word taken from the document", "page 1 has no extractable text");
    skip("a match covers the characters searched for", "page 1 has no extractable text");
    skip("case is ignored", "page 1 has no extractable text");
    skip("a word that is not there is not found", "page 1 has no extractable text");
    skip("searches forward from the page being read", "page 1 has no extractable text");
    skipSearchOptions("page 1 has no extractable text");
    skip("finds something from the end of the document", "page 1 has no extractable text");
    skip("counts more than the matches on one page", "page 1 has no extractable text");
    skip("Cmd-G moves to a match on another page", "page 1 has no extractable text");
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
    skip("a match covers the characters searched for", "nothing was found to check");
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

/** The three checks {@link searchOptionChecks} records, for the skip path. */
const SEARCH_OPTION_CHECKS = [
  "matching case rejects the hit that ignoring it accepted",
  "whole words rejects a hit inside a longer word",
  "turning the options off finds the hit again",
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
  seen: { status: ViewerStatus | null },
  needle: string,
  firstHit: { page: number; start: number } | undefined,
  pageCount: number,
): Promise<void> {
  const done = (): boolean =>
    !viewer.searching && (seen.status?.search.scanned ?? 0) >= pageCount;
  const hitAtFirst = (): boolean =>
    !!firstHit &&
    viewer.searchMatches.some((m) => m.page === firstHit.page && m.start === firstHit.start);

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
    viewer.setSearchOptions({ matchCase: true, wholeWord: false });
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
  viewer.setSearchOptions({ matchCase: false, wholeWord: true });
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
  viewer.setSearchOptions({ matchCase: false, wholeWord: false });
  viewer.search(stem);
  await settle(done);
  check(
    SEARCH_OPTION_CHECKS[2],
    hitAtFirst(),
    `"${stem}" unrestricted -> ${viewer.searchMatches.length} matches, including ` +
      `${firstHit.page}:${firstHit.start}`,
  );
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
  check(
    name,
    found && !!first && first.page >= from,
    `reading page ${from + 1}, first hit on page ${first ? first.page + 1 : "none"}` +
      (first && first.page < from ? " -- the scan restarted at the beginning" : ""),
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
  await eventually(
    spread,
    () => viewer.searchMatches.some((m) => m.page !== viewer.searchMatches[0]?.page),
    () =>
      `${viewer.searchMatches.length} matches across ` +
      `${new Set(viewer.searchMatches.map((m) => m.page)).size} pages`,
  );

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
    () => `pages present: ${viewer.accessibleText.present.join(", ") || "none"}`,
  );

  const extracted = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const spoken = spokenText(viewer.accessibleText.elementFor(0));

  if (!extracted || extracted.codes.length === 0) {
    check(
      "a page with no text says so rather than falling silent",
      spoken.includes("no extractable text"),
      `reads "${preview(spoken)}"`,
    );
    skip("the text read out is the page's own text", "the page has no extractable text");
  } else {
    skip("a page with no text says so rather than falling silent", "this page has text");
    // Compared against an independent extraction, not against the viewer's
    // cache, so the layer cannot be confirmed by agreeing with itself.
    const expected = flatten(String.fromCodePoint(...extracted.codes));
    check(
      "the text read out is the page's own text",
      spoken === expected,
      spoken === expected
        ? `${spoken.length} characters match the extraction`
        : `reads ${spoken.length} characters, extraction has ${expected.length}: ` +
          `"${preview(spoken)}" vs "${preview(expected)}"`,
    );
  }

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
    skip("focus in the text survives a scroll", "the scroll left the page entirely");
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
    skip("a page that leaves the screen leaves the tree", "the document has one screen");
  } else {
    await eventually(
      "a page that leaves the screen leaves the tree",
      () =>
        !viewer.accessibleText.present.includes(0) &&
        viewer.accessibleText.present.includes(last),
      () => `after End: pages ${viewer.accessibleText.present.join(", ") || "none"}`,
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
  return flatten(
    [...article.querySelectorAll("p")].map((p) => p.textContent ?? "").join(" "),
  );
}

/** Collapses whitespace, so a line break is not a difference in content. */
function flatten(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * The command palette, driven through its own DOM.
 *
 * The registry is built here rather than reusing `App.svelte`'s, because the
 * shell is not mounted --- so what this covers is the palette and the registry,
 * and **not** the command list the application actually registers or the Cmd-K
 * that opens it. Both are wiring in `App.svelte`, and both are unchecked; the
 * ranking underneath is covered by `commands.test.ts`.
 *
 * The load-bearing assertion is that Enter *ran* something: a palette that
 * filters beautifully and does nothing passes every other check here. It carries
 * the control this repository keeps needing --- the viewer is asserted not to be
 * at the end of the document before the command that takes it there.
 */
async function paletteChecks(viewer: Viewer, pageCount: number): Promise<void> {
  const registry = new CommandRegistry();
  registry.register(
    { id: "view.fitWidth", title: "Fit width", keys: "⌘0", run: () => viewer.fitWidth() },
    { id: "nav.lastPage", title: "Go to end", keys: "End", run: () => viewer.goToEnd() },
    { id: "nav.firstPage", title: "Go to start", keys: "Home", run: () => viewer.goToStart() },
    { id: "edit.copy", title: "Copy selection", enabled: () => false, run: () => {} },
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
    skip("Enter runs the highlighted command", "already at the end before running it");
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
        if (!/^[0-9]+$/.test(trimmed)) return `"${trimmed}" is not a page number`;
        const page = Number(trimmed);
        return page < 1 || page > pages ? `This document has ${pages} pages` : null;
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
      !palette.isOpen && went === target - 1 && from !== target - 1 && (atTop || pinned),
      from === target - 1
        ? `already on page ${target} before the jump, so this proves nothing`
        : `typed ${target}, ran with page ${went + 1}, viewer shows page ${viewer.position.page + 1}` +
          ` (from ${from + 1})` +
          (atTop || !pinned ? "" : ", scrolled to the end since it is the last page"),
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

  const treeitems = document.querySelectorAll('.tpdf-sidebar [role="treeitem"]');
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
      check("activating a row goes to its page", false, `no row for ${elsewhere.id}`);
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
): Promise<void> {
  const strip = sidebar.thumbnails;
  if (!strip) {
    check("the sidebar has a tab for pages", false, "no strip was built");
    return;
  }

  const tabs = panelTabs();
  check(
    "the sidebar has a tab for pages",
    tabs.length === 2 && tabs.filter((t) => t.getAttribute("aria-selected") === "true").length === 1,
    `${tabs.length} tabs (${tabs.map((t) => t.textContent).join(", ")}), ` +
      `${tabs.filter((t) => t.getAttribute("aria-selected") === "true").length} selected`,
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
  await yieldChecks(root, viewer, sidebar, doc.page_count, strip);
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
  const across = sideways ? [bounds.left, bounds.right] : [bounds.top, bounds.bottom];
  const along = sideways ? [bounds.top, bounds.bottom] : [bounds.left, bounds.right];
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

  if (!before || before.early.length < 8 || before.late.length < 8) {
    skip(name, "the upright page yielded no two lines to drag out of it");
    return;
  }
  if (before.early === before.late) {
    // A one-line page, or a drag that clamped to the same line twice. Either
    // way the comparison could not tell a rotation from a mirror.
    skip(name, "both samples came from the same line, which cannot distinguish a turn");
    return;
  }

  const after = await sampleLines(root, viewer);
  if (!after || after.page !== before.page) {
    skip(name, `the rotation left page ${(after?.page ?? -1) + 1}, not ${before.page + 1}`);
    return;
  }

  const held = sameLine(after.early, before.early) && sameLine(after.late, before.late);
  const swapped = sameLine(after.early, before.late) && sameLine(after.late, before.early);
  check(
    name,
    held,
    `"${preview(before.early)}" then "${preview(before.late)}" upright; ` +
      `"${preview(after.early)}" then "${preview(after.late)}" turned` +
      (held ? "" : swapped ? " -- the two swapped, i.e. the turn went the wrong way" : ""),
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
async function rotatedTextLayerCheck(viewer: Viewer, doc: DocumentInfo): Promise<void> {
  const name = "the text layer turns with the view";
  const at = viewer.position.page;
  const shown = viewer.textOn(at);
  if (!shown || shown.codes.length === 0) {
    skip(name, "the page has no extractable text");
    return;
  }

  // From the backend, so the comparison is against the document rather than
  // against the same cache being checked.
  const raw = await invoke<{ quarter_turns: number; width_pt: number }>("page_text", {
    doc: doc.id,
    page: at,
  }).catch(() => null);
  if (!raw) {
    skip(name, "the page's text could not be fetched a second time");
    return;
  }

  const wanted = (raw.quarter_turns + viewer.rotation) % 4;
  const swapped = viewer.rotation % 2 === 1;
  check(
    name,
    shown.quarter_turns === wanted &&
      (swapped ? shown.width_pt !== raw.width_pt : shown.width_pt === raw.width_pt),
    `page is /Rotate ${raw.quarter_turns * 90}, view turned ${viewer.rotation * 90}: ` +
      `the text layer reports /Rotate ${shown.quarter_turns * 90} ` +
      `(wanted ${wanted * 90}) on a ${shown.width_pt.toFixed(0)} pt wide page ` +
      `(the document says ${raw.width_pt.toFixed(0)})`,
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
async function tileBytes(req: Parameters<typeof tileUrl>[0]): Promise<Uint8ClampedArray | null> {
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

  const [plainPixels, darkPixels] = await Promise.all([request(false), request(true)]).catch(
    () => [null, null],
  );
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
    skip(exact, "the two tiles are identical, so there is nothing to compare against");
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
      skip(name, "this layout composites per tile, so there is no single surface");
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
      skip(name, `the page is already dark at ${(before * 100).toFixed(0)}% lightness`);
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
  const core = (text: string): string => text.slice(2, -2);
  return core(a).length >= 8 && (b.includes(core(a)) || a.includes(core(b)));
}

/** The box enclosing every character that has one, in the view's own space. */
function inkBounds(
  text: { boxes: number[] },
): { left: number; top: number; right: number; bottom: number } | null {
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
  return [...document.querySelectorAll<HTMLElement>('.tpdf-sidebar [role="tab"]')];
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
    candidates.filter((page) => page < doc.page_count - 1).pop() ?? candidates.pop();
  const element = target === undefined ? null : strip.elementFor(target);
  if (target === undefined || !element) {
    skip(name, `no row other than page ${here + 1} is currently built`);
    return;
  }

  element.focus();
  key(element, "Enter");
  await settle(() => viewer.position.page === target);
  check(
    name,
    viewer.position.page === target,
    `from page ${here + 1} to ${viewer.position.page + 1}, wanted ${target + 1}`,
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
  const yields = "the strip withdraws its work when the viewer needs the renderer";
  const quiet = "a hidden strip asks for nothing";
  const resumes = "and starts again when it is shown";

  /** Waits to catch a thumbnail in flight, which every check here needs. */
  const caught = async (): Promise<boolean> => {
    const deadline = performance.now() + 3000;
    while (!strip.outstanding && performance.now() < deadline) await frame();
    return strip.outstanding;
  };

  const why = "no thumbnail stayed in flight long enough to collide with anything";
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
  const words = String.fromCodePoint(...codes.slice(0, 4096)).match(/[A-Za-z]{5,}/g);
  if (!words?.length) return null;
  const lead = words[0]?.toLowerCase();
  return words.find((word) => word.toLowerCase() !== lead) ?? words[0] ?? null;
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

async function printChecks(path: string, doc: DocumentInfo): Promise<void> {
  const print = async (pages: number[] | null, turns: number): Promise<string> => {
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
