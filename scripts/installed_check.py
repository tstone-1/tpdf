#!/usr/bin/env python3
"""Installs the Windows installer, uses what it installed, and removes it again.

Usage, on Windows, in a session with a desktop:
    python scripts/installed_check.py <tpdf_<version>_x64-setup.exe> <document.pdf> --word WORD

The release's run is `testdata/text-wide.pdf --word Ledger`. The word is named
and not chosen here, because whether a removal can be proved depends on what
the text recogniser can read on the page. In `text-base14.pdf` the Windows
recogniser does not read the 10 pt word the tool uses as its control, so every
removal there is written out and reported as not verified, which is the tool
being right and this check being aimed at the wrong document.

WHY THIS EXISTS. Every other check runs a binary out of `target/` or one
unpacked from the installer with 7z. Neither runs the installer, so neither
says whether a reader who double-clicks it gets a program that opens a
document. `BUILD.md` records four releases in a row whose notes read "the
installer was not run". This is that run, as one command.

WHAT IT DOES, in order, and each line it prints is one check:
  1. runs the installer silently into a folder of its own;
  2. reads the installed files and the installed tool's version;
  3. with the installed command-line tool: removes a word from the document,
     and reads the copy back to see that the word is gone;
  4. starts the installed application on the document and waits for its
     window, for a worker that has the *installed* PDFium mapped, and for the
     session file to name the document;
  5. asks the window to close and waits for the application and its workers;
  6. uninstalls, and reads the registry and the folder afterwards.

WHAT IT DOES NOT SAY. A normal build has no check harness in it, so nothing
here reads the page the window drew, and nothing edits or saves in the window:
step 3 does that work through the tool, which runs the same code in the same
kind of worker. `window_checks.py` is what drives the window, on a checks
build.

IT WRITES TO THE MACHINE: an uninstall entry, the application's own key and the
`.pdf` class of the current user, all three taken away again by the uninstall,
which step 6 reads back. It refuses to start when tpdf is installed or running
there, so it is for a build machine and not for the one you read on. The
session and the log go to a scratch folder (`TPDF_SESSION_FILE`,
`TPDF_LOG_FILE`).

IT NEEDS A DESKTOP, because step 4 opens a window. Over ssh that means an
interactive scheduled task; `BUILD.md`, *Cutting a release*, step 8.

Exit codes: 0 every check passed, 1 a check failed, 2 the run could not start.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from ctypes import wintypes
from pathlib import Path

from live_output import stream_results

UNINSTALL_KEY = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\tpdf"
PDF_CLASS = r"HKCU\Software\Classes\.pdf"
NAME = re.compile(r"^tpdf_(\d+\.\d+\.\d+)_x64-setup\.exe$")

TH32CS_SNAPPROCESS = 0x00000002
WM_CLOSE = 0x0010
MAX_PATH = 260


class PROCESSENTRY32W(ctypes.Structure):
    """Toolhelp's process record. Field order and types are load-bearing."""

    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_void_p),
        ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", wintypes.LONG),
        ("dwFlags", wintypes.DWORD),
        ("szExeFile", wintypes.WCHAR * MAX_PATH),
    ]


def processes() -> list[tuple[int, int, str]]:
    """Every process as (pid, parent pid, image name). Empty if it cannot be read."""
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    kernel32.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snapshot in (None, ctypes.c_void_p(-1).value):
        return []
    found: list[tuple[int, int, str]] = []
    try:
        entry = PROCESSENTRY32W()
        entry.dwSize = ctypes.sizeof(PROCESSENTRY32W)
        more = kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            found.append((entry.th32ProcessID, entry.th32ParentProcessID, entry.szExeFile))
            more = kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel32.CloseHandle(snapshot)
    return found


def descendants(root: int) -> list[int]:
    """The pids started by `root`, directly or through another of them."""
    table = processes()
    found: list[int] = []
    frontier = [root]
    while frontier:
        parent = frontier.pop()
        for pid, parent_pid, _ in table:
            if parent_pid == parent and pid != root and pid not in found:
                found.append(pid)
                frontier.append(pid)
    return found


def started(root: int) -> str:
    """Each process `root` started, with how many of its modules can be read."""
    from win_modules import modules_of

    names = {pid: image for pid, _, image in processes()}
    return ", ".join(f"{names.get(pid, '?')} {pid} ({len(modules_of(pid))} modules)"
                     for pid in descendants(root)) or "nothing"


def installed_engine(workers: dict[int, str], engine: str) -> bool:
    """Whether some worker maps a PDFium, and every one that does maps the installed one."""
    return bool(workers) and all(path.lower().removeprefix("\\\\?\\").startswith(engine)
                                 for path in workers.values())


def window_titles(pid: int) -> list[tuple[int, str]]:
    """The visible top-level windows of one process, as (handle, title)."""
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    found: list[tuple[int, str]] = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def each(hwnd: int, _: int) -> bool:
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            text = ctypes.create_unicode_buffer(512)
            user32.GetWindowTextW(hwnd, text, 512)
            found.append((hwnd, text.value))
        return True

    user32.EnumWindows(callback_type(each), 0)
    return found


def registry(key: str) -> str | None:
    """What `reg query /s` prints for a key, or None when the key is not there."""
    asked = subprocess.run(["reg", "query", key, "/s"], capture_output=True, text=True, check=False)
    return asked.stdout if asked.returncode == 0 else None


class Checks:
    """Prints one line per check and remembers whether any failed."""

    def __init__(self) -> None:
        self.failed: list[str] = []
        self.count = 0

    def note(self, ok: bool, name: str, detail: str = "") -> bool:
        self.count += 1
        if not ok:
            self.failed.append(name)
        print(f"{'[PASS]' if ok else '[FAIL]'} {name}{': ' + detail if detail else ''}")
        return ok


def tool(cli: Path, *args: str, timeout: float = 120) -> subprocess.CompletedProcess[str]:
    return subprocess.run([str(cli), *args], capture_output=True, text=True, encoding="utf-8",
                          errors="replace", timeout=timeout, check=False)


def check_files(checks: Checks, folder: Path, version: str) -> Path | None:
    print("--- what the installer wrote ---")
    cli = folder / "tpdf-cli.exe"
    for relative in ("tpdf.exe", "tpdf-cli.exe", "uninstall.exe", "pdfium/pdfium.dll"):
        checks.note((folder / relative).is_file(), f"{relative} is installed")
    checks.note(registry(UNINSTALL_KEY) is not None, "Windows lists tpdf as installed")
    if not cli.is_file():
        return None
    said = tool(cli, "--version")
    checks.note(said.returncode == 0 and version in said.stdout,
                f"the installed tool is version {version}", said.stdout.strip())
    return cli


def check_tool(checks: Checks, cli: Path, pdf: Path, scratch: Path, word: str) -> None:
    print("--- the installed tool removes a word and reads the copy back ---")
    before = tool(cli, "text", str(pdf))
    had = len(re.findall(re.escape(word), before.stdout, re.IGNORECASE))
    if not checks.note(before.returncode == 0 and had > 0,
                       f"the tool reads the document's text, and '{word}' is in it", f"{had} times"):
        return
    copy = scratch / "redacted.pdf"
    report = tool(cli, "redact", str(pdf), "-o", str(copy), "--text", word, "--json")
    try:
        told = json.loads(report.stdout)
    except ValueError:
        told = {}
    why = "; ".join(str(reason) for reason in told.get("reasons", [])) or report.stderr.strip()
    checks.note(report.returncode == 0, f"removing '{word}' is reported as proved clean",
                f"exit {report.returncode}" + (f", {why[:400]}" if report.returncode else ""))
    checks.note(told.get("written") is True and told.get("verified") is True and copy.is_file(),
                "the report says written and verified, and the copy is there")
    if not copy.is_file():
        return
    after = tool(cli, "text", str(copy))
    left = len(re.findall(re.escape(word), after.stdout, re.IGNORECASE))
    checks.note(after.returncode == 0 and bool(after.stdout.strip()) and left == 0,
                f"the copy still has text, and '{word}' is not in it", f"{had} before, {left} after")


def check_window(checks: Checks, folder: Path, pdf: Path, scratch: Path, version: str,
                 wait: float) -> None:
    from win_modules import maps_parser, modules_of

    print("--- the installed application opens the document ---")
    session = scratch / "session.json"
    env = dict(os.environ, TPDF_SESSION_FILE=str(session), TPDF_LOG_FILE=str(scratch / "tpdf.log"))
    app = subprocess.Popen([str(folder / "tpdf.exe"), str(pdf)], env=env, cwd=str(scratch))
    engine = str(folder / "pdfium").lower()
    title = f"tpdf v{version}"
    windows: list[tuple[int, str]] = []
    workers: dict[int, str] = {}
    named = False
    parser_in_app = False
    peak = 0
    deadline = time.monotonic() + wait
    try:
        while time.monotonic() < deadline and app.poll() is None:
            mapped, count = maps_parser(app.pid)
            parser_in_app = parser_in_app or mapped
            peak = max(peak, count)
            windows = [w for w in window_titles(app.pid) if w[1].startswith(title)]
            for pid in descendants(app.pid):
                for module in modules_of(pid):
                    if "pdfium" in module.lower():
                        workers[pid] = module
            if session.is_file():
                named = pdf.name in session.read_text(encoding="utf-8", errors="replace")
            if windows and installed_engine(workers, engine) and named:
                break
            time.sleep(0.1)

        checks.note(bool(windows), f"a window titled '{title}' is shown",
                    "" if windows else f"saw {[t for _, t in window_titles(app.pid)]}")
        checks.note(installed_engine(workers, engine), "a worker has the installed PDFium mapped",
                    ", ".join(sorted(set(workers.values()))) or f"no PDFium in: {started(app.pid)}")
        checks.note(named, "the session file names the document")
        # The count is the control: zero modules is a process that could not be
        # read, and that must not pass as a process with no parser in it.
        checks.note(peak > 0 and not parser_in_app, "the application's own process maps no PDFium",
                    f"{peak} modules seen")

        print("--- and closes when asked ---")
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        # Whatever main window it shows, so that a wrong title above does not
        # also read as an application that will not close.
        shown = [w for w in window_titles(app.pid) if w[1].startswith("tpdf v")]
        for hwnd, _ in shown:
            user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
        try:
            app.wait(timeout=20)
            closed = bool(shown)
        except subprocess.TimeoutExpired:
            closed = False
        checks.note(closed, "the application ends after its window is asked to close")
        gone_by = time.monotonic() + 10
        alive = {pid for pid, _, _ in processes()} & set(workers)
        while alive and time.monotonic() < gone_by:
            time.sleep(0.2)
            alive = {pid for pid, _, _ in processes()} & set(workers)
        checks.note(bool(workers) and not alive, "its workers are gone too",
                    f"still running: {sorted(alive)}" if alive else "")
    finally:
        if app.poll() is None:
            subprocess.run(["taskkill", "/PID", str(app.pid), "/T", "/F"], capture_output=True, check=False)
            app.wait(timeout=10)


def uninstall(checks: Checks, folder: Path, pdf_class_before: str | None) -> None:
    print("--- uninstalling ---")
    remover = folder / "uninstall.exe"
    if remover.is_file():
        # `_?=` keeps the uninstaller in place and makes it wait; without it it
        # copies itself to the temporary folder and returns at once.
        subprocess.run(f'"{remover}" /S _?={folder}', timeout=180, check=False)
    checks.note(not (folder / "tpdf.exe").exists() and not (folder / "pdfium" / "pdfium.dll").exists(),
                "the application and its PDFium are removed")
    checks.note(registry(UNINSTALL_KEY) is None, "Windows no longer lists tpdf as installed")
    checks.note(registry(PDF_CLASS) == pdf_class_before, "the .pdf class reads as it did before the install")
    shutil.rmtree(folder, ignore_errors=True)


def main() -> int:
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("installer")
    parser.add_argument("pdf")
    parser.add_argument("--word", required=True,
                        help="a word of the document that the tool can remove and prove gone")
    parser.add_argument("--version", help="when the installer's name does not carry it")
    parser.add_argument("--wait", type=float, default=60, help="seconds to wait for the window")
    args = parser.parse_args()

    if sys.platform != "win32":
        print("[ERROR] this check runs the Windows installer; run it on Windows")
        return 2
    installer = Path(args.installer).resolve()
    pdf = Path(args.pdf).resolve()
    named = NAME.match(installer.name)
    version = args.version or (named.group(1) if named else None)
    for path in (installer, pdf):
        if not path.is_file():
            print(f"[ERROR] no {path}")
            return 2
    if not version:
        print(f"[ERROR] {installer.name} does not name its version; pass --version")
        return 2
    if registry(UNINSTALL_KEY) is not None:
        print("[ERROR] tpdf is installed on this machine; this check installs and removes it")
        return 2
    if any(image.lower() == "tpdf.exe" for _, _, image in processes()):
        print("[ERROR] tpdf is running; a second start would be handed to it")
        return 2
    folder = Path(os.environ["LOCALAPPDATA"]) / "tpdf-installed-check"
    if folder.exists() or " " in str(folder):
        print(f"[ERROR] {folder} exists or has a space in it; remove it and run again")
        return 2

    checks = Checks()
    pdf_class_before = registry(PDF_CLASS)
    with tempfile.TemporaryDirectory(prefix="tpdf-installed-check-") as temporary:
        scratch = Path(temporary)
        print(f"--- installing {installer.name} into {folder} ---")
        # `/D=` is last and unquoted: that is how the installer reads it.
        ran = subprocess.run(f'"{installer}" /S /D={folder}', timeout=300, check=False)
        checks.note(ran.returncode == 0, "the installer ends with exit code 0", f"exit {ran.returncode}")
        try:
            cli = check_files(checks, folder, version)
            if cli:
                check_tool(checks, cli, pdf, scratch, args.word)
            if (folder / "tpdf.exe").is_file():
                check_window(checks, folder, pdf, scratch, version, args.wait)
        finally:
            uninstall(checks, folder, pdf_class_before)

    print()
    if checks.failed:
        print(f"[FAIL] {len(checks.failed)} of {checks.count} checks failed: {'; '.join(checks.failed)}")
        return 1
    print(f"[OK] {checks.count}/{checks.count} checks passed on the installed {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
