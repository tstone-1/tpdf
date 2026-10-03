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

import type { FieldKind, MarkView } from "./pages";

/** The word a kind's placeholder name begins with. */
const WORDS: Record<FieldKind, string> = {
  text: "Text",
  // One word for both text kinds: whether a field wraps is not part of what a
  // reader would call it.
  multiline: "Text",
  checkbox: "Checkbox",
};

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

/** The first `Text n` or `Checkbox n`, counting from 1, that is not taken. */
export function nextFieldName(kind: FieldKind, taken: ReadonlySet<string>): string {
  const word = WORDS[kind];
  for (let n = 1; ; n++) {
    const name = `${word} ${n}`;
    if (!taken.has(name)) return name;
  }
}
