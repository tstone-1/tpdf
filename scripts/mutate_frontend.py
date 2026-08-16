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


#: Two mutations that belong here and are deliberately absent, because running
#: them established they are *not* defects --- and a variant that changes no
#: behaviour looks exactly like a test that cannot fail:
#:
#:   * banding `reading.ts`'s characters in arrival order rather than sorted by
#:     position, and
#:   * splitting a band at any gap at all rather than at a gutter-sized one.
#:
#: Both survive because the design repairs them downstream: `blocksOf` re-applies
#: the threshold when it decides where the columns are, and `readingLines` merges
#: fragments that share a band within a block. So over-splitting and mis-banding
#: are both recoverable, and only *under*-splitting loses information --- which is
#: the mutation immediately below the two, and which is caught.
#:
#: Recorded rather than deleted silently: the next person to notice the gap
#: should find out that it was measured, not overlooked.
MUTATIONS = [
    Mutation(
        # `resumé` decomposed came back as three lines --- `resume`, the accent
        # alone, then the rest --- because an acute sits above the x-height and its
        # box does not touch a word with no ascender. `café` hides it: the `f`
        # reaches up far enough to drag the band into contact.
        "mark: let a combining mark open a band of its own",
        "src/lib/reading.ts",
        "    const mark = last >= 0 && combining(text.codes[index] ?? 0);",
        "    const mark = false;",
        "does not open a line of its own",
    ),
    Mutation(
        # The other direction: attach the mark to the character *after* it. The
        # line count is then right and the text reads `resum` `é` the wrong way
        # round, which a check on the number of lines cannot see.
        "mark: attach a combining mark to the character after it",
        "src/lib/reading.ts",
        "      const at = trailing.get(last) ?? [];\n      at.push(index);\n      trailing.set(last, at);\n      if (mark && placed(box)) {",
        "      const at = trailing.get(mark ? index : last) ?? [];\n      at.push(index);\n      trailing.set(mark ? index : last, at);\n      if (mark && placed(box)) {",
        "stays with the character it decorates",
    ),
    Mutation(
        # Key on the geometry rather than on the character: a small box high up.
        # It catches the accent and also catches a superscript, which is a
        # character in its own right with its own advance width.
        "mark: treat anything small and raised as a combining mark",
        "src/lib/reading.ts",
        "const COMBINING = /^[\\p{Mn}\\p{Me}]$/u;",
        "const COMBINING = /^[\\p{Mn}\\p{Me}0-9]$/u;",
        "keys on the character rather than on the box",
    ),
    Mutation(
        # Leave the mark out of the base's box. The line then reads correctly and
        # cannot be hit-tested at the top of the accent, which is the half a text
        # comparison is blind to.
        "mark: leave a combining mark out of its line's box",
        "src/lib/reading.ts",
        "          absorb(base.box, box);\n          base.extents = extentsOf(base.box, axes);",
        "",
        "is covered by its line's box",
    ),
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
        "    a.matchCase === b.matchCase &&\n    a.wholeWord === b.wholeWord &&\n    a.regex === b.regex",
        "    a.matchCase === b.matchCase",
        "is true only when both options agree",
    ),
    Mutation(
        "search: let the plain search match case",
        "src/lib/search.ts",
        "export const PLAIN_SEARCH: SearchOptions = {\n  matchCase: false,",
        "export const PLAIN_SEARCH: SearchOptions = {\n  matchCase: true,",
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
        # The anchor here went stale on 2026-08-02, when `statusFor` was
        # restructured to say what an unreadable page means: the harness
        # reported "its anchor appears 0 times", which is the right verdict and
        # is why it is checked before the run rather than inferred from a
        # survivor afterwards.
        "results: do not say a scan is still running",
        "src/lib/results.ts",
        '  if (running) return total === 0 ? "Searching…" : `${countOf(total)}, '
        "still searching…`;",
        "",
        "says a scan is still running",
    ),
    Mutation(
        # The tempting mistake, and `searchmapping.test.ts` says so in its own
        # header: `truncated` and `guessing` are both "not known to be fine", and
        # folding them together puts a warning on every encrypted document ---
        # `lopdf` cannot paginate one at all, so every page of it comes back
        # truncated. A false alarm on a file the reader can search perfectly well.
        #
        # It was proved by an ad-hoc mutation when the module landed and by
        # nothing afterwards, because the test file was not in TEST_FILES above.
        # Same gap `encoding::` had in `mutate_rust.py`, on the other side.
        "mapping: report a page nobody could judge as unreadable",
        "src/lib/search.ts",
        "    return this.mapping.filter((page) => page.guessing > 0).length;",
        "    return this.mapping.filter((page) => page.guessing > 0 || page.truncated).length;",
        "does not count a page nobody could judge",
    ),
    Mutation(
        "mapping: never say the backend has answered",
        "src/lib/search.ts",
        "    this.mappingSettled = true;",
        "",
        "says whether the question has been answered",
    ),
    Mutation(
        # The other direction. Together they say the flag is *set by the fetch*
        # rather than that one of its two values happens to satisfy the check
        # harness that waits on it.
        "mapping: say the backend has answered before it is asked",
        "src/lib/search.ts",
        "  private mappingSettled = false;",
        "  private mappingSettled = true;",
        "says whether the question has been answered",
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
    Mutation(
        "zoom: fit the page to the larger of the two fits",
        "src/lib/zoom.ts",
        "  return clampZoom(Math.min(wide, viewport.height / page.height_pt));",
        "  return clampZoom(Math.max(wide, viewport.height / page.height_pt));",
        "fits a page by its height when the window is wide",
    ),
    Mutation(
        "zoom: fit the page to its height alone",
        "src/lib/zoom.ts",
        "  return clampZoom(Math.min(wide, viewport.height / page.height_pt));",
        "  return clampZoom(viewport.height / page.height_pt);",
        "fits a page by its width when the window is tall",
    ),
    Mutation(
        "zoom: subtract the horizontal margin vertically too",
        "src/lib/zoom.ts",
        "  return clampZoom(Math.min(wide, viewport.height / page.height_pt));",
        "  return clampZoom(Math.min(wide, (viewport.height - FIT_MARGIN * 2) / page.height_pt));",
        "fits a page by its height when the window is wide",
    ),
    Mutation(
        "zoom: fit the width with no margin either side",
        "src/lib/zoom.ts",
        "  const wide = (viewport.width - FIT_MARGIN * 2) / page.width_pt;",
        "  const wide = viewport.width / page.width_pt;",
        "leaves a margin either side when fitting the width",
    ),
    Mutation(
        "zoom: let fit-width bound itself by the height as well",
        "src/lib/zoom.ts",
        '  if (mode === "width") return clampZoom(wide);\n',
        "",
        "ignores the viewport height when fitting the width",
    ),
    Mutation(
        "zoom: clamp a zoom that is not a number the arithmetic way",
        "src/lib/zoom.ts",
        "  if (!Number.isFinite(zoom)) return MIN_ZOOM;\n",
        "",
        "turns a zoom that is not a number into the smallest one",
    ),
    Mutation(
        "zoom: hand back the end stop instead of saying there is none",
        "src/lib/zoom.ts",
        "  return stop ?? null;",
        "  return stop ?? zoom;",
        "says there is no next stop rather than returning the last one again",
    ),
    Mutation(
        "zoom: let a step find the stop it is standing on",
        "src/lib/zoom.ts",
        "      ? ZOOM_STEPS.find((z) => z > zoom + 1e-6)",
        "      ? ZOOM_STEPS.find((z) => z >= zoom)",
        "does not find the stop it is standing on",
    ),
    Mutation(
        "zoom: parse a typed zoom the way `Number` would",
        "src/lib/zoom.ts",
        "  if (!/^[0-9]+(\\.[0-9]+)?$/.test(trimmed)) return null;\n",
        "",
        "refuses what `Number` would have accepted",
    ),
    Mutation(
        "zoom: accept a typed zoom outside the range",
        "src/lib/zoom.ts",
        "  if (zoom < MIN_ZOOM || zoom > MAX_ZOOM) return null;\n",
        "",
        "refuses a zoom outside the range rather than clamping it",
    ),
    Mutation(
        "zoom: truncate the percentage instead of rounding it",
        "src/lib/zoom.ts",
        "  return Math.round(zoom * 100);",
        "  return Math.floor(zoom * 100);",
        "rounds to whole percent",
    ),
    Mutation(
        "zoom: give two fit modes the same words",
        "src/lib/zoom.ts",
        '  if (mode === "page") return "Fit page";',
        '  if (mode === "page") return "Fit width";',
        "gives each mode its own words",
    ),
    Mutation(
        "reading: cut rows before columns",
        "src/lib/reading.ts",
        "  const columns = split(spans, (s) => [s.extents.alongStart, s.extents.alongEnd], gap);",
        "  const columns: Span[][] = [];",
        "reads two columns down and then across, however they were emitted",
    ),
    Mutation(
        "reading: cut at every row gap rather than the widest",
        "src/lib/reading.ts",
        "  const rows = splitOnce(spans, (s) => [s.extents.crossStart, s.extents.crossEnd]);",
        "  const rows = split(spans, (s) => [s.extents.crossStart, s.extents.crossEnd], gap);",
        # Predicted as the two-column test and caught by the heading one, which
        # is the right answer: a row cut only ever happens where no column cut
        # is available, and a plain two-column page always has one.
        "keeps a heading that spans the columns above both of them",
    ),
    Mutation(
        "reading: never split a band, however wide the gap",
        "src/lib/reading.ts",
        "      if (current.length > 0 && item.extents.alongStart - reach > gap) {",
        "      if (false) {",
        "splits a band where the gap is wider than a few characters",
    ),
    Mutation(
        "reading: forget which way a line runs when the page is turned",
        "src/lib/reading.ts",
        "    alongSign: at === 2 || at === 3 ? -1 : 1,",
        "    alongSign: 1,",
        "reads a rotated page the same way it reads an upright one",
    ),
    Mutation(
        "reading: forget which way the lines advance when the page is turned",
        "src/lib/reading.ts",
        "    crossSign: at === 1 || at === 2 ? -1 : 1,",
        "    crossSign: 1,",
        "reads a rotated page the same way it reads an upright one",
    ),
    Mutation(
        "reading: take every page as reading left to right",
        "src/lib/reading.ts",
        "    sideways: at % 2 === 1,",
        "    sideways: false,",
        "puts lines across the page when it is turned a quarter",
    ),
    Mutation(
        "reading: average the character widths rather than taking the median",
        "src/lib/reading.ts",
        "  const median = widths[Math.floor(widths.length / 2)] ?? 0;",
        "  const median = widths.reduce((sum, w) => sum + w, 0) / (widths.length || 1);",
        "is not moved by one enormous character",
    ),
    Mutation(
        "reading: cut at a fixed distance instead of a multiple of the type",
        "src/lib/reading.ts",
        "  return median * CUT_CHARS;",
        "  return 30;",
        "scales with the type rather than with the page",
    ),
    Mutation(
        "reading: drop the characters PDFium placed nowhere",
        "src/lib/reading.ts",
        """    if (!placed(box) || sliver(extents, typical) || mark) {
      const at = trailing.get(last) ?? [];""",
        """    if (!placed(box) || sliver(extents, typical) || mark) {
      if (!placed(box)) continue;
      const at = trailing.get(last) ?? [];""",
        "returns every character exactly once",
    ),
    Mutation(
        "reading: never notice two lines side by side",
        "src/lib/reading.ts",
        "      if (sameBand(bands[a] as Extents, bands[b] as Extents)) return true;",
        "      if (false) return true;",
        "is true where two lines sit at the same height",
    ),
    Mutation(
        "reading: copy a range in the order the file was written",
        "src/lib/reading.ts",
        "  const wanted = readingOrder(text).filter((index) => index >= start && index < end);",
        "  const wanted = readingOrder(text)\n    .filter((index) => index >= start && index < end)\n    .sort((a, b) => a - b);",
        "emits a range in reading order rather than index order",
    ),
    Mutation(
        "reading: sort a line's ranges instead of keeping them ordered",
        "src/lib/reading.ts",
        "  for (const range of ranges) {",
        "  for (const range of [...ranges].sort((a, b) => a.from - b.from)) {",
        "concatenates the ranges in the order it is given them",
    ),
    Mutation(
        "structure: ignore the document's own reading order",
        "src/lib/reading.ts",
        "  const tagged = usableRuns(text);",
        "  const tagged = null;",
        "reads the page in the order the tags give",
    ),
    Mutation(
        "structure: use the runs even when they leave visible text unclaimed",
        "src/lib/reading.ts",
        "    if (isVisible(code) && placed(charQuad(text, index))) return null;",
        "",
        "is null for a page whose runs leave a visible character unclaimed",
    ),
    Mutation(
        # Written first against an *unplaced* space, and it survived: that case
        # is refused by the other half of the condition, so the whitespace half
        # was covered by nothing. The fixture now uses a space with a box.
        "structure: reject a page over an untagged space",
        "src/lib/reading.ts",
        "    if (isVisible(code) && placed(charQuad(text, index))) return null;",
        "    if (placed(charQuad(text, index))) return null;",
        "ignores an unclaimed character that is only whitespace",
    ),
    Mutation(
        "structure: reject a page over a character PDFium placed nowhere",
        "src/lib/reading.ts",
        "    if (isVisible(code) && placed(charQuad(text, index))) return null;",
        "    if (isVisible(code)) return null;",
        "ignores an unclaimed character PDFium placed nowhere",
    ),
    Mutation(
        # The clipping, rather than the bucketing. A fragment that straddles two
        # elements is the case a reading order gets wrong quietly: the words end
        # up in one block, in the wrong order, and every count still agrees.
        "structure: put a straddling fragment in whichever run it starts in",
        "src/lib/reading.ts",
        "      for (let index = range.from; index < range.to; index++) {\n        if (!mine.has(index)) continue;",
        "      if (!mine.has(range.from)) continue;\n      for (let index = range.from; index < range.to; index++) {",
        "clips a fragment that straddles two runs",
    ),
    Mutation(
        # Order the tagged blocks by position instead of by the tags, which is
        # the mutation the margin-note fixture alone cannot catch: geometry and
        # tags agree about *which* block comes first there for two of three.
        "structure: order the tagged blocks geometrically after all",
        "src/lib/reading.ts",
        "  if (tagged) {\n    return ownership(text, tagged).map((owned, at) => ({",
        "  if (tagged) {\n    tagged = [...tagged].sort((a, b) => a.start - b.start);\n    return ownership(text, tagged).map((owned, at) => ({",
        "follows the tags even where they disagree with the geometry entirely",
    ),
    Mutation(
        "structure: emit only the characters a run claims",
        "src/lib/reading.ts",
        "  let last = -1;\n  for (let index = 0; index < owner.length; index++) {\n    if (owner[index] === -1) owner[index] = last;\n    else last = owner[index] as number;\n  }\n  // Backwards for anything before the first claimed character, which the forward\n  // pass could only leave at -1.\n  let next = runs.length > 0 ? 0 : -1;\n  for (let index = owner.length - 1; index >= 0; index--) {\n    if (owner[index] === -1) owner[index] = next;\n    else next = owner[index] as number;\n  }",
        "",
        "are still in the reading order",
    ),
    Mutation(
        "structure: give an unclaimed character to the run after it, not before",
        "src/lib/reading.ts",
        "  let last = -1;\n  for (let index = 0; index < owner.length; index++) {\n    if (owner[index] === -1) owner[index] = last;\n    else last = owner[index] as number;\n  }",
        "",
        "stay with the text they follow",
    ),
    Mutation(
        # Found by the tagged fixture and present long before it: the geometric
        # path produced the same broken line.
        "reading: let a comma open a line of its own",
        "src/lib/reading.ts",
        "  if (shorter < Math.max(...heights) * SHORT_MARK) return true;",
        "",
        "is one line, not a line of letters and a line of marks",
    ),
    Mutation(
        "reading: let any box that touches a line join it",
        "src/lib/reading.ts",
        "  if (shorter < Math.max(...heights) * SHORT_MARK) return true;",
        "  return true;",
        "does not make two real lines into one",
    ),
    Mutation(
        "a11y: flatten every heading to one level",
        "src/lib/a11y.ts",
        "  if (heading) return `h${heading[1]}`;",
        '  if (heading) return "h1";',
        "gives a heading the level the document stated",
    ),
    Mutation(
        # Written first as a dropped `$`, and it survived: neither `H7` nor
        # `Hyperlink` matches `^H([1-6])` either way, so the anchor was not what
        # those cases test. The `$` is covered by `H1Alt`; the character class is
        # covered by `H7`, and HTML has no `h7`.
        "a11y: accept a heading level HTML does not have",
        "src/lib/a11y.ts",
        "  const heading = /^H([1-6])$/.exec(tag);",
        "  const heading = /^H([0-9])$/.exec(tag);",
        "does not read a level out of a type that merely starts with H",
    ),
    Mutation(
        "a11y: read a level off the front of a longer type",
        "src/lib/a11y.ts",
        "  const heading = /^H([1-6])$/.exec(tag);",
        "  const heading = /^H([1-6])/.exec(tag);",
        "does not read a level out of a type that merely starts with H",
    ),
    Mutation(
        "a11y: leave an unlevelled heading as a paragraph",
        "src/lib/a11y.ts",
        '  return tag === "H" ? "h2" : "p";',
        '  return "p";',
        "gives a bare H a level, since the document did not",
    ),
    Mutation(
        # The distinction the whole block/line split exists for: an inferred
        # boundary is not a stated one.
        "a11y: treat an inferred block as a stated paragraph",
        "src/lib/reading.ts",
        "  return blocksOf(fragments, axes, gap).map((block) => ({\n    tag: null,",
        '  return blocksOf(fragments, axes, gap).map((block) => ({\n    tag: "P",',
        "reports an inferred block as having no type",
    ),
    Mutation(
        "a11y: lose the run's type on the way to the consumer",
        "src/lib/reading.ts",
        "      tag: tagged[at]?.tag ?? null,",
        "      tag: null,",
        "carries each tagged run's type",
    ),
    Mutation(
        # Take the first mark the point is inside rather than the smallest. A
        # note icon dropped inside a square is inside both, and which one opens
        # then depends on the order the producer wrote them in.
        "comments: open whichever mark the file listed first",
        "src/lib/comments.ts",
        "    if (area <= bestArea) {",
        "    if (best === null) {",
        "prefers the smaller of two marks the point is inside",
    ),
    Mutation(
        # Hit-test a hidden comment. `/F` bit 2 means the page does not show it,
        # so this opens a note for a mark that is not there --- and the panel
        # still lists it, which is why the two cannot be the same rule.
        "comments: let a hidden comment be pressed",
        "src/lib/comments.ts",
        "    if (item.page !== page || item.hidden) continue;",
        "    if (item.page !== page) continue;",
        "ignores a hidden comment",
    ),
    Mutation(
        # Treat a rectangle of no area as a hit, which puts an invisible target
        # in the page's top-left corner --- exactly where `annots.rs` reports a
        # `/Rect` it could not read.
        "comments: accept a rectangle with no area",
        "src/lib/comments.ts",
        "    if (width <= 0 || height <= 0) continue;",
        "    if (false) continue;",
        "ignores a rectangle with no area",
    ),
    Mutation(
        # Indent a reply by its chain depth. A reply to a reply then sits two
        # levels in, which a 260-pixel panel does not have room for --- and the
        # row order is unchanged, so a check on the list contents cannot see it.
        "comments: indent a reply by how deep its chain runs",
        "src/lib/comments.ts",
        "      emit(reply, 1, budget - 1);",
        "      emit(reply, depth + 1, budget - 1);",
        "indents a reply to a reply once, not twice",
    ),
    Mutation(
        # Turn the caller's own array rather than a copy. The viewer holds that
        # array permanently and calls this on every pointer press, so the marks
        # walk off the page one press at a time.
        "comments: turn the rectangles in place",
        "src/lib/comments.ts",
        "  return items.map((item) => {\n    const quad = viewRect(item.rect, turns, width, height);",
        "  return items.map((item) => {\n    const quad = viewRect(item.rect, turns, width, height);\n    item.rect = [quad.left, quad.top, quad.right, quad.bottom];",
        "copies the list rather than turning it in place",
    ),
    Mutation(
        # Say something whatever the limits were. A notice that is always there
        # is a notice nobody reads, and this one exists to say the list in front
        # of the reader is incomplete.
        "comments: say the list is incomplete even when nothing was cut",
        "src/lib/comments.ts",
        "  if (parts.length === 0) return null;",
        '  if (parts.length === 0) parts.push("nothing worth mentioning");',
        "says nothing when nothing was cut",
    ),
    Mutation(
        # Point every reply's row at its parent, including one whose parent is
        # not in the list. `aria-describedby` naming an absent element tells a
        # screen reader to read nothing, which is worse than the indent alone.
        "comments: name a parent row that is not there",
        "src/lib/commentlist.ts",
        "    if (comment.reply_to !== null && this.elements.has(comment.reply_to)) {",
        "    if (comment.reply_to !== null) {",
        "names the row a reply answers, and only when that row is there",
    ),
    Mutation(
        # Read the roving tabindex's own mirror rather than the row the key
        # landed on. The outline paid for this one: a window without system
        # focus moves `activeElement` without delivering `focusin`.
        "comments: activate the row the panel last remembered",
        "src/lib/commentlist.ts",
        "    const from = idOf(event.target) ?? this.focused;",
        "    const from = this.focused;",
        "activates the row the key landed on, not the one it remembered",
    ),
    Mutation(
        # Always open to the right of the mark. A note on a comment near the
        # right edge then opens past the window, where the reader cannot see it
        # at all --- and every note on every other comment still looks right.
        "comments: open the note to the right of the mark whatever the room",
        "src/lib/commentpopup.ts",
        "      rightOf + POPUP_WIDTH + MARGIN <= width",
        "      true",
        "flips to the left of the mark when there is not",
    ),
    Mutation(
        # Take the keyboard on every open. Pressing a mark then moves focus off
        # the page, and the arrow keys stop scrolling --- which reads as the
        # viewer having frozen.
        "comments: take the keyboard whenever a note opens",
        "src/lib/commentpopup.ts",
        "    if (focus) this.element.focus();",
        "    this.element.focus();",
        "takes the keyboard only when asked",
    ),
    Mutation(
        # Hide the note without emptying it. Nothing on screen changes, and the
        # check harness reading `commentText` after a close is told what the
        # last comment said.
        "comments: hide the note without emptying it",
        "src/lib/commentpopup.ts",
        "    this.element.style.display = \"none\";\n    this.element.replaceChildren();",
        "    this.element.style.display = \"none\";",
        "forgets the comment when it hides",
    ),
    # --- links.ts -----------------------------------------------------------
    Mutation(
        # Take the largest of two overlapping links rather than the smallest. A
        # producer that wraps a paragraph in one link and a phrase inside it in
        # another is ordinary, and the phrase is what the reader aimed at.
        "links: take the largest overlapping link rather than the smallest",
        "src/lib/links.ts",
        "    if (area <= bestArea) {",
        "    if (area >= bestArea) {",
        "takes the smallest of two overlapping links",
    ),
    Mutation(
        # Give links the comments' three points of slack. Neighbouring links are
        # a point or two apart on a wrapped sentence, so the gap between two
        # then belongs to both and the second one listed wins.
        "links: use the comment slack, so neighbours overlap",
        "src/lib/links.ts",
        "export const LINK_SLACK_PT = 1;",
        "export const LINK_SLACK_PT = 3;",
        "keeps neighbouring links apart, which is why the slack is small",
    ),
    Mutation(
        # Hit-test a zero-area rectangle, which puts an invisible target where
        # the file wrote nothing usable.
        "links: hit-test a rectangle with no area",
        "src/lib/links.ts",
        "    if (width <= 0 || height <= 0) continue;",
        "    if (false) continue;",
        "ignores a rectangle with no area",
    ),
    Mutation(
        # Push the popped place onto the forward stack instead of where the
        # reader is now. Back still moves, so a check that only asserted Back
        # would stay green --- and Forward becomes a toggle back to the origin.
        "links: forward returns to the origin rather than the destination",
        "src/lib/links.ts",
        "    const to = this.past.pop();\n    if (!to) return null;\n    this.future.push(now);\n    return to;",
        "    const to = this.past.pop();\n    if (!to) return null;\n    this.future.push(to);\n    return to;",
        "goes forward to where going back left",
    ),
    Mutation(
        # Keep the forward branch when a new jump is made, so Forward offers a
        # place the reader abandoned and never chose.
        "links: keep the forward branch across a new jump",
        "src/lib/links.ts",
        "    this.past.push(from);\n    if (this.past.length > MAX_HISTORY) this.past.shift();\n    this.future.length = 0;",
        "    this.past.push(from);\n    if (this.past.length > MAX_HISTORY) this.past.shift();",
        "drops the forward branch on a new jump",
    ),
    Mutation(
        # Record a jump that lands where the reader already is, so pressing one
        # cross-reference twice needs Back twice.
        "links: record a jump that goes nowhere",
        "src/lib/links.ts",
        "    if (top && samePlace(top, from)) {",
        "    if (false && top && samePlace(top, from)) {",
        "does not record a jump that lands where the reader already was",
    ),
    Mutation(
        # Drop the newest entry rather than the oldest when the stack is full,
        # which silently stops recording at the point navigation got interesting.
        "links: drop the newest history entry rather than the oldest",
        "src/lib/links.ts",
        "    if (this.past.length > MAX_HISTORY) this.past.shift();",
        "    if (this.past.length > MAX_HISTORY) this.past.pop();",
        "drops the oldest entry rather than refusing a new one",
    ),
    Mutation(
        # Say nothing about a refused link. A rectangle that swallows a click
        # without a word is indistinguishable from a broken viewer.
        "links: refuse a link silently",
        "src/lib/links.ts",
        "  if (isNavigable(target)) return null;\n  const reason = reasonFor(target);\n  return reason ? `This link ${reason}.` : null;",
        "  return null;",
        "uses the outline's words for a refused action",
    ),
    Mutation(
        # Report a cut list as complete, which is the failure every bound in this
        # application is arranged to avoid.
        "links: report a truncated scan as complete",
        "src/lib/links.ts",
        '  if (limits.over_budget) parts.push("too many links to list them all");',
        "  if (false) parts.push(\"unreachable\");",
        "names each bound separately",
    ),
    Mutation(
        # Order links by top-then-left with no line banding. Two links on one
        # line come out in the wrong order whenever the right-hand one's box
        # starts a point higher, which on real text is most of the time.
        "links: order links by top-then-left with no line banding",
        "src/lib/links.ts",
        "    if (!sameLine(a, b)) return a.rect[1] - b.rect[1];",
        "    if (a.rect[1] !== b.rect[1]) return a.rect[1] - b.rect[1];",
        "orders across the page for two links on one line",
    ),
    Mutation(
        # Band by an absolute overlap rather than a fraction of the shorter box.
        # A footnote marker is 8 points tall against a 20-point sentence, so a
        # constant tuned for body text separates them onto two lines.
        "links: band lines by absolute overlap rather than by proportion",
        "src/lib/links.ts",
        "  return shorter > 0 && overlap >= shorter * SAME_LINE_OVERLAP;",
        "  return shorter > 0 && overlap >= 10;",
        "keeps a footnote marker on the line it sits in",
    ),
    Mutation(
        # Leave the order partial where two rectangles are identical, so which
        # link is "next" depends on the sort's stability rather than on a rule.
        "links: leave the link order partial for identical rectangles",
        "src/lib/links.ts",
        "    if (a.rect[0] !== b.rect[0]) return a.rect[0] - b.rect[0];\n    return a.id - b.id;",
        "    return a.rect[0] - b.rect[0];",
        "is a total order even for identical rectangles",
    ),
    Mutation(
        # Sort in place, which reorders the caller's array. The viewer holds the
        # scan's order for hit-testing and the walk order separately.
        "links: sort the caller's array in place",
        "src/lib/links.ts",
        "  return [...items].sort((a, b) => {",
        "  return (items as Link[]).sort((a, b) => {",
        "does not modify the array it is given",
    ),
    Mutation(
        # Wrap at the end of the document instead of stopping. On 775 pages
        # arriving back at page 1 is a surprise, and the reader has no way to
        # tell it from having walked the whole document.
        "links: wrap the link walk at the end",
        "src/lib/links.ts",
        "    if (index >= 0) return ordered[index + direction] ?? null;",
        "    if (index >= 0)\n      return (\n        ordered[index + direction] ??\n        (direction === 1 ? ordered[0] : ordered[ordered.length - 1]) ??\n        null\n      );",
        "stops at each end rather than wrapping",
    ),
    Mutation(
        # Start the walk at the top of the document rather than at the viewport.
        # A reader on page 400 pressing "next link" is sent back to page 1.
        "links: start the link walk at the document rather than the viewport",
        "src/lib/links.ts",
        "    return ordered.find((item) => isAfter(item, at)) ?? null;",
        "    return ordered[0] ?? null;",
        "starts from the viewport when nothing is focused",
    ),
    Mutation(
        # Treat a link level with the viewport as behind it, so Previous lands
        # on the link Next just arrived at and the pair becomes a toggle.
        "links: treat a link level with the viewport as behind it",
        "src/lib/links.ts",
        "  return link.rect[1] < at.top;",
        "  return link.rect[1] <= at.top;",
        "goes back to the link before the viewport, not the one level with it",
    ),
    Mutation(
        # Give up when the focused link is not in the list, instead of falling
        # back to the viewport. After a reload the key then does nothing at all.
        "links: give up when the focused link is stale",
        "src/lib/links.ts",
        "    if (index >= 0) return ordered[index + direction] ?? null;",
        "    return ordered[index + direction] ?? null;",
        "falls back to the viewport when the focused link is gone",
    ),
    Mutation(
        # Take a character into a link when their boxes overlap rather than when
        # the character's centre is inside. Annotation rectangles are drawn
        # generously around their text, so this makes a link claim the word on
        # either side of it and a screen reader announce a link with a stray word
        # at each end.
        "links: claim a character whose box merely overlaps the link",
        "src/lib/links.ts",
        "    if (x >= l && x <= r && y >= t && y <= b) return link;",
        "    if (right >= l && left <= r && bottom >= t && top <= b) return link;",
        "takes a character by its centre, not by its box overlapping",
    ),
    Mutation(
        # Merge adjacent runs whose links point at the same place. Two
        # cross-references to one chapter are two links, and merging them
        # announces them as one.
        "links: merge adjacent runs that point at the same page",
        "src/lib/links.ts",
        "      if (last && last.link === found) {",
        "      if (\n        last &&\n        (last.link === found ||\n          (last.link !== null &&\n            found !== null &&\n            JSON.stringify(last.link.target) === JSON.stringify(found.target)))\n      ) {",
        "keeps two links apart even where they point at the same page",
    ),
    Mutation(
        # Index a link into the band of its top edge only. A link taller than one
        # band is then invisible to any character below its first 12 points.
        "links: index a link into one band rather than every band it covers",
        "src/lib/links.ts",
        "    const last = Math.floor(bottom / BAND_PT);",
        "    const last = first;",
        "finds a link on a band boundary",
    ),
    Mutation(
        # Read past the end of the boxes array without noticing. `undefined`
        # compares false against every bound, so this marks the tail of an
        # over-long range as ordinary text --- or as a link, depending on which
        # comparison is written first.
        "links: read a character box past the end of the array",
        "src/lib/links.ts",
        "  if (\n    left === undefined ||\n    top === undefined ||\n    right === undefined ||\n    bottom === undefined\n  ) {\n    return null;\n  }\n  const x = (left + right) / 2;\n  const y = (top + bottom) / 2;",
        "  const x = ((left ?? 0) + (right ?? 0)) / 2;\n  const y = ((top ?? 0) + (bottom ?? 0)) / 2;",
        "handles a range that runs past the boxes it has",
    ),
    Mutation(
        # Index a link with no height, which then covers a band and claims every
        # character whose centre falls on that exact line.
        "links: index a link whose rectangle has no height",
        "src/lib/links.ts",
        "    if (!(bottom > top)) continue;",
        "    if (false) continue;",
        "ignores a link whose rectangle has no height",
    ),
    Mutation(
        # Announce a refused link as an ordinary one. The reader is told it is a
        # link, presses it, and nothing happens --- misled by us rather than by
        # the file.
        "a11y: announce a refused link as an ordinary one",
        "src/lib/a11y.ts",
        '      span.setAttribute("aria-disabled", "true");',
        '      span.dataset.page = "0";',
        "says a refused link is unavailable rather than leaving it inert",
    ),
    Mutation(
        # Mark up only the pages built after the links arrive. They land on their
        # own chain after first paint, so this leaves the first page of every
        # document announced as prose --- the one page every reader sees.
        "a11y: do not rebuild the pages already built when links arrive",
        "src/lib/a11y.ts",
        "      const from = this.built.get(page);\n      if (!from) continue;",
        "      const from = this.built.get(page);\n      if (!from || true) continue;",
        "rebuilds a page that was already built when the links arrive",
    ),
    Mutation(
        # Emit the link's text without the role, which is the whole announcement:
        # the words are read either way, and nothing says they are a link.
        "a11y: emit a link's text without saying it is a link",
        "src/lib/a11y.ts",
        '    span.setAttribute("role", "link");',
        '    span.setAttribute("data-role", "link");',
        "announces a link as a link, and only the characters it covers",
    ),
    Mutation(
        # `viewer.ts` had no mutation coverage at all until 2026-08-16, and both
        # defects found that day were in it --- one of them shipped in 26.8.0.
        # Its tests exist; nothing was checking that they could fail.
        #
        # Replay a recorded place as though it were a destination, which is what
        # `jumpTo` used to do. The margin comes off a second time on every jump,
        # so a Back/Forward round trip drifts a page each time.
        "viewer: replay a recorded place through the destination path",
        "src/lib/viewer.ts",
        "      const page = Math.max(0, Math.min(place.page, this.opts.pageCount - 1));\n"
        "      const offset = this.turns === 0 ? Math.max(0, place.top) : 0;\n"
        "      this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);",
        "      this.goToDestination(place.page, place.top);",
        "leaves air above a destination, and none above a recorded place",
    ),
    Mutation(
        # Take the margin off an offset of zero, which scrolls into the previous
        # page. Every `/Fit` destination, every heading within 6 pt of a page
        # top, and every destination at all on a rotated view.
        "viewer: let a destination's margin cross into the previous page",
        "src/lib/viewer.ts",
        "    const air = Math.max(0, offset - DESTINATION_MARGIN_PT);",
        "    const air = offset - DESTINATION_MARGIN_PT;",
        "lands on the page a top-of-page destination names, not the one before",
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
    "src/lib/zoom.test.ts",
    "src/lib/reading.test.ts",
    "src/lib/a11y.test.ts",
    "src/lib/searchmapping.test.ts",
    "src/lib/comments.test.ts",
    "src/lib/commentlist.test.ts",
    "src/lib/commentpopup.test.ts",
    "src/lib/links.test.ts",
    "src/lib/viewer.test.ts",
]

FAILED_TEST = re.compile(r"^\s*(?:x|×)\s+(.*?)(?:\s+\d+ms)?$", re.M)
TEST_NAME = re.compile(r"^\s*[✓x×]\s+\S+\.test\.ts\s*>\s*(.*?)(?:\s+\d+ms)?$", re.M)
SUMMARY = re.compile(r"^\s*Tests\s+(?:(\d+) failed)?.*?(\d+) passed", re.M)


def npx() -> str:
    """Resolves npx, which is `npx.cmd` on Windows and not on PATH as `npx`."""
    return shutil.which("npx") or "npx"


def run_tests() -> tuple[set[str], int | None, str]:
    """Runs the suite, returning the failed test names, the summary's count and the log."""
    done = subprocess.run(
        [npx(), "vitest", "run", *TEST_FILES],
        cwd=ROOT,
        capture_output=True,
        text=True,
        # vitest marks a test with U+2713/U+00D7, and `text=True` alone decodes
        # with the locale codec -- cp1252 on Windows, where those bytes become
        # mojibake and every mark-keyed regex silently matches nothing.
        encoding="utf-8",
        errors="replace",
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
        [npx(), "vitest", "run", "--reporter=verbose", *TEST_FILES],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
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
                # Bytes, decoded explicitly: `read_text` uses the locale codec,
                # which is cp1252 on Windows, and an anchor holding a glyph like
                # the Option sign then matches nothing -- reported as "the
                # mutation is not the one described", which reads as drift in
                # the source rather than in this harness.
                #
                # Its newline translation was doing real work, though, and
                # removing it alone takes three failures to twelve: a Windows
                # checkout is CRLF and every anchor here is written with "\n".
                # So normalise for matching and put the file's own convention
                # back, leaving the mutation as the only difference on disk.
                raw = target.read_bytes().decode("utf-8")
                crlf = "\r\n" in raw
                source = raw.replace("\r\n", "\n") if crlf else raw
                if source.count(mutation.before) != 1:
                    print(
                        f"[FAIL] {mutation.name}: its anchor appears "
                        f"{source.count(mutation.before)} times, so the mutation is not the "
                        "one described"
                    )
                    problems += 1
                    continue
                mutated = source.replace(mutation.before, mutation.after)
                if crlf:
                    mutated = mutated.replace("\n", "\r\n")
                target.write_bytes(mutated.encode("utf-8"))
                names, counted, out = run_tests()
            finally:
                target.write_bytes(backup.read_bytes())

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
