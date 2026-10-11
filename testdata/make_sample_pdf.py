#!/usr/bin/env python3
"""Write the badly redacted letter the landing page offers for trying tpdf.

    uv run --with reportlab testdata/make_sample_pdf.py site/sample-redacted.pdf

One page, a made-up letter from a housing office, in which four passages are
covered the wrong way: a black rectangle is drawn over each and the words stay
in the file. It is the commonest wrong redaction, and the page needs a document
that has it, because a visitor rarely has one they may open in a new program.

Everything in it is synthetic and says so --- "Example Housing Office", "Jane
Example", an `example.org` address, an account number of zeros --- because the
file is published, and a realistic name in it reads as somebody's real name.

`site/sample-redacted.pdf` is tracked, since the page links to it. `invariant`
makes reportlab leave out the date and the random identifier, so writing it
again gives the same bytes and a changed file means a changed letter.
`COVERED` is what `tpdf hidden` has to list for it.
"""

from __future__ import annotations

import sys
from pathlib import Path

from reportlab.lib.colors import HexColor, black
from reportlab.pdfbase.pdfmetrics import stringWidth
from reportlab.pdfgen.canvas import Canvas

WIDTH, HEIGHT = 595.0, 842.0
LEFT = 72.0
INK = HexColor("#1b1f24")
SOFT = HexColor("#5b6570")
FONT, BOLD, SIZE = "Helvetica", "Helvetica-Bold", 11.0

#: The passages under a black rectangle, in reading order.
COVERED = ("Jane Example", "12 Sample Street, Example Town", "EX00 0000 0000 0000 0000 00", "4,250.00")

#: The letter. A `{n}` is where `COVERED[n]` stands, under its rectangle.
LINES = [
    "Dear {0},",
    "",
    "we have decided on your application of 3 March 2026 for a housing",
    "allowance for the flat at {1}.",
    "",
    "The allowance is granted from 1 April 2026. The first payment covers April",
    "to June and comes to EUR {3}. It is paid to the account",
    "{2}",
    "within ten working days.",
    "",
    "You can object to this decision in writing within one month.",
    "",
    "Yours sincerely,",
    "Example Housing Office",
]


def build(out: Path) -> None:
    page = Canvas(str(out), pagesize=(WIDTH, HEIGHT), invariant=1)
    page.setTitle("Decision on a housing allowance (sample)")
    page.setAuthor("Example Housing Office")
    page.setFillColor(INK)
    page.setFont(BOLD, 16)
    page.drawString(LEFT, HEIGHT - 90, "Example Housing Office")
    page.setFont(FONT, 9)
    page.setFillColor(SOFT)
    page.drawString(LEFT, HEIGHT - 106, "1 Example Square, Example Town  -  office@example.org")
    page.drawString(LEFT, HEIGHT - 150, "Reference EX-2026-0001  -  17 March 2026")
    page.drawString(LEFT, HEIGHT - 780, "A made-up letter for trying tpdf. No person, address or account in it is real.")
    page.setFillColor(INK)
    page.setFont(BOLD, 12)
    page.drawString(LEFT, HEIGHT - 190, "Decision on your application for a housing allowance")

    y = HEIGHT - 226
    boxes = []
    for line in LINES:
        x = LEFT
        rest = line
        while rest:
            before, brace, after = rest.partition("{")
            page.setFont(FONT, SIZE)
            page.drawString(x, y, before)
            x += stringWidth(before, FONT, SIZE)
            if not brace:
                break
            number, _, rest = after.partition("}")
            words = COVERED[int(number)]
            page.drawString(x, y, words)
            width = stringWidth(words, FONT, SIZE)
            boxes.append((x - 2, y - 3.5, width + 4, SIZE + 4))
            x += width
        y -= 18
    # After the text, so the rectangles are over it. The words are still there.
    page.setFillColor(black)
    for box in boxes:
        page.rect(*box, stroke=0, fill=1)
    page.save()


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    build(Path(sys.argv[1]))
    print(f"[OK] wrote {sys.argv[1]}")
