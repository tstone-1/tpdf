#!/usr/bin/env python3
"""Holds `served_old`, which decides whether an update check starts the old copy again.

A wrong yes hides a real failure behind a second start: a check that failed or
stalled would be retried until the run passed on a later answer. A wrong no
brings back the manual rerun the second start replaced. The lines are the ones
`update_check_line` in `src-tauri/src/commands/app.rs` writes.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from update_log import served_old  # noqa: E402

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


if __name__ == "__main__":
    unittest.main()
