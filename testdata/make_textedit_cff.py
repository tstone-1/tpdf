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
    if mode == "ligatures":
        names += ["f_l", "f_f", "f_i", "f_f_i"]
    if mode == "unicode":
        names += ["minus", "uni00A0", "quoteleft", "quoteright", "endash", "sterling"]
    if mode == "cyrillic":
        # A letter outside Latin-1 under its AGL uniXXXX name, as a brochure
        # with a Russian edition names its Cyrillic in every font's Differences.
        names += ["uni0410"]
    chars = {}
    for name in names:
        pen = T2CharStringPen(600, None)
        if name not in ("space", "uni00A0") or mode == "broken-space":
            left = -20 if (mode == "overhang" and name == "A") or name == "minus" else 0
            right = 620 if name == "minus" else 333 if name == "space" else 400
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
    if mode == "builtin-encoding":
        # The program's own encoding, as xdvipdfmx's TeX fonts carry one: the
        # printable ASCII names, with A and B swapped so that a reader using
        # StandardEncoding instead shows the wrong letters.
        encoding = [".notdef"] * 256
        for code in range(32, 127):
            encoding[code] = UV2AGL[code]
        encoding[0x41], encoding[0x42] = "B", "A"
        builder.font["CFF "].cff.topDictIndex[0].Encoding = encoding
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


def pdf(data, path, *, remap=False, to_unicode=False, unicode=False):
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
    original = "SYNTHETIC FIRST"
    if unicode:
        original = "SYNTHETIC \u2212\u00a0\u2018\u2019\u2013£" if to_unicode else "SYNTHETIC\u2013FIRST"
        codes.update({0x2212: 26, 0xa0: 27, 0x2018: 0x91, 0x2019: 0x92, 0x2013: 0x96, 0xa3: 0xa3})
        descriptor[NameObject("/FontBBox")] = num([-20, 0, 620, 700])
        font[NameObject("/FirstChar")] = NumberObject(26)
        font[NameObject("/LastChar")] = NumberObject(163)
        font[NameObject("/Widths")] = num([600] * 138)
        font[NameObject("/Encoding")] = writer._add_object(d(
            BaseEncoding=NameObject("/WinAnsiEncoding"),
            Differences=ArrayObject([NumberObject(26), NameObject("/minus")] + ([NameObject("/uni00A0")] if to_unicode else [])),
        ))
    if to_unicode:
        mapping = DecodedStreamObject()
        items = sorted(codes.items(), key=lambda item: item[1])
        blocks = []
        for offset in range(0, len(items), 100):
            batch = items[offset:offset + 100]
            entries = "".join(f"<{code:02X}> <{ch:04X}>\n" for ch, code in batch)
            blocks.append(f"{len(batch)} beginbfchar\n" + entries + "endbfchar ")
        mapping.set_data(("/CIDInit /ProcSet findresource begin 12 dict begin begincmap "
            "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def "
            "/CMapName /Adobe-Identity-UCS def /CMapType 2 def "
            "1 begincodespacerange <00> <FF> endcodespacerange "
            + "".join(blocks) + "endcmap CMapName currentdict /CMap defineresource pop end end").encode())
        font[NameObject("/ToUnicode")] = writer._add_object(mapping)
    page[NameObject("/Resources")] = d(Font=d(F1=writer._add_object(font)))
    content = DecodedStreamObject()
    if remap or to_unicode or unicode:
        first, second = (bytes(codes[ord(ch)] for ch in text).hex() for text in
                         [original, "SYNTHETIC SECOND"])
        content.set_data(f"1 Tw BT /F1 12 Tf 40 180 Td <{first}> Tj ET BT /F1 12 Tf 40 140 Td <{second}> Tj ET".encode())
    else:
        content.set_data(
            b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET"
        )
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(path)
    assert (
        " ".join(PdfReader(path).pages[0].extract_text().split())
        == " ".join((original + " SYNTHETIC SECOND").split())
    )


def pdf_ligatures(path):
    # The first ffi is three ordinary glyphs, the second is one ligature.
    # They extract identically but must have different advances and Tc counts.
    pdf(program("ligatures"), path)
    writer = PdfWriter(clone_from=path)
    page = writer.pages[0]
    font = page["/Resources"]["/Font"]["/F1"]
    font[NameObject("/FirstChar")] = NumberObject(28)
    font[NameObject("/Widths")] = ArrayObject([NumberObject(600)] * 99)
    font[NameObject("/Encoding")] = DictionaryObject({
        NameObject("/BaseEncoding"): NameObject("/WinAnsiEncoding"),
        NameObject("/Differences"): ArrayObject([NumberObject(28)] +
            [NameObject("/" + name) for name in ["f_l", "f_f", "f_i", "f_f_i"]]),
    })
    mapping = DecodedStreamObject()
    entries = "".join(f"<{code:02x}> <{code:04x}> " for code in range(32, 127))
    entries += "<1c> <0066006c> <1d> <00660066> <1e> <00660069> <1f> <006600660069>"
    mapping.set_data(("/CIDInit /ProcSet findresource begin 12 dict begin begincmap "
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def "
        "/CMapName /Adobe-Identity-UCS def /CMapType 2 def "
        "1 begincodespacerange <00> <FF> endcodespacerange "
        "99 beginbfchar " + entries + " endbfchar "
        "endcmap CMapName currentdict /CMap defineresource pop end end").encode())
    font[NameObject("/ToUnicode")] = writer._add_object(mapping)
    content = DecodedStreamObject()
    page["/Resources"][NameObject("/ColorSpace")] = DictionaryObject({NameObject("/RGB"): NameObject("/DeviceRGB")})
    first = b"SYNTHETIC ffi \x1f \x1e \x1c \x1d".hex()
    content.set_data((f"q /RGB CS .2 .4 .6 SCN 2 w 10 30 15 -20 re 35 10 -10 20 re B Q 1 Tc 1 Tw BT /F1 12 Tf 40 180 Td <{first}> Tj ET "
        "0 Tc 0 Tw BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET").encode())
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(path)
    assert " ".join(PdfReader(path).pages[0].extract_text().split()) == "SYNTHETIC ffi ffi fi fl ff SYNTHETIC SECOND"


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
        "builtin-encoding",
        "overhang",
        "unicode",
        "ligatures",
        "cyrillic",
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
    pdf_ligatures(args.output / "ligatures.pdf")
    pdf(program(), args.output / "synthetic.pdf")
    pdf(program(), args.output / "remapped.pdf", remap=True)
    pdf(program(), args.output / "remapped-unicode.pdf", remap=True, to_unicode=True)
    pdf(program("unicode"), args.output / "unicode.pdf", unicode=True)
    pdf(program("unicode"), args.output / "unicode-mapped.pdf", unicode=True, to_unicode=True)
    print("[PASS] generated original CFF programs and independently decoded the PDF")


if __name__ == "__main__":
    main()
