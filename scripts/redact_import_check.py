#!/usr/bin/env python3
"""Reads back a redaction applied to a document that also holds inserted pages.

`save::import_tests` and `redact::tests` prove the pieces with `lopdf` on both
sides of the comparison, which cannot notice a file it agrees with itself about.
This runs the whole removal through `redact-import-probe` --- which writes and
verifies across the sandboxed worker boundary --- and then asks two readers that
share no code with tpdf: `qpdf --check` for the structure, and `pypdf` for which
page each word is on.

**The assertion that bites is not "the word is gone".** A region is marked on the
opened document's own page while a page of another file sits in front of it, so
the page's number in the base file and its slot in the output are different
numbers. A writer that addressed the removal by output slot would strip the
inserted page instead, and the file would still be valid, still have the right
number of pages and still be missing the word.

**And the second run is what the increment is really about.** With `--echo` the
inserted page prints the very word the region covered. `verify::scan` reads the
whole file, so it reports that word as still present --- which is honest and is
not a failed removal. pypdf can say which page it is on and the scan cannot, so
this script asserts the thing the scan structurally could not: the hit is on the
inserted page, the marked page is clean, and the report carries the sentence
saying it could not tell them apart rather than a claim that it could.

Fixtures are the tracked synthetic corpus; the echo file and both outputs go to a
temporary directory and are removed.

Usage:
    uv run --with pypdf scripts/redact_import_check.py
    uv run --with pypdf scripts/redact_import_check.py --probe src-tauri/target/release/examples/redact-import-probe

Exits 0 when every check passes, 1 on a failure, 2 on a usage or setup problem.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: The document the reader has open, and the one the region is marked on.
BASE = ROOT / "testdata" / "text-base14.pdf"
#: The file pages are inserted from in the plain run. It carries neither word.
OTHER = ROOT / "testdata" / "links.pdf"
#: What the marked region covers on page 1 of the base document.
NEEDLE = "4711-0815"
#: On another line of the same page. Route B takes whole operations, so this is
#: the survivor that says a scan finding nothing has actually looked.
KEEP = "Sphinx of black quartz"

failures: list[str] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"[{'PASS' if ok else 'FAIL'}] {name}{f' --- {detail}' if detail else ''}")
    if not ok:
        failures.append(name)


def page_text(reader, index: int) -> str:
    return reader.pages[index].extract_text() or ""


def run_probe(probe: Path, out: Path, other: Path, echo: bool) -> dict | None:
    argv = [str(probe), str(BASE), str(other), str(out), "--needle", NEEDLE, "--keep", KEEP]
    if echo:
        argv.append("--echo")
    run = subprocess.run(argv, capture_output=True, text=True)
    sys.stderr.write(run.stderr)
    if run.returncode != 0:
        print(f"[FAIL] the probe refused the removal (exit {run.returncode})")
        return None
    return json.loads(run.stdout)


def qpdf_checks(out: Path, label: str) -> None:
    qpdf = shutil.which("qpdf")
    if not qpdf:
        print("[WARN] qpdf is not installed; the structural check was skipped")
        return
    checked = subprocess.run([qpdf, "--check", str(out)], capture_output=True, text=True)
    check(
        f"{label}: qpdf reports no syntax or stream encoding errors",
        checked.returncode == 0,
        checked.stdout.strip().splitlines()[-1] if checked.stdout else "",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--probe",
        default=str(
            ROOT / "src-tauri" / "target" / "debug" / "examples" / "redact-import-probe"
        ),
        help="the redact-import-probe binary (cargo build --example redact-import-probe)",
    )
    args = parser.parse_args()

    probe = Path(args.probe)
    if not probe.exists():
        print(
            f"[FAIL] no probe at {probe}; build it with "
            "`cargo build --example redact-import-probe`"
        )
        return 2
    for fixture in (BASE, OTHER):
        if not fixture.exists():
            print(f"[FAIL] missing fixture {fixture}; run scripts/make_fixtures.sh")
            return 2

    try:
        from pypdf import PdfReader
    except ImportError:
        print("[FAIL] pypdf is not available; run under `uv run --with pypdf`")
        return 2

    work = Path(tempfile.mkdtemp(prefix="tpdf-redact-import-"))
    try:
        # --- Run one: the other file carries neither word -------------------
        out = work / "plain.pdf"
        said = run_probe(probe, out, OTHER, echo=False)
        if said is None:
            return 1
        qpdf_checks(out, "plain")

        saved = PdfReader(str(out))
        base = PdfReader(str(BASE))
        other = PdfReader(str(OTHER))
        at = said["inserted_at"]
        marked = said["marked_slot"]
        check(
            "plain: the saved document is the opened one plus the inserted page",
            len(saved.pages) == len(base.pages) + 1,
            f"{len(saved.pages)} against {len(base.pages)} + 1",
        )
        removed_from = page_text(saved, marked)
        check(
            "plain: the marked words are gone from the page they were marked on",
            NEEDLE not in removed_from,
            f"page {marked + 1}",
        )
        check(
            "plain: and the rest of that page survived, so the removal was not the whole page",
            KEEP in removed_from,
        )
        # The collision this pair of fixtures exists for: the inserted page sits
        # at the slot the marked page's own number would have addressed.
        inserted = page_text(saved, at)
        check(
            "plain: the inserted page reads back as the page it came from",
            inserted.strip() == page_text(other, 0).strip(),
            f"slot {at + 1}, against page 1 of the other file",
        )
        check(
            "plain: and nothing was taken out of it",
            NEEDLE not in inserted and inserted.strip() != "",
        )
        check(
            "plain: the marked words are on no page of the file at all",
            not any(NEEDLE in page_text(saved, n) for n in range(len(saved.pages))),
        )
        # What tpdf itself said about the same file, across the worker boundary.
        check(
            "plain: tpdf's own scan agrees the words are gone",
            said["needle_found"] is False,
        )
        check(
            "plain: and it could see the file, because it still finds the survivor",
            said["keep_found"] is True,
        )
        check(
            "plain: so no note about inserted pages was added, and the file can verify",
            said["note"] is None,
        )

        # --- Run two: the inserted page prints the very word ----------------
        echoed = work / "echo-out.pdf"
        said = run_probe(probe, echoed, OTHER, echo=True)
        if said is None:
            return 1
        qpdf_checks(echoed, "echo")

        saved = PdfReader(str(echoed))
        at = said["inserted_at"]
        marked = said["marked_slot"]
        check(
            "echo: the marked page is still clean",
            NEEDLE not in page_text(saved, marked),
            f"page {marked + 1}",
        )
        check(
            "echo: and the word is on the inserted page, which nobody marked",
            NEEDLE in page_text(saved, at),
            f"page {at + 1}",
        )
        # The whole point. tpdf's scan reports the word as present and cannot
        # say which page; pypdf has just said which page, which is the
        # attribution the report does not claim to have.
        check(
            "echo: tpdf's scan reports the word as still in the file",
            said["needle_found"] is True,
        )
        check(
            "echo: and the report says it cannot tell which page that is",
            isinstance(said["note"], str)
            and "does not say which page a word is on" in said["note"],
            (said["note"] or "")[:60],
        )
        check(
            "echo: the note claims nothing about the removal having worked",
            isinstance(said["note"], str) and "was removed" not in said["note"],
        )
        check(
            "echo: and it counts the inserted pages it is talking about",
            isinstance(said["note"], str)
            and f"{said['inserted']} page(s) inserted" in said["note"],
        )
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        print(f"\n[FAIL] {len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print(
        "\n[OK] the removal took the marked page's words and left the inserted page whole, "
        "and the report said what it could and could not prove"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
