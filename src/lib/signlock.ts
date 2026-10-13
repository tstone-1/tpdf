/**
 * Whether a signing locks the document, and what is remembered about it.
 *
 * A signature can say that nothing may change after it (`sign_prepare.rs`,
 * *The lock*): readers that honour it then report any later change as one the
 * signer did not permit. The owner's decision of 2026-10-13 is that this is
 * what a reader who signs expects, so:
 *
 * - **Ticked until the reader unticks it.** A reader who has never chosen
 *   gets a locked document. Unticking is for a document somebody else still
 *   has to sign: a locked document takes no further signature.
 * - **The choice is remembered** in `localStorage` under {@link LOCK_KEY},
 *   guarded as `signtimestamp.ts` guards its own: only the exact value tpdf
 *   writes for *no* reads as unticked, so storage that throws or holds
 *   anything else is the default.
 *
 * The default is the stricter of the two on purpose. A signing that locks
 * when the reader did not want it is refused by the next signer's program
 * with a reason, and can be made again without the lock; one that does not
 * lock when the reader thought it did is found out when somebody has already
 * changed the file.
 */

/** Where the choice lives. */
export const LOCK_KEY = "tpdf.signatureLock";

type Store = Pick<Storage, "getItem" | "setItem">;

/** The remembered choice, or `true`. */
export function readLock(storage: () => Store = () => window.localStorage): boolean {
  try {
    return storage().getItem(LOCK_KEY) !== "false";
  } catch {
    return true;
  }
}

/** Keeps the choice; `false` when storage refused, which changes nothing now. */
export function writeLock(lock: boolean, storage: () => Store = () => window.localStorage): boolean {
  try {
    storage().setItem(LOCK_KEY, lock ? "true" : "false");
    return true;
  } catch {
    return false;
  }
}

/** The checkbox's words. */
export const LOCK_LABEL =
  " Lock the document after signing — the signature then says that nothing may " +
  "change after it, and a program that checks signatures reports any later change. " +
  "Untick it when somebody else still has to sign: a locked document takes no " +
  "further signature.";
