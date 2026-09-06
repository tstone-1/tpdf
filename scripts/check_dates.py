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
tracked files, every one reading a day or two ahead of the commit that wrote
it. Nothing was wrong with the measurements; the provenance on all of them was.

The dates that defect wrote are described here rather than quoted, because this
file is scanned like every other tracked file and a quoted future date would be
a finding about the checker. Its *table* is a different case, handled below.

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

# The table above has to spell its dates out in Python source, and this file is
# tracked text like any other -- so on the day the gate landed it read its own
# three keys as future stamps and failed on both CI legs, having never once been
# green. A date *here* is a key, not a claim about when something was measured.
#
# Derived from EXEMPT rather than listed, so a fourth exemption cannot re-break
# the gate, and scoped to the keys rather than to the file: a future date in this
# file that no exemption names is still a finding, which is what stops this from
# becoming the whole-file excuse the comment above rejects.
#
# From `__file__` rather than a constant, because a constant would name nothing
# after a rename. `as_posix()` because `git ls-files` reports forward slashes on
# every platform, Windows included.
SELF = Path(__file__).resolve().relative_to(ROOT).as_posix()
EXEMPT_DATES = {when for _, when in EXEMPT}


def head_author_date() -> "datetime.date | None":
    """The author date of `HEAD`, on the author's own calendar.

    `%aI` carries the author's UTC offset, so the first ten characters are the
    date the author saw when they committed, whatever clock the machine running
    this gate keeps. `None` when there is no repository to ask, which the caller
    treats as "the machine's own date is all there is".
    """
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), "log", "-1", "--format=%aI", "HEAD"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=True,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    try:
        return datetime.date.fromisoformat(out.stdout.strip()[:10])
    except ValueError:
        return None


def latest_permitted(
    today: "datetime.date | None" = None,
    head: "datetime.date | None" = None,
) -> datetime.date:
    """The latest date a stamp may carry: the later of two calendars.

    **Not the machine's own date alone.** A stamp written at 00:30 local on the
    7th is the 6th in UTC, and a CI runner is on UTC --- so a gate keyed on
    `date.today()` was green on the machine that wrote the stamp and red on the
    runner minutes later, on a file nobody had touched. The commit that carries
    the stamp knows better: its author date is on the author's own calendar,
    offset and all, and a stamp no later than that commit's date is one the
    author could have written.

    **Not "today anywhere on Earth" either.** That was the first fix, on
    2026-09-06: accept any date that is today somewhere, i.e. UTC+14. It closed
    the runner case and opened a worse one --- from 10:00 UTC onwards it accepts
    *tomorrow*, so the same afternoon twenty-four stamps for the 7th were written
    on the 6th and passed. A gate that accepts tomorrow every evening is not a
    gate on the future.

    So the threshold is the later of the machine's date and `HEAD`'s author
    date. On the runner the author date carries the after-midnight stamp; on
    the machine that wrote it, `today` already does; and nowhere does either
    calendar reach a day that has not started for whoever is writing.

    Both arguments are parameters so this is answerable without waiting for a
    date to come round; nothing in the repository passes them.
    """
    today = today or datetime.date.today()
    head = head if head is not None else head_author_date()
    return max(today, head) if head else today


def tracked() -> "list[str]":
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files"],
        capture_output=True,
        text=True,
        # Decoded explicitly rather than by the locale codec: git prints a path
        # outside ASCII verbatim when `core.quotePath` is off, and a cp1252
        # console would then turn one such file into a UnicodeDecodeError with
        # no listing at all.
        encoding="utf-8",
        errors="replace",
        check=True,
    )
    return out.stdout.split()


def main(
    today: "datetime.date | None" = None,
    head: "datetime.date | None" = None,
) -> int:
    today = latest_permitted(today, head)
    try:
        files = tracked()
    except (subprocess.CalledProcessError, FileNotFoundError) as why:
        print(f"[FAIL] could not list tracked files: {why}")
        return 2

    # **A listing that succeeds and returns nothing.** The branch above catches
    # git failing; it does not catch git answering emptily, and until 2026-09-02
    # this gate then printed `[OK] no date ahead of ... in 0 tracked text files`
    # and exited 0. The population was reported honestly and asserted by nothing,
    # which is the one shape every other gate in `scripts/` already guards.
    #
    # Measured rather than imagined: `GIT_INDEX_FILE=/tmp/no-such-index` makes
    # `git ls-files` print nothing and exit 0, and that is the mutation
    # `scripts/mutate_python.py` uses. A scan over no files agrees with a clean
    # tree about every date in the repository.
    if not files:
        print(
            "[FAIL] `git ls-files` listed no file at all, so this run scanned "
            "nothing.\n       A green verdict here would be about an empty "
            "population, not a clean tree."
        )
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
                if when <= today:
                    continue
                stamp = when.isoformat()
                if (name, stamp) in EXEMPT:
                    continue
                if name == SELF and stamp in EXEMPT_DATES:
                    continue
                ahead.append(f"{name}:{line_no}: {stamp}")

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
