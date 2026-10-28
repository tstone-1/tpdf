import { decodeSignature, signatureCanvas, trimSignature, type SignatureImage } from "./signature";

import { isLinux } from "./keys";
import { prepareSignatureStorage, loadSignature, rememberSignature, forgetSignature } from "./signaturestore";
import { button, footer, option, paragraph, row } from "./dialogparts";
import { catalog } from "./i18n";
import { spokenError } from "./said";

const TEXT = catalog({
  "en-US": {
    place: "Place signature image",
    image: "Signature image",
    useImage: "Use this image",
    help: "Draw your signature or import a PNG or JPEG image.",
    notice: "Visual mark only. This does not verify your identity or create a "
      + "certificate-based digital signature.",
    drawHere: "Draw your signature",
    importImage: "Import image...",
    clear: "Clear",
    useSaved: "Use saved signature",
    forgetSaved: "Forget saved signature",
    removeWhite: "Remove white background",
    remember: "Remember on this device",
    cancel: "Cancel",
    tooLarge: "Choose a PNG or JPEG image smaller than 10 MB.",
    notOpened: "This image could not be opened.",
    nothingDrawn: "Draw or import a signature first.",
    notRemembered: "The signature could not be remembered securely. "
      + "Clear that option to place it without saving it locally.",
    savedNotPlaced: "The earlier signature was saved on this device but not placed, "
      + "because the signature changed. Place the current one when ready.",
    noneSaved: "No signature has been saved on this device.",
    removed: "Saved signature removed from this device.",
    notRemoved: "The saved signature could not be removed.",
  },
  "de-DE": {
    place: "Unterschriftsbild platzieren",
    image: "Unterschriftsbild",
    useImage: "Dieses Bild verwenden",
    help: "Zeichne deine Unterschrift oder importiere ein PNG- oder JPEG-Bild.",
    notice: "Nur eine sichtbare Markierung. Sie bestätigt nicht deine Identität und erzeugt "
      + "keine zertifikatsbasierte Signatur.",
    drawHere: "Zeichne deine Unterschrift",
    importImage: "Bild importieren...",
    clear: "Leeren",
    useSaved: "Gespeicherte Unterschrift verwenden",
    forgetSaved: "Gespeicherte Unterschrift entfernen",
    removeWhite: "Weißen Hintergrund entfernen",
    remember: "Auf diesem Gerät merken",
    cancel: "Abbrechen",
    tooLarge: "Wähle ein PNG- oder JPEG-Bild unter 10 MB.",
    notOpened: "Dieses Bild konnte nicht geöffnet werden.",
    nothingDrawn: "Zeichne oder importiere zuerst eine Unterschrift.",
    notRemembered: "Die Unterschrift konnte nicht sicher gespeichert werden. "
      + "Deaktiviere diese Option, um sie zu platzieren, ohne sie lokal zu speichern.",
    savedNotPlaced: "Die vorherige Unterschrift wurde auf diesem Gerät gespeichert, aber nicht platziert, "
      + "weil sich die Unterschrift geändert hat. Platziere die aktuelle, wenn du so weit bist.",
    noneSaved: "Auf diesem Gerät ist keine Unterschrift gespeichert.",
    removed: "Die gespeicherte Unterschrift wurde von diesem Gerät entfernt.",
    notRemoved: "Die gespeicherte Unterschrift konnte nicht entfernt werden.",
  },
});

/** What the dialog is called and what its accepting button says, for one use of it. */
export interface SignatureDialogWords { title: string; action: string }

/** The words for placing a visual mark, which is what the dialog was built for. */
export const PLACE_WORDS: SignatureDialogWords = { title: TEXT().place, action: TEXT().place };

/** The words for choosing the image a digital signature's appearance shows. */
export const APPEARANCE_WORDS: SignatureDialogWords = { title: TEXT().image, action: TEXT().useImage };

/** A visual signature editor; image files are decoded by the webview, never Rust. */
export class SignatureDialog {
  private readonly dialog = document.createElement("dialog");
  private readonly canvas = document.createElement("canvas");
  private readonly message = document.createElement("p");
  private readonly remember = document.createElement("input");
  private readonly white = document.createElement("input");
  private readonly place: HTMLButtonElement;
  private readonly heading = document.createElement("h2");
  private source: HTMLCanvasElement | null = null;
  private pending: ((image: SignatureImage | null) => void) | null = null;
  private previousFocus: HTMLElement | null = null;
  private point: { x: number; y: number } | null = null;
  private generation = 0;

  constructor(host: HTMLElement) {
    void prepareSignatureStorage().catch((error: unknown) => { this.message.textContent = spokenError(error); });
    this.dialog.className = "signature-dialog tpdf-dialog";
    const said = TEXT();
    this.dialog.setAttribute("aria-label", PLACE_WORDS.title);
    const heading = this.heading;
    heading.textContent = PLACE_WORDS.title;
    const help = paragraph("tpdf-dialog-intro", said.help);
    const notice = paragraph("tpdf-dialog-hint", said.notice);
    notice.style.marginTop = "4px";
    this.canvas.width = 1024; this.canvas.height = 400;
    this.canvas.style.cssText = "box-sizing:border-box;width:100%;height:auto;aspect-ratio:1024/400;display:block;"
      + "background:white;border:1px solid #999;border-radius:6px;touch-action:none;cursor:crosshair;margin-top:14px";
    this.canvas.setAttribute("aria-label", said.drawHere);
    const tools = row(); tools.style.margin = "12px 0";
    const file = document.createElement("input"); file.type = "file"; file.accept = "image/png,image/jpeg"; file.hidden = true;
    const load = this.button(said.importImage, () => { file.value = ""; file.click(); });
    file.addEventListener("change", () => { const chosen = file.files?.[0]; if (chosen) void this.importFile(chosen); });
    // Linux keeps no saved signature yet (`signature_store.rs`), so nothing
    // offers to save one, use it or forget it there.
    const keeps = !isLinux();
    tools.append(load, this.button(said.clear, () => this.clear()));
    if (keeps) tools.append(
      this.button(said.useSaved, () => void this.loadSaved()),
      this.button(said.forgetSaved, () => void this.forgetSaved()),
    );
    const options = row();
    const label = (input: HTMLInputElement, text: string) => {
      input.type = "checkbox";
      return option(input, text);
    };
    options.append(label(this.white, said.removeWhite));
    if (keeps) options.append(label(this.remember, said.remember));
    this.white.checked = true;
    this.white.addEventListener("change", () => { if (this.source) this.render(); });
    this.message.setAttribute("role", "status"); this.message.style.cssText = "min-height:1.5em;margin:10px 0 0";
    this.place = this.button(PLACE_WORDS.action, () => void this.accept()); this.place.disabled = true;
    this.place.className = "primary";
    this.dialog.append(heading, help, notice, this.canvas, tools, options, this.message,
      footer(this.button(said.cancel, () => this.finish(null)), this.place), file);
    host.append(this.dialog);
    this.dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.finish(null); });
    this.dialog.addEventListener("keydown", (event) => event.stopPropagation());
    const position = (e: PointerEvent) => {
      const r = this.canvas.getBoundingClientRect();
      return {
        x: (e.clientX - r.left) * this.canvas.width / r.width,
        y: (e.clientY - r.top) * this.canvas.height / r.height,
      };
    };
    this.canvas.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      this.source = null; this.generation++; this.point = position(e);
      // PointerDrag uses the same fallback for synthetic accessibility/check events.
      try { this.canvas.setPointerCapture(e.pointerId); } catch { /* No active browser pointer. */ }
      this.stroke(this.point); e.preventDefault();
    });
    this.canvas.addEventListener("pointermove", (e) => { if (this.point) this.stroke(position(e)); });
    const end = () => { this.point = null; };
    this.canvas.addEventListener("pointerup", end); this.canvas.addEventListener("pointercancel", end);
  }

  get isOpen(): boolean { return this.dialog.open; }
  /** Opens the dialog; `words` name it for one use (the signing panel's, say). */
  ask(words: SignatureDialogWords = PLACE_WORDS): Promise<SignatureImage | null> {
    if (this.pending) this.finish(null);
    this.heading.textContent = words.title;
    this.dialog.setAttribute("aria-label", words.title);
    this.place.textContent = words.action;
    this.previousFocus = document.activeElement as HTMLElement | null;
    this.clear(); this.remember.checked = false;
    this.dialog.showModal();
    return new Promise((resolve) => { this.pending = resolve; });
  }
  dispose(): void { this.finish(null); this.dialog.remove(); }

  private button(text: string, action: () => void): HTMLButtonElement {
    const made = button(text); made.addEventListener("click", action); return made;
  }
  private clear(): void {
    this.generation++; this.point = null; this.source = null;
    this.canvas.getContext("2d")!.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.message.textContent = ""; this.place.disabled = true;
  }
  private stroke(next: { x: number; y: number }): void {
    const ctx = this.canvas.getContext("2d")!;
    ctx.strokeStyle = "#17243a"; ctx.fillStyle = "#17243a"; ctx.lineWidth = 4; ctx.lineCap = "round"; ctx.lineJoin = "round";
    ctx.beginPath(); ctx.moveTo(this.point!.x, this.point!.y); ctx.lineTo(next.x, next.y); ctx.stroke();
    ctx.beginPath(); ctx.arc(next.x, next.y, 2, 0, Math.PI*2); ctx.fill();
    this.point = next; this.place.disabled = false; this.message.textContent = "";
  }
  private async importFile(file: File): Promise<void> {
    const generation = ++this.generation;
    if (!["image/png", "image/jpeg"].includes(file.type) || file.size > 10 * 1024 * 1024) {
      this.message.textContent = TEXT().tooLarge; return;
    }
    let image: ImageBitmap | undefined;
    try {
      image = await decodeSignature(file);
      if (generation !== this.generation || !this.isOpen) return;
      const source = document.createElement("canvas");
      const scale = Math.min(1, 1024/image.width, 512/image.height);
      source.width = Math.max(1, Math.round(image.width*scale)); source.height = Math.max(1, Math.round(image.height*scale));
      source.getContext("2d")!.drawImage(image, 0, 0, source.width, source.height);
      this.source = source; this.render(); this.message.textContent = "";
    } catch (error) {
      if (generation === this.generation)
        this.message.textContent = error instanceof Error ? error.message : TEXT().notOpened;
    }
    finally { image?.close(); }
  }
  private render(): void {
    if (!this.source) return;
    const ctx = this.canvas.getContext("2d")!;
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    const source = this.source.getContext("2d")!.getImageData(0, 0, this.source.width, this.source.height);
    if (this.white.checked) {
      for (let i = 0; i < source.data.length; i += 4) {
        if (Math.min(source.data[i]!, source.data[i+1]!, source.data[i+2]!) > 240) source.data[i+3] = 0;
      }
    }
    const trimmed = trimSignature(source.width, source.height, source.data);
    if (!trimmed) { this.place.disabled = true; return; }
    const sourceCanvas = signatureCanvas(trimmed);
    const scale = Math.min((this.canvas.width-24)/trimmed.width, (this.canvas.height-24)/trimmed.height);
    ctx.drawImage(sourceCanvas, 12, 12, trimmed.width*scale, trimmed.height*scale);
    this.place.disabled = false;
  }
  private async accept(): Promise<void> {
    const generation = this.generation;
    const ctx = this.canvas.getContext("2d")!;
    const drawn = ctx.getImageData(0, 0, this.canvas.width, this.canvas.height).data;
    const trimmed = trimSignature(this.canvas.width, this.canvas.height, drawn);
    if (!trimmed) { this.message.textContent = TEXT().nothingDrawn; return; }
    const scale = Math.min(1, 512/trimmed.width, 256/trimmed.height);
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(trimmed.width*scale));
    canvas.height = Math.max(1, Math.round(trimmed.height*scale));
    const small = canvas.getContext("2d")!;
    small.drawImage(signatureCanvas(trimmed), 0, 0, canvas.width, canvas.height);
    const pixels = small.getImageData(0, 0, canvas.width, canvas.height).data;
    const image: SignatureImage = { width: canvas.width, height: canvas.height, rgba: [...pixels] };
    const remembering = this.remember.checked;
    if (remembering) {
      try { this.place.disabled = true; await rememberSignature(image); }
      catch {
        this.place.disabled = false;
        this.message.textContent = TEXT().notRemembered;
        return;
      }
    }
    if (generation === this.generation && this.isOpen) { this.finish(image); return; }
    // **Superseded while the save was in flight.** Placing is "remember, then
    // place", and the remember has finished: the device now holds this image
    // because the reader asked for it with the box ticked. It is not undone,
    // because the store holds one signature and forgetting it would also delete
    // the one saved before. What must not happen is placing it --- the reader
    // cancelled, or drew or loaded something else, after pressing Place --- or
    // saying nothing: a dialog still showing is told what was kept, so the next
    // "Use saved signature" is not a surprise. A closed dialog has nobody to
    // tell, and there the cancel is the answer the reader gave.
    if (remembering && this.isOpen)
      this.message.textContent = TEXT().savedNotPlaced;
  }
  private async loadSaved(): Promise<void> {
    const generation = ++this.generation;
    try {
      const saved = await loadSignature();
      if (generation !== this.generation || !this.isOpen) return;
      if (saved) { this.source = signatureCanvas(saved); this.render(); }
      else this.message.textContent = TEXT().noneSaved;
    } catch (error) { if (generation === this.generation) this.message.textContent = spokenError(error); }
  }
  private async forgetSaved(): Promise<void> {
    try { await forgetSignature(); this.remember.checked = false; this.message.textContent = TEXT().removed; }
    catch { this.message.textContent = TEXT().notRemoved; }
  }
  private finish(image: SignatureImage | null): void {
    this.generation++; this.point = null;
    const resolve = this.pending; this.pending = null;
    if (this.dialog.open) this.dialog.close(); this.previousFocus?.focus(); resolve?.(image);
  }
}
