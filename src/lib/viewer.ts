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
import { DESTINATION_MARGIN_PT } from "./outline";
import { Scroller, type PageSize } from "./scroller";
import { Search, type Match } from "./search";
import { Selection } from "./selection";
import { caretAt, runsFor, TextCache, type Caret, type PageText } from "./text";

/** What the search box should be showing. */
export interface SearchStatus {
  /** The query being scanned for, or "". */
  query: string;
  /** Matches found so far. */
  total: number;
  /** One-based position of the current match, or 0 when there is none. */
  index: number;
  /** Pages scanned so far, out of the document's page count. */
  scanned: number;
  /** Whether a scan is still running. */
  running: boolean;
  /**
   * Whether every page scanned so far had no extractable text.
   *
   * "No matches" and "nothing to match against" are different answers and are
   * reported as different: a scan of a scanned document never tested the query.
   */
  textless: boolean;
}

/** What the surface is currently showing, for a status line to render. */
export interface ViewerStatus {
  /** One-based, as a reader counts. */
  page: number;
  pageCount: number;
  /** CSS pixels per PDF point. */
  zoom: number;
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
  /** Characters currently selected, so a status line can say so. */
  selected: number;
  /** State of the find-in-document scan. */
  search: SearchStatus;
}

export interface ViewerOptions {
  doc: number;
  pageCount: number;
  /** Geometry of page 1, taken as representative --- see `scroller.ts`. */
  page: PageSize;
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
}

/**
 * Zoom stops, in CSS pixels per PDF point.
 *
 * A ladder rather than a continuous zoom because every step throws away every
 * tier-2 tile: on the A0 sheet each one costs about a second to replace, so a
 * zoom bound to a trackpad's pinch resolution would queue work faster than the
 * renderer can retire it and never converge on anything.
 */
const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3, 4, 6, 8];

/** Width of the scrollbar gutter, in CSS pixels. */
const SCROLLBAR_WIDTH = 12;

/** Shortest the scrollbar thumb may get on a long document. */
const MIN_THUMB = 24;

/** CSS pixels a line-mode wheel notch scrolls. */
const LINE_HEIGHT = 40;

/** CSS pixels an arrow key scrolls. */
const ARROW_STEP = 60;

/** Fraction of a screen that Page Up/Down moves, leaving context behind. */
const PAGE_OVERLAP = 0.9;

/** Margin either side of the page in fit-width, in CSS pixels. */
const FIT_MARGIN = 24;

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
   * Index into `searcher.matches` of the match the viewport is on, or -1.
   *
   * Kept here rather than in {@link Search}, which only accumulates: which hit
   * a reader is looking at is a property of the surface, and the scan has to be
   * able to finish without moving anyone.
   */
  private currentMatch = -1;

  private selection: Selection | null = null;
  /** Whether a pointer is currently extending the selection. */
  private selecting = false;
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
   * Whether the zoom is still tracking the window width.
   *
   * Fit-width has to survive a resize, and the first resize is not optional:
   * the constructor runs before the layout that gives `root` a width, so the
   * zoom it computes is against a viewport of one pixel. Anchoring the initial
   * fit to a flag rather than to a measurement means the correction arrives
   * through the same path every later resize uses.
   */
  private fitting = true;

  private frameHandle = 0;
  private running = false;
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
    this.searcher = new Search(opts.doc, opts.pageCount, () => this.onSearchProgress());

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

    this.zoom = this.fitWidthZoom(this.viewportSize().width);
    this.scroller = new Scroller(this.surfaceHost, {
      doc: opts.doc,
      pageCount: opts.pageCount,
      page: opts.page,
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
    this.stop();
    this.a11y.destroy();
    this.searcher.cancel();
    this.observer.disconnect();
    this.root.removeEventListener("wheel", this.onWheel);
    this.root.removeEventListener("keydown", this.onKeyDown);
    this.root.removeEventListener("pointerdown", this.onSelectStart);
    this.track.removeEventListener("pointerdown", this.onTrackPointerDown);
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
   * should not have to know whether the loop happens to be awake.
   */
  wake(): void {
    this.tail = 2;
    if (this.running) return;
    this.running = true;
    this.frameHandle = requestAnimationFrame(this.tick);
  }

  private stop(): void {
    if (!this.running) return;
    this.running = false;
    cancelAnimationFrame(this.frameHandle);
  }

  private readonly tick = (): void => {
    const stats = this.scroller.frame(this.scrollTop);
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
      return;
    }
    this.frameHandle = requestAnimationFrame(this.tick);
  };

  /**
   * The page a reader would say they are on.
   *
   * Taken at the middle of the viewport rather than its top edge, so the number
   * changes when most of the screen changes rather than as soon as the previous
   * page's last line scrolls out.
   */
  private currentPage(): number {
    return this.scroller.pageAt(this.scrollTop + this.viewportSize().height / 2);
  }

  /** Emits a status only when something a reader could notice has changed. */
  private report(stats: { sharp: number; any: number }): void {
    const status: ViewerStatus = {
      page: this.currentPage() + 1,
      pageCount: this.opts.pageCount,
      zoom: this.zoom,
      turns: this.turns,
      invert: this.invert,
      sharp: stats.sharp,
      any: stats.any,
      pending: this.scroller.pendingWork,
      selected: this.selectedCount(),
      search: this.searchStatus(),
    };
    const summary = [
      status.page,
      status.zoom,
      status.turns,
      status.invert,
      Math.round(status.sharp * 100),
      Math.round(status.any * 100),
      status.pending > 0,
      status.selected,
      status.search.query,
      status.search.total,
      status.search.index,
      status.search.scanned,
      status.search.running,
      status.search.textless,
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

  /** The page's size in points as displayed, i.e. after the view rotation. */
  private displayedPage(): { width_pt: number; height_pt: number } {
    const { page } = this.opts;
    return this.turns % 2 === 0
      ? page
      : { width_pt: page.height_pt, height_pt: page.width_pt };
  }

  /** The zoom at which the page fills the viewport's width. */
  private fitWidthZoom(width: number): number {
    return Math.max(0.05, (width - FIT_MARGIN * 2) / this.displayedPage().width_pt);
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
    const next = Math.max(0.05, Math.min(zoom, 16));
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
    const next =
      direction > 0
        ? ZOOM_STEPS.find((z) => z > this.zoom + 1e-6)
        : [...ZOOM_STEPS].reverse().find((z) => z < this.zoom - 1e-6);
    if (next === undefined) return;
    this.fitting = false;
    this.setZoom(next);
  }

  /** Sets the zoom so the page fills the viewport's width, and keeps it there. */
  fitWidth(): void {
    this.fitting = true;
    this.setZoom(this.fitWidthZoom(this.viewportSize().width));
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

  /** A page's size on screen, as the scroller laid it out. */
  get pageBoxCss(): { width: number; height: number } {
    return this.scroller.pageBoxCss;
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
    const next = ((this.turns + delta) % 4 + 4) % 4;
    if (next === this.turns) return;

    const page = this.currentPage();
    const before = this.scroller.pagePitchCss;
    const through =
      before > 0 ? (this.scrollTop - this.scroller.pageTopOf(page)) / before : 0;

    this.applyTurns(next);
    // A rotation changes the page's aspect, so a view that was fitted to the
    // width is no longer fitted to anything. Refitting is what makes the
    // command feel like turning a sheet of paper rather than cropping one.
    if (this.fitting) {
      this.zoom = this.fitWidthZoom(this.viewportSize().width);
      this.scroller.setZoom(this.zoom);
    }

    this.scrollTop = Math.max(
      0,
      Math.min(
        this.scroller.pageTopOf(page) + through * this.scroller.pagePitchCss,
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

  /** Whether the zoom is following the window width rather than a fixed stop. */
  get isFitting(): boolean {
    return this.fitting;
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
    fitting: boolean;
    turns: number;
  }): void {
    this.applyTurns(((place.turns % 4) + 4) % 4);

    this.fitting = place.fitting;
    if (place.fitting) {
      this.zoom = this.fitWidthZoom(this.viewportSize().width);
      this.scroller.setZoom(this.zoom);
    } else {
      this.setZoom(place.zoom);
    }

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
    if (this.fitting) this.setZoom(this.fitWidthZoom(viewport.width));
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
      this.fitting = false;
      this.setZoom(this.zoom * Math.exp(-event.deltaY / 300));
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

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    const screen = this.viewportSize().height * PAGE_OVERLAP;
    const accel = event.metaKey || event.ctrlKey;

    if (accel && (event.key === "+" || event.key === "=")) {
      this.zoomStep(1);
    } else if (accel && event.key === "-") {
      this.zoomStep(-1);
    } else if (accel && event.key === "0") {
      this.fitWidth();
    } else if (accel && (event.key === "r" || event.key === "R")) {
      // Preview's bindings. Acrobat puts these on Shift-Cmd-+/-, which on this
      // keyboard is the same `key` as Cmd-+ and would collide with zoom.
      this.rotateBy(1);
    } else if (accel && (event.key === "l" || event.key === "L")) {
      this.rotateBy(-1);
    } else if (event.key === "ArrowDown") {
      this.scrollBy(ARROW_STEP);
    } else if (event.key === "ArrowUp") {
      this.scrollBy(-ARROW_STEP);
    } else if (event.key === "PageDown" || event.key === " ") {
      this.scrollBy(event.shiftKey ? -screen : screen);
    } else if (event.key === "PageUp") {
      this.scrollBy(-screen);
    } else if (event.key === "Home") {
      this.goToStart();
    } else if (event.key === "End") {
      this.goToEnd();
    } else if (accel && event.key === "c") {
      void this.copySelection();
    } else if (accel && event.key === "a") {
      this.selectPage();
    } else if (accel && (event.key === "g" || event.key === "G")) {
      // Cmd-G and Cmd-Shift-G, which is what find-next is on this platform.
      // `key` carries the shifted form, so both spellings have to be listed.
      if (event.shiftKey) this.prevMatch();
      else this.nextMatch();
    } else if (event.key === "Escape") {
      this.clearSelection();
    } else if (event.key === "n") {
      this.nextPage();
    } else if (event.key === "p") {
      this.previousPage();
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

  private caretFrom(event: PointerEvent): Caret | null {
    const bounds = this.root.getBoundingClientRect();
    const docY = event.clientY - bounds.top + this.scrollTop;
    const page = this.scroller.pageAt(docY);

    const text = this.text.peek(page);
    if (!text) {
      this.requestText(page);
      return null;
    }

    const origin = this.scroller.pageOrigin(page);
    const x = (event.clientX - bounds.left - origin.left) / this.zoom;
    const y = (docY - origin.top) / this.zoom;
    return { page, index: caretAt(text, x, y) };
  }

  private readonly onSelectStart = (event: PointerEvent): void => {
    // Only the primary button starts a selection; a right-click will open a
    // context menu once there is one, and should not clear what is selected.
    if (event.button !== 0) return;
    // The scrollbar is inside the root and has its own drag.
    if (this.track.contains(event.target as Node)) return;
    this.root.focus();

    const caret = this.caretFrom(event);
    this.selection = caret ? new Selection(caret) : null;
    this.selecting = caret !== null;

    capture(this.root, event.pointerId);
    this.root.addEventListener("pointermove", this.onSelectMove);
    this.root.addEventListener("pointerup", this.onSelectEnd);
    event.preventDefault();
    this.wake();
  };

  private readonly onSelectMove = (event: PointerEvent): void => {
    if (!this.selecting || !this.selection) return;
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

  /** Selects every character of the page currently being read. */
  selectPage(): void {
    const page = this.currentPage();
    const text = this.text.peek(page);
    if (!text) {
      void this.text.load(page).then(() => this.selectPage());
      return;
    }
    this.selection = new Selection({ page, index: 0 });
    this.selection.focus = { page, index: text.codes.length };
    this.wake();
  }

  /**
   * Puts the selected text on the clipboard.
   *
   * Resolves to what was copied, or `null` if there was nothing. It waits for
   * any page whose text has not arrived: a selection dragged quickly across a
   * page boundary can reach the clipboard before the extraction does, and
   * silently copying the part that happened to be loaded is the kind of bug a
   * user discovers in someone else's document.
   */
  async copySelection(): Promise<string | null> {
    const selection = this.selection;
    if (!selection) return null;

    if (!selection.isComplete(this.text)) {
      await Promise.all(selection.pages().map((page) => this.text.load(page)));
    }
    const text = selection.text(this.text);
    if (!text) return null;

    await navigator.clipboard.writeText(text);
    return text;
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
      total: this.searcher.matches.length,
      index: this.currentMatch < 0 ? 0 : this.currentMatch + 1,
      scanned: this.searcher.scanned,
      running: this.searcher.running,
      textless: this.searcher.textless,
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
    void this.searcher.run(query, this.currentPage());
    this.wake();
  }

  /** Drops the query and its results. */
  clearSearch(): void {
    this.currentMatch = -1;
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
    if (!text) return;
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
      if (!match || !visible.has(match.page)) continue;
      // Not requested if missing: `prefetchText` already asks for every visible
      // page, and asking again from the paint path would queue an extraction
      // per frame for a page whose reply has not landed yet.
      const text = this.text.peek(match.page);
      if (!text) continue;

      ctx.fillStyle =
        index === this.currentMatch ? CURRENT_MATCH_FILL : MATCH_FILL;
      const origin = this.scroller.pageOrigin(match.page);
      for (const quad of runsFor(text, match.start, match.end)) {
        ctx.fillRect(
          (origin.left + quad.left * this.zoom) * dpr,
          (origin.top + quad.top * this.zoom - this.scrollTop) * dpr,
          (quad.right - quad.left) * this.zoom * dpr,
          (quad.bottom - quad.top) * this.zoom * dpr,
        );
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
    this.a11y.sync(this.scroller.visiblePages(), (page) => this.text.peek(page));
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
