#!/usr/bin/env python3
"""Refuse a gate run over a tree with untracked files that are not ignored.

Several gates read their population from `git ls-files` --- `dates` is the one
that proved it --- so a file that exists on disk and is not yet tracked is
invisible to them, and a local run reports green over a tree whose commit will
not be. That happened twice: on 2026-09-27 the command-line tool's samples
reached CI without their `dates` exemption, and on 2026-09-28 commit cca0e57
passed 26/26 locally and failed `dates` at HEAD, because `revocation/tests.rs`
was untracked during the run. Both times the files were new and the gate run
was the pre-commit one.

The remedy is not to make each gate read the working tree instead: the tracked
set is the right population for what is about to be committed. It is to refuse
to call a run green while the two differ. `git add` the new files (or ignore
them) and run again.

Ignored files are not reported --- `git status --porcelain` leaves them out ---
so build output, generated fixtures and `.mutations/` do not trip this. On a CI
checkout nothing is untracked, and the fixture generator writes only ignored
paths, so this gate is green there by construction.

A git that cannot answer is a failure, not a clean tree: an empty listing is
the healthy answer here, so a listing that failed must not be read as one.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def untracked() -> "list[str]":
    """Paths `git status` reports as untracked and not ignored, one per file."""
    out = subprocess.run(
        [
            "git",
            "-C",
            str(ROOT),
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "-z",
        ],
        capture_output=True,
        check=True,
    )
    entries = out.stdout.decode("utf-8", errors="replace").split("\0")
    return [entry[3:] for entry in entries if entry.startswith("?? ")]


def main() -> int:
    try:
        found = untracked()
    except (subprocess.CalledProcessError, FileNotFoundError) as why:
        print(f"[FAIL] could not ask git for untracked files: {why}")
        return 2
    if found:
        print(
            f"[FAIL] {len(found)} untracked file(s) that are not ignored; gates that "
            "read `git ls-files` cannot see them, so a green run would not describe "
            "the commit:"
        )
        for path in found[:50]:
            print(f"       {path}")
        if len(found) > 50:
            print(f"       ... and {len(found) - 50} more")
        print("       `git add` them (or ignore them) and run the gates again.")
        return 1
    print("[OK] no untracked file outside the ignore rules")
    return 0


if __name__ == "__main__":
    sys.exit(main())
