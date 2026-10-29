/**
 * The landing page against the README.
 *
 * WHY THIS EXISTS. `site/index.html` restates facts the README owns: the
 * start-up time, the platforms, the publisher's name on the Windows installer,
 * the Homebrew command. They are written twice because the page is for a
 * reader who never opens the repository, and nothing held the two equal. The
 * README is the original; a figure changed there and not here would be a page
 * that advertises a measurement nobody made.
 *
 * Two rules. Each sentence in {@link SHARED} is in both. And every figure with
 * a unit that the page prints is somewhere in the README, so a number added to
 * the page later is held too without being listed here.
 *
 * `site/de/index.html` is German, so it cannot share the English sentences.
 * It is held to the README by what reads the same in both languages: the
 * commands, the publisher's name, its figures, and the invoice number it
 * tells a visitor to search for.
 */
import { describe, expect, test } from "vitest";
import page from "../../site/index.html?raw";
import german from "../../site/de/index.html?raw";
import readme from "../../README.md?raw";

/** Text as a reader sees it: no tags, no Markdown marks, single spaces. */
function words(source: string): string {
  return source
    .replace(/<style[\s\S]*?<\/style>/g, " ")
    .replace(/<[^>]+>/g, " ")
    .replace(/[`*_]/g, "")
    .replace(/\s+/g, " ");
}

const SHARED = [
  "is 276 ms, measured warm on an Apple silicon Mac",
  "brew install --cask tstone-1/tpdf/tpdf",
  "Open Source Developer Timo Stein",
  "There is no build for Intel Macs, Windows on ARM or ARM Linux",
  "sudo apt install ./tpdf",
  "Recognition is local and temporary: the PDF stays unchanged",
  "INV-2026-0417",
];

/** What the German page has to say word for word, as the README does. */
const SHARED_GERMAN = [
  "brew install --cask tstone-1/tpdf/tpdf",
  "Open Source Developer Timo Stein",
  "sudo apt install ./tpdf",
];

const FIGURE = /\d[\d.,]*\s?(?:ms|MB|GB)\b/g;

describe("the landing page", () => {
  const shown = words(page);
  const original = words(readme);

  test.each(SHARED)("says what the README says: %s", (sentence) => {
    // As booleans with a label: a failed `toContain` prints the whole README.
    expect(shown.includes(sentence), "on the page").toBe(true);
    expect(original.includes(sentence), "in the README").toBe(true);
  });

  test("prints no figure the README does not have", () => {
    const figures = shown.match(FIGURE) ?? [];
    // Without one the rule below would pass on a page that states nothing.
    expect(figures.length).toBeGreaterThan(0);
    for (const figure of figures) expect(original.includes(figure), figure).toBe(true);
  });
});

describe("the German landing page", () => {
  const shown = words(german);
  const original = words(readme);

  test.each(SHARED_GERMAN)("says what the README says: %s", (sentence) => {
    expect(shown.includes(sentence), "on the page").toBe(true);
    expect(original.includes(sentence), "in the README").toBe(true);
  });

  test("prints no figure the README does not have", () => {
    const figures = shown.match(FIGURE) ?? [];
    expect(figures.length).toBeGreaterThan(0);
    for (const figure of figures) expect(original.includes(figure), figure).toBe(true);
  });

  test("names the German invoice's number, which differs from the English one", () => {
    expect(shown.includes("RE-2026-0417")).toBe(true);
    expect(shown.includes("INV-2026-0417")).toBe(false);
  });
});
