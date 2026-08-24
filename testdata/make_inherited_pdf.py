#!/usr/bin/env python3
"""Generates a document whose pages state nothing and inherit everything.

PDF 32000-1 table 29 lets a page take /Resources, /MediaBox, /CropBox and
/Rotate from any ancestor in the page tree. Every other fixture in this corpus
writes those on the page itself, so every one of them is blind to the mistake
this file exists to catch: copying a page *out* of the tree that supplies them.

Measured, 2026-08-24: mutating `pagetree::detached_page` to materialise nothing
left `merge-probe` at 38/38 on `rotated.pdf` + `links.pdf`, because no page in
either inherits anything. The check that is the whole point of that function --
"the incoming page keeps the size it had in its own file" -- could not fail. That
is the trap this project records as a fixture where the right rule and the wrong
rule agree.

The shape is one intermediate /Pages node between the root and the leaves:

    catalog -> /Pages root -> /Pages node (states everything) -> three /Page

so a page lifted out of it and hung somewhere else loses its size, its
resources and its rotation at once. What that looks like downstream is not a
failure: PDFium falls back to US Letter, so the page comes out 612x792 upright
instead of 400x600 turned a quarter, renders with substituted fonts, and looks
like a page.

**The size is deliberately not a standard one.** 400x600 is neither Letter nor
A4, so a fallback is visible in the number rather than plausible.

**/Rotate 90 is on the node too**, because the four inheritable keys are not
equally easy to lose: a missing /Rotate transposes the reported size, which is
the same shape of error as a missing /MediaBox and would be masked by it on a
square page. This page is not square.

Each page carries different text, so a comparison that landed on the wrong page
says so rather than passing.

Base-14 Helvetica, so this needs no font file and nothing it writes is anyone
else's to redistribute.

Usage: python3 make_inherited_pdf.py <outdir>
"""

import os
import sys

WIDTH, HEIGHT = 400, 600
ROTATE = 90

PAGES = (
    ("alpha", "one two three four five"),
    ("bravo", "six seven eight nine ten"),
    ("charlie", "eleven twelve thirteen fourteen"),
)


def body(name: str, words: str) -> bytes:
    """Six lines down the page, naming the page so a mix-up is visible."""
    lines = [f"BT /F1 18 Tf 40 {HEIGHT - 60} Td (Page {name}) Tj ET"]
    y = HEIGHT - 110
    for row in range(5):
        lines.append(f"BT /F1 12 Tf 40 {y} Td (Line {row + 1} of {name}: {words}) Tj ET")
        y -= 22
    return "\n".join(lines).encode("ascii")


def build(path: str) -> None:
    # 1 catalog, 2 page tree root, 3 the node that states everything, 4 font,
    # then one content stream and one page object per page -- interleaved, so
    # that a page and its stream are adjacent object numbers and a shift that
    # went wrong is easy to read in the file.
    first = 5
    kids = " ".join("%d 0 R" % (first + 1 + 2 * i) for i in range(len(PAGES)))

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count %d >>" % len(PAGES),
        # The node under test. Everything a page needs is here and nowhere else.
        (
            "<< /Type /Pages /Parent 2 0 R /Kids [%s] /Count %d "
            "/MediaBox [0 0 %d %d] /Rotate %d "
            "/Resources << /Font << /F1 4 0 R >> >> >>"
            % (kids, len(PAGES), WIDTH, HEIGHT, ROTATE)
        ).encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    for index, (name, words) in enumerate(PAGES):
        stream = body(name, words)
        objects.append(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
        # No /MediaBox, no /Resources, no /Rotate. That absence is the fixture.
        objects.append(
            ("<< /Type /Page /Parent 3 0 R /Contents %d 0 R >>" % (first + 2 * index)).encode(
                "ascii"
            )
        )

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for number, payload in enumerate(objects, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + payload + b"\nendobj\n"

    xref_at = len(out)
    out += b"xref\n0 %d\n" % (len(objects) + 1)
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1,
        xref_at,
    )

    with open(path, "wb") as handle:
        handle.write(out)


if __name__ == "__main__":
    outdir = sys.argv[1] if len(sys.argv) > 1 else "."
    path = os.path.join(outdir, "inherited.pdf")
    build(path)
    print(
        f"[OK] wrote {path}: {len(PAGES)} pages stating nothing, "
        f"inheriting {WIDTH}x{HEIGHT} at /Rotate {ROTATE} from the node above them"
    )
