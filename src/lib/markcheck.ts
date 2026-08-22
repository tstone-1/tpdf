/**
 * A functional check of the reader's own marks, inside the real application.
 *
 * ## Why this exists, and it is not thoroughness
 *
 * On 2026-08-22 a reader reported that a shape drawn on a page vanished when
 * they let go of the button. It did. `Viewer.onDrawn` reported the page's *id*
 * and `Edits.mark` read that number as a *slot*, so a mark was written to the
 * next page along --- and on the last page, where there is no next slot, to
 * nowhere at all, with no command sent and no refusal shown.
 *
 * **Every gate was green, at four layers.** `edits.test.ts` asserts that the
 * model call sends an id, and it did. `viewerdraw.test.ts` asserts that the
 * viewer reports an id, and it did. `viewer_check.py` drives a real window ---
 * and builds its own `Viewer` with no model behind it, so a drag draws its
 * preview and commits nothing. The defect was in none of those halves. It was
 * in the *join*, which is an object literal in `App.svelte`: a file no unit test
 * imports, no window harness constructs, and nothing had ever executed with a
 * document open.
 *
 * So this check exists to run the route a reader takes and nothing else:
 *
 * > command → gesture on the real viewer → callback → edit model → overlay
 *
 * It asserts against **the model**, never against the viewer that produced the
 * gesture. Reading the answer back out of the thing under test is the
 * writer-agreeing-with-its-own-reader failure this repository has been caught by
 * twice; here the two ends are genuinely independent, because the model is in
 * Rust and heard about the mark over an IPC boundary.
 *
 * ## The one assertion that would have caught it
 *
 * *"and it is recorded on the page it was pressed on"* derives the expected page
 * **id** from the model's own page list and the viewer's own slot, and compares
 * it against the id the model filed the mark under. Under the shipped defect
 * those differ by one on every document, which is why a fixture of any length
 * catches it --- and on the last page there is no mark at all, which the check
 * above it says.
 *
 * ## What it deliberately does not do
 *
 * It does not check what a mark *looks like*: `viewer_check.py`'s overlay phase
 * measures ink per kind against a mark handed straight to `setMarks`, and
 * `annot-probe` measures the saved file's pixels. One ink reading is here, and
 * its job is only to say that a mark the model has reaches the screen --- the
 * last hop of the chain, which a model assertion cannot see.
 */

import { invoke } from "@tauri-apps/api/core";

import { pause, Report, settle as settleFor } from "./checkreport";
import type { MarkView, PageView } from "./pages";
import type { Viewer } from "./viewer";

/** How long any single wait may take before the check gives up. */
const TIMEOUT_MS = 30_000;

/**
 * How long to wait before deciding that *no* mark was made.
 *
 * A negative assertion cannot settle on a condition --- there is nothing to wait
 * for --- so it waits out the round trip instead. Generous, because what is
 * being ruled out is a command reaching Rust and coming back, and a check that
 * declared "nothing happened" before the reply landed would be a control that
 * passes whatever the code does.
 */
const QUIET_MS = 1_500;

const report = new Report();

const settle = (predicate: () => boolean) => settleFor(predicate, TIMEOUT_MS);

const check = (name: string, ok: boolean, detail: string) => report.check(name, ok, detail);

/** What the check needs from the application, in the application's own terms. */
export interface MarkCheckHost {
  /**
   * Runs a command through the registry, exactly as the palette does.
   *
   * **Not the underlying action.** What is being tested is the chain a reader
   * sets off, and the registry is its first link --- a check that called
   * `armDraw` directly would skip the enablement guard, the command's own
   * argument handling and the wiring in between, which is three quarters of
   * what can be wrong.
   */
  run: (id: string) => boolean;
  /** The live viewer, for where things are on screen and for the overlay. */
  viewer: () => Viewer | null;
  /**
   * The element the viewer was built on, which is where a press has to land.
   *
   * Handed over rather than read off the viewer, exactly as `sessioncheck.ts`
   * takes it: the viewer does not expose its own root, and adding an accessor so
   * that a harness can reach around to the element the application already holds
   * would widen the public surface for no one else's benefit.
   */
  root: () => HTMLElement | null;
  /** The marks the **model** holds. The independent end of every assertion. */
  marks: () => readonly MarkView[];
  /** The pages the model holds, so an expected page id can be derived. */
  pages: () => readonly PageView[];
  /** The path that is open, so the check can say it had a document. */
  path: () => string;
}

/**
 * Runs the check if `TPDF_MARKCHECK` is set, then exits the process.
 *
 * Returns `false` when it was not requested, so the caller carries on into the
 * real application. Called at the end of the boot, after a document handed over
 * on the command line has opened --- the check needs one and does not open it
 * itself, for `sessioncheck.ts`'s reason: the open is part of the application
 * and a harness that replaced it would be testing something else.
 */
export async function runMarkCheckIfRequested(host: MarkCheckHost): Promise<boolean> {
  const mode = await invoke<string | null>("markcheck_mode");
  if (!mode) return false;

  try {
    await run(host);
  } catch (e) {
    report.check("the check ran", false, String(e));
  }

  await report.finish();
  return true;
}

/** A press and release at one point, with no movement between them. */
function click(root: HTMLElement, x: number, y: number): void {
  root.dispatchEvent(
    new PointerEvent("pointerdown", {
      button: 0,
      pointerId: 1,
      clientX: x,
      clientY: y,
      bubbles: true,
    }),
  );
  root.dispatchEvent(
    new PointerEvent("pointerup", { pointerId: 1, clientX: x, clientY: y, bubbles: true }),
  );
}

/** A press, a move and a release --- the gesture every shape tool reads. */
function drag(
  root: HTMLElement,
  from: { x: number; y: number },
  to: { x: number; y: number },
): void {
  root.dispatchEvent(
    new PointerEvent("pointerdown", {
      button: 0,
      pointerId: 1,
      clientX: from.x,
      clientY: from.y,
      bubbles: true,
    }),
  );
  root.dispatchEvent(
    new PointerEvent("pointermove", {
      pointerId: 1,
      clientX: to.x,
      clientY: to.y,
      bubbles: true,
    }),
  );
  root.dispatchEvent(
    new PointerEvent("pointerup", {
      pointerId: 1,
      clientX: to.x,
      clientY: to.y,
      bubbles: true,
    }),
  );
}

/** The ids in the model's page list, in reading order. */
const idsOf = (pages: readonly PageView[]) => pages.map((page) => page.id);

/** The mark the model gained since `before`, or `null` for none. */
function added(before: ReadonlySet<number>, now: readonly MarkView[]): MarkView | null {
  return now.find((mark) => !before.has(mark.id)) ?? null;
}

/**
 * The fraction of a rectangle on the overlay that carries any of our ink.
 *
 * The alpha channel rather than a colour, for `viewercheck.ts`'s reason: the
 * overlay is transparent where nothing was painted, and keying on a particular
 * colour would make this a check on the palette instead.
 */
function inked(viewer: Viewer, box: { left: number; top: number; right: number; bottom: number }): number | null {
  const canvas = viewer.overlaySurface;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  const dpr = window.devicePixelRatio || 1;
  const x0 = Math.max(0, Math.round(box.left * dpr));
  const y0 = Math.max(0, Math.round(box.top * dpr));
  const x1 = Math.min(canvas.width, Math.round(box.right * dpr));
  const y1 = Math.min(canvas.height, Math.round(box.bottom * dpr));
  if (x1 - x0 < 2 || y1 - y0 < 2) return null;
  const { data } = ctx.getImageData(x0, y0, x1 - x0, y1 - y0);
  let hit = 0;
  for (let at = 3; at < data.length; at += 4) {
    if ((data[at] ?? 0) > 8) hit++;
  }
  return hit / (data.length / 4);
}

async function run(host: MarkCheckHost): Promise<void> {
  const opened = await settle(() => host.path() !== "" && host.viewer() !== null);
  check(
    "a document is open to put a mark on",
    opened,
    opened ? `${host.pages().length} page(s)` : "nothing opened",
  );
  if (!opened) return;

  const viewer = host.viewer();
  const root = host.root();
  if (!viewer || !root) {
    check("the viewer hands over a surface to press on", false, "no surface");
    return;
  }
  await settle(() => viewer.idle);

  // Mid-window, well inside whatever page is on screen. Nothing here depends on
  // the point being over text: a comment is dropped on the paper and a box is
  // dragged across it, which is exactly why these tools have no `hasSelection`
  // guard.
  const bounds = root.getBoundingClientRect();
  const mid = { x: bounds.left + bounds.width * 0.4, y: bounds.top + bounds.height * 0.4 };

  // --- The control, and it goes first ------------------------------------
  //
  // Every assertion below says "a mark appeared". Without this, all of them are
  // satisfied by an application that makes a mark on any press at all --- and
  // the press position is shared, so this is the same press.
  {
    const before = new Set(host.marks().map((mark) => mark.id));
    click(root, mid.x, mid.y);
    await pause(QUIET_MS);
    const stray = added(before, host.marks());
    check(
      "a press with nothing armed marks nothing",
      stray === null,
      stray ? `made a ${stray.kind}` : `${host.marks().length} mark(s), unchanged`,
    );
  }

  // --- A comment, placed by a press --------------------------------------
  {
    const armed = host.run("edit.addComment");
    check("the add-comment command runs and arms the pointer", armed && viewer.drawArmed === "note", `armed: ${viewer.drawArmed}`);

    const slot = viewer.position.page;
    const before = new Set(host.marks().map((mark) => mark.id));
    click(root, mid.x, mid.y);

    const landed = await settle(() => added(before, host.marks()) !== null);
    const mark = added(before, host.marks());
    check(
      "a comment placed by a press reaches the model",
      landed && mark?.kind === "note",
      mark ? `mark ${mark.id}, a ${mark.kind}` : "the model gained nothing",
    );

    if (mark) {
      // **The assertion the whole file is for.** The expected id comes from the
      // model's own page list at the viewer's own slot; the actual id is what
      // the model filed the mark under. Two ends that only agree if every hop
      // between them carried the same page --- and under the defect this check
      // was written for they differ by one on every document there is.
      const want = host.pages()[slot]?.id;
      check(
        "and it is recorded on the page it was pressed on",
        want !== undefined && mark.page === want,
        `slot ${slot} is page ${want}; the mark says ${mark.page} (ids: ${idsOf(host.pages()).slice(0, 6).join(", ")})`,
      );
    } else {
      report.skip(
        "and it is recorded on the page it was pressed on",
        "no mark was made, so there is no page to compare",
      );
    }
  }

  // --- A shape on the last page ------------------------------------------
  //
  // The case that produced no mark at all rather than one in the wrong place:
  // the id of the last page is one past the end of the slot list, so the lookup
  // that should not have been there answered `undefined` and the method returned
  // without sending anything. A one-page document exercises it too --- which is
  // most of what a reader opens, and is why the defect was reported so quickly.
  {
    const last = host.pages().length - 1;
    viewer.goToPage(last);
    await settle(() => viewer.idle);

    const armed = host.run("edit.drawBox");
    check("the draw-box command runs and arms the pointer", armed && viewer.drawArmed === "square", `armed: ${viewer.drawArmed}`);

    const slot = viewer.position.page;
    const before = new Set(host.marks().map((mark) => mark.id));
    drag(root, { x: mid.x, y: mid.y }, { x: mid.x + 120, y: mid.y + 90 });

    const landed = await settle(() => added(before, host.marks()) !== null);
    const mark = added(before, host.marks());
    check(
      "a shape drawn on the last page reaches the model",
      landed && mark?.kind === "square",
      mark
        ? `mark ${mark.id} on page ${mark.page}`
        : `nothing was made on slot ${slot} of ${host.pages().length}`,
    );

    if (mark) {
      const want = host.pages()[slot]?.id;
      check(
        "and that one names the last page too",
        want !== undefined && mark.page === want,
        `slot ${slot} is page ${want}; the mark says ${mark.page}`,
      );

      // The last hop, and the only one a model assertion cannot see: a mark the
      // model holds has to reach the screen. Read over the viewer's own anchor
      // for the mark, so a rotated or cropped page is sampled where the mark
      // actually is rather than where an independent derivation thinks it is.
      await settle(() => viewer.idle);
      const anchor = viewer.markAnchor(mark.id);
      const ink = anchor ? inked(viewer, anchor) : null;
      if (ink === null) {
        report.skip(
          "and the overlay draws it",
          anchor ? "the overlay has no readable 2d context" : "the mark has no anchor",
        );
      } else {
        check(
          "and the overlay draws it",
          ink > 0.01,
          `${(ink * 100).toFixed(1)}% of the mark's own rectangle is inked`,
        );
      }

      // --- Moving it, and undoing that ------------------------------------
      const home = [...mark.quads];
      const centre = anchor
        ? { x: (anchor.left + anchor.right) / 2, y: (anchor.top + anchor.bottom) / 2 }
        : mid;
      drag(root, centre, { x: centre.x - 60, y: centre.y - 40 });

      const moved = await settle(() => {
        const now = host.marks().find((item) => item.id === mark.id);
        return now !== undefined && now.quads[0] !== home[0];
      });
      const now = host.marks().find((item) => item.id === mark.id);
      check(
        "a mark dragged across the page moves in the model",
        moved && now !== undefined,
        now ? `left ${home[0]?.toFixed(1)} -> ${now.quads[0]?.toFixed(1)}` : "the mark is gone",
      );

      if (moved) {
        check(
          "and undo puts it back where it was",
          host.run("edit.undo") &&
            (await settle(() => {
              const back = host.marks().find((item) => item.id === mark.id);
              return back !== undefined && back.quads[0] === home[0];
            })),
          `back to ${host.marks().find((item) => item.id === mark.id)?.quads[0]?.toFixed(1)} from ${home[0]?.toFixed(1)}`,
        );
      } else {
        report.skip("and undo puts it back where it was", "nothing moved, so there is nothing to undo");
      }
    } else {
      for (const name of [
        "and that one names the last page too",
        "and the overlay draws it",
        "a mark dragged across the page moves in the model",
        "and undo puts it back where it was",
      ]) {
        report.skip(name, "no mark was made, so there is nothing to look at");
      }
    }
  }
}
