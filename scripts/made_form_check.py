#!/usr/bin/env python3
"""A form made by tpdf, against a reader that shares no code with it.

Every other check of a made form is tpdf reading what tpdf wrote. The people
who receive a form open it in Preview or Acrobat, so this asks PDFKit, the
engine Preview reads and saves forms with:

1. `save::tests::a_form_of_every_kind_is_made_and_answered...` makes a form
   with one field of each kind, out of reading order, through the save, and
   answers it. With TPDF_MADE_FORM set it leaves both files.
2. `made_form_pdfkit.swift` reads both with PDFKit: kinds, the radio group,
   limit, alignment, tooltip, the order of the page's list, and the answers.
   It then answers the empty form itself and saves it.
3. `tpdf-cli fields` reads what PDFKit saved, and every answer must be the
   one PDFKit gave.

macOS only: PDFKit is Apple's. Run from the repository root:

    uv run scripts/made_form_check.py [directory to keep the files in]

On 2026-10-04 step 3 found a checkbox PDFKit had ticked reading as empty and
a radio group it had answered as unreadable: PDFKit writes a button's answer
as text where the format has a name, and replaces the chosen radio button's
two states with one drawing. `forms::scan` reads both now.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "src-tauri" / "Cargo.toml"
TEST = "save::tests::a_form_of_every_kind_is_made_and_answered_and_left_for_another_reader"

# What `made_form_pdfkit.swift` answers, as `tpdf fields` reports a value.
ANSWERED = {"Name": "Grace", "Notes": "first\nsecond", "Agree": True, "Colour": "Blue", "Pay": "Bank transfer"}


def run(command: list[str], **more: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, capture_output=True, text=True, check=False, **more)  # type: ignore[call-overload]


def main() -> int:
    if sys.platform != "darwin":
        print("[FAIL] PDFKit is macOS's; this check runs there")
        return 2
    kept = len(sys.argv) > 1
    directory = Path(sys.argv[1]).resolve() if kept else Path(tempfile.mkdtemp(prefix="tpdf-made-form-"))
    directory.mkdir(parents=True, exist_ok=True)

    made = run(["cargo", "test", "--locked", "--manifest-path", str(MANIFEST), "--lib", TEST, "--", "--exact"],
               env={**os.environ, "TPDF_MADE_FORM": str(directory)})
    if made.returncode != 0 or "1 passed" not in made.stdout:
        print(f"[FAIL] the form was not made: {(made.stdout + made.stderr)[-600:]}")
        return 1
    if not (directory / "made.pdf").is_file() or not (directory / "made-filled.pdf").is_file():
        print("[FAIL] the test passed and left no file: is TPDF_MADE_FORM read?")
        return 1
    print(f"[OK]   tpdf made the form and answered it: {directory}")

    reader = run(["swift", str(ROOT / "scripts" / "made_form_pdfkit.swift"), str(directory)])
    print(reader.stdout.rstrip())
    if reader.returncode != 0:
        print(reader.stderr[-600:])
        return 1

    built = run(["cargo", "build", "--locked", "--manifest-path", str(MANIFEST), "--bin", "tpdf-cli"])
    if built.returncode != 0:
        print(f"[FAIL] the command-line tool did not build: {built.stderr[-600:]}")
        return 1
    tool = ROOT / "src-tauri" / "target" / "debug" / "tpdf-cli"
    listed = run([str(tool), "fields", str(directory / "pdfkit-filled.pdf"), "--json"])
    try:
        fields = {field["name"]: field for field in json.loads(listed.stdout)["fields"]}
    except (ValueError, KeyError):
        print(f"[FAIL] tpdf could not read what PDFKit saved: {listed.stderr.strip()[:300]}")
        return 1
    ok = True
    for name, answer in ANSWERED.items():
        got = fields.get(name, {}).get("value", "<no such field>")
        good = got == answer
        ok &= good
        print(f"{'[OK]  ' if good else '[FAIL]'} tpdf reads {name!r} as PDFKit answered it: {got!r}")
    only = set(fields) == set(ANSWERED)
    ok &= only
    print(f"{'[OK]  ' if only else '[FAIL]'} and no other field: {sorted(fields)}")
    print("[OK] the form holds in both directions" if ok else "[FAIL] tpdf and PDFKit disagree about the form")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
