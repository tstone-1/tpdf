/**
 * Releasing documents a previous webview left behind.
 *
 * ## What this is for
 *
 * `close_document` has exactly one caller in the application: `App.svelte`, when
 * a *successful* subsequent open replaces the current document. So the backend's
 * document table is owned entirely by webview state, and a webview reload sets
 * that state back to nothing while the backend keeps everything. Every document
 * opened before the reload is then unreachable, with its worker pool alive, for
 * the life of the process.
 *
 * A freshly loaded page holds no document id by construction --- ids come back
 * from `open_document` and it has not called it yet --- so every id the backend
 * holds at that moment is one nobody can name. That is the whole argument, and it
 * depends on there being one window; `release_documents` in `lib.rs` carries what
 * happens to it if tpdf ever grows a second.
 *
 * ## Why this is a module rather than two lines in the component
 *
 * Nothing renders `App.svelte`. A decision written inside the component is a
 * decision no check can reach --- which is how a whole feature shipped inert in
 * 26.8.4 and why `check_viewer_wiring.py` exists. What is decidable here is
 * small and is exactly the part that can be wrong: whether a failure is allowed
 * to reach the reader, and whether the ordinary case says anything.
 *
 * **The call site itself is still unchecked**, and that is worth saying rather
 * than implying otherwise. One line in the component invokes this; no test
 * imports that file, and the wiring gate covers `Viewer`'s callbacks rather than
 * component startup.
 */

/** The backend command's shape: how many documents it released. */
export type Release = () => Promise<number>;

/** Where a note goes, so a test can read what was said. */
export type Note = (line: string) => void;

/**
 * Asks the backend to release anything a previous webview left holding.
 *
 * Never rejects. A reader who has just started the application is not waiting on
 * this and cannot act on it failing --- the documents stay held, which is exactly
 * the state that already existed. Raising it would replace an invisible leak with
 * a visible error about a thing nobody asked for.
 *
 * Returns what was released, which is what makes it testable at all: `0` is the
 * ordinary case and means the application started, and anything else means a
 * webview reloaded. `-1` says the call itself failed, which is neither.
 */
export async function releaseOrphans(release: Release, note: Note): Promise<number> {
  try {
    const held = await release();
    // Silent on zero. This runs on every start, and a line saying nothing
    // happened on every start is how a reader learns to skip the line that
    // matters --- the same reason the memory doctor stays quiet under its hook.
    if (held > 0) {
      note(`released ${held} document(s) a previous page left behind`);
    }
    return held;
  } catch (e) {
    // Reported, not raised. A backend that cannot answer this is a backend that
    // cannot answer anything, and the next thing the reader does will say so
    // far more usefully than a startup error about housekeeping.
    note(`could not release documents from a previous page: ${String(e)}`);
    return -1;
  }
}
