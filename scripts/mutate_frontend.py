#!/usr/bin/env python3
"""Breaks the front-end selection code on purpose, one edit at a time.

A test that has only ever passed looks exactly like one that cannot fail, so
each mutation below names the test it is *expected* to turn red, and the run
reports a mutation that nothing caught as a defect in the suite.

Two properties this harness has because `AGENTS.md` records what their absence
costs:

**It cross-checks.** Every run derives the failure count two ways -- by counting
the reporter's per-test `x` lines and by reading its summary line -- and a
disagreement is reported as a broken run rather than as either answer. The trap
entry is about a harness that printed SURVIVED while its own summary, four lines
below in the same buffer, said a check had failed.

**A run that produced no summary is not a pass.** A crash, a timeout and a
syntax error from a bad mutation all produce no failing-test lines, which is
exactly what a surviving mutation looks like.

Usage:
    scripts/mutate_frontend.py            # every mutation
    scripts/mutate_frontend.py --list
    scripts/mutate_frontend.py --since HEAD~3   # only what the diff touched
    scripts/mutate_frontend.py --resume         # reuse a killed run's verdicts
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402
import mutation_resume  # noqa: E402
import mutation_since  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


#: What a row is, and the rows: `mutation_rows/frontend/`, one part per
#: stretch of the table, joined in the order they run by its `__init__.py`.
#: Moved out of this file on 2026-10-15, when it had grown to 10,910 lines
#: of which all but about 500 were rows.
from mutation_rows.frontend.base import Mutation  # noqa: E402,F401
from mutation_rows.frontend import ROWS  # noqa: E402

MUTATIONS = list(ROWS)


#: The suites this harness deliberately does NOT run, and why for each.
#:
#: Every other suite under `src/` is run: `TEST_FILES`, below this table, is
#: what is on disk less what is named here. Until 2026-10-04 that list was
#: written by hand, kept short because each entry is paid for on every
#: mutation, and it drifted thirteen times by its own notes: the tests are
#: written first and the list was edited only by whoever wrote the mutations.
#: `scripts/check_mutation_test_files.py` made each omission a gate failure
#: and not a refused run; deriving the list removes the step that was
#: forgotten. The whole frontend suite runs in about two seconds, so the cost
#: the short list saved is the ten suites below, each excluded *with a reason*.
#:
#: A new suite is therefore run from the day it exists. One that should not be
#: is added here, with why. The objection recorded against a glob, that a
#: wider name set can surface a duplicate test name and refuse a run, applies
#: to a suite added to the tree and not to this change, which runs the same
#: files the hand-written list named.
UNMUTATED = {
    # Its own assertions are never mutated. The three mutations aimed at
    # `rowline.ts` expect tests in `marklist.test.ts`, which is listed, so they
    # can go red --- this file is the one place where a defect in `rowline.ts`
    # would be caught by an assertion no mutation has ever broken.
    "src/lib/rowline.test.ts": "no mutation aims at src/lib/rowline.ts's own suite",
    # The ten below are the same shape as each other: the module has tests and
    # no mutation is aimed at it, so running its suite would cost time in every
    # mutation run and prove nothing. Writing one is what moves the entry --- and
    # the moment a mutation names a test in one of these files, the gate refuses
    # it, which is the twelve-times failure caught before the run rather than
    # twenty minutes into it.
    "src/lib/backoff.test.ts": "no mutation aims at src/lib/backoff.ts",
    "src/lib/degraded.test.ts": "no mutation aims at src/lib/degraded.ts",
    "src/lib/lifetime.test.ts": "no mutation aims at src/lib/lifetime.ts",
    "src/lib/paths.test.ts": "no mutation aims at src/lib/paths.ts",
    "src/lib/serial.test.ts": "no mutation aims at src/lib/serial.ts",
    "src/lib/tiles.test.ts": "no mutation aims at src/lib/tiles.ts",
    # A different shape from the ten above, and the reason is worth stating
    # rather than borrowing theirs. This suite has no production module at all:
    # it asserts that every panel keeping a focus mirror has a sibling test that
    # dispatches an arrow key --- the enforcement `docs/TRAPS.md` chose when it
    # declined to extract that widget. No mutation of any `.ts` file can redden
    # it, because what it reads is the shape of the *test tree*. It was proved
    # falsifiable by hand instead, three ways: renaming `ArrowDown` in one
    # panel's suite, widening its discriminator until the palette matched, and
    # narrowing it until nothing did.
    "src/lib/rovinglists.test.ts": "asserts the shape of the test tree, which no source mutation changes",
    # `rovinglists.test.ts`'s shape rather than the ten above it: no mutation
    # aims at `ipc.ts`, and none could redden this suite anyway, because what it
    # reads is the *source text* of `generate_handler!` in `lib.rs` against the
    # keys of `ipc.ts`'s command map. A mutation of a `.ts` module's behaviour
    # changes neither string. Every one of its checks was proved able to go red
    # by hand instead: an entry removed from the map, a fabricated entry added,
    # two entries swapped, a duplicate entry, each of the two regexes broken so
    # that the side it reads comes back empty, and the registry regex widened
    # past the macro's own name so that what it captures is not identifiers.
    "src/lib/ipc.test.ts": "reads two source texts, which no source mutation of a module changes",
    # `ipc.test.ts`'s shape, one level down: what this suite reads is the JSON
    # `src-tauri/src/replies.rs` writes, held against the mirror types. Nothing
    # a mutation could do to a `.ts` module's behaviour changes either side, and
    # the two checks that could be wrong are the compiler's (`satisfies`) and a
    # key comparison against files Rust produced. Proved able to go red by hand
    # instead, and by the mechanism it exists to catch: renaming one Rust field
    # on the wire (`#[serde(rename)]` on `docinfo::Properties::language`) and
    # regenerating turned `tsc` red on the missing key, and both key checks red
    # on the missing name and on the new one.
    "src/lib/replyshapes.test.ts": "reads JSON written by the Rust side, which no source mutation of a module changes",
}

#: The suites every mutation run executes: each one under `src/` that
#: `UNMUTATED` does not name. Found on disk, so a suite that is not committed
#: yet is run as well.
TEST_FILES = sorted(
    path.relative_to(ROOT).as_posix()
    for path in (ROOT / "src").rglob("*.test.ts")
    if path.relative_to(ROOT).as_posix() not in UNMUTATED
)

FAILED_TEST = re.compile(r"^\s*(?:x|×)\s+(.*?)(?:\s+\d+ms)?$", re.M)
TEST_NAME = re.compile(r"^\s*[✓x×]\s+(\S+\.test\.ts)\s*>\s*(.*?)(?:\s+\d+ms)?$", re.M)
#: vitest's own count, and it must not require anything to have PASSED.
#:
#: This read `(?:(\d+) failed)?.*?(\d+) passed` until 2026-08-21, which was
#: true of every run it had ever seen and stopped being true the moment the run
#: was narrowed to one file: `Tests  2 failed (2)` has no `passed` segment at
#: all, because in that file nothing did. The regex then matched nothing, the
#: harness reported `no summary line -- the run did not finish`, and a mutation
#: its test had caught perfectly read as a broken run.
#:
#: The lookahead is the strict half and is deliberate: a transform error prints
#: `Tests  no tests` and must stay unreadable rather than parse as zero
#: failures, which would report SURVIVED for a run that never executed.
SUMMARY = re.compile(r"^\s*Tests\s+(?=.*(?:failed|passed))(.+)$", re.M)

#: The failing count inside that line, absent when everything passed.
SUMMARY_FAILED = re.compile(r"(\d+) failed")


def npx() -> str:
    """Resolves npx, which is `npx.cmd` on Windows and not on PATH as `npx`."""
    return shutil.which("npx") or "npx"


def run_tests(files: list[str] | None = None) -> tuple[set[str], int | None, int, str]:
    """Runs the suite, returning the failed names, the summary's count, the line count and the log.

    The names come back as a SET and the line count separately, because they are
    different quantities and the cross-check below needs the second one. Thirteen
    test names are defined in more than one file --- `closes on Escape`,
    `is one tab stop`, `normalises a negative turn` and ten others --- so a
    mutation that reddens two of them collapses to one name while vitest counts
    two, and the cross-check condemned the harness for a discrepancy the suite
    had put there. `all_test_names` had already learned that duplicates exist and
    returns a list of files per name; this end of the same run had not.

    `files` narrows the run to the file the mutation's own test lives in, which
    vitest is told by name rather than by any table kept here -- the control
    run's verbose listing prints the file beside every test, and that mapping is
    what the caller passes back in. The whole file rather than `-t <name>`, so a
    mutation that reddens three tests in it still reports three.

    The caller runs every file anyway whenever the narrow run finds nothing red.
    That is the case where the answer matters most: SURVIVED and "a test in
    another file caught it" are different findings, and only the full set can
    tell them apart.
    """
    done = subprocess.run(
        [npx(), "vitest", "run", *(files or TEST_FILES)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        # vitest marks a test with U+2713/U+00D7, and `text=True` alone decodes
        # with the locale codec -- cp1252 on Windows, where those bytes become
        # mojibake and every mark-keyed regex silently matches nothing.
        encoding="utf-8",
        errors="replace",
        timeout=300,
    )
    out = done.stdout + done.stderr
    # Split on the marker and take the rest of the line -- never a fixed column.
    lines = [m.strip() for m in FAILED_TEST.findall(out) if m.strip()]
    names = set(lines)
    summary = SUMMARY.search(out)
    counted = None
    if summary:
        failed = SUMMARY_FAILED.search(summary.group(1))
        counted = int(failed.group(1)) if failed else 0
    return names, counted, len(lines), out


def all_test_names() -> dict[str, list[str]]:
    """Every test the suite defines, mapped to the file(s) it is in.

    Files, plural, and that is not defensive. `says nothing when nothing was
    cut` is defined in both `comments.test.ts` and `links.test.ts`, so a
    `dict[str, str]` kept whichever the listing printed last and the narrow run
    below aimed at the wrong file --- caught on 2026-08-21 only because the
    fallback ran all 36 files and got the right answer anyway, at the cost of the
    saving this whole mechanism exists for.
    """
    done = subprocess.run(
        [npx(), "vitest", "run", "--reporter=verbose", *TEST_FILES],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=300,
    )
    out = done.stdout + done.stderr
    # `✓ src/lib/x.test.ts > describe > name 3ms` -- split on the marker and take
    # the rest, never a fixed column. The file comes back too: it is printed
    # right there, and deriving it beats a second table that can drift from the
    # one above.
    found: dict[str, list[str]] = {}
    for file, name in TEST_NAME.findall(out):
        name, file = name.strip(), file.strip()
        if not name:
            continue
        where = found.setdefault(name, [])
        if file not in where:
            where.append(file)
    return found


def main() -> int:
    # Before anything prints. A redirected run is block-buffered otherwise, and
    # this harness takes the better part of an hour: on 2026-08-19 a full run's
    # output sat at three lines for forty minutes and its verdict was lost
    # entirely when the run was interrupted, which is the exact ambiguity
    # `live_output` exists to remove. The three window harnesses had the fix and
    # the three mutation harnesses did not.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    # Same flag, same meaning as `mutate_viewer.py`'s. Added when the page
    # deletion work put twenty new mutations into a table of a hundred: the
    # whole table is what runs before a push, and re-proving ninety-odd
    # mutations that could not have moved is an hour of somebody waiting.
    # The control run and the name cross-check below still run in full.
    parser.add_argument(
        "--only", action="append", default=[], help="run mutations whose name contains this; repeatable"
    )
    # `--only` matches names; `--since` selects by the file a mutation
    # edits, without needing a filter per behaviour. See `mutation_since.py` for
    # why it is loud and why nothing selected is a refusal.
    parser.add_argument(
        "--since", default="", help="run only mutations whose file changed since this ref"
    )
    # This is the table that was killed twice at about twenty-five minutes on
    # 2026-08-30, losing four hundred proved verdicts and leaving its current
    # mutation in `src/lib/pages.ts`. Both halves of that are answered by
    # `mutation_resume.py`, and the recovery half runs whether or not this flag
    # is passed --- a mutated tree is a defect regardless of what the next run
    # means to do.
    # The release's selection since 2026-10-04: a mutation whose target is
    # within a few lines of a change, or whose test changed. `--since` reruns
    # every mutation in a changed file and is what runs outside the release.
    # See `mutation_since.py` for what each gives up.
    parser.add_argument(
        "--near",
        default="",
        metavar="REF",
        help="only mutations near a line changed since REF, or whose test changed",
    )
    parser.add_argument(
        "--resume",
        action="store_true",
        help="reuse verdicts from an earlier run, if the tracked tree has not moved",
    )
    args = parser.parse_args()
    # Match every requested filter. Silently keeping only the last one reports
    # success for a smaller run than the caller requested.
    missing = [needle for needle in args.only if not any(needle.lower() in m.name.lower() for m in MUTATIONS)]
    if missing:
        print(f"[FAIL] no mutation matches {missing!r}")
        return 1
    chosen = [m for m in MUTATIONS if not args.only or any(needle.lower() in m.name.lower() for needle in args.only)]
    if args.since:
        # No prefix: this table names paths from the repository root already.
        chosen, code = mutation_since.apply(chosen, args.since)
        if code:
            return code
    if args.near:
        chosen, code = mutation_since.apply_near(chosen, args.near)
        if code:
            return code

    if args.list:
        for mutation in chosen:
            print(f"{mutation.name}  ->  expects: {mutation.expect}")
        return 0
    if not chosen:
        print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    # Before the control run, and before the fingerprint: a tree still holding
    # somebody else's mutation is neither green nor the tree these verdicts
    # would be about.
    resume = mutation_resume.Resume("frontend", reuse=args.resume)
    code, lines = resume.recover()
    for line in lines:
        print(line, flush=True)
    if code:
        return code
    for line in resume.open(chosen):
        print(line, flush=True)

    print("--- control: the suite must be green before anything is broken", flush=True)
    names, counted, red_lines, out = run_tests()
    if counted is None:
        print("[FAIL] the control run produced no summary line, so nothing below is readable")
        print(out[-2000:])
        return 1
    if counted != 0 or names:
        print(f"[FAIL] the control run is not green: {counted} failed, {sorted(names)}")
        return 1
    print("[OK]   control green", flush=True)

    # Every `expect` must name a test this harness can actually run. One named a
    # check that only `viewer_check.py` records, and the run reported SURVIVED --
    # which reads as a gap in the suite rather than a mistake in the harness, and
    # is the most misleading verdict a mutation pass can print. Derived from the
    # control run's own list rather than from a hand-kept table.
    known = all_test_names()
    unknown = [m for m in chosen if not any(m.expect in name for name in known)]
    if unknown:
        for mutation in unknown:
            print(
                f"[FAIL] {mutation.name}: no test here is named {mutation.expect!r} -- "
                "it cannot go red, so this mutation would report SURVIVED"
            )
        return 1
    print(f"[OK]   every mutation names one of the {len(known)} tests", flush=True)

    def tally(ok: bool) -> int:
        """What one mutation adds to `problems`: 0 when it was caught, 1 otherwise.

        One line, and it exists because a reused verdict and one taken just now
        reach `problems` by different routes. Written at both, the rule would be
        two copies of itself --- the shape this repository keeps finding drifted,
        and the one a mutation of either half survives.
        """
        return 0 if ok else 1

    def say(mutation, ok: bool, *lines: str) -> int:
        """Print one mutation's verdict and file it, in that order.

        Every outcome below goes through this, so a run killed a moment later
        has already recorded everything it decided. The return value is what the
        caller adds to `problems`, which keeps the two from drifting.
        """
        for line in lines:
            print(line, flush=True)
        resume.record(mutation, ok, "\n".join(lines))
        return tally(ok)

    problems = 0
    for mutation in chosen:
        stored = resume.done(mutation)
        if stored is not None:
            # Marked, because a reused verdict and one taken just now are
            # the same claim about the tree and not the same run, and a
            # transcript that cannot tell them apart is where somebody reads
            # "all caught" for a table that was never executed here.
            print(f"{stored.line}   [reused]", flush=True)
            problems += tally(stored.ok)
            continue
        target = ROOT / mutation.path
        # Bytes, decoded explicitly: `read_text` uses the locale codec,
        # which is cp1252 on Windows, and an anchor holding a glyph like
        # the Option sign then matches nothing -- reported as "the
        # mutation is not the one described", which reads as drift in
        # the source rather than in this harness.
        #
        # Its newline translation was doing real work, though, and
        # removing it alone took three failures to twelve, because every
        # anchor here is written with "\n" and the checkout was CRLF. So
        # normalise for matching and put the file's own convention back,
        # leaving the mutation as the only difference on disk.
        #
        # **The checkout is no longer CRLF**: `.gitattributes` has pinned
        # `* text=auto eol=lf` since 2026-08-26. Kept regardless, because
        # a checkout is not the only thing that writes a file -- see the
        # longer note in `mutate_rust.py`.
        clean = target.read_bytes()
        raw = clean.decode("utf-8")
        crlf = "\r\n" in raw
        source = raw.replace("\r\n", "\n") if crlf else raw
        if source.count(mutation.before) != 1:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: its anchor appears "
                f"{source.count(mutation.before)} times, so the mutation is not the "
                "one described",
            )
            continue
        mutated = source.replace(mutation.before, mutation.after)
        if crlf:
            mutated = mutated.replace("\n", "\r\n")
        payload = mutated.encode("utf-8")
        # Applied and taken off again by `mutation_resume.py`, which holds
        # the backup that outlives this process and enforces the rules the
        # three harnesses each used to carry their own copy of: the record
        # is written before the bytes, the restore writes bytes rather than
        # moving or copying a file over them -- a move replaces the file the
        # tooling may already be watching, and a copy carries the backup's
        # older mtime -- and the restored file is left newer than the
        # mutation, so nothing that decides staleness by timestamp goes on
        # serving it. `docs/TRAPS.md` has each of them.
        resume.apply(mutation, target, clean, payload)
        try:
            # The file holding the test this mutation names, and only it.
            # Nearly every mutation is caught there, and running the other
            # nineteen files to find that out cost 5.8 s a time -- 31
            # minutes over this table, measured 2026-08-21 before this.
            aimed = sorted(
                {
                    file
                    for name, files in known.items()
                    if mutation.expect in name
                    for file in files
                }
            )
            names, counted, red_lines, out = run_tests(aimed or None)
            narrow = bool(aimed)
            if counted is not None and not names and narrow:
                # Nothing in that file noticed. Whether anything else did is
                # exactly the question, so now every file runs.
                names, counted, red_lines, out = run_tests()
                narrow = False
        finally:
            put_back, notes = resume.restore()
        # A restore that could not put the file back stops the run. Carrying
        # on would test every later mutation against a file still holding
        # this one, and the verdicts would be about a tree nobody described.
        if not put_back:
            for line in notes:
                print(line, flush=True)
            return 1

        if counted is None:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: no summary line -- the run did not finish",
            )
            continue
        # The cross-check: the reporter's per-test lines and its own count
        # must agree, or one of the two has stopped describing the run. It
        # counts LINES rather than distinct names -- see `run_tests`.
        if red_lines != counted:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: {red_lines} failing test lines but the summary "
                f"says {counted} -- this harness cannot read its own output",
            )
            continue
        if not names:
            problems += say(
                mutation, False, f"[FAIL] {mutation.name}: SURVIVED -- no test noticed"
            )
            continue
        hit = any(mutation.expect in name for name in names)
        mark = "[OK]  " if hit else "[FAIL]"
        # Which files were run, because `1 red` out of one file and `1 red`
        # out of twenty are not the same statement.
        scope = (
            ", ".join(Path(f).name for f in aimed)
            if narrow
            else f"{len(TEST_FILES)} files"
        )
        verdict = [
            f"{mark} {mutation.name}: {counted} red in {scope}"
            + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
        ]
        if not hit:
            verdict.append(f"         red instead: {sorted(names)}")
        problems += say(mutation, hit, *verdict)

    print()
    for line in resume.closing():
        print(line)
    print(
        f"[OK] all {len(chosen)} mutations caught by the test named for them"
        if problems == 0
        else f"[FAIL] {problems} of {len(chosen)} mutations were not caught as described"
    )
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
