/**
 * The colours a mark can be, and which one a new mark takes.
 *
 * ## Why this is a module rather than a constant in `edits.ts`
 *
 * {@link MARK_COLORS} lived there, with a doc comment saying what it was not:
 * *"A palette is a different question --- where the swatches live, whether a
 * reader picks before or after marking --- and answering it with a constant here
 * would be answering it invisibly."* This file answers it, and the two halves of
 * the answer are why it is one file: the swatch row in the mark popup and the
 * `Colour:` commands in the palette both read {@link PALETTE}, and the defaults
 * below are what a reader gets back by choosing {@link DEFAULT_SWATCH}.
 *
 * ## Both directions, one notion
 *
 * A reader picks a colour **before** marking --- the next highlight is green ---
 * and **after** --- this highlight is green now. Those are one thing in the
 * product and one command here: picking sets the choice, and applies it to the
 * mark whose note is open if there is one. Preview and Word both behave this
 * way, and the alternative --- two families of commands, "mark in green" beside
 * "recolour this green" --- doubles the surface to state a distinction a reader
 * does not have.
 *
 * ## The choice can be *none*, and that is not the same as yellow
 *
 * With nothing chosen, each kind keeps the colour {@link MARK_COLORS} gives it:
 * a wash is yellow and a line is red, for the reasons stated there. Once a
 * reader picks green, every kind is green. {@link DEFAULT_SWATCH} is how they
 * get back, and it earns its place: without it a reader who tried green could
 * never again have a yellow highlight *and* a red underline without picking
 * twice, which is a choice they never made.
 *
 * ## Nothing here crosses to Rust
 *
 * The model stores whatever three floats it is handed and `save.rs` writes them
 * into `/C`; neither has a table of colours. So this is the one statement of
 * what the swatches are, rather than the near-copy across a language boundary
 * that `markband.ts` has to be and says so.
 */

import type { MarkKind } from "./pages";

/** Red, green and blue in 0..=1, as `/C` takes them and `Mark::color` holds them. */
export type MarkColor = readonly [number, number, number];

/** One entry in the swatch row. */
export interface Swatch {
  /** The part of the command id after `edit.color.`, and the session's word for it. */
  readonly id: string;
  /** What a reader sees, lowercase --- it appears mid-sentence in a command title. */
  readonly name: string;
  /**
   * What a mark drawn in it is, or `null` for {@link DEFAULT_SWATCH}, which
   * means "whatever the kind's own colour is" rather than any one value.
   */
  readonly rgb: MarkColor | null;
}

/**
 * The colour each kind is written in when the reader has chosen nothing.
 *
 * The two lines are red rather than the wash's yellow, and not by convention
 * alone: a yellow rule 1.3 pt thick on white paper is close to invisible, where
 * the same yellow spread over a whole line of text is exactly right. The wash is
 * drawn multiplied at 40% and the lines opaque, so what reads well as one cannot
 * be assumed to read well as the other.
 */
export const MARK_COLORS: Record<MarkKind, MarkColor> = {
  highlight: [1, 0.9, 0.2],
  underline: [0.85, 0.15, 0.15],
  strikeout: [0.85, 0.15, 0.15],
  // The rules' red, because it is one of them: a line under words, drawn
  // opaque. Sharing the underline's colour is also what makes the two read as a
  // choice of shape rather than as two unrelated marks.
  squiggly: [0.85, 0.15, 0.15],
  // The wash's yellow rather than the lines' red, and for the opposite reason
  // to both: this colour is not ink over words at all, it is the fill of an
  // icon sitting on the paper beside them. Yellow is what every reader draws a
  // comment bubble in, so a file opened in Acrobat looks like the file that was
  // saved --- `/C` is what Acrobat colours its own icon with.
  note: [1, 0.9, 0.2],
  // The lines' red, not the bubble's yellow, because a box is a line: its ink
  // is a stroke and it is drawn opaque and on top. A yellow box on white paper
  // is nearly invisible, which is the same reason the underline and the
  // strikeout above are not the wash's colour either.
  square: [0.85, 0.15, 0.15],
  // The lines' red again, for the box's reason: ink is a stroke, drawn opaque
  // and on top, and yellow ink on white paper is nearly invisible. It is also
  // what a reader reaches for a pen to do --- annotate in a colour that is not
  // the document's --- which is the same argument, from the other end.
  ink: [0.85, 0.15, 0.15],
  // The box's red, because it is the box's argument exactly: an ellipse is a
  // line too, drawn opaque and on top, and yellow on white paper is nearly
  // invisible. Sharing a default is also what makes the two read as one family
  // when a reader drags a box round one figure and a ring round another.
  ellipse: [0.85, 0.15, 0.15],
  // The lines' red again, and here it colours *words* rather than a stroke. Red
  // type on white paper reads as an annotation rather than as part of the
  // document, which is the whole reason a reader puts a text box on a page.
  textbox: [0.85, 0.15, 0.15],
  // The lines' red a fourth time, and it is the box's argument with one word
  // added: a stamp is a stroked border *and* red type, so both halves of it are
  // the reasons the box and the text box are red. A stamp is also the one kind
  // whose whole purpose is to be seen from across the page, which rules out the
  // wash's yellow more firmly than for any other kind here.
  stamp: [0.85, 0.15, 0.15],
};

/**
 * Going back to a colour per kind, rather than to any one colour.
 *
 * First in {@link PALETTE} so that it is the entry a reader lands on, and
 * carrying `null` rather than a value --- see {@link Swatch.rgb}. A swatch
 * holding yellow here would quietly turn every underline yellow the first time
 * somebody used it to undo a choice.
 */
export const DEFAULT_SWATCH: Swatch = { id: "default", name: "default", rgb: null };

/**
 * The swatches, in the order the row draws them.
 *
 * **Yellow and red are the values {@link MARK_COLORS} already uses**, byte for
 * byte rather than close to them. A palette whose yellow is a shade off the
 * default yellow means a reader who picks the swatch matching their existing
 * highlights gets marks that do not match them, and nothing on screen says why.
 *
 * Six, and the set is a highlighter pack rather than a colour wheel: these are
 * what a reader reaches for, they are told apart at a glance on white paper, and
 * they survive both renderings --- multiplied at 40% as a wash, opaque as a
 * 1.3 pt rule. A palette large enough to need a grid is a colour picker, which
 * is a different piece of work and one nobody has asked for.
 */
export const PALETTE: readonly Swatch[] = [
  DEFAULT_SWATCH,
  { id: "yellow", name: "yellow", rgb: MARK_COLORS.highlight },
  { id: "green", name: "green", rgb: [0.35, 0.8, 0.35] },
  { id: "blue", name: "blue", rgb: [0.3, 0.6, 0.95] },
  { id: "pink", name: "pink", rgb: [0.95, 0.45, 0.75] },
  { id: "orange", name: "orange", rgb: [1, 0.6, 0.15] },
  { id: "red", name: "red", rgb: MARK_COLORS.underline },
];

/** The swatch with this id, or `undefined`. */
export function swatch(id: string): Swatch | undefined {
  return PALETTE.find((entry) => entry.id === id);
}

/**
 * What a new mark of `kind` is drawn in, given what the reader has chosen.
 *
 * The whole of the rule, in one expression and one place: a choice applies to
 * every kind, and no choice leaves each kind its own.
 */
export function colorFor(kind: MarkKind, chosen: MarkColor | null): MarkColor {
  return chosen ?? MARK_COLORS[kind];
}

/**
 * A colour as CSS, opaque.
 *
 * For the swatch buttons, which are the colour rather than being painted in it
 * --- the overlay has its own, `markInk` in `viewer.ts`, because what it draws
 * over a tile carries an alpha that depends on whether the mark is a wash or a
 * line. Two callers, two questions, and folding them together would put a
 * painting decision in a button.
 */
export function cssColor(rgb: MarkColor): string {
  const [r, g, b] = rgb.map((v) => Math.round(v * 255));
  return `rgb(${r}, ${g}, ${b})`;
}

/** Whether two colours are the same, for showing which swatch is on. */
export function sameColor(a: MarkColor | null, b: MarkColor | null): boolean {
  if (a === null || b === null) return a === b;
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}
