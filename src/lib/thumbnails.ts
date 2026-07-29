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
  /** Geometry of page 1, taken as representative --- as `scroller.ts` does. */
  page: PageSize;
  tier1: Tier1Access;
  /** Called when a row is activated, with a zero-based page index. */
  onNavigate: (page: number) => void;
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

  /** Requests withdrawn to get out of the viewer's way. For the check harness. */
  private yielded = 0;
  /** Thumbnails taken from the viewer's tier 1 rather than rendered again. */
  private borrowed = 0;

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

    this.spacer = document.createElement("div");
    this.spacer.style.cssText = `height:${this.rowHeight * opts.pageCount}px;`;

    this.list.appendChild(this.spacer);
    this.host.appendChild(this.list);
    root.appendChild(this.host);

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
    this.observer.disconnect();
    // Before the withdrawal, and load-bearing: `pump` refuses to issue anything
    // while this is false, and a strip torn down with it still true keeps
    // pumping. The settling request's `.then` calls `pump`, which starts the
    // next page, whose reply starts the one after --- one render per reply until
    // the window's rows run out, which is a screenful plus twice the overscan,
    // all of it for a document nobody is looking at any more and all of it in
    // front of the tiles for the one they are.
    this.active = false;
    this.withdraw();
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
    if (!active) this.withdraw();
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
    if (busy) this.withdraw();
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
    this.withdraw();

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
    this.withdraw();

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
    const row = this.rows.get(page);
    if (row) row.scrollIntoView({ block: "nearest" });
    else this.host.scrollTop = page * this.rowHeight - this.host.clientHeight / 2;
    this.layout();
  }

  /** Withdraws the outstanding request, if there is one. */
  private withdraw(): void {
    if (!this.request || this.request.withdrawn) return;
    this.request.withdrawn = true;
    this.yielded++;
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

    const rid = nextRequestId();
    const outstanding: Outstanding = { page, rid, withdrawn: false };
    this.request = outstanding;

    void fetchTile({
      rid,
      doc: this.opts.doc,
      page,
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
            if (result) this.keep(page, result.bitmap);
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
      case "Home":
        this.move(-this.opts.pageCount);
        break;
      case "End":
        this.move(this.opts.pageCount);
        break;
      case "Enter":
      case " ":
        this.opts.onNavigate(this.focused);
        break;
      default:
        return;
    }
    event.preventDefault();
    // The viewer underneath scrolls on the same keys.
    event.stopPropagation();
  };
}
