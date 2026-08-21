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
    scripts/viewer_sweep.py <app-exe> --raise      # only if a run produced nothing

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
import time
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
    (
        "incr-signed",
        # Named ahead of the `incr-*` rule below because its reason is no longer
        # the same one. On 2026-08-21 this became the first file in the tree that
        # could make the properties dialog draw a certificate, so "not the
        # viewer" had stopped being the whole truth. It said "the ONLY file"
        # until later the same day, by which point five others could --- a claim
        # of uniqueness with nothing asserting it, in the file that exists
        # because the list of corpora had no home. It stays out anyway: a new corpus
        # has to satisfy the sample points all 109 checks hardcode (see the trap
        # of that name), and what a window run would add over
        # `properties.test.ts` is that the dialog prints rows it already prints
        # for every other section. Worth revisiting if the certificate rows ever
        # gain behaviour of their own rather than being text.
        "signed, and deliberately still not a window corpus --- "
        "its certificate rows are covered by properties.test.ts, and a 1-page "
        "8 KB document does not meet the sample points the other checks pin",
    ),
    ("incr-*", "incremental-save fixtures for the document model, not the viewer"),
    (
        "signed-nested-field",
        "a signature field two levels down the /AcroForm tree, read by "
        "signature-probe and by docinfo's own tests; nothing in a window "
        "depends on where in the field tree a signature sits",
    ),
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


def classify() -> tuple[list[str], list[str], list[str]]:
    """Every fixture on disk, split into corpora and excluded, plus the corpora
    that have no fixture here.

    Those are two different questions and merging them cost the first tag of
    26.8.3 both gate legs. *Is every file present accounted for* is an invariant
    of the repository and holds anywhere. *Is every corpus present* is a
    precondition of running a sweep, and it is deliberately false on a hosted
    runner: `scripts/ci_fixtures.py` generates seven fixtures and states why the
    rest are not generatable there --- fonttools and a per-image system font,
    qpdf, a 550 MB write. Refusing on the second question inside the first made
    the gate red on every machine that was not sweeping, which is every machine
    but this one, and the gate had never run on one because a development
    checkout has the whole set. So the missing list is returned rather than
    raised, and the run path refuses on it.
    """
    on_disk = sorted(p.stem for p in TESTDATA.glob("*.pdf"))
    corpora = [stem for stem, _ in WINDOW_CORPORA]
    missing = [stem for stem in corpora if stem not in on_disk]

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
    # Only asked where the fixtures are all present: on a runner that generates
    # seven of them most patterns match nothing for a reason that is not rot,
    # and a warning that fires on every CI run is one nobody reads on the run
    # where it means something.
    if not missing:
        for pattern, _ in NOT_WINDOW:
            if not any(fnmatch.fnmatch(stem, pattern) for stem in on_disk):
                print(f"[WARN] no fixture matches the exclusion {pattern!r}")

    return corpora, excluded, missing


def _kill_leftovers() -> None:
    """Kills any tpdf still running, on whichever platform this is.

    **This was `pkill` unconditionally, and on Windows that is not a program.**
    `subprocess.run(..., check=False)` swallows a non-zero exit and not a
    `FileNotFoundError`, so the sweep died on its first corpus with a traceback
    --- and the shell reported exit 0, because the traceback goes to stderr. A
    harness that dies while looking like one that ran is the failure this
    repository has an entry about. This one was at least loud: it printed a
    traceback and no table.

    Failure is ignored on purpose. "There was nothing to kill" is the ordinary
    case, and both tools report it with a non-zero exit.
    """
    if sys.platform == "win32":
        command = ["taskkill", "/F", "/IM", "tpdf.exe"]
    else:
        command = ["pkill", "-f", "tpdf.app/Contents/MacOS/tpdf"]
    try:
        subprocess.run(command, check=False, capture_output=True)
    except OSError:
        # No such tool on this machine. A leftover is a slow run or a swallowed
        # launch rather than a wrong answer, and both are visible in the check
        # output, so this is not worth refusing over.
        pass


def run_one(app: Path, stem: str, timeout: int, raise_window: bool) -> dict[str, object]:
    """One fixture through the harness, returning what the table needs.

    The wall-clock is measured because the sweep is the slowest thing anyone
    runs here and, until it was, nobody could say which fixture was costing
    them. It is one A0 corpus: `vector-multi` was 338 s of a 721 s sweep --- 47%
    of the run in one of fourteen entries --- and that is a fact about the
    fixture rather than about the checks, so a total tells you nothing about
    where to look.
    """
    began = time.monotonic()
    # A leftover window occludes the next one, WebKit suspends an occluded
    # page, and the run then produces nothing while using no CPU. `--raise`
    # covers the other half, a window with nowhere visible to go.
    #
    # **Off by default since 2026-08-20, on the machine's owner's report that a
    # sweep locks the Mac up.** It did: fourteen launches, each one taking the
    # keyboard away from whatever was in front. And it was never what the checks
    # need --- `lib.rs` says so at the call site, and its own default is polite:
    # *"the check drives behaviour rather than timing it, so an unfocused window
    # costs it nothing"*. What they need is a window that is not **occluded**,
    # which is a different property. This forced the raise anyway, as a blunt
    # guarantee that suited an unattended run and nothing else.
    #
    # The failure this guards against is real and is already detected: an
    # occluded page is suspended, produces nothing, and uses no CPU, which
    # `webview_guard.py` tells apart from a hang by a CPU delta and answers with
    # the name of this flag. So the cost of the polite default is one wasted run
    # that says what to do, rather than every run taking the screen.
    #
    # On Windows the leftover is worse than an occlusion:
    # `tauri-plugin-single-instance` makes a new process forward its argv to the
    # old one and exit, so the run reports one line and no checks at all.
    # Measured on 2026-08-19, against three stray `tauri dev` processes.
    _kill_leftovers()
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
        # **Explicit, because `text=True` alone uses the locale codec** --- which
        # is cp1252 on Windows, and `multilingual.pdf` is the corpus whose whole
        # point is text that is not Latin. The decode raised inside subprocess's
        # own reader thread, which left `stdout` as None, so the failure arrived
        # as `TypeError: unsupported operand type(s) for +` on the line below
        # and said nothing about an encoding. Six corpora had already passed.
        #
        # `errors="replace"` rather than "strict": this text is read by
        # `CHECK-NAMES-JSON` and a few regexes, and a mangled glyph in a check's
        # detail line is worth less than a run that dies. Same decision, for the
        # same reason, that `mutate_frontend.py` records for vitest's output.
        encoding="utf-8",
        errors="replace",
        env={**os.environ, **({"TPDF_RAISE": "1"} if raise_window else {})},
    )
    out = (result.stdout or "") + (result.stderr or "")
    summary = SUMMARY.search(out)

    roll = NAMES_JSON.search(out)
    if roll is None:
        # Refused rather than fallen back on. Comparing name sets recovered by
        # guesswork is how the first version of this reported agreement about a
        # set that was wrong.
        #
        # What it must NOT do is name a cause it has not established. This said
        # "the bundle predates it --- rebuild" and nothing else, and the first
        # time it fired the bundle was current: the run had died before printing
        # the roll for an unrelated reason, and a freshly built app was rebuilt
        # again on the strength of a sentence. A stale bundle, a crash, a
        # timeout, a window that never became visible and an app that refused to
        # start all produce this same silence, so report what was actually seen
        # and let the reader pick. See the trap about a static reason turning a
        # failure into a wrong diagnosis.
        failures = [line for line in out.splitlines() if line.startswith("[FAIL]")]
        tail = "\n".join(f"       | {line}" for line in out.splitlines()[-8:]) or "       | (no output at all)"
        raise SystemExit(
            f"[FAIL] {stem}: the run printed no CHECK-NAMES-JSON line, so its\n"
            "       check names cannot be compared with the other corpora.\n"
            f"       exit={result.returncode}  bytes={len(out)}  "
            f"summary-line={'yes' if summary else 'no'}  "
            f"[FAIL] lines={len(failures)}\n"
            "       Causes that all look like this: a bundle predating the roll\n"
            "       (rebuild with `npm run tauri build`), a crash, the --timeout\n"
            "       expiring, or a window that never became visible. The last\n"
            "       lines of the run:\n" + tail
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
        "seconds": time.monotonic() - began,
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
    # 420 until 2026-08-17, when the page-deletion phase pushed `vector-multi`
    # --- twelve A0 pages, where a tier-1 render alone costs about a second and
    # a half --- past it, and the sweep stopped there. Cutting that phase from
    # two delete-and-restore cycles to one brought it back to **338 s measured**,
    # which is 82 s of margin: enough today and not enough to build on. A
    # generous timeout costs nothing on a fast run, and a tight one fails as
    # "the run printed no CHECK-NAMES-JSON line", which reads as a crash.
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument(
        "--raise",
        dest="raise_window",
        action="store_true",
        help=(
            "focus each window as it launches. Needed only where there is "
            "nowhere visible to put one --- a full-screen window over it, "
            "another Space --- and it takes the keyboard fourteen times, so it "
            "is off by default. A run that produces nothing says to use it."
        ),
    )
    args = parser.parse_args()

    corpora, excluded, missing = classify()

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
        if missing:
            # Stated, not refused. A machine without the whole set cannot sweep
            # and is not trying to; what it can still answer is whether every
            # fixture it does have is accounted for, which is what this gate is.
            print(
                f"\n[INFO] {len(missing)} corpus fixture(s) are not on this "
                "machine, so a sweep here would cover less than it claims: "
                + ", ".join(missing)
            )
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

    # The refusal that used to live in classify(), aimed at the fixtures this
    # run will actually open rather than at all fourteen. A full sweep is
    # unchanged by that, since `wanted` is then every corpus; `--only` on a
    # machine holding those two now works instead of being refused for the
    # twelve it was not going to touch.
    absent = [stem for stem in wanted if stem in missing]
    if absent:
        raise SystemExit(
            f"[FAIL] {len(absent)} corpus fixture(s) are not in testdata/: "
            + ", ".join(absent)
            + "\n       Generate them first; a sweep that silently skips a corpus "
            "reports a clean run over less than it claims."
        )

    results = []
    for stem in wanted:
        result = run_one(app, stem, args.timeout, args.raise_window)
        results.append(result)
        mark = "[OK]  " if result["exit"] == 0 else "[FAIL]"
        print(
            f"{mark} {stem:<16} {result['ran']} ran, {result['skipped']} skipped, "
            f"{len(result['names'])} names, {result['seconds']:.0f}s",
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

    # Where the time went, which is the question a total cannot answer. One A0
    # corpus was 47% of a 721 s sweep, and the only way anyone found that out
    # was by timing the fixtures by hand after being asked why the machine had
    # been at full tilt for twelve minutes. Printed as a share of the run so a
    # fixture that starts dominating says so before it has to be measured again.
    total = sum(float(result["seconds"]) for result in results)
    slowest = sorted(results, key=lambda r: float(r["seconds"]), reverse=True)[:3]
    print(f"\n=== {total:.0f}s over {len(results)} corpora")
    for result in slowest:
        share = 100.0 * float(result["seconds"]) / total if total else 0.0
        print(f"  {str(result['stem']):<16} {float(result['seconds']):>6.0f}s  {share:>4.0f}%")

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

    # A name that is a prefix of another cannot be aimed at. `mutate_viewer.py`
    # decides whether a mutation was caught with `line.startswith(expect)` over
    # the failing check names, and refuses an expectation matching more than
    # one --- correctly, and only when somebody writes that mutation. So the
    # constraint is on the *names*, and it has been broken once already:
    # `search_probe.rs` had `query astral-alone` alongside `query astral-alone:
    # indices address the hit`, and no mutation could target the first.
    #
    # Checked here because this is where the whole set is known. `docs/TRAPS.md`
    # states the rule under "a check name that is a prefix of another cannot be
    # aimed at"; it was enforced by nothing until now.
    shadowed = []
    for result in results:
        names = sorted(set(result["names"]))
        for name in names:
            longer = [other for other in names if other != name and other.startswith(name)]
            if longer:
                shadowed.append((result["stem"], name, longer))
    for stem, name, longer in shadowed:
        print(f"[FAIL] {stem}: {name!r} is a prefix of {len(longer)} other check name(s)")
        for other in longer:
            print(f"       {other!r}")
        print("       No mutation can be aimed at the shorter one. Give it a suffix.")

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
    if not shadowed and results:
        print(f"[OK]   no check name is a prefix of another, so each can be aimed at")
    if failed:
        print(f"[FAIL] {len(failed)} corpus/corpora had failing checks")
    else:
        print(f"[OK]   no failing checks on any of {len(results)} corpora")

    return 1 if (failed or drifted or duplicated or shadowed) else 0


if __name__ == "__main__":
    sys.exit(main())
