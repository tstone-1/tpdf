"""Guards shared by every script that drives a frame loop inside the webview.

WebKit suspends a page whose window is not visible, and behind a lock screen or
a dark display every window qualifies. The suspension stops
`requestAnimationFrame` *and* `setTimeout`, so a run in that state does not go
slowly -- it does not go at all, and it cannot report that, because whatever
would report it is suspended alongside the thing it was watching.

Both callers need exactly this and the message is long, so it lives once. A
second copy would drift, and the copy that drifted would be the one printed at
the moment someone is trying to work out why nothing happened.
"""

import os
import subprocess
import sys

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


def require_visible_session() -> bool:
    """Refuses to continue behind a lock screen, and holds the display awake."""
    if screen_is_locked():
        print(LOCKED_MESSAGE, file=sys.stderr)
        return False
    hold_display_awake()
    return True
