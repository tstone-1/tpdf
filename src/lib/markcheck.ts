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

import { call } from "./ipc";

import { pause, Report, settle as settleFor } from "./checkreport";
import type { MarkView, PageView } from "./pages";
import type { Viewer } from "./viewer";
import { isMac } from "./keys";

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
  /** Queues the file the next picture panel answers with, as the panel would. */
  answerPictures: (paths: string[]) => void;
  /** A picture's pixels, read back from the model over the IPC boundary. */
  picturePixels: (id: number) => Promise<Uint8Array>;
  /** The kinds the mark clipboard holds, so a paste waits for its copy. */
  held: () => readonly string[];
  /** The message the window shows, if any: what a refused paste said. */
  said: () => string | null;
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
  const mode = await call("markcheck_mode");
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
      // The anchor is measured from the viewer's own corner, which is the space
      // the overlay is sampled in above, and a press is given in the window's.
      // The viewer's corner is added here. Until 2026-10-09 it was not, so the
      // press landed above the box by the height of everything over the page:
      // inside it while that was less than half the box, and outside it, moving
      // nothing, once the toolbar and the row of tabs were there.
      const corner = root.getBoundingClientRect();
      const centre = anchor
        ? {
            x: corner.left + (anchor.left + anchor.right) / 2,
            y: corner.top + (anchor.top + anchor.bottom) / 2,
          }
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
        now
          ? `left ${home[0]?.toFixed(1)} -> ${now.quads[0]?.toFixed(1)}; the page area starts ${corner.top.toFixed(0)} px down the window`
          : "the mark is gone",
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

  // --- An arrow by a drag, and a polygon a corner per click ---------------
  //
  // The two gestures the four shapes of straight lines use, through their
  // commands. What reaches the model is the kind and its points, which the
  // viewer sends as strokes rather than as a rectangle.
  {
    const low = { x: bounds.left + bounds.width * 0.3, y: bounds.top + bounds.height * 0.7 };
    const armed = host.run("edit.drawArrow");
    check("the add-arrow command runs and arms the pointer", armed && viewer.drawArmed === "arrow", `armed: ${viewer.drawArmed}`);
    let before = new Set(host.marks().map((mark) => mark.id));
    drag(root, low, { x: low.x + 150, y: low.y + 40 });
    await settle(() => added(before, host.marks()) !== null);
    const arrow = added(before, host.marks());
    check(
      "an arrow dragged out reaches the model as its two ends",
      arrow?.kind === "arrow" && arrow.strokes.length === 1 && arrow.strokes[0]?.length === 4,
      arrow ? `a ${arrow.kind} with strokes of ${arrow.strokes.map((one) => one.length / 2).join(", ")} point(s)` : "the model gained nothing",
    );
    await settle(() => viewer.idle);
    const anchor = arrow ? viewer.markAnchor(arrow.id) : null;
    const ink = anchor ? inked(viewer, anchor) : null;
    if (ink === null) report.skip("and the overlay draws the arrow", "the arrow has no anchor or no readable overlay");
    else check("and the overlay draws the arrow", ink > 0.005, `${(ink * 100).toFixed(1)}% of its rectangle is inked`);

    check("the add-polygon command runs and arms the pointer",
      host.run("edit.drawPolygon") && viewer.drawArmed === "polygon", `armed: ${viewer.drawArmed}`);
    before = new Set(host.marks().map((mark) => mark.id));
    const at = { x: low.x, y: low.y - 160 };
    click(root, at.x, at.y);
    click(root, at.x + 120, at.y);
    click(root, at.x + 60, at.y + 90);
    await pause(QUIET_MS);
    const early = added(before, host.marks());
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await settle(() => added(before, host.marks()) !== null);
    const polygon = added(before, host.marks());
    check(
      "a polygon placed a corner per click reaches the model on Enter, and not before",
      early === null && polygon?.kind === "polygon" && polygon.strokes[0]?.length === 6,
      polygon
        ? `a ${polygon.kind} of ${(polygon.strokes[0]?.length ?? 0) / 2} corner(s)${early ? ", made before Enter" : ""}`
        : "the model gained nothing",
    );

    // --- Copy, cut and paste --------------------------------------------
    //
    // Through the commands, which name the mark whose note is open, and the
    // paste back through the ordinary add path. On the page the marks came
    // from a paste lands a step down and to the right of them.
    if (arrow) {
      viewer.showMark(arrow.id);
      const copied = await settle(() => viewer.markOpen === arrow.id) && host.run("edit.copyMark");
      before = new Set(host.marks().map((mark) => mark.id));
      const pasted = copied && host.run("edit.pasteMark");
      await settle(() => added(before, host.marks()) !== null);
      const copy = added(before, host.marks());
      const from = arrow.strokes[0] ?? [];
      const to = copy?.strokes[0] ?? [];
      const step = to.map((value, at) => value - (from[at] ?? 0));
      check(
        "a copied arrow pastes as an arrow a step down and to the right",
        pasted && copy?.kind === "arrow" && step.length === 4 && step.every((delta) => Math.abs(delta - 12) < 0.01),
        copy ? `a ${copy.kind}, moved by ${step.map((delta) => delta.toFixed(2)).join(", ")}` : "the model gained nothing",
      );
    } else {
      report.skip("a copied arrow pastes as an arrow a step down and to the right", "no arrow was made");
    }
    if (polygon) {
      viewer.showMark(polygon.id);
      const cut = await settle(() => viewer.markOpen === polygon.id) && host.run("edit.cutMark");
      const gone = cut && (await settle(() => !host.marks().some((mark) => mark.id === polygon.id)));
      before = new Set(host.marks().map((mark) => mark.id));
      const pasted = gone && host.run("edit.pasteMark");
      await settle(() => added(before, host.marks()) !== null);
      const back = added(before, host.marks());
      check(
        "a cut polygon leaves the page, and pastes back as a polygon",
        gone && pasted && back?.kind === "polygon" && back.strokes[0]?.length === 6,
        back ? `a ${back.kind} of ${(back.strokes[0]?.length ?? 0) / 2} corner(s)` : gone ? "nothing pasted" : "the cut removed nothing",
      );
    } else {
      report.skip("a cut polygon leaves the page, and pastes back as a polygon", "no polygon was made");
    }

    // --- A link, and where it goes ---------------------------------------
    //
    // Dragged like a box; its target is typed in the box that opens on it,
    // and reaches the model as the link's note, which a save reads.
    check("the add-link command runs and arms the pointer",
      host.run("edit.addLink") && viewer.drawArmed === "link", `armed: ${viewer.drawArmed}`);
    before = new Set(host.marks().map((mark) => mark.id));
    const corner = { x: low.x + 200, y: low.y - 120 };
    drag(root, corner, { x: corner.x + 100, y: corner.y + 30 });
    await settle(() => added(before, host.marks()) !== null);
    const link = added(before, host.marks());
    const opened = link !== null && (await settle(() => viewer.markOpen === link.id));
    if (opened) {
      const field = viewer.markNoteField;
      field.value = "https://example.com/";
      field.dispatchEvent(new Event("input", { bubbles: true }));
      viewer.closeMark();
    }
    const target = link && (await settle(() => host.marks().find((mark) => mark.id === link.id)?.note === "https://example.com/"));
    check(
      "a link's target, typed in its box, reaches the model as its note",
      link?.kind === "link" && opened && target === true,
      link ? `a ${link.kind}${opened ? "" : " whose box did not open"}; note ${JSON.stringify(host.marks().find((mark) => mark.id === link.id)?.note)}` : "the model gained nothing",
    );

    // --- A picture from a file -------------------------------------------
    //
    // The app's own 128 pixel icon, tracked beside the fixture, chosen through
    // the panel's queue and decoded by a worker. What reaches the model is
    // its size; the pixels come back through their own command.
    const icon = host.path().replace(/testdata[\\/][^\\/]*$/, "src-tauri/icons/128x128.png");
    host.answerPictures([icon]);
    const chosen = host.run("edit.addPicture") && (await settle(() => viewer.drawArmed === "picture"));
    check("the add-picture command decodes the file and arms the placement", chosen,
      `armed: ${viewer.drawArmed} for ${icon}`);
    before = new Set(host.marks().map((mark) => mark.id));
    const spot = { x: low.x + 40, y: low.y - 260 };
    if (chosen) drag(root, spot, { x: spot.x + 120, y: spot.y + 60 });
    await settle(() => added(before, host.marks()) !== null);
    const picture = added(before, host.marks());
    const [left, top, right, bottom] = picture?.quads ?? [];
    const square = picture !== null && left !== undefined && Math.abs((right! - left) - (bottom! - top!)) < 0.01;
    check(
      "a picture is placed keeping its proportions, and the model holds its size and not its pixels",
      picture?.kind === "picture" && picture.picture?.width === 128 && picture.picture.height === 128
        && picture.image === undefined && square,
      picture ? `a ${picture.kind} of ${JSON.stringify(picture.picture)} over ${picture.quads.map((v) => v.toFixed(1)).join(", ")}` : "the model gained nothing",
    );
    const pixels = picture ? await host.picturePixels(picture.id).catch(() => new Uint8Array()) : new Uint8Array();
    check(
      "its pixels come back whole, with something to see",
      pixels.length === 128 * 128 * 4 && pixels.some((value, at) => at % 4 === 3 && value > 0),
      `${pixels.length} bytes`,
    );
    // The overlay draws a picture from pixels it fetches after the mark
    // arrives, and an outline until then. The icon covers most of its square
    // and a dashed outline a few percent of it, so a quarter tells them apart.
    const shown = picture ? viewer.markAnchor(picture.id) : null;
    const covered = shown !== null && (await settle(() => (inked(viewer, shown) ?? 0) > 0.25));
    check(
      "and the overlay draws the picture, not only its outline",
      covered,
      shown ? `${((inked(viewer, shown) ?? 0) * 100).toFixed(1)}% of its box inked` : "the picture has no anchor",
    );
    if (picture) {
      viewer.showMark(picture.id);
      const copied = await settle(() => viewer.markOpen === picture.id) && host.run("edit.copyMark")
        && (await settle(() => host.held().includes("picture")));
      before = new Set(host.marks().map((mark) => mark.id));
      const pasted = copied && host.run("edit.pasteMark");
      await settle(() => added(before, host.marks()) !== null);
      const copy = added(before, host.marks());
      const same = copy ? await host.picturePixels(copy.id).catch(() => new Uint8Array()) : new Uint8Array();
      check(
        "a copied picture pastes as the same picture",
        pasted && copy?.kind === "picture" && same.length === pixels.length && same.every((value, at) => value === pixels[at]),
        copy ? `a ${copy.kind} of ${same.length} bytes` : copied ? "nothing pasted" : "the copy held no picture",
      );
    } else {
      report.skip("a copied picture pastes as the same picture", "no picture was placed");
    }

    // --- A picture from the clipboard --------------------------------------
    //
    // `mark_check.py` puts the same icon on the clipboard before the launch.
    // The command and ⌘V both send AppKit's `paste:` (`paste` in
    // `commands/menubar.rs`), WebKit fires a paste event holding a PNG, and a
    // worker decodes it. WebKit writes the PNG itself, so the pixels are held
    // to the size and not to the file's bytes. Windows and Linux paste on the
    // key in the web view, which nothing here drives.
    if (!isMac()) {
      report.skip("the paste-picture command takes the picture on the clipboard", "macOS only: elsewhere the web view pastes on Ctrl+V");
      report.skip("and so does Cmd-V on the page", "macOS only");
    } else {
      const fromClipboard = async (how: string, arm: () => boolean) => {
        const armed = arm() && (await settle(() => viewer.drawArmed === "picture"));
        let placed: MarkView | null = null;
        if (armed) {
          before = new Set(host.marks().map((mark) => mark.id));
          const at = { x: low.x + 220, y: low.y - 260 };
          drag(root, at, { x: at.x + 80, y: at.y + 40 });
          await settle(() => added(before, host.marks()) !== null);
          placed = added(before, host.marks());
        }
        const got = placed ? await host.picturePixels(placed.id).catch(() => new Uint8Array()) : new Uint8Array();
        check(
          how,
          placed?.kind === "picture" && placed.picture?.width === 128 && placed.picture.height === 128
            && got.length === 128 * 128 * 4 && got.some((value, at) => at % 4 === 3 && value > 0),
          placed ? `a ${placed.kind} of ${JSON.stringify(placed.picture)}, ${got.length} bytes`
            : `armed: ${viewer.drawArmed}; said: ${host.said()}`,
        );
      };
      await fromClipboard("the paste-picture command takes the picture on the clipboard",
        () => host.run("edit.pastePicture"));
      await fromClipboard("and so does Cmd-V on the page", () => {
        root.focus();
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "v", code: "KeyV", metaKey: true, bubbles: true, cancelable: true }));
        return true;
      });
      // The same ⌘V in a text field pastes text: before it was sent, ⌘V
      // pasted nothing in one, as a web view does in an application whose menu
      // has no Paste item. `mark_check.py` put the text beside the picture. A
      // field of the check's own, so the find bar's state is not touched.
      const field = document.createElement("input");
      document.body.append(field);
      field.focus();
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "v", code: "KeyV", metaKey: true, bubbles: true, cancelable: true }));
      const pasted = await settle(() => field.value === "pasted by Cmd-V");
      check("and Cmd-V in a text field pastes text", pasted, `the field holds ${JSON.stringify(field.value)}`);
      field.remove();
    }
  }
}
