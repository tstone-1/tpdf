"""Guards shared by every script that drives a frame loop inside the webview.

WebKit suspends a page whose window is not visible, and behind a lock screen or
a dark display every window qualifies. The suspension stops
`requestAnimationFrame` *and* `setTimeout`, so a run in that state does not go
slowly -- it does not go at all, and it cannot report that, because whatever
would report it is suspended alongside the thing it was watching.

Both callers need exactly this and the message is long, so it lives once. A
second copy would drift, and the copy that drifted would be the one printed at
the moment someone is trying to work out why nothing happened.

`require_visible_session` answers the question *before* a run, and that leaves the
harder half: the screen can lock, or a window can be covered, **during** one. Then
a harness reports a timeout, which is the same thing it reports for a page stuck in
a loop, and the two want opposite responses. `diagnose_silence` tells them apart
from outside the process --- see its docstring for why the measurement has to be a
delta.
"""

import os
import subprocess
import sys
import time

LOCKED_MESSAGE = """[FAIL] the screen is locked, so every window is occluded and WebKit
       suspends the page. requestAnimationFrame never fires and the
       run cannot even time itself out. Unlock and re-run.

       There is no way to unlock a macOS session from a script, by
       design, so this can only be prevented rather than recovered
       from. Holding `caffeinate -du` covers one run and not the gap
       before it -- a long headless bench alongside it holds nothing.
       Wrap a whole batch instead:

           caffeinate -du bash -c '<run> ; <run> ; <run>'"""


def hold_display_awake() -> "subprocess.Popen[bytes] | None":
    """Keeps the display awake and on for as long as this process lives.

    `-u` as well as `-d`: `-d` only prevents the display going idle, and a
    display that is already off stays off. `-u` declares user activity, which
    turns it back on. The assertion is released when this process exits.
    """
    if sys.platform != "darwin":
        return None
    try:
        return subprocess.Popen(
            ["caffeinate", "-du", "-w", str(os.getpid())],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        print("[WARN] caffeinate not available; a sleeping display will hang the run", file=sys.stderr)
        return None


def screen_is_locked() -> bool:
    """Whether the login session is locked, which makes every window invisible."""
    if sys.platform != "darwin":
        return False
    result = subprocess.run(["ioreg", "-n", "Root", "-d1", "-a"], capture_output=True, text=True)
    marker = "<key>CGSSessionScreenIsLocked</key>"
    index = result.stdout.find(marker)
    return index >= 0 and "<true/>" in result.stdout[index : index + 120]


#: Fraction of a core above which a live process counts as spinning rather than
#: waiting. A tenth: an idling webview measured 4% between checks, and a page in a
#: loop uses a whole core.
BUSY_FRACTION = 0.1


def _cpu_seconds(pid: int) -> float | None:
    """CPU time the process has used so far, in seconds, or `None` if it is gone."""
    if sys.platform == "win32":
        return None
    result = subprocess.run(
        ["ps", "-o", "cputime=", "-p", str(pid)], capture_output=True, text=True
    )
    text = result.stdout.strip()
    if not text:
        return None
    # `[[dd-]hh:]mm:ss[.ss]`, most-significant part first, so parse from the end.
    parts = text.replace("-", ":").split(":")
    try:
        numbers = [float(part) for part in parts]
    except ValueError:
        return None
    seconds = 0.0
    for factor, number in zip((1, 60, 3600, 86400), reversed(numbers)):
        seconds += factor * number
    return seconds


def diagnose_silence(pid: int, seconds: float = 2.0) -> str:
    """Why a run stopped producing output: suspended, or busy and stuck.

    The two look identical from outside --- no output, process alive --- and they
    want opposite responses. A **suspended** page is environmental: the window is
    occluded by a full-screen window, sits on another Space, or the screen locked
    after `require_visible_session` had already said yes. A **busy** one is a defect
    in whatever was last changed. Reporting "timed out" for both is what sends an
    hour into instrumenting code that was never running.

    A third band sits between them and is the reason this returns prose rather
    than a boolean: a page can be alive, using almost no CPU, and waiting on
    something that will never arrive. That is not suspended and not spinning, and
    the honest thing is to say so and point at the last check printed.

    The distinguishing observable is CPU time, and it has to be a **delta**: a
    single `ps -o %cpu` is an average over the process's whole lifetime on macOS,
    so a page that worked hard and then got suspended reads as busy. Two samples
    a couple of seconds apart cannot be fooled that way --- a suspended process's
    CPU time does not advance at all.

    Costs `seconds` on a run that has already failed, which is the cheapest
    diagnostic in this repository and the only one that answers the question
    everyone actually has.
    """
    before = _cpu_seconds(pid)
    if before is None:
        return "the process was already gone, so it exited rather than hanging"
    time.sleep(seconds)
    after = _cpu_seconds(pid)
    if after is None:
        return "the process exited while being sampled"
    used = after - before
    if used < 0.01:
        locked = screen_is_locked()
        return (
            f"the process used {used:.2f}s of CPU in {seconds:.0f}s, so it is "
            "**suspended, not stuck** -- WebKit has parked the page because its "
            "window is not visible. "
            + (
                "The screen is locked now, though it was not when the run started."
                if locked
                else "Check for a full-screen window over it or another Space; "
                "TPDF_RAISE=1 raises the window."
            )
        )
    # Three bands, not two, because "alive" is not one state. A webview running
    # normally but slowly burns a few percent of a core between checks --- an
    # artificially short timeout on a healthy run measured 0.08s in 2s --- and
    # calling that "stuck" is an over-claim that names a defect where there may be
    # none. What the measurement supports is only the first split: suspended or
    # not. Beyond that it can say which of the two live shapes it is, and should
    # not pretend the distinction is a verdict.
    if used < seconds * BUSY_FRACTION:
        return (
            f"the process used {used:.2f}s of CPU in {seconds:.0f}s, so it is "
            "**alive and waiting** -- not suspended, so this is not an occluded "
            "window. It is either waiting on a reply that has not come or on a "
            "condition that cannot hold; the last check printed says which was "
            "in progress"
        )
    return (
        f"the process used {used:.2f}s of CPU in {seconds:.0f}s, so it is "
        "**alive and spinning** -- the page is executing without finishing, which "
        "is a defect rather than an occluded window"
    )


def require_visible_session() -> bool:
    """Refuses to continue behind a lock screen, and holds the display awake."""
    if screen_is_locked():
        print(LOCKED_MESSAGE, file=sys.stderr)
        return False
    hold_display_awake()
    return True
