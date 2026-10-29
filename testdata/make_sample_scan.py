#!/usr/bin/env python3
"""Write the scanned invoices the landing pages offer for trying page recognition.

    uv run --with reportlab testdata/make_sample_scan.py <tpdf-cli> site

One page each, a made-up invoice from an office supplier: `site/sample-scan.pdf`
in English for `site/index.html`, `site/de/beispiel-scan.pdf` in German for
`site/de/index.html`. Each page is a picture and holds no text, as a scan does,
so a search for the invoice number finds nothing until the page is recognised.
A visitor rarely has a scan to hand that they may open in a new program, which is
why the page needs one.

The invoice is laid out with reportlab, rendered to a picture by tpdf's own
`render` at 150 dpi, and wrapped in a PDF by `scan_pdf` from
`scripts/tabs_check.py`, the same route the recognise window check takes.

Everything in it is synthetic and says so --- "Example Office Supplies",
`example.org`, an account number of zeros --- because the files are published.
`NUMBER` is what each landing page tells the visitor to search for.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

from reportlab.lib.colors import HexColor
from reportlab.pdfgen.canvas import Canvas

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
from tabs_check import scan_pdf  # noqa: E402

WIDTH, HEIGHT = 595.0, 842.0
LEFT, RIGHT = 72.0, 523.0
INK = HexColor("#1b1f24")
SOFT = HexColor("#5b6570")
FONT, BOLD = "Helvetica", "Helvetica-Bold"
DPI = 150

#: What each landing page tells the visitor to search for, by language.
NUMBER = {"en": "INV-2026-0417", "de": "RE-2026-0417"}

TEXT = {
    "en": {
        "file": "sample-scan.pdf",
        "seller": "Example Office Supplies",
        "address": "1 Example Road, Example Town  -  sales@example.org",
        "title": "Invoice",
        "facts": [("Invoice number", NUMBER["en"]), ("Date", "14 September 2026"),
                  ("Customer", "Example Housing Office")],
        "head": ("Item", "Quantity", "Amount"),
        "items": [("Printer paper A4, box of 2,500 sheets", "10", "EUR 289.00"),
                  ("Toner cartridge, black", "4", "EUR 356.00"),
                  ("Ring binder, 5 cm", "50", "EUR 112.50")],
        "total": ("Total", "EUR 757.50"),
        "pay": [f"Please pay within 30 days and quote {NUMBER['en']}.",
                "Account EX00 0000 0000 0000 0000 00"],
        "note": "A made-up invoice for trying tpdf. No company, address or account in it is real.",
    },
    "de": {
        "file": "de/beispiel-scan.pdf",
        "seller": "Beispiel Bürobedarf GmbH",
        "address": "Beispielweg 1, 00000 Beispielstadt  -  verkauf@example.org",
        "title": "Rechnung",
        "facts": [("Rechnungsnummer", NUMBER["de"]), ("Datum", "14. September 2026"),
                  ("Kunde", "Beispiel Wohnungsamt")],
        "head": ("Artikel", "Menge", "Betrag"),
        "items": [("Druckerpapier A4, Karton mit 2.500 Blatt", "10", "289,00 EUR"),
                  ("Tonerkartusche, schwarz", "4", "356,00 EUR"),
                  ("Ordner, 5 cm", "50", "112,50 EUR")],
        "total": ("Summe", "757,50 EUR"),
        "pay": [f"Bitte zahlen Sie innerhalb von 30 Tagen unter Angabe von {NUMBER['de']}.",
                "Konto EX00 0000 0000 0000 0000 00"],
        "note": "Eine erfundene Rechnung zum Ausprobieren von tpdf. Firma, Adresse und Konto sind nicht echt.",
    },
}


def layout(text: dict, out: Path) -> None:
    """The invoice as a PDF with text, before it becomes a picture."""
    page = Canvas(str(out), pagesize=(WIDTH, HEIGHT), invariant=1)
    page.setFillColor(INK)
    page.setFont(BOLD, 16)
    page.drawString(LEFT, HEIGHT - 90, text["seller"])
    page.setFont(FONT, 9)
    page.setFillColor(SOFT)
    page.drawString(LEFT, HEIGHT - 106, text["address"])
    page.drawString(LEFT, HEIGHT - 780, text["note"])
    page.setFillColor(INK)
    page.setFont(BOLD, 20)
    page.drawString(LEFT, HEIGHT - 170, text["title"])

    y = HEIGHT - 205
    for label, value in text["facts"]:
        page.setFont(FONT, 11)
        page.drawString(LEFT, y, label)
        page.setFont(BOLD, 11)
        page.drawString(LEFT + 130, y, value)
        y -= 18

    y -= 22
    item, quantity, amount = text["head"]
    page.setFont(BOLD, 11)
    page.drawString(LEFT, y, item)
    page.drawRightString(LEFT + 330, y, quantity)
    page.drawRightString(RIGHT, y, amount)
    page.setStrokeColor(SOFT)
    page.line(LEFT, y - 6, RIGHT, y - 6)
    y -= 24
    page.setFont(FONT, 11)
    for item, quantity, amount in text["items"]:
        page.drawString(LEFT, y, item)
        page.drawRightString(LEFT + 330, y, quantity)
        page.drawRightString(RIGHT, y, amount)
        y -= 20
    page.line(LEFT, y + 8, RIGHT, y + 8)
    y -= 10
    page.setFont(BOLD, 11)
    page.drawString(LEFT, y, text["total"][0])
    page.drawRightString(RIGHT, y, text["total"][1])

    y -= 50
    page.setFont(FONT, 11)
    for line in text["pay"]:
        page.drawString(LEFT, y, line)
        y -= 18
    page.save()


def build(tool: Path, site: Path) -> None:
    with tempfile.TemporaryDirectory() as room:
        for language, text in TEXT.items():
            typed, picture = Path(room) / f"{language}.pdf", Path(room) / f"{language}.png"
            layout(text, typed)
            subprocess.run([str(tool), "render", str(typed), "-o", str(picture), "--dpi", str(DPI)],
                           check=True, capture_output=True)
            out = site / text["file"]
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(scan_pdf(picture.read_bytes(), DPI, 1))
            print(f"[OK] wrote {out}: search for {NUMBER[language]}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    build(Path(sys.argv[1]), Path(sys.argv[2]))
