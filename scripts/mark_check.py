#!/usr/bin/env python3
"""Drives the reader's own marks through the real application, end to end.

Usage:
    scripts/mark_check.py <app-binary|tpdf.exe> <file.pdf> [--timeout SECONDS]

**What this covers that nothing else did.** `viewer_check.py` drives a real window
and builds its own `Viewer` with no edit model behind it, so a drag draws a
preview and commits nothing; the unit tests import modules and never `App.svelte`.
Between those two is the wiring that turns a command into a mark --- an object
literal binding the viewer's callbacks to the functions that reach the model ---
and on 2026-08-22 a reader found a defect living in exactly that gap: a shape
drawn on the last page of a document was dropped with no command sent and no
message shown, while all sixteen gates stayed green.

So this launches the app with a document, drives the route a reader takes, and
asserts against **the model** --- the marks that came back over the IPC boundary,
not the viewer that produced the gesture. `src/lib/markcheck.ts` holds the checks
and the argument for each; what this script adds is the launch and the reading of
the transcript, neither of which can be arranged from inside the process.

Takes the **binary**, not a `.app` bundle: unlike the open check there is no
Launch Services route here, and the document is handed over in `argv` like any
other spike entry point. On macOS the binary inside a bundle still needs the
bundle identity for WKWebView, so pass
`…/bundle/macos/tpdf.app/Contents/MacOS/tpdf` rather than `target/release/tpdf`.

Requires an unlocked, unoccluded screen for the same reason the viewer check does:
a suspended WebKit page does not run the check slowly, it does not run it at all.
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

from live_output import stream_results
from stray import clear_strays
from webview_guard import require_visible_session

SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed", re.M)
RESULT = re.compile(r"^\[(OK|FAIL|SKIP)\]\s+(.*)$")

#: The check whose absence means the run never got a document, by name.
#:
#: Duplicated from `markcheck.ts`, and it is a coupling rather than an assertion
#: --- a rename there would leave this unreachable and silently restore the
#: transcript of eight failures it exists to explain. So its absence is reported
#: as a failure of *this script*, never treated as "the fixture is fine".
PRECONDITION = "a document is open to put a mark on"

#: The one check that would have caught the defect this harness was written for.
#:
#: Named here so a run that somehow stopped printing it fails rather than passing
#: with one fewer line. A check that silently disappears is the failure mode this
#: repository records under "a check named by its position in a list".
KEYSTONE = "and it is recorded on the page it was pressed on"


def outcome_of(out: str, name: str) -> str | None:
    """The verdict recorded for a named check, or None when it is absent.

    Split on the label rather than read from a fixed column, for the reason
    `session_check.py` gives: `Report` pads names to a width nobody remembers,
    so a pattern encoding that padding stops matching the day a name grows past
    it --- silently, and in the direction that reads as good news.
    """
    for line in out.splitlines():
        found = RESULT.match(line)
        if found and found.group(2).startswith(name):
            return found.group(1)
    return None


def launch(binary: str, pdf: str, timeout: float) -> tuple[int, str]:
    """Runs the check once, returning its exit code and transcript."""
    env = dict(os.environ, TPDF_MARKCHECK="1")
    try:
        done = subprocess.run(
            [binary, pdf], env=env, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return 1, "[FAIL] run timed out\n"
    return done.returncode, done.stdout + done.stderr


def report(code: int, out: str) -> bool:
    """Prints the transcript, and says whether it is readable and green.

    Three separate facts, and all three are needed --- the argument is
    `session_check.py`'s and is worth restating because it was learned the hard
    way there. A run that produced no summary line is a *broken* run, not a
    pass: a crash, a timeout and a suspended page all print nothing, which is
    what a silent success looks like too. And the summary is parsed rather than
    inferred from the exit code, because `AppHandle::exit` does not set one ---
    so a disagreement between the two numbers is itself a failure, meaning one
    of them has stopped describing the run.
    """
    print(out, end="" if out.endswith("\n") else "\n")

    summary = SUMMARY.search(out)
    if not summary:
        print("[FAIL] no summary line, so the run did not finish")
        return False

    passed, total = int(summary.group(1)), int(summary.group(2))
    green = passed == total
    if green != (code == 0):
        print(f"[FAIL] summary says {passed}/{total} but exit was {code}")
        return False

    # The precondition, read rather than assumed. Without a document every check
    # below it skips, and a transcript of skips with a green summary is exactly
    # what a fixture problem looks like -- so it is named as one here instead of
    # being reported as a pass.
    got = outcome_of(out, PRECONDITION)
    if got != "OK":
        print(f"[FAIL] the run never opened a document ({PRECONDITION!r}: {got})")
        return False

    # And the one assertion the harness exists for. A skip here is a legitimate
    # outcome of the code path -- no mark was made, so there is no page to
    # compare -- and it is not a legitimate outcome of a *run*: it means the
    # thing this was built to check did not get checked.
    keystone = outcome_of(out, KEYSTONE)
    if keystone != "OK":
        print(f"[FAIL] the page-identity check did not run green ({keystone})")
        return False

    if not green:
        print(f"[FAIL] {total - passed} of {total} checks failed")
        return False
    print(f"[OK] {passed}/{total} checks passed")
    return True


#: A transcript of a clean run, as `Report` prints one. The self-test's control.
#:
#: Hand-written rather than captured, and that is the point: it is what the
#: script *claims* to accept, so a change to the reader that broke the shape has
#: something to fail against. The names match `markcheck.ts`; the padding does
#: not, deliberately, because `outcome_of` must not depend on a column width.
GREEN = """[OK]   a document is open to put a mark on           8 page(s)
[OK]   the add-comment command runs and arms the pointer   armed: note
[OK]   a comment placed by a press reaches the model    mark 1, a note
[OK]   and it is recorded on the page it was pressed on slot 0 is page 1; the mark says 1
[OK]   a shape drawn on the last page reaches the model mark 2 on page 8
7/7 checks passed
"""


def self_test() -> int:
    """Every way this script must refuse a transcript, and the one it accepts.

    **A reader that accepts everything is the instrument this repository has been
    caught by most.** `report` has four independent grounds for refusal and each
    one exists because the reassuring branch is the wrong answer: a run with no
    summary looks exactly like a silent success, `AppHandle::exit` does not set
    an exit code so the two numbers can disagree, a transcript of skips with a
    green summary is a fixture problem rather than a pass, and the keystone
    check skipping means the thing this harness was built for did not run.

    None of that needs a screen, which is the other reason it is here: the launch
    half cannot be exercised on a locked machine and this half can be exercised
    anywhere, so the script is never entirely unproved.
    """
    import io
    import contextlib

    def verdict(code: int, out: str) -> bool:
        with contextlib.redirect_stdout(io.StringIO()):
            return report(code, out)

    cases = [
        ("a clean transcript is accepted", True, 0, GREEN),
        (
            "a run with no summary line is refused, not read as silence",
            False,
            0,
            GREEN.replace("7/7 checks passed\n", ""),
        ),
        (
            "a summary that disagrees with the exit code is refused",
            False,
            1,
            GREEN,
        ),
        (
            "a failing summary is refused",
            False,
            1,
            GREEN.replace("7/7 checks passed", "6/7 checks passed"),
        ),
        (
            "a run that never opened a document is refused",
            False,
            0,
            GREEN.replace("[OK]   a document is open", "[FAIL] a document is open"),
        ),
        (
            "a skipped page-identity check is refused, not counted as green",
            False,
            0,
            GREEN.replace(
                "[OK]   and it is recorded on the page",
                "[SKIP] and it is recorded on the page",
            ),
        ),
    ]

    ok = True
    for name, want, code, out in cases:
        got = verdict(code, out)
        if got == want:
            print(f"[OK]   {name}")
        else:
            print(f"[FAIL] {name}: expected {want}, got {got}")
            ok = False

    # And that the name lookup does not depend on the column a name happens to
    # land in --- the failure `outcome_of`'s own comment describes, which reads
    # as good news because a name it cannot find is not a name that failed.
    widened = GREEN.replace(
        "[OK]   and it is recorded on the page it was pressed on ",
        "[OK]   and it is recorded on the page it was pressed on, in the model's own numbering    ",
    )
    if outcome_of(widened, KEYSTONE) == "OK":
        print("[OK]   a name found by prefix, whatever it is padded to")
    else:
        print("[FAIL] the reader is keyed on a column width")
        ok = False

    return 0 if ok else 2


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise, and
    # a partial transcript is then an empty file. See `live_output`.
    stream_results()
    if "--self-test" in sys.argv[1:]:
        # Before the argument parser, which requires a binary and a document ---
        # neither of which this needs, and demanding them would make the one part
        # runnable without a screen the part that needs a build.
        return self_test()
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    # A stray from an earlier run silently absorbs every later launch on Windows,
    # where single-instance forwards the argv and exits. See `stray`.
    clear_strays(Path(args.binary))

    pdf = str(Path(args.pdf).resolve())
    code, out = launch(args.binary, pdf, args.timeout)
    return 0 if report(code, out) else 1


if __name__ == "__main__":
    sys.exit(main())
