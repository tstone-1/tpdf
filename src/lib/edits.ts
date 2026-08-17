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

/**
 * One page of the working document.
 *
 * Mirrors `edits::PageView`. Field names are the Rust identifiers --- there is no
 * `rename_all` on that struct, for the reason `ipc.ts` gives at length.
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
  id: number;
  /** Which baseline page supplies the content. Equal to the slot today. */
  source: number;
  /** Quarter turns clockwise on top of the page's own `/Rotate`, 0 to 3. */
  turns: number;
}

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
  private current: EditState = NOTHING_OPEN;

  constructor(doc: number) {
    this.doc = doc;
  }

  /** The last answer from the model. */
  get state(): EditState {
    return this.current;
  }

  /** Whether the document differs from the file on disk. */
  get dirty(): boolean {
    return this.current.dirty;
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

  /** Records an answer and returns it. */
  private adopt(state: EditState): EditState {
    this.current = state;
    return state;
  }
}

/**
 * Which slots differ between two states, so a caller redraws only those.
 *
 * Returned as slots rather than ids because the viewer lays pages out by
 * position: this is the one place the two vocabularies meet on the way *back*,
 * as `Edits.rotate` is on the way out.
 *
 * A state with a different number of pages reports **every** slot in the longer
 * of the two, which is not a shortcut --- when pages appear or disappear, every
 * slot from the first change onwards holds a different page, and a comparison of
 * turns would report "nothing moved" for a document whose pages had all shifted
 * by one.
 */
export function changedSlots(before: EditState, after: EditState): number[] {
  const slots: number[] = [];
  if (before.pages.length !== after.pages.length) {
    for (let at = 0; at < Math.max(before.pages.length, after.pages.length); at++) {
      slots.push(at);
    }
    return slots;
  }
  for (let at = 0; at < after.pages.length; at++) {
    const was = before.pages[at];
    const now = after.pages[at];
    if (!was || !now) continue;
    if (was.id !== now.id || was.turns !== now.turns || was.source !== now.source) {
      slots.push(at);
    }
  }
  return slots;
}
