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
import { frame } from "./checkreport";
import type { DocumentInfo } from "./ipc";
import type { WebTarget } from "./outline";
import type { RegionPlan } from "./pages";
import { Sidebar } from "./sidebar";
import { type ViewerStatus, Viewer } from "./viewer";
import {
  check,
  describeThrown,
  eventually,
  HEIGHT,
  key,
  report,
  settle,
  skip,
  wheel,
  WIDTH,
} from "./viewercheck/common";
import { fitChecks, readingChecks, selectionChecks } from "./viewercheck/reading";
import { spreadChecks } from "./viewercheck/spreads";
import { mappingChecks, resultsChecks, searchChecks } from "./viewercheck/search";
import { accessibilityChecks, crossPageChecks, scopedSearchChecks } from "./viewercheck/accessibility";
import { appCommandChecks, paletteChecks } from "./viewercheck/commands";
import {
  commentChecks,
  markColorChecks,
  markNoteChecks,
  markPanelChecks,
} from "./viewercheck/marks";
import { propertiesChecks } from "./viewercheck/properties";
import { linkChecks, outlineChecks } from "./viewercheck/links";
import { rotationChecks, thumbnailChecks } from "./viewercheck/thumbnails";
import { pageDeletionChecks, pageMoveChecks, pageRotationChecks } from "./viewercheck/pages";
import { invertChecks } from "./viewercheck/invert";
import { geometryChecks, releaseChecks } from "./viewercheck/geometry";
import { markAgreementChecks, overlayInkChecks } from "./viewercheck/ink";
import {
  cropCommandChecks,
  markCommandChecks,
  printChecks,
  validationDataChecks,
} from "./viewercheck/editing";

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
    check("run completed", false, describeThrown(e));
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
  const markEdits: string[] = [];
  /** Every reorder the page strip asked for, in order. */
  const drags: [number, number][] = [];
  /** Every right-click the page strip reported, as the slot it landed on. */
  const menus: number[] = [];
  /** Every mark the panel's remove control asked to take off. */
  const marksRemoved: number[] = [];
  /**
   * What the panel is told each mark covers, by id.
   *
   * The application fills this from the selection that made the mark; here the
   * marks are synthetic, so a phase writes the words it wants to see and reads
   * the row back. What that leaves untested is `markSelection`'s recording,
   * which this harness cannot reach at all --- the command probe stubs it.
   */
  const marksCovered = new Map<number, string>();
  /** Every region the redactions panel's remove control asked to take off. */
  const regionsRemoved: number[] = [];
  /** Every region a row press reported, in order. */
  const regionsPicked: number[] = [];
  /** What the panel is told a removal of each region would take, by id. */
  const regionPlans = new Map<number, RegionPlan>();
  /**
   * What the redactions panel is told each region covers, by id.
   *
   * `marksCovered`'s arrangement and its reason, plus one of its own: the
   * application computes these from the page's text, and the four answers a row
   * can draw include two --- not read yet, could not be read --- that a real
   * document reaches only by failing. Writing them here is how a phase gets to
   * see all four.
   */
  const regionsCovered = new Map<number, string | null | undefined>();
  /** Every tab the sidebar has reported showing, in order. */
  const tabsSeen: string[] = [];
  /** Web targets the outline or a page reported, newest last. */
  const webLinksActivated: WebTarget[] = [];
  const panel = document.createElement("div");
  panel.style.cssText = `position:fixed;left:${WIDTH}px;top:0;width:300px;height:${HEIGHT}px;`;
  document.body.appendChild(panel);
  const sidebar = new Sidebar(panel, {
    onNavigate: (target, top) => viewer.goToDestination(target, top),
    // Recorded, never opened. A check that handed an address to the operating
    // system would put a browser window over the one this harness is measuring
    // --- and `AGENTS.md` records that a locked or occluded screen is enough to
    // stop a check running at all. What the recording is for is the assertion
    // below: that activating a web row reports it rather than doing nothing.
    onWebLink: (target) => webLinksActivated.push(target),
    results: { onPick: (index) => viewer.showMatch(index) },
    comments: { onPick: (id) => viewer.showComment(id) },
    marks: {
      onPick: (id) => viewer.showMark(id),
      // Recorded rather than driven: this harness opens one document and every
      // later phase reads the marks it made, so a phase that actually took one
      // off would change what the phases after it are looking at.
      onRemove: (id) => marksRemoved.push(id),
      coveredFor: (id) => marksCovered.get(id) ?? "",
    },
    redactions: {
      // Recorded rather than scrolled. What the application does with a pick is
      // `showRedaction`, which needs the edit model to turn a region into a
      // slot; what no unit test can reach is whether pressing a row in a real
      // web view arrives here at all, carrying the region it landed on.
      onPick: (id) => regionsPicked.push(id),
      // Recorded rather than applied, for `marks.onRemove`'s reason: this
      // harness opens one document and every later phase reads the regions the
      // phases before it made.
      onRemove: (id) => regionsRemoved.push(id),
      wordsFor: (id) => regionsCovered.get(id),
      // Written by a phase, for `regionsCovered`'s reason: the objects a removal
      // cannot take come from the backend, and a synthetic region on a fixture
      // reaches only the arms that fixture happens to have.
      planFor: (id) => regionPlans.get(id),
    },
    // Not driven by any phase yet: the check behind it needs the application's
    // command, and `hiddenlist.test.ts` presses the rows.
    hidden: { onPick: () => {} },
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
      // Recorded rather than opening a menu, for the reason `onReorder` gives:
      // what the shell does with this is one line, and what no unit test can
      // reach is whether a right-click on a row in a real web view arrives here
      // at all -- carrying the slot it landed on, and having stopped the web
      // view's own menu from appearing over it.
      onContextMenu: (slot) => menus.push(slot),
    },
    // Recorded, not acted on. In the application this is what starts the
    // comments panel's covered-words lookup, so that the extraction is paid for
    // by a reader who opened the tab; here the phase drives that lookup itself,
    // and what is left to check is that selecting a tab reports it at all.
    onTab: (tab) => tabsSeen.push(tab),
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
    // What the viewer asked the model to do with the reader's own marks. There
    // is no model here, so this record *is* the observable --- see
    // `markNoteChecks`.
    // Wired exactly as `App.svelte` wires it, so what the marks panel phase
    // checks below is the connection the application has rather than a second
    // one written for the harness.
    onMark: (id) => sidebar.marks.select(id),
    onMarkNote: (mark, note) => markEdits.push(`note:${mark}:${note}`),
    onMarkRemove: (mark) => markEdits.push(`remove:${mark}`),
    // The swatch row's, recorded into the same list for the same reason --- see
    // `markColorChecks`.
    onMarkRecolor: (mark, color) => markEdits.push(`color:${mark}:${color.join(",")}`),
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
  // Early, on an upright view that nothing has edited, and it leaves the view
  // one page to a row and fitted as it found it.
  await spreadChecks(viewer, doc, seen);
  await selectionChecks(root, viewer, doc);
  await searchChecks(root, viewer, doc, seen);
  await resultsChecks(viewer, sidebar, doc, seen);
  await mappingChecks(viewer, sidebar, doc, seen);
  await crossPageChecks(viewer, doc, seen);
  await scopedSearchChecks(root, viewer, seen);
  await paletteChecks(viewer, doc.page_count);
  await appCommandChecks(viewer, doc);
  await accessibilityChecks(root, viewer, doc, seen);
  await outlineChecks(viewer, sidebar, doc, webLinksActivated);
  // Before `rotationChecks`, which leaves the view turned: `screenPoint` maps a
  // page-space point without the view's own rotation, so a press aimed through
  // it lands somewhere else once the reader has turned the page. That is a
  // property of the harness rather than of the viewer --- `Viewer.anchorFor`
  // does apply the turn --- and running here is cheaper than a second mapping
  // nobody else needs.
  await commentChecks(root, viewer, sidebar, doc);
  // Beside the comments and under the same constraint: `screenPoint` does not
  // apply the view's rotation, so both have to run before `rotationChecks`
  // leaves the view turned.
  await markNoteChecks(root, viewer, markEdits);
  await markColorChecks(viewer, markEdits);
  // Under the same constraint again --- it presses a mark on the page through
  // `screenPoint`, which does not apply the view's own rotation.
  await markPanelChecks(root, viewer, sidebar, doc, marksRemoved, marksCovered);
  await linkChecks(root, viewer, doc, problems);
  // Reads only --- it drives no surface and turns nothing, so it is free of
  // the ordering constraint every check above it carries.
  await propertiesChecks(doc);
  await thumbnailChecks(root, viewer, sidebar, doc, page, drags, menus);
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
  // Before the print and the release, because it leaves a journal entry on the
  // open document: a highlight made, noted and taken off again.
  // After every phase that drives the surface and before the backend ones:
  // it paints marks of its own on the overlay and clears them again, so it
  // must not run while another phase is looking at what is drawn.
  await overlayInkChecks(root, viewer);
  // After it, and for the same reason it runs where it does: it paints marks of
  // its own and then saves them, so nothing that looks at the surface may run
  // while it is going. It leaves the open document as it found it.
  await markAgreementChecks(path, doc, viewer);
  await markCommandChecks(doc);
  await cropCommandChecks(doc);
  await printChecks(path, doc);
  await validationDataChecks(path);
  await releaseChecks(path);

  sidebar.destroy();
  panel.remove();
  viewer.destroy();
  check("destroys cleanly", viewer.idle, "frame loop stopped");
}
