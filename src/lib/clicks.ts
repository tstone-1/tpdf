/**
 * Counting a run of clicks, so a double- and triple-click can be told apart.
 *
 * Kept out of the viewer for the usual reason --- it is arithmetic over a time
 * and two coordinates, and the interesting cases are all boundaries: a second
 * click one millisecond too late, one pixel too far, or one that arrives after
 * the counter has already reached three.
 *
 * **Not `PointerEvent.detail`**, which is the obvious answer and does not work.
 * `detail` carries a click count on `mousedown`, but the pointer events this
 * viewer listens to are specified to report `0` for it, and the value a given
 * webview actually supplies is neither guaranteed nor the same across the two
 * platforms tpdf ships on. Counting here also makes the thresholds ours to
 * state and to test, rather than the system's to change underneath us.
 */

/**
 * How long after a click a second one still belongs to it.
 *
 * macOS exposes a user setting for this and there is no API in a webview to
 * read it, so this is a fixed value near the platform default rather than the
 * true one. A reader with a slow double-click configured gets two single
 * clicks, which selects nothing wrong --- it just fails to select the word.
 */
export const MULTI_CLICK_MS = 500;

/**
 * How far the pointer may move between clicks and still count as the same run.
 *
 * Not zero: a click is a press *and* a release, and a hand holding a mouse
 * moves a pixel or two between them. Small enough that clicking a neighbouring
 * word starts a new run rather than extending the last.
 */
export const MULTI_CLICK_SLOP_PX = 4;

/**
 * Counts clicks that arrive close together in time and place.
 *
 * **The place is a point in the laid-out document, not on the screen**, and
 * that choice is what makes this correct rather than merely convenient. A click
 * is only part of a run if it lands on the same *text*, and between two clicks
 * a reader can scroll, zoom, rotate or jump to another page --- all of which
 * move the text while the pointer holds still. Fed screen coordinates, this
 * would call that a double-click and select a word on the strength of a click
 * aimed at something else, and the repair would be a `reset()` call at every
 * one of those call sites, silently wrong the day a fifth was added. Fed
 * document coordinates, every one of them moves the point and breaks the run
 * without being told to.
 */
export class ClickCounter {
  private count = 0;
  private atMs = -Infinity;
  private x = 0;
  private y = 0;

  /**
   * Records a press and returns its position in the run: 1, 2 or 3.
   *
   * `x` and `y` are in the laid-out document, not the viewport --- see the
   * class docs, where that is the whole of why there is no `reset`.
   *
   * Wraps back to 1 after 3 rather than counting upwards. A fourth click has no
   * larger unit to select --- there is no paragraph here --- so the choice is
   * between repeating the line selection and starting over, and starting over
   * is the one a reader can act on: it gives them a way back to a caret without
   * waiting for the timer to lapse.
   */
  press(x: number, y: number, nowMs: number): number {
    const near =
      Math.abs(x - this.x) <= MULTI_CLICK_SLOP_PX && Math.abs(y - this.y) <= MULTI_CLICK_SLOP_PX;
    const soon = nowMs - this.atMs <= MULTI_CLICK_MS;

    this.count = near && soon ? (this.count % 3) + 1 : 1;
    this.atMs = nowMs;
    this.x = x;
    this.y = y;
    return this.count;
  }
}
