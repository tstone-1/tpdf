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
import { button, footer, option, paragraph, section } from "./dialogparts";
import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    unsaved:
      "Save your changes first: validation data is added to the file as it is on disk, and this " +
      "document has edits that are not in it yet.",
    waiting:
      "Asking the certificate authorities and the timestamp authority, and writing the copy...",
    nothingSigned:
      "This document has no signature, so there is nothing to add long-term validation data for.",
    help:
      "tpdf asks the certificate authorities named in the signatures' certificates whether " +
      "those certificates are revoked, adds their answers to the document, and asks the " +
      "timestamp authority you choose for a timestamp over the whole. The certificate " +
      "authorities are sent the certificates' serial numbers, and the timestamp authority a " +
      "hash — nothing of the document. The result is saved as a new file; the original is not " +
      "changed. Every signature in the document is covered, or nothing is written. It works " +
      "while the certificates involved are still valid: once one has expired, its authority " +
      "no longer answers for it.",
    choose: "Choose the timestamp authority to ask.",
    title: "Add long-term validation data",
    authority: "Timestamp authority",
    another: "Another authority…",
    address: "Timestamp authority address",
    cancel: "Cancel",
    add: "Add…",
  },
  "de-DE": {
    unsaved:
      "Speichere zuerst deine Änderungen: Die Validierungsdaten werden der Datei so hinzugefügt, " +
      "wie sie auf dem Datenträger liegt, und dieses Dokument hat Änderungen, die noch nicht " +
      "darin sind.",
    waiting:
      "Die Zertifizierungsstellen und der Zeitstempeldienst werden angefragt, und die Kopie wird " +
      "geschrieben...",
    nothingSigned:
      "Dieses Dokument hat keine Signatur, also gibt es nichts, wofür Langzeitvalidierungsdaten " +
      "hinzugefügt werden könnten.",
    help:
      "tpdf fragt die Zertifizierungsstellen, die in den Zertifikaten der Signaturen genannt sind, " +
      "ob diese Zertifikate widerrufen sind, fügt ihre Antworten dem Dokument hinzu und bittet den " +
      "gewählten Zeitstempeldienst um einen Zeitstempel über das Ganze. Die " +
      "Zertifizierungsstellen erhalten die Seriennummern der Zertifikate, der Zeitstempeldienst " +
      "einen Hash — nichts vom Dokument. Das Ergebnis wird als neue Datei gespeichert; das " +
      "Original bleibt unverändert. Entweder wird jede Signatur im Dokument erfasst, oder es wird " +
      "nichts geschrieben. Das funktioniert nur, solange die beteiligten Zertifikate noch gültig " +
      "sind: Ist eines abgelaufen, gibt seine Zertifizierungsstelle keine Auskunft mehr darüber.",
    choose: "Wähle den Zeitstempeldienst, der angefragt werden soll.",
    title: "Langzeitvalidierungsdaten hinzufügen",
    authority: "Zeitstempeldienst",
    another: "Anderer Zeitstempeldienst…",
    address: "Adresse des Zeitstempeldienstes",
    cancel: "Abbrechen",
    add: "Hinzufügen…",
  },
  "de-DE-formal": {
    unsaved: "Speichern Sie zuerst Ihre Änderungen: Die Validierungsdaten werden der Datei so hinzugefügt, " +
      "wie sie auf dem Datenträger liegt, und dieses Dokument hat Änderungen, die noch nicht " +
      "darin sind.",
    choose: "Wählen Sie den Zeitstempeldienst, der angefragt werden soll.",
  },
});

/**
 * The refusal for a document with edits nobody has saved. Mirrors
 * `commands::validation::refuse_unsaved`, which stays English until the
 * worker's messages are translated.
 */
export const UNSAVED = TEXT().unsaved;

/** What the window says while the authorities are asked and the copy is written. */
export const WAITING = TEXT().waiting;

/** What a document nobody has signed is told, before anything is asked. */
export const NOTHING_SIGNED = TEXT().nothingSigned;

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
export const HELP = TEXT().help;

/** What the dialog says when *Add* is pressed with no authority chosen. */
export const CHOOSE = TEXT().choose;

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
  dialog.className = "validation-data-dialog tpdf-dialog";
  const said = TEXT();
  dialog.setAttribute("aria-label", said.title);
  const heading = document.createElement("h2");
  heading.textContent = said.title;
  const help = paragraph("tpdf-dialog-intro", HELP);
  const remembered = readStampChoice(storage, AUTHORITY_KEY);
  const authority = section(said.authority);
  const list = document.createElement("div");
  list.setAttribute("role", "radiogroup");
  list.setAttribute("aria-label", said.authority);
  list.className = "tpdf-dialog-stack";
  const entry = (value: string, text: string) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "validation-authority";
    radio.value = value;
    radio.checked = remembered.server === value;
    list.append(option(radio, text));
    return radio;
  };
  const radios = [
    ...SERVERS.map((server) => entry(server.name, server.label)),
    entry("other", said.another),
  ];
  const otherUrl = document.createElement("input");
  otherUrl.type = "url";
  otherUrl.value = remembered.url;
  otherUrl.placeholder = "https://";
  otherUrl.setAttribute("aria-label", said.address);
  // The address shows only while *another authority* is chosen. The alert
  // stays outside it: it also says that no authority is chosen at all.
  const syncOther = () => {
    otherUrl.hidden = radios.find((radio) => radio.checked)?.value !== "other";
  };
  for (const radio of radios) radio.addEventListener("change", syncOther);
  syncOther();
  const problem = document.createElement("p");
  problem.setAttribute("role", "alert");
  authority.append(list, otherUrl, problem);
  const cancel = button(said.cancel);
  const next = button(said.add, true);
  dialog.append(heading, help, authority, footer(cancel, next));
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
