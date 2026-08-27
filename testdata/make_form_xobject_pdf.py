#!/usr/bin/env python3
"""Generates a page whose text lives inside Form XObjects.

`docs/PLAN.md` section 6 measures form XObjects as the largest carrier a
redaction cannot take that is made of ordinary text -- 9,310 of 154,095
realistic regions across 41 real documents, three times the image count. PDFium
enumerates a form as ONE page object, so the text inside it is not in the page's
text-object list and `redact::remove_shows` cannot address it. Nothing in the
corpus exercises that: every fixture carrying `/Subtype /Form` carries it as an
annotation appearance stream, which is a different thing in a different place.

Four cases, chosen so each rule the removal needs has a subject and so that no
two of them can be confused for one another:

  page 1
    * page-level text, drawn by the page's own stream. The CONTROL: a removal
      inside a form must not touch it, and it is what the OCR gate reads back.
    * /Fm0 -- a form carrying TWO lines, one to remove and one to keep. Removing
      by position inside a form is the whole mechanism, and a form with one line
      in it cannot tell "removed the right one" from "removed everything".
      Placed with a translating matrix, so form space and page space differ and
      a bounds convention error cannot pass.
    * /Fm1 -- a form drawing a NESTED form (/Fm2) that carries text. Descending
      one level is a decision; text a level further down has to be reported
      rather than silently missed, and this is what a check for that reads.

  page 2
    * /Fm3, drawn TWICE. A form's stream is shared by every reference to it, so
      removing from it changes all of them -- the refusal case, and the same
      posture the structure carrier already takes for a shared element.

  page 3
    * /Fm4 -- a form carrying text at one end and a PATH at the other, far
      enough apart that no region reaches both. Every other form here has its
      unreachable child sitting on top of its text, so the right rule and the
      wrong one agree on all of them: reporting a child the region covers and
      reporting every child of a form the region touches are the same answer
      when the two are in the same place. Measured over 40 real documents,
      56% of every refusal was a form child and every image refusal was one,
      because a form is routinely a whole-page container. This is the page that
      can tell the two rules apart, and it needs both regions: one over the
      text, which must report nothing, and one over the path, which must.

Base-14 Helvetica throughout: no font to embed, so a hosted runner can build it.

Usage: python3 make_form_xobject_pdf.py <out.pdf>
"""

import sys

WIDTH, HEIGHT = 595, 842

# What each form draws. The strings are the fixture's whole vocabulary, so a
# test can say which line went by name rather than by counting.
REMOVE = "REDACT ME: account 4711-0815 inside a form"
KEEP_IN_FORM = "Keep this line, it is in the same form"
CONTROL = "Sphinx of black quartz, judge my vow."
NESTED = "This line is one level further down"
SHARED = "This form is drawn twice"
FAR_TEXT = "Text at one end of the form"


def form(body: str, bbox: tuple[int, int, int, int], resources: str) -> bytes:
    """A Form XObject stream drawing `body` at its own origin."""
    stream = body.encode("latin-1")
    head = (
        f"<< /Type /XObject /Subtype /Form /FormType 1 "
        f"/BBox [{bbox[0]} {bbox[1]} {bbox[2]} {bbox[3]}] "
        f"/Resources {resources} /Length {len(stream)} >>"
    )
    return head.encode("latin-1") + b"\nstream\n" + stream + b"\nendstream"


def text(x: int, y: int, s: str, size: int = 12) -> str:
    return f"BT /F1 {size} Tf {x} {y} Td ({s}) Tj ET\n"


def build() -> bytes:
    objects: dict[int, bytes] = {}

    font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    font_res = "<< /Font << /F1 5 0 R >> >>"

    # /Fm0: two lines at its own origin. The BBox is generous so neither line is
    # clipped -- a clipped line is drawn and not visible, which is a different
    # fixture entirely.
    fm0 = form(text(0, 20, REMOVE) + text(0, 0, KEEP_IN_FORM), (0, -6, 400, 40), font_res)

    # /Fm2 is nested inside /Fm1, which draws nothing of its own.
    fm2 = form(text(0, 0, NESTED), (0, -6, 400, 20), font_res)
    fm1 = form("q 1 0 0 1 0 0 cm /Fm2 Do Q\n", (0, -6, 400, 20), "<< /XObject << /Fm2 8 0 R >> >>")

    fm3 = form(text(0, 0, SHARED), (0, -6, 400, 20), font_res)

    # /Fm4: text at the form's origin and a filled rectangle 300 points to the
    # right of it. Nothing in this fixture but this pair is far enough apart for
    # a region to cover one and miss the other.
    fm4 = form(
        text(0, 0, FAR_TEXT) + "0 0 0 rg 300 0 40 12 re f\n",
        (0, -6, 400, 20),
        font_res,
    )

    # Page 1 draws its own line of text, then the two forms, each translated so
    # form space is not page space.
    page1_content = (
        text(60, 700, CONTROL)
        + "q 1 0 0 1 60 600 cm /Fm0 Do Q\n"
        + "q 1 0 0 1 60 500 cm /Fm1 Do Q\n"
    ).encode("latin-1")

    # Page 2 draws one form twice, at two positions.
    page2_content = (
        "q 1 0 0 1 60 700 cm /Fm3 Do Q\nq 1 0 0 1 60 600 cm /Fm3 Do Q\n"
    ).encode("latin-1")

    # Page 3 draws the one form whose two children are far apart.
    page3_content = "q 1 0 0 1 60 700 cm /Fm4 Do Q\n".encode("latin-1")

    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = b"<< /Type /Pages /Kids [3 0 R 4 0 R 12 0 R] /Count 3 >>"
    objects[3] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Resources << /Font << /F1 5 0 R >> "
        f"/XObject << /Fm0 6 0 R /Fm1 7 0 R >> >> /Contents 9 0 R >>"
    ).encode("latin-1")
    objects[4] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Resources << /XObject << /Fm3 11 0 R >> >> /Contents 10 0 R >>"
    ).encode("latin-1")
    objects[5] = font
    objects[6] = fm0
    objects[7] = fm1
    objects[8] = fm2
    objects[9] = (
        f"<< /Length {len(page1_content)} >>".encode("latin-1")
        + b"\nstream\n"
        + page1_content
        + b"\nendstream"
    )
    objects[10] = (
        f"<< /Length {len(page2_content)} >>".encode("latin-1")
        + b"\nstream\n"
        + page2_content
        + b"\nendstream"
    )
    objects[11] = fm3
    objects[12] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Resources << /Font << /F1 5 0 R >> /XObject << /Fm4 14 0 R >> >> "
        f"/Contents 13 0 R >>"
    ).encode("latin-1")
    objects[13] = (
        f"<< /Length {len(page3_content)} >>".encode("latin-1")
        + b"\nstream\n"
        + page3_content
        + b"\nendstream"
    )
    objects[14] = fm4

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    for num in sorted(objects):
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("latin-1") + objects[num] + b"\nendobj\n"

    start = len(out)
    top = max(objects) + 1
    out += f"xref\n0 {top}\n".encode("latin-1")
    out += b"0000000000 65535 f \n"
    for num in range(1, top):
        out += f"{offsets[num]:010d} 00000 n \n".encode("latin-1")
    out += f"trailer\n<< /Size {top} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n".encode(
        "latin-1"
    )
    return bytes(out)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: make_form_xobject_pdf.py <out.pdf>")
    with open(sys.argv[1], "wb") as fh:
        fh.write(build())
    print(f"[OK] wrote {sys.argv[1]}")


if __name__ == "__main__":
    main()
