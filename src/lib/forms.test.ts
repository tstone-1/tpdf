import { describe, expect, it } from "vitest";
import { answerError, fieldValue, sameAnswer, shownAlign, shownChoices, shownValue, standingChoice, type FormWidget } from "./forms";

const text: FormWidget = {
  object: [12, 0], widget: [13, 0], page: 0,
  rect: [0, 0, 100, 20], display_rect: [0, 0, 100, 20],
  name: "ACME.answer", value: "original", control: { kind: "text" }, multiline: false,
  max_length: 20, reason: null,
};

describe("what a field's control shows", () => {
  const empty: FormWidget = { ...text, value: "" };
  // The same field shown a second time: another widget, one object.
  const twin: FormWidget = { ...empty, widget: [14, 0] };
  const other: FormWidget = { ...empty, object: [20, 0], widget: [21, 0] };
  const all = [empty, twin, other];
  const given = (object: [number, number], value: string, removed = false) =>
    ({ object, page: 5, removed, props: { default_value: value } });

  it("is the answer, pending or the file's, when the field holds one", () => {
    expect(shownValue(text, [], [given([13, 0], "n/a")], [text])).toBe("original");
    expect(shownValue(empty, [{ object: [12, 0], value: "typed" }], [given([13, 0], "n/a")], all)).toBe("typed");
    expect(shownValue(empty, [], [], all)).toBe("");
  });

  it("is the default a reader has given a field that holds nothing, as the save will answer it", () => {
    expect(shownValue(empty, [], [given([13, 0], "n/a")], all)).toBe("n/a");
    // Cleared by the reader: the save answers it with the default all the same.
    expect(shownValue(empty, [{ object: [12, 0], value: "" }], [given([13, 0], "n/a")], all)).toBe("n/a");
    // Held under the field's other widget, also when that one was removed.
    expect(shownValue(empty, [], [given([14, 0], "n/a")], all)).toBe("n/a");
    expect(shownValue(empty, [], [given([14, 0], "n/a", true)], all)).toBe("n/a");
    // Another field's default is not this one's, and a change with no default gives none.
    expect(shownValue(empty, [], [given([21, 0], "n/a")], all)).toBe("");
    expect(shownValue(empty, [], [{ object: [13, 0], page: 5, removed: false, props: { tooltip: "x" } }], all)).toBe("");
    // A default taken off again.
    expect(shownValue(empty, [], [given([13, 0], "")], all)).toBe("");
  });

  it("is never a default for a field that is not a text field", () => {
    const box: FormWidget = { ...empty, value: false, control: { kind: "checkbox" } };
    expect(shownValue(box, [], [given([13, 0], "n/a")], [box])).toBe(false);
    const list: FormWidget = { ...empty, value: "", control: { kind: "choice", combo: true, editable: true, multiple: false, options: [] } };
    expect(shownValue(list, [], [given([13, 0], "n/a")], [list])).toBe("");
    // A kind that is neither text nor a list, holding an empty string.
    const odd: FormWidget = { ...empty, control: { kind: "unsupported" } };
    expect(shownValue(odd, [], [given([13, 0], "n/a")], [odd])).toBe("");
  });
});

describe("the choices and the alignment a control shows before the save", () => {
  const list: FormWidget = {
    ...text, value: [1], align: "right",
    control: { kind: "choice", combo: true, editable: false, multiple: false,
      options: [{ export: "y", label: "Yes" }, { export: "n", label: "No" }, { export: "n2", label: "No" }, { export: "l", label: "Later" }] },
  };
  const twin: FormWidget = { ...list, widget: [14, 0] };
  const changed = (object: [number, number], props: object, removed = false) => ({ object, page: 5, removed, props });

  it("are the file's while a reader has changed neither", () => {
    expect(shownChoices(list, [], [list])).toBeNull();
    expect(shownChoices(list, [changed([13, 0], { tooltip: "x" })], [list])).toBeNull();
    expect(shownChoices(text, [changed([13, 0], { options: ["a"] })], [text])).toBeNull();
    expect(shownAlign(list, [], [list])).toBe("right");
    expect(shownAlign(text, [], [text])).toBe("left");
    // With the file's choices an answer is the file's own, two equal labels apart.
    expect(shownValue({ ...list, value: [2] }, [], [], [list])).toEqual([2]);
  });

  it("are the ones a reader has just given, each at the first place the file has its label", () => {
    const to = [changed([13, 0], { options: ["Later", "No", "Maybe"], align: "center" })];
    expect(shownChoices(list, to, [list])).toEqual([
      { label: "Later", index: 3 }, { label: "No", index: 1 }, { label: "Maybe", index: null },
    ]);
    expect(shownAlign(list, to, [list])).toBe("center");
    // Held under the field's other widget, also when that one was removed.
    expect(shownChoices(list, [changed([14, 0], { options: ["Maybe"] }, true)], [list, twin])).toEqual([{ label: "Maybe", index: null }]);
    expect(shownAlign(list, [changed([14, 0], { align: "left" })], [list, twin])).toBe("left");
    // Another field's are not this one's.
    expect(shownChoices(list, [changed([99, 0], { options: ["Maybe"] })], [list, twin])).toBeNull();
    expect(shownAlign(list, [changed([99, 0], { align: "center" })], [list, twin])).toBe("right");
  });

  it("keep chosen what the new choices still have by label, as the save does", () => {
    const to = [changed([13, 0], { options: ["Later", "No", "Maybe"] })];
    expect(shownValue(list, [], to, [list])).toEqual([1]);
    // The second "No" of the file is shown where the control offers "No".
    expect(shownValue({ ...list, value: [2] }, [], to, [list])).toEqual([1]);
    // "Yes" is gone, so nothing is chosen; a pending answer is treated as the file's is.
    expect(shownValue({ ...list, value: [0] }, [], to, [list])).toEqual([]);
    expect(shownValue(list, [{ object: [12, 0], value: [0, 3] }], to, [list])).toEqual([3]);
    expect(standingChoice([{ label: "a" }, { label: "b" }, { label: "a" }], [{ label: "b", index: 1 }, { label: "a", index: 0 }], [2, 1, 0])).toEqual([0, 1]);
    expect(standingChoice([{ label: "a" }], [{ label: "a", index: null }], [0, 7])).toEqual([]);
    // Text typed into a list that takes it is not a choice and stays.
    expect(shownValue({ ...list, value: "own words" }, [], to, [list])).toBe("own words");
  });
});

describe("form answers", () => {
  const choice: FormWidget = { ...text, value: [0], control: { kind: "choice", combo: true, editable: false, multiple: false,
    options: [{ export: "SAME", label: "First" }, { export: "SAME", label: "Second" }, { export: "OTHER", label: "Third" }] } };
  const radio: FormWidget = { ...text, value: [0], control: { kind: "radio", index: 0, states: [[65], [66]], unison: false, no_toggle_off: true } };
  it("keeps duplicate exports distinct and compares selections across IPC replies", () => {
    expect(fieldValue(choice, [{ object: choice.object, value: [1] }])).toEqual([1]);
    expect(sameAnswer([0, 2], [0, 2])).toBe(true);
    expect(sameAnswer([0], [1])).toBe(false);
    expect(sameAnswer([], "")).toBe(false);
    expect(sameAnswer([0, 2], [0])).toBe(false);
  });
  it("validates option indices and selection cardinality", () => {
    expect(answerError(choice, [1])).toBeNull();
    expect(answerError(choice, [])).toBeNull();
    for (const answer of [[3], [-1], [0.5], [NaN], [0, 1], [1, 1], "SAME", true]) expect(answerError(choice, answer)).not.toBeNull();
    const list: FormWidget = { ...choice, control: { ...choice.control as Extract<FormWidget["control"], { kind: "choice" }>, combo: false, multiple: true } };
    expect(answerError(list, [0, 2])).toBeNull();
    expect(answerError(list, [2, 0])).not.toBeNull();
    expect(answerError(list, [1, 1])).not.toBeNull();
    expect(answerError(radio, [1])).toBeNull();
    expect(answerError(radio, [])).not.toBeNull();
    expect(answerError(radio, [0, 1])).not.toBeNull();
    expect(answerError(radio, true)).not.toBeNull();
  });
  it("accepts custom text only in editable dropdowns", () => {
    const editable: FormWidget = { ...choice, control: { ...choice.control as Extract<FormWidget["control"], { kind: "choice" }>, editable: true } };
    expect(answerError(editable, "Custom answer")).toBeNull();
    expect(answerError(editable, "漢")).not.toBeNull();
    expect(answerError(editable, [2])).toBeNull();
    expect(answerError(editable, false)).not.toBeNull();
  });
  it("clears a field, shares an answer, and restores the original on undo", () => {
    const other = { ...text, widget: [14, 0] as [number, number], page: 1 };
    expect(fieldValue(text, [{ object: [12, 0], value: "" }])).toBe("");
    expect(fieldValue(other, [{ object: [12, 0], value: "new" }])).toBe("new");
    expect(fieldValue(text, [])).toBe("original");
    expect(fieldValue(text, [{ object: [12, 1], value: "wrong generation" }])).toBe("original");
  });
  it("does not confuse an unchecked checkbox with an absent answer", () => {
    expect(fieldValue({ ...text, value: true }, [{ object: [12, 0], value: false }])).toBe(false);
  });
  it("accepts supported text and refuses loss, wrong types and field restrictions", () => {
    expect(answerError(text, "Grüße")).toBeNull();
    expect(answerError(text, "")).toBeNull();
    expect(answerError(text, "a\nb")).not.toBeNull();
    expect(answerError({ ...text, multiline: true }, "a\nb")).toBeNull();
    expect(answerError(text, "漢")).not.toBeNull();
    expect(answerError(text, "x".repeat(21))).not.toBeNull();
    expect(answerError(text, false)).not.toBeNull();
    expect(answerError({ ...text, reason: "Read-only" }, "new")).toBe("Read-only");
    expect(answerError({ ...text, value: false, control: { kind: "checkbox" } }, true)).toBeNull();
  });
});
