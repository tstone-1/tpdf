"""Read a file `tpdf redact` wrote with an independent PDF parser, against what should be gone.

Usage:
    TPDF_REDACT_PROBE=/tmp/tpdf-redact cargo test --manifest-path src-tauri/Cargo.toml --test cli
    uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts-redacted.pdf \
        /tmp/tpdf-redact/contacts-expected.json
    # The negative control: the unredacted input must be rejected.
    uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts.pdf \
        /tmp/tpdf-redact/contacts-expected.json --expect-fail
    uv run --with pypdf scripts/redact_pdf_check.py --self-test

pypdf extracts each page's text with its own content-stream interpreter and its
own font decoding, so a string it does not find is two readers agreeing that the
text layer lost it, not one reader agreeing with itself. It is also asked for
every allocated PDF object, including form values, widget appearances and orphan objects,
and every decoded stream's raw bytes, which is where a black rectangle drawn
over surviving text would give itself away: the words would still be operands of
a `Tj`. The expected file names the strings that must be `gone` and the controls
that must be `kept` --- the second half is what makes a run that removed
everything fail.
"""

import io
import json
import sys

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (
    ArrayObject,
    ByteStringObject,
    DecodedStreamObject,
    DictionaryObject,
    IndirectObject,
    NameObject,
    TextStringObject,
)


def carriers(reader):
    """Inspect all allocated objects, including orphan values and appearances.

    Page extraction does not visit AcroForm values or widget appearance streams.
    Walk the xref too: a removed field can leave its answer in an orphan object.
    Object-stream entries have a separate index in pypdf and belong here as well.
    """
    seen = set()

    def walk(value, where):
        if isinstance(value, IndirectObject):
            key = (value.idnum, value.generation)
            if key in seen:
                return
            seen.add(key)
            yield from walk(value.get_object(), f"object {key[0]} {key[1]}")
        elif isinstance(value, (TextStringObject, ByteStringObject)):
            yield where, str(value) if isinstance(value, TextStringObject) else bytes(value)
        elif isinstance(value, DictionaryObject):
            if hasattr(value, "get_data"):
                yield where + " decoded stream", value.get_data()
            for key, item in value.items():
                yield from walk(item, where + " " + str(key))
        elif isinstance(value, ArrayObject):
            for index, item in enumerate(value):
                yield from walk(item, f"{where}[{index}]")

    yield from walk(reader.trailer, "trailer")
    for generation, objects in reader.xref.items():
        if generation == 65535:
            continue
        for number in objects:
            if number:
                yield from walk(IndirectObject(number, generation, reader), "xref")
    for number in reader.xref_objStm:
        yield from walk(IndirectObject(number, 0, reader), "object stream")


def inspect(reader, expected):
    """Return surviving secrets separately from missing non-secret controls."""
    if not expected.get("gone") or not expected.get("kept"):
        raise ValueError("both gone strings and kept controls are required")
    text = "\n".join(page.extract_text() or "" for page in reader.pages)
    data = list(carriers(reader))
    survivors, missing = [], []
    for word in expected["gone"]:
        if not isinstance(word, str) or not word:
            raise ValueError("each gone string must be non-empty text")
        if word in text:
            survivors.append(f"pypdf's text still has {word!r}")
        encodings = {word.encode("utf-8"), word.encode("utf-16-be")}
        try:
            encodings.add(word.encode("latin-1"))
        except UnicodeEncodeError:
            pass
        for where, value in data:
            found = word in value if isinstance(value, str) else any(b in value for b in encodings)
            if found:
                survivors.append(f"{where} still carries {word!r}")
    for word in expected["kept"]:
        if not isinstance(word, str) or not word:
            raise ValueError("each kept string must be non-empty text")
        if word not in text:
            missing.append(f"pypdf's text lost the control {word!r}")
    return survivors, missing


def rejected_as_expected(survivors, missing):
    # A missing CONTROL-KEEP is not evidence that a surviving secret was found.
    return bool(survivors) and not missing


def self_test():
    """Prove page, field, appearance and orphan carriers on serialized PDFs."""
    expected = {"gone": ["ACME-SECRET"], "kept": ["CONTROL-KEEP"]}

    def fixture(kind):
        writer = PdfWriter()
        page = writer.add_blank_page(width=300, height=300)
        font = writer._add_object(DictionaryObject({
            NameObject("/Type"): NameObject("/Font"),
            NameObject("/Subtype"): NameObject("/Type1"),
            NameObject("/BaseFont"): NameObject("/Helvetica"),
        }))
        page[NameObject("/Resources")] = DictionaryObject({
            NameObject("/Font"): DictionaryObject({NameObject("/F1"): font}),
        })
        content = DecodedStreamObject()
        word = "ACME-SECRET" if kind in {"page", "secret-without-control"} else "CONTROL-KEEP"
        if kind == "page":
            word += " CONTROL-KEEP"
        if kind == "lost-control":
            word = "unrelated"
        content.set_data(f"BT /F1 12 Tf 20 200 Td ({word}) Tj ET".encode())
        page[NameObject("/Contents")] = writer._add_object(content)
        if kind == "field":
            field = writer._add_object(DictionaryObject({
                NameObject("/FT"): NameObject("/Tx"),
                NameObject("/T"): TextStringObject("ACME.field"),
                NameObject("/V"): TextStringObject("ACME-SECRET"),
            }))
            writer._root_object[NameObject("/AcroForm")] = DictionaryObject({
                NameObject("/Fields"): ArrayObject([field]),
            })
        if kind == "appearance":
            ap = DecodedStreamObject()
            ap.set_data(b"BT /F1 12 Tf (ACME-SECRET) Tj ET")
            widget = writer._add_object(DictionaryObject({
                NameObject("/Subtype"): NameObject("/Widget"),
                NameObject("/AP"): DictionaryObject({NameObject("/N"): writer._add_object(ap)}),
            }))
            page[NameObject("/Annots")] = ArrayObject([widget])
        if kind == "orphan":
            writer._add_object(TextStringObject("ACME-SECRET"))
        buffer = io.BytesIO()
        writer.write(buffer)
        buffer.seek(0)
        return PdfReader(buffer)

    for kind in ["page", "field", "appearance", "orphan"]:
        survivors, missing = inspect(fixture(kind), expected)
        assert rejected_as_expected(survivors, missing), (kind, survivors, missing)
    assert inspect(fixture("clean"), expected) == ([], [])
    survivors, missing = inspect(fixture("lost-control"), expected)
    assert not survivors and missing and not rejected_as_expected(survivors, missing)
    survivors, missing = inspect(fixture("secret-without-control"), expected)
    assert survivors and missing and not rejected_as_expected(survivors, missing)
    print("[PASS] page, field, appearance and orphan survivors rejected; clean output accepted; lost control fails")
    return 0


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    expect_fail = "--expect-fail" in sys.argv[1:]
    if len(args) != 2:
        print(__doc__)
        return 2
    path, expected_path = args
    with open(expected_path, encoding="utf-8") as f:
        expected = json.load(f)

    reader = PdfReader(path)
    survivors, missing = inspect(reader, expected)
    problems = survivors + missing

    for problem in problems:
        print(f"[FAIL] {problem}")
    if expect_fail:
        if rejected_as_expected(survivors, missing):
            print(f"[PASS] surviving secret rejected, with every control kept: {len(survivors)} finding(s)")
            return 0
        print("[FAIL] negative control needs a surviving secret and every kept control")
        return 1
    if problems:
        return 1
    print(
        f"[PASS] {len(expected['gone'])} string(s) gone and {len(expected['kept'])} "
        f"control(s) kept across {len(reader.pages)} page(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
