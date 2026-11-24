#!/usr/bin/env python3
"""Holds `served_old`, which decides whether an update check starts the old copy again.

A wrong yes hides a real failure behind a second start: a check that failed or
stalled would be retried until the run passed on a later answer. A wrong no
brings back the manual rerun the second start replaced. The lines are the ones
`update_check_line` in `src-tauri/src/commands/app.rs` writes.
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from update_log import UpdateAttempt, has_open_tab, offered_button, last_launch, served_old  # noqa: E402

CURRENT = "2026-11-15T16:21:42.719Z [update] launch check after 519 ms: current"
AVAILABLE = "2026-11-15T16:31:12.101Z [update] launch check after 610 ms: available 26.11.9"
FAILED = "2026-11-15T16:21:42.719Z [update] launch check after 30001 ms: failed: timed out"
MANUAL = "2026-11-15T16:22:02.000Z [update] asked check after 480 ms: current"


class ServedOld(unittest.TestCase):
    def answer(self, *lines: str) -> bool:
        with tempfile.TemporaryDirectory() as scratch:
            log = Path(scratch) / "tpdf.log"
            log.write_text("\n".join(["2026-11-15 started", *lines]) + "\n", encoding="utf-8")
            return served_old(log)

    def test_a_launch_answered_current_earns_a_second_start(self) -> None:
        self.assertTrue(self.answer(CURRENT))

    def test_a_failed_or_stalled_launch_check_is_a_finding(self) -> None:
        self.assertFalse(self.answer(FAILED))

    def test_an_offer_found_is_not_retried(self) -> None:
        self.assertFalse(self.answer(AVAILABLE))

    def test_the_last_launch_decides(self) -> None:
        self.assertFalse(self.answer(CURRENT, AVAILABLE))
        self.assertTrue(self.answer(FAILED, CURRENT))

    def test_a_check_the_reader_asked_for_is_not_a_launch(self) -> None:
        self.assertFalse(self.answer(MANUAL))

    def test_no_line_and_no_log_are_findings(self) -> None:
        self.assertFalse(self.answer())
        self.assertFalse(served_old(Path(tempfile.gettempdir()) / "no-such-tpdf.log"))


class FirstAttempt(unittest.TestCase):
    """The first launch's answer stays in the summary when a second start passes."""

    def answer(self, *lines: str) -> str | None:
        with tempfile.TemporaryDirectory() as scratch:
            log = Path(scratch) / "tpdf.log"
            log.write_text("\n".join(["2026-11-15 started", *lines]) + "\n", encoding="utf-8")
            return last_launch(log)

    def test_the_last_launch_answer_is_read_whole(self) -> None:
        self.assertEqual(self.answer(CURRENT), "current")
        self.assertEqual(self.answer(AVAILABLE), "available 26.11.9")
        self.assertEqual(self.answer(FAILED), "failed: timed out")
        self.assertEqual(self.answer(CURRENT, AVAILABLE), "available 26.11.9")
        self.assertIsNone(self.answer(MANUAL))
        self.assertIsNone(self.answer())

    def test_a_pass_on_the_second_start_is_reported_as_a_first_launch_miss(self) -> None:
        said = UpdateAttempt("current", True).summary()
        self.assertIn("MISSED", said)
        self.assertIn("current", said)

    def test_a_first_launch_that_was_offered_is_not_a_miss(self) -> None:
        said = UpdateAttempt("available 26.11.9").summary()
        self.assertNotIn("MISSED", said)
        self.assertIn("available 26.11.9", said)

    def test_failed_retry_reports_no_offer(self) -> None:
        self.assertIn("no offer observed", UpdateAttempt("current", False).summary())
        self.assertIn("offered on the second start", UpdateAttempt("current", True).summary())

    def test_only_a_live_tab_with_the_requested_full_path_counts(self) -> None:
        doc = Path("/docs/with space/file.pdf")
        self.assertFalse(has_open_tab([f"BUTTON {doc}", "TAB /elsewhere/file.pdf"], doc))
        self.assertTrue(has_open_tab([f"TAB {doc}"], doc))

    @unittest.skipUnless(sys.platform == "darwin", "uses macOS process file descriptors")
    def test_mac_requires_the_same_process_to_hold_the_file_and_enable_close(self) -> None:
        from update_check_mac import document_open
        with tempfile.TemporaryDirectory() as folder:
            pdf = Path(folder) / "paper.pdf"
            pdf.touch()
            with patch("update_check_mac.menu", return_value="enabled") as menu:
                self.assertFalse(document_open(os.getpid(), pdf))
                with pdf.open("rb"):
                    self.assertTrue(document_open(os.getpid(), pdf))
                    menu.assert_called_with(os.getpid(), "Close tab")
                    menu.return_value = "disabled"
                    self.assertFalse(document_open(os.getpid(), pdf))

    def test_offers_match_the_full_version_in_both_languages(self) -> None:
        for prefix in ("Update to", "Update auf"):
            self.assertEqual(offered_button([f"BUTTON {prefix} 1.0"], "1.0"), f"{prefix} 1.0")
            self.assertIsNone(offered_button([f"BUTTON {prefix} 1.01"], "1.0"))

    def test_no_line_is_named(self) -> None:
        self.assertIn("no launch check line", UpdateAttempt().summary())


if __name__ == "__main__":
    unittest.main()
