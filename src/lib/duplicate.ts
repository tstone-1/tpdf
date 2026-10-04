/**
 * Copies of the rectangles a reader has picked.
 *
 * A form is mostly the same field many times: ten lines to write on, a row
 * of boxes to tick. Duplicating makes a copy of each picked rectangle a step
 * down and to the right, picked in place of the originals, so the next thing
 * a reader does is drag the copies where they go.
 *
 * What is copied is the kind, the size and the properties. What is not is
 * the name, which a form holds once: a copied field gets the next free name
 * of its kind, as a newly placed one does, and a copied radio button stays
 * in its group with the next free value. A field of the file is copied as a
 * newly placed field, since that is the only way a field comes to exist.
 *
 * No window here. The viewer says where the copies go, and the model makes
 * them; this decides what each copy is.
 */

import type { EditState } from "./edits";
import { placing } from "./fieldnames";
import type { Form } from "./forms";
import type { FieldKind, MarkKind, PageId, PlacedField } from "./pages";
import { isSaved, properties, savedField } from "./savedfields";

type Rect = [number, number, number, number];

/** One mark to make. */
export interface Copy {
  kind: MarkKind;
  page: PageId;
  quads: number[];
  note: string;
  color: [number, number, number];
  width: number;
  field?: PlacedField;
}

type State = Pick<EditState, "marks" | "fields" | "pages">;

/** The kind of placed field a field of the file is copied as, or why it is not copied. */
function kindOf(control: Form["widgets"][number]["control"], multiline: boolean): FieldKind | string {
  switch (control.kind) {
    case "text":
      return multiline ? "multiline" : "text";
    case "checkbox":
      return "checkbox";
    case "radio":
      return "radio";
    case "choice":
      return control.combo && !control.editable && !control.multiple
        ? "dropdown"
        : "A list, or a dropdown that takes text of its own, cannot be duplicated yet";
    case "signature":
      // A copy is a new place for a signature, whether or not this one is signed.
      return "signature";
    default:
      return "This kind of field cannot be duplicated";
  }
}

/** The properties a copy carries over, as a placed field holds them. */
function carried(now: ReturnType<typeof properties>): Partial<PlacedField> {
  if (!now) return {};
  return {
    ...(now.tooltip ? { tooltip: now.tooltip } : {}),
    ...(now.required ? { required: true } : {}),
    ...(now.readOnly ? { read_only: true } : {}),
    ...(now.maxLength ? { max_length: now.maxLength } : {}),
    ...(now.align && now.align !== "left" ? { align: now.align } : {}),
    ...(now.textSize ? { text_size: now.textSize } : {}),
    ...(now.defaultValue ? { default_value: now.defaultValue } : {}),
  };
}

/**
 * The copy of the mark `id` names, at `rect`, or why it cannot be copied.
 *
 * `border` is whether a copied field of the file gets a line round it: the
 * file does not say whether the original was placed with one.
 */
export function copyOf(
  id: number,
  rect: Rect,
  state: State,
  form: Form | null,
  formNames: readonly string[],
  border: boolean,
): Copy | string {
  const base = { quads: [...rect], color: [0.1, 0.35, 0.75] as [number, number, number], width: 1 };
  if (isSaved(id)) {
    const found = form ? savedField(form, state, id) : null;
    if (!form || !found) return "That field is no longer in the document";
    const kind = kindOf(found.widget.control, found.widget.multiline);
    if (kind !== "text" && kind !== "multiline" && kind !== "checkbox" && kind !== "dropdown" && kind !== "radio" && kind !== "signature") return kind;
    if (kind === "radio" && found.widget.name.includes(".")) return "A radio button inside a group of fields cannot be duplicated yet";
    const now = properties(form, state, id);
    const made = placing(
      { kind, options: kind === "dropdown" ? now?.options ?? [] : [], group: found.widget.name },
      border, formNames, state.marks, form,
    );
    return { ...base, kind: "field", page: found.page, note: made.name, field: { ...made.field, ...carried(now) } };
  }
  const mark = state.marks.find((one) => one.id === id);
  if (!mark) return "That mark is no longer on the page";
  const copied = { ...base, page: mark.page, color: mark.color, width: mark.width };
  if (mark.kind === "square" || mark.kind === "ellipse") return { ...copied, kind: mark.kind, note: mark.note };
  if (mark.kind !== "field" || !mark.field) return "Only form fields, boxes and ellipses are duplicated";
  const made = placing(
    { kind: mark.field.kind, options: mark.field.kind === "dropdown" ? mark.field.options ?? [] : [], group: mark.note },
    mark.field.border, formNames, state.marks, form,
  );
  const { kind: _kind, border: _border, options: _options, ...rest } = mark.field;
  return { ...copied, kind: "field", note: made.name, field: { ...made.field, ...rest } };
}

/** What {@link duplicate} needs from the window around it. */
export interface DuplicateDeps {
  /** Where the copies of the picked marks go, and the gesture they are made under. */
  copies(): { rects: { mark: number; rect: Rect }[]; sweep: number } | null;
  state(): State | null;
  form(): Form | null;
  formNames(): readonly string[];
  border(): boolean;
  /** Makes one mark as part of the gesture. */
  make(copy: Copy, sweep: number): Promise<unknown>;
  /** Picks the copies, in place of what was picked. */
  pick(ids: number[]): void;
  say(message: string): void;
}

/**
 * Copies every picked mark that can be copied, as one step of undo, and
 * picks the copies. Resolves with how many were made.
 *
 * One at a time, asking the model's state again before each: a copy's name
 * must differ from the copy made just before it.
 */
export async function duplicate(deps: DuplicateDeps): Promise<number> {
  const asked = deps.copies();
  if (!asked) return 0;
  const made: number[] = [];
  for (const { mark, rect } of asked.rects) {
    const state = deps.state();
    if (!state) break;
    const copy = copyOf(mark, rect, state, deps.form(), deps.formNames(), deps.border());
    if (typeof copy === "string") {
      deps.say(copy);
      continue;
    }
    const before = new Set(state.marks.map((one) => one.id));
    await deps.make(copy, asked.sweep);
    const now = deps.state()?.marks.find((one) => !before.has(one.id));
    if (now) made.push(now.id);
  }
  if (made.length > 0) deps.pick(made);
  return made.length;
}
