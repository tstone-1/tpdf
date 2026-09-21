#!/usr/bin/env python3
"""Every mutation's *replacement* still compiles against the tree it aims at.

`check_mutation_anchors.py` asks whether a mutation's `before` string is still
there. This asks the other half, and the two are not the same question: an
anchor that still matches is not a mutation that still works. The `before` can
sit untouched for months while the code around it grows a return value, an
enum variant or an argument that the `after` does not account for --- and then
the mutation no longer type-checks, so the harness that applies it gets a
compile error instead of a red test.

**The harness reports that correctly and far too late.** `mutate_rust.py` calls
it `no summary line -- the run did not finish`, which is the right words for a
mutation that cannot be executed; but the run that says so takes tens of
minutes, and until somebody makes it the table looks complete. Nothing else
says anything: `git status` is clean, the anchors gate is green, and the
mutation reads as coverage it is not providing.

Three of them were found in one week, each by a full `--since <tag>` run rather
than by any gate:

* `browser state: skip external state validation` and `image: skip image
  validation` (26.9.14) --- both calls had gained a return value the replacement
  did not supply.
* `edits: address a mark by its baseline page rather than its position`
  (26.9.16) --- its replacement rewrote a `match` that gained an arm when
  `PageSource::Imported` was added.

So the invariant here is: **for every mutation, the tree with that mutation
applied can still be built and run.** A mutation that fails it cannot be
executed at all, which makes it worth strictly less than no mutation --- no
mutation is an admitted gap in a table somebody can read, and this one is a gap
wearing a row. What "built and run" means differs by language and that
difference is load-bearing; `JS_FATAL` below has it.

--- how it is cheap ------------------------------------------------------

Applying every mutation one at a time and type-checking each would be hours ---
the tables are in the thousands, and `--list` prints the count rather than this
sentence carrying one. Three things bring it to seconds, and each was measured
rather than assumed (`docs/RATIONALE.md`, *Type-checking the mutation table*):

**It batches.** Mutations are applied together, as many as will fit, and one
check covers all of them. The only thing that stops two sharing a batch is
their anchors *overlapping* in the same file --- offsets are computed against
the original text, so simultaneous replacement is well defined. Measured
2026-09-21: the whole Rust table packed into **five** batches and the front end
into four, so the floor is nine compiles rather than a few thousand.

**It reads the compiler's file and line rather than bisecting.** A failing
batch is not searched: each error is attributed to the mutation whose replaced
span covers the line rustc named, that mutation is then checked *alone* to
separate "this replacement is broken" from "these two do not like each other",
and the batch is re-run without it. Bisection is still there for an error no
span covers, which is what happens when a mutation's damage surfaces in another
file.

**It caches on the thing the answer depends on.** A mutation's verdict is a
statement about the whole crate, not about its own file --- the 26.9.14 pair
broke because a callee elsewhere changed --- so the cache key is a digest of
every source file the checker reads. Any edit to any of them re-runs the whole
group. That is deliberately blunt: a narrower key would be a cache that can be
silently wrong, which is the one thing a check like this must not be.

The cache can only lose entries, never gain them. It is keyed per mutation
*and* per fingerprint, so a mutation added to the table while the sources sit
still is unverified and gets checked; a fingerprint that moves invalidates
everything. Deleting `.mutations/types.json` costs time and nothing else.

--- what it does not cover ------------------------------------------------

**Code this platform does not compile.** A mutation declaring `only_on` for
another platform is skipped and named; those are counted in the summary rather
than folded into it. The residual is narrower and real: a mutation *without*
`only_on` that happens to sit inside a `#[cfg(windows)]` region is parsed here
but not type-checked, so its verdict on a Mac is syntax-only. Run this on both
platforms --- CI does --- and the pair covers it.

**Tables with no type checker behind them.** `.md`, `.yml`, `.py`, `.toml` and
`.json` anchors are reported as unverifiable, by suffix and with a count, so the
population that is not covered is a number on the screen rather than an absence.

**A running mutation harness.** This writes to the working tree, exactly as the
harnesses do, so the two must not share a checkout at the same time --- the same
rule, for the same reason. A run killed between the backup and the restore is
recovered from `.mutations/types-inflight.json` by the next one, loudly.

Usage:
    scripts/check_mutation_types.py              # both groups, using the cache
    scripts/check_mutation_types.py --all        # ignore the cache
    scripts/check_mutation_types.py --group rust
    scripts/check_mutation_types.py --list       # and what cannot be checked
    scripts/check_mutation_types.py --self-test  # the control
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mutation_resume import write_newer  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "src-tauri"
STATE = ROOT / ".mutations"
CACHE = STATE / "types.json"
#: Where a batch's original bytes go before anything is written over them, and
#: what a killed run is recovered from. Same reasoning as `mutation_resume.py`'s
#: backup: a `finally` does not survive a kill, and a mutation left on the tree
#: is invisible in `git status` on a branch that is already editing that file.
INFLIGHT = STATE / "types-inflight.json"
BACKUP = STATE / "types-backup"

#: The cache format. Bumped when a key's meaning changes, which discards rather
#: than reinterprets --- a stored verdict that is read under new rules is worse
#: than no stored verdict.
VERSION = 1

#: Which platform this is, in the vocabulary `Mutation.only_on` uses. Same
#: spelling as `mutate_rust.py`'s, deliberately: the two must agree about what
#: `only_on="macos"` selects.
HERE = "macos" if sys.platform == "darwin" else "windows" if sys.platform == "win32" else "linux"

#: The tables, and the directory each one's paths are relative to --- the same
#: pairing `check_mutation_anchors.py` uses, and for the same reason:
#: `mutate_rust` names paths inside the crate, the others from the repository
#: root.
TABLES = [
    ("scripts/mutate_rust.py", CRATE),
    ("scripts/mutate_frontend.py", ROOT),
    ("scripts/mutate_viewer.py", ROOT),
    ("scripts/mutate_python.py", ROOT),
]

#: Suffixes with a type checker behind them, and the group each belongs to.
#: Everything else is reported as unverifiable, by suffix.
GROUP_OF = {".rs": "rust", ".ts": "js", ".svelte": "js"}

#: Why each unverifiable suffix is unverifiable. Written out rather than left as
#: "not .rs or .ts", because the reason is what decides whether it is worth
#: fixing: a `.py` anchor could have a syntax check put behind it and would
#: still not catch the arity change this gate exists for, while a `.md` anchor
#: has nothing to check at all.
UNVERIFIABLE = {
    ".md": "prose: nothing type-checks it",
    ".yml": "a workflow: its checker is check_workflow_parity.py, not a compiler",
    ".toml": "configuration: parsed by cargo, not type-checked",
    ".json": "configuration: parsed at run time",
    ".nsh": "an NSIS installer hook: read by makensis at package time",
    ".py": (
        "Python: a syntax check would pass the arity and name changes this gate "
        "exists for, so it would be a check that looked"
    ),
    "": "no path: the mutation edits something this gate cannot open",
}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def npm() -> str:
    """Resolves npm, which is `npm.cmd` on Windows and not on PATH as `npm`."""
    return shutil.which("npm") or "npm"


def npx() -> str:
    return shutil.which("npx") or "npx"


def load(path: str):
    """The module, imported for its `MUTATIONS` table alone."""
    name = Path(path).stem
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"[FAIL] cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    # Registered before exec, as in `check_mutation_anchors.py`: `@dataclass`
    # resolves its own module through `sys.modules`.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def key_of(table: str, mutation) -> str:
    """What a verdict is filed under.

    Everything that decides what the mutation *is*, plus the table it came from
    --- two tables may hold the same edit for different reasons, and a verdict
    about one is not a verdict about the other. Deliberately a superset of
    `mutation_resume.key_of`'s parts rather than a call to it: that key exists
    to identify a run's verdict and this one to identify a compile, and coupling
    them would make a change to either silently discard the other's cache.
    """
    parts = [
        table,
        mutation.name,
        str(mutation.path),
        mutation.before,
        mutation.after,
    ]
    return digest("\0".join(parts).encode("utf-8"))


# --- what each group is checked with --------------------------------------


def rust_sources() -> "list[Path]":
    """Every file a `cargo check` of the lib reads and could change its answer.

    `Cargo.lock` and `Cargo.toml` are in because a dependency's version decides
    what type-checks; `build.rs` and `tauri.conf.json` because the build script
    reads the second and generates code from it; `rust-toolchain.toml` because a
    different rustc is a different answer.

    `target/` is excluded by the walk, and `warm.pdf` and the other non-`.rs`
    payloads are not here on purpose: `include_bytes!` changes no type.
    """
    # `CRATE / "src"`, never `CRATE.rglob` with `target` filtered out of the
    # results: the walk still *visits* `src-tauri/target`, which is a couple of
    # hundred gigabytes of build artifacts here. Measured 2026-09-21 --- the
    # cached run, which does nothing but hash, took **17.6 s** that way and
    # 0.6 s from this one. A filter applied after the walk costs what the walk
    # costs.
    files = list((CRATE / "src").rglob("*.rs"))
    for name in ("Cargo.toml", "Cargo.lock", "build.rs", "tauri.conf.json"):
        if (CRATE / name).exists():
            files.append(CRATE / name)
    if (ROOT / "rust-toolchain.toml").exists():
        files.append(ROOT / "rust-toolchain.toml")
    return files


def js_sources() -> "list[Path]":
    """Every file `tsc` and `svelte-check` read, plus what pins their types.

    `package-lock.json` rather than `node_modules`: the lockfile is what decides
    which `@types/*` are installed, and walking `node_modules` to hash it would
    cost more than the check it is protecting.
    """
    files = [
        p
        for pattern in ("*.ts", "*.svelte")
        for p in (ROOT / "src").rglob(pattern)
        if "node_modules" not in p.parts
    ]
    for name in ("tsconfig.json", "package.json", "package-lock.json", "svelte.config.js", "vite.config.ts"):
        if (ROOT / name).exists():
            files.append(ROOT / name)
    return sorted(set(files))


def tool_version(argv: "list[str]") -> str:
    """The checker's own version, folded into the fingerprint.

    A cached verdict is a statement about a compiler as much as about a tree. A
    toolchain bump that changes nothing else must still re-run the sweep, and
    without this it would not --- which is the same defect as a cache keyed on
    one file when the answer depends on the crate.
    """
    try:
        done = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.SubprocessError):
        return "unknown"
    return (done.stdout + done.stderr).strip()


def fingerprint(files: "list[Path]", version: str) -> str:
    """A digest of every file the checker reads, and of the checker."""
    parts = [f"v{VERSION}", version]
    for path in sorted(files):
        try:
            parts.append(f"{path.relative_to(ROOT).as_posix()}:{digest(path.read_bytes())}")
        except OSError:
            parts.append(f"{path.relative_to(ROOT).as_posix()}:missing")
    return digest("\0".join(parts).encode("utf-8"))


#: `rustc --error-format=short` writes `path:line:col: error[E0282]: ...`; the
#: summary lines (`error: could not compile ...`) carry no path and are skipped
#: by the same pattern, which is why it anchors on the colon-separated prefix.
RUST_ERROR = re.compile(r"^(?P<path>[^\s:][^:]*):(?P<line>\d+):\d+: error")
#: `tsc --noEmit` writes `src/lib/viewer.ts(120,7): error TS2554: ...`.
TS_ERROR = re.compile(r"^(?P<path>[^\s(]+)\((?P<line>\d+),\d+\): error(?: TS(?P<code>\d+))?")

#: The TypeScript diagnostics this gate fails on, and why each one is fatal.
#:
#: **The two groups are asked different questions, and that is the whole design
#: of this file.** For Rust the question is *does it compile*, because cargo
#: refuses to run a mutation that does not and the harness gets a compile error
#: where it wanted a red test. For TypeScript it cannot be: vitest transpiles
#: with esbuild and never type-checks, so **no** diagnostic stops a mutation
#: running --- and the front-end table is full of mutations whose whole point is
#: a type error. Passing a slot number where a branded `FilePage` is wanted is
#: how you mutate a page-addressing bug into existence; `noUncheckedIndexedAccess`
#: makes any dropped guard an error; removing a call leaves its import unread.
#: Measured 2026-09-21 over the whole table: requiring the front end to
#: type-check reported **40** mutations, and 36 of them were deliberate and run
#: exactly as written. A gate that red-flags working mutations would be answered
#: by casting them until it stopped, which is worse than not having it.
#:
#: So the front-end criterion is the same claim in the vocabulary that pipeline
#: has: **a replacement that cannot even run.** These are the diagnostics whose
#: runtime consequence is a thrown `ReferenceError`, `SyntaxError` or strict-mode
#: `TypeError` rather than a wrong value --- and the four they found were all
#: genuinely vacuous mutations, each reddening its test for a reason that had
#: nothing to do with what it claimed to break.
#:
#: Everything else is counted and reported rather than dropped, so the size of
#: the population this gate does not fail on is a number on the screen.
JS_FATAL = {
    "2304": "a name that does not exist -- a ReferenceError at run time",
    "2552": "a name that does not exist",
    "2307": "a module that does not exist",
    "2300": "a duplicate declaration -- a SyntaxError",
    "2451": "a redeclared block-scoped variable -- a SyntaxError",
    "2448": "a block-scoped name used before its declaration -- a ReferenceError",
    "2588": "an assignment to a constant -- a TypeError in a module's strict mode",
    "2540": "an assignment to a read-only property -- a TypeError in strict mode",
}
#: `svelte-check` writes a machine-readable stream when asked; the default
#: human output puts the path on its own line. Both forms are matched, because
#: `npm run check` runs `tsc` first and only reaches `svelte-check` when tsc is
#: clean --- so in practice only one of the two ever produces the errors here.
SVELTE_ERROR = re.compile(r"^\d+ (?P<path>\S+):(?P<line>\d+):\d+ ")


class Checker:
    """One type checker, and how to read what it says."""

    def __init__(self, name: str, base: Path) -> None:
        self.name = name
        self.base = base

    def run(self, uses_svelte: bool):
        """Returns (ok, fatal spots, soft spots, the whole transcript).

        A *spot* is `(path, line, the diagnostic as printed)`. `soft` is what
        the checker complained about and this gate does not fail on; it is
        carried rather than dropped so the summary can say how large that
        population is, which is the difference between a stated exemption and a
        silent one.
        """
        raise NotImplementedError


class Cargo(Checker):
    """`cargo check --lib --tests`, in the mutation harness's target directory.

    `--tests` is not decoration. `cargo check --lib` alone does not compile
    `#[cfg(test)]` code, and what the harness runs is `cargo test --lib`, which
    does --- so without it this gate would be checking a different program from
    the one whose compile failure it exists to predict.

    **`mutate_rust.py`'s target directory, with its `CARGO_PROFILE_DEV_DEBUG=0`**,
    so the two share one set of dependency artifacts rather than each paying for
    its own. Sharing the *default* directory instead was the obvious idea and was
    measured before being rejected: the same clean sweep took **86.7 s** there
    against **21.4 s** here, warm both times, and the extra was almost all `sys`
    --- cargo stat-ing a `target/debug` that is two hundred gigabytes of debug
    builds. A directory holding only check artifacts is scanned in a fraction of
    the time, and the harness had already paid to create it.
    """

    def __init__(self) -> None:
        super().__init__("cargo", CRATE)
        self.env = {
            **os.environ,
            "CARGO_TARGET_DIR": str(CRATE / "target" / "mutations"),
            "CARGO_PROFILE_DEV_DEBUG": "0",
        }

    def run(self, uses_svelte: bool = False):
        done = subprocess.run(
            ["cargo", "check", "--lib", "--tests", "--message-format=short"],
            cwd=CRATE,
            env=self.env,
            capture_output=True,
            text=True,
            # As in the harnesses: `text=True` alone decodes with the locale
            # codec, cp1252 on Windows, and these sources hold bytes it cannot
            # read.
            encoding="utf-8",
            errors="replace",
            timeout=1800,
        )
        out = done.stdout + done.stderr
        spots = [
            (m.group("path"), int(m.group("line")), line.strip())
            for line in out.splitlines()
            for m in [RUST_ERROR.match(line.strip())]
            if m
        ]
        # Nothing soft here: cargo refuses to build, so every error is fatal to
        # the mutation. The empty list is the claim, not an omission.
        return done.returncode == 0, spots, [], out


class Typescript(Checker):
    """`tsc --noEmit`, or the whole `npm run check` when a `.svelte` is mutated.

    Measured 2026-09-21: `tsc --noEmit` is 0.4 s and `npm run check` is 16.3 s,
    because the second adds `svelte-check`. Three mutations in the whole tree
    aim at a `.svelte` file, so paying the 16 s for those and 0.4 s for the
    other 812 is the difference between a gate and a coffee break.
    """

    def __init__(self) -> None:
        super().__init__("tsc", ROOT)

    def run(self, uses_svelte: bool = False):
        argv = (
            [npm(), "run", "check"]
            if uses_svelte
            else [npx(), "tsc", "--noEmit", "--project", "tsconfig.json"]
        )
        done = subprocess.run(
            argv,
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=1800,
        )
        out = done.stdout + done.stderr
        spots, soft, seen = [], [], 0
        for line in out.splitlines():
            for pattern in (TS_ERROR, SVELTE_ERROR):
                m = pattern.match(line.strip())
                if not m:
                    continue
                seen += 1
                where = (m.group("path"), int(m.group("line")), line.strip())
                fatal = pattern is not TS_ERROR or m.group("code") in JS_FATAL
                (spots if fatal else soft).append(where)
                break
        # Not the exit code, because a soft diagnostic still makes tsc exit 1
        # --- and not "no fatal spots" either, because a checker that fell over
        # prints no diagnostics at all and would then read as clean. It is a
        # pass only when the run succeeded, or when everything it did print is
        # something this gate has a stated reason not to fail on.
        return (done.returncode == 0 or (seen > 0 and not spots)), spots, soft, out


# --- applying, and putting it back ----------------------------------------


class Tree:
    """Holds the original bytes, writes mutated ones, and can always undo.

    Three rules, all of them paid for once by the harnesses and recorded in
    `docs/TRAPS.md`: the backup is on disk *before* anything is written over
    it, the restore writes bytes rather than copying a file over them, and the
    restored file is left newer than the mutation so cargo does not go on
    serving what it built from the mutated copy.
    """

    def __init__(self, base: Path) -> None:
        self.base = base
        self.clean: "dict[str, bytes]" = {}
        self.text: "dict[str, str]" = {}
        self.crlf: "dict[str, bool]" = {}
        self.dirty: "set[str]" = set()

    def source(self, path: str) -> str:
        """The file as the anchors are written: decoded, and newline-normalised.

        The file's own convention goes back on the way out. `.gitattributes` has
        pinned `eol=lf` since 2026-08-26 so this is rarely the CRLF branch, and
        it is kept for the same reason `mutate_rust.py` keeps its own: a
        checkout is not the only thing that writes a file.
        """
        if path not in self.text:
            raw = (self.base / path).read_bytes()
            self.clean[path] = raw
            decoded = raw.decode("utf-8")
            self.crlf[path] = "\r\n" in decoded
            self.text[path] = decoded.replace("\r\n", "\n") if self.crlf[path] else decoded
        return self.text[path]

    def apply(self, edits: "dict[str, list[tuple[int, int, str]]]") -> None:
        """Write every file in `edits`, each with all of its replacements."""
        STATE.mkdir(exist_ok=True)
        BACKUP.mkdir(exist_ok=True)
        # The record before the bytes. A kill between the two leaves a backup
        # for a file that was never written, which restores to itself; the other
        # order leaves a mutated file nothing knows about.
        held = sorted(edits)
        for path in held:
            (BACKUP / digest(path.encode())).write_bytes(self.clean[path])
        INFLIGHT.write_text(
            json.dumps({"base": str(self.base), "paths": held}, indent=1),
            encoding="utf-8",
        )
        for path, items in edits.items():
            text = self.text[path]
            out, last = [], 0
            for start, end, after in sorted(items):
                out.append(text[last:start])
                out.append(after)
                last = end
            out.append(text[last:])
            mutated = "".join(out)
            if self.crlf[path]:
                mutated = mutated.replace("\n", "\r\n")
            write_newer(self.base / path, mutated.encode("utf-8"))
            self.dirty.add(path)

    def restore(self) -> None:
        for path in sorted(self.dirty):
            write_newer(self.base / path, self.clean[path])
        self.dirty.clear()
        INFLIGHT.unlink(missing_ok=True)


def recover() -> "list[str]":
    """Put back whatever a killed run left on the tree, and say so.

    Silence here would be the failure `check_mutation_anchors.py` was written
    for arriving through a different door: a mutated file sitting in a tree that
    `git status` reads as clean, because the branch is already editing it.
    """
    if not INFLIGHT.exists():
        return []
    try:
        state = json.loads(INFLIGHT.read_text(encoding="utf-8"))
        base = Path(state["base"])
        paths = list(state["paths"])
    except (OSError, ValueError, KeyError):
        return [
            f"[FAIL] {INFLIGHT} is unreadable -- a run was killed and this cannot "
            "say what it left behind. Check `git diff` by hand."
        ]
    notes = []
    for path in paths:
        blob = BACKUP / digest(path.encode())
        if not blob.exists():
            notes.append(f"[FAIL] no backup for {path} -- restore it by hand")
            continue
        write_newer(base / path, blob.read_bytes())
        notes.append(f"[WARN] put back {path}: a killed run had left its edit on the tree")
    INFLIGHT.unlink(missing_ok=True)
    return notes


# --- the sweep -------------------------------------------------------------


class Item:
    """One mutation, resolved against the tree: where it lands and under what key."""

    __slots__ = ("table", "mutation", "path", "start", "end", "key")

    def __init__(self, table: str, mutation, path: str, start: int, end: int) -> None:
        self.table = table
        self.mutation = mutation
        self.path = path
        self.start = start
        self.end = end
        self.key = key_of(table, mutation)

    @property
    def name(self) -> str:
        return f"{self.table}: {self.mutation.name}"


def batched(items: "list[Item]") -> "list[list[Item]]":
    """Greedy packing: as few batches as overlapping anchors allow.

    Two mutations may share a batch unless their spans intersect in the same
    file, because every offset is taken against the *original* text and a set of
    disjoint replacements is well defined however many there are. The whole Rust
    table packs into five batches, so a sweep is five compiles rather than 1,554.
    """
    batches: "list[dict[str, list[tuple[int, int]]]]" = []
    out: "list[list[Item]]" = []
    for item in items:
        for index, taken in enumerate(batches):
            if all(
                not (item.start < end and start < item.end)
                for start, end in taken.get(item.path, [])
            ):
                taken.setdefault(item.path, []).append((item.start, item.end))
                out[index].append(item)
                break
        else:
            batches.append({item.path: [(item.start, item.end)]})
            out.append([item])
    return out


def edits_for(items: "list[Item]") -> "dict[str, list[tuple[int, int, str]]]":
    edits: "dict[str, list[tuple[int, int, str]]]" = {}
    for item in items:
        edits.setdefault(item.path, []).append((item.start, item.end, item.mutation.after))
    return edits


def attribute(items: "list[Item]", spots: "list[tuple[str, int, str]]", tree: Tree) -> "set[int]":
    """Which mutations the compiler's own file:line land inside.

    The mutated text is rebuilt per file so each replacement's *new* span is
    known, and an error line inside one of those spans names its mutation. This
    is what makes a failing batch cost one more compile rather than eleven: the
    compiler already knows where the problem is, and bisection is only the
    fallback for an error that lands somewhere no replacement covers.
    """
    hit: "set[int]" = set()
    by_path: "dict[str, list[tuple[int, Item]]]" = {}
    for index, item in enumerate(items):
        by_path.setdefault(item.path, []).append((index, item))
    for path, group in by_path.items():
        text = tree.text[path]
        out, last, spans = [], 0, []
        for index, item in sorted(group, key=lambda pair: pair[1].start):
            out.append(text[last:item.start])
            at = sum(len(chunk) for chunk in out)
            out.append(item.mutation.after)
            spans.append((at, at + len(item.mutation.after), index))
            last = item.end
        out.append(text[last:])
        mutated = "".join(out)
        # Line starts, once per file. A per-error scan of the whole text would
        # be the same answer and quadratic in the error count.
        starts = [0]
        for line in mutated.split("\n"):
            starts.append(starts[-1] + len(line) + 1)
        for spot_path, line, _text in spots:
            # The compiler names the path relative to its own working directory,
            # which is the base the mutation's path is relative to. Compared as
            # posix on both sides: rustc and tsc both emit forward slashes.
            if Path(spot_path).as_posix() != Path(path).as_posix():
                continue
            if line < 1 or line >= len(starts):
                continue
            low, high = starts[line - 1], starts[line]
            for start, end, index in spans:
                if start < high and low < end:
                    hit.add(index)
    return hit


def sweep(
    label: str,
    checker: Checker,
    tree: Tree,
    items: "list[Item]",
    solo: "set[str]",
    log,
) -> "tuple[set[str], dict[str, str], set[str], set[str], list[str]]":
    """Check every item: (verified, broken, interacting, soft, notes).

    `soft` names the mutations a checker complained about without this gate
    failing on it --- the front end's deliberate type errors. Collected only so
    the summary can print how many there are: an exemption whose size nobody
    can see is the same thing as no exemption at all.

    `solo` names keys that a previous run found could not share a batch. They go
    in singleton batches from the start, which costs a compile each and saves
    rediscovering them. Reusing it is sound in one direction only, and that is
    the safe one: checking a mutation alone is strictly stronger than checking
    it among others, so a stale entry costs time and can never excuse a failure.
    """
    verified: "set[str]" = set()
    broken: "dict[str, str]" = {}
    interacting: "set[str]" = set()
    softly: "set[str]" = set()
    notes: "list[str]" = []
    compiles = 0

    pending = [i for i in items if i.key not in solo]
    alone = [i for i in items if i.key in solo]

    def check(group: "list[Item]"):
        nonlocal compiles
        compiles += 1
        tree.apply(edits_for(group))
        try:
            ok, spots, soft, out = checker.run(
                any(i.path.endswith(".svelte") for i in group)
            )
        finally:
            tree.restore()
        if soft:
            softly.update(group[i].key for i in attribute(group, soft, tree))
        return ok, spots, out

    def singly(item: Item) -> None:
        """Settle one mutation on its own, and say which of the two it is.

        A batch failure does not distinguish "this replacement is broken" from
        "these two do not like each other", and only the first is a defect in
        the table. The reported line comes from `spots` rather than from a scan
        of the output, so it is one of the diagnostics this gate actually
        counted --- a first line picked by grep can be an ignored one, which
        makes the verdict read as being about something it is not.
        """
        ok, spots, out = check([item])
        if ok:
            verified.add(item.key)
            # Only an item that reached here from a *failing* batch is really
            # interacting. One that was already in `solo` passes through, which
            # keeps it there --- the list is cheap to be wrong about in that
            # direction and expensive in the other.
            interacting.add(item.key)
            return
        broken[item.key] = (
            spots[0][2]
            if spots
            else next(
                (l.strip() for l in out.splitlines() if ": error" in l or l.startswith("error")),
                "no diagnostic could be read from the checker",
            )
        )

    # The control, and it is not a formality: a tree that does not compile
    # before anything is mutated makes **every** batch fail, and attribution
    # then names whichever mutation happens to sit on the compiler's line. That
    # is a wrong diagnosis rather than a missing one --- the shape this
    # repository keeps writing traps about --- and it is the everyday case for
    # a gate, because the everyday reason to run one is that you have just
    # edited the code. `mutate_rust.py` opens the same way, and for the same
    # reason.
    ok, spots, _soft, out = checker.run(False)
    compiles += 1
    if not ok:
        first = (
            spots[0][2]
            if spots
            else next(
                (l.strip() for l in out.splitlines() if ": error" in l or l.startswith("error")),
                "no diagnostic could be read",
            )
        )
        notes.append(
            f"[FAIL] {label}: the tree does not compile before anything is mutated, "
            f"so nothing below would be about a mutation -- {first}"
        )
        log(f"       {label}: {compiles} compile(s)")
        return verified, broken, interacting, softly, notes
    log(f"[OK]   {label}: the unmutated tree compiles")

    for item in alone:
        singly(item)

    # Each round removes at least one mutation from `pending` --- an attributed
    # suspect is settled by `singly` and never returns --- so this terminates.
    # The bound is a guard against a checker whose errors name nothing, which
    # would otherwise loop on an empty suspect set; it is loud rather than a
    # quiet exit, because stopping early is exactly "a check that checked
    # nothing".
    rounds = 0
    while pending:
        rounds += 1
        if rounds > 60:
            notes.append(
                f"[FAIL] {label}: gave up after {rounds - 1} rounds with "
                f"{len(pending)} mutations unchecked -- this proved nothing about them"
            )
            for item in pending:
                broken.setdefault(item.key, "unchecked: the sweep did not converge")
            break
        still: "list[Item]" = []
        for group in batched(pending):
            ok, spots, out = check(group)
            if ok:
                verified.update(i.key for i in group)
                continue
            suspects = attribute(group, spots, tree)
            if not suspects:
                # Nothing the compiler named lands in a replacement, so the
                # damage surfaced somewhere else. Bisection is the only thing
                # left, and it is the rare path rather than the design.
                suspects = bisect(group, check)
                notes.append(
                    f"[WARN] {label}: an error named no mutated line; "
                    f"bisected {len(group)} to {len(suspects)}"
                )
            if not suspects:
                notes.append(
                    f"[FAIL] {label}: a batch of {len(group)} does not compile and "
                    "neither attribution nor bisection could name a mutation in it"
                )
                for item in group:
                    broken.setdefault(item.key, "unattributed batch failure")
                continue
            for index in sorted(suspects):
                singly(group[index])
            still.extend(item for index, item in enumerate(group) if index not in suspects)
        if len(still) == len(pending):
            # No suspect was settled, so another round would ask the same
            # question. Cannot happen while `suspects` is non-empty; the guard
            # is here because "loops forever" and "passes" look the same from a
            # gate summary.
            notes.append(f"[FAIL] {label}: a round settled nothing; {len(still)} left unchecked")
            for item in still:
                broken.setdefault(item.key, "unchecked: a round settled nothing")
            break
        pending = still

    log(f"       {label}: {compiles} compile(s)")
    return verified, broken, interacting, softly, notes


def bisect(group: "list[Item]", check) -> "set[int]":
    """Halve until a single mutation fails. Indices are into `group`."""
    if len(group) == 1:
        return {0}
    half = len(group) // 2
    found: "set[int]" = set()
    for offset, part in ((0, group[:half]), (half, group[half:])):
        ok, _, _ = check(part)
        if not ok:
            found.update(offset + i for i in bisect(part, check))
    return found


# --- the cache -------------------------------------------------------------


def read_cache() -> dict:
    try:
        state = json.loads(CACHE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return state if state.get("version") == VERSION else {}


def write_cache(state: dict) -> None:
    STATE.mkdir(exist_ok=True)
    state["version"] = VERSION
    temp = CACHE.with_suffix(".json.part")
    temp.write_text(json.dumps(state, indent=1, sort_keys=True), encoding="utf-8")
    temp.replace(CACHE)


# --- main ------------------------------------------------------------------


#: Where each group's checker runs, which is what the paths it prints are
#: relative to --- and therefore what an `Item.path` has to be relative to for
#: the compiler's own file:line to be usable.
BASE_OF = {"rust": CRATE, "js": ROOT}


def relocate(table_base: Path, group: str, path: str) -> "str | None":
    """One table's path, expressed from its group's checker's directory.

    The four tables do not agree about this and cannot be made to: `mutate_rust`
    names paths inside the crate and `mutate_viewer` names the same crate's
    files from the repository root (`src-tauri/src/save/marks.rs`). Both are
    right for their own harness, so the conversion belongs here.
    """
    try:
        return (table_base / path).resolve().relative_to(BASE_OF[group].resolve()).as_posix()
    except ValueError:
        return None


def collect(only_table: "str | None" = None):
    """Resolve every table into items, by group.

    Returns (items by group, unresolved anchors, unverifiable counts by suffix,
    names skipped as another platform's, the two trees).
    """
    items: "dict[str, list[Item]]" = {"rust": [], "js": []}
    unresolved: "list[str]" = []
    unverifiable: "dict[str, int]" = {}
    elsewhere: "list[str]" = []
    trees = {"rust": Tree(CRATE), "js": Tree(ROOT)}
    for table, base in TABLES:
        if only_table and table != only_table:
            continue
        for mutation in load(table).MUTATIONS:
            suffix = Path(mutation.path).suffix if mutation.path else ""
            group = GROUP_OF.get(suffix)
            if group is None:
                unverifiable[suffix] = unverifiable.get(suffix, 0) + 1
                continue
            only_on = getattr(mutation, "only_on", None)
            if only_on and only_on != HERE:
                elsewhere.append(f"{table}: {mutation.name}")
                continue
            rel = relocate(base, group, mutation.path)
            if rel is None:
                unresolved.append(
                    f"{table}: {mutation.name} -- {mutation.path} is outside "
                    f"{BASE_OF[group]}, which is where its checker runs"
                )
                continue
            tree = trees[group]
            try:
                text = tree.source(rel)
            except OSError:
                unresolved.append(f"{table}: {mutation.name} -- cannot read {mutation.path}")
                continue
            if text.count(mutation.before) != 1:
                unresolved.append(
                    f"{table}: {mutation.name} -- its anchor occurs "
                    f"{text.count(mutation.before)}x in {mutation.path}, so nothing "
                    "could be applied (check_mutation_anchors.py owns this one)"
                )
                continue
            start = text.index(mutation.before)
            items[group].append(Item(table, mutation, rel, start, start + len(mutation.before)))
    return items, unresolved, unverifiable, elsewhere, trees


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument(
        "--all",
        action="store_true",
        help="ignore the cache and re-check every mutation",
    )
    parser.add_argument(
        "--group",
        action="append",
        choices=["rust", "js"],
        help="check only this group (repeatable; default: both)",
    )
    parser.add_argument("--list", action="store_true", help="say what would be checked and stop")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="the control: a deliberately uncompilable mutation must be reported",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    recovery = recover()
    for line in recovery:
        print(line, flush=True)
    if any(line.startswith("[FAIL]") for line in recovery):
        # A recovery that could not put a file back leaves a mutation on the
        # tree, and every answer below would be about that tree rather than
        # about this one. Refusing here is the same rule `mutate_rust.py`
        # applies to a restore it could not complete.
        print("[FAIL] the tree could not be recovered, so nothing below would be about it")
        return 1

    started = time.monotonic()
    items, unresolved, unverifiable, elsewhere, trees = collect()

    checkers = {"rust": Cargo(), "js": Typescript()}
    fingerprints = {
        "rust": fingerprint(rust_sources(), tool_version(["cargo", "--version"])),
        "js": fingerprint(js_sources(), tool_version([npx(), "tsc", "--version"])),
    }
    groups = args.group or ["rust", "js"]

    if args.list:
        for group in groups:
            print(f"{group}: {len(items[group])} mutation(s), {len(batched(items[group]))} batch(es)")
        for suffix, count in sorted(unverifiable.items()):
            print(f"unverifiable {suffix or '(no path)'}: {count} -- {UNVERIFIABLE.get(suffix, 'no checker')}")
        return 0

    cache = read_cache()
    problems: "list[str]" = []
    refusals: "list[str]" = []
    total_verified = total_reused = 0

    for group in groups:
        stored = cache.get(group, {}) if not args.all else {}
        fresh = stored.get("fingerprint") == fingerprints[group]
        known = set(stored.get("verified", [])) if fresh else set()
        solo = set(stored.get("solo", []))
        todo = [i for i in items[group] if i.key not in known]
        reused = len(items[group]) - len(todo)
        total_reused += reused
        if not todo:
            # Said out loud, with the count. "Nothing to do" and "everything
            # passed" are different facts, and a summary that prints the second
            # for the first is how a cache becomes a way of checking nothing.
            print(
                f"[OK]   {group}: all {len(items[group])} already verified against "
                f"these sources (cached)",
                flush=True,
            )
            continue
        print(
            f"--- {group}: {len(todo)} to check"
            + (f", {reused} reused from the cache" if reused else "")
            + f", in {len(batched([i for i in todo if i.key not in solo]))} batch(es)"
            + (f" plus {len([i for i in todo if i.key in solo])} checked alone" if solo else ""),
            flush=True,
        )
        verified, broken, interacting, softly, notes = sweep(
            group, checkers[group], trees[group], todo, solo, lambda line: print(line, flush=True)
        )
        # `[FAIL]` notes are held back and printed with the other refusals
        # below, so one does not appear twice in a transcript somebody is
        # counting failures in.
        for line in notes:
            if not line.startswith("[FAIL]"):
                print(line, flush=True)
        # A `[FAIL]` note is a statement about the run rather than about one
        # mutation --- the tree not compiling, or the sweep not converging ---
        # and it has to reach the exit code by itself. Nothing else would carry
        # it: `problems` is keyed by mutation, and a run that could not start is
        # a run with no mutation to blame.
        refused = [line for line in notes if line.startswith("[FAIL]")]
        if refused:
            refusals.extend(line[len("[FAIL] "):] for line in refused)
            continue
        if softly:
            print(
                f"[INFO] {group}: at least {len(softly)} mutation(s) do not type-check "
                "and run anyway -- this group is checked for names that do not resolve, "
                "not for types (see JS_FATAL)",
                flush=True,
            )
        total_verified += len(verified)
        for item in todo:
            if item.key in broken:
                problems.append(f"{item.name}\n       {broken[item.key]}")
        # Only what this run proved, plus what the cache already held for the
        # same fingerprint. A broken mutation is not written down as verified,
        # so fixing it and re-running re-checks it rather than reading a verdict
        # taken before the fix.
        cache[group] = {
            "fingerprint": fingerprints[group],
            "verified": sorted(known | verified),
            "solo": sorted(solo | interacting),
        }
        write_cache(cache)

    seconds = time.monotonic() - started
    print()
    for suffix, count in sorted(unverifiable.items()):
        print(f"[INFO] {count} mutation(s) in {suffix or '(no path)'}: {UNVERIFIABLE.get(suffix, 'no checker')}")
    if elsewhere:
        print(f"[INFO] {len(elsewhere)} mutation(s) skipped: another platform's, and this is {HERE}")
    for line in unresolved + refusals:
        print(f"[FAIL] {line}")
    if problems:
        print()
        for line in problems:
            print(f"[FAIL] {line}")
        print(
            f"\n[FAIL] {len(problems)} mutation(s) cannot be executed as written against "
            "this tree. A Rust one would report `no summary line -- the run did not finish`; "
            "a front-end one would throw where its test expected a behaviour change, and be "
            "filed as caught."
        )
        return 1
    if unresolved or refusals:
        return 1
    checked = sum(len(items[g]) for g in groups)
    if not checked:
        print("[FAIL] no mutation was checked -- this run proved nothing")
        return 1
    print(
        f"[OK] all {checked} mutation(s) in {', '.join(groups)} can still be applied and run "
        f"({total_verified} checked now, {total_reused} cached) in {seconds:.1f}s"
    )
    return 0


# --- the control -----------------------------------------------------------


class Broken:
    """A mutation whose replacement cannot compile, on purpose."""

    def __init__(self, name: str, path: str, before: str, after: str) -> None:
        self.name = name
        self.path = path
        self.before = before
        self.after = after
        self.expect = "none"
        self.only_on = None


def self_test() -> int:
    """A check that has never fired looks exactly like one that keeps passing.

    So this plants a replacement that cannot run, in a real file, beside a
    handful of real mutations, and requires that the sweep names it --- and that
    the real ones beside it come back clean. It writes nothing to the cache: a
    control must not teach the thing it is controlling.

    The third arm is the one that is easy to leave out and is half the claim.
    A gate with an exemption needs its exemption demonstrated too, or nobody can
    tell a rule that is deliberately narrow from one that is quietly broken: a
    planted TypeScript mutation that is a *type* error and nothing more must
    come back **clean**, and be counted among the soft ones.
    """
    print("--- control: a mutation that cannot compile must be reported by name")
    failures = 0
    for group, table, suffix, breakage in (
        (
            "rust",
            "scripts/mutate_rust.py",
            ".rs",
            # A `u8` bound to a string literal. Deliberately **not** a syntax
            # error: a parse failure would be caught by anything at all, and
            # what this gate exists for is the replacement that parses cleanly
            # and does not type-check --- which is what all three of the stale
            # mutations it was written for looked like.
            'let _: u8 = "not a number";',
        ),
        (
            "js",
            "scripts/mutate_frontend.py",
            ".ts",
            # A call to a name that does not exist. The Rust control plants a
            # type error because cargo refuses to build one; this one plants a
            # `ReferenceError` because vitest would run a type error happily,
            # and a control has to aim at the criterion its group is judged by.
            "tpdfTypeControlMissing();",
        ),
    ):
        base = BASE_OF[group]
        tree = Tree(base)
        real: "list[Item]" = []
        host: "Item | None" = None
        for mutation in load(table).MUTATIONS:
            if Path(mutation.path).suffix != suffix:
                continue
            if getattr(mutation, "only_on", None) not in (None, HERE):
                continue
            rel = relocate(base if group == "rust" else ROOT, group, mutation.path)
            if rel is None:
                continue
            text = tree.source(rel)
            if text.count(mutation.before) != 1:
                continue
            start = text.index(mutation.before)
            item = Item(table, mutation, rel, start, start + len(mutation.before))
            # The planted replacement is a *statement*, so it needs an anchor
            # that is one --- a `before` ending in a semicolon. Replacing an
            # expression fragment with `let ...;` would be a parse error, which
            # is a different failure from the one this control is about.
            if host is None and mutation.before.rstrip().endswith(";"):
                host = item
                continue
            if len(real) < 3:
                real.append(item)
            if host is not None and len(real) == 3:
                break
        if host is None or not real:
            print(f"[FAIL] {group}: found no usable subject, so this control proves nothing")
            failures += 1
            continue
        planted = Item(
            table,
            Broken("control: a replacement that cannot type-check", host.path, host.mutation.before, breakage),
            host.path,
            host.start,
            host.end,
        )
        subjects = [planted] + real
        checker = Cargo() if group == "rust" else Typescript()
        verified, broken, _, _soft, notes = sweep(
            f"{group}/control", checker, tree, subjects, set(), lambda line: print(line, flush=True)
        )
        for line in notes:
            print(line, flush=True)
        if planted.key in broken:
            print(f"[OK]   {group}: reported -- {broken[planted.key][:140]}")
        else:
            print(f"[FAIL] {group}: the planted mutation was NOT reported; this gate cannot fire")
            failures += 1
        alongside = [i for i in real if i.key in broken]
        if alongside:
            print(f"[FAIL] {group}: {len(alongside)} healthy mutation(s) reported too: {[i.name for i in alongside]}")
            failures += 1
        elif len(verified) == len(real):
            print(f"[OK]   {group}: the {len(real)} healthy mutation(s) beside it came back clean")
        else:
            print(f"[FAIL] {group}: {len(verified)} of {len(real)} healthy mutations verified")
            failures += 1
        tree.restore()

    # --- and the exemption, which is the other half of the claim -----------
    print("--- control: a front-end mutation that only breaks the TYPES must pass")
    tree = Tree(ROOT)
    host = None
    for mutation in load("scripts/mutate_frontend.py").MUTATIONS:
        if Path(mutation.path).suffix != ".ts":
            continue
        text = tree.source(mutation.path)
        if text.count(mutation.before) == 1 and mutation.before.rstrip().endswith(";"):
            start = text.index(mutation.before)
            host = Item("control", mutation, mutation.path, start, start + len(mutation.before))
            break
    if host is None:
        print("[FAIL] soft: no usable subject, so this control proves nothing")
        failures += 1
    else:
        planted = Item(
            "control",
            # Resolves, assigns, and is wrong. esbuild strips the annotation and
            # runs it, which is exactly the population this gate must not fail.
            Broken("control: a type error that still runs", host.path, host.mutation.before,
                   "const _tpdfSoftControl: number = 'not a number';"),
            host.path,
            host.start,
            host.end,
        )
        verified, broken, _, softly, notes = sweep(
            "js/soft", Typescript(), tree, [planted], set(), lambda line: print(line, flush=True)
        )
        for line in notes:
            print(line, flush=True)
        if planted.key in broken:
            print(f"[FAIL] soft: reported as broken -- {broken[planted.key]}; the exemption is not working")
            failures += 1
        elif planted.key not in softly:
            print("[FAIL] soft: passed without being counted as a soft diagnostic, "
                  "so this control cannot tell an exemption from a checker that saw nothing")
            failures += 1
        else:
            print("[OK]   soft: passed, and counted -- the exemption is deliberate and visible")
        tree.restore()

    print()
    if failures:
        print(f"[FAIL] the control failed {failures} way(s)")
        return 1
    print(
        "[OK] the control fires on a replacement that cannot run, stays quiet on healthy "
        "ones, and lets a type-only front-end break through while counting it"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
