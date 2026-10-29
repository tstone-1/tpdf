/**
 * Saving a copy with a password, or without the one the document has: whether
 * a typed password can be one, the name the copy is offered, and the sentence
 * the window ends on.
 *
 * `App.svelte` keeps the dialogs and the `invoke`; what is decided is here, so
 * it has tests. The encryption is `src-tauri/src/protect.rs`, which applies
 * the same rules to the password and is the one that counts.
 */

import { catalog } from "./i18n";
import { basename } from "./paths";
import { afterCopy } from "./recovery";

/** `MAX_BYTES` in `protect.rs`: revision 6 reads no more of a password. */
export const MAX_BYTES = 127;

const TEXT = catalog({
  "en-US": {
    notAscii:
      "Preview on macOS does not open a document whose password has accented or " +
      "non-Latin characters. Other readers do.",
    advice: "The copy cannot be opened without this password, and tpdf cannot recover it.",
    typePassword: "Type a password.",
    untypeable: "The password holds a tab, a line break or another character that cannot be typed back.",
    tooLong: (max: number, bytes: number) =>
      `The password is too long: a PDF password is at most ${max} bytes, and this one is ${bytes}.`,
    notSame: "The two passwords are not the same.",
    protectedName: (stem: string) => `${stem} protected.pdf`,
    unprotectedName: (stem: string) => `${stem} unprotected.pdf`,
    savedProtected: (name: string) => `Saved ${name}. It needs the new password to open.`,
    savedUnprotected: (name: string) => `Saved ${name}. It opens without a password.`,
  },
  "de-DE": {
    notAscii:
      "Vorschau unter macOS öffnet kein Dokument, dessen Passwort Akzente oder " +
      "nicht lateinische Zeichen enthält. Andere Programme schon.",
    advice: "Die Kopie lässt sich ohne dieses Passwort nicht öffnen, und tpdf kann es nicht wiederherstellen.",
    typePassword: "Gib ein Passwort ein.",
    untypeable:
      "Das Passwort enthält einen Tabulator, einen Zeilenumbruch oder ein anderes Zeichen, das sich nicht wieder eintippen lässt.",
    tooLong: (max: number, bytes: number) =>
      `Das Passwort ist zu lang: Ein PDF-Passwort hat höchstens ${max} Byte, dieses hat ${bytes}.`,
    notSame: "Die beiden Passwörter stimmen nicht überein.",
    protectedName: (stem: string) => `${stem} geschützt.pdf`,
    unprotectedName: (stem: string) => `${stem} ungeschützt.pdf`,
    savedProtected: (name: string) => `${name} gespeichert. Zum Öffnen ist das neue Passwort nötig.`,
    savedUnprotected: (name: string) => `${name} gespeichert. Die Datei öffnet sich ohne Passwort.`,
  },
  "de-DE-formal": {
    typePassword: "Geben Sie ein Passwort ein.",
  },
});

/**
 * Shown under the fields while the password has a character outside ASCII.
 *
 * Measured 2026-10-03: PDFKit, which Preview uses, does not open a document
 * whose password has such a character, whether tpdf or `qpdf` wrote it.
 */
export const NOT_ASCII = TEXT().notAscii;

/** What a password must be, said before the reader has typed one. */
export const ADVICE = TEXT().advice;

/**
 * Why `first`, typed again as `second`, cannot be the password, or `null`
 * when it can.
 */
export function judge(first: string, second: string): string | null {
  if (first === "") return TEXT().typePassword;
  // By code point, as `char::is_control` counts: C0, DEL and C1.
  if (/[\u0000-\u001f\u007f-\u009f]/.test(first)) {
    return TEXT().untypeable;
  }
  const bytes = new TextEncoder().encode(first).length;
  if (bytes > MAX_BYTES) {
    return TEXT().tooLong(MAX_BYTES, bytes);
  }
  if (first !== second) return TEXT().notSame;
  return null;
}

/** Whether `password` has a character {@link NOT_ASCII} is about. */
export function beyondAscii(password: string): boolean {
  return /[^\u0000-\u007f]/.test(password);
}

/** `report.pdf` gives `report protected.pdf`, or `report unprotected.pdf`. */
export function suggestedName(path: string, set: boolean): string {
  const stem = basename(path).replace(/\.pdf$/i, "");
  return set ? TEXT().protectedName(stem) : TEXT().unprotectedName(stem);
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
  const said = set ? TEXT().savedProtected(basename(path)) : TEXT().savedUnprotected(basename(path));
  const changed = afterCopy(copied);
  return changed ? `${said} ${changed}` : said;
}
