/**
 * How thick a drawing's ink is, and which nib a new one takes.
 *
 * ## Why this is a module rather than a constant in `markband.ts`
 *
 * {@link INK_WIDTH} still lives there, and it still is what a drawing is made
 * with when the reader has chosen nothing --- this file imports it rather than
 * restating the number, exactly as `markcolors.ts` reuses `MARK_COLORS`'s yellow
 * byte for byte. What is here is the *choice*: the widths a reader can pick, the
 * words for them, and the one function that turns a choice into the number that
 * crosses to the backend. `markband.ts` says what ink is; this says what the pen
 * can be set to.
 *
 * ## One direction, unlike the palette
 *
 * A colour can be picked before marking *and* after --- `markcolors.ts` argues
 * that at length, and `Command::Recolor` is what makes the second half work. A
 * nib is picked **before** only, and that is a decision with a mechanism behind
 * it rather than a gap: a drawing's rectangle is `Stroke::bounds` of its strokes
 * padded by half its width, so changing the width of an existing mark has to
 * rebuild the rectangle on every replay of the journal. That is a command with a
 * body table and a derivation in it. `Mark::width` says the same thing from the
 * other side, and says what would have to happen for the second half to exist.
 *
 * ## Points, and there is nothing to decide
 *
 * Every number here is in the page's own points. The appearance stream's `w` is
 * in the form's space, which is the page's with no matrix, so this number *is*
 * what a foreign reader draws --- a width taken from the view would make a line
 * drawn at 400% four times thinner in the file than the same line drawn at 100%.
 *
 * ⚠ **`docs/PLAN.md` ranked this as "the same open question the ink eraser
 * left", and the two are not one question.** The eraser's nib is
 * `ERASER_RADIUS` in `markband.ts`, a hit radius in *view pixels*, and whether it should be
 * clamped to `HIT_SLACK_PT` in points is genuinely open. This nib is a width in
 * a file. They share a word and not a quantity, and the note joined them by the
 * word.
 */

import { INK_WIDTH } from "./markband";

/** One entry in the nib list. */
export interface Nib {
  /** The part of the command id after `edit.nib.`, and the session's word for it. */
  readonly id: string;
  /** What a reader sees, lowercase --- it appears mid-sentence in a command title. */
  readonly name: string;
  /** How thick a line drawn with it is, in points. */
  readonly pt: number;
}

/**
 * The nibs, from thinnest to thickest.
 *
 * **The default is in the list rather than beside it**, which is where this
 * parts company with `markcolors.ts`. A swatch can mean "whatever the kind's own
 * colour is", which is not any one value and is why `DEFAULT_SWATCH` carries
 * `null`; a nib is one number for every kind, so "the usual one" is an entry
 * with {@link INK_WIDTH} in it and needs no second notion. A reader going back
 * to it picks it the same way they picked anything else.
 *
 * Four, and they are a pen case rather than a slider: at a glance on the page
 * they are told apart, each is roughly double the one before, and they cover
 * ruling a margin at one end and striking a paragraph out at the other. A list
 * fine enough to need a number box is a width picker, which nobody has asked
 * for and which the range in `docmodel.rs` would still have to bound.
 */
export const NIBS: readonly Nib[] = [
  { id: "fine", name: "fine", pt: 1 },
  // The default, by reference rather than by value: a list whose "medium" is a
  // shade off `INK_WIDTH` means a reader picking the entry that matches their
  // existing drawings gets ink that does not match them, and nothing on screen
  // says why. `markcolors.ts` makes the same argument about yellow.
  { id: "medium", name: "medium", pt: INK_WIDTH },
  { id: "broad", name: "broad", pt: 5 },
  { id: "marker", name: "marker", pt: 10 },
];

/**
 * The nib a reader gets before they have chosen one.
 *
 * The entry holding {@link INK_WIDTH}, found rather than named twice --- the
 * whole point of the list carrying it by reference is that there is one place
 * the default is written, and a second `"medium"` here would be a second place
 * for it to drift from.
 */
export const DEFAULT_NIB: Nib = NIBS.find((entry) => entry.pt === INK_WIDTH) ?? NIBS[0]!;

/** The nib with this id, or `undefined`. */
export function nib(id: string): Nib | undefined {
  return NIBS.find((entry) => entry.id === id);
}

/**
 * How thick a new drawing is, given what the reader has chosen.
 *
 * The whole of the rule, in one expression and one place, which is
 * `colorFor`'s shape --- and the argument for the seam is the stronger one
 * here, because the caller is `App.svelte`, which no unit test reaches.
 */
export function widthFor(chosen: Nib | null): number {
  return chosen?.pt ?? INK_WIDTH;
}
