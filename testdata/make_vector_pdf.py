#!/usr/bin/env python3
"""Generates a deliberately hostile vector PDF for the Phase 0 render spike.

An A0-size page carrying a few hundred thousand path segments, which is the
case docs/PLAN.md section 4 keeps citing: tiling bounds the output bitmap, but
it does NOT bound Pdfium's display-list traversal, so a single 512px tile of
this page still walks most of the page's objects. That is the thing worth
measuring, and it needs a page that actually exhibits it.

Deterministic: fixed seed, so benchmark runs are comparable across machines.

Usage: python3 make_vector_pdf.py <out.pdf> [segments]
"""

import random
import sys
import zlib

# A0 in PDF points (1/72 inch).
WIDTH, HEIGHT = 2384, 3370


def content_stream(segments: int) -> bytes:
    """Builds a content stream of stroked polylines, arcs and hatching."""
    random.seed(20260726)
    out = ["0.25 w"]

    # A dense hatch grid -- the kind of fill a CAD export produces, and cheap to
    # describe but expensive to traverse.
    for x in range(0, WIDTH, 7):
        out.append(f"{x} 0 m {x} {HEIGHT} l S")
    for y in range(0, HEIGHT, 7):
        out.append(f"0 {y} m {WIDTH} {y} l S")

    # Scattered polylines and bezier curves with per-object colour changes, so
    # the renderer cannot batch them away.
    emitted = len(out)
    while emitted < segments:
        x, y = random.uniform(0, WIDTH), random.uniform(0, HEIGHT)
        out.append(f"{random.random():.3f} {random.random():.3f} {random.random():.3f} RG")
        out.append(f"{x:.2f} {y:.2f} m")
        for _ in range(random.randint(3, 12)):
            if random.random() < 0.4:
                pts = [f"{random.uniform(0, WIDTH):.2f} {random.uniform(0, HEIGHT):.2f}" for _ in range(3)]
                out.append(f"{' '.join(pts)} c")
            else:
                out.append(f"{random.uniform(0, WIDTH):.2f} {random.uniform(0, HEIGHT):.2f} l")
            emitted += 1
        out.append("S")

    return zlib.compress("\n".join(out).encode("ascii"), 6)


def build(path: str, segments: int) -> None:
    stream = content_stream(segments)

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {WIDTH} {HEIGHT}] "
        f"/Contents 4 0 R /Resources << >> >>".encode("ascii"),
        b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(stream)
        + stream
        + b"\nendstream",
    ]

    body = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for number, payload in enumerate(objects, start=1):
        offsets.append(len(body))
        body += b"%d 0 obj\n" % number + payload + b"\nendobj\n"

    xref_at = len(body)
    body += b"xref\n0 %d\n" % (len(objects) + 1)
    body += b"0000000000 65535 f \n"
    for offset in offsets:
        body += b"%010d 00000 n \n" % offset
    body += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objects) + 1,
        xref_at,
    )

    with open(path, "wb") as handle:
        handle.write(body)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "vector-heavy.pdf"
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 200_000
    build(out, count)
    print(f"[OK] wrote {out}")
