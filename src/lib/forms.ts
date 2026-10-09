import type { EditState } from "./edits";
import type { Anchor } from "./popup";
import type { FieldEdited, FieldProps } from "./savedfields";
import { isEmptySignature } from "./signfield";

/** Option indices preserve choices whose export values happen to be equal. */
export type FormValue = string | boolean | number[];
export type FormControl = { kind: "text" | "checkbox" | "unsupported" }
  /** A signature field: a place for a signature while `signed` is false. */
  | { kind: "signature"; signed: boolean }
  | { kind: "radio"; index: number; states: number[][]; unison: boolean; no_toggle_off: boolean }
  | { kind: "choice"; options: { export: string; label: string }[]; combo: boolean; editable: boolean; multiple: boolean };

/** Mirrors the worker's AcroForm reply. Object ids name shared fields. */
export interface FormWidget {
  object: [number, number];
  widget: [number, number];
  page: number;
  rect: [number, number, number, number];
  display_rect: [number, number, number, number];
  name: string;
  value: FormValue;
  control: FormControl;
  multiline: boolean;
  max_length: number | null;
  reason: string | null;
  /** The field's `/TU`; empty for none. */
  tooltip?: string;
  required?: boolean;
  read_only?: boolean;
  align?: FormAlign;
  /** The text size the field declares, in points; `null` for one that follows its height. */
  text_size?: number | null;
  /** What a text field holds after a reset; empty for none. */
  default_value?: string;
  /** Quarter turns the field declares its drawing is turned by, as its page is. */
  turns?: number;
}
/** Where a field's text sits between its left and right edges. */
export type FormAlign = "left" | "center" | "right";
export interface Form { widgets: FormWidget[] }
export interface FormChange { object: [number, number]; value: FormValue }

export function fieldKey(object: readonly [number, number]): string { return object.join(":"); }

/** Distinguishes clearing a value from leaving it unchanged, including undo. */
export function fieldValue(widget: FormWidget, changes: readonly FormChange[]): FormValue {
  return changes.find((change) => fieldKey(change.object) === fieldKey(widget.object))?.value ?? widget.value;
}

/**
 * What a field's control shows: its pending answer or the file's, and for a
 * text field that would hold nothing, the default value a reader has just
 * given it. The save answers such a field with the default, so the control
 * says so before it.
 *
 * `widgets` are the form's, because a field's properties are held under one
 * of its widgets and that need not be this one. It may be one a reader has
 * removed: the field keeps what was set under it.
 */
export function shownValue(
  widget: FormWidget,
  changes: readonly FormChange[],
  fields: readonly FieldEdited[],
  widgets: readonly FormWidget[],
): FormValue {
  const value = fieldValue(widget, changes);
  if (widget.control.kind === "choice") {
    const choices = shownChoices(widget, fields, widgets);
    return Array.isArray(value) && choices ? standingChoice(widget.control.options, choices, value) : value;
  }
  if (value !== "" || widget.control.kind !== "text") return value;
  return given(widget, fields, widgets, "default_value") ?? "";
}

/** One part of the properties a reader has given a field, held under any of its widgets. */
function given<K extends keyof FieldProps>(
  widget: FormWidget,
  fields: readonly FieldEdited[],
  widgets: readonly FormWidget[],
  part: K,
): FieldProps[K] | undefined {
  const own = widgets.filter((other) => fieldKey(other.object) === fieldKey(widget.object));
  return fields
    .filter((edit) => own.some((other) => fieldKey(other.widget) === fieldKey(edit.object)))
    .map((edit) => edit.props?.[part])
    .find((value) => value !== undefined);
}

/**
 * One choice a list offers. `index` is its place among the file's choices,
 * which is what an answer names until the save; `null` for a choice a reader
 * has just added, which the file does not have yet and so cannot be chosen.
 */
export interface ShownChoice { label: string; index: number | null }

/**
 * The choices a reader has just given a list, or `null` when its choices are
 * the file's. A choice whose label the file has takes the first place the
 * file has that label at: the save carries what is chosen from the old
 * choices to the new ones by label, and so does this.
 */
export function shownChoices(
  widget: FormWidget,
  fields: readonly FieldEdited[],
  widgets: readonly FormWidget[],
): ShownChoice[] | null {
  if (widget.control.kind !== "choice") return null;
  const to = given(widget, fields, widgets, "options");
  if (to === undefined) return null;
  const was = widget.control.options.map((option) => option.label);
  return to.map((label) => {
    const index = was.indexOf(label);
    return { label, index: index < 0 ? null : index };
  });
}

/**
 * What stays chosen under new choices: each chosen choice whose label the new
 * choices still have, at the place a control offers that label at. The rest
 * the save takes off, and the control says so before it.
 */
export function standingChoice(
  was: readonly { label: string }[],
  choices: readonly ShownChoice[],
  value: readonly number[],
): number[] {
  const kept = value
    .map((at) => choices.find((choice) => choice.label === was[at]?.label)?.index)
    .filter((at): at is number => typeof at === "number");
  return [...new Set(kept)].sort((a, b) => a - b);
}

/** Where a field's text sits: as a reader has just set it, or as the file has it. */
export function shownAlign(
  widget: FormWidget,
  fields: readonly FieldEdited[],
  widgets: readonly FormWidget[],
): FormAlign {
  return given(widget, fields, widgets, "align") ?? widget.align ?? "left";
}

/** Reports unsupported input before a tab switch or save can close its editor. */
export function answerError(widget: FormWidget, value: FormValue): string | null {
  if (widget.reason) return widget.reason;
  const control = widget.control;
  if (Array.isArray(value)) {
    if (control.kind !== "radio" && control.kind !== "choice") return "The answer does not match this field.";
    const length = control.kind === "radio" ? control.states.length : control.options.length;
    if (value.some((i, at) => !Number.isInteger(i) || i < 0 || i >= length || (at > 0 && i <= value[at - 1]!))) return "Choose available options without duplicates.";
    if ((control.kind === "radio" || !control.multiple || control.combo) && value.length > 1) return "Choose one option.";
    if (control.kind === "radio" && control.no_toggle_off && !value.length) return "Choose one radio button.";
    return null;
  }
  if (control.kind === "radio" || (control.kind === "choice" && (!control.editable || typeof value !== "string")) || control.kind === "unsupported") return "The answer does not match this field.";
  if (control.kind !== "choice" && typeof value !== typeof widget.value) return "The answer does not match this field.";
  if (typeof value === "boolean") return null;
  if (new TextEncoder().encode(value).length > 16384) return "A form answer is limited to 16 KB.";
  if (widget.max_length !== null && [...value].length > widget.max_length) return "This answer exceeds the field's maximum length.";
  if (!/^[\x20-\x7e\xa0-\xff\n]*$/.test(value)) return "This field supports Western European characters only.";
  if (!widget.multiline && /[\r\n]/.test(value)) return "This field accepts one line only.";
  return null;
}

interface Mounted {
  widget: FormWidget;
  input: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;
  accepted: FormValue;
  pending: number;
  /** A list's choices as the control offers them, and the element that holds them. */
  choices: ShownChoice[];
  list: HTMLElement | null;
}

/** Array identity changes at every IPC reply; equality is about the selected indices. */
export function sameAnswer(a: FormValue, b: FormValue): boolean {
  return Array.isArray(a) && Array.isArray(b) ? a.length === b.length && a.every((v, i) => v === b[i]) : a === b;
}

/**
 * Where Tab takes the keyboard from the control at `at`, going `step` along
 * the controls.
 *
 * The next control that can take it, and not simply the next: one whose field
 * is being changed has no place on the page and is put away, and a control
 * that is put away or withheld takes no keyboard, so a Tab sent to it went
 * nowhere and said nothing. `placed` answers whether a control has a place at
 * all, without moving anything. `reach` brings one into view and answers
 * whether it can then take the keyboard; it is asked only of a control that is
 * placed, since it scrolls the page.
 *
 * `to` is the control, or null when none further on can take the keyboard.
 * `further` is whether there are controls further on at all: with none the
 * key is the web view's, which leaves the form as it does from any last
 * field, and with some that cannot be reached the keyboard stays where it is.
 * `moved` is whether `reach` was asked of a control that then could not take
 * it, which has scrolled the page away from the one the reader is in.
 */
export function tabStop<T>(
  controls: readonly T[],
  at: number,
  step: 1 | -1,
  placed: (control: T) => boolean,
  reach: (control: T) => boolean,
): { to: T | null; further: boolean; moved: boolean } {
  let further = false;
  let moved = false;
  if (at < 0) return { to: null, further, moved };
  for (let next = at + step; next >= 0 && next < controls.length; next += step) {
    const control = controls[next] as T;
    further = true;
    if (!placed(control)) continue;
    if (reach(control)) return { to: control, further, moved };
    moved = true;
  }
  return { to: null, further, moved };
}

/**
 * A radio group as an arrow key goes round it from the button at `at`: that
 * button first, then the others in the order the key reaches them, round the
 * end and back. {@link tabStop} then finds the first of them that is shown.
 */
export function ringFrom<T>(group: readonly T[], at: number, step: 1 | -1): T[] {
  const here = group[at];
  if (here === undefined) return [];
  const ahead = step === 1 ? [...group.slice(at + 1), ...group.slice(0, at)]
    : [...group.slice(0, at).reverse(), ...group.slice(at + 1).reverse()];
  return [here, ...ahead];
}

/** Native page controls. Editing commits once on blur; pending text survives repaint. */
export class FormLayer {
  private readonly node = document.createElement("div");
  private readonly controls: Mounted[] = [];
  /** A button over each empty signature field, which signs it. */
  private readonly places: { widget: FormWidget; button: HTMLButtonElement }[] = [];
  private changes: readonly FormChange[] = [];
  private fields: readonly FieldEdited[] = [];
  private disposed = false;
  private readonly pending = new Set<Promise<void>>();

  constructor(host: HTMLElement, form: Form,
    private readonly anchor: (widget: FormWidget) => (Anchor & { clip?: string; scale?: number }) | null,
    private readonly change: (object: [number, number], value: FormValue) => Promise<void>,
    private readonly reveal: (widget: FormWidget) => void,
    private readonly error: (message: string) => void,
    /** Signs an empty signature field. Without it such a field is offered nothing. */
    private readonly sign?: (widget: FormWidget) => void,
  ) {
    this.node.className = "form-fields";
    this.node.style.cssText = "position:absolute;inset:0;pointer-events:none;overflow:hidden;z-index:3";
    host.append(this.node);
    for (const widget of form.widgets) {
      // A place for a signature is pressed, not typed into.
      if (this.sign && isEmptySignature(widget)) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "form-sign";
        button.textContent = "Sign";
        button.setAttribute("aria-label", `Sign ${widget.name || "the signature field"}`);
        button.title = widget.tooltip || `Sign ${widget.name} with a certificate`;
        button.style.cssText = "position:absolute;box-sizing:border-box;margin:0;pointer-events:auto;cursor:pointer;border:1px dashed #4674be;border-radius:1px;background:#4674be1f;color:#27508f;font:12px Helvetica,Arial,sans-serif;min-width:0;min-height:0;overflow:hidden";
        button.addEventListener("pointerdown", (event) => event.stopPropagation());
        button.addEventListener("click", () => { if (!this.disposed) this.sign?.(widget); });
        this.places.push({ widget, button });
        this.node.append(button);
        continue;
      }
      if (widget.reason) continue;
      const kind = widget.control;
      const input = kind.kind === "choice" && !kind.editable ? document.createElement("select") : widget.multiline ? document.createElement("textarea") : document.createElement("input");
      if (input instanceof HTMLInputElement) {
        input.type = kind.kind === "radio" ? "radio" : kind.kind === "checkbox" ? "checkbox" : "text";
        if (kind.kind === "radio") input.name = `form-${fieldKey(widget.object)}${kind.unison ? `-${kind.index}` : ""}`;
      }
      let list: HTMLElement | null = null;
      if (kind.kind === "choice") {
        if (input instanceof HTMLSelectElement) {
          input.multiple = kind.multiple && !kind.combo;
          list = input;
        } else {
          list = document.createElement("datalist"); list.id = `form-options-${fieldKey(widget.widget)}`;
          input.setAttribute("list", list.id); this.node.append(list);
        }
      }
      input.setAttribute("aria-label", widget.name || "Form field");
      input.dataset.field = fieldKey(widget.object);
      input.autocomplete = "off";
      input.spellcheck = false;
      input.disabled = widget.reason !== null;
      input.title = widget.reason ?? (widget.tooltip || widget.name);
      if (widget.required) input.required = true;
      input.style.cssText = "position:absolute;box-sizing:border-box;margin:0;pointer-events:auto;border:1px solid #4674be88;border-radius:1px;background:#f4f7ff;color:#171717;padding:2px;font:12px Helvetica,Arial,sans-serif;resize:none;min-width:0;min-height:0";
      if (widget.align && widget.align !== "left") input.style.textAlign = widget.align;
      const control: Mounted = { widget, input, accepted: widget.value, pending: 0, choices: [], list };
      if (kind.kind === "choice") this.offer(control, kind.options.map((option, index) => ({ label: option.label, index })));
      this.controls.push(control);
      this.put(control, widget.value);
      input.addEventListener("pointerdown", (event) => event.stopPropagation());
      input.addEventListener("keydown", (raw) => {
        const event = raw as KeyboardEvent;
        // Save and application undo retain their usual meaning. Ordinary typing
        // must never reach the viewer's page-navigation shortcuts.
        if (!(event.metaKey || event.ctrlKey)) event.stopPropagation();
        if (kind.kind === "radio" && ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft"].includes(event.key)) {
          const group = this.controls.filter((c) => fieldKey(c.widget.object) === fieldKey(widget.object));
          const direction = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
          // The next button that is shown. An arrow key also answers, and it
          // must not answer with a button the reader cannot see.
          const ring = ringFrom(group, group.indexOf(control), direction);
          const stop = this.stopIn(ring, 0, 1);
          if (stop.to) { stop.to.input.focus({ preventScroll: true }); stop.to.input.click(); }
          if (!stop.to && stop.moved) { this.reveal(widget); this.layout(); }
          event.preventDefault();
        }
        if (event.key === "Tab") {
          const stop = this.stopIn(this.controls, this.controls.indexOf(control), event.shiftKey ? -1 : 1);
          if (stop.further) event.preventDefault();
          if (stop.to) stop.to.input.focus({ preventScroll: true });
          // Looking for one scrolled the page. Back to the control the
          // keyboard is still in, so the reader sees where they are.
          else if (stop.moved) { this.reveal(widget); this.layout(); }
        }
        if (event.key === "Escape") { input.blur(); event.preventDefault(); }
      });
      input.addEventListener("blur", () => { if (!this.disposed) this.commitOne(control, false); });
      input.addEventListener("change", () => { if (kind.kind !== "text") this.commitOne(control, false); });
      this.node.append(input);
    }
    this.layout();
  }

  /**
   * The next control in `order` that can take the keyboard: one with a place
   * on the page, shown once the page has been brought to it, and not
   * withheld. Tab and a radio group's arrow keys both ask here.
   */
  private stopIn(order: readonly Mounted[], at: number, step: 1 | -1) {
    return tabStop(order, at, step,
      (one) => this.anchor(one.widget) !== null,
      (one) => { this.reveal(one.widget); this.layout(); return one.input.style.display !== "none" && !one.input.disabled; });
  }

  /**
   * Fills a list with the choices it offers. One a reader has just added is
   * shown and cannot be chosen: an answer names a choice the file has.
   */
  private offer(control: Mounted, choices: ShownChoice[]): void {
    const kind = control.widget.control;
    if (kind.kind !== "choice" || !control.list) return;
    control.choices = choices;
    control.list.replaceChildren();
    if (control.input instanceof HTMLSelectElement) {
      if (!kind.combo) control.input.size = Math.max(2, Math.min(8, choices.length));
      if (kind.combo) {
        const empty = document.createElement("option"); empty.value = ""; empty.textContent = "Choose an option"; control.list.append(empty);
      }
    }
    for (const choice of choices) {
      const item = document.createElement("option");
      if (control.input instanceof HTMLSelectElement) {
        item.textContent = choice.label;
        item.value = choice.index === null ? "new" : String(choice.index);
        if (choice.index === null) { item.disabled = true; item.title = "Save the document to choose this."; }
      } else item.value = choice.label;
      control.list.append(item);
    }
  }

  private read(control: Mounted): FormValue {
    const kind = control.widget.control;
    if (kind.kind === "radio") {
      if (!(control.input as HTMLInputElement).checked) return control.accepted;
      if (Array.isArray(control.accepted) && kind.unison && control.accepted.some((i) => sameAnswer(kind.states[i]!, kind.states[kind.index]!))) return control.accepted;
      return [kind.index];
    }
    if (control.input instanceof HTMLSelectElement) return [...control.input.selectedOptions].filter((o) => o.value !== "").map((o) => Number(o.value));
    if (kind.kind === "choice" && kind.editable) {
      if (control.input.value === "") return [];
      if (typeof control.accepted === "string" && control.input.value === control.accepted) return control.accepted;
      if (Array.isArray(control.accepted) && control.accepted.length === 1 && control.input.value === kind.options[control.accepted[0]!]!.label) return control.accepted;
      const index = control.choices.find((choice) => choice.label === control.input.value)?.index ?? null;
      return index === null ? control.input.value : [index];
    }
    return kind.kind === "checkbox" ? (control.input as HTMLInputElement).checked : control.input.value;
  }

  private put(control: Mounted, value: FormValue): void {
    const kind = control.widget.control;
    if (kind.kind === "radio" && Array.isArray(value)) {
      (control.input as HTMLInputElement).checked = value.some((i) => i === kind.index || (kind.unison && sameAnswer(kind.states[i]!, kind.states[kind.index]!)));
    } else if (control.input instanceof HTMLSelectElement && Array.isArray(value)) {
      for (const option of control.input.options) option.selected = option.value === "" ? value.length === 0 : value.includes(Number(option.value));
    } else if (typeof value === "boolean") (control.input as HTMLInputElement).checked = value;
    else if (Array.isArray(value) && kind.kind === "choice") control.input.value = value.map((i) => kind.options[i]!.label).join(", ");
    else if (typeof value === "string") control.input.value = value;
    control.accepted = value;
  }

  private commitOne(control: Mounted, throwing: boolean): void {
    const value = this.read(control);
    if (sameAnswer(value, control.accepted)) { control.input.setCustomValidity(""); return; }
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
      if (!control.pending && !sameAnswer(fieldValue(control.widget, this.changes), value)) {
        control.accepted = previous;
        control.input.setCustomValidity("This answer was refused. Correct it before saving or switching documents.");
      }
      this.update({ forms: [...this.changes], fields: [...this.fields] } as EditState);
    }).finally(() => { this.pending.delete(task); });
    this.pending.add(task);
  }

  /** Flush before save, switching tabs, or closing the document. */
  commit(): void { for (const control of this.controls) this.commitOne(control, true); }

  /** Freeze editing during file writes without blurring an uncommitted draft. */
  setBusy(busy: boolean): void {
    for (const { button } of this.places) button.disabled = busy;
    for (const { widget, input } of this.controls) {
      if (input instanceof HTMLSelectElement || widget.control.kind === "radio" || widget.control.kind === "checkbox") input.disabled = busy;
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
    this.fields = state.fields ?? [];
    const widgets = this.controls.map((control) => control.widget);
    for (const control of this.controls) {
      const clean = !control.pending && sameAnswer(this.read(control), control.accepted) && !control.input.validationMessage;
      const kind = control.widget.control;
      if (kind.kind === "choice") {
        const choices = shownChoices(control.widget, this.fields, widgets)
          ?? kind.options.map((option, index) => ({ label: option.label, index }));
        if (JSON.stringify(choices) !== JSON.stringify(control.choices)) this.offer(control, choices);
      }
      const align = shownAlign(control.widget, this.fields, widgets);
      control.input.style.textAlign = align === "left" ? "" : align;
      if (clean) this.put(control, shownValue(control.widget, this.changes, this.fields, widgets));
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
    for (const { widget, button } of this.places) {
      const box = this.anchor(widget);
      const visible = box && box.bottom >= 0 && box.top <= height;
      const display = visible ? "block" : "none";
      if (button.style.display !== display) button.style.display = display;
      if (!box || !visible) continue;
      Object.assign(button.style, { fontSize: `${12 * (box.scale ?? 1)}px`, clipPath: box.clip ?? "none", left: `${box.left}px`, top: `${box.top}px`, width: `${box.right - box.left}px`, height: `${box.bottom - box.top}px` });
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
