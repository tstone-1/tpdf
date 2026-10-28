"""One part of the frontend mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# Pages of another file: where each one is drawn, read, searched and linked
# from. Every one of these fails the same way to the eye --- a correct-looking
# picture, text or match taken from the wrong document --- so each is aimed at
# the test that can tell two files apart rather than two page numbers.
ROWS += [
    Mutation(
        # Draw an imported page nobody joined to its handle from the opened
        # document instead. Page n of the wrong file, looking entirely right.
        "pages: draw an unjoined imported page from the opened document",
        "src/lib/pages.ts",
        "    return view.from === undefined\n      ? undefined\n      : { doc: view.from, page: filePage(source.imported.page) };",
        "    return view.from === undefined\n      ? { doc, page: filePage(source.imported.page) }\n      : { doc: view.from, page: filePage(source.imported.page) };",
        "answers no address for an imported page nobody joined to its handle",
    ),
    Mutation(
        # Ask the opened document for every imported page's pixels.
        "pages: address an imported page by the opened document's handle",
        "src/lib/pages.ts",
        "      : { doc: view.from, page: filePage(source.imported.page) };",
        "      : { doc, page: filePage(source.imported.page) };",
        "names the opened document for its own pages and the other file's handle for one of its",
    ),
    Mutation(
        # Never write the handle on. Every imported page then has no address,
        # which is a document with holes in it rather than a wrong picture.
        "pages: leave the imported pages without their handle",
        "src/lib/pages.ts",
        "    return from === undefined ? view : { ...view, from };",
        "    return view;",
        "names the opened document for its own pages and the other file's handle for one of its",
    ),
    Mutation(
        # Resolve a destination in another file to whichever slot shows that
        # page number of *any* file.
        "pages: find an imported page by its number alone",
        "src/lib/pages.ts",
        "        view?.from === doc &&\n        \"imported\" in view.source &&",
        "        \"imported\" in view.source &&",
        "finds the slot showing a page of the other file, and only of that file",
    ),
    Mutation(
        # Leave a destination the other file names as its own page number,
        # which the viewer reads as a slot of this document.
        "pages: keep another file's destination to a page that was not imported",
        "src/lib/pages.ts",
        "  const slot = pages.slotOfImported(doc, target.page);\n  if (slot === undefined) return { kind: \"broken\" };",
        "  const slot = pages.slotOfImported(doc, target.page);\n  if (slot === undefined) return target;",
        "puts a link on the slot showing its page, and a destination on the slot showing its target",
    ),
    Mutation(
        # Place the other file's web link without the handle its token was
        # numbered by. The target still looks like a web link and still opens
        # -- through the opened document's list, which is somebody else's.
        "pages: drop the handle from a web link found in another file",
        "src/lib/pages.ts",
        "  if (target.kind === \"web\") return { ...target, doc };",
        "  if (target.kind === \"web\") return target;",
        "follows a web address found in the other file through that file's handle",
    ),
    Mutation(
        # Keep the other file's link ids, which the opened file's scan also
        # numbered from zero --- two links, one id, and a click follows the other.
        "pages: keep the other file's link ids",
        "src/lib/pages.ts",
        "        id: next++,",
        "        id: link.id,",
        "numbers the other file's links on from the opened file's",
    ),
    Mutation(
        # Adopt a reply without writing the handles on. The map and the viewer
        # then hold imported pages with nowhere to draw them from.
        "edits: adopt a reply without joining its handles",
        "src/lib/edits.ts",
        "      pages: withSources(state.pages, state.sources),",
        "      pages: state.pages,",
        "imports the named pages behind the page in a slot, and joins the handle the reply names",
    ),
    Mutation(
        # A stale press becomes an import at the front, `insertPage`'s trap in
        # the new command.
        "edits: turn a stale slot into an import at the front",
        "src/lib/edits.ts",
        "  async importPages(after: number | null, pending: number, pages: number[]): Promise<EditState> {\n    const anchor =\n      after === null ? null : (this.current.pages[after]?.id ?? undefined);",
        "  async importPages(after: number | null, pending: number, pages: number[]): Promise<EditState> {\n    const anchor = after === null ? null : (this.current.pages[after]?.id ?? null);",
        "releases the file rather than importing for a slot the model has never mentioned",
    ),
    Mutation(
        # Send nothing for a stale slot, as `insertPage` does --- which leaves
        # the file open for an answer that has already been given.
        "edits: leave the file open when the slot has gone",
        "src/lib/edits.ts",
        "      await call(\"page_import_cancel\", { doc: this.doc, pending });\n      return this.current;",
        "      return this.current;",
        "releases the file rather than importing for a slot the model has never mentioned",
    ),
    Mutation(
        # Ask the opened document for the imported page's tiles.
        "scroller: ask the opened document for an imported page's tile",
        "src/lib/scroller.ts",
        "    void fetchTile({\n      rid,\n      doc: source.doc,\n      page: source.page,\n      scale: this.opts.zoom * this.opts.dpr,",
        "    void fetchTile({\n      rid,\n      doc: this.opts.doc,\n      page: source.page,\n      scale: this.opts.zoom * this.opts.dpr,",
        "asks the other file's handle for a page inserted from it",
    ),
    Mutation(
        # The second request path, which the test above cannot see through the
        # first one's correct requests unless the page number is unique.
        "scroller: ask the opened document for an imported page's placeholder",
        "src/lib/scroller.ts",
        "    void fetchTile({\n      rid,\n      doc: source.doc,\n      page: source.page,\n      scale,",
        "    void fetchTile({\n      rid,\n      doc: this.opts.doc,\n      page: source.page,\n      scale,",
        "asks the other file's handle for a page inserted from it",
    ),
    Mutation(
        "thumbnails: ask the opened document for an imported row",
        "src/lib/thumbnails.ts",
        "      doc: source.doc,\n      page: source.page,",
        "      doc: this.opts.doc,\n      page: source.page,",
        "asks the other file's handle for a row inserted from it",
    ),
    Mutation(
        # One run across the join between two files, asked of the first file's
        # handle for pages of both.
        "search: run across a change of document",
        "src/lib/search.ts",
        "    if (previous !== undefined && source.doc !== previous.doc) break;",
        "    if (false) break;",
        "stops where the pages start coming from another file",
    ),
    Mutation(
        "search: ask the opened document about an imported page",
        "src/lib/search.ts",
        "        result = await this.ask({\n          doc: source.doc,",
        "        result = await this.ask({\n          doc: this.doc,",
        "asks the file each page is drawn from, and finds a match on the imported one",
    ),
    Mutation(
        # Read every page's text out of the opened document's cache.
        "viewer: read an imported page's text from the opened document",
        "src/lib/viewer.ts",
        "    if (doc === this.opts.doc) return this.text;",
        "    return this.text;",
        "selects the text of a page inserted from another file, asked of that file",
    ),
    Mutation(
        "viewer: measure an imported page's crop against the opened document",
        "src/lib/viewer.ts",
        "      const at = await pageGeometry(source.doc, source.page, want).catch(",
        "      const at = await pageGeometry(this.opts.doc, source.page, want).catch(",
        "measures a cropped page inserted from another file against that file",
    ),
    Mutation(
        # Forget which files were asked about, so every edit scans them again.
        "importedlinks: ask about the same file again",
        "src/lib/importedlinks.ts",
        "    for (const doc of fresh) this.asked.add(doc);",
        "",
        "asks about each other file once, in the order its pages appear",
    ),
]

# --- Choosing an inserted page's size (2026-08-30) --------------------------
#
# The table is five sizes and one function, and everything that can be wrong
# with it is a number: a pair transposed, a size off its series, a US size that
# has drifted off a whole point. So the mutations below are each one edited
# constant, and each is aimed at the one assertion that can see it --- which is
# also the record of which assertion is doing which job.
ROWS += [
    Mutation(
        # Transpose the pair the model is handed. Every size is still in the
        # table and every command still inserts a page; the pages come out
        # landscape. This is the mistake `dimensionsOf` exists to keep out of
        # `App.svelte`, where no unit test reaches it.
        "pagesizes: answer height first, then width",
        "src/lib/pagesizes.ts",
        "  return [size.width, size.height];",
        "  return [size.height, size.width];",
        "answers width first, then height",
    ),
    Mutation(
        # Turn one entry on its side in the table itself, which `dimensionsOf`
        # then reports faithfully --- so the mutation above cannot see it.
        "pagesizes: give A4 a landscape shape",
        "src/lib/pagesizes.ts",
        '  a4: { title: "A4", width: 595.276, height: 841.89 },',
        '  a4: { title: "A4", width: 841.89, height: 595.276 },',
        "is portrait throughout",
    ),
    Mutation(
        # Take one A size off the ratio the series is defined by, while leaving
        # it portrait, non-integral and correctly related to its neighbour. Only
        # the ratio assertion can see this one.
        "pagesizes: give A3 a shape the A series does not have",
        "src/lib/pagesizes.ts",
        '  a3: { title: "A3", width: 841.89, height: 1190.551 },',
        '  a3: { title: "A3", width: 841.89, height: 1250.5 },',
        "gives every A size the ratio the series is defined by",
    ),
    Mutation(
        # Break the chain without breaking the shape: 300 x 424.264 is root two
        # to four figures, portrait, and on no whole point --- so the ratio, the
        # portrait and the whole-point assertions all pass, and A5 is no longer
        # half of A4.
        "pagesizes: make A5 the right shape and the wrong size",
        "src/lib/pagesizes.ts",
        '  a5: { title: "A5", width: 419.528, height: 595.276 },',
        '  a5: { title: "A5", width: 300, height: 424.264 },',
        "halves each A size into the next one down",
    ),
    Mutation(
        # Drift a US size off the whole point its definition in inches puts it
        # on. Half a point is invisible on screen and in every other assertion
        # here: still portrait, still nobody's neighbour in a series.
        "pagesizes: put US Letter half a point off",
        "src/lib/pagesizes.ts",
        '  letter: { title: "US Letter", width: 612, height: 792 },',
        '  letter: { title: "US Letter", width: 612.5, height: 792 },',
        "puts the US sizes on whole points",
    ),
    Mutation(
        # The `map` mistake the colours and the stamps each have a mutation for:
        # five commands built in one loop, all passing the same name. Every
        # command is registered, every one runs, and a reader gets five ways to
        # insert A4.
        "appcommands: give every sized insert the same size",
        "src/lib/appcommands.ts",
        "      run: () => actions.insertSizedPage(name),",
        '      run: () => actions.insertSizedPage("a4"),',
        "insert a page of the size the command names",
    ),
    Mutation(
        # Put the size after the noun instead of inside the phrase. Every title
        # is still unique and still readable, and "Insert blank page" becomes a
        # strict prefix of all five --- which `commands.ts` decides by
        # registration order, and `docs/TRAPS.md` records shipping once.
        "appcommands: name the sized inserts so one title is a prefix of another",
        "src/lib/appcommands.ts",
        "    insertSizedPage: (size: string) => `Insert blank ${size} page`,",
        "    insertSizedPage: (size: string) => `Insert blank page ${size}`,",
        "give the sized inserts titles that are nobody's prefix",
    ),
]

# The nib a reader picks: the table, the seam the viewer holds it on, and the
# commands built from it.
ROWS += [
    Mutation(
        # Two entries with one width. Both are inside the range, both are
        # plausible pen weights, and the list is still sorted --- so only the
        # distinctness half can see it, and a reader would have two commands
        # they could not tell apart by their effect.
        "marknibs: give two nibs the same width",
        "src/lib/marknibs.ts",
        '  { id: "broad", name: TEXT().broad, pt: 5 },',
        '  { id: "broad", name: TEXT().broad, pt: 1 },',
        "goes from thinnest to thickest, with no two the same",
    ),
    Mutation(
        # Past the ceiling `docmodel.rs` clamps to. It stays sorted, stays
        # distinct and stays a number --- the door in `edits.rs` silently brings
        # it back to 24, so the reader picks "marker" and gets something else
        # with nothing on either side saying so.
        "marknibs: offer a nib past what the backend accepts",
        "src/lib/marknibs.ts",
        '  { id: "marker", name: TEXT().marker, pt: 10 },',
        '  { id: "marker", name: TEXT().marker, pt: 40 },',
        "offers every one inside the range the backend clamps to",
    ),
    Mutation(
        # The default a shade off, which is the whole argument for holding it by
        # reference: 2.6 is a pen weight, is between its neighbours, and means a
        # reader picking "medium" to match their existing drawings gets ink that
        # does not match them.
        "marknibs: write the default nib as a number instead of the constant",
        "src/lib/marknibs.ts",
        '  { id: "medium", name: TEXT().medium, pt: INK_WIDTH },',
        '  { id: "medium", name: TEXT().medium, pt: 2.6 },',
        "holds the default by reference, so it cannot be a shade off",
    ),
    Mutation(
        # No choice must mean the default. Answering the thinnest nib instead is
        # the shape that ships: every drawing made without picking anything comes
        # out hairline, and `App.svelte` is reached by no unit test, so the seam
        # is the only place this can be caught.
        "marknibs: fall back to the first nib rather than to the default",
        "src/lib/marknibs.ts",
        "  return chosen?.pt ?? INK_WIDTH;",
        "  return chosen?.pt ?? NIBS[0]!.pt;",
        "falls back to the default width when nothing is chosen",
    ),
    Mutation(
        # The seam the feature rests on: the preview is painted from the armed
        # nib and the mark carries it out here. Reporting the constant instead
        # gives a reader a broad preview and a thin saved line, with nothing on
        # screen saying which is right until the file is reopened.
        "viewer: report the default nib rather than the armed one",
        "src/lib/viewer.ts",
        "          { quads: this.fileRectOn(live.slot, quad), strokes: [], width: this.nib,",
        "          { quads: this.fileRectOn(live.slot, quad), strokes: [], width: INK_WIDTH,",
        "carries the nib that was set, and the default until one is",
    ),
    Mutation(
        # Spend the nib with the tool, which is what `drawStamp` beside it does
        # and is exactly the distinction the field's comment draws. A reader who
        # picks "broad" then gets one broad drawing and silently goes back to the
        # default for every one after it.
        "viewer: spend the nib when the armed tool is spent",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        '        if (this.tool.kind === "draw") this.tool = NO_TOOL;\n'
        "        this.showCursor();",
        '        if (this.tool.kind === "draw") this.tool = NO_TOOL;\n'
        "        this.nib = INK_WIDTH;\n"
        "        this.showCursor();",
        "keeps the nib across arming, because a pen outlives a drag",
    ),
    Mutation(
        # Four commands out of one `map`, all reaching one action with an
        # argument: the copy-and-paste this table exists for. Every command still
        # runs and every one sets the default.
        "appcommands: give every nib command the same nib",
        "src/lib/appcommands.ts",
        "      run: () => actions.setNib(entry.id),",
        '      run: () => actions.setNib("medium"),',
        "every nib command asks for its own nib",
    ),
]

# Deleting a comment the file came with: the join that drops the row, the popup
# that sends it, and the guard that offers the command.
ROWS += [
    Mutation(
        # Drop nothing. Every deleted comment goes on being listed by the panel
        # and openable in the popup until the file is saved and reopened -- the
        # scan is a reading of the file on disk and cannot know, so this join is
        # the only place a deletion can show.
        "pages: go on listing a comment the reader deleted",
        "src/lib/pages.ts",
        "      continue;\n    }\n    const edit = comment.object",
        "    }\n    const edit = comment.object",
        "drops a comment the reader deleted, and no other",
    ),
    Mutation(
        # Match on the object number alone. Two comments differing only in the
        # generation are then one comment to this join, so deleting either drops
        # both -- which is the rewrite join's own trap, on the other list.
        "pages: match a deletion on the object number and not the generation",
        "src/lib/pages.ts",
        "          gone.object[0] === comment.object?.[0] &&\n          gone.object[1] === comment.object?.[1],",
        "          gone.object[0] === comment.object?.[0],",
        "matches a deletion on the generation too",
    ),
    Mutation(
        # Treat a missing object as a wildcard, which makes one deletion drop
        # every direct-dictionary comment in the document.
        #
        # ⚠ **Both halves of the comparison, and it took two survivors to get
        # here.** The first try deleted an outer `comment.object &&` guard and
        # SURVIVED, because the chaining below already refused; the second made
        # one half of the chaining a wildcard and SURVIVED too, because the outer
        # guard refused first. Two mechanisms for one rule, each untestable while
        # the other stands. The guard is gone now, and this mutates the whole
        # predicate: a wildcard in one half alone is still refused by the other,
        # so a mutation aimed at either half would survive for a third time.
        "pages: let a comment with no object match any deletion",
        "src/lib/pages.ts",
        "          gone.object[0] === comment.object?.[0] &&\n          gone.object[1] === comment.object?.[1],",
        "          gone.object[0] === (comment.object?.[0] ?? gone.object[0]) &&\n          gone.object[1] === (comment.object?.[1] ?? gone.object[1]),",
        "leaves a comment with no object of its own out of the deletion join",
    ),
    Mutation(
        # Delete without closing. The state reply then rebuilds the panel from a
        # list this comment is no longer in while the popup goes on showing it,
        # which reads as the deletion not having happened.
        "commentpopup: leave the popup open after deleting the comment it shows",
        "src/lib/commentpopup.ts",
        "    this.hide();\n    this.opts.onDiscard?.(comment);",
        "    this.opts.onDiscard?.(comment);",
        "sends the comment on show to be deleted, and closes",
    ),
    Mutation(
        # Send a deletion for a comment that has no object to name, which the
        # backend refuses -- so the reader gets an error for a command the popup
        # should not have offered.
        "commentpopup: send a deletion for a comment with no object",
        "src/lib/commentpopup.ts",
        "  remove(): void {\n    if (!this.deletable) return;",
        "  remove(): void {",
        "deletes nothing for a comment the file wrote without an object",
    ),
    Mutation(
        # Read the Edit guard for the Delete command. The two agree today, which
        # is exactly why one flag would not catch this and the harness gives each
        # its own.
        "appcommands: guard deleting a comment on whether it can be edited",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canDeleteComment(),",
        "      enabled: () => withDocument() && actions.canEditComment(),",
        "offers deleting a comment exactly when the popup says it can be deleted",
    ),
    Mutation(
        # Drop the document half of the conjunction, which is the operand a test
        # varying only the popup's answer cannot see.
        "appcommands: offer deleting a comment with no document open",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canDeleteComment(),",
        "      enabled: () => actions.canDeleteComment(),",
        "withholds deleting a comment with no document, however deletable",
    ),
]

ROWS += [
    Mutation(
        # Mint the gesture number at the press rather than at the release, which
        # is the placement that looks more natural and spends a number on every
        # cancelled sweep. Harmless for grouping -- neighbours still differ --
        # and it is the pin on where the mint happens that goes.
        "viewer: report the gesture the sweep before this one had",
        "src/lib/viewer.ts",
        "          const sweep = ++this.sweeps;",
        "          const sweep = this.sweeps;",
        "spends no number on a sweep the reader cancelled",
    ),
    Mutation(
        # Give every release the same number. One sweep still groups, which is
        # why the single-gesture check cannot see this: what goes is the reader
        # who erased, stopped and erased again getting their second gesture
        # back along with their first.
        "viewer: give every sweep one gesture",
        "src/lib/viewer.ts",
        "          const sweep = ++this.sweeps;",
        "          const sweep = 1;",
        "gives two sweeps two numbers",
    ),
    Mutation(
        # Send the wire's no-gesture from the eraser, which is what every call
        # sent until 2026-08-30. The reply is identical and only a press of undo
        # can tell the difference.
        "edits: send no gesture with an erasure",
        "src/lib/edits.ts",
        "  async erase(mark: number, remove: number[], sweep = 0): Promise<EditState> {",
        "  async erase(mark: number, remove: number[], _sweep = 0): Promise<EditState> {\n    const sweep = 0;",
        "carries the gesture a sweep gave it, on both of its commands",
    ),
    Mutation(
        # The same, for the half of a sweep that takes a mark whole. A gesture
        # that clipped a drawing and took a highlight then goes back in two
        # presses, which is exactly half fixed.
        "edits: send no gesture when a sweep takes a mark whole",
        "src/lib/edits.ts",
        "  async unmark(mark: number, sweep = 0): Promise<EditState> {",
        "  async unmark(mark: number, sweep = 0): Promise<EditState> {\n    sweep = 0;",
        "carries the gesture a sweep gave it, on both of its commands",
    ),
]

#: The guards added on 2026-09-06, when the frame path and four continuations
#: that outlive their subject were gone over. Every one of them is a reply, an
#: index or a cache that can be about a document the reader has already changed.
ROWS += [
    Mutation(
        # Keep a page's extraction even though its crop moved while the request
        # was in flight. Character boxes are measured from the displayed page's
        # corner, so the reply is not stale, it is in another space --- and the
        # caret then lands a crop's width from the glyph under the pointer.
        "text: store an extraction issued under the crop the page has left",
        "src/lib/text.ts",
        "        if (stale()) return null;\n        this.remember(page, text);",
        "        this.remember(page, text);",
        "throws away a reply for the crop that was in force when it was sent",
    ),
    Mutation(
        # Delete the pending entry whichever request is settling. The abandoned
        # one settles after its replacement was recorded, so this takes the live
        # request out of the map and every caller after it issues a duplicate.
        "text: drop the pending entry without checking whose it is",
        "src/lib/text.ts",
        "        if (this.pending.get(page) === request) this.pending.delete(page);",
        "        this.pending.delete(page);",
        "leaves the newer request pending when the older one settles",
    ),
    Mutation(
        # Select the next region to read by page rather than by region. One
        # extraction does answer every region on a page, which is why the walk
        # answers them together --- and a second region drawn on a page already
        # read is then never selected, so its row says "reading" for ever.
        "redactlist: pick the next region by page rather than by region",
        "src/lib/redactlist.ts",
        "  return regions.find((region) => !words.has(region.id));",
        "  return regions.find(\n"
        "    (region) =>\n"
        "      !regions.some((other) => other.page === region.page && words.has(other.id)),\n"
        "  );",
        "picks the second region on a page whose text has already been read",
    ),
    Mutation(
        # Write a crop's geometry into the slot the page was in when the round
        # trip went out. A reorder in the meantime records the cropped page's
        # dimensions against whatever moved into that slot.
        "viewer: adopt a crop into a page order that has been replaced",
        "src/lib/viewer.ts",
        "      if (this.pages !== pages) return;\n      if (!at) continue;",
        "      if (!at) continue;",
        "does not write the geometry into the slot the page has left",
    ),
    Mutation(
        # Record a withdrawn request's failure against the row. `failed` is
        # cleared only by a change of orientation or order, so the row is never
        # asked for again and stays blank for the life of the document.
        "thumbnails: blame the row for a request the strip itself withdrew",
        "src/lib/thumbnails.ts",
        "        if (generation !== this.generation) {\n"
        "          this.pump();\n"
        "          return;\n"
        "        }\n",
        "",
        "does not give up on a row because a withdrawn request came back an error",
    ),
    Mutation(
        # Keep the mark index across a change of page order. It is keyed by
        # slot, and a deletion renumbers every slot after the gap, so a mark is
        # drawn and hit-tested where it no longer is.
        "viewer: keep the mark index when the page order changes",
        "src/lib/viewer.ts",
        "    // The marks have not changed and every one of them may have moved: the\n"
        "    // index is by slot, and a deletion renumbers every slot after the gap.\n"
        "    this.markIndex = null;\n",
        "",
        "finds a mark under the pointer after its page has moved",
    ),
    Mutation(
        # The same index across a change of marks, which is every undo.
        "viewer: keep the mark index when the marks change",
        "src/lib/viewer.ts",
        "    this.marks = marks;\n    this.markIndex = null;",
        "    this.marks = marks;",
        "forgets a mark the model has taken away",
    ),
    Mutation(
        # Place every mark in the document each frame, then test visibility.
        # Placing a mark is what says which page it is on, so this pays for the
        # whole document to draw what is in front of the reader.
        "viewer: place every mark in the document on every frame",
        "src/lib/viewer.ts",
        "    for (const { slot, mark } of this.marksAcross(visible)) {",
        "    for (const { slot, mark } of this.marksAcross(\n"
        "      this.marksIndexed().bySlot.keys().toArray(),\n"
        "    )) {",
        "places only the marks on the slots the frame says are on screen",
    ),
    Mutation(
        # Measure the element instead of reading the cached size. The tick has
        # already written styles by then, so every ask flushes the layout.
        "viewer: measure the surface on every frame",
        "src/lib/viewer.ts",
        "    const known = this.viewport;\n"
        "    if (known) return known;\n"
        "    const measured = this.measureViewport();\n"
        "    this.viewport = measured;\n"
        "    return measured;",
        "    return this.measureViewport();",
        "does not measure the surface while drawing a frame",
    ),
    Mutation(
        # Ask the scroller again rather than sharing the frame's own reading.
        "viewer: read the visible pages again inside the frame",
        "src/lib/viewer.ts",
        "    this.paintOverlay(visible);",
        "    this.paintOverlay(this.scroller.visiblePages());",
        "asks the scroller for the visible pages twice a frame, not a dozen times",
    ),
    Mutation(
        # Lay the document out once per learnt size again. Every pass but the
        # last is undone before the frame ends.
        "scroller: lay out once per learnt size rather than once per batch",
        "src/lib/scroller.ts",
        "      if (this.recordSize(page, size)) learnt = true;\n"
        "    }\n"
        "    return learnt ? this.applySizes() : false;",
        "      if (this.notePageSize(page, size)) learnt = true;\n"
        "    }\n"
        "    return learnt;",
        "lays the document out once for a screenful of learnt sizes",
    ),
    Mutation(
        # Ask for sharp tiles at every scale a pinch passes through. What
        # reaches the renderer is a screenful a frame, none of it ever drawn.
        "scroller: ask for tiles while the zoom is still moving",
        "src/lib/scroller.ts",
        "      this.requestPlaceholder(page, now);\n      if (settling) continue;",
        "      this.requestPlaceholder(page, now);",
        "asks for no sharp tile while the zoom is still moving",
    ),
    Mutation(
        # Hold the placeholders back too, which leaves the page grey for the
        # length of the gesture rather than stretching what is already drawn.
        "scroller: hold back placeholders during the settle as well",
        "src/lib/scroller.ts",
        "      this.requestPlaceholder(page, now);\n      if (settling) continue;",
        "      if (settling) continue;\n      this.requestPlaceholder(page, now);",
        "still asks for placeholders during the settle",
    ),
    Mutation(
        # Sort the outline's destinations by document order rather than by
        # where they land. The reached test is then not monotone along the
        # list, so the search is not searching a sorted list at all.
        "outline: order the destinations by the tree rather than by the page",
        "src/lib/outline.ts",
        "  reached.sort((a, b) => a.page - b.page || a.top - b.top || a.at - b.at);",
        "  reached.sort((a, b) => a.page - b.page || a.at - b.at);",
        "orders two entries on one page by where they land, not by where they are listed",
    ),
    Mutation(
        # Build the whole selection to ask whether it is empty, which is what a
        # menu guard on the frame loop was doing.
        "selection: build every page to answer whether anything is selected",
        "src/lib/selection.ts",
        "  hasText(look: TextLookup): boolean {\n    const { start, end } = this.ordered;",
        "  hasText(look: TextLookup): boolean {\n"
        "    return this.text(look) !== \"\";\n"
        "    const { start, end } = this.ordered;",
        "stops at the first page that contributes",
    ),
]

# Web links, added 2026-09-07 with the feature. Three of the five are about the
# confirmation being real -- the dialog is the whole of what stands between a
# click and a request to a host a stranger chose -- and two are the branch
# *order* in the viewer and the sidebar, where a web target that falls through
# to the navigable test is reported to a reader as a link that goes nowhere.
ROWS += [
    Mutation(
        # Open on Enter, which is what an ordinary dialog does and is exactly
        # the reflex this one exists to interrupt.
        "weblink: let Enter open the address instead of cancelling",
        "src/lib/weblinkdialog.ts",
        "        event.preventDefault();\n"
        "        event.stopPropagation();\n"
        "        this.settle(false);\n"
        "      }\n"
        "    });\n"
        "  }\n"
        "\n"
        "  /** Whether it is on screen. */",
        "        event.preventDefault();\n"
        "        event.stopPropagation();\n"
        "        this.settle(true);\n"
        "      }\n"
        "    });\n"
        "  }\n"
        "\n"
        "  /** Whether it is on screen. */",
        "cancels on Enter rather than opening",
    ),
    Mutation(
        # Focus the affirmative button. Every assertion about what the dialog
        # *says* still passes; what changes is that a reader's next keystroke
        # opens the link.
        "weblink: focus Open rather than Cancel",
        "src/lib/weblinkdialog.ts",
        "    // Cancel, not Open. See the header.\n    this.cancel.focus();",
        "    this.panel.children[this.panel.children.length - 1]?.focus?.();",
        "focuses Cancel, not Open",
    ),
    Mutation(
        # Open first and ask afterwards. The dialog still appears, so a person
        # watching sees the feature working -- and the address has already been
        # handed to the operating system by the time they read it.
        "weblink: open before the reader has answered",
        "src/lib/weblinkdialog.ts",
        "  const wanted = await deps.ask({ host: target.host, rest: target.rest });\n"
        "  if (!wanted) return false;",
        "  const wanted = true;\n"
        "  void deps.ask({ host: target.host, rest: target.rest });",
        "does not open when the reader says no",
    ),
    Mutation(
        # Test the web arm after the refusal arm. A web link then reports itself
        # to the reader as a link that goes nowhere, which is the failure the
        # ordering was written for.
        "viewer: check for a web link after the refusal branch, not before",
        "src/lib/viewer.ts",
        "    if (link.target.kind === \"web\") {\n"
        "      this.opts.onWebLink?.(link.target);\n"
        "      return;\n"
        "    }\n"
        "\n"
        "    if (link.target.kind !== \"page\") {",
        "    if (link.target.kind !== \"page\") {",
        "reports a web link rather than jumping or refusing",
    ),
    Mutation(
        # Grey the row and swallow its click, which is what `isNavigable` alone
        # does: it answers "is a page in this document", and a web target is
        # not one.
        "sidebar: treat a web outline row as one that goes nowhere",
        "src/lib/sidebar.ts",
        "    if (row.target.kind === \"web\") {\n"
        "      this.opts.onWebLink(row.target);\n"
        "      return;\n"
        "    }\n",
        "",
        "reports an outline web link instead of navigating, and does not grey it",
    ),
]

ROWS += [
    # Right-clicking a pending redaction, 2026-09-07. Four mutations, each
    # proved to redden the test named for it before being written down.
    Mutation(
        # Fall through to the mark and selection rule, which is what the
        # function did before regions were hit-tested at all --- and is exactly
        # the defect reported: a right-click on a region a reader had just
        # dragged offered Copy, the marks and Find, and no way off the region.
        "contextmenu: ignore a region under the pointer",
        "src/lib/contextmenu.ts",
        "  if (redactionUnderPointer !== null) return REDACTION_MENU;\n",
        "",
        "offers the region's menu when the pointer is on a pending redaction",
    ),
    Mutation(
        # Answer nothing, always. The guard reads as an emptiness check and the
        # comparison is what makes it one, so this is the shape a typo takes ---
        # and with it every right-click on a region gets the selection menu
        # while the region list, the painting and the panel all stay correct.
        "viewer: answer no region for every point",
        "src/lib/viewer.ts",
        "    if (this.redactions.length === 0) return null;",
        "    if (this.redactions.length >= 0) return null;",
        "finds the region under the point the drag covered, and nothing outside it",
    ),
    Mutation(
        # Clear the pick on every state rather than only when the model has
        # stopped listing the region. `setRedactions` runs after every edit, so
        # this takes the pick off a region the reader has just right-clicked
        # whenever anything else lands --- and the menu is already open.
        "viewer: clear the picked region on every edit",
        "src/lib/viewer.ts",
        "    if (this.picked !== null && !regions.some((region) => region.id === this.picked)) {\n"
        "      this.picked = null;\n"
        "    }\n",
        "    this.picked = null;\n",
        "holds the picked region until it is cleared or the model drops it",
    ),
    Mutation(
        # Never clear it. The pick then outlives the region it names, so
        # `edit.removeRedaction` stays offered and acts on an id the model no
        # longer has --- a command that is live and does nothing.
        "viewer: keep a pick whose region the model has dropped",
        "src/lib/viewer.ts",
        "    if (this.picked !== null && !regions.some((region) => region.id === this.picked)) {\n"
        "      this.picked = null;\n"
        "    }\n",
        "",
        "holds the picked region until it is cleared or the model drops it",
    ),
]

ROWS += [
    Mutation(
        "tab labels: any saved number is a size",
        "src/lib/tablabels.ts",
        "      if (TAB_LABEL_SIZES.includes(saved)) this.#px = saved;",
        "      if (saved > 0) this.#px = saved;",
        "reads back a saved size and ignores one it does not offer",
    ),
    Mutation(
        "tab labels: a step is not remembered",
        "src/lib/tablabels.ts",
        "    try { this.storage().setItem(KEY, String(next)); } catch { /* Kept for this session. */ }",
        "",
        "starts at the default and steps through the offered sizes, stopping at both ends",
    ),
    Mutation(
        "tab labels: stepping runs off the ends",
        "src/lib/tablabels.ts",
        "      : TAB_LABEL_SIZES[Math.min(TAB_LABEL_SIZES.length - 1, Math.max(0, at + direction))]",
        "      : TAB_LABEL_SIZES[at + direction]",
        "starts at the default and steps through the offered sizes, stopping at both ends",
    ),
    Mutation(
        "tab labels: a failed write loses the new size",
        "src/lib/tablabels.ts",
        "    this.#px = next;\n    try { this.storage().setItem(KEY, String(next)); } catch { /* Kept for this session. */ }",
        "    try { this.storage().setItem(KEY, String(next)); this.#px = next; } catch { /* Kept for this session. */ }",
        "keeps working when storage throws",
    ),
]

ROWS += [
    Mutation(
        "tabs: move a saved document to the end instead of replacing its handle",
        "src/lib/documenttabs.ts",
        "    else this.entries[index] = tab;",
        "    else this.entries.push(tab);",
        "replaces a saved handle in its original position",
    ),
    Mutation(
        "tabs: closing a background document changes the active tab",
        "src/lib/documenttabs.ts",
        "    if (this.active === id)",
        "    if (true)",
        "closes a background tab without moving focus and chooses a neighbour for the active tab",
    ),
    Mutation(
        "tabs: allow a transition before a save has reopened its document",
        "src/lib/documenttabs.ts",
        "    while (this.busy) await this.pending.catch(() => {});",
        "    return;",
        "holds a transition until a save and its reopen finish",
    ),
]

ROWS += [
    Mutation(
        "visual signature: stretch outside the placement rectangle",
        "src/lib/signature.ts",
        "Math.min((rect.right - rect.left) / image.width, (rect.bottom - rect.top) / image.height)",
        "Math.max((rect.right - rect.left) / image.width, (rect.bottom - rect.top) / image.height)",
        "fits a wide signature without stretching or escaping the dragged rectangle",
    ),
    Mutation(
        "visual signature: ignore page turns",
        "src/lib/signature.ts",
        "turn < ((turns % 4) + 4) % 4",
        "turn < 0",
        "rotates pixel orientation with the page and reverses a placement turn",
    ),
]

ROWS += [
    Mutation("boxed edit: ignore layout-only changes", "src/lib/textedit.ts",
        'return this.input.value !== this.accepted || JSON.stringify(this.controls.read()) !== this.acceptedLayout;',
        'return this.input.value !== this.accepted;',
        "applies layout-only changes and wraps on Ctrl+Enter while Enter adds a line"),
    Mutation("boxed edit: display stale preview", "src/lib/textedit.ts",
        'if (this.disposed || generation !== this.previewGeneration || !result.preview) return;',
        'if (this.disposed || !result.preview) return;',
        "debounces previews, discards stale replies and never writes a cancelled preview"),
]

ROWS += [
    Mutation("tab menu: discard clicked tab actions", "src/lib/contextmenu.ts",
        "const action = actions.find((candidate) => candidate.id === entry);",
        "const action = actions.find((candidate) => candidate.id === entry && false);",
        "runs the clicked tab's actions without changing or leaking into the registry"),
]

# Editing text on a page inserted from another file (26.9.16). Each of these
# leaves the page number right and the *document* wrong, which is the pair
# that collides only once a reader has inserted pages.
ROWS += [
    Mutation("inserted text: key a change by its page number alone", "src/lib/textedit.ts",
        'const key = (change: TextPage) => JSON.stringify([change.source ?? null, change.page]);',
        'const key = (change: TextPage) => JSON.stringify([change.page]);',
        "tells one file's page from another file's page of the same number"),
    Mutation("inserted text: report a change as the opened file's", "src/lib/textedit.ts",
        'return source === null ? { page: at } : { source, page: at }; });',
        'return { page: at }; });',
        "tells one file's page from another file's page of the same number"),
    Mutation("inserted text: match a pending change on the page number alone", "src/lib/textedit.ts",
        'return a.page === b.page && (a.source ?? -1) === (b.source ?? -1);',
        'return a.page === b.page;',
        "shows the replacement on its own file's page, not the opened file's of the same number"),
    Mutation("inserted text: find the slot by page number alone", "src/lib/pages.ts",
        'view.imported.source === source &&\n        view.imported.page === page',
        'view.imported.page === page',
        "finds the slot of another file's page by the model's id for that file"),
    Mutation("inserted text: look a change's slot up as the opened file's", "src/lib/viewer.ts",
        'at.source === undefined\n          ? this.pages.slotOf(at.page)\n          : this.pages.slotOfSource(at.source, at.page);',
        'this.pages.slotOf(at.page);',
        "re-extracts only the inserted page when its own text was edited"),
    Mutation("inserted text: keep the edited page's words in the cache", "src/lib/viewer.ts",
        'if (text) text.cache.invalidatePage(text.page);',
        'void text;',
        "re-extracts only the inserted page when its own text was edited"),
]

# Finishing an update from inside the application (26.9.17). The header used to
# read "Update ready --- restart to finish" over a button that was `disabled`, so
# the one state whose label asks for a step offered no way to take it. The step
# ends the process, which is why every one of these is about the question in
# front of it rather than about the relaunch itself.
ROWS += [
    Mutation("update finish: run a step the state does not allow", "src/lib/update.ts",
        '  if (state.kind !== wanted) return "withheld";',
        '',
        "refuses a restart before the update is applied"),
    Mutation("update finish: guard every step against the ready state", "src/lib/update.ts",
        '  const wanted = step === "install" ? "available" : "ready";',
        '  const wanted = "ready";',
        "refuses an install that is already applied, and one never found"),
    Mutation("update finish: ask about unsaved work even when nothing ends", "src/lib/update.ts",
        '  if (ends) {',
        '  if (ends || true) {',
        "asks nothing for a step that does not end the process"),
    Mutation("update finish: act although the reader said no", "src/lib/update.ts",
        '    if (prompt && !await effects.confirm(prompt)) return "cancelled";',
        '    if (prompt) await effects.confirm(prompt);',
        "leaves the update ready when the reader says no"),
    Mutation("update finish: swallow a failed relaunch", "src/lib/update.ts",
        '    effects.say(String(e));',
        '',
        "tells the reader when the step itself fails, rather than failing silently"),
    # Backwards is the dangerous direction: it takes the question away from the
    # Windows install, which ends the process, and puts it in front of the macOS
    # one, which discards nothing.
    Mutation("update platform: installing ends the process on macOS too", "src/lib/update.ts",
        '  return !mac;',
        '  return true;',
        "does not end on macOS, where the restart is a separate step"),
    Mutation("update prompt: ask although nothing is unsaved", "src/lib/update.ts",
        '  if (unsaved.length === 0) return null;',
        '',
        "asks nothing when nothing is unsaved"),
    Mutation("update prompt: one wording for both steps", "src/lib/update.ts",
        '  return step === "restart"\n    ? {',
        '  return true\n    ? {',
        "says which act is about to happen, and the two are not the same act"),
    Mutation("update prompt: count the one document instead of naming it", "src/lib/update.ts",
        "    unsavedOne: (name: string) => `unsaved changes to ${name}`,",
        "    unsavedOne: (name: string) => `unsaved changes in 1 open document`,",
        "names the one document, and counts several"),
    Mutation("update label: name the step rather than the action that takes it", "src/lib/update.ts",
        "      return TEXT().restartToFinish;",
        '      return "Update ready \u2014 restart to finish";',
        "names the version on offer, and the restart when one is waiting"),
    # The registry half. Both guards are one line each and neither is visible
    # from the other, which is what let the install stay live after it had run.
    Mutation("update command: offer the restart before anything is installed", "src/lib/appcommands.ts",
        '      enabled: () => actions.updateReady(),',
        '      enabled: () => true,',
        "withholds the restart until the update is actually applied"),
    Mutation("update command: keep promising a restart it may not perform", "src/lib/appcommands.ts",
        '    installUpdate: "Install update",',
        '    installUpdate: "Install update and restart",',
        "promises no restart from the command that may not perform one"),
    # The shell, where no test imports the file and a source assertion is the
    # only instrument there is.
    Mutation("update shell: read the platform alone, so a macOS restart asks nothing",
        "src/lib/update.ts",
        '  return step === "restart" || installEndsProcess(mac);',
        '  return installEndsProcess(mac);',
        "asks about unsaved work before a restart, whatever installing does here"),
    Mutation("update shell: a restart is all that ends the process", "src/lib/update.ts",
        '  return step === "restart" || installEndsProcess(mac);',
        '  return step === "restart";',
        "asks about unsaved work before a restart, whatever installing does here"),
    Mutation("update shell: ask the platform and not the step", "src/App.svelte",
        '    const ends = stepEndsProcess(step, isMac());',
        '    const ends = !isMac(); // stepEndsProcess(step, isMac());',
        "asks about unsaved work before a restart, whatever installing does here"),
    Mutation("update button: install again once the update is installed", "src/lib/update.ts",
        '  return state.kind === "ready" ? "restart" : "install";',
        '  return state.kind === "ready" ? "install" : "restart";',
        "leaves the ready state pressable, rather than labelling a disabled button"),
    Mutation("update button: relaunch around the guarded step", "src/App.svelte",
        '        onclick={() => void finishUpdateStep(stepFor(updateState))}>',
        '        onclick={() => void updates.install()}><!-- onclick={() => void finishUpdateStep(stepFor(updateState))} -->',
        "leaves the ready state pressable, rather than labelling a disabled button"),
    Mutation("update shell: end the process without waiting for the place", "src/App.svelte",
        '        await places.settled();',
        '',
        "waits for the reading position to land before ending the process"),
    Mutation("session settle: resolve before the write is answered", "src/lib/session.ts",
        '    return this.queue.then(() => undefined, () => undefined);',
        '    return Promise.resolve();',
        "waits for a flushed write to be answered, so a relaunch cannot outrun it"),
]

# The note box is dragged aside by its heading (2026-10-05): it opens beside its
# mark and can cover the next field. Each entry is one rule of `markpopup.ts`.
ROWS += [
    Mutation('note box: keep a drag for the next mark', "src/lib/markpopup.ts",
        '    if (was !== mark.id) this.moved = { x: 0, y: 0 };\n',
        '',
        'opens beside the next mark, and beside the same one after it was closed'),
    Mutation('note box: place the box where its mark is, drag or none', "src/lib/markpopup.ts",
        '    };\n    this.settle();\n  }\n',
        '    };\n  }\n',
        'is dragged aside by its heading, and goes on following its mark from there'),
    Mutation('note box: keep the part of a drag that left the window', "src/lib/markpopup.ts",
        '    this.moved = { x: left - this.placed.left, y: top - this.placed.top };\n',
        '',
        'stays inside the window however far it is dragged, and comes straight back'),
    Mutation('note box: let a drag take the box out of the window', "src/lib/markpopup.ts",
        '      Math.max(DRAG_MARGIN, Math.min(value, Math.max(DRAG_MARGIN, room - size - DRAG_MARGIN)));',
        '      value + 0 * (size + room);',
        'stays inside the window however far it is dragged, and comes straight back'),
    Mutation('note box: keep a drag the browser took away', "src/lib/markpopup.ts",
        '        if (committed) return;\n        this.moved = from.moved;',
        '        if (committed) return;',
        'goes back where the drag found it when the pointer is taken away'),
    Mutation('note box: start every drag from where the box is placed', "src/lib/markpopup.ts",
        '        from = { pointer: at, moved: this.moved };',
        '        from = { pointer: at, moved: { x: 0, y: 0 } };',
        'is dragged aside by its heading, and goes on following its mark from there'),
    Mutation('note box: a heading that drags nothing', "src/lib/markpopup.ts",
        '      drag.start(event);\n',
        '',
        'is dragged aside by its heading, and goes on following its mark from there'),
]

# One mark picked says that Shift adds another, and the line about the picked
# marks goes when it stops being true (2026-10-05). The owner pressed one field and
# then a second and nothing on screen named Shift.
ROWS += [
    Mutation('picked notice: say nothing with one picked', 'src/lib/arrange.ts',
        '  if (count === 1 && more) return PICK_ANOTHER;\n',
        '',
        'says that Shift adds another when one is picked and another is there'),
    Mutation('picked notice: offer Shift with nothing to add', 'src/lib/arrange.ts',
        '  if (count === 1 && more) return PICK_ANOTHER;',
        '  if (count === 1) return PICK_ANOTHER;',
        'takes its own line away when it stops being true, and leaves any other'),
    Mutation('picked notice: leave a line that stopped being true', 'src/lib/arrange.ts',
        "  return current !== null && PICKED_LINE.test(current) ? null : current;",
        '  return current;',
        'takes its own line away when it stops being true, and leaves any other'),
    Mutation('picked notice: take any line away', 'src/lib/arrange.ts',
        "  return current !== null && PICKED_LINE.test(current) ? null : current;",
        '  return null;',
        'takes its own line away when it stops being true, and leaves any other'),
    Mutation('picked notice: take away a line that only begins with a number', 'src/lib/arrange.ts',
        "const PICKED_LINE = new RegExp(`^\\\\d+ ${TEXT().picked}\\\\. `);",
        "const PICKED_LINE = new RegExp(`^\\\\d+ ${TEXT().picked}`);",
        'takes its own line away when it stops being true, and leaves any other'),
    Mutation('picked: count a picked mark as one still to pick', 'src/lib/viewer.ts',
        '          !this.arranging.has(other.id) && other.page === picked.page && isResizable(other.kind),',
        '          other.page === picked.page && isResizable(other.kind),',
        'says with each change whether the page has another mark to pick with those'),
    Mutation('picked: count a mark on another page as one to pick', 'src/lib/viewer.ts',
        '          !this.arranging.has(other.id) && other.page === picked.page && isResizable(other.kind),',
        '          !this.arranging.has(other.id) && isResizable(other.kind),',
        'does not count a mark on another page as one to pick with these'),
    Mutation('picked: count a highlight as one to pick', 'src/lib/viewer.ts',
        '          !this.arranging.has(other.id) && other.page === picked.page && isResizable(other.kind),',
        '          !this.arranging.has(other.id) && other.page === picked.page,',
        'says with each change whether the page has another mark to pick with those'),
]

# A long folder on the blank page keeps its end (2026-10-05). Cut by the
# page's layout it lost "page 5" with it, and cut from its start there the
# ellipsis was drawn over the text.
ROWS += [
    Mutation('start page: cut a folder that just fits', "src/lib/startpage.ts",
        '  if (folder.length <= FOLDER_CHARS) return folder;',
        '  if (folder.length < FOLDER_CHARS) return folder;',
        'is left alone up to the limit'),
    Mutation('start page: show a long folder whole', "src/lib/startpage.ts",
        '  if (folder.length <= FOLDER_CHARS) return folder;',
        '  if (folder.length <= FOLDER_CHARS * 99) return folder;',
        'keeps its end, from a separator, after an ellipsis'),
    Mutation('start page: cut a folder in the middle of a name', "src/lib/startpage.ts",
        '  return `…${at >= 0 && at <= 16 ? tail.slice(at) : tail}`;',
        '  return `…${tail}`;',
        'keeps its end, from a separator, after an ellipsis'),
    Mutation('start page: cut at a separator however far in', "src/lib/startpage.ts",
        '  return `…${at >= 0 && at <= 16 ? tail.slice(at) : tail}`;',
        '  return `…${at >= 0 ? tail.slice(at) : tail}`;',
        'cuts by characters where no separator is near'),
    Mutation('start page: cut a folder without saying so', "src/lib/startpage.ts",
        '  return `…${at >= 0 && at <= 16 ? tail.slice(at) : tail}`;',
        '  return `${at >= 0 && at <= 16 ? tail.slice(at) : tail}`;',
        'cuts by characters where no separator is near'),
    Mutation("start page: show a row's folder uncut", "src/lib/startpage.ts",
        '      folder: tailOf(shortenHome(folderOf(place.path), home)),',
        '      folder: shortenHome(folderOf(place.path), home),',
        'is what a row shows'),
]

# --- signing with a certificate the reader has (Phase 6 step 2, 2026-09-26) ---
# The command-line tool restates the application's verdict sentences in Rust
# (`src-tauri/src/words.rs`) and writes every case to
# `src-tauri/testdata/cli/wording.json`; `cliwording.test.ts` holds the two to
# each other. These change the application's side alone, which is the drift
# that test exists for: the tool would go on printing the old words.
ROWS += [
    # Re-aimed 2026-09-28: the revocation caveat left the trust row for a row
    # of its own, and the application saying that row differently is the drift.
    Mutation("cli wording: the application says none differently", "src/lib/integrity.ts",
        "          `does not fetch any, so a certificate its issuer has since withdrawn reads the ` +",
        "          `does not fetch any, so a certificate withdrawn since reads the ` +",
        "says each revocation answer as the properties dialog does"),
    Mutation("cli wording: the application says intact differently", "src/lib/integrity.ts",
        "          `intact — the signed bytes are unchanged and the signature checks out ` +",
        "          `intact — the signed bytes are unchanged and the signature verifies ` +",
        "says each integrity verdict as the properties dialog does"),
    Mutation("cli wording: the application closes a signing differently", "src/lib/signing.ts",
        "Read back after writing, the signature is intact.`,",
        "Read back, the signature is intact.`,",
        "closes a signing as the signing panel does"),
    # 2026-10-05: `tpdf verify` restates the dialog's Appended row
    # (`appendix_sentence` in `src-tauri/src/cli/verify.rs`).
    Mutation("cli wording: the application names another signature differently",
        "src/lib/properties.ts",
        '    anotherSignature: "another signature",',
        '    anotherSignature: "a further signature",',
        "says what was appended after a signature as the properties dialog does"),
]

# `tpdf redact` restates the window's route from a search hit to the regions it
# marks (`src-tauri/src/cli/regions.rs`), and its verdict sentence
# (`words.rs`); `cliregions.test.ts` and `cliwording.test.ts` hold them to
# the originals. These change the application's side alone.
ROWS += [
    Mutation("cli regions: the application ends a hit over a break at its own end",
        "src/lib/search.ts",
        "      halves.push({ slot: match.page, from: match.start, to: Infinity });",
        "      halves.push({ slot: match.page, from: match.start, to: match.end });",
        "splits a hit over a page break as matchHalves does"),
    Mutation("cli regions: the application joins characters from different lines",
        "src/lib/text.ts",
        "  return shorter > 0 && overlap / shorter > 0.5;",
        "  return shorter > 0;",
        "merges a range into the runs runsFor gives"),
    Mutation("cli regions: the application lets a comma open a run of its own",
        "src/lib/text.ts",
        "  if (overlap > 0 && shorter < Math.max(...heights) * SHORT_MARK_HEIGHT) return true;",
        "",
        "merges a range into the runs runsFor gives"),
    Mutation("cli regions: the application joins a short box that does not touch",
        "src/lib/text.ts",
        "  if (overlap > 0 && shorter < Math.max(...heights) * SHORT_MARK_HEIGHT) return true;",
        "  if (shorter < Math.max(...heights) * SHORT_MARK_HEIGHT) return true;",
        "merges a range into the runs runsFor gives"),
    Mutation("cli regions: the application keeps a sliver", "src/lib/selection.ts",
        "export const MIN_REDACTION_SIDE = 0.5;",
        "export const MIN_REDACTION_SIDE = 0;",
        "shares the viewer's limits"),
    Mutation("cli wording: the application calls a redaction clean differently",
        "src/lib/recovery.ts",
        "tpdf read the file back and none of the removed words are in it.`",
        "tpdf read the file back and it is clean.`",
        "reports a redaction as the window does"),
]

# `tpdf text` restates `reading.ts` in Rust (`src-tauri/src/reading.rs`) and
# writes every case's order to `src-tauri/testdata/cli/reading.json`;
# `clireading.test.ts` holds the two together. These change the application's
# side alone, which is the drift that test exists for.
ROWS += [
    Mutation("cli reading: the application cuts columns only at a wider gap", "src/lib/reading.ts",
        "const CUT_CHARS = 3;",
        "const CUT_CHARS = 60;",
        "finds the same lines, in the same order"),
    Mutation("cli reading: the application trusts tags that miss a visible character",
        "src/lib/reading.ts",
        "    if (isVisible(code) && placed(charQuad(text, index))) return null;",
        "    if (false) return null;",
        "takes the same route as the viewer"),
    # Fixed 2026-09-27: a first character with no box was filed under -1 and lost.
    Mutation("reading: drop a first character that has no box", "src/lib/reading.ts",
        "  if (leading) {\n    const first",
        "  if (false && leading) {\n    const first",
        "keeps a first character that has no box"),
]

# `signing.ts` is the sequence and the sentences; `App.svelte` supplies the
# shell. Each mutation removes one step or one word the reader relies on.
ROWS += [
    Mutation("signing: sign over unsaved edits", "src/lib/signing.ts",
        "  if (shell.dirty()) return UNSAVED;\n", "",
        "refuses unsaved edits before asking the OS anything"),
    Mutation("signing: read dirty without settling the drafts", "src/lib/signing.ts",
        "  await shell.settle();\n  if (shell.dirty()) return UNSAVED;\n",
        "  if (shell.dirty()) return UNSAVED;\n",
        "signDocument_settles_drafts_before_it_reads_dirty"),
    Mutation("signing: settle the drafts without waiting for them", "src/lib/signing.ts",
        "  await shell.settle();\n  if (shell.dirty()) return UNSAVED;\n",
        "  void shell.settle();\n  if (shell.dirty()) return UNSAVED;\n",
        "waits for the drafts to settle before it asks anything else"),
    Mutation("signing: go on after the chooser was cancelled", "src/lib/signing.ts",
        "  if (chosen === null) return null;\n", "",
        "stops without a word when the reader cancels either question"),
    Mutation("signing: go on after the save panel was cancelled", "src/lib/signing.ts",
        "  if (!path) return null;\n", "",
        "stops without a word when the reader cancels either question"),
    Mutation("signing: open the chooser with nothing in it", "src/lib/signing.ts",
        "  if (choices.usable.length === 0) return nothingToChoose(choices);\n", "",
        "says what was found when nothing can sign, and asks nothing more"),
    Mutation("signing: report success for a signature that did not read back intact",
        "src/lib/signing.ts",
        '  if (!ours || (ours.integrity?.verdict !== "intact" && !unchecked)) {',
        "  if (!ours) {",
        "puts a new signature that did not read back intact first, and warns"),
    Mutation("signing: say do not rely of a signature not checked again", "src/lib/signing.ts",
        '  const unchecked =\n    ours?.integrity?.verdict === "unchecked" && ours.integrity.why === "budget";',
        "  const unchecked = false;",
        "says which check happened for a new signature the read-back had no budget for"),
    Mutation("signing: say not checked again of any unchecked signature", "src/lib/signing.ts",
        '    ours?.integrity?.verdict === "unchecked" && ours.integrity.why === "budget";',
        '    ours?.integrity?.verdict === "unchecked";',
        "says which check happened for a new signature the read-back had no budget for"),
    Mutation("signing: report the timestamp of a signature not checked again", "src/lib/signing.ts",
        "  const stamp = unchecked ? null : ours.timestamp;",
        "  const stamp = ours.timestamp;",
        "says which check happened for a new signature the read-back had no budget for"),
    Mutation("reading: read a line's halves in the order of their top edges", "src/lib/reading.ts",
        "  if (fragments.length < 2) return fragments;\n  const rows",
        "  if (fragments.length < Infinity) return fragments;\n  const rows",
        "is read left to right although the right half stands higher"),
    Mutation("reading: read every row from its left end", "src/lib/reading.ts",
        "    const backwards = writtenBackwards(text, row, axes);",
        "    const backwards = false;",
        "is read from its right end when its characters are written right to left"),
    Mutation("reading: read every row from its right end", "src/lib/reading.ts",
        "    const backwards = writtenBackwards(text, row, axes);",
        "    const backwards = true;",
        "is read left to right by the geometry too, where no cut parts the halves"),
    Mutation("reading: let one step back turn a row round", "src/lib/reading.ts",
        "  return backwards > forwards;",
        "  return backwards > 0;",
        "is not turned round by one word written the other way"),
    Mutation("reading: count the step back to a combining mark", "src/lib/reading.ts",
        "        if (!placed(box) || combining(text.codes[index] ?? 0)) continue;",
        "        if (!placed(box)) continue;",
        "is not turned round by a combining mark drawn left of its base"),
    Mutation("reading: take whatever touches a line for a half of its row", "src/lib/reading.ts",
        "    const row = rows.find((candidate) => sameRow(candidate.first, extents));",
        "    const row = rows.find((candidate) => sameBand(candidate.first, extents));",
        "is not made of type of another size that merely touches the line"),
    Mutation("reading: measure a row against the shorter fragment", "src/lib/reading.ts",
        "  const taller = Math.max(a.crossEnd - a.crossStart, b.crossEnd - b.crossStart);",
        "  const taller = Math.min(a.crossEnd - a.crossStart, b.crossEnd - b.crossStart);",
        "is not made of type of another size that merely touches the line"),
    Mutation("forms: an arrow key goes round the group the wrong way", "src/lib/forms.ts",
        "  const ahead = step === 1 ? [...group.slice(at + 1), ...group.slice(0, at)]",
        "  const ahead = step !== 1 ? [...group.slice(at + 1), ...group.slice(0, at)]",
        "is the rest of the group in the order an arrow key reaches it"),
    Mutation("forms: an arrow key answers with a button that is put away", "src/lib/forms.ts",
        "          if (stop.to) { stop.to.input.focus({ preventScroll: true }); stop.to.input.click(); }",
        "          const blind = ring[1]; if (blind) { blind.input.focus({ preventScroll: true }); blind.input.click(); } void stop;",
        "moves an arrow key's answer to the next button that is shown"),
    Mutation("search: keep the page total of a search that was dropped", "src/lib/search.ts",
        "    this.scanned = 0;\n    this.toScan = 0;",
        "    this.scanned = 0;",
        "drops the scope with the matches, since both name slots"),
    Mutation("signing: leave the earlier signatures out of the report", "src/lib/signing.ts",
        "  const earlier = signed.signatures.filter((s) => !s.ours && !s.archive);",
        "  const earlier = signed.signatures.filter(() => false);",
        "lists every earlier signature after the new one"),
    Mutation("signing: strip only a lowercase .pdf from the suggested name", "src/lib/signing.ts",
        '  const stem = basename(openPath).replace(/\\.pdf$/i, "");',
        '  const stem = basename(openPath).replace(/\\.pdf$/, "");',
        "suggests a -signed name beside the original, whatever its case"),
    Mutation("signing: say nothing about what was found but not offered", "src/lib/signing.ts",
        "  if (choices.skipped.length === 0) return none + WITHOUT_CERTIFICATE;\n",
        "  return none + WITHOUT_CERTIFICATE;\n",
        "names what was found but not offered, and where a picture without a certificate is"),
    Mutation("signing: send nobody to the picture that needs no certificate", "src/lib/signing.ts",
        "  if (choices.skipped.length === 0) return none + WITHOUT_CERTIFICATE;\n",
        "  if (choices.skipped.length === 0) return none;\n",
        "names what was found but not offered, and where a picture without a certificate is"),
    Mutation(
        # The inert command again: registered, guarded, and reaching nothing.
        "appcommands: register Sign document without reaching its action",
        "src/lib/appcommands.ts",
        "      run: () => actions.signDocument(),",
        "      run: () => actions.saveCopy(),",
        "signs the document through its own command, offered with or without edits"),
    Mutation("appcommands: withhold Sign document on an edited document",
        "src/lib/appcommands.ts",
        "      title: TEXT(locale).signDocument,\n"
        "      enabled: withDocument,",
        "      title: TEXT(locale).signDocument,\n"
        "      enabled: () => withDocument() && !actions.isDirty(),",
        "signs the document through its own command, offered with or without edits"),
]

# --- a visible appearance for a digital signature (Phase 6, 2026-09-27) --------
# `signing.ts` asks where and hands the image on; `viewer.ts` lends the crop's
# drag for one rectangle and must answer every waiter. Each mutation removes
# one of those promises.
ROWS += [
    Mutation("signing: sign visibly without asking where", "src/lib/signing.ts",
        "  if (chosen.visible) {", "  if (false) {",
        "asks what a visible signature shows, then where, then the file, then signs"),
    Mutation("signing: ask where an invisible signature goes", "src/lib/signing.ts",
        "  if (chosen.visible) {", "  if (true) {",
        "signs invisibly by default, asking nothing about a page or an image"),
    Mutation("signing: go on after the placement was escaped", "src/lib/signing.ts",
        "    if (placed === null) return null;\n", "",
        "stops without a word when the reader escapes the placement"),
    Mutation("signing: leave the chosen image out of the placement", "src/lib/signing.ts",
        "    placement = { ...placed, image: appearance.image, options: appearance.options };",
        "    placement = { ...placed, image: null, options: appearance.options };",
        "asks what a visible signature shows, then where, then the file, then signs"),
    Mutation("signing: answer visible from the chooser whatever was picked", "src/lib/signing.ts",
        "        visible: visible.checked,\n        timestamp,\n",
        "        visible: true,\n        timestamp,\n",
        "answers a visible signature unless the reader picks an invisible one"),
    Mutation("signing: open the chooser with neither appearance picked", "src/lib/signing.ts",
        '  const visible = appearance("visible", said.visible, true);',
        '  const visible = appearance("visible", said.visible, false);',
        "answers a visible signature unless the reader picks an invisible one"),
    Mutation("lock: never ask the backend for it", "src/lib/signing.ts",
        "    chosen.lock === true,\n  );", "    false,\n  );",
        "is asked of the backend as the reader chose, and never by omission"),
    Mutation("lock: ask for it when the reader was never asked", "src/lib/signing.ts",
        "    chosen.lock === true,\n  );", "    chosen.lock !== false,\n  );",
        "is asked of the backend as the reader chose, and never by omission"),
    Mutation("lock: say it of a signature that locks nothing", "src/lib/signing.ts",
        "  if (ours.lock === 1 && !unchecked) text += LOCKED;",
        "  if (!unchecked) text += LOCKED;",
        "is said from what was read back, right after the verdict, and only beside an intact one"),
    Mutation("lock: say it of a signature that was not checked again", "src/lib/signing.ts",
        "  if (ours.lock === 1 && !unchecked) text += LOCKED;",
        "  if (ours.lock === 1) text += LOCKED;",
        "is said from what was read back, right after the verdict, and only beside an intact one"),
    Mutation("lock: select an answer before the reader gives one", "src/lib/signing.ts",
        "    radio.name = LOCK_NAME;\n    radio.value = answer.value;\n    radio.checked = false;",
        "    radio.name = LOCK_NAME;\n    radio.value = answer.value;\n    radio.checked = answer.lock;",
        "asks whether anyone else signs, with no answer, and signs nothing until it has one"),
    Mutation("lock: sign with the question unanswered", "src/lib/signing.ts",
        "      if (lock === null) {\n        unanswered.textContent = LOCK_UNANSWERED;\n        return;\n      }",
        "      if (lock === null) {\n        unanswered.textContent = LOCK_UNANSWERED;\n      }",
        "asks whether anyone else signs, with no answer, and signs nothing until it has one"),
    Mutation("lock: answer a lock whatever was chosen", "src/lib/signing.ts",
        "        lock,\n      });", "        lock: true,\n      });",
        "asks whether anyone else signs, with no answer, and signs nothing until it has one"),
    Mutation("lock: lock for the reader who says others still sign", "src/lib/signlock.ts",
        "    text: T.others,\n"
        '    hint: "",\n'
        "    lock: false,",
        "    text: T.others,\n"
        '    hint: "",\n'
        "    lock: true,",
        "locks only for the reader who says nobody else signs"),
    Mutation("lock: read an unknown answer as a lock", "src/lib/signlock.ts",
        "?.lock ?? null;", "?.lock ?? true;",
        "has no answer while none is chosen or the value is not one of ours"),
    Mutation("viewer: leave a placement waiting when the tool is taken", "src/lib/viewer.ts",
        '    if (next.kind !== "place" && this.placing) {', "    if (false) {",
        "answers nothing when another tool takes the hand or the viewer goes"),
    Mutation("viewer: leave a placement waiting when the viewer goes", "src/lib/viewer.ts",
        "    // document it was for is going, and its sequence must not wait forever.\n    this.tool = NO_TOOL;\n",
        "    // document it was for is going, and its sequence must not wait forever.\n",
        "answers nothing when another tool takes the hand or the viewer goes"),
    Mutation("viewer: send a placed rectangle to the crop callback", "src/lib/viewer.ts",
        "        if (placing) placing({ page: id, rect });\n        else if (marking)",
        "        if (false) placing({ page: id, rect });\n        else if (marking)",
        "answers the dragged rectangle, in the space a crop is reported in, and crops nothing"),
    Mutation("viewer: refuse the drag a placement is armed for", "src/lib/viewer.ts",
        # Re-aimed 2026-10-16 when the region to copy or save joined the crop's drag.
        '        if (this.tool.kind !== "crop" && this.tool.kind !== "redact" && this.tool.kind !== "place" && this.tool.kind !== "region") {',
        '        if (this.tool.kind !== "crop" && this.tool.kind !== "redact" && this.tool.kind !== "region") {',
        "answers the dragged rectangle, in the space a crop is reported in, and crops nothing"),
    Mutation("viewer: answer a committed placement with nothing", "src/lib/viewer.ts",
        "        const placing = this.placing;\n        this.placing = null;\n",
        "        const placing = this.placing;\n",
        "answers the dragged rectangle, in the space a crop is reported in, and crops nothing"),
]

# --- configuring a visible signature's appearance (2026-09-27) ---------------
# `signappearance.ts` holds the panel's state, the remembered default and the
# dialog; `signing.ts` puts the panel between the chooser and the drag. Each
# mutation removes one rule the reader relies on.
ROWS += [
    Mutation("signing: go on after the appearance panel was cancelled", "src/lib/signing.ts",
        "    if (appearance === null) return null;\n", "",
        "stops without a word, and asks nothing more, when the reader cancels the panel"),
    Mutation("signing: sign with the default lines whatever the panel chose", "src/lib/signing.ts",
        "    placement = { ...placed, image: appearance.image, options: appearance.options };",
        '    placement = { ...placed, image: appearance.image, options: { label: true, name: true, date: true, reason: "", location: "" } };',
        "asks what a visible signature shows, then where, then the file, then signs"),
    Mutation("signing: offer the panel no saved image", "src/lib/signing.ts",
        "    const appearance = await shell.appearance(chosen.identity, saved);",
        "    const appearance = await shell.appearance(chosen.identity, null);",
        "asks what a visible signature shows, then where, then the file, then signs"),
    Mutation("appearance: trust a remembered image source tpdf never writes",
        "src/lib/signappearance.ts",
        '  if (image !== "saved" && image !== "none") return defaults();\n', "",
        "falls back to the defaults, whole, for anything tpdf did not write"),
    Mutation("appearance: trust a remembered line that is not a boolean",
        "src/lib/signappearance.ts",
        "  if (!flag(o.label) || !flag(o.name)", "  if (!flag(o.name)",
        "falls back to the defaults, whole, for anything tpdf did not write"),
    Mutation("appearance: trust a remembered reason of any length",
        "src/lib/signappearance.ts",
        '  const text = (v: unknown): v is string => typeof v === "string" && v.length <= MAX_NOTE_CHARS;',
        '  const text = (v: unknown): v is string => typeof v === "string";',
        "falls back to the defaults, whole, for anything tpdf did not write"),
    Mutation("appearance: read a remembered value that is not an object",
        "src/lib/signappearance.ts",
        '  if (typeof value !== "object" || value === null) return defaults();\n', "",
        "falls back to the defaults, whole, for anything tpdf did not write"),
    Mutation("appearance: hand out the shared defaults themselves",
        "src/lib/signappearance.ts",
        "  return { image: DEFAULT_PREFERENCE.image, options: { ...DEFAULT_PREFERENCE.options } };",
        "  return DEFAULT_PREFERENCE;",
        "hands out a copy of the defaults, so changing one changes nothing shared"),
    Mutation("appearance: remember the choices without being asked",
        "src/lib/signappearance.ts",
        "    if (this.remember) {", "    if (true) {",
        "keeps nothing unless asked, and nothing on Cancel"),
    Mutation("appearance: remember an image drawn now as no image",
        "src/lib/signappearance.ts",
        '        { image: this.source === "none" ? "none" : "saved", options: { ...this.options } },',
        '        { image: this.source === "saved" ? "saved" : "none", options: { ...this.options } },',
        "remembers an image drawn in the panel as the saved one, never its pixels"),
    Mutation("appearance: open on the saved image when none is saved",
        "src/lib/signappearance.ts",
        '    this.source = preference.image === "saved" && shell.saved ? "saved" : "none";',
        '    this.source = preference.image === "saved" ? "saved" : "none";',
        "opens on no image when the saved one is wanted and there is none"),
    Mutation("appearance: choose the saved image when there is none",
        "src/lib/signappearance.ts",
        '    if (source === "saved" && !this.shell.saved) return false;\n', "",
        "refuses a source with nothing in it, and keeps the one it had"),
    Mutation("appearance: paint an earlier preview over the latest",
        "src/lib/signappearance.ts",
        "      if (this.#closed || generation !== this.#generation) return;\n      await this.shell.show(preview);",
        "      if (this.#closed) return;\n      await this.shell.show(preview);",
        "shows only the latest, whichever order the answers land in"),
    Mutation("appearance: paint a preview after the panel was cancelled",
        "src/lib/signappearance.ts",
        "      if (this.#closed || generation !== this.#generation) return;\n      await this.shell.show(preview);",
        "      if (generation !== this.#generation) return;\n      await this.shell.show(preview);",
        "paints nothing and says nothing once the panel is cancelled"),
    Mutation("appearance: let a refused appearance be placed",
        "src/lib/signappearance.ts",
        '    return !this.#closed && this.state.kind !== "refused";',
        "    return !this.#closed;",
        "holds Place on page while the choices are refused, with the signing's words"),
    Mutation("appearance: preview the default lines whatever is chosen",
        "src/lib/signappearance.ts",
        "      const preview = await this.shell.preview(this.image(), options);",
        "      const preview = await this.shell.preview(this.image(), { ...DEFAULT_PREFERENCE.options });",
        "is asked for the image and options chosen now"),
    Mutation("appearance: place on the page what the signing would refuse",
        "src/lib/signappearance.ts",
        "      if (!panel.canContinue()) return;\n", "",
        "will not place what the signing would refuse, and says why"),
    Mutation("appearance: leave a preview free to paint after Cancel",
        "src/lib/signappearance.ts",
        "      if (answer === null) panel.cancel();\n", "",
        "answers nothing for Cancel or Escape, and paints nothing still on its way"),
    Mutation("appearance: drop what was typed and not yet drawn",
        "src/lib/signappearance.ts",
        "    panel.setLocation(location.value);\n    redrawSoon();",
        "    redrawSoon();",
        "previews on opening, and answers what the controls say on Place on page"),
    Mutation("appearance: ignore the image drawn in the Phase 4 dialog",
        "src/lib/signappearance.ts",
        "      if (image) panel.useDrawn(image);\n", "",
        "uses an image drawn in the Phase 4 dialog, and keeps its choice when that is cancelled"),
    Mutation("appearance: ignore Remember as my default",
        "src/lib/signappearance.ts",
        "    panel.remember = remember.checked;", "    panel.remember = false;",
        "keeps the choices when Remember is ticked"),
    Mutation("signature dialog: keep the last use's title",
        "src/lib/signaturedialog.ts",
        "    this.heading.textContent = words.title;\n", "",
        "is called what the signing panel asks, and what it was built for the next time"),
]

# --- a timestamp when signing (Phase 6 step 3, increment B, 2026-09-28) -------
# `signtimestamp.ts` holds the list, the remembered choice and the address
# check; `signing.ts` asks in the chooser and answers a timestamp that did not
# come. Each mutation removes one promise the owner's decision rests on.
ROWS += [
    Mutation("timestamp: select an authority whatever the reader chose",
        "src/lib/signing.ts",
        "  stampList.value = remembered.server;",
        '  stampList.value = "sectigo";',
        "asks nobody for a reader who chose none, this time and the next"),
    Mutation("timestamp: preselect nothing for a reader who never chose",
        "src/lib/signing.ts",
        "  const remembered = readStampChoice(storage, undefined, preselectedStamp);",
        "  const remembered = readStampChoice(storage);",
        "preselects one authority, by name, for a reader who has never chosen"),
    Mutation("timestamp: preselect from storage that cannot be read",
        "src/lib/signtimestamp.ts",
        "  } catch {\n    return noStamp();\n  }\n  if (raw === null) return never();",
        "  } catch {\n    return never();\n  }\n  if (raw === null) return never();",
        "are asked only when chosen or preselected: a signing preselects one, nothing else does"),
    Mutation("timestamp: preselect the authority asked over plain HTTP",
        "src/lib/signtimestamp.ts",
        'export const PRESELECTED = "sectigo";',
        'export const PRESELECTED = "digicert";',
        "are asked only when chosen or preselected: a signing preselects one, nothing else does"),
    Mutation("timestamp: forget the reader's choice", "src/lib/signing.ts",
        "      writeStampChoice(chosenStamp, storage);\n", "",
        "remembers the reader's choice and offers it next time"),
    Mutation("timestamp: sign with none when another authority's address is mistyped",
        "src/lib/signing.ts",
        "        if (wrong !== null) {", "        if (false) {",
        "holds the chooser open on another authority's address it would not ask"),
    Mutation("timestamp: trust a remembered authority tpdf never lists",
        "src/lib/signtimestamp.ts",
        '  if (typeof server !== "string" || !known(server)) return noStamp();',
        '  if (typeof server !== "string") return noStamp();',
        "falls back to none, whole, for anything tpdf did not write"),
    Mutation("timestamp: hand the backend no authority", "src/lib/signing.ts",
        "    chosen.identity, path, placement, chosen.timestamp, longTerm, field?.name ?? null,",
        "    chosen.identity, path, placement, null, longTerm, field?.name ?? null,",
        "is asked of the chosen authority, and the closing sentence says what it attests"),
    Mutation("timestamp: leave the attested time out of the closing sentence",
        "src/lib/signing.ts",
        "  if (stamp) {", "  if (false) {",
        "is asked of the chosen authority, and the closing sentence says what it attests"),
    Mutation("timestamp: try again without the authority", "src/lib/signing.ts",
        '          next === "retry" ? chosen.timestamp : null,',
        "          null,",
        "asks the same authority again when the reader tries again"),
    Mutation("timestamp: keep the signature held after the reader cancels",
        "src/lib/signing.ts",
        "      await shell.discard(pending);\n", "",
        "drops the signature and says nothing was written when the reader cancels"),
]

# --- the watch on the open file (2026-10-01) ----------------------------------
# `diskwatch.ts` decides whether the open file changed on disk, what happens
# then, and what the reader chose. Each mutation removes one thing a reader
# would notice: a reload of a half-written file, a prompt every second, a
# prompt for a file that was only touched, or lost edits.
ROWS += [
    Mutation("disk watch: act on the first sight of a new stamp", "src/lib/diskwatch.ts",
        "        this.seen = now;\n        return;\n", "        this.seen = now;\n",
        "reports a changed file once, and only after the stamp held still"),
    Mutation("disk watch: report the same change on every check", "src/lib/diskwatch.ts",
        "      if (now === this.judged) return;\n", "",
        "reports a changed file once, and only after the stamp held still"),
    Mutation("disk watch: report a file whose bytes did not change", "src/lib/diskwatch.ts",
        "      if ((await this.probe.differs(doc, path)) === false) {",
        "      if ((await this.probe.differs(doc, path)) === undefined) {",
        "compares a touched file by content once, and stays quiet"),
    # The `opened` half only. Dropping the `now` half changes nothing: a `null`
    # now equals the `null` that `judged` starts at, so the next line returns.
    # Run and it survived, which is how that was found.
    Mutation("disk watch: report a document that has no stamp from its open", "src/lib/diskwatch.ts",
        "      if (opened === null || now === null) return;",
        "      if (now === null) return;",
        "says nothing when either stamp could not be read"),
    Mutation("disk watch: drop a report nobody acted on", "src/lib/diskwatch.ts",
        "      if (await this.report(doc)) this.judged = now;",
        "      await this.report(doc);\n      this.judged = now;",
        "reports again when the report was not acted on"),
    # A promise is truthy, so without the wait every report reads as acted on.
    Mutation("disk watch: judge a report before it has answered", "src/lib/diskwatch.ts",
        "      if (await this.report(doc)) this.judged = now;",
        "      if (this.report(doc)) this.judged = now;",
        "is waited for by the watch, which reports again after a 'not now'"),
    Mutation("disk change: reload on the dirty read before the drafts settled", "src/lib/diskwatch.ts",
        "      await host.settle();\n      dirty = host.dirty();\n",
        "      await host.settle();\n",
        "a_draft_the_settle_commits_gets_the_prompt_and_not_the_reload"),
    Mutation("disk change: reload over a draft that could not be committed", "src/lib/diskwatch.ts",
        "    } catch {\n      dirty = true;\n    }\n    if (!host.current()) return false;",
        "    } catch {\n    }\n    if (!host.current()) return false;",
        "asks about a draft that could not be committed"),
    Mutation("disk change: act after the wait let another document in", "src/lib/diskwatch.ts",
        "    if (!host.current()) return false;\n", "",
        "does nothing, and says it did not act, when the wait let something else in"),
    Mutation("disk change: settle the drafts to show a prompt", "src/lib/diskwatch.ts",
        '  if (mode === "reload" && !dirty) {', "  if (!dirty) {",
        "settles nothing where no reload is coming"),
    Mutation("disk watch: carry one document's verdict to the next", "src/lib/diskwatch.ts",
        "        this.doc = doc;\n        this.seen = null;\n        this.judged = null;\n",
        "        this.doc = doc;\n",
        "starts over for another document"),
    Mutation("disk watch: reload over unsaved edits when set to reload", "src/lib/diskwatch.ts",
        "  if (dirty) {", '  if (dirty && mode !== "reload") {',
        "reloads without asking only when there is nothing to lose"),
    Mutation("disk watch: prompt a reader who chose not to look", "src/lib/diskwatch.ts",
        '  if (mode === "ignore") return null;\n', "",
        "does nothing when the reader chose not to look"),
    Mutation("disk watch: trust a remembered mode tpdf never wrote", "src/lib/diskwatch.ts",
        "  return DISK_CHANGE_MODES.find((mode) => mode === raw) ?? DEFAULT_DISK_CHANGE_MODE;",
        "  return (raw as DiskChangeMode | null) ?? DEFAULT_DISK_CHANGE_MODE;",
        "reads anything tpdf did not write as the default"),
]

# --- long-term validation data when signing (increment C2, 2026-09-28) -------
# `signtimestamp.ts` remembers the choice; `signing.ts` asks it in the chooser,
# only with a timestamp, and answers the data that did not come.
ROWS += [
    Mutation("long-term: ask for it with no timestamp", "src/lib/signing.ts",
        "  const longTerm = chosen.longTerm === true && chosen.timestamp !== null;",
        "  const longTerm = chosen.longTerm === true;",
        "is asked for only with a timestamp, and said when it reads back good"),
    Mutation("long-term: answer its failure with the timestamp's question", "src/lib/signing.ts",
        "    const next = afterLongTerm ? await shell.longTermFailed(why) : await shell.stampFailed(why);",
        "    const next = await shell.stampFailed(why);",
        "asks its own question when it does not come, and signs without it only when told"),
    Mutation("long-term: gather it again when told to sign without it", "src/lib/signing.ts",
        '        await shell.resume(pending, chosen.timestamp, next === "retry")',
        "        await shell.resume(pending, chosen.timestamp, true)",
        "asks its own question when it does not come, and signs without it only when told"),
    Mutation("long-term: never gather it again", "src/lib/signing.ts",
        '        await shell.resume(pending, chosen.timestamp, next === "retry")',
        "        await shell.resume(pending, chosen.timestamp, false)",
        "gathers it again, keeping the timestamp, when the reader tries again"),
    Mutation("long-term: ask for it after signing without a timestamp", "src/lib/signing.ts",
        '          next === "retry" && longTerm,',
        "          longTerm,",
        "is asked again with a timestamp tried again, and never with none"),
    Mutation("long-term: let it be ticked with no authority chosen", "src/lib/signing.ts",
        "    longTerm.disabled = stampList.value === \"none\";",
        "    longTerm.disabled = false;",
        "is unticked, and cannot be ticked while no authority is chosen"),
    Mutation("long-term: forget the reader's choice", "src/lib/signing.ts",
        "      writeLongTerm(longTerm.checked, storage);\n", "",
        "is remembered, and asked for only with a timestamp"),
    Mutation("long-term: answer it from the chooser without an authority", "src/lib/signing.ts",
        "        longTerm: longTerm.checked && timestamp !== null,",
        "        longTerm: longTerm.checked,",
        "is remembered, and asked for only with a timestamp"),
    Mutation("long-term: tick it for a reader who never did", "src/lib/signtimestamp.ts",
        '    return storage().getItem(LONG_TERM_KEY) === "true";',
        '    return storage().getItem(LONG_TERM_KEY) !== "false";',
        "is unticked until written ticked, and reads back what was written"),
    Mutation("long-term: leave the revocation out of the closing sentence", "src/lib/signing.ts",
        '  if (ours.revocation?.standing === "good") {', "  if (false) {",
        "is asked for only with a timestamp, and said when it reads back good"),
]

# --- long-term validation data for a document already signed -----------------
#
# `validationdata.ts`, 2026-10-10: the sequence in front of
# `add_validation_data`, and the dialog that names the timestamp authority.
# What goes wrong here is an order --- asking for an authority and a file name
# for a document nobody signed, reading `dirty` before the drafts settle ---
# and one separation: the authority chosen here is remembered under its own
# key, so that choosing one never makes a signing ask a server by default.
ROWS += [
    Mutation("validation-data: add to a document with unsaved edits", "src/lib/validationdata.ts",
        "  if (shell.dirty()) return UNSAVED;",
        "  if (false) return UNSAVED;",
        "refuses unsaved edits before anything is read or asked"),
    Mutation("validation-data: read dirty before the drafts settle", "src/lib/validationdata.ts",
        "  await shell.settle();\n  if (shell.dirty()) return UNSAVED;",
        "  if (shell.dirty()) return UNSAVED;\n  await shell.settle();",
        "settles drafts before it reads dirty"),
    Mutation("validation-data: ask for an authority for a document nobody signed", "src/lib/validationdata.ts",
        "  if (!hasSignature(await shell.properties())) return NOTHING_SIGNED;",
        "  await shell.properties();",
        "tells a document with no signature so, before any dialog"),
    Mutation("validation-data: count an empty signature field as a signature", "src/lib/validationdata.ts",
        "  return properties.signatures.some((signature) => signature.signed);",
        "  return properties.signatures.some(() => true);",
        "counts a signed field and not an empty one"),
    Mutation("validation-data: go on after the dialog was cancelled", "src/lib/validationdata.ts",
        "  if (timestamp === null) return null;",
        "  if (timestamp === undefined) return null;",
        "is cancelled by the dialog and by the save panel, and adds nothing"),
    Mutation("validation-data: go on after the save panel was cancelled", "src/lib/validationdata.ts",
        "  if (!path) return null;\n  shell.waiting(WAITING);",
        "  shell.waiting(WAITING);",
        "is cancelled by the dialog and by the save panel, and adds nothing"),
    Mutation("validation-data: suggest a name that keeps an upper-case extension", "src/lib/validationdata.ts",
        '  const stem = basename(openPath).replace(/\\.pdf$/i, "");\n  return `${stem}-long-term.pdf`;',
        '  const stem = basename(openPath).replace(/\\.pdf$/, "");\n  return `${stem}-long-term.pdf`;',
        "suggests a name beside the original"),
    Mutation("validation-data: answer with no authority chosen", "src/lib/validationdata.ts",
        "      if (address === null) {\n        problem.textContent = CHOOSE;\n        return;\n      }",
        "      if (address === null) {\n        problem.textContent = CHOOSE;\n      }",
        "preselects nothing the first time, and holds Add until an authority is chosen"),
    Mutation("validation-data: read the signing's remembered authority", "src/lib/validationdata.ts",
        "  const remembered = readStampChoice(storage, AUTHORITY_KEY);",
        "  const remembered = readStampChoice(storage);",
        "remembers the choice under its own key, and never a signing's"),
    Mutation("validation-data: remember the authority as a signing's", "src/lib/validationdata.ts",
        "      writeStampChoice(chosen, storage, AUTHORITY_KEY);",
        "      writeStampChoice(chosen, storage);",
        "remembers the choice under its own key, and never a signing's"),
    Mutation("validation-data: read every remembered choice from the signing's key", "src/lib/signtimestamp.ts",
        "    raw = storage().getItem(key);",
        "    raw = storage().getItem(CHOICE_KEY);",
        "remembers the choice under its own key, and never a signing's"),
    Mutation("validation-data: write every remembered choice to the signing's key", "src/lib/signtimestamp.ts",
        "    storage().setItem(key, JSON.stringify(choice));",
        "    storage().setItem(CHOICE_KEY, JSON.stringify(choice));",
        "remembers the choice under its own key, and never a signing's"),
    Mutation("validation-data: say nothing of an address tpdf does not ask", "src/lib/validationdata.ts",
        "        if (wrong !== null) {\n          problem.textContent = wrong;\n          return;\n        }",
        "        if (wrong !== null) {\n          problem.textContent = CHOOSE;\n          return;\n        }",
        "holds another authority's address to what tpdf asks"),
    Mutation("validation-data: remember an authority the reader cancelled", "src/lib/validationdata.ts",
        '    cancel.addEventListener("click", () => finish(null));\n    next.addEventListener("click", () => {\n      const server = radios.find((radio) => radio.checked)?.value ?? "";',
        '    cancel.addEventListener("click", () => {\n      writeStampChoice({ server: "digicert", url: "" }, storage, AUTHORITY_KEY);\n      finish(null);\n    });\n    next.addEventListener("click", () => {\n      const server = radios.find((radio) => radio.checked)?.value ?? "";',
        "answers null for Cancel and for Escape, and remembers nothing"),
    Mutation("validation-data: let Escape close the dialog its own way", "src/lib/validationdata.ts",
        '    dialog.addEventListener("cancel", (event) => {\n      event.preventDefault();\n      finish(null);\n    });\n    dialog.addEventListener("close", () => finish(null));\n    dialog.addEventListener("keydown", (event) => event.stopPropagation());\n    dialog.showModal();\n    (radios.find',
        '    dialog.addEventListener("cancel", () => {\n      finish(null);\n    });\n    dialog.addEventListener("close", () => finish(null));\n    dialog.addEventListener("keydown", (event) => event.stopPropagation());\n    dialog.showModal();\n    (radios.find',
        "answers null for Cancel and for Escape, and remembers nothing"),
    Mutation("validation-data: wire the command to the signing beside it", "src/lib/appcommands.ts",
        "      run: () => actions.addValidationData(),",
        "      run: () => actions.signDocument(),",
        "adds validation data through its own command, offered on any open document"),
    Mutation("validation-data: withhold the command from an edited document", "src/lib/appcommands.ts",
        "      title: TEXT(locale).addValidationData,\n"
        "      enabled: withDocument,",
        "      title: TEXT(locale).addValidationData,\n"
        "      enabled: () => withDocument() && !actions.isDirty(),",
        "adds validation data through its own command, offered on any open document"),
    Mutation("validation-data: leave the command out of the File menu", "src/lib/menubar.ts",
        '      "file.addValidationData",\n',
        "",
        "keeps separators where the layout puts them"),
    Mutation("validation-data: say nothing while the backend works", "src/lib/validationdata.ts",
        "  shell.waiting(WAITING);\n",
        "",
        "asks in order, and answers the backend's sentence"),
    Mutation("validation-data: go on saying it waits after a refusal", "src/lib/validationdata.ts",
        "    return said;\n  } finally {\n    // Whether it answered or refused: the wait is over either way.\n    shell.waiting(null);\n  }",
        "    shell.waiting(null);\n    return said;\n  } finally {\n    // Whether it answered or refused: the wait is over either way.\n  }",
        "says it is waiting only while the backend works, and stops when it refuses"),
]

# --- revocation, and the signer judged at an attested time ------------------
#
# `integrity.ts` and `properties.ts`, 2026-09-28 (Phase 6 step 3, increment
# C1). The words are where a revocation answer goes wrong: `none` read as
# reassurance, a later revocation read as undoing the signature, a moment
# with nobody's clock named.
ROWS += [
    Mutation(
        "revocation: call a document with no revocation data not revoked",
        "src/lib/integrity.ts",
        "            `not checked — the document carries no revocation data for ${w.whose}, and tpdf ` +",
        "            `not revoked — the document carries no revocation data for ${w.whose}, and tpdf ` +",
        "says a document carrying no revocation data was not checked, never that it is fine",
    ),
    Mutation(
        "revocation: let none pass without a warning",
        "src/lib/integrity.ts",
        '    revocation.standing === "good" ||\n'
        '    (revocation.standing === "revoked" && revocation.after_moment);',
        '    revocation.standing === "good" || revocation.standing === "none" ||\n'
        '    (revocation.standing === "revoked" && revocation.after_moment);',
        "marks everything but a good answer and a later revocation for a reader's attention",
    ),
    Mutation(
        "revocation: word every revocation as after the timestamp",
        "src/lib/integrity.ts",
        "          if (r.after_moment) {\n"
        "            return (\n"
        "              `revoked after the timestamp",
        "          if (true) {\n"
        "            return (\n"
        "              `revoked after the timestamp",
        "says a revocation after an attested moment does not undo the signature, and only then",
    ),
    Mutation(
        "revocation: call the attested moment the authority's own statement",
        "src/lib/integrity.ts",
        "      return `${at}, the time the timestamp attests`;",
        "      return `${at}, the time the timestamp states`;",
        "names whose clock the moment is",
    ),
    Mutation(
        "revocation: drop the revocation row",
        "src/lib/properties.ts",
        "  if (withdrawn) rows.push(withdrawn);",
        "",
        "puts the revocation row under the trust row, the authority's for a document timestamp",
    ),
    Mutation(
        "revocation: word a document timestamp's revocation as the signer's",
        "src/lib/properties.ts",
        "revocationRow(signature.revocation, document);",
        "revocationRow(signature.revocation, false);",
        "puts the revocation row under the trust row, the authority's for a document timestamp",
    ),
    Mutation(
        "revocation: drop the authority's revocation row",
        "src/lib/properties.ts",
        "    if (lapsed) rows.push(lapsed);",
        "",
        "puts the authority's revocation under its standing, and only beside one",
    ),
    Mutation(
        "chain: show a chain of the leaf alone",
        "src/lib/integrity.ts",
        "  if (chain.certificates.length <= 1 && chain.dropped === 0) return null;",
        "",
        "is not shown when nothing stands above the leaf, which its own row then says",
    ),
    Mutation(
        "chain: name the leaf when an issuing certificate decides",
        "src/lib/integrity.ts",
        "  const shown = revocationRow(judged.revocation, authority, issuingCertificate(judged));",
        "  const shown = revocationRow(judged.revocation, authority);",
        "names the issuing certificate a revocation is about, and warns",
    ),
    Mutation(
        "chain: warn about nothing",
        "src/lib/integrity.ts",
        '  const warn = !(chain.standing === "good" || (chain.standing === "revoked" && chain.after_moment));',
        "  const warn = false;",
        "never calls a chain with a certificate nothing was said about good",
    ),
    Mutation(
        "chain: drop the chain row",
        "src/lib/properties.ts",
        "  if (above) rows.push(above);",
        "",
        "puts each chain's row under its certificate's own, and only when there is a chain above it",
    ),
    Mutation(
        "chain: drop the authority's chain row",
        "src/lib/properties.ts",
        "    if (lapsedAbove) rows.push(lapsedAbove);",
        "",
        "puts each chain's row under its certificate's own, and only when there is a chain above it",
    ),
    Mutation(
        "archive: list the archive timestamp as an earlier signature",
        "src/lib/signing.ts",
        "  const earlier = signed.signatures.filter((s) => !s.ours && !s.archive);",
        "  const earlier = signed.signatures.filter((s) => !s.ours);",
        "closes a signing as the signing panel does",
    ),
    Mutation(
        "archive: leave the archive timestamp out of the closing sentence",
        "src/lib/signing.ts",
        "    text += said.archivePart(listed);",
        "",
        "closes a signing as the signing panel does",
    ),
    Mutation(
        "trust: say trusted at the timestamp without saying when",
        "src/lib/integrity.ts",
        "            `${w.computer} trusts at ${w.attestedAt}, the time a timestamp from an ` +",
        "            `${w.computer} trusts at the time a timestamp from an ` +",
        "says whose moment an attested judgement is, and that expiry since does not undo it",
    ),
    Mutation(
        "trust: hide that an untrusted chain was judged at the attested moment",
        "src/lib/integrity.ts",
        '      const judged = w.attestedAt ? `, judged at ${w.attestedAt}, the time the timestamp attests` : "";',
        '      const judged = w.attestedAt ? "" : "";',
        "says whose moment an attested judgement is, and that expiry since does not undo it",
    ),
    # `tpdf verify`, 2026-09-28: the moment after the reason read as the
    # reason's last clause --- the Adobe sentence's, for a self-made root.
    Mutation(
        "trust: say the attested moment after the reason again",
        "src/lib/integrity.ts",
        "            `not trusted${judged} — ${w.why}. So nothing establishes that the key ` +",
        "            `not trusted — ${w.why}${judged}. So nothing establishes that the key ` +",
        "states the attested moment once, as the whole verdict's and not the reason's",
    ),
    Mutation(
        "authority: hide that an untrusted authority was judged at the archive's moment",
        "src/lib/integrity.ts",
        "          return `not trusted${judged} — ${w.why}. So nothing establishes who attests this time.`;",
        "          return `not trusted — ${w.why}. So nothing establishes who attests this time.`;",
        "states the archive's moment once, beside the verdict's word",
    ),
    Mutation(
        "authority: call an authority out of its dates the signer's certificate",
        "src/lib/integrity.ts",
        "          return `${whose} certificate was not in force then`;",
        "          return \"the signer's certificate was not in force then\";",
        "says an authority out of its dates is the authority's certificate, not the signer's",
    ),
]

# --- the window's signing phase, and what it concludes ----------------------
#
# `signphase.ts` and `saveanswer.ts`, 2026-09-28. The phase itself needs a
# screen, a keychain prompt and the network, so it runs by hand; what it
# concludes from the screen is here, and each rule has to go red on its own.
ROWS += [
    Mutation(
        "sign phase: accept a timestamp from whoever answered",
        "src/lib/signphase.ts",
        "    ok: stamped.includes(\", attested by \") && stamped.includes(authority),",
        "    ok: stamped.includes(\", attested by \"),",
        "fails a timestamp that is not attested, or is another authority's",
    ),
    Mutation(
        "sign phase: call every revocation row not checked when there is none",
        "src/lib/signphase.ts",
        "      revocations.length >= 2 &&",
        "      revocations.length >= 0 &&",
        "fails revocation data the signing did not add, and a revocation row gone missing",
    ),
    Mutation(
        "sign phase: pass a key asked for more often than the step allows",
        "src/lib/signphase.ts",
        "    ok: after - before === expected,",
        "    ok: after - before >= expected,",
        "holds the count to exactly what the step asked for, both ways",
    ),
    Mutation(
        "sign phase: sign with whatever identity was named",
        "src/lib/signphase.ts",
        "  if (!/^[0-9a-f]{64}$/.test(id)) {",
        "  if (id === \"\") {",
        "refuses to run without an identity, or with a name for one",
    ),
    Mutation(
        "sign phase: pass a signer judged now",
        "src/lib/signphase.ts",
        "    ok: /^not trusted, judged at [^—]+, the time the timestamp attests —/.test(trust),",
        "    ok: trust.startsWith(\"not trusted\"),",
        "fails a signer read as trusted, or judged now rather than at the timestamp",
    ),
    Mutation(
        "save answer: answer every later panel with the same path",
        "src/lib/saveanswer.ts",
        "    this.queued = null;\n    return path;",
        "    return path;",
        "answers a queued path once, without the panel, then shows it again",
    ),
    # --- arrangebar.ts, and the line about Shift in the note box --------------
    # The bar beside several picked marks is a second door to the Arrange
    # commands, so most of these are about what it must not decide for itself.
    Mutation("arrange bar: show it with one mark picked", "src/lib/arrangebar.ts", "marks.length >= BETWEEN && !held", "marks.length >= 1 && !held", "appears with two marks picked, and not with one or none"),
    Mutation("arrange bar: leave it on screen when it is hidden", "src/lib/arrangebar.ts", "    this.shown = false;\n    this.element.style.display = \"none\";", "    this.shown = false;", "goes when fewer than two are left, and when it is hidden"),
    Mutation("arrange bar: stay under an open note box", "src/lib/arrangebar.ts", "marks.length >= BETWEEN && !held", "marks.length >= BETWEEN", "steps aside while the note box is open, and comes back when it closes"),
    Mutation("arrange bar: draw a bar with a command missing", "src/lib/arrangebar.ts", "    if (!at || commands.some((command) => command === undefined)) {", "    if (!at) {", "is not there when no command answers, or when one of the ten does not"),
    Mutation("arrange bar: run the first button's command from every button", "src/lib/arrangebar.ts", "      const command = this.opts.command(entry.id);\n      if (command?.enabled) command.run();", "      const command = this.opts.command(\"edit.alignLeft\");\n      if (command?.enabled) command.run();", "each run their own command, once, and no other"),
    Mutation("arrange bar: keep the group's name in a button's label", "src/lib/arrangebar.ts", "  const said = title.replace(/^[^:]*:\\s*/, \"\");", "  const said = title;", "are named by their command's title without its group, as label and as tooltip"),
    Mutation("arrange bar: run a command that is not enabled", "src/lib/arrangebar.ts", "      if (command?.enabled) command.run();", "      command?.run();", "show a command that cannot run as disabled, say why, and do not run it"),
    Mutation("arrange bar: never show a button as disabled", "src/lib/arrangebar.ts", "    button.setAttribute(\"aria-disabled\", String(!command.enabled));", "    button.setAttribute(\"aria-disabled\", \"false\");", "show a command that cannot run as disabled, say why, and do not run it"),
    Mutation("arrange bar: say nothing about why a button is disabled", "src/lib/arrangebar.ts", "    button.title = !command.enabled && command.why ? `${label} ${command.why}` : label;", "    button.title = label;", "show a command that cannot run as disabled, say why, and do not run it"),
    Mutation("arrange bar: say what is missing of a command that can run", "src/lib/arrangebar.ts", "    button.title = !command.enabled && command.why ?", "    button.title = command.why ?", "show a command that cannot run as disabled, say why, and do not run it"),
    Mutation("arrange bar: draw a disabled button at full strength", "src/lib/arrangebar.ts", "    button.style.opacity = command.enabled ? \"1\" : \"0.35\";", "    button.style.opacity = \"1\";", "show a command that cannot run as disabled, say why, and do not run it"),
    Mutation("arrange bar: dress the buttons once and never again", "src/lib/arrangebar.ts", "      if (command) this.dress(one.button, command);", "      if (command && !this.shown) this.dress(one.button, command);", "follow their commands from one frame to the next, and ask again when pressed"),
    Mutation("arrange bar: trust the drawn state when a button is pressed", "src/lib/arrangebar.ts", "      if (command?.enabled) command.run();", "      if (button.getAttribute(\"aria-disabled\") !== \"true\") command?.run();", "follow their commands from one frame to the next, and ask again when pressed"),
    Mutation("arrange bar: let a press on a button through to the page", "src/lib/arrangebar.ts", "      // front of.\n      event.preventDefault();\n      event.stopPropagation();", "      // front of.\n      event.preventDefault();", "keep a press from the page and from the keyboard's focus, on a button and between two"),
    Mutation("arrange bar: let a press on a button take the focus", "src/lib/arrangebar.ts", "      // front of.\n      event.preventDefault();\n      event.stopPropagation();", "      // front of.\n      event.stopPropagation();", "keep a press from the page and from the keyboard's focus, on a button and between two"),
    Mutation("arrange bar: let a press between two buttons through to the page", "src/lib/arrangebar.ts", "    this.element.addEventListener(\"pointerdown\", (event) => {\n      event.preventDefault();\n      event.stopPropagation();\n    });", "    this.element.addEventListener(\"pointerdown\", (event) => {\n      event.preventDefault();\n    });", "keep a press from the page and from the keyboard's focus, on a button and between two"),
    Mutation("arrange bar: let a press between two buttons take the focus", "src/lib/arrangebar.ts", "    this.element.addEventListener(\"pointerdown\", (event) => {\n      event.preventDefault();\n      event.stopPropagation();\n    });", "    this.element.addEventListener(\"pointerdown\", (event) => {\n      event.stopPropagation();\n    });", "keep a press from the page and from the keyboard's focus, on a button and between two"),
    Mutation("arrange bar: put the buttons in the Tab order", "src/lib/arrangebar.ts", "    button.tabIndex = -1;", "    button.tabIndex = 0;", "keep a press from the page and from the keyboard's focus, on a button and between two"),
    Mutation("arrange bar: run on any button of the pointer", "src/lib/arrangebar.ts", "      if (event.button) return;\n", "", "run nothing for a press with another button of the pointer"),
    Mutation("arrange bar: light a button that cannot be pressed", "src/lib/arrangebar.ts", "      if (button.getAttribute(\"aria-disabled\") !== \"true\") button.style.background = HOVER;", "      button.style.background = HOVER;", "light under the pointer when they can be pressed, and not otherwise"),
    Mutation("arrange bar: stay lit when the pointer leaves", "src/lib/arrangebar.ts", "    button.addEventListener(\"pointerleave\", () => {\n      button.style.background = \"none\";\n    });\n", "", "light under the pointer when they can be pressed, and not otherwise"),
    Mutation("arrange bar: never above the marks", "src/lib/arrangebar.ts", "    { left: across, top: box.top - GAP - BAR_HEIGHT },\n", "", "is above the marks and centred on them"),
    Mutation("arrange bar: start at the marks' left edge", "src/lib/arrangebar.ts", "  const across = within((box.left + box.right - BAR_WIDTH) / 2,", "  const across = within(box.left,", "is above the marks and centred on them"),
    Mutation("arrange bar: touch the marks from above", "src/lib/arrangebar.ts", "    { left: across, top: box.top - GAP - BAR_HEIGHT },", "    { left: across, top: box.top - BAR_HEIGHT },", "is above the marks and centred on them"),
    Mutation("arrange bar: never below the marks", "src/lib/arrangebar.ts", "    { left: across, top: box.bottom + GAP },\n", "", "is below them when there is no room above"),
    Mutation("arrange bar: above the marks and off the top of the host", "src/lib/arrangebar.ts", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "    place.left >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "is below them when there is no room above"),
    Mutation("arrange bar: never to the right of the marks", "src/lib/arrangebar.ts", "    { left: box.right + GAP, top: down },\n", "", "is beside them when they fill the height: on the right, or else on the left"),
    Mutation("arrange bar: never to the left of the marks", "src/lib/arrangebar.ts", "    { left: box.left - GAP - BAR_WIDTH, top: down },\n", "", "is beside them when they fill the height: on the right, or else on the left"),
    Mutation("arrange bar: below the marks and off the bottom of the host", "src/lib/arrangebar.ts", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN;", "is beside them when they fill the height: on the right, or else on the left"),
    Mutation("arrange bar: over the marks when no place is clear", "src/lib/arrangebar.ts", "  return places.find(fits) ?? null;", "  return places.find(fits) ?? places[0] ?? null;", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: beside marks that are out of view", "src/lib/arrangebar.ts", "  if (!seen) return null;\n", "", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: beside marks scrolled off to the left", "src/lib/arrangebar.ts", "  const seen = box.right > 0 && box.left < host.width", "  const seen = box.left < host.width", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: beside marks scrolled off to the right", "src/lib/arrangebar.ts", "  const seen = box.right > 0 && box.left < host.width", "  const seen = box.right > 0", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: beside marks scrolled off above", "src/lib/arrangebar.ts", "&& box.bottom > 0 && box.top < host.height;", "&& box.top < host.height;", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: beside marks scrolled off below", "src/lib/arrangebar.ts", "&& box.bottom > 0 && box.top < host.height;", "&& box.bottom > 0;", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: left of the marks and off the left of the host", "src/lib/arrangebar.ts", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "    place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: right of the marks and off the right of the host", "src/lib/arrangebar.ts", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.left + BAR_WIDTH <= host.width - MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "    place.left >= MARGIN\n    && place.top >= MARGIN\n    && place.top + BAR_HEIGHT <= host.height - MARGIN;", "is not there when no place is clear of the marks, or when they are out of view"),
    Mutation("arrange bar: run off the host's side instead of sliding in", "src/lib/arrangebar.ts", "  const across = within((box.left + box.right - BAR_WIDTH) / 2, MARGIN, host.width - BAR_WIDTH - MARGIN);", "  const across = (box.left + box.right - BAR_WIDTH) / 2;", "is held inside the host at each edge"),
    Mutation("arrange bar: slide in from the left only", "src/lib/arrangebar.ts", "  return Math.max(low, Math.min(value, high));", "  return Math.max(low, value);", "is held inside the host at each edge"),
    Mutation("arrange bar: slide in from the right only", "src/lib/arrangebar.ts", "  return Math.max(low, Math.min(value, high));", "  return Math.min(value, high);", "is held inside the host at each edge"),
    Mutation("arrange bar: beside tall marks, level with a middle that is out of view", "src/lib/arrangebar.ts", "  const down = within((box.top + box.bottom - BAR_HEIGHT) / 2, MARGIN, host.height - BAR_HEIGHT - MARGIN);", "  const down = (box.top + box.bottom - BAR_HEIGHT) / 2;", "is held inside the host at each edge"),
    Mutation("arrange bar: stay where it was first placed", "src/lib/arrangebar.ts", "    this.element.style.top = `${Math.round(at.top)}px`;", "    if (!this.shown) this.element.style.top = `${Math.round(at.top)}px`;", "follows the marks when it is placed again"),
    Mutation("arrange bar: read the host's size once", "src/lib/arrangebar.ts", "      ? barSpot({ width: this.host.clientWidth, height: this.host.clientHeight }, around(marks))", "      ? barSpot(this.shown ? { width: 900, height: 700 } : { width: this.host.clientWidth, height: this.host.clientHeight }, around(marks))", "follows the marks when it is placed again"),
    Mutation("arrange bar: run a command past the registry", "src/lib/arrangebar.ts", "    run: () => void registry.run(id),", "    run: () => void registry.find(id)?.run?.(),", "has a registered Arrange command behind every button, and a press runs that arrangement"),
    Mutation("arrange bar: take every registered command for one that can run", "src/lib/arrangebar.ts", "    enabled: command.enabled?.() ?? true,", "    enabled: true,", "greys spacing with two picked and says it needs three, as the command does"),
    Mutation("arrange bar: drop what the command says is missing", "src/lib/arrangebar.ts", "    why: command.why?.() ?? null,", "    why: null,", "greys spacing with two picked and says it needs three, as the command does"),
    Mutation("arrange: say something is missing with just enough picked", "src/lib/arrange.ts", "  if (picked >= want) return null;", "  if (picked > want) return null;", "says what is missing with too few picked, and nothing with enough"),
    Mutation("arrange: say how many are needed as a figure", "src/lib/arrange.ts", '    needs: (count: number) => `needs ${["none", "one", "two", "three"][count] ?? count} picked`,', "    needs: (count: number) => `needs ${count} picked`,", "says what is missing with too few picked, and nothing with enough"),
    Mutation("arrange command: say nothing about what is missing", "src/lib/appcommands.ts", "      why,\n      run: () => actions.arrange(how),", "      run: () => actions.arrange(how),", "offers each arrangement once enough marks are picked for it, and runs that one"),
    Mutation("mark popup: say Shift picks another on every mark", "src/lib/markpopup.ts", "    this.together.hidden = !together;", "    this.together.hidden = false;", "says Shift picks another only when it is told so, and for that opening only"),
    Mutation("mark popup: never say Shift picks another", "src/lib/markpopup.ts", "    this.together.hidden = !together;", "    this.together.hidden = true;", "says Shift picks another only when it is told so, and for that opening only"),
    Mutation("mark popup: build the line about Shift shown", "src/lib/markpopup.ts", "    this.together.hidden = true;\n    this.together.style.cssText", "    this.together.style.cssText", "says Shift picks another only when it is told so, and for that opening only"),
    Mutation("mark popup: build the line about Shift with no words", "src/lib/markpopup.ts", "    this.together.textContent = TOGETHER;", "    this.together.textContent = \"\";", "says Shift picks another only when it is told so, and for that opening only"),
    Mutation("mark popup: hide the line about Shift when asked to show it", "src/lib/markpopup.ts", "    this.together.hidden = !on;", "    this.together.hidden = on;", "shows and hides the line about Shift under an open box"),
    Mutation("arrange bar viewer: never place the bar", "src/lib/viewer.ts", "    this.syncArrangeBar();\n", "", "shows it beside the marks once two are picked, and takes it away at one"),
    Mutation("arrange bar viewer: ask nobody for the commands", "src/lib/viewer.ts", "      command: (id) => this.opts.onArrangeCommand?.(id),", "      command: () => undefined,", "shows it beside the marks once two are picked, and takes it away at one"),
    Mutation("arrange bar viewer: place it round the first picked mark only", "src/lib/viewer.ts", "      if (anchor) at.push(anchor);", "      if (anchor && at.length < 2) at.push(at[0] ?? anchor);", "shows it beside the marks once two are picked, and takes it away at one"),
    Mutation("arrange bar viewer: keep the bar under an open box", "src/lib/viewer.ts", "    this.arrangeBar.sync(at, this.markNote.openId !== null);", "    this.arrangeBar.sync(at);", "steps aside while the box of one of the picked marks is open"),
    Mutation("arrange bar viewer: leave a mark dragged along where it was", "src/lib/viewer.ts", "(live.id === mark.id || live.others.includes(mark.id)) ? live : null;", "live.id === mark.id ? live : null;", "follows marks that are dragged together, the ones that are not under the hand too"),
    Mutation("shift line: not said when the box opens", "src/lib/viewer.ts", "    this.markNote.show(mark, at, focus, this.pickableWith(mark));", "    this.markNote.show(mark, at, focus);", "is shown for a rectangle picked alone on a page that has another"),
    Mutation("shift line: count a mark no press picks", "src/lib/viewer.ts", "other.page === mark.page && isResizable(other.kind),", "other.page === mark.page,", "is not shown when the page has no other rectangle to pick"),
    Mutation("shift line: count a rectangle on another page", "src/lib/viewer.ts", "(other) => other.id !== mark.id && other.page === mark.page && isResizable(other.kind),", "(other) => other.id !== mark.id && isResizable(other.kind),", "is not shown when the page has no other rectangle to pick"),
    Mutation("shift line: count the mark itself as another", "src/lib/viewer.ts", "(other) => other.id !== mark.id && other.page === mark.page", "(other) => other.page === mark.page", "is not shown when the page has no other rectangle to pick"),
    Mutation("shift line: said with several picked", "src/lib/viewer.ts", "    if (this.arranging.count !== 1 || !this.arranging.has(mark.id)) return false;", "    if (!this.arranging.has(mark.id)) return false;", "is not shown with several picked, or with this one not picked"),
    Mutation("shift line: said of a mark that is not the one picked", "src/lib/viewer.ts", "    if (this.arranging.count !== 1 || !this.arranging.has(mark.id)) return false;", "    if (this.arranging.count !== 1) return false;", "is not shown with several picked, or with this one not picked"),
    Mutation("shift line: not said again under an open box", "src/lib/viewer.ts", "    this.markNote.offerTogether(this.pickableWith(mark));\n", "", "goes when it stops being true under the open box, and comes back"),
]

# The blank window lists the recent documents (2026-10-05). Its rows, keys and
# two ways of forgetting are `startpage.ts`; the last entries are the join in
# `App.svelte`, read as source because nothing imports that file.
ROWS += [
    Mutation("start page: cut a folder at a forward slash only", "src/lib/startpage.ts",
        '  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\\\"));',
        '  const cut = path.lastIndexOf("/");',
        "is everything before the last separator, of either kind"),
    Mutation("start page: name the root of a POSIX path as nothing", "src/lib/startpage.ts",
        '  if (folder === "" || /^[A-Za-z]:$/.test(folder)) return path.slice(0, cut + 1);',
        '  if (/^[A-Za-z]:$/.test(folder)) return path.slice(0, cut + 1);',
        "names a root by its separator"),
    Mutation("start page: name a drive's root without its separator", "src/lib/startpage.ts",
        '  if (folder === "" || /^[A-Za-z]:$/.test(folder)) return path.slice(0, cut + 1);',
        '  if (folder === "") return path.slice(0, cut + 1);',
        "names a root by its separator"),
    Mutation("start page: write a tilde for a Windows home", "src/lib/startpage.ts",
        '  if (!home || !home.startsWith("/")) return folder;',
        '  if (!home) return folder;',
        "leaves a Windows folder as written, under a Windows home too"),
    Mutation("start page: shorten a folder that only begins like home", "src/lib/startpage.ts",
        '  return folder.startsWith(`${base}/`) ? `~${folder.slice(base.length)}` : folder;',
        '  return folder.startsWith(base) ? `~${folder.slice(base.length)}` : folder;',
        "leaves a folder that only begins like home"),
    Mutation("start page: shorten every path when home is the root", "src/lib/startpage.ts",
        '  if (base === "") return folder;\n',
        '',
        "does not shorten anything when home is the root"),
    Mutation("start page: leave the home folder itself as written", "src/lib/startpage.ts",
        '  if (folder === base) return "~";\n',
        '',
        "writes the home folder, and what is under it, with a tilde"),
    Mutation("start page: keep a home's trailing separator", "src/lib/startpage.ts",
        '  const base = home.replace(/\\/+$/, "");',
        '  const base = home;',
        "writes the home folder, and what is under it, with a tilde"),
    Mutation("start page: say page 1 of a document left on its first page", "src/lib/startpage.ts",
        '  return place.page > 0 ? TEXT().page(place.page + 1) : "";',
        '  return place.page >= 0 ? TEXT().page(place.page + 1) : "";',
        "says nothing on the first page and counts from one after it"),
    Mutation("start page: count pages from zero", "src/lib/startpage.ts",
        '  return place.page > 0 ? TEXT().page(place.page + 1) : "";',
        '  return place.page > 0 ? TEXT().page(place.page) : "";',
        "says nothing on the first page and counts from one after it"),
    Mutation("start page: list every remembered document", "src/lib/startpage.ts",
        '  const offered = places.slice(0, MAX_RECENTS);',
        '  const offered = [...places];',
        "offers the newest few and no more"),
    Mutation("start page: label a row with its whole path", "src/lib/startpage.ts",
        '  const labels = labelsFor(offered.map((place) => place.path));',
        '  const labels = offered.map((place) => place.path);',
        "is a row per place, in order, with what the page shows"),
    Mutation("start page: show the folder without shortening home", "src/lib/startpage.ts",
        '      folder: tailOf(shortenHome(folderOf(place.path), home)),',
        '      folder: tailOf(folderOf(place.path)),',
        "is a row per place, in order, with what the page shows"),
    Mutation("start page: mark no row", "src/lib/startpage.ts",
        '      trouble: unopened.has(place.path) ? UNOPENED : "",',
        '      trouble: "",',
        "marks the documents that would not open, and only those"),
    Mutation("start page: every command opens the first document", "src/lib/startpage.ts",
        '    run: () => open(row.path),',
        '    run: () => open(rows[0]?.path ?? row.path),',
        "makes the command each row names, and it opens that row's document"),
    Mutation("start page: title a command with something the row does not show", "src/lib/startpage.ts",
        "    title: TEXT().open(row.label),",
        "    title: TEXT().open(row.path),",
        "makes the command each row names, and it opens that row's document"),
    Mutation("start page: answer a chord", "src/lib/startpage.ts",
        '  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return null;\n',
        '',
        "leaves a chord alone"),
    Mutation("start page: any key enters the list", "src/lib/startpage.ts",
        '  if (current < 0) return event.key === "ArrowDown" ? { focus: 0 } : null;',
        '  if (current < 0) return { focus: 0 };',
        "enters the list on the first Down, and on nothing else"),
    Mutation("start page: Down wraps to the first row", "src/lib/startpage.ts",
        '    case "ArrowDown": return { focus: Math.min(at + 1, last) };',
        '    case "ArrowDown": return { focus: (at + 1) % count };',
        "walks the rows and stops at the ends"),
    Mutation("start page: Up leaves the list from the first row", "src/lib/startpage.ts",
        '    case "ArrowUp": return { focus: Math.max(at - 1, 0) };',
        '    case "ArrowUp": return { focus: at - 1 };',
        "walks the rows and stops at the ends"),
    Mutation("start page: Home goes to the last row", "src/lib/startpage.ts",
        '    case "Home": return { focus: 0 };',
        '    case "Home": return { focus: last };',
        "walks the rows and stops at the ends"),
    Mutation("start page: End goes to the first row", "src/lib/startpage.ts",
        '    case "End": return { focus: last };',
        '    case "End": return { focus: 0 };',
        "walks the rows and stops at the ends"),
    Mutation("start page: Delete removes nothing", "src/lib/startpage.ts",
        '    case "Delete":\n',
        '',
        "removes the focused row on Delete and on Backspace"),
    Mutation("start page: Backspace removes the first row", "src/lib/startpage.ts",
        '    case "Backspace": return { remove: at };',
        '    case "Backspace": return { remove: 0 };',
        "removes the focused row on Delete and on Backspace"),
    Mutation("start page: enter a list with no rows", "src/lib/startpage.ts",
        '  if (count <= 0) return null;\n',
        '',
        "has nowhere to go in an empty list"),
    Mutation("start page: walk from a row that has gone", "src/lib/startpage.ts",
        '  const at = Math.min(current, last);',
        '  const at = current;',
        "treats a row that has since gone as the last one"),
    Mutation("start page: focus past the end after removing the last row", "src/lib/startpage.ts",
        '  return Math.min(index, remaining - 1);',
        '  return index;',
        "is the row that took the place, or the one above the last"),
    Mutation("start page: change the list before the places have landed", "src/lib/startpage.ts",
        '  await writer.settled();\n',
        '',
        "changes the list only after every place already issued has landed"),
    Mutation("start page: leave a pending place to land after the change", "src/lib/startpage.ts",
        '  writer.flush();\n',
        '',
        "changes the list only after every place already issued has landed"),
    Mutation("start page: swallow a refused change", "src/lib/startpage.ts",
        '    return spokenError(e);',
        '    return null;',
        "hands back what a refused change said"),
    Mutation("start page: forget where home is", "src/lib/startpage.ts",
        '    this.#home = session.home ?? null;',
        '    this.#home = null;',
        "tells its rows when it is given a session"),
    Mutation("start page: keep a mark for a row that has gone", "src/lib/startpage.ts",
        '      if (!listed.has(path)) this.#unopened.delete(path);',
        '      if (listed.size === 0) this.#unopened.delete(path);',
        "drops the mark with the row, so a document remembered again starts clean"),
    Mutation("start page: drop every mark when the list is re-read", "src/lib/startpage.ts",
        '      if (!listed.has(path)) this.#unopened.delete(path);',
        '      this.#unopened.delete(path);',
        "keeps a mark across a re-read of the same list"),
    Mutation("start page: mark a document whose password was declined", "src/lib/startpage.ts",
        '    if (isOpenRefusal(why) && why.locked) return;\n',
        '',
        "does not mark a document whose password was declined"),
    Mutation("start page: mark a row without telling the page", "src/lib/startpage.ts",
        '    this.#unopened.add(path);\n    this.#tell();',
        '    this.#unopened.add(path);',
        "marks a document that would not open, and unmarks it when it does"),
    Mutation("start page: keep the mark of a document that opened", "src/lib/startpage.ts",
        '    if (this.#unopened.delete(path)) this.#tell();',
        '    if (this.#unopened.has(path)) this.#tell();',
        "marks a document that would not open, and unmarks it when it does"),
    Mutation("start page: tell the page on every open", "src/lib/startpage.ts",
        '    if (this.#unopened.delete(path)) this.#tell();',
        '    this.#unopened.delete(path);\n    this.#tell();',
        "marks a document that would not open, and unmarks it when it does"),
    Mutation("session writer: suppress a place the store has forgotten", "src/lib/session.ts",
        '  forgotten(): void {\n    this.written = null;',
        '  forgotten(): void {',
        "writes the same place again once told the store has forgotten it"),
    Mutation("clear recents: offered with nothing to clear", "src/lib/appcommands.ts",
        '      enabled: () => actions.recentDocuments() > 0,',
        '      enabled: () => true,',
        "is offered while there are some, with or without a document"),
    Mutation("clear recents: withheld under a busy document", "src/lib/appcommands.ts",
        '      enabled: () => actions.recentDocuments() > 0,',
        '      enabled: () => actions.recentDocuments() > 0 && available(),',
        "is not withheld while a document is busy"),
    Mutation("start page shell: build the palette's commands from a second list", "src/App.svelte",
        '      recentCommands(startPage.offer(from), (path) => void openPath(path)),',
        '      recentCommands(rowsFor(from.places, from.home), (path) => void openPath(path)),',
        "builds the palette's commands from the rows the page shows"),
    Mutation("start page shell: open a row's document around the registry", "src/App.svelte",
        '                onclick={() => runStartCommand(row.command)}>',
        '                onclick={() => void openPath(row.path)}>',
        "runs a row's command, and the button's, through the registry"),
    Mutation("start page shell: never mark a row whose document would not open", "src/App.svelte",
        '      startPage.failed(path, e);\n',
        '',
        "tells the page when an open fails and when one succeeds"),
    Mutation("start page shell: keep the mark of a document that opened", "src/App.svelte",
        '      startPage.opened(path);\n',
        '',
        "tells the page when an open fails and when one succeeds"),
    Mutation("start page shell: clear without re-noting the document on screen", "src/App.svelte",
        '    places.forgotten();\n    notePlace();\n    await refreshStartPage();',
        '    await refreshStartPage();',
        "forgets behind the places already issued, and re-reads the list after"),
    Mutation("start page shell: take Down from whatever has the focus", "src/App.svelte",
        '    if (!title && (focused === null || focused === document.body) && startPageKey(event, -1)) return;',
        '    if (!title && startPageKey(event, -1)) return;',
        "enters the list from the window only when nothing has the focus"),
]

# What the page draws while a note is typed (2026-10-05): a draft of a text
# box's words and of a field's name, which is not an edit. `markdraft.ts` holds
# every decision; the box reports keystrokes, the viewer draws, `App.svelte`
# joins the wrap command and hands the edit's promise back.
ROWS += [
    Mutation("live draft: every kind's note gets a draft", "src/lib/markdraft.ts",
        '  if (kind === "field") return "name";\n  return null;',
        '  if (kind === "field") return "name";\n  return "name";',
        "is the two whose note the page draws, and each in its own way"),
    Mutation("live draft: keep a draft for a note the page does not draw", "src/lib/markdraft.ts",
        '    if (how === null) return;\n',
        '',
        "keeps nothing and asks nothing for a kind whose note is not on the page"),
    Mutation("live draft: a field's name has no draft", "src/lib/markdraft.ts",
        '  if (kind === "field") return "name";\n',
        '',
        "is the name as typed, at once and with nothing asked"),
    Mutation("live draft: ask for a field's name to be wrapped", "src/lib/markdraft.ts",
        '    if (how === "lines") this.ask(draft, mark);',
        '    this.ask(draft, mark);',
        "is the name as typed, at once and with nothing asked"),
    Mutation("live draft: ask for a resized field's name to be wrapped", "src/lib/markdraft.ts",
        '    if (!draft || draft.mark !== mark.id || drafted(mark.kind) !== "lines") return;',
        '    if (!draft || draft.mark !== mark.id) return;',
        "is the name as typed, at once and with nothing asked"),
    Mutation("live draft: wrap for the wrong edges", "src/lib/markdraft.ts",
        'right: mark.quads[2] ?? 0 };',
        'right: mark.quads[3] ?? 0 };',
        "asks for the lines of what was typed, for the box's own edges"),
    Mutation("live draft: take the answer to an older question", "src/lib/markdraft.ts",
        '        if (draft.asked !== asked || this.of(draft.mark) !== draft) return;',
        '        if (this.of(draft.mark) !== draft) return;',
        "takes only the answer to the newest question, whatever order they arrive in"),
    Mutation("live draft: redraw for an answer to a draft that is gone", "src/lib/markdraft.ts",
        '        if (draft.asked !== asked || this.of(draft.mark) !== draft) return;',
        '        if (draft.asked !== asked) return;',
        "drops an answer that arrives after the box closed"),
    Mutation("live draft: forget the last lines on every keystroke", "src/lib/markdraft.ts",
        '    const draft: Draft = this.live?.mark === mark.id\n',
        '    const draft: Draft = this.live?.mark === -1\n',
        "keeps the last lines it drew while the next are asked for"),
    Mutation("live draft: fall back to the model's lines when a wrap fails", "src/lib/markdraft.ts",
        '      () => {},\n    );',
        '      () => {\n        draft.lines = null;\n      },\n    );',
        "keeps what it showed when the wrap fails, and goes on with the next"),
    Mutation("live draft: keep drawing what was typed after the box closes", "src/lib/markdraft.ts",
        '    if (!this.live || this.live.mark === mark) return;\n    this.live = null;',
        '    if (!this.live || this.live.mark === mark) return;',
        "is dropped when the box closes, and when another mark takes it"),
    Mutation("live draft: drop the draft when the box reports the mark it is on", "src/lib/markdraft.ts",
        '    if (!this.live || this.live.mark === mark) return;',
        '    if (!this.live) return;',
        "is kept when the box reports the mark it is already on"),
    Mutation("live draft: do not ask again when the box is resized", "src/lib/markdraft.ts",
        '=== edges(mark).right) return;\n    this.ask(draft, mark);',
        '=== edges(mark).right) return;',
        "asks again when the box is resized under it, and not when it is only sent again"),
    Mutation("live draft: ask again whenever the mark is sent again", "src/lib/markdraft.ts",
        '    if (draft.left === edges(mark).left && draft.right === edges(mark).right) return;\n',
        '',
        "asks again when the box is resized under it, and not when it is only sent again"),
    Mutation("live draft: ask again for whichever mark was sent", "src/lib/markdraft.ts",
        '    if (!draft || draft.mark !== mark.id || drafted(mark.kind) !== "lines") return;',
        '    if (!draft || drafted(mark.kind) !== "lines") return;',
        "asks again when the box is resized under it, and not when it is only sent again"),
    Mutation("live draft: drop the draft the moment its note is sent", "src/lib/markdraft.ts",
        '    this.held.set(mark, draft);\n',
        '',
        "keeps the draft drawn until the edit settles, so the old words are never drawn between"),
    Mutation("live draft: hold a sent draft for ever", "src/lib/markdraft.ts",
        '    Promise.resolve(sent).then(done, done);',
        '    Promise.resolve(sent).then(() => {}, done);',
        "keeps the draft drawn until the edit settles, so the old words are never drawn between"),
    Mutation("live draft: hold a draft whose edit was refused", "src/lib/markdraft.ts",
        '    Promise.resolve(sent).then(done, done);',
        '    Promise.resolve(sent).then(done, () => {});',
        "lets go when the edit is refused, and the page shows what the model has"),
    Mutation("live draft: hold a draft that is not what was sent", "src/lib/markdraft.ts",
        '    if (draft.text !== note) {\n      this.deps.changed();\n      return;\n    }\n',
        '',
        "does not hold a draft whose text is not what was sent"),
    Mutation("live draft: hold a draft for a mark that had none", "src/lib/markdraft.ts",
        '    if (!draft || draft.mark !== mark) return;\n    this.live = null;',
        '    if (!draft) return;\n    this.live = null;',
        "holds nothing for a mark other than the one being typed for"),
    Mutation("live draft: a first edit settling drops the second's draft", "src/lib/markdraft.ts",
        '      if (this.held.get(mark) !== draft) return;\n',
        '',
        "draws what is typed next over what is held, and a first edit settling does not drop a second"),
    Mutation("live draft: draw a held draft over what is being typed", "src/lib/markdraft.ts",
        '    return this.live?.mark === mark ? this.live : this.held.get(mark);',
        '    return this.held.get(mark) ?? (this.live?.mark === mark ? this.live : undefined);',
        "draws what is typed next over what is held, and a first edit settling does not drop a second"),
    Mutation("note box: report no keystroke", "src/lib/markpopup.ts",
        '      if (this.shown !== null) this.opts.onDraft(this.shown, this.input.value);',
        '',
        "reports what the box holds on every keystroke, and commits nothing by it"),
    Mutation("note box: report a keystroke in a closed box", "src/lib/markpopup.ts",
        '      if (this.shown !== null) this.opts.onDraft(this.shown, this.input.value);',
        '      this.opts.onDraft(this.shown ?? 0, this.input.value);',
        "reports nothing from a box that is not open on a mark"),
    Mutation("note box: leave a draft on the page when the box is filled again", "src/lib/markpopup.ts",
        '    if (refilled) this.opts.onDraft(mark.id, mark.note);\n',
        '',
        "reports the note again when the box is filled again on the mark it is on"),
    Mutation("note box: report a draft whenever another mark takes the box", "src/lib/markpopup.ts",
        '    const refilled = was === mark.id && this.input.value !== mark.note;',
        '    const refilled = this.input.value !== mark.note;',
        "reports the note again when the box is filled again on the mark it is on"),
    Mutation("note box: report a draft on every show of the open mark", "src/lib/markpopup.ts",
        '    const refilled = was === mark.id && this.input.value !== mark.note;',
        '    const refilled = was === mark.id;',
        "reports the note again when the box is filled again on the mark it is on"),
    Mutation("draft on the page: draw the model's lines over a text box's draft", "src/lib/viewer.ts",
        '          this.drafts.linesOf(mark).forEach((line, index) => {',
        '          mark.lines.forEach((line, index) => {',
        "draws a text box's words as they are typed, in the lines it was answered with"),
    Mutation("draft on the page: ask nobody for a text box's lines", "src/lib/viewer.ts",
        '    wrap: (note, left, right) => this.opts.onMarkDraft?.(note, left, right),',
        '    wrap: () => undefined,',
        "draws a text box's words as they are typed, in the lines it was answered with"),
    Mutation("draft on the page: draw the model's name over a field's draft", "src/lib/viewer.ts",
        '            ctx.fillText(this.drafts.noteOf(mark), left + 3 * dpr, top + height / 2);',
        '            ctx.fillText(mark.note, left + 3 * dpr, top + height / 2);',
        "draws a field's name as it is typed, and asks nobody"),
    Mutation("draft on the page: the box's keystrokes reach no draft", "src/lib/viewer.ts",
        '        if (mark) this.drafts.typed(mark, note);\n',
        '        void mark;\n',
        "draws a field's name as it is typed, and asks nobody"),
    Mutation("draft on the page: a closed box leaves its draft drawn", "src/lib/viewer.ts",
        '        this.drafts.opened(mark);\n',
        '',
        "drops the draft when the mark is removed under the box"),
    Mutation("draft on the page: draw the old words between the close and the model's answer", "src/lib/viewer.ts",
        '        this.drafts.committed(mark, note, this.opts.onMarkNote?.(mark, note)),',
        '        void this.opts.onMarkNote?.(mark, note),',
        "on Escape sends the note and keeps drawing it until the model has answered"),
    Mutation("draft on the page: a resized text box keeps the lines of its old width", "src/lib/viewer.ts",
        '    if (open) this.drafts.refit(open);\n',
        '',
        "asks again for a text box resized under its open box"),
    Mutation("draft wiring: wrap between the wrong edges", "src/App.svelte",
        'call("annot_draft_lines", { note, left, right }),',
        'call("annot_draft_lines", { note, left: right, right: left }),',
        "asks the command that wraps as the model does, with the box's edges"),
    Mutation("draft wiring: do not say when a mark's note has been answered", "src/App.svelte",
        '        if (!isSaved(mark)) return applyEdit((e) => e.renote(mark, note));',
        '          if (!isSaved(mark)) return void applyEdit((e) => e.renote(mark, note));',
        "hands the edit's promise back for a note, so a draft is held until the model answers"),
    Mutation("draft wiring: do not say when a saved field's name has been answered", "src/App.svelte",
        '        return to ? applyEdit((e) => e.refield([to])) : undefined;',
        '          if (to) void applyEdit((e) => e.refield([to]));',
        "hands the edit's promise back for a note, so a draft is held until the model answers"),
    Mutation("ocr language: take a blank answer as automatic", "src/lib/ocrlanguage.ts",
        '  if (typed === "") {\n    return { problem:',
        '  if (typed === "") {\n    if (offered) return { language: null };\n    return { problem:',
        "asks for an answer rather than taking a blank one"),
    Mutation("ocr language: compare what was typed by case", "src/lib/ocrlanguage.ts",
        "  const typed = raw.trim().toLowerCase();",
        "  const typed = raw.trim();",
        "takes a tag in any case, and in full before anything that begins the same"),
    Mutation("ocr language: let a beginning win over a whole tag", "src/lib/ocrlanguage.ts",
        "  if (whole) return { language: whole.language };",
        "  if (whole && !whole) return { language: whole.language };",
        "takes a tag in any case, and in full before anything that begins the same"),
    Mutation("ocr language: take the first of several", "src/lib/ocrlanguage.ts",
        "  if (only && begun.length === 1) return { language: only.language };",
        "  if (only) return { language: only.language };",
        "refuses a beginning that several share, and names them"),
    Mutation("ocr language: match by tag alone", "src/lib/ocrlanguage.ts",
        "      words: [tag.toLowerCase(), nameOf(tag).toLowerCase()],",
        "      words: [tag.toLowerCase()],",
        "takes a name, and the beginning of one tag or name"),
    Mutation("ocr language: do not offer automatic", "src/lib/ocrlanguage.ts",
        "    { language: null, words: [AUTOMATIC.toLowerCase()] },\n",
        "",
        "takes automatic as the recogniser's own choice"),
    Mutation("ocr language: tell a Mac reader about Windows Settings", "src/lib/ocrlanguage.ts",
        "  return offered.installable ? `${said} ${INSTALL_MORE}` : said;",
        "  return `${said} ${INSTALL_MORE}`;",
        "says where more are installed only where they can be"),
    Mutation("ocr language: never say where more are installed", "src/lib/ocrlanguage.ts",
        "  return offered.installable ? `${said} ${INSTALL_MORE}` : said;",
        "  return said;",
        "says where more are installed only where they can be"),
    Mutation("ocr language: list every tag in the palette", "src/lib/ocrlanguage.ts",
        "  const SHOWN = 12;\n  const rest = tags.length - SHOWN;",
        "  const SHOWN = 1200;\n  const rest = tags.length - SHOWN;",
        "shortens a long list"),
    Mutation("ocr language: let a tag that cannot be named throw", "src/lib/ocrlanguage.ts",
        "  } catch {\n    return tag;\n  }\n}",
        "  } catch (error) {\n    throw error;\n  }\n}",
        "is the tag alone for one that cannot be named"),
    Mutation("ocr language: show a language without its tag", "src/lib/ocrlanguage.ts",
        "  return name === language ? language : `${name}, ${language}`;",
        "  return name;",
        "is the language and its region in English, beside the tag"),
    Mutation("ocr language: keep a session value that is not a string", "src/lib/ocrlanguage.ts",
        '    this.language = typeof stored === "string" && stored !== "" ? stored : null;',
        "    this.language = (stored ?? null) as string | null;",
        "starts with the recogniser's own choice, and takes what the session held"),
    Mutation("ocr language: keep the list after an answer", "src/lib/ocrlanguage.ts",
        "    const offered = this.asking;\n    this.asking = null;",
        "    const offered = this.asking;",
        "takes an answer once, says it, and stops asking"),
    Mutation("ocr language: take an answer that names nothing as automatic", "src/lib/ocrlanguage.ts",
        "    if (picked.problem !== undefined) return null;\n    this.language = picked.language;",
        "    this.language = picked.problem !== undefined ? null : picked.language;\n    if (picked.problem !== undefined) return null;",
        "keeps the choice when the answer names nothing"),
    Mutation("ocr language: do not say a language was missing", "src/lib/recognise.ts",
        "  if (read.languageUnavailable) said.push(fellBack(read.languageUnavailable));\n",
        "",
        "says which language was missing when the recogniser had to choose"),
    Mutation("ocr language: offer the question with no list held", "src/lib/appcommands.ts",
        "      enabled: () => actions.recognitionLanguages() !== null,",
        "      enabled: () => true,",
        "offers the question only while a list of languages is held"),
    Mutation("ocr language: take an answer with no list to hold it against", "src/lib/appcommands.ts",
        "          if (!asked) return TEXT(locale).noLanguageList;",
        "          if (!asked) return null;",
        "offers the question only while a list of languages is held"),
    Mutation("ocr language: need a document to choose a language", "src/lib/appcommands.ts",
        '      title: message("recogniseLanguage", locale),\n      run: () => actions.chooseRecognitionLanguage(),',
        '      title: message("recogniseLanguage", locale),\n      enabled: withDocument,\n      run: () => actions.chooseRecognitionLanguage(),',
        "is asked for with or without a document, and promises a question"),
    Mutation("ocr language: keep the list when the question is dismissed", "src/lib/appcommands.ts",
        "        dismissed: () => actions.dropRecognitionLanguages(),",
        "        dismissed: () => {},",
        "drops the list when the question is dismissed"),
    Mutation("ocr language: show the placeholder without the present choice", "src/lib/appcommands.ts",
        "          return languagePlaceholder(actions.recognitionLanguages()?.current ?? null);",
        "          return languagePlaceholder(null);",
        "shows the language chosen now, and what an answer will do"),
    Mutation("ocr language wiring: recognise in the engine's choice whatever was chosen", "src/App.svelte",
        "          await edits.ocrCopy(source, chosen, recognitionRun, recognitionLanguage.language),",
        "          await edits.ocrCopy(source, chosen, recognitionRun, null),",
        "recognises text in the language chosen"),
    Mutation("ocr language wiring: forget the choice at launch", "src/App.svelte",
        "      recognitionLanguage.restore(session.ocr_language);\n",
        "",
        "takes the choice from the session at launch, and has the session keep a new one"),
    Mutation("ocr language: ask before the list is here", "src/lib/ocrlanguage.ts",
        '    try {\n      this.hold(await fetch());\n    } catch (error) {\n      say(String(error));\n      return;\n    }\n    ask();',
        '    ask();\n    try {\n      this.hold(await fetch());\n    } catch (error) {\n      say(String(error));\n      return;\n    }',
        "fetches the machine's list and then asks the question about it"),
    Mutation("ocr language: ask about a list that could not be fetched", "src/lib/ocrlanguage.ts",
        '      say(String(error));\n      return;\n    }\n    ask();',
        '      say(String(error));\n    }\n    ask();',
        "says why the list could not be fetched, and asks nothing"),
    Mutation("ocr language: say nothing when the list cannot be fetched", "src/lib/ocrlanguage.ts",
        '      say(String(error));\n      return;\n    }\n    ask();',
        '      return;\n    }\n    ask();',
        "says why the list could not be fetched, and asks nothing"),
    Mutation("ocr language wiring: ask around the module", "src/App.svelte",
        '    await recognitionLanguage.choose(\n',
        '    palette?.askFor("file.recogniseTextLanguage.choice");\n    await ((...parts: unknown[]) => parts)( // recognitionLanguage.choose(\n',
        "asks through the module, which fetches the list first"),
    Mutation("ocr language: drop the language on the way to the backend", "src/lib/edits.ts",
        'return await call("ocr_copy", { doc: this.doc, source, path, run, language });',
        'return await call("ocr_copy", { doc: this.doc, source, path, run, language: null });',
        "sends the chosen language with the recognition"),
    Mutation("ocr refused: say nothing about a refused page", "src/lib/recognise.ts",
        "  if (read.refused.length > 0) {",
        "  if (read.refused.length > 9000) {",
        "names the pages the recogniser refused, says what that usually means, and counts them"),
    Mutation("ocr refused: leave refused pages out of the count", "src/lib/recognise.ts",
        "    read.nothingRead.length +\n    read.refused.length +\n",
        "    read.nothingRead.length +\n",
        "names the pages the recogniser refused, says what that usually means, and counts them"),
    Mutation("ocr refused: name the pages with no reason", "src/lib/recognise.ts",
        "    refused: (pages: string) => `The recognizer refused ${pages}, ${REFUSED_MEANS}.`,",
        "    refused: (pages: string) => `The recognizer refused ${pages}.`,",
        "names the pages the recogniser refused, says what that usually means, and counts them"),
    Mutation("ocr refused: say what a refusal means in other words", "src/lib/recognise.ts",
        '  "which usually means a script it cannot read or a scan too unclear to tell the script";',
        '  "which means a script it cannot read";',
        "says what a refused page usually is in the backend's own words"),
]
