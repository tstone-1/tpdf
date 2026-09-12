#!/usr/bin/env python3
"""Drive signed-save consent in the real checks app on disposable synthetic PDFs.

Usage: uv run scripts/signed_save_check.py <checks-executable> <signed-fixture.pdf>
Two separate launches prove cancellation leaves identical bytes and acceptance
writes the edit. The app drives its own modal, requiring no OS Accessibility grant.
"""
import argparse
import os
from pathlib import Path
import signal
import subprocess
import tempfile
from harness_launch import report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("fixture", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="tpdf-signed-save-") as directory:
        room = Path(directory)
        original = args.fixture.read_bytes()
        for action in ("cancel", "accept"):
            pdf = room / f"synthetic-{action}.pdf"
            pdf.write_bytes(original)
            log = room / f"{action}.log"
            env = dict(os.environ, TPDF_OPENCHECK=f"signed-save-{action}:{pdf}", TPDF_SESSION_FILE=str(room / "session.json"))
            with log.open("wb") as output:
                process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=os.name != "nt")
                try:
                    code = process.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], check=False)
                    else:
                        os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
                    raise RuntimeError("signed-save check timed out")
            text = log.read_text(errors="replace")
            if not report(text, code, phase=action):
                return 1
            changed = pdf.read_bytes() != original
            if changed != (action == "accept"):
                raise RuntimeError(f"file bytes disagree with {action}")
            print(f"[OK] {action}: file bytes " + ("changed" if changed else "identical"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
