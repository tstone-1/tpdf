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
import { inTextField, isMac, matches } from "./keys";
import { CommentPopup } from "./commentpopup";
import {
  intoCrop,
  outOfCrop,
  pageGeometry,
  uncropped,
  type CropGeometry,
} from "./crop";
import {
  boxQuad,
  ERASER_RADIUS,
  ICON_SIZE,
  iconQuad,
  isMovable,
  isPath,
  paintOf,
  LINE_FRACTION,
  STAMP_CAP,
  STAMP_INSET,
  TEXT_INSET,
  TEXT_LEADING,
  TEXT_SIZE,
  SQUIGGLE_PERIOD,
  INK_SAMPLE,
  INK_WIDTH,
  isWash,
  markBand,
  OUTLINE_WIDTH,
  quadSwept,
  scrimBands,
  strokeSwept,
} from "./markband";
import { PointerDrag, type DragPoint } from "./drag";

/**
 * A point on a page, in that page's laid-out space, in points.
 *
 * Deliberately not {@link ScreenPoint}, which is in client coordinates: the two
 * are the same shape with different field names for exactly that reason, since
 * a value that means one and is used as the other is a rectangle in the wrong
 * place rather than a type error.
 */
export interface Point {
  x: number;
  y: number;
}

/**
 * A point on screen, as every event that carries one reports it.
 *
 * Structural rather than `PointerEvent`, so a `contextmenu` MouseEvent and a
 * synthetic point from the check harness are the same thing to the hit tests.
 * Nothing that takes one reads any other field.
 */
export interface ScreenPoint {
  clientX: number;
  clientY: number;
}
import { MarkPopup } from "./markpopup";
import { colorFor, sameColor, type MarkColor } from "./markcolors";
import type { Anchor } from "./popup";
import { hitTest, onPage, turnedFor, viewRect, type Comment } from "./comments";
import {
  History,
  linkAt,
  onPage as linksOnPage,
  orderedLinks,
  refusalFor,
  stepAlong,
  turnedFor as linksTurnedFor,
  type Link,
  type Place,
} from "./links";
import { Lifetime } from "./lifetime";
import { DESTINATION_MARGIN_PT } from "./outline";
import {
  markWalk,
  PageMap,
  stampWord,
  unedited,
  type MarkKind,
  type MarkView,
  type StampName,
  type PageId,
  type PageView,
  type RedactionView,
} from "./pages";
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
  turnQuad,
  unturnQuad,
  wordAt,
  type Caret,
  type PageText,
  type Quad,
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
  /**
   * Strokes in the drawing being made, or `null` when none is.
   *
   * **A mode has to be visible, and this is the whole of how.** Every other tool
   * in this viewer is one-shot: it arms, the next gesture spends it, and there
   * is nothing to be stuck in. Ink is not, because a drawing is several strokes
   * --- so a reader can be in a state where the next press draws rather than
   * selects, with no other sign of it. The box's own note argues that a mode a
   * reader can enter and then not recognise is worse than one they ask for, and
   * this field is what pays that argument off: the window reads it and says what
   * is being made and which two keys end it.
   *
   * Zero is reachable and is not the same as `null`: the tool is armed and
   * nothing has been drawn yet.
   */
  drawing: number | null;
  /**
   * What this sweep of the eraser has taken, or `null` when it is not armed.
   *
   * {@link drawing}'s twin and for its reason: the eraser is the second tool
   * here that stays armed between gestures, so a reader can be in a state where
   * the next press rubs out rather than selects. Both counts at zero means
   * armed and nothing swept, which is not the same as `null`.
   *
   * `strokes` counts *across* the drawings a sweep crossed, because what the
   * reader is watching is strokes disappearing and they do not care which
   * drawing each one belonged to. `marks` counts every other kind, which has no
   * parts and so goes whole.
   *
   * **One field holding two numbers rather than two nullable fields**, so that
   * "armed" is stated once. Two fields would have to be `null` together, and a
   * pair that must agree is a pair that can disagree.
   */
  erasing: { strokes: number; marks: number } | null;
  /**
   * The one-shot tool waiting for a gesture, or `null` when none is.
   *
   * **The two above are modes a reader can be stuck in; this is one they can be
   * *lost* in**, which is a different complaint and needs its own answer. A
   * one-shot tool spends itself on the next gesture, so there is nothing to
   * escape from --- but between arming it from the palette and making that
   * gesture there is a crosshair and no words, and a reader who armed it and
   * then looked away has nothing telling them what their next press will do.
   * Naming it costs a line of chrome that comes and goes exactly as the two
   * above already do.
   *
   * `ink` never appears here. It arms like the rest and then reports through
   * {@link ViewerStatus.drawing} the moment it is armed, and two lines saying
   * the same thing in different words is worse than one.
   *
   * **`"crop"` is not a `MarkKind` and appears here anyway**, because the
   * complaint above is about being *lost*, not about making a mark: a reader who
   * armed the crop from the palette and then looked away has the same crosshair
   * and the same absence of words. Widening this field rather than adding a
   * second boolean keeps that one line of chrome one line --- two elements coming
   * and going beside each other is the toolbar-rearrangement trap, and only one
   * of the two can ever be set.
   *
   * **`"redact"` joins it on the same argument**, and it is the member whose
   * absence would cost most: a reader who armed the redaction tool and looked
   * away sees the identical crosshair a crop gives, and the two gestures are the
   * same drag with opposite meanings. The line of chrome is where they are told
   * which one their next press is.
   */
  armed: MarkKind | "crop" | "redact" | null;
  /** State of the find-in-document scan. */
  search: SearchStatus;
}

/**
 * What a completed drag produced, in the file's display space.
 *
 * **Both fields always, one of them empty**, rather than a union of two shapes.
 * That is deliberate and it is not laziness: this is exactly `NewMark` on the
 * wire, so the callback hands its caller the command's own arguments and there
 * is nothing to translate --- and the rule that decides which is empty lives in
 * one place, `Doc::annotate`, which refuses a mark whose kind and shape
 * disagree. A union here would put a second copy of that rule in TypeScript,
 * where it could only be checked against itself.
 */
export interface Drawn {
  /** Four numbers per rectangle. Empty for ink, whose rectangle is derived. */
  quads: number[];
  /** `x y x y ...` per stroke. Empty for every kind but ink. */
  strokes: number[][];
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
  /**
   * Called when the comment shown on the page changes, or `null` when it closes.
   *
   * The sidebar's selection follows it, so clicking a note on the page
   * highlights its row and the two never disagree about which comment is being
   * read. Optional, because the check harness builds a viewer with no sidebar.
   */
  onComment?: (id: number | null) => void;
  /**
   * Called when the note on one of the reader's own marks opens, or closes.
   *
   * {@link onComment}'s twin, and the marks panel's selection follows it for the
   * same reason --- pressing a mark on the page highlights its row, and neither
   * side keeps its own idea of which mark is being read.
   *
   * Fired by `markpopup.ts` rather than from here, because the box is closed in
   * four places for five reasons and one of them is a mark disappearing out from
   * under it. Only ever on a change: reopening the box on the mark it is already
   * on says nothing. A viewer being destroyed reports the close too, which is
   * what a caller wants --- the panel is going with it either way.
   */
  onMark?: (id: number | null) => void;
  /**
   * Called when the reader typed a note on one of their own marks.
   *
   * The whole note, and only when it changed --- see `markpopup.ts`. Optional
   * for the reason `onComment` is: a viewer with no model behind it can still be
   * driven, and a mark it cannot save a note for is better than one it refuses
   * to open.
   */
  onMarkNote?: (mark: number, note: string) => void;
  /** Called when the reader asked to take one of their own marks off the page. */
  onMarkRemove?: (mark: number) => void;
  /**
   * Called when the reader picked a colour for one of their own marks.
   *
   * Only ever with a colour the mark is not already --- the swatch row does that
   * comparison, next to the button that was pressed. Optional for
   * {@link onMarkNote}'s reason: a viewer with no model behind it still draws
   * the row and still shows which swatch is on.
   */
  onMarkRecolor?: (mark: number, color: MarkColor) => void;
  /**
   * Called when the reader finished drawing a mark, with the page it is on.
   *
   * `page` is the page's **id**, not its slot, and {@link Drawn} is in the
   * file's display space --- the same pair a `mark` command takes, because that is what
   * the caller does with it. Doing the translation here rather than at the
   * caller is the point: the slot, the crop and the two rotations are all the
   * viewer's, and a caller that had to undo them would be a second copy of
   * {@link fileRectOn}.
   *
   * Optional, so a viewer with no model behind it can still be driven. A drag
   * then draws its preview and commits nothing, which is what the harness does.
   */
  onDrawn?: (
    kind: MarkKind,
    page: PageId,
    drawn: Drawn,
    stamp: StampName | null,
  ) => void;
  /**
   * The reader dragged one of their own marks somewhere else on its page.
   *
   * An offset in the page's **display** space, which is the space the model
   * holds a mark's quads in --- not the view's, and not client pixels. The
   * viewer is the only thing that can convert between them, because it is the
   * only thing that knows the zoom, the crop and both rotations, and a consumer
   * doing it would be a second copy of that conversion.
   *
   * Already clamped so that the mark stays on its page. `docmodel` cannot do it
   * --- the page's size in points is the renderer's answer, not the model's ---
   * so it happens here, in the same place and for the same reason `iconQuad` and
   * `boxQuad` clamp the geometry they build.
   *
   * Optional for {@link onDrawn}'s reason: a viewer with no model behind it
   * still has to be draggable, which is what lets the window check drive the
   * gesture and read the preview without committing anything.
   */
  onMarkMoved?: (id: number, dx: number, dy: number) => void;

  /**
   * One sweep of the eraser: which drawing, and which of its strokes went.
   *
   * `remove` is *positions* into the strokes the viewer was last given, not
   * points --- the backend owns what a drawing is made of and this says only
   * which parts of it to drop. One call per mark the sweep crossed, which is
   * almost always one.
   */
  onErased?: (mark: number, remove: number[]) => void;
  /**
   * One mark the same sweep took whole: a highlight, a box, a note, a stamp.
   *
   * Separate from {@link onErased} because it is a different command --- that
   * one drops parts of a drawing and this one takes a mark off its page --- and
   * because a caller given `(mark, [])` would have to know that an empty list
   * means "all of it". One call per mark, as above.
   *
   * Every kind but `ink` arrives here, since nothing else is made of parts. A
   * drawing the reader rubbed every stroke out of still comes through
   * {@link onErased}: what happens to an empty drawing is the backend's
   * decision, and `edits.rs` makes it.
   */
  onUnmarked?: (mark: number) => void;
  /**
   * Called after a jump that Back can undo, so a caller can re-enable a button.
   *
   * Separate from `onPosition`, which fires every frame: this fires only when
   * the *history* changed, which is what a Back and Forward affordance reads.
   */
  onNavigate?: () => void;
  /**
   * The reader dragged out the part of a page they want to keep.
   *
   * `page` is the page's **id** and `rect` is in the file's display space,
   * exactly as {@link onDrawn}'s pair is and for the same reason: the slot, the
   * crop and the two rotations are the viewer's, and a caller undoing them would
   * be a second copy of `fileRectOn`.
   *
   * **It is not a crop box**, and the difference is the whole of why this stops
   * here. A crop box is in the page's own unrotated space, which is one further
   * turn away and needs the page's `/Rotate` --- something this side is never
   * told. The caller asks the backend for that turn. Handing back a display
   * rectangle keeps this callback the same shape as every other gesture's.
   *
   * Optional for {@link onDrawn}'s reason: the window harness drives the drag
   * and reads the preview without a model behind it.
   */
  onCropped?: (page: PageId, rect: [number, number, number, number]) => void;
  /**
   * The reader dragged out a region to be removed from a page.
   *
   * `rect` is `left, top, right, bottom` in the page's **display** space ---
   * the same rectangle {@link onCropped} is handed, from the same drag and the
   * same clamp. Unlike a crop it needs no further turn: the model holds a
   * pending redaction in display space, so this is the number that is stored.
   *
   * **Marking is not removing.** This puts a row in the review list and an
   * outline on the page; `docs/PLAN.md` §6 keeps the destructive step behind a
   * separate command on purpose.
   *
   * Optional for {@link onDrawn}'s reason: the window harness drives the drag
   * and reads the preview without a model behind it.
   */
  onRedacted?: (page: PageId, rect: [number, number, number, number]) => void;
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
 * How opaque a mark's ink is on the overlay, by whether it is a wash or a line.
 *
 * **Two renderers draw these marks and they must agree.** This is the overlay's,
 * painted over a tile; the other is the appearance stream `save.rs` writes,
 * drawn by PDFium once the file has been saved and reopened. `annot-probe`
 * (`--mode ink` for the wash, `--mode rule` for a line) is what says the saved
 * file's ink lands where this one did. `docs/PLAN.md` §10 question 8 is this
 * decision.
 *
 * **The alphas differ from the file's and the colours no longer do.** This was
 * `const MARK_FILL = "rgba(255, 230, 51, 0.85)"` --- the highlight's colour
 * written out a second time, with a comment saying it was `edits.ts`'s
 * `1, 0.9, 0.2`. The overlay now reads {@link MarkView.color}, which is the
 * value that actually went to the model and into the file, so there is one
 * statement of it rather than two that agree today. Measured before the change
 * rather than assumed: `Math.round(0.9 * 255)` is 230, so a highlight is drawn
 * in exactly the pixels the literal named, and a line comes out at
 * `217, 38, 38`, which is the red `annot_probe` measures for a rule.
 *
 * The *alpha* stays ours, because it is a fact about painting over a tile rather
 * than about the mark: the file's wash is 40% and needs to be stronger here. A
 * line is opaque in both, for the reason `is_wash` gives --- a translucent line
 * reads as a smudge.
 */
const WASH_ALPHA = 0.85;
const LINE_ALPHA = 1;

/**
 * Lays down the zigzag of a squiggle, fitted to a band.
 *
 * Traces without painting, for {@link traceEllipse}'s reason: both callers set
 * their own stroke, and the committed mark's colour is not the preview's.
 *
 * **Straight segments, matching `save.rs`.** A curve would look the same at this
 * size and would be a second approximation to keep in step across two languages;
 * `lineTo` and `l` say the same thing exactly. The trough sits half a stroke
 * above the band's foot and the peak half a stroke below its top, so the wave
 * stays inside the band the hit test and the popup anchor use.
 *
 * The last segment is clipped to the band's right edge and interpolated rather
 * than snapped to a peak --- a wave that climbed to full height in a tenth of a
 * period ends on a near-vertical tick, which reads as a stray mark.
 */
function traceSquiggle(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
  thickness: number,
): void {
  const low = top + height - thickness / 2;
  const high = top + thickness / 2;
  const half = (height * SQUIGGLE_PERIOD) / 2;
  ctx.beginPath();
  if (half <= 0 || low <= high) return;
  ctx.moveTo(left, low);
  let x = left;
  let up = true;
  while (x < left + width) {
    const next = Math.min(x + half, left + width);
    const reached = (next - x) / half;
    const from = up ? low : high;
    const to = up ? high : low;
    ctx.lineTo(next, from + (to - from) * reached);
    x = next;
    up = !up;
  }
}

/**
 * Lays down the path of the ellipse inscribed in a rectangle.
 *
 * **Traces, does not paint.** Both callers set their own stroke first --- the
 * committed mark in the reader's colour, the drag preview dashed in
 * `PREVIEW_STROKE` --- and a helper that stroked for them would need a colour
 * argument and a dash argument to say the same two things twice.
 *
 * `ctx.ellipse` rather than the four Bézier arcs `save.rs` writes, and the two
 * agree: `KAPPA` is the approximation a content stream needs because PDF has no
 * ellipse operator, and a canvas has one. Using it here keeps the overlay exact
 * and leaves the approximation in the one place that cannot avoid it.
 *
 * No inset, for the box's reason: the stroke straddles the path and half of it
 * falls outside the rectangle, which is correct on a canvas with no clip and
 * wrong in an appearance stream, whose `/BBox` would cut it away.
 */
function traceEllipse(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
): void {
  ctx.beginPath();
  // Radii must not be negative: the caller normalises its corners, and
  // `Math.abs` is the cheaper half of saying so at the boundary that would
  // throw rather than trusting every future caller to have done it.
  ctx.ellipse(
    left + width / 2,
    top + height / 2,
    Math.abs(width) / 2,
    Math.abs(height) / 2,
    0,
    0,
    Math.PI * 2,
  );
}

/**
 * Draws a comment's icon in the box a mark gives it.
 *
 * **Ours only until the file is saved.** Every reader synthesises its own icon
 * for a `/Text` annotation --- which is why `save.rs` writes no appearance
 * stream for one --- so this is what a reader sees while they are working, and
 * Acrobat's or Preview's bubble is what they see afterwards. The two will not
 * be the same picture. They are in the same place and the same size, which is
 * what `ICON_SIZE` is for, and that is the property worth having: a comment
 * must not move when the file is saved.
 *
 * A rounded box with a tail, outlined in a darker shade of its own fill so it
 * stays visible on yellow paper and on a dark inverted page alike. Drawn with
 * the canvas's own primitives rather than an image, because an icon that needs
 * a resource is an icon that can fail to load.
 */
function drawBubble(
  ctx: CanvasRenderingContext2D,
  left: number,
  top: number,
  width: number,
  height: number,
  stroke = BUBBLE_EDGE,
): void {
  // The tail hangs below the box, so the body takes the upper three quarters
  // and the point sits on the bottom edge of the rectangle the mark owns. That
  // keeps everything drawn inside the quad, which is the same discipline
  // `markBand` follows for a line and for the same reason: the saved `/Rect` is
  // what other readers clip their own icon to.
  const body = height * 0.78;
  const radius = Math.min(width, body) * 0.28;
  const fill = ctx.fillStyle;

  ctx.beginPath();
  ctx.roundRect(left, top, width, body, radius);
  ctx.moveTo(left + width * 0.28, top + body);
  ctx.lineTo(left + width * 0.3, top + height);
  ctx.lineTo(left + width * 0.52, top + body);
  ctx.closePath();
  ctx.fill();

  // The outline, and it is not decoration: the fill is the mark's own colour,
  // which on a yellow-ish page or under a highlight is close to invisible. A
  // stroke at 55% of the fill against black gives an edge in every combination
  // without introducing a second colour to keep in step with `MARK_COLORS`.
  ctx.save();
  ctx.strokeStyle = stroke;
  ctx.lineWidth = Math.max(1, Math.min(width, height) * 0.06);
  ctx.stroke();
  ctx.restore();
  ctx.fillStyle = fill;
}

/** A mark's colour as the canvas wants it, from the value the model holds. */
function markInk(color: readonly [number, number, number], wash: boolean): string {
  const [r, g, b] = color.map((v) => Math.round(v * 255));
  return `rgba(${r}, ${g}, ${b}, ${wash ? WASH_ALPHA : LINE_ALPHA})`;
}
const SELECTION_FILL = "rgba(80, 140, 255, 0.35)";
/**
 * The dashed rectangle a drag draws before it is committed.
 *
 * The selection's blue rather than the mark's red: a preview is the
 * application's own feedback about a gesture in progress, in the colour
 * everything else about a gesture in progress uses, and drawing it in the
 * mark's colour would make a half-drawn box indistinguishable from a made one
 * at a glance.
 */
const PREVIEW_STROKE = "rgba(80, 140, 255, 0.95)";
/**
 * The wash inside the comment tool's ghost bubble.
 *
 * {@link PREVIEW_STROKE}'s blue at a fifth, for {@link paintCommentGhost}'s
 * reason: a dashed outline of a 20 pt icon is a handful of marks and is hard to
 * see over a dense page, and anything more opaque hides the words the reader is
 * choosing between.
 */
const GHOST_FILL = "rgba(80, 140, 255, 0.2)";
/**
 * The wash over the part of a page a crop would throw away.
 *
 * **Grey and not the preview blue, because it means the opposite thing.** Every
 * other preview in this file marks what the reader is *making*; this marks what
 * they are about to lose, and the kept rectangle is the hole in it. A blue scrim
 * would read as "this is the selection" over exactly the region that is not.
 *
 * Dark rather than light so that it works on a white page, which is nearly all
 * of them, and at a third so the discarded content stays legible --- a reader
 * adjusting a crop is looking at what falls outside it as much as at what falls
 * in.
 */
const CROP_SCRIM = "rgba(0, 0, 0, 0.33)";
/**
 * What a pending redaction is drawn in, before it is applied.
 *
 * **Not black**, which is the one colour it must not be. A black rectangle over
 * words is what a redaction that does not work looks like --- the failure this
 * whole subsystem exists to refuse --- so a *pending* one has to read as an
 * instruction rather than as a result. Red at a third says "this is going" while
 * leaving the words underneath readable, which is what step 2 is for: a reader
 * checking a region cannot check one they cannot see through.
 *
 * The applied result is not drawn by this file at all. It is whatever the page
 * becomes once the content is gone, rendered by PDFium like any other page.
 */
const REDACT_FILL = "rgba(190, 30, 45, 0.30)";
/** The edge of a pending redaction. The same red, opaque, so the region has a
 * definite boundary a reader can judge an over-selection against. */
const REDACT_EDGE = "rgba(190, 30, 45, 0.95)";
/**
 * The outline a committed comment's bubble is drawn with.
 *
 * A parameter of {@link drawBubble} with this as its default rather than a
 * literal inside it, because the ghost the comment tool follows the pointer
 * with is the same shape in a different colour --- and a second copy of the
 * shape is how the preview and the mark come to disagree about where the tail
 * hangs. See {@link paintCommentGhost}.
 */
const BUBBLE_EDGE = "rgba(0, 0, 0, 0.55)";
/**
 * Overlay fills, all painted with `multiply` so the glyphs stay legible.
 *
 * The current match is a different hue rather than a darker shade of the same
 * one: on a page with many hits, "which of these am I on" has to survive being
 * read at a glance and next to a highlight of the other colour.
 */
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
  /** Every comment in the document, in page order. Empty until it arrives. */
  private commentItems: readonly Comment[] = [];
  private linkItems: readonly Link[] = [];
  /**
   * The links of one page, already turned into the view's space.
   *
   * Memoised because a hover hit-tests on every pointer move, and `turnedFor`
   * allocates: on a page carrying the per-page maximum that is 4,000 objects
   * per mouse movement. Keyed by both things that invalidate it --- which page,
   * and how the view is turned --- rather than cleared from the two places that
   * change them, which is a rule someone has to keep following.
   */
  private turnedLinks: { page: number; turns: number; items: Link[] } | null = null;
  /** Whether the pointer is currently over a link, so the cursor is set once. */
  private overLink = false;
  private readonly history = new History();
  /** True while the history is driving a jump, so it is not re-recorded. */
  private replaying = false;
  /**
   * Every link in the order a reader meets them, computed once per document.
   *
   * Kept beside `linkItems` rather than sorted on demand: a keyboard reader
   * presses "next link" repeatedly, and re-sorting a document's worth of links
   * on each press is work proportional to how fast they are reading.
   */
  private linkWalk: readonly Link[] = [];
  /** The link the keyboard is on, or `null`. */
  private focusedLink: Link | null = null;
  /** The ring drawn over the focused link. */
  private readonly ring: HTMLElement;
  /** The note shown on the page, built once and reused. */
  private readonly popup: CommentPopup;
  private readonly markNote: MarkPopup;

  /**
   * The tool a reader armed, or `null` when a press means what it always did.
   *
   * **A mode, in an application whose stated principle is contextual actions
   * rather than modes** (`docs/PLAN.md` §8), so it is worth saying why there is
   * no alternative. Every existing gesture on a page reads a point and acts on
   * what is under it; drawing reads two points and acts on the paper between
   * them, and nothing in a press can distinguish "select this text" from "draw
   * a box here" without being told first.
   *
   * What the principle *does* decide is that it is **one-shot**: armed by a
   * command, spent by one rectangle, and dropped by Escape or by the document
   * closing. A reader can never be stuck in it and never has to find the way
   * out, which is the failure the principle is actually about. A tool that
   * stays armed is the obvious next step and is a decision, not an oversight.
   */
  private drawKind: MarkKind | null = null;
  /** Which stamp an armed stamp tool will place. `null` for every other kind. */
  private drawStamp: StampName | null = null;
  /**
   * The finished strokes of the drawing in progress, and the page they are on.
   *
   * **Ink is the first tool that is not one-shot, and the reason is the format
   * rather than a preference.** `/InkList` is a list of lists precisely so that
   * one annotation holds several strokes, and a drawing normally is several: a
   * circle and the arrow into it, a crossed-out word, anything handwritten. The
   * writer, the model and `annot-probe --mode strokes` were all built for that
   * from the start --- the probe sends two strokes --- and until this existed the
   * window could produce exactly one, so the harness was creating a document no
   * reader of tpdf could.
   *
   * The slot is held with them because every stroke of one drawing must land on
   * one page: an annotation belongs to a page, so a second stroke started on the
   * next page down is not part of this mark. It is refused rather than moved.
   */
  private inking: { slot: number; strokes: Point[][] } | null = null;

  /**
   * Whether the eraser is the armed tool.
   *
   * Separate from {@link drawKind} rather than a seventh `MarkKind`, because it
   * is not a kind of mark: nothing it does creates one. Arming either closes the
   * other, so the two are never both set --- which is asserted by the window
   * check rather than by a type, since the states live in one object nobody
   * constructs by hand.
   */
  private erasing = false;

  /**
   * What the sweep in progress has touched: strokes by mark id, and whole marks.
   *
   * **Accumulated across the whole drag and committed on release**, exactly as
   * {@link inking} accumulates strokes and commits on Enter. A reader sweeping
   * across four strokes did one thing, so it is one call and one undo; sending
   * each stroke as it is touched would cost four presses of undo to put back a
   * gesture that took one movement of the hand.
   *
   * The doomed strokes stop being painted the moment they are added, which is
   * the whole of the preview --- there is no separate ghost to keep in step.
   *
   * **Two sets rather than one, because they are two commands.** A drawing
   * loses strokes and survives, which is `Erase`; every other kind has no part
   * to lose, so the nib takes the mark, which is `Unannotate`. Folding them
   * into one map --- an empty stroke set standing for "all of it" --- would put
   * that distinction in a caller's head instead of in the type, and the caller
   * that reads it wrong sends the wrong command.
   */
  private doomed: {
    slot: number;
    marks: Map<number, Set<number>>;
    /** Marks the nib crossed that are taken whole, by id. */
    whole: Set<number>;
    /** Where the nib was at the last report, so the sweep tests its travel. */
    last: Point;
  } | null = null;

  /**
   * The rectangle being dragged, in the slot's laid-out space.
   *
   * Held rather than recomputed from the drag, because the overlay paints it
   * once a frame and the drag reports client coordinates --- which mean
   * something different after a scroll. Both corners are stored on the page, so
   * a preview follows the page rather than the window.
   */
  private drawing: {
    slot: number;
    from: Point;
    to: Point;
    /**
     * Every point the pointer visited, for ink. `[from]` for a box, unused.
     *
     * Kept beside `from`/`to` rather than replacing them, because the two are
     * still what a box's preview and its committed quad are built from --- and
     * because {@link drawPreview}, which the window harness reads, is about a
     * rectangle. A drawing's preview is the same rectangle, growing as the hand
     * moves, which is what tells a reader the tool is live.
     */
    points: Point[];
  } | null = null;

  /**
   * Whether the crop tool is armed: the next drag on a page crops it.
   *
   * A boolean and not a {@link MarkKind}, for {@link erasing}'s reason --- it
   * makes no mark. Arming it puts any drawing tool away and vice versa, so at
   * most one of the three is live and no gesture has to ask which meant it.
   */
  private cropping = false;

  /**
   * Whether the redaction tool is armed: the next drag marks a region for
   * removal.
   *
   * **Beside {@link cropping} rather than sharing it**, even though the two
   * gestures are the same drag over the same clamp. What a drag *commits* is
   * the whole difference between them --- a crop hides part of a page and a
   * redaction is a step towards destroying it --- so a single flag with a
   * destination read from somewhere else is one bug away from cropping when the
   * reader asked to redact. Both are cleared by every arming method, so at most
   * one is ever live.
   *
   * The drag itself is shared, which is the other half of the same reasoning:
   * `armDraw`'s note says a second method would be a second copy of the whole
   * gesture, and that argument does not stop at the third tool.
   */
  private redacting = false;

  /**
   * The rectangle being dragged for a crop, in the slot's laid-out space.
   *
   * Its own field rather than a mode inside {@link drawing}, because the two are
   * committed by different callbacks and clamped against the same page for
   * different reasons --- and because a preview painted from one field that
   * sometimes means a mark and sometimes means a crop is the "one predicate
   * answering two questions" trap wearing a rectangle.
   */
  private cropDrawing: { slot: number; from: Point; to: Point } | null = null;

  /**
   * The crop's drag, which owns its own listener pair.
   *
   * A third `PointerDrag` for the reason there are two: each one's `begin`
   * refuses a press the others want. This one refuses unless the crop tool is
   * armed, `drawDrag` refuses unless a drawing tool is, and `moveDrag` refuses
   * unless the press landed on a mark the reader may move. One instance would
   * have to ask which of the three it was at every callback.
   */
  private readonly cropDrag: PointerDrag;
  /**
   * Where the pointer is, in the page's own space, while a tool is armed.
   *
   * Only the comment tool reads it, and only to paint the ghost bubble that says
   * where a press would land --- {@link paintDrawing}. Recorded for every armed
   * kind rather than for that one, because the cost is two numbers per pointer
   * move and the alternative is a condition here that has to be kept in step
   * with the condition that paints.
   *
   * `null` when nothing is armed, and when the pointer has left the surface: a
   * ghost left behind at the last place the pointer was seen is a bubble drawn
   * where a press cannot happen.
   */
  private armedAt: { slot: number; x: number; y: number } | null = null;

  /**
   * The mark being dragged to a new place, and how far it has come.
   *
   * `dx`/`dy` are in the page's own display space and are what the overlay adds
   * to the mark's quads while the drag is live --- the mark is *not* moved in
   * the model until the pointer comes up, so an Escape or a `pointercancel`
   * puts it back by clearing this and nothing else. Same posture as
   * {@link drawing}: what is on screen mid-gesture is a preview, and the model
   * hears once.
   *
   * `slot` is where the mark is laid out, held so that the clamp has a page size
   * to work against without asking which page the pointer has since wandered on
   * to --- a mark belongs to one page, exactly as a box does.
   */
  private moving: {
    id: number;
    slot: number;
    from: Point;
    dx: number;
    dy: number;
  } | null = null;

  private readonly moveDrag: PointerDrag;

  /**
   * The box's drag, which owns its own listener pair.
   *
   * Constructed once, in the constructor, because it registers nothing until
   * {@link PointerDrag.start} takes a press --- an idle one costs a field.
   */
  private readonly drawDrag: PointerDrag;

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

  /**
   * Which page of the file is in which slot, and everything derived from it.
   *
   * The one copy of the translation on this side. Replaced whole by
   * {@link setPages} when the model answers, never edited --- see `pages.ts`.
   */
  private pages: PageMap;

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

    // A document nobody has edited, which is what every document is at the
    // instant it opens. Replaced by the model's own answer as soon as
    // `edit_state` returns --- `App.svelte` deliberately does not hold the first
    // page behind that round trip, so the viewer needs an order before it.
    this.pages = unedited(opts.pageCount);
    this.text = new TextCache(opts.doc);
    this.a11y = new AccessibleText(root, opts.pageCount);
    this.searcher = new Search(
      opts.doc,
      opts.pageCount,
      () => this.onSearchProgress(),
      (slot) => this.pages.sourceOf(slot),
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

    // Built once and reused: it is one element, it is hidden when nothing is
    // shown, and rebuilding it per comment would lose the reader's scroll
    // position inside a long note every time the page moved.
    // Hosted by the root rather than by the tile surface: the surface is
    // `aria-hidden`, and a dialog inside it would be unreachable by a screen
    // reader --- which is the one reader who cannot see the mark it is anchored
    // to either.
    this.popup = new CommentPopup(root, () => this.closeComment());

    // The reader's own marks get their own box, for the reason `markpopup.ts`
    // gives. Hosted by the root and built once, exactly as above.
    this.markNote = new MarkPopup(root, {
      onNote: (mark, note) => this.opts.onMarkNote?.(mark, note),
      onRecolor: (mark, color) => this.opts.onMarkRecolor?.(mark, color),
      onRemove: () => this.removeOpenMark(),
      onClose: () => this.closeMark(),
      onOpen: (mark) => this.opts.onMark?.(mark),
    });

    // The keyboard's position on the page, drawn as an outline over the focused
    // link. Hosted by the root rather than the tile surface for the same reason
    // the popup is: the surface is `aria-hidden`.
    //
    // A drawn element rather than a rectangle on the overlay canvas, because the
    // canvas is repainted by the render loop and this has to survive a frame in
    // which nothing rendered --- and because `outline-offset` and the focus
    // colour are then the platform's rather than ours to guess.
    this.ring = document.createElement("div");
    this.ring.setAttribute("aria-hidden", "true");
    this.ring.style.cssText =
      "position:absolute;display:none;pointer-events:none;z-index:4;" +
      "border-radius:2px;outline:2px solid Highlight;outline-offset:2px;" +
      "background:color-mix(in srgb, Highlight 12%, transparent);";
    root.appendChild(this.ring);

    // The box's drag. Its three callbacks are the whole of the gesture; the
    // arithmetic they call is in `markband.ts`, which the file's writer mirrors.
    // **Its own `PointerDrag`, not a mode inside the one below.** The two read
    // the same events and mean opposite things: `drawDrag` refuses a press that
    // lands on nothing armed, and this one refuses a press that does *not* land
    // on a mark the reader may move. One instance would have to ask which it was
    // at every callback, which is the shape of the "one predicate answering two
    // questions" trap --- and `PointerDrag` deliberately allows only one live
    // drag per instance, so two gestures need two of them.
    this.moveDrag = new PointerDrag(root, {
      begin: (_at: DragPoint) => {
        // Never starts itself: `onSelectStart` decides which mark was pressed and
        // sets `moving` before calling `start`, because that decision needs the
        // hit-test ladder --- a comment of the document's outranks a mark of the
        // reader's at a shared point --- and this callback sees only a pair of
        // coordinates.
        return this.moving !== null;
      },
      move: (at: DragPoint) => {
        const live = this.moving;
        if (!live) return;
        // The page the *mark* is on, not the page under the pointer. A mark
        // belongs to one page and cannot be dragged onto the next --- the same
        // rule the box's drag states, and here it is stronger: an annotation
        // moved across a page boundary would have to change which page object
        // owns it, which is a different command from moving it.
        const now = this.pointOn(live.slot, at);
        const want = { dx: now.x - live.from.x, dy: now.y - live.from.y };
        const bound = this.clampMove(live.id, live.slot, want);
        if (bound.dx === live.dx && bound.dy === live.dy) return;
        live.dx = bound.dx;
        live.dy = bound.dy;
        this.wake();
      },
      end: (_at: DragPoint, committed: boolean) => {
        const live = this.moving;
        this.moving = null;
        this.wake();
        if (!live || !committed) return;
        // **A press that did not move is not a move.** It is how a reader opens
        // a note, which `onSelectStart` has already done on the way in --- so a
        // zero offset here would journal a command for a click, and undo would
        // then step through every note the reader had ever opened.
        if (live.dx === 0 && live.dy === 0) return;
        // Into the model's space at the last possible moment. Everything above
        // is laid-out points, because that is the space the reader's hand and
        // the page's edge are both in; the model holds the file's.
        const sent = this.fileDelta(live.slot, live.dx, live.dy);
        this.opts.onMarkMoved?.(live.id, sent.dx, sent.dy);
      },
    });

    // The crop's drag. Deliberately the plainest of the three: it has no
    // sampling, no per-page refusal beyond the first press, and nothing to keep
    // between drags --- a crop replaces whatever crop the page had, so there is
    // no equivalent of ink's several strokes.
    this.cropDrag = new PointerDrag(root, {
      begin: (at: DragPoint) => {
        if (!this.cropping && !this.redacting) return false;
        const { page, x, y } = this.pageAndPoint(at);
        this.cropDrawing = { slot: page, from: { x, y }, to: { x, y } };
        this.wake();
        return true;
      },
      move: (at: DragPoint) => {
        const live = this.cropDrawing;
        if (!live) return;
        // The page the drag *started* on, not the one under the pointer. A crop
        // is a property of one page --- cropping two at once is a different
        // command --- so a drag that wanders onto the next is clamped to the
        // first by `boxQuad`, exactly as a box's is.
        const { x, y } = this.pageAndPoint(at);
        live.to = { x, y };
        this.wake();
      },
      end: (_at: DragPoint, committed: boolean) => {
        const live = this.cropDrawing;
        this.cropDrawing = null;
        this.wake();
        if (!committed || !live) {
          // Cancelled. The tool goes with it, because Escape means "stop" and
          // `cancelDraw` has already cleared it --- this is the browser's own
          // `pointercancel` arriving by the same door.
          this.cropping = false;
          this.redacting = false;
          this.showCursor();
          return;
        }
        // `boxQuad` and not a crop-specific clamp: the two want exactly the
        // same thing --- a proper rectangle, inside the page, big enough to
        // have been dragged rather than clicked --- and a second copy would be
        // a second bound to keep in step with `MIN_BOX`.
        //
        // A four-point crop is a legal crop and a useless one, but that is the
        // reader's rectangle and it is undoable and plainly visible. What the
        // bound is for is separating a click from a drag, which is the same
        // separation a mark needs.
        const quad = boxQuad(live.from, live.to, this.laidSize(live.slot));
        const id = quad ? this.pages.idOf(live.slot) : undefined;
        if (!quad || id === undefined) {
          // A click rather than a drag, and the tool stays armed --- `boxQuad`'s
          // own note says why. Spending it here would make a slipped click cost
          // the reader the command with nothing on screen to say so.
          return;
        }
        // Which of the two was armed, read before either is cleared. Both are
        // spent here and cleared *before* the callback for `onDrawn`'s reason:
        // a caller that arms something again must not be undone by these lines.
        const marking = this.redacting;
        this.cropping = false;
        this.redacting = false;
        this.showCursor();
        // The same display rectangle either way, and the callbacks part company
        // there: `onCropped`'s caller turns it into a crop box, which needs the
        // page's own `/Rotate` and a further turn; `onRedacted`'s does not,
        // because the model holds a redaction in exactly this space.
        const rect = this.fileRectOn(live.slot, quad);
        if (marking) this.opts.onRedacted?.(id, rect);
        else this.opts.onCropped?.(id, rect);
      },
    });

    this.drawDrag = new PointerDrag(root, {
      begin: (at: DragPoint) => {
        if (!this.drawKind && !this.erasing) return false;
        const { page, x, y } = this.pageAndPoint(at);
        if (this.erasing) {
          // The page the sweep started on, and it does not move --- the same
          // rule the box's drag states below, for the same reason: a mark
          // belongs to one page, so a sweep that wanders onto the next one
          // erases nothing there rather than something the reader could not see
          // themselves aiming at.
          this.doomed = {
            slot: page,
            marks: new Map(),
            whole: new Set(),
            last: { x, y },
          };
          this.sweep(x, y);
          this.wake();
          return true;
        }
        // **A drawing lives on one page**, so a second stroke that starts on a
        // different one is refused rather than moved onto the first. Refusing
        // lets the press go on to whatever would have had it --- a selection, a
        // link --- which is the honest answer: the reader pressed somewhere this
        // drawing cannot reach, and silently dragging their stroke a page
        // upwards would be worse than doing nothing.
        if (this.inking && this.inking.slot !== page) return false;
        this.drawing = {
          slot: page,
          from: { x, y },
          to: { x, y },
          points: [{ x, y }],
        };
        this.wake();
        return true;
      },
      move: (at: DragPoint) => {
        if (this.doomed) {
          const { x, y } = this.pageAndPoint(at);
          this.sweep(x, y);
          this.wake();
          return;
        }
        const live = this.drawing;
        if (!live) return;
        // The page is the one the drag *started* on and is deliberately not
        // re-read here. A box spanning two pages is not a thing a PDF can hold
        // --- an annotation belongs to one page --- so a drag that wanders onto
        // the next one is clamped to the first rather than silently moving.
        const { x, y } = this.pageAndPoint(at);
        live.to = { x, y };
        // **Sampled, not every event.** A pointer reports at the display's rate
        // and a slow hand produces dozens of points inside one millimetre, none
        // of which changes the line and all of which go into the file and over
        // the IPC boundary. `INK_SAMPLE` is in the page's own points, so the
        // spacing is the same on the paper whatever the zoom --- sampling in
        // client pixels would put four times as many points in a stroke drawn
        // at 400% as in the identical stroke drawn at 100%.
        const last = live.points[live.points.length - 1];
        if (
          !last ||
          Math.abs(x - last.x) >= INK_SAMPLE ||
          Math.abs(y - last.y) >= INK_SAMPLE
        ) {
          live.points.push({ x, y });
        }
        this.wake();
      },
      end: (_at: DragPoint, committed: boolean) => {
        const swept = this.doomed;
        if (swept) {
          this.doomed = null;
          this.wake();
          // Cancelled mid-sweep: the strokes come back, because nothing was
          // sent. `cancelDraw` has already disarmed the tool if this arrived by
          // Escape; a `pointercancel` leaves it armed, which is right --- the
          // reader did not ask to stop erasing.
          if (!committed) return;
          for (const [mark, strokes] of swept.marks) {
            if (strokes.size === 0) continue;
            // Sorted so that the list a reader's gesture produces does not
            // depend on the order their hand happened to cross the strokes.
            // Nothing downstream requires it; a diagnostic quoting the list is
            // readable because of it.
            this.opts.onErased?.(mark, [...strokes].sort((a, b) => a - b));
          }
          // One call per mark, matching the loop above: a sweep that crossed a
          // highlight and a stamp did two things to the document and the reader
          // undoes them one at a time, which is what the eraser has always done
          // to two separate drawings.
          for (const mark of swept.whole) this.opts.onUnmarked?.(mark);
          return;
        }
        const live = this.drawing;
        const kind = this.drawKind;
        this.drawing = null;
        this.wake();
        if (!committed || !live || !kind) {
          // Cancelled. The tool goes with it, because Escape means "stop", and
          // `cancelDraw` has already cleared it --- this is the browser's own
          // `pointercancel` arriving by the same door.
          this.drawKind = null;
          this.showCursor();
          return;
        }
        if (kind === "ink") {
          // **The last point is added unconditionally**, because the sample
          // above may have dropped it: a stroke that ends with a short movement
          // would otherwise stop where the last kept sample was, which shortens
          // every line by up to `INK_SAMPLE` and is most visible on the short
          // strokes where it matters least to the eye and most to a tick or a
          // cross.
          const last = live.points[live.points.length - 1];
          if (!last || last.x !== live.to.x || last.y !== live.to.y) {
            live.points.push({ ...live.to });
          }
          // Two points is the minimum a stroke can be drawn from, and it is the
          // same bound `Stroke::is_drawable` applies in the model --- stated
          // here as well so that a press that never moved keeps the tool armed
          // rather than spending it on a refusal the reader cannot see. A click
          // is not a failure; it is a reader who has not started yet.
          if (live.points.length < 2) return;
          // **Kept, not committed.** The tool stays armed and the stroke joins
          // the drawing; Enter finishes it and Escape throws it away. That is
          // the whole difference from the box, and it is what `/InkList` being
          // a list of lists is for. See `inking`.
          this.inking ??= { slot: live.slot, strokes: [] };
          this.inking.strokes.push(live.points);
          this.wake();
          return;
        }
        // **A comment is placed, not dragged out, so it reads the press and not
        // the rectangle.** Every other armed kind needs two corners and refuses
        // a click, which is right for a shape and wrong for a pin: a reader
        // choosing *Add comment* has already said what they want and is only
        // saying where. `live.from` rather than the release point, so a hand
        // that slips a pixel between press and release still drops the bubble
        // where it was aimed --- and so that the ghost the reader was watching
        // is the mark they get.
        const quad =
          kind === "note"
            ? iconQuad(live.from.x, live.from.y, this.laidSize(live.slot))
            : boxQuad(live.from, live.to, this.laidSize(live.slot));
        const id = quad ? this.pages.idOf(live.slot) : undefined;
        if (!quad || id === undefined) {
          // **A click rather than a drag, and the tool stays armed.** Silent,
          // because nothing went wrong --- `boxQuad` refuses a rectangle too
          // small to be one and the reader simply tries again. Spending the
          // tool here would make a slipped click cost them the command as well,
          // and there is nothing on screen to say why.
          return;
        }
        // Spent, and cleared *before* the callback so that an `onDrawn` which
        // arms it again is not undone by this line. The stamp is read out first
        // for the same reason and cleared with the kind: the two describe one
        // armed tool and must be spent together.
        const stamp = this.drawStamp;
        this.drawKind = null;
        this.drawStamp = null;
        this.showCursor();
        this.opts.onDrawn?.(
          kind,
          id,
          { quads: this.fileRectOn(live.slot, quad), strokes: [] },
          stamp,
        );
      },
    });

    root.addEventListener("wheel", this.onWheel, { passive: false });
    root.addEventListener("keydown", this.onKeyDown);
    root.addEventListener("pointerdown", this.onSelectStart);
    // Always on, unlike the drag's own `pointermove`: a reader has to be told a
    // run of text is a link *before* pressing it, and the press is too late.
    root.addEventListener("pointermove", this.onHover);
    this.track.addEventListener("pointerdown", this.onTrackPointerDown);

    this.observer = new ResizeObserver(() => this.onResize());
    this.observer.observe(root);

    this.wake();
  }

  destroy(): void {
    // First, so anything that lands during the teardown below finds it set.
    this.life.end();
    this.stop();
    // Before the listeners go: an open note is a `position:absolute` box over a
    // surface that is about to be replaced, and the next document's first frame
    // would otherwise paint under somebody else's comment.
    this.popup.hide();
    // Without committing. A document being closed is not a reader finishing a
    // sentence, and the model behind this viewer is going away with it --- the
    // note would be sent to a handle nobody holds.
    this.markNote.hide(false);
    this.clearLinkFocus();
    clearTimeout(this.retryTimer);
    this.a11y.destroy();
    this.searcher.cancel();
    this.observer.disconnect();
    this.root.removeEventListener("wheel", this.onWheel);
    this.root.removeEventListener("keydown", this.onKeyDown);
    this.root.removeEventListener("pointerdown", this.onSelectStart);
    this.root.removeEventListener("pointermove", this.onHover);
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
    // And the box's, which owns its own listener pair rather than the two
    // above. `dispose` ends a live drag without committing it, which is the
    // right answer for a document being closed mid-gesture: the model it would
    // have sent the box to is going away with it.
    this.moveDrag.dispose();
    this.drawDrag.dispose();
    this.cropDrag.dispose();
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
    this.syncComment();
    this.syncMark();
    if (this.focusedLink) this.placeRing();
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
      // `PageText` reports the page as *displayed*, so every turn in force has
      // to come back out before this is the document's geometry --- the page's
      // own edit turn as well as the view's, since `TextCache.peek` applies
      // both. The two are the same thing at an even number of quarter-turns,
      // which is why getting it wrong is invisible until somebody rotates a
      // mixed document. Measured wrong before the page turn was included: a
      // page turned before it had ever been on screen learned its size
      // transposed, 800x600 for a 600x800 page, and kept it.
      const shown = { width_pt: text.width_pt, height_pt: text.height_pt };
      if (
        this.scroller.notePageSize(
          page,
          displayedSize(shown, -this.scroller.effectiveTurns(page)),
        )
      ) {
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
      drawing: this.drawnStrokes,
      erasing: this.swept,
      // Never while `drawing` is reporting. `drawnStrokes` answers `0` for an
      // armed pen as well as for one mid-drawing, so the two would otherwise
      // both name ink and the window would say it twice.
      // The crop and the redaction first, because none of the three can be set
      // together --- every arming method clears the other two --- so the order
      // is a statement about which is checked, not about which wins.
      armed: this.redacting
        ? "redact"
        : this.cropping
          ? "crop"
          : this.drawnStrokes === null
            ? this.drawKind
            : null,
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
      // **The three mode fields, and they were missing.** This string is what
      // decides whether `onStatus` fires at all, so a field absent from it is a
      // field the window is told about only when something else happens to move
      // --- and a tool being armed moves nothing else: no tile is pending, the
      // selection is empty, the page and the zoom are where they were. So the
      // line that exists to make a mode visible could be a frame late, or never
      // arrive. Found while adding `armed`, which would have inherited it.
      status.drawing,
      // Both halves, because either can move on its own: a sweep across a
      // drawing and a highlight changes one number per mark it crosses, and a
      // field left out of this string is a field the window is told about only
      // when something else happens to move.
      status.erasing?.strokes ?? null,
      status.erasing?.marks ?? null,
      status.armed,
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
    const page = this.currentPage();
    // Both turns, for the reason `Scroller.effectiveTurns` gives: a fit is about
    // the sheet in front of the reader, and a page an edit has turned is a
    // different shape from the one the file describes. Through that method
    // rather than adding here, which is what the method exists to stop: this
    // line held the only other copy of the sum, and six further places held the
    // view's half of it alone and were wrong.
    return displayedSize(this.scroller.pageSize(page), this.scroller.effectiveTurns(page));
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
   * The overlay canvas, so a window check can read what a mark actually looks
   * like.
   *
   * **This exists because there was no check on the overlay at all, and the
   * defect that fact allowed was reported by a reader.** Every mark was filled
   * across its whole quad in one colour, so an underline and a strikeout both
   * appeared as a highlight while the document was open --- and the saved file
   * was correct the whole time, which made it the worse shape of wrong: the mark
   * changed under the reader when they saved and reopened it. `annot-probe`
   * measures the *file's* pixels and can say nothing about these.
   *
   * Separate from {@link compositedSurface}, which is the tile surface: the
   * marks are drawn on a canvas above it, and reading the wrong one would report
   * the page rather than what is over it.
   */
  get overlaySurface(): HTMLCanvasElement {
    return this.overlay;
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

  /**
   * Turns one page of the *document*, which is what a save writes.
   *
   * Distinct from {@link rotateBy} in the only way that matters: that one
   * changes how the whole document is looked at and writes nothing, and this one
   * changes one page and makes the document differ from the file on disk. They
   * compose --- a page turned by an edit inside a view turned by the reader is
   * turned by the sum, and the scroller and the text layer are handed the same
   * number so they cannot disagree about which.
   *
   * The model is the authority: this is told what a page's turn *is*, never by
   * how much to change it, because the arithmetic belongs to the journal that
   * has to replay it. `App.svelte` calls the backend and hands the answer here.
   *
   * Re-anchors like {@link rotateBy}, and for the same reason --- a turned page
   * is a different height, so everything below it has moved and a reader left at
   * the same offset is looking somewhere else. Refits for the same reason too:
   * under fit-page a page that has just become landscape is otherwise shown at a
   * scale chosen for the portrait one.
   */
  setPageTurns(page: number, turns: number): void {
    if (page < 0 || page >= this.opts.pageCount) return;
    if (this.scroller.pageExtraTurns(page) === (((turns % 4) + 4) % 4)) return;

    const anchor = this.currentPage();
    const before = this.scroller.pagePitchOf(anchor);
    const through =
      before > 0
        ? (this.scrollTop - this.scroller.pageTopOf(anchor)) / before
        : 0;

    // The text cache is keyed by the page of the file; the scroller by the slot.
    // Both are told the same number of turns, which is what stops the tiles and
    // the caret disagreeing about which way a page is facing.
    const source = this.pages.sourceOf(page);
    if (source !== undefined) this.text.setPageTurns(source, turns);
    this.scroller.setPageTurns(page, turns);
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

  /** Quarter-turns an edit has applied to a page, 0 to 3. For the checks. */
  pageExtraTurns(page: number): number {
    return this.scroller.pageExtraTurns(page);
  }

  /**
   * The working document's pages, in slot order. For the check harness.
   *
   * A copy, so that a caller building the next order out of this one --- which is
   * what a check does --- cannot edit the map in place and leave the viewer
   * believing it has already applied it.
   */
  get pageOrder(): PageView[] {
    return [...this.pages.pages];
  }

  /**
   * Takes the working document's pages, whatever changed about them.
   *
   * The single entry point for an edit reaching the viewer: `App.svelte` calls
   * the backend and hands the whole answer here, exactly as {@link setPageTurns}
   * is handed a turn rather than a delta. Nothing here computes what the next
   * order should be.
   *
   * **Two paths, and the branch is the order rather than the turns.** A document
   * whose pages are the same pages in the same slots has at most had some of them
   * turned, and a turn is a change the layout absorbs in place while keeping the
   * reader where they were --- that is {@link setPageTurns}, which is left to do
   * it. A document whose *order* moved has invalidated everything keyed by a
   * slot: the tiles, the placeholders, the links, the hits, the selection. That
   * is the second path, and it is deliberately blunt.
   *
   * Returns whether the *order* moved, which is what tells the caller that
   * everything else it holds about pages --- the links, the comments, the
   * outline, the page strip --- has to be translated again. Answered here rather
   * than compared again by the caller, so there is one definition of "the pages
   * moved" and not two: a comparison of page *counts* is the same answer today
   * and stops being one the moment a page can be reordered.
   */
  setPages(views: readonly PageView[]): boolean {
    const before = this.pages;
    const after = new PageMap(views);
    this.pages = after;

    // Before either branch below, because a crop can change without the order
    // changing --- which is the common case, since cropping one page moves no
    // page --- and the early return would then leave the text cache holding an
    // extraction measured under the box the reader just replaced.
    void this.adoptCrops(after);

    if (before.sameOrder(after)) {
      // Every slot, not the ones that differ: `setPageTurns` returns early for a
      // page whose turn has not moved, so the comparison it would take to avoid
      // the call is the comparison it already does.
      for (let slot = 0; slot < after.length; slot++) {
        this.setPageTurns(slot, after.turnsOf(slot));
      }
      return false;
    }

    // Where the reader is, by identity rather than by slot --- the whole point
    // of this path is that slots have moved. A reader standing on the page that
    // was just deleted has nowhere to be put back to, so they stay at the slot
    // number they were on, which now holds the page that followed it. That is
    // what a reader who deletes the page they are looking at expects to see.
    const wasAt = this.currentPage();

    this.opts.pageCount = after.length;
    this.scroller.setPages([...views]);
    this.a11y.setPages(after.length);
    this.searcher.setPages(after.length);

    // Keyed by a slot, and every slot after the change holds a different page.
    // A selection left alone would highlight a run of characters on a page
    // nobody selected; the ring and the open note would point at pages that have
    // moved out from under them.
    this.clearSelection();
    this.clearLinkFocus();
    this.closeComment();
    this.turnedLinks = null;

    // The slot the page the reader was on has moved to, or --- if that is the
    // page they just deleted --- the slot number they were on, which now holds
    // whatever followed it.
    const landing =
      after.slotFrom(before, wasAt) ??
      Math.min(wasAt, Math.max(0, after.length - 1));
    this.applyFit();
    this.scrollTop = Math.max(
      0,
      Math.min(this.scroller.pageTopOf(landing), this.scroller.maxScroll),
    );
    this.wake();
    return true;
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
    const offset =
      this.scroller.effectiveTurns(page) === 0 ? Math.max(0, place.top_pt) : 0;
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
   * A page under a quarter turn reports the page and no offset, for the same
   * reason {@link goToDestination} ignores one: the destinations this is
   * compared against are measured down an upright page, and under a quarter
   * turn that is not the axis being scrolled. Outline highlighting falls back to
   * page granularity, which is coarse and right, rather than fine and wrong.
   *
   * **Either turn counts**, which is what `effectiveTurns` is asked rather than
   * the view's own number: a page the reader rotated with Rotate Right is as
   * turned as one under a rotated view, and reported an offset down an axis it
   * no longer had until 2026-08-18. That number goes into the history and the
   * session, so it is what Back and a restart land on.
   */
  get position(): { page: number; top: number } {
    const page = this.scroller.pageAt(this.scrollTop);
    if (this.scroller.effectiveTurns(page) !== 0) return { page, top: 0 };
    const top = (this.scrollTop - this.scroller.pageTopOf(page)) / this.zoom;
    return { page, top: Math.max(0, top) };
  }

  /**
   * Scrolls to an outline destination.
   *
   * `top` is points from the page's top, or `null` for a destination like
   * `/Fit` that names no coordinate --- which means the page, so the page's top
   * is the honest interpretation of it.
   *
   * **It records where the reader was**, so Back can undo it. Here rather than
   * at the four places that call it --- a link, an outline row, a search result,
   * a comment --- because "remember to record the jump" is a rule someone has
   * to keep following, and the fifth caller is the one that forgets. The only
   * jump not recorded is the history's own, which would otherwise push the
   * place it is leaving straight back onto the stack it just popped from.
   *
   * A destination that lands where the reader already is records nothing: see
   * {@link History.push}.
   */
  goToDestination(page: number, top: number | null): void {
    if (!this.replaying) {
      this.history.push(this.position);
      // **Announced here for the reason the push itself is here.** Back has
      // somewhere to go now that it did not a moment ago, and a caller telling
      // the window about that is a rule somebody has to keep following --- the
      // fifth caller is the one that forgets, which is the argument the
      // paragraph above already makes about recording the jump at all. Until
      // 2026-08-23 only `followLink` announced it, so a jump from the outline,
      // a search result or a comment left Back greyed in the menu bar with
      // somewhere to go.
      this.opts.onNavigate?.();
    }
    const clamped = Math.max(0, Math.min(page, this.opts.pageCount - 1));
    const base = this.scroller.pageTopOf(clamped);
    // A turned page has no vertical offset to scroll to: at a quarter turn the
    // destination's axis is the screen's horizontal one, and at a half turn it
    // counts upwards from the bottom while the reader still scrolls down.
    // Rather than place a heading somewhere plausible and wrong, this lands on
    // the page --- which is exactly what `/Fit` means, and what `outline.rs`
    // already returns for a destination it cannot place.
    //
    // The page's own turn as well as the view's, and for the page being jumped
    // *to* rather than the one being left: this asked `this.turns === 0` until
    // 2026-08-18 and scrolled 394 pt down a page an edit had made 600 pt tall.
    const offset = this.scroller.effectiveTurns(clamped) === 0 ? (top ?? 0) : 0;
    // A little air above, for the same reason `goToMatch` leaves a third of a
    // screen: a heading flush against the top edge reads as cut off.
    //
    // **Never past the page's own top**, which is not a rounding nicety. The
    // margin is meant to reveal what sits above the heading; when the heading
    // *is* the top of the page there is nothing above it, and 6 pt of the
    // *previous* page is what gets revealed instead. `position` then reports
    // that previous page, and `currentId` drops any entry whose page is past
    // the reader before {@link REACHED_TOLERANCE_PT} is ever consulted --- so
    // clicking an entry highlights a different entry, which is the bug that
    // tolerance was added to fix and could only fix within a page.
    //
    // Found by `viewer_check.py` on `links.pdf`, whose outline is deliberately
    // not in page order: jumping to "Chapter two" (`/Fit`, page 5) highlighted
    // "Named, flat" on page 4. Every `/Fit` and `/FitB` destination has this
    // shape, and so does every destination on a rotated view, where the offset
    // is zero by construction.
    const air = Math.max(0, offset - DESTINATION_MARGIN_PT);
    this.scrollTo(base + air * this.zoom);
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
    // Nothing below is meant for a reader who is typing, and since 2026-08-18
    // there is a text field inside this root: the note on a mark. Before this
    // guard, typing "n" into a note turned the page under the box, "p" turned it
    // back, Home jumped to the start and the space bar scrolled --- and ⌘R
    // rotated the view while ⌘C overwrote what the reader had just copied out of
    // the field. All of it measured, none of it visible in `git status`, because
    // this handler predates the only text field it can ever see.
    //
    // *Everything* rather than the literal keys alone. `appcommands.ts` guards
    // ⌘Z and ⌘⇧Z only, and its reasoning is right for the window: the chords it
    // holds are ones no text field claims, so taking them from the find bar is
    // what a reader wants. This handler holds the opposite half --- `n`, `p`,
    // Space, Home, End, the arrows --- plus ⌘A and ⌘C, which mean *this field*
    // when a field has the keyboard.
    if (inTextField(event)) return;

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
    } else if (event.key === "Enter" && this.drawKind === "note") {
      // **The comment tool's keyboard route, and it is not a convenience.** The
      // tool is armed by a command a reader can reach from the palette and the
      // menu bar with no pointer at all, and its gesture is a press on the page
      // --- so without this, choosing *Add comment* from the keyboard arms a
      // mode the keyboard cannot finish. Enter places it where {@link commentAt}
      // puts a comment nobody pointed at: the top-left of what is on screen.
      //
      // Above the ink arm rather than below it, and the two cannot both be
      // live: `armDraw` is what sets `drawKind`, and it is `inking`'s own
      // finisher that clears it. The order is stated for the reason the Escape
      // ladder below states its own --- it costs a comparison and stays right if
      // that ever changes.
      this.placeComment();
    } else if (event.key === "Enter" && this.inking) {
      // **First on the Enter ladder, and unlike the Escape one this ordering is
      // load-bearing.** A drawing in progress cannot co-exist with an open note
      // --- `armDraw` closes one and a press with a tool armed cannot open one
      // --- but it very much can co-exist with a *focused link*, which the
      // keyboard walk leaves behind and which `armDraw` does not clear. Below
      // that arm, Enter on a document with a link focused would follow the link
      // and strand the drawing.
      this.finishDrawing();
    } else if (event.key === "Enter" && this.markNote.openId !== null) {
      // Before the link arm, on the ladder Escape already uses here: the
      // innermost thing wins, and a note the walk just opened is inside a link
      // that was focused before it. This is how a reader who walked to a mark
      // starts typing --- the walk deliberately left the keyboard on the page so
      // that the next press would step again.
      event.preventDefault();
      this.markNote.focusField();
    } else if (event.key === "Enter" && this.focusedLink) {
      // Only with a link focused, so Enter is not swallowed on every other
      // document --- an unhandled key falls through to the window, and taking
      // it here unconditionally would be a viewer that eats a keystroke it does
      // nothing with.
      this.followFocusedLink();
    } else if (matches("nav.nextLink", event)) {
      this.stepLink(1);
    } else if (matches("nav.previousLink", event)) {
      this.stepLink(-1);
    } else if (matches("nav.nextMark", event)) {
      this.stepMark(1);
    } else if (matches("nav.previousMark", event)) {
      this.stepMark(-1);
    } else if (matches("edit.clearSelection", event)) {
      // Escape, and the innermost thing it can dismiss goes first. A reader
      // with a note open, a link focused and a selection presses it three
      // times, which is the order every application uses --- dismissing them
      // together would lose two things they were not asking about.
      //
      // **An armed tool goes first, and that ordering is defensive rather than
      // load-bearing --- which is worth saying, because the comment here first
      // claimed the opposite.** It said a note open alongside an armed tool
      // would otherwise take the first Escape. That state cannot arise:
      // `armDraw` closes both boxes, and a press with a tool armed is
      // intercepted before anything can open one. A mutation written to swap
      // these two survived, and it survived because there is no reachable input
      // that tells them apart.
      //
      // Kept in this order anyway. It costs a comparison, it is the order that
      // stays correct if a later change *does* make the two co-exist, and the
      // alternative is a reader stuck in a mode --- which is the one failure the
      // one-shot design exists to rule out.
      // The eraser is in this list too. It stays armed between sweeps, so it is
      // a mode a reader can be in with nothing on screen but the cursor --- the
      // exact state Escape exists for, and the one a guard listing only the
      // pen's fields leaves them stuck in.
      if (this.moving) {
        // **First, and this one *is* load-bearing where the ordering below is
        // defensive.** A move is live only while the pointer is down and a note
        // box is open on the mark being dragged, so Escape during a move can
        // genuinely reach two things --- and the mark springing back to where it
        // was is what the reader pressed it for.
        this.moveDrag.cancel();
      } else if (
        this.drawKind !== null ||
        this.drawing ||
        this.inking ||
        this.erasing ||
        this.doomed ||
        this.cropping ||
        this.redacting ||
        this.cropDrawing
      ) {
        this.cancelDraw();
      }
      else if (this.popup.openId !== null) this.closeComment();
      else if (this.focusedLink) this.clearLinkFocus();
      else this.clearSelection();
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

  // --- Comments ------------------------------------------------------------

  /**
   * Replaces the comments the page knows about.
   *
   * The marks themselves are painted by PDFium inside the tiles, so this adds
   * no drawing at all --- what it adds is the ability to press one, and the note
   * that opens when a reader does.
   */
  setComments(items: readonly Comment[]): void {
    this.commentItems = items;
    if (this.popup.openId !== null && !items.some((item) => item.id === this.popup.openId)) {
      // The open note is not in the new list --- a different document, or a
      // reload. Closing it is the only honest option: leaving it up would
      // attribute somebody's words to a page that does not have them.
      this.closeComment();
    }
  }

  /**
   * Replaces the links the page knows about.
   *
   * Like {@link setComments} this adds no drawing: a link's underline or box is
   * whatever the producer put in the page, and PDFium already paints it. What
   * this adds is the ability to follow one, and the pointer that says you can.
   */
  setLinks(items: readonly Link[]): void {
    this.linkItems = items;
    this.linkWalk = orderedLinks(items);
    this.turnedLinks = null;
    this.clearLinkFocus();
    // The history belongs to the document, not to the session: keeping it would
    // let Back scroll a new document to a page number the old one had.
    this.history.clear();
    // And say so, or Back stays live in the menu on a document with nowhere to
    // go back to --- the mirror of the case above, and the one a reader meets
    // every time they open a second file.
    this.opts.onNavigate?.();
    // The accessibility tree too, and this is the half a sighted reader never
    // sees: without it a table of contents is announced as ordinary prose, with
    // nothing for a screen reader to tell one from the other.
    this.a11y.setLinks(items);
  }

  /**
   * The cursor the surface is currently showing. For the check harness.
   *
   * Read back off the element rather than off {@link overLink}, which is this
   * class's own belief: a check that asked the viewer what it thinks it did
   * could not see the style write failing.
   */
  get cursorName(): string {
    return this.surfaceHost.style.cursor || "default";
  }

  /** A slot's laid-out size in points, as displayed. For the check harness. */
  pageSizeOf(slot: number): { width_pt: number; height_pt: number } {
    return this.scroller.pageSize(slot);
  }

  /** How many links the page knows about. For the check harness. */
  get linkCount(): number {
    return this.linkItems.length;
  }

  /**
   * The first page carrying a link, or -1. For the check harness.
   *
   * From the walk order rather than from the scan order, so it is the page a
   * reader reaches first rather than the one whose annotation the file happened
   * to write first.
   */
  get firstLinkPage(): number {
    return this.linkWalk[0]?.page ?? -1;
  }

  /** Whether Back and Forward would do anything. For the check harness. */
  get historyDepths(): { back: number; forward: number } {
    return this.history.depths;
  }

  /**
   * Forgets where the reader has been. For the check harness.
   *
   * `setLinks` already does this, and using it for the purpose would mean
   * clearing the links to clear the history --- a side effect standing in for an
   * intention, which reads as a trick to whoever meets it next. The harness
   * needs it because Back and Forward are guarded on the history rather than on
   * a document being open, so "which commands does an open document offer" has
   * no fixed answer until the history is at a known point: a corpus whose text
   * the search phase could jump into arrives at the command phase with somewhere
   * to go back to, and one with no text does not.
   *
   * Announced, for the reason every other history change is: Back has nowhere to
   * go now, and a menu that was told about the jump has to be told about this.
   */
  clearHistory(): void {
    this.history.clear();
    this.opts.onNavigate?.();
  }

  /** The link the keyboard is on, or -1. For the check harness. */
  get linkFocus(): number {
    return this.focusedLink?.id ?? -1;
  }

  /**
   * Whether the focus ring is drawn. For the check harness.
   *
   * Read off the element rather than off {@link focusedLink}, which is this
   * class's own belief: a check that asked the viewer what it thinks it drew
   * could not see the ring failing to appear.
   */
  get linkRingShown(): boolean {
    return this.ring.style.display === "block";
  }

  /**
   * Moves the keyboard to the next link, or the previous one.
   *
   * The only way to reach a link without a pointer, and until this existed a
   * reader on the keyboard could move by page, heading and search hit and could
   * not follow a cross-reference at all --- on a document whose table of
   * contents is its navigation, that is most of the document.
   *
   * It scrolls the link into view rather than assuming it is on screen, and it
   * reports running out rather than wrapping: arriving back at page 1 of a
   * 775-page document is a surprise, and silence reads as a broken key.
   */
  stepLink(direction: 1 | -1): boolean {
    if (this.linkWalk.length === 0) {
      this.opts.onError?.("This document has no links.");
      return false;
    }
    const next = stepAlong(this.linkWalk, this.focusedLink, this.position, direction);
    if (!next) {
      this.opts.onError?.(
        direction === 1 ? "No further links." : "No earlier links.",
      );
      return false;
    }

    this.focusedLink = next;
    // Scrolled to only when it is not already on screen, so walking a page of
    // links does not jerk the view on every press. Same test `showComment`
    // makes, and for the same reason: a page being visible is not the link
    // being visible.
    const where = this.linkAnchor(next);
    const height = this.viewportSize().height;
    if (where.bottom < 0 || where.top > height) {
      this.goToDestination(next.page, Math.max(0, this.linkTopPt(next)));
    }
    this.placeRing();
    this.wake();
    return true;
  }

  /** Follows the focused link, if there is one. */
  followFocusedLink(): boolean {
    if (!this.focusedLink) return false;
    this.followLink(this.focusedLink.id);
    return true;
  }

  /** Takes the keyboard off any link. */
  clearLinkFocus(): void {
    if (!this.focusedLink) return;
    this.focusedLink = null;
    this.ring.style.display = "none";
  }

  /** Points from the displayed page's top to the focused link's top edge. */
  private linkTopPt(link: Link): number {
    return this.viewRectOn(link.page, link.rect).top;
  }

  /** Where a link's rectangle is on screen, in the root's coordinates. */
  private linkAnchor(link: Link): Anchor {
    return this.anchorOn(link.page, link.rect);
  }

  /**
   * Puts the ring over the focused link.
   *
   * Called every frame while one is focused, like the popup's `place`, so it
   * follows a scroll, a zoom and a rotation. A ring left where it was drawn
   * would, one flick later, be outlining a different paragraph --- which is
   * worse than no ring, because it says the keyboard is somewhere it is not.
   */
  private placeRing(): void {
    if (!this.focusedLink) return;
    const where = this.linkAnchor(this.focusedLink);
    this.ring.style.display = "block";
    this.ring.style.left = `${Math.round(where.left)}px`;
    this.ring.style.top = `${Math.round(where.top)}px`;
    this.ring.style.width = `${Math.max(1, Math.round(where.right - where.left))}px`;
    this.ring.style.height = `${Math.max(1, Math.round(where.bottom - where.top))}px`;
  }

  /**
   * Follows a link: records where the reader was, then jumps.
   *
   * A refusal is reported through `onError` rather than ignored, because a
   * rectangle that swallows a click without a word is indistinguishable from a
   * broken viewer --- the same reasoning `outline.ts` gives for saying why a
   * heading does nothing.
   */
  followLink(id: number): void {
    const link = this.linkItems.find((item) => item.id === id);
    if (!link) return;

    if (link.target.kind !== "page") {
      const said = refusalFor(link.target);
      if (said) this.opts.onError?.(said);
      return;
    }

    // Recorded by `goToDestination`, not here: one mechanism, so a link and an
    // outline row cannot come to disagree about what Back means. The same now
    // goes for *announcing* it --- this line used to call `onNavigate` itself,
    // which is why the link was the one route that told the window and the
    // other three were not.
    this.goToDestination(link.target.page, link.target.top_pt);
  }

  /**
   * Whether Back has anywhere to go, so a menu item can grey when it has not.
   *
   * `History`'s own answer rather than a second one derived here, which is the
   * rule this file keeps: a copy of a distinction is what lets one of them
   * drift. It exists because {@link ViewerOptions.onNavigate} did --- that
   * callback was declared so a Back affordance could be re-enabled after a
   * jump, and until 2026-08-23 nothing consumed it, because both commands were
   * guarded on "a document is open" alone and neither ever greyed. The wiring
   * gate carried it as its one exemption, saying that wiring the callback was
   * the same piece of work as making them grey. This is that work.
   */
  get canGoBack(): boolean {
    return this.history.canGoBack;
  }

  /** Whether Forward has anywhere to go. {@link canGoBack}'s twin. */
  get canGoForward(): boolean {
    return this.history.canGoForward;
  }

  /** Goes back to where the reader was before the last jump. */
  goBack(): boolean {
    const to = this.history.back(this.position);
    if (!to) return false;
    this.jumpTo(to);
    return true;
  }

  /** Goes forward again, after a {@link goBack}. */
  goForward(): boolean {
    const to = this.history.forward(this.position);
    if (!to) return false;
    this.jumpTo(to);
    return true;
  }

  /**
   * Scrolls to a recorded place, without recording anything itself.
   *
   * The flag is what stops {@link goToDestination} pushing the place being left
   * onto the stack it was just popped from --- which would make Back a toggle
   * between two positions and Forward unreachable.
   */
  private jumpTo(place: Place): void {
    this.replaying = true;
    try {
      // **Not `goToDestination`, and that is the fix rather than a shortcut.**
      // That method subtracts `DESTINATION_MARGIN_PT` so a heading is not flush
      // against the top edge, which is right for a *destination* the document
      // named and wrong for a position the reader was actually at: Back then
      // lands slightly above where they were, `position` reports the page above,
      // and a Back/Forward round trip drifts a page each time.
      //
      // Found by `viewer_check.py` on a 775-page document: going to the last
      // page and back reported 773 rather than 774, and Forward returned 772.
      // The margin compounds, so the further a reader travels the further off
      // they land --- which is the shape that reads as "Back is unreliable"
      // rather than as an off-by-one.
      const page = Math.max(0, Math.min(place.page, this.opts.pageCount - 1));
      const offset =
        this.scroller.effectiveTurns(page) === 0 ? Math.max(0, place.top) : 0;
      this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);
      this.wake();
    } finally {
      this.replaying = false;
    }
    this.opts.onNavigate?.();
  }

  /** The comment whose note is open, or -1. For the check harness. */
  get commentOpen(): number {
    return this.popup.openId ?? -1;
  }

  /** What the open note says. For the check harness. */
  get commentText(): string {
    return this.popup.text;
  }

  /** The note element, so the check harness can look at it. */
  get commentPopup(): HTMLElement {
    return this.popup.node;
  }

  /**
   * Scrolls to a comment and opens its note.
   *
   * `focus` moves the keyboard into the note, which is what a reader arriving
   * from the sidebar wants and what a reader clicking the mark does not --- see
   * {@link CommentPopup.show}.
   */
  showComment(id: number, focus = true): void {
    const comment = this.commentItems.find((item) => item.id === id);
    if (!comment) return;

    // Whether the *mark* is on screen, not whether its page is: a page is
    // "visible" when any part of it is, so a note near the foot of the page a
    // reader is at the top of would otherwise open with its anchor below the
    // window --- the popup clamps itself into view and points at nothing.
    const where = this.anchorFor(comment);
    const height = this.viewportSize().height;
    if (where.bottom < 0 || where.top > height) {
      // Placed by its own top edge rather than at the top of the page: a note
      // on page 300 of a manual is somewhere *on* that page, and scrolling to
      // the page alone can leave the mark below the fold.
      this.goToDestination(comment.page, this.topPtOf(comment));
    }
    // Re-read after the scroll above, which moves it.
    this.popup.show(comment, this.repliesTo(id), this.anchorFor(comment), focus);
    this.opts.onComment?.(id);
    this.wake();
  }

  /** Closes the note, if one is open. */
  closeComment(): void {
    if (this.popup.openId === null) return;
    this.popup.hide();
    this.opts.onComment?.(null);
    // The keyboard goes back to the page rather than nowhere: a `display:none`
    // element keeps focus in Chromium until something else takes it, and the
    // arrow keys would then scroll nothing.
    this.root.focus();
  }

  /** The replies to one comment, in document order. */
  private repliesTo(id: number): Comment[] {
    return this.commentItems.filter((item) => item.reply_to === id);
  }

  /**
   * Everything that turns a page's rectangles, and the size to turn them in.
   *
   * A rectangle the backend sends --- a comment's, a link's, a mark's --- is in
   * the page's **display** space: after the file's own `/Rotate` and before any
   * turn the reader or an edit added. Placing one therefore needs both of those
   * turns, and `scroller.effectiveTurns` is the one place they are added.
   *
   * Here rather than at each call site because six of them wrote `this.turns`
   * instead --- the view's rotation alone --- and were wrong by exactly the page
   * turn. The measurement is in `docs/PLAN.md`: on a page turned by an edit, a
   * comment was painted in one place and found in another, while a *mark* with
   * the identical rectangle was found where it was painted. The size is the
   * document's, before either turn, because that is what `turnQuad` turns in.
   */
  private turnsOn(page: number): { turns: number; width_pt: number; height_pt: number } {
    const size = this.scroller.pageSize(page);
    return {
      turns: this.scroller.effectiveTurns(page),
      width_pt: size.width_pt,
      height_pt: size.height_pt,
    };
  }

  /**
   * Where a slot's crop sits inside the page the file describes, and its size.
   *
   * `uncropped` for a page nobody has cropped and for one whose geometry has not
   * arrived yet --- the second is a real state, since the geometry is an IPC
   * round trip, and the honest answer during it is the page as the file has it.
   * A rectangle placed under the wrong one is out by the crop's offset for a
   * frame, which is what the page's own tile epoch corrects.
   */
  private cropAt(page: number): CropGeometry {
    const id = this.pages.idOf(page);
    const known = id === undefined ? undefined : this.crops.get(id);
    if (known) return known;
    const size = this.scroller.pageSize(page);
    return uncropped(size.width_pt, size.height_pt);
  }

  /**
   * A rectangle from the file, placed under the crop and every turn in force.
   *
   * The crop first and the turn second, and the order is the whole of it: a
   * rectangle arrives measured from the **file's** displayed corner, `intoCrop`
   * moves it to the cropped page's corner, and only then is it turned --- by the
   * *cropped* page's size, which is what `turnsOn` reports. Turning first would
   * turn it in a box it is not in.
   *
   * Text is the one thing that does **not** come through here, and that is not
   * an omission: `page_text` is answered by a worker that already applied the
   * crop, so a character box is in the cropped page's space when it arrives.
   * The asymmetry is real and is why the two are placed by different routes.
   */
  private viewRectOn(page: number, rect: readonly [number, number, number, number]): Quad {
    const { turns, width_pt, height_pt } = this.turnsOn(page);
    return viewRect(intoCrop(rect, this.cropAt(page)), turns, width_pt, height_pt);
  }

  /**
   * The size a slot is laid out at: its crop, under both turns.
   *
   * What a reader sees and what a rectangle they draw has to be clamped into.
   * `scroller.pageSize` is the cropped page *before* the turn, which is the
   * convention `viewRect` wants and the wrong one for anything measured on
   * screen --- a quarter turn swaps the sides, so clamping against it puts a
   * mark dropped near the bottom of a landscape page somewhere in the middle.
   */
  private laidSize(slot: number): { width: number; height: number } {
    const size = displayedSize(
      this.scroller.pageSize(slot),
      this.scroller.effectiveTurns(slot),
    );
    return { width: size.width_pt, height: size.height_pt };
  }

  /**
   * A rectangle in a slot's laid-out space, back in the file's display space.
   *
   * **The inverse of {@link viewRectOn}, and the only thing that travels this
   * way.** Everything else in the application goes from the file, through the
   * crop, through the turn, onto the screen; a reader who *drops* a comment or
   * *drags* a box starts at the far end and the two steps have to be undone in
   * the opposite order. The model holds one space and it is the file's --- see
   * `crop.ts` --- so a mark stored in the laid-out space would move the moment
   * the reader turned the page or changed the crop.
   *
   * It is one function for the reason `viewRectOn` is: the failure is a
   * plausible rectangle somewhere else on the page, it only appears on a page
   * that is cropped or turned, and no unrotated corpus can see it.
   */
  private fileRectOn(slot: number, quad: Quad): [number, number, number, number] {
    const { turns } = this.turnsOn(slot);
    const laid = this.laidSize(slot);
    const back = unturnQuad(quad, turns, laid.width, laid.height);
    const moved = outOfCrop(
      [back.left, back.top, back.right, back.bottom],
      this.cropAt(slot),
    );
    return [moved[0] ?? 0, moved[1] ?? 0, moved[2] ?? 0, moved[3] ?? 0];
  }

  /**
   * Where a rectangle on a page is on screen, in the root's coordinates.
   *
   * One implementation for a comment's note and a link's focus ring, which had
   * a copy each and differed only in the type of the thing being placed.
   */
  private anchorOn(
    page: number,
    rect: readonly [number, number, number, number],
  ): Anchor {
    const quad = this.viewRectOn(page, rect);
    const origin = this.scroller.pageOrigin(page);
    return {
      left: origin.left + quad.left * this.zoom,
      top: origin.top + quad.top * this.zoom - this.scrollTop,
      right: origin.left + quad.right * this.zoom,
      bottom: origin.top + quad.bottom * this.zoom - this.scrollTop,
    };
  }

  /**
   * A comment's distance from the top of its page, in points, as displayed.
   *
   * `goToDestination` takes the destination convention --- points from the
   * page's top --- and the rectangle is already in that space, so the only work
   * is the turns {@link turnsOn} collects.
   */
  private topPtOf(comment: Comment): number {
    return Math.max(0, this.viewRectOn(comment.page, comment.rect).top);
  }

  /** Where a comment's rectangle is on screen, in the root's coordinates. */
  private anchorFor(comment: Comment): Anchor {
    return this.anchorOn(comment.page, comment.rect);
  }

  /**
   * The comment under a pointer event, or `null`.
   *
   * Works from the page and point {@link pointFrom} produces --- except that it
   * must answer on a page whose *text* has not arrived, which `pointFrom`
   * refuses to do. A comment is not text and does not wait for any: a note on a
   * scanned page has to be clickable, and a page with no text layer at all would
   * otherwise never answer.
   */
  private commentUnder(event: PointerEvent): Comment | null {
    if (this.commentItems.length === 0) return null;
    const { page, x, y } = this.pageAndPoint(event);

    const { turns, width_pt, height_pt } = this.turnsOn(page);
    const here = turnedFor(onPage(this.commentItems, page), turns, width_pt, height_pt);
    return hitTest(here, page, x, y);
  }

  /**
   * The link under a pointer event, or `null`.
   *
   * Same coordinate work as {@link commentUnder}, and separate from it on
   * purpose: a comment wins a shared point, because a note somebody wrote is a
   * stronger claim on a click than a rectangle a producer generated, and the
   * two do overlap --- a highlight over a cross-reference is ordinary.
   */
  private linkUnder(event: PointerEvent): Link | null {
    if (this.linkItems.length === 0) return null;
    const { page, x, y } = this.pageAndPoint(event);
    return linkAt(this.linksOn(page), page, x, y);
  }

  /**
   * Which page a pointer event is over, and where on it, in that page's points.
   *
   * The three hit tests --- comments, the reader's own marks, links --- all need
   * exactly this and had two copies of it between them before the third arrived.
   * Deliberately *not* {@link pointFrom}, which answers the same question and
   * additionally refuses a page whose text has not been extracted: a mark is not
   * text and must be clickable on a scanned page, which has none.
   */
  private pageAndPoint(event: ScreenPoint): { page: number; x: number; y: number } {
    const bounds = this.root.getBoundingClientRect();
    const docY = event.clientY - bounds.top + this.scrollTop;
    const page = this.scroller.pageAt(docY);
    const origin = this.scroller.pageOrigin(page);
    return {
      page,
      x: (event.clientX - bounds.left - origin.left) / this.zoom,
      y: (docY - origin.top) / this.zoom,
    };
  }

  /**
   * Where a point is on one named page, whatever page it is over.
   *
   * {@link pageAndPoint}'s twin, and the difference is the whole reason it
   * exists: that one asks which page the pointer is on and answers in that
   * page's space, which is right for starting a gesture and wrong for
   * continuing one. A mark belongs to one page, so a drag that wanders below it
   * has to keep being measured against the page the mark is on --- otherwise the
   * offset jumps by a page height the moment the pointer crosses the gap.
   */
  private pointOn(slot: number, at: ScreenPoint): Point {
    const bounds = this.root.getBoundingClientRect();
    const origin = this.scroller.pageOrigin(slot);
    return {
      x: (at.clientX - bounds.left - origin.left) / this.zoom,
      y: (at.clientY - bounds.top + this.scrollTop - origin.top) / this.zoom,
    };
  }

  /**
   * An offset in the laid-out space, as the model's display space sees it.
   *
   * **Two points through {@link fileRectOn} rather than one rectangle**, which
   * is the same move {@link viewStrokesOf} makes and for the same reason: that
   * conversion unturns and then un-crops, and a *vector* is not a rectangle --- a
   * quarter turn swaps its components and negates one, and the crop does nothing
   * to it at all. Mapping both ends and subtracting gets both of those right
   * without a second copy of the rule, and a copy of that rule is what
   * `docs/TRAPS.md` records drifting across eleven call sites once already.
   */
  private fileDelta(slot: number, dx: number, dy: number): { dx: number; dy: number } {
    const at = this.fileRectOn(slot, { left: 0, top: 0, right: 0, bottom: 0 });
    const to = this.fileRectOn(slot, { left: dx, top: dy, right: dx, bottom: dy });
    return { dx: (to[0] ?? 0) - (at[0] ?? 0), dy: (to[1] ?? 0) - (at[1] ?? 0) };
  }

  /**
   * An offset cut down so the mark it moves stays on its page.
   *
   * In the laid-out space, which is where the page has an edge a reader can see:
   * the model holds a mark in the file's display space and does not know how big
   * a page is at all, so a clamp there would have nothing to clamp against ---
   * see `Doc::displace`.
   *
   * The bounding box of every quad, not each quad separately. A multi-quad mark
   * moves as one thing, and clamping the rectangles one at a time would slide
   * them relative to each other and take the mark apart.
   *
   * A mark larger than its page pins to the top-left rather than being refused,
   * which is {@link iconQuad}'s answer to the same question and is not a case
   * worth a branch.
   */
  private clampMove(
    id: number,
    slot: number,
    want: { dx: number; dy: number },
  ): { dx: number; dy: number } {
    const mark = this.marks.find((item) => item.id === id);
    const placed = mark ? this.viewQuadsOf(mark) : null;
    if (!placed || placed.quads.length === 0) return want;
    let left = Infinity;
    let top = Infinity;
    let right = -Infinity;
    let bottom = -Infinity;
    for (const quad of placed.quads) {
      left = Math.min(left, quad.left);
      top = Math.min(top, quad.top);
      right = Math.max(right, quad.right);
      bottom = Math.max(bottom, quad.bottom);
    }
    const laid = this.laidSize(slot);
    return {
      dx: Math.min(Math.max(want.dx, -left), laid.width - right),
      dy: Math.min(Math.max(want.dy, -top), laid.height - bottom),
    };
  }

  /** One page's links in the view's space, memoised across pointer moves. */
  private linksOn(page: number): Link[] {
    const { turns, width_pt, height_pt } = this.turnsOn(page);
    // Keyed on the *effective* turns, not the view's: a page an edit turned
    // while this page was the cached one would otherwise be served rectangles
    // placed under the rotation it had before the turn.
    const cached = this.turnedLinks;
    if (cached && cached.page === page && cached.turns === turns) {
      return cached.items;
    }
    const items = linksTurnedFor(
      linksOnPage(this.linkItems, page),
      turns,
      width_pt,
      height_pt,
    );
    this.turnedLinks = { page, turns, items };
    return items;
  }

  /**
   * Shows a pointer over a link, and puts it back afterwards.
   *
   * The only thing that tells a reader a run of text is clickable --- a PDF's
   * link rectangles are usually invisible, and every producer draws them
   * differently or not at all. Guarded on a change rather than written every
   * move: assigning `style.cursor` on each pointer event is a style write per
   * event on a surface that is trying to hold 60 frames.
   */
  private readonly onHover = (event: PointerEvent): void => {
    // **Before the link guard, which returns for a document with no links.**
    // Putting it after would leave the comment tool's ghost bubble working on
    // documents that happen to be linked and invisible on documents that are
    // not, which is the shape of defect that gets reported as "it works on my
    // other PDF".
    this.trackArmed(event);
    if (this.linkItems.length === 0) return;
    const over = this.linkUnder(event) !== null;
    if (over === this.overLink) return;
    this.overLink = over;
    this.showCursor();
  };

  /**
   * Follows the pointer while a tool is armed, so the ghost can be painted.
   *
   * A frame is only asked for when the answer *moves*, which matters because
   * this runs on every pointer event: repainting the overlay per event on a
   * surface trying to hold 60 frames is the cost {@link onHover}'s own comment
   * argues about for a single style write.
   */
  private trackArmed(event: { clientX: number; clientY: number }): void {
    if (this.drawKind === null) {
      if (this.armedAt === null) return;
      this.armedAt = null;
      this.wake();
      return;
    }
    const { page, x, y } = this.pageAndPoint(event);
    const was = this.armedAt;
    if (was && was.slot === page && was.x === x && was.y === y) return;
    this.armedAt = { slot: page, x, y };
    this.wake();
  }

  /**
   * Keeps an open note against the mark it belongs to.
   *
   * Runs every frame while one is open, which is what makes it follow a scroll,
   * a zoom and a rotation. A note whose page has scrolled out of sight is
   * closed rather than pinned to the edge: at that point it is a floating box of
   * text with nothing to attribute it to.
   */
  private syncComment(): void {
    const id = this.popup.openId;
    if (id === null) return;
    const comment = this.commentItems.find((item) => item.id === id);
    if (!comment || !this.scroller.visiblePages().includes(comment.page)) {
      this.closeComment();
      return;
    }
    this.popup.place(this.anchorFor(comment));
  }

  /**
   * One mark's rectangles as the view draws them, and the slot they are on.
   *
   * The three things that need a mark's geometry --- painting it, hit-testing
   * it, and anchoring its note to it --- go through here, so what a reader can
   * click is by construction what they can see. Two of them wrote this loop out
   * before the third arrived, which is the shape this repository records as *two
   * copies of a distinction drift*.
   *
   * `null` when the mark's page is not in the current order, which is what an
   * undo of a page deletion looks like for one frame.
   */
  private viewQuadsOf(mark: MarkView): { slot: number; quads: Quad[] } | null {
    const slot = this.pages.slotOfId(mark.page);
    if (slot === undefined) return null;
    // Through {@link turnsOn}, which comments and links also place their
    // rectangles with. This held its own copy of the same two lines until
    // 2026-08-18, and a copy of a distinction is what lets one of them drift ---
    // which is exactly what had happened on the other side, where six call
    // sites turned by the view's rotation alone. One implementation also means
    // the window checks on a mark reach the primitive all three use.
    // Through `viewRectOn`, which is where the crop is applied before the turn.
    // A mark is stored in the file's display space --- see `crop.ts` on why --- so
    // it needs exactly the same two steps a comment and a link need, and doing
    // the turn here with `turnQuad` alone is how the mark subsystem drifted
    // from the other two once already.
    const quads: Quad[] = [];
    for (let at = 0; at + 3 < mark.quads.length; at += 4) {
      quads.push(
        this.viewRectOn(slot, [
          mark.quads[at] ?? 0,
          mark.quads[at + 1] ?? 0,
          mark.quads[at + 2] ?? 0,
          mark.quads[at + 3] ?? 0,
        ]),
      );
    }
    return { slot, quads };
  }

  /**
   * A mark's strokes in view space, or `null` when its page is not laid out.
   *
   * **Built on {@link viewRectOn} by handing it the point as a rectangle of no
   * size**, which is the same move `user_strokes` makes in `save.rs` with
   * `from_device` and for the same reason: the crop and the two turns are one
   * rule, and a second copy written for points is a second thing to get wrong at
   * every `/Rotate`. It is also the rule that has already drifted here once,
   * which {@link viewQuadsOf}'s own comment records.
   *
   * Empty for every kind but ink, because nothing else carries strokes.
   */
  private viewStrokesOf(
    mark: MarkView,
  ): { slot: number; strokes: { x: number; y: number }[][] } | null {
    const slot = this.pages.slotOfId(mark.page);
    if (slot === undefined) return null;
    const strokes = mark.strokes.map((flat) => {
      const points: { x: number; y: number }[] = [];
      for (let at = 0; at + 1 < flat.length; at += 2) {
        const x = flat[at] ?? 0;
        const y = flat[at + 1] ?? 0;
        const placed = this.viewRectOn(slot, [x, y, x, y]);
        points.push({ x: placed.left, y: placed.top });
      }
      return points;
    });
    return { slot, strokes };
  }

  /**
   * The reader's own mark under a pointer event, or `null`.
   *
   * Hit-tested per *rectangle* rather than over the mark's bounding box: a
   * highlight running across three lines has a box that covers the margins
   * beside the short last line, and a note that opened from a press on white
   * paper reads as a misplaced mark.
   *
   * {@link hitTest} rather than an inequality written here, so the slack around
   * a small rectangle and the "smallest wins" rule for overlapping ones are the
   * ones comments and links already use.
   */
  private markUnder(event: ScreenPoint): MarkView | null {
    if (this.marks.length === 0) return null;
    const { page, x, y } = this.pageAndPoint(event);
    const here: { page: number; rect: [number, number, number, number]; mark: MarkView }[] =
      [];
    for (const mark of this.marks) {
      const placed = this.viewQuadsOf(mark);
      if (!placed) continue;
      for (const quad of placed.quads) {
        // Labelled with the slot the mark is *on*, not the one under the
        // pointer, so `hitTest` does the page match rather than a filter here
        // agreeing with it --- two page tests are two chances to disagree.
        here.push({
          page: placed.slot,
          rect: [quad.left, quad.top, quad.right, quad.bottom],
          mark,
        });
      }
    }
    return hitTest(here, page, x, y)?.mark ?? null;
  }

  /** Where a mark's note hangs: the union of its rectangles, in the host's space. */
  private anchorForMark(mark: MarkView): Anchor | null {
    const placed = this.viewQuadsOf(mark);
    const first = placed?.quads[0];
    if (!placed || !first) return null;
    const box = placed.quads.reduce(
      (into, quad) => ({
        left: Math.min(into.left, quad.left),
        top: Math.min(into.top, quad.top),
        right: Math.max(into.right, quad.right),
        bottom: Math.max(into.bottom, quad.bottom),
      }),
      { ...first },
    );
    const origin = this.scroller.pageOrigin(placed.slot);
    // The drag's offset, for {@link paintMarks}' reason and one of its own: the
    // note box is open on the mark being dragged --- opening it is what the press
    // did --- so an anchor that ignored the offset would leave the box pointing
    // at the place the mark has left.
    const shift = this.moving?.id === mark.id ? this.moving : null;
    const ox = shift ? shift.dx * this.zoom : 0;
    const oy = shift ? shift.dy * this.zoom : 0;
    return {
      left: origin.left + box.left * this.zoom + ox,
      top: origin.top + box.top * this.zoom - this.scrollTop + oy,
      right: origin.left + box.right * this.zoom + ox,
      bottom: origin.top + box.bottom * this.zoom - this.scrollTop + oy,
    };
  }

  /**
   * The id of the reader's own mark at a point on screen, or `null`.
   *
   * The public half of {@link markUnder}, which the press path has used since
   * marks existed. It is here because a right-click on a highlight offered a
   * menu about the *selection* --- copy, mark, find --- and nothing about the
   * mark under the pointer, so there was no way to take a mark off without
   * first left-pressing it to open its note. Reported from use.
   *
   * Both this and {@link pageAndPoint} beneath it take a structural point
   * rather than a `PointerEvent`, because a `contextmenu` event is a
   * `MouseEvent` and neither ever read anything but the two client
   * coordinates. Widening the parameter accepts strictly more than before, so
   * no existing caller changes behaviour.
   */
  markAt(point: ScreenPoint): number | null {
    return this.markUnder(point)?.id ?? null;
  }

  /**
   * Where a new comment would go, as the page and the one quad it needs.
   *
   * **One rule with two entry points, rather than two rules.** A right-click
   * names a point and the palette does not, and the temptation is to answer
   * those separately --- which would be two statements of where a comment is
   * allowed to land, drifting the first time one of them is adjusted. So the
   * point is optional and its absence has an answer: the top-left of whatever
   * of the current page is actually on screen, inset by the icon's own size.
   *
   * The inset is what makes the answer usable rather than merely defined. A
   * comment placed hard against the corner of the visible area sits under the
   * page's own margin, and one placed at the page's top-left when the reader
   * has scrolled halfway down lands somewhere they cannot see --- which reads
   * as a command that did nothing.
   *
   * **Built in the laid-out space and converted at the end.** `pageAndPoint`
   * answers where a pointer is on the page *as it is displayed*, and the model
   * holds the file's space; this took the first for the second, so on a page
   * that is turned or cropped the comment landed somewhere else entirely. It
   * clamped against the un-turned size as well, which is the same mistake a
   * second time. The clamp belongs in the laid-out space --- that is the
   * rectangle the reader can see --- and {@link fileRectOn} is the one step
   * back, shared with the box.
   */
  commentAt(point: ScreenPoint | null): { page: PageId; quads: number[] } | null {
    const slot = point ? this.pageAndPoint(point).page : this.currentPage();
    // **The id, not the slot.** What comes back is handed to the model, and the
    // model names pages by id --- see `Edits.mark`, whose parameter used to be a
    // slot and which a drawn shape fed an id into for a fortnight. `undefined`
    // is a slot that is not in the current order, which a caller must treat as
    // "nowhere to put it" rather than as page zero.
    const id = this.pages.idOf(slot);
    if (id === undefined) return null;
    let x: number;
    let y: number;
    if (point) {
      const at = this.pageAndPoint(point);
      x = at.x;
      y = at.y;
    } else {
      const origin = this.scroller.pageOrigin(slot);
      x = ICON_SIZE;
      y = Math.max(0, (this.scrollTop - origin.top) / this.zoom) + ICON_SIZE;
    }
    const quad = iconQuad(x, y, this.laidSize(slot));
    return { page: id, quads: this.fileRectOn(slot, quad) };
  }

  /**
   * Arms the box tool: the next drag on a page draws one.
   *
   * Takes the kind rather than being `armBox()`, because the next tool that
   * needs a drag --- an ellipse, a text box --- differs from this one in the
   * subtype it writes and in nothing else, and a second method would be a
   * second copy of the whole gesture.
   *
   * Arming closes an open note. A reader who is typing in one and then chooses
   * a drawing tool has finished with the note, and leaving it open would put a
   * box over the paper *and* commit whatever was in the field on the next press
   * anywhere.
   */
  armDraw(kind: MarkKind, stamp: StampName | null = null): void {
    if (this.markNote.openId !== null) this.closeMark();
    if (this.popup.openId !== null) this.closeComment();
    // Two tools, one hand. Arming a pen puts the eraser and the crop away, so
    // no two of the three can be set and no gesture has to ask which meant it.
    this.erasing = false;
    this.cropping = false;
    this.redacting = false;
    this.drawKind = kind;
    // Held beside the kind rather than in the caller, so that what a drag
    // commits comes from one place. A second copy in `App.svelte` would be a
    // piece of state that can disagree with `drawKind` about which tool is
    // armed.
    this.drawStamp = stamp;
    this.showCursor();
    this.wake();
  }

  /**
   * Arms the crop tool: the next drag on a page keeps what is inside it.
   *
   * **One-shot, like every drawing tool and unlike the eraser.** A crop replaces
   * whatever crop the page had, so a second drag would undo the first rather
   * than add to it --- there is nothing to accumulate, and a reader who has
   * cropped their page is not still cropping.
   *
   * It arms rather than acting, which is the whole difference from *Crop page to
   * content*: that one measures the ink and needs no gesture, and this one is
   * for the case only the reader can decide --- a figure out of a plate, one
   * column of two, a scan with a hand in the corner.
   *
   * Arming closes an open note, for {@link armDraw}'s reason.
   */
  armCrop(): void {
    if (this.markNote.openId !== null) this.closeMark();
    if (this.popup.openId !== null) this.closeComment();
    this.drawKind = null;
    this.drawStamp = null;
    this.inking = null;
    this.erasing = false;
    this.redacting = false;
    this.cropping = true;
    this.showCursor();
    this.wake();
  }

  /**
   * Arms the redaction tool: the next drag marks a region for removal.
   *
   * **The same gesture as {@link armCrop} and the opposite meaning, which is
   * why it is a tool of its own rather than a mode of that one.** A crop hides
   * everything outside the rectangle and can be reset; a redaction is a step
   * towards destroying everything inside it and cannot. Two commands that share
   * a drag but not an outcome must not share a name, an entry in the palette or
   * a preview --- see `paintCropPreview`, where the scrim is inverted so that
   * what is about to go is what is shaded.
   *
   * **One-shot, like the crop and unlike the eraser.** A reader marking several
   * regions arms it again for each, which is deliberate: every region is a
   * separate row in the review list and a separate undo, and a tool that stayed
   * armed would let a slipped drag add one nobody meant to make.
   *
   * Marking destroys nothing. `docs/PLAN.md` §6 splits marking from applying
   * precisely so that a reader looks at the list before anything is removed.
   *
   * Arming closes an open note, for {@link armDraw}'s reason.
   */
  armRedact(): void {
    if (this.markNote.openId !== null) this.closeMark();
    if (this.popup.openId !== null) this.closeComment();
    this.drawKind = null;
    this.drawStamp = null;
    this.inking = null;
    this.erasing = false;
    this.cropping = false;
    this.redacting = true;
    this.showCursor();
    this.wake();
  }

  /**
   * Arms the eraser: the next drag takes what its nib crosses.
   *
   * **It stays armed, and there is no finishing key.** Ink needed Enter because
   * its strokes pile into one mark and something has to say the drawing is
   * done; a sweep is complete when the reader lifts the pointer, so each one
   * commits on its own and the next one can start immediately. Escape puts it
   * away, which is the same key that abandons a drawing and means the same
   * thing.
   *
   * **From a drawing it takes whole strokes, not parts of them.** Sweeping
   * across the middle of a line removes that line, rather than splitting it in
   * two and leaving a gap --- which is what a pen-and-paper eraser does and is
   * not what this is. Splitting would mean rewriting `/InkList` into more
   * strokes than the reader drew and re-deriving the appearance around a hole;
   * it is a real feature and it is not this one.
   *
   * ⚠ **And it takes every other kind whole, which this comment used to argue
   * against.** It read: *"Only drawings are erasable. A sweep over a highlight
   * does nothing, because a highlight has no strokes to take and making the
   * eraser remove whole marks of any kind would be a second, much more
   * destructive command wearing the same cursor --- Remove mark already exists
   * and says what it does."* Every clause of that is still true except the
   * conclusion. *Remove mark* does exist and is kept: it takes the mark whose
   * note is **open**, which is the mark a reader has already named, and it is
   * chosen from a menu with the pointer somewhere else entirely. What it cannot
   * be is *aimed* --- and reaching a highlight through it means opening a form
   * first, on a target as small as a 24-point icon. The nib is the aimed route,
   * and "much more destructive" is answered by the sweep being one press of
   * undo away and by the preview showing what is about to go while the pointer
   * is still down.
   *
   * That paragraph had also been **orphaned** since the crop tool landed: a
   * second doc comment was inserted between it and this method, so it
   * documented nothing and TypeScript said nothing about it. See the trap.
   *
   * Arming closes an open note, for {@link armDraw}'s reason.
   */
  armErase(): void {
    if (this.markNote.openId !== null) this.closeMark();
    if (this.popup.openId !== null) this.closeComment();
    this.drawKind = null;
    this.inking = null;
    this.cropping = false;
    this.redacting = false;
    this.erasing = true;
    this.showCursor();
    this.wake();
  }

  /**
   * Adds everything within the nib of `(x, y)` to the sweep.
   *
   * A drawing loses the strokes the nib touched and keeps the rest; every other
   * kind is taken whole, because it has no parts. Which of the two a mark gets
   * is {@link isPath}, the same predicate the painter asks --- so what the
   * eraser treats as strokes is by construction what the reader sees drawn as
   * strokes.
   *
   * Reads both geometries through {@link viewStrokesOf} and {@link viewQuadsOf},
   * so the comparison happens in the space the reader is pointing at rather than
   * the page's own --- which is what lets {@link ERASER_RADIUS} be a fixed number
   * of screen pixels at every zoom.
   *
   * **It can only reach the reader's own marks.** `this.marks` is the edit
   * model's, and a comment annotation the file arrived with is not in it, so a
   * sweep across somebody else's sticky note does nothing to the document. That
   * is a property of what this loop is over rather than a check, and it is the
   * behaviour to keep: the eraser is a tool for taking back your own marks.
   */
  private sweep(x: number, y: number): void {
    const swept = this.doomed;
    if (!swept) return;
    // **From where the nib was to where it is**, not the point it is at. A
    // pointer reports at the display's rate and a hand crosses several strokes
    // between two reports; testing the samples alone let a quick sweep down a
    // column of three strokes take the outer two and leave the middle one.
    const from = swept.last;
    const to = { x, y };
    swept.last = to;
    const nib = ERASER_RADIUS / this.zoom;
    for (const mark of this.marks) {
      if (!isPath(mark.kind)) {
        const placed = this.viewQuadsOf(mark);
        if (!placed || placed.slot !== swept.slot) continue;
        if (placed.quads.some((quad) => quadSwept(quad, from, to, nib))) {
          swept.whole.add(mark.id);
        }
        continue;
      }
      const inked = this.viewStrokesOf(mark);
      if (!inked || inked.slot !== swept.slot) continue;
      for (const [index, stroke] of inked.strokes.entries()) {
        if (!strokeSwept(stroke, from, to, nib)) continue;
        let taken = swept.marks.get(mark.id);
        if (!taken) {
          taken = new Set();
          swept.marks.set(mark.id, taken);
        }
        taken.add(index);
      }
    }
  }

  /** Whether the eraser is armed. For the menu's enablement and the harness. */
  get eraseArmed(): boolean {
    return this.erasing;
  }

  /**
   * What the sweep in progress has taken, or `null` when the eraser is not
   * armed.
   *
   * `ViewerStatus.erasing` is this, for the reason {@link drawnStrokes} gives:
   * a second expression computing the same thing is a copy that a mutation can
   * break in one place and not the other.
   *
   * Two zeroes is armed-and-nothing-swept, which is a mode the window says out
   * loud.
   */
  get swept(): { strokes: number; marks: number } | null {
    if (!this.erasing) return null;
    let strokes = 0;
    for (const taken of this.doomed?.marks.values() ?? []) strokes += taken.size;
    return { strokes, marks: this.doomed?.whole.size ?? 0 };
  }

  /**
   * Drops the armed tool and any drag in progress.
   *
   * Safe with nothing armed, which is what lets Escape call it without asking
   * first. {@link PointerDrag.cancel} reports the drag as not committed, so a
   * box half-drawn when Escape is pressed is not written.
   */
  cancelDraw(): void {
    this.drawDrag.cancel();
    // The crop goes with it. This method is "drop whatever tool is armed" ---
    // which is what Escape means and what every caller wants --- so a third
    // named method would only leave Escape asking which of three was live.
    this.cropDrag.cancel();
    this.cropping = false;
    this.redacting = false;
    this.cropDrawing = null;
    this.drawKind = null;
    this.drawing = null;
    // Everything drawn goes with it. Escape means "abandon this" and it has
    // meant that here since the box, so a drawing of six strokes is discarded
    // by it exactly as a half-dragged rectangle is --- which is why the finish
    // gesture had to be a *different* key rather than a second Escape.
    this.inking = null;
    // The eraser goes with it. This method is "drop whatever tool is armed",
    // which is what Escape means and what every caller wants; a separate
    // `cancelErase` would leave Escape asking which of the two was live.
    this.erasing = false;
    this.doomed = null;
    this.showCursor();
    this.wake();
  }

  /**
   * Drops the armed comment where a reader with no pointer would want it.
   *
   * {@link commentAt}'s no-point answer --- the top-left of the part of the
   * current page that is actually on screen, inset by the icon's own size ---
   * routed through the same {@link ViewerOptions.onDrawn} a press produces, so
   * the mark is made by one path however the reader asked for it. Two paths
   * would be two answers to "what does placing a comment do", and the note box
   * opening afterwards is the half that would drift first.
   *
   * Spends the tool, like a press does. A reader who has placed their comment is
   * not still in placing mode, and Escape is not the way out of a thing that
   * already happened.
   */
  private placeComment(): void {
    const where = this.commentAt(null);
    this.drawKind = null;
    this.showCursor();
    this.wake();
    if (!where) return;
    // Never a stamp: a comment is placed by a click and a stamp by a drag, so
    // this path cannot be reached with a stamp armed.
    this.opts.onDrawn?.(
      "note",
      where.page,
      { quads: where.quads, strokes: [] },
      null,
    );
  }

  /**
   * Ends the drawing in progress and sends it as one mark.
   *
   * **Enter, and it had to be its own key.** Escape already means abandon, and
   * a mode a reader can only leave by throwing away what they made is not a
   * mode they will use twice. The two are the pair every application uses for
   * "done" and "cancel", which is what makes an unlabelled mode learnable ---
   * and the status line names both while a drawing is live, because a mode
   * nobody can see is the one thing the box's one-shot design existed to rule
   * out.
   *
   * Does nothing with no drawing in progress, so the key falls through to
   * whatever else claims it. The caller does not have to ask first.
   */
  finishDrawing(): void {
    const made = this.inking;
    if (!made || made.strokes.length === 0) return;
    const id = this.pages.idOf(made.slot);
    this.inking = null;
    this.drawKind = null;
    this.showCursor();
    this.wake();
    // A page that went while the drawing was being made --- deleted from under
    // it, or the document replaced. The strokes are dropped rather than sent to
    // whatever moved into that slot, which is the same reasoning `Annotate`
    // carries a page *id* for.
    if (id === undefined) return;
    this.opts.onDrawn?.(
      "ink",
      id,
      {
        quads: [],
        strokes: made.strokes.map((stroke) =>
        stroke.flatMap((point) => {
          const mapped = this.fileRectOn(made.slot, {
            left: point.x,
            top: point.y,
            right: point.x,
            bottom: point.y,
          });
          return [mapped[0], mapped[1]];
        }),
        ),
      },
      // Never a stamp: ink is finished with Enter and a stamp with a drag.
      null,
    );
  }

  /**
   * Strokes in the drawing being made, or `null` when none is being made.
   *
   * **`ViewerStatus.drawing` is this, not a second reading of the same state.**
   * Written as its own expression in the status it was a copy of a distinction,
   * and a mutation that stopped filling the status field survived every test ---
   * because the tests asked the viewer and the window reads the status, and the
   * two were one line apart. One accessor, used by both, has nothing to drift.
   *
   * Zero is armed-with-nothing-drawn and is not `null`: the next press draws,
   * which is a mode, and the window says so.
   */
  get drawnStrokes(): number | null {
    if (this.inking) return this.inking.strokes.length;
    return this.drawKind === "ink" ? 0 : null;
  }

  /** The armed tool, or `null`. For the menu's enablement and the harness. */
  get drawArmed(): MarkKind | null {
    return this.drawKind;
  }

  /** Whether the crop tool is armed. For the status line and the harness. */
  get cropArmed(): boolean {
    return this.cropping;
  }

  /**
   * Whether the redaction tool is armed. For the status line and the harness.
   *
   * Separate from {@link cropArmed} for {@link cropPreview}'s reason: an
   * accessor answering from whichever of the two is set would be a check unable
   * to tell a crop from a redaction, and those are the two this application can
   * most plausibly confuse.
   */
  get redactArmed(): boolean {
    return this.redacting;
  }

  /**
   * The crop being dragged, in the page's laid-out space. For the harness.
   *
   * Separate from {@link drawPreview} because the two states are separate, and
   * a single accessor answering from whichever is set would be a check unable
   * to tell a crop from a box --- which is exactly the confusion this gesture
   * could plausibly ship with.
   */
  get cropPreview(): { slot: number; from: Point; to: Point } | null {
    return this.cropDrawing;
  }

  /** The rectangle being dragged, in the page's laid-out space. For the harness. */
  get drawPreview(): { slot: number; from: Point; to: Point } | null {
    return this.drawing;
  }

  /**
   * Puts the right pointer over the surface.
   *
   * One place, because there are now two things that change it and they are not
   * ordered: a crosshair while a tool is armed, a hand over a link, the
   * platform's own otherwise. Written as a single assignment rather than two
   * handlers each clearing the other, which is how a cursor gets stuck.
   */
  private showCursor(): void {
    // **The ghost is dropped here, and here is the only place that can be
    // trusted to do it.** Five sites clear `drawKind` --- arming, cancelling,
    // finishing a drawing, spending a one-shot tool, and a `pointercancel` ---
    // and every one of them calls this, because every one of them changes what
    // the pointer means. Clearing at each instead would be five copies of one
    // rule, and the sixth site is the one that forgets.
    //
    // Only when nothing is armed, and the condition is load-bearing rather than
    // an optimisation: {@link onHover} calls this whenever the pointer crosses a
    // link's edge, *after* recording the position, so an unconditional clear
    // would blank the ghost for a frame every time the reader passed over a
    // link. A stale position cannot survive a disarm, since a disarm is what
    // this branch runs on.
    if (this.drawKind === null) this.armedAt = null;
    this.surfaceHost.style.cursor =
      this.drawKind || this.cropping || this.redacting
        ? "crosshair"
        : this.overLink
          ? "pointer"
          : "";
  }

  /**
   * Where one of the reader's own marks is on screen, or `null`.
   *
   * The viewer's own placement, not a second computation: it is what the note
   * box hangs off, so it already carries the crop and both rotations. A window
   * check sampling {@link overlaySurface} needs exactly this rectangle and
   * deriving it independently would be a second implementation of the same
   * turn, which is the drift this repository has a trap about.
   */
  markAnchor(id: number): Anchor | null {
    const mark = this.marks.find((item) => item.id === id);
    return mark ? this.anchorForMark(mark) : null;
  }

  /** The mark whose note is open, or -1. For the check harness and the menu. */
  get markOpen(): number {
    return this.markNote.openId ?? -1;
  }

  /** What the open note's box holds. For the check harness. */
  get markNoteText(): string {
    return this.markNote.text;
  }

  /** The note editor's text field, so a harness can ask what has the keyboard. */
  get markNoteField(): HTMLTextAreaElement {
    return this.markNote.field;
  }

  /** The note editor's element, so the check harness can look at it. */
  get markPopup(): HTMLElement {
    return this.markNote.node;
  }

  /**
   * Opens the note on one of the reader's own marks.
   *
   * **It scrolls to the mark when the mark is off screen**, exactly as
   * {@link showComment} does and for the same reason: a page being visible is
   * not the mark being visible, and a note anchored below the window clamps
   * itself into view and points at nothing. This did not, and said so --- "every
   * route in is a press on the mark itself, so it is on screen by construction.
   * The day a panel lists these, that stops being true and this needs the same
   * treatment." {@link stepMark} is that day. A press is unaffected: a mark you
   * can press is on screen, so the test below is false for it.
   *
   * `focus` moves the keyboard into the note field. True from a press, which is
   * a reader reaching for a mark in order to type on it; false from the walk,
   * where the reader is looking rather than writing --- and where taking the
   * keyboard would strand them, since the guard at the top of {@link onKeyDown}
   * means the next press of the walk key would go to the field and do nothing.
   */
  showMark(id: number, focus = true): void {
    const mark = this.marks.find((item) => item.id === id);
    if (!mark) return;
    const where = this.anchorForMark(mark);
    if (!where) return;
    const height = this.viewportSize().height;
    if (where.bottom < 0 || where.top > height) {
      this.goToDestination(
        this.pages.slotOfId(mark.page) ?? 0,
        this.markTopPt(mark),
      );
    }
    // Re-read after the scroll above, which moves it.
    const at = this.anchorForMark(mark);
    if (!at) return;
    this.markNote.show(mark, at, focus);
    this.wake();
  }

  /**
   * Points from the displayed page's top to a mark's topmost edge.
   *
   * The page's top for a mark with no rectangles, which is the honest reading of
   * "somewhere on this page" --- and not `Math.min()` of nothing, which is
   * `Infinity` and would scroll to the end of the document. The walk cannot
   * produce such a mark, since `markWalk` drops it, and a press could not
   * either, having nothing to hit. A saved file reopened is the case neither of
   * those covers.
   */
  private markTopPt(mark: MarkView): number {
    const placed = this.viewQuadsOf(mark);
    if (!placed || placed.quads.length === 0) return 0;
    return Math.max(0, Math.min(...placed.quads.map((quad) => quad.top)));
  }

  /**
   * Moves the reader to the next mark of their own, or the previous one.
   *
   * The only way to reach a mark without a pointer. Until this existed the
   * pointer was it: a highlight's note could not be read, edited or taken off
   * from the keyboard at all, which `docs/PLAN.md` had recorded as outstanding
   * through two increments.
   *
   * It **opens the note** rather than drawing a focus ring the way the link walk
   * does, and the asymmetry is the point. A link is a thing you go *through*, so
   * focusing it and following it are two steps; a mark is a thing you go *to*,
   * and everything a reader can do with one --- read the note, change it, take
   * the mark off --- is in the box. A ring would be a step that only ever
   * precedes opening the box.
   *
   * The keyboard stays on the page, so repeated presses walk. Enter moves it
   * into the note; see {@link onKeyDown}.
   *
   * Same two starting points and the same refusal to wrap as
   * {@link stepAlong} --- it is the same function.
   */
  stepMark(direction: 1 | -1): boolean {
    const walk = markWalk(this.marks, this.pages);
    if (walk.length === 0) {
      this.opts.onError?.("You have not marked anything in this document.");
      return false;
    }
    const from = walk.find((item) => item.id === this.markNote.openId) ?? null;
    const next = stepAlong(walk, from, this.position, direction);
    if (!next) {
      this.opts.onError?.(
        direction === 1 ? "No further marks." : "No earlier marks.",
      );
      return false;
    }
    this.showMark(next.id, false);
    return true;
  }

  /**
   * Takes the mark whose note is open off the page.
   *
   * The one implementation, reached from the popup's own button and from the
   * Edit menu --- which is why it reads the open note for its subject rather
   * than taking an id: both callers mean *this* mark, and an id parameter would
   * let them mean different ones.
   *
   * Closed **without committing**, because the note is going with the mark: a
   * commit here would journal a note onto a highlight that the next command
   * deletes, and the reader would have to undo twice.
   */
  removeOpenMark(): void {
    const id = this.markNote.openId;
    if (id === null) return;
    this.markNote.hide(false);
    this.opts.onMarkRemove?.(id);
    this.root.focus();
  }

  /**
   * Draws the mark whose note is open in `color`.
   *
   * {@link removeOpenMark}'s shape and its argument: the open note is where a
   * reader says which mark they mean, so the `Colour:` commands --- chosen from
   * a palette with the pointer somewhere else entirely --- hand the question
   * back here rather than carrying an id.
   *
   * **Answers whether it did anything**, which the removal does not need to:
   * the caller sets the colour for new marks either way, and a `false` is how it
   * knows there was nothing open to recolour rather than having to ask twice.
   * Nothing is sent for a mark that is already that colour, which is the same
   * comparison the swatch row makes and is made here for the same reason ---
   * an undo step that changes nothing is worse than no command at all.
   *
   * **`null` is the mark's own kind's colour**, which is what the default swatch
   * means and is resolved here rather than by the caller: the kind is the mark's
   * and the mark is the one this method just found. A caller resolving it would
   * have to look up a mark it addressed by not naming.
   */
  recolorOpenMark(color: MarkColor | null): boolean {
    const id = this.markNote.openId;
    if (id === null) return false;
    const mark = this.marks.find((held) => held.id === id);
    if (!mark) return false;
    const want = colorFor(mark.kind, color);
    if (sameColor(mark.color, want)) return false;
    this.opts.onMarkRecolor?.(id, want);
    return true;
  }

  /** Closes the note editor, committing what was typed in it. */
  closeMark(): void {
    if (this.markNote.openId === null) return;
    this.markNote.hide();
    // The keyboard goes back to the page rather than nowhere, for the reason
    // `closeComment` gives: focus left in a `display:none` element stops the
    // arrow keys scrolling anything.
    this.root.focus();
  }

  /**
   * Keeps an open note against the mark it belongs to.
   *
   * The mark half of {@link syncComment}, with one difference that is not
   * cosmetic: a mark can be *removed* under the box --- by an undo, or by a page
   * deletion --- and the note is then closed **without committing**. Sending
   * what was typed would be refused by the model, and the reader would see an
   * error for a highlight they took off themselves.
   */
  private syncMark(): void {
    const id = this.markNote.openId;
    if (id === null) return;
    const mark = this.marks.find((item) => item.id === id);
    if (!mark) {
      this.markNote.hide(false);
      this.root.focus();
      return;
    }
    const at = this.anchorForMark(mark);
    if (!at || !this.scroller.visiblePages().includes(this.pages.slotOfId(mark.page) ?? -1)) {
      this.closeMark();
      return;
    }
    this.markNote.place(at);
  }

  /** A page's text as the view shows it, or `null` if it has not arrived. */
  textOn(slot: number): PageText | null {
    const source = this.pages.sourceOf(slot);
    return source === undefined ? null : this.text.peek(source);
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

  /**
   * Turns a pointer event into a caret.
   *
   * Returns `null` while the page's text has not arrived --- and asks for it, so
   * the next attempt can succeed. A drag that begins before the text lands
   * therefore does nothing until it does, rather than anchoring at character
   * zero and selecting the whole page on the first move.
   */
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
    //
    // **And on macOS a Control+click *is* a right-click**, which the button
    // number does not say. WebKit reports `button: 0` with `ctrlKey` set,
    // because it is physically the primary button and the window server
    // synthesises the menu from it; only a two-finger tap and a real second
    // button arrive as `button === 2`. Testing the number alone therefore let
    // the commonest way a Mac reader opens a context menu through as an
    // ordinary press, which replaced their selection with an empty one at the
    // caret --- and `contextmenu` fires *after* `pointerdown`, so the menu then
    // opened on nothing. Reported from use, and it read as a defect in the menu
    // rather than in the press precisely because the two-finger tap worked
    // throughout.
    //
    // Asked of the platform rather than of `ctrlKey` alone, because Ctrl+click
    // is not a right-click on Windows: there it is an ordinary modified press
    // and must still start a selection. `isMac` is the same answer the chord
    // spelling reads, so there is one platform detector rather than two.
    if (event.button !== 0 || (isMac() && event.ctrlKey)) return;
    // The scrollbar is inside the root and has its own drag.
    if (this.track.contains(event.target as Node)) return;
    this.root.focus();

    // **Before every hit test below**, and that ordering is the mode. A press
    // with a tool armed means "start here" whatever is under it --- a reader
    // drawing a box around a highlighted paragraph must not have the press
    // swallowed by the highlight's note, and one drawing around a
    // cross-reference must not be sent to another page.
    if (this.cropDrag.start(event) || this.drawDrag.start(event)) {
      event.preventDefault();
      return;
    }

    // A press on a mark opens its note and starts no selection. Before the
    // click counter, deliberately: a double-click on a note is two requests to
    // open the same note, not a request to select the word underneath it.
    const mark = this.commentUnder(event);
    // Then the reader's own marks. Below the file's comments, because a note
    // somebody wrote is the more specific claim on a point --- a sticky note is
    // 24 points square and a highlight covers whole lines, so putting marks
    // first would make an existing note under one unreachable, and there is no
    // other route to it. Above links, because a press is the *only* way to open
    // a mark's note or take it off, while a link inside a highlighted paragraph
    // is still reachable from the keyboard.
    //
    // Decided before anything acts on it, rather than in the branch that uses
    // it, because the rule below needs to know the winner: a comment that wins
    // a shared point means the mark under it did *not*.
    const own = mark ? null : this.markUnder(event);

    // **One note box at a time, and one line that says so.** Any press that is
    // not on the mark whose note is open closes it, committing what was typed
    // --- a comment, a link, a word, the paper. Written once here rather than in
    // each branch below, three of which return before reaching the next.
    if (this.markNote.openId !== null && this.markNote.openId !== own?.id) {
      this.closeMark();
    }

    if (mark) {
      event.preventDefault();
      this.showComment(mark.id, false);
      return;
    }
    if (own) {
      event.preventDefault();
      if (this.popup.openId !== null) this.closeComment();
      // Already open on this mark: the box and what is in it are left alone.
      // Reopening would refill it from the model, which still holds the note as
      // it was --- the reader's typing has not been committed yet, and will not
      // be until the box closes.
      if (this.markNote.openId !== own.id) this.showMark(own.id);
      // **And the press may also be the start of a move.** The note opens either
      // way, on the press rather than on the release: a reader who clicks a mark
      // wants its box now, and making them wait for the button to come up so
      // that a drag could be told apart would make every note feel slow. The box
      // follows the mark while it is dragged --- `syncMark` re-anchors it every
      // frame --- so nothing is lost by opening first.
      //
      // Only the kinds a reader placed. A highlight is made of the words under
      // it, and dragging one leaves a wash over a line it does not mark; see
      // `isMovable`, which is where that rule is stated once.
      if (isMovable(own.kind)) {
        const placed = this.viewQuadsOf(own);
        if (placed) {
          this.moving = {
            id: own.id,
            slot: placed.slot,
            from: this.pointOn(placed.slot, event),
            dx: 0,
            dy: 0,
          };
          // Refused only if a drag is somehow already live, in which case the
          // press is not ours to take --- and `moving` must not be left set, or
          // the overlay would offset a mark nothing is dragging.
          if (!this.moveDrag.start(event)) this.moving = null;
        }
      }
      return;
    }

    // Then a link, which loses a shared point to a comment above and wins it
    // against a selection here: a press on a cross-reference is a request to go
    // there, and starting a selection instead is what makes a link in a PDF
    // feel like it does not work.
    const target = this.linkUnder(event);
    if (target) {
      event.preventDefault();
      if (this.popup.openId !== null) this.closeComment();
      // The keyboard follows the pointer onto the link, so a reader who clicks
      // one and then presses "next link" continues from there rather than from
      // wherever the keyboard was left.
      this.focusedLink = target;
      this.placeRing();
      this.followLink(target.id);
      return;
    }
    this.clearLinkFocus();

    // A press anywhere else closes an open note, which is what every popup in
    // every application does and is the only gesture a reader will try.
    if (this.popup.openId !== null) this.closeComment();

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

  /**
   * The marks the model last reported, in page order.
   *
   * Held rather than derived: they arrive with an edit state, which is the one
   * answer the frontend caches. Keyed by the page's *id* --- see
   * {@link PageMap.slotOfId}.
   */
  private marks: readonly MarkView[] = [];

  /**
   * Every region marked for removal, as the model reports them.
   *
   * Its own list beside {@link marks} rather than entries in it. See
   * {@link setRedactions}, and `docmodel.rs` for the property that arrangement
   * carries.
   */
  private redactions: readonly RedactionView[] = [];

  /**
   * Each cropped page's geometry, by page **id**.
   *
   * By id and not by slot, because a page moves and its crop moves with it ---
   * `docs/TRAPS.md` records state keyed by a slot belonging to whatever moves
   * into that slot. Absent means the file's own box, which is what all but a
   * cropped page has.
   */
  private readonly crops = new Map<number, CropGeometry>();

  /**
   * Learns each page's crop, and drops what was measured under the old one.
   *
   * The model answers with the crop **box**; laying a cropped page out needs its
   * displayed size and where it sits inside the file's page, and neither can be
   * computed here --- see `crop.ts`. So a page whose box changed costs one IPC
   * round trip, and only a page whose box changed: a state reply arrives after
   * every command, and a rotate must not re-ask for every page in the document.
   *
   * Asynchronous, and the frame in between is honest rather than hidden: until
   * the geometry lands, `cropAt` answers with the file's own page and rectangles
   * are placed as they were. The page's tile epoch is bumped when it lands,
   * which is what redraws it.
   */
  private async adoptCrops(pages: PageMap): Promise<void> {
    const live = new Set<number>();
    for (let slot = 0; slot < pages.length; slot++) {
      const view = pages.at(slot);
      if (!view) continue;
      live.add(view.id);
      const want = view.crop;
      const held = this.crops.get(view.id);
      if (want === undefined) {
        // Nothing to ask: the file's own page is what `cropAt` falls back to,
        // so a cleared crop needs the entry gone rather than a round trip.
        if (held) {
          this.crops.delete(view.id);
          this.text.setPageCrop(slot, undefined);
          this.scroller.invalidatePage(slot);
        }
        continue;
      }
      this.text.setPageCrop(slot, want);
      const at = await pageGeometry(this.opts.doc, view.source, want).catch(
        () => null,
      );
      if (!at) continue;
      this.crops.set(view.id, at);
      this.scroller.notePageSize(slot, {
        width_pt: at.width_pt,
        height_pt: at.height_pt,
      });
      this.scroller.invalidatePage(slot);
    }
    // A page that has gone takes its geometry with it, or the map grows for the
    // life of the document and a reused id would find a stale answer.
    for (const id of [...this.crops.keys()]) {
      if (!live.has(id)) this.crops.delete(id);
    }
    this.wake();
  }

  /**
   * Records the marks an edit state carried, and redraws.
   *
   * Called with every state, not only when the list changed: the comparison
   * that would avoid the redraw is the comparison the redraw already is, and a
   * mark that was undone has to leave the screen in the same frame the model
   * says it is gone.
   */
  setMarks(marks: readonly MarkView[]): void {
    this.marks = marks;
    // `wake` rather than a repaint: the overlay is drawn from the frame loop,
    // which may be idle when a mark is made from the menu bar with nothing
    // scrolling. Painting here as well would draw the same rectangles twice.
    this.wake();
  }

  /**
   * The pending redactions, as the model reports them.
   *
   * {@link setMarks}' twin, and a separate call for the reason the two lists are
   * separate everywhere else: a mark is written into the saved file and a
   * redaction is never. One setter taking both would be the first place the
   * distinction could be lost.
   */
  setRedactions(regions: readonly RedactionView[]): void {
    this.redactions = regions;
    // `wake` rather than a repaint, for {@link setMarks}' reason.
    this.wake();
  }

  /**
   * The selected text's rectangles on each page, in the page's own space.
   *
   * **Not the view's space**, which is what {@link TextCache.peek} answers and
   * what the selection paints with. A mark outlives the view: stored against
   * the reader's rotation it would move the next time they turned the window,
   * or the next time an edit turned the page. `peekUnturned` is the page as the
   * document displays it, and the character indices are identical in both --- a
   * rotation renumbers nothing --- so the same range yields the same characters.
   *
   * Empty when nothing is selected, and empty for a page whose text has not
   * arrived. Not requested here: the copy path already loads what it needs, and
   * asking from a command would queue an extraction the reader is waiting on.
   *
   * **The words come out beside the rectangles**, because this is the one
   * moment they are both known and free. A highlight is the reader marking a
   * phrase, and the phrase is what they would recognise it by in a list --- but
   * a saved file records no such thing, so anything reading it back off the
   * page would be a second answer to "which characters does this rectangle
   * cover", drifting from the one that made the rectangle. `marklist.ts` says
   * what is done with it.
   *
   * `textFrom` rather than `textOf` over the same range: it reads in the order
   * the page reads rather than the order the file was written in, which is what
   * a copy of the same selection produces. Two spellings of the selected text
   * would differ on exactly the documents where the difference matters ---
   * columns.
   */
  selectionQuadsByPage(): { page: PageId; quads: number[]; text: string }[] {
    if (!this.selection) return [];
    const out: { page: PageId; quads: number[]; text: string }[] = [];
    for (const page of this.selection.pages()) {
      // `page` is a slot here, and what leaves is an id, for `commentAt`'s
      // reason: the model names pages by id and a slot is not a name.
      const id = this.pages.idOf(page);
      if (id === undefined) continue;
      const text = this.text.peekUnturned(page);
      if (!text) continue;
      const range = this.selection.rangeOn(page);
      if (!range) continue;
      const said = this.selection.textFrom(page, text) ?? "";
      const quads = runsFor(text, range.from, range.to).flatMap((quad) => [
        quad.left,
        quad.top,
        quad.right,
        quad.bottom,
      ]);
      // Out of the crop before it leaves, because the model holds one space and
      // it is the file's. A mark made on a cropped page and stored in the
      // cropped space would be written where the crop was rather than where the
      // words are, the moment the reader changed the crop or took it off.
      if (quads.length > 0) {
        out.push({ page: id, quads: outOfCrop(quads, this.cropAt(page)), text: said });
      }
    }
    return out;
  }

  /**
   * A page's text as the document displays it, fetching it if it is not here.
   *
   * The asking counterpart of {@link TextCache.peekUnturned}, which answers only
   * from the cache. Unturned for the same reason as there: a caller reading a
   * comment's `/QuadPoints` has rectangles in the page's own space, and matching
   * them against boxes in the reader's rotation would find the words of a
   * different part of the page every time the window was turned.
   *
   * Waits on `load` and then reads the cache rather than using `load`'s own
   * answer, because that one is turned into the view. Reading it back is safe
   * against nothing: the page could be evicted between the two, which is why
   * `null` is a real outcome here rather than a formality.
   *
   * Deliberately **not** used by anything on the paint path. It queues an
   * extraction behind the tiles, so a caller wanting many pages should ask for
   * one at a time --- `App.svelte`'s comment-words loop is the worked example.
   */
  async unturnedText(page: number): Promise<PageText | null> {
    await this.text.load(page);
    return this.text.peekUnturned(page);
  }

  /** Draws the marks, the search highlights and the selection. */
  private paintOverlay(): void {
    const ctx = this.overlayCtx;
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);

    // Multiply keeps the glyphs legible underneath, which a flat fill over the
    // tile would not: the text is already painted into the pixels.
    ctx.globalCompositeOperation = "multiply";
    // Marks first, and **the order no longer fails to matter**. It used to:
    // under `multiply` each layer contributes a factor independent of what is
    // underneath, even with alpha, so painting A then B and B then A both leave
    // `dst * ((1-a) + a*A) * ((1-b) + b*B)` --- a reading order rather than a
    // z-order. The comment here ended "the day one of the three stops
    // multiplying, that stops being true", and that day arrived: an underline
    // and a strikeout are opaque lines drawn `source-over`, because a
    // translucent line reads as a smudge --- `is_wash` in `save.rs` decides the
    // same thing the same way for the file. So this is a z-order now for line
    // kinds, and marks go first deliberately: a search hit and a selection are
    // washes, and a reader dragging across a struck-out line has to see the
    // selection over it rather than under it.
    this.paintMarks(ctx, dpr);
    this.paintMatches(ctx, dpr);
    this.paintSelection(ctx, dpr);
    // Over the marks, the hits and the selection, and under the live gesture.
    // A pending redaction is the most consequential thing on the page --- it
    // names content that is about to stop existing --- so nothing the reader is
    // merely looking through may sit on top of it. The hand still wins, because
    // what the hand is doing is the only thing more current.
    this.paintRedactions(ctx, dpr);
    // Last, and over everything: it is the thing the reader's hand is on.
    this.paintDrawing(ctx, dpr);
    ctx.globalCompositeOperation = "source-over";
  }

  /**
   * Draws every pending redaction on a visible page.
   *
   * Placed through {@link viewRectOn}, which is the same primitive
   * {@link viewQuadsOf} uses for a mark and `comments.ts` and `links.ts` use for
   * theirs: the crop and the two turns are one rule, and a second copy written
   * for regions is a second thing to get wrong at every `/Rotate`. The mark
   * subsystem drifted from the other two once already by holding its own copy.
   *
   * Drawn `source-over` rather than `multiply`, unlike a wash. A redaction is
   * not a highlight over words --- it is a statement that the words are going,
   * and it has to keep a definite edge over paper as well as over text so an
   * over-selection is visible against the margin.
   */
  private paintRedactions(ctx: CanvasRenderingContext2D, dpr: number): void {
    if (this.redactions.length === 0) return;

    const visible = new Set(this.scroller.visiblePages());
    ctx.save();
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = REDACT_FILL;
    ctx.strokeStyle = REDACT_EDGE;
    ctx.lineWidth = Math.max(1, OUTLINE_WIDTH * this.zoom * dpr);
    for (const region of this.redactions) {
      const slot = this.pages.slotOfId(region.page);
      if (slot === undefined || !visible.has(slot)) continue;
      const quad = this.viewRectOn(slot, region.area);
      const origin = this.scroller.pageOrigin(slot);
      const x = (origin.left + quad.left * this.zoom) * dpr;
      const y = (origin.top + quad.top * this.zoom - this.scrollTop) * dpr;
      const w = (quad.right - quad.left) * this.zoom * dpr;
      const h = (quad.bottom - quad.top) * this.zoom * dpr;
      ctx.fillRect(x, y, w, h);
      // Solid, where the drag's preview is dashed. That is the one difference
      // between the two states and it carries the whole distinction: dashed is
      // a rectangle still being decided, solid is one that is in the list.
      ctx.strokeRect(x, y, w, h);
    }
    ctx.restore();
  }

  /**
   * Draws every mark on a visible page.
   *
   * The quads are in the page's own display space and the reader may be looking
   * at the page turned --- by the view, or by an edit, or both --- so each is
   * put through {@link turnQuad} with the sum of the two, which
   * `scroller.effectiveTurns` is the one place that adds. The page size handed
   * in is the *document's*, before either turn, which is what `turnQuad` takes.
   */
  private paintMarks(ctx: CanvasRenderingContext2D, dpr: number): void {
    if (this.marks.length === 0) return;

    const visible = new Set(this.scroller.visiblePages());
    for (const mark of this.marks) {
      const placed = this.viewQuadsOf(mark);
      if (!placed || !visible.has(placed.slot)) continue;
      // **The preview is the absence**, which is what the stroke loop below
      // says for a drawing coming apart. A mark the sweep has taken whole is
      // simply not drawn, so the reader watches it go under the nib and there
      // is no ghost copy to keep in step with what will be sent.
      if (this.doomed?.whole.has(mark.id)) continue;

      // The kind decides the ink and the blend, exactly as `is_wash` decides
      // both for the saved file. Set per mark rather than once for the loop:
      // a document can carry all three, and hoisting this out was how every
      // mark came to be drawn as a highlight in the first place.
      const wash = isWash(mark.kind);
      ctx.fillStyle = markInk(mark.color, wash);
      ctx.globalCompositeOperation = wash ? "multiply" : "source-over";

      const origin = this.scroller.pageOrigin(placed.slot);
      // **The mark being dragged is painted where the hand has it, not where the
      // model has it.** The model hears once, when the pointer comes up, so
      // between the press and the release the offset lives only here --- which is
      // also what makes Escape and a `pointercancel` free: clearing `moving` puts
      // the mark back with no command to undo.
      //
      // Added in device pixels rather than to the quad in points, so that it
      // reaches the strokes below by the same two numbers. Offsetting the quad
      // instead would move the rectangle and leave a drawing's strokes behind.
      const shift = this.moving?.id === mark.id ? this.moving : null;
      const ox = shift ? shift.dx * this.zoom * dpr : 0;
      const oy = shift ? shift.dy * this.zoom * dpr : 0;
      // **Ink is drawn from its strokes and never from its quad**, which is the
      // one rectangle in this loop that is not the shape of the mark: it is a
      // box round the drawing, so painting it would put a filled block where a
      // reader drew a line. Same class of defect as the underline that looked
      // like a highlight, and the same tell --- the saved file would be right.
      const style = paintOf(mark.kind);
      if (style === "path") {
        const inked = this.viewStrokesOf(mark);
        if (inked) {
          ctx.strokeStyle = markInk(mark.color, false);
          ctx.lineWidth = INK_WIDTH * this.zoom * dpr;
          // Round, matching the `1 J 1 j` the appearance stream sets: a mitre
          // on a hand-drawn corner spikes, and a butt cap leaves a stroke that
          // stops square where the reader's hand did not.
          ctx.lineCap = "round";
          ctx.lineJoin = "round";
          const going = this.doomed?.marks.get(mark.id);
          for (const [index, stroke] of inked.strokes.entries()) {
            // **The preview is the absence.** A stroke the sweep has taken stops
            // being painted at once, so the reader watches the drawing come
            // apart under the nib; there is no ghost copy to keep in step with
            // what will be sent, because what will be sent is exactly what is
            // no longer here.
            if (going?.has(index)) continue;
            const [first, ...rest] = stroke;
            if (!first) continue;
            ctx.beginPath();
            ctx.moveTo(
              (origin.left + first.x * this.zoom) * dpr + ox,
              (origin.top + first.y * this.zoom - this.scrollTop) * dpr + oy,
            );
            for (const point of rest) {
              ctx.lineTo(
                (origin.left + point.x * this.zoom) * dpr + ox,
                (origin.top + point.y * this.zoom - this.scrollTop) * dpr + oy,
              );
            }
            // One path per stroke, for the reason `save.rs` gives: a single
            // path across all of them joins the end of each to the start of the
            // next with a line the reader never drew.
            ctx.stroke();
          }
        }
        continue;
      }
      for (const quad of placed.quads) {
        const band = markBand(mark.kind, quad);
        const left = (origin.left + band.left * this.zoom) * dpr + ox;
        const top = (origin.top + band.top * this.zoom - this.scrollTop) * dpr + oy;
        const width = (band.right - band.left) * this.zoom * dpr;
        const height = (band.bottom - band.top) * this.zoom * dpr;
        if (style === "text") {
          // The reader's words, in the lines the *backend* broke them into.
          // Nothing here measures text: `ctx.measureText` would measure whatever
          // font the system resolved and the file is set in Helvetica, so two
          // measurements would break lines in two places. See `MarkView.lines`.
          //
          // Helvetica first in the stack for the same reason the breaks come
          // from Rust: on a machine that has it the glyphs on screen are the
          // glyphs in the file. Arial next, which was drawn to Helvetica's
          // metrics, then whatever the system calls sans-serif.
          //
          // `fillText` puts the string on a canvas as pixels. It is not a markup
          // sink and cannot become one, which is what lets document-controlled
          // text be drawn here at all -- see `docs/THREAT-MODEL.md` T8.
          ctx.fillStyle = markInk(mark.color, false);
          const size = TEXT_SIZE * this.zoom * dpr;
          ctx.font = `${size}px Helvetica, Arial, sans-serif`;
          ctx.textBaseline = "alphabetic";
          const inset = TEXT_INSET * this.zoom * dpr;
          const leading = size * TEXT_LEADING;
          mark.lines.forEach((line, index) => {
            // The same first baseline `save.rs` uses: one size below the top
            // inset, because a baseline placed *at* the top edge hangs the whole
            // line above the box.
            const y = top + inset + size + leading * index;
            if (y > top + height) return;
            ctx.fillText(line, left + inset, y);
          });
        } else if (style === "wave") {
          // Stroked along the band rather than filling it, which is the whole
          // of what a squiggle is. Filling would draw a solid bar two and a
          // half times an underline's height and the file would stay correct --
          // the shape of the defect the underline shipped with.
          //
          // The thickness is the *quad's* `LINE_FRACTION`, not the band's, so a
          // squiggle and an underline over the same words are drawn with the
          // same weight of line. Taking it from the band would make the wave
          // two and a half times heavier than the rule beside it.
          ctx.strokeStyle = markInk(mark.color, false);
          const pen = (quad.bottom - quad.top) * LINE_FRACTION * this.zoom * dpr;
          ctx.lineWidth = pen;
          traceSquiggle(ctx, left, top, width, height, pen);
          ctx.stroke();
        } else if (style === "stamp") {
          // A border and one word, which is both of the two branches around
          // this one and neither of them alone. Drawn as a box would be, plus
          // the word a box has no room for.
          //
          // **The size is measured here and computed in `save.rs`**, and the two
          // cannot be made to agree exactly: the file is set in Helvetica by our
          // own table and this is whatever font the system resolved. That is the
          // text box's situation, and it is why a stamp is one *word* --- a word
          // that overflows is visibly wrong, where a paragraph broken in a
          // different place is not.
          ctx.strokeStyle = markInk(mark.color, false);
          ctx.lineWidth = OUTLINE_WIDTH * this.zoom * dpr;
          ctx.strokeRect(left, top, width, height);
          const word = mark.stamp ? stampWord(mark.stamp) : "";
          const innerW = width - STAMP_INSET * 2 * this.zoom * dpr;
          const innerH = height - STAMP_INSET * 2 * this.zoom * dpr;
          if (word && innerW > 0 && innerH > 0) {
            ctx.fillStyle = markInk(mark.color, false);
            ctx.textBaseline = "alphabetic";
            // One measurement at a nominal size, scaled: `measureText` is linear
            // in the font size, so this is one call rather than a search.
            ctx.font = "100px Helvetica, Arial, sans-serif";
            const unit = Math.max(ctx.measureText(word).width / 100, 1e-6);
            const size = Math.max(1, Math.min(innerW / unit, innerH / STAMP_CAP));
            ctx.font = `${size}px Helvetica, Arial, sans-serif`;
            const drawn = ctx.measureText(word).width;
            ctx.fillText(
              word,
              left + (width - drawn) / 2,
              top + (height + size * STAMP_CAP) / 2,
            );
          }
        } else if (style === "icon") drawBubble(ctx, left, top, width, height);
        else if (style === "outline") {
          // Stroked, not filled, which is the whole of what a box is --- and
          // the same decision `save.rs` makes with `re S` for the file. A
          // filled box hides what it was drawn around, which is the one thing
          // it exists not to do.
          //
          // The stroke straddles the path, so half of it falls outside the
          // rectangle. That is correct here and wrong in the file, where the
          // appearance stream's `/BBox` would clip it --- see `outline_path`,
          // which insets for exactly that reason and which the overlay must not
          // copy, or the two would draw the box at two different sizes.
          ctx.strokeStyle = markInk(mark.color, false);
          ctx.lineWidth = OUTLINE_WIDTH * this.zoom * dpr;
          ctx.strokeRect(left, top, width, height);
        } else if (style === "ellipse") {
          // Stroked like the box, and the same argument for it: a filled ring
          // hides what it was drawn around. What differs is only the path.
          ctx.strokeStyle = markInk(mark.color, false);
          ctx.lineWidth = OUTLINE_WIDTH * this.zoom * dpr;
          traceEllipse(ctx, left, top, width, height);
          ctx.stroke();
        } else {
          // `fill`, and the only kind that reaches it is a highlight --- the
          // blend was set per mark above. This was a bare `else` catching
          // whatever the chain above had no branch for, so a kind nobody
          // painted was drawn as a filled rectangle: a wrong picture rather
          // than a missing one, and nothing could go red. `paintOf` is
          // exhaustive, so a new kind is a compile error there instead.
          ctx.fillRect(left, top, width, height);
        }
      }
    }
    // Put back what the caller set, because the two below still rely on it.
    ctx.globalCompositeOperation = "multiply";
  }

  /**
   * The box being dragged, as a dashed outline.
   *
   * **Dashed, where the committed mark is solid**, so the two are never
   * confused: a preview is not yet a mark, and a reader who lets go expects
   * something to change. Drawn from the two page-space corners rather than from
   * the pointer's client position, so it stays on the paper when the view
   * scrolls under a held pointer.
   *
   * Refuses nothing. `boxQuad`'s minimum applies when the drag *ends*; showing
   * a reader the rectangle they are dragging, however small, is what tells them
   * the tool is armed and working.
   */
  private paintDrawing(ctx: CanvasRenderingContext2D, dpr: number): void {
    // **Ink first, and a rectangle is the wrong preview for it.** This painted
    // the rubber band for *every* live drag, ink included, so a reader drawing
    // freehand watched a dashed box stretch from where they pressed to wherever
    // the pen was --- their line appearing only after they let go. It shipped in
    // the commit that added ink and no check saw it: the overlay phase paints
    // marks the model has, and a preview is by definition not one of those.
    //
    // Drawn before the `!live` return, because the strokes already finished
    // have to stay on screen between them: a drawing is several strokes and the
    // reader is looking at the ones they have made while deciding on the next.
    this.paintInkPreview(ctx, dpr);
    this.paintCommentGhost(ctx, dpr);
    this.paintCropPreview(ctx, dpr);

    const live = this.drawing;
    if (!live || this.drawKind === "ink") return;
    const origin = this.scroller.pageOrigin(live.slot);
    const left = Math.min(live.from.x, live.to.x);
    const top = Math.min(live.from.y, live.to.y);
    const right = Math.max(live.from.x, live.to.x);
    const bottom = Math.max(live.from.y, live.to.y);

    ctx.globalCompositeOperation = "source-over";
    ctx.strokeStyle = PREVIEW_STROKE;
    ctx.lineWidth = Math.max(1, OUTLINE_WIDTH * this.zoom * dpr);
    ctx.setLineDash([6 * dpr, 4 * dpr]);
    // **The preview is the shape that will be committed**, which is the lesson
    // the note above records rather than a new one: a rectangle was the wrong
    // preview for ink, and it is the wrong preview for an ellipse for the same
    // reason. A reader dragging out a ring should watch a ring, not a box that
    // turns into one when they let go.
    const px = (origin.left + left * this.zoom) * dpr;
    const py = (origin.top + top * this.zoom - this.scrollTop) * dpr;
    const pw = (right - left) * this.zoom * dpr;
    const ph = (bottom - top) * this.zoom * dpr;
    if (this.drawKind === "ellipse") {
      traceEllipse(ctx, px, py, pw, ph);
      ctx.stroke();
    } else ctx.strokeRect(px, py, pw, ph);
    // Put back, or every later stroke on this context is dashed --- the mark
    // outlines above are painted on the same canvas on the next frame.
    ctx.setLineDash([]);
  }

  /**
   * What a crop would keep, and what it would throw away.
   *
   * **The scrim is the preview, and the rectangle is the hole in it.** A dashed
   * outline alone --- which is what every other gesture here gets --- says "a
   * rectangle is being dragged" and says nothing about which side of it
   * survives. A crop is the one gesture in the application that *removes*
   * something the reader can see, so the feedback has to be about the removal:
   * the four bands outside the rectangle are darkened, and what stays bright is
   * exactly what the page will become.
   *
   * Four `fillRect`s rather than one path with a hole, because the even-odd fill
   * rule and a reversed sub-path are two more things to get right for a result
   * a reader cannot tell apart. Clipped to the page and not to the viewport, so
   * a reader who has scrolled the page's top out of view still sees the scrim
   * end where the paper does.
   */
  private paintCropPreview(ctx: CanvasRenderingContext2D, dpr: number): void {
    const live = this.cropDrawing;
    if (!live) return;
    // **The redaction's preview is this one inverted, and the inversion is the
    // point.** Both gestures are the same drag; a crop keeps the inside and a
    // redaction destroys it, so shading the same band for both would leave the
    // reader with no way to tell from the screen which tool is armed. The scrim
    // marks what goes: outside the rectangle for a crop, inside it for a
    // redaction.
    const marking = this.redacting;
    const origin = this.scroller.pageOrigin(live.slot);
    const size = this.laidSize(live.slot);
    // The page's own rectangle on screen, which the scrim is bounded by.
    const px0 = origin.left * dpr;
    const py0 = (origin.top - this.scrollTop) * dpr;
    const px1 = (origin.left + size.width * this.zoom) * dpr;
    const py1 = (origin.top + size.height * this.zoom - this.scrollTop) * dpr;

    // Clamped exactly as the committed rectangle will be, so the preview and the
    // result cannot disagree about a drag that left the page --- which is one
    // flick of the wrist at the edge and the commonest way to draw a crop.
    const quad = boxQuad(live.from, live.to, size);
    const kept = quad ?? { left: 0, top: 0, right: 0, bottom: 0 };
    const kx0 = (origin.left + kept.left * this.zoom) * dpr;
    const ky0 = (origin.top + kept.top * this.zoom - this.scrollTop) * dpr;
    const kx1 = (origin.left + kept.right * this.zoom) * dpr;
    const ky1 = (origin.top + kept.bottom * this.zoom - this.scrollTop) * dpr;

    ctx.save();
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = marking ? REDACT_FILL : CROP_SCRIM;
    // Which bands those are is `scrimBands`, not this loop: the inversion is
    // the whole distinction between the two tools and a branch written inside
    // this call sequence would be reachable by no test --- the fake DOM's
    // `getContext` answers `null`, so nothing here runs under vitest at all.
    for (const [x, y, w, h] of scrimBands(
      marking,
      { x0: px0, y0: py0, x1: px1, y1: py1 },
      { x0: kx0, y0: ky0, x1: kx1, y1: ky1 },
    )) {
      ctx.fillRect(x, y, w, h);
    }

    // The outline as well, dashed, which is what says the rectangle is still
    // being dragged rather than already applied --- the scrim alone would look
    // like a crop that had happened and left the rest of the paper showing.
    // Blue for a crop and red for a redaction, matching the fill each one drew.
    ctx.strokeStyle = marking ? REDACT_EDGE : PREVIEW_STROKE;
    ctx.lineWidth = Math.max(1, OUTLINE_WIDTH * this.zoom * dpr);
    ctx.setLineDash([6 * dpr, 4 * dpr]);
    ctx.strokeRect(kx0, ky0, kx1 - kx0, ky1 - ky0);
    ctx.restore();
  }

  /**
   * The bubble the comment tool would drop, following the pointer.
   *
   * **This is what the reader asked the cursor to become.** Arming a shape tool
   * gives a crosshair and nothing else, which is enough for a shape --- the
   * reader is about to drag out a rectangle and will watch it grow. A comment
   * is one press, so between arming and pressing there is no feedback at all,
   * and the command that used to drop the bubble at the top-left corner of the
   * page reads as one that ignores where you are pointing. Reported from use.
   *
   * A cursor image was the obvious answer and is the worse one. It cannot show
   * the *size* the icon will be, so it would not move with the zoom; it is drawn
   * by the window server rather than by us, so it cannot sit under the page's
   * own colours; and a `url(data:...)` in a `cursor` rule is an image load,
   * which the application's `default-src 'self'` policy has an opinion about.
   * This is a shape on our own canvas, in the same dashed preview blue as every
   * other gesture in progress, and it goes exactly where the mark will go
   * because {@link iconQuad} is what places both.
   *
   * Nothing is painted once the press begins: `drawing` being set means the
   * bubble's position is decided, and a ghost still tracking the pointer would
   * be a second bubble in a place the mark is not going.
   */
  private paintCommentGhost(ctx: CanvasRenderingContext2D, dpr: number): void {
    if (this.drawKind !== "note" || this.drawing) return;
    const at = this.armedAt;
    if (!at) return;
    if (!this.scroller.visiblePages().includes(at.slot)) return;

    const quad = iconQuad(at.x, at.y, this.laidSize(at.slot));
    const origin = this.scroller.pageOrigin(at.slot);
    const left = (origin.left + quad.left * this.zoom) * dpr;
    const top = (origin.top + quad.top * this.zoom - this.scrollTop) * dpr;
    const width = (quad.right - quad.left) * this.zoom * dpr;
    const height = (quad.bottom - quad.top) * this.zoom * dpr;

    ctx.save();
    ctx.globalCompositeOperation = "source-over";
    // Filled faintly as well as outlined, because an outline alone at this size
    // is a few dashes and reads as a smudge over text. The fill is the preview
    // blue at a fifth, so what is underneath stays legible.
    ctx.fillStyle = GHOST_FILL;
    ctx.strokeStyle = PREVIEW_STROKE;
    ctx.lineWidth = Math.max(1, OUTLINE_WIDTH * this.zoom * dpr);
    ctx.setLineDash([5 * dpr, 3 * dpr]);
    drawBubble(ctx, left, top, width, height, PREVIEW_STROKE);
    ctx.restore();
  }

  /**
   * The drawing in progress: the strokes already made, and the one being drawn.
   *
   * **In the preview colour rather than the mark's**, which is the rule the
   * box's dashed outline states: a preview is not yet a mark, and a reader who
   * presses Enter expects something to change. Dashing a freehand line would
   * make it look like a different *kind* of mark rather than an unfinished one,
   * so the colour carries it and the weight stays honest at {@link INK_WIDTH}.
   *
   * Both in one pass, because they are one thing to the reader: the stroke under
   * the pen is the newest part of the drawing, not a separate object.
   */
  private paintInkPreview(ctx: CanvasRenderingContext2D, dpr: number): void {
    const pending = this.inking;
    const live = this.drawKind === "ink" ? this.drawing : null;
    if (!pending && !live) return;
    const slot = pending?.slot ?? live?.slot;
    if (slot === undefined) return;

    const origin = this.scroller.pageOrigin(slot);
    ctx.globalCompositeOperation = "source-over";
    ctx.strokeStyle = PREVIEW_STROKE;
    ctx.lineWidth = INK_WIDTH * this.zoom * dpr;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";

    const strokes = [...(pending?.strokes ?? [])];
    // The live one only while it is on the same page as the rest --- `begin`
    // refuses a stroke elsewhere, so this can only differ before the first
    // stroke is kept, and then `pending` is null and the live one is the page.
    if (live && live.slot === slot) strokes.push(live.points);

    for (const stroke of strokes) {
      const [first, ...rest] = stroke;
      if (!first) continue;
      ctx.beginPath();
      ctx.moveTo(
        (origin.left + first.x * this.zoom) * dpr,
        (origin.top + first.y * this.zoom - this.scrollTop) * dpr,
      );
      for (const point of rest) {
        ctx.lineTo(
          (origin.left + point.x * this.zoom) * dpr,
          (origin.top + point.y * this.zoom - this.scrollTop) * dpr,
        );
      }
      ctx.stroke();
    }
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
      (slot) => this.textOn(slot),
      (slot) => {
        const source = this.pages.sourceOf(slot);
        return source !== undefined && this.searcher.unreadablePage(source);
      },
    );
    this.a11y.announce(this.currentPage());
  }

  /** The screen-reader text layer. For the check harness. */
  get accessibleText(): AccessibleText {
    return this.a11y;
  }

  /**
   * Asks for a page's text once, waking the loop when it lands.
   *
   * Keyed by the page of the *file*, like the cache itself: a page's text is a
   * property of the document rather than of where it currently sits, so a
   * deletion above it must not make it be fetched again.
   */
  private requestText(slot: number): void {
    const source = this.pages.sourceOf(slot);
    if (source === undefined) return;
    if (this.textAsked.has(source)) return;
    this.textAsked.add(source);
    void this.text.load(source).then(() => this.wake());
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
