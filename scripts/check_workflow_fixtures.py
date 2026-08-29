#!/usr/bin/env python3
"""Refuses a workflow step naming a fixture the runner cannot produce.

Every `testdata/*.pdf` a workflow step passes to a probe has to be a file that
`scripts/ci_fixtures.py` writes. A runner starts from a fresh checkout, and
`testdata/` is gitignored and empty --- so a step naming anything else is a step
that has never once run, on any machine, and it fails on the *first* run rather
than after a regression.

WHY THIS EXISTS. On 2026-08-29 four steps landed across the two workflows, all
passing `testdata/text-base14.pdf` to a probe. `ci_fixtures.py` deliberately does
not generate that file --- it comes from `make_text_pdf.py`, which embeds a
*system* font that differs per runner, and the exclusion is written out in that
file under a heading announcing itself. Both CI legs went red on the first push
that carried them, and the release workflow's copies would have blocked a tag
outright, which is the more expensive half: a release run that fails publishes
nothing at all.

What made it survive is not subtle and is worth naming, because it is the part a
check can fix. The commits carrying those steps sat unpushed for a day, so **no
run existed to be red**, and a workflow file is the one kind of source in this
repository that no local gate exercises: `scripts/gates.py` never reads a
workflow's `run:` lines, and `check_workflow_parity.py` compares the two files
against *each other*, so four identical wrong paths are perfect parity. Both
checks were green throughout. This is the third workflow-shaped defect the trap
index records, and all three have the same shape --- a claim about a workflow
that only a workflow run could refute.

WHAT IS CHECKED, in both directions and neither on its own:

  * every `testdata/<name>.pdf` appearing in a `run:` line of either workflow is
    a fixture `ci_fixtures.py` generates. That is the direction the defect went.
  * `ci_fixtures.py`'s own generated list is non-empty, which is the emptiness
    control: a regex that matched nothing there would make every path above
    "missing" and the check would fail loudly, while a regex that matched
    nothing in the *workflows* would pass silently and say nothing. The loud
    direction needs no control; the quiet one does, so the count of paths found
    in the workflows is asserted as well.

WHAT IS NOT CHECKED: whether the probe is the right one for that fixture, or
whether it passes on it. That is what the run is for. This answers only the
question a run cannot answer cheaply --- whether the file can exist at all.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = [
    ROOT / ".github" / "workflows" / "ci.yml",
    ROOT / ".github" / "workflows" / "release.yml",
]
FIXTURES = ROOT / "scripts" / "ci_fixtures.py"

# `testdata/<name>.pdf`, in either slash direction: a workflow is read on both
# platforms and a step written with a backslash would otherwise be invisible.
PATH = re.compile(r"testdata[/\\]([A-Za-z0-9._-]+)\.pdf")


def generated() -> set[str]:
    """Every fixture stem `ci_fixtures.py` writes."""
    return set(PATH.findall(FIXTURES.read_text(encoding="utf-8")))


def wanted(path: Path) -> list[tuple[int, str]]:
    """Every fixture stem a `run:` line in `path` names, with its line number.

    Only `run:` lines, because a fixture named in a *comment* is prose --- and
    this file's own docstring names `text-base14.pdf` twice, which a whole-file
    scan would report as a defect in the checker. That is the trap about a
    scanner reading its own exemption table, avoided here by scanning what
    executes rather than what is written.
    """
    out: list[tuple[int, str]] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.lstrip()
        if not (stripped.startswith("run:") or stripped.startswith("- run:")):
            continue
        for stem in PATH.findall(line):
            out.append((number, stem))
    return out


def main() -> int:
    have = generated()
    if not have:
        print(
            "[FAIL] scripts/ci_fixtures.py names no testdata fixture at all --- "
            "the pattern this check reads it with has stopped matching, so every "
            "workflow path below would read as missing"
        )
        return 1

    problems = 0
    asked = 0
    for workflow in WORKFLOWS:
        for number, stem in wanted(workflow):
            asked += 1
            if stem in have:
                continue
            problems += 1
            print(
                f"[FAIL] {workflow.relative_to(ROOT)}:{number} runs against "
                f"testdata/{stem}.pdf, which scripts/ci_fixtures.py does not "
                f"generate --- a runner starts from an empty testdata/, so this "
                f"step cannot ever have run"
            )

    if not asked:
        # The quiet direction, and the one that needs saying out loud: a `run:`
        # matcher that stopped matching would report no problems and look
        # exactly like a clean tree.
        print(
            "[FAIL] no workflow step names a testdata fixture at all --- either "
            "the probe steps were removed, or the `run:` matcher here has "
            "drifted and this check is inspecting nothing"
        )
        return 1

    if problems:
        return 1

    print(
        f"[OK]   all {asked} workflow fixture path(s) name one of the "
        f"{len(have)} fixtures a runner generates"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
