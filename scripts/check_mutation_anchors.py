#!/usr/bin/env python3
"""Every mutation's anchor is present in the tree exactly once.

Two failures look identical in `git status` and this is the only thing that tells
them apart from a clean tree.

**A mutation left behind.** `docs/TRAPS.md` records that a mutation harness which
dies leaves its edit in the working tree. What it does not say is that the edit is
invisible: the harnesses mutate files that a feature branch is usually already
modifying, so `git status` shows exactly what it showed before, and `git diff` on a
600-line change does not draw the eye to two swapped lines. On 2026-08-16 two
harness runs were killed and left `viewer.ts` holding `this.rotateBy(turns)` in
place of the two lines a page turn needs --- and the next run's baseline went red
in a way that read as a defect in the feature.

**An anchor that has drifted.** A mutation whose `before` string no longer occurs
is aimed at nothing. The harness itself refuses such a mutation when it reaches it,
which is correct and far too late: it is one run of a harness that takes an hour,
and until that run happens the table looks complete. `mutate_viewer.py` carried an
anchor removed by commit 9e9be98 --- the line it named had not existed for weeks
--- and nothing said so, because the harness that would have said so had not
completed a run in that time. The same thing happened again the same day and much
faster: an ordinary `*id` -> `id` cleanup in `save.rs` silently unaimed a mutation
that had passed an hour earlier.

So the invariant is one line and it covers both: **for every mutation in every
table, its `before` string occurs exactly once in the file it names.** More than
once and the harness cannot place the edit; zero and either the anchor has drifted
or a previous mutation is still sitting in the tree.

This deliberately does *not* try to distinguish those two cases. They need
different fixes and the difference is obvious once you look at the line, whereas a
check that guessed would be confidently wrong about half of them --- see the trap
about a static reason turning a failure into a wrong diagnosis.

A second invariant, added 2026-08-20 after the first instance cost nothing only
because it was found by reading: **a mutation whose expected test lives in a
platform-gated test module has to declare that platform in `only_on`.** The two
`recentdocs` mutations written on Windows named a test inside
`#[cfg(all(test, windows))] mod tests`, and carried no `only_on`. On a Mac that
test does not exist, so `mutate_rust.py`'s name guard --- which is right to be
loud about a name it cannot find --- would have refused the **entire** table over
it. That is the trap already recorded as *"a guard that answers by refusing the
whole run turns two blocked mutations into 178"*, and it is the mirror of the
`menu::` incident the harness's own comment describes: written on one platform,
silently blocking the other, and invisible until somebody runs it there.

The anchor check could not see it, because the anchor is a *string in a file* and
platform gating decides which strings become *code*. So this asks the other
question: where is the test that is supposed to go red, and can it go red here?
"""

from __future__ import annotations

import importlib.util
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The tables, and the directory each one's paths are relative to. `mutate_rust`
#: names paths inside the crate; the other two name them from the repository root.
TABLES = [
    ("scripts/mutate_rust.py", ROOT / "src-tauri"),
    ("scripts/mutate_frontend.py", ROOT),
    ("scripts/mutate_viewer.py", ROOT),
    ("scripts/mutate_python.py", ROOT),
]


def load(path: str):
    """The module, imported for its `MUTATIONS` table alone."""
    name = pathlib.Path(path).stem
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"[FAIL] cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    # Registered before exec: `@dataclass` resolves its own module through
    # `sys.modules`, and without this it raises on a bare AttributeError that
    # reads as a syntax problem in the table.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


#: A test module gated to one platform, and the `only_on` value that names it.
#: Only the two forms this tree actually uses --- inventing more would be a
#: parser with no subject, and a wrong guess here reads as a table defect.
PLATFORM_GATES = [
    (re.compile(r'^#\[cfg\(all\(test,\s*windows\)\)\]'), "windows"),
    (re.compile(r'^#\[cfg\(all\(test,\s*target_os\s*=\s*"macos"\)\)\]'), "macos"),
]


#: Any `fn`, so the scan below can be one pass over the tree rather than one per
#: mutation. Re-walking every source for each of ~200 names took 3.4 s against
#: 0.3 s for the rest of this gate, which is the wrong shape for something that
#: runs before every push.
DEFINITION = re.compile(r"^\s*(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")


#: A `#[test]` attribute, in the two spellings this tree uses. A `fn` reached
#: with this armed is a test; every other `fn` is not, and counting those would
#: make the check below satisfiable by an ordinary function that happens to
#: share a name --- a control easier than the check certifies nothing.
TEST_ATTRIBUTE = re.compile(r"^\s*#\[(?:tokio::)?test\]")


def test_names(root: pathlib.Path) -> set[str]:
    """Every `#[test]` function name in the tree.

    The harness runs `cargo test <expect>`, which is a *substring* filter, so a
    name it cannot match is not an error there --- it selects nothing, the
    mutation cannot go red, and the harness reports SURVIVED. Its own guard
    refuses the run instead, which is right and arrives only after a full
    control pass: on 2026-08-27 that cost a run to a name the increment before
    had renamed, `an_image_in_the_region_makes_the_plan_incomplete` becoming
    `an_object_this_cannot_remove_makes_the_plan_incomplete`. This asks the same
    question in about a second, and it is the Rust half of what
    `check_mutation_test_files.py` already does for the frontend.

    **Exact names, not substrings.** All 439 distinct `expect` values in the
    Rust table are exact test-function names, measured before this was written;
    a deliberate substring filter would have to become one, which is the trade
    this makes on purpose --- `--only "text: "` is already in `docs/TRAPS.md` as
    a substring filter that ran more than it named.
    """
    found: set[str] = set()
    for source in sorted(root.rglob("*.rs")):
        armed = False
        for line in source.read_text(encoding="utf-8").splitlines():
            if TEST_ATTRIBUTE.match(line):
                armed = True
                continue
            match = DEFINITION.match(line)
            if match:
                if armed:
                    found.add(match.group(1))
                armed = False
    return found


def gated_tests(root: pathlib.Path) -> dict[str, set[str]]:
    """Every `fn` name defined inside a platform-gated test module, to its gates.

    A name **absent** from this map is either ungated --- so it compiles
    everywhere --- or not a test at all, and neither is this check's business:
    a name the harness cannot find anywhere is the case its own guard owns, and
    is deliberately loud about.

    A name mapping to *several* gates is one rule with a test on each side of the
    cfg, which needs no declaration because it can go red wherever it is aimed.
    """
    found: dict[str, set[str]] = {}
    for source in sorted(root.rglob("*.rs")):
        gate = None
        for line in source.read_text(encoding="utf-8").splitlines():
            for pattern, platform in PLATFORM_GATES:
                if pattern.match(line):
                    # A module attribute at column 0 opens a region that runs to
                    # the next one or to the end of the file. Every test module
                    # in this tree is written that way, and a nested one would
                    # simply be attributed to its outer gate --- which is the
                    # conservative direction.
                    gate = platform
            match = DEFINITION.match(line)
            if match and gate is not None:
                found.setdefault(match.group(1), set()).add(gate)
    return found


def rows_after_entry_point(path: str) -> int:
    """How many `MUTATIONS +=` blocks sit below the `__main__` guard.

    This gate *imports* each table, and an import runs the whole module --- so a
    block appended below `if __name__ == "__main__":` is counted here and is
    **not** registered when the harness actually runs, because by then `main()`
    has already read the table and returned. The gate goes green on anchors the
    harness will never reach, which is the exact shape of the trap this file's
    comment below records about reading a subject differently from the thing
    that uses it. Hit on 2026-08-21 with ten new mutations, all of them counted
    and none of them runnable.
    """
    below = 0
    seen_guard = False
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith('if __name__ == "__main__":'):
            seen_guard = True
        elif seen_guard and line.startswith("MUTATIONS"):
            below += 1
    return below


def main() -> int:
    problems: list[str] = []
    total = 0
    for path, base in TABLES:
        stranded = rows_after_entry_point(path)
        if stranded:
            problems.append(
                f"{path}: {stranded} MUTATIONS block(s) below the __main__ guard -- "
                "this gate imports the module and counts them, a real run does not"
            )
        module = load(path)
        table = getattr(module, "MUTATIONS", None)
        # An empty or missing table passes every assertion below while proving
        # nothing, which is the shape this repository keeps recording.
        if not table:
            problems.append(f"{path}: no MUTATIONS table, or it is empty")
            continue
        intact = 0
        for mutation in table:
            # An environment mutation has no anchor: `scripts/mutate_python.py`
            # perturbs `RUSTUP_TOOLCHAIN` for the gate whose whole subject is
            # that variable, and no edit to a tracked file produces it.
            #
            # **Guarded on `env`, not on the empty anchor.** A skip keyed on the
            # anchor alone would silently excuse a *file* mutation whose `before`
            # had been emptied, which is the one thing this gate exists to catch.
            if not mutation.before:
                if not getattr(mutation, "env", None):
                    problems.append(
                        f"{path}: {mutation.name} -- no anchor and no env, so this "
                        "row perturbs nothing and cannot fail"
                    )
                continue
            target = base / mutation.path
            if not target.exists():
                problems.append(f"{path}: {mutation.name} -- {mutation.path} does not exist")
                continue
            # `read_text` translates newlines, so a CRLF file is counted as if it
            # were LF -- which is what every anchor in every table is written
            # with. That must stay the SAME convention the harnesses match under,
            # and for a while it was not: they read bytes, and `mutate_viewer.py`
            # had no normalisation, so this gate was green on 289 anchors while
            # that harness could match none of the multi-line ones. A guard
            # reading its subject differently from the thing it guards is
            # measuring a different file. See `docs/TRAPS.md`.
            #
            # `.gitattributes` has pinned `* text=auto eol=lf` since 2026-08-26,
            # so the two conventions no longer diverge at checkout on any
            # platform. That removes the everyday case and not the requirement:
            # what this comment asks for is that the gate and the harnesses agree
            # about the file, whatever wrote it, and a tool rewriting a source in
            # text mode on Windows still produces CRLF.
            found = target.read_text(encoding="utf-8").count(mutation.before)
            if found == 1:
                intact += 1
            else:
                problems.append(
                    f"{path}: {mutation.name}\n"
                    f"       anchor occurs {found}x in {mutation.path}, expected 1.\n"
                    f"       Read the line. Three things look like this: the anchor has\n"
                    f"       drifted and needs re-aiming, a killed harness left its edit\n"
                    f"       behind, or a mutation harness is running RIGHT NOW and this\n"
                    f"       is its edit in flight -- check before concluding."
                )
        total += len(table)
        print(f"[OK] {path}: {intact}/{len(table)} anchors present exactly once")

        # Only the Rust table has platform-gated tests; the frontend ones run
        # under one Node. Scanning the others would be a check with no subject,
        # which passes exactly like one that looked.
        if path != "scripts/mutate_rust.py":
            continue
        # Does the test each mutation names exist at all? A name that matches
        # nothing selects nothing, so the mutation reports SURVIVED --- which
        # reads as a gap in the tests rather than a mistake in the table.
        defined = test_names(base / "src")
        # A scan that found no test at all agrees with a clean tree about every
        # mutation below, which is the same shape as the empty-table guard.
        if not defined:
            problems.append(f"{path}: no #[test] functions found under {base / 'src'}")
            continue
        for mutation in table:
            if mutation.expect not in defined:
                problems.append(
                    f"{path}: {mutation.name}\n"
                    f"       expects `{mutation.expect}`, which is not a #[test] function\n"
                    f"       anywhere under {base / 'src'}. `cargo test` would select\n"
                    f"       nothing, so the harness refuses the whole table over it.\n"
                    f"       Either the test was renamed, or the name is a typo."
                )
        print(f"[OK] {path}: every mutation names one of the {len(defined)} tests in the tree")

        gated = gated_tests(base / "src")
        # A scan that found no gated test at all passes every mutation below
        # while proving nothing -- the same shape as the empty table above, and
        # this tree has had two such modules since 2026-08-19.
        if not gated:
            problems.append(f"{path}: no platform-gated test modules found under {base / 'src'}")
            continue
        declared = 0
        for mutation in table:
            platforms = gated.get(mutation.expect)
            if not platforms:
                continue
            if mutation.only_on in platforms:
                declared += 1
                continue
            if len(platforms) > 1:
                # Reachable on every platform it is gated to, so no declaration
                # is needed -- one rule with a test on each side of the cfg.
                declared += 1
                continue
            (only,) = sorted(platforms)
            problems.append(
                f"{path}: {mutation.name}\n"
                f"       expects `{mutation.expect}`, which is defined only inside a\n"
                f"       {only}-gated test module, and only_on is {mutation.only_on!r}.\n"
                f"       On any other platform that test does not exist, and the\n"
                f"       harness refuses the WHOLE table over an unknown name.\n"
                f"       Set only_on=\"{only}\"."
            )
        print(f"[OK] {path}: {declared} mutation(s) aimed at platform-gated tests declare it")

    if problems:
        print()
        for problem in problems:
            print(f"[FAIL] {problem}")
        return 1
    print(f"[OK] all {total} mutation anchors are aimed at code that exists, exactly once.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
