# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: Phase 0 closed, Phase 1 in progress, Phase 2 begun. First release: `26.8.0`.**
The feasibility spikes are done and every load-bearing assumption has a measured verdict;
on top of that evidence there is a viewer you can read a PDF in, on macOS arm64 and on
Windows, including documents behind a password. **It edits**: pages can be turned, moved,
deleted, cropped and extracted; text can be highlighted, underlined, struck out or
squiggled; and you can draw on a page, put a box, an ellipse, a text box, a stamp or a
comment on it, move what you put there, erase any of it, and save --- over the open file or
to a copy. **It redacts**: mark regions, review them in a list, and remove the words from
the page's own instructions --- over the open file or to a copy --- with the result read
back and reported either way. What is *not* built is the list further down, and in-place
text editing is the one that matters.
Installers are on the [Releases](https://github.com/tstone-1/tpdf/releases) page:
macOS is signed with a Developer ID identity and notarized, Windows is unsigned and
SmartScreen will warn on first launch. See [`docs/PLAN.md`](docs/PLAN.md) for the
architecture and roadmap, [`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) for the security
position, [`BUILD.md`](BUILD.md) to build it yourself, and [`AGENTS.md`](AGENTS.md) for
project conventions.

## What the viewer does today

- Every document is parsed and rendered in **sandboxed worker processes** with no
  filesystem or network authority --- a pool per document, and a worker that dies is
  replaced and its request retried.
- Tiled rendering behind a virtual scroller; zoom --- in, out, actual size, fit-width,
  fit-page, or a figure you type; view rotation; and page inversion for reading on a dark
  screen.
  <!-- built: view.zoomIn view.zoomOut view.zoomTo view.actualSize view.fitWidth view.fitPage view.rotateClockwise view.rotateCounterClockwise view.invertPages -->
- Text selection and copy; find-in-document, with case, whole-word, regular-expression and
  within-the-selection options; a sidebar carrying the outline, a page-thumbnail strip and
  your own marks; and a text layer for screen readers.
  <!-- built: edit.selectAll edit.copy find.open find.next find.previous find.matchCase find.wholeWord find.regex find.inSelection view.toggleSidebar view.showOutline view.showThumbnails view.showMarks -->
- **Links are followable**, and so is the way back: Back and Forward walk the jumps you have
  made, and Next link / Previous link reach one without the pointer. Back and Forward grey out
  when there is nowhere to go.
  <!-- built: nav.back nav.forward nav.nextLink nav.previousLink -->
- Session restore: the document, page, zoom and rotation you left on.
- **A document behind a password opens**: tpdf asks for one and retries, and holds it for
  as long as the document is open, because every worker that renders it meets the same
  encryption.
- **What a document says about itself**: its title and producer, whether it is encrypted
  and what that permits, what conformance it claims, and who signed it --- the signer's
  certificate, its issuer and its validity, read out of the signature itself. Reading only:
  there is no trust store here, no chain is built, and nothing shown to you has been
  verified.
  <!-- built: file.properties -->
- Printing through the system print panel, on both platforms — and every print job is read
  back through the operating system's own PDF parser before the panel opens, which is a
  parser independent of the one that wrote the job and the one that drew what you saw.
  macOS prints vectors; Windows has no in-box "print this PDF" API at any layer, so it
  rasterises at 300 dpi like every other Windows PDF viewer does.
  <!-- built: file.print -->
- Every command reachable from the command palette, which renders each shortcut from the
  same table the key handler matches against, so a label cannot advertise a chord that
  does nothing.

All of that has run on macOS arm64 and on Windows. Every *measurement* quoted in this
repository is macOS arm64 unless it says otherwise: the two platforms differ enough that
carrying a number across is a guess rather than an estimate, and where both have been
measured the Windows render constants come out 1.5–1.8x worse.

## What it edits today

- **Turn a page in the document**, not only in the view --- with undo and redo, and a
  history that survives any number of turns because it is replayed rather than reversed.
  <!-- built: edit.rotatePageClockwise edit.rotatePageCounterClockwise edit.undo edit.redo -->
- **Delete a page**, from the command palette. Undo puts it back where it was, with its own
  rotation. It has no keyboard shortcut on purpose: it is the one command that removes
  something you can see.
  <!-- built: edit.deletePage -->
- **Move a page** by dragging its thumbnail in the page strip, or one slot at a time from
  the palette. A moved page takes its size, its crop and its rotation with it even where the
  file states none of them on the page itself --- a PDF lets a page inherit those from the
  group it sits in, and that is where moving one silently changes it.
  <!-- built: edit.movePageUp edit.movePageDown -->
- **Print what you edited.** A print job carries the pages that are left, the order they
  are in and the way each one is turned, read from the document model rather than from the
  file on disk.
- **Mark a selection** --- highlight, underline, strike out or squiggle --- as a real PDF
  annotation, not a rectangle drawn over the page, so Acrobat and Preview show it as what
  it is. Each mark takes a note, and **Next mark** / **Previous mark** walk them from the
  keyboard: the pointer is not the only way to reach one.
  <!-- built: edit.highlightSelection edit.underlineSelection edit.strikeoutSelection edit.squigglySelection nav.nextMark nav.previousMark -->
- **Draw on a page** --- freehand ink, a box, an ellipse, a text box, or a comment placed
  where you press. Each is a real annotation of its own kind rather than ink pretending to
  be one, so another reader gets a comment they can open and a shape they can select. What
  you have drawn can be dragged to somewhere else on its page afterwards.
  <!-- built: edit.draw edit.drawBox edit.drawEllipse edit.addTextBox edit.addComment -->
- **Choose a colour** for a mark --- seven of them, the default among them. Chosen with a
  note open it recolours that mark; chosen with none open it sets what the next one will
  be, which is the commoner of the two and is why it is offered either way.
  <!-- built: edit.color.default edit.color.yellow edit.color.green edit.color.blue edit.color.pink edit.color.orange edit.color.red -->
- **Stamp a document** APPROVED, CONFIDENTIAL, DRAFT or FINAL, dragged out like a box. The
  word is set to fill the rectangle you dragged, and it is written as a real `/Stamp`
  annotation carrying the standard name as well as the picture --- so another reader gets a
  stamp rather than a drawing that looks like one.
  <!-- built: edit.stamp.approved edit.stamp.confidential edit.stamp.draft edit.stamp.final -->
- **Erase what you marked** by dragging across it. The nib takes strokes out of a drawing
  and leaves the rest of it; every other kind has no parts to lose, so it goes whole ---
  which is the only way to take a highlight off without opening its note first. It reaches
  your own marks and nothing else: a comment the file arrived with is never touched. A
  mark whose note you have opened can be removed from the note box instead, which is how
  you take off the one you have named rather than the ones you cross.
  <!-- built: edit.erase edit.removeMark -->
- **Crop a page**, either to what is on it or to a rectangle you drag out. Cropping to
  content measures where the ink actually is rather than reading the page's objects, so it
  works on a scan --- where every object union is the whole sheet --- as well as on a page of
  type. Dragging is for the cases a measurement cannot answer: a figure out of a plate, one
  column of two, a scan with a hand in the corner. While you drag, what falls outside the
  rectangle is darkened, so what stays bright is what the page becomes. The crop is part of the document: undoable, carried when the page moves,
  and written into a saved copy as a real `/CropBox`, so another reader opens the file
  cropped the way you left it.
  <!-- built: edit.cropToDrag edit.cropToContent edit.resetCrop -->
- **Mark a region for redaction** --- and nothing more than mark it. Drag out a region and
  it joins a list, drawn in red over the page with the words still readable underneath,
  because a region you cannot see through is one you cannot check. Undo takes it back off.
  **Nothing is removed, and no file is written**: the step that destroys content is not
  built, which is why *True redaction* is still in the list below. A tpdf that could mark
  and not remove must not look like one that removes, so a pending region is never black,
  never saved, and never written into a copy --- see the two entries under *What Phase 0
  established* for why this is the hardest thing here to get right.
  <!-- built: edit.redactRegion -->
- **Review what you marked**, in a sidebar tab that lists every pending region down the
  document with the words under it, so you can check six regions across forty pages
  without scrolling to each one. A region covering no text says so, which is worth
  knowing: it means a removal would take nothing out of that rectangle. The panel is
  where a region comes off again --- undo is chronological, and the second of six you
  drew is not reachable that way. It says what every row here has in common: nothing has
  been removed yet.
  <!-- built: view.showRedactions -->
- **Redact and save as** writes a new file with the marked regions' text removed from the
  page's instructions --- not covered over, removed --- and then reads that file back and
  tells you what it found. It says *verified*, or it says it could not prove the file is
  clean and why. It never says nothing. The document you have open is untouched, so if you
  do not like the result you still have your marks. It removes **text**: a region covering a
  picture or a drawing leaves that where it is, and says so both in the panel before you
  commit and in the report afterwards, because a file with the words gone and a picture of
  the words still in it is worse than no redaction at all. It also takes whole lines ---
  removing part of one means removing the instruction that drew it, so a word beside the one
  you marked goes with it. On a document tagged for accessibility it takes the second copy
  of those words that the tag keeps beside them --- both where it sits beside the words and
  where the document files it separately under the accessibility structure --- and where that
  copy is shared between pages it refuses rather than change the others. It also takes any
  comment sitting on the words --- with its replies --- and leaves the ones elsewhere on the
  page alone, and it takes the document's own title, author and other properties, because a
  title that paraphrases what you removed matches no search for it. And it takes the bookmarks
  that name what went, with whatever hangs under them, leaving the rest of your table of
  contents where it is --- a bookmark's title is the heading it points at, so redacting the
  heading and keeping the bookmark puts the words back on screen in tpdf's own sidebar.
  It takes the **form answers** that went with it, because a field keeps its answer in the
  document rather than on the page --- a field whose widgets have all gone, or whose answer
  is text that went, wherever the widget for it sits; a field naming somebody else's answer
  stays. An **XFA form is refused rather than half redacted**: those keep a complete second
  copy of every answer in a separate packet, so removing the fields would leave everything
  recoverable while telling you it had gone.
  <!-- built: file.redactCopy -->
- **Redact and save** does all of that to the file you opened, rather than to a new one.
  It warns first and offers to save you a copy, because there is no undo across it and no
  original left afterwards: the document is closed by the write, reopened from disk, and
  the marks go with it. The report is the same one, and it arrives after the file is
  already the redacted one --- which is the reason for the warning rather than an argument
  against it. Reach for *Redact and save as* while you are still deciding.
  <!-- built: file.redactDocument -->
- **Extract pages to a second file**, naming a range the way you would say it out loud.
  It reads the document and writes elsewhere, so there is nothing to undo and the open
  file is untouched. It refuses a reversed range rather than quietly correcting it.
  <!-- built: file.extractPages -->
- **Split a document into several files**, naming the pages to cut after: `3,7` on a
  ten-page document writes three files of 3, 4 and 3 pages. You choose one name and get
  numbered siblings --- `report-1.pdf`, `report-2.pdf` --- and the name you chose is not
  one of them. It refuses before writing anything if a file it would write is already
  there, because those are names you never saw a dialog for.
  <!-- built: file.splitDocument -->
- **Merge documents**: pick any number of PDFs and get one file holding this document
  followed by all of them. The open document goes in as you have it --- edited, marked up,
  with deleted pages gone --- and the others go in as they are on disk. Each incoming page
  takes its own size, box and rotation with it even where the file states those on the
  group the page sits in rather than on the page, which is where a naive merge silently
  resizes half a document. Links inside a merged document keep working; its bookmarks and
  named destinations do not come across, and neither do form fields. Like extract, it
  writes elsewhere and changes nothing about what you have open.
  <!-- built: file.mergeDocuments -->
- **Save**, over the open file or to a copy. A save is refused outright if the file changed
  on disk since you opened it --- length, modification time and a digest of every byte,
  taken at open and checked again before anything is written. Deleting a page drops the
  document's bookmarks, because their destinations name pages that are no longer in the
  file --- repairing them one by one is its own piece of work. Moving a page keeps them,
  because a bookmark names a page rather than a position.
  <!-- built: file.save file.saveCopy -->

  A save that only *adds* marks is written as a PDF incremental update: the previous
  revision is left exactly where it is and a few hundred bytes go on the end. Everything
  else --- a deletion, a move, a turn, a crop --- rebuilds the file beside itself and renames
  it into place, so an interrupted save leaves the original rather than half of a new one.
  The append has no such instant and does not claim one: it writes the body, waits for it
  to reach the disk, then writes the trailer that makes it the current revision, and cuts
  the file back to what it was if anything goes wrong.

  **That split is what an encrypted document can and cannot have.** Marks are appended, and
  each appended object is encrypted with the key the document was opened under, so the file
  stays locked and the same password still opens it. A rewrite is refused outright rather
  than written out in the clear --- so on an encrypted document you can add marks and save,
  and a deletion, a move, a turn or a crop is declined and says so.

## Not built yet

This list is checked rather than remembered: each bullet carries the command that would
exist if it were built, and `src/lib/readme.test.ts` refuses any of them the application
actually registers. It is here because the list went on naming drawing, shapes, text boxes
and squiggly for weeks after all four shipped.

**That direction alone was not enough, and the shortfall was countable.** It catches a
bullet whose command ships under the name the bullet guessed, and nothing else --- so stamps
went on being listed here after shipping as `edit.stamp.approved` and three siblings,
because the bullet had guessed `edit.addStamp`. The check now runs the other way as well:
every command the application registers is either named in the two sections above or
excluded by name with a reason, so a capability cannot arrive unmentioned by being called
something nobody predicted. What is excluded is opening a file, checking for an update and
moving about a document; the reasons are in the test rather than here, one per command.

The ids come from the registry itself rather than from a scan of the source, and that is
not fastidiousness: the colour commands and the stamps are built in a loop, so their ids
are literals nowhere on disk. The scan this replaced was blind to all eleven of them ---
including the four stamps the paragraph above is about, which it would have passed as
unbuilt while they shipped.

- The rest of the page operations: insert
  <!-- not-built: edit.insertPages -->
- Editing a comment that came out of a file. Your own marks are yours to change; a note
  somebody else wrote is read-only, because the model knows nothing about it.
  <!-- not-built: edit.editForeignMark -->
- **Marking for redaction by selecting text, or by searching for a pattern** --- an email
  address, an order number, a word list --- rather than only by dragging a rectangle. The
  removal and its verification are built; what is missing is the other two ways to say
  what to remove. And a region over a picture is still reported rather than removed.
  <!-- not-built: edit.redactSelection edit.redactMatches -->
- Forms and visual signatures. Signatures are read, never made.
  <!-- not-built: edit.fillForm edit.signDocument -->
- In-place text editing
  <!-- not-built: edit.editText -->

## What Phase 0 established

- Cold start to first page is **276 ms warm**, against a 300 ms target --- but ~250 ms of
  that is Tauri and WebKit before any application code runs, so the budget that is
  actually ours is about 50 ms.
- PDFium charges roughly **1 second per render call** on a dense A0 page whatever size
  tile you ask for, so tiling helps by covering less area, never by asking smaller.
- A **worker process boundary is nearly free** --- 6 µs of control latency, 0.11 ms to move
  a 4 MB tile --- which is what makes sandboxing every parse affordable rather than a
  trade-off.
- **PDFium is not usable for redaction.** Its edit path regenerates whole content streams
  and discards marked content, and `set_text()` silently draws `.notdef` for glyphs
  outside a subsetted font. Surgical `lopdf` operator rewriting does neither.
- A **byte scan cannot verify a redaction** on any document with a Type0 font, because the
  content stream carries glyph ids rather than text. A verifier that cannot decode a
  carrier reports "not verified", never "clean".

Known limit carried into Phase 1: on an A0 vector sheet the scroller holds a flawless
60 fps over a screen that is 6--10% sharp while moving. Nothing goes blank --- the
low-resolution page under it covers the rest, on the worst frame of every round measured
--- but frame rate alone cannot distinguish a viewer that is keeping up from one that has
given up, which is why coverage is now measured beside it.

## Stack

Tauri 2, Svelte 5, Rust, PDFium (via `pdfium-render`), `lopdf`.

Every dependency is permissively licensed, and deliberately so: no AGPL or GPL, which
rules out MuPDF — the engine SumatraPDF uses and the obvious choice on the merits. That
decision is what makes this repository MIT rather than AGPL, and it was taken before the
first line was written, because it is not a decision you can revisit later.

## Build

```
npm install
scripts/fetch_pdfium.py     # pinned PDFium, verified by digest
scripts/gates.py            # all quality gates
```

[`BUILD.md`](BUILD.md) has the details, including why benchmarking through `tauri dev`
without `--release` produces inverted results.

## Security

tpdf parses hostile input by design, and [`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) is
the worked-out position rather than a paragraph of reassurance: the trust boundaries, the
sandbox profile in full, and the residual risks in one list, with every claim either
measured and attributed to the spike that measured it, or marked untested.

To report a vulnerability, see [`SECURITY.md`](SECURITY.md). Please do not open a public
issue for one.

## A note on the documentation

[`docs/TRAPS.md`](docs/TRAPS.md) is a list of every mistake this project has made that was
expensive enough to be worth writing down — indexed by title in [`AGENTS.md`](AGENTS.md),
with a gate keeping the two lists the same list. How many there are is
`grep -c '^### ' docs/TRAPS.md` and is deliberately not written here: this paragraph said
"over two hundred" while the file held 425, which is the same drift the gate exists to
stop one level down. A good half
of the entries are not about PDFs at all but about measurement and about writing checks that
are capable of failing, which is the recurring subject: a test that cannot go red passes
exactly like one that can.

It is kept for the next person working on this, and that has generally been me a fortnight
later. It is public on the theory that it is more useful than it is embarrassing.

## Licence

MIT — see [`LICENSE`](LICENSE).

The binaries additionally bundle PDFium and a Rust crate tree, whose licences require their
notices to be reproduced in binary distributions.
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) is that file: the Rust crates linked
into the binary, the npm packages the bundler actually put in the frontend, and the C++
libraries compiled into PDFium. It is generated by `scripts/third_party_notices.py`, ships
inside both installers, and a gate fails if it goes stale or if a GPL-family licence ever
appears. It carries its own counts, which is why none are quoted here — the three that used
to be were 325, four and fourteen against a tree holding 382, and they had been wrong for
weeks.

That last population is the point of doing it this way: `cargo metadata` is structurally
blind to what is inside a compiled blob, so a sweep that is complete over cargo and silent
about everything else passes exactly like one that covered the whole product. The C++
libraries are enumerated from the licence files shipped beside the library instead, and a
new file appearing there is a finding rather than a footnote.
