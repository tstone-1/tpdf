/**
 * Marking an object dead to its own async continuations.
 *
 * Every class here that owns a document's worth of state has a `destroy()`, and
 * every one of them also has `.then` callbacks in flight when it is called ---
 * a tile that is still rendering, a text extraction still queued, an
 * `ImageBitmap` being copied. Those callbacks run afterwards. They always did;
 * what changed is that the object they close over is now rubble.
 *
 * This was the dominant defect class in the frontend for a while, and it kept
 * arriving in new spellings: a destroyed viewer restarting its own frame loop,
 * a select-all retry surviving the document it was selecting in, a page strip
 * pumping requests for a file nobody is looking at. Each was fixed where it was
 * found. What did not exist was the *pattern*, so the next async feature had to
 * remember on its own, and the ones written after the fixes did not.
 *
 * ## Why a class rather than a boolean
 *
 * A boolean is what the first fixes used and it is not wrong --- it is just not
 * enough for the half of these that carry a resource. `ImageBitmap` holds
 * GPU-backed memory that is freed by `close()` and by nothing else, so a
 * continuation that lands after teardown and merely *returns early* leaks it as
 * thoroughly as one that stores it in a map that has already been cleared.
 * Three such paths existed when this was written --- both of the scroller's tile
 * arrivals and both of the strip's --- and a plain `if (dead) return` in front
 * of each would have looked exactly like a fix.
 *
 * So {@link Lifetime.claim} takes the disposal as a **required** argument. You
 * cannot write the guard without saying what happens to the value the guard
 * throws away, which is the difference between this and a flag: the flag lets
 * you forget, and the forgetting is invisible. Continuations that carry nothing
 * to release read {@link Lifetime.ended} directly, and that is the whole API.
 */

/** One object's liveness, and the guard its continuations are written against. */
export class Lifetime {
  /** Set once, in `destroy()`, and never cleared --- there is no coming back. */
  private over = false;

  /** Whether the owner has been torn down. */
  get ended(): boolean {
    return this.over;
  }

  /**
   * Marks the owner dead.
   *
   * Call it **first** in `destroy()`, before releasing anything. A teardown that
   * ends the lifetime last leaves a window in which a continuation sees a live
   * object with half its state already freed, which is worse than either state
   * on its own.
   */
  end(): void {
    this.over = true;
  }

  /**
   * Wraps a continuation that receives a value the owner would take ownership of.
   *
   * While the lifetime is live, `live` gets the value. Once it has ended,
   * `dispose` gets it instead --- and `dispose` is not optional, because the
   * values worth guarding here are the ones that have to be released by hand.
   *
   * Both callbacks are the caller's: this decides *whether*, never *what*.
   */
  claim<T>(live: (value: T) => void, dispose: (value: T) => void): (value: T) => void {
    return (value: T) => {
      if (this.over) {
        dispose(value);
        return;
      }
      live(value);
    };
  }
}
