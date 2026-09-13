#!/usr/bin/env python3
"""Write a synthetic kerning-array fixture with the independent pypdf writer.

uv run --with pypdf testdata/make_textedit_kerning.py <output.pdf>
Uses the native text-edit probe's two lines and geometry. This isolates TJ;
it is not a claim that Quartz or LibreOffice exports are already editable.
"""
import sys
from pathlib import Path

from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

if len(sys.argv) != 2:
    raise SystemExit("expected output.pdf")
target = Path(sys.argv[1])
target.parent.mkdir(parents=True, exist_ok=True)
writer = PdfWriter()
page = writer.add_blank_page(width=300, height=240)
font = DictionaryObject({NameObject(k): NameObject(v) for k, v in {
    "/Type": "/Font", "/Subtype": "/Type1", "/BaseFont": "/Helvetica",
    "/Encoding": "/WinAnsiEncoding",
}.items()})
page[NameObject("/Resources")] = DictionaryObject({
    NameObject("/Font"): DictionaryObject({NameObject("/F1"): writer._add_object(font)})
})
content = DecodedStreamObject()
content.set_data(
    b"BT /F1 12 Tf 40 180 Td [(SYN) 12.5 (THETIC) -7.5 ( FIRST)] TJ ET\n"
    b"BT /F1 12 Tf 40 140 Td [(SYNTHETIC) -5 ( SECOND)] TJ ET"
)
page[NameObject("/Contents")] = writer._add_object(content)
writer.write(target)
print("[OK] wrote synthetic kerning-array fixture")
