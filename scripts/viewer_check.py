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
    # 300 s was marginal and produced an intermittent failure that looked like a
    # hang: `vector-multi` renders twelve A0 pages and measured 276 s, i.e. it
    # passed or timed out depending on the machine's mood. A timeout is not a
    # useful signal here --- nothing in this check can wedge quietly, and every
    # real failure prints a `[FAIL]` line --- so the bound exists only to stop an
    # unattended run hanging forever, and belongs well clear of the slowest
    # corpus rather than next to it.
    parser.add_argument("--timeout", type=float, default=900.0)
    args = parser.parse_args()

    if not require_visible_session():
        return 1

    env = dict(os.environ, TPDF_VIEWERCHECK=args.pdf)

    try:
        completed = subprocess.run(
            [args.binary], env=env, capture_output=True, text=True, timeout=args.timeout
        )
    except subprocess.TimeoutExpired as expired:
        # The partial transcript, not just the verdict. `viewercheck.ts` prints
        # each result as it is recorded precisely so a run that stops midway can
        # say where --- and discarding it here threw that away again, leaving a
        # timeout indistinguishable from a page that never ran a line of
        # JavaScript. Both were seen on the same corpus within an hour.
        partial = expired.stdout or b""
        if isinstance(partial, bytes):
            partial = partial.decode("utf-8", "replace")
        print(partial, end="")
        done = partial.count("[OK]") + partial.count("[FAIL]") + partial.count("[SKIP]")
        print(f"[FAIL] run timed out after {args.timeout:.0f}s, {done} checks in", file=sys.stderr)
        return 1

    print(completed.stdout, end="")
    if completed.returncode != 0:
        print(completed.stderr, end="", file=sys.stderr)
        print(f"[FAIL] exit {completed.returncode}", file=sys.stderr)
        return completed.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
