import type { Properties } from "./properties";

export class SaveCancelled extends Error {}

/** A permitted DocMDP operation is not proof that this writer preserves it. */
export function signatureSaveMessage(properties: Properties | null): string | null {
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
  dialog.className = "signed-save-dialog";
  dialog.setAttribute("role", "alertdialog"); dialog.setAttribute("aria-label", "Digital signatures");
  dialog.style.cssText = "max-width:480px;padding:22px;border:1px solid #8885;border-radius:12px;background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2"); heading.textContent = "Digital signatures";
  const text = document.createElement("p"); text.textContent = message; text.id = "tpdf-signed-save-message";
  dialog.setAttribute("aria-describedby", text.id);
  const footer = document.createElement("div"); footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end";
  const cancel = document.createElement("button"); cancel.textContent = "Cancel"; cancel.autofocus = true;
  const save = document.createElement("button"); save.textContent = "Save anyway";
  footer.append(cancel, save); dialog.append(heading, text, footer); document.body.append(dialog);
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
