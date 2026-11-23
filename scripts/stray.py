#!/usr/bin/env python3
"""Clears leftover instances of tpdf before a harness launches its own.

**Why this exists, measured rather than anticipated.** Windows gives tpdf its
document handover through `tauri-plugin-single-instance`: a second launch forwards
its argv to the first process and then **exits**. That is exactly the behaviour a
reader wants and it is poison for a harness, because a stray instance left behind by
an earlier run --- a killed check, a timeout, an aborted build --- silently absorbs
every later launch. The new process writes nothing and exits at once, and the harness
reports `run timed out` / `no summary line, so the run did not finish`.

Which reads as the app hanging. It cost a diagnosis: `session_check.py`'s
*control: opening without a session* phase timed out while `verify` on the same
document passed 7/7 in the same run, with four stray processes on the machine. Same
code, cleared table, and the phase passes. Nothing was wrong with the app.

So the hazard is not "a stray process is untidy", it is that **single-instance
converts a stray process into a launch that succeeds and does nothing**, and the
failure surfaces one phase later as a timeout with no output at all.

`clear_strays` matches on the **executable path**, never on the process name. A
harness that killed every `tpdf` would kill the copy the person at the keyboard is
reading, which is a harness that cannot be run on a working machine. Only processes
running the exact binary under test are ended, which for a `target/release` build is
always ours.

`clear_strays` reports what it did, always. A helper that silently tidies up is one whose failures
become someone else's mystery --- if a run needed this, the transcript should say so.
"""

import ctypes
from functools import cache
import os
import subprocess
import sys
from pathlib import Path


def clear_strays(binary: Path) -> int:
    """Ends any process already running `binary`, and says how many.

    Returns the number ended. Zero is the normal case and prints nothing; anything
    else prints a `[WARN]`, because a run that had to clear leftovers is a run whose
    earlier phases may have been affected by them.
    """
    path = str(Path(binary).resolve())
    try:
        pids = _running(path)
    except Exception as exc:  # noqa: BLE001 - a probe failure must not stop the run
        print(f"[WARN] could not check for stray instances of {path}: {exc}")
        return 0

    if not pids:
        return 0

    print(
        f"[WARN] {len(pids)} stray instance(s) of {Path(path).name} were already "
        f"running (pids {', '.join(map(str, pids))}); ending them. On Windows a stray "
        f"instance silently absorbs later launches through the single-instance plugin, "
        f"so a run that finds any here should be treated as suspect."
    )
    ended = 0
    for pid in pids:
        if sys.platform == "win32" or _executable(pid) == path:
            _end(pid)
            ended += 1
    return ended


def _running(path: str) -> list[int]:
    """Pids whose executable is exactly `path`."""
    if sys.platform == "win32":
        # CIM rather than `tasklist`, because only CIM reports the full executable
        # path --- and the path is the whole point of matching this way.
        out = subprocess.run(
            [
                "powershell",
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process | "
                "Where-Object { $_.ExecutablePath -ne $null } | "
                "ForEach-Object { \"$($_.ProcessId)|$($_.ExecutablePath)\" }",
            ],
            capture_output=True,
            text=True,
            timeout=60,
        ).stdout
        found = []
        for line in out.splitlines():
            pid, _, exe = line.partition("|")
            if exe.strip().lower() == path.lower() and pid.strip().isdigit():
                found.append(int(pid))
        return found

    out = subprocess.run(["ps", "-axo", "pid="], check=True, capture_output=True,
                         text=True, timeout=60).stdout
    return [int(pid) for pid in out.split() if pid.isdigit() and _executable(int(pid)) == path]


@cache
def _libproc() -> ctypes.CDLL:
    return ctypes.CDLL("/usr/lib/libproc.dylib")


def _executable(pid: int) -> str | None:
    """Read the kernel's image path. Arguments can contain any other program's name."""
    if sys.platform == "darwin":
        buf = ctypes.create_string_buffer(4096)  # PROC_PIDPATHINFO_MAXSIZE
        if _libproc().proc_pidpath(pid, buf, len(buf)) <= 0:
            return None
        return os.path.realpath(os.fsdecode(buf.value))
    try:
        return os.readlink(f"/proc/{pid}/exe")
    except OSError:
        return None


def _end(pid: int) -> None:
    """Ends one process, ignoring a race with it exiting on its own."""
    if sys.platform == "win32":
        subprocess.run(
            ["taskkill", "/PID", str(pid), "/F", "/T"],
            capture_output=True,
            timeout=30,
        )
    else:
        subprocess.run(["kill", "-9", str(pid)], capture_output=True, timeout=30)


def main(argv: "list[str]") -> int:
    """Reports whether anything is holding the single-instance slot.

    **This module had no entry point until 2026-09-07, and that was a defect
    rather than a style.** `viewer_check.py`'s own failure message names this
    file --- *"a second launch forwarding its argv to a window already open
    (close it, or see `scripts/stray.py`)"* --- so a reader who hits that message
    runs it. With no `__main__` it printed nothing and exited 0, which is
    byte-for-byte what "there are no strays" would look like. It was run in
    exactly that situation with three `tpdf.exe` alive and answered clean.

    It **reports and does not kill**, which is the same policy `clear_strays`
    holds and for its reason: a person may have their own tpdf open on a
    document, and a diagnostic that closes it is one nobody can run on a working
    machine. The harness does the killing, because the harness knows which binary
    is under test.

    Exit 1 when something is running, so a caller can branch on it. Exit 2 when
    the probe itself could not answer --- which must not be reported as a clean
    machine, since that is the confusion this whole function exists to end.
    """
    binary = Path(argv[0]) if argv else _default_binary()
    path = str(binary.resolve())
    try:
        pids = _running(path)
    except Exception as exc:  # noqa: BLE001 - the failure must not read as clean
        print(f"[FAIL] could not check for instances of {path}: {exc}")
        return 2

    if not pids:
        print(f"[OK] nothing is running {path}")
        return 0

    print(
        f"[WARN] {len(pids)} instance(s) of {binary.name} are running "
        f"(pids {', '.join(map(str, pids))}). On Windows a running instance "
        f"absorbs a later launch through the single-instance plugin, so a harness "
        f"started now writes nothing and looks like it hung. Close the window, or "
        f"end them yourself -- this reports and does not kill."
    )
    return 1


def _default_binary() -> Path:
    """The release build, which is what a harness is pointed at."""
    root = Path(__file__).resolve().parent.parent
    name = "tpdf.exe" if sys.platform == "win32" else "tpdf"
    return root / "src-tauri" / "target" / "release" / name


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
