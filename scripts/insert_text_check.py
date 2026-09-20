#!/usr/bin/env python3
"""Reads back a save that edited text on a page inserted from another file.

`save::import_tests` proves the writer with `lopdf` on both sides of the
comparison, which cannot notice a file it agrees with itself about. This runs
the same path through `insert-text-probe` and then asks two readers that share
no code with tpdf: `qpdf --check` for the structure, and `pypdf` for the words.

**The assertion that bites is not "the replacement is there".** Page `n` of the
opened document and page `n` of the file its pages came from are two different
pages, and every stage between the keystroke and the bytes has to keep them
apart --- so the fixtures are chosen to make the collision real: the inserted
page is page 1 of the other file, and the opened document has a page 1 of its
own, with different words on it. A writer that edited the wrong document would
put the replacement on that page instead, and the file would still be valid,
still have the right number of pages, and still contain the replacement
somewhere.

Fixtures are the tracked synthetic corpus, and nothing here writes into the
tree: the saved file goes to a temporary directory and is removed.

Usage:
    uv run --with pypdf scripts/insert_text_check.py
    uv run --with pypdf scripts/insert_text_check.py --probe target/release/examples/insert-text-probe

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

#: The document the reader has open. Eight pages, each naming itself.
BASE = ROOT / "testdata" / "links.pdf"
#: The file pages are inserted from. Its page 1 is unmistakably not the base's.
OTHER = ROOT / "testdata" / "text-base14.pdf"
#: Narrower than the run it replaces, so no layout is needed, and a word that
#: appears in neither fixture --- a replacement that were a substring of the
#: original would be found by a check that a truncation would also pass.
REPLACEMENT = "gasket acme"

failures: list[str] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"[{'PASS' if ok else 'FAIL'}] {name}{f' --- {detail}' if detail else ''}")
    if not ok:
        failures.append(name)


def page_text(reader, index: int) -> str:
    return reader.pages[index].extract_text() or ""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--probe",
        default=str(ROOT / "src-tauri" / "target" / "debug" / "examples" / "insert-text-probe"),
        help="the insert-text-probe binary (cargo build --example insert-text-probe)",
    )
    args = parser.parse_args()

    probe = Path(args.probe)
    if not probe.exists():
        print(f"[FAIL] no probe at {probe}; build it with "
              "`cargo build --example insert-text-probe`")
        return 2
    for fixture in (BASE, OTHER):
        if not fixture.exists():
            print(f"[FAIL] missing fixture {fixture}")
            return 2

    try:
        from pypdf import PdfReader
    except ImportError:
        print("[FAIL] pypdf is not available; run under "
              "`uv run --with pypdf`")
        return 2

    work = Path(tempfile.mkdtemp(prefix="tpdf-insert-text-"))
    try:
        out = work / "saved.pdf"
        run = subprocess.run(
            [
                str(probe), str(BASE), str(OTHER), str(out),
                "--page", "0", "--after", "0", "--replacement", REPLACEMENT,
            ],
            capture_output=True,
            text=True,
        )
        sys.stderr.write(run.stderr)
        if run.returncode != 0:
            print(f"[FAIL] the probe refused the save (exit {run.returncode})")
            return 1
        said = json.loads(run.stdout)

        # Structure, by a reader that is not ours and not pypdf either.
        qpdf = shutil.which("qpdf")
        if qpdf:
            checked = subprocess.run(
                [qpdf, "--check", str(out)], capture_output=True, text=True
            )
            check(
                "qpdf reports no syntax or stream encoding errors",
                checked.returncode == 0,
                checked.stdout.strip().splitlines()[-1] if checked.stdout else "",
            )
        else:
            print("[WARN] qpdf is not installed; the structural check was skipped")

        saved = PdfReader(str(out))
        base = PdfReader(str(BASE))
        other = PdfReader(str(OTHER))
        check(
            "the saved document is the opened one plus the inserted page",
            len(saved.pages) == len(base.pages) + 1,
            f"{len(saved.pages)} against {len(base.pages)} + 1",
        )

        at = said["inserted_at"]
        inserted = page_text(saved, at)
        check(
            "the replacement is on the inserted page",
            REPLACEMENT in inserted,
            f"page {at + 1}",
        )
        check(
            "the words it replaced are gone from that page",
            said["original"] not in inserted,
        )
        # The rest of the inserted page came across unedited, which is what
        # separates a replacement from a page that lost its content stream.
        source_page = page_text(other, said["other_page"])
        rest = [
            line.strip()
            for line in source_page.splitlines()
            if line.strip() and said["original"] not in line
        ]
        check(
            "the inserted page keeps the rest of its own text",
            all(line in inserted for line in rest),
            f"{len(rest)} other line(s)",
        )

        # The collision, and the reason these two fixtures: the opened
        # document has a page of the same number as the page that was
        # inserted, and it must be untouched.
        collides = page_text(saved, 0)
        check(
            "the opened document's page of the same number is unchanged",
            collides == page_text(base, said["other_page"])
            and REPLACEMENT not in collides,
            f"page 1, against page {said['other_page'] + 1} of the opened file",
        )
        # And every other page of the opened document, in order.
        kept = [
            page_text(saved, slot if slot < at else slot + 1) == page_text(base, slot)
            for slot in range(len(base.pages))
        ]
        check(
            "every page of the opened document reads back as it was",
            all(kept),
            f"{sum(kept)} of {len(kept)}",
        )
        check(
            "the replacement is nowhere else in the file",
            sum(
                1
                for index in range(len(saved.pages))
                if REPLACEMENT in page_text(saved, index)
            )
            == 1,
        )
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        print(f"\n[FAIL] {len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("\n[OK] the replacement is on the page it was typed on, and only there")
    return 0


if __name__ == "__main__":
    sys.exit(main())
