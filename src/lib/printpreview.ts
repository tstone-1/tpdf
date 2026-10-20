/**
 * What the print preview shows, worked out apart from the dialog that shows it.
 *
 * The preview is of the print job itself: the bytes the platform's dialog is
 * handed next, with the reader's marks, crops, deleted pages and turn already
 * in them, drawn by a worker over those bytes (`commands::print::Previews`).
 * So a sheet here is a page of the job, not a page of the open document.
 *
 * macOS has no preview of tpdf's: its print panel draws the job itself.
 */

import type { PageSize, PrintPlacement, PrintPaper, PrintScale, PrintSettings, TwoSided } from "./ipc";
import { PAGE_SIZES } from "./pagesizes";

/** The pages-to-a-sheet a reader can choose. `print_layout::PER_SHEET`. */
export const PER_SHEET = [1, 2, 4, 6, 9, 16] as const;

/** The largest percentage a reader can type. `print_layout::MOST_PERCENT`. */
export const MOST_PERCENT = 400;

/** The most copies a reader can ask for. */
export const MOST_COPIES = 999;

/**
 * One sheet of paper as the preview draws it: the paper, and the pages placed
 * on its printable area.
 */
export interface Sheet {
  paper: PrintPaper;
  placements: PrintPlacement[];
}

/**
 * The sheets where the system's dialog decides the paper (Linux): each page
 * on a sheet of its own size, filling it.
 */
export function pageSheets(pages: readonly PageSize[]): Sheet[] {
  return pages.map((page, index) => ({
    paper: {
      width: page.width_pt,
      height: page.height_pt,
      left: 0,
      top: 0,
      printableWidth: page.width_pt,
      printableHeight: page.height_pt,
    },
    placements: [{ page: index, x: 0, y: 0, width: page.width_pt, height: page.height_pt, turned: false }],
  }));
}

/**
 * The pages a reader typed, as zero-based page numbers in the order typed:
 * "1-3, 5" is `[0, 1, 2, 4]`. Empty means every page, `null`. A string is
 * the reason the text is refused, in the reader's words.
 */
export function parsePages(text: string, count: number): number[] | null | string {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  const pages: number[] = [];
  for (const part of trimmed.split(",")) {
    const piece = part.trim();
    if (piece === "") continue;
    const match = /^(\d+)\s*(?:-\s*(\d+))?$/.exec(piece);
    if (!match) return `"${piece}" is not a page or a range of pages.`;
    const first = Number(match[1]);
    const last = match[2] === undefined ? first : Number(match[2]);
    for (const page of [first, last]) {
      if (page < 1 || page > count) {
        return count === 1 ? `This job has 1 page, not ${page}.` : `This job has pages 1 to ${count}, not ${page}.`;
      }
    }
    if (last < first) return `"${piece}" runs backwards.`;
    for (let page = first; page <= last; page += 1) pages.push(page - 1);
  }
  return pages.length === 0 ? null : pages;
}

/**
 * Whether a sheet should be turned sideways for `perSheet` pages of the
 * first page's shape: two or six portrait pages sit side by side on a
 * landscape sheet, and otherwise the sheet follows the page.
 */
export function suggestLandscape(first: PageSize | undefined, perSheet: number): boolean {
  const wide = first !== undefined && first.width_pt > first.height_pt;
  return perSheet === 2 || perSheet === 6 ? !wide : wide;
}

/**
 * Which pages print: the range, then which of those, then in what order.
 * `text` is read only for a `custom` range, and `current` (zero based) only
 * for `current`.
 */
export interface PageChoice {
  range: "all" | "current" | "custom";
  text: string;
  current: number;
  subset: "all" | "odd" | "even";
  reverse: boolean;
}

/**
 * The pages a choice names, zero based and in print order; `null` for every
 * page in order, which is what the backend takes as "all". A string is the
 * reason it is refused, in the reader's words.
 *
 * Odd and even go by page number, as a reader counts them: page 1 is odd.
 * Printing the odd pages, turning the stack over and printing the even
 * pages in reverse is how a printer that prints one side does both.
 */
export function choosePages(choice: PageChoice, count: number): number[] | null | string {
  let pages: number[];
  if (choice.range === "custom") {
    const typed = parsePages(choice.text, count);
    if (typeof typed === "string") return typed;
    pages = typed ?? [...Array(count).keys()];
  } else if (choice.range === "current") {
    if (!(choice.current >= 0 && choice.current < count)) return "There is no current page in this job.";
    pages = [choice.current];
  } else {
    pages = [...Array(count).keys()];
  }
  if (choice.subset !== "all") {
    const odd = choice.subset === "odd";
    pages = pages.filter((page) => (page % 2 === 0) === odd);
    if (pages.length === 0) {
      return odd ? "None of these pages has an odd number." : "None of these pages has an even number.";
    }
  }
  if (choice.reverse) pages = [...pages].reverse();
  const everything = pages.length === count && pages.every((page, index) => page === index);
  return everything ? null : pages;
}

/** The settings a preview opens with: the default printer, one page to a sheet, nothing scaled up. */
export function defaultSettings(pages: readonly PageSize[]): PrintSettings {
  return {
    printer: null,
    paper: null,
    landscape: suggestLandscape(pages[0], 1),
    copies: 1,
    twoSided: null,
    colour: null,
    perSheet: 1,
    scale: { kind: "shrink" },
    rotate: true,
    marks: true,
    pages: null,
  };
}

/**
 * The settings a print keeps for the next one: the printer and how it prints.
 * Not the pages, the copies or the orientation, which belong to a document;
 * the orientation follows each document's first page, and with pages turned
 * to fit that is what a reader wants.
 */
export interface Kept {
  printer: string;
  /** The paper by the driver's number, which means nothing on another printer. */
  paper: number | null;
  twoSided: TwoSided | null;
  colour: boolean | null;
  perSheet: number;
  scale: PrintScale;
  rotate: boolean;
  marks: boolean;
}

/** Where the settings are kept. */
const KEPT_KEY = "tpdf.print-settings";

type Store = Pick<Storage, "getItem" | "setItem">;

/** What `value` is, when it is settings tpdf wrote; `null` for anything else. */
export function asKept(value: unknown): Kept | null {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  const scale = v["scale"] as Record<string, unknown> | undefined;
  const scaleOk =
    typeof scale === "object" &&
    scale !== null &&
    (["shrink", "fit", "actual"].includes(scale["kind"] as string) ||
      ((scale["kind"] === "percent" || scale["kind"] === "poster") &&
        Number.isInteger(scale["percent"]) &&
        (scale["percent"] as number) >= 1 &&
        (scale["percent"] as number) <= MOST_PERCENT));
  const ok =
    typeof v["printer"] === "string" &&
    (v["paper"] === null || Number.isInteger(v["paper"])) &&
    (v["twoSided"] === null || ["off", "long", "short"].includes(v["twoSided"] as string)) &&
    (v["colour"] === null || typeof v["colour"] === "boolean") &&
    (PER_SHEET as readonly number[]).includes(v["perSheet"] as number) &&
    scaleOk &&
    typeof v["rotate"] === "boolean" &&
    typeof v["marks"] === "boolean";
  if (!ok) return null;
  return {
    printer: v["printer"] as string,
    paper: v["paper"] as number | null,
    twoSided: v["twoSided"] as TwoSided | null,
    colour: v["colour"] as boolean | null,
    perSheet: v["perSheet"] as number,
    scale: v["scale"] as PrintScale,
    rotate: v["rotate"] as boolean,
    marks: v["marks"] as boolean,
  };
}

/** The settings the last print kept, or `null` when there are none or storage refuses. */
export function readKept(storage: () => Store = () => window.localStorage): Kept | null {
  try {
    const text = storage().getItem(KEPT_KEY);
    return text === null ? null : asKept(JSON.parse(text));
  } catch {
    return null;
  }
}

/** Keeps what `settings` say about the printer; `false` when storage refused. */
export function writeKept(settings: PrintSettings, storage: () => Store = () => window.localStorage): boolean {
  if (settings.printer === null) return false;
  const kept: Kept = {
    printer: settings.printer,
    paper: settings.paper,
    twoSided: settings.twoSided,
    colour: settings.colour,
    perSheet: settings.perSheet,
    scale: settings.scale,
    rotate: settings.rotate,
    marks: settings.marks,
  };
  try {
    storage().setItem(KEPT_KEY, JSON.stringify(kept));
    return true;
  } catch {
    return false;
  }
}

/**
 * A preview's opening settings with the last print's laid over them. The
 * orientation is suggested again from the kept pages to a sheet.
 */
export function withKept(settings: PrintSettings, kept: Kept | null, first: PageSize | undefined): PrintSettings {
  if (!kept) return settings;
  return {
    ...settings,
    ...kept,
    landscape: suggestLandscape(first, kept.perSheet),
  };
}

/** A box's position inside another, as CSS percentages. */
export interface Percents {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where the printable area sits on the paper, in percent of the paper. */
export function printableBox(paper: PrintPaper): Percents {
  return {
    left: (paper.left / paper.width) * 100,
    top: (paper.top / paper.height) * 100,
    width: (paper.printableWidth / paper.width) * 100,
    height: (paper.printableHeight / paper.height) * 100,
  };
}

/**
 * Where a turned page's picture sits in its placement, before it is turned:
 * the picture is the page upright, so its width is the placement's height,
 * and it is centred so that a quarter turn about its middle fills the
 * placement exactly. `null` for a page that is not turned.
 */
export function turnedPicture(placed: PrintPlacement): Percents | null {
  if (!placed.turned || !(placed.width > 0 && placed.height > 0)) return null;
  const width = (placed.height / placed.width) * 100;
  const height = (placed.width / placed.height) * 100;
  return { left: (100 - width) / 2, top: (100 - height) / 2, width, height };
}

/**
 * Where a placed page sits in the printable area, in percent of that area.
 * It may reach past 0 or 100: the area clips it, as the printer does.
 */
export function placementBox(placed: PrintPlacement, paper: PrintPaper): Percents {
  return {
    left: (placed.x / paper.printableWidth) * 100,
    top: (placed.y / paper.printableHeight) * 100,
    width: (placed.width / paper.printableWidth) * 100,
    height: (placed.height / paper.printableHeight) * 100,
  };
}

/** The line under the heading once the job is laid out on paper: sheets, and pages if they differ. */
export function sheetSummary(sheets: number, pages: number, copies: number): string {
  const paper = sheets === 1 ? "1 sheet of paper" : `${sheets} sheets of paper`;
  const of = pages === sheets ? "" : ` for ${pages === 1 ? "1 page" : `${pages} pages`}`;
  const times = copies > 1 ? `, ${copies} copies` : "";
  return `${paper}${of}${times}`;
}

/**
 * The widest picture the backend draws. `PREVIEW_MAX_WIDTH` in
 * `commands/print.rs`; `printpreview.test.ts` holds the two together.
 */
export const PREVIEW_MAX_WIDTH = 1600;

/** Space between two sheets, in CSS pixels. */
export const SHEET_GAP = 16;

/** How close a page must be to a named size to be called by its name, in points. */
const NAMED_WITHIN = 3;

/**
 * A sheet's paper, as a reader names it: "A4", "A3 landscape", "US Letter",
 * or its size in millimetres when it is none of those.
 */
export function paperName(size: PageSize): string {
  const { width_pt: width, height_pt: height } = size;
  for (const named of Object.values(PAGE_SIZES)) {
    if (Math.abs(width - named.width) <= NAMED_WITHIN && Math.abs(height - named.height) <= NAMED_WITHIN) {
      return named.title;
    }
    if (Math.abs(width - named.height) <= NAMED_WITHIN && Math.abs(height - named.width) <= NAMED_WITHIN) {
      return `${named.title} landscape`;
    }
  }
  const mm = (points: number) => Math.round((points / 72) * 25.4);
  return `${mm(width)} x ${mm(height)} mm`;
}

/**
 * The line under the preview's heading: how many sheets, and on what paper.
 * Up to three different papers are named; more than that is "mixed sizes".
 */
export function previewSummary(pages: readonly PageSize[]): string {
  const count = pages.length === 1 ? "1 page" : `${pages.length} pages`;
  const names = [...new Set(pages.map(paperName))];
  if (names.length === 0) return count;
  if (names.length === 1) return pages.length === 1 ? `${count}, ${names[0]}` : `${count}, all ${names[0]}`;
  if (names.length <= 3) return `${count}: ${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return `${count} in mixed sizes`;
}

/**
 * A sheet's size on screen, in CSS pixels: as large as fits `maxWidth` by
 * `maxHeight` with its own proportions. A landscape page fills the width, a
 * portrait one the height, so every sheet is as readable as the box allows.
 */
export function fitSheet(size: PageSize, maxWidth: number, maxHeight: number): { width: number; height: number } {
  const { width_pt: width, height_pt: height } = size;
  if (!(width > 0 && height > 0 && maxWidth > 0 && maxHeight > 0)) return { width: 0, height: 0 };
  const scale = Math.min(maxWidth / width, maxHeight / height);
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}

/**
 * How many pixels wide to ask the backend for a sheet shown `cssWidth`
 * pixels wide: sharp on the screen's own pixels, never past
 * {@link PREVIEW_MAX_WIDTH}, and at least one.
 */
export function pictureWidth(cssWidth: number, devicePixelRatio: number): number {
  const ratio = devicePixelRatio > 0 ? devicePixelRatio : 1;
  return Math.max(1, Math.min(PREVIEW_MAX_WIDTH, Math.round(cssWidth * ratio)));
}

/**
 * The sheets at least partly inside the scrolled view, as `[first, last]`, or
 * `null` when there are none. `heights` are the sheets' heights in order, each
 * followed by {@link SHEET_GAP}.
 */
export function visibleSheets(
  heights: readonly number[],
  scrollTop: number,
  viewHeight: number,
): [number, number] | null {
  let top = 0;
  let first = -1;
  let last = -1;
  const bottom = scrollTop + viewHeight;
  for (let index = 0; index < heights.length; index += 1) {
    const end = top + (heights[index] ?? 0);
    if (end > scrollTop && top < bottom) {
      if (first < 0) first = index;
      last = index;
    } else if (top >= bottom) {
      break;
    }
    top = end + SHEET_GAP;
  }
  return first < 0 ? null : [first, last];
}

/**
 * The next page to draw: on the visible sheets first, then the sheet after
 * the view, so scrolling down finds it drawn. A page is drawn again only when
 * the picture it has is less than nine tenths of the width it is now shown
 * at, which a larger layout asks for. `width` is the pixel width a placed page
 * wants on its sheet, and `drawn` the width each page has been drawn at.
 * `null` when everything near the view is sharp.
 */
export function nextPicture(
  sheets: readonly Sheet[],
  visible: [number, number] | null,
  width: (sheet: number, placed: PrintPlacement) => number,
  drawn: ReadonlyMap<number, number>,
): { sheet: number; page: number; width: number } | null {
  if (!visible) return null;
  const [first, last] = visible;
  for (let sheet = first; sheet <= Math.min(last + 1, sheets.length - 1); sheet += 1) {
    for (const placed of sheets[sheet]?.placements ?? []) {
      const wanted = width(sheet, placed);
      const have = drawn.get(placed.page);
      if (have === undefined || have < wanted * 0.9) return { sheet, page: placed.page, width: wanted };
    }
  }
  return null;
}
