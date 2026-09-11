import { describe, expect, it } from "vitest";
import { answerError, fieldValue, sameAnswer, type FormWidget } from "./forms";

const text: FormWidget = {
  object: [12, 0], widget: [13, 0], page: 0,
  rect: [0, 0, 100, 20], display_rect: [0, 0, 100, 20],
  name: "ACME.answer", value: "original", control: { kind: "text" }, multiline: false,
  max_length: 20, reason: null,
};

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
