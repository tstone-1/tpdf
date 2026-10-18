/**
 * What the window decides about page numbers and watermarks before the writer
 * sees them, and the names and defaults it shares with `addedtext.rs`.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/addedtext.rs?raw";
import { afterAdding, defaults, judge, POSITIONS, suggestedName, type Typed } from "./addtext";

const typed = (over: Partial<Typed> = {}): Typed => ({
  text: "Page {page} of {pages}", position: "bottom-center", size: "10", extra: "1", pages: "", ...over,
});

describe("the names and defaults are addedtext.rs's", () => {
  it("spells every position as the backend does, in its order", () => {
    const names = [...backend.matchAll(/Position::\w+ => "([a-z-]+)",/g)].map((m) => m[1]);
    expect(names).toEqual([...POSITIONS]);
  });

  it("starts from the backend's two sets of defaults", () => {
    const body = (name: string) => new RegExp(`pub fn ${name}\\([^)]*\\) -> Self \\{([\\s\\S]*?)\\n    \\}`).exec(backend)?.[1] ?? "";
    const numbers = body("page_numbers"), watermark = body("watermark");
    expect(numbers).toContain('text: "Page {page} of {pages}".into()');
    expect(numbers).toContain("position: Position::BottomCenter");
    expect(numbers).toContain(`size: ${defaults("numbers").size}.0`);
    expect(watermark).toContain("position: Position::Center");
    expect(watermark).toContain(`size: ${defaults("watermark").size}.0`);
    expect(watermark).toContain(`opacity: ${defaults("watermark").opacity}`);
    expect(watermark).toContain(`angle: ${defaults("watermark").angle}.0`);
    expect(backend).toContain(`pub const MARGIN: f32 = ${defaults("numbers").margin}.0;`);
  });
});

describe("judge", () => {
  it("turns what was typed into the line to write", () => {
    expect(judge("numbers", typed({ pages: "2-3", extra: "5" }), 4)).toEqual({
      ...defaults("numbers"), first_number: 5, pages: [1, 2],
    });
    const mark = judge("watermark", typed({ text: "COPY", extra: "25", position: "top-right" }), 1);
    expect(mark).toMatchObject({ text: "COPY", opacity: 0.25, angle: 45, position: "top-right" });
  });

  it("refuses what the writer would refuse, in a sentence", () => {
    for (const [over, said] of [
      [{ text: " " }, "Type the text"],
      [{ text: "Page\u0007{page}" }, "control character"],
      [{ text: "Seite {seite}" }, "{seite} is not a field"],
      [{ text: "Page {page" }, "not closed"],
      [{ text: "x".repeat(201) }, "longer than 200 characters"],
      [{ size: "2" }, "from 4 to 300"],
      [{ extra: "1.5" }, "whole number"],
      [{ pages: "9" }, "This document has 4 pages"],
    ] as [Partial<Typed>, string][]) {
      const got = judge("numbers", typed(over), 4);
      expect(typeof got === "string" && got.includes(said), `${JSON.stringify(over)}: ${String(got)}`).toBe(true);
    }
    expect(judge("watermark", typed({ extra: "0" }), 1)).toContain("opacity");
    expect(judge("watermark", typed({ text: "x".repeat(200) }), 1)).toMatchObject({ text: "x".repeat(200) });
  });

  it("writes German text, which Latin-1 has", () => {
    expect(judge("numbers", typed({ text: "Seite {page} von {pages} für Prüfung" }), 1)).toMatchObject({
      text: "Seite {page} von {pages} für Prüfung",
    });
  });

  it("leaves which scripts a font has to the backend", () => {
    for (const text of ["Σελίδα {page}", "第{page}页", "עמוד {page}"]) {
      expect(judge("numbers", typed({ text }), 1)).toMatchObject({ text });
    }
  });
});

describe("names and sentences", () => {
  it("offers a name that says what the copy has", () => {
    expect(suggestedName("/a/report.PDF", "numbers")).toBe("report numbered.pdf");
    expect(suggestedName("report.pdf", "watermark")).toBe("report watermarked.pdf");
  });

  it("says what was saved", () => {
    expect(afterAdding({}, "/a/b numbered.pdf", "numbers")).toBe("Saved b numbered.pdf with page numbers.");
    expect(afterAdding({ changed: true }, "c.pdf", "watermark")).toMatch(/^Saved c.pdf with a watermark. The file was written/);
  });
});
