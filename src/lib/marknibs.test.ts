import { describe, expect, it } from "vitest";

import { INK_WIDTH } from "./markband";
import { DEFAULT_NIB, NIBS, nib, widthFor } from "./marknibs";

/**
 * The bounds `docmodel.rs` clamps an arriving width into.
 *
 * **Transcribed, which this file otherwise refuses to do**, and the reason is
 * that they are not ours: they are the far side of the wire, in another
 * language, and the assertion below is precisely that this table sits inside
 * them. A test that read them from the Rust would be a test of nothing, because
 * there is no route from here to that file --- and a table that drifted outside
 * would not fail loudly, it would silently draw every "marker" at the ceiling.
 */
const NIB_MIN = 0.25;
const NIB_MAX = 24;

describe("the nibs a reader can pick", () => {
  it("offers every one inside the range the backend clamps to", () => {
    // The door in `edits.rs` clamps rather than refuses, so a nib outside these
    // would arrive as something else and nothing here or there would say so:
    // the reader would pick "marker", get the ceiling, and see a line that is
    // not the one they asked for.
    for (const entry of NIBS) {
      expect(entry.pt, entry.id).toBeGreaterThanOrEqual(NIB_MIN);
      expect(entry.pt, entry.id).toBeLessThanOrEqual(NIB_MAX);
    }
  });

  it("goes from thinnest to thickest, with no two the same", () => {
    // The order is what the list is read in and what the commands are
    // registered in, so it is a property rather than an accident of typing.
    // Distinctness is the half with teeth: two entries with one width is two
    // commands a reader cannot tell apart by their effect.
    const widths = NIBS.map((entry) => entry.pt);
    expect(widths).toStrictEqual([...widths].sort((a, b) => a - b));
    expect(new Set(widths).size).toBe(widths.length);
  });

  it("names them all differently, and none is another's prefix", () => {
    // The command titles are built from these, and a title that is a strict
    // prefix of another ties in the palette's ranking --- which this repository
    // has already paid for once, with the full name of one command running the
    // other.
    for (const a of NIBS) {
      for (const b of NIBS) {
        if (a === b) continue;
        expect(b.name.startsWith(a.name), `${a.name} / ${b.name}`).toBe(false);
        expect(a.id === b.id, `${a.id} twice`).toBe(false);
      }
    }
  });

  it("holds the default by reference, so it cannot be a shade off", () => {
    // A "medium" a fraction away from `INK_WIDTH` means a reader picking the
    // entry matching their existing drawings gets ink that does not match them.
    // The assertion is equality with the constant rather than with a number
    // written here, which is what makes it survive the constant moving.
    expect(DEFAULT_NIB.pt).toBe(INK_WIDTH);
    expect(NIBS.filter((entry) => entry.pt === INK_WIDTH)).toHaveLength(1);
  });

  it("finds a nib by id, and nothing by a name that is not one", () => {
    expect(nib(DEFAULT_NIB.id)).toBe(DEFAULT_NIB);
    expect(nib("hairline")).toBeUndefined();
  });

  it("falls back to the default width when nothing is chosen", () => {
    // The seam `App.svelte` calls, and the only reason it is a function: that
    // file is reached by no unit test, so a caller writing `chosen.pt` and
    // crashing on null would be checked by nothing.
    expect(widthFor(null)).toBe(INK_WIDTH);
    const broad = nib("broad");
    expect(broad).toBeDefined();
    expect(widthFor(broad!)).toBe(broad!.pt);
    expect(widthFor(broad!)).not.toBe(INK_WIDTH);
  });
});
