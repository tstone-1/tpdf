/**
 * The sentences of text recognition, and the one name this side shares with
 * `commands/ocr.rs` by spelling.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/commands/ocr.rs?raw";
import layer from "../../src-tauri/src/ocr_layer.rs?raw";
import {
  afterRecognition,
  PROGRESS_EVENT,
  progressLine,
  REFUSED_MEANS,
  SAVE_FIRST,
  suggestedName,
  type Recognised,
} from "./recognise";

function read(over: Partial<Recognised> = {}): Recognised {
  return {
    pages: [{ page: 1, words: 40 }],
    alreadyText: [],
    nothingRead: [],
    refused: [],
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
      ["alreadyText", "engine", "nothingRead", "pages", "refused", "tooLarge"].sort(),
    );
    for (const key of [
      "alreadyText",
      "nothingRead",
      "refused",
      "tooLarge",
      "languageUnavailable",
    ]) {
      expect(backend).toContain(`"${key}"`);
    }
  });
});

describe("what the window shares with the layer's rules by spelling", () => {
  it("says what a refused page usually is in the backend's own words", () => {
    const declared = /pub const REFUSED_MEANS: &str =\s*"([^"]+)";/.exec(layer)?.[1];
    // The control, as above: no match would compare against `undefined`.
    expect(declared).toBeDefined();
    expect(REFUSED_MEANS).toBe(declared);
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

  it("says which language was missing when the recogniser had to choose", () => {
    const said = afterRecognition(read({ languageUnavailable: "de-DE" }), "a.pdf");
    expect(said).toBe(
      "Saved a.pdf. Text was added to 1 of 1 page (40 words). " +
        "German (Germany), de-DE is not available on this computer, " +
        'so the recogniser chose the language itself. ' +
        'Choose another with "Recognise text: language...".',
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

  it("names the pages the recogniser refused, says what that usually means, and counts them", () => {
    const one = read({ refused: [2] });
    expect(afterRecognition(one, "a.pdf")).toBe(
      "Saved a.pdf. Text was added to 1 of 2 pages (40 words). " +
        "The recogniser refused page 2, which usually means a script it cannot read " +
        "or a scan too unclear to tell the script.",
    );
    const more = read({ nothingRead: [3], refused: [2, 4] });
    const said = afterRecognition(more, "a.pdf");
    expect(said).toContain("1 of 4 pages");
    expect(said).toContain("No text was recognised on page 3. The recogniser refused pages 2, 4, which");
    // Nothing is said about refusing when no page was refused.
    expect(afterRecognition(read(), "a.pdf")).not.toContain("refused");
  });

  it("does not list three hundred pages", () => {
    const many = read({ alreadyText: Array.from({ length: 300 }, (_, i) => i + 2) });
    const said = afterRecognition(many, "a.pdf");
    expect(said).toContain("Already had text: pages 2, 3, 4, 5, 6, 7, 8, 9 and 292 more.");
    expect(said).toContain("1 of 301 pages");
  });
});
