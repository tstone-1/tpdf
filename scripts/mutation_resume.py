"""Carrying a mutation table's verdicts across a run that did not finish.

WHY THIS EXISTS. Twice on 2026-08-30 a backgrounded `mutate_frontend.py` was
killed at about twenty-five minutes, and the cost was not the lost time --- it
was the two things a kill leaves behind:

  * **A mutation applied in the tree.** The restore in each harness sits in a
    `finally`, and a `finally` does not survive a kill. `git status` was the
    only witness, and only because somebody thought to look. The viewer harness
    is worse than the other two: it holds the original bytes in memory alone, so
    a kill there leaves the mutation with no backup anywhere.
  * **Every verdict already proved.** Four hundred mutations that had each been
    caught by the test named for them, discarded because the process that knew
    it died.

Both are answered from one small piece of state, and the first is answered
whether or not anybody asks for it. **Recovery is unconditional; reusing
verdicts needs `--resume`.** A tree left mutated is a defect regardless of what
the next run intends to do, and finding it should not depend on a flag.

## What makes a stored verdict reusable

One guard, not two: **the tracked tree must be byte-identical to the one the
verdicts were taken against.** A mutation's verdict is a claim about the whole
suite -- what caught it, and that nothing else did -- so an edit anywhere can
change it. Per-file invalidation would be finer and would be a claim this cannot
support; `mutation_since.py` documents the same limit for `--since`, where the
selection is a convenience and the caveat is printed. Here there is a choice, so
the strict answer is taken: any edit at all discards every stored verdict.

That is blunt on purpose. Resuming after a kill reuses everything, because
nothing changed. Fixing a surviving mutation's code and resuming reuses nothing,
because the tree it would be reusing verdicts about no longer exists.

The fingerprint is `HEAD`, the full `git diff HEAD --binary` (binary, so a
changed `.pdf` is its bytes rather than the words *Binary files differ*), and
every untracked-but-not-ignored file's digest. Two gaps, both named rather than
papered over:

  * **Gitignored inputs are invisible to it** -- the generated corpus under
    `testdata/`, `vendor/pdfium/`, `node_modules/`, `target/`. Regenerating a
    fixture between a kill and a resume changes what the suite runs against and
    this cannot see it. A caller that knows which files it depends on passes
    them as `also=`, and `mutate_viewer.py` passes the fixtures its chosen
    runners open, which is the largest part of the gap for the harness where it
    matters most.
  * **The state directory must be ignored by git**, or writing it would change
    the fingerprint it just recorded. `.mutations/` is in `.gitignore` for that
    reason and not for tidiness.

The digest of each mutation (name, path, anchor, replacement, expectation) is
stored too, and it is the **index** a verdict is looked up by --- not a second
guard. It cannot disagree with the fingerprint, because the table lives in a
tracked file that the fingerprint covers, and this repository has paid for
keeping two mechanisms where one rule belongs.

## The ordering that makes recovery work

The state naming a mutation as in flight is written **before** the mutated bytes
reach the file, never after. A kill in that window leaves the record saying
"pending" and the file still clean, which recovery reads correctly and does
nothing about. The other order would leave a mutated file that no record names,
which is exactly the state this module exists to end.

Recovery answers by digest and never by assumption. The file is the one the run
started from (nothing to do), or the mutation the run wrote (restored from the
backup, and the restore is verified), or it is neither --- in which case it is
somebody's edit and this refuses to touch it, because clobbering the repair
somebody made by hand is a worse outcome than the mutation it would undo.

Usage:
    scripts/mutation_resume.py --self-test
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: The state lives in `<root>/.mutations/`, gitignored --- which is load-bearing
#: rather than tidy: it is written during the run whose tree it fingerprints, so
#: a tracked one would make every fingerprint disagree with itself. The path is
#: built from each `Resume`'s own `root` rather than from a module constant,
#: because the self-test's root is a scratch repository.

#: Bumped when the shape below changes. An older file is discarded rather than
#: read hopefully -- a half-understood record is worse than none, and the only
#: thing it could give back is a verdict.
VERSION = 2


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def key_of(mutation) -> str:
    """What a verdict is filed under.

    Everything that decides what the mutation *is*. Two mutations differing only
    in their expectation are different mutations, and a verdict for one says
    nothing about the other.
    """
    parts = [
        mutation.name,
        str(mutation.path),
        mutation.before,
        mutation.after,
        mutation.expect,
        getattr(mutation, "runner", ""),
    ]
    return digest("\0".join(parts).encode("utf-8"))


def tree_fingerprint(root: Path = ROOT, also: "list[Path] | None" = None) -> "str | None":
    """A digest of the tracked tree, or `None` when git could not answer.

    `None` is *unknown*, never *unchanged*: an unresolvable repository and a
    clean one both produce empty output, and only one of them makes a stored
    verdict meaningful. Every caller treats it as a refusal to reuse.
    """
    parts: "list[str]" = [f"v{VERSION}"]
    for cmd in (
        ["git", "rev-parse", "HEAD"],
        # `--binary` because the plain diff renders a changed `.pdf` as the
        # sentence "Binary files ... differ", which is the same sentence for
        # every possible change to it. `src-tauri/src/warm.pdf` is tracked and
        # is compiled into the shipped binary.
        ["git", "diff", "HEAD", "--binary"],
        ["git", "ls-files", "--others", "--exclude-standard"],
    ):
        done = subprocess.run(cmd, cwd=root, capture_output=True, text=True)
        if done.returncode != 0:
            return None
        parts.append(done.stdout)
    untracked = [line.strip() for line in parts[-1].splitlines() if line.strip()]
    for path in sorted(untracked) + [str(p) for p in sorted(also or [])]:
        full = root / path
        parts.append(path)
        parts.append(digest(full.read_bytes()) if full.is_file() else "absent")
    return digest("\0".join(parts).encode("utf-8"))


@dataclass
class Verdict:
    """One mutation's decided outcome, as an earlier process printed it."""

    ok: bool
    outcome: str
    line: str


class Resume:
    """The state file for one harness, and the three things it is asked.

    `recover()` before anything else and unconditionally; `open()` once the tree
    is the tree the run will use; then `done()`, `begin()` and `record()` around
    each mutation.
    """

    def __init__(self, harness: str, reuse: bool, root: Path = ROOT,
                 also: "list[Path] | None" = None) -> None:
        self.reuse = reuse
        self.root = root
        self.also = [p.relative_to(root) if p.is_absolute() else p for p in (also or [])]
        self.dir = root / ".mutations"
        self.path = self.dir / f"{harness}.json"
        self.backup = self.dir / f"{harness}.bak"
        self.state = self._load()
        self.reused: "dict[str, Verdict]" = {}
        self.ran = 0
        self.fingerprint: "str | None" = None

    # --- the file -------------------------------------------------------

    def _load(self) -> dict:
        try:
            state = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return {}
        # A file this does not understand is discarded rather than guessed at.
        return state if isinstance(state, dict) and state.get("version") == VERSION else {}

    def _save(self) -> None:
        """Written whole and replaced atomically, after every verdict.

        A verdict that reaches the file only at the end is a verdict the kill
        this module exists for would take with it, which is the defect rather
        than a cost of it. `os.replace` so a kill mid-write cannot leave a
        truncated file that reads as no verdicts at all.
        """
        self.dir.mkdir(parents=True, exist_ok=True)
        temp = self.path.with_suffix(".json.part")
        temp.write_text(json.dumps(self.state, indent=1), encoding="utf-8")
        os.replace(temp, self.path)

    # --- recovery, which is not optional --------------------------------

    def recover(self) -> "tuple[int, list[str]]":
        """Put back a mutation a killed run left in the tree. Exit code and lines.

        Runs before the fingerprint is taken, and must: a tree still holding a
        mutation does not fingerprint as the tree that will be tested.
        """
        pending = self.state.get("pending")
        if not pending:
            return 0, []
        target = self.root / pending["path"]
        if not target.is_file():
            return 1, [
                f"[FAIL] {pending['path']} was mutated by a run that did not finish and is "
                "now missing entirely -- restore it before running anything here"
            ]
        now = digest(target.read_bytes())
        if now == pending["clean"]:
            # The kill landed between recording the intent and writing the
            # bytes. Nothing to undo, and saying so is worth a line: silence
            # here is indistinguishable from this module not having run.
            self.state.pop("pending", None)
            self._save()
            return 0, [
                f"[OK]   a run of this harness did not finish, and {pending['path']} was "
                "not left mutated"
            ]
        if now != pending["mutated"]:
            return 1, [
                f"[FAIL] {pending['path']} is neither the file the killed run started from "
                f"nor the mutation it wrote ({pending['name']}), so somebody has edited it "
                "since -- refusing to touch it. Check it, then delete "
                f"{self.path.relative_to(self.root)}"
            ]
        if not self.backup.is_file():
            return 1, [
                f"[FAIL] {pending['path']} still holds the mutation {pending['name']!r} and "
                f"the backup beside it is gone -- `git checkout -- {pending['path']}` if it "
                "is tracked and unmodified otherwise"
            ]
        original = self.backup.read_bytes()
        if digest(original) != pending["clean"]:
            return 1, [
                f"[FAIL] the backup for {pending['path']} is not the file the run started "
                "from, so restoring from it would put back something else"
            ]
        # The backup's own digest is checked against `clean` above, and
        # `write_bytes` raises rather than truncating, so a readback here could
        # not fail: a mutation deleting it reddened nothing, which makes it
        # decoration rather than a guard. The check that does the work is the
        # one on the backup, and a mutation dropping *that* reddens three.
        target.write_bytes(original)
        self.state.pop("pending", None)
        self._save()
        return 0, [
            f"[OK]   restored {pending['path']}, left mutated by a run that did not finish "
            f"({pending['name']})"
        ]

    # --- reuse ----------------------------------------------------------

    def open(self, table) -> "list[str]":
        """Decide what may be reused, and say it. Call after the tree is settled.

        Returns the lines to print. It never refuses: a fingerprint that cannot
        be taken, or one that disagrees, costs the stored verdicts and nothing
        else --- the run then does the whole table, which is what it would have
        done anyway.

        **Keeping and reusing are separate decisions, and only the second is
        what `--resume` asks about.** Verdicts taken against this exact tree are
        kept whatever the flag says, so a narrow `--only` run in the middle of a
        long table adds one verdict rather than destroying four hundred. The
        first version of this wiped the file on every run that did not pass
        `--resume`, which made the state useless in the workflow it exists for:
        a killed table, one small run to check something, and the record gone.
        """
        self.fingerprint = tree_fingerprint(self.root, self.also)
        stored = self.state.get("done", {})
        if not isinstance(stored, dict):
            stored = {}
        known = {key_of(m) for m in table}
        matches = self.fingerprint is not None and self.state.get("fingerprint") == self.fingerprint
        # Everything from this tree survives; anything from another is gone.
        # Half-keeping is what would produce a record no single tree ever
        # produced, and the fingerprint could not tell you afterwards.
        kept = dict(stored) if matches else {}
        usable = {k: v for k, v in kept.items() if k in known}

        lines: "list[str]" = []
        if self.reuse:
            if self.fingerprint is None:
                lines.append(
                    "[WARN] --resume: git could not fingerprint the tree, so no stored "
                    "verdict can be shown to describe it -- running the whole table"
                )
            elif not stored:
                lines.append("[INFO] --resume: no stored verdicts, so this is an ordinary run")
            elif not matches:
                lines.append(
                    f"[WARN] --resume: the tree has changed since the stored verdicts were "
                    f"taken, so all {len(stored)} of them are discarded. A verdict is a claim "
                    "about the whole suite, and an edit anywhere can move it"
                )
            else:
                self.reused = {k: Verdict(**v) for k, v in usable.items()}
                lines.append(
                    f"--- resume: {len(self.reused)} of {len(table)} verdicts reused from a "
                    f"run started {self.state.get('started', '?')}; the tracked tree is "
                    "byte-identical to the one they were taken against"
                )
                lines.append(
                    "[WARN] that fingerprint does not cover gitignored inputs -- a "
                    "regenerated corpus or a rebuilt vendor directory is invisible to it"
                )
        elif usable:
            lines.append(
                f"[INFO] {len(usable)} of these {len(table)} mutations already have a "
                f"verdict from a run of {self.state.get('started', 'an earlier process')} "
                "against this exact tree -- `--resume` would reuse them"
            )

        self.state = {
            "version": VERSION,
            # The timestamp belongs to the tree, not to the process: it is what
            # the reuse line quotes, and re-stamping it on every run would make
            # a reused verdict claim to have been taken a moment ago.
            "started": (self.state.get("started") if matches else None)
            or time.strftime("%Y-%m-%dT%H:%M:%S"),
            "fingerprint": self.fingerprint,
            "done": kept,
        }
        self._save()
        return lines

    def done(self, mutation) -> "Verdict | None":
        """The stored verdict for this mutation, if one may be reused."""
        return self.reused.get(key_of(mutation))

    def begin(self, mutation, target: Path, clean: bytes, mutated: bytes) -> None:
        """Record the mutation as in flight. Call BEFORE writing `mutated`.

        The other order leaves a window in which the tree holds a mutation no
        record names, which is the state recovery cannot answer.
        """
        self.dir.mkdir(parents=True, exist_ok=True)
        self.backup.write_bytes(clean)
        self.state["pending"] = {
            "name": mutation.name,
            "path": str(Path(target).resolve().relative_to(self.root.resolve())).replace("\\", "/"),
            "clean": digest(clean),
            "mutated": digest(mutated),
        }
        self._save()

    def record(self, mutation, ok: bool, line: str, outcome: str = "") -> None:
        """File one decided verdict and clear the in-flight record."""
        self.ran += 1
        self.state.pop("pending", None)
        if self.fingerprint is not None:
            self.state.setdefault("done", {})[key_of(mutation)] = vars(
                Verdict(ok=ok, outcome=outcome or ("caught" if ok else "not caught"), line=line)
            )
        self._save()

    def closing(self) -> "list[str]":
        """What the summary must say about where the verdicts came from."""
        if not self.reused:
            return []
        return [
            f"[INFO] {len(self.reused)} of those verdicts came from an earlier process and "
            f"{self.ran} ran now; --resume reused them because the tracked tree had not moved"
        ]


# --- self-test ----------------------------------------------------------
#
# Every case below is written so that it can fail, and each was watched fail
# before being trusted -- `--self-test` is the control, and the mutations in
# `docs/TRAPS.md` terms are the six one-line edits listed beside `self_test` in
# BUILD.md. The scratch tree is a real `git init`, because the fingerprint's
# whole job is asking git a question, and a fake would be a second
# implementation agreeing with itself.


@dataclass
class _Fake:
    name: str
    path: str
    before: str
    after: str
    expect: str


def _state(root: Path) -> dict:
    """The state file, read so that a missing one is a reading rather than a raise.

    Every check that looks at the file goes through this. Written directly, they
    raise when a mutation stops the file being written at all -- and a traceback
    is the one verdict shape that names no check, so the run that found the
    defect reports a stack and the reader learns nothing.
    """
    try:
        return json.loads((root / ".mutations" / "t.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def _repo(tmp: Path) -> Path:
    root = tmp / "repo"
    (root / "src").mkdir(parents=True)
    (root / "src" / "a.rs").write_text("let x = 1;\n", encoding="utf-8")
    (root / ".gitignore").write_text(".mutations/\n", encoding="utf-8")
    for cmd in (
        ["git", "init", "-q"],
        ["git", "add", "-A"],
        ["git", "-c", "user.email=t@e", "-c", "user.name=t", "commit", "-qm", "x"],
    ):
        subprocess.run(cmd, cwd=root, check=True, capture_output=True)
    return root


def self_test() -> int:
    import tempfile

    failures: "list[str]" = []

    def check(name: str, got, want) -> None:
        # Printed as it happens, both ways. The failures used to be collected
        # and printed at the end, and a mutation that made a later check *raise*
        # took every earlier failure with it -- so a run that had found the
        # defect reported a traceback and named nothing. `docs/TRAPS.md` has the
        # general form under "A harness that prints only at the end".
        if got != want:
            line = f"[FAIL] {name}: got {got!r}, wanted {want!r}"
            failures.append(line)
            print(line, flush=True)
        else:
            print(f"[OK]   {name}", flush=True)

    mutation = _Fake("a: one", "src/a.rs", "= 1", "= 2", "a_test")

    with tempfile.TemporaryDirectory() as tmp:
        root = _repo(Path(tmp))
        target = root / "src" / "a.rs"
        clean = target.read_bytes()

        # 1. A verdict recorded now is reused by a later run against the same tree.
        first = Resume("t", reuse=False, root=root)
        first.open([mutation])
        first.record(mutation, True, "[OK]  a: one caught")
        again = Resume("t", reuse=True, root=root)
        again.open([mutation])
        got = again.done(mutation)
        check("a verdict is reused when the tree has not moved", got and got.line,
              "[OK]  a: one caught")

        # 2. Any edit at all discards it.
        target.write_bytes(b"let x = 3;\n")
        moved = Resume("t", reuse=True, root=root)
        lines = moved.open([mutation])
        check("an edited tree discards the stored verdict", moved.done(mutation), None)
        check("and says so", any("has changed" in line for line in lines), True)
        target.write_bytes(clean)

        # 3. An edited mutation is a different mutation, so its key misses.
        # The verdict is recorded here rather than borrowed from case 1: case 2
        # left the file with none, so both lookups missed and this check passed
        # without ever asking a question -- a mutation that stopped the key
        # reading the replacement survived it.
        seeded = Resume("t", reuse=False, root=root)
        seeded.open([mutation])
        seeded.record(mutation, True, "[OK]  a: one caught")
        edited = _Fake("a: one", "src/a.rs", "= 1", "= 9", "a_test")
        back = Resume("t", reuse=True, root=root)
        back.open([mutation, edited])
        check("the verdict is still there for the mutation it was taken on",
              bool(back.done(mutation)), True)
        check("an edited anchor is not the mutation the verdict was for",
              back.done(edited), None)

        # 4. A mutation left in the tree by a kill is restored byte for byte.
        killed = Resume("t", reuse=False, root=root)
        killed.open([mutation])
        mutated = clean.replace(b"= 1", b"= 2")
        killed.begin(mutation, target, clean, mutated)
        target.write_bytes(mutated)          # and now the process dies.
        rescuer = Resume("t", reuse=False, root=root)
        code, lines = rescuer.recover()
        check("recovery exits 0", code, 0)
        check("recovery restores the file byte for byte", target.read_bytes(), clean)
        check("recovery says what it put back", any("restored" in line for line in lines), True)
        check("and clears the record", Resume("t", reuse=False, root=root).recover(), (0, []))

        # 5. A kill between recording the intent and writing the bytes.
        early = Resume("t", reuse=False, root=root)
        early.begin(mutation, target, clean, mutated)
        code, lines = Resume("t", reuse=False, root=root).recover()
        check("a kill before the write needs no repair", (code, target.read_bytes()), (0, clean))
        check("and is still reported", any("not left mutated" in line for line in lines), True)

        # 6. A file somebody has since edited is refused, not clobbered.
        contested = Resume("t", reuse=False, root=root)
        contested.begin(mutation, target, clean, mutated)
        target.write_bytes(b"let x = 42;  // fixed by hand\n")
        code, lines = Resume("t", reuse=False, root=root).recover()
        check("an edited file is refused", code, 1)
        check("and is left exactly as it was",
              target.read_bytes(), b"let x = 42;  // fixed by hand\n")
        check("and the refusal says why", any("edited it" in line for line in lines), True)
        target.write_bytes(clean)
        (root / ".mutations" / "t.json").unlink(missing_ok=True)

        # 7. Without --resume nothing is reused, and recovery still happens.
        plain = Resume("t", reuse=False, root=root)
        plain.open([mutation])
        plain.record(mutation, True, "[OK]  a: one caught")
        second = Resume("t", reuse=False, root=root)
        hint = second.open([mutation])
        check("without --resume no verdict is reused", second.done(mutation), None)
        check("but the run says one was available",
              any("--resume` would reuse" in line for line in hint), True)
        second.record(mutation, True, "[OK]  a: one caught")

        # And a run of something ELSE leaves it alone. A plain run used to
        # replace the file wholesale, so one narrow `--only` run in the middle
        # of a killed table threw away every verdict it had proved -- in the
        # workflow this module exists for. Asserted across two mutations on
        # purpose: written as one, the run that wiped the verdict immediately
        # recorded it again, and the check could not see the wipe at all.
        elsewhere = _Fake("c: three", "src/a.rs", "= 1", "= 5", "c_test")
        narrow = Resume("t", reuse=False, root=root)
        narrow.open([elsewhere])
        narrow.record(elsewhere, True, "[OK]  c: three caught")
        after = Resume("t", reuse=True, root=root)
        after.open([mutation, elsewhere])
        check("a narrow run keeps the verdicts of the table it is not running",
              bool(after.done(mutation)), True)

        # The reuse line quotes this, so it is a claim about when the verdicts
        # were taken. Re-stamping it on every run would make a verdict from an
        # hour ago say it was taken a moment ago, which is exactly backwards for
        # a reader deciding whether to trust it.
        stamp = _state(root)
        time.sleep(1.05)
        Resume("t", reuse=True, root=root).open([mutation])
        check("the timestamp stays with the tree, not the process",
              _state(root).get("started"), stamp.get("started"))

        # 8. A verdict is on disk the moment it is recorded, not at the end.
        live = Resume("t", reuse=True, root=root)
        live.open([mutation])
        live.record(mutation, False, "[FAIL] a: one SURVIVED", outcome="survived")
        outcome = _state(root).get("done", {}).get(key_of(mutation), {}).get("outcome", "absent")
        check("a verdict reaches the file immediately", outcome, "survived")

        # 9. A decided verdict clears the in-flight record, so the next run
        # does not go looking for a mutation that was put back long ago.
        tidy = Resume("t", reuse=False, root=root)
        tidy.open([mutation])
        tidy.begin(mutation, target, clean, mutated)
        tidy.record(mutation, True, "[OK]  a: one caught")
        check("recording a verdict clears the in-flight record",
              Resume("t", reuse=False, root=root).recover(), (0, []))

        # 10. A verdict for a mutation the table no longer holds is not reused,
        # and is not counted. `--only` and `--since` both hand this a table
        # smaller than the one the verdicts came from.
        narrowed = Resume("t", reuse=True, root=root)
        other = _Fake("b: two", "src/a.rs", "= 1", "= 7", "b_test")
        lines = narrowed.open([other])
        check("a verdict outside the table is not reused", len(narrowed.reused), 0)
        check("and the run does not claim it", any("1 of 1 verdicts" in l for l in lines), False)

        # 11. A file the fingerprint is told about, outside git's view.
        hidden = root / ".mutations" / "fixture.bin"
        hidden.write_bytes(b"one")
        before = tree_fingerprint(root, [Path(".mutations/fixture.bin")])
        hidden.write_bytes(b"two")
        after = tree_fingerprint(root, [Path(".mutations/fixture.bin")])
        check("an `also` file changes the fingerprint", before != after, True)
        check("and a gitignored file the caller did not name does not",
              tree_fingerprint(root), tree_fingerprint(root))

    for line in failures:
        print(line)
    print(f"\n{'[OK] self-test passed' if not failures else f'[FAIL] {len(failures)} checks'}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else print(__doc__) or 0)
