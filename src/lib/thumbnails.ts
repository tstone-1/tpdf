/**
 * The page strip: one small picture of every page, in the sidebar.
 *
 * ## It is background work, and the renderer is one thread
 *
 * A thumbnail is a render call, and `docs/PLAN.md` §4 measured what a render
 * call costs on a page that is hard to draw: **1.52 s for a 150 px thumbnail of
 * the A0 sheet**, because Pdfium pays about a second of fixed cost per call
 * before any area-proportional work. The render service is a single FIFO thread
 * (concurrent Pdfium is undefined behaviour), so a strip that asked for all of a
 * document's thumbnails would put seconds of work in front of every tile the
 * reader is waiting for. Scrolling would stall, which is the one thing this
 * project says it will not do.
 *
 * Two rules follow, and together they are the whole design:
 *
 *  - **At most one thumbnail is ever outstanding.** Queueing more buys nothing
 *    --- the renderer is serial, so a queue of ten is ten times the delay and
 *    the same throughput --- and it costs the ability to get out of the way,
 *    because only the request that has not been sent yet is free to cancel.
 *  - **It yields.** The viewer reports how much work it has outstanding; while
 *    that is above zero the strip asks for nothing, and withdraws whatever it
 *    already asked for. A withdrawal reaches Pdfium's progressive API and
 *    returns in 0.25--24 ms against a render that would otherwise have run for
 *    a second and a half.
 *
 * The withdrawn page is simply not drawn yet and is asked for again when the
 * viewer settles. A thumbnail that never arrives is a worse strip; a thumbnail
 * that arrives while the page is stuttering is a worse *viewer*, and the viewer
 * is what someone is actually looking at.
 *
 * ## It reads the viewer's tier-1 cache, and does not write to it
 *
 * §4 says the tier-1 placeholder "doubles as the thumbnail", and it does --- the
 * same 150 px render, at the same scale. So the strip asks the viewer first and
 * draws for free any page the viewer has already prepared, which is why opening
 * the strip shows the page being read immediately even on the A0 sheet.
 *
 * The other direction is deliberately not wired. The scroller keeps tier 1 for
 * the whole session, so donating every thumbnail into it would grow it to one
 * bitmap per page --- 98 MB on the 775-page corpus, for pages nobody has looked
 * at. The strip keeps its own bounded set instead and lets the viewer render its
 * own placeholders, which on any document where that is expensive is a document
 * with few enough pages for the duplication not to matter.
 *
 * ## Only the visible rows exist
 *
 * Unlike the outline --- which is bounded at 10,000 entries and can afford a real
 * element per row --- a page strip has as many rows as the document has pages,
 * and nothing bounds that. So rows are built for the visible window and a little
 * either side, and destroyed when they leave. That makes `aria-setsize` and
 * `aria-posinset` load-bearing rather than decorative: without them a screen
 * reader is told this is a list of eleven things when it is a list of 775.
 */

import { Lifetime } from "./lifetime";
import { displayedSize, TIER1_WIDTH, type PageSize } from "./scroller";
import { cancelTile, fetchTile, nextRequestId } from "./tiles";

/** Width of a thumbnail as drawn, in CSS pixels. */
const THUMB_WIDTH = 116;

/** Space around a thumbnail and its number, in CSS pixels. */
const ROW_PADDING = 10;

/** Height of the page number under each thumbnail, in CSS pixels. */
const LABEL_HEIGHT = 16;

/**
 * Rows built beyond the visible window, on each side.
 *
 * One screenful of scrolling is the movement a reader makes most often, and a
 * row that is already mounted only has to be positioned rather than built and
 * re-drawn from its bitmap.
 */
export const OVERSCAN = 3;

/**
 * Thumbnails kept in memory.
 *
 * At 150 px wide a thumbnail is about 127 KB, so this is roughly 30 MB. The cap
 * exists because the alternative is one bitmap per page: on the 775-page corpus
 * that is 98 MB held for pages nobody is looking at, which is exactly the cost
 * this class refuses to push into the viewer's tier-1 cache and so must not
 * quietly take on itself.
 */
const MAX_KEPT = 240;

/**
 * How far a pointer must travel before a press becomes a drag, in CSS pixels.
 *
 * A press on a row already navigates to that page, and it must keep doing so
 * --- so the threshold is what separates "the reader clicked a thumbnail" from
 * "the reader is rearranging the document". Too small and an unsteady click
 * reorders the document; too large and a short drag does nothing, which reads
 * as the strip being broken rather than as the reader having missed.
 */
const DRAG_THRESHOLD = 6;

/**
 * How near the panel's edge the pointer must be for the strip to scroll under
 * it, in CSS pixels, and how far it scrolls per frame.
 *
 * The strip is virtual and a row is roughly 200 px tall, so a sidebar shows
 * three or four of them. Without this, a drag could only reach the rows already
 * on screen --- which is a smaller move than the two palette commands already
 * make, and would not be worth having.
 */
const EDGE_ZONE = 48;
const EDGE_SPEED = 14;

/** What the strip reads out of the viewer's tier-1 cache. */
export interface Tier1Access {
  /** A page's placeholder bitmap, or `null` if it has not been rendered. */
  placeholderFor(page: number): ImageBitmap | null;
}

/**
 * Re-exported rather than declared again: the strip's rows have to be the shape
 * the scroller lays a page out in, and two structurally identical interfaces are
 * two places for that to stop being true silently.
 */
export type { PageSize };

export interface ThumbnailOptions {
  doc: number;
  pageCount: number;
  /**
   * Which page of the file a row draws, or `undefined` for a row that is not in
   * the document.
   *
   * A row is a slot, and a tile request names a page of the file --- the two
   * stopped being the same number when a page could be deleted. Optional, and
   * defaulting to the identity, because the strip is also driven by harnesses
   * that never edit; see `pages.ts`.
   */
  sourceOf?: (slot: number) => number | undefined;
  /** Geometry of page 1, taken as representative --- as `scroller.ts` does. */
  page: PageSize;
  tier1: Tier1Access;
  /** Called when a row is activated, with a zero-based page index. */
  onNavigate: (page: number) => void;
  /**
   * Called when a row is dragged to a new place, with two slot indices.
   *
   * `to` is where the page ends up in the order the drop produces, which is
   * what `Edits.move` takes --- not the gap the pointer was over. The two
   * differ for every drag towards the back of the document, and
   * {@link landingSlot} is the one place that conversion is done.
   *
   * Optional, because the strip is also driven by harnesses and by a document
   * nobody can edit, and a strip with no handler simply does not drag.
   */
  onReorder?: (from: number, to: number) => void;
  /**
   * Called when a row is right-clicked, with its slot and where the pointer was.
   *
   * The strip suppresses the web view's own menu whenever this is present, and
   * only then --- a handler that is absent leaves the platform's behaviour
   * exactly as it was, which is what the harnesses that drive this class
   * without one need.
   *
   * Optional for the same reason {@link onReorder} is: the strip is also driven
   * by a document nobody can edit, where a menu of page operations would be a
   * list of things that cannot happen.
   */
  onContextMenu?: (slot: number, at: { x: number; y: number }) => void;
}

/**
 * The gap between rows a pointer at `contentY` is nearest to.
 *
 * A gap rather than a row, and they are not the same thing: there are
 * `pageCount + 1` places a page can be dropped into and `pageCount` rows, so a
 * function answering "which row is under the pointer" would have no way to say
 * *after the last one*. Gap `g` means "before the page currently in slot `g`".
 *
 * `contentY` is measured from the top of the whole strip rather than from the
 * top of the panel, so it is unaffected by scrolling --- which is what lets a
 * drag that scrolls the strip under it stay pointed at the same gap.
 */
export function insertionGap(
  contentY: number,
  rowHeight: number,
  pageCount: number,
): number {
  if (rowHeight <= 0) return 0;
  const gap = Math.round(contentY / rowHeight);
  return Math.max(0, Math.min(gap, pageCount));
}

/**
 * Where a page dragged from slot `from` ends up if it is dropped into `gap`.
 *
 * **The one piece of arithmetic in this file, and it is off by one in exactly
 * half the cases.** The gap is read against the order the page is *still* in;
 * the answer is an index into the order it will be in once the page has left.
 * Dropping into a gap above the page changes nothing about the slots above it,
 * so the two agree. Dropping into a gap below it removes one slot from
 * everything in between, so the landing is one lower than the gap.
 *
 * The two gaps either side of the page itself both mean "leave it alone", and
 * both come back as `from` --- gap `from` because it is already there, and gap
 * `from + 1` because it becomes `from` here. That is the property that makes a
 * drag that goes nowhere a no-op rather than a refusal from the model.
 */
export function landingSlot(from: number, gap: number): number {
  return gap > from ? gap - 1 : gap;
}

/** The rows a strip shows at a given scroll offset, plus the overscan. */
export function stripWindow(
  scrollTop: number,
  height: number,
  rowHeight: number,
  pageCount: number,
  overscan: number,
): { first: number; last: number } {
  if (pageCount <= 0 || rowHeight <= 0) return { first: 0, last: -1 };
  const clamp = (page: number): number =>
    Math.max(0, Math.min(page, pageCount - 1));
  const first = clamp(Math.floor(scrollTop / rowHeight) - overscan);
  // `height` can be zero before the panel has been laid out, which must still
  // yield the one row at `first` rather than an empty window --- otherwise the
  // strip renders nothing until something happens to resize it.
  const last = clamp(Math.ceil((scrollTop + height) / rowHeight) - 1 + overscan);
  return { first, last: Math.max(first, last) };
}

/**
 * The next page worth rendering, nearest to `centre` first, or `null`.
 *
 * Nearest-first rather than in page order because the strip scrolls to follow
 * the reader: page order would spend the A0 sheet's 1.5 s per page rendering
 * from the top of a document whose reader is at page 400, and every one of those
 * thumbnails would be evicted before it was looked at.
 */
export function nextWanted(
  window: { first: number; last: number },
  centre: number,
  have: (page: number) => boolean,
): number | null {
  const from = Math.max(window.first, Math.min(centre, window.last));
  for (let step = 0; step <= window.last - window.first; step++) {
    const below = from - step;
    if (below >= window.first && !have(below)) return below;
    const above = from + step;
    if (step > 0 && above <= window.last && !have(above)) return above;
  }
  return null;
}

/**
 * Height of one row, thumbnail plus its number plus padding.
 *
 * Sized from the page as *displayed* --- `scroller.ts`'s `displayedSize`, the
 * same one the viewer lays out with --- because the strip follows the view's
 * rotation: its thumbnails are the viewer's tier-1 placeholders, borrowed rather
 * than re-rendered, and those are produced in whatever orientation the view is
 * in. Rows sized from the file would leave a gap under every one of them while
 * the borrowed bitmap overflowed.
 *
 * **One page's size for the whole strip, and on a mixed-size document that is
 * wrong.** Stated rather than left to be discovered: the viewer stopped assuming
 * a uniform document on 2026-08-02 --- `scroller.ts` holds a size per page now
 * --- and this did not. Every row is page 1's aspect, and the render request is
 * `TIER1_WIDTH` wide at `TIER1_WIDTH / page1.width_pt`, so a page wider than
 * page 1 is asked for only as far as page 1 reaches and its thumbnail is
 * cropped. It is a smaller fault than the viewer's was, because a thumbnail is
 * an index rather than the content, and the row it sits in is a fixed box that a
 * reader reads as one --- but it is a fault, not a case that happens to be safe.
 *
 * Fixing it is a separate piece of work rather than an oversight in this one. It
 * needs a size per page here too, and the strip has no channel to learn one: the
 * viewer's comes from the text extraction it already performs for every visible
 * page, and the strip extracts no text. What it would take is either that lookup
 * threaded through from the viewer, or rows sized from the bitmap that arrives
 * --- and the second changes the virtualised list's arithmetic, which is
 * currently one row height times an index.
 */
export function rowHeightFor(page: PageSize, turns = 0): number {
  const shown = displayedSize(page, turns);
  const scale = THUMB_WIDTH / Math.max(1, shown.width_pt);
  return Math.round(shown.height_pt * scale) + LABEL_HEIGHT + ROW_PADDING * 2;
}

interface Outstanding {
  page: number;
  rid: number;
  /** Set when the request has been withdrawn, so it is withdrawn only once. */
  withdrawn: boolean;
}

/**
 * Why an outstanding request is being given up.
 *
 * Two different events, and only one of them is contention. A `yield` is the
 * renderer being wanted elsewhere --- the viewer has work, or the panel is
 * hidden and so has no claim on the one thread --- and that is what
 * {@link Thumbnails.yieldCount} reports and what `viewercheck.ts` reads as the
 * strip getting out of the way in time. A `discard` is the *picture* no longer
 * being the one wanted: a rotation, an inversion, a teardown. Counting the two
 * together made the contention metric say less the more the reader rotated.
 */
type Withdrawal = "yield" | "discard";

export class Thumbnails {
  private readonly opts: ThumbnailOptions;
  private readonly host: HTMLElement;
  private readonly list: HTMLElement;
  private readonly spacer: HTMLElement;

  private rowHeight: number;
  private thumbHeight: number;
  /** Quarter-turns clockwise the view is rotated by. */
  private turns = 0;

  /** Whether the page's lightness is inverted, for reading in the dark. */
  private invert = false;
  private readonly observer: ResizeObserver;

  /** Rendered pages, in least-recently-drawn order. See {@link MAX_KEPT}. */
  private readonly bitmaps = new Map<number, ImageBitmap>();
  /** Row elements currently mounted, by page. */
  private readonly rows = new Map<number, HTMLElement>();

  private request: Outstanding | null = null;
  /**
   * Pages whose bitmap is being copied out of the viewer's tier 1.
   *
   * A borrow finishes in a microtask rather than immediately, and `pump` is
   * called from a scroll handler, a resize and every position change --- so
   * without this the same page is borrowed again on each of them, because it is
   * not in `bitmaps` yet and no request is outstanding. It showed up as twelve
   * borrows on a twelve-page document that had only a handful of rows on screen.
   */
  private readonly borrowing = new Set<number>();
  /**
   * Pages whose render came back an error.
   *
   * Not retried for the life of the strip's current orientation. `pump` is
   * called from a scroll handler, a resize, a tab change and every request
   * settling, so a page that fails every time would otherwise be re-rendered on
   * each of those --- less pathological than the frame-driven loop `scroller.ts`
   * had, and the same shape. Cleared by `setTurns` and `setInvert`, which are
   * the reader asking for a different picture and so are owed a fresh attempt.
   */
  private readonly failed = new Set<number>();
  /**
   * Whether this strip is still alive, for its two arrivals to consult.
   *
   * Deliberately **not** {@link active}, which the arrivals could be read as
   * already covering. That flag means "the strip is the visible tab" --- it goes
   * false when the reader switches to the outline and true again when they
   * switch back, so a continuation testing it would refuse a bitmap the strip
   * still wants. Hidden and destroyed are different facts, and `keep` after
   * teardown leaks into a map that has already been cleared. See `lifetime.ts`.
   */
  private readonly life = new Lifetime();
  /** Whether the strip is the visible tab. Nothing is rendered when it is not. */
  private active = false;
  /** Whether the viewer has work outstanding. See the class docs. */
  private busy = false;
  /** The page the reader is on, which is what rendering works outwards from. */
  private current = 0;
  /** Row holding the roving tabindex. */
  private focused = 0;
  /**
   * Which picture the strip is currently asking for: bumped by every change of
   * orientation or polarity.
   *
   * A withdrawal only reaches a request that is still queued or running. One
   * that had already finished comes back a full result, and keeping it leaves
   * a single thumbnail in the previous orientation for the life of the
   * document --- `have()` reads a kept bitmap as "this page is drawn", so it is
   * never asked for again. This is the render path's copy of the guard the
   * borrow path gets from `borrowing` and the scroller gets from its
   * placeholder generation; it was the one of the three without one.
   */
  private generation = 0;

  /**
   * Requests withdrawn to get out of the viewer's way, and only those --- see
   * {@link Withdrawal}. For the check harness.
   */
  private yielded = 0;
  /** Thumbnails taken from the viewer's tier 1 rather than rendered again. */
  private borrowed = 0;

  /**
   * The press that may become a drag, and the drag it became.
   *
   * One object for both, with `dragging` saying which, rather than two fields
   * that can disagree. `gap` is kept because the drop reads it: the pointer can
   * be released outside the panel entirely, and a drop that recomputed the gap
   * from wherever the pointer finally was would move the page somewhere the
   * indicator never showed.
   */
  private press: {
    from: number;
    pointerId: number;
    startY: number;
    dragging: boolean;
    gap: number;
    /** Where the pointer is now, in client coordinates, for the edge scroll. */
    clientY: number;
  } | null = null;

  /** The line drawn where the page would land, mounted only while dragging. */
  private indicator: HTMLElement | null = null;

  /** The edge-scroll frame, so a drop cancels one already queued. */
  private edgeFrame = 0;

  /** Drops completed, for the check harness. */
  private dropped = 0;

  /**
   * The gap the last drag was released over, for the check harness.
   *
   * Kept past the end of the drag because a drop that decides to do nothing is
   * exactly the case worth diagnosing, and by then the press is gone. `-1`
   * means no drag has ended yet.
   */
  private lastGap = -1;

  constructor(root: HTMLElement, opts: ThumbnailOptions) {
    this.opts = opts;
    this.rowHeight = rowHeightFor(opts.page);
    this.thumbHeight = this.rowHeight - LABEL_HEIGHT - ROW_PADDING * 2;

    this.host = document.createElement("div");
    this.host.style.cssText =
      "flex:1;min-height:0;overflow:auto;position:relative;";
    this.host.addEventListener("scroll", () => this.layout());

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.setAttribute("aria-label", "Pages");
    this.list.style.cssText = "position:relative;outline:none;";
    this.list.addEventListener("keydown", this.onKeyDown);
    // Same reason as the outline tree: focus can arrive without going through
    // `focus(page)`, and a roving tabindex that does not follow it aims every
    // arrow key at whichever row happened to be tracked.
    this.list.addEventListener("focusin", (event) => {
      const page = (event.target as HTMLElement | null)?.dataset?.page;
      if (page !== undefined) this.focus(Number(page));
    });

    // On the list rather than on each row, so a right-click in the gaps between
    // rows is caught too --- and `preventDefault` regardless of whether a row
    // was hit, because the web view's own menu is not something to fall back
    // to: its one entry reloads the frontend and throws away the reader's view
    // of the document.
    if (opts.onContextMenu) {
      const offer = opts.onContextMenu;
      this.list.addEventListener("contextmenu", (event: MouseEvent) => {
        event.preventDefault();
        const page = (event.target as HTMLElement | null)?.closest?.("[data-page]");
        const slot = (page as HTMLElement | null)?.dataset?.page;
        if (slot === undefined) return;
        offer(Number(slot), { x: event.clientX, y: event.clientY });
      });
    }

    this.spacer = document.createElement("div");
    this.spacer.style.cssText = `height:${this.rowHeight * opts.pageCount}px;`;

    this.list.appendChild(this.spacer);
    this.host.appendChild(this.list);
    root.appendChild(this.host);

    // On the host rather than on each row, because a drag routinely leaves the
    // row it started on --- and because a row can be unmounted mid-drag by the
    // windowing, which would take its listeners with it. `setPointerCapture`
    // on the host makes that explicit rather than incidental.
    this.host.addEventListener("pointermove", this.onPointerMove);
    this.host.addEventListener("pointerup", this.onPointerUp);
    this.host.addEventListener("pointercancel", this.onPointerCancel);

    // The window depends on the panel's height, which is zero until the layout
    // has reached it and changes again whenever the window does.
    this.observer = new ResizeObserver(() => this.layout());
    this.observer.observe(this.host);
    this.layout();
  }

  destroy(): void {
    // First, for the reason `scroller.ts` gives: a render landing mid-teardown
    // must find a strip that is dead, not one that is partly dismantled.
    this.life.end();
    // Abandoned rather than dropped: a strip being torn down must not run one
    // last edit on its way out, and the frame the edge scroll has queued would
    // otherwise reach a host that has been removed from the document.
    this.endDrag(false);
    this.observer.disconnect();
    // Before the withdrawal, and load-bearing: `pump` refuses to issue anything
    // while this is false, and a strip torn down with it still true keeps
    // pumping. The settling request's `.then` calls `pump`, which starts the
    // next page, whose reply starts the one after --- one render per reply until
    // the window's rows run out, which is a screenful plus twice the overscan,
    // all of it for a document nobody is looking at any more and all of it in
    // front of the tiles for the one they are.
    this.active = false;
    this.withdraw("discard");
    for (const bitmap of this.bitmaps.values()) bitmap.close();
    this.bitmaps.clear();
    this.rows.clear();
    this.host.remove();
  }

  /** Pages whose thumbnail has been drawn. For the check harness. */
  get rendered(): number[] {
    return [...this.bitmaps.keys()].sort((a, b) => a - b);
  }

  /** Whether a thumbnail render is currently outstanding. */
  get outstanding(): boolean {
    return this.request !== null && !this.request.withdrawn;
  }

  /** Requests withdrawn because the viewer needed the renderer. */
  get yieldCount(): number {
    return this.yielded;
  }

  /** Thumbnails that came from the viewer's tier-1 cache for free. */
  get borrowCount(): number {
    return this.borrowed;
  }

  /** Drags that ended in a move. For the check harness. */
  get dropCount(): number {
    return this.dropped;
  }

  /** The gap the last drag was released over, or -1. For the check harness. */
  get releasedOver(): number {
    return this.lastGap;
  }

  /** The height of one row, in CSS pixels. For the check harness. */
  get rowPitch(): number {
    return this.rowHeight;
  }

  /** The panel's scroll offset and its position on screen. For the harness. */
  get panelAt(): { scrollTop: number; top: number } {
    return {
      scrollTop: this.host.scrollTop,
      top: this.host.getBoundingClientRect().top,
    };
  }

  /**
   * Whether a row is being dragged right now. For the check harness.
   *
   * A press that has not passed the threshold is deliberately *not* a drag
   * here: the distinction is the whole of what the threshold buys, and an
   * observable that blurred it could not tell a click from a rearrangement any
   * better than the code it is watching.
   */
  get dragging(): boolean {
    return this.press?.dragging ?? false;
  }

  /** Rows currently mounted, in page order. For the check harness. */
  get mounted(): number[] {
    return [...this.rows.keys()].sort((a, b) => a - b);
  }

  /** The row element for a page, if it is mounted. */
  elementFor(page: number): HTMLElement | null {
    return this.rows.get(page) ?? null;
  }

  /**
   * Shows or hides the strip.
   *
   * A hidden strip renders nothing at all --- not a reduced rate, nothing. The
   * renderer it would be competing for is the one drawing the page in front of
   * the reader, and a panel nobody can see has no claim on it.
   */
  setActive(active: boolean): void {
    if (this.active === active) return;
    this.active = active;
    if (!active) this.withdraw("yield");
    else {
      this.layout();
      this.scrollTo(this.current);
    }
    this.pump();
  }

  /**
   * Tells the strip whether the viewer has work outstanding.
   *
   * Driven from the viewer's per-frame status, which reports `pending` and
   * therefore changes within a frame of the viewer wanting anything. The
   * withdrawal that follows costs one poll of Pdfium's pause callback --- worst
   * observed 66 ms, typically under 25 --- so the viewer waits tens of
   * milliseconds for a thumbnail rather than the second and a half it would
   * otherwise take.
   */
  setViewerBusy(busy: boolean): void {
    if (this.busy === busy) return;
    this.busy = busy;
    if (busy) this.withdraw("yield");
    else this.pump();
  }

  /**
   * Rotates the strip to match the view, dropping every thumbnail.
   *
   * All of them, because a thumbnail is a rendered bitmap and a rotated page is
   * a different picture --- the same reason the scroller drops tier 1. The rows
   * change shape with it, so they are rebuilt rather than restyled.
   */
  /**
   * Turns page inversion on or off, discarding every thumbnail.
   *
   * Narrower than {@link setTurns} and for a reason worth stating: the rows do
   * not change shape, so they are kept and only their pictures are dropped.
   * Rebuilding them would work and would also scroll the strip back to wherever
   * `scrollTo` put it, which a reader who only changed the colours did not ask
   * for.
   *
   * A borrowed placeholder is dropped with the rest. The viewer re-renders its
   * own tier 1 inverted, so the next borrow gets the right polarity --- and
   * `borrowing` is cleared for the same reason it is in `setTurns`: a copy
   * already in flight lands in the old polarity and there is no way to stop it.
   */
  setInvert(invert: boolean): void {
    if (invert === this.invert) return;
    this.invert = invert;
    this.generation++;
    this.withdraw("discard");

    for (const bitmap of this.bitmaps.values()) bitmap.close();
    this.bitmaps.clear();
    this.borrowing.clear();
    this.failed.clear();
    this.layout();
  }

  setTurns(turns: number): void {
    const next = ((turns % 4) + 4) % 4;
    if (next === this.turns) return;
    this.turns = next;
    this.generation++;
    this.withdraw("discard");

    for (const bitmap of this.bitmaps.values()) bitmap.close();
    this.bitmaps.clear();
    // A borrow in flight would land as a bitmap of the previous orientation and
    // `keep` would believe it. Clearing the set does not stop the copy --- there
    // is no way to --- so `keep` refuses anything not still marked as borrowed.
    this.borrowing.clear();
    this.failed.clear();

    this.rowHeight = rowHeightFor(this.opts.page, next);
    this.thumbHeight = this.rowHeight - LABEL_HEIGHT - ROW_PADDING * 2;
    this.spacer.style.height = `${this.opts.pageCount * this.rowHeight}px`;
    for (const row of this.rows.values()) row.remove();
    this.rows.clear();
    this.layout();
    this.scrollTo(this.current);
  }

  /**
   * Takes the document's page order, throwing away every thumbnail.
   *
   * Not selective, and the reason is the same one `Scroller.setPages` gives: a
   * thumbnail is held under the row it was rendered for, and after a deletion
   * every row below the gap shows a different page. Keeping them would leave the
   * strip captioned "page 4" over a picture of the old page 4, which is the
   * plausible wrong answer rather than an obviously stale one.
   *
   * **It takes a count and discards on every call, which is not the same thing
   * as taking a count and discarding when the count changed.** This was
   * `setPageCount`, and it returned early when the number matched --- correct
   * for a deletion, which always shortens the document, and wrong for a *move*,
   * which never does. The strip would have gone on showing the old order with
   * nothing to say it had not been asked. So the guard is the caller's: it is
   * called when the order changed, and the order changing is what the model's
   * reply answers.
   */
  setPages(pageCount: number): void {
    this.opts.pageCount = pageCount;
    this.generation++;
    // Every row is about to be rebuilt, so a drag still in flight is aimed at
    // slots that are about to mean something else. Abandoned rather than
    // dropped, because the commonest caller *is* the edit a drop just made:
    // completing it here would apply the reader's move a second time.
    this.endDrag(false);
    this.withdraw("discard");

    for (const bitmap of this.bitmaps.values()) bitmap.close();
    this.bitmaps.clear();
    this.borrowing.clear();
    this.failed.clear();

    this.spacer.style.height = `${this.opts.pageCount * this.rowHeight}px`;
    for (const row of this.rows.values()) row.remove();
    this.rows.clear();
    this.current = Math.max(0, Math.min(this.current, pageCount - 1));
    this.layout();
    this.scrollTo(this.current);
  }

  /** The page as displayed, i.e. after the view rotation. */
  private displayed(): PageSize {
    return displayedSize(this.opts.page, this.turns);
  }

  /** Device-pixel height of a thumbnail bitmap at {@link TIER1_WIDTH}. */
  private thumbPixels(): number {
    const shown = this.displayed();
    return Math.round((shown.height_pt * TIER1_WIDTH) / shown.width_pt);
  }

  /** Tells the strip which page the reader is on. */
  setCurrentPage(page: number): void {
    if (page === this.current) return;
    this.current = page;
    this.markCurrent();
    if (this.active) this.scrollTo(page);
    this.pump();
  }

  /** Scrolls a page's row into view, without disturbing the keyboard. */
  private scrollTo(page: number): void {
    // **Never while a pointer is down on a row.** Pressing a row navigates, and
    // navigating comes back here as "show the page being read" --- so without
    // this the strip scrolls to the pressed row at the instant a drag begins,
    // sliding the content out from under a pointer that has not moved. The
    // reader then drops on a gap they never pointed at, and the whole document
    // is rearranged wrongly by a gesture that looked right.
    //
    // Found by the corpus sweep, not by reasoning: four of the fourteen came
    // back with a drop that asked for nothing, and the four that passed did so
    // because their strips were short enough for the scroll to clamp.
    if (this.press) return;
    const row = this.rows.get(page);
    if (row) row.scrollIntoView({ block: "nearest" });
    else this.host.scrollTop = page * this.rowHeight - this.host.clientHeight / 2;
    this.layout();
  }

  /** Withdraws the outstanding request, if there is one. */
  private withdraw(why: Withdrawal): void {
    if (!this.request || this.request.withdrawn) return;
    this.request.withdrawn = true;
    if (why === "yield") this.yielded++;
    cancelTile(this.request.rid);
  }

  /**
   * Issues the next thumbnail, if the renderer is ours to use.
   *
   * Cheap and idempotent: it is called from a scroll handler, from the viewer's
   * status callback and from every request settling, and the common case is
   * that it finds nothing to do and returns.
   */
  private pump(): void {
    if (!this.active || this.busy || this.request) return;

    const window = this.currentWindow();
    const page = nextWanted(
      window,
      this.current,
      (p) => this.bitmaps.has(p) || this.borrowing.has(p) || this.failed.has(p),
    );
    if (page === null) return;

    // The viewer may already have this page as a tier-1 placeholder, in which
    // case there is nothing to render: it is the same 150 px bitmap at the same
    // scale. Copied rather than retained, because the scroller closes its own
    // placeholders when the document is closed.
    const borrowed = this.opts.tier1.placeholderFor(page);
    if (borrowed) {
      this.borrowed++;
      this.borrowing.add(page);
      void createImageBitmap(borrowed)
        .then(
          this.life.claim(
            (copy: ImageBitmap) => {
              // Not `delete` then `keep`: a rotation during the copy clears the
              // set, and that is exactly the signal that this bitmap is the
              // wrong way up.
              if (!this.borrowing.delete(page)) {
                copy.close();
                return;
              }
              this.keep(page, copy);
              this.pump();
            },
            // `destroy` does not clear `borrowing`, so without this the copy
            // passes the test above and is kept in a map that was emptied.
            (copy) => copy.close(),
          ),
        )
        .catch(() => {
          this.borrowing.delete(page);
        });
      return;
    }

    const source = this.opts.sourceOf?.(page) ?? page;
    const rid = nextRequestId();
    const outstanding: Outstanding = { page, rid, withdrawn: false };
    this.request = outstanding;
    // Read before the request goes out, and compared when it comes back. See
    // {@link generation}: a withdrawal cannot reach a render that has already
    // finished, so this is what tells the reply apart from one the reader still
    // wants.
    const generation = this.generation;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page: source,
      scale: TIER1_WIDTH / this.displayed().width_pt,
      turns: this.turns,
      invert: this.invert,
      x: 0,
      y: 0,
      width: TIER1_WIDTH,
      height: this.thumbPixels(),
      format: "raw",
    })
      .then(
        this.life.claim(
          (result) => {
            this.request = null;
            // `null` is the withdrawal landing: the page keeps no thumbnail and
            // is asked for again by the next pump, which is what makes yielding
            // safe to do at any moment rather than only between requests.
            //
            // A *result* after a rotation or an inversion is the other outcome
            // of the same withdrawal --- the render beat it, so there was
            // nothing left to cancel --- and it is a picture of the orientation
            // the reader has just left. Dropped rather than kept, so the next
            // pump asks for the page again in the one they are now in.
            if (result) {
              if (generation === this.generation) this.keep(page, result.bitmap);
              else result.bitmap.close();
            }
            this.pump();
          },
          // Withdrawal races the renderer, so a thumbnail that had already
          // finished still arrives after teardown --- into a cleared map.
          (result) => result?.bitmap.close(),
        ),
      )
      .catch((reason: unknown) => {
        this.request = null;
        // Once per page, which is every failure here --- the page is never
        // retried --- and `tiles.ts` builds an error naming it. Dropping that
        // left a strip with a blank row and nothing anywhere saying why.
        if (!this.failed.has(page)) {
          console.warn(`thumbnail for page ${page + 1} failed: ${String(reason)}`);
        }
        this.failed.add(page);
        // Still pumped: the strip should carry on with the *other* pages rather
        // than stopping at the first one that cannot be drawn.
        this.pump();
      });
  }

  /** Records a thumbnail, drawing it if its row is on screen. */
  private keep(page: number, bitmap: ImageBitmap): void {
    this.bitmaps.get(page)?.close();
    this.bitmaps.set(page, bitmap);
    this.draw(page);
    this.evict();
  }

  /**
   * Drops thumbnails over the cap, oldest first, keeping the visible ones.
   *
   * A visible row whose bitmap was evicted would go blank and immediately be
   * re-rendered, which on the A0 sheet is 1.5 s to redraw something that was
   * already on screen.
   */
  private evict(): void {
    if (this.bitmaps.size <= MAX_KEPT) return;
    const window = this.currentWindow();
    for (const page of [...this.bitmaps.keys()]) {
      if (this.bitmaps.size <= MAX_KEPT) break;
      if (page >= window.first && page <= window.last) continue;
      this.bitmaps.get(page)?.close();
      this.bitmaps.delete(page);
    }
  }

  private currentWindow(): { first: number; last: number } {
    return stripWindow(
      this.host.scrollTop,
      this.host.clientHeight,
      this.rowHeight,
      this.opts.pageCount,
      OVERSCAN,
    );
  }

  /** Builds, drops and positions rows to match the visible window. */
  private layout(): void {
    const { first, last } = this.currentWindow();

    for (const [page, row] of this.rows) {
      if (page < first || page > last) {
        row.remove();
        this.rows.delete(page);
      }
    }
    for (let page = first; page <= last; page++) {
      if (!this.rows.has(page)) this.mount(page);
    }
    this.pump();
  }

  private mount(page: number): void {
    const row = document.createElement("div");
    row.setAttribute("role", "option");
    row.setAttribute("aria-label", `Page ${page + 1}`);
    // The window is a rendering detail; a reader is in a list of every page.
    row.setAttribute("aria-setsize", String(this.opts.pageCount));
    row.setAttribute("aria-posinset", String(page + 1));
    row.setAttribute("aria-selected", String(page === this.current));
    row.tabIndex = page === this.focused ? 0 : -1;
    row.dataset.page = String(page);
    row.style.cssText =
      `position:absolute;left:0;right:0;top:${page * this.rowHeight}px;` +
      `height:${this.rowHeight}px;box-sizing:border-box;` +
      `padding:${ROW_PADDING}px 0;display:flex;flex-direction:column;` +
      "align-items:center;gap:2px;cursor:default;outline-offset:-2px;";

    const canvas = document.createElement("canvas");
    canvas.width = TIER1_WIDTH;
    canvas.height = this.thumbPixels();
    canvas.setAttribute("aria-hidden", "true");
    // The backing store stays 150 px wide whatever the row is drawn at, for the
    // same reason the scroller's placeholders do: it is the bitmap tier 1
    // produces, and re-rendering it at the panel's width would be a second
    // render call of exactly the kind this class exists to ration.
    canvas.style.cssText =
      `width:${THUMB_WIDTH}px;height:${this.thumbHeight}px;` +
      "background:color-mix(in srgb, currentColor 8%, Canvas);" +
      "box-shadow:0 0 0 1px color-mix(in srgb, currentColor 20%, transparent);";

    const label = document.createElement("span");
    label.setAttribute("aria-hidden", "true");
    label.style.cssText = `height:${LABEL_HEIGHT}px;line-height:${LABEL_HEIGHT}px;opacity:0.65;font-variant-numeric:tabular-nums;`;
    label.textContent = String(page + 1);

    row.append(canvas, label);
    row.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.focus(page);
      // Still on the press, and deliberately not deferred to the release now
      // that a press can become a drag. Navigating first means the reader is
      // looking at the page they are about to move, and it keeps a plain click
      // exactly as responsive as it was --- a drag that also navigates is
      // coherent, because the viewer follows a page by identity and ends up on
      // it wherever it lands.
      //
      // **Recorded before the navigation, not after**, which is load-bearing
      // rather than tidy: navigating makes the strip scroll to the page it
      // moved to, and {@link scrollTo} refuses to do that while a press is
      // live. Set afterwards, the scroll happens first and the content the
      // reader is about to aim at has already slid under the pointer.
      if (this.opts.onReorder) {
        this.host.setPointerCapture(event.pointerId);
        this.press = {
          from: page,
          pointerId: event.pointerId,
          startY: event.clientY,
          dragging: false,
          gap: page,
          clientY: event.clientY,
        };
      }
      this.opts.onNavigate(page);
    });

    this.spacer.appendChild(row);
    this.rows.set(page, row);
    this.mark(page, page === this.current);
    this.draw(page);
  }

  /** Paints a page's bitmap into its row, if both exist. */
  private draw(page: number): void {
    const bitmap = this.bitmaps.get(page);
    const canvas = this.rows.get(page)?.querySelector("canvas");
    if (!bitmap || !canvas) return;
    canvas.getContext("2d", { alpha: false })?.drawImage(bitmap, 0, 0);
    // Re-inserting moves it last, which is what makes the map an LRU rather
    // than a queue in arrival order.
    this.bitmaps.delete(page);
    this.bitmaps.set(page, bitmap);
  }

  /**
   * Turns a pointer position into the gap it is over, and shows it.
   *
   * The conversion adds `scrollTop` back, so the answer is in the strip's own
   * coordinates rather than the panel's --- which is what makes it survive the
   * edge scroll moving the content under a stationary pointer.
   */
  private aimAt(clientY: number): void {
    if (!this.press) return;
    const top = this.host.getBoundingClientRect().top;
    const contentY = clientY - top + this.host.scrollTop;
    this.press.gap = insertionGap(contentY, this.rowHeight, this.opts.pageCount);
    this.showIndicator(this.press.gap);
  }

  /** Draws the line where the page would land, creating it on first use. */
  private showIndicator(gap: number): void {
    if (!this.indicator) {
      const line = document.createElement("div");
      line.setAttribute("aria-hidden", "true");
      line.style.cssText =
        "position:absolute;left:6px;right:6px;height:2px;border-radius:1px;" +
        "background:currentColor;pointer-events:none;";
      this.spacer.appendChild(line);
      this.indicator = line;
    }
    this.indicator.style.top = `${Math.max(0, gap * this.rowHeight - 1)}px`;
  }

  /**
   * Ends the drag, whether it dropped or was abandoned.
   *
   * Takes the whole press away before calling out, because `onReorder` runs an
   * edit that comes back through `setPages` and rebuilds every row --- and a
   * strip that still believed a drag was live would then be pointing at rows
   * that no longer exist.
   */
  private endDrag(drop: boolean): void {
    const press = this.press;
    this.press = null;
    if (this.edgeFrame) {
      cancelAnimationFrame(this.edgeFrame);
      this.edgeFrame = 0;
    }
    this.indicator?.remove();
    this.indicator = null;
    if (press) {
      const row = this.rows.get(press.from);
      if (row) row.style.opacity = "";
      if (this.host.hasPointerCapture(press.pointerId)) {
        this.host.releasePointerCapture(press.pointerId);
      }
    }
    if (!drop || !press?.dragging) return;
    this.lastGap = press.gap;
    const to = landingSlot(press.from, press.gap);
    if (to === press.from) return;
    this.dropped++;
    this.opts.onReorder?.(press.from, to);
  }

  private readonly onPointerMove = (event: PointerEvent): void => {
    const press = this.press;
    if (!press || event.pointerId !== press.pointerId) return;
    press.clientY = event.clientY;
    if (!press.dragging) {
      if (Math.abs(event.clientY - press.startY) < DRAG_THRESHOLD) return;
      press.dragging = true;
      const row = this.rows.get(press.from);
      // Dimmed rather than moved. A row that followed the pointer would have to
      // be taken out of the absolutely-positioned layout that the windowing
      // owns, and the indicator is what actually says where the page is going.
      if (row) row.style.opacity = "0.4";
    }
    this.aimAt(event.clientY);
    this.edgeScroll();
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (this.press && event.pointerId === this.press.pointerId) this.endDrag(true);
  };

  private readonly onPointerCancel = (event: PointerEvent): void => {
    if (this.press && event.pointerId === this.press.pointerId) this.endDrag(false);
  };

  /**
   * Scrolls the strip while the pointer rests near an edge.
   *
   * A frame loop rather than a step per `pointermove`, because the case that
   * needs it most is a pointer held still at the bottom of the panel: with no
   * loop, a reader who has already reached the edge has to keep jiggling the
   * pointer for the strip to keep moving.
   */
  private edgeScroll(): void {
    if (this.edgeFrame || !this.press?.dragging) return;
    const step = (): void => {
      this.edgeFrame = 0;
      const press = this.press;
      if (!press?.dragging) return;
      const box = this.host.getBoundingClientRect();
      const above = press.clientY - box.top;
      const below = box.top + box.height - press.clientY;
      const by =
        above < EDGE_ZONE ? -EDGE_SPEED : below < EDGE_ZONE ? EDGE_SPEED : 0;
      if (by === 0) return;
      const was = this.host.scrollTop;
      this.host.scrollTop = Math.max(
        0,
        Math.min(was + by, this.rowHeight * this.opts.pageCount - box.height),
      );
      // The gap is recomputed even when the scroll did not move, because the
      // pointer may have travelled since the last frame; and the loop stops
      // when it did not move, because a strip already at its end would
      // otherwise reschedule for the whole life of the drag.
      this.aimAt(press.clientY);
      if (this.host.scrollTop !== was) this.edgeFrame = requestAnimationFrame(step);
    };
    this.edgeFrame = requestAnimationFrame(step);
  }

  private markCurrent(): void {
    for (const page of this.rows.keys()) this.mark(page, page === this.current);
  }

  private mark(page: number, on: boolean): void {
    const row = this.rows.get(page);
    if (!row) return;
    row.setAttribute("aria-selected", String(on));
    row.style.background = on
      ? "color-mix(in srgb, currentColor 12%, transparent)"
      : "";
  }

  private focus(page: number): void {
    if (this.focused === page) return;
    const previous = this.rows.get(this.focused);
    if (previous) previous.tabIndex = -1;
    this.focused = page;
    const row = this.rows.get(page);
    if (row) row.tabIndex = 0;
  }

  /** Moves the keyboard by `delta` rows. */
  private move(delta: number): void {
    const next = Math.max(
      0,
      Math.min(this.focused + delta, this.opts.pageCount - 1),
    );
    this.focus(next);
    this.scrollTo(next);
    this.rows.get(next)?.focus();
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    switch (event.key) {
      case "ArrowDown":
      case "ArrowRight":
        this.move(1);
        break;
      case "ArrowUp":
      case "ArrowLeft":
        this.move(-1);
        break;
      case "Escape":
        // Abandons a drag rather than dropping it. There is no other way out
        // once the pointer has been captured: releasing it *is* the drop, so
        // without this a reader who started a drag by accident has to complete
        // one. Falls through to the default when nothing is being dragged, so
        // Escape keeps whatever meaning the surrounding UI gives it.
        if (!this.press) return;
        this.endDrag(false);
        break;
      case "Home":
        this.move(-this.opts.pageCount);
        break;
      case "End":
        this.move(this.opts.pageCount);
        break;
      case "Enter":
      case " ": {
        // The row the key actually reached, not the one this class believes has
        // focus. `focused` is a mirror of the DOM's focus kept up to date by the
        // `focusin` listener, and a mirror can be stale: a document without
        // system focus moves `activeElement` without delivering the focus event,
        // so the mirror still says page 0 while the key lands on another row --
        // and activating page 0 is indistinguishable from Enter doing nothing.
        // The event's own target is authoritative in every case, because that
        // *is* the focused element; the mirror is only the fallback for a key
        // that arrived on the list rather than on a row.
        const from = (event.target as HTMLElement | null)?.dataset?.page;
        this.opts.onNavigate(from === undefined ? this.focused : Number(from));
        break;
      }
      default:
        return;
    }
    event.preventDefault();
    // The viewer underneath scrolls on the same keys.
    event.stopPropagation();
  };
}
