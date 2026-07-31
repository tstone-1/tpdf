#!/usr/bin/env python3
"""Generates the AcroForm fixture for the progressive-render identity check.

The text field deliberately has a value and no `/AP` appearance stream. PDFium
must initialise its form-fill environment and draw the widget with
`FPDF_FFLDraw`; the ordinary page render cannot make the value visible by
itself. That makes this fixture discriminate the form overlay from
`FPDF_ANNOT`, which already draws annotations with stored appearances.

The page also carries ordinary content. A renderer that draws only the form
would therefore fail the same pixel comparison as one that omits it.

Usage:
    python testdata/make_form_pdf.py [output.pdf]
"""

import argparse
from pathlib import Path

from make_text_pdf import Pdf, escape


WIDTH, HEIGHT = 595, 842
VALUE = "VISIBLE FORM VALUE"


def build(path: Path) -> None:
    """Writes one page with an appearance-less text widget."""
    pdf = Pdf()
    font = pdf.add(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )

    content = pdf.stream(
        b"<< >>",
        (
            b"q 0.88 0.92 0.98 rg 40 600 515 140 re f Q\n"
            b"q 0.10 0.35 0.75 RG 2 w 40 580 m 555 580 l S Q\n"
            b"BT /Helv 14 Tf 60 740 Td (Ordinary page content) Tj ET\n"
        ),
    )

    page = pdf.reserve()
    pages = pdf.reserve()
    widget = pdf.add(
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (fixture-field) "
        b"/V ("
        + escape(VALUE)
        + b") /Rect [72 650 360 692] /F 4 /P %d 0 R " % page
        + b"/DA (/Helv 18 Tf 0 g) >>"
    )
    pdf.put(
        page,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /Helv %d 0 R >> >> /Contents %d 0 R "
        b"/Annots [%d 0 R] >>" % (pages, WIDTH, HEIGHT, font, content, widget),
    )
    pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
    form = pdf.add(
        b"<< /Fields [%d 0 R] /NeedAppearances true "
        b"/DA (/Helv 18 Tf 0 g) /DR << /Font << /Helv %d 0 R >> >> >>"
        % (widget, font)
    )
    root = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R /AcroForm %d 0 R >>" % (pages, form)
    )
    path.write_bytes(pdf.serialize(root))


def main() -> None:
    """Parses the optional output path and creates the fixture."""
    parser = argparse.ArgumentParser()
    parser.add_argument("output", nargs="?", type=Path, default=Path("testdata/form.pdf"))
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    build(args.output)
    print(f"[OK] wrote {args.output} ({args.output.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
