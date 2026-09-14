#!/usr/bin/env python3
"""Original geometric CFF fixtures, no installed or third-party font outlines.

uv run --with fonttools --with pypdf testdata/make_textedit_cff.py <output-dir>
--rust-fixtures writes the original programs used by Rust tests into the tree.
The PDF deliberately uses WinAnsi while CFF uses its independent default encoding.
"""

import argparse
import io
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.agl import UV2AGL
from pypdf import PdfWriter, PdfReader
from pypdf.generic import (
    DictionaryObject,
    NameObject,
    NumberObject,
    ArrayObject,
    DecodedStreamObject,
)


def program(mode="normal"):
    names = [".notdef"] + [UV2AGL[code] for code in range(32, 127)]
    chars = {}
    for name in names:
        pen = T2CharStringPen(600, None)
        if name != "space" or mode == "broken-space":
            left = -20 if mode == "overhang" and name == "A" else 0
            right = 333 if name == "space" else 400
            pen.moveTo((left, 0))
            pen.lineTo((left, 700))
            pen.lineTo((right, 700))
            pen.lineTo((right, 0))
            pen.closePath()
        chars[name] = pen.getCharString()
    if mode == "missing-A":
        names.remove("A")
        del chars["A"]
    builder = FontBuilder(1000, isTTF=False)
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap(
        {code: UV2AGL[code] for code in range(32, 127) if UV2AGL[code] in names}
    )
    builder.setupHorizontalMetrics({name: (600, 0) for name in names})
    info = {}
    if mode == "matrix":
        info["FontMatrix"] = [0.002, 0, 0, 0.002, 0, 0]
    if mode == "paint":
        info["PaintType"] = 2
    if mode == "charstring":
        info["CharstringType"] = 1
    if mode == "preview-only":
        info["PostScript"] = "/FSType 4 def"
    if mode == "editable-rights":
        info["PostScript"] = "/FSType 8 def /OrigFontType /OpenType def"
    if mode == "unknown-postscript":
        info["PostScript"] = "/FontMatrix [2 0 0 2 0 0] def"
    builder.setupCFF("TPDFSyntheticCFF", info, chars, {})
    if mode == "expert-encoding":
        builder.font["CFF "].cff.topDictIndex[0].Encoding = "ExpertEncoding"
    data = builder.font.getTableData("CFF ")
    if mode == "broken-space":
        # Corrupt after serialization: fontTools otherwise normalizes an invalid
        # suffix away while calculating the font bbox. Keep all INDEX offsets.
        from fontTools.cffLib import CFFFontSet

        parsed = CFFFontSet()
        parsed.decompile(io.BytesIO(data), None)
        encoded = parsed[parsed.fontNames[0]].CharStrings["space"].bytecode
        assert encoded[-1] == 14 and data.count(encoded) == 1
        data = data.replace(encoded, encoded[:-1] + b"\x0c")
    return data


def pdf(data, path, *, remap=False, to_unicode=False):
    def d(**kwargs):
        return DictionaryObject({NameObject("/" + k): v for k, v in kwargs.items()})

    def num(values):
        return ArrayObject([NumberObject(v) for v in values])

    writer = PdfWriter()
    page = writer.add_blank_page(width=300, height=240)
    stream = DecodedStreamObject()
    stream.set_data(data)
    stream[NameObject("/Subtype")] = NameObject("/Type1C")
    descriptor = d(
        Type=NameObject("/FontDescriptor"),
        FontName=NameObject("/TPDFSyntheticCFF"),
        Flags=NumberObject(32),
        FontBBox=num([0, 0, 400, 700]),
        ItalicAngle=NumberObject(0),
        Ascent=NumberObject(800),
        Descent=NumberObject(-200),
        CapHeight=NumberObject(700),
        StemV=NumberObject(100),
        FontFile3=writer._add_object(stream),
    )
    font = d(
        Type=NameObject("/Font"),
        Subtype=NameObject("/Type1"),
        BaseFont=NameObject("/TPDFSyntheticCFF"),
        Encoding=NameObject("/WinAnsiEncoding"),
        FirstChar=NumberObject(32),
        LastChar=NumberObject(126),
        Widths=num([600] * 95),
        FontDescriptor=writer._add_object(descriptor),
    )
    codes = {code: code for code in range(32, 127)}
    if remap:
        codes[32], codes[83] = 83, 32
        font[NameObject("/Encoding")] = writer._add_object(d(
            Type=NameObject("/Encoding"), BaseEncoding=NameObject("/WinAnsiEncoding"),
            Differences=ArrayObject([NumberObject(32), NameObject("/S"), NumberObject(83), NameObject("/space")]),
        ))
    if to_unicode:
        mapping = DecodedStreamObject()
        entries = "".join(f"<{code:02X}> <{ch:04X}>\n" for ch, code in sorted(codes.items(), key=lambda item: item[1]))
        mapping.set_data(("/CIDInit /ProcSet findresource begin 12 dict begin begincmap "
            "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def "
            "/CMapName /Adobe-Identity-UCS def /CMapType 2 def "
            "1 begincodespacerange <00> <FF> endcodespacerange "
            "95 beginbfchar\n" + entries + "endbfchar endcmap CMapName currentdict /CMap defineresource pop end end").encode())
        font[NameObject("/ToUnicode")] = writer._add_object(mapping)
    page[NameObject("/Resources")] = d(Font=d(F1=writer._add_object(font)))
    content = DecodedStreamObject()
    if remap or to_unicode:
        first, second = (bytes(codes[code] for code in text).hex() for text in
                         [b"SYNTHETIC FIRST", b"SYNTHETIC SECOND"])
        content.set_data(f"1 Tw BT /F1 12 Tf 40 180 Td <{first}> Tj ET BT /F1 12 Tf 40 140 Td <{second}> Tj ET".encode())
    else:
        content.set_data(
            b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET"
        )
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(path)
    assert (
        " ".join(PdfReader(path).pages[0].extract_text().split())
        == "SYNTHETIC FIRST SYNTHETIC SECOND"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--rust-fixtures", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    modes = [
        "normal",
        "broken-space",
        "missing-A",
        "matrix",
        "paint",
        "charstring",
        "preview-only",
        "editable-rights",
        "unknown-postscript",
        "expert-encoding",
        "overhang",
    ]
    for mode in modes:
        data = program(mode)
        (args.output / (mode + ".cff")).write_bytes(data)
        if args.rust_fixtures:
            dest = (
                Path(__file__).resolve().parents[1]
                / "src-tauri/src/textedit/fonts/cff/fixtures"
            )
            dest.mkdir(exist_ok=True)
            (dest / (mode + ".cff")).write_bytes(data)
    pdf(program(), args.output / "synthetic.pdf")
    pdf(program(), args.output / "remapped.pdf", remap=True)
    pdf(program(), args.output / "remapped-unicode.pdf", remap=True, to_unicode=True)
    print("[PASS] generated original CFF programs and independently decoded the PDF")


if __name__ == "__main__":
    main()
