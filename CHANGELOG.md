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
- **Find in document.** Cmd-F, search-as-you-type, Enter and Cmd-G to step through hits,
  Shift for backwards, Escape to drop it. Every hit on a visible page is highlighted and the
  current one differently. The scan starts at the page being read and wraps, so a reader on
  page 700 is shown the next hit rather than the first one in the document.

  Matching is in Rust over the **same character codes selection reads**, not through
  `FPDFText_FindStart` --- PDFium's search would have been shorter and answers in positions
  into its own extracted string, which is a second index space beside the one the text layer
  exists to be the only one of. A hit is therefore a range of the indices the boxes are keyed
  by, and highlighting one is the selection code with a different colour.

  Case is ignored, runs of whitespace collapse so a phrase spanning a line break still
  matches, and soft hyphens are dropped. Ligatures, accents and hyphen-broken words are
  deliberately **not** normalised: each would make the highlight cover characters the query
  did not contain.

  A whole-document scan of the 775-page corpus for a word that is not in it --- the worst
  case --- takes **843 ms**, about 1.1 ms per page, essentially all of it extraction. A
  document with no extractable text says so rather than reporting no matches.
- **A command palette on Cmd-K**, and a command registry under it. `docs/PLAN.md` §8 calls
  the palette "the thesis, not a garnish": the complaint about Acrobat is unreachable
  capability, not missing capability, so a palette only helps if every command is in it ---
  which means commands have to be data rather than branches of a key handler. That handler
  had reached fifteen branches. Fourteen commands are registered today and the next feature
  registers rather than growing the chain.

  Ranking is subsequence matching scored like a code editor's --- word starts, then
  consecutive runs, then position --- so `fw` finds "Fit width". It returns the matched
  positions and the palette bolds them, because a highlight that disagreed with the ranking
  would be worse than none. Each row shows its keybinding, so the palette teaches shortcuts
  instead of replacing them. Recents break ties and cannot beat a better match.
- **A screen-reader text layer** (`src/lib/a11y.ts`). `docs/PLAN.md` §8 states accessibility
  as an architectural constraint rather than a later pass, and this lands before thumbnails
  and an outline are built on the same scroller --- everything added first is more that would
  have to be rewritten.

  A canvas-rendered, virtualized page list has **no DOM text at all**, so a screen reader
  finds an empty scrolling region. This maintains a parallel, visually hidden DOM of the
  visible pages' text, split into lines from the same character geometry the selection uses.
  Elements are keyed by page and **never recycled**: a page that stays on screen keeps the
  same element, so a reading cursor inside it survives a scroll. The tiles and the selection
  overlay are `aria-hidden`, and the page number is announced through a polite `role=status`.

  Not verified against a screen reader, and not claimed to be: the checks assert that the
  text is present, is the page's own, and survives scrolling. Reading order also comes from
  geometry rather than from a tagged PDF's `/StructTree`, which is strictly worse for a
  document that has one.
- **The document outline, and a sidebar** (`src-tauri/src/outline.rs`, `src/lib/outline.ts`,
  `src/lib/sidebar.ts`). Cmd-`\` shows a real `role=tree` --- one tab stop with a roving
  tabindex, arrow keys to move, collapse and expand, and the entry the reader is currently
  inside highlighted as they scroll. Clicking one goes to the destination's *position* on
  the page, not merely to the page.

  This is the first feature whose input is openly hostile, and not by inference: PDFium's
  own documentation for `FPDFBookmark_GetNextSibling` says the caller must handle circular
  references. The walk carries a visited set, a depth limit and an item budget, each
  catching what the others cannot, and reports whatever any of them cut rather than showing
  a truncated table of contents as a complete one. 44 entries of a deliberately malformed
  outline --- two cycles, a 200-level chain, a 50,000-character title --- walk in **1.6 ms**;
  an ordinary one in **0.17 ms**.

  Building that fixture found a real defect: **`FPDFBookmark_GetDest` follows the bookmark's
  action without checking its type**, so a `/GoToR` meaning "open other.pdf at page 1" comes
  back as an ordinary destination and resolves against the open document. Reading the action
  first removes the fallback's opportunity to fire. `/Launch`, `/URI`, `/GoToR` and
  `/EmbeddedGoTo` entries are shown, marked and explained rather than dropped or silently
  inert.

  17 mutations, all caught --- one only after the test it aimed at was rewritten, having
  been unable to fail. Nine viewer checks cover the sidebar itself, three of which went red
  before the fixes they prompted: a roving tabindex that did not follow real focus, an
  arrival highlighting the entry *before* the one clicked, and a fixture whose lines were
  all identical.
- **Page thumbnails in the sidebar** (`src/lib/thumbnails.ts`), as its second tab. The first
  feature that competes with the reader for the renderer: a 150 px thumbnail of the A0 sheet
  costs **1.52 s**, PDFium charges that per render *call*, and the render service is one FIFO
  thread. So the strip keeps at most one request outstanding and **withdraws it whenever the
  viewer has work** --- through the same progressive-API cancellation a stale tile uses, which
  returns in 0.25--24 ms. The viewer waits tens of milliseconds for a thumbnail instead of a
  second and a half, and the withdrawn page is asked for again once things settle.

  A hidden strip renders nothing at all. Rows exist only for the visible window plus an
  overscan, so `aria-setsize` and `aria-posinset` are load-bearing rather than decorative.
  Tier 1 is *read* --- the placeholder and the thumbnail are the same bitmap, so the page
  being read appears instantly --- and deliberately not written, since tier 1 is permanent
  and one entry per page is 98 MB on the 775-page corpus.

  Twelve mutations, all caught by the check each was aimed at --- after two of the new
  checks turned out to be wrong in ways only mutation could show.
  One could be **switched off by the defect it was aimed at**: it skipped when every row was
  built, so deleting windowing made it report itself inapplicable rather than fail. The other
  was bounded the wrong way round --- "some thumbnail was borrowed" passes *harder* when a
  missing in-flight guard borrows the same page on every scroll, which is what it was doing.
  A new twelve-page fixture, `vector-multi.pdf`, is the only document where a thumbnail is
  slow enough to collide with the viewer at all; elsewhere those checks skip and say so.
- **Front-end unit tests, and a seventh quality gate.** `vitest`, over command ranking and
  line splitting --- the first front-end logic with an answer that can be *wrong* rather than
  merely ugly.
  The plan had said `npm run test` would land when there was something for it to check.
  Twenty-two mutations against the new code, all caught; one branch was deleted rather than
  tested, because nothing could make it fail.
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
  own root, and asserts twenty behaviours. Three of them are controls: idling is asserted in
  both directions, every coverage recovery is preceded by an assertion that the tiles were
  actually discarded first, and a zero-length drag must select nothing.

  Without the second of those, "covers the last page" passed instantly on coverage the first
  screen had already established, while its own detail line read `page 1/775`. Eleven
  deliberate mutations across two passes, one at a time. Nine were caught; one was an
  identity that tested nothing; one found a guard --- `Selection.isEmpty` --- that no mutation
  could break, now deleted.

  The selection assertion is the second attempt at one. The first checked that the dragged
  text was a **substring** of the page's text, which cannot fail: a selection is a contiguous
  range of character indices, so its string is a substring however wrong the boxes are.
  Inverting the y-flip in `text.rs` passed all twenty checks and returned real words from the
  wrong part of the page. What discriminates is ordering --- text dragged near the top of the
  page must come from earlier in the page's text than text dragged further down.

  It does not take focus. The window has to stay visible, because WebKit suspends an occluded
  page, but raising it over whatever someone is doing every time a check runs is its own bug.
  `scroll_bench.py` still focuses on purpose: an unfocused window is throttled, and a
  frame-rate benchmark would be measuring the throttle.
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
- **Printing** (`⌘P`, macOS). tpdf hands the operating system a **PDF, never pixels** ---
  measured: `cupsfilter -d <queue>` against a PDF-native printer returns the input file byte
  for byte, so rasterising first could only throw information away. What is ours is deciding
  *which* PDF: everything unrotated is handed over untouched, a page range deletes pages in
  place so nothing loses an inherited `/Resources`, and the reader's view rotation composes
  onto each page's effective `/Rotate`. PDFKit paginates and runs the panel; the panel's own
  page-range field is why there is no range UI here.

  Every job is re-read with PDFKit --- a parser that did not write it --- before the panel
  opens. That is not ceremony: a page table left contradicting its own `Kids` array passes
  every `lopdf` check and makes PDFKit report five pages for a two-page document, the extra
  three being blank sheets it manufactures to satisfy the count.

  Page deletion is ours rather than `lopdf::delete_pages`, which runs a quadratic graph walk
  once per deleted page: keeping two pages of a 775-page document costs 620 ms there and
  1.2 ms here, for byte-identical output.

  **Windows is not implemented**, and says so with an error rather than doing nothing.
- **Every document is parsed in a sandboxed worker process** (`src-tauri/src/render.rs`).
  The one Phase 0 constraint that had never reached the running program: the boundary
  existed and was measured, but the viewer still opened documents in the app process.
  `RenderService` now runs on either backend, defaulting to workers on macOS, with
  `TPDF_BACKEND=in-process` selecting the control --- and refusing any other value, because a
  typo that silently ran the other implementation would make every comparison between them
  meaningless.

  What says it really moved is not a comment: `backend-probe` reads the **dynamic linker's**
  image table and finds no `libpdfium` mapped in a process that has just opened a 775-page
  document and rendered a tile from it. A startup mark of our own would only report what our
  code believes it did.

  The boundary is transparent on six corpora --- tiles byte for byte, and the same page
  geometry, character boxes, search ranges and outlines. It costs **11--16 ms at startup**
  of a ~50 ms application budget: 3.1 ms to spawn and 8.9 ms for the child to bind PDFium,
  sandbox itself and parse. Warm start is 287--295 ms against a 300 ms target, so the margin
  lazy page geometry bought has largely been spent.

  A withdrawal now has two halves that do different jobs --- the parent's queue decides what
  the caller sees, the wire withdrawal decides whether the worker keeps burning CPU --- and
  the first check for it could not have failed, since `Abandoned` is what the parent
  produces on its own. It now asserts the latency too: 2.2 ms against a 1,125 ms render.

  **Windows still refuses rather than running unsandboxed**, and so defaults to in-process.
- **Tiles of one page render in several worker processes at once.** The worker backend is
  served by a pool of threads over one job queue, and each document has a pool of processes
  they draw from. A screenful of the A0 sheet goes from **3.46 s to 0.83 s, 4.2x**, measured
  through the service itself with interleaved rounds; a cheap page gains 2.7x. Six workers is
  where the curve flattens --- neither the core count nor the performance-core count.

  **Growth is lazy**: a document opens with one worker and gains another only under
  contention, so a reader turning one page at a time never pays for a second parse of it.
  Nothing retires an idle worker afterwards, so a fully grown pool on the A0 sheet keeps
  about 290 MB --- recorded as a residual rather than hidden.

  The in-process backend is deliberately *not* pooled: concurrent PDFium in one process is
  undefined behaviour whatever the handles are.

  Two of five mutations first survived, and both pointed at the design rather than the tests.
  With one thread per worker the pool's own capacity bound was unreachable --- the thread
  count was enforcing it --- which also meant six tiles of a slow document could occupy every
  thread and starve a second document whose workers were idle. Threads are now `pool + 2`.
- **A document is released when the reader moves to another file.** Until now nothing ever
  removed one, so a session that opened a dozen files held a dozen documents --- which the
  process boundary turned from a heap allocation into a dozen sandboxed children at
  7.8--48.2 MB each.

  A released id leaves a **hole** rather than being removed, and is never handed out again.
  The `Vec` index is the id, so removing the entry renumbers every document after it and a
  request naming the closed one is answered in full from a file the caller never asked about
  --- demonstrated by mutation, which returned a perfectly good tile of the wrong document.
  Whether a request might still be in flight needs no answer at all: the render thread is
  FIFO, so a close lands behind everything already queued.
- **A worker that dies is replaced, and the request retried once.** Isolation that ends the
  reading session is isolation nobody wants: a crash caused by anything other than the
  request in hand is now invisible to the reader. The replacement is handed the **same
  document mapping**, not the same path, so a file rewritten in between cannot silently
  become what is on screen --- and a 337 MB scan is not read twice. A live worker that
  answers with an error is *not* replaced; only one the kernel says has exited.

  The bound on a crash loop is the single retry rather than a restart budget. A page that
  faults deterministically then costs one process per attempt, which is bounded by the
  reader's own requests --- a counter on top would be defence nothing could reach, and
  `AGENTS.md` says to delete those rather than keep them.

  `backend-probe` kills the worker out of the OS process table and asserts the same pixels
  come back from a *different* pid. Six mutations, five caught; the survivor is recorded
  with its reason. Two of the findings were about the checks: `SIGSEGV` does not kill a Rust
  process the first time it is sent, and a check nested inside a lookup for the thing under
  test disappears rather than failing when the defect removes it.
- **A parent that does not trust its worker's arithmetic.** A reply states how many bytes of
  the shared mapping it wrote; that claim is checked against the mapping's size and, for raw
  pixels, against `width x height x 4` exactly. Reply lines are bounded at 32 MB, because
  `read_line` on a pipe is otherwise unbounded and a worker made to emit an endless one would
  take the app down with it --- perfect isolation, dead application.
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

- **Rotating the view**, clockwise on Cmd-R and anticlockwise on Cmd-L, both also in the
  palette. Preview's bindings rather than Acrobat's, whose Shift-Cmd-`+`/`−` produce the same
  `key` as the zoom shortcuts on this keyboard. It turns the *view* and never the document:
  rotating pages in the file is a page operation and belongs with the ones that write.

  PDFium's render call takes a rotation and composes it with the page's own `/Rotate`, so the
  renderer's half is one argument threaded from the tile URL down --- plus the dimension swap
  it needs, since PDFium fits the page into the rect it is given and passing the upright size
  squeezes a landscape page rather than turning it. Character boxes cannot go the same way,
  being a property of the document, so they are turned in our own code where the cache hands
  them out. The two implementations are tied by a rule asserted over all sixteen
  combinations: turning a device box after `to_device` must equal `to_device` of the summed
  turn --- which is how the frontend's turn inherits the verification `text-probe --mode
  align` did against pixels.

  **Both cache tiers go.** A zoom step keeps the tier-1 placeholder because it is only
  stretched; a rotated one is a different picture, and keeping it would leave the page
  sideways under its own sharp tiles. So a rotation on the A0 sheet goes grey for the ~1.5 s
  that placeholder costs to produce again.

  **An outline destination is not placed while the view is rotated.** It carries an offset
  down an upright page; at a quarter turn that axis is the screen's horizontal one, and at a
  half turn it counts upwards while the reader scrolls down. Navigation and the outline
  highlight fall back to page granularity --- which is what `/Fit` means, and what
  `outline.rs` already returns for a destination it cannot place.

  Fourteen mutations, all caught by the check aimed at them. Three of the six new checks
  exist because a mutation survived first: "the same lines come back out of a rotated page"
  derived its drag positions from the very boxes it was testing and so passed with the text
  layer never told about the rotation; nothing in the harness looked at a pixel, so dropping
  the rotation from the tile URL passed everything; and the viewer and the scroller each keep
  a rotation, so a scroller laying every page out upright survived a check that only measured
  the zoom.

- **Session restore.** tpdf reopens the document you were reading, on the page you were on,
  at the zoom and rotation you had, with the sidebar as you left it --- and opening a document
  you have read before puts you back where you were in it. One place per document, 32 of
  them, kept in `session.json` in the app config directory and written through a temp file
  and a rename so a crash mid-write leaves the previous session rather than a truncated file.

  **A malformed session file is an empty session, never an error**, and a field out of range
  is repaired rather than refused --- the opposite of what the tile protocol does with a bad
  parameter, because a file that has sat on disk across upgrades is not a live instruction,
  and rejecting it would discard every other document's place over one bad number. A
  remembered page is clamped to what the document has *now*: a path is not an identity, and
  the file may have been rebuilt shorter since.

  Positions are written at most once a second, and chained through one promise --- not for
  throughput, but because `invoke` resolves out of order under load and two writes a second
  apart can otherwise land in the other order, the older place overwriting the newer.

  Checked by `scripts/session_check.py` across **four launches of the real app**, because
  restoring is part of the boot and a harness that replaced the application --- which is what
  every other one here does --- would be checking a second implementation. Two of the four
  assert nothing about restoring: that the app does *not* open in the remembered state by
  itself, and that nothing opens when nothing is remembered. Without the first, "restored to
  page 7" is satisfied by an app that happens to open there.

- **Dark mode.** Two things wear that name and only one was missing. The chrome already
  followed the system, being built on `Canvas` / `CanvasText`; the scrollbar and the surround
  around the page had escaped that and now follow it too. The surround needs two literals
  rather than a formula, since it has to be darker than the paper in *both* themes.

  The page gets an explicit command instead --- **Invert page colours**, ⌘⇧I. Named that and
  not "Dark mode" because the chrome is already dark when the desktop is, so a command called
  dark mode would appear to do nothing for the reader who most expects it to. Inverting a
  document changes what it looks like, and a reader who darkened their desktop has not asked
  for that, so it is never inferred from the system theme.

  It inverts HSL **lightness**, holding hue and saturation, so blue headings stay blue where a
  plain `255 - c` would turn them yellow. That has a closed form --- chroma is unchanged by the
  inversion, so every channel moves by the same `255 - max - min` --- which needs no float, can
  never clamp, and is an exact involution. Applied in the renderer rather than as a CSS filter,
  because a filter is applied by the compositor and its pixels cannot be read back: a check
  could then only assert that a style was set.

  Photographs come out as negatives with the right hues, as they do in every reader that
  offers this. That is why the mode is off by default and asked for explicitly.

- **File associations.** Double-clicking a PDF, "Open With", dragging one onto the icon, and
  `tpdf file.pdf` from a terminal all open it. Declared as `role: Viewer` rather than Tauri's
  default `Editor`, deliberately: `Editor` tells Launch Services tpdf can edit a PDF, and it
  cannot yet. Rank stays `Default` --- not `Owner`, since tpdf does not create PDFs, and not
  `Alternate`, which ranks it below every other viewer.

  A macOS double-click puts nothing in `argv` --- it is an Apple Event, and it can arrive
  before the webview exists --- so paths are queued until the frontend is listening and
  emitted directly after, with the drain and the flag flip under one lock. The event's name
  is fetched from Rust rather than duplicated as a constant on both sides, because a constant
  that drifts fails by silence: the app keeps working and merely stops noticing documents
  opened while it is already running.

  A handed-over document beats a remembered one, since someone who double-clicked a file is
  asking for that file. Checked by `scripts/open_check.py` across five launches and 11
  checks, two of them controls --- and the checks themselves by eleven mutations, all of
  which behaved as predicted, including one predicted to survive.

### Fixed

- **A viewer check vanished instead of skipping, on every one-page document.**
  `searchesFromHere` records two check names, and its two early returns skipped only the
  first --- so on a document with one page, `"finds something from the end of the document"`
  did not pass, did not fail, and did not appear at all. It had been that way since the check
  was written, through every green run and every mutation pass.

  Nothing red found it. It surfaced as an inconsistency *between corpora*: 86 check names on
  five of them and 85 on `text-cid`. That invariant --- the set of names is fixed, and a count
  that moves is itself a defect --- was written down when a check disappeared inside an
  `if let`; this is the first time it has caught one. A static scan for names that are
  recorded but never skipped is not a substitute: it returns 48 candidates, nearly all false,
  because a skip can be reached through a `const` or a call it cannot see. Diffing the name
  sets across corpora costs one `diff` and names the missing check exactly.

- **The test guarding `path_from_url`'s scheme check could not fail.** The behaviour was
  always correct; the test was not. `Url::to_file_path` rejects `https://example.com/a.pdf` on
  its own, because the host is a domain --- so deleting our scheme check broke nothing, which
  by the standing rule marks it a guard to delete. It is not: a `localhost` host is treated as
  *no host at all* whatever the scheme, so `https://localhost/a.pdf` resolves to `/a.pdf`. A
  second case covers that direction and goes red alone when the guard is removed.

- **A macOS double-click crashed the app before it could open anything.** `RunEvent::Opened`
  fires *before* Tauri's setup hook, so state registered there is not yet managed and
  `state::<Launch>()` panicked --- on precisely the path it existed to serve. No error, no
  output, an empty window, and `EXC_CRASH SIGABRT` in the crash reports. Registered on the
  builder now, before the event loop exists, and read with `try_state` so the same mistake
  would cost one document rather than the launch.

- **Every automated check reported success through its exit code, whatever it printed.**
  `AppHandle::exit(code)` ends Tauri's event loop; `App::run` then returns normally, `run()`
  returns, `main` returns unit, and the process exits **0** regardless. So
  `scripts/viewer_check.py`'s closing `return completed.returncode` could not fail, and had
  not been able to since it was written. The mutation harnesses read `[FAIL]` lines out of
  the transcript rather than `$?`, which is why their results were nonetheless correct --- the
  exit code was the one consumer with nothing to cross-check it.

  `spike_exit` now flushes stdout and calls `std::process::exit(code)`. Verified in **both**
  directions: a failing run exits 1 and a passing one exits 0, because a fix tested only
  against failure would be satisfied by exiting 1 unconditionally.

- **Character boxes and outline destinations on a page carrying `/Rotate`.** PDFium reports
  the page size *after* rotation and renders to match, but reports character boxes and
  destination coordinates in the page's own *unrotated* space --- so the flip against the
  reported height was correct at `/Rotate 0` and wrong at every other value. Measured with
  `text-probe --mode align` on a new fixture: 100% of character boxes landed on ink at 0 and
  **0.0% at 90, 180 and 270**. Every selection, every search highlight and the whole
  screen-reader reading order was elsewhere on any scanned page, in tidy rectangles.

  The turn is one function with two callers, and the probe now reports what each *wrong*
  rotation scores rather than only what the flip does --- on a rotated page those are
  different questions.

  Fixing it exposed a second defect that the first did not imply: characters are grouped into
  lines by vertical overlap, which on a page whose text runs down the screen puts each one on
  its own line, so the screen reader read the page **letter by letter**. Every text assertion
  still passed; what caught it was a comparison against an independent extraction, 877
  characters against 534.

  Reading the rotation needs a loaded page, which took the outline walk from **0.17 ms to
  7.5 ms** on a twelve-page fixture --- about 1 ms per distinct page named. The outline is
  therefore now requested after the first screen is painted rather than at open, since the
  render thread is FIFO. Thirteen mutations, all caught.


- **The viewer check printed nothing unless it reached the end.** Every result was buffered
  and emitted in one block, so a run that stopped midway was indistinguishable from one that
  never started --- which is exactly what happened when an occluded window suspended the
  page, and is why that took an afternoon to identify rather than a minute. Results now
  print as they are recorded, chained through one promise so the transcript cannot arrive
  out of order.
- **The watchdog says when a page was never executed at all.** Every spike entry point
  begins by asking Rust for its path, which records a `webview alive` mark; a timeout
  without one now prints that the page never ran a line of JavaScript, and why, instead of a
  mark list that has to be interpreted. It fires on a raw `cargo build` binary --- which
  runs no webview content at all, WKWebView needing the bundle identity --- and stays quiet
  on a bundled one.
- **`TPDF_RAISE=1`** raises the window for a check that has nowhere visible to put one.
  WebKit suspends a page whose window is fully covered, and an unlocked screen is not a
  visible window. Opt-in: raising a window over someone's work on every run is its own bug.
- **The sidebar's roving tabindex now follows focus that arrives from outside it** --- a Tab
  into the tree or a programmatic focus previously left every arrow key aimed elsewhere.
- **An outline destination no longer highlights the entry before the one clicked.** The air
  left above a heading on arrival is measured in points rather than CSS pixels, with a
  matching tolerance in the highlight.
- **A zero-length render slice ran to completion instead of pausing immediately.** The
  pause deadline used 0 as its "no deadline" sentinel, and `Instant` on Apple Silicon ticks
  at 41.67 ns --- so arming a zero slice right after taking the origin produced a genuinely
  zero elapsed time and hit the sentinel. Intermittent, and invisible to every identity
  check, because a render that never pauses is byte-identical to one that never had to.
- The PDFium install path assumed the macOS archive layout. Windows ships the loadable
  DLL at `bin/pdfium.dll` and only an import library in `lib/`. The fetch script now knows
  both; `pdfium_library_dir()` in `src-tauri/src/lib.rs` still does not, and is recorded
  as a known Windows defect.
