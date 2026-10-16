import type { Properties } from "./properties";
import { button, footer, paragraph } from "./dialogparts";

export class SaveCancelled extends Error {}

/** A permitted DocMDP operation is not proof that this writer preserves it. */
export function signatureSaveMessage(properties: Properties | null): string | null {
  // A signature that permits no change is said first and in its own words:
  // after one, saving does not risk the signature, it breaks what it states.
  if (properties?.signatures.some((signature) => signature.lock === 1 || signature.certification === 1)) {
    return "A signature in this PDF says that nothing may change after it. A program that checks signatures will report the saved file as changed against that signature. Keep the original signed file. Continue saving?";
  }
  if (properties?.signatures.some((signature) => signature.signed || signature.certification > 0)) {
    return "This PDF contains digital signatures. Saving can invalidate them, even when the document permits form filling. Keep the original signed file. Continue saving?";
  }
  const limits = properties?.limits;
  if (!limits || limits.locked || limits.unreadable > 0 || limits.fields_dropped > 0
      || limits.signatures_dropped > 0 || limits.values_clipped > 0) {
    return "The document's digital-signature status could not be read completely. Saving may invalidate signatures. Keep the original file. Continue saving?";
  }
  return null;
}

export async function confirmSignatureSave(read: () => Promise<Properties>,
  confirm: (message: string) => Promise<boolean>, merging = false): Promise<void> {
  let properties: Properties | null = null;
  try { if (!merging) properties = await read(); } catch { /* An unreadable status needs consent too. */ }
  const message = merging
    ? "Merging creates a new PDF and does not preserve digital signatures from the selected files. Keep the original signed files. Continue saving?"
    : signatureSaveMessage(properties);
  if (message && !await confirm(message)) throw new SaveCancelled("Saving cancelled.");
}


/** A modal warning, using the same webview dialog surface as signature placement. */
export function askSignatureSave(message: string): Promise<boolean> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  // No primary button: the work here is the risky one, and Cancel is the default.
  dialog.className = "signed-save-dialog tpdf-dialog";
  dialog.setAttribute("role", "alertdialog"); dialog.setAttribute("aria-label", "Digital signatures");
  const heading = document.createElement("h2"); heading.textContent = "Digital signatures";
  const text = paragraph("", message); text.id = "tpdf-signed-save-message";
  dialog.setAttribute("aria-describedby", text.id);
  const cancel = button("Cancel"); cancel.autofocus = true;
  const save = button("Save anyway");
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
