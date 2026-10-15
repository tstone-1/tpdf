import { describe, expect, it } from "vitest";

/**
 * How many test files read `App.svelte` as text, and the ceiling on it.
 *
 * A check of the component's source sees that a line is there, not whether it
 * runs (`sourcetext.ts`), so it is the weakest test this repository has. The
 * suites doing it went from 6 to 13 between 2026-10-09 and 2026-10-15 while
 * the decisions they guarded could have been functions in `src/lib` with unit
 * tests, which is what `documentopen.ts` and `documentmount.ts` did. This
 * holds the number where it is: a new decision goes into `src/lib`, and the
 * component hands it what it acts on, so its type checks the join.
 *
 * Lower it when a suite stops reading the component; never raise it.
 */
const CEILING = 13;

/** This file names the import to count it, and is not one of them. */
const SELF = "./apptext.test.ts";

const suites = import.meta.glob("./*.test.ts", { query: "?raw", import: "default", eager: true });

function readersOfTheComponent(files: Record<string, unknown>): string[] {
  return Object.entries(files)
    .filter(([name, text]) => name !== SELF && typeof text === "string" && /App\.svelte\?raw/.test(text))
    .map(([name]) => name)
    .sort();
}

describe("App.svelte as text", () => {
  it("is read by no more suites than the ceiling", () => {
    const found = readersOfTheComponent(suites);
    expect(
      found.length,
      `${found.length} suites read App.svelte as text, above ${CEILING}. Put the decision in a ` +
        "src/lib function with a unit test, as documentopen.ts does, instead of checking the " +
        `component's source:\n${found.join("\n")}`,
    ).toBeLessThanOrEqual(CEILING);
  });

  it("finds the suites it counts", () => {
    // The emptiness control: a glob that matched nothing, or a pattern that no
    // longer matches the import, would hold any ceiling.
    const found = readersOfTheComponent(suites);
    expect(found).toContain("./documentopen.test.ts");
    expect(found.length).toBe(CEILING);
  });

  it("counts a suite that reads the component, and not one that does not", () => {
    expect(readersOfTheComponent({
      "./a.test.ts": 'import app from "../App.svelte?raw";',
      "./b.test.ts": 'import { x } from "./x";',
      [SELF]: 'import app from "../App.svelte?raw";',
    })).toEqual(["./a.test.ts"]);
  });
});
