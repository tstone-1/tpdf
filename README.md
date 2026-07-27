# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: Phase 0 complete, no viewer yet.** The feasibility spikes are done and every
load-bearing assumption has a measured verdict; what exists in the tree is that evidence
and its harnesses, not an application you can read a PDF in. See
[`docs/PLAN.md`](docs/PLAN.md) for the architecture and roadmap,
[`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) for the security position,
[`BUILD.md`](BUILD.md) to build it, and [`AGENTS.md`](AGENTS.md) for project conventions.

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

Known failure carried into Phase 1: on an A0 vector sheet the scroller holds a flawless
60 fps over a screen that is 0--4% sharp. Frame rate cannot distinguish a viewer that is
keeping up from one that has given up.

## Planned capabilities

- Viewer: tiled GPU-composited rendering, search-as-you-type, outline, thumbnails
- Page operations: reorder, rotate, delete, insert, extract, split, merge, crop
- Annotations: highlight, ink, notes, shapes, stamps --- real PDF annotation objects
- **True redaction** with an automatic post-save verification pass
- Forms and visual signatures
- In-place text editing

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
