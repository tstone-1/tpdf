#!/usr/bin/env python3
"""Generates the comment (annotation) corpus for the comment layer.

Nothing else in `testdata/` carries an annotation a reader would call a comment.
`text-marked.pdf` has one `/Contents` entry, put there as a *carrier* for the
redaction spike rather than as something to read, and every other fixture has
none at all --- so an extractor written against the existing corpus would be
exercised by one string.

One document, five pages, each with a property of its own:

  page 0  The ordinary case. A sticky note with an author, a date and a body; a
          reply to it by a second author; a highlight carrying a comment; a
          square with its own appearance stream; a note with no body at all;
          and --- deliberately --- a `/Link` with a `/URI` action and a form
          `/Widget`, neither of which is a comment and neither of which may
          appear in the answer.

  page 1  Strings and dates. PDF 32000-1 gives a text string two encodings and
          PDF 2.0 adds a third, and a producer picks whichever suits it: an
          author in UTF-16BE with an astral character in it, a body in
          PDFDocEncoding using a byte that is a *control* in Latin-1 and a
          quotation mark here, a body in UTF-8, a body carrying newlines and
          control characters, a date with a timezone offset, and a date that is
          not a date.

  page 2  The hostile page. A 60,000-character body; two notes that reply to
          each other; a reply to an annotation on another page; a reply to an
          object that is not an annotation; a popup with no parent; a rectangle
          written backwards; a rectangle at 1e10; a hidden annotation; an
          annotation with no `/Subtype`; an `/Annots` entry pointing at an
          object that does not exist; an entry that is not a dictionary at all;
          and 1,200 notes, which is past any per-page bound worth having.

  page 3  `/Annots` as an indirect reference to an array rather than written
          inline. `AGENTS.md` records that this distinction decides how large an
          annotation edit is; here it decides only whether the scan resolves it,
          and a scan that does not finds no comments on this page.

And a second file, `comments-rotated.pdf`: one page carrying `/Rotate 90` with a
note at a known place, which is what says the scan reports rectangles in display
space. It is separate because a rotated page in an upright document makes the
document mixed-size, and `viewer_check.py`'s rotation checks derive their
expected zoom from page 1's aspect --- so inside `comments.pdf` it turned two of
them red against a viewer that was behaving as designed.

Writes `comments-corpus.json` next to them, keyed by file name, so
`examples/comments-probe` reads its expectations rather than hardcoding them.

**Not `comments-manifest.json`, and the suffix is the reason.** `viewer_check.py`
binds any `<fixture>-manifest.json` to `TPDF_READING_MANIFEST`, which *enrols*
the fixture in the reading-order check --- so a sidecar under that name is handed
to a consumer expecting a list of pages and their lines. It was called that for
one commit, and the window harness died on `{} is not iterable` sixteen checks
in, taking the other 155 with it. `mixed.pdf` avoids the same collision by
writing `mixed-geometry.json`; this is the same dodge.

The output is gitignored. Usage:
    python3 testdata/make_comments_pdf.py [outdir]
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

#: Characters in the oversized body. Far past any bound worth having, and small
#: enough that the fixture still writes in a blink.
HUGE_BODY_CHARS = 60_000

#: Notes on the crowded page. Past a per-page bound of 1,000 by enough that
#: "the bound fired" and "the page was simply busy" cannot be confused.
CROWD = 1_200


def literal(raw: bytes) -> bytes:
    """Wraps bytes as a PDF literal string, escaping what has to be escaped.

    Bytes rather than `str` on purpose: three of the strings below are chosen
    for their *encoding*, and going through a Python string would decide that
    question before the file does.
    """
    for char in (b"\\", b"(", b")"):
        raw = raw.replace(char, b"\\" + char)
    return b"(" + raw + b")"


def utf16be(value: str) -> bytes:
    """A text string in UTF-16BE with the byte-order mark that declares it."""
    return literal(b"\xfe\xff" + value.encode("utf-16-be"))


def utf8(value: str) -> bytes:
    """A text string in UTF-8 with the byte-order mark PDF 2.0 added."""
    return literal(b"\xef\xbb\xbf" + value.encode("utf-8"))


#: Lines of text on every page.
#:
#: Enough to reach the bottom of the page, which is a requirement rather than a
#: choice: `viewer_check.py` drags at two fixed heights and asserts the lower one
#: comes from later in the page's text. Ten lines left the lower drag below the
#: last of them, where it caught a stray character and the check reported the
#: page reading bottom to top.
ROWS = 36

#: Leading, in points. Tight enough that the gap between two lines is smaller
#: than a line, so a drag at an arbitrary height lands *on* one rather than
#: between two --- where it catches a stray character, and a one-character
#: selection cannot be located in the page's text at all.
LEADING = 18

#: Words a line is built from. Every one is at least four characters, and there
#: is deliberately no bare digit: `viewer_check.py` double-clicks at a fixed
#: point and asserts it selected a *word*, and a line reading "... line 0" put a
#: single character under that point --- which reads as a viewer with no notion
#: of granularity rather than as a fixture with nothing to select.
WORDS = [
    "alpha",
    "bravo",
    "charlie",
    "delta",
    "echo",
    "foxtrot",
    "golf",
    "hotel",
]


def body_content(rows: int, label: str) -> bytes:
    """Page content: a few lines of words, so the page is not blank behind the marks.

    Long enough lines that a drag across the middle of the page lands on one.
    """
    lines = []
    for row in range(rows):
        y = HEIGHT - 80 - row * LEADING
        words = " ".join(WORDS[(row + at) % len(WORDS)] for at in range(6))
        lines.append(
            "BT /F1 13 Tf 72 %d Td (%s) Tj ET"
            % (y, escape(f"{label} line {row:02d}: {words}").decode("latin-1"))
        )
    return "\n".join(lines).encode("latin-1")


class Page:
    """One page under construction, and the annotations hanging off it."""

    def __init__(self, pdf: Pdf, pages: int, font: int, label: str, rotate: int = 0):
        """Reserves the page object so annotations can name it before it exists."""
        self.pdf = pdf
        self.number = pdf.reserve()
        self.pages = pages
        self.font = font
        self.rotate = rotate
        self.content = pdf.stream(b"<< >>", body_content(ROWS, label))
        self.entries: list[bytes] = []

    def annot(self, body: bytes, number: "int | None" = None) -> int:
        """Adds an annotation dictionary, filling in `/P` and listing it."""
        body = body.strip()
        assert body.startswith(b"<<") and body.endswith(b">>")
        full = body[:-2] + b" /P %d 0 R >>" % self.number
        placed = self.pdf.add(full) if number is None else self.pdf.put(number, full)
        self.entries.append(b"%d 0 R" % placed)
        return placed

    def raw(self, entry: bytes) -> None:
        """Puts something in `/Annots` that is not one of our annotations.

        Position matters and is the caller's to choose: an entry written after
        the crowd below is never reached, because the per-page bound stops the
        scan first --- which is how the first draft of this fixture exercised
        none of the three malformed entries while looking as though it did.
        """
        self.entries.append(entry)

    def finish(self, indirect: bool = False) -> int:
        """Writes the page dictionary."""
        array = b"[ " + b" ".join(self.entries) + b" ]"
        if indirect:
            array = b"%d 0 R" % self.pdf.add(array)
        rotate = b" /Rotate %d" % self.rotate if self.rotate else b""
        self.pdf.put(
            self.number,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [ 0 0 %d %d ]%s "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R "
            b"/Annots %s >>"
            % (self.pages, WIDTH, HEIGHT, rotate, self.font, self.content, array),
        )
        return self.number


def build(path: str) -> dict:
    """Writes the fixture and returns what a probe should expect to find in it."""
    pdf = Pdf()
    pages = pdf.reserve()
    font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    expected: dict = {"pages": {}}

    # ---------------------------------------------------------------- page 0
    ordinary = Page(pdf, pages, font, "Ordinary")

    popup = pdf.reserve()
    note = ordinary.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 300 700 324 724 ] "
        b"/Contents " + literal(b"The figure on page 2 contradicts this.") + b" "
        b"/T " + literal(b"Timo") + b" /M (D:20260812101500Z) /Name /Comment "
        b"/C [ 1 0.82 0.2 ] /Popup %d 0 R >>" % popup
    )
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Popup /Rect [ 330 620 530 724 ] "
        b"/Parent %d 0 R /Open true >>" % note,
        number=popup,
    )
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 300 660 324 684 ] "
        b"/Contents " + literal(b"It does not - page 2 is the revised figure.") + b" "
        b"/T " + literal(b"Reviewer") + b" /M (D:20260812114000Z) "
        b"/IRT %d 0 R /RT /R >>" % note
    )
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Highlight /Rect [ 70 640 320 662 ] "
        b"/QuadPoints [ 72 660 300 660 72 642 300 642 ] /C [ 1 1 0 ] "
        b"/Contents " + literal(b"Check this claim.") + b" "
        b"/T " + literal(b"Timo") + b" /M (D:20260812101600Z) >>"
    )
    appearance = pdf.stream(
        b"<< /Type /XObject /Subtype /Form /BBox [ 0 0 120 60 ] >>",
        b"1 0 0 RG 4 w 2 2 116 56 re S",
    )
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Square /Rect [ 380 500 500 560 ] "
        b"/AP << /N %d 0 R >> /C [ 1 0 0 ] " % appearance
        + b"/Contents " + literal(b"Boxed for discussion.") + b" "
        b"/T " + literal(b"Timo") + b" /M (D:20260812101700Z) >>"
    )
    # A mark with no words. Listing it is the decision being tested: a reader
    # who highlighted a line without typing anything still made a mark.
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Underline /Rect [ 70 600 320 620 ] "
        b"/T " + literal(b"Timo") + b" /M (D:20260812101800Z) >>"
    )
    # Neither of these is a comment, and both carry text that would look like
    # one to a scan that keys on `/Contents` alone.
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Link /Rect [ 70 560 320 580 ] "
        b"/A << /Type /Action /S /URI /URI (https://example.invalid/leak) >> "
        b"/Contents " + literal(b"Follow me") + b" >>"
    )
    ordinary.annot(
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T " + literal(b"field1") + b" "
        b"/Rect [ 380 400 500 420 ] /V " + literal(b"typed into a form") + b" >>"
    )
    ordinary.finish()
    expected["pages"]["0"] = {
        "comments": 5,
        "authors": ["Timo", "Reviewer"],
        "note_body": "The figure on page 2 contradicts this.",
        "reply_body": "It does not - page 2 is the revised figure.",
        "replies": 1,
        "kinds": ["text", "text", "highlight", "square", "underline"],
        "absent": ["Follow me", "typed into a form", "example.invalid"],
    }

    # ---------------------------------------------------------------- page 1
    strings = Page(pdf, pages, font, "Strings")
    strings.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 100 700 124 724 ] "
        b"/T " + utf16be("Ávila 第三章 \U0001d11e") + b" "
        b"/Contents " + utf16be("Sixteen bits at a time.") + b" "
        b"/M (D:20260812101500+02'00') >>"
    )
    # 0x90 is U+2019 RIGHT SINGLE QUOTATION MARK in PDFDocEncoding and a C1
    # control in Latin-1, so a decoder that treats the default encoding as
    # Latin-1 produces an unprintable byte here while every character after it
    # stays correct --- which is why this is the byte and not one of the
    # accented ones, where the two encodings agree. (0xE1 in the author is such
    # an agreement, and is here as the control: it must survive either way.)
    strings.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 100 660 124 684 ] "
        b"/T " + literal(b"Se\341n") + b" "
        b"/Contents " + literal(b"It\220s fine as it stands.") + b" "
        b"/M (D:20260812101600) >>"
    )
    strings.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 100 620 124 644 ] "
        b"/T " + utf8("Zoë") + b" "
        b"/Contents " + utf8("Eight bits at a time, since PDF 2.0.") + b" "
        b"/M (D:20260812101700Z) >>"
    )
    strings.annot(
        b"<< /Type /Annot /Subtype /FreeText /Rect [ 100 560 400 600 ] "
        b"/Contents " + literal(b"First paragraph.\n\nSecond one,\tafter a tab.\x07") + b" "
        b"/T " + literal(b"Timo") + b" /M (yesterday) >>"
    )
    strings.finish()
    expected["pages"]["1"] = {
        "comments": 4,
        "utf16_author": "Ávila 第三章 \U0001d11e",
        "pdfdoc_author": "Seán",
        "pdfdoc_body": "It’s fine as it stands.",
        "utf8_body": "Eight bits at a time, since PDF 2.0.",
        "paragraph_body": "First paragraph.\n\nSecond one, after a tab.",
        "offset_date": "2026-08-12 10:15",
        "bad_date_is_absent": True,
    }

    # ---------------------------------------------------------------- page 2
    hostile = Page(pdf, pages, font, "Hostile")
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 700 84 724 ] "
        b"/Contents " + literal(b"H" * HUGE_BODY_CHARS) + b" "
        b"/T " + literal(b"Flood") + b" >>"
    )
    loop_a = pdf.reserve()
    loop_b = pdf.reserve()
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 660 84 684 ] "
        b"/Contents " + literal(b"A replies to B.") + b" /IRT %d 0 R >>" % loop_b,
        number=loop_a,
    )
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 620 84 644 ] "
        b"/Contents " + literal(b"B replies to A.") + b" /IRT %d 0 R >>" % loop_a,
        number=loop_b,
    )
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 580 84 604 ] "
        b"/Contents " + literal(b"A reply to something on another page.") + b" "
        b"/IRT %d 0 R >>" % note
    )
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 540 84 564 ] "
        b"/Contents " + literal(b"A reply to the page itself.") + b" "
        b"/IRT %d 0 R >>" % hostile.number
    )
    hostile.annot(b"<< /Type /Annot /Subtype /Popup /Rect [ 200 600 400 700 ] >>")
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 500 700 300 600 ] "
        b"/Contents " + literal(b"My rectangle is written backwards.") + b" >>"
    )
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ -10000000000 -10000000000 "
        b"10000000000 10000000000 ] "
        b"/Contents " + literal(b"My rectangle is the whole plane.") + b" >>"
    )
    # /F bit 2 is Hidden. Reported, and reported *as* hidden: a comment the
    # producer meant not to show is still a comment somebody wrote.
    hostile.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 60 500 84 524 ] /F 2 "
        b"/Contents " + literal(b"You were not meant to see this.") + b" >>"
    )
    hostile.annot(
        b"<< /Type /Annot /Rect [ 60 460 84 484 ] "
        b"/Contents " + literal(b"I have no subtype.") + b" >>"
    )
    # Before the crowd, so the bound below cannot hide them.
    hostile.raw(b"99999 0 R")
    hostile.raw(b"42")
    hostile.raw(literal(b"not a dictionary"))
    for index in range(CROWD):
        hostile.annot(
            b"<< /Type /Annot /Subtype /Text /Rect [ %d %d %d %d ] /Contents %s >>"
            % (
                60 + (index % 40) * 12,
                60 + (index // 40) * 10,
                72 + (index % 40) * 12,
                72 + (index // 40) * 10,
                literal(b"Crowd note %d" % index),
            )
        )
    hostile.finish()
    expected["pages"]["2"] = {
        "body_is_clipped": True,
        "cycle_broken": True,
        "hidden_body": "You were not meant to see this.",
        "crowd": CROWD,
        "per_page_bound_fires": True,
        # The no-subtype annotation, a reference to an object that does not
        # exist, an integer, and a string.
        "unreadable": 4,
    }

    # ---------------------------------------------------------------- page 3
    indirect = Page(pdf, pages, font, "Indirect")
    indirect.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 200 700 224 724 ] "
        b"/Contents " + literal(b"My /Annots array is an indirect object.") + b" "
        b"/T " + literal(b"Timo") + b" >>"
    )
    indirect.finish(indirect=True)
    expected["pages"]["3"] = {
        "comments": 1,
        "body": "My /Annots array is an indirect object.",
    }

    kids = [
        ordinary.number,
        strings.number,
        hostile.number,
        indirect.number,
    ]
    pdf.put(
        pages,
        b"<< /Type /Pages /Kids [ %s ] /Count %d >>"
        % (b" ".join(b"%d 0 R" % n for n in kids), len(kids)),
    )
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)

    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))

    expected["page_count"] = len(kids)
    return expected


def build_rotated(path: str) -> dict:
    """Writes the one-page rotated fixture and what a probe should find in it.

    **A file of its own, and the split is not tidiness.** A `/Rotate 90` page in
    an otherwise upright document makes the document *mixed-size*, and
    `viewer_check.py`'s rotation checks derive their expected zoom from page 1's
    aspect ratio --- so with this page inside `comments.pdf` two of them went red
    against a viewer that was behaving as designed, because the fit had last been
    computed on a page of a different shape. `make_rotated_pdf.py` splits its own
    corpus for the same reason. The rotation this page exists to test is a
    property of the *scan*, which `comments-probe` reads directly and the window
    harness never looks at.
    """
    pdf = Pdf()
    pages = pdf.reserve()
    font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

    turned = Page(pdf, pages, font, "Rotated", rotate=90)
    # Near the bottom-left corner in the page's own space. Under /Rotate 90 the
    # displayed page is HEIGHT wide and WIDTH tall and this lands at the top
    # left --- so a scan that forgets the rotation puts it at the bottom.
    # Deliberately **not square**: a 24-by-24 rectangle at (20, 20) maps to
    # itself under a quarter turn, so it cannot tell a correct rotation from an
    # identity, which is what the first draft of this fixture used.
    turned.annot(
        b"<< /Type /Annot /Subtype /Text /Rect [ 20 30 44 90 ] "
        b"/Contents " + literal(b"I am at the bottom left of an unrotated page.") + b" "
        b"/T " + literal(b"Timo") + b" >>"
    )
    turned.finish()
    pdf.put(
        pages,
        b"<< /Type /Pages /Kids [ %d 0 R ] /Count 1 >>" % turned.number,
    )
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))

    return {
        "page_count": 1,
        "pages": {
            "0": {
                "comments": 1,
                # Display space, y down from the top of the displayed page. A
                # quarter turn clockwise sends the page's y to the display's x.
                "rect": [30.0, 20.0, 90.0, 44.0],
                "displayed_size": [HEIGHT, WIDTH],
            }
        },
    }


def main() -> int:
    """Writes both fixtures and the manifest they share."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    # Keyed by file name, as `outline-manifest.json` is: one sidecar describing
    # two fixtures, so a probe reads the section for the file it was handed
    # rather than being told which manifest goes with which document.
    expected = {}
    for name, builder in (("comments.pdf", build), ("comments-rotated.pdf", build_rotated)):
        path = os.path.join(args.outdir, name)
        expected[name] = builder(path)
        print(f"[OK] {path} ({os.path.getsize(path):,} bytes)")

    manifest = os.path.join(args.outdir, "comments-corpus.json")
    with open(manifest, "w", encoding="utf-8") as handle:
        json.dump(expected, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
    print(f"[OK] {manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
