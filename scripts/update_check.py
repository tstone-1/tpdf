#!/usr/bin/env python3
"""Installs the previous release on Windows and applies the published update to it.

Usage, on Windows, in a session with a desktop:
    python scripts/update_check.py <tpdf_<previous>_x64-setup.exe> <document.pdf>

WHY THIS EXISTS. `BUILD.md`, *Cutting a release*, step 12 is the only proof
that the updater works from end to end: `update.test.ts` fakes the plugin, so
no test fetches the real `latest.json`, checks a real signature or runs a real
installer over a running copy. The step was done by hand, which means it was
skipped: the records of 26.10.13 and 26.10.14 both say so. This is that step as
one command.

WHAT IT DOES, in order, and each line it prints is one check:
  1. reads `latest.json` as an installed copy does, for the version on offer;
  2. installs the previous release silently into a folder of its own;
  3. starts it on a copy of the document that lies under a folder with a space
     in its name, and waits for its window;
  4. finds the button that offers the update in that window, and presses it;
     when the copy's own check was answered `current`, it closes the copy and
     starts it once more (`update_log.py`, `SECOND_START_S`);
  5. waits for the application to end and for the installer to start it again:
     a window of the new version, from the same folder, with the document open
     again in a worker that maps the installed PDFium;
  6. reads the installed tool's version, and looks for a second offer in the
     new window, which must not be there;
  7. closes the window, uninstalls, and reads the registry back.

THE FOLDER WITH A SPACE is the point of step 3. The installer starts tpdf
again with the document's path, and that hand-over lost its quotes until
26.10.12: an update applied from an empty window looked right, and the same
update with such a document open ended in *could not open*.

HOW THE BUTTON IS PRESSED. Through UI Automation, which WebView2 answers: the
button is found by its name, `Update to <version>`, and invoked. A normal build
has no check harness in it, so this is the one way in that a reader's own
click also takes. It is asked through PowerShell, which ships the automation
client; nothing is installed for it.

THE SESSION. The application is started with `TPDF_SESSION_FILE` in a scratch
folder, and the copy the installer starts may not inherit it. So the real
session file is moved away before the start and put back at the end, and after
the update the new window's tab is checked by full path.

IT WRITES TO THE MACHINE what `installed_check.py` writes, and reads the same
three keys back. It refuses to start when tpdf is installed or running there.
It needs the network: the offer and the update come from the published release.

CONTROL. `--control no-press` finds the button and does not press it. The run
then ends with exit code 0 exactly when the checks of the restart failed and no
other did.

Exit codes: 0 every check passed, 1 a check failed, 2 the run could not start.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from installed_check import (APP_KEY, NAME, PDF_CLASS, UNINSTALL_KEY, WM_CLOSE, Checks, RanOut,
                             already_installed, bounded, descendants, installed_engine, processes,
                             registry, tool, uninstall, window_titles)
from live_output import stream_results
from win_modules import mapped_engines
from update_log import isolated_session, log_timestamp, relaunched_answer, SECOND_START_S, UpdateAttempt, explain, has_open_tab, last_launch, offered_button, served_old

LATEST = "https://github.com/tstone-1/tpdf/releases/latest/download/latest.json"

# The checks that fail when the button is not pressed, and no others.
NO_PRESS = [
    "the button is pressed",
    "the application that was running ends",
    "the installer starts the application again",
    "a window titled",
    "the document is open again",
    "a worker of the new copy has the installed PDFium mapped",
    "the installed tool is version",
]

# Lists the names of every button under one window, or invokes the first whose
# name equals the given text. One line per button; `PRESSED` after a press.
AUTOMATION = r"""
param([long]$Window, [string]$Press = "")
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$A = [System.Windows.Automation.AutomationElement]
$root = $A::FromHandle([IntPtr]$Window)
$tabs = New-Object System.Windows.Automation.PropertyCondition(
    $A::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
foreach ($tab in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $tabs)) {
    "TAB $($tab.Current.HelpText)"
}
$buttons = New-Object System.Windows.Automation.PropertyCondition(
    $A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
foreach ($button in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttons)) {
    $name = $button.Current.Name
    "BUTTON $name"
    if ($Press -and $name -ceq $Press) {
        $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        "PRESSED $name"
        break
    }
}
"""


def buttons(script: Path, window: int, press: str = "") -> list[str]:
    """What the automation script printed for one window."""
    command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(script),
               "-Window", str(window)]
    if press:
        command += ["-Press", press]
    try:
        ran = subprocess.run(command, capture_output=True, text=True, encoding="utf-8",
                             errors="replace", timeout=60, check=False)
    except subprocess.TimeoutExpired as e:
        raise RanOut("the window's buttons could not be read within 60 s") from e
    return [line.strip() for line in ran.stdout.splitlines() if line.strip()]


def copies(folder: Path) -> list[int]:
    """The running applications whose image is the `tpdf.exe` in `folder`.

    A worker is the same image started by the application, so a process whose
    parent is one of these is left out: until that was done, the workers of the
    copy that was never updated counted as the application started again, and
    the control that presses nothing passed two of its checks.
    """
    from win_modules import modules_of

    image = str(folder / "tpdf.exe").lower()
    found: dict[int, int] = {}
    for pid, parent, name in processes():
        if name.lower() != "tpdf.exe":
            continue
        if any(m.lower().removeprefix("\\\\?\\") == image for m in modules_of(pid)):
            found[pid] = parent
    return [pid for pid, parent in found.items() if parent not in found]


def offered() -> str:
    """The version `latest.json` offers a Windows copy."""
    with urllib.request.urlopen(LATEST, timeout=30) as answer:
        manifest = json.load(answer)
    if "windows-x86_64" not in manifest.get("platforms", {}):
        raise RuntimeError("latest.json offers nothing for windows-x86_64")
    return str(manifest["version"])


def close(pids: list[int]) -> list[int]:
    """Asks each copy's window to close; the copies that had to be killed after 20 s.

    A copy that is still there after the request was asked something, or
    hung, and is killed so the run can go on. The answer says which, because
    a caller that checks only that the copy is gone afterwards cannot tell a
    copy that closed from one that was killed with a question on screen.
    """
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    for pid in pids:
        for hwnd, title in window_titles(pid):
            if title.startswith("tpdf v"):
                user32.PostMessageW(ctypes.c_void_p(hwnd), WM_CLOSE, 0, 0)
    gone_by = time.monotonic() + 20
    while time.monotonic() < gone_by and {p for p, _, _ in processes()} & set(pids):
        time.sleep(0.2)
    killed = sorted({p for p, _, _ in processes()} & set(pids))
    for pid in killed:
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True, check=False)
    return killed


def start(folder: Path, pdf: Path, env: dict[str, str], scratch: Path, old: str,
          wait: float) -> tuple[subprocess.Popen[bytes], int]:
    """Starts the installed old copy on the document; its window, or 0 when none came."""
    app = subprocess.Popen([str(folder / "tpdf.exe"), str(pdf)], env=env, cwd=str(scratch))
    window = 0
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline and app.poll() is None and not window:
        shown = [w for w in window_titles(app.pid) if w[1].startswith(f"tpdf v{old}")]
        window = shown[0][0] if shown else 0
        time.sleep(0.2)
    return app, window


def offers(script: Path, window: int, version: str, wait: float) -> list[str]:
    """The window's buttons, read until one is the offer or `wait` runs out.

    The offer comes after a request to the network, and the automation tree
    after the first question put to it, so both are asked for again.
    """
    seen: list[str] = []
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline and not offered_button(seen, version):
        seen = buttons(script, window)
        time.sleep(1)
    return seen


def update(checks: Checks, folder: Path, pdf: Path, scratch: Path, old: str, new: str,
           wait: float, press: bool, attempt: UpdateAttempt) -> None:
    """`attempt` receives the first launch's answer and the outcome of a retry."""
    from win_modules import modules_of

    script = scratch / "buttons.ps1"
    script.write_text(AUTOMATION, encoding="utf-8")
    session = scratch / "session.json"
    log = scratch / "tpdf.log"
    env = dict(os.environ, TPDF_SESSION_FILE=str(session), TPDF_LOG_FILE=str(log))
    print(f"--- {old} opens the document and offers the update ---")
    app, window = start(folder, pdf, env, scratch, old, wait)
    started: list[int] = [app.pid]
    try:
        checks.note(bool(window), f"a window titled 'tpdf v{old}' is shown")
        if not window:
            return
        seen = offers(script, window, new, wait)
        offer = offered_button(seen, new)
        attempt.answer = last_launch(log)
        if not offer and served_old(log):
            attempt.second_offer = False
            print(f"[INFO] {old} was told it is current ({explain(log)}), while latest.json "
                  f"offers {new}; it asks once per launch, so it is started again in "
                  f"{SECOND_START_S} s, once")
            close([app.pid])
            time.sleep(SECOND_START_S)
            app, window = start(folder, pdf, env, scratch, old, wait)
            started.append(app.pid)
            seen = offers(script, window, new, wait) if window else []
            offer = offered_button(seen, new)
            attempt.second_offer = bool(offer)
        checks.note(bool(offer), f"the window offers version {new}",
                    f"{len(seen)} buttons read" if seen else "no button could be read")
        if not offer:
            print(f"[INFO] the application's own update checks: {explain(scratch / 'tpdf.log')}")
            return
        restart_after = log_timestamp()
        if not press:
            checks.note(False, "the button is pressed", "not pressed")
            time.sleep(15)
        else:
            pressed = buttons(script, window, press=offer)
            checks.note(f"PRESSED {offer}" in pressed, "the button is pressed")

        print("--- the update is installed and the application comes back ---")
        try:
            app.wait(timeout=wait * 3 if press else 5)
            ended = True
        except subprocess.TimeoutExpired:
            ended = False
        checks.note(ended, "the application that was running ends")

        title = f"tpdf v{new}"
        engine = str(folder / "pdfium" / "pdfium.dll")
        again: list[int] = []
        windows: list[tuple[int, int]] = []
        workers: dict[int, set[str]] = {}
        named = False
        deadline = time.monotonic() + (wait * 3 if ended else 5)
        while time.monotonic() < deadline:
            again = [pid for pid in copies(folder) if pid != app.pid]
            started.extend(pid for pid in again if pid not in started)
            windows = [(hwnd, pid) for pid in again for hwnd, caption in window_titles(pid)
                       if caption.startswith(title)]
            for root in again:
                for pid in descendants(root):
                    if paths := mapped_engines(modules_of(pid)):
                        workers.setdefault(pid, set()).update(paths)
            named = any(has_open_tab(buttons(script, hwnd), pdf) for hwnd, _ in windows)
            if windows and installed_engine(workers, engine) and named:
                break
            time.sleep(0.3)
        checks.note(bool(again), "the installer starts the application again",
                    f"from {folder}" if again else "no copy from the install folder is running")
        checks.note(bool(windows), f"a window titled '{title}' is shown",
                    "" if windows else f"saw {[t for p in again for _, t in window_titles(p)]}")
        checks.note(bool(named), "the document is open again, from under a folder with a space",
                    str(pdf))
        checks.note(installed_engine(workers, engine),
                    "a worker of the new copy has the installed PDFium mapped",
                    ", ".join(sorted(set().union(*workers.values()))))
        said = tool(folder / "tpdf-cli.exe", "--version").stdout.strip()
        checks.note(said.endswith(new), f"the installed tool is version {new}", said)

        if windows:
            default_log = Path(os.environ["LOCALAPPDATA"]) / "com.timostein.tpdf/logs/tpdf.log"
            completed = relaunched_answer((log, default_log), restart_after, windows[0][1], wait)
            now = []
            asked_until = time.monotonic() + 30
            # The first question to a new window's automation tree is often
            # answered with nothing; the tree is built after it.
            while not now and time.monotonic() < asked_until:
                now = buttons(script, windows[0][0])
                time.sleep(1)
            checks.note(completed == "current" and any(line.startswith("BUTTON ") for line in now) and not any(
                            line.startswith(("BUTTON Update to ", "BUTTON Update auf ")) for line in now),
                        "the new copy offers no update",
                        f"launch check: {completed or 'no answer'}; {len(now)} buttons read")
    finally:
        running = [pid for pid in copies(folder)]
        close(sorted(set(running) | ({app.pid} if app.poll() is None else set())))


def main() -> int:
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("installer", help="the installer of the release before the published one")
    parser.add_argument("pdf")
    parser.add_argument("--wait", type=float, default=60, help="seconds to wait for a window")
    parser.add_argument("--control", choices=["no-press"],
                        help="break one thing on purpose; exit 0 when exactly its checks fail")
    args = parser.parse_args()

    if sys.platform != "win32":
        print("[ERROR] this check runs the Windows installer; run it on Windows")
        return 2
    installer = Path(args.installer).resolve()
    pdf = Path(args.pdf).resolve()
    for path in (installer, pdf):
        if not path.is_file():
            print(f"[ERROR] no {path}")
            return 2
    named = NAME.match(installer.name)
    if not named:
        print(f"[ERROR] {installer.name} does not name its version")
        return 2
    old = named.group(1)
    try:
        new = offered()
        running = any(image.lower() == "tpdf.exe" for _, _, image in processes())
    except (RuntimeError, OSError, ValueError, KeyError) as e:
        print(f"[ERROR] {e}")
        return 2
    if new == old:
        print(f"[ERROR] latest.json offers {new}, which is the installer given; give the one before")
        return 2
    listing = registry(UNINSTALL_KEY)
    if listing is not None:
        print(already_installed(listing))
        return 2
    if running:
        print("[ERROR] tpdf is running; a second start would be handed to it")
        return 2
    folder = Path(os.environ["LOCALAPPDATA"]) / "tpdf-update-check"
    if folder.exists() or " " in str(folder):
        print(f"[ERROR] {folder} exists or has a space in it; remove it and run again")
        return 2

    control = args.control
    if control:
        print("--- control: the button is found and not pressed ---")
    checks = Checks()
    attempt = UpdateAttempt()
    before = {key: registry(key) for key in (PDF_CLASS, APP_KEY)}
    real_session = Path(os.environ["APPDATA"]) / "com.timostein.tpdf" / "session.json"
    with tempfile.TemporaryDirectory(prefix="tpdf-update-check-") as temporary:
        scratch = Path(temporary)
        with isolated_session(real_session):
            try:
                spaced = scratch / "a folder with a space"
                spaced.mkdir()
                document = spaced / pdf.name
                shutil.copyfile(pdf, document)
                print(f"--- latest.json offers {new}; installing {old} into {folder} ---")
                code = bounded(f'"{installer}" /S /D={folder}', "the installer", 300)
                checks.note(code == 0, "the installer ends with exit code 0", f"exit {code}")
                said = tool(folder / "tpdf-cli.exe", "--version").stdout.strip()
                checks.note(said.endswith(old), f"the installed tool is version {old}", said)
                update(checks, folder, document, scratch, old, new, args.wait,
                       press=control != "no-press", attempt=attempt)
            except (RanOut, RuntimeError, OSError) as e:
                checks.note(False, "every step ends, and within its time limit", str(e))
            finally:
                uninstall(checks, folder, before, run_it=True)

    print()
    first = attempt.summary()
    if control:
        unexpected = [n for n in checks.failed if not any(n.startswith(e) for e in NO_PRESS)]
        missing = [e for e in NO_PRESS if not any(n.startswith(e) for n in checks.failed)]
        if unexpected or missing:
            print(f"[FAIL] control '{control}': did not fail but should have: {missing or 'nothing'}; "
                  f"failed but should not have: {unexpected or 'nothing'}")
            return 1
        print(f"[OK] control '{control}': what it is aimed at failed ({len(NO_PRESS)}), and nothing else")
        return 0
    if checks.failed:
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}; {first}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed: {old} updated itself to {new}; {first}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
