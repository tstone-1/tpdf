/**
 * How large the page is drawn, and what the zoom is following.
 *
 * Split out of `viewer.ts` rather than added to it, because every decision here
 * is arithmetic over four numbers and none of it needs a DOM --- which is the
 * difference between a unit test and a check that has to launch a webview.
 * `viewer.ts` keeps the *state*; this file answers the questions about it.
 *
 * ## Fitting is a mode, not a flag
 *
 * It was a boolean --- "is the zoom following the window width" --- and a
 * boolean cannot hold three answers. Fit-page is not fit-width with a different
 * number: it has to survive a resize and a rotation the same way fit-width
 * does, which means the viewer has to remember *which* fit to re-apply, not
 * merely that it is fitting something.
 *
 * The two spellings are not kept side by side. A `fitting: boolean` beside a
 * `fit: FitMode` is two records of one fact, and the one the viewer reads is
 * not necessarily the one a test asserts on --- `docs/TRAPS.md` has that entry
 * already. The boolean is gone, including out of the session file.
 *
 * ## Fit-page is the smaller of the two fits, and that is the whole of it
 *
 * A page fitted to the viewport's height alone is cut off at the sides on
 * anything wider than the window; fitted to its width alone it is what
 * fit-width already does. Fitting *the page* means both bounds hold at once, so
 * it is the smaller zoom --- which on a portrait page in a landscape window is
 * the height, and on a spreadsheet in a tall window is the width.
 */

/** What the zoom is following, if anything. */
export type FitMode = "none" | "width" | "page";

/** The fits that compute a zoom. `"none"` is the absence of one. */
export type Fitted = Exclude<FitMode, "none">;

/**
 * Zoom stops, in CSS pixels per PDF point.
 *
 * A ladder rather than a continuous zoom because every step throws away every
 * tier-2 tile: on the A0 sheet each one costs about a second to replace, so a
 * zoom bound to a trackpad's pinch resolution would queue work faster than the
 * renderer can retire it and never converge on anything.
 */
export const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3, 4, 6, 8];

/**
 * Zoom bounds, in CSS pixels per PDF point.
 *
 * `src-tauri/src/session.rs` holds the same two numbers, because a session file
 * is sanitized on the side that reads it from disk and that side is Rust. They
 * are a pair that has to be changed together; there is no import that crosses
 * the language boundary to enforce it.
 */
export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 16;

/** Margin either side of the page when fitting, in CSS pixels. */
export const FIT_MARGIN = 24;

/** The viewport a page is being fitted into, in CSS pixels. */
export interface Viewport {
  width: number;
  height: number;
}

/** A page's size as displayed, i.e. after the view rotation, in points. */
export interface PagePoints {
  width_pt: number;
  height_pt: number;
}

/** Forces a zoom into the range the renderer and the session file agree on. */
export function clampZoom(zoom: number): number {
  // A viewport of zero and a page of zero both produce NaN, and NaN survives
  // `Math.max`/`Math.min` unchanged --- it would reach the scroller as a page
  // size of NaN and lay out nothing at all, silently. The constructor really
  // does run against a viewport of one pixel (see `viewer.ts`), so this is not
  // a defensive hypothetical.
  if (!Number.isFinite(zoom)) return MIN_ZOOM;
  return Math.max(MIN_ZOOM, Math.min(zoom, MAX_ZOOM));
}

/**
 * The zoom at which `page` fits `viewport` under `mode`.
 *
 * `"none"` is not accepted, and that is the point of {@link Fitted} existing:
 * there is no zoom that means "not fitting", so a caller that has not decided
 * yet cannot call this by accident. The check that would otherwise be here
 * would be unreachable, and an unreachable guard reads as load-bearing.
 */
export function fitZoom(mode: Fitted, viewport: Viewport, page: PagePoints): number {
  const wide = (viewport.width - FIT_MARGIN * 2) / page.width_pt;
  if (mode === "width") return clampZoom(wide);
  // No vertical margin, unlike the horizontal one: pages are laid out flush
  // against each other with only `PAGE_GAP` between them, so there is no air at
  // the top of the first page to leave room for. Subtracting one here would
  // make the page smaller than it needs to be to be wholly visible, which is
  // the only thing fit-page is for.
  return clampZoom(Math.min(wide, viewport.height / page.height_pt));
}

/**
 * The next zoom stop past `zoom` in `direction`, or `null` at the end.
 *
 * `null` rather than the end stop again: the caller pins the fit mode off when
 * it steps, and returning the same zoom would pin it off on a keypress that did
 * nothing. A reader at 800% pressing zoom-in has not asked to stop fitting.
 */
export function nextStop(zoom: number, direction: 1 | -1): number | null {
  // The epsilon is against a fitted zoom that happens to sit within floating
  // point noise of a stop: without it, zoom-in from a fit-width of exactly 1
  // would find 1 itself and appear to do nothing.
  const stop =
    direction > 0
      ? ZOOM_STEPS.find((z) => z > zoom + 1e-6)
      : [...ZOOM_STEPS].reverse().find((z) => z < zoom - 1e-6);
  return stop ?? null;
}

/**
 * A typed zoom, as a scale, or `null` if it is not one.
 *
 * Percentages, because that is what the toolbar shows and what every other
 * reader accepts --- `150`, with the `%` optional since typing it is work and
 * omitting it is unambiguous. Out of range is `null` rather than clamped, for
 * the reason `nav.goToPage` refuses a page past the end: a reader who typed
 * 5000 has made a mistake, and silently going to 1600% hides it.
 */
export function parseZoomPercent(raw: string): number | null {
  const trimmed = raw.trim().replace(/\s*%$/, "");
  // Deliberately not `Number()`, which accepts `1e3`, `0x10`, `Infinity` and
  // the empty string. A zoom is a plain decimal number and nothing else.
  if (!/^[0-9]+(\.[0-9]+)?$/.test(trimmed)) return null;
  const zoom = Number(trimmed) / 100;
  if (zoom < MIN_ZOOM || zoom > MAX_ZOOM) return null;
  return zoom;
}

/** What the zoom is following, in words. For a tooltip and the palette. */
export function describeFit(mode: FitMode): string {
  if (mode === "width") return "Fit width";
  if (mode === "page") return "Fit page";
  return "Fixed zoom";
}

/** A zoom as a reader reads it: whole percent, no decimals. */
export function percentOf(zoom: number): number {
  return Math.round(zoom * 100);
}
