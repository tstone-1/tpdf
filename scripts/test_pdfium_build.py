"""Failure controls for the source-build artifact boundary. Standard library only."""
from copy import deepcopy
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest

from build_pdfium import canonical_archive, check_upstream_report, complete_licenses, digest, windows_bash
from pdfium_verify import LIMITATIONS, ORDINARY, verify


def observation():
    """A manifest and an observation that verify: ordinary cases extract their
    authored text, each limitation extracts exactly its pinned wrong text."""
    names = sorted(ORDINARY | set(LIMITATIONS))
    manifest = {"cases": [{"file": name, "expected": "AB"} for name in names]}

    def seen(text):
        chars = [[ord(ch), [float(i), float(i + 1), 0.0, 1.0]] for i, ch in enumerate(text)]
        return {"text": text, "chars": chars, "pixels_sha256": "a" * 64}
    return manifest, {name: seen(LIMITATIONS.get(name, "AB")) for name in names}


class ObservationTests(unittest.TestCase):
    def test_accepts_nine_correct_and_two_pinned_limitations(self):
        self.assertEqual(verify(*observation())["ordinary_correct"], 9)

    def test_rejects_an_ordinary_case_with_wrong_text(self):
        manifest, seen = observation()
        seen["arabic.pdf"]["text"] = "BA"
        seen["arabic.pdf"]["chars"].reverse()
        with self.assertRaisesRegex(ValueError, "differs from the authored text"):
            verify(manifest, seen)

    def test_rejects_missing_and_duplicate_fixtures(self):
        for alteration in ("missing", "duplicate"):
            with self.subTest(alteration=alteration):
                manifest, seen = observation()
                if alteration == "missing":
                    seen.pop(manifest["cases"].pop()["file"])
                else:
                    manifest["cases"][1] = manifest["cases"][0]
                with self.assertRaisesRegex(ValueError, "inventory"):
                    verify(manifest, seen)

    def test_rejects_missing_observation(self):
        manifest, seen = observation()
        seen.pop("latin.pdf")
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(manifest, seen)

    def test_rejects_empty_observation(self):
        manifest, seen = observation()
        seen["latin.pdf"] = {"text": "", "chars": [], "pixels_sha256": "a" * 64}
        with self.assertRaisesRegex(ValueError, "empty observation"):
            verify(manifest, seen)

    def test_rejects_character_text_disagreement(self):
        manifest, seen = observation()
        seen["arabic.pdf"]["chars"].reverse()
        with self.assertRaisesRegex(ValueError, "indices disagree"):
            verify(manifest, seen)

    def test_rejects_a_limitation_extracting_different_wrong_text(self):
        manifest, seen = observation()
        # The patched 8044 build's order for this case: Latin in place, Hebrew reversed.
        text = "Hello \u05d4\u05d9\u05d5\u05dd \u05e2\u05d5\u05dc\u05dd \u05e9\u05dc\u05d5\u05dd"
        seen["latin-prefix.pdf"]["text"] = text
        seen["latin-prefix.pdf"]["chars"] = [[ord(c), [0.0, 1.0, 0.0, 1.0]] for c in text]
        with self.assertRaisesRegex(ValueError, "known limitation changed"):
            verify(manifest, seen)

    def test_rejects_a_limitation_that_is_now_correct(self):
        manifest, seen = observation()
        seen["latin-suffix.pdf"]["text"] = "AB"
        seen["latin-suffix.pdf"]["chars"] = [[65, [0.0, 1.0, 0.0, 1.0]], [66, [1.0, 2.0, 0.0, 1.0]]]
        with self.assertRaisesRegex(ValueError, "now extracts correctly"):
            verify(manifest, seen)


class ArtifactTests(unittest.TestCase):
    def test_windows_bash_belongs_to_git_installation(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            shell = root / "Git/bin/bash.exe"
            shell.parent.mkdir(parents=True)
            shell.touch()
            for relative in ("cmd/git.exe", "bin/git.exe", "mingw64/bin/git.exe"):
                self.assertEqual(windows_bash(root / "Git" / relative), shell.resolve())
            shell.unlink()
            other = root / "Windows/System32/bash.exe"
            other.parent.mkdir(parents=True)
            other.touch()
            with self.assertRaisesRegex(ValueError, "Git Bash is absent"):
                windows_bash(root / "Git/cmd/git.exe")

    def test_archive_is_identical_across_mtime_and_permission_changes(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            stage = root / "stage"
            (stage / "lib").mkdir(parents=True)
            library = stage / "lib/libpdfium.dylib"
            library.write_bytes(b"synthetic-library")
            canonical_archive(stage, root / "first.tgz")
            os.utime(library, (123, 456))
            library.chmod(0o755)
            canonical_archive(stage, root / "second.tgz")
            self.assertEqual(digest(root / "first.tgz"), digest(root / "second.tgz"))
            with tarfile.open(root / "first.tgz") as archive:
                self.assertEqual(archive.getnames(), ["lib/libpdfium.dylib"])
                self.assertEqual(archive.extractfile("lib/libpdfium.dylib").read(), b"synthetic-library")

    def test_no_skipped_or_missing_upstream_tests(self):
        valid = {"tests": 2, "failures": 0, "disabled": 0, "errors": 0,
                 "testsuites": [{"testsuite": [
                     {"status": "RUN", "result": "COMPLETED"},
                     {"status": "RUN", "result": "COMPLETED"}]}]}
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "tests.json"
            path.write_text(json.dumps(valid))
            check_upstream_report(path, 2)
            variants = []
            for key in ("tests", "failures", "disabled", "errors"):
                bad = deepcopy(valid)
                bad[key] += 1
                variants.append(bad)
            bad = deepcopy(valid)
            bad["testsuites"][0]["testsuite"][0]["result"] = "SKIPPED"
            variants.append(bad)
            bad = deepcopy(valid)
            bad["testsuites"] = []
            variants.append(bad)
            for bad in variants:
                path.write_text(json.dumps(bad))
                with self.assertRaises(ValueError):
                    check_upstream_report(path, 2)

    def test_unknown_library_blocks_packaging(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            licenses = root / "licenses"
            licenses.mkdir()
            (licenses / "pdfium.txt").write_text("Synthetic license fixture")
            complete_licenses(root, root, "")
            with self.assertRaisesRegex(ValueError, "incomplete"):
                complete_licenses(root, root, "WARNING: unknow library unlisted")
            (licenses / "pdfium.txt").write_text("")
            with self.assertRaisesRegex(ValueError, "empty"):
                complete_licenses(root, root, "")

    def test_supplemented_notice_is_copied_from_source(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "licenses").mkdir()
            (root / "licenses/pdfium.txt").write_text("Synthetic PDFium license fixture")
            source = root / "third_party/dragonbox/src/LICENSE-Boost"
            source.parent.mkdir(parents=True)
            source.write_text("Synthetic Dragonbox license fixture")
            complete_licenses(root, root, "WARNING: unknow library dragonbox")
            self.assertEqual((root / "licenses/dragonbox.txt").read_bytes(), source.read_bytes())


if __name__ == "__main__":
    unittest.main()
