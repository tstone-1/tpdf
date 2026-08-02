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

import { AccessibleText, elementFor } from "./a11y";
import { installFakeDom } from "./testdom";

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

describe("AccessibleText and a page whose text means nothing", () => {
  /** One page of two characters, laid out on one line. */
  function page() {
    return {
      codes: [72, 105],
      boxes: [10, 100, 20, 90, 20, 100, 30, 90],
      width_pt: 612,
      height_pt: 792,
      turns: 0,
    };
  }

  /** Every string the layer put in the tree, joined. */
  function render(unreadable: boolean): string {
    const dom = installFakeDom();
    try {
      const layer = new AccessibleText(dom.root as never, 1);
      layer.sync(
        [0],
        () => page() as never,
        () => unreadable,
      );
      const said: string[] = [];
      const walk = (element: { textContent: string; children: unknown[] }): void => {
        if (element.textContent) said.push(element.textContent);
        for (const child of element.children) {
          walk(child as { textContent: string; children: unknown[] });
        }
      };
      walk(dom.root as unknown as { textContent: string; children: unknown[] });
      // Joined with nothing: the layer emits one element per *line*, so a
      // two-character page arrives as "H" and "i" and any separator would break
      // a word the assertion is looking for.
      return said.join("");
    } finally {
      dom.restore();
    }
  }

  it("withholds the characters when the document does not say what they mean", () => {
    // PDFium returns text of the right length that means nothing, and a screen
    // reader has nothing to tell it apart from the page. Reading it aloud is the
    // symptom whose reader can least easily notice it, so the characters are
    // withheld and the reason given instead.
    const said = render(true);
    expect(said).toContain("cannot be read");
    expect(said).not.toContain("Hi");
  });

  it("reads the page normally when the document does say", () => {
    // The control, and the one that matters: a rule that withheld text from an
    // ordinary page would silence the accessibility layer for every document
    // while passing the test above.
    const said = render(false);
    expect(said).toContain("Hi");
    expect(said).not.toContain("cannot be read");
  });
});
