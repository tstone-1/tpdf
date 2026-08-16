#!/usr/bin/env python3
"""Runs `viewer_check.py` over the window corpora and prints BUILD.md's table.

**This file is the corpus list.** Before it existed the list was whatever a
person typed into a throwaway shell loop, and on 2026-08-16 that put
`links-rotated.pdf` into a sweep --- a fixture BUILD.md already described as a
*separate file* precisely because it mixes page sizes and reddens two of the
harness's rotation checks. Eight red checks, three of them chased, none of them
a defect in the viewer. A list with no home is a list nobody can be wrong
about.

So every `testdata/*.pdf` has to be accounted for here: either it is a window
corpus with a stated purpose, or it is excluded with a stated reason. A fixture
matching neither is an error rather than a silent omission --- the same shape as
`scripts/check_trap_index.py`, which diffs two sets rather than counting one.

Two invariants this asserts that BUILD.md could only state in prose:

* **Every corpus reports the same check names.** What differs between fixtures
  is how many are `[SKIP]` and why. A name that stops being printed rather than
  skipping is the bug the whole arrangement exists to catch, and it passes every
  per-fixture count comparison, because a missing name and a missing skip look
  identical in a total.
* **Zero failures.** Reported per fixture, with the failing lines echoed, so a
  red run says which check on which document rather than which count moved.

Usage:

    scripts/viewer_sweep.py --list                # the corpora, and what each is for
    scripts/viewer_sweep.py <app-exe>             # run them all, print the table
    scripts/viewer_sweep.py <app-exe> --only links,mixed

The app must be a **bundle** executable
(`src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`), not a
raw `cargo build` binary --- see the trap of that name: a raw binary embeds no
frontend and runs no webview content at all, so every check would report
nothing having failed.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TESTDATA = ROOT / "testdata"

# The window corpora, in the order the table lists them. The second field is
# what the fixture is *for*, which is the column that stops the table from
# being a list of numbers: a corpus that cannot say what it uniquely exercises
# is a corpus nobody can decide to delete.
WINDOW_CORPORA: list[tuple[str, str]] = [
    ("text-heavy", "the dense case, and search across 775 pages"),
    ("outline-simple", "the only fixture with an ordinary outline"),
    ("outline-hostile", "the only one with a `/Launch` entry to refuse"),
    ("vector-heavy", "one page, no extractable text, and no white paper to invert"),
    (
        "vector-multi",
        "twelve A0 pages: the only one where a thumbnail is slow enough to "
        "collide with the viewer",
    ),
    ("rotated-90", "every page at `/Rotate 90`, which nothing else in the corpus has"),
    ("columns", "the only one whose content-stream order is not its reading order"),
    (
        "tagged",
        "the only one carrying a `/StructTreeRoot`, and the only two-page one",
    ),
    (
        "multilingual",
        "the only one whose text is not Latin: CJK with no word separators, "
        "Arabic right-to-left, a decomposed accent, and a code point above the BMP",
    ),
    (
        "encodings",
        "the only one whose character mappings are absent, broken or predefined "
        "--- and the only fixture that reaches the replacement-character path at all",
    ),
    (
        "mixed",
        "the only one whose pages are not all the same size, and the only one "
        "that exercises the three layout checks at all",
    ),
    (
        "comments",
        "the only one carrying annotations: notes, a reply, a highlight, three "
        "text-string encodings, an indirect `/Annots` array and 1,200 marks on "
        "one page --- the only corpus where all eight comment checks run",
    ),
    (
        "links",
        "the only one with link annotations, and the only one whose outline is "
        "deliberately not in page order --- which is what let it catch a "
        "destination landing on the page before the one it named",
    ),
    (
        "links-cropped",
        "the only one whose `/CropBox` is not its `/MediaBox`, so a rectangle "
        "placed in media space lands visibly wrong",
    ),
]

# Everything else in `testdata/`, with the reason it is not run in a window.
# Patterns rather than names where a whole family shares one reason; each is
# required to match something, so a family that goes away is reported instead
# of quietly excusing nothing.
NOT_WINDOW: list[tuple[str, str]] = [
    (
        "links-rotated",
        "mixes page sizes, which reddens two rotation checks that derive what "
        "they expect from page 1's aspect ratio --- the same split, for the same "
        "reason, as comments-rotated",
    ),
    (
        "comments-rotated",
        "same split as links-rotated: the mapping it tests is a property of the "
        "scan, which comments-probe reads directly",
    ),
    ("hostile-*", "sanitation fixtures, read by sanitize-* rather than opened"),
    ("incr-*", "incremental-save fixtures for the document model, not the viewer"),
    (
        "text-base14",
        "a backend-probe fixture: font coverage, measured through the worker",
    ),
    ("text-cid", "a backend-probe fixture: a Type0 font with a /ToUnicode"),
    ("text-truetype", "a backend-probe fixture: an embedded TrueType subset"),
    ("text-marked", "a text-probe fixture: marked content and text objects"),
    ("rotated", "the uniform rotation fixture the probes read; rotated-90 is its window twin"),
    ("form", "an AcroForm fixture for the document model, with no viewer path yet"),
]

# The names come from the run's own JSON roll, never from the printed column.
# That column is `LABEL name.padEnd(46) detail`, so any name longer than 46
# characters is separated from its detail by a single space and cannot be told
# from the single spaces inside the name. The first version of this file used a
# `\s{2,}` split, matched 175 of 189 lines, deduplicated the truncations, and
# then reported two corpora agreeing about a 137-name set --- a check that
# passed while measuring the wrong thing, in the tool written to stop exactly
# that.
NAMES_JSON = re.compile(r"^CHECK-NAMES-JSON (\[.*\])$", re.MULTILINE)
# The "not applicable" clause is omitted when nothing skipped, so it is
# optional here --- written as required, a corpus with no skips would fail
# to match and silently lose the roll-versus-summary cross-check below.
SUMMARY = re.compile(r"(\d+)/(\d+) checks passed(?:, (\d+) not applicable)?")


def classify() -> tuple[list[str], list[str]]:
    """Every fixture on disk, split into corpora and excluded, or raises."""
    on_disk = sorted(p.stem for p in TESTDATA.glob("*.pdf"))
    corpora = [stem for stem, _ in WINDOW_CORPORA]

    missing = [stem for stem in corpora if stem not in on_disk]
    if missing:
        raise SystemExit(
            f"[FAIL] {len(missing)} corpus fixture(s) are not in testdata/: "
            + ", ".join(missing)
            + "\n       Generate them first; a sweep that silently skips a corpus "
            "reports a clean run over less than it claims."
        )

    excluded: list[str] = []
    unclassified: list[str] = []
    for stem in on_disk:
        if stem in corpora:
            continue
        if any(fnmatch.fnmatch(stem, pattern) for pattern, _ in NOT_WINDOW):
            excluded.append(stem)
        else:
            unclassified.append(stem)

    if unclassified:
        raise SystemExit(
            f"[FAIL] {len(unclassified)} fixture(s) are neither a window corpus nor "
            "excluded: " + ", ".join(unclassified) + "\n"
            "       Add each to WINDOW_CORPORA with what it is for, or to "
            "NOT_WINDOW with why not.\n"
            "       This is the check that stopped a probe fixture being swept as "
            "a corpus."
        )

    # An exclusion naming nothing is how an exclusion list rots into a blanket
    # permission --- the same rule the webview-sink gate applies to its markers.
    for pattern, _ in NOT_WINDOW:
        if not any(fnmatch.fnmatch(stem, pattern) for stem in on_disk):
            print(f"[WARN] no fixture matches the exclusion {pattern!r}")

    return corpora, excluded


def run_one(app: Path, stem: str, timeout: int) -> dict[str, object]:
    """One fixture through the harness, returning what the table needs."""
    subprocess.run(
        ["pkill", "-f", "tpdf.app/Contents/MacOS/tpdf"],
        check=False,
        capture_output=True,
    )
    # A leftover window occludes the next one, WebKit suspends an occluded
    # page, and the run then produces nothing while using no CPU. TPDF_RAISE
    # covers the other half, a window with nowhere visible to go.
    result = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "viewer_check.py"),
            str(app),
            str(TESTDATA / f"{stem}.pdf"),
            "--timeout",
            str(timeout),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        env={**os.environ, "TPDF_RAISE": "1"},
    )
    out = result.stdout + result.stderr
    summary = SUMMARY.search(out)

    roll = NAMES_JSON.search(out)
    if roll is None:
        # Refused rather than fallen back on. A bundle predating the roll is an
        # old bundle, and comparing name sets recovered by guesswork is how the
        # first version of this reported agreement about a set that was wrong.
        raise SystemExit(
            f"[FAIL] {stem}: the run printed no CHECK-NAMES-JSON line.\n"
            "       The bundle predates it --- rebuild with `npm run tauri build`."
        )
    names = json.loads(roll.group(1))
    if summary is None:
        # The roll is printed just before the summary, so a roll with no summary
        # after it is a run that died in between. Refused rather than reported
        # with a -1 in the table.
        raise SystemExit(
            f"[FAIL] {stem}: the run printed its check names and then no summary."
        )
    ran = int(summary.group(2))
    skipped = int(summary.group(3) or 0)
    if len(names) != ran + skipped:
        raise SystemExit(
            f"[FAIL] {stem}: the roll lists {len(names)} names and the summary "
            f"counts {ran + skipped} ({ran} ran, {skipped} skipped)."
        )

    fails = [line for line in out.splitlines() if line.startswith("[FAIL]")]
    return {
        "stem": stem,
        "exit": result.returncode,
        "names": names,
        "ran": ran,
        "skipped": skipped,
        "fails": fails,
        "output": out,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", nargs="?", help="the bundle executable to run")
    parser.add_argument("--list", action="store_true", help="print the corpora and exit")
    parser.add_argument("--only", help="comma-separated stems, for one fixture at a time")
    parser.add_argument("--timeout", type=int, default=420)
    args = parser.parse_args()

    corpora, excluded = classify()

    if args.list:
        print(f"=== {len(corpora)} window corpora")
        for stem, why in WINDOW_CORPORA:
            print(f"  {stem:<16} {why}")
        print(f"\n=== {len(excluded)} fixtures not run in a window")
        for stem in excluded:
            why = next(
                reason
                for pattern, reason in NOT_WINDOW
                if fnmatch.fnmatch(stem, pattern)
            )
            print(f"  {stem:<24} {why}")
        return 0

    if not args.app:
        parser.error("an app executable is required unless --list is given")
    app = Path(args.app).resolve()
    if not app.exists():
        raise SystemExit(f"[FAIL] no such executable: {app}")

    wanted = corpora
    if args.only:
        asked = [stem.strip() for stem in args.only.split(",")]
        unknown = [stem for stem in asked if stem not in corpora]
        if unknown:
            raise SystemExit(f"[FAIL] not window corpora: {', '.join(unknown)}")
        wanted = asked

    results = []
    for stem in wanted:
        result = run_one(app, stem, args.timeout)
        results.append(result)
        mark = "[OK]  " if result["exit"] == 0 else "[FAIL]"
        print(
            f"{mark} {stem:<16} {result['ran']} ran, {result['skipped']} skipped, "
            f"{len(result['names'])} names",
            flush=True,
        )
        for line in result["fails"]:
            print(f"       {line}", flush=True)

    print("\n=== the table, for BUILD.md")
    print("| fixture | ran | skipped | what it is there for |")
    print("|---|---|---|---|")
    purpose = dict(WINDOW_CORPORA)
    for result in results:
        stem = str(result["stem"])
        print(
            f"| `{stem}.pdf` | {result['ran']} | {result['skipped']} | "
            f"{purpose[stem]} |"
        )

    # The invariant the totals cannot express. Compared as sets against the
    # first fixture's, and printed as a difference rather than a count: two
    # runs disagreeing by one name is a check that stopped existing, and it
    # arrives looking exactly like a check that started skipping.
    print()
    failed = [result for result in results if result["exit"] != 0]

    # A name used twice inside one run is its own defect --- two checks under
    # one name make the transcript ambiguous, and `set()` would hide it while
    # the comparison below still passed.
    duplicated = []
    for result in results:
        names = list(result["names"])
        seen = {name for name in names if names.count(name) > 1}
        if seen:
            duplicated.append(result)
            print(f"[FAIL] {result['stem']} prints a name more than once")
            for name in sorted(seen):
                print(f"       {names.count(name)}x: {name}")

    baseline = sorted(set(results[0]["names"])) if results else []
    drifted = []
    for result in results[1:]:
        here = sorted(set(result["names"]))
        if here != baseline:
            drifted.append(result)
            only_there = set(here) - set(baseline)
            only_here = set(baseline) - set(here)
            print(f"[FAIL] {result['stem']} does not report the same check names")
            for name in sorted(only_there):
                print(f"       only in {result['stem']}: {name}")
            for name in sorted(only_here):
                print(f"       missing from {result['stem']}: {name}")

    if not drifted and not duplicated and results:
        print(f"[OK]   all {len(results)} corpora report the same {len(baseline)} check names")
    if failed:
        print(f"[FAIL] {len(failed)} corpus/corpora had failing checks")
    else:
        print(f"[OK]   no failing checks on any of {len(results)} corpora")

    return 1 if (failed or drifted or duplicated) else 0


if __name__ == "__main__":
    sys.exit(main())
