/**
 * What to offer a reader whose command was refused, or who is about to lose work.
 *
 * A save, a redaction and a print all reach these rules, and the rules do not
 * know which: what they read is the refusal's own flags. See {@link afterRefusal}
 * for why that is the naming as well as the mechanism.
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

/**
 * One thing the window can offer to do about a refusal or a warning.
 *
 * **Every variant needs its own arm where these are rendered.** `App.svelte`
 * matched `saveCopy` and let an `{:else}` draw everything else as *Reload from
 * disk*, which was correct while there were two and would have drawn a button
 * that discards the reader's work under a prompt about destroying their file.
 * That is this repository's own note about a catch-all arm being what makes
 * forgetting the quiet outcome, arriving in the one place where the wrong
 * button is worse than no button.
 */
export type Offer = "reload" | "saveCopy" | "redact";

/**
 * The half of a refusal these rules read.
 *
 * Named for the two flags rather than for `lib.rs`'s `SaveFailure`, which is
 * only one of the things that arrives in this shape: a print refusal is a
 * `save::Refusal` and carries `changed` alone, and a save's is a `SaveFailure`
 * and carries both. What the rules below need is the same either way, and a
 * name that says *save* is a name the next producer makes wrong.
 */
export interface RefusalShape {
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
 * Reads a caught value as a {@link RefusalShape}, whatever was thrown.
 *
 * **One reading of an `unknown`, rather than one per catch.** Every command
 * rejects with whatever its error serialised to: an object for a refusal the
 * backend worded, a string for a panic or a transport failure, and nothing at
 * all is guaranteed. Three catches in `App.svelte` wrote the same cast and the
 * same `?? String(e)` by hand and a fourth --- printing --- wrote neither, so
 * the day printing's refusal grew fields the reader was shown `[object
 * Object]`. A shape with three hand-written readings has three chances to be
 * one field behind the backend.
 *
 * The message falls back to the stringified value because a throw that is not a
 * refusal still has to say something, and something is what a reader standing
 * in front of a command that did nothing needs.
 *
 * **A flag is taken only where it is genuinely a boolean.** A field that
 * arrives as a string or a number is `undefined` here rather than truthy, which
 * matters in one direction only and that direction is expensive: a truthy
 * `reopen` withholds both offers, so a backend sending `"false"` would leave a
 * reader with their edits, a changed file underneath and nothing to press.
 * `undefined` rather than `false`, because {@link RefusalShape} draws that
 * distinction deliberately and it is not this function's to collapse.
 */
export function refusalOf(thrown: unknown): RefusalShape {
  const fields = (thrown ?? {}) as {
    message?: unknown;
    reopen?: unknown;
    changed?: unknown;
  };
  return {
    message: typeof fields.message === "string" ? fields.message : String(thrown),
    reopen: flagOf(fields.reopen),
    changed: flagOf(fields.changed),
  };
}

/** A field is an answer only where it is a boolean; anything else is silence. */
function flagOf(field: unknown): boolean | undefined {
  return typeof field === "boolean" ? field : undefined;
}

/**
 * What to offer after an operation was refused.
 *
 * **Named for the refusal rather than for the operation**, which is deliberate
 * and was a rename: this was `afterFailedSave` while a save was the only thing
 * that could produce one, and it decided nothing about saving even then. It
 * reads two flags. A print refused because the file changed underneath wants
 * exactly the answer a save refused for the same reason wants --- the reader's
 * unsaved edits are the print job's input, so writing them somewhere loses
 * nothing and reloading spends them --- and `docs/TRAPS.md` records what a name
 * that describes the population it happens to cover costs the day the
 * population grows.
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
 *
 *    A print cannot reach this case, and that is a property of printing rather
 *    than an assumption made here: nothing on that path closes the document, so
 *    its refusals carry no `reopen` at all. The arm is read by saves and
 *    redactions, and it stays a flag rather than a caller's promise.
 *  - **Anything else.** Nothing. "A document must keep at least one page" is
 *    fixed by putting a page back, and a Reload button beside it would offer to
 *    discard the reader's work in exchange for nothing at all.
 */
export function afterRefusal(failure: RefusalShape): Prompt {
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
 * What to say before removing marked regions from the reader's own file.
 *
 * **Unconditional, where {@link beforeReload} asks whether there is anything to
 * lose.** There always is: the file the reader opened is about to stop
 * containing the words they marked, and no copy of it exists. Reload spends a
 * journal, which is work; this spends the document, which is the thing the work
 * was about.
 *
 * The measure of whether this is over-cautious is the sibling command: *Redact
 * and save as* writes somewhere else and asks nothing, because a reader who
 * dislikes the result still has the original open. Take the original away and
 * the last chance to stop moves here.
 *
 * Save a copy leads, for the reason it leads in both rules above, and here it is
 * more than an ordering: it is the only way to keep an unredacted copy, and the
 * working document is still unredacted at the moment this is on screen.
 *
 * `name` is the file's own name rather than its path. A reader with two windows
 * open needs to know which one this is; the directory it sits in does not help
 * them decide and would push the sentence off the line.
 */
export function beforeRedactingInPlace(name: string): Prompt {
  return {
    message:
      `Redacting removes the marked text from ${name} itself. There is no undo ` +
      "and no original left afterwards. Save a copy first to keep one.",
    offers: ["saveCopy", "redact"],
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
 * What to say after a redaction, which is **always** something.
 *
 * {@link afterCopy}'s opposite, and the asymmetry is the whole of
 * `docs/PLAN.md` §6. A copy that worked is silent, because the file appearing
 * where the reader put it is the acknowledgement. A redaction is never silent:
 * the reader has just destroyed content on the strength of a claim, and the
 * claim is the thing they need to see. §6 step 4 puts it as *reports verified,
 * or not verified with specifics --- never a bare success*, and the second half
 * is what this exists for.
 *
 * The counts are both said because they answer different questions. *Regions* is
 * what the reader marked, so it is what they can check against the panel;
 * *removals* is what actually came out of the content stream, and the two differ
 * whenever a region covers several lines or two regions share one.
 *
 * A source that changed under the reader is appended rather than replacing
 * anything, because it is a fact about a *different* thing --- which document
 * this was built from --- and dropping either sentence loses something a reader
 * has to act on.
 */
export function afterRedaction(applied: {
  regions: number;
  shows: number;
  verified: boolean;
  why: string[];
  changed?: boolean;
}): string {
  const removed = `${count(applied.regions, "region")}, ${count(applied.shows, "removal")}`;
  const verdict = applied.verified
    ? `Redacted ${removed}. tpdf read the file back and none of the removed words are in it.`
    : // Named as a failure to *prove* rather than as a failure to remove,
      // because those are different and only one of them is known. A blind spot
      // is a scan that could not look, and telling a reader their words are
      // still there when nothing said so would be its own confident lie.
      `Redacted ${removed}, but tpdf could not prove the file is clean: ` +
      `${applied.why.join("; ")}. Treat it as unredacted until you have checked it.`;
  if (!applied.changed) return verdict;
  return (
    `${verdict} The original also changed on disk while you had it open, so this was ` +
    "built from the newer version."
  );
}

/** `1 region` or `4 regions`, so a sentence does not say "1 regions". */
function count(many: number, noun: string): string {
  return many === 1 ? `1 ${noun}` : `${many} ${noun}s`;
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
