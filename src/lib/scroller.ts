/**
 * A windowed page scroller, built to be measured (spike 0.8).
 *
 * This is the design in docs/PLAN.md section 4 reduced to the parts that can
 * affect a frame: a two-tier cache, a tile window with one screen of prefetch,
 * a client-side supersedable queue, and an LRU bound on how many tiles are
 * resident. It is not the viewer --- there is no selection, no accessibility
 * tree, no zoom animation --- because the question it exists to answer is
 * whether the webview can composite this at the display's cadence, and every
 * one of those would only add cost to the thing being measured.
 *
 * Two layouts are implemented rather than one, because who does the
 * compositing is the actual open question:
 *
 *  - `tiles`: a canvas per tile, positioned in a natively scrolling container.
 *    Pixels are drawn once, on arrival; scrolling is then the compositor's
 *    problem and our per-frame cost is only the window bookkeeping.
 *  - `viewport`: a single viewport-sized canvas, redrawn from cached
 *    ImageBitmaps every frame. Our per-frame cost is a drawImage per visible
 *    tile; the compositor sees one layer that never moves.
 *
 * The first is what a virtual scroller normally is. The second is the first
 * step of section 4's "if the webview is not fast enough" escalation, and
 * measuring it now costs one afternoon rather than one architecture.
 *
 * Page geometry is taken from page 1 and assumed uniform. That is the same
 * assumption the lazy-geometry startup path makes (section 4), and it holds on
 * both corpora here; a mixed-size document would need the correcting estimate
 * described there, which is a scrollbar problem rather than a frame-rate one.
 */

import { cancelTile, fetchTile, nextRequestId } from "./tiles";

export type Layout = "tiles" | "viewport";

export interface PageSize {
  width_pt: number;
  height_pt: number;
}

export interface ScrollerOptions {
  doc: number;
  pageCount: number;
  /** Geometry of page 1, taken as representative of the document. */
  page: PageSize;
  /** CSS pixels per PDF point. 1.0 is "100%" for this spike's purposes. */
  zoom: number;
  layout: Layout;
  /** Tile edge in device pixels. Section 4 measured 1024-2048 as the range. */
  tilePx: number;
  /** Device pixels per CSS pixel. */
  dpr: number;
  /** Viewport size in CSS pixels. */
  viewport: { width: number; height: number };
  /** Screens of prefetch above and below the viewport. */
  prefetchScreens: number;
  /** Resident tile budget, shared by both layouts so they compare fairly. */
  cacheTiles: number;
  /** Concurrent tile requests. The client's half of a supersedable queue. */
  maxInFlight: number;
  /**
   * Whether a request that stops being wanted is withdrawn from the renderer.
   *
   * A variant dimension rather than a constant, so the behaviour it is supposed
   * to fix can be measured beside it in the same run. With this off the client
   * still drops stale tiles on arrival --- it just pays for them first, which is
   * what spike 0.8 measured as 60 fps over an empty screen.
   */
  cancel: boolean;
}

/** What one frame of scrolling cost and how much of it was covered. */
export interface FrameStats {
  /** Tiles drawn into a canvas during this frame. */
  drawn: number;
  /**
   * Fraction of the visible page area covered by a sharp tier-2 tile. A
   * scroller that never stutters because it never paints anything would pass a
   * pure frame-rate test; this is the number that catches it.
   */
  sharp: number;
  /**
   * Fraction covered by anything at all, tier 1 included.
   *
   * Section 4 promises the user "sees a blurry page sharpen, never a white
   * rectangle". That is a claim about this number rather than about the one
   * above, and on a page whose tier-1 placeholder itself costs 1.5 s it is the
   * claim in doubt.
   */
  any: number;
}

/** Totals for a whole run. */
export interface RunStats {
  requested: number;
  delivered: number;
  /** Arrived after leaving the window, i.e. work the queue should have cut. */
  discarded: number;
  /**
   * Withdrawn before the render finished, i.e. work the queue did cut.
   *
   * The counterpart of `discarded`, and the number that says whether
   * cancellation is doing anything: a run where this stays zero while
   * `discarded` climbs is one where every stale tile was still paid for.
   */
  abandoned: number;
  bytes: number;
  /** Time inside Pdfium, summed over delivered tiles. */
  renderMs: number;
  /** Client-side decode into an ImageBitmap, summed over delivered tiles. */
  decodeMs: number;
  evicted: number;
}

/** Device-pixel width of a tier-1 page placeholder. */
const TIER1_WIDTH = 150;

/** CSS pixels between pages. */
const PAGE_GAP = 16;

interface TileKey {
  page: number;
  col: number;
  row: number;
}

interface TileRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface TileEntry {
  key: TileKey;
  /** Present in the `tiles` layout: the pixels live here. */
  canvas: HTMLCanvasElement | null;
  /** Present in the `viewport` layout: redrawn from here every frame. */
  bitmap: ImageBitmap | null;
  /** Device-pixel rect within the scaled page. */
  rect: TileRect;
}

/** A request that has been issued and has not settled. */
interface Outstanding {
  /** Server-side id, by which the request can be withdrawn. */
  rid: number;
  /**
   * Whether this is still worth paying for, tested against the window as it is
   * now rather than as it was when the request went out. A closure because the
   * two kinds of request answer it differently.
   */
  isWanted: () => boolean;
  /** Tier-1 placeholders outlive a generation change; tier-2 tiles do not. */
  survivesClear: boolean;
  /** Whether a withdrawal has already been sent, so it is sent only once. */
  withdrawn: boolean;
}

/** A tile that has landed but has not been drawn into anything yet. */
interface Arrival {
  key: TileKey;
  rect: TileRect;
  bitmap: ImageBitmap;
  generation: number;
}

function keyOf(k: TileKey): string {
  return `${k.page}:${k.col}:${k.row}`;
}

export class Scroller {
  private readonly opts: ScrollerOptions;
  private readonly host: HTMLElement;

  /** `tiles` layout: the scrolling container and the full-height spacer. */
  private container: HTMLDivElement | null = null;
  private spacer: HTMLDivElement | null = null;
  /** `viewport` layout: the single canvas and its context. */
  private surface: HTMLCanvasElement | null = null;
  private surfaceCtx: CanvasRenderingContext2D | null = null;

  /** Tier 2, LRU by insertion order: re-inserting on use moves an entry last. */
  private readonly tiles = new Map<string, TileEntry>();
  /** Tier 1, permanent for the session, keyed by page index. */
  private readonly placeholders = new Map<number, ImageBitmap>();
  /** Tier-1 canvases mounted in the `tiles` layout, so they can be recycled. */
  private readonly placeholderCanvases = new Map<number, HTMLCanvasElement>();

  /** Requests issued and not yet settled, by id. */
  private readonly inFlight = new Map<string, Outstanding>();
  /**
   * Tiles that have landed since the last frame.
   *
   * Drawing them is deferred to the next frame deliberately. A fetch settles in
   * a microtask, and pixel work done there would land between two frames where
   * the harness's callback timer cannot see it --- the frame interval would
   * still lengthen, so the drop would be counted, but it would be attributed to
   * the compositor rather than to us. Draining here keeps every pixel we are
   * responsible for inside the interval that is being attributed.
   */
  private readonly arrived: Arrival[] = [];
  private readonly arrivedPlaceholders: {
    page: number;
    bitmap: ImageBitmap;
  }[] = [];

  private scrollTop = 0;
  private drawnThisFrame = 0;
  /**
   * Bumped when tier 2 is cleared between rounds.
   *
   * A tile requested by the previous round can land during this one. It is not
   * a superseded tile --- nothing about the scroll invalidated it --- so
   * counting it as one would report a queue failure that did not happen, and
   * adopting it would give a round free pixels it never paid for. Dropped
   * silently instead.
   */
  private generation = 0;

  readonly stats: RunStats = {
    requested: 0,
    delivered: 0,
    discarded: 0,
    abandoned: 0,
    bytes: 0,
    renderMs: 0,
    decodeMs: 0,
    evicted: 0,
  };

  // Page geometry, in device pixels and in CSS pixels. Both are derived from
  // the device-pixel size so the two can never disagree by a rounding step.
  private readonly pageWidthDev: number;
  private readonly pageHeightDev: number;
  private readonly pageWidthCss: number;
  private readonly pageHeightCss: number;
  private readonly cols: number;
  private readonly rows: number;

  constructor(host: HTMLElement, opts: ScrollerOptions) {
    this.host = host;
    this.opts = opts;

    this.pageWidthDev = Math.round(opts.page.width_pt * opts.zoom * opts.dpr);
    this.pageHeightDev = Math.round(opts.page.height_pt * opts.zoom * opts.dpr);
    this.pageWidthCss = this.pageWidthDev / opts.dpr;
    this.pageHeightCss = this.pageHeightDev / opts.dpr;
    this.cols = Math.ceil(this.pageWidthDev / opts.tilePx);
    this.rows = Math.ceil(this.pageHeightDev / opts.tilePx);

    this.mount();
  }

  /** Total scrollable height in CSS pixels. */
  get documentHeight(): number {
    return this.opts.pageCount * (this.pageHeightCss + PAGE_GAP) - PAGE_GAP;
  }

  /** The furthest the viewport can be scrolled. */
  get maxScroll(): number {
    return Math.max(0, this.documentHeight - this.opts.viewport.height);
  }

  /**
   * Requests issued and not yet settled.
   *
   * Exposed so a harness can wait for a variant's work to leave the renderer
   * before the next one starts. The render service is one shared FIFO, so a
   * round that begins while the previous variant's backlog is still draining
   * measures the two of them together --- which reverses the result when the
   * variants are swapped, and did.
   */
  get outstanding(): number {
    return this.inFlight.size;
  }

  /** Tiles per page, so a run can report its working set. */
  get tilesPerPage(): number {
    return this.cols * this.rows;
  }

  /**
   * Advances the scroll position and does one frame of work.
   *
   * Everything that touches the DOM happens here rather than in a fetch
   * callback, so the whole per-frame cost of the design is inside the interval
   * the harness times. A tile that arrives between frames is drawn at the start
   * of the next one.
   */
  frame(scrollTop: number): FrameStats {
    this.scrollTop = Math.max(0, Math.min(scrollTop, this.maxScroll));
    this.drawnThisFrame = 0;

    if (this.container) this.container.scrollTop = this.scrollTop;

    this.drain();
    this.request();
    if (this.opts.layout === "viewport") this.paintSurface();
    this.evict();

    return { drawn: this.drawnThisFrame, ...this.coverage() };
  }

  /**
   * Drops every tier-2 tile, keeping tier 1.
   *
   * Called between rounds so each round scrolls over content that has not been
   * rendered yet, which is the case the criterion is about. Tier 1 survives
   * because section 4 makes it permanent for the session --- clearing it would
   * measure a document being opened, not a document being scrolled.
   */
  clearTiles(): void {
    for (const entry of this.tiles.values()) this.release(entry);
    this.tiles.clear();
    for (const arrival of this.arrived.splice(0)) arrival.bitmap.close();
    this.generation++;

    // No tier-2 request survives a generation change --- `drain` drops every
    // one of them on arrival --- so let the renderer stop paying for them.
    // Without this the next round starts against a renderer still working
    // through the previous one's queue, which is the shape of the bug that made
    // this harness report 60 fps over an empty screen.
    this.withdraw((outstanding) => !outstanding.survivesClear);
  }

  /** Zeroes the run counters, so a round reports only its own work. */
  resetStats(): void {
    this.stats.requested = 0;
    this.stats.delivered = 0;
    this.stats.discarded = 0;
    this.stats.abandoned = 0;
    this.stats.bytes = 0;
    this.stats.renderMs = 0;
    this.stats.decodeMs = 0;
    this.stats.evicted = 0;
  }

  destroy(): void {
    this.clearTiles();
    for (const bitmap of this.placeholders.values()) bitmap.close();
    this.placeholders.clear();
    this.placeholderCanvases.clear();
    this.host.replaceChildren();
  }

  /**
   * Takes ownership of everything that landed since the last frame.
   *
   * The relevance test happens here rather than on arrival, against the window
   * as it is now: a tile that was wanted when it landed and is not wanted a
   * frame later should still be dropped rather than drawn.
   */
  private drain(): void {
    for (const arrival of this.arrived.splice(0)) {
      if (arrival.generation !== this.generation) {
        arrival.bitmap.close();
        continue;
      }
      if (!this.isWanted(arrival.key)) {
        this.stats.discarded++;
        arrival.bitmap.close();
        continue;
      }
      this.stats.delivered++;
      this.adopt(arrival);
    }

    for (const { page, bitmap } of this.arrivedPlaceholders.splice(0)) {
      this.placeholders.set(page, bitmap);
      if (this.opts.layout === "tiles") this.mountPlaceholder(page, bitmap);
    }
  }

  private mount(): void {
    const { viewport, dpr, layout } = this.opts;

    this.host.replaceChildren();
    this.host.style.width = `${viewport.width}px`;
    this.host.style.height = `${viewport.height}px`;
    this.host.style.position = "relative";
    this.host.style.overflow = "hidden";
    this.host.style.background = "#666";

    if (layout === "tiles") {
      const container = document.createElement("div");
      container.style.cssText =
        `position:absolute;inset:0;overflow-y:scroll;overflow-x:hidden;` +
        // The scroller drives scrollTop itself; a smooth-scroll behaviour or a
        // rubber band would put the container's idea of the offset out of step
        // with the window that was computed from it.
        `scroll-behavior:auto;overscroll-behavior:none;`;
      const spacer = document.createElement("div");
      spacer.style.cssText = `position:relative;width:100%;height:${this.documentHeight}px;`;
      container.appendChild(spacer);
      this.host.appendChild(container);
      this.container = container;
      this.spacer = spacer;
      return;
    }

    const surface = document.createElement("canvas");
    surface.width = Math.round(viewport.width * dpr);
    surface.height = Math.round(viewport.height * dpr);
    surface.style.cssText = `position:absolute;inset:0;width:${viewport.width}px;height:${viewport.height}px;`;
    this.host.appendChild(surface);
    this.surface = surface;
    // `alpha: false` lets the compositor skip blending a layer that covers its
    // own bounds completely, which is what this one does.
    this.surfaceCtx = surface.getContext("2d", { alpha: false });
  }

  /** CSS-pixel top of a page in the scrolled document. */
  private pageTop(page: number): number {
    return page * (this.pageHeightCss + PAGE_GAP);
  }

  /** Pages intersecting a CSS-pixel band, clamped to the document. */
  private pagesIn(top: number, bottom: number): number[] {
    const pitch = this.pageHeightCss + PAGE_GAP;
    const first = Math.max(0, Math.floor(top / pitch));
    const last = Math.min(this.opts.pageCount - 1, Math.floor(bottom / pitch));
    const pages: number[] = [];
    for (let page = first; page <= last; page++) pages.push(page);
    return pages;
  }

  /** The band the window covers: the viewport plus its prefetch margins. */
  private band(): { top: number; bottom: number } {
    const { viewport, prefetchScreens } = this.opts;
    const margin = viewport.height * prefetchScreens;
    return {
      top: this.scrollTop - margin,
      bottom: this.scrollTop + viewport.height + margin,
    };
  }

  /** Device-pixel rect of one tile within its scaled page. */
  private tileRect(
    col: number,
    row: number,
  ): { x: number; y: number; width: number; height: number } {
    const x = col * this.opts.tilePx;
    const y = row * this.opts.tilePx;
    return {
      x,
      y,
      // Clamped at the page edge: a tile hanging off the page would carry
      // several megabytes of white and cost the same to move as real content.
      width: Math.min(this.opts.tilePx, this.pageWidthDev - x),
      height: Math.min(this.opts.tilePx, this.pageHeightDev - y),
    };
  }

  /**
   * Issues the tiles the window wants, nearest to the viewport centre first.
   *
   * Ordering is the client's half of section 4's supersedable queue. The render
   * service is one FIFO thread, so anything already sent cannot be recalled ---
   * what a short in-flight limit buys is that requests for tiles the viewport
   * has already left are still in *our* list when they stop being wanted, and
   * are simply never sent.
   */
  private request(): void {
    this.withdraw((outstanding) => !outstanding.isWanted());

    const { top, bottom } = this.band();
    const centre = this.scrollTop + this.opts.viewport.height / 2;

    const wanted: { key: TileKey; distance: number }[] = [];

    for (const page of this.pagesIn(top, bottom)) {
      this.requestPlaceholder(page);

      const pageTop = this.pageTop(page);
      for (let row = 0; row < this.rows; row++) {
        const tileTop = pageTop + (row * this.opts.tilePx) / this.opts.dpr;
        const tileBottom = tileTop + this.opts.tilePx / this.opts.dpr;
        if (tileBottom < top || tileTop > bottom) continue;

        for (let col = 0; col < this.cols; col++) {
          const key: TileKey = { page, col, row };
          const id = keyOf(key);
          if (this.tiles.has(id) || this.inFlight.has(id)) continue;
          if (!this.isWanted(key)) continue;
          wanted.push({
            key,
            distance: Math.abs((tileTop + tileBottom) / 2 - centre),
          });
        }
      }
    }

    wanted.sort((a, b) => a.distance - b.distance);

    for (const { key } of wanted) {
      if (this.inFlight.size >= this.opts.maxInFlight) break;
      this.send(key);
    }
  }

  /**
   * Withdraws every outstanding request matching `predicate`.
   *
   * The entry stays in `inFlight` until its reply lands. A withdrawal is a
   * request to stop, not proof of having stopped --- it can lose the race with a
   * tile that was already finishing --- so forgetting it here would let
   * `request` issue a duplicate for a tile that is still on its way. The
   * `withdrawn` flag is what keeps this from re-sending the same withdrawal on
   * every frame until the reply arrives.
   */
  private withdraw(predicate: (outstanding: Outstanding) => boolean): void {
    if (!this.opts.cancel) return;
    for (const outstanding of this.inFlight.values()) {
      if (outstanding.withdrawn || !predicate(outstanding)) continue;
      outstanding.withdrawn = true;
      cancelTile(outstanding.rid);
    }
  }

  private send(key: TileKey): void {
    const id = keyOf(key);
    const rect = this.tileRect(key.col, key.row);
    const generation = this.generation;
    const rid = nextRequestId();
    this.inFlight.set(id, {
      rid,
      isWanted: () => this.generation === generation && this.isWanted(key),
      survivesClear: false,
      withdrawn: false,
    });
    this.stats.requested++;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page: key.page,
      scale: this.opts.zoom * this.opts.dpr,
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      format: "raw",
    })
      .then((result) => {
        this.inFlight.delete(id);
        // Withdrawn in time: the renderer stopped, and there is nothing to
        // count as delivered or as discarded because nothing was produced.
        if (!result) {
          this.stats.abandoned++;
          return;
        }
        this.stats.bytes += result.bytes;
        this.stats.renderMs += result.renderUs / 1000;
        this.stats.decodeMs += result.decodeMs;
        this.arrived.push({ key, rect, bitmap: result.bitmap, generation });
      })
      .catch(() => {
        this.inFlight.delete(id);
      });
  }

  /**
   * Whether a tile is still inside the current window, horizontally as well as
   * vertically.
   *
   * At 400% the page is twice the width of the viewport, so a purely vertical
   * window would request --- and wait for --- three columns nobody can see. It
   * is prefetch in the one direction this scroller cannot move, which is to say
   * it is waste, and it lands in the same FIFO as the tiles that are visible.
   */
  private isWanted(key: TileKey): boolean {
    const { top, bottom } = this.band();
    const rect = this.tileRect(key.col, key.row);

    const tileTop = this.pageTop(key.page) + rect.y / this.opts.dpr;
    const tileBottom = tileTop + rect.height / this.opts.dpr;
    if (tileBottom < top || tileTop > bottom) return false;

    const tileLeft = this.pageLeftCss() + rect.x / this.opts.dpr;
    const tileRight = tileLeft + rect.width / this.opts.dpr;
    return tileRight >= 0 && tileLeft <= this.opts.viewport.width;
  }

  /** Takes ownership of an arrived tile, in whichever form the layout needs. */
  private adopt({ key, rect, bitmap }: Arrival): void {
    const entry: TileEntry = { key, canvas: null, bitmap: null, rect };

    if (this.opts.layout === "tiles") {
      const canvas = document.createElement("canvas");
      canvas.width = rect.width;
      canvas.height = rect.height;
      canvas.style.cssText =
        `position:absolute;` +
        `left:${this.pageLeftCss() + rect.x / this.opts.dpr}px;` +
        `top:${this.pageTop(key.page) + rect.y / this.opts.dpr}px;` +
        `width:${rect.width / this.opts.dpr}px;height:${rect.height / this.opts.dpr}px;`;
      const ctx = canvas.getContext("2d", { alpha: false });
      ctx?.drawImage(bitmap, 0, 0);
      // The canvas holds the pixels now; keeping the bitmap as well would
      // double the resident cost of every tile for nothing.
      bitmap.close();
      this.spacer?.appendChild(canvas);
      entry.canvas = canvas;
      this.drawnThisFrame++;
    } else {
      entry.bitmap = bitmap;
    }

    this.tiles.set(keyOf(key), entry);
  }

  /** CSS-pixel left edge of a page, centred in the viewport. */
  private pageLeftCss(): number {
    return Math.max(0, (this.opts.viewport.width - this.pageWidthCss) / 2);
  }

  /**
   * Requests a page's tier-1 placeholder once.
   *
   * Section 4 records that this is not free on a hard page --- a 150 px render
   * of the A0 sheet costs 1.5 s --- so it goes through the same queue as
   * everything else and the scroller shows nothing until it lands. The
   * alternative, blocking on it, would hide exactly that cost.
   */
  private requestPlaceholder(page: number): void {
    const id = `p${page}`;
    if (this.placeholders.has(page) || this.inFlight.has(id)) return;

    const scale = TIER1_WIDTH / this.opts.page.width_pt;
    const height = Math.round(this.opts.page.height_pt * scale);
    const rid = nextRequestId();
    // Withdrawable like any other request, and for the same reason: a
    // placeholder is permanent once it lands, but a page that has left the band
    // is not one the renderer should be spending 1.5 s on while the visible
    // page waits behind it in the queue. It is re-requested if the page comes
    // back.
    this.inFlight.set(id, {
      rid,
      isWanted: () => this.pageInBand(page),
      survivesClear: true,
      withdrawn: false,
    });
    this.stats.requested++;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page,
      scale,
      x: 0,
      y: 0,
      width: TIER1_WIDTH,
      height,
      format: "raw",
    })
      .then((result) => {
        this.inFlight.delete(id);
        if (!result) {
          this.stats.abandoned++;
          return;
        }
        this.stats.delivered++;
        this.stats.bytes += result.bytes;
        this.stats.renderMs += result.renderUs / 1000;
        this.stats.decodeMs += result.decodeMs;
        this.arrivedPlaceholders.push({ page, bitmap: result.bitmap });
      })
      .catch(() => {
        this.inFlight.delete(id);
      });
  }

  /** Whether any part of a page lies in the current band. */
  private pageInBand(page: number): boolean {
    const { top, bottom } = this.band();
    const pageTop = this.pageTop(page);
    return pageTop + this.pageHeightCss >= top && pageTop <= bottom;
  }

  /**
   * Mounts a tier-1 bitmap as a page-sized but low-resolution canvas.
   *
   * The backing store stays 150 px wide and CSS stretches it, so the blurry
   * page underneath the sharp tiles costs 90 KB rather than the 32 MB a
   * page-resolution canvas would take at 400%.
   */
  private mountPlaceholder(page: number, bitmap: ImageBitmap): void {
    if (this.placeholderCanvases.has(page)) return;
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.style.cssText =
      `position:absolute;z-index:0;` +
      `left:${this.pageLeftCss()}px;top:${this.pageTop(page)}px;` +
      `width:${this.pageWidthCss}px;height:${this.pageHeightCss}px;`;
    canvas.getContext("2d", { alpha: false })?.drawImage(bitmap, 0, 0);
    this.spacer?.appendChild(canvas);
    this.placeholderCanvases.set(page, canvas);
  }

  /** Redraws the whole viewport from cache. Only the `viewport` layout. */
  private paintSurface(): void {
    const ctx = this.surfaceCtx;
    const surface = this.surface;
    if (!ctx || !surface) return;

    const { dpr, viewport } = this.opts;
    ctx.fillStyle = "#666";
    ctx.fillRect(0, 0, surface.width, surface.height);

    const left = this.pageLeftCss();

    for (const page of this.pagesIn(
      this.scrollTop,
      this.scrollTop + viewport.height,
    )) {
      const top = this.pageTop(page) - this.scrollTop;

      const placeholder = this.placeholders.get(page);
      if (placeholder) {
        ctx.drawImage(
          placeholder,
          left * dpr,
          top * dpr,
          this.pageWidthCss * dpr,
          this.pageHeightCss * dpr,
        );
        this.drawnThisFrame++;
      }

      for (let row = 0; row < this.rows; row++) {
        for (let col = 0; col < this.cols; col++) {
          const entry = this.tiles.get(keyOf({ page, col, row }));
          if (!entry?.bitmap) continue;
          ctx.drawImage(
            entry.bitmap,
            left * dpr + entry.rect.x,
            top * dpr + entry.rect.y,
          );
          this.drawnThisFrame++;
        }
      }
    }
  }

  /**
   * Fractions of the strictly visible page area that are sharp, and that have
   * anything at all.
   *
   * Only page area counts. The gaps between pages are not content and a
   * scroller cannot be blamed for leaving them grey, so including them would
   * quietly credit every variant for the same empty pixels.
   */
  coverage(): { sharp: number; any: number } {
    const { viewport } = this.opts;
    const top = this.scrollTop;
    const bottom = top + viewport.height;

    let visible = 0;
    let sharp = 0;
    let any = 0;

    for (const page of this.pagesIn(top, bottom)) {
      const pageTop = this.pageTop(page);
      const hasPlaceholder = this.placeholders.has(page);

      for (let row = 0; row < this.rows; row++) {
        const rect = this.tileRect(0, row);
        const tileTop = pageTop + rect.y / this.opts.dpr;
        const tileBottom = tileTop + rect.height / this.opts.dpr;
        const overlap = Math.min(bottom, tileBottom) - Math.max(top, tileTop);
        if (overlap <= 0) continue;

        for (let col = 0; col < this.cols; col++) {
          const columnRect = this.tileRect(col, row);
          const tileLeft = this.pageLeftCss() + columnRect.x / this.opts.dpr;
          const tileRight = tileLeft + columnRect.width / this.opts.dpr;
          // Horizontal intersection too: at 400% half the page hangs off the
          // side of the viewport, and counting it would charge the scroller
          // for pixels nobody is looking at.
          const across =
            Math.min(viewport.width, tileRight) - Math.max(0, tileLeft);
          if (across <= 0) continue;

          const area = overlap * across;
          const isSharp = this.tiles.has(keyOf({ page, col, row }));
          visible += area;
          if (isSharp) sharp += area;
          if (isSharp || hasPlaceholder) any += area;
        }
      }
    }

    if (visible <= 0) return { sharp: 1, any: 1 };
    return { sharp: sharp / visible, any: any / visible };
  }

  /**
   * Drops oldest-first down to the tile budget, never dropping one the window
   * still wants.
   *
   * Oldest-first is the right order here only because the scroll is monotonic,
   * which makes insertion order the same as distance behind the viewport. The
   * skip is what stops a budget smaller than the working set from evicting a
   * tile and immediately re-requesting it --- which would not read as a bug,
   * just as an inexplicably busy renderer.
   */
  private evict(): void {
    if (this.tiles.size <= this.opts.cacheTiles) return;
    for (const [id, entry] of this.tiles) {
      if (this.tiles.size <= this.opts.cacheTiles) break;
      if (this.isWanted(entry.key)) continue;
      this.release(entry);
      this.tiles.delete(id);
      this.stats.evicted++;
    }
  }

  private release(entry: TileEntry): void {
    entry.bitmap?.close();
    entry.canvas?.remove();
  }
}
