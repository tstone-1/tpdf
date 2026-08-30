/**
 * Which page of the file is in which slot of the viewer.
 *
 * **Two vocabularies, and everything here is the translation between them.** The
 * viewer addresses pages by *slot*: slot 0 is the first page on screen, and every
 * array it holds --- sizes, boxes, tile epochs, thumbnail rows --- is indexed that
 * way. The backend addresses them by *source*: the page of the file the content
 * comes from, which is what a tile request, `page_text` and `search_page` all
 * mean by "page", and what the outline, the links and the comments name in their
 * answers.
 *
 * The two were the same number until a page could be deleted, which is why this
 * module did not exist before: a translation that is the identity function cannot
 * be told from a broken one by any test, and `docs/TRAPS.md` has that under *"a
 * property that holds by construction cannot test the thing it resembles"*.
 *
 * **This holds no rules.** It is built from the `pages` of an `EditState`, which
 * the model in Rust produced, and it answers questions about that answer. When
 * the model replies again, the map is replaced rather than updated --- the same
 * posture `edits.ts` takes, and for the same reason: a map that computed its own
 * next state would be a second implementation of the journal.
 */

import type { Comment } from "./comments";
import type { Link } from "./links";
import type { OutlineItem, Target } from "./outline";

/**
 * The model's name for a page, which is not its position on screen.
 *
 * **A `number` at runtime and a distinct type at compile time**, because the two
 * page numbers in this application are both small integers and were told apart
 * by nothing. A slot is where a page sits in the order the reader is looking at,
 * counted from 0; an id is what the model calls that page, counted from 1 and
 * unchanged by a deletion or a move. Every command the backend takes names a
 * page by id.
 *
 * The confusion is not hypothetical. `Viewer.onDrawn` hands the id of the page a
 * gesture happened on --- deliberately, so a re-order during the drag cannot
 * move the mark --- and `Edits.mark` took a slot and indexed `pages` by it. So a
 * box drawn on the first page of an unedited document was written to the second,
 * and one drawn on the last page was dropped with no message, because there is
 * no slot past the end. Every gate was green: `number` accepts `number`.
 *
 * {@link pageId} is the only way to make one, so the places that mint an id are
 * countable --- `unedited` here, and the check harness building marks by hand.
 * Everything else receives one from the model and passes it on.
 */
export type PageId = number & { readonly __pageId: unique symbol };

/**
 * Names a number as a page id.
 *
 * A cast with a name on it, which is all a branded type can offer: it cannot
 * check that the caller was holding an id rather than a slot. What it buys is
 * that the assertion is *visible* --- a slot reaching a model command now has to
 * be written down as one, rather than passing silently because both are numbers.
 */
export function pageId(value: number): PageId {
  return value as PageId;
}

/**
 * A page of the *file*, as opposed to a slot on screen or a {@link PageId}.
 *
 * The third number in this module, and the one that had no name until a drag
 * stopped selecting text. `TextCache` is keyed by page of the file, deliberately
 * --- a page's text is a property of the document, so a deletion above it must
 * not make it be fetched again --- and `viewer.ts` translated at three of its
 * eighteen call sites. The other fifteen passed a slot, so on any document with
 * an edit in it the paint path looked under one key while the loader stored
 * under another: measured, a drag that selects `"ab"` on an untouched document
 * selects nothing at all once the first page is deleted.
 *
 * A slot and a page of the file are both `number` and are equal on every
 * document nobody has edited, which is why nothing caught it and why a rule
 * ("translate at the call sites") was the wrong instrument. This is the
 * mechanism: {@link PageMap.sourceOf} is the only thing that mints one, so a
 * slot reaching the cache is now a type error rather than a silent miss.
 */
export type FilePage = number & { readonly __filePage: unique symbol };

/**
 * Names a number as a page of the file.
 *
 * {@link pageId}'s counterpart, and the same honest limit: a cast with a name on
 * it. Callers outside this module should be getting one from
 * {@link PageMap.sourceOf} rather than minting it --- what needs this are the
 * places holding a number that came from the file in the first place, which is
 * `unedited` here and the harnesses that build a document by hand.
 */
export function filePage(value: number): FilePage {
  return value as FilePage;
}

/**
 * What kind of mark a reader made. The names are the wire format --- see
 * `MarkKind` in `docmodel.rs`, which is where the set is closed.
 *
 * Here rather than in `edits.ts`, where the command that sends one lives, for
 * the reason the note above {@link PageView} gives: this module must not import
 * the module that talks to Tauri, and {@link MarkView} needs the type.
 */
export type MarkKind =
  | "highlight"
  | "underline"
  | "strikeout"
  | "squiggly"
  | "note"
  | "square"
  | "ellipse"
  | "textbox"
  | "ink"
  | "stamp";

/**
 * Which standard stamp a `"stamp"` mark is.
 *
 * `StampName` in `docmodel.rs`. Four of the fourteen names PDF 32000-1 lists,
 * chosen there rather than here --- a name outside that list could not be
 * written as `/Name`, which is what a reader that synthesises its own stamp
 * appearance draws from.
 */
export type StampName = "approved" | "confidential" | "draft" | "final";

/** The word a stamp draws, which is upper case because a stamp is. */
export function stampWord(name: StampName): string {
  switch (name) {
    case "approved":
      return "APPROVED";
    case "confidential":
      return "CONFIDENTIAL";
    case "draft":
      return "DRAFT";
    case "final":
      return "FINAL";
  }
}

/**
 * One region a reader has marked for removal, as the backend reports it.
 *
 * Mirrors `edits::RedactionView`, and lives here for {@link MarkView}'s reason:
 * the overlay draws these and must not import the module that talks to Tauri.
 *
 * **A separate type from {@link MarkView} rather than another {@link MarkKind}**,
 * and the reason is the one `docmodel.rs` states: a mark is written into the
 * saved file as an annotation and a redaction must never be. Keeping them apart
 * here as well as in Rust means nothing that paints marks can paint one of these
 * by accident, and nothing that lists marks can offer one for saving.
 */
export interface RedactionView {
  /** The model's identity, sent back verbatim to take it off again. */
  id: number;
  /** The page it is on, by {@link PageView.id} --- never a slot. */
  page: PageId;
  /**
   * `left, top, right, bottom` in the page's **display** space, the same space
   * {@link MarkView.quads} uses.
   *
   * One rectangle rather than a list: a redaction is a region a reader dragged
   * out, not a run of words.
   */
  area: [number, number, number, number];
}

/**
 * One comment out of the file that the reader has rewritten.
 *
 * Mirrors `edits::NoteEditView`, and lives here with {@link MarkView} and
 * {@link RedactionView} for their reason --- but the consumer is different, and
 * it is what decides the shape. Those two are drawn; this one is **joined**, by
 * {@link commentsIn}, onto the comments a scan of the file produced. Without
 * that join a reader would edit a comment and see the file's own words in the
 * panel until they saved and opened it again.
 *
 * **Addressed by the object rather than by `Comment.id`**, which is a position
 * in one scan and moves whenever an earlier comment is inserted. A comment whose
 * `object` is `null` cannot be edited at all --- see `Comment.object`.
 */
export interface NoteEdited {
  /** The annotation object, as `[number, generation]`. */
  object: [number, number];
  /** The page it is on, by {@link PageView.id} --- never a slot. */
  page: PageId;
  /** What the comment now says. Replaces `Comment.body` whole. */
  body: string;
  /**
   * When the reader typed it, in PDF date form.
   *
   * The writer's form. Carried so that nothing on this side has to build one,
   * and shown to nobody --- {@link shown} is the reader's.
   */
  made: string;
  /**
   * The same moment as `YYYY-MM-DD HH:MM`, or `null` if it would not parse.
   *
   * Built in Rust by the same `parse_date` the scan uses, so an edited comment's
   * byline is in the same shape as the byline of the row above it.
   */
  shown: string | null;
}

/**
 * What removing one region would take, and what it would miss.
 *
 * Mirrors `redact::RegionPlan`, and it is the one thing about a pending
 * redaction the frontend cannot work out for itself: which text-showing
 * operations the region's characters belong to is a fact about the content
 * stream, and route B removes a whole operation when any of its glyphs is
 * inside. So {@link taking} is at least the words the region covers and
 * commonly the rest of the line --- which is exactly what a reviewer is looking
 * for.
 */
export interface RegionPlan {
  /**
   * Which of the page's text-showing operations the removal would delete.
   *
   * Ordinals, and they mean nothing here: they address operators in a content
   * stream this process has never parsed. What the panel reads is how many
   * there are. They cross the boundary because the **coordinator** applies
   * them, and it is one type for one answer rather than two shapes of it.
   *
   * `redact::RegionPlan` also carries `text_objects`, which is deliberately
   * absent here: it is a fact about the page that only the writer needs, and a
   * field nothing reads is a field that goes stale without anything saying so.
   */
  shows: number[];
  /** What those operations draw, in the page's own object order. */
  taking: string;
  /**
   * What the region covers that a removal could not take.
   *
   * Non-empty means the region is **not** redactable, which is the single most
   * important thing this panel can say: the words gone and the picture of the
   * words still there is the confident lie `docs/PLAN.md` §6 forbids.
   */
  unhandled: { at: number; kind: string }[];
  /**
   * Which of the page's images the removal would delete.
   *
   * Ordinals, meaning nothing here for {@link shows}' reason; what the panel
   * reads is **how many**. It has to say so, because taking a whole picture is
   * the one consequence of a redaction a reader cannot undo afterwards and
   * cannot see from the region they drew: an image is removed entire, since
   * removing part of one means decoding and re-encoding it.
   *
   * Optional because a reply written before images were removable carries no
   * such field, and a panel that read `undefined.length` would break on it.
   */
  images?: number[];
}

/**
 * One mark a reader made, as the backend reports it.
 *
 * Mirrors `edits::MarkView`, and lives here rather than in `edits.ts` for the
 * reason the note above {@link PageView} gives: the viewer paints these and must
 * not import the module that talks to Tauri.
 */
export interface MarkView {
  id: number;
  /**
   * Which of the three marks it is.
   *
   * **Read by the overlay, and this said the opposite.** It read "a label rather
   * than geometry: PDFium paints every mark inside the tile, so nothing here
   * draws from this" --- true of the file's own comment annotations, and wrong
   * about the reader's marks, which the overlay paints itself. Nothing did draw
   * from it, and that was the defect rather than the design: every kind was
   * filled over its whole quad in one colour, so an underline and a strikeout
   * both looked like a highlight until the file was saved and reopened.
   * `markband.ts` is what the overlay asks now. The note box reads it too, to
   * name the thing a reader is about to remove.
   */
  kind: MarkKind;
  /** The page it is on, by {@link PageView.id} --- never a slot. */
  page: PageId;
  /**
   * Four numbers per rectangle --- `left, top, right, bottom` --- in the page's
   * **display** space: points from the displayed page's top-left corner, after
   * the page's own `/Rotate` and before any turn the reader or an edit added.
   * The overlay applies those when it paints.
   */
  quads: number[];
  /**
   * One entry per stroke, each `x y x y ...` in the same display space.
   *
   * Empty for every kind but `ink`, whose shape this *is*: its {@link quads}
   * holds one rectangle, and that rectangle is a box round the drawing rather
   * than the drawing. The overlay paints from here when it is non-empty.
   */
  strokes: number[][];
  /**
   * Which standard stamp this is, for `"stamp"` and nothing else.
   *
   * `StampName` in `docmodel.rs`, where the set is closed. The overlay needs it
   * or it cannot draw a stamp at all: the quads say where the mark is and this
   * says what it says, which is {@link MarkView.strokes}'s situation exactly.
   * `null` for every other kind, and the backend refuses the two ways this can
   * disagree with {@link MarkView.kind}.
   */
  stamp: StampName | null;
  color: [number, number, number];
  /**
   * How thick this mark's ink is, in points, before any zoom.
   *
   * **The overlay needs it or the tile and the overlay disagree.** PDFium paints
   * every saved mark inside the tile at whatever `w` the appearance stream sets,
   * and the overlay redraws ink on top from {@link MarkView.strokes}; a constant
   * here would draw every drawing at the default weight over a tile drawing it
   * at the reader's, which reads as a rendering fault rather than as a wrong
   * number.
   *
   * `Mark::width` in `docmodel.rs`, and fixed when the mark is made --- there is
   * no recolouring counterpart for it, and that entry says what one would cost.
   */
  width: number;
  /**
   * What the reader typed, which may be empty.
   *
   * **Attacker-controlled once a saved file is reopened**, so it is treated as
   * `annots.rs` treats a comment body: it reaches the DOM as text, and nothing
   * here may carry a URL. See `docs/THREAT-MODEL.md` T8.
   */
  note: string;
  /**
   * The note broken into the lines a text box is drawn in, empty otherwise.
   *
   * **The backend wraps, not the overlay.** `ctx.measureText` would measure
   * whatever font the system resolved, and the file is set in Helvetica by
   * `textbox.rs`'s own metrics --- two measurements of two fonts break lines in
   * different places, so a reader would see three lines and save four. There is
   * one layout, in one language, and this is what it produced.
   *
   * Attacker-controlled exactly as {@link MarkView.note} is, and it reaches the
   * page the same way: as text, through the canvas, never as markup.
   */
  lines: string[];
}

/**
 * What supplies a page's content: a page of the file, or a page tpdf made.
 *
 * Mirrors `docmodel::PageSource`, which serialises externally tagged --- so a
 * baseline page arrives as `{ baseline: 3 }` and a page tpdf made as
 * `{ blank: { width: 595, height: 842 } }`. The two payload shapes differ, which
 * is what lets `"baseline" in source` narrow it.
 *
 * **A page tpdf made has no number in the file, and that is the whole of what
 * this type exists to say.** Every consumer that asks a worker for a tile, for a
 * page's text or for its objects addresses the page by that number, so each of
 * them has to answer what it does when there is none --- see
 * {@link PageMap.sourceOf}, which answers `undefined` and documents why it must
 * not fall back to the slot.
 */
export type PageSource =
  | { readonly baseline: number }
  | { readonly blank: { readonly width: number; readonly height: number } };

/**
 * The baseline page a source names, or `undefined` for a page tpdf made.
 *
 * A free function rather than a method on the union, because the union is a
 * wire shape and has no methods; and named for the answer rather than for the
 * question, so a caller reads it as "which page of the file is this".
 */
export function baselineOf(source: PageSource): FilePage | undefined {
  return "baseline" in source ? filePage(source.baseline) : undefined;
}

/**
 * The size of a page tpdf made, or `undefined` for a page of the file.
 *
 * {@link baselineOf}'s mirror, and the reason both exist rather than one: a page
 * has a size here exactly when it has no baseline number, so a caller that
 * needs the size is the one that got `undefined` from the other. A page of the
 * file has a size too and it is the *file's* answer, which the render path
 * reports and nothing here may contradict.
 */
export function madeSizeOf(
  source: PageSource,
): { readonly width: number; readonly height: number } | undefined {
  return "blank" in source ? source.blank : undefined;
}

/**
 * One page of the working document, as the backend reports it.
 *
 * Mirrors `edits::PageView`. It lives here rather than in `edits.ts` so that the
 * modules that only need the *shape* --- the scroller, the thumbnails --- do not
 * have to import the module that talks to Tauri, which cannot be loaded outside
 * a webview.
 */
export interface PageView {
  /**
   * The model's identity for this page, sent back verbatim in a command.
   *
   * A `u64` in Rust and a JavaScript number here, which is exact for every id
   * the model can currently issue --- they are allocated from 1 upwards, one per
   * baseline page. **An allocator that ever issues an id past 2^53 breaks this
   * silently**, rotating whichever page the rounded value happens to name, so
   * that is a constraint on the allocator `docmodel.rs` says has yet to be
   * written rather than a property of this line.
   */
  id: PageId;
  /** What supplies the content --- see {@link PageSource}. */
  source: PageSource;
  /** Quarter turns clockwise an edit has applied, on top of the page's own. */
  turns: number;
  /**
   * The page's visible box as the reader has set it, or absent for the file's
   * own.
   *
   * `[llx, lly, urx, ury]` in the **page's own** space, y upwards --- the one
   * value here that is not in display space, because it decides what display
   * space is and expressing it there would be circular.
   *
   * Nothing in the frontend does arithmetic with it beyond passing it back to
   * the renderer. What the *layout* needs is a page's displayed size and where
   * the crop sits inside the file's own page, and neither can be computed from
   * this without the page's `/Rotate`, which the frontend is never told: see
   * `edits.ts`'s `pageGeometry`, which asks.
   */
  crop?: readonly [number, number, number, number];
}

/**
 * The order the reader is looking at, and both directions of the translation.
 *
 * Immutable. Built once per state reply, and small: one entry per live page.
 */
export class PageMap {
  private readonly views: readonly PageView[];
  /**
   * Source page to slot.
   *
   * A page of the file appears at most once in the working document today,
   * because nothing duplicates a page --- `docmodel.rs` says so and says what
   * would have to be proved first. Were one to appear twice, this would answer
   * with the first of them, which is why {@link slotOf} says "the first slot".
   */
  private readonly bySource: Map<number, number>;

  constructor(views: readonly PageView[]) {
    this.views = views;
    this.bySource = new Map();
    for (let slot = 0; slot < views.length; slot++) {
      const view = views[slot];
      // A page tpdf made has no baseline number, so it is in no direction of
      // this map: nothing arriving from the backend can name it, because
      // everything that does names a page of the file.
      const source = view === undefined ? undefined : baselineOf(view.source);
      if (source === undefined || this.bySource.has(source)) continue;
      this.bySource.set(source, slot);
    }
  }

  /** How many pages the reader sees. */
  get length(): number {
    return this.views.length;
  }

  /** The pages, in reading order. */
  get pages(): readonly PageView[] {
    return this.views;
  }

  /** The page in a slot, or `undefined` if there is none. */
  at(slot: number): PageView | undefined {
    return this.views[slot];
  }

  /**
   * Which page of the file a slot draws, or `undefined` for a slot that is not
   * in the document.
   *
   * Deliberately not falling back to the slot. That fallback is the identity
   * function, it is right for every document nobody has edited, and it is wrong
   * in exactly the case this class exists for --- so a caller that cannot handle
   * a missing slot must not ask for a tile rather than ask for the wrong one.
   */
  sourceOf(slot: number): FilePage | undefined {
    const view = this.views[slot];
    return view === undefined ? undefined : baselineOf(view.source);
  }

  /**
   * The size of the page in a slot, when tpdf made it rather than the file.
   *
   * `undefined` for a page of the file, whose size the render path reports ---
   * see {@link madeSizeOf}. The layout needs this because nothing will ever
   * render a made page and report its size: {@link sourceOf} answers
   * `undefined` for it precisely so that no tile is asked for.
   */
  madeSizeOf(
    slot: number,
  ): { readonly width: number; readonly height: number } | undefined {
    const view = this.views[slot];
    return view === undefined ? undefined : madeSizeOf(view.source);
  }

  /**
   * The page of the file a slot draws, or the nearest one before it.
   *
   * For the one caller that must answer with a *file* page and cannot skip:
   * remembering where the reader was. A page tpdf made is in no file, so a
   * place naming it could not be restored --- and the honest answer is the page
   * they would have been on had they not inserted it, which is the one before.
   *
   * `undefined` only when no slot at or before this one shows a page of the
   * file, which for a real document means the reader is on a made page at the
   * very front.
   */
  nearestSourceAt(slot: number): FilePage | undefined {
    for (let at = Math.min(slot, this.views.length - 1); at >= 0; at--) {
      const view = this.views[at];
      const source = view === undefined ? undefined : baselineOf(view.source);
      if (source !== undefined) return source;
    }
    return undefined;
  }

  /**
   * The first slot showing a page of the file, or `undefined` if it was deleted.
   *
   * The direction everything arriving from the backend needs: a link's rectangle,
   * a comment's page, an outline destination and a search result all name a page
   * of the file, and the viewer can only draw or scroll to a slot.
   */
  slotOf(source: number): number | undefined {
    return this.bySource.get(source);
  }

  /**
   * The first slot showing the page with this identity, or `undefined`.
   *
   * The direction a *mark* needs: the model keys a highlight by the page's id,
   * because a slot is not a name for a page, and the overlay draws in slots.
   * {@link slotOf} answers the same question for a baseline page number, which
   * is what links and comments carry --- two different keys because they come
   * from two different places, and conflating them would silently work until a
   * page moved.
   */
  slotOfId(id: PageId): number | undefined {
    for (let slot = 0; slot < this.views.length; slot++) {
      if (this.views[slot]?.id === id) return slot;
    }
    return undefined;
  }

  /** Quarter turns an edit has applied to the page in a slot, 0 to 3. */
  turnsOf(slot: number): number {
    return this.views[slot]?.turns ?? 0;
  }

  /** The model's identity for the page in a slot, for sending a command back. */
  idOf(slot: number): PageId | undefined {
    return this.views[slot]?.id;
  }

  /**
   * The page of the file each slot draws, in slot order. For the tests.
   *
   * Nothing in the application asks for the whole list --- every consumer asks
   * about one slot --- and it is here because an assertion reading
   * `[0, 2, 3]` says what a deletion did, where three separate `sourceOf`
   * calls say it three times.
   */
  sources(): (FilePage | undefined)[] {
    return this.views.map((page) => baselineOf(page.source));
  }

  /**
   * Whether two maps put the same pages in the same slots.
   *
   * Identity and order only --- **not** the turns, which is the distinction the
   * viewer branches on: a turn is a change to one page that the layout can
   * absorb in place, and a change of order moves every slot after it and
   * invalidates everything keyed by one.
   */
  sameOrder(other: PageMap): boolean {
    if (this.views.length !== other.views.length) return false;
    return this.views.every((page, slot) => page.id === other.views[slot]?.id);
  }

  /**
   * Where the page that was in `slot` of `before` has gone, or `undefined`.
   *
   * By identity rather than by source, so it stays right when a page can be
   * duplicated. Used to carry per-slot state --- a learned page size, a tile
   * epoch --- across an edit rather than throwing it away.
   */
  slotFrom(before: PageMap, slot: number): number | undefined {
    const id = before.idOf(slot);
    if (id === undefined) return undefined;
    const found = this.views.findIndex((page) => page.id === id);
    return found === -1 ? undefined : found;
  }
}

/** The map of a document with nothing open, which has no pages. */
export const NO_PAGES = new PageMap([]);

/**
 * The links of the working document: rectangles in slots, targets in slots.
 *
 * A link is a rectangle **on** a page and a destination **to** a page, and both
 * are pages of the file as `links.rs` reports them. A link whose own page is
 * gone goes with it --- there is nowhere to draw it. A link whose *destination*
 * is gone stays, and becomes `broken`, which already means "points at a page
 * this document does not have" and is exactly what a reader has done by deleting
 * it.
 */
export function linksIn(items: readonly Link[], pages: PageMap): Link[] {
  const mapped: Link[] = [];
  for (const link of items) {
    const slot = pages.slotOf(link.page);
    if (slot === undefined) continue;
    mapped.push({ ...link, page: slot, target: targetIn(link.target, pages) });
  }
  return mapped;
}

/**
 * The comments of the working document, those on deleted pages left out.
 *
 * **And with the reader's own edits written over them**, which is the second job
 * and the reason `notes` is a parameter rather than something a caller applies
 * afterwards. The scan is a reading of the file on disk and the model is what
 * has happened since; a consumer that saw the first without the second would
 * show a reader the words they had just replaced. Doing it here means every
 * consumer --- the panel, the popup, the overlay's hit test --- is answered from
 * one join rather than each remembering to look.
 *
 * Matched on `object`, never on `Comment.id`: an id is a position in one scan
 * and a plan crosses a process boundary. A comment whose `object` is `null`
 * matches nothing, which is right --- the model cannot name one either.
 *
 * `notes` defaults to empty so that a caller with no model in hand --- a test, or
 * a document opened before the first state reply --- gets the scan unchanged
 * rather than having to pass a placeholder.
 */
export function commentsIn(
  items: readonly Comment[],
  pages: PageMap,
  notes: readonly NoteEdited[] = [],
): Comment[] {
  const mapped: Comment[] = [];
  for (const comment of items) {
    const slot = pages.slotOf(comment.page);
    if (slot === undefined) continue;
    const edit = comment.object
      ? notes.find(
          (note) =>
            note.object[0] === comment.object?.[0] &&
            note.object[1] === comment.object[1],
        )
      : undefined;
    mapped.push(
      edit
        ? { ...comment, page: slot, body: edit.body, date: edit.shown }
        : { ...comment, page: slot },
    );
  }
  return mapped;
}

/**
 * The reader's own marks, in the order a keyboard walk should meet them.
 *
 * The other three translators here answer *where does this belong now*; this one
 * also answers *which comes first*, and the two are the same question asked of a
 * mark: a `MarkView` carries the page's **id**, so its position in the document
 * is whatever slot that id is in today. A reader who moves page 9 to the front
 * meets its highlights first, and nothing about the marks themselves changed.
 *
 * The rectangle is the union of the mark's quads in **display** space --- before
 * any turn the reader or an edit added, which is the space `Place.top` is
 * comparable in. See {@link stepAlong}, which does the comparing.
 *
 * Ordered by slot, then by the union's top edge, then by id. The last is not
 * decoration: two marks can share a top edge exactly --- a reader marking the
 * same line twice --- and without it which one is "next" would depend on the
 * sort's stability rather than on a rule. Same argument as `orderLinks`.
 */
export function markWalk(
  items: readonly MarkView[],
  pages: PageMap,
): { id: number; page: number; rect: [number, number, number, number] }[] {
  const walk: { id: number; page: number; rect: [number, number, number, number] }[] =
    [];
  for (const mark of items) {
    const slot = pages.slotOfId(mark.page);
    if (slot === undefined) continue;
    const rect = unionOf(mark.quads);
    // A mark with no rectangles is nowhere, so it is not somewhere a walk can
    // stop. The model does not make one; a saved file reopened might.
    if (!rect) continue;
    walk.push({ id: mark.id, page: slot, rect });
  }
  return inPageOrder(walk);
}

/** Something the reader put on a slot, for {@link inPageOrder} to order. */
interface OnPage {
  id: number;
  page: number;
  rect: [number, number, number, number];
}

/**
 * Slot, then top edge, then id --- sorted in place and handed back.
 *
 * The rule {@link markWalk} argues, applied to the redaction list as well, and
 * one function because it is one rule rather than two lists that happen to agree
 * today. **The id is not decoration**: two things can share a top edge exactly,
 * a reader marking the same line twice or dragging two regions along one, and
 * without it which comes first would depend on the sort's stability rather than
 * on anything anybody decided.
 *
 * What the two callers do *not* share is which things get here at all: a mark
 * with no rectangles is nowhere and a walk cannot stop on it, while a region
 * always has one. That is a question about the item, asked before this.
 */
function inPageOrder<T extends OnPage>(items: T[]): T[] {
  items.sort((a, b) => {
    if (a.page !== b.page) return a.page - b.page;
    if (a.rect[1] !== b.rect[1]) return a.rect[1] - b.rect[1];
    return a.id - b.id;
  });
  return items;
}

/** One row of the marks panel: a mark, and the slot it is drawn on. */
export interface MarkRow {
  mark: MarkView;
  /**
   * Slot the mark is drawn on, or `null` when nothing could place it.
   *
   * Not reachable from the model as it stands --- `edits::snapshot` walks the
   * live pages, so a mark on a deleted page is not in the list at all, and
   * `annotate` refuses a mark that covers nothing. It is here because a panel
   * that *silently* drops a row tells a reader their mark is gone, and the two
   * sibling panels already made the other choice: `outline.ts` draws a row for
   * a destination that resolves to no page, and `commentlist.ts` draws one for
   * a reply whose parent was cut.
   */
  page: number | null;
}

/**
 * The reader's own marks, in the order a panel should list them.
 *
 * **The order is {@link markWalk}'s, because it has to be the same order.** The
 * keyboard walk and the panel are two ways of meeting the same marks, and a
 * reader who steps with the walk key and reads down the list must see them agree
 * --- a second sort here would be the trap this repository records as *"Two
 * copies of a distinction drift, and a mutation of one survives"*, with the two
 * copies being "which mark comes next".
 *
 * What this adds is that nothing is dropped. The walk leaves out a mark it
 * cannot place, which is right for stepping --- there is nowhere to step to ---
 * and wrong for a list, so those come last with no page against them.
 */
export function markRows(items: readonly MarkView[], pages: PageMap): MarkRow[] {
  const left = new Map<number, MarkView>(items.map((mark) => [mark.id, mark]));
  const rows: MarkRow[] = [];
  for (const step of markWalk(items, pages)) {
    const mark = left.get(step.id);
    if (!mark) continue;
    left.delete(step.id);
    rows.push({ mark, page: step.page });
  }
  // In the order they were made, which is the order the model reports them in.
  for (const mark of items) {
    if (left.has(mark.id)) rows.push({ mark, page: null });
  }
  return rows;
}

/**
 * Pairs the plans a backend answered with the regions they were asked about.
 *
 * **Refuses rather than zipping when the counts disagree**, and that is the
 * whole reason this is a function. The reply is a list in the order the request
 * put the regions, so one plan too few would attach every later plan to the
 * wrong region --- and a plan is a claim about what a removal takes, so the
 * failure is a reader shown the wrong words next to the wrong rectangle. The
 * trap index calls the general shape *"a test cannot see the direction of an
 * attachment it puts in index order"*; here the fix is that a mismatch produces
 * nothing at all.
 *
 * Empty is the honest answer for a mismatch: the rows then say nothing about
 * what a removal would take, which is what they said before the reply arrived.
 */
export function pairPlans(
  asked: readonly RedactionView[],
  plans: readonly RegionPlan[],
): Map<number, RegionPlan> {
  const out = new Map<number, RegionPlan>();
  if (asked.length !== plans.length) return out;
  asked.forEach((region, at) => {
    const plan = plans[at];
    if (plan) out.set(region.id, plan);
  });
  return out;
}

/** One row of the redactions panel: a region, and the slot it is drawn on. */
export interface RedactionRow {
  redaction: RedactionView;
  /**
   * Slot the region is drawn on, or `null` when nothing could place it.
   *
   * Not reachable from the model as it stands, for {@link MarkRow.page}'s exact
   * reason --- `Working::all_redactions` walks the live page order, so a region
   * on a deleted page is not in the list at all. It is here because this panel
   * is the one place a reader can take a region off again, and a row silently
   * dropped from *this* list says a redaction they cannot see is no longer
   * pending. That is the one thing a redaction review must never say.
   */
  page: number | null;
}

/**
 * The regions marked for removal, in the order they should be reviewed.
 *
 * `markRows`'s twin, and deliberately a second function rather than a generic
 * over both: a mark's rectangle is the union of its quads and a region *is* a
 * rectangle, so the shapes differ exactly where the two types are meant to
 * differ. What they share --- the ordering, and the rule that nothing is
 * dropped --- is shared as {@link inPageOrder} and as this comment.
 *
 * `docs/PLAN.md` §6 step 2 asks for the marks "listed with page", which is page
 * order: down the document as the reader will read it, not the order the
 * regions were dragged in.
 */
export function redactionRows(
  items: readonly RedactionView[],
  pages: PageMap,
): RedactionRow[] {
  const placed: (OnPage & { redaction: RedactionView })[] = [];
  const lost: RedactionRow[] = [];
  for (const redaction of items) {
    const slot = pages.slotOfId(redaction.page);
    if (slot === undefined) {
      lost.push({ redaction, page: null });
      continue;
    }
    placed.push({ id: redaction.id, page: slot, rect: redaction.area, redaction });
  }
  const rows: RedactionRow[] = inPageOrder(placed).map((step) => ({
    redaction: step.redaction,
    page: step.page,
  }));
  return [...rows, ...lost];
}

/** The smallest rectangle covering every quad, or `null` if there are none. */
function unionOf(
  quads: readonly number[],
): [number, number, number, number] | null {
  let box: [number, number, number, number] | null = null;
  for (let at = 0; at + 3 < quads.length; at += 4) {
    const left = quads[at] ?? 0;
    const top = quads[at + 1] ?? 0;
    const right = quads[at + 2] ?? 0;
    const bottom = quads[at + 3] ?? 0;
    box = box
      ? [
          Math.min(box[0], left),
          Math.min(box[1], top),
          Math.max(box[2], right),
          Math.max(box[3], bottom),
        ]
      : [left, top, right, bottom];
  }
  return box;
}

/**
 * The outline with every destination in slots, and the dead ones marked broken.
 *
 * The tree is kept whole: an entry whose page has gone is still a heading with
 * children under it, and dropping it would take a chapter's subsections out of
 * the table of contents because the chapter's title page was deleted.
 */
export function outlineIn(
  items: readonly OutlineItem[],
  pages: PageMap,
): OutlineItem[] {
  return items.map((item) => ({
    ...item,
    target: targetIn(item.target, pages),
    children: outlineIn(item.children, pages),
  }));
}

/**
 * One destination in slots, or `broken` if its page is not in the document.
 *
 * `broken` rather than a variant of its own. Its wording --- "points at a page
 * this document does not have" --- is already the true sentence for a page the
 * reader deleted, and a new variant would have to be mirrored in `outline.rs`'s
 * `Target`, where nothing can produce it.
 */
function targetIn(target: Target, pages: PageMap): Target {
  if (target.kind !== "page") return target;
  const slot = pages.slotOf(target.page);
  if (slot === undefined) return { kind: "broken" };
  return { ...target, page: slot };
}

/**
 * A map for a document nobody has edited: `count` pages, in order, unturned.
 *
 * The state the backend would answer with, built locally so that the viewer has
 * a map from the first frame rather than from the first reply. The ids match the
 * model's own allocation, which numbers the baseline pages from 1 --- stated in
 * `docmodel::Working::baseline` and mirrored here, because a viewer whose ids
 * were made up would send commands naming pages that do not exist.
 */
export function unedited(count: number): PageMap {
  return new PageMap(
    Array.from({ length: Math.max(0, count) }, (_unused, slot) => ({
      id: pageId(slot + 1),
      source: { baseline: slot },
      turns: 0,
    })),
  );
}
