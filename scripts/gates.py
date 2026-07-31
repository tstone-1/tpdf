#!/usr/bin/env python3
"""Runs every quality gate, and is the definition of what the gates are.

AGENTS.md carries a rule learned the expensive way on `screenpick`: a release
checklist weaker than the gate it exists to satisfy is worse than no checklist,
because following it faithfully still produces a red build -- and the failure
arrives after the release is cut. The usual fix is to copy each command into
the checklist verbatim, flags and all, and to keep re-checking that the copy
still matches.

This script removes the copy. `BUILD.md` names one command; the gates live
here, once. If remote CI is added later -- and it is deliberately not here
while the project is pre-release and single-machine -- the workflow should
invoke this script rather than re-list the commands in YAML, which is what
makes the checklist and the gate the same object rather than two things that
agree today.

Usage:
    scripts/gates.py              # run all gates, report, exit 1 if any failed
    scripts/gates.py --list       # print the commands without running them
    scripts/gates.py --gate fmt --gate clippy

Every gate runs even when an earlier one fails. Locally that is what you want:
one pass tells you everything that is wrong, rather than making you re-run to
discover the next problem. The exit code is still non-zero if any failed.

`cargo test --locked` runs the unit tests and is also a lockfile gate: it fails
on a `Cargo.lock` that was not committed after a `cargo update`, and it compiles
the test targets, which is where `--all-targets` clippy findings and broken
test-only code show up.

`cargo build --bins --examples` is there because **neither of the two gates above
links a binary**, and on 2026-07-29 this sweep reported 7/7 on Windows while
`npm run tauri build` failed outright. clippy stops at metadata and never calls
the linker at all; `cargo test` does link each binary target, but with its `main`
replaced by the test harness's own, so anything reachable only from `main` is
dead code the linker drops. `backend_probe.rs` referenced two dyld symbols that
exist on no other platform, and both gates were blind to it for exactly that
reason. A gate list that never links what it ships cannot see a link error.

`--examples` is not decoration on that flag, it is where the sixteen spike
harnesses went on 2026-07-31 when they stopped being `[[bin]]` so the installer
would stop shipping them. `backend_probe.rs` --- the file that motivated this gate
in the first place --- is one of them, so dropping `--examples` would silently
narrow the gate back to the state it was added to fix, and the only target left
under `--bins` is the app itself.

`vitest` covers the front-end logic that has an answer which can be *wrong*
rather than merely ugly -- currently command ranking. Behaviour that needs a
document and a window is asserted by `scripts/viewer_check.py` instead, which is
not a gate: it needs a built bundle and a generated fixture, neither of which a
gate run has.
"""

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CARGO_MANIFEST = str(REPO / "src-tauri" / "Cargo.toml")


def npm() -> str:
    """Resolves npm, which is `npm.cmd` on Windows and not on PATH as `npm`."""
    return shutil.which("npm") or "npm"


# (name, argv, what a failure means). Order is cheapest-first, so a formatting
# slip is reported in a second rather than after a full clippy build.
def gates() -> "list[tuple[str, list[str], str]]":
    """Returns the gate list. A function, so npm is resolved at run time."""
    return [
        (
            "pdfium",
            [sys.executable, str(REPO / "scripts" / "fetch_pdfium.py"), "--check"],
            "vendor/pdfium is absent or is not the pinned build",
        ),
        (
            "fmt",
            ["cargo", "fmt", "--manifest-path", CARGO_MANIFEST, "--check"],
            "Rust formatting differs from rustfmt",
        ),
        (
            "clippy",
            [
                "cargo",
                "clippy",
                "--manifest-path",
                CARGO_MANIFEST,
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            "clippy found a lint (warnings are denied)",
        ),
        (
            "test",
            ["cargo", "test", "--manifest-path", CARGO_MANIFEST, "--locked"],
            "a test failed, or Cargo.lock is stale",
        ),
        (
            "bins",
            [
                "cargo",
                "build",
                "--manifest-path",
                CARGO_MANIFEST,
                "--locked",
                "--bins",
                "--examples",
            ],
            "a binary does not link (clippy and cargo test never link one)",
        ),
        (
            "check",
            [npm(), "run", "check"],
            "svelte-check or tsc found a type error",
        ),
        (
            "vitest",
            [npm(), "run", "test"],
            "a front-end unit test failed",
        ),
        (
            "build",
            [npm(), "run", "build"],
            "the frontend does not build",
        ),
    ]


def run(name: str, argv: "list[str]") -> "tuple[bool, float]":
    """Runs one gate, streaming its output. Returns (passed, seconds)."""
    print(f"\n=== {name}: {' '.join(argv)}", flush=True)
    started = time.monotonic()
    try:
        completed = subprocess.run(argv, cwd=REPO, check=False)
        ok = completed.returncode == 0
    except FileNotFoundError:
        print(f"[FAIL] {argv[0]} not found on PATH", file=sys.stderr)
        ok = False
    return ok, time.monotonic() - started


def main() -> int:
    """Parses arguments, runs the selected gates, prints a summary."""
    all_gates = gates()
    names = [name for name, _, _ in all_gates]

    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument(
        "--gate",
        action="append",
        choices=names,
        help="run only this gate (repeatable; default: all)",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="print the gate commands and exit",
    )
    args = parser.parse_args()

    selected = [g for g in all_gates if not args.gate or g[0] in args.gate]

    if args.list:
        for name, argv, _ in selected:
            print(f"{name:8} {' '.join(argv)}")
        return 0

    results = [(name, *run(name, argv), reason) for name, argv, reason in selected]

    print("\n=== summary")
    failed = 0
    for name, ok, seconds, reason in results:
        status = "[OK]  " if ok else "[FAIL]"
        detail = "" if ok else f"  -- {reason}"
        print(f"{status} {name:8} {seconds:6.1f}s{detail}")
        failed += 0 if ok else 1

    print(f"\n{len(results) - failed}/{len(results)} gates passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
