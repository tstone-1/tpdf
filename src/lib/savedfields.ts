/**
 * The form fields a document already has, as rectangles a reader can move.
 *
 * A field placed in this session is a mark until it is saved, and the viewer
 * knows how to pick, drag, resize, name and remove a mark. A field that is
 * already in the file is not a mark and never becomes one: it is changed
 * through `form_field_edit`, by its widget. This module is the join between
 * the two. It shows each saved field to the viewer as a mark of kind `field`
 * under an id of its own, and turns what the viewer then reports about that
 * id back into a change to the widget.
 *
 * No window and no state: every function here is asked with the scanned form
 * and the model's reply, and answers from those.
 */

import type { EditState } from "./edits";
import { radioExport, type Form, type FormAlign, type FormWidget } from "./forms";
import { INK_WIDTH } from "./markband";
import type { MarkView, PageId, PageView } from "./pages";
import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    nameTaken: (name: string) => `\`${name}\`: another field has this name`,
    valueTaken: (value: string) => `The group already has a button with the value \`${value}\``,
  },
  "de-DE": {
    nameTaken: (name: string) => `„${name}“: Ein anderes Feld hat diesen Namen`,
    valueTaken: (value: string) => `Die Gruppe hat schon eine Schaltfläche mit dem Wert „${value}“`,
  },
});

/**
 * Where the ids of saved fields start. The model counts marks up from one, so
 * a document would need a million million marks before the two met.
 */
export const SAVED_BASE = 2 ** 40;

/** Whether an id the viewer reports names a saved field. */
export function isSaved(id: number): boolean {
  return id >= SAVED_BASE;
}

/** The parts of a field's properties a reader has changed. A part left out is as the file has it. */
export interface FieldProps {
  /** Empty takes the tooltip off. */
  tooltip?: string;
  required?: boolean;
  read_only?: boolean;
  /** Nought takes the limit off. */
  max_length?: number;
  align?: FormAlign;
  options?: string[];
  /** In points. Nought is a size that follows the field's height. */
  text_size?: number;
  /** Empty takes the default off. */
  default_value?: string;
}

/** What a reader has changed about one widget, as the model reports it. */
export interface FieldEdited {
  object: [number, number];
  page: number;
  rect?: [number, number, number, number];
  name?: string;
  removed: boolean;
  props?: FieldProps;
  /** The value a radio button now gives its group. */
  value?: string;
}

/** One widget to change, as `form_field_edit` takes it. */
export interface FieldTarget {
  object: [number, number];
  page: number;
  patch: { field_pages?: number[]; rect?: [number, number, number, number]; name?: string; removed?: boolean; props?: FieldProps; value?: string };
}

type Rect = [number, number, number, number];

const same = (a: readonly [number, number], b: readonly [number, number]): boolean =>
  a[0] === b[0] && a[1] === b[1];

function editOf(widget: FormWidget, state: Pick<EditState, "fields">): FieldEdited | undefined {
  return state.fields?.find((edit) => same(edit.object, widget.widget));
}

function sharedEdit(widget: FormWidget, state: Pick<EditState, "fields">): FieldEdited | undefined {
  return state.fields?.find((edit) => same(edit.object, widget.object));
}

/** Field properties belong to the terminal field even when its widgets span pages. */
function sharedTarget(form: Form, found: { widget: FormWidget; page: PageId }, patch: FieldTarget["patch"]): FieldTarget {
  const { widget, page } = found;
  if (same(widget.object, widget.widget)) return target(found, patch);
  const pages = form.widgets.filter((other) => same(other.object, widget.object)).map((other) => other.page);
  return { object: widget.object, page, patch: { ...patch, field_pages: [...new Set(pages)] } };
}

/** The last part of a field's full name, which is the part a rename changes. */
export function leafName(full: string): string {
  return full.slice(full.lastIndexOf(".") + 1);
}

/** The id of the page of the opened file a widget is on, if it is still there. */
function pageOf(widget: FormWidget, pages: readonly PageView[]): PageId | undefined {
  return pages.find((page) => "baseline" in page.source && page.source.baseline === widget.page)?.id;
}

/**
 * The form as it now stands: each widget where a reader has moved it and under
 * the name they gave it, and without the ones they removed.
 *
 * What the filling layer is given, so that an answer typed after a field was
 * moved lands where the field now is.
 */
export function standing(form: Form, state: Pick<EditState, "fields">): FormWidget[] {
  return form.widgets.flatMap((widget) => {
    const edit = editOf(widget, state);
    if (edit?.removed) return [];
    return [{ ...widget, display_rect: edit?.rect ?? widget.display_rect }];
  });
}

/**
 * Every saved field still in the document, as a mark the viewer can pick,
 * move, resize and name. A widget on a page that has been deleted has none.
 */
export function asMarks(form: Form, state: Pick<EditState, "fields" | "pages">): MarkView[] {
  return form.widgets.flatMap((widget, at) => {
    const page = pageOf(widget, state.pages);
    const edit = editOf(widget, state);
    if (page === undefined || edit?.removed) return [];
    const kind = widget.control.kind === "checkbox" || widget.control.kind === "radio"
      ? "checkbox"
      : widget.control.kind === "signature" ? "signature"
      : widget.multiline ? "multiline" : "text";
    return [{
      id: SAVED_BASE + at,
      kind: "field",
      stamp: null,
      page,
      quads: [...(edit?.rect ?? widget.display_rect)],
      strokes: [],
      color: [0.1, 0.35, 0.75],
      width: INK_WIDTH,
      note: sharedEdit(widget, state)?.name ?? leafName(widget.name),
      lines: [],
      field: { kind, border: false },
      fieldUnit: widget.unit ?? 1,
    } satisfies MarkView];
  });
}

/** The widget an id names, with the page it is on, or `null` for none. */
export function savedField(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  id: number,
): { widget: FormWidget; page: PageId } | null {
  return named(form, state, id);
}

function named(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  id: number,
): { widget: FormWidget; page: PageId; rect: Rect } | null {
  const widget = isSaved(id) ? form.widgets[id - SAVED_BASE] : undefined;
  const page = widget ? pageOf(widget, state.pages) : undefined;
  if (!widget || page === undefined) return null;
  return { widget, page, rect: editOf(widget, state)?.rect ?? widget.display_rect };
}

function target(found: { widget: FormWidget; page: PageId }, patch: FieldTarget["patch"]): FieldTarget {
  return { object: found.widget.widget, page: found.page, patch };
}

/** A saved field moved by an offset, as a change to its widget. */
export function moved(form: Form, state: Pick<EditState, "fields" | "pages">, id: number, dx: number, dy: number): FieldTarget | null {
  const found = named(form, state, id);
  if (!found) return null;
  const [left, top, right, bottom] = found.rect;
  return target(found, { rect: [left + dx, top + dy, right + dx, bottom + dy] });
}

/** A saved field given a new rectangle. */
export function placed(form: Form, state: Pick<EditState, "fields" | "pages">, id: number, rect: Rect): FieldTarget | null {
  const found = named(form, state, id);
  return found ? target(found, { rect }) : null;
}

/** A saved field removed. */
export function removed(form: Form, state: Pick<EditState, "fields" | "pages">, id: number): FieldTarget | null {
  const found = named(form, state, id);
  return found ? target(found, { removed: true }) : null;
}

/**
 * What taking the mark the viewer names off the page comes to: the model's own
 * mark removed, a saved field removed through its widget, or nothing.
 *
 * One answer for every route the viewer reports a removal by --- the note's
 * Remove, the Delete key, the eraser taking a mark whole --- because while the
 * fields are being changed a saved field is in the viewer's list under an id
 * the model never issued, and a route that sent that id on as a mark's asked
 * the backend to remove a mark it has never heard of. `null` is a field's id
 * that names no field now: the form is not scanned, or its page is gone.
 */
export function removal(
  form: Form | null,
  state: Pick<EditState, "fields" | "pages"> | null,
  id: number,
): { mark: number } | { field: FieldTarget } | null {
  if (!isSaved(id)) return { mark: id };
  const field = form && state ? removed(form, state, id) : null;
  return field ? { field } : null;
}

/**
 * A saved field renamed, or the reason it cannot have that name.
 *
 * The save refuses a name another field has; asked here first, of the form as
 * it now stands, so a reader is told while the name box is still open. Only
 * fields at the top of the form are compared: a field inside a group may
 * share a name with one in another group, and the save decides those.
 */
export function renamed(
  form: Form,
  state: Pick<EditState, "fields" | "pages" | "marks">,
  id: number,
  name: string,
): FieldTarget | string | null {
  const found = named(form, state, id);
  if (!found) return null;
  const top = !found.widget.name.includes(".");
  const others = form.widgets.filter((other) => !same(other.object, found.widget.object) && !other.name.includes("."));
  const taken = top && (
    others.some((other) => {
      const edit = editOf(other, state);
      return !edit?.removed && (sharedEdit(other, state)?.name ?? other.name) === name;
    })
    || state.marks.some((mark) => mark.kind === "field" && mark.note === name)
  );
  if (taken) return TEXT().nameTaken(name);
  return sharedTarget(form, found, { name });
}

/**
 * A field's properties as they now stand: the file's, with what a reader has
 * changed laid over them. A part the field's kind does not have is `null`.
 */
export interface FieldProperties {
  /** The field's name, for the panel's heading. */
  name: string;
  tooltip: string;
  required: boolean;
  readOnly: boolean;
  /** The most characters a text field takes; nought for no limit. */
  maxLength: number | null;
  /** Where a text or choice field's text sits. */
  align: FormAlign | null;
  /** What a choice field offers; for a radio button, the one value it gives its group. */
  options: string[] | null;
  /** Whether `options` is one value and not a list: a radio button. */
  single?: boolean;
  /** A multiline text field accepts line breaks in its default. */
  multiline?: boolean;
  /**
   * The size a text or choice field's text is drawn at where it fits, in
   * points; nought for a size that follows the field's height. Absent or
   * `null` for a field that has none to set.
   */
  textSize?: number | null;
  /**
   * What a text field holds after a reset, and now if it holds nothing; empty
   * for none. Absent or `null` for a field that has none to set.
   */
  defaultValue?: string | null;
}

/**
 * The value a radio button of the file gives its group: the one a reader has
 * just given it, or the name of the state the file has for it being chosen.
 */
export function buttonValue(widget: FormWidget, state: Pick<EditState, "fields">): string {
  const given = editOf(widget, state)?.value;
  if (given !== undefined) return given;
  const control = widget.control;
  return control.kind === "radio" ? radioExport(control, control.index) : "";
}

/** Qualified names after saved-widget removals and pending terminal-field renames. */
export function effectiveFieldNames(form: Form, fields: readonly FieldEdited[]): string[] {
  const edits = new Map(fields.map(field => [field.object.join(":"), field]));
  return form.widgets.flatMap(widget => {
    if (edits.get(widget.widget.join(":"))?.removed) return [];
    const leaf = edits.get(widget.object.join(":"))?.name;
    return [leaf === undefined ? widget.name : widget.name.slice(0, widget.name.lastIndexOf(".") + 1) + leaf];
  });
}

/**
 * The values a group's buttons already have: the ones placed in this session
 * and the ones the file's form has under that name.
 */
export function radioValues(group: string, marks: readonly MarkView[], form: Form | null, fields: readonly FieldEdited[]): Set<string> {
  const values = savedRadioValues(group, form, fields);
  for (const mark of marks) {
    if (mark.kind === "field" && mark.note === group && mark.field?.kind === "radio") {
      for (const value of mark.field.options ?? []) values.add(value);
    }
  }
  return values;
}

export function radioValueTaken(value: string): string {
  return TEXT().valueTaken(value);
}

/** Export values still occupied after pending group and widget edits. */
function savedRadioValues(group: string, form: Form | null, fields: readonly FieldEdited[]): Set<string> {
  const edits = new Map(fields.map(field => [field.object.join(":"), field]));
  const byField = new Map<string, (string | undefined)[]>();
  for (const widget of form?.widgets ?? []) {
    const control = widget.control;
    if (control.kind !== "radio" || widget.name.includes(".")) continue;
    const key = widget.object.join(":");
    if ((edits.get(key)?.name ?? widget.name) !== group) continue;
    let values = byField.get(key);
    if (!values) {
      // Each widget carries the group's full export list, even when another
      // member has no widget on a scanned page.
      values = control.states.map((_, index) => radioExport(control, index));
      byField.set(key, values);
    }
    const edit = edits.get(widget.widget.join(":"));
    if (edit?.removed) values[control.index] = undefined;
    else if (edit?.value !== undefined) values[control.index] = edit.value;
  }
  const occupied = new Set<string>();
  for (const values of byField.values()) {
    for (const value of values) if (value !== undefined) occupied.add(value);
  }
  return occupied;
}

/** The properties of the saved field an id names, or `null` when it names none. */
export function properties(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  id: number,
): FieldProperties | null {
  const found = named(form, state, id);
  if (!found) return null;
  const { widget } = found;
  const shared = sharedEdit(widget, state);
  const changed = <K extends keyof FieldProps>(part: K): FieldProps[K] | undefined => shared?.props?.[part];
  const kind = widget.control.kind;
  return {
    name: shared?.name ?? leafName(widget.name),
    tooltip: changed("tooltip") ?? widget.tooltip ?? "",
    required: changed("required") ?? widget.required ?? false,
    readOnly: changed("read_only") ?? widget.read_only ?? false,
    maxLength: kind === "text" ? changed("max_length") ?? widget.max_length ?? 0 : null,
    align: kind === "text" || kind === "choice" ? changed("align") ?? widget.align ?? "left" : null,
    options: widget.control.kind === "choice"
      ? changed("options") ?? widget.control.options.map((option) => option.label)
      : widget.control.kind === "radio" ? [buttonValue(widget, state)] : null,
    ...(kind === "radio" ? { single: true } : {}),
    ...(widget.multiline && kind === "text" ? { multiline: true } : {}),
    textSize: kind === "text" || kind === "choice" ? changed("text_size") ?? widget.text_size ?? 0 : null,
    defaultValue: kind === "text" ? changed("default_value") ?? widget.default_value ?? "" : null,
  };
}

/**
 * The parts of `to` that differ from `now`, among the parts the field's kind
 * has. Empty when none does.
 */
export function differing(now: FieldProperties, to: FieldProperties): FieldProps {
  const props: FieldProps = {};
  if (to.tooltip !== now.tooltip) props.tooltip = to.tooltip;
  if (to.required !== now.required) props.required = to.required;
  if (to.readOnly !== now.readOnly) props.read_only = to.readOnly;
  if (now.maxLength !== null && to.maxLength !== null && to.maxLength !== now.maxLength) props.max_length = to.maxLength;
  if (now.align !== null && to.align !== null && to.align !== now.align) props.align = to.align;
  if (now.options !== null && to.options !== null
    && (to.options.length !== now.options.length || to.options.some((option, at) => option !== now.options![at]))) {
    props.options = [...to.options];
  }
  if (now.textSize != null && to.textSize != null && to.textSize !== now.textSize) props.text_size = to.textSize;
  if (now.defaultValue != null && to.defaultValue != null && to.defaultValue !== now.defaultValue) {
    props.default_value = to.defaultValue;
  }
  return props;
}

/**
 * The properties of a field placed in this session, or `null` for a mark
 * that is not a field.
 */
export function placedProperties(mark: MarkView | undefined): FieldProperties | null {
  const field = mark?.kind === "field" ? mark.field : undefined;
  if (!mark || !field) return null;
  const text = field.kind === "text" || field.kind === "multiline";
  return {
    name: mark.note,
    tooltip: field.tooltip ?? "",
    required: field.required ?? false,
    readOnly: field.read_only ?? false,
    maxLength: text ? field.max_length ?? 0 : null,
    align: field.kind === "checkbox" || field.kind === "radio" ? null : field.align ?? "left",
    options: field.kind === "dropdown" || field.kind === "radio" ? [...(field.options ?? [])] : null,
    ...(field.kind === "radio" ? { single: true } : {}),
    ...(field.kind === "multiline" ? { multiline: true } : {}),
    textSize: text || field.kind === "dropdown" ? field.text_size ?? 0 : null,
    defaultValue: text ? field.default_value ?? "" : null,
  };
}

/**
 * A saved field given new properties: a change naming only the parts that
 * differ from what the field now has, or `null` when none does.
 */
export function propertied(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  id: number,
  to: FieldProperties,
): FieldTarget | null {
  const found = named(form, state, id);
  const now = properties(form, state, id);
  if (!found || !now) return null;
  const props = differing(now, to);
  // A button's value is its own and not the group's: `revalued` makes that change.
  if (now.single) delete props.options;
  if (Object.keys(props).length === 0) return null;
  return sharedTarget(form, found, { props });
}

/**
 * A radio button of the file given a new value: a change to that button, a
 * sentence when another button of its group has the value, or `null` when the
 * value is the one it has or the field is not a radio button.
 */
export function revalued(
  form: Form,
  state: Pick<EditState, "fields" | "pages" | "marks">,
  id: number,
  to: FieldProperties,
): FieldTarget | string | null {
  const found = named(form, state, id);
  const value = to.options?.[0];
  if (!found || found.widget.control.kind !== "radio" || value === undefined) return null;
  if (value === buttonValue(found.widget, state)) return null;
  const group = sharedEdit(found.widget, state)?.name ?? found.widget.name;
  const taken = !found.widget.name.includes(".")
    ? radioValues(group, state.marks, form, state.fields ?? []).has(value)
    : form.widgets.some((other) => same(other.object, found.widget.object)
      && !editOf(other, state)?.removed && buttonValue(other, state) === value);
  if (taken) return radioValueTaken(value);
  return target(found, { value });
}

/**
 * An arrangement's moves, split into the placed marks and the saved fields.
 * Both halves are sent under one gesture, so one undo puts all of them back.
 */
export function split(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  moves: readonly { mark: number; rect: Rect }[],
): { marks: { mark: number; rect: Rect }[]; fields: FieldTarget[] } {
  const marks: { mark: number; rect: Rect }[] = [];
  const fields: FieldTarget[] = [];
  for (const move of moves) {
    if (!isSaved(move.mark)) {
      marks.push(move);
      continue;
    }
    const to = placed(form, state, move.mark, move.rect);
    if (to) fields.push(to);
  }
  return { marks, fields };
}

/**
 * Where the filling layer puts a widget's control: where a reader moved the
 * field to, where the file has it, or nowhere for one they removed.
 */
export function shownAt(widget: FormWidget, state: Pick<EditState, "fields">): Rect | null {
  const edit = editOf(widget, state);
  if (edit?.removed) return null;
  return edit?.rect ?? widget.display_rect;
}

/**
 * Where a widget's control is put for a document whose fields are, or are not,
 * being changed: nowhere while they are, since a press on a field then picks
 * it and does not type, nowhere for a document with no model, and otherwise
 * {@link shownAt}.
 *
 * `editing` and `state` are the control's own document's. `App.svelte` reads
 * them as that document, whichever one the reader is working in: a control
 * lays itself out from its own key handler too, and the reader may be in the
 * other side then.
 */
export function controlRect(
  widget: FormWidget,
  editing: boolean,
  state: Pick<EditState, "fields"> | null,
): Rect | null {
  if (editing || !state) return null;
  return shownAt(widget, state);
}

/**
 * The pages of the opened file that are drawn differently after a change to
 * its fields: every page with a widget whose pending change is not what it
 * was. Undoing a change counts, and so does a widget of a renamed field on
 * another page.
 *
 * The page picture is drawn with the pending changes in it, so these are the
 * pages to draw again. Asked with the widgets and not with the changes alone,
 * because a change names the page's id and a picture is asked for by the page
 * of the file, which a deleted page still has.
 */
export function redrawn(
  widgets: readonly Pick<FormWidget, "widget" | "object" | "page">[],
  before: readonly FieldEdited[],
  after: readonly FieldEdited[],
): number[] {
  const changes = (edits: readonly FieldEdited[]) =>
    new Map(edits.map((edit) => [edit.object.join(":"), JSON.stringify(edit)]));
  const previous = changes(before), next = changes(after);
  const changed = (key: string) => previous.get(key) !== next.get(key);
  const pages = widgets.filter((widget) => changed(widget.widget.join(":")) || changed(widget.object.join(":")))
    .map((widget) => widget.page);
  return [...new Set(pages)];
}

/** The part of an edit model an arrangement of saved fields needs. */
interface Arranges {
  arrange(moves: { mark: number; rect: Rect }[], sweep: number): Promise<EditState>;
  refield(targets: FieldTarget[], sweep?: number): Promise<EditState>;
}

/**
 * Makes an arrangement that may hold placed marks, saved fields or both. The
 * two kinds go to two commands under the one gesture, so the journal holds
 * them side by side and one undo crosses both.
 */
export async function arrangeBoth(
  form: Form | null,
  state: Pick<EditState, "fields" | "pages">,
  edits: Arranges,
  moves: { mark: number; rect: Rect }[],
  sweep: number,
): Promise<EditState> {
  const parts = form ? split(form, state, moves) : { marks: moves, fields: [] };
  if (parts.fields.length === 0) return edits.arrange(parts.marks, sweep);
  if (parts.marks.length > 0) await edits.arrange(parts.marks, sweep);
  return edits.refield(parts.fields, sweep);
}
