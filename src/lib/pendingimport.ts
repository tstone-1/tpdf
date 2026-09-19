/**
 * An insert from another file that is waiting for the reader to say which pages.
 *
 * `edit.insertPages` is two questions --- which file, then which of its pages ---
 * and the second cannot be asked until the first is answered, because a page
 * range is read against a page count nobody knows before the file is open. So
 * the backend opens and checks the file first (`page_import_prepare`), holds it,
 * and answers an id, a count and a name; the palette then asks for a range, and
 * `page_import` places what the reader named. Between the two, this module is
 * the webview's whole memory of the question.
 *
 * **What this holds is an id, never the file.** The worker pool the file was
 * opened in belongs to the document's model in the backend, which releases it
 * when the document closes, when a second prepare replaces it, and when a
 * commit the model refuses ends it. What is left for this side is the one case
 * the backend cannot see: the reader walking away from the question. That is
 * {@link PendingImports.drop}, which the palette's dismissal reaches.
 *
 * Its own module rather than three variables in `App.svelte`, because that
 * component is reached by no unit test and the rules here --- a replaced
 * question releases the old file, an answer for another document releases
 * rather than inserts, a blank answer means every page --- each have a wrong
 * version that looks right.
 */

import { namePages, parsePageRange, type PageRange } from "./pageranges";

/**
 * What `page_import_prepare` answers.
 *
 * Mirrors `edits::PreparedImport`; `replyshapes.test.ts` holds the two to the
 * committed sample.
 */
export interface PreparedImport {
  /** The id `page_import` and `page_import_cancel` name the waiting file by. */
  pending: number;
  /** How many pages the file has. */
  pages: number;
  /** The file's name without its directories. */
  name: string;
}

/** A prepared import, and the document it was prepared for. */
export interface PendingImport extends PreparedImport {
  doc: number;
}

/**
 * The one import the window is waiting on, if any.
 *
 * One rather than one per tab, because the question is asked in the palette,
 * and the palette asks one thing at a time. The backend keeps one per document
 * for its own reason --- a close has to find it --- and the two agree because a
 * question for a document that is no longer the one on screen is not offered
 * (`current(doc)`) and is released rather than answered (`take`).
 */
export class PendingImports {
  #held: PendingImport | null = null;
  readonly #release: (doc: number, pending: number) => void;

  /**
   * @param release Ends the backend's wait for an import that will not be
   *   answered. Called at most once per import, and never for one that was
   *   {@link take}n: after a commit the backend has already ended it.
   */
  constructor(release: (doc: number, pending: number) => void) {
    this.#release = release;
  }

  /**
   * The import waiting for `doc`, or null.
   *
   * Asked with the document on screen, so a question prepared for another tab
   * is not offered here even though it has not been released yet.
   */
  current(doc: number | null): PendingImport | null {
    const held = this.#held;
    return held !== null && held.doc === doc ? held : null;
  }

  /**
   * Starts waiting on a freshly prepared import, releasing any other.
   *
   * The previous one is released even though a second prepare for the *same*
   * document has already released it in the backend: a cancel naming an id the
   * backend no longer waits on is a no-op there, and one for a different
   * document is the only thing that ends that document's wait.
   */
  hold(doc: number, prepared: PreparedImport): void {
    const previous = this.#held;
    this.#held = { doc, ...prepared };
    if (previous) this.#release(previous.doc, previous.pending);
  }

  /**
   * Hands over the import waiting for `doc`, to be committed, and forgets it.
   *
   * Null when nothing is waiting. **An import waiting for another document is
   * released, not handed over**: its pages would be placed in the document the
   * reader has moved to, which is not the one they chose the file for.
   */
  take(doc: number): PendingImport | null {
    const held = this.#held;
    this.#held = null;
    if (held === null) return null;
    if (held.doc !== doc) {
      this.#release(held.doc, held.pending);
      return null;
    }
    return held;
  }

  /** Stops waiting, releasing the file. Nothing happens when nothing waits. */
  drop(): void {
    const held = this.#held;
    this.#held = null;
    if (held) this.#release(held.doc, held.pending);
  }
}

/**
 * The pages of the other file a reader named, zero-based and in file order.
 *
 * `parsePageRange` against the other file's count, with one difference, and it
 * is the reason this is not a call to that function: **a blank answer is every
 * page**, which is what the command did before it asked and what most readers
 * want. Extract has no such default, because extracting everything is a copy.
 *
 * A count the reader is told is the file's, not "this document's": the range is
 * about the file they just chose, and the document on screen has a different
 * number of pages.
 */
export function chosenPages(raw: string, file: PreparedImport): PageRange {
  if (raw.trim() === "") {
    return { slots: Array.from({ length: file.pages }, (_, page) => page) };
  }
  const range = parsePageRange(raw, file.pages);
  if (range.problem?.startsWith("This document has")) {
    return { problem: range.problem.replace("This document", file.name) };
  }
  return range;
}

/** What the range question's input says before anything is typed. */
export function rangePlaceholder(file: PreparedImport): string {
  return `Pages of ${file.name} (1-${file.pages}); blank for all`;
}

/** What inserting the named pages will do, or "" while the answer is unusable. */
export function rangePreview(raw: string, file: PreparedImport): string {
  const range = chosenPages(raw, file);
  if (!range.slots) return "";
  if (range.slots.length === file.pages) {
    return file.pages === 1
      ? `Insert the page of ${file.name}`
      : `Insert all ${file.pages} pages of ${file.name}`;
  }
  return `Insert ${namePages(range.slots)} of ${file.name}`;
}
