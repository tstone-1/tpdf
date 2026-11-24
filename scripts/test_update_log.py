#!/usr/bin/env python3
"""Holds `served_old`, which decides whether an update check starts the old copy again.

A wrong yes hides a real failure behind a second start: a check that failed or
stalled would be retried until the run passed on a later answer. A wrong no
brings back the manual rerun the second start replaced. The lines are the ones
`update_check_line` in `src-tauri/src/commands/app.rs` writes.
"""

from __future__ import annotations

from contextlib import redirect_stdout
from io import StringIO
import os
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from update_log import UpdateAttempt, has_open_tab, offered_button, last_launch, relaunched_answer, served_old  # noqa: E402

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


class Relaunch(unittest.TestCase):
    def test_waits_for_the_new_process_to_finish_its_launch_check(self):
        since = "2026-11-15T16:30:00.000Z"
        with tempfile.TemporaryDirectory() as tmp:
            scratch, fallback = (Path(tmp) / name for name in ("scratch.log", "default.log"))
            scratch.write_text(CURRENT + "\n")
            for answer in ("current", "available 26.11.9", "failed: timed out"):
                other = AVAILABLE.replace("[update]", "[pid 41] [update]")
                fallback.write_text(other + "\n" + MANUAL)
                self.assertIsNone(relaunched_answer((scratch, fallback), since, 42, 0))
                completed = other.replace("pid 41", "pid 42").replace("available 26.11.9", answer)
                with patch("update_log.time.sleep", side_effect=lambda _: fallback.write_text(completed)):
                    self.assertEqual(relaunched_answer((scratch, fallback), since, 42, 1), answer)
            # Already published versions wrote the timestamp but no PID.
            fallback.write_text(AVAILABLE)
            self.assertEqual(relaunched_answer((scratch, fallback), since, 42, 0), "available 26.11.9")

    def test_mac_check_refuses_overlap_and_restores_after_a_mount_failure(self):
        import update_check_mac as mac
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            session = root / "config" / "session.json"
            session.parent.mkdir()
            session.write_bytes(b"reader tabs")
            dmg, pdf = root / "tpdf_26.10.1_aarch64.dmg", root / "input.pdf"
            dmg.touch()
            pdf.touch()
            def external(*args, **kwargs):
                if args[:2] == ("hdiutil", "attach"):
                    self.assertFalse(session.exists())
                    self.assertEqual(mac.main(), 2)
                    return subprocess.CompletedProcess(args, 1, "", "synthetic mount failure")
                return subprocess.CompletedProcess(args, 0, "", "")
            with patch.object(mac, "REAL_SESSION", session), patch.object(mac, "run", side_effect=external), \
                 patch.object(mac, "offered", return_value="26.10.2"), patch.object(sys, "platform", "darwin"), \
                 patch.object(sys, "argv", ["check", str(dmg), str(pdf)]):
                with redirect_stdout(StringIO()):
                    self.assertEqual(mac.main(), 1)
            self.assertEqual(session.read_bytes(), b"reader tabs")


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


class WindowsUpdateChecks(unittest.TestCase):
    def test_close_returns_without_killing_a_process_that_closed_its_window(self) -> None:
        import update_check as check
        with patch.object(check.ctypes, "WinDLL", create=True) as load, \
             patch.object(check, "window_titles", return_value=[(123, "tpdf v1")]), \
             patch.object(check, "processes", return_value=[]), patch.object(check.subprocess, "run") as kill:
            self.assertEqual(check.close([]), [])
            self.assertEqual(check.close([7]), [])
            post = load.return_value.PostMessageW
            post.assert_called_once()
            window, message, _, _ = post.call_args.args
            self.assertEqual((window.value, message), (123, check.WM_CLOSE))
            kill.assert_not_called()

    def test_session_is_restored_after_failure_and_a_missing_session_stays_missing(self) -> None:
        from update_check import isolated_session
        with tempfile.TemporaryDirectory() as folder:
            real = Path(folder) / "app" / "session.json"
            with isolated_session(real):
                real.write_bytes(b"test session")
            self.assertFalse(real.exists())
            real.write_bytes(b"reader session")
            with self.assertRaisesRegex(ValueError, "stopped"), isolated_session(real):
                self.assertFalse(real.exists())
                with self.assertRaisesRegex(RuntimeError, "unrestored"), isolated_session(real):
                    self.fail("overlapping run acquired the session")
                real.write_bytes(b"test session")
                raise ValueError("stopped")
            self.assertEqual(real.read_bytes(), b"reader session")

    def test_restore_failure_keeps_original_after_run_temp_directory_is_removed(self) -> None:
        from update_check import isolated_session
        with tempfile.TemporaryDirectory() as folder:
            real = Path(folder) / "app" / "session.json"
            real.parent.mkdir()
            real.write_bytes(b"reader session")
            with self.assertRaisesRegex(OSError, "original remains") as caught:
                with tempfile.TemporaryDirectory(dir=folder), isolated_session(real):
                    # A directory at the destination makes the atomic restore fail on every OS.
                    real.mkdir()
            backup = Path(str(caught.exception).split("original remains at ")[1])
            self.assertEqual(backup.read_bytes(), b"reader session")
            with self.assertRaisesRegex(RuntimeError, "unrestored"), isolated_session(real):
                self.fail("second run claimed the first run's backup")

    def test_edit_after_requires_both_turns_at_each_readback(self):
        from update_edit_check import Checks, EDIT_AFTER, check_rotation
        for step in (False, True):
            for angle in ("90", "180", "270"):
                checks = Checks()
                with redirect_stdout(StringIO()):
                    check_rotation(checks, angle, step, "edit-after")
                self.assertEqual(checks.failed == [EDIT_AFTER[int(step)]], angle == "270")

    def test_localized_edit_save_and_confirmation_controls(self) -> None:
        import update_edit_check as check
        with tempfile.TemporaryDirectory() as folder:
            pdf = Path(folder) / "demo.pdf"
            windows = [
                (check.ENGLISH, ("Pages", "Rotate page clockwise", "Save"), "Unsaved changes",
                 ("Discard and install", "Keep open"), "Install update"),
                (check.GERMAN, ("Seiten", "Seite im Uhrzeigersinn drehen", "Speichern"),
                 "Nicht gespeicherte Änderungen", ("Verwerfen und installieren", "Geöffnet lassen"),
                 "Update installieren"),
            ]
            for labels, controls, unsaved, choices, title in windows:
                with self.subTest(title=title):
                    pdf.write_bytes(b"original")

                    def elements(script, window, press="", expandable=False):
                        if press:
                            self.assertIn(press, controls)
                            if press == controls[2]:
                                os.utime(pdf, ns=(0, 0))
                            return [f"PRESSED|{press}|invoke"]
                        return [f"Tab|{pdf.name}", f"Text|{unsaved}", *(f"Button|{c}" for c in choices)]

                    with patch.object(check, "elements", side_effect=elements) as read, \
                         patch.object(check.time, "sleep"), patch.object(check, "dialog", return_value=0) as dialog, \
                         patch.object(check.ctypes, "WinDLL", create=True) as win:
                        self.assertTrue(check.edit(pdf, 1, pdf.name, labels)[0])
                        self.assertTrue(check.save(pdf, 1, pdf, labels)[0])
                        pressed = [call.kwargs["press"] for call in read.call_args_list if "press" in call.kwargs]
                        self.assertEqual(tuple(pressed), controls)
                        for button, number in [(choices[1], 1001), (choices[0], 1000)]:
                            self.assertTrue(check.answer(check.Checks(), pdf, 1, 2, button, labels))
                            dialog.assert_called_with(1, title, 0.5)
                            self.assertEqual(win.return_value.PostMessageW.call_args.args[2], number)


if __name__ == "__main__":
    unittest.main()
