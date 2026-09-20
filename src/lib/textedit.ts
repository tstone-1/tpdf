import type { EditState } from "./edits";
import { defaultTextLayout, TextLayoutControls } from "./textlayout";

/** Worker-inspected source addresses; these always refer to the original PDF. */
export interface TextRun {
  operator: number;
  text: string;
  font: string;
  size: number;
  matrix: [number, number, number, number, number, number];
  advance: number;
  display_rect: [number, number, number, number];
  minimum_height?: number;
}
export interface TextPreview { png: number[]; font: string;
  /** The box the draft needed: what the dashed outline follows. */
  rect: [number, number, number, number];
  /** That box together with every run the draft pushes along its line, which is
   * what the worker's crop covers. Only the backend reads it. */
  extent: [number, number, number, number];
  lines: number }
export interface TextRuns { page: number;
  /** Which document `page` is a page of: the file an inserted page came from,
   * by the model's id for it, and absent for the document the reader opened.
   * Mirrors `textedit::PageRuns::source`, and it is what lets the editor tell
   * its own pending replacements from those on the opened file's page of the
   * same number. */
  source?: number;
  revision: number[]; runs: TextRun[]; preview?: TextPreview }
export interface TextLayout { width: number; height: number; size: number; wrap: boolean;
  font: "auto" | "original" | "noto_sans" | "noto_sans_bold" | "noto_sans_italic" | "noto_sans_bold_italic" | "noto_sans_cjk_sc" | "noto_sans_cjk_sc_bold";
  /** The reader has not sized this box, so it follows the text they type as far
   * as the room after the run allows; see `TextLayoutControls` and, in the
   * worker, `textedit::layout::free_width`. */
  grow: boolean }
export interface TextChange { page: number;
  /** The file the page came from, for a page inserted from another document;
   * absent for a page of the opened one. Mirrors `textedit::Edit::source`.
   *
   * **Only the backend sets it.** A draft on its way to `text_replace` does
   * not carry one: the command resolves the file from the journal page id it
   * is given, which is the only name for a page that cannot be ambiguous. */
  source?: number;
  revision: number[]; operator: number; original: string; replacement: string; layout?: TextLayout }

/** A page of a document, and which document --- {@link TextChange}'s address. */
export interface TextPage { readonly source?: number; readonly page: number }

/** Whether two addresses name the same page of the same document. */
export function sameTextPage(a: TextPage, b: TextPage): boolean {
  return a.page === b.page && (a.source ?? -1) === (b.source ?? -1);
}

/** Compare per-page bodies, including edits removed by undo.
 *
 * **Keyed by the file as well as the page number**, and that is the whole of
 * what an inserted page costs this: page 3 of the opened document and page 3
 * of a file inserted from are two different pages that render in two
 * different slots, so a key of the number alone would report an edit on one
 * of them as a change to the other --- repainting a page nobody touched and
 * leaving the edited one showing its old words. */
export function changedTextPages(before: readonly TextChange[], after: readonly TextChange[]): TextPage[] {
  const key = (change: TextPage) => JSON.stringify([change.source ?? null, change.page]);
  const group = (changes: readonly TextChange[]) => {
    const pages = new Map<string, string[]>();
    for (const change of changes) {
      const entries = pages.get(key(change)) ?? [];
      entries.push(JSON.stringify(change)); pages.set(key(change), entries);
    }
    return new Map([...pages].map(([page, entries]) => [page, JSON.stringify(entries.sort())]));
  };
  const old = group(before), next = group(after);
  return [...new Set([...old.keys(), ...next.keys()])]
    .filter((page) => old.get(page) !== next.get(page))
    .map((page) => { const [source, at] = JSON.parse(page) as [number | null, number];
      return source === null ? { page: at } : { source, page: at }; });
}

export function replacementError(value: string, wrap = false): string | null {
  return Array.from(value).length > 4096 || /[\x00-\x1f\x7f-\x9f\ud800-\udfff\u2028\u2029]/u.test(wrap ? value.replaceAll("\n", "") : value)
    ? "Use at most 4096 characters without control characters. Enable wrapping for line breaks."
    : null;
}

type Anchor = { left: number; top: number; right: number; bottom: number; clip: string };

/** Hit targets use viewer geometry; the worker renders the accepted replacement. */
export class TextEditor {
  private readonly root = document.createElement("div");
  private readonly toolbar = document.createElement("div");
  private readonly popup = document.createElement("div");
  private readonly input = document.createElement("textarea");
  private readonly controls: TextLayoutControls;
  // webview-sink-ok: source is a local image/png Blob rendered by the bounded worker, never a document URL.
  private readonly previewImage = document.createElement("img");
  private readonly outline = document.createElement("div");
  private previewUrl: string | null = null;
  private previewRect: TextPreview["rect"] | null = null;
  private previewTimer: ReturnType<typeof setTimeout> | undefined;
  private previewGeneration = 0;
  private acceptedLayout = "";
  private layoutTouched = false;
  private readonly message = document.createElement("p");
  private readonly apply = document.createElement("button");
  private readonly buttons: HTMLButtonElement[] = [];
  private readonly done: HTMLButtonElement;
  private active: TextRun | null = null;
  private changes: readonly TextChange[] = [];
  private accepted = "";
  private pending: Promise<void> = Promise.resolve();
  private failure: unknown = null;
  private busy = false;
  private saving = false;
  private disposed = false;

  constructor(host: HTMLElement, private readonly pageId: number, private readonly source: TextRuns,
    private readonly anchor: (run: TextRun) => Anchor | null,
    private readonly write: (change: TextChange) => Promise<EditState>,
    private readonly close: () => void,
    private readonly preview?: (change: TextChange) => Promise<TextRuns>) {
    this.controls = new TextLayoutControls(() => { this.layoutTouched = true; this.draftChanged(); });
    this.root.className = "text-editor";
    for (const event of ["pointerdown", "click", "dblclick"]) this.root.addEventListener(event, (e) => e.stopPropagation());
    this.root.style.cssText = "position:absolute;inset:0;pointer-events:none;z-index:12;overflow:hidden";
    this.toolbar.style.cssText = "position:absolute;top:8px;left:50%;transform:translateX(-50%);padding:8px;display:flex;align-items:center;gap:12px;background:Canvas;color:CanvasText;border:1px solid #888;border-radius:6px;pointer-events:auto;z-index:2";
    const title = document.createElement("span"); title.textContent = "Edit existing text: choose an outlined line";
    this.done = this.button("Done", () => {
      this.commit(); void this.settle().then(() => {
        // A tab transition may have removed this editor while its draft saved.
        if (this.disposed) return;
        this.close();
        host.focus({ preventScroll: true });
      }).catch(() => {});
    });
    this.toolbar.append(title, this.done);
    this.popup.className = "text-edit-popup";
    this.popup.setAttribute("role", "group");
    this.popup.setAttribute("aria-label", "Replace existing text");
    this.popup.style.cssText = "position:absolute;max-width:calc(100% - 16px);max-height:calc(100% - 64px);overflow:auto;width:440px;box-sizing:border-box;padding:12px;background:Canvas;color:CanvasText;border:1px solid #888;border-radius:6px;pointer-events:auto;box-shadow:0 5px 24px #0004;z-index:3";
    this.popup.hidden = true;
    const label = document.createElement("label"); label.textContent = "Replacement text";
    this.input.rows = 2; this.input.maxLength = 8192; this.input.style.cssText = "display:block;width:100%;box-sizing:border-box;margin:8px 0;resize:vertical";
    label.append(this.input);
    const help = document.createElement("p"); help.textContent = "Longer text grows into the room after the line by itself; set a width to size the box yourself. Apply updates the page; Save writes the PDF. With wrapping, Ctrl+Enter applies.";
    help.style.cssText = "font-size:12px;margin:6px 0";
    this.message.setAttribute("role", "alert"); this.message.tabIndex = 0;
    this.message.style.cssText = "font-size:12px;margin:6px 0";
    this.apply.type = "button"; this.apply.className = "text-edit-apply"; this.apply.textContent = "Apply";
    this.apply.addEventListener("click", () => this.commit());
    this.previewImage.alt = "PDF preview of the replacement"; this.previewImage.hidden = true;
    this.previewImage.style.cssText = "max-width:100%;max-height:140px;object-fit:contain;background:white;margin:8px 0";
    this.previewImage.addEventListener("load", () => { if (!this.disposed) this.layout(); });
    this.popup.append(label, this.controls.root, help, this.previewImage, this.message, this.button("Cancel", () => this.cancel()), this.apply);
    this.popup.addEventListener("keydown", (event) => {
      event.stopPropagation();
      // Buttons own their native Enter activation; intercepting Cancel here
      // would apply the very draft the reader is trying to discard.
      if (event.key === "Enter" && event.target === this.input && !event.isComposing && (!this.controls.wrap.checked || event.ctrlKey || event.metaKey)) { event.preventDefault(); this.commit(); }
      if (event.key === "Escape") { event.preventDefault(); this.cancel(); }
    });
    this.input.addEventListener("input", () => this.draftChanged());
    for (const run of source.runs) {
      const button = this.button(`Edit: ${run.text || "empty text"}`, () => this.select(run));
      button.textContent = ""; button.className = "text-edit-run";
      button.style.cssText = "position:absolute;padding:0;border:1px solid #2874d0;background:#2874d012;pointer-events:auto;cursor:text";
      this.buttons.push(button); this.root.append(button);
    }
    this.outline.style.cssText = "position:absolute;border:2px dashed #2874d0;box-sizing:border-box;pointer-events:none";
    this.outline.hidden = true;
    this.root.append(this.outline, this.toolbar, this.popup); host.append(this.root); this.layout();
    (this.buttons.find((button) => !button.hidden) ?? this.done).focus({ preventScroll: true });
  }

  private button(title: string, action: () => void): HTMLButtonElement {
    const button = document.createElement("button"); button.type = "button"; button.textContent = title;
    button.setAttribute("aria-label", title); button.addEventListener("click", action); return button;
  }
  /** The pending replacement on one of this page's runs, or the source text.
   *
   * Matched on the file too: this page's number is a number of whichever
   * document it is drawn from, and the opened file's page of the same number
   * can carry a replacement of its own. */
  private pendingOn(run: TextRun): TextChange | undefined {
    return this.changes.find((change) => sameTextPage(change, this.source) && change.operator === run.operator);
  }
  private value(run: TextRun): string {
    return this.pendingOn(run)?.replacement ?? run.text;
  }
  private select(run: TextRun): void {
    if (this.busy || this.saving || this.disposed) return;
    // Finish the previous draft before moving its input to another source address.
    if (this.active && this.dirtyDraft()) {
      this.commit(); void this.settle().then(() => this.select(run)).catch(() => {}); return;
    }
    this.active = run; this.accepted = this.value(run); this.input.value = this.accepted;
    const previous = this.pendingOn(run);
    this.layoutTouched = previous?.layout !== undefined;
    this.controls.set(previous?.layout ?? defaultTextLayout(run));
    this.acceptedLayout = JSON.stringify(this.controls.read());
    this.clearPreview();
    this.failure = null; this.message.textContent = ""; this.popup.hidden = false;
    // Focusing a low target must not scroll the overflow-hidden overlay away
    // from the PDF surface; viewer geometry owns its position.
    this.layout(); this.input.focus({ preventScroll: true }); this.input.select();
  }
  private cancel(): void {
    if (this.saving) return;
    const selected = this.active ? this.buttons[this.source.runs.indexOf(this.active)] : null;
    const target = selected?.hidden ? this.done : selected;
    this.active = null; this.failure = null; this.popup.hidden = true;
    this.clearPreview();
    // A hidden input cannot retain keyboard navigation; keep the PDF in place.
    target?.focus({ preventScroll: true });
  }
  update(state: EditState): void {
    if (!state.pages.some((page) => page.id === this.pageId)) { this.close(); return; }
    this.changes = state.text_edits ?? [];
    if (this.active && !this.saving && !this.dirtyDraft()) {
      this.accepted = this.value(this.active); this.input.value = this.accepted;
      const previous = this.active ? this.pendingOn(this.active) : undefined;
      this.layoutTouched = previous?.layout !== undefined;
      this.controls.set(previous?.layout ?? defaultTextLayout(this.active));
      this.acceptedLayout = JSON.stringify(this.controls.read());
      this.clearPreview();
    }
    this.buttons.forEach((button, index) => button.setAttribute("aria-label", `Edit: ${this.value(this.source.runs[index]!) || "empty text"}`));
    this.layout();
  }
  setBusy(busy: boolean): void {
    this.busy = busy;
    this.root.querySelectorAll("button, input, textarea, select").forEach((node) => {
      (node as HTMLButtonElement | HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement).disabled = busy || this.saving;
    });
  }
  commit(): void {
    if (!this.active || this.disposed || this.saving || !this.dirtyDraft()) return;
    const replacement = this.input.value;
    const error = replacementError(replacement, this.controls.wrap.checked) ?? this.controls.error();
    if (error) { this.failure = new Error(error); this.message.textContent = error; return; }
    const run = this.active;
    const change: TextChange = this.draft(run);
    this.clearPreview();
    this.saving = true; this.failure = null; this.setBusy(this.busy);
    // Start synchronously: the application's popup-drain permission is scoped to this call.
    this.pending = this.write(change).then((state) => {
      if (this.disposed) return;
      this.changes = state.text_edits ?? []; this.accepted = replacement;
      this.acceptedLayout = JSON.stringify(this.controls.read());
      this.message.textContent = "Applied. Save to write the PDF.";
    }).catch((error: unknown) => {
      if (!this.disposed) { this.failure = error; this.message.textContent = String(error); }
    }).finally(() => { this.saving = false; if (!this.disposed) this.setBusy(this.busy); });
  }
  async settle(): Promise<void> { await this.pending; if (this.failure) throw this.failure; }
  private dirtyDraft(): boolean { return this.input.value !== this.accepted || JSON.stringify(this.controls.read()) !== this.acceptedLayout; }
  private draft(run: TextRun): TextChange {
    return { page: this.source.page, revision: this.source.revision, operator: run.operator, original: run.text, replacement: this.input.value,
      ...((this.preview || this.layoutTouched) ? { layout: this.controls.read() } : {}) };
  }
  private clearPreview(): void {
    clearTimeout(this.previewTimer); this.previewGeneration++;
    if (this.previewUrl) URL.revokeObjectURL(this.previewUrl);
    this.previewUrl = null; this.previewImage.hidden = true; this.previewImage.removeAttribute("src");
    this.previewRect = null; this.outline.hidden = true;
  }
  private draftChanged(): void {
    this.clearPreview(); this.failure = null; this.message.textContent = "";
    if (!this.active || !this.preview || !this.dirtyDraft() || this.busy || this.saving) return;
    const error = replacementError(this.input.value, this.controls.wrap.checked) ?? this.controls.error();
    if (error) { this.message.textContent = error; return; }
    const change = this.draft(this.active), generation = this.previewGeneration;
    this.message.textContent = "Preparing preview...";
    this.previewTimer = setTimeout(() => {
      if (this.disposed || generation !== this.previewGeneration) return;
      void this.preview!(change).then((result) => {
        if (this.disposed || generation !== this.previewGeneration || !result.preview) return;
        const preview = result.preview;
        this.previewUrl = URL.createObjectURL(new Blob([new Uint8Array(preview.png)], { type: "image/png" }));
        // webview-sink-ok: URL.createObjectURL above receives only a local image/png Blob; no document URL is used.
        this.previewImage.src = this.previewUrl; this.previewImage.hidden = false;
        this.previewRect = preview.rect;
        this.message.textContent = `Preview: ${preview.font}, ${preview.lines} ${preview.lines === 1 ? "line" : "lines"}.`;
        this.layout();
      }).catch((error: unknown) => {
        if (!this.disposed && generation === this.previewGeneration) this.message.textContent = String(error);
      });
    }, 250);
  }
  layout(): void {
    const width = this.root.clientWidth, height = this.root.clientHeight;
    this.source.runs.forEach((run, index) => {
      const button = this.buttons[index]!; const box = this.anchor(run);
      // Clipped targets must also leave the keyboard navigation order.
      button.hidden = !run.text || !box || box.right <= box.left || box.bottom <= box.top || box.right <= 0 || box.bottom <= 0 || box.left >= width || box.top >= height;
      if (box) Object.assign(button.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${Math.max(12, box.right-box.left)}px`, height: `${Math.max(12, box.bottom-box.top)}px`, clipPath: box.clip });
    });
    if (this.active && !this.popup.hidden) {
      const area = this.previewRect && this.anchor({ ...this.active, display_rect: this.previewRect });
      this.outline.hidden = !area;
      if (area) Object.assign(this.outline.style, { left: `${area.left}px`, top: `${area.top}px`, width: `${area.right-area.left}px`, height: `${area.bottom-area.top}px`, clipPath: area.clip });
      const box = this.anchor(this.active);
      this.popup.style.left = `${Math.max(8, Math.min(box?.left ?? 8, width - 456))}px`;
      this.popup.style.top = `${Math.max(48, Math.min((box?.bottom ?? 48) + 8, height - this.popup.offsetHeight - 8))}px`;
    }
  }
  destroy(): void { this.disposed = true; this.clearPreview(); this.root.remove(); }
}
