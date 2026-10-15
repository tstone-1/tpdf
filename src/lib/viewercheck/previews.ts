/**
 * The previews of ink, a crop, the eraser and a whole mark.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { frame } from "../checkreport";
import { INK_WIDTH } from "../markband";
import { pageId } from "../pages";
import { Viewer } from "../viewer";
import {
  check,
  drag,
  pointer,
  skip,
} from "./common";
import { INK_CHECK } from "./ink";

/** Hue in degrees, or `null` for a pixel with no colour to speak of. */
export function hueOf(r: number, g: number, b: number): number | null {
  const high = Math.max(r, g, b);
  const low = Math.min(r, g, b);
  if (high - low < 12) return null;
  const span = high - low;
  const deg =
    high === r
      ? 60 * (((g - b) / span) % 6)
      : high === g
        ? 60 * ((b - r) / span + 2)
        : 60 * ((r - g) / span + 4);
  return (deg + 360) % 360;
}

/**
 * What a reader sees *while* drawing, which no committed mark can show.
 *
 * **This exists because the first version of ink previewed a freehand stroke as
 * a dashed rubber-band rectangle.** `paintDrawing` drew that for every live
 * drag, so the line appeared only on release --- and it shipped, because every
 * check in this phase reads marks the *model* holds, and a preview is by
 * definition not one of those. The overlay had a phase and the preview had
 * nothing.
 *
 * Two readings, and the second is what makes this about *drawings* rather than
 * about one stroke. A rubber band inks all four sides of its own box and leaves
 * the middle clear, which is also what a correct two-stroke drawing does --- so
 * "not a rectangle" is read from a single diagonal stroke, whose box a rectangle
 * would outline and whose line crosses the middle instead.
 */
export async function inkPreviewChecks(
  root: HTMLElement,
  viewer: Viewer,
  inked: (l: number, t: number, r: number, b: number) => number | null,
): Promise<void> {
  const names = [INK_CHECK.preview, INK_CHECK.second];
  // **Where page 1 is on screen, taken from a mark rather than from the
  // scroller.** `markAnchor` is the one thing here that answers in the root's
  // own coordinates, which is what `pointer` needs --- and it is what the phase
  // above already uses, so the two agree by construction rather than by two
  // copies of the same arithmetic.
  const size = viewer.pageSize(0);
  viewer.setMarks([
    {
      id: 4243,
      kind: "highlight",
      stamp: null,
      page: pageId(1),
      quads: [0, 0, size.width_pt, size.height_pt],
      strokes: [],
      color: [1, 0.9, 0.2],
      width: INK_WIDTH,
      note: "",
      lines: [],
    },
  ]);
  await frame();
  await frame();
  const box = viewer.markAnchor(4243);
  viewer.setMarks([]);
  await frame();
  if (!box || box.right - box.left < 80 || box.bottom - box.top < 80) {
    for (const name of names) skip(name, "page 1 is not on screen, or is too small to draw on");
    return;
  }
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  const x0 = box.left + width * 0.25;
  const y0 = box.top + height * 0.25;
  const x1 = box.left + width * 0.65;
  const y1 = box.top + height * 0.55;

  viewer.armDraw("ink");
  pointer(root, "pointerdown", x0, y0);
  for (let step = 1; step <= 6; step++) {
    const t = step / 6;
    pointer(root, "pointermove", x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
  }
  await frame();
  await frame();

  // The middle of the diagonal's own bounding box. A line through it inks this;
  // a rectangle outlining the same box leaves it empty, which is the whole
  // discrimination.
  const w = x1 - x0;
  const h = y1 - y0;
  const core = inked(x0 + w * 0.35, y0 + h * 0.35, x0 + w * 0.65, y0 + h * 0.65);
  const corner = inked(x0 + w * 0.02, y0 + h * 0.7, x0 + w * 0.3, y0 + h * 0.98);
  check(
    INK_CHECK.preview,
    core !== null && core > 0.02 && (corner ?? 1) < 0.02,
    `${((core ?? 0) * 100).toFixed(0)}% down the diagonal, ` +
      `${((corner ?? 0) * 100).toFixed(0)}% in the corner a rubber band would outline`,
  );

  pointer(root, "pointerup", x1, y1);
  await frame();

  // And a second stroke elsewhere, which must not replace the first. Read as
  // the viewer's own count rather than in pixels: the two strokes are far apart
  // and a band that held one of them would be measuring position, not identity.
  const before = viewer.drawnStrokes;
  drag(
    root,
    [box.left + width * 0.2, box.top + height * 0.75],
    [box.left + width * 0.5, box.top + height * 0.8],
  );
  await frame();
  check(
    INK_CHECK.second,
    before === 1 && viewer.drawnStrokes === 2,
    `${before} stroke after the first, ${viewer.drawnStrokes} after the second`,
  );

  // Escape rather than Enter: this phase must leave the document as it found
  // it, and Enter would put a mark on page 1 that every later phase would see.
  viewer.cancelDraw();
  await frame();
  await erasePreviewChecks(root, viewer, inked, box);
  // A sibling of the phase above rather than a tail call inside it, and that is
  // not style. `erasePreviewChecks` returns early when the drawing did not
  // render, skipping its own two names --- and a call after that return would
  // print nothing at all on any corpus that took it, which the sweep reads as
  // two names going missing rather than as two skips.
  await wholeMarkPreviewChecks(root, viewer, inked, box);
  await cropPreviewChecks(root, viewer, inked, box);
}

/**
 * What a reader sees *while* dragging a crop.
 *
 * **The only check on `paintCropPreview`, and the only one there can be.** A
 * crop's preview exists between a press and a release and reaches no model, so
 * `viewercrop.test.ts` --- which drives the same gesture against a fake DOM ---
 * cannot see a pixel of it, and no phase reading the model or the saved file can
 * either. The overlay is where it exists.
 *
 * **The two readings are one assertion, not one plus thoroughness.** The scrim
 * is a darkening of everything *outside* the rectangle, so "the outside is
 * covered" is satisfied by a blanket over the whole page --- which is the most
 * likely way to get this wrong, and it would hide the page a reader is aiming
 * with. What says it is a hole rather than a blanket is that the inside stays
 * clear, and neither reading alone says that.
 *
 * Left mid-drag for exactly as long as the readings take, then cancelled: this
 * phase, like the two above it, must hand the document on as it found it.
 */
async function cropPreviewChecks(
  root: HTMLElement,
  viewer: Viewer,
  inked: (l: number, t: number, r: number, b: number) => number | null,
  box: { left: number; top: number; right: number; bottom: number },
): Promise<void> {
  const names = [INK_CHECK.scrimOut, INK_CHECK.scrimIn];
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  if (width < 80 || height < 80) {
    for (const name of names) skip(name, "page 1 is too small to drag a crop on");
    return;
  }
  // The middle half of the page, so there is room outside it on all four sides
  // for the scrim to be read in and room inside for the hole.
  const x0 = box.left + width * 0.25;
  const y0 = box.top + height * 0.25;
  const x1 = box.left + width * 0.75;
  const y1 = box.top + height * 0.75;

  viewer.armCrop();
  pointer(root, "pointerdown", x0, y0);
  pointer(root, "pointermove", x1, y1);
  await frame();
  await frame();

  // Outside the rectangle and inside the page: the band along the top, which
  // the scrim covers wholly. Read above the rectangle rather than beside it
  // because the top band is the full width of the page in the painter, so a
  // partial reading there would be the painter's fault rather than the check's.
  const outside = inked(box.left + 2, box.top + 2, box.right - 2, y0 - 2);
  // Well inside the rectangle, clear of the dashed outline on every side.
  const inside = inked(
    x0 + width * 0.06,
    y0 + height * 0.06,
    x1 - width * 0.06,
    y1 - height * 0.06,
  );

  // Cancelled before the checks are recorded, so that a check throwing cannot
  // leave the tool armed for the phases after this one.
  viewer.cancelDraw();
  await frame();

  check(
    INK_CHECK.scrimOut,
    outside !== null && outside > 0.9,
    `${((outside ?? 0) * 100).toFixed(0)}% of the band above the rectangle is covered`,
  );
  check(
    INK_CHECK.scrimIn,
    inside !== null && inside < 0.02,
    `${((inside ?? 0) * 100).toFixed(0)}% of the rectangle's middle is covered`,
  );
}

/**
 * What a reader sees *while* rubbing something out.
 *
 * **The preview is an absence, which is what makes it hard to check and easy to
 * get wrong.** A stroke the sweep has taken simply stops being painted; there
 * is no ghost to compare against, so the only reading available is the pixels
 * where the stroke used to be. That also means the check needs a *second*
 * stroke as its control: "the band is empty" is satisfied by an overlay that
 * stopped painting the drawing altogether, which is exactly the defect a
 * one-band reading would certify.
 *
 * Runs on a mark handed straight to the viewer rather than one the model holds,
 * for the reason the phase above does: the document must be as this found it.
 */
async function erasePreviewChecks(
  root: HTMLElement,
  viewer: Viewer,
  inked: (l: number, t: number, r: number, b: number) => number | null,
  box: { left: number; top: number; right: number; bottom: number },
): Promise<void> {
  const names = [INK_CHECK.erased, INK_CHECK.spared];
  const size = viewer.pageSize(0);
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  // Two rules across the page in its own points, a third of the way down and
  // two thirds. Far enough apart that the nib cannot reach both.
  const upper = size.height_pt * 0.35;
  const lower = size.height_pt * 0.65;
  const from = size.width_pt * 0.2;
  const to = size.width_pt * 0.8;
  viewer.setMarks([
    {
      id: 4244,
      kind: "ink",
      stamp: null,
      page: pageId(1),
      quads: [from, upper, to, lower],
      strokes: [
        [from, upper, to, upper],
        [from, lower, to, lower],
      ],
      color: [0.85, 0.15, 0.15],
      width: INK_WIDTH,
      note: "",
      lines: [],
    },
  ]);
  await frame();
  await frame();

  /** A thin band across one of the two rules, in the root's coordinates. */
  const band = (atY: number) => {
    const y = box.top + (atY / size.height_pt) * height;
    return [box.left + width * 0.25, y - 4, box.left + width * 0.75, y + 4] as const;
  };
  const before = inked(...band(upper));
  if (before === null || before < 0.02) {
    for (const name of names) {
      skip(name, "the drawing is not on screen, or its strokes drew nothing to erase");
    }
    viewer.setMarks([]);
    await frame();
    return;
  }

  viewer.armErase();
  // Held, not released: what is being measured is the preview, and a release
  // would send the sweep to a model that does not hold this mark.
  const y = box.top + (upper / size.height_pt) * height;
  pointer(root, "pointerdown", box.left + width * 0.25, y);
  pointer(root, "pointermove", box.left + width * 0.75, y);
  await frame();
  await frame();

  const gone = inked(...band(upper));
  const kept = inked(...band(lower));
  check(
    INK_CHECK.erased,
    (gone ?? 1) < 0.02,
    `${((before ?? 0) * 100).toFixed(0)}% of the band before the nib, ` +
      `${((gone ?? 0) * 100).toFixed(0)}% after it`,
  );
  // The control, and it is not a formality: an overlay that stopped painting
  // the whole drawing satisfies the check above perfectly.
  check(
    INK_CHECK.spared,
    (kept ?? 0) > 0.02,
    `${((kept ?? 0) * 100).toFixed(0)}% of the band the nib did not cross`,
  );

  viewer.cancelDraw();
  viewer.setMarks([]);
  await frame();
}

/**
 * The same preview, for a mark that has no strokes to lose.
 *
 * The eraser takes a highlight, a box, a note or a stamp **whole**, so what the
 * reader watches is the mark itself going rather than a drawing coming apart.
 * The reading is the same one and so is the trap: *the band is empty* is
 * satisfied by an overlay that stopped painting marks altogether, so the second
 * highlight beside it is the control and not a formality.
 *
 * Two washes rather than a wash and a drawing, because the point under test is
 * that this path is chosen by the kind: a fixture with ink in it would let a
 * check pass on the stroke loop's behaviour.
 */
async function wholeMarkPreviewChecks(
  root: HTMLElement,
  viewer: Viewer,
  inked: (l: number, t: number, r: number, b: number) => number | null,
  box: { left: number; top: number; right: number; bottom: number },
): Promise<void> {
  const names = [INK_CHECK.taken, INK_CHECK.left];
  const size = viewer.pageSize(0);
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  // Two washes across the page, a third of the way down and two thirds, each
  // 20 points tall. Far enough apart that the nib cannot reach both.
  const upper = size.height_pt * 0.35;
  const lower = size.height_pt * 0.65;
  const from = size.width_pt * 0.2;
  const to = size.width_pt * 0.8;
  viewer.setMarks([
    {
      id: 4245,
      kind: "highlight",
      stamp: null,
      page: pageId(1),
      quads: [from, upper - 10, to, upper + 10],
      strokes: [],
      color: [0.85, 0.15, 0.15],
      width: INK_WIDTH,
      note: "",
      lines: [],
    },
    {
      id: 4246,
      kind: "highlight",
      stamp: null,
      page: pageId(1),
      quads: [from, lower - 10, to, lower + 10],
      strokes: [],
      color: [0.85, 0.15, 0.15],
      width: INK_WIDTH,
      note: "",
      lines: [],
    },
  ]);
  await frame();
  await frame();

  /** A thin band across one of the two washes, in the root's coordinates. */
  const band = (atY: number) => {
    const y = box.top + (atY / size.height_pt) * height;
    return [box.left + width * 0.25, y - 4, box.left + width * 0.75, y + 4] as const;
  };
  const before = inked(...band(upper));
  if (before === null || before < 0.02) {
    for (const name of names) {
      skip(name, "the page is not on screen, or the washes drew nothing to erase");
    }
    viewer.setMarks([]);
    await frame();
    return;
  }

  viewer.armErase();
  // Held, not released, for the reason the phase above gives: the reading is
  // the preview, and a release would send the sweep to a model that does not
  // hold these marks.
  const y = box.top + (upper / size.height_pt) * height;
  pointer(root, "pointerdown", box.left + width * 0.25, y);
  pointer(root, "pointermove", box.left + width * 0.75, y);
  await frame();
  await frame();

  const gone = inked(...band(upper));
  const kept = inked(...band(lower));
  check(
    INK_CHECK.taken,
    (gone ?? 1) < 0.02,
    `${((before ?? 0) * 100).toFixed(0)}% of the wash before the nib, ` +
      `${((gone ?? 0) * 100).toFixed(0)}% after it`,
  );
  check(
    INK_CHECK.left,
    (kept ?? 0) > 0.02,
    `${((kept ?? 0) * 100).toFixed(0)}% of the wash the nib did not cross`,
  );

  viewer.cancelDraw();
  viewer.setMarks([]);
  await frame();
}
