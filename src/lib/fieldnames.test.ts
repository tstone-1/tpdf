import { describe, expect, it } from "vitest";

import { nextFieldName, parseChoices, readFieldBorder, takenNames, writeFieldBorder } from "./fieldnames";

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
