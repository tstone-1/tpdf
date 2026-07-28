#!/usr/bin/env python3
"""Checks that a PDF handed to tpdf from outside actually opens.

Usage:
    scripts/open_check.py <app-bundle.app> <file.pdf> [--other OTHER.pdf] [--timeout SECONDS]

Note this takes the **`.app` bundle**, not the executable inside it, because two
of the five phases go through Launch Services and there is nothing else to hand
`open`.

What each phase asserts is in `src/lib/opencheck.ts`. What this script adds is
the delivery, which is the part that cannot be arranged from inside a process:

  * `argv`         -- the terminal and Windows double-click route.
  * `double-click` -- an Apple Event to a *cold* app. This is how virtually
                      everyone will actually open a document on macOS, and
                      nothing in `argv` arrives at all.
  * `beats`        -- a handed-over document wins over the remembered one.
  * `control`      -- with nothing handed over, the remembered one opens; without
                      this, `beats` passes on an app that ignores the session
                      entirely.
  * `running`      -- an Apple Event to an app that is *already up*, which is the
                      half that goes through the event rather than the queue.

The environment does reach an app that Launch Services started --- verified, and
it is what makes the double-click phase testable rather than merely argued. Both
`open` phases capture the app's stdout with `open --stdout`.

Requires an unlocked screen for the same reason the viewer check does, and a
bundle rather than a raw binary: WKWebView needs the bundle identity or the page
never runs.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from webview_guard import require_visible_session

SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed", re.M)


def executable(bundle: Path) -> Path:
    """The binary inside a `.app`, for the phases that do not need `open`."""
    return bundle / "Contents/MacOS" / bundle.stem


def write_session(path: Path, pdf: str) -> None:
    """Plants a remembered document, so precedence has something to lose to."""
    path.write_text(
        json.dumps({"places": [{"path": pdf, "page": 3, "zoom": 1.0, "fitting": False}]})
    )


def report(phase: str, code: int, out: str) -> bool:
    """Prints a phase's transcript and says whether it is readable and green.

    Both the summary line and the exit code are read, and a disagreement between
    them is itself a failure. A run with no summary at all is a *broken run* --
    a crash, a timeout and a suspended page all print nothing, which is exactly
    what a silent pass looks like.
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


def run_direct(binary: Path, mode: str, session: Path, args: list[str], timeout: float):
    """Launches the executable itself, so stdout is ours."""
    env = dict(os.environ, TPDF_OPENCHECK=mode, TPDF_SESSION_FILE=str(session))
    try:
        done = subprocess.run(
            [str(binary), *args], env=env, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return 1, "[FAIL] run timed out\n"
    return done.returncode, done.stdout + done.stderr


def run_via_open(bundle: Path, mode: str, session: Path, pdf: str, timeout: float, scratch: Path):
    """Launches through Launch Services, which is what a double-click does."""
    captured = scratch / "open-stdout.txt"
    captured.write_text("")
    env = dict(os.environ, TPDF_OPENCHECK=mode, TPDF_SESSION_FILE=str(session))
    try:
        done = subprocess.run(
            ["open", "-a", str(bundle), "--stdout", str(captured), "--wait-apps", pdf],
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return 1, "[FAIL] run timed out\n"
    # `open` reports whether it could *launch*, never what the app concluded, so
    # the app's own exit code is unavailable here. The transcript is the verdict,
    # and `report` fails a run that produced none.
    text = captured.read_text()
    code = 0 if SUMMARY.search(text) and "[FAIL]" not in text else 1
    return code, text + done.stdout + done.stderr


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle")
    parser.add_argument("pdf")
    parser.add_argument("--other", default=None, help="a second document, for precedence")
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    bundle = Path(args.bundle).resolve()
    binary = executable(bundle)
    if not binary.exists():
        print(f"[FAIL] no executable at {binary} -- pass the .app, not the binary")
        return 1

    pdf = str(Path(args.pdf).resolve())
    other = str(Path(args.other).resolve()) if args.other else pdf

    with tempfile.TemporaryDirectory(prefix="tpdf-open-check-") as scratch:
        room = Path(scratch)
        ok = True

        # Each phase gets its own session file. Shared ones get written by the
        # phase before -- a control contaminated that way is already recorded in
        # AGENTS.md, from the session check.
        code, out = run_direct(binary, f"opened:{pdf}", room / "argv.json", [pdf], args.timeout)
        ok &= report("argv", code, out)

        code, out = run_via_open(bundle, f"opened:{pdf}", room / "click.json", pdf, args.timeout, room)
        ok &= report("double-click (Apple Event, cold)", code, out)

        if other != pdf:
            remembered = room / "beats.json"
            write_session(remembered, other)
            code, out = run_direct(binary, f"opened:{pdf}", remembered, [pdf], args.timeout)
            ok &= report("a handed-over document beats the remembered one", code, out)

            control = room / "control.json"
            write_session(control, other)
            code, out = run_direct(binary, f"opened:{other}", control, [], args.timeout)
            ok &= report("control: with nothing handed over, the remembered one opens", code, out)
        else:
            print("--- precedence ---")
            print("[SKIP] needs --other, a second document to remember")

        ok &= running_phase(binary, bundle, pdf, room, args.timeout)

    print()
    print("[OK] file associations verified" if ok else "[FAIL] file associations are not verified")
    return 0 if ok else 1


def running_phase(binary: Path, bundle: Path, pdf: str, room: Path, timeout: float) -> bool:
    """Hands a document to an app that is already up.

    The only phase that exercises the *emit* branch rather than the queue, so it
    is the only one that would notice the frontend and the backend disagreeing
    about the event's name. Started with no document so the check's own control
    -- nothing open before one is handed over -- can hold.
    """
    env = dict(os.environ, TPDF_OPENCHECK=f"arrives:{pdf}", TPDF_SESSION_FILE=str(room / "run.json"))
    app = subprocess.Popen(
        [str(binary)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    )
    try:
        # Long enough to be past the check's own quiet window, so the document
        # genuinely arrives at a running app rather than racing the boot.
        time.sleep(6)
        subprocess.run(["open", "-a", str(bundle), pdf], capture_output=True, timeout=30)
        out, _ = app.communicate(timeout=timeout)
        code = app.returncode
    except subprocess.TimeoutExpired:
        app.kill()
        out, code = "[FAIL] run timed out\n", 1
    return report("a document handed to a running app", code, out)


if __name__ == "__main__":
    sys.exit(main())
