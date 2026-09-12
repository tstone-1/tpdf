#!/usr/bin/env python3
"""Independent producer fixtures for text-edit-probe and text_edit_pdfkit.swift.

uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-reportlab
Pass each resulting PDF as text-edit-probe's optional second argument.
"""

import sys
from pathlib import Path

from reportlab import rl_config
from reportlab.pdfgen.canvas import Canvas

# Exercise the supported bounded Flate decoder without an ASCII85 wrapper.
rl_config.useA85 = False
root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
for mode in ("separate", "multiline"):
    canvas = Canvas(str(root / f"{mode}.pdf"), pagesize=(300, 240),
                    pageCompression=1, invariant=1)
    if mode == "separate":
        canvas.drawString(40, 180, "SYNTHETIC FIRST")
        canvas.drawString(40, 140, "SYNTHETIC SECOND")
    else:
        text = canvas.beginText(40, 180)
        text.setLeading(40)
        text.textLine("SYNTHETIC FIRST")
        text.textLine("SYNTHETIC SECOND")
        canvas.drawText(text)
    canvas.save()
    print(f"[OK] wrote {mode}.pdf")
