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

describe("AccessibleText and a change of order", () => {
  function page(codes: number[]) {
    return {
      codes,
      boxes: codes.flatMap((_, at) => [10 + at * 10, 100, 20 + at * 10, 90]),
      width_pt: 612,
      height_pt: 792,
      turns: 0,
    };
  }

  /** Everything the tree currently says, joined. */
  function said(root: unknown): string {
    const parts: string[] = [];
    const walk = (element: { textContent: string; children: unknown[] }): void => {
      if (element.textContent) parts.push(element.textContent);
      for (const child of element.children) {
        walk(child as { textContent: string; children: unknown[] });
      }
    };
    walk(root as { textContent: string; children: unknown[] });
    return parts.join("");
  }

  it("rebuilds when the order changes and the page count does not", () => {
    // The case a deletion cannot produce. This dropped every built page only
    // when the count differed, which is right for a document that lost a page
    // and wrong for one whose pages were rearranged: a screen reader would go on
    // reading the order the document used to be in, and nothing in the tree
    // would say so.
    const dom = installFakeDom();
    try {
      const layer = new AccessibleText(dom.root as never, 2);
      layer.sync([0], () => page([72, 105]) as never);
      expect(said(dom.root)).toContain("Hi");

      // Two pages before and two after, which is what a swap reports.
      layer.setPages(2);
      expect(said(dom.root)).toBe("");

      // And it builds again from whatever the pages now hold, rather than being
      // left empty --- which is the half a bare "it cleared" assertion misses.
      layer.sync([0], () => page([78, 111]) as never);
      expect(said(dom.root)).toContain("No");
    } finally {
      dom.restore();
    }
  });
});

describe("AccessibleText and links", () => {
  /**
   * A page of one line: "GO" inside a link, then " ON" outside it.
   *
   * Boxes are `[left, top, right, bottom]` per character in the page's displayed
   * space, which is the same space a link's rectangle is in --- so the two can be
   * compared with no rotation, and this fixture is the smallest thing that says
   * a link claims some characters and not others.
   */
  function page() {
    return {
      codes: [71, 79, 32, 79, 78],
      boxes: [
        10, 100, 20, 112, // G, centre (15, 106) -- inside
        20, 100, 30, 112, // O, centre (25, 106) -- inside
        30, 100, 36, 112, // space, centre (33, 106) -- outside
        36, 100, 46, 112, // O
        46, 100, 56, 112, // N
      ],
      width_pt: 612,
      height_pt: 792,
      turns: 0,
    };
  }

  /** A link over the first two characters only. */
  function link(over: Record<string, unknown> = {}) {
    return {
      id: 0,
      page: 0,
      rect: [8, 98, 32, 114],
      target: { kind: "page", page: 4, top_pt: 200 },
      ...over,
    };
  }

  /** Builds the tree and returns the root, with the fake DOM still installed. */
  function build(links: unknown[]): {
    dom: ReturnType<typeof installFakeDom>;
    spans: { textContent: string; attributes: Map<string, string>; dataset: Record<string, string> }[];
    text: string;
  } {
    const dom = installFakeDom();
    const layer = new AccessibleText(dom.root as never, 1);
    layer.sync([0], () => page() as never);
    layer.setLinks(links as never);

    const spans: {
      textContent: string;
      attributes: Map<string, string>;
      dataset: Record<string, string>;
    }[] = [];
    let text = "";
    const walk = (element: {
      tagName: string;
      textContent: string;
      children: unknown[];
      attributes: Map<string, string>;
      dataset: Record<string, string>;
    }): void => {
      if (element.attributes.get("role") === "link") spans.push(element);
      if (element.children.length === 0) text += element.textContent;
      for (const child of element.children) walk(child as never);
    };
    walk(dom.root as never);
    return { dom, spans, text };
  }

  it("announces a link as a link, and only the characters it covers", () => {
    const { dom, spans, text } = build([link()]);
    try {
      expect(spans).toHaveLength(1);
      expect(spans[0]?.textContent).toBe("GO");
      // The whole line is still there. A marked-up run that swallowed the rest
      // of the line would pass an assertion about the span alone.
      expect(text).toBe("GO ON");
    } finally {
      dom.restore();
    }
  });

  it("marks nothing on a page with no links", () => {
    // The control. Without it, a rule that marked every character would pass the
    // test above --- the span's text would still be "GO" if the split happened
    // to land there.
    const { dom, spans, text } = build([]);
    try {
      expect(spans).toHaveLength(0);
      expect(text).toBe("GO ON");
    } finally {
      dom.restore();
    }
  });

  it("carries the destination page as a number of ours", () => {
    const { dom, spans } = build([link()]);
    try {
      expect(spans[0]?.dataset.page).toBe("4");
      expect(spans[0]?.attributes.get("aria-disabled")).toBeUndefined();
    } finally {
      dom.restore();
    }
  });

  it("says a refused link is unavailable rather than leaving it inert", () => {
    const { dom, spans } = build([link({ target: { kind: "refused", action: "uri" } })]);
    try {
      expect(spans).toHaveLength(1);
      expect(spans[0]?.attributes.get("aria-disabled")).toBe("true");
      // And no destination, which is what says the two cases are distinguishable
      // from the DOM rather than only from the target we were handed.
      expect(spans[0]?.dataset.page).toBeUndefined();
    } finally {
      dom.restore();
    }
  });

  it("never creates an element that could carry a URL", () => {
    // `docs/THREAT-MODEL.md` T8 and the `sinks` gate both turn on this: a span
    // with a role is announced as a link and can hold no URL, and an `<a>` here
    // would be the one element that could. Asserted on the DOM rather than by
    // reading the source, so it is a statement about what was built.
    const { dom, spans } = build([link()]);
    try {
      expect(spans[0]).toBeDefined();
      const tags: string[] = [];
      const walk = (element: { tagName: string; children: unknown[] }): void => {
        tags.push(element.tagName.toLowerCase());
        for (const child of element.children) walk(child as never);
      };
      walk(dom.root as never);
      expect(tags).not.toContain("a");
      expect(tags).not.toContain("iframe");
      expect(tags).toContain("span");
    } finally {
      dom.restore();
    }
  });

  it("rebuilds a page that was already built when the links arrive", () => {
    // The links land on their own chain, after first paint, so the page a reader
    // is on has already been built without them. A layer that only marked pages
    // built afterwards would announce the first page of every document as prose.
    const dom = installFakeDom();
    try {
      const layer = new AccessibleText(dom.root as never, 1);
      layer.sync([0], () => page() as never);
      const before: string[] = [];
      const collect = (into: string[]) => {
        const walk = (element: {
          children: unknown[];
          attributes: Map<string, string>;
        }): void => {
          if (element.attributes.get("role") === "link") into.push("link");
          for (const child of element.children) walk(child as never);
        };
        walk(dom.root as never);
      };
      collect(before);
      expect(before).toHaveLength(0);

      layer.setLinks([link()] as never);
      const after: string[] = [];
      collect(after);
      expect(after).toHaveLength(1);
    } finally {
      dom.restore();
    }
  });
});

/**
 * Joining the lines of one tagged paragraph.
 *
 * A tagged block is handed to a screen reader whole, so its lines have to be
 * joined with a space --- a line break inside a paragraph is a decision the
 * producer made about the page, not part of what it says. Without the join the
 * last word of one line runs into the first of the next, which reads aloud as a
 * word that is not in the document.
 *
 * **This is here rather than in the window harness because no fixture reaches
 * it.** The separator exists only for a *tagged* block, `tagged.pdf` is the one
 * corpus carrying a `/StructTreeRoot`, and none of its tagged blocks spans two
 * lines --- so the mutation aimed at this branch survived the whole viewer run
 * while changing nothing observable. The trap *A mutation aimed at code no
 * fixture reaches survives, and the fix is not a new corpus* is the same
 * finding from the other direction, and prescribes exactly this: test the
 * branch directly, and move the mutation to the harness that can judge it.
 */
describe("AccessibleText and a paragraph the producer wrapped", () => {
  /**
   * Two lines of two characters each, one above the other.
   *
   * `AB` on the upper line and `CD` on the lower --- far enough apart to be two
   * lines, and **close enough to stay one block**, which the control depends on.
   * Spread them further and the geometry splits them into two blocks, at which
   * point the tagged path and the untagged one both emit two paragraphs and the
   * control cannot tell them apart. Measured, after a mutation survived it.
   */
  function wrapped(runs: unknown[] | undefined) {
    return {
      codes: [65, 66, 67, 68],
      boxes: [
        10, 100, 20, 112, // A
        20, 100, 30, 112, // B
        10, 130, 20, 142, // C
        20, 130, 30, 142, // D
      ],
      width_pt: 612,
      height_pt: 792,
      turns: 0,
      quarter_turns: 0,
      extract_ms: 0,
      runs,
    };
  }

  /** The text of every leaf, and how many elements the page became. */
  function build(runs: unknown[] | undefined): {
    dom: ReturnType<typeof installFakeDom>;
    text: string;
    blocks: number;
  } {
    const dom = installFakeDom();
    const layer = new AccessibleText(dom.root as never, 1);
    layer.sync([0], () => wrapped(runs) as never);

    let text = "";
    let blocks = 0;
    const walk = (element: {
      tagName: string;
      textContent: string;
      children: unknown[];
    }): void => {
      if (element.tagName === "p") blocks += 1;
      if (element.children.length === 0) text += element.textContent;
      for (const child of element.children) walk(child as never);
    };
    walk(dom.root as never);
    return { dom, text, blocks };
  }

  it("joins the lines of one tagged paragraph with a space", () => {
    const { dom, text, blocks } = build([
      { tag: "P", path: ["Document", "P"], start: 0, end: 4 },
    ]);
    try {
      // One element, because the producer said these four characters are one
      // paragraph -- and the space is the whole point: without it a reader hears
      // "ABCD".
      expect(blocks).toBe(1);
      expect(text).toBe("AB CD");
    } finally {
      dom.restore();
    }
  });

  it("leaves an untagged page as a paragraph per line, unjoined", () => {
    // The control on the assertion above: without it, "joins the lines" is
    // satisfied by an implementation that joins everything, and a join applied
    // to an inferred boundary would silently merge two columns into a sentence.
    //
    // **It is a weaker control than it looks and is named for what it actually
    // establishes.** On this fixture the geometry puts the two lines in two
    // *blocks*, not one block of two lines --- measured, after a mutation that
    // sent untagged blocks down the tagged path survived it, because both paths
    // then emit one paragraph per block. Tightening the spacing further did not
    // help; the block cut is a multiple of the type size and these lines are
    // past it either way. The tag distinction itself is proved a layer up, in
    // `reading.test.ts`, by the mutation `a11y: treat an inferred block as a
    // stated paragraph`, which is caught. What this pins is the consumer side:
    // an untagged page comes out unjoined.
    const { dom, text, blocks } = build(undefined);
    try {
      expect(blocks).toBe(2);
      expect(text).toBe("ABCD");
    } finally {
      dom.restore();
    }
  });
});
