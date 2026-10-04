/**
 * The panel a form field's properties are set in.
 *
 * `savedfields.ts` says what a field's properties are and turns a new set
 * into a change; this asks a reader for the new set. What it shows is decided
 * by the field's kind: every field has a tooltip and the two flags, a text
 * field has a most characters and an alignment, a field of choices has an
 * alignment and its choices.
 *
 * What a reader typed is read by {@link read}, which has no window in it, so
 * each refusal has a test. Every string shown goes in through `textContent`
 * or a control's `value`.
 */

import { parseChoices } from "./fieldnames";
import type { EditState } from "./edits";
import type { Form, FormAlign } from "./forms";
import {
  type FieldProperties, type FieldProps, type FieldTarget,
  differing, isSaved, placedProperties, propertied, properties,
} from "./savedfields";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-field-properties";

/** The most characters a text field can be limited to: the most an answer holds. */
export const MAX_LENGTH = 16384;
/** The most characters a tooltip has. */
export const MAX_TOOLTIP = 1024;

/** What the panel's controls hold, as text and ticks. */
export interface Typed {
  tooltip: string;
  required: boolean;
  readOnly: boolean;
  /** Empty for no limit. */
  maxLength: string;
  align: string;
  /** With semicolons between them, as `Form field: dropdown` takes them. */
  options: string;
}

/** What the controls show for a field's properties. */
export function shown(now: FieldProperties): Typed {
  return {
    tooltip: now.tooltip,
    required: now.required,
    readOnly: now.readOnly,
    maxLength: now.maxLength ? String(now.maxLength) : "",
    align: now.align ?? "left",
    options: (now.options ?? []).join("; "),
  };
}

/**
 * The properties a reader typed, or why they cannot be the field's.
 *
 * `now` says which parts the field's kind has; a part it does not have stays
 * `null` whatever the control holds.
 */
export function read(now: FieldProperties, typed: Typed): FieldProperties | { problem: string } {
  const tooltip = typed.tooltip.trim();
  if ([...tooltip].length > MAX_TOOLTIP) return { problem: `A tooltip is at most ${MAX_TOOLTIP} characters` };
  if (/\p{Cc}/u.test(tooltip)) return { problem: "A tooltip cannot contain a control character" };
  let maxLength: number | null = null;
  if (now.maxLength !== null) {
    const text = typed.maxLength.trim();
    if (text === "") {
      maxLength = 0;
    } else if (!/^[0-9]{1,6}$/.test(text) || Number(text) < 1 || Number(text) > MAX_LENGTH) {
      return { problem: `The most characters is a number from 1 to ${MAX_LENGTH}, or empty for no limit` };
    } else {
      maxLength = Number(text);
    }
  }
  let align: FormAlign | null = null;
  if (now.align !== null) {
    if (typed.align !== "left" && typed.align !== "center" && typed.align !== "right") {
      return { problem: "The alignment is left, centre or right" };
    }
    align = typed.align;
  }
  let options: string[] | null = null;
  if (now.options !== null) {
    const parsed = parseChoices(typed.options);
    if ("problem" in parsed) return parsed;
    options = parsed.options;
  }
  return { name: now.name, tooltip, required: typed.required, readOnly: typed.readOnly, maxLength, align, options };
}

/** What {@link changeProperties} needs from the window around it. */
export interface PropertiesDeps {
  /** The ids the viewer has picked. */
  picked(): readonly number[];
  /** The open document's form, while its own fields are being changed. */
  form(): Form | null;
  state(): Pick<EditState, "fields" | "pages" | "marks"> | null;
  /** Puts the panel to the reader. */
  ask(now: FieldProperties): Promise<FieldProperties | null>;
  /** Changes a field of the file, as one undoable edit. */
  refield(target: FieldTarget): void;
  /** Changes a field placed in this session, as one undoable edit. */
  refit(mark: number, props: FieldProps): void;
  say(message: string): void;
}

type Subject = Pick<PropertiesDeps, "picked" | "form" | "state">;

/**
 * What the one picked field's properties are: a field of the file while those
 * are being changed, or a field placed in this session. `null` with none
 * picked, several picked, or a mark that is not a field.
 */
function subject(deps: Subject): { id: number; now: FieldProperties } | null {
  const picked = deps.picked();
  const state = deps.state();
  const id = picked.length === 1 ? picked[0]! : null;
  if (id === null || !state) return null;
  const form = deps.form();
  const now = isSaved(id)
    ? form && properties(form, state, id)
    : placedProperties(state.marks.find((mark) => mark.id === id));
  return now ? { id, now } : null;
}

/** The one field the panel would be about, by the id the viewer has for it. */
export function pickedField(deps: Subject): number | null {
  return subject(deps)?.id ?? null;
}

/**
 * Asks for the picked field's properties and makes the change. Resolves with
 * whether anything was changed.
 *
 * The field is asked for again after the panel closes: a panel can stay open
 * across an undo or a closed tab, and the change is made to the field as it
 * then is, or not at all.
 */
export async function changeProperties(deps: PropertiesDeps): Promise<boolean> {
  const before = subject(deps);
  if (!before) return false;
  const to = await deps.ask(before.now);
  if (!to) return false;
  const after = subject({ ...deps, picked: () => [before.id] });
  const state = deps.state();
  const form = deps.form();
  if (!after || !state) return false;
  if (!isSaved(after.id)) {
    const props = differing(after.now, to);
    if (Object.keys(props).length === 0) {
      deps.say("Nothing about the field was changed.");
      return false;
    }
    deps.refit(after.id, props);
    return true;
  }
  const target = form ? propertied(form, state, after.id, to) : null;
  if (!target) {
    deps.say("Nothing about the field was changed.");
    return false;
  }
  deps.refield(target);
  return true;
}

const FIELD_STYLE =
  "display:block;width:100%;box-sizing:border-box;padding:0.4rem 0.5rem;font:inherit;" +
  "border-radius:6px;border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
  "background:Field;color:FieldText;";

/** A modal asking for a field's properties. Built once and reused. */
export class FieldPropertiesDialog {
  private readonly backdrop: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly problem: HTMLElement;
  private readonly tooltip: HTMLInputElement;
  private readonly required: HTMLInputElement;
  private readonly readOnly: HTMLInputElement;
  private readonly maxLength: HTMLInputElement;
  private readonly align: HTMLSelectElement;
  private readonly options: HTMLInputElement;
  /** The rows a field's kind may not have, hidden for it. */
  private readonly rows: { maxLength: HTMLElement; align: HTMLElement; options: HTMLElement };
  private returnFocus: HTMLElement | null = null;
  private pending: ((to: FieldProperties | null) => void) | null = null;
  private now: FieldProperties | null = null;
  private open = false;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("aria-label", "Field properties");
    panel.style.cssText =
      "margin-top:14vh;width:min(420px,92vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0 0 0.6rem;font-size:15px;font-weight:600;overflow-wrap:anywhere;";

    this.tooltip = this.input("Tooltip");
    this.required = this.tick();
    this.readOnly = this.tick();
    this.maxLength = this.input("Most characters");
    this.maxLength.inputMode = "numeric";
    this.maxLength.placeholder = "No limit";
    this.align = document.createElement("select");
    this.align.setAttribute("aria-label", "Alignment");
    this.align.style.cssText = FIELD_STYLE;
    for (const [value, label] of [["left", "Left"], ["center", "Centre"], ["right", "Right"]] as const) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      this.align.append(option);
    }
    this.options = this.input("Choices");
    this.options.placeholder = "Yes; No; Maybe";

    this.rows = {
      maxLength: this.row("Most characters", this.maxLength),
      align: this.row("Alignment", this.align),
      options: this.row("Choices, with a semicolon between them", this.options),
    };

    this.problem = document.createElement("p");
    this.problem.setAttribute("role", "alert");
    this.problem.style.cssText = "margin:0.6rem 0 0;min-height:1.55em;";

    const buttons = document.createElement("div");
    buttons.style.cssText = "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.85rem;";
    const cancel = this.button("Cancel", () => this.settle(null));
    const apply = this.button("Apply", () => this.submit());
    apply.style.fontWeight = "600";
    buttons.append(cancel, apply);

    panel.append(
      this.heading,
      this.row("Tooltip", this.tooltip),
      this.ticked("Required", this.required),
      this.ticked("Read-only", this.readOnly),
      this.rows.maxLength,
      this.rows.align,
      this.rows.options,
      this.problem,
      buttons,
    );
    this.backdrop.append(panel);
    host.append(this.backdrop);

    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.settle(null);
    });
    this.backdrop.addEventListener("keydown", (event) => {
      // Stopped as well as defaulted away: the window's key handler binds both.
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.settle(null);
      } else if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        this.submit();
      } else {
        // A letter typed here is the panel's, not a command's.
        event.stopPropagation();
      }
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.open;
  }

  /** Asks for a field's properties, or `null` when dismissed. */
  ask(now: FieldProperties): Promise<FieldProperties | null> {
    this.settle(null);
    this.now = now;
    this.heading.textContent = `Properties of ${now.name}`;
    this.problem.textContent = "";
    this.put(shown(now));
    this.rows.maxLength.style.display = now.maxLength === null ? "none" : "block";
    this.rows.align.style.display = now.align === null ? "none" : "block";
    this.rows.options.style.display = now.options === null ? "none" : "block";
    const active = document.activeElement as { focus?: () => void } | null;
    this.returnFocus = typeof active?.focus === "function" ? (active as HTMLElement) : null;
    this.open = true;
    this.backdrop.style.display = "flex";
    this.tooltip.focus();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(null);
  }

  /** Fills the controls. Also what the check harness types with. */
  put(typed: Typed): void {
    this.tooltip.value = typed.tooltip;
    this.required.checked = typed.required;
    this.readOnly.checked = typed.readOnly;
    this.maxLength.value = typed.maxLength;
    this.align.value = typed.align;
    this.options.value = typed.options;
  }

  /** Applies what the controls hold, or says why not and stays open. */
  submit(): void {
    if (!this.now) return;
    const to = read(this.now, {
      tooltip: this.tooltip.value,
      required: this.required.checked,
      readOnly: this.readOnly.checked,
      maxLength: this.maxLength.value,
      align: this.align.value,
      options: this.options.value,
    });
    if ("problem" in to) {
      this.problem.textContent = to.problem;
      return;
    }
    this.settle(to);
  }

  /** Resolves once and hides the dialog. */
  private settle(to: FieldProperties | null): void {
    const pending = this.pending;
    this.pending = null;
    this.now = null;
    if (this.open) {
      this.open = false;
      this.backdrop.style.display = "none";
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(to);
  }

  private input(label: string): HTMLInputElement {
    const field = document.createElement("input");
    field.type = "text";
    field.setAttribute("aria-label", label);
    field.autocomplete = "off";
    field.spellcheck = false;
    field.style.cssText = FIELD_STYLE;
    return field;
  }

  private tick(): HTMLInputElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    return box;
  }

  /** A control under its caption. */
  private row(caption: string, control: HTMLElement): HTMLElement {
    const row = document.createElement("label");
    row.style.cssText = "display:block;margin-top:0.5rem;";
    const text = document.createElement("span");
    text.textContent = caption;
    text.style.cssText = "display:block;opacity:0.72;";
    row.append(text, control);
    return row;
  }

  /** A tick box with its caption beside it. */
  private ticked(caption: string, box: HTMLInputElement): HTMLElement {
    const row = document.createElement("label");
    row.style.cssText = "display:flex;gap:0.45rem;align-items:center;margin-top:0.5rem;";
    const text = document.createElement("span");
    text.textContent = caption;
    row.append(box, text);
    return row;
  }

  private button(label: string, onClick: () => void): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.style.cssText =
      "padding:0.35rem 0.9rem;font:inherit;border-radius:6px;" +
      "border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:ButtonFace;color:ButtonText;cursor:pointer;";
    button.addEventListener("click", onClick);
    return button;
  }
}
