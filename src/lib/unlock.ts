/**
 * Opening a document that may be locked, as a decision that can be tested.
 *
 * The loop itself is four lines and lived in `App.svelte`, where nothing could
 * reach it: no test imports that component, and the `wiring` gate --- which
 * exists because a tool shipped inert through exactly that gap --- checks which
 * callbacks are supplied, not what the functions beside them do. So the logic
 * moved here and `App.svelte` keeps only the two things it is the right place
 * for: the `invoke`, and the dialog.
 *
 * ## What the decision is
 *
 * Retry for as long as the reader supplies a password, and only for the refusal
 * that is answerable. Three things about it are choices rather than mechanics,
 * and each has a test:
 *
 * - **A loop, not one retry.** Mistyping twice is ordinary, and a second failure
 *   is not a different situation from the first --- the worker retries the load
 *   in place, so an attempt costs a reply rather than a process.
 * - **Dismissing rethrows the refusal**, rather than returning something the
 *   caller has to recognise. The caller's `catch` already puts the reader back
 *   where they were with the document they had; a second sentinel would need
 *   that whole arm written twice. What they see is the sentence saying the
 *   document is locked, which is true and is what they just declined to answer.
 * - **No dialog means no prompt**, which is not an edge case: every spike entry
 *   point opens documents before the shell is mounted, and one of them prompting
 *   would hang a headless run at a dialog nobody can see.
 */

import { isOpenRefusal, type OpenRefusal } from "./ipc";

/** Opens `path`, with a password when one has been supplied. */
export type Open<T> = (password: string | undefined) => Promise<T>;

/**
 * Asks the reader for a password, resolving with `null` if they decline.
 *
 * `problem` is the backend's own wording. Which sentence it is says whether a
 * password has already been tried, and that is a fact only the worker that
 * tried it has --- see `worker_child::unlock`.
 */
export type Ask = ((problem: string) => Promise<string | null>) | null;

/**
 * Opens a document, asking for a password for as long as one is offered.
 *
 * @throws the refusal, when it is not one a reader can answer, when there is
 * nothing to ask through, or when they decline to answer it.
 */
export async function openWithPassword<T>(open: Open<T>, ask: Ask): Promise<T> {
  let password: string | undefined;
  for (;;) {
    try {
      return await open(password);
    } catch (e) {
      // On the flag, never on the wording. A frontend that decided by looking
      // for the word "password" in a backend string is one rewording away from
      // prompting for the wrong refusal --- and this increment reworded it twice.
      if (!isOpenRefusal(e) || !e.locked || !ask) throw e;
      const typed = await ask((e as OpenRefusal).reason);
      if (typed === null) throw e;
      password = typed;
    }
  }
}
