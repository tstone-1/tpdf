#!/usr/bin/env python3
"""Default Helvetica fixtures and an independent oracle for the Rust code map.

uv run --with reportlab --with pypdf testdata/make_textedit_default.py scratch/textedit-default --mapping scratch/textedit-default/mapping.json
The mapping file comes from TPDF_DEFAULT_ENCODING_PROBE on the focused Rust test.
"""
import argparse
import json
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DictionaryObject, NameObject, DecodedStreamObject
from reportlab.pdfbase._fontdata import encodings, widthsByFontGlyph
from reportlab.pdfbase._glyphlist import _glyphname2unicode

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("output", type=Path)
parser.add_argument("--mapping", type=Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
rows, codes = [], {}
for code, name in enumerate(encodings["StandardEncoding"]):
    point = _glyphname2unicode.get(name)
    supported = point is not None and (32 <= point <= 126 or 160 <= point <= 255)
    text = chr(point) if supported else None
    width = widthsByFontGlyph["Helvetica"][name] if supported else None
    rows.append(dict(code=code, text=text, width=width, encoded=[code] if supported else None))
    if supported:
        assert text not in codes, "ambiguous independent mapping"
        codes[text] = code
assert len(codes) == 117
if args.mapping:
    actual = json.loads(args.mapping.read_text())
    assert actual == rows, "Rust default encoding differs from independent codes or widths"
    print("[PASS] all 256 Rust mapping decisions and 117 widths agree with ReportLab")

for name, first in [("ascii", "SYNTHETIC FIRST"), ("mapped", "SYNTHETIC ' ` £ ß")]:
    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=240)
    # Deliberately author the ordinary omitted-Encoding form. No output rewrite,
    # normalization or font substitution is used to make this fixture editable.
    font = DictionaryObject({NameObject("/Type"): NameObject("/Font"),
                             NameObject("/Subtype"): NameObject("/Type1"),
                             NameObject("/BaseFont"): NameObject("/Helvetica")})
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"):
        DictionaryObject({NameObject("/F1"): writer._add_object(font)})})
    content = DecodedStreamObject()
    content.set_data(b"\n".join(
        f"BT /F1 12 Tf 40 {y} Td <{bytes(codes[ch] for ch in text).hex()}> Tj ET".encode()
        for y, text in [(180, first), (140, "SYNTHETIC SECOND")]))
    page[NameObject("/Contents")] = writer._add_object(content)
    path = args.output / (name + ".pdf")
    writer.write(path)
    reader = PdfReader(path)
    assert " ".join(reader.pages[0].extract_text().split()) == first + " SYNTHETIC SECOND"
    assert "/Encoding" not in reader.pages[0]["/Resources"]["/Font"]["/F1"]
    print("[PASS] wrote and independently decoded", path.name)
