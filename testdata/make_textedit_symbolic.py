#!/usr/bin/env python3
"""Create a synthetic symbolic-font editing fixture with LibreOffice-style codes.

uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-symbolic/fixture.pdf
Append --ranges to use Quartz-style scalar ToUnicode ranges.
Append --spacing -0.005 to retain character spacing around the first line.
Append --intent Perceptual to retain both ri and ExtGState RI settings.
Append --image to retain an opaque RGB image alongside the text.
Append --dash to edit an en dash through its original font code.
Append --tagged-indirect for referenced tagging metadata and omitted element Type.
Use --unit-font --word-code space --word-spacing 12.112 for a tab-sized gap;
the font size is 1 and Tm supplies the 12pt scale. PDFKit readback uses --wide-spacing;
the native fixture phase is textedit-wide-spacing (the gap forms geometric columns).
Original geometric outlines, MIT like this repository; no installed font is read.
"""
from io import BytesIO
from pathlib import Path
import argparse
import math
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


def main():
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, DecodedStreamObject, DictionaryObject, NameObject, NumberObject, BooleanObject

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--ranges", action="store_true")
    parser.add_argument("--spacing", type=float, default=0.)
    parser.add_argument("--word-spacing", type=float, default=0.)
    parser.add_argument("--tagged-indirect", action="store_true")
    parser.add_argument("--unit-font", action="store_true", help="use Tf=1 and a 12x text matrix")
    parser.add_argument("--word-code", choices=["space", "letter"],
                        help="map a space or S to PDF byte 32; default has no code 32")
    parser.add_argument("--intent", choices=["AbsoluteColorimetric", "RelativeColorimetric", "Saturation", "Perceptual"])
    parser.add_argument("--image", action="store_true")
    parser.add_argument("--print-state", action="store_true", help="preserve explicit mask defaults and scoped overprint settings")
    parser.add_argument("--dash", action="store_true")
    args = parser.parse_args()
    small_spacing = 0.25 if args.unit_font else 3
    if not math.isfinite(args.spacing) or abs(args.spacing) > small_spacing:
        parser.error(f"spacing must be finite and within {-small_spacing}..{small_spacing}")
    if not math.isfinite(args.word_spacing) or not -small_spacing <= args.word_spacing <= 12.5:
        parser.error(f"word spacing must be finite and within {-small_spacing}..12.5")
    ranges, target = args.ranges, args.output
    target.parent.mkdir(parents=True, exist_ok=True)
    alphabet = "SYNTHEIC FRODAB" + ("\u2013" if args.dash else "")
    assert len(set(alphabet)) == len(alphabet)
    face = TTFont(BytesIO(make_font(characters=alphabet)))
    original = face.getBestCmap()
    # Sorting the range variant makes the edited letters use expanded ranges,
    # rather than exercising only singleton entries in the end-to-end check.
    codes = {ch: index + 1 for index, ch in enumerate(sorted(alphabet) if ranges else alphabet)}
    if args.word_code:
        codes[" " if args.word_code == "space" else "S"] = 32
        codes = dict(sorted(codes.items(), key=lambda item: item[1]))
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
    font[NameObject("/LastChar")] = NumberObject(max(codes.values()))
    font[NameObject("/Widths")] = ArrayObject([NumberObject(600 if code in codes.values() else 0) for code in range(max(codes.values()) + 1)])
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
    prefix = "SYNTHETIC" + ("\u2013" if args.dash else " ")
    def position(y):
        return f"/F1 1 Tf 12 0 0 12 40 {y} Tm" if args.unit_font else f"/F1 12 Tf 40 {y} Td"
    first = f"BT {position(180)} [{encoded(prefix)} 20 {encoded('FIRST')}] TJ ET"
    if args.word_spacing:
        first = f"q {args.word_spacing:g} Tw {first} Q"
    if args.spacing:
        first = f"q {args.spacing:g} Tc {first} Q"
    if args.intent:
        page["/Resources"][NameObject("/ExtGState")] = DictionaryObject({
            NameObject("/IntentState"): writer._add_object(DictionaryObject({
                NameObject("/RI"): NameObject("/" + args.intent),
            })),
        })
        first = f"/{args.intent} ri q /IntentState gs {first} Q"
    if args.print_state:
        states = page["/Resources"].setdefault(NameObject("/ExtGState"), DictionaryObject())
        for name, overprint, mode in [("PrintOn", True, 0), ("PrintOff", False, 1)]:
            states[NameObject("/" + name)] = writer._add_object(DictionaryObject({
                NameObject("/Type"): NameObject("/ExtGState"),
                NameObject("/BM"): NameObject("/Normal"),
                NameObject("/ca"): NumberObject(1), NameObject("/CA"): NumberObject(1),
                NameObject("/OP"): BooleanObject(overprint),
                NameObject("/op"): BooleanObject(overprint),
                NameObject("/OPM"): NumberObject(mode),
                NameObject("/SA"): BooleanObject(True),
                NameObject("/AIS"): BooleanObject(False),
                NameObject("/SMask"): NameObject("/None"),
            }))
        first = f"/PrintOff gs q /PrintOn gs {first} Q"
    if args.image:
        image = DecodedStreamObject()
        image.update({NameObject(k): value for k, value in {
            "/Type": NameObject("/XObject"), "/Subtype": NameObject("/Image"),
            "/Width": NumberObject(841), "/Height": NumberObject(141),
            "/ColorSpace": NameObject("/DeviceRGB"), "/BitsPerComponent": NumberObject(8),
        }.items()})
        image.set_data(bytes(channel for y in range(141) for x in range(841)
                             for channel in (x % 256, y % 256, 90)))
        page["/Resources"][NameObject("/XObject")] = DictionaryObject({
            NameObject("/Image1"): writer._add_object(image.flate_encode()),
        })
        first = "q 168.2 0 0 28.2 40 40 cm /Image1 Do Q\n" + first
    second = f"BT {position(140)} {encoded('SYNTHETIC SECOND')} Tj ET"
    if args.tagged_indirect:
        root, document = DictionaryObject(), DictionaryObject()
        root_ref, document_ref = writer._add_object(root), writer._add_object(document)
        attributes = writer._add_object(DictionaryObject({
            NameObject("/O"): NameObject("/Layout"),
            NameObject("/Placement"): NameObject("/Block"),
        }))
        paragraphs = ArrayObject()
        for mcid in range(2):
            # Type is optional on structure elements; S and P are required.
            paragraphs.append(writer._add_object(DictionaryObject({
                NameObject("/S"): NameObject("/Standard"),
                NameObject("/P"): document_ref,
                NameObject("/Pg"): page.indirect_reference,
                NameObject("/K"): NumberObject(mcid),
                NameObject("/A"): attributes,
            })))
        document.update({NameObject("/S"): NameObject("/Document"),
                         NameObject("/P"): root_ref, NameObject("/K"): paragraphs})
        nums = writer._add_object(ArrayObject([NumberObject(0), paragraphs]))
        parents = writer._add_object(DictionaryObject({NameObject("/Nums"): nums}))
        roles = writer._add_object(DictionaryObject({NameObject("/Standard"): NameObject("/P")}))
        root.update({NameObject("/Type"): NameObject("/StructTreeRoot"),
                     NameObject("/K"): document_ref, NameObject("/RoleMap"): roles,
                     NameObject("/ParentTree"): parents})
        writer._root_object[NameObject("/StructTreeRoot")] = root_ref
        writer._root_object[NameObject("/MarkInfo")] = DictionaryObject({NameObject("/Marked"): BooleanObject(True)})
        page[NameObject("/StructParents")] = NumberObject(0)
        first = f"/Standard << /MCID 0 >> BDC {first} EMC"
        second = f"/Standard << /MCID 1 >> BDC {second} EMC"
    content.set_data((first + "\n" + second).encode())
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    print("[PASS] generated symbolic TrueType fixture with distinct PDF and Unicode codes")


if __name__ == "__main__":
    main()
