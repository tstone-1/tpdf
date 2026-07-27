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

import { Scroller, type PageSize } from "./scroller";
import { Selection } from "./selection";
import { caretAt, TextCache, type Caret } from "./text";

/** What the surface is currently showing, for a status line to render. */
export interface ViewerStatus {
  /** One-based, as a reader counts. */
  page: number;
  pageCount: number;
  /** CSS pixels per PDF point. */
  zoom: number;
  /** Fraction of the visible page area backed by a sharp tile. */
  sharp: number;
  /** Fraction backed by anything at all, tier-1 placeholder included. */
  any: number;
  /** Requests outstanding, so "still working" can be distinguished from "done". */
  pending: number;
  /** Characters currently selected, so a status line can say so. */
  selected: number;
}

export interface ViewerOptions {
  doc: number;
  pageCount: number;
  /** Geometry of page 1, taken as representative --- see `scroller.ts`. */
  page: PageSize;
  /** Tile edge in device pixels. Section 4 measured 1024--2048 as the range. */
  tilePx?: number;
  onStatus?: (status: ViewerStatus) => void;
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

    this.surfaceHost = document.createElement("div");
    this.surfaceHost.style.cssText = "position:absolute;left:0;top:0;";
    root.appendChild(this.surfaceHost);

    // Above the tiles and transparent to the pointer. A separate layer rather
    // than a pass inside the scroller, so the class that owns the tile cache
    // does not also have to know what a selection is.
    this.overlay = document.createElement("canvas");
    this.overlay.style.cssText =
      "position:absolute;left:0;top:0;pointer-events:none;";
    root.appendChild(this.overlay);
    this.overlayCtx = this.overlay.getContext("2d");

    this.track = document.createElement("div");
    this.track.style.cssText =
      `position:absolute;top:0;right:0;bottom:0;width:${SCROLLBAR_WIDTH}px;` +
      `background:rgba(0,0,0,0.12);`;
    this.thumb = document.createElement("div");
    this.thumb.style.cssText =
      `position:absolute;left:2px;right:2px;top:0;height:0;border-radius:4px;` +
      `background:rgba(0,0,0,0.38);`;
    this.track.appendChild(this.thumb);
    root.appendChild(this.track);

    this.zoom = this.fitWidthZoom(this.viewportSize().width);
    this.scroller = new Scroller(this.surfaceHost, {
      doc: opts.doc,
      pageCount: opts.pageCount,
      page: opts.page,
      zoom: this.zoom,
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
    this.paintSelection();
    this.paintThumb();
    this.report(stats);

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
      sharp: stats.sharp,
      any: stats.any,
      pending: this.scroller.pendingWork,
      selected: this.selectedCount(),
    };
    const summary = [
      status.page,
      status.zoom,
      Math.round(status.sharp * 100),
      Math.round(status.any * 100),
      status.pending > 0,
      status.selected,
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

  /** The zoom at which the page fills the viewport's width. */
  private fitWidthZoom(width: number): number {
    return Math.max(0.05, (width - FIT_MARGIN * 2) / this.opts.page.width_pt);
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

  /** Scrolls so page `page` (zero-based) starts at the top of the viewport. */
  goToPage(page: number): void {
    const clamped = Math.max(0, Math.min(page, this.opts.pageCount - 1));
    this.scrollTo(this.scroller.pageTopOf(clamped));
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
    const page = this.currentPage();
    const accel = event.metaKey || event.ctrlKey;

    if (accel && (event.key === "+" || event.key === "=")) {
      this.zoomStep(1);
    } else if (accel && event.key === "-") {
      this.zoomStep(-1);
    } else if (accel && event.key === "0") {
      this.fitWidth();
    } else if (event.key === "ArrowDown") {
      this.scrollBy(ARROW_STEP);
    } else if (event.key === "ArrowUp") {
      this.scrollBy(-ARROW_STEP);
    } else if (event.key === "PageDown" || event.key === " ") {
      this.scrollBy(event.shiftKey ? -screen : screen);
    } else if (event.key === "PageUp") {
      this.scrollBy(-screen);
    } else if (event.key === "Home") {
      this.scrollTo(0);
    } else if (event.key === "End") {
      this.scrollTo(this.scroller.maxScroll);
    } else if (accel && event.key === "c") {
      void this.copySelection();
    } else if (accel && event.key === "a") {
      this.selectPage();
    } else if (event.key === "Escape") {
      this.clearSelection();
    } else if (event.key === "n") {
      this.goToPage(page + 1);
    } else if (event.key === "p") {
      this.goToPage(page - 1);
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

  private paintSelection(): void {
    const ctx = this.overlayCtx;
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);
    if (!this.selection) return;

    // Multiply keeps the glyphs legible underneath, which a flat fill over the
    // tile would not: the text is already painted into the pixels.
    ctx.globalCompositeOperation = "multiply";
    ctx.fillStyle = "rgba(80, 140, 255, 0.35)";

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
    ctx.globalCompositeOperation = "source-over";
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
