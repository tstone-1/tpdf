#!/usr/bin/env python3
"""Runs every quality gate, and is the definition of what the gates are.

AGENTS.md carries a rule learned the expensive way on `screenpick`: a release
checklist weaker than the gate it exists to satisfy is worse than no checklist,
because following it faithfully still produces a red build -- and the failure
arrives after the release is cut. The usual fix is to copy each command into
the checklist verbatim, flags and all, and to keep re-checking that the copy
still matches.

This script removes the copy. `BUILD.md` names one command; the gates live
here, once. Both `.github/workflows/ci.yml` and `release.yml` invoke this script
rather than re-listing the commands in YAML, which is what makes the checklist,
the local run and CI the same object rather than three things that agree today.

(This paragraph said remote CI was "deliberately not here while the project is
pre-release and single-machine" until 2026-08-02. Being single-machine was never
an argument for skipping CI -- it is an argument *for* it, and the first run
found three real defects, one of them a Windows build that had been broken for
two days on a commit whose author had watched this script report 9/9.)

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

`toolchain` runs **first, and must**: if the compiler is not the one we think,
every result after it is a statement about a different toolchain. It asserts that
the running rustc matches `rust-toolchain.toml`, and that clippy and rustfmt were
built from the same commit. The failure it exists for is invisible otherwise --
`RUSTUP_TOOLCHAIN` in the environment overrides the pin file completely and
silently, which a CI action whose job is installing a toolchain may well set, and
then the pin added to stop a new stable turning `main` red is doing nothing while
everything stays green. A pin nothing verifies is indistinguishable from no pin.

`notices` runs **last, and must**: it reads `dist/assets/*.js.map` to find which npm
packages the bundler actually put in the shipped output, so it needs the `build`
gate above it to have run. It is two checks in one command -- that
`THIRD-PARTY-NOTICES.md` still matches what the dependency tree would generate,
which is the binary-distribution obligation, and that no GPL, LGPL or AGPL
licence has appeared anywhere. The second half covers a population `cargo
metadata` structurally cannot see: the fourteen C++ libraries compiled into
libpdfium, enumerated from `vendor/pdfium/licenses/`. All three of its failure
modes were proved by mutation before it was trusted.

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
            "toolchain",
            [sys.executable, str(REPO / "scripts" / "check_toolchain.py")],
            "the running rustc is not the one rust-toolchain.toml pins",
        ),
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
        (
            "notices",
            [sys.executable, str(REPO / "scripts" / "third_party_notices.py"), "--check"],
            "THIRD-PARTY-NOTICES.md is stale, or a forbidden licence appeared",
        ),
    ]


def run(name: str, argv: "list[str]") -> "tuple[bool, float, int]":
    """Runs one gate, streaming its output. Returns (passed, seconds, exit code)."""
    print(f"\n=== {name}: {' '.join(argv)}", flush=True)
    started = time.monotonic()
    code = -1
    try:
        completed = subprocess.run(argv, cwd=REPO, check=False)
        code = completed.returncode
        ok = code == 0
    except FileNotFoundError:
        print(f"[FAIL] {argv[0]} not found on PATH", file=sys.stderr)
        ok = False
    return ok, time.monotonic() - started, code


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

    width = max(len(name) for name, _, _, _, _ in results)
    print("\n=== summary")
    failed = 0
    for name, ok, seconds, code, reason in results:
        status = "[OK]  " if ok else "[FAIL]"
        # "usually means", not "means". The reason is a static hint attached to
        # the gate, and a non-zero exit can just as easily be the checker itself
        # falling over. On 2026-08-02 `third_party_notices.py` crashed on a
        # Windows encoding bug and this line reported "THIRD-PARTY-NOTICES.md is
        # stale", which sent two rounds of investigation after a content
        # difference that did not exist. The exit code is printed for the same
        # reason: 1 is a checker saying no, and anything else is usually a
        # traceback.
        detail = "" if ok else f"  -- exit {code}; usually means: {reason}"
        # Width from the actual names, not a literal: `toolchain` is 9 and the
        # hardcoded 8 silently broke the column the day it was added. Nothing
        # parses this output -- the mutation harnesses read `cargo test` -- so
        # this is only legibility, but the fixed literal is the same shape as
        # the padded-column parsing trap in `docs/TRAPS.md`.
        print(f"{status} {name:{width}} {seconds:6.1f}s{detail}")
        failed += 0 if ok else 1

    print(f"\n{len(results) - failed}/{len(results)} gates passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
