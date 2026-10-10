/**
 * Adding long-term validation data to a document that is already signed: the
 * sequence and its words.
 *
 * The backend is `longterm/existing.rs` (what is asked about, and every
 * refusal), `sign_dss.rs` and `sign_prepare.rs` (the worker's two revisions)
 * and `commands/validation.rs` (the write and the read-back). This module is
 * the frontend's half, and it is here rather than in `App.svelte` because that
 * file is the layer no gate reaches (`AGENTS.md`): the order of the questions
 * and what the reader is told before anything is asked of anybody live here
 * with tests, and `App.svelte` supplies the four things that need the shell
 * --- the document's properties, the dialog, the save panel and the command.
 *
 * ## The order is a decision
 *
 * 1. **Unsaved edits refuse first.** The data is added to the file on disk,
 *    and the reader is looking at the file plus their edits. The backend
 *    refuses the same thing; saying it here saves a dialog and a save panel.
 * 2. **A document with no signature is told so**, from the reading of the
 *    document's signatures the properties dialog shows --- before the reader
 *    is asked to choose an authority and name a file for nothing. The backend
 *    decides again from its own reading; this is a first look.
 * 3. **The reader chooses a timestamp authority** ({@link askAuthority}).
 *    Nothing is preselected the first time: choosing one is what lets tpdf ask
 *    it. The dialog says who else is asked --- the certificate authorities the
 *    signatures' certificates name --- and that only a hash of the document
 *    goes to the timestamp authority.
 * 4. **The reader names the new file** --- `<name>-long-term.pdf` beside the
 *    original. The original is never written.
 * 5. **The window says it is waiting** while the backend asks the authorities
 *    and writes ({@link WAITING}), and stops saying so whether the answer is a
 *    sentence or a refusal.
 * 6. **The result is the backend's sentence**, from what a worker read back
 *    in the written file. A refusal is thrown on, for the caller to show as it
 *    shows every other; it says whether anything was written.
 *
 * ## No signed-save warning
 *
 * Signing shows none, and for the same reason this shows none: nothing here
 * rewrites the document. Two revisions are appended, and the backend refuses
 * unless every signature the document held reads afterwards as it did before.
 *
 * ## Nothing is held between tries
 *
 * A signing keeps the signature the OS made while the reader decides what to
 * do about a timestamp that did not come, because asking for the key twice is
 * a cost. Here no key is used: a refusal is a sentence, and the command is
 * run again.
 */

import { basename } from "./paths";
import type { Properties } from "./properties";
import {
  SERVERS,
  addressProblem,
  readStampChoice,
  stampUrl,
  writeStampChoice,
  type StampChoice,
} from "./signtimestamp";

/** The refusal for a document with edits nobody has saved. Mirrors `commands::validation::refuse_unsaved`. */
export const UNSAVED =
  "Save your changes first: validation data is added to the file as it is on disk, and this " +
  "document has edits that are not in it yet.";

/** What the window says while the authorities are asked and the copy is written. */
export const WAITING =
  "Asking the certificate authorities and the timestamp authority, and writing the copy...";

/** What a document nobody has signed is told, before anything is asked. */
export const NOTHING_SIGNED =
  "This document has no signature, so there is nothing to add long-term validation data for.";

/**
 * Where the authority chosen for this lives. **Its own key**, not a
 * signing's: a signing's remembered choice is *none* until the reader chooses
 * one there, and choosing an authority here must not turn into a signing that
 * asks a server by default.
 */
export const AUTHORITY_KEY = "tpdf.validationAuthority";

/** The name the save panel suggests: `contract.pdf` becomes `contract-long-term.pdf`. */
export function keptName(openPath: string): string {
  const stem = basename(openPath).replace(/\.pdf$/i, "");
  return `${stem}-long-term.pdf`;
}

/** Whether the document holds at least one signature, as its properties read. */
export function hasSignature(properties: Properties): boolean {
  return properties.signatures.some((signature) => signature.signed);
}

/** What {@link addValidationData} needs from the shell. */
export interface ValidationShell {
  /**
   * Commits whatever the reader is still typing and waits for the edit that
   * makes, as a signing does. Rejects when a draft cannot be committed.
   */
  settle(): Promise<void>;
  /** Whether the document has edits that are not in the file. Asked after {@link settle}. */
  dirty(): boolean;
  /** The file the document was opened from. */
  openPath: string;
  /** `document_properties`: what the document's signatures read as. */
  properties(): Promise<Properties>;
  /** The dialog: the timestamp authority's address, or `null` for Cancel. */
  chooseAuthority(): Promise<string | null>;
  /** The save panel, suggesting `suggested`: a path, or `null` for Cancel. */
  saveAs(suggested: string): Promise<string | null>;
  /** `add_validation_data`: the sentence about the written file. */
  add(path: string, timestamp: string): Promise<string>;
  /**
   * Says what is being waited for, or with `null` that the wait is over. The
   * requests can take a minute and a half between them, and a window that
   * says nothing for that long reads as one that did nothing.
   */
  waiting(message: string | null): void;
}

/**
 * The whole sequence, in the module note's order.
 *
 * Answers the sentence to show, or `null` when the reader cancelled. A refusal
 * from the backend is thrown on.
 */
export async function addValidationData(shell: ValidationShell): Promise<string | null> {
  // Before `dirty` is read, for the reason `signing.ts` gives: a form answer
  // commits only when its control loses the keyboard.
  await shell.settle();
  if (shell.dirty()) return UNSAVED;
  if (!hasSignature(await shell.properties())) return NOTHING_SIGNED;
  const timestamp = await shell.chooseAuthority();
  if (timestamp === null) return null;
  const path = await shell.saveAs(keptName(shell.openPath));
  if (!path) return null;
  shell.waiting(WAITING);
  try {
    const said = await shell.add(path, timestamp);
    return said;
  } finally {
    // Whether it answered or refused: the wait is over either way.
    shell.waiting(null);
  }
}

/** What the dialog says before the reader chooses. */
export const HELP =
  "tpdf asks the certificate authorities named in the signatures' certificates whether " +
  "those certificates are revoked, adds their answers to the document, and asks the " +
  "timestamp authority you choose for a timestamp over the whole. The certificate " +
  "authorities are sent the certificates' serial numbers, and the timestamp authority a " +
  "hash — nothing of the document. The result is saved as a new file; the original is not " +
  "changed. Every signature in the document is covered, or nothing is written. It works " +
  "while the certificates involved are still valid: once one has expired, its authority " +
  "no longer answers for it.";

/** What the dialog says when *Add* is pressed with no authority chosen. */
export const CHOOSE = "Choose the timestamp authority to ask.";

/**
 * The dialog: which timestamp authority to ask for the archive timestamp.
 *
 * As the reader last chose here, and nothing the first time. A modal on the
 * same surface as the signing chooser, built from `textContent` only.
 * Answers the address, or `null` for Cancel and Escape.
 */
export function askAuthority(
  storage?: () => Pick<Storage, "getItem" | "setItem">,
): Promise<string | null> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.className = "validation-data-dialog";
  dialog.setAttribute("aria-label", "Add long-term validation data");
  dialog.style.cssText =
    "max-width:560px;padding:22px;border:1px solid #8885;border-radius:12px;" +
    "font:13px/1.55 system-ui,-apple-system,sans-serif;" +
    "background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2");
  heading.textContent = "Add long-term validation data";
  const help = document.createElement("p");
  help.textContent = HELP;
  const remembered = readStampChoice(storage, AUTHORITY_KEY);
  const list = document.createElement("div");
  list.setAttribute("role", "radiogroup");
  list.setAttribute("aria-label", "Timestamp authority");
  list.style.cssText = "display:flex;flex-direction:column;gap:6px;margin:12px 0";
  const option = (value: string, text: string) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "validation-authority";
    radio.value = value;
    radio.checked = remembered.server === value;
    const label = document.createElement("label");
    label.append(radio, ` ${text}`);
    list.append(label);
    return radio;
  };
  const radios = [
    ...SERVERS.map((server) => option(server.name, `Timestamp from ${server.label}`)),
    option("other", "Timestamp from another authority:"),
  ];
  const otherUrl = document.createElement("input");
  otherUrl.type = "url";
  otherUrl.value = remembered.url;
  otherUrl.placeholder = "https://";
  otherUrl.setAttribute("aria-label", "Timestamp authority address");
  const problem = document.createElement("p");
  problem.setAttribute("role", "alert");
  list.append(otherUrl, problem);
  const footer = document.createElement("div");
  footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end";
  const cancel = document.createElement("button");
  cancel.textContent = "Cancel";
  const next = document.createElement("button");
  next.textContent = "Add…";
  footer.append(cancel, next);
  dialog.append(heading, help, list, footer);
  document.body.append(dialog);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (address: string | null) => {
      if (settled) return;
      settled = true;
      dialog.close();
      dialog.remove();
      previous?.focus();
      resolve(address);
    };
    cancel.addEventListener("click", () => finish(null));
    next.addEventListener("click", () => {
      const server = radios.find((radio) => radio.checked)?.value ?? "";
      const chosen: StampChoice = { server, url: otherUrl.value };
      // *Other* with an address tpdf would not ask holds the dialog open, and
      // says what is wrong with the address.
      if (server === "other") {
        const wrong = addressProblem(chosen.url);
        if (wrong !== null) {
          problem.textContent = wrong;
          return;
        }
      }
      // Nothing chosen holds the dialog open: there is no authority tpdf asks
      // without being told to.
      const address = stampUrl(chosen);
      if (address === null) {
        problem.textContent = CHOOSE;
        return;
      }
      writeStampChoice(chosen, storage, AUTHORITY_KEY);
      finish(address);
    });
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      finish(null);
    });
    dialog.addEventListener("close", () => finish(null));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal();
    (radios.find((radio) => radio.checked) ?? radios[0])?.focus();
  });
}
