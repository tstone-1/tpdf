#!/usr/bin/env python3
"""Clears leftover instances of tpdf before a harness launches its own.

Two functions with two policies, and the difference is the whole content of this
module. `clear_strays` ends only what is running the exact binary under test.
`clear_leftover_app` ends every tpdf, which is what the two loops over
`viewer_check.py` have always done; its docstring says why that is not simply
narrowed to the other.

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
always ours. `clear_leftover_app` does not hold to that on Windows and says so.

`clear_strays` reports what it did, always. A helper that silently tidies up is one whose failures
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


#: The bundle path a leftover viewer run is matched on, on POSIX.
#:
#: Not resolved from a caller's argument, because the two callers hand
#: `viewer_check.py` a path that may be either the bundle or the executable
#: inside it, and the pattern has to match the command line either way.
LEFTOVER_APP = "tpdf.app/Contents/MacOS/tpdf"


def clear_leftover_app() -> None:
    """Kills any tpdf still running, on whichever platform this is.

    The coarse counterpart to `clear_strays`, used by the two scripts that drive
    `viewer_check.py` in a loop --- `mutate_viewer.py` and `viewer_sweep.py` ---
    where a window left by the previous iteration occludes the next one, WebKit
    suspends an occluded page, and the run then produces nothing while using no
    CPU. Both had their own copy of these six lines.

    **This was `pkill` unconditionally, and on Windows that is not a program.**
    `check=False` swallows a non-zero exit and not a `FileNotFoundError`, so
    `mutate_viewer.py` died before its first mutation with a traceback and exit
    0, and `viewer_sweep.py` died on its first corpus with a traceback and no
    table. A harness that dies while looking like one that ran is the failure
    this repository has an entry about.

    Failure is ignored on purpose: "there was nothing to kill" is the ordinary
    case and both tools report it with a non-zero exit. And no such tool on this
    machine is a slow run or a swallowed launch rather than a wrong answer, both
    of which are visible in the check output, so it is not worth refusing over.

    **It matches by image name on Windows, which `clear_strays` refuses to do**,
    and the difference is not an oversight to tidy away. `clear_strays` will only
    end a process running the exact binary under test, precisely so a harness
    cannot close the document a reader has open in their own installed tpdf; this
    ends every `tpdf.exe`. Narrowing it is the right change and is a behaviour
    change to two harnesses that need a screen to run, so it is named here rather
    than made blind.
    """
    if sys.platform == "win32":
        command = ["taskkill", "/F", "/IM", "tpdf.exe"]
    else:
        command = ["pkill", "-f", LEFTOVER_APP]
    try:
        subprocess.run(command, check=False, capture_output=True)
    except OSError:
        pass


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
