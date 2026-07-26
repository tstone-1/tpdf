# tpdf

A fast, lightweight PDF viewer and editor for macOS and Windows.

SumatraPDF's speed with Acrobat's capability, and a UI where you never hunt for a tool.

**Status: planning.** No code yet. See [`docs/PLAN.md`](docs/PLAN.md) for the
architecture and roadmap, and [`AGENTS.md`](AGENTS.md) for project conventions.

## Planned capabilities

- Viewer: tiled GPU-composited rendering, search-as-you-type, outline, thumbnails
- Page operations: reorder, rotate, delete, insert, extract, split, merge, crop
- Annotations: highlight, ink, notes, shapes, stamps --- real PDF annotation objects
- **True redaction** with an automatic post-save verification pass
- Forms and signatures
- In-place text editing

## Stack

Tauri 2, Svelte 5, Rust, PDFium (via `pdfium-render`), `lopdf`.
