/**
 * The sentences of text recognition, and the one name this side shares with
 * `commands/ocr.rs` by spelling.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/commands/ocr.rs?raw";
import {
  afterRecognition,
  PROGRESS_EVENT,
  progressLine,
  SAVE_FIRST,
  suggestedName,
  type Recognised,
} from "./recognise";

function read(over: Partial<Recognised> = {}): Recognised {
  return {
    pages: [{ page: 1, words: 40 }],
    alreadyText: [],
    nothingRead: [],
    tooLarge: [],
    engine: "vision",
    ...over,
  };
}

describe("what the window shares with the backend by spelling", () => {
  it("listens for the event the backend emits", () => {
    const declared = /pub const PROGRESS_EVENT: &str = "([^"]+)";/.exec(backend)?.[1];
    // The control: a regex that stopped matching would compare against
    // `undefined`, which no event name equals, so this cannot pass by absence.
    expect(declared).toBeDefined();
    expect(PROGRESS_EVENT).toBe(declared);
  });

  it("refuses unsaved changes in the backend's own words", () => {
    const declared = /pub const UNSAVED: &str = "([^;]+)";/.exec(backend)?.[1];
    expect(declared).toBeDefined();
    // The Rust literal is wrapped with a line continuation; the sentence is not.
    expect(declared?.replace(/\\\n\s*/g, "")).toBe(SAVE_FIRST);
  });

  it("names every field the backend's report serialises", () => {
    // `the_report_reaches_the_window_in_its_own_spelling` pins the same keys
    // from the other side; a field added there and not here is red in one.
    expect(Object.keys(read()).sort()).toEqual(
      ["alreadyText", "engine", "nothingRead", "pages", "tooLarge"].sort(),
    );
    for (const key of ["alreadyText", "nothingRead", "tooLarge"]) {
      expect(backend).toContain(`"${key}"`);
    }
  });
});

describe("suggestedName", () => {
  it("keeps the name and says what the copy is", () => {
    expect(suggestedName("/home/a/Scan 12.pdf")).toBe("Scan 12 searchable.pdf");
    expect(suggestedName("C:\\docs\\SCAN.PDF")).toBe("SCAN searchable.pdf");
  });
});

describe("progressLine", () => {
  it("says which page of how many", () => {
    expect(progressLine({ page: 3, of: 12 })).toBe("Recognising text: page 3 of 12...");
  });
});

describe("afterRecognition", () => {
  it("counts the pages and the words", () => {
    expect(afterRecognition(read(), "a searchable.pdf")).toBe(
      "Saved a searchable.pdf. Text was added to 1 of 1 page (40 words).",
    );
  });

  it("adds the words of every page given a layer", () => {
    const two = read({
      pages: [
        { page: 1, words: 40 },
        { page: 3, words: 1 },
      ],
    });
    expect(afterRecognition(two, "a.pdf")).toContain("2 of 2 pages (41 words)");
    const one = read({ pages: [{ page: 1, words: 1 }] });
    expect(afterRecognition(one, "a.pdf")).toContain("(1 word)");
  });

  it("names the pages that got no layer, each kind in its own sentence", () => {
    const mixed = read({ alreadyText: [2], nothingRead: [3, 5], tooLarge: [4] });
    expect(afterRecognition(mixed, "a.pdf")).toBe(
      "Saved a.pdf. Text was added to 1 of 5 pages (40 words). " +
        "Already had text: page 2. " +
        "No text was recognised on pages 3, 5. " +
        "Too large to read: page 4.",
    );
  });

  it("does not list three hundred pages", () => {
    const many = read({ alreadyText: Array.from({ length: 300 }, (_, i) => i + 2) });
    const said = afterRecognition(many, "a.pdf");
    expect(said).toContain("Already had text: pages 2, 3, 4, 5, 6, 7, 8, 9 and 292 more.");
    expect(said).toContain("1 of 301 pages");
  });
});
