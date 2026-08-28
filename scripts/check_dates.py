#!/usr/bin/env python3
"""Fails when a tracked file carries a date that has not happened yet.

**Provenance is this repository's main instrument, and it is written as dates.**
`AGENTS.md`, `BUILD.md`, `docs/TRAPS.md`, `docs/PLAN.md` and a great many doc
comments carry sentences of the form *"measured 2026-08-19 over 40 documents"*.
A reader weighs such a claim by how old it is: a number measured last week is
worth acting on, the same number from three months ago is worth re-measuring.

A date in the future breaks that in a way nothing else notices. It is not
merely wrong --- it makes every stamp in the same batch unreliable, because a
reader who spots one has no way to tell which of the others were written in the
same sitting. On 2026-08-28 there were **seventy** such stamps across eleven
tracked files, all reading 2026-08-29 or 2026-08-30, every one of them written
by a commit dated 2026-08-28. Nothing was wrong with the measurements; the
provenance on all of them was.

**Today rather than HEAD's commit date**, deliberately. Comparing against the
last commit would refuse a working tree in which you have correctly stamped
today's work while HEAD is from yesterday --- which is the normal shape of an
edit and would train a reader to skip the gate. A measurement taken in the
future is impossible on any clock; that is the invariant, and it needs no
repository state to check.

Exit codes: 0 clean, 1 a date lies ahead, 2 the check could not run.
"""

import datetime
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ISO only. The repository writes every provenance stamp this way, and a looser
# pattern would start reading version numbers and byte offsets as dates.
DATE = re.compile(rb"\b(20\d\d)-(\d\d)-(\d\d)\b")

# Dates that are legitimately in the future because they are dates the code is
# *about* rather than dates a sentence was written on: a certificate's validity
# runs forward by definition.
#
# **Keyed by file and by the exact date**, not by file alone. Exempting a whole
# file would take `docs/THREAT-MODEL.md`'s two dozen provenance stamps out of the
# gate to excuse one expiry, which is how a check quietly stops covering what it
# was written for. Each entry carries a reason, and an entry whose date is no
# longer in that file is a failure rather than a silent excuse -- a certificate
# regenerated with a new expiry has to be looked at, not waved through.
EXEMPT: "dict[tuple[str, str], str]" = {
    (
        "docs/THREAT-MODEL.md",
        "2031-07-26",
    ): "when the Developer ID signing certificate expires",
    (
        "src-tauri/src/docinfo.rs",
        "2030-01-01",
    ): "the generated signing fixture's certificate validity, asserted by the reader",
    (
        "src/lib/properties.test.ts",
        "2030-01-01",
    ): "the same fixture certificate, in the frontend's own expectation",
}


def tracked() -> "list[str]":
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files"],
        capture_output=True,
        text=True,
        check=True,
    )
    return out.stdout.split()


def main() -> int:
    today = datetime.date.today()
    try:
        files = tracked()
    except (subprocess.CalledProcessError, FileNotFoundError) as why:
        print(f"[FAIL] could not list tracked files: {why}")
        return 2

    # Every exemption must still name something real. Checked before the scan, so
    # a stale entry is reported even on a tree that is otherwise clean.
    for (name, when), why in EXEMPT.items():
        path = ROOT / name
        if not path.exists():
            print(f"[FAIL] the exemption for {name} ({why}) names a file that is not there")
            return 2
        if when.encode() not in path.read_bytes():
            print(
                f"[FAIL] {name} no longer carries {when} ({why}), so the exemption is stale"
            )
            return 2

    ahead: "list[str]" = []
    scanned = 0
    for name in files:
        path = ROOT / name
        try:
            raw = path.read_bytes()
        except OSError:
            continue
        # A NUL in the first block is git's own definition of binary, and a PDF
        # fixture is not a document anyone reads dates out of.
        if b"\0" in raw[:8000]:
            continue
        scanned += 1
        for line_no, line in enumerate(raw.splitlines(), start=1):
            for match in DATE.finditer(line):
                year, month, day = (int(part) for part in match.groups())
                try:
                    when = datetime.date(year, month, day)
                except ValueError:
                    continue  # 2026-13-01 is not a date, it is a coincidence
                if when > today and (name, when.isoformat()) not in EXEMPT:
                    ahead.append(f"{name}:{line_no}: {when.isoformat()}")

    if ahead:
        print(f"[FAIL] {len(ahead)} date(s) in tracked files have not happened yet:")
        for one in ahead[:40]:
            print(f"  {one}")
        if len(ahead) > 40:
            print(f"  ... and {len(ahead) - 40} more")
        print(f"  (today is {today.isoformat()})")
        return 1

    print(f"[OK] no date ahead of {today.isoformat()} in {scanned} tracked text files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
