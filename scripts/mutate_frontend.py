#!/usr/bin/env python3
"""Breaks the front-end selection code on purpose, one edit at a time.

A test that has only ever passed looks exactly like one that cannot fail, so
each mutation below names the test it is *expected* to turn red, and the run
reports a mutation that nothing caught as a defect in the suite.

Two properties this harness has because `AGENTS.md` records what their absence
costs:

**It cross-checks.** Every run derives the failure count two ways -- by counting
the reporter's per-test `x` lines and by reading its summary line -- and a
disagreement is reported as a broken run rather than as either answer. The trap
entry is about a harness that printed SURVIVED while its own summary, four lines
below in the same buffer, said a check had failed.

**A run that produced no summary is not a pass.** A crash, a timeout and a
syntax error from a bad mutation all produce no failing-test lines, which is
exactly what a surviving mutation looks like.

Usage:
    scripts/mutate_frontend.py            # every mutation
    scripts/mutate_frontend.py --list
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Mutation:
    """One edit, and the test whose job it is to notice."""

    name: str
    path: str
    before: str
    after: str
    expect: str


MUTATIONS = [
    Mutation(
        "word: do not walk left from the clicked character",
        "src/lib/text.ts",
        "  while (from > 0 && classOf(codes[from - 1] ?? 0) === kind) from--;",
        "",
        "selects the run of letters a character sits in",
    ),
    Mutation(
        "word: do not walk right from the clicked character",
        "src/lib/text.ts",
        "  while (to < codes.length && classOf(codes[to] ?? 0) === kind) to++;",
        "",
        "selects the run of letters a character sits in",
    ),
    Mutation(
        "word: treat every character as a word character",
        "src/lib/text.ts",
        '  if (WORD_CHARACTER.test(char)) return "word";',
        '  if (char) return "word";',
        "selects the second word, not the whole line",
    ),
    Mutation(
        # Predicted against the hyphen test first, and it survived: a lone mark
        # comes out the same whether it is returned directly or walked outwards
        # from, since its neighbours are a different class either way. Only a
        # *run* of marks distinguishes the two, which nothing covered.
        "word: let a punctuation mark join the run beside it",
        "src/lib/text.ts",
        '  if (kind === "mark") return { from: at, to: at + 1 };',
        "",
        "selects one mark of a run of punctuation, not the run",
    ),
    Mutation(
        "word: do not clamp an index past the last character",
        "src/lib/text.ts",
        "  const at = Math.min(Math.max(index, 0), codes.length - 1);",
        "  const at = index;",
        "does not run past the ends of the page",
    ),
    Mutation(
        "word: drop combining marks from the word class",
        "src/lib/text.ts",
        "const WORD_CHARACTER = /[\\p{L}\\p{N}\\p{M}_]/u;",
        "const WORD_CHARACTER = /[\\p{L}\\p{N}_]/u;",
        "treats a combining mark as part of the word",
    ),
    Mutation(
        "line: miss the first character of a line",
        "src/lib/text.ts",
        "    if (at >= line.from && at < line.to) return line;",
        "    if (at > line.from && at < line.to) return line;",
        "includes the first character of a line",
    ),
    Mutation(
        "line: return the word instead of the line",
        "src/lib/text.ts",
        "  for (const line of linesOf(text)) {",
        "  for (const line of [wordAt(text, at)]) {",
        "selects the whole line, not the word under the pointer",
    ),
    Mutation(
        "clicks: measure the slop on x only",
        "src/lib/clicks.ts",
        "      Math.abs(x - this.x) <= MULTI_CLICK_SLOP_PX && Math.abs(y - this.y) <= MULTI_CLICK_SLOP_PX;",
        "      Math.abs(x - this.x) <= MULTI_CLICK_SLOP_PX;",
        "measures the slop on both axes",
    ),
    Mutation(
        "clicks: exclude the deadline instead of including it",
        "src/lib/clicks.ts",
        "    const soon = nowMs - this.atMs <= MULTI_CLICK_MS;",
        "    const soon = nowMs - this.atMs < MULTI_CLICK_MS;",
        "counts a click at exactly the deadline as part of the run",
    ),
    Mutation(
        "clicks: count upwards forever instead of wrapping",
        "src/lib/clicks.ts",
        "    this.count = near && soon ? (this.count % 3) + 1 : 1;",
        "    this.count = near && soon ? this.count + 1 : 1;",
        "wraps back to a single click after the third",
    ),
    Mutation(
        "clicks: keep the run's first position instead of the last",
        "src/lib/clicks.ts",
        "    this.x = x;\n    this.y = y;",
        "    if (this.count === 1) {\n      this.x = x;\n      this.y = y;\n    }",
        "measures the distance from the last click, not from where the run began",
    ),
    Mutation(
        "clicks: measure the gap from the run's first click",
        "src/lib/clicks.ts",
        "    this.atMs = nowMs;",
        "    if (this.count === 1) this.atMs = nowMs;",
        "measures the gap from the last click, not from the first",
    ),
    Mutation(
        # Predicted against the upright test first, which was simply wrong:
        # this replaces the *sideways* branch, and the sideways test is what
        # went red. Being wrong about which test notices is a result, not a
        # nuisance -- the pair below now covers both branches, where one
        # mutation covered one branch and claimed the other.
        "caret: on a turned page, never place it after the character",
        "src/lib/text.ts",
        "  return sideways\n    ? y > (quad.top + quad.bottom) / 2",
        "  return sideways\n    ? false",
        "splits on the reading axis when the page is turned",
    ),
    Mutation(
        "caret: on an upright page, never place it after the character",
        "src/lib/text.ts",
        "    : x > (quad.left + quad.right) / 2",
        "    : false",
        "puts the caret after a character the pointer is past the middle of",
    ),
    Mutation(
        "caret: fall back to the last character rather than the first",
        "src/lib/text.ts",
        "  if (best < 0) return 0;",
        "  if (best < 0) return text.codes.length;",
        "puts the caret at the start of a page that places no characters",
    ),
    Mutation(
        "nearest: ignore the weight, so a click lands a line away",
        "src/lib/text.ts",
        "    const distance = along * along + (across * ACROSS_LINE_WEIGHT) ** 2;",
        "    const distance = along * along + across ** 2;",
        "weights distance across the lines, not along them",
    ),
    Mutation(
        "nearest: count a character PDFium gave no box",
        "src/lib/text.ts",
        "    if (!isPlaced(quad)) continue;\n\n    const dx = Math.max(quad.left - x, 0, x - quad.right);",
        "    const dx = Math.max(quad.left - x, 0, x - quad.right);",
        "has no character to find on a page that places none",
    ),
]

FAILED_TEST = re.compile(r"^\s*(?:x|×)\s+(.*?)(?:\s+\d+ms)?$", re.M)
SUMMARY = re.compile(r"^\s*Tests\s+(?:(\d+) failed)?.*?(\d+) passed", re.M)


def run_tests() -> tuple[set[str], int | None, str]:
    """Runs the suite, returning the failed test names, the summary's count and the log."""
    done = subprocess.run(
        ["npx", "vitest", "run", "src/lib/text.test.ts", "src/lib/clicks.test.ts"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=300,
    )
    out = done.stdout + done.stderr
    # Split on the marker and take the rest of the line -- never a fixed column.
    names = {m.strip() for m in FAILED_TEST.findall(out) if m.strip()}
    summary = SUMMARY.search(out)
    counted = int(summary.group(1) or 0) if summary else None
    return names, counted, out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.list:
        for mutation in MUTATIONS:
            print(f"{mutation.name}  ->  expects: {mutation.expect}")
        return 0

    print(f"--- control: the suite must be green before anything is broken", flush=True)
    names, counted, out = run_tests()
    if counted is None:
        print("[FAIL] the control run produced no summary line, so nothing below is readable")
        print(out[-2000:])
        return 1
    if counted != 0 or names:
        print(f"[FAIL] the control run is not green: {counted} failed, {sorted(names)}")
        return 1
    print("[OK]   control green", flush=True)

    problems = 0
    with tempfile.TemporaryDirectory(prefix="tpdf-mutate-") as scratch:
        for mutation in MUTATIONS:
            target = ROOT / mutation.path
            # Copied aside and copied *back*, never moved: a move replaces the
            # file the tooling may already be watching, and AGENTS.md records a
            # restore-by-move that left the mutated build in place.
            backup = Path(scratch) / f"{len(list(Path(scratch).iterdir()))}.bak"
            shutil.copy2(target, backup)
            try:
                source = target.read_text()
                if source.count(mutation.before) != 1:
                    print(
                        f"[FAIL] {mutation.name}: its anchor appears "
                        f"{source.count(mutation.before)} times, so the mutation is not the "
                        "one described"
                    )
                    problems += 1
                    continue
                target.write_text(source.replace(mutation.before, mutation.after))
                names, counted, out = run_tests()
            finally:
                shutil.copy2(backup, target)

            if counted is None:
                print(f"[FAIL] {mutation.name}: no summary line -- the run did not finish")
                problems += 1
                continue
            # The cross-check: the reporter's per-test lines and its own count
            # must agree, or one of the two has stopped describing the run.
            if len(names) != counted:
                print(
                    f"[FAIL] {mutation.name}: {len(names)} failing test lines but the summary "
                    f"says {counted} -- this harness cannot read its own output"
                )
                problems += 1
                continue
            if not names:
                print(f"[FAIL] {mutation.name}: SURVIVED -- no test noticed")
                problems += 1
                continue
            hit = any(mutation.expect in name for name in names)
            mark = "[OK]  " if hit else "[FAIL]"
            print(
                f"{mark} {mutation.name}: {counted} red"
                + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
            )
            if not hit:
                print(f"         red instead: {sorted(names)}")
                problems += 1

    print()
    print(
        f"[OK] all {len(MUTATIONS)} mutations caught by the test named for them"
        if problems == 0
        else f"[FAIL] {problems} of {len(MUTATIONS)} mutations were not caught as described"
    )
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
