#!/usr/bin/env python3
"""The glibc a Linux build needs is the one the README states.

A Linux binary needs the newest glibc symbol version it was linked against,
and that is set by the machine that built it, not by anything in the tree.
26.10.19 was built on `ubuntu-24.04` and needs GLIBC_2.39 (Rust's standard
library refers to `pidfd_spawnp` and `pidfd_getpid`, weakly, but the version
entry is not weak), so on Ubuntu 22.04 it stops before `main` with "version
`GLIBC_2.39' not found". Measured on 2026-10-15 under WSL. The README said
"Debian, Ubuntu and their relatives" until then.

So the README carries `<!-- glibc-floor: X.Y -->` and this compares it with
what the binaries need, both ways: a higher need is a release that does not
start where the README says it does, and a lower one is a README that turns
people away for nothing. A runner image moving to a newer Ubuntu is how the
first would happen without anybody changing a line here.

    scripts/check_glibc_floor.py BINARY [BINARY ...]

Reads each binary's dynamic symbol table with `objdump -T` (binutils, on every
Ubuntu runner). Linux only; CI runs it on the window it builds, and release.yml
on the binaries inside the deb.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
README = ROOT / "README.md"

MARKER = re.compile(r"<!-- glibc-floor: (\d+\.\d+) -->")
NEEDED = re.compile(r"\(GLIBC_(\d+(?:\.\d+)+)\)")


def version(text: str) -> tuple[int, ...]:
    return tuple(int(part) for part in text.split("."))


def stated(readme: str) -> str:
    found = MARKER.findall(readme)
    if len(found) != 1:
        raise SystemExit(f"[FAIL] README.md has {len(found)} glibc-floor markers, expected 1")
    return found[0]


def needed(symbols: str) -> str | None:
    """The newest GLIBC_ version in `objdump -T` output, or None."""
    found = NEEDED.findall(symbols)
    return max(found, key=version) if found else None


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__.strip().splitlines()[0])
        print("usage: check_glibc_floor.py BINARY [BINARY ...]")
        return 2
    floor = stated(README.read_text(encoding="utf-8"))
    failed = False
    for binary in argv:
        run = subprocess.run(["objdump", "-T", binary], capture_output=True, text=True)
        need = needed(run.stdout) if run.returncode == 0 else None
        # A binary with no GLIBC_ version at all was not read: objdump refuses
        # a non-ELF file and prints nothing to match for a static one. That is
        # a failure, or a wrong path would pass.
        if need is None:
            print(f"[FAIL] {binary}: no GLIBC_ symbol versions found")
            failed = True
        elif version(need) != version(floor):
            print(f"[FAIL] {binary} needs glibc {need}; README.md states {floor}")
            failed = True
        else:
            print(f"[OK]   {binary} needs glibc {need}, as README.md states")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
