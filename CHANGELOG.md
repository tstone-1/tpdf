# Changelog

All notable changes to tpdf are recorded here.

Versioning is CalVer `YY.M.MICRO` --- see [`BUILD.md`](BUILD.md).

Nothing has shipped yet. Phase 0 is a feasibility investigation, and what it produced is
measurements and a verdict on each load-bearing assumption, not a viewer. The entries
below are what exists in the tree, so that the first release has a history rather than a
single "initial release" line.

## [26.7.0] - Unreleased

### Phase 0 --- feasibility spikes

All seven spikes have documented verdicts and the exit criterion is met. The evidence is
in `docs/PLAN.md`; the traps each one paid for are in `AGENTS.md`.

- **Render pipeline.** Raw pixels over the Tauri custom scheme, 1024²--2048² tiles.
  PDFium charges ~1 s per *render call* on a dense A0 page regardless of how small the
  request is, so small tiles multiply a constant rather than dividing the work.
- **Process architecture.** A worker boundary costs 6 µs of control latency and 0.11 ms
  to move a 4 MB tile through shared memory, against 3.0 ms to hand the same tile to the
  webview. Four workers give 3.9× throughput on a 4P+6E machine. macOS only.
- **Startup.** Warm, cold, and first-launch-after-build are three regimes; the last two
  are the OS. The shell floor is ~250 ms before any application code runs. Warm start is
  276 ms with lazy page geometry and a non-default menu, against a 300 ms target.
- **Text-object round trip.** Both routes reproduce the page with zero collateral pixels,
  but only surgical `lopdf` operator rewriting preserves marked content, and only it
  detects an out-of-subset character instead of silently drawing `.notdef`.
- **Sanitized full rewrite.** A collected `lopdf` rewrite matches QPDF on every hostile
  fixture, so QPDF is not required --- but `lopdf`'s own collection is quadratic and the
  mark-and-sweep has to be ours.
- **Incremental save.** The update section stays under a kilobyte whatever the document
  weighs, and beats a full rewrite 8.2× on disk at 337 MB. Signatures stay
  cryptographically intact and stop being trusted, at every DocMDP level.
- **Threat model.** `docs/THREAT-MODEL.md`, with the sandbox profile it implies. The
  vendored PDFium has no V8 and no XFA compiled in, so document JavaScript cannot run
  rather than being switched off.

Known failure, recorded rather than smoothed over: the A0 vector page sustains 60 fps
over a screen that is 0--4% sharp. Frame rate cannot distinguish a viewer that keeps up
from one that has given up, so the criterion now carries a coverage floor.

Closed in Phase 1 on 2026-07-27 --- by the progressive render API and stale-request
withdrawal, and **not** by the worker pool, which was measured before being built and
takes a screenful of that page from 8.19 s to 2.55 s rather than to anything scrollable.
Read the closure narrowly: the page never falls below its tier-1 placeholder, which is
what the criterion asks, and it stays 6--10% sharp while moving, which is not a good
experience.

### Added

- **A text layer** (`src-tauri/src/text.rs`), which selection, search and the accessibility
  tree will all read --- one extraction rather than three that disagree. It carries one
  Unicode scalar per PDFium character index and no string: `FPDFText_GetText` extracts UCS-2
  and drops characters it cannot represent, so its string and the indices the boxes are keyed
  by diverge on exactly the documents nobody tests with.

  Extraction costs **1.42 ms** on a 2,725-character page with the page already loaded, and
  **43.2 ms** on the A0 sheet where almost all of it is `FPDF_LoadPage`. That sheet has zero
  extractable characters, which search will have to say out loud rather than return nothing.
- **Text selection and copy.** Drag to select, across pages; Cmd-A for the page, Escape to
  clear, Cmd-C to copy. Highlights are drawn on an overlay canvas above the tiles, so the
  class that owns the tile cache does not also have to know what a selection is. A copy waits
  for any page whose text has not arrived --- a fast drag can reach the clipboard before the
  extraction does, and silently copying the loaded part is a bug found in someone else's
  document.
- `src-tauri/src/bin/text_probe.rs` --- checks the page-space to device-space flip against
  **pixels**, per character, and carries a control that fails the run if the wrong convention
  would also pass. On the four small fixtures: 100% against 4.1--4.8%. On the dense corpus the
  wrong convention scores **69.9%**, so that page cannot tell the conventions apart and the
  probe says so instead of reporting the 100%.
- **A viewer a person can drive** (`src/lib/viewer.ts`). Open a PDF from the file dialog or
  by dropping it on the window, then scroll it with a trackpad, a wheel, the arrow and page
  keys, Home and End, or the scrollbar; zoom by the Cmd-`+`/`-` ladder, a pinch, or Cmd-0
  for fit-width, which then tracks the window. It drives the same `Scroller` the benchmark
  drives, deliberately: the class that knows what a frame costs is not also the class that
  knows where the finger went.

  **The frame loop idles.** It runs only while the scroll is moving or the scroller has work
  that has not reached the screen. A viewer that ran the benchmark's fixed loop would hold a
  core awake for as long as it was open.

  **The status line reports the degraded state** `docs/PLAN.md` §9 recorded as owed --- and
  reports the two failures separately, since "no page yet" and "a blurry page" are different
  things. Both numbers are the scroller's own coverage measurement, so a reader is told the
  same number the benchmark reports.
- **A functional check of the viewer** (`src/lib/viewercheck.ts`, `scripts/viewer_check.py`).
  Opens a document in a real webview, dispatches real wheel and key events at the viewer's
  own root, and asserts sixteen behaviours. Two of them are controls: idling is asserted in
  both directions, and every coverage recovery is preceded by an assertion that the tiles
  were actually discarded first --- without which "covers the last page" passed instantly on
  coverage the first screen had already established, while its own detail line read
  `page 1/775`. Six deliberate mutations, one at a time, and every one was noticed.
- **Cancellable rendering** (`src-tauri/src/progressive.rs`). PDFium's progressive API,
  driven on raw `FPDF_DOCUMENT` / `FPDF_PAGE` / `FPDF_BITMAP` handles, because
  `pdfium-render` keeps every handle accessor `pub(crate)` and the safe wrapper therefore
  cannot reach it. A render can now be abandoned from another thread in 0.25--24 ms where
  it previously ran to completion over 6.3 s. Uncancelled output is byte-identical to the
  existing path. Not yet wired into the viewer --- see `docs/PLAN.md` §Phase 1.
- **Stale tiles are withdrawn from the renderer** (`render.rs`, `protocol.rs`,
  `tiles.ts`, `scroller.ts`). Every tile request carries an id; `tile://localhost/cancel/<id>`
  withdraws it. One that has not started is dropped without rendering, one already running
  is abandoned through the progressive API. The viewer's render service now runs on the raw
  handles throughout, which also removes a full-tile copy --- Pdfium renders straight into
  the buffer that is handed on, where the safe path's `as_rgba_bytes()` allocated and copied
  a second 16 MB at 2048².

  Measured against the coverage floor rather than the frame rate, withdrawal being the
  variant: **inert on the text corpus** (100% sharp either way, nothing withdrawn) and on
  the A0 sheet it removes the waste without buying coverage --- five finished-then-discarded
  tiles per round become zero, and the visible area stays 6% sharp. The A0 page still fails
  the criterion; that is the worker pool, not the queue.
- **A page-handle cache on `RawDocument`.** `FPDF_LoadPage` re-parses the page on every
  call --- PDFium caches nothing --- which is 0.18 ms on the text corpus and **44.3 ms on
  the A0 sheet**. Loading per tile request, as `render.rs` still does, costs a six-tile
  screenful 266 ms of re-parsing on the document that is already too slow.
- `src-tauri/src/bin/progressive_probe.rs` --- measures the above: pixel identity against
  the safe path, poll frequency and the latency it bounds, and what a cancelled bitmap
  actually contains. Its `identity` mode fails a run in which nothing paused, so a passing
  result cannot be one that never exercised pausing.
- **The first tests a change can break** --- 26 of them, over the request-withdrawal state
  machine (`src-tauri/src/queue.rs`, extracted from `render.rs` so the orderings can be
  driven directly instead of provoked) and the `tile://` URL parser. Each was verified by
  mutating the code it covers and confirming the expected test failed; that pass found a
  guard no mutation could break, now deleted, and a test that asserted the wrong half of the
  property it was named for.
- The scroll benchmark drains a variant's outstanding requests before the next one starts,
  and reports the tiles each round withdrew beside the ones it threw away. Without the
  drain the two variants share a render queue: whichever ran first measured better, and
  swapping them swapped the result.
- `scripts/fetch_pdfium.py` --- installs the pinned PDFium build (`chromium/7881`),
  verifying its SHA256 before extracting and refusing a V8 asset. A clean clone could not
  previously build: `vendor/pdfium/` is gitignored and nothing fetched it.
- `scripts/gates.py` --- runs every quality gate and *is* the definition of them, so the
  checklist in `BUILD.md` cannot drift from what actually gates.
- `BUILD.md` and this changelog.

### Changed

- **Lazy page geometry is the default**, with `TPDF_EAGER_GEOMETRY` to restore the walk.
  Enumerating every page of a 775-page document costs 86 ms on the critical path to buy a
  scrollbar exactness the scroller estimates anyway; it is what takes warm startup from
  374 ms to inside the 300 ms target, and shipping the opposite default meant the Phase 0
  exit criterion was met by a variant nobody ran. Warm start measures **276 ms median,
  267--293**, with the dialog plugin now linked in.
- **The single-canvas `viewport` layout is what the viewer uses**, applying the verdict
  §4 reached and had not yet acted on.
- The app's window is the document, not the spike harness: the manual A/B benchmark button
  and its `src/lib/bench.ts` are gone, superseded by the automated `autobench` path that
  every published transfer measurement was actually taken with.
- `scroll_bench.py` and `viewer_check.py` share their lock-screen and display guards
  (`scripts/webview_guard.py`) rather than carrying two copies of a long message that only
  matters at the moment someone is working out why nothing happened.
- **Corrected the worker-pool scaling claim.** `AGENTS.md` and `docs/PLAN.md` §3 said to
  size the pool from the performance-core count, on the strength of 3.89x across four
  workers. That was one tile from each of many pages of the text corpus; across six tiles
  of one A0 page --- what a viewport actually asks for --- the same machine gives 2.56x on
  four, 3.22x on six and nothing at eight. `worker-bench` grew a `--grid` work list to be
  able to ask, since its old one walked pages and the A0 fixture has one.
- **The scroll benchmark reports a coverage floor**, the worst single frame of the worst
  round, beside the mean it already reported. "Never below the tier-1 placeholder" is a
  claim about a minimum, and a mean that rounds to 100% is equally consistent with a frame
  that showed nothing --- so the criterion could not be tested by the number that was being
  read for it.
- **The single-canvas scroll layout should be the default, not the fallback.** Over ~3,300
  timed frames it dropped no frames where the canvas-per-tile layout dropped three and
  stalled once, at identical coverage, and its per-frame cost is 3--4x lower.
- **Corrected a load-bearing architectural claim.** `AGENTS.md`, `docs/PLAN.md`,
  `docs/THREAT-MODEL.md`, `render.rs`, `Cargo.toml` and `worker_bench.rs` all stated that
  `pdfium-render`'s `thread_safe` feature serializes every PDFium call behind one global
  mutex, and that multiple document handles therefore render sequentially but safely. It
  does not, and they do not. There is no mutex in the crate's native path; the feature
  only makes `Pdfium` `Send + Sync`. Two threads rendering the A0 page **segfault**, while
  four threads on a simple document returned pixel-correct tiles six times out of six at a
  3.85x speedup — then crashed on the next round. `src/bin/thread_probe.rs` is the
  measurement. The conclusion (render in worker processes) is unchanged; its justification
  is now that threads are undefined behaviour rather than merely futile.

### Fixed

- **A zero-length render slice ran to completion instead of pausing immediately.** The
  pause deadline used 0 as its "no deadline" sentinel, and `Instant` on Apple Silicon ticks
  at 41.67 ns --- so arming a zero slice right after taking the origin produced a genuinely
  zero elapsed time and hit the sentinel. Intermittent, and invisible to every identity
  check, because a render that never pauses is byte-identical to one that never had to.
- The PDFium install path assumed the macOS archive layout. Windows ships the loadable
  DLL at `bin/pdfium.dll` and only an import library in `lib/`. The fetch script now knows
  both; `pdfium_library_dir()` in `src-tauri/src/lib.rs` still does not, and is recorded
  as a known Windows defect.
