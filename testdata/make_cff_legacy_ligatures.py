"""Derive src-tauri/src/textedit/fonts/cff/fixtures/legacy-ligatures.cff from the
synthetic ligatures.cff beside it, renaming its ligature glyphs to Adobe's
original names (f_i -> fi, f_l -> fl, f_f -> ff, f_f_i -> ffi), which older CFF
fonts use. Outlines, widths and everything else are unchanged.

Usage:
    uv run --with fonttools python testdata/make_cff_legacy_ligatures.py
"""

import io
import pathlib
import types

from fontTools.cffLib import CFFFontSet

HERE = pathlib.Path(__file__).resolve().parent.parent / "src-tauri/src/textedit/fonts/cff/fixtures"
RENAMES = {"f_i": "fi", "f_l": "fl", "f_f": "ff", "f_f_i": "ffi"}

cff = CFFFontSet()
cff.decompile(io.BytesIO((HERE / "ligatures.cff").read_bytes()), None)
top = cff[cff.fontNames[0]]
order = top.getGlyphOrder()
assert all(name in order for name in RENAMES), order
renamed = [RENAMES.get(name, name) for name in order]
top.charset = renamed
top.CharStrings.charStrings = {name: index for index, name in enumerate(renamed)}
out = io.BytesIO()
cff.compile(out, types.SimpleNamespace(recalcBBoxes=False))
(HERE / "legacy-ligatures.cff").write_bytes(out.getvalue())

check = CFFFontSet()
check.decompile(io.BytesIO(out.getvalue()), None)
assert check[check.fontNames[0]].getGlyphOrder() == renamed
print("[OK] wrote legacy-ligatures.cff:", [n for n in renamed if n in RENAMES.values()])
