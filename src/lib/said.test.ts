/**
 * Every `said!` in the Rust source has German here, and nothing else does.
 *
 * The sources are read as text, the way `addtext.test.ts` reads one, so a
 * sentence added in Rust is a red test here until it is translated. Then each
 * template is rendered with sample arguments and handed to `recognise`, which
 * must name that template and no other: two templates a rendered sentence
 * could fit both would translate one reader's message as the other's, and
 * nothing at a window would show which.
 */

import { describe, expect, it } from "vitest";

import { parse, recognise, spoken } from "./said";
import { AREAS, GERMAN } from "./said/de";

const sources = import.meta.glob("../../src-tauri/src/**/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Reads the Rust string literal starting at `at` (its opening quote). */
function literal(text: string, at: number): { value: string; end: number } | null {
  if (text[at] !== '"') return null;
  let value = "";
  let i = at + 1;
  while (i < text.length) {
    const c = text[i]!;
    if (c === '"') return { value, end: i + 1 };
    if (c !== "\\") {
      value += c;
      i += 1;
      continue;
    }
    const next = text[i + 1]!;
    if (next === "\n" || next === "\r") {
      // A continuation: the newline and the next line's leading whitespace go.
      i += 1;
      while (i < text.length && /\s/.test(text[i]!)) i += 1;
      continue;
    }
    const simple: Record<string, string> = { n: "\n", t: "\t", r: "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'" };
    if (next in simple) {
      value += simple[next];
      i += 2;
      continue;
    }
    if (next === "u") {
      const close = text.indexOf("}", i);
      value += String.fromCodePoint(parseInt(text.slice(i + 3, close), 16));
      i = close + 1;
      continue;
    }
    return null;
  }
  return null;
}

interface Found {
  file: string;
  template: string | null;
}

/** Every `said!(` outside a comment, with its template or `null` when it has none. */
function inRust(): Found[] {
  const found: Found[] = [];
  for (const [file, text] of Object.entries(sources)) {
    // The macro's own file, whose tests use it on sentences nobody is shown.
    if (file.endsWith("/said.rs")) continue;
    for (const match of text.matchAll(/said!\(/g)) {
      const lineStart = text.lastIndexOf("\n", match.index) + 1;
      if (text.slice(lineStart, match.index).trimStart().startsWith("//")) continue;
      let at = match.index + match[0].length;
      while (/\s/.test(text[at]!)) at += 1;
      found.push({ file, template: literal(text, at)?.value ?? null });
    }
  }
  return found;
}

const rust = inRust();
const templates = new Set(rust.flatMap((f) => (f.template === null ? [] : [f.template])));

describe("said!: Rust's sentences in German", () => {
  it("finds sentences to check", () => {
    // The emptiness control: a reader that lost its way would compare nothing.
    expect(Object.keys(sources).length).toBeGreaterThan(100);
    expect(templates.size).toBeGreaterThan(0);
  });

  it("every said! takes one plain string literal", () => {
    expect(rust.filter((f) => f.template === null).map((f) => f.file)).toEqual([]);
  });

  it("every template names its placeholders", () => {
    const bad = [...templates].filter((t) => {
      try {
        parse(t);
        return false;
      } catch {
        return true;
      }
    });
    expect(bad).toEqual([]);
  });

  it("every template has German, and every German entry a template", () => {
    const keys = new Set(Object.keys(GERMAN));
    expect([...templates].filter((t) => !keys.has(t)).sort()).toEqual([]);
    expect([...keys].filter((k) => !templates.has(k)).sort()).toEqual([]);
  });

  it("no template has German in two files", () => {
    const seen = new Map<string, string>();
    const twice: string[] = [];
    for (const [file, entries] of Object.entries(AREAS)) {
      for (const key of Object.keys(entries)) {
        if (seen.has(key)) twice.push(`${key}: ${seen.get(key)} and ${file}`);
        seen.set(key, file);
      }
    }
    expect(twice).toEqual([]);
  });

  it("a German wording uses the template's placeholders, all of them", () => {
    const wrong: string[] = [];
    for (const [key, wording] of Object.entries(GERMAN)) {
      const names = parse(key).names;
      if (typeof wording === "function") {
        const args = Object.fromEntries(names.map((n, i) => [n, `${i + 2}`]));
        const out = wording(args);
        if (typeof out !== "string" || out.length === 0) wrong.push(key);
        continue;
      }
      const used = [...wording.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)].map((m) => m[1]!);
      if ([...new Set(used)].sort().join() !== [...new Set(names)].sort().join()) wrong.push(key);
    }
    expect(wrong).toEqual([]);
  });

  it("each template is recognised as itself and as no other", () => {
    const confused: string[] = [];
    for (const key of Object.keys(GERMAN)) {
      const { names } = parse(key);
      let i = 0;
      // One pass, doubled braces first, as `format!` and `parse` read them:
      // `{{name}}` is the text `{name}`, not a placeholder.
      const sentence = key.replace(/\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*)(?::[^}]*)?\}/g, (token: string) =>
        token === "{{" ? "{" : token === "}}" ? "}" : `⟨${(i += 1)}⟩`);
      const found = recognise(sentence);
      if (found?.template.key !== key) {
        confused.push(`${key} -> ${found?.template.key ?? "nothing"}`);
        continue;
      }
      names.forEach((name, n) => {
        if (found.args[name] !== `⟨${n + 1}⟩`) confused.push(`${key}: ${name} = ${found.args[name]}`);
      });
    }
    expect(confused).toEqual([]);
  });

  it("no template carries a source line's indentation", () => {
    // A `\` at the end of a line inside a Rust string drops the newline and the
    // next line's indentation. Lose the backslash and the indentation is in the
    // sentence: three messages showed readers runs of 18 and 21 spaces until
    // 2026-10-28, and nothing compared them.
    expect([...templates].filter((t) => / {3,}/.test(t))).toEqual([]);
  });

  it("no German wording uses Rust's prose dash", () => {
    // The keys keep Rust's `---` to match it; the German is shown, and takes " – ".
    const dashed = Object.entries(GERMAN).filter(([key, wording]) => {
      const args = Object.fromEntries(parse(key).names.map((n) => [n, "x"]));
      return (typeof wording === "function" ? wording(args) : wording).includes("---");
    });
    expect(dashed.map(([key]) => key)).toEqual([]);
  });

  it("every template has words of its own", () => {
    // A placeholder matches any text, so `{what}: {why}` would claim every
    // sentence with a colon in it that no longer template claimed first.
    const generic = [...templates].filter((t) => {
      const words = t.replace(/\{[^}]*\}/g, " ").match(/[A-Za-z]{2,}/g) ?? [];
      return words.join("").length < 6;
    });
    expect(generic).toEqual([]);
  });
});

describe("spoken", () => {
  it("renders a sentence from Rust in German, with the sentence inside it", () => {
    expect(spoken("This file could not be read from disk.", "de-DE")).toBe(
      "Diese Datei konnte nicht vom Datenträger gelesen werden.",
    );
    expect(spoken("could not load Pdfium from C:\\a b\\pdfium.dll: This file could not be read from disk.", "de-DE"))
      .toBe("PDFium konnte nicht aus C:\\a b\\pdfium.dll geladen werden: Diese Datei konnte nicht vom Datenträger gelesen werden.");
    expect(spoken("This file could not be read from disk.", "en-GB")).toBe("This file could not be read from disk.");
  });

  it("leaves English and unknown sentences alone", () => {
    expect(spoken("anything at all", "en-US")).toBe("anything at all");
    expect(spoken("a sentence no template has", "de-DE")).toBe("a sentence no template has");
  });

  it("refuses a positional placeholder", () => {
    expect(() => parse("page {} of {}")).toThrow(/positional/);
  });

  it("takes linear time on a sentence built from a template's own separators", () => {
    // A field name comes from the document. The lazy regex this replaced took
    // 6.6 s here at 4 KB, each placeholder trying every later separator; the
    // budget is far above what a linear match takes and far below that.
    const key = "`{name}`: its rectangle is not inside page {page}, which is {width} by {height} points";
    expect(Object.keys(GERMAN)).toContain(key);
    // Every separator many times over, and a last character that breaks the
    // final literal, so the regex had to try every split before giving up.
    const filler = "`: its rectangle is not inside page , which is  by ".repeat(300);
    const sentence = `\`${filler}\`: its rectangle is not inside page ${filler}, which is ${filler} by ${filler} points!`;
    const started = performance.now();
    expect(spoken(sentence, "de-DE")).toBe(sentence);
    expect(performance.now() - started).toBeLessThan(500);
  });

  it("splits a placeholder at the first separator after it, as the lazy regex did", () => {
    const found = recognise("could not read a: b: c");
    expect(found?.template.key).toBe("could not read {path}: {e}");
    expect(found?.args).toEqual({ path: "a", e: "b: c" });
  });

  it("recognises braces written double", () => {
    expect(parse("a {{literal}} and {name}").segments).toEqual(["a {literal} and ", ""]);
  });
});
