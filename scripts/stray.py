#!/usr/bin/env python3
"""Clears leftover instances of the binary a harness is about to launch.

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

Matched on the **executable path**, never on the process name. A harness that killed
every `tpdf` would kill the copy the person at the keyboard is reading, which is a
harness that cannot be run on a working machine. Only processes running the exact
binary under test are ended, which for a `target/release` build is always ours.

Reports what it did, always. A helper that silently tidies up is one whose failures
become someone else's mystery --- if a run needed this, the transcript should say so.
"""

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
    for pid in pids:
        _end(pid)
    return len(pids)


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

    # `pgrep -f` matches the whole command line, and the binary path is its first
    # word for every launch a harness makes.
    out = subprocess.run(
        ["pgrep", "-f", path], capture_output=True, text=True, timeout=60
    ).stdout
    return [int(p) for p in out.split() if p.isdigit()]


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
