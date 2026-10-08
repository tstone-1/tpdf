"""Holds `tpdf_client.reports` against the committed CLI samples, in both directions.

Run from the repository root:

    python3 -m unittest discover -s api/python -p 'test_reports.py'

Forward: every sample in `src-tauri/testdata/cli/` is walked beside the
TypedDict that `REPORTS` names for it; a key the type does not declare, a
required key the JSON lacks, a null where the type allows none, or a value of
the wrong kind is a failure.

Backward: every required key of every TypedDict reachable from `REPORTS` must
have been seen in a sample, `REPORTS` must name exactly the sample files, and
the walk must have visited at least `MIN_OBJECTS` objects.
"""

from __future__ import annotations

import json
import types
import typing
import unittest
from pathlib import Path
from typing import Any, Literal, TypedDict

from tpdf_client import reports

SAMPLES = Path(__file__).resolve().parents[2] / "src-tauri" / "testdata" / "cli"

# In the same directory, and not reports.
NOT_REPORTS = {"wording", "reading", "regions"}

# Measured 2026-10-02: the seventeen samples hold 157 JSON objects.
MIN_OBJECTS = 150


class Walk:
    """Checks JSON values against annotations and records what it met."""

    def __init__(self) -> None:
        self.errors: list[str] = []
        self.objects = 0
        # (TypedDict name, key) for every key met in a sample object.
        self.seen: set[tuple[str, str]] = set()

    def check(self, value: Any, annotation: Any, where: str) -> None:
        problem = self._mismatch(value, annotation, where)
        if problem:
            self.errors.append(problem)

    def _mismatch(self, value: Any, annotation: Any, where: str) -> str | None:
        """None when `value` fits `annotation`; otherwise the first reason."""
        if annotation is Any:
            return None
        if annotation is None or annotation is type(None):
            return None if value is None else f"{where}: expected null, got {value!r}"
        origin = typing.get_origin(annotation)
        args = typing.get_args(annotation)
        if origin is typing.Union or origin is types.UnionType:
            return self._union(value, args, where)
        if value is None:
            return f"{where}: null, and the type does not allow it"
        if origin is Literal:
            if value in args and type(value) in {type(a) for a in args}:
                return None
            return f"{where}: {value!r} is not one of {args!r}"
        if origin is list:
            if not isinstance(value, list):
                return f"{where}: expected a list, got {type(value).__name__}"
            for index, item in enumerate(value):
                self.check(item, args[0], f"{where}[{index}]")
            return None
        if origin is dict:
            if not isinstance(value, dict):
                return f"{where}: expected an object, got {type(value).__name__}"
            for key, item in value.items():
                self.check(item, args[1], f"{where}.{key}")
            return None
        if typing.is_typeddict(annotation):
            return self._object(value, annotation, where)
        if annotation is bool:
            ok = isinstance(value, bool)
        elif annotation is int:
            ok = isinstance(value, int) and not isinstance(value, bool)
        elif annotation is float:
            ok = isinstance(value, (int, float)) and not isinstance(value, bool)
        elif annotation is str:
            ok = isinstance(value, str)
        else:
            return f"{where}: the walk does not understand the type {annotation!r}"
        if ok:
            return None
        return f"{where}: expected {annotation.__name__}, got {type(value).__name__} {value!r}"

    def _union(self, value: Any, members: tuple[Any, ...], where: str) -> str | None:
        """A value fits a union when it fits one member with no error below it."""
        reasons = []
        for member in members:
            trial = Walk()
            problem = trial._mismatch(value, member, where)
            if problem is None and not trial.errors:
                self.objects += trial.objects
                self.seen |= trial.seen
                return None
            reasons.extend([problem] if problem else trial.errors)
        if value is None:
            return f"{where}: null, and the type does not allow it"
        return f"{where}: fits no member of the union ({'; '.join(reasons)})"

    def _object(self, value: Any, shape: Any, where: str) -> str | None:
        if not isinstance(value, dict):
            return f"{where}: expected an object ({shape.__name__}), got {type(value).__name__}"
        self.objects += 1
        hints = typing.get_type_hints(shape)
        for key in sorted(value.keys() - hints.keys()):
            self.errors.append(f"{where}.{key}: not declared in {shape.__name__}")
        for key in sorted(shape.__required_keys__ - value.keys()):
            self.errors.append(f"{where}: {shape.__name__} requires {key!r}, and it is absent")
        for key, item in value.items():
            if key in hints:
                self.seen.add((shape.__name__, key))
                self.check(item, hints[key], f"{where}.{key}")
        return None


def reachable(annotation: Any, found: dict[str, Any]) -> None:
    """Every TypedDict an annotation can lead to, by name."""
    if typing.is_typeddict(annotation):
        if annotation.__name__ in found:
            return
        found[annotation.__name__] = annotation
        for hint in typing.get_type_hints(annotation).values():
            reachable(hint, found)
        return
    for arg in typing.get_args(annotation):
        if not isinstance(arg, (str, int, bool)):
            reachable(arg, found)


def sample_stems() -> set[str]:
    return {path.stem for path in SAMPLES.glob("*.json")}


def walk_samples() -> Walk:
    walk = Walk()
    for stem, shape in reports.REPORTS.items():
        value = json.loads((SAMPLES / f"{stem}.json").read_text(encoding="utf-8"))
        walk.check(value, shape, stem)
    return walk


class ReportsMatchSamples(unittest.TestCase):
    maxDiff = None

    def test_reports_names_exactly_the_sample_files(self) -> None:
        on_disk = sample_stems()
        self.assertEqual(NOT_REPORTS - on_disk, set(), "an ignored sample no longer exists")
        self.assertEqual(set(reports.REPORTS), on_disk - NOT_REPORTS)

    def test_every_sample_fits_its_type(self) -> None:
        walk = walk_samples()
        self.assertEqual(walk.errors, [])

    def test_the_walk_visited_the_samples(self) -> None:
        walk = walk_samples()
        self.assertGreaterEqual(walk.objects, MIN_OBJECTS)

    def test_every_required_key_is_in_a_sample(self) -> None:
        walk = walk_samples()
        shapes: dict[str, Any] = {}
        for shape in reports.REPORTS.values():
            reachable(shape, shapes)
        self.assertGreater(len(shapes), len(set(reports.REPORTS.values())))
        unseen = sorted(
            f"{name}.{key}"
            for name, shape in shapes.items()
            for key in shape.__required_keys__
            if (name, key) not in walk.seen
        )
        self.assertEqual(unseen, [])

    def test_the_verify_sample_holds_an_appendix_and_a_signature_without_one(self) -> None:
        # A key that may be null is checked as an object only where a sample
        # has it as one; `appendix` is null exactly when nothing was appended.
        report: reports.VerifyReport = json.loads(
            (SAMPLES / "verify.json").read_text(encoding="utf-8")
        )
        signatures = [s for f in report["files"] for s in f["signatures"]]
        with_one = [s["appendix"] for s in signatures if s["appendix"] is not None]
        self.assertEqual(
            [s["appended_bytes"] == 0 for s in signatures],
            [s["appendix"] is None for s in signatures],
        )
        self.assertTrue(with_one)
        self.assertLess(len(with_one), len(signatures))
        listed = [page for appendix in with_one for page in appendix["pages_listing"]]
        self.assertTrue(listed)
        for appendix in with_one:
            self.assertLessEqual(len(appendix["pages_listing"]), appendix["pages_touched"])

    def test_no_reachable_type_is_any(self) -> None:
        # `Any` fits every value, so a key typed with it is checked by nothing.
        shapes: dict[str, Any] = {}
        for shape in reports.REPORTS.values():
            reachable(shape, shapes)

        def has_any(annotation: Any) -> bool:
            return annotation is Any or any(has_any(a) for a in typing.get_args(annotation))

        loose = sorted(
            f"{name}.{key}"
            for name, shape in shapes.items()
            for key, hint in typing.get_type_hints(shape).items()
            if has_any(hint)
        )
        self.assertEqual(loose, [])

    def test_every_public_name_exists(self) -> None:
        missing = [name for name in reports.__all__ if not hasattr(reports, name)]
        self.assertEqual(missing, [])
        declared = {
            name
            for name, value in vars(reports).items()
            if not name.startswith("_")
            and (typing.is_typeddict(value) or typing.get_origin(value) is Literal)
        }
        self.assertEqual(declared - set(reports.__all__), set())


class _Inner(TypedDict):
    name: str
    size: float | None


class _OuterRequired(TypedDict):
    kind: Literal["a", "b"]
    count: int
    items: list[_Inner]


class _Outer(_OuterRequired, total=False):
    note: str


GOOD = {"kind": "a", "count": 2, "items": [{"name": "x", "size": None}, {"name": "y", "size": 1}]}


class TheWalkCanFail(unittest.TestCase):
    """Each defect the walk exists to find, on a shape built here."""

    def errors(self, **changes: Any) -> list[str]:
        walk = Walk()
        walk.check({**GOOD, **changes}, _Outer, "o")
        return walk.errors

    def test_the_control_passes(self) -> None:
        walk = Walk()
        walk.check(GOOD, _Outer, "o")
        self.assertEqual(walk.errors, [])
        self.assertEqual(walk.objects, 3)
        self.assertEqual(self.errors(note="optional and present"), [])

    def test_an_undeclared_key(self) -> None:
        self.assertEqual(len(self.errors(extra=1)), 1)

    def test_a_missing_required_key(self) -> None:
        walk = Walk()
        walk.check({"kind": "a", "items": []}, _Outer, "o")
        self.assertEqual(len(walk.errors), 1)
        self.assertIn("count", walk.errors[0])

    def test_a_null_the_type_does_not_allow(self) -> None:
        self.assertEqual(len(self.errors(count=None)), 1)
        self.assertEqual(len(self.errors(items=[{"name": None, "size": None}])), 1)

    def test_a_value_of_the_wrong_kind(self) -> None:
        self.assertEqual(len(self.errors(count="2")), 1)
        self.assertEqual(len(self.errors(count=True)), 1)
        self.assertEqual(len(self.errors(count=2.5)), 1)
        self.assertEqual(len(self.errors(note=3)), 1)
        self.assertEqual(len(self.errors(items={})), 1)
        self.assertEqual(len(self.errors(items=[{"name": "x", "size": "big"}])), 1)

    def test_a_string_outside_the_literal(self) -> None:
        self.assertEqual(len(self.errors(kind="c")), 1)

    def test_every_list_element_is_checked(self) -> None:
        items = [{"name": "x", "size": 1.0}, {"name": 5, "size": 1.0}]
        self.assertEqual(len(self.errors(items=items)), 1)


if __name__ == "__main__":
    unittest.main()
