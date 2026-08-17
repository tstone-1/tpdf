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

import { PageMap, unedited, type PageView } from "./pages";

// Re-exported because this is the module a reader of the edit state comes to
// first, and the declaration lives in `pages.ts` so that the modules which only
// need the shape --- the scroller, the thumbnails --- do not have to import this
// one, which cannot be loaded outside a webview.
export type { PageView };

/** Mirrors `edits::EditState`. */
export interface EditState {
  pages: PageView[];
  can_undo: boolean;
  can_redo: boolean;
  /** Whether anything differs from the file on disk. */
  dirty: boolean;
}

/** The state of a document nobody has edited and nobody has opened. */
export const NOTHING_OPEN: EditState = {
  pages: [],
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
  private readonly doc: number;
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

  /** Steps the journal back one command. */
  async undo(): Promise<EditState> {
    return this.adopt(await invoke<EditState>("edit_undo", { doc: this.doc }));
  }

  /** Steps the journal forward one command. */
  async redo(): Promise<EditState> {
    return this.adopt(await invoke<EditState>("edit_redo", { doc: this.doc }));
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

  /** Records an answer, and the translation it implies, and returns it. */
  private adopt(state: EditState): EditState {
    this.current = state;
    this.pageMap = new PageMap(state.pages);
    return state;
  }
}
