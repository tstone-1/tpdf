#!/usr/bin/env python3
"""Write testdata/redact-pages.pdf: three ways a redacted word can survive.

    python3 testdata/make_redact_pages_pdf.py

`verify::scan` answers one of three things about a word a redaction removed and
the written file still holds: the pages it is on, that more than one page draws
its carrier so no page owns it, or that nothing reachable from a page carries
it. The sentence a reader is shown differs in each case, and
`redact::marked_pages_note` adds a fourth difference on top -- whether the
surviving word sits on a page somebody marked.

No fixture in `testdata/` could reach any of that. Every word in the short
synthetic files occurs once, so every removal is clean and every verdict is the
same one; `text-base14.pdf` is one page, and a *page* attribution needs two.

So this file is laid out to make one marked region produce each answer, and a
fourth region produce none of them:

    page 1 (400 x 360)                     what a region over it produces
    ------------------------------------   ---------------------------------
    y 320  KILO-2200                       still in the file, ON PAGE 1
    y 280  KILO-2200      (never marked)     -- the second copy is why
    y 240  LIMA-3300                       still in the file, ON PAGE 2
    y 200  MIKE-4400                       carried by something two pages draw
    y 160  NOVEMBER-5500                   nothing: it really is gone
    y  40  /X1 Do         (never marked)

    page 2 (400 x 360)
    y 320  LIMA-3300                       the survivor page 1's region leaves
    y 280  PAGE TWO ONLY
    y  40  /X1 Do                          the same form object as page 1's

`NOVEMBER-5500` is the control, and it is the whole reason the other three
readings are evidence about the *fixture* rather than about redaction never
removing anything: it is marked in every pass, it occurs nowhere else, and the
verdict must not name it. A run where all four words are reported is a scan that
reports everything; a run where none is, is one that looks at nothing.

Four properties are load-bearing and a change that breaks any of them turns a
check red for the wrong reason:

* **One word per text-showing operation, one operation per line.** Route B
  removes the whole operation containing any covered glyph, so the needle a
  region produces is everything that operation draws. A line carrying two words
  would make the needle a phrase, and the phrase is what the scan then looks
  for.
* **40 points between baselines.** PDFium is what decides which objects a
  region covers, and it reports these lines at page y 319.87-328.75,
  279.87-288.75, 239.86-248.63, 199.87-208.63 and 159.87-168.75 -- ink boxes
  under 9 pt tall, 40 apart. So a 20 pt band centred on a baseline holds one
  line and comes no closer than 25 pt to the next. The driving phase states the
  rectangles it uses and repeats those numbers.
* **`MIKE-4400` is in the form and on page 1, and the form is drawn by both
  pages.** That is what makes the carrier shared: remove the page's own copy and
  the survivor sits in an object `verify::reach` reaches from two slots, which
  is the one case that has no page number at all.
* **The form sits at the foot of the page**, its ink at page y 49.87-58.63,
  which is 101 pt below the lowest line's. A region overlapping it would plan a
  removal inside the form as well, and the shared copy is the thing that has to
  survive.

No font is embedded: base-14 Helvetica is what `fonts::standard` measures, so
the words reach the content stream as their own ASCII bytes -- which is what
`verify::scan` searches for, and what a CID-keyed subset would hide.
"""

from pathlib import Path

#: Marked on page 1 and left on page 1, because the line below it is a copy.
REPEATED = b"KILO-2200"
#: Marked on page 1 and left on page 2, where no region was marked.
ELSEWHERE = b"LIMA-3300"
#: Marked on page 1 and left in the form object both pages draw.
SHARED = b"MIKE-4400"
#: Marked in every pass and genuinely removed. The control.
ONLY_ONCE = b"NOVEMBER-5500"

#: Drawn by both pages, at the foot of each, and never covered by a region.
FORM = b"BT /F1 12 Tf 0 10 Td (%s) Tj ET\n" % SHARED

PAGE_ONE = b"""\
BT /F1 12 Tf 40 320 Td (%s) Tj ET
BT /F1 12 Tf 40 280 Td (%s) Tj ET
BT /F1 12 Tf 40 240 Td (%s) Tj ET
BT /F1 12 Tf 40 200 Td (%s) Tj ET
BT /F1 12 Tf 40 160 Td (%s) Tj ET
q 1 0 0 1 40 40 cm /X1 Do Q
""" % (REPEATED, REPEATED, ELSEWHERE, SHARED, ONLY_ONCE)

PAGE_TWO = b"""\
BT /F1 12 Tf 40 320 Td (%s) Tj ET
BT /F1 12 Tf 40 280 Td (PAGE TWO ONLY) Tj ET
q 1 0 0 1 40 40 cm /X1 Do Q
""" % ELSEWHERE

#: `/X1` is one object named by both pages' resources, which is the whole of
#: what makes its text a shared carrier rather than two copies.
OBJECTS = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 360] "
    b"/Resources << /Font << /F1 7 0 R >> /XObject << /X1 8 0 R >> >> "
    b"/Contents 4 0 R >>",
    b"<< /Length %d >>\nstream\n%s\nendstream" % (len(PAGE_ONE), PAGE_ONE),
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 360] "
    b"/Resources << /Font << /F1 7 0 R >> /XObject << /X1 8 0 R >> >> "
    b"/Contents 6 0 R >>",
    b"<< /Length %d >>\nstream\n%s\nendstream" % (len(PAGE_TWO), PAGE_TWO),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 200 30] "
    b"/Resources << /Font << /F1 7 0 R >> >> /Length %d >>\nstream\n%s\nendstream"
    % (len(FORM), FORM),
]


def build() -> bytes:
    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for index, body in enumerate(OBJECTS, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % index + body + b"\nendobj\n"
    start = len(out)
    out += b"xref\n0 %d\n" % (len(OBJECTS) + 1)
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(OBJECTS) + 1,
        start,
    )
    return bytes(out)


if __name__ == "__main__":
    path = Path(__file__).resolve().parent / "redact-pages.pdf"
    path.write_bytes(build())
    print(f"[OK] wrote {path} ({path.stat().st_size} bytes)")
