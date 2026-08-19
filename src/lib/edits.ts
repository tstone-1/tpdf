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
  type PageView,
} from "./pages";

// Re-exported because this is the module a reader of the edit state comes to
// first, and the declaration lives in `pages.ts` so that the modules which only
// need the shape --- the scroller, the thumbnails --- do not have to import this
// one, which cannot be loaded outside a webview.
export type { MarkView, PageView };

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
 * The colour each kind is written in, as red, green and blue in 0..=1.
 *
 * One colour per kind, because there is one command per kind. A *palette* is a
 * different question --- where the swatches live, whether a reader picks before
 * or after marking --- and answering it with a constant here would be answering
 * it invisibly.
 *
 * The two lines are red rather than the wash's yellow, and not by convention
 * alone: a yellow rule 1.3 pt thick on white paper is close to invisible, where
 * the same yellow spread over a whole line of text is exactly right. The wash
 * is drawn multiplied at 40% and the lines opaque, so what reads well as one
 * cannot be assumed to read well as the other.
 */
const MARK_COLORS: Record<MarkKind, [number, number, number]> = {
  highlight: [1, 0.9, 0.2],
  underline: [0.85, 0.15, 0.15],
  strikeout: [0.85, 0.15, 0.15],
  // The wash's yellow rather than the lines' red, and for the opposite reason
  // to both: this colour is not ink over words at all, it is the fill of an
  // icon sitting on the paper beside them. Yellow is what every reader draws a
  // comment bubble in, so a file opened in Acrobat looks like the file that was
  // saved --- `/C` is what Acrobat colours its own icon with.
  note: [1, 0.9, 0.2],
  // The lines' red, not the bubble's yellow, because a box is a line: its ink
  // is a stroke and it is drawn opaque and on top. A yellow box on white paper
  // is nearly invisible, which is the same reason the underline and the
  // strikeout above are not the wash's colour either.
  square: [0.85, 0.15, 0.15],
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
   * Marks `quads` on the page in slot `page`, with a mark of `kind`.
   *
   * The quads are display-space rectangles --- see {@link MarkView} --- and they
   * must come from the page's *own* text rather than from the view's, which is
   * what `TextCache.peekUnturned` is for. Sending view-space quads would store a
   * mark that moves when the reader rotates the window.
   *
   * Takes a slot and sends the id, as {@link rotate} does. What it does not do
   * is decide whether the mark is acceptable: a mark covering nothing is refused
   * by the model, and predicting that here would be a second copy of the rule.
   *
   * **One method for all three kinds**, matching the one command behind it. The
   * three differ in a subtype, a colour and how the appearance is drawn, all of
   * which the writer decides; nothing on this side of the boundary changes with
   * the kind except which constant is read.
   */
  async mark(kind: MarkKind, page: number, quads: number[], note = ""): Promise<EditState> {
    const id = this.current.pages[page]?.id;
    if (id === undefined) return this.current;
    return this.adopt(
      await invoke<EditState>("annot_mark", {
        doc: this.doc,
        mark: { kind, page: id, quads, color: MARK_COLORS[kind], author: "", note },
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
  async saveCopy(source: string, path: string): Promise<void> {
    await invoke<void>("save_copy", { doc: this.doc, source, path });
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
  ): Promise<void> {
    await invoke<void>("extract_pages", {
      doc: this.doc,
      source,
      path,
      slots,
    });
  }

  /** Records an answer, and the translation it implies, and returns it. */
  private adopt(state: EditState): EditState {
    this.current = state;
    this.pageMap = new PageMap(state.pages);
    return state;
  }
}
