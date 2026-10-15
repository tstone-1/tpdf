#!/usr/bin/env python3
"""Breaks a Rust module on purpose, one edit at a time.

The counterpart to `mutate_frontend.py`, and it exists for the same reason: a
test that has only ever passed looks exactly like one that cannot fail. Each
mutation below names the test it is *expected* to turn red, and a mutation that
nothing caught is reported as a defect in the suite rather than shrugged at.

`search.rs` is the module it covers first. It is the densest piece of pure logic
in the backend --- a fold, an index map back through it, and two options that
each change what is accepted --- and every one of its assertions is over a
fixture the module itself never wrote, which is what makes mutation the only
thing that can say whether they bite.

`text.rs`, `structure.rs`, `encoding.rs`, `docmodel.rs`, `annots.rs`, `links.rs`,
`progressive.rs`, `edits.rs` and `save.rs` are covered too, and `FILTERS` below is
the list of record. `encoding.rs` is the one to be careful about, because its
tests are the *only* thing that can catch its central mutation: `encodings.pdf`
has `/Encoding` and `/Ordering` covarying on every page, so a rule keyed on the
wrong one of the two passes every fixture on disk. A harness without those
mutations would look thorough and prove nothing about the field that decides.

Two properties carried over from the front-end harness, both because
`docs/TRAPS.md` records what their absence costs:

**It cross-checks.** Every run derives the failure count two ways -- by counting
libtest's per-test `FAILED` lines and by reading its summary line -- and a
disagreement is a broken run rather than either answer.

**A run that produced no summary is not a pass.** A compile error from a bad
mutation produces no failing-test lines, which is exactly what a surviving
mutation looks like. It is the likeliest outcome here and the one that would
otherwise read as good news.

**It builds into its own target directory** (`src-tauri/target/mutations`) and
**runs only the test each mutation names**, falling back to the whole suite when
that test does not go red. Both are about cost rather than coverage, and the
whole table went from 4.4 hours to 405 s on 2026-08-21 --- `docs/TRAPS.md`,
"A harness that edits source files pays for the editor watching them", has the
measurements and why the fallback is the part that keeps it honest.

Usage:
    scripts/mutate_rust.py            # every mutation
    scripts/mutate_rust.py --list
    scripts/mutate_rust.py --only save
    scripts/mutate_rust.py --since HEAD~3   # only what the diff touched
    scripts/mutate_rust.py --resume         # reuse a killed run's verdicts
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402
import mutation_resume  # noqa: E402
import mutation_since  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "src-tauri"

#: This harness builds into its own target directory, and it is not tidiness.
#:
#: Every mutation writes a file under `src-tauri/src`, and an editor with
#: rust-analyzer open answers each of those writes with
#: `cargo check --workspace --all-targets`. That check takes the build
#: directory's lock, so the mutation's own `cargo test` queues behind a
#: whole-workspace re-check --- measured on 2026-08-21 with cargo saying so in as
#: many words, `Blocking waiting for file lock on build directory`, and a
#: no-op `cargo test --lib --no-run` taking 28.2 s where it takes 0.2 s alone.
#: Multiply that by the table and the run is hours longer than the work in it.
#:
#: Inside `target/` so the existing ignore rule covers it, and persistent so the
#: cost is one cold build ever rather than one per run. It also stops the editor
#: from reporting diagnostics against a tree that is mutated at the time.
MUT_TARGET = CRATE / "target" / "mutations"

#: And it builds that target directory *without debug info*, which is worth 7x.
#:
#: Every mutation touches one file, so cargo re-codegens the crate and relinks
#: the test binary --- and with full debug info that binary is 33 MB and the
#: cycle costs 22 to 30 s. Measured 2026-08-27, interleaved against the same
#: command in a second target directory built with `debug = 0`:
#:
#:     round 1:  debug= 22.6s   nodebug=  3.9s
#:     round 2:  debug= 30.1s   nodebug=  3.7s
#:     round 3:  debug= 27.0s   nodebug=  3.3s
#:
#: For the 508-mutation table that is 5.6 hours against well under one, and the
#: target directory is 1.7 GB rather than 14 GB. The cold build is 178 s, paid
#: once.
#:
#: It is safe because `debug` is debug *information* only: `debug_assertions`
#: and overflow checks are separate knobs and are untouched, so every test runs
#: exactly the program it ran before. What is given up is line numbers in a
#: panic backtrace, and nothing here reads one --- the harness identifies a
#: caught mutation by the test *name* libtest prints, which is unaffected.
#:
#: The figure this replaces is `BUILD.md`'s 1.77 s per mutation, measured on
#: 2026-08-21 when the crate was much smaller. That number did not go stale
#: because anybody changed the harness; it went stale because the crate grew,
#: which is exactly the kind of expiry nothing goes red about.
CARGO_ENV = {
    **os.environ,
    "CARGO_TARGET_DIR": str(MUT_TARGET),
    "CARGO_PROFILE_DEV_DEBUG": "0",
}

#: Which platform this is, in the vocabulary `Mutation.only_on` uses.
HERE = "macos" if sys.platform == "darwin" else "windows" if sys.platform == "win32" else "linux"

#: Only these modules' tests are run, so an unrelated failure elsewhere cannot be
#: read as a mutation being caught. libtest takes several filters and ORs them,
#: but only after `--`: `cargo test --lib a:: b::` is cargo's own argument error,
#: which is worth knowing because it looks like the feature being unsupported.
FILTERS = [
    "redaction_fill::",
    "raster_redact::",
    # Added 2026-10-08 with the repair of a path that lost its quotes.
    "launch::",
    "objects::",
    "textedit::",
    "textview::",
    "forms::",
    "search::",
    "structure::",
    "text::",
    "encoding::",
    "docmodel::",
    "annots::",
    "links::",
    "progressive::",
    "edits::",
    "save::",
    # Added 2026-08-16 with the shared-page fix. `print.rs` had been mutated by
    # nothing, so its module was never in this list --- and the three mutations
    # written for that fix named tests the harness could not see, because this
    # list is what `--list` is filtered by. It refused to start and said which
    # three, which is that guard doing its job rather than a bug in it.
    "print::",
    # Added 2026-08-17 with the page-tree module. Its own tests were invisible to
    # the harness for the same reason `print::` was: this list is what selects
    # them, and a mutation whose test cannot be seen reports SURVIVED.
    "pagetree::",
    # Added 2026-10-07 with the bookmark repair, whose tests are its own module's.
    "outline_repair::",
    # Added 2026-10-07 with `tpdf hidden`: the comparison and the command.
    "hidden::",
    "cli::hidden::",
    # Added 2026-08-18 with the crop. Fourth time this list has been forgotten
    # and fourth time the guard is what said so, before six mutations could
    # report SURVIVED for tests it simply could not see.
    "content::",
    # Added 2026-08-17 with the menu bar. The guard fired a third time and said
    # exactly which mutation it was, before the mutation could report SURVIVED.
    #
    # `menu.rs` is macOS-only, so on Windows this filter matches nothing and the
    # mutation naming its test is refused rather than run. That is the right
    # answer there --- the code under it does not exist on that platform --- and
    # it is worth knowing before the refusal reads as a broken harness.
    "menu::",
    # Added 2026-08-17 with the keyboard-layout lookup, macOS-only for the
    # same reason `menu::` is: Carbon does not exist on Windows.
    "keylayout::",
    # Added 2026-08-19 with the version display. `lib.rs`'s own root test module
    # had never been selected by anything here, so the test asserting the four
    # version files agree could not go red -- and the guard said so before the
    # mutation aimed at it could report SURVIVED. Fifth time; the list is the
    # thing this harness most reliably forgets, and the guard is the thing that
    # most reliably catches it.
    "tests::",
    # Added 2026-08-19 with the external-modification check. Sixth time, and this
    # one was found by looking rather than by the guard: no mutation named a
    # `fingerprint::` test, so nothing refused to start --- a module whose tests
    # are invisible AND unaimed-at is silent in both directions.
    "fingerprint::",
    # Added 2026-08-19 with the Jump List. Written at the same time as the
    # mutations rather than after them, which is the whole lesson of the five
    # times this list was forgotten and the sixth time it was not even noticed.
    "recentdocs::",
    # Added 2026-08-26 with the verification module, at the same time as the
    # mutations rather than after them. Seventh time this list has needed an
    # entry, and the first time it was written without the guard having to say
    # so -- which is what the six notes above are for.
    "verify::",
    # Added 2026-08-31 with `save_outside.rs`, in the same edit that moved the
    # two read-back deadline mutations into it. Eighth time, and the first where
    # the list had to change for a *move* rather than for new code: `save::` is
    # not a prefix of `save_outside::` --- libtest filters on substring, and
    # "save::" does not occur in "save_outside::tests::..." --- so the two
    # mutations would have named tests the harness could no longer see, and
    # reported SURVIVED for a deadline that works.
    "save_outside::",
    # Added 2026-08-31 with `save_order.rs`, in the same edit that took the save
    # sequence out of `save_document`. Ninth time, and the second in a day where
    # the substring rule is what makes the entry necessary rather than tidy:
    # "save::" does not occur in "save_order::tests::...", and "save_outside::"
    # does not either, so without this line the five mutations aimed at the
    # ordering would name tests the harness cannot see and report SURVIVED for a
    # sequence that works. `scripts/check_mutation_anchors.py` cannot say so --
    # it reads anchors and test names out of the sources and knows nothing about
    # this list -- which is why the note is here rather than trusted to a gate.
    "save_order::",
    # Added 2026-09-06 with `fields.rs`, in the same edit that moved the two
    # field-tree walks into it. It is subsumed by `tests::` above and is written
    # anyway, because that entry matches only because this module happens to
    # call its test module `tests` -- a filter that depends on nobody renaming a
    # private module is not a filter. Tenth entry; the eight notes above are all
    # the same incident.
    "fields::",
    # Added 2026-09-26 with the signature integrity verdict, in the same edit
    # as its mutations. Eleventh entry. `tests::` would match today for the
    # reason the note above gives, and that is not a reason to rely on it.
    "integrity::",
    # Added 2026-09-26 with Phase 6 step 2, in the same edit as its mutations:
    # the worker's revision, the app process's CMS and splice, and the OS key
    # store's pure parts.
    "sign_prepare::",
    "sign_cms::",
    "keystore::",
    # Added 2026-09-27 with the trust standing, in the same edit as its
    # mutations. `docinfo::` is already here through `tests::`, which is the
    # reliance the notes above refuse; the wiring test lives there too.
    "trust::",
    "docinfo::",
    # Added 2026-09-27 with the command-line tool, in the same edit as its
    # mutations. `cli::` would not reach `clitool::` --- libtest filters on a
    # substring, and "cli::" is not in "clitool::tests::..." --- so both.
    "cli::",
    # "Show in folder" on Linux, 2026-10-15.
    "commands::document::",
    "clitool::",
    # Added 2026-09-27 with `tpdf text`, in the same edit as its mutations:
    # `reading.rs` restates `reading.ts` for the command line.
    "reading::",
    # Added 2026-09-28 with the timestamp client, in the same edit as its
    # mutations: the fake authority on 127.0.0.1 is in `tsa::tests`.
    "tsa::",
    # Added 2026-09-28 with revocation (increment C1), in the same edit as its
    # mutations. "docinfo::" already reaches `docinfo::revocation_tests`.
    "revocation::",
    # Added 2026-09-28 with long-term validation data (increment C2), in the
    # same edit as its mutations: the fake PKI's tests are in `longterm::tests`,
    # the `/DSS` writer's in `sign_dss::tests`, and the held stage's in
    # `commands::sign::tests` --- which `tests::` reaches today only by the
    # accident the notes above refuse to rely on.
    "longterm::",
    "sign_dss::",
    "commands::sign::",
    # Added 2026-10-10 with validation data for a document already signed, in
    # the same edit as its mutations: the shared tail's tests are in
    # `commands::validation::tests`. `longterm::` already reaches
    # `longterm::existing::tests`.
    "commands::validation::",
    # Added 2026-09-30 with installed-font matching, in the same edit as its
    # mutations: the app process's lookup and round trip are `sysfont::tests`.
    "sysfont::",
    # Added 2026-10-09 with the description of a reply that answers the wrong
    # question, in the same edit as its mutations: the tests are in
    # `workers::tests` and `worker_proto::tests`, which `tests::` reaches
    # today only by the accident the notes above refuse to rely on.
    "workers::",
    "worker_proto::",
    # Added 2026-10-15 with the Linux rows, in the same edit as them. Each is
    # its own module's tests; `worker_child::` holds the forked boundary probe.
    "worker_child::",
    "worker_handover::",
    "sandbox_linux::",
    "pin_prompt::",
    "certfiles::",
    "pkcs12::",
    "pkcs11::",
    "ocr_tesseract::",
    "print_linux::",
]


#: What a row is, and the rows: `mutation_rows/rust/`, one part per
#: stretch of the table, joined in the order they run by its `__init__.py`.
#: Moved out of this file on 2026-10-15, when it had grown past 19,000 lines
#: of which all but about 700 were rows.
from mutation_rows.rust.base import Mutation  # noqa: E402,F401
from mutation_rows.rust import ROWS  # noqa: E402

MUTATIONS = list(ROWS)


#: libtest prints `test <name> ... FAILED` per failure and a `test result:` line.
FAILED_TEST = re.compile(r"^test (\S+) \.\.\. FAILED$", re.M)
SUMMARY = re.compile(r"^test result: \w+\. \d+ passed; (\d+) failed", re.M)
#: `--list` prints `search::tests::a_match_is_found_where_it_is: test`.
LISTED_TEST = re.compile(r"^(\S+): test$", re.M)


def run_tests(only: str | None = None) -> tuple[set[str], int | None, str]:
    """Runs the suite, returning failed names, the summary count and the log.

    `only` narrows libtest's filter to the one test a mutation names, and that
    is what makes a full table affordable. The twelve tests that reach macOS
    frameworks -- `save.rs` and `print.rs`'s third-parser checks, and
    `keylayout.rs` -- take 17 to 40 s each while the other 595 take 0.6 s
    between them, so running everything for every mutation spends ~35 s to
    exercise one assertion. Measured 2026-08-21.

    The caller runs the full suite anyway whenever the narrow one finds nothing
    red, which is the case where the diagnostic matters: SURVIVED and "something
    else caught it" are different findings and both need the whole set.
    """
    done = subprocess.run(
        ["cargo", "test", "--lib", "--", *([only] if only else FILTERS)],
        cwd=CRATE,
        env=CARGO_ENV,
        capture_output=True,
        text=True,
        # As in `mutate_frontend.py`: `text=True` alone decodes with the locale
        # codec, cp1252 on Windows. `search.rs` holds characters whose UTF-8
        # contains byte 0x81, which is undefined there, and a failing test that
        # echoes one would take the whole harness down mid-run rather than
        # reporting a survivor.
        encoding="utf-8",
        errors="replace",
        timeout=900,
    )
    out = done.stdout + done.stderr
    # Split on the marker and take the name -- never a fixed column.
    names = set(FAILED_TEST.findall(out))
    summary = SUMMARY.search(out)
    counted = int(summary.group(1)) if summary else None
    return names, counted, out


def all_test_names() -> set[str]:
    """Every test the filter selects, from libtest's own listing."""
    done = subprocess.run(
        ["cargo", "test", "--lib", "--", *FILTERS, "--list"],
        cwd=CRATE,
        env=CARGO_ENV,
        capture_output=True,
        text=True,
        timeout=900,
    )
    out = done.stdout + done.stderr
    return {m for m in LISTED_TEST.findall(out)}


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
    # release selects affected behaviour, and re-proving ninety-odd mutations
    # that could not have moved is an hour of somebody waiting.
    # The control run and the name cross-check below still run in full.
    # **Repeatable, and every value must match something.** It was a plain
    # string option until 2026-09-01, so argparse kept the LAST of several and
    # silently discarded the rest --- three `--only` flags selected one mutation
    # and the run ended `[OK] all 1 mutations caught by the test named for
    # them`, which is a confident pass for two mutations that never executed.
    # The existing empty-selection guard could not see it, because the selection
    # was not empty. So the guard is per filter: a value matching nothing is
    # refused by name, whether or not the others matched.
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        metavar="TEXT",
        help="run mutations whose name contains this; repeatable",
    )
    # For the loop while a change is being made, where `--only` needs you to
    # know the mutation names and this needs you to know nothing. It selects the
    # mutations whose FILE the diff touched, which is sound as far as it goes:
    # every test these name is `#[cfg(test)]` in the file it mutates.
    #
    # **It does not go as far as the whole table, and the difference is not
    # scope but reach.** A change in `docmodel.rs` can decide what `save.rs`
    # does, so a mutation in an untouched file can stop being caught without
    # that file changing. Include affected callers when selecting release scope
    # (BUILD.md); the count left out is printed rather than implied.
    parser.add_argument(
        "--since",
        default="",
        metavar="REF",
        help="only mutations in files changed since REF (plus the working tree)",
    )
    # Written for the kill this table keeps meeting: it is measured in tens of
    # minutes, and a process that dies loses every verdict it had proved *and*
    # leaves its current mutation in the tree, because the restore below is in a
    # `finally` and a `finally` does not survive a kill. See
    # `mutation_resume.py` for what makes a stored verdict reusable, and note
    # that the recovery half runs whether or not this flag is passed.
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
    unmatched = [
        f
        for f in args.only
        if not any(f.lower() in m.name.lower() for m in MUTATIONS)
    ]
    if unmatched:
        for f in unmatched:
            print(f"[FAIL] no mutation matches {f!r}")
        print(
            "[FAIL] a filter that selects nothing is a filter that proved nothing, "
            "and the mutations the other filters did select do not make up for it"
        )
        return 1
    chosen = [
        m
        for m in MUTATIONS
        if not args.only or any(f.lower() in m.name.lower() for f in args.only)
    ]
    if args.since:
        # The prefix is a string, not `Path("src-tauri") / m.path`, and that is
        # the whole of what was wrong with this for as long as it existed: git
        # reports forward slashes on every platform, `str(WindowsPath(...))`
        # produces backslashes, and the set membership test therefore matched
        # nothing on Windows. Measured 2026-08-25 -- `--since HEAD~1` over a
        # commit that changed `docinfo.rs`, which 48 mutations aim at, selected
        # **0**. It refused rather than passing, because the guard below treats
        # an empty selection as "proved nothing"; so the flag was useless here
        # rather than misleading, which is the difference that guard makes.
        chosen, code = mutation_since.apply(chosen, args.since, "src-tauri/")
        if code:
            return code
    if args.near:
        chosen, code = mutation_since.apply_near(chosen, args.near, "src-tauri/")
        if code:
            return code
    # Mutations for another platform. `--list` still shows them, marked: the
    # table is the same everywhere and what differs is which rows this machine
    # can execute, so a listing that silently dropped them would make two
    # machines look like they disagree about the table.
    elsewhere = [m for m in chosen if m.only_on and m.only_on != HERE]

    if args.list:
        for mutation in chosen:
            aside = f"   [{mutation.only_on} only]" if mutation in elsewhere else ""
            print(f"{mutation.name}  ->  expects: {mutation.expect}{aside}")
        return 0
    if not chosen:
        # Not exit 0. "Nothing to run" and "everything passed" are different
        # facts, and a caller reading only the status must not be told the
        # second when this is the first.
        if args.since:
            print(
                f"[FAIL] no mutation is aimed at anything that changed since "
                f"{args.since!r} -- this run proved nothing, which is not the "
                "same as a green table"
            )
        else:
            print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    for mutation in elsewhere:
        print(f"[SKIP] {mutation.name}: {mutation.only_on} only, and this is {HERE}")
    chosen = [m for m in chosen if m not in elsewhere]
    if not chosen:
        print(f"[FAIL] every mutation matching {args.only!r} is for another platform")
        return 1

    # Before the control run, and before the fingerprint: a tree still holding
    # somebody else's mutation is neither green nor the tree these verdicts
    # would be about.
    resume = mutation_resume.Resume("rust", reuse=args.resume)
    code, lines = resume.recover()
    for line in lines:
        print(line, flush=True)
    if code:
        return code
    for line in resume.open(chosen):
        print(line, flush=True)

    print("--- control: the suite must be green before anything is broken", flush=True)
    names, counted, out = run_tests()
    if counted is None:
        print("[FAIL] the control run produced no summary line, so nothing below is readable")
        print(out[-2000:])
        return 1
    if counted != 0 or names:
        print(f"[FAIL] the control run is not green: {counted} failed, {sorted(names)}")
        return 1
    print("[OK]   control green", flush=True)

    # The same cross-check the front-end harness carries, and for the same
    # reason: an `expect` naming a test that does not exist cannot go red, so the
    # run prints SURVIVED and the fault reads as a gap in the suite. Derived from
    # libtest's own list rather than from a hand-kept table.
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

        Every outcome below goes through this, so a run that is killed a moment
        later has already recorded everything it decided. The return value is
        what the caller adds to `problems`, which keeps the two from drifting.
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
            # the same claim about the tree and not the same run --- and a
            # transcript that cannot tell them apart is where somebody
            # reads "all caught" for a table that was never executed here.
            print(f"{stored.line}   [reused]", flush=True)
            problems += tally(stored.ok)
            continue
        target = CRATE / mutation.path
        # Bytes, decoded explicitly. `read_text` uses the locale codec,
        # and on Windows that is cp1252, which does not merely mangle
        # this file -- it *cannot read it*: search.rs holds `İ` and `ﬁ`
        # for the case-folding tests, whose UTF-8 encodings contain the
        # byte 0x81, and cp1252 leaves 0x81 undefined. So this raised
        # UnicodeDecodeError on the first mutation and the harness never
        # ran here at all.
        #
        # The newlines are normalised for matching only, because the
        # anchors are written with "\n" and eight of them span lines.
        # The file's own convention goes back on the way out, and the
        # restore below is bytes, as docs/TRAPS.md requires.
        #
        # **This said "a Windows checkout is CRLF", and since 2026-08-26
        # it is not**: `.gitattributes` pins `* text=auto eol=lf`, so
        # every text file checks out LF on every platform. Kept anyway,
        # and not as ceremony -- a checkout is not the only way a file
        # gets written, and a tool that rewrites one in text mode on
        # Windows produces CRLF whatever git checked out. The branch is
        # two lines and its absence cost this harness every multi-line
        # anchor once already.
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
        # the backup that outlives this process and enforces the three rules
        # this harness paid for one at a time: the record is written before
        # the bytes, the restore writes bytes rather than moving or copying
        # a file over them, and the restored file is left newer than the
        # mutation so cargo does not go on serving it. `docs/TRAPS.md` has
        # each of them. The anchor check above happens before any of it, so
        # the only path under the `finally` is one where a mutation is
        # really on the tree.
        resume.apply(mutation, target, clean, payload)
        try:
            # The test this mutation names, first and alone. When it goes
            # red -- which is the whole table's expected outcome -- the
            # verdict is settled and the run stops there.
            names, counted, out = run_tests(mutation.expect)
            narrow = True
            if counted is not None and not names:
                # It did not. Now the full set matters: "nothing noticed"
                # and "something else noticed" are different findings, and
                # the second one prints which tests went red instead.
                names, counted, out = run_tests()
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
            # Almost always a compile error, which produces no failing-test
            # lines at all -- indistinguishable from a survivor without this.
            first = next(
                (line for line in out.splitlines() if line.startswith("error")), ""
            )
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: no summary line -- the run did not finish"
                + (f" ({first})" if first else ""),
            )
            continue
        if len(names) != counted:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: {len(names)} failing test lines but the summary "
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
        # The scope is printed because the count means different things in
        # the two cases: `1 red` out of the one test named for the mutation
        # is not the same statement as `1 red` out of 607.
        scope = "the test named for it" if narrow else f"{len(known)} tests"
        verdict = [
            f"{mark} {mutation.name}: {counted} red of {scope}"
            + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
        ]
        if not hit:
            verdict.append(f"         red instead: {sorted(names)}")
        problems += say(mutation, hit, *verdict)

    print()
    # The skip count rides on the verdict rather than sitting in a line above it.
    # A run that could not execute part of its own table must not read, three
    # scrollbacks later, like one that executed all of it.
    aside = f", {len(elsewhere)} skipped as not runnable on {HERE}" if elsewhere else ""
    for line in resume.closing():
        print(line)
    print(
        f"[OK] all {len(chosen)} mutations caught by the test named for them{aside}"
        if problems == 0
        else f"[FAIL] {problems} of {len(chosen)} mutations were not caught as described{aside}"
    )
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
