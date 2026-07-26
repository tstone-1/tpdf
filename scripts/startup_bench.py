#!/usr/bin/env python3
"""Runs the startup timeline (spike 0.2) repeatedly and aggregates it.

Each launch is a separate process -- that is the whole point, since the thing
being measured is what a launch costs -- so aggregation has to happen out here.

Usage:
    scripts/startup_bench.py <app-binary> <file.pdf> [--runs N] [--purge]

The first run is reported separately from the rest. It is the closest thing to a
cold start available without evicting the page cache, and mixing it into a
median would hide exactly the number the 300 ms target is about. Pass --purge
for a genuinely cold cache; it needs sudo and is slow, so it is opt-in.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys


def run_once(binary: str, pdf: str, timeout: float) -> "list[tuple[str, float]] | None":
    """Launches the app once and returns its milestone list, or None on failure."""
    env = dict(os.environ, TPDF_STARTUP=pdf)
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


def purge_page_cache() -> bool:
    """Evicts the page cache so the next launch is genuinely cold."""
    result = subprocess.run(["sudo", "-n", "purge"], capture_output=True, text=True)
    if result.returncode != 0:
        print("[WARN] purge failed (needs passwordless sudo); run is warm", file=sys.stderr)
        return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("pdf")
    parser.add_argument("--runs", type=int, default=8)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument(
        "--purge",
        action="store_true",
        help="evict the page cache before every run (needs sudo)",
    )
    args = parser.parse_args()

    runs: "list[list[tuple[str, float]]]" = []
    for index in range(args.runs):
        if args.purge:
            purge_page_cache()
        marks = run_once(args.binary, args.pdf, args.timeout)
        if marks is None:
            return 1
        runs.append(marks)
        print(f"  run {index + 1}/{args.runs}: {marks[-1][1]:.1f} ms", file=sys.stderr)

    # Milestone names in timeline order, taken from the first run. Ordering is
    # by time within a run, so a later run that reorders two adjacent marks does
    # not reshuffle the table.
    names = [name for name, _ in runs[0]]

    first = dict(runs[0])
    rest = runs[1:]

    print()
    # Under --purge every run is cold, so there is no warm steady state to
    # contrast run 1 against -- calling the median "warm" there would invert
    # what the table says.
    first_label, rest_label = ("cold 1", "cold med") if args.purge else ("run 1", "warm med")
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

    print()
    totals = [run[-1][1] for run in rest]
    print(
        f"first page presented: {first_label} {runs[0][-1][1]:.1f} ms, "
        f"{rest_label} {statistics.median(totals):.1f} ms over {len(totals)} runs"
    )
    print("target is 300 ms warm (see docs/PLAN.md section 4).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
