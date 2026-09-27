/**
 * `tpdf redact` marks a match where the window's *Mark all matches for
 * redaction* marks it.
 *
 * The window takes a hit through `matchHalves` (`search.ts`), `runsFor`
 * (`text.ts`) and `areasFrom` (`selection.ts`), and each rectangle that comes
 * out is one `Edits.redact`. The command-line tool has no webview, so
 * `src-tauri/src/cli/regions.rs` restates the three --- and a restatement is a
 * second copy, which drifts. So Rust writes every case it asks about to
 * `src-tauri/testdata/cli/regions.json` (`TPDF_CLI_SAMPLES=write`), and this
 * file asks the originals the same questions and compares, number for number.
 * A rule changed on either side is a red test here; the arrangement
 * `clireading.test.ts` has with `reading.rs`.
 *
 * A region crosses into Rust as JSON and is parsed as an `f32`, so the areas
 * are compared after `Math.fround` on both sides; the runs before it are
 * compared exactly, because nothing has narrowed them yet.
 *
 * The emptiness controls are the counts: a sample that lost its cases would
 * otherwise compare nothing and pass.
 */

import { describe, expect, it } from "vitest";

import regions from "../../src-tauri/testdata/cli/regions.json";
import { MAX_MATCHES_TO_MARK, matchHalves, type Match } from "./search";
import { MIN_REDACTION_SIDE, areasFrom } from "./selection";
import { runsFor, type PageText } from "./text";

interface RunCase {
  name: string;
  text: PageText;
  from: number;
  to: number | null;
  quads: number[];
  areas: number[][];
}

const runs = regions.runs as unknown as RunCase[];
const rawAreas = regions.areas as unknown as { quads: number[]; areas: number[][] }[];

function flat(text: PageText, from: number, to: number): number[] {
  return runsFor(text, from, to).flatMap((q) => [q.left, q.top, q.right, q.bottom]);
}

function narrowed(areas: readonly (readonly number[])[]): number[][] {
  return areas.map((area) => area.map((value) => Math.fround(value)));
}

describe("the command-line tool's regions", () => {
  it("asks about every reading case four ways, and some of them mark something", () => {
    expect(runs.length).toBe(18 * 4);
    expect(runs.filter((c) => c.areas.length > 0).length).toBeGreaterThan(30);
    expect(runs.some((c) => c.to === null)).toBe(true);
    expect(rawAreas.length).toBe(6);
  });

  it("shares the viewer's limits", () => {
    expect(regions.min_redaction_side).toBe(MIN_REDACTION_SIDE);
    expect(regions.max_matches_to_mark).toBe(MAX_MATCHES_TO_MARK);
  });

  it("merges a range into the runs runsFor gives", () => {
    for (const c of runs) {
      expect(flat(c.text, c.from, c.to ?? Infinity), `${c.name} ${c.from}..${c.to}`).toEqual(
        c.quads,
      );
    }
  });

  it("turns runs into the regions areasFrom gives", () => {
    for (const c of runs) {
      expect(narrowed(areasFrom(c.quads)), `${c.name} ${c.from}..${c.to}`).toEqual(
        narrowed(c.areas),
      );
    }
    for (const c of rawAreas) {
      expect(narrowed(areasFrom(c.quads)), JSON.stringify(c.quads)).toEqual(narrowed(c.areas));
    }
  });

  it("splits a hit over a page break as matchHalves does", () => {
    const halves = matchHalves(regions.matches as unknown as Match[]);
    expect(halves.length).toBe(regions.halves.length);
    expect(halves.length).toBeGreaterThan((regions.matches as unknown[]).length);
    expect(
      halves.map((h) => ({ page: h.slot, from: h.from, to: h.to === Infinity ? null : h.to })),
    ).toEqual(regions.halves);
  });
});
