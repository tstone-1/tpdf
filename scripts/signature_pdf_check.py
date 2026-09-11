"""Independent rendering check for synthetic signature quadrants on rotated/cropped pages.

Generate: TPDF_SIGNATURE_PROBE=<dir> cargo test --lib signature_pixels_alpha_and_placement
Run: uv run --with pypdfium2 --with pillow scripts/signature_pdf_check.py <dir>
Expected display positions are fixed fixture coordinates, independent of writer matrices.
"""
from pathlib import Path
import sys
import pypdfium2 as pdfium

def main() -> None:
    root = Path(sys.argv[1])
    count = 0
    for prefix in ("signature", "signature-append"):
        for turns in range(4):
            with pdfium.PdfDocument(root / f"{prefix}-{turns}.pdf") as doc:
                page = doc[0]
                try:
                    image = page.render(scale=1, draw_annots=True).to_pil().convert("RGB")
                    for x, y, expected in [(80, 80, (255, 0, 0)), (160, 80, (0, 255, 0)),
                                            (80, 160, (0, 0, 255)), (160, 160, (255, 255, 255))]:
                        actual = image.getpixel((x, y))
                        assert all(abs(a-b) <= 2 for a, b in zip(actual, expected)), (prefix, turns, x, y, actual, expected)
                        count += 1
                    image.save(root / f"{prefix}-{turns}.png")
                finally:
                    page.close()
    print(f"[PASS] {count} independent pixel checks: placement, orientation and transparency in 8 saved PDFs")

if __name__ == "__main__":
    main()
