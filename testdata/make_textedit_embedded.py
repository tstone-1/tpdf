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


def check(before, after, page_index=0, wrapped=False, float32=False, cid_latin1=False, overhang=False, default_encoding=False):
    """Independent parser: one changed operand, identical fonts and colour data."""
    from pypdf import PdfReader
    from pypdf.generic import ContentStream, DictionaryObject, StreamObject, FloatObject

    def value(obj):
        if isinstance(obj, tuple):
            return [value(v) for v in obj]
        obj = obj.get_object() if hasattr(obj, "get_object") else obj
        if isinstance(obj, list):
            return [value(v) for v in obj]
        if isinstance(obj, StreamObject):
            return ({str(k): value(v) for k, v in obj.items() if k not in ("/Length", "/Filter")}, obj.get_data())
        if isinstance(obj, DictionaryObject):
            return {str(k): value(v) for k, v in obj.items()}
        if float32 and isinstance(obj, FloatObject):
            # Explicit opt-in for lopdf's Real storage. Default checks retain
            # exact numeric comparison. Streams and nonnumeric values stay exact.
            import math
            import struct
            number = struct.unpack(">f", struct.pack(">f", float(obj)))[0]
            assert math.isfinite(number), "nonfinite float32 value"
            return number
        return obj

    readers = [PdfReader(path) for path in (before, after)]
    count = len(readers[0].pages)
    assert 0 <= page_index < count <= 128 and len(readers[1].pages) == count, "wrong page count"
    for index, (old_page, new_page) in enumerate(zip(readers[0].pages, readers[1].pages)):
        assert value(old_page["/Resources"]) == value(new_page["/Resources"]), "page resources changed"
        if index != page_index:
            assert old_page.get_contents().get_data() == new_page.get_contents().get_data(), "untouched page content changed"
            assert old_page.extract_text() == new_page.extract_text(), "untouched page text changed"
    pages = [reader.pages[page_index] for reader in readers]
    if "/StructTreeRoot" in readers[0].trailer["/Root"]:
        # Structure is cyclic through /P and references the page. Compare its
        # entire graph, independent of object renumbering, with the page as an
        # explicit leaf (its one changed content operand is checked below).
        from pypdf.generic import IndirectObject

        def structure(reader):
            identities, nodes = {}, []
            page_refs = {(p.indirect_reference.idnum, p.indirect_reference.generation): index for index, p in enumerate(reader.pages)}

            def visit(obj):
                if isinstance(obj, IndirectObject):
                    identity = (obj.idnum, obj.generation)
                    if identity in page_refs:
                        return ("page", page_refs[identity])
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
            return (top, nodes, [p.get("/StructParents") for p in reader.pages], mark_info)

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
        for operation, expected in [(old, "SYNTHETIC FIRST" + (" " if wrapped else "")), (new, "EDITED FIRST")]:
            parts = operation[0][0] if operation[1] == b"TJ" else operation[0]
            raw = b"".join(part.original_bytes if isinstance(part, str) else bytes(part)
                           for part in parts if isinstance(part, (str, bytes)))
            assert all(chr(code) in mapping for code in raw), "unmapped symbolic code"
            assert "".join(mapping[chr(code)] for code in raw) == expected, "wrong mapped operand"
    # Let the independent parser apply the font's encoding and ToUnicode map.
    # Comparing raw operand bytes to ASCII cannot verify symbolic font codes.
    expected_text = ("SYNTHETIC ÄÖÜ äöü ß", "ÖÄÜ äöü ß" if overhang else "ÄÖÜ äöü ß") if cid_latin1 or overhang else ("SYNTHETIC FIRST", "EDITED FIRST")
    if default_encoding:
        expected_text = ("SYNTHETIC ' ` £ ß", "£ ' ` ß")
    for page, first in zip(pages, expected_text):
        assert " ".join(page.extract_text().split()) == first + " SYNTHETIC SECOND", "wrong decoded text"
    if float32:
        print("[PASS] independent parser: only target text operand changed; resources agree at float32 precision with exact stream bytes")
    else:
        print("[PASS] independent parser: only target text operand changed; font and colour resources preserved")


def tagged_controls(before, after, page_index=0, wrapped=False):
    """Prove nonvisual structure corruption fails the independent readback."""
    import contextlib
    import io
    import tempfile
    from pypdf import PdfWriter
    from pypdf.generic import DecodedStreamObject, NameObject, NumberObject, TextStringObject

    check(before, after, page_index, wrapped)
    with tempfile.TemporaryDirectory(prefix="tpdf-tagged-controls-") as room:
        sample = PdfWriter(clone_from=after)
        paragraphs = sample.root_object["/StructTreeRoot"]["/K"][0].get_object()["/K"]
        modes = ["root", "parent", "mcid", "alternate", "page_key"]
        if any("/EndIndent" in p.get_object().get("/A", {}) for p in paragraphs):
            modes.append("end_indent")
        if len(sample.pages) > 1:
            modes += ["page_owner", "other_page"]
        if any(len(p.get_object()["/K"]) > 1 for p in paragraphs):
            modes += ["item_order", "item_missing"]
        if any(isinstance(item, dict) for p in paragraphs for item in p.get_object()["/K"]):
            modes += ["mcr_page", "mcr_id"]
        for mode in modes:
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
            elif mode == "end_indent":
                attrs = next(p.get_object()["/A"] for p in root["/K"][0].get_object()["/K"] if "/EndIndent" in p.get_object().get("/A", {}))
                del attrs["/EndIndent"]
            elif mode == "page_key":
                writer.pages[0][NameObject("/StructParents")] = NumberObject(1)
            elif mode == "page_owner":
                old = paragraph.raw_get("/Pg")
                paragraph[NameObject("/Pg")] = next(p.indirect_reference for p in writer.pages if p.indirect_reference != old)
            elif mode in ("item_order", "item_missing"):
                items = next(p.get_object()["/K"] for p in root["/K"][0].get_object()["/K"] if len(p.get_object()["/K"]) > 1)
                if mode == "item_order":
                    items.reverse()
                else:
                    items.pop()
            elif mode in ("mcr_page", "mcr_id"):
                mcr = next(item for p in root["/K"][0].get_object()["/K"] for item in p.get_object()["/K"] if isinstance(item, dict))
                if mode == "mcr_page":
                    old = mcr.raw_get("/Pg")
                    mcr[NameObject("/Pg")] = next(p.indirect_reference for p in writer.pages if p.indirect_reference != old)
                else:
                    mcr[NameObject("/MCID")] = NumberObject(int(mcr["/MCID"]) + 1)
            else:
                other = next(p for index, p in enumerate(writer.pages) if index != page_index)
                stream = DecodedStreamObject()
                stream.set_data(other.get_contents().get_data() + b"\n% changed untouched page\n")
                other[NameObject("/Contents")] = writer._add_object(stream)
            target = Path(room) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, page_index, wrapped)
            except AssertionError as error:
                expected = "untouched page content" if mode == "other_page" else "tagged structure"
                assert expected in str(error), str(error)
            else:
                raise AssertionError("structure control survived: " + mode)
            print("[PASS] independent structure control:", mode)


def main():
    if len(sys.argv) >= 4 and sys.argv[1] in ("--check", "--tagged-controls"):
        page, wrapped, float32, default_encoding = 0, False, False, False
        for option in sys.argv[4:]:
            if option.startswith("--page="):
                page = int(option.split("=", 1)[1])
            elif option == "--wrapped":
                wrapped = True
            elif option == "--default-encoding" and sys.argv[1] == "--check":
                default_encoding = True
            elif option == "--float32" and sys.argv[1] == "--check":
                float32 = True
            else:
                raise SystemExit("expected --page=N (zero based), --wrapped, --float32 or --default-encoding (--check only)")
        action = check if sys.argv[1] == "--check" else tagged_controls
        if float32 or default_encoding:
            check(*sys.argv[2:4], page, wrapped, float32=float32, default_encoding=default_encoding)
        else:
            action(*sys.argv[2:4], page, wrapped)
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
