#!/usr/bin/env python3
"""Lists the DLLs another process has loaded, on Windows.

This exists to answer one question with an *observable* rather than a claim:
after a document is opened, is the PDF parser mapped into the application
process? `AGENTS.md` records why the distinction matters --- a milestone we
record says what our code believes it did, and the question here is what the
process actually is. On macOS `backend-probe` reads dyld's own image table from
inside the process; this reads the module list from **outside** it, which is
strictly better evidence: nothing in tpdf participates in the answer.

Toolhelp rather than `EnumProcessModules`, because it needs no handle rights
negotiation and no two-call sizing dance. It sees only same-bitness modules,
which is what we want --- the app and this interpreter are both x64.

Usable on its own:

    python scripts/win_modules.py <pid>
"""

from __future__ import annotations

import ctypes
from ctypes import wintypes

TH32CS_SNAPMODULE = 0x00000008
TH32CS_SNAPMODULE32 = 0x00000010
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
ERROR_BAD_LENGTH = 24
MAX_MODULE_NAME32 = 255
MAX_PATH = 260


class MODULEENTRY32W(ctypes.Structure):
    """Toolhelp's module record. Field order and types are load-bearing."""

    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("th32ModuleID", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("GlblcntUsage", wintypes.DWORD),
        ("ProccntUsage", wintypes.DWORD),
        # Pointer-sized, and declaring either of these as DWORD silently
        # misaligns every field after it on x64 --- the names then come back as
        # garbage rather than as an error, which reads exactly like a process
        # that has loaded nothing recognisable.
        ("modBaseAddr", ctypes.c_void_p),
        ("modBaseSize", wintypes.DWORD),
        ("hModule", ctypes.c_void_p),
        ("szModule", wintypes.WCHAR * (MAX_MODULE_NAME32 + 1)),
        ("szExePath", wintypes.WCHAR * MAX_PATH),
    ]


def modules_of(pid: int) -> list[str]:
    """Every module path the process has mapped, or [] if it cannot be read.

    An empty list means "could not enumerate", never "nothing is loaded" --- a
    live process always has at least its own image and ntdll. Callers must treat
    the two the same way `AGENTS.md` insists: an enumeration probe needs a count
    of everything it saw, so that "found nothing" can be told apart from
    "enumerated nothing".
    """
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel32.Module32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(MODULEENTRY32W)]
    kernel32.Module32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(MODULEENTRY32W)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

    # ERROR_BAD_LENGTH is documented as transient: the target is mid-load and the
    # module list moved underneath the snapshot. Retrying is the documented fix,
    # and this is called precisely while an application is starting up.
    for _ in range(20):
        snapshot = kernel32.CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid
        )
        if snapshot != INVALID_HANDLE_VALUE:
            break
        if ctypes.get_last_error() != ERROR_BAD_LENGTH:
            return []
    else:
        return []

    found: list[str] = []
    try:
        entry = MODULEENTRY32W()
        entry.dwSize = ctypes.sizeof(MODULEENTRY32W)
        if not kernel32.Module32FirstW(snapshot, ctypes.byref(entry)):
            return []
        while True:
            found.append(entry.szExePath or entry.szModule)
            if not kernel32.Module32NextW(snapshot, ctypes.byref(entry)):
                break
    finally:
        kernel32.CloseHandle(snapshot)
    return found


def maps_parser(pid: int) -> tuple[bool, int]:
    """(is the PDF parser mapped, how many modules were seen at all).

    The second value is the control. Without it a `False` from a failed
    enumeration is indistinguishable from a `False` that means the containment
    worked --- and that is the direction of error that would let a broken flip
    look like a fixed one.
    """
    loaded = modules_of(pid)
    return any("pdfium" in m.lower() for m in loaded), len(loaded)


if __name__ == "__main__":
    import sys

    target = int(sys.argv[1])
    mapped, count = maps_parser(target)
    print(f"pid {target}: {count} modules, pdfium mapped: {mapped}")
    for path in modules_of(target):
        print(f"  {path}")
