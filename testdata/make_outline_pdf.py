#!/usr/bin/env python3
"""Generates the outline (bookmark) corpus for Phase 1.

Nothing in the existing corpus has an outline at all, so a walker written
against it would be exercised by nothing. These two fixtures exist to give it
something to walk and something to survive.

  outline-simple    Twelve pages and a three-level outline: /XYZ destinations
                    with a y coordinate, a /Fit destination without one, an
                    entry whose destination arrives through a /A GoTo action
                    rather than /Dest, an entry with no destination at all, a
                    subtree marked closed, and titles in Latin, CJK and astral
                    Unicode. The ordinary case, deliberately not uniform --- a
                    fixture where every entry is shaped the same way cannot tell
                    a walker that handles one shape from one that handles all of
                    them.

  outline-hostile   PDFium's own documentation for FPDFBookmark_GetNextSibling
                    says "the caller is responsible for handling circular
                    bookmark references, as may arise from malformed
                    documents", which makes an infinite outline an input we are
                    told to expect rather than one we are speculating about.
                    This fixture is that input: a sibling chain that loops, a
                    child that points back at its own ancestor, a 200-level
                    chain, destinations at a page that does not exist, actions
                    tpdf must refuse to follow (/Launch, /URI, /GoToR), and
                    titles built to break a decoder --- 50,000 characters,
                    embedded control characters, and an unpaired UTF-16
                    surrogate.

Writes `outline-manifest.json` next to them, so the probe reads its
expectations rather than hardcoding them.

One expectation there is deliberately weaker than the others. The unpaired
surrogate is marked `observed`, not `required`: PDFium may well repair or drop
it while parsing the document string, in which case the fixture proves nothing
about our decoder and pretending otherwise would be a test that cannot fail.
The decoder is pinned by a unit test in `outline.rs` that hands it the bytes
directly, where the input is ours to control.

The output is gitignored. Usage:
    python3 testdata/make_outline_pdf.py [outdir]
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

# How many characters the oversized title carries. Any cap worth having is far
# below this, and it stays small enough that the fixture writes in a blink.
HUGE_TITLE_CHARS = 50_000

# Levels in the deep chain. Chosen above any plausible depth bound so that
# "the bound fired" and "the document was simply shallow" cannot be confused.
DEEP_LEVELS = 200


def text_string(value: str) -> bytes:
    """Encodes a PDF text string in the narrowest encoding that holds it.

    ASCII goes out as a literal and anything else as UTF-16BE with a byte-order
    mark, which is what real producers do --- so both decoding paths through
    PDFium are exercised rather than only the one a uniform fixture would reach.
    """
    raw = value.encode("latin-1", "ignore")
    if raw.decode("latin-1") == value and all(0x20 <= b < 0x7F for b in raw):
        for char in (b"\\", b"(", b")"):
            raw = raw.replace(char, b"\\" + char)
        return b"(" + raw + b")"
    return hex_string(b"\xfe\xff" + value.encode("utf-16-be"))


def hex_string(raw: bytes) -> bytes:
    """Encodes arbitrary bytes as a PDF hex string.

    Needed because the surrogate fixture is not a Python `str` --- it is a
    UTF-16 sequence Python would refuse to encode, which is the whole point of
    it.
    """
    return b"<" + raw.hex().upper().encode("ascii") + b">"


class Node:
    """One outline entry, before object numbers exist."""

    def __init__(
        self,
        title,
        *,
        page: "int | None" = None,
        top: "int | None" = None,
        action: "bytes | None" = None,
        dest: "bytes | None" = None,
        children: "tuple[Node, ...]" = (),
        is_open: bool = True,
    ) -> None:
        """Records an entry; `title` may be `str` or pre-encoded `bytes`."""
        self.title = title
        self.page = page
        self.top = top
        self.action = action
        self.dest = dest
        self.children = list(children)
        self.is_open = is_open
        self.number = 0

    def visible(self) -> int:
        """Descendants a viewer would show, per PDF 32000-1 table 153."""
        if not self.is_open:
            return 0
        return sum(1 + child.visible() for child in self.children)


# Rotated to build every body line, so no two lines share a substring at any
# offset. That is a requirement, not a flourish: the selection check locates a
# drag with `indexOf` on the page's text, and a page whose lines are all "the
# quick brown fox..." resolves both of its drags to the same index and reports
# that the page reads bottom to top.
WORDS = (
    "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima "
    "mike november oscar papa quebec romeo sierra tango uniform victor whiskey "
    "xray"
).split()


def body_line(row: int, page_index: int) -> str:
    """One body line, unique within its page at every offset."""
    words = [WORDS[(row + step) % len(WORDS)] for step in range(8)]
    return "Line %02d of page %d: %s" % (row + 1, page_index + 1, " ".join(words))


def build_pages(pdf: Pdf, count: int) -> "tuple[int, list[int]]":
    """Adds `count` numbered pages and returns the page tree and page objects."""
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica"
        b" /Encoding /WinAnsiEncoding >>"
    )
    pages = pdf.reserve()
    numbers = []
    for index in range(count):
        # Marks near the top and near the middle, so a destination's y
        # coordinate lands somewhere identifiable rather than on a blank sheet.
        #
        # The body lines in between are not decoration. Every check in
        # `viewercheck.ts` runs on whatever document it is given, and the
        # selection ones drag at fixed viewport coordinates --- on a page with
        # two lines of text those drags land on blank paper and the check fails
        # for a reason that has nothing to do with selection. A fixture has to
        # exercise the checks that run on *every* document, not only the ones it
        # was built for.
        lines = [
            "BT /F1 28 Tf 60 %d Td (Page %d) Tj ET\n" % (HEIGHT - 90, index + 1),
            "q 0.85 0.90 0.98 rg 40 %d 515 40 re f Q\n" % (HEIGHT - 120),
        ]
        for row in range(24):
            lines.append(
                "BT /F1 12 Tf 60 %d Td (%s) Tj ET\n"
                % (
                    HEIGHT - 170 - row * 22,
                    escape(body_line(row, index)).decode("latin-1"),
                )
            )
        lines.append(
            "BT /F1 14 Tf 60 %d Td (%s) Tj ET\n"
            % (
                HEIGHT - 460,
                escape("Middle of page %d" % (index + 1)).decode("latin-1"),
            )
        )
        content = "".join(lines)
        stream = pdf.stream(b"<< >>", content.encode("latin-1"))
        numbers.append(
            pdf.add(
                b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d]"
                b" /Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
                % (pages, WIDTH, HEIGHT, font, stream)
            )
        )
    kids = b" ".join(b"%d 0 R" % number for number in numbers)
    pdf.put(pages, b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, count))
    return pages, numbers


def emit(pdf: Pdf, nodes: "list[Node]", parent: int, page_objects: "list[int]") -> None:
    """Writes a sibling list and everything under it.

    Numbers are reserved for the whole level first, because /Prev and /Next
    reference siblings in both directions and neither can be written before the
    other exists.
    """
    for node in nodes:
        node.number = pdf.reserve()
    for index, node in enumerate(nodes):
        body = b"<< /Title " + (
            node.title if isinstance(node.title, bytes) else text_string(node.title)
        )
        body += b" /Parent %d 0 R" % parent
        if index > 0:
            body += b" /Prev %d 0 R" % nodes[index - 1].number
        if index + 1 < len(nodes):
            body += b" /Next %d 0 R" % nodes[index + 1].number

        if node.dest is not None:
            body += b" /Dest " + node.dest
        elif node.page is not None:
            target = page_objects[node.page]
            top = b"null" if node.top is None else b"%d" % node.top
            body += b" /Dest [%d 0 R /XYZ null %s null]" % (target, top)
        if node.action is not None:
            body += b" /A " + node.action

        if node.children:
            emit(pdf, node.children, node.number, page_objects)
            body += b" /First %d 0 R /Last %d 0 R" % (
                node.children[0].number,
                node.children[-1].number,
            )
            count = node.visible()
            body += b" /Count %d" % (count if node.is_open else -len(node.children))
        pdf.put(node.number, body + b" >>")


def build_outline(pdf: Pdf, roots: "list[Node]", page_objects: "list[int]") -> int:
    """Adds the outline dictionary and everything under it."""
    outlines = pdf.reserve()
    emit(pdf, roots, outlines, page_objects)
    total = sum(1 + node.visible() for node in roots)
    pdf.put(
        outlines,
        b"<< /Type /Outlines /First %d 0 R /Last %d 0 R /Count %d >>"
        % (roots[0].number, roots[-1].number, total),
    )
    return outlines


def build_simple(path: str) -> dict:
    """The ordinary outline: nested, mixed destination shapes, mixed scripts."""
    pdf = Pdf()
    pages, page_objects = build_pages(pdf, 12)

    roots = [
        Node("Introduction", page=0, top=HEIGHT - 100),
        Node(
            "Getting Started",
            page=1,
            top=HEIGHT - 240,
            children=(
                Node("Installation", page=1, top=HEIGHT - 440),
                # /Fit carries no coordinate at all, so a walker that assumes
                # every destination has a y has to cope with one that does not.
                Node("Ünderständing Fonts", dest=b"[%d 0 R /Fit]" % page_objects[2]),
            ),
        ),
        Node(
            "第三章 — Deep Structure",
            page=3,
            top=HEIGHT - 62,
            is_open=False,
            children=(
                Node(
                    "Nested",
                    page=4,
                    top=HEIGHT - 342,
                    children=(Node("Deeper 𝄞", page=5, top=HEIGHT - 542),),
                ),
                # Destination through an action rather than /Dest. PDFium
                # returns NULL from FPDFBookmark_GetDest here and the caller has
                # to go via FPDFBookmark_GetAction, which is easy to miss
                # because the common case never needs it.
                Node(
                    "Via an action",
                    action=b"<< /S /GoTo /D [%d 0 R /XYZ null %d null] >>"
                    % (page_objects[6], HEIGHT - 100),
                ),
            ),
        ),
        Node("No destination"),
        Node("Appendix", page=11, top=HEIGHT - 42),
    ]

    outlines = build_outline(pdf, roots, page_objects)
    root = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /Outlines %d 0 R /PageMode /UseOutlines >>"
        % (pages, outlines)
    )
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))

    return {
        "pages": 12,
        "roots": 5,
        # Every entry the walk should reach, depth-first, with the page it
        # resolves to and whether it carries a y coordinate.
        "entries": [
            {"title": "Introduction", "depth": 0, "page": 0, "has_top": True},
            {"title": "Getting Started", "depth": 0, "page": 1, "has_top": True},
            {"title": "Installation", "depth": 1, "page": 1, "has_top": True},
            {"title": "Ünderständing Fonts", "depth": 1, "page": 2, "has_top": False},
            {"title": "第三章 — Deep Structure", "depth": 0, "page": 3, "has_top": True},
            {"title": "Nested", "depth": 1, "page": 4, "has_top": True},
            {"title": "Deeper 𝄞", "depth": 2, "page": 5, "has_top": True},
            {"title": "Via an action", "depth": 1, "page": 6, "has_top": True},
            {"title": "No destination", "depth": 0, "page": None, "has_top": False},
            {"title": "Appendix", "depth": 0, "page": 11, "has_top": True},
        ],
        "closed": ["第三章 — Deep Structure"],
    }


def build_hostile(path: str) -> dict:
    """The outline that does not terminate, encode, or resolve."""
    pdf = Pdf()
    pages, page_objects = build_pages(pdf, 3)

    # Built as a well-formed tree first and then damaged, because the damage has
    # to name object numbers that do not exist until the tree is written.
    cycle_a = Node("Cycle sibling A", page=0, top=HEIGHT - 100)
    cycle_b = Node("Cycle sibling B", page=1, top=HEIGHT - 100)
    ancestor_child = Node("Ancestor cycle child", page=0)

    deep: Node = Node("Deep %d" % DEEP_LEVELS, page=2)
    for level in range(DEEP_LEVELS - 1, 0, -1):
        deep = Node("Deep %d" % level, page=2, children=(deep,))

    huge = Node("H" * HUGE_TITLE_CHARS, page=0)
    controls = Node("Line\nbreak\tand\rreturn\x00and NUL", page=0)
    # A lone high surrogate followed by "A". Python cannot hold this as a str,
    # so it is written as raw UTF-16BE bytes behind a BOM.
    surrogate = Node(hex_string(b"\xfe\xff\xd8\x00\x00\x41"), page=0)

    roots = [
        Node("Sibling cycle", page=0, children=(cycle_a, cycle_b)),
        Node("Ancestor cycle", page=0, children=(ancestor_child,)),
        deep,
        Node("Launch action", action=b"<< /S /Launch /F (/bin/sh) >>"),
        Node("URI action", action=b"<< /S /URI /URI (https://example.invalid/) >>"),
        Node(
            "Remote goto",
            action=b"<< /S /GoToR /F (other.pdf) /D [0 /Fit] >>",
        ),
        # A page index the document does not have. PDFium reports -1 here, but
        # a destination naming an in-range-looking page it cannot resolve is the
        # case a page_count guard exists for.
        Node("Broken destination", dest=b"[9999 /Fit]"),
        huge,
        controls,
        surrogate,
    ]

    outlines = build_outline(pdf, roots, page_objects)

    # The damage. Each rewrites one already-emitted object.
    #
    # A's /Next points at B and B's /Next points back at A, so the child list of
    # "Sibling cycle" has no end.
    pdf.objects[cycle_b.number - 1] = (
        pdf.objects[cycle_b.number - 1].replace(b" >>", b" /Next %d 0 R >>" % cycle_a.number)
    )
    # The child's /First points at its own grandparent, so descending never
    # gets deeper --- it gets back to where it started.
    pdf.objects[ancestor_child.number - 1] = pdf.objects[
        ancestor_child.number - 1
    ].replace(
        b" >>",
        b" /First %d 0 R /Last %d 0 R /Count 1 >>" % (roots[1].number, roots[1].number),
    )

    root = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /Outlines %d 0 R >>" % (pages, outlines)
    )
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))

    return {
        "pages": 3,
        "roots": 10,
        "deep_levels": DEEP_LEVELS,
        "huge_title_chars": HUGE_TITLE_CHARS,
        # Titles that must appear somewhere in the walk. If the sibling cycle
        # were mishandled by aborting the level rather than stopping at the
        # repeat, everything after it would be missing.
        "required_titles": [
            "Sibling cycle",
            "Cycle sibling A",
            "Cycle sibling B",
            "Ancestor cycle",
            "Deep 1",
            "Launch action",
            "URI action",
            "Remote goto",
            "Broken destination",
        ],
        # Titles whose entry must carry a refusal rather than a destination,
        # with the reason each must give.
        "refused": {
            "Launch action": "launch",
            "URI action": "uri",
            "Remote goto": "remote",
        },
        "broken": ["Broken destination"],
        # Weaker than the rest on purpose --- see the module docstring.
        "observed": ["unpaired surrogate title"],
    }


def main() -> int:
    """Writes both fixtures and the manifest."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    manifest = {
        "outline-simple.pdf": build_simple(
            os.path.join(args.outdir, "outline-simple.pdf")
        ),
        "outline-hostile.pdf": build_hostile(
            os.path.join(args.outdir, "outline-hostile.pdf")
        ),
    }
    path = os.path.join(args.outdir, "outline-manifest.json")
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=2)
        handle.write("\n")

    for name in manifest:
        full = os.path.join(args.outdir, name)
        print("[OK] %s (%d bytes)" % (full, os.path.getsize(full)))
    print("[OK] %s" % path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
