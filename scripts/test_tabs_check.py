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
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from run_on_mac import RECEIVE, REMOTE, VOICEOVER_DRIVER, remote_path
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


@unittest.skipUnless(sys.platform == "darwin", "the remote receiver runs on macOS")
class GeneratedFixtures(unittest.TestCase):
    def test_hosted_runs_require_both_bookmark_fixtures(self):
        from ci_fixtures import FIXTURES

        required = {"testdata/outline-simple.pdf", "testdata/outline-labels.pdf"}
        self.assertLessEqual(required, {name for name, _ in FIXTURES})

    def test_a_stale_sibling_cannot_satisfy_the_current_generator(self):
        from contextlib import redirect_stdout
        from io import StringIO
        from unittest.mock import patch
        import ci_fixtures

        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "write.py").write_text("from pathlib import Path\nPath('first').write_bytes(b'new')\n")
            (root / "second").write_bytes(b"old")
            wanted = [(name, ["write.py"]) for name in ("first", "second")]
            output = StringIO()
            with patch.object(ci_fixtures, "ROOT", root), redirect_stdout(output):
                self.assertEqual(ci_fixtures.generate(wanted), 1)
            self.assertIn("did not produce second", output.getvalue())
            self.assertEqual((root / "first").read_bytes(), b"new")
            self.assertFalse((root / "second").exists())


class MacReceiver(unittest.TestCase):
    def test_concurrent_checks_cannot_stage_over_the_active_run(self):
        with tempfile.TemporaryDirectory() as scratch:
            clone = Path(scratch) / "clone"
            (clone / ".git").mkdir(parents=True)
            runs = Path(str(clone) + "-check-run")

            def receive(run_id):
                return RECEIVE.format(clone=remote_path(str(clone)), run_id=run_id,
                                      sha="revision", capture="", marker="RESULT") + 'echo "RESULT 0"'

            with subprocess.Popen(["zsh", "-c", receive("first")], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE) as first:
                try:
                    bundle = runs / "first/tree.bundle"
                    deadline = time.monotonic() + 5
                    while not bundle.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(bundle.exists())
                    second = subprocess.run(["zsh", "-c", receive("second")], input=b"second",
                                            capture_output=True, timeout=5, check=True)
                    self.assertIn(b"RESULT 2", second.stdout)
                    self.assertFalse((runs / "second").exists())
                    output, _ = first.communicate(b"first", timeout=5)
                    self.assertIn(b"RESULT 0", output)
                    self.assertEqual(bundle.read_bytes(), b"first")
                finally:
                    if first.poll() is None:
                        first.kill()
            retry = subprocess.run(["zsh", "-c", receive("retry")], input=b"retry",
                                   capture_output=True, timeout=5, check=True)
            self.assertIn(b"RESULT 0", retry.stdout)
            self.assertEqual((runs / "retry/tree.bundle").read_bytes(), b"retry")


@unittest.skipIf(sys.platform == "win32", "the remote driver's process groups are POSIX")
class RemoteInputsAndTimeout(unittest.TestCase):
    def test_timeout_stops_a_delayed_descendant_before_a_retry(self):
        with tempfile.TemporaryDirectory() as scratch:
            room = Path(scratch)
            job = room / "job.sh"
            job.write_text('#!/bin/sh\n(trap "" INT; sleep 0.6; touch "$0.late") &\nwait\n')
            job.chmod(0o700)
            command = [sys.executable, "-c", VOICEOVER_DRIVER, scratch, "0.15"]
            subprocess.run(command, check=True, timeout=6)
            self.assertTrue((room / "timed-out").exists())
            job.write_text('#!/bin/sh\ntouch "$0.retry"\n')
            subprocess.run(command, check=True, timeout=3)
            self.assertTrue(Path(str(job) + ".retry").exists())
            time.sleep(0.65)
            self.assertFalse(Path(str(job) + ".late").exists())

    @unittest.skipUnless(sys.platform == "darwin", "uses the remote Mac's shell")
    def test_a_removed_phase_input_cannot_survive_a_second_run(self):
        with tempfile.TemporaryDirectory() as scratch:
            room = Path(scratch)
            clone, tree = room / "clone", room / "check"
            for folder in (clone,tree):
                (folder / "testdata").mkdir(parents=True)
            subprocess.run(["git","init","-q","-b","main",str(tree)],check=True)
            name = "testdata/example.pdf"
            script = REMOTE[REMOTE.index("phase_input={phase_input}"):REMOTE.index("# Rebuild the window")].format(phase_input=name)
            cmd = ["zsh","-c",'stop() { exit 2; }; clone=$1\n' + script, "stage",str(clone)]
            (clone / name).write_bytes(PDF)
            subprocess.run(cmd,cwd=tree,check=True,capture_output=True)
            self.assertEqual((tree/name).read_bytes(),PDF)
            (clone / name).unlink()
            subprocess.run(cmd,cwd=tree,check=True,capture_output=True)
            self.assertFalse((tree/name).exists())


if __name__ == "__main__":
    unittest.main()
