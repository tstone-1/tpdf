/**
 * A windowed page scroller, built to be measured (spike 0.8).
 *
 * This is the design in docs/PLAN.md section 4 reduced to the parts that can
 * affect a frame: a two-tier cache, a tile window with one screen of prefetch,
 * a client-side supersedable queue, and an LRU bound on how many tiles are
 * resident. It is still not the whole viewer --- there is no selection and no
 * accessibility tree --- because the question it was written to answer is
 * whether the webview can composite this at the display's cadence, and every
 * one of those would only add cost to the thing being measured.
 *
 * It has two callers now, and the split is deliberate. This class knows a
 * scroll offset and a frame; `viewer.ts` knows where the offset comes from and
 * whether a frame is worth running. That is what lets the benchmark drive it at
 * a fixed cadence over a fixed distance while a reader drives it from a
 * trackpad, without either one being a special case inside the other.
 *
 * Two layouts are implemented rather than one, because who does the
 * compositing was the actual open question. It has an answer --- `viewport`,
 * measured in section 4 --- and `tiles` is kept because it is the control that
 * answer was measured against:
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
 * ## Page geometry is per page, estimated where it is not yet known
 *
 * This class held **one** `PageSize` until 2026-08-02, taken from page 1 and
 * multiplied by the page index. The cost of that was larger than a scrollbar:
 * `computeGeometry` derived `rows` and `cols` from that one size, and those
 * decide which tiles are ever *requested*, so an A3 insert in an A4 document was
 * asked for only as far as A4 reached and drawn cropped, silently and with no
 * error anywhere. Every page after a differing one sat somewhere it is not.
 * Content truncation and a drifting position, not cosmetics --- and the comment
 * that used to stand here called it a scrollbar problem, which was true of the
 * benchmark it was written for and false of the shipped viewer.
 *
 * What replaces it is section 4's table: {@link sizes} holds one entry per page,
 * `null` where the size is not known yet, and {@link boxes} is the layout derived
 * from it with each page's `top` accumulated from the heights *before* it rather
 * than assumed. Nothing here multiplies.
 *
 * The table starts almost empty on purpose. Collecting every page's size at open
 * costs 86 ms on the 775-page corpus (section 4's startup measurement), so the
 * backend sends page 1 alone unless `TPDF_EAGER_GEOMETRY` is set --- which is why
 * an estimate is needed at all rather than being a concession. Unknown pages are
 * laid out at the running mean of the sizes that *are* known, which is page 1's
 * size until a second one arrives and is exact immediately on the overwhelming
 * majority of documents, where every page is the same. {@link notePageSize} is
 * how a real size arrives; `viewer.ts` feeds it from the text extractions it
 * already performs for every visible page, so the correction rides an IPC round
 * trip that was happening anyway.
 *
 * Correcting a page invalidates **that page** rather than the document. Each page
 * carries its own {@link epochs} counter, bumped when its box moves, and a reply
 * naming a stale epoch is dropped on arrival exactly as a stale generation is.
 * A single global counter would have been fewer lines and would repaint the whole
 * screen every time a page's size was learned --- once per page, on a document
 * being read straight through.
 *
 * `testdata/make_mixed_pdf.py` generates the document that can tell any of this
 * apart; every other fixture in the corpus is uniform, which is why no check
 * could go red on it before that file existed.
 */

import { Backoff } from "./backoff";
import type { PageSize } from "./ipc";
import { Lifetime } from "./lifetime";
import { cancelTile, fetchTile, nextRequestId } from "./tiles";
// From `tilestatus`, never from `./tiles`: four test files replace that
// module wholesale, and a class reached through it would be `undefined`
// inside them --- turning this `instanceof` into a TypeError thrown from a
// failure handler, which surfaces as a frame loop that never settles.
import { unedited, type PageView } from "./pages";
import { DocumentGone } from "./tilestatus";

export type Layout = "tiles" | "viewport";

// Re-exported rather than moved out of the callers' reach: `viewer.ts` and
// `thumbnails.ts` take their page geometry from the scroller alongside
// `displayedSize`, and the type they mean is the backend's, not one of ours.
// The declaration lives in `ipc.ts` because `render.rs` owns it.
export type { PageSize };

export interface ScrollerOptions {
  doc: number;
  pageCount: number;
  /**
   * The working document's pages, in slot order.
   *
   * What makes slot `i` something other than page `i` of the file: a deleted
   * page leaves the order, and every slot after it draws a page whose number in
   * the file is one higher than its position here. The tile requests below are
   * the reason this class needs it at all --- their `page` is a page of the
   * *file*, and asking for the slot would ask for the wrong picture.
   *
   * Optional, defaulting to a document nobody has edited, because the benchmark
   * harnesses open a file and drive a viewport and never touch a page. That
   * default is the identity mapping, and it is the truth for those callers
   * rather than a fallback.
   */
  order?: PageView[];
  /**
   * Geometry of the pages whose size is known, index-aligned from page 1.
   *
   * Shorter than `pageCount` whenever the open was lazy, which is the default:
   * `render.rs` sends `[page 1]` alone unless `TPDF_EAGER_GEOMETRY` is set. The
   * pages past the end of this array are laid out at an estimate until
   * {@link Scroller.notePageSize} learns them.
   *
   * A non-empty tuple rather than an array, so "a document laid out from no
   * size at all" is not a state this class has to have an answer for. There is
   * no honest fallback --- inventing Letter would put every page of a document
   * somewhere it is not --- and the callers all have page 1 in hand.
   */
  pages: [PageSize, ...PageSize[]];
  /** CSS pixels per PDF point. 1.0 is "100%" for this spike's purposes. */
  zoom: number;
  /**
   * Quarter-turns clockwise the view is rotated by, 0 to 3.
   *
   * A property of the view, never of the document: rotating changes what the
   * renderer is asked for and how the page is laid out, and writes nothing.
   */
  turns: number;
  /**
   * Whether the page's lightness is inverted, for reading in the dark.
   *
   * A property of the view like `turns`, and carried in the tile request for the
   * same reason: the renderer produces the pixels that are shown, so what is on
   * screen is something a check can read rather than infer from a style.
   */
  invert: boolean;
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
  /**
   * Called once when the document's file is found to have been truncated.
   *
   * Separate from any per-tile failure reporting, because it is not a tile that
   * failed: the file the whole document was mapped from has lost the bytes those
   * pages were in, no further request can succeed, and the reader needs telling
   * rather than a blank region that quietly never fills.
   *
   * Optional so the benchmark harnesses, which drive a scroller with no UI to
   * report into, do not each need a stub.
   */
  onGone?: (message: string) => void;
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
  /**
   * Requests that came back an error.
   *
   * Counted rather than discarded. Every `catch` in this file used to swallow
   * the failure whole, which left a viewer whose renderer was erroring on every
   * request indistinguishable --- to the status line, to the benchmark and to
   * anyone reading it --- from one that was merely slow.
   */
  failed: number;
  bytes: number;
  /** Time inside Pdfium, summed over delivered tiles. */
  renderMs: number;
  /** Client-side decode into an ImageBitmap, summed over delivered tiles. */
  decodeMs: number;
  evicted: number;
}

/**
 * Device-pixel width of a tier-1 page placeholder.
 *
 * Exported because the page strip renders at exactly this width: §4 says the
 * placeholder "doubles as the thumbnail", and it only does while the two are
 * the same bitmap. A strip that picked its own width would render every page a
 * second time.
 */
export const TIER1_WIDTH = 150;

/** CSS pixels between pages. */
const PAGE_GAP = 16;

/**
 * A page's size as displayed under `turns` quarter-turns clockwise.
 *
 * One implementation because three places need it and each had grown its own: a
 * scroller laying pages out, a viewer computing fit-width, and the page strip
 * sizing its rows. A quarter-turn swap is one line, which is exactly why it gets
 * copied --- and three copies are three chances for one of them to disagree
 * about a rotated page. The symptoms are not obviously the same bug: rows the
 * wrong shape in the strip, or a fit-width that fits the other axis.
 *
 * Reduced modulo four twice, which normalises a negative turn --- `rotateBy(-1)`
 * reaches here. Worth being precise that this is not what makes negatives work:
 * JavaScript's remainder keeps the sign, so the parity test below is right
 * either way and dropping one reduction changes nothing. It is kept because it
 * is the form the strip already used and because a reader of `((t % 4) + 4) % 4`
 * does not have to reason about the sign at all.
 */
export function displayedSize(page: PageSize, turns: number): PageSize {
  return (((turns % 4) + 4) % 4) % 2 === 0
    ? page
    : { width_pt: page.height_pt, height_pt: page.width_pt };
}

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

/**
 * One page's layout, in device pixels, CSS pixels and document coordinates.
 *
 * Both pixel spaces are derived from the device-pixel size so the two can never
 * disagree by a rounding step, and `top` is accumulated over the pages before
 * this one rather than multiplied --- which is the whole of what a mixed-size
 * document needs and the whole of what the single-`PageSize` layout could not do.
 */
interface PageBox {
  widthDev: number;
  heightDev: number;
  widthCss: number;
  heightCss: number;
  cols: number;
  rows: number;
  /** CSS-pixel top of the page in the scrolled document. */
  top: number;
}

/** A request that has been issued and has not settled. */
interface Outstanding {
  /** Server-side id, by which the request can be withdrawn. */
  rid: number;
  /**
   * The page it is for, so a size correction can withdraw that page's work
   * without touching the rest of the document's.
   */
  page: number;
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
  /**
   * The page's layout epoch when the request went out.
   *
   * Separate from `generation`, and for the same reason `placeholderGeneration`
   * is separate from it: a size correction invalidates one page's pixels and
   * nothing else's, and a counter that could only say "everything" would repaint
   * the whole screen once per page on a document being read through.
   */
  epoch: number;
}

function keyOf(k: TileKey): string {
  return `${k.page}:${k.col}:${k.row}`;
}

/** Fallback surround, used only if the stylesheet has not loaded. */
const SURROUND_FALLBACK = "#666";

/**
 * The colour around the page, read from the theme.
 *
 * Resolved rather than fixed because the `viewport` layout fills the whole
 * canvas with it, and a canvas takes a colour, not a custom property --- so the
 * one place it is actually visible is the one place CSS cannot reach.
 */
function surroundColour(): string {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue("--tpdf-surround")
    .trim();
  return value || SURROUND_FALLBACK;
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

  /** The colour behind and between pages, following the system theme. */
  private surround = surroundColour();

  /**
   * Re-reads the surround when the desktop switches between light and dark.
   *
   * A listener rather than reading it per frame: `getComputedStyle` forces style
   * resolution, and the frame loop is the one place in this file that has a
   * budget. Held as a field so `destroy` can take it off again --- a scroller
   * outlives no document, but the listener would outlive the scroller.
   */
  private readonly onScheme = () => {
    this.surround = surroundColour();
    if (this.opts.layout === "tiles") {
      this.host.style.background = this.surround;
    }
  };

  private readonly scheme = window.matchMedia("(prefers-color-scheme: dark)");

  /** Tier 2, LRU by insertion order: re-inserting on use moves an entry last. */
  private readonly tiles = new Map<string, TileEntry>();
  /** Tier 1, permanent for the session, keyed by page index. */
  private readonly placeholders = new Map<number, ImageBitmap>();
  /** Tier-1 canvases mounted in the `tiles` layout, so they can be recycled. */
  private readonly placeholderCanvases = new Map<number, HTMLCanvasElement>();

  /** Requests issued and not yet settled, by id. */
  private readonly inFlight = new Map<string, Outstanding>();
  /**
   * Requests that failed, and the earliest they may be issued again.
   *
   * A separate class rather than a map here, because the semantics are what
   * matter and none of them was reachable from a test while they lived in a
   * private field of a class that needs a webview to exist. See `backoff.ts`,
   * which carries the reasoning and the failure it was written for.
   */
  private readonly backoff = new Backoff();
  /**
   * Whether the document's file was truncated on disk under this scroller.
   *
   * A latch, and one that is never cleared: it is set from a 410, which the
   * backend serves only after it has stopped being able to build a worker that
   * could answer. Reopening the file is what recovers, and that builds a new
   * scroller.
   *
   * It gates `send` rather than the frame loop, so what is already painted stays
   * painted. That is not politeness --- those tiles are the last true picture of
   * the document there will be, and clearing them would replace something
   * correct with a blank page.
   */
  private gone = false;
  /**
   * Whether this scroller is still alive, for the tile arrivals to consult.
   *
   * `destroy` withdraws everything outstanding, and a withdrawal that lands in
   * time returns nothing --- but withdrawal races the renderer, so a tile that
   * had already finished still arrives afterwards with its bitmap. Queued into
   * `arrived` by a continuation that cannot tell, it is never drained, because
   * the frame loop it was waiting for is gone. See `lifetime.ts`.
   */
  private readonly life = new Lifetime();
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
    generation: number;
    /** The page's layout epoch when it was asked for. See {@link Arrival}. */
    epoch: number;
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
  /**
   * Bumped when tier 1 is dropped, which only a rotation does.
   *
   * Separate from `generation` because the two invalidate opposite halves:
   * tier-1 requests are precisely the ones that survive a tier-2 clear, so a
   * single counter could not express "keep the placeholders, drop the tiles"
   * and "drop the placeholders" at once. Without it, a placeholder render that
   * was already finishing when the view turned is adopted sideways --- and
   * stays, because tier 1 is permanent.
   */
  private placeholderGeneration = 0;

  readonly stats: RunStats = {
    requested: 0,
    delivered: 0,
    discarded: 0,
    abandoned: 0,
    failed: 0,
    bytes: 0,
    renderMs: 0,
    decodeMs: 0,
    evicted: 0,
  };

  /**
   * Each page's size in points, or `null` where it is not known yet.
   *
   * Seeded from `opts.pages`, which on a lazy open is page 1 alone. Written by
   * {@link notePageSize} as the viewer learns real sizes.
   */
  private sizes: (PageSize | null)[];
  /**
   * What an unknown page is laid out at: the mean of the sizes that are known.
   *
   * A mean rather than page 1's size, which is what section 4 asks for --- "the
   * scroller estimates total height from the pages it has loaded". With one
   * known page the two are the same thing, so nothing is lost on the uniform
   * case; with several they are not, and the mean is the better guess about the
   * pages still unseen.
   */
  private estimate: PageSize;
  /** The layout, one entry per page. Rebuilt by {@link computeGeometry}. */
  private boxes: PageBox[] = [];
  /** Total scrollable height in CSS pixels, accumulated with the boxes. */
  private totalHeight = 0;
  /**
   * Per-page invalidation counters, bumped when a page's box moves.
   *
   * A request carries the epoch it was issued under and its reply is dropped if
   * that has moved on --- the per-page analogue of {@link generation}, and
   * per-page precisely because a size correction is per-page. See the header.
   */
  private epochs: number[];

  /**
   * Quarter-turns clockwise a page is turned by **on top of the view**, 0 to 3.
   *
   * A property of the *document being edited*, which is what separates it from
   * {@link ScrollerOptions.turns}: rotating the view writes nothing and is one
   * number for the whole document, and this one is per page and is what a save
   * puts in the file. They compose, and everything downstream wants the sum ---
   * see {@link effectiveTurns}, which is the only place the two are added.
   *
   * Not in `ScrollerOptions` because it is not an option: a scroller is
   * constructed once per document and this changes while the reader is looking
   * at it.
   */
  private pageTurns: number[];

  /**
   * Which page of the file each slot draws, and under which identity.
   *
   * Held rather than derived because both directions are needed: the source, on
   * every tile request, and the identity, when the order changes --- a page's
   * learned size and its tile epoch have to travel with the *page*, and the only
   * thing that says where a page went is its id.
   */
  private order: PageView[];

  constructor(host: HTMLElement, opts: ScrollerOptions) {
    this.host = host;
    this.opts = opts;

    this.order = opts.order ?? [...unedited(opts.pageCount).pages];
    this.sizes = new Array<PageSize | null>(Math.max(0, opts.pageCount)).fill(
      null,
    );
    this.pageTurns = this.order.map((page) => page.turns % 4);
    for (let page = 0; page < this.sizes.length; page++) {
      this.sizes[page] = opts.pages[page] ?? null;
    }
    this.epochs = new Array<number>(this.sizes.length).fill(0);
    // Before `computeGeometry`, which reads it for every page whose size the
    // open did not carry.
    this.estimate = this.meanKnownSize();

    this.computeGeometry();
    this.mount();
    this.scheme.addEventListener("change", this.onScheme);
  }

  /**
   * The canvas the `viewport` layout composites into, or `null` for `tiles`.
   *
   * Exposed so a check can read what was actually drawn. Everything else about
   * the page is either a request the renderer answered or a style, and neither
   * is evidence about the screen: a check on a style is the style agreeing with
   * itself, which is the exact failure the whole inversion path is arranged to
   * avoid.
   */
  get compositedSurface(): HTMLCanvasElement | null {
    return this.surface;
  }

  /**
   * A page's size in points, real if it is known and the estimate if it is not.
   *
   * The one place the two are conflated, deliberately: everything that lays out
   * or requests wants "how big is this page", and a caller that had to ask
   * whether the answer was real would be a caller that could forget to.
   * {@link knowsPageSize} is there for the one consumer --- a check --- whose
   * question genuinely is which of the two it got.
   */
  pageSize(page: number): PageSize {
    return this.sizes[page] ?? this.estimate;
  }

  /** Whether a page's size is the document's own rather than the estimate. */
  knowsPageSize(page: number): boolean {
    return this.sizes[page] != null;
  }

  /**
   * Quarter-turns a page is drawn by: the view's, plus the page's own edit.
   *
   * The one place the two are added. Everything that asks a renderer for pixels
   * or lays a box out has to agree about this sum, and three of them had grown
   * their own copy of the view's number --- which is the shape `displayedSize`
   * already exists to stop.
   *
   * Not reduced modulo four here. Every consumer either normalises (the tile
   * request must, since the server refuses a fifth turn rather than reducing it)
   * or does not care ({@link displayedSize} tests the parity), and reducing in
   * two places is how one of them comes to disagree.
   */
  private effectiveTurns(page: number): number {
    return this.opts.turns + (this.pageTurns[page] ?? 0);
  }

  /** Quarter-turns a page is drawn by, normalised to 0..3. For a tile request. */
  private requestTurns(page: number): number {
    const turns = this.effectiveTurns(page);
    return ((turns % 4) + 4) % 4;
  }

  /**
   * A page's size in points as displayed, i.e. after every rotation in force.
   *
   * Both of them: the view's and the page's own. A page turned by an edit is a
   * different shape in the layout, and a `displayedSize` that saw only the view
   * would lay a turned page out in its old box and then paint a tile of the new
   * one into it.
   */
  private displayedPageSize(page: number): PageSize {
    return displayedSize(this.pageSize(page), this.effectiveTurns(page));
  }

  /** Quarter-turns an edit has applied to a page, 0 to 3. */
  pageExtraTurns(page: number): number {
    return this.pageTurns[page] ?? 0;
  }

  /**
   * Records that an edit turned a page, relaying out and redrawing it.
   *
   * Returns whether the layout moved, which is the caller's cue to re-anchor the
   * reader --- the same contract {@link notePageSize} has, and for the same
   * reason: a page that changed shape changed the length of the document above
   * everything below it.
   *
   * **The page is invalidated whatever the turn was**, before the geometry is
   * even consulted. A half turn leaves the box exactly as it was, so the box
   * comparison inside {@link applySizes} sees nothing move --- and the pixels
   * are upside down. Letting the geometry decide what to invalidate is right
   * for a size correction, where a box that did not move really does still hold
   * the right picture, and wrong here.
   */
  setPageTurns(page: number, turns: number): boolean {
    if (page < 0 || page >= this.pageTurns.length) return false;
    const next = ((turns % 4) + 4) % 4;
    if (this.pageTurns[page] === next) return false;
    this.pageTurns[page] = next;
    this.invalidatePage(page);
    const moved = this.applySizes();
    // `applySizes` relays out only when something moved. A half turn moves
    // nothing and still has to be drawn again, and the tiles to draw it with
    // were just discarded.
    if (!moved) this.relayout();
    return moved;
  }

  /**
   * Takes a new page order, carrying each page's state to wherever it went.
   *
   * What a deleted page costs, and why this is not simply a rebuild: a slot's
   * learned size, its tile epoch and its own turn all belong to the *page*, not
   * to the position, so they are re-indexed by identity. A page nobody could
   * find in the new order --- the one that was deleted --- takes its entries
   * with it.
   *
   * **Everything rendered is dropped**, both tiers, exactly as {@link setTurns}
   * does. That is not laziness about which tiles survive: a tile is placed by
   * the slot it was requested for, and after a deletion every slot below the gap
   * holds a different page, so the surviving pixels are in the wrong places
   * rather than merely stale. Re-keying them by identity is possible and is not
   * worth it for an operation a reader performs once in a while --- and it is
   * what the epochs already guarantee cannot be got wrong by accident.
   *
   * Returns whether the layout moved, which is the caller's cue to re-anchor the
   * reader, on the same contract as {@link notePageSize} and
   * {@link setPageTurns}.
   */
  setPages(next: PageView[]): boolean {
    const before = this.order;
    const at = new Map(before.map((page, slot) => [page.id, slot]));

    this.order = [...next];
    this.opts.pageCount = next.length;
    // Read out of the *old* arrays by the slot the page used to be in. Written
    // in one pass into fresh arrays rather than spliced in place, because a
    // deletion in the middle moves every entry after it and an in-place shuffle
    // would read entries it had already written.
    const sizes: (PageSize | null)[] = [];
    const turns: number[] = [];
    const epochs: number[] = [];
    for (const page of next) {
      const was = at.get(page.id);
      sizes.push(was === undefined ? null : (this.sizes[was] ?? null));
      turns.push(page.turns % 4);
      // Carried, and deliberately **not** bumped. A reply for a tile requested
      // under the old order can still arrive, and the mechanism that drops it is
      // the generation bump inside `clearTiles` below --- which covers every
      // outstanding request at once. Bumping here as well was written first and
      // a mutation removing it survived the whole suite, which is what says it
      // was a second mechanism for one outcome rather than a guard.
      //
      // What the carry is for is the *value*: a page's epoch must not go
      // backwards when it moves, or a per-page invalidation after the move can
      // collide with a request issued before it.
      epochs.push(was === undefined ? 0 : (this.epochs[was] ?? 0));
    }
    this.sizes = sizes;
    this.pageTurns = turns;
    this.epochs = epochs;

    // Before the geometry moves, for the reason `setZoom` and `setTurns` clear
    // first: `clearTiles` withdraws by asking the window which tiles it still
    // wants, and that has to be the window they were requested for.
    this.clearTiles();
    this.dropPlaceholders();
    this.estimate = this.meanKnownSize();
    const beforeTotal = this.totalHeight;
    this.computeGeometry();
    this.relayout();
    return this.totalHeight !== beforeTotal || before.length !== next.length;
  }

  /** The mean of the page sizes that are known, which is never none. */
  private meanKnownSize(): PageSize {
    let width = 0;
    let height = 0;
    let known = 0;
    for (const size of this.sizes) {
      if (!size) continue;
      width += size.width_pt;
      height += size.height_pt;
      known++;
    }
    // A document with no pages has no known size and nothing to lay out; page 1
    // is what the type guarantees, so the fallback names something real rather
    // than a paper size nobody asked for.
    if (known === 0) return this.opts.pages[0];
    return { width_pt: width / known, height_pt: height / known };
  }

  /**
   * Rebuilds every page's box from the sizes, zoom and rotation as they are now.
   *
   * O(pageCount), and run on a zoom, a rotation and every size correction. On
   * the 775-page corpus that is 775 iterations of arithmetic --- measurably
   * nothing beside the 86 ms the backend would spend collecting the same table
   * eagerly, which is why the table is built here from what arrives rather than
   * asked for up front.
   */
  private computeGeometry(): void {
    const { zoom, dpr, tilePx, pageCount } = this.opts;
    const boxes: PageBox[] = new Array<PageBox>(Math.max(0, pageCount));
    let top = 0;
    for (let page = 0; page < boxes.length; page++) {
      const shown = this.displayedPageSize(page);
      const widthDev = Math.round(shown.width_pt * zoom * dpr);
      const heightDev = Math.round(shown.height_pt * zoom * dpr);
      boxes[page] = {
        widthDev,
        heightDev,
        widthCss: widthDev / dpr,
        heightCss: heightDev / dpr,
        cols: Math.ceil(widthDev / tilePx),
        rows: Math.ceil(heightDev / tilePx),
        top,
      };
      top += heightDev / dpr + PAGE_GAP;
    }
    this.boxes = boxes;
    // The trailing gap is between pages, so the last page does not get one.
    this.totalHeight = Math.max(0, top - PAGE_GAP);
  }

  /**
   * Records a page's real size, relaying out if it differs from what was assumed.
   *
   * Returns whether anything moved, which is the caller's cue to re-anchor the
   * reader: the scroll offset is in CSS pixels down a document that has just
   * changed length, so leaving it alone teleports the view. `viewer.ts` owns the
   * offset and does exactly that.
   *
   * A size that matches what was already laid out returns `false` and touches
   * nothing --- which is every page of every uniform document, i.e. almost every
   * page this will ever be called for. The learning is still recorded, because a
   * page whose size is *known* stops depending on an estimate that can move.
   */
  notePageSize(page: number, size: PageSize): boolean {
    if (page < 0 || page >= this.boxes.length) return false;
    const known = this.sizes[page];
    if (
      known &&
      known.width_pt === size.width_pt &&
      known.height_pt === size.height_pt
    ) {
      return false;
    }
    this.sizes[page] = size;
    return this.applySizes();
  }

  /**
   * Rebuilds the layout after a size changed, invalidating whatever moved.
   *
   * Every page is compared, not only the one that was learned: the estimate is a
   * mean over the known sizes, so learning one page relocates every page whose
   * size is still unknown. A page whose *box* changed had its tiles rendered for
   * a different rectangle and is invalidated; a page that only slid down the
   * document keeps its pixels and is merely re-placed.
   */
  private applySizes(): boolean {
    const before = this.boxes;
    const beforeTotal = this.totalHeight;
    this.estimate = this.meanKnownSize();
    this.computeGeometry();

    let changed = this.totalHeight !== beforeTotal;
    for (let page = 0; page < this.boxes.length; page++) {
      const was = before[page];
      const now = this.boxes[page];
      if (!was || !now) continue;
      if (was.widthDev !== now.widthDev || was.heightDev !== now.heightDev) {
        this.invalidatePage(page);
        changed = true;
      } else if (was.top !== now.top) {
        changed = true;
      }
    }
    if (changed) this.relayout();
    return changed;
  }

  /**
   * Throws away one page's pixels and the work still out for them.
   *
   * The epoch bump is what covers the race the withdrawal cannot: a render that
   * had already finished still arrives, and adopting it would paint a tile drawn
   * for the old box into the new one --- which on the page whose size was just
   * corrected is exactly the crop the correction exists to remove.
   */
  private invalidatePage(page: number): void {
    this.epochs[page] = (this.epochs[page] ?? 0) + 1;

    for (const [id, entry] of this.tiles) {
      if (entry.key.page !== page) continue;
      this.release(entry);
      this.tiles.delete(id);
    }
    const placeholder = this.placeholders.get(page);
    if (placeholder) {
      placeholder.close();
      this.placeholders.delete(page);
    }
    const canvas = this.placeholderCanvases.get(page);
    if (canvas) {
      canvas.remove();
      this.placeholderCanvases.delete(page);
    }
    // The tier-1 wait, because the placeholder is about to be asked for again at
    // a size nothing has tried yet. The tier-2 waits are left: their ids are
    // per tile and a failing tile is still failing at a different rectangle.
    this.backoff.clear(`p${page}`);
    this.withdraw((outstanding) => outstanding.page === page);
  }

  /** Total scrollable height in CSS pixels. */
  get documentHeight(): number {
    return this.totalHeight;
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

  /**
   * Work asked for and not yet on screen.
   *
   * The viewer's frame loop idles when this reaches zero, so it counts tiles
   * that have landed but not been drained as well as requests still out. A
   * reply and the frame that draws it are one frame apart --- stopping between
   * the two would leave the pixels in a buffer nobody looks at again, and the
   * screen would stay blank until the next scroll happened to restart the loop.
   */
  get pendingWork(): number {
    return (
      this.inFlight.size + this.arrived.length + this.arrivedPlaceholders.length
    );
  }

  /**
   * Milliseconds until the earliest backed-off request may be issued again, or
   * `null` if nothing is waiting.
   *
   * `reference` is the caller's clock reading, and it has to be the same one the
   * frame that just ran was given --- which is why {@link frame} takes one. Two
   * decisions are made about a backed-off request: "may this be issued yet",
   * inside the frame, and "when should the loop be woken", as it goes idle.
   * Sampling the clock separately for each lets an entry come due *between* the
   * two readings: the frame did not issue it, and the wake is not armed for it,
   * so it waits for some unrelated input. That is the permanently blank square
   * this whole mechanism exists to avoid, arriving as a race rather than by
   * design.
   *
   * A request whose wait has already elapsed is deliberately not counted; see
   * `backoff.ts` for why.
   */
  nextRetryMs(reference: number): number | null {
    return this.backoff.nextWaitMs(reference);
  }

  /**
   * A page's tier-1 bitmap, or `null` if it has not been rendered.
   *
   * Read-only, and deliberately: the page strip borrows from this cache and does
   * not add to it. Tier 1 is permanent for the session, so letting the strip
   * fill it would grow it to one bitmap per page --- 98 MB on the 775-page
   * corpus, for pages nobody has looked at.
   *
   * The bitmap belongs to the scroller and is closed when the document is; a
   * caller that wants to keep it must copy it.
   */
  placeholderFor(page: number): ImageBitmap | null {
    return this.placeholders.get(page) ?? null;
  }

  /**
   * A page's size on screen, in CSS pixels.
   *
   * The geometry the scroller actually laid out, rather than what it was asked
   * for --- so a caller can check that a rotation reached this class and not
   * only the one above it. The two are separately capable of being wrong.
   */
  pageBoxCssOf(page: number): { width: number; height: number } {
    const box = this.boxes[page];
    return { width: box?.widthCss ?? 0, height: box?.heightCss ?? 0 };
  }

  /** Tiles a page is divided into, so a run can report its working set. */
  tilesOnPage(page: number): number {
    const box = this.boxes[page];
    return box ? box.cols * box.rows : 0;
  }

  /**
   * Changes the zoom, keeping tier 1 and dropping tier 2.
   *
   * Every tier-2 tile was rendered at a scale, so none of them survives. Tier-1
   * placeholders are rendered at a fixed 150 px and only stretched by CSS, so
   * all of them do --- which is what stops a zoom step on the A0 sheet going
   * grey for the 1.5 s its placeholder costs to produce again.
   */
  setZoom(zoom: number): void {
    if (zoom === this.opts.zoom) return;
    this.opts.zoom = zoom;
    // Before the geometry moves: `clearTiles` withdraws by a predicate that
    // asks the window which tiles it still wants, and the window it should be
    // asking is the one those tiles were requested for.
    this.clearTiles();
    this.computeGeometry();
    this.relayout();
  }

  /**
   * Rotates the view, dropping **both** tiers.
   *
   * Unlike a zoom step, which keeps tier 1 because a 150 px placeholder only
   * gets stretched: a placeholder is a rendered bitmap and a rotated one is a
   * different picture. Keeping it would leave the page sideways underneath its
   * own sharp tiles until every tile landed.
   *
   * So a rotation on the A0 sheet goes grey for the 1.5 s its placeholder costs
   * to produce again, and there is no way around that short of rotating the
   * bitmap ourselves --- which is a real option (a quarter turn is lossless) and
   * is not taken here because nothing has measured whether it is worth the code.
   */
  setTurns(turns: number): void {
    const next = ((turns % 4) + 4) % 4;
    if (next === this.opts.turns) return;

    // Before the geometry moves, for the same reason `setZoom` clears first:
    // `clearTiles` withdraws by asking the window which tiles it still wants,
    // and that has to be the window they were requested for.
    this.clearTiles();
    this.dropPlaceholders();
    this.opts.turns = next;
    this.computeGeometry();
    this.relayout();
  }

  /**
   * Turns page inversion on or off, discarding everything drawn the other way.
   *
   * Cheaper than {@link setTurns} in one respect and identical in the other: the
   * geometry does not move, so nothing has to be recomputed or laid out again,
   * but every tile and placeholder on screen is the wrong colour and has to be
   * rendered again. On the A0 sheet that is the same seconds a rotation costs,
   * and for the same reason --- the pixels are produced by Pdfium, and there is
   * no way to reach them without asking it.
   *
   * Inverting the bitmaps we already hold is the obvious alternative, and it
   * would be exact: the transform is its own inverse. It is not done because
   * nothing has measured whether inverting a screenful of tiles beats rendering
   * them, and on the cheap corpus a tile costs 1.5 ms to render outright.
   */
  setInvert(invert: boolean): void {
    if (invert === this.opts.invert) return;
    // Cleared before the flag moves, exactly as `setTurns` does: `clearTiles`
    // asks the window which tiles it still wants, and that has to be the window
    // they were requested for.
    this.clearTiles();
    this.dropPlaceholders();
    this.opts.invert = invert;
    this.relayout();
  }

  /** Forgets every tier-1 placeholder, and any request still out for one. */
  private dropPlaceholders(): void {
    for (const bitmap of this.placeholders.values()) bitmap.close();
    this.placeholders.clear();
    this.backoff.clearAll();
    for (const canvas of this.placeholderCanvases.values()) canvas.remove();
    this.placeholderCanvases.clear();
    for (const { bitmap } of this.arrivedPlaceholders.splice(0)) bitmap.close();
    // `survivesClear` is exactly the placeholder requests, which is what
    // `clearTiles` deliberately spares --- so this is the other half of it, and
    // the two together withdraw everything outstanding.
    this.withdraw((outstanding) => outstanding.survivesClear);
    // A withdrawal can lose the race with a render that was already finishing.
    // The generation bump is what stops that reply being adopted as a
    // placeholder for the orientation it is no longer in.
    this.placeholderGeneration++;
  }

  /**
   * Changes the viewport size, keeping both tiers.
   *
   * Nothing inside a tile depends on the viewport --- only where it is drawn
   * does --- so a resize re-places tiles rather than re-rendering them. It does
   * move the window, so the next frame requests whatever the new one uncovers.
   */
  resize(viewport: { width: number; height: number }): void {
    this.opts.viewport = viewport;
    this.host.style.width = `${viewport.width}px`;
    this.host.style.height = `${viewport.height}px`;

    if (this.surface) {
      this.surface.width = Math.round(viewport.width * this.opts.dpr);
      this.surface.height = Math.round(viewport.height * this.opts.dpr);
      this.surface.style.width = `${viewport.width}px`;
      this.surface.style.height = `${viewport.height}px`;
    }
    this.relayout();
  }

  /**
   * Re-places everything whose position depends on the zoom or the viewport.
   *
   * Only the `tiles` layout mounts anything; the `viewport` layout repaints
   * from cache every frame and so needs nothing but the new geometry.
   */
  private relayout(): void {
    if (this.spacer) this.spacer.style.height = `${this.documentHeight}px`;

    for (const [page, canvas] of this.placeholderCanvases) {
      const box = this.boxes[page];
      if (!box) continue;
      canvas.style.left = `${this.pageLeftCss(page)}px`;
      canvas.style.top = `${box.top}px`;
      canvas.style.width = `${box.widthCss}px`;
      canvas.style.height = `${box.heightCss}px`;
    }
    for (const entry of this.tiles.values()) {
      if (!entry.canvas) continue;
      entry.canvas.style.left = `${
        this.pageLeftCss(entry.key.page) + entry.rect.x / this.opts.dpr
      }px`;
      entry.canvas.style.top = `${
        this.pageTop(entry.key.page) + entry.rect.y / this.opts.dpr
      }px`;
    }
  }

  /**
   * Advances the scroll position and does one frame of work.
   *
   * Everything that touches the DOM happens here rather than in a fetch
   * callback, so the whole per-frame cost of the design is inside the interval
   * the harness times. A tile that arrives between frames is drawn at the start
   * of the next one.
   *
   * `now` is the frame's clock reading, and a caller that also asks
   * {@link nextRetryMs} when to wake again must pass the same one to both --- see
   * there. It defaults so the benchmark, which asks nothing about retries, does
   * not have to carry a clock it has no use for.
   */
  frame(scrollTop: number, now = performance.now()): FrameStats {
    this.scrollTop = Math.max(0, Math.min(scrollTop, this.maxScroll));
    this.drawnThisFrame = 0;

    if (this.container) this.container.scrollTop = this.scrollTop;

    this.drain();
    this.request(now);
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
    // Backoff is dropped with the tiles, and only the callers of this method can
    // drop it: every one of them is a *reader's* action --- a zoom, a rotation,
    // an inversion --- and someone who has just asked for something different is
    // owed an immediate attempt rather than the tail of a wait they cannot see.
    // Nothing on the frame path clears it, which is what keeps the bound real.
    this.backoff.clearAll();

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
    this.stats.failed = 0;
    this.stats.bytes = 0;
    this.stats.renderMs = 0;
    this.stats.decodeMs = 0;
    this.stats.evicted = 0;
  }

  destroy(): void {
    // First, before anything is released: a tile arriving mid-teardown must see
    // a scroller that is dead rather than one that is half dismantled.
    this.life.end();
    this.scheme.removeEventListener("change", this.onScheme);

    // Everything outstanding, unconditionally --- **not** through `withdraw`,
    // which honours the `cancel` variant flag. That flag exists so the benchmark
    // can measure what withdrawal is worth; a teardown is not a variant, and a
    // document being closed while its tiles are still rendering is the one case
    // where there is nothing to trade off. Without this the outgoing document's
    // renders stay in the FIFO ahead of the first page of the file the reader
    // has just opened, and on the A0 corpus a single tier-1 placeholder is 1.5 s
    // of that queue.
    for (const outstanding of this.inFlight.values()) {
      if (outstanding.withdrawn) continue;
      outstanding.withdrawn = true;
      cancelTile(outstanding.rid);
    }

    this.clearTiles();
    for (const bitmap of this.placeholders.values()) bitmap.close();
    this.placeholders.clear();
    // The arrival queue too: `clearTiles` splices `arrived`, and these are the
    // other half of it --- placeholders that landed and had not been drawn yet.
    for (const { bitmap } of this.arrivedPlaceholders.splice(0)) bitmap.close();
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
      // The page's own epoch as well as the document's generation. A tile
      // rendered for the box a page had *before* its size was corrected is not a
      // superseded tile --- nothing about the scroll invalidated it --- so
      // counting it as one would report a queue failure that did not happen, and
      // drawing it would paint the crop the correction just removed.
      if (
        arrival.generation !== this.generation ||
        arrival.epoch !== (this.epochs[arrival.key.page] ?? 0)
      ) {
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

    for (const {
      page,
      bitmap,
      generation,
      epoch,
    } of this.arrivedPlaceholders.splice(0)) {
      if (
        generation !== this.placeholderGeneration ||
        epoch !== (this.epochs[page] ?? 0)
      ) {
        bitmap.close();
        continue;
      }
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
    // The surround, from a custom property so it follows the system theme.
    // A literal here was a mid grey that reads as "unlit" against a light
    // window and as "lit" against a dark one --- brighter than the page it
    // surrounds, which is the one thing it must not be.
    this.host.style.background = this.surround;

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

  /**
   * CSS-pixel top-left of a page, in document coordinates.
   *
   * Exposed for the selection overlay, which draws in the same space the tiles
   * do and must not derive it independently --- a second copy of the centring
   * and page-pitch arithmetic is a second thing to keep in step, and the symptom
   * of it drifting is a highlight that sits beside the text rather than on it.
   */
  pageOrigin(page: number): { left: number; top: number } {
    return { left: this.pageLeftCss(page), top: this.pageTop(page) };
  }

  /** Pages any part of which is currently on screen. */
  visiblePages(): number[] {
    return this.pagesIn(
      this.scrollTop,
      this.scrollTop + this.opts.viewport.height,
    );
  }

  /**
   * Zero-based index of the page occupying a scroll offset.
   *
   * A binary search over the accumulated tops rather than a division, which is
   * what a per-page layout costs here and it is the whole cost: the tops are
   * non-decreasing by construction, so the answer is the last page starting at
   * or before `css`. An offset in the gap below a page belongs to that page,
   * which is what the pitch-based division it replaced also did.
   */
  pageAt(css: number): number {
    let low = 0;
    let high = this.boxes.length - 1;
    if (high < 0) return 0;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if ((this.boxes[mid]?.top ?? 0) <= css) low = mid;
      else high = mid - 1;
    }
    return low;
  }

  /** Scroll offset that puts a page's top edge at the top of the viewport. */
  pageTopOf(page: number): number {
    return this.pageTop(page);
  }

  /**
   * Distance from a page's top to the next page's, in CSS pixels.
   *
   * Exposed so a caller can express a position as a fraction through a page and
   * restore it after the geometry changes underneath it --- which is what
   * rotating the view does, and what learning a page's real size does.
   */
  pagePitchOf(page: number): number {
    return (this.boxes[page]?.heightCss ?? 0) + PAGE_GAP;
  }

  /** CSS-pixel top of a page in the scrolled document. */
  private pageTop(page: number): number {
    return this.boxes[page]?.top ?? 0;
  }

  /** Pages intersecting a CSS-pixel band, clamped to the document. */
  private pagesIn(top: number, bottom: number): number[] {
    const pages: number[] = [];
    // `pageAt` clamps, so a band starting above the document has to enter the
    // walk at page 0 rather than at whatever the clamp returned.
    let page = top <= 0 ? 0 : this.pageAt(top);
    for (; page < this.boxes.length; page++) {
      const box = this.boxes[page];
      if (!box || box.top > bottom) break;
      pages.push(page);
    }
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
    page: number,
    col: number,
    row: number,
  ): { x: number; y: number; width: number; height: number } {
    const x = col * this.opts.tilePx;
    const y = row * this.opts.tilePx;
    const box = this.boxes[page];
    return {
      x,
      y,
      // Clamped at the page edge: a tile hanging off the page would carry
      // several megabytes of white and cost the same to move as real content.
      width: Math.min(this.opts.tilePx, (box?.widthDev ?? 0) - x),
      height: Math.min(this.opts.tilePx, (box?.heightDev ?? 0) - y),
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
  private request(now: number): void {
    this.withdraw((outstanding) => !outstanding.isWanted());

    const { top, bottom } = this.band();
    const centre = this.scrollTop + this.opts.viewport.height / 2;

    const wanted: { key: TileKey; distance: number }[] = [];

    for (const page of this.pagesIn(top, bottom)) {
      this.requestPlaceholder(page, now);

      const box = this.boxes[page];
      if (!box) continue;
      const pageTop = box.top;
      for (let row = 0; row < box.rows; row++) {
        const tileTop = pageTop + (row * this.opts.tilePx) / this.opts.dpr;
        const tileBottom = tileTop + this.opts.tilePx / this.opts.dpr;
        if (tileBottom < top || tileTop > bottom) continue;

        for (let col = 0; col < box.cols; col++) {
          const key: TileKey = { page, col, row };
          const id = keyOf(key);
          if (this.tiles.has(id) || this.inFlight.has(id)) continue;
          if (this.backoff.blocked(id, now)) continue;
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

  /**
   * Records a failed request, and says once why it failed.
   *
   * `tiles.ts` builds an error naming the page and the tile's origin, and every
   * `catch` in this file used to drop it --- so a renderer erroring on one page
   * and a renderer erroring on everything left the same trace, which is none.
   * Printed on the first failure only, because the reason does not change
   * between retries and the retries go on for as long as the document is open.
   */
  private noteFailure(id: string, reason: unknown): void {
    this.stats.failed++;
    // Not a tile that failed --- a document that is gone. Backing this one off
    // would schedule a retry of something the backend has already established
    // can never succeed, and every retry is another refusal. The latch stops
    // this scroller asking again at all, and reports once: the condition is a
    // property of the document, so N failing tiles are one piece of news.
    if (reason instanceof DocumentGone) {
      if (!this.gone) {
        this.gone = true;
        this.opts.onGone?.(reason.message);
      }
      return;
    }
    if (this.backoff.note(id, performance.now()) === 1) {
      console.warn(`tile ${id} could not be rendered: ${String(reason)}`);
    }
  }

  /**
   * Which page of the file a slot draws.
   *
   * `undefined` for a slot that is not in the document, and nothing asks for a
   * tile in that case. Deliberately not falling back to the slot number: that
   * fallback is right for every unedited document and asks for the wrong page in
   * exactly the case the order exists for.
   */
  private sourceOf(slot: number): number | undefined {
    return this.order[slot]?.source;
  }

  private send(key: TileKey): void {
    // The document's bytes are gone; there is nothing any tile request can do
    // but be refused.
    //
    // Gated here *and* in `requestPlaceholder`, because there are two request
    // paths and not one. This comment claimed there was only this one, the test
    // counted three requests where it expected two, and the placeholder was the
    // third --- a tier-1 render of every page the reader scrolls onto, each one
    // a refusal. Gating a "single choke point" that is not the only choke point
    // looks exactly like gating the real thing.
    if (this.gone) return;
    // A slot with no page behind it. Reachable while a state reply is in flight,
    // and the honest answer is to render nothing rather than to guess a page
    // number --- the next frame lays out the order that has arrived by then.
    const source = this.sourceOf(key.page);
    if (source === undefined) return;
    const id = keyOf(key);
    const rect = this.tileRect(key.page, key.col, key.row);
    const generation = this.generation;
    const epoch = this.epochs[key.page] ?? 0;
    const rid = nextRequestId();
    this.inFlight.set(id, {
      rid,
      page: key.page,
      isWanted: () =>
        this.generation === generation &&
        this.epochs[key.page] === epoch &&
        this.isWanted(key),
      survivesClear: false,
      withdrawn: false,
    });
    this.stats.requested++;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page: source,
      scale: this.opts.zoom * this.opts.dpr,
      turns: this.requestTurns(key.page),
      invert: this.opts.invert,
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      format: "raw",
    })
      .then(
        this.life.claim(
          (result) => {
            this.inFlight.delete(id);
            // Withdrawn in time: the renderer stopped, and there is nothing to
            // count as delivered or as discarded because nothing was produced.
            // Deliberately not treated as a success either --- the request never
            // ran, so it says nothing about whether this tile can be rendered,
            // and clearing the backoff on it would let a withdrawal reset the
            // wait.
            if (!result) {
              this.stats.abandoned++;
              return;
            }
            this.backoff.clear(id);
            this.stats.bytes += result.bytes;
            this.stats.renderMs += result.renderUs / 1000;
            this.stats.decodeMs += result.decodeMs;
            this.arrived.push({
              key,
              rect,
              bitmap: result.bitmap,
              generation,
              epoch,
            });
          },
          // Landed after teardown: `arrived` is drained by a frame loop that no
          // longer runs, so pushing it here is how the bitmap is lost.
          (result) => result?.bitmap.close(),
        ),
      )
      .catch((reason: unknown) => {
        this.inFlight.delete(id);
        this.noteFailure(id, reason);
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
    const rect = this.tileRect(key.page, key.col, key.row);

    const tileTop = this.pageTop(key.page) + rect.y / this.opts.dpr;
    const tileBottom = tileTop + rect.height / this.opts.dpr;
    if (tileBottom < top || tileTop > bottom) return false;

    const tileLeft = this.pageLeftCss(key.page) + rect.x / this.opts.dpr;
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
        `left:${this.pageLeftCss(key.page) + rect.x / this.opts.dpr}px;` +
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

  /**
   * CSS-pixel left edge of a page, centred in the viewport.
   *
   * Per page rather than per document, because on a mixed-size document the
   * pages are not the same width --- an A3 insert centres on the same axis the
   * A4 pages around it do, which is what makes the column of pages read as one
   * document rather than as two left-aligned stacks.
   */
  private pageLeftCss(page: number): number {
    const width = this.boxes[page]?.widthCss ?? 0;
    return Math.max(0, (this.opts.viewport.width - width) / 2);
  }

  /**
   * Requests a page's tier-1 placeholder once.
   *
   * Section 4 records that this is not free on a hard page --- a 150 px render
   * of the A0 sheet costs 1.5 s --- so it goes through the same queue as
   * everything else and the scroller shows nothing until it lands. The
   * alternative, blocking on it, would hide exactly that cost.
   */
  private requestPlaceholder(page: number, now: number): void {
    if (this.gone) return;
    const source = this.sourceOf(page);
    if (source === undefined) return;
    const id = `p${page}`;
    if (this.placeholders.has(page) || this.inFlight.has(id)) return;
    if (this.backoff.blocked(id, now)) return;

    const displayed = this.displayedPageSize(page);
    const scale = TIER1_WIDTH / displayed.width_pt;
    const height = Math.round(displayed.height_pt * scale);
    const rid = nextRequestId();
    const generation = this.placeholderGeneration;
    const epoch = this.epochs[page] ?? 0;
    // Withdrawable like any other request, and for the same reason: a
    // placeholder is permanent once it lands, but a page that has left the band
    // is not one the renderer should be spending 1.5 s on while the visible
    // page waits behind it in the queue. It is re-requested if the page comes
    // back.
    this.inFlight.set(id, {
      rid,
      page,
      isWanted: () => this.epochs[page] === epoch && this.pageInBand(page),
      survivesClear: true,
      withdrawn: false,
    });
    this.stats.requested++;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page: source,
      scale,
      turns: this.requestTurns(page),
      invert: this.opts.invert,
      x: 0,
      y: 0,
      width: TIER1_WIDTH,
      height,
      format: "raw",
    })
      .then(
        this.life.claim(
          (result) => {
            this.inFlight.delete(id);
            if (!result) {
              this.stats.abandoned++;
              return;
            }
            this.backoff.clear(id);
            this.stats.delivered++;
            this.stats.bytes += result.bytes;
            this.stats.renderMs += result.renderUs / 1000;
            this.stats.decodeMs += result.decodeMs;
            this.arrivedPlaceholders.push({
              page,
              bitmap: result.bitmap,
              generation,
              epoch,
            });
          },
          // As the tier-2 arrival above: `destroy` empties this queue once and
          // nothing drains it again.
          (result) => result?.bitmap.close(),
        ),
      )
      .catch((reason: unknown) => {
        this.inFlight.delete(id);
        this.noteFailure(id, reason);
      });
  }

  /** Whether any part of a page lies in the current band. */
  private pageInBand(page: number): boolean {
    const { top, bottom } = this.band();
    const box = this.boxes[page];
    if (!box) return false;
    return box.top + box.heightCss >= top && box.top <= bottom;
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
    const box = this.boxes[page];
    if (!box) return;
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.style.cssText =
      `position:absolute;z-index:0;` +
      `left:${this.pageLeftCss(page)}px;top:${box.top}px;` +
      `width:${box.widthCss}px;height:${box.heightCss}px;`;
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
    ctx.fillStyle = this.surround;
    ctx.fillRect(0, 0, surface.width, surface.height);

    for (const page of this.pagesIn(
      this.scrollTop,
      this.scrollTop + viewport.height,
    )) {
      const box = this.boxes[page];
      if (!box) continue;
      const left = this.pageLeftCss(page);
      const top = box.top - this.scrollTop;

      const placeholder = this.placeholders.get(page);
      if (placeholder) {
        ctx.drawImage(
          placeholder,
          left * dpr,
          top * dpr,
          box.widthCss * dpr,
          box.heightCss * dpr,
        );
        this.drawnThisFrame++;
      }

      for (let row = 0; row < box.rows; row++) {
        for (let col = 0; col < box.cols; col++) {
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
      const box = this.boxes[page];
      if (!box) continue;
      const pageTop = box.top;
      const pageLeft = this.pageLeftCss(page);
      const hasPlaceholder = this.placeholders.has(page);

      for (let row = 0; row < box.rows; row++) {
        const rect = this.tileRect(page, 0, row);
        const tileTop = pageTop + rect.y / this.opts.dpr;
        const tileBottom = tileTop + rect.height / this.opts.dpr;
        const overlap = Math.min(bottom, tileBottom) - Math.max(top, tileTop);
        if (overlap <= 0) continue;

        for (let col = 0; col < box.cols; col++) {
          const columnRect = this.tileRect(page, col, row);
          const tileLeft = pageLeft + columnRect.x / this.opts.dpr;
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
