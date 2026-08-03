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

  * anything from `make_text_pdf.py` (text-heavy, outline-hostile,
    hostile-filters) --- it needs fonttools and embeds a *system* font, which
    differs per runner and would make fixture-dependent assertions depend on the
    image.
  * `hostile-*.pdf` from `make_hostile_pdf.py` --- it shells out to qpdf, which
    is not installed on a hosted runner.
  * `incr-scan-5p.pdf` --- `make_incremental_pdf.py` writes ~550 MB on purpose
    and needs pyhanko.

Tests wanting those still skip on a runner and are covered locally, per
`BUILD.md`. The two below are the dependency-free ones and cost about half a
second.

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
    (
        "testdata/vector-multi.pdf",
        ["testdata/make_vector_pdf.py", "testdata/vector-multi.pdf", "200000", "12"],
    ),
]


def check() -> int:
    """Reports which fixtures are present. Non-zero if any is missing."""
    missing = 0
    for artifact, _ in FIXTURES:
        path = ROOT / artifact
        if path.exists():
            print(f"[OK]   {artifact} ({path.stat().st_size:,} bytes)")
        else:
            print(f"[FAIL] {artifact} is absent")
            missing += 1
    return 1 if missing else 0


def generate() -> int:
    """Runs every generator. A generator that fails stops the run."""
    for artifact, argv in FIXTURES:
        print(f"[..]   {artifact}", flush=True)
        result = subprocess.run([sys.executable, *argv], cwd=ROOT, check=False)
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
    args = parser.parse_args()
    return check() if args.check else generate()


if __name__ == "__main__":
    sys.exit(main())
