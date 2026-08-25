"""Selecting the mutations a change could have moved, and saying what was left out.

WHY THIS EXISTS. Each mutation costs one run of the suite the harness drives, so
a table's cost is linear in its size and the tables are large. Running all of
them proves the tree; running all of them to check one edit is the wrong
granularity. `mutate_rust.py` grew a `--since` for exactly that in August and the
other two did not, which `BUILD.md` recorded as *the obvious next piece of work*
and as making the split "lopsided" -- the Rust harness could run six mutations
where the frontend one was all-or-nothing at about fifty.

This is that flag, in one place rather than three. `changed_files` below is
`mutate_rust.py`'s own, moved here unchanged: it was the working implementation
and a second one written beside it would be the copy this repository keeps
finding in other forms.

## Why the report is loud, and why nothing selected is a refusal

A narrowed run and a full run print the same last line. `[OK] all 12 mutations
caught by the test named for them` is what a complete pass looks like, and the
whole risk of this flag is reading one for the other -- a silent cap reads as
"covered everything" when it did not. So a `--since` run states the ref, the
count against the table's total, and the changed files no mutation aims at; and
it ends by saying it is not the full table.

Nothing selected is **refused** rather than reported green, because zero
mutations caught is indistinguishable in the output from zero mutations run and
the reassuring reading is the wrong one. The message says nothing to run, not
something failed.

## What it does not reach, which is not the same as what it does not cover

A mutation is selected by the file it edits. That is sound as far as it goes --
the test a mutation names is almost always in or beside the file it mutates --
and it does not go as far as the table. **A change in one file can decide what
another does**, so a mutation in an untouched file can stop being caught without
that file appearing in any diff. This is the loop while a change is being made;
the table is what runs before a push.

It also does not look at which *test* files changed. Mapping a mutation's
`expect` -- a test name -- back to a file needs a second inventory, and this
repository has already paid for a check that kept one.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def changed_files(ref: str) -> "set[str] | None":
    """Repo-relative paths differing from `ref`, working tree included.

    Two questions, because they have different answers and both matter: what
    the commits since `ref` touched, and what is edited right now and not
    committed. A run that read only the first would skip exactly the mutation
    aimed at the code being written.

    `None` when git could not answer, which the caller must treat as *unknown*
    rather than as *nothing changed*: an unresolvable ref and a clean tree both
    produce an empty list, and only one of them makes a selection meaningful.
    """
    out: "set[str]" = set()
    for cmd in (
        ["git", "diff", "--name-only", f"{ref}...HEAD"],
        ["git", "diff", "--name-only", "HEAD"],
        ["git", "ls-files", "--others", "--exclude-standard"],
    ):
        done = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        if done.returncode != 0:
            return None
        out |= {line.strip() for line in done.stdout.splitlines() if line.strip()}
    return out


def select(mutations, ref: str, prefix: str = "") -> "tuple[list, list[str]] | None":
    """The mutations `ref` could have moved, and the lines to print about them.

    `prefix` turns a table's paths into repo-relative ones: `mutate_rust.py`
    names paths inside the crate, the other two name them from the repository
    root. The caller passes it rather than this guessing from a path's shape,
    because a wrong guess selects *nothing* and a run that selected nothing is
    refused -- loud, and traceable to the caller that got it wrong.

    `None` when git could not answer. A selection built on an unanswerable
    question is not a smaller run, it is an unknown one.
    """
    touched = changed_files(ref)
    if touched is None:
        return None

    def repo_path(mutation) -> str:
        return f"{prefix}{mutation.path}".replace("\\", "/")

    normalised = {path.replace("\\", "/") for path in touched}
    chosen = [m for m in mutations if repo_path(m) in normalised]

    counts: "dict[str, int]" = {}
    for mutation in chosen:
        counts[repo_path(mutation)] = counts.get(repo_path(mutation), 0) + 1
    aimed_at = {repo_path(m) for m in mutations}

    report = [
        f"--- since {ref}: {len(normalised)} file(s) changed, "
        f"{len(chosen)} of {len(mutations)} mutations selected, "
        f"{len(mutations) - len(chosen)} NOT run"
    ]
    for path, count in sorted(counts.items()):
        report.append(f"       {count:>3}  {path}")
    # Named rather than counted. A changed file no mutation aims at is the
    # ordinary case for a document or a script, and it is also what a new and
    # entirely uncovered module looks like -- worth telling apart by eye, which
    # only the list allows.
    silent = sorted(path for path in normalised if path not in aimed_at)
    if silent:
        report.append(f"       no mutation aims at {len(silent)} of them:")
        for path in silent:
            report.append(f"         {path}")
    report.append(
        "[WARN] a change elsewhere can still break a mutation in a file this missed "
        "--- run the whole table before pushing"
    )
    return chosen, report


def apply(mutations, ref: str, prefix: str = "") -> "tuple[list, int] | None":
    """`select`, printed, with the exit code a caller should use on refusal.

    Returns `None` to mean *carry on with what you had*, so a caller writes one
    `if ref:` and no branching on how the flag failed. The three harnesses reach
    this identically, and reaching it identically is the point: the first
    version of this lived in one of them, and the split is what `BUILD.md`
    called lopsided.
    """
    picked = select(mutations, ref, prefix)
    if picked is None:
        print(f"[FAIL] git could not diff against {ref!r}, so nothing below would be readable")
        return [], 1
    chosen, report = picked
    for line in report:
        print(line, flush=True)
    if not chosen:
        print(
            f"[FAIL] --since {ref} selected no mutation, so there is nothing to run "
            "-- which is not a pass"
        )
        return [], 1
    return chosen, 0
