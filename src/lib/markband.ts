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
 * `ink`, `LINE_FRACTION`, `line_rect` and `OUTLINE_WIDTH` in
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
  }
}
