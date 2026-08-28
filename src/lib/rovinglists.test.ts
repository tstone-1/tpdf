/**
 * Every panel that keeps a focus mirror has a test that can see it go stale.
 *
 * ## Why this exists
 *
 * Five classes in this frontend are the same widget: a list of rows with a
 * roving `tabIndex`, a `focused` field mirroring which row the DOM has, and a
 * `move(delta)` that steps it. `docs/TRAPS.md` records the same defect being
 * fixed in that widget **three separate times** across eighteen months of
 * commits -- *"A mirror of the DOM's focus goes stale, and Enter activates the
 * row nobody is on"*, *"A synchroniser is not a fix"*, *"The fourth copy carried
 * the explanation and not the fix"*. A window without system focus moves
 * `activeElement` without delivering `focusin`, so a handler that reads the
 * mirror acts on a row the reader is not on.
 *
 * ## This is not the extraction, and that is deliberate
 *
 * The obvious response to five near-copies is to extract them, and that was
 * **measured and declined** -- the reasons are in the last of those three trap
 * entries and they are specific: `thumbnails` is windowed, so its rows map holds
 * only mounted rows and it cannot share the array-backed step; the five key
 * switches differ irreducibly (tree expand/collapse, Delete, drag-Escape); and
 * most decisively the defect lives in `onKeyDown`, which an extraction of
 * `move`/`focus` would not own. The refactor the duplication argues for would
 * not have prevented the thing that motivated it.
 *
 * What the entry chose instead is *"a behavioural test per list, in the place
 * where each one can fail"*. That is a good decision and it was a **convention**:
 * nothing checked it, and the way this defect actually spreads is a sixth copy
 * arriving without one. This test is that decision made mechanical. It asserts
 * the enforcement exists, not the behaviour -- each panel's own test asserts the
 * behaviour, which is the point.
 *
 * ## What counts as the widget
 *
 * A file with both `private move(` and `this.focused`. Measured over
 * `src/lib/*.ts`: that is exactly the five panels. `palette.ts` has a
 * `move(delta)` and is deliberately **not** matched -- its `selected` is an index
 * into a result list rather than a mirror of DOM focus, its rows are not
 * focusable at all, and focus stays in the query field. A discriminator of
 * `move` alone picks it up and would demand a test for a defect it cannot have.
 */

import { describe, expect, it } from "vitest";

const sources = import.meta.glob("./*.ts", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Files that keep a roving focus mirror, by the two markers that define it. */
function panels(): string[] {
  return Object.keys(sources)
    .filter((path) => !path.endsWith(".test.ts"))
    .filter((path) => {
      const source = sources[path] ?? "";
      return source.includes("private move(") && source.includes("this.focused");
    })
    .sort();
}

describe("the roving-focus panels", () => {
  it("are found at all", () => {
    // The emptiness control. A discriminator that stops matching passes every
    // assertion below exactly like a tree with nothing wrong in it, which is the
    // failure this repository records about every check built on a pattern.
    const found = panels();
    expect(found.length).toBeGreaterThanOrEqual(5);
  });

  it("does not match the command palette, which is a different widget", () => {
    // The negative control, and it is what keeps the discriminator honest:
    // `palette.ts` has a `move(delta)` and no focus mirror. If this ever fails,
    // the palette has grown one and needs the test the others have -- not an
    // exemption.
    expect(panels()).not.toContain("./palette.ts");
  });

  it("each have a sibling test that steps focus with an arrow key", () => {
    const missing: string[] = [];
    for (const path of panels()) {
      const test = sources[path.replace(/\.ts$/, ".test.ts")];
      if (test === undefined || !test.includes("ArrowDown")) {
        missing.push(path);
      }
    }
    expect(
      missing,
      "these keep a focus mirror and nothing dispatches an arrow key at them. " +
        "docs/TRAPS.md has this defect three times over; the enforcement chosen " +
        "for it is a behavioural test per list, and this is the list.",
    ).toEqual([]);
  });
});
