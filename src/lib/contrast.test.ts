/**
 * Dimmed text must still be readable.
 *
 * The interface dims secondary text with `opacity` over the system's own text
 * and background colours, so its contrast is a function of that one number. On
 * white, 0.5 gives 3.98:1 and 0.55 gives 4.76:1; WCAG asks 4.5:1 of text. Dark
 * backgrounds are kinder at every value, so light is the case that decides.
 * Measured on 2026-10-13, seven texts stood at 0.5, the shortcut beside every
 * row of the command palette among them.
 *
 * A disabled control is exempt in WCAG and is exempt here by its selector. An
 * opacity of 0 is a thing hidden and not a thing dimmed.
 */

import { describe, expect, it } from "vitest";

const sources = import.meta.glob(["../**/*.svelte", "./**/*.ts", "!./**/*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** The least opacity text may have: 4.76:1 on white. */
const LEAST = 0.55;

/** Every opacity below {@link LEAST} that is not a disabled control's, as `file: line`. */
function tooFaint(files: Record<string, string>): string[] {
  return Object.entries(files).flatMap(([file, text]) =>
    text.split("\n").flatMap((line) => {
      if (/:disabled\s*\{/.test(line)) return [];
      return [...line.matchAll(/opacity:\s*(0?\.\d+|[01])\b/g)]
        .map((match) => Number(match[1]))
        .filter((value) => value > 0 && value < LEAST)
        .map((value) => `${file}: opacity ${value} in ${line.trim().slice(0, 60)}`);
    }),
  );
}

describe("dimmed text", () => {
  it("is looked for in the sources that style the interface", () => {
    // The control: with no file read, nothing below is ever too faint.
    expect(Object.keys(sources).length).toBeGreaterThan(100);
    expect(Object.keys(sources).some((file) => file.endsWith("App.svelte"))).toBe(true);
    const dimmed = Object.values(sources).join("\n").match(/opacity:\s*0?\.\d+/g) ?? [];
    expect(dimmed.length).toBeGreaterThan(40);
  });

  it("finds a faint text, and passes over a disabled control and a hidden one", () => {
    const sample = {
      "a.ts": 'x.style.cssText = "opacity:0.5;";',
      "b.svelte": "  button:disabled { opacity: .45; }\n  .gone { opacity: 0; }\n  .fine { opacity: .6; }",
    };
    expect(tooFaint(sample)).toEqual(['a.ts: opacity 0.5 in x.style.cssText = "opacity:0.5;";']);
  });

  it("is nowhere fainter than text can be read at", () => {
    expect(tooFaint(sources)).toEqual([]);
  });
});
