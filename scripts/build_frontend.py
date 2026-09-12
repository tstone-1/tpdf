#!/usr/bin/env python3
"""Verify the optional native-check build, then leave normal release assets in dist.

Called by the shared quality gates; ordinary `npm run build` stays production-only.
"""
from pathlib import Path
import subprocess
import sys
from gates import npm

ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    commands = [
        [npm(), "run", "build:checks"],
        [sys.executable, str(ROOT / "scripts" / "check_bundle_share.py"), "--checks"],
        [npm(), "run", "build"],
    ]
    for command in commands:
        result = subprocess.run(command, cwd=ROOT, check=False)
        if result.returncode:
            return result.returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
