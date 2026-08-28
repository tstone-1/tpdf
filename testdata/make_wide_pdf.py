#!/usr/bin/env python3
"""Generates `text-wide.pdf`: an ordinary page on a very wide sheet.

Written for the OCR gate's shape sweep, and the reason is arithmetic rather than
a property of the text. `ocr_gate::stack` builds a probe image
`margin | region | gap | control | margin` tall and one page wide, so its aspect
is `width_pt / (tallest + control_pt + padding)` with `padding` fixed at 24 pt --
two 6 pt margins and a 12 pt gap. On A4 that caps the shape at about 11:1 with
readable text, because reaching 16:1 would need the region and the control
together to come to under 13 pt.

`redact-reach-probe` measures real documents going **silent** above 16:1 --- the
engine answering and returning no spans at all --- and until this fixture nothing
in `testdata/` could build a probe image that wide. The two that came closest,
`text-heavy.pdf` and `incr-xrefstream.pdf`, reach 12.2:1 with a control so small
that the sweep's own control check refuses their columns: the token does not read
back at the gate's own shape, so nothing further out is attributable to shape.

**The lever is the page's width, not the text's size**, and that is the point of
this fixture rather than an accident of it. Measured: a 1684 pt sheet with
ordinary 14 pt text builds an **18.1:1** probe image and sweeps to 28.1:1, where
A4 with the same text caps out at 10.8:1. The control strip is 34.5 pt tall
against A4's 30.5 -- near enough the same, and well clear of
`ocr::MIN_CONTROL_PX`. A wide image and a comfortably readable control pull
against each other on A4 and do not here.

A first draft of this paragraph predicted 32:1 from `1684 / (14 + 14 + 24)`,
which is wrong because a control *strip* is not a font size: the crop around a
recognised span runs about 2.4x the point size, so the arithmetic wants 34.5
where 14 was written. The numbers above are read off a run.

Landscape sheets this wide are ordinary in the world the corpus comes from:
spreadsheets printed to PDF, drawings, timelines. Nothing here is exotic except
the aspect ratio, which is the variable under test.

Base-14 Helvetica, so **no font is embedded and fonttools is not needed** --- this
script is dependency-free and `scripts/ci_fixtures.py` builds it on a runner.
That is why it is its own file rather than a fifth builder inside
`make_text_pdf.py`, whose other three embed a system font.

Usage: testdata/make_wide_pdf.py [outdir]
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import Pdf, escape  # noqa: E402

#: A3 landscape. The width is the whole point; the height only has to hold a few
#: lines, and a short sheet keeps the file small.
WIDTH, HEIGHT = 1684, 595

#: 14 pt, the same size `make_text_pdf.py` uses. Held equal on purpose: if this
#: fixture also shrank the text it would move two variables at once, and the
#: sweep could not say whether a wide image or a small control produced its
#: answer -- which is exactly the confound that makes `text-heavy.pdf`'s columns
#: unreadable.
SIZE = 14

#: Distinct words of five characters or more, so `ocr::control_from_page` has
#: several candidates and the one it picks is not the only one on the sheet.
#: Spread across the width rather than left-aligned, because a control cropped
#: from a line with nothing beside it is a strip no real page produces.
LINES = [
    "Ledger column headings run the whole width of this landscape sheet.",
    "REDACT ME: account 4711-0815 belongs to A. Beispiel.",
    "Quarterly totals, regional splits, and a footnote nobody reads.",
]


def build_wide(path: str) -> None:
    """Writes the wide fixture: Helvetica, no embedded font, no subsetting."""
    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    content = ""
    for index, line in enumerate(LINES):
        y = HEIGHT - 120 - index * 60
        content += "BT /F1 %d Tf 60 %d Td (%s) Tj ET\n" % (
            SIZE,
            y,
            escape(line).decode("latin-1"),
        )
    stream = pdf.stream(b"<< >>", content.encode("latin-1"))
    page = pdf.reserve()
    pages = pdf.reserve()
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
        % (pages, WIDTH, HEIGHT, font, stream),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
    with open(path, "wb") as handle:
        handle.write(pdf.serialize(root))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("outdir", nargs="?", default="testdata")
    args = parser.parse_args()
    os.makedirs(args.outdir, exist_ok=True)
    path = os.path.join(args.outdir, "text-wide.pdf")
    build_wide(path)
    print(f"[OK] {path} ({os.path.getsize(path)} bytes), {WIDTH}x{HEIGHT} pt")
    return 0


if __name__ == "__main__":
    sys.exit(main())
