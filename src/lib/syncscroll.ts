/**
 * Two documents side by side, scrolled as one.
 *
 * What is held is an offset between two places, each a page and how far down
 * it the top of the view is. With the left side at page 3, 40% down, and the
 * right at page 5, 40% down, the right side stays two pages ahead. Pages and
 * fractions rather than pixels, because the two documents need not share a
 * zoom or a page size, and a viewer corrects its page heights as it reads them:
 * a pixel offset is wrong after any of the three.
 *
 * This decides and moves nothing. `App.svelte` reports every frame of either
 * document to {@link ScrollLock.moved} and carries out what it answers.
 */

/** The top of a view: a page, counted from 0, and the share of that page above it. */
export interface Reading {
  page: number;
  fraction: number;
}

/** A reading as one number: whole pages and the part of the next. */
export function lineOf(at: Reading): number {
  return at.page + at.fraction;
}

/** The reading a line stands for. Before the first page is the first page's top. */
export function readingAt(line: number): Reading {
  const held = Math.max(0, line);
  const page = Math.floor(held);
  return { page, fraction: held - page };
}

/** Two lines closer than this are the same place: a frame that moved nothing. */
const SAME = 1e-6;

export class ScrollLock<Id extends number = number> {
  #pair: [Id, Id] | null = null;
  /** How far the second document of the pair is ahead of the first, in pages. */
  #ahead = 0;
  readonly #last = new Map<Id, number>();
  readonly #zoom = new Map<Id, number>();

  /** Whether `id` is one of the two documents locked together. */
  locks(id: Id): boolean {
    return this.#pair !== null && this.#pair.includes(id);
  }

  /** The document locked to `id`, or -1. */
  partnerOf(id: Id): Id {
    if (!this.#pair || !this.#pair.includes(id)) return -1 as Id;
    return this.#pair[0] === id ? this.#pair[1] : this.#pair[0];
  }

  /**
   * Locks two documents at the places they are in now. Called when the reader
   * turns the lock on, and again when a side shows a different tab.
   */
  lock(first: { id: Id; at: Reading; zoom: number }, second: { id: Id; at: Reading; zoom: number }): void {
    this.#pair = [first.id, second.id];
    this.#ahead = lineOf(second.at) - lineOf(first.at);
    this.#last.clear();
    this.#zoom.clear();
    this.#last.set(first.id, lineOf(first.at));
    this.#last.set(second.id, lineOf(second.at));
    this.#zoom.set(first.id, first.zoom);
    this.#zoom.set(second.id, second.zoom);
  }

  release(): void {
    this.#pair = null;
    this.#last.clear();
    this.#zoom.clear();
  }

  /**
   * A frame of document `id` showed it at `at`. Answers where its partner
   * goes, or null when the partner stays.
   *
   * The partner stays when the frame moved nothing, which is most frames: a
   * tile arriving draws a frame too. That is also what keeps a document held
   * at its last page from dragging the longer one back. With `following`, the
   * frame is the partner being moved by an earlier answer, and answering it
   * would send the move back where it came from.
   *
   * With `alone`, the reader is scrolling one side on purpose: the partner
   * stays and the offset becomes whatever the two places now are, for which
   * `partnerAt` is needed.
   */
  moved(
    id: Id,
    at: Reading,
    how: { following: boolean; alone: boolean; partnerAt: Reading },
  ): Reading | null {
    if (!this.#pair || !this.#pair.includes(id)) return null;
    const line = lineOf(at);
    const before = this.#last.get(id);
    this.#last.set(id, line);
    if (how.following) return null;
    if (before !== undefined && Math.abs(line - before) < SAME) return null;
    const leadsFirst = this.#pair[0] === id;
    if (how.alone) {
      const partner = lineOf(how.partnerAt);
      this.#ahead = leadsFirst ? partner - line : line - partner;
      return null;
    }
    return readingAt(leadsFirst ? line + this.#ahead : line - this.#ahead);
  }

  /**
   * Where the partner of `id` belongs while `id` is at `at`, whether or not
   * anything moved. For after a zoom, which shifts the partner's own place.
   */
  aim(id: Id, at: Reading): Reading | null {
    if (!this.#pair || !this.#pair.includes(id)) return null;
    const line = lineOf(at);
    return readingAt(this.#pair[0] === id ? line + this.#ahead : line - this.#ahead);
  }

  /**
   * Records where `id` is and at what zoom without answering anything: it was
   * put there by its partner, and its next frame is to find nothing changed.
   */
  seen(id: Id, at: Reading, zoom: number): void {
    if (!this.#pair || !this.#pair.includes(id)) return;
    this.#last.set(id, lineOf(at));
    this.#zoom.set(id, zoom);
  }

  /**
   * A frame of document `id` showed it at `zoom`. Answers the factor to zoom
   * its partner by, or null.
   *
   * Only for a zoom the reader set. `fitted` is whether the document is held
   * to its side's width or height: its zoom then follows the window, both
   * sides are refitted by the same resize, and passing the change across would
   * apply it twice.
   */
  zoomed(id: Id, zoom: number, how: { following: boolean; fitted: boolean }): number | null {
    if (!this.#pair || !this.#pair.includes(id)) return null;
    const before = this.#zoom.get(id);
    this.#zoom.set(id, zoom);
    if (how.following || how.fitted || before === undefined || before <= 0) return null;
    const factor = zoom / before;
    return Math.abs(factor - 1) < SAME ? null : factor;
  }
}
