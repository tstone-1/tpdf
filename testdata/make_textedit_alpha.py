#!/usr/bin/env python3
"""Generate text-editing PDFs whose images carry alpha, mappings and metadata.

uv run --with fonttools --with pypdf testdata/make_textedit_alpha.py <directory>
Three original images, each beside the same two editable text runs:
  alpha.pdf   DeviceRGB samples with a DeviceGray soft mask, both written with
              decode parameters that select no prediction, and an XMP packet.
  palette.pdf an indexed image carrying an explicit copy of the default sample
              mapping for its bit depth, which is what ISO 32000-1 Table 89
              already specifies.
  masked.pdf  the indexed image above with a soft mask of its own.
Worker/native readback uses the ordinary textedit mode; PDFKit uses --image.
"""
from pathlib import Path
import argparse
import subprocess
import sys
import zlib

WIDTH, HEIGHT = 24, 8
PACKET = (b'<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>'
          b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
          b'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
          b'<rdf:Description rdf:about=""/></rdf:RDF></x:xmpmeta>'
          b'<?xpacket end="r"?>')


def main():
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import (ArrayObject, ByteStringObject, DecodedStreamObject,
                               DictionaryObject, EncodedStreamObject, NameObject,
                               NumberObject)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output
    root.mkdir(parents=True, exist_ok=True)
    base = root / "base.pdf"
    subprocess.run([sys.executable, str(Path(__file__).with_name("make_textedit_symbolic.py")),
                    str(base), "--image"], check=True)

    def stream(data, entries):
        image = EncodedStreamObject()
        image.update({NameObject(k): v for k, v in {
            "/Type": NameObject("/XObject"), "/Subtype": NameObject("/Image"),
            "/Width": NumberObject(WIDTH), "/Height": NumberObject(HEIGHT),
            "/BitsPerComponent": NumberObject(8),
            "/Filter": NameObject("/FlateDecode"),
            **entries,
        }.items()})
        image._data = zlib.compress(data, 9)
        return image

    # Predictor is absent, so it defaults to 1 and the other entries describe
    # nothing. This is what Word and LiveCycle write beside a Flate image.
    def parameters(colors):
        return DictionaryObject({
            NameObject("/BitsPerComponent"): NumberObject(8),
            NameObject("/Colors"): NumberObject(colors),
            NameObject("/Columns"): NumberObject(WIDTH),
        })

    colour = bytes(v for y in range(HEIGHT) for x in range(WIDTH)
                   for v in (x * 10, y * 30, 90))
    # Opaque except the last column, so the painted region stays visible and the
    # mask is still doing something a reader can measure.
    alpha = bytes(0 if x == WIDTH - 1 else 255 for _ in range(HEIGHT) for x in range(WIDTH))
    palette = bytes((0, 0, 0, 240, 90, 20, 20, 90, 240))
    indices = bytes((x + y) % 3 for y in range(HEIGHT) for x in range(WIDTH))

    def build(name, image, mask=None, metadata=False):
        writer = PdfWriter(clone_from=PdfReader(base))
        if mask is not None:
            image[NameObject("/SMask")] = writer._add_object(mask)
        if metadata:
            packet = DecodedStreamObject()
            packet.update({NameObject("/Type"): NameObject("/Metadata"),
                           NameObject("/Subtype"): NameObject("/XML")})
            packet.set_data(PACKET)
            image[NameObject("/Metadata")] = writer._add_object(packet)
        resources = writer.pages[0]["/Resources"]
        resources["/XObject"][NameObject("/Image1")] = writer._add_object(image)
        writer.write(root / f"{name}.pdf")

    def gray_mask():
        return stream(alpha, {"/ColorSpace": NameObject("/DeviceGray"),
                              "/DecodeParms": parameters(1)})

    def indexed():
        return stream(indices, {
            "/ColorSpace": ArrayObject([
                NameObject("/Indexed"), NameObject("/DeviceRGB"),
                NumberObject(2), ByteStringObject(palette)]),
            # The default mapping for eight-bit samples, written out in full.
            "/Decode": ArrayObject([NumberObject(0), NumberObject(255)]),
        })

    build("alpha", stream(colour, {"/ColorSpace": NameObject("/DeviceRGB"),
                                   "/DecodeParms": parameters(3)}),
          mask=gray_mask(), metadata=True)
    build("palette", indexed())
    build("masked", indexed(), mask=gray_mask())
    print("[PASS] generated soft-masked, indexed and metadata-bearing image PDFs")


if __name__ == "__main__":
    main()
