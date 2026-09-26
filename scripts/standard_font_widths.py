"""Generate src-tauri/src/textedit/fonts/standard.rs: advance widths of the
twelve Latin standard PDF fonts (ISO 32000-1 9.6.2.2) for printable WinAnsi
codes, the domain the text editor offers for standard Helvetica, and each
font's FontBBox; and for Symbol and ZapfDingbats, the widths of every code of
their built-in encodings and their boxes, which keep text in them read-only.

Widths come from two independent transcriptions of Adobe's Core 14 AFM
metrics that must agree on every value, or nothing is written: ReportLab's
table keyed by glyph name (through its own WinAnsiEncoding) and pdfminer.six's
keyed by character (through Python's cp1252). Boxes come from pdfminer.six's
table and the FontBBox line of the Adobe AFM files matplotlib ships, and are
their union: they differ for the oblique Helvetica and all four Courier styles,
and a box is only used as an upper bound on ink.

The output is passed through rustfmt, so `--check` compares against the file
as `cargo fmt` leaves it.

Usage:
    uv run --with reportlab --with pdfminer.six --with matplotlib python scripts/standard_font_widths.py
    uv run --with reportlab --with pdfminer.six --with matplotlib python scripts/standard_font_widths.py --check
"""

import pathlib
import subprocess
import sys

import matplotlib
from pdfminer.fontmetrics import FONT_METRICS
from reportlab.pdfbase import _fontdata

FONTS = [
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
]
CODES = list(range(32, 127)) + list(range(160, 256))
OUT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri/src/textedit/fonts/standard.rs"
AFM = pathlib.Path(matplotlib.get_data_path()) / "fonts" / "pdfcorefonts"


def character(code):
    # WinAnsi 0xA0 and 0xAD are the space and hyphen glyphs (ISO 32000-1 D.2).
    return {0xA0: " ", 0xAD: "-"}.get(code, bytes([code]).decode("cp1252"))


def widths(font):
    names = _fontdata.encodings["WinAnsiEncoding"]
    by_glyph = _fontdata.widthsByFontGlyph[font]
    by_char = FONT_METRICS[font][1]
    result = []
    for code in CODES:
        glyph = {0xA0: "space", 0xAD: "hyphen"}.get(code, names[code])
        a = by_glyph[glyph]
        b = by_char[character(code)]
        if a != b:
            sys.exit(f"[FAIL] {font} code {code}: ReportLab {a}, pdfminer {b}")
        result.append(a)
    return result


SYMBOLIC = [("Symbol", "SymbolEncoding"), ("ZapfDingbats", "ZapfDingbatsEncoding")]


def symbolic_widths(font, encoding):
    """Every code of the font's built-in encoding, 0 where it names no glyph.

    ReportLab's encoding and width tables against the glyph name and width the
    AFM file gives each code: both must name the same glyph with the same width
    for every code either encodes, or nothing is written."""
    names = _fontdata.encodings[encoding]
    by_glyph = _fontdata.widthsByFontGlyph[font]
    afm = {}
    for line in (AFM / f"{font}.afm").read_text(encoding="latin-1").splitlines():
        if line.startswith("C "):
            fields = dict(part.strip().split(" ", 1) for part in line.split(";") if part.strip())
            if int(fields["C"]) >= 0:
                afm[int(fields["C"])] = (fields["N"], int(fields["WX"]))
    result = []
    for code in range(256):
        name = names[code]
        if name is None and code not in afm:
            result.append(0)
            continue
        if name is None or code not in afm or afm[code] != (name, by_glyph[name]):
            sys.exit(f"[FAIL] {font} code {code}: ReportLab {name}, AFM {afm.get(code)}")
        result.append(by_glyph[name])
    return result


def box(font):
    a = [int(v) for v in FONT_METRICS[font][0]["FontBBox"]]
    lines = (AFM / f"{font}.afm").read_text(encoding="latin-1").splitlines()
    found = [line.split()[1:] for line in lines if line.startswith("FontBBox ")]
    if len(found) != 1:
        sys.exit(f"[FAIL] {font}: expected one FontBBox line in its AFM file")
    b = [int(v) for v in found[0]]
    # The box is only ever an upper bound on ink, so the union of the two is
    # sound whichever of them is right; a typo that shrinks one is covered by
    # the other. They do differ: pdfminer gives Helvetica-Oblique a left edge
    # of -171 where Adobe's own AFM says -170.
    if a != b:
        print(f"[NOTE] {font} FontBBox: pdfminer {a}, AFM {b}; using the union", file=sys.stderr)
    return [min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3])]


def render():
    lines = [
        "//! Advance widths of the twelve Latin standard fonts, in 1/1000 em, for",
        "//! WinAnsi codes 32..=126 then 160..=255, and each font's FontBBox. Generated",
        "//! by `scripts/standard_font_widths.py` from two independent transcriptions",
        "//! of Adobe's Core 14 metrics: the widths must agree, the boxes are their",
        "//! union. Do not edit by hand.",
        "",
        "#[cfg(test)]",
        "mod tests;",
        "",
        "pub(super) const FONTS: [(&[u8], [u16; 191]); 12] = [",
    ]
    for font in FONTS:
        values = widths(font)
        lines.append(f'    (b"{font}", [')
        for start in range(0, len(values), 16):
            chunk = ", ".join(str(v) for v in values[start:start + 16])
            lines.append(f"        {chunk},")
        lines.append("    ]),")
    lines.append("];")
    lines.append("")
    lines.append("// Left, bottom, right, top, in 1/1000 em; every glyph of the font lies inside.")
    lines.append("pub(super) const BOXES: [(&[u8], [i16; 4]); 12] = [")
    for font in FONTS:
        lines.append(f'    (b"{font}", [{", ".join(str(v) for v in box(font))}]),')
    lines.append("];")
    lines.append("")
    lines.append("// Symbol and ZapfDingbats in their built-in encodings: each code's width")
    lines.append("// (0 where the encoding names no glyph) and the font's box. Text in them is")
    lines.append("// only ever kept read-only.")
    lines.append("pub(super) const SYMBOLIC: [(&[u8], [u16; 256], [i16; 4]); 2] = [")
    for font, encoding in SYMBOLIC:
        values = symbolic_widths(font, encoding)
        lines.append(f'    (b"{font}", [')
        for start in range(0, len(values), 16):
            chunk = ", ".join(str(v) for v in values[start:start + 16])
            lines.append(f"        {chunk},")
        lines.append(f"    ], [{', '.join(str(v) for v in box(font))}]),")
    lines.append("];")
    source = "\n".join(lines) + "\n"
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2021", "--emit", "stdout"],
        input=source.encode(),
        capture_output=True,
        check=True,
    ).stdout.decode()
    return formatted.replace("\r\n", "\n")


def main():
    text = render()
    if "--check" in sys.argv:
        if OUT.read_text(encoding="utf-8") != text:
            sys.exit(f"[FAIL] {OUT} is stale; regenerate it")
        print(f"[OK] {OUT.name} matches both sources")
        return
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"[OK] wrote {OUT.name}: {len(FONTS)} fonts x {len(CODES)} codes and boxes, "
          f"{len(SYMBOLIC)} symbolic fonts x 256 codes")


main()
