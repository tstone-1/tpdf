/**
 * Inverted colours, rotated tiles, the strip of a rotated page, navigating from the strip, and yielding to input.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { frame } from "../checkreport";
import type { DocumentInfo } from "../ipc";
import { Sidebar } from "../sidebar";
import { tileUrl, fetchRequiredTile } from "../tiles";
import { rowHeightFor } from "../thumbnails";
import { Viewer, type ViewerStatus } from "../viewer";
import {
  check,
  eventually,
  key,
  settle,
  skip,
} from "./common";

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
export async function invertChecks(
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
export async function tileBytes(
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
export function meanLightness(rgba: Uint8ClampedArray): number {
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
export async function rotatedTileCheck(
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
export function sameLine(a: string, b: string): boolean {
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
export const CORE_CHARS = 8;

/** The box enclosing every character that has one, in the view's own space. */
export function inkBounds(text: {
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
export function rotatedStripCheck(
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
export function panelTabs(): HTMLElement[] {
  return [
    ...document.querySelectorAll<HTMLElement>('.tpdf-sidebar [role="tab"]'),
  ];
}

/** Whether a panel is displayed. */
export function panelShown(tab: string): boolean {
  const panel = document.getElementById(`tpdf-panel-${tab}`);
  return !!panel && getComputedStyle(panel).display !== "none";
}

/** Activating a row moves the viewer, asserted against not being there already. */
export async function navigateFromStrip(
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
export async function yieldChecks(
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
export function pickNeedle(codes: number[]): string | null {
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
export function preview(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > 40 ? `${flat.slice(0, 40)}...` : flat;
}
