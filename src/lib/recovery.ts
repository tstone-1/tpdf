/**
 * What to offer a reader whose save was refused, or who is about to lose work.
 *
 * ## Why this is a module rather than three `if`s in `App.svelte`
 *
 * Nothing renders `App.svelte`. The window harness builds its own actions and
 * the unit tests import modules, so a decision written inside the component is
 * a decision no check can reach --- which is how a whole feature shipped inert
 * in 26.8.4 and why `check_viewer_wiring.py` exists. The rules here are the part
 * that can be wrong; the component's job is to display what they return.
 *
 * ## The rule these encode
 *
 * `save.rs`'s comment states it: *the fallback the message names has to keep
 * working, or the refusal strands the reader*. A refusal that says "save them
 * under another name" has to be accompanied by a way to do exactly that, and a
 * Reload that silently spends a journal is the same failure pointed the other
 * way --- the reader loses the work the refusal was protecting.
 *
 * ## Why the decision is made from a flag and not from the message
 *
 * `SaveFailure` carries `changed` and `reopen` as fields. Deciding from the
 * prose would make the backend unable to reword a message without breaking the
 * window, and `docs/TRAPS.md` has an entry about that message being reworded
 * within the week of it being written.
 */

/** One thing the window can offer to do about a refusal. */
export type Offer = "reload" | "saveCopy";

/** The half of `SaveFailure` these rules read. */
export interface SaveFailureShape {
  message: string;
  /**
   * The document is closed, whatever became of the file.
   *
   * `| undefined` explicitly, not merely optional: `exactOptionalPropertyTypes`
   * is on, and the caller reads these off an error object where a missing field
   * is genuinely `undefined` rather than absent. The rules below treat
   * `undefined` and `false` alike, which is asserted, because a backend that
   * stopped sending the field must not start offering reloads.
   */
  reopen?: boolean | undefined;
  /** The file changed on disk since the reader opened it. */
  changed?: boolean | undefined;
}

/** What the reader is told, and what they can press. */
export interface Prompt {
  message: string;
  /** In the order they should appear. The first is the safer one. */
  offers: Offer[];
}

/**
 * What to offer after a save was refused.
 *
 * Three cases, and the flags separate them completely:
 *
 *  - **The file changed and the document survived.** Both offers, copy first:
 *    the reader's edits are still in the journal, and writing them somewhere is
 *    the move that loses nothing. Reload is second because it is the one that
 *    spends them.
 *  - **The file changed and the document is closed** (`reopen`). Nothing, and
 *    this said Reload until it was read against what the window does: on a
 *    `reopen` failure `App.svelte` opens the file again by itself, so a Reload
 *    button reloads what is already on screen and Save a copy copies a
 *    freshly-opened, unedited document. Two buttons that look like help and do
 *    nothing are worse than none, because a reader presses them.
 *  - **Anything else.** Nothing. "A document must keep at least one page" is
 *    fixed by putting a page back, and a Reload button beside it would offer to
 *    discard the reader's work in exchange for nothing at all.
 */
export function afterFailedSave(failure: SaveFailureShape): Prompt {
  if (!failure.changed || failure.reopen) {
    return { message: failure.message, offers: [] };
  }
  return { message: failure.message, offers: ["saveCopy", "reload"] };
}

/**
 * What to say before reloading, or `null` when it can simply happen.
 *
 * Reload reopens the file, which closes the document and spends the journal. On
 * an unedited document that costs nothing and a confirmation would be noise. On
 * an edited one it is the reader's work, and until 2026-08-19 it went without a
 * word --- the command was written before there was anything to lose, and
 * nothing revisited it when there was.
 *
 * Save a copy leads, for the same reason it leads above.
 */
export function beforeReload(dirty: boolean): Prompt | null {
  if (!dirty) return null;
  return {
    message:
      "Reloading opens the file again and discards the edits you have not saved. " +
      "Save a copy first to keep them.",
    offers: ["saveCopy", "reload"],
  };
}

/**
 * What to say after a copy was written from a source that had changed.
 *
 * `null` for the ordinary case, because a copy that worked says so by the file
 * appearing where the reader put it --- `App.svelte` explains why success is
 * otherwise silent.
 *
 * Not an error, and that is the point of it being separate: the file is written
 * and is the best tpdf can produce. What the reader cannot be left to discover
 * is that it was built from a document that is no longer the one on screen.
 */
export function afterCopy(copied: { changed?: boolean }): string | null {
  if (!copied.changed) return null;
  return (
    // "The file", not "the copy": three commands reach this --- Save a copy,
    // Extract pages and Merge documents --- and the second of them was not
    // calling it at all until 2026-08-24, while `lib.rs`'s own comment on
    // `extract_pages` said the reader "is told the same way". A sentence naming
    // one caller is how that stays wrong when it is fixed.
    "The file was written, but the original changed on disk while you had it open, " +
    "so your edits were applied to the newer version. Check it before relying on it."
  );
}

/**
 * What to say after a split, which is always something.
 *
 * {@link afterMerge}'s exception for {@link afterCopy}'s reason, arriving from
 * the other direction. A copy and an extract go to the name the reader typed,
 * so the file appearing is the acknowledgement. A split goes to names the
 * reader never typed and never saw: `save::split_paths` derives them from the
 * chosen one, and the chosen one is not among them. A reader told nothing would
 * look for the file they named and not find it.
 *
 * So the first and last names are the report. Not all of them --- a split can
 * make sixty files and this is one line --- and not a count alone, which says
 * how many without saying where.
 */
export function afterSplit(split: { changed?: boolean; paths: string[] }): string {
  const count = split.paths.length;
  const first = basenameOf(split.paths[0] ?? "");
  const last = basenameOf(split.paths[count - 1] ?? "");
  const said =
    count === 0
      ? "Nothing was written."
      : `Wrote ${count} file${count === 1 ? "" : "s"}, ${first} to ${last}.`;
  if (!split.changed) return said;
  return (
    `${said} The original changed on disk while you had it open, so your edits ` +
    "were applied to the newer version. Check them before relying on them."
  );
}

/** The last path segment, for either platform's separator. */
function basenameOf(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at === -1 ? path : path.slice(at + 1);
}

/**
 * What to say after a merge, which is always something.
 *
 * The one write path whose success is **not** silent, and the exception is
 * deliberate rather than an inconsistency with {@link afterCopy}. A copy and an
 * extract produce what the reader asked for by name --- a file here, these three
 * pages --- so the file appearing is the acknowledgement. A merge produces
 * however many pages the documents it was given happened to hold, and a reader
 * who picked four files cannot tell from the destination that all four went in
 * without opening it and counting.
 *
 * So the counts are the report, and they are the reason `save::Merged` carries
 * them at all: a field describing what the caller could not otherwise check,
 * with no caller reading it, is the shape this repository has a trap about.
 *
 * A source that changed under the reader is appended rather than replacing the
 * counts. Both facts are true and the reader needs both --- the merge happened,
 * and it was built from a document that is no longer the one on screen.
 */
export function afterMerge(merged: {
  changed?: boolean;
  pages: number;
  files: number;
}): string {
  const others =
    merged.files === 1 ? "1 other document" : `${merged.files} other documents`;
  const pages = merged.pages === 1 ? "1 page" : `${merged.pages} pages`;
  const said = `Merged this document with ${others} — ${pages} in all.`;
  if (!merged.changed) return said;
  return (
    `${said} The original changed on disk while you had it open, ` +
    "so your edits were applied to the newer version."
  );
}
