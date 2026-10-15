/**
 * Reading order, fitting the page, selection, selection by word and line, and the quads a mark is drawn from.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import { MULTI_CLICK_SLOP_PX } from "../clicks";
import type { DocumentInfo } from "../ipc";
import { readingLines, hasSideBySideLines } from "../reading";
import { TextCache } from "../text";
import { filePage } from "../pages";
import { Viewer, SCROLLBAR_WIDTH } from "../viewer";
import {
  check,
  click,
  drag,
  eventually,
  key,
  pointer,
  settle,
  skip,
} from "./common";
import { preview } from "./invert";

/** What a fixture's generator says each of its pages should read as. */
export interface ReadingManifest {
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
export async function readingChecks(doc: DocumentInfo): Promise<void> {
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
    // A page of the file, and these corpora are unedited, so it is also the
    // slot. The cast says which of the two this number is --- see `FilePage`.
    const text = await cache.load(filePage(at));
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
export async function fitChecks(root: HTMLElement, viewer: Viewer): Promise<void> {
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
export async function selectionChecks(
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
    // The mark pair too, and leaving them out is what the sweep caught: this
    // block *returns*, so on a page with no text the two names were neither run
    // nor skipped -- they simply did not exist, and `vector-heavy` and
    // `vector-multi` reported 235 names against everything else's 237. A name
    // that has vanished is the failure the identical-name-sets rule is for.
    await markQuadChecks(root, viewer, false);
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
  await markQuadChecks(root, viewer, ok && whole !== "");

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

/**
 * The rectangles a mark would be made from, and the one property they must have.
 *
 * A highlight is stored against the *page*, not against how the reader is
 * holding it --- so the quads `selectionQuadsByPage` produces have to be the
 * same numbers whichever way the view is rotated. They are built from
 * `TextCache.peekUnturned` for exactly that reason, and the alternative anyone
 * would reach for first --- the boxes the selection paints with, which is what
 * `peek` answers --- turns with the view and would store a mark that moves the
 * next time somebody presses ⌘R.
 *
 * **This is the only check that can catch that**, and it needs the rotation:
 * with the view upright the two sources are identical, so a version built on
 * the wrong one passes everything else in this file.
 *
 * The view is restored before returning. Every phase after this one inherits
 * whatever it is left in, and `AGENTS.md` records a run where three later
 * phases went red because one did not put the rotation back.
 */
async function markQuadChecks(
  root: HTMLElement,
  viewer: Viewer,
  selected: boolean,
): Promise<void> {
  if (!selected) {
    skip(
      "a mark's rectangles come from the page's own text",
      "nothing is selected, so there are no rectangles to take",
    );
    skip(
      "and they do not move when the view is rotated",
      "nothing is selected, so there are no rectangles to compare",
    );
    skip(
      "and the words they cover are the words that are selected",
      "nothing is selected, so there are no words to compare",
    );
    return;
  }

  key(root, "a", true);
  await settle(() => viewer.idle);
  const upright = viewer.selectionQuadsByPage();
  check(
    "a mark's rectangles come from the page's own text",
    upright.length > 0 && upright.every((page) => page.quads.length % 4 === 0),
    `${upright.length} page(s), ${upright.reduce((n, p) => n + p.quads.length / 4, 0)} quad(s)`,
  );

  // The words that come out beside the rectangles, which is what the marks
  // panel lists a highlight by when nobody typed a note on it.
  //
  // Two routes to one string, which is what makes this a comparison rather than
  // a restatement: this one is per page and off `peekUnturned`, and
  // `selectedText` is the whole selection off `peek` --- the view's text, which
  // on a rotated page is a different `PageText` object. They agree because a
  // rotation renumbers no character. Select-all takes one page, so the join is
  // exact rather than approximately right.
  //
  // What it cannot see is a text taken from the *view's* space, since the two
  // agree with the view upright; the rotation check below is what covers that,
  // and it covers it now that the words travel in the record it compares.
  const said = upright.map((page) => page.text).join("\n");
  const whole = viewer.selectedText;
  check(
    "and the words they cover are the words that are selected",
    said.length > 0 && said === whole,
    `${said.length} char(s) over ${upright.length} page(s) against ` +
      `${whole.length} selected: ${JSON.stringify(said.slice(0, 40))} against ` +
      `${JSON.stringify(whole.slice(0, 40))}`,
  );

  const before = viewer.rotation;
  viewer.rotateBy(1);
  await settle(() => viewer.idle);
  const turned = viewer.selectionQuadsByPage();
  viewer.rotateBy(-1);
  await settle(() => viewer.idle);

  check(
    "and they do not move when the view is rotated",
    viewer.rotation === before &&
      JSON.stringify(turned) === JSON.stringify(upright),
    turned.length === upright.length
      ? `${turned.length} page(s), first quad ${JSON.stringify(turned[0]?.quads.slice(0, 4))} against ${JSON.stringify(upright[0]?.quads.slice(0, 4))}, ` +
        `${turned[0]?.text.length} char(s) against ${upright[0]?.text.length}`
      : `${turned.length} page(s) rotated against ${upright.length} upright`,
  );
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
