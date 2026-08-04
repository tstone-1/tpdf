/**
 * The reading surface: input, a frame loop, and a {@link Scroller} under it.
 *
 * `scroller.ts` was built to be driven by a benchmark, which supplies its own
 * scroll offset and calls `frame()` exactly as many times as it wants to
 * measure. This is the other caller --- a person with a trackpad --- and it owns
 * the three things a person needs that a benchmark does not: where the scroll
 * offset comes from, when a frame is worth running, and what to say when the
 * page on screen is not the page the document contains.
 *
 * ## The loop idles
 *
 * The benchmark runs a fixed number of frames back to back. A viewer that did
 * that would hold a core awake for as long as it was open, which on a laptop is
 * not a performance detail but the whole battery. So the loop runs only while
 * there is something to do --- the scroll is moving, or the scroller has work
 * that has not reached the screen --- and stops itself otherwise. Every input
 * path therefore has to {@link Viewer.wake}; a path that changes state without
 * waking leaves the screen stale until the next unrelated event, which looks
 * exactly like a rendering bug and is not one.
 *
 * ## What it says while it waits
 *
 * docs/PLAN.md section 9 closed the A0 vector page against "never below the
 * tier-1 placeholder" and recorded that the honest description of that pass is
 * "a blurry page that never blinks out" --- with the degraded state the UI owes
 * the user still owed. {@link ViewerStatus} is that debt: `sharp` and `any` come
 * straight out of the scroller's own coverage measurement, so what the status
 * line reports is the same number the benchmark reports, not an estimate of it.
 */

import { AccessibleText } from "./a11y";
import { matches } from "./keys";
import { Lifetime } from "./lifetime";
import { DESTINATION_MARGIN_PT } from "./outline";
import { displayedSize, Scroller, type PageSize } from "./scroller";
import {
  PLAIN_SEARCH,
  Search,
  sameOptions,
  type Match,
  type SearchOptions,
  type SearchScope,
  type ScopeRange,
} from "./search";
import { Selection } from "./selection";
import {
  caretAt,
  lineAt,
  nearestChar,
  runsFor,
  TextCache,
  wordAt,
  type Caret,
  type PageText,
} from "./text";
import { ClickCounter } from "./clicks";
import {
  clampZoom,
  fitZoom,
  nextStop,
  type FitMode,
  type Fitted,
} from "./zoom";

/** What a drag extends the selection by. */
export type SelectUnit = "char" | "word" | "line";

/** What the search box should be showing. */
export interface SearchStatus {
  /** The query being scanned for, or "". */
  query: string;
  /**
   * How a scan matches. What the toggles show.
   *
   * There is deliberately no second field for "how the results below were
   * matched": a rescan clears the matches in the same synchronous step that it
   * takes the new options, so the two could never be observed to disagree and a
   * check on them would be one that holds by construction.
   */
  options: SearchOptions;
  /** Matches found so far. */
  total: number;
  /** One-based position of the current match, or 0 when there is none. */
  index: number;
  /** Pages scanned so far, out of {@link toScan}. */
  scanned: number;
  /**
   * Pages this scan will look at: the document, or the selection it is scoped
   * to. What "finished" is measured against.
   */
  toScan: number;
  /** Whether the scan is confined to a selection the reader made. */
  scoped: boolean;
  /** Whether a scan is still running. */
  running: boolean;
  /**
   * Whether every page scanned so far had no extractable text.
   *
   * "No matches" and "nothing to match against" are different answers and are
   * reported as different: a scan of a scanned document never tested the query.
   */
  textless: boolean;
  /**
   * How many pages store text that no search can read.
   *
   * A third state between "this page has text" and "this page has none": the
   * page draws characters, and its fonts declare no mapping saying what they
   * mean, so what is extracted is PDFium's guess. `textless` cannot see it ---
   * the page is not textless --- and without this "No matches." is a lie about
   * a page nobody could have searched. See `encoding.rs`.
   */
  unsearchablePages: number;
  /**
   * Whether the backend has answered the question `unsearchablePages` reports.
   *
   * Zero means "no page is unreadable" and "nobody has asked yet", and no
   * consumer that draws a line for a reader needs to tell those apart --- both
   * say nothing. The check harness does: without it, the assertion that an
   * ordinary document reports no unreadable page is satisfied by a backend that
   * never answers at all.
   */
  mappingKnown: boolean;
  /**
   * Why the query could not be run at all, or "".
   *
   * Only a pattern can fail to be a query. Distinct from `total === 0` because
   * they are different statements: one says the document does not contain the
   * query, the other says the query was never asked.
   */
  problem: string;
}

/** What the surface is currently showing, for a status line to render. */
export interface ViewerStatus {
  /** One-based, as a reader counts. */
  page: number;
  pageCount: number;
  /** CSS pixels per PDF point. */
  zoom: number;
  /**
   * What the zoom is following, if anything.
   *
   * Beside the zoom rather than instead of it: the number is what a reader
   * wants to know, and the mode is why it changed by itself when they last
   * resized the window.
   */
  fit: FitMode;
  /** Quarter-turns clockwise the view is rotated by, 0 to 3. */
  turns: number;
  /** Whether the page's lightness is inverted, for reading in the dark. */
  invert: boolean;
  /** Fraction of the visible page area backed by a sharp tile. */
  sharp: number;
  /** Fraction backed by anything at all, tier-1 placeholder included. */
  any: number;
  /** Requests outstanding, so "still working" can be distinguished from "done". */
  pending: number;
  /**
   * Requests that came back an error since the document opened.
   *
   * Reported because "slow" and "erroring on everything" look identical from
   * every other field here, and they are the two things a reader most needs told
   * apart: one resolves by waiting and the other never does.
   */
  failed: number;
  /** Characters currently selected, so a status line can say so. */
  selected: number;
  /** State of the find-in-document scan. */
  search: SearchStatus;
}

export interface ViewerOptions {
  doc: number;
  pageCount: number;
  /**
   * Geometry of the pages the open reported, index-aligned from page 1.
   *
   * Usually just page 1: the open is lazy by default because collecting the
   * whole table costs 86 ms on a long document. The rest is learned as the
   * reader arrives at it --- see {@link Viewer.learnGeometry} --- and estimated
   * meanwhile by `scroller.ts`.
   */
  pages: [PageSize, ...PageSize[]];
  /** Tile edge in device pixels. Section 4 measured 1024--2048 as the range. */
  tilePx?: number;
  onStatus?: (status: ViewerStatus) => void;
  /**
   * Called every frame with the top of the viewport.
   *
   * Separate from `onStatus`, which deliberately fires only when something a
   * reader would notice changed and therefore not while scrolling *within* a
   * page --- which is exactly the movement an outline highlight has to follow.
   */
  onPosition?: (page: number, top: number) => void;
  /**
   * Called when something the reader asked for could not be done.
   *
   * Not for a tile that failed --- that is `ViewerStatus.failed` and the status
   * line already says so. This is for a command someone typed and is standing
   * there waiting for, where silence reads as a broken application: a copy that
   * could not include every page it spans, or a clipboard that refused the
   * write. Both used to resolve to nothing and say nothing.
   */
  onError?: (message: string) => void;
  /**
   * Called once if the document's file is truncated on disk while it is open.
   *
   * Deliberately not folded into {@link onError}, whose contract above is "a
   * command someone typed and is standing there waiting for". This is the
   * opposite: nobody asked for it, it arrives while they are reading, and the
   * cause is outside the application entirely. Keeping them apart is what lets
   * that doc comment stay true --- and lets a caller present them differently if
   * it ever wants to, without having to guess which kind a message is.
   */
  onGone?: (message: string) => void;
}

/**
 * Width of the scrollbar gutter, in CSS pixels.
 *
 * Exported for `viewercheck.ts`, which needs the width a page is actually
 * fitted into rather than the element's own. The difference is what made a
 * check on fit-page unable to fail: at the element's full width, a page that
 * had *not* been refitted after a rotation still appeared to fit, because the
 * 12 px it overflowed by was the gutter it was hiding under.
 */
export const SCROLLBAR_WIDTH = 12;

/** Shortest the scrollbar thumb may get on a long document. */
const MIN_THUMB = 24;

/** CSS pixels a line-mode wheel notch scrolls. */
const LINE_HEIGHT = 40;

/** CSS pixels an arrow key scrolls. */
const ARROW_STEP = 60;

/** Fraction of a screen that Page Up/Down moves, leaving context behind. */
const PAGE_OVERLAP = 0.9;

/**
 * Pages a copy asks for at once.
 *
 * Small enough that a tile the reader is waiting for never queues behind more
 * than a handful of extractions, large enough that a copy of a long selection
 * is not one round trip per page. See `Viewer.selectionText`.
 */
const COPY_CHUNK = 16;

/**
 * Overlay fills, all painted with `multiply` so the glyphs stay legible.
 *
 * The current match is a different hue rather than a darker shade of the same
 * one: on a page with many hits, "which of these am I on" has to survive being
 * read at a glance and next to a highlight of the other colour.
 */
const SELECTION_FILL = "rgba(80, 140, 255, 0.35)";
const MATCH_FILL = "rgba(255, 214, 0, 0.55)";
const CURRENT_MATCH_FILL = "rgba(255, 132, 0, 0.75)";

/**
 * Captures a pointer, tolerating one that does not exist.
 *
 * `setPointerCapture` throws `NotFoundError` for an id the browser has no active
 * pointer for, which is every id a synthetic `PointerEvent` carries --- so the
 * viewer check would take out the whole drag path rather than exercising it.
 * Capture is a convenience here: it keeps a drag alive past the edge of the
 * window, and losing it costs that and nothing else.
 */
function capture(element: HTMLElement, pointerId: number): void {
  try {
    element.setPointerCapture(pointerId);
  } catch {
    // No such pointer; the drag still works, it just ends at the window edge.
  }
}

/** Releases a capture that may never have been taken. */
function release(element: HTMLElement, pointerId: number): void {
  try {
    element.releasePointerCapture(pointerId);
  } catch {
    // Never captured; nothing to release.
  }
}

export class Viewer {
  private readonly root: HTMLElement;
  private readonly opts: ViewerOptions;
  private readonly surfaceHost: HTMLDivElement;
  private readonly overlay: HTMLCanvasElement;
  private readonly overlayCtx: CanvasRenderingContext2D | null;
  private readonly track: HTMLDivElement;
  private readonly thumb: HTMLDivElement;
  private readonly text: TextCache;
  private readonly a11y: AccessibleText;

  private readonly searcher: Search;
  /**
   * How the *next* scan will match, which is not always how the current results
   * were matched --- see {@link Search.options}.
   */
  private searchOptions: SearchOptions = PLAIN_SEARCH;
  /**
   * Index into `searcher.matches` of the match the viewport is on, or -1.
   *
   * Kept here rather than in {@link Search}, which only accumulates: which hit
   * a reader is looking at is a property of the surface, and the scan has to be
   * able to finish without moving anyone.
   */
  private currentMatch = -1;

  private selection: Selection | null = null;
  /** Where the search may look, or null for the whole document. */
  private searchScope: SearchScope | null = null;
  /** Whether a pointer is currently extending the selection. */
  private selecting = false;
  /** Counts clicks, so a double- and triple-click can be told apart. */
  private readonly clicks = new ClickCounter();
  /**
   * What a drag extends by: a character, a word, or a line.
   *
   * Set from the click count at the press and held for the whole drag, which is
   * what makes dragging after a double-click extend word by word rather than
   * dropping back to characters the moment the pointer moves.
   */
  private selectUnit: SelectUnit = "char";
  /**
   * The unit the drag started in, kept whole.
   *
   * A word- or line-granular selection has to cover the unit it *began* in
   * however far the pointer travels, and which end of that unit is the anchor
   * flips depending on the direction. Storing the caret alone loses the
   * information needed to flip it back.
   */
  private anchorUnit: { page: number; from: number; to: number } | null = null;
  /**
   * Pages whose text has been asked for.
   *
   * `TextCache` already dedupes the request, but not the `.then` attached to it,
   * and the frame loop would attach a fresh one every frame of a scroll over a
   * page still being extracted. This is what stops that.
   */
  private readonly textAsked = new Set<number>();

  private readonly scroller: Scroller;
  private scrollTop = 0;
  private zoom = 1;
  /** Quarter-turns clockwise the view is rotated by, 0 to 3. */
  private turns = 0;

  /**
   * Whether the page's lightness is inverted, for reading in the dark.
   *
   * Off by default and never inferred from the system theme. Inverting a page
   * changes what the document looks like, and a reader who has turned their
   * desktop dark has not thereby asked for that --- the chrome follows the
   * system, the page waits to be asked.
   */
  private invert = false;
  /**
   * What the zoom is tracking, if anything.
   *
   * A fit has to survive a resize, and the first resize is not optional: the
   * constructor runs before the layout that gives `root` a width, so the zoom
   * it computes is against a viewport of one pixel. Anchoring the initial fit
   * to a mode rather than to a measurement means the correction arrives through
   * the same path every later resize uses.
   *
   * It was a boolean until fit-page existed, and the change is not cosmetic:
   * two fits both have to be re-applied on a resize and a rotation, so the
   * viewer has to remember *which* one, not merely that it is fitting.
   */
  private fit: FitMode = "width";

  private frameHandle = 0;
  private running = false;
  /**
   * Whether {@link destroy} has run. Checked wherever a continuation re-enters.
   *
   * Almost everything here is asynchronous and almost none of it is cancellable:
   * a text extraction is an IPC round trip that resolves whenever it resolves,
   * and `TextCache.load` never rejects --- a failure resolves to `null`. So a
   * document closed while any of that is outstanding leaves `.then` callbacks
   * holding a viewer that has been torn down, and the one they all reach is
   * {@link wake}, which was idempotent about *running* and said nothing about
   * *destroyed*: it restarted the frame loop.
   *
   * What that costs is worth spelling out, because a resurrected loop does not
   * look like a leak from anywhere. The zombie tick drives the destroyed
   * scroller, which requests tiles for a document the backend has closed; they
   * fail; the backoff arms a retry; the retry wakes the zombie again, every
   * eight seconds, forever. And each tick fires the *old* `onStatus` and
   * `onPosition` closures, which in `App.svelte` write the module-level `status`
   * --- so a closed document keeps driving the sidebar and the header of the one
   * that replaced it.
   */
  private readonly life = new Lifetime();
  /**
   * Wake scheduled for a request whose backoff has not elapsed.
   *
   * The loop idles when there is no work, and a backed-off request is not work
   * yet --- so without this a tile that failed once would sit unretried until
   * some unrelated input happened to restart the loop, which for a transient
   * failure is a blank square that never fills. One timer, rearmed on each idle,
   * gives the scroller exactly one retry per backoff. See `scroller.ts`'s
   * `nextRetryMs`, which deliberately reports nothing for a request that is
   * already due.
   */
  private retryTimer = 0;
  /**
   * Frames to run after the last piece of work settles.
   *
   * One is not enough: the frame that drains an arrival is also the frame that
   * draws it, and the coverage the status line reports is measured after that.
   * Two means the reported state is always the state on screen.
   */
  private tail = 0;

  private lastStatus = "";
  private readonly observer: ResizeObserver;
  private dragOffset: number | null = null;

  constructor(root: HTMLElement, opts: ViewerOptions) {
    this.root = root;
    this.opts = opts;

    root.replaceChildren();
    root.style.position = "relative";
    root.style.overflow = "hidden";
    // Focusable so the arrow keys reach it without a global key listener that
    // would keep firing after the viewer is gone.
    root.tabIndex = 0;
    root.style.outline = "none";

    this.text = new TextCache(opts.doc);
    this.a11y = new AccessibleText(root, opts.pageCount);
    this.searcher = new Search(opts.doc, opts.pageCount, () =>
      this.onSearchProgress(),
    );

    this.surfaceHost = document.createElement("div");
    this.surfaceHost.style.cssText = "position:absolute;left:0;top:0;";
    // The tiles carry no text a screen reader can reach, and the parallel DOM in
    // `a11y.ts` carries all of it -- so leaving the canvases in the tree would
    // offer a reader a large, empty region to get lost in.
    this.surfaceHost.setAttribute("aria-hidden", "true");
    root.appendChild(this.surfaceHost);

    // Above the tiles and transparent to the pointer. A separate layer rather
    // than a pass inside the scroller, so the class that owns the tile cache
    // does not also have to know what a selection is.
    this.overlay = document.createElement("canvas");
    this.overlay.style.cssText =
      "position:absolute;left:0;top:0;pointer-events:none;";
    this.overlay.setAttribute("aria-hidden", "true");
    root.appendChild(this.overlay);
    this.overlayCtx = this.overlay.getContext("2d");

    this.track = document.createElement("div");
    this.track.style.cssText =
      `position:absolute;top:0;right:0;bottom:0;width:${SCROLLBAR_WIDTH}px;` +
      `background:color-mix(in srgb, CanvasText 12%, transparent);`;
    this.thumb = document.createElement("div");
    this.thumb.style.cssText =
      `position:absolute;left:2px;right:2px;top:0;height:0;border-radius:4px;` +
      `background:color-mix(in srgb, CanvasText 38%, transparent);`;
    this.track.appendChild(this.thumb);
    root.appendChild(this.track);

    // Page 1 by name rather than through {@link zoomFor}, which asks the
    // scroller which page is being read and the scroller does not exist yet. The
    // answer at this instant is page 1 in any case, and the fit that matters
    // arrives through the ResizeObserver: the constructor runs before the layout
    // that gives `root` a width, so this one is against a viewport of one pixel.
    this.zoom = fitZoom(
      "width",
      this.viewportSize(),
      displayedSize(opts.pages[0], 0),
    );
    this.scroller = new Scroller(this.surfaceHost, {
      doc: opts.doc,
      pageCount: opts.pageCount,
      pages: opts.pages,
      zoom: this.zoom,
      turns: 0,
      invert: false,
      // Measured verdict, docs/PLAN.md section 4: over ~3,300 timed frames the
      // per-tile-canvas layout dropped three frames and stalled once, while
      // this one dropped none at identical coverage and 3--4x lower per-frame
      // cost. It was written as the escalation path and is the default.
      layout: "viewport",
      tilePx: opts.tilePx ?? 1024,
      dpr: window.devicePixelRatio || 1,
      viewport: this.viewportSize(),
      prefetchScreens: 1,
      cacheTiles: 48,
      maxInFlight: 4,
      cancel: true,
      // Passed straight through rather than handled here. The viewer has nothing
      // useful to do about it --- it cannot re-read the file, and what is already
      // painted is worth keeping --- so the only correct action is at the level
      // that owns the window and can tell the reader.
      onGone: (message) => this.opts.onGone?.(message),
    });

    this.sizeOverlay();

    root.addEventListener("wheel", this.onWheel, { passive: false });
    root.addEventListener("keydown", this.onKeyDown);
    root.addEventListener("pointerdown", this.onSelectStart);
    this.track.addEventListener("pointerdown", this.onTrackPointerDown);

    this.observer = new ResizeObserver(() => this.onResize());
    this.observer.observe(root);

    this.wake();
  }

  destroy(): void {
    // First, so anything that lands during the teardown below finds it set.
    this.life.end();
    this.stop();
    clearTimeout(this.retryTimer);
    this.a11y.destroy();
    this.searcher.cancel();
    this.observer.disconnect();
    this.root.removeEventListener("wheel", this.onWheel);
    this.root.removeEventListener("keydown", this.onKeyDown);
    this.root.removeEventListener("pointerdown", this.onSelectStart);
    // A drag adds these two and its own `pointerup` takes them away again ---
    // which a document closed mid-drag never reaches. Removing a listener that
    // was never added is a no-op, so this needs no flag to consult; what it
    // must not do is rely on `life.ended`, which stops the frame loop and does
    // nothing about a move still extending a selection and still asking the
    // backend for the text of every page it crosses.
    this.root.removeEventListener("pointermove", this.onSelectMove);
    this.root.removeEventListener("pointerup", this.onSelectEnd);
    this.track.removeEventListener("pointerdown", this.onTrackPointerDown);
    // The scrollbar's drag is the same shape, and the same closed document can
    // be left in the middle of one.
    this.track.removeEventListener("pointermove", this.onTrackPointerMove);
    this.track.removeEventListener("pointerup", this.onTrackPointerUp);
    this.scroller.destroy();
    this.root.replaceChildren();
  }

  /** Gives the surface keyboard focus. */
  focus(): void {
    this.root.focus();
  }

  /**
   * Whether the frame loop has stopped.
   *
   * The claim this class makes about battery is that it does not run a frame
   * loop while nothing is happening, and a claim like that needs something that
   * can observe it --- otherwise it is a comment. `viewer-check` asserts both
   * directions: idle after the document settles, and *not* idle while work is
   * outstanding.
   */
  get idle(): boolean {
    return !this.running;
  }

  /** Current scroll offset, in CSS pixels from the top of the document. */
  get offset(): number {
    return this.scrollTop;
  }

  /** Furthest the document can be scrolled, in CSS pixels. */
  get maxOffset(): number {
    return this.scroller.maxScroll;
  }

  /** Current zoom, in CSS pixels per PDF point. */
  get currentZoom(): number {
    return this.zoom;
  }

  /**
   * A page's tier-1 placeholder, for the page strip to borrow.
   *
   * The strip renders at the same 150 px, so a page the viewer has already
   * prepared costs it nothing --- which on the A0 sheet is the difference
   * between showing the page being read immediately and 1.5 s of blank row.
   */
  placeholderFor(page: number): ImageBitmap | null {
    return this.scroller.placeholderFor(page);
  }

  /**
   * Starts the frame loop if it is not already running.
   *
   * Every state change goes through here. It is idempotent on purpose: callers
   * should not have to know whether the loop happens to be awake --- nor whether
   * the viewer is still alive, which is what the first line answers. That is the
   * whole of the post-destroy guard: every async continuation in this class ends
   * up here, so refusing here is refusing all of them at once. See
   * {@link life}.
   */
  wake(): void {
    if (this.life.ended) return;
    this.tail = 2;
    clearTimeout(this.retryTimer);
    this.retryTimer = 0;
    if (this.running) return;
    this.running = true;
    this.frameHandle = requestAnimationFrame(this.tick);
  }

  private stop(): void {
    clearTimeout(this.retryTimer);
    this.retryTimer = 0;
    if (!this.running) return;
    this.running = false;
    cancelAnimationFrame(this.frameHandle);
  }

  /**
   * Arranges one wake for the earliest backed-off request, if there is one.
   *
   * `now` is the frame's own clock reading, not a fresh one. The frame decided
   * which requests were due against that instant; taking a second reading here
   * would silently drop any request that came due between the two --- it was not
   * issued, and no wake would be armed for it. See `scroller.ts`'s
   * `nextRetryMs`.
   */
  private scheduleRetry(now: number): void {
    clearTimeout(this.retryTimer);
    this.retryTimer = 0;
    const wait = this.scroller.nextRetryMs(now);
    if (wait === null) return;
    this.retryTimer = setTimeout(() => this.wake(), wait) as unknown as number;
  }

  private readonly tick = (): void => {
    const now = performance.now();
    // Before the frame, so a correction reaches the tile requests this frame
    // issues rather than the next one's: a page laid out at the wrong width is
    // asked for at the wrong width, and every one of those tiles is then thrown
    // away by the correction that follows it.
    this.learnGeometry();
    const stats = this.scroller.frame(this.scrollTop, now);
    this.prefetchText();
    this.syncAccessibleText();
    this.paintOverlay();
    this.paintThumb();
    this.report(stats);
    const where = this.position;
    this.opts.onPosition?.(where.page, where.top);

    // Work outstanding keeps the loop awake indefinitely; `tail` covers the
    // frames after the last of it settles, and is refilled by every input.
    if (this.scroller.pendingWork > 0) this.tail = 2;
    else this.tail--;

    if (this.tail <= 0) {
      this.running = false;
      // Going idle is the one moment a backed-off request needs somebody to
      // come back for it.
      this.scheduleRetry(now);
      return;
    }
    this.frameHandle = requestAnimationFrame(this.tick);
  };

  /**
   * Tells the scroller the real size of any visible page it is guessing at.
   *
   * The open is lazy --- `render.rs` sends page 1's geometry alone, because
   * collecting the whole table costs 86 ms on a long document --- so every other
   * page starts out estimated. This is where the estimate is corrected, and it
   * costs nothing extra: {@link prefetchText} already asks for the text of every
   * visible page, every `PageText` carries the page's size, and the round trip
   * was happening anyway. There is no second request and no new command.
   *
   * A page is asked about once. Once its size is known it stops being read from
   * the cache, which also keeps this off the LRU: `peek` counts as a use, and
   * touching every visible page here as well as in the paint path would be
   * bookkeeping for an answer already held.
   *
   * The re-anchor is the half that is easy to leave out. The scroll offset is
   * CSS pixels down a document that has just changed length, so a correction
   * three pages up moves the reader without them touching anything --- which on
   * a long document is far enough that they have simply lost their place. What is
   * preserved is the page and the fraction through it, exactly as
   * {@link rotateBy} preserves them across a change of proportions.
   */
  private learnGeometry(): void {
    const anchor = this.scroller.pageAt(this.scrollTop);
    const pitch = this.scroller.pagePitchOf(anchor);
    const through =
      pitch > 0
        ? (this.scrollTop - this.scroller.pageTopOf(anchor)) / pitch
        : 0;

    let moved = false;
    for (const page of this.scroller.visiblePages()) {
      if (this.scroller.knowsPageSize(page)) continue;
      const text = this.text.peek(page);
      if (!text) continue;
      // `PageText` reports the page as *displayed*, so the view's own rotation
      // has to come back out before this is the document's geometry. The two
      // are the same thing at an even number of quarter-turns, which is why
      // getting it wrong is invisible until somebody rotates a mixed document.
      const shown = { width_pt: text.width_pt, height_pt: text.height_pt };
      if (this.scroller.notePageSize(page, displayedSize(shown, -this.turns))) {
        moved = true;
      }
    }
    if (!moved) return;

    // The fit follows the page being read, so a page that has just turned out to
    // be A3 is refitted rather than left at the previous page's scale. Before
    // the re-anchor: a fit changes the zoom, which changes the pitch the
    // fraction below is resolved against.
    this.applyFit();
    this.scrollTop = Math.max(
      0,
      Math.min(
        this.scroller.pageTopOf(anchor) +
          through * this.scroller.pagePitchOf(anchor),
        this.scroller.maxScroll,
      ),
    );
    this.wake();
  }

  /**
   * The page a reader would say they are on.
   *
   * Taken at the middle of the viewport rather than its top edge, so the number
   * changes when most of the screen changes rather than as soon as the previous
   * page's last line scrolls out.
   */
  private currentPage(): number {
    return this.scroller.pageAt(
      this.scrollTop + this.viewportSize().height / 2,
    );
  }

  /**
   * Emits a status only when something a reader could notice has changed.
   *
   * The summary below is the whole of that decision, so every field the UI
   * renders has to appear in it. One that does not is a control that sticks:
   * with an empty query the search toggles and the scope move nothing else in
   * here, so the button keeps its old `class:on` and its old `aria-pressed`
   * while the viewer's setting has already flipped --- silently, and for a
   * screen reader as well as on screen.
   */
  private report(stats: { sharp: number; any: number }): void {
    const status: ViewerStatus = {
      page: this.currentPage() + 1,
      pageCount: this.opts.pageCount,
      zoom: this.zoom,
      fit: this.fit,
      turns: this.turns,
      invert: this.invert,
      sharp: stats.sharp,
      any: stats.any,
      pending: this.scroller.pendingWork,
      failed: this.scroller.stats.failed,
      selected: this.selectedCount(),
      search: this.searchStatus(),
    };
    const summary = [
      status.page,
      status.zoom,
      status.fit,
      status.turns,
      status.invert,
      Math.round(status.sharp * 100),
      Math.round(status.any * 100),
      status.pending > 0,
      status.failed,
      status.selected,
      status.search.query,
      status.search.options.matchCase,
      status.search.options.wholeWord,
      status.search.options.regex,
      status.search.scoped,
      status.search.total,
      status.search.index,
      status.search.scanned,
      status.search.running,
      status.search.textless,
      status.search.unsearchablePages,
    ].join("/");
    if (summary === this.lastStatus) return;
    this.lastStatus = summary;
    this.opts.onStatus?.(status);
  }

  private viewportSize(): { width: number; height: number } {
    return {
      width: Math.max(1, this.root.clientWidth - SCROLLBAR_WIDTH),
      height: Math.max(1, this.root.clientHeight),
    };
  }

  /**
   * The size of the page being read, as displayed, i.e. after the view rotation.
   *
   * The page being read rather than page 1, which is what a fit is about: on a
   * document with an A3 insert, fit-width means "fit the sheet in front of me",
   * and fitting every page to page 1's width leaves the wide one overflowing the
   * window with no way to see its edge.
   */
  private displayedPage(): PageSize {
    return displayedSize(
      this.scroller.pageSize(this.currentPage()),
      this.turns,
    );
  }

  /** A page's size in points, real or estimated. For the check harness. */
  pageSize(page: number): PageSize {
    return this.scroller.pageSize(page);
  }

  /** Whether a page's size is the document's own rather than an estimate. */
  knowsPageSize(page: number): boolean {
    return this.scroller.knowsPageSize(page);
  }

  /** The zoom `mode` asks for, against the viewport and page as they are now. */
  private zoomFor(mode: Fitted): number {
    return fitZoom(mode, this.viewportSize(), this.displayedPage());
  }

  /**
   * Re-applies the current fit, if there is one.
   *
   * The single place a fit is recomputed, called from the four events that
   * invalidate one: a resize, a rotation, a restore, and the command that asks
   * for the fit in the first place.
   *
   * It goes through {@link setZoom}, so the reader's place is anchored on the
   * viewport centre --- which is what a resize wants. A rotation and a restore
   * overwrite the scroll offset immediately afterwards with a place they have
   * computed themselves, so the anchoring is discarded there rather than
   * fighting them, and they need no separate path.
   */
  private applyFit(): void {
    if (this.fit === "none") return;
    this.setZoom(this.zoomFor(this.fit));
  }

  private scrollBy(delta: number): void {
    this.scrollTo(this.scrollTop + delta);
  }

  private scrollTo(top: number): void {
    const clamped = Math.max(0, Math.min(top, this.scroller.maxScroll));
    if (clamped === this.scrollTop) return;
    this.scrollTop = clamped;
    this.wake();
  }

  /**
   * Sets the zoom, keeping the point at the centre of the viewport fixed.
   *
   * Anchoring on the centre rather than the top is what makes a zoom step feel
   * like magnification instead of a jump: the scroll offset is in CSS pixels
   * and the whole document just changed length underneath it.
   */
  setZoom(zoom: number): void {
    const next = clampZoom(zoom);
    if (next === this.zoom) return;

    const half = this.viewportSize().height / 2;
    const anchor = (this.scrollTop + half) / this.zoom;
    this.zoom = next;
    this.scroller.setZoom(next);
    this.scrollTop = Math.max(
      0,
      Math.min(anchor * next - half, this.scroller.maxScroll),
    );
    this.wake();
  }

  /** Steps to the next zoom stop in `direction`. */
  zoomStep(direction: 1 | -1): void {
    const next = nextStop(this.zoom, direction);
    // At either end of the ladder there is no step to take, and pinning the fit
    // off for a keypress that changed nothing would silently stop the zoom
    // following the next resize.
    if (next === null) return;
    this.setZoomFixed(next);
  }

  /** Sets a zoom that stays put: the fit stops following the window. */
  setZoomFixed(zoom: number): void {
    this.fit = "none";
    this.setZoom(zoom);
    this.wake();
  }

  /**
   * Fits the page to the window, and keeps it fitted.
   *
   * One entry point for both fits rather than a method each, because the only
   * difference between them is which zoom {@link fitZoom} returns --- everything
   * about surviving a resize and a rotation is shared, and two copies of that
   * is how one of them ends up not surviving a rotation.
   */
  setFit(mode: Fitted): void {
    this.fit = mode;
    this.applyFit();
    this.wake();
  }

  /** Quarter-turns clockwise the view is currently rotated by. */
  get rotation(): number {
    return this.turns;
  }

  /** Whether the page's lightness is currently inverted. */
  get inverted(): boolean {
    return this.invert;
  }

  /**
   * Turns page inversion on or off.
   *
   * Nothing about the layout moves, so unlike a rotation there is no place to
   * preserve --- the reader stays exactly where they were and the pixels arrive
   * again in the other polarity. What they see meanwhile is the tier-1
   * placeholder, itself re-rendered inverted, which is the same degradation any
   * zoom or rotation already produces.
   */
  setInverted(invert: boolean): void {
    if (invert === this.invert) return;
    this.invert = invert;
    this.scroller.setInvert(invert);
    this.wake();
  }

  /** What the scroller composited into, for a check that must read pixels. */
  get compositedSurface(): HTMLCanvasElement | null {
    return this.scroller.compositedSurface;
  }

  /**
   * The size on screen of the page being read, as the scroller laid it out.
   *
   * The page being read, for the same reason {@link displayedPage} is: it is the
   * page a fit is computed against, so a check comparing the two has to be
   * looking at the same one. Identical on a uniform document.
   */
  get pageBoxCss(): { width: number; height: number } {
    return this.scroller.pageBoxCssOf(this.currentPage());
  }

  /** A page's size on screen, whichever page a caller means. */
  pageBoxCssOf(page: number): { width: number; height: number } {
    return this.scroller.pageBoxCssOf(page);
  }

  /**
   * CSS-pixel top of a page in the scrolled document.
   *
   * For the check harness, which asserts that the gap between two pages' tops is
   * the *first* page's own height. `goToPage` scrolls there and is not a
   * substitute: it clamps to `maxScroll`, so the last page's top is unreadable
   * through it on any document shorter than the window.
   */
  pageTopCss(page: number): number {
    return this.scroller.pageTopOf(page);
  }

  /**
   * Rotates the view by `delta` quarter-turns, keeping the reader on their page.
   *
   * A landscape page turned upright is a different length, so the scroll offset
   * that was three pages down is now somewhere else entirely --- and on a long
   * document "somewhere else" is far enough that the reader has simply lost
   * their place. The page is preserved rather than the offset, and the fraction
   * *through* that page with it, which is the closest thing to "the same place"
   * that survives a change of proportions.
   *
   * Rotating never touches the document. It is a property of the view, and the
   * page's own `/Rotate` is left exactly as the file has it --- changing that is
   * a page operation, and belongs with the ones that write.
   */
  rotateBy(delta: number): void {
    const next = (((this.turns + delta) % 4) + 4) % 4;
    if (next === this.turns) return;

    const page = this.currentPage();
    const before = this.scroller.pagePitchOf(page);
    const through =
      before > 0
        ? (this.scrollTop - this.scroller.pageTopOf(page)) / before
        : 0;

    this.applyTurns(next);
    // A rotation changes the page's aspect, so a view that was fitted is no
    // longer fitted to anything. Refitting is what makes the command feel like
    // turning a sheet of paper rather than cropping one --- and under fit-page
    // it is the difference between seeing the turned page and seeing a third of
    // it, since a landscape page fitted upright is far too tall.
    this.applyFit();

    this.scrollTop = Math.max(
      0,
      Math.min(
        this.scroller.pageTopOf(page) +
          through * this.scroller.pagePitchOf(page),
        this.scroller.maxScroll,
      ),
    );
    // The selection is a range of character indices and survives untouched: a
    // rotation is an isometry, so the same characters are still selected and
    // `runsFor` draws their highlight in the new orientation because the boxes
    // it reads have turned with the view.
    this.wake();
  }

  /**
   * Turns the view, and everything keyed to its orientation.
   *
   * Three consumers, and each is separately capable of being wrong while the
   * other two are right --- a scroller laying pages out upright inside a
   * correctly turned window, a text layer selecting the page's other axis. The
   * viewer check asserts all three.
   */
  private applyTurns(next: number): void {
    this.turns = next;
    this.text.setTurns(next);
    this.scroller.setTurns(next);
  }

  /** What the zoom is following, if anything. */
  get fitMode(): FitMode {
    return this.fit;
  }

  /**
   * Puts the view back where a previous session left it.
   *
   * Order is the whole of it. A rotation changes a page's proportions and a
   * zoom changes how long the document is, so both have to land before the
   * scroll offset is computed --- the same offset means a different place after
   * either. `rotateBy` solves the opposite problem, preserving a reader's place
   * *across* a turn; here there is no place to preserve yet.
   *
   * The offset is applied only to an upright view, for the reason `position`
   * records: a rotated view reports no offset because the axis it would be
   * measured down is not the one being scrolled, so there is nothing to restore
   * and landing on the page is the honest interpretation.
   */
  restore(place: {
    page: number;
    top_pt: number;
    zoom: number;
    fit: FitMode;
    turns: number;
  }): void {
    this.applyTurns(((place.turns % 4) + 4) % 4);

    this.fit = place.fit;
    // The remembered zoom is used only when nothing was being followed. Under a
    // fit it is stale by construction --- it was computed against the window
    // the reader had last time, and this one is a different size.
    if (this.fit === "none") this.setZoom(place.zoom);
    else this.applyFit();

    const page = Math.max(0, Math.min(place.page, this.opts.pageCount - 1));
    const offset = this.turns === 0 ? Math.max(0, place.top_pt) : 0;
    this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);
    this.wake();
  }

  /**
   * Where the top of the viewport is: a page, and points down it.
   *
   * The *top* edge rather than the middle, which is what `currentPage` uses.
   * They answer different questions: "which page am I on" is about what fills
   * the screen, and "which section am I in" is about the heading above me,
   * which sits at the top.
   *
   * A rotated view reports the page and no offset, for the same reason
   * {@link goToDestination} ignores one: the destinations this is compared
   * against are measured down an upright page, and under a quarter turn that is
   * not the axis being scrolled. Outline highlighting falls back to page
   * granularity, which is coarse and right, rather than fine and wrong.
   */
  get position(): { page: number; top: number } {
    const page = this.scroller.pageAt(this.scrollTop);
    if (this.turns !== 0) return { page, top: 0 };
    const top = (this.scrollTop - this.scroller.pageTopOf(page)) / this.zoom;
    return { page, top: Math.max(0, top) };
  }

  /**
   * Scrolls to an outline destination.
   *
   * `top` is points from the page's top, or `null` for a destination like
   * `/Fit` that names no coordinate --- which means the page, so the page's top
   * is the honest interpretation of it.
   */
  goToDestination(page: number, top: number | null): void {
    const clamped = Math.max(0, Math.min(page, this.opts.pageCount - 1));
    const base = this.scroller.pageTopOf(clamped);
    // A rotated view has no vertical offset within a page to scroll to: at a
    // quarter turn the destination's axis is the screen's horizontal one, and at
    // a half turn it counts upwards from the bottom while the reader still
    // scrolls down. Rather than place a heading somewhere plausible and wrong,
    // this lands on the page --- which is exactly what `/Fit` means, and what
    // `outline.rs` already returns for a destination it cannot place.
    const offset = this.turns === 0 ? (top ?? 0) : 0;
    // A little air above, for the same reason `goToMatch` leaves a third of a
    // screen: a heading flush against the top edge reads as cut off.
    this.scrollTo(base + (offset - DESTINATION_MARGIN_PT) * this.zoom);
  }

  /** Scrolls so page `page` (zero-based) starts at the top of the viewport. */
  goToPage(page: number): void {
    const clamped = Math.max(0, Math.min(page, this.opts.pageCount - 1));
    this.scrollTo(this.scroller.pageTopOf(clamped));
  }

  /** Scrolls to the very top of the document. */
  goToStart(): void {
    this.scrollTo(0);
  }

  /**
   * Scrolls to the very bottom.
   *
   * The end of the document, not the top of the last page: on a document whose
   * last page is taller than the window those are different places, and End is
   * expected to reach the end.
   */
  goToEnd(): void {
    this.scrollTo(this.scroller.maxScroll);
  }

  /** Scrolls to the top of the next page. */
  nextPage(): void {
    this.goToPage(this.currentPage() + 1);
  }

  /** Scrolls to the top of the previous page. */
  previousPage(): void {
    this.goToPage(this.currentPage() - 1);
  }

  private onResize(): void {
    const viewport = this.viewportSize();
    this.scroller.resize(viewport);
    this.sizeOverlay();
    this.applyFit();
    this.scrollTo(Math.min(this.scrollTop, this.scroller.maxScroll));
    this.wake();
  }

  private readonly onWheel = (event: WheelEvent): void => {
    // The webview would otherwise rubber-band the whole page around a surface
    // that does its own scrolling.
    event.preventDefault();

    // A trackpad pinch arrives as a wheel event with `ctrlKey` set --- there is
    // no separate gesture event here --- and Cmd-wheel is the mouse equivalent.
    if (event.ctrlKey || event.metaKey) {
      this.setZoomFixed(this.zoom * Math.exp(-event.deltaY / 300));
      return;
    }

    const scale =
      event.deltaMode === 1
        ? LINE_HEIGHT
        : event.deltaMode === 2
          ? this.viewportSize().height
          : 1;
    this.scrollBy(event.deltaY * scale);
  };

  /**
   * The surface's key handling.
   *
   * Advertised bindings are matched through `keys.ts`, which is the same table
   * the palette renders its labels from --- so a label can no longer teach a
   * chord this handler does not accept. It also tests the modifiers in *both*
   * directions, which the hand-written chain did not: `event.key === "p"` had no
   * accelerator test and sat after the ⌘-guarded arms, so ⌘P printed the
   * document and turned the page back at the same time.
   *
   * The scrolling keys below stay literal. They are not commands, the palette
   * does not list them, and there is no label for them to disagree with.
   */
  private readonly onKeyDown = (event: KeyboardEvent): void => {
    const screen = this.viewportSize().height * PAGE_OVERLAP;

    if (matches("view.zoomIn", event)) {
      this.zoomStep(1);
    } else if (matches("view.zoomOut", event)) {
      this.zoomStep(-1);
    } else if (matches("view.fitWidth", event)) {
      this.setFit("width");
    } else if (matches("view.fitPage", event)) {
      this.setFit("page");
    } else if (matches("view.actualSize", event)) {
      this.setZoomFixed(1);
    } else if (matches("view.rotateClockwise", event)) {
      // Preview's bindings. Acrobat puts these on Shift-Cmd-+/-, which on this
      // keyboard is the same `key` as Cmd-+ and would collide with zoom.
      this.rotateBy(1);
    } else if (matches("view.rotateCounterClockwise", event)) {
      this.rotateBy(-1);
    } else if (matches("edit.copy", event)) {
      void this.copySelection();
    } else if (matches("edit.selectAll", event)) {
      this.selectPage();
    } else if (matches("find.previous", event)) {
      this.prevMatch();
    } else if (matches("find.next", event)) {
      this.nextMatch();
    } else if (matches("edit.clearSelection", event)) {
      this.clearSelection();
    } else if (matches("nav.nextPage", event)) {
      this.nextPage();
    } else if (matches("nav.previousPage", event)) {
      this.previousPage();
    } else if (matches("nav.firstPage", event)) {
      this.goToStart();
    } else if (matches("nav.lastPage", event)) {
      this.goToEnd();
    } else if (event.key === "ArrowDown") {
      this.scrollBy(ARROW_STEP);
    } else if (event.key === "ArrowUp") {
      this.scrollBy(-ARROW_STEP);
    } else if (event.key === "PageDown" || event.key === " ") {
      this.scrollBy(event.shiftKey ? -screen : screen);
    } else if (event.key === "PageUp") {
      this.scrollBy(-screen);
    } else {
      return;
    }
    event.preventDefault();
  };

  // --- Text selection ------------------------------------------------------

  private sizeOverlay(): void {
    const { width, height } = this.viewportSize();
    const dpr = window.devicePixelRatio || 1;
    this.overlay.width = Math.round(width * dpr);
    this.overlay.height = Math.round(height * dpr);
    this.overlay.style.width = `${width}px`;
    this.overlay.style.height = `${height}px`;
  }

  /** Characters selected, or 0. Cheap enough to compute every frame. */
  private selectedCount(): number {
    if (!this.selection) return 0;
    let total = 0;
    for (const page of this.selection.pages()) {
      const range = this.selection.rangeOn(page);
      const text = range && this.text.peek(page);
      if (!range || !text) continue;
      total += Math.min(range.to, text.codes.length) - range.from;
    }
    return Math.max(0, total);
  }

  /**
   * Turns a pointer event into a caret.
   *
   * Returns `null` while the page's text has not arrived --- and asks for it, so
   * the next attempt can succeed. A drag that begins before the text lands
   * therefore does nothing until it does, rather than anchoring at character
   * zero and selecting the whole page on the first move.
   */
  /**
   * Where a point in a page lands in the window, in CSS pixels from its corner.
   *
   * The inverse of what {@link caretFrom} does with a pointer event, and it
   * exists for the check harness: a check that wants to drag across a *specific*
   * part of a page has to know where that part is on screen, and picking a fixed
   * screen row instead makes the check a statement about the fixture's margins.
   */
  screenPoint(page: number, x: number, y: number): { x: number; y: number } {
    const origin = this.scroller.pageOrigin(page);
    return {
      x: origin.left + x * this.zoom,
      y: origin.top + y * this.zoom - this.scrollTop,
    };
  }

  /** A page's text as the view shows it, or `null` if it has not arrived. */
  textOn(page: number): PageText | null {
    return this.text.peek(page);
  }

  /** Where a pointer event falls, in one page's own point space. */
  private pointFrom(
    event: PointerEvent,
  ): { page: number; text: PageText; x: number; y: number } | null {
    const bounds = this.root.getBoundingClientRect();
    const docY = event.clientY - bounds.top + this.scrollTop;
    const page = this.scroller.pageAt(docY);

    const text = this.text.peek(page);
    if (!text) {
      this.requestText(page);
      return null;
    }

    const origin = this.scroller.pageOrigin(page);
    return {
      page,
      text,
      x: (event.clientX - bounds.left - origin.left) / this.zoom,
      y: (docY - origin.top) / this.zoom,
    };
  }

  private caretFrom(event: PointerEvent): Caret | null {
    const point = this.pointFrom(event);
    if (!point) return null;
    return { page: point.page, index: caretAt(point.text, point.x, point.y) };
  }

  /**
   * The word or line under a pointer, for a granular drag.
   *
   * Asks for the character under the pointer rather than the caret beside it:
   * the caret after a word's last glyph names the space that follows, so a word
   * selection built on one selects the gap. See `nearestChar`.
   */
  private unitFrom(
    event: PointerEvent,
  ): { page: number; from: number; to: number } | null {
    const point = this.pointFrom(event);
    if (!point) return null;
    const index = nearestChar(point.text, point.x, point.y);
    if (index < 0) return null;
    const range =
      this.selectUnit === "line"
        ? lineAt(point.text, index)
        : wordAt(point.text, index);
    return { page: point.page, from: range.from, to: range.to };
  }

  private readonly onSelectStart = (event: PointerEvent): void => {
    // Only the primary button starts a selection; a right-click will open a
    // context menu once there is one, and should not clear what is selected.
    if (event.button !== 0) return;
    // The scrollbar is inside the root and has its own drag.
    if (this.track.contains(event.target as Node)) return;
    this.root.focus();

    // Document coordinates, not viewport ones: a scroll, zoom or page jump
    // between two clicks moves the text out from under a still pointer, and
    // keying the run on where the *document* was clicked ends it automatically.
    const bounds = this.root.getBoundingClientRect();
    const count = this.clicks.press(
      event.clientX - bounds.left,
      event.clientY - bounds.top + this.scrollTop,
      performance.now(),
    );
    this.selectUnit = count === 2 ? "word" : count === 3 ? "line" : "char";

    if (this.selectUnit === "char") {
      const caret = this.caretFrom(event);
      this.selection = caret ? new Selection(caret) : null;
      this.selecting = caret !== null;
      this.anchorUnit = null;
    } else {
      // The unit is read with the granularity already set, so a triple-click
      // asks for a line rather than widening the word a double-click found.
      const unit = this.unitFrom(event);
      this.anchorUnit = unit;
      this.selecting = unit !== null;
      if (unit) {
        this.selection = new Selection({ page: unit.page, index: unit.from });
        this.selection.focus = { page: unit.page, index: unit.to };
      } else {
        this.selection = null;
      }
    }

    capture(this.root, event.pointerId);
    this.root.addEventListener("pointermove", this.onSelectMove);
    this.root.addEventListener("pointerup", this.onSelectEnd);
    event.preventDefault();
    this.wake();
  };

  private readonly onSelectMove = (event: PointerEvent): void => {
    if (!this.selecting || !this.selection) return;

    const anchor = this.anchorUnit;
    if (this.selectUnit !== "char" && anchor) {
      const unit = this.unitFrom(event);
      if (!unit) return;
      // Which end of the *anchor's* unit is the fixed one flips with the
      // direction of travel, or dragging backwards would leave the word the
      // drag started in half-selected --- from its start to the pointer, rather
      // than from its end back to where the pointer now is.
      const before =
        unit.page < anchor.page ||
        (unit.page === anchor.page && unit.to <= anchor.from);
      this.selection.anchor = {
        page: anchor.page,
        index: before ? anchor.to : anchor.from,
      };
      this.selection.focus = {
        page: unit.page,
        index: before ? unit.from : unit.to,
      };
      this.requestText(unit.page);
      this.wake();
      return;
    }

    const caret = this.caretFrom(event);
    if (!caret) return;
    this.selection.focus = caret;
    // Pages crossed mid-drag have to be fetched, or the highlight stops at the
    // page boundary and the copied text quietly omits them.
    this.requestText(caret.page);
    this.wake();
  };

  private readonly onSelectEnd = (event: PointerEvent): void => {
    this.selecting = false;
    release(this.root, event.pointerId);
    this.root.removeEventListener("pointermove", this.onSelectMove);
    this.root.removeEventListener("pointerup", this.onSelectEnd);
    this.wake();
  };

  /** Clears the selection. */
  clearSelection(): void {
    if (!this.selection) return;
    this.selection = null;
    this.wake();
  }

  /**
   * Selects every character of the page currently being read.
   *
   * Retried when the text arrives, and only then. The retry used to be
   * unconditional --- `.then(() => this.selectPage())` --- which on a page whose
   * extraction fails is an unbounded loop of IPC calls: `TextCache.load`
   * resolves to `null` on error and caches nothing, so `peek` is still empty,
   * so the next attempt issues a *fresh* `page_text` and so does the one after
   * it. Nothing bounded that, and closing the document did not stop it either.
   * The load's own result is the answer: no text, no retry.
   */
  selectPage(): void {
    const page = this.currentPage();
    const text = this.text.peek(page);
    if (!text) {
      void this.text.load(page).then((arrived) => {
        if (arrived && !this.life.ended) this.selectPage();
      });
      return;
    }
    this.selection = new Selection({ page, index: 0 });
    this.selection.focus = { page, index: text.codes.length };
    this.wake();
  }

  /**
   * Puts the selected text on the clipboard.
   *
   * Resolves to what was copied, or `null` if nothing was --- including when
   * something went wrong, which is reported through `onError` rather than by
   * rejecting: every caller of this is a `void`ed keystroke, so a rejection
   * would be an unhandled one.
   *
   * It waits for any page whose text has not arrived: a selection dragged
   * quickly across a page boundary can reach the clipboard before the
   * extraction does, and silently copying the part that happened to be loaded is
   * the kind of bug a user discovers in someone else's document.
   *
   * **Waiting is not the same as having it**, which is the half that was
   * missing. A load resolves whether or not the extraction succeeded ---
   * `TextCache.load` resolves to `null` on failure --- and `Selection.text`
   * skips a page it cannot read, by design and as its own docstring says. So
   * the text is taken from each reply as it lands, and a reply that carries
   * nothing is an error rather than a page quietly left out. See
   * {@link selectionText}.
   */
  async copySelection(): Promise<string | null> {
    const selection = this.selection;
    if (!selection) return null;

    const text = await this.selectionText(selection);
    if (text === null) {
      this.opts.onError?.(
        "Some of the selected pages' text could not be read, so nothing was copied.",
      );
      return null;
    }
    if (!text) return null;

    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      // A refusal, not a silence: the reader pressed a key and is entitled to
      // know the clipboard does not hold what they asked for. The webview can
      // reject this --- permission, or a window that is not focused --- and
      // every caller here voids the promise, so nothing else would ever see it.
      this.opts.onError?.(`Could not write to the clipboard: ${String(e)}`);
      return null;
    }
    return text;
  }

  /**
   * The whole of a selection's text, loading whatever has not arrived, or
   * `null` if any page of it could not be read.
   *
   * Chunked rather than one `Promise.all`, because a selection dragged to the
   * end of the 775-page corpus names 775 pages and asking for them at once puts
   * 775 extractions onto the single FIFO queue that also draws the page in
   * front of the reader. `prefetchText` and `TextCache` both go out of their
   * way to avoid exactly that ("asking for all of it up front would put a
   * minute of extraction in front of the first tile"); the copy path let it
   * back in through the other door. Chunked rather than *capped*, because a
   * copy has to be complete --- silently copying the part that fitted is the
   * bug this whole path exists to avoid.
   *
   * Each page's text is taken from its own reply, as it lands, and never read
   * back out of the cache afterwards. That is not tidiness: `TextCache` evicts
   * least-recently-used down to `TEXT_CACHE_CHARS`, and this loop touches each
   * page exactly once and in ascending order --- so the eviction order *is* the
   * order of the selection, and the front of it is dropped while the tail is
   * still arriving. A copy that re-read the cache could therefore never succeed
   * past the bound, and blamed the document for it. The cache is an
   * optimisation; correctness here may not rest on it holding anything.
   */
  private async selectionText(selection: Selection): Promise<string | null> {
    const pages = selection.pages();
    const parts: string[] = [];
    for (let at = 0; at < pages.length; at += COPY_CHUNK) {
      const chunk = await Promise.all(
        pages.slice(at, at + COPY_CHUNK).map(async (page) => ({
          page,
          text: await this.text.load(page),
        })),
      );
      for (const { page, text } of chunk) {
        // The page could not be read at all. Reported rather than skipped ---
        // see {@link copySelection}.
        if (!text) return null;
        const part = selection.textFrom(page, text);
        if (part !== null) parts.push(part);
      }
    }
    return parts.join("\n");
  }

  /** The selected text, without touching the clipboard. For the check harness. */
  get selectedText(): string {
    return this.selection ? this.selection.text(this.text) : "";
  }

  /** Draws the search highlights and the selection, in that order. */
  private paintOverlay(): void {
    const ctx = this.overlayCtx;
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);

    // Multiply keeps the glyphs legible underneath, which a flat fill over the
    // tile would not: the text is already painted into the pixels.
    ctx.globalCompositeOperation = "multiply";
    this.paintMatches(ctx, dpr);
    this.paintSelection(ctx, dpr);
    ctx.globalCompositeOperation = "source-over";
  }

  private paintSelection(ctx: CanvasRenderingContext2D, dpr: number): void {
    if (!this.selection) return;
    ctx.fillStyle = SELECTION_FILL;

    for (const page of this.scroller.visiblePages()) {
      const origin = this.scroller.pageOrigin(page);
      for (const quad of this.selection.quadsOn(page, this.text)) {
        const left = (origin.left + quad.left * this.zoom) * dpr;
        const top = (origin.top + quad.top * this.zoom - this.scrollTop) * dpr;
        ctx.fillRect(
          left,
          top,
          (quad.right - quad.left) * this.zoom * dpr,
          (quad.bottom - quad.top) * this.zoom * dpr,
        );
      }
    }
  }

  // --- Find in document ----------------------------------------------------

  private searchStatus(): SearchStatus {
    return {
      query: this.searcher.query,
      options: this.searchOptions,
      total: this.searcher.matches.length,
      index: this.currentMatch < 0 ? 0 : this.currentMatch + 1,
      scanned: this.searcher.scanned,
      toScan: this.searcher.toScan,
      // The viewer's own scope, not the scan's copy of it. They agree the
      // moment there is a query --- `search` hands one to the other in the same
      // synchronous step --- and disagree exactly when there is not: a scope
      // taken with an empty find bar is remembered here and never reaches the
      // scan, so the scan's copy said "not scoped" while the button that sets
      // it toggles on {@link searchScoped}. Pressing it then rendered off and
      // behaved on, which is the one state a toggle must not have.
      scoped: this.searchScope !== null,
      running: this.searcher.running,
      textless: this.searcher.textless,
      unsearchablePages: this.searcher.unsearchablePages,
      mappingKnown: this.searcher.mappingKnown,
      problem: this.searcher.problem,
    };
  }

  /**
   * Starts a scan for `query`, replacing any scan in progress.
   *
   * It begins at the page being read and wraps, so the first hit a reader is
   * shown is the next one after where they are rather than the first one in the
   * document --- which on a 775-page manual is the difference between a useful
   * search and a jump to the beginning.
   */
  search(query: string): void {
    this.currentMatch = -1;
    void this.searcher.run(
      query,
      this.currentPage(),
      this.searchOptions,
      this.searchScope,
    );
    this.wake();
  }

  /**
   * Confines the search to what is selected, and rescans.
   *
   * Returns whether there was a selection to confine it to. The scope is a
   * **snapshot** taken here, not a live reading --- see `SearchScope`, which has
   * why: clicking on the page clears the selection, so a live scope would
   * quietly become the whole document while the find bar still said otherwise.
   *
   * The selection itself is left alone. A reader who scoped a search is looking
   * at the range they drew, and clearing it would take away the only thing on
   * screen saying what the search is confined to.
   */
  scopeSearchToSelection(): boolean {
    const selection = this.selection;
    if (!selection) return false;
    const scope: SearchScope = [];
    for (const page of selection.pages()) {
      const range = selection.rangeOn(page);
      if (range) scope.push({ page, from: range.from, to: range.to });
    }
    if (scope.length === 0) return false;
    this.searchScope = scope;
    if (this.searcher.query) this.search(this.searcher.query);
    else this.wake();
    return true;
  }

  /** Lets the search see the whole document again, and rescans. */
  clearSearchScope(): void {
    if (!this.searchScope) return;
    this.searchScope = null;
    if (this.searcher.query) this.search(this.searcher.query);
    else this.wake();
  }

  /** Whether the search is confined to a selection. */
  get searchScoped(): boolean {
    return this.searchScope !== null;
  }

  /**
   * The ranges the search is confined to, or null.
   *
   * For `viewercheck.ts`, and it earns the accessor: a check that derives the
   * scope's bounds from the matches it got back cannot fail, because a filter
   * that stopped clipping widens the bounds it is measured against in the same
   * step. Two mutations survived exactly that way.
   */
  get searchScopeRanges(): readonly ScopeRange[] | null {
    return this.searchScope;
  }

  /**
   * Changes how the query is matched, rescanning if there is one.
   *
   * A toggle with no query is remembered rather than refused --- the next search
   * uses it --- but nothing is asked of the backend, because there is nothing to
   * ask about and a scan of 775 pages for `""` is not free just because it finds
   * nothing.
   */
  setSearchOptions(options: SearchOptions): void {
    if (sameOptions(options, this.searchOptions)) return;
    this.searchOptions = options;
    if (this.searcher.query) this.search(this.searcher.query);
    else this.wake();
  }

  /** How a query is currently matched. */
  get searchOptionsNow(): SearchOptions {
    return this.searchOptions;
  }

  /** Drops the query and its results. */
  clearSearch(): void {
    this.currentMatch = -1;
    // The scope goes with the query. It was drawn to narrow *this* search, and
    // leaving it behind would silently narrow the next one --- with the find
    // bar empty, there would be nothing on screen saying so.
    this.searchScope = null;
    this.searcher.clear();
    this.wake();
  }

  /** Moves to the next match, wrapping at the end. */
  nextMatch(): void {
    void this.goToMatch(this.currentMatch + 1);
  }

  /** Moves to the previous match, wrapping at the start. */
  prevMatch(): void {
    void this.goToMatch(this.currentMatch - 1);
  }

  /**
   * Moves to a particular match, by its index in {@link searchMatches}.
   *
   * What the results panel calls. Out-of-range wraps rather than being refused,
   * which is the same rule the two steppers get and matters here for the same
   * reason: the list a row was clicked in can be one reply older than the list
   * the index is resolved against.
   */
  showMatch(index: number): void {
    void this.goToMatch(index);
  }

  /** Index of the current match in {@link searchMatches}, or -1. */
  get matchIndex(): number {
    return this.currentMatch;
  }

  /** Matches found so far. For the check harness. */
  get searchMatches(): readonly Match[] {
    return this.searcher.matches;
  }

  /** Whether a scan is still running. For the check harness. */
  get searching(): boolean {
    return this.searcher.running;
  }

  /** Wall time of the last completed scan, in ms. For the check harness. */
  get searchElapsedMs(): number {
    return this.searcher.elapsedMs;
  }

  /**
   * Called for every page the scan finishes.
   *
   * The first match found is jumped to, and only the first: a scan that kept
   * moving the viewport as it went would drag a reader through the document
   * while they were trying to read the hit they were already shown.
   */
  private onSearchProgress(): void {
    if (this.currentMatch < 0 && this.searcher.matches.length > 0) {
      void this.goToMatch(0);
    }
    this.wake();
  }

  /**
   * Scrolls the match at `index` into view, wrapping the index into range.
   *
   * The page's text has to be loaded to know where on the page the match is,
   * and on a page the reader has never visited it will not be --- so this is
   * async, and the scroll happens when the answer arrives rather than being
   * approximated from the page top.
   */
  private async goToMatch(index: number): Promise<void> {
    const count = this.searcher.matches.length;
    if (count === 0) return;

    const wrapped = ((index % count) + count) % count;
    const match = this.searcher.matches[wrapped];
    if (!match) return;
    this.currentMatch = wrapped;
    this.wake();

    const text = await this.text.load(match.page);
    // The load outlives a document being closed --- it is an IPC round trip and
    // nothing withdraws it --- so the scroll below would run against a torn-down
    // scroller. See `life`.
    if (!text || this.life.ended) return;
    const [first] = runsFor(text, match.start, match.end);
    const top =
      this.scroller.pageTopOf(match.page) + (first ? first.top * this.zoom : 0);
    // A third down rather than at the top edge: a hit flush against the top of
    // the viewport has no context above it and reads as though the line before
    // it is missing.
    this.scrollTo(top - this.viewportSize().height / 3);
    this.wake();
  }

  /**
   * Paints every match on a visible page, and the current one differently.
   *
   * A linear scan of every match each frame, which on a common word in a long
   * document is tens of thousands of comparisons --- cheap enough to measure as
   * nothing next to the rest of a frame, and the alternative is a page-keyed
   * index that has to be kept in step with a list that is still growing.
   */
  private paintMatches(ctx: CanvasRenderingContext2D, dpr: number): void {
    const matches = this.searcher.matches;
    if (matches.length === 0) return;

    const visible = new Set(this.scroller.visiblePages());
    for (let index = 0; index < matches.length; index++) {
      const match = matches[index];
      if (!match) continue;
      ctx.fillStyle =
        index === this.currentMatch ? CURRENT_MATCH_FILL : MATCH_FILL;

      // Two halves when the hit runs over a page break, and each is painted by
      // the page it belongs to --- there is no shared coordinate space between
      // two pages, so one rectangle cannot span them. `Infinity` is the first
      // half's end because it runs to wherever that page's text stops, which
      // this does not have to know and `runsFor` clamps.
      const halves: { page: number; from: number; to: number }[] =
        match.endPage === undefined
          ? [{ page: match.page, from: match.start, to: match.end }]
          : [
              { page: match.page, from: match.start, to: Infinity },
              { page: match.endPage, from: 0, to: match.end },
            ];

      for (const half of halves) {
        if (!visible.has(half.page)) continue;
        // Not requested if missing: `prefetchText` already asks for every
        // visible page, and asking again from the paint path would queue an
        // extraction per frame for a page whose reply has not landed yet.
        const text = this.text.peek(half.page);
        if (!text) continue;

        const origin = this.scroller.pageOrigin(half.page);
        for (const quad of runsFor(text, half.from, half.to)) {
          ctx.fillRect(
            (origin.left + quad.left * this.zoom) * dpr,
            (origin.top + quad.top * this.zoom - this.scrollTop) * dpr,
            (quad.right - quad.left) * this.zoom * dpr,
            (quad.bottom - quad.top) * this.zoom * dpr,
          );
        }
      }
    }
  }

  /**
   * Keeps the screen-reader text in step with what is on screen.
   *
   * Runs every frame and is nearly free when nothing changed --- a page already
   * present is not touched, which is the property that keeps a reading cursor
   * alive across a scroll. See `a11y.ts`.
   */
  private syncAccessibleText(): void {
    // Asked for here because a screen-reader user may never search, and is the
    // reader least able to tell that what they are being read is nonsense.
    //
    // This runs every frame and is therefore what *actually* triggers the fetch
    // for every document, ahead of the search path that was written first. At
    // most one fetch per document however often this runs, and it is off the
    // startup path -- which is the only place its measured 0.1--11.9 ms would
    // matter, since warm startup has ~25 ms of margin against its target.
    this.searcher.ensureMapping();
    this.a11y.sync(
      this.scroller.visiblePages(),
      (page) => this.text.peek(page),
      (page) => this.searcher.unreadablePage(page),
    );
    this.a11y.announce(this.currentPage());
  }

  /** The screen-reader text layer. For the check harness. */
  get accessibleText(): AccessibleText {
    return this.a11y;
  }

  /** Asks for a page's text once, waking the loop when it lands. */
  private requestText(page: number): void {
    if (this.textAsked.has(page)) return;
    this.textAsked.add(page);
    void this.text.load(page).then(() => this.wake());
  }

  /**
   * Loads the text of every visible page, so a click can land immediately.
   *
   * Extraction measured 1.4 ms on a dense page, and it shares the render thread
   * with tiles --- so this deliberately runs from the frame loop rather than
   * eagerly at open: on a 775-page document, asking for all of it up front would
   * put a minute of extraction in front of the first tile.
   */
  private prefetchText(): void {
    for (const page of this.scroller.visiblePages()) this.requestText(page);
  }

  /** Geometry of the scrollbar thumb, in CSS pixels within the track. */
  private thumbRect(): { top: number; height: number } {
    const trackHeight = this.root.clientHeight;
    const { maxScroll } = this.scroller;
    const documentHeight = this.scroller.documentHeight;
    const height = Math.max(
      MIN_THUMB,
      Math.min(
        trackHeight,
        (this.viewportSize().height / documentHeight) * trackHeight,
      ),
    );
    const travel = trackHeight - height;
    const top = maxScroll > 0 ? (this.scrollTop / maxScroll) * travel : 0;
    return { top, height };
  }

  private paintThumb(): void {
    const { top, height } = this.thumbRect();
    this.thumb.style.top = `${top}px`;
    this.thumb.style.height = `${height}px`;
    this.track.style.visibility =
      this.scroller.maxScroll > 0 ? "visible" : "hidden";
  }

  private readonly onTrackPointerDown = (event: PointerEvent): void => {
    const trackTop = this.track.getBoundingClientRect().top;
    const y = event.clientY - trackTop;
    const { top, height } = this.thumbRect();

    // Grab the thumb where it was clicked; click the bare track and it centres
    // there, which is what every native scrollbar on this platform does.
    this.dragOffset = y >= top && y <= top + height ? y - top : height / 2;
    capture(this.track, event.pointerId);
    this.track.addEventListener("pointermove", this.onTrackPointerMove);
    this.track.addEventListener("pointerup", this.onTrackPointerUp);
    this.dragTo(y);
    event.preventDefault();
  };

  private readonly onTrackPointerMove = (event: PointerEvent): void => {
    this.dragTo(event.clientY - this.track.getBoundingClientRect().top);
  };

  private readonly onTrackPointerUp = (event: PointerEvent): void => {
    this.dragOffset = null;
    release(this.track, event.pointerId);
    this.track.removeEventListener("pointermove", this.onTrackPointerMove);
    this.track.removeEventListener("pointerup", this.onTrackPointerUp);
  };

  private dragTo(y: number): void {
    if (this.dragOffset === null) return;
    const { height } = this.thumbRect();
    const travel = this.root.clientHeight - height;
    if (travel <= 0) return;
    const fraction = Math.max(0, Math.min(1, (y - this.dragOffset) / travel));
    this.scrollTo(fraction * this.scroller.maxScroll);
  }
}
