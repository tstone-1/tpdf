#!/usr/bin/env python3
"""Generates the test fixtures a hosted runner can build, for every workflow.

`testdata/*.pdf` is gitignored and generated, and several unit tests read a
fixture and skip when it is absent --- correctly, but a run where every fixture
is missing then looks exactly like a run where every one passed. `print.rs`
guards that with `assert!(examined > 0)`, so a workflow that never generates
anything goes red rather than quietly testing nothing.

That assertion has now caught the same gap twice, which is why this script
exists rather than a third copy of the list. The first time was CI's own first
run. The second was 2026-08-03: `release.yml`'s gates job was written from
`ci.yml` and lost the fixture step in the copy, so
`a_third_parser_checks_a_job_built_from_a_document_we_did_not_write` --- which
needs `rotated.pdf` --- failed on the release runner while passing on the CI
runner and locally, and the *release* gate was therefore weaker than the gate it
exists to satisfy. `AGENTS.md` records that rule about hand-copied commands
losing a flag; here the copy lost an entire step, which is the same failure with
more of it missing.

So both workflows call this, and the list of what a runner generates lives in
exactly one place. Adding a fixture here reaches every workflow at once.

WHAT THIS DELIBERATELY DOES NOT GENERATE, so the gap is stated rather than
discovered:

  * anything from `make_text_pdf.py` (text-base14, text-truetype, text-cid,
    text-marked) --- it needs fonttools and embeds a *system* font, which differs
    per runner and would make fixture-dependent assertions depend on the image.
  * `outline-hostile.pdf` from `make_outline_pdf.py`, which needs fonttools for
    the same reason. Note `outline-simple.pdf` comes from the same script.
  * `text-heavy.pdf`, which **no script writes**: it is a real document, supplied
    by hand, and `BUILD.md` says so. A runner cannot have it and neither can a
    fresh checkout.

    Those three lines read "anything from `make_text_pdf.py` (text-heavy,
    outline-hostile, hostile-filters)" until 2026-08-19, and that script writes
    none of the three. The reading it invited is the expensive one: that
    `text-heavy.pdf` is generatable and merely excluded here, when in fact
    nothing anywhere can produce it.
  * `hostile-*.pdf` from `make_hostile_pdf.py` --- it shells out to qpdf, which
    is not installed on a hosted runner.
  * `incr-scan-5p.pdf` --- `make_incremental_pdf.py` writes ~550 MB on purpose
    and needs pyhanko.

Tests wanting those still skip on a runner and are covered locally, per
`BUILD.md`. The four scripts below are the dependency-free ones and cost about
half a second. ("The three" until 2026-08-21, which had been wrong since
`make_links_pdf.py` joined them --- a count in prose with nothing asserting it.)

`--signed` adds a fifth group, and it is the one that needs something installed:
`make_incremental_pdf.py` and pyhanko, for the nine fixtures carrying a real
signature and the two carrying real encryption. Without that flag nothing here needs anything but the standard
library, which is what the paragraph above promises. `make_comments_pdf.py` imports `make_text_pdf.py` for its PDF writer and
is still dependency-free: that module reaches for fonttools inside the function
that embeds a font, which nothing here calls. `make_links_pdf.py` imports it
for the same writer and is dependency-free for the same reason.

Usage: scripts/ci_fixtures.py [--check]

  --check  report which of the fixtures exist and exit non-zero if any is
           missing, without generating anything. For asking whether a machine
           is ready rather than making it so.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

#: Repository root, from this file rather than from the working directory --- a
#: workflow runs from the root but a person may not.
ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Each entry is the artifact and the command that produces it, argv-style so no
#: shell is involved. The interpreter is this one, so a venv is honoured.
FIXTURES: list[tuple[str, list[str]]] = [
    ("testdata/rotated.pdf", ["testdata/make_rotated_pdf.py", "testdata"]),
    ("testdata/comments.pdf", ["testdata/make_comments_pdf.py", "testdata"]),
    # Written by the same run as the line above; listed so that a check for
    # its presence is a check for the file rather than for its sibling.
    ("testdata/comments-rotated.pdf", ["testdata/make_comments_pdf.py", "testdata"]),
    ("testdata/links.pdf", ["testdata/make_links_pdf.py", "testdata"]),
    # Pure Python, no dependency and no system font: a runner can build it, and
    # `redact-apply-probe`'s form section skips without it.
    (
        "testdata/form-xobject.pdf",
        ["testdata/make_form_xobject_pdf.py", "testdata/form-xobject.pdf"],
    ),
    # A page tree that states the four inheritable attributes on the node above
    # its pages rather than on them. Nothing else in the corpus does, so every
    # other fixture is blind to a copy that lifts a page out of the tree that
    # supplies its size --- which is what `merge-probe` measures and what a
    # mutation of `pagetree::detached_page` could not fail against before it.
    ("testdata/inherited.pdf", ["testdata/make_inherited_pdf.py", "testdata"]),
    # Written by the same run as the line above; listed so a check for its
    # presence is a check for the file rather than for its sibling.
    ("testdata/links-rotated.pdf", ["testdata/make_links_pdf.py", "testdata"]),
    ("testdata/links-cropped.pdf", ["testdata/make_links_pdf.py", "testdata"]),
    (
        "testdata/vector-multi.pdf",
        ["testdata/make_vector_pdf.py", "testdata/vector-multi.pdf", "200000", "12"],
    ),
]

#: The signed fixtures, which need **pyhanko** and are behind `--signed`.
#:
#: Kept apart from the list above because that one is dependency-free and this
#: one is not: a caller must install pyhanko first, and the flag is what says so
#: out loud rather than leaving a runner to discover it as an ImportError. What
#: does *not* differ is where the list lives --- adding a signed fixture reaches
#: both workflows from here, which is the rule the release job broke once by
#: being written from CI's file rather than calling the same script.
#:
#: Every one comes from a single run of `make_incremental_pdf.py`, and one of its
#: outputs is deliberately not asked for: `incr-scan-*.pdf` is hundreds of
#: megabytes (`--scan-pages` with no values skips it).
#:
#: **The two encrypted fixtures joined this list on 2026-08-23**, when they
#: stopped being built with qpdf. A hosted runner has no qpdf, so until then
#: nothing that needed an encrypted document could be checked anywhere but on a
#: developer machine --- which covered the save path's encryption guard, and that
#: guard was wrong for four weeks with every gate green. pyhanko writes them now,
#: and pyhanko is what this group already installs.
SIGNED: list[tuple[str, list[str]]] = [
    (f"testdata/{name}.pdf", ["testdata/make_incremental_pdf.py", "testdata", "--scan-pages"])
    for name in (
        "incr-encrypted-open",
        "incr-encrypted-pw",
        "incr-signed",
        "incr-certified-1",
        "incr-certified-2",
        "incr-certified-3",
        "incr-certified-3-indirect",
        "incr-timestamped",
        "incr-two-signers",
        "incr-ber",
        "signed-nested-field",
    )
]


def check(wanted: list[tuple[str, list[str]]]) -> int:
    """Reports which fixtures are present. Non-zero if any is missing."""
    missing = 0
    for artifact, _ in wanted:
        path = ROOT / artifact
        if path.exists():
            print(f"[OK]   {artifact} ({path.stat().st_size:,} bytes)")
        else:
            print(f"[FAIL] {artifact} is absent")
            missing += 1
    return 1 if missing else 0


def generate(wanted: list[tuple[str, list[str]]]) -> int:
    """Runs every generator. A generator that fails stops the run."""
    ran: set[tuple[str, ...]] = set()
    for artifact, argv in wanted:
        print(f"[..]   {artifact}", flush=True)
        # One script may write several artifacts, and nine of them come from one
        # run. Each is still checked for on its own below --- what is skipped is
        # re-running a command that has already run in this invocation, not the
        # question of whether it produced the file.
        if tuple(argv) not in ran:
            result = subprocess.run([sys.executable, *argv], cwd=ROOT, check=False)
            ran.add(tuple(argv))
            if result.returncode != 0:
                print(f"[FAIL] {argv[0]} exited {result.returncode}")
                return result.returncode
        path = ROOT / artifact
        if not path.exists():
            # A generator that exits 0 without writing its artifact is the
            # silent case this whole script is about, so it is an error here
            # rather than a surprise in a test three steps later.
            print(f"[FAIL] {argv[0]} exited 0 but {artifact} does not exist")
            return 1
        print(f"[OK]   {artifact} ({path.stat().st_size:,} bytes)")
    return 0


def main() -> int:
    """Parses the one flag and dispatches."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="report which fixtures exist instead of generating them",
    )
    parser.add_argument(
        "--signed",
        action="store_true",
        help="include the signed fixtures, which need pyhanko installed",
    )
    args = parser.parse_args()
    wanted = FIXTURES + SIGNED if args.signed else FIXTURES
    return check(wanted) if args.check else generate(wanted)


if __name__ == "__main__":
    sys.exit(main())
