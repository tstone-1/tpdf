#!/usr/bin/env python3
"""Generate the original MIT-licensed subset used by worker/editor tests.

uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py
uv run --with pypdf testdata/make_textedit_embedded.py --check before.pdf after.pdf
Writes a deterministic test font and an ignored PDF with the native UI fixture's
geometry. No installed or third-party font is read.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


def check(before, after):
    """Independent parser: only one operand changed; the entire font survived."""
    from pypdf import PdfReader
    from pypdf.generic import ContentStream, DictionaryObject, StreamObject

    def value(obj):
        obj = obj.get_object()
        if isinstance(obj, StreamObject):
            return ({str(k): value(v) for k, v in obj.items() if k not in ("/Length", "/Filter")}, obj.get_data())
        if isinstance(obj, DictionaryObject):
            return {str(k): value(v) for k, v in obj.items()}
        if isinstance(obj, list):
            return [value(v) for v in obj]
        return obj

    readers = [PdfReader(path) for path in (before, after)]
    assert all(len(reader.pages) == 1 for reader in readers), "wrong page count"
    pages = [reader.pages[0] for reader in readers]
    assert value(pages[0]["/Resources"]["/Font"]) == value(pages[1]["/Resources"]["/Font"]), "font changed"
    operations = [ContentStream(page["/Contents"], reader).operations
                  for page, reader in zip(pages, readers)]
    assert len(operations[0]) == len(operations[1]), "operator count changed"
    changes = [(old, new) for old, new in zip(*operations) if old != new]
    assert len(changes) == 1, "expected exactly one changed operand"
    old, new = changes[0]
    if old[1] == b"TJ":
        assert len(old[0]) == 1 and isinstance(old[0][0], list), "wrong source array"
        parts = old[0][0]
        assert all(isinstance(part, (str, int, float)) for part in parts), "invalid array item"
        assert "".join(part for part in parts if isinstance(part, str)) == "SYNTHETIC FIRST", "wrong source text"
        assert new == ([["EDITED FIRST"]], b"TJ"), "wrong replacement array"
    else:
        assert old == (["SYNTHETIC FIRST"], b"Tj"), "wrong source operand"
        assert new == (["EDITED FIRST"], b"Tj"), "wrong replacement operand"
    print("[PASS] independent parser: only target text operand changed; font dictionaries and program bytes preserved")


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--check":
        check(*sys.argv[2:])
        return
    if len(sys.argv) != 1:
        raise SystemExit("expected no arguments or --check before.pdf after.pdf")
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, DecodedStreamObject, NameObject, NumberObject

    data = make_font(characters="AB SYNTHETIC FIRST EDITED SECOND")
    (ROOT / "src-tauri/src/textedit/synthetic.ttf").write_bytes(data)
    target = ROOT / "testdata/textedit-embedded.pdf"
    pdf_round_trip(data, "ttf", target)
    writer = PdfWriter(clone_from=PdfReader(target))
    page = writer.pages[0]
    page[NameObject("/MediaBox")] = ArrayObject([NumberObject(v) for v in (0, 0, 300, 240)])
    font = page["/Resources"]["/Font"]["/F1"]
    font[NameObject("/LastChar")] = NumberObject(89)
    font[NameObject("/Widths")] = ArrayObject([NumberObject(600)] * 58)
    content = DecodedStreamObject()
    content.set_data(b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\n"
                     b"BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    print("[PASS] generated synthetic TrueType subset and editor PDF")


if __name__ == "__main__":
    main()
