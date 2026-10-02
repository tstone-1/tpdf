#!/usr/bin/env python3
"""Write the document the README's screenshots are taken of.

    uv run --with reportlab testdata/make_demo_pdf.py testdata/demo.pdf

Three pages of a made-up tide survey: a title, prose, a table, a chart, and a
contact block. Everything in it is synthetic and says so --- "Example Port
Authority", "Jane Example", an `example.org` address --- because a screenshot is
published, and a realistic name in one reads as somebody's real name.

`scripts/screenshots.py` imports `build` rather than running this file, because
it needs what `build` returns: where on page 1 the line to highlight, the table
row to box, the comment and the two contact lines to redact are. Those are
computed from the text widths here, so changing a sentence moves the marks with
it instead of leaving them over the wrong words.

Regions are `[left, top, right, bottom]` in points from the page's top-left
corner, which is what the application's own mark and redaction calls take.
"""

from __future__ import annotations

import sys
from pathlib import Path

from reportlab.lib.colors import HexColor
from reportlab.pdfbase.pdfmetrics import stringWidth
from reportlab.pdfgen.canvas import Canvas

WIDTH, HEIGHT = 595.0, 842.0
LEFT = 64.0
INK = HexColor("#1b1f24")
SOFT = HexColor("#5b6570")
RULE = HexColor("#c9d1d9")
BLUE = HexColor("#2f6feb")

SUMMARY = [
    "The 2026 survey measured the tide at four stations in Example Harbour",
    "between March and August. The highest water was 4.82 m at the North Pier,",
    "which is 0.31 m above the level the quay was built for.",
    "Every reading in this report is invented for demonstration.",
]
ROWS = [
    ("Station", "Highest", "Lowest", "Mean range"),
    ("North Pier", "4.82 m", "0.41 m", "3.10 m"),
    ("Ferry Dock", "4.67 m", "0.38 m", "3.02 m"),
    ("Lighthouse", "4.71 m", "0.44 m", "2.97 m"),
    ("River Mouth", "4.35 m", "0.52 m", "2.61 m"),
]
CONTACT = [
    "Prepared by Jane Example, Example Port Authority",
    "12 Sample Street, Exampleton",
    "jane.example@example.org",
]


def line_box(text: str, font: str, size: float, baseline: float, x: float = LEFT) -> list[float]:
    """The box around one line, in display space, with a little air."""
    return [x - 2, baseline - size * 0.85, x + stringWidth(text, font, size) + 2, baseline + size * 0.3]


def build(path: Path) -> dict[str, list[float]]:
    """Writes the document and returns the regions the screenshots mark."""
    canvas = Canvas(str(path), pagesize=(WIDTH, HEIGHT))
    canvas.setTitle("Harbour Tide Survey 2026")
    canvas.setAuthor("Example Port Authority")
    regions: dict[str, list[float]] = {}

    def text(value: str, top: float, font: str = "Helvetica", size: float = 11, colour=INK, x: float = LEFT) -> None:
        canvas.setFont(font, size)
        canvas.setFillColor(colour)
        canvas.drawString(x, HEIGHT - top, value)

    # ---- Page 1
    canvas.bookmarkPage("summary")
    canvas.addOutlineEntry("Summary", "summary", level=0)
    # Everything the screenshots mark sits in the top 470 points: that is what
    # a 1200 x 900 window shows of the page at fit-width, and a marked line
    # below it is a region in the list with nothing to see on the page.
    text("Harbour Tide Survey 2026", 84, "Helvetica-Bold", 26)
    text("Example Port Authority  ·  synthetic demonstration document", 108, size=11, colour=SOFT)
    canvas.setStrokeColor(RULE)
    canvas.line(LEFT, HEIGHT - 124, WIDTH - LEFT, HEIGHT - 124)

    text("Summary", 158, "Helvetica-Bold", 16)
    for at, value in enumerate(SUMMARY):
        text(value, 184 + at * 18)
    regions["highlight"] = line_box(SUMMARY[1], "Helvetica", 11, 184 + 18)
    regions["note"] = [WIDTH - LEFT - 6, 166, WIDTH - LEFT + 14, 186]

    text("Contact", 280, "Helvetica-Bold", 16)
    for at, value in enumerate(CONTACT):
        text(value, 306 + at * 18)
    # The name alone on the first line, and the whole address line.
    before = stringWidth("Prepared by ", "Helvetica", 11)
    regions["redact_name"] = line_box("Jane Example", "Helvetica", 11, 306, LEFT + before)
    regions["redact_mail"] = line_box(CONTACT[2], "Helvetica", 11, 306 + 36)

    text("Readings by station", 384, "Helvetica-Bold", 16)
    columns = [LEFT, 220.0, 320.0, 420.0]
    for row, cells in enumerate(ROWS):
        top = 412 + row * 24
        for x, cell in zip(columns, cells):
            text(cell, top, "Helvetica-Bold" if row == 0 else "Helvetica", colour=SOFT if row == 0 else INK, x=x)
        canvas.line(LEFT, HEIGHT - (top + 8), WIDTH - LEFT, HEIGHT - (top + 8))
    regions["box"] = [LEFT - 6, 412 + 24 - 15, WIDTH - LEFT + 6, 412 + 24 + 7]
    canvas.showPage()

    # ---- Page 2: the chart
    canvas.bookmarkPage("chart")
    canvas.addOutlineEntry("Highest water by month", "chart", level=0)
    text("Highest water by month", 104, "Helvetica-Bold", 20)
    months = ["Mar", "Apr", "May", "Jun", "Jul", "Aug"]
    levels = [4.31, 4.52, 4.67, 4.82, 4.60, 4.44]
    base, top_of, step = HEIGHT - 520, 300.0, 70.0
    canvas.setStrokeColor(RULE)
    for grid in range(5):
        y = base + grid * top_of / 4
        canvas.line(LEFT, y, WIDTH - LEFT, y)
    for at, (month, level) in enumerate(zip(months, levels)):
        x = LEFT + 30 + at * step
        height = (level - 4.0) * top_of
        canvas.setFillColor(BLUE)
        canvas.rect(x, base, 40, height, stroke=0, fill=1)
        text(month, 540, colour=SOFT, x=x + 9)
        text(f"{level:.2f}", HEIGHT - (base + height + 6), size=9, x=x + 8)
    canvas.showPage()

    # ---- Page 3: method
    canvas.bookmarkPage("method")
    canvas.addOutlineEntry("Method", "method", level=0)
    text("Method", 104, "Helvetica-Bold", 20)
    for at, value in enumerate([
        "Each station carried one pressure gauge, read every ten minutes.",
        "Gauges were levelled against the harbour datum before and after the season.",
        "Gaps shorter than one hour were filled from the nearest station.",
    ]):
        text(value, 140 + at * 18)
    canvas.showPage()
    canvas.save()
    return regions


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    for name, region in build(Path(sys.argv[1])).items():
        print(f"{name}: {[round(v, 1) for v in region]}")
