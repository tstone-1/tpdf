#!/usr/bin/env python3
"""Synthetic boxed-edit round trip, independently read with pypdf.

uv run --with pypdf scripts/text_layout_check.py --generate scratch/layout-fixture
text-edit-probe --roundtrip scratch/layout-fixture/source.pdf scratch/layout-fixture/requests.json scratch/layout-result
uv run --with pypdf scripts/text_layout_check.py --check scratch/layout-fixture/source.pdf scratch/layout-result/edited.pdf
"""
import argparse
import json
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject


def generate(directory):
    directory.mkdir(parents=True, exist_ok=True)
    writer = PdfWriter()
    font = writer._add_object(DictionaryObject({NameObject(k): NameObject(v) for k, v in
        [("/Type", "/Font"), ("/Subtype", "/Type1"), ("/BaseFont", "/Helvetica"), ("/Encoding", "/WinAnsiEncoding")]}))
    stream = DecodedStreamObject()
    stream.set_data(b"BT /F1 12 Tf 40 240 Td (TITLE) Tj 200 0 Td (NEXT) Tj ET\n"
                    b"BT /F1 12 Tf 40 160 Td (WIDE CELL) Tj 200 0 Td (NEIGHBOUR) Tj ET\n"
                    b"BT /F1 12 Tf 40 40 Td (UNCHANGED BELOW) Tj ET\n")
    content = writer._add_object(stream)
    for _ in range(2):
        page = writer.add_blank_page(400, 300)
        page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
        page[NameObject("/Contents")] = content
    writer.write(directory / "source.pdf")
    edits = [dict(page=0, contains="TITLE", replacement="ACME \u03a9 Al\u2082O\u2083",
                  layout=dict(width=190, height=20, size=12, wrap=False, font="auto")),
             dict(page=0, contains="WIDE CELL", replacement="SYNTHETIC FIRST SECOND\nTHIRD",
                  layout=dict(width=85, height=65, size=10, wrap=True, font="noto_sans_bold"))]
    (directory / "requests.json").write_text(json.dumps(edits), encoding="utf-8")
    print("[OK] generated synthetic two-page layout fixture and edit requests")


def check(source, saved):
    before, after = PdfReader(source), PdfReader(saved)
    assert len(after.pages) == len(before.pages) == 2
    text = after.pages[0].extract_text()
    assert "ACME \u03a9 Al\u2082O\u2083" in text
    assert "SYNTHETIC FIRST SECOND THIRD" in " ".join(text.split())
    assert all(word in text for word in ["NEXT", "NEIGHBOUR", "UNCHANGED BELOW"])
    assert "TITLE" not in text and "WIDE CELL" not in text
    assert before.pages[1].extract_text() == after.pages[1].extract_text()
    assert before.pages[1].get_contents().get_data() == after.pages[1].get_contents().get_data()
    fonts = after.pages[0]["/Resources"]["/Font"]
    embedded = [value.get_object() for name, value in fonts.items() if name.startswith("/TPDFEdit")]
    assert len(embedded) == 2
    for font in embedded:
        child = font["/DescendantFonts"][0].get_object()
        assert len(child["/FontDescriptor"]["/FontFile2"].get_data()) > 100000
        assert len(child["/CIDToGIDMap"].get_data()) > 2
        assert b"beginbfchar" in font["/ToUnicode"].get_data()
    assert set(after.pages[1]["/Resources"]["/Font"]) == {"/F1"}
    print("[PASS] independent extraction, embedded fonts, wrapping and untouched shared page")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--generate", type=Path)
    mode.add_argument("--check", nargs=2, type=Path)
    args = parser.parse_args()
    if args.generate:
        generate(args.generate)
    else:
        check(*args.check)
