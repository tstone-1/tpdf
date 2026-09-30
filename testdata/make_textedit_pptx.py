#!/usr/bin/env python3
"""Generate one page holding every PowerPoint shape the text editor admits.

uv run --with fonttools --with pypdf testdata/make_textedit_pptx.py <directory>
Writes <directory>/pptx.pdf: the symbolic fixture's two lines, tagged and drawn
the way Microsoft PowerPoint for Microsoft 365 exports a slide (measured on the
EC consumer factsheet, BUILD.md *PowerPoint factsheet*), with original content:
  - a page clip `m l l l W* n`, closed by the clip, corners off the axis by noise;
  - the first line in a table cell, TD > Textbox (RoleMap: Sect) > P > Span;
  - the second in a body paragraph, P > Span, and every Span carrying ActualText
    equal to its words, the second with the word's space at its end;
  - a third, read-only line, a link's words: Link > Span, the annotation named
    through an OBJR that is an indirect object;
  - a gradient: an axial shading whose function is a type 0 sampled stream;
  - a picture turned inside a turned clip, its soft mask carrying /Matte, in a
    Figure whose BBox names its corners top first.
Worker and native checks use the ordinary textedit mode; the independent
readers take --pptx (the Span's ActualText has to change with its words).
"""
from pathlib import Path
import argparse
import subprocess
import sys
import zlib


def main():
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import (ArrayObject, BooleanObject, DecodedStreamObject,
                               DictionaryObject, EncodedStreamObject, FloatObject,
                               NameObject, NumberObject, TextStringObject)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output
    root.mkdir(parents=True, exist_ok=True)
    base = root / "base.pdf"
    subprocess.run([sys.executable, str(Path(__file__).with_name("make_textedit_symbolic.py")),
                    str(base)], check=True)
    writer = PdfWriter(clone_from=PdfReader(base))
    page = writer.pages[0]
    first, second = page.get_contents().get_data().decode().split("\n")
    # The link's words: the codes of FIRST, drawn below the two lines.
    link_words = "BT /F1 12 Tf 40 100 Td <0a070b0104> Tj ET"

    def name(value):
        return NameObject("/" + value)

    def numbers(*values):
        return ArrayObject(FloatObject(v) if isinstance(v, float) else NumberObject(v) for v in values)

    def add(entries, data=None, compress=False):
        if data is None:
            return writer._add_object(DictionaryObject({NameObject(k): v for k, v in entries.items()}))
        stream = EncodedStreamObject() if compress else DecodedStreamObject()
        stream.update({NameObject(k): v for k, v in entries.items()})
        if compress:
            stream[NameObject("/Filter")] = name("FlateDecode")
            stream._data = zlib.compress(data, 9)
        else:
            stream.set_data(data)
        return writer._add_object(stream)

    # The gradient: 512 eight-bit RGB samples, Flate-compressed, as PowerPoint.
    samples = bytes(value for i in range(512) for value in (i // 2, 255 - i // 2, 128))
    function = add({"/FunctionType": NumberObject(0), "/Domain": numbers(0, 1),
                    "/Range": numbers(0, 1, 0, 1, 0, 1), "/Size": numbers(512),
                    "/BitsPerSample": NumberObject(8), "/Order": NumberObject(1),
                    "/Encode": numbers(0, 511), "/Decode": numbers(0, 1, 0, 1, 0, 1)},
                   samples, compress=True)
    shading = add({"/ShadingType": NumberObject(2), "/ColorSpace": name("DeviceRGB"),
                   "/Coords": numbers(200, 0, 260, 0),
                   "/Extend": ArrayObject([BooleanObject(True), BooleanObject(True)]),
                   "/Function": function})
    pattern = add({"/Type": name("Pattern"), "/PatternType": NumberObject(2), "/Shading": shading})
    # The picture: 8 x 8 RGB, and a soft mask whose colour was premultiplied
    # against black.
    mask = add({"/Type": name("XObject"), "/Subtype": name("Image"), "/Width": NumberObject(8),
                "/Height": NumberObject(8), "/BitsPerComponent": NumberObject(8),
                "/ColorSpace": name("DeviceGray"), "/Matte": numbers(0, 0, 0)},
               bytes((x * 32) for y in range(8) for x in range(8)), compress=True)
    picture = add({"/Type": name("XObject"), "/Subtype": name("Image"), "/Width": NumberObject(8),
                   "/Height": NumberObject(8), "/BitsPerComponent": NumberObject(8),
                   "/ColorSpace": name("DeviceRGB"), "/SMask": mask},
                  bytes(value for y in range(8) for x in range(8) for value in (x * 32, y * 32, 96)),
                  compress=True)
    resources = page["/Resources"]
    resources[NameObject("/Pattern")] = DictionaryObject({NameObject("/P0"): pattern})
    resources[NameObject("/XObject")] = DictionaryObject({NameObject("/Im0"): picture})

    body = "\n".join([
        "q -0.00006 240 m 300 240 l 300 -0.00012 l -0.00006 -0.00006 l W* n",
        "/Artifact BMC q /Pattern cs /P0 scn 200 20 60 30 re f Q EMC",
        "/Figure <</MCID 3>> BDC q 250 60 m 268 57 l 265 39 l 247 42 l 250 60 l h W* n",
        "q 1 0.14 -0.14 1 247 42 cm 18 0 0 18 0 0 cm /Im0 Do Q Q EMC",
        f"/TD <</MCID 0>> BDC {first} EMC",
        f"/P <</MCID 1>> BDC {second} EMC",
        f"/Span <</MCID 2>> BDC {link_words} EMC",
        "Q",
    ])
    content = DecodedStreamObject()
    content.set_data(body.encode())
    page[NameObject("/Contents")] = writer._add_object(content)

    # The structure, PowerPoint's shapes around the two lines and the link.
    ref = {}

    def element(key, kind, parent, kids, **extra):
        ref.setdefault(key, writer._add_object(DictionaryObject()))
        entries = {"/Type": name("StructElem"), "/S": name(kind), "/P": ref[parent], "/Pg": page.indirect_reference,
                   "/K": ArrayObject(kids)}
        entries.update({NameObject("/" + k): v for k, v in extra.items()})
        ref[key].get_object().update({NameObject(k): v for k, v in entries.items()})
        return ref[key]

    for key in ("root", "document", "table", "row", "cell", "textbox", "first", "first-span",
                "body", "second-span", "link-paragraph", "link", "link-span", "figure"):
        ref[key] = writer._add_object(DictionaryObject())
    annotation = add({"/Type": name("Annot"), "/Subtype": name("Link"), "/P": page.indirect_reference,
                      "/Rect": numbers(38, 96, 90, 112), "/StructParent": NumberObject(1),
                      "/Border": numbers(0, 0, 0)})
    objr = add({"/Type": name("OBJR"), "/Obj": annotation, "/Pg": page.indirect_reference})
    element("first-span", "Span", "first", [NumberObject(0)], ActualText=TextStringObject("SYNTHETIC FIRST"))
    element("first", "P", "textbox", [ref["first-span"]])
    element("textbox", "Textbox", "cell", [ref["first"]])
    element("cell", "TD", "row", [ref["textbox"]])
    element("row", "TR", "table", [ref["cell"]])
    element("table", "Table", "document", [ref["row"]])
    element("second-span", "Span", "body", [NumberObject(1)], ActualText=TextStringObject("SYNTHETIC SECOND "))
    element("body", "P", "document", [ref["second-span"]])
    element("link-span", "Span", "link", [NumberObject(2)], ActualText=TextStringObject("FIRST"))
    element("link", "Link", "link-paragraph", [ref["link-span"], objr])
    element("link-paragraph", "P", "document", [ref["link"]])
    element("figure", "Figure", "document", [NumberObject(3)],
            A=DictionaryObject({NameObject("/O"): name("Layout"), NameObject("/BBox"): numbers(247, 60, 268, 39)}),
            Alt=TextStringObject("A turned picture"))
    element("document", "Document", "root",
            [ref["table"], ref["body"], ref["link-paragraph"], ref["figure"]])
    del ref["document"].get_object()["/Pg"]
    parents = add({"/Nums": ArrayObject([
        NumberObject(0), ArrayObject([ref["first-span"], ref["second-span"], ref["link-span"], ref["figure"]]),
        NumberObject(1), ref["link"]])})
    ref["root"].get_object().update({
        NameObject("/Type"): name("StructTreeRoot"), NameObject("/K"): ArrayObject([ref["document"]]),
        NameObject("/ParentTree"): parents, NameObject("/ParentTreeNextKey"): NumberObject(2),
        NameObject("/RoleMap"): DictionaryObject({NameObject("/Textbox"): name("Sect")})})
    for key in ("document",):
        ref[key].get_object()[NameObject("/P")] = ref["root"]
    page[NameObject("/StructParents")] = NumberObject(0)
    page[NameObject("/Annots")] = ArrayObject([annotation])
    writer.root_object[NameObject("/StructTreeRoot")] = ref["root"]
    writer.root_object[NameObject("/MarkInfo")] = DictionaryObject({NameObject("/Marked"): BooleanObject(True)})
    target = root / "pptx.pdf"
    writer.write(target)
    base.unlink()
    text = PdfReader(target).pages[0].extract_text()
    assert "SYNTHETIC FIRST" in text and "SYNTHETIC SECOND" in text, text
    print("[PASS] generated the PowerPoint-shaped text-editing page", target)


if __name__ == "__main__":
    main()
