#!/usr/bin/env python3
"""Asserts that AGENTS.md's trap index names every entry in docs/TRAPS.md.

The index exists because `docs/TRAPS.md` is too large to load on every task and
the thing worth loading is the knowledge that a trap *exists* -- the paragraph
matters once you are already in that area. That only works if the index is
complete. An index missing an entry is worse than no index, because it answers
"is there a trap about this?" with a confident no.

`AGENTS.md` already names `grep -c '^### ' docs/TRAPS.md` as the authority for
the *count*, and the count is the half that stopped drifting. The titles did
not: on 2026-08-02 the file said 218, `docs/TRAPS.md` held 218, and the index
listed 215 -- so the number everyone checks was right while the list nobody
counts was three short, all three added by the commit that had updated the
number. This is the repo's own doctrine arriving one level up: document the
invariant, not the tally. The invariant is the **set of titles**, and a set diff
needs no knowledge of how many there are.

The correspondence rule, which is a rule about the file as it is rather than one
imposed on it: **an index bullet is the entry's title verbatim**, optionally
followed by a parenthetical the index adds for a title that misleads on its own
-- one bullet does that today, to warn that the title names the wrong mechanism.
So a bullet matches a title if it equals it, or if it equals it followed by
` (...)`. Anything else is a mismatch and is reported as one.

Three refusals besides the diff, because a scan that examined nothing reports no
findings and so does a clean one: no titles found, no bullets found, or a
duplicate on either side (a duplicate makes a set comparison lie -- two bullets
can cover one title while another goes missing and the sets still match).

Note what this check reads: `### ` headings in `docs/TRAPS.md`, and `- ` bullets
under the `## Known traps` section of `AGENTS.md`. It reads no other prose, and
in particular nothing in this file, so no description of the check can satisfy
the check. `AGENTS.md` records a checker that was silenced by the note tracking
the very gap it was looking for; the cheap defence here is that the scanned
region is two named structures rather than "text that looks like a trap list".

Usage:
    scripts/check_trap_index.py
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRAPS = ROOT / "docs" / "TRAPS.md"
AGENTS = ROOT / "AGENTS.md"

# The index section of AGENTS.md, and the heading level its groups use. Bullets
# outside a group are not index entries -- the section's own prose may use them.
INDEX_SECTION = "## Known traps"
GROUP = "### "
ENTRY = "### "


def read(path: Path) -> "list[str]":
    """Returns a file's lines, decoded as UTF-8.

    Bytes, then UTF-8: `read_text()` decodes with the locale codec, which is
    cp1252 on Windows, and both these files hold characters it cannot read.
    That is the trap that stopped `mutate_rust.py` running on Windows at all.
    """
    return path.read_bytes().decode("utf-8").splitlines()


def titles() -> "list[str]":
    """Returns every entry title in docs/TRAPS.md, in file order."""
    return [line[len(ENTRY) :].strip() for line in read(TRAPS) if line.startswith(ENTRY)]


def bullets() -> "list[tuple[str, str]]":
    """Returns (group, bullet) for every index entry in AGENTS.md, in order."""
    lines = read(AGENTS)
    found: "list[tuple[str, str]]" = []
    inside = False
    group = ""

    for line in lines:
        if line.startswith("## "):
            inside = line.strip() == INDEX_SECTION
            group = ""
            continue
        if not inside:
            continue
        if line.startswith(GROUP):
            group = line[len(GROUP) :].strip()
        elif line.startswith("- ") and group:
            found.append((group, line[2:].strip()))

    return found


def title_of(bullet: str, known: "set[str]") -> str:
    """Returns the title a bullet names, or the bullet itself if it names none.

    A bullet is the title verbatim, or the title plus a parenthetical the index
    adds. Exact match is tried first, so a title that itself ends in `(...)`
    cannot be truncated by the second rule; the split points are then tried in
    order, so the shortest prefix that is a real title wins.
    """
    if bullet in known:
        return bullet
    if bullet.endswith(")"):
        at = bullet.find(" (")
        while at != -1:
            if bullet[:at] in known:
                return bullet[:at]
            at = bullet.find(" (", at + 1)
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
    """Compares the trap corpus against the index and reports any difference."""
    entries = titles()
    index = bullets()
    known = set(entries)
    named = [title_of(bullet, known) for _, bullet in index]

    groups = len({group for group, _ in index})
    print(
        f"docs/TRAPS.md: {len(entries)} entries; "
        f"AGENTS.md index: {len(index)} bullets in {groups} groups",
        flush=True,
    )

    problems: "list[str]" = []

    # An empty population is not a clean one. Either file could be moved,
    # renamed or restructured, and every one of those reads as "no difference".
    if not entries:
        problems.append(f"no `{ENTRY.strip()}` entries found in {TRAPS.name}")
    if not index:
        problems.append(f"no index bullets found under `{INDEX_SECTION}` in {AGENTS.name}")

    for title in duplicates(entries):
        problems.append(f"duplicate entry in {TRAPS.name}: {title}")
    for bullet in duplicates(named):
        problems.append(f"duplicate index bullet in {AGENTS.name}: {bullet}")

    listed = set(named)
    for title in entries:
        if title not in listed:
            problems.append(f"in {TRAPS.name}, missing from the index: {title}")
    for title in named:
        if title not in known:
            problems.append(f"in the index, no such entry in {TRAPS.name}: {title}")

    if problems:
        print(
            f"[FAIL] {len(problems)} difference(s) between docs/TRAPS.md and the "
            "AGENTS.md trap index.\n"
            "       A new trap goes in both, in the same commit: the entry under a "
            "`### ` heading\n"
            "       in docs/TRAPS.md, and its title verbatim as a bullet under the "
            "matching group\n"
            "       in AGENTS.md. An index that omits an entry answers \"is there a "
            "trap about\n       this?\" with a confident no.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"       {problem}", file=sys.stderr)
        return 1

    print(f"[OK] every one of the {len(entries)} traps is named in the AGENTS.md index.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
