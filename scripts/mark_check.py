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
import sys
from pathlib import Path

import harness_launch
from harness_launch import outcome_of, run_app
from live_output import stream_results
from stray import clear_strays
from webview_guard import require_visible_session

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


def launch(binary: str, pdf: str, timeout: float) -> tuple[int, str]:
    """Runs the check once, returning its exit code and transcript.

    The launch and the transcript reader are `harness_launch.py`'s; what is left
    here is the one environment variable this check needs.
    """
    env = dict(os.environ, TPDF_MARKCHECK="1")
    done = run_app([binary, pdf], env=env, timeout=timeout)
    return done.code, done.out


def named_checks(out: str) -> "str | None":
    """The two checks this harness will not call a run green without.

    Read after the summary and the exit code have been compared and before the
    count of failures is reported, which is what `harness_launch.report`'s
    `extra` seam is for --- reporting "3 of 7 checks failed" here would point at
    the code when the fixture is what is wrong.

    The precondition is read rather than assumed. Without a document every check
    below it skips, and a transcript of skips with a green summary is exactly
    what a fixture problem looks like -- so it is named as one instead of being
    reported as a pass.

    And the keystone is the one assertion the harness exists for. A skip there is
    a legitimate outcome of the code path -- no mark was made, so there is no
    page to compare -- and it is not a legitimate outcome of a *run*: it means
    the thing this was built to check did not get checked.
    """
    got = outcome_of(out, PRECONDITION)
    if got != "OK":
        return f"the run never opened a document ({PRECONDITION!r}: {got})"
    keystone = outcome_of(out, KEYSTONE)
    if keystone != "OK":
        return f"the page-identity check did not run green ({keystone})"
    return None


def report(code: int, out: str) -> bool:
    """Prints the transcript, and says whether it is readable and green.

    The three grounds a run is refused on whatever it printed --- no summary
    line, a summary disagreeing with the exit code, a summary that is not all
    green --- are `harness_launch.report`'s, and the argument for each is written
    out there. This adds the two checks read by name, and the passing count on
    the way out: one run, one line, where a multi-phase script prints its own
    summary instead.
    """
    return harness_launch.report(out, code, extra=named_checks, announce=True)


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
