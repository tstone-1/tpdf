/**
 * Comments: what `annots.rs` sends, and the pure decisions made about it here.
 *
 * The marks themselves are already on the page --- PDFium renders annotation
 * appearances, generating one where a document supplies none --- so nothing in
 * this file paints anything. What it does is answer three questions the DOM side
 * needs: which comment is under the pointer, what order the panel lists them in,
 * and where a comment's rectangle sits once the reader has turned the view.
 *
 * ## Everything here is text a stranger wrote
 *
 * A body, an author and a subject are document-controlled strings, and they go
 * into `textContent` and nowhere else --- `scripts/check_webview_sinks.py` is
 * what keeps that true, and `annots.rs` is what keeps a URL from arriving in a
 * field anything would resolve. The one derived value that is *not* the
 * document's is {@link labelFor}: the kind is an enum from the backend, and this
 * turns it into the word a reader sees.
 *
 * ## The rectangle arrives in display space and the view can be turned again
 *
 * `Comment.rect` is `[left, top, right, bottom]` from the displayed page's
 * top-left corner, after the page's own `/Rotate` --- the same space `text.ts`
 * reports character boxes in. A reader rotating the *view* turns it further, and
 * {@link viewRect} does that through `turnQuad`, which is the one implementation
 * of that turn on this side. Doing it a second way here is how a marker ends up
 * plausibly placed and wrong.
 */

import { coveredText } from "./reading";
import { rowLine } from "./rowline";
import { turnQuad, type PageText, type Quad } from "./text";

/** The kinds `annots.rs` reports, as it serialises them. */
export type CommentKind =
  | "text"
  | "freetext"
  | "highlight"
  | "underline"
  | "squiggly"
  | "strikeout"
  | "square"
  | "circle"
  | "line"
  | "polygon"
  | "polyline"
  | "ink"
  | "stamp"
  | "caret"
  | "fileattachment"
  | "sound"
  | "redact";

/** One comment, as `document_comments` returns it. */
export interface Comment {
  id: number;
  /**
   * The object this annotation is in the file, as `[number, generation]`.
   *
   * {@link Comment.id} is a position in one scan, so it cannot survive a save:
   * inserting a comment on an earlier page renumbers every later one. This is
   * what the file itself calls the annotation, and it is the only name an
   * incremental update can override.
   *
   * `null` means the annotation is a direct dictionary inside the page's
   * `/Annots` array rather than an object of its own, which PDF permits. Such a
   * comment **cannot be edited in place** -- there is nothing to override -- and
   * that is a structural limit rather than a gap. See `annots.rs`.
   */
  object: [number, number] | null;
  page: number;
  kind: CommentKind;
  author: string;
  body: string;
  subject: string;
  date: string | null;
  /** `[left, top, right, bottom]`, points from the displayed page's top-left. */
  rect: [number, number, number, number];
  /**
   * The rectangles a text-markup annotation covers, four values each.
   *
   * Same space as {@link Comment.rect} and the same flat shape `edits.ts` uses
   * for a mark's quads, so {@link coveredText} needs no rotation of its own.
   * Empty for every other kind, and empty for a highlight whose producer wrote
   * no `/QuadPoints` --- see `annots.rs`, which declines a malformed array whole
   * rather than keeping the part of it that parses.
   */
  quads: number[];
  reply_to: number | null;
  hidden: boolean;
  /**
   * `/C`, the colour the *file* gives this annotation, as RGB in 0..1.
   *
   * `null` where the document names no colour and where it names one
   * `annots.rs` could not read --- a `/C` of the wrong length is not a colour
   * with a component missing, it is not a colour.
   *
   * **The rendered tile does not carry it.** PDFium draws a `/Text` icon in its
   * own yellow whatever `/C` says, so a comment shown in the colour somebody
   * chose has to be drawn by the overlay from this field.
   */
  color: [number, number, number] | null;
}

/** What the scan cut, so the panel can say the list is incomplete. */
export interface CommentLimits {
  crowded_pages: number;
  over_budget: boolean;
  bodies_clipped: number;
  unknown_kinds: number;
  unreadable: number;
  cycles: number;
  pages_missed: number;
}

/** A document's comments, as `document_comments` returns them. */
export interface Comments {
  items: Comment[];
  limits: CommentLimits;
  scan_ms: number;
}

/**
 * The least a thing needs to be placed on a page and hit-tested.
 *
 * {@link hitTest}, {@link onPage} and {@link turnedFor} are generic over this
 * rather than over {@link Comment}, so `links.ts` reuses them instead of holding
 * a second copy of the same geometry. A second copy is the trap this repository
 * records as *"Two copies of a distinction drift, and a mutation of one
 * survives"* --- and here it would be a rectangle that a comment and a link
 * sharing one `/Rect` in the file could be drawn at two different places from.
 *
 * `hidden` is optional because only comments have it: `annots.rs` keeps a hidden
 * comment so the panel can still list it, while `links.rs` drops a hidden link
 * at scan time --- there is no panel to list it in, so an unclickable rectangle
 * would be all that survived.
 */
export interface Placed {
  page: number;
  rect: readonly [number, number, number, number];
  hidden?: boolean;
}

/** One row of the panel: a comment, and whether it is a reply. */
export interface CommentRow {
  comment: Comment;
  /** 0 for a comment nobody replied to, 1 for a reply. */
  depth: number;
}

/**
 * Points of slack around a comment's rectangle when hit-testing.
 *
 * A sticky note's icon is 24 points square and a caret's rectangle can be
 * narrower than the pointer is accurate, so an exact test makes small marks feel
 * broken rather than absent. Points rather than pixels: the slack should be the
 * same physical size at every zoom, and at 400% an exact test is already easy.
 */
export const HIT_SLACK_PT = 3;

/** The word a reader sees for a kind. Ours, in their language, not the file's. */
export function labelFor(kind: CommentKind): string {
  switch (kind) {
    case "text":
      return "Note";
    case "freetext":
      return "Text box";
    case "highlight":
      return "Highlight";
    case "underline":
      return "Underline";
    case "squiggly":
      return "Squiggly";
    case "strikeout":
      return "Strikeout";
    case "square":
      return "Rectangle";
    case "circle":
      return "Ellipse";
    case "line":
      return "Line";
    case "polygon":
      return "Polygon";
    case "polyline":
      return "Polyline";
    case "ink":
      return "Drawing";
    case "stamp":
      return "Stamp";
    case "caret":
      return "Insertion";
    case "fileattachment":
      return "Attachment";
    case "sound":
      return "Sound";
    case "redact":
      return "Redaction";
  }
}

/**
 * The panel's rows: every thread, root first, its replies under it.
 *
 * A reply whose parent is not in the list --- cut by a bound, on a page that was
 * not read --- becomes a root of its own rather than disappearing. That is the
 * same rule `annots.rs` applies when resolving `/IRT`, and the reason is the
 * same: a comment nobody can see is a comment lost, and the reader cannot tell
 * the difference between one that was dropped and one that was never written.
 *
 * The walk needs no visited set because `reply_to` is acyclic by construction on
 * the backend --- and it still bounds itself, because a promise made by another
 * process is not one this can check on every render.
 */
export function rowsOf(items: readonly Comment[]): CommentRow[] {
  const byId = new Map<number, Comment>(items.map((item) => [item.id, item]));
  const replies = new Map<number, Comment[]>();
  const roots: Comment[] = [];

  for (const item of items) {
    const parent = item.reply_to !== null ? byId.get(item.reply_to) : undefined;
    if (!parent) {
      roots.push(item);
      continue;
    }
    const siblings = replies.get(parent.id);
    if (siblings) siblings.push(item);
    else replies.set(parent.id, [item]);
  }

  const rows: CommentRow[] = [];
  const emit = (comment: Comment, depth: number, budget: number): void => {
    rows.push({ comment, depth });
    if (budget <= 0) return;
    for (const reply of replies.get(comment.id) ?? []) {
      // Depth 1 for every reply however deep the chain runs: a panel 260 pixels
      // wide has room for one indent, and a reply to a reply is still an answer
      // in the same conversation.
      emit(reply, 1, budget - 1);
    }
  };
  for (const root of roots) emit(root, 0, items.length);
  return rows;
}

/**
 * The comment under a point on a page, or `null`.
 *
 * `x` and `y` are in the page's own displayed space, which is what the viewer's
 * pointer mapping produces --- the same space the rectangles are in, so no
 * zoom or scroll enters here at all.
 *
 * **The smallest hit wins.** A note icon dropped inside a square annotation is
 * inside both, and the one a reader is pointing at is the small one; picking by
 * document order would open whichever the producer happened to write last. Ties
 * go to the later comment, which is the one drawn on top.
 *
 * A hidden comment is never hit: `/F` bit 2 means the page does not show it, so
 * there is no mark under the pointer to have been clicked. It is still listed in
 * the panel, where the reader asked for it explicitly.
 */
export function hitTest<T extends Placed>(
  items: readonly T[],
  page: number,
  x: number,
  y: number,
  slack = HIT_SLACK_PT,
): T | null {
  let best: T | null = null;
  let bestArea = Infinity;

  for (const item of items) {
    if (item.page !== page || item.hidden) continue;
    const [left, top, right, bottom] = item.rect;
    const width = right - left;
    const height = bottom - top;
    // A rectangle with no area is one the file did not state or stated
    // unusably, and `annots.rs` reports it as zeroes. Treating that as a hit
    // would put an invisible target in the page's top-left corner.
    if (width <= 0 || height <= 0) continue;
    if (
      x < left - slack ||
      x > right + slack ||
      y < top - slack ||
      y > bottom + slack
    ) {
      continue;
    }
    const area = width * height;
    if (area <= bestArea) {
      best = item;
      bestArea = area;
    }
  }
  return best;
}

/** The items on one page, in document order. */
export function onPage<T extends Placed>(items: readonly T[], page: number): T[] {
  return items.filter((item) => item.page === page);
}

/**
 * The same comments with their rectangles turned into the view's space.
 *
 * Recomputed rather than memoised: it is called on a pointer press and once per
 * frame for the one comment a popup is open on, over a list already bounded at
 * 5,000 by the backend, and a memo would have to be invalidated by *two* things
 * that move independently --- the view rotation and a page whose real size has
 * just arrived to replace an estimate.
 */
export function turnedFor<T extends Placed>(
  items: readonly T[],
  turns: number,
  width: number,
  height: number,
): T[] {
  if (((turns % 4) + 4) % 4 === 0) return [...items];
  return items.map((item) => {
    const quad = viewRect(item.rect, turns, width, height);
    return {
      ...item,
      rect: [quad.left, quad.top, quad.right, quad.bottom] as [number, number, number, number],
    };
  });
}

/**
 * A comment's rectangle under the view's own rotation.
 *
 * `width`/`height` are the page's displayed size *before* the view turn, which
 * is what `turnQuad` expects and what the scroller holds.
 */
export function viewRect(
  rect: readonly [number, number, number, number],
  turns: number,
  width: number,
  height: number,
): Quad {
  const [left, top, right, bottom] = rect;
  return turnQuad({ left, top, right, bottom }, turns, width, height);
}

/**
 * What the panel says when the scan cut something, or `null` when it did not.
 *
 * Every bound `annots.rs` applies is named here, because a list shown as
 * complete when it is not is the failure that whole module is arranged to avoid.
 * `unknown_kinds` is included and `unreadable` is not folded into it: one is
 * "there are marks tpdf does not understand", the other is "there are entries
 * nothing could read", and a reader chasing a missing comment needs to know
 * which.
 */
export function noticeFor(limits: CommentLimits): string | null {
  const parts: string[] = [];
  if (limits.over_budget) parts.push("more comments than tpdf will show");
  if (limits.crowded_pages > 0) {
    parts.push(
      limits.crowded_pages === 1
        ? "a page with more comments than tpdf will show"
        : `${limits.crowded_pages} pages with more comments than tpdf will show`,
    );
  }
  if (limits.bodies_clipped > 0) {
    parts.push(
      limits.bodies_clipped === 1
        ? "a comment too long to show whole"
        : `${limits.bodies_clipped} comments too long to show whole`,
    );
  }
  if (limits.unknown_kinds > 0) parts.push("marks of a kind tpdf does not read");
  if (limits.unreadable > 0) parts.push("entries that could not be read");
  if (limits.cycles > 0) parts.push("replies that pointed in a circle");
  // The one entry that means "the scan could not look" rather than "it looked
  // and cut something". Both belong here: an empty list shown as complete is
  // the failure this whole notice exists to prevent, and "no comments" is
  // exactly what a scan that saw no pages produces.
  if (limits.pages_missed > 0) {
    parts.push(
      limits.pages_missed === 1
        ? "a page that could not be read at all"
        : `${limits.pages_missed} pages that could not be read at all`,
    );
  }
  if (parts.length === 0) return null;
  return `This document has ${joinList(parts)} — what is shown is incomplete.`;
}

/** Joins a list the way a sentence does, with an "and" before the last part. */
function joinList(parts: readonly string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/**
 * The one line a row shows for a comment, and whether a person wrote it.
 *
 * Three candidates in {@link rowLine}'s order, shared with the marks panel. The
 * body wins; failing that, the words the annotation covers, which is what makes
 * a reviewer's bare highlight recognisable instead of a row reading "Highlight,
 * no comment" nine times; failing both, the kind is named.
 *
 * `covered` is supplied by the caller rather than derived here, because deriving
 * it needs the page's extracted text and this file is pure. `App.svelte` fetches
 * it a page at a time for the comments {@link pagesNeedingWords} names, so a
 * document nobody opens the panel on costs nothing.
 */
export function summaryOf(
  comment: Comment,
  covered = "",
): { text: string; own: boolean } {
  return rowLine(comment.body, covered, `${labelFor(comment.kind)}, no comment`);
}

/**
 * The kinds whose `/QuadPoints` say which words they are about.
 *
 * The frontend half of `Kind::covers_text` in `annots.rs`, and the two must
 * agree: that one decides whether quads are read out of the file at all, this
 * one decides whether to go looking for the words they cover. They are written
 * twice because they are in two languages, so the cost of disagreeing is a
 * comment whose words are fetched and never arrive --- which is why the
 * `comments` corpus check reads a highlight's line rather than trusting either.
 */
export function coversText(kind: CommentKind): boolean {
  return (
    kind === "highlight" ||
    kind === "underline" ||
    kind === "squiggly" ||
    kind === "strikeout"
  );
}

/**
 * Whether a comment would be listed by the words it covers, if they were known.
 *
 * All three conditions, and each rules out a different waste. A body means the
 * row already says something a person wrote, and {@link rowLine} would discard
 * the words anyway. A kind that marks no text has no words under it. And an
 * empty `quads` is a highlight whose producer wrote no usable `/QuadPoints`,
 * where the answer is knowably empty without reading the page.
 */
export function needsWords(comment: Comment): boolean {
  return (
    comment.body.trim() === "" &&
    coversText(comment.kind) &&
    comment.quads.length >= 4
  );
}

/**
 * The words every comment on one page covers, keyed by id.
 *
 * The whole of the lookup, so that `App.svelte` and `viewercheck.ts` run the
 * same one: a harness that re-implemented it would be checking its own copy,
 * which is the failure recorded as *"A writer and its own reader agree about a
 * document that is wrong"*. What is **not** here is the scheduling --- one page
 * at a time, and only while somebody is looking at the tab --- because that is a
 * policy about the reader's attention rather than a fact about comments.
 *
 * `textOf` rather than a `PageText` so the caller decides how the page is
 * fetched, and answers `null` for a page whose text will not come. Empty then,
 * and the rows keep saying what they said: an empty string here is *also* the
 * honest answer for a highlight over a picture, and the two are the same
 * outcome for a reader --- there are no words either way.
 */
export async function wordsForPage(
  items: readonly Comment[],
  page: number,
  textOf: (page: number) => Promise<PageText | null>,
): Promise<Map<number, string>> {
  const out = new Map<number, string>();
  const wanted = wantingWordsOn(items, page);
  if (wanted.length === 0) return out;
  const text = await textOf(page);
  if (!text) return out;
  for (const comment of wanted) out.set(comment.id, coveredText(text, comment.quads));
  return out;
}

/**
 * The comments on one page whose covered words are worth asking for.
 *
 * Exported because the caller has to record what it asked about, and recording
 * a *page* is what went wrong: see {@link pagesNeedingWords}. It is the same
 * selection {@link wordsForPage} makes, shared rather than written twice, so a
 * walk cannot mark one set of comments answered while the extraction answers
 * another.
 */
export function wantingWordsOn(
  items: readonly Comment[],
  page: number,
): Comment[] {
  return items.filter((comment) => comment.page === page && needsWords(comment));
}

/**
 * The pages holding comments that want their covered words, lowest first.
 *
 * A page at a time is the unit because extracting text is a per-page request to
 * the backend, and one page answers every comment on it. Ordered so the reader
 * sees the front of the document fill in first, which is where they are looking
 * --- the alternative is answers arriving in whatever order the annotations were
 * written in the file, which is nobody's order.
 *
 * **`answered` is a set of comment ids and must not be a set of pages**, which
 * is what the caller kept until 2026-08-26 and is a defect rather than a
 * preference. `comment.page` here is a *slot*, and a deletion renumbers every
 * slot after it --- so a walk that had answered slot 5 and then lost a page
 * would refuse to ask for the page that moved into slot 5, and the comments on
 * it would say *no comment* for the rest of the session. Measured before it was
 * fixed: two bare highlights, one answered, one page deleted, and the walk
 * returned nothing with a comment still wanting words. A comment id is what the
 * annotation itself carries, and it does not move.
 *
 * Required rather than defaulted, so the one caller has to say what it is
 * tracking. A default of the empty set would have kept the old call site
 * compiling and wrong.
 */
export function pagesNeedingWords(
  items: readonly Comment[],
  answered: ReadonlySet<number>,
): number[] {
  const pages = new Set<number>();
  for (const comment of items) {
    if (!answered.has(comment.id) && needsWords(comment)) pages.add(comment.page);
  }
  return [...pages].sort((a, b) => a - b);
}

/** The line under a row's body: who wrote it, and when. */
export function bylineOf(comment: Comment): string {
  const who = comment.author.trim() || "Unknown";
  return comment.date ? `${who} · ${comment.date}` : who;
}
