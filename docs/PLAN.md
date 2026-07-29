# tpdf — Architecture and Roadmap

Status: **Phase 0 closed; Phase 1 in progress.** The viewer runs --- sandboxed worker
pool, virtual scroller, selection, find, outline, page strip, session restore and
printing --- on macOS arm64. Windows has never been built. Editing, annotations, forms and
redaction are not started.

This document records the design and the reasoning behind it, so that decisions can be
revisited on their merits rather than re-argued from scratch. Sections written before a
thing was built still describe it in the future tense; §9 carries the verdict on each
Phase 0 question, and `AGENTS.md` carries what is settled.

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
- **Parallelism.** Concurrent in-process PDFium calls are undefined behaviour --- upstream
  gives no thread-safety guarantee and recommends parallel processes over threads, and
  `pdfium-render`'s `thread_safe` feature does not serialize them despite its README
  saying so (measured 2026-07-27; see AGENTS.md). Two threads rendering a complex page
  segfault. In-process parallel rendering is therefore not merely awkward, it is unsafe.

The first draft proposed "N document handles over one shared buffer, rendered in
parallel". That does not work, and it is recorded in `AGENTS.md` as a corrected error.

Retrofitting a process boundary later is a rewrite, so it is Phase 0 work.

### Worker processes — measured 2026-07-26

`worker-bench` builds the shape above and measures it. The binary is both halves: the
parent spawns copies of itself through `current_exe()`, which is what works inside a
signed `.app`. Three commitments were tested rather than assumed, and all three hold.

**The boundary is free.** A bare control round trip — write a JSON line, wake the worker,
read a JSON line — is **6 µs**. Delivering a 1024² tile costs, per variant, on a 775-page
text document at 1.5×, five interleaved rounds of twenty renders each:

| variant | end to end | render | swizzle | parent reads it | transport |
|---|---|---|---|---|---|
| in-process | 2.54 ms | 1.39 | 0.27 | 0.77 | 0.11 |
| worker, pipe | 3.07 ms | 1.35 | 0.27 | 0.84 | 0.62 |
| worker, shared memory | 2.57 ms | 1.35 | 0.27 | 0.83 | 0.12 |

Every variant folds all 4 MB in the parent, timed separately, so none can look cheap by
never reading what it received. Net of that, **moving a 4 MB tile out of a worker costs
0.11 ms through shared memory and 0.61 ms through a pipe** — and the shared-memory figure
is indistinguishable from the in-process residual, i.e. from zero. PDFium renders straight
into the shared mapping through `PdfBitmap::from_bytes`, so there is no copy on the worker
side at all; the pipe carries only the JSON line.

Worth stating against §3's other number: handing the same 4 MB to the *webview* costs
3.0 ms. **The webview boundary is ~27× more expensive than the process boundary.** The
architecture pays almost nothing for isolation, and quite a lot for the UI layer.

**Parallelism is real, and it stops at the performance cores.** 96 pages, best of four
interleaved rounds, on a 4P + 6E machine:

| workers | pages/s | speedup |
|---|---|---|
| in-process baseline | 384 | 1.00× |
| 1 | 402 | 1.05× |
| 2 | 787 | 2.05× |
| 3 | 1161 | 3.02× |
| 4 | 1495 | 3.89× |
| 6 | 1775 | 4.62× |
| 8 | 2052 | 5.34× |
| 10 | 2237 | 5.83× |

Near-linear to four, then each further worker buys about 0.4×. So the default pool size is
the **performance**-core count, not `hw.ncpu`; the efficiency cores are worth having for
background work (thumbnails, search indexing) and not for latency-critical tiles. One
worker matches the in-process baseline, confirming the boundary costs nothing at this
granularity.

**Crashes are contained and cheap to recover from.** A worker killed by SIGABRT, by
SIGSEGV, or exiting non-zero is noticed by the parent within **0.1–0.6 ms** — as an EOF on
the control channel, which is why the channel is line-delimited — and respawning it,
reopening the 775-page document and rendering the first tile takes **8.5–12.9 ms** total.
The parent is unaffected in every case.

**A runaway render costs one process.** The 250 ms deadline test on the A0 CAD page:
kill-to-reaped **1.2 ms**, respawn-to-ready **4.8 ms**. Note what this does *not* give —
the killed work cannot be resumed, so this is the blunt fallback. PDFium's progressive API
(`IFSDK_PAUSE`) is the cooperative route and is still unexercised.

**Resource limits are half-available on macOS, and the missing half is the important one.**
`setrlimit` refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright with `EINVAL`
(verified independently through Python's `resource` module), so **there is no memory bound
via rlimits at all**. `RLIMIT_CPU` works: SIGXCPU fires and kills the worker. `RLIMIT_NOFILE`
and `RLIMIT_FSIZE 0` are accepted and cost nothing — a worker that cannot create a file
cannot be talked into writing one.

But `RLIMIT_CPU` is a *process-lifetime* budget, not a per-request one. Measured: under a
3 s limit, one 1.72 s render succeeds and the next dies 1.30 s in, at a cumulative 3.0 s.
So it bounds a worker's total life, and per-render bounds have to come from the parent's
deadline-and-kill.

### Bounding memory without a kernel limit — measured 2026-07-26

The gap left above is closed as far as it can be, and the shape of what remains is the
interesting part. `worker-bench --mode footprint` has the parent sample the worker's
`ri_phys_footprint` through `proc_pid_rusage` and kill it over budget. A sample costs
**0.33 µs**, so the poll interval is a pure trade of overshoot against nothing. Against a
child taking memory as fast as the allocator hands it over — about 22 GB/s, far faster than
any document — and a 128 MB budget, five interleaved rounds:

| poll | overshoot, median | worst seen | what the interval bounds | poll costs | bursts missed |
|---|---|---|---|---|---|
| 0 ms | 0.0 MB | 0.0 MB | — | 100% of a core | 0/5 |
| 1 ms | 16.4 MB | 18.3 MB | 22 MB | 0.033% of a core | 0/5 |
| 5 ms | 22.4 MB | 88.7 MB | 113 MB | 0.007% of a core | 0/5 |
| 20 ms | 225.8 MB | 225.8 MB | 280 MB | 0.002% of a core | 4/5 |
| 50 ms | 368.7 MB | 368.7 MB | 483 MB | 0.001% of a core | 4/5 |

Every worker the parent caught, it killed. **The last column is the result that matters:**
at 20 ms and above, most runs never saw the event at all — the child took its full 512 MB
burst and exited between two samples. Polling bounds a sustained leak; it does not bound a
burst. So supervision cannot be the only memory defence, and bounding the *inputs* —
decompressed stream size, tile dimensions, pages per request — is not optional.

Three smaller things worth carrying. Neither the median nor the worst *observed* overshoot
is the bound — both depend on where the crossing happens to fall between two samples, so a
budget has to be set from interval × growth rate. A zero interval is not free supervision;
it burns a core, and its low overshoot is partly bought by starving the child. And a
footprint excludes clean file-backed pages, which is exactly why it is the right number: a
worker with a 337 MB document mapped is not consuming 337 MB of anything scarce, and a
bound on RSS would kill it for reading its input. A worker at rest is **2.2 MB**, +0.3 MB
to open the 775-page document, +1.0 MB after ten 1024² tiles.

A pool of N workers is therefore budgeted at (per-worker budget + bounded overshoot) × N.
That overshoot term is the price of having no kernel limit, and it should vanish on
Windows, where a job object with `JOB_OBJECT_LIMIT_PROCESS_MEMORY` is a real bound needing
no polling at all.

**The worker can be denied the filesystem and the network and still render correctly — but
only under a profile that had to be found by bisection.** With `sandbox_init`, file reads,
file writes and socket binds are all denied, and the document still opens, because it
arrives as a mapped descriptor and never as a path.

The trap is what "still renders" means. On a document with **embedded** fonts, a
deny-everything profile is pixel-identical to an unsandboxed worker. On a **base-14**
document it is not: PDFium returns success and produces different pixels, with almost the
same amount of ink — a silently substituted face, not a blank page. The obvious repair is
wrong too: denying `file-read*` and allowing `file-read*` back on `/System/Library/Fonts`
still renders differently. What restores it is allowing `file-read-metadata` *globally* and
`file-read-data` only under the font directories, which is what `PROFILE_WORKER` in the
harness does, verified pixel-identical on base-14, TrueType, CID and the 775-page corpus.

Two things follow. The residual authority is that a hostile document can learn which paths
exist; it cannot read one, write one, or open a socket. And a sandbox has to be verified by
**comparing pixels**, never by checking that the render returned `ok` — this is the same
silent-substitution failure mode §6 found in `set_text()`, arriving from a completely
different direction.

Windows is untested. Job objects, restricted tokens and named section objects share no
mechanism with any of this, and it needs its own spike before the shape can be called
cross-platform.

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
that thread's throughput, and the render thread is already the only place PDFium may be
called from at all.

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
   render, occupying the one thread PDFium may be called from throughout. This is the
   strongest empirical argument for worker processes; see §3.
4. **Progressive rendering is mandatory, not an optimization.** A 1 s floor per call means
   cancellation has to work *inside* a render, which is what the `IFSDK_PAUSE` path is
   for.

Peak RSS: 211 MB for one A0 page, 70 MB for the 775-page text document.

### Two-tier cache

- **Tier 1, permanent:** every page gets a cheap low-resolution bitmap (~150 px wide),
  rendered once, kept for the session. Doubles as the thumbnail — the sidebar's page strip
  renders at exactly this width and borrows from this cache rather than rendering again.
  It does not *add* to it: permanent plus one entry per page is 98 MB on the 775-page
  corpus, for pages nobody has opened.
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

Backstop for the genuinely pathological: a per-request deadline, and worker process
termination when it is exceeded. Process isolation makes that cheap and safe —
measured at 1.2 ms to kill and reap and 4.8 ms to respawn (§3). **Wired 2026-07-29**: the
pool's supervisor (`workers.rs`) kills any worker whose request has outrun `TPDF_CALL_MS`
(default 30 s) and the caller gets an error rather than a wait. The budget is enforced by
the parent's deadline and not by `RLIMIT_CPU`, which is a process-lifetime budget rather
than a per-request one.

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

#### Cold start is 1.56 s, and it is not the PDF layer's fault either

Measured with the page cache evicted before every run (`scripts/startup_bench.py --purge`,
6 runs, median; spread 1534–1585 ms). Same document and preview as the warm table:

| milestone | cold ms | Δ | warm Δ |
|---|---|---|---|
| main entry | 217.4 | 217.4 | 4.2 |
| tauri setup | 1168.4 | **951.1** | 141.9 |
| pdfium bound | 1172.3 | 3.9 | 0.8 |
| webview script start | 1397.4 | 225.1 | 47.6 |
| app mounted | 1410.4 | 13.0 | 47.0 |
| document parsed | 1429.7 | 2.3 | 0.6 |
| document open complete | 1516.2 | **86.5** | 85.9 |
| first tile rendered | 1524.6 | 8.4 | 7.2 |
| first preview bitmap ready | 1531.4 | 6.8 | 8.2 |
| first page presented | **1562.4** | 31.0 | 26.6 |

**One interval accounts for the entire cold penalty.** `main` to the Tauri setup callback
goes from 142 ms warm to 951 ms cold — 809 ms of paging Tauri and WebKit off disk. Together
with 217 ms of pre-`main` paging and 225 ms to reach the first line of webview script,
**1.17 s of the 1.56 s is spent before the frontend can execute anything at all.**

Two secondary results are worth having:

- **Page geometry enumeration is 86.5 ms cold against 85.9 ms warm** — identical, so it is
  pure CPU, not I/O. It cannot be cached away or prefetched. The only fix is not doing it,
  which is what the lazy-geometry change below does.
- **Binding the ~10 MB Pdfium dylib costs 3.9 ms cold.** The dependency that looks heaviest
  is not a startup cost at all, warm or cold.

#### First launch after install or update is a third, separate number

Distinct from cold cache. First-ever launch of a freshly built bundle spends **444 ms before
`main`**; the same binary relaunched with a warm cache spends 4.2 ms. That gap is not paging:
copying the bundle to a new path — same bytes, same warm cache, only the file identity is
new — reproduces it at 299 ms, and relaunching *that* copy costs 4.8 ms. It is the OS
validating a code signature it has not seen before.

The two effects are roughly additive, which the numbers bear out: 217 ms of cold paging plus
~230 ms of first-time validation is the 444 ms actually observed on the first run after a
build. So there are three startup regimes, not two, and they should be reported as such:

| regime | to first page | when the user meets it |
|---|---|---|
| warm | 374 ms | every ordinary launch |
| cold cache | 1562 ms | first launch after boot |
| cold + new identity | ~1.8 s (444 ms pre-`main` observed) | first launch after install or update |

The signature cost is charged **per binary identity, once**, and is spent before tpdf runs
at all — no budget can cover it. Hence the target is stated as *warm*, with the other two
regimes measured and reported rather than quietly folded in.

#### Where the shell cost actually is — measured 2026-07-26

The tables above left 358 ms of the 374 attributed only to "Tauri and WebKit", which is
not an attribution. §10 q3 asked how much of it is reducible and named two candidate
levers: lazy framework loading, and a smaller initial webview payload. Both were guesses.

Finer marks were added on the Rust side (`context built`, `app built`, `event loop ready`,
and an explicitly timed window build) and on the webview side (`first ipc returned`,
`clock calibrated`, plus the navigation-timing milestones), and five variants were run
**interleaved** — one launch of each per round, compared pairwise within a round, since
wall clock drifts more over a few minutes than most of the differences being looked for.

The 142 ms from `main` to the setup hook splits three ways. Tauri creates the windows
listed in `tauri.conf.json` *before* calling the setup hook, so webview creation was inside
that interval and no mark could be placed between them; clearing the config windows and
building the window inside the hook instead separates them:

| interval | warm ms | what is in it |
|---|---|---|
| exec → `main` | 4.4 | dyld, framework linking |
| → context built | 0.2 | embedded config, asset table |
| → app built | 29.9 | `Builder::build()` |
| → tauri setup | 25.2 | `App::run` prologue |
| → window built | 77.6 | `WebviewWindowBuilder::build()` — the WKWebView |
| → webview script start | 57.8 | HTML load, first inline script |
| → first ipc returned | 48.0 | see below |

**Not one of those seven intervals is ours.** The first line of application code runs at
~247 ms, and everything before it is the shell reaching the point where it can be asked
for anything.

The variants, against a 367.6 ms baseline (three independent runs of 8 rounds; the columns
are the median of within-round differences, so drift cannot be attributed to a variant):

| variant | what it changes | run 1 | run 2 | run 3 |
|---|---|---|---|---|
| `manual` | window built in the setup hook, not from config | +1.3 | — | −0.2 |
| `blank` | same work, no framework at all | −8.4 | +9.9 | −0.2 |
| `lazy` | page geometry for page 1 only | −82.8 | −85.7 | −75.0 |
| `eager` | document opened during shell boot | −77.3 | −85.5 | −74.1 |
| `lazy` + `eager` | both | −76.6 | −84.3 | — |
| `no-menu` | Tauri's default macOS menu replaced by an empty one | — | — | −9.0 |
| `lazy` + `no-menu` | both | — | — | −92.5 |

**The frontend payload is worth nothing measurable.** `blank` is a page that opens the
document, renders the tile and waits for the compositor using raw `__TAURI_INTERNALS__`
calls in one inline script — no module graph, no Svelte, no `@tauri-apps/api`. Deleting
the entire framework moved startup by −8 ms in one run and +10 ms in the other, i.e. by
nothing.

The reason is worth having, because it is not "the framework is fast". The baseline spends
47 ms between its first script and its mount, and its *first IPC then costs 0.0 ms*. The
blank page has no module to fetch, is fully loaded 39 ms earlier — and its first IPC costs
**43.9 ms**. Both pay the same toll through different doors: **the webview's first request
over a Tauri custom protocol costs ~45 ms**, and whichever request happens to be first pays
it. A smaller payload does not avoid it; it only changes which line of the table it lands
on. The lever named in q3 does not exist.

Nor does the other one. Moving window creation out of the config and into the setup hook
changes the total by +1.3 ms — the cost is the WKWebView, not when it is asked for.

**What was reducible was ours.** Collecting every page's geometry — 86 ms on the 775-page
document, and the one item §4 already called self-inflicted — is worth 83–86 ms, and
removing it is what takes a 374 ms warm start to **290 ms**. There are two ways to remove
it and they are alternatives rather than complements, which the `applied` row shows: doing
it lazily and doing it during the shell's boot both delete the same 86 ms, so doing both
buys nothing over doing either. Lazy is the better of the two — it is a smaller change, it
holds for a document opened later in the session, and it does not depend on knowing the
path before the window exists.

**One shell cost is ours to choose, and it is the default menu.** Tauri installs a full
macOS application menu unless given one. Replacing it with an empty `Menu::new` is worth
**16 ms** — measured against `lazy` pairwise, negative in all seven rounds, −7.7 to −41.9 —
and it does not sit where it looks like it should: `Builder::build()` drops 39.7 → 32.9 and
the `App::run` prologue drops 94.0 → 87.8, so it is split across both. Against the noisier
baseline the same variant reads −9.0 with a range spanning zero, which is the same effect
buried in a wider distribution; the low-variance pairing is the one to believe.

An empty menu is not shippable — Cmd-Q, Cmd-W and clipboard shortcuts come from it — so the
finding is not "ship without a menu" but **"build the menu tpdf needs rather than accepting
the default"**, and expect around 10–16 ms for it.

Three things follow for the architecture:

- **The shell floor is ~250 ms warm on this machine**, and the only route below it is to
  present the first page without waiting for the webview at all — the native-surface
  escalation at the end of §4, which is an architecture, not a tuning pass. Nothing short
  of that touches it.
- **The budget available to tpdf is ~50 ms**, and the current spend is about 45: 1 ms to
  open lazily, 7 to render the tile, 10 to transfer and decode, 26 for the compositor. That
  is the number to defend, and it means a regression of 50 ms in our own code is a missed
  target with no headroom anywhere else to absorb it.
- **The best measured configuration is 276 ms warm** (`lazy` + `no-menu`, median of 8,
  250–285), against 368 for the shipped shape.

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

The run cannot time itself out, and finding that out cost an hour. WebKit suspends a page
whose window is not visible — behind a lock screen, or on a display that has gone dark,
every window qualifies — and the suspension stops `requestAnimationFrame` *and*
`setTimeout`. A JavaScript watchdog is therefore suspended alongside the thing it was
watching, and the launch sits in its event loop producing no output at all, which reads
exactly like a slow machine. Three things guard it now: `startup_bench.py` refuses to start
when the session is locked, holds `caffeinate -du` for its own lifetime (`-d` alone will not
turn a display that is already off back *on*), and the app carries a Rust-side watchdog that
prints the marks it did reach and exits 2. That last one is what identified this: the
timeline stopped at `first preview bitmap ready` every time, with only presentation missing.

### Sustained scroll — measured 2026-07-26

The other half of the Phase 0 exit criterion: "no dropped frames on sustained scroll", at
100% and 400%. `src/lib/scroller.ts` is this section in miniature — two-tier cache, a tile
window with one screen of prefetch, a client-side supersedable queue, an LRU bound — and
`src/lib/scrollbench.ts` scrolls it 30 CSS px per frame (~1800 px/s) for 300 frames, five
interleaved rounds per variant, on a release bundle. Tier 2 is cleared between rounds, so
every round scrolls over content that has not been rendered yet.

**The webview presents at 59 Hz on a 120 Hz panel.** An idle animation loop is timed first,
because a drop threshold derived from an assumed 60 Hz is a guess rather than a
measurement. It comes back at **17.0 ms median** against a display in a 120.00 Hz mode.
That is not the battery — it was re-measured on AC — and not an occluded or unfocused
window, both of which the run asserts in its own header. So tpdf's frame budget inside a
webview is 16.7 ms rather than the 8.3 ms this machine can present at. It is the same shape
as the ~250 ms startup floor: a ceiling the shell imposes, not one we can tune.

Against that budget, on the 775-page text document:

| variant | fps | median | p99 | max | drops | stalls | main thread | sharp |
|---|---|---|---|---|---|---|---|---|
| tiles 100% | 60.1 | 17.0 | 18.0 | 21.0 | 0 | 0 | 0.64 ms | 100% |
| tiles 400% | 60.2 | 17.0 | 19.0 | 25.0 | 0 | 0 | 0.51 ms | 100% |
| viewport 100% | 60.0 | 17.0 | 18.0 | 20.0 | 0 | 0 | 0.11 ms | 100% |
| viewport 400% | 59.8 | 17.0 | 18.0 | 68.0 | 2 | 2 | 0.16 ms | 100% |

**No dropped frames**, at either zoom, while sustaining 46 MB/s of tiles at 400% with the
visible page fully sharp. The single 68 ms outlier is two frames in one round out of five
and did not reproduce in a re-run. Our own per-frame work is 0.1–0.6 ms against 16.7 —
between 1% and 4% of the budget.

**And on the hard page that result is very nearly vacuous.** The same four variants on the
A0 vector sheet:

| variant | fps | max | drops | sharp | any |
|---|---|---|---|---|---|
| tiles 100% | 60.0 | 18.0 | 0 | **4%** | 100% |
| tiles 400% | 60.0 | 18.0 | 0 | **0%** | 80% |
| viewport 100% | 60.0 | 18.0 | 0 | **3%** | 93% |
| viewport 400% | 60.0 | 18.0 | 0 | **0%** | 80% |

Sixty frames a second, every frame, over a page that is essentially blank. The warm-up ran
its full three-second cap without filling the first screen — 10% sharp at 100%, 0% at 400%
— and the timed scroll never recovered. `any` is the tier-1 placeholder, so at 400% roughly
a fifth of frames showed *nothing at all*: §4's promise that the user "never sees a white
rectangle" is false on precisely the page where it was supposed to matter, because tier 1
costs 1.5 s there too. Meanwhile the renderer consumed ~4.0 s of PDFium time per 5 s round
and had one to two of every four tiles discarded as superseded before they could be drawn.

This is the architecture working as designed and the design not being enough. The frame
loop is decoupled from the renderer, which is why it never stutters — and it is exactly why
frame rate alone cannot tell a viewer that keeps up from one that has given up. Doubling
the tile to 2048² changes 3% to 4%, i.e. nothing; §4's "fewest, largest tiles" is not
contradicted, it simply cannot rescue a page with a one-second floor per render call. What
would move this number is the worker pool — but by less than §3's 3.89× suggested, and it
was measured directly on 2026-07-27 rather than extrapolated. Six tiles covering one
screenful of this page at 2× cost **8.19 s in-process and 2.55 s on six worker processes**,
a 3.2× ceiling that is reached at six workers and does not improve at eight
(`worker-bench --mode parallel --grid 3`). So the pool takes a screenful from eight seconds
to two and a half, which is a real improvement and still not a scroll. The real answers for
such a page are the progressive API and an honest degraded state, not a faster tile.

**The two layouts are both far inside budget, and the surprise is which is cheaper.** One
viewport-sized canvas redrawn every frame costs **0.11–0.17 ms** of main thread; a canvas
per tile in a natively scrolling container costs **0.49–0.64 ms**. Assigning `scrollTop`
against a container of absolutely positioned canvases is a forced layout, and it is more
expensive than compositing half a dozen GPU-resident ImageBitmaps into one canvas. Neither
is close to mattering, so the escalation path at the end of this section is not needed. The
webview is not the bottleneck; PDFium is.

That said "the choice can be made on other grounds" — and by 2026-07-27 there were grounds.
Across roughly 3,300 timed frames on the text corpus, the per-tile-canvas layout dropped
**three frames and stalled once**; the single-canvas layout dropped **none** in the same
runs, at identical 100% coverage. Both are rare enough that no single run separates them,
which is why it took several. A dropped frame is the thing the criterion is actually about,
so the cheaper layout is also the one that misses less, and `viewport` should be the default
rather than the fallback. The drops are consistent with where the cost sits: `tiles` pays
its main-thread time creating a canvas and appending it to the DOM as each tile *arrives*,
which is layer-tree work partly outside our own callback and therefore outside the number
above.

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

### Incremental save — measured 2026-07-26

Spike 0.6 (`src-tauri/src/bin/incremental_save.rs`) writes an update section with `lopdf`
and puts it to four independent parsers — PDFium, QPDF 12.3, poppler, and CoreGraphics,
i.e. what Preview and Quick Look use. Each is asked for something falsifiable rather than
for acceptance: QPDF for a structural check, poppler for the text of the edited page,
CoreGraphics and PDFium for its pixels. **Twelve fixtures pass on all four**, including a
document whose page dictionary lives inside an `/ObjStm` behind a cross-reference stream,
and one that already had two revisions.

The update section is 500–800 bytes regardless of document size, the original bytes are
preserved exactly, and `/Prev` chains to the previous `startxref` in every case. The
appended cross-reference keeps the previous revision's *form* — a table stays a table, a
stream stays a stream — which is required and is not automatic.

**The speed claim is true, but only once the file is on disk.** In memory a full rewrite
of a 336 MB scan costs 12.4 ms against the append's 12.3 ms, because `lopdf`'s rewrite is
essentially a copy and the machine has the bandwidth for it. The distinction only appears
when the save actually lands:

| fixture | size | append | rewrite | ratio | bytes written |
|---|---|---|---|---|---|
| text-base14 | 0.9 KB | 3.02 ms | 3.16 ms | 1.0× | 672 vs 847 |
| text-heavy | 1.4 MB | 4.77 ms | 6.85 ms | 1.4× | 812 vs 1,344,570 |
| scan, 5 pages | 42 MB | 5.60 ms | 26.7 ms | 4.8× | 708 vs 42,078,102 |
| scan, 20 pages | 168 MB | 13.6 ms | 96.3 ms | 7.1× | 714 vs 168,311,790 |
| scan, 40 pages | 337 MB | 29.1 ms | 239 ms | 8.2× | 723 vs 336,623,496 |

Three things follow. **Below a few megabytes there is no reason to prefer either** — both
are dominated by one `F_FULLFSYNC`, about 3 ms on this machine. **The append's remaining
cost is parsing**, not writing: 5.7 ms of the 29 ms at 337 MB is `lopdf` reading the
document, which any edit must pay to know what it is editing. And **the rewrite needs room
for two copies** of the document while it runs, which the ratio does not show at all.

**Encryption is preserved, including the ciphertext.** The trap from §6 — `lopdf` silently
writing plaintext on a full rewrite — does not recur on the incremental path: the appended
objects are encrypted with the original key, verified by searching the update section for
a known needle in the clear. A document behind a real user password is refused without
one; a document with an *empty* user password is edited without prompting, which is
correct, because it opens unprompted in every reader.

**Signatures: the cryptography survives and the trust does not.** Measured against pyhanko
on an approval signature and on DocMDP levels 1, 2 and 3:

- `intact=yes valid=yes` after every append. The signed bytes are untouched, so the CMS
  digest still verifies. This is the kernel of truth in the retracted claim.
- Coverage drops from `ENTIRE_FILE` to `ENTIRE_REVISION` and the modification level is
  `OTHER` in every single case. Every validator will report the document as changed after
  signing.
- The difference analysis rejects **every** edit at **every** level, including an
  annotation-only edit to a level-3 certified document, which the specification permits.

That last one is not a defect in the append. Reducing the edit to its minimum — extending
an `/Annots` array that is its own object, so the page dictionary is never rewritten —
narrows the complaint to two objects and does not clear it, and pyhanko says why in its
own log: *"StandardDiffPolicy was not designed to support DocMDP level 3
(MDPPerm.ANNOTATE)."* So "DocMDP 3 permits annotations" is a statement about the format,
not about what validators do with it.

The design consequence is that **the three-mode classification needs a fourth answer, and
"Forbidden" is too narrow a name for it.** A signed document cannot be edited *and* keep
its signature trusted, whatever the DocMDP level says, so the UI must say that plainly and
offer to save a copy rather than implying the signature will be fine. What the append does
buy is real and worth keeping: the signed revision survives byte for byte inside the new
file, so a validator can still show exactly what was signed.

One document-shape dependency worth carrying into Phase 2: **whether `/Annots` is an
indirect array decides whether adding an annotation rewrites the page dictionary.** When
it is written inline there is no way to add an annotation without replacing a signed
structural object. Prefer extending the array object; when the producer inlined it, know
that the edit is larger than it looks.

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

### Text-object round trip — measured 2026-07-26

Spike 0.3, the gating one. Harness `src-tauri/src/bin/text_roundtrip.rs`, corpus
`testdata/make_text_pdf.py`: four single-page A4 fixtures, each with four text lines and
four unrelated non-text objects, differing only in how the text is encoded —

| fixture | font |
|---|---|
| `text-base14` | Helvetica, not embedded |
| `text-truetype` | embedded subsetted TrueType simple font, WinAnsiEncoding |
| `text-cid` | embedded subsetted CIDFontType2 under Type0 / Identity-H |
| `text-marked` | as `text-truetype`, plus carriers holding a second copy of the text |

Two routes, each asked to replace one text object and (separately) to delete it. The
measurement is **collateral damage**: device pixels differing from the untouched baseline
*outside* the edited object's own bounds, at 2x.

**The spike passes. Both routes changed zero pixels outside the target, on every fixture,
for both operations.** Surgical operator rewriting is feasible, so partial-text redaction
route A is not hand-waving and in-place text editing stays on the roadmap.

Four things it also established, each of which changes something:

**1. Zero pixel damage is not fidelity.** PDFium's `FPDFPage_GenerateContent()` did not
splice the one edited object — it rewrote every operator on the page. `Td` became `Tm`,
`Tj` became `TJ`, each text run was wrapped in `q`/`Q` with explicit `rg`, `RG` and `0 Tr`,
an ExtGState `/FXE1 gs` appeared, `/F1` was renamed `/FXF1` — and the marked-content span
was **discarded entirely, `/ActualText` with it**. The page rendered identically. So a
single unrelated edit destroys tagged structure, accessibility text and optional-content
membership while every pixel-based check passes. Surgical rewriting preserved all of it.
This is why §6's route A matters and why a visual regression test cannot be the only gate.

**2. `set_text()` fails silently on a subsetted font.** Asked for `QUARZ ÜBERPRÜFT` — whose
`Q`, `U`, `Z`, `Ü`, `P`, `F`, `T` are absent from the subset — PDFium returned success
every time, and:

- **Identity-H:** every missing character was encoded as **glyph 0**, `.notdef`. Renders
  `□□AR□ □BER□R□□T`. Extraction returns neither the old text nor the new.
- **TrueType simple:** correct WinAnsi codes were written for glyphs that do not exist,
  rendering `ARBERRT` — jammed together, because the missing codes carry zero advance —
  while text extraction returns `QUARZ ÜBERPRÜFT` in full. **Displayed text and extracted
  text disagree**, which is the worse of the two failures: a search hit on text nobody can
  see.

Route B refused instead: *"'Q' is not drawn by this object, so its code is unknown"*. That
is the behaviour §7 point 6 demands, and it comes free from working in code space — a
route that re-emits Unicode cannot detect the condition at all.

**3. A byte-level leak scan cannot verify a CID document,** and proved it by getting the
answer wrong. The harness's scanner reported the `text-cid` fixture *clean* while
extraction showed the needle still present, because Identity-H stores glyph ids and the
secret is never in the file as its own bytes. It now reports **not verified** on any
document containing a Type0 font rather than passing it. Generalisation worth carrying into
Phase 3: a verifier must decode each carrier in the carrier's own encoding, and "grep found
nothing" is not evidence.

**4. `/ActualText` survives surgical removal.** Deleting the show operator left the
marked-content property dictionary — and its verbatim copy of the line — in the content
stream. PDFium's regeneration happened to drop it, but as a side effect of destroying all
marked content, not as sanitation. Neither is a redaction. Confirms the §6 position that
redaction is whole-graph sanitation and not a page edit, with `/ActualText` as the cheapest
demonstration.

Ordinal mapping between PDFium's text objects and the content stream's show operators held
4:4 on all four fixtures, which is what let route B address the target at all. That is the
easy case and is not guaranteed — a `TJ` split across objects, or a Form XObject
contributing objects from another stream, breaks it. The harness checks the counts agree
and warns when they do not; real files need a stronger correspondence than ordinal.

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
   limits. An encrypted, limit-exceeded or unrecognised-filter carrier is a hard failure.
   A carrier in a *recognised image* filter is not — it is an image, and it is checked by
   step 4 rather than by a byte scan. Spike 0.4 measured why the distinction is necessary
   rather than fussy: without it, one `/DCTDecode` stream refuses the whole document, and
   that is every scanned page in existence.
4. Render the redacted regions and OCR them, confirming no legible text survives.
5. Re-parse with an independent parser (QPDF), so a single library's blind spot cannot
   certify itself.
6. Confirm one logical revision, no trailing bytes, no unreachable objects. The revision
   count is not bookkeeping: spike 0.4 showed an object a prior revision overwrote is
   reachable by no parser at all, so a multi-revision file cannot be certified by any of
   the checks above — only rewritten, and then certified.
7. String search across decoded content, as a cheap additional smoke test only.

"Verified" means *every required check completed and passed*. If any check could not run,
the result is "not verified" and tpdf says so. Given the PDFium `GenerateContent` trap in
`AGENTS.md` — where a removed object silently survives into the file while the in-memory
API reports it gone — this pass is not belt-and-braces, it is load-bearing.

### Sanitized full rewrite — measured 2026-07-26

Spike 0.4. Harness `src-tauri/src/bin/sanitize_rewrite.rs`, corpus
`testdata/make_hostile_pdf.py`: eleven fixtures, each hiding a distinct needle in a
different carrier, with `hostile-manifest.json` recording for each one whether a
reachability sweep is *expected* to clear it. Six routes, from a byte copy (the control)
through `lopdf` with and without collection to QPDF.

**The rewrite works, and `lopdf` is enough for it.** On every fixture, a collected `lopdf`
rewrite reaches exactly the same verdict as QPDF: the four unreachable-object classes go —
plain orphan, mutual-reference cycle, orphan inside an `/ObjStm`, orphan inside an
encrypted file — the four reachable ones stay, bytes past `%%EOF` disappear because the
file is written rather than patched, and all 775 pages of the bench document survive with
zero changed pixels. Open question 4 is answered: **QPDF is not required for the rewrite.**

Six things it established that change something:

**1. `lopdf`'s own collection is quadratic; the algorithm is not.** `prune_objects` and
`renumber_objects` both walk the graph via `traverse_objects`, which records what it has
seen in a `Vec` and calls `contains` before each push. Cost of collection over a plain
save, three rounds interleaved, warm:

| objects | plain save | `prune_objects` + renumber | our mark-and-sweep | QPDF |
|---|---|---|---|---|
| 2,445 | 4.6 ms | 8.3 ms | 3.6 ms | 14.1 ms |
| 7,758 | 12.5 ms | 95.2 ms | 10.5 ms | 34.6 ms |
| 25,583 | 66.6 ms | 1,480 ms | 70.3 ms | 175 ms |

A 3.3x larger graph costs 17x more to collect — and 25,583 objects is not a large document.
A mark-and-sweep over a hash set, thirty lines in the harness, is **indistinguishable from
not collecting at all** and produces byte-identical results. Renumbering is dropped with
it: contiguous object numbers are cosmetic and cost a second quadratic pass. So the shape
of the answer is *use `lopdf`, write the sweep ourselves* — not *take QPDF*.

**2. A superseded object is invisible to every parser.** The `stale` fixture carries two
secrets from a prior revision. The one the update *dropped from the page* is still in the
cross-reference table, so a graph walk sees it — and `lopdf` without collection duly leaked
it. The one the update *overwrote* is not: its bytes sit at their old offset with nothing
pointing at them, and no parser will ever hand them to a verifier. Only a byte scan finds
it, and only because the fixture leaves it uncompressed; compressed, nothing in the harness
could see it at all. Hence a rule this spike adds to §6's verification list: **more than one
revision in a file is a blind spot, not a detail.** The check is `%%EOF` count, and a file
with two of them cannot be certified — only rewritten and then certified.

**3. Taken literally, "every stream must decode or it is a hard failure" refuses almost
every scanned document.** The `filters` fixture draws ordinary page content through
`/ASCIIHexDecode` and an image through `/RunLengthDecode`. `lopdf` implements neither —
it supports Flate, LZW and ASCII85 and returns `Unimplemented` for the rest — so all six
routes report *not verified*, including QPDF's own output. The realistic cases are worse:
`/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and `/JPXDecode` are what scanners emit,
and every one of them lands in the same branch. This is the concrete form of open question
9, and the refusal rate under the naive rule is not "some documents", it is "most of them".
The rule needs splitting: a carrier whose encoding we understand but cannot decode is a
hard failure; an *image* carrier is not undecodable, it is an image, and belongs to the
render-and-OCR check rather than the byte scan. A filter we do not recognise at all remains
a refusal.

**4. `lopdf` silently drops encryption.** Handed an AES-256 file with an empty user
password — one that opens in any reader without a prompt — `lopdf` decrypts it on load and
writes plaintext on save, with no error and no warning. QPDF re-encrypts with the original
parameters. Removing a document's protection while claiming to sanitize it is its own
security failure, so the save path has to preserve encryption or refuse, never quietly
downgrade.

**5. A decompression bomb costs QPDF nearly two seconds of CPU for a 2.9 KB input.** QPDF's
default re-encodes stream data, which means fully decoding the `bomb` fixture's 1 GiB:
1.92 s, though only 8.4 MB resident, because it streams rather than buffers. `lopdf` with
`max_decompressed_size` refuses in 0.3 ms. `qpdf --stream-data=preserve` finishes in under
10 ms — but it does that by copying bytes it has not looked at, which is exactly what a
sanitizing rewrite must not do. The bound therefore belongs on the *rewriter*, not only on
the verifier, and hostile-input limits are a property of the rewrite path.

**6. A renderer that accepts a file has not checked it.** The independent-parser
requirement (point 5 above) earned its place on its first run: our own mark-and-sweep left
`/Size` claiming more objects than the file contained, because sweeping does not touch
`max_id`. PDFium rendered all six outputs of that fixture pixel-identically and raised
nothing; `qpdf --check` named it immediately. `lopdf`'s *own* plain save produced the same
defect on the encrypted fixture. PDFium is deliberately lenient about malformed files — that
is why it is the right renderer — which is precisely why it cannot be the structural check.

QPDF keeps a place for two things it does better, neither of them collection: it preserves
encryption, and `--object-streams=generate` shrank the 6.1 MB bench output to 1.46 MB.
Whether that is worth a C++ dependency is a Phase 3 decision, not a Phase 0 one.

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
| ~~**Render pipeline**~~ | **Passed 2026-07-26** (§3, §4). Raw pixels over the custom scheme, 1024²–2048² tiles, ~1 s fixed cost per render call on a dense CAD page, 211 MB peak. Sustained scroll holds 60 fps at 100% and 400% on both corpora with 0.1–0.6 ms of main thread per frame — but the webview presents at 59 Hz on a 120 Hz panel, and on the CAD page it holds that 60 fps over a blank screen |
| ~~**Process architecture**~~ | **Passed 2026-07-26** (§3), on macOS only. The boundary costs 6 µs of control latency and 0.11 ms to move a 4 MB tile through shared memory; four workers give 3.9× throughput; a crash is noticed in under a millisecond and recovered in ~10 ms; the worker renders correctly with files and network denied. Two gaps recorded rather than closed: macOS has no memory rlimit, and Windows is untested |
| ~~**Startup**~~ | **Passed 2026-07-26** (§4). Warm, cold and first-launch-after-build are three separate regimes, and the last two are the OS. The shell floor is ~250 ms before any application code runs and is not reducible by anything tpdf controls; the two avoidable items — our page-geometry walk and Tauri's default menu — are worth 92 ms together and take it to 276 ms warm |
| ~~**Text-object round trip**~~ | **Passed 2026-07-26** (§6). Both routes reproduce the page with zero collateral pixels; only the surgical route preserves marked content, and only it detects an out-of-subset character instead of silently drawing `.notdef` |
| ~~**Sanitized full rewrite**~~ | **Passed 2026-07-26** (§6). A collected `lopdf` rewrite matches QPDF on every fixture, so QPDF is not required — but `lopdf`'s own collection is quadratic and the sweep has to be ours, and "every stream must decode" would refuse most scanned documents |
| ~~**Incremental save**~~ | **Passed 2026-07-26** (§5). Twelve fixtures, four independent readers, each asked for pixels or text rather than for acceptance. The update section is under a kilobyte whatever the document weighs, encryption and its ciphertext survive, and on disk the append beats a full rewrite 8.2× at 337 MB — but not at all below a few MB, where one `fsync` dominates both. Signatures stay cryptographically intact and stop being trusted, at every DocMDP level |
| ~~**Threat model**~~ | **Written 2026-07-26** — `docs/THREAT-MODEL.md`, with the sandbox policy it implies. Two claims were promoted from policy to build property on the way: the vendored PDFium has no V8 and no XFA compiled in, so document JavaScript cannot run rather than being switched off. Memory bounding is answered as far as macOS allows and the residue is named — polling bounds a leak, not a burst |

**Exit criterion:** first compositor presentation on a typical document under 300 ms
*warm*, no dropped frames on sustained scroll, and a documented verdict on each spike. A
failed spike changes the stack, which is why `AGENTS.md` marks the PDF layer provisional
until this completes.

The criterion was "under 300 ms cold" until the startup measurement showed cold start
carries ~300 ms of one-time code-signature validation before `main` runs (§4). Restating it
as warm is not moving the goalposts to make it passable — it is stating a bound tpdf can
actually influence. First launch after install or update is separately reported and never
claimed to hit 300 ms.

Warm was **374 ms** and failing. With page geometry collected lazily — the change §4 had
already committed to for the scroller, for the same reason — it is **292 ms median, 284 to
300**. Replacing Tauri's default macOS menu as well takes it to **276 ms median, 250 to
285**, which clears the target on every launch of the run rather than only at the median.

That is the honest position: the criterion is met, with ~25 ms of margin, and every bit of
the margin comes from two changes that were already going to be made for other reasons. The
shell floor underneath is ~250 ms and nothing we do moves it.

The other half — no dropped frames on sustained scroll — **passes, and the pass is worth
less than it looks** (§4). Zero drops in every variant on both corpora, with our own work
taking 1–4% of the frame budget; but the same run holds a flawless 60 fps on the A0 page
while 0–4% of the visible area is sharp and, at 400%, a fifth of frames show nothing at
all. The criterion is satisfiable by a scroller that renders nothing, because the frame
loop is decoupled from the renderer by design. It is therefore restated with a floor on
what was actually on screen:

> **no dropped frames on sustained scroll, with the visible page area at least 95% sharp
> on a text document and never below the tier-1 placeholder on any document.**

The text corpus meets that at 100% sharp. **The A0 vector page did not, and was recorded as
a known failure** rather than smoothed over.

It was closed on 2026-07-27, by cancellable rendering and stale-request withdrawal rather
than by the worker pool — see Phase 1 below for the measurement, and read the closure
narrowly. What the criterion asks of that page is that it never falls below its tier-1
placeholder, and it does not: the worst single frame of the worst round shows 100% of the
visible page area backed by something, in all four layout-and-zoom variants, with no dropped
frames. What it is *not* is legible. Sharp coverage while scrolling that page is 6–10%, so
the honest description of a pass here is "a blurry page that never blinks out", and the
degraded state the UI owes the user is still owed.

#### Closed 2026-07-27

The phase is closed and the PDF layer is marked settled in `AGENTS.md`. Closing it meant
making the evidence reproducible, which it was not: `vendor/pdfium/` is gitignored and
nothing fetched it, so a clean clone could not run a single spike, and the pin that every
measurement above depends on existed only as an untracked file on one machine.
`scripts/fetch_pdfium.py` now installs `chromium/7881` by verified digest, and
`scripts/gates.py` holds the quality gates as one executable list rather than a checklist
to be kept in step by hand. There is deliberately no remote CI while the project is
pre-release and single-machine; when it arrives it should call `gates.py`.

Three things are carried forward rather than resolved, and none of them should be
rediscovered:

- ~~**The A0 vector page scrolls blank.**~~ Closed 2026-07-27 against the criterion as
  written — it never drops below its tier-1 placeholder — and not against what anyone would
  call a good experience, since it stays 6–10% sharp while moving. See Phase 1.
- **Windows is entirely unverified** --- no build, no gate run, no measurement --- and the
  tree does not currently compile there. `sanitize_rewrite.rs` and `tile_bench.rs` call
  `libc::getrusage` with no `cfg` gate; PDFium's loadable library is at `bin/pdfium.dll`
  rather than `lib/libpdfium.dylib`, which `pdfium_library_dir()` does not know; and the
  worker sandbox is `sandbox_init` SBPL, so the containment argument in
  `docs/THREAT-MODEL.md` is macOS-specific and needs its own answer there. `BUILD.md`
  keeps the list.
- ~~**There are no tests.**~~ Started 2026-07-27, with the request queue and the `tile://`
  parser --- 26 of them, each shown to fail against a deliberate mutation of the code it
  covers. Rendering is still asserted by the spike probes rather than by tests, which is the
  right split while it needs a PDF and a PDFium build to say anything.

### Phase 1 — The viewer

Open, scroll, zoom, rotate view, search-as-you-type, text selection and copy, thumbnails,
outline, print, file associations, session restore, dark mode, command palette,
accessibility architecture.

Two items here are quietly large and should not be estimated as viewer polish: **search
across a large multilingual document** with malformed encodings and custom CMaps, and
**cross-platform printing**.

**Exit criterion:** tpdf is the daily default for reading. If it is not, it is not
finished.

#### Cancellable rendering — done 2026-07-27

The carried-forward A0 failure is a latency problem: a tile takes seconds, the render is
uninterruptible, and so the renderer stays busy on a tile the viewport left long ago.
`src/progressive.rs` removes the "uninterruptible" half. It owns `FPDF_DOCUMENT`,
`FPDF_PAGE` and `FPDF_BITMAP` directly, because `pdfium-render` keeps every handle
accessor `pub(crate)` and the progressive functions take raw handles — so the safe wrapper
cannot reach them at all.

`bin/progressive_probe.rs` measured it on the A0 sheet, one 1024² tile at 1x:

| question | answer |
|---|---|
| does pausing change the pixels? | no — byte-identical to the safe path, sliced or not |
| how often can it be interrupted? | 268 times in a 6.4 s render, **and 268 whatever slice is asked for** |
| what does that bound? | 24 ms mean between polls, 66 ms worst observed |
| what does cancelling cost to notice? | 0.25–24 ms, against a 6.3 s render |
| what does slicing cost? | 1–2%, inside the round-to-round noise |

The one that changes a design assumption is the second. The poll points are wherever
PDFium's own work divides, so the slice picks *which* of them we stop at and cannot create
more. Latency budgets have to be written against PDFium's spacing, not against a number we
choose.

Two things surfaced while building it, both bigger than the thing being built.

**`FPDF_LoadPage` is not cached by PDFium**, and re-parses the page on every call: 0.18 ms
on the text corpus, **44.3 ms on the A0 sheet**. `render.rs` loads a page per tile request,
so a screenful of six tiles pays 266 ms of pure re-parsing on the document that is already
too slow — and nothing measurable on the corpus most likely to be used for testing.
`RawDocument` now caches page handles.

**A zero-valued sentinel collided with a genuinely zero elapsed time.** `Instant` on this
hardware ticks at 41.67 ns, so arming a zero-length slice immediately after taking the
origin produced `elapsed == 0`, which the pause state read as "no deadline". Slicing
silently stopped working. It was caught by the probe's rule that a sliced render reporting
`resumes: 0` fails — not by any test of the sentinel, and not by the identity checks, which
all still passed because a render that never pauses is byte-identical to one that never
had to.

Two things are known and not yet done:

- **A cancelled tile is a real partial composite**, not an untouched buffer, but whether it
  is worth showing is unmeasured: the A0 fixture saturates every similarity metric tried
  (see `AGENTS.md`). That needs a realistic drawing, not a stress fixture.
- **Form-field appearances are not drawn.** The safe path follows its render with
  `FPDF_FFLDraw`; the progressive path does not, so documents with interactive widgets will
  differ until that pass exists.

#### Withdrawing a stale tile — done 2026-07-27, and it does not fix the A0 page

`render.rs` now runs on the raw handles, with the page cache, and every request carries an
id it can be withdrawn by. A withdrawal that arrives before the render starts drops the
whole thing; one that arrives after abandons it through the progressive API. Only the
client can know a tile has stopped being wanted — the window is its state — so this is an
explicit withdrawal rather than an epoch. An epoch would have to either cancel still-wanted
long renders on every window change, which finishes nothing on a hard page, or leave stale
ones running, which is the behaviour being fixed.

Measured against the coverage floor, not the frame rate, with withdrawal as an interleaved
variant so both behaviours are in one run. Four rounds, 300 frames, brisk 30 css px/frame
scroll, and every number below reproduced within a tile when the two variants were run in
the opposite order:

| corpus | zoom | variant | sharp | tiles kept | tiles wasted | withdrawn |
|---|---|---|---|---|---|---|
| text-heavy | 1x | plain | 100% | 49 | 0 | 0 |
| text-heavy | 1x | withdraw | 100% | 49 | 0 | 0 |
| text-heavy | 4x | plain | 100% | 67 | 0 | 0 |
| text-heavy | 4x | withdraw | 100% | 66 | 0 | 0 |
| vector-heavy | 1x | plain | 11% | 3 | 0 | 0 |
| vector-heavy | 1x | withdraw | 9% | 1 | 0 | 12 |
| vector-heavy | 4x | plain | 6% | 1 | 5 | 0 |
| vector-heavy | 4x | withdraw | 6% | 1 | 0 | 19 |

Read the text rows first: withdrawal is **inert** where the queue never goes stale. Nothing
is withdrawn, coverage and render time are unchanged, and the mechanism costs nothing when
it has nothing to do. That is the control, and it is the row that says this is safe to
leave switched on.

The vector rows are the honest result: **withdrawal stops the waste and buys no coverage.**
At 400% it turns five finished-then-discarded tiles into zero — the renderer no longer
spends about four seconds producing tiles that are thrown away on arrival — and the visible
area is 6% sharp either way. It cannot do better, because there is nothing useful to spend
the freed time on: every other tile of that page also costs a second, and the criterion
needs a screenful. **The A0 page still fails §8's floor, and closing it is the worker pool,
not the queue discipline.**

The 1x row is worse than that and worth stating rather than averaging away: withdrawal cost
two delivered tiles and two points of coverage. The predicate is "not wanted *now*", and the
harness's scroll reverses at both ends of the document, so a tile that leaves the window
comes back into it about eighty frames later — by which time the render that would have
served it has been thrown away. Real reading reverses too. A withdrawal predicate that
accounted for scroll direction, or a grace period before withdrawing, would not have paid
that; neither is implemented, and neither should be guessed at without measuring.

#### The coverage floor, measured properly — 2026-07-27

With withdrawal in place, §9's restated criterion was checked on both corpora, all four
layout-and-zoom variants, three to four rounds of 300 frames each:

| corpus | sharp | any (mean) | floor (worst frame) | drops |
|---|---|---|---|---|
| text-heavy | 100% | 100% | **100%** | 0–2 per 1,200 frames |
| vector-heavy | 6–10% | 100% | **100%** | 0 |

The `floor` column had to be added to answer the question at all. "Never below the tier-1
placeholder" is a claim about a **minimum**, and the report showed a mean that rounds to
100% — which is equally consistent with a frame that showed nothing. A statistic that cannot
express the failure cannot test for it, and reading the mean would have declared the
criterion met without evidence either way. It is the worst frame of the worst round, not the
mean of the per-round minima, because one blank frame anywhere is the failure.

Two caveats that belong next to the pass. The floor is measured over the *timed* section,
which begins after a warm-up in which the page has been open for about three seconds; on
the A0 sheet the tier-1 placeholder itself costs 1.5 s to render, so there is a window after
opening in which the screen is grey and this measurement says nothing about it. And a page
held still on that document reaches 56% sharp at 100% zoom and 92% at 400% within the
warm-up, so the 6–10% figure is the cost of *movement*, not a ceiling on the page.

#### What the worker pool is actually for — measured 2026-07-27

The pool was next on the list because it was the thing expected to fix the A0 page. Measured
before being built, it does not. Six 1024² tiles at 2×, one screenful in the scroll
benchmark's own geometry (`worker-bench --mode parallel --grid 3 --pages 6`):

| workers | wall | vs in-process |
|---|---|---|
| in-process, 1 thread | 8.19 s | — |
| 1 | 8.12 s | 1.01× |
| 2 | 4.58 s | 1.79× |
| 4 | 3.19 s | 2.56× |
| 6 | 2.55 s | 3.22× |
| 8 | 2.55 s | 3.21× |

So a screenful goes from eight seconds to two and a half, and stops improving at six
workers. That is worth having and it is not a scroll.

It also corrects §3, which measured 3.89× on four workers and concluded that a pool should
be sized from the performance-core count. That was one tile from each of many pages of the
*text* corpus; across tiles of one expensive page the same machine gives 2.56× on four and
plateaus at six. The shape of the work changes both the speedup and where it stops, so
neither number is "the" scaling factor — and the bench could not ask the second question at
all until `--grid` was added, because its work list walked pages and the A0 fixture has one.

The consequence for the roadmap: **the pool's justification is `docs/THREAT-MODEL.md`, not
the coverage floor.** Parsing hostile input in a sandboxed, restartable, resource-bounded
process is the reason to build it, and that reason was always sufficient on its own. It
should not be scheduled as a rendering-performance fix, because measured as one it buys a
3.2× that leaves the hard page just as unscrollable.

#### A viewer a person can drive — 2026-07-27

Everything above was measured through a harness that supplies its own scroll offset. This
is the other caller: `src/lib/viewer.ts` owns input, a frame loop and the zoom, and drives
the same `Scroller` the benchmark drives. Keeping the split is deliberate — the class that
knows *what a frame costs* should not also be the class that knows *where the finger went*,
or the benchmark becomes a special case inside the viewer.

`viewport` is the default layout, which is the verdict §4 already reached and had not yet
applied. Lazy page geometry is now the default too, for the same reason: it is what takes
warm startup from 374 ms to inside the target, and shipping the opposite default meant the
exit criterion was met by a variant nobody ran.

**The frame loop idles.** The benchmark runs 300 frames back to back because that is what
it is measuring; a viewer that did the same would hold a core awake for as long as it was
open. The loop therefore runs only while the scroll is moving or the scroller has work that
has not reached the screen, and stops itself otherwise. That makes every input path
responsible for waking it, which is a real hazard — a path that changes state without waking
leaves the screen stale until some unrelated event, and looks exactly like a rendering bug.

**The degraded state is no longer owed.** §9 closed the A0 page against "never below the
tier-1 placeholder" and recorded that the honest description was "a blurry page that never
blinks out", with the UI owing the user an account of it. The status line now carries one,
and it distinguishes the two failures rather than averaging them: `any` is whether there is
a page at all (*preparing page*), `sharp` is whether it can be read (*sharpening*). Both
numbers come out of the scroller's own coverage measurement, so what a reader is told is the
same number the benchmark reports, not an estimate of it.

**It is checked, not demonstrated.** `src/lib/viewercheck.ts` opens a document in a real
webview, dispatches real `WheelEvent`s and `KeyboardEvent`s at the viewer's root, and
asserts sixteen behaviours — fit-width, wheel and key scrolling, End and Home, the zoom
ladder, a pinch, resize, and that the loop idles. Two checks carry their own control,
because the alternative is a check satisfied by nothing happening: idling is asserted in
both directions, and every coverage recovery is preceded by an assertion that the tiles were
actually thrown away first.

That second control is not theoretical. Written without it, "covers the last page" waited
for full coverage that the *first* screen had already established, returned before the jump
had rendered anything, and passed — while its own detail line read `page 1/775`. It is the
fourth time in this project that a green result came from a test that never ran.

Six deliberate mutations, one at a time, and every one was noticed. Two of them were noticed
by a *different* check than the one aimed at, which is worth recording as a result rather
than tidied away: one mutation was a no-op (it assigned a field the line above had already
set, so it tested nothing), and one deleted zoom invalidation entirely, so the check that
caught it was the control rather than the recovery. A mutation that changes nothing looks
exactly like a check that cannot fail.

Warm startup is **276 ms median, 267–293**, unchanged and inside the 300 ms target, with the
Tauri dialog plugin now linked in. That is a cross-time comparison rather than an interleaved
one, so it says the criterion still holds; it is not evidence about what the plugin cost.

What this is not, and should not be mistaken for: there is no text selection, no search, no
thumbnails, no outline, no accessibility tree, and no command palette. The exit criterion for
this phase is that tpdf is the daily default for reading, and it is not.

#### The text layer, and selection on top of it — 2026-07-27

Selection, search and the accessibility tree all need the same thing: the page's characters
with their positions. `src-tauri/src/text.rs` is that layer and is deliberately the only
one — three features reading three different extractions would disagree in ways no test
catches, each being self-consistent.

**It carries codes, not a string.** `FPDFText_GetText` is shorter and wrong: it extracts
UCS-2 and, in its own documentation, "ignores characters without UCS-2 representations", so
the string and the character indices the boxes are keyed by fall out of step on exactly the
documents where nobody would notice — CJK, symbol fonts, anything astral. One Unicode scalar
per index cannot desync, and the caller builds a string from the range it selected. Same
lesson as `set_text()` drawing `.notdef`: work in the code space the document uses.

**The coordinate flip is checked against pixels.** A y-flip is the classic failure here and
the classic one to miss — the highlight still lands in tidy rectangles, on the wrong lines.
`bin/text-probe --mode align` renders the page, and for every drawable character asks
whether its mapped box covers ink:

| corpus | boxes land on ink | un-flipped control |
|---|---|---|
| `text-marked`, `text-truetype` | 100% of 145 | 4.1% |
| `text-cid`, `text-base14` | 100% of 145 | 4.8% |
| `text-heavy` | 100% of 2,278 | **69.9% — cannot discriminate** |

The last row is the result worth keeping. On a dense page of uniform lines a *flipped*
mapping still lands on ink most of the time, so that page cannot tell the two conventions
apart — and the probe fails the run and says so, rather than reporting the 100%. The
corpus most likely to be reached for is the one where this check is blind.

A whole-page bounding box was tried first and is not an oracle at all: the text fixtures
draw a frame as well as text, so the ink box is far larger than the characters and *neither*
convention matched it.

**What it costs** (`text-probe --mode extract`, 7 rounds interleaved):

| corpus | page cached | page not cached | characters |
|---|---|---|---|
| `text-heavy` | 1.42 ms | 1.64 ms | 2,725 |
| `vector-heavy` (A0) | 0.12 ms | **43.2 ms** | 0 |

So selection on the page already on screen is free, and a document-wide search pays about
1.6 ms per page on text and 43 ms per page on complex vector art — where almost all of it is
`FPDF_LoadPage` rather than the text. The A0 sheet has no extractable characters at all,
which is the correct answer and a reminder that search will need to say so rather than
return nothing.

That table also came out of a broken measurement first: the timer started *after* the page
was loaded, so both columns measured extraction alone and reported the A0 sheet's uncached
extraction at 0.116 ms — against a page load known to be 44 ms. A column named for a cost it
excludes reads as a finding.

**Selection** is `src/lib/text.ts` (cache, hit-testing, run merging) and
`src/lib/selection.ts` (two carets and the per-page range between them), with the viewer
owning an overlay canvas above the tiles. Drag to select across pages, Cmd-A for the page,
Escape to clear, Cmd-C to copy — and the copy waits for any page whose text has not arrived,
because a drag can reach the clipboard before the extraction does and silently copying the
part that happened to be loaded is a bug found in someone else's document.

The check gained four assertions, and the load-bearing one is that **text dragged near the top
of the page comes from earlier in the page's text than text dragged further down**. It is the
only one that ties a screen position to specific characters, and so the only one that can see
a coordinate error. Its control is a zero-length drag, which must select nothing.

It is the second attempt. The first asserted that the dragged text was a **substring** of the
whole page's text, which sounds like the same claim and cannot fail: a selection is a
contiguous range of character *indices* and its string is built from them, so it is a
substring however wrong the boxes are. Inverting the y-flip in `text.rs` — precisely the
defect it was written to catch — passed all twenty checks and returned real words from the
wrong part of the page. The ordering check catches it, and says so in its own words: *"the
page reads bottom to top, which it does not"*.

Five mutations, one at a time. Four were caught. The fifth deleted `Selection.isEmpty`'s
meaning by making it always return `false`, and **nothing noticed** — because every call site
already handled an empty range correctly, so the guard changed no behaviour at all. It went
the way of `queue.rs`'s zero guard: deleted rather than kept, since unreachable defence reads
as load-bearing and can quietly become wrong.

The mutation harness needed a control of its own first. It looked for `[FAIL]` lines, and a
run that never produced a report has none either — so two results read as "nothing went red"
when the truth was "nothing ran". It now requires the summary line.

Not done, and not pretended otherwise: word and line selection by double- and triple-click,
selection across a column boundary in reading order rather than index order, and any
handling of rotated or vertical text.

#### Find in document — 2026-07-27

Search reads the text layer above rather than PDFium's, and that is the decision worth
recording. `FPDFText_FindStart` exists, it is what Chrome's Ctrl-F uses, and it would have
been shorter — but it searches PDFium's *own* extracted string and answers in positions
into it. That is a second extraction with a second index space, sitting beside the one
`text.rs` exists to be the only one of. The rule that file opens with is that three
features reading three different extractions disagree in ways no test catches, each being
self-consistent; taking the shorter route would have made search the second of the three,
in the same session that argued against it.

So `src-tauri/src/search.rs` matches over the character codes, and a hit is a range of the
same indices the boxes are keyed by. Highlighting one is the selection code with a
different colour, and there is no mapping between index spaces left to get wrong.

The cost of that is that Unicode-aware matching is ours. The fold does what a reader
expects Ctrl-F to do and nothing more: case ignored (`to_lowercase`, so `Ä` matches `ä`),
runs of whitespace collapsed to one so a phrase spanning a line break still matches, soft
hyphens dropped. Because folding can change a character's length — `İ` lowercases to two
characters — the folded sequence carries the source index of each character, and a match
translates back through that map rather than by arithmetic.

What it deliberately does not do: normalise ligatures, strip accents, or rejoin a word a
hyphen broke across two lines. Each is a real feature and each makes the highlight cover
characters the query did not contain; none is guessed at. A query of only whitespace is
refused rather than run, because the fold has already destroyed the only distinction such
a query could be drawing.

**One page per request, sequentially.** The render thread is FIFO and shared with tiles, so
a single job that scanned the document would hold it and every tile behind it would wait.
At page granularity a search interleaves, and cancellation is not asking for the next page
— there is nothing to withdraw. A generation counter drops replies belonging to a query
that has been superseded.

**What it costs.** A whole-document scan of the 775-page corpus for a word that is not in
it — the worst case, since nothing can stop early — takes **843 ms**, about 1.1 ms per
page. That is the extraction measured above, not the matching. The first hit appears in the
time it takes to reach the page it is on, which for a search from where the reader is
standing is the first request.

**Checked, then broken on purpose.** Thirteen unit tests over the fold and the index
mapping, plus five viewer checks in a real webview. The load-bearing one is that a match's
index range *covers the characters searched for*, re-extracted independently rather than
read out of the viewer's cache — every other search assertion passes just as well when the
indices are off by one. The negative control is the same needle with letters glued to the
front, and it scans all 775 pages to say so.

Eleven mutations, one at a time, each with its expected victim written down first: eight in
Rust, three in the front end. All eleven went red; ten did so through the check predicted
for them. Note the needle is chosen so its first hit is *not* at index 0 — 0 is exactly the
value an implementation that had lost track of its indices would return, so a check anchored
there could not tell the two apart.

The eleventh is the interesting one. Deleting the generation guard — so a superseded scan
keeps appending into the query that replaced it — was predicted to fail the negative
control, and instead failed *"case is ignored"*, which times out. Following that through is
what found a real weakness: the negative control waited for the scan to stop **running**,
and an older scan finishing clears that flag, so the check was reading its answer at a
moment when neither scan had put anything in the list. It now waits for the page count to be
reached. The mutation was caught either way; the wrong prediction is what produced the fix.

The harness itself failed twice first, both times in the family this repository keeps
rediscovering — a test that cannot distinguish its own silence from a pass.

- It restored each file by **moving** the backup over it, which carries the backup's *older*
  mtime, so cargo compared timestamps, concluded nothing had changed, and ran the suite
  against the previous mutated binary. Every mutation in the loop was written with a fresh
  timestamp, so those results stand; it was the run confirming the tree was clean afterwards
  that reported a failure not present in the tree.
- It parsed failure lines with a regex expecting two spaces between the name and the detail
  — and names are padded to 40 characters, so a name of exactly 41 (*"searches forward from
  the page being read"*) is followed by one. That mutation was reported as a **survivor**
  while the summary line in the same output said 28 of 29 passed. The two numbers disagreed
  and nothing was comparing them; they are compared now.

Not done: whole-word and case-sensitive options, regular expressions, a results sidebar,
search within a selection, and matching across a page boundary. Nor is there a bound on the
front-end text cache, which a reader stepping through hits on a thousand pages would find.

#### The command palette, and the registry under it — 2026-07-27

§8 calls this "the thesis, not a garnish", and the argument is worth restating because it
decides the shape rather than the styling: the complaint about Acrobat is not that it lacks
capability, it is that the capability is unreachable. A palette only answers that if
*every* command is in it — which means commands have to exist as data somewhere, rather
than as branches of a key handler. That handler had reached fifteen branches, and anything
added to it is something the palette cannot see.

So `src/lib/commands.ts` is the registry and the key handler becomes one of its callers.
Fourteen commands are registered today; the next feature registers rather than growing the
`if` chain.

**Ranking is subsequence matching, scored the way a code editor scores it** — word starts
first, then consecutive runs, then position — so `fw` finds "Fit width" and `zi` finds
"Zoom in". It returns the matched *positions*, not just a score, because the palette bolds
them and a highlight that disagreed with the ranking would be worse than none. Recents
break ties and deliberately cannot beat a better match: the tie-break bonus is smaller than
a single word-start match, so typing something specific always wins over history.

**It is plain DOM rather than a Svelte component**, for the same reason `viewer.ts` is:
`viewercheck.ts` mounts these classes directly and dispatches real events at them, and a
component that only exists inside `App.svelte` is one the check cannot reach.

**The front end has unit tests now, and a gate.** The plan previously noted that
`npm run test` did not exist and would land when there was something for it to check;
ranking is that — pure logic with an answer that can be *wrong* rather than merely ugly.
Twenty tests, `vitest`, and `scripts/gates.py` grew a seventh gate. Behaviour needing a
document and a window stays in the viewer check, which is not a gate because it needs a
built bundle and a generated fixture.

Fourteen mutations: eleven against the ranking, three against the palette, each with its
expected victim written down first. All fourteen were caught. Two results worth keeping:

- **A branch was deleted rather than tested.** `startsWord` also treated a capital after a
  lower-case letter as a word start, for titles like `zoomIn`. No mutation of that clause
  could fail anything, because every title is a human-readable label in sentence case. It
  went the way of `Selection.isEmpty` and `queue.rs`'s zero guard.
- **A branch that no test can name directly is not the same as an untested branch.** The
  `index === 0` case of `startsWord` cannot be isolated by a score comparison — there is no
  title where position 0 is *not* a word start to compare against. It is pinned by the
  ordering test that recency must not beat a better match, which fails without it, and that
  is recorded next to the line so the next reader does not "fix" it.

Not done, and unchecked rather than merely unfinished: **Cmd-K itself and the command list
`App.svelte` registers are covered by nothing.** The check builds its own registry, so what
it proves is that the palette works, not that the application's commands are wired to it.
Also absent: user-rebindable keys, commands that take an argument (go to page *n*),
persisted recents, and any verification that a command's displayed keybinding matches the
handler that implements it — a wrong label there teaches a wrong shortcut.

#### The accessibility tree — 2026-07-27

§8 states this as an architectural constraint and the virtual-scrolling section repeats it:
*"Accessibility constrains this design and must be settled before it is built, not after."*
It is done now, before thumbnails and an outline are built on the same scroller, because
every feature added to that scroller first is more that would have to be rewritten.

The default state of what we had built is worse than "unstyled": a canvas-rendered,
virtualized page list has **no DOM text at all**, so a screen reader finds an empty
scrolling region. `src/lib/a11y.ts` maintains a parallel, visually hidden DOM of the
visible pages' text, and two properties in it are the whole point:

- **Elements are keyed by page and never recycled.** A page that stays on screen keeps the
  *same* element across every frame, so a reading cursor or the keyboard focus inside it
  survives a scroll. Reusing a container for a different page — the obvious optimisation,
  and what a windowed list normally does — moves the cursor to different text underneath
  the user with no indication.
- **Text is split into lines**, from the same character geometry the selection uses. One
  2,700-character blob per page is present and unusable; moving by line is most of how a
  document is read.

The tiles and the selection overlay are `aria-hidden`, so the text is not doubled by a
large empty region, and the page number is announced through a polite `role="status"`.

**What it is not.** It is not pdf.js's approach of positioning invisible text spans over
the glyphs, which additionally buys native selection and hit-testing — tpdf already does
selection on the canvas against real character boxes, so a second selectable copy of every
page would be a liability rather than a feature. And **it is not verified against a screen
reader**: what the check asserts is that the text is present, is the page's own, is in
reading order, and survives scrolling. Whether VoiceOver announces it *well* needs a person,
and no claim is made about it.

Six checks in the viewer check and seven unit tests on the line splitting. Eight mutations,
all caught. Counting the check names across the two corpora also turned up two that had been
*vanishing* rather than skipping on a document with no text — 43 names in one run against 41
in the other — and both were controls, which is the case where a silent disappearance costs
the most. Two of them found real defects in the *checking*, both of the same shape — a
text comparison cannot see a property that is not about text:

- **`textContent` concatenates block elements with nothing between them.** The first
  comparison against an independent extraction failed at 2,562 characters against 2,618 —
  one missing separator per line, on a 56-line page. The content was identical and only the
  structure differed, which is precisely what the check was flattening away. It now joins
  the blocks.
- **`display:none` and `visibility:hidden` remove an element from the accessibility tree,
  and every text assertion still passes.** The layer could be completely inert while
  reading correct. There is now a check that the container is 1×1 but neither hidden that
  way — visually gone, still in the tree.

Not done, and the first of these is a real limitation rather than a missing nicety:
**reading order is derived from geometry, not from the document's own tagged structure.**
A tagged PDF carries a `/StructTree` that says what is a heading, a table cell, an
alternative text, and in what order it should be read; we infer lines from character boxes,
which is what an untagged document forces and is strictly worse for one that is tagged.
Also absent: headings and table semantics, a document language attribute, visible keyboard
navigation between pages, and any high-contrast handling.

#### The outline, and a sidebar to put it in — 2026-07-27

§8 wants a sidebar with thumbnails, outline, annotations and search results as tabs. Only
the outline is here. The panel is nonetheless a container with a header rather than a bare
list, because otherwise the *second* tab is the one that has to introduce the chrome, by
which point something else is positioned against its absence.

**The outline is the first feature in this project whose input is openly hostile.** Not
inferred from the threat model — stated by PDFium, in the documentation of the function the
walk is built on: *"the caller is responsible for handling circular bookmark references, as
may arise from malformed documents."* The naive loop hangs the render thread, silently and
forever. `testdata/make_outline_pdf.py` builds two fixtures for this; `qpdf --check` agrees
with one of them independently, reporting *"loop detected in /Outlines tree"*.

`src-tauri/src/outline.rs` carries three bounds — a visited set, a depth limit, an item
budget — and the reason for three is that each catches what the others cannot. The visited
set stops a cycle *at its first repeat*, which is what keeps the rest of the outline
reachable; abandoning the level instead loses nine of the fixture's ten top-level entries.
The depth limit stops 200 distinct nested nodes, which put nothing in the set twice. The
budget stops the case where the set is defeated, so that termination does not depend on the
mechanism expected to work. Whatever any of them cuts is counted and shown as a warning,
because an outline displayed as complete when it is not is the same failure as a leak
scanner reporting clean on a carrier it could not decode.

**Two findings, and the first is the reason to have built a hostile fixture at all.**

- **`FPDFBookmark_GetDest` follows the bookmark's action without checking its type.** When
  there is no `/Dest` it falls back to the action's `/D` array regardless of the action's
  `/S`, so a `/GoToR` — *"open other.pdf at page 1"* — comes back as an ordinary
  destination and resolves against **this** document. The probe reported `page 1` for it.
  Not an error and not a refusal: a plausible page of the file the reader already has open.
  The fix is an ordering rather than a filter — read the action first, and reach
  `FPDFBookmark_GetDest` only when there is none, which is exactly when its fallback has
  nothing to reach. Every ordinary outline resolves identically either way, which is why
  only a fixture built to contain a remote destination could find it.
- **Enumerating page sizes for the destinations is cheap if you do not load the pages.**
  A destination's `/XYZ` y arrives in page space and has to be flipped against the page's
  height. `FPDF_LoadPage` costs 44 ms on a complex page and an outline can name hundreds;
  `FPDF_GetPageSizeByIndexF` reads the page dictionary's boxes instead. The whole walk of
  the ordinary fixture is **0.17 ms**, and of the hostile one — 44 entries, two cycles, a
  50,000-character title — **1.6 ms**.

Actions tpdf declines to follow are **shown, marked, and explained** rather than dropped:
`/Launch`, `/URI`, `/GoToR` and `/EmbeddedGoTo` each get their own wording next to the row.
An entry missing from a table of contents reads as a bug in tpdf, and one that silently
ignores clicks reads as a worse one. `docs/THREAT-MODEL.md` disables launch actions by
default; this is where an outline click would otherwise reach one.

The sidebar is a real `role="tree"` with `aria-level`, `aria-expanded` and a roving
tabindex, so it is one tab stop and arrow keys move within it. Tabbing through a thousand
headings is the alternative and is why so many outline panels are unusable without a mouse.
The list is **bounded, not virtualized** — 10,000 entries, which is what makes a real
element per visible row affordable; bounding the input is the honest version of a windowing
implementation that does not exist.

**17 mutations, all caught, and one only after the test it aimed at was fixed.** `allRows`
ignoring collapse could be mutated to respect it with all 58 checks still passing, because
every fixture tree in the test file said `open: true` — the test folded rows through an
`Expansion` it held, which is not the thing `allRows` is about. The hazard is a
*producer-closed* subtree, and the test now uses one. Same shape as the query-key test that
probed the wrong direction: an assertion about the mistake that came to mind rather than the
mistake the code can make.

Three of the mutations are only catchable by the PDFium probe, since each is about what the
library does rather than what our arithmetic does: reversing the action/`GetDest` ordering,
inverting the y-flip, and deleting cycle detection. The last one is worth its own sentence —
it does **not** hang, because the item budget catches it, which is precisely why the probe
asserts the budget was *not* what stopped the walk. Without that control the run still says
18 checks, and 14 of them go red for reasons nobody would connect to a loop.

**The sidebar is checked in the viewer, and three of those checks went red first.** Nine
were added to `viewercheck.ts` — rows drawn, tree roles present, one tab stop, collapse and
re-expand, activation moving the viewer to a page it was not on, a destination's y landing
further down than a same-page entry above it, the highlight following a scroll, and a
refused action being inert. Two found real defects and the third found a defect in a
fixture, which is the best evidence available that they can fail at all:

- **A roving tabindex that did not follow real focus.** `Sidebar` tracked the focused row
  only when it moved focus *itself*, so focus arriving any other way — a Tab into the tree,
  a programmatic `element.focus()` — left every arrow key aimed at whichever row was
  tracked before. Collapse reported `7 rows -> 7`. A `focusin` listener on the tree fixes
  it, and that is correct behaviour independently of the check.
- **Clicking an entry highlighted the entry before it.** Arriving at a destination
  deliberately leaves a little air above the heading, so on arrival the heading is *below*
  the viewport top — and `currentId` required it to be at or above. The margin is now
  stated in points rather than CSS pixels (in pixels it is 32 pt of the page at the lowest
  zoom stop and 4 pt at the highest, which no single tolerance could bound), with a
  matching tolerance in `outline.ts` pinned by unit tests in *both* directions.
- **A fixture whose every line read the same.** The body text was 24 identical
  "quick brown fox" lines, and the selection check locates a drag with `indexOf` — so both
  drags resolved to the same index and it reported that the page reads bottom to top. The
  lines are now built from a rotating word list, unique at every offset.

One check was also skipping when it should have run: "a destination's y is measured from
the page top" looked for one entry at the very top of a page and one below 50 pt, and the
only fixture with the pair has them at 240 and 440. It now takes the highest and lowest on
any shared page. That check is the y-flip discriminator, so a silent skip there was the
expensive kind.

At that point all four corpora passed and every run reported the same 53 check names --- the
strip below adds a fifth corpus and ten more names: `outline-simple` 51/51,
`outline-hostile` 52/52, `text-heavy` 43/43, `vector-heavy` 29/29, the differences being
skips with their reasons.

Also not done: a resizable panel, persisted collapse state, "reveal current section in
outline" as a command, and search results as a second tab.

#### Page thumbnails, and what a second consumer of one renderer costs — 2026-07-27

The sidebar's second tab. What makes it worth writing up is not the strip, which is a
windowed list of small pictures; it is that **it is the first feature that competes with
the reader for the renderer.** Everything before it was work the reader had asked for.

§4 measured the price of a thumbnail and it is not small: a 150 px render of the A0 sheet
costs **1.52 s**, because PDFium charges about a second of fixed cost per render call
whatever is being asked for. The render service is one FIFO thread — concurrent PDFium is
undefined behaviour — so a strip that asked for a document's thumbnails would put seconds
of work in front of every tile someone is waiting for.

Two rules, and together they are the design:

- **At most one thumbnail is outstanding.** A queue of ten buys no throughput on a serial
  renderer and costs the only thing that matters here, which is the ability to get out of
  the way.
- **It yields.** The viewer already reports its outstanding work every frame; while that is
  above zero the strip asks for nothing and withdraws what it has asked for. The withdrawal
  goes through the same progressive-API cancellation as a stale tile and returns in
  0.25–24 ms against a render that would have run for a second and a half.

So the viewer waits **tens of milliseconds** for a thumbnail rather than the second and a
half a naive strip would cost it, and the page whose thumbnail was withdrawn is simply
asked for again when things settle.

**Tier 1 is read, not written.** §4 says the placeholder "doubles as the thumbnail" and it
does — same 150 px, same scale — so the strip asks the viewer first and draws for free any
page it has already prepared. That is why opening the strip shows the page being read
immediately even on the A0 sheet. The reverse is deliberately not wired: tier 1 is
permanent for the session, so donating every thumbnail into it would grow it to one bitmap
per page, 98 MB on the 775-page corpus, for pages nobody has looked at.

**Only the visible rows exist.** The outline can afford a real element per row because the
walk is bounded at 10,000 entries; a page strip is bounded by the document. Rows are built
for the window plus an overscan and destroyed when they leave, which makes `aria-setsize`
and `aria-posinset` load-bearing rather than decorative.

**It needed a fixture that does not exist elsewhere.** Three new checks assert the yield,
and on every corpus but one they report `[SKIP] the thumbnail finished before the viewer
asked for anything` — because a thumbnail costs about a millisecond on a text page. That
skip is the honest answer and reporting it as a pass would have been the familiar failure.
`vector-multi.pdf` is twelve A0 pages sharing one content stream, and it is the only
document where the collision can happen at all. Twelve, not three: the check suite visits
page 1 and the last page, so on a short document the viewer has already made a placeholder
for every page and the strip borrows all of them without rendering anything.

Twelve mutations, one at a time — seven against the pure geometry, five against the strip
through the real webview — and every one was caught by the check it was aimed at, after
**two of those checks turned out to be wrong.** Both were found this way rather than by
reading them, and one of them before it was ever run:

- **A check the defect could switch off.** "The strip builds only the rows on screen"
  skipped when every row was built, on the reasoning that a document whose rows all fit
  needs no windowing — so the mutation that deletes windowing makes it report itself
  inapplicable instead of failing. It now bounds `mounted` by what the panel height and
  row height say *could* be on screen, which the defect does not control. A skip is a third
  outcome, and a defect that reaches it is as invisible as one that passes.
- **A bound written the wrong way round.** "The page already rendered is not rendered
  twice" asserted that some thumbnail was borrowed rather than re-rendered. A borrow
  completes in a microtask, so without an in-flight set the same page is borrowed again on
  every scroll and resize in between — twelve borrows on a twelve-page document with seven
  rows on screen. The defect makes `borrowCount > 0` pass *harder*; the upper bound
  `borrowCount <= renderedCount` is what sees it.

All five corpora then reported the same **63 check names**: `outline-simple` 58/58,
`outline-hostile` 58/58, `text-heavy` 52/52, `vector-heavy` 34/34, `vector-multi` 41/41,
the differences being skips with their reasons. A sixth arrives in the entry below.

Not done: reordering pages by dragging a thumbnail (that is Phase 2, and needs the
editing model), a resizable panel, and any persistence of which tab was open.

#### A page that says `/Rotate`, and the two things that were wrong on it — 2026-07-27

Not a feature. A defect, found by building the fixture that nothing in the corpus had:
`/Rotate 90` is what a scanner emits, and no document here carried one.

`text-probe --mode align` renders a page and asks, per character, whether its mapped box
covers ink. On the new four-page fixture, before anything was changed:

| page | character boxes on ink |
|---|---|
| `/Rotate 0` | **100%** of 439 |
| `/Rotate 90` | **0.0%** |
| `/Rotate 180` | **0.0%** |
| `/Rotate 270` | **0.0%** |

Not approximately wrong. Every selection, every search highlight and the whole
screen-reader reading order was somewhere else entirely, in tidy rectangles, on exactly the
documents a scanner produces.

**The cause is that PDFium answers in two coordinate systems at once.**
`FPDF_GetPageWidthF`/`GetPageHeightF` report the size *after* rotation and a render comes
out rotated to match — so layout and tiles were already right. `FPDFText_GetCharBox` and
`FPDFDest_GetLocationInPage` report the page's own *unrotated* space. The flip
`height_pt - y` against the reported height is therefore correct at 0 and wrong everywhere
else. `FPDF_GetPageSizeByIndexF` was measured rather than assumed and belongs to the first
group, which is not obvious given it reads the page dictionary rather than the loaded page.

The turn is now one function, `text::to_device`, with two callers: character boxes, and the
outline's destinations by way of a degenerate box. A second implementation would be a second
place to get it wrong, and the destination is the half nobody would test.

**The probe gained the control the fix needs.** Its existing one asks whether the *flip*
could have been wrong on this page; on a rotated page that is a different question from
whether the *turn* could have been. It now also reports what each of the three wrong
rotations scores, and fails if any reaches the ceiling. With the rotation deliberately never
read, the run says `reading /Rotate as 90 does not — 100.0%` — the control naming the
mistake outright.

**The fix exposed a second defect, which the first did not imply.** Characters are grouped
into lines by *vertical* overlap. On a page whose text runs down the screen that puts every
character on its own line, so the screen-reader layer read the page **letter by letter** and
the selection highlight became one rectangle per glyph. Every text assertion still passed —
`textContent` is identical either way — and what caught it was the comparison against an
independent extraction: 877 characters against 534, the difference being the spaces from
joining one block per character. The grouping axis now comes from the page's rotation. The
honest limit: this fixes the whole-page case only, and a rotated *run* inside an upright page
is still split character by character, as before.

**Reading the rotation is not free, and the schedule changed because of it.**
`FPDFPage_GetRotation` needs a loaded page, while the rest of the outline walk reads the page
dictionary. Measured on `outline-simple`, interleaved: **0.17 ms → 7.5 ms** steady state,
45.7 ms on a cold first run, about 1 ms per distinct page named with coordinates. A
three-hundred-entry table of contents is a third of a second of the render thread, which is
FIFO — so the outline is now asked for after the first screen is up rather than at open,
with a one-second grace so a document whose first page is slow still gets one.

State that last change for what it is: **scheduling, and the only thing here with no
automated check behind it.** The viewer check invokes `document_outline` directly rather than
through `App.svelte`, so nothing exercises the wait; and the corpus has no document with a
table of contents large enough for the cost it avoids to be visible. What is measured is the
walk itself, above.

Thirteen mutations, all caught by the check each was aimed at: five against the turn
arithmetic, three against the line-grouping axis, one against the page's rotation never being
read, and four against the outline's destination path. The last set is the reason the rotated
fixture carries an outline at all — and one of its entries is `/XYZ null 600 0`, a
destination naming no horizontal coordinate, which exists so the code path that declines to
place it has an input that reaches it. Without that entry the guard could be deleted and
nothing would notice, which by this repository's own rule makes it a guard to delete rather
than keep.

All six corpora report the same **63 check names**: `outline-simple` 59/59, `outline-hostile`
58/58, `rotated-90` 52/52, `text-heavy` 52/52, `vector-multi` 41/41, `vector-heavy` 34/34.

#### Rotating the view — done 2026-07-27

The Phase 1 item the coordinate work above was opened for. **Cmd-R** turns the view a quarter
clockwise and **Cmd-L** the other way, both also in the palette; the document is never
touched, and rotating *pages* stays where it belongs, with the operations that write.

PDFium's own render call takes a rotation and composes it with the page's `/Rotate`, so the
renderer's half is one argument threaded from the tile URL down to
`FPDF_RenderPageBitmap_Start` — plus the dimension swap it needs, since `size_x`/`size_y` are
the displayed size and a quarter turn exchanges them. The text half cannot go the same way:
character boxes are a property of the document and know nothing about how someone is looking
at it, so the boxes are turned in our own code, once, where the cache hands them out.

That leaves two implementations of one idea, and the rule that ties them is the reason it is
safe: turning a device box by `v` after `to_device(p, …)` must equal `to_device((p + v) % 4,
…)` of the raw box. `text.rs` asserts it over all sixteen combinations, against the mapping
that was already verified against pixels — so the frontend's turn inherits that verification
rather than asking to be trusted.

**What rotating costs, and what it deliberately does not try to avoid.** Both cache tiers go.
A zoom step keeps tier 1 because a 150 px placeholder is only stretched; a rotated placeholder
is a different picture, and keeping it would leave the page sideways under its own sharp
tiles. So a rotation on the A0 sheet goes grey for the ~1.5 s its placeholder costs to
produce again. Turning the bitmap ourselves is a real option and a lossless one at ninety
degrees; nothing has measured whether it is worth the code, so it is not in.

**Two things a rotated view cannot answer, stated rather than approximated.** An outline
destination carries a vertical offset down an upright page: at a quarter turn that axis is the
screen's horizontal one, and at a half turn it counts upwards while the reader scrolls down.
Both `goToDestination` and the position report therefore fall back to page granularity while
the view is rotated — which is exactly what `/Fit` means, and what `outline.rs` already
returns for a destination it cannot place. Coarse and right, rather than fine and wrong.

Twelve new checks in the viewer harness and fourteen mutations, all caught by the check aimed
at them: five against the Rust arithmetic and the wire format, nine against the frontend.
Three of those checks were written *because* a mutation survived, and each is a different way
of the same mistake — a check that cannot fail:

- **"The same lines come back" derived its drag positions from the very boxes it was
  testing.** Delete the line that tells the text layer about the rotation and it is wrong
  consistently: the sample and the caret agree, the same lines come back, and the selection
  is ninety degrees out from the page on screen. What catches it is a direct assertion that
  the text layer *reports* the turned rotation, checked against a second fetch from the
  backend.
- **Nothing in the harness looked at a pixel**, so dropping the rotation on its way into the
  tile URL passed everything. One tile fetched at two rotations must differ — with the
  control that the same rotation twice is byte-identical, or "they differ" is satisfied by a
  renderer that is merely non-deterministic.
- **The viewer and the scroller each keep a rotation.** The zoom check covers the viewer's;
  a scroller laying every page out upright survived it entirely, producing a narrow page
  inside a correctly refitted window. Its detail line, once the scroller's own page box was
  asserted, read `aspect 0.773 then 0.773`.

The other two red results were the harness's own: a missing wiring line it shared with
`App.svelte`, and a rotation table I had written as one when it is two — across the lines and
along a line disagree at every turn but zero. The second went red on two corpora and passed
on neither, which is the check working.

All six corpora now report the same **75 check names**: `outline-simple` 70/70,
`outline-hostile` 70/70, `rotated-90` 64/64, `text-heavy` 65/65, `vector-multi` 51/51,
`vector-heavy` 44/44.

#### The check could not run, and the reason was not what it looked like

Not a feature, but it cost more of this increment than the feature did, and the repairs
are permanent. Every webview harness produced *nothing* — no output, 0% CPU, the process
alive — and three confident diagnoses were wrong before the right one: the new code, then
a broken frontend bundle, then window occlusion.

The cause is that **a raw `cargo build` binary runs no webview content at all.**
`src-tauri/target/release/tpdf` opens a window and never executes a line of JavaScript;
the same code inside `target/release/bundle/macos/tpdf.app` works perfectly. WKWebView
needs the bundle identity. Every other harness here is a plain executable with no webview
in it, so nothing else in the repository had ever shown this, and `BUILD.md` gave the
bundle path in its example while saying beside it that the check "does not require a
release bundle" — true, and misleading: not a release *build*, a *bundle*.

What settled it was building **HEAD itself** and watching it fail identically. An earlier
control that reverted only the frontend was not enough, and the way it failed is the
lesson: the Rust changes were still in that binary, so "it is not my frontend" got
silently generalised to "it is not my code". **A control has to cover everything that
changed, not the part currently under suspicion.**

Two repairs, both kept regardless of what the next cause turns out to be:

- **The watchdog names the condition.** Every spike entry point begins by asking Rust for
  its path, so the first such call proves the page ran; it is recorded as a `webview alive`
  mark, and a timeout without one says the page never executed rather than printing a mark
  list to interpret. Both halves are verified — it fires on a raw binary and stays quiet
  on a bundled one.
- **`viewercheck.ts` prints each result as it is recorded**, not in one block at the end.
  Buffering meant a run that stopped midway printed nothing, which is identical to a run
  that never started — exactly the state under diagnosis.

`TPDF_RAISE=1` also landed, raising the window for a run with nowhere visible to put one.
It did not fix this and is kept because occlusion is a real WebKit behaviour with the same
symptom; it is opt-in so the default stays polite.

#### Session restore — done 2026-07-27

A reader that opens on an empty window every morning is not the one anybody reaches for, so
this is scope rather than polish: the phase's exit criterion is "tpdf is the daily default
for reading", and forgetting the document is a direct answer to it.

`src-tauri/src/session.rs` keeps one *place* per document — path, page, offset down it,
zoom, whether the zoom was fitting, rotation, sidebar — most recently read first, bounded at
32 and written through a temp file and a rename. `src/lib/session.ts` decides when to write.
Launching with nothing on the command line reopens the top entry; opening a document you
have read before puts you back where you were in it.

Three decisions are worth the words, because each looks like an omission.

**A malformed session file is an empty session, never an error.** Refusing to start because
a convenience file did not parse trades the whole application for the feature. Every path
through `Session::load` returns a session.

**A field out of range is repaired, not refused** — the opposite of what `protocol.rs` does
with a `turns` query parameter, and the difference is who is calling. A tile request is a
live instruction from code we wrote, so a value it could not have produced is a bug worth
surfacing. A session file has been sitting on disk across upgrades and possibly a text
editor, and rejecting it would discard every *other* document's place over one bad number.

**A remembered page is clamped to the document as it is now.** A path is not an identity:
the file may have been rebuilt shorter since it was read, and a viewer scrolled past its own
last page is a worse answer than the wrong page.

Writes are throttled to one a second and **chained through a single promise**. Not for
throughput — because `invoke` resolves out of order under load, so two writes issued a second
apart can land in the other order and the older place overwrites the newer. The same hazard,
from a different direction, as the shuffled check transcript above.

**The check needed a shape nothing else here uses.** Every other harness replaces the
application: it opens a document, does its work, exits, and `App.svelte` never boots. That
cannot work for a property *of* the boot. A check that drove `session.ts` directly would be
a second implementation agreeing with the first — the self-consistency trap that the
rotation increment was caught by twice. So `scripts/session_check.py` launches the real app
four times and observes it, and the two launches that assert nothing about restoring are the
ones that make the other two mean anything:

| phase | session | argument | asserts |
|---|---|---|---|
| `record` | fresh | a document | drives to page 7, one quarter turn, a fixed zoom, sidebar open — then writes it |
| `control` | empty | a document | that state is **not** where the app opens by itself |
| `verify` | recorded | none | it came up in that state, told only by the file |
| `control` | empty | none | nothing opens when nothing is remembered |

The first control fails if *any* of the four fields already matches, not only if all four do
— a restore that got only the rotation right would otherwise hide behind a default that
happened to share the page. Between the phases the script reads the written `session.json`
itself, because writing a place and reading one back are different halves and a run that
only did the second would find nothing to restore and report that somewhere else.

Sixteen mutations, all caught; **fifteen by the check aimed at them**. The sixteenth is a
result rather than a miss: switching off recording entirely was predicted to fail *"recorded
page"*, and failed *"no session file was written"* instead — with nothing ever recorded there
is no file to compare fields in, so the guard before them fires first. Both messages exist
and say different things, which is what was wanted; the prediction was simply too specific.

**Two defects surfaced by building it, and the first is not in this feature at all.**

**`AppHandle::exit` does not set the process's exit code.** It ends the event loop, `App::run`
returns normally, `main` returns unit, and the process exits 0 whatever was asked for. So
every automated run in this repository has reported success for its entire existence,
including `scripts/viewer_check.py`, whose closing `return completed.returncode` could not
fail. The mutation harnesses parse `[FAIL]` lines out of the transcript rather than reading
`$?`, which is why the results they produced were nonetheless right — the exit code was the
one consumer with no second opinion. `spike_exit` now flushes and calls `std::process::exit`,
verified in both directions, because a fix checked only against a failing run would have been
satisfied by exiting 1 unconditionally.

What surfaced it: this check's own harness printed `[OK] session restore verified` directly
beneath a phase whose last line read `0/1 checks passed`. Two numbers in one buffer
disagreeing, with nothing comparing them.

**A control contaminated by the phase before it.** Both controls were pointed at one scratch
session file, since both wanted it empty — but the first control *opens a document*, which is
what it is for, and the app remembers it. The second then launched with something to restore,
restored it, and failed. The standing rule about what one variant leaves behind for the next
covers this exactly and did not fire, because a control is the thing assumed to be inert.

**What the per-frame hook costs is not measured, and is not claimed.** Positions are noted
from `onPosition`, which fires every frame the loop is awake. The marginal work is one object
literal and a seven-field comparison — `viewer.position` was already being computed by the
tick regardless — but that is a description, not a number. What was run is a *regression
check*, not an A/B: `scroll_bench.py` on the text corpus still reports 60.0 fps in every
variant, a 100% coverage floor, and a per-frame callback mean of 0.14–0.66 ms, which is the
same range as before. Comparing that against a previously recorded run is not evidence of the
hook's cost — the interleaving rule exists precisely because wall clock drifts between runs —
so it says the criteria still hold and nothing more.

#### File associations — done 2026-07-28

The literal reading of the phase's exit criterion: an application you cannot double-click a
PDF into is not the default reader for anything. A PDF now reaches tpdf three ways —
`argv` from a terminal and from a Windows double-click, an Apple Event from a macOS
double-click or "Open With", and the file dialog that was already there.

`bundle.fileAssociations` declares it, and one field is deliberately not Tauri's default:
**`role` is `Viewer`, where the default is `Editor`.** `Editor` tells Launch Services and the
Finder that tpdf can edit a PDF, which it cannot yet. It becomes `Editor` when that is true.
Rank stays `Default` — not `Owner`, since tpdf does not create PDFs, and not `Alternate`,
which would rank it below every other viewer and defeat the point. No exported type is
declared: PDF is `com.adobe.pdf`, a system type, and Tauri infers it from the extension.

The design problem is *when* a path arrives, not where from. An Apple Event can be delivered
before the webview exists, so `launch.rs` queues paths until the frontend says it is
listening and emits them directly afterwards — with the drain and the flag flip under one
lock, since doing them separately just makes the same lost document rarer. The event's name
is fetched from Rust rather than agreed as a constant in two places: a constant that drifts
fails by *silence*, the app simply ceasing to notice documents opened while it is running,
which is the half nobody checks by hand.

A handed-over document beats a remembered one. Someone who double-clicked a file is asking
for that file, and yesterday's document appearing instead reads as the association being
broken.

`scripts/open_check.py` runs five launches of the real app, 11 checks. Two of them are
controls: with nothing handed over the remembered document must open — without which
"the handed-over one wins" passes on an app that ignores the session entirely — and the
running-app phase asserts that *nothing* is open before one arrives, or "a document arrived"
is satisfied by one that was already there.

**The check earned itself immediately: the cold double-click was broken, and only that phase
could see it.** `RunEvent::Opened` fires *before* the setup hook, so managed state registered
there is absent and `state::<Launch>()` panicked — an `EXC_CRASH SIGABRT` with no output, an
empty window, and `app built` as the last startup mark. The state is registered on the
builder now, and read with `try_state`.

Two things about finding it are worth more than the fix. A second, unrelated cause —
leftover windows occluding new ones, so other runs also printed nothing — nearly buried it,
and `TPDF_RAISE=1` "fixing" those read as the whole problem being environmental. And the
harness was suspected before the feature was, reasonably, since `open` detaches stdout; what
settled it was asking a different question through a channel the harness did not share.
The app was not writing its session file after a double-click either, which turned "the
capture is broken" into "the feature is broken" in one command.

The reporting half of the unattended checks now lives in `src/lib/checkreport.ts`, shared
rather than copied. What it encodes — print each result as it is recorded, chain the lines —
is a bug already paid for once, and two copies of it is two chances to drift back.

**Eleven mutations, all eleven behaving as predicted — including the one predicted to
survive.** Seven against `launch.rs` through `cargo test`, four against the wiring, each of
those a full bundle rebuild and a five-launch run of `scripts/open_check.py`. Writing the
prediction down before running it is the cheap half of the procedure, and it paid twice here.

The prediction that mattered was E4: put `manage(launch)` back in the setup hook — the exact
shape of the crash — and **exactly one phase should go red, the cold double-click**, since
`argv` is queued before the builder exists and the running-app route arrives after setup. That
is what happened, and it is what makes that phase worth its runtime: no other route through
the application can see the ordering.

The prediction of *survival* was the more useful one. `path_from_url` guards on the URL's
scheme, and deleting that guard breaks nothing — which by the standing rule makes it a guard
to delete. It would have been wrong. `Url::to_file_path` refuses `https://example.com/a.pdf`
because its host is a *domain*, and it maps a `localhost` host to no host at all whatever the
scheme, so `https://localhost/a.pdf` resolves to `/a.pdf` and the guard is the only thing in
the way. The test probed the one direction the guard does not defend, and the dependency's own
refusal covered for it. A second case pins it now, and goes red alone when the guard is
removed.

Recording a predicted survivor separately from an unexpected one is the whole point of the
distinction: collapsing them into one "caught" number is how a suite takes credit for coverage
it does not have.

#### Dark mode — done 2026-07-28

Two separate things wear this name, and only one of them was missing.

**The chrome already followed the system**, because it was built on `Canvas` / `CanvasText`
and `color-scheme: light dark` rather than on literals. Two places had escaped that and are
fixed: the scrollbar was drawn in `rgba(0,0,0,…)`, invisible on a dark window, and the
surround around the page was a fixed `#666` — which reads as unlit against a light window
and as *lit* against a dark one, brighter than the page it surrounds, the one thing it must
not be. No single formula over `Canvas` and `CanvasText` produces both, since the surround has
to be darker than the paper in *both* themes; it is two literals behind a custom property.

The surround also has to be resolved rather than referenced, and that is not obvious: the
default layout composites into one canvas and fills it with the surround, and a canvas takes a
colour, not `var(--tpdf-surround)`. The one place the value is actually visible is the one
place CSS cannot reach it. It is read once and re-read on a `prefers-color-scheme` change,
rather than per frame, because `getComputedStyle` forces style resolution and the frame loop
is the one thing here with a budget.

**The page did not follow anything, and should not follow the system either.** Inverting a
document changes what it looks like, and a reader who has turned their desktop dark has not
asked for that. It is an explicit command — *Invert page colours*, ⌘⇧I, and named that rather
than "Dark mode" because a command called dark mode would appear to do nothing for the reader
who most expects it to, the chrome having already been dark.

##### The transform is a constant offset, and that is derivable rather than lucky

The obvious inversion, `255 - c` per channel, also rotates every hue half a turn: blue
headings come out yellow, a red stamp comes out cyan. What is wanted is HSL's **lightness**
inverted with hue and saturation held, which normally means a round trip through HSL and does
not have to. Writing `M` and `m` for the largest and smallest channel:

* `L = (M + m) / 2`, so inverting it asks for `M' + m' = 2 - (M + m)`.
* HSL saturation divides chroma by `1 - |2L - 1|`, and `|2(1-L) - 1|` is the *same number* —
  so holding saturation fixed holds **chroma** fixed, and `M' - m' = M - m`.

Sum moves, difference does not, so every channel moves by the same amount: `d = 255 - M - m`.
Three properties fall out, and each is a test. It needs no float and **no clamp** — the
extremes become `255 - m` and `255 - M`, in range by construction. It is an **exact
involution**, so the mode is reasonable to toggle. And on a neutral pixel it reduces to
`255 - c`, so black text on white paper does exactly what anyone would predict.

##### It is done in the renderer, and that is a testability decision

A CSS filter over the tile layer would be free, instant, and invalidate nothing. It is not
used, because a filter is applied by the compositor and the pixels cannot be read back: the
only thing a check could then assert is that the style was set, which is the style agreeing
with itself. That is precisely the failure this repository has recorded four times.

So `invert` rides in the tile request beside `turns`, and a toggle discards every tile the way
a rotation does. The cost is the same seconds a rotation costs on the A0 sheet, and the reader
sees the tier-1 placeholder — itself re-rendered inverted — meanwhile.

Five checks in the viewer harness, at both ends of the path, because two very different things
can be wrong. The renderer might not invert; or it might invert perfectly while nothing on
screen changes because the flag never reached a request. The first is answered **exactly** —
the closed form is computable, so the assertion is byte-for-byte rather than "they differ" —
with the control beside it that the transform must actually change the tile, since every pixel
of a mid-grey tile is its own inversion and the exact check would pass on a renderer that did
nothing. The second is answered on the composited canvas, with the drop asserted before the
recovery.

The session check grew a fifth field. The preference is deliberately **not** part of a place —
it belongs to the reader, not to a document — and that is exactly why it needed covering: the
place writer skips a place equal to the last one it sent, and inverting the page moves nothing,
so routed through the throttle it would never have been written at all.

It drives the mode by **pressing ⌘⇧I at the window**, not by calling the function behind it.
The palette advertises that chord, and a label teaching a shortcut that does nothing is worse
than no label — so the binding is the thing worth testing, and calling the toggle directly
would have left it the one part of this that nothing touched. Confirmed load-bearing by
deleting it: the *record* phase's own precondition goes red first, which is the right place,
and the restore check follows.

**What it does not do: photographs.** A photograph's lightness *is* its content, so a face
comes out as a negative with the right hues. Every reader offering this has the same
limitation. Excluding image regions is possible — PDFium reports each page object's type and
bounds — and is not done, because nothing has measured what enumerating objects per tile costs
on a page with two hundred thousand of them. The honest position is that the mode is off by
default and asked for explicitly, never that the inversion is clever enough to be safe.

##### Fourteen mutations, all caught by the check aimed at them

Seven against the transform and its plumbing through `cargo test`, five against what reaches
the screen, two against what survives a launch. Three results are worth more than the count.

**The layers are genuinely independent, and the mutations prove it rather than assert it.**
Deleting the viewer's call into the scroller (V3) left both *tile* checks green — they ask the
renderer directly and never go through the viewer — and turned only the screen checks red. Had
all four gone red, two of them would have been measuring the same thing and one would not be
worth its runtime.

**Two mutations are indistinguishable, and that is a limit worth stating.** V3 (the viewer
never tells the scroller) and V4 (the scroller never discards what it invalidated) produce
*identical* red sets. Both are real defects and both are caught, so nothing is missed — but
the suite cannot say which of the two happened, and a reader of a future failure should not
expect it to.

**The harness's own report disagreed with its verdict, in miniature.** It prints at most five
red checks per mutation, and I1 turned six red — so it printed `[CAUGHT]` above a list that
did not contain the check it claimed had caught it. The verdict was computed from the full
list and was correct; the evidence shown was not evidence for it. That is the same shape as
the `search.rs` harness whose regex silently matched nothing, small enough to be harmless here
and worth the sentence because the next instance may not be: **when a harness summarises, the
thing it claims must appear in what it prints.**

#### Print — started 2026-07-28, measurement first

The remaining Phase 1 item, and one of the two this plan flags as quietly large. The decision
that everything else follows from is whether tpdf **hands the PDF to the operating system** or
**renders pages itself**, so that is what was measured before anything was written.

The answer is unambiguous on macOS. Asking CUPS what the configured printer actually receives —
`cupsfilter -d <queue> file.pdf` — produced output **byte-identical to the input file**. That
printer is PDF-native, as AirPrint and IPP Everywhere devices are, so the document reaches it
untouched and there is nothing we could add by rendering it ourselves. Rasterising first would
be strictly destructive.

Where a printer is *not* PDF-native the conversion is CUPS's, and its quality is not ours to
control: `cupsfilter -m application/postscript` turned the same one-page fixture from 18,840
bytes into 98,231, with image operators present and **no embedded fonts at all** (`/Type42` and
`/Type1` both absent) — i.e. the page became a raster. Worth knowing, and not worth working
around: every other macOS application is on that same path, and pre-rasterising ourselves at a
resolution we guessed would only make it worse.

**The first version of this measurement proved nothing, and the tell was a byte count.** It ran
`cupsfilter -m application/pdf` and compared extractable text before and after: 177 characters
in, 177 out, which reads like a clean vector round trip. Both files were 18,840 bytes. Asking a
converter for the format it already has is a copy, so the chain under test was the identity and
the result was guaranteed by construction — the same shape as every other precondition already
recorded here, wearing a MIME type. **A conversion is only evidence once something has been
shown to have been converted.**

So the architecture: hand over a PDF, never pixels. What that leaves as genuinely ours is
producing the *right* PDF — a page range, and the view rotation if the reader wants what they
see — which is `lopdf` page subsetting and is platform-independent and testable. The platform
half is the print dialog: `objc2-app-kit` already exposes `NSPrintOperation`, `NSPrintPanel`
and `NSPrintInfo`, and both it and `objc2` are already linked transitively under MIT, so no new
dependency and no licensing question. PDFKit has no bindings crate and would need direct
`msg_send!` interop on the main thread.

##### The half that is ours: building the right PDF

`src-tauri/src/print.rs` produces the bytes to hand over. Three cases, and the first is the
one worth arguing for: **everything unrotated is the file itself**, byte for byte. Rewriting a
document in order to change nothing about it is pure risk — `lopdf` drops encryption silently
(AGENTS.md), a rewrite reflows structure, and the printer was going to receive these exact
bytes anyway.

A page range **deletes pages in place** rather than re-parenting the survivors under a fresh
`/Pages`. That is the whole design decision: `/Resources`, `/MediaBox`, `/CropBox` and
`/Rotate` are inheritable, and a page moved out from under its parent loses whatever it was
inheriting — after which it still opens, still counts as a page, and prints blank. A view
rotation composes onto each page's *effective* `/Rotate`, resolved up the `/Parent` chain,
because the literal one is absent on exactly the documents that inherit it. The outline is
dropped whenever pages are, since its destinations name pages the file no longer has.

The mark-and-sweep moved out of `bin/sanitize_rewrite.rs` into `src/sweep.rs`, since printing a
range needs the same walk. That refactor was verified rather than assumed: the spike's eleven
rewritten fixtures are **byte-identical before and after the move**, checked by running the
pre-move code as a control. (One of them, `hostile-stale`, differs from lopdf's own collector
— it already did, which is why the control was necessary to say so.)

##### Eleven mutations, and four of the tests were wrong

All eleven behave as predicted now. They did not at first: **four survived**, and every one was
a defect in a test rather than in the code. Two are general enough to be in `AGENTS.md`.

- **The passthrough fixture was written by `lopdf`**, so loading and saving it reproduced it
  byte for byte and "the file was handed over untouched" was equally true of a full rewrite.
  Both passthrough mutations survived. Fixed in the fixture — a tail past `%%EOF` that readers
  tolerate and no serialiser emits — not in the assertion.
- **The inheritance test used a more forgiving oracle than a renderer.** `lopdf`'s
  `get_page_fonts` merges the page's resources with every ancestor's; PDF makes an inherited
  attribute one the page's own dictionary *replaces*. So a page carrying an empty `/Resources`
  — precisely the defect — still reported the font, and the mutation modelling it survived.
- **"Fewer objects than before" did not test the sweep.** Deleting a page removes the page
  object, so the count falls whether or not anything was collected. What only the sweep can
  remove is the content stream a deleted page pointed at, so that is what is named and looked
  for now.

One mutation is predicted to **hang** rather than fail: unbounding the `/Parent` walk spins on
a cyclic document, and a test that never returns prints no failure line — indistinguishable
from a mutation nobody noticed. The runner has a timeout and reports that as its own outcome
rather than folding it into either answer.

##### The platform half: PDFKit, and a parser that did not write the file

`src-tauri/src/print_macos.rs` opens the panel. PDFKit builds the `NSPrintOperation`, AppKit
runs it, `MainThreadMarker` carries the one real requirement in the type system, and the
command builds the job off the main thread so a 337 MB scan is not parsed on the thread the
webview draws on.

One correction to the note above: **PDFKit does have a bindings crate.** `objc2-pdf-kit` 0.3.2
exists, matches the `objc2` 0.6 already in the tree, and is `Zlib OR Apache-2.0 OR MIT`, so
there is no `msg_send!` interop and no licensing question. It is the only genuinely new
dependency; `objc2`, `objc2-foundation` and `objc2-app-kit` were already there via Tauri.

Two decisions are stated in the code and **neither is verified, because both need paper**:
pages are scaled down to fit, or an A0 sheet prints its top-left corner; and PDFKit's
auto-rotate is offered only when no page carries a rotation, since it turns a page to fill the
sheet and would otherwise spin back the exact turn the reader asked for.

The half worth the writeup is [`print_macos::read`], which is not on the printing path at all.
Every check on the print job had been reading it back with `lopdf` --- the library that wrote
it --- and that tests the round trip rather than the document. The mutation demonstrating it
leaves `/Pages /Count` at its pre-subset value: every `lopdf` check passes, and PDFKit reports
**five pages for a two-page document**, the two real ones followed by three blank pages it
manufactures to satisfy the count. Two correct sheets and three blank ones, invisible to the
writer's own reader. That is now an `AGENTS.md` entry, and the three `a_third_parser_*` checks
assert through PDFKit instead.

##### Two defects the real corpora found, and a third the profile nearly hid

The synthetic fixtures said the print path was fine. Running it over the actual documents said
otherwise, twice, and both were on the critical path to a print panel.

**`lopdf::delete_pages` does not scale.** It calls `delete_object` per page, and that calls
`traverse_objects` --- the quadratic walk this plan already recorded for `prune_objects`, here
run once *per deleted page*. Keeping two pages of the 775-page corpus: **620.5 ms**. A single
pass doing the same work --- drop `/Kids` entries and dictionary keys naming a doomed page,
decrement `/Count` up every `/Parent` chain --- costs **1.2 ms**, a 533x difference, and its
output is **byte-identical** on the synthetic fixture and on six corpora. `incr-xrefstream`
reproduces it at 663.1 ms against 1.0 ms. The byte comparison is kept as a test.

**The verification was the expensive half.** `print_macos::read` extracted every page's text,
which only the checks use, and that is **1,017 ms** on 775 pages and **467 ms** on twelve A0
pages --- a second of waiting in front of a print panel to fill a field nothing on that path
reads. Split into a structural read (count and rotations, **62 ms** and **0.6 ms**) and a
text-carrying one for the checks. `PageReading::text` became `Option<String>` in the process,
because "not extracted" and "no extractable text" are different facts and one empty string for
both is the leak-scanner defect again.

**And the first number here was a debug-profile measurement, written into a doc comment as
fact.** `delete_pages` measured 15,912 ms under `cargo test` and 620 ms under
`cargo test --release` --- 26x apart. The conclusion survived; the number would not have. Now
an `AGENTS.md` entry, because the existing rule named `tauri dev` and this arrived through a
test runner.

##### Eleven more mutations, and a page tree with a middle

Eleven, all as predicted, plus the earlier eleven re-run as a control on the refactor --- since
`build` changed underneath them, their previous result no longer said anything.

One predicted **survivor** is kept rather than dropped: `pageCount` disagreeing with what
`pageAtIndex` produces has no fixture that can provoke it, so that guard is unpinned and known
to be.

A second predicted survivor was closed instead. Deleting a page must decrement `/Count` on
**every** ancestor, and every fixture here built its pages directly under the root --- where
"the page's parent" and "the whole chain" are the same node, so a walk that stops after one
step is indistinguishable from a correct one. Real producers balance the tree. A nested fixture
(three groups of two, resources two levels up) makes the mutation fail, and the check asserts
different deltas at different levels so that decrementing per *group* rather than per *page* is
wrong in the other direction.

`⌘P` is bound, and prevented even with no document open --- WKWebView's own `⌘P` prints the
*chrome*. No page-range field of ours: the system panel has one, and its numbers refer to the
document handed over, which is every page. `print::build` takes a range because printing
selected thumbnails will need it, not because anything asks today.

**Windows is not written**, and `present_job` says so with an error rather than doing nothing.

#### The worker boundary — started 2026-07-28, parent half landed

The one Phase 0 constraint that never landed. Every PDF is still parsed in the app process
today, and `AGENTS.md` is explicit that this cannot be a later hardening pass — retrofitting
a process boundary is an architectural rewrite, so it is one now rather than one later. Note
the justification is `docs/THREAT-MODEL.md` and **not** the coverage floor: measured above, a
pool buys 3.2× on a screenful of the A0 sheet and leaves it just as unscrollable.

`src/worker.rs` is the parent half and the shared contract: the protocol, the shared mapping,
spawn/call/withdraw, the epitaph, footprint supervision, and the measured SBPL profile. Every
design decision in it is a spike 0.5 number rather than a preference — the document crosses as
a **mapped descriptor and never a path**, which is the whole reason a sandbox that denies
`file-read*` can work at all; payloads cross through the mapping at 0.11 ms against 0.61 ms
down the pipe; `file-read-metadata` is allowed back because font lookup stats paths and
denying it makes PDFium substitute a typeface *silently*.

Two choices worth stating because they are not forced. **One worker serves one document** —
stronger isolation than multiplexing, and it makes a worker restartable with no reopening
protocol. And the worker is this executable re-exec'd with `--render-worker`, not a second
binary, because a path that resolves in development and not in the bundle is a defect this
file already records once for the PDFium library directory.

Seven tests, and one that clippy correctly refused: `TILE_CAPACITY >= 2048²×4` is two
constants, so it could not fail at runtime any more than `2 + 2 == 4` can. It is a `const`
assertion now, where it also cannot drift.

##### The child half, and what it is measured against

`src/worker_child.rs` is the other side, and its ordering *is* the security argument: PDFium
is bound **before** `sandbox_init`, because binding opens and maps the dylib; the document is
opened **after**, because that is the attacker's input. Move either across that line and it
breaks or is defeated, and neither shows as an error — a sandbox applied too late still
returns `ok`.

Two threads, and they cannot be one. A withdrawal exists to reach a render *already running*,
so it has to be read while the render thread is inside PDFium. The reader thread never touches
the document, which is what makes that sound — `RawDocument` is not `Send`, and concurrent
PDFium is undefined behaviour whatever the handles are. The queue's claim/withdraw state
machine is reused verbatim from `queue.rs` rather than rewritten.

`bin/worker_probe.rs` is the evidence, and the load-bearing check compares **pixels** against
an in-process render of the same tile — because a sandbox that renders the wrong typeface
returns `ok` and this file already records that happening. 12/12 on `text-heavy`,
`vector-heavy` and `rotated-90`:

| | text-heavy | vector-heavy (A0) | rotated-90 |
|---|---|---|---|
| pixels vs in-process | identical | identical | identical |
| 1 MB tile across the boundary | 2.1 ms | 433.6 ms | 0.4 ms |
| worker footprint | 17.5 MB | 48.2 MB | 7.8 MB |

The A0 figure is the render, not the boundary: the same tile in-process costs the same, which
is the point of comparing rather than timing.

**Two defects, both found by running it rather than by reading it.** `mmap` refused the
document with `EACCES` — mapped `PROT_WRITE` off a read-only descriptor. The fix is not a
wider open: a worker holding a *writable* mapping of the reader's own file could corrupt the
document it was asked to display, which is exactly the authority the boundary withholds. The
kernel refused it before the threat model did.

And the first withdrawal check was wrong, not the code. `Queue::withdraw` ignores an id it has
never seen — deliberately, since remembering them is what lets its tables grow without bound —
so withdrawing *before* sending the request is a no-op, and that is what the check did. It now
pipelines two tiles and withdraws the second, with the first tile's render as the window, and
prints that window: 0.1 ms on `rotated-90`, which is a thinner margin than it looks and is why
the number is in the output.

**Still to come:** a pool, and reclaiming a document nobody is reading. `Worker::spawn` and
`apply_sandbox` both refuse on Windows rather than running unsandboxed — the correct
half-answer until a Windows build has ever run.

**A dead worker was not replaced** when this was written; *The service survives its worker*
below closed it on 2026-07-28, and the shape predicted here is the shape it took — one
worker serves one document, so there was nothing to re-establish but the document itself.

**A document was never released, and that made the leak a process.** Closed on 2026-07-28 —
see *A document can be released* below. The question this paragraph left open, "is any
request still naming this id", turned out not to need an answer: the render thread is FIFO,
so a close lands behind everything already queued for that document.

##### The service switches over — 2026-07-28

`RenderService` now runs on either backend, chosen by `Backend` and defaulting to
`Backend::Worker` on macOS. The in-process path stays, and not out of sentiment: it is the
control the worker is compared against, and `TPDF_BACKEND=in-process` selects it. An
unrecognised value is **refused**, because the whole purpose of the variable is to pin down
which implementation ran, and a typo that silently selected the other one would let a
comparison between them report anything it liked.

What actually changed above the module is small: the app process no longer binds PDFium, a
reply can now fail because the worker died, and a withdrawal grew a second half. Everything
else — the callback shape, the FIFO ordering, the queue's semantics — is what it was, which
is what the interface was shaped for in Phase 0.

`bin/backend_probe.rs` is the comparison, at the level callers use rather than at the
protocol: one service per backend, driven through the same public methods the viewer calls.
Sixteen checks, of which the pixel comparison is the one that matters — a sandboxed PDFium
that renders the wrong typeface returns `ok`, and this project has recorded that happening.
On `text-heavy`, `vector-heavy`, `vector-multi`, `outline-simple`, `rotated-90` and
`text-cid` the two backends agree byte for byte on tiles, page geometry, character boxes,
search ranges and outlines.

The claim it makes that nothing else can is that **the app process never maps libpdfium at
all**. That is read out of the dynamic linker's own image table, not out of a startup mark
of ours: a mark says what our code believes it did, and the question is what the process
*is*. 615 images loaded with no pdfium among them after a 775-page document has been opened
and a tile rendered from it; 616 with it present once the in-process service starts, which
is the control saying the scan can see one.

**Ten mutations, eight caught, two survivors that were predicted.** Writing the predictions
down first paid for itself twice, the same way it did on the page strip.

| mutation | which check went red |
|---|---|
| worker mode silently runs in-process | the linker scan, and the `worker spawned` mark |
| the view rotation is not sent | a turned and inverted tile is identical too |
| the inversion is not sent | a turned and inverted tile is identical too |
| the tile origin is transposed | a tile is identical whichever backend rendered it |
| the open is always lazy | page geometry crosses the boundary unchanged |
| text always reads the first page | one page's characters and boxes survive |
| a withdrawal never crosses the pipe | a withdrawal reaches a render already inside PDFium |
| a withdrawal never reaches the queue | a tile withdrawn before it starts comes back abandoned |
| the lost-race guard is deleted | **nothing** — see below |
| the parent trusts the worker's payload length | **nothing** — pinned by unit tests instead |

Three things came out of that run, and none of them is the table.

**The withdrawal check could not have failed as first written.** It asserted that a
withdrawn tile comes back `Abandoned`, which the parent's own token produces whatever the
worker did — so deleting the wire withdrawal entirely left it green while the worker burnt a
full second of CPU on a tile nobody wanted. What discriminates is *when* the reply arrives:
2.2 ms against a 1,125 ms render with the withdrawal crossing, and the whole render without
it. The assertion is now the outcome *and* the latency, with the threshold taken from that
render's measured time rather than from a constant.

**The compared tile had to be placed from the document, not fixed.** A rectangle at a fixed
offset lands in `rotated-90`'s margin, and the uniform-buffer control fired: two backends
agreeing about an empty tile is not evidence of anything. It is now sized and placed from
the page's own geometry — and deliberately not square, not at the origin and not at 1x,
because a request whose width equals its height and whose `x` equals its `y` cannot tell a
field that was dropped in translation from one that arrived.

**One guard is knowingly unpinned**, and it is recorded here rather than deleted or quietly
kept. If a withdrawal wins the race to the pipe and arrives *before* the tile it names, the
worker's queue no-ops it and renders anyway; the parent then returns `Abandoned` on its own
token. No check can provoke that interleaving, so by this project's standing rule it is a
guard to delete — except that rule is about guards whose condition is *unreachable*, and
this one's is merely rare. Deleting it would hand the reader a stale tile they had
withdrawn, in a window that exists.

###### What the boundary costs at startup

Measured the same day with interleaved variants (`startup_bench.py --variant`, two runs of
8 and 14 rounds, `text-heavy`, release bundle). The boundary is on the critical path to the
first page, so it had to be measured rather than waved through on spike 0.5's 6 µs control
latency — that number is the round trip, not the spawn.

| interval | in-process | worker |
|---|---|---|
| `document open requested` → `document open complete` | **1.2 ms** | **12.0 ms** (3.1 to spawn, 8.9 to bind PDFium, sandbox and parse) |
| → `first tile rendered` | +5.5 ms | +10.4 ms |
| first page presented, warm | 282–284 ms | 287–295 ms |

Three statistics agree on the size of it: the open interval is +10.8 ms, and the pairwise
within-round deltas are −10.3 ms and −17.7 ms across the two runs. Individual rounds go both
ways, which is why the end-to-end medians (+2.4 ms in one run, +12.8 ms in the other) are the
weakest of the three and not the one to quote.

So the process boundary costs **roughly 11–16 ms of a ~50 ms application budget**, against a
~250 ms shell floor nothing on our side moves. It is affordable and it is not free, and the
consequence to carry is that warm start is now 287–295 ms against a 300 ms target: the margin
that lazy page geometry bought has largely been spent. The next thing to want here is a
worker started *before* the document is chosen, since 8.9 of those milliseconds are PDFium
binding and have nothing to do with which file it is.

The two bounds on the parent's side of the pipe are pinned by unit tests instead, and one of
those tests was wrong: it fed 4,096 bytes with no newline to a 64-byte limit and asserted
"too long", which **passes with the bound deleted** — an unbounded read consumes the lot,
hits EOF, and is refused for the other reason. The input is now a complete line that is
merely too long, and the assertion is the reader's *position* afterwards. The property is
"it stopped reading", and no statement about the return value can express that.

##### The service survives its worker — 2026-07-28

Isolation that ends the reading session is isolation nobody wants. A worker that dies is now
replaced, and the request that found it dead is retried against the replacement — so the
common case, a death caused by something other than the request in hand, is invisible to the
reader.

Three decisions, and each of them is the interesting half.

**The replacement is handed the same mapping, not the same path.** `Held` owns the document
`Shm` behind an `Arc` and `Worker::spawn_shared` borrows it, so re-opening reads no file and
cannot pick up a different one. Re-reading the path is the obvious implementation and is
quietly wrong: a document rewritten between the death and the restart would become what the
reader is looking at, under a scroller sized for the old one, with nothing to say so. It also
means a 337 MB scan is not read a second time. The property holds by construction — there is
no path stored to re-read — rather than by a check, which is the right place for it.

**The bound on a crash loop is the retry, not a counter.** `with_worker` tries once more and
no further. A crash the document *causes* reproduces on that retry, so the reader pays two
crashes for that tile and gets an error, and the next request pays one more; the whole thing
is bounded by the requests the reader makes. A restart budget on top of that would be
unreachable defence of exactly the kind `AGENTS.md` says to delete — there is no loop left
for one to break. What it costs is stated rather than hidden: a document that reliably kills
its worker on one page spawns a process per attempt at that page.

**A worker that answers with an error is not replaced.** A live process that said "no such
page" has answered; restarting it would spend a document reopen on every malformed request
and hide a bug in the protocol behind a fresh process that gets the next question right. The
discriminator is `Worker::is_running`, i.e. `try_wait`, which asks the kernel rather than
inferring death from a failed call.

`backend-probe` grew seven checks for this, and every observation of a *process* comes from
the OS table (`pgrep -P`) rather than from our own `Vec<Held>` — the same reason the
libpdfium check reads dyld's image list. On `vector-heavy`: **23/23**.

**Six mutations, five caught, one predicted survivor.**

| mutation | which check went red |
|---|---|
| `with_worker` never retries | the tile returns; a new process; a withdrawal reaches the replacement; the text path |
| the replacement gets a *different* document | the tile returns — plus two by overlap |
| the restart does not re-point the sender | **only** "a withdrawal reaches the replacement too" |
| a dead worker looks alive | the same four as the first |
| a live worker looks dead | "a worker that answers with an error is not replaced" |
| `live()` never restarts a `None` worker | **nothing** — predicted |

The survivor is honest and stays: that branch is reachable only after a *spawn* failure,
which no fixture provokes, and deleting it would leave a document permanently dead after one
transient failure to fork. The third row is the one that earned its keep — a withdrawal sent
down a dead pipe still comes back `Abandoned` on this side's own token, so the check that
sees it is the latency (`abandoned after 1101.7 ms, against a 1153 ms render`), for exactly
the reason the first withdrawal check needed the same treatment.

Two findings were about the checks rather than the code, and both are in `AGENTS.md`.
`SIGSEGV` does not kill a Rust process the first time it is *sent* — std's stack-overflow
handler restores the default and returns — so the first version of this killed nothing and
two checks passed against a worker that was still alive. And the crash checks were nested
inside `if let Some(victim) = worker_pids().first()`, so the mutation that stops workers being
replaced made them **vanish** rather than fail: no `[FAIL]`, no `[SKIP]`, and the only trace
was the total dropping from 23 to 22. Both were caught by controls that existed because the
prediction was written down first.

**Still not done:** a pool, which is two sections below.

##### A document can be released — 2026-07-28

The leak that the boundary made expensive: `open` appended and nothing ever removed, so a
window that moved to another file left the first one's worker alive, holding the 7.8–48.2 MB
`worker-probe` measured per corpus. `RenderService::close` releases it, and `App.svelte`
calls it when the reader opens something else.

The question the earlier note left open — *is any request still naming this id* — did not
need an answer. **The render thread is FIFO**, so a close is queued behind everything the
outgoing document already had outstanding, and there is no instant at which a request
outlives its document. No reference count, no epoch, no lock.

What did need deciding is what a released id *becomes*. It leaves a **hole**: the `Vec`
index is the id, so removing the entry renumbers every document after it, and a request
naming the closed id is then answered in full from a document the caller has never asked
about. That is not a hypothesis — the mutation that removes instead of holing returned
`rendered 1048576 bytes` of the wrong file, and the check that caught it was the one
asserting a *refusal*. Ids are therefore never reused, and the two failures are named apart:
past the end is a caller that invented an id, a hole is a caller still using one it closed.

Five checks in `backend-probe`, two documents open at once so that a close has something to
be measured against — "the worker is gone" is otherwise equally true of a close that killed
every worker there was, which is the failure that matters to a reader with two files open.
On `vector-heavy`: **29/29**.

**Four mutations, four caught — but two of them only after the run found real defects.**

| mutation | which check went red |
|---|---|
| close leaves the slot in place | the process is killed; a closed document is refused |
| close removes the entry instead of holing it | the *other* document renders; the refusal; the id is not reused |
| a hole reads as out of range | the refusal's wording — **only after the message was shared** |
| close does not clear the sender slot | the descriptor count — **only after that check existed** |

The third survived at first because `open_slot` and `open_slot_mut` each spelled the
distinction out, and the worker path goes through one while the in-process tile path goes
through the other: each check passed through whichever copy was still right. The fourth
survived because nothing counted anything. Its leak is a descriptor per document ever
opened — the withdrawal broadcast holds a *clone* of the worker's `ChildStdin`, so killing
the worker does not close the pipe — and it has no functional symptom at all, because writing
to a dead pipe fails harmlessly. `/dev/fd` is what discriminates: 9 descriptors before the
second open and 9 after the close, against 10 with the clearing removed.

One thing is pinned less tightly than the rest, and it is worth saying so. `backend-probe`
drives `RenderService` directly, so it cannot see whether the **Tauri command** exists under
the name `App.svelte` invokes or takes an argument called `doc` — a mistake there fails only
at runtime, only when a second file is opened, and only as a console warning. `viewercheck.ts`
covers that seam with two checks, the second being the control: releasing the same id twice
must be refused *by that id*, since "no error" is equally true of a command that ignored its
argument.

##### The pool — 2026-07-28

The last Phase 1 item. The worker backend is now served by several threads sharing one job
queue, and each document has a pool of processes they draw from, so several tiles of the same
page render at once. The in-process backend is deliberately **not** pooled: concurrent PDFium
in one process is undefined behaviour whatever the handles are, which is the reason security
and performance wanted the same architecture in the first place.

`bin/pool_bench.rs` measures it through `RenderService` rather than the raw protocol, six
1024-square tiles of the A0 sheet, interleaved across rounds and compared pairwise within a
round. Two runs:

| workers | 1 | 2 | 4 | 6 | 8 |
|---|---|---|---|---|---|
| screenful | 3457–3465 ms | 1800–1868 ms | 1263–1299 ms | 830–837 ms | 843–851 ms |
| speedup | 1.00x | 1.92–1.94x | 2.67–2.93x | 4.15–4.18x | 4.07–4.12x |

Six is the default because that is where the curve flattens — eight is slower by less than
the spread, so read it as flat. Note six is neither the core count (10) nor the
performance-core count (4). Two runs are quoted rather than one because the four-worker
figure moved 2.67–2.93x between them while six moved 0.03x; a single run would have
presented that as a measurement. On a *cheap* corpus the pool helps too and does not hurt:
`text-heavy` goes 6.5 ms → 2.5 ms, 2.69x.

**Growth is lazy, and that is what makes it affordable.** A document opens with one worker
and gains another only when a request arrives while the first is busy — so a reader turning
one page at a time never pays for a second parse. A fully grown pool on the A0 sheet is about
290 MB, which is the cost of the number and is stated in `docs/THREAT-MODEL.md` as a
residual, because nothing retires an idle worker afterwards.

**`close` drains.** Dequeue order is still FIFO — one channel — so a close is taken off the
queue after everything queued before it; but with several threads those requests may still be
*running*, in workers the close is about to kill. It waits for the pool to come home first,
which is the guarantee the single-threaded version got for free.

**Five mutations, five caught — after two of them exposed a design fault rather than a test
gap.**

| mutation | which check went red |
|---|---|
| the pool never grows | the pool grew; the recovery tile; the dead worker; the oversized burst |
| the pool grows without bound | "an oversized burst is served, and the pool stays at its ceiling" |
| `close` does not drain | "a close waits for the render it interrupted" |
| a discarded worker's slot is not given back | the close, which never returns; and the closed id, still answering |
| one service thread instead of a pool of them | "concurrent tiles grew the pool" |

The two that first survived are the interesting ones. **The thread count was doing the
capacity bound's job**: with one thread per worker, `idle` can only be empty when every worker
is checked out, which takes one thread each, so a thread arriving to find none free cannot
exist — both the ceiling and the wait beside it were unreachable, and removing the ceiling
changed nothing any check could see. Threads are now `pool + 2`, which makes both reachable
*and* fixes a real starvation: with exactly `pool` threads, six tiles of a slow document
occupy every one of them and a request for a second document waits behind a render while its
own workers sit idle.

Even then it took a second correction. The burst provoking contention was `capacity + 1`
tiles and the extra one was the tile being *withdrawn* — and a withdrawal is refused at the
claim, before a worker is checked out, so the burst could never demand more than `capacity`
workers however the cap behaved. A surplus that gets cancelled is not a surplus.

**And several properties here fail by not answering at all.** A pool that believes in a
worker it retired never finishes a close; a checkout waits for a process that will never
exist. Written with a blocking receive, those checks could only stop, never go red. The
probe's `wait` now has a 60 s bound against a 1.2 s render and reports *"the service is
wedged, not slow"*; the mutation harness keeps the partial transcript on timeout, because one
mutation turned a check red **and then** wedged the run — and a harness that reads a timeout
as "no result" throws away a correct red.

##### Retiring idle workers — 2026-07-28

The last thing the pool was missing, and the reason the section above had to state its
cost as a standing residual: growth is driven by contention, contention is a *burst*, and
the burst is over long before the reader is. A worker idle past `DEFAULT_IDLE` (30 s) is
now killed, down to one per document, by a reaper thread sweeping every quarter of that.

`pool-bench --mode retire` measures both halves. Two runs each, per the standing rule:

| corpus | one worker | grown | retired | given back | next screenful |
|---|---|---|---|---|---|
| `vector-heavy` (A0) | 47.3–47.4 MB | 289.8–289.9 MB | 48.4 MB | **242.5 MB** | +64.6 / +66.9 ms on 811–814 ms |
| `text-heavy` | 15.9 MB | 72.0–72.1 MB | 10.3–10.4 MB | **56.1–56.2 MB** | +14.7 / +14.9 ms on 2.4–2.6 ms |

So it gives back 84% of a grown A0 pool and charges the screenful after the pause about
8% of itself for it — five workers respawning and re-parsing concurrently, against a
44 ms page parse. On the cheap corpus the ratio reads far worse (a 15 ms penalty on a
2.5 ms screenful) and the absolute number is the one that matters: 15 ms, once, after
someone has stopped scrolling for half a minute.

Two notes on reading that table. The `retired` column is a *different process* from the
`at open` one — the survivor is whichever worker was hottest, not the original — so on
`text-heavy` it lands below the open figure and is not a floor. And footprint excludes
clean file-backed pages, so none of these numbers include the document itself.

**One worker is kept, and zero was the tempting alternative.** Nothing breaks at zero:
the checkout path spawns from `spawned == 0` and the close drain is trivially satisfied
by it. What it costs is a spawn plus a full re-parse charged to the *next page turn* —
the stall landing exactly when someone is watching — to save the 7.8–48.2 MB of one
process. Retiring to one already returns five sixths of a full pool.

**The reaper holds a `Weak`.** With a strong `Arc<Workers>` it would keep every worker,
and every document mapping, alive for the life of the process after the last handle to
the service was dropped — a worse leak than the one it exists to fix, and invisible to
every other check here, all of which run against a service that is still alive. That is
the same shape as the spare that outlived its parent, and it needed the same answer: a
check that drops a service and asks the OS whether its processes went with it.

**Six mutations, six caught — after the harness reported one of them as a survivor.**

| mutation | which check went red |
|---|---|
| the reaper never runs | the pool retired; the descriptor count |
| retirement ignores the idle timeout | the control: a worker idle for less than its timeout |
| the last worker is retired too | the pool retired (**0** workers, not 1); the descriptor count |
| retirement does not lower `spawned` | the regrowth burst; the close, which wedges |
| the withdrawal sender is left behind | the descriptor count |
| the reaper holds a strong handle | dropping a service kills the workers it owned |

Three things came out of it worth more than the table.

**The control is the whole check.** "The pool shrank to one" is equally satisfied by a
reaper that kills everything it finds on every sweep, which is not retirement but a pool
of one with extra steps. The sample taken *before* the timeout is what discriminates, and
it is the only thing the second mutation turned red.

**Two of the mutations fail by not answering.** A pool that keeps a ceiling nothing is
under blocks the next checkout forever; the close that drains against `spawned` does the
same. Both are bounded here — the burst by a measured multiple of a render, the close by
the probe's 60 s answer bound — because a check whose failure mode is a wait cannot fail.

**And the harness lost a red.** Its regex wanted two spaces between a check's name and
its detail; the probe pads names to 56, so a 55-character name is followed by one space
and matched nothing. It reported `SURVIVED` while its own summary line in the same buffer
said one check had failed. That is the defect `AGENTS.md` already records from the
`search.rs` harness, reproduced by someone who had read the entry — the lesson that did
not transfer was not "beware regexes" but the *repair*: derive the fact both ways and make
the harness compare them. It does now, and a mismatch is reported as a broken run rather
than as either answer.

**Still to come here:** nothing. Phase 1's worker backend is complete.

#### The viewer runs on Windows — 2026-07-29

The previous entry left Windows compiling and gating green with **nothing ever run** on it,
and `BUILD.md` said not to claim a Windows build worked until one had opened a document. One
has. Four corpora through `viewer_check.py`, every one reporting the **86 check names** that
are the invariant, with ran/skipped splits inside the macOS ranges:

| fixture | ran | skipped | failed |
|---|---|---|---|
| `outline-simple.pdf` | 81 | 5 | 0 |
| `outline-hostile.pdf` | 81 | 5 | 0 |
| `rotated-90.pdf` | 75 | 11 | 0 |
| `vector-heavy.pdf` | 52 | 34 | 0 |

The harness needed no changes: `webview_guard` already returns early off darwin, and WebView2
wants no bundle identity, so a plain `tpdf.exe` runs where macOS needs an `.app`.

**Three defects, and no amount of compiling would have found any of them.** That is the
result worth carrying, more than the table: the platform had been green for a day, and each
of these was on the critical path to a first painted pixel.

**`npm run tauri build` failed on a tree that gated 7/7.** `backend_probe.rs` called
`_dyld_image_count` and `_dyld_get_image_name` with no `cfg`. Neither gate could see it:
clippy stops at metadata and never links, and `cargo test` *does* link each `[[bin]]` but with
`main` replaced by the harness's own, so a symbol reachable only from `main` is dead code the
linker drops. The gate list now carries `cargo build --locked --bins`, and it was proved to
fail against the un-gated file — 5.7 s, red, in the **debug** profile, checked separately
because the original observation was a release build and the entire finding is that linking
depends on how the target was built. The probe is now a thin entry over `backend_probe/imp.rs`
refusing off macOS, the shape `fdpass_probe.rs` already used.

**Not one tile was ever painted, and nothing reported an error.** `tiles.ts` fetched
`tile://localhost/...`; WebView2 cannot register a URI scheme, so Tauri serves custom
protocols at `http://tile.localhost/...` there. PDFium bound at 262 ms, the document parsed,
twelve pages laid out, the page fitted the window, the frame loop ran, scrolling worked — and
every coverage check read `sharp=0.0%`. Everything that does not need a tile worked, which is
what a viewer looks like when the only broken subsystem is the one that draws. The origin now
comes from Tauri's own `convertFileSrc`, for the origin only: handed a whole path it
percent-encodes the separators the server splits on. The CSP gained `http://tile.localhost` —
and it **already carried `http://ipc.localhost`**, so the convention was understood and
applied to one scheme and not the other, which is what a platform-conditional spelling does
when it is written by hand in two places instead of derived once.

**`cargo build --release` is not a production build.** It produced a window displaying the
webview's own *"localhost refused to connect"*. `frontendDist` is embedded under the cargo
feature `tauri/custom-protocol` — `tauri`'s `build.rs` computes `dev = !has_feature(..)` — and
the profile has nothing to do with it. Verified in both directions rather than read off the
source: the same tree built with that feature passed the check 84/84.

Five checks were added for the tile origin, and four mutations run against them, each
matching its prediction: hardcoding the macOS scheme in the URL turns one red, hardcoding it
in the origin turns three, dropping the memo turns one, and encoding the whole path turns two.
The harness's own cross-check earned its place immediately — it parsed twice as many failures
as vitest's summary reported, because `FAIL ` matches the file-level block as well as each
test, and it said so instead of reporting either number.

**Still open, and it is the whole of it:** Windows has no containment.
`Backend::default_here()` selects in-process off macOS, so hostile input is parsed in the app
process and it fails open rather than refusing. The viewer working there raises the stakes on
that rather than settling anything.

It is at least no longer silent. The uncontained default records `UNSANDBOXED_MARK` on the
startup timeline and prints a `[WARN]`, so an uncontained run is distinguishable after the
fact — which it was not, and the timeline is what every harness here already reads. A mark
rather than a refusal is deliberate: refusing makes the platform useless rather than
uncontained, and that is a product decision, not a defect to fix while passing through.

Adding it immediately exposed a second silence one level out. `viewer_check.py` echoed the
child's stderr only when the run failed, so the new warning was invisible to a run that
passed — a full-marks Windows run showed no trace of it, and the first reading was that the
warning had not fired at all. The general form is worth carrying: a diagnostic whose whole
purpose is *"this run was fine, but you should know X"* is precisely what the usual
stderr-on-failure convention suppresses.

#### Spike 0.7 — what Windows containment can be (2026-07-29)

`bin/win_sandbox_probe.rs`, in the shape spike 0.5 established: re-exec this binary as a
contained child, render one tile, compare **pixels** against an in-process render. Pixels
rather than exit codes, because the macOS work already recorded a sandboxed PDFium returning
`ok` while silently substituting a typeface, and the Windows font path has no reason to be
kinder — the default fixture is `text-base14.pdf` precisely because base-14 faces are not
embedded and must be found on the system.

Six rungs, four real and two diagnostic:

| rung | renders | identical | exit | denies |
|------|---------|-----------|------|--------|
| `bare` (control) | yes | yes | 0 | nothing |
| `job` | yes | yes | 0 | memory over 512 MB, a second process, orphans |
| `lowil` | **yes** | **yes** | 0 | writing `%USERPROFILE%`, `OpenProcess` on the parent |
| `noprivs` *(diagnostic)* | yes | yes | 0 | nothing |
| `sidonly` *(diagnostic)* | no | — | `STATUS_DLL_NOT_FOUND` | — |
| `restricted` | no | — | `STATUS_DLL_NOT_FOUND` | — |

**The answer is a job object plus low integrity.** PDFium renders byte-identically under it,
so the font risk did not materialise, and the process loses the authority to write anything
or to reach into the app process. What it does *not* lose is the ability to **read** — an
integrity level governs writes — which is why the child is handed its document and its output
as inherited handles and never opens a path: that is the Windows analogue of the `dup2` the
macOS worker does, and proving it works is half of what a real worker needs.

**The stronger rung is not reachable by adding a flag.** A restricting SID (`S-1-5-12`) makes
every access check consult the restricted list too, system DLLs grant it nothing, and the
child dies in the loader before `main`. That is the exact inverse of the macOS ordering rule
— there the process applies the boundary to itself and there is a "before" in which to bind
PDFium; here the token is in force from the first instruction. Chromium's answer is an
initial impersonation token for startup handed over to a lockdown token afterwards, and that
is a piece of work rather than a parameter.

Two diagnostics earned their place: with `restricted` failing, either ingredient was a
plausible cause, and `sidonly` versus `noprivs` settled it in one run. See `docs/TRAPS.md`
for that and for why `CreateProcessAsUser` rejected the first token derivation outright.

**Still not wired in.** The probe proves the mechanism; no worker uses it. `RenderService`
still selects in-process off macOS, so Windows fails open today exactly as it did this
morning — with the difference that the shape of the fix is now measured rather than assumed.

The first piece of that landed the same day: **`Shm` is real on Windows** (a nameless
section object; see the changelog). That is the shared-buffer half of the transport, and it
removes a refusal that was never a policy — every off-unix constructor returned the
worker-refusal sentence, which reads like containment and was the absence of code. What
remains for a worker: handing the section to the child (`DuplicateHandle`, or inheritance
with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` so a hostile child receives only the two handles it
is given rather than everything marked inheritable, which is what the probe does and says
so), un-gating `worker_child` from `#[cfg(unix)]`, and applying the job object and integrity
level at spawn. `Worker::spawn` refuses off macOS throughout, and should keep refusing until
all of that exists — that refusal *is* about the sandbox, which is the distinction worth
holding on to.

The dependency this needed is `windows-sys`, MIT/Apache and already in the tree transitively,
so it adds no crate — checked with `cargo metadata` rather than assumed, per `AGENTS.md`.

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

1. ~~**Can PDFium round-trip a text object faithfully?**~~ **Answered 2026-07-26** (§6).
   Both PDFium mutation and surgical `lopdf` operator rewriting reproduce the rest of the
   page with zero collateral pixels, so surgical redaction and text editing both stay on
   the roadmap. But *only* the surgical route is faithful in the sense that matters:
   PDFium's regeneration rewrites every operator on the page and discards marked content,
   and its `set_text()` silently emits `.notdef` — or codes for glyphs that do not exist —
   when a character is outside the font's subset. Route A of §6 is therefore the committed
   direction for anything precision-critical, and PDFium mutation is not usable for
   redaction at all.
2. ~~**Protocol** — custom scheme vs alternatives.~~ **Answered 2026-07-26.** 1024²–2048²
   tiles (§4), sent as raw pixels (§3), over the custom scheme. Delivery costs 240–293% of
   rendering, so the scheme is not a zero-copy fast path — but encoding is worse in both
   directions, and on the startup path the whole transfer-and-decode of a 16.6 MB tile is
   8.2 ms against a 374 ms budget. Not where the time goes.
3. ~~**How much of the shell cost is reducible?**~~ **Answered 2026-07-26** (§4). Almost
   none of it, and neither lever this question proposed exists. A smaller initial payload
   buys nothing: a variant that does the same work with no module graph, no Svelte and no
   `@tauri-apps/api` measured −8 ms in one run and +10 ms in another, because the ~45 ms
   both pay is the webview's *first request over a custom protocol* and not the framework —
   whichever request is first pays it. Building the window in the setup hook rather than
   from the config is −0.2 ms, so the cost is the WKWebView itself. The shell floor is
   ~250 ms warm before the first line of application code runs, leaving tpdf about 50 ms,
   of which it currently spends 45. Two things *were* reducible: the 86 ms page-geometry
   walk, which is ours, and Tauri's default macOS menu at ~16 ms, which is a default rather
   than a requirement. Together they take 368 ms warm to **276 ms**. Below the floor there
   is only the native-surface escalation at the end of §4 — an architecture, not a tuning
   pass.
4. ~~**Can `lopdf` safely rewrite a hostile corpus,** or is QPDF required for the rewrite
   path?~~ **Answered 2026-07-26** (§6). `lopdf` is enough: on eleven hostile fixtures a
   collected `lopdf` rewrite reaches the same verdict as QPDF on every one, and the
   dependency set does not have to grow for this. Two conditions attach. Its
   `prune_objects` and `renumber_objects` are quadratic — 1.48 s to collect a 25,583-object
   graph against 70 ms for a mark-and-sweep that produces the same bytes — so the sweep is
   ours to write, which is thirty lines. And it silently drops encryption on save, so the
   save path must preserve it or refuse. QPDF stays worth having for two things collection
   is not: preserving encryption, and object streams, which shrank a 6.1 MB output to
   1.46 MB.
5. ~~**Worker process count and IPC cost** — does multi-process rendering actually meet the
   latency target, given the boundary crossing it adds?~~ **Answered 2026-07-26** (§3). The
   boundary crossing is not a cost worth reasoning about: 6 µs for a control round trip and
   0.11 ms to move a 4 MB tile, against 3.0 ms to hand the same tile to the webview. One
   worker matches the in-process baseline exactly. **Process count should default to the
   performance-core count** — speedup is near-linear to 4 on a 4P+6E machine (3.89×) and
   then buys ~0.4× per further worker, so the efficiency cores belong to background work
   rather than to latency-critical tiles. What remains open is not the cost but the
   *policy*: how many workers a document should get, whether a second document shares the
   pool, and whether a worker is recycled or retired after a page that took seconds.
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
   understand?** Current answer is no, by design — and spike 0.4 measured what that costs
   (§6). Under the rule as written, one stream in an unimplemented filter makes the whole
   document unverifiable, and `/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and
   `/JPXDecode` all qualify. The refusal rate on scanned documents would be close to total,
   so the rule has to distinguish a carrier we cannot decode from a carrier that is an image
   and belongs to a different check. What remains open is where that line sits on a real
   corpus, and it needs one — the fixtures only prove the failure mode exists.
