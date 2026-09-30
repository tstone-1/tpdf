#!/usr/bin/env python3
"""Generate the original fonts that stand in for an *installed* font in tests.

uv run --with fonttools testdata/make_installed_fonts.py
uv run --with fonttools testdata/make_installed_fonts.py --check

Writes into src-tauri/src/textedit/fonts/installed/. Every outline is an
original rectangle built here; no installed or third-party font is read, and the
names are made up. The fonts share one PostScript name, TPDFInstalledSans, on
purpose: what the editor must tell apart is a real copy from a wrong one that
carries the right name.

  full.ttf        the "installed" font: printable ASCII plus a few characters
                  beyond Latin-1, at 2048 units per em, so every width scales
                  to a fraction of a thousandth and a test can sit either side
                  of the one-unit tolerance
  subset.ttf      the document's subset of it: only the characters of
                  "TITLE SECOND", same outlines and advances, a (3,1) cmap
  sparse.ttf      a copy missing D, a character the document's subset has
  collection.ttc  face 0 is another font (TPDFOtherSans), face 1 is full.ttf
  cff.otf         the same font with CFF outlines, U+00A0 drawn by the space
                  glyph as many OpenType fonts draw it
  cff2048.otf     cff.otf drawn at 2048 units per em, a CFF FontMatrix the
                  editor's CID-keyed CFF reader refuses
  subset.cff      the document's subset of cff.otf as a bare Type1C program,
                  glyphs named as ISO 32000-1 Annex D names them, the way a
                  producer embeds an OpenType font in a simple font
  colour.ttf      full.ttf with an empty COLR/CPAL pair: a colour font

--check regenerates in memory and compares byte for byte, so a hand edit or a
fontTools release that changes the output is a failure rather than a drift.
"""
from __future__ import annotations

import argparse
import io
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "src-tauri" / "src" / "textedit" / "fonts" / "installed"
NAME = "TPDFInstalledSans"
FULL = "".join(chr(c) for c in range(32, 127)) + "éüßΩ€"
SUBSET = "TITLE SECOND"
UPEM = 2048


def advance(ch: str) -> int:
    """A different advance per character, in font units."""
    return 512 if ch == " " else 900 + (ord(ch) * 37) % 400


def build(characters: str, *, ps_name: str = NAME, cff: bool = False,
          adobe_names: bool = False, cff_upem: int = 1000) -> bytes:
    from fontTools.agl import UV2AGL
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.t2CharStringPen import T2CharStringPen
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    upem = cff_upem if cff else UPEM
    scale = upem / UPEM
    chars = sorted(set(characters) - {" "})
    names = {ch: UV2AGL[ord(ch)] if adobe_names else f"uni{ord(ch):04X}" for ch in chars}
    order = [".notdef", "space", *names.values()]
    builder = FontBuilder(upem, isTTF=not cff)
    builder.setupGlyphOrder(order)
    shared = {0xA0: "space"} if cff and not adobe_names else {}
    builder.setupCharacterMap({32: "space", **shared,
                               **{ord(ch): name for ch, name in names.items()}})
    widths = {".notdef": round(1000 * scale), "space": round(advance(" ") * scale)}
    glyphs = {}
    for name in order:
        ch = next((c for c, n in names.items() if n == name), None)
        width = round(advance(ch) * scale) if ch else widths[name]
        widths[name] = width
        pen = T2CharStringPen(width, None) if cff else TTGlyphPen(None)
        if name != "space":
            # One rectangle per glyph, its height and inset set by the code
            # point so no two glyphs draw the same.
            code = ord(ch) if ch else 0
            left = round((40 + code % 60) * scale)
            right = max(left + 10, width - left)
            top = round((900 + (code * 13) % 500) * scale)
            pen.moveTo((left, 0))
            pen.lineTo((left, top))
            pen.lineTo((right, top))
            pen.lineTo((right, 0))
            pen.closePath()
        glyphs[name] = pen.getCharString() if cff else pen.glyph()
    if cff:
        # Hint values only, so that the Private DICT is not empty: an offset
        # to an empty dict reads the same wherever it points.
        private = {"BlueValues": [-12, 0, 700, 712], "StdHW": 60, "StdVW": 80}
        builder.setupCFF(ps_name, {"FullName": "TPDF Installed Sans"}, glyphs, private)
    else:
        builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name: (widths[name], 0) for name in order})
    builder.setupHorizontalHeader(ascent=round(1800 * scale), descent=round(-400 * scale))
    family = "TPDF Other Sans" if ps_name != NAME else "TPDF Installed Sans"
    builder.setupNameTable({"familyName": family, "styleName": "Regular",
                            "uniqueFontIdentifier": f"{ps_name}-1",
                            "fullName": family, "psName": ps_name})
    builder.setupOS2(sTypoAscender=round(1800 * scale), sTypoDescender=round(-400 * scale),
                     usWinAscent=round(1800 * scale), usWinDescent=round(400 * scale), fsType=0)
    builder.setupPost()
    builder.font["head"].created = builder.font["head"].modified = 3800000000
    output = io.BytesIO()
    builder.save(output)
    return output.getvalue()


def collection(first: bytes, second: bytes) -> bytes:
    from fontTools.ttLib import TTCollection, TTFont

    fonts = TTCollection()
    fonts.fonts = [TTFont(io.BytesIO(data), recalcTimestamp=False) for data in (first, second)]
    output = io.BytesIO()
    fonts.save(output, shareTables=False)
    return output.getvalue()


def colour(data: bytes) -> bytes:
    from fontTools.ttLib import TTFont, newTable
    from fontTools.ttLib.tables.C_P_A_L_ import Color

    font = TTFont(io.BytesIO(data), recalcTimestamp=False)
    palette = newTable("CPAL")
    palette.version, palette.numPaletteEntries = 0, 1
    palette.palettes = [[Color(red=0, green=0, blue=0, alpha=255)]]
    layers = newTable("COLR")
    layers.version, layers.ColorLayers = 0, {}
    font["CPAL"], font["COLR"] = palette, layers
    output = io.BytesIO()
    font.save(output)
    return output.getvalue()


def bare_cff(data: bytes) -> bytes:
    """The CFF table of an OpenType font, as a PDF's FontFile3 carries it."""
    from fontTools.ttLib import TTFont

    return TTFont(io.BytesIO(data)).reader["CFF "]


def generate() -> dict[str, bytes]:
    full = build(FULL)
    return {
        "full.ttf": full,
        "subset.ttf": build(SUBSET),
        "sparse.ttf": build(FULL.replace("D", "")),
        "collection.ttc": collection(build(FULL, ps_name="TPDFOtherSans"), full),
        "cff.otf": build(FULL, cff=True),
        "cff2048.otf": build(FULL, cff=True, cff_upem=2048),
        "subset.cff": bare_cff(build(SUBSET, cff=True, adobe_names=True)),
        "colour.ttf": colour(full),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    fonts = generate()
    if args.check:
        stale = [name for name, data in fonts.items() if (OUT / name).read_bytes() != data]
        for name in stale:
            print(f"[FAIL] {name} differs from what the generator writes")
        if not stale:
            print(f"[PASS] {len(fonts)} fonts match the generator")
        return 1 if stale else 0
    OUT.mkdir(parents=True, exist_ok=True)
    for name, data in fonts.items():
        (OUT / name).write_bytes(data)
        print(f"[OK] {name} {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
