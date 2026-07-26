# tpdf — Architecture and Roadmap

Status: **planning**, pre-Phase-0. Nothing is built yet. This document records the design
and the reasoning behind it, so that decisions can be revisited on their merits rather
than re-argued from scratch.

Revised 2026-07-26 after an independent audit (Codex, cold read). The audit found the
first draft's thread-safety model factually wrong, its redaction verifier over-claiming,
its edit journal underspecified, and its security boundary missing entirely. Those are
fixed below; the audit's own errors are noted where relevant so they are not
reintroduced.

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
4. **Never claim more than was proved.** Applies to redaction above all: an unverifiable
   result is reported as unverified, never as clean. Silent font substitution, silent
   over-redaction, and silent truncation are the same defect in different clothes.
5. **Measure, do not assert.** Any performance claim comes with interleaved A/B numbers
   on real documents. Wall clock on these machines drifts several percent over minutes,
   which is larger than most changes worth making --- so alternate A,B,A,B over several
   rounds and compare pairwise. Never two blocks back to back.

---

## 3. System shape

Three processes, not one. This is the central architectural decision and it is forced
from two independent directions at once.

```
+---------------------------------------------------------------+
|  UI process (Tauri 2 + Svelte 5)                              |
|    command palette | virtual scroller | canvas | overlays     |
+------------------------ IPC ----------------+-----------------+
|  commands (JSON, small)                     | custom protocol |
|                                             | (bytes, large)  |
+---------------------------------------------------------------+
|  Coordinator (Rust, trusted)                                  |
|    edit journal | working document | search index | cache      |
|    worker supervision, restart, resource limits                |
+---------------------------------------------------------------+
                |                              |
     +----------v---------+        +-----------v----------+
     | Render worker(s)   |        | Surgery worker       |
     | PDFium, sandboxed  |        | lopdf / QPDF, sandboxed
     | no fs, no network  |        | bounded decoding     |
     +--------------------+        +----------------------+
```

**Why processes, not threads.** Two constraints land on the same answer:

- **Security.** PDFium is native C++ parsing attacker-controlled input. A malformed file
  should cost a worker, not the application and not the user's home directory. Chrome
  sandboxes PDFium for this reason.
- **Parallelism.** `pdfium-render` serializes *every* PDFium call behind one global mutex
  (`thread_safe`, on by default), and upstream recommends parallel processes over threads.
  In-process parallel rendering is therefore not merely awkward, it does not exist.

The first draft proposed "N document handles over one shared buffer, rendered in
parallel". That does not work, and it is recorded in `AGENTS.md` as a corrected error.

Retrofitting a process boundary later is a rewrite, so it is Phase 0 work.

### The two IPC channels

- **Commands** carry small JSON: open a document, apply an edit, run a search. Ordinary
  Tauri `invoke`.
- **Pixels** never touch JSON. Tiles go over a Tauri **custom URI protocol**.

The first draft called the custom protocol a zero-copy fast path. It is not — it is
merely *much better than base64 over `invoke`*, which carries a ~33% size penalty plus a
main-thread JSON parse and is why most Tauri PDF viewers feel sluggish. A custom scheme
still crosses the boundary with allocation, copying and per-request dispatch. That was
argued as a reason to prefer many small tiles; §4 measures the opposite, so per-request
dispatch is charged far fewer times than the first draft assumed.

One audit correction worth recording: `createImageBitmap()` *can* consume raw pixels ---
via `new ImageData(new Uint8ClampedArray(buf), w, h)` --- so an uncompressed path is
available and does not force PNG encode/decode.

### Transfer format: send raw pixels, measured 2026-07-26

`tile-bench --mode encode` renders a centred tile and PNG-encodes it. The result kills
encoding for this workload, because its cost and its benefit are anti-correlated:

| corpus | tile | render | PNG encode | raw | PNG |
|---|---|---|---|---|---|
| text, 4× | 1024² | 0.9 ms | 1.0 ms (**110%**) | 4096 KB | 397 KB |
| text, 4× | 2048² | 3.9 ms | 4.3 ms (**111%**) | 16384 KB | 1895 KB |
| vector, 1× | 1024² | 6431.7 ms | 4.0 ms (0%) | 4096 KB | **4097 KB** |

On a cheap page, encoding **roughly doubles the cost of producing a tile** — it buys a
5–10× smaller payload for 100% more CPU on the scarcest resource in the system. On an
expensive page, encoding is free relative to the render but compresses **nothing**: dense
vector content is noise-like, and PNG returns the input plus a kilobyte of headers.

So encoding costs most where it helps most, and helps least where it is affordable. Raw it
is. A 4 MB payload over a localhost protocol handler is a memcpy; the compression was
never buying back an actual bottleneck.

One caveat stated honestly: this is PNG at default compression. A faster preset trades
payload for CPU and would move the text-page row, though not the vector one, since no
codec compresses noise.

### Delivery costs more than production — measured 2026-07-26

The webview half runs inside a real webview via `TPDF_AUTOBENCH=<file.pdf> npm run tauri
dev -- --release`, which opens the document, measures, prints on stdout and exits. Text
page, tile centred, six interleaved rounds:

| variant | render | encode | transfer | decode | **end to end** | KB |
|---|---|---|---|---|---|---|
| raw 1024² | 1.36 | — | 3.00 | 1.00 | **5.4 ms** | 4096 |
| png 1024² | 1.55 | 1.41 | 4.00 | 6.00 | **13.0 ms** | 824 |
| raw 1224×1584 | 2.29 | — | 5.00 | 0.50 | **7.8 ms** | 7574 |
| png 1224×1584 | 2.67 | 2.50 | 6.00 | 10.00 | **21.2 ms** | 1430 |

Raw wins end to end by **2.4–2.7×**, confirming the server-side result from the other
direction. The 2048² variants clamp to 1224×1584 — the page at 2× is smaller than the
requested tile.

The more consequential finding is the ratio in the raw rows: **delivery costs 240–293% of
rendering.** Moving 4 MB across the custom-scheme boundary takes 3 ms — about 1.3 GB/s,
which is well short of memcpy and confirms §3's correction that a custom protocol is not a
zero-copy fast path. Per megabyte, larger tiles are again cheaper (1.00 ms/MB at 1024²
versus 0.74 ms/MB at 1224×1584), so the same "fewest, largest tiles" conclusion holds for
delivery as for rendering.

Scaled up, this is a real constraint: repainting a 1920×1080 viewport at 2× device pixel
ratio is 33 MB of raw pixels, so ~33 ms of delivery — two frames, for a full repaint such
as a zoom step. Scrolling only delivers the newly exposed strip and is far cheaper, but a
zoom change is not. Whether that needs shared memory, WebGL texture upload or simply
tolerating a two-frame zoom is now a quantified open question rather than a guess.

Measurement caveat: webview `performance.now()` is clamped to 1 ms here, visible in the
integer-valued per-round series. The 2.4× conclusion is far larger than that granularity,
but sub-millisecond claims from this harness are not supportable.

**Design consequence:** if encoding is ever reintroduced (for a remote or bandwidth-bound
transport), it must not run on the render thread. At ~100% of render time it would halve
that thread's throughput, and the render thread is already serialized behind PDFium's
global mutex.

---

## 4. Render pipeline

### Tiling

Pages render in tiles at the current zoom; only visible tiles plus one screen of prefetch
are rendered.

**Tile size: measured, 2026-07-26. 512² was the wrong guess — use 1024² or larger.**
Spike 0.1 (`src-tauri/src/bin/tile_bench.rs`, `--mode single`) rendered one centred tile
at each size plus the whole page, interleaved across rounds. Ratios were stable to within
1% across rounds on both corpora, so these differences are real, not clock drift.

The decision metric is *time to fill a viewport*, not time to render a page. For a
1920×1080 viewport on the A0 vector page at 1×:

| tile | tiles needed | time to fill |
|---|---|---|
| 256² | 40 | 39.3 s |
| 512² | 12 | 28.1 s |
| 1024² | 4 | 26.2 s |
| 2048² | 1 | 17.9 s |

Larger wins, by more than 2×. On the text page the same comparison is flat (~3.5–4 ms at
any tile size), so nothing is given up. The bet is asymmetric: tile size barely matters on
easy pages and matters enormously on hard ones.

### The fixed per-render cost, and why it drives the architecture

Tiling bounds *output bitmap* memory — the A0 page at 2× is a 128 MB bitmap, and that is
what tiling stops. It was an open question whether tiling also bounds PDFium's traversal.
Measured answer: **partly, and with a hard floor.**

PDFium *does* cull spatially — a 256² tile of the A0 page costs 4.3% of a full-page
render while covering 0.8% of its area. But cost does not approach zero as the request
shrinks. Rendering that page to a 150 px-wide thumbnail (0.03 Mpixel, 1/270th of the
page's pixels at 1×) still takes **1.52 s**, and a 256² tile still takes 0.98 s. Fitting
the two: roughly **1 s of fixed cost per render call**, plus an area-proportional term.

That floor is per *render call*, not per document or page open — the bench holds one
`PdfPage` across every measurement. PDFium rebuilds substantial per-page state on every
render. Four consequences, all load-bearing:

1. **Tile count multiplies a constant.** Every extra tile on a complex page costs ~1 s
   before drawing anything. Hence "fewest, largest tiles", above.
2. **The tier-1 placeholder is not free.** §4's promise that the user "never sees a white
   rectangle" fails on exactly the pages where it matters most — the cheap thumbnail of
   this page costs 1.5 s. Tier 1 must be rendered once at document open, in a worker, off
   the critical path, and the UI must degrade honestly while it is absent.
3. **A single such page starves the process.** 22.8 s at 1×, 48.4 s at 2× for a full
   render, holding PDFium's global mutex throughout. This is the strongest empirical
   argument for worker processes; see §3.
4. **Progressive rendering is mandatory, not an optimization.** A 1 s floor per call means
   cancellation has to work *inside* a render, which is what the `IFSDK_PAUSE` path is
   for.

Peak RSS: 211 MB for one A0 page, 70 MB for the 775-page text document.

### Two-tier cache

- **Tier 1, permanent:** every page gets a cheap low-resolution bitmap (~150 px wide),
  rendered once, kept for the session. Doubles as the thumbnail.
- **Tier 2, transient:** sharp tiles at the current zoom, LRU-evicted against a budget.

While a sharp tile is in flight, tier 1 is upscaled into its place. The user sees a blurry
page sharpen, never a white rectangle. This one mechanism accounts for most of the
perceived speed difference against Acrobat.

### Supersedable, interruptible render queue

Requests carry a generation counter; when the viewport moves, queued work for tiles now
off-screen is dropped and completed-but-stale results are discarded.

For work already started, `FPDF_RenderPageBitmap()` cannot be abandoned once entered —
but `FPDF_RenderPageBitmap_Start()` accepts an `IFSDK_PAUSE` callback whose
`NeedToPauseNow()` is polled during rendering, with `FPDF_RenderPage_Continue()` to
resume. Anything not a small bounded tile uses the progressive API, so a pathological page
yields instead of blocking. (The audit asserted PDFium rendering is uncancellable; that is
wrong, and the progressive API is precisely the mechanism.)

Backstop for the genuinely pathological: a per-tile CPU budget, and worker process
termination and restart when it is exceeded. Process isolation makes that cheap and safe.

### Startup path — measured 2026-07-26

Target: **cold start to first page painted under 300 ms** — stated in the first draft
without defining the boundary, which made it unfalsifiable. Five timestamps were
instrumented (`src-tauri/src/startup.rs`, `src/lib/startup.ts`), the timeline's origin is
the kernel's process-creation time rather than `main`, and the run is automated so cold and
warm are the same measurement repeated (`scripts/startup_bench.py`).

Measured on the M5 MacBook Pro against a release bundle, opening the 775-page text corpus
and painting the region the 1200×900 window actually shows — 2400×1726 device pixels at
DPR 2, one tile, raw transfer, per §3 and §4. Median of 7 warm runs; spread was 361–387 ms:

| milestone | warm ms | Δ | what happens in the interval |
|---|---|---|---|
| main entry | 4.2 | 4.2 | exec, dyld, framework linking |
| tauri setup | 146.1 | **141.9** | Tauri runtime + WebKit initialization |
| pdfium bound | 146.9 | 0.8 | loading and binding the Pdfium dylib |
| webview script start | 194.4 | 47.6 | webview creation, HTML load |
| app mounted | 241.4 | 47.0 | JS module load, Svelte mount |
| document open requested | 245.4 | 4.0 | IPC |
| document parsed | 246.0 | **0.6** | `FPDF_LoadDocument` on 775 pages |
| document open complete | 332.0 | **85.9** | collecting page geometry |
| first tile rendered | 339.2 | 7.2 | Pdfium |
| first preview bitmap ready | 347.4 | 8.2 | transfer + decode, 16.6 MB |
| first page presented | 374.1 | 26.6 | compositor |

**Warm start is 374 ms — 25% over a target that was supposed to be the cold one.**

Three things follow, and none of them were in the first draft.

**The PDF work is a rounding error.** Parse, render, transfer and decode together are
~16 ms of 374. The other 358 ms is shell: 142 ms of Tauri/WebKit init, 95 ms of webview and
JS boot, 86 ms of page enumeration, 27 ms of compositing. Optimising the PDF path for
startup would be optimising the wrong 4%.

**Page geometry enumeration is the one large self-inflicted cost.** Parsing a 775-page
document takes 0.6 ms — Pdfium's cross-reference handling is lazy and excellent. Walking it
to collect every page's size takes 86 ms, and on the *one-page* vector document it still
takes 52 ms, so this is per-page loading plus a fixed cost, not geometry arithmetic. The
virtual scroller (below) wants the full table up front precisely so the scrollbar is correct
on the first frame. It cannot have it on the critical path. Geometry must be lazy, with the
scrollbar estimating from the pages it has seen and correcting as it learns.

**Binding Pdfium is free** (0.8 ms), so there is nothing to gain from deferring it.

#### Cold start is a different problem than it looks

First-ever launch of a freshly built bundle: **1030 ms**, of which **444 ms is spent before
`main`**. That is not dyld and not cold I/O. Copying the bundle to a new path — same bytes,
same warm page cache, only the file identity is new — reproduces it at 299 ms, and
relaunching that same copy costs 4.8 ms. It is the OS validating a code signature it has
not seen before.

So the cost is charged **per binary identity, once**: on install and on every update, not on
every launch. A 300 ms budget cannot cover it, because it is spent before tpdf runs at all.
Two consequences: the target must be stated as *warm* start, and first-launch-after-update
is a distinct, unavoidable, ~300 ms-worse experience that the UI should not be surprised by.

A true cold-page-cache measurement of an already-known binary still needs `sudo purge` and
has not been taken; `scripts/startup_bench.py --purge` does it when run with sudo available.

#### One document blows the budget by 36×

The A0 vector page (~200k path segments) reaches first presentation in **10.9 s**, of which
10.6 s is a single Pdfium render call for the visible half of the page. Everything else in
the table is unchanged. §4's fixed-cost finding already said tiling cannot rescue this, and
this confirms it lands squarely on the startup path.

The target therefore cannot be stated as a property of all documents. It holds for typical
ones; for the rest, the requirement is that the app is *responsive* and honest — chrome up,
scrollbar correct, a visible "still rendering" state, and the progressive API yielding so
nothing else is starved. That is a design requirement, not a caveat.

#### Method notes

Rust and webview clocks have different origins, so the mapping is calibrated NTP-style
(`src/lib/clock.ts`): bracket an IPC call with local readings, assume the remote timestamp
falls at the midpoint, keep the sample with the shortest round trip. Uncertainty is reported
with every run. Webview `performance.now()` is clamped to 1 ms here, which floors it.

"Presented" is a double `requestAnimationFrame`, since the first fires *before* its frame is
painted. That makes the last number an upper bound: the true present is somewhere between
the two callbacks.

Startup must not be measured under `tauri dev` — the frontend is served by a Vite dev server
over HTTP there, so the numbers describe Vite. All of the above is a release bundle.

### Virtual scrolling

The frontend never mounts 500 page elements. A windowed scroller recycles a handful of
page containers. Accessibility constrains this design (§8) and must be settled before it is
built, not after.

The first draft had geometry "computed up front from the page size table so the scrollbar is
correct from the first frame". The startup measurement above kills that: building the table
costs 86 ms on a 775-page document and is the single largest avoidable item in the budget.
Geometry is therefore lazy — the scroller estimates total height from the pages it has
loaded and corrects as it learns more. A scrollbar that settles within the first few hundred
milliseconds is a far better trade than one that is exact but arrives 86 ms late, and page
sizes within a document are overwhelmingly uniform, so the estimate is usually exact
immediately. Documents with mixed page sizes are where it visibly adjusts, and that is the
case to design the correction behaviour around.

### If the webview is not fast enough

Escalation path, in order: `OffscreenCanvas` in a worker → WebGL/WebGPU tile compositing →
a native `wgpu` surface under the webview with the webview reduced to chrome. The last is
a real architectural cost (hit testing, coordinate mapping, window layering) and is a
fallback, not a plan.

---

## 5. Document model

The first draft proposed an immutable baseline plus an append-only command journal, with
undo as a pointer into the log. The audit correctly identified that this is a *journal*,
not a document model, and that it breaks in several places. Revised design:

### Three layers

1. **Baseline** — the file as loaded. Immutable.
2. **Working document** — a materialized, deterministically derived view of baseline +
   applied commands. This is what renders, searches, hit-tests and reports page geometry.
3. **Journal** — the command log, for undo/redo, recovery and save.

The working document is necessary because the first draft's "annotations render as an
overlay" only covers annotations. Page deletion, reordering, rotation, crop, form values
and text replacement all change rendering, extraction, search and geometry *immediately*,
long before save. An overlay cannot express them.

### Stable identity

Commands address **stable entity IDs**, never indices. `MovePage { from: 3, to: 7 }` is
invalid by construction — indices shift under other commands and the same journal replays
differently. Page order is expressed as operations over page IDs.

Required, and absent from the first draft:

- **Preconditions** on every command, checked at apply and at replay.
- **Tombstones** for deleted entities, so a later command targeting one fails explicitly
  rather than silently corrupting state.
- **Dependency invalidation** — `AddAnnotation(page)` followed by `DeletePage(page)` must
  define what happens to the annotation, and undoing the deletion must resurrect both page
  and annotation exactly.
- **Deterministic replay plus periodic snapshots**, since undo-by-pointer only works if
  the derived state can be rebuilt identically.
- Explicit handling of **shared state**: form fields whose value is shared across multiple
  widgets, and resources referenced by more than one page.

### Save, and rebasing after save

On save the journal is applied and written out. Afterwards **every PDF object identity has
changed** and the baseline digest is different, so the first draft's crash-recovery key no
longer matches its own file. Save therefore rebases: new baseline, regenerated stable-ID
mapping, compacted journal, updated recovery record.

### Save modes

Each command is classified into one of three modes, and the strictest one present wins:

| Mode | Meaning |
|------|---------|
| Incremental | Appendable as a PDF update section. Fast on large files; prior revision stays verifiable |
| Full rewrite | Requires complete reserialization (all redaction, structural sanitation) |
| Forbidden | Prohibited by a DocMDP certification signature on the document |

Incremental save remains genuinely valuable — appending to a 300 MB scan is near-instant
where a rewrite is not. But the first draft's claim that "existing digital signatures
survive" was wrong and is retracted: incremental save preserves a prior revision's
cryptographic integrity, which is **not** the same as the signature remaining valid and
trusted, and a certification signature may forbid the edit outright.

### External modification

The first draft keyed recovery on a file hash and had no story for live races. If another
process replaces the file while tpdf holds unsaved commands, saving would overwrite it or
replay commands against a different object graph. Required: retain file identity plus
size, mtime and baseline digest; recheck immediately before save; write to a temporary
file and atomically replace; on a changed baseline, require reload, save-as, or explicit
reconciliation.

---

## 6. Redaction

The highest-stakes subsystem. A redaction that leaks is worse than none, because it is a
confident lie. The audit was hardest on this section and largely right.

### Workflow: mark, review, apply, verify

1. **Mark.** Drag regions, select text, or pattern-search (emails, order numbers, a word
   list) and mark all hits. Marks are journal commands rendered as an overlay; nothing is
   destroyed and everything is undoable.
2. **Review.** Every mark listed with page, extracted text and thumbnail. The last chance
   to catch an over- or under-selection.
3. **Apply.** Destructive, full-rewrite, journal truncated at that point.
4. **Verify.** Mandatory. Reports *verified*, or *not verified* with specifics — never a
   bare success.

### Redaction is whole-graph sanitation

The first draft treated redaction as page-object surgery plus a metadata sweep. That is
not enough. The leaks are almost never in the obvious place.

Carriers that must be handled, expanded after the audit:

| Class | Carriers |
|-------|----------|
| Page content | Text, path and image objects; **nested Form XObjects**; transparency groups; tiling patterns; soft masks and image masks; alternate images; Type 3 font glyph procedures |
| Shadow text | Invisible OCR text layers; `/ActualText`, `/Alt` and `/E` in the structure tree; marked-content property lists |
| Annotations | Appearance streams, popup notes, **replies**, rich-text `/RC`, author/subject fields |
| Forms | Field values *and* default values, including widgets outside the redacted rectangle; AcroForm calculation and tab order; **XFA packets** |
| Document level | XMP and DocInfo metadata; outlines; page labels; page thumbnails; Names trees; `/OpenAction`, page actions, annotation actions; PieceInfo and application-private dictionaries |
| Attached content | Embedded files, associated files, portfolios/collections, RichMedia, sound, movie and 3D assets |
| Structural residue | Unreachable and orphaned objects that a serializer would preserve |

Two rules that fall out of this and were missing entirely:

- **Clone-on-write for shared resources.** An image XObject or pattern may be referenced
  by many pages. Editing it in place to redact one page silently alters every other page
  that shares it. Always clone, then edit the clone.
- **Deny by default.** Any object or stream type the sanitizer does not understand is a
  verification failure, not a shrug. Unknown constructs cannot be certified.

**XFA is out of scope.** It is a dead Adobe extension, it can carry a complete second copy
of the document's data, and sanitizing it properly is a project of its own. An XFA
document is refused for redaction with a clear message rather than silently
under-redacted.

### Partial-text redaction: the honest position

The first draft promised to split a partially-intersecting text object and re-emit the
surviving substring "preserving font, matrix, char/word spacing and colour". The audit is
right that this was hand-waving, and the reason is specific: `pdfium-render`'s
`set_text()` re-emits Unicode, which is **not** the same as preserving original glyph
codes, and PDFium exposes no getters for the original text-state (character and word
spacing, rise, horizontal scaling, writing mode, kerning within `TJ` arrays,
marked-content association).

Two viable routes, to be decided by a dedicated spike (§9), not asserted now:

- **A — operator rewriting.** Tokenize the content stream with `lopdf`, locate the
  text-showing operators covering the region, split the `TJ` array at glyph boundaries and
  re-emit with original codes and adjustments intact. Correct, and entirely our own code.
- **B — over-redaction.** Remove the whole text-showing operation containing any redacted
  glyph. Trivially safe, visibly destructive — it eats neighbouring words on the line.

Until a hostile corpus proves round-trip fidelity for A, B is the shipped behaviour.

### Flatten to image

The safe fallback for content that cannot be surgically redacted (partial vector path
intersections, unknown constructs). The first draft called it "unconditionally safe",
which it is not as described — replacing a page's `/Contents` with a raster leaves
annotations, widgets, thumbnails, structure data, XObjects and unreachable originals
untouched, and OCR run over a *pre-redaction* image reinstates the secret as invisible
text.

Correct form: build a **new page tree from sanitized raster pixels**, discard the original
object graph, copy only an explicit allowlist of metadata, and if OCR is applied, run it
only on already-redacted pixels with the redacted regions masked out.

### The full rewrite

A non-incremental save is necessary but not sufficient — a serializer can rewrite a file
while retaining unreachable objects, unused resources and copied streams, and an in-place
overwrite can leave trailing bytes past the new `%%EOF`.

Required: write a fresh temporary file from a **garbage-collected reachable object graph**,
explicitly not sourced from the original bytes; assert exactly one logical revision and no
trailing data; then atomically replace the target. This is the strongest argument for
QPDF, whose structural rewriting does exactly this GC and is Apache-2.0.

Stated plainly in the UI: this sanitizes the PDF file. It does not sanitize other copies,
backups, or recoverable filesystem sectors.

### Verification

The first draft verified by extracting text and scanning decompressed object streams for
the redacted strings. The audit's central objection is correct and disqualifying: **PDF
text is not stored as Unicode.** It is font-specific character codes, hex strings,
fragmented `TJ` operands, ligatures and custom CMaps — so a plaintext substring may never
appear in the file *even before redaction*, and the check would pass vacuously. In the
other direction, a legitimate remaining occurrence of a common word elsewhere would fail
it. String search is a smoke test, not a proof.

Verification is therefore **carrier-based**:

1. Build a manifest of every content carrier intersecting each redaction region before
   apply.
2. After apply, reopen the written file from disk and prove each manifest entry was
   removed or replaced.
3. Traverse the complete reachable object graph; every stream is decoded with **bounded**
   limits. Any undecodable, unsupported-filter, encrypted or limit-exceeded carrier is a
   hard failure.
4. Render the redacted regions and OCR them, confirming no legible text survives.
5. Re-parse with an independent parser (QPDF), so a single library's blind spot cannot
   certify itself.
6. Confirm one logical revision, no trailing bytes, no unreachable objects.
7. String search across decoded content, as a cheap additional smoke test only.

"Verified" means *every required check completed and passed*. If any check could not run,
the result is "not verified" and tpdf says so. Given the PDFium `GenerateContent` trap in
`AGENTS.md` — where a removed object silently survives into the file while the in-memory
API reports it gone — this pass is not belt-and-braces, it is load-bearing.

---

## 7. In-place text editing

Deliberately last, designed for from day one.

### Why it is hard

Embedded fonts are **subsetted**: the font programme in the PDF contains only the glyphs
already used. Type a character outside the subset and there is no glyph to draw.
Recovering requires locating the same font on the system, extracting the missing glyphs,
re-embedding an extended subset, and re-justifying with correct metrics. When Acrobat
mangles an edit, this is why.

### Approach

1. **Extract glyph runs** with per-character position, font, size, matrix and colour
   (`FPDFText_*`).
2. **Group glyphs into lines and blocks** by spacing and baseline heuristics. Edit quality
   lives here, and it is entirely our own code.
3. **Serve the embedded font to the webview** as an `@font-face` over the tile protocol,
   so the edit overlay renders in the document's *actual* font. This is the trick that
   makes editing look correct, and the main reason the text layer lives in the webview.
   Two caveats the first draft ignored: extracted subsets are frequently **not
   browser-loadable** without repair (CFF bare fonts, broken `cmap`, missing tables), and
   an embedded font's licensing bits may not permit re-serving it. Both are Phase 5
   feasibility questions, with a rasterized-preview fallback.
4. **Edit in an overlay** over the glyph run, with the underlying region suppressed.
5. **Commit** via operator rewriting (§6 route A) — the same machinery surgical redaction
   needs, which is why that spike comes first.
6. **Handle missing glyphs honestly.** Attempt system font matching and glyph
   re-embedding; failing that, substitute a metric-compatible font and **show the user
   which characters were substituted**. Silent substitution is what makes competitors
   untrustworthy.

### Scoping

- **First cut:** edit existing text where glyphs exist in the subset, plus system font
  matching with a visible warning where they do not. Within-block reflow.
- **Later:** paragraph reflow across lines, size and style changes, new text blocks in
  arbitrary fonts.

---

## 8. UX

Interaction borrowed from code editors, not office suites, because the stated pain is
discovery.

- **Command palette on Cmd/Ctrl+K.** The primary route to any command. Fuzzy search,
  recents first, and it displays each command's keybinding so it teaches shortcuts as a
  side effect. Phase 1 — it is the thesis, not a garnish.
- **Contextual actions, not modes.** Select text and highlight/copy/redact/comment appear
  at the selection; select an image and extract/replace/delete appear there.
- **One thin toolbar** — page navigation, zoom, search, sidebar toggle. Everything else in
  the palette or in context.
- **Keyboard-first,** every command bindable, Sumatra-familiar navigation.
- **No modal dialogs for routine work.**
- **Sidebar** with thumbnails, outline, annotations and search results as tabs.
- Dark and light themes following the system.

**Accessibility is an architectural constraint, not a later pass.** A canvas-rendered,
virtualized page list is inaccessible by default: there is no DOM text to read, and
recycling containers destroys focus. The screen-reader text representation, focus model
and command surface must be designed alongside the virtual scroller in Phase 1 — bolting
them on afterwards means rewriting it.

---

## 9. Roadmap

A phase is done when its exit criterion is met, not when the code is written.

### Phase 0 — Feasibility spikes

The first draft's Phase 0 tested only rendering, which cannot validate the architecture it
was meant to de-risk. Expanded to prove every load-bearing assumption, each on a corpus
that includes deliberately hostile files:

| Spike | Proves |
|-------|--------|
| Render pipeline | Tiles over the custom protocol; raw vs encoded transfer; tile size; CPU and peak allocation per tile on a dense CAD page; frame rate at 100% and 400% |
| Process architecture | Worker isolation, supervision, restart, resource limits, and the real IPC latency cost |
| Startup | The five timestamps of §4, cold and warm |
| **Text-object round trip** | Edit one existing text object and reproduce it faithfully. If this fails, surgical redaction and text editing are both off the table |
| **Sanitized full rewrite** | GC'd reachable-graph rewrite, verified by an independent parser |
| Incremental save | A real appended update section that other readers accept |
| Threat model | Written, with the sandbox policy it implies |

**Exit criterion:** first compositor presentation on a typical document under 300 ms
*warm*, no dropped frames on sustained scroll, and a documented verdict on each spike. A
failed spike changes the stack, which is why `AGENTS.md` marks the PDF layer provisional
until this completes.

The criterion was "under 300 ms cold" until the startup measurement showed cold start
carries ~300 ms of one-time code-signature validation before `main` runs (§4). Restating it
as warm is not moving the goalposts to make it passable — warm is currently **374 ms** and
therefore still failing. It is stating a bound tpdf can actually influence. First launch
after install or update is separately reported and never claimed to hit 300 ms.

### Phase 1 — The viewer

Open, scroll, zoom, rotate view, search-as-you-type, text selection and copy, thumbnails,
outline, print, file associations, session restore, dark mode, command palette,
accessibility architecture.

Two items here are quietly large and should not be estimated as viewer polish: **search
across a large multilingual document** with malformed encodings and custom CMaps, and
**cross-platform printing**.

**Exit criterion:** tpdf is the daily default for reading. If it is not, it is not
finished.

### Phase 2 — Editing foundation

Working document, stable-ID entity graph, journal with preconditions and tombstones,
undo/redo, snapshots, save-mode classification, incremental save, rebase-after-save, crash
recovery, external-modification handling.

Page operations: reorder by dragging thumbnails, rotate, delete, insert, extract, split,
merge, crop. Annotations: highlight, underline, strikeout, notes, ink, shapes, text boxes,
stamps — as real PDF annotation objects.

**Exit criterion:** a document can be marked up, saved, reopened in Acrobat and Preview,
and look right.

### Phase 3 — Redaction

The full subsystem of §6: whole-graph sanitation, clone-on-write, GC'd rewrite,
carrier-based verification, flatten-to-image, XFA refusal.

**Exit criterion:** verification passes on a corpus of deliberately nasty documents —
nested XObjects, shared resources, invisible OCR layers, structure-tree duplicates, hidden
OCG content, embedded attachments, prior incremental revisions — and correctly *refuses to
certify* the ones it cannot fully decode.

### Phase 4 — Forms and visual signatures

AcroForm filling with saved state, appearance stream regeneration, field inheritance,
shared widgets, form JavaScript policy (disabled by default), signature *image* placement.

Explicitly **not** cryptographic signing. XFA out of scope.

### Phase 5 — Text editing

§7, scoped as described there. Depends on the Phase 0 text round-trip spike and the
operator-rewriting machinery built in Phase 3.

### Phase 6 — Cryptographic signing

A separate subsystem, not an extension of Phase 4: trust stores, certificate selection,
timestamping, revocation, long-term validation, and DocMDP enforcement. PDFium's signature
API is read-only, so this needs its own crypto stack.

### Cross-cutting

OCR (feeding search, selection and redaction verification) has interfaces defined in
Phase 1 even though implementation lands later. Localization as it becomes binding.

---

## 10. Open questions

Each needs a measurement or a decision. The first draft listed five; the audit was right
that it presented several genuinely unresolved questions as settled architecture.

1. **Can PDFium round-trip a text object faithfully?** Phase 0. Gates both surgical
   redaction and text editing.
2. ~~**Protocol** — custom scheme vs alternatives.~~ **Answered 2026-07-26.** 1024²–2048²
   tiles (§4), sent as raw pixels (§3), over the custom scheme. Delivery costs 240–293% of
   rendering, so the scheme is not a zero-copy fast path — but encoding is worse in both
   directions, and on the startup path the whole transfer-and-decode of a 16.6 MB tile is
   8.2 ms against a 374 ms budget. Not where the time goes.
3. **How much of the 237 ms shell cost is reducible?** Now the dominant term in startup
   (§4): 142 ms from `main` to the Tauri setup callback, and 95 ms more before the webview
   has mounted a Svelte component. Neither has been attributed further. If most of the
   142 ms is WebKit framework initialization it is a floor, and 300 ms warm needs the page
   to paint before the framework finishes booting — which is an architecture, not a tuning
   pass. Measure before choosing.
4. **Can `lopdf` safely rewrite a hostile corpus,** or is QPDF required for the rewrite
   path? Affects the dependency set.
5. **Worker process count and IPC cost** — does multi-process rendering actually meet the
   latency target, given the boundary crossing it adds?
6. **Are extracted font subsets browser-loadable,** and does their licensing permit
   re-serving them? Gates the §7 `@font-face` approach.
7. **PDFium binary distribution.** Prebuilt from `bblanchon/pdfium-binaries`, dynamically
   linked and bundled, is pragmatic; static linking means building PDFium. Bundling has
   macOS notarization and signing implications that bit `screenpick`'s release path and
   need settling early. ~10 MB per platform, so ~25 MB total against Acrobat's gigabyte.
8. **Where the annotation overlay lives.** Frontend drawing gives 60 fps manipulation but
   means two rendering paths that can diverge visually; round-tripping through PDFium on
   every change is correct but slow. Likely: frontend while editing, PDFium on commit,
   with a visual regression test asserting they agree.
9. **Can redaction ever certify a document containing constructs the sanitizer does not
   understand?** Current answer is no, by design — but the refusal rate on a real corpus
   determines whether that is usable or merely principled.
