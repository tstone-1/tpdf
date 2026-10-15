/**
 * The ink a mark puts on the page, and agreement between a mark and its pixels.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import {
  INK_WIDTH,
  TEXT_INSET,
  TEXT_SIZE,
  TEXT_LEADING,
} from "../markband";
import type { DocumentInfo } from "../ipc";
import { type MarkKind, pageId } from "../pages";
import { Viewer } from "../viewer";
import { check, settle, skip } from "./common";
import { preview, tileBytes } from "./invert";
import { hueOf, inkPreviewChecks } from "./previews";
import { attempt } from "./editing";

/**
 * The names this phase reports, so a run that cannot start still prints them.
 *
 * **Keyed, not positional, and that is the second repair to the same mistake.**
 * The distinctness check was written as `OVERLAY_INK_CHECKS[6]`; adding ink
 * shifted it and it began reporting under the drawing's name. That was "fixed"
 * with `length - 1` --- which encodes *"distinctness is last"* --- and two
 * checks appended after it broke it again, identically, within the hour: the
 * run printed the distinctness verdict under *"a second stroke joins the
 * drawing"*, printed that name twice, and never printed *"the six kinds do not
 * all look the same"* at all.
 *
 * A key cannot shift. {@link OVERLAY_INK_CHECKS} is derived from this so the
 * declared list and the names used stay one thing.
 */
export const INK_CHECK = {
  empty: "an untouched page has nothing on the overlay",
  highlight: "a highlight fills its quad",
  underline: "an underline leaves the middle of its quad clear",
  squiggly: "a squiggle rises above where an underline's rule stops",
  strikeout: "a strikeout crosses the middle of its quad",
  square: "a box is a frame with its middle clear",
  ellipse: "an ellipse touches its rectangle's sides and misses its corners",
  textbox: "a text box draws its words and not its rectangle",
  note: "a comment draws inside its own icon box",
  ink: "a drawing follows its strokes and does not fill its rectangle",
  stamp: "a stamp is a border with a word inside it",
  // **No count in the name, and it used to carry one.** It read "the nine kinds
  // do not all look the same", and a tenth kind renames it --- which changes the
  // set of check names every corpus is diffed against, for a reason that is not
  // about the check. `docs/TRAPS.md` has the entry: a test named for the
  // population it covers is renamed by every kind you add.
  distinct: "no two kinds look the same",
  preview: "a drawing in progress is previewed as a line, not as a rubber band",
  second: "a second stroke joins the drawing rather than replacing it",
  erased: "a stroke the eraser has taken stops being drawn at once",
  spared: "and one the nib missed is still there",
  taken: "a mark with no strokes the eraser has crossed stops being drawn whole",
  left: "and one beside it the nib missed is still there",
  scrimOut: "a crop being dragged darkens the paper it would throw away",
  scrimIn: "and leaves the part it would keep clear",
} as const;

const OVERLAY_INK_CHECKS: string[] = Object.values(INK_CHECK);



/**
 * What the reader actually sees, in overlay pixels.
 *
 * **This phase exists because there was no check on the overlay at all, and a
 * reader found the defect that fact allowed.** `paintMarks` filled every mark
 * across its whole quad in one colour, so while a document was open an
 * underline and a strikeout both appeared as a highlight --- and the *saved
 * file* was correct throughout, which is the worse shape of wrong: nothing
 * could be seen until the file was saved and reopened, at which point the mark
 * changed under the reader.
 *
 * `annot-probe --mode ink|rule|outline` measures the file's pixels and can say
 * nothing about these. The two renderers have to agree, and until now only one
 * of them was measured.
 *
 * **Every reading is relative to the mark's own anchor**, which the viewer
 * supplies. That is what makes the phase rotation- and crop-independent: a
 * corpus at `/Rotate 90` draws an underline down the side of the screen, so
 * anything keyed on "the bottom band" would be measuring the wrong strip there,
 * and a fixture that skipped rotated corpora would skip the one that most needs
 * this.
 */
export async function overlayInkChecks(
  root: HTMLElement,
  viewer: Viewer,
): Promise<void> {
  const canvas = viewer.overlaySurface;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) {
    for (const name of OVERLAY_INK_CHECKS) {
      skip(name, "the overlay has no 2d context to read");
    }
    return;
  }

  viewer.goToPage(0);
  await settle(() => viewer.idle);

  const dpr = window.devicePixelRatio || 1;

  /** The fraction of a device-pixel box that carries any of our ink. */
  const inked = (
    left: number,
    top: number,
    right: number,
    bottom: number,
  ): number | null => {
    const x0 = Math.max(0, Math.round(left * dpr));
    const y0 = Math.max(0, Math.round(top * dpr));
    const x1 = Math.min(canvas.width, Math.round(right * dpr));
    const y1 = Math.min(canvas.height, Math.round(bottom * dpr));
    if (x1 - x0 < 2 || y1 - y0 < 2) return null;
    const { data } = ctx.getImageData(x0, y0, x1 - x0, y1 - y0);
    let hit = 0;
    for (let at = 3; at < data.length; at += 4) {
      // The alpha channel, not the colour: the overlay is transparent where
      // nothing was painted, and every kind is painted in some colour. Keying
      // on a particular colour would make this a check on `MARK_COLORS`, which
      // is a different question and one a unit test already answers.
      if ((data[at] ?? 0) > 8) hit++;
    }
    return hit / (data.length / 4);
  };

  /** Paints one mark alone and reads its own rectangle back. */
  const paint = async (
    kind: MarkKind,
  ): Promise<{
    whole: number;
    core: number;
    edges: number;
    corners: number;
    shoulder: number;
    second: number;
    /** Ink in the column and band where the first line of type must be. */
    lineOne: number;
    /** The same, where a second line goes. */
    lineTwo: number;
    /** How many of the box's top, right and bottom borders carry ink. */
    rim: number;
    /** Which sides carry ink, for the detail line. */
    sides: string[];
    /** The rectangle as it was measured on screen, in CSS pixels. */
    box: { width: number; height: number };
  } | null> => {
    const size = viewer.pageSize(0);
    // **A text box's rectangle is a fixed size in points, not a fraction of the
    // page**, and that is the repair for a check that was red on four of the
    // fourteen corpora. Every other reading here is a fraction of the mark's
    // rectangle, which is right for a shape that scales with the page --- and a
    // text box's content does not scale: it is 11-point type whatever the paper
    // is. On A0 the page-relative box was 1,073 x 135 points, so two lines of
    // type rounded to 0% of it and the check read "nothing was drawn"; on a
    // 20-pixel-tall box the *sample* for the left edge, which reads the middle
    // tenth of the height, landed on the first line and the check read "the
    // rectangle was drawn". Both readings were wrong about a correct painter,
    // in opposite directions, and they are the same defect: a fraction of a
    // rectangle compared against something that is not.
    //
    // 260 x 90 points. The width and the *type* need far less --- two lines of
    // 11-point Helvetica end by 28.4 points down, which is
    // `TEXT_INSET + 2 * TEXT_SIZE * TEXT_LEADING` --- and the height is set by
    // the **sampler's own floor at the smallest scale in the corpus**, not by
    // the type. `inked` refuses a region under two pixels, `core` reads the
    // middle tenth of the height, and an A0 page fits into a 900-pixel window at
    // 0.37 pixels per point: at 40 points tall that sample is 1.5 pixels and the
    // whole reading comes back `null`, which turned a red check into a skipped
    // one on `vector-heavy` and `vector-multi`. At 90 it is 3.4 pixels. The
    // extra height is empty paper below the type, which is what a text box
    // dragged out large and typed one sentence into actually looks like.
    //
    // The same rectangle on every corpus is what lets the readings below be
    // literals. Clamped to the page in case one is ever smaller than that; no
    // corpus is, and if the clamp ever bites the check goes red with the
    // measured box printed in its detail line rather than passing quietly.
    const TEXT_BOX_PT = { width: 260, height: 90 };
    const boxPt = {
      width: Math.min(TEXT_BOX_PT.width, size.width_pt * 0.5),
      height: Math.min(TEXT_BOX_PT.height, size.height_pt * 0.2),
    };
    // **A comment's box needs the same floor the text box's does, and it was
    // 20 points square until 2026-08-25.** `core` reads the middle tenth of the
    // height, so a 20-point box asks the sampler for two points of it, and
    // `inked` refuses a region under two pixels --- which it is at every scale
    // in the corpus, not merely at the smallest. So this check reported
    // `[SKIP] the mark's rectangle is off screen or too small to sample` on
    // every fixture, measured on `outline-simple.pdf` and `comments.pdf`
    // before the change: it was in the name set, so the pairwise name diff saw
    // nothing missing, and it could not go red. A mutation aimed at it is what
    // found it --- the harness refused to start rather than reporting SURVIVED,
    // which is the verdict that would have read as a gap in the checks.
    //
    // Square rather than the text box's 260x90, because a bubble is drawn from
    // both of its dimensions: `drawBubble` takes the body as 78% of the height
    // and the corner radius from `min(width, body)`, so a wide short box would
    // change the shape being read rather than only its size. 90 points is the
    // text box's height and carries its argument verbatim --- an A0 page in a
    // 900-pixel window is 0.37 pixels per point, where a tenth of 90 is 3.3
    // pixels against the floor of two. Clamped the same way, and no corpus
    // clamps below 80.
    const NOTE_BOX_PT = 90;
    const notePt = {
      width: Math.min(NOTE_BOX_PT, size.width_pt * 0.5),
      height: Math.min(NOTE_BOX_PT, size.height_pt * 0.2),
    };
    const quad =
      kind === "note"
        ? [
            size.width_pt * 0.2,
            size.height_pt * 0.08,
            size.width_pt * 0.2 + notePt.width,
            size.height_pt * 0.08 + notePt.height,
          ]
        : kind === "textbox"
          ? [
              size.width_pt * 0.15,
              size.height_pt * 0.08,
              size.width_pt * 0.15 + boxPt.width,
              size.height_pt * 0.08 + boxPt.height,
            ]
          : [size.width_pt * 0.15, size.height_pt * 0.08, size.width_pt * 0.6, size.height_pt * 0.12];
    // **Ink is the one kind whose rectangle is not its shape**, so it is sent
    // the strokes and the rectangle is derived here the way the model derives
    // it. Two horizontal strokes along the top and bottom of the same box every
    // other kind uses: that reads as two inked edges and an empty centre, which
    // is a combination none of the other five produces --- an underline has one
    // edge, a strikeout fills the centre, a frame has four.
    const [left, top, right, bottom] = [
      quad[0] ?? 0,
      quad[1] ?? 0,
      quad[2] ?? 0,
      quad[3] ?? 0,
    ];
    const strokes =
      kind === "ink"
        ? [
            [left, top + INK_WIDTH, right, top + INK_WIDTH],
            [left, bottom - INK_WIDTH, right, bottom - INK_WIDTH],
          ]
        : [];
    // **A text box is the one kind that draws nothing without a note**, because
    // for every other kind the note is a remark about something drawn and here
    // it is the thing on the page. An empty one is a correct empty box, which
    // is why the check below cannot be `whole > 0` alone.
    //
    // The lines are supplied rather than derived: in the application the backend
    // wraps and sends them, and what this phase measures is whether the overlay
    // *draws* what it is handed. Two short lines, so the ink lands across the
    // top of the box and touches no edge -- which is the reading that separates
    // a drawn text box from one that fell through to `fillRect`.
    const words = kind === "textbox" ? ["the reader", "typed this"] : [];
    viewer.setMarks([
      {
        id: 7777,
        kind,
        // A stamp with no name draws an empty border, which is a box --- so the
        // reading that separates the two would fail for the wrong reason. The
        // model refuses a name on any other kind, so this is that biconditional
        // rather than a default.
        stamp: kind === "stamp" ? ("draft" as const) : null,
        page: pageId(1),
        quads: quad,
        strokes,
        color: [0.85, 0.15, 0.15],
        width: INK_WIDTH,
        note: words.join(" "),
        lines: words,
      },
    ]);
    await frame();
    await frame();
    const at = viewer.markAnchor(7777);
    if (!at) return null;
    const width = at.right - at.left;
    const height = at.bottom - at.top;
    if (width < 20 || height < 12) return null;
    const whole = inked(at.left, at.top, at.right, at.bottom);
    // **A small box at the dead centre, not the inner half.** The inner half was
    // the first draft and it made the strikeout check fail on a correct
    // painter: a rule is 7% of the quad's height, so inside a box half the
    // quad's height it covers 14% of the area, and the reading came back 17%
    // where the check wanted "most of it". The bound was not too tight --- the
    // *observable* was the wrong one. At the centre a rule reads full and an
    // underline reads empty, which is the discrimination in one number.
    const core = inked(
      at.left + width * 0.4,
      at.top + height * 0.45,
      at.right - width * 0.4,
      at.bottom - height * 0.45,
    );
    // **How many of the four sides carry ink**, rather than how much one of
    // them does. The first draft measured the left edge as a fraction and
    // wanted most of it: a stroke is 1.5 points wide inside a sample five per
    // cent of the mark's width, so a correct box reads 7%, and the only repair
    // available would have been a threshold two per cent above an underline's
    // zero. A count is the robust form of the same question --- an underline has
    // one side, a rule through the middle has two, a frame has four --- and it
    // needs no bound that depends on the zoom the fixture happened to pick.
    const along = (
      left: number,
      top: number,
      right: number,
      bottom: number,
    ): boolean => (inked(left, top, right, bottom) ?? 0) > 0.02;
    const thickX = Math.max(3, width * 0.06);
    const thickY = Math.max(3, height * 0.15);
    const inkedSides = (
      [
        // Left and right, sampled across the middle tenth of the height, where a
        // rule through the centre crosses them and an underline does not.
        ["left", along(at.left, at.top + height * 0.45, at.left + thickX, at.bottom - height * 0.45)],
        ["right", along(at.right - thickX, at.top + height * 0.45, at.right, at.bottom - height * 0.45)],
        // Top and bottom, across the middle fifth of the width.
        ["top", along(at.left + width * 0.4, at.top, at.right - width * 0.4, at.top + thickY)],
        ["bottom", along(at.left + width * 0.4, at.bottom - thickY, at.right - width * 0.4, at.bottom)],
      ] as [string, boolean][]
    )
      .filter(([, hit]) => hit)
      .map(([side]) => side);
    // **Which sides, not just how many.** A count says a check failed and a
    // name says why: `edges === 1` on `rotated-90` was read three ways from
    // arithmetic before this line existed, and the answer --- *which* edge ---
    // is one word. The predicates below still read the count; this is for the
    // detail line, which is what somebody has to act on.
    const edges = inkedSides.length;
    // **How many of the four corners carry ink**, which is the only reading that
    // tells a frame from a ring. Every other number here is the same for both:
    // an ellipse touches its rectangle at the middle of each side, which is
    // exactly where `edges` samples, and its centre is as empty as a box's --- so
    // a painter that drew `strokeRect` for an ellipse would satisfy `whole`,
    // `core` and `edges` and be caught by nothing.
    //
    // The same reading `annot-probe --mode outline` needed for the file, and it
    // is needed here for a different reason: there the writer could emit `re`,
    // here the overlay could call `strokeRect`. Two independent halves, one
    // discriminating observable.
    //
    // Empty by a margin rather than by a hair: at `thickX` in from the left the
    // ellipse still spans only the middle half of the height, well clear of the
    // top `thickY`.
    const corners = [
      [at.left, at.top, at.left + thickX, at.top + thickY],
      [at.right - thickX, at.top, at.right, at.top + thickY],
      [at.left, at.bottom - thickY, at.left + thickX, at.bottom],
      [at.right - thickX, at.bottom - thickY, at.right, at.bottom],
    ].filter(([l, tp, r, b]) => along(l ?? 0, tp ?? 0, r ?? 0, b ?? 0)).length;
    // **The strip above where an underline's rule stops**, which is the only
    // reading that tells a squiggle from a rule. Every other number here is the
    // same for both: a squiggle's ink is a thin band at the bottom of the quad
    // with an empty centre, one inked side and two inked corners, which is an
    // underline exactly. The kinds differ in how far up the quad the ink
    // reaches, and nothing above measured that.
    //
    // **Fixed fractions, not derived from `LINE_FRACTION` or
    // `SQUIGGLE_HEIGHT`.** A band computed from the constants it polices moves
    // with them and stops being able to fail. The rule ends at 7% of the height
    // and the wave's peaks reach 18%, so 10% to 16% is clear of both edges.
    // `annot-probe --mode wave` reads the same strip in the saved file, against
    // a different wrong implementation.
    const shoulder = inked(
      at.left,
      at.bottom - height * 0.16,
      at.right,
      at.bottom - height * 0.1,
    );
    // **Ink where a second line of type would go**, which is the only reading
    // that can count lines. Added because a mutation drawing just the first line
    // of a text box survived everything else: `whole` drops from 5% to 3% and
    // stays inside its bounds, `edges` stays at 0, and a reader loses every line
    // after the first while the saved file keeps them all.
    //
    // The lower half of the box, which the fixture's first line cannot reach: at
    // 11 pt in a box of about 34, the first line's body ends around 38% of the
    // way down and the second begins around 44%.
    const second = inked(
      at.left,
      at.top + height * 0.5,
      at.right,
      at.top + height * 0.9,
    );

    // **Three readings in points off the box's own corner, not in fractions of
    // it.** They exist because the text box needs an observable that does not
    // move when the paper does, and they are taken for every kind so that the
    // distinctness signature keeps one shape --- which also makes the other
    // eight their controls: a wash inks both line bands *and* all three rim
    // strips, a frame inks the rims and neither band.
    //
    // `scale` is derived from the two things that define it, the rectangle as
    // asked for in points and as measured on screen, rather than from the
    // viewer's zoom --- so it is right whatever the fit and needs no second
    // route to the same number.
    const scale = width / (right - left);
    // **This kind's own rectangle in points, not the text box's.** `boxPt`
    // above is the text box's fixed 260 x 90, and using it here read a note's
    // right-hand border 240 points outside the 20-point box it is drawn in ---
    // a sample of empty canvas reported as a clean border.
    const ownW = right - left;
    const ownH = bottom - top;
    const pt = (x0: number, y0: number, x1: number, y1: number) =>
      along(
        at.left + x0 * scale,
        at.top + y0 * scale,
        at.left + x1 * scale,
        at.top + y1 * scale,
      );
    const band = (y0: number, y1: number) =>
      inked(
        at.left + 2 * scale,
        at.top + y0 * scale,
        // Clamped to this kind's own width, for `ownW`'s reason: 80 points of
        // column is the text box's two strings and is wider than a note's whole
        // rectangle.
        at.left + Math.min(80, ownW) * scale,
        at.top + y1 * scale,
      ) ?? 0;
    // **Literals, and the constants they were chosen from are stated rather
    // than read.** A band computed from `TEXT_SIZE` and `TEXT_LEADING` moves
    // with them and stops being able to fail, which is the argument
    // `shoulder` already makes about `SQUIGGLE_HEIGHT`. At 11-point type with
    // 1.2 leading and a 2-point inset the first line's baseline is at 13 points
    // and its bodies span roughly 5 to 13; the second sits at 26.2, spanning
    // 18 to 26. So 4..15 and 17..28 hold one line each and no part of the
    // other, and both are in the top third of a box 90 points tall. The column
    // is 2..80 points, which covers the
    // fixture's two strings --- about 55 points of Helvetica --- and is
    // independent of how wide the rectangle is.
    const lineOne = band(4, 15);
    const lineTwo = band(17, 28);
    // **The rectangle's own border, three sides of it.** This replaces
    // `edges === 0` for the text box, which asked the wrong question: that
    // sample reads the middle tenth of the height, which in a two-line box is
    // where the *second line* sits, and on A4 it passed by half a point.
    //
    // The left side is deliberately absent. The type starts `TEXT_INSET` in,
    // which is 2 points, and a strip narrow enough to sit inside that is
    // narrower than a glyph's antialiasing --- so it would be a reading that
    // fails on correct output. Three sides catch what this has to catch: a
    // `strokeRect` inks all four and a `fillRect` inks everything.
    const rim = [
      pt(0, 0, ownW, 3),
      pt(ownW - 3, 0, ownW, ownH),
      pt(0, ownH - 3, ownW, ownH),
    ].filter(Boolean).length;
    return whole === null || core === null || shoulder === null || second === null
      ? null
      : {
          whole,
          core,
          edges,
          corners,
          shoulder,
          second,
          lineOne,
          lineTwo,
          rim,
          sides: inkedSides,
          box: { width, height },
        };
  };

  // The control, and it comes first for the reason every control here does: a
  // reading of "nothing is inked" means nothing unless an empty overlay reads
  // that way too, and a canvas this harness never cleared would read as inked
  // everywhere and pass three of the checks below by accident.
  viewer.setMarks([]);
  await frame();
  await frame();
  const empty = inked(0, 0, canvas.width / dpr, canvas.height / dpr);
  check(
    INK_CHECK.empty,
    empty !== null && empty < 0.001,
    empty === null ? "the overlay is too small to sample" : `${(empty * 100).toFixed(2)}% inked`,
  );

  type Reading = {
    whole: number;
    core: number;
    edges: number;
    corners: number;
    shoulder: number;
    second: number;
    lineOne: number;
    lineTwo: number;
    rim: number;
  };
  const read: Record<string, Reading> = {};
  const wanted: [MarkKind, string, (r: Reading) => boolean][] = [
    // A wash covers its quad, so every reading is high.
    ["highlight", INK_CHECK.highlight, (r) => r.whole > 0.8 && r.core > 0.8],
    // A rule at the bottom edge: a thin band, nothing at the centre and nothing
    // on the side. This is the exact reading the shipped defect got wrong --- it
    // drew the whole quad, so all three came back full.
    [
      "underline",
      INK_CHECK.underline,
      (r) =>
        r.whole < 0.3 && r.core < 0.05 && r.edges === 1 && r.shoulder < 0.01,
    ],
    // A squiggle: the underline's three readings exactly, plus ink in the strip
    // the rule leaves empty. The `shoulder < 0.01` on the line above is this
    // one's control, and it is what stops the emptiness being a reading that
    // "nothing was drawn" satisfies just as well.
    [
      "squiggly",
      INK_CHECK.squiggly,
      (r) =>
        r.whole < 0.3 && r.core < 0.05 && r.edges === 1 && r.shoulder > 0.05,
    ],
    // The same thin band, through the one place an underline must not be. The
    // two together say the kinds are told apart, which neither says alone.
    //
    // **`core > 0.5`, and 0.8 was unreachable.** The bound went in at 0.8 with
    // this check and this check has never once passed: `markBand` centres a rule
    // `LINE_FRACTION` --- 7% --- of the quad's height, and `core` samples the
    // middle tenth, so the *correct* painter's ceiling is 0.07/0.10 = 0.70. The
    // reading was 0.71 on every run, antialiasing included. An assertion that
    // cannot pass is the loud twin of one that cannot fail, and it survived a
    // day only because this harness is not a gate and CI cannot run it.
    //
    // Not derived from `LINE_FRACTION`, which would make it move with the thing
    // it polices and stop being able to fail --- the trap about a check that
    // measures along the axis it is policing. A fixed number, chosen for what it
    // has to tell apart: an underline, a frame and a drawing all read 0.00 here,
    // so anything between those and 0.70 discriminates, and 0.5 sits in the
    // middle of that gap.
    [
      "strikeout",
      INK_CHECK.strikeout,
      (r) => r.whole < 0.3 && r.core > 0.5 && r.edges === 2,
    ],
    // A frame: a side where an underline has none, and a clear centre where a
    // filled box would have ink. Those are the two kinds it has to be told from,
    // and it takes both readings to do it.
    [
      "square",
      INK_CHECK.square,
      (r) => r.whole < 0.3 && r.core < 0.05 && r.edges === 4 && r.corners === 4,
    ],
    // Words, which read like nothing else here: ink across the top of the box
    // and none of it touching an edge. **The reading that matters is
    // `edges === 0`**, and it separates a drawn text box from the way it fails.
    // A box that fell through to the final `fillRect` -- which is what a missing
    // `isText` gives you -- reads 1.00 whole with four inked sides, and a reader
    // sees a solid red rectangle where they typed while the saved file has the
    // words in it the whole time. That is the underline defect's shape for a
    // third time, and this is the reading that catches it.
    [
      "textbox",
      INK_CHECK.textbox,
      // **Not one of these readings is a fraction of the rectangle's size**, and
      // that is the whole of the repair: the box is a fixed 260 x 40 points on
      // every corpus and the three readings are taken in points off its corner.
      // The predicate that stood here until 2026-08-20 --- `whole > 0.02 &&
      // edges === 0 && second > 0.005` --- was red on four of the fourteen
      // corpora against a painter that was drawing correctly on all four.
      //
      // Each clause is a different way for the drawing to be wrong. `lineOne`
      // is that the words are there at all; `lineTwo` is that there are two of
      // them, which is the reading a mutation drawing only the first line
      // defeated everything else to reach; `rim` is that the rectangle itself
      // is not drawn, which is what a `strokeRect` would do; and `whole` is
      // that it is not a filled box, which is what a fall-through to the final
      // `fillRect` gives --- the underline defect's shape for a third time, and
      // the reason this check was written.
      (r) => r.lineOne > 0.01 && r.lineTwo > 0.01 && r.rim === 0 && r.whole < 0.6,
    ],
    // A ring: the box's three readings exactly, and the corners are what make it
    // a different mark rather than the same one. Stated as `corners === 0` with
    // the box's `corners === 4` directly above, so the pair is a discrimination
    // in both directions -- an emptiness assertion whose control is the line
    // before it.
    [
      "ellipse",
      INK_CHECK.ellipse,
      (r) => r.whole < 0.3 && r.core < 0.05 && r.edges === 4 && r.corners === 0,
    ],
    // A border **and** a word, which is the box's reading with the one clause
    // that separates them flipped. A box requires `core < 0.05` and a stamp
    // requires ink there: a stamp drawn as a plain `strokeRect` --- which is what
    // a missing `isStamp` gives, since `isOutline` is one branch away --- reads
    // exactly like the box above and the saved file stays correct, which is the
    // underline defect's shape for a fourth time.
    //
    // `edges === 4` is the other half and it is not redundant with it: a stamp
    // drawn as *only* its word, which is what borrowing the text box's branch
    // would give, has ink in the core and none on any side.
    [
      "stamp",
      INK_CHECK.stamp,
      (r) => r.edges === 4 && r.core > 0.02 && r.whole < 0.9,
    ],
    // A bubble drawn inside a 20-point box. Loose on purpose: what shape it
    // should be is a drawing decision, and what a check can say is that
    // something of ours is there, at the centre, and it is not the whole
    // rectangle --- which a `fillRect` fallback would be.
    [
      "note",
      INK_CHECK.note,
      (r) => r.whole > 0.2 && r.whole < 0.95 && r.core > 0.5,
    ],
    // Two rules near the top and bottom edges, and nothing between them. The
    // reading that matters is `edges === 2` *with* an empty core: a painter
    // that fell back to the quad --- which is what `markBand` answers for this
    // kind, and what every other kind is drawn from --- fills all three, and
    // that is the exact defect the underline shipped with, one kind later.
    [
      "ink",
      INK_CHECK.ink,
      (r) => r.whole < 0.4 && r.core < 0.05 && r.edges === 2,
    ],
  ];

  // **The literals above are a statement about the type, so the type is
  // checked.** `lineOne` and `lineTwo` are written as fixed point offsets on
  // purpose --- a band derived from the constants it polices moves with them and
  // stops being able to fail --- and the cost of that is exactly this: the
  // numbers 4..15 and 17..28 are only the right strips while the inset is 2, the
  // size 11 and the leading 1.2. Changing one of those silently aims the sample
  // somewhere else, so it is refused rather than measured.
  const typeAsAssumed =
    TEXT_INSET === 2 && TEXT_SIZE === 11 && TEXT_LEADING === 1.2;

  for (const [kind, name, holds] of wanted) {
    if (kind === "textbox" && !typeAsAssumed) {
      skip(
        name,
        `the type bands are written for an inset of 2, a size of 11 and a ` +
          `leading of 1.2, and it is now ${TEXT_INSET}/${TEXT_SIZE}/${TEXT_LEADING}`,
      );
      continue;
    }
    const got = await paint(kind);
    if (!got) {
      skip(name, "the mark's rectangle is off screen or too small to sample");
      continue;
    }
    read[kind] = got;
    check(
      name,
      holds(got),
      `${(got.whole * 100).toFixed(0)}% of the rectangle, ` +
        `${(got.core * 100).toFixed(0)}% of its centre, ` +
        `ink on ${got.edges} of its 4 sides` +
        (got.sides.length ? ` (${got.sides.join(", ")})` : "") +
        `, ${got.corners} of its 4 corners, ` +
        `${(got.shoulder * 100).toFixed(0)}% of the strip above a rule, ` +
        `${(got.second * 100).toFixed(1)}% where a second line goes, ` +
        `${(got.lineOne * 100).toFixed(0)}%/${(got.lineTwo * 100).toFixed(0)}% in ` +
        `the two type bands, ink on ${got.rim} of its 3 border strips, ` +
        // The box's own size, because every sample above is a *fraction* of it
        // while a text box's type is a fixed 11 points --- so a reading only
        // means what it appears to mean beside the rectangle it was taken in.
        `in a ${got.box.width.toFixed(0)}x${got.box.height.toFixed(0)} px box`,
    );
  }

  // The discrimination itself, in one line. Every check above is a bound, and a
  // painter that drew one shape for everything could in principle satisfy a
  // subset of them; this says the nine kinds read as nine.
  //
  // **The key holds six of the nine readings, and the three added for the text
  // box are deliberately not in it.** A signature with more dimensions in it
  // collides less often, so adding them would make this pass more easily --- a
  // distinctness check is one of the few where a richer key is a *weaker*
  // check. Six already tell the nine kinds apart.
  //
  // **The index is the last entry of the list and moved when ink was added.**
  // Written as a literal it silently named ink's own check instead, which would
  // have reported the distinctness verdict under the drawing's name and left
  // the drawing's own unreported --- two wrong lines from one number.
  const shapes = new Set(
    Object.values(read).map(
      // `corners` is in the key because without it the box and the ellipse can
      // collide: their `whole` differs only by the length of a rounded corner
      // against a curve, and two readings that round to the same two decimals
      // would report a distinctness failure on a correct painter.
      (r) =>
        `${r.whole.toFixed(2)}/${r.core.toFixed(2)}/${r.edges}/${r.corners}/` +
        `${r.shoulder.toFixed(2)}/${r.second.toFixed(2)}`,
    ),
  );
  const distinct = INK_CHECK.distinct;
  if (Object.keys(read).length < wanted.length) {
    skip(distinct, "not every kind could be sampled");
  } else {
    check(
      distinct,
      shapes.size === wanted.length,
      `${shapes.size} distinct readings from ${wanted.length} kinds`,
    );
  }

  viewer.setMarks([]);
  await frame();
  await inkPreviewChecks(root, viewer, inked);
}

/**
 * How the phase names its results, keyed rather than indexed --- see
 * {@link INK_CHECK} for what an index cost the check above it.
 */
const AGREE_CHECK = {
  rendered: "the saved copy renders, so there are two pictures to compare",
  changed: "control: saving the marks changed what the file renders",
  untouched: "control: paper no mark was placed on renders identically",
  covered: "each mark covers as much of its rectangle in the file as on screen",
  hue: "each mark is the same colour in the file as on screen",
} as const;

const AGREE_CHECKS: string[] = Object.values(AGREE_CHECK);

/**
 * The overlay against the file, which is the one comparison nothing made.
 *
 * ## Why this exists
 *
 * Two renderers draw every mark. While the document is open the overlay paints
 * it from `markband.ts`; once it is saved PDFium paints the appearance stream
 * `save.rs` wrote. Each was measured --- {@link overlayInkChecks} reads the
 * overlay's pixels, `annot-probe --mode ink|rule|outline` reads the file's ---
 * and **each was measured against the model's own numbers, never against the
 * other**. `docs/PLAN.md` §10 question 8 states the residual in one line: they
 * can only diverge in colour, blend and inset, and no check compares any of the
 * three.
 *
 * That is not a theoretical seam. `markband.ts` is a deliberate second copy of
 * `save.rs`'s geometry constants across a language boundary, and its own module
 * comment says so. A second copy is exactly the thing that drifts, and the
 * reader sees the drift as their document changing under them at the moment
 * they save.
 *
 * ## The file's ink is isolated by diffing renders, not by its colour
 *
 * The obvious classifier --- "pixels whose hue matches the colour we sent" ---
 * cannot be used here, because the hue is the thing under test: counting only
 * hue-matching pixels and then comparing their mean hue is a check deriving its
 * input from its own subject, which this repository has a trap about.
 *
 * So the page is rendered **before** any mark is made and again from the saved
 * copy, and a file pixel counts as ink when the two differ. Page content
 * cancels exactly, the classifier knows nothing about colour, and the hue
 * comparison afterwards is then a reading rather than a restatement. It also
 * makes the check independent of what is under the mark: a highlight over dense
 * type and one over blank paper are both measured by what the mark added.
 *
 * ## What the two controls are for
 *
 * A diff of two identical pictures is empty, and an empty diff satisfies "the
 * file's coverage is not far from the overlay's" for every mark whose overlay
 * reading is also empty --- so the comparison could pass on a save that wrote
 * nothing at all. {@link AGREE_CHECK.changed} is what refuses that. Its mirror,
 * {@link AGREE_CHECK.untouched}, reads a band of the page no mark was placed on
 * and requires it to be **identical**: without it, a render that differs
 * everywhere --- a different scale, a stale tile, a reopened document laid out
 * differently --- would satisfy the first control and make every coverage
 * reading meaningless.
 */
export async function markAgreementChecks(
  path: string,
  doc: DocumentInfo,
  viewer: Viewer,
): Promise<void> {
  const scratch = await invoke<string | null>("viewercheck_scratch");
  if (!scratch) {
    for (const name of AGREE_CHECKS) {
      skip(name, "no scratch path was bound, so nothing can be saved and read back");
    }
    return;
  }

  const canvas = viewer.overlaySurface;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  const size = viewer.pageSize(0);
  if (!ctx || size.width_pt < 1 || size.height_pt < 1) {
    for (const name of AGREE_CHECKS) {
      skip(name, "the overlay has no 2d context, or the page has no size");
    }
    return;
  }

  viewer.goToPage(0);
  await settle(() => viewer.idle);

  // A whole page, at a scale that keeps the smaller dimension worth sampling.
  // The marks are laid out down the page, so the *height* is what has to carry
  // nine bands with enough pixels in each to measure --- a width-driven scale
  // reads a landscape page at a third of the resolution it needs.
  const scale = Math.min(4, Math.max(0.5, 900 / Math.max(size.width_pt, size.height_pt)));
  const px = {
    width: Math.round(size.width_pt * scale),
    height: Math.round(size.height_pt * scale),
  };
  const tile = async (id: number): Promise<Uint8ClampedArray | null> =>
    tileBytes({
      doc: id,
      page: 0,
      scale,
      x: 0,
      y: 0,
      width: px.width,
      height: px.height,
      format: "raw",
    });

  const before = await tile(doc.id);
  if (!before || before.length < px.width * px.height * 4) {
    for (const name of AGREE_CHECKS) {
      skip(name, "the page did not render before any mark was made");
    }
    return;
  }

  // Nine bands down the page, one kind each, with a tenth left bare for the
  // untouched control. Laid out rather than reusing one rectangle because two
  // synthetic marks addressed by page land on top of each other on a one-page
  // corpus --- the trap of that name --- and here they would also make every
  // per-mark diff carry its neighbour's ink.
  const KINDS = [
    "highlight",
    "underline",
    "squiggly",
    "strikeout",
    "square",
    "ellipse",
    "ink",
    "note",
    "textbox",
    "stamp",
  ] as const;
  const COLOR: [number, number, number] = [0.15, 0.35, 0.9];
  const top = size.height_pt * 0.06;
  const band = (size.height_pt * 0.78) / (KINDS.length + 1);
  const left = size.width_pt * 0.12;
  const right = size.width_pt * 0.88;
  // Under a third of a band, so the gap between one mark and the next is more
  // than twice the mark itself: a diff that bled into a neighbour would have to
  // travel further than any antialiasing can.
  const tall = Math.min(band * 0.3, 28);
  if (tall * scale < 6 || (right - left) * scale < 40) {
    for (const name of AGREE_CHECKS) {
      skip(name, `the bands come out at ${(tall * scale).toFixed(1)} px, too small to sample`);
    }
    return;
  }

  const placed = KINDS.map((kind, at) => {
    const y = top + band * at;
    const box =
      kind === "note"
        ? [left, y, left + 20, y + 20]
        : [left, y, right, y + tall];
    const strokes =
      kind === "ink"
        ? [[box[0] ?? 0, (box[1] ?? 0) + 2, box[2] ?? 0, (box[3] ?? 0) - 2]]
        : [];
    const lines = kind === "textbox" ? ["the reader", "typed this"] : [];
    return {
      id: 9100 + at,
      kind,
      page: pageId(1),
      quads: box,
      strokes,
      // The biconditional the model enforces, restated here because this phase
      // builds the overlay's marks itself: a name on any other kind is refused
      // and a stamp without one is too.
      stamp: kind === "stamp" ? ("draft" as const) : null,
      color: COLOR,
      width: INK_WIDTH,
      note: lines.join(" "),
      lines,
    };
  });

  // The overlay and the model are driven separately on purpose: this harness
  // builds its own `Viewer` with no model behind it, which is the gap
  // `markcheck.ts` exists to cover and is not this phase's subject. Both are
  // built from `placed`, so the two renderers are given one description of one
  // mark rather than two descriptions that could differ before either draws.
  viewer.setMarks(placed);
  await frame();
  await frame();

  const opened = await attempt("edit_state", { doc: doc.id });
  const page = opened.state?.pages[0]?.id;
  if (page === undefined) {
    for (const name of AGREE_CHECKS) {
      skip(name, opened.error || "the model reports no pages");
    }
    viewer.setMarks([]);
    return;
  }
  let madeError = "";
  // Aligned with `placed` by index, holding `undefined` where the model refused
  // one --- pushing only the successes would silently shift every later mark
  // onto its neighbour's reading, which is a defect that looks like a
  // disagreement between the renderers.
  const ids: (number | undefined)[] = [];
  for (const mark of placed) {
    // The command's own shape, which is not `MarkView`'s: the model mints the
    // id, and `author` is a field the overlay has no use for. Spreading the
    // overlay's object straight in was refused with `invalid args`, and the
    // `changed` control is what reported it --- nothing was saved, so nothing
    // moved in the file, which is exactly what that control is for.
    const { kind, quads, strokes, stamp, note, lines } = mark;
    const one = await attempt("annot_mark", {
      doc: doc.id,
      mark: { kind, page, quads, strokes, stamp, color: COLOR, author: "", note, lines },
    });
    madeError ||= one.error;
    ids.push(one.state?.marks[one.state.marks.length - 1]?.id);
  }

  // Not through `attempt`, which is typed for the commands that answer with an
  // `EditState`. This one answers with what it wrote.
  let saveError = "";
  try {
    await invoke("save_copy", { doc: doc.id, source: path, path: scratch });
  } catch (e) {
    saveError = String(e);
  }
  const second = saveError
    ? null
    : await invoke<DocumentInfo>("open_document", { path: scratch }).catch(() => null);
  const after = second ? await tile(second.id) : null;
  // The mark errors are part of this verdict rather than only its detail line.
  // The name is "there are two pictures to compare", and a run where the model
  // refused every mark renders two *identical* pictures --- which is a state
  // this phase cannot say anything from, and which passed here once with the
  // refusal printed in its own detail beside the `[OK]`.
  check(
    AGREE_CHECK.rendered,
    madeError === "" && after !== null && after.length === before.length,
    saveError
      ? preview(saveError)
      : madeError
        ? preview(madeError)
        : after
          ? `${px.width}x${px.height} px from both, ${ids.length} marks saved`
          : "the saved copy did not render",
  );
  if (madeError || !after || after.length !== before.length) {
    for (const name of AGREE_CHECKS.slice(1)) {
      skip(name, "there is only one picture, so there is nothing to compare");
    }
    viewer.setMarks([]);
    return;
  }

  /** Whether a pixel of the saved render differs from the same pixel before. */
  const moved = (at: number): boolean =>
    Math.abs((after[at] ?? 0) - (before[at] ?? 0)) +
      Math.abs((after[at + 1] ?? 0) - (before[at + 1] ?? 0)) +
      Math.abs((after[at + 2] ?? 0) - (before[at + 2] ?? 0)) >
    24;

  /** Coverage and mean hue of the ink the save added, over a page-space box. */
  const inFile = (box: number[]): { covered: number; hue: number | null } => {
    const x0 = Math.max(0, Math.round((box[0] ?? 0) * scale));
    const y0 = Math.max(0, Math.round((box[1] ?? 0) * scale));
    const x1 = Math.min(px.width, Math.round((box[2] ?? 0) * scale));
    const y1 = Math.min(px.height, Math.round((box[3] ?? 0) * scale));
    let hit = 0;
    let seen = 0;
    let r = 0;
    let g = 0;
    let b = 0;
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const at = (y * px.width + x) * 4;
        seen++;
        if (!moved(at)) continue;
        hit++;
        r += after[at] ?? 0;
        g += after[at + 1] ?? 0;
        b += after[at + 2] ?? 0;
      }
    }
    return {
      covered: seen === 0 ? 0 : hit / seen,
      hue: hit === 0 ? null : hueOf(r / hit, g / hit, b / hit),
    };
  };

  /** The same two readings taken off the overlay, over a mark's own anchor. */
  const onScreen = (id: number): { covered: number; hue: number | null } | null => {
    const at = viewer.markAnchor(id);
    if (!at) return null;
    const dpr = window.devicePixelRatio || 1;
    const x0 = Math.max(0, Math.round(at.left * dpr));
    const y0 = Math.max(0, Math.round(at.top * dpr));
    const x1 = Math.min(canvas.width, Math.round(at.right * dpr));
    const y1 = Math.min(canvas.height, Math.round(at.bottom * dpr));
    if (x1 - x0 < 2 || y1 - y0 < 2) return null;
    const { data } = ctx.getImageData(x0, y0, x1 - x0, y1 - y0);
    let hit = 0;
    let r = 0;
    let g = 0;
    let b = 0;
    for (let p = 0; p + 3 < data.length; p += 4) {
      if ((data[p + 3] ?? 0) <= 8) continue;
      hit++;
      r += data[p] ?? 0;
      g += data[p + 1] ?? 0;
      b += data[p + 2] ?? 0;
    }
    const total = data.length / 4;
    return {
      covered: total === 0 ? 0 : hit / total,
      hue: hit === 0 ? null : hueOf(r / hit, g / hit, b / hit),
    };
  };

  // The bare band below the last mark. Identical, not merely similar: any
  // difference here is the two renders disagreeing about something other than
  // the marks, and every reading above is a difference between those renders.
  const bare = [left, top + band * KINDS.length, right, top + band * KINDS.length + tall];
  const spare = inFile(bare);
  check(
    AGREE_CHECK.untouched,
    spare.covered < 0.01,
    `${(spare.covered * 100).toFixed(2)}% of the bare band moved`,
  );

  // **A text box's type is a fixed 11 points and these bands scale with the
  // paper**, so on a short page the phase's own mark has no room for a line and
  // both renderers correctly draw nothing. That is agreement, not a
  // disagreement, and it is excluded here rather than reported: `changed` would
  // otherwise call it a kind that put no ink in the file, which is true and is
  // a fact about this harness's rectangle.
  //
  // The band is `height_pt * 0.78 / 11 * 0.3`, so across the corpus it is 17.91
  // on `columns` at 842 points, **13.02** on `rotated-90` at 612, and 8.51 on
  // `inherited` at 400 --- against the 13.00 a first baseline needs. So this is
  // not a guard for one odd fixture: `rotated-90` has been passing by two
  // hundredths of a point, and any change to `TEXT_SIZE`, `TEXT_INSET` or the
  // band fractions flips it. This makes that margin visible rather than leaving
  // it to a rounding.
  //
  // The check *names* are untouched, which matters: `viewer_sweep.py` diffs
  // them across corpora as sets, and a phase that dropped a name on a short
  // page would read as a check that had stopped existing.
  const typeFits = tall >= TEXT_INSET + TEXT_SIZE;
  const readings = placed
    .map((mark, at) => ({
      kind: mark.kind,
      file: inFile(mark.quads),
      // The overlay is read only where the model also took the mark, so a
      // coverage comparison is always between two renderings of one mark rather
      // than between a drawing and an absence.
      screen: ids[at] === undefined ? null : onScreen(mark.id),
    }))
    .filter((read) => typeFits || read.kind !== "textbox");
  const shortPage = typeFits
    ? ""
    : `; the text box is left out, its band being ${tall.toFixed(1)} pt against the ${(
        TEXT_INSET + TEXT_SIZE
      ).toFixed(1)} one baseline needs`;
  const measured = readings.filter((r) => r.screen !== null);
  const somethingMoved = readings.filter((r) => r.file.covered > 0.01);
  check(
    AGREE_CHECK.changed,
    somethingMoved.length === readings.length,
    somethingMoved.length === readings.length
      ? `all ${readings.length} kinds put ink in the file${shortPage}`
      : `only ${somethingMoved.map((r) => r.kind).join(", ") || "none"} did${shortPage}`,
  );

  if (measured.length === 0) {
    skip(AGREE_CHECK.covered, "no mark reported an anchor on screen");
    skip(AGREE_CHECK.hue, "no mark reported an anchor on screen");
    viewer.setMarks([]);
    return;
  }

  // A ratio with a wide band, because the two renderers antialias differently
  // and sample at different resolutions --- and wide is still decisive, since
  // what this is aimed at is a kind drawn as the wrong *shape* in one of them.
  // A wash over a quad and a rule along its bottom differ by about fourteen
  // times, and a frame and a filled box by about ten.
  //
  // **Four, and the number is set by the text box rather than chosen.** Every
  // other kind is a shape both renderers draw from the same rectangle; a text
  // box is *type*, and by design not the same type --- the overlay draws what
  // the system resolved and the file is set in Helvetica by our own metrics,
  // which `docs/PLAN.md` says in as many words. Measured at 2.7x on `columns`,
  // 2.3% on screen against 6.4% in the file, which is the largest legitimate
  // disagreement in the set. The bound sits above it with margin and an order
  // of magnitude below the smallest defect it has to catch.
  const off = measured.map((r) => {
    const screen = r.screen?.covered ?? 0;
    const file = r.file.covered;
    return {
      kind: r.kind,
      ratio: Math.max(screen, file) / Math.max(0.004, Math.min(screen, file)),
      screen,
      file,
    };
  });
  const worst = off.reduce((a, b) => (a.ratio > b.ratio ? a : b));
  check(
    AGREE_CHECK.covered,
    off.every((r) => r.ratio < 4),
    `worst ${worst.kind}: ${(worst.screen * 100).toFixed(1)}% on screen against ` +
      `${(worst.file * 100).toFixed(1)}% in the file (${worst.ratio.toFixed(1)}x)`,
  );

  // Hue and not the whole colour. The overlay hands back its own paint
  // unpremultiplied; the file's reading is what a translucent mark composited
  // over paper came to. Those differ in how pale they are by design, and agree
  // on which colour it is --- which is the question a reader would ask.
  //
  // **The comment is excluded, and the reason is measured rather than assumed.**
  // `save.rs` writes no appearance stream for a `/Text` annotation on purpose,
  // because every reader synthesises its own icon --- so what PDFium paints here
  // is PDFium's house style, not ours. Sending blue read 224 degrees on screen
  // and **60** in the file; sending red read 0 on screen and **60** again. The
  // file is not at fault: it carries `/C` with the colour the reader chose. The
  // renderer ignores it, and no comparison between the two renderers can be
  // made for this kind while that is true.
  //
  // That leaves a real gap rather than a tidy one, and it is stated in
  // `docs/PLAN.md` §10 question 8: a reader who colours a comment sees their
  // colour until they save, and PDFium's yellow afterwards. Closing it means
  // writing an appearance stream for a comment, which `save.rs` argues against
  // for reasons that have nothing to do with this check.
  const hues = measured
    .filter((r) => r.kind !== "note")
    .map((r) => ({
      kind: r.kind,
      apart:
        r.file.hue === null || r.screen?.hue === undefined || r.screen.hue === null
          ? null
          : Math.min(
              Math.abs(r.file.hue - r.screen.hue),
              360 - Math.abs(r.file.hue - r.screen.hue),
            ),
      file: r.file.hue,
      screen: r.screen?.hue ?? null,
    }))
    .filter((r) => r.apart !== null);
  if (hues.length === 0) {
    skip(AGREE_CHECK.hue, "neither renderer put enough ink anywhere to have a colour");
  } else {
    const far = hues.reduce((a, b) => ((a.apart ?? 0) > (b.apart ?? 0) ? a : b));
    check(
      AGREE_CHECK.hue,
      hues.every((r) => (r.apart ?? 360) < 25),
      `worst ${far.kind}: ${far.screen?.toFixed(0)}deg on screen against ` +
        `${far.file?.toFixed(0)}deg in the file, ${far.apart?.toFixed(0)}deg apart ` +
        `(${hues.length} of ${KINDS.length} kinds read, the comment excluded)`,
    );
  }

  viewer.setMarks([]);
  for (const id of ids) {
    if (id !== undefined) await attempt("annot_remove", { doc: doc.id, mark: id, sweep: 0 });
  }
  // The copy is closed and the marks are taken off, so the phases after this one
  // meet the document this one was handed. `markCommandChecks` runs next and
  // asserts the model holds exactly one mark after making one, which a leftover
  // from here would break --- and it would break it as a failure in that phase,
  // three hundred lines from the cause.
  if (second) await invoke("close_document", { doc: second.id }).catch(() => undefined);
}
