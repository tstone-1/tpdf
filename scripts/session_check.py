#!/usr/bin/env python3
"""Runs the session-restore check across four launches of the real app.

Usage:
    scripts/session_check.py <app-binary> <file.pdf> [--timeout SECONDS]

What each phase asserts is in `src/lib/sessioncheck.ts`. What this script adds
is the part that cannot live inside the app: session restore is a property *of*
a launch, so it takes more than one, and the session file has to be inspected
from outside the process that wrote it.

Two guards here matter as much as the phases.

**The session file is a temporary one**, handed to every launch through
`TPDF_SESSION_FILE`. Without it the check would read and overwrite whatever the
person using this machine was last reading -- and a check that destroys the
state it checks cannot be run twice.

**The recorded file is inspected between the phases.** The app writing a place
and the app reading one back are different halves, and a check that only ran the
second would pass on a session file that was never written -- it would simply
find nothing to restore and say so somewhere else.

Like `viewer_check.py` this needs a *bundle*, not a raw `cargo build` binary: a
bare Mach-O opens a window and never runs a line of JavaScript.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from live_output import stream_results
from webview_guard import require_visible_session

# Kept in step with TARGET in `src/lib/sessioncheck.ts`. Duplicated on purpose:
# this side is what the *file* must contain, and the check inside the app is what
# the *viewer* must show. A single source would make the two agree by
# construction, which is the one thing they must not do.
EXPECTED_PAGE = 7
EXPECTED_TURNS = 1


def launch(binary: str, mode: str, session_file: Path, timeout: float) -> tuple[int, str]:
    """Runs one phase, returning its exit code and transcript."""
    env = dict(os.environ, TPDF_SESSIONCHECK=mode, TPDF_SESSION_FILE=str(session_file))
    try:
        done = subprocess.run(
            [binary], env=env, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return 1, "[FAIL] run timed out\n"
    return done.returncode, done.stdout + done.stderr


SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed", re.M)


def report(phase: str, code: int, out: str) -> bool:
    """Prints a phase's transcript, and says whether it is readable and green.

    Three separate facts, and it needs all three.

    A run that produced no summary line is a *broken run*, not a pass: a crash,
    a timeout and a suspended page all print nothing, which is exactly what a
    silent success looks like.

    The summary is parsed rather than trusted to the exit code. Written the
    other way round first, and it reported `[OK] session restore verified` under
    a phase whose own last line said `0/1 checks passed` -- because
    `AppHandle::exit` does not set a process's exit code. One number in the
    buffer disagreed with another and nothing compared them, which is the exact
    defect this repository's mutation harness had.

    So both are read, and a disagreement between them is itself a failure: it
    means one of the two stopped describing the run.
    """
    print(f"--- {phase} ---")
    print(out, end="" if out.endswith("\n") else "\n")

    summary = SUMMARY.search(out)
    if not summary:
        print(f"[FAIL] {phase}: no summary line, so the run did not finish")
        return False

    passed, total = int(summary.group(1)), int(summary.group(2))
    green = passed == total
    if green != (code == 0):
        print(f"[FAIL] {phase}: summary says {passed}/{total} but exit was {code}")
        return False
    if not green:
        print(f"[FAIL] {phase}: {total - passed} of {total} checks failed")
        return False
    return True


def check_recorded_file(session_file: Path, pdf: str) -> bool:
    """Asserts the file the app wrote says what the app was driven to."""
    print("--- the file the app wrote ---")
    if not session_file.exists():
        print("[FAIL] no session file was written")
        return False

    try:
        places = json.loads(session_file.read_text())["places"]
    except (ValueError, KeyError) as e:
        print(f"[FAIL] session file is not readable: {e}")
        return False

    if not places:
        print("[FAIL] session file has no places in it")
        return False

    place = places[0]
    ok = True
    for name, got, want in (
        ("path", place.get("path"), str(Path(pdf).resolve())),
        ("page", place.get("page"), EXPECTED_PAGE),
        ("turns", place.get("turns"), EXPECTED_TURNS),
        ("fitting", place.get("fitting"), False),
        ("sidebar", place.get("sidebar"), True),
    ):
        good = got == want
        ok &= good
        print(f"{'[OK]  ' if good else '[FAIL]'} recorded {name:<10} {got!r}")
    return bool(ok)


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise,
    # and then a partial transcript is an empty file. See `live_output`.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    pdf = str(Path(args.pdf).resolve())

    with tempfile.TemporaryDirectory(prefix="tpdf-session-check-") as scratch:
        recorded = Path(scratch) / "recorded.json"
        # A file of its own for each control, not one shared between them.
        #
        # Shared first, and the `empty` control failed: the `default` phase had
        # opened a document into it, so by the time `empty` launched there *was*
        # something to remember and a document duly opened. The control was
        # contaminated by the phase before it -- which is the standing rule about
        # what one variant leaves behind for the next, arriving somewhere that
        # did not look like an A/B at all.
        no_default = Path(scratch) / "control-default.json"
        no_empty = Path(scratch) / "control-empty.json"

        ok = True
        code, out = launch(args.binary, f"record:{pdf}", recorded, args.timeout)
        ok &= report("record", code, out)
        ok &= check_recorded_file(recorded, pdf)

        code, out = launch(args.binary, f"default:{pdf}", no_default, args.timeout)
        ok &= report("control: opening without a session", code, out)

        code, out = launch(args.binary, f"verify:{pdf}", recorded, args.timeout)
        ok &= report("verify", code, out)

        code, out = launch(args.binary, "empty", no_empty, args.timeout)
        ok &= report("control: launching with nothing remembered", code, out)

    print()
    print("[OK] session restore verified" if ok else "[FAIL] session restore is not verified")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
