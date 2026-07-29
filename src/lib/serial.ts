/**
 * Running async bodies one at a time.
 *
 * Extracted from `App.svelte`, where the same four lines guarded document opens
 * and could not be tested: the invariant lives in a component with no test
 * harness, and the end-to-end check that exercises it is a race --- measured, it
 * reports a removed chain about two runs in three, which is a smoke test and not
 * a gate. Here the property is deterministic. Same move as `backoff.ts`, for the
 * same reason.
 *
 * The failure it prevents was real and is worth stating, because the shape
 * recurs wherever a component owns singletons and an async body mutates them.
 * Two document opens overlapped --- a double-click on a second file while the
 * first was still opening --- and each body read the *other's* freshly-set
 * document id as the one to release, so each closed the file the other was
 * about to mount. The second `new Viewer` then overwrote the first without
 * destroying it, leaving two viewers with live `wheel`, `keydown` and
 * `pointerdown` listeners on one element, and two sidebars, because `Sidebar`
 * appends rather than replacing.
 *
 * A chain rather than a generation counter or a busy flag, and the difference
 * matters: a flag makes the second call a no-op, which loses the document the
 * reader just asked for, and a counter lets both bodies run and only sorts out
 * who may write at the end --- by which time both have already released a file
 * and mounted a viewer. The invariant is "one at a time", and a chain is that
 * sentence. The cost is that a second open waits for the first.
 */

/**
 * A queue of one.
 *
 * Bodies handed to {@link run} start in call order and never overlap.
 */
export class Serial {
  /**
   * The tail of the queue.
   *
   * Pending or fulfilled, **never rejected** --- {@link run} flattens both
   * outcomes into it, and everything else here depends on that.
   */
  private tail: Promise<unknown> = Promise.resolve();

  /**
   * Queues `body`, and resolves with its result.
   *
   * One document failing to open must not stop the next, and the single-arm
   * `then` here is only safe because of the line beneath it: `tail` is `next`
   * with **both** outcomes flattened to `undefined`, so it can never be a
   * rejected promise and the arm that would handle one is unreachable. Written
   * with two arms first, and a mutation reducing it to one survived the whole
   * suite --- which is what unreachable defence looks like from the outside.
   * Deleting the arm and pinning the assumption in the line that enforces it is
   * the honest version; a test covers the pair by rejecting a body and
   * requiring the next one to run.
   *
   * The returned promise, unlike the tail, does reject when `body` does. A
   * caller that wants to know whether its own open failed has to be told, while
   * the queue itself must stay usable regardless --- and that difference is the
   * whole reason the two are separate promises.
   */
  run<T>(body: () => Promise<T>): Promise<T> {
    const next = this.tail.then(body);
    this.tail = next.then(
      () => undefined,
      () => undefined,
    );
    return next;
  }
}
