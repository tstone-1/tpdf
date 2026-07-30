#!/usr/bin/env python3
"""Checks that a PDF handed to tpdf from outside actually opens.

Usage:
    scripts/open_check.py <app-bundle.app|tpdf.exe> <file.pdf> [--other OTHER.pdf] [--timeout SECONDS]

On macOS this takes the **`.app` bundle**, not the executable inside it, because the
cold-double-click phase goes through Launch Services and there is nothing else to hand
`open`. On Windows it takes the **executable**: there is no bundle and WebView2 needs no
bundle identity.

**Five of the six phases run on both platforms.** Only the cold double-click is macOS-only,
and for a reason that is not a gap: an Explorer double-click hands the path over in `argv`,
which the `argv` phase already covers, so there is no second mechanism there to test. The
handover to a *running* app used to skip on Windows too and no longer does --- see
`HANDS_OVER_TO_RUNNING`. A phase with no route prints `[SKIP]` with its reason rather than
disappearing, so the phase names are the same list on both platforms.

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
  * `race`         -- two opens issued without waiting for the first, the case
                      `openPath`'s chain exists for. Issued from inside the app:
                      Launch Services hands over one document at a time, so the
                      overlap cannot be arranged from out here.
  * `running`      -- an Apple Event to an app that is *already up*, which is the
                      half that goes through the event rather than the queue.

The environment does reach an app that Launch Services started --- verified, and
it is what makes the double-click phase testable rather than merely argued. Both
`open` phases capture the app's stdout with `open --stdout`.

Requires an unlocked screen for the same reason the viewer check does. On macOS it
also needs a bundle rather than a raw binary, because WKWebView needs the bundle
identity or the page never runs; on Windows it needs a binary built with
`--features tauri/custom-protocol`, or the window shows "localhost refused to
connect" and no phase produces a summary. Note `webview_guard` returns early off
darwin, so on Windows nothing here protects against an *occluded* window --- and
Chromium throttles those too. Keep the window visible; see the trap.
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

from live_output import stream_results
from stray import clear_strays
from webview_guard import require_visible_session

SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed", re.M)


COLD_CLICK = "double-click (Apple Event, cold)"
"""The cold-launch phase's name, written once.

Two call sites use it --- `report` where the route exists and `skip` where it does
not --- and a phase name is what a reader diffs across platforms to see that the
list is the same one. Two copies of it would eventually differ, and the diff would
then show a check that had vanished on one platform when nothing had.
"""

RUNNING_HANDOVER = "a document handed to a running app"
"""The running-handover phase's name. See [`COLD_CLICK`]."""

USES_LAUNCH_SERVICES = sys.platform == "darwin"
"""Whether launching goes through Launch Services and an `.app` bundle.

Split out from [`HANDS_OVER_TO_RUNNING`] on 2026-07-30, because that constant had been
standing for two different facts and flipping it for Windows broke the run instantly:
the harness demanded a `.app` on a platform that has none. They coincided only while
macOS was the only platform with a handover.

The two are genuinely independent. This one governs *how a launch is spelled* --- an
`.app` bundle, `open -a`, an Apple Event for a double-click. The other governs *whether a
second launch reaches the first process*, which Windows now does through argv forwarding
with no Launch Services anywhere in it.
"""

HANDS_OVER_TO_RUNNING = True
"""Whether this platform delivers a document to an app that is already running.

**Both platforms do, since 2026-07-30, by different routes.** macOS receives an Apple
Event in `RunEvent::Opened`, which is `#[cfg(target_os = "macos")]`. Windows had no
counterpart and this constant was `sys.platform == "darwin"` --- measured, not inferred:
two launches produced **two independent processes**, each with its own window and its own
worker pool, where macOS produced one app that swapped documents. That is now closed by
`tauri-plugin-single-instance`, whose callback forwards the second process's argv to the
first and exits, feeding the same `Launch` queue and emitting the same `OPEN_EVENT`.

Kept as a named constant rather than deleted along with the skip. It is the thing to flip
if a third platform arrives without a handover route, and it documents that the phase
below tests one *property* reached two ways rather than two different properties.

Named once rather than inlined, because `AGENTS.md` records what becomes of two copies
of a platform distinction --- and this file has just paid for the opposite mistake, one
name standing for two distinctions. See [`USES_LAUNCH_SERVICES`].
"""


def executable(target: Path) -> Path:
    """The binary to launch directly, given a bundle or a bare executable.

    Takes either, because the two platforms hand over different things. macOS
    needs the `.app` --- Launch Services has nothing else to accept, and WKWebView
    needs the bundle identity or the page never runs. Windows has no bundle:
    WebView2 needs no identity, so a `target/release/tpdf.exe` built with
    `--features tauri/custom-protocol` is the whole story.
    """
    if target.suffix == ".app":
        return target / "Contents/MacOS" / target.stem
    return target


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


def skip(phase: str, reason: str) -> bool:
    """Records a phase that cannot run here, in the same shape as one that did.

    Returns `True`: a phase with no route on this platform is not a failure. But
    it is printed, with its name and its reason --- `AGENTS.md` records that a
    check which silently stops existing on some inputs cannot be told apart from
    one that ran, and the name is what a reader diffs across platforms.
    """
    print(f"--- {phase} ---")
    print(f"[SKIP] {reason}")
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


"""How many launches the race phase gets.

Each launch is one cold attempt at the interleaving, and a cold attempt is the
only kind worth having: repeating the round *inside* a launch was measured and
is worse than useless, because every round after the first runs against warmed
workers and an already-open document and lands in the same order every time.

A single cold attempt reports a removed queue about two runs in three, so four
of them miss it roughly once in eighty. That is a smoke check and this file
should not pretend otherwise -- the property itself is pinned deterministically
by `src/lib/serial.test.ts`, which runs in the gates. What only this can say is
that the application still routes its opens through that queue.
"""
RACE_LAUNCHES = 4


def race_phase(binary: Path, pdf: str, other: str, room: Path, timeout: float) -> bool:
    """Two overlapping opens, from a cold start, several times over.

    No document handed over and no session, so each launch starts on the empty
    state the phase's own control asserts. Both opens are issued from inside the
    app: what is being tested is the serialisation of two calls that overlap,
    which nothing out here can arrange, since Launch Services delivers one
    document at a time.
    """
    ok = True
    for launch in range(1, RACE_LAUNCHES + 1):
        session = room / f"race-{launch}.json"
        code, out = run_direct(binary, f"race:{pdf}|{other}", session, [], timeout)
        ok &= report(f"two overlapping opens, launch {launch}/{RACE_LAUNCHES}", code, out)
    return ok


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise,
    # and then a partial transcript is an empty file. See `live_output`.
    stream_results()
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
    # See `stray`. This harness is the one that *depends* on the single-instance
    # forwarding for its handover phase, which makes it the one a leftover instance
    # misleads most: the handover would appear to work while going to the wrong
    # process entirely.
    clear_strays(binary)
    if not binary.exists():
        hint = "pass the .app, not the binary" if USES_LAUNCH_SERVICES else "pass the .exe"
        print(f"[FAIL] no executable at {binary} -- {hint}")
        return 1
    if USES_LAUNCH_SERVICES and bundle.suffix != ".app":
        print(f"[FAIL] {bundle} is not a .app -- two phases go through Launch Services")
        return 1

    pdf = str(Path(args.pdf).resolve())
    other = str(Path(args.other).resolve()) if args.other else pdf

    # `ignore_cleanup_errors` because the handover phase redirects the app's stdout to a
    # file in here, and the app's render workers **inherit that handle** --- so on Windows
    # the file can still be locked for a moment after every check has passed, and
    # `PermissionError: [WinError 32]` would fail a run whose phases were all green.
    #
    # Worth noting which way that traded, because it is the same inheritance in both
    # directions: with a *pipe* the consequence was a silent timeout with no transcript
    # before any result; with a *file* it is a loud unlink error after every result. The
    # second is strictly the better failure, and this line downgrades it to nothing.
    with tempfile.TemporaryDirectory(
        prefix="tpdf-open-check-", ignore_cleanup_errors=True
    ) as scratch:
        room = Path(scratch)
        ok = True

        # Each phase gets its own session file. Shared ones get written by the
        # phase before -- a control contaminated that way is already recorded in
        # AGENTS.md, from the session check.
        code, out = run_direct(binary, f"opened:{pdf}", room / "argv.json", [pdf], args.timeout)
        ok &= report("argv", code, out)

        if USES_LAUNCH_SERVICES:
            code, out = run_via_open(
                bundle, f"opened:{pdf}", room / "click.json", pdf, args.timeout, room
            )
            ok &= report(COLD_CLICK, code, out)
        else:
            ok &= skip(
                COLD_CLICK,
                "no such route here --- an Explorer double-click hands the path over in argv, "
                "which the phase above already covers, so there is no second mechanism to test",
            )

        if other != pdf:
            remembered = room / "beats.json"
            write_session(remembered, other)
            code, out = run_direct(binary, f"opened:{pdf}", remembered, [pdf], args.timeout)
            ok &= report("a handed-over document beats the remembered one", code, out)

            control = room / "control.json"
            write_session(control, other)
            code, out = run_direct(binary, f"opened:{other}", control, [], args.timeout)
            ok &= report("control: with nothing handed over, the remembered one opens", code, out)
            ok &= race_phase(binary, pdf, other, room, args.timeout)
        else:
            print("--- precedence ---")
            print("[SKIP] needs --other, a second document to remember")
            print("--- overlapping opens ---")
            print("[SKIP] needs --other, a second document to race against")

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
    if not HANDS_OVER_TO_RUNNING:
        return skip(
            RUNNING_HANDOVER,
            "this platform has no route for delivering a document to an app already "
            "running, so a second launch is a second process with its own window and its "
            "own worker pool, and the emit branch this phase tests is unreachable. macOS "
            "routes an Apple Event to `RunEvent::Opened`; Windows forwards argv through "
            "`tauri-plugin-single-instance`. A third platform needs one of the two before "
            "this phase can mean anything",
        )

    env = dict(os.environ, TPDF_OPENCHECK=f"arrives:{pdf}", TPDF_SESSION_FILE=str(room / "run.json"))
    # **A file, not a pipe, and this is the one phase where that matters.** tpdf's
    # render workers are re-execs of the same binary and **inherit the app's stdout**,
    # so a pipe reaches EOF only once every worker has also gone. `communicate()` waits
    # for EOF as well as for exit, so a worker that outlives the app by a moment turns
    # this phase into a timeout that prints *nothing at all* --- observed, and it reads
    # exactly like the app hanging. `docs/TRAPS.md` records the pipe-versus-process
    # distinction from the Rust side; this is the same hazard from the harness side.
    #
    # A file has no EOF to wait for: the process is waited on, then the file is read.
    log = room / "running.log"
    handle = log.open("w", encoding="utf-8", errors="replace")
    app = subprocess.Popen([str(binary)], env=env, stdout=handle, stderr=subprocess.STDOUT)
    try:
        # Long enough to be past the check's own quiet window, so the document
        # genuinely arrives at a running app rather than racing the boot.
        time.sleep(6)
        # The second launch, spelled the way each platform spells it. `open -a` asks
        # Launch Services to route to the running app; on Windows the plain
        # executable *is* the route --- the single-instance plugin makes the second
        # process forward its argv and exit, so what looks like a fresh launch is the
        # handover. Running the second process without waiting for it is deliberate:
        # on Windows it terminates on its own, and blocking on it would be waiting for
        # the process whose job is to disappear.
        if sys.platform == "darwin":
            subprocess.run(["open", "-a", str(bundle), pdf], capture_output=True, timeout=30)
        else:
            second = subprocess.Popen(
                [str(binary), pdf],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            try:
                second.wait(timeout=30)
            except subprocess.TimeoutExpired:
                second.kill()
        code = app.wait(timeout=timeout)
        handle.close()
        out = log.read_text(encoding="utf-8", errors="replace")
    except subprocess.TimeoutExpired:
        app.kill()
        handle.close()
        # The partial transcript, not just the word "timeout". A run that stopped
        # partway names the last thing it completed, and discarding that is the failure
        # `docs/TRAPS.md` records under a timeout throwing away its evidence --- it is
        # also what made this phase's own flake take three runs to understand, since
        # `no summary line` was all it ever said. Writing to a file rather than a pipe is
        # what makes the partial output available here at all.
        out = log.read_text(encoding="utf-8", errors="replace")
        out += f"[FAIL] run timed out after {timeout:.0f} s\n"
        code = 1
    return report(RUNNING_HANDOVER, code, out)


if __name__ == "__main__":
    sys.exit(main())
