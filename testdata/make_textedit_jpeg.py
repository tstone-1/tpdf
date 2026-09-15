#!/usr/bin/env python3
"""Generate original geometric JPEG images and text-editing PDFs.

uv run --with pillow --with fonttools --with pypdf testdata/make_textedit_jpeg.py <directory>
The four 8x8 JPEGs also regenerate src-tauri/src/textedit/images/synthetic-*.jpg.
Worker/native readback uses the ordinary textedit mode; PDFKit uses --image.
"""
from io import BytesIO
from pathlib import Path
import argparse
import subprocess
import sys


def main():
    from PIL import Image
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import EncodedStreamObject, NameObject, NumberObject

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output
    root.mkdir(parents=True, exist_ok=True)
    base = root / "base.pdf"
    subprocess.run([sys.executable, str(Path(__file__).with_name("make_textedit_symbolic.py")),
                    str(base), "--image"], check=True)
    for name, mode, progressive in [("rgb", "RGB", False), ("gray", "L", False),
                                    ("progressive", "RGB", True), ("cmyk", "CMYK", False)]:
        pixels = Image.new("RGB", (8, 8))
        pixels.putdata([(x * 31, y * 31, 90) for y in range(8) for x in range(8)])
        output = BytesIO()
        pixels.convert(mode).save(output, format="JPEG", quality=90, progressive=progressive)
        data = output.getvalue()
        (root / f"synthetic-{name}.jpg").write_bytes(data)
        writer = PdfWriter(clone_from=PdfReader(base))
        resources = writer.pages[0]["/Resources"]
        image = EncodedStreamObject()
        image.update({NameObject(k): value for k, value in {
            "/Type": NameObject("/XObject"), "/Subtype": NameObject("/Image"),
            "/Width": NumberObject(8), "/Height": NumberObject(8),
            "/BitsPerComponent": NumberObject(8), "/Filter": NameObject("/DCTDecode"),
            "/ColorSpace": NameObject("/" + {"RGB": "DeviceRGB", "L": "DeviceGray", "CMYK": "DeviceCMYK"}[mode]),
            "/Name": NameObject("/AuthoredImage"),
        }.items()})
        image._data = data
        resources["/XObject"][NameObject("/Image1")] = writer._add_object(image)
        writer.write(root / f"{name}.pdf")
    print("[PASS] generated four synthetic JPEG streams and image/text PDFs")


if __name__ == "__main__":
    main()
