/**
 * The links scans of the other files a document's pages were inserted from.
 *
 * `document_links` answers for one document, and an imported page's links are
 * in another one: the file open under the handle the page is drawn from. So the
 * opened file's scan is not enough once a page can come from somewhere else,
 * and each other file is scanned once, through its own handle, when the first
 * of its pages arrives.
 *
 * **Here rather than in `App.svelte`**, which is the layer no unit test
 * reaches: this is a set of handles asked about and a map of answers, and
 * `AGENTS.md` records three shipped defects that were state like this living
 * in the component. `App.svelte` keeps the requests and the wiring.
 *
 * What an answer is *used for* --- which slot a link lands on, what its
 * destination becomes --- is `importedLinksIn` in `pages.ts`, not this.
 */

import type { Link } from "./links";
import type { PageMap } from "./pages";

export class ImportedLinks {
  /** Answers, by render handle. */
  private readonly answers = new Map<number, readonly Link[]>();
  /**
   * Handles already asked about, answered or not.
   *
   * Separate from {@link answers} because a scan in flight and a scan that
   * failed are both "do not ask again": the frontend calls {@link wanted} after
   * every state reply, and a file whose scan fails would otherwise be scanned
   * again on every edit for the life of the document.
   */
  private readonly asked = new Set<number>();

  /**
   * The handles that pages of `pages` are drawn from and nobody has asked
   * about yet, each once. Marks them asked, so the caller must ask.
   */
  wanted(pages: PageMap): number[] {
    const fresh = pages.importedDocs().filter((doc) => !this.asked.has(doc));
    for (const doc of fresh) this.asked.add(doc);
    return fresh;
  }

  /** Records one file's scan. */
  record(doc: number, links: readonly Link[]): void {
    this.answers.set(doc, links);
  }

  /** Every answer so far, by handle, for `allLinksIn`. */
  get all(): ReadonlyMap<number, readonly Link[]> {
    return this.answers;
  }

  /** Forgets everything, for a document that is no longer the one shown. */
  clear(): void {
    this.answers.clear();
    this.asked.clear();
  }
}
