/**
 * Carrying out what {@link ScrollLock} answers: the two viewers locked
 * together, the flag that tells a frame of the partner being moved from a
 * frame the reader made, and the short spell in which a wheel turned with Alt
 * scrolls its own side alone.
 *
 * `App.svelte` finds the two viewers (it owns the documents) and calls
 * {@link SyncFollow.step} from every frame of either; what a frame does to the
 * other side is decided and done here, where a test can reach it.
 */

import type { Reading, ScrollLock } from "./syncscroll";

/** The part of a viewer one side moves the other with. */
export interface Follower {
  readonly reading: Reading;
  readonly currentZoom: number;
  setZoomFixed(zoom: number): void;
  followTo(page: number, fraction: number): void;
}

/** How long a wheel turned with Alt held scrolls its own side alone, in milliseconds. */
export const ALONE_MS = 250;

export class SyncFollow<Id extends number, V extends Follower> {
  readonly #viewers = new Map<Id, V>();
  #following = false;
  #aloneUntil = 0;

  constructor(
    private readonly lock: ScrollLock<Id>,
    private readonly now: () => number = () => performance.now(),
  ) {}

  /**
   * Locks the two documents at the places their viewers are in, and keeps the
   * viewers beside the lock: {@link step} runs in every frame, and asking for
   * the other side's viewer there would cost a lookup a frame for an answer
   * that changes only when a document is mounted or torn down.
   */
  hold(first: { id: Id; viewer: V }, second: { id: Id; viewer: V }): void {
    const reading = (side: { id: Id; viewer: V }) =>
      ({ id: side.id, at: side.viewer.reading, zoom: side.viewer.currentZoom });
    this.lock.lock(reading(first), reading(second));
    this.#viewers.clear();
    this.#viewers.set(first.id, first.viewer);
    this.#viewers.set(second.id, second.viewer);
  }

  /** Ends the lock, and lets go of both viewers. */
  release(): void {
    this.lock.release();
    this.#viewers.clear();
  }

  /** From now, for {@link ALONE_MS}, a scroll moves only its own side. */
  aloneFor(): void {
    this.#aloneUntil = this.now() + ALONE_MS;
  }

  /**
   * Moves the other document to where a frame of `id`, shown by `leader`,
   * says it belongs. `fitted` is whether the leader is held to its side's
   * width or height, whose zoom the window and not the reader sets.
   *
   * Runs inside that frame: the other viewer draws before this returns, so the
   * two sides are never a frame apart. The partner's own frames during the
   * move are recognised as such and answered with nothing, which is what
   * stops a move being sent back where it came from.
   */
  step(id: Id, leader: V, fitted: boolean): void {
    const partnerId = this.lock.partnerOf(id);
    const partner = this.#viewers.get(partnerId);
    if (!partner) return;
    const following = this.#following;
    const factor = this.lock.zoomed(id, leader.currentZoom, { following, fitted });
    const moved = this.lock.moved(id, leader.reading,
      { following, alone: this.now() < this.#aloneUntil, partnerAt: partner.reading });
    if (factor === null && moved === null) return;
    this.#following = true;
    try {
      // The zoom first: it shifts the partner's own place, and the place asked
      // for after it is then the one it ends in.
      if (factor !== null) partner.setZoomFixed(partner.currentZoom * factor);
      const to = moved ?? this.lock.aim(id, leader.reading);
      if (to) partner.followTo(to.page, to.fraction);
      this.lock.seen(partnerId, partner.reading, partner.currentZoom);
    } finally { this.#following = false; }
  }
}
