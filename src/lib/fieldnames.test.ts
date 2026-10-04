import { describe, expect, it } from "vitest";

import type { Form, FormWidget } from "./forms";
import {
  leastSide,
  canOrderTabs, nextFieldName, parseChoices, parseGroup, placing, readFieldBorder, takenNames, writeFieldBorder,
} from "./fieldnames";
import { pageId, type MarkView, type PlacedField } from "./pages";

describe("the name a placed form field starts with", () => {
  it("counts from one, by the kind's word", () => {
    const none = new Set<string>();
    expect(nextFieldName("text", none)).toBe("Text 1");
    expect(nextFieldName("multiline", none)).toBe("Text 1");
    expect(nextFieldName("checkbox", none)).toBe("Checkbox 1");
  });

  it("takes the first number that is free, not one past the highest", () => {
    expect(nextFieldName("text", new Set(["Text 1", "Text 2", "Text 4"]))).toBe("Text 3");
    expect(nextFieldName("checkbox", new Set(["Text 1", "Checkbox 1"]))).toBe("Checkbox 2");
    // Another kind's names are not in the way.
    expect(nextFieldName("text", new Set(["Checkbox 1"]))).toBe("Text 1");
  });

  it("counts the file's fields by their first part and the placed ones by their name", () => {
    const taken = takenNames(
      ["Text 1", "Text 2.left", "Address.City"],
      [
        { kind: "field", note: "Text 3" },
        // A highlight whose note happens to read like a name holds no name.
        { kind: "highlight", note: "Text 4" },
      ],
    );
    expect([...taken].sort()).toEqual(["Address", "Text 1", "Text 2", "Text 3"]);
    expect(nextFieldName("text", taken)).toBe("Text 4");
    expect(takenNames([], []).size).toBe(0);
  });
});

describe("whether a new text field gets a line round it", () => {
  const store = (seed: Record<string, string> = {}) => {
    const kept = new Map(Object.entries(seed));
    return {
      getItem: (key: string) => kept.get(key) ?? null,
      setItem: (key: string, value: string) => void kept.set(key, value),
    };
  };

  it("is on until it is turned off, and remembers either", () => {
    const kept = store();
    expect(readFieldBorder(() => kept)).toBe(true);
    expect(writeFieldBorder(false, () => kept)).toBe(true);
    expect(kept.getItem("tpdf.fieldBorder")).toBe("false");
    expect(readFieldBorder(() => kept)).toBe(false);
    expect(writeFieldBorder(true, () => kept)).toBe(true);
    expect(readFieldBorder(() => kept)).toBe(true);
  });

  it("reads anything tpdf did not write as on, and survives storage that throws", () => {
    expect(readFieldBorder(() => store({ "tpdf.fieldBorder": "no" }))).toBe(true);
    expect(readFieldBorder(() => store({ "tpdf.fieldBorder": "" }))).toBe(true);
    const broken = () => {
      throw new Error("denied");
    };
    expect(readFieldBorder(broken)).toBe(true);
    expect(writeFieldBorder(false, broken)).toBe(false);
  });
});

describe("the choices typed for a dropdown", () => {
  const options = (raw: string) => {
    const parsed = parseChoices(raw);
    return "options" in parsed ? parsed.options : parsed.problem;
  };

  it("are split at semicolons, with the space round each dropped", () => {
    expect(options("Yes;No")).toEqual(["Yes", "No"]);
    expect(options("  Yes ;  No, by post ; Maybe  ")).toEqual(["Yes", "No, by post", "Maybe"]);
    expect(options("Only")).toEqual(["Only"]);
    // A semicolon at the end is how a list is typed, not an empty choice.
    expect(options("Yes; No;")).toEqual(["Yes", "No"]);
    expect(options("Yes; No ;  ")).toEqual(["Yes", "No"]);
  });

  it("need at least one, none empty and none twice", () => {
    expect(options("")).toContain("semicolon between them");
    expect(options("   ")).toContain("semicolon between them");
    expect(options(";")).toContain("semicolon between them");
    expect(options("Yes;;No")).toBe("There is an empty choice between two semicolons");
    expect(options(";Yes")).toBe("There is an empty choice between two semicolons");
    expect(options("Yes; No; Yes")).toBe('"Yes" is there twice');
    // Case is a difference a reader can see.
    expect(options("Yes; yes")).toEqual(["Yes", "yes"]);
  });

  it("are held to the most a dropdown offers and the longest a choice may be", () => {
    const many = Array.from({ length: 1001 }, (_, n) => String(n)).join(";");
    expect(options(many)).toBe("A dropdown offers at most 1000 choices");
    expect(options(Array.from({ length: 1000 }, (_, n) => String(n)).join(";"))).toHaveLength(1000);
    expect(options(`a;${"x".repeat(256)}`)).toBe("A choice is at most 255 characters");
    expect(options(`a;${"x".repeat(255)}`)).toHaveLength(2);
    // Counted in characters, not in the units a string stores them in.
    // A clef is one character and two units: 200 of them are 200, not 400.
    expect(options("\u{1D11E}".repeat(200))).toHaveLength(1);
    expect(options("a;b\u0007c")).toBe("A choice cannot contain a control character");
  });
});

describe("a dropdown's placeholder name", () => {
  it("is counted on its own, like a checkbox's", () => {
    expect(nextFieldName("dropdown", new Set())).toBe("Dropdown 1");
    expect(nextFieldName("dropdown", new Set(["Dropdown 1", "Text 1"]))).toBe("Dropdown 2");
  });
});

describe("the group typed for radio buttons", () => {
  it("is the name with the space round it dropped", () => {
    expect(parseGroup("  Payment ")).toEqual({ group: "Payment" });
    expect(parseGroup("How you pay")).toEqual({ group: "How you pay" });
  });

  it("is held to what a field's name may be", () => {
    const problem = (raw: string) => {
      const parsed = parseGroup(raw);
      return "problem" in parsed ? parsed.problem : null;
    };
    expect(problem("")).toContain("name of the group");
    expect(problem("   ")).toContain("name of the group");
    expect(problem("a.b")).toContain("period");
    expect(problem("a\tb")).toContain("control character");
    expect(problem("x".repeat(256))).toContain("at most 255");
    expect(problem("x".repeat(255))).toBeNull();
  });
});

describe("the field a drag places", () => {
  const mark = (id: number, note: string, field?: PlacedField): MarkView => ({
    id, kind: field ? "field" : "square", stamp: null, page: pageId(5), quads: [0, 0, 10, 10], strokes: [],
    color: [0, 0, 0], width: 1, note, lines: [], ...(field ? { field } : {}),
  } as MarkView);
  const radio = (id: number, group: string, value: string) => mark(id, group, { kind: "radio", border: false, options: [value] });
  const bytes = (text: string) => [...new TextEncoder().encode(text)];
  const saved = (name: string, states: string[]): FormWidget => ({
    object: [9, 0], widget: [10, 0], page: 0, rect: [0, 0, 0, 0], display_rect: [0, 0, 10, 10], name, value: [],
    control: { kind: "radio", index: 0, states: states.map(bytes), unison: false, no_toggle_off: true },
    multiline: false, max_length: null, reason: null,
  });
  const armed = (kind: PlacedField["kind"], extra: { options?: string[]; group?: string } = {}) =>
    ({ kind, options: extra.options ?? [], group: extra.group ?? "" });

  it("is a text field or a checkbox under a name no field has, with the line asked for", () => {
    expect(placing(armed("text"), true, ["Text 1"], [mark(1, "Text 2", { kind: "text", border: true })], null)).toEqual({
      field: { kind: "text", border: true }, name: "Text 3",
    });
    expect(placing(armed("checkbox"), false, [], [], null)).toEqual({
      field: { kind: "checkbox", border: false }, name: "Checkbox 1",
    });
  });

  it("is a signature field under the next name of its kind, which is at least 24 points a side", () => {
    expect(placing(armed("signature"), true, ["Signature 1"], [], null)).toEqual({
      field: { kind: "signature", border: true }, name: "Signature 2",
    });
    expect([leastSide("signature"), leastSide("text"), leastSide("multiline"), leastSide("dropdown"),
      leastSide("checkbox"), leastSide("radio"), leastSide(undefined)]).toEqual([24, 8, 8, 8, 6, 6, 8]);
  });

  it("is a dropdown holding the choices it was armed with", () => {
    expect(placing(armed("dropdown", { options: ["A", "B"] }), true, [], [], null)).toEqual({
      field: { kind: "dropdown", border: true, options: ["A", "B"] }, name: "Dropdown 1",
    });
  });

  it("is a radio button named for its group, with the first value the group does not have", () => {
    expect(placing(armed("radio", { group: "Pay" }), true, [], [], null)).toEqual({
      field: { kind: "radio", border: false, options: ["Choice 1"] }, name: "Pay",
    });
    const marks = [radio(1, "Pay", "Choice 1"), radio(2, "Pay", "Choice 3"), radio(3, "Send", "Choice 2")];
    expect(placing(armed("radio", { group: "Pay" }), true, [], marks, null).field.options).toEqual(["Choice 2"]);
    // A name that only reads like the group's holds no value of it: a text
    // field, and a mark that is no field.
    const others = [mark(4, "Pay", { kind: "text", border: false, options: ["Choice 1"] }), mark(5, "Pay")];
    expect(placing(armed("radio", { group: "Pay" }), true, [], others, null).field.options).toEqual(["Choice 1"]);
  });

  it("counts the values the file's group has as well", () => {
    const form: Form = { widgets: [saved("Pay", ["Choice 1", "Choice 2"]), saved("Send", ["Choice 4"])] };
    expect(placing(armed("radio", { group: "Pay" }), true, [], [radio(1, "Pay", "Choice 3")], form).field.options)
      .toEqual(["Choice 4"]);
    const text: Form = { widgets: [{ ...saved("Pay", []), control: { kind: "text" } }] };
    expect(placing(armed("radio", { group: "Pay" }), true, [], [], text).field.options).toEqual(["Choice 1"]);
  });
});

describe("whether the tab order can be asked for", () => {
  const widget = { name: "Name" } as FormWidget;
  const field = { kind: "field" } as const;
  const box = { kind: "square" } as const;

  it("needs a field: one of the file's, or one placed since", () => {
    expect(canOrderTabs({ marks: [] }, { widgets: [widget] })).toBe(true);
    expect(canOrderTabs({ marks: [box, field] }, null)).toBe(true);
    expect(canOrderTabs({ marks: [box] }, { widgets: [] })).toBe(false);
    expect(canOrderTabs({ marks: [] }, null)).toBe(false);
  });

  it("is not offered again once asked for, nor with no document", () => {
    expect(canOrderTabs({ tab_order: true, marks: [field] }, { widgets: [widget] })).toBe(false);
    expect(canOrderTabs({ tab_order: false, marks: [field] }, null)).toBe(true);
    expect(canOrderTabs(null, { widgets: [widget] })).toBe(false);
  });
});
