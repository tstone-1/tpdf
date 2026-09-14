#!/usr/bin/env python3
"""Write small, redistributable PDFium text-ordering regression fixtures.

Usage: uv run --with fonttools --with arabic-reshaper testdata/make_rtl_pdf.py \
    OUT --font DejaVuSans.ttf

The font must be DejaVu Sans from the project's 2.37 binary release, whose
digest is checked before embedding. Ship the release's LICENSE alongside any
fixture sent upstream. No system font is accepted. The font release is at
https://github.com/dejavu-fonts/dejavu-fonts/releases/tag/version_2_37 .

These deliberately emit visual glyph order in one ordinary Tj instruction,
without ActualText or a document-level direction override. Explicit expected
logical text is the oracle; it is never learned from PDFium's extraction.
"""

import argparse
import hashlib
import json
from pathlib import Path
import unicodedata

from arabic_reshaper import ArabicReshaper

from make_multilingual_pdf import Embedded, presentation_forms, visual_order
from make_text_pdf import Pdf

FONT_SHA256 = "7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954"


def cases():
    arabic = "البحر الأزرق واسع"
    hebrew = "שלום עולם היום"
    shaped = ArabicReshaper(configuration={"support_ligatures": False}).reshape(arabic)
    if unicodedata.normalize("NFKC", shaped) != arabic:
        raise ValueError("Shaping changed the authored Arabic text")
    return [
        ("arabic", arabic, visual_order(arabic)),
        ("arabic-forms", arabic, visual_order(presentation_forms(arabic))),
        ("arabic-shaped", arabic, shaped[::-1]),
        ("arabic-latin", "الملف PDF جاهز", visual_order("الملف PDF جاهز")),
        ("hebrew", hebrew, visual_order(hebrew)),
        ("hebrew-latin", "שלום PDF עולם", visual_order("שלום PDF עולם")),
        ("latin", "PDF file is ready", "PDF file is ready"),
        ("latin-hebrew", "Hello is שלום", "Hello is " + "שלום"[::-1]),
        # More RTL segments than LTR segments do not imply an RTL paragraph.
        ("latin-prefix", "Hello " + hebrew, "Hello " + visual_order(hebrew)),
        ("latin-suffix", hebrew + " Hello", visual_order(hebrew) + " Hello"),
        ("arabic-numbers", "البحر 123-456 واسع", visual_order("البحر 123-456 واسع")),
    ]


def build(out: Path, font_path: Path):
    if hashlib.sha256(font_path.read_bytes()).hexdigest() != FONT_SHA256:
        raise ValueError("Expected the redistributable DejaVu Sans 2.37 font")
    out.mkdir(parents=True, exist_ok=True)
    manifest = {"font_sha256": FONT_SHA256, "cases": []}
    for label, expected, drawn in cases():
        pdf = Pdf()
        pages = pdf.reserve()
        font = Embedded(pdf, "RT", str(font_path), drawn, {})
        if any(font.gid(char) == 0 for char in drawn):
            raise ValueError(f"Font subset lacks a glyph for {label}")
        stream = pdf.stream(
            b"<< >>",
            b"BT /F1 24 Tf 50 100 Td <" + font.encode(drawn) + b"> Tj ET\n",
            compress=False,
        )
        page = pdf.add(
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 600 200] "
            b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>"
            % (pages, font.ref, stream)
        )
        pdf.put(pages, b"<< /Type /Pages /Kids [%d 0 R] /Count 1 >>" % page)
        root = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages)
        target = out / (label + ".pdf")
        target.write_bytes(pdf.serialize(root))
        manifest["cases"].append({
            "file": target.name, "expected": expected,
            "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
        })
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"[OK] wrote {len(manifest['cases'])} synthetic text-ordering fixtures")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out", type=Path)
    parser.add_argument("--font", type=Path, required=True)
    args = parser.parse_args()
    build(args.out, args.font)
