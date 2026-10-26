"""One part of the frontend mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- a colour a reader can choose ----------------------------------------
ROWS += [
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
        "    const mark = this.markById(id);\n"
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

ROWS += [
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
        "    const label = removeLabel(mark.kind);",
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
        # The defect 26.8.11 published: the release notes calling a capability
        # unbuilt one release after it shipped. This is the third time in four
        # releases that block has described an older product, and the first time
        # anything can go red about it.
        "release-notes: call a shipped command unbuilt",
        ".github/workflows/release.yml",
        "          <!-- not-built: file.certifyDocument -->",
        "          <!-- not-built: file.certifyDocument edit.insertPages -->",
        "calls nothing unbuilt that the application registers",
    ),
    Mutation(
        # The half that makes this worth having rather than a second list to
        # keep: two documents can each be free of registered ids and still
        # disagree, and a reader who meets both then gets two answers.
        #
        # A **mistyped** id, which is what that drift actually looks like -- one
        # document saying `edit.editTextBox` where the other says
        # `edit.editText`. The first attempt at this mutation used
        # `edit.editForeignMark` and SURVIVED, correctly: the README claims that
        # one absent too, so the two documents still agreed. A registered id
        # would redden the check above instead, and no *real* unregistered id can
        # break the agreement -- the README's own gate makes every unbuilt
        # command appear in its list. So the only shape left is one that names
        # nothing, which is also the only shape this can fail as.
        "release-notes: claim something the README does not",
        ".github/workflows/release.yml",
        "          <!-- not-built: file.certifyDocument -->",
        "          <!-- not-built: file.certifyDocument edit.editTextBox -->",
        "agrees with the README about what is not built",
    ),
    Mutation(
        # The extraction stops matching, which reads exactly like a release body
        # that claims nothing wrong. A regex over a YAML file is the fragile
        # part of this check and the refusals are what own it.
        "release-notes: rename the heredoc the notes are built in",
        ".github/workflows/release.yml",
        "cat > release-notes.md <<'NOTES'",
        "cat > notes.md <<'NOTES'",
        "finds the release notes inside the workflow",
    ),
    Mutation(
        # The error the whole check exists for, and the one its Python
        # predecessor could not see: `edit.stamp.approved` is built in a loop, so
        # a regex over `appcommands.ts` finds no such literal and reports the
        # claim clean. Measured green there on 2026-08-24 before this replaced
        # it.
        "readme: say a shipped command is not built",
        "README.md",
        "  <!-- not-built: file.certifyDocument -->",
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
        "- Certification signatures: a signature that states which changes a document permits",
        "- Certification signatures: <!-- built: file.print --> a signature that states which changes a document permits",
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
        "const title = signature.field ? `${what} — ${signature.field}` : what;",
        "const title = signature.field ? `${what} --- ${signature.field}` : what;",
        "holds no prose dash outside the separator sentinel",
    ),
    Mutation(
        "update: spell the ready-to-restart notice with the prose dash",
        "src/lib/update.ts",
        "    installed: (newer: string) => `Version ${newer} is installed — restart tpdf to finish`,",
        "    installed: (newer: string) => `Version ${newer} is installed --- restart tpdf to finish`,",
        "holds no prose dash outside the separator sentinel",
    ),
    Mutation(
        "outline: spell a refused action with the prose dash",
        "src/lib/outline.ts",
        '    uri: "opens a web link — not followed",',
        '    uri: "opens a web link --- not followed",',
        "holds no prose dash outside the separator sentinel",
    ),
    Mutation(
        # Open every web link through the document the reader has open. Right
        # for the opened file's own links and wrong for one on a page inserted
        # from another file, whose token indexes that file's list -- so it
        # opens whichever address happens to sit at the same index.
        "weblinkdialog: open every link through the opened document's list",
        "src/lib/weblinkdialog.ts",
        "    await deps.open(target.doc ?? opened, source, target.token);",
        "    await deps.open(opened, source, target.token);",
        "opens a link from an inserted page through that file's own handle",
    ),
    Mutation(
        # The other half of the same expression: take the target's handle even
        # when it names none, which is every link of the opened document and
        # every entry of its outline. `undefined` reaches the command as the
        # document to open through, and nothing opens.
        "weblinkdialog: open every link through the target's own handle",
        "src/lib/weblinkdialog.ts",
        "    await deps.open(target.doc ?? opened, source, target.token);",
        "    await deps.open(target.doc as number, source, target.token);",
        "asks before it opens, and opens what was asked about",
    ),
    # ------------------------------------------------------ split a document
    Mutation(
        # The boundary. `3` means "the first file ends at page 3", so the cut
        # is exclusive of the slot it names; making it inclusive moves one page
        # from every part to the one before it. Every part still opens, the
        # counts still sum to the document, and only which pages landed where
        # says so -- which is why the test reads the fixture's rotations rather
        # than counting.
        "split: cut inclusively, moving one page into the earlier part",
        "src/lib/pageranges.ts",
        "    for (let slot = from; slot < cut; slot += 1) run.push(slot);",
        "    for (let slot = from; slot <= cut; slot += 1) run.push(slot);",
        "covers every page exactly once, whatever the cuts",
    ),
    Mutation(
        # Allow a cut after the last page, which writes a final file of no
        # pages. `lopdf` will serialise an empty page list quite happily.
        "split: allow a cut after the last page",
        "src/lib/pageranges.ts",
        "    if (page === pageCount) {",
        "    if (false) {",
        "refuses the last page, because cutting after it makes an empty file",
    ),
    Mutation(
        # Merge a repeated cut instead of refusing it, which is what
        # `parsePageRange` correctly does for a SET and is wrong for a cut
        # list: the second `3` names a file with no pages in it.
        "split: merge a repeated cut rather than refusing it",
        "src/lib/pageranges.ts",
        "    if (cuts.has(page)) return { problem: TEXT().namedTwice(page) };",
        "    if (false) return { problem: TEXT().namedTwice(page) };",
        "refuses a repeated cut rather than merging it, because it makes an empty file",
    ),
    Mutation(
        # Read an extract range in the split box. `1-3` would reach `readPage`
        # as one token, fail there, and tell the reader "1-3" is not a page
        # number -- sending them to look for a typo that is not there.
        "split: let a range reach the number parser",
        "src/lib/pageranges.ts",
        '    if (piece.includes("-")) {',
        "    if (false) {",
        "refuses a range, naming the box the reader meant",
    ),
    Mutation(
        # The command's second line of defence, which is the one that decides
        # whether files get written: a caller that skipped `problem` reaches
        # `run` directly, and this is what stops it.
        "split: act on an argument that did not parse",
        "src/lib/appcommands.ts",
        "          if (!split.groups) return;",
        "          if (false) return;",
        "refuse to split what does not parse, and reach no action",
    ),
    # ------------------------------------------------------- docs/PLAN.md
    # The plan names command ids in its prose, and a rename leaves those
    # mentions behind. `PLAN.md:8800` records that happening twice already.
    # `src/lib/plan.test.ts` is what reads it.
    Mutation(
        # A command renamed, with the plan left behind. This is the case the
        # check exists for and the only one it can catch on its own.
        "plan: name a command that is not registered",
        "docs/PLAN.md",
        "`edit.drawBox runs from the palette`",
        "`edit.drawBoxen runs from the palette`",
        "names only commands that exist",
    ),
    Mutation(
        # A whole family renamed. No single id is named in the plan for these,
        # so the exact-id check is structurally blind to it and the family
        # check is the only thing standing there.
        "plan: name a command family that is not registered",
        "docs/PLAN.md",
        # Widened 2026-08-30. The anchor was the bare `` `edit.color.*` ``, and a
        # later section naming the same family in passing made it match twice ---
        # the anchor never moved and nothing about the mutation was wrong, which
        # is the shape `docs/TRAPS.md` records. Widened rather than answered by
        # rewording the new prose: a document should be free to name a family the
        # plan already talks about.
        "the shape\n`edit.color.*` already has.",
        "the shape\n`edit.colour.*` already has.",
        "names only command families that exist",
    ),
    Mutation(
        # An exclusion outliving its subject. Left alone it becomes a blanket
        # permission: the id it names stops appearing in the plan, and the
        # entry goes on excusing nothing while looking deliberate.
        "plan: keep an exclusion for a string the plan never mentions",
        "src/lib/plan.test.ts",
        '  "file.pdf": "a placeholder filename',
        '  "edit.neverMentionedAnywhere": "planted",\n  "file.pdf": "a placeholder filename',
        "keeps no exclusion for a string the plan no longer contains",
    ),
    Mutation(
        # The emptiness control. A scan whose pattern stops matching agrees
        # with every claim in the file, which is what a clean run looks like.
        "plan: scan for ids under a namespace nothing uses",
        "src/lib/plan.test.ts",
        "/\\b(?:app|edit|file|find|nav|view)\\.",
        "/\\b(?:zzz)\\.",
        "finds command ids in the plan to check",
    ),
]

#: The redaction review panel --- `docs/PLAN.md` §6 step 2, built 2026-08-26.
#:
#: The two under `text.ts` and `reading.ts` are the ones worth reading: the whole
#: design decision of this increment is that a region marked for removal may NOT
#: be reviewed with the containment rule an annotation uses, and each of them
#: reintroduces exactly that.
ROWS += [
    Mutation(
        # Review a region with the annotation rule. This is the defect the two
        # rules exist to keep apart, and it is one word: a highlight's rectangle
        # is drawn generously around its words, so the centre rule forgives an
        # overhang; a dragged region is a claim about what disappears, so
        # forgiving one understates what an apply takes.
        "reading: review a region with the annotation's containment rule",
        "src/lib/reading.ts",
        "  return inReadingOrder(text, touchedIndices(text, quads));",
        "  return inReadingOrder(text, coveredIndices(text, quads));",
        "takes a character the rectangle cuts through, which the other rule leaves",
    ),
    Mutation(
        # Require the glyph to be wholly inside, horizontally. The direction
        # that under-reports: a reader approves a list of words and the removal
        # takes the half-covered ones either side of it as well.
        "text: touch only a character the rectangle wholly contains",
        "src/lib/text.ts",
        "    box.right >= left && box.left <= right && box.bottom >= top && box.top <= bottom",
        "    box.left >= left && box.right <= right && box.bottom >= top && box.top <= bottom",
        "takes a character the rectangle cuts through, which the other rule leaves",
    ),
    Mutation(
        # The vertical half of the same rule, and the one that matters more: a
        # rectangle dragged four points too deep clips the tops of the next
        # line's glyphs, `redact.rs` removes the whole text object they belong
        # to, and this list is the only place a reader could have seen it.
        "text: ignore a line the rectangle's lower edge only clips",
        "src/lib/text.ts",
        "box.bottom >= top && box.top <= bottom",
        "box.bottom >= top && box.bottom <= bottom",
        "takes the line below when the rectangle's edge clips it",
    ),
    Mutation(
        # Order two regions on one page by nothing. `Array.sort` is stable, so
        # this is not a shuffle --- the list simply comes back in the order the
        # regions were dragged, which is a plausible order and the wrong one.
        "pages: order things on a page with no tiebreak at all",
        "src/lib/pages.ts",
        "    return a.id - b.id;\n  });\n  return items;",
        "    return 0;\n  });\n  return items;",
        "breaks a shared top edge by id rather than by the sort's stability",
    ),
    Mutation(
        # Take a region's page id for its slot. The two are the same number
        # until a page is moved or deleted, which is exactly when a reviewer
        # most needs the list to be in the order they will meet the regions.
        "pages: place a region by its page id rather than by its slot",
        "src/lib/pages.ts",
        "    const slot = pages.slotOfId(redaction.page);\n    if (slot === undefined) {",
        "    const slot = redaction.page as number;\n    if (slot === undefined) {",
        "lists regions down the document, not in the order they were dragged",
    ),
    Mutation(
        # Drop a region nothing could place. A review panel that silently drops
        # a row tells a reader a pending redaction is no longer pending, which
        # is the one thing this subsystem must never say.
        "pages: drop a region whose page is in no slot",
        "src/lib/pages.ts",
        "      lost.push({ redaction, page: null });\n      continue;",
        "      continue;",
        "keeps a region nothing could place, with no page against it",
    ),
    Mutation(
        # Report a page that could not be read as a region with no words in it.
        # A claim about the document, made from a failure to look at it.
        "redactlist: call an unreadable page a region with no text",
        "src/lib/redactlist.ts",
        "  if (words === null) return { text: UNREADABLE, own: false };",
        "  if (words === null) return { text: NO_TEXT, own: false };",
        "says the page could not be read, which is not the same as empty",
    ),
    Mutation(
        # The mirror: report a page still being extracted as one holding no
        # words. The reassuring answer, arrived at before anything was read.
        "redactlist: call an unread page a region with no text",
        "src/lib/redactlist.ts",
        "  if (words === undefined) return { text: READING, own: false };",
        "  if (words === undefined) return { text: NO_TEXT, own: false };",
        "says the page is still being read when nothing has answered yet",
    ),
    Mutation(
        # Drop the standing line. The count alone reads as a list of things that
        # have happened, and §6's thesis is that a redaction which looks done
        # and is not is worse than none.
        "redactlist: count the regions without saying nothing has been removed",
        "src/lib/redactlist.ts",
        "    nothingRemoved: (marked: string) => `${marked}. Nothing has been removed yet.`,",
        "    nothingRemoved: (marked: string) => `${marked}.`,",
        "counts the regions and says nothing has been removed",
    ),
    Mutation(
        # Say nothing about the pictures a region takes. A reader commits without
        # being told that a figure their rectangle happened to cover will be gone
        # from the file, whole, with no copy left.
        "redactlist: take a picture without saying so",
        "src/lib/redactlist.ts",
        "  if (pictures > 0) {",
        "  if (pictures > 9000) {",
        "says a picture goes whole, because that cannot be undone afterwards",
    ),
    Mutation(
        # Say it on every region. A sentence that is always there is one a reader
        # stops seeing, which costs exactly the regions where it matters.
        "redactlist: claim every region takes a picture",
        "src/lib/redactlist.ts",
        "  if (said.length === 0) return cuts > 0",
        "  if (false) return cuts > 0",
        "says nothing when the region takes no picture",
    ),
    Mutation(
        # Read the field without allowing for a reply that has none. A plan from
        # a build before pictures were removable then breaks the panel rather
        # than the sentence.
        "redactlist: assume every plan carries a picture list",
        "src/lib/redactlist.ts",
        "  const pictures = (plan?.images?.length ?? 0) + (plan?.form_images?.length ?? 0);",
        "  const pictures = (plan as { images: number[] }).images.length + (plan?.form_images?.length ?? 0);",
        "survives a plan written before pictures were removable",
    ),
    Mutation(
        # Say nothing about regions that are on no page. They are listed with an
        # em dash for a page and no explanation of what that means.
        "redactlist: leave the unplaceable regions out of the notice",
        "src/lib/redactlist.ts",
        "  if (lost === 0) return said;",
        "  return said;\n  if (lost === 0) return said;",
        "names regions that are on no page, without dropping the standing line",
    ),
    Mutation(
        # A words setter that redraws nothing. Every row then says it is still
        # reading the page for the rest of the session, because the only other
        # thing that repaints is the list changing.
        "redactlist: accept words that have arrived and redraw nothing",
        "src/lib/redactlist.ts",
        "  setWords(): void {\n    this.paint();\n  }",
        "  setWords(): void {}",
        "shows a word that arrives after the row was drawn",
    ),
    Mutation(
        # Answer Enter on a region with nowhere to go. The reader presses and
        # the document does not move, which reads as a broken panel rather than
        # as a row that says it is disabled.
        "redactlist: answer Enter on a region that is on no page",
        "src/lib/redactlist.ts",
        "        if (from !== null && this.placed(from)) this.opts.onPick(from);",
        "        if (from !== null) this.opts.onPick(from);",
        "refuses to scroll to a region that is on no page",
    ),
    Mutation(
        # The pointer half of the same refusal, which is a separate listener and
        # therefore a separate defect.
        "redactlist: let a region on no page be pressed",
        "src/lib/redactlist.ts",
        '      element.setAttribute("aria-disabled", "true");\n      element.style.opacity = "0.55";\n      return element;',
        '      element.setAttribute("aria-disabled", "true");\n      element.style.opacity = "0.55";',
        "refuses to scroll to a region that is on no page",
    ),
    Mutation(
        # Guard removal on the same predicate. That would leave a region on no
        # page listed for ever: undo is chronological, and this control is the
        # only other way off.
        "redactlist: refuse to remove a region that is on no page",
        "src/lib/redactlist.ts",
        "        if (from !== null) this.opts.onRemove(from);",
        "        if (from !== null && this.placed(from)) this.opts.onRemove(from);",
        "removes with Delete and with Backspace, including a region on no page",
    ),
    Mutation(
        # Let a key pressed on the remove control fall through to the row.
        # `idOf` finds no id on a button, so the fallback hands it the focused
        # row and the panel scrolls instead of the region coming off.
        "redactlist: let Enter on the remove control reach the row",
        "src/lib/redactlist.ts",
        '    if (part === "remove") return;\n\n    // The event',
        '    if (part === "removed") return;\n\n    // The event',
        "leaves a key pressed on the region's remove control to the control",
    ),
    Mutation(
        # Name the control for nothing in particular. A screen reader walking
        # the list then hears six identical buttons.
        "redactlist: name the remove control without saying what it removes",
        "src/lib/redactlist.ts",
        '    removeRegion: "Remove region",',
        '    removeRegion: "Remove",',
        "names the control for what it takes off",
    ),
    Mutation(
        # A sidebar forwarder that drops its argument. The panel is built, the
        # tab is there, and every region the reader marks reaches nothing --- the
        # inert-wiring defect this repository has a gate for one layer up.
        "sidebar: accept the regions and hand them to no panel",
        "src/lib/sidebar.ts",
        "  setRedactions(rows: readonly RedactionRow[]): void {\n    this.pending.setRedactions(rows);\n  }",
        "  setRedactions(rows: readonly RedactionRow[]): void {\n    void rows;\n  }",
        "puts the regions in the redactions panel and nothing in the marks one",
    ),
    Mutation(
        # The same for the words. A forwarder that drops this leaves every row
        # saying it is still reading, which is the state the panel exists to
        # distinguish from the others.
        "sidebar: accept arriving words and redraw nothing",
        "src/lib/sidebar.ts",
        "  setRedactionWords(): void {\n    this.pending.setWords();\n  }",
        "  setRedactionWords(): void {}",
        "redraws the regions when words arrive, without being given the list again",
    ),
]

#: The comment word walk's key --- a defect found while building the redaction
#: panel's own walk, and fixed on 2026-08-26. The first of these three
#: reintroduces exactly what shipped.
ROWS += [
    Mutation(
        # Key the answered set by page rather than by comment. `comment.page` is
        # a SLOT, so a deletion renumbers it: the walk then refuses the page
        # that moved into an answered slot and the comments on it read "no
        # comment" for the rest of the session.
        "comments: record a page as answered rather than the comments on it",
        "src/lib/comments.ts",
        "    if (!answered.has(comment.id) && needsWords(comment)) pages.add(comment.page);",
        "    if (!answered.has(comment.page) && needsWords(comment)) pages.add(comment.page);",
        "keeps asking for a comment a deletion moved into an answered slot",
    ),
    Mutation(
        # Ask again for everything, every round. The walk never terminates on a
        # document with a bare highlight on it, and every edit re-extracts the
        # page in front of the one the reader is looking at.
        "comments: ignore what has already been answered",
        "src/lib/comments.ts",
        "    if (!answered.has(comment.id) && needsWords(comment)) pages.add(comment.page);",
        "    if (needsWords(comment)) pages.add(comment.page);",
        "leaves out a comment already answered, and keeps its page for the others",
    ),
    Mutation(
        # Record every comment on the page as answered, not only the ones an
        # extraction would answer. A comment that later loses its body would
        # then never be asked about, because it was marked answered while it
        # still had one.
        "comments: name every comment on the page, not the ones wanting words",
        "src/lib/comments.ts",
        "  return items.filter((comment) => comment.page === page && needsWords(comment));",
        "  return items.filter((comment) => comment.page === page);",
        "names the comments on a page whose words are worth asking for",
    ),
]

#: Redacting: the panel's warning, the pairing, and the sentence a reader gets.
ROWS += [
    Mutation(
        # Draw a warning for a region whose plan has not arrived. "Nothing to
        # warn about" and "not asked yet" then look alike, which is the
        # distinction the rest of this panel is built on.
        "redactlist: warn about a region whose plan has not arrived",
        "src/lib/redactlist.ts",
        "  if (!plan || plan.unhandled.length === 0) return \"\";",
        "  if (plan && plan.unhandled.length === 0) return \"\";",
        "says nothing at all when the plan has not arrived",
    ),
    Mutation(
        # Report every unhandled object as its own sentence. A page with three
        # pictures then says the same thing three times, and a reader cannot
        # tell three findings from one printed thrice.
        "redactlist: repeat a kind rather than counting it",
        "src/lib/redactlist.ts",
        "    const key = `${place} ${drawn}`;",
        "    const key = `${place} ${drawn} ${object.at}`;",
        "counts objects of a kind rather than repeating the sentence",
    ),
    Mutation(
        # Say only what kind an object is, never that the document repeats
        # it. A letterhead left behind then reads as a picture tpdf cannot
        # handle, which sends a reader looking for another tool rather than
        # at their own document -- and the sentence is right for every other
        # finding, so nothing else in the panel can tell.
        "redactlist: drop the repeat count from what a warning says",
        "src/lib/redactlist.ts",
        "    const drawn = object.drawn ?? 0;",
        "    const drawn = 0;",
        "says a picture stays because the document repeats it",
    ),
    Mutation(
        # Pluralise the whole phrase rather than the kind inside it. `2
        # image drawn 22 timess` reads as a typo rather than as a count, and
        # the row saying it is the row a reader is deciding on.
        "redactlist: pluralise the phrase rather than the kind in it",
        "src/lib/redactlist.ts",
        """      const what = many === 1 ? one : TEXT().many(many, several);
      return drawn > 0 ? TEXT().drawnTimes(what, drawn) : what;""",
        """      const what = drawn > 0 ? TEXT().drawnTimes(one, drawn) : one;
      return many === 1 ? what : TEXT().many(many, `${what}s`);""",
        "pluralises the kind rather than the count in the phrase",
    ),
    Mutation(
        # Leave the kinds in PDFium's enumeration order, so two regions covering
        # the same two kinds read differently depending on the file.
        "redactlist: list the kinds in the order the file happened to draw them",
        "src/lib/redactlist.ts",
        "    .sort((a, b) => a.place - b.place || a.drawn - b.drawn)",
        "    .sort(() => 0)",
        "names every kind, in an order that does not depend on the file",
    ),
    Mutation(
        # Draw the warning on every row. The control in the panel test is what
        # says a warning is a fact about the region rather than chrome.
        "redactlist: draw a warning line whether or not there is a warning",
        "src/lib/redactlist.ts",
        "    if (warning) {",
        "    if (warning !== undefined) {",
        "draws the warning under the words, and nothing when there is none",
    ),
    Mutation(
        # Zip a short reply onto the regions anyway. Every plan after the
        # missing one then describes the wrong rectangle, which is a reader
        # shown the wrong words beside the wrong region.
        "pages: pair plans with regions even when the counts disagree",
        "src/lib/pages.ts",
        "  if (asked.length !== plans.length) return out;",
        "  if (asked.length < plans.length) return out;",
        "attaches nothing at all when the counts disagree",
    ),
    Mutation(
        # Say nothing after a clean redaction, the way a copy does. The reader
        # has just destroyed content on the strength of a claim they never see.
        "recovery: stay silent after a redaction that verified",
        "src/lib/recovery.ts",
        "      `Redacted ${removed}. tpdf read the file back and none of the removed words are in it.`,",
        "      ``,",
        "always says something, unlike a copy",
    ),
    Mutation(
        # Drop the reasons. "Could not prove the file is clean" with nothing
        # after it is the bare failure §6 forbids as firmly as the bare success.
        "recovery: report a redaction as unproven without saying why",
        "src/lib/recovery.ts",
        '      TEXT().redactionNotVerified(removed, applied.why.join("; "));',
        '      TEXT().redactionNotVerified(removed, "");',
        "names every reason it could not prove the file clean",
    ),
    Mutation(
        # Let the changed-source note replace the verdict rather than follow it.
        # Two facts, and the reader needs both.
        "recovery: let the changed-source note stand in for the verdict",
        "src/lib/recovery.ts",
        "  return `${verdict}${TEXT().redactionAlsoChanged}`;",
        "  return TEXT().redactionAlsoChanged;",
        "adds the changed-source note without dropping the verdict",
    ),
    Mutation(
        # Warn only when there is unsaved work, the way a reload does. The
        # commonest in-place redaction is of a document whose only edit is the
        # marks, and this is the shape that would go quiet on it -- and it is
        # the shape somebody copying `beforeReload` would write.
        "recovery: warn before an in-place redaction only when the document is dirty",
        "src/lib/recovery.ts",
        "export function beforeRedactingInPlace(name: string): Prompt {\n  return {",
        "export function beforeRedactingInPlace(name: string): Prompt {\n  if (name) return { message: \"\", offers: [] };\n  return {",
        "always warns, where a reload only warns when there is work to lose",
    ),
    Mutation(
        # Offer the redaction first. The reader's eye goes to the leading button
        # and Save a copy is the only way left to keep an unredacted file.
        "recovery: put the redaction ahead of the copy it is the escape from",
        "src/lib/recovery.ts",
        '    offers: ["saveCopy", "redact"],',
        '    offers: ["redact", "saveCopy"],',
        "offers the copy first, which is the only way left to keep one",
    ),
    Mutation(
        # Drop the half a reader is most likely to assume is untrue. "No undo"
        # is survivable if a copy exists; the sentence's work is saying none
        # does.
        "recovery: warn about the undo without saying the original goes too",
        "src/lib/recovery.ts",
        '      "and no original left afterwards. Save a copy first to keep one.",',
        '      "afterwards. Save a copy first to keep one.",',
        "says what it costs rather than asking whether to continue",
    ),
    Mutation(
        # Name no file. A reader with two windows open cannot tell which one
        # they are about to destroy, and the message reads perfectly.
        "recovery: warn about an in-place redaction without saying which file",
        "src/lib/recovery.ts",
        "      `Redacting removes the marked text from ${name} itself. There is no undo ` +",
        "      `Redacting removes the marked text from this file itself. There is no undo ` +",
        "names the file, so a reader with two windows open knows which",
    ),
    Mutation(
        # A registered command that reaches no action -- the shape that shipped
        # inert once before, and which no type error and no registry sweep can
        # see. Aimed at the destructive one of the pair on purpose.
        "appcommands: register Redact and save without reaching its action",
        "src/lib/appcommands.ts",
        "      run: () => actions.redactDocument(),",
        "      run: () => {},",
        "redact the open file through the command, with no value to carry",
    ),
    Mutation(
        # One box around the page's runs instead of one region per run. A
        # bounding box over a selection that spans lines covers everything
        # between them, which on a two-column page is the other column.
        "redactsel: put one box around a page's runs rather than one per run",
        "src/lib/selection.ts",
        "    areas.push([left, top, right, bottom]);",
        "    if (areas.length > 0) {\n      const [l, t, r, b] = areas[0] as [number, number, number, number];\n      areas[0] = [\n        Math.min(l, left),\n        Math.min(t, top),\n        Math.max(r, right),\n        Math.max(b, bottom),\n      ];\n    } else {\n      areas.push([left, top, right, bottom]);\n    }",
        "turns three runs into three regions, and not into one box around them",
    ),
    Mutation(
        # Keep a run with no width. It holds no glyph's centre, so it can only
        # remove nothing -- and a row in the review list that will never remove
        # anything makes the list overstate what is about to happen.
        "redactsel: mark a run with no width",
        "src/lib/selection.ts",
        "    if (right - left < MIN_REDACTION_SIDE) continue;",
        "",
        "drops a run with no width and keeps the one beside it",
    ),
    Mutation(
        # The other side, which a width-only check cannot see.
        "redactsel: mark a run with no height",
        "src/lib/selection.ts",
        "    if (bottom - top < MIN_REDACTION_SIDE) continue;",
        "",
        "drops a run with no height and keeps the one beside it",
    ),
    Mutation(
        # Measure the bound against the area rather than against each side. A
        # run a hundredth of a point wide and two hundred long has a larger area
        # than many real words and is still nothing.
        "redactsel: bound the region's area rather than its sides",
        "src/lib/selection.ts",
        "    if (right - left < MIN_REDACTION_SIDE) continue;",
        "    if ((right - left) * (bottom - top) < MIN_REDACTION_SIDE) continue;",
        "measures the bound against each side rather than against the area",
    ),
    Mutation(
        # Trust the sides as they arrive. Nothing in the viewer produces a run
        # with its right edge left of its left one, and a region is a claim
        # about what will be destroyed.
        "redactsel: trust the order of a run's sides",
        "src/lib/selection.ts",
        "    const left = Math.min(a, c);\n    const right = Math.max(a, c);",
        "    const left = a;\n    const right = c;",
        "orders the sides rather than trusting them",
    ),
    Mutation(
        # Read a trailing group that is not a whole run. The `NaN` defaults then
        # reach the arithmetic, and every comparison with a `NaN` is false -- so
        # the side bound does not stop it either and a region equal to nothing
        # is marked for removal.
        #
        # The first version of this mutation loosened the LOOP BOUND instead and
        # SURVIVED, correctly: the bound and this check were two mechanisms for
        # one limit, so removing either left the other rescuing the partial run.
        # The fix was to keep one of them, not to strengthen the test.
        "redactsel: read a trailing group that is not a whole run",
        "src/lib/selection.ts",
        "    if (run.length < 4) continue;",
        "",
        "drops a trailing group that is not a whole run",
    ),
    Mutation(
        # Arm the drag on a reader who has already said which words they meant.
        # The palette entry looks correct throughout, which is why this command
        # is aimed separately in the window harness rather than covered by its
        # neighbour.
        "redactsel: reach the drag's action from the selection command",
        "src/lib/appcommands.ts",
        "      run: () => actions.redactSelection(),",
        "      run: () => actions.redactRegion(),",
        "reaches the selection's own action rather than the drag's",
    ),
    Mutation(
        # Offer it with nothing selected. It would mark nothing, and a command
        # that runs and does nothing reads as a broken command -- which is the
        # same reason the four mark commands carry this guard.
        "redactsel: offer the selection command with nothing selected",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.hasSelection(),\n      run: () => actions.redactSelection(),",
        "      enabled: withDocument,\n      run: () => actions.redactSelection(),",
        "withholds redacting a selection when there is none",
    ),
    Mutation(
        # Mark the first MAX_MATCHES_TO_MARK and report success. The reader then
        # reviews a list that UNDERSTATES their own search, applies it, and is
        # told the file is clean -- which is the one thing section 6 forbids.
        "redactpat: mark as many matches as will fit rather than refusing",
        "src/lib/search.ts",
        "  if (count <= MAX_MATCHES_TO_MARK) return null;",
        "  if (count <= Number.MAX_SAFE_INTEGER) return null;",
        "permits the bound itself and refuses one more",
    ),
    Mutation(
        # Refuse one short of the bound. The mirror, and it is the direction a
        # bound tested only from above cannot see -- an implementation that
        # refused everything would pass that half.
        "redactpat: refuse a match set the bound permits",
        "src/lib/search.ts",
        "  if (count <= MAX_MATCHES_TO_MARK) return null;",
        "  if (count < MAX_MATCHES_TO_MARK) return null;",
        "permits the bound itself and refuses one more",
    ),
    Mutation(
        # Set the bound where the pathological case lives. Measured across 41
        # real PDFs: the single letter `e` matches 85,337 times in one of them,
        # and a review list of that size is the same as no list.
        "redactpat: put the bound past what anybody could review",
        "src/lib/search.ts",
        "export const MAX_MATCHES_TO_MARK = 500;",
        "export const MAX_MATCHES_TO_MARK = 100000;",
        "sits above every realistic pattern and far below the pathological one",
    ),
    Mutation(
        # Put the bound below the counts a real pattern produces. A
        # six-digit-or-longer number matched 123 times in one document, so this
        # makes the command refuse the work it exists for.
        "redactpat: put the bound below what a real pattern matches",
        "src/lib/search.ts",
        "export const MAX_MATCHES_TO_MARK = 500;",
        "export const MAX_MATCHES_TO_MARK = 100;",
        "sits above every realistic pattern and far below the pathological one",
    ),
    Mutation(
        # A refusal that does not say the number. It is what the reader can
        # check against the results panel in front of them; without it they have
        # to take the refusal on trust.
        "redactpat: refuse without saying how many there were",
        "src/lib/search.ts",
        "      `${count} matches is more than the ${most} that can be marked at ` +",
        "      `That is more than the ${most} that can be marked at ` +",
        "names the count and asks for a narrower search",
    ),
    Mutation(
        # Sweep the document from the selection command's action. All three
        # redaction commands reach an action, so the registry sweep passes.
        "redactpat: reach a sibling's action from the pattern command",
        "src/lib/appcommands.ts",
        "      run: () => actions.redactMatches(),",
        "      run: () => actions.redactSelection(),",
        "reaches the pattern command's own action rather than a sibling's",
    ),
    Mutation(
        # Offer it with no matches. It would mark nothing, which reads as a
        # broken command -- the same reason its two siblings carry a guard.
        "redactpat: offer the pattern command with no matches",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.matchCount() > 0,",
        "      enabled: withDocument,",
        "withholds redacting every result when the search found none",
    ),
]

# The range question `edit.insertPages` asks, 2026-09-19: the waiting file's
# lifetime on the webview side, the answer's grammar, and the palette's
# dismissal hook that releases a file nobody answered for. What each of these
# costs is a sandboxed worker pool held until the document closes, or pages of
# the wrong file, or the wrong pages of the right one.
ROWS += [
    Mutation(
        # Forget the previous file without releasing it.
        "pendingimport: replace a waiting file without releasing it",
        "src/lib/pendingimport.ts",
        "    if (previous) this.#release(previous.doc, previous.pending);",
        "",
        "releases the file a second prepare replaces",
    ),
    Mutation(
        # Hand over an import prepared for another tab, placing its pages in
        # the document the reader moved to.
        "pendingimport: commit another document's file",
        "src/lib/pendingimport.ts",
        "    if (held.doc !== doc) {\n      this.#release(held.doc, held.pending);\n      return null;\n    }",
        "",
        "releases rather than hands over an import prepared for another document",
    ),
    Mutation(
        # Offer the question on whichever tab is showing.
        "pendingimport: offer the waiting file on every document",
        "src/lib/pendingimport.ts",
        "    return held !== null && held.doc === doc ? held : null;",
        "    return held;",
        "is offered only for the document it was prepared for",
    ),
    Mutation(
        # Forget on dismissal without ending the backend's wait.
        "pendingimport: drop without releasing",
        "src/lib/pendingimport.ts",
        "    if (held) this.#release(held.doc, held.pending);\n  }\n}",
        "  }\n}",
        "releases the file once when the question is dropped",
    ),
    Mutation(
        # Read a blank answer the way extract does, as a refusal.
        "pendingimport: refuse a blank answer",
        "src/lib/pendingimport.ts",
        "  if (raw.trim() === \"\") {\n    return { slots: Array.from({ length: file.pages }, (_, page) => page) };\n  }",
        "",
        "reads a blank answer as every page, in the file's order",
    ),
    Mutation(
        # Tell the reader "this document" has a count that is the other file's.
        "pendingimport: word a page past the end as the document's",
        "src/lib/pendingimport.ts",
        "  return parsePageRange(raw, file.pages, file.name);",
        "  return parsePageRange(raw, file.pages);",
        "names the file, not the document, when a page is past its end",
    ),
    Mutation(
        # List the question when no file is waiting.
        "appcommands: offer the range question with nothing waiting",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.pendingImport() !== null,",
        "      enabled: withDocument,",
        "is not offered unless a file is waiting",
    ),
    Mutation(
        # Leave the question with no dismissal: the file stays open.
        "appcommands: release nothing when the range question is dismissed",
        "src/lib/appcommands.ts",
        "        dismissed: () => actions.dropImport(),",
        "",
        "releases the file when the question is dismissed",
    ),
    Mutation(
        # Read the range against the open document's page count.
        "appcommands: read the range against the document's count",
        "src/lib/appcommands.ts",
        """          if (!file) return TEXT(locale).noFileWaiting;
          return chosenPages(raw, file).problem ?? null;""",
        """          if (!file) return TEXT(locale).noFileWaiting;
          return chosenPages(raw, { ...file, pages: actions.pageCount() }).problem ?? null;""",
        "reads a range against the other file's count, not the document's",
    ),
    Mutation(
        # Close without telling the command it was abandoned.
        "palette: close without a dismissal",
        "src/lib/palette.ts",
        "    this.backdrop.style.display = \"none\";\n    this.leave();",
        "    this.backdrop.style.display = \"none\";\n    this.asking = null;",
        "is a dismissal when the palette is closed",
    ),
    Mutation(
        # Escape back to the list without telling the command.
        "palette: Escape back to the list without a dismissal",
        "src/lib/palette.ts",
        "  private stopAsking(): void {\n    this.leave();",
        "  private stopAsking(): void {\n    this.asking = null;",
        "is a dismissal on Escape back to the list, once",
    ),
    Mutation(
        # Report an answered question as abandoned, which releases the file
        # the answer is about to insert from.
        "palette: count an answer as a dismissal",
        "src/lib/palette.ts",
        "    this.asking = null;\n    this.close();\n    // Unless",
        "    this.close();\n    // Unless",
        "is not a dismissal when the question is answered",
    ),
    Mutation(
        # Swallow a run the registry refused, leaving the file held.
        "palette: hold on to a question the registry would not run",
        "src/lib/palette.ts",
        "    if (!this.registry.run(command.id, raw)) command.argument.dismissed?.();",
        "    this.registry.run(command.id, raw);",
        "is a dismissal when the command can no longer run as answered",
    ),
]

# --- the signature integrity row -------------------------------------------
#
# `integrity.ts`, 2026-09-26: the one verdict the properties dialog gives, so
# each mutation is a way it could say more than was checked.
ROWS += [
    Mutation(
        # Drop the sentence that stops "intact" meaning "trustworthy".
        "integrity: call a signature intact without saying trust was not checked",
        "src/lib/integrity.ts",
        '  const owner = trustFollows ? "" : ` ${said.trustNotChecked}`;',
        '  const owner = "";',
        "says after every answer that could mean trust that trust was not checked",
    ),
    Mutation(
        # Call a SHA-1 match intact.
        "integrity: render a weak match as intact",
        "src/lib/integrity.ts",
        "`unchanged under SHA-1 only — the digest and the signature match` +",
        "`intact — the digest and the signature match` +",
        "never calls a SHA-1 match intact",
    ),
    Mutation(
        # An altered signature shown without the warning mark.
        "integrity: leave an altered signature unmarked",
        "src/lib/integrity.ts",
        '  return row(said.integrityName, value, integrity.verdict !== "intact");',
        '  return row(said.integrityName, value, integrity.verdict !== "intact" && integrity.verdict !== "altered");',
        "marks everything but an intact signature for a reader's attention",
    ),
    Mutation(
        # An unchecked signature shown without the warning mark --- the
        # reassuring branch, reached by not looking.
        "integrity: leave an unchecked signature unmarked",
        "src/lib/integrity.ts",
        '  return row(said.integrityName, value, integrity.verdict !== "intact");',
        '  return row(said.integrityName, value, integrity.verdict !== "intact" && integrity.verdict !== "unchecked");',
        "marks everything but an intact signature for a reader's attention",
    ),
    Mutation(
        # Say nothing about what was appended after an intact signature.
        "integrity: call a signature intact without mentioning what was appended",
        "src/lib/integrity.ts",
        "  const value = said.integrity(integrity.verdict, how(integrity), appended > 0, owner, why);",
        "  const value = said.integrity(integrity.verdict, how(integrity), false, owner, why);",
        "says an intact signature does not cover what was appended after it",
    ),
    Mutation(
        # Leave the reason out of an unchecked row.
        "integrity: refuse without the reason",
        "src/lib/integrity.ts",
        "  const why = integrity.why ? WHY[integrity.why] : said.noReason;",
        "  const why = said.noReason;",
        "gives each reason its own sentence, and says an unchecked one means nothing",
    ),
    Mutation(
        # Build the verdict row and never show it: the shape a whole feature
        # shipped inert once, which is why `check_viewer_wiring.py` exists.
        "properties: build the integrity row and never show it",
        "src/lib/properties.ts",
        "  if (verdict) rows.push(verdict);",
        "",
        "comes first, above who the signature says signed it",
    ),
]

# The trust row, 2026-09-27: the second verdict, from `trust.rs`, and the
# words it is carried in. Each mutation removes one thing the row must say.
ROWS += [
    Mutation(
        # Build the trust row and never show it --- the inert-feature shape.
        "properties: build the trust row and never show it",
        "src/lib/properties.ts",
        "  if (trusted) rows.push(trusted);",
        "",
        "follows the integrity row, and replaces its not-checked sentence",
    ),
    Mutation(
        # Keep saying ownership was not checked above a row that checked it.
        "properties: keep the not-checked sentence above a trust row",
        "src/lib/properties.ts",
        "  const verdict = integrityRow(signature.integrity, signature.appended_bytes, !!trusted);",
        "  const verdict = integrityRow(signature.integrity, signature.appended_bytes);",
        "follows the integrity row, and replaces its not-checked sentence",
    ),
    Mutation(
        # Let "trusted" speak for revocation. Until 2026-09-28 this mutation
        # dropped the sentence saying revocation was not checked; revocation is
        # a row of its own since, and the defect worth catching is now the
        # trust row claiming what only that row may say.
        "trust: call a trusted chain unrevoked",
        "src/lib/integrity.ts",
        "            `key belongs to the person the certificate names.`",
        "            `key belongs to the person the certificate names, and it has not been revoked.`",
        "leaves revocation to its own row, and no longer speaks for it",
    ),
    Mutation(
        # Say "trusts" without saying whose store.
        "trust: name no store",
        "src/lib/integrity.ts",
        "      const chained = `the signer's certificate chains to a root ${w.computer} trusts`;",
        "      const chained = `the signer's certificate chains to a trusted root`;",
        "names the store that answered",
    ),
    Mutation(
        # Leave the date out of an expired standing.
        "trust: say expired without the date it ran out",
        "src/lib/integrity.ts",
        '            `expired — ${chained}, and it ran out${w.until ? ` on ${w.until}` : ""}. The ` +',
        "            `expired — ${chained}, and it ran out. The ` +",
        "says an expired certificate may have been in force when used, and cannot tell",
    ),
    Mutation(
        # Drop the Adobe sentence: the reader whose colleague sees "trusted"
        # in Acrobat is then left with a contradiction and no explanation.
        "trust: leave Adobe's list out of an untrusted root",
        "src/lib/integrity.ts",
        """            `issued to themselves reads this way, and so does one whose root only ` +
            `Adobe's trust list carries`""",
        "            `issued to themselves reads this way`",
        "says a root only Adobe's list carries reads as untrusted here",
    ),
    Mutation(
        # An untrusted chain shown without the warning mark.
        "trust: leave an untrusted chain unmarked",
        "src/lib/integrity.ts",
        """  const fine = trust.standing === "trusted" || trust.standing === "trusted_at_timestamp";
  return row(said.trustName, value, !fine);""",
        """  const fine = trust.standing === "trusted" || trust.standing === "trusted_at_timestamp";
  return row(said.trustName, value, !fine && trust.standing !== "untrusted");""",
        "marks everything but a trusted chain for a reader's attention",
    ),
    Mutation(
        # Read the doubt from the wrong table entry: every reason the same.
        "trust: give every doubt the same sentence",
        "src/lib/integrity.ts",
        "  const why = trust.why ? DOUBT[trust.why](computer) : said.noReason;",
        "  const why = DOUBT.rejected(computer);",
        "gives each doubt its own sentence",
    ),
    Mutation(
        # Date an expired standing from nothing: the certificate's own dates are
        # what make "ran out on" true.
        "properties: date the trust row from nothing",
        "src/lib/properties.ts",
        "    signature.certificate?.until,",
        "    undefined,",
        "dates an expired standing from the certificate the dialog shows",
    ),
]

ROWS += [
    Mutation(
        "redaction walk: keep the slot read before the text wait",
        "src/lib/redactlist.ts",
        "    if (walk.slotOf(next.page) !== slot) continue;",
        "    if (false) continue;",
        "a plan asked before a deletion above is not stored for the shifted page",
    ),
    Mutation(
        "redaction walk: store a plan for a page the region no longer names",
        "src/lib/redactlist.ts",
        "        if (now === undefined || walk.sourceOf(now) !== source) {",
        "        if (false) {",
        "asks again when the region's own page was replaced while its plan was computed",
    ),
    Mutation(
        "tabs: restore a kept tab without one of its fields",
        "src/lib/documenttabs.ts",
        "  return { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, messageSuccess, offers, notice,\n",
        "  return { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, messageSuccess, offers,\n",
        "restores every field that was kept, and nothing from another tab",
    ),
    Mutation(
        "tabs: switching away keeps nothing",
        "src/lib/documenttabs.ts",
        "  Object.assign(tab, state);",
        "  void tab; void state;",
        "restores every field that was kept, and nothing from another tab",
    ),
    Mutation(
        "tabs: a fresh document starts with a search already typed",
        "src/lib/documenttabs.ts",
        '    query: "",',
        '    query: " ",',
        "restores the fresh state for a document that was never kept",
    ),
    Mutation(
        "signature dialog: place a signature whose save finished after the reader moved on",
        "src/lib/signaturedialog.ts",
        "    if (generation === this.generation && this.isOpen) { this.finish(image); return; }",
        "    if (true) { this.finish(image); return; }",
        "does not hand a late-remembered signature to the next time the dialog is asked",
    ),
    Mutation(
        "signature dialog: say nothing when a late save kept a signature it did not place",
        "src/lib/signaturedialog.ts",
        "    if (remembering && this.isOpen)\n",
        "    if (false)\n",
        "says the earlier signature was kept when the reader drew again while it was being saved",
    ),
    Mutation(
        "signature dialog: a saved signature replaces a stroke made while it loaded",
        "src/lib/signaturedialog.ts",
        "      const saved = await loadSignature();\n      if (generation !== this.generation || !this.isOpen) return;",
        "      const saved = await loadSignature();\n      if (!this.isOpen) return;",
        "ignores a saved signature that arrives after a new stroke",
    ),
    Mutation(
        "signature dialog: report a failed import that a stroke replaced",
        "src/lib/signaturedialog.ts",
        "      if (generation === this.generation)\n        this.message.textContent = error instanceof Error",
        "      if (true)\n        this.message.textContent = error instanceof Error",
        "does not report a failed import that a new stroke replaced",
    ),
    Mutation(
        "signature dialog: draw an import over a stroke made while it decoded",
        "src/lib/signaturedialog.ts",
        "      image = await decodeSignature(file);\n      if (generation !== this.generation || !this.isOpen) return;",
        "      image = await decodeSignature(file);\n      if (!this.isOpen) return;",
        "does not draw an imported image over a stroke made while it was decoding",
    ),
]

# --- what a document says about itself ------------------------------------
ROWS += [
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
        """    value: said.notWholeShort(formatBytes(short)),
    warn: true,""",
        """    value: said.notWholeShort(formatBytes(short)),
    warn: false,""",
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
        "      value: said.upToSignature(formatBytes(signature.appended_bytes)),",
        "      value: said.upToSignature(formatBytes(bytes - signature.covered_bytes)),",
        "does not let the container reach the number a reader is shown",
    ),
    Mutation(
        # Go back to claiming the whole file. Every signature excludes its own
        # container, so this is false of all of them -- and it is what made the
        # appended row above read as though the container were a second problem
        # rather than the same one, named once.
        "properties: claim a signature covers the whole file",
        "src/lib/properties.ts",
        '    wholeFile: "the whole file, except the signature container it cannot cover",',
        '    wholeFile: "the whole file",',
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
        "  return [...locked, ...counted, ...signatures, ...described, ...security, file, ...cut];",
        "  return [...locked, file, ...counted, ...signatures, ...described, ...security, ...cut];",
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
        # Re-aimed 2026-10-16: `formatBytes` moved to `bytes.ts`.
        "src/lib/bytes.ts",
        "  if (bytes < 1024) return exact;",
        "  if (bytes < 0) return exact;",
        "gives a plain count below a kilobyte, with no rounded size beside it",
    ),
    Mutation(
        # Let a size that is not a number through. `NaN bytes` reads as a
        # measurement of the file rather than as the absence of one.
        "properties: format a size that is not a number as though it were",
        "src/lib/bytes.ts",
        "  if (!Number.isFinite(bytes) || bytes < 0) return text.unknown;",
        "  if (bytes < -1) return text.unknown;",
        "says so rather than printing NaN for a size it was not given",
    ),
    Mutation(
        # Describe an ordinary approval signature as a certification. Level 0 is
        # not one of the three the specification defines, and naming it makes a
        # claim about what the signer intended.
        "properties: describe every signature as a certification",
        "src/lib/properties.ts",
        """            ? "certified, form filling and comments permitted"
            : "",""",
        """            ? "certified, form filling and comments permitted"
            : "certified, no changes permitted",""",
        "says nothing for a level the specification does not define",
    ),
    Mutation(
        # The same for a lock: a signature that locks nothing said to lock.
        "properties: describe every signature as locking the document",
        "src/lib/properties.ts",
        """            ? "locked by this signature, form filling, signing and comments permitted"
            : "",""",
        """            ? "locked by this signature, form filling, signing and comments permitted"
            : "locked by this signature, no changes permitted",""",
        "is worded by level, on certificationOf's terms, and is a row of its own",
    ),
    Mutation(
        "properties: list no row for a signature's lock",
        "src/lib/properties.ts",
        """  if (lock) rows.push({ name: said.lockName, value: lock });
""",
        "",
        "is listed under the certification, and not at all for a signature that locks nothing",
    ),
    Mutation(
        "save warning: say nothing of its own for a signature that locks",
        "src/lib/signedsave.ts",
        "signature.lock === 1 || signature.certification === 1",
        "signature.certification === 1",
        "warns for signed documents and every certification permission, including allowed form filling",
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

  claimed(said.signerTyped, signature.name);""",
        """  claimed(said.signerTyped, signature.name);

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
        "      value: said.selfIssued,",
        "      value: certificate.issuer_cn || certificate.issuer,",
        "says a self-issued certificate was vouched for by nobody",
    ),
    Mutation(
        # Warn on a self-issued certificate. The opposite error and the more
        # tempting one: it looks like caution, and it is a verdict tpdf has no
        # trust store with which to reach.
        "certificate: treat self-issued as something to warn about",
        "src/lib/properties.ts",
        """      name: said.issuedBy,
      value: said.selfIssued,
    });""",
        """      name: said.issuedBy,
      value: said.selfIssued,
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
        """  say(said.certificatesName, limits.certificates_unread, said.certificatesUnread);
""",
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
        ? said.keyNotStated
        : usage.length > 0
          ? usage.join(", ")
          : said.keyNothing,""",
        """      usage === null || usage.length === 0
        ? said.keyNothing
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
        "      `${claims} — the document's own claim, which tpdf does not check`,",
        "      claims,",
        "shows a claim as a claim",
    ),
    Mutation(
        # Show only the first standard a document claims, so a file stating both
        # PDF/A and PDF/UA is reported as stating one.
        "conformance: show only the first standard a document claims",
        "src/lib/properties.ts",
        '      value: said.conformance(xmp.conformance.join(", ")),',
        "      value: said.conformance(xmp.conformance[0]),",
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
        # Show the attested time with no authority named and nothing said
        # about what was checked. It then reads as a bare fact.
        "timestamp: state an attested time as a bare fact",
        "src/lib/integrity.ts",
        """            `${when}, attested by ${authority} — the timestamp checks out under the key ` +
            `in its certificate and covers ${subject}${how}.`""",
        "            when",
        "names the authority beside the time, and says the time is attested",
    ),
    Mutation(
        # Call a broken token's time attested: the words undo the verdict the
        # worker reached before it believed the time.
        "timestamp: call a broken token's time attested",
        "src/lib/integrity.ts",
        "          `not attested — a timestamp naming ${authority} states ${when}, but its own ` +",
        "          `${when}, attested by ${authority} — but its own ` +",
        "calls the time attested only when the token is intact or weak",
    ),
    Mutation(
        # Leave the authority's standing out of the readout.
        "timestamp: drop the authority's standing",
        "src/lib/properties.ts",
        "    if (authority) rows.push(authority);",
        "",
        "puts the authority's standing under the time, and only when there is one",
    ),
    Mutation(
        # Word a document timestamp as a signature's.
        "timestamp: word a document timestamp as a signature's",
        "src/lib/properties.ts",
        "  const document = signature.kind === DOCUMENT_TIMESTAMP;",
        "  const document = false;",
        "says a document timestamp covers the document, not a signature",
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
        "  claimed(said.dateGiven, signature.when);",
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
ROWS += [
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
        "        let quad =\n"
        '          kind === "note"\n'
        "            ? iconQuad(lands.x, lands.y, this.laidSize(live.slot))\n"
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));",
        "        let quad = boxQuad(live.from, live.to, this.laidSize(live.slot));",
        "drops the bubble where the reader pressed, from a click alone",
    ),
    Mutation(
        # Take Enter for any armed tool, which drops a zero-sized box on a
        # keystroke the reader meant for something else.
        "comment: place a mark on Enter whatever tool is armed",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        '    } else if (event.key === "Enter" && this.drawArmed === "note") {',
        '    } else if (event.key === "Enter" && this.drawArmed !== null) {',
        "takes Enter only for the comment tool",
    ),
    Mutation(
        # Leave the comment tool armed after it places one, so the reader's next
        # press --- on a link, on a word --- becomes another bubble.
        "comment: keep the tool armed after a bubble is placed",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # Code only now: the comment above these lines was reworded by that
        # change, and an anchor carrying prose is re-aimed by every rewording.
        '        const stamp = this.tool.kind === "draw" ? this.tool.stamp : null;\n'
        '        if (this.tool.kind === "draw") this.tool = NO_TOOL;',
        '        const stamp = this.tool.kind === "draw" ? this.tool.stamp : null;\n'
        '        if (kind !== "note" && this.tool.kind === "draw") this.tool = NO_TOOL;',
        "spends the tool on that press, and takes no second comment",
    ),
    Mutation(
        # Drop the three mode fields from the summary, which is what decides
        # whether `onStatus` fires at all. The status line then hears about an
        # armed tool only when something unrelated moves.
        "status: leave the mode fields out of the summary that gates a report",
        "src/lib/viewer.ts",
        "      status.drawing,\n      status.corners,\n      // Both halves, because either can move on its own: a sweep across a\n"
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
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # Re-aimed 2026-09-27 when the signature placement joined the crop's drag.
        # Re-aimed 2026-10-16 when the region to copy or save joined the crop's drag.
        '          : this.tool.kind === "region"\n'
        '            ? "region"\n'
        "            : this.drawnStrokes === null\n"
        "            ? this.drawArmed\n"
        "            : null,",
        '          : this.tool.kind === "region"\n'
        '            ? "region"\n'
        "            : this.drawArmed,",
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
        '    case "line":\n'
        '    case "arrow":\n'
        '    case "polygon":\n'
        '    case "polyline":\n'
        "    // A stamp is placed by the reader and anchored to nothing, so it moves for\n"
        "    // the box's reason exactly.\n"
        '    case "stamp":\n'
        '    case "signature":\n'
        '    case "picture":\n'
        "    // A link is placed by a drag and anchored to nothing, like the box.\n"
        '    case "link":\n'
        "      return true;",
        '    case "note":\n'
        "      return true;\n"
        '    case "square":\n'
        '    case "ellipse":\n'
        '    case "textbox":\n'
        '    case "ink":\n'
        '    case "line":\n'
        '    case "arrow":\n'
        '    case "polygon":\n'
        '    case "polyline":\n'
        '    case "stamp":\n'
        '    case "signature":\n'
        '    case "picture":\n'
        '    case "link":\n'
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
        "            this.clampMove(live.id, live.slot, want),\n          );",
        "            want,\n          );",
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

ROWS += [
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
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # Re-aimed 2026-09-27 when the signature placement joined the crop's drag.
        # Re-aimed 2026-10-16 when the region to copy or save joined the crop's drag.
        '        if (this.tool.kind !== "crop" && this.tool.kind !== "redact" && this.tool.kind !== "place" && this.tool.kind !== "region") {',
        "        if (false) {",
        "reports nothing until the tool is armed",
    ),
    Mutation(
        # Report the laid-out rectangle. On an uncropped, unturned page it is
        # the right answer; on a page already cropped it walks the crop further
        # in every time, which reads as a viewer that mis-draws rather than as a
        # translation that never happened.
        "crop: report the rectangle in the page's laid-out space",
        "src/lib/viewer.ts",
        # Re-aimed 2026-08-26: the two region tools now share this line, so the
        # translation is done once into `rect` rather than inside the call.
        "        const rect = this.fileRectOn(live.slot, quad);",
        "        const rect = [quad.left, quad.top, quad.right, quad.bottom] as\n"
        "          [number, number, number, number];",
        "reports the rectangle in the file's space and not the page's",
    ),
    Mutation(
        # Leave the tool armed. Every drawing tool is one-shot and a crop has a
        # stronger reason to be: a second crop replaces the first, so a reader
        # who did not notice it was still armed loses the crop they just made.
        "crop: leave the crop tool armed after a rectangle",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # Re-aimed 2026-09-27 when the signature placement joined the crop's drag.
        # Re-aimed 2026-10-07 when the redaction tool left this line to stay armed.
        # Re-aimed 2026-10-16 when the region to copy or save joined the crop's drag.
        '        if (this.tool.kind === "crop" || this.tool.kind === "place" || this.tool.kind === "region") {\n'
        "          this.tool = NO_TOOL;\n"
        "        }",
        '        if (this.tool.kind === "crop" || this.tool.kind === "place" || this.tool.kind === "region") {\n'
        '          this.tool = { kind: "crop" };\n'
        "        }",
        "is spent by one rectangle",
    ),
    Mutation(
        # Escape's ladder without the crop's two fields. It falls through to
        # clearing the selection, so a reader who presses Escape mid-drag
        # watches the rectangle stay and then commits it by letting go.
        "crop: leave the crop out of what Escape can reach",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # The crop's two states are the `crop` variant and `cropDrawing`, and
        # the after keeps every other tool's reach so that only the crop's goes.
        '        this.tool.kind !== "none" ||\n'
        "        this.drawing ||\n"
        "        this.inking ||\n"
        "        this.doomed ||\n"
        "        this.cropDrawing",
        '        this.tool.kind === "draw" ||\n'
        '        this.tool.kind === "erase" ||\n'
        '        this.tool.kind === "redact" ||\n'
        "        this.drawing ||\n"
        "        this.inking ||\n"
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
        # Fill `armed` from the drawing tool alone, which is what it did before
        # the crop existed. The reader arms the crop from the palette, gets a
        # crosshair and no words, and has nothing telling them what their next
        # press will do -- the exact complaint the field was added for. The
        # accessors all still answer correctly, so only a test reading the
        # status can see it.
        "crop: leave the armed crop out of the status the window reads",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        # Re-aimed 2026-09-27 when the signature placement joined the crop's drag.
        '        : this.tool.kind === "crop"\n'
        '          ? "crop"\n'
        '          : this.tool.kind === "place"\n',
        '        : this.tool.kind === "place"\n',
        "names the armed crop, which is not a mark kind",
    ),
    Mutation(
        # Join a rewrite onto every comment rather than onto the one it names.
        # The reader edits one note and finds their words on all of them, which
        # is the shape a join keyed on nothing produces -- and every test that
        # rewrites a single comment on a single-comment fixture passes.
        "comments: apply a rewrite to whichever comment comes first",
        "src/lib/pages.ts",
        "    const edit = comment.object\n"
        "      ? notes.find(\n"
        "          (note) =>\n"
        "            note.object[0] === comment.object?.[0] &&\n"
        "            note.object[1] === comment.object[1],\n"
        "        )\n"
        "      : undefined;",
        "    const edit = comment.object ? notes[0] : undefined;",
        "writes a reader's rewrite over the comment it names, and no other",
    ),
    Mutation(
        # Match on the object number and ignore the generation. Two objects that
        # differ only there are rare in a real file and legal, and the wrong one
        # is rewritten silently.
        "comments: match a rewrite on the object number alone",
        "src/lib/pages.ts",
        "            note.object[0] === comment.object?.[0] &&\n"
        "            note.object[1] === comment.object[1],",
        "            note.object[0] === comment.object?.[0],",
        "matches on the object, never on the id or the generation",
    ),
    Mutation(
        # Take the new body and leave the old date. Every viewer shows `/M`, so
        # the reader's own words appear under somebody else's timestamp -- and
        # the file that gets saved says the same thing.
        "comments: keep the file's date when the body is rewritten",
        "src/lib/pages.ts",
        "        ? { ...comment, page: slot, body: edit.body, date: edit.shown }",
        "        ? { ...comment, page: slot, body: edit.body }",
        "writes a reader's rewrite over the comment it names, and no other",
    ),
    Mutation(
        # Let a comment with no object be edited. There is nothing an
        # incremental update could override, so the reader types into a box
        # whose text can never reach the file -- and nothing tells them.
        "comments: offer the editor for a comment with no object of its own",
        "src/lib/commentpopup.ts",
        # Widened to carry the accessor's own name on 2026-08-29, when
        # `replyable` landed with a body identical to this one and the bare
        # `return` line matched twice. The gate caught it; a narrower anchor
        # would have mutated whichever came first.
        "  get editable(): boolean {\n"
        "    return this.shown !== null && this.subject?.object != null;",
        "  get editable(): boolean {\n    return this.shown !== null;",
        "offers no editor for a comment the file wrote without an object",
    ),
    Mutation(
        # Send the body whether it changed or not. A reader who opens the
        # editor and closes it again gets an undo step in front of them for
        # nothing, and the document is reported dirty.
        "comments: send the body even when the reader changed nothing",
        "src/lib/commentpopup.ts",
        "    if (now === this.was) return;",
        "    if (false) return;",
        "sends nothing when the editor closes with the text it opened with",
    ),
    Mutation(
        # Rebuild the editor on a second arming. The reader presses Edit twice
        # -- or the command after the button -- and what they have typed is
        # replaced by the file's own words with no warning.
        "comments: let a second arming rebuild the editor",
        "src/lib/commentpopup.ts",
        # Re-aimed 2026-08-29: the guard moved into `arm`, which `edit` and
        # `reply` share, so the line it used to sit on is gone. The mutation is
        # unchanged in what it does -- it is the same guard, one level down.
        "    if (!allowed || this.editing) return;",
        "    if (!allowed) return;",
        "does not throw away what is typed when Edit is pressed twice",
    ),
    Mutation(
        # Commit on a suppressed hide. A document being closed, or a comment
        # disappearing out from under the popup, would then send text to an
        # object nobody is looking at.
        "comments: commit the body even when the caller said not to",
        "src/lib/commentpopup.ts",
        "  hide(commit = true): void {\n    if (commit) this.commit();",
        "  hide(commit = true): void {\n    if (commit || true) this.commit();",
        "sends nothing when the popup is hidden without committing",
    ),
    Mutation(
        # Drop the commit that runs when a second comment takes the box over.
        # The reader opens another note and what they typed into the first is
        # gone -- silently, because the popup looks the same either way.
        "comments: let a second comment take the box without committing the first",
        "src/lib/commentpopup.ts",
        "    this.commit();\n    this.shown = comment.id;",
        "    this.shown = comment.id;",
        "commits the first comment when a second one takes the box over",
    ),
    Mutation(
        # Offer the command with no document open. The other half of the
        # conjunction, and the half a test that only varies the popup cannot
        # see.
        "comments: offer Edit this comment with no document open",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canEditComment(),\n"
        "      run: () => actions.editComment(),",
        "      enabled: () => actions.canEditComment(),\n"
        "      run: () => actions.editComment(),",
        "withholds editing a comment with no document, however editable",
    ),
    Mutation(
        # Send a reply through the rewrite door. The reader answers a comment
        # and their words replace it instead -- somebody else's note, silently
        # overwritten, which is the worst outcome this feature can produce.
        "comments: send a reply as an edit to the comment it answers",
        "src/lib/commentpopup.ts",
        "    this.opts.onReply(comment, now);",
        "    this.opts.onRewrite(comment, now);",
        "arms an empty editor for a reply and sends it under the comment it answers",
    ),
    Mutation(
        # Open the reply box on the comment's own words. The reader edits around
        # somebody else's sentence and answers in their voice.
        "comments: open a reply box holding the comment's own words",
        "src/lib/commentpopup.ts",
        '    this.was = mode === "body" ? comment.body : "";',
        "    this.was = comment.body;",
        "arms an empty editor for a reply and sends it under the comment it answers",
    ),
    Mutation(
        # Send a reply that is only whitespace. An empty comment appears in
        # somebody's thread, which is the one outcome the reader did not ask
        # for -- and it cannot be told from a deliberate blank.
        "comments: send a reply with nothing in it",
        "src/lib/commentpopup.ts",
        "    if (!now.trim()) return;",
        "",
        "sends no reply when the box is typed into and then emptied",
    ),
    Mutation(
        # Apply the blank check to a rewrite as well. The mirror, and it is why
        # the check sits at the destination rather than in `arm`: a reader who
        # deletes a comment's text has asked for an empty comment.
        "comments: refuse to clear a body because a reply may not be blank",
        "src/lib/commentpopup.ts",
        '    if (mode === "body") {\n      this.opts.onRewrite(comment, now);\n      return;\n    }',
        '    if (mode === "body") {\n      if (now.trim()) this.opts.onRewrite(comment, now);\n      return;\n    }',
        "clears a body of the same shape, because an empty rewrite is a real edit",
    ),
    Mutation(
        # Offer a reply for a comment the file wrote as a direct dictionary.
        # `/IRT` names an object; there is nothing for the reply to point at, so
        # the button would be a promise the file cannot keep.
        "comments: offer a reply for a comment with no object of its own",
        "src/lib/commentpopup.ts",
        "  get replyable(): boolean {\n"
        "    return this.shown !== null && this.subject?.object != null;",
        "  get replyable(): boolean {\n    return this.shown !== null;",
        "offers no reply for a comment the file wrote without an object",
    ),
    Mutation(
        # Offer the command with no document open. The other half of the
        # conjunction, which a test that only varies the popup cannot see.
        "comments: offer Reply to this comment with no document open",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canReplyToComment(),",
        "      enabled: () => actions.canReplyToComment(),",
        "withholds replying to a comment with no document, however replyable",
    ),
]

ROWS += [
    # Slot versus page of the file, 2026-08-30. Each of these reverts one
    # translation between the two -- putting a slot back where a page of the
    # file belongs, which is what fifteen call sites did until `FilePage` made
    # it a type error. They do not need to type-check: the harness runs vitest,
    # which strips types without checking them, so what each one measures is
    # whether the test can see the wrong page being asked about.
    #
    # Seven mutations for nine of the sixteen sites -- two of them put a slot
    # back in both halves of one function. The other seven sites are held by the
    # type rather than by a test: `npm run check` is a gate, and a slot reaching
    # `TextCache` is an error in it, so a mutation for one of those would be
    # measuring the compiler. These nine are the ones with a behavioural
    # observable.
    Mutation(
        # The measured defect, in the place a reader meets it first: press on a
        # page and nothing happens, on any document with an edit in it.
        "viewer: place a caret from the cache under the slot's number",
        "src/lib/viewer.ts",
        "    const text = this.textOn(page);\n"
        "    if (!text) {\n"
        "      this.requestText(page);",
        "    const text = this.text.peek(page);\n"
        "    if (!text) {\n"
        "      this.requestText(page);",
        "selects what the pointer dragged over, after the page above went",
    ),
    Mutation(
        # Select-all-on-page, which reaches the cache by a different route --
        # `loadOn` rather than a press -- and so needs its own mutation.
        "viewer: select a whole page under the slot's number",
        "src/lib/viewer.ts",
        "    const text = this.textOn(page);\n"
        "    if (!text) {\n"
        "      void this.loadOn(page).then((arrived) => {",
        "    const text = this.text.peek(page);\n"
        "    if (!text) {\n"
        "      void this.text.load(page).then((arrived) => {",
        "selects a whole page, after the page above went",
    ),
    Mutation(
        # The status line's count. Wrong in the quiet direction: a selection
        # that is drawn and copies correctly while the count beside it says 0.
        "viewer: count the selection's characters under the slot's number",
        "src/lib/viewer.ts",
        "      const range = this.selection.rangeOn(page);\n"
        "      const text = range && this.textOn(page);",
        "      const range = this.selection.rangeOn(page);\n"
        "      const text = range && this.text.peek(page);",
        "counts the selected characters, after the page above went",
    ),
    Mutation(
        # The asking path, which a caller uses to read a page it has never
        # visited -- the comment-words loop in `App.svelte` is the live one.
        "viewer: ask for the unturned text of the slot's number",
        "src/lib/viewer.ts",
        "    await at.cache.load(at.page);\n"
        "    return at.cache.peekUnturned(at.page);",
        # The slot number raw, as every neighbouring page-addressing mutation
        # here passes it. It was `filePage(slot)`, which `viewer.ts` does not
        # import, so the mutation threw `ReferenceError: filePage is not
        # defined` and the red test said nothing about which page was read.
        # `check_mutation_types.py` found it.
        "    await at.cache.load(slot);\n"
        "    return at.cache.peekUnturned(slot);",
        "answers a page's unturned text, after the page above went",
    ),
    Mutation(
        # The quads a mark is built from. Wrong here writes a highlight over
        # another page's words, which outlives the session in the file.
        "viewer: take a selection's quads from the slot's number",
        "src/lib/viewer.ts",
        "      const text = this.unturnedOn(page);",
        "      const text = this.text.peekUnturned(page);",
        "hands back a selection's quads by page, after the page above went",
    ),
    Mutation(
        # Drop the crop under the slot. The extraction taken under the old box
        # stays in the cache and goes on placing carets in the wrong space.
        "viewer: drop the cropped extraction under the slot's number",
        "src/lib/viewer.ts",
        "      source.cache.setPageCrop(source.page, want);",
        # The slot raw, for the same reason as `unturnedText`'s above: the
        # `filePage` this named is not imported in `viewer.ts`, so the mutation
        # threw rather than dropping the wrong page's extraction.
        "      source.cache.setPageCrop(slot, want);",
        "drops the cropped page's extraction, not the extraction at its slot",
    ),
    Mutation(
        # `Selection` takes a lookup rather than the cache precisely so that it
        # never meets the distinction. Handing it the cache back is the shape
        # the code had, and it reads as tidier.
        "viewer: give the selection the cache instead of a lookup",
        "src/lib/viewer.ts",
        "    return this.selection ? this.selection.text((page) => this.textOn(page)) : \"\";",
        "    return this.selection ? this.selection.text((page) => this.text.peek(page)) : \"\";",
        "selects what the pointer dragged over, after the page above went",
    ),
]

ROWS += [
    # Whether asking again is worth a round trip, 2026-08-30. Four rules, one
    # mutation each. The defect these replaced -- a record of what had been
    # asked for, kept in `viewer.ts` where nothing could clear it, so a cropped
    # page was never fetched again -- has no one-line mutation, because the
    # whole of it was *where* the record lived. It was measured directly
    # instead; `viewertext.test.ts` records the numbers.
    Mutation(
        # Forget that the page already answered with nothing. A damaged page
        # then issues a fresh `page_text` on every frame for the life of the
        # document, which is the loop `selectPage`'s comment describes meeting
        # once before.
        "text: ask again for a page that already came back with nothing",
        "src/lib/text.ts",
        "      !this.unreadable.has(page)",
        "      true",
        "does not ask again for a page that could not be read",
    ),
    Mutation(
        # Forget that one is in flight. The frame loop attaches a fresh
        # continuation to the same extraction every frame -- the request is
        # deduplicated and the `.then` is not, which is what this guard is for.
        "text: ask again for a page whose extraction is in flight",
        "src/lib/text.ts",
        "      !this.pending.has(page) &&",
        "      true &&",
        "says no while the extraction is in flight",
    ),
    Mutation(
        # Forget that the text is already here. Every visible page is re-fetched
        # on every frame, which is the cache doing nothing at all.
        "text: ask again for a page whose text has already arrived",
        "src/lib/text.ts",
        "      !this.pages.has(page) &&",
        "      true &&",
        "says no for a page whose text has arrived",
    ),
    Mutation(
        # Keep the failure across a crop. A new crop box is a different
        # question, so this is the direction that reads as reasonable: it
        # remembers a true fact about an extraction nobody is asking for
        # any more.
        "text: keep a page's failure when its crop changes",
        "src/lib/text.ts",
        "    this.unreadable.delete(page);",
        "    if (false) this.unreadable.delete(page);",
        "says yes again once a crop drops a failure",
    ),
    Mutation(
        # Take a blank page for a page that could not be read. The copy is
        # refused with a message naming pages that were read without
        # difficulty, and the status line goes on showing the selection it
        # will not hand over.
        "viewer: refuse a copy because a page tpdf made has no text",
        "src/lib/viewer.ts",
        "          if (made) continue;",
        "          if (false) continue;",
        "copies a selection that runs across a page tpdf made",
    ),
]

ROWS += [
    Mutation(
        "search recovery: drop the recognised replacement",
        "src/lib/searchrecovery.ts",
        "    this.pages.set(this.key(doc, page), text);",
        "    this.pages.delete(this.key(doc, page));",
        "replaces the garbled source for search and cached selection",
    ),
    Mutation(
        "search recovery: send a mixed batch to native extraction",
        "src/lib/searchrecovery.ts",
        "{ ...args, page, pages: [page] }",
        "{ ...args, page, pages }",
        "keeps unrelated documents and pages native",
    ),
    Mutation(
        "search recovery: retain unlimited recognised text",
        "src/lib/searchrecovery.ts",
        "if (total > 1_000_000)",
        "if (total < 0)",
        "bounds retained text instead of silently evicting",
    ),
]

ROWS += [
    Mutation(
        "signing: report success when a previous signature is broken",
        "src/lib/signing.ts",
        'signed.signatures.every(s => s.integrity?.verdict === "intact")',
        'signed.signatures.some(s => s.integrity?.verdict === "intact")',
        "uses success only for a new signature read back intact",
    ),
]
