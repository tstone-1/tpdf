# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: Phase 0 closed, Phase 1 in progress. Nothing has been released yet.** The
feasibility spikes are done and every load-bearing assumption has a measured verdict; on
top of that evidence there is a viewer you can read a PDF in, on macOS arm64 and on
Windows. Nothing in it edits a document yet, and there are no tags and no installers to
download — building it yourself is currently the only way to run it. See
[`docs/PLAN.md`](docs/PLAN.md) for the architecture and roadmap,
[`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) for the security position,
[`BUILD.md`](BUILD.md) to build it, and [`AGENTS.md`](AGENTS.md) for project conventions.

## What the viewer does today

- Every document is parsed and rendered in **sandboxed worker processes** with no
  filesystem or network authority --- a pool per document, and a worker that dies is
  replaced and its request retried.
- Tiled rendering behind a virtual scroller, zoom, view rotation, and page inversion for
  reading on a dark screen.
- Text selection and copy, find-in-document, an outline sidebar, a page-thumbnail strip,
  and a text layer for screen readers.
- Session restore: the document, page, zoom and rotation you left on.
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

## Not built yet

- Page operations: reorder, rotate, delete, insert, extract, split, merge, crop --- in the
  document, not only in the view
- Annotations: highlight, ink, notes, shapes, stamps --- real PDF annotation objects
- **True redaction** with an automatic post-save verification pass
- Forms and visual signatures
- In-place text editing

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

[`docs/TRAPS.md`](docs/TRAPS.md) is a numbered list of every mistake this project has made
that was expensive enough to be worth writing down — currently 206 of them, indexed by
title in [`AGENTS.md`](AGENTS.md). A good half are not about PDFs at all but about
measurement and about writing checks that are capable of failing, which is the recurring
subject: a test that cannot go red passes exactly like one that can.

It is kept for the next person working on this, and that has generally been me a fortnight
later. It is public on the theory that it is more useful than it is embarrassing.

## Licence

MIT — see [`LICENSE`](LICENSE).

The binaries additionally bundle PDFium (BSD-3-Clause) and a Rust crate tree that is MIT
and Apache-2.0, both of which require their notices to be reproduced in binary
distributions. **That notice file is not written yet**, and it gates the first release
rather than this repository — nothing is distributed as a binary today.
