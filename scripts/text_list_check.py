#!/usr/bin/env python3
"""Edit a LibreOffice list in paragraph styles of its own, and read it back three ways.

uv run --with pypdf --with pdfplumber scripts/text_list_check.py <text-edit-probe> <tpdf-cli>
    <new-directory> [--source <export.pdf>]

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

Then three list items that are full to the page are given a sentence more, one
round trip each, since each wraps onto a new line and moves what is below it: an
item of one line, an item above a bulleted sublist, and an item whose paragraph
ends a later line at a hyphen its producer added. What is below is partly text
the editor cannot rewrite (bullets, that hyphen, a justified paragraph), and it
has to move with the rest. Each saved copy is read by the same three readers:
the item's text with the sentence after it, and every other line of the page as
it was and in the order it was. `text_wrap_check.py --compare` then pairs every
glyph before and after as pdfplumber reads them: a glyph stayed or moved straight
down with its line, the page gained exactly the glyphs the sentence adds, and no
more pairs of glyphs overlap than did before. Those comparisons are run where
they must fail as well: on the unedited source, and on a reading with two lines
below the edit exchanged.

Exit 0 only when every reader agrees. Run it from outside the checkout's
environment (`uv run` makes none here: it needs `--with pypdf --with pdfplumber`
and nothing else).
"""
import argparse
import hashlib
import json
import math
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import ArrayObject, DictionaryObject, IndirectObject, NameObject, StreamObject

sys.path.insert(0, str(Path(__file__).resolve().parent))
import text_wrap_check  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "src-tauri/src/textedit/tagging/fixtures/libreoffice-list.pdf"
DIGEST = "558ff52a4bc29f8f4f07b320af70a699ff302a966f15b4a812f517e007ae5930"
# (a substring naming one run, what replaces it, whether the run is centred)
EDITS = [
    ("without exception", "without objection", False),
    ("Sample maker", "Sample rules", False),
    ("Rule two", "Rule six", False),
    ("Declaration", "Exception", True),
]
# (a substring naming one run, the sentence typed after it): each run is full
# to the page with it, so each wraps. Only letters the embedded subset has.
WRAPS = [
    ("model EX-1.", " It is described here for illustration only and it represents nothing at "
     "all, as the sample maker gives no undertaking."),
    ("requirements listed here:", " The same rulebook is described here for illustration only "
     "and represents nothing."),
    ("gives no undertaking;", " the sample maker gives no undertaking for the demonstration "
     "widget described."),
]
LABEL = re.compile(r"^(?:\d+\.|\u2022)(?:\s+|$)")
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


def flow(text):
    """A reading as one line of words, without the list labels.

    Readers differ on where a label goes: beside its item, on a line of its own,
    or with the other labels as a column, and the same reader may change its
    mind when an item gains a line. The words of the page and their order are
    what every reader has to agree on."""
    lines = (LABEL.sub("", line.strip()) for line in text.splitlines())
    return " ".join(" ".join(line.split()) for line in lines if line.strip())


def wrapped(text, source_text, run_text, added, reader):
    """Why a reading is not the source's with `added` after `run_text`, or None.

    PDFium and PDFKit place every glyph and are held to the words and the
    spaces between them. pypdf reads the stream in order and guesses a space
    from each change of the text matrix: a show a wrap moved is drawn from a
    `Tm` of its own, and pypdf then reads a space before it that no glyph
    shows (`repre -` for `repre-`). It is held to the characters and their
    order, which is what a reader in stream order can say."""
    before, after = flow(source_text), flow(text)
    old = " ".join(run_text.split())
    new = old + " " + " ".join(added.split())
    if reader == "pypdf":
        before, after, old, new = ("".join(part.split()) for part in (before, after, old, new))
    if before.count(old) != 1:
        return f"{reader}: the source does not show the edited run exactly once"
    if after.count(new) != 1:
        return f"{reader}: the item does not read as its text and the sentence after it"
    if after != before.replace(old, new):
        return f"{reader}: text outside the edited item changed or is out of order"
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
    wraps(options, source, found, before, source_text)
    print("[OK] a LibreOffice list in paragraph styles of its own is edited and read back")


def wraps(options, source, found, before, source_text):
    """Each of `WRAPS` in a round trip of its own, read back and compared."""
    swift = platform.system() == "Darwin" and shutil.which("swift")
    script = options.directory / "read.swift"
    kit_source = run("swift", script, source) if swift else None
    for index, (old, added) in enumerate(WRAPS):
        runs = [item for item in found["runs"] if old in item["text"]]
        if len(runs) != 1:
            fail(f"{old!r} names {len(runs)} runs, not one")
        text = runs[0]["text"]
        up = lambda value: math.ceil(value * 1000) / 1000
        size = runs[0]["size"]
        # `original` is not the probe's to read; `text_wrap_check` counts with it.
        request = {"page": 0, "operator": runs[0]["operator"], "replacement": text + added,
                   "original": text,
                   "layout": {"width": max(up(runs[0]["advance"]), 0.1),
                              "height": up(size * 1.25), "size": up(size), "wrap": False,
                              "font": "original", "grow": True}}
        plan = options.directory / f"wrap{index}.json"
        plan.write_text(json.dumps([request]), encoding="utf-8")
        result = options.directory / f"wrap{index}"
        run(options.probe, "--roundtrip", source, plan, result)
        edited = result / "edited.pdf"
        after = describe(edited)
        for part in ("structure", "fonts", "annotations", "pages"):
            if after[part] != before[part]:
                fail(f"wrap {index}: pypdf: the {part} changed")
        readings = [("pypdf", PdfReader(str(edited)).pages[0].extract_text(),
                     PdfReader(str(source)).pages[0].extract_text()),
                    ("PDFium", run(options.cli, "text", edited), source_text)]
        if swift:
            readings.append(("PDFKit", run("swift", script, edited), kit_source))
        for reader, reading, original in readings:
            # Where the comparison must fail: nothing typed, and two lines of
            # the page below the edit exchanged.
            if wrapped(original, original, text, added, reader) is None:
                fail(f"wrap {index}: {reader}: the unedited source passed as the result")
            lines = reading.splitlines()
            long = [number for number, line in enumerate(lines) if len(line.split()) > 3]
            lines[long[-1]], lines[long[-2]] = lines[long[-2]], lines[long[-1]]
            if wrapped("\n".join(lines), original, text, added, reader) is None:
                fail(f"wrap {index}: {reader}: two lines exchanged passed as the result")
            why = wrapped(reading, original, text, added, reader)
            if why:
                fail(f"wrap {index}: {why}")
        try:
            text_wrap_check.compare(source, edited, 0, plan)
        except AssertionError as error:
            fail(f"wrap {index}: pdfplumber: {error}")
        try:
            text_wrap_check.compare(source, source, 0, plan)
        except AssertionError:
            pass
        else:
            fail(f"wrap {index}: pdfplumber: the unedited source passed as the result")
        print(f"[OK] wrap {index}: {len(readings)} readers show the sentence after its item "
              "and every other line as it was, in order; structure, fonts and annotations kept")


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
