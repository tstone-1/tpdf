/**
 * Reopening where the reader left off.
 *
 * The store itself is in `src-tauri/src/session.rs`; this is the half that
 * decides *when* to write and what a remembered place means against the
 * document actually on disk now.
 *
 * Two things here are less obvious than they look.
 *
 * **Writes are throttled and chained.** A position changes on every scroll, and
 * a file write per frame would be absurd --- so notes collapse into at most one
 * write per interval, leading edge and trailing edge. They are also chained
 * through a single promise rather than fired independently, because `invoke`
 * resolves out of order under load: two writes issued a second apart can land
 * in the other order, and the loser overwrites the newer place with an older
 * one. That has already cost this repository a shuffled transcript once.
 *
 * **A remembered page is clamped, not trusted.** The file may have been edited,
 * replaced or truncated since. See {@link clampPlace}.
 */

import { invoke } from "@tauri-apps/api/core";

import type { FitMode } from "./zoom";

/** Where one document was left. Field names match the Rust struct. */
export interface Place {
  path: string;
  /** Zero-based page at the top of the viewport. */
  page: number;
  /** Points down that page. A rotated view reports 0 --- see `viewer.ts`. */
  top_pt: number;
  zoom: number;
  /** What the zoom was following, if anything. Matches Rust's `Fit`. */
  fit: FitMode;
  /** Quarter-turns clockwise, 0 to 3. */
  turns: number;
  sidebar: boolean;
  /** Pages the document had when this was written. */
  page_count: number;
}

export interface Session {
  places: Place[];
  /**
   * Whether pages are shown with their lightness inverted.
   *
   * A preference, not a place: it belongs to the reader rather than to any one
   * document. Optional because a session file written before it existed has no
   * such field, and an older file must not be discarded over a missing one.
   */
  invert_pages?: boolean;
}

/**
 * Shortest gap between two writes of the same document, in milliseconds.
 *
 * A second is long enough that continuous scrolling costs one write per second
 * and short enough that the last thing a reader did is almost always recorded
 * before they quit. The residual --- quitting inside the window --- is covered
 * by {@link SessionWriter.flush}, not by making this smaller.
 */
export const SAVE_INTERVAL_MS = 1000;

/**
 * Fits a remembered place to the document as it is now.
 *
 * The path is the only thing tying the two together, and a path is not an
 * identity: the file may have been rebuilt, replaced with a shorter draft, or
 * truncated since it was last read. Landing on a page that no longer exists
 * would leave the viewer scrolled past the end of its own document.
 *
 * The page is clamped rather than dropped. On a document that changed length
 * the remembered page is a guess, but it is a far better one than page 1 ---
 * and it is what every reader that does this at all does.
 */
export function clampPlace(place: Place, pageCount: number): Place {
  const last = Math.max(0, pageCount - 1);
  const page = Math.min(Math.max(0, Math.floor(place.page)), last);
  return { ...place, page, page_count: pageCount };
}

/**
 * Whether two places would produce the same record.
 *
 * Compared rather than written unconditionally so that a document nobody is
 * touching costs no writes at all: `onStatus` fires for reasons that do not
 * move the reader --- a tile arriving changes `sharp`, and coverage is not part
 * of a place.
 */
export function samePlace(a: Place, b: Place): boolean {
  return (
    a.path === b.path &&
    a.page === b.page &&
    a.top_pt === b.top_pt &&
    a.zoom === b.zoom &&
    a.fit === b.fit &&
    a.turns === b.turns &&
    a.sidebar === b.sidebar
  );
}

/** Reads the remembered places, most recently read first. */
export async function loadSession(): Promise<Session> {
  try {
    return await invoke<Session>("session_load");
  } catch {
    // The backend already treats every failure as an empty session, so this
    // catches only the IPC itself failing --- in which case there is nothing to
    // restore and nothing worth telling the reader.
    return { places: [] };
  }
}

/** Collapses a stream of positions into at most one write per interval. */
export class SessionWriter {
  private pending: Place | null = null;
  private written: Place | null = null;
  private timer = 0;
  private queue: Promise<unknown> = Promise.resolve();
  private stopped = false;

  /**
   * @param send    Writes one place. Injected so the throttle can be tested
   *                without a backend.
   * @param interval Milliseconds between writes.
   */
  constructor(
    private readonly send: (place: Place) => Promise<unknown> = (place) =>
      invoke("session_remember", { place }),
    private readonly interval: number = SAVE_INTERVAL_MS,
  ) {}

  /**
   * Records where the reader is now.
   *
   * The first note of a quiet period is written immediately, so a document
   * opened and closed at once is still remembered. Notes arriving during the
   * interval that follows are collapsed into one trailing write.
   */
  note(place: Place): void {
    if (this.stopped) return;
    if (this.written && samePlace(this.written, place)) {
      // Nothing a reader would notice changed. Any trailing write already
      // scheduled is left alone: it may be carrying an *older* pending place
      // that still needs to land.
      return;
    }

    if (this.timer !== 0) {
      this.pending = place;
      return;
    }

    this.write(place);
    this.timer = setTimeout(() => {
      this.timer = 0;
      const trailing = this.pending;
      this.pending = null;
      if (trailing) this.note(trailing);
    }, this.interval) as unknown as number;
  }

  /**
   * Writes whatever is outstanding now, without waiting for the interval.
   *
   * For the paths where there may be no next interval --- the window closing,
   * the page being hidden. Best effort by construction: the write is an async
   * IPC call and the process may not outlive it.
   */
  flush(): void {
    const outstanding = this.pending;
    this.pending = null;
    if (outstanding && !(this.written && samePlace(this.written, outstanding))) {
      this.write(outstanding);
    }
  }

  /** Stops accepting notes, and drops any scheduled write. */
  stop(): void {
    this.stopped = true;
    this.pending = null;
    if (this.timer !== 0) {
      clearTimeout(this.timer);
      this.timer = 0;
    }
  }

  /** Writes issued so far, so a check can assert the throttle collapsed them. */
  get writes(): number {
    return this.count;
  }

  private count = 0;

  /**
   * Issues one write, behind every write already issued.
   *
   * `written` is updated when the call is *made*, not when it resolves, so that
   * a note identical to one already in flight is still suppressed.
   */
  private write(place: Place): void {
    this.written = place;
    this.count += 1;
    this.queue = this.queue.then(
      () => this.send(place),
      () => this.send(place),
    );
    // A failed write must not poison the chain, or the first error stops every
    // later position from being recorded.
    this.queue = this.queue.catch(() => undefined);
  }
}
