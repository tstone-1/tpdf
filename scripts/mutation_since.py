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

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: How far from a changed line a mutation's target may be and still be
#: selected by `--near`. Measured on the 26.10.4 release tree, 2026-10-04: of
#: 2,965 Rust mutations the file rule selected 981, and this selected 378 at
#: 25 lines, 499 at 100 and 223 at none. The one mutation that run found
#: blunted sat a few lines from the change that blunted it.
NEAR_LINES = 25


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
        "--- include affected callers when selecting release scope (BUILD.md)"
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


# --- `--near`: by the lines that changed, not by the files -------------------
#
# The file rule reruns every mutation in a file one line of which changed.
# Three files of this repository hold hundreds each, so a release that touched
# them reran a third of the table, most of it far from anything that moved:
# 981 Rust mutations and 85 minutes for 26.10.4. This selects a mutation when
# what it breaks is near a change, or when the test that should catch it
# changed. It is the release's rule since 2026-10-04; the file rule still runs,
# outside the release, where nobody waits for it (BUILD.md, step 7).
#
# What it gives up is stated rather than hidden: a change far from a mutation,
# in the same file, that blunts the test for it. The file rule finds that and
# this does not, which is why the file rule is kept and not replaced.


def changed_lines(ref: str, root: Path = ROOT) -> "dict[str, list[tuple[int, int]]] | None":
    """For each file differing from `ref`, the line ranges that are new or changed.

    Lines of the working tree, so that they can be compared with where a
    mutation's target is found in the file as it now is. A file with only
    deletions has the line the deletion sits at. An untracked file is changed
    from its first line to its last. `None` when git could not answer.
    """
    done = subprocess.run(
        ["git", "diff", "-U0", "--no-color", ref, "--"],
        cwd=root, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    if done.returncode != 0:
        return None
    out: "dict[str, list[tuple[int, int]]]" = {}
    path = None
    for line in done.stdout.splitlines():
        if line.startswith("+++ "):
            path = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("@@") and path:
            found = re.search(r"\+(\d+)(?:,(\d+))?", line)
            if found:
                first = int(found.group(1))
                count = int(found.group(2)) if found.group(2) is not None else 1
                out.setdefault(path, []).append((first, first + max(count, 1) - 1))
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=root, capture_output=True, text=True,
    )
    if untracked.returncode != 0:
        return None
    for name in untracked.stdout.splitlines():
        if name.strip():
            out[name.strip()] = [(1, 10**9)]
    return out


def _overlaps(ranges: "list[tuple[int, int]]", low: int, high: int, reach: int) -> bool:
    return any(first - reach <= high and low <= last + reach for first, last in ranges)


#: A function a Rust test is, and a quoted string a frontend test's title is.
_NAMED = re.compile(r"\bfn (\w+)\(|([\"'`])((?:(?!\2).)+)\2")


def _tests_in(text: str, wanted: "set[str]") -> "dict[str, tuple[int, int]]":
    """Where each wanted test is in `text`: its first and last line.

    A Rust test is named by its function and a frontend one by its title, so
    a test begins on a line holding `fn name(` or the title in quotes. It runs
    from there to the first later line that is indented no further, which is
    the line that closes it as both languages' formatters write one.

    One pass over the file whatever the number of tests asked for: a table
    names well over a thousand, and a search of every changed file for each
    of them took minutes where this takes a second.
    """
    lines = text.split("\n")
    found: "dict[str, tuple[int, int]]" = {}
    for at, line in enumerate(lines):
        for match in _NAMED.finditer(line):
            name = match.group(1) or match.group(3)
            if name not in wanted or name in found:
                continue
            indent = len(line) - len(line.lstrip())
            end = at
            for later in range(at + 1, len(lines)):
                end = later
                body = lines[later]
                if body.strip() and len(body) - len(body.lstrip()) <= indent:
                    break
            found[name] = (at + 1, end + 1)
    return found


def pick_near(mutations, changed, read, prefix: str = "", reach: int = NEAR_LINES) -> list:
    """The mutations whose target is within `reach` lines of a change, or whose test changed.

    `changed` is `changed_lines`' answer and `read` gives a file's text by its
    repository path, or `None`. Separate from git so that it can be tested
    with what a test chooses.
    """
    texts: "dict[str, str | None]" = {}

    def text_of(path: str) -> "str | None":
        if path not in texts:
            texts[path] = read(path)
        return texts[path]

    # The tests that changed: every test a mutation names whose lines a
    # changed range falls in, looked for in the changed files alone, since a
    # test in a file that did not change did not change.
    wanted = {m.expect.split("::")[-1] for m in mutations}
    moved: "set[str]" = set()
    for path, ranges in changed.items():
        text = text_of(path)
        if text is None:
            continue
        for name, (first, last) in _tests_in(text, wanted).items():
            if _overlaps(ranges, first, last, 0):
                moved.add(name)

    chosen = []
    for mutation in mutations:
        path = f"{prefix}{mutation.path}".replace("\\", "/")
        ranges = changed.get(path)
        near = False
        if ranges:
            text = text_of(path)
            at = text.find(mutation.before) if text is not None else -1
            if at < 0:
                # A target that cannot be found is not shown to be far from
                # the change, so it is run.
                near = True
            else:
                # The lines the target's own text is on: a search string that
                # begins or ends with a line break reaches no further for it.
                body = mutation.before.strip("\n")
                lead = len(mutation.before) - len(mutation.before.lstrip("\n"))
                low = text.count("\n", 0, at + lead) + 1
                near = _overlaps(ranges, low, low + body.count("\n"), reach)
        if near or mutation.expect.split("::")[-1] in moved:
            chosen.append(mutation)
    return chosen


def apply_near(mutations, ref: str, prefix: str = "") -> "tuple[list, int]":
    """`pick_near` against git, printed, with the exit code to use on refusal."""
    changed = changed_lines(ref)
    by_file = select(mutations, ref, prefix)
    if changed is None or by_file is None:
        print(f"[FAIL] git could not diff against {ref!r}, so nothing below would be readable")
        return [], 1

    def read(path: str) -> "str | None":
        try:
            return (ROOT / path).read_text(encoding="utf-8", newline="")
        except (OSError, UnicodeDecodeError):
            return None

    chosen = pick_near(mutations, changed, read, prefix)
    print(
        f"--- near {ref}: {len(chosen)} of {len(mutations)} mutations selected, those within "
        f"{NEAR_LINES} lines of a change or whose test changed; "
        f"{len(by_file[0]) - len(chosen)} more are in changed files and NOT run, "
        f"{len(mutations) - len(by_file[0])} are in files that did not change",
        flush=True,
    )
    print(
        "[WARN] a change far from a mutation can still blunt the test for it "
        "--- `--since` with the same ref runs those too, outside the release (BUILD.md)",
        flush=True,
    )
    if not chosen:
        print(f"[FAIL] --near {ref} selected no mutation, so there is nothing to run -- which is not a pass")
        return [], 1
    return chosen, 0


# --- self-test ---------------------------------------------------------------


def self_test() -> int:
    """`pick_near` and `changed_lines` against cases chosen to separate them."""
    import tempfile
    from types import SimpleNamespace as M

    failures: "list[str]" = []

    def check(name: str, ok: bool, detail: str = "") -> None:
        print(f"{'[OK]  ' if ok else '[FAIL]'} {name}{'  ' + detail if detail and not ok else ''}")
        if not ok:
            failures.append(name)

    code = "\n".join(f"line {n}" for n in range(1, 201)) + "\n"
    tests = (
        "mod tests {\n    #[test]\n    fn far_one() {\n        a();\n        b();\n    }\n\n"
        "    #[test]\n    fn other() {\n        c();\n    }\n}\n"
        "describe('x', () => {\n  it(\"reads a title\", () => {\n    d();\n  });\n});\n"
    )
    files = {"src/code.rs": code, "src/tests.rs": tests}
    table = [
        M(name="at the change", path="src/code.rs", before="line 100\n", expect="other"),
        M(name="25 below", path="src/code.rs", before="line 125\n", expect="other"),
        M(name="26 below", path="src/code.rs", before="line 126\n", expect="other"),
        M(name="25 above", path="src/code.rs", before="line 75\n", expect="other"),
        M(name="26 above", path="src/code.rs", before="line 74\n", expect="other"),
        M(name="spans to it", path="src/code.rs", before="line 72\nline 73\nline 74\nline 75\n", expect="other"),
        M(name="far, test changed", path="src/code.rs", before="line 10\n", expect="far_one"),
        M(name="far, titled test changed", path="src/code.rs", before="line 11\n", expect="reads a title"),
        M(name="gone", path="src/code.rs", before="no such text", expect="other"),
        M(name="another file", path="src/untouched.rs", before="x", expect="other"),
    ]
    names = lambda picked: [m.name for m in picked]  # noqa: E731
    read = files.get
    only_code = {"src/code.rs": [(100, 100)]}
    check(
        "a mutation is selected within the reach of a change, on either side, and not one line past it",
        names(pick_near(table, only_code, read)) == ["at the change", "25 below", "25 above", "spans to it", "gone"],
        str(names(pick_near(table, only_code, read))),
    )
    check(
        "a reach of nought selects only what the change touches",
        names(pick_near(table, only_code, read, reach=0)) == ["at the change", "gone"],
        str(names(pick_near(table, only_code, read, reach=0))),
    )
    # Line 5 is inside `far_one`; line 10 is inside `other`; 15 inside the titled test.
    check(
        "a mutation far from any change is selected when the test it names changed",
        names(pick_near(table, {"src/tests.rs": [(5, 5)]}, read)) == ["far, test changed"],
        str(names(pick_near(table, {"src/tests.rs": [(5, 5)]}, read))),
    )
    check(
        "and a test named by its title is found as one named by its function is",
        names(pick_near(table, {"src/tests.rs": [(15, 15)]}, read)) == ["far, titled test changed"],
        str(names(pick_near(table, {"src/tests.rs": [(15, 15)]}, read))),
    )
    check(
        "a change between two tests selects neither",
        pick_near(table, {"src/tests.rs": [(7, 7)]}, read) == [],
        str(names(pick_near(table, {"src/tests.rs": [(7, 7)]}, read))),
    )
    check(
        "a prefix turns a table's path into the repository's",
        names(pick_near([M(name="in crate", path="code.rs", before="line 100\n", expect="other")],
                        only_code, read, prefix="src/")) == ["in crate"],
    )
    check("nothing changed selects nothing", pick_near(table, {}, read) == [])

    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)

        def git(*args: str) -> None:
            subprocess.run(["git", "-c", "user.email=t@example.invalid", "-c", "user.name=t", *args],
                           cwd=root, check=True, capture_output=True)

        git("init", "-q", "-b", "main")
        (root / "a.txt").write_text("".join(f"{n}\n" for n in range(1, 21)))
        (root / "gone.txt").write_text("one\ntwo\nthree\n")
        git("add", "-A")
        git("commit", "-q", "-m", "base")
        lines = [f"{n}\n" for n in range(1, 21)]
        lines[4] = "five\n"
        lines[9:9] = ["new a\n", "new b\n"]
        (root / "a.txt").write_text("".join(lines))
        (root / "gone.txt").write_text("one\nthree\n")
        (root / "fresh.txt").write_text("x\n")
        got = changed_lines("HEAD", root)
        check(
            "changed lines are the working tree's: a changed line, added lines, a deletion's place, a new file",
            got == {"a.txt": [(5, 5), (10, 11)], "gone.txt": [(1, 1)], "fresh.txt": [(1, 10**9)]},
            str(got),
        )
        check("an unknown ref is no answer, not an empty one", changed_lines("no-such-ref", root) is None)

    print(f"\n{'[OK] self-test passed' if not failures else f'[FAIL] {len(failures)} checks'}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else print(__doc__) or 0)
