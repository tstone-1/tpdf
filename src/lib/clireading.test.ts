/**
 * `tpdf text` reads a page in the order the viewer reads it.
 *
 * The command-line tool has no webview, so `src-tauri/src/reading.rs` restates
 * this directory's `reading.ts` in Rust --- and a restatement is a second copy,
 * which drifts. So Rust writes every case in `reading::tests::cases` with the
 * order it computes to `src-tauri/testdata/cli/reading.json`
 * (`TPDF_CLI_SAMPLES=write`), and this file asks the original the same
 * questions and compares, route, line and index. A rule changed on either side
 * is a red test here; the same arrangement as `cliwording.test.ts`.
 *
 * The emptiness controls are the counts: a sample that lost its cases, or whose
 * cases stopped exercising one of the two routes, would otherwise compare less
 * than it claims and pass.
 */

import { describe, expect, it } from "vitest";

import reading from "../../src-tauri/testdata/cli/reading.json";
import { readingLines, readingOrder, usableRuns } from "./reading";
import type { PageText } from "./text";

interface Case {
  name: string;
  text: PageText;
  route: "tagged" | "geometric";
  lines: { from: number; to: number }[][];
  order: number[];
}

const cases = reading.cases as unknown as Case[];

describe("the command-line tool's reading order", () => {
  it("covers both routes, every rotation and the degenerate pages", () => {
    expect(cases.length).toBe(24);
    // A line in two halves: by the tags, by the geometry, right to left, with
    // one word written the other way, with a combining mark in it, and beside
    // type of another size.
    expect(cases.filter((c) => c.name.startsWith("split-line")).length).toBe(6);
    const routes = new Set(cases.map((c) => c.route));
    expect([...routes].sort()).toEqual(["geometric", "tagged"]);
    expect(new Set(cases.map((c) => c.text.quarter_turns)).size).toBe(4);
    expect(cases.some((c) => (c.text.char_turns?.length ?? 0) > 0)).toBe(true);
    expect(cases.some((c) => c.text.codes.length === 0)).toBe(true);
  });

  it("takes the same route as the viewer", () => {
    for (const c of cases) {
      expect(usableRuns(c.text) === null ? "geometric" : "tagged", c.name).toBe(c.route);
    }
  });

  it("finds the same lines, in the same order", () => {
    for (const c of cases) {
      expect(readingLines(c.text).map((line) => line.ranges), c.name).toEqual(c.lines);
    }
  });

  it("reads the characters in the same order", () => {
    for (const c of cases) {
      expect(readingOrder(c.text), c.name).toEqual(c.order);
    }
  });
});
