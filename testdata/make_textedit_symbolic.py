#!/usr/bin/env python3
"""Create a synthetic symbolic-font editing fixture with LibreOffice-style codes.

uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-symbolic/fixture.pdf
Append --ranges to use Quartz-style scalar ToUnicode ranges.
Original geometric outlines, MIT like this repository; no installed font is read.
"""
from io import BytesIO
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


def main():
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, DecodedStreamObject, NameObject, NumberObject

    if len(sys.argv) not in (2, 3) or (len(sys.argv) == 3 and sys.argv[2] != "--ranges"):
        raise SystemExit("expected output PDF path [--ranges]")
    ranges = len(sys.argv) == 3
    target = Path(sys.argv[1])
    target.parent.mkdir(parents=True, exist_ok=True)
    alphabet = "SYNTHEIC FRODAB"
    assert len(set(alphabet)) == len(alphabet)
    face = TTFont(BytesIO(make_font(characters=alphabet)))
    original = face.getBestCmap()
    # Sorting the range variant makes the edited letters use expanded ranges,
    # rather than exercising only singleton entries in the end-to-end check.
    codes = {ch: index + 1 for index, ch in enumerate(sorted(alphabet) if ranges else alphabet)}
    cmap = CmapSubtable.newSubtable(0)
    cmap.platformID, cmap.platEncID, cmap.language = 1, 0, 0
    cmap.cmap = {code: original[ord(ch)] for ch, code in codes.items()}
    face["cmap"].tables = [cmap]
    face.sfntVersion = "true"
    del face["OS/2"]
    program = BytesIO()
    face.save(program)
    pdf_round_trip(program.getvalue(), "ttf", target)
    writer = PdfWriter(clone_from=PdfReader(target))
    page = writer.pages[0]
    page[NameObject("/MediaBox")] = ArrayObject([NumberObject(v) for v in (0, 0, 300, 240)])
    font = page["/Resources"]["/Font"]["/F1"]
    del font["/Encoding"]
    font[NameObject("/FirstChar")] = NumberObject(0)
    font[NameObject("/LastChar")] = NumberObject(len(codes))
    font[NameObject("/Widths")] = ArrayObject([NumberObject(0)] + [NumberObject(600)] * len(codes))
    font["/FontDescriptor"][NameObject("/Flags")] = NumberObject(4)
    entries = [[code, code, ord(ch)] for ch, code in codes.items()]
    if ranges:
        grouped = []
        for first, last, target_code in entries:
            if grouped and first == grouped[-1][1] + 1 and target_code == grouped[-1][2] + first - grouped[-1][0]:
                grouped[-1][1] = last
            else:
                grouped.append([first, last, target_code])
        entries = grouped
    kind = "bfrange" if ranges else "bfchar"
    # Use one entry per line, as emitted by Quartz and read by PDFKit/pypdf.
    body = "".join(f"<{first:02X}> " + (f"<{last:02X}> " if ranges else "") +
                   f"<{target_code:04X}>\n" for first, last, target_code in entries)
    mapping = DecodedStreamObject()
    mapping.set_data(("/CIDInit/ProcSet findresource begin\n12 dict begin\nbegincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n"
        "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n"
        f"{len(entries)} begin{kind}\n" + body +
        f"end{kind}\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n").encode())
    font[NameObject("/ToUnicode")] = writer._add_object(mapping)
    content = DecodedStreamObject()
    def encoded(text):
        return "<" + bytes(codes[ch] for ch in text).hex() + ">"
    # Mix Tj and TJ so both paths must decode and re-encode the symbolic codes.
    content.set_data((f"BT /F1 12 Tf 40 180 Td [{encoded('SYNTHETIC ')} 20 {encoded('FIRST')}] TJ ET\n"
                      f"BT /F1 12 Tf 40 140 Td {encoded('SYNTHETIC SECOND')} Tj ET").encode())
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    print("[PASS] generated symbolic TrueType fixture with distinct PDF and Unicode codes")


if __name__ == "__main__":
    main()
