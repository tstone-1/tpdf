#!/usr/bin/env python3
"""Check text order, character geometry and pixels through PDFium's public API.

Run each library in a fresh process (PDFium has process-global state):
  uv run scripts/pdfium_rtl_check.py --lib /path/to/libpdfium.dylib \
      --fixtures scratch/pdfium-rtl/fixtures --record baseline.json
  uv run scripts/pdfium_rtl_check.py --lib /path/to/patched/libpdfium.dylib \
      --fixtures scratch/pdfium-rtl/fixtures --compare baseline.json

The unpatched regression is expected to exit 1. A record is an observation,
never a replacement for the generator's independently authored expected text.
The optional comparison requires unchanged rendered pixels and the same
multiset of (Unicode value, character box) pairs, even when indices reorder.
Windows accepts the full path to pdfium.dll instead of a dylib.
"""

import argparse
from collections import Counter
import ctypes as C
import hashlib
import json
from pathlib import Path


def bind(lib, name, result, *args):
    fn = getattr(lib, name)
    fn.restype = result
    fn.argtypes = args
    return fn


def read(lib_path: Path, path: Path):
    lib = C.CDLL(str(lib_path.resolve()))
    ptr, integer = C.c_void_p, C.c_int
    init = bind(lib, "FPDF_InitLibrary", None)
    destroy = bind(lib, "FPDF_DestroyLibrary", None)
    load = bind(lib, "FPDF_LoadMemDocument64", ptr, ptr, C.c_size_t, C.c_char_p)
    close = bind(lib, "FPDF_CloseDocument", None, ptr)
    page_load = bind(lib, "FPDF_LoadPage", ptr, ptr, integer)
    page_close = bind(lib, "FPDF_ClosePage", None, ptr)
    text_load = bind(lib, "FPDFText_LoadPage", ptr, ptr)
    text_close = bind(lib, "FPDFText_ClosePage", None, ptr)
    count = bind(lib, "FPDFText_CountChars", integer, ptr)
    unicode = bind(lib, "FPDFText_GetUnicode", C.c_uint, ptr, integer)
    box = bind(lib, "FPDFText_GetCharBox", integer, ptr, integer,
               *([C.POINTER(C.c_double)] * 4))
    get_text = bind(lib, "FPDFText_GetText", integer, ptr, integer, integer,
                    C.POINTER(C.c_ushort))
    bitmap_create = bind(lib, "FPDFBitmap_Create", ptr, integer, integer, integer)
    bitmap_fill = bind(lib, "FPDFBitmap_FillRect", None, ptr, integer, integer,
                       integer, integer, C.c_uint)
    bitmap_buffer = bind(lib, "FPDFBitmap_GetBuffer", ptr, ptr)
    bitmap_stride = bind(lib, "FPDFBitmap_GetStride", integer, ptr)
    bitmap_destroy = bind(lib, "FPDFBitmap_Destroy", None, ptr)
    render = bind(lib, "FPDF_RenderPageBitmap", None, ptr, ptr,
                  integer, integer, integer, integer, integer, integer)
    init()
    document = page = text = bitmap = None
    try:
        data = path.read_bytes()
        buffer = C.create_string_buffer(data)
        document = load(buffer, len(data), None)
        if not document:
            raise ValueError("PDFium could not load fixture")
        page = page_load(document, 0)
        if not page:
            raise ValueError("PDFium could not load page")
        text = text_load(page)
        if not text:
            raise ValueError("PDFium could not extract text")
        total = count(text)
        if not 0 < total < 10000:
            raise ValueError("Unexpected fixture character count")
        chars = []
        for index in range(total):
            coords = [C.c_double() for _ in range(4)]
            present = box(text, index, *[C.byref(c) for c in coords])
            chars.append([unicode(text, index),
                          [c.value for c in coords] if present else None])
        utf16 = (C.c_ushort * (total + 1))()
        length = get_text(text, 0, total, utf16)
        if not 0 < length <= total + 1 or utf16[length - 1] != 0:
            raise ValueError("Invalid text buffer result")
        decoded = bytes(utf16)[:(length - 1) * 2].decode("utf-16-le")
        bitmap = bitmap_create(600, 200, 0)
        if not bitmap:
            raise ValueError("PDFium could not allocate bitmap")
        bitmap_fill(bitmap, 0, 0, 600, 200, 0xFFFFFFFF)
        render(bitmap, page, 0, 0, 600, 200, 0, 0)
        size = bitmap_stride(bitmap) * 200
        pixels = C.string_at(bitmap_buffer(bitmap), size)
        return {"text": decoded, "chars": chars,
                "pixels_sha256": hashlib.sha256(pixels).hexdigest()}
    finally:
        if bitmap:
            bitmap_destroy(bitmap)
        if text:
            text_close(text)
        if page:
            page_close(page)
        if document:
            close(document)
        destroy()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lib", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--record", type=Path)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--case", action="append", default=[],
                        help="Fixture filename; repeat to select several cases")
    args = parser.parse_args()
    manifest = json.loads((args.fixtures / "manifest.json").read_text())
    previous = json.loads(args.compare.read_text()) if args.compare else None
    observed, failures = {}, []
    cases = manifest["cases"]
    if not cases or len({c["file"] for c in cases}) != len(cases):
        raise ValueError("Empty or duplicated fixture inventory")
    selected = set(args.case)
    if selected - {c["file"] for c in cases}:
        raise ValueError("Requested case is absent from fixture inventory")
    if selected:
        cases = [c for c in cases if c["file"] in selected]
    if previous is not None and set(previous) != {c["file"] for c in cases}:
        raise ValueError("Comparison inventory differs from fixture inventory")
    for case in cases:
        name = case["file"]
        path = args.fixtures / name
        if hashlib.sha256(path.read_bytes()).hexdigest() != case["sha256"]:
            raise ValueError("Fixture bytes differ from manifest")
        result = observed[name] = read(args.lib, path)
        problems = []
        if result["text"] != case["expected"]:
            problems.append("text order")
        if "".join(chr(c[0]) for c in result["chars"]) != result["text"]:
            problems.append("character index / text disagreement")
        if previous is not None:
            before = previous[name]
            if before["pixels_sha256"] != result["pixels_sha256"]:
                problems.append("rendered pixels changed")
            if Counter(map(json.dumps, before["chars"])) != Counter(map(json.dumps, result["chars"])):
                problems.append("character geometry changed")
        if problems:
            failures.append(name)
        print(f"[{'FAIL' if problems else 'OK'}] {name}: {', '.join(problems) or 'text, indices and comparison agree'}")
    if args.record:
        args.record.write_text(json.dumps(observed, indent=2) + "\n")
    print(f"{len(cases) - len(failures)}/{len(cases)} cases passed")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
