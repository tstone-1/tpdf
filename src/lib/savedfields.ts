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
import type { Form, FormAlign, FormWidget } from "./forms";
import { INK_WIDTH } from "./markband";
import type { MarkView, PageId, PageView } from "./pages";

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
}

/** One widget to change, as `form_field_edit` takes it. */
export interface FieldTarget {
  object: [number, number];
  page: number;
  patch: { rect?: [number, number, number, number]; name?: string; removed?: boolean; props?: FieldProps };
}

type Rect = [number, number, number, number];

const same = (a: readonly [number, number], b: readonly [number, number]): boolean =>
  a[0] === b[0] && a[1] === b[1];

function editOf(widget: FormWidget, state: Pick<EditState, "fields">): FieldEdited | undefined {
  return state.fields?.find((edit) => same(edit.object, widget.widget));
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
      note: edit?.name ?? leafName(widget.name),
      lines: [],
      field: { kind, border: false },
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
      return !edit?.removed && (edit?.name ?? other.name) === name;
    })
    || state.marks.some((mark) => mark.kind === "field" && mark.note === name)
  );
  if (taken) return `\`${name}\`: another field has this name`;
  return target(found, { name });
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
  /** What a choice field offers; for a radio button placed in this session, its one value. */
  options: string[] | null;
  /** Whether `options` is one value and not a list: a placed radio button. */
  single?: boolean;
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

/** The properties of the saved field an id names, or `null` when it names none. */
export function properties(
  form: Form,
  state: Pick<EditState, "fields" | "pages">,
  id: number,
): FieldProperties | null {
  const found = named(form, state, id);
  if (!found) return null;
  const { widget } = found;
  // A field has one set, held under whichever of its widgets was changed.
  const edits = (state.fields ?? []).filter((edit) =>
    form.widgets.some((other) => same(other.widget, edit.object) && same(other.object, widget.object)));
  const changed = <K extends keyof FieldProps>(part: K): FieldProps[K] | undefined =>
    edits.map((edit) => edit.props?.[part]).find((value) => value !== undefined);
  const kind = widget.control.kind;
  return {
    name: editOf(widget, state)?.name ?? leafName(widget.name),
    tooltip: changed("tooltip") ?? widget.tooltip ?? "",
    required: changed("required") ?? widget.required ?? false,
    readOnly: changed("read_only") ?? widget.read_only ?? false,
    maxLength: kind === "text" ? changed("max_length") ?? widget.max_length ?? 0 : null,
    align: kind === "text" || kind === "choice" ? changed("align") ?? widget.align ?? "left" : null,
    options: widget.control.kind === "choice"
      ? changed("options") ?? widget.control.options.map((option) => option.label)
      : null,
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
  if (Object.keys(props).length === 0) return null;
  // A field shown in several places has one set of properties, and the save
  // refuses two. So the change is always made under the same widget: the
  // field's first that is still there.
  const first = form.widgets
    .filter((other) => same(other.object, found.widget.object) && !editOf(other, state)?.removed)
    .map((other) => ({ widget: other, page: pageOf(other, state.pages) }))
    .find((other): other is { widget: FormWidget; page: PageId } => other.page !== undefined);
  return target(first ?? found, { props });
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
  widgets: readonly Pick<FormWidget, "widget" | "page">[],
  before: readonly FieldEdited[],
  after: readonly FieldEdited[],
): number[] {
  const of = (edits: readonly FieldEdited[], widget: readonly [number, number]) =>
    JSON.stringify(edits.find((edit) => same(edit.object, widget)) ?? null);
  const pages = widgets
    .filter(({ widget }) => of(before, widget) !== of(after, widget))
    .map(({ page }) => page);
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
