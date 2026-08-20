#!/usr/bin/env python3
"""Type-checks the Windows tree from a Mac, so a Windows-only break is caught
before a tag is pushed rather than by a runner six minutes later.

WHY THIS EXISTS. `cargo check` on a Mac never compiles a `#[cfg(windows)]`
line, so every Windows-only file in this repository --- `print_win.rs`,
`examples/print_probe.rs`, `examples/win_sandbox_probe.rs`, the Windows halves
of `worker*.rs` --- is invisible to `scripts/gates.py`. That is not a
hypothetical: the page-move work changed `print::Pages::Only` from `Vec<u32>`
to `Vec<PagePlan>`, `print_probe.rs` was the one Windows-only caller, and 16
commits went by with 15/15 gates green before the first tag of 26.8.3 turned
both runner legs red on it. The trap is the one `AGENTS.md` already names:
*the gates had never run on the platform where they fail*.

`cargo check --target x86_64-pc-windows-msvc --all-targets` answers it, and
does not link, so no MSVC linker is needed --- only headers and an archiver.
What that costs, once per machine:

    brew install xwin llvm
    xwin --accept-license --arch x86_64 --variant desktop splat --output ~/.xwin
    scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
    cp /tmp/pdfium-win/bin/pdfium.dll vendor/pdfium/bin/pdfium.dll

The last two are not optional and the failure does not say so: Tauri's build
script resolves `bundle.resources` for the *target* platform, so without the
Windows DLL present it dies on `resource path ... doesn't exist`, which reads
like a broken checkout. `vendor/` is gitignored, so the DLL sitting beside the
dylib changes nothing tracked and nothing on this platform loads it.

The three environment variables below took five iterations to find, each one
failing with an error naming a different missing tool, which is precisely the
sort of thing that should live in a file rather than in somebody's memory:

  * `CFLAGS_<triple>` --- `ring` (from the updater's TLS stack) compiles C, and
    without the splatted headers it stops at `'assert.h' file not found`.
  * `AR_<triple>` --- `cc-rs` then wants MSVC's `lib.exe`; `llvm-lib` stands in.
  * `PATH` --- `tauri-winres` compiles a Windows resource and panics with
    `NotAttempted("llvm-rc")` unless LLVM's bin directory is on the path.

WHAT THIS IS NOT. It is not a gate, deliberately: it needs a 629 MB SDK splat
and an LLVM install that CI does not need and a fresh checkout does not have,
and CI runs a real `windows-2025` runner which is strictly better evidence. It
is the local instrument that makes the CI run a formality instead of a
discovery. Run it before pushing anything that touches a Windows-only file, and
before cutting a release.

Nor does it prove the Windows build *links* or *runs* --- clippy stops at
type-checking too. A linker error, a missing symbol at load, or a behavioural
difference is still the runner's to find.

Usage: scripts/check_windows.py [--verbose]
"""

from __future__ import annotations

import argparse
import os
import pathlib
import shutil
import subprocess
import sys

#: Repository root, from this file rather than from the working directory.
ROOT = pathlib.Path(__file__).resolve().parent.parent
TRIPLE = "x86_64-pc-windows-msvc"
LLVM_BIN = pathlib.Path("/opt/homebrew/opt/llvm/bin")
XWIN = pathlib.Path.home() / ".xwin"


def missing_prerequisites() -> list[str]:
    """What is not installed, named with the command that installs it.

    Reported all at once rather than one per run: each of these failed with an
    error naming a different tool, and finding them one at a time is five
    rebuilds.
    """
    problems: list[str] = []

    if not (XWIN / "sdk" / "include" / "ucrt" / "assert.h").is_file():
        problems.append(
            f"the Windows SDK headers are not splatted at {XWIN}\n"
            "      brew install xwin && xwin --accept-license --arch x86_64 "
            "--variant desktop splat --output ~/.xwin"
        )
    for tool in ("llvm-lib", "llvm-rc"):
        if not (LLVM_BIN / tool).is_file() and shutil.which(tool) is None:
            problems.append(f"{tool} is not installed\n      brew install llvm")
            break
    if not (ROOT / "vendor" / "pdfium" / "bin" / "pdfium.dll").is_file():
        problems.append(
            "the Windows PDFium is not in vendor/pdfium/bin/ (Tauri resolves\n"
            "      bundle.resources for the target, and fails with 'resource "
            "path ... doesn't exist')\n"
            "      scripts/fetch_pdfium.py --platform win-x64 --dest "
            "/tmp/pdfium-win\n"
            "      cp /tmp/pdfium-win/bin/pdfium.dll vendor/pdfium/bin/"
        )
    if TRIPLE not in subprocess.run(
        ["rustup", "target", "list", "--installed"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout:
        problems.append(
            f"the {TRIPLE} target is not installed\n"
            f"      rustup target add {TRIPLE}"
        )
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--verbose", action="store_true", help="print cargo's output as it goes"
    )
    args = parser.parse_args()

    problems = missing_prerequisites()
    if problems:
        print(f"[FAIL] {len(problems)} prerequisite(s) missing:")
        for problem in problems:
            print(f"  * {problem}")
        return 2

    env = dict(os.environ)
    env["PATH"] = f"{LLVM_BIN}{os.pathsep}{env.get('PATH', '')}"
    env[f"CFLAGS_{TRIPLE.replace('-', '_')}"] = " ".join(
        [
            "-Wno-unused-command-line-argument",
            f"-I{XWIN}/crt/include",
            f"-I{XWIN}/sdk/include/ucrt",
            f"-I{XWIN}/sdk/include/um",
            f"-I{XWIN}/sdk/include/shared",
        ]
    )
    env[f"AR_{TRIPLE.replace('-', '_')}"] = str(LLVM_BIN / "llvm-lib")

    argv = [
        "cargo",
        # **clippy, not check, and `-D warnings` below.** It was `cargo check`
        # until 2026-08-20, which type-checks without linting --- so a constant
        # used only from a `#[cfg(target_os = "macos")]` function passed here and
        # failed `windows-2025` as `constant TEXT_SIZE is never used`, since the
        # `clippy` gate denies warnings. That is the same gap this script exists
        # to close, one layer in: a Mac compiler never parses the other
        # platform's arms, so it cannot see what is dead there either. It cost a
        # rehearsal tag and a 25-minute round trip; clippy costs about the same
        # here as check did.
        "clippy",
        "--manifest-path",
        str(ROOT / "src-tauri" / "Cargo.toml"),
        "--target",
        TRIPLE,
        # Examples are the point: `print_probe` is an example, and it is what
        # broke. Without this flag the one target that fails is not built.
        "--all-targets",
        # Exactly what the `clippy` gate denies, so this leg and that one agree
        # about what counts as a failure.
        "--",
        "-D",
        "warnings",
    ]
    print(f"[..] cargo clippy --target {TRIPLE} --all-targets -- -D warnings", flush=True)
    result = subprocess.run(
        argv,
        cwd=ROOT,
        env=env,
        capture_output=not args.verbose,
        text=True,
    )
    if result.returncode != 0:
        if not args.verbose:
            out = (result.stdout or "") + (result.stderr or "")
            print(out.rstrip())
        print(f"\n[FAIL] the Windows tree does not type-check (exit {result.returncode})")
        return 1
    print("[OK] the Windows tree type-checks, examples included.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
