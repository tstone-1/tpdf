#!/usr/bin/env python3
"""Runs every window check against one checks build, and says which failed.

Usage:
    scripts/window_checks.py <checks-binary> [--only TEXT]... [--list]

WHY THIS EXISTS. The window checks drive a real window, so `gates.py` cannot
run them: on a machine with no unlocked screen they hang. That left them to be
run one at a time by whoever remembered, and on 2026-10-09 two of them had been
red for days without anyone reading them: the viewer check's command lists had
fallen behind the registry, and the mark check aimed its drag in the wrong
coordinates. One command that runs them all is what a release step can name.

WHAT IT RUNS. The list below, each as its own process, one after the other:
they all want the screen. A check that exits non-zero, cannot start, or names
a fixture that is not there is a failure. A fixture that is missing is never a
skip, because a run that skipped everything would read as a run that passed.

`--only` keeps the checks whose name contains the text, for the loop while one
of them is being worked on. A value that matches nothing is refused.

NOT A GATE. It needs an unlocked, unoccluded screen and several minutes, and
it takes the keyboard focus while it runs. `BUILD.md`, *Cutting a release*,
says when it is run. The binary is a checks build (`BUILD.md`, top).
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path

from live_output import stream_results

ROOT = Path(__file__).resolve().parent.parent
TEXT = "testdata/text-heavy.pdf"
# Signed with a certificate its signer issued to themselves, which no computer
# trusts: what the viewer check's validation-data phase is refused for.
SIGNED = "testdata/incr-signed.pdf"

# Name, script, fixture, further arguments. A name is what `--only` matches and
# what the summary prints; the fixture is checked for before the script starts.
CHECKS: list[tuple[str, str, str, list[str]]] = [
    ("viewer", "viewer_check.py", TEXT, ["--signed", SIGNED]),
    ("marks on links", "mark_check.py", "testdata/links.pdf", []),
    ("marks on text", "mark_check.py", TEXT, []),
    ("tabs", "tabs_check.py", TEXT, ["--phase", "tabs"]),
    ("tab positions", "tabs_check.py", TEXT, ["--phase", "tabs-position"]),
    ("sides", "tabs_check.py", TEXT, ["--phase", "sides"]),
    ("views", "tabs_check.py", TEXT, ["--phase", "views"]),
    ("answers", "tabs_check.py", "testdata/outline-simple.pdf",
     ["--phase", "answers", "--other", "testdata/comments.pdf"]),
    ("form beside", "tabs_check.py", "testdata/form.pdf", ["--phase", "form-beside"]),
    ("import", "tabs_check.py", "testdata/text-base14.pdf",
     ["--phase", "import", "--other", "testdata/links.pdf"]),
    ("session", "session_check.py", TEXT, []),
]


def fixtures_of(fixture: str, extra: list[str]) -> list[str]:
    """Every file under `testdata/` one check reads."""
    return [fixture, *[arg for arg in extra if arg.startswith("testdata/")]]


def main() -> int:
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?")
    parser.add_argument("--only", action="append", default=[], metavar="TEXT",
                        help="run the checks whose name contains this; repeatable")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    unmatched = [text for text in args.only
                 if not any(text.lower() in name.lower() for name, *_ in CHECKS)]
    if unmatched:
        parser.error(f"--only matches no check: {', '.join(unmatched)}")
    chosen = [check for check in CHECKS
              if not args.only or any(text.lower() in check[0].lower() for text in args.only)]

    if args.list:
        for name, script, fixture, extra in chosen:
            print(f"{name:<16} {script} {fixture} {' '.join(extra)}".rstrip())
        return 0
    if not args.binary:
        parser.error("the checks binary is required")

    results: list[tuple[str, bool, str]] = []
    for name, script, fixture, extra in chosen:
        print(f"=== {name}: {script} {fixture} {' '.join(extra)}".rstrip())
        missing = [path for path in fixtures_of(fixture, extra) if not (ROOT / path).is_file()]
        if missing:
            why = f"no {', '.join(missing)}"
            print(f"[FAIL] {name}: {why}")
            results.append((name, False, why))
            continue
        started = time.monotonic()
        try:
            code = subprocess.run(
                [sys.executable, str(ROOT / "scripts" / script), args.binary, fixture, *extra],
                cwd=ROOT, check=False,
            ).returncode
        except OSError as e:
            print(f"[FAIL] {name}: did not start: {e}")
            results.append((name, False, "did not start"))
            continue
        took = f"{time.monotonic() - started:.0f}s"
        results.append((name, code == 0, took if code == 0 else f"exit {code} after {took}"))

    print()
    print("=== summary")
    for name, ok, detail in results:
        print(f"{'[OK]  ' if ok else '[FAIL]'} {name:<16} {detail}")
    failed = [name for name, ok, _ in results if not ok]
    left_out = len(CHECKS) - len(chosen)
    if left_out:
        print(f"[NOTE] {left_out} check(s) left out by --only; this is not the release's run")
    print()
    if failed:
        print(f"[FAIL] {len(failed)} of {len(results)} window checks failed: {', '.join(failed)}")
        return 1
    print(f"[OK] {len(results)}/{len(results)} window checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
