import { describe, expect, it } from "vitest";

import { nextFieldName, takenNames } from "./fieldnames";

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
