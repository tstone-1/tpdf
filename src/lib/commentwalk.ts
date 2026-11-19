/**
 * The walk that fills in the words each bare highlight covers, a page at a
 * time, for `fillCommentWords` in `App.svelte`.
 *
 * **Why it exists.** A reviewer's highlight with nothing typed on it is a
 * rectangle and no text, so the panel listed nine of them as nine rows all
 * reading "Highlight, no comment". The words are in the page, under the
 * rectangle, and `wordsForPage` reads them out.
 *
 * **Why a page at a time, awaited.** Each page is a `page_text` extraction
 * answered by the pool that draws tiles. Firing them all at once puts a
 * document's worth of extractions in front of the page the reader is looking
 * at; awaiting each means at most one is ever queued.
 *
 * **Why what has been asked is counted by comment id and not by page.** A page
 * number here is a slot, and deleting a page renumbers every slot after it: a
 * walk that had answered slot 5 then refused the page that moved into slot 5,
 * and its comments read *no comment* for the rest of the session. The trap is
 * "An 'already asked' set keyed by a slot is renumbered by the next deletion".
 *
 * **Why the comments are slotted again every round.** For the same reason: a
 * reader who deletes a page mid-walk renumbers every slot after it, so a list
 * taken before the loop would read slot 4's text for the comment that used to
 * be there.
 *
 * Its own module, as `fillRedactionRegions` in `redactlist.ts` is, because
 * `App.svelte` is reached by no unit test; the component keeps the wiring.
 * {@link CommentWords} is what the walk keeps between calls, one per mounted
 * document (`LiveDocument.commentWords`), so that a change of side swaps the
 * set, the words and the flag together.
 */

import { pagesNeedingWords, wantingWordsOn, wordsForPage, type Comment } from "./comments";
import type { PageText } from "./text";

/** What the walk is given. */
export interface CommentWalk {
  /**
   * The comments with their pages as the document is now, or null once the
   * document the walk began in has gone. Asked again every round.
   */
  comments: () => readonly Comment[] | null;
  /** The comments whose words have been asked for, by id. Added to here. */
  asked: Set<number>;
  /** The text of the page in `slot`, or null when it cannot be read. */
  text: (slot: number) => Promise<PageText | null>;
  /**
   * The words found on one page, by comment id. Answers false when the
   * document the walk began in has gone, which ends it.
   */
  found: (words: Map<number, string>) => boolean;
}

/**
 * Asks for the words of every comment that wants them, a page at a time, until
 * none is left or the document has gone.
 *
 * The comments a round answers are recorded as asked before the wait, so a
 * page that cannot be read is not asked for again on the next round.
 */
export async function fillCommentWords(walk: CommentWalk): Promise<void> {
  for (;;) {
    const items = walk.comments();
    if (!items) return;
    const page = pagesNeedingWords(items, walk.asked)[0];
    if (page === undefined) return;
    for (const comment of wantingWordsOn(items, page)) walk.asked.add(comment.id);
    const words = await wordsForPage(items, page, walk.text);
    if (!walk.found(words)) return;
  }
}

/**
 * What one document's comment walk keeps: the comments asked about, the words
 * found, and whether a walk is running.
 *
 * **The words are held here as well as in the panel, and that is not belt and
 * braces.** `applyPageOrder` re-supplies the whole comment list whenever a page
 * is deleted or moved, and the panel drops its words with every `setComments`
 * --- ids are per document, so keeping them there across a list it did not
 * compute would be the one way a sentence lands on the wrong row. Without this
 * the rows fell back to "Highlight, no comment" on the first page deletion and
 * stayed there for the rest of the session, because {@link asked} had already
 * recorded every comment as asked. Found by a mutation that survived: clearing
 * the panel's map on each answer changed nothing observable.
 *
 * **`asked` holds comment ids, and until 2026-08-26 it held page slots**; the
 * walk's own comment above says why. Cleared with the document, since an id
 * means nothing across two files.
 */
export class CommentWords {
  /** The comments whose words have been asked for, by id, so none is asked twice. */
  readonly asked = new Set<number>();
  /** The words each comment covers, for the ones that have been looked up. */
  readonly words = new Map<number, string>();
  /** Whether {@link fill} is walking, so a second call stands down. */
  filling = false;

  /**
   * Runs the walk over this document's set, unless one is already running,
   * and keeps what each page answers. `found` is told first and answers
   * whether the document is still there; words for one that has gone are not
   * kept. Can be called again: the set stops a comment being asked twice, so a
   * reader flipping to the comments tab repeatedly costs one pass.
   */
  async fill(walk: Omit<CommentWalk, "asked">): Promise<void> {
    if (this.filling) return;
    this.filling = true;
    try {
      await fillCommentWords({
        comments: walk.comments,
        asked: this.asked,
        text: walk.text,
        found: (words) => {
          if (!walk.found(words)) return false;
          for (const [comment, said] of words) this.words.set(comment, said);
          return true;
        },
      });
    } finally {
      this.filling = false;
    }
  }
}
