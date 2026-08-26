/**
 * The document's page edits, as the backend reports them.
 *
 * The model lives in Rust --- `src-tauri/src/docmodel.rs` holds the journal, the
 * undo replay and the snapshots, and `src-tauri/src/edits.rs` gives it a home per
 * open document. **This file holds no rules.** It is a cache of the last answer
 * plus the calls that ask for a new one, which is deliberate: a frontend that
 * computed the next state from the current one would be a second implementation
 * of the journal, and the cases where two implementations disagree are exactly
 * the ones undo exists for.
 *
 * So every method here returns the whole state and replaces the cache with it.
 * The only way for this to be wrong is to be stale, and the only way to be stale
 * is to have skipped a reply.
 */

import { invoke } from "@tauri-apps/api/core";

import {
  PageMap,
  unedited,
  type MarkKind,
  type MarkView,
  type PageId,
  type PageView,
  type StampName,
} from "./pages";
import { colorFor, type MarkColor } from "./markcolors";

// Re-exported because this is the module a reader of the edit state comes to
// first, and the declaration lives in `pages.ts` so that the modules which only
// need the shape --- the scroller, the thumbnails --- do not have to import this
// one, which cannot be loaded outside a webview.
export type { MarkView, PageView };

/**
 * What `save_copy` and `extract_pages` report about the file they wrote.
 *
 * A copy is written even from a source that changed under the reader, because
 * refusing it closes the only door the in-place refusal points at --- see
 * `save.rs`'s `OnChange`. So the copy exists, and this is how it says which
 * document it was built from. `recovery.ts` turns it into a sentence.
 */
export interface Copied {
  /** The source changed on disk since it was opened, and this was written anyway. */
  changed: boolean;
}

/**
 * What `merge_documents` reports about the file it wrote.
 *
 * `changed` is {@link Copied}'s field and means the same thing. The two counts
 * are what a merge has that a copy does not: the reader asked for "these files,
 * combined" rather than for a named result, so the number of pages is the only
 * evidence that each document was really read. `recovery.ts`'s `afterMerge`
 * turns them into the sentence, and is the reason they are not carried for
 * nobody.
 */
export interface Merged {
  /** The open document changed on disk since it was opened, and this was written anyway. */
  changed: boolean;
  /** How many pages the written file holds, this document's included. */
  pages: number;
  /** How many documents were merged in, not counting this one. */
  files: number;
}

/**
 * What `split_document` reports about the files it wrote.
 *
 * `changed` is {@link Copied}'s field and means the same thing. `paths` is what
 * a split has that the other two do not: the reader chose **one** name and got
 * several, under a numbering rule that lives in `save::split_paths` and that
 * they never saw. Naming the files is the only way they learn where the
 * document went, which is why this is a list rather than a count.
 */
export interface Split {
  /** The source changed on disk since it was opened, and these were written anyway. */
  changed: boolean;
  /** Every file written, in order. */
  paths: string[];
}

/** Mirrors `edits::EditState`. */
export interface EditState {
  pages: PageView[];
  marks: MarkView[];
  can_undo: boolean;
  can_redo: boolean;
  /** Whether anything differs from the file on disk. */
  dirty: boolean;
}

/** The state of a document nobody has edited and nobody has opened. */
export const NOTHING_OPEN: EditState = {
  pages: [],
  marks: [],
  can_undo: false,
  can_redo: false,
  dirty: false,
};


/**
 * The edit state of one open document, and the commands that change it.
 *
 * Constructed per document, and holds the handle so that a caller cannot pass
 * the wrong one --- which matters because the render service reuses document
 * numbers, so a stale handle names a real document rather than nothing.
 */
export class Edits {
  /**
   * The open document's handle.
   *
   * Readable because the two crop questions --- what is on this page, and how
   * big is it under this box --- are asked of the *file* rather than of the
   * model, so they go to the renderer with this handle rather than through any
   * method here. See `crop.ts`.
   */
  readonly doc: number;
  private current: EditState;
  /** The translation the last answer implies, rebuilt with it. */
  private pageMap: PageMap;

  /**
   * `pages` seeds the cache with a document nobody has edited.
   *
   * The one thing in this file that is not the model's answer, and it is the
   * assumption `refresh` immediately confirms: a freshly opened document *is*
   * unedited, and it is the same assumption the viewer's constructor makes about
   * the order it lays out before the first reply. Without it every reader of the
   * map --- the links, the comments, the page strip --- would have to have an
   * answer for "no pages yet", and would translate a real document into nothing
   * for the length of one round trip.
   *
   * The day a session carries edits, this stops being true for one frame and
   * `refresh` corrects it. That is why it is a seed rather than a rule.
   */
  constructor(doc: number, pages = 0) {
    this.doc = doc;
    this.current =
      pages > 0
        ? { ...NOTHING_OPEN, pages: [...unedited(pages).pages] }
        : NOTHING_OPEN;
    this.pageMap = new PageMap(this.current.pages);
  }

  /** The last answer from the model. */
  get state(): EditState {
    return this.current;
  }

  /** Whether the document differs from the file on disk. */
  get dirty(): boolean {
    return this.current.dirty;
  }

  /**
   * The last answer as a translation between slots and pages of the file.
   *
   * Rebuilt when a reply lands rather than kept in step, which is the same
   * posture as the cache it is built from: there is one answer, and this is a
   * reading of it.
   *
   * **Built once per reply and not once per call**, which is not a micro-
   * optimisation: the page strip asks `sourceOf` for every row it renders while
   * a reader drags the scrollbar, and a getter that constructed a `PageMap`
   * each time would build a 775-entry index per thumbnail on the long corpus.
   */
  get map(): PageMap {
    return this.pageMap;
  }

  /** Quarter-turns an edit has applied to the page in slot `page`. */
  turnsOf(page: number): number {
    return this.current.pages[page]?.turns ?? 0;
  }

  /**
   * Reads the model's state without changing it.
   *
   * Called once after an open rather than assumed: a freshly opened document is
   * unedited today, and will not be once a session can carry edits.
   */
  async refresh(): Promise<EditState> {
    return this.adopt(await invoke<EditState>("edit_state", { doc: this.doc }));
  }

  /**
   * Turns the page in slot `page` by `turns` quarter-turns clockwise.
   *
   * Takes a slot because that is what a reader points at, and sends the *id*,
   * because that is what the model accepts --- the translation is one lookup in
   * the state this object already holds. A slot whose page is not in the last
   * answer is not sent at all: the model would refuse it, and refusing here says
   * the same thing without a round trip.
   */
  async rotate(page: number, turns: number): Promise<EditState> {
    const id = this.current.pages[page]?.id;
    if (id === undefined) return this.current;
    return this.adopt(
      await invoke<EditState>("page_rotate", { doc: this.doc, page: id, turns }),
    );
  }

  /**
   * Sets or clears the visible box of the page in slot `page`.
   *
   * Takes a slot and sends the id, as {@link rotate} does and for the same
   * reason. `to` is `[llx, lly, urx, ury]` in the page's own space, y upwards,
   * or `null` to put the file's own box back.
   *
   * **Absolute, never relative.** A second crop replaces the first rather than
   * composing with it, so undoing one is `null` and not an inverse to compute.
   */
  async crop(
    page: number,
    to: readonly [number, number, number, number] | null,
  ): Promise<EditState> {
    const id = this.current.pages[page]?.id;
    if (id === undefined) return this.current;
    return this.adopt(
      await invoke<EditState>("page_crop", { doc: this.doc, page: id, to }),
    );
  }

  /**
   * Removes the page in slot `page` from the working document.
   *
   * Takes a slot and sends the id, as {@link rotate} does and for the same
   * reason. What it does *not* do is decide whether the page may go: a document
   * must keep at least one page, and that rule is the model's --- checking it
   * here as well would be a second copy of it, able to disagree with the first
   * about a document whose pages a command in flight has already changed.
   */
  async delete(page: number): Promise<EditState> {
    const id = this.current.pages[page]?.id;
    if (id === undefined) return this.current;
    return this.adopt(
      await invoke<EditState>("page_delete", { doc: this.doc, page: id }),
    );
  }

  /**
   * Moves the page in slot `from` so that it ends up in slot `to`.
   *
   * **The one piece of arithmetic in this file, and it is an inversion rather
   * than a rule.** The model takes a *neighbour*: put this page behind that one.
   * A reader points at a destination, so something has to turn one into the
   * other, and it has to be this side --- the model refuses an index for the
   * reason `edits.rs` gives, and inverting it in Rust would need the order the
   * frontend already holds.
   *
   * The inversion is over the order *without the moved page in it*, because that
   * is the order the model inserts into: it removes the page first and then
   * reads the anchor's position. Reading the anchor out of the order that still
   * contains it is correct for every move towards the front, and for a move
   * towards the back it lands one slot short --- except by exactly one slot,
   * where it names the moved page itself and the model refuses it outright. Two
   * different symptoms for one arithmetic error, which is why both are checked.
   *
   * A move to slot 0 has no anchor, which is what `null` means on the wire.
   */
  async move(from: number, to: number): Promise<EditState> {
    const page = this.current.pages[from]?.id;
    if (page === undefined) return this.current;
    const rest = this.current.pages.filter((_, slot) => slot !== from);
    const landing = Math.max(0, Math.min(to, rest.length));
    if (landing === from) return this.current;
    const after = landing === 0 ? null : (rest[landing - 1]?.id ?? null);
    return this.adopt(
      await invoke<EditState>("page_move", { doc: this.doc, page, after }),
    );
  }

  /**
   * Marks `quads` on the page `page` names, with a mark of `kind`.
   *
   * The quads are display-space rectangles --- see {@link MarkView} --- and they
   * must come from the page's *own* text rather than from the view's, which is
   * what `TextCache.peekUnturned` is for. Sending view-space quads would store a
   * mark that moves when the reader rotates the window.
   *
   * **Takes an id, where {@link rotate} takes a slot, and the difference is the
   * one this method got wrong.** Every other command here is issued the moment a
   * reader asks for it, so translating a slot at the boundary is safe; a mark is
   * geometry lifted off the page at the *end of a gesture*, and the id is what
   * pins it to the page that gesture happened on. `Viewer.onDrawn` said so and
   * handed one over --- into a parameter that indexed `pages` by it, so a shape
   * drawn on slot 0 of an unedited document was written to slot 1, and one drawn
   * on the last page was silently dropped because there is no slot past the end.
   * Nothing went red: an id and a slot are both `number`, and the two callers
   * that did hold slots were right. {@link PageId} is a distinct type for that
   * reason --- the mistake is now `error TS2345` rather than a mark on the wrong
   * page.
   *
   * What it does not do is decide whether the mark is acceptable: a mark
   * covering nothing is refused by the model, and predicting that here would be
   * a second copy of the rule.
   *
   * **One method for all three kinds**, matching the one command behind it. The
   * three differ in a subtype, a colour and how the appearance is drawn, all of
   * which the writer decides; nothing on this side of the boundary changes with
   * the kind except which constant is read.
   *
   * `chosen` is the reader's colour, or `null` for the kind's own --- see
   * `markcolors.ts`. Passed in rather than held here because {@link Edits} is
   * built per document and the choice outlives one: a reader who picks green,
   * closes the file and opens another has not gone back to yellow.
   */
  async mark(
    kind: MarkKind,
    page: PageId,
    quads: number[],
    strokes: number[][] = [],
    note = "",
    chosen: MarkColor | null = null,
    stamp: StampName | null = null,
  ): Promise<EditState> {
    // A page the model has never mentioned, or one that has gone since the
    // gesture started. Nothing is sent, which is what the slot lookup used to
    // give for free and has to be said now that no lookup happens.
    if (!this.current.pages.some((view) => view.id === page)) return this.current;
    return this.adopt(
      await invoke<EditState>("annot_mark", {
        doc: this.doc,
        mark: {
          kind,
          page,
          quads,
          strokes,
          // The biconditional the model enforces, and it is defaulted rather
          // than required for the reason `NewMark::stamp` gives: every caller
          // that predates stamps keeps working, and the model refuses a name on
          // the wrong kind rather than drawing one.
          stamp,
          color: colorFor(kind, chosen),
          author: "",
          note,
        },
      }),
    );
  }

  /**
   * Replaces what one mark says, by the id a state reply gave it.
   *
   * The whole note, not an edit to it: the reader types in a box and what is
   * sent is what the box now holds. Whether that is a change is not decided
   * here --- the model journals whatever it is told, so the caller is the one
   * that must not send an unchanged note, and `markpopup.ts` is where that
   * comparison lives, next to the box it compares.
   */
  async renote(mark: number, note: string): Promise<EditState> {
    return this.adopt(
      await invoke<EditState>("annot_note", { doc: this.doc, mark, note }),
    );
  }

  /**
   * Moves one mark by an offset, by the id a state reply gave it.
   *
   * An offset rather than a new rectangle, which is what makes it a *move*: one
   * number pair applied to everything the mark owns cannot resize a box or
   * reshape a drawing, where a geometry computed on this side and sent whole
   * could do both through a defect in one line. See `Doc::displace`.
   *
   * In the page's **display** space, which the viewer converts to --- not the
   * view's, and not client pixels. And clamped there, because keeping a mark on
   * its page needs the page's size in points and the model does not hold one.
   *
   * The model journals whatever it is told, so a caller must not send a zero
   * offset: that is a press without a drag, which is how a reader opens a note,
   * and it would put an undo step in front of them for nothing. `viewer.ts`
   * drops it, next to the gesture that produced it, exactly as `markpopup.ts`
   * drops an unchanged note.
   *
   * **`displace` rather than `move`**, which is the reader's word and is already
   * taken here by {@link Edits.move} --- that one moves a *page* in the order.
   * The name is `Doc::displace`'s, so the two sides of the boundary say the same
   * thing, and the window still calls it *Move* where a reader can see it.
   */
  async displace(mark: number, dx: number, dy: number): Promise<EditState> {
    return this.adopt(
      await invoke<EditState>("annot_move", { doc: this.doc, mark, dx, dy }),
    );
  }

  /**
   * Replaces what one mark is drawn in, by the id a state reply gave it.
   *
   * {@link renote}'s shape, and its caveat too: the model journals whatever it
   * is told, so a caller must not send the colour a mark already is --- that
   * would be an undo step for nothing. The comparison is `markpopup.ts`'s, next
   * to the swatch the reader pressed.
   */
  async recolor(mark: number, color: MarkColor): Promise<EditState> {
    return this.adopt(
      await invoke<EditState>("annot_recolor", { doc: this.doc, mark, color }),
    );
  }

  /**
   * Rubs strokes out of one drawing, by the id a state reply gave it.
   *
   * `remove` is positions into the strokes the last state reply carried, which
   * is why this takes no points: the backend owns what a drawing is made of and
   * a command named "erase" must not be able to rewrite it. One call per sweep,
   * so one call per undo.
   *
   * A sweep that takes every stroke removes the drawing --- decided in
   * `edits.rs`, not here, and the reply simply comes back without the mark.
   */
  async erase(mark: number, remove: number[]): Promise<EditState> {
    return this.adopt(
      await invoke<EditState>("annot_erase", { doc: this.doc, mark, remove }),
    );
  }

  /** Takes one mark off the page it is on, by the id a state reply gave it. */
  async unmark(mark: number): Promise<EditState> {
    return this.adopt(
      await invoke<EditState>("annot_remove", { doc: this.doc, mark }),
    );
  }

  /** Steps the journal back one command. */
  async undo(): Promise<EditState> {
    return this.adopt(await invoke<EditState>("edit_undo", { doc: this.doc }));
  }

  /** Steps the journal forward one command. */
  async redo(): Promise<EditState> {
    return this.adopt(await invoke<EditState>("edit_redo", { doc: this.doc }));
  }

  /**
   * Writes the working document over the file it was opened from.
   *
   * **This object is spent when it returns.** The backend closes the document
   * as part of the save --- every object identity in the file has changed, so
   * the baseline the journal replays against is gone --- and the caller opens
   * the path again. Nothing is adopted because there is no state to adopt: the
   * next state comes from the reopened document, not from this one.
   *
   * A rejection carries `{ message, reopen }` rather than a string, and the
   * second field is the one to act on: `reopen: false` means nothing was
   * touched and the reader still has their document, `reopen: true` means it is
   * closed whatever became of the file. See `lib.rs`'s `SaveFailure`.
   */
  async save(source: string): Promise<void> {
    await invoke<void>("save_document", { doc: this.doc, source });
  }

  /**
   * Writes the working document to `path`.
   *
   * Does not clear {@link dirty}. The journal is still the journal --- a copy
   * having been written says nothing about the document being edited, and
   * reporting it as clean would be a claim that the *open* file matches what is
   * on disk, which it does not.
   */
  async saveCopy(source: string, path: string): Promise<Copied> {
    return await invoke<Copied>("save_copy", { doc: this.doc, source, path });
  }

  /**
   * Writes the pages at `slots` to `path`, as a second file.
   *
   * Changes nothing about this document --- not the order, not the journal, not
   * {@link dirty} --- which is why it returns no state to adopt. Extract is a
   * read of the working document, and the thing it produces is somewhere else.
   *
   * The slots are positions in the current order, and the backend refuses a
   * selection that is empty, out of range, repeated or descending rather than
   * normalising it. Normalising here and there would be two readers of one
   * rule; refusing means the one place that sorts is `parsePageRange`.
   */
  async extractPages(
    source: string,
    path: string,
    slots: number[],
  ): Promise<Copied> {
    return await invoke<Copied>("extract_pages", {
      doc: this.doc,
      source,
      path,
      slots,
    });
  }

  /**
   * Writes this document's pages to several files, one per group.
   *
   * {@link extractPages} repeated, and it changes nothing about this document
   * for that method's reason: nothing is journalled and {@link dirty} is
   * untouched, so there is no state to adopt.
   *
   * `path` is the name the reader chose, and it is a **stem** rather than a
   * destination --- `save::split_paths` derives `name-1.pdf`, `name-2.pdf` from
   * it and the chosen name itself is never written. The refusal that follows
   * from that is the one only a split has: no derived name may already exist,
   * because the reader was never asked about those.
   *
   * The groups are positions in the current order, each ascending and
   * deduplicated, and the backend refuses anything else per group rather than
   * normalising it --- `parseSplitPoints` is the one place that orders them.
   */
  async splitDocument(
    source: string,
    path: string,
    groups: number[][],
  ): Promise<Split> {
    return await invoke<Split>("split_document", {
      doc: this.doc,
      source,
      path,
      groups,
    });
  }

  /**
   * Writes this document followed by every page of `others`, to `path`.
   *
   * Changes nothing about this document, exactly as {@link extractPages} does
   * not --- no command is journalled and {@link dirty} is untouched, so there is
   * no state to adopt. A merge reads; what it produces is somewhere else.
   *
   * This document goes in **as it stands**, with its edits and marks applied.
   * The others go in as they are on disk: they are not open, so there is no
   * working document for them to have. `save::write_merged` holds that
   * asymmetry.
   */
  async mergeDocuments(
    source: string,
    path: string,
    others: string[],
  ): Promise<Merged> {
    return await invoke<Merged>("merge_documents", {
      doc: this.doc,
      source,
      path,
      others,
    });
  }

  /** Records an answer, and the translation it implies, and returns it. */
  private adopt(state: EditState): EditState {
    this.current = state;
    this.pageMap = new PageMap(state.pages);
    return state;
  }
}
