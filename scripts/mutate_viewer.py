#!/usr/bin/env python3
"""Breaks the application's command wiring on purpose, one edit at a time.

`mutate_rust.py` and `mutate_frontend.py` drive `cargo test` and `vitest`. The
command list and the window shortcuts are covered by neither: they are asserted
by `viewer_check.py`, which needs a built bundle and a real webview. So this is
the third harness, and it exists for the same reason as the other two -- a check
that has only ever passed looks exactly like one that cannot fail, and the
command list was covered by nothing at all until 2026-08-01.

It inherits the properties `AGENTS.md` records the absence of:

**A run with no summary line is a broken run, not a survivor.** A crash, a
timeout, a bad mutation that fails to build and an occluded window all produce
no `[FAIL]` lines, which is exactly what a surviving mutation looks like.

**The failure count is derived two ways** -- by counting `[FAIL]` lines and by
reading the summary's own arithmetic -- and a disagreement is reported as a
broken run rather than as either answer.

**It refuses to start if a mutation names a check the suite does not define.**
The names come from a clean baseline run, and a name that cannot go red reports
SURVIVED and reads as a gap in the checks rather than as a typo here.

**Expected names are matched as a prefix of the line after the marker**, never
by slicing the padded name column. The width is `checkreport.ts`'s, and is 46
today; it was 40 while `viewercheck.ts` carried its own copy of the reporter,
and the two having drifted a column apart unnoticed is the reason this file
encodes neither. A name *longer* than the width is followed by
a single space, so a column parse silently stops matching the day a check gets a
long name -- which has happened here once already, in the direction that looks
like good news. `after_marker` strips the label and whatever spacing follows it,
which is what `session_check.py` does and is indifferent to both widths.

**Files are restored by bytes and verified by digest**, not by `write_text`,
whose locale codec rewrites every line ending on Windows and compares equal
afterwards because both sides read through the same translation.

Usage:
    scripts/mutate_viewer.py                     # every mutation
    scripts/mutate_viewer.py --list
    scripts/mutate_viewer.py --only "Cmd-K"      # substring of the name
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WINDOWS = sys.platform == "win32"

#: The built application the viewer runner drives.
#:
#: macOS needs a **bundle** --- WKWebView will not run a line of JavaScript
#: without the bundle identity, and the failure is a blank window and silence.
#: Windows has no such requirement, so the plain executable is the thing, and
#: `npm run tauri build` produces it on the way to the installers.
APP = (
    ROOT / "src-tauri/target/release/tpdf.exe"
    if WINDOWS
    else ROOT / "src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf"
)

#: Where the vendored PDFium sits, which is not the same directory on both
#: platforms --- see the trap of that name. The probe runners take it as an
#: argument, so getting it wrong here is a probe that cannot load a document
#: and a mutation that reports a broken run.
PDFIUM_DIR = "vendor/pdfium/bin" if WINDOWS else "vendor/pdfium/lib"

FIXTURE = ROOT / "testdata/outline-simple.pdf"
#: The corpus the tagged-reading-order checks need. On every other fixture they
#: `[SKIP]`, correctly and uselessly for a mutation: a check that skips cannot go
#: red, so a mutation aimed at one would report SURVIVED against a harness that
#: never ran it.
TAGGED_FIXTURE = ROOT / "testdata/tagged.pdf"

# Bound on one `viewer_check.py` run, matching `viewer_sweep.py`'s default. It
# exists so that a hang ends as a failure with a transcript instead of stopping
# the harness for as long as nobody looks.
CHECK_TIMEOUT = 420

#: The corpus with a page whose fonts state no character mapping. The three
#: checks about it assert the *presence* of a warning there and its *absence*
#: everywhere else, so a mutation aimed at one of them on any other fixture is
#: aimed at the branch that is not taken.
ENCODINGS_FIXTURE = ROOT / "testdata/encodings.pdf"

#: The only corpus whose pages are not all the same size, which is what makes
#: "a page carries its measured size to wherever it moved" observable at all.
#: On a uniform document the right rule and the wrong one produce the same
#: layout --- the check says so and skips, and a mutation aimed at it on any
#: other fixture is aimed at a check that cannot go red.
MIXED_FIXTURE = ROOT / "testdata/mixed.pdf"


@dataclass(frozen=True)
class Mutation:
    """One edit, and the check whose job it is to notice."""

    name: str
    path: str
    before: str
    after: str
    expect: str
    #: Which harness runs it. "viewer" is `viewer_check.py` against a bundle,
    #: "viewer-tagged" the same against the tagged corpus, and "structure" and
    #: "search" are `structure-probe` and `search-probe`, which need no webview at
    #: all --- so neither waits for a bundle nor requires an unlocked screen. All
    #: of them print
    #: the same `[FAIL] name` lines and the same summary, so everything below ---
    #: the cross-check, the byte restore, the name validation --- is shared.
    runner: str = "viewer"


MUTATIONS = [
    Mutation(
        "a11y: announce every heading at one level",
        "src/lib/a11y.ts",
        "  if (heading) return `h${heading[1]}`;",
        '  if (heading) return "h1";',
        "a heading is announced as a heading, at the document's own level",
        runner="viewer-tagged",
    ),
    Mutation(
        # The other direction, and the one the first check cannot see: it asks
        # only that the headings the tags wanted are present.
        "a11y: announce every block as a heading",
        "src/lib/a11y.ts",
        "  return tag === \"H\" ? \"h2\" : \"p\";",
        '  return "h1";',
        "nothing the document did not call a heading becomes one",
        runner="viewer-tagged",
    ),
    Mutation(
        # The check that reads the tree's text has to see every element, not the
        # ones it was written before. A selector naming `p` reads a tagged page
        # short by its headings --- and presents that as the page's text being
        # wrong rather than the selector.
        # Re-aimed 2026-08-16. Its anchor was `joined = texts.filter(...).join(" ")`,
        # a line commit 9e9be98 removed when links were announced as links, and
        # nothing noticed for as long as this harness was not completing a run.
        # The separator now goes in per range, so this is the same edit at the
        # line that took over the job.
        "a11y: hand a paragraph's lines over as separate paragraphs",
        "src/lib/a11y.ts",
        '      if (breaks.has(rangeAt)) piece += " ";',
        "      if (false) piece += \" \";",
        # Aimed at the TEXT rather than the order: dropping the separator merges
        # two lines into one word and changes no block's position, so the
        # order check it named cannot see it.
        "the text read out is the page's own text",
        runner="viewer-tagged",
    ),
    Mutation(
        "structure: show the geometry's order to a screen reader",
        "src/lib/reading.ts",
        "  const tagged = usableRuns(text);",
        "  const tagged = null;",
        "the accessibility tree is built in the order the tags give",
        runner="viewer-tagged",
    ),
    Mutation(
        # The control on the fixture rather than on the code: if the two orders
        # ever stopped differing, the check above would pass without a tag being
        # read. Mutating the *geometric* side proves the comparison is live.
        "structure: compare the tagged order against itself",
        "src/lib/viewercheck.ts",
        "  const geometric = lines({ ...text, runs: [] });",
        "  const geometric = lines(text);",
        "a tagged page's reading order is not the one its geometry gives",
        runner="viewer-tagged",
    ),
    Mutation(
        "Cmd-K reaches no arm at all",
        "src/lib/appcommands.ts",
        '  if ((event.metaKey || event.ctrlKey) && event.key === "k") {',
        "  if (false) {",
        "Cmd-K opens the palette",
    ),
    Mutation(
        "Cmd-K needs no modifier",
        "src/lib/appcommands.ts",
        '  if ((event.metaKey || event.ctrlKey) && event.key === "k") {',
        '  if (event.key === "k") {',
        "a bare k does not open the palette",
    ),
    Mutation(
        "anticlockwise turns clockwise",
        "src/lib/appcommands.ts",
        "      run: () => actions.viewer()?.rotateBy(-1),",
        "      run: () => actions.viewer()?.rotateBy(1),",
        "view.rotateCounterClockwise runs from the palette",
    ),
    Mutation(
        "a command leaves the list",
        "src/lib/appcommands.ts",
        """    {
      id: "view.showThumbnails",
      title: "Show page thumbnails",
      enabled: withDocument,
      run: () => actions.showTab("pages"),
    },
""",
        "",
        "every registered command is classified",
    ),
    Mutation(
        "print is wired to the open dialog",
        "src/lib/appcommands.ts",
        "      run: () => actions.printDocument(),",
        "      run: () => actions.openDocument(),",
        "file.print runs from the palette",
    ),
    Mutation(
        "Zoom to ignores what was typed",
        "src/lib/appcommands.ts",
        "          if (zoom !== null) actions.viewer()?.setZoomFixed(zoom);",
        "          if (zoom !== null) actions.viewer()?.setZoomFixed(1);",
        "view.zoomTo runs from the palette",
    ),
    Mutation(
        "Cmd-F opens find with no document",
        "src/lib/appcommands.ts",
        '  } else if (matches("find.open", event) && title) {',
        '  } else if (matches("find.open", event)) {',
        "Cmd-P prints with no document open",
    ),
    Mutation(
        "Cmd-O stacks file dialogs",
        "src/lib/appcommands.ts",
        "    if (!actions.busyOpening()) actions.openDocument();",
        "    actions.openDocument();",
        "Cmd-O opens one dialog at a time",
    ),
    Mutation(
        "zoom in is offered with no document",
        "src/lib/appcommands.ts",
        """      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
      enabled: withDocument,
""",
        """      id: "view.zoomIn",
      title: "Zoom in",
      keys: label("view.zoomIn"),
""",
        "with no document only the commands needing none are offered",
    ),
    Mutation(
        "a page break is not looked across at all",
        "src-tauri/src/search.rs",
        "    let tail = carry_for(text, page, query, options);",
        "    let tail = None;",
        "a phrase is found across a page break",
    ),
    Mutation(
        "the page break is not whitespace",
        "src-tauri/src/search.rs",
        """        if index == split {
            items.push((BREAK, '\\n'));
        }
""",
        "",
        "a phrase is found across a page break",
    ),
    Mutation(
        "a hit that starts outside the selection is kept",
        "src/lib/search.ts",
        "            m.start >= startsIn.from &&",
        "            true &&",
        "a scoped search looks only inside the selection",
    ),
    Mutation(
        "a hit that ends outside the selection is kept",
        "src/lib/search.ts",
        "            m.end <= endsIn.to",
        "            true",
        "a scoped search looks only inside the selection",
    ),
    Mutation(
        "a broken pattern reports no problem",
        "src-tauri/src/search.rs",
        "            problem: Some(problem),",
        "            problem: None,",
        "a pattern that does not compile says so instead of finding nothing",
    ),
    Mutation(
        # The defect the corpus was built to look for, seen from the far end: the
        # unit tests in `text.rs` cover the pairing rule, and this covers what a
        # reader actually experiences --- a query for an Extension B ideograph
        # finding nothing on a page that plainly shows one.
        "an astral code point is two characters",
        "src-tauri/src/text.rs",
        "    match next {\n        Some(low) if (0xDC00..0xE000).contains(&low) => {",
        "    match None::<u32> {\n        Some(low) if (0xDC00..0xE000).contains(&low) => {",
        "query astral-alone: hit count",
        "search",
    ),
    Mutation(
        # `encodings.pdf` is the only fixture that reaches the replacement path at
        # all: it needs a `/ToUnicode` mapping a CID to a lone surrogate, which no
        # correct document contains. Until it existed this was covered by unit tests
        # alone, and this mutation is what says the end-to-end path agrees with them.
        "a lone surrogate is dropped rather than replaced",
        "src-tauri/src/text.rs",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (REPLACEMENT, 1);\n    }",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (0x3F, 1);\n    }",
        "query a-lone-surrogate-becomes-a-replacement: hit count",
        "encodings",
    ),
    Mutation(
        # The other half, on the corpus where two broken entries pair. Aimed here as
        # well as at `multilingual.pdf` on purpose: there the pair comes from a
        # correct CMap, and here from two unrelated characters whose broken mappings
        # happen to form one --- the same code, reached two ways.
        "a broken pair is not joined",
        "src-tauri/src/text.rs",
        "        Some(low) if (0xDC00..0xE000).contains(&low) => {",
        "        Some(low) if false && (0xDC00..0xE000).contains(&low) => {",
        "query two-broken-entries-can-pair: hit count",
        "encodings",
    ),
    Mutation(
        # A mutation only a non-Latin corpus can catch, which is the point of
        # having one: with `is_word` restricted to ASCII, Kanji stop being word
        # characters, so the whole-word search that correctly finds nothing inside
        # a Japanese run starts finding something. Every Latin fixture agrees with
        # both spellings of the rule.
        "only ASCII letters are word characters",
        "src-tauri/src/search.rs",
        "    ch.is_alphanumeric() || ch == '_'",
        "    ch.is_ascii_alphanumeric() || ch == '_'",
        "query cjk-substring-whole-word: hit count",
        "search",
    ),
    Mutation(
        "a paragraph's generated gaps are not bridged",
        "src-tauri/src/structure.rs",
        "            Some(last) if bridgeable(last.1 as usize, span.0 as usize) => last.1 = span.1,",
        "            Some(_) if false => {}",
        "page 1: every run's characters are a block's text",
        "structure",
    ),
    Mutation(
        "the tree's roots are walked backwards",
        "src-tauri/src/structure.rs",
        "    for index in 0..roots.max(0) {",
        "    for index in (0..roots.max(0)).rev() {",
        "page 1: the order is the one the tags give",
        "structure",
    ),
    Mutation(
        "every element reports no type",
        "src-tauri/src/structure.rs",
        "        let tag = self.type_of(element);",
        "        let tag = String::new();",
        "page 1: each run carries its element's type",
        "structure",
    ),
    Mutation(
        "every character is asked about the first text object",
        "src-tauri/src/structure.rs",
        "unsafe { bindings.FPDFText_GetTextObject(text.handle(), index as i32) }",
        "unsafe { bindings.FPDFText_GetTextObject(text.handle(), 0) }",
        "page 1: every run's characters are a block's text",
        "structure",
    ),
    Mutation(
        "the phase does not put the rotation back",
        "src/lib/viewercheck.ts",
        "  viewer.rotateBy(entry.rotation - viewer.rotation);",
        "",
        "leaves the viewer as the phase before it did",
    ),
    Mutation(
        # The frontend hop of the encoding path. `encoding.rs` is covered by
        # `mutate_rust.py`, which says the *rule* is right; nothing said the
        # answer reached a reader until this corpus was opened in a window.
        "encoding: report no unsearchable page whatever the backend said",
        "src/lib/search.ts",
        "    return this.mapping.filter((page) => page.guessing > 0).length;",
        "    return 0;",
        "the pages whose text is a guess reach the frontend",
        runner="viewer-encodings",
    ),
    Mutation(
        # Aimed at the *control*, and it has to be: on `encodings.pdf` a panel
        # that says the line unconditionally is indistinguishable from one that
        # says it correctly. Only a document with nothing to report can tell,
        # which is why the check runs on every corpus rather than skipping.
        "encoding: say a page could not be searched on every document",
        "src/lib/results.ts",
        "  if (unsearchablePages > 0) {",
        "  if (unsearchablePages >= 0) {",
        "a reader is told when a page could not be searched",
    ),
    Mutation(
        # Hand the next phase a document with a sideways first page. The
        # analogue of "the phase does not put the rotation back", and it has to
        # be aimed at this phase's OWN restore assertion: the cross-phase check
        # named "leaves the viewer as the phase before it did" runs at the end of
        # the palette phase, which is well before this one, so it could not see a
        # turn left behind here however it were worded.
        "page turn: hand the next phase a page still turned",
        "src/lib/viewercheck.ts",
        # Narrowed to the FIRST put-back. The phase gained a second
        # `setPageTurns(target, 0)` when the half-turn check was added, and a
        # bare anchor then matched twice -- caught by the `anchors` gate rather
        # than by a run.
        "  viewer.setPageTurns(target, 0);\n  await frame();\n  const restored",
        "  await frame();\n  const restored",
        "turning a page back restores the layout",
    ),
    Mutation(
        # Turn the whole VIEW instead of the one page. The defect the three
        # negative checks exist for: every positive statement about the turned
        # page --- its shape, its discarded tiles, its sideways text --- is
        # equally true of this, so written with only the positive half the phase
        # would pass.
        "page turn: rotate the whole view instead of the one page",
        "src/lib/viewer.ts",
        "    if (source !== undefined) this.text.setPageTurns(source, turns);\n    this.scroller.setPageTurns(page, turns);",
        "    this.rotateBy(turns);",
        "a page nobody turned keeps its shape",
    ),
    Mutation(
        # The text layer left behind. Tiles turn, the caret does not, and it
        # reads as selection being slightly wrong on one page rather than as a
        # missing feature.
        "page turn: leave the text layer upright when a page turns",
        "src/lib/viewer.ts",
        "    if (source !== undefined) this.text.setPageTurns(source, turns);",
        "",
        "the text layer turns with the page",
    ),
    Mutation(
        # Invalidate only when the box moves. A half turn moves no box, so the
        # stale pixels stay on screen --- which is why `setPageTurns` invalidates
        # before it touches the geometry rather than relying on `applySizes`.
        #
        # **Credited to the half-turn check, and it named the quarter-turn one
        # until 2026-08-17.** A quarter turn changes the page box, so `applySizes`
        # invalidates the page whether the explicit call is there or not --- which
        # is the whole reason the half-turn check was written, in the commit that
        # added this expectation and did not move it. The run reported SURVIVED
        # and printed the check that *had* gone red, which is what made a
        # mis-aimed expectation readable rather than a hole in the suite.
        "page turn: invalidate a turned page only when its box moves",
        "src/lib/scroller.ts",
        "    this.pageTurns[page] = next;\n    this.invalidatePage(page);",
        "    this.pageTurns[page] = next;",
        "a half turn discards the pixels its box did not move",
    ),
    Mutation(
        # Take the new order and leave the page count where it was. The document
        # on screen is one page shorter and every count --- the status line, the
        # page strip, the accessibility tree --- still says otherwise.
        "page delete: keep the page count the document had",
        "src/lib/viewer.ts",
        "    this.opts.pageCount = after.length;",
        "    this.opts.pageCount = Math.max(after.length, this.opts.pageCount);",
        "deleting a page leaves the document one page shorter",
    ),
    Mutation(
        # Keep what was painted. The tiles are placed by the slot they were
        # requested for, so every page below the gap is drawn with the pixels of
        # the page that used to be above it.
        "page delete: keep the tiles across a change of order",
        "src/lib/scroller.ts",
        "    this.clearTiles();\n    this.dropPlaceholders();\n    this.estimate = this.meanKnownSize();",
        "    this.estimate = this.meanKnownSize();",
        "deleting a page discards what was painted",
    ),
    Mutation(
        # Put the reader back on the slot number they were on rather than on the
        # page they were reading. Deleting a page above them then scrolls the
        # document under them by one page.
        "page delete: keep the reader on the slot rather than on the page",
        "src/lib/viewer.ts",
        "      after.slotFrom(before, wasAt) ??",
        "      undefined ??",
        "the reader stays on the page they were reading",
    ),
    Mutation(
        # The half of a reorder that a page count cannot see. A page's measured
        # size is read out of the old array *by the slot the page used to be
        # in*; forgetting it hands every page the estimate, which is the mean of
        # a document whose pages are different sizes and therefore fits none of
        # them. Only observable on `mixed.pdf` --- everywhere else the estimate
        # and the truth are the same number, which is why this mutation has its
        # own runner rather than riding on the default corpus.
        "page move: forget every page's measured size when the order changes",
        "src/lib/scroller.ts",
        "      sizes.push(was === undefined ? null : (this.sizes[was] ?? null));",
        "      sizes.push(null);",
        "the moved page and the one it displaced keep their sizes",
        "viewer-mixed",
    ),
    Mutation(
        # No press ever travels far enough, so the strip is a list of pictures
        # that cannot be rearranged. Aimed at the window rather than at the unit
        # suite on purpose: the frontend tests drive a fake DOM, and what this
        # asks is whether a real WKWebView delivers the moves at all.
        "page drag: put the drag threshold past any distance a pointer travels",
        "src/lib/thumbnails.ts",
        "const DRAG_THRESHOLD = 6;",
        "const DRAG_THRESHOLD = 100000;",
        "dragging a thumbnail asks for the slot it was dropped on",
        "viewer-mixed",
    ),
    Mutation(
        # The other direction, and the one a reader meets first: every click on
        # a thumbnail becomes a drag, so looking at a page rearranges the
        # document. This is what the control check is for, and without it the
        # mutation above is satisfied by a strip that reorders on any press.
        "page drag: treat a press that never moved as a drag",
        "src/lib/thumbnails.ts",
        "      if (Math.abs(event.clientY - press.startY) < DRAG_THRESHOLD) return;",
        "      if (false) return;",
        "a press that does not travel asks for nothing",
        "viewer-mixed",
    ),
    Mutation(
        # The third symptom, and the one whose reader can least easily tell
        # something is wrong: the guessed characters read aloud as though they
        # were the page.
        "encoding: read a guessed page's characters out anyway",
        "src/lib/a11y.ts",
        "    if (unreadable) {",
        "    if (false) {",
        "a page whose characters mean nothing is not read out",
        runner="viewer-encodings",
    ),
]

MARKER = re.compile(r"^\[(OK|FAIL|SKIP)\]\s+")
SUMMARY = re.compile(r"^(\d+)/(\d+) checks passed")


def after_marker(line: str) -> str:
    """The check's name and detail, with the label and its spacing removed.

    Every producer -- `checkreport.ts` and the two Rust probes -- happens to put
    the name at column 7 today, and that is exactly the coincidence not to build
    on: the label widths are three separate literals in three files. Matching the
    marker instead means a wider label is a formatting change rather than a
    parser that quietly stops finding anything.
    """
    found = MARKER.match(line)
    return line[found.end() :] if found else line


def npm() -> str:
    """Resolves npm, which is `npm.cmd` on Windows and not on PATH as `npm`."""
    return shutil.which("npm") or "npm"


#: Rebuilding the application. macOS asks for the `app` bundle and nothing else,
#: since that is the only artifact the viewer runner can drive; on Windows the
#: executable is the artifact, and naming a bundle type there is either wrong
#: (`app` does not exist) or a WiX run per mutation for nothing.
APP_BUILD = (
    [npm(), "run", "tauri", "build", "--", "--no-bundle"]
    if WINDOWS
    else [npm(), "run", "tauri", "build", "--", "--bundles", "app"]
)


#: How each runner is built and invoked. The structure probe needs no webview
#: and no bundle, so it neither waits for one nor requires an unlocked screen.
RUNNERS = {
    "viewer": {
        "build": APP_BUILD,
        "run": None,  # built in `run_check`, which needs the app path
    },
    "viewer-tagged": {
        "build": APP_BUILD,
        "run": None,
    },
    "viewer-encodings": {
        "build": APP_BUILD,
        "run": None,
    },
    "viewer-mixed": {
        "build": APP_BUILD,
        "run": None,
    },
    "search": {
        "build": [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--example",
            "search-probe",
        ],
        "run": [
            "src-tauri/target/release/examples/search-probe",
            "--lib",
            PDFIUM_DIR,
            "--file",
            "testdata/multilingual.pdf",
        ],
    },
    "encodings": {
        "build": [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--example",
            "search-probe",
        ],
        "run": [
            "src-tauri/target/release/examples/search-probe",
            "--lib",
            PDFIUM_DIR,
            "--file",
            "testdata/encodings.pdf",
        ],
    },
    "structure": {
        "build": [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--example",
            "structure-probe",
        ],
        "run": [
            "src-tauri/target/release/examples/structure-probe",
            "--library",
            PDFIUM_DIR,
            "--file",
            "testdata/tagged.pdf",
            "--untagged",
            "testdata/text-base14.pdf",
        ],
    },
}


def build(runner: str = "viewer") -> tuple[bool, str]:
    """Rebuilds what the runner needs. A mutation that will not compile is broken."""
    done = subprocess.run(
        RUNNERS[runner]["build"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        errors="replace",
    )
    return done.returncode == 0, done.stderr[-2000:]


def run_probe(runner: str) -> tuple[list[str], str, str]:
    """Runs a probe that needs no webview, in the same shape as `run_check`."""
    done = subprocess.run(
        RUNNERS[runner]["run"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        errors="replace",
    )
    lines = [line for line in done.stdout.splitlines() if MARKER.match(line)]
    return lines, done.stdout, done.stderr


def run_check(fixture: Path = FIXTURE) -> tuple[list[str], str, str]:
    """Runs the viewer check. Returns its result lines, its stdout, and its stderr.

    Only **stdout** carries check results. `viewer_check.py` writes its own
    verdicts on the run -- `[FAIL] exit 1`, a timeout, and the loaded-module
    audit in **both** directions -- to stderr, in the same shape a check line
    has, and the first version of this harness
    read both streams and counted that wrapper line as an eleventh failing check.
    Every one of ten mutations then came back off by exactly one and was reported
    as a broken run. That is the cross-check working: the count from the lines
    and the count from the summary disagreed, and it refused to answer rather
    than picking the wrong one. The stream split is the fix.
    """
    # Three things `viewer_sweep.py` does that this did not, and the absence of
    # them made a mutation run hang rather than fail. Measured 2026-08-16: the
    # app sat for fourteen minutes at 0.0% CPU with the harness waiting on it,
    # which is what an occluded window looks like --- WebKit suspends the page,
    # nothing runs, and there is nothing to time out because no bound was passed.
    #
    #  - `pkill` first, because a leftover window occludes the next one.
    #  - `TPDF_RAISE`, which covers the other half: a window with nowhere visible
    #    to go.
    #  - `--timeout`, so that a hang is a bounded failure. A harness whose worst
    #    case is an unbounded wait cannot report anything at all, and this one is
    #    run unattended by design.
    subprocess.run(
        ["pkill", "-f", "tpdf.app/Contents/MacOS/tpdf"],
        check=False,
        capture_output=True,
    )
    done = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/viewer_check.py"),
            str(APP),
            str(fixture),
            "--timeout",
            str(CHECK_TIMEOUT),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        errors="replace",
        env={**os.environ, "TPDF_RAISE": "1"},
    )
    lines = [line for line in done.stdout.splitlines() if MARKER.match(line)]
    return lines, done.stdout, done.stderr


def execute(runner: str) -> tuple[list[str], str, str]:
    """Runs one harness, whichever it is."""
    if runner == "viewer":
        return run_check()
    if runner == "viewer-tagged":
        return run_check(TAGGED_FIXTURE)
    if runner == "viewer-encodings":
        return run_check(ENCODINGS_FIXTURE)
    if runner == "viewer-mixed":
        return run_check(MIXED_FIXTURE)
    return run_probe(runner)


def verdict(lines: list[str], text: str, stderr: str) -> tuple[set[str], str | None]:
    """Failing check names, or a reason the run cannot be read at all.

    The two counts are the point. `failed` comes from the lines and `stated`
    from the summary's own arithmetic; they answer the same question through
    different code, and a run where they disagree is thrown away rather than
    resolved in either direction.
    """
    summary = next((m for line in text.splitlines() if (m := SUMMARY.match(line))), None)
    if summary is None:
        # The wrapper's own stderr says which silence this was: a timeout, a
        # page that never ran, an occluded window.
        said = " / ".join(line for line in stderr.splitlines() if line.startswith("[FAIL]"))
        return set(), f"no summary line: the run did not finish ({said or 'nothing on stderr'})"
    failed = {after_marker(line) for line in lines if line.startswith("[FAIL]")}
    passed, ran = int(summary.group(1)), int(summary.group(2))
    stated = ran - passed
    if stated != len(failed):
        return set(), f"summary says {stated} failed, {len(failed)} [FAIL] lines"
    return failed, None


def caught(failures: set[str], expect: str) -> bool:
    return any(line.startswith(expect) for line in failures)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="print the pairs and stop")
    parser.add_argument("--only", default="", help="run mutations whose name contains this")
    parser.add_argument(
        "--runner",
        default="",
        choices=["", *RUNNERS],
        help="run only the mutations judged by this harness",
    )
    args = parser.parse_args()

    chosen = [
        m
        for m in MUTATIONS
        if args.only.lower() in m.name.lower() and (not args.runner or m.runner == args.runner)
    ]
    if args.list:
        for m in chosen:
            print(f"{m.name}\n    {m.path} [{m.runner}]\n    expects: {m.expect}")
        return 0
    if not chosen:
        print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    # Only the fixtures the chosen runners actually open. Demanding all of them
    # would refuse a `--runner viewer` run on a machine that has never built the
    # tagged corpus, which is a fixture problem invented by the harness.
    fixtures = {
        "viewer": FIXTURE,
        "viewer-tagged": TAGGED_FIXTURE,
        "viewer-encodings": ENCODINGS_FIXTURE,
        "viewer-mixed": MIXED_FIXTURE,
    }
    for needed in {fixtures[m.runner] for m in chosen if m.runner in fixtures}:
        if not needed.exists():
            print(f"[FAIL] {needed} is missing --- testdata is generated, not committed")
            return 1

    # One baseline per runner in play, since each has its own check names and a
    # mutation's expectation is validated against the runner that will judge it.
    wanted = sorted({m.runner for m in chosen})
    baseline: dict[str, list[str]] = {}
    for runner in wanted:
        print(f"Baseline: building and running the {runner} harness")
        ok, err = build(runner)
        if not ok:
            print(f"[FAIL] the unmutated tree does not build for {runner}\n{err}")
            return 1
        lines, out, err = execute(runner)
        failures, broken = verdict(lines, out, err)
        if broken:
            print(f"[FAIL] {runner} baseline unreadable: {broken}")
            return 1
        if failures:
            print(f"[FAIL] {runner} baseline is not green: {sorted(failures)[:3]}")
            return 1
        baseline[runner] = lines
    lines = baseline[wanted[0]]

    # Refuse to start on a name that cannot go red, and on one that is ambiguous.
    # A prefix matching two checks would report the wrong one as the catcher.
    problems = []
    for m in chosen:
        hits = [line for line in baseline[m.runner] if after_marker(line).startswith(m.expect)]
        if len(hits) != 1:
            problems.append(f"{m.name!r} expects {m.expect!r}, which matches {len(hits)} checks")
        # A check that *skipped* in the baseline is present in the name set and
        # cannot go red, so a mutation aimed at it reports SURVIVED --- the most
        # misleading verdict this harness can print, because it reads as a gap in
        # the checks rather than a fixture that does not exercise them.
        elif hits[0].startswith("[SKIP]"):
            problems.append(
                f"{m.name!r} expects {m.expect!r}, which SKIPS on the {m.runner} "
                "fixture and therefore cannot go red"
            )
    if problems:
        print("[FAIL] " + "\n[FAIL] ".join(problems))
        return 1
    print(
        "Baseline green: "
        + ", ".join(f"{len(baseline[r])} {r} check names" for r in wanted)
        + ", every expectation names exactly one\n"
    )

    survived, unreadable = [], []
    for index, m in enumerate(chosen, 1):
        path = ROOT / m.path
        original = path.read_bytes()
        digest = hashlib.sha256(original).hexdigest()
        source = original.decode("utf-8")
        if source.count(m.before) != 1:
            print(f"[FAIL] {m.name}: its anchor appears {source.count(m.before)} times")
            unreadable.append(m.name)
            continue

        started = time.monotonic()
        path.write_bytes(source.replace(m.before, m.after).encode("utf-8"))
        try:
            built, err = build(m.runner)
            if not built:
                # A mutation that will not compile is not a caught mutation: the
                # checks never ran, so they said nothing about it either way.
                print(f"[BROKEN] {m.name}: does not build\n{err[-400:]}")
                unreadable.append(m.name)
                continue
            lines, out, err = execute(m.runner)
            failures, broken = verdict(lines, out, err)
        finally:
            path.write_bytes(original)
            restored = hashlib.sha256(path.read_bytes()).hexdigest() == digest
        if not restored:
            print(f"[FAIL] {m.path} was not restored byte for byte")
            return 1

        took = time.monotonic() - started
        if broken:
            print(f"[BROKEN] {m.name}: {broken} ({took:.0f}s)")
            unreadable.append(m.name)
        elif caught(failures, m.expect):
            print(f"[CAUGHT] {index}/{len(chosen)} {m.name} -> {len(failures)} red ({took:.0f}s)")
        else:
            print(f"[SURVIVED] {index}/{len(chosen)} {m.name}")
            print(f"    expected {m.expect!r} to fail; {len(failures)} did: {sorted(failures)[:3]}")
            survived.append(m.name)

    # Restored, and rebuilt, so the tree is not left serving a mutated bundle --
    # a stale binary is the other way a later run reports a defect that is not
    # there.
    print("\nRebuilding the clean tree")
    for runner in wanted:
        build(runner)
    print(
        f"\n{len(chosen) - len(survived) - len(unreadable)}/{len(chosen)} caught, "
        f"{len(survived)} survived, {len(unreadable)} unreadable"
    )
    for name in survived:
        print(f"  SURVIVED: {name}")
    for name in unreadable:
        print(f"  UNREADABLE: {name}")
    return 1 if survived or unreadable else 0


if __name__ == "__main__":
    sys.exit(main())
