/**
 * The links on a page, mirroring `links.rs`, plus the history that makes them
 * usable.
 *
 * ## A link carries its hit regions and destination
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

import { onPage as placedOnPage, viewRect } from "./comments";
import { catalog } from "./i18n";
import { isNavigable, reasonFor, type Target } from "./outline";
import { centreOfCharacter, type IndexRange } from "./text";

type Point = [number, number];

/** Clickable regions and their destination in file display coordinates. */
export interface Link {
  id: number;
  page: number;
  /** `[left, top, right, bottom]` in points from the displayed page's top-left. */
  rect: [number, number, number, number];
  /** Perimeter-ordered link polygons when the file supplies QuadPoints. */
  quads?: Point[][];
  target: Target;
}

/** What the scan cut, mirroring `links.rs`'s `Limits`. */
export interface LinkLimits {
  crowded_pages: number;
  over_budget: boolean;
  unreadable: number;
  unresolved_names: number;
  pages_missed: number;
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

/** Clip a convex link polygon to the visible rectangle in the same space. */
function clippedPolygon(points: Point[], rect: Link["rect"]): Point[] {
  for (const edge of [0, 1, 2, 3] as const) {
    const axis = edge % 2 as 0 | 1;
    const boundary = rect[edge];
    const inside = (p: Point) => edge < 2 ? p[axis] >= boundary : p[axis] <= boundary;
    const next: Point[] = [];
    let previous = points.at(-1);
    if (!previous) return [];
    for (const point of points) {
      if (inside(previous) !== inside(point)) {
        const t = (boundary - previous[axis]) / (point[axis] - previous[axis]);
        next.push([previous[0] + t * (point[0] - previous[0]), previous[1] + t * (point[1] - previous[1])]);
      }
      if (inside(point)) next.push(point);
      previous = point;
    }
    points = next;
  }
  const area = points.reduce((sum, p, i) => {
    const q = points[(i + 1) % points.length]!;
    return sum + p[0] * q[1] - p[1] * q[0];
  }, 0);
  return area > 0 ? points : [];
}

/** Intersect file-space link geometry with a crop, keeping it in file space. */
export function clippedLink(link: Link, box: Link["rect"]): Link | null {
  const r = link.rect;
  const rect: Link["rect"] = [Math.max(box[0], r[0]), Math.max(box[1], r[1]),
    Math.min(box[2], r[2]), Math.min(box[3], r[3])];
  if (rect[0] >= rect[2] || rect[1] >= rect[3]) return null;
  if (!link.quads?.length) return { ...link, rect };
  const quads = link.quads.map(q => clippedPolygon(q, rect)).filter(q => q.length);
  if (!quads.length) return null;
  const corners = quads.flat();
  const xs = corners.map(p => p[0]), ys = corners.map(p => p[1]);
  return { ...link, quads, rect: [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)] };
}

/** Translate every hit region to the cropped page's origin. */
export function offsetLink(link: Link, x: number, y: number): Link {
  const [l, t, r, b] = link.rect;
  return { ...link, rect: [l - x, t - y, r - x, b - y],
    ...(link.quads && { quads: link.quads.map(q => q.map(([px, py]): Point => [px - x, py - y])) }) };
}

/** Distance from the pointer to each clockwise edge in displayed coordinates. */
function containsPoint(points: Point[], x: number, y: number, slack: number): boolean {
  return points.every(([px, py], i) => {
    const [qx, qy] = points[(i + 1) % points.length]!;
    const dx = qx - px, dy = qy - py;
    return dx * (y - py) - dy * (x - px) >= -slack * Math.hypot(dx, dy);
  });
}

/** The link under a point on `page`, in page points, or `null`. */
export function linkAt(
  items: readonly Link[],
  page: number,
  x: number,
  y: number,
  slack = LINK_SLACK_PT,
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
      x < left - slack ||
      x > right + slack ||
      y < top - slack ||
      y > bottom + slack
    ) {
      continue;
    }
    if (item.quads?.length && !item.quads.some(q => containsPoint(q, x, y, slack))) continue;
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
  if (turns % 4 === 0) return [...items];
  return items.map(link => {
    const r = viewRect(link.rect, turns, width, height);
    const rotate = ([x, y]: Point): Point => {
      const point = viewRect([x, y, x, y], turns, width, height);
      return [point.left, point.top];
    };
    return { ...link, rect: [r.left, r.top, r.right, r.bottom],
      ...(link.quads && { quads: link.quads.map(q => q.map(rotate)) }) };
  });
}

/**
 * How much two rectangles must overlap vertically to count as one line.
 *
 * A fraction of the shorter one's height rather than an absolute distance,
 * because a footnote marker and the sentence it sits in differ in height by more
 * than any constant that also works on a heading.
 */
export const SAME_LINE_OVERLAP = 0.5;

/**
 * Every link in the order a reader meets them: page, then line, then across.
 *
 * **Ordered on the rectangles as `links.rs` returns them**, which is the page's
 * own display space *before* the view's rotation. That is deliberate and it is
 * the difference between "next link" meaning the next one in the document and
 * the next one down the screen: a reader who has turned the view a quarter still
 * expects the cross-references to come in the order the document has them, and
 * ordering the turned rectangles would reverse them at two of the four turns.
 *
 * **The line banding is this file's, not `reading.ts`'s, and that is a real
 * choice rather than an oversight.** That module groups *glyph* boxes and takes
 * a `PageText` to do it; reusing it would mean synthesising a fake page of text
 * whose characters are annotation rectangles, which is a worse coupling than a
 * dozen lines of overlap arithmetic. What is shared is the definition of a line
 * --- vertical overlap of more than half the shorter box --- and the tests here
 * pin exactly that, so a divergence is visible rather than silent.
 */
export function orderedLinks(items: readonly Link[]): Link[] {
  // Pairwise overlap is not transitive. Establish bands top-to-bottom before
  // ordering links across each band, keeping its first rectangle as the anchor.
  const across = (a: Link, b: Link) => a.rect[0] - b.rect[0] || a.id - b.id;
  const byPosition = [...items].sort((a, b) => a.page - b.page || a.rect[1] - b.rect[1] || across(a, b));
  const ordered: Link[] = [];
  let start = 0;
  while (start < byPosition.length) {
    const anchor = byPosition[start]!;
    let end = start + 1;
    while (end < byPosition.length && byPosition[end]!.page === anchor.page &&
           sameLine(anchor, byPosition[end]!)) end++;
    const band = byPosition.slice(start, end).sort(across);
    for (const link of band) ordered.push(link);
    start = end;
  }
  return ordered;
}

/** Whether two links sit on one line of the page. */
function sameLine(a: Link, b: Link): boolean {
  const top = Math.max(a.rect[1], b.rect[1]);
  const bottom = Math.min(a.rect[3], b.rect[3]);
  const overlap = bottom - top;
  if (overlap <= 0) return false;
  const shorter = Math.min(a.rect[3] - a.rect[1], b.rect[3] - b.rect[1]);
  return shorter > 0 && overlap >= shorter * SAME_LINE_OVERLAP;
}

/** The least a thing needs to be walked through: an identity and a place. */
export interface Walkable {
  id: number;
  page: number;
  rect: readonly [number, number, number, number];
}

/**
 * The next thing in `direction`, from a focused one or from where the reader is.
 *
 * Two starting points because they answer different questions and a reader uses
 * both without thinking about it. With one already focused, "next" means the
 * one after *it* --- so repeated presses walk the page. With none, it means the
 * first one after *the viewport*, so pressing it once after scrolling starts
 * where the reader is looking rather than back at the top of the document.
 *
 * **It does not wrap.** On a 775-page document arriving back at page 1 is a
 * surprise rather than a convenience, and this repository has a trap recording
 * that a wrap is correct when there is nothing ahead, which makes the check
 * unable to fire. `null` at either end is the caller's cue to say so.
 *
 * Generic over {@link Walkable} since 2026-08-18, when the reader's own marks
 * became walkable too. One implementation rather than two, for the reason
 * `comments.ts` gives about `hitTest`: a second copy of a rule is a second thing
 * to keep right, and a mutation of one of them survives the other's tests. What
 * the two callers do *not* share is the list --- `orderedLinks` bands lines,
 * `markWalk` resolves page ids to slots --- and that is the half that genuinely
 * differs.
 */
export function stepAlong<T extends Walkable>(
  ordered: readonly T[],
  from: T | null,
  at: Place,
  direction: 1 | -1,
): T | null {
  if (ordered.length === 0) return null;

  if (from) {
    const index = ordered.findIndex((item) => item.id === from.id);
    if (index >= 0) return ordered[index + direction] ?? null;
    // A focused link that is not in the list --- a new document, or a scan that
    // replaced it. Falling through to the position is right: the id is stale,
    // and the reader's viewport is not.
  }

  if (direction === 1) {
    return ordered.find((item) => startsAtOrAfter(item, at)) ?? null;
  }
  // A link level with the viewport is a forward candidate only.
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const item = ordered[index];
    if (item && isBefore(item, at)) return item;
  }
  return null;
}

/** Whether something starts at or after a place in the document. */
function startsAtOrAfter(item: Walkable, at: Place): boolean {
  if (item.page !== at.page) return item.page > at.page;
  return item.rect[1] >= at.top;
}

/** Whether something starts before a place in the document. */
function isBefore(item: Walkable, at: Place): boolean {
  if (item.page !== at.page) return item.page < at.page;
  return item.rect[1] < at.top;
}

/**
 * Height of a band in the index that makes character-to-link lookup cheap.
 *
 * Points, and roughly a line of body text. The index exists because the obvious
 * loop is characters times links: a page carrying the per-page maximum of 4,000
 * links and 3,000 characters is 12 million rectangle tests, which is a visible
 * hitch when that page scrolls into view. Bucketing by vertical band makes it
 * characters plus links, and the band size only decides how many candidates a
 * lookup considers --- never the answer, which is why it can be a round number
 * chosen by eye rather than a measured one.
 */
const BAND_PT = 12;

/** A stretch of characters that belong to one link, or to none. */
export interface LinkRun {
  /** Character index ranges, in the order they were given. */
  ranges: IndexRange[];
  /** The link these characters are inside, or `null` for ordinary text. */
  link: Link | null;
}

/**
 * Splits character ranges into runs of "inside this link" and "ordinary text".
 *
 * This is what lets a screen reader be told that a run of words is a
 * cross-reference. `a11y.ts` builds the page's text from reading-order ranges;
 * this says which of those characters a link covers, so the text can be handed
 * over as a `role="link"` element rather than as prose.
 *
 * **Both sides are already in the page's displayed space**, which is the reason
 * no rotation appears here: `text.rs` turns character boxes through `to_device`
 * before they leave Rust, and `links.rs` turns annotation rectangles through the
 * same function. A page carrying `/Rotate 90` therefore needs no special case,
 * and adding one would be the second implementation of a turn this repository
 * has a trap about.
 *
 * **A character belongs to a link when its box's centre is inside the
 * rectangle**, not when the boxes overlap. Overlap makes a link claim the
 * characters on either side of it --- annotation rectangles are drawn generously
 * around their text and routinely touch the words next door --- and the failure
 * that produces is a screen reader announcing a link whose name has a stray word
 * at each end.
 */
export function linkRunsIn(
  ranges: readonly IndexRange[],
  boxes: readonly number[],
  links: readonly Link[],
): LinkRun[] {
  const index = bandIndex(links);
  const runs: LinkRun[] = [];

  for (const range of ranges) {
    for (let at = Math.max(0, range.from); at < range.to; at += 1) {
      const found = linkOfCharacter(index, boxes, at);
      const last = runs.at(-1);
      // Extended rather than appended when the link is the same *object*: two
      // adjacent characters inside one link are one run, and two inside
      // different links that happen to point at the same page are not.
      if (last && last.link === found) {
        const tail = last.ranges.at(-1);
        if (tail && tail.to === at) tail.to = at + 1;
        else last.ranges.push({ from: at, to: at + 1 });
      } else {
        runs.push({ ranges: [{ from: at, to: at + 1 }], link: found });
      }
    }
  }
  return runs;
}

/** Links bucketed by the vertical bands their rectangles cover. */
function bandIndex(links: readonly Link[]): Map<number, Link[]> {
  const index = new Map<number, Link[]>();
  for (const link of links) {
    const [, top, , bottom] = link.rect;
    if (!(bottom > top)) continue;
    const first = Math.floor(top / BAND_PT);
    const last = Math.floor(bottom / BAND_PT);
    for (let band = first; band <= last; band += 1) {
      const bucket = index.get(band);
      if (bucket) bucket.push(link);
      else index.set(band, [link]);
    }
  }
  return index;
}

/** The link containing character `at`, or `null`. */
function linkOfCharacter(
  index: Map<number, Link[]>,
  boxes: readonly number[],
  at: number,
): Link | null {
  const centre = centreOfCharacter(boxes, at);
  if (!centre) return null;

  const candidates = index.get(Math.floor(centre.y / BAND_PT));
  const first = candidates?.[0];
  return candidates && first ? linkAt(candidates, first.page, centre.x, centre.y, 0) : null;
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
  return reason ? TEXT().thisLink(reason) : null;
}

const TEXT = catalog({
  "en-US": {
    thisLink: (reason: string) => `This link ${reason}.`,
    overBudget: "too many links to list them all",
    crowded: (count: number) =>
      count === 1 ? "one page had more links than tpdf lists" : `${count} pages had more links than tpdf lists`,
    unreadable: (count: number) =>
      count === 1 ? "one annotation could not be read" : `${count} annotations could not be read`,
    unresolved: (count: number) =>
      count === 1
        ? "one named destination could not be resolved"
        : `${count} named destinations could not be resolved`,
    pagesMissed: (count: number) =>
      count === 1 ? "one page could not be read at all" : `${count} pages could not be read at all`,
    missing: (parts: string) => `Some links are missing: ${parts}.`,
    goToPage: (page: number) => `Go to page ${page}`,
    open: (address: string) => `Open ${address}`,
    broken: "This link goes to a page the document does not have",
    refused: (action: string) => `tpdf does not follow this link: ${action}`,
  },
  "de-DE": {
    thisLink: (reason: string) => `Dieser Link ${reason}.`,
    overBudget: "zu viele Links, um alle aufzulisten",
    crowded: (count: number) =>
      count === 1
        ? "eine Seite hatte mehr Links, als tpdf auflistet"
        : `${count} Seiten hatten mehr Links, als tpdf auflistet`,
    unreadable: (count: number) =>
      count === 1
        ? "eine Anmerkung konnte nicht gelesen werden"
        : `${count} Anmerkungen konnten nicht gelesen werden`,
    unresolved: (count: number) =>
      count === 1
        ? "ein benanntes Ziel konnte nicht aufgelöst werden"
        : `${count} benannte Ziele konnten nicht aufgelöst werden`,
    pagesMissed: (count: number) =>
      count === 1
        ? "eine Seite konnte gar nicht gelesen werden"
        : `${count} Seiten konnten gar nicht gelesen werden`,
    missing: (parts: string) => `Einige Links fehlen: ${parts}.`,
    goToPage: (page: number) => `Gehe zu Seite ${page}`,
    open: (address: string) => `${address} öffnen`,
    broken: "Dieser Link verweist auf eine Seite, die das Dokument nicht hat",
    refused: (action: string) => `tpdf folgt diesem Link nicht: ${action}`,
  },
});

/**
 * What the panel says when the scan cut something, or `null` when it did not.
 *
 * Every bound `links.rs` applies is named, for the reason that module states: a
 * list shown as complete when it is not is the failure the whole arrangement is
 * built to avoid. Here it is not hypothetical --- the 7,694-link document
 * measured in that module's header is over the per-document budget by itself.
 */
export function noticeFor(limits: LinkLimits): string | null {
  const text = TEXT();
  const parts: string[] = [];
  if (limits.over_budget) parts.push(text.overBudget);
  if (limits.crowded_pages > 0) parts.push(text.crowded(limits.crowded_pages));
  if (limits.unreadable > 0) parts.push(text.unreadable(limits.unreadable));
  if (limits.unresolved_names > 0) parts.push(text.unresolved(limits.unresolved_names));
  // First in the sentence would be better and is not worth the reordering: this
  // is the one entry that means "the scan could not look", where every other
  // means "it looked and cut something". Both belong in the notice.
  if (limits.pages_missed > 0) parts.push(text.pagesMissed(limits.pages_missed));
  if (parts.length === 0) return null;
  return text.missing(parts.join("; "));
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

/**
 * What a link goes to, in the words its tooltip shows when the pointer is
 * over it: the page for a link inside the document, the address for a web
 * link. The host is the backend's, in punycode, as the confirmation before
 * opening shows it; this side never builds one.
 */
export function linkTitle(target: Target): string {
  switch (target.kind) {
    case "page": return TEXT().goToPage(target.page + 1);
    case "web": return TEXT().open(`${target.host}${target.rest}`);
    case "broken": return TEXT().broken;
    case "refused": return TEXT().refused(target.action);
    case "none": return "";
  }
}
