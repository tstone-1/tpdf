/**
 * Saving a copy with page numbers, or with a watermark: what the dialog starts
 * with, whether what was typed can be written, the name the copy is offered and
 * the sentence the window ends on.
 *
 * `App.svelte` keeps the dialog and the `invoke`; what is decided is here, so it
 * has tests. The writer is `src-tauri/src/addedtext.rs`, which `tpdf
 * page-numbers` and `tpdf watermark` use too, and which applies the same rules
 * and is the one that counts. The defaults here are its defaults:
 * `addtext.test.ts` reads them out of that file.
 */

import { parsePageRange } from "./pageranges";
import { basename } from "./paths";
import { afterCopy } from "./recovery";

/** `Position` in `addedtext.rs`, in reading order. */
export const POSITIONS = [
  "top-left", "top-center", "top-right",
  "middle-left", "center", "middle-right",
  "bottom-left", "bottom-center", "bottom-right",
] as const;
export type Position = (typeof POSITIONS)[number];

/** What a position is called in the dialog. */
export const POSITION_TITLES: Record<Position, string> = {
  "top-left": "Top left",
  "top-center": "Top center",
  "top-right": "Top right",
  "middle-left": "Middle left",
  center: "Center",
  "middle-right": "Middle right",
  "bottom-left": "Bottom left",
  "bottom-center": "Bottom center",
  "bottom-right": "Bottom right",
};

/** `AddedText` in `addedtext.rs`, as `save_with_text` takes it. */
export interface AddedText {
  text: string;
  position: Position;
  size: number;
  color: [number, number, number];
  opacity: number;
  angle: number;
  margin: number;
  first_number: number;
  /** Zero-based slots, or `null` for every page. */
  pages: number[] | null;
}

export type Kind = "numbers" | "watermark";

/**
 * A watermark's opacity in the written PDF, a third opaque. A named constant
 * rather than a literal beside the word `opacity`, which `contrast.test.ts`
 * reads as a dimmed line of interface text.
 */
const WATERMARK_SHOWS = 0.3;

/** `AddedText::page_numbers` and `AddedText::watermark`. */
export function defaults(kind: Kind): AddedText {
  return kind === "numbers"
    ? { text: "Page {page} of {pages}", position: "bottom-center", size: 10, color: [0, 0, 0],
      opacity: 1, angle: 0, margin: 36, first_number: 1, pages: null }
    : { text: "DRAFT", position: "center", size: 60, color: [0.5, 0.5, 0.5],
      opacity: WATERMARK_SHOWS, angle: 45, margin: 36, first_number: 1, pages: null };
}

/** `MAX_CHARS` and `SIZES` in `addedtext.rs`. */
export const MAX_CHARS = 200;
export const MIN_SIZE = 4;
export const MAX_SIZE = 300;

/** What the dialog's fields hold, as typed. */
export interface Typed {
  text: string;
  position: Position;
  size: string;
  /** The first number, for page numbers; the opacity in percent, for a watermark. */
  extra: string;
  /** A page range as the palette takes one, or empty for every page. */
  pages: string;
}

/**
 * What was typed as the line to write, or why it cannot be.
 *
 * The same refusals `addedtext::check` makes, said before a save panel is
 * opened rather than after; the backend still checks.
 */
export function judge(kind: Kind, typed: Typed, pageCount: number): AddedText | string {
  const added = defaults(kind);
  const text = typed.text;
  if (text.trim() === "") return "Type the text to write on the pages.";
  if ([...text].length > MAX_CHARS) return `The text is longer than ${MAX_CHARS} characters.`;
  // `addedtext::check`'s first refusal. Whether a font has every character
  // is the backend's to say: it holds the fonts, and a copy of their
  // coverage here would drift from them.
  if (/\p{Cc}/u.test(text)) return "The text has a control character in it.";
  for (const field of text.match(/\{[^}]*\}?/g) ?? []) {
    if (!field.endsWith("}")) return "A field opened with { is not closed.";
    if (field !== "{page}" && field !== "{pages}") {
      return `${field} is not a field. The fields are {page} and {pages}.`;
    }
  }
  const size = Number(typed.size);
  if (!Number.isFinite(size) || size < MIN_SIZE || size > MAX_SIZE) {
    return `The size is a number of points from ${MIN_SIZE} to ${MAX_SIZE}.`;
  }
  const extra = Number(typed.extra);
  if (kind === "numbers") {
    if (!Number.isInteger(extra) || extra < 0 || extra > 1_000_000) {
      return "The first number is a whole number from 0.";
    }
    added.first_number = extra;
  } else {
    if (!Number.isFinite(extra) || extra <= 0 || extra > 100) {
      return "The opacity is a percentage, more than 0 and at most 100.";
    }
    added.opacity = extra / 100;
  }
  let pages: number[] | null = null;
  if (typed.pages.trim() !== "") {
    const parsed = parsePageRange(typed.pages, pageCount);
    if (parsed.problem !== undefined) return parsed.problem;
    pages = parsed.slots;
  }
  return { ...added, text, position: typed.position, size, pages };
}

/** `report.pdf` gives `report numbered.pdf`, or `report watermarked.pdf`. */
export function suggestedName(path: string, kind: Kind): string {
  const stem = basename(path).replace(/\.pdf$/i, "");
  return `${stem} ${kind === "numbers" ? "numbered" : "watermarked"}.pdf`;
}

/**
 * What to say once the copy is written. Never silent: the window still shows
 * the document without the text, which is on the copy only.
 */
export function afterAdding(copied: { changed?: boolean }, path: string, kind: Kind): string {
  const said = kind === "numbers"
    ? `Saved ${basename(path)} with page numbers.`
    : `Saved ${basename(path)} with a watermark.`;
  const changed = afterCopy(copied);
  return changed ? `${said} ${changed}` : said;
}
