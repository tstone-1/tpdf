# tpdf — Architecture and Roadmap

Status: **planning**, pre-Phase-0. Nothing is built yet. This document records the design
and the reasoning behind it, so that decisions can be revisited on their merits rather
than re-argued from scratch.

Durable constraints (licensing, versioning, quality gates, known traps) live in
[`AGENTS.md`](../AGENTS.md) and are not repeated here.

---

## 1. The problem

Every PDF tool in daily use fails in a specific, diagnosable way.

**Adobe Acrobat** is slow to start, slow to scroll, and its capability is buried. The
tools exist but finding them takes longer than using them. Much of the slowness is not
PDF work at all --- it is a plugin host, a JavaScript runtime, telemetry, and cloud sync
sitting between the user and a bitmap.

**Foxit** is the same architecture with different chrome. Faster, still modal, still a
ribbon hunt.

**SumatraPDF** is the proof that fast is achievable: it renders on a background thread,
caches aggressively, and puts nothing between the user and the page. It cannot edit.

**Open-source editors** fail on text. LibreOffice Draw substitutes fonts badly and breaks
line wrapping; Inkscape's importer converts text to uneditable paths. There is no clean
FOSS answer that views, edits, fills forms, redacts, and runs locally.

tpdf targets the gap: Sumatra's speed, Acrobat's capability, and a discovery model
borrowed from code editors rather than from office suites.

---

## 2. Design principles

These are the tie-breakers when a decision is otherwise balanced.

1. **Never show a blank page.** A stale or low-resolution page is always better than
   nothing. Most of what "feels instant" means is the absence of empty states.
2. **No modes.** Acrobat makes you enter the Comment tool before you can comment. tpdf
   surfaces actions on what is selected, where it is selected.
3. **Local operations get no spinner.** If something local is slow enough to need a
   progress indicator, that is a bug to fix, not a UI to design.
4. **Destructive operations are explicit, reviewable, and verified.** Redaction in
   particular: mark, review, apply, verify.
5. **Measure, do not assert.** Any performance claim comes with interleaved A/B numbers
   on real documents. Wall clock on these machines drifts several percent over minutes,
   which is larger than most changes worth making --- so alternate A,B,A,B over several
   rounds and compare pairwise. Never two blocks back to back.

---

## 3. System shape

```
+-------------------------------------------------------------+
|  Webview (Svelte 5)                                          |
|                                                              |
|  Command palette | Virtual page scroller | Overlay layers    |
|                    (canvas tiles)          (annotations,     |
|                                             text edit, marks)|
+---------------------- Tauri IPC -----------+-----------------+
|  commands (JSON, small)                    | custom protocol |
|                                            | (bytes, large)  |
+-------------------------------------------------------------+
|  Rust core                                                   |
|                                                              |
|  +-------------+  +--------------+  +---------------------+  |
|  | Render pool |  | Edit journal |  | Document services   |  |
|  | (tiles,     |  | (commands,   |  | text index, search, |  |
|  |  cancellable)|  |  undo/redo) |  | outline, fonts      |  |
|  +------+------+  +------+-------+  +----------+----------+  |
|         |                |                     |             |
|  +------+----------------+---------------------+----------+  |
|  |  PDFium (pdfium-render)   |   lopdf (object graph)     |  |
|  +---------------------------+----------------------------+  |
+-------------------------------------------------------------+
```

Two distinct channels cross the IPC boundary, and keeping them separate is the single
most important implementation detail:

- **Commands** carry small JSON: open a document, apply an edit, run a search. Ordinary
  Tauri `invoke`.
- **Pixels** never touch JSON. Bitmaps go through a Tauri **custom URI protocol**, which
  is served by the webview's native network stack --- no base64, no serialization. The
  frontend requests `tile://<doc>/<page>/<zoom>/<x>/<y>`, gets raw bytes, and turns them
  into an `ImageBitmap`.

Base64-over-`invoke` is the default way people wire this up and it is the reason most
Tauri PDF viewers feel sluggish. It is roughly a 33% size penalty on top of JSON
string encoding and a main-thread parse. Avoid entirely.

---

## 4. Render pipeline

This is where "fast" is won or lost.

### Tiling

Pages render in tiles (starting point 512x512 device pixels) at the current zoom. Only
visible tiles plus one screen of prefetch in the scroll direction are rendered. Without
this, a 400% zoom on an A0 drawing allocates a bitmap measured in gigabytes.

### Two-tier cache

- **Tier 1, permanent:** every page gets a cheap low-resolution bitmap (~150 px wide),
  rendered once, kept for the session. Doubles as the thumbnail. Total cost for a
  500-page document is a few megabytes.
- **Tier 2, transient:** sharp tiles at the current zoom, LRU-evicted against a memory
  budget.

While a sharp tile is in flight, the tier-1 bitmap is upscaled into its place. The user
sees a blurry page sharpen, never a white rectangle. This one mechanism accounts for most
of the perceived speed difference against Acrobat.

### Cancellable render queue

Tiles are rendered by a Rust worker pool. Every request carries a generation counter;
when the viewport moves, in-flight work for tiles now off-screen is cancelled rather than
completed and discarded. Fast scrolling in Acrobat stutters precisely because it does not
do this --- it finishes rendering pages you have already scrolled past.

PDFium is not thread-safe on a single document handle (see `AGENTS.md`). Two candidate
designs, to be settled by measurement in Phase 0:

- **A:** one dedicated render thread per open document, work queue, no locking.
- **B:** N document handles opened over one shared memory-mapped buffer, rendered in
  parallel.

B should scale better on multi-core and costs more memory. Measure both.

### Startup path

Target: **cold start to first page painted under 300 ms.**

The critical insight is that the JS framework must not be on the critical path. On
launch, Rust receives the file path (Tauri single-instance plus macOS file-association
`open-url` / Windows argv), opens the document, and begins rendering page 1 *before* the
webview has finished booting. By the time Svelte mounts, the first tile is already
waiting.

### Virtual scrolling

The frontend never mounts 500 page elements. A windowed scroller keeps a handful of page
containers alive and recycles them, with page geometry computed up front from the page
size table so the scrollbar is correct from the first frame.

### If the webview is not fast enough

Phase 0 exists to find this out with numbers rather than after months of work. The
escalation path, in order:

1. `OffscreenCanvas` in a worker, so decode and paint leave the main thread.
2. WebGL/WebGPU compositing of tiles instead of 2D canvas `drawImage`.
3. A native `wgpu` surface rendered underneath the webview, with the webview reduced to
   chrome and overlays. This is a real architectural cost --- hit testing, coordinate
   mapping, and window layering all get harder --- so it is a fallback, not a plan.

---

## 5. Document model: an edit journal

The naive approach is to mutate the PDFium document directly on every edit. It makes
undo/redo painful, couples the UI to a C++ library's mutation semantics, and turns every
save of a large file into a full rewrite.

Instead: **the loaded PDF is an immutable baseline, and every edit is a command in an
append-only log.**

```rust
enum EditCommand {
    AddAnnotation { page: PageId, annot: Annotation },
    ModifyAnnotation { id: AnnotId, delta: AnnotationDelta },
    DeleteAnnotation { id: AnnotId },
    MovePage { from: usize, to: usize },
    RotatePage { page: PageId, degrees: i32 },
    DeletePage { page: PageId },
    InsertPages { at: usize, source: PageSource },
    SetFormField { field: FieldId, value: FieldValue },
    ReplaceTextRun { run: TextRunId, text: String },
    MarkRedaction { page: PageId, region: Region },
    ApplyRedactions { marks: Vec<RedactionMarkId> },   // barrier, see section 6
}
```

What this buys:

- **Undo/redo is a pointer into the log.** No inverse operations to hand-write, no
  divergence between what the UI shows and what the document holds.
- **Annotations render as an overlay layer**, drawn by us on top of the page bitmap
  rather than baked into it. Dragging a highlight is a frontend operation at 60 fps; the
  page bitmap never re-renders.
- **Crash recovery.** The log is persisted next to the session, keyed by file hash.
  Reopening a file after a crash offers to replay unsaved edits.
- **Incremental save.** On save, the log is applied and written as a PDF incremental
  update --- an appended section rather than a rewrite. Saving a 300 MB scanned document
  becomes near-instant instead of a coffee break, and existing digital signatures on the
  original survive.

The log is also the natural seam for a future "what changed?" review view, and for
collaboration if that ever becomes interesting.

---

## 6. Redaction

Treated as a first-class subsystem, because a redaction that leaves recoverable content
is worse than no redaction --- it is a confident lie.

### Workflow: mark, review, apply, verify

Redaction is irreversible, so it is a two-phase operation as in Acrobat, but with a
verification step Acrobat does not offer.

1. **Mark.** The user drags regions, or selects text, or runs a pattern search (email
   addresses, order numbers, a supplied word list) and marks all hits. Marks are stored
   as `MarkRedaction` commands in the edit journal and rendered as an overlay. Nothing is
   destroyed yet; marks are fully undoable.
2. **Review.** A dedicated view lists every mark with its page, extracted text, and a
   thumbnail. This is the last chance to catch an over- or under-selection.
3. **Apply.** Destructive. Content is removed, the file is fully rewritten, and the
   journal is truncated at this point --- you cannot undo past an applied redaction once
   saved.
4. **Verify.** Automatic, mandatory, and reported to the user.

### What "apply" must remove

Visible page content is the easy part and the part everyone gets right. The leaks are
elsewhere.

| Carrier | Treatment |
|---------|-----------|
| Text objects fully inside the region | Remove the page object |
| Text objects partially inside | Split: emit new text object(s) for the surviving substring, preserving font, size, text matrix, char/word spacing, and colour; remove the original |
| Images intersecting | Map the region through the inverse image matrix, blank those pixels, re-encode, replace. Remove outright if fully covered |
| Vector paths intersecting | Remove if the bounding box is contained. Partial intersection falls back to region rasterization (see below) |
| Annotations overlapping | Remove, including their appearance streams and popup notes |
| Form fields overlapping | Remove the field, its value, and its appearance stream |
| Document metadata | Scrub XMP, DocInfo, and custom properties |
| Structure tree | `/Alt`, `/ActualText`, and `/E` entries frequently duplicate the visible text verbatim. Must be scrubbed |
| Optional content groups | Hidden layers may carry the same content. Enumerate and scrub |
| Embedded files and attachments | Enumerate; warn, and offer removal |
| Document JavaScript | Can contain literal strings. Warn |
| Outline/bookmark titles | Often copy heading text. Scrub matches |
| Page labels | Same |
| Incremental update history | **Forced full rewrite.** No incremental section, no retained original objects |
| Embedded font subsets | A subset may retain glyph outlines used only by redacted text. Low risk, but re-subsetting is the correct fix. Deferred past v1, documented as a known limitation |

The last two rows are where naive implementations leak. Saving a redaction incrementally
leaves the original page content verbatim in the file, one `strings` invocation away.

### Region rasterization fallback

Partial vector-path intersection has no clean answer: removing the whole path
over-redacts and visibly damages the page; clipping it correctly is hard.

The escape hatch is to rasterize --- render the affected region (or the whole page) at
high DPI, replace the content with the resulting image, and optionally re-OCR the
non-redacted parts to restore searchability. This is lossy but unconditionally safe, and
many organizations mandate exactly this for outgoing documents. Offer it as an explicit
"flatten to image" mode alongside surgical redaction, and choose it automatically when
surgical redaction cannot be proven complete.

### The verification pass

After writing the redacted file, tpdf reopens it from disk --- not from memory --- and:

1. Re-extracts all text from every page and asserts no redacted string appears.
2. Decompresses every object stream via `lopdf` and scans the raw bytes for the redacted
   strings, which catches content that is present but not reachable through normal text
   extraction.
3. Walks metadata, structure tree, outlines, optional content groups, annotations, and
   embedded files for the same strings.
4. Confirms the file contains no incremental update section carrying pre-redaction
   objects.

The result is reported plainly: verified clean, or a specific list of what was found and
where. Given the PDFium `GenerateContent` trap in `AGENTS.md` --- where a removed object
silently survives into the saved file while the in-memory API reports it gone --- this
pass is not belt-and-braces, it is the only thing standing between a plausible-looking
redaction and a leak.

---

## 7. In-place text editing

The hardest subsystem, deliberately scheduled last, but designed for from day one so the
architecture does not preclude it.

### Why it is hard

Embedded fonts are **subsetted**: the font programme inside the PDF contains only the
glyphs the document already uses. Type a character that is not in the subset and there is
no glyph to draw. Recovering requires locating the same font on the system, extracting
the missing glyphs, re-embedding an extended subset, and re-justifying the line with
correct metrics. When Acrobat mangles an edit, this is why.

### The approach

1. **Extract glyph runs** with per-character position, font reference, size, text matrix,
   and colour. PDFium provides this through the `FPDFText_*` API.
2. **Group glyphs into lines and blocks** using spacing and baseline heuristics. This
   step is where edit quality actually lives, and it is entirely our own code --- no
   library does it well.
3. **Serve the embedded font to the webview.** Extract the font programme from the PDF
   and expose it over the same custom protocol as tiles, registered as an `@font-face`.
   The edit overlay then renders in the document's *actual* font, because it is the same
   font file. This is the trick that makes the editing experience look correct, and it is
   the main reason to build the text layer in the webview rather than natively.
4. **Edit in an overlay** positioned exactly over the glyph run, with the underlying page
   region suppressed.
5. **Commit** by rewriting the text-showing operators for that block and regenerating the
   content stream.
6. **Handle missing glyphs honestly.** If a typed character is outside the subset:
   attempt system font matching and glyph re-embedding; if that fails, substitute a
   metric-compatible font and **tell the user visibly** which characters were substituted.
   Silent substitution is what makes competitors untrustworthy.

### Scoping

- **v1 of the feature:** edit existing text where glyphs exist in the subset, plus system
  font matching with a clear warning when they do not. Single-line and
  within-block reflow.
- **Later:** full paragraph reflow across lines, font size and style changes, adding new
  text blocks with arbitrary fonts.

---

## 8. UX

The interaction model is borrowed from code editors, not office suites, because the
stated pain is discovery.

- **Command palette on Cmd/Ctrl+K.** The primary way to reach any command. Fuzzy search,
  recently used first, shows the keybinding for whatever it finds so it teaches shortcuts
  as a side effect. Available from Phase 1 --- it is the thesis, not a garnish.
- **Contextual actions, not modes.** Select text and highlight/copy/redact/comment appear
  at the selection. Select an image and extract/replace/delete appear there. There is no
  state in which the wrong click does nothing.
- **One thin toolbar** holding only the handful of controls used constantly: page
  navigation, zoom, search, and a sidebar toggle. Everything else lives in the palette or
  in context.
- **Keyboard-first,** with every command bindable, and navigation keys that will feel
  familiar to Sumatra users.
- **No modal dialogs for routine work.** Inline, dismissible, non-blocking.
- **Sidebar** with thumbnails, outline, annotation list, and search results as tabs.
- Dark and light themes, following the system.

---

## 9. Roadmap

Each phase has an exit criterion. A phase is not done when the code is written; it is
done when the criterion is met.

### Phase 0 — Spike (1-2 days)

Prove the render pipeline before committing to it.

Build the thinnest possible Tauri 2 + PDFium + custom-protocol + tiled-canvas path.
Benchmark on real documents: a 500-page text-heavy report, a heavy vector CAD drawing,
and a 300 MB scanned scan.

Measure, with interleaved A/B and pairwise comparison:

- Cold start to first page painted.
- Sustained scroll frame rate at 100% and 400% zoom.
- Tile latency after a zoom change.
- Memory ceiling on the 500-page document.
- Render thread design A vs B (section 4).

**Exit criterion:** under 300 ms to first paint and no dropped frames on sustained
scroll, or a decision to escalate down the fallback list in section 4. Record the numbers
and the thread-safety conclusion in `AGENTS.md`.

### Phase 1 — The viewer

Beat SumatraPDF on capability without losing to it on speed.

Open, scroll, zoom (fit width/page/selection), rotate view, search-as-you-type across the
whole document, text selection and copy, thumbnails sidebar, outline/TOC, print, file
associations, session and scroll-position restore, dark mode, command palette.

**Exit criterion:** tpdf becomes the daily default for reading. If it is not, it is not
finished.

### Phase 2 — Editing foundation

Edit journal, undo/redo, incremental save, crash recovery.

Page operations: reorder by dragging thumbnails, rotate, delete, insert, extract, split,
merge, crop.

Annotations: highlight, underline, strikeout, sticky notes, freehand ink, shapes, text
boxes, stamps --- as real PDF annotation objects, interoperable with Acrobat and Preview.

**Exit criterion:** a document can be marked up, saved, reopened in Acrobat, and look
right.

### Phase 3 — Redaction

The full subsystem in section 6, including the verification pass and the rasterization
fallback. Scheduled ahead of forms because it is the higher-value capability here and the
one with no acceptable off-the-shelf answer.

**Exit criterion:** verification passes on a corpus of deliberately nasty documents ---
text in structure trees, hidden layers, embedded attachments, prior incremental updates.

### Phase 4 — Forms and signatures

AcroForm field filling with saved state, appearance stream regeneration, signature image
placement, and optionally cryptographic signing. XFA is explicitly out of scope; it is a
dead Adobe extension and a rabbit hole.

### Phase 5 — Text editing

Section 7, scoped as described there.

### Cross-cutting, not a phase

OCR (Tesseract, for scanned documents, feeding both search and redaction), accessibility,
and localization. Slotted in as they become the binding constraint.

---

## 10. Open questions

Recorded rather than guessed at. Each needs either a measurement or a decision.

1. **PDFium binary distribution.** Prebuilt binaries from `bblanchon/pdfium-binaries`
   dynamically linked and bundled is the pragmatic path; static linking means building
   PDFium, which is painful. Dynamic bundling has implications for macOS notarization and
   signing that need to be worked out early, since it bit `screenpick`'s release path.
   Roughly 10 MB per platform, giving a total app size around 25 MB against Acrobat's
   gigabyte-plus.
2. **Render thread design A vs B** (section 4). Phase 0 decides.
3. **Where the annotation overlay lives.** Drawing annotations in the frontend gives
   60 fps manipulation but means two rendering code paths (ours for editing, PDFium's for
   the saved file), which risks visual divergence. The alternative is round-tripping
   through PDFium on every change, which is correct but slow. Likely answer: frontend
   overlay while editing, PDFium render on commit, with a visual regression test that the
   two agree.
4. **Tile size.** 512 is a starting guess, not a result. Measure.
5. **Whether a document ever needs more than one PDFium instance** for concurrent
   open documents, or whether one pool serves all.
