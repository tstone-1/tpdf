# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: Phase 0 closed, Phase 1 in progress.** The feasibility spikes are done and every
load-bearing assumption has a measured verdict; on top of that evidence there is now a
viewer you can read a PDF in. It has been built and checked on macOS arm64 only ---
Windows has never been built --- and nothing in it edits a document yet. See
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
- Printing through the system print panel, on macOS.
- Every command reachable from the command palette, which renders each shortcut from the
  same table the key handler matches against, so a label cannot advertise a chord that
  does nothing.

All of that has run on macOS arm64 and nowhere else.

## Not built yet

- Page operations: reorder, rotate, delete, insert, extract, split, merge, crop --- in the
  document, not only in the view
- Annotations: highlight, ink, notes, shapes, stamps --- real PDF annotation objects
- **True redaction** with an automatic post-save verification pass
- Forms and visual signatures
- In-place text editing
- A Windows build. `BUILD.md` lists the three things known to be in the way.

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
rules out MuPDF. That keeps the code reusable in private and work repositories, and leaves
the option of making this repo public.

## Build

```
npm install
scripts/fetch_pdfium.py     # pinned PDFium, verified by digest
scripts/gates.py            # all quality gates
```

[`BUILD.md`](BUILD.md) has the details, including why benchmarking through `tauri dev`
without `--release` produces inverted results.
