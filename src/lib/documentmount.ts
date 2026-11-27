/**
 * The decisions the viewer and the sidebar of a mounting document make, for
 * `buildViewer` and `buildSidebar` in `App.svelte`.
 *
 * Those two functions are options literals: each callback joins an event to
 * the component's variables. Where a callback *decides* something on the way
 * --- which page and object a comment edit addresses, what choosing a tab sets
 * off, what the strip takes from a status --- the decision is here, where a
 * test can call it, as `documentopen.ts` holds `openDocument`'s. The component
 * keeps the joins.
 *
 * Nothing here waits, and nothing here reads the component's variables: what a
 * decision acts on is handed in.
 */

import type { Comment } from "./comments";
import type { PageId, PageMap } from "./pages";
import type { Tab } from "./sidebar";
import type { ViewerStatus } from "./viewer";

/**
 * What an edit of somebody else's comment addresses: the object the file gave
 * the annotation, and the identity of the page it is on.
 *
 * A popup can submit after a reorder or deletion. Use the owner captured by
 * commentsIn and confirm that page still exists before recording an edit.
 */
export function commentTarget(
  comment: Pick<Comment, "object" | "pageId">,
  pages: PageMap | undefined,
): { object: [number, number]; page: PageId } | null {
  const { object, pageId } = comment;
  if (!object || pageId === undefined || pages?.slotOfId(pageId) === undefined) return null;
  return { object, page: pageId };
}

/** What choosing a sidebar tab may set off, as the component does each. */
export interface TabWork {
  /** Find the words each bare highlight covers. */
  commentWords: () => void;
  /** Find the words under each pending redaction. */
  redactionWords: () => void;
  /** Take away the ring the hidden-text list drew over a passage. */
  clearRegion: () => void;
}

/**
 * Does what choosing `tab` sets off.
 *
 * The two word walks are a text extraction per page carrying a comment or a
 * region, so each is paid for by a reader who opens its tab and by nobody else:
 * a document opened, read and closed on the outline costs neither. The
 * redaction walk is also driven when a region is drawn, because a region the
 * reader has just dragged wants its words while they look at the panel; that
 * is the component's, not this.
 *
 * The ring over a passage belongs to the list that drew it, so every other tab
 * takes it away.
 */
export function chooseTab(tab: Tab, work: TabWork): void {
  if (tab === "comments") work.commentWords();
  if (tab === "redactions") work.redactionWords();
  if (tab !== "hidden") work.clearRegion();
}

/** The parts of the sidebar a viewer status is passed to. */
export interface StatusFollower {
  setViewerBusy(busy: boolean): void;
  setTurns(turns: number): void;
  setInvert(invert: boolean): void;
}

/**
 * Passes the viewer's status to the page strip.
 *
 * Through the status rather than from the commands, so the strip follows
 * however a rotation or an inversion was reached: the palette, the keyboard,
 * or anything later that does it without going through a command. The strip
 * stops asking for thumbnails, and withdraws what it asked for, whenever the
 * viewer has work outstanding (`thumbnails.ts`), which is what keeps them out
 * of the page's way.
 */
export function followStatus(
  strip: StatusFollower | null | undefined,
  next: Pick<ViewerStatus, "pending" | "turns" | "invert">,
): void {
  strip?.setViewerBusy(next.pending > 0);
  strip?.setTurns(next.turns);
  strip?.setInvert(next.invert);
}
