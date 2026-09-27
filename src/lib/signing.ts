/**
 * Signing with a certificate the reader already has: the sequence and its words.
 *
 * `docs/PLAN.md` §9 Phase 6 step 2. The backend is `sign_prepare.rs` (the
 * worker's revision), `sign_cms.rs` (the CMS and the splice, in the app
 * process) and `keystore.rs` (the OS store). This module is the frontend's
 * half, and it is here rather than in `App.svelte` because that file is the
 * layer no gate reaches (`AGENTS.md`): the order of the questions, what the
 * reader is told at each refusal, and the sentence after signing all live here
 * with tests, and `App.svelte` only supplies the four things that need the
 * shell --- the chooser, the save panel, the command and the message area.
 *
 * ## The order is a decision
 *
 * 1. **Unsaved edits refuse first**, before anything is asked of the OS: a
 *    signature is over the file, and the reader is looking at the file plus
 *    their edits. The backend refuses the same thing; saying it here saves a
 *    trip through a certificate chooser to be told.
 * 2. **The certificates are listed**, and none usable is a message naming the
 *    ones that were found and why each is not offered.
 * 3. **The reader chooses**, then **names the new file** --- `<name>-signed.pdf`
 *    beside the original. The original is never written.
 * 4. **The result is what a worker read back from the written file**, one line
 *    per signature, the new one first. Its words are `integrity.ts`'s, so a
 *    signature the reader just made is described exactly as the properties
 *    dialog would describe it.
 */

import { basename } from "./paths";
import type { Integrity } from "./integrity";
import { WHY } from "./integrity";

/** A certificate the chooser may offer. Mirrors `sign_cms::Choice`. */
export interface Choice {
  /** What goes back to `sign_document` when it is picked. */
  id: string;
  subject: string;
  issuer: string;
  /** `YYYY-MM-DD HH:MM:SS UTC`. */
  expires: string;
  /** `RSA 3072`, `ECDSA P-256`. */
  method: string;
}

/** A certificate with a key that is not offered. Mirrors `sign_cms::Skipped`. */
export interface Skipped {
  subject: string;
  /** A clause: "it has expired". */
  why: string;
}

/** What `sign_identities` answers. Mirrors `sign_cms::Choices`. */
export interface Choices {
  usable: Choice[];
  skipped: Skipped[];
}

/** One signature in the written file. Mirrors `sign_cms::Checked`. */
export interface Checked {
  field: string;
  integrity: Integrity | null;
  ours: boolean;
}

/** What `sign_document` answers. Mirrors `sign_cms::Signed`. */
export interface Signed {
  path: string;
  field: string;
  signatures: Checked[];
}

/** The refusal for a document with edits nobody has saved. */
export const UNSAVED =
  "Save your changes first: a signature covers the file as it is on disk, and this " +
  "document has edits that are not in it yet.";

/** The name the save panel suggests: `report.pdf` becomes `report-signed.pdf`. */
export function signedName(openPath: string): string {
  const stem = basename(openPath).replace(/\.pdf$/i, "");
  return `${stem}-signed.pdf`;
}

/** The date part of an expiry, which is all a chooser needs to show. */
function day(expires: string): string {
  return expires.split(" ")[0] ?? expires;
}

/** One row of the chooser: who, from whom, until when, and how. */
export function choiceLabel(choice: Choice): string {
  return `${choice.subject} — issued by ${choice.issuer}, expires ${day(choice.expires)} (${choice.method})`;
}

/**
 * What to say when there is nothing to choose from.
 *
 * Names what *was* found, when anything was: a reader whose card holds only an
 * expired certificate is otherwise told they have none, and goes looking for
 * the wrong problem.
 */
export function nothingToChoose(choices: Choices): string {
  const none =
    "No certificate that can sign was found in your keychain or certificate store.";
  if (choices.skipped.length === 0) return none;
  const reasons = choices.skipped.map((s) => `${s.subject}: ${s.why}`).join("; ");
  return `${none} Found but not offered — ${reasons}.`;
}

/** A verdict in a few words, for a list of signatures. */
function verdict(integrity: Integrity | null): string {
  if (!integrity) return "not checked";
  switch (integrity.verdict) {
    case "intact":
      return "intact";
    case "weak":
      return "unchanged under SHA-1 only";
    case "altered":
      return "altered";
    case "broken":
      return "broken";
    case "unchecked":
      return `not checked (${integrity.why ? WHY[integrity.why] : "no reason given"})`;
  }
}

/**
 * The sentence after signing, from what a worker read back.
 *
 * Leads with the new signature and says where the file went; then every
 * earlier signature, because keeping those intact is the half of signing a
 * reader cannot see and most needs to be told. The new signature not being
 * intact is stated first and plainly --- it should not happen, since the app
 * refuses to write one its own check would not call intact, and a sentence that
 * buried it would be the reassuring branch.
 */
export function afterSigning(signed: Signed): string {
  const name = basename(signed.path);
  const ours = signed.signatures.find((s) => s.ours);
  const earlier = signed.signatures.filter((s) => !s.ours);
  if (!ours || ours.integrity?.verdict !== "intact") {
    return (
      `${name} was written, but reading it back did not find the new signature ` +
      `${signed.field} intact: ${ours ? verdict(ours.integrity) : "it is not in the file"}. ` +
      "Do not rely on that copy."
    );
  }
  let text = `Signed as ${signed.field} and saved to ${name}. Read back after writing, the signature is intact.`;
  if (earlier.length > 0) {
    const listed = earlier.map((s) => `${s.field} ${verdict(s.integrity)}`).join(", ");
    text += ` Earlier signature${earlier.length === 1 ? "" : "s"}: ${listed}.`;
  }
  return text;
}

/** What {@link signDocument} needs from the shell. */
export interface SigningShell {
  /** Whether the document has edits that are not in the file. */
  dirty(): boolean;
  /** The file the document was opened from. */
  openPath: string;
  /** `sign_identities`. */
  list(): Promise<Choices>;
  /** The chooser: the id picked, or `null` for Cancel. */
  choose(choices: Choice[]): Promise<string | null>;
  /** The save panel, suggesting `suggested`: a path, or `null` for Cancel. */
  saveAs(suggested: string): Promise<string | null>;
  /** `sign_document`. */
  sign(identity: string, path: string): Promise<Signed>;
}

/**
 * The whole sequence, in the module note's order.
 *
 * Answers the sentence to show, or `null` when the reader cancelled --- which
 * is an answer and not an event worth a message. A refusal from the backend is
 * thrown on, for the caller to show as it shows every other.
 */
export async function signDocument(shell: SigningShell): Promise<string | null> {
  if (shell.dirty()) return UNSAVED;
  const choices = await shell.list();
  if (choices.usable.length === 0) return nothingToChoose(choices);
  const identity = await shell.choose(choices.usable);
  if (identity === null) return null;
  const path = await shell.saveAs(signedName(shell.openPath));
  if (!path) return null;
  return afterSigning(await shell.sign(identity, path));
}

/**
 * The chooser: one radio button per certificate, the first selected.
 *
 * A modal on the same surface as the signed-save warning. Built from
 * `textContent` only, since a certificate's subject is somebody else's text.
 */
export function askIdentity(choices: Choice[]): Promise<string | null> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.className = "sign-identity-dialog";
  dialog.setAttribute("aria-label", "Sign document");
  dialog.style.cssText =
    "max-width:560px;padding:22px;border:1px solid #8885;border-radius:12px;" +
    "background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2");
  heading.textContent = "Sign document";
  const help = document.createElement("p");
  help.textContent =
    "Choose the certificate to sign with. The signed document is saved as a new file; " +
    "the original is not changed. Your operating system may ask for access to the key " +
    "or for a PIN — tpdf never sees it.";
  const list = document.createElement("div");
  list.setAttribute("role", "radiogroup");
  list.style.cssText = "display:flex;flex-direction:column;gap:6px;margin:12px 0";
  const radios = choices.map((choice, at) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "sign-identity";
    radio.value = choice.id;
    radio.checked = at === 0;
    const label = document.createElement("label");
    label.append(radio, ` ${choiceLabel(choice)}`);
    list.append(label);
    return radio;
  });
  const footer = document.createElement("div");
  footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end";
  const cancel = document.createElement("button");
  cancel.textContent = "Cancel";
  const next = document.createElement("button");
  next.textContent = "Sign…";
  footer.append(cancel, next);
  dialog.append(heading, help, list, footer);
  document.body.append(dialog);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (id: string | null) => {
      if (settled) return;
      settled = true;
      dialog.close();
      dialog.remove();
      previous?.focus();
      resolve(id);
    };
    cancel.addEventListener("click", () => finish(null));
    next.addEventListener("click", () =>
      finish(radios.find((radio) => radio.checked)?.value ?? null),
    );
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      finish(null);
    });
    dialog.addEventListener("close", () => finish(null));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal();
    radios[0]?.focus();
  });
}
