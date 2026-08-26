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
import atexit
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time

from live_output import stream_results
from webview_guard import diagnose_silence, require_visible_session

#: How long to wait for a killed app's pipes after killing it.
#:
#: Short on purpose. Reaping a process that has been sent a kill is instant; if
#: this elapses, the wait is not on the process at all but on a pipe something
#: else inherited, and no amount of further waiting ends it.
REAP_TIMEOUT = 10.0


def _kill_tree(pid: int) -> None:
    """Kills a process and everything descended from it, best effort.

    `Popen.kill` reaches one process. tpdf's render workers inherit its stdout
    and stderr, so one that outlives its parent goes on holding the write end of
    the pipe the driver is reading --- which is a wait with no end, since the
    process the driver knows about is already dead. The whole tree has to go.

    Best effort by design: this runs on a path that has already failed, and the
    transcript it exists to protect was captured before it. A failure here must
    not replace a timeout report with a traceback, so everything is swallowed.

    **POSIX does nothing here, deliberately.** The obvious counterpart is
    `os.killpg(os.getpgid(pid), 9)`, and it is wrong in this program: the app is
    launched without `start_new_session`, so it shares *this* process group ---
    and that call would kill the driver, and `mutate_viewer.py` above it, which
    is a far worse outcome than the wait it is trying to end. Giving the child
    its own session would fix that and changes how the app is launched on a
    platform this cannot be verified from, so it is not done here. The bound
    still holds on both platforms, because what ends the wait is the timeout
    around `communicate`, not this; on POSIX an orphan is left for the operator
    rather than taken with the shell it was started from.

    **And this is best effort in a second sense: it may not reach the orphan
    that motivated it.** `/T` enumerates by parent pid, and the case measured on
    2026-08-25 had a worker whose parent was *already gone* -- so whether
    taskkill still walks to it from a dead pid is untested here, and claiming it
    does would be the kind of unverified mechanism this repository keeps writing
    down. What ends the wait is the timeout above, which is bounded whatever
    this reaches. Killing by image name would certainly reach it and is not done
    on purpose: a reader may have their own installed tpdf open, and a test
    harness must not close somebody's document. The precise sweep -- kill tpdf
    processes whose parent is this pid, by asking the OS rather than taskkill --
    is the follow-up.
    """
    if sys.platform != "win32":
        return
    try:
        subprocess.run(
            ["taskkill", "/PID", str(pid), "/T", "/F"],
            capture_output=True,
            timeout=REAP_TIMEOUT,
            check=False,
        )
    except Exception:
        pass


#: A transcript of a run that recorded two checks, as `checkreport.ts` prints
#: one. The self-test's control.
#:
#: Hand-written rather than captured, deliberately: it is what this script
#: *claims* to accept, so a change to the reader that broke the shape has
#: something to fail against. The padding does not match the real column width,
#: because nothing here may depend on one.
GREEN = """[OK]   a document is open                          8 page(s)
[SKIP] a second window                            not applicable --- one window
CHECK-NAMES-JSON ["a document is open", "a second window"]

1/1 checks passed, 1 not applicable
"""

#: The wrapper's own containment verdict, which is the only check-shaped line a
#: forwarded launch produces --- and it is on stderr, which is what kept it from
#: ever being counted as a check by the harnesses above. It must not be counted
#: as one here either.
CONTAINMENT = "[OK]   the app process never mapped the PDF parser 45 modules at peak over 12 samples\n"


def self_test() -> int:
    """Every transcript this script must refuse on an exit of zero, and the one
    it accepts.

    None of it needs a screen or a bundle, which is why it is here: the launch
    half of this script cannot be exercised on a locked machine or without a
    build, and this half can be exercised anywhere, so the guard is never
    entirely unproved. Same reasoning, and the same `--self-test` spelling, as
    `mark_check.py` and `menu_check.py`.

    The load-bearing case is the last one. A reader that refuses everything
    passes all five refusals and is useless, so the accepting branch is proved
    first; and a reader keyed on *any* check-shaped line would accept a forwarded
    launch, because that run does print one.
    """
    cases = [
        ("a transcript with a check-name roll is accepted", True, GREEN, ""),
        ("a transcript with no roll at all is refused", False, "", ""),
        (
            "a roll listing no checks is refused",
            False,
            GREEN.replace('["a document is open", "a second window"]', "[]"),
            "",
        ),
        (
            "a roll that is not readable JSON is refused, not skipped over",
            False,
            GREEN.replace('["a document is open", "a second window"]', "[not json]"),
            "",
        ),
        (
            "a summary with no roll above it is refused",
            False,
            "\n1/1 checks passed\n",
            "",
        ),
        (
            "a forwarded launch, whose only check-shaped line is the wrapper's own",
            False,
            "",
            CONTAINMENT,
        ),
    ]

    ok = True
    for name, accept, out, err in cases:
        refusal = _transcript(out, err, 0)
        if (refusal is None) == accept:
            print(f"[OK]   {name}")
        else:
            got = "accepted" if refusal is None else f"refused: {refusal.splitlines()[0]}"
            print(f"[FAIL] {name}: {got}")
            ok = False
    return 0 if ok else 2


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise,
    # and then a partial transcript is an empty file. See `live_output`.
    stream_results()
    if "--self-test" in sys.argv[1:]:
        # Before the argument parser, which requires a binary and a document.
        # Demanding either would make the one part runnable without a screen the
        # part that needs a build.
        return self_test()
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

    # The app runs a watchdog of its own, and raising the bound above was worth
    # nothing while that one stayed at 300 s: it kills the process itself, so the
    # *tighter* of the two budgets is the one that decides, and it was not the
    # one anybody was editing. Measured 2026-08-03 on macOS --- `vector-multi` at
    # 275 s, 387 s and once past 600 s, `vector-heavy` killed at 300 s and then
    # green in 249 s --- so the old bound sat inside the spread and this was a
    # coin flip rather than a consistent failure, which is why it lasted. See
    # BUILD.md for the table. The watchdog is still meant to fire *first*, because
    # its timeline says where the run stopped and `communicate` can only say
    # that something took too long; that ordering is now a consequence of one
    # number rather than a coincidence of two. An explicit setting still wins,
    # so a caller can drive the watchdog on its own.
    env.setdefault("TPDF_VIEWERCHECK_TIMEOUT", str(max(30, int(args.timeout) - 60)))

    # A fixture generated with a manifest states what each of its pages should
    # read as, and the reading-order checks assert against that rather than
    # against anything the viewer computed. Passed as a path and read in Rust:
    # the webview has no filesystem, and the point of the file is that a
    # different program wrote it.
    manifest = os.path.splitext(args.pdf)[0] + "-manifest.json"
    if os.path.exists(manifest):
        env["TPDF_READING_MANIFEST"] = manifest

    # And a `-geometry.json` sidecar states every page's size and where every
    # marker was drawn, which the layout checks assert against. A second name
    # rather than a second field of the one above, because the `-manifest.json`
    # suffix *enrols* a fixture in the reading-order check: `mixed.pdf` carries
    # markers at its own corners rather than a sentence, so a manifest under that
    # name would put it in a check it was not built for and cannot pass.
    geometry = os.path.splitext(args.pdf)[0] + "-geometry.json"
    if os.path.exists(geometry):
        env["TPDF_GEOMETRY_MANIFEST"] = geometry

    # And a `-corpus.json` sidecar states what a generator put in the fixture ---
    # the words the one bare mark is drawn over, among other things. Keyed by
    # file name inside, because one generator writes several fixtures, so the
    # check looks itself up by basename rather than taking the whole file as
    # being about the document that is open.
    corpus = os.path.splitext(args.pdf)[0] + "-corpus.json"
    if os.path.exists(corpus):
        env["TPDF_CORPUS_MANIFEST"] = corpus

    # A writable path for the one phase that has to compare the overlay against
    # the *file*: it saves a copy, opens it and renders it, and the webview has
    # no filesystem of its own. Made here rather than in the app so the run owns
    # the cleanup, and named after the fixture so a killed run leaves something
    # attributable behind rather than an anonymous temp file.
    scratch = os.path.join(
        tempfile.gettempdir(),
        f"tpdf-viewercheck-{os.path.basename(args.pdf)}-{os.getpid()}.pdf",
    )
    env["TPDF_VIEWERCHECK_SCRATCH"] = scratch
    # `atexit` rather than a `finally`, because this function returns from five
    # places including the timeout path, and a cleanup that has to be repeated at
    # each of them is a cleanup that will be missed at one. A killed run leaves
    # the file, which is why it is named after the fixture and the pid.
    atexit.register(lambda: _discard(scratch))

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
        # **Bounded, and the reap is not what you are waiting for.** This was a
        # bare `process.communicate()`, which waits for the pipes to reach EOF
        # rather than for the process to die --- and a pipe is held open by
        # everything that inherited it, not by the process it was created for.
        # tpdf's render workers inherit stdout and stderr, and on 2026-08-25 a
        # pre-spawned one outlived its parent: the app was already gone, the
        # orphan held the write end, and this line blocked for 22 minutes past a
        # 420-second bound that had fired correctly. A timeout whose failure
        # path has no timeout is not a bound, and this one sat inside the very
        # code written to stop a hang. See `docs/TRAPS.md`.
        try:
            process.communicate(timeout=REAP_TIMEOUT)
        except subprocess.TimeoutExpired:
            # The pipe is still held by something we did not launch. Take the
            # whole tree rather than the process, then stop waiting on the pipes
            # altogether -- the partial transcript below comes from `expired`,
            # which was captured before any of this, so nothing is lost by
            # giving up on them.
            _kill_tree(process.pid)
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

    # Before the containment verdict, and before returning 0: a run that never
    # ran a check must not be reported as a run that passed. See `_transcript`.
    refusal = _transcript(completed.stdout or "", completed.stderr or "", completed.returncode)
    if refusal is not None:
        print(refusal, file=sys.stderr)
        return 1
    return _report_containment(watcher)


#: The roll `checkreport.ts` prints just before its summary, listing every check
#: the run recorded. Machine-readable on purpose --- the printed column is
#: `LABEL name.padEnd(46) detail`, so a long name runs into its detail and cannot
#: be parsed back out. `viewer_sweep.py` reads the same line for the same reason.
NAMES_JSON = re.compile(r"^CHECK-NAMES-JSON (\[.*\])$", re.MULTILINE)


def _transcript(out: str, err: str, code: int) -> str | None:
    """Why this exit-zero run must not be read as a pass, or `None` if it may.

    **A run that did nothing exits 0 and looks exactly like a run that passed.**
    Everything above this decides what to do about a run that *failed* --- a
    non-zero exit, a timeout, a suspended page --- and nothing asked whether the
    viewer's check had run at all. On Windows it does not have to: single-instance
    makes a second launch forward its argv to the window already open and exit 0
    immediately, so the transcript is empty, the only check-shaped line in the run
    is this wrapper's own containment `[OK]` on stderr, and the exit code is the
    one a full-marks run produces. That is a wrapper certifying a corpus it never
    opened, and it is the shape `viewer_sweep.py:349` already refuses one layer
    up --- so the sweep was protected, `mutate_viewer.py` was protected by its own
    missing-summary guard, and a direct run, which is what `BUILD.md` tells a
    reader to make, was protected by nothing.

    **Neither of those two is made dead by this.** The sweep searches for the
    roll before it looks at the exit code, so on a corpus that produced none it
    still raises its own refusal, which names the corpus and prints the run's
    last eight lines; this one fires under it and is what a reader running a
    single corpus by hand sees. The condition is shared and the reports are not.

    The observable is the roll rather than the summary or a count of `[OK]`
    lines. `finish()` emits it after the duplicate-name check and before the
    summary, so its presence says the check reached its own end, and its length
    says how much it recorded --- where a count of printed labels would also
    count this wrapper's, which is the confusion the stream split above exists to
    prevent. The roll-versus-summary arithmetic stays in `viewer_sweep.py` and is
    deliberately not repeated here: two copies of one distinction drift, and that
    one needs the numbers while this needs only the fact.

    **It names no single cause.** An empty transcript is produced by a forwarded
    launch, a crash before the first check, a bundle predating the roll, and an
    app that refused to start, and a message picking one of those sends the
    reader to rebuild something that was current. What is printed is what was
    seen.
    """
    roll = NAMES_JSON.search(out)
    if roll is not None:
        try:
            names = json.loads(roll.group(1))
        except json.JSONDecodeError as broken:
            return f"[FAIL] the run's check-name roll could not be read: {broken}"
        if names:
            return None
        seen = "the run printed an empty check-name roll: it finished having recorded nothing"
    else:
        seen = "the run printed no check-name roll, so no check in it is known to have run"
    failures = [line for line in (out + err).splitlines() if line.startswith("[FAIL]")]
    return (
        f"[FAIL] {seen}.\n"
        f"       exit={code}  stdout={len(out)} bytes  [FAIL] lines={len(failures)}\n"
        "       Causes that all look like this: on Windows a second launch\n"
        "       forwarding its argv to a window already open (close it, or see\n"
        "       `scripts/stray.py`), a crash before the first check, a bundle\n"
        "       predating the roll (rebuild with `npm run tauri build`), or an\n"
        "       app that never started."
    )


def _discard(path: str) -> None:
    """Removes the scratch copy, and says nothing if it was never written."""
    try:
        os.unlink(path)
    except OSError:
        pass


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

    **On stderr, all three verdicts, including the passing one.** This is the
    wrapper's own verdict on the run rather than one of the viewer's checks, and
    `mutate_viewer.py` takes the check names from stdout alone precisely so that
    the wrapper's lines cannot be counted as checks -- its `run_check` says so
    outright. The two `[FAIL]` forms were on stderr from the start and the `[OK]`
    was not, so on Windows the baseline carried an extra "check name" that no
    mutation could ever turn red, and a mutation whose expectation happened to be
    a prefix of this line would have been matched against the wrapper instead of
    against a check. A pass is the case where nobody looks, which is exactly why
    it is the one that drifted.
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
    print(
        f"[OK]   the app process never mapped the PDF parser {detail}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
