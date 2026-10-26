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
  differing, isSaved, placedProperties, propertied, properties, revalued,
} from "./savedfields";
import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    tooltipTooLong: (most: number) => `A tooltip is at most ${most} characters`,
    tooltipControl: "A tooltip cannot contain a control character",
    maxLengthRange: (most: number) => `The most characters is a number from 1 to ${most}, or empty for no limit`,
    alignmentChoice: "The alignment is left, center or right",
    buttonValue: "The value this button gives its group",
    radioOneValue: "A radio button has one value, with no semicolon in it",
    offReserved: "Off is what a group holds when nothing is chosen",
    textSizeRange: (min: number, max: number) =>
      `The text size is ${min} to ${max} points, or empty for one that follows the field's height`,
    defaultControl: "A default value cannot contain a control character",
    defaultTooLong: (most: number) => `The default value has more than the ${most} characters the field takes`,
    nothingChanged: "Nothing about the field was changed.",
    fieldProperties: "Field properties",
    tooltip: "Tooltip",
    mostCharacters: "Most characters",
    noLimit: "No limit",
    alignment: "Alignment",
    left: "Left",
    center: "Center",
    right: "Right",
    choices: "Choices",
    choicesExample: "Yes; No; Maybe",
    textSize: "Text size",
    followsHeight: "Follows the field's height",
    defaultValue: "Default value",
    choicesCaption: "Choices, with a semicolon between them",
    textSizeCaption: "Text size, in points",
    cancel: "Cancel",
    apply: "Apply",
    required: "Required",
    readOnly: "Read-only",
    propertiesOf: (name: string) => `Properties of ${name}`,
    buttonValueCaption: "Value this button gives its group",
  },
  "en-GB": {
    alignmentChoice: "The alignment is left, centre or right",
    center: "Centre",
  },
  "de-DE": {
    tooltipTooLong: (most: number) => `Ein Tooltip hat höchstens ${most} Zeichen`,
    tooltipControl: "Ein Tooltip darf kein Steuerzeichen enthalten",
    maxLengthRange: (most: number) => `Die Höchstzahl an Zeichen ist eine Zahl von 1 bis ${most}, oder leer für keine Grenze`,
    alignmentChoice: "Die Ausrichtung ist links, zentriert oder rechts",
    buttonValue: "Der Wert, den diese Schaltfläche ihrer Gruppe gibt",
    radioOneValue: "Ein Optionsfeld hat einen einzigen Wert, ohne Semikolon",
    offReserved: "„Off“ ist der Wert einer Gruppe, in der nichts gewählt ist",
    textSizeRange: (min: number, max: number) =>
      `Die Schriftgröße ist ${min} bis ${max} Punkt, oder leer für eine, die sich nach der Höhe des Felds richtet`,
    defaultControl: "Ein Standardwert darf kein Steuerzeichen enthalten",
    defaultTooLong: (most: number) => `Der Standardwert hat mehr als die ${most} Zeichen, die das Feld aufnimmt`,
    nothingChanged: "Am Feld wurde nichts geändert.",
    fieldProperties: "Feldeigenschaften",
    tooltip: "Tooltip",
    mostCharacters: "Höchstzahl an Zeichen",
    noLimit: "Keine Grenze",
    alignment: "Ausrichtung",
    left: "Links",
    center: "Zentriert",
    right: "Rechts",
    choices: "Auswahlmöglichkeiten",
    choicesExample: "Ja; Nein; Vielleicht",
    textSize: "Schriftgröße",
    followsHeight: "Richtet sich nach der Höhe des Felds",
    defaultValue: "Standardwert",
    choicesCaption: "Auswahlmöglichkeiten, durch Semikolon getrennt",
    textSizeCaption: "Schriftgröße in Punkt",
    cancel: "Abbrechen",
    apply: "Übernehmen",
    required: "Pflichtfeld",
    readOnly: "Schreibgeschützt",
    propertiesOf: (name: string) => `Eigenschaften von ${name}`,
    buttonValueCaption: "Wert, den diese Schaltfläche ihrer Gruppe gibt",
  },
});

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
  /** In points; empty for a size that follows the field's height. */
  textSize: string;
  defaultValue: string;
}

/** The least and the most text size a field is given, in points. */
export const MIN_TEXT_SIZE = 4;
export const MAX_TEXT_SIZE = 144;

/** What the controls show for a field's properties. */
export function shown(now: FieldProperties): Typed {
  return {
    tooltip: now.tooltip,
    required: now.required,
    readOnly: now.readOnly,
    maxLength: now.maxLength ? String(now.maxLength) : "",
    align: now.align ?? "left",
    options: (now.options ?? []).join("; "),
    textSize: now.textSize ? String(now.textSize) : "",
    defaultValue: now.defaultValue ?? "",
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
  if ([...tooltip].length > MAX_TOOLTIP) return { problem: TEXT().tooltipTooLong(MAX_TOOLTIP) };
  if (/\p{Cc}/u.test(tooltip)) return { problem: TEXT().tooltipControl };
  let maxLength: number | null = null;
  if (now.maxLength !== null) {
    const text = typed.maxLength.trim();
    if (text === "") {
      maxLength = 0;
    } else if (!/^[0-9]{1,6}$/.test(text) || Number(text) < 1 || Number(text) > MAX_LENGTH) {
      return { problem: TEXT().maxLengthRange(MAX_LENGTH) };
    } else {
      maxLength = Number(text);
    }
  }
  let align: FormAlign | null = null;
  if (now.align !== null) {
    if (typed.align !== "left" && typed.align !== "center" && typed.align !== "right") {
      return { problem: TEXT().alignmentChoice };
    }
    align = typed.align;
  }
  let options: string[] | null = null;
  if (now.options !== null) {
    const parsed = parseChoices(typed.options);
    if ("problem" in parsed) return now.single ? { problem: TEXT().buttonValue } : parsed;
    if (now.single && parsed.options.length !== 1) return { problem: TEXT().radioOneValue };
    if (now.single && parsed.options[0] === "Off") return { problem: TEXT().offReserved };
    options = parsed.options;
  }
  // A part the field's kind does not have stays as it was said: `null`, or left out.
  const sized: Pick<FieldProperties, "textSize" | "defaultValue"> = {
    ...(now.textSize === null ? { textSize: null } : {}),
    ...(now.defaultValue === null ? { defaultValue: null } : {}),
  };
  if (now.textSize != null) {
    const text = typed.textSize.trim().replace(",", ".");
    const size = text === "" ? 0 : Number(text);
    if (text !== "" && (!/^[0-9]{1,3}(\.[0-9]{1,2})?$/.test(text) || size < MIN_TEXT_SIZE || size > MAX_TEXT_SIZE)) {
      return { problem: TEXT().textSizeRange(MIN_TEXT_SIZE, MAX_TEXT_SIZE) };
    }
    sized.textSize = size;
  }
  if (now.defaultValue != null) {
    const value = typed.defaultValue;
    if (/\p{Cc}/u.test(value)) return { problem: TEXT().defaultControl };
    const most = maxLength || MAX_LENGTH;
    if ([...value].length > most) return { problem: TEXT().defaultTooLong(most) };
    sized.defaultValue = value;
  }
  return {
    name: now.name, tooltip, required: typed.required, readOnly: typed.readOnly, maxLength, align, options,
    ...(now.single ? { single: true } : {}),
    ...sized,
  };
}

/** What {@link changeProperties} needs from the window around it. */
export interface PropertiesDeps {
  /**
   * The open document's model, compared by identity and never read: what
   * says whether the document the panel was opened on is still the one open.
   */
  model(): unknown;
  /** The ids the viewer has picked. */
  picked(): readonly number[];
  /** The open document's form, while its own fields are being changed. */
  form(): Form | null;
  state(): Pick<EditState, "fields" | "pages" | "marks"> | null;
  /** Puts the panel to the reader. */
  ask(now: FieldProperties): Promise<FieldProperties | null>;
  /** Changes a field of the file, or two of its widgets at once, as one undoable edit. */
  refield(targets: FieldTarget[]): void;
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
 *
 * **And only in the document it was opened on.** Asking again by id is not
 * enough for that: a mark's id starts at 1 in every document and a field of
 * the file is named by its place in the form, so after a switch of tabs the
 * same id is very likely another document's field, found and changed.
 */
export async function changeProperties(deps: PropertiesDeps): Promise<boolean> {
  const model = deps.model();
  const before = subject(deps);
  if (!before) return false;
  const to = await deps.ask(before.now);
  if (!to) return false;
  if (deps.model() !== model) return false;
  const after = subject({ ...deps, picked: () => [before.id] });
  const state = deps.state();
  const form = deps.form();
  if (!after || !state) return false;
  if (!isSaved(after.id)) {
    const props = differing(after.now, to);
    if (Object.keys(props).length === 0) {
      deps.say(TEXT().nothingChanged);
      return false;
    }
    deps.refit(after.id, props);
    return true;
  }
  const target = form ? propertied(form, state, after.id, to) : null;
  const button = form ? revalued(form, state, after.id, to) : null;
  if (typeof button === "string") {
    deps.say(button);
    return false;
  }
  if (!target && !button) {
    deps.say(TEXT().nothingChanged);
    return false;
  }
  // The group's properties are held under its first button and the value
  // under the button picked. Where those are one button it is one change.
  const one = target && button && target.object.join() === button.object.join();
  deps.refield(one
    ? [{ ...target, patch: { ...target.patch, ...button.patch } }]
    : [target, button].filter((made): made is FieldTarget => made !== null));
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
  private readonly textSize: HTMLInputElement;
  private readonly defaultValue: HTMLInputElement;
  /** The rows a field's kind may not have, hidden for it. */
  private readonly rows: {
    maxLength: HTMLElement; align: HTMLElement; options: HTMLElement; textSize: HTMLElement; defaultValue: HTMLElement;
  };
  /** The caption over the choices, which a radio button reads as its value. */
  private readonly optionsCaption: HTMLElement;
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
    panel.setAttribute("aria-label", TEXT().fieldProperties);
    panel.style.cssText =
      "margin-top:14vh;width:min(420px,92vw);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0 0 0.6rem;font-size:15px;font-weight:600;overflow-wrap:anywhere;";

    const text = TEXT();
    this.tooltip = this.input(text.tooltip);
    this.required = this.tick();
    this.readOnly = this.tick();
    this.maxLength = this.input(text.mostCharacters);
    this.maxLength.inputMode = "numeric";
    this.maxLength.placeholder = text.noLimit;
    this.align = document.createElement("select");
    this.align.setAttribute("aria-label", text.alignment);
    this.align.style.cssText = FIELD_STYLE;
    for (const [value, label] of [["left", text.left], ["center", text.center], ["right", text.right]] as const) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      this.align.append(option);
    }
    this.options = this.input(text.choices);
    this.options.placeholder = text.choicesExample;
    this.textSize = this.input(text.textSize);
    this.textSize.inputMode = "decimal";
    this.textSize.placeholder = text.followsHeight;
    this.defaultValue = this.input(text.defaultValue);

    this.rows = {
      maxLength: this.row(text.mostCharacters, this.maxLength),
      align: this.row(text.alignment, this.align),
      options: this.row(text.choicesCaption, this.options),
      textSize: this.row(text.textSizeCaption, this.textSize),
      defaultValue: this.row(text.defaultValue, this.defaultValue),
    };
    this.optionsCaption = this.rows.options.children[0] as HTMLElement;

    this.problem = document.createElement("p");
    this.problem.setAttribute("role", "alert");
    this.problem.style.cssText = "margin:0.6rem 0 0;min-height:1.55em;";

    const buttons = document.createElement("div");
    buttons.style.cssText = "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.85rem;";
    const cancel = this.button(text.cancel, () => this.settle(null));
    const apply = this.button(text.apply, () => this.submit());
    apply.style.fontWeight = "600";
    buttons.append(cancel, apply);

    panel.append(
      this.heading,
      this.row(text.tooltip, this.tooltip),
      this.ticked(text.required, this.required),
      this.ticked(text.readOnly, this.readOnly),
      this.rows.maxLength,
      this.rows.align,
      this.rows.textSize,
      this.rows.defaultValue,
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
    this.heading.textContent = TEXT().propertiesOf(now.name);
    this.problem.textContent = "";
    this.put(shown(now));
    this.rows.maxLength.style.display = now.maxLength === null ? "none" : "block";
    this.rows.align.style.display = now.align === null ? "none" : "block";
    this.rows.options.style.display = now.options === null ? "none" : "block";
    this.rows.textSize.style.display = now.textSize == null ? "none" : "block";
    this.rows.defaultValue.style.display = now.defaultValue == null ? "none" : "block";
    this.optionsCaption.textContent = now.single
      ? TEXT().buttonValueCaption
      : TEXT().choicesCaption;
    this.options.placeholder = now.single ? "" : TEXT().choicesExample;
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
    this.textSize.value = typed.textSize;
    this.defaultValue.value = typed.defaultValue;
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
      textSize: this.textSize.value,
      defaultValue: this.defaultValue.value,
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
