/**
 * The type a tagged block is announced as.
 *
 * The rest of `a11y.ts` builds DOM, and there is no DOM here --- adding one to
 * test it would be a dependency in place of the check that already asserts the
 * real elements in a real webview. What that check cannot do is enumerate a
 * mapping table, which is what this is for.
 *
 * Every case below was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import { elementFor } from "./a11y";

describe("elementFor", () => {
  it("gives a heading the level the document stated", () => {
    // Not "a heading becomes h1". The level is the document's, and flattening
    // every heading to one level destroys the outline a screen reader skims by
    // while still passing any check that only asks whether a heading appeared.
    expect(["H1", "H2", "H3", "H4", "H5", "H6"].map(elementFor)).toEqual([
      "h1",
      "h2",
      "h3",
      "h4",
      "h5",
      "h6",
    ]);
  });

  it("gives a bare H a level, since the document did not", () => {
    // `/H` is legal and says "heading" without saying which. `h2` rather than
    // `h1`: a page's own `H1` is its title, and an unlevelled heading competing
    // with it would put two titles in the outline.
    expect(elementFor("H")).toBe("h2");
  });

  it("does not read a level out of a type that merely starts with H", () => {
    // The hazard of a prefix match, in both directions: `H7` is not a level HTML
    // has, and `Hyperlink` is not a heading at all.
    expect(elementFor("H7")).toBe("p");
    expect(elementFor("Hyperlink")).toBe("p");
    // And the anchor, which the two above do not test: neither matches
    // `^H([1-6])` with or without it, so dropping the `$` survived them. A type
    // that *begins* with a level is what the anchor is for.
    expect(elementFor("H1Alt")).toBe("p");
  });

  it("makes everything else a paragraph", () => {
    // Including the two that have an obvious element and must not get it: `TD`
    // outside a `<table>` is not a cell, and a `<figure>` without the `/Alt` text
    // says nothing a paragraph does not. See the note on the function.
    expect(["P", "Note", "TD", "Figure", "Quote"].map(elementFor)).toEqual([
      "p",
      "p",
      "p",
      "p",
      "p",
    ]);
  });

  it("makes an inferred block a paragraph", () => {
    // `null` is "the geometry drew this boundary", which is never a heading.
    expect(elementFor(null)).toBe("p");
  });
});
