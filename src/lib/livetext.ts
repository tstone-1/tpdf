/**
 * A live region's text, written when a screen reader will hear it.
 *
 * VoiceOver drops a polite announcement made while tpdf's window is not the
 * key window, or is just becoming it again: the page announcement at launch,
 * under "tpdf window, has keyboard focus", and the report of an extraction,
 * under "Closing dialogue" as the save sheet went. Measured with
 * `voiceover_check.py` on 2026-11-15; every announcement made in the steady
 * state was heard. NVDA heard both.
 *
 * So a text written while the window lacks the focus waits for it, and one
 * written within {@link SETTLE_MS} of the focus coming back waits out the
 * rest of that. Only the latest text is kept: a region holds one message, and
 * a superseded one is not news. Clearing waits the same way, so an old text
 * is never cleared after a newer one was written.
 */

/** How long after the window gets the focus back a text is held. */
export const SETTLE_MS = 400;

/**
 * How long after the page started a text is held: the window opening. A
 * document opened later in a running window is not held. VoiceOver began the
 * window's phrase about 4 s after the app was opened and spoke it for about
 * 3 s, and a page announcement held for {@link SETTLE_MS} was lost under it.
 */
export const OPENING_MS = 5000;

/** What the writer needs to know of the window, so a test can be the window. */
export interface FocusClock {
  hasFocus(): boolean;
  /** Milliseconds since the page started, as `performance.now()` counts them. */
  now(): number;
  /** Calls `listener` each time the window gets the focus. */
  onFocus(listener: () => void): void;
  later(run: () => void, ms: number): void;
}

/** The browser's window. */
export function windowClock(): FocusClock {
  return {
    hasFocus: () => document.hasFocus(),
    now: () => performance.now(),
    onFocus: (listener) => window.addEventListener("focus", listener),
    later: (run, ms) => void setTimeout(run, ms),
  };
}

/**
 * `write`, held back as the header says: until {@link OPENING_MS} after the
 * page started, and {@link SETTLE_MS} after each time the window gets the
 * focus, whichever ends later.
 */
export function whenHeard(write: (text: string) => void, clock: FocusClock = windowClock()): (text: string) => void {
  let focusedAt = -Infinity;
  let pending: string | null = null;
  let waiting = false;
  const flush = (): void => {
    waiting = false;
    if (pending === null || !clock.hasFocus()) return;
    const rest = Math.max(OPENING_MS, focusedAt + SETTLE_MS) - clock.now();
    if (rest > 0) {
      waiting = true;
      clock.later(flush, rest);
      return;
    }
    write(pending);
    pending = null;
  };
  clock.onFocus(() => {
    focusedAt = clock.now();
    if (pending !== null && !waiting) {
      waiting = true;
      clock.later(flush, SETTLE_MS);
    }
  });
  return (text) => {
    pending = text;
    if (!waiting) flush();
  };
}
