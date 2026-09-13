#!/usr/bin/env python3
"""Generate the original MIT-licensed subset used by worker/editor tests.

uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py
uv run --with pypdf testdata/make_textedit_embedded.py --check before.pdf after.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls before.pdf after.pdf
Writes a deterministic test font and an ignored PDF with the native UI fixture's
geometry. No installed or third-party font is read.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from text_edit_fonts import make_font, pdf_round_trip


def check(before, after):
    """Independent parser: one changed operand, identical fonts and colour data."""
    from pypdf import PdfReader
    from pypdf.generic import ContentStream, DictionaryObject, StreamObject

    def value(obj):
        obj = obj.get_object()
        if isinstance(obj, StreamObject):
            return ({str(k): value(v) for k, v in obj.items() if k not in ("/Length", "/Filter")}, obj.get_data())
        if isinstance(obj, DictionaryObject):
            return {str(k): value(v) for k, v in obj.items()}
        if isinstance(obj, list):
            return [value(v) for v in obj]
        return obj

    readers = [PdfReader(path) for path in (before, after)]
    assert all(len(reader.pages) == 1 for reader in readers), "wrong page count"
    pages = [reader.pages[0] for reader in readers]
    assert value(pages[0]["/Resources"]) == value(pages[1]["/Resources"]), "page resources changed"
    if "/StructTreeRoot" in readers[0].trailer["/Root"]:
        # Structure is cyclic through /P and references the page. Compare its
        # entire graph, independent of object renumbering, with the page as an
        # explicit leaf (its one changed content operand is checked below).
        from pypdf.generic import IndirectObject

        def structure(reader):
            identities, nodes = {}, []
            page_ref = reader.pages[0].indirect_reference

            def visit(obj):
                if isinstance(obj, IndirectObject):
                    if obj == page_ref:
                        return ("page", 0)
                    identity = (obj.idnum, obj.generation)
                    if identity in identities:
                        return ("ref", identities[identity])
                    index = identities[identity] = len(nodes)
                    nodes.append(None)
                    assert len(nodes) <= 256, "structure graph exceeds fixture limit"
                    nodes[index] = visit(obj.get_object())
                    return ("ref", index)
                if isinstance(obj, DictionaryObject):
                    return {str(k): visit(obj.raw_get(k)) for k in sorted(obj)}
                if isinstance(obj, list):
                    return [visit(v) for v in obj]
                return obj

            root = reader.trailer["/Root"]
            assert "/StructTreeRoot" in root, "tagged structure disappeared"
            top = visit(root.raw_get("/StructTreeRoot"))
            mark_info = value(root["/MarkInfo"]) if "/MarkInfo" in root else None
            return (top, nodes, reader.pages[0].get("/StructParents"), mark_info)

        assert structure(readers[0]) == structure(readers[1]), "tagged structure or parent references changed"
        print("[PASS] independent parser: complete tagged structure graph and page parent key preserved")
    operations = [ContentStream(page["/Contents"], reader).operations
                  for page, reader in zip(pages, readers)]
    assert len(operations[0]) == len(operations[1]), "operator count changed"
    changes = [(old, new) for old, new in zip(*operations) if old != new]
    assert len(changes) == 1, "expected exactly one changed operand"
    old, new = changes[0]
    assert old[1] == new[1] and old[1] in (b"Tj", b"TJ"), "text-show operator changed"
    assert len(old[0]) == len(new[0]) == 1, "wrong text-show operand count"
    if old[1] == b"TJ":
        assert isinstance(old[0][0], list), "wrong source array"
        assert isinstance(new[0][0], list) and len(new[0][0]) == 1, "wrong replacement array"
        assert isinstance(new[0][0][0], (str, bytes)), "replacement array contains an adjustment"
    else:
        assert isinstance(new[0][0], (str, bytes)), "wrong replacement operand"
    fonts = list(pages[0]["/Resources"]["/Font"].values())
    symbolic = [font.get_object() for font in fonts
                if "/Encoding" not in font.get_object() and "/ToUnicode" in font.get_object()]
    if symbolic:
        assert len(fonts) == len(symbolic) == 1, "symbolic readback requires a single fixture font"
        # extract_text() deliberately falls back to identity for unmapped codes.
        # Read pypdf's parsed map explicitly so fallback cannot pass this check.
        from pypdf._cmap import get_encoding
        _, mapping = get_encoding(symbolic[0])
        assert mapping and all(isinstance(k, str) and len(k) == len(v) == 1 for k, v in mapping.items()), "unexpected fixture map"
        for operation, expected in [(old, "SYNTHETIC FIRST"), (new, "EDITED FIRST")]:
            parts = operation[0][0] if operation[1] == b"TJ" else operation[0]
            raw = b"".join(part.original_bytes if isinstance(part, str) else bytes(part)
                           for part in parts if isinstance(part, (str, bytes)))
            assert all(chr(code) in mapping for code in raw), "unmapped symbolic code"
            assert "".join(mapping[chr(code)] for code in raw) == expected, "wrong mapped operand"
    # Let the independent parser apply the font's encoding and ToUnicode map.
    # Comparing raw operand bytes to ASCII cannot verify symbolic font codes.
    for page, first in zip(pages, ("SYNTHETIC FIRST", "EDITED FIRST")):
        assert " ".join(page.extract_text().split()) == first + " SYNTHETIC SECOND", "wrong decoded text"
    print("[PASS] independent parser: only target text operand changed; font and colour resources preserved")


def tagged_controls(before, after):
    """Prove nonvisual structure corruption fails the independent readback."""
    import contextlib
    import io
    import tempfile
    from pypdf import PdfWriter
    from pypdf.generic import NameObject, NumberObject, TextStringObject

    check(before, after)
    with tempfile.TemporaryDirectory(prefix="tpdf-tagged-controls-") as room:
        for mode in ("root", "parent", "mcid", "alternate", "page_key"):
            writer = PdfWriter(clone_from=after)
            root = writer.root_object["/StructTreeRoot"]
            paragraph = root["/K"][0].get_object()["/K"][0].get_object()
            if mode == "root":
                del writer.root_object["/StructTreeRoot"]
            elif mode == "parent":
                paragraph[NameObject("/P")] = root.indirect_reference
            elif mode == "mcid":
                paragraph["/K"][0] = NumberObject(1)
            elif mode == "alternate":
                paragraph[NameObject("/ActualText")] = TextStringObject("STALE SYNTHETIC TEXT")
            else:
                writer.pages[0][NameObject("/StructParents")] = NumberObject(1)
            target = Path(room) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target)
            except AssertionError as error:
                assert "tagged structure" in str(error), str(error)
            else:
                raise AssertionError("structure control survived: " + mode)
            print("[PASS] independent structure control:", mode)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--check":
        check(*sys.argv[2:])
        return
    if len(sys.argv) == 4 and sys.argv[1] == "--tagged-controls":
        tagged_controls(*sys.argv[2:])
        return
    if len(sys.argv) != 1:
        raise SystemExit("expected no arguments, --check or --tagged-controls before.pdf after.pdf")
    from pypdf import PdfReader, PdfWriter
    from pypdf.generic import ArrayObject, DecodedStreamObject, NameObject, NumberObject

    data = make_font(characters="AB SYNTHETIC FIRST EDITED SECOND")
    (ROOT / "src-tauri/src/textedit/synthetic.ttf").write_bytes(data)
    target = ROOT / "testdata/textedit-embedded.pdf"
    pdf_round_trip(data, "ttf", target)
    writer = PdfWriter(clone_from=PdfReader(target))
    page = writer.pages[0]
    page[NameObject("/MediaBox")] = ArrayObject([NumberObject(v) for v in (0, 0, 300, 240)])
    font = page["/Resources"]["/Font"]["/F1"]
    font[NameObject("/LastChar")] = NumberObject(89)
    font[NameObject("/Widths")] = ArrayObject([NumberObject(600)] * 58)
    content = DecodedStreamObject()
    content.set_data(b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\n"
                     b"BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET")
    page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(target)
    print("[PASS] generated synthetic TrueType subset and editor PDF")


if __name__ == "__main__":
    main()
