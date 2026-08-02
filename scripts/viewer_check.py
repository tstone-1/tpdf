#!/usr/bin/env python3
"""Runs the viewer's functional check inside a real webview.

Usage:
    scripts/viewer_check.py <app-binary> <file.pdf> [--timeout SECONDS]

What it checks is in `src/lib/viewercheck.ts`. What this script adds is the one
guard that has nothing to do with the viewer: WebKit suspends a page whose
window is not visible, so behind a lock screen the check does not fail, it
stops -- and a check that cannot report its own failure is worse than none.

It does not require a *release* build -- it asserts behaviour rather than timing
it, so a debug binary is only slower -- but it does require a **bundle**. A raw
`cargo build` executable opens a window and never runs a line of JavaScript,
because WKWebView needs the bundle identity; the failure is a blank window, no
error, and no output at all. Build with `npm run tauri build -- --bundles app`
and point this at `target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`.
(The earlier wording here said only "does not require a release bundle", which
is true and cost an afternoon.)
"""

import argparse
import os
import subprocess
import sys
import threading
import time

from live_output import stream_results
from webview_guard import diagnose_silence, require_visible_session


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise,
    # and then a partial transcript is an empty file. See `live_output`.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    # 300 s was marginal and produced an intermittent failure that looked like a
    # hang: `vector-multi` renders twelve A0 pages and measured 276 s, i.e. it
    # passed or timed out depending on the machine's mood. A timeout is not a
    # useful signal here --- nothing in this check can wedge quietly, and every
    # real failure prints a `[FAIL]` line --- so the bound exists only to stop an
    # unattended run hanging forever, and belongs well clear of the slowest
    # corpus rather than next to it.
    parser.add_argument("--timeout", type=float, default=900.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    env = dict(os.environ, TPDF_VIEWERCHECK=args.pdf)

    # A fixture generated with a manifest states what each of its pages should
    # read as, and the reading-order checks assert against that rather than
    # against anything the viewer computed. Passed as a path and read in Rust:
    # the webview has no filesystem, and the point of the file is that a
    # different program wrote it.
    manifest = os.path.splitext(args.pdf)[0] + "-manifest.json"
    if os.path.exists(manifest):
        env["TPDF_READING_MANIFEST"] = manifest

    # Launched rather than run, so that something can look at the process *while*
    # it holds a document open. `communicate` below gives back the timeout and
    # partial-transcript behaviour `run` had, which the comments underneath are
    # about and which is not being traded away for this.
    process = subprocess.Popen(
        [args.binary],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        # `text=True` alone decodes with the locale codec, which is cp1252 on
        # Windows. Every check detail the app prints is document text --- a word
        # off the page, a line read out of the accessibility tree --- so on
        # `multilingual.pdf` the stream carries UTF-8 whose bytes include 0x81,
        # undefined in cp1252, and `communicate` died inside its own reader
        # thread with `UnicodeDecodeError`. The run then produced a traceback,
        # exit 1 and a transcript file holding the word `None`: the corpus was
        # unrunnable on this platform and looked like a broken build.
        #
        # `errors="replace"` as well, because a decoder that raises here takes
        # the whole run down for a character in a *detail string*, and the
        # detail is not the verdict. Same fix as `mutate_rust.py` and
        # `mutate_frontend.py` carry, arriving in the third harness.
        encoding="utf-8",
        errors="replace",
    )
    watcher = _watch_modules(process)

    try:
        stdout, stderr = process.communicate(timeout=args.timeout)
        completed = subprocess.CompletedProcess(
            [args.binary], process.returncode, stdout, stderr
        )
    except subprocess.TimeoutExpired as expired:
        # Diagnosed *before* the kill, because the observable is the live
        # process's CPU time and there is none to sample afterwards. Two seconds
        # on a run that has already failed, in exchange for the difference between
        # "an occluded window" and "a defect in what you just changed" --- which
        # this printed identically until now.
        silence = diagnose_silence(process.pid)
        process.kill()
        process.communicate()
        # The partial transcript, not just the verdict. `viewercheck.ts` prints
        # each result as it is recorded precisely so a run that stops midway can
        # say where --- and discarding it here threw that away again, leaving a
        # timeout indistinguishable from a page that never ran a line of
        # JavaScript. Both were seen on the same corpus within an hour.
        partial = expired.stdout or b""
        if isinstance(partial, bytes):
            partial = partial.decode("utf-8", "replace")
        print(partial, end="")
        done = partial.count("[OK]") + partial.count("[FAIL]") + partial.count("[SKIP]")
        print(f"[FAIL] run timed out after {args.timeout:.0f}s, {done} checks in", file=sys.stderr)
        print(f"[FAIL] {silence}", file=sys.stderr)
        return 1

    print(completed.stdout, end="")
    if completed.returncode != 0:
        print(completed.stderr, end="", file=sys.stderr)
        print(f"[FAIL] exit {completed.returncode}", file=sys.stderr)
        return completed.returncode

    # A passing run's stderr was discarded entirely, which made every warning the
    # app prints invisible to exactly the runs that succeed --- the uncontained
    # backend announces itself there, and a full-marks Windows run showed nothing.
    # The whole line is echoed rather than summarised, and only lines that
    # announce themselves as warnings, so a passing run stays quiet about the
    # webview's ordinary teardown noise.
    for line in (completed.stderr or "").splitlines():
        if "[WARN]" in line:
            print(line, file=sys.stderr)
    return _report_containment(watcher)


def _watch_modules(process: "subprocess.Popen[str]"):
    """Samples the app's loaded modules for as long as it runs. Windows only.

    A thread rather than one look, because the parser is mapped at the moment a
    document is opened and unmapped again when it is closed --- a single sample
    could miss it in either direction and would then report containment that is
    not there. What is accumulated is a *union*: the parser having been mapped at
    any instant is the failure, so the check cannot be passed by sampling at a
    quiet moment.
    """
    if sys.platform != "win32":
        return None

    from win_modules import maps_parser

    state = {"mapped": False, "peak": 0, "samples": 0}

    def sample() -> None:
        while process.poll() is None:
            mapped, count = maps_parser(process.pid)
            state["samples"] += 1
            state["mapped"] = state["mapped"] or mapped
            state["peak"] = max(state["peak"], count)
            time.sleep(0.05)

    thread = threading.Thread(target=sample, daemon=True)
    thread.start()
    return state


def _report_containment(state) -> int:
    """Prints the one check this harness adds, and decides the exit code.

    Reported separately from the viewer's own results and deliberately not folded
    into them: `BUILD.md` records the count of check *names* `viewercheck.ts`
    produces as the cross-platform invariant, and quietly adding a Windows-only
    name to that set would make the two platforms look divergent when they are
    not.
    """
    if state is None:
        return 0
    # The control first. An enumeration that read nothing reports "not mapped"
    # exactly as containment does, so a peak of zero is a broken observation and
    # must never be allowed to read as good news.
    if state["peak"] == 0:
        print(
            f"[FAIL] the app process could not be read at all "
            f"({state['samples']} samples, 0 modules seen)",
            file=sys.stderr,
        )
        return 1
    detail = f"{state['peak']} modules at peak over {state['samples']} samples"
    if state["mapped"]:
        print(f"[FAIL] the app process mapped the PDF parser   {detail}", file=sys.stderr)
        return 1
    print(f"[OK]   the app process never mapped the PDF parser {detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
