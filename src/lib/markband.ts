/**
 * Where a mark's ink goes inside the text it marks.
 *
 * ## Why this exists
 *
 * The overlay drew every mark the same way: `fillRect` over the whole quad in
 * one colour, for all three kinds. So while a document was open an underline
 * and a strikeout both looked exactly like a highlight, and a reader who chose
 * "Underline selection" from the right-click menu got a yellow wash over the
 * line. Reported from use.
 *
 * The saved file was right the whole time --- `save.rs` writes the correct
 * subtype and an appearance stream whose geometry `annot-probe --mode rule`
 * measures --- which made this the worse shape of wrong: the reader could not
 * tell what they had made until they saved the file and opened it again, and at
 * that point the mark changed under them.
 *
 * ## The rule is `save.rs`'s, restated here rather than invented
 *
 * `paint`, `LINE_FRACTION`, `line_rect` and `OUTLINE_WIDTH` in
 * `src-tauri/src/save.rs` decide this for the file, and what a reader sees on screen has to be the same
 * decision or the two disagree about what a mark *is*. This is a second copy of
 * a constant across a language boundary, which this repository has a trap about;
 * the honest mitigation is that it is stated in one function with the Rust names
 * beside it, tested against the same numbers, and that {@link markBand} is the
 * only thing the overlay asks.
 *
 * A third statement would be the one to refuse. A kind added anywhere has to be
 * added here, in `save.rs`, and in `MarkKind` --- and that is not advice, it is
 * what the exhaustive `switch` below and the exhaustive `match` there both
 * enforce as compile errors.
 *
 * ## Two kinds the document did not place
 *
 * Three of the five are anchored to words a reader selected, so their quads come
 * from the text layer and this file only decides where the ink goes inside them.
 * A comment and a box are the other case: the reader chose the rectangle, so the
 * rectangle itself is decided here --- {@link iconQuad} for a point, {@link
 * boxQuad} for a drag --- and both have to be clamped into the page, because
 * `save.rs` maps quads and does not police them.
 */

import type { MarkKind } from "./pages";

/**
 * A line's thickness as a fraction of the marked text's height.
 *
 * `LINE_FRACTION` in `save.rs`, and proportional for the reason stated there: a
 * fixed 1 pt rule across 36 pt type is a hairline, and a reader who cannot see
 * the line they just drew draws it again.
 */
export const LINE_FRACTION = 0.07;

/**
 * How tall a squiggle's band is, as a fraction of the marked text's height.
 *
 * `SQUIGGLE_HEIGHT` in `save.rs`. Peak to trough, from the bottom of the quad
 * up, and **larger than {@link LINE_FRACTION} on purpose**: the gap between the
 * two is what makes a squiggle distinguishable from an underline rather than a
 * wobbly one, and every check that tells the kinds apart reads inside it.
 */
export const SQUIGGLE_HEIGHT = 0.18;

/**
 * One full cycle of a squiggle, as a multiple of its band's height.
 *
 * `SQUIGGLE_PERIOD` in `save.rs`. Tied to the band rather than to the quad's
 * width, so a run of two words and a run of two lines get the same wave rather
 * than the same number of cycles.
 */
export const SQUIGGLE_PERIOD = 2;

/**
 * The size a text box's words are set at, in points.
 *
 * `textbox::SIZE` in Rust. Fixed rather than proportional to the box: nothing
 * about how large a reader dragged the rectangle says how large they want the
 * words.
 */
export const TEXT_SIZE = 11;

/** Leading, as a multiple of {@link TEXT_SIZE}. `textbox::LEADING` in Rust. */
export const TEXT_LEADING = 1.2;

/**
 * The inset from the rectangle's edge to the first glyph, in points.
 *
 * `textbox::INSET` in Rust, and it has to agree with it for a reason the other
 * mirrored constants do not have: the backend subtracts this from the box's
 * width before wrapping, so a different value here does not merely shift the
 * text, it draws lines that were measured against a width the overlay is not
 * using.
 */
export const TEXT_INSET = 2;

/** A quad in the page's display space, as the overlay holds one. */
export interface Quad {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Whether a kind covers its quads or draws a line across them.
 *
 * `is_wash` in `save.rs`. It decides the geometry here and the blend mode at the
 * call site, which is the same pair of things it decides for the saved file: a
 * wash multiplies with the text so the words stay readable underneath, and a
 * line is opaque and on top, because a translucent line reads as a smudge.
 */
export function isWash(kind: MarkKind): boolean {
  return kind === "highlight";
}

/**
 * Whether a kind draws a picture rather than ink over words.
 *
 * `Ink::None` in `save.rs`, and the same pair of questions: {@link isWash} asks
 * how ink is laid down, this asks whether there is any ink over the text at all.
 * A comment is a bubble sitting on the paper --- it covers nothing and it is not
 * anchored to words.
 *
 * **Not `is_note`, which is a narrower question there.** That predicate used to
 * answer this one too, and the box separated them: on the file's side a comment
 * is the kind that gets *no appearance stream of ours*, because every reader
 * synthesises its own icon. Here it is the kind drawn as a picture rather than
 * as ink over words. The two coincide for now and are not the same question, so
 * the name that travels is the one that will still be right when they part.
 */
export function isIcon(kind: MarkKind): boolean {
  return kind === "note";
}

/**
 * Whether a kind is drawn as an outline rather than filled.
 *
 * `Ink::Outline` in `save.rs`. A box is the first kind whose ink is a stroke:
 * the three text marks and the comment bubble all fill something, and a filled
 * box would hide whatever a reader drew it around, which is the one thing a box
 * exists not to do.
 */
export function isOutline(kind: MarkKind): boolean {
  return kind === "square";
}

/**
 * Whether a kind is drawn as the ellipse inscribed in its rectangle.
 *
 * `Paint::Ellipse` in `save.rs`, and separate from {@link isOutline} for the
 * reason that variant is separate from `Paint::Outline`: the two differ in their
 * geometry, which is the one thing these predicates exist to decide. Folding
 * them into one would mean a caller that asks the kind again after asking this,
 * which is a second copy of a distinction already made here.
 *
 * **Its rectangle is not its ink.** A box's stroke runs along the edge of its
 * quad; this one touches that edge at four points and is inside it everywhere
 * else. Nothing downstream cares --- the popup anchor, `/Rect` and the hit test
 * all want the rectangle, and the rectangle is what the reader dragged.
 */
export function isEllipse(kind: MarkKind): boolean {
  return kind === "ellipse";
}

/**
 * Whether a kind is drawn as a wave along the bottom of its band.
 *
 * `Paint::Wave` in `save.rs`, and separate from every predicate here for the
 * reason they are all separate: the geometry differs. This is the one that
 * stops a squiggle being painted as a filled band --- which is what
 * {@link markBand} returns for it, and what the overlay would draw without
 * being told otherwise. That would put a solid red bar under the words, two and
 * a half times the height of an underline, and the saved file would be right
 * the whole time: the underline defect's shape exactly.
 */
export function isWave(kind: MarkKind): boolean {
  return kind === "squiggly";
}

/**
 * Whether a kind is drawn as lines of type rather than as a shape.
 *
 * `Paint::Text` in `save.rs`. **The one predicate here whose kind draws
 * something that is not derived from its rectangle at all** --- {@link isPath}
 * is the other, and even ink's strokes are geometry. What a text box draws is
 * the reader's words, which arrive already broken into lines by the backend, so
 * the overlay places them and measures nothing.
 *
 * Without it a text box falls through to the final `fillRect` and a reader sees
 * a solid red rectangle where they typed --- and the saved file, drawn by
 * `Paint::Text`, has the words in it the whole time.
 */
export function isText(kind: MarkKind): boolean {
  return kind === "textbox";
}

/**
 * Whether a kind is a stamp: a border round its quad and a word across it.
 *
 * **Its own predicate rather than `isOutline` plus a note**, because a stamp
 * that answered `isOutline` would be drawn as an empty box and the file would
 * be right --- which is the shape of the underline defect this file's own header
 * describes, and the reason `--mode stamp` exists beside `--mode outline` in
 * `annot-probe`. Two renderers, one of them wrong, nothing red.
 */
export function isStamp(kind: MarkKind): boolean {
  return kind === "stamp";
}

/**
 * A capital's height as a fraction of the font size, for Helvetica.
 *
 * `STAMP_CAP` in `save.rs`, and the same argument for it: every word a stamp
 * draws is upper case, so the ink's height is the cap height and not the size,
 * which includes descender space no stamp uses.
 */
export const STAMP_CAP = 0.718;

/**
 * How far a stamp's word sits inside its border, in points.
 *
 * `STAMP_INSET` in `save.rs`.
 */
export const STAMP_INSET = 4;

/**
 * Whether a kind is drawn from strokes rather than from its rectangle.
 *
 * `Paint::Path` in `save.rs`. **The first kind whose quad is not its shape**:
 * every other mark is its rectangle, a band inside it, or its edge, and this one
 * is a path that merely happens to fit in one. So an overlay that painted from
 * {@link markBand} would draw a filled box where a reader drew a line --- which
 * is the same class of defect as the underline that looked like a highlight,
 * and the reason that one is worth remembering is that the *file* was right
 * throughout.
 *
 * It is also what the eraser branches on: a drawing loses the strokes the nib
 * crossed and everything else is taken whole, so *what has parts* is decided
 * here once rather than in the sweep. This comment had been **orphaned** above
 * {@link isStamp}'s own since the stamp landed, so `isPath` --- the predicate
 * two subsystems ask --- carried no documentation at all and nothing said so.
 */
export function isPath(kind: MarkKind): boolean {
  return kind === "ink";
}

/**
 * Whether a reader can drag a mark to somewhere else on the page.
 *
 * **The only predicate here that is not about drawing**, and it is a product
 * rule rather than a geometric one: `Doc::displace` will move any mark, because
 * every mark has a rectangle and moving one is arithmetic. What decides whether
 * to *offer* it is what the mark is made of.
 *
 * A highlight, an underline, a strikeout and a squiggle are made **of words**.
 * Their rectangles came off a text selection, so dragging one somewhere else
 * leaves a wash sitting over a line it does not mark --- and the reader has no
 * way to put it back except undo, since there is nothing to snap to. The mark to
 * make there is a new one over the words that were meant.
 *
 * Everything else the reader placed: a comment they dropped, a box and an
 * ellipse they dragged out, a text box they typed in, a drawing they made. None
 * of those is anchored to anything, so where it sits is theirs to change --- and
 * that a comment could not be moved is exactly what was reported.
 */
export function isMovable(kind: MarkKind): boolean {
  // **Exhaustive on purpose, where every predicate above is an equality test.**
  // Those answer a question about one kind and a new kind simply is not it; this
  // answers a question about all of them, and a default would quietly decide it
  // for whatever is added next --- in whichever direction the default happened
  // to point. Written out, the ninth kind is a compile error here until somebody
  // says which half it is in.
  switch (kind) {
    case "highlight":
    case "underline":
    case "strikeout":
    case "squiggly":
      return false;
    case "note":
    case "square":
    case "ellipse":
    case "textbox":
    case "ink":
    // A stamp is placed by the reader and anchored to nothing, so it moves for
    // the box's reason exactly.
    case "stamp":
      return true;
  }
}

/**
 * How thick a freehand line is, in points, before any zoom.
 *
 * `INK_WIDTH` in `save.rs`. Heavier than {@link OUTLINE_WIDTH} for the reason
 * stated there: a box is a frame round something and should not compete with
 * it, while a drawn line *is* the content, and at hairline weight freehand ink
 * reads as tentative and breaks up wherever the pointer moved fast.
 */
export const INK_WIDTH = 2.5;

/**
 * How far the pointer must move before a stroke keeps another point, in points.
 *
 * **No counterpart in `save.rs`**, and it is the one constant here that is not
 * mirroring one: sampling is a property of the input device, and the writer
 * receives whatever the drag decided. It is in this file because it belongs
 * beside the ink constants a reader of either would look for together.
 *
 * Half a point, in the *page's* space rather than in client pixels, so a line
 * drawn at 400% carries the same number of points as the identical line drawn at
 * 100%. At 2.5 pt of line width it is well under what any hand can see, and it
 * takes a slow diagonal across a page from thousands of points to hundreds.
 */
export const INK_SAMPLE = 0.5;

/**
 * How thick a box's outline is, in points, before any zoom.
 *
 * `OUTLINE_WIDTH` in `save.rs`. Fixed rather than proportional, which is the
 * opposite of {@link LINE_FRACTION} and for a reason that does not carry over: a
 * line through text scales with the text because the text decides how big the
 * mark is, and nothing decides how big a box is except the reader. A border that
 * grew with the rectangle would make a box round a figure four times heavier
 * than one round a word.
 */
export const OUTLINE_WIDTH = 1.5;

/**
 * How close the eraser has to pass to a stroke to take it, in *view pixels*.
 *
 * **Screen pixels rather than the page's own points**, which is the opposite
 * choice from {@link INK_SAMPLE} one constant above and worth saying why. A
 * sample interval is about the fidelity of the line that gets stored, so it
 * belongs to the paper; an eraser is a thing the reader aims with a pointer, and
 * an eraser that shrank on screen as they zoomed in would get harder to hit a
 * stroke with exactly when they are trying to be precise. At every zoom this is
 * the same-sized nib under the cursor.
 *
 * Six rather than two: a stroke is 2.5 pt of line and the pointer is aimed by
 * hand, so the nib has to forgive a near miss.
 *
 * ⚠ **This said it is "deliberately smaller than the ring a press uses to find
 * a mark", and that is false at every zoom below 200%.** The two are measured
 * in different units, so the comparison moves with the zoom and no single
 * sentence about it can be right. `HIT_SLACK_PT` is 3 **points** and this is 6
 * **view pixels**, which the sweep divides by the zoom --- so in the page's own
 * points the nib is 12 pt at 50%, 6 pt at 100%, 4 pt at 150%, and only at 200%
 * does it become the 3 pt the press ring always is. Measured rather than
 * reasoned about, and a reader is below 200% almost all the time.
 *
 * So the eraser is *more* forgiving than a press, in the direction the old
 * sentence said it must not be, and the argument it gave --- taking the wrong
 * mark is a loss and opening the wrong note is not --- still stands. Recorded
 * rather than changed: whether the nib should be clamped to `HIT_SLACK_PT` in
 * page points is a question about how the tool feels when a reader is zoomed
 * out, and an eraser that is hard to hit is the complaint that gets reported.
 * `docs/PLAN.md` has it as a ranked question.
 */
export const ERASER_RADIUS = 6;

/**
 * The sentence the status line shows while the eraser is armed.
 *
 * **Here rather than in `App.svelte`, and that is the point of it.** Every
 * other phrase the status line builds lives in the window's own script, where
 * no unit test imports it and the window harness --- which builds a `Viewer` of
 * its own and never renders the application's header --- cannot reach it
 * either. So the words a reader actually reads had no check of any kind, which
 * is the shape this repository records as *the window reads the status and the
 * tests read the viewer*. It sits beside {@link ERASER_RADIUS} because this
 * file already owns what the nib is; what it says is the same subject.
 *
 * Both counts, because a sweep can take three strokes out of a drawing and a
 * highlight beside it, and "3 strokes" would be a lie about the highlight. A
 * zero half is left out rather than printed, so the common case --- one kind of
 * thing --- reads as one clause.
 */
export function sweepLabel(taken: { strokes: number; marks: number }): string {
  const parts: string[] = [];
  if (taken.strokes > 0) {
    parts.push(`${taken.strokes} stroke${taken.strokes === 1 ? "" : "s"}`);
  }
  if (taken.marks > 0) {
    parts.push(`${taken.marks} mark${taken.marks === 1 ? "" : "s"}`);
  }
  // "a mark", not "a drawing": the nib takes every kind now, and a reader told
  // to drag across a drawing would not try it on the highlight they want gone.
  return parts.length === 0
    ? "Erasing — drag across a mark"
    : `Erasing: ${parts.join(", ")}`;
}

/**
 * Whether `at` is within `radius` of the polyline `points`.
 *
 * Distance to the nearest *segment*, not to the nearest recorded point, and the
 * difference is the whole of it: a fast hand leaves points far apart, so a
 * nearest-point test would let the eraser pass straight through the middle of a
 * long stroke without touching it. Points are in whatever space the caller is
 * working in and `radius` has to match.
 *
 * ⚠ **This said the viewer hands both "in view pixels", and it does not.** It
 * hands them in the slot's **laid-out points** --- `viewRectOn` applies the crop
 * and the turns and no zoom at all --- and converts only the radius, with
 * `ERASER_RADIUS / this.zoom`. That distinction is what made the false
 * comparison in {@link ERASER_RADIUS}'s own comment easy to write, so it is
 * corrected here in the same breath: the *constant* is view pixels, the
 * *comparison* is points.
 *
 * A stroke of one point cannot be drawn --- the model refuses it --- but this
 * still answers for one, as a plain point distance, rather than returning false
 * for input it will never see. A geometry helper that is wrong on a degenerate
 * case is a helper somebody will one day call from somewhere else.
 */
export function strokeTouches(
  points: { x: number; y: number }[],
  at: { x: number; y: number },
  radius: number,
): boolean {
  return strokeSwept(points, at, at, radius);
}

/**
 * Whether the nib travelling from `from` to `to` comes within `radius` of the
 * polyline `points`.
 *
 * **The travel, not the two ends of it**, and that is not a refinement: a
 * pointer reports at the display's rate and a hand crosses several strokes
 * between two reports, so testing the sampled positions alone lets a quick
 * sweep pass straight over a stroke and leave it there. Found by a test that
 * dragged down a column of three strokes and got the outer two --- the middle
 * one lay between the samples. It is the same failure {@link strokeTouches}
 * already avoids *inside* a stroke, arriving one level up.
 *
 * A press is `from === to`, which is a segment of no length and needs no
 * special case: the arithmetic below clamps to a point and answers the point
 * distance.
 */
export function strokeSwept(
  points: { x: number; y: number }[],
  from: { x: number; y: number },
  to: { x: number; y: number },
  radius: number,
): boolean {
  if (points.length === 0) return false;
  const within = radius * radius;
  const first = points[0];
  if (first === undefined) return false;
  if (points.length === 1) {
    return pointToSegment(first, from, to) <= within;
  }
  for (let index = 0; index + 1 < points.length; index += 1) {
    const a = points[index];
    const b = points[index + 1];
    if (a === undefined || b === undefined) continue;
    // Crossing segments are at distance zero and no endpoint of either need be
    // near an endpoint of the other --- an X of two long strokes is the case,
    // and the four endpoint distances below are all large for it.
    if (segmentsCross(a, b, from, to)) return true;
    const nearest = Math.min(
      pointToSegment(a, from, to),
      pointToSegment(b, from, to),
      pointToSegment(from, a, b),
      pointToSegment(to, a, b),
    );
    if (nearest <= within) return true;
  }
  return false;
}

/**
 * Whether the nib travelling from `from` to `to` touches the rectangle `quad`.
 *
 * The eraser's rule for every mark that is **not** a drawing --- a highlight, a
 * box, an ellipse, a text box, a stamp, a note --- where {@link strokeSwept} is
 * the rule for the one that is. The rectangle, the points and `radius` are in
 * whatever space the caller works in --- the viewer's is the slot's laid-out
 * points, with the nib converted into them; see {@link strokeTouches}, which had
 * that written down the other way round for months.
 *
 * **The whole rectangle counts, not the mark's own ink**, and that is the
 * decision rather than an approximation. A box's ink is its border and an
 * ellipse's is a curve inside its quad, so a nib passing through the empty
 * middle of either touches nothing a reader can see --- and taking the mark
 * anyway is still right, because this is the same rectangle a press already
 * uses to open that mark's note. One answer to "where is this mark" beats two
 * that agree today. The alternative is a per-kind geometry here, which would be
 * a second copy of what {@link markBand} decides for the painter, and a copy of
 * a distinction is what this file's own header watches for.
 *
 * The honest cost: a reader who drags the eraser across the hollow middle of a
 * large box loses the box.
 *
 * `to` is deliberately not tested for containment, and it is not an omission. A
 * segment lying wholly inside the rectangle has `from` inside it; one that is
 * partly inside crosses the boundary, which the polyline below answers. So a
 * second containment test could never be the only thing that fired, and a term
 * no input can reach is a term no mutation can kill.
 */
export function quadSwept(
  quad: Quad,
  from: { x: number; y: number },
  to: { x: number; y: number },
  radius: number,
): boolean {
  if (
    from.x >= quad.left &&
    from.x <= quad.right &&
    from.y >= quad.top &&
    from.y <= quad.bottom
  ) {
    return true;
  }
  // Closed: the last point repeats the first, so the left edge is a segment
  // like the other three rather than the gap between the ends of an open line.
  return strokeSwept(
    [
      { x: quad.left, y: quad.top },
      { x: quad.right, y: quad.top },
      { x: quad.right, y: quad.bottom },
      { x: quad.left, y: quad.bottom },
      { x: quad.left, y: quad.top },
    ],
    from,
    to,
    radius,
  );
}

/**
 * Squared distance from `p` to the segment `a`--`b`.
 *
 * Clamped to the segment, so a point beyond either end measures to that end
 * rather than to the infinite line the segment sits on --- which would let the
 * eraser take a stroke it passed nowhere near, along that stroke's own
 * direction. Squared, because every caller compares against a squared radius
 * and a square root would round.
 */
function pointToSegment(
  p: { x: number; y: number },
  a: { x: number; y: number },
  b: { x: number; y: number },
): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = dx * dx + dy * dy;
  const along =
    length === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));
  const nx = a.x + along * dx;
  const ny = a.y + along * dy;
  return (p.x - nx) ** 2 + (p.y - ny) ** 2;
}

/** Whether the segments `a`--`b` and `c`--`d` properly cross. */
function segmentsCross(
  a: { x: number; y: number },
  b: { x: number; y: number },
  c: { x: number; y: number },
  d: { x: number; y: number },
): boolean {
  const side = (
    p: { x: number; y: number },
    q: { x: number; y: number },
    r: { x: number; y: number },
  ) => Math.sign((q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x));
  // Collinear and touching cases are deliberately left to the endpoint
  // distances above, which answer them: this only has to catch the crossing
  // that no endpoint pair is near.
  return (
    side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0
  );
}

/**
 * The smallest box a drag may commit, in points.
 *
 * A click without a drag is a rectangle of no size, and one saved is an
 * annotation nothing draws and nobody can find again to remove --- so it reads
 * as a command that silently did nothing. Below this the drag is refused and
 * the reader keeps the armed tool, which is the answer that needs no undo.
 *
 * Four points rather than one: a press on a trackpad moves a pixel or two on
 * the way up, and the point of the bound is to separate "clicked" from "drew".
 */
export const MIN_BOX = 4;

/**
 * The rectangle between two points a reader dragged, clamped into the page.
 *
 * **Both corners are the reader's**, so either may be the smaller one: dragging
 * up and to the left is as ordinary as dragging down and to the right, and the
 * `left > right` rectangle that falls out of subtracting them in the order they
 * arrived is one nothing draws. `Math.min`/`Math.max` rather than a swap, so
 * there is no branch that can be wrong for one of the four directions.
 *
 * Clamped for {@link iconQuad}'s reason, and it bites harder here: a drag that
 * leaves the page --- which is one flick of the wrist at the edge --- would
 * otherwise write a `/Rect` extending past the page box.
 *
 * `null` when the result is smaller than {@link MIN_BOX} in either direction.
 * That is a refusal rather than a tiny box, and the caller is expected to keep
 * the tool armed rather than report an error: a reader who clicked instead of
 * dragging has not made a mistake worth a message.
 */
export function boxQuad(
  from: { x: number; y: number },
  to: { x: number; y: number },
  page: { width: number; height: number },
): Quad | null {
  const clampX = (v: number): number => Math.max(0, Math.min(v, page.width));
  const clampY = (v: number): number => Math.max(0, Math.min(v, page.height));
  const quad = {
    left: clampX(Math.min(from.x, to.x)),
    top: clampY(Math.min(from.y, to.y)),
    right: clampX(Math.max(from.x, to.x)),
    bottom: clampY(Math.max(from.y, to.y)),
  };
  if (quad.right - quad.left < MIN_BOX) return null;
  if (quad.bottom - quad.top < MIN_BOX) return null;
  return quad;
}

/**
 * How big a comment's icon is, in points, before any zoom.
 *
 * The size a reader's `/Rect` gets, so the box drawn here is the box saved.
 * Twenty points is what Acrobat and Preview use for theirs, which matters
 * because they draw their *own* icon at their own size inside whatever
 * rectangle the file gives: a wildly different number here would put our bubble
 * and theirs in visibly different places.
 */
export const ICON_SIZE = 20;

/**
 * Where a comment's icon goes when a reader drops one at a point.
 *
 * The point is the icon's **top-left**, not its centre: dropping a pin puts the
 * thing where you pointed, and a centred icon means half of it lands above the
 * line a reader aimed at.
 *
 * **Clamped into the page, which is not tidiness.** A comment placed near the
 * right or bottom edge would otherwise get a `/Rect` running past the page box.
 * `save.rs` writes that rectangle without complaint --- it maps quads, it does
 * not police them --- and the result is an annotation other readers clip, draw
 * half of, or place somewhere of their own choosing. The clamp is also why this
 * takes the page's size at all.
 *
 * A page smaller than the icon is not special-cased. The clamp then pins the
 * icon to the top-left and it overhangs, which is the least surprising answer
 * for a page 10 points wide and not a case worth a branch.
 */
export function iconQuad(
  x: number,
  y: number,
  page: { width: number; height: number },
): Quad {
  const left = Math.max(0, Math.min(x, page.width - ICON_SIZE));
  const top = Math.max(0, Math.min(y, page.height - ICON_SIZE));
  return { left, top, right: left + ICON_SIZE, bottom: top + ICON_SIZE };
}

/**
 * The rectangle a mark's ink actually fills, inside its quad.
 *
 * `line_rect` in `save.rs`, in the overlay's coordinates --- which are
 * top-down, where the PDF's are bottom-up, so "sits on the bottom edge" is
 * arithmetic on `bottom` here and on `bottom` there and the two are not the same
 * direction. Written out per kind rather than as an offset the caller applies,
 * for the reason `save.rs` gives: the two kinds need different arithmetic, and
 * an offset that happens to be half the thickness for one of them is a
 * coincidence waiting to be tidied into a defect.
 */
export function markBand(kind: MarkKind, quad: Quad): Quad {
  const height = quad.bottom - quad.top;
  const thickness = height * LINE_FRACTION;
  switch (kind) {
    case "highlight":
      return quad;
    case "underline":
      // Sitting *on* the bottom edge, not centred on it. Centred would put half
      // the line outside the quad, which the saved file's `/BBox` clips away ---
      // so on screen it would be a line in the wrong place and in the file a
      // line of half the thickness, and neither would look like a bug.
      return { ...quad, top: quad.bottom - thickness };
    case "strikeout":
      return {
        ...quad,
        top: quad.top + height / 2 - thickness / 2,
        bottom: quad.top + height / 2 + thickness / 2,
      };
    case "squiggly":
      // A band at the bottom, like an underline's and taller. `save.rs`'s
      // `line_rect` answers the same question with the same arithmetic, because
      // where a kind's ink sits inside its quad is one question and two answers
      // to it drift.
      //
      // **What is returned is the band, not the wave.** The overlay asks
      // {@link isWave} and draws a zigzag inside this; the band is what the
      // wave is fitted to, and it is also what a reader's press has to land in.
      return { ...quad, top: quad.bottom - height * SQUIGGLE_HEIGHT };
    case "note":
      // The whole quad, and the quad is the icon's own box rather than a run of
      // text --- so this is the one kind where "the whole quad" is not a
      // statement about how much of the marked words are covered. What draws
      // inside it is a bubble rather than a rectangle; see `isIcon`.
      return quad;
    case "square":
      // The whole quad again, and for a third distinct reason: the quad *is*
      // the mark. There is no band inside it, because the ink is on its edge
      // rather than within it --- see `isOutline`, which is what tells the
      // overlay to stroke this rather than fill it. The stroke straddles the
      // edge, so half of it falls outside the quad; `save.rs` insets its own
      // path by half the width because the appearance stream's /BBox would clip
      // that half away, and the overlay has no /BBox and needs no inset.
      return quad;
    case "textbox":
      // The whole quad, and this is the fifth distinct reason: the quad is the
      // rectangle the reader dragged, the words are placed inside it from the
      // top down, and neither a band nor an edge describes where they land. The
      // overlay asks {@link isText} and draws the lines; what this returns is
      // the box they are laid inside, which is what the anchor and the hit test
      // want.
      return quad;
    case "ellipse":
      // The whole quad a fourth time, and the reason is the box's above with one
      // word changed: the quad *is* the mark, and the ink is on a curve through
      // it rather than on its edge. The overlay asks {@link isEllipse} and draws
      // that curve; what this returns is the rectangle the curve is inscribed
      // in, which is what the anchor and the hit test want.
      return quad;
    case "ink":
      // The whole quad a fifth time, and it is the only one of the five where
      // the answer is not merely *unused* but meaningless: a drawing has no
      // band, no edge and no relationship to its rectangle beyond happening to
      // fit inside it. The overlay asks {@link isPath} first and paints the
      // strokes, so nothing reaches this arm --- it exists because the switch
      // is exhaustive, which is what makes a sixth kind a compile error here.
      return quad;
     case "stamp":
      // The whole quad a sixth time. A stamp is a border on the quad's edge and
      // a word across its middle, which is two things rather than a band --- the
      // overlay asks {@link isStamp} and draws both. What this returns is the
      // rectangle they are placed from, which is what the anchor and the hit
      // test want.
      return quad;
  }
}

/**
 * How the overlay draws a kind, as one value rather than seven questions.
 *
 * The names are `save.rs`'s `Paint` variants, deliberately: the overlay and the
 * writer draw every mark, and a reader comparing the two should not have to
 * translate between two vocabularies for one decision. `docs/TRAPS.md` records
 * a differential between them finding a defect neither side's own tests could.
 *
 * **`Fill` has no `Paint` counterpart** and is not an omission. A wash is drawn
 * multiplied here and written as a `/Highlight` with a blend mode there, so the
 * file's version of it is a property of the annotation rather than of the shape;
 * what the overlay does is fill the quad. The blend is still {@link isWash}'s to
 * decide, because it is set per mark on the context rather than per shape.
 */
export type Paint =
  | "fill"
  | "line"
  | "wave"
  | "outline"
  | "ellipse"
  | "text"
  | "path"
  | "icon"
  | "stamp";

/**
 * Which of those a kind is drawn as.
 *
 * **Exhaustive, for {@link isMovable}'s reason and one more.** The overlay used
 * to ask the `isX` predicates in an if-chain ending in a bare `else` that filled
 * the quad, so a kind nobody added a branch for was drawn as a filled rectangle
 * --- a wrong picture rather than a missing one, on every page, with nothing
 * going red. `paintMarks`' own comments record a kind that shipped unpainted for
 * exactly that reason.
 *
 * Answering it once, here, is what turns that into a compile error: a tenth kind
 * has no arm, and `MarkKind` is a union rather than an enum so TypeScript refuses
 * the switch rather than falling through it. The individual predicates stay ---
 * they answer questions the painter is not asking, `isWash` for the blend mode
 * and `isMovable` for the drag --- and this is the only one the painter needs.
 *
 * The mapping is `paint()` in `save.rs` arm for arm, except that a highlight is
 * `fill` here where it is `Wash` there. See {@link Paint}.
 */
export function paintOf(kind: MarkKind): Paint {
  switch (kind) {
    case "highlight":
      return "fill";
    case "underline":
    case "strikeout":
      return "line";
    case "squiggly":
      return "wave";
    case "square":
      return "outline";
    case "ellipse":
      return "ellipse";
    case "textbox":
      return "text";
    case "ink":
      return "path";
    case "note":
      return "icon";
    case "stamp":
      return "stamp";
  }
}
