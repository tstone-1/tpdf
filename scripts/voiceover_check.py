#!/usr/bin/env python3
"""Drives tpdf under VoiceOver on macOS and checks what VoiceOver says.

    scripts/voiceover_check.py [APP] [--pdf testdata/outline-labels.pdf] [--keep DIR]

WHY THIS EXISTS. `nvda_check.py`'s reason, on the other platform: the names and
live regions tpdf gives a screen reader are checked in the webview by role and
text, which cannot tell whether anything is spoken, and WebKit and VoiceOver
are not WebView2 and NVDA. Run against the 26.11.8 build, VoiceOver said
nothing after *Extract pages* was chosen and nothing of the preview, exactly
where NVDA was silent (measured 2026-11-15).

WHAT IT DOES. The walk of `nvda_check.py`, with its expected sentences:

  open          "Page 1 of 8, printed as i"
  palette       "Run a command"
  question      "Extract pages: Pages, e.g. 1-3,5"
  preview       "Extract 2 pages · printed as 1–2"            (typed `3-4`)
  preview more  "Extract 3 pages · printed as 1–2, App-A"     (typed `,7`)
  page turn     "Page 2 of 8, printed as ii"                  (Page Down in the field)
  cancel        nothing naming *Extract* after the palette is left
  printed typed "Printed as ii: type 2"                       (typed `ii`)
  extracted     "Wrote 2 pages to extracted.pdf."             (`3-4`, saved)

VoiceOver keeps no log, so its AppleScript `content of last phrase` is read
five times a second, and each change is an utterance at the time it was read.
A phrase replaced within a fifth of a second can be missed; what is checked
here is said as a sentence, and is not. The keys are System Events' and the
save goes to the scratch folder through the sheet's *Go to Folder* (Cmd-Shift-G),
which takes a full path. The document's digest is compared before and after.

WHAT IT NEEDS. macOS, an unlocked session with tpdf free to come to the front,
and *Allow VoiceOver to be controlled with AppleScript* (VoiceOver Utility,
General), which takes an administrator's password once and leaves
`/private/var/db/Accessibility/.VoiceOverAppleScriptEnabled`. The process
running this needs Accessibility, for the keys. VoiceOver is started if it is
not running, and quit at the end if this started it. Over SSH, run it with
`scripts/run_on_mac.py --voiceover`, which builds the checks app on the other
Mac and runs this there. The sentences are tpdf's English text.

EXIT. 0 every check heard, 1 a check failed, 2 it could not run (no AppleScript
control, no app or fixture, or a focus that left tpdf).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from nvda_check import Step, judge

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_APP = ROOT / "src-tauri/target/release/bundle/macos/tpdf Checks.app"
DEFAULT_PDF = ROOT / "testdata" / "outline-labels.pdf"
ENABLED = Path("/private/var/db/Accessibility/.VoiceOverAppleScriptEnabled")
VOICEOVER = "/System/Library/CoreServices/VoiceOver.app"
EXTRACTED = "extracted.pdf"
# System Events key codes.
ENTER, ESCAPE, PAGE_DOWN, BACKSPACE = 36, 53, 121, 51


class Lost(Exception):
    """The run cannot go on: the keys that follow would go elsewhere."""


def osa(*lines: str, timeout: float = 15) -> str:
    """Runs AppleScript lines and returns what they answer, or raises Lost."""
    command = ["osascript"]
    for line in lines:
        command += ["-e", line]
    try:
        got = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        raise Lost(f"osascript did not answer: {lines[-1]}") from error
    if got.returncode != 0:
        raise Lost(f"osascript: {got.stderr.strip()}")
    return got.stdout.strip()


def phrase() -> str:
    return osa("with timeout of 5 seconds", 'tell application "VoiceOver" to get content of last phrase',
               "end timeout", timeout=10)


def keystroke(text: str, using: str = "") -> None:
    osa(f'tell application "System Events" to keystroke {quoted(text)}{using}')
    time.sleep(0.3)


def key(code: int) -> None:
    osa(f'tell application "System Events" to key code {code}')
    time.sleep(0.3)


def quoted(text: str) -> str:
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def front() -> str:
    return osa('tell application "System Events" to get name of first process whose frontmost is true')


def focused_role() -> str:
    return osa('tell application "System Events" to tell (first process whose frontmost is true) '
               'to get role of (value of attribute "AXFocusedUIElement")')


def ensure_front() -> None:
    name = front()
    if name != "tpdf":
        raise Lost(f"the frontmost process is {name!r}, not tpdf")


class Listener(threading.Thread):
    """Reads VoiceOver's last phrase five times a second and keeps each change."""

    def __init__(self) -> None:
        super().__init__(daemon=True)
        self.said: list[tuple[float, str]] = []
        self.stopping = threading.Event()

    def run(self) -> None:
        last = ""
        while not self.stopping.is_set():
            try:
                now = phrase()
            except Lost:
                now = last
            if now and now != last:
                self.said.append((time.time(), now))
                last = now
            time.sleep(0.2)


def running(name: str) -> bool:
    return subprocess.run(["pgrep", "-x", name], capture_output=True, check=False).returncode == 0


def walk(app: Path, pdf: Path, scratch: Path) -> tuple[list[Step], float]:
    steps: list[Step] = []
    steps.append(Step("open", time.time(), expect=["Page 1 of 8, printed as i"]))
    subprocess.run(["open", "-a", str(app), str(pdf)], check=True)
    time.sleep(10)
    # Brought forward once: VoiceOver, started a moment ago, can leave the app
    # that was active before it in front.
    if front() != "tpdf":
        osa('tell application "System Events" to set frontmost of process "tpdf" to true')
        time.sleep(3)
    ensure_front()

    steps.append(Step("palette", time.time(), expect=["Run a command"]))
    keystroke("k", " using command down")
    time.sleep(1.5)
    keystroke("extract pages")
    time.sleep(1.5)
    ensure_front()
    steps.append(Step("question", time.time(), expect=["Extract pages: Pages, e.g. 1-3,5"]))
    key(ENTER)
    time.sleep(2.5)
    if focused_role() != "AXTextField":
        raise Lost(f"after Enter the focus is a {focused_role()}, not the palette's field")

    steps.append(Step("preview", time.time(), expect=["Extract 2 pages · printed as 1–2"]))
    keystroke("3-4")
    time.sleep(3)
    steps.append(Step("preview more", time.time(), expect=["Extract 3 pages · printed as 1–2, App-A"]))
    keystroke(",7")
    time.sleep(3)
    ensure_front()
    steps.append(Step("page turn", time.time(), expect=["Page 2 of 8, printed as ii"]))
    key(PAGE_DOWN)
    time.sleep(3)

    # Escape until the field has lost the focus: the palette leaves its
    # question one level at a time.
    for _ in range(4):
        ensure_front()
        key(ESCAPE)
        time.sleep(1.5)
        if focused_role() != "AXTextField":
            break
    else:
        raise Lost("four presses of Escape did not leave the palette")
    steps.append(Step("cancel", time.time(), forbid=["Extract"]))
    time.sleep(4)

    # The palette again, so its speech is this step's and not the cancel's.
    steps.append(Step("reopen", time.time()))
    ensure_front()
    keystroke("k", " using command down")
    time.sleep(1.5)
    keystroke("extract pages")
    time.sleep(1.5)
    key(ENTER)
    time.sleep(2.5)
    if focused_role() != "AXTextField":
        raise Lost(f"after Enter the focus is a {focused_role()}, not the palette's field")
    steps.append(Step("printed typed", time.time(), expect=["Printed as ii: type 2"]))
    keystroke("ii")
    time.sleep(3)
    key(BACKSPACE)
    key(BACKSPACE)
    keystroke("3-4")
    time.sleep(2)
    steps.append(Step("save dialog", time.time()))
    key(ENTER)
    # Waited for: Cmd-Shift-G pressed before the sheet is there is tpdf's own
    # Find previous, and the walk would go on with nothing saved.
    for _ in range(20):
        time.sleep(0.5)
        if osa('tell application "System Events" to tell process "tpdf" '
               'to get exists sheet 1 of window 1') == "true":
            break
    else:
        raise Lost("Enter on 3-4 opened no save sheet")
    time.sleep(1)
    # The sheet's Go to Folder takes a full path, file name included.
    keystroke("g", " using {command down, shift down}")
    time.sleep(2)
    keystroke(str(scratch / EXTRACTED))
    time.sleep(1)
    key(ENTER)
    time.sleep(2)
    steps.append(Step("extracted", time.time(), expect=[f"Wrote 2 pages to {EXTRACTED}."]))
    key(ENTER)
    time.sleep(6)
    ensure_front()
    end = time.time()
    keystroke("q", " using command down")
    time.sleep(3)
    return steps, end


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("app", nargs="?", type=Path, default=DEFAULT_APP,
                        help="the .app, or the executable inside it")
    parser.add_argument("--pdf", type=Path, default=DEFAULT_PDF,
                        help="a document labelled i, ii, 1-4, App-A, App-B (the fixture)")
    parser.add_argument("--keep", type=Path, help="keep what was said in this folder")
    args = parser.parse_args()

    if sys.platform != "darwin":
        print("[FAIL] VoiceOver runs on macOS only")
        return 2
    app = args.app.resolve()
    while app.suffix != ".app" and app != app.parent:
        app = app.parent
    for path, what in ((app, "the tpdf app"), (args.pdf, "the fixture")):
        if not path.exists():
            print(f"[FAIL] {what} is not at {path}")
            return 2
    if not ENABLED.exists():
        print("[FAIL] VoiceOver cannot be read from a script: tick *Allow VoiceOver to be "
              "controlled with AppleScript* in VoiceOver Utility, General")
        return 2
    if running("tpdf"):
        print("[FAIL] tpdf is already running; quit it first, or its window is not this run's")
        return 2

    started = not running("VoiceOver")
    if started:
        subprocess.run(["open", "-a", VOICEOVER], check=False)
        time.sleep(8)
    scratch = Path(tempfile.mkdtemp(prefix="tpdf-voiceover-"))
    listener = Listener()
    try:
        # A path tpdf has never seen, so it opens on page 1.
        pdf = scratch / f"labels-{os.getpid()}-{int(time.time())}.pdf"
        shutil.copyfile(args.pdf, pdf)
        before = hashlib.sha256(pdf.read_bytes()).hexdigest()
        try:
            phrase()
        except Lost as lost:
            print(f"[FAIL] VoiceOver does not answer: {lost}")
            return 2
        listener.start()
        try:
            steps, end = walk(app, pdf, scratch)
        except Lost as lost:
            print(f"[FAIL] the walk stopped: {lost}")
            return 2
        finally:
            listener.stopping.set()
            listener.join(timeout=10)
            subprocess.run(["pkill", "-x", "tpdf"], check=False)
        said = listener.said
        if args.keep:
            args.keep.mkdir(parents=True, exist_ok=True)
            (args.keep / "voiceover.txt").write_text(
                "".join(f"{at:.2f}\t{text}\n" for at, text in said), encoding="utf-8")
        if not said:
            print("[FAIL] VoiceOver said nothing that could be read")
            return 2
        failures = judge(steps, said, end)
        if hashlib.sha256(pdf.read_bytes()).hexdigest() != before:
            failures.append("the document changed on disk, and the walk only extracts from it")
        if not (scratch / EXTRACTED).is_file():
            failures.append(f"extracted: no {EXTRACTED} was written")
        print(f"[..]   {len(said)} phrases" + (f", kept in {args.keep / 'voiceover.txt'}" if args.keep else ""))
        for failure in failures:
            print(f"[FAIL] {failure}")
        checks = sum(len(step.expect) + len(step.forbid) for step in steps) + 2
        if failures:
            print(f"[FAIL] {len(failures)} of {checks} checks failed")
            return 1
        print(f"[OK] all {checks} checks heard as expected")
        return 0
    finally:
        if started:
            subprocess.run(["osascript", "-e", 'tell application "VoiceOver" to quit'],
                           capture_output=True, timeout=15, check=False)
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
