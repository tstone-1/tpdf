import type { EditState } from "./edits";
import type { Anchor } from "./popup";

/** Mirrors the worker's AcroForm reply. Object ids name shared fields. */
export interface FormWidget {
  object: [number, number];
  widget: [number, number];
  page: number;
  rect: [number, number, number, number];
  display_rect: [number, number, number, number];
  name: string;
  value: string | boolean;
  multiline: boolean;
  max_length: number | null;
  reason: string | null;
}
export interface Form { widgets: FormWidget[] }
export interface FormChange { object: [number, number]; value: string | boolean }

export function fieldKey(object: readonly [number, number]): string { return object.join(":"); }

/** Distinguishes clearing a value from leaving it unchanged, including undo. */
export function fieldValue(widget: FormWidget, changes: readonly FormChange[]): string | boolean {
  return changes.find((change) => fieldKey(change.object) === fieldKey(widget.object))?.value ?? widget.value;
}

/** Reports unsupported input before a tab switch or save can close its editor. */
export function answerError(widget: FormWidget, value: string | boolean): string | null {
  if (widget.reason) return widget.reason;
  if (typeof value !== typeof widget.value) return "The answer does not match this field.";
  if (typeof value === "boolean") return null;
  if (new TextEncoder().encode(value).length > 16384) return "A form answer is limited to 16 KB.";
  if (widget.max_length !== null && [...value].length > widget.max_length) return "This answer exceeds the field's maximum length.";
  if (!/^[\x20-\x7e\xa0-\xff\n]*$/.test(value)) return "This field supports Western European characters only.";
  if (!widget.multiline && /[\r\n]/.test(value)) return "This field accepts one line only.";
  return null;
}

interface Control { widget: FormWidget; input: HTMLInputElement | HTMLTextAreaElement; accepted: string | boolean; pending: number }

/** Native page controls. Editing commits once on blur; pending text survives repaint. */
export class FormLayer {
  private readonly node = document.createElement("div");
  private readonly controls: Control[] = [];
  private changes: readonly FormChange[] = [];
  private disposed = false;
  private readonly pending = new Set<Promise<void>>();

  constructor(host: HTMLElement, form: Form,
    private readonly anchor: (widget: FormWidget) => (Anchor & { clip?: string; scale?: number }) | null,
    private readonly change: (object: [number, number], value: string | boolean) => Promise<void>,
    private readonly reveal: (widget: FormWidget) => void,
    private readonly error: (message: string) => void,
  ) {
    this.node.className = "form-fields";
    this.node.style.cssText = "position:absolute;inset:0;pointer-events:none;overflow:hidden;z-index:3";
    host.append(this.node);
    for (const widget of form.widgets) {
      if (widget.reason) continue;
      const input = widget.multiline ? document.createElement("textarea") : document.createElement("input");
      if (input instanceof HTMLInputElement) input.type = typeof widget.value === "boolean" ? "checkbox" : "text";
      input.setAttribute("aria-label", widget.name || "Form field");
      input.dataset.field = fieldKey(widget.object);
      input.autocomplete = "off";
      input.spellcheck = false;
      input.disabled = widget.reason !== null;
      input.title = widget.reason ?? widget.name;
      input.style.cssText = "position:absolute;box-sizing:border-box;margin:0;pointer-events:auto;border:1px solid #4674be88;border-radius:1px;background:#f4f7ff;color:#171717;padding:2px;font:12px Helvetica,Arial,sans-serif;resize:none;min-width:0;min-height:0";
      const control = { widget, input, accepted: widget.value, pending: 0 };
      this.controls.push(control);
      this.put(control, widget.value);
      input.addEventListener("pointerdown", (event) => event.stopPropagation());
      input.addEventListener("keydown", (raw) => {
        const event = raw as KeyboardEvent;
        // Save and application undo retain their usual meaning. Ordinary typing
        // must never reach the viewer's page-navigation shortcuts.
        if (!(event.metaKey || event.ctrlKey)) event.stopPropagation();
        if (event.key === "Tab") {
          const at = this.controls.indexOf(control);
          const next = this.controls[at + (event.shiftKey ? -1 : 1)];
          if (next) {
            event.preventDefault();
            this.reveal(next.widget); this.layout(); next.input.focus({ preventScroll: true });
          }
        }
        if (event.key === "Escape") { input.blur(); event.preventDefault(); }
      });
      input.addEventListener("blur", () => { if (!this.disposed) this.commitOne(control, false); });
      input.addEventListener("change", () => { if (typeof widget.value === "boolean") this.commitOne(control, false); });
      this.node.append(input);
    }
    this.layout();
  }

  private read(control: Control): string | boolean {
    return typeof control.widget.value === "boolean" ? (control.input as HTMLInputElement).checked : control.input.value;
  }

  private put(control: Control, value: string | boolean): void {
    if (typeof value === "boolean") (control.input as HTMLInputElement).checked = value;
    else control.input.value = value;
    control.accepted = value;
  }

  private commitOne(control: Control, throwing: boolean): void {
    const value = this.read(control);
    if (value === control.accepted) { control.input.setCustomValidity(""); return; }
    const error = answerError(control.widget, value);
    if (error) {
      control.input.setCustomValidity(error);
      this.error(error);
      if (throwing) throw new Error(error);
      return;
    }
    control.input.setCustomValidity("");
    const previous = control.accepted;
    control.accepted = value;
    control.pending += 1;
    const task = this.change(control.widget.object, value).then(() => {
      control.pending -= 1;
      if (this.disposed) return;
      if (!control.pending && fieldValue(control.widget, this.changes) !== value) {
        control.accepted = previous;
        control.input.setCustomValidity("This answer was refused. Correct it before saving or switching documents.");
      }
      this.update({ forms: [...this.changes] } as EditState);
    }).finally(() => { this.pending.delete(task); });
    this.pending.add(task);
  }

  /** Flush before save, switching tabs, or closing the document. */
  commit(): void { for (const control of this.controls) this.commitOne(control, true); }

  /** Freeze editing during file writes without blurring an uncommitted draft. */
  setBusy(busy: boolean): void {
    for (const { widget, input } of this.controls) {
      if (typeof widget.value === "boolean") input.disabled = busy;
      else input.readOnly = busy;
    }
  }

  /** Wait for validation before any operation can discard the mounted controls. */
  async settle(): Promise<void> {
    await Promise.all([...this.pending]);
    const invalid = this.controls.find((control) => control.input.validationMessage);
    if (invalid) throw new Error(invalid.input.validationMessage);
  }

  /** Backend state is authoritative after undo, redo and each completed edit. */
  update(state: EditState): void {
    this.changes = state.forms ?? [];
    for (const control of this.controls) {
      if (!control.pending && this.read(control) === control.accepted && !control.input.validationMessage)
        this.put(control, fieldValue(control.widget, this.changes));
    }
    this.layout();
  }

  /** Uses the viewer's crop and rotation mapping; no second page layout. */
  layout(): void {
    const height = this.node.clientHeight;
    for (const control of this.controls) {
      const box = this.anchor(control.widget);
      const visible = box && box.bottom >= 0 && box.top <= height;
      const display = visible ? "block" : "none";
      if (control.input.style.display !== display) control.input.style.display = display;
      if (!box || !visible) continue;
      Object.assign(control.input.style, { fontSize: `${12 * (box.scale ?? 1)}px`, padding: `${2 * (box.scale ?? 1)}px`, clipPath: box.clip ?? "none", left: `${box.left}px`, top: `${box.top}px`, width: `${box.right - box.left}px`, height: `${box.bottom - box.top}px` });
    }
  }

  /** Palette access also brings an off-screen field into view. */
  focus(): void {
    const first = this.controls[0];
    if (!first) { this.error("This document has no supported editable form fields."); return; }
    this.reveal(first.widget); this.layout(); first.input.focus({ preventScroll: true });
  }

  destroy(): void { this.disposed = true; this.node.remove(); }
}
