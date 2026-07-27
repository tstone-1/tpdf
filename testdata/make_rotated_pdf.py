#!/usr/bin/env python3
"""Generates a document whose pages carry a /Rotate attribute.

Nothing else in the corpus has one, and a page with /Rotate 90 is the ordinary
output of a scanner, not an edge case. It matters here because Pdfium reports
two different coordinate systems for such a page and they are easy to mix up:

  * FPDF_GetPageWidthF / GetPageHeightF give the size *after* rotation, which is
    what a viewer lays out and what a render call produces.
  * FPDFText_GetCharBox gives boxes in the page's own, *unrotated* space.

So a mapping that flips character boxes against the reported page height is
correct at /Rotate 0 and wrong at every other value -- and wrong in a way that
still produces tidy rectangles, which is the failure mode this project keeps
paying for.

The four pages carry identical content and differ only in /Rotate, so any
difference the probe reports is the rotation and nothing else.

Text is deliberately confined to the **upper** part of the page. The alignment
probe's control asks whether the un-flipped convention *also* lands on ink; on a
page with text spread evenly that control cannot discriminate, and the probe
fails the run rather than reporting a meaningless pass.

Base-14 Helvetica, so this needs no font file and nothing it writes is anyone
else's to redistribute.

Usage: python3 make_rotated_pdf.py <outdir>
"""

import json
import os
import sys

WIDTH, HEIGHT = 612, 792

WORDS = (
    "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima "
    "mike november oscar papa quebec romeo sierra tango"
).split()

ROTATIONS = (0, 90, 180, 270)


def body() -> str:
    """Twelve lines in the top third of the page, each unique at every offset."""
    lines = []
    y = HEIGHT - 90
    for row in range(12):
        words = " ".join(WORDS[(row + step) % len(WORDS)] for step in range(6))
        lines.append(f"BT /F1 13 Tf 60 {y} Td (Line {row + 1:02d} {words}) Tj ET")
        y -= 18
    return "\n".join(lines)


# Two destinations on the first page, as (title, x, y) in the page's own
# unrotated space.
#
# They differ in **both** axes, which is the whole point. Under /Rotate 90 the
# page-space x is what becomes the distance down the display, so the expected
# offsets are 100 and 500. Under the ordinary flip -- height minus y, which is
# right for an upright page and was applied to every page before this fixture
# existed -- they come out 412 and 12 instead: still numbers, still inside the
# page, and in the opposite order. A fixture whose destinations differed only in
# y would produce two *identical* offsets under the wrong mapping, and the check
# would report itself inapplicable rather than fail.
#
# The third names no x at all --- `/XYZ null 600 0`, which is legal and common
# for "this page, at this height". On a rotated page that is a destination whose
# *display* position cannot be computed, because the axis it names is the one
# that became horizontal. It exists so the code path that declines has something
# that reaches it: without it, the guard could be deleted and no check would
# notice, which by this project's own rule makes it a guard to delete rather
# than keep.
DESTINATIONS = (
    ("Top of the sheet", 100, 200),
    ("Further along", 500, 600),
    ("No sideways coordinate", None, 600),
)


def build(path: str, rotations: tuple[int, ...], outline: bool = False) -> None:
    stream = body().encode("ascii")

    # 1 catalog, 2 page tree, 3 font, 4 content, then one page object each,
    # then -- when asked for -- the outline root and its entries.
    first_page = 5
    kids = " ".join("%d 0 R" % (first_page + i) for i in range(len(rotations)))
    outline_root = first_page + len(rotations)
    catalog = "<< /Type /Catalog /Pages 2 0 R"
    if outline:
        catalog += " /Outlines %d 0 R" % outline_root
    catalog += " >>"

    objects = [
        catalog.encode("ascii"),
        ("<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, len(rotations))).encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream",
    ]
    for rotate in rotations:
        objects.append(
            (
                f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
                f"/Rotate {rotate} /Contents 4 0 R "
                f"/Resources << /Font << /F1 3 0 R >> >> >>"
            ).encode("ascii")
        )

    if outline:
        first_entry = outline_root + 1
        last_entry = first_entry + len(DESTINATIONS) - 1
        objects.append(
            (
                "<< /Type /Outlines /First %d 0 R /Last %d 0 R /Count %d >>"
                % (first_entry, last_entry, len(DESTINATIONS))
            ).encode("ascii")
        )
        for index, (title, x, y) in enumerate(DESTINATIONS):
            number = first_entry + index
            entry = "<< /Title (%s) /Parent %d 0 R" % (title, outline_root)
            if index > 0:
                entry += " /Prev %d 0 R" % (number - 1)
            if index < len(DESTINATIONS) - 1:
                entry += " /Next %d 0 R" % (number + 1)
            across = "null" if x is None else str(x)
            entry += " /Dest [%d 0 R /XYZ %s %d 0] >>" % (first_page, across, y)
            objects.append(entry.encode("ascii"))

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

    # One page per rotation, for the alignment probe, which takes a --page.
    mixed = os.path.join(outdir, "rotated.pdf")
    build(mixed, ROTATIONS)
    print(f"[OK] wrote {mixed} with pages at /Rotate {', '.join(map(str, ROTATIONS))}")

    # And a uniform one, because the viewer cannot use the first. Its scroller
    # lays every page out at page 1's size, so a document that alternates
    # portrait and landscape is drawn wrong for reasons that have nothing to do
    # with rotation -- and a check run on it would be measuring that instead.
    uniform = os.path.join(outdir, "rotated-90.pdf")
    build(uniform, (90,) * len(ROTATIONS), outline=True)
    print(f"[OK] wrote {uniform} with every page at /Rotate 90")

    # Its outline is what pins the *destination* half of the same conversion.
    # Character boxes and outline destinations both arrive in the page's own
    # unrotated space and both have to be turned; they go through one function
    # for that reason, and this is the fixture that can tell whether the second
    # caller passes it the right arguments.
    manifest = {
        "rotated-90.pdf": {
            "pages": len(ROTATIONS),
            "roots": len(DESTINATIONS),
            # Under /Rotate 90 the display's vertical axis is the page's x.
            "tops": [x for _title, x, _y in DESTINATIONS if x is not None],
            "entries": [
                {"title": title, "depth": 0, "page": 0, "has_top": x is not None}
                for title, x, _y in DESTINATIONS
            ],
        }
    }
    path = os.path.join(outdir, "rotated-manifest.json")
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    print(f"[OK] wrote {path}")
