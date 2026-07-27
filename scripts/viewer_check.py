#!/usr/bin/env python3
"""Runs the viewer's functional check inside a real webview.

Usage:
    scripts/viewer_check.py <app-binary> <file.pdf> [--timeout SECONDS]

What it checks is in `src/lib/viewercheck.ts`. What this script adds is the one
guard that has nothing to do with the viewer: WebKit suspends a page whose
window is not visible, so behind a lock screen the check does not fail, it
stops -- and a check that cannot report its own failure is worse than none.

It does not require a *release* build -- it asserts behaviour rather than timing
it, so a debug binary is only slower -- but it does require a **bundle**. A raw
`cargo build` executable opens a window and never runs a line of JavaScript,
because WKWebView needs the bundle identity; the failure is a blank window, no
error, and no output at all. Build with `npm run tauri build -- --bundles app`
and point this at `target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`.
(The earlier wording here said only "does not require a release bundle", which
is true and cost an afternoon.)
"""

import argparse
import os
import subprocess
import sys

from webview_guard import require_visible_session


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--timeout", type=float, default=300.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    env = dict(os.environ, TPDF_VIEWERCHECK=args.pdf)

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
