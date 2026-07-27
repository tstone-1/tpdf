#!/usr/bin/env python3
"""Runs the startup timeline (spike 0.2) repeatedly and aggregates it.

Each launch is a separate process -- that is the whole point, since the thing
being measured is what a launch costs -- so aggregation has to happen out here.

Usage:
    scripts/startup_bench.py <app-binary> <file.pdf> [--runs N] [--purge]
    scripts/startup_bench.py <app-binary> <file.pdf> \
        --variant baseline --variant "eager:TPDF_EAGER_GEOMETRY=1" ...

Lazy page geometry became the default on 2026-07-27, so the variant that used to
be the improvement is now the baseline and `TPDF_EAGER_GEOMETRY` restores the
86 ms whole-document walk it replaced.

The first run is reported separately from the rest. It is the closest thing to a
cold start available without evicting the page cache, and mixing it into a
median would hide exactly the number the 300 ms target is about. Pass --purge
for a genuinely cold cache; it needs sudo and is slow, so it is opt-in.

With more than one --variant, the variants are run interleaved -- one launch of
each per round, in order, repeated -- rather than as consecutive blocks. Wall
clock on this machine drifts several percent over minutes, which is larger than
most of the differences worth finding, so a block layout would attribute drift
to whichever variant ran later. Comparisons are made pairwise within a round.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys

# One run of one variant: its milestone list, or None if the launch failed.
Marks = "list[tuple[str, float]] | None"


def parse_variant(spec: str) -> "tuple[str, dict[str, str]]":
    """Parses `name` or `name:KEY=VAL,KEY=VAL` into a name and its environment."""
    name, _, assignments = spec.partition(":")
    env: "dict[str, str]" = {}
    for assignment in assignments.split(",") if assignments else []:
        key, _, value = assignment.partition("=")
        if not key:
            continue
        env[key] = value
    return name, env


def run_once(binary: str, pdf: str, extra_env: "dict[str, str]", timeout: float) -> Marks:
    """Launches the app once and returns its milestone list, or None on failure."""
    env = dict(os.environ, TPDF_STARTUP=pdf, **extra_env)
    try:
        completed = subprocess.run(
            [binary],
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        print("[FAIL] run timed out", file=sys.stderr)
        return None

    for line in completed.stdout.splitlines():
        if line.startswith("TIMELINE-JSON "):
            payload = json.loads(line[len("TIMELINE-JSON ") :])
            return [(name, float(at)) for name, at in payload["marks"]]

    print("[FAIL] no timeline in output", file=sys.stderr)
    print(completed.stdout[-2000:], file=sys.stderr)
    print(completed.stderr[-2000:], file=sys.stderr)
    return None


def hold_display_awake() -> "subprocess.Popen[bytes] | None":
    """Keeps the display awake and on for as long as this process lives.

    WebKit suspends a page whose window is not visible, and a dark display makes
    every window invisible. Both `requestAnimationFrame` and `setTimeout` stop,
    so the run cannot even time itself out -- it simply stops after the last
    milestone before presentation. An unattended benchmark that outlives the
    idle timer therefore hangs rather than failing.

    `-u` as well as `-d`: `-d` only prevents the display going *idle*, and a
    display that is already off stays off. `-u` declares user activity, which
    turns it back on. The assertion is released when this process exits, so it
    does not keep the machine up afterwards.
    """
    if sys.platform != "darwin":
        return None
    try:
        return subprocess.Popen(
            ["caffeinate", "-du", "-w", str(os.getpid())],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        print("[WARN] caffeinate not available; a sleeping display will hang runs", file=sys.stderr)
        return None


def screen_is_locked() -> bool:
    """Whether the login session is locked, which makes every window invisible.

    WebKit suspends a page whose window is not visible, and behind a lock screen
    none of them are. The run then stops dead at the last milestone before
    presentation and cannot even report it, because the timer that would have
    reported it is suspended too. Worth one cheap check up front rather than
    thirty seconds per launch of looking like a slow machine.
    """
    if sys.platform != "darwin":
        return False
    result = subprocess.run(
        ["ioreg", "-n", "Root", "-d1", "-a"], capture_output=True, text=True
    )
    marker = "<key>CGSSessionScreenIsLocked</key>"
    index = result.stdout.find(marker)
    return index >= 0 and "<true/>" in result.stdout[index : index + 120]


def purge_page_cache() -> bool:
    """Evicts the page cache so the next launch is genuinely cold."""
    result = subprocess.run(["sudo", "-n", "purge"], capture_output=True, text=True)
    if result.returncode != 0:
        print("[WARN] purge failed (needs passwordless sudo); run is warm", file=sys.stderr)
        return False
    return True


def print_timeline(runs: "list[list[tuple[str, float]]]", purged: bool) -> None:
    """Prints the milestone table for one variant's runs."""
    # Milestone names in timeline order, taken from the first run. Ordering is
    # by time within a run, so a later run that reorders two adjacent marks does
    # not reshuffle the table.
    names = [name for name, _ in runs[0]]
    first = dict(runs[0])
    rest = runs[1:] or runs

    # Under --purge every run is cold, so there is no warm steady state to
    # contrast run 1 against -- calling the median "warm" there would invert
    # what the table says.
    first_label, rest_label = ("cold 1", "cold med") if purged else ("run 1", "warm med")
    header = (
        f"{'milestone':<30} {first_label:>9} {rest_label:>9} "
        f"{'min':>9} {'max':>9} {'delta':>9}"
    )
    print(header)
    print("-" * len(header))

    previous_median = 0.0
    for name in names:
        values = [dict(run).get(name) for run in rest]
        values = [v for v in values if v is not None]
        if not values:
            continue
        median = statistics.median(values)
        print(
            f"{name:<30} {first.get(name, float('nan')):>9.1f} {median:>9.1f} "
            f"{min(values):>9.1f} {max(values):>9.1f} {median - previous_median:>9.1f}"
        )
        previous_median = median


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--runs", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument(
        "--variant",
        action="append",
        default=[],
        metavar="NAME[:KEY=VAL,...]",
        help="an environment to run interleaved with the others; repeatable",
    )
    parser.add_argument(
        "--purge",
        action="store_true",
        help="evict the page cache before every run (needs sudo)",
    )
    args = parser.parse_args()

    variants = [parse_variant(spec) for spec in args.variant] or [("baseline", {})]

    if screen_is_locked():
        print(
            "[FAIL] the screen is locked, so every window is occluded and WebKit\n"
            "       suspends the page. No frame is ever presented and the run\n"
            "       cannot time itself out. Unlock and re-run.",
            file=sys.stderr,
        )
        return 1

    hold_display_awake()

    # runs[variant name] is that variant's launches, in round order, so round i
    # of every variant sits at index i and pairwise comparison is by index.
    runs: "dict[str, list[list[tuple[str, float]]]]" = {name: [] for name, _ in variants}

    for index in range(args.runs):
        for name, env in variants:
            if args.purge:
                purge_page_cache()
            marks = run_once(args.binary, args.pdf, env, args.timeout)
            if marks is None:
                print(f"[FAIL] variant {name}, round {index + 1}", file=sys.stderr)
                return 1
            runs[name].append(marks)
            print(
                f"  round {index + 1}/{args.runs} {name:<12} {marks[-1][1]:.1f} ms",
                file=sys.stderr,
            )

    for name, _ in variants:
        print()
        print(f"=== {name}")
        print()
        print_timeline(runs[name], args.purge)

    label = "cold" if args.purge else "warm"
    # The first run of a freshly built bundle pays ~300 ms of one-time code
    # signature validation (PLAN section 4), so it is excluded from the medians
    # whenever there is more than one round to fall back on.
    print()
    print(f"=== last milestone, median of rounds 2..{args.runs} ({label})")
    print()
    baseline_name = variants[0][0]
    baseline_totals = [run[-1][1] for run in runs[baseline_name][1:]] or [
        run[-1][1] for run in runs[baseline_name]
    ]
    baseline_median = statistics.median(baseline_totals)

    print(f"{'variant':<14} {'median':>9} {'min':>9} {'max':>9} {'vs base':>9} {'milestone'}")
    for name, _ in variants:
        totals = [run[-1][1] for run in runs[name][1:]] or [run[-1][1] for run in runs[name]]
        median = statistics.median(totals)
        print(
            f"{name:<14} {median:>9.1f} {min(totals):>9.1f} {max(totals):>9.1f} "
            f"{median - baseline_median:>+9.1f} {runs[name][0][-1][0]}"
        )

    if len(variants) > 1:
        print()
        print(f"=== pairwise against {baseline_name}, within each round")
        print()
        print(f"{'variant':<14} {'median delta':>13} {'min':>9} {'max':>9}")
        for name, _ in variants[1:]:
            pairs = [
                runs[name][i][-1][1] - runs[baseline_name][i][-1][1]
                for i in range(1, min(len(runs[name]), len(runs[baseline_name])))
            ]
            if not pairs:
                continue
            print(
                f"{name:<14} {statistics.median(pairs):>+13.1f} "
                f"{min(pairs):>+9.1f} {max(pairs):>+9.1f}"
            )

    print()
    print("target is 300 ms warm (see docs/PLAN.md section 4).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
