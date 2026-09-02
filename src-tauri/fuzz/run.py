#!/usr/bin/env python3
"""Builds and runs one fuzz target, or all of them.

**This file is the invocation, not a description of one.** Three things have to
be right on every run and none of them is guessable from `cargo fuzz --help`:
the toolchain, a linker flag without which the build does not link at all, and a
per-target input bound. Keeping them here rather than in a document is the same
argument `scripts/gates.py` makes about the gate list -- a command copied into
prose loses a flag and then measures something weaker than the real one.

    src-tauri/fuzz/run.py --list
    src-tauri/fuzz/run.py --target ber_definite --seconds 3600
    src-tauri/fuzz/run.py --all --seconds 3600 --background
    src-tauri/fuzz/run.py --build-only

# The three things

**`+nightly`.** `cargo fuzz` refuses to run on anything else: the coverage
instrumentation it asks for goes in through compiler flags that are not stable.
It is passed per command and never by setting `RUSTUP_TOOLCHAIN`, which would
override `rust-toolchain.toml` silently for everything else run in the same
shell -- `scripts/check_toolchain.py` exists because that override is invisible,
and this must not be the thing that trips it. `rust-toolchain.toml`'s pin is
untouched: nothing here builds the application.

**`--sanitizer=none`, and `-Clink-arg=-Wl,-undefined,dynamic_lookup` with it.**
Both follow from one fact about the dependency graph, and neither is a
preference.

`tpdf` declares `crate-type = ["staticlib", "cdylib", "rlib"]` and
`pdfium-render` declares `["lib", "staticlib", "cdylib"]`. Cargo builds every
declared crate type whenever it builds that lib, even though a dependent needs
only the rlib -- so building any fuzz target *links two shared libraries* that
have nothing to do with fuzzing, and both of them are instrumented.

That breaks in two different ways depending on the sanitizer, and one of the
breakages looks like success:

* **Sanitizer on.** The link fails with `ld: initializer pointer has no target
  in ... libtauri_utils....rlib`, reported against `tpdf (lib)` -- Apple's
  current linker refusing a static initializer the sanitizer emits into the
  `cdylib`. `-Wl,-no_fixup_chains` does not help. `-Wl,-ld_classic` makes the
  link succeed, and **the binaries it produces cannot run at all**: every one
  dies inside libFuzzer's own first `Printf`, before a single input, with
  `SEGV on unknown address 0x68 ... in flockfile`. Nothing is printed, so there
  is no missing banner to notice, and `cargo fuzz build` reports success either
  way. Measured, not reasoned about -- the mutation proof that was supposed to
  run against those binaries is what found it.
* **Sanitizer off.** The instrumentation is still emitted, but the runtime that
  defines its symbols is linked only into the final executable, so the
  intermediate `cdylib`s fail with `Undefined symbols ...
  _sancov.module_ctor_8bit_counters`. `-Wl,-undefined,dynamic_lookup` defers
  those, which is correct here rather than a suppression: nothing ever loads
  those two shared libraries. The executable resolves the same symbols normally
  out of libFuzzer.

The cost of running without the sanitizer is small and stateable: every module
under test is safe Rust with no `unsafe` in it at all, so what it adds over
Rust's own bounds checks is close to nothing. The checks that do matter for a
parser survive, because they come from `-Cdebug-assertions`, which `cargo fuzz`
passes regardless: `debug_assert!` -- which is where `ber.rs` compares its
measuring walk against its writing one -- and integer overflow, which is the
arithmetic a length field drives.

**`-max_len`.** Per target, because the shapes that matter differ: a BER blob is
interesting at a few kilobytes and a PDF is not interesting at all until it has
a header, a cross-reference table and a page tree. A bound that is too large
buys nothing and costs executions per second, which is the only currency a
fuzzer has.

# Why the runs exec the binary rather than `cargo fuzz run`

`cargo fuzz run` builds before it runs, and nine of them started together do not
build nine times -- they **queue on one build-directory lock**, so eight sit in
`Blocking waiting for file lock on build directory` printing nothing while the
first one works. That reads exactly like nine fuzzers having started and found
nothing to say, which is the failure shape this repository is least able to
notice.

Building once and then executing the binaries removes the lock entirely, and it
buys a second property that matters more here than tidiness: a run is then
**immune to the source changing under it**. An hour-long run whose subject is
rebuilt halfway through is measuring two different programs and reporting one
number.

The command line is the one `cargo fuzz run` would have produced: the artifact
prefix so a finding is written where `cargo fuzz tmin` looks for it, and the
corpus directory as the sole positional argument.

# What a run reports

`-print_final_stats=1`, so the last lines carry `stat::number_of_executions`.
That number is the answer to "was this actually fuzzed", and a run reported
without it is a run nobody can weigh -- a target that executed 900 times in an
hour has found nothing because it never ran, not because there is nothing there.
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

FUZZ = Path(__file__).resolve().parent
SRC_TAURI = FUZZ.parent
LOGS = FUZZ / "logs"

# The target triple `cargo fuzz` builds for, which is where the binaries land.
# Read from the toolchain rather than written down, because a hard-coded triple
# is right on exactly one machine and silently wrong on the next.
TRIPLE = (
    subprocess.run(
        ["rustc", "-vV"], capture_output=True, text=True, check=False
    ).stdout.partition("host: ")[2].split("\n")[0].strip()
    or "aarch64-apple-darwin"
)

# What `cargo fuzz` is told about instrumentation. `none` is not a default and
# not a convenience -- see the header. It applies to the build and to the run,
# and the two must agree or `cargo fuzz run` rebuilds under the other setting.
SANITIZER = "none"

# Deferred resolution of the sanitizer-coverage symbols in the two shared
# libraries cargo builds and nothing loads. See the header: without it the build
# does not link, and it is a fact about `crate-type` in someone else's manifest
# rather than about anything here.
RUSTFLAGS = "-Clink-arg=-Wl,-undefined,dynamic_lookup"

# Each target with the input bound it is run at, and why that bound.
TARGETS = {
    "ber_definite": (
        4096,
        "structure, not size: nesting and length forms are what the walk decides on",
    ),
    "ber_certificate": (
        8192,
        "a real CMS blob with one certificate in it is a few kilobytes",
    ),
    "lopdf_load": (65536, "a document needs a header, an xref and a page tree"),
    "annots_scan": (65536, "as lopdf_load: the subject is a whole document"),
    "links_scan": (65536, "as lopdf_load"),
    "docinfo_scan": (65536, "as lopdf_load"),
    "encoding_scan": (65536, "as lopdf_load"),
    "annots_text": (4096, "a PDF text string, which is a field rather than a file"),
    "save_rewrite_update": (
        65536,
        "a document plus the bytes the plan is built from",
    ),
}

# A single input may not run longer than this. libFuzzer's default is twenty
# minutes, which is not a bound at all on an hour-long run: one pathological
# document would spend a third of the budget and the run would report it as
# nothing having happened.
TIMEOUT_S = 25

# libFuzzer stops the run when process RSS passes this. Left at the tool's own
# default deliberately -- it is the value a person reading a report will assume,
# and raising it everywhere would make an out-of-memory finding here mean
# something different from one anywhere else.
RSS_LIMIT_MB = 2048

# The two targets that need more, with the measurement rather than a shrug.
#
# `lopdf_load` decompresses every object and every page's content, each bounded
# at 64 MiB, and `save_rewrite_update` serialises a whole document per input --
# so both allocate large buffers where the other targets allocate small ones.
# Process RSS then climbs and does not come back down: `lopdf_load` reached
# **982 MB after 215,224 executions** on a fifty-document corpus where
# `annots_scan` sat at **74 MB after 190,483**, and `save_rewrite_update` met
# the 2 GB ceiling at 151,337.
#
# It is **not a leak per call**, and that was measured rather than assumed: four
# hundred executions of the *same* document peak at 37 MB, flat, and a control
# whose inputs are too short to parse at all is flat at 33 MB over 5,559,622
# executions. No single input reproduces it either -- the heaviest costs 108 MB.
# What is left is the allocator holding freed large blocks, which is a fact
# about the run rather than about the code, so the answer is headroom rather
# than a suppression.
#
# Stated per target so that an out-of-memory in any *other* target still means
# what it means everywhere else.
RSS_LIMIT_OVERRIDE = {"lopdf_load": 6144, "save_rewrite_update": 6144}

# The targets that cannot finish a run without forking, and why.
#
# libFuzzer stops at the **first** finding. That is the right default: a finding
# is the point, and stopping puts it in front of you. It is the wrong default
# for a target that reaches a defect **we cannot fix**, because the run then
# ends at the same place every time and everything behind it is unreachable.
#
# Every target whose subject is a **whole document** is here, and that is the
# rule rather than a list of the ones observed to stop. Residual risk 21 records
# that the abort is on the load path and that all five of its readers reach it,
# "verified by feeding one file to each" --- so a target left out would be one
# waiting to lose its run to a defect already known to be reachable from it.
# `annots_scan` is what made the rule rather than the list: it had never filed an
# artifact, and on 2026-09-02 it stopped at 8,051,057 executions, 14 minutes into
# a one-hour run, on the same abort at a fifth magnitude.
#
# `save_rewrite_update` is deliberately **not** here even though it loads a
# document too. Its findings are *ours*, and stopping on one is the point; it ran
# 13,638,855 executions in a clean hour on 2026-09-02 without reaching risk 21.
# An artifact of that shape appearing there is what would move it.
#
# All of these reach `docs/THREAT-MODEL.md` residual risk 21 --- `lopdf`'s
# cross-reference parser multiplies out the field widths a document declares in
# `/W` and asks for the product, which aborts through `handle_alloc_error` where
# no `catch_unwind` and no guard in tpdf's own code can sit. Measured 2026-09-02
# on `lopdf_load`: **80,269 executions in 21 s** before it stopped, against
# **380,803 in 96 s** forked, on the same corpus and the same binary. It had been
# stopping like that since 2026-09-01, and nothing said so, because no campaign
# had been run to completion since.
#
# In fork mode each input runs in a child, so an abort costs a child rather than
# the run. Read what that changes under `verdicts`, below --- it is not free.
MUST_FORK = {
    "lopdf_load": "reaches residual risk 21 within seconds and stops there",
    "encoding_scan": "the same abort, through its own entry point",
    "annots_scan": "the same abort, 14 minutes in, at a fifth magnitude",
    "links_scan": "a whole-document reader, so risk 21 is on its load path too",
    "docinfo_scan": "as links_scan",
}


def environment() -> "dict[str, str]":
    """The build environment: one toolchain, and the link flag the build needs.

    `+nightly` is passed on the command line, so `RUSTUP_TOOLCHAIN` must not
    also be set: two ways of choosing a toolchain is how one of them ends up
    silently winning, which is the failure `scripts/check_toolchain.py` exists
    for.
    """
    env = dict(os.environ)
    env.pop("RUSTUP_TOOLCHAIN", None)
    # Appended rather than assigned: `cargo fuzz` puts its own instrumentation
    # flags after whatever this process exports, so replacing would drop a
    # caller's.
    existing = env.get("RUSTFLAGS", "").strip()
    env["RUSTFLAGS"] = f"{existing} {RUSTFLAGS}".strip()
    # Without a sanitizer libFuzzer cannot symbolise a crash itself -- it says so
    # on every start, three `Failed to find function "__sanitizer_..."` lines --
    # so the panic's own backtrace is the only frame list a finding comes with.
    env.setdefault("RUST_BACKTRACE", "1")
    return env


def seed(target: "str | None") -> None:
    """Fills the corpus before the run, because an unseeded PDF target is inert."""
    command = [sys.executable, str(FUZZ / "seed.py")]
    if target:
        command += ["--target", target]
    subprocess.run(command, check=True)


def build(targets: "list[str]") -> int:
    command = ["cargo", "+nightly", "fuzz", "build", f"--sanitizer={SANITIZER}"]
    if len(targets) == 1:
        command.append(targets[0])
    print(f"[INFO] {' '.join(command)}")
    return subprocess.run(command, cwd=SRC_TAURI, env=environment()).returncode


def binary(target: str) -> Path:
    """Where `cargo fuzz build` put this target."""
    return FUZZ / "target" / TRIPLE / "release" / target


def artifacts_of(target: str) -> "set[str]":
    """The artifact filenames this target has on disk right now.

    **The verdict reads this rather than an exit code**, and that is the whole
    reason it exists. In fork mode libFuzzer answers 0 for a run that finished,
    findings and all, so an exit code stops meaning what it means everywhere
    else in this repository. A file appearing in `artifacts/<target>/` means the
    same thing in both modes.
    """
    directory = FUZZ / "artifacts" / target
    return {entry.name for entry in directory.iterdir()} if directory.is_dir() else set()


def run(
    target: str, seconds: int, background: bool, fork: bool
) -> "subprocess.Popen | int":
    length, _why = TARGETS[target]
    command = [
        str(binary(target)),
        f"-artifact_prefix={FUZZ / 'artifacts' / target}/",
        f"-max_len={length}",
        f"-max_total_time={seconds}",
        f"-timeout={TIMEOUT_S}",
        f"-rss_limit_mb={RSS_LIMIT_OVERRIDE.get(target, RSS_LIMIT_MB)}",
        "-print_final_stats=1",
    ]
    if fork or target in MUST_FORK:
        # `-fork=1` runs each input in a child; `-ignore_crashes=1` is what makes
        # the parent carry on rather than exiting with the child. Without the
        # second flag the first is nearly pointless here.
        command += ["-fork=1", "-ignore_crashes=1"]
    command.append(str(FUZZ / "corpus" / target))
    (FUZZ / "artifacts" / target).mkdir(parents=True, exist_ok=True)
    print(f"[INFO] {' '.join(command)}")
    if not background:
        return subprocess.run(command, cwd=SRC_TAURI, env=environment()).returncode

    LOGS.mkdir(exist_ok=True)
    log = LOGS / f"{target}.log"
    handle = log.open("wb")
    print(f"[INFO] {target}: output to {log}")
    return subprocess.Popen(
        command,
        cwd=SRC_TAURI,
        env=environment(),
        stdout=handle,
        stderr=subprocess.STDOUT,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", help="one target by name")
    parser.add_argument("--all", action="store_true", help="every target")
    parser.add_argument("--list", action="store_true", help="the targets and their bounds")
    parser.add_argument("--build-only", action="store_true", help="build, do not run")
    parser.add_argument(
        "--fork",
        action="store_true",
        help="run every target forked, so a finding does not end the run",
    )
    parser.add_argument(
        "--seconds", type=int, default=3600, help="wall time per target (default 3600)"
    )
    parser.add_argument(
        "--background",
        action="store_true",
        help="start every target at once, logging to fuzz/logs/<target>.log",
    )
    args = parser.parse_args()

    if args.list:
        for name, (length, why) in TARGETS.items():
            # `fork` is printed because it changes what a clean run *means*
            # --- see `verdicts` --- so a reader consulting this list must not
            # have to open `MUST_FORK` to find out which targets it applies to.
            mode = "fork " if name in MUST_FORK else "     "
            print(f"  {name:<22} {mode} -max_len={length:<6} {why}")
        return 0

    if shutil.which("cargo-fuzz") is None:
        print(
            "[FAIL] cargo-fuzz is not installed: cargo install cargo-fuzz --locked",
            file=sys.stderr,
        )
        return 2

    if args.target and args.target not in TARGETS:
        print(
            f"[FAIL] no target named {args.target}; known: {', '.join(TARGETS)}",
            file=sys.stderr,
        )
        return 2
    if not args.target and not args.all and not args.build_only:
        print("[FAIL] pass --target <name>, --all, or --build-only", file=sys.stderr)
        return 2

    chosen = [args.target] if args.target else list(TARGETS)
    seed(args.target)

    # Built once, here, for every target that is about to run. See the header:
    # nine `cargo fuzz run` invocations queue on one lock and print nothing.
    code = build(chosen)
    if code != 0:
        print(f"[FAIL] the build failed with exit {code}", file=sys.stderr)
        return code
    if args.build_only:
        print("[OK] built")
        return 0

    if not args.background:
        # **The same verdict as the background path, and it is a separate copy
        # of nothing.** This branch is the common invocation --- a bare
        # `--target <name>` lands here --- and when the artifact check was first
        # written it covered only the branch below, so the one people actually
        # run kept the exit code as its whole answer. `docs/TRAPS.md` has that
        # shape as *a check bound to one caller covers only that caller*; here
        # the interpreter said so, because `run` had grown an argument.
        worst = 0
        for name in chosen:
            before = artifacts_of(name)
            code = run(name, args.seconds, background=False, fork=args.fork)
            new_artifacts = sorted(artifacts_of(name) - before)
            for artifact in new_artifacts:
                print(f"[FAIL] new artifact: artifacts/{name}/{artifact}")
            if new_artifacts:
                print(
                    "       triage it before believing anything else: BUILD.md, "
                    "'Reading an artifact'"
                )
            worst = max(worst, code, 1 if new_artifacts else 0)
        return worst

    missing = [name for name in chosen if not binary(name).is_file()]
    if missing:
        # The build reported success, so this is not a build failure -- it is the
        # triple being wrong, and without the check the run would fail nine times
        # with a file-not-found that reads as the harness being broken.
        print(
            f"[FAIL] built, but no binary at {binary(missing[0])} "
            f"(triple {TRIPLE}); {len(missing)} target(s) affected",
            file=sys.stderr,
        )
        return 2

    # Snapshotted **before** anything starts, because the verdict below is a
    # difference and not a count: `artifacts/` already holds files from earlier
    # runs, and a run that finds nothing must not inherit them as its own.
    before = {name: artifacts_of(name) for name in chosen}
    started = [
        (name, run(name, args.seconds, background=True, fork=args.fork))
        for name in chosen
    ]
    print(f"[OK] {len(started)} target(s) started; they stop after {args.seconds}s each")
    failed = 0
    for name, process in started:
        code = process.wait()
        new_artifacts = sorted(artifacts_of(name) - before[name])
        # **Two observables, because neither covers the other.** A non-zero exit
        # is a build error, a corpus that will not read, a bad flag -- and, in
        # the default mode, a finding. A new artifact is a finding in *either*
        # mode, and in fork mode it is the only sign of one: libFuzzer answers 0
        # for a forked run that completed, however many children it buried. A
        # harness that kept reading the exit code alone would report a target
        # aborting every few minutes as permanently green, which is the failure
        # shape `docs/TRAPS.md` collects from a dozen directions.
        bad = code != 0 or bool(new_artifacts)
        verdict = "[FAIL]" if bad else "[OK]"
        forked = " forked" if (args.fork or name in MUST_FORK) else ""
        print(f"{verdict} {name}{forked}: exit {code}  (log: {LOGS / f'{name}.log'})")
        for artifact in new_artifacts:
            print(f"       new artifact: artifacts/{name}/{artifact}")
        if new_artifacts:
            print("       triage it before believing anything else: BUILD.md, "
                  "'Reading an artifact'")
        failed += bad
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
