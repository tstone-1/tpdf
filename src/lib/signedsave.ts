import type { Properties } from "./properties";
import { button, footer, paragraph } from "./dialogparts";
import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    locked:
      "A signature in this PDF says that nothing may change after it. A program that checks signatures will report the saved file as changed against that signature. Keep the original signed file. Continue saving?",
    signed:
      "This PDF contains digital signatures. Saving can invalidate them, even when the document permits form filling. Keep the original signed file. Continue saving?",
    unread:
      "The document's digital-signature status could not be read completely. Saving may invalidate signatures. Keep the original file. Continue saving?",
    merging:
      "Merging creates a new PDF and does not preserve digital signatures from the selected files. Keep the original signed files. Continue saving?",
    heading: "Digital signatures",
    cancel: "Cancel",
    saveAnyway: "Save anyway",
  },
  "de-DE": {
    locked:
      "Eine Signatur in diesem PDF besagt, dass sich nach ihr nichts ändern darf. Ein Programm, das Signaturen prüft, wird die gespeicherte Datei gegenüber dieser Signatur als geändert melden. Bewahre die signierte Originaldatei auf. Mit dem Speichern fortfahren?",
    signed:
      "Dieses PDF enthält Signaturen. Das Speichern kann sie ungültig machen, auch wenn das Dokument das Ausfüllen von Formularen erlaubt. Bewahre die signierte Originaldatei auf. Mit dem Speichern fortfahren?",
    unread:
      "Der Signaturstatus des Dokuments konnte nicht vollständig gelesen werden. Das Speichern kann Signaturen ungültig machen. Bewahre die Originaldatei auf. Mit dem Speichern fortfahren?",
    merging:
      "Beim Zusammenführen entsteht ein neues PDF, das die Signaturen der ausgewählten Dateien nicht übernimmt. Bewahre die signierten Originaldateien auf. Mit dem Speichern fortfahren?",
    heading: "Signaturen",
    cancel: "Abbrechen",
    saveAnyway: "Trotzdem speichern",
  },
});

export class SaveCancelled extends Error {}

/** A permitted DocMDP operation is not proof that this writer preserves it. */
export function signatureSaveMessage(properties: Properties | null): string | null {
  // A signature that permits no change is said first and in its own words:
  // after one, saving does not risk the signature, it breaks what it states.
  if (properties?.signatures.some((signature) => signature.lock === 1 || signature.certification === 1)) {
    return TEXT().locked;
  }
  if (properties?.signatures.some((signature) => signature.signed || signature.certification > 0)) {
    return TEXT().signed;
  }
  const limits = properties?.limits;
  if (!limits || limits.locked || limits.unreadable > 0 || limits.fields_dropped > 0
      || limits.signatures_dropped > 0 || limits.values_clipped > 0) {
    return TEXT().unread;
  }
  return null;
}

export async function confirmSignatureSave(read: () => Promise<Properties>,
  confirm: (message: string) => Promise<boolean>, merging = false): Promise<void> {
  let properties: Properties | null = null;
  try { if (!merging) properties = await read(); } catch { /* An unreadable status needs consent too. */ }
  const message = merging
    ? TEXT().merging
    : signatureSaveMessage(properties);
  // The exception's message is never shown: every caller returns on it.
  if (message && !await confirm(message)) throw new SaveCancelled("Saving cancelled.");
}


/** A modal warning, using the same webview dialog surface as signature placement. */
export function askSignatureSave(message: string): Promise<boolean> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  // No primary button: the work here is the risky one, and Cancel is the default.
  dialog.className = "signed-save-dialog tpdf-dialog";
  dialog.setAttribute("role", "alertdialog"); dialog.setAttribute("aria-label", TEXT().heading);
  const heading = document.createElement("h2"); heading.textContent = TEXT().heading;
  const text = paragraph("", message); text.id = "tpdf-signed-save-message";
  dialog.setAttribute("aria-describedby", text.id);
  const cancel = button(TEXT().cancel); cancel.autofocus = true;
  const save = button(TEXT().saveAnyway);
  dialog.append(heading, text, footer(cancel, save)); document.body.append(dialog);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (accepted: boolean) => {
      if (settled) return;
      settled = true; dialog.close(); dialog.remove(); previous?.focus(); resolve(accepted);
    };
    cancel.addEventListener("click", () => finish(false)); save.addEventListener("click", () => finish(true));
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); finish(false); });
    dialog.addEventListener("close", () => finish(false));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal(); cancel.focus();
  });
}
