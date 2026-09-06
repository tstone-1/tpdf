#!/usr/bin/env python3
"""Launching the app for a check, and reading the transcript it prints.

Three scripts drive tpdf's in-app check reporters from outside --- `open_check.py`,
`session_check.py` and `mark_check.py` --- and each of them had grown its own copy
of the same three functions: run the binary and collect its output, find the
verdict recorded for a named check, and decide whether the run was readable and
green. The copies were not identical, which is the point. `session_check.py`
carried the full account of why the summary is parsed rather than inferred from
the exit code; `open_check.py` carried four lines of it; `mark_check.py` referred
the reader to the other two. The argument lived in one place and the code lived in
three, so the two that had lost the argument were the ones a change would reach
first.

What each of them still owns is what differs for a reason. `mark_check.py` reads
two named checks out of the transcript before it will call a run green, because a
skipped keystone is a legitimate outcome of the code path and never of a run.
`open_check.py` launches through Launch Services as well as directly, and what a
`open --wait-apps` exit code means is not what a binary's own exit code means.
Both are passed in rather than folded together.

Nothing here prints a verdict of its own beyond the transcript and the failure
lines: a helper that decides a run's outcome and reports it too leaves the caller
unable to add anything the reader needs.
"""

from __future__ import annotations

import re
import subprocess
from typing import Callable, NamedTuple

#: The reporters' last line: `N/M checks passed`. Matched at the start of a line
#: rather than anywhere, because a detail string can quote one.
SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed", re.M)

#: One recorded check. The detail is whatever follows the name, and the split is
#: on the label alone --- see `outcome_of`.
RESULT = re.compile(r"^\[(OK|FAIL|SKIP)\]\s+(.*)$")

#: What a run that never answered leaves behind, in place of a transcript.
#:
#: A constant rather than a string each caller writes out, so a caller that has
#: to tell a timeout from an ordinary failure can compare against this instead of
#: pattern-matching prose it does not own. `Run.timed_out` is the better question
#: and is what the callers here ask; this exists because the line itself ends up
#: in a transcript a reader sees.
TIMED_OUT = "[FAIL] run timed out\n"


class Run(NamedTuple):
    """One launch: its exit code, everything it printed, and whether it answered.

    `timed_out` is carried separately rather than inferred from the other two. A
    timeout and a refusal both come back as exit 1 with no summary line, and a
    caller that has to tell them apart --- `open_check.py`'s Launch Services path
    computes its own exit code from a captured file, and must not do that for a
    run that never finished --- would otherwise be reading it back out of prose.
    """

    code: int
    out: str
    timed_out: bool


def run_app(command: list[str], env: "dict[str, str] | None" = None,
            timeout: "float | None" = None) -> Run:
    """Runs one launch to completion and collects both streams.

    `text=True` alone decodes with the locale codec, which is cp1252 on Windows,
    and this transcript is document text --- a word off the page, a title, a path
    --- so a UTF-8 byte such as 0x81 kills the reader thread with
    `UnicodeDecodeError` and `.stdout` comes back `None`. The run then reports a
    traceback about `NoneType` instead of the check it was making.
    `errors="replace"` because a character in a detail string is not the verdict.
    Same fix as `viewer_check.py` carries, and it was written out four times
    across three files before it lived here.

    A timeout returns the transcript-shaped `TIMED_OUT` line rather than raising:
    every caller reports a run that did not answer as a failed phase, and a
    traceback out of a `finally` would say less than the line does.

    **Not the bounded reap `viewer_check.py` does, and deliberately.** That one
    holds the process open with `Popen` so it can print the partial transcript
    and diagnose the silence, and it then has to bound its own kill because
    tpdf's render workers inherit the pipes and a dead parent does not close
    them. `subprocess.run` kills and reaps on timeout itself, and gives up the
    partial transcript in exchange. These three checks are short phases whose
    value is the verdict rather than the timeline; the harness that needs the
    timeline already pays for it.
    """
    try:
        done = subprocess.run(
            command,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return Run(1, TIMED_OUT, True)
    return Run(done.returncode, done.stdout + done.stderr, False)


def outcome_of(out: str, name: str) -> "str | None":
    """The verdict recorded for a named check, or None when it is absent.

    Found by splitting on the label and matching the rest of the line, never by a
    fixed column. The reporters pad names to a width nobody remembers, so a
    pattern that encodes the padding stops matching the day a name grows past it
    -- silently, and in the direction that reads as good news. `docs/TRAPS.md`
    records exactly that: a mutation harness that printed SURVIVED while its own
    summary line, four lines below in the same buffer, said a check had failed.
    """
    for line in out.splitlines():
        found = RESULT.match(line)
        if found and found.group(2).startswith(name):
            return found.group(1)
    return None


def report(out: str, code: int, phase: "str | None" = None,
           extra: "Callable[[str], str | None] | None" = None,
           announce: bool = False) -> bool:
    """Prints a run's transcript, and says whether it is readable and green.

    Three separate facts, and it needs all three.

    A run that produced no summary line is a *broken run*, not a pass: a crash, a
    timeout and a suspended page all print nothing, which is exactly what a
    silent success looks like.

    The summary is parsed rather than trusted to the exit code. Written the other
    way round first, and it reported `[OK] session restore verified` under a
    phase whose own last line said `0/1 checks passed` -- because
    `AppHandle::exit` does not set a process's exit code. One number in the
    buffer disagreed with another and nothing compared them, which is the exact
    defect this repository's mutation harness had.

    So both are read, and a disagreement between them is itself a failure: it
    means one of the two stopped describing the run.

    `phase` names the run in the header and in every failure line, for a script
    that makes several. A script that makes one passes none, and its lines carry
    no prefix --- which is what its transcript looked like before this was shared.

    `extra` is a caller's own reading of the transcript, run after the two
    numbers have been compared and before the count of failed checks is
    reported. It returns a failure line or `None`. The order is what makes it
    worth a parameter: `mark_check.py` names the check whose absence means the
    run never got a document, and reporting "3 of 9 checks failed" instead of
    "the run never opened a document" points at the code when the fixture is what
    is wrong.

    `announce` prints the passing count on success. Only the script that makes a
    single run does it; in a multi-phase transcript the per-phase line would say
    the same thing several times and the caller prints one summary instead.
    """
    if phase:
        print(f"--- {phase} ---")
    print(out, end="" if out.endswith("\n") else "\n")
    label = f"{phase}: " if phase else ""

    summary = SUMMARY.search(out)
    if not summary:
        print(f"[FAIL] {label}no summary line, so the run did not finish")
        return False

    passed, total = int(summary.group(1)), int(summary.group(2))
    green = passed == total
    if green != (code == 0):
        print(f"[FAIL] {label}summary says {passed}/{total} but exit was {code}")
        return False

    if extra is not None:
        why = extra(out)
        if why is not None:
            print(f"[FAIL] {label}{why}")
            return False

    if not green:
        print(f"[FAIL] {label}{total - passed} of {total} checks failed")
        return False
    if announce:
        print(f"[OK] {passed}/{total} checks passed")
    return True
