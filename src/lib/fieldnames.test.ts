import { describe, expect, it } from "vitest";

import { nextFieldName, readFieldBorder, takenNames, writeFieldBorder } from "./fieldnames";

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
