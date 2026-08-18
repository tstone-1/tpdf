/**
 * Where a popup goes, for the two that hang off a mark on the page.
 *
 * One implementation, because the placement rule is the whole of what makes a
 * popup feel attached to what it belongs to: preferred to the right of the mark
 * and level with its top, flipped to the left when there is no room, and clamped
 * into the window either way. Two copies would agree about the middle of a page
 * and disagree at the edges --- which is the only place the rule does anything.
 *
 * The two callers are `commentpopup.ts`, which shows what a stranger wrote in
 * the file, and `markpopup.ts`, which edits what the reader wrote themselves.
 * They share nothing else: one is read-only and one is a form.
 */

/** Width of a popup, in CSS pixels. */
export const POPUP_WIDTH = 280;

/** Gap between the mark and the popup, in CSS pixels. */
const GAP = 10;

/** Margin kept between the popup and the window's edges, in CSS pixels. */
const MARGIN = 8;

/** A rectangle in the host's coordinates, which is what the viewer computes. */
export interface Anchor {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Moves `element` next to `at`, without rebuilding it.
 *
 * Called every frame while a popup is open, so it does no work beyond two style
 * writes --- and it reads the host's size each time rather than caching it,
 * because the window can be resized while a popup is open and a cached size
 * would put the clamp somewhere the window no longer is.
 */
export function place(host: HTMLElement, element: HTMLElement, at: Anchor): void {
  const width = host.clientWidth;
  const height = host.clientHeight;
  const own = element.offsetHeight || 0;

  // To the right of the mark, or to its left when there is no room. Compared
  // against the *window*, not against the page: a page narrower than the
  // window has room to its right even when the mark is at its edge.
  const rightOf = at.right + GAP;
  const left =
    rightOf + POPUP_WIDTH + MARGIN <= width
      ? rightOf
      : Math.max(MARGIN, at.left - GAP - POPUP_WIDTH);

  const top = Math.max(
    MARGIN,
    Math.min(at.top, Math.max(MARGIN, height - own - MARGIN)),
  );

  element.style.left = `${Math.round(left)}px`;
  element.style.top = `${Math.round(top)}px`;
}
