#!/usr/bin/env python3
"""Independent producer fixtures for text-edit-probe and text_edit_pdfkit.swift.

uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-reportlab
Pass each PDF as text-edit-probe's optional second argument; add --latin1 for latin1.pdf.
"""

import re
import sys
from pathlib import Path

from reportlab import rl_config
from reportlab.pdfgen.canvas import Canvas
from reportlab.pdfbase._fontdata import encodings, widthsByFontGlyph

# Check the entire supported width domain, not one rendered sentence. This
# reference is owned by the independent PDF producer and is not derived from Rust.
source = (Path(__file__).resolve().parents[1] / "src-tauri/src/textbox.rs").read_text()
for name, start, count in (("WIDTHS", 32, 95), ("LATIN1_WIDTHS", 160, 96)):
    bodies = re.findall(rf"const {name}: \[u16; \d+\] = \[(.*?)\];", source, re.S)
    assert len(bodies) == 1, f"expected one {name} table"
    body = re.sub(r"//[^\n]*", "", bodies[0])
    values = [int(value) for value in re.findall(r"\d+", body)]
    expected = [widthsByFontGlyph["Helvetica"][encodings["WinAnsiEncoding"][code]]
                for code in range(start, start + count)]
    assert values == expected, f"{name} differs from independent Helvetica metrics"
print("[PASS] all 191 supported Helvetica advances match independent metrics")

root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
for mode, wrapped in (("separate", False), ("multiline", False),
                      ("separate-ascii85", True), ("multiline-ascii85", True), ("latin1", True)):
    # True is ReportLab's normal wrapper for compressed page content; keep the
    # Flate-only layouts as controls over the extra decoding stage.
    rl_config.useA85 = wrapped
    canvas = Canvas(str(root / f"{mode}.pdf"), pagesize=(300, 240),
                    pageCompression=1, invariant=1)
    if mode.startswith("separate"):
        canvas.drawString(40, 180, "SYNTHETIC FIRST")
        canvas.drawString(40, 140, "SYNTHETIC SECOND")
    else:
        text = canvas.beginText(40, 180)
        text.setLeading(40)
        text.textLine("SYNTHETIC ÄÖÜ ß" if mode == "latin1" else "SYNTHETIC FIRST")
        text.textLine("SYNTHETIC SECOND")
        canvas.drawText(text)
    canvas.save()
    print(f"[OK] wrote {mode}.pdf")
