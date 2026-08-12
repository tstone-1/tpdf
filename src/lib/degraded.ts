/**
 * When the status bar is allowed to say that the view is not yet sharp.
 *
 * `docs/PLAN.md` section 9 owes the reader an account of a page that is
 * legitimately blurry for seconds at a time, because a viewer that says nothing
 * about it is indistinguishable from one that is broken. That account is worth
 * keeping. What it must not do is appear and vanish several times a second
 * while somebody scrolls, which is what it did: every dip in coverage put a
 * label into the header and every recovery took it out again.
 *
 * The existing thresholds already encode the same judgement one step lower
 * down. They sit just short of 1 rather than at it because a tile boundary that
 * lands a rounding step inside the viewport leaves a fraction of a percent
 * uncovered on a page that is fully rendered --- and, in that comment's own
 * words, "a status line that flickers on that is worse than none". A fast
 * scroll is the same failure with a different cause: the coverage really has
 * fallen, and reporting it truthfully at display cadence is still worse than
 * none.
 *
 * So the fix is a delay, not a wider threshold. Nothing here changes what
 * counts as degraded; it changes only how long a transient episode has to last
 * before it is worth telling anybody about.
 *
 * **The delay is deliberately not applied to a failure.** A blurry page is a
 * promise that waiting will fix it, and waiting does fix it; a request that
 * came back an error is the one state where that promise is false, which is why
 * it is tested first both here and in the caller. It is also the one state that
 * can arrive with the frame loop already quiescent --- see the note on
 * {@link DegradedLabel.update} --- so delaying it would not merely postpone the
 * message, it could suppress it for as long as the document stays open.
 */

/**
 * How long a transient degraded state must hold before it is shown.
 *
 * Long enough that a scroll which resolves within a few frames says nothing at
 * all, short enough that the A0 vector page this indicator exists for --- which
 * is blurry for whole seconds --- still reports itself almost immediately.
 */
export const SHOW_AFTER_MS = 300;

/** The coverage numbers this decision reads, as {@link ViewerStatus} reports them. */
export interface Coverage {
  /** Requests that came back an error since the document opened. */
  failed: number;
  /** Fraction backed by anything at all, tier-1 placeholder included. */
  any: number;
  /** Fraction of the visible page area backed by a sharp tile. */
  sharp: number;
  /** Requests outstanding. */
  pending: number;
}

/** What the surface is doing, when it is not simply showing the document. */
export interface Degraded {
  /** The words shown to the reader. */
  text: string;
  /**
   * Whether waiting fixes it.
   *
   * `false` for every state that is merely slow, and the only reason this flag
   * exists: an urgent state is shown at once, a transient one is timed.
   */
  urgent: boolean;
}

/**
 * Classifies the coverage, with no notion of time.
 *
 * Split out from the timing so that the ordering below --- which is the part
 * that was reasoned about once and is easy to get subtly wrong again --- can be
 * tested without a clock at all.
 */
export function describeDegraded(status: Coverage | null): Degraded | null {
  if (!status) return null;
  // First, because it is the one state waiting does not fix. "preparing page"
  // in front of a renderer that is erroring on every request is a lie by
  // omission.
  if (status.failed > 0) return { text: "some pages could not be drawn", urgent: true };
  if (status.any < 0.999) return { text: "preparing page", urgent: false };
  if (status.sharp < 0.999) return { text: "sharpening", urgent: false };
  return status.pending > 0 ? { text: "loading ahead", urgent: false } : null;
}

/**
 * Holds a transient degraded state back until it has lasted {@link SHOW_AFTER_MS}.
 *
 * The clock is passed in rather than read here, for the reason `Backoff` gives:
 * a test that has to wait 300 ms to observe a 300 ms delay is a slow test that
 * still cannot say which of the two readings the code used.
 *
 * There is deliberately **no** minimum display time on the way out. Adding one
 * would need a timer, because the thing that drives this is the viewer's status
 * callback and that stops arriving once the view is sharp --- so a hold would
 * be a promise the caller has no way to keep, and the label would stick until
 * some unrelated event moved it. Hiding the instant the page is readable is
 * also simply correct: nobody is annoyed by a warning that stops being true.
 *
 * The episode is timed, not the wording. A page that goes from "preparing page"
 * to "sharpening" without ever becoming readable is one continuous episode from
 * the reader's point of view, and restarting the clock on the change would hide
 * a genuinely slow page for twice as long --- or indefinitely, if the two
 * states alternate.
 */
export class DegradedLabel {
  /** When the current transient episode began, or `null` between episodes. */
  #since: number | null = null;

  /**
   * The label to show now, or `null` for nothing.
   *
   * Call on every status update. `nowMs` is any monotonic millisecond reading;
   * only differences are used.
   */
  update(status: Coverage | null, nowMs: number): string | null {
    const next = describeDegraded(status);
    if (!next) {
      this.#since = null;
      return null;
    }
    if (next.urgent) return next.text;
    if (this.#since === null) this.#since = nowMs;
    return nowMs - this.#since >= SHOW_AFTER_MS ? next.text : null;
  }
}
