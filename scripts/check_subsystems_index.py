#!/usr/bin/env python3
"""Asserts that AGENTS.md's index of docs/SUBSYSTEMS.md names every section in it.

`docs/SUBSYSTEMS.md` is not loaded on every task; `AGENTS.md` is, and its *Stack*
section lists what the other file holds so that a reader can decide to open it.
That only works while the list is complete. On 2026-10-05 it named seven topics,
all of them inside the first two sections, and the file had grown six more
sections that the list did not mention: pictures, passwords, the smaller copy,
the text layer, drawings under a redaction and the signature image. Nothing was
red, because nothing compared the two.

The correspondence rule: **an index bullet begins with the section's title.** The
title is the `## ` heading of `docs/SUBSYSTEMS.md` with a trailing parenthetical
removed, so `## A smaller copy (`tpdf compress`, *Save a smaller copy*)` is
indexed as `A smaller copy`. A bullet is that title alone, optionally with a
closing full stop, or the title followed by `: ` and a gloss naming what the
section covers. The gloss is free text and is not checked; the ceiling on
`AGENTS.md` (`check_trap_index.py`) is what bounds it.

The two sides are compared as sets, in both directions, and then for order,
because the index says "in order". Refusals besides the diff, because a scan
that examined nothing reports no difference and so does a clean one: no marker,
no bullets, no headings, or a duplicate on either side.

What this reads: `## ` headings in `docs/SUBSYSTEMS.md`, and in `AGENTS.md` the
`- ` bullets that follow the line holding `MARKER`, up to the first line that is
neither a bullet nor blank. It reads no other prose, so no sentence about the
index can stand in for the index.

Usage:
    scripts/check_subsystems_index.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUBSYSTEMS = ROOT / "docs" / "SUBSYSTEMS.md"
AGENTS = ROOT / "AGENTS.md"

# The line in AGENTS.md that the index follows. A marker and not a sentence: the
# sentence above the list can then be reworded without the check losing the list.
MARKER = "<!-- subsystems-index -->"
SECTION = "## "
PARENTHETICAL = re.compile(r"\s+\([^()]*\)$")


def read(path: Path) -> "list[str]":
    """Returns a file's lines, decoded as UTF-8 (never the locale codec)."""
    return path.read_bytes().decode("utf-8").splitlines()


def sections() -> "list[str]":
    """Returns every `## ` title in docs/SUBSYSTEMS.md, without its parenthetical."""
    found: "list[str]" = []
    fenced = False
    for line in read(SUBSYSTEMS):
        if line.startswith("```"):
            fenced = not fenced
        elif not fenced and line.startswith(SECTION):
            found.append(PARENTHETICAL.sub("", line[len(SECTION) :].strip()))
    return found


def bullets() -> "tuple[list[str], int]":
    """Returns the index bullets of AGENTS.md and how often the marker occurs."""
    lines = read(AGENTS)
    marks = [i for i, line in enumerate(lines) if MARKER in line]
    found: "list[str]" = []
    if len(marks) == 1:
        for line in lines[marks[0] + 1 :]:
            if line.startswith("- "):
                found.append(line[2:].strip())
            elif line.strip():
                break
    return found, len(marks)


def title_of(bullet: str, known: "set[str]") -> str:
    """Returns the section a bullet names, or the bullet itself if it names none.

    The longest known title wins, so a title that is the start of another one
    cannot take the other's bullet.
    """
    for title in sorted(known, key=len, reverse=True):
        if bullet in (title, title + ".") or bullet.startswith(title + ": "):
            return title
    return bullet


def duplicates(values: "list[str]") -> "list[str]":
    """Returns each value that occurs more than once, in first-seen order."""
    seen: "set[str]" = set()
    twice: "list[str]" = []
    for value in values:
        if value in seen and value not in twice:
            twice.append(value)
        seen.add(value)
    return twice


def main() -> int:
    """Compares the sections against the index and reports any difference."""
    titles = sections()
    index, marks = bullets()
    known = set(titles)
    named = [title_of(bullet, known) for bullet in index]

    print(
        f"docs/SUBSYSTEMS.md: {len(titles)} sections; "
        f"AGENTS.md index: {len(index)} bullets",
        flush=True,
    )

    problems: "list[str]" = []
    if marks != 1:
        problems.append(f"`{MARKER}` occurs {marks} time(s) in {AGENTS.name}, expected exactly one")
    if not titles:
        problems.append(f"no `{SECTION.strip()}` sections found in {SUBSYSTEMS.name}")
    if not index:
        problems.append(f"no bullets found after `{MARKER}` in {AGENTS.name}")

    for title in duplicates(titles):
        problems.append(f"section title occurs twice in {SUBSYSTEMS.name}: {title}")
    for title in duplicates(named):
        problems.append(f"index names a section twice in {AGENTS.name}: {title}")

    listed = set(named)
    for title in titles:
        if title not in listed:
            problems.append(f"section in {SUBSYSTEMS.name}, missing from the {AGENTS.name} index: {title}")
    for title in named:
        if title not in known:
            problems.append(f"index bullet in {AGENTS.name}, no such section in {SUBSYSTEMS.name}: {title}")

    if not problems and named != titles:
        problems.append(
            f"the {AGENTS.name} index is not in the order of {SUBSYSTEMS.name}: "
            + "; ".join(titles)
        )

    if problems:
        print(
            f"[FAIL] {len(problems)} difference(s) between docs/SUBSYSTEMS.md and its index in "
            "AGENTS.md.\n"
            "       A new `## ` section needs a bullet in AGENTS.md's *Stack* that begins with its\n"
            "       title (the heading without its trailing parenthetical), in the same position.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"       {problem}", file=sys.stderr)
        return 1

    print(f"[OK] every one of the {len(titles)} sections of docs/SUBSYSTEMS.md is named in AGENTS.md.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
