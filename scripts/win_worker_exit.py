#!/usr/bin/env python3
"""Check that a finished Windows test application left no live worker children.

Call while retaining the application's Popen object (and therefore its process
handle), so its PID cannot be recycled during enumeration. Only direct children
with the exact test executable path are inspected. Run --self-test on Windows to
prove that a live child is rejected and its terminated process is accepted.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import subprocess
import sys
import time


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD), ("th32DefaultHeapID", ctypes.c_size_t),
        ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", wintypes.LONG),
        ("dwFlags", wintypes.DWORD), ("szExeFile", wintypes.WCHAR * 260),
    ]


def _kernel():
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    api.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    api.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    for name in ("Process32FirstW", "Process32NextW"):
        getattr(api, name).argtypes = [wintypes.HANDLE, ctypes.POINTER(PROCESSENTRY32W)]
    api.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    api.OpenProcess.restype = wintypes.HANDLE
    api.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                             wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
    api.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    api.WaitForSingleObject.restype = wintypes.DWORD
    api.CloseHandle.argtypes = [wintypes.HANDLE]
    return api


def _children(api, parent: int, name: str) -> tuple[list[int], int]:
    snapshot = api.CreateToolhelp32Snapshot(2, 0)  # TH32CS_SNAPPROCESS
    if snapshot == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        entry = PROCESSENTRY32W()
        entry.dwSize = ctypes.sizeof(entry)
        if not api.Process32FirstW(snapshot, ctypes.byref(entry)):
            raise ctypes.WinError(ctypes.get_last_error())
        children, seen = [], set()
        while True:
            seen.add(entry.th32ProcessID)
            if entry.th32ParentProcessID == parent and entry.szExeFile.casefold() == name.casefold():
                children.append(entry.th32ProcessID)
            if not api.Process32NextW(snapshot, ctypes.byref(entry)):
                if ctypes.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                    raise ctypes.WinError(ctypes.get_last_error())
                break
        if os.getpid() not in seen:
            raise RuntimeError("process snapshot omitted the running observer")
        return children, len(seen)
    finally:
        api.CloseHandle(snapshot)


def live_workers(parent: int, binary: Path, timeout: float = 5) -> tuple[list[int], int]:
    """Return surviving child PIDs and the enumeration count; errors raise.

    This observes rather than kills: cleanup cannot turn a failed check green.
    Wait on retained process handles, never on a PID that may be reused.
    """
    api = _kernel()
    pids, count = _children(api, parent, binary.name)
    deadline = time.monotonic() + timeout
    alive = []
    expected = os.path.normcase(str(binary.resolve()))
    for pid in pids:
        handle = api.OpenProcess(0x100000 | 0x1000, False, pid)  # SYNCHRONIZE | QUERY_LIMITED_INFORMATION
        if not handle:
            if ctypes.get_last_error() == 87:  # process already disappeared
                continue
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if api.WaitForSingleObject(handle, 0) == 0:
                continue
            path = ctypes.create_unicode_buffer(32768)
            length = wintypes.DWORD(len(path))
            if not api.QueryFullProcessImageNameW(handle, 0, path, ctypes.byref(length)):
                query_error = ctypes.get_last_error()
                # A concurrent exit may make the query fail; only a signalled
                # process handle establishes that it exited.
                if api.WaitForSingleObject(handle, 0) == 0:
                    continue
                raise ctypes.WinError(query_error)
            if os.path.normcase(path.value) != expected:
                continue
            result = api.WaitForSingleObject(handle, max(0, int((deadline - time.monotonic()) * 1000)))
            if result == 258:  # WAIT_TIMEOUT
                alive.append(pid)
            elif result != 0:
                raise ctypes.WinError(ctypes.get_last_error())
        finally:
            api.CloseHandle(handle)
    return alive, count


def check_worker_exit(process: subprocess.Popen, binary: Path) -> bool:
    if os.name != "nt":
        return True
    try:
        if process.poll() is None:
            raise RuntimeError("application has not exited")
        alive, count = live_workers(process.pid, binary)
        if alive:
            print(f"[FAIL] workers survived application exit: {alive} ({count} processes enumerated)")
            return False
        print(f"[OK] no surviving test workers ({count} processes enumerated)")
        return True
    except (OSError, RuntimeError) as error:
        print(f"[FAIL] could not verify worker exit: {error}")
        return False


def self_test() -> None:
    binary = Path(sys.executable)
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
    try:
        alive, count = live_workers(os.getpid(), binary, timeout=0.05)
        assert child.pid in alive, (alive, child.pid, count)
        # Wrong executable must not match an unrelated process with our parent.
        wrong, _ = live_workers(os.getpid(), binary.parent / "tpdf-no-such-directory" / binary.name, timeout=0)
        assert not wrong, wrong
        child.kill()
        child.wait(timeout=5)
        dead, _ = live_workers(os.getpid(), binary, timeout=0)
        assert child.pid not in dead, dead
        print("[PASS] worker-exit observer detects live child, ignores unrelated executable, accepts exit")
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=5)


if __name__ == "__main__":
    if sys.argv[1:] != ["--self-test"]:
        raise SystemExit("Usage: python scripts/win_worker_exit.py --self-test")
    self_test()
