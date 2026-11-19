#!/usr/bin/env python3
"""Holds `answer_rewrites`, the half of the disk-change phase that runs outside the app.

The app cannot write a file, so `src/lib/diskchangecheck.ts` prints a request line
and waits until the copy on disk has changed. A request this side does not answer
fails that phase as "the copy on disk did not change", which reads as a fault in the
window; a request answered twice, or a quoted one answered, moves the file at a
moment the phase is not waiting for. The child here plays the page: it asks, and
waits for the file to grow before it asks again or exits.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tabs_check import answer_rewrites  # noqa: E402

PDF = b"%PDF-1.7\n1 0 obj\n<< >>\nendobj\ntrailer\n<< >>\n%%EOF\n"

# Asks twice, each time waiting for the file to grow, with a line between that
# quotes the request inside a sentence; then exits with the code it is given.
PAGE = r"""
import sys, time
from pathlib import Path
log, target, code = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3])
def ask(n):
    size = target.stat().st_size
    with log.open("a", encoding="utf-8") as out:
        out.write(f"[disk-change] rewrite {n}\n")
    deadline = time.monotonic() + 10
    while target.stat().st_size == size:
        if time.monotonic() > deadline:
            sys.exit(9)
        time.sleep(0.02)
ask(1)
with log.open("a", encoding="utf-8") as out:
    out.write("[FAIL] the copy did not change; is tabs_check.py answering [disk-change] rewrite 7?\n")
ask(2)
sys.exit(code)
"""


class AnswerRewrites(unittest.TestCase):
    def run_page(self, code: int) -> tuple[int, bytes]:
        with tempfile.TemporaryDirectory() as scratch:
            log, target = Path(scratch) / "run.log", Path(scratch) / "first.pdf"
            log.write_text("", encoding="utf-8")
            target.write_bytes(PDF)
            child = subprocess.Popen([sys.executable, "-c", PAGE, str(log), str(target), str(code)])
            try:
                returned = answer_rewrites(child, log, target, 30)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()
            return returned, target.read_bytes()

    def test_each_request_is_answered_once_by_an_appended_comment(self) -> None:
        code, after = self.run_page(0)
        self.assertEqual(code, 0)
        self.assertTrue(after.startswith(PDF), "the copy's own bytes are kept")
        self.assertEqual(after[len(PDF):], b"\n% tpdf disk-change check 1\n\n% tpdf disk-change check 2\n")

    def test_the_app_exit_code_is_returned(self) -> None:
        self.assertEqual(self.run_page(3)[0], 3)

    def test_a_run_that_does_not_end_times_out_as_wait_does(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            log, target = Path(scratch) / "run.log", Path(scratch) / "first.pdf"
            log.write_text("", encoding="utf-8")
            target.write_bytes(PDF)
            child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
            try:
                with self.assertRaises(subprocess.TimeoutExpired):
                    answer_rewrites(child, log, target, 0.3)
            finally:
                child.kill()
                child.wait()
            self.assertEqual(target.read_bytes(), PDF, "nothing asked, nothing changed")


if __name__ == "__main__":
    unittest.main()
