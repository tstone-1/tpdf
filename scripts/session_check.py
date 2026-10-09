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

**A fixture too short to test stops the run, rather than colouring it.** The
`record` phase refuses a document with fewer than `EXPECTED_PAGE + 1` pages, and
that refusal used to be the *first* of eleven failures: the other three phases
launched anyway and duly reported `it opens on the remembered page: page 0,
wanted 7`, which is the signature of a broken restore and not of a wrong
fixture. The named check said which -- at the top, where a redirected run's tail
is what gets read. So the driver now reads that check's verdict and skips the
rest by name, and the verdict line says the fixture was the problem. See the
trap of the same name.

Like `viewer_check.py` this needs a *bundle*, not a raw `cargo build` binary: a
bare Mach-O opens a window and never runs a line of JavaScript.
"""

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

from harness_launch import outcome_of, report, run_app
from live_output import stream_results
from stray import clear_strays
from webview_guard import require_visible_session

# Kept in step with TARGET in `src/lib/sessioncheck.ts`. Duplicated on purpose:
# this side is what the *file* must contain, and the check inside the app is what
# the *viewer* must show. A single source would make the two agree by
# construction, which is the one thing they must not do.
EXPECTED_PAGE = 7
EXPECTED_TURNS = 1

# The phase labels, as constants because each is now written at two call sites --
# `report` where the phase ran and the skip path where it did not. `open_check.py`
# names its two branching phases the same way and for the same reason: a name
# written twice eventually differs, and the diff then shows a phase that vanished
# on one input when nothing had.
PHASE_RECORD = "record"
PHASE_DEFAULT = "control: opening without a session"
PHASE_VERIFY = "verify"
PHASE_EMPTY = "control: launching with nothing remembered"
PHASE_TABS_RECORD = "tabs: leaving three open"
PHASE_TABS_VERIFY = "tabs: every tab comes back"
PHASE_TABS_MISSING = "tabs: the front tab's file is gone"
PHASE_TABS_LEAVE = "tabs: leaving three open, preference off"
PHASE_TABS_OFF = "control: only the last document without the preference"
PHASE_SIDES_RECORD = "sides: leaving two documents side by side"
PHASE_SIDES_VERIFY = "sides: the two sides come back"

# The precondition check inside `record`, by name, so its verdict can be read.
#
# Duplicated from `src/lib/sessioncheck.ts` -- and unlike EXPECTED_PAGE above,
# where the duplication *is* the assertion, this copy is a coupling and would
# rot silently. A rename there would leave the skip path below unreachable and
# quietly restore the eleven-failure transcript it exists to prevent, which is a
# guard that stops firing without saying so. So its absence is reported as a
# failure of this script rather than treated as "the fixture is fine".
PRECONDITION = "the document is long enough to test page restore"

# The names `check_recorded_file` prints, in order, shared with the skip path so
# the two cannot drift apart.
RECORDED_FIELDS = ("path", "page", "turns", "fit", "sidebar")


def launch(binary: str, mode: str, session_file: Path, timeout: float) -> tuple[int, str]:
    """Runs one phase, returning its exit code and transcript.

    The launch itself, the transcript reader and the verdict all live in
    `harness_launch.py`: three scripts drive these reporters and each had its own
    copy, differing mostly in how much of the reasoning had survived the copy.
    What is left here is the environment one phase needs.
    """
    env = dict(os.environ, TPDF_SESSIONCHECK=mode, TPDF_SESSION_FILE=str(session_file))
    done = run_app([binary], env=env, timeout=timeout)
    return done.code, done.out


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
    wanted = {
        "path": str(Path(pdf).resolve()),
        "page": EXPECTED_PAGE,
        "turns": EXPECTED_TURNS,
        "fit": "none",
        "sidebar": True,
    }
    ok = True
    for name in RECORDED_FIELDS:
        got = place.get(name)
        good = got == wanted[name]
        ok &= good
        print(f"{'[OK]  ' if good else '[FAIL]'} recorded {name:<10} {got!r}")
    return bool(ok)


def skip_recorded_file(why: str) -> None:
    """Prints the names `check_recorded_file` would have, as skips.

    The app returned before driving to the target, so the file holds a fresh
    document's place -- page 0, upright, fitted. Comparing it would produce four
    failures describing a restore that was never attempted, which is the whole
    defect this path exists to remove.
    """
    print("--- the file the app wrote ---")
    for name in RECORDED_FIELDS:
        print(f"[SKIP] recorded {name:<10} {why}")


def tabs_argument(tabs: list[Path], front: Path) -> str:
    """The `tabs-*` phases' argument: the tabs in order, then the one in front."""
    return "|".join(str(path) for path in [*tabs, front])


def check_tabs_file(session_file: Path, tabs: list[Path], front: Path, restore: bool) -> bool:
    """Asserts the tab list the app wrote, independently of the app reading it."""
    print("--- the tab list the app wrote ---")
    try:
        session = json.loads(session_file.read_text())
    except (OSError, ValueError) as e:
        print(f"[FAIL] session file is not readable: {e}")
        return False
    wanted = {
        "tabs": [str(path) for path in tabs],
        "active_tab": str(front),
        "restore_tabs": restore,
    }
    ok = True
    for name, want in wanted.items():
        got = session.get(name)
        good = got == want
        ok &= good
        shown = [Path(p).name for p in got] if isinstance(got, list) else (
            Path(got).name if isinstance(got, str) else got)
        print(f"{'[OK]  ' if good else '[FAIL]'} recorded {name:<12} {shown!r}")
    return bool(ok)


def check_sides_file(session_file: Path, wanted: dict | None) -> bool:
    """Asserts the two sides the app wrote; `None` is a window showing one document."""
    try:
        got = json.loads(session_file.read_text()).get("sides")
    except (OSError, ValueError) as e:
        print(f"[FAIL] session file is not readable: {e}")
        return False
    good = got == wanted
    shown = got and {**got, "right": [Path(p).name for p in got.get("right", [])],
                     "beside": got.get("beside") and Path(got["beside"]).name}
    print(f"{'[OK]  ' if good else '[FAIL]'} recorded {'sides':<12} {shown!r}")
    return good


def check_tabs(binary: str, pdf: str, scratch: Path, timeout: float) -> bool:
    """Leaves three tabs open and relaunches, with the preference on and off."""
    import shutil

    # Three files, because a tab is a path: one document opened three times is
    # one tab. Named out of alphabetical order so that a strip sorted by name,
    # or by most recent, cannot pass for the order they were left in.
    docs = scratch / "tabs"
    docs.mkdir()
    tabs = [docs / name for name in ("middle.pdf", "zulu.pdf", "alpha.pdf")]
    for path in tabs:
        shutil.copyfile(pdf, path)
    front = tabs[1]
    recorded = scratch / "tabs-on.json"
    left = scratch / "tabs-off.json"

    ok = True
    code, out = launch(binary, f"tabs-record:{tabs_argument(tabs, front)}", recorded, timeout)
    ok &= report(out, code, PHASE_TABS_RECORD)
    ok &= check_tabs_file(recorded, tabs, front, True)
    # The control for the sides recorded further down: these tabs had one side.
    ok &= check_sides_file(recorded, None)

    # A copy for the second launch below: the first one rewrites the list when
    # it switches tabs, and the missing-file case must start from what was left.
    again = scratch / "tabs-missing.json"
    shutil.copyfile(recorded, again)

    code, out = launch(binary, f"tabs-verify:{tabs_argument(tabs, front)}", recorded, timeout)
    ok &= report(out, code, PHASE_TABS_VERIFY)

    code, out = launch(binary, f"tabs-leave:{tabs_argument(tabs, front)}", left, timeout)
    ok &= report(out, code, PHASE_TABS_LEAVE)
    ok &= check_tabs_file(left, tabs, front, False)

    code, out = launch(binary, f"tabs-off:{tabs_argument(tabs, front)}", left, timeout)
    ok &= report(out, code, PHASE_TABS_OFF)

    # The front tab on the right, alone, and its neighbour in front on the left.
    sides = scratch / "sides.json"
    code, out = launch(binary, f"sides-record:{tabs_argument(tabs, front)}", sides, timeout)
    ok &= report(out, code, PHASE_SIDES_RECORD)
    ok &= check_tabs_file(sides, tabs, front, True)
    ok &= check_sides_file(sides, {"right": [str(front)], "beside": str(tabs[2]), "share": 0.5})

    code, out = launch(binary, f"sides-verify:{tabs_argument(tabs, front)}", sides, timeout)
    ok &= report(out, code, PHASE_SIDES_VERIFY)

    # Last, because it deletes a file the phases above open.
    front.unlink()
    rest = [path for path in tabs if path != front]
    code, out = launch(binary, f"tabs-verify:{tabs_argument(rest, rest[0])}", again, timeout)
    ok &= report(out, code, PHASE_TABS_MISSING)
    return bool(ok)


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise,
    # and then a partial transcript is an empty file. See `live_output`.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument(
        "--only", choices=("places", "tabs"),
        help="run one half: the remembered place, or reopening every tab",
    )
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    # Before the first launch, not between phases: each phase already waits for its
    # own process to exit, so a stray here came from an *earlier* run. See `stray`
    # for why one silently absorbs every later launch on Windows.
    clear_strays(Path(args.binary))

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
        if args.only == "tabs":
            ok = check_tabs(args.binary, pdf, Path(scratch), args.timeout)
            print()
            print("[OK] reopening tabs verified" if ok else "[FAIL] reopening tabs is not verified")
            return 0 if ok else 1

        code, out = launch(args.binary, f"record:{pdf}", recorded, args.timeout)
        ok &= report(out, code, PHASE_RECORD)

        # Read before anything else is launched, because what it decides is
        # whether launching anything else can mean anything.
        verdict = outcome_of(out, PRECONDITION)
        if verdict is None:
            print(
                f"[FAIL] this script cannot find a check named {PRECONDITION!r} in the "
                "record transcript -- it has been renamed in sessioncheck.ts, so the "
                "too-short-fixture path below is now dead code"
            )
            ok = False

        if verdict == "FAIL":
            why = f"{Path(args.pdf).name} is too short -- see the record phase"
            skip_recorded_file(why)
            # The phases are named even though they did not launch, so the phase
            # list is the same shape on a fixture that cannot be tested as on one
            # that can. Their individual checks are necessarily absent: the app
            # never ran, so there was nothing to report them.
            for phase in (PHASE_DEFAULT, PHASE_VERIFY, PHASE_EMPTY):
                print(f"--- {phase} ---")
                print(f"[SKIP] {phase}: {why}")
            print()
            print(
                f"[FAIL] session restore was not tested: {Path(args.pdf).name} has too "
                f"few pages to reach page {EXPECTED_PAGE}. Rerun with a document of at "
                f"least {EXPECTED_PAGE + 1} pages."
            )
            return 1

        ok &= check_recorded_file(recorded, pdf)

        code, out = launch(args.binary, f"default:{pdf}", no_default, args.timeout)
        ok &= report(out, code, PHASE_DEFAULT)

        code, out = launch(args.binary, f"verify:{pdf}", recorded, args.timeout)
        ok &= report(out, code, PHASE_VERIFY)

        code, out = launch(args.binary, "empty", no_empty, args.timeout)
        ok &= report(out, code, PHASE_EMPTY)

        if args.only != "places":
            ok &= check_tabs(args.binary, pdf, Path(scratch), args.timeout)

    print()
    print("[OK] session restore verified" if ok else "[FAIL] session restore is not verified")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
