/**
 * The page layouts a reader picks, by name, and how one is remembered.
 *
 * `scroller.ts` owns what a {@link Spread} does to the layout. This owns the
 * three choices the palette and the View menu offer --- one page to a row,
 * facing pages, and book view with the cover alone --- and the conversion to
 * and from the session's `Place`, so neither lives in `App.svelte`.
 */

import { SINGLE, type Spread } from "./scroller";

/** The layouts offered, in the order the menu lists them. */
export const SPREAD_KINDS = ["single", "facing", "book"] as const;
export type SpreadKind = (typeof SPREAD_KINDS)[number];

/** What the palette and the menu call each layout. */
export const SPREAD_TITLES: Record<SpreadKind, string> = {
  single: "Pages: one to a row",
  facing: "Pages: facing",
  book: "Pages: book, with the cover alone",
};

/** Which of the offered layouts `spread` is. */
export function kindOf(spread: Spread): SpreadKind {
  if (spread.columns === 1) return "single";
  return spread.coverAlone ? "book" : "facing";
}

/**
 * `spread` changed to `kind`, keeping the reading direction.
 *
 * Right to left is kept through a change between facing and book, and through
 * one page to a row, where it does nothing, so that going back to two columns
 * finds it where the reader left it.
 */
export function withKind(spread: Spread, kind: SpreadKind): Spread {
  return {
    columns: kind === "single" ? 1 : 2,
    coverAlone: kind === "book",
    rightToLeft: spread.rightToLeft,
    continuous: spread.continuous,
  };
}

/** The fields of a session `Place` that hold a layout. */
export interface PlaceSpread {
  columns?: number;
  cover_alone?: boolean;
  right_to_left?: boolean;
  continuous?: boolean;
}

/**
 * The layout a remembered place names. A place written before facing pages
 * existed names none, and is one page to a row; so is a column count other
 * than two, as the backend reads it.
 */
export function spreadOfPlace(place: PlaceSpread): Spread {
  // Continuous unless the place says otherwise: a place written before the
  // choice existed was read continuously.
  const continuous = place.continuous !== false;
  const rightToLeft = place.right_to_left === true;
  if (place.columns !== 2) return { ...SINGLE, rightToLeft, continuous };
  return { columns: 2, coverAlone: place.cover_alone === true, rightToLeft, continuous };
}

/** The fields to write for `spread`. */
export function placeOfSpread(spread: Spread): Required<PlaceSpread> {
  return {
    columns: spread.columns,
    cover_alone: spread.coverAlone,
    right_to_left: spread.rightToLeft,
    continuous: spread.continuous,
  };
}
