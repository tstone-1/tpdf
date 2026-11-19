/**
 * Where a report nobody asked for is put: the message bar, or the notice line.
 *
 * The window has two places for words. The message bar under the header holds
 * one message and the buttons that go with it, and `say()` in `App.svelte`
 * replaces all of it. The notice line in the header holds one line and nothing
 * to press. Most messages are the answer to something the reader just did, and
 * the bar is theirs. A few arrive on their own, a while after anything was
 * pressed: a document's links read once its first page is painted, its form,
 * a file shortened under the reader, the tabs a launch could not open.
 *
 * **One of those, put in the bar, replaced whatever the bar held.** A redaction
 * in place says its verdict right after the document opens again, and the
 * links of that document are read about a second later. On a document whose
 * links were cut short the verdict, "not verified" included, was on screen for
 * that second and then replaced by a sentence about links, its tone and its
 * buttons with it. The same happened to a saved copy's "Open saved copy", to
 * the result a written file is opened with, and to the message and buttons a
 * tab was returned to with.
 *
 * So a report that arrives on its own takes the bar only while the bar is
 * empty, and the notice line otherwise. It is not dropped: a document whose
 * links half work has to say so, for the reason `readLinks` gives.
 */

/** Where {@link whereUnasked} puts a report. */
export type UnaskedPlace = "message" | "notice";

/**
 * Where a report that arrived on its own goes, given the message on screen
 * (`null` for none). The bar while it is empty, the notice line otherwise.
 * An empty string is no message: the bar draws nothing for it.
 */
export function whereUnasked(onScreen: string | null): UnaskedPlace {
  return onScreen ? "notice" : "message";
}

/**
 * Whether a prompt that arrived on its own, and needs the bar's buttons, may
 * be shown now, given the message on screen (`null` for none).
 *
 * The prompt for a file that changed on disk carries *Reload* and *Save a
 * copy*, and the notice line has nothing to press, so {@link whereUnasked}
 * has no place for it. It waits instead: the disk watch looks again about a
 * second later and asks then, once the reader has dealt with what the bar
 * holds. The same answer holds back a reload nobody is asked about, because
 * a reload opens the file again and that clears the bar.
 */
export function promptUnaskedNow(onScreen: string | null): boolean {
  return !onScreen;
}
