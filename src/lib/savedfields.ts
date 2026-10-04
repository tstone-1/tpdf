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
import type { Form, FormWidget } from "./forms";
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

/** What a reader has changed about one widget, as the model reports it. */
export interface FieldEdited {
  object: [number, number];
  page: number;
  rect?: [number, number, number, number];
  name?: string;
  removed: boolean;
}

/** One widget to change, as `form_field_edit` takes it. */
export interface FieldTarget {
  object: [number, number];
  page: number;
  patch: { rect?: [number, number, number, number]; name?: string; removed?: boolean };
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
