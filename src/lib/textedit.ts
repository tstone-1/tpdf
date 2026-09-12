import type { EditState } from "./edits";

/** Worker-inspected source addresses; these always refer to the original PDF. */
export interface TextRun {
  operator: number;
  text: string;
  font: string;
  size: number;
  matrix: [number, number, number, number, number, number];
  advance: number;
  display_rect: [number, number, number, number];
}
export interface TextRuns { page: number; revision: number[]; runs: TextRun[] }
export interface TextChange { page: number; revision: number[]; operator: number; original: string; replacement: string }

/** Compare per-page bodies, including edits removed by undo. */
export function changedTextPages(before: readonly TextChange[], after: readonly TextChange[]): number[] {
  const group = (changes: readonly TextChange[]) => {
    const pages = new Map<number, string[]>();
    for (const change of changes) {
      const entries = pages.get(change.page) ?? [];
      entries.push(JSON.stringify(change)); pages.set(change.page, entries);
    }
    return new Map([...pages].map(([page, entries]) => [page, JSON.stringify(entries.sort())]));
  };
  const old = group(before), next = group(after);
  return [...new Set([...old.keys(), ...next.keys()])].filter((page) => old.get(page) !== next.get(page));
}

export function replacementError(value: string): string | null {
  return value.length > 4096 || /[^\x20-\x7e]/.test(value)
    ? "Use at most 4096 printable English characters. Accents and line breaks are not supported yet."
    : null;
}

type Anchor = { left: number; top: number; right: number; bottom: number; clip: string };

/** Hit targets use viewer geometry; the worker renders the accepted replacement. */
export class TextEditor {
  private readonly root = document.createElement("div");
  private readonly toolbar = document.createElement("div");
  private readonly popup = document.createElement("div");
  private readonly input = document.createElement("input");
  private readonly message = document.createElement("p");
  private readonly apply = document.createElement("button");
  private readonly buttons: HTMLButtonElement[] = [];
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
    private readonly close: () => void) {
    this.root.className = "text-editor";
    for (const event of ["pointerdown", "click", "dblclick"]) this.root.addEventListener(event, (e) => e.stopPropagation());
    this.root.style.cssText = "position:absolute;inset:0;pointer-events:none;z-index:12;overflow:hidden";
    this.toolbar.style.cssText = "position:absolute;top:8px;left:50%;transform:translateX(-50%);padding:8px;display:flex;align-items:center;gap:12px;background:Canvas;color:CanvasText;border:1px solid #888;border-radius:6px;pointer-events:auto;z-index:2";
    const title = document.createElement("span"); title.textContent = "Edit existing text: choose an outlined line";
    this.toolbar.append(title, this.button("Done", () => {
      this.commit(); void this.settle().then(() => this.close()).catch(() => {});
    }));
    this.popup.className = "text-edit-popup";
    this.popup.setAttribute("role", "group");
    this.popup.setAttribute("aria-label", "Replace existing text");
    this.popup.style.cssText = "position:absolute;max-width:calc(100% - 16px);width:440px;box-sizing:border-box;padding:12px;background:Canvas;color:CanvasText;border:1px solid #888;border-radius:6px;pointer-events:auto;box-shadow:0 5px 24px #0004;z-index:3";
    this.popup.hidden = true;
    const label = document.createElement("label"); label.textContent = "Replacement text";
    this.input.type = "text"; this.input.maxLength = 4096; this.input.style.cssText = "display:block;width:100%;box-sizing:border-box;margin:8px 0";
    label.append(this.input);
    const help = document.createElement("p"); help.textContent = "The replacement must fit within the original width. Apply updates the page; save writes the PDF.";
    help.style.cssText = "font-size:12px;margin:6px 0";
    this.message.setAttribute("role", "alert"); this.message.style.cssText = "font-size:12px;margin:6px 0";
    this.apply.type = "button"; this.apply.className = "text-edit-apply"; this.apply.textContent = "Apply";
    this.apply.addEventListener("click", () => this.commit());
    this.popup.append(label, help, this.message, this.button("Cancel", () => this.cancel()), this.apply);
    this.popup.addEventListener("keydown", (event) => {
      event.stopPropagation();
      if (event.key === "Enter" && !event.isComposing) { event.preventDefault(); this.commit(); }
      if (event.key === "Escape") { event.preventDefault(); this.cancel(); }
    });
    this.input.addEventListener("input", () => { this.failure = null; this.message.textContent = ""; });
    for (const run of source.runs) {
      const button = this.button(`Edit: ${run.text || "empty text"}`, () => this.select(run));
      button.textContent = ""; button.className = "text-edit-run";
      button.style.cssText = "position:absolute;padding:0;border:1px solid #2874d0;background:#2874d012;pointer-events:auto;cursor:text";
      this.buttons.push(button); this.root.append(button);
    }
    this.root.append(this.toolbar, this.popup); host.append(this.root); this.layout();
    this.buttons[0]?.focus();
  }

  private button(title: string, action: () => void): HTMLButtonElement {
    const button = document.createElement("button"); button.type = "button"; button.textContent = title;
    button.setAttribute("aria-label", title); button.addEventListener("click", action); return button;
  }
  private value(run: TextRun): string {
    return this.changes.find((change) => change.page === this.source.page && change.operator === run.operator)?.replacement ?? run.text;
  }
  private select(run: TextRun): void {
    if (this.busy || this.saving || this.disposed) return;
    // Finish the previous draft before moving its input to another source address.
    if (this.active && this.input.value !== this.accepted) {
      this.commit(); void this.settle().then(() => this.select(run)).catch(() => {}); return;
    }
    this.active = run; this.accepted = this.value(run); this.input.value = this.accepted;
    this.failure = null; this.message.textContent = ""; this.popup.hidden = false;
    this.layout(); this.input.focus(); this.input.select();
  }
  private cancel(): void {
    if (this.saving) return;
    this.active = null; this.failure = null; this.popup.hidden = true;
  }
  update(state: EditState): void {
    if (!state.pages.some((page) => page.id === this.pageId)) { this.close(); return; }
    this.changes = state.text_edits ?? [];
    if (this.active && !this.saving && this.input.value === this.accepted) {
      this.accepted = this.value(this.active); this.input.value = this.accepted;
    }
    this.buttons.forEach((button, index) => button.setAttribute("aria-label", `Edit: ${this.value(this.source.runs[index]!) || "empty text"}`));
    this.layout();
  }
  setBusy(busy: boolean): void {
    this.busy = busy;
    this.root.querySelectorAll("button, input").forEach((node) => {
      (node as HTMLButtonElement | HTMLInputElement).disabled = busy || this.saving;
    });
  }
  commit(): void {
    if (!this.active || this.disposed || this.saving || this.input.value === this.accepted) return;
    const replacement = this.input.value;
    const error = replacementError(replacement);
    if (error) { this.failure = new Error(error); this.message.textContent = error; return; }
    const run = this.active;
    const change: TextChange = { page: this.source.page, revision: this.source.revision, operator: run.operator, original: run.text, replacement };
    this.saving = true; this.failure = null; this.setBusy(this.busy);
    // Start synchronously: the application's popup-drain permission is scoped to this call.
    this.pending = this.write(change).then((state) => {
      if (this.disposed) return;
      this.changes = state.text_edits ?? []; this.accepted = replacement;
      this.message.textContent = "Applied. Save to write the PDF.";
    }).catch((error: unknown) => {
      if (!this.disposed) { this.failure = error; this.message.textContent = String(error); }
    }).finally(() => { this.saving = false; if (!this.disposed) this.setBusy(this.busy); });
  }
  async settle(): Promise<void> { await this.pending; if (this.failure) throw this.failure; }
  layout(): void {
    this.source.runs.forEach((run, index) => {
      const button = this.buttons[index]!; const box = this.anchor(run);
      button.hidden = !box;
      if (box) Object.assign(button.style, { left: `${box.left}px`, top: `${box.top}px`, width: `${Math.max(12, box.right-box.left)}px`, height: `${Math.max(12, box.bottom-box.top)}px`, clipPath: box.clip });
    });
    if (this.active && !this.popup.hidden) {
      const box = this.anchor(this.active);
      const width = this.root.clientWidth, height = this.root.clientHeight;
      this.popup.style.left = `${Math.max(8, Math.min(box?.left ?? 8, width - 456))}px`;
      this.popup.style.top = `${Math.max(48, Math.min((box?.bottom ?? 48) + 8, height - this.popup.offsetHeight - 8))}px`;
    }
  }
  destroy(): void { this.disposed = true; this.root.remove(); }
}
