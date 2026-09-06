#!/usr/bin/env python3
"""Asserts that docs/TRAPS.md's table of contents names every entry in it.

The index exists because `docs/TRAPS.md` is too large to load on every task and
the thing worth loading is the knowledge that a trap *exists* -- the paragraph
matters once you are already in that area. That only works if the index is
complete. An index missing an entry is worse than no index, because it answers
"is there a trap about this?" with a confident no.

`AGENTS.md` names `grep -c '^### ' docs/TRAPS.md` as the authority for the
*count*, and the count is the half that stopped drifting. The titles did not: on
2026-08-02 the file said 218, `docs/TRAPS.md` held 218, and the index listed
215 -- so the number everyone checks was right while the list nobody counts was
three short, all three added by the commit that had updated the number. This is
the repo's own doctrine arriving one level up: document the invariant, not the
tally. The invariant is the **set of titles**, and a set diff needs no knowledge
of how many there are.

**The index moved into `docs/TRAPS.md` on 2026-09-06, and this check moved with
it.** It had been in `AGENTS.md` since 2026-07-28, where by then it was 639
bullets, about 51 KB of a file loaded whole before every task; `AGENTS.md` stood
at 112,084 characters against the 130,000 ceiling below, and the corpus had been
growing about 130 entries a week for three weeks, which put the ceiling two to
three weeks out. So the diff now runs inside one file -- the `## ` groups and
their bullets at the top of `docs/TRAPS.md` against the `### ` entries beneath
them -- and adding a trap costs `AGENTS.md` nothing. What `AGENTS.md` keeps is
the thirteen group names, one line each, saying when an area is worth opening;
those are checked against the `## ` groups over there in both directions, so a
group cannot be added or renamed on one side alone.

The correspondence rule, which is a rule about the file as it is rather than one
imposed on it: **a table-of-contents bullet is the entry's title verbatim**,
optionally followed by a parenthetical the index adds for a title that misleads
on its own -- one bullet does that today, to warn that the title names the wrong
mechanism. So a bullet matches a title if it equals it, or if it equals it
followed by ` (...)`. Anything else is a mismatch and is reported as one.

**That tolerance is also how the index reached 111 KB.** The rule above was
written down and enforced by nobody: a tail is invisible to the set diff, so by
2026-08-31 **323** of 588 bullets carried one, 62,440 characters, and
`AGENTS.md` went over the 150,000-character limit at which the harness stops
loading it whole. Every tail read was a compression of the entry it points at --
audited one by one, exactly one carried a fact its entry did not, and that fact
was merged into the entry rather than kept in the index. So the tolerance is now
an allowlist: a bullet must be its title, unless `ALLOWED_PARENTHETICAL` names
the title and says why. An entry there for a title that no longer exists is a
failure too, because an allowlist nobody prunes excuses things nobody chose.

**And a second rule, because the first only bounds what a bullet may hold, not
how many there are.** Moving the index bought room rather than a guarantee: the
other sections of `AGENTS.md` go on growing, and it was the trap list that
crossed the harness's limit only because it was the fastest grower in the file.
`SIZE_CEILING` fails while there is still room to act. It is a deadline, not a
target: when it fires, the fix is to move a section out to a file the file
points at, the way `docs/TRAPS.md` and `docs/RATIONALE.md` were split off in the
first place, and the way the trap index itself was on 2026-09-06.

Refusals besides the diff, because a scan that examined nothing reports no
findings and so does a clean one: no titles found, no bullets found, no groups
found on either side, or a duplicate in any of those four populations (a
duplicate makes a set comparison lie -- two bullets can cover one title while
another goes missing and the sets still match).

Note what this check reads: `### ` headings in `docs/TRAPS.md`, the `## `
headings and `- ` bullets above the first of them in the same file, and `- `
bullets naming a group in bold under the `## Known traps` section of
`AGENTS.md`. It reads no other prose, and in particular nothing in this file, so
no description of the check can satisfy the check. `docs/TRAPS.md` records a
checker that was silenced by the note tracking the very gap it was looking for;
the cheap defence here is that the scanned regions are named structures rather
than "text that looks like a trap list".

Usage:
    scripts/check_trap_index.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRAPS = ROOT / "docs" / "TRAPS.md"
AGENTS = ROOT / "AGENTS.md"

# The table of contents in docs/TRAPS.md: `## ` groups and their bullets, all of
# it above the first entry. The first `### ` ends the region, which is why the
# scan needs no end marker of its own -- there is no arrangement of this file in
# which an entry sits inside its own index.
GROUP = "## "
ENTRY = "### "

# The section of AGENTS.md that names the groups, and the shape of one line in
# it: `- **<group>** --- <when to read it>`. A bullet in that section that does
# not name a group in bold is a failure rather than something to skip, because a
# skipped line is how a group quietly leaves one side of the comparison.
AGENTS_SECTION = "## Known traps"
AGENTS_GROUP = re.compile(r"^- \*\*(?P<name>.+?)\*\*")

# Titles whose bullet may carry a parenthetical, and the reason each one does.
# A title is a claim rather than the lesson, and the section's own prose already
# warns that several are the opposite of what they sound like -- so a gloss that
# merely restates the entry does not belong here. This is for a title that is
# actively wrong about its own subject, where a reader who trusts it is misled.
ALLOWED_PARENTHETICAL = {
    "Restoring a mutated file by *moving* a backup over it tests the mutated binary": (
        "the title names the wrong mechanism, and the entry below it is the correction"
    ),
}

# The whole of AGENTS.md, in characters. The harness stops loading the file at
# 150,000, so this fires with room to move a section out rather than at the
# moment the file has already stopped being read.
SIZE_CEILING = 130_000


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


def contents() -> "tuple[list[str], list[tuple[str, str]]]":
    """Returns the group headings and the (group, bullet) pairs of the index.

    The scan stops at the first entry heading. A `## ` heading below that point
    would be a structural change to the corpus and is not silently absorbed.
    The headings come back as their own list, in file order: the pairs repeat a
    group once per bullet, so counting duplicates in them would report every
    group with more than one entry, which is all of them.
    """
    headings: "list[str]" = []
    found: "list[tuple[str, str]]" = []
    group = ""

    for line in read(TRAPS):
        if line.startswith(ENTRY):
            break
        if line.startswith(GROUP):
            group = line[len(GROUP) :].strip()
            headings.append(group)
        elif line.startswith("- ") and group:
            found.append((group, line[2:].strip()))

    return headings, found


def agents_groups() -> "tuple[list[str], list[str]]":
    """Returns (group names, malformed lines) from AGENTS.md's `Known traps`."""
    names: "list[str]" = []
    bad: "list[str]" = []
    inside = False

    for line in read(AGENTS):
        if line.startswith("## "):
            inside = line.strip() == AGENTS_SECTION
            continue
        if not inside or not line.startswith("- "):
            continue
        match = AGENTS_GROUP.match(line)
        if match:
            names.append(match.group("name").strip())
        else:
            bad.append(line.strip())

    return names, bad


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
    """Compares the trap corpus against its index and reports any difference."""
    entries = titles()
    headings, index = contents()
    toc_groups = list(dict.fromkeys(headings))
    named_groups, malformed = agents_groups()

    known = set(entries)
    named = [title_of(bullet, known) for _, bullet in index]

    size = len(AGENTS.read_bytes().decode("utf-8"))

    print(
        f"docs/TRAPS.md: {len(entries)} entries; "
        f"table of contents: {len(index)} bullets in {len(toc_groups)} groups; "
        f"AGENTS.md names {len(named_groups)} groups, "
        f"{size:,} chars of {SIZE_CEILING:,}",
        flush=True,
    )

    problems: "list[str]" = []

    # An empty population is not a clean one. Any of these files could be moved,
    # renamed or restructured, and every one of those reads as "no difference".
    if not entries:
        problems.append(f"no `{ENTRY.strip()}` entries found in {TRAPS.name}")
    if not index:
        problems.append(f"no table-of-contents bullets found in {TRAPS.name}")
    if not toc_groups:
        problems.append(f"no `{GROUP.strip()}` groups found in {TRAPS.name}")
    if not named_groups:
        problems.append(f"no group named under `{AGENTS_SECTION}` in {AGENTS.name}")

    for line in malformed:
        problems.append(f"bullet under `{AGENTS_SECTION}` names no group: {line}")

    for title in duplicates(entries):
        problems.append(f"duplicate entry in {TRAPS.name}: {title}")
    for bullet in duplicates(named):
        problems.append(f"duplicate table-of-contents bullet in {TRAPS.name}: {bullet}")
    for group in duplicates(headings):
        problems.append(f"group listed twice in the {TRAPS.name} table of contents: {group}")
    for group in duplicates(named_groups):
        problems.append(f"group named twice in {AGENTS.name}: {group}")

    listed = set(named)
    for title in entries:
        if title not in listed:
            problems.append(f"in {TRAPS.name}, missing from its table of contents: {title}")
    for title in named:
        if title not in known:
            problems.append(f"in the table of contents, no such entry in {TRAPS.name}: {title}")

    # A bullet is its title. The set diff above cannot see a tail, which is how
    # 323 of them accumulated; this is the rule that was written down and had
    # nothing enforcing it.
    for (_, bullet), title in zip(index, named):
        if bullet != title and title in known and title not in ALLOWED_PARENTHETICAL:
            problems.append(
                f"table-of-contents bullet carries a parenthetical the entry should: {bullet}"
            )

    # And the allowlist itself, which otherwise rots into a list of exemptions
    # for entries nobody can find.
    for title in ALLOWED_PARENTHETICAL:
        if title not in known:
            problems.append(f"ALLOWED_PARENTHETICAL names no entry in {TRAPS.name}: {title}")

    # The two lists of groups, both ways. AGENTS.md says when an area is worth
    # opening and docs/TRAPS.md holds the area; a group on one side alone is
    # either an area nobody is sent to or a signpost pointing at nothing.
    for group in toc_groups:
        if group not in set(named_groups):
            problems.append(f"group in the {TRAPS.name} table of contents, not in {AGENTS.name}: {group}")
    for group in named_groups:
        if group not in set(toc_groups):
            problems.append(f"group named in {AGENTS.name}, not in the {TRAPS.name} table of contents: {group}")

    if size > SIZE_CEILING:
        problems.append(
            f"{AGENTS.name} is {size:,} chars, over the {SIZE_CEILING:,} ceiling -- "
            "move a section out to a file the index points at"
        )

    if problems:
        print(
            f"[FAIL] {len(problems)} difference(s) within docs/TRAPS.md, or between it "
            "and the\n"
            "       group list in AGENTS.md.\n"
            "       A new trap goes in one commit: the entry under a `### ` heading in "
            "docs/TRAPS.md,\n"
            "       and its title verbatim as a bullet under the matching `## ` group in "
            "that file's\n"
            "       table of contents. An index that omits an entry answers \"is there a "
            "trap about\n       this?\" with a confident no.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"       {problem}", file=sys.stderr)
        return 1

    print(
        f"[OK] every one of the {len(entries)} traps is named in the docs/TRAPS.md "
        "table of contents."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
