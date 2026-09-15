#!/usr/bin/env python3
"""Generate and independently check consecutive text shows.

uv run --with pypdf scripts/text_continuation_check.py --generate <input.pdf> [--array] [--inline tab|bell|tabs]
uv run --with pypdf scripts/text_continuation_check.py <before.pdf> <after.pdf>
Use text-edit-probe <directory> <input.pdf> --continued, or
tabs_check.py --phase textedit with the generated fixture.
PDFKit: swift scripts/text_edit_pdfkit.swift <probe-directory> --continued
Use --inline in that PDFKit command for fixtures generated with --inline.
"""
import argparse
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, NameObject, StreamObject

ROOT = Path(__file__).resolve().parents[1]


def value(obj):
    obj = obj.get_object()
    if isinstance(obj, StreamObject):
        return ({str(k): value(v) for k, v in obj.items()
                 if k not in ("/Length", "/Filter", "/DecodeParms")}, obj.get_data())
    if isinstance(obj, dict):
        return {str(k): value(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [value(v) for v in obj]
    return obj


def generate(path, array, inline=None):
    writer = PdfWriter(clone_from=ROOT / "testdata/textedit-embedded.pdf")
    first = b"[(SYNTHETIC FIRST) -125] TJ" if array else b"(SYNTHETIC FIRST) Tj"
    spacer = b""
    if inline:
        actual = {"tab": b"FEFF0009", "bell": b"FEFF0007", "tabs": b"FEFF00090009"}[inline]
        show = b"[( ) -125 ( )] TJ" if inline == "tabs" else b"( ) Tj"
        spacer = b"/Span << /ActualText <" + actual + b"> >> BDC " + show + b" EMC "
    stream = DecodedStreamObject()
    stream.set_data(b"% SYNTHETIC continuation\nBT /F1 12 Tf 40 180 Td " + first
                    + b" % untouched follower\n" + spacer + b"( SYNTHETIC SECOND) Tj ET")
    writer.pages[0][NameObject("/Contents")] = writer._add_object(stream)
    path.parent.mkdir(parents=True, exist_ok=True)
    writer.write(path)


def check(before, after):
    readers = [PdfReader(p) for p in (before, after)]
    assert all(len(r.pages) == 1 for r in readers), "expected one-page fixtures"
    pages = [r.pages[0] for r in readers]
    assert value(pages[0]["/Resources"]) == value(pages[1]["/Resources"]), "resources changed"
    for key in ("/MediaBox", "/CropBox", "/Rotate"):
        assert pages[0].get(key) == pages[1].get(key), "page geometry changed"
    ops = [p.get_contents().operations for p in pages]
    assert len(ops[0]) == len(ops[1]), "operator count changed"
    changed = [i for i, pair in enumerate(zip(*ops)) if pair[0] != pair[1]]
    assert changed == [3], "expected only the first text show to change"
    assert ops[0][:3] == [([], b"BT"), (["/F1", 12], b"Tf"), ([40, 180], b"Td")]
    suffix = ops[0][4:]
    if suffix[0][1] == b"BDC":
        assert suffix[0][0][0] == "/Span" and set(suffix[0][0][1]) == {"/ActualText"}
        actual = suffix[0][0][1]["/ActualText"]
        assert actual in ("\t", "\x07", "\t\t")
        expected = ([[" ", -125, " "]], b"TJ") if len(actual) == 2 else ([" "], b"Tj")
        assert suffix[1] == expected and suffix[2] == ([], b"EMC")
        suffix = suffix[3:]
    assert suffix == [([" SYNTHETIC SECOND"], b"Tj"), ([], b"ET")]
    font = pages[0]["/Resources"]["/Font"]["/F1"]
    assert font["/Encoding"] == "/WinAnsiEncoding", "expected explicit fixture encoding"

    def advance(operation, expected):
        args, op = operation
        assert len(args) == 1 and op in (b"Tj", b"TJ")
        parts = args[0] if op == b"TJ" else args
        text, distance = "", 0.0
        for part in parts:
            if isinstance(part, str):
                text += part
                distance += sum(float(font["/Widths"][byte - font["/FirstChar"]])
                                for byte in part.original_bytes) * 12 / 1000
            else:
                distance -= float(part) * 12 / 1000
        assert text == expected, "wrong text operand"
        return distance

    old = advance(ops[0][3], "SYNTHETIC FIRST")
    new = advance(ops[1][3], "EDITED FIRST")
    assert ops[1][3][1] == b"TJ" and len(ops[1][3][0][0]) == 2, "missing compensation"
    assert abs(old - new) < 1e-6, "following text would move"
    for page, first in zip(pages, ("SYNTHETIC FIRST", "EDITED FIRST")):
        assert page.extract_text().split() == (first + " SYNTHETIC SECOND").split(), "wrong extracted text"
    untouched = pages[0].get_contents().get_data().split(b"% untouched follower\n", 1)[1]
    assert b"% untouched follower\n" + untouched in pages[1].get_contents().get_data()
    print("[PASS] independent parser: replacement, following origin, resources and untouched operators")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--generate", type=Path)
    parser.add_argument("--array", action="store_true")
    parser.add_argument("--inline", choices=("tab", "bell", "tabs"))
    args = parser.parse_args()
    if args.generate and not args.paths:
        generate(args.generate, args.array, args.inline)
    elif not args.generate and not args.array and not args.inline and len(args.paths) == 2:
        check(*args.paths)
    else:
        parser.error("provide --generate <input.pdf> [--array], or <before.pdf> <after.pdf>")


if __name__ == "__main__":
    main()
