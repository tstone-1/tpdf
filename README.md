# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: Phase 0 closed, Phase 1 in progress, Phase 2 begun. First release: `26.8.0`.**
The feasibility spikes are done and every load-bearing assumption has a measured verdict;
on top of that evidence there is a viewer you can read a PDF in, on macOS arm64 and on
Windows. **It edits**: pages can be turned, moved, deleted, cropped and extracted; text can
be highlighted, underlined, struck out or squiggled; and you can draw on a page, put a box,
an ellipse, a text box or a comment on it, move what you put there, and save --- over the
open file or to a copy. What is *not* built is the list further down, and redaction and
in-place text editing are the two that matter.
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
- Tiled rendering behind a virtual scroller, zoom, view rotation, and page inversion for
  reading on a dark screen.
- Text selection and copy, find-in-document, an outline sidebar, a page-thumbnail strip,
  and a text layer for screen readers.
- Session restore: the document, page, zoom and rotation you left on.
- **What a document says about itself**: its title and producer, whether it is encrypted
  and what that permits, what conformance it claims, and who signed it --- the signer's
  certificate, its issuer and its validity, read out of the signature itself. Reading only:
  there is no trust store here, no chain is built, and nothing shown to you has been
  verified.
- Printing through the system print panel, on both platforms — and every print job is read
  back through the operating system's own PDF parser before the panel opens, which is a
  parser independent of the one that wrote the job and the one that drew what you saw.
  macOS prints vectors; Windows has no in-box "print this PDF" API at any layer, so it
  rasterises at 300 dpi like every other Windows PDF viewer does.
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
- **Delete a page**, from the command palette. Undo puts it back where it was, with its own
  rotation. It has no keyboard shortcut on purpose: it is the one command that removes
  something you can see.
- **Move a page** by dragging its thumbnail in the page strip, or one slot at a time from
  the palette. A moved page takes its size, its crop and its rotation with it even where the
  file states none of them on the page itself --- a PDF lets a page inherit those from the
  group it sits in, and that is where moving one silently changes it.
- **Print what you edited.** A print job carries the pages that are left, the order they
  are in and the way each one is turned, read from the document model rather than from the
  file on disk.
- **Mark a selection** --- highlight, underline, strike out or squiggle --- as a real PDF
  annotation, not a rectangle drawn over the page, so Acrobat and Preview show it as what
  it is. Each mark takes a note, and **Next mark** / **Previous mark** walk them from the
  keyboard: the pointer is not the only way to reach one.
- **Draw on a page** --- freehand ink, a box, an ellipse, a text box, or a comment placed
  where you press --- with an eraser for the ones you change your mind about. Each is a
  real annotation of its own kind rather than ink pretending to be one, so another reader
  gets a comment they can open and a shape they can select. What you have drawn can be
  dragged to somewhere else on its page afterwards.
- **Crop a page**, either to what is on it or to a rectangle you drag out. Cropping to
  content measures where the ink actually is rather than reading the page's objects, so it
  works on a scan --- where every object union is the whole sheet --- as well as on a page of
  type. Dragging is for the cases a measurement cannot answer: a figure out of a plate, one
  column of two, a scan with a hand in the corner. While you drag, what falls outside the
  rectangle is darkened, so what stays bright is what the page becomes. The crop is part of the document: undoable, carried when the page moves,
  and written into a saved copy as a real `/CropBox`, so another reader opens the file
  cropped the way you left it.
- **Extract pages to a second file**, naming a range the way you would say it out loud.
  It reads the document and writes elsewhere, so there is nothing to undo and the open
  file is untouched. It refuses a reversed range rather than quietly correcting it.
- **Save**, over the open file or to a copy. A save is refused outright if the file changed
  on disk since you opened it --- length, modification time and a digest of every byte,
  taken at open and checked again before anything is written. An encrypted source is
  refused rather than silently saved without its encryption. Deleting a page drops the
  document's bookmarks, because their destinations name pages that are no longer in the
  file --- repairing them one by one is its own piece of work. Moving a page keeps them,
  because a bookmark names a page rather than a position.

  A save that only *adds* marks is written as a PDF incremental update: the previous
  revision is left exactly where it is and a few hundred bytes go on the end. Everything
  else --- a deletion, a move, a turn, a crop --- rebuilds the file beside itself and renames
  it into place, so an interrupted save leaves the original rather than half of a new one.
  The append has no such instant and does not claim one: it writes the body, waits for it
  to reach the disk, then writes the trailer that makes it the current revision, and cuts
  the file back to what it was if anything goes wrong.

## Not built yet

This list is checked rather than remembered: each bullet carries the command that would
exist if it were built, and a gate refuses any of them the application actually registers.
It is here because the list went on naming drawing, shapes, text boxes and squiggly for
weeks after all four shipped.

The check is narrower than it sounds, and both halves of that are worth knowing. It catches
a bullet whose command ships **under the name the bullet guessed**, and nothing else --- so
stamps went on being listed here after shipping as `edit.stamp.approved` and three siblings,
because the bullet had guessed `edit.addStamp`. When a bullet leaves this list, check that
the id it named is the id that shipped.

- The rest of the page operations: insert, split, merge
  <!-- not-built: edit.insertPages edit.splitDocument edit.mergeDocuments -->
- Editing a comment that came out of a file. Your own marks are yours to change; a note
  somebody else wrote is read-only, because the model knows nothing about it.
  <!-- not-built: edit.editForeignMark -->
- **True redaction** with an automatic post-save verification pass
  <!-- not-built: edit.redactSelection -->
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
