/**
 * Saving a copy with a password, or without the one the document has: whether
 * a typed password can be one, the name the copy is offered, and the sentence
 * the window ends on.
 *
 * `App.svelte` keeps the dialogs and the `invoke`; what is decided is here, so
 * it has tests. The encryption is `src-tauri/src/protect.rs`, which applies
 * the same rules to the password and is the one that counts.
 */

import { basename } from "./paths";
import { afterCopy } from "./recovery";

/** `MAX_BYTES` in `protect.rs`: revision 6 reads no more of a password. */
export const MAX_BYTES = 127;

/**
 * Shown under the fields while the password has a character outside ASCII.
 *
 * Measured 2026-10-03: PDFKit, which Preview uses, does not open a document
 * whose password has such a character, whether tpdf or `qpdf` wrote it.
 */
export const NOT_ASCII =
  "Preview on macOS does not open a document whose password has accented or " +
  "non-Latin characters. Other readers do.";

/** What a password must be, said before the reader has typed one. */
export const ADVICE =
  "The copy cannot be opened without this password, and tpdf cannot recover it.";

/**
 * Why `first`, typed again as `second`, cannot be the password, or `null`
 * when it can.
 */
export function judge(first: string, second: string): string | null {
  if (first === "") return "Type a password.";
  // By code point, as `char::is_control` counts: C0, DEL and C1.
  if (/[\u0000-\u001f\u007f-\u009f]/.test(first)) {
    return "The password holds a tab, a line break or another character that cannot be typed back.";
  }
  const bytes = new TextEncoder().encode(first).length;
  if (bytes > MAX_BYTES) {
    return `The password is too long: a PDF password is at most ${MAX_BYTES} bytes, and this one is ${bytes}.`;
  }
  if (first !== second) return "The two passwords are not the same.";
  return null;
}

/** Whether `password` has a character {@link NOT_ASCII} is about. */
export function beyondAscii(password: string): boolean {
  return /[^\u0000-\u007f]/.test(password);
}

/** `report.pdf` gives `report protected.pdf`, or `report unprotected.pdf`. */
export function suggestedName(path: string, set: boolean): string {
  const stem = basename(path).replace(/\.pdf$/i, "");
  return `${stem} ${set ? "protected" : "unprotected"}.pdf`;
}

/**
 * What to say once the copy is written.
 *
 * Never silent, where an ordinary copy is: the window still shows the
 * document as it was, and whether the file on disk now needs a password is
 * the one thing the reader cannot see by looking.
 */
export function afterProtect(
  copied: { changed?: boolean },
  path: string,
  set: boolean,
): string {
  const said = set
    ? `Saved ${basename(path)}. It needs the new password to open.`
    : `Saved ${basename(path)}. It opens without a password.`;
  const changed = afterCopy(copied);
  return changed ? `${said} ${changed}` : said;
}
