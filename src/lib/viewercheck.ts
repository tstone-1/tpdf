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
import { CommandRegistry } from "./commands";
import { allRows, isNavigable, type Outline, type Row } from "./outline";
import { Palette } from "./palette";
import { Sidebar } from "./sidebar";
import { fetchRequiredTile } from "./tiles";
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
  await paletteChecks(viewer);
  await accessibilityChecks(root, viewer, doc, seen);
  await outlineChecks(viewer, sidebar, doc);
  await thumbnailChecks(root, viewer, sidebar, doc, page);
  await rotationChecks(root, viewer, sidebar, doc, page, seen);

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

  await searchesFromHere(root, viewer, seen, needle, doc.page_count);
  await stepToAnotherPage(root, viewer, seen, needle, doc.page_count);
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
  if (pageCount < 2) {
    skip(name, "the document has one page");
    return;
  }

  key(root, "End");
  await settle(() => viewer.idle);
  const from = (seen.status?.page ?? 1) - 1;
  if (from === 0) {
    skip(name, "the whole document fits on one screen");
    return;
  }

  viewer.search(needle);
  const found = await eventually(
    "finds something from the end of the document",
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
async function paletteChecks(viewer: Viewer): Promise<void> {
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

  palette.destroy();
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
