#!/usr/bin/env python3
"""Exercise real extraction on mixed text matrices and every page rotation.

  uv run --with pypdf scripts/text_direction_check.py src-tauri/target/debug/examples/text-probe

The expected directions come from authored matrices, not the engine's angles.
Each invocation loads PDFium in a separate process. No input fixture is retained.
"""

import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from pypdf import PdfWriter
from pypdf.generic import ArrayObject, DecodedStreamObject, DictionaryObject, NameObject, NumberObject


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("probe", type=Path)
    args = parser.parse_args()
    writer = PdfWriter()
    font = writer._add_object(DictionaryObject({
        NameObject("/Type"): NameObject("/Font"),
        NameObject("/Subtype"): NameObject("/Type1"),
        NameObject("/BaseFont"): NameObject("/Helvetica"),
    }))
    labels = ["DIRECTIONZERO", "DIRECTIONONE", "DIRECTIONTWO", "DIRECTIONTHREE"]
    matrices = ["1 0 0 1 80 500", "0 -1 1 0 200 500", "-1 0 0 -1 380 300", "0 1 -1 0 500 120"]
    for turn in range(4):
        page = writer.add_blank_page(600, 800)
        page[NameObject("/Rotate")] = NumberObject(turn * 90)
        page[NameObject("/CropBox")] = ArrayObject([NumberObject(n) for n in [10, 20, 590, 790]])
        page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
        stream = DecodedStreamObject()
        stream.set_data("\n".join(f"BT /F1 12 Tf {matrix} Tm ({label}) Tj ET" for matrix, label in zip(matrices, labels)).encode("ascii"))
        page[NameObject("/Contents")] = writer._add_object(stream)
    # Upright-only control proves that omitted metadata still means upright.
    page = writer.add_blank_page(600, 800)
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): font})})
    stream = DecodedStreamObject()
    stream.set_data(b"BT /F1 12 Tf 1 0 0 1 80 500 Tm (UPRIGHT) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(stream)
    # A stand-in A maps to one non-BMP scalar; PDFium exposes two UTF-16 units.
    # The following B must still address the second entry in all three arrays.
    mapping = DecodedStreamObject()
    mapping.set_data(b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
                     b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
                     b"/CMapName /Synthetic def /CMapType 2 def\n"
                     b"1 begincodespacerange <00> <FF> endcodespacerange\n"
                     b"2 beginbfchar <41> <D840DC00> <42> <0042> endbfchar\n"
                     b"endcmap CMapName currentdict /CMap defineresource pop end end")
    astral_font = writer._add_object(DictionaryObject({
        **font.get_object(), NameObject("/ToUnicode"): writer._add_object(mapping),
    }))
    page = writer.add_blank_page(600, 800)
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): astral_font})})
    stream = DecodedStreamObject()
    stream.set_data(b"BT /F1 12 Tf 0 1 -1 0 500 120 Tm (AB) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(stream)
    with tempfile.TemporaryDirectory(prefix="tpdf-directions-") as room:
        source = Path(room) / "synthetic.pdf"
        writer.write(source)
        for turn in range(6):
            result = subprocess.run([str(args.probe.resolve()), str(source), "--page", str(turn), "--mode", "json"],
                                    text=True, capture_output=True, check=True, timeout=30)
            text = json.loads(result.stdout)
            decoded = "".join(chr(code) for code in text["codes"])
            assert len(text["boxes"]) == len(text["codes"]) * 4
            if turn == 4:
                assert decoded == "UPRIGHT" and "char_turns" not in text
                continue
            if turn == 5:
                assert text["codes"] == [0x20000, 66]
                assert text["char_turns"] == [3, 3]
                continue
            assert text["quarter_turns"] == turn
            assert len(text["char_turns"]) == len(text["codes"])
            for direction, label in enumerate(labels):
                assert decoded.count(label) == 1, (turn, label, decoded)
                at = decoded.index(label)
                assert text["char_turns"][at:at + len(label)] == [direction] * len(label), (turn, label)
    print("[OK] 16 text/page direction combinations; scalar indices align; upright metadata omitted")


if __name__ == "__main__":
    main()
