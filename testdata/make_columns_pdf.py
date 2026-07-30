#!/usr/bin/env python3
"""Generates a multi-column document whose content-stream order is adversarial.

Nothing else in the corpus has columns, and columns are where "copy the text"
stops being a matter of concatenating characters. A PDF carries no reading
order: it carries glyphs at positions, in whatever sequence the producer chose
to emit them. PDFium hands them back in that sequence, so a two-column page
whose producer emitted line-by-line across the gutter extracts as

    alpha one beta one alpha two beta two ...

which is what a reader gets on the clipboard, and what a screen reader reads
aloud. Recovering the intended order is geometry, and geometry is the only
thing available.

The three pages are one question each, and the first two are the whole point:

  1. **natural** -- two columns, the content stream emitting all of column one
     and then all of column two. Index order already *is* reading order.
  2. **interleaved** -- the same page, visually identical to within a rounding
     error, with the content stream emitting one line from each column in turn.
  3. **heading** -- a full-width heading over interleaved columns. The case a
     naive x-position clustering gets wrong: the heading spans both columns, so
     an algorithm that assigns every line to a column either splits the heading
     or merges the columns.

**Pages 1 and 2 must extract to the same text.** That is the assertion the
fixture exists for, and it is a differential one: two files laid out the same
way and emitted differently, compared against each other rather than against
this script's idea of what either says. `AGENTS.md` records why that matters ---
a writer and its own reader agree about output that is wrong --- and here the
comparison is between two documents that a correct implementation cannot tell
apart, which no amount of self-consistency can satisfy.

`columns-manifest.json` states the expected reading order per page, so a probe
reads it rather than carrying a copy of these strings.

Base-14 Helvetica, so this needs no font file and nothing it writes is anyone
else's to redistribute. The output is gitignored.

Usage: python3 make_columns_pdf.py <outdir>
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

#: Left edge of each column, and the gutter between them, in points.
LEFT_X = 60
RIGHT_X = 320
#: The gutter is 60pt wide at 12pt type, which is a wide one --- deliberately.
#: A fixture that only works at the tightest spacing tests the constant rather
#: than the algorithm, and a *narrow* gutter belongs in its own fixture once
#: there is a threshold worth probing.
COLUMN_WIDTH = 200

#: The columns start well below the heading, because that is what typography
#: does and because the algorithm depends on it: a spanning element is separated
#: from the body by a horizontal band of whitespace *wider than the body's line
#: leading*, and that is what tells the two apart. With the two gaps equal the
#: page is genuinely ambiguous, and `reading.ts` says so rather than pretending
#: otherwise.
TOP_Y = 700
LINE_STEP = 30
LINES_PER_COLUMN = 10

#: Long enough, and set large enough, that its box crosses the gutter between
#: the columns. A short heading sitting entirely above column one leaves the
#: gutter empty for the full height of the text, so a vertical cut alone would
#: separate the columns correctly and the page would not test the heading case
#: at all --- it would just be page 2 with an extra line.
HEADING = "Heading that spans across both columns of this page"
HEADING_SIZE = 16
HEADING_Y = 786

#: Distinct per column and per line, so a scrambled extraction names itself
#: rather than merely being wrong.
ORDINALS = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
]


def column_lines(word: str) -> "list[str]":
    """The lines of one column, as they should be read."""
    return [f"{word} {ordinal}" for ordinal in ORDINALS[:LINES_PER_COLUMN]]


def show(x: int, y: int, text: str, size: int = 12) -> str:
    """One text object at a point, which is one line on the page."""
    return "BT /F1 %d Tf %d %d Td (%s) Tj ET\n" % (
        size,
        x,
        y,
        escape(text).decode("latin-1"),
    )


def gutter_rule() -> str:
    """A hairline down the gutter, so the two columns are visibly two.

    Drawn rather than implied: a human opening the fixture to see why a check
    failed should be able to see the layout, and a vector operator among the
    text also gives the extraction something that is not a glyph to skip.
    """
    middle = (LEFT_X + COLUMN_WIDTH + RIGHT_X) // 2
    bottom = TOP_Y - LINE_STEP * LINES_PER_COLUMN
    return "q 0.7 0.7 0.7 RG 0.5 w %d %d m %d %d l S Q\n" % (
        middle,
        bottom,
        middle,
        TOP_Y + 12,
    )


def natural_page() -> "tuple[str, list[str]]":
    """Column one in full, then column two. Index order is reading order."""
    left, right = column_lines("alpha"), column_lines("beta")
    content = gutter_rule()
    for index, line in enumerate(left):
        content += show(LEFT_X, TOP_Y - index * LINE_STEP, line)
    for index, line in enumerate(right):
        content += show(RIGHT_X, TOP_Y - index * LINE_STEP, line)
    return content, left + right


def interleaved_page() -> "tuple[str, list[str]]":
    """One line from each column in turn. Index order is not reading order."""
    left, right = column_lines("alpha"), column_lines("beta")
    content = gutter_rule()
    for index in range(LINES_PER_COLUMN):
        y = TOP_Y - index * LINE_STEP
        content += show(LEFT_X, y, left[index])
        content += show(RIGHT_X, y, right[index])
    return content, left + right


def heading_page() -> "tuple[str, list[str]]":
    """A full-width heading over interleaved columns.

    The heading is emitted *last*, after both columns, so a fixture that
    happened to work by following the content stream cannot pass this page by
    accident either.
    """
    content, order = interleaved_page()
    content += show(LEFT_X, HEADING_Y, HEADING, HEADING_SIZE)
    return content, [HEADING] + order


def build(path: str) -> "list[dict]":
    """Writes the three-page fixture and returns what each page should read as."""
    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    pages_ref = pdf.reserve()

    manifest = []
    page_refs = []
    for name, make in (
        ("natural", natural_page),
        ("interleaved", interleaved_page),
        ("heading", heading_page),
    ):
        content, order = make()
        stream = pdf.stream(b"<< >>", content.encode("latin-1"))
        page_refs.append(
            pdf.add(
                b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
                b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
                % (pages_ref, WIDTH, HEIGHT, font, stream)
            )
        )
        manifest.append({"page": len(page_refs) - 1, "name": name, "lines": order})

    kids = b" ".join(b"%d 0 R" % ref for ref in page_refs)
    pdf.put(
        pages_ref,
        b"<< /Type /Pages /Count %d /Kids [%s] >>" % (len(page_refs), kids),
    )
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_ref)

    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))
    return manifest


def main() -> int:
    """Writes `columns.pdf` and its manifest into the output directory."""
    outdir = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(outdir, exist_ok=True)

    path = os.path.join(outdir, "columns.pdf")
    manifest = build(path)
    print(f"[OK]   wrote {path} ({os.path.getsize(path)} bytes)")

    # Stated here rather than derived by a probe, so the probe has something
    # independent to be wrong against.
    sidecar = os.path.join(outdir, "columns-manifest.json")
    with open(sidecar, "w", encoding="utf-8") as handle:
        json.dump({"pages": manifest}, handle, ensure_ascii=False, indent=2)
    print(f"[OK]   wrote {sidecar}")

    natural = next(p for p in manifest if p["name"] == "natural")
    shuffled = next(p for p in manifest if p["name"] == "interleaved")
    if natural["lines"] != shuffled["lines"]:
        print("[FAIL] the two column pages must expect the same reading order")
        return 1
    print(f"[OK]   pages 1 and 2 expect the same {len(natural['lines'])} lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())
