#!/usr/bin/env python3
"""Runs the clippy gate on the Linux tree from Windows, under WSL, so a change
that leaves the macOS and Linux trees broken is caught before the push rather
than by two runners twenty minutes later.

WHY THIS EXISTS. `scripts/gates.py` on Windows never compiles a
`#[cfg(any(target_os = "macos", target_os = "linux"))]` line, and it cannot see
the opposite either: shared code whose every caller has just moved behind
`#[cfg(windows)]` is live on Windows and dead on the other two, where
`-D warnings` makes it an error. On 2026-10-20 the print preview did exactly
that to `commands::print::chosen_pages`: 33 of 33 gates on the Windows desktop,
then `function chosen_pages is never used` on both the macOS and the Ubuntu
runner. `check_windows.py` is the instrument for the other direction, a Mac
change that breaks Windows; this is its mirror. See the trap *A helper whose
callers all became Windows-only passed every gate on Windows and failed on the
other two*.

WHAT IT SENDS. The tree as it is here, as `run_on_mac.py` sends it: `git stash
create` makes a commit of the tracked changes without touching the working
tree, and `refs/tpdf-check/linux` points at it. Untracked files do not travel,
so it refuses while there are any (`git add` them first, as the `untracked`
gate asks). With a clean tree it sends `HEAD`.

WHERE IT RUNS. In `~/tpdf-linux` inside the WSL distribution, a clone of this
one made on the first run and moved to the sent commit each time with a
detached checkout. Its `node_modules`, `vendor`, `dist` and `target` are its
own: building in `/mnt/c` would share them with the Windows tree. The pinned
PDFium and language data are installed there by the repository's own fetch
scripts, because Tauri's build script refuses to compile while a bundle
resource is missing.

WHAT IT RUNS. `python3 scripts/gates.py --gate clippy` in that checkout, so the
command is the gate's own, with its `--locked --all-targets -D warnings` and its
bound on Cargo's jobs, and not a copy of it.

WHAT THIS IS NOT. Linux is not macOS. The failure it was written for, dead code
outside Windows, shows on both, but a line under `target_os = "macos"` alone is
still compiled only by the macOS runner. Nor is it a gate: it needs WSL with
Rust, WebKitGTK and the GTK headers installed (BUILD.md, *Linux*), which CI
does not, and CI's Ubuntu runner is better evidence. Run it before pushing a
change that adds, moves or removes a platform `cfg`, and before a tag.

Usage: scripts/check_linux.py [--distro NAME]
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
REF = "refs/tpdf-check/linux"
CLONE = "~/tpdf-linux"


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=REPO, check=True, capture_output=True, text=True
    ).stdout.strip()


def wsl_path(path: pathlib.Path) -> str:
    """`C:\\Users\\x` as WSL mounts it, `/mnt/c/Users/x`."""
    drive, rest = path.drive, path.as_posix()[len(path.drive) :]
    return f"/mnt/{drive.rstrip(':').lower()}{rest}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--distro", default="Ubuntu-24.04", help="WSL distribution (default: %(default)s)")
    args = parser.parse_args()

    untracked = git("ls-files", "--others", "--exclude-standard")
    if untracked:
        print("[FAIL] untracked files would not be sent; `git add` them first:")
        for name in untracked.splitlines():
            print(f"       {name}")
        return 2
    # Empty when nothing tracked has changed, and then HEAD is the tree.
    sent = git("stash", "create") or git("rev-parse", "HEAD")
    git("update-ref", REF, sent)
    print(f"[INFO] sending {sent[:10]} to {args.distro}:{CLONE}")

    script = f"""set -e
source "$HOME/.cargo/env" 2>/dev/null || true
origin='{wsl_path(REPO)}'
if [ ! -d {CLONE}/.git ]; then git clone -q "$origin" {CLONE}; fi
cd {CLONE}
git fetch -q "$origin" '{REF}'
git checkout -q --detach -f FETCH_HEAD
# Tracked leftovers of the previous commit go; ignored ones (vendor, target,
# node_modules) stay, since rebuilding them is the expensive part.
git clean -q -fd
python3 scripts/fetch_pdfium.py >/dev/null
python3 scripts/fetch_tessdata.py >/dev/null
if [ ! -d dist ]; then npm ci --silent && npm run build --silent; fi
python3 scripts/gates.py --gate clippy
"""
    done = subprocess.run(
        ["wsl.exe", "-d", args.distro, "-e", "bash", "-s"],
        input=script.encode(),
        check=False,
    )
    print("[OK] the Linux tree passes clippy" if done.returncode == 0
          else f"[FAIL] the Linux tree does not pass clippy (exit {done.returncode})")
    return 0 if done.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
