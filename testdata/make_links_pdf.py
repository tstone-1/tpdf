#!/usr/bin/env python3
"""Generates the link corpus for Phase 1.

Nothing in the existing corpus has a link annotation except one hostile entry in
`comments.pdf`, which exists to prove the comment scan *ignores* links. So a link
reader written against the corpus would be exercised by almost nothing --- and
the measurement that motivated writing one says links are in 16 of the 39 PDFs in
a real Downloads folder, one of them 7,694 of them.

  links.pdf          Eight uniform A4 pages carrying every shape a link comes
                     in, and an outline whose entries point at *the same*
                     destinations. That last part is the point of the fixture
                     rather than a convenience: `links.rs` resolves destinations
                     through `lopdf` and `outline.rs` resolves them through
                     PDFium, and two resolvers for one job is the drift trap this
                     repository has an entry about. With both reading one
                     document, `links-probe` can put the answers side by side and
                     a disagreement is a finding.

  links-rotated.pdf  One page carrying `/Rotate 90` and one that does not, with a
                     link and a destination on each. Split out of the file above
                     for the reason `comments-rotated.pdf` was: a rotated page
                     makes a document mixed-size, and two of `viewer_check.py`'s
                     rotation checks derive what they expect from page 1's aspect
                     ratio. A rotated page inside the main fixture reddens them
                     for a reason that has nothing to do with links.

Writes `links-corpus.json` next to them --- keyed by file name, like
`comments-corpus.json`, and named so that `viewer_check.py`'s
`<fixture>-manifest.json` rule does not enrol it in a check it never claimed.

The output is gitignored. Usage:
    python3 testdata/make_links_pdf.py [outdir]
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

# Body rows per page, and their leading. Both taken from `make_comments_pdf.py`
# after that fixture's own text broke four checks that had nothing to do with
# comments: `viewer_check.py` drags at fixed viewport coordinates and
# double-clicks at a fixed point, so a page with few short lines fails checks
# about selection. A fixture must satisfy the checks that run on *every*
# document, not only the ones it was built for.
ROWS = 36
LEADING = 18

WORDS = (
    "cinder harbor lantern meadow pillar ransom silver tunnel velvet wander "
    "yonder zephyr basalt copper dahlia ember fathom gasket hollow indigo"
).split()

# Links crowded onto one page. Under `MAX_PER_PAGE` (4,000) on purpose: this
# fixture is about shapes, and a page over the bound would make every count in
# the manifest a statement about the bound instead.
CROWD = 40


def body_line(row: int, page_index: int) -> str:
    """One body line, unique within its page at every offset."""
    words = [WORDS[(row + step) % len(WORDS)] for step in range(9)]
    return "Line %02d of page %d: %s" % (row + 1, page_index + 1, " ".join(words))


def page_content(index: int) -> bytes:
    """A page of readable text, with a heading and a marked middle."""
    lines = [
        "BT /F1 28 Tf 60 %d Td (Page %d) Tj ET\n" % (HEIGHT - 90, index + 1),
        "q 0.85 0.90 0.98 rg 40 %d 515 40 re f Q\n" % (HEIGHT - 120),
    ]
    for row in range(ROWS):
        lines.append(
            "BT /F1 11 Tf 60 %d Td (%s) Tj ET\n"
            % (
                HEIGHT - 170 - row * LEADING,
                escape(body_line(row, index)).decode("latin-1"),
            )
        )
    return "".join(lines).encode("latin-1")


def build_pages(pdf: Pdf, count: int, rotate: "list[int] | None" = None):
    """Adds `count` pages and returns the page tree object and the page objects.

    Each page's `/Annots` is left reserved, because a link's destination names a
    page and a page names its links --- neither can be written first.
    """
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
        b" /Encoding /WinAnsiEncoding >>"
    )
    pages = pdf.reserve()
    numbers = []
    annots = []
    for index in range(count):
        stream = pdf.stream(b"<< >>", page_content(index))
        annot_array = pdf.reserve()
        annots.append(annot_array)
        turn = (rotate or [0] * count)[index]
        numbers.append(
            pdf.add(
                b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] /Rotate %d"
                b" /Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R"
                b" /Annots %d 0 R >>"
                % (pages, WIDTH, HEIGHT, turn, font, stream, annot_array)
            )
        )
    kids = b" ".join(b"%d 0 R" % number for number in numbers)
    pdf.put(pages, b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, count))
    return pages, numbers, annots


def link(rect, body: bytes) -> bytes:
    """A `/Link` annotation dictionary with `body` spliced in.

    `/F 4` is Print, which every real link sets and which is deliberately *not*
    the Hidden bit --- so a scan that read `/F` as a boolean rather than testing
    bit 2 would drop every link in this fixture, loudly.
    """
    return (
        b"<< /Type /Annot /Subtype /Link /F 4 /Border [0 0 0]"
        b" /Rect [%d %d %d %d] " % tuple(rect)
    ) + body + b" >>"


def build_links(path: str) -> dict:
    """Eight pages, every link shape, and an outline aimed at the same places."""
    pdf = Pdf()
    pages, page_objects, annot_arrays = build_pages(pdf, 8)

    # Where each interesting link sits. The y ranges follow the body rows so a
    # link lands on real text rather than on blank paper --- a check that clicks
    # one has to find something there.
    def row_rect(row: int, left: int = 60, width: int = 300):
        top = HEIGHT - 170 - row * LEADING + 11
        return [left, top - 13, left + width, top]

    expected = []
    per_page: "dict[int, list[bytes]]" = {index: [] for index in range(8)}

    def add(page: int, rect, body: bytes, target: dict, note: str) -> None:
        per_page[page].append(link(rect, body))
        expected.append(
            {"page": page, "rect": rect, "target": target, "note": note}
        )

    # --- page 0: the ordinary shapes ------------------------------------
    # A `/GoTo` action with an /XYZ destination naming a y coordinate.
    add(
        0,
        row_rect(2),
        b"/A << /S /GoTo /D [%d 0 R /XYZ 60 %d 0] >>"
        % (page_objects[3], HEIGHT - 300),
        {"kind": "page", "page": 3, "top_pt": 300},
        "GoTo action, XYZ with a y",
    )
    # A bare `/Dest` array rather than an action --- the older spelling, and
    # still what many producers write.
    add(
        0,
        row_rect(4),
        b"/Dest [%d 0 R /Fit]" % page_objects[5],
        {"kind": "page", "page": 5, "top_pt": None},
        "Dest array, /Fit names no coordinate",
    )
    # `/FitH` carries its top one element earlier than `/XYZ` does.
    add(
        0,
        row_rect(6),
        b"/Dest [%d 0 R /FitH %d]" % (page_objects[6], HEIGHT - 250),
        {"kind": "page", "page": 6, "top_pt": 250},
        "FitH, top one element earlier than XYZ",
    )
    # `/XYZ null null null` --- keep the reader where they are vertically.
    add(
        0,
        row_rect(8),
        b"/A << /S /GoTo /D [%d 0 R /XYZ null null null] >>" % page_objects[2],
        {"kind": "page", "page": 2, "top_pt": None},
        "XYZ with nulls names no coordinate",
    )

    # --- page 1: named destinations, both mechanisms ----------------------
    add(
        1,
        row_rect(2),
        b"/Dest /flat-chapter",
        {"kind": "page", "page": 4, "top_pt": 200},
        "named through the PDF 1.1 /Dests dictionary",
    )
    add(
        1,
        row_rect(4),
        b"/Dest (tree-chapter)",
        {"kind": "page", "page": 7, "top_pt": 150},
        "named through the /Names name tree",
    )
    add(
        1,
        row_rect(6),
        b"/Dest (deep-chapter)",
        {"kind": "page", "page": 6, "top_pt": None},
        "named through a two-level name tree",
    )
    add(
        1,
        row_rect(8),
        b"/Dest (no-such-name)",
        {"kind": "broken"},
        "a name the tree does not define",
    )

    # --- page 2: everything tpdf declines to follow -----------------------
    add(
        2,
        row_rect(2),
        b"/A << /S /URI /URI (https://example.invalid/tracker?doc=42) >>",
        {"kind": "refused", "action": "uri"},
        "a web link",
    )
    add(
        2,
        row_rect(4),
        b"/A << /S /Launch /F (calc.exe) >>",
        {"kind": "refused", "action": "launch"},
        "launches a program",
    )
    add(
        2,
        row_rect(6),
        b"/A << /S /GoToR /F (other.pdf) /D [2 /Fit] >>",
        {"kind": "refused", "action": "remote"},
        "another file, with a destination that would resolve here",
    )
    add(
        2,
        row_rect(8),
        b"/A << /S /JavaScript /JS (app.alert\\(1\\)) >>",
        {"kind": "refused", "action": "unsupported"},
        "an action tpdf has never heard of",
    )
    add(
        2,
        row_rect(10),
        b"/A << /S /GoToE /D [1 /Fit] >>",
        {"kind": "refused", "action": "embedded"},
        "a destination inside an embedded file",
    )

    # --- page 3: malformed, and the precedence rule -----------------------
    # `/Dest` and `/A` together is malformed per 12.3.3. They name *different*
    # pages so the manifest can say which one was taken; with both at one page
    # the assertion could not fail.
    add(
        3,
        row_rect(2),
        b"/A << /S /GoTo /D [%d 0 R /Fit] >> /Dest [%d 0 R /Fit]"
        % (page_objects[7], page_objects[1]),
        {"kind": "page", "page": 7, "top_pt": None},
        "action and Dest together: the action wins",
    )
    add(
        3,
        row_rect(4),
        b"/A << /S /GoTo /D [%d 0 R /XYZ 60 %d 0] >>" % (pages, HEIGHT - 100),
        {"kind": "broken"},
        "a destination naming the page tree rather than a page",
    )
    add(
        3,
        row_rect(6),
        b"/A << /S /GoTo >>",
        {"kind": "broken"},
        "a GoTo action carrying no destination",
    )
    add(3, row_rect(8), b"", {"kind": "none"}, "a rectangle with no action at all")
    # A rectangle written with its corners the other way round, which 12.5.2
    # requires a consumer to normalise.
    top = HEIGHT - 170 - 10 * LEADING + 11
    add(
        3,
        [360, top, 60, top - 13],
        b"/Dest [%d 0 R /Fit]" % page_objects[2],
        {"kind": "page", "page": 2, "top_pt": None},
        "corners written backwards",
    )

    # --- page 4: things that must not be listed ---------------------------
    # A hidden link: /F bit 2. There is no panel for a link, so a hidden one is
    # simply not there --- unlike a hidden comment, which the panel still lists.
    per_page[4].append(
        b"<< /Type /Annot /Subtype /Link /F 2 /Rect [60 700 360 720]"
        b" /Dest [%d 0 R /Fit] >>" % page_objects[1]
    )
    # A zero-area rectangle: unclickable, so listing it puts a target in the
    # list that no reader can reach and every hit test walks past.
    per_page[4].append(
        b"<< /Type /Annot /Subtype /Link /F 4 /Rect [60 600 60 620]"
        b" /Dest [%d 0 R /Fit] >>" % page_objects[1]
    )
    # A comment sharing the array, which is ordinary and is not a defect.
    per_page[4].append(
        b"<< /Type /Annot /Subtype /Text /Rect [400 700 424 724]"
        b" /T (Timo) /Contents (A note beside the links.) >>"
    )
    # Three entries nothing can read. Written *before* any crowd, because a
    # per-page bound would otherwise cut them and the manifest's count would
    # silently be about the bound: the same defect `comments.pdf` had.
    unreadable_page = 4

    # --- page 5: a crowd, all of them real --------------------------------
    for row in range(CROWD):
        top = HEIGHT - 170 - (row % ROWS) * LEADING + 11
        left = 60 + (row // ROWS) * 240
        per_page[5].append(
            link(
                [left, top - 13, left + 220, top],
                b"/Dest [%d 0 R /Fit]" % page_objects[(row % 7) + 1],
            )
        )

    # --- pages 6 and 7: destinations, and one link each --------------------
    add(
        6,
        row_rect(2),
        b"/A << /S /GoTo /D [%d 0 R /XYZ 60 %d 0] >>" % (page_objects[0], HEIGHT - 200),
        {"kind": "page", "page": 0, "top_pt": 200},
        "back to the first page",
    )
    add(
        7,
        row_rect(2),
        b"/A << /S /GoTo /D [%d 0 R /FitR 40 %d 555 %d] >>"
        % (page_objects[0], HEIGHT - 400, HEIGHT - 350),
        {"kind": "page", "page": 0, "top_pt": 350},
        "FitR takes its top from the fifth element",
    )

    # Named destinations. The flat dictionary is PDF 1.1 and the name tree is
    # 1.2; both are still written by real producers, and a reader that knows one
    # silently drops every link in the half of the corpus using the other.
    flat = pdf.add(
        b"<< /flat-chapter [%d 0 R /XYZ 60 %d 0] >>"
        % (page_objects[4], HEIGHT - 200)
    )
    leaf = pdf.add(
        b"<< /Limits [(deep-chapter) (deep-chapter)]"
        b" /Names [(deep-chapter) [%d 0 R /Fit]] >>" % page_objects[6]
    )
    tree = pdf.add(
        b"<< /Kids [%d 0 R] /Names [(tree-chapter) [%d 0 R /XYZ 60 %d 0]] >>"
        % (leaf, page_objects[7], HEIGHT - 150)
    )
    names = pdf.add(b"<< /Dests %d 0 R >>" % tree)

    # Now the annotation arrays, once every destination object exists.
    for index in range(8):
        entries = [pdf.add(body) for body in per_page[index]]
        refs = b" ".join(b"%d 0 R" % number for number in entries)
        if index == unreadable_page:
            # A dangling reference, a bare integer and a string. Each is an
            # `/Annots` entry nothing can read, and the scan must count all
            # three rather than stopping at the first.
            refs = b"9999 0 R 42 (not a dictionary) " + refs
        pdf.put(annot_arrays[index], b"[%s]" % refs)

    # The outline, aimed at the same destinations the links use. This is the
    # fixture's reason for existing: `outline.rs` resolves through PDFium and
    # `links.rs` through `lopdf`, and only a document both read can say whether
    # they agree.
    shared = [
        ("Chapter one", b"/Dest [%d 0 R /XYZ 60 %d 0]" % (page_objects[3], HEIGHT - 300)),
        ("Chapter two", b"/Dest [%d 0 R /Fit]" % page_objects[5]),
        ("Chapter three", b"/Dest [%d 0 R /FitH %d]" % (page_objects[6], HEIGHT - 250)),
        ("Named, flat", b"/Dest /flat-chapter"),
        ("Named, tree", b"/Dest (tree-chapter)"),
        ("A web link", b"/A << /S /URI /URI (https://example.invalid/) >>"),
    ]
    outline_root = pdf.reserve()
    entries = [pdf.reserve() for _ in shared]
    for index, ((title, body), number) in enumerate(zip(shared, entries)):
        links_to = b"<< /Title (%s) /Parent %d 0 R %s" % (
            title.encode("latin-1"),
            outline_root,
            body,
        )
        if index > 0:
            links_to += b" /Prev %d 0 R" % entries[index - 1]
        if index + 1 < len(entries):
            links_to += b" /Next %d 0 R" % entries[index + 1]
        pdf.put(number, links_to + b" >>")
    pdf.put(
        outline_root,
        b"<< /Type /Outlines /First %d 0 R /Last %d 0 R /Count %d >>"
        % (entries[0], entries[-1], len(entries)),
    )

    catalog = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /Dests %d 0 R /Names %d 0 R"
        b" /Outlines %d 0 R >>" % (pages, flat, names, outline_root)
    )
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(catalog))

    return {
        "pages": 8,
        # Every link the scan must list, in document order: pages 0-3 as
        # declared, then the crowd on page 5, then pages 6 and 7. Page 4
        # contributes none --- one hidden, one flat, one comment.
        "expected": expected,
        "crowd_page": 5,
        "crowd": CROWD,
        "total": len(expected) + CROWD,
        "limits": {
            "crowded_pages": 0,
            "over_budget": False,
            "unreadable": 3,
            "unresolved_names": 0,
        },
        # What the outline points at, so the probe can compare the two
        # resolvers. Only the entries whose destinations the links also use.
        "shared_targets": [
            {"title": "Chapter one", "target": {"kind": "page", "page": 3, "top_pt": 300}},
            {"title": "Chapter two", "target": {"kind": "page", "page": 5, "top_pt": None}},
            {"title": "Chapter three", "target": {"kind": "page", "page": 6, "top_pt": 250}},
            {"title": "Named, flat", "target": {"kind": "page", "page": 4, "top_pt": 200}},
            {"title": "Named, tree", "target": {"kind": "page", "page": 7, "top_pt": 150}},
            {"title": "A web link", "target": {"kind": "refused", "action": "uri"}},
        ],
    }


def build_rotated(path: str) -> dict:
    """Two pages, the first turned a quarter, each with a link and a target."""
    pdf = Pdf()
    pages, page_objects, annot_arrays = build_pages(pdf, 2, rotate=[90, 0])

    # On the turned page the destination's vertical axis is the display's
    # horizontal one, so there is no offset to scroll to and the scan must say
    # so rather than placing it somewhere plausible.
    first = pdf.add(
        link(
            [60, 600, 360, 640],
            b"/A << /S /GoTo /D [%d 0 R /XYZ 60 %d 0] >>"
            % (page_objects[1], HEIGHT - 300),
        )
    )
    # And a link *to* the turned page, whose own offset is equally unplaceable.
    second = pdf.add(
        link(
            [60, 600, 360, 640],
            b"/A << /S /GoTo /D [%d 0 R /XYZ 60 %d 0] >>"
            % (page_objects[0], HEIGHT - 300),
        )
    )
    pdf.put(annot_arrays[0], b"[%d 0 R]" % first)
    pdf.put(annot_arrays[1], b"[%d 0 R]" % second)

    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(catalog))

    return {
        "pages": 2,
        "expected": [
            {
                "page": 0,
                # A quarter turn swaps the displayed size, so the rectangle is
                # measured against 842 x 595 rather than 595 x 842.
                "target": {"kind": "page", "page": 1, "top_pt": 300},
                "note": "from a rotated page to an upright one",
            },
            {
                "page": 1,
                "target": {"kind": "page", "page": 0, "top_pt": None},
                "note": "to a rotated page: no vertical axis to name",
            },
        ],
        "limits": {
            "crowded_pages": 0,
            "over_budget": False,
            "unreadable": 0,
            "unresolved_names": 0,
        },
    }


def main() -> int:
    """Writes both fixtures and the manifest that describes them."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "outdir",
        nargs="?",
        default=os.path.dirname(os.path.abspath(__file__)),
        help="where to write the fixtures",
    )
    args = parser.parse_args()
    os.makedirs(args.outdir, exist_ok=True)

    manifest = {}
    main_path = os.path.join(args.outdir, "links.pdf")
    manifest["links.pdf"] = build_links(main_path)
    print("[OK] wrote %s" % main_path)

    rotated_path = os.path.join(args.outdir, "links-rotated.pdf")
    manifest["links-rotated.pdf"] = build_rotated(rotated_path)
    print("[OK] wrote %s" % rotated_path)

    # Keyed by file name, and named `-corpus` rather than `-manifest`: the
    # viewer harness binds any `<fixture>-manifest.json` to its reading-order
    # check, and a differently shaped file there killed a whole run once.
    manifest_path = os.path.join(args.outdir, "links-corpus.json")
    with open(manifest_path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2)
        handle.write("\n")
    print("[OK] wrote %s" % manifest_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
