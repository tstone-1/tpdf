"""Failure controls for the source-build artifact boundary. Standard library only."""
from copy import deepcopy
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest

from build_pdfium import canonical_archive, check_upstream_report, complete_licenses, digest
from pdfium_verify import CONTROLS, LIMITATIONS, REGRESSIONS, verify


def observations():
    names = sorted(CONTROLS | REGRESSIONS | LIMITATIONS)
    manifest = {"cases": [{"file": name, "expected": "AB"} for name in names]}
    a, b = [65, [0.0, 1.0, 0.0, 1.0]], [66, [1.0, 2.0, 0.0, 1.0]]
    def record(wrong):
        return {name: {"text": "BA" if name in wrong else "AB",
                       "chars": deepcopy([b, a] if name in wrong else [a, b]),
                       "pixels_sha256": "a" * 64} for name in names}
    return manifest, record(REGRESSIONS | LIMITATIONS), record(LIMITATIONS)


class DifferentialTests(unittest.TestCase):
    def test_accepts_seven_fixes_and_two_unchanged_limitations(self):
        self.assertEqual(verify(*observations())["regressions_restored"], 7)

    def test_rejects_unpatched_candidate(self):
        manifest, before, _ = observations()
        with self.assertRaisesRegex(ValueError, "Candidate failed"):
            verify(manifest, before, deepcopy(before))

    def test_rejects_patched_control(self):
        manifest, _, after = observations()
        with self.assertRaisesRegex(ValueError, "Control failed"):
            verify(manifest, deepcopy(after), after)

    def test_rejects_missing_and_duplicate_fixtures(self):
        for alteration in ("missing", "duplicate"):
            with self.subTest(alteration=alteration):
                manifest, before, after = observations()
                if alteration == "missing":
                    removed = manifest["cases"].pop()["file"]
                    before.pop(removed)
                    after.pop(removed)
                else:
                    manifest["cases"][1] = manifest["cases"][0]
                with self.assertRaisesRegex(ValueError, "inventory"):
                    verify(manifest, before, after)

    def test_rejects_missing_observation(self):
        manifest, before, after = observations()
        after.pop("latin.pdf")
        with self.assertRaisesRegex(ValueError, "inventory"):
            verify(manifest, before, after)

    def test_rejects_pixels_geometry_and_duplicate_character_changes(self):
        for change in ("pixels", "box", "duplicate"):
            with self.subTest(change=change):
                manifest, before, after = observations()
                entry = after["arabic.pdf"]
                if change == "pixels":
                    entry["pixels_sha256"] = "b" * 64
                elif change == "box":
                    entry["chars"][0][1][0] += 1
                else:
                    entry["chars"].append(deepcopy(entry["chars"][0]))
                    entry["text"] += "A"
                with self.assertRaisesRegex(ValueError, "pixels changed|geometry changed"):
                    verify(manifest, before, after)

    def test_rejects_character_text_disagreement(self):
        manifest, before, after = observations()
        after["arabic.pdf"]["chars"].reverse()
        with self.assertRaisesRegex(ValueError, "indices disagree"):
            verify(manifest, before, after)

    def test_rejects_new_behavior_in_known_limitation(self):
        manifest, before, after = observations()
        # Index-only changes must also be held to the current behavior.
        after["latin-prefix.pdf"]["chars"].reverse()
        after["latin-prefix.pdf"]["text"] = "AB"
        with self.assertRaisesRegex(ValueError, "known limitation changed"):
            verify(manifest, before, after)


class ArtifactTests(unittest.TestCase):
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
