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
        "argument: run a value-taking command with no value",
        "src/lib/commands.ts",
        "      if (argument === undefined) return false;",
        "      if (argument === undefined) return true;",
        "refuses to run without one",
    ),
    Mutation(
        "argument: trust the caller's value instead of checking it",
        "src/lib/commands.ts",
        "      if (command.argument.problem(argument) !== null) return false;",
        "",
        "refuses a value its own check rejects",
    ),
    Mutation(
        "argument: silently ignore a value a command cannot take",
        "src/lib/commands.ts",
        "      // takes none has misunderstood something, and silently dropping it hides\n      // that until someone wonders why the value had no effect.\n      return false;",
        "      // takes none has misunderstood something, and silently dropping it hides\n      // that until someone wonders why the value had no effect.\n      argument = undefined;",
        "refuses a value for a command that takes none",
    ),
    Mutation(
        "argument: record a refused command as recent anyway",
        "src/lib/commands.ts",
        "      if (argument === undefined) return false;",
        "      if (argument === undefined) {\n        this.recent.unshift(id);\n        return false;\n      }",
        "does not record a refused command as recent",
    ),
    Mutation(
        "keys: stop checking Option in both directions",
        "src/lib/keys.ts",
        "  if (event.altKey !== (binding.alt ?? false)) return false;",
        "",
        "distinguishes a chord from the same chord with Option",
    ),
    Mutation(
        "keys: leave Option out of the rendered label",
        "src/lib/keys.ts",
        '${binding.alt ? "⌥" : ""}',
        "",
        "renders the modifiers the binding actually declares",
    ),
    Mutation(
        # Reachable only through `render`, not through `label`: no command holds
        # Shift and Option at once, which is how the order between them stayed
        # wrong -- and disagreeing with the comment beside it -- until a test
        # could name a binding that does not exist.
        "keys: put Shift before Option in a rendered label",
        "src/lib/keys.ts",
        '  return `${binding.alt ? "⌥" : ""}${binding.shift ? "⇧" : ""}',
        '  return `${binding.shift ? "⇧" : ""}${binding.alt ? "⌥" : ""}',
        "orders the modifiers as the platform does",
    ),
    Mutation(
        "search: compare only the first of the two options",
        "src/lib/search.ts",
        "  return a.matchCase === b.matchCase && a.wholeWord === b.wholeWord;",
        "  return a.matchCase === b.matchCase;",
        "is true only when both options agree",
    ),
    Mutation(
        "search: let the plain search match case",
        "src/lib/search.ts",
        "export const PLAIN_SEARCH: SearchOptions = { matchCase: false, wholeWord: false };",
        "export const PLAIN_SEARCH: SearchOptions = { matchCase: true, wholeWord: false };",
        "describes the plain search as neither option",
    ),
    Mutation(
        "recents: show only the basename, whatever collides",
        "src/lib/recents.ts",
        "        if ((depth[index] ?? 1) < (longest[index] ?? 1)) {\n          depth[index] = (depth[index] ?? 1) + 1;\n          grew = true;\n        }",
        "",
        "lengthens a colliding pair until it is distinct",
    ),
    Mutation(
        "recents: lengthen every label, not only the colliding ones",
        "src/lib/recents.ts",
        "      if (group.length < 2) continue;",
        "",
        "lengthens only the labels that collide",
    ),
    Mutation(
        "recents: give up after one extra directory",
        "src/lib/recents.ts",
        "    if (!grew) return labels;",
        "    return labels;",
        "keeps lengthening while a pair is still ambiguous",
    ),
    Mutation(
        "recents: rewrite every separator as a slash",
        "src/lib/recents.ts",
        '  const separator = path.includes("\\\\") && !path.includes("/") ? "\\\\" : "/";',
        '  const separator = "/";',
        "keeps the separator the path was written with",
    ),
    Mutation(
        "recents: number every recent command the same",
        "src/lib/recents.ts",
        "  return `${RECENT_PREFIX}${index}`;",
        "  return RECENT_PREFIX;",
        "shares the prefix the registry replaces by",
    ),
    Mutation(
        "registry: replace by substring rather than by prefix",
        "src/lib/commands.ts",
        "      if (this.commands[i]?.id.startsWith(prefix)) this.commands.splice(i, 1);",
        "      if (this.commands[i]?.id.includes(prefix)) this.commands.splice(i, 1);",
        "does not remove a command whose id merely contains the prefix",
    ),
    Mutation(
        "registry: keep the recents of commands that no longer exist",
        "src/lib/commands.ts",
        "      if (id?.startsWith(prefix)) this.recent.splice(i, 1);",
        "",
        "forgets that a replaced command was recent",
    ),
    Mutation(
        "registry: clear every recent when a group is replaced",
        "src/lib/commands.ts",
        "      if (id?.startsWith(prefix)) this.recent.splice(i, 1);",
        "      this.recent.splice(i, 1);",
        "leaves the recents of commands it did not replace",
    ),
    Mutation(
        "registry: append the group instead of replacing it",
        "src/lib/commands.ts",
        "      if (this.commands[i]?.id.startsWith(prefix)) this.commands.splice(i, 1);",
        "      void this.commands[i];",
        "swaps the group and leaves everything else alone",
    ),
    Mutation(
        "results: rebuild the whole list on every reply",
        "src/lib/results.ts",
        "    for (let i = this.built; i < matches.length && i < MAX_RESULT_ROWS; i++) {",
        "    this.list.replaceChildren();\n    this.rows.length = 0;\n    for (let i = 0; i < matches.length && i < MAX_RESULT_ROWS; i++) {",
        "appends only what has arrived since the last paint",
    ),
    Mutation(
        "results: append to the old rows when the query changes",
        "src/lib/results.ts",
        "    if (matches !== this.shown) {",
        "    if (false) {",
        "rebuilds when the match list is replaced",
    ),
    Mutation(
        "results: keep building rows past the cap",
        "src/lib/results.ts",
        "    this.built = Math.min(matches.length, MAX_RESULT_ROWS);",
        "    this.built = matches.length;",
        "stops building rows at the cap while the count stays exact",
    ),
    Mutation(
        "results: leave the previous row highlighted",
        "src/lib/results.ts",
        "    this.paintRow(this.currentIndex, false);",
        "",
        "moves the highlight to the current match and off the previous one",
    ),
    Mutation(
        "results: number rows from zero, as the code does rather than a reader",
        "src/lib/results.ts",
        "    page.textContent = String(match.page + 1);",
        "    page.textContent = String(match.page);",
        "numbers pages as a reader does, from one",
    ),
    Mutation(
        "results: write the status line on every reply",
        "src/lib/results.ts",
        "    if (text === this.said) return;",
        "",
        "writes the status line only when it changes",
    ),
    Mutation(
        "results: call an empty query and an empty result the same thing",
        "src/lib/results.ts",
        '  if (!query) return "Type in the find field to search.";',
        "",
        "tells an empty query apart from a search that has found nothing",
    ),
    Mutation(
        "results: apply the row cap without saying so",
        "src/lib/results.ts",
        '    total > MAX_RESULT_ROWS ? `, showing the first ${MAX_RESULT_ROWS}` : "";',
        '    "";',
        "states the row cap rather than applying it silently",
    ),
    Mutation(
        "results: do not say a scan is still running",
        "src/lib/results.ts",
        "  return running ? `${found}${capped}, still searching…` : `${found}${capped}`;",
        "  return `${found}${capped}`;",
        "says a scan is still running",
    ),
    Mutation(
        "cache: never evict, whatever the bound says",
        "src/lib/text.ts",
        "      if (this.chars <= TEXT_CACHE_CHARS || this.pages.size <= TEXT_CACHE_FLOOR) break;",
        "      break;",
        "drops pages once the bound is passed",
    ),
    Mutation(
        "cache: do not count a peek as a use",
        "src/lib/text.ts",
        "    if (text !== undefined) this.touch(page);",
        "",
        "drops the least recently used page, not the oldest arrival",
    ),
    Mutation(
        "cache: do not count a cache hit in load as a use",
        "src/lib/text.ts",
        "    if (cached) {\n      this.touch(page);",
        "    if (cached) {",
        "counts a load of a page it already has as a use",
    ),
    Mutation(
        "cache: drop the floor, so one huge page empties the cache",
        "src/lib/text.ts",
        " || this.pages.size <= TEXT_CACHE_FLOOR) break;",
        ") break;",
        "keeps a floor of pages larger than the bound itself",
    ),
    Mutation(
        "cache: hand back a dropped page as empty rather than fetching it",
        "src/lib/text.ts",
        "    const cached = this.pages.get(page);\n    if (cached) {",
        "    const cached = this.pages.get(page) ?? this.pages.values().next().value;\n    if (cached) {",
        "asks the backend again for a page it has dropped",
    ),
    Mutation(
        "cache: leave the turned view behind when the page is evicted",
        "src/lib/text.ts",
        "      this.turned.delete(oldest);",
        "",
        "drops the turned view with the page it was turned from",
    ),
    Mutation(
        "nearest: count a character PDFium gave no box",
        "src/lib/text.ts",
        "    if (!isPlaced(quad)) continue;\n\n    const dx = Math.max(quad.left - x, 0, x - quad.right);",
        "    const dx = Math.max(quad.left - x, 0, x - quad.right);",
        "has no character to find on a page that places none",
    ),
]

#: Suites this harness runs. Named once: `run_tests` and the name check below
#: must agree, or the second validates a list the first never runs.
TEST_FILES = [
    "src/lib/text.test.ts",
    "src/lib/clicks.test.ts",
    "src/lib/commands.test.ts",
    "src/lib/keys.test.ts",
    "src/lib/search.test.ts",
    "src/lib/textcache.test.ts",
    "src/lib/results.test.ts",
    "src/lib/recents.test.ts",
]

FAILED_TEST = re.compile(r"^\s*(?:x|×)\s+(.*?)(?:\s+\d+ms)?$", re.M)
TEST_NAME = re.compile(r"^\s*[✓x×]\s+\S+\.test\.ts\s*>\s*(.*?)(?:\s+\d+ms)?$", re.M)
SUMMARY = re.compile(r"^\s*Tests\s+(?:(\d+) failed)?.*?(\d+) passed", re.M)


def run_tests() -> tuple[set[str], int | None, str]:
    """Runs the suite, returning the failed test names, the summary's count and the log."""
    done = subprocess.run(
        ["npx", "vitest", "run", *TEST_FILES],
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


def all_test_names() -> set[str]:
    """Every test name the suite defines, from the verbose reporter."""
    done = subprocess.run(
        ["npx", "vitest", "run", "--reporter=verbose", *TEST_FILES],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=300,
    )
    out = done.stdout + done.stderr
    # `✓ src/lib/x.test.ts > describe > name 3ms` -- split on the marker and take
    # the rest, never a fixed column.
    return {m.strip() for m in TEST_NAME.findall(out) if m.strip()}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.list:
        for mutation in MUTATIONS:
            print(f"{mutation.name}  ->  expects: {mutation.expect}")
        return 0

    print("--- control: the suite must be green before anything is broken", flush=True)
    names, counted, out = run_tests()
    if counted is None:
        print("[FAIL] the control run produced no summary line, so nothing below is readable")
        print(out[-2000:])
        return 1
    if counted != 0 or names:
        print(f"[FAIL] the control run is not green: {counted} failed, {sorted(names)}")
        return 1
    print("[OK]   control green", flush=True)

    # Every `expect` must name a test this harness can actually run. One named a
    # check that only `viewer_check.py` records, and the run reported SURVIVED --
    # which reads as a gap in the suite rather than a mistake in the harness, and
    # is the most misleading verdict a mutation pass can print. Derived from the
    # control run's own list rather than from a hand-kept table.
    known = all_test_names()
    unknown = [m for m in MUTATIONS if not any(m.expect in name for name in known)]
    if unknown:
        for mutation in unknown:
            print(
                f"[FAIL] {mutation.name}: no test here is named {mutation.expect!r} -- "
                "it cannot go red, so this mutation would report SURVIVED"
            )
        return 1
    print(f"[OK]   every mutation names one of the {len(known)} tests", flush=True)

    problems = 0
    with tempfile.TemporaryDirectory(prefix="tpdf-mutate-") as scratch:
        for mutation in MUTATIONS:
            target = ROOT / mutation.path
            # Copied aside and written *back*, never moved: a move replaces
            # the file the tooling may already be watching, and docs/TRAPS.md
            # records a restore-by-move that left the mutated build in place.
            #
            # And written back rather than copied back: `shutil.copy2` preserves
            # the backup's mtime, which is enough to make a build system believe
            # the mutated artifact is current. It bit `mutate_rust.py`, where
            # cargo then served the last mutation to every later run.
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
                target.write_text(backup.read_text())

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
