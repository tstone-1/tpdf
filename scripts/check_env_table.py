#!/usr/bin/env python3
"""Asserts that docs/build/running.md's table of environment variables names
every `TPDF_*` variable the shipped code reads, and nothing nobody reads.

The application, the command-line tool and their tests read their switches from
`TPDF_*` variables through helpers that refuse loudly (`spike_env`, `env_or`,
`env_list`), so the mechanism is sound, and the inventory was nowhere: most of
the names were in no document, so a reader met a variable only by reading the
code that reads it. A table nobody holds to the code is the same defect one step
later, wrong the first time a variable is added or removed and silent about it.

WHAT IS COMPARED, both ways:

  1. Every name occurring in a tracked file under `SHIPPED` has a row. That
     population is the code that ships and the tests compiled beside it, which
     the gate cannot tell apart by reading text; a test-only name has a row
     marked `test`, which costs one line and says what it is.

  2. Every row names a file that is tracked, lies under `SHIPPED` or
     `ELSEWHERE`, and contains the name. So a row for a variable nobody reads
     any more fails, and so does a *Read in* column that has gone stale when a
     read moved, which is the column's only reason to be checked rather than
     trusted.

A name is matched by `NAME`: `TPDF_`, then capitals, digits and underscores,
ending on a capital or a digit, and not preceded by a word character, so the
Vite define `__TPDF_CHECKS__` is not a variable. Occurrence is not the same as
being read: a name in a comment counts. That errs toward a row too many, which
the second direction then holds to a file that still names it.

WHAT IS REFUSED rather than reported, because each reads like a clean run: `git`
missing or failing, no tracked file under a `SHIPPED` folder, no name found in
them, no table section, no rows, a row whose *For* is not one of `FOR`, and a
name with two rows.

Usage: scripts/check_env_table.py
"""

from __future__ import annotations

import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The document and the heading of the section holding the table.
TABLE = "docs/build/running.md"
HEADING = "### Environment variables the code reads"

#: Where a name must have a row. `src-tauri/shellext/src` reads none today; it
#: ships as the Explorer extension, so one it reads tomorrow needs a row.
SHIPPED = ("src-tauri/src", "src", "src-tauri/shellext/src")

#: Where a row may also point: probes, checks, scripts and the client's tests,
#: which read variables of their own and are documented when they are worth it.
ELSEWHERE = ("scripts", "src-tauri/examples", "src-tauri/tests", "src-tauri/shellext/examples", "api")

#: Who sets a variable, the table's last column.
FOR = ("run", "harness", "test")

NAME = re.compile(r"(?<![A-Za-z0-9_])TPDF_[A-Z0-9_]*[A-Z0-9]")

#: `| `TPDF_X` | `path` | what | for |`
ROW = re.compile(r"^\|\s*`(TPDF_[A-Z0-9_]+)`\s*\|\s*`([^`]+)`\s*\|(.*)\|\s*([a-z]+)\s*\|\s*$")


def tracked(folders: tuple[str, ...]) -> list[str]:
    """Tracked files under `folders`, repo-relative with forward slashes.

    From `git ls-files`, so build output and fetched dependencies never count.
    Fails closed: a missing or failing `git` would otherwise list nothing, and an
    empty population passes direction 1 trivially.
    """
    git = shutil.which("git")
    if git is None:
        raise SystemExit("[FAIL] git is not on PATH, so the tracked files cannot be listed")
    done = subprocess.run(
        [git, "-C", str(ROOT), "ls-files", "-z", "--", *folders],
        capture_output=True,
        check=False,
    )
    if done.returncode != 0:
        raise SystemExit(f"[FAIL] git ls-files exited {done.returncode}: {done.stderr.decode(errors='replace')}")
    return [p for p in done.stdout.decode("utf-8").split("\0") if p]


def names_in(path: str) -> set[str]:
    """Every `TPDF_*` name in one file; a file that is not text names none."""
    try:
        text = (ROOT / path).read_bytes().decode("utf-8", errors="replace")
    except OSError:
        return set()
    return set(NAME.findall(text))


def table_rows(text: str) -> tuple[list[tuple[str, str, str]], list[str]]:
    """The section's rows as (name, path, for), and lines that look like rows and are not."""
    lines = text.split("\n")
    try:
        start = lines.index(HEADING) + 1
    except ValueError:
        return [], []
    rows, malformed = [], []
    for line in lines[start:]:
        if line.startswith("### ") or line.startswith("## "):
            break
        if not line.startswith("| `TPDF_"):
            continue
        m = ROW.match(line)
        if m is None:
            malformed.append(line[:100])
            continue
        rows.append((m.group(1), m.group(2), m.group(4)))
    return rows, malformed


def main() -> int:
    """Compares the table with the names the tree contains."""
    problems: list[str] = []

    shipped_files = tracked(SHIPPED)
    if not shipped_files:
        print(f"[FAIL] no tracked file under {', '.join(SHIPPED)}: nothing was scanned")
        return 1
    required: dict[str, str] = {}
    for path in shipped_files:
        for name in names_in(path):
            required.setdefault(name, path)
    if not required:
        print(f"[FAIL] no TPDF_* name in {len(shipped_files)} shipped files: the pattern matches nothing")
        return 1

    document = ROOT / TABLE
    if not document.exists():
        print(f"[FAIL] {TABLE} does not exist")
        return 1
    rows, malformed = table_rows(document.read_text(encoding="utf-8"))
    if not rows:
        print(f"[FAIL] {TABLE}: no rows under {HEADING!r}")
        return 1
    for line in malformed:
        problems.append(f"{TABLE}: a row that does not read as name | file | what | for: {line}")

    allowed = set(shipped_files) | set(tracked(ELSEWHERE))
    seen: set[str] = set()
    for name, path, who in rows:
        if name in seen:
            problems.append(f"{TABLE}: {name} has two rows")
        seen.add(name)
        if who not in FOR:
            problems.append(f"{TABLE}: {name} is for {who!r}, not one of {', '.join(FOR)}")
        if path not in allowed:
            problems.append(f"{TABLE}: {name} is read in {path}, which is not a tracked file under the scanned folders")
        elif name not in names_in(path):
            problems.append(f"{TABLE}: {name} is read in {path}, and that file does not name it")

    for name in sorted(set(required) - seen):
        problems.append(f"{name} is in {required[name]} and has no row in {TABLE}")

    for line in problems:
        print(f"[FAIL] {line}")
    if problems:
        return 1
    outside = len(seen - set(required))
    print(
        f"[OK]   {len(required)} TPDF_* names in {len(shipped_files)} shipped files all have a row in "
        f"{TABLE}; {outside} more rows name variables read elsewhere, each in the file it names"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
