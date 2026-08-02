#!/usr/bin/env python3
"""Asserts that the Rust toolchain actually running is the one pinned.

`rust-toolchain.toml` states a channel, and rustup honours it -- unless
`RUSTUP_TOOLCHAIN` is set in the environment, which overrides the file
completely and without a word. That is not a hypothetical corner: a CI action
whose job is to install a toolchain is entitled to set it, and the failure mode
is the worst kind. Everything builds, every gate passes, and the pin that was
added specifically so that a new stable could not turn `main` red is doing
nothing at all. A pin nothing verifies is indistinguishable from no pin.

So this is the gate that makes the pin real, and it runs first: if the compiler
is not the one we think, every result after it is about a different toolchain.

Usage:
    scripts/check_toolchain.py
"""

import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PIN_FILE = REPO / "rust-toolchain.toml"


def pinned_channel() -> str:
    """Reads the pinned channel from rust-toolchain.toml."""
    with PIN_FILE.open("rb") as handle:
        return tomllib.load(handle)["toolchain"]["channel"]


def running_version(argv: "list[str]") -> "str | None":
    """Returns the `x.y.z` a toolchain binary reports, or None if it will not run."""
    try:
        out = subprocess.run(
            argv,
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    hit = re.search(r"\b(\d+\.\d+\.\d+)\b", out)
    return hit.group(1) if hit else None


def running_host() -> "str | None":
    """Returns the host triple rustc reports, or None if it will not run."""
    try:
        out = subprocess.run(
            ["rustc", "-vV"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    hit = re.search(r"^host:\s*(\S+)$", out, re.M)
    return hit.group(1) if hit else None


# The ABI each platform's product actually is. PDFium `win-x64` is an MSVC
# build, the WiX and NSIS installers are MSVC, and every Windows measurement in
# AGENTS.md was taken on MSVC. A GNU-ABI toolchain is a different product, not a
# variation on this one.
EXPECTED_ENV = {"win32": "pc-windows-msvc", "darwin": "apple-darwin"}


def commit_hash(argv: "list[str]") -> "str | None":
    """Returns the toolchain commit hash a binary reports in parentheses."""
    try:
        out = subprocess.run(
            argv,
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    hit = re.search(r"\(([0-9a-f]{7,40})\b", out)
    return hit.group(1) if hit else None


def main() -> int:
    """Compares the pin against what is actually running."""
    pin = pinned_channel()
    rustc = running_version(["rustc", "--version"])
    override = os.environ.get("RUSTUP_TOOLCHAIN")

    print(
        f"pinned={pin}  rustc={rustc}  host={running_host() or '(unknown)'}  "
        f"RUSTUP_TOOLCHAIN={override or '(unset)'}"
    )

    if rustc is None:
        print("[FAIL] rustc did not run -- is rustup installed?", file=sys.stderr)
        return 1

    if rustc != pin:
        print(
            f"[FAIL] rustc is {rustc}, but rust-toolchain.toml pins {pin}.",
            file=sys.stderr,
        )
        if override:
            # Name the cause rather than leaving it to be discovered. This is the
            # one failure mode the gate exists for, and it is invisible from the
            # version numbers alone.
            print(
                f"       RUSTUP_TOOLCHAIN={override} is set, and it overrides\n"
                "       rust-toolchain.toml silently. Something in the environment\n"
                "       -- most likely a CI step that installs its own toolchain --\n"
                "       is defeating the pin. Remove it rather than changing the pin.",
                file=sys.stderr,
            )
        else:
            print(
                "       Run `rustup show` to let rustup install the pinned\n"
                "       toolchain, or edit rust-toolchain.toml deliberately.",
                file=sys.stderr,
            )
        return 1

    # A version match is not a toolchain match. `channel = "1.97.1"` carries no
    # target triple, so rustup resolves it against its **default host triple** --
    # a different setting from the default *toolchain*, with nothing keeping the
    # two in step. On this project's Windows desktop the default toolchain was
    # `stable-x86_64-pc-windows-msvc` while the default host was
    # `x86_64-pc-windows-gnu`, so adding the pin silently moved that machine from
    # MSVC to GNU. rustc reported the pinned 1.97.1, clippy and rustfmt matched
    # its commit hash, and this gate said [OK]; three gates later the build died
    # on a missing `dlltool.exe`, because the GNU ABI wants MinGW binutils that
    # were never installed.
    #
    # CI cannot catch it: GitHub's windows runners default to MSVC, so the pin
    # resolves correctly there and stays green. It is per-machine and invisible
    # from the other platform, which is why it has to live in the gate.
    #
    # Fix the machine, not the pin: `rustup set default-host <triple>`. Writing a
    # full triple into rust-toolchain.toml would pin one platform's ABI into a
    # file both platforms read.
    host = running_host()
    expected_env = EXPECTED_ENV.get(sys.platform)
    if expected_env is not None:
        if host is None:
            print("[FAIL] rustc did not report a host triple.", file=sys.stderr)
            return 1
        if not host.endswith(expected_env):
            print(
                f"[FAIL] rustc's host triple is {host}, but this platform builds "
                f"{expected_env}.\n"
                "       The pin names a channel and no triple, so rustup resolved "
                "it against\n"
                "       `rustup show`'s default host -- which is not the same "
                "setting as the\n"
                "       default toolchain. Fix the machine:\n"
                f"       rustup set default-host <arch>-{expected_env}",
                file=sys.stderr,
            )
            return 1

    # clippy and rustfmt must come from the *same* toolchain, and their version
    # numbers cannot say so: the three schemes are unrelated -- rustc 1.97.1,
    # clippy 0.1.97, rustfmt 1.9.0-stable. The first draft of this check compared
    # "the minor" across them and failed on a perfectly correct toolchain,
    # because clippy's 97 is its patch and rustfmt's version tracks nothing here
    # at all.
    #
    # What all three do carry is the **commit hash of the toolchain build**, and
    # that is the actual oracle for "same toolchain". rustc prints nine
    # characters of it and the others ten, so compare on the shorter.
    #
    # This matters rather than being belt-and-braces: a clippy from a different
    # toolchain is exactly how `-D warnings` starts failing on lints the pin was
    # added to hold back.
    rustc_hash = commit_hash(["rustc", "--version"])
    for name, argv in (
        ("clippy", ["cargo", "clippy", "--version"]),
        ("rustfmt", ["cargo", "fmt", "--version"]),
    ):
        other = commit_hash(argv)
        if other is None:
            print(
                f"[FAIL] {name} did not run, or printed no commit hash. "
                "rust-toolchain.toml must list it under `components`.",
                file=sys.stderr,
            )
            return 1
        if rustc_hash and not (
            other.startswith(rustc_hash) or rustc_hash.startswith(other)
        ):
            print(
                f"[FAIL] {name} was built from {other} and rustc from "
                f"{rustc_hash} -- they are different toolchains.",
                file=sys.stderr,
            )
            return 1

    print(f"[OK] toolchain is the pinned {pin}, with clippy and rustfmt to match.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
