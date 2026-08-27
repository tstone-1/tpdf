#!/usr/bin/env python3
"""Generates a page carrying images a redaction region can be dragged over.

`docs/PLAN.md` section 6 leaves a region over a picture reported and unremoved,
which on a scanned page means a redaction that does nothing. Removing one is
removing the `Do` that draws it -- the same mechanism as a show operator -- plus
the resource entry, after which the sweep every rewrite runs drops the object
itself.

**The pixels are the check, and that is why they are uncompressed.** "The page
no longer draws it" and "the bytes have left the file" are different claims, and
only the second is a redaction; a compressed stream cannot be searched for, so
each image here is a raw `/DeviceRGB` block whose content is one repeated marker.
A probe greps the written file for the marker and the answer is unambiguous.

  page 1
    * a line of text. The CONTROL: removing an image must not touch it.
    * /Im0, marked. Its marker must leave the file.
    * /Im1, elsewhere on the page and not marked. Its marker must stay -- without
      it, a removal that dropped every image would pass every other check.

  page 2
    * /Im2, drawn TWICE. Removing one `Do` would stop this page drawing it once
      and leave the pixels in the file, so it is refused -- the same rule and the
      same message a form drawn twice gets.

Base-14 Helvetica and no compression: no dependency, so a hosted runner can
build it.

Usage: python3 make_image_pdf.py <out.pdf>
"""

import sys

WIDTH, HEIGHT = 595, 842
CONTROL = "Sphinx of black quartz, judge my vow."

# 8x8 RGB, uncompressed. Each image's marker is a four-byte pattern repeated to
# fill it, chosen so it cannot occur by accident anywhere else in the file.
SIDE = 8
MARKERS = {
    "Im0": b"\xde\xad\xbe\xef",
    "Im1": b"\xca\xfe\xd0\x0d",
    "Im2": b"\xfe\xed\xfa\xce",
}


def image(marker: bytes) -> bytes:
    body = (marker * ((SIDE * SIDE * 3) // len(marker) + 1))[: SIDE * SIDE * 3]
    head = (
        f"<< /Type /XObject /Subtype /Image /Width {SIDE} /Height {SIDE} "
        f"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {len(body)} >>"
    )
    return head.encode("latin-1") + b"\nstream\n" + body + b"\nendstream"


def build() -> bytes:
    objects: dict[int, bytes] = {}

    page1 = (
        f"BT /F1 12 Tf 60 700 Td ({CONTROL}) Tj ET\n"
        "q 120 0 0 60 60 560 cm /Im0 Do Q\n"
        "q 120 0 0 60 60 400 cm /Im1 Do Q\n"
    ).encode("latin-1")
    page2 = (
        "q 120 0 0 60 60 700 cm /Im2 Do Q\nq 120 0 0 60 60 560 cm /Im2 Do Q\n"
    ).encode("latin-1")

    objects[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[2] = b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"
    objects[3] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Resources << /Font << /F1 5 0 R >> "
        f"/XObject << /Im0 6 0 R /Im1 7 0 R >> >> /Contents 9 0 R >>"
    ).encode("latin-1")
    objects[4] = (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Resources << /XObject << /Im2 8 0 R >> >> /Contents 10 0 R >>"
    ).encode("latin-1")
    objects[5] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
    objects[6] = image(MARKERS["Im0"])
    objects[7] = image(MARKERS["Im1"])
    objects[8] = image(MARKERS["Im2"])
    objects[9] = (
        f"<< /Length {len(page1)} >>".encode("latin-1")
        + b"\nstream\n"
        + page1
        + b"\nendstream"
    )
    objects[10] = (
        f"<< /Length {len(page2)} >>".encode("latin-1")
        + b"\nstream\n"
        + page2
        + b"\nendstream"
    )

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
        raise SystemExit("usage: make_image_pdf.py <out.pdf>")
    with open(sys.argv[1], "wb") as fh:
        fh.write(build())
    print(f"[OK] wrote {sys.argv[1]}")


if __name__ == "__main__":
    main()
