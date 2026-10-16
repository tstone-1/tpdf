/**
 * Where a document started from a command line is shown: `--page` and
 * `--zoom`, read out of the command line by `launch.rs` and checked here.
 *
 * Checked here and not in Rust, because a page number means nothing until the
 * document says how many pages it has, and the zoom's rules are the palette's
 * (`parseZoomPercent`): one rule, whichever way the zoom is asked for. A value
 * refused is said in the window, the one place a reader who started tpdf from
 * an editor or a script can see it, and the document still opens.
 */

import { parseZoomPercent, percentOf, MAX_ZOOM, MIN_ZOOM, type Fitted } from "./zoom";

/** A document handed over from outside: `launch::Opening`. */
export interface Opening {
  path: string;
  /** `--page`, as typed; `""` when the option had no value. */
  page: string | null;
  /** `--zoom`, as typed; `""` when the option had no value. */
  zoom: string | null;
  /** `--forward-search`, `file.tex:LINE`, the file made absolute (`forward.ts`). */
  forward: string | null;
}

/** What a viewer has to offer for a place to be applied. */
export interface Placeable {
  goToPage(page: number): void;
  setFit(mode: Fitted): void;
  setZoomFixed(zoom: number): void;
}

/** The fits by the names a command line uses, with a space or a hyphen. */
const FITS: Record<string, Fitted> = {
  "fit-width": "width",
  "fit-page": "page",
  "fit-height": "height",
  "fit-content": "content",
};

/** A `--zoom` value: a fit, a zoom, or what is wrong with it. */
export function launchZoom(raw: string): { fit: Fitted } | { zoom: number } | { problem: string } {
  const typed = raw.trim();
  const fit = FITS[typed.toLowerCase().replace(/\s+/g, "-")];
  if (fit) return { fit };
  const zoom = parseZoomPercent(typed);
  if (zoom !== null) return { zoom };
  return {
    problem:
      `tpdf was asked to open at --zoom "${typed}", which is not a zoom: use fit-width, fit-page, ` +
      `fit-height, fit-content, or a percentage from ${percentOf(MIN_ZOOM)} to ${percentOf(MAX_ZOOM)}`,
  };
}

/** A `--page` value: a page counted from 0, or what is wrong with it. */
export function launchPage(raw: string, pageCount: number): { page: number } | { problem: string } {
  const typed = raw.trim();
  const said = `tpdf was asked to open at --page "${typed}"`;
  if (!/^[0-9]+$/.test(typed)) return { problem: `${said}, which is not a page number` };
  const page = Number(typed);
  if (page < 1 || page > pageCount) {
    return { problem: `${said}, and this document has ${pageCount} page${pageCount === 1 ? "" : "s"}` };
  }
  return { page: page - 1 };
}

/**
 * Shows the opening's place on `viewer`, and says what could not be shown, or
 * `null` when everything asked for was.
 *
 * The zoom goes first, because a zoom keeps the middle of the window where it
 * was and would move the page that `--page` put at the top. Fit content is the
 * exception: it moves the view onto the content of the page in front, so it
 * goes after the page.
 */
export function applyPlace(viewer: Placeable, opening: Opening, pageCount: number): string | null {
  const problems: string[] = [];
  const zoom = opening.zoom === null ? null : launchZoom(opening.zoom);
  const page = opening.page === null ? null : launchPage(opening.page, pageCount);
  const content = zoom !== null && "fit" in zoom && zoom.fit === "content";
  if (zoom !== null && !content) {
    if ("fit" in zoom) viewer.setFit(zoom.fit);
    else if ("zoom" in zoom) viewer.setZoomFixed(zoom.zoom);
  }
  if (page !== null) {
    if ("page" in page) viewer.goToPage(page.page);
    else problems.push(page.problem);
  }
  if (content) viewer.setFit("content");
  if (zoom !== null && "problem" in zoom) problems.push(zoom.problem);
  return problems.length ? `${problems.join(". ")}.` : null;
}

/** Whether the opening asks for a place at all. */
export function hasPlace(opening: Opening): boolean {
  return opening.page !== null || opening.zoom !== null || opening.forward !== null;
}
