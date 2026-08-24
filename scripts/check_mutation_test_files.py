#!/usr/bin/env python3
"""Refuses a front-end mutation that cannot go red, before anything is mutated.

WHY THIS EXISTS. `mutate_frontend.py` runs `vitest` over `TEST_FILES`, a
hand-kept list, and every mutation names a test that must go red. A suite absent
from that list still resolves as a name on disk -- it simply never runs -- so a
mutation aimed at it can only report SURVIVED, which reads as a gap in the tests
rather than a mistake in the harness. The harness therefore refuses to start when
a mutation names a test its own control run did not see, and that guard has
worked: twelve times between 2026-08-17 and 2026-08-23 the list was forgotten,
twelve times the refusal is what said so, and no mutation has ever reported a
false SURVIVED for this reason.

What the guard cannot do is answer early. It runs after a full control pass, so
each catch costs a run that had already started, and on 2026-08-23 that was seven
mutations refused while a release was being cut. This asks the same question in
about six seconds, against the same source of names, before a byte is edited.

WHAT IS CHECKED, both directions:

  1. Every mutation's `expect` names a test defined in a file `TEST_FILES` runs.
     Same rule as the harness -- `expect` is a substring of a resolved
     `describe > test` name -- and the names come from vitest's own collection
     rather than from a regex over the sources, so there is no second parser to
     drift. That matters: a name built in a loop (`... at ${turns} turns`) is not
     a literal anywhere on disk, and a static scan reports three false failures.

  2. Every test file vitest collects is either in `TEST_FILES` or in `UNMUTATED`
     with a reason. A suite that is neither is invisible: nothing runs it and
     nothing says so.

WHAT IS REFUSED rather than reported, because each of these reads exactly like a
clean run: a collection that came back empty, an empty `MUTATIONS` or
`TEST_FILES`, a `TEST_FILES` entry vitest did not collect (it names a file that
runs nothing -- which is what a clobbered or renamed suite looks like), the same
file in both tables, and the same file listed twice in `TEST_FILES`.

An `UNMUTATED` entry naming a file vitest did not collect is a `[WARN]`, not a
failure, following the exemption tables in `check_webview_sinks.py` and
`check_viewer_wiring.py`: an allowlist entry naming something that no longer
exists is how an allowlist rots into a blanket permission, and it has to be
visible without being fatal.

WHAT IS NOT CHECKED. Whether a mutation is aimed at the *right* test, whether the
assertion it expects is any good, and anything at all about `mutate_rust.py` or
`mutate_viewer.py` -- those have their own name guards and their own runners.

⚠ `vitest list --json` takes an OPTIONAL PATH. `--json <file>` writes there
instead of to stdout, so a positional argument after it is a destination, never a
filter: `vitest list --json src/lib/text.test.ts` silently overwrites that test
file with the listing. It cost a tracked file here on 2026-08-24 (recovered from
git, which was the only witness). Collect everything and filter in Python, which
is what this does, and never put a path after `--json`.

Usage: scripts/check_mutation_test_files.py
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The harness whose two tables this gate reads.
HARNESS = "scripts/mutate_frontend.py"


def load(path: str):
    """The module, imported for its tables alone."""
    name = pathlib.Path(path).stem
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"[FAIL] cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    # Registered before exec, for the reason `check_mutation_anchors.py` records:
    # `@dataclass` resolves its own module through `sys.modules`.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def collect(npx: str) -> dict[str, list[str]]:
    """Every test vitest can see, mapped from repo-relative file to test names.

    `list` collects without executing, so this is the suite's own answer about
    what exists rather than an approximation of it. No positional argument and
    no path after `--json` -- see the warning in this module's docstring.
    """
    done = subprocess.run(
        [npx, "vitest", "list", "--json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=300,
    )
    if done.returncode != 0:
        print(f"[FAIL] vitest list exited {done.returncode}")
        print((done.stderr or done.stdout)[-2000:])
        raise SystemExit(1)
    try:
        rows = json.loads(done.stdout)
    except json.JSONDecodeError:
        # A banner, a transform error, or an empty stdout. Each is a broken
        # instrument and none of them may pass as "nothing to report".
        print("[FAIL] vitest list did not return JSON on stdout")
        print((done.stdout or done.stderr)[-2000:])
        raise SystemExit(1)
    found: dict[str, list[str]] = {}
    for row in rows:
        where = pathlib.Path(row["file"]).resolve()
        try:
            rel = where.relative_to(ROOT).as_posix()
        except ValueError:
            rel = where.as_posix()
        found.setdefault(rel, []).append(row["name"])
    return found


def main() -> int:
    """Compares the harness's two tables with what vitest can actually run."""
    harness = load(HARNESS)
    listed: list[str] = list(getattr(harness, "TEST_FILES", []))
    excluded: dict[str, str] = dict(getattr(harness, "UNMUTATED", {}))
    mutations = list(getattr(harness, "MUTATIONS", []))

    problems: list[str] = []
    warnings: list[str] = []

    # Refusals first: an empty table answers every question below with silence.
    if not listed:
        problems.append(f"{HARNESS}: TEST_FILES is empty")
    if not mutations:
        problems.append(f"{HARNESS}: MUTATIONS is empty")
    if problems:
        for line in problems:
            print(f"[FAIL] {line}")
        return 1

    for name in sorted({f for f in listed if listed.count(f) > 1}):
        problems.append(f"TEST_FILES lists {name} twice")
    both = sorted(set(listed) & set(excluded))
    for name in both:
        problems.append(f"{name} is in TEST_FILES and UNMUTATED at once")

    collected = collect(harness.npx())
    if not collected:
        print("[FAIL] vitest collected no tests at all")
        return 1

    for name in sorted(set(listed) - set(collected)):
        problems.append(
            f"TEST_FILES names {name}, which vitest collected no test from -- "
            "every mutation expecting one of its tests would report SURVIVED"
        )
    for name in sorted(set(excluded) - set(collected)):
        warnings.append(f"UNMUTATED names {name}, which vitest did not collect")
    for name in sorted(set(collected) - set(listed) - set(excluded)):
        problems.append(
            f"{name} is neither in TEST_FILES nor excluded in UNMUTATED -- "
            "nothing runs it under mutation and nothing says so"
        )

    runnable = [n for f, names in collected.items() if f in set(listed) for n in names]
    for mutation in mutations:
        if not any(mutation.expect in name for name in runnable):
            problems.append(
                f"{mutation.name}: no test TEST_FILES runs is named "
                f"{mutation.expect!r} -- it cannot go red, so it would report SURVIVED"
            )

    for line in warnings:
        print(f"[WARN] {line}")
    for line in problems:
        print(f"[FAIL] {line}")
    if problems:
        return 1

    print(
        f"[OK]   {len(mutations)} mutations name tests in {len(listed)} of the "
        f"{len(collected)} suites vitest sees; the other {len(excluded)} are "
        "excluded with a reason"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
