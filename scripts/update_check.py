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
the update the document is looked for in whichever of the two was written.

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
                             bounded, descendants, installed_engine, processes, registry, tool,
                             uninstall, window_titles)
from live_output import stream_results

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
# name starts with the given text. One line per button; `PRESSED` after a press.
AUTOMATION = r"""
param([long]$Window, [string]$Press = "")
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$root = $A::FromHandle([IntPtr]$Window)
$buttons = New-Object System.Windows.Automation.PropertyCondition(
    $A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
foreach ($button in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttons)) {
    $name = $button.Current.Name
    "BUTTON $name"
    if ($Press -and $name.StartsWith($Press)) {
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


def close(pids: list[int]) -> None:
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    for pid in pids:
        for hwnd, title in window_titles(pid):
            if title.startswith("tpdf v"):
                user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
    gone_by = time.monotonic() + 20
    while time.monotonic() < gone_by and {p for p, _, _ in processes()} & set(pids):
        time.sleep(0.2)
    for pid in {p for p, _, _ in processes()} & set(pids):
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True, check=False)


def update(checks: Checks, folder: Path, pdf: Path, scratch: Path, old: str, new: str,
           real_session: Path, wait: float, press: bool) -> None:
    from win_modules import modules_of

    script = scratch / "buttons.ps1"
    script.write_text(AUTOMATION, encoding="utf-8")
    session = scratch / "session.json"
    env = dict(os.environ, TPDF_SESSION_FILE=str(session), TPDF_LOG_FILE=str(scratch / "tpdf.log"))
    print(f"--- {old} opens the document and offers the update ---")
    app = subprocess.Popen([str(folder / "tpdf.exe"), str(pdf)], env=env, cwd=str(scratch))
    started: list[int] = [app.pid]
    try:
        window = 0
        deadline = time.monotonic() + wait
        while time.monotonic() < deadline and app.poll() is None and not window:
            shown = [w for w in window_titles(app.pid) if w[1].startswith(f"tpdf v{old}")]
            window = shown[0][0] if shown else 0
            time.sleep(0.2)
        checks.note(bool(window), f"a window titled 'tpdf v{old}' is shown")
        if not window:
            return
        # The offer comes after a request to the network, and the automation
        # tree after the first question put to it, so both are asked for again.
        offer = f"Update to {new}"
        seen: list[str] = []
        while time.monotonic() < deadline and f"BUTTON {offer}" not in seen:
            seen = buttons(script, window)
            time.sleep(1)
        checks.note(f"BUTTON {offer}" in seen, f"the window offers '{offer}'",
                    f"{len(seen)} buttons read" if seen else "no button could be read")
        if f"BUTTON {offer}" not in seen:
            return
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
        engine = str(folder / "pdfium").lower()
        again: list[int] = []
        windows: list[tuple[int, str]] = []
        workers: dict[int, str] = {}
        named = ""
        deadline = time.monotonic() + (wait * 3 if ended else 5)
        while time.monotonic() < deadline:
            again = [pid for pid in copies(folder) if pid != app.pid]
            started.extend(pid for pid in again if pid not in started)
            windows = [w for pid in again for w in window_titles(pid) if w[1].startswith(title)]
            for root in again:
                for pid in descendants(root):
                    for module in modules_of(pid):
                        if "pdfium" in module.lower():
                            workers[pid] = module
            # The copy the installer starts may not have the scratch session.
            for where in (session, real_session):
                if where.is_file() and pdf.name in where.read_text(encoding="utf-8", errors="replace"):
                    # The old copy wrote the scratch one too, so it counts only
                    # when it was written again after that copy ended.
                    if where == real_session or where.stat().st_mtime > began_again(again):
                        named = str(where)
            if windows and installed_engine(workers, engine) and named:
                break
            time.sleep(0.3)
        checks.note(bool(again), "the installer starts the application again",
                    f"from {folder}" if again else "no copy from the install folder is running")
        checks.note(bool(windows), f"a window titled '{title}' is shown",
                    "" if windows else f"saw {[t for p in again for _, t in window_titles(p)]}")
        checks.note(bool(named), "the document is open again, from under a folder with a space",
                    f"named in {named}" if named else f"{pdf}")
        checks.note(installed_engine(workers, engine),
                    "a worker of the new copy has the installed PDFium mapped",
                    ", ".join(sorted(set(workers.values()))))
        said = tool(folder / "tpdf-cli.exe", "--version").stdout.strip()
        checks.note(said.endswith(new), f"the installed tool is version {new}", said)

        if windows:
            # Long enough for a check for updates to have answered.
            time.sleep(12)
            now = []
            asked_until = time.monotonic() + 30
            # The first question to a new window's automation tree is often
            # answered with nothing; the tree is built after it.
            while not now and time.monotonic() < asked_until:
                now = buttons(script, windows[0][0])
                time.sleep(1)
            checks.note(bool(now) and not any(line.startswith("BUTTON Update to") for line in now),
                        "the new copy offers no update",
                        f"{len(now)} buttons read" if now else "no button could be read")
    finally:
        running = [pid for pid in copies(folder)]
        close(sorted(set(running) | ({app.pid} if app.poll() is None else set())))


def began_again(pids: list[int]) -> float:
    """When the earliest of these processes was created, as a file time in seconds."""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    earliest = float("inf")
    for pid in pids:
        handle = kernel32.OpenProcess(0x1000, False, pid)
        if not handle:
            continue
        created, a, b, c = (ctypes.c_ulonglong() for _ in range(4))
        if kernel32.GetProcessTimes(ctypes.c_void_p(handle), ctypes.byref(created), ctypes.byref(a),
                                    ctypes.byref(b), ctypes.byref(c)):
            # 100 ns ticks since 1601, to seconds since 1970.
            earliest = min(earliest, created.value / 1e7 - 11644473600)
        kernel32.CloseHandle(ctypes.c_void_p(handle))
    return earliest


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
    if registry(UNINSTALL_KEY) is not None:
        print("[ERROR] tpdf is installed on this machine; this check installs and removes it")
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
    before = {key: registry(key) for key in (PDF_CLASS, APP_KEY)}
    real_session = Path(os.environ["APPDATA"]) / "com.timostein.tpdf" / "session.json"
    with tempfile.TemporaryDirectory(prefix="tpdf-update-check-") as temporary:
        scratch = Path(temporary)
        kept = scratch / "session-as-it-was.json"
        if real_session.is_file():
            shutil.move(str(real_session), str(kept))
        spaced = scratch / "a folder with a space"
        spaced.mkdir()
        document = spaced / pdf.name
        shutil.copyfile(pdf, document)
        try:
            print(f"--- latest.json offers {new}; installing {old} into {folder} ---")
            code = bounded(f'"{installer}" /S /D={folder}', "the installer", 300)
            checks.note(code == 0, "the installer ends with exit code 0", f"exit {code}")
            said = tool(folder / "tpdf-cli.exe", "--version").stdout.strip()
            checks.note(said.endswith(old), f"the installed tool is version {old}", said)
            update(checks, folder, document, scratch, old, new, real_session, args.wait,
                   press=control != "no-press")
        except (RanOut, RuntimeError, OSError) as e:
            checks.note(False, "every step ends, and within its time limit", str(e))
        finally:
            uninstall(checks, folder, before, run_it=True)
            if real_session.is_file():
                real_session.unlink()
            if kept.is_file():
                real_session.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(str(kept), str(real_session))

    print()
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
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed: {old} updated itself to {new}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
