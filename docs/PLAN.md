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
termination and restart when it is exceeded. Process isolation makes that cheap and safe —
measured at 1.2 ms to kill and reap and 4.8 ms to respawn (§3). Note the budget has to be
enforced by the parent's deadline, not by `RLIMIT_CPU`, which is a process-lifetime budget
rather than a per-request one.

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
would move this number is the worker pool (§3: 3.89× on four), and even four workers leave
~25 tile-seconds to fill one screen. The real answers for such a page are the progressive
API and an honest degraded state, not a faster tile.

**The two layouts are both far inside budget, and the surprise is which is cheaper.** One
viewport-sized canvas redrawn every frame costs **0.11–0.17 ms** of main thread; a canvas
per tile in a natively scrolling container costs **0.49–0.64 ms**. Assigning `scrollTop`
against a container of absolutely positioned canvases is a forced layout, and it is more
expensive than compositing half a dozen GPU-resident ImageBitmaps into one canvas. Neither
is close to mattering, so the choice can be made on other grounds — but the escalation path
at the end of this section is not needed. The webview is not the bottleneck; PDFium is.

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

The text corpus meets that at 100% sharp. **The A0 vector page does not, and is recorded as
a known failure** rather than smoothed over: it needs the worker pool, the progressive API,
and a degraded state the UI states honestly. Phase 1 does not get to call the viewer done
while a page in the corpus scrolls blank.

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

- **The A0 vector page scrolls blank.** Owned by Phase 1, per the criterion above.
- **Windows is entirely unverified** --- no build, no gate run, no measurement --- and the
  tree does not currently compile there. `sanitize_rewrite.rs` and `tile_bench.rs` call
  `libc::getrusage` with no `cfg` gate; PDFium's loadable library is at `bin/pdfium.dll`
  rather than `lib/libpdfium.dylib`, which `pdfium_library_dir()` does not know; and the
  worker sandbox is `sandbox_init` SBPL, so the containment argument in
  `docs/THREAT-MODEL.md` is macOS-specific and needs its own answer there. `BUILD.md`
  keeps the list.
- **There are no tests.** `cargo test --locked` gates the lockfile and compiles the test
  targets; it runs nothing. The spike harnesses assert their own results, which is why
  Phase 0 conclusions are trustworthy, but none of that is a regression suite. Phase 1
  starts the first one that a change can break.

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

Three things are known and not yet done, and the next increment is the first of them:

- **Nothing supersedes anything yet.** The mechanism exists; `render.rs` still runs one
  uninterrupted render per request off a queue. Wiring cancellation in needs a generation
  from the frontend saying which tiles the viewport still wants, and it has to be measured
  against §8's coverage floor rather than against frame rate — the failure being fixed is
  one that already posts a perfect 60 fps.
- **A cancelled tile is a real partial composite**, not an untouched buffer, but whether it
  is worth showing is unmeasured: the A0 fixture saturates every similarity metric tried
  (see `AGENTS.md`). That needs a realistic drawing, not a stress fixture.
- **Form-field appearances are not drawn.** The safe path follows its render with
  `FPDF_FFLDraw`; the progressive path does not, so documents with interactive widgets will
  differ until that pass exists.

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
