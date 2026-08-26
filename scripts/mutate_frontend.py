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

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402
import mutation_since  # noqa: E402

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
#: A third, added 2026-08-17 and removed the same day: bumping every page's tile
#: *epoch* in `Scroller.setPages` as well as carrying it. It survived the whole
#: suite, and the reason is that `setPages` calls `clearTiles`, which bumps the
#: generation --- one mechanism already drops every outstanding reply, so the
#: per-page bump changed no behaviour any test could see. The bump is gone and
#: the epoch is only carried; what replaced the mutation aims at `clearTiles`
#: itself, which is the mechanism that does the work.
#:
#: Recorded rather than deleted silently: the next person to notice the gap
#: should find out that it was measured, not overlooked.
MUTATIONS = [
    Mutation(
        # Drop the counts and keep only the warning. A merge whose report says
        # nothing about how much went in is a copy's report on an operation the
        # reader cannot check by looking at what they asked for.
        "recovery: report a merge without saying what it merged",
        "src/lib/recovery.ts",
        "  const said = `Merged this document with ${others} — ${pages} in all.`;",
        '  const said = "The merge was written.";',
        "says how many documents went in and how many pages came out",
    ),
    Mutation(
        # Always plural. "1 other documents" reads as a defect in everything
        # else on screen, which is why the singular is asserted rather than
        # left to taste.
        "recovery: pluralise a merge of one document",
        "src/lib/recovery.ts",
        '    merged.files === 1 ? "1 other document" : `${merged.files} other documents`;',
        "    `${merged.files} other documents`;",
        "says it in the singular for one document and one page",
    ),
    Mutation(
        # Return before the warning. The counts survive and the fact that the
        # file underneath moved does not --- the direction that looks like a
        # working report.
        "recovery: drop the changed-source warning from a merge",
        "src/lib/recovery.ts",
        "  if (!merged.changed) return said;",
        "  if (merged.changed || !merged.changed) return said;",
        "adds the changed-source warning without dropping the counts",
    ),
    Mutation(
        # A registered command that reaches no action. This is the shape that
        # shipped inert once before, and no type error and no registry sweep
        # can see it: the command has a `run`, and the `run` does nothing.
        "appcommands: register Merge documents without reaching its action",
        "src/lib/appcommands.ts",
        "      run: () => actions.mergeDocuments(),",
        "      run: () => {},",
        "merge documents through the command, with no value to carry",
    ),
    Mutation(
        # Prompt for every refusal, not only the answerable one. A password
        # dialog in front of a corrupt file asks a reader for something that
        # cannot help, and no answer ends it.
        #
        # This one is why the three `ask` mocks that assert they are NOT called
        # still resolve `null`: with a bare `vi.fn()` resolving `undefined` this
        # mutation SPUN FOREVER and killed the vitest worker, which is a hang and
        # not a red test -- docs/TRAPS.md has the entry.
        "unlock: prompt for a refusal a password cannot fix",
        "src/lib/unlock.ts",
        "      if (!isOpenRefusal(e) || !e.locked || !ask) throw e;",
        "      if (!isOpenRefusal(e) || !ask) throw e;",
        "does not ask about a refusal that is not the answerable one",
    ),
    Mutation(
        # Read a dismissal as "try again with nothing". The reader pressed
        # Cancel and the open runs once more, refuses identically, and they are
        # shown the refusal they had already dismissed.
        "unlock: retry with no password when the reader declines",
        "src/lib/unlock.ts",
        "      if (typed === null) throw e;",
        "      if (typed === null) return await open(undefined);",
        "rethrows the refusal when the reader declines",
    ),
    Mutation(
        # One retry rather than a loop. Mistyping twice is ordinary, and the
        # second attempt would return the raw refusal instead of asking again.
        "unlock: allow one retry instead of as many as are offered",
        "src/lib/unlock.ts",
        "  let password: string | undefined;\n  for (;;) {",
        "  let password: string | undefined;\n  for (let once = 0; once < 2; once++) {",
        "asks again after a wrong password, showing the backend's second wording",
    ),
    Mutation(
        # Send an empty field as an empty password. PDFium distinguishes them --
        # an empty *user* password is what most permission-restricted documents
        # carry -- so this retries the attempt that already failed, and the
        # reader watches Unlock do nothing.
        "passworddialog: read an empty field as an empty password",
        "src/lib/passworddialog.ts",
        "    this.settle(this.field.value || null);",
        "    this.settle(this.field.value);",
        "reads an empty field as no answer rather than as an empty password",
    ),
    Mutation(
        # Leave the password in the input. It is the one place the secret would
        # outlive its use, in a live element attached to the document.
        "passworddialog: keep the password in the field after closing",
        "src/lib/passworddialog.ts",
        '    this.field.value = "";\n    if (this.shown) {',
        "    if (this.shown) {",
        "clears the field on every close, however it closed",
    ),
    Mutation(
        # Stack a second question on the first. Whoever awaited the first is
        # holding an open that never finishes, and nothing later can settle it --
        # the test that catches this one does so by TIMING OUT, which is the
        # failure shape docs/TRAPS.md warns reads as a broken harness.
        "passworddialog: leave the previous question unsettled",
        "src/lib/passworddialog.ts",
        "    // A second question dismisses the first rather than stacking on it. Nothing\n"
        "    // issues two today; what this rules out is a promise nobody settles.\n"
        "    this.settle(null);",
        "",
        "settles an outstanding question when a second one is asked",
    ),
    Mutation(
        # Dismiss on any click, including one that bubbled out of the panel. The
        # reader clicks their own password field and the dialog closes.
        "passworddialog: dismiss on a click anywhere, not only the backdrop",
        "src/lib/passworddialog.ts",
        "      if (event.target === this.backdrop) this.settle(null);",
        "      this.settle(null);",
        "treats a click on the backdrop as a dismissal and one on the panel as nothing",
    ),
    Mutation(
        # Offer a reload for every refusal. "A document must keep at least one
        # page" then arrives with a button that discards the reader's work in
        # exchange for nothing, which is the shape this whole module exists to
        # avoid: an offer is only safe where it is the answer.
        "recovery: offer a reload whatever the refusal was",
        "src/lib/recovery.ts",
        "  if (!failure.changed || failure.reopen) {",
        "  if (failure.reopen) {",
        "offers nothing for a refusal that is not about the file changing",
    ),
    Mutation(
        # Offer buttons on a refusal the window has already acted on. `App.svelte`
        # reopens the file itself when `reopen` is set, so Reload reloads what is
        # on screen and Save a copy copies a freshly-opened, unedited document --
        # and a reader presses them, because they are the ones that sound safe.
        "recovery: offer buttons after the window has already reopened the file",
        "src/lib/recovery.ts",
        "  if (!failure.changed || failure.reopen) {",
        "  if (!failure.changed) {",
        "offers nothing once the document is closed, because the window reopened it",
    ),
    Mutation(
        # Put the destructive one first. Both buttons are present either way, so
        # a check on the set cannot see this; the reader reaching for the nearest
        # button is what can.
        "recovery: lead with the offer that spends the journal",
        "src/lib/recovery.ts",
        '    offers: ["saveCopy", "reload"],\n  };\n}\n\n/**\n * What to say after a copy',
        '    offers: ["reload", "saveCopy"],\n  };\n}\n\n/**\n * What to say after a copy',
        "warns before discarding unsaved edits, and offers the copy first",
    ),
    Mutation(
        # Reload without a word, which is what it did until this landed. The
        # command was written before there was anything to lose.
        "recovery: reload an edited document without warning",
        "src/lib/recovery.ts",
        "  if (!dirty) return null;",
        "  return null;\n  // eslint-disable-next-line",
        "warns before discarding unsaved edits, and offers the copy first",
    ),
    Mutation(
        # Write a copy from a changed source and say nothing, so a file built
        # from a document the reader is not looking at reads as one that is.
        "recovery: write a copy from a changed source silently",
        "src/lib/recovery.ts",
        "  if (!copied.changed) return null;",
        "  return null;",
        "says the copy came from a newer file, and does not call it an error",
    ),
    Mutation(
        # Answer "am I up to date" without saying which version that is. The
        # reader presses the command *because* they want the number, and an
        # answer that omits it is the silence this replaced wearing a sentence.
        "update: answer the current check without naming the running version",
        "src/lib/update.ts",
        "      return `tpdf ${version} is the latest version`;",
        '      return "Up to date";',
        "says the running version is the latest, rather than saying nothing",
    ),
    Mutation(
        # Report the box in the space the reader is looking at rather than the
        # file's. Identical on an unrotated, uncropped page -- which is thirteen
        # of the fourteen corpora -- and a rectangle somewhere else entirely on
        # the fourteenth.
        "viewer: report a drawn box in the laid-out space, not the file's",
        "src/lib/viewer.ts",
        "    return [moved[0] ?? 0, moved[1] ?? 0, moved[2] ?? 0, moved[3] ?? 0];",
        "    return [quad.left, quad.top, quad.right, quad.bottom];",
        "puts a box drawn at the screen's top-left in the corner the turn implies",
    ),
    Mutation(
        # Undo the turn with the page's un-turned dimensions, which is the
        # mistake `unturnQuad`'s doc comment warns about: a plausible rectangle
        # in the wrong place, with the right proportions.
        "viewer: unturn a drawn box against the wrong pair of dimensions",
        "src/lib/viewer.ts",
        "    const back = unturnQuad(quad, turns, laid.width, laid.height);",
        "    const back = unturnQuad(quad, turns, laid.height, laid.width);",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Clamp against the page as the file describes it rather than as it is
        # laid out. Passes at half turns, where the sides do not swap.
        "viewer: clamp a drawn box against the un-turned page",
        "src/lib/viewer.ts",
        "      this.scroller.effectiveTurns(slot),\n    );",
        "      0,\n    );",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Spend the tool even when the box was refused. A slipped click then
        # costs the reader the command as well, with nothing on screen saying
        # why -- the shape this was written as first, and the test found it.
        # **Re-aimed 2026-08-22**, when the comment tool made the quad a
        # conditional: a comment is placed by a press and every other kind still
        # needs two corners, so the one line this named is now three. The edit is
        # the same one --- spend the tool before the refusal is decided.
        "viewer: spend the box tool on a click that drew nothing",
        "src/lib/viewer.ts",
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));",
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));\n        this.drawKind = null;",
        "refuses a click, and keeps the tool armed",
    ),
    Mutation(
        # Leave the tool armed after a box is drawn. A reader who wanted one box
        # gets a second on their next press, which is a mode they cannot see and
        # did not ask to stay in.
        "viewer: keep the box tool armed after a box is drawn",
        "src/lib/viewer.ts",
        "        this.drawKind = null;\n        this.drawStamp = null;\n        this.showCursor();",
        "        this.drawStamp = null;\n        this.showCursor();",
        "is spent by one box",
    ),
    Mutation(
        # Back to one-shot: commit on release, as the box does. A drawing is then
        # one stroke and can never be more, which is the state the window was in
        # until 2026-08-20 -- while `/InkList`, the writer and the probe were all
        # built for several, so the harness could make a document no reader could.
        "viewer: commit a drawing when the pointer comes up",
        "src/lib/viewer.ts",
        "          this.inking ??= { slot: live.slot, strokes: [] };\n          this.inking.strokes.push(live.points);",
        "          this.inking ??= { slot: live.slot, strokes: [] };\n          this.inking.strokes.push(live.points);\n          this.finishDrawing();",
        "stays armed so a drawing can be several strokes",
    ),
    Mutation(
        # Keep what was drawn when Escape ends it. Escape has meant abandon since
        # the box, so a drawing left behind by it is a mode the reader thinks
        # they left -- and the next stroke anywhere joins a drawing they believe
        # they threw away.
        "viewer: leave the strokes behind when Escape ends a drawing",
        "src/lib/viewer.ts",
        "    // by it exactly as a half-dragged rectangle is --- which is why the finish\n    // gesture had to be a *different* key rather than a second Escape.\n    this.inking = null;",
        "    // by it exactly as a half-dragged rectangle is --- which is why the finish\n    // gesture had to be a *different* key rather than a second Escape.",
        "throws the whole drawing away on Escape",
    ),
    Mutation(
        # Let a stroke on another page join the drawing. An annotation belongs to
        # one page, so the second stroke is then drawn on a page it was not made
        # on -- ink where nobody put it, which is worse than a refused press.
        "viewer: let a stroke started on another page join the drawing",
        "src/lib/viewer.ts",
        "        if (this.inking && this.inking.slot !== page) return false;",
        "        if (false) return false;",
        "refuses a stroke that starts on another page",
    ),
    Mutation(
        # Stop reporting the drawing. The one mode in this application then has
        # nothing on screen to say it is live or how to leave it, which is what
        # the box's one-shot design existed to avoid.
        #
        # **Aimed at the accessor, because `ViewerStatus.drawing` IS the
        # accessor.** It was its own expression in the status object, and this
        # mutation emptied that and SURVIVED: every test asked the viewer and
        # only the window reads the status, so the copy nobody tested was the one
        # a reader sees. Removing the copy is what makes this catchable.
        "viewer: keep the drawing out of the status, so the mode is invisible",
        "src/lib/viewer.ts",
        "    if (this.inking) return this.inking.strokes.length;",
        "    if (false) return this.inking?.strokes.length ?? 0;",
        "reports the drawing in the status, so the mode is visible",
    ),
    Mutation(
        # Let two checks share a name. The roll is compared as a set, so a repeat
        # is invisible there: one verdict prints under another's label and a third
        # name leaves the roll, with every count still adding up.
        "checkreport: let two checks share a name",
        "src/lib/checkreport.ts",
        "    if (repeated.length > 0) {",
        "    if (false) {",
        "fails the run when two checks share a name",
    ),
    Mutation(
        # Keep every pointer event. A hand resting on a trackpad produces dozens
        # of points inside a tenth of a point, none of which changes the line and
        # all of which go into the file and over the IPC boundary.
        "viewer: keep every pointer event rather than sampling the stroke",
        "src/lib/viewer.ts",
        "        const last = live.points[live.points.length - 1];\n        if (\n          !last ||",
        "        const last = live.points[live.points.length - 1];\n        if (\n          true ||",
        "samples the pointer rather than keeping every event",
    ),
    Mutation(
        # Drop the point the pointer was released on. The sample may have
        # discarded it, so every stroke ends up to one sample short --- invisible
        # on a long sweep and the whole of a tick or the crossing of a t.
        "viewer: end a stroke at the last sample rather than at the release",
        "src/lib/viewer.ts",
        "          if (!last || last.x !== live.to.x || last.y !== live.to.y) {\n            live.points.push({ ...live.to });\n          }",
        "          if (false) {\n            live.points.push({ ...live.to });\n          }",
        "keeps the point the pointer was released on",
    ),
    Mutation(
        # Commit a drawing of one point. A press that never moved is a reader who
        # has not started, and spending the tool on it costs them the command
        # with nothing on screen to say why.
        "viewer: commit a drawing the pointer never moved for",
        "src/lib/viewer.ts",
        "          if (live.points.length < 2) return;",
        "          if (false) return;",
        "keeps the tool armed when the pointer never moved",
    ),
    Mutation(
        # Send a drawing's rectangle as well as its strokes. The model refuses
        # the pair outright, so the mark never lands -- which reads as the tool
        # doing nothing rather than as a wire defect.
        "viewer: send a rectangle alongside a drawing's strokes",
        "src/lib/viewer.ts",
        '      "ink",\n      id,\n      {\n        quads: [],',
        '      "ink",\n      id,\n      {\n        quads: [0, 0, 1, 1],',
        "commits strokes and no rectangle",
    ),
    Mutation(
        # Leave a drawing's points in the view's space. Correct on an unturned
        # page and wrong on every other, which is the shape of defect a single
        # untried corpus hides.
        "viewer: leave a drawing's points unmapped into the file's space",
        "src/lib/viewer.ts",
        "          const mapped = this.fileRectOn(made.slot, {",
        "          const mapped = [point.x, point.y] as const;\n          void this.fileRectOn(made.slot, {",
        "puts the points back in the file's space on a turned page",
    ),
    Mutation(
        # Send the slot rather than the page's id. Works on every document until
        # a page is deleted or moved, and then marks land on the wrong page.
        "viewer: send a drawn box to the slot rather than to the page's id",
        "src/lib/viewer.ts",
        # The `quad` line above it is what disambiguates: the crop's drag has an
        # identical `const id = quad ? ...`, and it builds its quad with
        # `boxQuad(live.from, live.to, ...)` unconditionally rather than choosing
        # between an icon and a box.
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));\n"
        "        const id = quad ? this.pages.idOf(live.slot) : undefined;",
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));\n"
        "        const id = quad ? live.slot : undefined;",
        "carries the armed kind and the page's id",
    ),
    Mutation(
        # Take the press after the hit tests rather than before them. A box
        # drawn across a highlight is then swallowed by the highlight's note,
        # and one across a link jumps to another page.
        "viewer: hit-test the page before letting the box tool have the press",
        "src/lib/viewer.ts",
        "    if (this.cropDrag.start(event) || this.drawDrag.start(event)) {\n"
        "      event.preventDefault();\n      return;\n    }",
        "",
        "goes to the drag when a tool is armed and not when one is not",
    ),
    Mutation(
        # Commit a cancelled drag. Escape then draws the box the reader was
        # trying not to draw -- the same defect as `drag.ts`'s own cancel
        # mutation, one layer up, where the box actually reaches the model.
        "viewer: draw the box even when the drag was cancelled",
        "src/lib/viewer.ts",
        "        if (!committed || !live || !kind) {",
        "        if (!live || !kind) {",
        "is dropped by Escape mid-drag, without committing the box",
    ),
    Mutation(
        # Re-read the page under the pointer on every move. A drag that wanders
        # onto the next page then silently moves the box to it, and the corner
        # it started from is measured on a page it is no longer on.
        # Named for the two-page test rather than the scroll one: the scroll
        # test asserts the *starting* corner, which this mutation leaves alone.
        # It was aimed at the wrong check first and the harness said so.
        "viewer: follow the pointer onto another page mid-drag",
        "src/lib/viewer.ts",
        # Disambiguated by the line after it: the crop's drag has the identical
        # two lines and then wakes, where the box's goes on to sample for ink.
        "        const { x, y } = this.pageAndPoint(at);\n        live.to = { x, y };\n"
        "        // **Sampled, not every event.**",
        "        const at2 = this.pageAndPoint(at);\n        live.slot = at2.page;\n"
        "        live.to = { x: at2.x, y: at2.y };\n"
        "        // **Sampled, not every event.**",
        "keeps the box on the page it started from",
    ),
    Mutation(
        # Build the box by subtracting in arrival order. Correct for a drag down
        # and to the right, inside out for the other three -- and an inside-out
        # rectangle does not draw, which reads as a tool that only works one way.
        # Named for `markband.test.ts`'s check, which is deliberately *not*
        # called the same thing as `viewerdraw.test.ts`'s: it was, and two tests
        # sharing a name made this harness's two failure counts disagree by one
        # and report that it could not read its own output. That guard is right
        # and cannot know the cause.
        "markband: build a box from the corners in the order they arrived",
        "src/lib/markband.ts",
        "    left: clampX(Math.min(from.x, to.x)),",
        "    left: clampX(from.x),",
        "normalises the corners whichever way the drag went",
    ),
    Mutation(
        # Classify a comment as a plain fill. The overlay then draws a filled
        # rectangle where the bubble goes --- which is what the painter's old
        # bare `else` did for *any* kind it had no branch for, and the reason
        # this classifier is exhaustive: a wrong picture rather than a missing
        # one, on every page, with the saved file still correct.
        "markband: draw a comment as a plain fill rather than as its own style",
        "src/lib/markband.ts",
        '    case "note":\n      return "icon";',
        '    case "note":\n      return "fill";',
        "agrees with save.rs arm for arm, which is where the pair can drift",
    ),
    Mutation(
        # Drop the clamp that keeps a drawn box on the page, exactly as the
        # comment's was dropped above. A drag off the edge writes a /Rect past
        # the page box, which `save.rs` maps without complaint.
        "markband: let a drawn box run off the edge of the page",
        "src/lib/markband.ts",
        "  const clampX = (v: number): number => Math.max(0, Math.min(v, page.width));",
        "  const clampX = (v: number): number => v;",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Accept a rectangle of any size. A click then writes an annotation
        # nothing draws and nobody can find again to remove.
        # Aimed at the tall-and-thin check, not at the click: a click is zero
        # in *both* dimensions, so the height guard below catches it whatever
        # this one does. Only a box that is wide enough and too narrow can say
        # which of the two fired.
        "markband: accept a box with no size",
        "src/lib/markband.ts",
        "  if (quad.right - quad.left < MIN_BOX) return null;",
        "",
        "refuses a box that is tall and thin",
    ),
    Mutation(
        # The other dimension. Together they say the bound is on *both* sides,
        # which one of them alone does not.
        "markband: accept a box with no height",
        "src/lib/markband.ts",
        "  if (quad.bottom - quad.top < MIN_BOX) return null;",
        "",
        "refuses a box that is wide and flat",
    ),
    Mutation(
        # Fill a box rather than outlining it, on the overlay. The saved file is
        # still right, so a reader sees a solid block until they save and reopen
        # -- which is exactly the shape of the underline defect this repository
        # already paid for once.
        "markband: draw a box as a filled rectangle, as every mark used to be",
        "src/lib/markband.ts",
        '  return kind === "square";',
        "  return false;",
        "says a box is drawn as an outline and the others are not",
    ),
    Mutation(
        # Fill the text box's rectangle rather than drawing words in it, which
        # is what the final `else` does and what a missing `isText` gives you: a
        # solid red block where the reader typed, with the saved file carrying
        # the words the whole time. The underline defect's shape, a third time.
        "markband: draw a text box as a filled rectangle",
        "src/lib/markband.ts",
        '  return kind === "textbox";',
        "  return false;",
        "says a text box is drawn as words and the others are not",
    ),
    Mutation(
        # Fill the squiggle's band on the overlay rather than waving through it.
        # A solid red bar two and a half times an underline's height under the
        # words, with the saved file correct throughout -- which is the underline
        # defect's shape, one kind later.
        "markband: draw a squiggle as a filled band",
        "src/lib/markband.ts",
        '  return kind === "squiggly";',
        "  return false;",
        "says a squiggle is drawn as a wave and the others are not",
    ),
    Mutation(
        # Give the squiggle the underline's band. Nothing about the shape
        # changes -- it is still a wave -- but it is drawn inside a rule's height,
        # which closes the strip every check that tells the kinds apart reads.
        "markband: fit a squiggle into an underline's band",
        "src/lib/markband.ts",
        "      return { ...quad, top: quad.bottom - height * SQUIGGLE_HEIGHT };",
        "      return { ...quad, top: quad.bottom - height * LINE_FRACTION };",
        "gives a squiggle a band taller than an underline's rule",
    ),
    Mutation(
        # Draw an ellipse as a filled rectangle on the overlay, which is what
        # the final `else` does. The box's mutation above is the same defect one
        # branch along, and the two are separate because the overlay asks
        # `isOutline` first: a reader watching a ring turn into a solid block
        # would see the saved file come back correct, which is the underline
        # defect's shape exactly.
        "markband: draw an ellipse as a filled rectangle",
        "src/lib/markband.ts",
        '  return kind === "ellipse";',
        "  return false;",
        "says an ellipse is drawn as one and the others are not",
    ),
    Mutation(
        # Leave the move listener registered after the drag ends. The pointer
        # goes on being tracked with the button up, which reads as a viewer that
        # has become sticky rather than as a missing line -- and is exactly the
        # failure that made a sixth hand-rolled listener pair worth refusing.
        "drag: keep following the pointer after the button comes up",
        "src/lib/drag.ts",
        '    this.host.removeEventListener("pointermove", this.onMove);',
        "",
        "takes its listeners off and releases the capture",
    ),
    Mutation(
        # Never release the capture. Every later pointer event on the page goes
        # to this element whatever it is over, and nothing about the drag itself
        # looks wrong.
        "drag: hold the pointer capture after the drag is over",
        "src/lib/drag.ts",
        "      this.host.releasePointerCapture(live.pointerId);",
        "      void live;",
        "takes its listeners off and releases the capture",
    ),
    Mutation(
        # Report a cancel as a commit. An Escape, or a touch that turned into a
        # scroll, then draws the box the reader was trying not to draw.
        "drag: treat a cancelled drag as a released one",
        "src/lib/drag.ts",
        "  private readonly onCancel = (event: PointerEvent): void => {\n    this.finish(event, false);",
        "  private readonly onCancel = (event: PointerEvent): void => {\n    this.finish(event, true);",
        "reports a browser cancel as not committed",
    ),
    Mutation(
        # Let a second pointer replace the live drag instead of being refused.
        # A second finger then ends the first drag at its own starting point,
        # committing a rectangle nobody drew.
        "drag: let a second press take over the live drag",
        "src/lib/drag.ts",
        "    if (this.live) return false;\n    const at = { clientX: event.clientX, clientY: event.clientY };",
        "    const at = { clientX: event.clientX, clientY: event.clientY };",
        "refuses a second press rather than replacing the live drag",
    ),
    Mutation(
        # Accept a refused begin anyway. The listeners and the capture are taken
        # for a press the caller said was not theirs -- a press on no page, or a
        # tool that is not armed -- so the surface stops receiving what it wanted.
        "drag: register the drag even when the target refused it",
        "src/lib/drag.ts",
        "    if (!this.target.begin(at)) return false;",
        "    this.target.begin(at);",
        "registers nothing when the target refuses",
    ),
    Mutation(
        # Tell the target before tearing down rather than after. A target that
        # starts another drag from its own `end` -- a one-shot tool that re-arms
        # -- then finds a half-registered one in the way.
        # Written first as an insertion, which *duplicated* the call rather than
        # moving it -- so `end` ran twice, the second run overwrote what the
        # first had recorded, and the check written for this stayed green while
        # three unrelated ones went red. A mutation that lands as something
        # other than what it says is the trap index's "three ways a mutation
        # lies to you"; the anchor below is the whole tail so the move is a move.
        "drag: tell the target before the drag is torn down",
        "src/lib/drag.ts",
        "    this.live = null;\n    this.host.removeEventListener(\"pointermove\", this.onMove);\n    this.host.removeEventListener(\"pointerup\", this.onUp);\n    this.host.removeEventListener(\"pointercancel\", this.onCancel);\n    try {\n      this.host.releasePointerCapture(live.pointerId);\n    } catch {\n      // Never captured.\n    }\n    this.target.end(at, committed);",
        "    this.target.end(at, committed);\n    this.live = null;\n    this.host.removeEventListener(\"pointermove\", this.onMove);\n    this.host.removeEventListener(\"pointerup\", this.onUp);\n    this.host.removeEventListener(\"pointercancel\", this.onCancel);\n    try {\n      this.host.releasePointerCapture(live.pointerId);\n    } catch {\n      // Never captured.\n    }",
        "is already torn down by the time the target is told",
    ),
    Mutation(
        # Take any pointer's release, not the one that started the drag. A
        # second finger lifting ends a drag the first one is still doing.
        "drag: end the drag on any pointer's release",
        "src/lib/drag.ts",
        "    if (event && event.pointerId !== live.pointerId) return;",
        "    if (event && false) return;",
        "ignores its release, so the drag stays live",
    ),
    Mutation(
        # Cancel from the point the drag started rather than the last one seen.
        # Invisible until a caller uses the point a cancel reports, which the
        # box preview does.
        "drag: cancel at the starting point rather than the last point seen",
        "src/lib/drag.ts",
        "    live.at = { clientX: event.clientX, clientY: event.clientY };",
        "",
        "cancels at the last point seen, since a cancel has no point of its own",
    ),
    Mutation(
        # Drop the clamp that keeps a comment's icon on the page. `save.rs` maps
        # quads and does not police them, so an unclamped drop near the edge
        # writes a /Rect running past the page box -- which other readers clip,
        # half-draw, or place wherever they like.
        "markband: let a comment's icon hang off the edge of the page",
        "src/lib/markband.ts",
        "  const left = Math.max(0, Math.min(x, page.width - ICON_SIZE));",
        "  const left = x;",
        "keeps a comment dropped at the far edge inside the page",
    ),
    Mutation(
        # Treat a comment as a wash. It would then be drawn multiplied into the
        # paper rather than as an opaque bubble sitting on it -- and `save.rs`
        # branches on the same question, so the file would get /CA 0.4 too.
        "markband: count a comment as a wash",
        "src/lib/markband.ts",
        '  return kind === "highlight";',
        '  return kind === "highlight" || kind === "note";',
        "is not a wash, so the two questions do not collapse",
    ),
    Mutation(
        # The defect exactly as it shipped, restored. `paintMarks` filled the
        # whole quad in one colour for every kind, so while a document was open
        # an underline and a strikeout both looked like a highlight --- and the
        # saved file was right the whole time, which meant the mark changed
        # under the reader when they saved and reopened it. Reported from use.
        "markband: draw an underline as a wash, as the overlay used to",
        "src/lib/markband.ts",
        "      return { ...quad, top: quad.bottom - thickness };",
        "      return quad;",
        "sits an underline on the bottom edge",
    ),
    Mutation(
        # The subtler half. Both line kinds still draw a line, and both draw it
        # in the same place -- which is the mistake `save.rs` warns about in
        # `line_rect`: a strikeout drawn at the bottom is an underline with the
        # wrong subtype, and nothing about it looks broken.
        "markband: draw a strikeout where an underline goes",
        "src/lib/markband.ts",
        "        top: quad.top + height / 2 - thickness / 2,",
        "        top: quad.bottom - thickness,",
        "centres a strikeout on the text",
    ),
    Mutation(
        # A fixed thickness rather than a fraction of the text. Right for body
        # text and a hairline across a heading, which is `LINE_FRACTION`'s whole
        # reason -- and the fixture that catches it has to be the 36 pt line,
        # since a 12 pt one at 7% is under a point either way.
        "markband: fix the line thickness instead of scaling it with the text",
        "src/lib/markband.ts",
        "  const thickness = height * LINE_FRACTION;",
        "  const thickness = 1;",
        "scales the line with the text rather than fixing it",
    ),
    Mutation(
        # Render the declared character even when the platform has said what the
        # key prints. The palette then advertises Cmd-backslash while the menu
        # bar, which resolves the key itself, shows Cmd-#: one application
        # disagreeing with itself about one shortcut, on the layout where the
        # chord could not be typed at all until this week.
        "keys: label a positional binding by its character rather than the key cap",
        "src/lib/keys.ts",
        "  const printed = binding.code === undefined ? undefined : PRINTED[binding.code];",
        "  const printed = undefined;",
        "names the key this keyboard shows, once the platform has said",
    ),
    Mutation(
        # Merge each answer into the last. A layout change then leaves the
        # previous layout's glyph on any position the new one does not name --
        # invisible until someone switches layouts, and wrong forever after.
        "keys: merge the printed keys instead of replacing them",
        "src/lib/keys.ts",
        "  for (const key of Object.keys(PRINTED)) delete PRINTED[key];",
        "",
        "replaces the whole map rather than merging into it",
    ),
    Mutation(
        # Drop the position path. `\\` is Option-Shift-7 on a German keyboard, so
        # the character path arrives with modifiers the binding refuses -- which
        # is why Cmd-backslash had never once worked there, measured on the
        # running application before this existed.
        "keys: match a binding by its character only",
        "src/lib/keys.ts",
        "  if (binding.code !== undefined && event.code === binding.code) return true;",
        "",
        "matches the physical key when the character is unreachable",
    ),
    Mutation(
        # Hoist the position check above the modifier checks. A position is not
        # a licence to ignore the rest of the chord: Shift-Cmd on the same key
        # would then be this binding too, and the both-directions Shift bug this
        # table already carries an entry for would be back on one binding.
        "keys: let the physical key win before the modifiers are read",
        "src/lib/keys.ts",
        "  const accel = event.metaKey || event.ctrlKey;\n  if (accel !== (binding.accel ?? false)) return false;",
        "  if (binding.code !== undefined && event.code === binding.code) return true;\n  const accel = event.metaKey || event.ctrlKey;\n  if (accel !== (binding.accel ?? false)) return false;",
        "keeps the modifier checks on the position path",
    ),
    Mutation(
        # Claim the character rather than the position in the menu. The two
        # disagree on any layout that is not the one the table was written on,
        # and the menu is where that disagreement becomes a wrong shortcut shown
        # to a reader.
        "keys: build the accelerator from the character even when a position is named",
        "src/lib/keys.ts",
        "  const key = binding.code ?? plainKey(binding.keys[0] ?? \"\");",
        "  const key = plainKey(binding.keys[0] ?? \"\");",
        "withholds a punctuation chord, because position is not character",
    ),
    Mutation(
        # The symmetric edit that looks obviously right and is not: Back and
        # Forward are as untypable on a German keyboard as Cmd-backslash was, so
        # naming their positions is the natural next step -- and BracketRight is
        # the `+` key there, which zoom-in already claims.
        "keys: give Forward the physical key a German keyboard prints + on",
        "src/lib/keys.ts",
        '  "nav.forward": { keys: ["]", "\\u2018"], accel: true },',
        '  "nav.forward": { keys: ["]", "\\u2018"], code: "BracketRight", accel: true },',
        "names no physical key that a German keyboard gives to another command",
    ),
    Mutation(
        # The one that matters most on this platform. A menu accelerator is
        # claimed by AppKit before the web view sees the key, so letting an
        # unmodified binding through puts bare `n` -- next page -- in the menu
        # bar, where it is taken out of the find field and every text input the
        # application ever grows. Nothing about the menu would look wrong.
        "keys: let a binding with no accelerator key into the menu",
        "src/lib/keys.ts",
        "  if (!binding.accel) return null;",
        "",
        "refuses a binding that holds no accelerator key",
    ),
    Mutation(
        # The second refusal, and it is a different fact: these five hold the
        # modifier and are still claimed by a text field. `handleWindowKey`
        # guards undo with `inTextField` for exactly this reason, and a menu
        # accelerator would undo that guard from outside the page.
        "menubar: claim the chords a text field needs",
        "src/lib/menubar.ts",
        "  if (id in NO_ACCELERATOR) return null;",
        "",
        "withholds a chord a text field claims",
    ),
    Mutation(
        # Let a punctuation key into the menu. An accelerator names a physical
        # key and `matches` reads the character it produced, so on the German
        # layout this advertises Cmd-# beside a command whose palette entry says
        # Cmd-backslash -- measured on the running application before this rule
        # existed, along with Cmd-O-umlaut for Back.
        "keys: claim a punctuation key whose character depends on the layout",
        "src/lib/keys.ts",
        "/^[A-Z0-9]$/",
        "/^[A-Z0-9\\\\]$/",
        "refuses a punctuation key, whose position is not its character",
    ),
    Mutation(
        # Guess at a key the table does not know. The parser accepts what it can
        # read and silently claims whatever chord it read, so this takes a
        # shortcut nobody chose rather than showing none.
        "keys: guess at a key the accelerator table cannot spell",
        "src/lib/keys.ts",
        "  const upper = key.toUpperCase();\n  return /^[A-Z0-9]$/.test(upper) ? upper : null;",
        "  const upper = key.toUpperCase();\n  return upper;",
        "refuses a key it cannot spell rather than guessing",
    ),
    Mutation(
        # Every item enabled. The menu then offers commands the palette
        # withholds -- Undo on an empty journal, Install update with no update
        # -- and choosing one does nothing, which reads as a broken menu.
        "menubar: build every item enabled",
        "src/lib/menubar.ts",
        "          enabled: command.enabled?.() ?? true,",
        "          enabled: true,",
        "reads enablement from the command rather than assuming it",
    ),
    Mutation(
        # Run a command whose guard is closed. The stale-menu case is real: the
        # enablement push is a round trip, so between an edit and its answer the
        # bar is one step behind, and this is what makes that a grey item rather
        # than a wrong action.
        "menubar: run a menu item whose command is withheld",
        "src/lib/menubar.ts",
        "  if (!(command.enabled?.() ?? true)) return false;",
        "",
        # Named at the *argument* case, not at the obvious one. This mutation
        # SURVIVED a test that ran a withheld plain command and expected false:
        # `registry.run` checks `enabled` as well, so both mechanisms produce
        # the same answer and neither is tested. An argument command never
        # reaches `run` -- it opens the palette -- so the guard is the only
        # thing standing there, and that is the branch a mutation can see.
        "refuses a withheld command that would have opened the palette",
    ),
    Mutation(
        # Run an argument command straight through the registry. `run` refuses
        # it for want of a value, so the item silently does nothing -- the exact
        # failure that looks like a menu bug and is not.
        "menubar: run a command that needs a value without asking for one",
        "src/lib/menubar.ts",
        "  if (command.argument) {\n    if (!palette) return false;\n    palette.askFor(id);\n    return true;\n  }",
        "",
        "opens the palette for a command that takes a value",
    ),
    Mutation(
        # Drop the separators. Purely cosmetic and worth pinning anyway: the
        # groups are what make a fifteen-item View menu readable, and nothing
        # else in the suite looks at the shape of a section.
        "menubar: drop every separator",
        "src/lib/menubar.ts",
        '      if (entry === SEPARATOR) return [{ kind: "separator" }];',
        "      if (entry === SEPARATOR) return [];",
        "keeps separators where the layout puts them",
    ),
    Mutation(
        # An empty enablement push. Every item then keeps whatever state it was
        # built with, forever -- so the menu is correct exactly until the first
        # edit and stale after it.
        "menubar: send an empty enablement update",
        "src/lib/menubar.ts",
        "      if (command) state[command.id] = command.enabled?.() ?? true;",
        "",
        "answers for every command in the layout and nothing else",
    ),
    Mutation(
        # A command missing from the layout, which is the state the whole
        # application was in: reachable from the palette, absent from the menu,
        # and nothing saying so. Deleting the page strip's own delete is the
        # case that started this.
        "menubar: leave a command out of the menu",
        "src/lib/menubar.ts",
        '      "edit.deletePage",\n',
        "",
        "gives every registered command a menu or a written reason",
    ),
    Mutation(
        # Lexicographic sort, which is what `Array.prototype.sort` does without
        # a comparator. Page 10 lands before page 2, `write_copy` writes a valid
        # PDF in an order nobody asked for, and no downstream check could see
        # it: the file opens, it has the right pages, and it is wrong.
        "pageranges: sort the slots the way JavaScript sorts by default",
        "src/lib/pageranges.ts",
        "  return { slots: [...slots].sort((a, b) => a - b) };",
        "  return { slots: [...slots].sort() };",
        "sorts numerically, so page 11 comes after page 3",
    ),
    Mutation(
        # Read a backwards range as the range it resembles. A reader who typed
        # `5-3` made a mistake, and this is the version that silently gives them
        # three pages instead of telling them.
        "pageranges: quietly correct a range that runs backwards",
        "src/lib/pageranges.ts",
        "    if (from > to) {\n      return { problem: `${from}-${to} runs backwards` };\n    }",
        "    if (from > to) {\n      return { slots: [] };\n    }",
        "refuses a range that runs backwards instead of correcting it",
    ),
    Mutation(
        # Off by one at the conversion. Every page a reader names comes out one
        # later, which on a range still produces the right *number* of pages --
        # so a check counting them passes.
        "pageranges: keep the reader's one-based numbers as slots",
        "src/lib/pageranges.ts",
        "      slots.add(one - 1);",
        "      slots.add(one);",
        "reads a single page as one zero-based slot",
    ),
    Mutation(
        # Exclusive at the top, which is what a reader of `2-4` does not mean
        # and what a half-open loop written from habit produces.
        "pageranges: make a range exclusive at its top end",
        "src/lib/pageranges.ts",
        "    for (let page = from; page <= to; page += 1) slots.add(page - 1);",
        "    for (let page = from; page < to; page += 1) slots.add(page - 1);",
        "reads a range inclusively at both ends",
    ),
    Mutation(
        # Accept anything `Number()` accepts. `2.0` becomes page 2, `1e1`
        # becomes page 10, and `+2` becomes page 2 -- none of which is a page
        # number a reader typed on purpose.
        "pageranges: accept every numeric literal rather than digits",
        "src/lib/pageranges.ts",
        '  if (!/^[0-9]+$/.test(text)) return `"${text}" is not a page number`;',
        "  if (Number.isNaN(Number(text))) return `not a page number`;",
        "refuses +2, which Number() would have accepted",
    ),
    Mutation(
        # Run the extract with whatever parsed, including nothing. The palette
        # guards this, so the mutation is aimed at the second line of defence --
        # the one that decides whether a defect writes a file.
        "appcommands: extract even when the range did not parse",
        "src/lib/appcommands.ts",
        "          if (!range.slots) return;\n          actions.extractPages(range.slots);",
        "          actions.extractPages(range.slots ?? []);",
        "refuse to extract what does not parse, and reach no action",
    ),
    Mutation(
        # Send the page's position instead of its identity. Identical on an
        # unedited document and wrong the moment a page moves, which is the whole
        # reason ids cross the boundary at all.
        "edits: name a page by its slot rather than by its id",
        "src/lib/edits.ts",
        "    const id = this.current.pages[page]?.id;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await invoke<EditState>(\"page_rotate\"",
        "    const id = page;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await invoke<EditState>(\"page_rotate\"",
        "sends the page's identity, not its position",
    ),
    Mutation(
        # The same defect on the command that removes a page, where it is worse:
        # a rotation aimed at the wrong page can be undone by looking at it, and
        # a deletion aimed at the wrong page cannot.
        "edits: delete a page by its slot rather than by its id",
        "src/lib/edits.ts",
        "    const id = this.current.pages[page]?.id;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await invoke<EditState>(\"page_delete\"",
        "    const id = page;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await invoke<EditState>(\"page_delete\"",
        "deletes by identity, not by position",
    ),
    Mutation(
        # Read the anchor out of the order that still holds the page being
        # moved. The model removes it *before* reading the anchor's position, so
        # this is one slot short for every move towards the back --- a drag that
        # lands just before where the reader dropped it.
        "edits: pick the move anchor out of the order the page is still in",
        "src/lib/edits.ts",
        "    const rest = this.current.pages.filter((_, slot) => slot !== from);",
        "    const rest = this.current.pages;",
        "turns a destination slot into the neighbour the model accepts",
    ),
    Mutation(
        # Name no anchor at all, which puts every moved page at the front. Half
        # of all one-slot moves then land correctly, and a suite reading only the
        # page count sees nothing wrong with any of them.
        "edits: move every page to the front whatever slot was asked for",
        "src/lib/edits.ts",
        "    const after = landing === 0 ? null : (rest[landing - 1]?.id ?? null);",
        "    const after = null;",
        "names the page after the one being moved for a single step back",
    ),
    Mutation(
        # The same defect in the accessibility tree, where its reader can least
        # easily notice it: a screen reader goes on reading the order the
        # document used to be in, and nothing in the tree says otherwise.
        "a11y: keep the built pages whenever the page count did not change",
        "src/lib/a11y.ts",
        "  setPages(pageCount: number): void {\n    this.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.pageCount) return;\n    this.pageCount = pageCount;",
        "rebuilds when the order changes and the page count does not",
    ),
    Mutation(
        # And in the search results, where it is a highlight over a passage that
        # is not there.
        "search: keep the matches whenever the page count did not change",
        "src/lib/search.ts",
        "  setPages(pageCount: number): void {\n    this.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.pageCount) return;\n    this.pageCount = pageCount;",
        "drops the matches when the order changes and the page count does not",
    ),
    Mutation(
        # Put the early return back. It was there, it was right for the deletion
        # it was written for, and it is what leaves the page strip showing the
        # old order after a move: same rows, same captions, the wrong pictures
        # under them, and no observable in the strip that says otherwise.
        "thumbnails: keep the strip whenever the page count did not change",
        "src/lib/thumbnails.ts",
        "  setPages(pageCount: number): void {\n    this.opts.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.opts.pageCount) return;\n    this.opts.pageCount = pageCount;",
        "throws its thumbnails away when the order changes and the count does not",
    ),
    Mutation(
        # Moved here from `mutate_viewer.py` on 2026-08-17, and the move is the
        # finding. Aimed at the window harness it SURVIVED a full run: the
        # separator exists only for a tagged block, `tagged.pdf` is the only
        # corpus with a structure tree, and none of its blocks wraps --- so no
        # fixture reached the branch and nothing could go red. Writing the test
        # that judges it found the branch had been broken since it was written.
        "a11y: hand a paragraph's lines over with no space between them",
        "src/lib/a11y.ts",
        "        if (wrote || piece) piece += \" \";",
        "        if (false) piece += \" \";",
        "joins the lines of one tagged paragraph with a space",
    ),
    Mutation(
        # The off-by-one the whole helper exists for. Reading the gap against
        # the order the page is still in, and handing that straight to the
        # model, leaves every drag towards the back of the document one slot
        # short --- and a drag one slot down does nothing at all.
        "thumbnails: drop a page where the gap says rather than where it lands",
        "src/lib/thumbnails.ts",
        "  return gap > from ? gap - 1 : gap;",
        "  return gap;",
        "takes one off a gap below the page, because the page has left it",
    ),
    Mutation(
        # The gap boundary moves from the middle of a row to its top edge, so
        # the page lands one slot from where the indicator was drawn --- which
        # is the failure a reader reads as the strip ignoring them.
        "thumbnails: take the gap from the row's top edge rather than its middle",
        "src/lib/thumbnails.ts",
        "  const gap = Math.round(contentY / rowHeight);",
        "  const gap = Math.floor(contentY / rowHeight);",
        "names the gap below the row the pointer is in the bottom half of",
    ),
    Mutation(
        # Every press becomes a drag, so a plain click on a thumbnail reorders
        # the document whenever the pointer moved by a pixel.
        "thumbnails: treat every press as a drag, however still the pointer",
        "src/lib/thumbnails.ts",
        "      if (Math.abs(event.clientY - press.startY) < DRAG_THRESHOLD) return;",
        "      if (false) return;",
        "does not reorder anything when the pointer barely moves",
    ),
    Mutation(
        # A second pointer aims the first one's drag. The guard is one clause
        # and reads like defensiveness; it is what keeps a trackpad's second
        # finger from choosing where the page lands.
        "thumbnails: let any pointer aim a drag another one started",
        "src/lib/thumbnails.ts",
        "    if (!press || event.pointerId !== press.pointerId) return;",
        "    if (!press) return;",
        "ignores a pointer that is not the one being dragged",
    ),
    Mutation(
        # `setPages` is what a drop's own edit comes back through, so completing
        # the drag there applies the reader's move a second time.
        "thumbnails: complete the drag when the document is rebuilt under it",
        "src/lib/thumbnails.ts",
        "    // Every row is about to be rebuilt, so a drag still in flight is aimed at",
        "    this.endDrag(true);\n    // Every row is about to be rebuilt, so a drag still in flight is aimed at",
        "runs no edit when the document is rebuilt under a live drag",
    ),
    Mutation(
        # The capture is never released, so the strip goes on receiving every
        # pointer event on the page for the rest of the session.
        "thumbnails: keep the pointer it captured after the drag ends",
        "src/lib/thumbnails.ts",
        "      if (this.host.hasPointerCapture(press.pointerId)) {",
        "      if (false) {",
        "releases the pointer it captured",
    ),
    Mutation(
        # The loop runs for the life of the drag on a strip that cannot move,
        # which is a frame callback per frame doing nothing.
        "thumbnails: keep the edge loop running when the strip cannot scroll",
        "src/lib/thumbnails.ts",
        "      if (this.host.scrollTop !== was) this.edgeFrame = requestAnimationFrame(step);",
        "      this.edgeFrame = requestAnimationFrame(step);",
        "stops the edge loop when the strip can scroll no further",
    ),
    Mutation(
        # The strip follows the page being read, and pressing a row navigates ---
        # so without this guard the content slides out from under a pointer that
        # has not moved, at the exact instant a drag begins. Four of the fourteen
        # window corpora caught it; the ten that did not had strips short enough
        # for the scroll to clamp.
        "thumbnails: follow the page being read even under a live press",
        "src/lib/thumbnails.ts",
        "    if (this.press) return;\n    const row = this.rows.get(page);",
        "    const row = this.rows.get(page);",
        "does not follow the page being read while a pointer is down on a row",
    ),
    Mutation(
        # Scrolling per event rather than per frame, so the speed follows the
        # pointer's report rate --- and a reader holding still at the edge stops
        # moving, which is the case the loop exists for.
        "thumbnails: scroll only while the pointer keeps moving",
        "src/lib/thumbnails.ts",
        "    this.edgeFrame = requestAnimationFrame(step);\n  }",
        "    step();\n  }",
        "scrolls the strip while the pointer rests against the bottom edge",
    ),
    Mutation(
        # Send a move that moves nothing. The model applies it, so the reader
        # gains an undo that visibly does nothing --- and a drag that ends where
        # it began becomes a journal entry.
        "edits: send a move onto the page's own slot",
        "src/lib/edits.ts",
        "    if (landing === from) return this.current;",
        "",
        "sends nothing for a move that changes no order",
    ),
    Mutation(
        # Answer a deleted page with a slot instead of nothing. Everything the
        # backend sends about a page --- a link, a comment, a destination --- then
        # lands on whichever page moved into the gap.
        "pages: answer a page that is gone with a slot rather than with nothing",
        "src/lib/pages.ts",
        "    return this.bySource.get(source);",
        "    return this.bySource.get(source) ?? source;",
        "says a deleted page is nowhere rather than answering with a slot",
    ),
    Mutation(
        # The fallback the class exists to refuse: right for every unedited
        # document, and asks for the wrong page in exactly the case it is for.
        "pages: fall back to the slot when it draws no page",
        "src/lib/pages.ts",
        "    return this.views[slot]?.source;",
        "    return this.views[slot]?.source ?? slot;",
        "says a slot past the end is nowhere rather than falling back to itself",
    ),
    Mutation(
        # Compare two orders by which page of the file is in each slot rather
        # than by identity. A page restored by an undo is then "the same page",
        # and the viewer takes the cheap path that only re-reads the turns.
        "pages: compare two orders by source rather than by identity",
        "src/lib/pages.ts",
        "    return this.views.every((page, slot) => page.id === other.views[slot]?.id);",
        "    return this.views.every((page, slot) => page.source === other.views[slot]?.source);",
        "is false when the same sources arrive under different identities",
    ),
    Mutation(
        # Hand `findIndex`'s -1 back as though it were a slot. The viewer's
        # `?? Math.min(...)` does not fire on it, so the reader is put at "slot
        # minus one" --- and the page that was deleted reads as a page that
        # moved somewhere impossible rather than as a page that is gone.
        #
        # Deliberately not the mutation this looked like it wanted, which was to
        # follow a page by its `source` instead of its `id`. Ids are allocated
        # one per baseline page, so on every document that exists today
        # `source == id - 1` and the two are the same function --- a variant
        # rather than a gap, which `docs/TRAPS.md` says to check for before
        # strengthening anything. It becomes distinguishable when a page can be
        # duplicated, and `docmodel.rs` says what has to be proved first.
        "pages: report a page that has gone as the slot before the first",
        "src/lib/pages.ts",
        "    return found === -1 ? undefined : found;",
        "    return found;",
        "says nothing for a page that is no longer there",
    ),
    Mutation(
        # Draw a link whose own page has been deleted. It is hit-tested against
        # whichever page moved into that slot, so a cross-reference appears in
        # the middle of a page that never had one.
        "pages: keep a link whose page has been deleted",
        "src/lib/pages.ts",
        "    const slot = pages.slotOf(link.page);\n    if (slot === undefined) continue;",
        "    const slot = pages.slotOf(link.page) ?? link.page;",
        "leaves out a link on a page that is gone",
    ),
    Mutation(
        # Leave a destination pointing at a page that is not in the document.
        # `goToDestination` then scrolls to whatever is in that slot.
        "pages: leave a destination pointing at a page that has gone",
        "src/lib/pages.ts",
        "  if (slot === undefined) return { kind: \"broken\" };",
        "  if (slot === undefined) return target;",
        "keeps a link whose destination is gone, and calls it broken",
    ),
    Mutation(
        # List a comment on a page nobody can see. It opens against the page that
        # moved into the slot, attributing somebody's note to a page they never
        # wrote on.
        "pages: keep a comment on a page that has been deleted",
        "src/lib/pages.ts",
        "    const slot = pages.slotOf(comment.page);\n    if (slot === undefined) continue;",
        "    const slot = pages.slotOf(comment.page) ?? comment.page;",
        "moves a comment to the slot its page is in and drops the rest",
    ),
    Mutation(
        # Carry each page's state by slot rather than by identity. Every page
        # below the gap then inherits the size and the tile epoch of the page
        # that used to be there --- invisible on a document whose pages are all
        # the same size, which is most of them, so this is asserted here rather
        # than left to a window check on a corpus that might not show it.
        "scroller: carry a page's learned size by its slot rather than by its page",
        "src/lib/scroller.ts",
        "      const was = at.get(page.id);",
        "      const was = this.order.indexOf(page);",
        "carries a learned size to wherever the page moved to",
    ),
    Mutation(
        # Keep what was painted across a change of order. Every tile is placed by
        # the slot it was requested for, so after a deletion the surviving pixels
        # are in the wrong places rather than merely stale --- and a reply still
        # in flight is adopted, because the generation bump that would have
        # dropped it went with the call.
        "scroller: keep the tiles when the page order changes",
        "src/lib/scroller.ts",
        "    this.clearTiles();\n    this.dropPlaceholders();\n    this.estimate = this.meanKnownSize();",
        "    this.dropPlaceholders();\n    this.estimate = this.meanKnownSize();",
        "drops a tile that was rendering when the order changed",
    ),
    Mutation(
        # Turn the view instead of the page. Every statement about the page that
        # was turned still holds; only its neighbour can tell.
        "scroller: apply a page's turn to the whole view",
        "src/lib/scroller.ts",
        "    return this.opts.turns + (this.pageTurns[page] ?? 0);",
        "    return this.opts.turns + (this.pageTurns[0] ?? 0);",
        "lays the turned page out sideways and leaves its neighbour alone",
    ),
    Mutation(
        # Lay the page out upright and render it turned. The box keeps the old
        # shape and the tile drawn into it is the new one.
        "scroller: leave the page's own turn out of the layout",
        "src/lib/scroller.ts",
        "    return displayedSize(this.pageSize(page), this.effectiveTurns(page));",
        "    return displayedSize(this.pageSize(page), this.opts.turns);",
        "lays the turned page out sideways and leaves its neighbour alone",
    ),
    Mutation(
        # Let the geometry decide what to invalidate. A half turn moves no box,
        # so the old pixels stay on screen upside down.
        "scroller: invalidate a turned page only when its box moves",
        "src/lib/scroller.ts",
        "    this.invalidatePage(page);\n    const moved = this.applySizes();",
        "    const moved = this.applySizes();",
        "discards a page's pixels even when its box does not move",
    ),
    Mutation(
        # Ask the renderer for the view's turn alone. The page is laid out
        # sideways and drawn upright inside its own box.
        "scroller: request a tile without the page's own turn",
        "src/lib/scroller.ts",
        "      turns: this.requestTurns(key.page),",
        "      turns: this.opts.turns,",
        "asks the renderer for the page's turn composed with the view's",
    ),
    Mutation(
        # Turn every page's text, not the one that was edited. Selection then
        # lands wrongly on every page except the one that was turned.
        "text: apply a page's turn to the whole cache",
        "src/lib/text.ts",
        "    const turns = this.turns + (this.extra.get(page) ?? 0);",
        "    const turns = this.turns + (this.extra.values().next().value ?? 0);",
        "turns only the page the edit named",
    ),
    Mutation(
        # Drop every turned view rather than the one page's. Correct and
        # wasteful, and invisible through `peek` --- only the accounting can see
        # it, which is why there is one.
        # Re-aimed 2026-08-18: `setPageCrop` drops the same page's turned view,
        # so the one-line anchor now matches twice. Widened to the two lines
        # above it, which are `setPageTurns`'s and nothing else's.
        "text: clear the whole turned cache when one page is turned",
        "src/lib/text.ts",
        "    else this.extra.set(page, next);\n    this.turned.delete(page);",
        "    else this.extra.set(page, next);\n    this.turned.clear();",
        "drops only that page's turned view when its turn changes",
    ),
    Mutation(
        # Both rotations clockwise. The command reaches the right action and
        # turns the page the wrong way.
        "commands: rotate the page clockwise whichever way was asked",
        "src/lib/appcommands.ts",
        "      run: () => actions.rotatePage(-1),",
        "      run: () => actions.rotatePage(1),",
        "rotate the page with the sign the reader asked for",
    ),
    Mutation(
        # Offer Save on any open document. Pressing it on one nobody edited
        # rewrites every object id in the file, which is a change to a document
        # the reader did not change.
        "commands: offer Save whenever a document is open",
        "src/lib/appcommands.ts",
        "      enabled: () => actions.viewer() !== null && actions.isDirty(),",
        "      enabled: withDocument,",
        "offer Save only once there is something to save",
    ),
    Mutation(
        # Drop the document half of Save's guard and keep the dirty half. The
        # command then survives the document being closed, because `dirty` is a
        # variable and nothing clears it.
        "commands: guard Save on the journal alone",
        "src/lib/appcommands.ts",
        "      enabled: () => actions.viewer() !== null && actions.isDirty(),",
        "      enabled: () => actions.isDirty(),",
        "withholds Save with no document, however dirty the model claims to be",
    ),
    Mutation(
        # Let the chord through with nothing to save. The reader who presses
        # Cmd-S by reflex on an untouched document gets the backend's refusal
        # as an error message.
        "commands: reach the save action on Cmd-S with nothing to save",
        "src/lib/appcommands.ts",
        "    if (actions.isDirty()) actions.saveDocument();",
        "    actions.saveDocument();",
        "does nothing on Cmd-S with nothing to save",
    ),
    Mutation(
        # Route a save in place through the copy path. That path refuses the
        # source outright, so Save could never write anything -- and the reader
        # is told their document changed under them.
        "edits: send a save in place to the copy command",
        "src/lib/edits.ts",
        '    await invoke<void>("save_document", { doc: this.doc, source });',
        '    await invoke<void>("save_copy", { doc: this.doc, source });',
        "names the open document and no destination when it saves in place",
    ),
    Mutation(
        # Offer Undo with an empty journal. The palette then teaches a reader
        # that the command does nothing.
        "commands: offer Undo whenever a document is open",
        "src/lib/appcommands.ts",
        "      enabled: () => actions.viewer() !== null && actions.canUndo(),",
        "      enabled: withDocument,",
        "are withheld while the journal is empty",
    ),
    Mutation(
        # One flag for both halves of the journal. A document with an edit and
        # nothing undone then offers Redo as well.
        "commands: guard Redo on there being something to undo",
        "src/lib/appcommands.ts",
        "      enabled: () => actions.viewer() !== null && actions.canRedo(),",
        "      enabled: () => actions.viewer() !== null && actions.canUndo(),",
        "are offered separately, each on its own half of the journal",
    ),
    Mutation(
        # Take Cmd-Z from the find field. A reader correcting a typo silently
        # undoes a page rotation instead, and nothing connects the two.
        "keys: take Cmd-Z out of the text field a reader is typing in",
        "src/lib/appcommands.ts",
        "  } else if (matches(\"edit.undo\", event) && title && !inTextField(event)) {",
        "  } else if (matches(\"edit.undo\", event) && title) {",
        "leaves Cmd-Z to the text field a reader is typing in",
    ),
    Mutation(
        # The half of the guard a tag-name check cannot cover: a contenteditable
        # element is a text field and is not an INPUT. Aimed at the field rather
        # than at the spelling, because the spelling is what makes the guard
        # testable here at all --- see the note on the function.
        # Re-pathed 2026-08-18: `inTextField` moved to `keys.ts`, because the
        # viewer's own handler needs it too and two copies of "what is a text
        # field" would be two chances to disagree. The mutation is unchanged and
        # so is the test it names --- Cmd-Z from the find bar.
        "keys: miss a contenteditable target",
        "src/lib/keys.ts",
        "  if (target.isContentEditable === true) return true;",
        "  if (false) return true;",
        "leaves Cmd-Z to the text field a reader is typing in",
    ),
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
        '      await invoke<EditState>("annot_note", { doc: this.doc, mark, note }),',
        '      await invoke<EditState>("annot_remove", { doc: this.doc, mark, note }),',
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
        "comments: answer every page's comments from one page's text",
        "src/lib/comments.ts",
        "  const wanted = items.filter((comment) => comment.page === page && needsWords(comment));",
        "  const wanted = items.filter((comment) => needsWords(comment));",
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
        "links: read a character box past the end of the array",
        "src/lib/text.ts",
        "  if (\n    left === undefined ||\n    top === undefined ||\n    right === undefined ||\n    bottom === undefined\n  ) {\n    return null;\n  }\n  if (!(right > left) || !(bottom > top)) return null;\n  return { x: (left + right) / 2, y: (top + bottom) / 2 };",
        "  return { x: ((left ?? 0) + (right ?? 0)) / 2, y: ((top ?? 0) + (bottom ?? 0)) / 2 };",
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
        "edits: send a mark's slot rather than its page id",
        "src/lib/edits.ts",
        "    if (!this.current.pages.some((view) => view.id === page)) return this.current;",
        "    const at = this.current.pages[page as number];\n"
        "    if (!at) return this.current;\n"
        "    page = at.id;",
        "sends the page's id rather than its slot when a mark is made",
    ),
    Mutation(
        # Merge each reply's marks into the cache instead of replacing it. Every
        # visible behaviour is identical until an undo, which is the one moment
        # the cache has to shrink -- and a mark that stays on screen after being
        # undone is the failure undo exists to prevent.
        "edits: merge the marks a reply carries into the ones already held",
        "src/lib/edits.ts",
        "  private adopt(state: EditState): EditState {\n    this.current = state;",
        "  private adopt(state: EditState): EditState {\n    this.current = {\n      ...state,\n      marks: [...this.current.marks, ...state.marks],\n    };",
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
        "      turns: this.scroller.effectiveTurns(page),",
        "      turns: this.turns,",
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
        "      turns: this.scroller.effectiveTurns(page),",
        "      turns: this.scroller.effectiveTurns(page) + 1,",
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
        "          displayedSize(shown, -this.scroller.effectiveTurns(page)),",
        "          displayedSize(shown, -this.turns),",
        "learns a page's size in the document's space, not the turned view's",
    ),
]

#: Suites this harness runs. Named once: `run_tests` and the name check below
#: must agree, or the second validates a list the first never runs.
# --- the keyboard route to a mark ----------------------------------------
MUTATIONS += [
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
        "    if (inTextField(event)) return;",
        "    if (false) return;",
        "scroll the page when they came from the page and not when they did not",
    ),
    Mutation(
        # The other direction: refuse *every* key, which passes the refusal half
        # of each pair above and fails its control. Without this, a guard that
        # made the viewer deaf would be indistinguishable from one that works.
        "viewer: refuse every key, whether or not it went to a field",
        "src/lib/viewer.ts",
        "    if (inTextField(event)) return;",
        "    return;",
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
MUTATIONS += [
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
        "      this.scroller.notePageSize(slot, {\n        width_pt: at.width_pt,\n        height_pt: at.height_pt,\n      });",
        "",
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
MUTATIONS += [
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
        "            this.opts.onErased?.(mark, [...strokes].sort((a, b) => a - b));",
        "            this.opts.onErased?.(mark, [...strokes]);",
        "reports the strokes in order however the hand crossed them",
    ),
    Mutation(
        # Leave the pen armed when the eraser is taken up. Two tools are then
        # live at once and the next press has to guess which one the reader
        # meant -- and it guesses the eraser, because that branch is first.
        "viewer: arm the eraser without putting the pen away",
        "src/lib/viewer.ts",
        "    this.drawKind = null;\n    this.inking = null;\n"
        "    this.cropping = false;\n    this.erasing = true;",
        "    this.erasing = true;",
        "puts the pen away, and the pen puts it away",
    ),
    Mutation(
        # Escape reaches only the pen's states, which is what it did when the
        # eraser was written: a reader with the eraser armed and nothing on
        # screen but a cursor cannot get out of the mode.
        "viewer: let Escape past an armed eraser",
        "src/lib/viewer.ts",
        "        this.erasing ||\n        this.doomed",
        "        false",
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
        "          for (const mark of swept.whole) this.opts.onUnmarked?.(mark);",
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
MUTATIONS += [
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

# --- a colour a reader can choose ----------------------------------------
MUTATIONS += [
    Mutation(
        # Ignore what the reader picked. Every mark comes out in its kind's own
        # colour, the swatch row and the status line both say green, and the
        # only place the disagreement shows is the page.
        "markcolors: let a chosen colour lose to the kind's own",
        "src/lib/markcolors.ts",
        "  return chosen ?? MARK_COLORS[kind];",
        "  return MARK_COLORS[kind];",
        "sends the reader's colour for every kind, once one is chosen",
    ),
    Mutation(
        # Resolve the default swatch against one kind rather than the mark's.
        # A red underline recoloured "default" becomes yellow -- a 1.3 pt yellow
        # rule on white paper, which is close to invisible.
        "viewer: read the default swatch as the highlight's colour, not the mark's",
        "src/lib/viewer.ts",
        "    const want = colorFor(mark.kind, color);",
        '    const want = colorFor("highlight", color);',
        "reads the default swatch as the open mark's own kind's colour",
    ),
    Mutation(
        # Send the colour even when the mark already wears it. Every press of an
        # already-ringed swatch is then an undo step for nothing, which a reader
        # discovers by pressing undo and watching nothing change.
        "viewer: recolour a mark that is already the colour asked for",
        "src/lib/viewer.ts",
        "    if (sameColor(mark.color, want)) return false;",
        "    if (false) return false;",
        "asks for nothing when the mark is already that colour",
    ),
    Mutation(
        # Take the first mark held rather than the one whose note is open. The
        # palette's `Colour:` commands then recolour a mark the reader is not
        # looking at, and do it with no note open at all.
        "viewer: recolour the first mark held rather than the open one",
        "src/lib/viewer.ts",
        "    const id = this.markNote.openId;\n"
        "    if (id === null) return false;\n"
        "    const mark = this.marks.find((held) => held.id === id);\n"
        "    if (!mark) return false;",
        "    const mark = this.marks[0];\n"
        "    if (!mark) return false;\n"
        "    const id = mark.id;",
        "asks for nothing when no note is open",
    ),
    Mutation(
        # Ring nothing. The row still works and no longer says which colour the
        # mark is, so a reader pressing the one it already wears gets an undo
        # step -- the guard below reads this attribute.
        "markpopup: ring no swatch at all",
        "src/lib/markpopup.ts",
        '      button.setAttribute("aria-pressed", String(sameColor(rgb, color)));',
        '      button.setAttribute("aria-pressed", String(false));',
        "shows which colour the mark it is open on is drawn in",
    ),
    Mutation(
        # Drop the no-op guard. The ring is what invites a second press, so the
        # swatch already on is the one a reader hits twice.
        "markpopup: send a colour the mark already wears",
        "src/lib/markpopup.ts",
        '        if (button.getAttribute("aria-pressed") === "true") return;',
        "        if (false) return;",
        "sends a colour the mark is not, and nothing for the one it is",
    ),
    Mutation(
        # Wait for the model to answer before moving the ring. There is no model
        # in the harness, and in the application the reply is a round trip -- so
        # a press looks ignored for as long as that takes.
        "markpopup: leave the ring where it was until the model answers",
        "src/lib/markpopup.ts",
        "        this.showColor(rgb);\n        this.opts.onRecolor(id, rgb);",
        "        this.opts.onRecolor(id, rgb);",
        "rings the colour that was pressed before the model has answered",
    ),
    Mutation(
        # Build all seven commands round one swatch. This is the shape this
        # repository's own note about `movePage` warns about: seven commands out
        # of one `map`, so a wrong argument is wrong seven times at once and
        # every one of them still runs.
        "appcommands: give every colour command the same swatch",
        "src/lib/appcommands.ts",
        "      run: () => actions.setMarkColor(entry.id),",
        '      run: () => actions.setMarkColor("yellow"),',
        "every colour command asks for its own colour",
    ),
]

MUTATIONS += [
    Mutation(
        # Sort the panel's rows the way they arrived, which is the order the
        # marks were *made*. Plausible, and it is the whole reason `markRows`
        # wraps `markWalk` rather than sorting: the keyboard walk and the panel
        # are two ways to the same marks, and a reader uses both in the same
        # minute. Aimed at the call, not at `markWalk` itself --- a rule
        # everything agrees about and nothing consults is the trap about a guard
        # only covered when a mutation removes its call.
        "markRows: list the marks in the order they were made",
        "src/lib/pages.ts",
        "  for (const step of markWalk(items, pages)) {",
        "  for (const step of items.map((mark, page) => ({ id: mark.id, page }))) {",
        "lists marks in the walk's order",
    ),
    Mutation(
        # Drop whatever the walk could not place. The panel then silently omits
        # a mark, which tells a reader their mark is gone --- and the notice
        # above the list, which counts the same rows, agrees that nothing is
        # missing.
        "markRows: drop a mark the walk could not place",
        "src/lib/pages.ts",
        "    if (left.has(mark.id)) rows.push({ mark, page: null });",
        "    if (false) rows.push({ mark, page: null });",
        "keeps a mark the walk could not place",
    ),
    Mutation(
        # Name each kind with the wire's own word. "textbox" and "note" reach the
        # panel, while the box that opens on the same mark says "Text box" and
        # "Comment" --- two spellings of one thing in one window, which is what
        # having a second table would eventually produce anyway.
        "marklist: label a row with the serde name rather than the reader's word",
        "src/lib/marklist.ts",
        "    kind.textContent = nameOf(mark.kind);",
        "    kind.textContent = mark.kind;",
        "calls each kind what the note box calls it",
    ),
    Mutation(
        # Put a row's first line in unflattened. It used to aim at a private
        # helper that only the note went through, and the comment here said so:
        # every kind but one is unaffected, because only a text box's note
        # routinely has newlines in it. `flatten` now takes the covered words as
        # well --- which come off a page and are *always* several lines --- so
        # this reddens three tests where it used to redden one, and a fixture of
        # highlights can see it after all.
        "marklist: draw a row's first line with its own line breaks in it",
        "src/lib/rowline.ts",
        '  return text.replace(/\\s+/g, " ").trim();',
        "  return text.trim();",
        "flattens a text box's own lines",
    ),
    Mutation(
        # List a noted mark by the words it sits on instead of by the note. The
        # substitution rule with its two candidates the wrong way round, which is
        # the version anyone would write who thought the covered words were the
        # more informative of the two --- they are, for the eight rows in nine
        # that have no note, and never for the ninth.
        "marklist: list a mark by the words it covers rather than its note",
        "src/lib/rowline.ts",
        "  if (wrote) return { text: wrote, own: true };",
        "  if (wrote) return { text: flatten(covered), own: true };",
        "prefers what the reader typed over what the mark covers",
    ),
    Mutation(
        # Call the document's words the reader's own. Nothing on the row moves
        # except the one flag that says whose sentence it is --- so the text is
        # right, the panel looks right, and a reader cannot tell a phrase they
        # wrote from a phrase they highlighted.
        "marklist: draw the covered words as though the reader had typed them",
        "src/lib/rowline.ts",
        "  if (words) return { text: words, own: false };",
        "  if (words) return { text: words, own: true };",
        "lists a mark nobody typed on by the words it covers",
    ),
    Mutation(
        # Ask for the words by the page rather than by the mark. With one row on
        # the list this is invisible --- the lookup misses, the row says nothing
        # was typed, and that is what a mark with no covered words says anyway.
        "marklist: look a row's covered words up by its page",
        "src/lib/marklist.ts",
        "    const line = rowLine(mark.note, this.opts.coveredFor(mark.id), NO_NOTE);",
        "    const line = rowLine(mark.note, this.opts.coveredFor(mark.page), NO_NOTE);",
        "asks for each row's words by that row's id",
    ),
    Mutation(
        # Let the keyboard activate a row that is on no page. The pointer is
        # still refused, because a disabled row gets no listener at all --- so
        # this is the half that has to be written twice and is the easy half to
        # forget.
        "marklist: let Enter navigate to a mark that is on no page",
        "src/lib/marklist.ts",
        "        if (from !== null && this.placed(from)) this.opts.onPick(from);",
        "        if (from !== null) this.opts.onPick(from);",
        "refuses to navigate to a mark that is on no page",
    ),
    Mutation(
        # Guard Delete the way Enter is guarded. Enter refuses an unplaced row
        # because there is nowhere to scroll to, and copying that reasoning onto
        # removal is the plausible mistake: it would strand every mark the model
        # could not place, which is the whole reason this control exists.
        "marklist: refuse to take off a mark that is on no page",
        "src/lib/marklist.ts",
        "        if (from !== null) this.opts.onRemove(from);",
        "        if (from !== null && this.placed(from)) this.opts.onRemove(from);",
        "removes with Delete and with Backspace, including a mark on no page",
    ),
    Mutation(
        # Let a key on the control reach the list's own handler. `idOf` finds no
        # id on a button, so the fallback aims it at the focused row and Enter
        # opens the note instead of taking the mark off.
        "marklist: let a key on the remove control fall through to the row",
        "src/lib/marklist.ts",
        '    if (part === "remove") return;',
        "    if (false) return;",
        "leaves a key pressed on the control to the control",
    ),
    Mutation(
        # Give the control to placed rows only. The row still lists the mark and
        # still says it is on no page; there is simply no way left to remove it.
        "marklist: give the remove control to placed rows only",
        "src/lib/marklist.ts",
        "    element.append(swatch, page, text, remove);",
        "    element.append(swatch, page, text);\n    if (row.page !== null) element.append(remove);",
        "offers the control on a mark that is on no page, which nothing else can reach",
    ),
    Mutation(
        # Name the control with the kind as the second line spells it. The
        # popup's own button lowers it, so this is the drift that makes one
        # affordance read as two.
        "marklist: name the remove control in the second line's case",
        "src/lib/marklist.ts",
        "    const label = `Remove ${nameOf(mark.kind).toLowerCase()}`;",
        "    const label = `Remove ${nameOf(mark.kind)}`;",
        "names the control for the kind of mark it is on",
    ),
    Mutation(
        # Keep a selection whose mark has gone. Ids are the model's and it hands
        # them out again, so the row that lights up next is whichever mark
        # inherits the number.
        "marklist: keep the selection after the mark it was on is removed",
        "src/lib/marklist.ts",
        "    if (this.selected !== null && !this.rows.some((row) => row.mark.id === this.selected)) {",
        "    if (false) {",
        "drops the selection when the mark it was on goes",
    ),
    Mutation(
        # Report an open on every `show`, including one that reopens the box on
        # the mark it is already on. The panel's selection scrolls itself into
        # view, so the visible cost is a list that jumps whenever a reader
        # presses the mark they are already reading.
        "markpopup: report an open for a box that was already on that mark",
        "src/lib/markpopup.ts",
        "    if (was !== mark.id) this.opts.onOpen(mark.id);",
        "    this.opts.onOpen(mark.id);",
        "says nothing for a close that closed nothing, or an open on the same mark",
    ),
    Mutation(
        # Say nothing when the box closes. The panel keeps a row marked as the
        # one being read after there is nothing open, which is the state the
        # four `hide` calls in `viewer.ts` would each have had to remember to
        # report --- and the reason this fires from the primitive instead.
        "markpopup: close the box without saying it closed",
        "src/lib/markpopup.ts",
        '    this.input.value = "";\n    this.opts.onOpen(null);',
        '    this.input.value = "";',
        "reports which mark the box is on, including that it is on none",
    ),
    # The six below mutate `README.md` rather than a module, and that is the
    # point: the README is the one document a stranger reads, and the drift it
    # has actually suffered is a bullet going stale rather than a function going
    # wrong. `src/lib/readme.test.ts` is what reads it.
    Mutation(
        # The error the whole check exists for, and the one its Python
        # predecessor could not see: `edit.stamp.approved` is built in a loop, so
        # a regex over `appcommands.ts` finds no such literal and reports the
        # claim clean. Measured green there on 2026-08-24 before this replaced
        # it.
        "readme: say a shipped command is not built",
        "README.md",
        "  <!-- not-built: edit.editText -->",
        "  <!-- not-built: edit.stamp.approved -->",
        "claims nothing absent that the application registers",
    ),
    Mutation(
        # A capability that ships and is never mentioned. This is the direction
        # the first check could not reach at all: nothing in the README claims
        # anything about stamps, so there is nothing to contradict.
        "readme: stop saying the stamps exist",
        "README.md",
        "  <!-- built: edit.stamp.approved edit.stamp.confidential edit.stamp.draft edit.stamp.final -->",
        "  <!-- built: edit.stamp.approved -->",
        "classifies every registered command",
    ),
    Mutation(
        # A renamed command leaves its marker pointing at nothing, and a bullet
        # whose id no longer exists is a bullet that has quietly stopped being
        # checked.
        "readme: claim a command that is not registered",
        "README.md",
        "  <!-- built: file.print -->",
        "  <!-- built: file.printDocument -->",
        "claims nothing built that the application does not register",
    ),
    Mutation(
        # Two bullets claiming one command is one of them saying nothing, and it
        # is how a bullet comes to look covered while its own claim went missing.
        "readme: claim one command in two bullets",
        "README.md",
        "  <!-- built: file.properties -->",
        "  <!-- built: file.properties file.print -->",
        "claims each command once, in one direction",
    ),
    Mutation(
        # A `built:` marker under "Not built yet" says the opposite of the prose
        # around it. Inserted rather than moved, so the id stays claimed in the
        # prose too and this is the only check that can go red.
        "readme: claim a command as built inside the not-built list",
        "README.md",
        "- **True redaction** with an automatic post-save verification pass",
        "- **True redaction** <!-- built: file.print --> with an automatic post-save verification pass",
        "keeps the absence claims out of the prose and the built claims out of the list",
    ),
    Mutation(
        # Claim a command in the README that is also excluded from it. Both
        # tables then describe the same id, and whichever is read second is
        # wrong; the id is registered and claimed once, so nothing else fires.
        "readme: claim a command that is also excluded by name",
        "README.md",
        "  <!-- built: view.zoomIn",
        "  <!-- built: file.open view.zoomIn",
        "does not both claim and exclude a command",
    ),
    # The three below reintroduce the defect a reader reported on 2026-08-25:
    # this repository's *prose* spelling of an em dash, `---`, inside a string
    # the window draws. Eighteen had shipped, in three modules, and the reason
    # nothing caught them is that a doc comment two lines above says `---` and
    # means it. `src/lib/readertext.test.ts` is what reads these.
    #
    # Three modules rather than one on purpose. The eighteen were confined to
    # properties, update and recovery, so a mutation in any of those tests the
    # check where it was already looking; `outline.ts` is the one aimed at a
    # module that was never broken, which is what says the check covers the
    # population rather than the sample it was written from.
    Mutation(
        # Speak on every start. This runs once per launch and the line would say
        # that nothing happened -- which is how a reader learns to skip the line
        # that matters, and this one exists to mean "a webview reloaded".
        "orphans: report a start that released nothing",
        "src/lib/orphans.ts",
        "    if (held > 0) {",
        "    if (held >= 0) {",
        "says nothing when there was nothing to release",
    ),
    Mutation(
        # Let a housekeeping failure reach the reader. They have just opened the
        # application and are waiting for a page; the documents stay held either
        # way, so this trades an invisible leak for a visible error about
        # something nobody asked for.
        "orphans: raise a housekeeping failure at the reader",
        "src/lib/orphans.ts",
        "    return -1;",
        "    throw e;",
        "never rejects, so a start is not blocked by housekeeping",
    ),
    Mutation(
        "properties: spell a signature's title with the prose dash",
        "src/lib/properties.ts",
        'const title = signature.field ? `Signature — ${signature.field}` : "Signature";',
        'const title = signature.field ? `Signature --- ${signature.field}` : "Signature";',
        "holds no prose dash outside the separator sentinel",
    ),
    Mutation(
        "update: spell the ready-to-restart notice with the prose dash",
        "src/lib/update.ts",
        "return `Version ${state.version} is ready — restart to finish`;",
        "return `Version ${state.version} is ready --- restart to finish`;",
        "holds no prose dash outside the separator sentinel",
    ),
    Mutation(
        "outline: spell a refused action with the prose dash",
        "src/lib/outline.ts",
        'return "opens a web link — not followed";',
        'return "opens a web link --- not followed";',
        "holds no prose dash outside the separator sentinel",
    ),
]

TEST_FILES = [
    "src/lib/text.test.ts",
    "src/lib/clicks.test.ts",
    "src/lib/commands.test.ts",
    "src/lib/keys.test.ts",
    "src/lib/search.test.ts",
    "src/lib/textcache.test.ts",
    "src/lib/results.test.ts",
    "src/lib/recents.test.ts",
    # Added 2026-08-19 with the recovery rules, in the same edit as the
    # mutations rather than after them. A test file absent from this list makes
    # every mutation aimed at it report SURVIVED, and the guard that refuses an
    # unknown test name cannot help: the name resolves, it just never runs.
    "src/lib/recovery.test.ts",
    "src/lib/zoom.test.ts",
    "src/lib/reading.test.ts",
    "src/lib/a11y.test.ts",
    "src/lib/searchmapping.test.ts",
    "src/lib/comments.test.ts",
    "src/lib/commentlist.test.ts",
    "src/lib/commentpopup.test.ts",
    "src/lib/links.test.ts",
    # The window reads `ViewerStatus` and the other viewer tests read the
    # accessors, so this is the only file where a mutation to `report`'s own
    # summary can go red. It was listed a second time lower down on 2026-08-22,
    # beside `viewermove.test.ts`, by somebody who needed it for exactly that and
    # did not notice it was already here --- harmless to vitest, invisible in a
    # diff of 400 mutations, and the reason the gate below refuses a repeat.
    "src/lib/viewer.test.ts",
    "src/lib/edits.test.ts",
    "src/lib/scroller.test.ts",
    "src/lib/appcommands.test.ts",
    # Added 2026-08-19 with the version display. `update.ts` had been covered by
    # no mutation since it was written, so its suite had never been here --- and
    # the sixth time this list has been forgotten is the sixth time the refusal
    # is what said so rather than the mutation reporting SURVIVED. Worth reading
    # as a pattern rather than as six accidents: a new module's suite reaches
    # this list only when somebody writes a mutation for it, which is a step
    # later than writing the tests, so the gap is the normal case and not the
    # careless one.
    "src/lib/update.test.ts",
    # Added 2026-08-17 with the page strip's reset. `thumbnails.ts` had been
    # covered by no mutation, so its suite had never been in this list --- and
    # the mutation written for the move increment named a test the harness
    # could not see. It said so and refused to start, which is the third time
    # that guard has caught a list this file forgot to grow.
    "src/lib/thumbnails.test.ts",
    # Added 2026-08-17 with `pages.ts`. This list is what the harness runs and
    # what its name cross-check reads, so a suite missing from it makes every
    # mutation naming one of its tests unprovable --- the check said so for
    # seven of them rather than reporting them survived, which is that guard
    # doing its job.
    "src/lib/pages.test.ts",
    # Added 2026-08-20 with the duplicate-name guard. Seventh time, and the
    # pattern the note above names holds again: `checkreport.ts` had been
    # covered by no mutation, so its suite had never been here, and the first
    # mutation written for it named a test the harness could not see. It
    # refused rather than reporting SURVIVED, which is the whole value of the
    # guard -- a mutation that cannot go red and a mutation nothing catches are
    # indistinguishable from the verdict alone.
    "src/lib/checkreport.test.ts",
    # Added 2026-08-21 with the properties dialog. Eighth time, and it happened
    # exactly as the note above predicts: the tests were written first, the
    # mutations second, and this list is edited only by whoever writes the
    # second. All ten mutations named tests the harness could not see, and it
    # refused all ten rather than reporting them survived.
    "src/lib/properties.test.ts",
    # Added 2026-08-17 with extract. The guard fired a fourth time, for five
    # mutations at once: every one named a `pageranges.test.ts` test and the
    # harness could not see the file, so it refused to start rather than
    # reporting all five SURVIVED. Four out of four times this list has been
    # forgotten, the refusal is what said so -- which is the argument for
    # keeping it loud rather than making it infer the files from the mutations.
    "src/lib/pageranges.test.ts",
    # Added 2026-08-17 with the menu bar. Added *before* the mutations rather
    # than after the guard fired for a fifth time, which is the whole of what
    # four previous entries here are about.
    "src/lib/menubar.test.ts",
    # Added 2026-08-18 with the page turn's placement, before writing a single
    # mutation below --- which is what the six entries above are collectively
    # about, and the second time in two increments it was done in that order.
    "src/lib/viewerturns.test.ts",
    # Added 2026-08-18 with the note on a mark. Sixth time: the five mutations
    # below `markpopup.ts` all named tests in a file this list did not have, and
    # the guard refused to start rather than calling them survivors. Adding the
    # file first would have been the lesson of the four entries above; adding it
    # second is at least the guard proving itself again.
    "src/lib/markpopup.test.ts",
    # Added 2026-08-18 with the keyboard walk through marks, before writing the
    # mutations rather than after the guard fired for a seventh time.
    "src/lib/viewermarks.test.ts",
    # Added 2026-08-18 with the crop, before writing the mutations.
    "src/lib/crop.test.ts",
    "src/lib/viewercrop.test.ts",
    # Added 2026-08-19 with the mark bands, before writing the mutations. The
    # rule it covers shipped wrong --- every kind drawn as a highlight while the
    # document was open --- so the point of the entries below is that the exact
    # shipped shape is one of them.
    "src/lib/markband.test.ts",
    # Added 2026-08-19 with the drag primitive and the box, before the
    # mutations. Ninth entry, ninth time in that order.
    "src/lib/drag.test.ts",
    "src/lib/viewerdraw.test.ts",
    # Added 2026-08-20 with the marks panel, before writing the mutations. Tenth
    # time in that order, and it holds the tests for `markRows` too --- which
    # lives in `pages.ts`, already listed, so this entry is not what makes those
    # runnable. It is what makes them *visible*: a mutation naming a test in an
    # unlisted file is refused rather than run, and the refusal names the test,
    # not the file it could not find.
    "src/lib/marklist.test.ts",
    # Added 2026-08-22 with dragging a mark to move it --- and *after* writing the
    # mutations, which is the wrong order and is why it is worth a line: the run
    # refused all seven with "no test here is named ...", correctly, because a
    # file the harness was not told about is a file whose tests cannot go red.
    # Eleventh time this list has grown, and the guard has caught the omission
    # every time.
    "src/lib/viewermove.test.ts",
    # Added 2026-08-23 while cutting 26.8.8, and *after* the mutations: the run
    # refused all seven of them --- three under `unlock.ts` and four under
    # `passworddialog.ts` --- with "no test here is named ...", for tests both
    # files plainly define. Twelfth time, and the note ten entries above predicts
    # it exactly: the tests are written first and this list is edited only by
    # whoever writes the mutations, so the gap is the normal case. Twelve
    # omissions, twelve refusals, no SURVIVED --- which is the guard earning its
    # keep and also the argument for deriving this list from a glob instead. Not
    # done here, because widening the name set on the day a release is cut can
    # surface a duplicate test name and refuse the run for an unrelated reason.
    "src/lib/unlock.test.ts",
    "src/lib/passworddialog.test.ts",
    # Added 2026-08-24 with the six README mutations above, in the same edit
    # rather than after them --- which is the thirteenth time this list has been
    # the thing that was forgotten, and the first time it was not.
    "src/lib/readme.test.ts",
    # Added 2026-08-25 with the three prose-dash mutations above, in the same
    # edit for the same reason as the entry above it.
    "src/lib/readertext.test.ts",
    # Added 2026-08-25 with the two orphans mutations above, in the same edit.
    "src/lib/orphans.test.ts",
]

#: The suites this harness deliberately does NOT run, and why for each.
#:
#: `TEST_FILES` above is short on purpose --- every entry is paid for on every
#: one of the ~400 mutations below, so listing the whole tree would slow each
#: run to prove nothing about modules no mutation touches. The cost of keeping
#: it short is that it drifts, and the entries above record twelve times it did:
#: the tests are written first and this list is edited only by whoever writes
#: the mutations, so a new module's suite arrives here a step late.
#:
#: Twelve times the harness's own guard caught it, correctly, and each catch
#: cost a run that had already started. `scripts/check_mutation_test_files.py`
#: is what makes that a gate instead: it asks the same question against the
#: same source of names, in seconds, before anything is mutated. This table is
#: the other half --- a suite is either run or excluded *with a reason*, so a
#: file that is neither is a finding rather than an omission nobody can see.
#:
#: The last entry above argued for deriving `TEST_FILES` from a glob instead,
#: and deferred it because widening the name set can surface a duplicate test
#: name and refuse a run for an unrelated reason. That objection still holds and
#: this does not touch it: the gate changes what is *checked*, never what runs.
UNMUTATED = {
    # Its own assertions are never mutated. The three mutations aimed at
    # `rowline.ts` expect tests in `marklist.test.ts`, which is listed, so they
    # can go red --- this file is the one place where a defect in `rowline.ts`
    # would be caught by an assertion no mutation has ever broken.
    "src/lib/rowline.test.ts": "no mutation aims at src/lib/rowline.ts's own suite",
    # The ten below are the same shape as each other: the module has tests and
    # no mutation is aimed at it, so running its suite would cost time in every
    # mutation run and prove nothing. Writing one is what moves the entry --- and
    # the moment a mutation names a test in one of these files, the gate refuses
    # it, which is the twelve-times failure caught before the run rather than
    # twenty minutes into it.
    "src/lib/backoff.test.ts": "no mutation aims at src/lib/backoff.ts",
    "src/lib/contextmenu.test.ts": "no mutation aims at src/lib/contextmenu.ts",
    "src/lib/degraded.test.ts": "no mutation aims at src/lib/degraded.ts",
    "src/lib/lifetime.test.ts": "no mutation aims at src/lib/lifetime.ts",
    "src/lib/outline.test.ts": "no mutation aims at src/lib/outline.ts",
    "src/lib/paths.test.ts": "no mutation aims at src/lib/paths.ts",
    "src/lib/serial.test.ts": "no mutation aims at src/lib/serial.ts",
    "src/lib/session.test.ts": "no mutation aims at src/lib/session.ts",
    "src/lib/sidebar.test.ts": "no mutation aims at src/lib/sidebar.ts",
    "src/lib/tiles.test.ts": "no mutation aims at src/lib/tiles.ts",
}

FAILED_TEST = re.compile(r"^\s*(?:x|×)\s+(.*?)(?:\s+\d+ms)?$", re.M)
TEST_NAME = re.compile(r"^\s*[✓x×]\s+(\S+\.test\.ts)\s*>\s*(.*?)(?:\s+\d+ms)?$", re.M)
#: vitest's own count, and it must not require anything to have PASSED.
#:
#: This read `(?:(\d+) failed)?.*?(\d+) passed` until 2026-08-21, which was
#: true of every run it had ever seen and stopped being true the moment the run
#: was narrowed to one file: `Tests  2 failed (2)` has no `passed` segment at
#: all, because in that file nothing did. The regex then matched nothing, the
#: harness reported `no summary line -- the run did not finish`, and a mutation
#: its test had caught perfectly read as a broken run.
#:
#: The lookahead is the strict half and is deliberate: a transform error prints
#: `Tests  no tests` and must stay unreadable rather than parse as zero
#: failures, which would report SURVIVED for a run that never executed.
SUMMARY = re.compile(r"^\s*Tests\s+(?=.*(?:failed|passed))(.+)$", re.M)

#: The failing count inside that line, absent when everything passed.
SUMMARY_FAILED = re.compile(r"(\d+) failed")


def npx() -> str:
    """Resolves npx, which is `npx.cmd` on Windows and not on PATH as `npx`."""
    return shutil.which("npx") or "npx"


def run_tests(files: list[str] | None = None) -> tuple[set[str], int | None, int, str]:
    """Runs the suite, returning the failed names, the summary's count, the line count and the log.

    The names come back as a SET and the line count separately, because they are
    different quantities and the cross-check below needs the second one. Thirteen
    test names are defined in more than one file --- `closes on Escape`,
    `is one tab stop`, `normalises a negative turn` and ten others --- so a
    mutation that reddens two of them collapses to one name while vitest counts
    two, and the cross-check condemned the harness for a discrepancy the suite
    had put there. `all_test_names` had already learned that duplicates exist and
    returns a list of files per name; this end of the same run had not.

    `files` narrows the run to the file the mutation's own test lives in, which
    vitest is told by name rather than by any table kept here -- the control
    run's verbose listing prints the file beside every test, and that mapping is
    what the caller passes back in. The whole file rather than `-t <name>`, so a
    mutation that reddens three tests in it still reports three.

    The caller runs every file anyway whenever the narrow run finds nothing red.
    That is the case where the answer matters most: SURVIVED and "a test in
    another file caught it" are different findings, and only the full set can
    tell them apart.
    """
    done = subprocess.run(
        [npx(), "vitest", "run", *(files or TEST_FILES)],
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
    lines = [m.strip() for m in FAILED_TEST.findall(out) if m.strip()]
    names = set(lines)
    summary = SUMMARY.search(out)
    counted = None
    if summary:
        failed = SUMMARY_FAILED.search(summary.group(1))
        counted = int(failed.group(1)) if failed else 0
    return names, counted, len(lines), out


def all_test_names() -> dict[str, list[str]]:
    """Every test the suite defines, mapped to the file(s) it is in.

    Files, plural, and that is not defensive. `says nothing when nothing was
    cut` is defined in both `comments.test.ts` and `links.test.ts`, so a
    `dict[str, str]` kept whichever the listing printed last and the narrow run
    below aimed at the wrong file --- caught on 2026-08-21 only because the
    fallback ran all 36 files and got the right answer anyway, at the cost of the
    saving this whole mechanism exists for.
    """
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
    # the rest, never a fixed column. The file comes back too: it is printed
    # right there, and deriving it beats a second table that can drift from the
    # one above.
    found: dict[str, list[str]] = {}
    for file, name in TEST_NAME.findall(out):
        name, file = name.strip(), file.strip()
        if not name:
            continue
        where = found.setdefault(name, [])
        if file not in where:
            where.append(file)
    return found


# --- what a document says about itself ------------------------------------
MUTATIONS += [
    Mutation(
        # Report a range that covers nothing as covering nothing. It is a
        # measurement, and there was no range to measure --- the reassuring
        # branch, arrived at by treating absence as a zero.
        "properties: read an absent byte range as zero coverage",
        "src/lib/properties.ts",
        "  if (signature.covered_bytes === 0) {",
        "  if (false) {",
        "refuses to answer at all when there is no byte range",
    ),
    Mutation(
        # Drop the warning flag on partial coverage. The row still says the
        # right words, so a check reading the text passes; what is lost is the
        # only thing that makes it visible in a list of twenty rows.
        # Drop the Appended row from the section. The function still returns the
        # right value and every test of `appendixRow` in isolation still passes;
        # what is lost is the row reaching a reader, which is the shape a whole
        # feature shipped inert in 26.8.4 and why `check_viewer_wiring.py` exists.
        "properties: build the appendix row and never show it",
        "src/lib/properties.ts",
        "  if (appended) rows.push(appended);",
        "  if (false) rows.push(appended);",
        "sits directly under the Covers row it completes",
    ),
    Mutation(
        # Read /DSS from the object list instead of from the catalog. A `/DSS`
        # object among fifteen could be anything a file happens to carry; the
        # catalog GAINING the key is what an LTV append is. The two agree on
        # every real document and disagree on the one input built to separate
        # them.
        "properties: call any appendix holding a DSS object validation data",
        "src/lib/properties.ts",
        '  if (appendix.catalog_gained.includes("DSS")) {',
        '  if (appendix.kinds.includes("DSS")) {',
        "reads DSS from the catalog rather than from the object list",
    ),
    Mutation(
        # Report an unreadable appendix as an ordinary one. It has no objects in
        # it, so it renders as an append that changed nothing --- absence and
        # failure collapsing into the reassuring one, which this panel refuses
        # everywhere else.
        "properties: read an unreadable appendix as an empty one",
        "src/lib/properties.ts",
        "  if (appendix.unread) {",
        "  if (false) {",
        "reports an appendix it could not read as unread, never as empty",
    ),
    Mutation(
        "properties: state partial coverage without marking it",
        "src/lib/properties.ts",
        "    value: `not the whole file — ${formatBytes(short)} lie outside the signed range`,\n    warn: true,",
        "    value: `not the whole file — ${formatBytes(short)} lie outside the signed range`,\n    warn: false,",
        "names how much lies outside a range that leaves the head of the file",
    ),
    Mutation(
        # The reported defect, restored: lead with everything the signature does
        # not cover instead of what was appended after it. Those differ by the
        # `/Contents` container, which is in every signed PDF there is and was
        # 65,536 of 74,637 bytes on the contract this came from. The row is then
        # arithmetically right and reads as an accusation about 73 KB of
        # unprotected content where 9 KB of validation data was added.
        "properties: lead with everything outside the range rather than the append",
        "src/lib/properties.ts",
        "        `everything up to the signature, and ${formatBytes(signature.appended_bytes)} ` +",
        "        `everything up to the signature, and ${formatBytes(bytes - signature.covered_bytes)} ` +",
        "does not let the container reach the number a reader is shown",
    ),
    Mutation(
        # Go back to claiming the whole file. Every signature excludes its own
        # container, so this is false of all of them -- and it is what made the
        # appended row above read as though the container were a second problem
        # rather than the same one, named once.
        "properties: claim a signature covers the whole file",
        "src/lib/properties.ts",
        '      value: "the whole file, except the signature container it cannot cover",',
        '      value: "the whole file",',
        "names the container it cannot cover, rather than claiming the whole file",
    ),
    Mutation(
        # Read the append before deciding whether anything was measured. With no
        # `/ByteRange` there is no end to subtract from, so `appended_bytes` is
        # zero for *not measured* exactly as `covered_bytes` is -- and reordering
        # makes the refusal unreachable for any signature that has one.
        "properties: report an append before checking a range was stated",
        "src/lib/properties.ts",
        "  if (signature.covered_bytes === 0) {",
        "  if (signature.covered_bytes === 0 && signature.appended_bytes === 0) {",
        "refuses to answer at all when there is no byte range",
    ),
    Mutation(
        # Put the disclaimer on every signature section including the unsigned
        # placeholder, which claims nothing and so has nothing to disclaim.
        "properties: disclaim a signature field nobody has signed",
        "src/lib/properties.ts",
        "    return signature.signed ? { title, rows, note: NOT_CHECKED } : { title, rows };",
        "    return { title, rows, note: NOT_CHECKED };",
        "does not disclaim an unsigned field, which claims nothing",
    ),
    Mutation(
        # Drop the disclaimer entirely. The one mutation here whose survival
        # would mean the honesty rule is held by a comment.
        "properties: show a signature with no disclaimer at all",
        "src/lib/properties.ts",
        "    return signature.signed ? { title, rows, note: NOT_CHECKED } : { title, rows };",
        "    return { title, rows };",
        "carries the disclaimer on every signed signature",
    ),
    Mutation(
        # Put the file's statistics above the signature. Both sections are
        # present either way, so only an order check sees it --- and the reader
        # who opened this dialog on a signed document opened it about that.
        "properties: list the file's statistics above the signature",
        "src/lib/properties.ts",
        "  return [...locked, ...signatures, ...described, ...security, file, ...cut];",
        "  return [...locked, file, ...signatures, ...described, ...security, ...cut];",
        "puts a signature above the file's own statistics",
    ),
    Mutation(
        # Report a locked document's tagging as a fact. `null` means the question
        # could not be asked, and collapsing it to `false` is a confident false
        # statement rather than a missing one.
        "properties: state tagging even when it could not be asked",
        "src/lib/properties.ts",
        "  if (properties.tagged !== null) {",
        "  if (true) {",
        "omits the tagged line when the question could not be asked",
    ),
    Mutation(
        # Round every size, including the ones below a kilobyte. "0.9 KB (923
        # bytes)" is two numbers for a quantity nobody needs rounded.
        "properties: put a rounded size beside a count of a few hundred bytes",
        "src/lib/properties.ts",
        "  if (bytes < 1024) return exact;",
        "  if (bytes < 0) return exact;",
        "gives a plain count below a kilobyte, with no rounded size beside it",
    ),
    Mutation(
        # Let a size that is not a number through. `NaN bytes` reads as a
        # measurement of the file rather than as the absence of one.
        "properties: format a size that is not a number as though it were",
        "src/lib/properties.ts",
        "  if (!Number.isFinite(bytes) || bytes < 0) return \"unknown\";",
        "  if (bytes < -1) return \"unknown\";",
        "says so rather than printing NaN for a size it was not given",
    ),
    Mutation(
        # Describe an ordinary approval signature as a certification. Level 0 is
        # not one of the three the specification defines, and naming it makes a
        # claim about what the signer intended.
        "properties: describe every signature as a certification",
        "src/lib/properties.ts",
        "    default:\n      return \"\";",
        "    default:\n      return \"certified, no changes permitted\";",
        "says nothing for a level the specification does not define",
    ),
    Mutation(
        # Show a field the document does not state, as an empty row. "Reason
        # given:" with nothing after it reads as a blank reason rather than as
        # an absent one.
        "properties: show an empty row for a claim the document never made",
        "src/lib/properties.ts",
        "    if (value) rows.push({ name, value });",
        "    rows.push({ name, value });",
        "leaves out a field the document does not state",
    ),
    Mutation(
        # Put what the signer typed above what the certificate says. A reader
        # asks who signed this and meets the first answer; making that the one
        # anybody can type into a dictionary is the wrong default, and nothing
        # about either line looks different afterwards.
        "certificate: show the typed name above the certificate's",
        "src/lib/properties.ts",
        """  rows.push(...certificateRows(signature));

  claimed("Signer typed", signature.name);""",
        """  claimed("Signer typed", signature.name);

  rows.push(...certificateRows(signature));""",
        "names the signer above what the signer typed",
    ),
    Mutation(
        # Say nothing about a certificate that vouches for itself. Every root in
        # every trust store is self-issued and so is every certificate a signer
        # made for themselves five minutes ago; the row is what lets a reader
        # tell those apart from one an authority issued.
        "certificate: stop saying when a certificate vouches for itself",
        "src/lib/properties.ts",
        """      value: "itself — self-issued, so no other party vouched for this name",""",
        """      value: certificate.issuer_cn || certificate.issuer,""",
        "says a self-issued certificate was vouched for by nobody",
    ),
    Mutation(
        # Warn on a self-issued certificate. The opposite error and the more
        # tempting one: it looks like caution, and it is a verdict tpdf has no
        # trust store with which to reach.
        "certificate: treat self-issued as something to warn about",
        "src/lib/properties.ts",
        """      name: "Issued by",
      value: "itself — self-issued, so no other party vouched for this name",
    });""",
        """      name: "Issued by",
      value: "itself — self-issued, so no other party vouched for this name",
      warn: true,
    });""",
        "says a self-issued certificate was vouched for by nobody",
    ),
    Mutation(
        # Stop reporting two names for one signer that disagree. This is the one
        # line a reader could not work out by eye from the rows above it.
        "certificate: say nothing when the two names for a signer differ",
        "src/lib/properties.ts",
        """  if (typed && inCert && typed.toLowerCase() !== inCert.toLowerCase()) {""",
        """  if (false) {""",
        "points out two names for one signer that disagree",
    ),
    Mutation(
        # Compare the two names case-sensitively, so `a. signer` and `A. Signer`
        # are reported as a disagreement. A false alarm on a document with
        # nothing wrong with it, and the shape that trains a reader to ignore
        # the row that matters.
        "certificate: call a difference of case a disagreement",
        "src/lib/properties.ts",
        "  if (typed && inCert && typed.toLowerCase() !== inCert.toLowerCase()) {",
        "  if (typed && inCert && typed !== inCert) {",
        "does not call a difference of case a disagreement",
    ),
    Mutation(
        # Show a certificate the signature does not point at without saying so.
        # With one certificate it is the only thing it could be; saying nothing
        # makes that indistinguishable from a match.
        "certificate: show an unmatched certificate as though it matched",
        "src/lib/properties.ts",
        "  if (!certificate.matched_signer) {",
        "  if (false) {",
        "warns when the signature does not point at the certificate shown",
    ),
    Mutation(
        # Report a certificate that could not be read as one that is not there.
        # The frontend half of the same confusion `docinfo.rs` guards against:
        # tpdf's failure rendered as the document's silence.
        "certificate: drop the notice for a certificate that could not be read",
        "src/lib/properties.ts",
        """  say(
    "Certificates",
    limits.certificates_unread,
    "were present but could not be read",
  );""",
        "",
        "is reported as tpdf's failure and not as the document's silence",
    ),
    Mutation(
        # Emit rows for a signature carrying no certificate. Every row would be
        # empty, and an empty "Certificate names" reads as a document that
        # states nothing rather than as one that carries nothing.
        "certificate: emit rows when there is no certificate at all",
        "src/lib/properties.ts",
        "  if (!certificate) return [];",
        "  if (!certificate) return [{ name: \"Certificate names\", value: \"\" }];",
        "says nothing at all when there is no certificate",
    ),
    Mutation(
        # Say the same thing about a certificate that states no key usage and
        # one that states an empty usage. Opposite claims, one row.
        "certificate: collapse an unstated key usage onto an empty one",
        "src/lib/properties.ts",
        """      usage === null
        ? "not stated — the certificate places no limit on what the key is used for"
        : usage.length > 0
          ? usage.join(", ")
          : "nothing — the certificate names no use for its own key",""",
        """      usage === null || usage.length === 0
        ? "nothing --- the certificate names no use for its own key"
        : usage.join(", "),""",
        "tells a certificate that states no use apart from one that states none",
    ),
    Mutation(
        # Drop the key usage row entirely when nothing is stated. A reader who
        # sees no row cannot tell "the issuer placed no limit" from a row that
        # was never written.
        "certificate: omit the key usage row when nothing is stated",
        "src/lib/properties.ts",
        "  const usage = certificate.key_usage;\n  rows.push({",
        "  const usage = certificate.key_usage;\n  if (usage !== null) rows.push({",
        "tells a certificate that states no use apart from one that states none",
    ),
    Mutation(
        # Warn on every certificate that is not an authority, which is nearly
        # all of them -- a warning on the ordinary case teaches the reader to
        # ignore the one that matters.
        "certificate: report every certificate's authority, not only a claimed one",
        "src/lib/properties.ts",
        "  if (certificate.authority === true) {",
        "  if (certificate.authority !== true) {",
        "says so when the signer's own certificate claims to issue others",
    ),
    Mutation(
        # Say nothing about extensions that could not be read, so an unknown
        # constraint reads as an absent one.
        "certificate: drop the notice for an extension that could not be read",
        "src/lib/properties.ts",
        "  if (certificate.extensions_unread > 0) {",
        "  if (false) {",
        "reports an extension it could not read rather than one that said nothing",
    ),
    Mutation(
        # Show a conformance claim as a bare standard name. It then reads as
        # tpdf agreeing that the document is PDF/A, which it has not checked and
        # cannot.
        "conformance: state a claim without saying it is unchecked",
        "src/lib/properties.ts",
        '      value: `${xmp.conformance.join(", ")} — the document\'s own claim, which tpdf does not check`,',
        '      value: xmp.conformance.join(", "),',
        "shows a claim as a claim",
    ),
    Mutation(
        # Show only the first standard a document claims, so a file stating both
        # PDF/A and PDF/UA is reported as stating one.
        "conformance: show only the first standard a document claims",
        "src/lib/properties.ts",
        '${xmp.conformance.join(", ")}',
        '${xmp.conformance[0]}',
        "lists every standard a document claims",
    ),
    Mutation(
        # Say something on every document, which is noise on the great majority
        # that claim nothing at all.
        "conformance: emit a row for a document that claims nothing",
        "src/lib/properties.ts",
        "  if (xmp.conformance.length > 0) {",
        "  if (xmp.conformance.length >= 0) {",
        "says nothing for a document that claims nothing",
    ),
    Mutation(
        # Read an unreadable packet as a document that says nothing about
        # itself, which is tpdf's failure reported as the document's silence.
        "conformance: say nothing about a packet that could not be read",
        "src/lib/properties.ts",
        "  if (xmp.unread) {",
        "  if (false) {",
        "speaks up when a packet is there and could not be read",
    ),
    Mutation(
        # Build the rows and drop them, so the readout never carries a claim.
        "conformance: leave the claim out of the readout",
        "src/lib/properties.ts",
        "  rows.push(...conformanceRows(properties.xmp));",
        "",
        "puts a claim in the file section of the readout",
    ),
    Mutation(
        # Show the attested time with no authority named and no disclaimer. It
        # then reads as tpdf vouching for the moment.
        "timestamp: state an attested time as a bare fact",
        "src/lib/properties.ts",
        '      value: `${stamp.when} by ${by} — a separate party\'s claim, which tpdf does not check`,',
        "      value: stamp.when,",
        "names the authority beside the time, and says it is unchecked",
    ),
    Mutation(
        # Drop the row when the token names no authority. A time worth less is
        # not a time worth nothing, and the signature then reads as one nobody
        # timestamped.
        "timestamp: drop a timestamp whose authority is unnamed",
        "src/lib/properties.ts",
        "  if (stamp?.when) {",
        "  if (stamp?.when && stamp.authority) {",
        "still reports a token that names no authority",
    ),
    Mutation(
        # Drop the signer's own date, leaving only the attested one. The
        # readout then shows one time where the document offers two from
        # different sources -- and it is the source the document itself states
        # that goes missing.
        #
        # Aimed at the ordering test because that test is where the two rows
        # are asserted to coexist. Its first version compared `indexOf` alone
        # and this SURVIVED it: -1 is less than every real index, so deleting
        # the row satisfied the ordering rather than breaking it.
        "timestamp: drop the signer's own date, leaving only the attested one",
        "src/lib/properties.ts",
        '  claimed("Date given", signature.when);',
        "",
        "puts the attested time under the signer's own date, not over it",
    ),
]

#: Marks: which page one names, where a comment is placed, and moving one.
#:
#: The first three aim at the defect a reader reported as shapes vanishing: the
#: viewer handed `onDrawn` a page **id**, `Edits.mark` took a **slot** and indexed
#: `pages` by it, so a box drawn on the first page of an unedited document was
#: written to the second and one drawn on the last page was dropped in silence.
#: `PageId` is what makes that combination a type error now; these are the
#: behavioural half, since a brand is erased at runtime and cannot be mutated.
MUTATIONS += [
    Mutation(
        # Answer `commentAt` with the slot, which is what it did while
        # `Edits.mark` translated. Correct for that caller and for no other.
        "pageid: place a comment by slot rather than by id",
        "src/lib/viewer.ts",
        "    return { page: id, quads: this.fileRectOn(slot, quad) };",
        "    return { page: slot as unknown as PageId, quads: this.fileRectOn(slot, quad) };",
        "places a comment on the page by id too",
    ),
    Mutation(
        # Report the drawn page by slot. The one-page fixtures every other test
        # in that file uses cannot see this; the re-ordered one can.
        "pageid: report the drawn page by slot rather than by id",
        "src/lib/viewer.ts",
        "        this.opts.onDrawn?.(\n          kind,\n          id,",
        "        this.opts.onDrawn?.(\n          kind,\n          live.slot as unknown as PageId,",
        "reports the drawn page by id, not by the slot it was drawn in",
    ),
    Mutation(
        # Make a comment need a rectangle again, so a click commits nothing and
        # the tool stays armed --- the state the reader reported as the command
        # dropping the bubble in the corner.
        "comment: require a drag rather than a press to place one",
        "src/lib/viewer.ts",
        "        const quad =\n"
        '          kind === "note"\n'
        "            ? iconQuad(live.from.x, live.from.y, this.laidSize(live.slot))\n"
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));",
        "        const quad = boxQuad(live.from, live.to, this.laidSize(live.slot));",
        "drops the bubble where the reader pressed, from a click alone",
    ),
    Mutation(
        # Take Enter for any armed tool, which drops a zero-sized box on a
        # keystroke the reader meant for something else.
        "comment: place a mark on Enter whatever tool is armed",
        "src/lib/viewer.ts",
        '    } else if (event.key === "Enter" && this.drawKind === "note") {',
        '    } else if (event.key === "Enter" && this.drawKind !== null) {',
        "takes Enter only for the comment tool",
    ),
    Mutation(
        # Leave the comment tool armed after it places one, so the reader's next
        # press --- on a link, on a word --- becomes another bubble.
        "comment: keep the tool armed after a bubble is placed",
        "src/lib/viewer.ts",
        "        // armed tool and must be spent together.\n"
        "        const stamp = this.drawStamp;\n"
        "        this.drawKind = null;",
        "        // Spent, and cleared *before* the callback so that an `onDrawn` which\n"
        "        // arms it again is not undone by this line.\n"
        '        if (kind !== "note") this.drawKind = null;',
        "spends the tool on that press, and takes no second comment",
    ),
    Mutation(
        # Drop the three mode fields from the summary, which is what decides
        # whether `onStatus` fires at all. The status line then hears about an
        # armed tool only when something unrelated moves.
        "status: leave the mode fields out of the summary that gates a report",
        "src/lib/viewer.ts",
        "      status.drawing,\n      // Both halves, because either can move on its own: a sweep across a\n"
        "      // drawing and a highlight changes one number per mark it crosses, and a\n"
        "      // field left out of this string is a field the window is told about only\n"
        "      // when something else happens to move.\n"
        "      status.erasing?.strokes ?? null,\n      status.erasing?.marks ?? null,\n      status.armed,\n",
        "",
        "reports a status when a tool is armed, and again when it is dropped",
    ),
    Mutation(
        # Report ink as armed as well, so the window carries two lines saying
        # the same thing in different words.
        "status: name a drawing in the armed field as well as its own",
        "src/lib/viewer.ts",
        # Re-aimed 2026-08-23 when the crop joined this expression. It stays a
        # mutation about ink -- the crop's ternary is kept so that the only thing
        # removed is `drawnStrokes`, which is what decides ink's own line.
        '      armed: this.cropping ? "crop" : this.drawnStrokes === null ? this.drawKind : null,',
        '      armed: this.cropping ? "crop" : this.drawKind,',
        "names a drawing in one field, not two",
    ),
    Mutation(
        # Offer the drag on every kind, including the ones made of the words
        # under them --- a highlight dragged off its line marks nothing.
        "move: let a mark made of words be dragged like a placed one",
        "src/lib/markband.ts",
        '    case "highlight":\n'
        '    case "underline":\n'
        '    case "strikeout":\n'
        '    case "squiggly":\n'
        "      return false;",
        '    case "highlight":\n'
        '    case "underline":\n'
        '    case "strikeout":\n'
        '    case "squiggly":\n'
        "      return true;",
        "does not move a mark that is made of the words under it",
    ),
    Mutation(
        # Offer it on the comment alone, which is the kind that was reported ---
        # and is what a predicate written from the report rather than from the
        # rule would say.
        "move: offer the drag on the reported kind and no other",
        "src/lib/markband.ts",
        '    case "note":\n'
        '    case "square":\n'
        '    case "ellipse":\n'
        '    case "textbox":\n'
        '    case "ink":\n'
        "    // A stamp is placed by the reader and anchored to nothing, so it moves for\n"
        "    // the box's reason exactly.\n"
        '    case "stamp":\n'
        "      return true;",
        '    case "note":\n'
        "      return true;\n"
        '    case "square":\n'
        '    case "ellipse":\n'
        '    case "textbox":\n'
        '    case "ink":\n'
        '    case "stamp":\n'
        "      return false;",
        "moves a box, an ellipse, a text box and a drawing",
    ),
    Mutation(
        # Send the offset measured on screen. Identical on an upright page and
        # sideways on a turned one, which is the whole reason the fixture turns.
        "move: send the laid-out offset rather than the file's",
        "src/lib/viewer.ts",
        "        const sent = this.fileDelta(live.slot, live.dx, live.dy);\n"
        "        this.opts.onMarkMoved?.(live.id, sent.dx, sent.dy);",
        "        this.opts.onMarkMoved?.(live.id, live.dx, live.dy);",
        "reports the offset in the file's space, not the reader's",
    ),
    Mutation(
        # Drop the clamp, so a mark can be dragged off the paper and written
        # with a `/Rect` other readers clip or place themselves.
        "move: let a mark be dragged off its page",
        "src/lib/viewer.ts",
        "        const bound = this.clampMove(live.id, live.slot, want);",
        "        const bound = want;",
        "cuts the offset short at the page's edge",
    ),
    Mutation(
        # Journal a command for a press that did not move, which is how a reader
        # opens a note --- so undo would step back through every note ever opened.
        "move: report a press that never moved as a move",
        "src/lib/viewer.ts",
        "        if (live.dx === 0 && live.dy === 0) return;",
        "",
        "reports nothing for a press that did not move",
    ),
    Mutation(
        # Report on every pointer event, so one drag becomes a dozen commands
        # and undo walks the mark home a step at a time.
        "move: report once per pointer event rather than once per drag",
        "src/lib/viewer.ts",
        "        live.dx = bound.dx;\n        live.dy = bound.dy;\n        this.wake();",
        "        live.dx = bound.dx;\n        live.dy = bound.dy;\n"
        "        this.opts.onMarkMoved?.(live.id, bound.dx, bound.dy);\n        this.wake();",
        "reports once for a drag, not once per pointer event",
    ),
    Mutation(
        # Let Escape fall through to the drawing tools while a mark is being
        # dragged, so the mark commits where the hand happened to be.
        "move: take Escape away from a drag in progress",
        "src/lib/viewer.ts",
        "      if (this.moving) {",
        "      if (false) {",
        "throws the move away on Escape",
    ),
]

MUTATIONS += [
    # Crop by dragging, 2026-08-23. Eight mutations, one per test, each proved to
    # redden the test named for it and no other -- except the first, which also
    # reddens the ordering test, because a drag the tool never refused commits
    # rectangles the ordering test then counts.
    Mutation(
        # Take any press. The tool is what decides, and without that a drag
        # anywhere on a page crops it -- which is the worst failure this gesture
        # has, because a crop removes something the reader can see.
        "crop: start the crop drag whether or not the tool is armed",
        "src/lib/viewer.ts",
        "        if (!this.cropping) return false;",
        "        if (false) return false;",
        "reports nothing until the tool is armed",
    ),
    Mutation(
        # Report the laid-out rectangle. On an uncropped, unturned page it is
        # the right answer; on a page already cropped it walks the crop further
        # in every time, which reads as a viewer that mis-draws rather than as a
        # translation that never happened.
        "crop: report the rectangle in the page's laid-out space",
        "src/lib/viewer.ts",
        "        this.opts.onCropped?.(id, this.fileRectOn(live.slot, quad));",
        "        this.opts.onCropped?.(id, [quad.left, quad.top, quad.right, quad.bottom]);",
        "reports the rectangle in the file's space and not the page's",
    ),
    Mutation(
        # Leave the tool armed. Every drawing tool is one-shot and a crop has a
        # stronger reason to be: a second crop replaces the first, so a reader
        # who did not notice it was still armed loses the crop they just made.
        "crop: leave the crop tool armed after a rectangle",
        "src/lib/viewer.ts",
        "        this.cropping = false;\n        this.showCursor();\n        this.opts.onCropped?.(",
        "        this.showCursor();\n        this.opts.onCropped?.(",
        "is spent by one rectangle",
    ),
    Mutation(
        # Escape's ladder without the crop's two fields. It falls through to
        # clearing the selection, so a reader who presses Escape mid-drag
        # watches the rectangle stay and then commits it by letting go.
        "crop: leave the crop out of what Escape can reach",
        "src/lib/viewer.ts",
        "        this.doomed ||\n        this.cropping ||\n        this.cropDrawing",
        "        this.doomed",
        "is dropped by Escape mid-drag, without cropping",
    ),
    Mutation(
        # Build the rectangle without `boxQuad`, so `MIN_BOX` never refuses one.
        # A click then crops the page to nothing, and the tool is spent doing it.
        "crop: commit a click as a rectangle of no size",
        "src/lib/viewer.ts",
        "        const quad = boxQuad(live.from, live.to, this.laidSize(live.slot));\n"
        "        const id = quad ? this.pages.idOf(live.slot) : undefined;",
        "        const quad = {\n"
        "          left: Math.min(live.from.x, live.to.x),\n"
        "          top: Math.min(live.from.y, live.to.y),\n"
        "          right: Math.max(live.from.x, live.to.x),\n"
        "          bottom: Math.max(live.from.y, live.to.y),\n"
        "        };\n"
        "        const id = this.pages.idOf(live.slot);",
        "keeps the tool armed when the reader clicks instead of dragging",
    ),
    Mutation(
        # The corners in arrival order. Three of the four directions a reader can
        # drag then produce an inside-out crop box, which the model refuses --
        # so the tool works downhill and not uphill.
        "crop: keep the corners in the order the drag reported them",
        "src/lib/viewer.ts",
        "        const quad = boxQuad(live.from, live.to, this.laidSize(live.slot));\n"
        "        const id = quad ? this.pages.idOf(live.slot) : undefined;\n"
        "        if (!quad || id === undefined) {",
        "        const quad = {\n"
        "          left: live.from.x,\n"
        "          top: live.from.y,\n"
        "          right: live.to.x,\n"
        "          bottom: live.to.y,\n"
        "        };\n"
        "        const id = this.pages.idOf(live.slot);\n"
        "        if (!quad || id === undefined) {",
        "orders the corners whichever way the drag went",
    ),
    Mutation(
        # Arm a drawing tool without putting the crop away. Both are then set,
        # and `onSelectStart` asks the crop drag first -- so choosing the box
        # tool crops the page instead.
        "crop: let armDraw leave the crop tool armed",
        "src/lib/viewer.ts",
        "    this.erasing = false;\n    this.cropping = false;\n    this.drawKind = kind;",
        "    this.erasing = false;\n    this.drawKind = kind;",
        "puts the drawing tool away, and the drawing tool puts it away",
    ),
    Mutation(
        # Fill `armed` from the drawing tool alone, which is what it did before
        # the crop existed. The reader arms the crop from the palette, gets a
        # crosshair and no words, and has nothing telling them what their next
        # press will do -- the exact complaint the field was added for. The
        # accessors all still answer correctly, so only a test reading the
        # status can see it.
        "crop: leave the armed crop out of the status the window reads",
        "src/lib/viewer.ts",
        '      armed: this.cropping ? "crop" : this.drawnStrokes === null ? this.drawKind : null,',
        "      armed: this.drawnStrokes === null ? this.drawKind : null,",
        "names the armed crop, which is not a mark kind",
    ),
    Mutation(
        # The mirror: arm the crop without putting the drawing tool away. The
        # crop drag wins the press, so the drawing tool is armed, invisible and
        # unreachable until the reader presses Escape."""
        "crop: let armCrop leave the drawing tool armed",
        "src/lib/viewer.ts",
        "    this.drawKind = null;\n    this.drawStamp = null;\n    this.inking = null;\n"
        "    this.erasing = false;\n    this.cropping = true;",
        "    this.inking = null;\n    this.erasing = false;\n    this.cropping = true;",
        "puts the drawing tool away, and the drawing tool puts it away",
    ),
]



def main() -> int:
    # Before anything prints. A redirected run is block-buffered otherwise, and
    # this harness takes the better part of an hour: on 2026-08-19 a full run's
    # output sat at three lines for forty minutes and its verdict was lost
    # entirely when the run was interrupted, which is the exact ambiguity
    # `live_output` exists to remove. The three window harnesses had the fix and
    # the three mutation harnesses did not.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    # Same flag, same meaning as `mutate_viewer.py`'s. Added when the page
    # deletion work put twenty new mutations into a table of a hundred: the
    # whole table is what runs before a push, and re-proving ninety-odd
    # mutations that could not have moved is an hour of somebody waiting.
    # The control run and the name cross-check below still run in full.
    parser.add_argument(
        "--only", default="", help="run mutations whose name contains this"
    )
    # `--only` matches a mutation's name, so a change across four modules is
    # four runs and four control passes. This selects by the file a mutation
    # edits, which is what an edit actually moves. See `mutation_since.py` for
    # why it is loud and why nothing selected is a refusal.
    parser.add_argument(
        "--since", default="", help="run only mutations whose file changed since this ref"
    )
    args = parser.parse_args()
    chosen = [m for m in MUTATIONS if args.only.lower() in m.name.lower()]
    if args.since:
        # No prefix: this table names paths from the repository root already.
        chosen, code = mutation_since.apply(chosen, args.since)
        if code:
            return code

    if args.list:
        for mutation in chosen:
            print(f"{mutation.name}  ->  expects: {mutation.expect}")
        return 0
    if not chosen:
        print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    print("--- control: the suite must be green before anything is broken", flush=True)
    names, counted, red_lines, out = run_tests()
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
    unknown = [m for m in chosen if not any(m.expect in name for name in known)]
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
        for mutation in chosen:
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
                # removing it alone took three failures to twelve, because every
                # anchor here is written with "\n" and the checkout was CRLF. So
                # normalise for matching and put the file's own convention back,
                # leaving the mutation as the only difference on disk.
                #
                # **The checkout is no longer CRLF**: `.gitattributes` has pinned
                # `* text=auto eol=lf` since 2026-08-26. Kept regardless, because
                # a checkout is not the only thing that writes a file -- see the
                # longer note in `mutate_rust.py`.
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
                # The file holding the test this mutation names, and only it.
                # Nearly every mutation is caught there, and running the other
                # nineteen files to find that out cost 5.8 s a time -- 31
                # minutes over this table, measured 2026-08-21 before this.
                aimed = sorted(
                    {
                        file
                        for name, files in known.items()
                        if mutation.expect in name
                        for file in files
                    }
                )
                names, counted, red_lines, out = run_tests(aimed or None)
                narrow = bool(aimed)
                if counted is not None and not names and narrow:
                    # Nothing in that file noticed. Whether anything else did is
                    # exactly the question, so now every file runs.
                    names, counted, red_lines, out = run_tests()
                    narrow = False
            finally:
                target.write_bytes(backup.read_bytes())

            if counted is None:
                print(f"[FAIL] {mutation.name}: no summary line -- the run did not finish")
                problems += 1
                continue
            # The cross-check: the reporter's per-test lines and its own count
            # must agree, or one of the two has stopped describing the run. It
            # counts LINES rather than distinct names -- see `run_tests`.
            if red_lines != counted:
                print(
                    f"[FAIL] {mutation.name}: {red_lines} failing test lines but the summary "
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
            # Which files were run, because `1 red` out of one file and `1 red`
            # out of twenty are not the same statement.
            scope = (
                ", ".join(Path(f).name for f in aimed)
                if narrow
                else f"{len(TEST_FILES)} files"
            )
            print(
                f"{mark} {mutation.name}: {counted} red in {scope}"
                + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
            )
            if not hit:
                print(f"         red instead: {sorted(names)}")
                problems += 1

    print()
    print(
        f"[OK] all {len(chosen)} mutations caught by the test named for them"
        if problems == 0
        else f"[FAIL] {problems} of {len(chosen)} mutations were not caught as described"
    )
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
