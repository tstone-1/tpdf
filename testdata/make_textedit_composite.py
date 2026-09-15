#!/usr/bin/env python3
"""Create a synthetic Identity-H/CIDFontType2 editor fixture from our original font.

uv run --with fonttools --with pypdf testdata/make_textedit_composite.py <output.pdf>
Add --ligatures for mixed separate-letter and two-byte ligature text. Read back
with text-edit-probe and scripts/text_edit_pdfkit.swift using --cid-ligatures.
The geometric glyphs are MIT-licensed repository material, never installed fonts.
"""
import argparse
import io
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


def create(target, reflected=False, tight_clip=False, ligatures=False):
    from fontTools.ttLib import TTFont
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject,
                              NameObject, NumberObject, TextStringObject)

    def dictionary(**values):
        return DictionaryObject({NameObject("/" + key): value for key, value in values.items()})

    program = (ROOT / "src-tauri/src/textedit/synthetic.ttf").read_bytes()
    if ligatures:
        program = make_font(characters="".join(chr(ch) for ch in range(32, 127)) + "\u00a0\u00a1\u00a2\u00a3")
    if tight_clip:
        from fontTools.pens.ttGlyphPen import TTGlyphPen
        variant = TTFont(io.BytesIO(program), recalcTimestamp=False)
        # B is taller than any source glyph, with fractional outline coordinates;
        # D descends below the baseline and appears in the replacement's first line.
        for name, transform in [("B", (1, 0, 0, 18729 / 16384, 0, 0)),
                                ("D", (1, 0, 0, 1, 0, -100))]:
            pen = TTGlyphPen(variant.getGlyphSet())
            pen.addComponent("A", transform)
            variant["glyf"][name] = pen.glyph()
        result = io.BytesIO()
        variant.save(result)
        program = result.getvalue()
    parsed = TTFont(io.BytesIO(program))
    codes = {chr(ch): parsed.getGlyphID(glyph) for ch, glyph in parsed.getBestCmap().items()}
    if ligatures:
        for ch, text in [("\u00a0", "ff"), ("\u00a1", "fi"), ("\u00a2", "fl"), ("\u00a3", "ffi")]:
            codes[text] = codes.pop(ch)
    assert all(all(32 <= ord(ch) <= 126 for ch in text) for text in codes)
    target.parent.mkdir(parents=True, exist_ok=True)
    assert pdf_round_trip(program, "ttf", target) == program
    writer = PdfWriter(clone_from=PdfReader(target))
    page = writer.pages[0]
    page[NameObject("/MediaBox")] = ArrayObject([NumberObject(v) for v in (0, 0, 300, 240)])
    original = page["/Resources"]["/Font"]["/F1"]
    descriptor = original["/FontDescriptor"]
    descriptor[NameObject("/Flags")] = NumberObject(4)
    if tight_clip:
        descriptor[NameObject("/FontBBox")] = ArrayObject([
            NumberObject(getattr(parsed["head"], key))
            for key in ("xMin", "yMin", "xMax", "yMax")
        ])
    child = dictionary(Type=NameObject("/Font"), Subtype=NameObject("/CIDFontType2"),
        BaseFont=original["/BaseFont"], FontDescriptor=writer._add_object(descriptor),
        CIDToGIDMap=NameObject("/Identity"), DW=NumberObject(600),
        CIDSystemInfo=dictionary(Registry=TextStringObject("Adobe"),
            Ordering=TextStringObject("Identity"), Supplement=NumberObject(0)))
    mapping = DecodedStreamObject()
    entries = "\n".join(f"<{code:04X}> <{text.encode('utf-16-be').hex()}>" for text, code in sorted(codes.items()))
    mapping.set_data(("/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
        "1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        f"{len(codes)} beginbfchar\n{entries}\nendbfchar\n"
        "endcmap CMapName currentdict /CMap defineresource pop end end").encode("ascii"))
    composite = dictionary(Type=NameObject("/Font"), Subtype=NameObject("/Type0"),
        BaseFont=original["/BaseFont"], Encoding=NameObject("/Identity-H"),
        DescendantFonts=ArrayObject([writer._add_object(child)]), ToUnicode=writer._add_object(mapping))
    page["/Resources"]["/Font"][NameObject("/F1")] = writer._add_object(composite)
    content = DecodedStreamObject()
    shows = []
    first = "SYNTHETIC ffi ffi fi fl ff" if ligatures else "SYNTHETIC FIRST"
    for text, y in [(first, 180), ("SYNTHETIC SECOND", 140)]:
        # The first ffi is three glyphs; the second is one. They must retain
        # different advances despite extracting to the same three characters.
        parts = [*"SYNTHETIC ffi ", "ffi", " ", "fi", " ", "fl", " ", "ff"] if ligatures and y == 180 else list(text)
        raw = b"".join(codes[ch].to_bytes(2, "big") for ch in parts).hex()
        position = f"1 0 0 -1 40 {240-y} Tm" if reflected else f"40 {y} Td"
        shows.append(f"BT /F1 12 Tf {position} <{raw}> Tj ET")
    if reflected:
        # Browser-style page reflection and scale, with a rectangular clip.
        # Exact binary scales keep this fixture focused on geometry, independent
        # of decimal serialization. The resulting text is at the same positions.
        clip = "0 200 1200 760" if tight_clip else "0 0 1200 960"
        shows = [f".25 0 0 -.25 0 240 cm q {clip} re W* n q 4 0 0 4 0 0 cm", *shows, "Q Q"]
    elif tight_clip:
        shows = ["q 0 0 300 190 re W n", *shows, "Q"]
    content.set_data("\n".join(shows).encode("ascii"))
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    reader = PdfReader(target)
    assert " ".join(reader.pages[0].extract_text().split()) == first + " SYNTHETIC SECOND"
    assert reader.pages[0]["/Resources"]["/Font"]["/F1"]["/DescendantFonts"][0].get_object()["/FontDescriptor"]["/FontFile2"].get_data() == program
    print("[PASS] generated Identity-H fixture; independent text and exact font readback agree")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--reflected", action="store_true", help="pair reflected page/text matrices with a clip")
    parser.add_argument("--tight-clip", action="store_true", help="clip inside the full-em box with taller and descending replacement glyphs")
    parser.add_argument("--ligatures", action="store_true", help="mix separate letters with exact ToUnicode ligature sequences")
    args = parser.parse_args()
    create(args.output, args.reflected, args.tight_clip, args.ligatures)
