#!/usr/bin/env python3
"""Runs the sustained-scroll benchmark (spike 0.8) and prints its report.

Usage:
    scripts/scroll_bench.py <app-binary> <file.pdf> [--set KEY=VAL ...]

Unlike the startup benchmark, one launch measures everything: the app opens the
document once and runs every variant interleaved inside it, because what is
being measured is a frame loop rather than a process start. So this script is
thin. What it is not is optional --- it exists for the two guards, which are the
difference between a failed run and a run that silently produces nothing:

  - WebKit suspends a page whose window is not visible, and that stops
    `requestAnimationFrame`. A frame-rate benchmark behind a lock screen or a
    dark display does not run slowly, it does not run at all, and it cannot
    report that because the timer that would report it is suspended too.
  - The app must be a release *bundle*. Under `tauri dev` the frontend is served
    by Vite over HTTP and our own Rust is unoptimised, which does not slow
    things uniformly --- it slows our code and not Pdfium's. See AGENTS.md.

Knobs are passed straight through as environment: --set SCROLL_PX=8 becomes
TPDF_SCROLL_PX=8. The defaults live in src-tauri/src/lib.rs.
"""

import argparse
import os
import subprocess
import sys


def hold_display_awake() -> "subprocess.Popen[bytes] | None":
    """Keeps the display awake and on for as long as this process lives.

    `-u` as well as `-d`: `-d` only prevents the display going idle, and a
    display that is already off stays off. `-u` declares user activity, which
    turns it back on. The assertion is released when this process exits.
    """
    if sys.platform != "darwin":
        return None
    try:
        return subprocess.Popen(
            ["caffeinate", "-du", "-w", str(os.getpid())],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        print("[WARN] caffeinate not available; a sleeping display will hang the run", file=sys.stderr)
        return None


def screen_is_locked() -> bool:
    """Whether the login session is locked, which makes every window invisible."""
    if sys.platform != "darwin":
        return False
    result = subprocess.run(["ioreg", "-n", "Root", "-d1", "-a"], capture_output=True, text=True)
    marker = "<key>CGSSessionScreenIsLocked</key>"
    index = result.stdout.find(marker)
    return index >= 0 and "<true/>" in result.stdout[index : index + 120]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument(
        "--set",
        action="append",
        default=[],
        metavar="KEY=VAL",
        help="a TPDF_-prefixed knob, e.g. SCROLL_PX=8; repeatable",
    )
    parser.add_argument("--timeout", type=float, default=1200.0)
    args = parser.parse_args()

    if "target/debug" in args.binary or args.binary.endswith("tauri"):
        print(
            "[FAIL] this must run against a release bundle. A debug build leaves\n"
            "       our Rust unoptimised while Pdfium arrives prebuilt, which\n"
            "       inverts ratios rather than inflating them. Build with\n"
            "       npm run tauri build -- --bundles app",
            file=sys.stderr,
        )
        return 1

    if screen_is_locked():
        print(
            "[FAIL] the screen is locked, so every window is occluded and WebKit\n"
            "       suspends the page. requestAnimationFrame never fires and the\n"
            "       run cannot even time itself out. Unlock and re-run.",
            file=sys.stderr,
        )
        return 1

    hold_display_awake()

    env = dict(os.environ, TPDF_SCROLLBENCH=args.pdf)
    for assignment in args.set:
        key, _, value = assignment.partition("=")
        if key:
            env[f"TPDF_{key}"] = value

    try:
        completed = subprocess.run(
            [args.binary], env=env, capture_output=True, text=True, timeout=args.timeout
        )
    except subprocess.TimeoutExpired:
        print("[FAIL] run timed out", file=sys.stderr)
        return 1

    print(completed.stdout, end="")
    if completed.returncode != 0:
        print(completed.stderr, end="", file=sys.stderr)
        print(f"[FAIL] exit {completed.returncode}", file=sys.stderr)
        return completed.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
