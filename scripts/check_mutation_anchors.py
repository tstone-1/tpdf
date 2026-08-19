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
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The tables, and the directory each one's paths are relative to. `mutate_rust`
#: names paths inside the crate; the other two name them from the repository root.
TABLES = [
    ("scripts/mutate_rust.py", ROOT / "src-tauri"),
    ("scripts/mutate_frontend.py", ROOT),
    ("scripts/mutate_viewer.py", ROOT),
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


def main() -> int:
    problems: list[str] = []
    total = 0
    for path, base in TABLES:
        module = load(path)
        table = getattr(module, "MUTATIONS", None)
        # An empty or missing table passes every assertion below while proving
        # nothing, which is the shape this repository keeps recording.
        if not table:
            problems.append(f"{path}: no MUTATIONS table, or it is empty")
            continue
        intact = 0
        for mutation in table:
            target = base / mutation.path
            if not target.exists():
                problems.append(f"{path}: {mutation.name} -- {mutation.path} does not exist")
                continue
            # `read_text` translates newlines, so a CRLF checkout is counted as
            # if it were LF -- which is what every anchor in every table is
            # written with. That must stay the SAME convention the harnesses
            # match under, and for a while it was not: they read bytes, and
            # `mutate_viewer.py` had no normalisation, so this gate was green on
            # 289 anchors while that harness could match none of the multi-line
            # ones. A guard reading its subject differently from the thing it
            # guards is measuring a different file. See `docs/TRAPS.md`.
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

    if problems:
        print()
        for problem in problems:
            print(f"[FAIL] {problem}")
        return 1
    print(f"[OK] all {total} mutation anchors are aimed at code that exists, exactly once.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
