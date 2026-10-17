"""One part of the frontend mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

ROWS += [
    Mutation(
        # Drop the whole-title bonus. Two commands whose titles prefix one
        # another then tie on every term the scorer has, and registration order
        # decides -- which is how "Redact and save" came to highlight "Redact
        # and save as..." for a reader who had typed the first one in full.
        "rank: score a whole-title match like any other match",
        "src/lib/commands.ts",
        "      command.title.toLowerCase() === trimmed.toLowerCase() ? EXACT : 0;",
        "      false ? EXACT : 0;",
        "ranks the exact title first, whatever the registration order",
    ),
    Mutation(
        # Compare unfolded. Everything else here matches case-insensitively, so
        # this is the one spelling where typing in lower case would change the
        # answer -- and it would do it silently, on the input people actually
        # type.
        "rank: make the whole-title match case-sensitive",
        "src/lib/commands.ts",
        "      command.title.toLowerCase() === trimmed.toLowerCase() ? EXACT : 0;",
        "      command.title === trimmed ? EXACT : 0;",
        "is case-insensitive, as every other match here is",
    ),
    Mutation(
        # Reverse it, which is the mutation the two-orders test exists for: the
        # exact match is pushed BELOW the title that merely starts with the
        # query, so the answer is wrong from either registration order rather
        # than from one.
        "rank: rank a whole-title match below a partial one",
        "src/lib/commands.ts",
        "const EXACT = WORD_START * 4;",
        "const EXACT = -WORD_START * 4;",
        "ranks it first from the other registration order too",
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
        '  return `${binding.ctrl ? "Ctrl+" : ""}${binding.alt ? "⌥" : ""}${binding.shift ? "⇧" : ""}',
        '  return `${binding.ctrl ? "Ctrl+" : ""}${binding.shift ? "⇧" : ""}${binding.alt ? "⌥" : ""}',
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
        # File a reply's hits under the page numbers the backend gave them. Those
        # are pages of the file, so a hit on page 0 of an inserted file lands on
        # slot 0; the window check's import phase found it after the unit test,
        # whose mock numbered hits by slot, had passed.
        "search: file hits under the backend's file page",
        "src/lib/search.ts",
        "const inScope = inSlots(result.matches, page).filter",
        "const inScope = result.matches.filter",
        "asks the file each page is drawn from, and finds a match on the imported one",
    ),
    Mutation(
        "search: relabel a carried hit as starting on the slot asked about",
        "src/lib/search.ts",
        "      : { ...m, page: slot - 1, endPage: slot },",
        "      : { ...m, page: slot, endPage: slot },",
        "files a hit that began in the carry under the slot before",
    ),
    Mutation(
        "search: let the plain search match case",
        "src/lib/search.ts",
        "export const PLAIN_SEARCH: SearchOptions = {\n  matchCase: false,",
        "export const PLAIN_SEARCH: SearchOptions = {\n  matchCase: true,",
        "describes the plain search as neither option",
    ),
    Mutation(
        # A run has to be contiguous in slots, or the walk's own carry rule
        # (`carried.page === page - 1`) does not hold inside one -- and an
        # unscoped scan wraps, so page 774 is followed by page 0.
        "search: run across a gap in the slots",
        "src/lib/search.ts",
        "    if (last !== undefined && entry.page !== last + 1) break;",
        "    if (false) break;",
        "stops at a gap in the slots",
    ),
    Mutation(
        # The second vocabulary, and the one an unedited document cannot show:
        # slots and file pages are the same sequence until a page is deleted, and
        # the backend chains the carry between pages *it* sees as neighbours.
        "search: run across a gap in the file pages",
        "src/lib/search.ts",
        "    if (previous !== undefined && source.page !== previous.page + 1) break;",
        "    if (false) break;",
        "stops where the file pages are not consecutive either",
    ),
    Mutation(
        # Ask about the whole plan in one request. The render thread is FIFO, so
        # this is a second and a half in front of every tile on the dense corpus.
        "search: ignore the run bound",
        "src/lib/search.ts",
        "  for (let step = at; step < plan.length && run.length < RUN_PAGES; step++) {",
        "  for (let step = at; step < plan.length; step++) {",
        "takes a whole run of consecutive pages, bounded by RUN_PAGES",
    ),
    Mutation(
        # Start every run at the beginning of the plan. The walk would re-ask
        # about the pages it has and never reach the ones it does not.
        "search: start a run at the beginning of the plan",
        "src/lib/search.ts",
        "  for (let step = at; step < plan.length && run.length < RUN_PAGES; step++) {",
        "  for (let step = 0; step < plan.length && run.length < RUN_PAGES; step++) {",
        "starts where it is asked to and not at the beginning",
    ),
    Mutation(
        # Pair every slot the run asked about, whatever came back. The backend
        # answers a prefix -- a reply is bounded -- so this puts page 3's hits on
        # slot 6 and counts the pages in between as searched.
        "search: pair every slot whatever came back",
        "src/lib/search.ts",
        "  for (const [step, answer] of answers.entries()) {\n    const slot = slots[step];\n    if (slot === undefined) break;",
        # **The loop variable has to be `slot`**, which the body below pushes.
        # An earlier replacement iterated `slots.entries()` while still naming
        # the pair's second element `answer`, so it redeclared the loop's own
        # binding and left `slot` undefined: a `SyntaxError` under esbuild and a
        # `ReferenceError` if it had got that far, either way a red test that
        # says nothing about pairing. `check_mutation_types.py` found it.
        "  for (const [step, slot] of slots.entries()) {\n    const answer = answers[step] ?? reply;",
        # **Re-aimed with the replacement.** It named `pairs a full run with the
        # slots it was asked about`, and that expectation was only ever met
        # vacuously: the broken replacement threw, which reddened every test in
        # the file including that one. A working replacement is noticed by the
        # test whose subject it actually is --- a reply is a prefix, so pairing
        # past its end files pages that never came back.
        "files only the pages that came back",
    ),
    Mutation(
        # Read only the first page of a run's reply. Fifteen pages in sixteen
        # would go unsearched and the walk would still advance past them, which
        # is a search that quietly covers a sixteenth of the document.
        "search: ignore the run and file only the first page",
        "src/lib/search.ts",
        "  const answers = [reply, ...(reply.more ?? [])];",
        "  const answers = [reply];",
        "pairs a full run with the slots it was asked about",
    ),
    Mutation(
        # The first-entry case, which is the only one that isolates the missing
        # slot guard from the file-page guard beside it.
        "search: include a slot with no page behind it",
        "src/lib/search.ts",
        "    if (source === undefined) break;\n    const last = run[run.length - 1];",
        "    const last = run[run.length - 1];",
        "stops at once when the first slot has no page behind it",
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
        # Put an empty pane's message back above the list, where it sat 0.4rem
        # higher and a shade darker than the other three panels' -- which is
        # what a reader reported after 26.8.10.
        "results: say it above the list even when there is nothing in the list",
        "src/lib/results.ts",
        "    this.notice.textContent = bare ? \"\" : text;",
        "    this.notice.textContent = text;",
        "draws an empty pane's message where the rows would be, not above them",
    ),
    Mutation(
        # Draw the placeholder here rather than taking it from `panelrow`. The
        # text lands in the same element and looks different, which is the whole
        # of the reader's complaint and is invisible to any check that only asks
        # where the message is.
        "results: give an empty pane's message a style of its own",
        "src/lib/results.ts",
        "      this.empty = placeholder(text);",
        "      this.empty = document.createElement(\"div\");\n      this.empty.textContent = text;",
        "draws it in the same element, with the same style, as the other panels",
    ),
    Mutation(
        # Guard on the message alone, as it was before the message could move.
        # A new query clears the list and takes the placeholder with it, so an
        # unchanged message then leaves a pane with no rows and nothing said.
        "results: decide what to redraw from the message alone",
        "src/lib/results.ts",
        "    if (text === this.said && wanted === (this.empty !== null)) return;",
        "    if (text === this.said) return;",
        "says so again when one empty result replaces another",
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
        # Re-aimed when the guard grew its second condition: the line it named
        # became `if (text === this.said && wanted === ...) return;` and the
        # anchor matched nothing. Deleting the whole guard is still the mutation
        # -- writing on every reply is 775 live-region announcements -- and the
        # sibling above it covers dropping only the new half.
        "results: write the status line on every reply",
        "src/lib/results.ts",
        "    if (text === this.said && wanted === (this.empty !== null)) return;",
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
        # **`tagged` is a `const`, so the order goes into a new binding.** An
        # earlier replacement assigned to it, which is a `TypeError` in a
        # module's strict mode: the mutation threw before ordering anything and
        # its test went red for that. The anchor reaches the `tag:` line so both
        # the ownership and the label read the same array --- sorting only the
        # first would be a third thing, and not the one the name claims.
        # `check_mutation_types.py` found it.
        "  if (tagged) {\n    return ownership(text, tagged).map((owned, at) => ({\n      tag: tagged[at]?.tag ?? null,",
        "  if (tagged) {\n    const ordered = [...tagged].sort((a, b) => a.start - b.start);\n    return ownership(text, ordered).map((owned, at) => ({\n      tag: ordered[at]?.tag ?? null,",
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
        "src/lib/popup.ts",
        "    rightOf + POPUP_WIDTH + MARGIN <= width",
        "    true",
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
    Mutation(
        # Send the note to the command that removes marks. The note is lost and
        # the mark goes with it, from a reader who was typing -- and the state
        # that comes back is a document with one fewer highlight, which is
        # exactly what the *other* button does.
        "marks: send a typed note to the removal command",
        "src/lib/edits.ts",
        '      await call("annot_note", { doc: this.doc, mark, note }),',
        '      await call("annot_remove", { doc: this.doc, mark, note }),',
        "sends the mark's own id and the whole note when one is typed",
    ),
    # --- markpopup.ts -------------------------------------------------------
    Mutation(
        # Commit on every close. Nothing on screen differs: the note says what
        # it said. What differs is the journal, which gains an entry for a
        # reader who opened a note to read it and closed it again --- so undo
        # steps over an edit nobody made, twice for every note they looked at.
        "marks: commit a note that nobody changed",
        "src/lib/markpopup.ts",
        "    const now = this.input.value;\n    if (now === this.was) return;",
        "    const now = this.input.value;",
        "sends nothing for a note that was opened and not typed in",
    ),
    Mutation(
        # Commit even when the caller said not to. The two callers that pass
        # false are a removal and a mark undone under the box; both would then
        # journal a note onto a highlight that is going, which costs the reader
        # a second undo and, for the undone one, a refusal from the model.
        "marks: commit the note of a mark that is going",
        "src/lib/markpopup.ts",
        "    if (commit) this.commit();",
        "    this.commit();",
        "sends nothing when the mark is going",
    ),
    Mutation(
        # Let a second mark take the box without committing the first. A reader
        # clicking straight from one highlight to the next loses what they
        # typed, and the box shows the new mark's text as though nothing was
        # there.
        "marks: let a second mark take the box without committing the first",
        "src/lib/markpopup.ts",
        "    if (this.shown !== null && this.shown !== mark.id) this.commit();\n",
        "",
        "commits the first mark's note when a second one takes the box",
    ),
    Mutation(
        # Open every note empty. The reader sees a blank box on a mark they
        # wrote on, and --- because the commit compares against what was filled
        # in --- typing anything then replaces what was there without warning.
        "marks: show the box empty rather than what the mark says",
        "src/lib/markpopup.ts",
        "    this.input.value = mark.note;",
        '    this.field.value = "";',
        "shows what the mark says",
    ),
    Mutation(
        # Close from inside instead of asking. The popup then disappears while
        # the viewer still believes a note is open: `markOpen` names a mark with
        # no box, and the Edit menu offers to remove it.
        "marks: close the note from inside rather than asking",
        "src/lib/markpopup.ts",
        "      this.opts.onClose();\n    });\n\n    this.input = document.createElement",
        "      this.hide();\n    });\n\n    this.input = document.createElement",
        "asks to be closed rather than closing itself",
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
        # Emit the covered characters in the order the file wrote them rather
        # than in the order the page reads. On a two-column page a highlight
        # across the gutter then lists one line from each column in turn, which
        # is not what copying the same rectangle gives.
        "reading: list a mark's covered words in the file's order",
        "src/lib/reading.ts",
        "  const order = readingOrder(text).filter((index) => wanted.has(index));",
        "  const order = [...wanted].sort((a, b) => a - b);",
        "reads a highlight across a gutter column by column, not line by line",
    ),
    Mutation(
        # Take a character whose box PDFium never placed. Four zeroes have their
        # centre at the page's top-left corner, which is inside any rectangle
        # anchored there -- and a highlight on a page's first line is one.
        #
        # It reddens two tests since 2026-08-26, one per reader of `placedBox`.
        # The second was nearly written as a `touchedText` assertion, which
        # could not have failed: `readingOrder` drops an unplaced character
        # itself, so the guard's neighbour refuses the same input. Asserting on
        # the indices is what makes this mutation reach it.
        "text: give an unplaced character to any rectangle at the page corner",
        "src/lib/text.ts",
        "  if (!(right > left) || !(bottom > top)) return null;",
        "  if (false) return null;",
        "does not take a character the page placed nowhere",
    ),
    Mutation(
        # Look words up for a comment somebody already wrote on. `rowLine`
        # discards them, so the only cost is a page extraction whose answer is
        # thrown away -- on every page of a reviewed document.
        "comments: fetch covered words for a comment that has a body",
        "src/lib/comments.ts",
        '    comment.body.trim() === "" &&',
        "    true &&",
        "does not want words for a comment somebody wrote on",
    ),
    Mutation(
        # Go looking under a kind that marks no text. A square's rectangle is
        # around a figure, so this lists it by whatever words happen to be inside.
        "comments: look for covered words under any kind of mark",
        "src/lib/comments.ts",
        "    coversText(comment.kind) &&",
        "    true &&",
        "does not want words for a kind that marks no text",
    ),
    Mutation(
        # Answer every bare comment in the document from one page's text, rather
        # than the ones on that page. Deliberately covered by a unit test rather
        # than by the window: `comments.pdf` has exactly one bare mark with
        # rectangles, so on that corpus dropping the page filter changes nothing
        # and the mutation would survive against a check that is working.
        #
        # Re-aimed 2026-08-26: the filter moved into `wantingWordsOn`, which the
        # walk calls too so that the comments it records as answered are the
        # ones the extraction answers. The page filter is still what this
        # deletes; it now deletes it for both readers at once.
        "comments: answer every page's comments from one page's text",
        "src/lib/comments.ts",
        "  return items.filter((comment) => comment.page === page && needsWords(comment));",
        "  return items.filter((comment) => needsWords(comment));",
        "asks for a page's text once and answers every comment on it",
    ),
    Mutation(
        # Ask for a page's text even when nothing on it wants words, which is
        # every page of an ordinary document.
        "comments: extract a page whose comments want no words",
        "src/lib/comments.ts",
        "  if (wanted.length === 0) return out;",
        "  if (false) return out;",
        "does not ask for a page whose comments all have bodies",
    ),
    Mutation(
        # Replace the words already known instead of merging. Each call carries
        # one page, so the panel then shows only the page that answered last.
        "commentlist: forget the pages that answered before this one",
        "src/lib/commentlist.ts",
        "    for (const [id, covered] of words) this.words.set(id, covered);",
        "    this.words.clear();\n    for (const [id, covered] of words) this.words.set(id, covered);",
        "keeps the words already known when a later page answers",
    ),
    Mutation(
        # Rebuild the list rather than rewriting the row. Correct in what it
        # shows and it drops the scroll position and the focused element under
        # a reader, once per page that answers.
        "commentlist: repaint the whole list when a page's words arrive",
        "src/lib/commentlist.ts",
        "      const body = bodyOf(this.elements.get(comment.id));",
        "      this.paint();\n      const body = bodyOf(this.elements.get(comment.id));",
        "rewrites the row rather than rebuilding the list",
    ),
    Mutation(
        # Keep one document's covered words across an open. Ids start again with
        # each document, so they land on whatever holds that id next -- and read
        # perfectly plausibly.
        "commentlist: carry covered words into the next document",
        "src/lib/commentlist.ts",
        "    this.words.clear();\n    this.focused = null;",
        "    this.focused = null;",
        "does not carry one document's words onto the next document's rows",
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
        "  return item.rect[1] < at.top;",
        "  return item.rect[1] <= at.top;",
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
        "src/lib/text.ts",
        "  return x >= left && x <= right && y >= top && y <= bottom;",
        "  return true;",
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
        # Re-aimed 2026-08-26: the guard it deletes moved out of
        # `centreOfCharacter` into `placedBox`, which the redaction panel's own
        # reader needs the same answer from. Both guards go, and they have to:
        # with only the `undefined` one removed, an out-of-range read is four
        # zeroes and the extent guard below refuses it anyway --- a mutation that
        # changes nothing, which is the fourth way one can lie.
        "links: read a character box past the end of the array",
        "src/lib/text.ts",
        "  if (\n    left === undefined ||\n    top === undefined ||\n    right === undefined ||\n    bottom === undefined\n  ) {\n    return null;\n  }\n  if (!(right > left) || !(bottom > top)) return null;\n  return { left, top, right, bottom };",
        "  return { left: left ?? 0, top: top ?? 0, right: right ?? 0, bottom: bottom ?? 0 };",
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
        "      const offset =\n"
        "        this.scroller.effectiveTurns(page) === 0 ? Math.max(0, place.top) : 0;\n"
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
    Mutation(
        # Send the slot instead of the page's id. The two are the same number on
        # a document nobody has rearranged, which is every document until
        # somebody moves a page -- and then a highlight lands on whichever page
        # took that slot, at the coordinates of the words it was made from.
        #
        # **Re-aimed 2026-08-22.** It named `page: id,` in the request, which was
        # the line that translated a slot into an id; the parameter is a `PageId`
        # now and there is no translation left to break, so the equivalent edit
        # is to put the lookup back. The anchor gate is what said so, on the
        # commit that removed it.
        # **Widened 2026-08-26**, when `Edits.redact` was written with the same
        # guard and the anchor started matching twice. The `invoke` line below
        # is what tells the two doors apart; the mutation itself is unchanged.
        "edits: send a mark's slot rather than its page id",
        "src/lib/edits.ts",
        "    if (!this.current.pages.some((view) => view.id === page)) return this.current;\n"
        "    return this.adopt(\n"
        '      await call("annot_mark", {',
        "    const at = this.current.pages[page as number];\n"
        "    if (!at) return this.current;\n"
        "    page = at.id;\n"
        "    return this.adopt(\n"
        '      await call("annot_mark", {',
        "sends the page's id rather than its slot when a mark is made",
    ),
    Mutation(
        # Mark a region for a page the model no longer has. `Edits.mark`'s guard
        # for the redaction door, and the damage is worse: a region is placed by
        # coordinates, so this puts a marking on whatever page the backend
        # resolves and offers it to the reader for review as if they had drawn
        # it there.
        "edits: mark a region without checking the page is still there",
        "src/lib/edits.ts",
        "    if (!this.current.pages.some((view) => view.id === page)) return this.current;\n"
        "    return this.adopt(\n"
        '      await call("redact_mark", { doc: this.doc, page, area }),',
        "    return this.adopt(\n"
        '      await call("redact_mark", { doc: this.doc, page, area }),',
        "does not send a redaction for a page the model has never mentioned",
    ),
    Mutation(
        # Spend the redaction tool on its first region. It was one-shot until
        # 2026-10-07 and stays armed now, because a reader with a dozen things
        # to take out armed it a dozen times. The crop's twin below still holds
        # the other half: the two were disarmed by one line.
        "viewer: spend the redaction tool on its first region",
        "src/lib/viewer.ts",
        # Re-aimed 2026-10-16 when the region to copy or save joined the crop's drag.
        '        if (this.tool.kind === "crop" || this.tool.kind === "place" || this.tool.kind === "region") {\n'
        "          this.tool = NO_TOOL;\n"
        "        }",
        '        if (this.tool.kind === "crop" || this.tool.kind === "redact" || this.tool.kind === "place" || this.tool.kind === "region") {\n'
        "          this.tool = NO_TOOL;\n"
        "        }",
        "stays armed for the next region, like the eraser, until Escape",
    ),
    Mutation(
        # The armed tool has two gestures and the press decides which. Without
        # this branch a press on the words drags a rectangle, as it did.
        "viewer: drag a rectangle when the armed redaction tool is pressed on text",
        "src/lib/viewer.ts",
        "    if (this.tool.kind === \"redact\" && this.pressOnText(event)) {",
        "    if (false) {",
        "marks the words dragged across and stays armed",
    ),
    Mutation(
        # And the other way: every press selects, so blank paper has no gesture.
        "viewer: select text when the armed redaction tool is pressed on blank paper",
        "src/lib/viewer.ts",
        "    if (this.tool.kind === \"redact\" && this.pressOnText(event)) {",
        "    if (this.tool.kind === \"redact\") {",
        "drags a rectangle when pressed on blank paper",
    ),
    Mutation(
        # Mark every selection, armed or not: a reader selecting to copy would
        # mark what they selected.
        "viewer: mark a selection made with no tool armed",
        "src/lib/viewer.ts",
        "    if (this.tool.kind === \"redact\" && this.hasSelection) this.opts.onRedactSelection?.();",
        "    if (this.hasSelection) this.opts.onRedactSelection?.();",
        "selects without marking when the tool is not armed",
    ),
    Mutation(
        # Ask for an empty selection to be marked, which is what a click is.
        "viewer: mark the nothing a click selects",
        "src/lib/viewer.ts",
        "    if (this.tool.kind === \"redact\" && this.hasSelection) this.opts.onRedactSelection?.();",
        "    if (this.tool.kind === \"redact\") this.opts.onRedactSelection?.();",
        "marks nothing for a click that selects nothing",
    ),
    Mutation(
        # The cursor is the only thing that says which gesture a press starts.
        "viewer: show the crosshair over text with the redaction tool armed",
        "src/lib/viewer.ts",
        "      : this.tool.kind === \"redact\" && this.overRedactText\n        ? \"text\"",
        "      : false\n        ? \"text\"",
        "shows the text cursor over words and the crosshair over paper",
    ),
    Mutation(
        # Call every point of a page with text on it text, which is what
        # `nearestChar` answers and why the tool cannot ask it.
        "text: call the margin text",
        "src/lib/text.ts",
        "    if (dx <= ON_TEXT_REACH && dy <= ON_TEXT_REACH) return true;",
        "    if (dx <= 1e9 && dy <= 1e9) return true;",
        "is false in the margin and between two lines",
    ),
    Mutation(
        # No reach at all: a press between two letters of a word starts a rectangle.
        "text: give a press beside a letter no reach",
        "src/lib/text.ts",
        "    if (dx <= ON_TEXT_REACH && dy <= ON_TEXT_REACH) return true;",
        "    if (dx <= 0 && dy <= 0) return true;",
        "is true on a character and just beside one",
    ),
    Mutation(
        # A damaged stored value becomes the fill: a redaction in a colour nobody chose.
        "redact fill: trust whatever is stored",
        "src/lib/redactfill.ts",
        "    return isFill(kept) ? kept : DEFAULT_FILL;",
        "    return (kept as Fill | null) ?? DEFAULT_FILL;",
        "reads as black when what is stored is not one of the three",
    ),
    Mutation(
        # The note is the only thing that says what white does.
        "redact fill: say nothing about white",
        "src/lib/redactfill.ts",
        "  return fill === \"white\"",
        "  return fill === \"red\"",
        "says what white means, and nothing for the other two",
    ),
    Mutation(
        # The backend fills black when it is told nothing, so this reads as working.
        "redact fill: send black for a copy whatever was chosen",
        "src/lib/edits.ts",
        "    return await call(\"redact_copy\", { doc: this.doc, source, path, fill });",
        "    return await call(\"redact_copy\", { doc: this.doc, source, path, fill: \"black\" });",
        "sends the colour of the boxes with both redactions",
    ),
    Mutation(
        # And the same for the document itself.
        "redact fill: send black for the document whatever was chosen",
        "src/lib/edits.ts",
        "    return await call(\"redact_document\", { doc: this.doc, source, fill });",
        "    return await call(\"redact_document\", { doc: this.doc, source, fill: \"black\" });",
        "sends the colour of the boxes with both redactions",
    ),
    Mutation(
        # Tell the owner of a choice that changed nothing.
        "redact fill: report the colour already chosen as a change",
        "src/lib/redactfill.ts",
        "    if (fill === this.chosen) return;\n",
        "",
        "opens on the kept colour and tells its owner of a new one, once",
    ),
    Mutation(
        # Leave the note up after white is no longer the choice.
        "redact fill: keep the note about white for every colour",
        "src/lib/redactfill.ts",
        "    const note = fillNote(this.chosen);",
        "    const note = fillNote(\"white\");",
        "shows the note while white is chosen and takes it away afterwards",
    ),
    Mutation(
        # Take the redaction out of the selection's right-click menu, which
        # sends the reader back to the toolbar's menu for every passage.
        "context menu: do not offer the redaction on a selection",
        "src/lib/contextmenu.ts",
        '  "edit.redactSelection",\n  "edit.selectAll",',
        '  "edit.selectAll",',
        "offers the redaction straight after the three marks",
    ),
    Mutation(
        # Add the vertical part of a sideways swipe on top of the strip's own
        # scrolling, so a trackpad moves the tabs twice.
        "tab wheel: move the strip for a swipe it already scrolls for",
        "src/lib/tabwheel.ts",
        "  if (Math.abs(wheel.deltaX) >= Math.abs(wheel.deltaY)) return 0;",
        "  if (false) return 0;",
        "leaves a sideways swipe to the strip itself",
    ),
    Mutation(
        # Read a wheel that counts lines as one that counts pixels: three
        # pixels a notch, which reads as a wheel that does nothing.
        "tab wheel: count a line as a pixel",
        "src/lib/tabwheel.ts",
        "  if (wheel.deltaMode === LINES) return wheel.deltaY * LINE_PX;",
        "  if (wheel.deltaMode === LINES) return wheel.deltaY;",
        "counts lines and pages in pixels",
    ),
    Mutation(
        "tab wheel: count a page as a line",
        "src/lib/tabwheel.ts",
        "  if (wheel.deltaMode === PAGES) return wheel.deltaY * width;",
        "  if (wheel.deltaMode === PAGES) return wheel.deltaY * LINE_PX;",
        "counts lines and pages in pixels",
    ),
    Mutation(
        "tab wheel: turn the wheel the other way",
        "src/lib/tabwheel.ts",
        "  return wheel.deltaY;\n}",
        "  return -wheel.deltaY;\n}",
        "turns a vertical turn into the same distance sideways",
    ),
    # The four below replace five rows deleted 2026-09-06. Each of those mutated
    # one arming method's clearing of the other tools' flags, and there are no
    # other flags: arming is one assignment over one `tool` field, so the state
    # they produced --- two tools live at once --- is not expressible and the
    # mutation would be a permanent red line. `docs/TRAPS.md` records the rule
    # under *A guard the type system already makes unexpressible has no mutation
    # to write*. What replaces them is the strongest edit the type still permits:
    # arming the WRONG tool, one row per arming method.
    Mutation(
        # Arm the redaction tool when the reader asked for a crop. The two
        # share a `PointerDrag` and differ only in what a released rectangle
        # commits, so this is the confusion that destroys a page the reader
        # meant to hide part of.
        "viewer: let armCrop arm the redaction tool instead",
        "src/lib/viewer.ts",
        '    this.inking = null;\n    this.tool = { kind: "crop" };',
        '    this.inking = null;\n    this.tool = { kind: "redact" };',
        "arming crop leaves every other tool disarmed",
    ),
    Mutation(
        # The mirror, and the direction that loses work rather than hiding it:
        # a reader marking a region for removal gets a crop.
        "viewer: let armRedact arm the crop instead",
        "src/lib/viewer.ts",
        '    this.inking = null;\n    this.tool = { kind: "redact" };',
        '    this.inking = null;\n    this.tool = { kind: "crop" };',
        "arming redact leaves every other tool disarmed",
    ),
    Mutation(
        # Take up the eraser when a drawing tool was chosen. The next press
        # rubs out what it crosses instead of drawing.
        "viewer: let armDraw arm the eraser instead",
        "src/lib/viewer.ts",
        '    this.tool = { kind: "draw", mark: kind, stamp };',
        '    this.tool = { kind: "erase" };',
        "arming draw leaves every other tool disarmed",
    ),
    Mutation(
        # And the eraser arming a pen, which is the one that stays armed: a
        # reader who reached for the eraser draws a box on every press until
        # they press Escape.
        "viewer: let armErase arm a drawing tool instead",
        "src/lib/viewer.ts",
        '    this.inking = null;\n    this.tool = { kind: "erase" };',
        '    this.inking = null;\n    this.tool = { kind: "draw", mark: "square", stamp: null };',
        "arming erase leaves every other tool disarmed",
    ),
    Mutation(
        # Send a dragged region to the crop instead. The two tools share one
        # drag and this branch is the only thing separating them, so getting it
        # backwards crops the page a reader asked to redact -- with an undo they
        # have to know to reach for and nothing on screen saying what happened.
        "viewer: commit a redaction drag as a crop",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-27 when the signature placement joined the crop's drag.
        "        else if (marking) this.opts.onRedacted?.(id, rect);\n"
        "        else this.opts.onCropped?.(id, rect);",
        "        else this.opts.onCropped?.(id, rect);",
        "sends a dragged region to the redaction callback and not the crop's",
    ),
    Mutation(
        # Shade the same area for both tools. The gesture still works and the
        # right thing still happens -- what goes is the only feedback telling a
        # reader which of the two they armed, on a pair of commands that differ
        # by whether the words survive.
        "markband: shade a redaction the way a crop is shaded",
        "src/lib/markband.ts",
        "  if (marking) return [[kept.x0, kept.y0, kept.x1 - kept.x0, kept.y1 - kept.y0]];",
        "  if (false) return [[kept.x0, kept.y0, kept.x1 - kept.x0, kept.y1 - kept.y0]];",
        "shades the rectangle for a redaction and everything but it for a crop",
    ),
    Mutation(
        # Cut the crop's side bands the full height of the page rather than
        # between the top and bottom ones. Every pixel outside the rectangle is
        # still shaded, so the preview looks almost right -- the two corners
        # where bands overlap draw twice, and a translucent scrim there reads as
        # a darker cross the reader takes for part of the gesture.
        "markband: overlap the crop's side bands with its top and bottom",
        "src/lib/markband.ts",
        "    [page.x0, kept.y0, kept.x0 - page.x0, kept.y1 - kept.y0],\n"
        "    [kept.x1, kept.y0, page.x1 - kept.x1, kept.y1 - kept.y0],",
        "    [page.x0, page.y0, kept.x0 - page.x0, page.y1 - page.y0],\n"
        "    [kept.x1, page.y0, page.x1 - kept.x1, page.y1 - page.y0],",
        "cuts the crop's bands so none of them overlaps another",
    ),
    Mutation(
        # Send the redaction's position in the list rather than its identity.
        # Correct for every reader who has removed nothing, and off by one for
        # every reader who has -- which is precisely the case a review list is
        # for.
        "edits: address a redaction by its position rather than its id",
        "src/lib/edits.ts",
        '      await call("redact_remove", { doc: this.doc, redaction }),',
        '      await call("redact_remove", {\n'
        "        doc: this.doc,\n"
        "        redaction: this.current.redactions.findIndex((r) => r.id === redaction),\n"
        "      }),",
        "sends the redaction's own id when one is taken back off",
    ),
    Mutation(
        # Merge each reply's marks into the cache instead of replacing it. Every
        # visible behaviour is identical until an undo, which is the one moment
        # the cache has to shrink -- and a mark that stays on screen after being
        # undone is the failure undo exists to prevent.
        "edits: merge the marks a reply carries into the ones already held",
        "src/lib/edits.ts",
        "    this.current = joined;",
        "    this.current = {\n      ...joined,\n      marks: [...this.current.marks, ...joined.marks],\n    };",
        "carries the marks a reply brought, and drops the ones it did not",
    ),
    Mutation(
        # Offer the highlight command with nothing selected. It appears in the
        # palette, it is selectable, and choosing it sends a mark with no quads
        # -- which the model refuses, so the reader gets an error message for
        # pressing a button the application offered them.
        "appcommands: offer the highlight with nothing selected",
        "src/lib/appcommands.ts",
        "      id: \"edit.highlightSelection\",\n      title: \"Highlight selection\",\n"
        "      enabled: () => withDocument() && actions.hasSelection(),",
        "      id: \"edit.highlightSelection\",\n      title: \"Highlight selection\",\n"
        "      enabled: withDocument,",
        "is withheld with nothing selected, and offered once there is",
    ),
    Mutation(
        # The same guard on the *third* entry, because the three are near-copies
        # and a check bound to one caller covers only that caller -- which is a
        # trap this repository has already paid for. The two mutations together
        # say the test walks all three rather than stopping at the first.
        "appcommands: offer the strikeout with nothing selected",
        "src/lib/appcommands.ts",
        "      id: \"edit.strikeoutSelection\",\n      title: \"Strike out selection\",\n"
        "      enabled: () => withDocument() && actions.hasSelection(),",
        "      id: \"edit.strikeoutSelection\",\n      title: \"Strike out selection\",\n"
        "      enabled: withDocument,",
        "is withheld with nothing selected, and offered once there is",
    ),
    Mutation(
        # And the argument rather than the guard: three entries differing only
        # in a string, which is the copy-and-paste this file's `movePage` note
        # is about. A reader who chooses Underline gets a highlight, and every
        # check that asks whether the command ran passes.
        "appcommands: run the underline as a highlight",
        "src/lib/appcommands.ts",
        "      run: () => actions.markSelection(\"underline\"),",
        "      run: () => actions.markSelection(\"highlight\"),",
        "is withheld with nothing selected, and offered once there is",
    ),
    Mutation(
        # Answer the first slot for every mark id. Correct on a document nobody
        # has rearranged and wrong the moment one page moves, which is exactly
        # the case this translation exists for.
        "pages: answer the first slot for any mark id",
        "src/lib/pages.ts",
        "  slotOfId(id: PageId): number | undefined {\n    for (let slot = 0; slot < this.views.length; slot++) {\n      if (this.views[slot]?.id === id) return slot;\n    }\n    return undefined;\n  }",
        "  slotOfId(id: PageId): number | undefined {\n    return this.views.length > 0 ? 0 : undefined;\n  }",
        "finds the slot a page identity is showing in",
    ),
    Mutation(
        # The defect this whole increment is about, at the one place it now
        # lives. Six call sites held this line before `turnsOn` collected them,
        # so the mutation that used to be needed six times is needed once --
        # which is the argument for the primitive rather than the rule.
        #
        # It names the *control* rather than one of the eight comparisons, and
        # that is the whole lesson of collapsing three copies into one: marks,
        # comments and links now agree by construction, so a fault in the
        # primitive moves all three together and every comparison stays green.
        # This was measured, not reasoned about -- the mutation reddened eight
        # checks before `viewQuadsOf` was routed through here and two after.
        "viewer: place a page's rectangles under the view's turn alone",
        "src/lib/viewer.ts",
        "      turns: this.scroller.effectiveTurns(page),\n      width_pt: size.width_pt,",
        "      turns: this.turns,\n      width_pt: size.width_pt,",
        "puts the rectangle somewhere else once the page is turned",
    ),
    Mutation(
        # The header's word, which named a highlight when a highlight was the
        # only mark there was.
        "markpopup: call every mark a highlight",
        "src/lib/markpopup.ts",
        "    this.title.textContent = NAMES[mark.kind];",
        "    this.title.textContent = NAMES.highlight;",
        "names the kind of mark it is open on",
    ),
    Mutation(
        # The button rather than the header. Two labels, two mutations: a box
        # that names the mark correctly and offers "Remove highlight" under it
        # is the half a single assertion on the header would miss.
        "markpopup: offer to remove a highlight whatever is open",
        "src/lib/markpopup.ts",
        '    this.remove.textContent = `Remove ${NAMES[mark.kind].toLowerCase()}`;',
        '    this.remove.textContent = "Remove highlight";',
        "names the kind of mark it is open on",
    ),
    Mutation(
        # Write the labels once, when the box is built, rather than when a mark
        # takes it over. Correct for the first mark opened and wrong for every
        # one after it, which is the state a reader is in within two clicks.
        "markpopup: label the box once rather than per mark",
        "src/lib/markpopup.ts",
        "    this.title.textContent = NAMES[mark.kind];\n"
        "    this.remove.textContent = `Remove ${NAMES[mark.kind].toLowerCase()}`;",
        "",
        "relabels itself when a mark of another kind takes the box",
    ),
    Mutation(
        # One colour for all three kinds, which is what the table replaced. A
        # 1.3 pt yellow rule on white paper is close to invisible, so a reader
        # presses Underline and nothing appears to happen.
        "markcolors: give every kind the wash's colour",
        "src/lib/markcolors.ts",
        "  underline: [0.85, 0.15, 0.15],",
        "  underline: [1, 0.9, 0.2],",
        "sends each kind with its own colour",
    ),
    Mutation(
        # And the other direction, which the control cannot see: a rectangle
        # that still moves when the page turns, just to the wrong place. Caught
        # by the absolute half of the differential -- a rectangle on a page has
        # to be found *within* that page, and a quarter too far puts it 130 pt
        # off the bottom of a page the turn made 600 pt tall.
        "viewer: turn every rectangle a quarter too far",
        "src/lib/viewer.ts",
        "      turns: this.scroller.effectiveTurns(page),\n      width_pt: size.width_pt,",
        "      turns: this.scroller.effectiveTurns(page) + 1,\n      width_pt: size.width_pt,",
        "places a comment where a mark with the same rectangle is, at 1 turns",
    ),
    Mutation(
        # The same line against the link half, which is a different subsystem
        # reaching the same primitive: `linksOn` turns its rectangles through
        # `turnsOn` while `commentUnder` turns its own.
        "viewer: place a link under the view's turn alone",
        "src/lib/viewer.ts",
        "  private turnsOn(page: number): { turns: number; width_pt: number; height_pt: number } {",
        "  private turnsOn(page: number): { turns: number; width_pt: number; height_pt: number } {\n    if (this.linkCount > 0) return { ...this.scroller.pageSize(page), turns: this.turns };",
        "places a link where a mark with the same rectangle is, at 1 turns",
    ),
    Mutation(
        # Read the links memo by the view's turn. It hits when it should miss,
        # so a page turned under a warm cache is hit-tested against the
        # rectangles it had before the turn.
        "viewer: look the links memo up by the view's turn",
        "src/lib/viewer.ts",
        "    if (cached && cached.page === page && cached.turns === turns) {",
        "    if (cached && cached.page === page && cached.turns === this.turns) {",
        "does not serve a link's old rectangle out of the cache after a turn",
    ),
    Mutation(
        # And write it by the view's turn, which is the half a one-way test
        # cannot reach: the poisoned entry is only read back after the turn is
        # undone. Two mutations because the key has two ends and only one of
        # them is exercised by turning a page once.
        "viewer: store the links memo under the view's turn",
        "src/lib/viewer.ts",
        "    this.turnedLinks = { page, turns, items };",
        "    this.turnedLinks = { page, turns: this.turns, items };",
        "does not serve a link's old rectangle out of the cache after a turn",
    ),
    Mutation(
        # A destination on a page an edit turned, scrolled down an axis that is
        # no longer vertical. The rotated *view* has followed the opposite rule
        # since it was written; this is the same rule reaching the same page by
        # the other route.
        "viewer: scroll into a turned page as though the view were upright",
        "src/lib/viewer.ts",
        "    const offset = this.scroller.effectiveTurns(clamped) === 0 ? (top ?? 0) : 0;",
        "    const offset = this.turns === 0 ? (top ?? 0) : 0;",
        "lands a destination on a turned page rather than partway down it",
    ),
    Mutation(
        # The number `position` reports goes into the history and the session,
        # so an offset down a turned page is what Back and a restart land on.
        "viewer: report an offset down a page an edit turned",
        "src/lib/viewer.ts",
        "    if (this.scroller.effectiveTurns(page) !== 0) return { page, top: 0 };",
        "    if (this.turns !== 0) return { page, top: 0 };",
        "reports no offset within a page an edit turned",
    ),
    Mutation(
        # The one that does not correct itself: a size is learned once, so a
        # page turned before it was ever on screen keeps a transposed size for
        # the life of the document.
        "viewer: learn a turned page's size without removing its turn",
        "src/lib/viewer.ts",
        "        size: displayedSize(shown, -this.scroller.effectiveTurns(page)),",
        "        size: displayedSize(shown, -this.turns),",
        "learns a page's size in the document's space, not the turned view's",
    ),
    Mutation(
        # Back to what shipped: the button number alone. On macOS a Control+click
        # is a right-click reported as `button: 0` with `ctrlKey` set, so this is
        # the defect a reader found --- the selection replaced with an empty one
        # at the caret, and the context menu opening on nothing a moment later.
        "viewer: guard the press on the button number alone",
        "src/lib/viewer.ts",
        "    if (event.button !== 0 || (isMac() && event.ctrlKey)) return;",
        "    if (event.button !== 0) return;",
        "keeps the selection when a Mac reader Control+clicks",
    ),
    Mutation(
        # The same edit, aimed at the second thing the early return buys: with
        # the guard gone the press also binds `pointermove`, so a reader holding
        # a menu over their selection has it extended under them.
        "viewer: let a Control+click start a drag on a Mac",
        "src/lib/viewer.ts",
        "    if (event.button !== 0 || (isMac() && event.ctrlKey)) return;",
        "    if (event.button !== 0) return;",
        "binds no drag to a Control+click on a Mac",
    ),
    Mutation(
        # Treat Ctrl+click as a right-click everywhere. Reasonable-looking, and
        # wrong on Windows, where it is an ordinary modified press that must
        # still start a selection --- a Mac gesture rule applied to a machine
        # that does not use it.
        "viewer: refuse a Control+click on every platform",
        "src/lib/viewer.ts",
        "    if (event.button !== 0 || (isMac() && event.ctrlKey)) return;",
        "    if (event.button !== 0 || event.ctrlKey) return;",
        "starts a selection when a Windows reader Control+clicks",
    ),
    Mutation(
        # Drop the older half of the guard, which was correct from the day it
        # was written and had no test until the modifier half gained one. This
        # is the two-finger tap, and it working throughout is why the defect
        # read as a broken menu rather than a broken press.
        "viewer: let the second button clear the selection",
        "src/lib/viewer.ts",
        "    if (event.button !== 0 || (isMac() && event.ctrlKey)) return;",
        "    if (isMac() && event.ctrlKey) return;",
        "keeps the selection when the second button is pressed",
    ),
    Mutation(
        # Put a page tpdf made into the file-page map. It has no baseline number,
        # so the slot becomes its key --- and every file page after it resolves to
        # the wrong slot, which is the direction everything arriving from the
        # backend is translated in.
        "pages: put a page tpdf made into the file-page translation",
        "src/lib/pages.ts",
        "      const source = view === undefined ? undefined : baselineOf(view.source);\n      if (source === undefined || this.bySource.has(source)) continue;",
        "      const source =\n        view === undefined ? undefined : (baselineOf(view.source) ?? slot);\n      if (source === undefined || this.bySource.has(source)) continue;",
        "is in no direction of the file-page translation",
    ),
    Mutation(
        # Answer with the slot's own page and never look back. A reader whose
        # place is a page tpdf made then has no place at all, and the fallback
        # one level up quietly uses the slot number instead.
        "pages: stop the nearest-file-page walk at the slot it was asked about",
        "src/lib/pages.ts",
        "    for (let at = Math.min(slot, this.views.length - 1); at >= 0; at--) {",
        "    for (let at = Math.min(slot, this.views.length - 1); at >= slot; at--) {",
        "answers with the page before it when a file page is what is needed",
    ),
    Mutation(
        # Leave a made page out of the source list. Every slot after it then
        # reads as the page before it, which is the misalignment the list exists
        # to make visible.
        "pages: leave a page tpdf made out of the source list",
        "src/lib/pages.ts",
        "    return this.views.map((page) => baselineOf(page.source));",
        "    return this.views.flatMap((page) => baselineOf(page.source) ?? []);",
        "reports itself in a source list rather than being left out of it",
    ),
    Mutation(
        # Stop the strip at a page tpdf made. Nothing pumps the row after it ---
        # the request it did not make has no reply --- so every row below a blank
        # page stays empty for the life of the document.
        "thumbnails: stop the strip at a page tpdf made",
        "src/lib/thumbnails.ts",
        "      this.paintBlank(page);\n      this.blank.add(page);\n      this.pump();\n      return;",
        "      this.paintBlank(page);\n      this.blank.add(page);\n      return;",
        "asks for no tile for a page tpdf made, and carries on past it",
    ),
    Mutation(
        # Collapse the two `undefined`s. An absent callback and one answering
        # `undefined` mean different things, and this asks a worker for whatever
        # page of the file happens to sit at the made page's slot number.
        "thumbnails: fall back to the slot for a page with no file page behind it",
        "src/lib/thumbnails.ts",
        "    const source = this.opts.addressOf\n      ? this.opts.addressOf(page)\n      : { doc: this.opts.doc, page: filePage(page) };",
        "    const source = this.opts.addressOf?.(page) ?? { doc: this.opts.doc, page: filePage(page) };",
        "asks for no tile for a page tpdf made, and carries on past it",
    ),
    Mutation(
        # Leave the surround showing through a page tpdf made. Nothing else will
        # ever draw it, so the reader sees a grey hole where they inserted a
        # sheet of paper.
        "scroller: leave the surround showing through a page tpdf made",
        "src/lib/scroller.ts",
        "  if (!view || !madeSizeOf(view.source)) return null;",
        "  if (!view || madeSizeOf(view.source)) return null;",
        "fills a page tpdf made rather than leaving the surround through it",
    ),
    Mutation(
        # White under inversion too, which is a blank sheet between two black
        # ones --- the one thing an inverted document must not draw.
        "scroller: fill a page tpdf made white under inversion as well",
        "src/lib/scroller.ts",
        '  return invert ? "#000000" : "#ffffff";',
        '  return "#ffffff";',
        "fills a page tpdf made rather than leaving the surround through it",
    ),
    Mutation(
        # Lay a made page out at the running estimate. It is never rendered, so
        # nothing corrects it: the page keeps the mean of the other pages' sizes
        # for the life of the document.
        "scroller: lay a page tpdf made out at the estimate rather than its own size",
        "src/lib/scroller.ts",
        "      const made = madeSizeOf(page.source);",
        "      const made = undefined as ReturnType<typeof madeSizeOf>;",
        "lays a page tpdf made out at the size the reply carried",
    ),
    Mutation(
        # Collapse a stale slot into an insert at the front. A press that arrives
        # after the order changed then puts a page somewhere nobody asked for,
        # which is the distinction `insertPage` is written around.
        "edits: turn a stale slot into an insert at the front",
        "src/lib/edits.ts",
        "    size: readonly [number, number],\n  ): Promise<EditState> {\n    const anchor =\n      after === null ? null : (this.current.pages[after]?.id ?? undefined);",
        "    size: readonly [number, number],\n  ): Promise<EditState> {\n    const anchor = after === null ? null : (this.current.pages[after]?.id ?? null);",
        "does not send an insert for a slot the model has never mentioned",
    ),
]

#: Suites this harness runs. Named once: `run_tests` and the name check below
#: must agree, or the second validates a list the first never runs.
# --- the keyboard route to a mark ----------------------------------------
ROWS += [
    Mutation(
        # Let every key through while the reader is typing. This is the shipped
        # defect the increment fixed: before the guard, "n" turned the page under
        # the note box, Home jumped to the start and the space bar scrolled it
        # away. Aimed at the guard's *call*, not at `inTextField` itself --- a
        # predicate everything agrees about and nothing consults is the trap this
        # repository records as a guard only covered when a mutation removes its
        # call.
        "viewer: act on a key that went to the note box",
        "src/lib/viewer.ts",
        '    if (inTextField(event) || nativeCopy(event)) return;',
        '    if (false) return;',
        "scroll the page when they came from the page and not when they did not",
    ),
    Mutation(
        # The other direction: refuse *every* key, which passes the refusal half
        # of each pair above and fails its control. Without this, a guard that
        # made the viewer deaf would be indistinguishable from one that works.
        "viewer: refuse every key, whether or not it went to a field",
        "src/lib/viewer.ts",
        '    if (inTextField(event) || nativeCopy(event)) return;',
        '    return;',
        "scroll the page when they came from the page and not when they did not",
    ),
    Mutation(
        # Throw on an event whose target has gone, rather than answering "no".
        # The safe half is that the viewer acts, which is what it did before this
        # guard existed; an exception takes the whole handler down.
        "keys: assume every event has a target",
        "src/lib/keys.ts",
        "  if (!target) return false;",
        "",
        "says no for an event with no target rather than throwing",
    ),
    Mutation(
        # Order marks by the id of the page they name rather than by the slot it
        # is in. Identical on an unedited document, and the reverse of the right
        # answer the moment a reader moves a page --- which is the whole reason
        # this is a translation and not a sort.
        "pages: walk marks by page id rather than by the slot it is in",
        "src/lib/pages.ts",
        "    walk.push({ id: mark.id, page: slot, rect });",
        "    walk.push({ id: mark.id, page: mark.page, rect });",
        "orders marks by the slot their page is in, not by its id",
    ),
    Mutation(
        # Take the first rectangle instead of the union. A one-line highlight is
        # unaffected; one across three lines takes its place in the walk from
        # whichever line the model emitted first.
        "pages: place a multi-line mark by its first rectangle",
        "src/lib/pages.ts",
        "    const rect = unionOf(mark.quads);",
        "    const first = mark.quads.slice(0, 4);\n    const rect =\n      first.length === 4\n        ? ([first[0] ?? 0, first[1] ?? 0, first[2] ?? 0, first[3] ?? 0] as [\n            number,\n            number,\n            number,\n            number,\n          ])\n        : null;",
        "takes the union of a mark's rectangles, not its first one",
    ),
    Mutation(
        # Leave the order partial where two marks share a top edge --- a reader
        # marking one line twice. Which is "next" then depends on the sort's
        # stability rather than on a rule. The same mutation `orderedLinks` has.
        "pages: leave the mark order partial for two marks on one line",
        "src/lib/pages.ts",
        "    if (a.rect[1] !== b.rect[1]) return a.rect[1] - b.rect[1];\n    return a.id - b.id;",
        "    return a.rect[1] - b.rect[1];",
        "is a total order for two marks with the same top edge",
    ),
    Mutation(
        # Keep a mark whose page the reader deleted, at slot -1. It sorts to the
        # front of the walk and opens a note anchored to nothing.
        "pages: keep a mark whose page is gone",
        "src/lib/pages.ts",
        "    if (slot === undefined) continue;\n    const rect = unionOf(mark.quads);",
        "    const rect = unionOf(mark.quads);",
        "leaves out a mark whose page is gone",
    ),
    Mutation(
        # Always start the walk at the top of the document rather than at where
        # the reader is looking. The difference `stepAlong` exists for: on a
        # 775-page document "next mark" would mean "the first one, again".
        "viewer: start the mark walk at the document rather than at the reader",
        "src/lib/viewer.ts",
        "    const next = stepAlong(walk, from, this.position, direction);",
        "    const next = stepAlong(walk, from, { page: 0, top: 0 }, direction);",
        "starts from where the reader is looking, not from the first mark",
    ),
    Mutation(
        # Take the keyboard into the note field on every route in, the walk
        # included. The next press of the walk key then goes to the field, where
        # the guard correctly refuses it --- so the reader reaches one mark and is
        # stranded on it.
        "viewer: take the keyboard into the note when the walk opens it",
        "src/lib/viewer.ts",
        "    this.showMark(next.id, false);",
        "    this.showMark(next.id, true);",
        "opens the next mark's note without taking the keyboard off the page",
    ),
    Mutation(
        # Wrap at the end of the walk instead of stopping, and say nothing. The
        # trap about a wrap being correct when there is nothing ahead.
        "viewer: wrap the mark walk at the end",
        "src/lib/viewer.ts",
        "    if (!next) {\n      this.opts.onError?.(\n        direction === 1 ? \"No further marks.\" : \"No earlier marks.\",\n      );\n      return false;\n    }",
        "    if (!next) {\n      this.showMark(walk[0]?.id ?? -1, false);\n      return true;\n    }",
        "stops at each end rather than wrapping, and says so",
    ),
    Mutation(
        # Say nothing on a document the reader has not marked. A key that does
        # nothing and reports nothing is indistinguishable from a broken one.
        "viewer: walk an unmarked document silently",
        "src/lib/viewer.ts",
        "      this.opts.onError?.(\"You have not marked anything in this document.\");",
        "",
        "says so on a document the reader has not marked",
    ),
    Mutation(
        # Do not scroll to a mark that is off screen. Its note box then clamps
        # itself into view and points at nothing --- the reason `showComment` has
        # had these three lines since it was written.
        "viewer: open a mark's note without scrolling to the mark",
        "src/lib/viewer.ts",
        "    if (where.bottom < 0 || where.top > height) {\n      this.goToDestination(\n        this.pages.slotOfId(mark.page) ?? 0,\n        this.markTopPt(mark),\n      );\n    }",
        "",
        "scrolls to a mark that is off screen",
    ),
    Mutation(
        # Swallow Enter whether or not a note is open. Nothing visible happens ---
        # the popup's own guard refuses the focus --- and the key never reaches
        # the link arm below it, so a focused cross-reference stops working the
        # moment the reader has marked anything.
        "viewer: take Enter for the note whether or not a note is open",
        "src/lib/viewer.ts",
        "    } else if (event.key === \"Enter\" && this.markNote.openId !== null) {",
        "    } else if (event.key === \"Enter\") {",
        "still reaches the focused link when no note is open",
    ),
    Mutation(
        # Focus the note field on Enter whether or not one is open, which focuses
        # a `display:none` element and takes the arrow keys off the page.
        "markpopup: focus the note field even when no note is open",
        "src/lib/markpopup.ts",
        "  focusField(): void {\n    if (this.shown === null) return;",
        "  focusField(): void {",
        "puts the keyboard in the note, and does nothing when none is open",
    ),
]

# --- cropping a page -----------------------------------------------------
ROWS += [
    Mutation(
        # Move the origin and leave the far corner, which is the shape of a
        # translation written for a point rather than a rectangle: every
        # highlight then grows as the page is cropped.
        "crop: move only a rectangle's origin into the crop",
        "src/lib/crop.ts",
        "    rect[2] - at.left,\n    rect[3] - at.top,",
        "    rect[2],\n    rect[3],",
        "moves a rectangle by the crop's corner, both edges of each axis",
    ),
    Mutation(
        # Add where it should subtract. A crop then moves everything the wrong
        # way by twice the offset, which on a small crop still looks like a
        # rectangle on the page.
        "crop: move rectangles the wrong way into the crop",
        "src/lib/crop.ts",
        "  return [\n    rect[0] - at.left,\n    rect[1] - at.top,",
        "  return [\n    rect[0] + at.left,\n    rect[1] + at.top,",
        "moves a rectangle by the crop's corner, both edges of each axis",
    ),
    Mutation(
        # Use the horizontal offset for every coordinate. Right whenever the two
        # happen to be equal, which is a crop inset by the same amount on the
        # left as at the top -- and that is what a symmetric fixture looks like.
        "crop: use one offset for both axes on the way out",
        "src/lib/crop.ts",
        "    moved.push((quads[index] ?? 0) + (index % 2 === 0 ? at.left : at.top));",
        "    moved.push((quads[index] ?? 0) + at.left);",
        "alternates the two offsets rather than applying one of them",
    ),
    Mutation(
        # An uncropped page whose geometry says it starts somewhere. Everything
        # on every ordinary document moves.
        "crop: give an uncropped page a corner that is not the origin",
        "src/lib/crop.ts",
        "  return { width_pt, height_pt, left: 0, top: 0 };",
        "  return { width_pt, height_pt, left: 1, top: 1 };",
        "moves a rectangle nowhere on a page nobody cropped",
    ),
    Mutation(
        # Place a mark without the crop, which is the drift the differential is
        # written for: the comment subsystem and the mark subsystem then put one
        # rectangle in two places.
        "viewer: place a mark without applying the page's crop",
        "src/lib/viewer.ts",
        "    return viewRect(intoCrop(rect, this.cropAt(page)), turns, width_pt, height_pt);",
        "    return viewRect(rect, turns, width_pt, height_pt);",
        "places a comment and a mark at the same point, and not where they were",
    ),
    Mutation(
        # Keep the geometry after the crop is cleared. Every rectangle on the
        # page stays shifted, on a page that is no longer cropped -- the failure
        # that looks least like the thing that caused it.
        "viewer: keep a page's crop geometry after the crop is cleared",
        "src/lib/viewer.ts",
        "        if (held) {\n          this.crops.delete(view.id);",
        "        if (held) {\n          void held;",
        "takes the geometry back off when the crop is cleared",
    ),
    Mutation(
        # Lay the cropped page out at the crop rectangle's own width and height
        # rather than the size the backend reported. Identical on an unrotated
        # page and transposed at every quarter turn.
        "viewer: lay a cropped page out without asking how big it is",
        "src/lib/viewer.ts",
        "      if (this.scroller.notePageSize(slot, size)) {",
        "      if (false) {",
        "lays the page out at the size the backend reported",
    ),
    Mutation(
        # Apply one page's crop to every page. The document reads as though the
        # reader cropped all of it, and on a uniform corpus that is invisible.
        "viewer: answer every page with the crop of the one that has one",
        "src/lib/viewer.ts",
        "    const known = id === undefined ? undefined : this.crops.get(id);",
        "    const known = [...this.crops.values()][0];",
        "leaves an uncropped page alone",
    ),
    Mutation(
        # Serve a page's characters from an extraction made under another crop
        # box. They are not stale, they are in another space -- so the caret and
        # every highlight land by the crop's offset.
        "text: keep a page's characters when its crop changes",
        "src/lib/text.ts",
        "    if (crop === undefined) this.crops.delete(page);\n    else this.crops.set(page, crop);\n    this.forget(page);",
        "    if (crop === undefined) this.crops.delete(page);\n    else this.crops.set(page, crop);",
        "re-asks for a page whose crop changed, and sends the box",
    ),
    Mutation(
        # Drop every page rather than the one whose crop moved, which is correct
        # and re-extracts the whole visible document on every state reply.
        "text: drop every page when one page's crop changes",
        "src/lib/text.ts",
        "    this.forget(page);\n  }\n\n  /** Drops one page's extraction, and everything derived from it. */",
        "    for (const held of [...this.pages.keys()]) this.forget(held);\n  }\n\n  /** Drops one page's extraction, and everything derived from it. */",
        "keeps its neighbours when one page's crop changes",
    ),
    Mutation(
        # Extract without the crop the page carries. The boxes come back in the
        # file's space while everything drawn over them is in the cropped one.
        "text: extract a cropped page without its crop",
        "src/lib/text.ts",
        "      crop: this.crops.get(page) ?? null,",
        "      crop: null,",
        "re-asks for a page whose crop changed, and sends the box",
    ),
]

# --- rubbing a drawing out -----------------------------------------------
ROWS += [
    Mutation(
        # Measure to the nearest recorded point instead of the nearest segment.
        # A fast hand leaves points far apart, so the eraser passes straight
        # through the middle of a long stroke without touching it -- and every
        # short stroke still erases, which is what makes it look like it works.
        "markband: measure the eraser to the nearest point on the stroke",
        "src/lib/markband.ts",
        "  const nx = a.x + along * dx;\n  const ny = a.y + along * dy;",
        "  const nx = a.x;\n  const ny = a.y;",
        "measures to the nearest segment, not to the nearest recorded point",
    ),
    Mutation(
        # Drop the clamp, so the distance is to the infinite line the segment
        # sits on. The eraser then takes a stroke it passed a long way clear of,
        # along that stroke's own direction.
        "markband: let the eraser reach along the line a segment sits on",
        "src/lib/markband.ts",
        "    length === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));",
        "    length === 0 ? 0 : ((p.x - a.x) * dx + (p.y - a.y) * dy) / length;",
        "does not reach along the line the segment sits on",
    ),
    Mutation(
        # Ignore the radius the caller passed and use the constant. The viewer
        # divides by the zoom, so this makes the nib the wrong size at every
        # zoom but 100% -- and at 100% every other check here still passes.
        "markband: give the eraser a nib of its own size whatever it was asked for",
        "src/lib/markband.ts",
        "  const within = radius * radius;",
        "  const within = ERASER_RADIUS * ERASER_RADIUS;",
        "uses the radius it is given",
    ),
    Mutation(
        # Answer for a stroke of one point by refusing it. It is input the model
        # never keeps, which is exactly why nothing else would notice.
        "markband: refuse to measure a stroke of a single point",
        "src/lib/markband.ts",
        "    return pointToSegment(first, from, to) <= within;",
        "    return false;",
        "answers for a stroke of one point rather than refusing it",
    ),
    Mutation(
        # Test the point the nib is at instead of the ground it covered. A
        # pointer reports at the display's rate, so a quick sweep then passes
        # over whole strokes and leaves them --- which is how this was found:
        # a drag down a column of three took the outer two.
        "viewer: sweep the eraser where it is rather than where it has been",
        "src/lib/viewer.ts",
        "    const from = swept.last;",
        "    const from = { x, y };",
        "takes several when the sweep crosses several, in one report",
    ),
    Mutation(
        # Drop the crossing test. Two long segments that cross at their middles
        # are at distance zero with all four endpoints far apart, so the nib
        # goes straight through the stroke it is aimed at.
        "markband: let the nib pass through a stroke it crosses",
        "src/lib/markband.ts",
        "    if (segmentsCross(a, b, from, to)) return true;",
        "    if (false) return true;",
        "takes a stroke it crosses in the middle, with every end far away",
    ),
    Mutation(
        # Report the strokes in the order the hand crossed them. Nothing
        # downstream requires the sort, which is why only a test that sweeps
        # upwards can see it -- and a diagnostic quoting the list then reads
        # differently for two readers who erased the same thing.
        "viewer: report an erased stroke in the order the hand reached it",
        "src/lib/viewer.ts",
        "              [...strokes].sort((a, b) => a - b),",
        "              [...strokes],",
        "reports the strokes in order however the hand crossed them",
    ),
    Mutation(
        # Escape reaches only the pen's states, which is what it did when the
        # eraser was written: a reader with the eraser armed and nothing on
        # screen but a cursor cannot get out of the mode.
        "viewer: let Escape past an armed eraser",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # The eraser's terms are no longer adjacent, so the whole flattened
        # condition is the anchor and the after drops every state but the pen's.
        '        this.tool.kind !== "none" ||\n'
        "        this.drawing ||\n"
        "        this.inking ||\n"
        "        this.doomed ||\n"
        "        this.cropDrawing",
        '        this.tool.kind === "draw" ||\n'
        "        this.drawing ||\n"
        "        this.inking ||\n"
        "        this.cropDrawing",
        "sends nothing when Escape ends the sweep",
    ),
    Mutation(
        # Treat every mark as a drawing. A highlight then loses whichever of
        # its (non-existent) strokes the nib crossed and stays on the page, so
        # the eraser silently does nothing to every kind but ink -- which is
        # exactly what it did before the whole-mark sweep landed, and the shape
        # a reader reports as "the eraser does not work".
        "viewer: treat every mark as a drawing in the eraser's sweep",
        "src/lib/viewer.ts",
        "      if (!isPath(mark.kind)) {\n        const placed = this.viewQuadsOf(mark);",
        "      if (false) {\n        const placed = this.viewQuadsOf(mark);",
        "takes a highlight whole, and rubs no stroke out of it",
    ),
    Mutation(
        # Take the mark only when the nib STOPS inside it. A sweep straight
        # across a highlight -- which is the gesture, and is how anybody would
        # use a rubber -- then leaves it there, and the tool works only if the
        # reader remembers to lift the pointer on top of what they meant.
        "markband: let the nib take a mark only where it comes to rest",
        "src/lib/markband.ts",
        "  // Closed: the last point repeats the first, so the left edge is a segment\n"
        "  // like the other three rather than the gap between the ends of an open line.\n"
        "  return strokeSwept(",
        "  return false && strokeSwept(",
        "takes one the nib crosses without stopping inside it",
    ),
    Mutation(
        # Drop the containment test, so only the rectangle's EDGE takes a mark.
        # A press in the middle of a large box then does nothing at all, which
        # reads as an eraser that misses whatever it is aimed at.
        "markband: measure the nib against a mark's edge and not its area",
        "src/lib/markband.ts",
        "  if (\n    from.x >= quad.left &&\n    from.x <= quad.right &&\n"
        "    from.y >= quad.top &&\n    from.y <= quad.bottom\n  ) {\n    return true;\n  }",
        "  if (false) {\n    return true;\n  }",
        "takes a mark the nib is pressed on, without a drag",
    ),
    Mutation(
        # Leave the rectangle's outline open, so its left edge is not a segment
        # at all. Everything works except a nib arriving from the left, which is
        # the direction a right-handed reader sweeps from.
        "markband: leave a mark's outline open at the corner it started from",
        "src/lib/markband.ts",
        "      { x: quad.left, y: quad.bottom },\n      { x: quad.left, y: quad.top },\n    ],",
        "      { x: quad.left, y: quad.bottom },\n    ],",
        "takes one the nib enters from outside it",
    ),
    Mutation(
        # A nib of no width. Every mark the sweep passes *near* survives, so the
        # eraser demands a direct hit -- which for a one-point-wide box border
        # is a gesture nobody can make.
        "viewer: give the eraser's nib no width when it looks for a mark",
        "src/lib/viewer.ts",
        "    const nib = ERASER_RADIUS / this.zoom;",
        "    const nib = 0;",
        "takes one the nib passes within its own width of",
    ),
    Mutation(
        # Take every mark on the page whether the nib went near it or not. One
        # sweep anywhere clears the page, and the reader has one undo per mark
        # to put it back.
        "viewer: take every mark on the page the sweep started on",
        "src/lib/viewer.ts",
        "        if (placed.quads.some((quad) => quadSwept(quad, from, to, nib))) {",
        "        if (true) {",
        "leaves one the nib passes clear of",
    ),
    Mutation(
        # Say nothing about the marks a sweep took whole. The preview shows them
        # going, the pointer comes up, and they are all back -- because nothing
        # ever reached the model.
        "viewer: keep the marks a sweep took whole to itself",
        "src/lib/viewer.ts",
        "          for (const mark of swept.whole) this.opts.onUnmarked?.(mark, sweep);",
        "          for (const mark of swept.whole) void mark;",
        "takes a mark the nib is pressed on, without a drag",
    ),
    Mutation(
        # Report no whole marks in the live count. The status line says "drag
        # across a mark" while three of them are already gone from the page.
        "viewer: leave whole marks out of what the sweep says it has taken",
        "src/lib/viewer.ts",
        "    return { strokes, marks: this.doomed?.whole.size ?? 0 };",
        "    return { strokes, marks: 0 };",
        "counts marks and strokes apart while the sweep is live",
    ),
    Mutation(
        # Call every taken thing a stroke. A reader who swept up two highlights
        # is told they took two strokes, which names a kind of mark they did not
        # touch and cannot find.
        "markband: report the marks a sweep took as strokes",
        "src/lib/markband.ts",
        '    parts.push(`${taken.marks} mark${taken.marks === 1 ? "" : "s"}`);',
        '    parts.push(`${taken.marks} stroke${taken.marks === 1 ? "" : "s"}`);',
        "leaves out the half that is zero",
    ),
    Mutation(
        # Print both halves always, so the ordinary sweep reads "3 strokes, 0
        # marks" -- a clause about nothing, in a line that comes and goes.
        "markband: report the half of a sweep that took nothing",
        "src/lib/markband.ts",
        "  if (taken.marks > 0) {",
        "  if (taken.marks >= 0) {",
        "leaves out the half that is zero",
    ),
]

# --- Back and Forward grey when there is nowhere to go ---------------------
ROWS += [
    Mutation(
        # Say nothing when a jump records a place. Back becomes live and the
        # menu is never told, so the item stays greyed with somewhere to go --
        # which is the state it shipped in until the guards existed at all, and
        # a stale grey is a route the reader cannot take.
        "viewer: keep a recorded jump from the window",
        "src/lib/viewer.ts",
        "      this.history.push(this.position);",
        "      this.history.push(this.position);\n      if (false)",
        "announces a jump, which is what makes Back live",
    ),
    Mutation(
        # Say nothing when a new document empties the history. Back stays live
        # in the menu on a file nobody has jumped in, and pressing it does
        # nothing -- the mirror of the mutation above, and the one a reader
        # meets every time they open a second file.
        "viewer: keep a cleared history from the window",
        "src/lib/viewer.ts",
        "    this.history.clear();\n    // And say so, or Back stays live in the menu on a document with nowhere",
        "    this.history.clear();\n    if (false)\n    // And say so, or Back stays live in the menu on a document with nowhere",
        "announces a new document emptying the history",
    ),
    Mutation(
        # Offer Back whenever a document is open, which is what both commands
        # did until 2026-08-23: the menu offers it on a document nobody has
        # jumped in and the press does nothing at all.
        "appcommands: offer Back with nowhere to go back to",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && (actions.viewer()?.canGoBack ?? false),",
        "      enabled: withDocument,",
        "withholds both on a document nobody has jumped in",
    ),
    Mutation(
        # Ask Back's question for Forward as well. After a jump Back is live and
        # Forward is not, so one predicate for both offers a Forward that goes
        # nowhere -- the case a single "has a history" flag cannot express.
        "appcommands: ask Back's question for Forward",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && (actions.viewer()?.canGoForward ?? false),",
        "      enabled: () => withDocument() && (actions.viewer()?.canGoBack ?? false),",
        "offers Back once there is somewhere to go, and still withholds Forward",
    ),
    Mutation(
        # Reach through a closed document to a remembered answer. With no viewer
        # there is nothing to ask, and `?? true` offers both commands on an
        # empty window.
        "appcommands: assume a jump is available when there is no document",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && (actions.viewer()?.canGoBack ?? false),",
        "      enabled: () => actions.viewer()?.canGoBack ?? true,",
        "withholds both with no document, whatever a stale history would say",
    ),
]
