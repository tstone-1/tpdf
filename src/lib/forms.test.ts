import { describe, expect, it } from "vitest";
import { answerError, fieldValue, type FormWidget } from "./forms";

const text: FormWidget = {
  object: [12, 0], widget: [13, 0], page: 0,
  rect: [0, 0, 100, 20], display_rect: [0, 0, 100, 20],
  name: "ACME.answer", value: "original", multiline: false,
  max_length: 20, reason: null,
};

describe("form answers", () => {
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
    expect(answerError({ ...text, value: false }, true)).toBeNull();
  });
});
