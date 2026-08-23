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

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402

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

#: Whether to focus each window as it launches; set from `--raise`. A module
#: global rather than a parameter because `run_check` is reached through the
#: runner dispatch, which takes a name and nothing else.
RAISE_WINDOW = False

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

#: The corpus that carries annotations of its own. The comments panel's checks
#: skip on a document with none, so a mutation aimed at one of them anywhere else
#: is aimed at a check that cannot go red --- the same reason the three constants
#: above exist.
COMMENTS_FIXTURE = ROOT / "testdata/comments.pdf"


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
        # The shipped defect exactly: every mark filled across its whole quad,
        # so an underline and a strikeout both look like a highlight while the
        # document is open. The saved file stays correct, which is what made it
        # invisible to `annot-probe` and to every gate.
        "viewer: draw every mark across its whole quad, as the overlay used to",
        "src/lib/viewer.ts",
        "        const band = markBand(mark.kind, quad);",
        "        const band = quad;",
        "an underline leaves the middle of its quad clear",
        runner="viewer",
    ),
    Mutation(
        # The file writes every mark in one colour, whatever the reader chose.
        # The overlay still paints the reader's colour, so this is a divergence
        # between the two renderers and nothing else: `annot-probe` measures the
        # file against the model's own numbers and passes, the overlay phase
        # measures the overlay against its own and passes, and the reader's mark
        # changes colour the moment they save.
        "save: write every mark's appearance in one fixed colour",
        "src-tauri/src/save.rs",
        "        r = mark.color[0],\n        g = mark.color[1],\n        b = mark.color[2],",
        "        r = 0.9_f32,\n        g = 0.6_f32,\n        b = 0.1_f32,",
        "each mark is the same colour in the file as on screen",
        runner="viewer",
    ),
    Mutation(
        # The shipped overlay defect, on the *file* side: every kind washed over
        # its whole quad. It is the mirror of the mutation at the top of this
        # table, and it is here rather than there because what has to be proved
        # is that the comparison reads the saved file --- a mutation of the
        # overlay alone could be caught by a check that never opens one.
        "save: give every kind the highlight's wash in the file",
        "src-tauri/src/save.rs",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Line,",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Wash,",
        "each mark covers as much of its rectangle in the file as on screen",
        runner="viewer",
    ),
    Mutation(
        # Every annotation's rectangle padded 120 points down the page, which is
        # the shape of a real defect --- `docs/TRAPS.md` has an entry about a
        # rectangle padded to make one refusal legal. Ink then lands outside the
        # rectangle the mark was made from and reaches the bare band the
        # untouched control reads, which is the one failure that makes every
        # other reading in the phase meaningless: a difference between the two
        # renders that is not about the marks at all.
        #
        # **Down the page, not up.** The first version of this mutation replaced
        # the whole rectangle with the page and SURVIVED --- it reddened two
        # other checks and not this one, because `bounds` works in the page's own
        # space where y grows upward, so growing the box moved the ink *away*
        # from the band below. The survivor was a statement about the mutation
        # rather than about the control.
        "save: pad every mark's rectangle down the page",
        "src-tauri/src/save.rs",
        "                acc[1].min(q[1]),",
        "                acc[1].min(q[1]) - 120.0,",
        "control: paper no mark was placed on renders identically",
        runner="viewer",
    ),
    Mutation(
        # Build the note box without its swatch row. The colour is then
        # unreachable by pointer --- the `Colour:` commands still work --- and
        # this is what says the window phase written for the row runs at all, in
        # the bundle that ships rather than against a fake DOM.
        "markpopup: build the note box without its swatch row",
        "src/lib/markpopup.ts",
        "    this.element.append(this.header(), this.colors(), this.input, this.actions());",
        "    this.element.append(this.header(), this.input, this.actions());",
        "the note box offers a swatch for every colour a mark can be",
        runner="viewer",
    ),
    Mutation(
        # Put a strikeout's rule on the bottom edge, where an underline's goes.
        # **The control for a bound that was lowered**: `core > 0.8` was
        # arithmetically unreachable and became `> 0.5`, and a bound relaxed to
        # make a red check green is exactly the move that produces an assertion
        # nothing can fail. This says 0.5 still rejects the wrong shape --- an
        # underline reads 0.00 at the centre, so the gap it has to discriminate
        # across is the whole of it.
        "markband: put a strikeout's rule where an underline's goes",
        "src/lib/markband.ts",
        "    case \"strikeout\":\n"
        "      return {\n"
        "        ...quad,\n"
        "        top: quad.top + height / 2 - thickness / 2,\n"
        "        bottom: quad.top + height / 2 + thickness / 2,\n"
        "      };",
        "    case \"strikeout\":\n"
        "      return { ...quad, top: quad.bottom - thickness };",
        "a strikeout crosses the middle of its quad",
        runner="viewer",
    ),
    Mutation(
        # Draw only the first line of a text box. Everything else about the mark
        # is right -- the words are there, in the right font, at the right place
        # -- and a reader loses every line after the first while the saved file
        # keeps them all. A check reading only "is there ink" cannot see it.
        "viewer: draw only a text box's first line",
        "src/lib/viewer.ts",
        "          mark.lines.forEach((line, index) => {",
        "          mark.lines.slice(0, 1).forEach((line, index) => {",
        "a text box draws its words and not its rectangle",
        runner="viewer",
    ),
    Mutation(
        # Fall through to the final `fillRect`, which is what a missing `isText`
        # gives: a solid red rectangle where the reader typed, while the saved
        # file has the words in it the whole time. The underline defect's shape
        # for a third time, and the reason this check exists at all. It reddens
        # two of the four clauses --- `whole` goes to 1.00 and `rim` to 3 --- and
        # the second of those is the reading that replaced `edges === 0`.
        "viewer: draw a text box as a filled rectangle",
        "src/lib/viewer.ts",
        "        if (isText(mark.kind)) {",
        "        if (false) {",
        "a text box draws its words and not its rectangle",
        "viewer",
    ),
    Mutation(
        # Start the type one line lower --- an off-by-one in the baseline, which
        # is the shape of mistake the arithmetic here invites. **Aimed at
        # `lineOne` alone**: the first line lands in the second line's band and
        # the second falls past both, so `lineTwo` still reads ink and only the
        # top band is empty. Without it that clause is the one nothing exercises,
        # since a fall-through to `fillRect` inks every band there is.
        "viewer: start a text box's type one line lower",
        "src/lib/viewer.ts",
        "            const y = top + inset + size + leading * index;",
        "            const y = top + inset + size + leading * (index + 1);",
        "a text box draws its words and not its rectangle",
        "viewer",
    ),
    Mutation(
        # Draw the squiggle as the underline's flat rule on the overlay. **The
        # mutation the `shoulder` reading exists for**: `whole`, `core`, `edges`
        # and `corners` are all satisfied by a rule -- a squiggle is a thin band
        # at the bottom of the quad with an empty centre, one inked side and two
        # inked corners, which is an underline exactly -- so without the strip
        # above the rule this passes every check on the overlay, and the saved
        # file stays correct, so nothing else in the repository sees it either.
        "viewer: draw a squiggle as the underline's flat rule on the overlay",
        "src/lib/viewer.ts",
        "          traceSquiggle(ctx, left, top, width, height, pen);\n"
        "          ctx.stroke();",
        "          ctx.fillRect(left, top + height - pen, width, pen);",
        "a squiggle rises above where an underline's rule stops",
        runner="viewer",
    ),
    Mutation(
        # Draw the ellipse with the box's `strokeRect` on the overlay. **The
        # mutation the corner reading exists for**, and the reason that reading
        # was added rather than the ellipse simply being given the box's
        # predicate: `whole`, `core` and `edges` are all satisfied by a rectangle
        # -- an ellipse touches its quad exactly where `edges` samples, and its
        # centre is as empty as a box's -- so without `corners` this mutation
        # passes every check on the overlay, and the file stays correct, so
        # nothing else in the repository sees it either.
        "viewer: draw an ellipse with the box's strokeRect on the overlay",
        "src/lib/viewer.ts",
        "          traceEllipse(ctx, left, top, width, height);\n"
        "          ctx.stroke();",
        "          ctx.strokeRect(left, top, width, height);",
        "an ellipse touches its rectangle's sides and misses its corners",
        runner="viewer",
    ),
    Mutation(
        # Fill the box rather than stroking it, on the overlay. The file is
        # still right, so a reader sees a solid block until they save and
        # reopen -- the same shape of wrong as the mutation above.
        "viewer: fill a box on the overlay rather than stroking it",
        "src/lib/viewer.ts",
        # **The preceding line disambiguates, and it did not need to until the
        # stamp arrived**: a stamp is bordered by the same `strokeRect` call, so
        # the bare line now matches twice. The box's is the one that follows its
        # own line width; the stamp's is followed by the word it draws.
        "          ctx.lineWidth = OUTLINE_WIDTH * this.zoom * dpr;\n"
        "          ctx.strokeRect(left, top, width, height);\n"
        "        } else if (isEllipse(mark.kind)) {",
        "          ctx.lineWidth = OUTLINE_WIDTH * this.zoom * dpr;\n"
        "          ctx.fillRect(left, top, width, height);\n"
        "        } else if (isEllipse(mark.kind)) {",
        "a box is a frame with its middle clear",
        runner="viewer",
    ),
    Mutation(
        # Draw a stamp as a plain box on the overlay: the border without the
        # word. **The saved file stays correct**, so a reader sees an empty
        # rectangle until they save and reopen, at which point the word appears
        # --- the underline defect's shape for a fourth time, and the reading
        # written for exactly it is the one that goes red.
        "viewer: draw a stamp as an empty box on the overlay",
        "src/lib/viewer.ts",
        '          const word = mark.stamp ? stampWord(mark.stamp) : "";',
        '          const word = "";',
        "a stamp is a border with a word inside it",
        runner="viewer",
    ),
    Mutation(
        # Draw a comment as a plain rectangle. Its /Rect is right and its icon
        # is the reader's, so the saved file is unaffected and only the screen
        # shows a red block where a bubble belongs.
        "viewer: draw a comment as a plain rectangle rather than a bubble",
        "src/lib/viewer.ts",
        "} else if (isIcon(mark.kind)) drawBubble(ctx, left, top, width, height);",
        "} else if (isIcon(mark.kind)) ctx.fillRect(left, top, width, height);",
        "a comment draws inside its own icon box",
        runner="viewer",
    ),
    Mutation(
        # Never clear the overlay between frames. Marks accumulate, so a page
        # the reader has scrolled past keeps its ink -- and the control is the
        # only check that can see it, since every other reading is taken inside
        # a mark's own rectangle where there is ink either way.
        "viewer: leave the overlay uncleared between frames",
        "src/lib/viewer.ts",
        "    ctx.clearRect(0, 0, this.overlay.width, this.overlay.height);",
        "",
        "an untouched page has nothing on the overlay",
        runner="viewer",
    ),
    Mutation(
        # Let the web view's own menu through. That is the state the application
        # shipped in and the reason this exists: right-clicking a page offered
        # Reload, which throws away the reader's view of the document.
        "thumbnails: let the web view's own context menu appear",
        "src/lib/thumbnails.ts",
        "        event.preventDefault();\n        const page =",
        "        const page =",
        "right-clicking a page suppresses the web view's own menu",
        runner="viewer-tagged",
    ),
    Mutation(
        # Report the page the reader is on rather than the one they pointed at.
        # Every command the menu offers acts on the current page, so this looks
        # right whenever they happen to agree -- which on the first row of a
        # freshly opened document is always.
        "thumbnails: report the wrong page for a right-click",
        "src/lib/thumbnails.ts",
        "        offer(Number(slot), { x: event.clientX, y: event.clientY });",
        "        offer(Number(slot) + 1, { x: event.clientX, y: event.clientY });",
        "right-clicking a page reports the page it landed on",
        runner="viewer-tagged",
    ),
    Mutation(
        # Commit the note on the way out of a removal. The reader loses nothing
        # visible -- the mark and its note both go -- and the journal gains a
        # note on a highlight that the very next command deletes, so undoing the
        # removal takes two presses and the first one appears to do nothing.
        "marks: send the note along with the removal",
        "src/lib/viewer.ts",
        "    const id = this.markNote.openId;\n    if (id === null) return;\n    this.markNote.hide(false);",
        "    const id = this.markNote.openId;\n    if (id === null) return;\n    this.markNote.hide();",
        "removing a mark from its note sends a removal and no note",
        runner="viewer-tagged",
    ),
    Mutation(
        # Commit the note of a mark that has gone. The frame loop closes the box
        # when its mark leaves the state -- an undo, or a page deletion -- and
        # committing then sends a note for a mark the model no longer has, which
        # comes back as a refusal the reader sees as an error for their own undo.
        "marks: commit the note of a mark that has been undone",
        "src/lib/viewer.ts",
        "    if (!mark) {\n      this.markNote.hide(false);",
        "    if (!mark) {\n      this.markNote.hide();",
        "a mark that goes while its note is open takes the note with it",
        runner="viewer-tagged",
    ),
    Mutation(
        # Forget to register the note command. Every layer below it still works
        # and every test of them still passes -- the model, the boundary, the
        # writer, and the frontend's call shape against a mock -- and a reader
        # typing on a highlight is told the command does not exist. This is the
        # one mutation in the table aimed at a *list* rather than at logic, and
        # the only check that can see it is the one that talks to the real app.
        "lib: leave the note command out of the handler list",
        "src-tauri/src/lib.rs",
        "            annot_note,\n",
        "",
        "the model takes a note through the command",
        runner="viewer-tagged",
    ),
    Mutation(
        # Leave the note open behind whatever the reader pressed next. Two boxes
        # sit over the page at once, and what was typed in the first is
        # committed by nothing until something else closes it.
        "marks: leave a note open behind whatever was pressed next",
        "src/lib/viewer.ts",
        "    if (this.markNote.openId !== null && this.markNote.openId !== own?.id) {\n      this.closeMark();\n    }\n",
        "",
        "a press on the page closes the note and keeps what was typed",
        runner="viewer-tagged",
    ),
    Mutation(
        # Reopen the note on the mark it is already open on. The box is refilled
        # from the model, which still holds the note as it was -- so a second
        # click on the mark being worked on silently throws away what has been
        # typed since, and nothing else in the application shows a difference.
        "marks: reopen the note on the mark it is already open on",
        "src/lib/viewer.ts",
        "      if (this.markNote.openId !== own.id) this.showMark(own.id);",
        "      this.showMark(own.id);",
        "pressing the same mark again leaves what was typed alone",
        runner="viewer-tagged",
    ),
    Mutation(
        # Open the note for whichever mark the state listed first. Every check
        # that presses a single mark still passes, and a reader with two
        # highlights on a page opens the wrong one half the time.
        "marks: open the first mark rather than the one under the pointer",
        "src/lib/viewer.ts",
        "    return hitTest(here, page, x, y)?.mark ?? null;",
        "    return here[0]?.mark ?? null;",
        "a press away from every mark opens no note",
        runner="viewer-tagged",
    ),
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
        '  if (matches("app.palette", event)) {',
        "  if (false) {",
        "Cmd-K opens the palette",
    ),
    # Re-aimed at `keys.ts` when the chord moved into the bindings table. The
    # expectation is unchanged and the file had to change with it: the arm no
    # longer states the modifier, so "needs no modifier" is not something that
    # line can be made to say any more. Dropping `accel` from the binding is now
    # the only way to spell this mutation, which is the table doing its job --- a
    # chord has one statement of its modifiers, so there is one place to break.
    Mutation(
        "Cmd-K needs no modifier",
        "src/lib/keys.ts",
        '  "app.palette": { keys: ["k", "K"], accel: true },',
        '  "app.palette": { keys: ["k", "K"] },',
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
    Mutation(
        # Proves the window harness reaches `turnsOn`, which is the one place
        # comments, links and the reader's own marks now turn their rectangles.
        # It had three copies until 2026-08-18, and six call sites of one of
        # them turned by the view's rotation alone -- so a page an edit had
        # turned drew a comment in one place and found it in another.
        #
        # A quarter turn added rather than the view's number substituted: the
        # mark phase runs before anything turns a page or the view, so
        # `effectiveTurns` and `this.turns` are both 0 there and swapping them
        # is a no-op. What this check can see is the primitive being wrong at
        # all, which is what makes it evidence that the phase reaches it.
        "placement: turn every rectangle a quarter too far",
        "src/lib/viewer.ts",
        "      turns: this.scroller.effectiveTurns(page),",
        "      turns: this.scroller.effectiveTurns(page) + 1,",
        "a press on the reader's own mark opens its note",
        "viewer",
    ),
    Mutation(
        # Fall back to the page's DISPLAYED rectangle for a page with no
        # `/CropBox`, which is the defect this rule replaced: on a rotated page
        # that is a box in the wrong space, and writing it back made a 612x792
        # page report itself as 612x612. No unit test can see it -- it needs a
        # loaded PDFium and that fixture.
        #
        # Aimed at the one check in that mode derived from the MEDIA box. Every
        # other check there reads `crop_pt`, which is the rule under test, so
        # this mutation corrupts their before and their after equally and it
        # survived them all -- the trap about a check deriving its inputs from
        # the thing it is testing, arriving in the probe written to catch this.
        "crop: fall back to the displayed rectangle for a page with no crop box",
        "src-tauri/src/progressive.rs",
        "        let Some(crop) = crop else { return media };",
        "        let Some(crop) = crop else {\n            return [0.0, 0.0, self.width_pt(), self.height_pt()];\n        };",
        "the page is the size the file says, read by another library",
        "crop-rotated",
    ),
    Mutation(
        # Measure the content box on the page as the reader sees it turned rather
        # than as the document has it. The crop then depends on which way the
        # window was rotated when the command was pressed.
        "crop: measure the content box through the reader's rotation",
        "src-tauri/src/content.rs",
        "        turns: 0,\n        x: 0,\n        y: 0,\n        width,\n        height,",
        "        turns: 1,\n        x: 0,\n        y: 0,\n        width,\n        height,",
        "cropping to the content box raises the ink density",
        "crop-content",
    ),
    Mutation(
        # The guard the whole increment rests on, driven through a real webview
        # rather than through a fake target: the point is that a key **bubbles**
        # from the note field to the root handler and is refused there. The unit
        # test dispatches on the root with a target of its own choosing, which is
        # a statement about the handler and not about the DOM it lives in.
        "note keys: act on a key that went to the note box",
        "src/lib/viewer.ts",
        "    if (inTextField(event)) return;",
        "    if (false) return;",
        "a key typed into a note does not move the page under it",
        "viewer",
    ),
    # The pair's other half --- refuse *every* key, so a deaf viewer cannot read as
    # a working guard --- is deliberately not here, having been run once and
    # measured as unmeasurable from this harness. Every key is how this harness
    # drives the viewer, so the mutation switches off its own instrument: the run
    # spent its whole 360 s bound and came back `[BROKEN] ... did not finish`
    # rather than red. That is the runner's guard working (a timeout is not a
    # survivor, and it said so), and it is the same shape as the trap about a
    # defect that switches off a check's precondition, arrived at harness scale.
    # `mutate_frontend.py` covers the direction, where the viewer is driven by
    # method calls rather than by keys.
    Mutation(
        # Leave the keyboard on the page when Enter asks for the note. In a real
        # webview this is `document.activeElement`, which the fake DOM cannot
        # answer -- so the two harnesses assert the same fact through different
        # instruments and neither can stand in for the other.
        "note keys: ignore Enter with a note open",
        "src/lib/markpopup.ts",
        "  focusField(): void {\n    if (this.shown === null) return;\n    this.input.focus();",
        "  focusField(): void {",
        "the walk leaves the keyboard on the page and Enter moves it in",
        "viewer",
    ),
    Mutation(
        # Wrap the walk at the end. The reader is returned to the first mark and
        # told nothing, which on a marked-up document reads as the key having
        # skipped one.
        "mark walk: wrap at the last mark rather than stopping",
        "src/lib/viewer.ts",
        "    if (!next) {\n      this.opts.onError?.(\n        direction === 1 ? \"No further marks.\" : \"No earlier marks.\",\n      );\n      return false;\n    }",
        "    if (!next) {\n      this.showMark(walk[0]?.id ?? -1, false);\n      return true;\n    }",
        "the walk stops at the last mark rather than wrapping",
        "viewer",
    ),
    Mutation(
        # Do not open a note at all when the walk arrives. The walk then moves the
        # view and shows nothing, which is the whole of what it is for.
        "mark walk: step without opening the note",
        "src/lib/viewer.ts",
        "    this.showMark(next.id, false);",
        "    void next;",
        "the keyboard walk opens a mark's note with no pointer at all",
        "viewer",
    ),
    Mutation(
        # Swallow the box's report of which mark it opened on. The panel is then
        # a list that never follows the page, which is exactly what it was before
        # `onMark` existed --- and nothing else changes, so no vitest suite and no
        # gate can see it: `App.svelte`'s object literal is the seam, and the
        # wiring gate only asks whether the key is *there*.
        "viewer: do not report which mark the note box opened on",
        "src/lib/viewer.ts",
        "      onOpen: (mark) => this.opts.onMark?.(mark),",
        "      onOpen: () => {},",
        "pressing a mark on the page selects its row",
        "viewer",
    ),
    Mutation(
        # Draw the rows and answer no press. The panel looks right, arrows move
        # through it, and pressing a row does nothing at all --- the inert-feature
        # shape this repository has now shipped once, and the reason a press in a
        # real window is checked rather than the handler being unit-tested.
        "marklist: draw the rows and ignore a press on one",
        "src/lib/marklist.ts",
        "      this.focus(mark.id);\n      this.opts.onPick(mark.id);",
        "      this.focus(mark.id);",
        "activating a row opens that mark's note and goes to it",
        "viewer",
    ),
    Mutation(
        # The same edit in the panel this one was copied from, on the corpus that
        # has comments to list. It is here because the marks-panel mutation below
        # *survived*: `rowCount` answered from the rows the panel was handed
        # rather than from the rows it drew, and the identical getter in
        # `commentlist.ts` made "the sidebar lists every comment" equally unable
        # to fail. Both read the DOM now, and this is what says so.
        "commentlist: draw a row for the first comment and stop",
        "src/lib/commentlist.ts",
        "    for (const row of this.rows) {\n      const element = this.build(row);",
        "    for (const row of this.rows.slice(0, 1)) {\n      const element = this.build(row);",
        "the sidebar lists every comment",
        "viewer-comments",
    ),
    Mutation(
        # Take the wrap off the tab row, which is the state the fifth tab shipped
        # in for about an hour. The button is still in the DOM, still
        # `role="tab"`, still counted by "the sidebar has a tab for pages" ---
        # and clipped by the host's `overflow:hidden`, so a pointer cannot reach
        # it. The check this reddens found it for real on its first run, which is
        # better evidence than a mutation; this is what keeps it true.
        "sidebar: lay the tab row out without wrapping",
        "src/lib/sidebar.ts",
        '      "flex:none;display:flex;flex-wrap:wrap;gap:0.2rem;padding:0.3rem 0.4rem;" +',
        '      "flex:none;display:flex;gap:0.2rem;padding:0.3rem 0.4rem;" +',
        "every sidebar tab fits inside the panel",
        "viewer",
    ),
    Mutation(
        # Let the press on a row's remove control reach the row underneath it.
        # **This is the one mutation only a real DOM can catch**: the fake DOM
        # the unit tests run against does not bubble, so `marklist.test.ts`
        # cannot tell `stopPropagation` from its absence -- its own comment says
        # so and points here. In a browser the row's `pointerdown` fires first
        # and opens the note of the mark being taken off, which is a flash and an
        # edit aimed through a box that is closing.
        "marklist: let a press on the remove control reach the row under it",
        "src/lib/marklist.ts",
        '    remove.addEventListener("pointerdown", (event) => event.stopPropagation());',
        '    remove.addEventListener("pointerdown", () => {});',
        "a row's remove control asks for that mark and does not open it",
        runner="viewer",
    ),
    Mutation(
        # Draw a sentence lifted off the page in the same face as one the reader
        # typed. Every word on the row is right and the flag beside it is right;
        # what goes is the only thing on screen that says whose sentence it is.
        # The fake DOM the unit tests run against resolves no styles, so this is
        # the half of the rule that needs a real window.
        "marklist: draw the covered words in the reader's own face",
        "src/lib/marklist.ts",
        '      (line.own ? "" : "opacity:0.6;font-style:italic;");',
        '      "";',
        "a mark nothing was typed on is listed by the words it covers",
        runner="viewer",
    ),
    Mutation(
        # Draw a comment's covered words in the same face as a body somebody
        # wrote. The marks panel's twin, one panel over: every word on the row is
        # right and the flag beside it is right, and what goes is the only thing
        # on screen saying whose sentence it is. The fake DOM resolves no styles,
        # so this half needs a real window.
        "commentlist: draw the covered words in the author's own face",
        "src/lib/commentlist.ts",
        '      (line.own ? "" : "opacity:0.6;font-style:italic;");',
        '      "";',
        "and a lifted sentence is not drawn as one somebody wrote",
        # `viewer-comments`, not `viewer`: this check is one of the five in
        # `COVERED_WORD_CHECKS`, which run on the comments corpus and SKIP
        # everywhere else. Registered against `viewer` when it was written, which
        # made the WHOLE table refuse to start -- correctly, since a mutation
        # aimed at a skipped check reports SURVIVED and reads as a gap in the
        # checks rather than as a fixture that does not exercise them. Caught by
        # the baseline validation on 2026-08-21, cutting 26.8.7, and by nothing
        # before it: a `--runner` run validates only its own runner's mutations,
        # so all three probe runners passed with this in the tree.
        #
        # It then SURVIVED, which was the second and larger finding: it named a
        # check about the WORDS while the edit changes only the FACE they are
        # drawn in, and the comments panel had no check for that at all. The
        # marks panel has had its twin since its own covered-words work. The
        # check named here was written to close that, and this mutation is what
        # says it can go red -- it fired `lifted row #4 normal, authored row #0
        # normal` before the name below was corrected to point at it.
        runner="viewer-comments",
    ),
    Mutation(
        # Hand the marks panel no words at all. The rectangles are unchanged, so
        # every check about where a mark goes still passes and the panel still
        # lists it --- saying "No note" for a highlight over a paragraph, which
        # is what this whole increment was to stop.
        "viewer: take the selection's rectangles without its words",
        "src/lib/viewer.ts",
        '      const said = this.selection.textFrom(page, text) ?? "";',
        '      const said = "";',
        "and the words they cover are the words that are selected",
        runner="viewer",
    ),
    Mutation(
        # List the first mark only. The panel is not empty, which is the point:
        # a reader with one mark sees a correct list, and the check that says
        # otherwise has to count rows against the marks it was handed.
        "marklist: draw a row for the first mark and stop",
        "src/lib/marklist.ts",
        "    for (const row of this.rows) {\n      const element = this.build(row);",
        "    for (const row of this.rows.slice(0, 1)) {\n      const element = this.build(row);",
        "the marks panel lists every mark the reader has made",
        "viewer",
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

def probe_exe(name: str) -> str:
    """The built path of an example binary, in the form `CreateProcess` accepts.

    Windows refuses a **relative forward-slash** path and wants the extension, so
    a bare `src-tauri/target/release/examples/<name>` raises `FileNotFoundError:
    [WinError 2] The system cannot find the file specified` for a file that is
    plainly there --- from inside `subprocess`, naming nothing in this repository,
    which reads as a missing build rather than as a wrong name.

    `BUILD.md` records exactly this against `viewer_check.py`'s app path. It
    arrived here anyway, in a harness written afterwards, because `cwd=ROOT` makes
    every *data* path in the same argv list work and only the executable is
    resolved differently. All five probe runners were dead on this platform from
    the day they were written; the run stopped on the first baseline it reached,
    so nothing was ever reported as SURVIVED.
    """
    path = ROOT / "src-tauri" / "target" / "release" / "examples" / name
    return f"{path}.exe" if WINDOWS else str(path)


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
    "viewer-comments": {
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
            probe_exe("search-probe"),
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
            probe_exe("search-probe"),
            "--lib",
            PDFIUM_DIR,
            "--file",
            "testdata/encodings.pdf",
        ],
    },
    # The rotated fixture, because that is the page the crop trap needs: with no
    # `/CropBox` of its own, PDFium answers `FPDFPage_GetCropBox` with the
    # *displayed* size, and writing that back shrinks the page.
    "crop-rotated": {
        "build": [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--example",
            "crop-probe",
        ],
        "run": [
            probe_exe("crop-probe"),
            "testdata/rotated-90.pdf",
            "--lib",
            PDFIUM_DIR,
            "--mode",
            "follows",
        ],
    },
    "crop-content": {
        "build": [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "src-tauri/Cargo.toml",
            "--example",
            "crop-probe",
        ],
        "run": [
            probe_exe("crop-probe"),
            "testdata/columns.pdf",
            "--lib",
            PDFIUM_DIR,
            "--mode",
            "ink",
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
            probe_exe("structure-probe"),
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


def _kill_leftovers() -> None:
    """Kills any tpdf still running, on whichever platform this is.

    **This was `pkill` unconditionally, and on Windows that is not a program.**
    `check=False` swallows a non-zero exit and not a `FileNotFoundError`, so the
    run died before its first mutation with a traceback and exit 0.
    `viewer_sweep.py` carried the same line and the same defect; the trap entry
    is under that file's name.

    Failure is ignored on purpose: "there was nothing to kill" is the ordinary
    case and both tools report it with a non-zero exit.
    """
    if sys.platform == "win32":
        command = ["taskkill", "/F", "/IM", "tpdf.exe"]
    else:
        command = ["pkill", "-f", "tpdf.app/Contents/MacOS/tpdf"]
    try:
        subprocess.run(command, check=False, capture_output=True)
    except OSError:
        pass


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
    #  - a leftover kill first, because a stray window occludes the next one ---
    #    and on Windows it is worse than an occlusion, since single-instance
    #    makes the new process forward its argv to the old one and exit.
    #  - `--raise`, which covers the other half: a window with nowhere visible
    #    to go. **Off by default since 2026-08-20**, for the reason
    #    `viewer_sweep.py` records at greater length: an unfocused window costs
    #    these checks nothing, an *occluded* one costs them everything, and the
    #    two are different properties --- so forcing the raise took the keyboard
    #    away from whoever was at the machine on every mutation, to guarantee
    #    something the polite default usually already has. A wrong default is
    #    caught on the **baseline**, which runs before any mutation, so it costs
    #    one run rather than the whole harness.
    #  - `--timeout`, so that a hang is a bounded failure. A harness whose worst
    #    case is an unbounded wait cannot report anything at all, and this one is
    #    run unattended by design.
    _kill_leftovers()
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
        # Explicit, not just `errors`: `text=True` decodes with the locale
        # codec, which is cp1252 on Windows, and the multilingual corpus holds
        # bytes it refuses. The same line in `viewer_sweep.py` killed a
        # thirteen-corpus run six corpora in --- see the trap of that name.
        encoding="utf-8",
        errors="replace",
        env={**os.environ, **({"TPDF_RAISE": "1"} if RAISE_WINDOW else {})},
    )
    out = done.stdout or ""
    lines = [line for line in out.splitlines() if MARKER.match(line)]
    return lines, out, done.stderr or ""


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
    if runner == "viewer-comments":
        return run_check(COMMENTS_FIXTURE)
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
    # Before anything prints. A redirected run is block-buffered otherwise, and
    # this harness takes the better part of an hour: on 2026-08-19 a full run's
    # output sat at three lines for forty minutes and its verdict was lost
    # entirely when the run was interrupted, which is the exact ambiguity
    # `live_output` exists to remove. The three window harnesses had the fix and
    # the three mutation harnesses did not.
    stream_results()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="print the pairs and stop")
    parser.add_argument("--only", default="", help="run mutations whose name contains this")
    parser.add_argument(
        "--runner",
        default="",
        choices=["", *RUNNERS],
        help="run only the mutations judged by this harness",
    )
    parser.add_argument(
        "--raise",
        dest="raise_window",
        action="store_true",
        help=(
            "focus each window as it launches. Needed only where there is "
            "nowhere visible to put one, and it takes the keyboard once per "
            "mutation, so it is off by default. The baseline run is what says "
            "whether it is needed here."
        ),
    )
    args = parser.parse_args()
    global RAISE_WINDOW
    RAISE_WINDOW = args.raise_window

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
        "viewer-comments": COMMENTS_FIXTURE,
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
        # Every anchor in the table above is written with "\n", and a Windows
        # checkout is CRLF -- so a multi-line anchor matches ZERO times here and
        # the mutation is reported as unreadable, which reads as drift in the
        # source. `mutate_frontend.py` and `mutate_rust.py` were given this on
        # 2026-07-30 and this harness was not; the probe-path defect above stopped
        # every Windows run before its first mutation, so nothing said so. The
        # `anchors` gate cannot see it either: it reads with `read_text`, whose
        # universal-newline translation makes every anchor match. Normalise for
        # matching, put the file's own convention back, and leave the mutation as
        # the only difference on disk.
        raw = original.decode("utf-8")
        crlf = "\r\n" in raw
        source = raw.replace("\r\n", "\n") if crlf else raw
        if source.count(m.before) != 1:
            print(f"[FAIL] {m.name}: its anchor appears {source.count(m.before)} times")
            unreadable.append(m.name)
            continue

        started = time.monotonic()
        mutated = source.replace(m.before, m.after)
        if crlf:
            mutated = mutated.replace("\n", "\r\n")
        path.write_bytes(mutated.encode("utf-8"))
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
