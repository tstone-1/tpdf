#!/usr/bin/env python3
"""Independent browser fixture readback, with explicit float32 resource semantics.

uv run --with pypdf scripts/text_edit_browser_check.py before.pdf after.pdf --controls
Controls change graphics state, clipping, positioning and font data. Distinct
adjacent float32 values must remain distinguishable; this is not a tolerance test.
"""
import argparse
import contextlib
import io
from pathlib import Path
import struct
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "testdata"))
from make_textedit_embedded import check


def next_float32(value):
    bits = struct.unpack(">I", struct.pack(">f", float(value)))[0]
    assert 0 < bits < 0x7F7FFFFF, "control needs a positive finite value"
    return struct.unpack(">f", struct.pack(">I", bits + 1))[0]


def controls(before, after):
    from pypdf import PdfWriter
    from pypdf.generic import ContentStream, FloatObject, NameObject, NumberObject

    # The normal checker must still expose decimal normalization, instead of
    # silently adopting this mode for every existing fixture.
    try:
        check(before, after)
    except AssertionError as error:
        assert str(error) == "page resources changed", str(error)
    else:
        raise AssertionError("browser fixture no longer exercises decimal normalization")
    print("[PASS] exact comparison exposes browser decimal normalization")
    with tempfile.TemporaryDirectory(prefix="tpdf-browser-controls-") as directory:
        for mode in ("alpha", "blend", "mask", "font-number", "font-bytes",
                     "clip-shift", "clip-removed", "transform-number", "second-line"):
            writer = PdfWriter(clone_from=after)
            page = writer.pages[0]
            resources = page["/Resources"]
            expected = "page resources changed"
            if mode in ("alpha", "blend", "mask"):
                state = next(iter(resources["/ExtGState"].values())).get_object()
                key, value = {"alpha": ("/ca", FloatObject(0.5)),
                              "blend": ("/BM", NameObject("/Multiply")),
                              "mask": ("/SMask", NameObject("/None"))}[mode]
                state[NameObject(key)] = value
            elif mode in ("font-number", "font-bytes"):
                font = next(iter(resources["/Font"].values())).get_object()
                descriptor = font["/DescendantFonts"][0].get_object()["/FontDescriptor"]
                if mode == "font-number":
                    descriptor[NameObject("/CapHeight")] = FloatObject(next_float32(descriptor["/CapHeight"]))
                else:
                    from pypdf.generic import DecodedStreamObject
                    old = descriptor["/FontFile2"]
                    data = old.get_data()
                    changed = DecodedStreamObject()
                    for key, value in old.items():
                        if key not in ("/Length", "/Filter", "/DecodeParms"):
                            changed[key] = value
                    changed.set_data(data[:-1] + bytes([data[-1] ^ 1]))
                    descriptor[NameObject("/FontFile2")] = writer._add_object(changed)
            else:
                content = ContentStream(page["/Contents"], writer)
                expected = "expected exactly one changed operand"
                if mode.startswith("clip"):
                    index = next(i for i, (_, op) in enumerate(content.operations) if op == b"re")
                    if mode == "clip-shift":
                        values = content.operations[index][0]
                        values[0] = FloatObject(float(values[0]) + 1)
                    else:
                        assert content.operations[index + 1][1] == b"W*"
                        assert content.operations[index + 2][1] == b"n"
                        del content.operations[index:index + 3]
                        expected = "operator count changed"
                elif mode == "transform-number":
                    values = next(args for args, op in content.operations if op == b"cm")
                    values[0] = FloatObject(next_float32(values[0]))
                else:
                    matrices = [args for args, op in content.operations if op == b"Tm"]
                    assert len(matrices) == 2
                    matrices[1][5] = NumberObject(79)
                page[NameObject("/Contents")] = writer._add_object(content)
            target = Path(directory) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, float32=True)
            except AssertionError as error:
                assert str(error) == expected, (mode, str(error))
            else:
                raise AssertionError("corruption control survived: " + mode)
            print("[PASS] independent browser corruption control:", mode)


def tagged_controls(before, after):
    from pypdf import PdfWriter
    from pypdf.generic import NameObject, NumberObject, TextStringObject

    with tempfile.TemporaryDirectory(prefix="tpdf-browser-tags-") as directory:
        for mode in ("parent", "reverse-parent", "mcid", "role", "language", "actual-text", "next-key", "removed-tree"):
            writer = PdfWriter(clone_from=after)
            root = writer.root_object["/StructTreeRoot"]
            document = root["/K"]
            paragraph = document["/K"][0].get_object()
            leaf = paragraph["/K"]
            if mode == "parent":
                leaf[NameObject("/P")] = document.indirect_reference
            elif mode == "reverse-parent":
                entries = root["/ParentTree"]["/Nums"][1].get_object()
                entries[0], entries[1] = entries[1], entries[0]
            elif mode == "mcid":
                leaf[NameObject("/K")] = NumberObject(1)
            elif mode == "role":
                leaf[NameObject("/S")] = NameObject("/Span")
            elif mode == "language":
                document[NameObject("/Lang")] = TextStringObject("de")
            elif mode == "actual-text":
                leaf[NameObject("/ActualText")] = TextStringObject("OLD TEXT")
            elif mode == "next-key":
                root[NameObject("/ParentTreeNextKey")] = NumberObject(0)
            else:
                del writer.root_object["/StructTreeRoot"]
            target = Path(directory) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, float32=True)
            except AssertionError as error:
                expected = "tagged structure disappeared" if mode == "removed-tree" else "tagged structure or parent references changed"
                assert str(error) == expected, (mode, str(error))
            else:
                raise AssertionError("tagged corruption control survived: " + mode)
            print("[PASS] independent tagged browser corruption control:", mode)


def flow_controls(before, after, page_index):
    """Corrupt page flow, its reverse ownership, and the supposedly untouched page."""
    from pypdf import PdfWriter
    from pypdf.generic import NameObject, NumberObject

    with tempfile.TemporaryDirectory(prefix="tpdf-browser-flow-") as directory:
        for mode in ("mcr-page", "missing-item", "page-key", "reverse-parent", "other-page"):
            writer = PdfWriter(clone_from=after)
            root = writer.root_object["/StructTreeRoot"]
            paragraph = root["/K"]["/K"]
            leaf = paragraph["/K"]
            items = leaf["/K"]
            assert len(items) == 2 and items[1]["/Type"] == "/MCR", "unexpected flow fixture structure"
            expected = "tagged structure or parent references changed"
            if mode == "mcr-page":
                items[1][NameObject("/Pg")] = writer.pages[0].indirect_reference
            elif mode == "missing-item":
                del items[1]
            elif mode == "page-key":
                writer.pages[1][NameObject("/StructParents")] = NumberObject(0)
            elif mode == "reverse-parent":
                root["/ParentTree"]["/Nums"][3].get_object()[0] = paragraph.indirect_reference
            else:
                from pypdf.generic import DecodedStreamObject
                other = writer.pages[1 - page_index]
                content = DecodedStreamObject()
                content.set_data(other.get_contents().get_data() + b"\n% changed untouched page\n")
                other[NameObject("/Contents")] = writer._add_object(content)
                expected = "untouched page content changed"
            target = Path(directory) / (mode + ".pdf")
            writer.write(target)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    check(before, target, page_index=page_index, float32=True)
            except AssertionError as error:
                assert str(error) == expected, (mode, str(error))
            else:
                raise AssertionError("flow corruption control survived: " + mode)
            print("[PASS] independent browser flow corruption control:", mode)


def main():
    from pypdf import PdfReader

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--controls", action="store_true")
    parser.add_argument("--tagged", action="store_true")
    parser.add_argument("--flow", action="store_true", help="check the two-page naturally wrapped fixture")
    parser.add_argument("--page", type=int, default=0, help="zero-based edited page")
    args = parser.parse_args()
    assert args.flow or args.page == 0, "page selection requires the flow fixture"
    assert not (args.flow and args.controls and not args.tagged), "flow controls require tagged input"
    for path in (args.before, args.after):
        reader = PdfReader(path)
        assert len(reader.pages) == (2 if args.flow else 1), "wrong browser fixture page count"
        assert ("/StructTreeRoot" in reader.trailer["/Root"]) == args.tagged, "browser fixture tagging differs"
    check(args.before, args.after, page_index=args.page, float32=True)
    if args.controls:
        if args.flow:
            flow_controls(args.before, args.after, args.page)
        else:
            controls(args.before, args.after)
            if args.tagged:
                tagged_controls(args.before, args.after)


if __name__ == "__main__":
    main()
