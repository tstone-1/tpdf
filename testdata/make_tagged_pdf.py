#!/usr/bin/env python3
"""Generates a tagged document whose structure order is not its geometry.

`make_columns_pdf.py` is the fixture for recovering reading order *from
geometry*, which is what an untagged document forces. This is the other half: a
document that carries `/StructTreeRoot`, so it says what its own reading order
is, and where believing the geometry gives the wrong answer.

The discriminating property is the whole point, and it is easy to get wrong. A
tagged page whose tag order happens to match what geometry would infer tests
nothing at all --- both implementations agree, and a check on it passes whether
or not the tags were read. So page 1 puts a **margin note** in the left margin
beside the first paragraph:

  * **Geometry** reads it first. It is the leftmost thing on the page and its
    first line is above the body's second paragraph, and every rule that orders
    blocks by position puts it at the front.
  * **The tags** read it last, after both body paragraphs, which is what the
    producer meant and what a screen reader announces.

That is not a contrived arrangement. Margin notes, pull quotes and figure
captions are tagged out of visual flow constantly, and it is exactly the case
tagging exists to disambiguate.

Page 2 is the control, and it matters as much: the same layout with no note, and
tagged in the order geometry would have inferred anyway. A tagged reader must
leave it alone. Without that page, "the tags are being read" and "the tags are
being read *and* scrambling everything" look identical.

Element types are carried too --- `/H1` for the heading, `/H2` for a subheading,
`/P` for the body, `/Note` for the margin note --- because reading order is not the
only thing a structure tree answers, and a consumer that flattens every element to
"text" has thrown away the half that makes a heading announce as a heading.

**Two heading levels, deliberately.** A page with one heading cannot distinguish a
consumer that uses the document's level from one that announces every heading as
`h1`; the mutation that does exactly that survived against the first version of
this fixture, and the check passed --- correctly, and uselessly. Whatever a fixture
is meant to discriminate, it needs at least two of.

`tagged-manifest.json` states the expected reading order and the expected type
per block, so a probe reads a file this script wrote rather than carrying a copy
of these strings. It states the *geometric* order too, which is what makes a
check able to say "the tagged answer was used" rather than merely "an answer was
produced".

Base-14 Helvetica, so this needs no font file and nothing it writes is anyone
else's to redistribute. The output is gitignored.

Usage: python3 make_tagged_pdf.py <outdir>
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

#: Left edge of the body column and of the margin note, in points.
BODY_X = 170
NOTE_X = 45
#: Leading between lines of a block.
LEADING = 16
#: Point size everything is set at.
SIZE = 12


class Block:
    """One tagged run of text: its lines, where they sit, and what it is."""

    def __init__(self, tag: str, name: str, x: float, top: float, lines: "list[str]"):
        """Records a block without emitting anything yet.

        `top` is the baseline of the first line, in PDF points from the bottom
        of the page, which is the coordinate system the content stream uses.
        """
        self.tag = tag
        self.name = name
        self.x = x
        self.top = top
        self.lines = lines

    def content(self, mcid: int) -> str:
        """The content-stream fragment for this block, wrapped in its MCID.

        The marked-content operators go *outside* `BT`/`ET`. Both nestings are
        legal and this one is what every producer emits, so it is what a
        consumer will meet.
        """
        out = f"/{self.tag} << /MCID {mcid} >> BDC\nBT\n/F1 {SIZE} Tf\n"
        y = self.top
        for line in self.lines:
            out += f"1 0 0 1 {self.x:.1f} {y:.1f} Tm ({escape(line).decode('latin-1')}) Tj\n"
            y -= LEADING
        return out + "ET\nEMC\n"

    def text(self) -> str:
        """What the block says, as a reader would read it."""
        return " ".join(self.lines)


def page_one() -> "list[Block]":
    """Heading, two body paragraphs, and a margin note tagged after them."""
    return [
        Block("H1", "heading", BODY_X, HEIGHT - 80, ["Quarterly review"]),
        Block(
            "P",
            "body-one",
            BODY_X,
            HEIGHT - 120,
            [
                "The first body paragraph runs down the main",
                "column and is the first thing the document",
                "means to be read after the heading.",
            ],
        ),
        # A *second* heading, at a different level, and it is not decoration: a
        # page with one heading cannot tell "the level the document stated" from
        # "every heading is an h1". The mutation that flattens every level to h1
        # survived against a fixture whose only heading was `H1` --- the check
        # passed, correctly and uselessly.
        Block("H2", "subheading", BODY_X, HEIGHT - 192, ["Second half"]),
        Block(
            "P",
            "body-two",
            BODY_X,
            HEIGHT - 220,
            [
                "The second body paragraph follows it, still",
                "in the main column, and closes the section.",
            ],
        ),
        # Beside the first paragraph, so its first line is *above* the second
        # paragraph's. Anything ordering by position reads it before that
        # paragraph, and every rule that sweeps left to right reads it before
        # the first one as well.
        Block(
            "Note",
            "margin-note",
            NOTE_X,
            HEIGHT - 130,
            ["Marginal", "aside kept", "out of the", "main flow."],
        ),
    ]


def page_two() -> "list[Block]":
    """The control: tagged in the order geometry would have inferred."""
    return [
        Block("H1", "control-heading", BODY_X, HEIGHT - 80, ["Appendix"]),
        Block(
            "P",
            "control-one",
            BODY_X,
            HEIGHT - 120,
            ["This page is tagged in the order it is laid out,", "top to bottom."],
        ),
        Block(
            "P",
            "control-two",
            BODY_X,
            HEIGHT - 180,
            ["So a reader that believes the tags and one that", "believes the geometry agree here."],
        ),
    ]


def geometric_order(blocks: "list[Block]") -> "list[str]":
    """The order a position-based rule would produce: down the page, then across.

    Written here rather than inferred, because the manifest's job is to state
    what the *other* answer is. A check comparing against a geometric order it
    computed itself would be comparing an implementation with itself.
    """
    return [b.name for b in sorted(blocks, key=lambda b: (-b.top, b.x))]


def build(path: str) -> None:
    """Writes the tagged PDF and its manifest."""
    pdf = Pdf()
    pages_ref = pdf.reserve()
    struct_root = pdf.reserve()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )

    page_refs = [pdf.reserve() for _ in range(2)]
    pages = [page_one(), page_two()]
    elements: "list[list[int]]" = []
    manifest = {"pages": []}

    for index, (page_ref, blocks) in enumerate(zip(page_refs, pages)):
        body = "".join(block.content(mcid) for mcid, block in enumerate(blocks))
        stream = pdf.stream(b"<< >>", body.encode("latin-1"))
        pdf.put(
            page_ref,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R "
            b"/StructParents %d >>"
            % (pages_ref, WIDTH, HEIGHT, font, stream, index),
        )
        # One struct element per block, in the order the document means them to
        # be read --- which for page 1 is *not* the order they were emitted in
        # either, since the note is drawn last and read last but the heading is
        # drawn first and read first. Emission order carries no meaning at all;
        # that is what the tree is for.
        refs = []
        for mcid, block in enumerate(blocks):
            refs.append(
                pdf.add(
                    b"<< /Type /StructElem /S /%s /P %d 0 R /Pg %d 0 R /K %d >>"
                    % (block.tag.encode("ascii"), struct_root, page_ref, mcid)
                )
            )
        elements.append(refs)
        manifest["pages"].append(
            {
                "tagged": [b.name for b in blocks],
                "geometric": geometric_order(blocks),
                "types": {b.name: b.tag for b in blocks},
                "text": {b.name: b.text() for b in blocks},
                # The three fields `viewer_check.py`'s reading-order check reads,
                # in the shape `make_columns_pdf.py` established. Emitted here so
                # this file is an ordinary corpus for that harness rather than one
                # it has to know about: the check then asserts the *lines*, in
                # tagged order, against expectations this program wrote --- which
                # is the same external-oracle mechanism, applied to tags.
                #
                # Lines rather than blocks, because that is what a reader is
                # handed: a tagged run is a paragraph and it comes back as its
                # lines, ordered by the geometry inside the run.
                "page": index,
                "name": "tagged" if index == 0 else "control",
                "lines": [line for block in blocks for line in block.lines],
            }
        )

    pdf.put(
        pages_ref,
        b"<< /Type /Pages /Count %d /Kids [%s] >>"
        % (len(page_refs), b" ".join(b"%d 0 R" % r for r in page_refs)),
    )
    # The number tree maps a page's /StructParents to its elements by MCID, and
    # is how a consumer goes from a mark in the content stream back to the tree.
    # PDFium can walk the tree without it; a real tagged document has it, and a
    # fixture that omits it would be testing a shape no producer emits.
    nums = b" ".join(
        b"%d [%s]" % (index, b" ".join(b"%d 0 R" % r for r in refs))
        for index, refs in enumerate(elements)
    )
    parent_tree = pdf.add(b"<< /Nums [%s] >>" % nums)
    pdf.put(
        struct_root,
        b"<< /Type /StructTreeRoot /K [%s] /ParentTree %d 0 R /ParentTreeNextKey %d >>"
        % (
            b" ".join(b"%d 0 R" % r for refs in elements for r in refs),
            parent_tree,
            len(elements),
        ),
    )
    catalog = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /StructTreeRoot %d 0 R "
        b"/MarkInfo << /Marked true >> /Lang (en-GB) >>" % (pages_ref, struct_root)
    )

    with open(path, "wb") as handle:
        handle.write(pdf.serialize(catalog))

    stem = os.path.splitext(path)[0]
    with open(f"{stem}-manifest.json", "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2)
        handle.write("\n")


def main() -> int:
    """Writes `tagged.pdf` and its manifest into the given directory."""
    if len(sys.argv) != 2:
        print("usage: make_tagged_pdf.py <outdir>", file=sys.stderr)
        return 2
    outdir = sys.argv[1]
    os.makedirs(outdir, exist_ok=True)
    path = os.path.join(outdir, "tagged.pdf")
    build(path)
    print(f"[OK] wrote {path}")

    # A fixture that does not discriminate is the failure this file exists to
    # avoid, so it is asserted here rather than left to be noticed later.
    with open(f"{os.path.splitext(path)[0]}-manifest.json", encoding="utf-8") as handle:
        manifest = json.load(handle)
    first, second = manifest["pages"]
    if first["tagged"] == first["geometric"]:
        print("[FAIL] page 1's tagged order equals its geometric order", file=sys.stderr)
        return 1
    if second["tagged"] != second["geometric"]:
        print("[FAIL] page 2 is the control and must agree", file=sys.stderr)
        return 1
    print(f"[OK]  page 1 tagged {first['tagged']}")
    print(f"[OK]  page 1 geometric {first['geometric']}")
    print(f"[OK]  page 2 agrees both ways: {second['tagged']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
