/**
 * Recovering a page's reading order from its geometry.
 *
 * The thing under test is an ordering, so every assertion here is about *which
 * characters come back in which order* rather than about boxes. The pages are
 * built character by character, which is verbose and is the point: the input is
 * geometry and nothing else, so a test that passed by accident would have to do
 * so on the geometry.
 *
 * The rotation tests are the ones worth reading. They never restate the sign
 * table in {@link axesFor} --- restating it would make them agree with it by
 * construction --- and instead assert the property it exists for: turning the
 * *view* does not change what the document says, so the order out of a rotated
 * page must equal the order out of the upright one. The turn is applied by
 * `turnedView`, which is `text.ts`'s own transform and has its own tests.
 *
 * Every test below was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import {
  axesFor,
  coveredText,
  cutWidth,
  hasSideBySideLines,
  readingBlocks,
  readingLines,
  readingOrder,
  readingTextOf,
  textOfRanges,
  usableRuns,
} from "./reading";
import { turnedView, type PageText, type TaggedRun } from "./text";

/** Builds a page from `(character, [left, top, right, bottom])` pairs. */
function page(
  chars: [string, [number, number, number, number] | null][],
  quarter_turns = 0,
): PageText {
  const codes: number[] = [];
  const boxes: number[] = [];
  for (const [char, box] of chars) {
    codes.push(char.codePointAt(0) ?? 0);
    boxes.push(...(box ?? [0, 0, 0, 0]));
  }
  return { codes, boxes, width_pt: 600, height_pt: 800, quarter_turns, extract_ms: 0 };
}

/** Ten-point characters laid left to right from `x`, on the band at `y`. */
function word(text: string, x: number, y: number): [string, [number, number, number, number]][] {
  return [...text].map((char, at) => {
    const left = x + at * 10;
    return [char, [left, y, left + 10, y + 12]] as [
      string,
      [number, number, number, number],
    ];
  });
}

/** The characters of a page, in the order they read. */
function readsAs(text: PageText): string {
  return readingOrder(text)
    .map((index) => String.fromCodePoint(text.codes[index] ?? 0))
    .join("");
}

/** Each line's text, which is what a screen reader is handed. */
function linesAs(text: PageText): string[] {
  return readingLines(text).map((line) =>
    line.ranges
      .flatMap((range) =>
        Array.from({ length: range.to - range.from }, (_, at) =>
          String.fromCodePoint(text.codes[range.from + at] ?? 0),
        ),
      )
      .join(""),
  );
}

/**
 * Two columns, laid out identically, with the lines emitted in `order`.
 *
 * `natural` emits column one and then column two; `interleaved` emits one line
 * from each in turn, which is what the fixture in `testdata` does and what
 * PDFium then hands back. The two must read the same.
 */
function columns(order: "natural" | "interleaved"): PageText {
  const rows = [
    ["ab", "AB"],
    ["cd", "CD"],
    ["ef", "EF"],
  ];
  const chars: [string, [number, number, number, number] | null][] = [];
  /** The `column`-th run on row `row`, where column 0 is at x=100 and 1 at 400. */
  const run = (row: number, column: 0 | 1) =>
    word(rows[row]?.[column] ?? "", column === 0 ? 100 : 400, 100 + row * 30);

  if (order === "natural") {
    for (const column of [0, 1] as const) {
      for (let row = 0; row < rows.length; row++) chars.push(...run(row, column));
    }
  } else {
    for (let row = 0; row < rows.length; row++) {
      chars.push(...run(row, 0), ...run(row, 1));
    }
  }
  return page(chars);
}

describe("readingOrder", () => {
  it("leaves a single column in the order it arrived", () => {
    // The control. Nothing here needs reordering, and an implementation that
    // shuffled the easy case would be worse than the one it replaced.
    const text = page([...word("ab", 100, 100), ...word("cd", 100, 130)]);
    expect(readsAs(text)).toBe("abcd");
  });

  it("reads two columns down and then across, however they were emitted", () => {
    // The whole feature in one assertion, and it is differential: two pages
    // with identical geometry and opposite emission order must agree.
    expect(readsAs(columns("natural"))).toBe("abcdefABCDEF");
    expect(readsAs(columns("interleaved"))).toBe("abcdefABCDEF");
  });

  it("keeps a heading that spans the columns above both of them", () => {
    // The case that defeats grouping by x position: the heading overlaps both
    // columns, so no vertical band of whitespace crosses the page, and only a
    // horizontal cut separates it.
    // The rows are far enough apart that the gap between *them* is also a
    // candidate cut. That is deliberate: with the rows close together, an
    // implementation that cut at every row gap and one that cut only at the
    // widest produced the same answer here, and the mutation survived.
    const text = page([
      ...word("ab", 100, 150),
      ...word("AB", 400, 150),
      ...word("cd", 100, 230),
      ...word("CD", 400, 230),
      // Emitted last, and further above the columns than they are apart from
      // each other -- see the note in `reading.ts` about why that margin is
      // what tells a heading from a first line.
      // Wide enough to cross the gutter. Written as a seven-character word
      // first, which reached from 100 to 170 and left the whole of 170..400
      // empty -- so a column cut was available after all, the heading was
      // quietly filed as the top of column one, and the answer came out right
      // by the wrong route. The same mistake, in the same session, as the one
      // the fixture generator has a comment about.
      ...word("HEADINGSPANNINGBOTHOFTHECOLUMNSBELOW", 100, 20),
    ]);
    expect(readsAs(text)).toBe("HEADINGSPANNINGBOTHOFTHECOLUMNSBELOWabcdABCD");
  });

  it("returns every character exactly once", () => {
    // A permutation, not a filter. Characters PDFium placed nowhere -- the
    // separators it synthesises between text objects -- are the ones an
    // implementation drops without noticing, and dropping them silently loses
    // the spaces between words on the clipboard.
    const text = page([
      ...word("ab", 100, 100),
      ["\n", null],
      ...word("cd", 100, 130),
      // Written as an escape rather than a literal NUL: a NUL byte in the
      // file makes git treat the whole test as **binary**, so every change to
      // it reviews as "Bin 12652 -> 22096 bytes" and no diff at all.
      ["\u0000", null],
    ]);
    const order = readingOrder(text);
    expect([...order].sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("has nothing to say about a page with no text", () => {
    expect(readingOrder(page([]))).toEqual([]);
    expect(readingLines(page([]))).toEqual([]);
  });

  it("keeps a page whose characters have no boxes in index order", () => {
    // A scanned page extracts characters PDFium can place nowhere. There is no
    // geometry to read an order out of, so the only honest answer is the one
    // that arrived -- and the characters must still all be there.
    const text = page([
      ["a", null],
      ["b", null],
      ["c", null],
    ]);
    expect(readsAs(text)).toBe("abc");
  });
});

describe("readingLines", () => {
  it("splits a band where the gap is wider than a few characters", () => {
    // The gutter, seen from the line's side: two runs at the same height are
    // two lines when they are far enough apart, and one line when they are not.
    expect(linesAs(page([...word("ab", 100, 100), ...word("cd", 400, 100)]))).toEqual([
      "ab",
      "cd",
    ]);
  });

  it("keeps a word space from being read as a column boundary", () => {
    // 25pt of gap against a 10pt character: a wide word space, as justification
    // produces, and not a gutter.
    //
    // Asserted through the *order* rather than through the line count, which was
    // the first version of this and could not fail: `readingLines` merges
    // fragments that share a band, so a line wrongly split into two fragments is
    // put back together before any assertion on lines can see it. What a wrong
    // threshold really costs is that the two halves are taken for two columns,
    // and then every line's second half is read after every line's first.
    const text = page([
      ...word("ab", 100, 100),
      ...word("cd", 145, 100),
      ...word("ef", 100, 130),
      ...word("gh", 145, 130),
    ]);
    expect(readsAs(text)).toBe("abcdefgh");
    expect(linesAs(text)).toEqual(["abcd", "efgh"]);
  });

  it("puts each column's lines in its own order", () => {
    expect(linesAs(columns("interleaved"))).toEqual(["ab", "cd", "ef", "AB", "CD", "EF"]);
  });
});

describe("axesFor", () => {
  it("reads a rotated page the same way it reads an upright one", () => {
    // The property the sign table exists for, asserted without restating it:
    // turning the view changes where the characters are and nothing about what
    // the document says. `turnedView` is `text.ts`'s transform, so this ties the
    // table to that rather than to a second copy of itself.
    const upright = columns("interleaved");
    const wanted = readsAs(upright);
    for (const turns of [1, 2, 3]) {
      expect(readsAs(turnedView(upright, turns))).toBe(wanted);
    }
  });

  it("puts lines across the page when it is turned a quarter", () => {
    // The one direct assertion on the table, and the coarsest: at a quarter turn
    // lines advance sideways, which is what stops a screen reader reading a
    // scanned page one character per line.
    expect(axesFor(1).sideways).toBe(true);
    expect(axesFor(3).sideways).toBe(true);
    expect(axesFor(0).sideways).toBe(false);
    expect(axesFor(2).sideways).toBe(false);
  });

  it("wraps a rotation outside 0 to 3", () => {
    expect(axesFor(4)).toEqual(axesFor(0));
    expect(axesFor(-1)).toEqual(axesFor(3));
  });
});

describe("cutWidth", () => {
  it("scales with the type rather than with the page", () => {
    // The same layout at twice the size has to cut in the same places, so the
    // threshold is a multiple of the character and not a constant in points.
    const small = page(word("abcd", 100, 100));
    const large = page(
      [..."abcd"].map((char, at) => [
        char,
        [100 + at * 20, 100, 120 + at * 20, 124] as [number, number, number, number],
      ]),
    );
    expect(cutWidth(large, axesFor(0))).toBe(cutWidth(small, axesFor(0)) * 2);
  });

  it("is not moved by one enormous character", () => {
    // A dropped capital, or a full-width rule that extracts as a character.
    // A mean would take the threshold past the gutter and stop cutting at all.
    const ordinary = page(word("abcde", 100, 100));
    const withDrop = page([
      ...word("abcde", 100, 100),
      ["Q", [0, 200, 500, 260]],
    ]);
    expect(cutWidth(withDrop, axesFor(0))).toBe(cutWidth(ordinary, axesFor(0)));
  });
});

describe("hasSideBySideLines", () => {
  it("is false for a single column, whatever its lines say", () => {
    // The control for the check it guards: on a one-column page the drag
    // ordering check must still run, and a helper that answered "yes" here
    // would silently retire it on every corpus.
    expect(
      hasSideBySideLines(page([...word("ab", 100, 100), ...word("cd", 100, 130)])),
    ).toBe(false);
  });

  it("is true where two lines sit at the same height", () => {
    expect(hasSideBySideLines(columns("interleaved"))).toBe(true);
    expect(hasSideBySideLines(columns("natural"))).toBe(true);
  });

  it("is false for a page with no text at all", () => {
    expect(hasSideBySideLines(page([]))).toBe(false);
  });
});

describe("readingTextOf", () => {
  it("emits a range in reading order rather than index order", () => {
    // The copy path. The selection is still a range of *indices*, so this takes
    // the whole interleaved page and has to hand back the columns in turn.
    const text = columns("interleaved");
    expect(readingTextOf(text, 0, text.codes.length)).toBe("abcdefABCDEF");
  });

  it("takes only the characters inside the range", () => {
    // A drag, rather than select-all: the characters outside the index range
    // must not appear however the ordering moves them about.
    const text = columns("interleaved");
    expect(readingTextOf(text, 0, 4)).toBe("abAB");
  });
});

describe("textOfRanges", () => {
  it("concatenates the ranges in the order it is given them", () => {
    // Deliberately *not* sorted: a line's ranges are already in reading order,
    // and re-sorting them here would undo the work upstream.
    const text = page(word("abcd", 100, 100));
    expect(
      textOfRanges(text, [
        { from: 2, to: 4 },
        { from: 0, to: 2 },
      ]),
    ).toBe("cdab");
  });
});

/**
 * A page whose tagged order is not its geometric one.
 *
 * The same shape as `testdata/make_tagged_pdf.py` builds and for the same
 * reason: a fixture whose two orders agree cannot tell a reader that used the
 * tags from one that ignored them. `note` sits to the left of `body`, on a band
 * above it, so every geometric rule reads it first --- and it is tagged last.
 *
 * The characters are emitted body-then-note, so index order is a third order
 * again and agrees with neither. That matters: it is what stops a tagged pass
 * that merely returned the characters in index order from passing.
 */
function marginNote(runs?: TaggedRun[]): PageText {
  const chars = [
    ...word("body", 200, 100),
    ...word("more", 200, 120),
    ...word("note", 40, 90),
  ];
  const text = page(chars);
  if (runs) text.runs = runs;
  return text;
}

/** The tagging `marginNote` is meant to be read with: body first, note last. */
const NOTE_LAST: TaggedRun[] = [
  { tag: "P", path: ["P"], start: 0, end: 8 },
  { tag: "Note", path: ["Note"], start: 8, end: 12 },
];

describe("usableRuns", () => {
  it("is null for a page the backend sent no runs for", () => {
    expect(usableRuns(marginNote())).toBeNull();
  });

  it("is null for a page whose runs leave a visible character unclaimed", () => {
    // The producer tagged the body and forgot the note. Using the runs would
    // drop "note" from what a screen reader reads, so the geometry --- wrong
    // about the order, right about the content --- wins.
    expect(usableRuns(marginNote([{ tag: "P", path: ["P"], start: 0, end: 8 }]))).toBeNull();
  });

  it("is the runs when they claim every visible character", () => {
    expect(usableRuns(marginNote(NOTE_LAST))).toHaveLength(2);
  });

  it("ignores an unclaimed character that is only whitespace", () => {
    // The space between two tagged elements on one line belongs to neither, and
    // it has a box like any other character. Rejecting a document over it would
    // reject every tagged document there is.
    //
    // Written with an unplaced space first, which was a weaker test than it
    // looks: an unplaced character is refused by the *other* half of the
    // condition, so removing the whitespace half changed nothing and the
    // mutation survived. The discriminating case is whitespace that is placed.
    const text = page([
      ...word("ab", 100, 100),
      [" ", [120, 100, 130, 112]],
      ...word("cd", 140, 100),
    ]);
    text.runs = [
      { tag: "P", path: ["P"], start: 0, end: 2 },
      { tag: "P", path: ["P"], start: 3, end: 5 },
    ];
    expect(usableRuns(text)).toHaveLength(2);
  });

  it("ignores an unclaimed character PDFium placed nowhere", () => {
    // The other half. A character with no box is invisible whatever its code
    // says --- these are the separators PDFium synthesises between text objects
    // --- so an untagged one is not a hole in the reading order.
    const text = page([...word("ab", 100, 100), ["x", null], ...word("cd", 100, 120)]);
    text.runs = [
      { tag: "P", path: ["P"], start: 0, end: 2 },
      { tag: "P", path: ["P"], start: 3, end: 5 },
    ];
    expect(usableRuns(text)).toHaveLength(2);
  });
});

describe("readingLines with a structure tree", () => {
  it("reads the page in the order the tags give", () => {
    expect(linesAs(marginNote(NOTE_LAST))).toEqual(["body", "more", "note"]);
  });

  it("reads the same page geometrically when the tags are absent", () => {
    // The control, and the half that can fail. Without it, "the tags were read"
    // and "the tags happened to agree with the geometry" look the same --- and
    // this asserts the geometry genuinely produces a different answer, so the
    // test above is measuring something.
    expect(linesAs(marginNote())).toEqual(["note", "body", "more"]);
  });

  it("follows the tags even where they disagree with the geometry entirely", () => {
    // Reversed tagging: nothing about the boxes suggests it, so an
    // implementation that ordered blocks by position and only *grouped* by tag
    // would fail here and pass the test above.
    const reversed: TaggedRun[] = [
      { tag: "Note", path: ["Note"], start: 8, end: 12 },
      { tag: "P", path: ["P"], start: 4, end: 8 },
      { tag: "P", path: ["P"], start: 0, end: 4 },
    ];
    expect(linesAs(marginNote(reversed))).toEqual(["note", "more", "body"]);
  });

  it("splits a run into lines by geometry", () => {
    // A tagged run is a paragraph and a screen reader is handed lines, so the
    // one run above covering "body" and "more" has to come back as two.
    const text = marginNote([{ tag: "P", path: ["P"], start: 0, end: 12 }]);
    expect(linesAs(text)).toEqual(["note", "body", "more"]);
  });

  it("clips a fragment that straddles two runs", () => {
    // Two tagged words side by side on one line are one *fragment*, because
    // nothing separates them geometrically. Assigning the fragment to whichever
    // run its first character falls in would put "two" in the first run and
    // leave the second empty.
    const text = page(word("onetwo", 100, 100));
    text.runs = [
      { tag: "P", path: ["P"], start: 3, end: 6 },
      { tag: "P", path: ["P"], start: 0, end: 3 },
    ];
    expect(linesAs(text)).toEqual(["two", "one"]);
  });

  it("puts the copy path in the tagged order too", () => {
    // The runs reach `readingTextOf` through the same funnel, which is the
    // reason they were put on `PageText` rather than fetched separately.
    const text = marginNote(NOTE_LAST);
    expect(readingTextOf(text, 0, text.codes.length)).toBe("bodymorenote");
  });
});

describe("a line with punctuation on it", () => {
  /**
   * Real boxes from `testdata/tagged.pdf`, which is where this was found.
   *
   * The numbers are the point: letters occupy 227.41--236.13, a comma starts
   * inside that and drops to 237.69, and a space is 0.01 pt tall sitting on the
   * baseline. Rounded or idealised, none of this reproduces.
   */
  function withComma(): PageText {
    const chars: [string, [number, number, number, number] | null][] = [];
    let x = 170;
    for (const char of "ab, cd") {
      if (char === ",") chars.push([char, [x, 234.8, x + 1.3, 237.69]]);
      else if (char === " ") chars.push([char, [x, 235.99, x + 3.3, 236.0]]);
      else chars.push([char, [x, 227.41, x + 5, 236.13]]);
      x += 6;
    }
    return page(chars);
  }

  it("is one line, not a line of letters and a line of marks", () => {
    // What it did instead: "abcd", and a second line holding ", " --- because the
    // comma overlaps a band of letters by 46% of itself, so it opened a band of
    // its own, and the space then matched *that* band by 100% of itself. Read
    // aloud and copied exactly like that.
    expect(linesAs(withComma())).toEqual(["ab, cd"]);
  });

  it("does not make two real lines into one", () => {
    // The control for the rule that fixes it: the risk of "a short box joins the
    // line it touches" is a *line* joining the line above.
    //
    // The two lines have to **overlap** for this to discriminate. Written first
    // with 16 pt of leading between 12 pt boxes, it could not fail --- the boxes
    // do not touch, so the guard above the new rule refuses them whatever the
    // rule says, and a mutation that merged everything left it green. Real text
    // lines overlap by their ascenders and descenders, which is the case worth
    // holding.
    const text = page([
      ...word("ab", 170, 100),
      ...[..."cd"].map(
        (char, at) =>
          [char, [170 + at * 10, 110, 180 + at * 10, 122]] as [
            string,
            [number, number, number, number],
          ],
      ),
    ]);
    expect(linesAs(text)).toEqual(["ab", "cd"]);
  });
});

describe("a space whose font floated its box off the line", () => {
  /**
   * The `multilingual.pdf` folding page as laid out in `msgothic.ttc`, which is
   * the substitute Windows picks and macOS does not.
   *
   * Measured through `FPDFText_GetCharBox` and recorded in `BUILD.md`: the space
   * comes back **placed**, 0.02 pt tall, with its band 0.12 pt clear of the
   * 13.94 pt band every letter on the line sits in. The offsets are the point
   * and the origin is not, so they are laid out here from 100 in `charQuad`'s
   * frame (`text.ts`: PDF points from the page's *top-left*, so below the line
   * is the larger number) rather than at the y the fixture happens to use.
   *
   * This is a Windows defect reproduced on any platform, because the input is
   * geometry. The macOS run of the same fixture is green under Arial Unicode,
   * whose space sits inside the letters' band --- so the corpus cannot discriminate
   * here and only these numbers can.
   */
  function floatedSpace(): PageText {
    const chars: [string, [number, number, number, number] | null][] = [];
    let x = 170;
    for (const char of "ab cd") {
      if (char === " ") chars.push([char, [x, 114.06, x + 3.3, 114.08]]);
      else chars.push([char, [x, 100, x + 5, 113.94]]);
      x += 6;
    }
    return page(chars);
  }

  it("stays on the line, in its own place", () => {
    // What it did instead: "abcd" --- the space matched no band, so it became a
    // fragment of its own and fell out of the line's ranges entirely. The
    // fixture's own line read `cafélatte`.
    //
    // Asserting the whole line rather than "the space is somewhere in it": a
    // character re-attached to the wrong index gives `abcd ` and reads aloud
    // exactly as wrong as dropping it.
    expect(linesAs(floatedSpace())).toEqual(["ab cd"]);
  });

  /**
   * The `encodings.pdf` predefined-CMap page, measured through
   * `FPDFText_GetCharBox` against the vendored library on Windows: the font has
   * no embedded metrics, so PDFium reports **every** character 0.018 pt tall ---
   * the two lines at y 89.982--90.000 and 721.982--722.000, 632 pt apart, with
   * the `\r\n` between them placed nowhere.
   *
   * This is the sample that says the sliver rule cannot be absolute. Under one,
   * every character here is refused, nothing is placed, and `fragmentsOf`
   * returns the whole page as a single fragment: the two lines came back as one,
   * read aloud and copied that way.
   */
  function degenerateMetrics(): PageText {
    const chars: [string, [number, number, number, number] | null][] = [];
    for (const [line, top] of [
      [0, 89.982],
      [1, 721.982],
    ] as const) {
      let x = 60;
      for (const char of "日本語の符号") {
        chars.push([char, [x, top, x + 18, top + 0.018]]);
        x += 18;
      }
      if (line === 0) {
        chars.push(["\r", [168, 90, 168, 90]]);
        chars.push(["\n", [168, 90, 168, 90]]);
      }
    }
    return page(chars);
  }

  it("does not refuse a page whose every glyph is that thin", () => {
    // Two lines, not one. Asserting both lines rather than the count: a rule
    // that split the page anywhere would give two lines of the wrong text, and
    // the defect this replaces was legible only in the text.
    expect(linesAs(degenerateMetrics())).toEqual(["日本語の符号\r\n", "日本語の符号"]);
  });

  it("is not thrown off by one glyph the page did find metrics for", () => {
    // The control for the median, which a maximum would fail. Written because
    // the maximum passed every other test here: a page mostly without metrics
    // needs only one substituted glyph -- which the broken-map page of the same
    // fixture shows is an ordinary thing for this document to contain -- and a
    // maximum then reads 13 pt as typical, calls every real character of the
    // page a twentieth of it, and collapses the two lines exactly as before.
    const chars: [string, [number, number, number, number] | null][] = [];
    for (const [line, top] of [
      [0, 89.982],
      [1, 721.982],
    ] as const) {
      let x = 60;
      for (const char of "日本語の符号") {
        // One glyph on the first line has real metrics; the rest have none.
        const tall = line === 0 && char === "語";
        chars.push([char, tall ? [x, top - 13, x + 18, top + 0.018] : [x, top, x + 18, top + 0.018]]);
        x += 18;
      }
      if (line === 0) {
        chars.push(["\r", [168, 90, 168, 90]]);
        chars.push(["\n", [168, 90, 168, 90]]);
      }
    }
    expect(linesAs(page(chars))).toEqual(["日本語の符号\r\n", "日本語の符号"]);
  });

  it("does not refuse small real type on a page of large type", () => {
    // The control for the *absolute* half of the conjunction, and the only
    // constructed geometry here: no corpus has display type, so this is the case
    // the clause exists for rather than a case that has occurred. A 5 pt
    // character on a page whose median is 200 pt is a twentieth of it, so the
    // relative clause alone would refuse it and append it to the line before --
    // and 5 pt is ordinary footnote type, not bookkeeping.
    const chars: [string, [number, number, number, number] | null][] = [
      ["A", [60, 100, 260, 300]],
      ["B", [260, 100, 460, 300]],
      ["1", [60, 400, 65, 405]],
    ];
    expect(linesAs(page(chars))).toEqual(["AB", "1"]);
  });

  it("does not swallow a mark that is merely short", () => {
    // The control for the rule, and it has to be a *near* miss to certify
    // anything. `tagged.pdf`'s real comma is 2.89 pt tall --- short enough to
    // need `SHORT_MARK`, and 29 times the sliver threshold. If the new rule were
    // "a short box joins by index" rather than "a box with no height does", this
    // is what it would break.
    //
    // Asserted on the *box*, because it reads the same either way: a character
    // routed by index keeps its place in the ranges and stops contributing its
    // extent, so the only observable is that the line no longer reaches the
    // comma's descender.
    const chars: [string, [number, number, number, number] | null][] = [
      ["a", [170, 227.41, 175, 236.13]],
      ["b", [176, 227.41, 181, 236.13]],
      [",", [182, 234.8, 183.3, 237.69]],
    ];
    const [line] = readingLines(page(chars));
    expect(line?.box.bottom).toBeCloseTo(237.69);
  });
});

describe("characters no tagged run claims", () => {
  /**
   * Two tagged blocks with an unplaced separator between them, unclaimed.
   *
   * Tagged **note first**, which is what makes the placement of the separator
   * observable at all: with the blocks in index order, attaching it to the block
   * before and attaching it to the block after produce the same string, and a
   * check on the direction cannot fail.
   */
  function withSeparator(): PageText {
    const text = page([
      ...word("body", 200, 100),
      ["\n", null],
      ...word("note", 40, 90),
    ]);
    text.runs = [
      { tag: "Note", path: ["Note"], start: 5, end: 9 },
      { tag: "P", path: ["P"], start: 0, end: 4 },
    ];
    return text;
  }

  it("are still in the reading order", () => {
    // The invariant: the tagged order is a permutation of every character index,
    // exactly as the geometric one is. Emitting only the claimed characters made
    // a page come back six characters shorter than the page --- one separator per
    // line break between paragraphs --- which the accessibility check caught as a
    // multiset mismatch and select-all caught as a count.
    const text = withSeparator();
    expect(readingOrder(text)).toHaveLength(text.codes.length);
  });

  it("stay with the text they follow", () => {
    // Not merely present: in the right place. The separator after "body" belongs
    // to the block before it, which is the rule `fragmentsOf` already uses for a
    // character PDFium placed nowhere.
    expect(readingTextOf(withSeparator(), 0, 9)).toBe("notebody\n");
  });
});

describe("readingBlocks", () => {
  it("carries each tagged run's type", () => {
    // What `a11y.ts` needs and `readingLines` cannot say: a line does not know
    // whether it is part of a heading.
    const text = marginNote([
      { tag: "H1", path: ["H1"], start: 0, end: 4 },
      { tag: "P", path: ["P"], start: 4, end: 8 },
      { tag: "Note", path: ["Note"], start: 8, end: 12 },
    ]);
    expect(readingBlocks(text).map((b) => b.tag)).toEqual(["H1", "P", "Note"]);
  });

  it("reports an inferred block as having no type", () => {
    // Not `"P"`. The geometry drew this boundary, and a consumer that treated it
    // as the producer's statement would announce a guessed paragraph break as a
    // real one --- which is why `a11y.ts` reads these line by line instead.
    expect(readingBlocks(marginNote()).every((b) => b.tag === null)).toBe(true);
  });

  it("flattens to exactly what readingLines gives", () => {
    // The two must not be able to disagree about the order, which is the whole
    // reason `readingLines` is written in terms of this rather than beside it.
    const text = marginNote(NOTE_LAST);
    expect(readingBlocks(text).flatMap((b) => b.lines)).toEqual(readingLines(text));
  });
});

describe("a combining mark", () => {
  /**
   * A decomposed word whose letters have **no ascender**, with a second word
   * after it so a broken line has somewhere wrong to go.
   *
   * The geometry is the measured one from `testdata/multilingual.pdf`: the acute
   * sits above the x-height and its box does not touch the letters' band. Boxes
   * here are device-space --- y downwards --- so the mark's top is the *smaller*
   * number, and it therefore sorts ahead of every letter on the line.
   */
  function decomposed(): PageText {
    const letters = (text: string, x: number): [string, [number, number, number, number]][] =>
      [...text].map((char, at) => {
        const left = x + at * 10;
        return [char, [left, 124, left + 10, 134]] as [
          string,
          [number, number, number, number],
        ];
      });
    return page([
      ...letters("resume", 60),
      // Above the x-height, 0.96pt clear of it: 120.7 to 123.36 against 124 to
      // 134. No overlap at all, in either direction.
      ["́", [110, 120.7, 113, 123.36]],
      [" ", null],
      // One character-width along, not a column away: at 200 the gap reads as a
      // gutter and `fragmentsOf` splits the line for that reason instead, which
      // is a different rule passing a test aimed at this one.
      ...letters("souvenu", 130),
      ["́", [190, 120.7, 193, 123.36]],
    ]);
  }

  it("does not open a line of its own", () => {
    // The defect this rule exists for. Before it, this page read as three lines
    // --- `resume`, the accent alone, and `souvenu` --- and the accessibility tree
    // announced them that way.
    expect(linesAs(decomposed())).toEqual(["resumé souvenú"]);
  });

  it("stays with the character it decorates", () => {
    // Present *and* in the right place. Attaching it to the following character
    // would read `resum` `é` the wrong way round, and a check on the line count
    // alone cannot see that.
    expect(readsAs(decomposed())).toBe("resumé souvenú");
  });

  it("is covered by its line's box", () => {
    // The accent is folded into its base's box rather than merely tolerated, so
    // hit-testing the line still reaches the top of the accent. 120.7 is above
    // the letters' own 124.
    const [line] = readingLines(decomposed());
    expect(line?.box.top).toBeCloseTo(120.7, 2);
  });

  it("with no character before it keeps its own band", () => {
    // A page that opens with a mark has no base for it. Nothing sensible can be
    // done, and inventing an attachment to the character *after* it would be
    // wrong in the one direction that reorders text.
    const text = page([["́", [110, 120.7, 113, 123.36]], ...word("after", 60, 124)]);
    expect(readsAs(text)).toBe("́after");
  });

  it("keys on the character rather than on the box", () => {
    // The control, and it took three attempts to make it able to fail. A rule
    // keyed on geometry --- a small box, raised --- would catch a superscript,
    // which is a character in its own right with its own advance width.
    //
    // What it cannot be is an *order* assertion. Within one fragment the order is
    // index order, so a raised digit beside its neighbours reads the same whether
    // it is a character or a mark, and two arrangements were tried before that
    // was clear. What does differ is how many lines there are: a digit widened
    // into the mark class attaches to the character before it, and the line below
    // disappears into the line above.
    const text = page([...word("ab", 60, 100), ...word("12", 60, 140)]);
    expect(linesAs(text)).toEqual(["ab", "12"]);
  });
});

describe("coveredText", () => {
  /** Two words on one line, with a second line under them. */
  function marked(): PageText {
    return page([...word("alpha", 100, 700), ...word("beta", 100, 720)]);
  }

  it("reads the characters whose centres are inside the rectangle", () => {
    // The rectangle covers the first line and nothing else. `alpha` is five
    // 10-point characters from x=100, so it ends at 150.
    expect(coveredText(marked(), [100, 700, 150, 712])).toBe("alpha");
  });

  it("takes a character whose centre is inside and leaves one that only overlaps", () => {
    // Cut through the middle of the third character: `l` and `p` have centres
    // at 115 and 135, and the rectangle reaches 130. Overlap would take `p`
    // too, and that is the whole difference this rule exists for --- a
    // highlight's rectangle routinely touches the words either side of it.
    expect(coveredText(marked(), [100, 700, 130, 712])).toBe("alp");
  });

  it("reads several rectangles as one phrase, in the page's order", () => {
    // One quad per line, which is the shape a real `/QuadPoints` has --- and
    // handed in bottom line first, so the answer cannot be the order they came.
    expect(
      coveredText(marked(), [100, 720, 140, 732, 100, 700, 150, 712]),
    ).toBe("alphabeta");
  });

  it("reads a highlight across a gutter column by column, not line by line", () => {
    // The case that makes this worth doing through `readingOrder` rather than
    // by sorting the covered indices. The file is written a line at a time
    // across both columns, so index order is `oneAAAtwoBBB` --- the columns
    // interleaved --- and reading order is `onetwoAAABBB`.
    //
    // Both columns are covered on purpose. A rectangle over one column alone
    // yields the same string either way, so a fixture that highlighted only the
    // left column would pass whichever rule ran.
    const two = page([
      ...word("one", 100, 700),
      ...word("AAA", 400, 700),
      ...word("two", 100, 720),
      ...word("BBB", 400, 720),
    ]);
    expect(coveredText(two, [90, 690, 500, 740])).toBe("onetwoAAABBB");
  });

  it("is empty for no rectangles, and for rectangles over nothing", () => {
    expect(coveredText(marked(), [])).toBe("");
    expect(coveredText(marked(), [400, 400, 500, 450])).toBe("");
  });

  it("does not take a character the page placed nowhere", () => {
    // Four zeroes is PDFium's "not placed", and its centre is the page's
    // top-left corner --- inside any rectangle anchored there. A highlight on
    // the first line of a page is exactly such a rectangle.
    //
    // **The unplaced character goes last, and the first draft had it first,
    // where this test could not fail.** `fragmentsOf` re-attaches a character it
    // could not place to the one *before* it, and a leading one has nothing
    // before it --- measured, it is dropped from `readingOrder` altogether, so
    // `coveredText` could never have emitted it whatever the rule under test
    // did. Caught by the mutation harness, which reported the edit red in
    // `links.test.ts` and green here.
    const withGap = page([...word("hi", 0, 0), ["x", null]]);
    expect(coveredText(withGap, [0, 0, 20, 12])).toBe("hi");
  });
});
