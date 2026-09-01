/**
 * Maps the webview's `performance.now()` onto the Rust process timeline.
 *
 * The two clocks have different origins: `performance.now()` counts from
 * navigation start, the Rust one from process exec, and the gap between them is
 * exactly the thing spike 0.2 is trying to measure. So the mapping has to be
 * established, not assumed.
 *
 * The obvious version -- ask Rust for its elapsed time, subtract
 * `performance.now()` -- is wrong by most of an IPC round trip, because the two
 * readings are taken at different moments. That is the same problem NTP has, so
 * this borrows NTP's answer: bracket the call with local readings, assume the
 * remote timestamp was taken at the midpoint, and keep the sample whose round
 * trip was shortest, since that is the one with the least room to be wrong.
 *
 * Residual uncertainty is half the best round trip, and it is reported rather
 * than hidden -- a 300 ms budget deserves to know whether its milestones are
 * placed to 1 ms or to 20.
 */

import { call } from "./ipc";

export interface ProcessClock {
  /** Converts a `performance.now()` reading to ms since process exec. */
  toProcessMs(perfNow: number): number;
  /** Half the best observed round trip: the mapping's error bar. */
  uncertaintyMs: number;
  /** The best observed round trip, for reporting. */
  roundTripMs: number;
}

/**
 * Calibrates the mapping.
 *
 * Note `performance.now()` is clamped to 1 ms in this webview, so a round trip
 * faster than that reads as 0 and the uncertainty is floored by the clock's own
 * resolution rather than by the IPC.
 */
export async function calibrateProcessClock(samples = 9): Promise<ProcessClock> {
  let bestRoundTrip = Infinity;
  let offsetMs = 0;

  for (let i = 0; i < samples; i++) {
    const before = performance.now();
    const elapsed = await call("process_elapsed_ms");
    const after = performance.now();

    const roundTrip = after - before;
    if (roundTrip < bestRoundTrip) {
      bestRoundTrip = roundTrip;
      offsetMs = elapsed - (before + after) / 2;
    }
  }

  return {
    toProcessMs: (perfNow: number) => perfNow + offsetMs,
    uncertaintyMs: bestRoundTrip / 2,
    roundTripMs: bestRoundTrip,
  };
}
