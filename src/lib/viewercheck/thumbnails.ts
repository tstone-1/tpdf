/**
 * The thumbnail strip, dragging from it, and text on a rotated page.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import type { DocumentInfo, PageSize } from "../ipc";
import { Sidebar, SIDEBAR_CLASS } from "../sidebar";
import { rowHeightFor, OVERSCAN, type Thumbnails } from "../thumbnails";
import { Viewer, type ViewerStatus } from "../viewer";
import {
  check,
  drag,
  eventually,
  HEIGHT,
  key,
  pointer,
  settle,
  SIDEBAR_TABS,
  skip,
} from "./common";
import {
  CORE_CHARS,
  inkBounds,
  navigateFromStrip,
  panelShown,
  panelTabs,
  preview,
  rotatedStripCheck,
  rotatedTileCheck,
  sameLine,
  yieldChecks,
} from "./invert";

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
export async function thumbnailChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  page: PageSize,
  drags: [number, number][],
  menus: number[],
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

  // Five tabs in a 260-pixel panel, and each new one takes width from the rest.
  // The count above cannot see this: a button clipped by the panel's
  // `overflow:hidden` is still in the DOM, still `role="tab"`, and still
  // unreachable by a pointer. Two readings, because they fail differently --- a
  // button whose *label* does not fit is unreadable, and one whose *box* runs
  // past the panel is unpressable.
  const bar = document.querySelector(`.${SIDEBAR_CLASS}`)?.getBoundingClientRect();
  const clipped = tabs.filter((t) => t.scrollWidth > t.clientWidth + 1);
  const outside = bar
    ? tabs.filter((t) => t.getBoundingClientRect().right > bar.right + 1)
    : [];
  check(
    "every sidebar tab fits inside the panel",
    !!bar && clipped.length === 0 && outside.length === 0,
    tabs
      .map((t) => `${t.textContent}=${t.scrollWidth}/${t.clientWidth}`)
      .join(" ") +
      ` in ${bar ? Math.round(bar.width) : "?"}px` +
      (clipped.length ? `, clipped [${clipped.map((t) => t.textContent).join(", ")}]` : "") +
      (outside.length ? `, past the edge [${outside.map((t) => t.textContent).join(", ")}]` : ""),
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
  // Right-clicking a page. Two things at once, and both are the report that
  // asked for this: the web view's own menu -- whose one entry reloads the
  // frontend and throws away the reader's document -- must not appear, and the
  // slot under the pointer must reach the shell. A synthetic `contextmenu` is
  // the only way to ask: a real secondary click posted through the window
  // server does not reach a web view's DOM at all, which was measured before
  // this check existed.
  {
    const slot = strip.mounted[0] ?? 0;
    const row = strip.elementFor(slot);
    const before = menus.length;
    const event = new MouseEvent("contextmenu", {
      bubbles: true,
      // Without this `preventDefault` is a no-op and `defaultPrevented` stays
      // false however correct the handler is --- an assertion that could only
      // fail.
      cancelable: true,
      clientX: 20,
      clientY: 20,
    });
    row?.dispatchEvent(event);
    check(
      "right-clicking a page suppresses the web view's own menu",
      row !== null && event.defaultPrevented,
      `row=${row !== null}, prevented=${event.defaultPrevented}`,
    );
    check(
      "right-clicking a page reports the page it landed on",
      menus.length === before + 1 && menus.at(-1) === slot,
      `${menus.length - before} report(s), last=${menus.at(-1)}, wanted ${slot}`,
    );
  }
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
export async function rotationChecks(
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
