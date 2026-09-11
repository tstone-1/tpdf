import { SIGNATURE_KEY, savedSignature, signatureCanvas, trimSignature, type SignatureImage } from "./signature";

/** A visual signature editor; image files are decoded by the webview, never Rust. */
export class SignatureDialog {
  private readonly dialog = document.createElement("dialog");
  private readonly canvas = document.createElement("canvas");
  private readonly message = document.createElement("p");
  private readonly remember = document.createElement("input");
  private readonly white = document.createElement("input");
  private readonly place: HTMLButtonElement;
  private source: HTMLCanvasElement | null = null;
  private pending: ((image: SignatureImage | null) => void) | null = null;
  private previousFocus: HTMLElement | null = null;
  private point: { x: number; y: number } | null = null;
  private generation = 0;

  constructor(host: HTMLElement) {
    this.dialog.className = "signature-dialog";
    this.dialog.setAttribute("aria-label", "Add signature");
    this.dialog.style.cssText = "width:min(580px,90vw);padding:22px;border:1px solid #8885;border-radius:12px;background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
    const heading = document.createElement("h2"); heading.textContent = "Add signature"; heading.style.margin = "0 0 8px";
    const help = document.createElement("p"); help.textContent = "Draw your signature or import a PNG or JPEG image.";
    this.canvas.width = 1024; this.canvas.height = 400;
    this.canvas.style.cssText = "width:100%;height:auto;aspect-ratio:1024/400;display:block;background:white;border:1px solid #999;border-radius:6px;touch-action:none;cursor:crosshair";
    this.canvas.setAttribute("aria-label", "Draw your signature");
    const tools = document.createElement("div"); tools.style.cssText = "display:flex;gap:8px;margin:12px 0;flex-wrap:wrap";
    const file = document.createElement("input"); file.type = "file"; file.accept = "image/png,image/jpeg"; file.hidden = true;
    const load = this.button("Import image...", () => { file.value = ""; file.click(); });
    file.addEventListener("change", () => { const chosen = file.files?.[0]; if (chosen) void this.importFile(chosen); });
    tools.append(load, this.button("Clear", () => this.clear()), this.button("Use saved signature", () => {
      const saved = savedSignature();
      if (saved) { this.generation++; this.source = signatureCanvas(saved); this.render(); } else this.message.textContent = "No signature has been saved on this device.";
    }), this.button("Forget saved signature", () => {
      try { localStorage.removeItem(SIGNATURE_KEY); this.remember.checked = false; this.message.textContent = "Saved signature removed from this device."; }
      catch { this.message.textContent = "The saved signature could not be removed."; }
    }));
    const options = document.createElement("div"); options.style.cssText = "display:flex;gap:16px;flex-wrap:wrap";
    const label = (input: HTMLInputElement, text: string) => { input.type = "checkbox"; const node = document.createElement("label"); node.append(input, ` ${text}`); return node; };
    options.append(label(this.white, "Remove white background"), label(this.remember, "Remember on this device"));
    this.white.checked = true;
    this.white.addEventListener("change", () => { if (this.source) this.render(); });
    this.message.setAttribute("role", "status"); this.message.style.cssText = "min-height:1.5em;font-size:13px";
    const footer = document.createElement("div"); footer.style.cssText = "display:flex;gap:8px;justify-content:flex-end";
    this.place = this.button("Place signature", () => this.accept()); this.place.disabled = true;
    footer.append(this.button("Cancel", () => this.finish(null)), this.place);
    this.dialog.append(heading, help, this.canvas, tools, options, this.message, footer, file);
    host.append(this.dialog);
    this.dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.finish(null); });
    this.dialog.addEventListener("keydown", (event) => event.stopPropagation());
    const position = (e: PointerEvent) => { const r = this.canvas.getBoundingClientRect(); return { x: (e.clientX-r.left)*this.canvas.width/r.width, y: (e.clientY-r.top)*this.canvas.height/r.height }; };
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
  ask(): Promise<SignatureImage | null> {
    if (this.pending) this.finish(null);
    this.previousFocus = document.activeElement as HTMLElement | null;
    this.clear(); this.remember.checked = false;
    this.dialog.showModal();
    return new Promise((resolve) => { this.pending = resolve; });
  }
  dispose(): void { this.finish(null); this.dialog.remove(); }

  private button(text: string, action: () => void): HTMLButtonElement {
    const button = document.createElement("button"); button.type = "button"; button.textContent = text;
    button.style.cssText = "padding:7px 12px;border:1px solid #8886;border-radius:5px;background:ButtonFace;color:ButtonText;cursor:pointer";
    button.addEventListener("click", action); return button;
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
      this.message.textContent = "Choose a PNG or JPEG image smaller than 10 MB."; return;
    }
    let image: ImageBitmap | undefined;
    try {
      image = await createImageBitmap(file);
      if (generation !== this.generation || !this.isOpen) return;
      const source = document.createElement("canvas");
      const scale = Math.min(1, 1024/image.width, 512/image.height);
      source.width = Math.max(1, Math.round(image.width*scale)); source.height = Math.max(1, Math.round(image.height*scale));
      source.getContext("2d")!.drawImage(image, 0, 0, source.width, source.height);
      this.source = source; this.render(); this.message.textContent = "";
    } catch { if (generation === this.generation) this.message.textContent = "This image could not be opened."; }
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
  private accept(): void {
    const ctx = this.canvas.getContext("2d")!;
    const trimmed = trimSignature(this.canvas.width, this.canvas.height, ctx.getImageData(0, 0, this.canvas.width, this.canvas.height).data);
    if (!trimmed) { this.message.textContent = "Draw or import a signature first."; return; }
    const scale = Math.min(1, 512/trimmed.width, 256/trimmed.height);
    const canvas = document.createElement("canvas"); canvas.width = Math.max(1, Math.round(trimmed.width*scale)); canvas.height = Math.max(1, Math.round(trimmed.height*scale));
    canvas.getContext("2d")!.drawImage(signatureCanvas(trimmed), 0, 0, canvas.width, canvas.height);
    const image: SignatureImage = { width: canvas.width, height: canvas.height, rgba: [...canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height).data] };
    if (this.remember.checked) {
      try { localStorage.setItem(SIGNATURE_KEY, JSON.stringify(image)); }
      catch { this.message.textContent = "The signature could not be remembered. Clear that option to place it without saving it locally."; return; }
    }
    this.finish(image);
  }
  private finish(image: SignatureImage | null): void {
    this.generation++; this.point = null;
    const resolve = this.pending; this.pending = null;
    if (this.dialog.open) this.dialog.close(); this.previousFocus?.focus(); resolve?.(image);
  }
}
