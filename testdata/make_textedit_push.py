#!/usr/bin/env python3
"""Write testdata/textedit-push.pdf: a line with a second run on it, and room
after that one.

    python3 testdata/make_textedit_push.py

The window fixtures that existed before it cannot exercise a push. Both runs of
`textedit-embedded.pdf` are on lines of their own -- `text-edit-probe --growth`
reports no right-hand neighbour for either -- so a longer draft there grows the
box and moves nothing, which is the previous increment and not this one.

The geometry is written out because the check that reads it asserts numbers.
Helvetica at 12 pt makes `SYNTHETIC FIRST` 106.008 pt wide and `SECOND COLUMN`
106.668; the second run starts at 200 and ends at 306.668 on a 400 pt page. So
the first run has 160 pt of room before it has to push anything, the draft the
check types is 215.352 pt, and the 55.352 pt of push puts the second run at
255.352 with 38 pt still to spare before the page edge. Measured, not assumed:
`text-edit-probe --roundtrip` on this file moves it there and reports the page
otherwise unchanged.

**The two runs end up flush**, which is the point -- the gap the document left is
spent before anything moves -- so any reader extracting the saved page reads
`FIRSTSECOND` as one word. A check over the text has to look for something other
than the first word of the run that moved.

The third line is below the first and must not move at all. It is called
SYNTHETIC SECOND because the rest of the text-editing phase looks for that name.

No font is embedded: base-14 Helvetica is what `fonts::standard` measures, so the
fixture carries no font programme and nothing here depends on one.
"""

from pathlib import Path

CONTENT = b"""\
BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET
BT /F1 12 Tf 200 180 Td (SECOND COLUMN) Tj ET
BT /F1 12 Tf 40 150 Td (SYNTHETIC SECOND) Tj ET
"""

OBJECTS = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 240] "
    b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
    b"<< /Length %d >>\nstream\n%s\nendstream" % (len(CONTENT), CONTENT),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
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
    path = Path(__file__).resolve().parent / "textedit-push.pdf"
    path.write_bytes(build())
    print(f"[OK] wrote {path} ({path.stat().st_size} bytes)")
