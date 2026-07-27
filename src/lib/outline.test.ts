import { describe, expect, it } from "vitest";

import {
  DESTINATION_MARGIN_PT,
  Expansion,
  REACHED_TOLERANCE_PT,
  allRows,
  currentId,
  flatten,
  isNavigable,
  openFlagOf,
  reasonFor,
  type OutlineItem,
  type Target,
} from "./outline";

/** A page destination, optionally partway down. */
function page(index: number, top: number | null = null): Target {
  return { kind: "page", page: index, top_pt: top };
}

function item(
  title: string,
  target: Target,
  children: OutlineItem[] = [],
  open = true,
): OutlineItem {
  return { title, open, target, children };
}

/**
 * A three-level tree, shaped like a real manual.
 *
 * ids:  0        Introduction        page 0
 *       1        Chapter One         page 1
 *       1.0        Setup             page 1 @ 400
 *       1.1        Fonts             page 2
 *       1.1.0        Subsetting      page 2 @ 300
 *       2        Chapter Two         page 5
 *       3        Appendix            page 9
 */
function manual(): OutlineItem[] {
  return [
    item("Introduction", page(0)),
    item("Chapter One", page(1), [
      item("Setup", page(1, 400)),
      item("Fonts", page(2), [item("Subsetting", page(2, 300))]),
    ]),
    item("Chapter Two", page(5)),
    item("Appendix", page(9)),
  ];
}

describe("flatten", () => {
  it("gives every row when everything is open", () => {
    const rows = flatten(manual(), new Expansion());
    expect(rows.map((row) => row.title)).toEqual([
      "Introduction",
      "Chapter One",
      "Setup",
      "Fonts",
      "Subsetting",
      "Chapter Two",
      "Appendix",
    ]);
  });

  it("numbers rows by their path through the tree", () => {
    const rows = flatten(manual(), new Expansion());
    expect(rows.map((row) => row.id)).toEqual([
      "0",
      "1",
      "1.0",
      "1.1",
      "1.1.0",
      "2",
      "3",
    ]);
  });

  it("records depth so a row can be indented", () => {
    const rows = flatten(manual(), new Expansion());
    expect(rows.map((row) => row.depth)).toEqual([0, 0, 1, 1, 2, 0, 0]);
  });

  it("hides the children of a collapsed row", () => {
    const expansion = new Expansion();
    expansion.toggle("1");
    const rows = flatten(manual(), expansion);
    expect(rows.map((row) => row.title)).toEqual([
      "Introduction",
      "Chapter One",
      "Chapter Two",
      "Appendix",
    ]);
  });

  it("hides a grandchild when the grandparent is collapsed", () => {
    // The row "1.1" is expanded in its own right; collapsing "1" must still
    // remove "1.1.0", which a flatten that consulted only the direct parent
    // would leave behind.
    const expansion = new Expansion();
    expansion.toggle("1");
    const rows = flatten(manual(), expansion);
    expect(rows.some((row) => row.title === "Subsetting")).toBe(false);
  });

  it("starts a subtree collapsed when the producer marked it closed", () => {
    const tree = [item("Chapter", page(0), [item("Section", page(1))], false)];
    expect(flatten(tree, new Expansion()).map((r) => r.title)).toEqual([
      "Chapter",
    ]);
  });

  it("opens a producer-closed subtree when it is toggled", () => {
    // The control for the test above: without it, a `flatten` that ignored the
    // toggle entirely would pass by always agreeing with the producer.
    const tree = [item("Chapter", page(0), [item("Section", page(1))], false)];
    const expansion = new Expansion();
    expansion.toggle("0");
    expect(flatten(tree, expansion).map((r) => r.title)).toEqual([
      "Chapter",
      "Section",
    ]);
  });

  it("marks a row without children as having none", () => {
    const rows = flatten(manual(), new Expansion());
    expect(rows[0]?.hasChildren).toBe(false);
    expect(rows[1]?.hasChildren).toBe(true);
  });

  it("never reports a childless row as expanded", () => {
    // A twisty drawn on a leaf is a promise of something to unfold.
    const rows = flatten(manual(), new Expansion());
    for (const row of rows) {
      if (!row.hasChildren) expect(row.expanded).toBe(false);
    }
  });
});

describe("Expansion", () => {
  it("follows the entry's own flag until it is toggled", () => {
    const expansion = new Expansion();
    expect(expansion.isExpanded("0", true)).toBe(true);
    expect(expansion.isExpanded("0", false)).toBe(false);
  });

  it("inverts the entry's flag once toggled", () => {
    const expansion = new Expansion();
    expansion.toggle("0");
    expect(expansion.isExpanded("0", true)).toBe(false);
    expect(expansion.isExpanded("0", false)).toBe(true);
  });

  it("returns to the entry's flag when toggled twice", () => {
    const expansion = new Expansion();
    expansion.toggle("0");
    expansion.toggle("0");
    expect(expansion.isExpanded("0", true)).toBe(true);
  });

  it("reports whether set() changed anything", () => {
    const expansion = new Expansion();
    expect(expansion.set("0", true, true)).toBe(false);
    expect(expansion.set("0", true, false)).toBe(true);
    expect(expansion.isExpanded("0", true)).toBe(false);
  });

  it("expands the ancestors of a row, but not the row itself", () => {
    const tree = manual();
    const expansion = new Expansion();
    expansion.toggle("1");
    expansion.toggle("1.1");
    expansion.reveal("1.1.0", (id) => openFlagOf(tree, id));

    expect(expansion.isExpanded("1", openFlagOf(tree, "1"))).toBe(true);
    expect(expansion.isExpanded("1.1", openFlagOf(tree, "1.1"))).toBe(true);
    expect(flatten(tree, expansion).some((r) => r.id === "1.1.0")).toBe(true);
  });

  it("leaves a revealed row's own collapse alone", () => {
    // Revealing "1" must not unfold "1" --- the reader asked to see the row,
    // not its contents, and expanding it too would move everything below.
    const tree = manual();
    const expansion = new Expansion();
    expansion.toggle("1");
    expansion.reveal("1", (id) => openFlagOf(tree, id));
    expect(expansion.isExpanded("1", openFlagOf(tree, "1"))).toBe(false);
  });
});

describe("openFlagOf", () => {
  it("finds a nested entry's flag", () => {
    const tree = [
      item("Chapter", page(0), [item("Section", page(1), [], false)]),
    ];
    expect(openFlagOf(tree, "0")).toBe(true);
    expect(openFlagOf(tree, "0.0")).toBe(false);
  });

  it("treats an id that names no entry as open", () => {
    expect(openFlagOf(manual(), "9.9.9")).toBe(true);
  });
});

describe("currentId", () => {
  const rows = allRows(manual());

  it("names the entry whose page the reader is on", () => {
    expect(currentId(rows, 5, 0)).toBe("2");
  });

  it("stays on the last entry before an unlisted page", () => {
    expect(currentId(rows, 7, 0)).toBe("2");
  });

  it("distinguishes two entries on the same page by their y", () => {
    // "Chapter One" points at the top of page 1 and "Setup" 400 pt down it.
    expect(currentId(rows, 1, 0)).toBe("1");
    expect(currentId(rows, 1, 500)).toBe("1.0");
  });

  it("does not advance to an entry the reader has not reached", () => {
    // The control for the test above. Comfortably short of "Setup" at 400 ---
    // "one point short" would now land inside the arrival tolerance below.
    expect(currentId(rows, 1, 300)).toBe("1");
  });

  it("counts an entry five points below the top edge as reached", () => {
    // Arriving at a destination deliberately leaves air above the heading, so
    // the heading is *below* the viewport top on arrival. Without this,
    // clicking an entry highlights the one before it --- which the viewer check
    // caught and no unit test did.
    //
    // The 5 is a literal on purpose. Written as `400 - REACHED_TOLERANCE_PT`
    // this could not fail: the input moved with the constant, so setting the
    // tolerance to zero passed all sixty checks. A test whose input is derived
    // from the thing it is testing asserts an identity.
    expect(currentId(rows, 1, 395)).toBe("1.0");
  });

  it("does not count an entry twenty points below the top edge as reached", () => {
    // The other half, and the point of having both: a tolerance that swallowed
    // any distance would satisfy the test above and make the highlight run
    // permanently one entry ahead. Together they pin the tolerance into
    // (5, 20] pt without either of them naming it.
    expect(currentId(rows, 1, 380)).toBe("1");
  });

  it("tolerates more than the jump leaves behind", () => {
    // The invariant the two literals above are standing in for, and the one
    // that actually matters: raising the margin past the tolerance silently
    // reinstates the bug, and neither line would look wrong on its own.
    expect(REACHED_TOLERANCE_PT).toBeGreaterThan(DESTINATION_MARGIN_PT);
  });

  it("names nothing before the first entry", () => {
    const later = [item("Chapter", page(3))];
    expect(currentId(allRows(later), 1, 0)).toBeNull();
  });

  it("uses collapsed rows too", () => {
    // A folded chapter is still the chapter the reader is in.
    //
    // The fold has to be the *producer's* --- `open: false` --- not one applied
    // to an `Expansion` the test holds. Written the second way this could not
    // fail: `allRows` builds its own expansion state, so a mutation replacing
    // its expand-everything with a fresh `Expansion` was invisible against a
    // tree whose entries all say `open: true`. Found by making exactly that
    // mutation and watching all 58 checks pass.
    const tree = [
      item("Chapter One", page(1), [item("Setup", page(1, 400))], false),
    ];
    expect(flatten(tree, new Expansion()).some((r) => r.id === "0.0")).toBe(false);
    expect(currentId(allRows(tree), 1, 500)).toBe("0.0");
  });

  it("ignores entries that point nowhere", () => {
    const tree = [
      item("Heading", { kind: "none" }),
      item("Real", page(0)),
      item("Broken", { kind: "broken" }),
    ];
    expect(currentId(allRows(tree), 2, 0)).toBe("1");
  });

  it("picks the furthest destination, not the last row", () => {
    // An outline that jumps backwards --- an index listed after the chapter it
    // indexes. Taking "the last row at or before here" would answer "Index"
    // for every page from 2 onwards.
    const scrambled = [
      item("Chapter One", page(1)),
      item("Chapter Two", page(8)),
      item("Index", page(2)),
    ];
    expect(currentId(allRows(scrambled), 9, 0)).toBe("1");
  });

  it("treats a destination with no coordinate as the top of its page", () => {
    const tree = [item("Fit", page(4, null)), item("Mid", page(4, 500))];
    expect(currentId(allRows(tree), 4, 100)).toBe("0");
    expect(currentId(allRows(tree), 4, 600)).toBe("1");
  });
});

describe("reasonFor", () => {
  it("says nothing about an entry that works", () => {
    expect(reasonFor(page(3))).toBe("");
  });

  it("names each refusal differently", () => {
    const reasons = ["launch", "uri", "remote", "embedded"].map((action) =>
      reasonFor({ kind: "refused", action }),
    );
    expect(new Set(reasons).size).toBe(reasons.length);
    for (const reason of reasons) expect(reason).not.toBe("");
  });

  it("still explains a refusal it has no wording for", () => {
    expect(reasonFor({ kind: "refused", action: "sound" })).not.toBe("");
  });

  it("distinguishes a broken destination from a missing one", () => {
    expect(reasonFor({ kind: "broken" })).not.toBe(reasonFor({ kind: "none" }));
  });
});

describe("isNavigable", () => {
  it("accepts a page destination and nothing else", () => {
    expect(isNavigable(page(0))).toBe(true);
    expect(isNavigable({ kind: "broken" })).toBe(false);
    expect(isNavigable({ kind: "none" })).toBe(false);
    expect(isNavigable({ kind: "refused", action: "launch" })).toBe(false);
  });
});
