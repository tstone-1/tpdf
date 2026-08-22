#!/usr/bin/env python3
"""Asserts the two workflows' `gates` jobs run the same steps.

`ci.yml` and `release.yml` each carry a job named `gates`, and they must be the
same job: CI's is what says a commit is good, and the release one is what stops
a tag on a broken commit producing artifacts. If the release copy is weaker,
every ordinary push is checked harder than the thing that actually ships.

They drifted exactly that way. `release.yml`'s gates job was written from
`ci.yml` and the copy lost the fixture-generation step, so a unit test needing
`rotated.pdf` failed on both release runners while passing in CI and locally ---
found on 2026-08-03 by the first tag this repository ever pushed, which is a
late and expensive place to find it.

`AGENTS.md` already states the rule this breaks: a release checklist that
restates a gating command quietly loses a `--locked` and then tests something
weaker. The comments in both files said so at the time and did not prevent it,
because a comment is not a thing that can go red. This is, and that is the whole
point of writing it as a script rather than a third warning.

WHAT IS COMPARED, and why it is the sequence rather than the text: the ordered
list of what each step *executes* --- every `uses:` with its pinned SHA and every
`run:` body, whitespace-normalised. Step `name:`s are not compared, since two
identical commands under different labels are still the same job, and the labels
are prose. Order is compared because these steps are a sequence with real
dependencies: fetching PDFium after running the gates would pass a set
comparison and fail every time.

WHAT IS NOT COMPARED: anything outside the `gates` job. The `release` job has no
counterpart in `ci.yml` by design, and the matrix and triggers are deliberately
different --- CI runs on `pull_request` with no secrets, and that difference is
the fork threat model rather than drift.

THE SECOND INVARIANT, added 2026-08-22: what authority a gates job holds while
it runs third-party code. Parity is blind to it by construction --- it compares
what the steps *execute*, not the `permissions:` or the `with:` blocks around
them --- and an outside review found the gap in the direction that costs most.
`release.yml` declared `contents: write` at workflow level, every job inherited
it, and the gates job then checked out with the default credential-persisting
checkout and `pip install`ed unpinned pyhanko from PyPI *before* running any
gate. So the newest release of a third-party package executed with a token that
can write to this repository.

Three properties close it, and each one has to hold in both files because the
whole point of the parity script is that these two jobs are one job:

  1. the `gates` job declares `permissions: contents: read` of its own, or the
     whole workflow does;
  2. its checkout sets `persist-credentials: false`;
  3. its Python install names a pinned requirements file rather than package
     names, so what runs is decided by a committed file rather than by PyPI at
     the moment the job starts.

Usage: scripts/check_workflow_parity.py
"""

from __future__ import annotations

import pathlib
import re
import sys

#: Repository root, taken from this file rather than the working directory.
ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The two workflows and the job that must agree between them.
WORKFLOWS = (".github/workflows/ci.yml", ".github/workflows/release.yml")
JOB = "gates"


def job_block(text: str, job: str) -> list[str]:
    """Returns the lines of one job, from its key to the next job's.

    Jobs sit at two-space indent under `jobs:`, so the block ends at the next
    line indented exactly that far. Written by hand rather than with a YAML
    parser because the gates themselves must run on a bare interpreter, and
    PyYAML is not a dependency of this repository.
    """
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if line == f"  {job}:":
            start = i + 1
            break
    if start is None:
        return []
    end = len(lines)
    for i in range(start, len(lines)):
        if re.match(r"^  \S", lines[i]):
            end = i
            break
    return lines[start:end]


def steps(block: list[str]) -> list[str]:
    """Extracts what each step executes, in order.

    A step is either `uses:` (an action, compared with its pinned SHA) or `run:`
    (a command, possibly a `|` block). Comments and `name:`/`with:`/`env:` are
    skipped --- see the module docstring for why names are not compared.
    """
    out: list[str] = []
    i = 0
    while i < len(block):
        line = block[i]
        uses = re.match(r"^\s*(?:- )?uses:\s*(\S+)", line)
        if uses:
            out.append(f"uses {uses.group(1)}")
            i += 1
            continue
        run = re.match(r"^(\s*)(?:- )?run:\s*(.*)$", line)
        if run:
            indent, first = run.group(1), run.group(2).strip()
            if first in ("|", ">"):
                # A block scalar: take every line indented past the `run:` key.
                body: list[str] = []
                i += 1
                while i < len(block) and (
                    not block[i].strip() or len(block[i]) - len(block[i].lstrip()) > len(indent)
                ):
                    if block[i].strip():
                        body.append(block[i].strip())
                    i += 1
                out.append("run " + " ; ".join(body))
                continue
            out.append("run " + " ".join(first.split()))
            i += 1
            continue
        i += 1
    return out


#: The requirements file the fixture-tool install has to name.
PINNED_TOOLS = "scripts/fixture-tools.txt"


def authority(workflow: str, block: list[str], whole: str) -> list[str]:
    """What is wrong with one `gates` job's authority, or nothing.

    See the module docstring for the three properties and what went wrong
    without them. Each is checked against the text of the job rather than a
    parsed document, for `job_block`'s reason: these gates run on a bare
    interpreter and PyYAML is not a dependency here.
    """
    wrong: list[str] = []
    body = "\n".join(block)

    # 1. Read-only, declared either on this job or on the workflow. Job level
    #    wins where both exist, and either is enough -- what must not happen is
    #    inheriting `contents: write` because nothing said otherwise.
    job_level = re.search(r"^\s{4}permissions:\s*$\n\s{6}contents:\s*read\s*$", body, re.M)
    file_level = re.search(r"^permissions:\s*$\n\s{2}contents:\s*read\s*$", whole, re.M)
    if not (job_level or file_level):
        wrong.append(
            f"{workflow}: the '{JOB}' job can write to the repository -- it installs and runs "
            f"third-party code before the gates, so give it 'permissions:\\n  contents: read'"
        )

    # 2. The checkout must not leave a pushable credential in `.git/config`.
    if "actions/checkout@" in body and "persist-credentials: false" not in body:
        wrong.append(
            f"{workflow}: the '{JOB}' job checks out with credentials persisted -- add "
            f"'persist-credentials: false', since nothing here does a git operation"
        )

    # 3. Whatever Python it installs comes from a committed file. Matched on the
    #    file name rather than on the absence of package names, so a second
    #    install step added later has to opt in rather than being missed.
    # Over what the steps *execute*, not over the block's text: the comments
    # above the install step discuss `pip install` at length, and a scan of raw
    # lines reported them as unpinned installs.
    installs = [line for line in steps(block) if "pip install" in line]
    for line in installs:
        if PINNED_TOOLS not in line:
            wrong.append(
                f"{workflow}: the '{JOB}' job installs Python packages without pinning them "
                f"({line.strip()[:70]}) -- install '-r {PINNED_TOOLS}' instead"
            )
    if not installs:
        # An empty scan reads exactly like a clean one, which is the failure
        # this repository writes traps about. If the install step is ever
        # removed the check has to say so rather than pass in silence.
        wrong.append(f"{workflow}: the '{JOB}' job has no pip install -- this check found nothing")
    return wrong


def main() -> int:
    """Compares the two jobs and reports the first difference."""
    found: dict[str, list[str]] = {}
    for workflow in WORKFLOWS:
        path = ROOT / workflow
        if not path.exists():
            print(f"[FAIL] {workflow} does not exist")
            return 1
        block = job_block(path.read_text(encoding="utf-8"), JOB)
        if not block:
            # A job that cannot be located reads exactly like two jobs that
            # agree, since both step lists would then be empty.
            print(f"[FAIL] {workflow} has no job named '{JOB}'")
            return 1
        found[workflow] = steps(block)
        if not found[workflow]:
            print(f"[FAIL] {workflow}'s '{JOB}' job has no steps -- the scan found nothing")
            return 1
        wrong = authority(workflow, block, path.read_text(encoding="utf-8"))
        if wrong:
            for line in wrong:
                print(f"[FAIL] {line}")
            return 1

    left, right = (found[w] for w in WORKFLOWS)
    if left == right:
        print(f"[OK]   both '{JOB}' jobs run the same {len(left)} steps, in the same order")
        print(f"[OK]   both read-only, both checked out without credentials, both installing "
              f"{PINNED_TOOLS}")
        return 0

    print(f"[FAIL] the '{JOB}' jobs have drifted")
    for i in range(max(len(left), len(right))):
        a = left[i] if i < len(left) else "(absent)"
        b = right[i] if i < len(right) else "(absent)"
        marker = "  " if a == b else "->"
        print(f"{marker} {i + 1:2}. {WORKFLOWS[0]}: {a[:100]}")
        print(f"{marker}     {WORKFLOWS[1]}: {b[:100]}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
