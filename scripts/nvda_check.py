#!/usr/bin/env python3
"""Drives tpdf under NVDA on Windows and checks what NVDA says.

    scripts/nvda_check.py [BINARY] [--pdf testdata/outline-labels.pdf] [--keep DIR]

WHY THIS EXISTS. The live regions, field names and page names tpdf gives a
screen reader are checked by their roles and text in the webview, and that
cannot tell whether anything is spoken. On 2026-11-15 NVDA 2026.2 with the
installed 26.11.8 said nothing when *Extract pages* asked for its range,
nothing of the preview under the field, and "Page 8 of 8" for the page the
counter showed as `8 / 8 (App-B)`, with every gate green
(`docs/TRAPS.md`, *A live region built together with its text is not
announced, and the palette's preview was one*). This runs that walk and reads
NVDA's own speech log.

WHAT IT DOES. Copies the fixture to a fresh path (tpdf reopens a known path at
the page it was left on, and the checks start on page 1), starts NVDA with a
config of its own and the speech logged (`--log-level=12`, a `Speaking [...]`
line per utterance), opens the copy in BINARY, and presses keys with
`SendInput`, marking the time of each step. Then it attributes each utterance
to the step it fell in and checks:

  open          "Page 1 of 8, printed as i"
  palette       "Run a command"
  question      "Extract pages: Pages, e.g. 1-3,5"
  preview       "Extract 2 pages · printed as 1–2"            (typed `3-4`)
  preview more  "Extract 3 pages · printed as 1–2, App-A"     (typed `,7`)
  page turn     "Page 2 of 8, printed as ii"                  (Page Down in the field)
  cancel        nothing naming *Extract* after the palette is left

Nothing is saved, so the document is never written; its digest is compared
before and after anyway.

NVDA KEEPS SOME KEYS, which is why this presses what it presses: in browse mode
a letter is quick navigation and Page Down moves NVDA's own cursor, and in focus
mode the first Escape returns NVDA to browse mode and is not passed on. The
palette's field puts NVDA in focus mode, so every key here is pressed there, and
the cancel presses Escape until the field has lost the focus. Read *Driving NVDA
from a script* in `docs/TRAPS.md` before changing a step: three early results
that read as tpdf defects were the driver's.

WHAT IT NEEDS. Windows, NVDA installed in `C:\\Program Files\\NVDA` (`winget
install NVAccess.NVDA`), no NVDA and no tpdf already running, an unlocked desktop
session, and tpdf in English. Run in the desktop session: over SSH use
`scripts/run_on_windows.py --nvda`, which builds the checks binary on the
Windows build machine and runs this there. The expected sentences are tpdf's
English text and NVDA's English role names.

EXIT. 0 every check heard, 1 a check failed, 2 it could not run (a missing
NVDA, fixture or window, or a focus that left tpdf). The whole speech log is kept
when `--keep` names a folder, and is otherwise in a temporary one that is
removed.
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as w
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NVDA = Path(r"C:\Program Files\NVDA\nvda.exe")
DEFAULT_BINARY = ROOT / "src-tauri" / "target" / "release" / "tpdf.exe"
DEFAULT_PDF = ROOT / "testdata" / "outline-labels.pdf"

# No welcome dialog, no question about usage statistics, no update check: each
# of those is a window that takes the focus from tpdf. The silence synthesizer,
# because what is logged is what NVDA decided to say, not what a speaker played.
NVDA_INI = """schemaVersion = 11
[general]
\tshowWelcomeDialogAtStartup = False
\tlanguage = en
[update]
\tautoCheck = False
\taskedAllowUsageStats = True
\tallowUsageStats = False
[speech]
\tsynth = silence
"""


@dataclass
class Step:
    """A step: what was done at `at`, and what must or must not be heard after it."""

    name: str
    at: float
    expect: list[str] = field(default_factory=list)
    forbid: list[str] = field(default_factory=list)


# --- Keys --------------------------------------------------------------------------------

user32 = ctypes.WinDLL("user32", use_last_error=True) if sys.platform == "win32" else None
ULONG_PTR = ctypes.c_size_t


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", w.WORD), ("wScan", w.WORD), ("dwFlags", w.DWORD),
                ("time", w.DWORD), ("dwExtraInfo", ULONG_PTR)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", w.LONG), ("dy", w.LONG), ("mouseData", w.DWORD),
                ("dwFlags", w.DWORD), ("time", w.DWORD), ("dwExtraInfo", ULONG_PTR)]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", w.DWORD), ("u", _INPUTUNION)]


KEYUP, UNICODE, EXTENDED = 0x2, 0x4, 0x1
VK = {"ctrl": 0x11, "alt": 0x12, "enter": 0x0D, "esc": 0x1B, "pgdn": 0x22, "k": 0x4B, "f4": 0x73}
# Page Down is an extended key; sent without the flag it is the keypad's 3.
EXTENDED_KEYS = {0x22}


def send(vk: int = 0, scan: int = 0, flags: int = 0) -> None:
    item = INPUT(type=1)
    item.u.ki = KEYBDINPUT(vk, scan, flags | (EXTENDED if vk in EXTENDED_KEYS else 0), 0, 0)
    user32.SendInput(1, ctypes.byref(item), ctypes.sizeof(INPUT))
    time.sleep(0.03)


def press(*names: str) -> None:
    codes = [VK[name] for name in names]
    for code in codes:
        send(code)
    for code in reversed(codes):
        send(code, flags=KEYUP)


def type_text(text: str) -> None:
    for char in text:
        send(scan=ord(char), flags=UNICODE)
        send(scan=ord(char), flags=UNICODE | KEYUP)
        time.sleep(0.08)


def foreground_title() -> str:
    buffer = ctypes.create_unicode_buffer(256)
    user32.GetWindowTextW(user32.GetForegroundWindow(), buffer, 256)
    return buffer.value


FOCUS_PS = r"""Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$e = [System.Windows.Automation.AutomationElement]::FocusedElement
if ($e) { "$($e.Current.ControlType.ProgrammaticName)|$($e.Current.Name)" } else { "none|" }
"""


def focused() -> str:
    """The focused element as UI Automation sees it: `ControlType.Edit|Run a command`."""
    got = subprocess.run(["powershell", "-NoProfile", "-Command", FOCUS_PS],
                         capture_output=True, text=True, timeout=30, check=False)
    return got.stdout.strip()


class Lost(Exception):
    """The focus left tpdf, so the keys that follow would go elsewhere."""


def ensure_front() -> None:
    title = foreground_title()
    if not title.startswith("tpdf"):
        raise Lost(f"the foreground window is {title!r}, not tpdf")


# --- The speech log ------------------------------------------------------------------------

SPOKEN_HEAD = re.compile(r"^IO - speech\.speech\.speak \((\d\d):(\d\d):(\d\d)\.(\d+)\)")
# The commands in a speech sequence, `LangChangeCommand ('en')` and the like,
# whose arguments are not speech.
COMMAND = re.compile(r"\w+Command\s*\((?:[^()]|\([^()]*\))*\)")
QUOTED = re.compile(r"'((?:[^'\\]|\\.)*)'|\"((?:[^\"\\]|\\.)*)\"")


def utterances(log: str, day: datetime) -> list[tuple[float, str]]:
    """Each `Speaking [...]` of the log as its time and its words, in order."""
    lines = log.splitlines()
    said: list[tuple[float, str]] = []
    for index, line in enumerate(lines[:-1]):
        head = SPOKEN_HEAD.match(line)
        if not head or not lines[index + 1].startswith("Speaking ["):
            continue
        hour, minute, second, fraction = head.groups()
        stamp = day.replace(hour=int(hour), minute=int(minute), second=int(second),
                            microsecond=int(fraction.ljust(6, "0")[:6])).timestamp()
        body = COMMAND.sub("", lines[index + 1][len("Speaking ["):])
        words = [single or double for single, double in QUOTED.findall(body)]
        text = " ".join(word.replace("\\n", " ").strip() for word in words).strip()
        if text:
            said.append((stamp, text))
    return said


def judge(steps: list[Step], said: list[tuple[float, str]], end: float) -> list[str]:
    """The failures: an expected sentence not heard in its step, a forbidden one heard."""
    failures: list[str] = []
    for index, step in enumerate(steps):
        until = steps[index + 1].at if index + 1 < len(steps) else end
        heard = [text for at, text in said if step.at <= at < until]
        for wanted in step.expect:
            if any(wanted in text for text in heard):
                print(f"[OK]   {step.name}: heard {wanted!r}")
            else:
                failures.append(f"{step.name}: {wanted!r} not heard; heard {heard[-6:]!r}")
        for unwanted in step.forbid:
            spoken = [text for text in heard if unwanted in text]
            if spoken:
                failures.append(f"{step.name}: {unwanted!r} heard after it should be gone: {spoken!r}")
            else:
                print(f"[OK]   {step.name}: nothing naming {unwanted!r}")
    return failures


# --- The run -----------------------------------------------------------------------------

def running(image: str) -> bool:
    got = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/NH"],
                         capture_output=True, text=True, check=False)
    return image.lower() in got.stdout.lower()


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def walk(binary: Path, pdf: Path, scratch: Path) -> tuple[list[Step], float, Path]:
    """Runs NVDA and tpdf through the steps. Returns the steps, the end time and the log."""
    config = scratch / "nvda-config"
    config.mkdir()
    (config / "nvda.ini").write_text(NVDA_INI, encoding="utf-8")
    log = scratch / "nvda.log"
    # Through `start`, which is ShellExecute: the installed nvda.exe has
    # uiAccess, and CreateProcess refuses it with WinError 740.
    subprocess.run(["cmd", "/c", "start", "", str(NVDA), "-m", f"--config-path={config}",
                    f"--log-file={log}", "--log-level=12"], check=True)
    steps: list[Step] = []
    app: subprocess.Popen[bytes] | None = None
    try:
        time.sleep(10)
        if not running("nvda.exe"):
            raise Lost("NVDA did not start")
        steps.append(Step("open", time.time(), expect=["Page 1 of 8, printed as i"]))
        app = subprocess.Popen([str(binary), str(pdf)])
        time.sleep(12)
        ensure_front()

        steps.append(Step("palette", time.time(), expect=["Run a command"]))
        press("ctrl", "k")
        time.sleep(2)
        type_text("extract pages")
        time.sleep(2)
        ensure_front()
        steps.append(Step("question", time.time(), expect=["Extract pages: Pages, e.g. 1-3,5"]))
        press("enter")
        time.sleep(2.5)
        if "Edit|" not in focused():
            raise Lost(f"after Enter the focus is {focused()!r}, not the palette's field")

        steps.append(Step("preview", time.time(), expect=["Extract 2 pages · printed as 1–2"]))
        type_text("3-4")
        time.sleep(3)
        steps.append(Step("preview more", time.time(), expect=["Extract 3 pages · printed as 1–2, App-A"]))
        type_text(",7")
        time.sleep(3)
        ensure_front()
        steps.append(Step("page turn", time.time(), expect=["Page 2 of 8, printed as ii"]))
        press("pgdn")
        time.sleep(3)

        # Escape until the field has lost the focus: NVDA keeps the first, and
        # the palette leaves its question one level at a time.
        for _ in range(4):
            ensure_front()
            press("esc")
            time.sleep(1.5)
            if "Edit|" not in focused():
                break
        else:
            raise Lost("four presses of Escape did not leave the palette")
        steps.append(Step("cancel", time.time(), forbid=["Extract"]))
        time.sleep(4)
        end = time.time()

        ensure_front()
        press("alt", "f4")
        time.sleep(4)
        return steps, end, log
    finally:
        if app is not None and app.poll() is None:
            app.kill()
        subprocess.run(["cmd", "/c", "start", "", str(NVDA), "-q"], check=False)
        time.sleep(4)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("binary", nargs="?", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--pdf", type=Path, default=DEFAULT_PDF,
                        help="a document labelled i, ii, 1-4, App-A, App-B (the fixture)")
    parser.add_argument("--keep", type=Path, help="keep the speech log and the copy in this folder")
    args = parser.parse_args()

    if sys.platform != "win32":
        print("[FAIL] NVDA runs on Windows only")
        return 2
    for path, what in ((NVDA, "NVDA (winget install NVAccess.NVDA)"), (args.binary, "the tpdf binary"),
                       (args.pdf, "the fixture (scripts/ci_fixtures.py makes the testdata)")):
        if not path.is_file():
            print(f"[FAIL] {what} is not at {path}")
            return 2
    for image in ("nvda.exe", args.binary.name):
        if running(image):
            print(f"[FAIL] {image} is already running; quit it first, or its speech and "
                  "its window are not this run's")
            return 2

    scratch = Path(tempfile.mkdtemp(prefix="tpdf-nvda-"))
    try:
        # A path tpdf has never seen, so it opens on page 1.
        pdf = scratch / f"labels-{os.getpid()}-{int(time.time())}.pdf"
        shutil.copyfile(args.pdf, pdf)
        before = digest(pdf)
        try:
            steps, end, log = walk(args.binary.resolve(), pdf, scratch)
        except Lost as lost:
            print(f"[FAIL] the walk stopped: {lost}")
            return 2
        if not log.is_file():
            print("[FAIL] NVDA wrote no log, so nothing was heard")
            return 2
        said = utterances(log.read_text(encoding="utf-8", errors="replace"), datetime.now())
        if not said:
            print("[FAIL] the log holds no speech at level 12")
            return 2
        failures = judge(steps, said, end)
        if digest(pdf) != before:
            failures.append("the document changed on disk, and nothing in the walk saves")
        kept = f", the log kept in {args.keep / log.name}" if args.keep else ""
        print(f"[..]   {len(said)} utterances{kept}")
        for failure in failures:
            print(f"[FAIL] {failure}")
        checks = sum(len(step.expect) + len(step.forbid) for step in steps) + 1
        if failures:
            print(f"[FAIL] {len(failures)} of {checks} checks failed")
            return 1
        print(f"[OK] all {checks} checks heard as expected")
        return 0
    finally:
        if args.keep:
            args.keep.mkdir(parents=True, exist_ok=True)
            for item in scratch.iterdir():
                if item.is_file():
                    shutil.copy2(item, args.keep / item.name)
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
