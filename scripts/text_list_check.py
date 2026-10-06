#!/usr/bin/env python3
"""Edit a LibreOffice list in paragraph styles of its own, and read it back three ways.

uv run --with pypdf scripts/text_list_check.py <text-edit-probe> <tpdf-cli> <new-directory>
    [--source <export.pdf>]

The source is the committed export of testdata/textedit-producer-list-styles.fodt,
src-tauri/src/textedit/tagging/fixtures/libreoffice-list.pdf, a synthetic document
whose every name is invented (docs/VERIFICATION.md, *A LibreOffice list in paragraph styles of
its own*, has the export command). --source takes another export of the same file,
for example one made by another LibreOffice version; its digest is not checked.

Four runs are edited through `text-edit-probe --roundtrip`, which holds the
worker's preview and its saved page to the same pixels and everything outside
the edits to the pixels it had: a word in a list paragraph, an item's bold first
words, a bullet of the sublist, and a centred line (with the layout the editor
opens, in the document's own font). The saved copy is then read by three readers
that share no code with the editor's scanner:

  pypdf    the page's text, the whole structure graph below the root, the role
           map, the parent tree and the embedded font programs
  PDFium   `tpdf-cli text`
  PDFKit   on macOS, when `swift` is on the PATH

Each must show the four replacements and nothing else changed. Before any of that
the comparisons are run where they must fail: the unedited source read as the
result, and a copy whose list paragraph has been renamed. A check that passes on
those has checked nothing.

Exit 0 only when every reader agrees. Run it from outside the checkout's
environment (`uv run` makes none here: it needs `--with pypdf` and nothing else).
"""
import argparse
import hashlib
import json
import math
import platform
import shutil
import subprocess
import sys
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import ArrayObject, DictionaryObject, IndirectObject, NameObject, StreamObject

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "src-tauri/src/textedit/tagging/fixtures/libreoffice-list.pdf"
DIGEST = "8f7e3f4514c568989892003a3170af06ff9134c7d826347067777e95a5fe62fb"
# (a substring naming one run, what replaces it, whether the run is centred)
EDITS = [
    ("without exception", "without objection", False),
    ("maker", "rules", False),
    ("Rule two", "Rule six", False),
    ("Declaration", "Exception", True),
]
SWIFT = """import Foundation
import PDFKit
guard let document = PDFDocument(url: URL(fileURLWithPath: CommandLine.arguments[1])),
      let text = document.string else { exit(2) }
print(text)
"""


def fail(message):
    print(f"[FAIL] {message}")
    sys.exit(1)


def words(text):
    return " ".join(text.split())


def expected(text):
    for old, new, _ in EDITS:
        if text.count(old) != 1:
            raise AssertionError(f"the source does not hold {old!r} exactly once")
        text = text.replace(old, new)
    return text


def canonical(value, pages, depth=0):
    """A structure value with page references as page numbers and no /P edges."""
    if depth > 64:
        raise AssertionError("structure deeper than 64 levels")
    if isinstance(value, IndirectObject):
        if value.idnum in pages:
            return ("page", pages[value.idnum])
        value = value.get_object()
    if isinstance(value, StreamObject):
        return ("stream", hashlib.sha256(value.get_data()).hexdigest())
    if isinstance(value, DictionaryObject):
        return {str(key): canonical(item, pages, depth + 1)
                for key, item in value.items() if key != "/P"}
    if isinstance(value, ArrayObject):
        return [canonical(item, pages, depth + 1) for item in value]
    if isinstance(value, float):
        return round(float(value), 4)
    return str(value) if isinstance(value, NameObject) else value


def describe(path):
    reader = PdfReader(str(path))
    pages = {page.indirect_reference.idnum: index for index, page in enumerate(reader.pages)}
    root = reader.trailer["/Root"]["/StructTreeRoot"]
    fonts = {}
    for page in reader.pages:
        for name, font in page["/Resources"]["/Font"].items():
            program = font.get_object()["/FontDescriptor"].get("/FontFile2")
            if program is not None:
                fonts[str(font.get_object()["/BaseFont"])] = hashlib.sha256(
                    program.get_object().get_data()).hexdigest()
    return {
        "text": words(reader.pages[0].extract_text()),
        "structure": canonical(root, pages),
        "fonts": fonts,
        "annotations": [canonical(annot, pages) for annot in reader.pages[0].get("/Annots", [])],
        "pages": len(reader.pages),
    }


def compare(before, after):
    """Why `after` is not `before` with the four edits made, or None."""
    if after["text"] != expected(before["text"]):
        return "the page's text is not the source's with the four replacements"
    for part in ("structure", "fonts", "annotations", "pages"):
        if after[part] != before[part]:
            return f"the {part} changed"
    if len(before["fonts"]) < 2 or not before["structure"].get("/RoleMap"):
        return "the source has no role map or fewer than two embedded fonts"
    return None


def reads(text, source_text, reader):
    """Why a second reader's text is not the edited document's, or None."""
    text, source_text = words(text), words(source_text)
    for old, new, _ in EDITS:
        if source_text.count(old) != 1:
            return f"{reader}: the source does not show {old!r} exactly once"
        if text.count(new) != source_text.count(new) + 1 or old in text:
            return f"{reader}: {new!r} did not replace {old!r}"
    if text != expected(source_text):
        return f"{reader}: text outside the four replacements changed"
    return None


def run(*command):
    done = subprocess.run([str(part) for part in command], capture_output=True, text=True)
    if done.returncode != 0:
        fail(f"{Path(str(command[0])).name} exited {done.returncode}: {done.stderr.strip()[-400:]}")
    return done.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("probe", type=lambda value: Path(value).absolute())
    parser.add_argument("cli", type=lambda value: Path(value).absolute())
    parser.add_argument("directory", type=Path)
    parser.add_argument("--source", type=Path)
    options = parser.parse_args()
    source = options.source or FIXTURE
    if options.source is None and hashlib.sha256(source.read_bytes()).hexdigest() != DIGEST:
        fail("the committed export is not the one this check was written for")
    if options.directory.exists():
        fail(f"{options.directory} exists; name a new directory")
    options.directory.mkdir(parents=True)

    found = json.loads(run(options.cli, "text-runs", "--json", source))
    requests = []
    for old, new, centred in EDITS:
        runs = [item for item in found["runs"] if old in item["text"]]
        if len(runs) != 1:
            fail(f"{old!r} names {len(runs)} runs, not one")
        request = {"page": 0, "contains": old, "replacement": new, "replace_match": True}
        if centred:
            # The box the editor opens on the run (`Layout::opened`).
            up = lambda value: math.ceil(value * 1000) / 1000
            size = runs[0]["size"]
            request["layout"] = {
                "width": max(up(runs[0]["advance"]), 0.1), "height": up(size * 1.25),
                "size": up(size), "wrap": False, "font": "original", "grow": True}
        requests.append(request)
    plan = options.directory / "requests.json"
    plan.write_text(json.dumps(requests), encoding="utf-8")
    result = options.directory / "result"
    run(options.probe, "--roundtrip", source, plan, result)
    edited = result / "edited.pdf"
    if not edited.is_file():
        fail("the round trip wrote no edited.pdf")
    print(f"[OK] {len(found['runs'])} runs offered; four edited; preview, save and "
          "untouched pixels agree")

    before, after = describe(source), describe(edited)
    # Where the comparisons must fail.
    if compare(before, before) is None:
        fail("the unedited source passed as the result")
    renamed = options.directory / "renamed.pdf"
    writer = PdfWriter(clone_from=str(edited))
    styles = [obj for obj in writer._objects
              if isinstance(obj, DictionaryObject) and obj.get("/S") == "/Example Numbers"]
    if not styles:
        fail("the export has no list paragraph under its style's name")
    styles[0][NameObject("/S")] = NameObject("/P")
    writer.write(str(renamed))
    why = compare(before, describe(renamed))
    if why != "the structure changed":
        fail(f"a renamed list paragraph was not refused for its structure: {why}")
    source_text = run(options.cli, "text", source)
    if reads(source_text, source_text, "PDFium") is None:
        fail("the unedited source's text passed as the result's")
    print("[OK] the unedited source and a renamed list paragraph are both refused")

    why = compare(before, after)
    if why:
        fail(f"pypdf: {why}")
    print(f"[OK] pypdf: text, {sum(1 for _ in walk(after['structure']))} structure values, "
          f"{len(after['fonts'])} font programs and the annotations")
    why = reads(run(options.cli, "text", edited), source_text, "PDFium")
    if why:
        fail(why)
    print("[OK] PDFium: the four replacements and no other change")
    if platform.system() == "Darwin" and shutil.which("swift"):
        script = options.directory / "read.swift"
        script.write_text(SWIFT, encoding="utf-8")
        why = reads(run("swift", script, edited), run("swift", script, source), "PDFKit")
        if why:
            fail(why)
        print("[OK] PDFKit: the four replacements and no other change")
    else:
        print("[SKIP] PDFKit: needs macOS and swift")
    print("[OK] a LibreOffice list in paragraph styles of its own is edited and read back")


def walk(value):
    yield value
    if isinstance(value, dict):
        for item in value.values():
            yield from walk(item)
    elif isinstance(value, list):
        for item in value:
            yield from walk(item)


if __name__ == "__main__":
    main()
