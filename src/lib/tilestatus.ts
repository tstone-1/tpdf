/**
 * Failure kinds shared between the tile client and the things that consume it.
 *
 * One class, in a module of its own, and the separation is load-bearing rather
 * than tidy. `tiles.ts` is mocked **wholesale** by four test files
 * (`vi.mock("./tiles", () => ...)`), and a factory supplies only what its author
 * remembered to name --- so a class that lived there would be `undefined` inside
 * every one of them, and `reason instanceof undefined` throws a `TypeError`.
 *
 * That is not a loud failure. It happens inside the scroller's *failure*
 * handler, so the tile is never settled, the frame loop never quiesces, and six
 * tests in two unrelated files fail with "the frame loop never settled" ---
 * which reads as a bug in the frame loop. It cost exactly that once. A mock of
 * `./tiles` cannot reach this module, so the identity `instanceof` needs is the
 * real one in production and in every test, including any written later that
 * nobody thought to update.
 */

/**
 * The document's file was truncated on disk while it was open.
 *
 * A class rather than a flag on the message, so callers distinguish it with
 * `instanceof` instead of by matching English --- the message is a sentence
 * written for a reader and is expected to be reworded.
 *
 * Nothing recovers from this. Every worker for that document maps the bytes that
 * are gone, so the backend refuses without spawning anything and will keep
 * refusing; the only way back to that file is to open it again, which builds a
 * new mapping. Retrying is therefore not merely useless but actively wrong, and
 * `Scroller` stops asking rather than backing off.
 */
export class DocumentGone extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DocumentGone";
  }
}
