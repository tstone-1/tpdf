#!/usr/bin/env python3
"""Break each gate's subject on purpose, and require the gate to notice.

`scripts/gates.py` runs sixteen gates whose implementation is a Python script.
Every one of them was proved able to fail on the day it was written, by hand, and
until 2026-09-02 **nothing re-proved any of them afterwards**: no test file named
one, and neither `mutate_rust.py` nor `mutate_frontend.py` carried a mutation
aimed at one --- both mention `check_*` only in prose. A gate is a check, and this
repository's whole position on checks is that one which has never been shown to
fail is indistinguishable from one that keeps passing.

That is not a hypothetical here. `check_workflow_fixtures.py` scanned lines whose
stripped form starts with `run:`, which is the first line of a `run: |` step and
none of the commands inside it, so every fixture path written in the ordinary
multi-command style was invisible to the gate written to catch exactly that
mistake. It had been half-blind since the day it was written and every run of it
was green. `docs/TRAPS.md` has the entry.

WHAT A MUTATION HERE IS. Not an edit to the checker --- an edit to the checker's
**subject**: the workflow, the trap index, the toolchain pin, the source file.
Planting the defect a gate exists to catch and requiring the gate to go red is the
only thing that establishes it can. Breaking the checker instead would answer a
different and much weaker question, because a checker with its matcher removed
reports a clean tree, which is the failure rather than the proof.

WHY EVERY ROW CARRIES `says`. A gate that goes red for the wrong reason passes an
exit-code test exactly as well as one that caught the plant. Each mutation
therefore names a string that must appear in the gate's output, and the harness
refuses a red whose message does not contain it. This is the same rule as
`docs/TRAPS.md`'s *a generic `raises(Exception)` cannot fail for the right
reason*.

WHY SOME ROWS EXPECT GREEN. A gate that reads too much passes the obvious
mutation just as well as one that reads correctly. The block-scalar fix is the
worked example: "scan everything after a `run:`" catches a bogus path in the block
and is wrong, and the mutation that separates it from the correct walk is a
*dedented* line that must **not** be read. Such a row asserts green plus a `says`
carrying the count, because a green run on its own is also what a gate that
stopped looking produces.

WHAT IS COVERED: fifteen of the sixteen Python-backed gates, as of 2026-09-02.
The exception is `bundleshare`, and the reason is measured rather than assumed.
It needs two things this harness does not do. Its subject is the *built* bundle,
so a mutation would have to run `npm run build` between the edit and the gate ---
each row here runs one gate and nothing else. And the obvious edit does not work:
a 200,000-character exported constant added to `viewercheck.ts` left the family at
151,362 units, unchanged, because Vite tree-shakes an export nothing imports. A
row that reaches it has to grow code that is actually *reached*. Stated this
precisely so the next attempt starts from the second problem rather than the
first, and because a harness that covers most of its population and says nothing
about the rest is the shape this repository keeps finding.

WHAT IT FOUND ON ITS FIRST TWO RUNS. `fetch_pdfium.py --check` printed `TAG` and
never compared it against the installed tree, so it answered `[OK] pdfium
chromium/9999 ... verified` for a tree stamped 7881. And two rows survived as
*variants rather than gaps* --- a doc comment planted at line 1, which is the one
position `check_doc_comments.py` exempts, and a renamed workflow step, which
`check_workflow_parity.py` deliberately ignores because it compares what steps
execute. Both were re-aimed rather than used to strengthen a gate that was right.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import live_output  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
GATES = ROOT / "scripts" / "gates.py"

#: A date that has not happened, assembled rather than written down.
#:
#: `scripts/check_dates.py` scans every tracked file for a date later than
#: today, and this file is tracked. So the `dates` mutation below cannot carry
#: its own future date as a literal --- doing so put next year's date into the tree
#: and the gate reported it, correctly, which made the harness's control refuse
#: to run. Same year and day, one year on, so it is in the future whenever this
#: is read and matches nothing when the file is at rest.
AHEAD = f"{date.today().year + 1}-{date.today():%m-%d}"


@dataclass(frozen=True)
class Mutation:
    """One edit to a gate's subject, and what the gate must then say."""

    name: str
    #: The gate in `scripts/gates.py`, run with `--gate`. Exactly one, so a red
    #: cannot come from a neighbour that happens to read the same file.
    gate: str
    #: The file to edit, relative to the repository root.
    path: str
    before: str
    after: str
    #: True when the gate must reject the mutated tree. False for an over-reach
    #: control, where the gate must stay green.
    red: bool
    #: A substring the gate's output must contain, whichever way `red` goes.
    #: Never optional: an exit code alone cannot say the gate failed for the
    #: reason the mutation planted, and a bare green cannot say it looked.
    says: str
    #: Environment to set for the gate's own process, for a gate whose subject
    #: is the environment rather than a file. `check_toolchain.py` is the one:
    #: what it exists to catch is `RUSTUP_TOOLCHAIN` silently overriding the
    #: pin, and there is no edit to any tracked file that produces that.
    #:
    #: A row with `env` set carries no anchor --- `path`, `before` and `after`
    #: are empty --- and `check_mutation_anchors.py` skips it on that condition,
    #: guarded so the skip cannot smuggle through an unanchored file mutation.
    env: "dict[str, str] | None" = None


#: The `run: |` step in `ci.yml` whose interior these mutations plant into. The
#: `run:` key sits at eight columns, so a line at twelve is inside the block and
#: one at eight is not --- which is the whole of the distinction the last of the
#: three tests.
CI_BLOCK = "            brew install qpdf\n"

MUTATIONS = [
    # --- the fixture gate, and the blindness that started this file ----------
    Mutation(
        "fixtures: name an ungeneratable fixture inside a `run: |` block",
        "fixtures",
        ".github/workflows/ci.yml",
        CI_BLOCK,
        CI_BLOCK + "            ./probe testdata/no-such-fixture.pdf\n",
        red=True,
        says="testdata/no-such-fixture.pdf",
    ),
    Mutation(
        # Green, and the count is the point: 6 to 7 says the planted line was
        # *read*. Without this row the one above is satisfied by a gate that
        # rejects the whole file for some unrelated reason.
        "fixtures: a real fixture inside the block must be read, not merely tolerated",
        "fixtures",
        ".github/workflows/ci.yml",
        CI_BLOCK,
        CI_BLOCK + "            ./probe testdata/comments.pdf\n",
        red=False,
        says="all 7 workflow fixture path(s)",
    ),
    Mutation(
        # The over-reach control. Eight columns is the `run:` key's own indent,
        # so the block has ended and this line is not a command in it. A gate
        # that simply scanned to the end of the file would go red here.
        "fixtures: a line dedented out of the block must not be read",
        "fixtures",
        ".github/workflows/ci.yml",
        CI_BLOCK,
        CI_BLOCK + "        ./dedented testdata/no-such-fixture.pdf\n",
        red=False,
        says="all 6 workflow fixture path(s)",
    ),
    # --- the trap index, in both directions ----------------------------------
    Mutation(
        "traps: an entry in docs/TRAPS.md that nothing in the index points at",
        "traps",
        "AGENTS.md",
        "- A `run:` line is not a `run:` step, and the emptiness control could not tell the difference\n",
        "",
        red=True,
        says="run:",
    ),
    Mutation(
        "traps: an index bullet naming an entry that does not exist",
        "traps",
        "AGENTS.md",
        "- A `run:` line is not a `run:` step, and the emptiness control could not tell the difference\n",
        "- A trap entry nobody ever wrote, which docs/TRAPS.md cannot possibly hold\n",
        red=True,
        says="A trap entry nobody ever wrote",
    ),
    # --- a date that has not happened yet ------------------------------------
    Mutation(
        # **The date is computed, never written.** A literal future date here is
        # a future date in a tracked file, which is precisely what `check_dates`
        # scans for --- so the row that tests the gate becomes a defect the gate
        # reports, and the harness's control refuses to start. Written as a
        # literal first, and it turned `dates` red on a clean tree.
        #
        # This is the trap about a scanner reading its own exemption table,
        # arriving through a mutation table rather than an allowlist. The fix is
        # to keep the string out of the file: `AHEAD` is built at import.
        "dates: a stamp in the future, which is how 70 of them shipped at once",
        "dates",
        "docs/TRAPS.md",
        "Measured 2026-09-02 by planting",
        f"Measured {AHEAD} by planting",
        red=True,
        says=AHEAD,
    ),
    # --- the toolchain pin ---------------------------------------------------
    # --- the toolchain pin, and the mutation that could not have worked -------
    #
    # **Editing the channel does not test this gate, and trying it installs a
    # toolchain.** The obvious mutation --- change the pin to a version that is
    # not running --- was written first and reported SURVIVED. It had to: rustup
    # *selects* rustc from `rust-toolchain.toml`, so with the file changed to
    # 1.70.0 the gate printed `pinned=1.70.0  rustc=1.70.0` and passed
    # correctly. The comparison is between the pin and a thing the pin decides,
    # which is this repository's *a writer and its own reader agree* arriving in
    # a gate. It also downloaded and installed a complete 1.70.0 toolchain as a
    # side effect, which had to be uninstalled afterwards --- a mutation with a
    # 300 MB footprint outside the repository is its own reason to check what a
    # row actually perturbs before adding it.
    #
    # What the gate does claim is narrower and worth reading exactly: that the
    # rustc *running* is the pinned one. The only thing that makes those differ
    # is `RUSTUP_TOOLCHAIN`, which is why the mutation is environmental. Nightly
    # rather than a version string because it is already installed here for
    # `cargo fuzz`, so this row downloads nothing.
    Mutation(
        "toolchain: RUSTUP_TOOLCHAIN set, which overrides the pin silently",
        "toolchain",
        "",
        "",
        "",
        red=True,
        says="RUSTUP_TOOLCHAIN=nightly",
        env={"RUSTUP_TOOLCHAIN": "nightly"},
    ),
    # --- the PDFium pin, and the second thing this harness found --------------
    #
    # `--check` printed `TAG` and never compared it against the tree. On
    # 2026-09-02 this row reported SURVIVED with the gate answering
    # `[OK] pdfium chromium/9999 mac-arm64 verified` for an install stamped
    # 7881. The archive digest below it was doing the real work and still is, so
    # nothing was unverified --- what was wrong is that the gate's own output
    # named a version nothing had checked, and the docstring's rule that TAG and
    # PINS move together had nothing enforcing it. `check()` now compares
    # `VERSION.txt`.
    Mutation(
        "pdfium: a TAG the installed tree was not built from",
        "pdfium",
        "scripts/fetch_pdfium.py",
        'TAG = "chromium/7881"',
        'TAG = "chromium/9999"',
        red=True,
        says="chromium/9999",
    ),
    # --- a doc comment bound to nothing ---------------------------------------
    Mutation(
        # Two `/** */` blocks in a row: JSDoc binds only the second, silently,
        # and the first documents nothing. The gate's founding scan found 31.
        #
        # **Aimed mid-file on purpose.** Written first against the block at line
        # 1, it reported SURVIVED --- correctly: the module header is the gate's
        # one structural exception, because a file's own header is followed by
        # the first declaration's doc comment in every well-formed module here.
        # A mutation aimed at the single position a rule exempts is the trap
        # about a mutation aimed at code no fixture reaches.
        "docs: a doc comment with another doc comment under it",
        "docs",
        "src/lib/a11y.ts",
        "export function elementFor(tag: string | null): string {",
        "/**\n * An orphan: the block below binds, and this one renders nowhere.\n */\n/**\n * Re-declared for the mutation.\n */\nexport function elementFor(tag: string | null): string {",
        red=True,
        says="a11y.ts",
    ),
    # --- a markup sink in the frontend ----------------------------------------
    Mutation(
        # `docs/THREAT-MODEL.md` T8: no markup-parsing sink anywhere in the
        # frontend, because document text reaches these modules.
        "sinks: an innerHTML assignment in a frontend module",
        "sinks",
        "src/lib/a11y.ts",
        "/**\n * What a screen reader finds when it reaches the document.",
        "const planted = document.body;\nplanted.innerHTML = 'x';\n/**\n * What a screen reader finds when it reaches the document.",
        red=True,
        says="innerHTML",
    ),
    # --- the two workflows drifting apart -------------------------------------
    Mutation(
        # The founding defect: `release.yml`'s gates job was copied from
        # `ci.yml` and dropped a step, so the release gate was weaker than the
        # gate it exists to satisfy.
        # **A `run:` body, not a step name.** Written first as a rename, it
        # reported SURVIVED --- correctly, because the check compares what the
        # steps *execute* and says so: two step names describing one command are
        # prose. What it exists to catch is the two jobs doing different work.
        "workflows: the gates job running a different command in release.yml",
        "workflows",
        ".github/workflows/release.yml",
        "            brew install qpdf\n",
        "            brew install qpdf --without-the-parity\n",
        red=True,
        says="qpdf",
    ),
    # --- an anchor aimed at nothing -------------------------------------------
    Mutation(
        # The gate that guards the other three harnesses. A drifted anchor is
        # invisible in `git status` and the harness only notices when it reaches
        # that row, which is an hour in.
        "anchors: a mutation anchor that no longer occurs in the file it names",
        "anchors",
        "scripts/mutate_python.py",
        'CI_BLOCK = "            brew install qpdf\\n"',
        'CI_BLOCK = "            brew install a package that is not there\\n"',
        red=True,
        says="anchor occurs",
    ),
    # --- the list that answers "how many ways can the webview cause a write" --
    Mutation(
        # `docs/THREAT-MODEL.md` §3's marker is the claim, and the row's count
        # follows it. It was wrong three times in two weeks, always
        # under-claiming, before the gate existed.
        "writers: a writing command missing from the threat model's list",
        "writers",
        "docs/THREAT-MODEL.md",
        "<!-- writers: save_copy save_document extract_pages split_document merge_documents print_document redact_copy redact_document -->",
        "<!-- writers: save_copy save_document extract_pages split_document merge_documents print_document redact_document -->",
        red=True,
        says="redact_copy",
    ),
    # --- a command the window harness neither drives nor excuses --------------
    Mutation(
        # Two commands shipped unclassified on 2026-08-29 because the harness
        # asserting this needs a screen and is run by hand.
        "classified: a registered command in neither probes nor undriven",
        "classified",
        "src/lib/viewercheck.ts",
        # **Two lines, because the id alone occurs twice**: once where the
        # harness registers the command and once in the probe that drives it.
        # The probe is the half that classifies, so the anchor carries the
        # `from:` line under it to name that one.
        '      id: "view.fitWidth",\n      from: () => viewer.setFit("page"),',
        '      id: "view.fitWidthUnclassified",\n      from: () => viewer.setFit("page"),',
        red=True,
        says="view.fitWidth",
    ),
    # --- a callback the component declares and App.svelte never supplies ------
    Mutation(
        # The founding defect: the box shipped inert with three layers of tests
        # green, because nothing looks at the object literal that joins them.
        "wiring: an optional callback dropped from App.svelte's literal",
        "wiring",
        "src/App.svelte",
        "        onMarkRemove: (mark) => void applyEdit((e) => e.unmark(mark)),\n",
        "",
        red=True,
        says="onMarkRemove",
    ),
    # --- a corpus with no stated purpose -------------------------------------
    Mutation(
        # The gate is a set difference between `testdata/*.pdf` and this list,
        # so dropping an entry is the same question from the other side as
        # adding an unclassified fixture --- and it needs no PDF written.
        "corpora: a window corpus with no stated purpose",
        "corpora",
        "scripts/viewer_sweep.py",
        '    ("outline-simple", "the only fixture with an ordinary outline"),\n',
        "",
        red=True,
        says="outline-simple",
    ),
    # --- a vitest suite nobody mutates and nobody excused --------------------
    Mutation(
        # Twelve seconds here against a control pass in the front-end harness,
        # which is the whole reason this gate exists.
        "mutations: a test suite neither mutated nor excluded with a reason",
        "mutations",
        "scripts/mutate_frontend.py",
        "UNMUTATED = {",
        "UNMUTATED = {} if True else {",
        red=True,
        says="UNMUTATED",
    ),
    # --- the notices file going stale ----------------------------------------
    Mutation(
        # A hand-maintained notices file is wrong the first time a dependency
        # changes and nothing says so, which is why this is generated.
        "notices: a THIRD-PARTY-NOTICES.md that no longer matches the tree",
        "notices",
        "THIRD-PARTY-NOTICES.md",
        "# Third-party notices\n",
        "# Third-party notices, edited by hand so the file is now stale\n",
        red=True,
        says="THIRD-PARTY-NOTICES.md",
    ),
    # --- the population, not the defect --------------------------------------
    #
    # Every other row here plants a defect in a file the gate already reads, so
    # none of them can see a gate whose *population* shrank to nothing. This one
    # asks the other question, and it found `check_dates.py` answering
    # `[OK] no date ahead of 2026-09-02 in 0 tracked text files` and exiting 0 ---
    # the count printed honestly and asserted by nothing.
    #
    # `GIT_INDEX_FILE` at a path that does not exist makes `git ls-files` print
    # nothing and exit 0, which is the case the error branch cannot catch: git
    # succeeded. No fixture and no temporary repository, so it is portable.
    Mutation(
        "dates: a tracked-file listing that succeeds and is empty",
        "dates",
        "",
        "",
        "",
        red=True,
        says="listed no file at all",
        env={"GIT_INDEX_FILE": "/tmp/tpdf-no-such-index"},
    ),
]


def gate(name: str, env: "dict[str, str] | None" = None) -> tuple[int, str]:
    """Run one gate and return its status and its combined output."""
    environ = dict(os.environ)
    if env:
        environ.update(env)
    done = subprocess.run(
        [sys.executable, str(GATES), "--gate", name],
        cwd=ROOT,
        capture_output=True,
        text=True,
        env=environ,
    )
    return done.returncode, done.stdout + done.stderr


def main() -> int:
    # Before anything prints: a redirected run is block-buffered otherwise, and
    # a harness that writes nothing until it exits is one whose transcript is
    # lost the moment it is interrupted.
    live_output.stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    # Repeatable, and every value must match something. A plain string option
    # keeps the LAST of several and silently discards the rest, which is how
    # `mutate_rust.py` once reported `all 1 mutations caught` for three filters.
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        metavar="TEXT",
        help="run mutations whose name contains this; repeatable",
    )
    args = parser.parse_args()

    unmatched = [f for f in args.only if not any(f.lower() in m.name.lower() for m in MUTATIONS)]
    if unmatched:
        for f in unmatched:
            print(f"[FAIL] no mutation matches {f!r}")
        print(
            "[FAIL] a filter that selects nothing is a filter that proved nothing, "
            "and the mutations the other filters did select do not make up for it"
        )
        return 1
    chosen = [m for m in MUTATIONS if not args.only or any(f.lower() in m.name.lower() for f in args.only)]

    if args.list:
        for mutation in chosen:
            want = "red" if mutation.red else "green"
            print(f"{mutation.name}  ->  {mutation.gate} must be {want}, saying: {mutation.says}")
        return 0
    if not chosen:
        # Not exit 0. "Nothing to run" and "everything passed" are different
        # facts, and a caller reading only the status must not be told the
        # second when this is the first.
        print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    # The control, and it is per gate rather than for the suite: a gate already
    # red on a clean tree makes every `red=True` row below pass for free, which
    # is a harness agreeing with itself.
    print("--- control: every gate under test must be green before anything is broken", flush=True)
    for name in sorted({m.gate for m in chosen}):
        code, out = gate(name)
        if code != 0:
            print(f"[FAIL] gate {name!r} is already red on a clean tree, so nothing below is readable")
            print(out[-2000:])
            return 1
        print(f"[OK]   control green: {name}", flush=True)

    problems = 0
    with tempfile.TemporaryDirectory(prefix="tpdf-mutate-py-") as scratch:
        for index, mutation in enumerate(chosen):
            if not mutation.before:
                # An environment mutation: nothing on disk moves, so there is
                # nothing to back up and nothing to restore. Guarded on `env`
                # rather than on the empty anchor alone, because a file mutation
                # that lost its anchor would otherwise land here and be reported
                # as a clean pass.
                if not mutation.env:
                    print(
                        f"[FAIL] {mutation.name}: no anchor and no env -- this row "
                        "perturbs nothing and cannot fail"
                    )
                    problems += 1
                    continue
                code, out = gate(mutation.gate, mutation.env)
                caught = (code != 0) if mutation.red else (code == 0)
                if caught and mutation.says in out:
                    print(f"[OK]   {mutation.name}", flush=True)
                else:
                    print(
                        f"[FAIL] {mutation.name}: gate {mutation.gate!r} exited {code} "
                        f"and {'said' if mutation.says in out else 'never said'} "
                        f"{mutation.says!r}",
                        flush=True,
                    )
                    problems += 1
                continue
            target = ROOT / mutation.path
            # Copied aside and written *back*, never moved: `docs/TRAPS.md`
            # records a restore-by-move that left the mutated build in place.
            backup = Path(scratch) / f"{index}.bak"
            shutil.copy2(target, backup)
            try:
                # Bytes, decoded explicitly. `read_text` uses the locale codec,
                # and these subjects hold em dashes and other non-ASCII that
                # cp1252 cannot round-trip. Newlines are normalised for matching
                # only, because the anchors are written with "\n" and several
                # span lines; the file's own convention goes back on the way out.
                raw = target.read_bytes().decode("utf-8")
                crlf = "\r\n" in raw
                flat = raw.replace("\r\n", "\n")
                found = flat.count(mutation.before)
                if found != 1:
                    print(
                        f"[FAIL] {mutation.name}: anchor occurs {found}x in "
                        f"{mutation.path}, expected 1 -- this mutation is aimed at nothing"
                    )
                    problems += 1
                    continue
                edited = flat.replace(mutation.before, mutation.after)
                out_text = edited.replace("\n", "\r\n") if crlf else edited
                target.write_bytes(out_text.encode("utf-8"))

                code, out = gate(mutation.gate)
                caught = (code != 0) if mutation.red else (code == 0)
                spoke = mutation.says in out
                if caught and spoke:
                    print(f"[OK]   {mutation.name}", flush=True)
                elif not caught:
                    want = "red" if mutation.red else "green"
                    print(
                        f"[FAIL] {mutation.name}: gate {mutation.gate!r} exited "
                        f"{code}, and this mutation requires it to be {want}",
                        flush=True,
                    )
                    problems += 1
                else:
                    print(
                        f"[FAIL] {mutation.name}: gate {mutation.gate!r} went the right "
                        f"way but never said {mutation.says!r} -- so it did not do so "
                        "for the reason this mutation planted",
                        flush=True,
                    )
                    problems += 1
            finally:
                # Written back rather than copied back: `shutil.copy2` preserves
                # the backup's mtime, and a restored file older than something
                # built from the mutated one serves the mutation.
                target.write_bytes(backup.read_bytes())

    if problems:
        print(f"\n[FAIL] {problems} of {len(chosen)} mutations were not caught by the gate named for them")
        return 1
    print(f"\n[OK] all {len(chosen)} mutations caught by the gate named for them")
    return 0


if __name__ == "__main__":
    sys.exit(main())
