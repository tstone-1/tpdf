/**
 * The name a form field gets when a reader places one.
 *
 * A field has to have a name the moment it exists, because the name is what
 * an answer is addressed to and what the form's list holds, and a reader who
 * has just dragged a rectangle has not been asked for one. So it is given one
 * a reader would recognise as a placeholder, and they rename it in the box
 * that opens: `Text 1`, `Checkbox 2`.
 *
 * What the name must not be is one the form already has. Two things hold
 * names: the form in the file, and the fields placed since it was opened,
 * which are marks. `formfields.rs` refuses a clash with either, the second
 * when the field is placed and the first at the save; this picks a name that
 * clashes with neither, so that neither refusal is what a reader meets.
 */

import type { Form } from "./forms";
import type { FieldKind, MarkView, PlacedField } from "./pages";

/**
 * The least a side of a field of this kind may be, in points: `formfields.rs`'s
 * `least_side`. A signature field's is the least a visible signature is drawn in.
 */
export function leastSide(kind: FieldKind | undefined): number {
  return kind === "checkbox" || kind === "radio" ? 6 : kind === "signature" ? 24 : 8;
}

/** The word a kind's placeholder name begins with. */
const WORDS: Record<FieldKind, string> = {
  text: "Text",
  // One word for both text kinds: whether a field wraps is not part of what a
  // reader would call it.
  multiline: "Text",
  checkbox: "Checkbox",
  dropdown: "Dropdown",
  // A radio button's name is its group's, which the reader types; this is
  // only what a group is called when nothing asked.
  radio: "Group",
  signature: "Signature",
};

/** The most choices a dropdown offers, and the most characters in one. `formfields.rs` holds both. */
const MAX_OPTIONS = 1000;
const MAX_OPTION = 255;

/**
 * The choices a reader typed for a dropdown, or why they cannot be its list.
 *
 * Typed on one line with semicolons between them, since a comma is ordinary
 * in a choice ("Yes, by post") and a semicolon rarely is. Space round each is
 * dropped, and so is a semicolon at the end. The rules are the save's, asked
 * here first so a reader is told before they have dragged anything: at least
 * one choice, none empty, none twice.
 */
export function parseChoices(raw: string): { options: string[] } | { problem: string } {
  const typed = raw.trim().replace(/;\s*$/, "");
  if (typed === "") return { problem: "The choices, with a semicolon between them: Yes; No; Maybe" };
  const options = typed.split(";").map((part) => part.trim());
  if (options.some((option) => option === "")) return { problem: "There is an empty choice between two semicolons" };
  if (options.length > MAX_OPTIONS) return { problem: `A dropdown offers at most ${MAX_OPTIONS} choices` };
  const long = options.find((option) => [...option].length > MAX_OPTION);
  if (long !== undefined) return { problem: `A choice is at most ${MAX_OPTION} characters` };
  // A character a reader cannot see cannot be told apart in a list either.
  if (options.some((option) => /\p{Cc}/u.test(option))) return { problem: "A choice cannot contain a control character" };
  const twice = options.find((option, at) => options.indexOf(option) !== at);
  if (twice !== undefined) return { problem: `"${twice}" is there twice` };
  return { options };
}

/**
 * The names a new field may not take.
 *
 * A name in the file's form is taken by its first part: a field `Address.City`
 * sits under a group `Address`, and a new field is always added at the top, so
 * `Address` is what it would collide with.
 */
export function takenNames(
  formNames: readonly string[],
  marks: readonly Pick<MarkView, "kind" | "note">[],
): Set<string> {
  const taken = new Set<string>();
  for (const name of formNames) taken.add(name.split(".")[0] ?? name);
  for (const mark of marks) if (mark.kind === "field") taken.add(mark.note);
  return taken;
}

type Store = Pick<Storage, "getItem" | "setItem">;

const BORDER_KEY = "tpdf.fieldBorder";

/**
 * Whether a text field placed from now on is drawn with a line round it.
 *
 * On unless the reader turned it off. An empty text field otherwise draws
 * nothing once saved, so a field placed on a blank part of a page would be
 * something its maker cannot find again; a reader placing fields on a page
 * that already prints its own lines turns this off. Storage that throws, or
 * that holds anything tpdf did not write, reads as on.
 */
export function readFieldBorder(storage: () => Store = () => window.localStorage): boolean {
  try {
    return storage().getItem(BORDER_KEY) !== "false";
  } catch {
    return true;
  }
}

/** Keeps the choice; `false` when storage refused, which changes nothing now. */
export function writeFieldBorder(
  border: boolean,
  storage: () => Store = () => window.localStorage,
): boolean {
  try {
    storage().setItem(BORDER_KEY, String(border));
    return true;
  } catch {
    return false;
  }
}

/**
 * Whether the tab order can be asked for: there is a field to order, the
 * file's or one placed since, and the order is not asked for already.
 */
export function canOrderTabs(
  state: { tab_order?: boolean; marks: readonly Pick<MarkView, "kind">[] } | null,
  form: Form | null,
): boolean {
  if (!state || state.tab_order) return false;
  return (form?.widgets.length ?? 0) > 0 || state.marks.some((mark) => mark.kind === "field");
}

/**
 * The name a reader typed for a group of radio buttons, or why it cannot be
 * one. The rules are `formfields.rs`'s for any field's name.
 */
export function parseGroup(raw: string): { group: string } | { problem: string } {
  const group = raw.trim();
  if (group === "") return { problem: "The name of the group this button belongs to, such as Payment" };
  if (group.includes(".")) return { problem: "A field's name cannot contain a period" };
  if (/\p{Cc}/u.test(group)) return { problem: "A field's name cannot contain a control character" };
  if ([...group].length > 255) return { problem: "A field's name is at most 255 characters" };
  return { group };
}

/**
 * The values a group's buttons already have: the ones placed in this session
 * and the ones the file's form has under that name.
 */
function radioValues(group: string, marks: readonly MarkView[], form: Form | null): Set<string> {
  const values = new Set<string>();
  for (const mark of marks) {
    if (mark.kind === "field" && mark.note === group && mark.field?.kind === "radio") {
      for (const value of mark.field.options ?? []) values.add(value);
    }
  }
  for (const widget of form?.widgets ?? []) {
    if (widget.name !== group || widget.control.kind !== "radio") continue;
    for (const state of widget.control.states) values.add(new TextDecoder().decode(new Uint8Array(state)));
  }
  return values;
}

/** What the field tool is armed with: a kind, a dropdown's choices, a radio button's group. */
export interface ArmedField {
  kind: FieldKind;
  options: string[];
  group: string;
}

/**
 * The field a drag places and the name it gets.
 *
 * A radio button is named for its group and given the first `Choice n` no
 * button of that group has; every other kind gets a name no field has.
 */
export function placing(
  armed: ArmedField,
  border: boolean,
  formNames: readonly string[],
  marks: readonly MarkView[],
  form: Form | null,
): { field: PlacedField; name: string } {
  if (armed.kind === "radio") {
    const taken = radioValues(armed.group, marks, form);
    let n = 1;
    while (taken.has(`Choice ${n}`)) n += 1;
    return { field: { kind: "radio", border: false, options: [`Choice ${n}`] }, name: armed.group };
  }
  return {
    field: { kind: armed.kind, border, ...(armed.options.length > 0 ? { options: armed.options } : {}) },
    name: nextFieldName(armed.kind, takenNames(formNames, marks)),
  };
}

/** The first `Text n` or `Checkbox n`, counting from 1, that is not taken. */
export function nextFieldName(kind: FieldKind, taken: ReadonlySet<string>): string {
  const word = WORDS[kind];
  for (let n = 1; ; n++) {
    const name = `${word} ${n}`;
    if (!taken.has(name)) return name;
  }
}
