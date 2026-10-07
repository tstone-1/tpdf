/**
 * The tab strip and a mouse wheel.
 *
 * The strip of open documents scrolls sideways when it holds more tabs than
 * fit, and it shows no scrollbar: a bar under a row of tabs is a second row of
 * chrome for something a trackpad already does with two fingers. Reported from
 * use with many documents open.
 *
 * A trackpad sends a sideways delta and the strip scrolls by itself. A mouse
 * wheel sends only a vertical one, and without the bar that reader would have
 * no way to reach a tab that is out of view. So a turn of the wheel over the
 * strip moves it sideways, which is what this decides.
 */

/** `WheelEvent.deltaMode` when the delta counts lines. */
const LINES = 1;
/** `WheelEvent.deltaMode` when the delta counts pages. */
const PAGES = 2;
/** What one line of a wheel is worth, in CSS pixels. */
const LINE_PX = 40;

/**
 * How far a wheel event should move the strip sideways, in CSS pixels.
 *
 * Zero when the event is already mostly sideways: the strip scrolls itself for
 * that, and adding the vertical part on top would move it twice.
 *
 * `width` is the strip's visible width, which is what a page of scrolling is.
 */
export function sidewaysBy(
  wheel: { deltaX: number; deltaY: number; deltaMode: number },
  width: number,
): number {
  if (Math.abs(wheel.deltaX) >= Math.abs(wheel.deltaY)) return 0;
  if (wheel.deltaMode === LINES) return wheel.deltaY * LINE_PX;
  if (wheel.deltaMode === PAGES) return wheel.deltaY * width;
  return wheel.deltaY;
}
