/**
 * The links on a page, mirroring `links.rs`, plus the history that makes them
 * usable.
 *
 * ## A link is a rectangle and a destination, and nothing else
 *
 * There is no title, no tooltip and no URL here, because there is none in the
 * backend's `Link` either --- see `links.rs` on why that is structural rather
 * than careful. Everything a reader is told about a link is composed from our
 * own words and the destination's *page number*, both of which are ours.
 *
 * ## Following one without a way back is a trap, not a feature
 *
 * A cross-reference in a 7,694-link document is a jump into the middle of
 * something; a reader who cannot get back has been moved rather than helped, and
 * has to remember a page number to undo it. {@link History} is the smallest
 * thing that fixes that: where you were before each jump, and where you were
 * before you went back.
 *
 * It records *positions*, not links, so a jump from the outline or the search
 * panel is on the same stack --- which is what a reader expects from a Back that
 * they have used in a browser. It deliberately does **not** record ordinary
 * scrolling: a stack that grew on every wheel event would make Back mean
 * "un-scroll a little", and the reader would never reach the place they came
 * from.
 */

import { onPage as placedOnPage, turnedFor as placedTurnedFor } from "./comments";
import { isNavigable, reasonFor, type Target } from "./outline";

/** One clickable rectangle, mirroring `links.rs`'s `Link`. */
export interface Link {
  id: number;
  page: number;
  /** `[left, top, right, bottom]` in points from the displayed page's top-left. */
  rect: [number, number, number, number];
  target: Target;
}

/** What the scan cut, mirroring `links.rs`'s `Limits`. */
export interface LinkLimits {
  crowded_pages: number;
  over_budget: boolean;
  unreadable: number;
  unresolved_names: number;
}

/** A whole document's links, mirroring `links.rs`'s `Links`. */
export interface Links {
  items: Link[];
  limits: LinkLimits;
  scan_ms: number;
}

/**
 * Points of slack around a link's rectangle when hit-testing.
 *
 * Smaller than {@link import("./comments").HIT_SLACK_PT}, and deliberately so.
 * A sticky note is a 24-point icon a reader aims at; a link is a run of text
 * that usually has more text right beside it, so slack there does not make a
 * small target reachable --- it makes the *neighbouring* link reachable, and a
 * cross-reference that jumps to the wrong section is worse than one that needs
 * a second click.
 */
export const LINK_SLACK_PT = 1;

/** The link under a point on `page`, in page points, or `null`. */
export function linkAt(
  items: readonly Link[],
  page: number,
  x: number,
  y: number,
): Link | null {
  // The smallest hit wins, which matters where a producer wraps a whole
  // paragraph in one link and a phrase inside it in another.
  let best: Link | null = null;
  let bestArea = Infinity;

  for (const item of items) {
    if (item.page !== page) continue;
    const [left, top, right, bottom] = item.rect;
    const width = right - left;
    const height = bottom - top;
    if (width <= 0 || height <= 0) continue;
    if (
      x < left - LINK_SLACK_PT ||
      x > right + LINK_SLACK_PT ||
      y < top - LINK_SLACK_PT ||
      y > bottom + LINK_SLACK_PT
    ) {
      continue;
    }
    const area = width * height;
    if (area <= bestArea) {
      best = item;
      bestArea = area;
    }
  }
  return best;
}

/** The links on one page. */
export function onPage(items: readonly Link[], page: number): Link[] {
  return placedOnPage(items, page);
}

/** The same links with their rectangles turned into the view's space. */
export function turnedFor(
  items: readonly Link[],
  turns: number,
  width: number,
  height: number,
): Link[] {
  return placedTurnedFor(items, turns, width, height);
}

/**
 * What a reader is told when a link does not take them anywhere.
 *
 * `null` for one that does, which is what the caller branches on. The words come
 * from `outline.ts`, so a refusal reads identically whether the reader met it in
 * the outline or on the page --- two wordings for one policy would read as two
 * policies.
 */
export function refusalFor(target: Target): string | null {
  if (isNavigable(target)) return null;
  const reason = reasonFor(target);
  return reason ? `This link ${reason}.` : null;
}

/**
 * What the panel says when the scan cut something, or `null` when it did not.
 *
 * Every bound `links.rs` applies is named, for the reason that module states: a
 * list shown as complete when it is not is the failure the whole arrangement is
 * built to avoid. Here it is not hypothetical --- the 7,694-link document
 * measured in that module's header is over the per-document budget by itself.
 */
export function noticeFor(limits: LinkLimits): string | null {
  const parts: string[] = [];
  if (limits.over_budget) parts.push("too many links to list them all");
  if (limits.crowded_pages > 0) {
    parts.push(
      limits.crowded_pages === 1
        ? "one page had more links than tpdf lists"
        : `${limits.crowded_pages} pages had more links than tpdf lists`,
    );
  }
  if (limits.unreadable > 0) {
    parts.push(
      limits.unreadable === 1
        ? "one annotation could not be read"
        : `${limits.unreadable} annotations could not be read`,
    );
  }
  if (limits.unresolved_names > 0) {
    parts.push(
      limits.unresolved_names === 1
        ? "one named destination could not be resolved"
        : `${limits.unresolved_names} named destinations could not be resolved`,
    );
  }
  if (parts.length === 0) return null;
  return `Some links are missing: ${parts.join("; ")}.`;
}

/** Somewhere in the document a reader was: a page and points down it. */
export interface Place {
  page: number;
  top: number;
}

/**
 * How far apart two places must be before a jump is worth recording, in points.
 *
 * A link that lands a reader where they already were should not push an entry
 * they then have to press Back twice to get past --- and a destination is often
 * a few points off the current scroll simply because of the margin the viewer
 * leaves above it. Half a page is far enough that a genuine jump always counts
 * and a near-miss never does.
 */
export const SAME_PLACE_PT = 360;

/** Whether two places are close enough that moving between them is not a jump. */
export function samePlace(a: Place, b: Place): boolean {
  return a.page === b.page && Math.abs(a.top - b.top) < SAME_PLACE_PT;
}

/**
 * Deepest the back stack goes.
 *
 * Bounded for the reason every list here is: an unbounded one is a document's to
 * grow. Dropping from the *bottom* rather than refusing at the top, because the
 * oldest entry is the one a reader is least likely to want and refusing would
 * silently stop recording at exactly the point navigation got interesting.
 */
export const MAX_HISTORY = 200;

/**
 * Where a reader has been, and where they went back from.
 *
 * Two stacks, the shape every browser uses: {@link back} pops onto
 * {@link forward}, and any new jump clears {@link forward} --- because a reader
 * who goes back and then somewhere else has abandoned the branch they were on,
 * and keeping it would make Forward jump somewhere they never chose.
 */
export class History {
  private readonly past: Place[] = [];
  private readonly future: Place[] = [];

  /** Records a jump from `from`, discarding any forward branch. */
  push(from: Place): void {
    const top = this.past.at(-1);
    // Two presses on the same cross-reference are one place to come back to.
    if (top && samePlace(top, from)) {
      this.future.length = 0;
      return;
    }
    this.past.push(from);
    if (this.past.length > MAX_HISTORY) this.past.shift();
    this.future.length = 0;
  }

  /** Whether there is anywhere to go back to. */
  get canGoBack(): boolean {
    return this.past.length > 0;
  }

  /** Whether there is anywhere to go forward to. */
  get canGoForward(): boolean {
    return this.future.length > 0;
  }

  /**
   * Steps back, given where the reader is now.
   *
   * `now` goes onto the forward stack rather than the place that was popped,
   * which is the difference between Forward meaning "where I was going" and
   * meaning "where I just came from". Getting it the other way round produces a
   * pair of buttons that bounce between two positions and never advance.
   */
  back(now: Place): Place | null {
    const to = this.past.pop();
    if (!to) return null;
    this.future.push(now);
    return to;
  }

  /** Steps forward, given where the reader is now. */
  forward(now: Place): Place | null {
    const to = this.future.pop();
    if (!to) return null;
    this.past.push(now);
    return to;
  }

  /** Forgets everything, for a new document. */
  clear(): void {
    this.past.length = 0;
    this.future.length = 0;
  }

  /** How deep each stack is. For the check harness. */
  get depths(): { back: number; forward: number } {
    return { back: this.past.length, forward: this.future.length };
  }
}
