/**
 * What every phase shares: the report, `check` and `skip`, waiting for the viewer to settle, and synthetic wheel, pointer and key input.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { Report, frame, settle as settleFor } from "../checkreport";
import { MULTI_CLICK_SLOP_PX } from "../clicks";

/**
 * Tabs the sidebar has: outline, pages, results, comments, marks, redactions,
 * text the pages do not show.
 *
 * Spelled out here rather than read from the sidebar, which would make the check
 * agree with whatever the sidebar happens to build. It went red on its own when
 * the results tab landed and again when the comments tab did, which is the check
 * working --- twice. Three times: the marks tab made it five. Four: the
 * redactions tab made it six, and that red was not read for a week, because
 * nothing runs this harness automatically and the mutation table is the only
 * thing that runs its baseline. Five: the tab for text the pages do not show
 * made it seven, and the number was changed with the tab, without a window run.
 */
export const SIDEBAR_TABS = 7;

/** Size of the surface the check mounts, in CSS pixels. */
export const WIDTH = 900;
export const HEIGHT = 700;

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
export const report = new Report();

export const check = (name: string, ok: boolean, detail: string): void =>
  report.check(name, ok, detail);

/**
 * Records a check that this document cannot exercise.
 *
 * Printed rather than omitted. A control that quietly disappears on some inputs
 * is indistinguishable from one that ran, and the whole point of a control is
 * to know whether it did.
 */
export const skip = (name: string, why: string): void => report.skip(name, why);

/**
 * Records a check that is satisfied by waiting, failing it on a timeout.
 *
 * A timeout is a failure of the thing being checked rather than a slow machine:
 * every wait here is for something that either happens within a second or is
 * broken. It is recorded rather than thrown so one broken behaviour does not
 * hide the state of the rest.
 */
export async function eventually(
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
export async function settle(predicate: () => boolean): Promise<void> {
  await settleFor(predicate, TIMEOUT_MS);
}

/** Dispatches a wheel event the way a trackpad would. */
export function wheel(root: HTMLElement, deltaY: number, zoomGesture = false): void {
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
export function pointer(
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
export function drag(
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
export function click(root: HTMLElement, x: number, y: number, times: number): void {
  const away = y + 4 * MULTI_CLICK_SLOP_PX;
  pointer(root, "pointerdown", x, away);
  pointer(root, "pointerup", x, away);
  for (let n = 0; n < times; n++) {
    pointer(root, "pointerdown", x, y);
    pointer(root, "pointerup", x, y);
  }
}

/** Dispatches a keydown the way the window would. */
export function key(root: HTMLElement, k: string, accel = false): void {
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
 * What was thrown, in a form that names it.
 *
 * `String(e)` was here, and for the two things this actually catches it says
 * nothing: a Tauri `invoke` rejects with a plain object, so the whole run's
 * only line read `[FAIL] run completed  [object Object]`. That is a diagnostic
 * that names no cause, which sends a reader to bisect their own change --- and
 * the one that produced it was a rejection from a command, which the object
 * carries and `String` throws away.
 *
 * An `Error` keeps its stack, because when the throw is ours rather than the
 * backend's the line number is the whole answer.
 */
export function describeThrown(e: unknown): string {
  if (e instanceof Error) return e.stack ?? `${e.name}: ${e.message}`;
  if (typeof e === "string") return e;
  try {
    return JSON.stringify(e) ?? String(e);
  } catch {
    return String(e);
  }
}
