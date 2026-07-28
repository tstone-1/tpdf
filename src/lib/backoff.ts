/**
 * How long a failed request waits before it may be issued again.
 *
 * Lifted out of `scroller.ts` because the semantics are subtle in three places
 * and none of them were reachable from a test while the map was a private field
 * of a class that needs a webview to construct. What is subtle: a due entry is
 * kept rather than deleted, {@link Backoff.nextWaitMs} deliberately reports
 * nothing for an entry that is already due, and only a *reader's* action clears
 * it. Each of those is a busy loop if it goes the other way.
 *
 * There was no such wait, and its absence was the most expensive thing in the
 * scroller. `request()` runs every frame and issues any wanted tile that is in
 * neither the cache nor the in-flight set; a failure deleted the entry from
 * `inFlight` and recorded nothing, so the next frame asked for it again --- and
 * the frame loop cannot idle out while `pendingWork` is above zero, which the
 * re-issued requests kept true. A tile that fails deterministically was
 * therefore re-requested at display cadence for as long as the document stayed
 * open.
 *
 * That is not merely wasteful. Under the worker backend each failed tile costs a
 * `kill` and a fresh `fork`/`exec` plus a full re-parse of the document
 * (`render.rs`, `Workers::with_worker`), so a page that faults deterministically
 * had the application spawning and killing sandboxed processes indefinitely with
 * nobody touching the machine. `docs/THREAT-MODEL.md` §7 claimed that cost was
 * "bounded by the reader's own requests"; the reader makes one, and the frame
 * loop made the rest.
 *
 * Doubling rather than a fixed interval, because the two cases want opposite
 * things: a transient failure should recover in a quarter of a second, and a
 * permanent one should stop costing anything at all.
 *
 * The clock is passed in rather than read here. Two decisions are made about the
 * same instant --- "may this be issued yet" during a frame, and "when should the
 * loop be woken" as it goes idle --- and sampling the clock separately for each
 * lets an entry fall between the two readings, which loses the wake that would
 * have retried it.
 */

/** Shortest wait after a first failure, in milliseconds. */
export const RETRY_BASE_MS = 250;

/** Ceiling the wait doubles towards, in milliseconds. */
export const RETRY_MAX_MS = 8000;

interface Entry {
  /** Earliest this request may be issued again. */
  until: number;
  /** Failures so far, which is what the wait doubles on. */
  tries: number;
}

/** Per-key exponential backoff, with the clock supplied by the caller. */
export class Backoff {
  /**
   * Failed requests, and the earliest each may be issued again.
   *
   * A due entry is left here rather than deleted, so the *next* failure doubles
   * its wait instead of starting over --- a document that fails every time must
   * not settle into a fixed retry rate, which is the thing being fixed.
   */
  private readonly entries = new Map<string, Entry>();

  /** Whether a request that failed is still inside its wait. */
  blocked(id: string, now: number): boolean {
    const entry = this.entries.get(id);
    return entry !== undefined && entry.until > now;
  }

  /**
   * Records a failure and returns how many there have now been for this key.
   *
   * The count is returned rather than kept private because the caller's other
   * duty --- saying *why* a tile could not be drawn --- is worth doing exactly
   * once per key: the reason is the same on every retry, and a renderer that is
   * erroring on everything would otherwise fill the console at the retry rate.
   */
  note(id: string, now: number): number {
    const tries = (this.entries.get(id)?.tries ?? 0) + 1;
    const wait = Math.min(RETRY_BASE_MS * 2 ** (tries - 1), RETRY_MAX_MS);
    this.entries.set(id, { until: now + wait, tries });
    return tries;
  }

  /** Forgets a key, which is what a success means. */
  clear(id: string): void {
    this.entries.delete(id);
  }

  /** Forgets everything. */
  clearAll(): void {
    this.entries.clear();
  }

  /**
   * Milliseconds until the earliest waiting request may be issued again, or
   * `null` if nothing is waiting.
   *
   * The caller's frame loop idles when there is no work, so without this a tile
   * that failed would sit unretried until some unrelated input happened to wake
   * it --- a transient hiccup would leave a permanently blank square. One
   * scheduled wake per wait gives it exactly one retry, which is the whole
   * difference between recovering and spinning.
   *
   * A request whose wait has already elapsed is deliberately **not** counted: it
   * will be issued by the next frame that runs anyway, and reporting it as "due
   * in 0 ms" would have the caller schedule an immediate wake, which is the busy
   * loop this mechanism exists to remove, rebuilt one level up.
   */
  nextWaitMs(reference: number): number | null {
    let soonest = Infinity;
    for (const { until } of this.entries.values()) {
      if (until > reference && until < soonest) soonest = until;
    }
    return soonest === Infinity ? null : soonest - reference;
  }
}
