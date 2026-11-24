"""Controls for the remote checkout and for observations used by platform checks."""
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import fetch_pdfium
import installed_check
import win_modules
import run_on_mac
import run_on_windows as windows
import mark_check
import nvda_check
import session_check
import stray


class Observations(unittest.TestCase):
    def test_uninstall_preserves_recovery_files_until_the_installation_is_gone(self):
        for registered, payload, code in [(True, False, 7), (False, True, 0), (False, False, 0)]:
            with self.subTest(registered=registered, payload=payload), tempfile.TemporaryDirectory() as temp:
                folder = Path(temp) / "install"
                folder.mkdir()
                remover = folder / "uninstall.exe"
                remover.touch()
                if payload:
                    (folder / "tpdf.exe").touch()
                registry = {installed_check.UNINSTALL_KEY: "installed" if registered else None}
                before = {installed_check.APP_KEY: None, installed_check.PDF_CLASS: None}
                checks = installed_check.Checks()
                with patch.object(installed_check, "bounded", return_value=code), \
                     patch.object(installed_check, "registry", side_effect=registry.get), \
                     patch.object(installed_check, "restore_app_key"), redirect_stdout(io.StringIO()) as output:
                    installed_check.uninstall(checks, folder, before, run_it=True)
                self.assertEqual(remover.is_file(), registered or payload)
                self.assertEqual("the uninstaller succeeds" in checks.failed, bool(code))
                if code:
                    self.assertIn(f"exit {code}", output.getvalue())

    def test_engine_mappings_must_all_name_the_installed_file(self):
        engine = r"C:\Program Files\tpdf\pdfium\pdfium.dll"
        correct = win_modules.mapped_engines(["\\\\?\\" + engine.upper(), r"C:\pdfium-old\app.exe"])
        other = win_modules.mapped_engines([engine.replace("pdfium\\", "pdfium-old\\")])
        self.assertTrue(installed_check.installed_engine({1: correct}, engine))
        self.assertFalse(installed_check.installed_engine({1: correct | other}, engine))
        self.assertFalse(installed_check.installed_engine({1: correct, 2: other}, engine))
        self.assertFalse(installed_check.installed_engine({}, engine))
        with patch.object(win_modules, "modules_of", return_value=[r"C:\pdfium-old\tpdf.exe"]):
            self.assertEqual(win_modules.maps_parser(1), (False, 1))

    def test_failed_acl_inspection_and_reset_are_reported(self):
        folder = Path("fixture")
        denied = subprocess.CompletedProcess([], 5, "", "access denied")
        explicit = subprocess.CompletedProcess([], 0, "explicit ACE", "")
        for replies in ([denied], [explicit, denied]):
            with patch.object(fetch_pdfium.os, "name", "nt"), patch.object(fetch_pdfium, "installed_complete", return_value=True), \
                 patch.object(fetch_pdfium.subprocess, "run", side_effect=replies), self.assertRaisesRegex(SystemExit, "access denied"):
                fetch_pdfium.install("win-x64", folder, False)

    def test_windows_stray_query_and_kill_failures_stay_visible(self):
        with patch.object(stray.sys, "platform", "win32"), redirect_stdout(io.StringIO()):
            with patch.object(stray.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "CIM")) as query:
                self.assertEqual(stray.main(["test-app"]), 2)
                self.assertTrue(query.call_args.kwargs["check"])
            with patch.object(stray.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "")):
                self.assertEqual(stray.main(["test-app"]), 0)
            for code, expected in [(0, True), (3, False), (1, None)]:
                with patch.object(stray.subprocess, "run", return_value=subprocess.CompletedProcess([], code)):
                    if expected is None:
                        with self.assertRaises(subprocess.CalledProcessError):
                            stray._end(99, "test-app")
                    else:
                        self.assertIs(stray._end(99, "test-app"), expected)

    def test_nvda_walk_uses_an_isolated_session(self):
        with tempfile.TemporaryDirectory() as room:
            scratch = Path(room)
            original = scratch / "original"
            original.write_text("reader data")
            with patch.dict(os.environ, TPDF_SESSION_FILE=str(original)), patch.object(nvda_check, "running", return_value=True), \
                 patch.object(nvda_check.time, "sleep"), patch.object(nvda_check.subprocess, "run"), \
                 patch.object(nvda_check.subprocess, "Popen", side_effect=InterruptedError) as launch:
                with self.assertRaises(InterruptedError):
                    nvda_check.walk(Path("tpdf"), scratch / "sample.pdf", scratch)
                env = launch.call_args.kwargs["env"]
                Path(env["TPDF_SESSION_FILE"]).write_text("test session")
                self.assertEqual(Path(env["TPDF_SESSION_FILE"]), scratch / "session.json")
                self.assertEqual(original.read_text(), "reader data")

    def test_clipboard_filename_is_an_applescript_argument(self):
        icon = Path('folder "with quotes"/icon.png')
        with patch.object(mark_check.sys, "platform", "darwin"), patch.object(mark_check, "ICON", icon), \
             patch.object(mark_check.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as called:
            self.assertTrue(mark_check.seed_clipboard())
        args = called.call_args.args[0]
        self.assertNotIn(str(icon), args[2])
        self.assertEqual(args[3:], [str(icon), mark_check.PASTED_TEXT])

    def test_saved_zoom_is_checked_independently_of_mode(self):
        with tempfile.TemporaryDirectory() as room, redirect_stdout(io.StringIO()):
            file = Path(room) / "session"
            pdf = str(Path(room).resolve() / "test.pdf")
            for zoom, good in [(99, False), (1.25, True)]:
                place = dict(path=pdf, page=session_check.EXPECTED_PAGE, turns=session_check.EXPECTED_TURNS,
                             fit="none", zoom=zoom, sidebar=True)
                file.write_text(json.dumps({"places": [place]}))
                self.assertEqual(session_check.check_recorded_file(file, pdf), good)


def git(folder, *args):
    return subprocess.check_output(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                                   "-c", "core.hooksPath=/dev/null", "-C", folder, *args], text=True).strip()


class RemoteCheckout(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.clone = self.root / "clone"
        self.tree = self.root / "clone-check"
        self.run = self.root / "clone-check-run" / windows.RUN_ID
        self.run.mkdir(parents=True)
        self.clone.mkdir()
        git(self.clone, "init", "-q", "-b", "main")
        (self.clone / "sample").write_text("committed")
        git(self.clone, "add", "sample")
        git(self.clone, "commit", "-qm", "base")
        self.sha = git(self.clone, "rev-parse", "HEAD")
        (self.clone / "sample").write_text("pending")

    def script(self, extra=""):
        setup = windows.checkout(str(self.clone), self.sha).split("$lock =")[0]
        return f"Set-Alias python '{sys.executable}'\n" + setup + extra

    def powershell(self, script):
        path = self.root / "probe.ps1"
        path.write_text(script, encoding="utf-8-sig")
        done = subprocess.run([shutil.which("pwsh") or "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                               "-File", path], capture_output=True, text=True, timeout=30)
        return done.stdout + done.stderr

    def check_cases(self, run):
        self.assertEqual(run(), 0)
        self.tree.rename(self.root / "moved")
        self.assertEqual(run(), 2, "stale Git registration")
        git(self.clone, "clone", "-q", str(self.clone), str(self.tree))
        (self.tree / "sample").write_text("another pending edit")
        self.assertEqual(run(), 2, "unrelated clone")
        self.assertEqual((self.tree / "sample").read_text(), "another pending edit")
        self.assertEqual((self.clone / "sample").read_text(), "pending")
        self.assertEqual(git(self.clone, "branch", "--show-current"), "main")

    @unittest.skipUnless(shutil.which("pwsh") or os.name == "nt", "PowerShell required")
    def test_windows_checkout_refuses_stale_and_unrelated_trees(self):
        self.check_cases(lambda: windows.verdict(self.powershell(self.script(f'"{windows.MARKER} 0"'))))

    @unittest.skipUnless(sys.platform == "darwin", "zsh receiver on Mac")
    def test_interrupted_mac_check_restores_visible_apps(self):
        script = run_on_mac.REMOTE.split('cat >"$run/hide.applescript"', 1)[1]
        script = ('cat >"$run/hide.applescript"' + script.split("printf '#!/bin/zsh", 1)[0]).format()
        # Simulate the OS recording and changing visibility. No desktop is touched.
        setup = r'''run=$1; owned_app="$run/staged"; mkdir "$owned_app"
osascript() {
  if [[ $1 == *hide.applescript ]]; then print -l 'App One' 'App Two'
  elif [[ $1 == *show.applescript ]]; then print -r -- "shown $2" >> "$run/events"
  else print -r -- "hidden ${argv[-1]}" >> "$run/events"; fi
}
'''
        done = subprocess.run(["zsh", "-c", setup + script + 'kill -TERM $$', "check", self.run],
                              capture_output=True, timeout=8)
        self.assertEqual(done.returncode, 2)
        self.assertEqual((self.run / "events").read_text().splitlines(),
                         ["hidden App One", "hidden App Two", "shown App One", "shown App Two"])
        self.assertFalse((self.run / "staged").exists())

    @unittest.skipUnless(sys.platform == "darwin", "zsh receiver on Mac")
    def test_interrupt_after_bootstrap_unloads_only_the_owned_job(self):
        args = SimpleNamespace(only=[], pictures=False, phase=None, voiceover=False,
                               clone="repo", timeout=15, wrap="")
        receiver = run_on_mac.remote(args, "abc", "", "test-run")
        block = receiver[receiver.index('hidden=""'):receiver.index('if [[ -f "$run/timed-out"')]
        setup = r'''run=$1; tree=$run; owned_app="$run/app"; app=$owned_app
job="com.tpdf.windowchecks.test-run"
mkdir "$app"
osascript() { return 0; }
launchctl() { print -r -- "$*" >> "$run/events"; }
sleep() { kill -TERM $$; }
stop() { exit 3; }
'''
        done = subprocess.run(["zsh", "-c", setup + block, "check", self.run],
                              capture_output=True, timeout=8)
        self.assertEqual(done.returncode, 2, done.stderr)
        domain = f"gui/{os.getuid()}"
        self.assertEqual((self.run / "events").read_text().splitlines(), [
            f"bootstrap {domain} {self.run}/job.plist", f"bootout {domain}/com.tpdf.windowchecks.test-run"])
        self.assertFalse((self.run / "app").exists())

    @unittest.skipUnless(sys.platform == "darwin", "zsh receiver on Mac")
    def test_mac_checkout_refuses_stale_and_unrelated_trees(self):
        start = run_on_mac.REMOTE.index('[ -d "$tree" ] || git worktree')
        end = run_on_mac.REMOTE.index('echo "[OK] $tree')
        guard = run_on_mac.REMOTE[start:end].format(verify_worktree=(windows.ROOT / "scripts/check_worktree.py").read_text())
        script = 'clone=$1; tree=$2; sha=$3; cd "$clone" || exit 2; stop() { exit 2; }\n' + guard
        self.check_cases(lambda: subprocess.run(["zsh", "-c", script, "run", self.clone, self.tree, self.sha],
                                                capture_output=True, timeout=10).returncode)

    @unittest.skipUnless(shutil.which("pwsh") or os.name == "nt", "PowerShell required")
    def test_a_second_windows_check_cannot_acquire_the_first_ones_lock(self):
        lock = self.run.parent / "worktree.lock"
        # A separate process owns the actual FileShare.None handle through the second checkout.
        owner = self.root / "owner.ps1"
        owner.write_text(f"$lock = [IO.File]::Open('{lock}', 'OpenOrCreate', 'ReadWrite', 'None')\n"
                         "[Console]::WriteLine('locked')\n$null = [Console]::ReadLine()\n", encoding="utf-8-sig")
        with subprocess.Popen([shutil.which("pwsh") or "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                               "-File", owner], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as held:
            try:
                self.assertEqual(held.stdout.readline().strip(), "locked")
                self.assertIn("cannot lock", self.powershell(self.script()))
                self.assertFalse(self.tree.exists())
            finally:
                held.communicate("\n", timeout=5)
        self.assertEqual(windows.verdict(self.powershell(self.script(f'"{windows.MARKER} 1"'))), 1)
        self.assertEqual(windows.verdict("TPDF-REMOTE-EXIT another-run 0"), 2)

    @unittest.skipUnless(shutil.which("pwsh") or os.name == "nt", "PowerShell required")
    def test_generated_and_borrowed_fixtures_on_reuse(self):
        inputs = self.clone / "testdata"
        inputs.mkdir()
        self.tree.mkdir()
        git(self.tree, "init", "-q", "-b", "main")
        fixtures = self.tree / "testdata"
        fixtures.mkdir()
        (self.tree / ".gitignore").write_text("testdata/*\n")
        scripts = self.tree / "scripts"
        scripts.mkdir()
        (scripts / "ci_fixtures.py").write_text("from pathlib import Path\nPath('testdata/local.pdf').write_bytes(b'generated')\n")
        (inputs / "local.pdf").write_bytes(b"external")
        (fixtures / "withdrawn.pdf").write_bytes(b"legacy")
        prepare = f"""Set-Alias python '{sys.executable}'
function Stop-Run($why, $code) {{ throw $why }}
$clone = '{self.clone}'; $run = '{self.run}'; $runs = '{self.run.parent}'
Set-Location -LiteralPath '{self.tree}'
""" + windows.FIXTURES
        for version in [b"old", b"updated", None]:
            for name in ["borrowed.pdf", "borrowed.json"]:
                if version:
                    (inputs / name).write_bytes(version)
                else:
                    (inputs / name).unlink()
            self.assertIn("[OK] fixtures:", self.powershell(prepare))
            for name in ["borrowed.pdf", "borrowed.json"]:
                if version:
                    self.assertEqual((fixtures / name).read_bytes(), version)
                else:
                    self.assertFalse((fixtures / name).exists())
            self.assertEqual((fixtures / "local.pdf").read_bytes(), b"generated")
            self.assertFalse((fixtures / "withdrawn.pdf").exists())


if __name__ == "__main__":
    unittest.main()
