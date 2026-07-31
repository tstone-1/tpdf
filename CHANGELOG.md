# Changelog

All notable changes to tpdf are recorded here.

Versioning is CalVer `YY.M.MICRO` --- see [`BUILD.md`](BUILD.md).

Nothing has shipped yet. Phase 0 is a feasibility investigation, and what it produced is
measurements and a verdict on each load-bearing assumption, not a viewer. The entries
below are what exists in the tree, so that the first release has a history rather than a
single "initial release" line.

## [26.7.0] - Unreleased

### Measurement --- the macOS half of the boundary cost, and a cross-check that disagreed

- **`latency-bench` runs on macOS**, closing the "compiles but has never executed there"
  qualifier it shipped with. Expected shape reproduced exactly on all three fixtures --- 3/3,
  3/3, and 3/4 with the documented `[SKIP]` on `vector-heavy` --- exit 0 throughout, and its
  four mutations re-proved here rather than taken on trust (4/4 caught, control green first,
  file restored by bytes and verified by digest against `HEAD`). The production worker's
  per-tile boundary cost is **0.071--0.103 ms** on macOS against 0.269--0.309 ms on Windows,
  ~3.5x rather than the 1.5--1.8x the other render constants differ by.
- **No sandbox font substitution.** The Mac run existed partly to check this, since a
  sandboxed PDFium has previously substituted fonts silently while still returning `ok`.
  In-process and worker renders agree to within 0.25% on all three fixtures.
- **The cross-check against `worker-bench --mode latency` disagreed by an order of
  magnitude, and the older harness was wrong.** Its `transport` is a residual and it
  baselines on `ping`, a variant that never renders, so the render-noise floor is left in
  the answer. On `text-base14` the subtraction error is 0.014 ms against a reported 0.015 ms;
  on `vector-heavy` it is 46.7 ms against a reported 46.6 ms, and the correctly baselined
  figure goes **negative**. `worker-bench` now prints its in-process residual and the
  `inproc`-baselined figure beside the two `ping`-baselined ones, and warns when the error is
  as large as the answer --- which is every fixture measured so far. Proved able to stay
  silent, because a warning that cannot not-fire is a constant.
- **The affected number was already hedged where it was written, and the hedge had not
  travelled.** `docs/PLAN.md` §3 says the shared-memory figure "is indistinguishable from the
  in-process residual"; the same 0.11 ms is quoted flat in the Phase 0 verdict table, in
  question 10's answer, and in `docs/THREAT-MODEL.md`. All three now carry it. No conclusion
  moves --- the boundary is cheap on every version of the number, and the production figure is
  still ~30x under the 3.0 ms webview hand-off.
- New trap, index now 168: *"A baseline that skips the expensive step leaves its noise in
  the answer"*. `AGENTS.md`'s own count of its own index was six behind; the sentence now
  names `grep -c '^### ' docs/TRAPS.md` as the authority instead of asserting a number.

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

- **Interactive form-field values are drawn by the cancellable renderer.** The raw PDFium
  path now retains a pinned form environment for the document, notifies it as cached pages
  open and close, and overlays `FPDF_FFLDraw` only after a complete base render. A cancelled
  tile remains an explicitly incomplete tile rather than receiving a complete widget over
  partial page pixels.

  A generated text widget with a value and no stored appearance stream makes the pass
  observable: before the fix the safe and progressive paths differed in 4,587 bytes; after
  it they are byte-identical both uninterrupted and through a forced pause/resume. The probe
  that proves it also now finds PDFium through the shared platform path --- on Windows it had
  still looked in macOS's `lib/` directory and told the reader to reinstall a valid DLL.

- **Text comes off a multi-column page in the order it is read.** A PDF carries no reading
  order --- only glyphs at positions, in whatever sequence its producer emitted them --- so a
  two-column page whose producer wrote line by line across the gutter copied as `alpha one
  beta one alpha two beta two`. `src/lib/reading.ts` recovers the order from the geometry by
  recursive XY-cut, which handles a heading spanning both columns as the same operation on
  the other axis. Wired into copy and into the screen-reader tree.

  Rotation is carried inside it: every rule is written over "along a line" and "across the
  lines", with the direction each runs derived from the backend's own coordinate mapping.
  Without the directions the order is right at 0° and 90° and exactly reversed at 180° and
  270°. On `rotated-90.pdf`, where PDFium extracts the lines backwards, 493 of 534
  characters now come out in a different --- and correct --- position; on a single-column
  document, none do.

  A drag still selects a contiguous range of character *indices*, so on such a page it takes
  in more than was dragged over. Making the drag geometric means carets carrying a reading
  position, which is a change to the selection model and is deliberately not in this.

  20 unit tests, 12 mutations, and two functional checks against a manifest written by the
  fixture's generator rather than by anything under test --- plus a differential one that
  needs no manifest, two pages laid out alike and emitted oppositely having to read alike.
  `viewer_check.py` is at **109 names** across seven corpora.

  Two existing checks rested on the assumption this removes, and were corrected rather than
  quietly relaxed; a third turned out to be decoration, and a precondition was wrong twice
  before it was right. All in `docs/TRAPS.md`.

- **Fit page, actual size, and a zoom you can type.** `⌘9` fits the whole page in the
  window, `⌘1` is 100%, and `⌥⌘Z` asks for a percentage through the same palette argument
  the page jump uses --- the zoom ladder is deliberately coarse, so 175% was previously
  unreachable. The toolbar's zoom readout is now the button that opens it, and its tooltip
  says what the zoom is following.

  Under it, the fit stopped being a boolean. Both fits have to survive a resize *and* a
  rotation, so the viewer remembers *which* one to re-apply rather than merely that it is
  fitting something; the old `fitting` flag is gone rather than kept beside the mode,
  including out of the session file, because two records of one fact drift. The arithmetic
  moved to `src/lib/zoom.ts`, which needs no DOM: 18 unit tests and 12 mutations, each
  caught by the test named for it. Six functional checks take `viewer_check.py` to **107
  names**, identical across all six corpora.

  One of those six could not fail, and the mutation is what said so --- it measured the page
  against the element's own width, which is 12 px wider than the width a page is fitted into,
  the scrollbar sitting in a gutter over that edge. The run still went red, through an
  *older* check, which is exactly why a count is not evidence for a new one. In
  `docs/TRAPS.md`.

- **A Windows distributable builds** --- an MSI and an NSIS installer from `npm run tauri build`.
  It did not before, and the cause is a rule about this repository's layout rather than a Tauri
  bug: **`src/bin/` must contain only declared bin sources.** The bundler enumerates that
  directory and registers the first entry no `[[bin]]` `path =` claims; a `.rs` file is always
  claimed, a *subdirectory* never is. So `src/bin/backend_probe/`, which held only `imp.rs`,
  became a phantom binary named `backend_probe` --- pointing at an executable that does not exist
  and colliding with the component id WiX derives from the real `backend-probe.exe`. Those two
  `imp.rs` bodies now live in `src/probes/`, reached by `#[path]`, which leaves module parentage
  and every `super::` in them unchanged.

  It had never been caught because Windows packaging had never been attempted. The installer does
  ship all 17 probe and benchmark executables, about 35 MB of spikes; that follows from declaring
  them `[[bin]]` in the bundled crate, is identical on macOS, and wants its own change.

- **A second launch on Windows hands its document to the running app**, as it does on macOS.
  It used to be a second process with its own window and its own worker pool ---
  `RunEvent::Opened` is macOS-only and nothing filled the gap. `tauri-plugin-single-instance`
  (Apache-2.0 OR MIT, no new crate refused by the licensing rule) now forwards the second
  process's argv to the first and exits it; the callback feeds the same `Launch` queue and emits
  the same `OPEN_EVENT` as every other route in, so there is one path for "open this document"
  rather than two that can drift. It also unminimises and focuses the window, because a handover
  that loaded the document behind whatever the reader was looking at would read as nothing having
  happened.

  `open_check.py` now runs **five of six** phases on Windows, up from four. Proved by mutation:
  disabling the plugin turns the phase red with *"nothing ever arrived"* while its control still
  passes. The remaining skip is the cold double-click, which is not a gap --- Explorer hands the
  path over in `argv`, already covered by the `argv` phase.

- **Printing works on Windows**, which was the last user-facing capability the platform lacked
  --- `present_job` returned `Err("printing is implemented on macOS only")`. `print_win.rs` reads
  the job back with `Windows.Data.Pdf`, the operating system's own PDF stack and the direct
  counterpart of the PDFKit readback on macOS: independent of the `lopdf` that wrote the job and
  the PDFium that drew what the reader saw, so it can attest that the output is readable by
  something else. It then rasterises each page onto a printer device context, because Windows has
  no in-box PDF print API at any layer and every Windows PDF viewer does the same. `PrintDlgW`
  runs the panel, on its own thread so the modal loop cannot freeze the window behind it.

  Windows output is therefore **raster at 300 dpi** where macOS is vector. Pages are requested
  from WinRT as BMP rather than the default PNG, which is what lets `StretchDIBits` take the
  bytes directly and keeps an image decoder out of the tree entirely.

  Adds the `windows` crate, and no crate to the dependency graph: it is already there
  transitively through Tauri's WebView2 stack, and it is `MIT OR Apache-2.0`.

- **`print-probe` drives the whole print path to a real spooler, without paper.** "Microsoft
  Print to PDF" is a real driver, and naming an output file in `DOCINFOW.lpszOutput` stops it
  raising a save dialog --- so everything except the panel runs unattended and the result is
  re-read by the OS parser. 8/8 checks. It asserts **ink** rather than a page count, because a
  wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad blit rectangle all produce the
  right number of blank sheets; mutating the blit away leaves the page count green and only the
  ink red. It also reads its own module table: 80 modules mapped, none named pdfium, with
  `Windows.Data.Pdf.dll` named beside it as what *is* mapped --- printing parses in the app
  process on both platforms, and what the boundary buys is that the parser doing it is not ours.

  It found a defect in the code it was written to check, which is the point of writing it:
  **every page was printed at half physical size.** A DIB rendered at 300 dpi was placed onto a
  600 dpi printer DC unit-for-unit, and for a page small enough that the fit-scale never engages
  there was nothing to correct it --- a wide even margin that looks deliberate. The probe's
  original oracle, printed ink over sent ink with an order-of-magnitude band, read `0.49` and
  passed; the same formula then failed at `0.01` on an A0 page purely because the paper is 16×
  smaller in area. What holds for both is predicting where the ink should land, from the source
  page's extent scaled by the page-to-sheet ratio: 1% error on the reference run, 48% against the
  reverted bug.

- **Large-format pages no longer allocate half a gigabyte per sheet.** `PRINT_DPI` was applied
  relative to the *page*, so an A0 page rasterised to 9933x14043 --- 532 MB as BGRA --- for a
  sheet that can show 9 MB of it, and `print-probe` on twelve A0 pages did not finish in two
  minutes. Pages now render at the resolution that yields 300 dpi *after* the fit to paper. The
  constant's own doc comment had done the arithmetic for A4, which is the page size that makes it
  look reasonable.

  What remains, measured and not a defect: one A0 page of 200,000 vector operations takes
  **2m51s**, nearly all of it inside the OS rasteriser and largely independent of resolution. A
  raster print path inherits that, and macOS avoids it entirely by handing vectors to
  `NSPrintOperation`. Avoiding it here needs `IPrintDocumentPackageTarget`, which GDI cannot
  express; not started.

- **Three of `print.rs`'s four third-parser checks now run on Windows**, taking `cargo test
  --lib print::` from 14 checks there to 18. They were `#[cfg(target_os = "macos")]` because
  PDFKit used to be the only independent parser available, which said nothing about the property
  under test --- so printing, the one subsystem whose output leaves the process, had no
  independent readback check on Windows at all. Shown to buy real coverage rather than merely
  existing: breaking `effective_rotation` turns both rotation checks red here, including
  `rotated.pdf`'s which-pages-survived case. The fourth needs per-page text, which
  `Windows.Data.Pdf` has none of, so it pins the page count and prints a `[SKIP]` naming the gap.

- **The job object's own two limits are measured**, having been claimed by `win-sandbox-probe`'s
  table since it was written and probed by nothing. Its three authority probes are all
  integrity-level properties, so every rung reported on `lowil` and above while
  `JOB_OBJECT_LIMIT_PROCESS_MEMORY` and `ActiveProcessLimit` went unexercised. With the
  uncontained rung as the control: `bare` commits 1 GB and starts a second process; every rung
  with a job is refused with `1455` (commit charge) and `1816` (process quota). Windows charges
  *committed* memory, so a bomb is refused before a byte of it exists --- a step earlier than the
  resident-memory polling macOS is limited to.

- **A Windows worker renders, contained.** `Worker::spawn` builds one off macOS for the first
  time: created suspended, dropped to low integrity, assigned to its job object before it
  executes an instruction, then handed two pipes and the document and tile sections as
  inherited handles named in argv. `worker-probe` is the proof --- **11/11 checks** on
  `text-base14`, `text-cid`, `vector-heavy` and `rotated`, tiles **pixel-identical** to the
  in-process render, plus text extraction, outlines and search across the boundary. The font
  substitution the macOS sandbox caused did not recur, as `win-sandbox-probe` predicted.

  `Worker` carries both platforms as per-platform **type aliases**, not an enum: a `Contained`
  where macOS has a `Child`, a `File` where it has `ChildStdin`/`ChildStdout`. Two methods have
  two bodies (`pid`, `epitaph`) and the rest are unchanged, so every macOS line in `worker.rs`
  is byte-identical --- which matters because none of this can be re-verified on macOS from a
  Windows machine, and a diff touching only Windows code is the strongest available statement
  about what cannot have regressed.

  Three findings came out of testing it rather than writing it. The parent must close its copy
  of the reply pipe's write end or a dead worker is indistinguishable from a slow one --- and
  the check for that has to bound its own wait, because the failure it looks for *is* a hang.
  An epitaph asked the instant a pipe reaches EOF says **"still running"**, since handles close
  before the process object is signalled; `Contained::epitaph` now waits 100 ms, and liveness
  polling still does not. And `TerminateJobObject` exited with `1`, indistinguishable from a
  worker failing on its own, where unix has "killed by signal 11" to say otherwise --- so a kill
  now uses a customer-flagged NTSTATUS the epitaph names in words.

  Pre-spawning is unimplemented there and says why --- a Windows child is given its document at
  `CreateProcess`, so one started before a file is chosen has nothing to be handed.
  `Worker::spawn_shared` takes every open instead, at the ~6.6 ms macOS saves.

- **`backend-probe` runs on Windows, and passes.** The probe was `#[cfg(target_os = "macos")]`;
  its four platform primitives now have Windows bodies --- Toolhelp for its own module list and
  for finding its worker children in the process table, `GetProcessHandleCount` for descriptors,
  and `TerminateProcess` for a hostile kill from outside the pool, which is deliberately not
  `Contained::kill` because the pool has to notice a death it did not cause.

  **36/41, 5 skipped** on `text-base14`; **39/41, 2 skipped** on `vector-heavy`, where a tile is
  slow enough for the withdrawal checks to run rather than skip. No failures. The boundary, the
  pixel comparisons, capacity, crash restart, replacement, retirement, close and descriptor
  return all pass, and the 41 check *names* are unchanged, which is the cross-platform invariant.

  **The two failures it first reported were its own, and the correction is the point.** They read
  as a pool grown to six that keeps one, beside a handle count that never moved --- two
  independent observations agreeing on "created, used and destroyed rather than pooled", which
  was written into three documents as an open defect. Both were honest and neither could say
  *when* it was taken: `settled_descriptors` waits up to five seconds for a pre-spawned spare,
  Windows has none, and the wait's verdict was discarded, so it spent its whole bound every call
  --- longer than the four-second idle timeout the phase runs at. The instrument retired the pool
  and then measured it. The spare clause is now asked for only where a spare can exist, under a
  single named `PRESPAWNS` shared with the spare-lifetime skip, and a wait that expires prints a
  `[WARN]` instead of passing for a slow one. **Nothing in `workers.rs` changed.**

- **Windows pre-spawns workers too.** A worker can now be started, contained and warmed before
  a file is chosen on both platforms. The handover is the only part that differs: macOS sends a
  descriptor as `SCM_RIGHTS`, Windows `DuplicateHandle`s the document section **into the running
  child's handle table** and sends a `Handover` line naming the number it wrote --- the direction
  integrity levels permit, so it crosses the boundary structurally rather than by luck. A message
  of its own rather than a `Request` variant, which makes a second handover unsayable instead of
  something the child must refuse. Containment is unchanged and unconditional: the child is
  created suspended, dropped to low integrity and put in its job before it executes an
  instruction, whether or not it has a document yet.

  Measured with `prespawn-bench`: **8.4--9.6 ms saved per open**. The saving has a different
  *shape* from macOS and that is the finding --- there ~7.4 ms of it is the system-font walk, here
  ~1.4 ms is, so on Windows pre-spawning buys the fixed floor (`CreateProcess`, the loader,
  mapping `pdfium.dll`, the token, the job) rather than font enumeration. First Windows numbers
  in the repository, labelled as such.

  `backend-probe` now runs the spare checks there: **37/41** on `text-base14` and `text-cid`,
  **38/41** on `outline-hostile`, **40/41** on `vector-heavy`, no failures, with the spare
  identified and excluded from the pool at open and taken with its service at the end.
  `viewer_check.py` re-run on four corpora, since this changes the app's own behaviour --- all
  green, 44 modules at peak, no `pdfium` among them.

  Two things it broke on the way, both in checks rather than in code. `closing gives back every
  descriptor opening took` went red at *137 / 145 / 142* --- one spare's worth --- because its
  three samples were raw and an `open` starts a replacement spare on another thread; macOS was
  winning that race and Windows does not, so they go through `settled_descriptors` now. And the
  test asserting that `prespawn` refuses on Windows failed on its own, which is the evidence the
  behaviour changed; it is replaced by one pinning `PRESPAWNS` against what `prespawn` actually
  does, proved able to fail by restoring the stale value.

- **`pool-bench`, `prespawn-bench` and `tile-bench` run on Windows.** The first two gated the
  `--render-worker` re-exec on `#[cfg(unix)]`, left from before `worker_child` compiled there; a
  binary that re-execs itself as a worker and then refuses to be one is not degraded, it is
  unrunnable. All three hardcoded `vendor/pdfium/lib`, which on Windows exists and holds the
  import library, so the failure lands at `LoadLibraryExW`. `tile-bench` also gained a real
  `peak_rss_mb` there (`GetProcessMemoryInfo`/`PeakWorkingSetSize`) in place of `NaN`, keeping
  the `NaN`-on-failure contract.

  **`tile-bench` had never refused anything** --- the documented list of four blocked binaries was
  wrong about two of them, in the direction a list written by reading always errs. `worker-bench`
  is the one real refusal, and its reason is accurate: its own POSIX worker, fd passing and SBPL
  bisection, sharing no mechanism with the job-object model.

- **The render constants are measured on Windows, and `docs/PLAN.md` §4 holds with worse numbers.**
  Same generated A0 fixture, same PDFium pin. Spatial culling intact --- a 256² tile is **3.8%** of
  a full render against 4.3% on macOS --- and the per-render floor is real but larger: **~1.3 s**
  against ~1 s, with a full page at **35.1 s / 88.3 s** for 1x / 2x against 22.8 s / 48.4 s. The
  ratios that drove the architecture reproduce; every absolute number is **1.5--1.8x worse**, so a
  latency budget written against the macOS figures is optimistic here by about a third. Peak RSS
  532 MB. Cross-checked against `backend-probe`'s independent 1536 ms 512² render of the same
  document on the same machine before being believed. The cheap-page half is flat (0.6--0.9
  ms/Mpixel, no floor), which confirms the asymmetry the plan bets on but is **not** a
  cross-platform comparison --- macOS measured a fixture this machine has not generated.

- **A pool buys a screenful 3.6x on Windows, and nothing past six.** `pool-bench` on six 1024²
  tiles of the A0 page: monotone gains to six workers and nothing at eight, the same shape as
  macOS's 3.22x-and-nothing, with six stable to 0.01x across two runs. The intermediate sizes are
  reported as **not** conclusions --- pool 4 moved 1.99x to 2.29x between identical runs and the
  per-round warm figures span 20%, so only the six and the flat eight are outside the spread.

- **A dying worker's diagnostic is one write.** Rust's stderr is unbuffered and `write_fmt`
  issues a write per format piece, so with every worker of every pool inheriting one handle the
  pieces interleave --- a `pool-bench` run of ~120 workers ended holding `[worker] ` with no
  message, which is indistinguishable from a worker that failed with an empty reason and is the
  one thing that line exists to rule out. It is a `format!` and a single `write_all` now, verified
  by making a worker fail and reading its message back. Every error path reaching it produces
  non-empty text, checked; the fragment did not recur on a stderr channel of its own, which is
  also why the capture channel is now part of the trap.

- **`worker-bench`'s refusal named a Windows design that was measured out.** It cited "restricted
  tokens" and "named section objects"; `win-sandbox-probe` established that a restricting SID
  stops the loader before `main`, so containment is a low-integrity token in a job object, and the
  sections are anonymous because a name is something another process can open. A wrong reason on a
  refusal is worse than a vague one --- it is a design instruction, and someone reading it to build
  the spike would have built the two rejected things. It now also says what a spike would measure
  that nothing else does. Two stale `// The child half exists only on unix` comments removed.

- **The threat model's strongest claim is unverified on Windows, and now says so.**
  `worker-bench --mode engine` is the check behind "there is no engine to disable" and "XFA is not
  built in". It spawns nothing --- it reads the library file --- yet sat inside a `#[cfg(unix)]`
  module, so it had never run on Windows and the claim was untested there rather than merely
  unmeasured. Moved to file scope; on Windows it reports **`[NOT VERIFIED]`**, because the shipped
  `pdfium.dll` carries no local C++ symbols (`CPDF_Document` absent), so `v8::` and `CXFA_` being
  absent from it means nothing. That is the harness's second control working as designed.
  `docs/THREAT-MODEL.md` now scopes both claims to macOS and states that on Windows they rest on
  the asserted asset name and pinned digest --- a claim about which file was fetched, not about
  what is in it.

  It also reads the **PE export table**, the one dimension stripping cannot hide, and prints it
  *before* the stripped-binary exit rather than after --- the run that most needs it was showing
  nothing. **460 exports, four XFA-named**: `FPDF_LoadXFA` and `FPDF_GetXFAPacket{Count,Name,
  Content}`. Surface, not a contradiction --- the three `GetXFAPacket*` calls read `/XFA` streams
  out of an AcroForm dict and need no XFA engine. Whether `FPDF_LoadXFA` is a stub is open, and
  unlike JavaScript it is behaviourally decidable: an `/XFA` fixture makes
  `FPDF_GetXFAPacketCount > 0` a positive control. The old text said the property "cannot be
  tested behaviourally", which is true of JS and over-generalised to XFA.

  Both counts cross-checked against an independent Python PE parse before being written down, and
  every branch exercised: non-PDFium `[FAIL]`s, a non-PE file that passes both controls says "not
  a PE image" rather than printing a zero, a missing `--lib` exits 2, other modes still refuse.
  The bump checklist in `BUILD.md` had the macOS-only `vendor/pdfium/lib` hardcoded; it now names
  both platforms.

- **The last two macOS-only harnesses run on Windows, and one needed nothing.**
  `session_check.py` passed all four phases on the first attempt, both controls included --- it
  takes a binary rather than a bundle and `webview_guard` already returns early off darwin, so
  there was nothing to port. `open_check.py` needed a real port and now runs **four of its six
  phases** there (`argv`, `beats`, `control`, and all four launches of `race`). The two that
  cannot print `[SKIP]` with the reason, so the phase-name list is identical on both platforms.

  Those two skips record a **measured platform divergence** that was previously unstated in
  either direction: `RunEvent::Opened` is macOS-only and no single-instance plugin is linked, so
  on Windows **a second launch is a second process** --- two `tpdf.exe`, two windows, two worker
  pools, where macOS produces one app that swaps documents. Whether that is the behaviour to want
  is a product decision; the *emit* branch `running` exists to exercise is simply unreachable
  there. `HANDS_OVER_TO_RUNNING` is the one place that distinction lives, and each branching
  phase name is a constant rather than a literal at both call sites.

  That makes **four** documented-blocker lists found wrong this week, always by over-reporting.
  The trap now carries the tally: of six benchmarks and harnesses listed as macOS-only, two were
  genuinely gated, one was trapped behind a `cfg` it never needed, one had only a hardcoded path,
  one needed nothing at all, and one was two-thirds portable.

- **A harness that prints as it goes wrote nothing until it exited.** `BUILD.md` claims these
  scripts stream their results so a partial run names where it stopped. True in a terminal, false
  under a redirect --- Python block-buffers stdout off a tty, so an `open_check.py` run held **zero
  bytes** for twelve minutes, indistinguishable from one that died at import. `scripts/live_output.py`
  makes all three line-buffered explicitly, called rather than left as an import side effect.
  A/B'd at the same four-second mark: **0 bytes against 38**. The hazard was already written down
  as a caution in the cross-repo memory and had been read in the same session --- which is the
  argument for making it a line of code instead.

  It paid for itself the same hour. With streaming, an `open_check.py` run finished in **45 s**;
  the attempt immediately before it, without, sat at zero bytes for **17 minutes** while the app
  it launched held **0.00 CPU** --- hung at the first phase. Both harnesses need a clean process
  table on Windows: a leftover `tpdf.exe` hangs the next run, reproduced twice and cleared both
  times by killing the strays. `webview_guard` returns early off darwin, so nothing guards
  occlusion there, and the tell is the CPU figure rather than the clock.

  `worker_pids` matches a child on its **image name** there rather than on argv, because
  Toolhelp reports a parent pid and an image but no command line. Weaker, and sufficient for a
  stated reason rather than an assumed one: the `caffeinate` shape that forced the argv match is
  a macOS wrapper with no Windows counterpart, and the `--spare-lifetime` child is never started
  because pre-spawning is unimplemented. That check now skips with that reason instead of
  failing.

  `tpdf_lib::PDFIUM_SUBDIR` and `PDFIUM_LOADABLE` are public, because the "`lib/` exists on
  Windows and holds the wrong thing" trap had by then cost two binaries on two separate days.
  Four spike binaries still hardcode `lib`; the next one ported takes the constant.

- **Windows stops failing open.** `Backend::default_here()` selects workers on both platforms
  that have a boundary, which is now macOS and Windows rather than macOS alone. One word of
  code; the rest of the work is the evidence, because the `[WARN]` this replaces was our own
  bookkeeping and removing the line that prints it would have looked identical to fixing it.

  `scripts/win_modules.py` reads the app process's loaded module list from **outside** it,
  through Toolhelp, and `viewer_check.py` now launches the app rather than blocking on it so it
  can sample throughout the run and take the union --- the parser is mapped only while a
  document is open, so one look could miss it either way. The module count is printed beside
  the verdict: an enumeration that read *nothing* reports "not mapped" exactly as containment
  does, so a peak of zero is a broken observation and never a pass.

  Run **before** the change it reported the parser mapped, 47 modules at peak. That control is
  why the pass afterwards means anything. After: `outline-simple`, `outline-hostile`,
  `rotated-90` and `vector-heavy` all green with unchanged ran/skipped splits (81/5, 81/5,
  75/11, 52/34), no `[WARN]`, and 44--45 modules at peak with no `pdfium` among them.

  Both `render.rs` tests went red on the one-word change, which is what they are for. One named
  macOS as the only platform with a boundary and now states the platform list independently of
  the code --- deliberate duplication, since sharing a predicate would make it agree with
  whatever the code said. The other stopped naming a platform at all: the uncontained mark is on
  the timeline exactly when the default is the uncontained backend. Mutating the mark to be
  recorded unconditionally fails it; mutating it to never be recorded does **not**, on either
  platform that now has a boundary, because the branch no longer executes --- measured, and
  written down rather than left to be rediscovered.

- **The viewer runs on Windows**, and `viewer_check.py` passes there unmodified. Four corpora,
  each reporting the **86 check names** that are the invariant, with ran/skipped splits inside
  the macOS ranges: `outline-simple` 81/5, `outline-hostile` 81/5, `rotated-90` 75/11,
  `vector-heavy` 52/34, no failures. The harness needed no porting --- `webview_guard` already
  returns early off darwin, and WebView2 wants no bundle identity, so a plain `tpdf.exe` runs
  where macOS needs an `.app`. Windows is still **not supported**: it has no containment, the
  backend falls back to in-process, and it fails open rather than refusing.

- **The uncontained backend announces itself.** Off macOS `Backend::default_here()` falls
  back to in-process, and until now nothing recorded that a document had been parsed in the
  app process --- the refusal in `Worker::spawn` guards `TPDF_BACKEND=worker`, a path the
  default never takes. It now records `render::UNSANDBOXED_MARK` on the startup timeline and
  prints a `[WARN]`, once per process. Visibility, not containment, and deliberately not a
  refusal: refusing would make Windows useless rather than uncontained, which is a product
  decision rather than a defect to fix in passing. It matters more now that the viewer
  actually works there.

  The check asserts both halves from one run --- marked where there is no sandbox, *not*
  marked where there is --- because either alone passes with the code wrong. Two mutations:
  removing the mark turns it red; recording it unconditionally **survives on Windows**, and
  that is stated rather than hidden, since the assertion that would catch it is in the macOS
  branch and no run on this machine reaches it.

- **`sandbox_win`: the containment the probe measured, as a module.** A job object (memory
  cap, one process, kill-on-close, die-on-unhandled-exception) and a low integrity level,
  applied to a child that is created **suspended** so the job exists before the child runs an
  instruction --- assigning a job to a running process is a race the process can win, and a
  limit that is usually applied in time is not a limit.

  It fixes the shortcut `bin/win_sandbox_probe.rs` documented in itself: inheritance is
  narrowed by `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` to an explicit set rather than handing the
  child every inheritable handle the parent holds. Marking handles inheritable and naming
  them in the list are two halves of one decision, so `spawn_contained` does both and neither
  can be forgotten separately.

  Two checks were written asserting the opposite of what Windows does, and both were
  corrected by running the call rather than by reasoning: an empty handle list is refused
  (`ERROR_BAD_LENGTH`), so "inherit nothing" is modelled as `Option<AttributeList>` and
  reaches `CreateProcess` as `bInheritHandles: FALSE`; and a zero memory cap is refused by
  the kernel (`ERROR_INVALID_PARAMETER`), not silently accepted. `Job::assign` and
  `make_inheritable` are `unsafe`, because a safe function over a raw `HANDLE` hides a real
  liveness obligation --- a recycled handle value applies the operation to someone else's
  object rather than failing cleanly.

  `WORKER_MEMORY_CAP` is a **real** kernel bound, which is the one place the Windows story is
  stronger than the macOS one: `docs/THREAT-MODEL.md` §T3 records that macOS refuses every
  relevant rlimit and the substitute poll can bound a leak but never a burst. Nothing calls
  this module yet.

- **The worker's child half compiles on Windows**, and a contained child can check that it
  *is* contained. The module was `#[cfg(unix)]` and `lib.rs` refused `--render-worker`
  anywhere else; both are gone. Exactly three functions knew the platform and each is now one
  function with two bodies rather than the module being absent: `adopt_tile` and
  `adopt_document`, because macOS inherits a mapping on a number agreed before `exec` while
  Windows inherits a handle whose *value* has to be told to the child in argv; and
  `establish_boundary`.

  That last one is the asymmetry worth stating. macOS **applies** `sandbox_init` and fails
  loudly if it cannot. Windows has nothing left to apply --- the token is chosen at
  `CreateProcess` and is in force from the first instruction --- so it **verifies** instead:
  `integrity_level` reads the process's own mandatory label, `in_any_job` answers
  `IsProcessInJob`. Neither is sufficient alone, and the second is not sufficient even with
  the first, because a debugger or a terminal host puts a process in a job for reasons of its
  own; a `false` disproves containment and a `true` does not prove it.

  A handle may travel in argv where a path may not: the value means nothing in another
  process and inheritance is what makes it live, so it grants nothing, whereas a path would
  be authority a low-integrity child could act on --- low integrity governs writes, not reads.
  Parsed as `usize`, tested with a value above `u32::MAX`, because narrowing would not fail,
  it would produce a *different* valid-looking handle.

  Deleting the `cfg(not(unix))` refusal is the part that needed proving rather than the port.
  It was never load-bearing; `establish_boundary` is, and now has a test that an uncontained
  process is refused. That test does not run on macOS, deliberately: there the call would
  *succeed*, leaving every later test in the process inside a sandbox with no filesystem and
  failing for reasons unrelated to what they assert.

  The containment policy is a pure function of the two facts, and that split is a finding
  rather than tidiness. Written as one function it could not be tested --- a test runner fails
  the integrity clause and returns, so the job clause was unreachable and deleting it outright
  passed every test. Two further mutations were **identities** and read exactly like missing
  coverage: a mandatory-label SID has one sub-authority, so indexing its first instead of its
  last changes nothing; and `!=` versus `>` on the level differ only for a level *stricter*
  than low, which is why a test now asserts untrusted integrity counts as contained.

- **A contained child gets pipes, and one was heard.** `spawn_contained` takes an optional
  `Stdio` and sets `STARTF_USESTDHANDLES`. `STARTUPINFO` cannot say "this stream, leave the
  others alone" --- the flag makes the child take all three --- so a stream left null is a child
  with no stderr rather than a child with the parent's, and `Stdio` has no per-stream
  `Option`. stderr is shared rather than piped for the reason `worker_child.rs` gives:
  nothing is reading a third pipe at the moment a worker dies.

  The stdio handles are folded into the inherit list by `spawn_contained`, not by the caller;
  with a handle list present, a standard handle the child is told to use and that is not in
  the list is simply not inherited. `pipe()` marks **neither** end inheritable, because
  `CreatePipe`'s security attributes would mark both and the end the parent keeps must never
  reach the child --- a worker holding the read end of its own reply pipe can watch every
  answer it gives.

  The test is the result rather than the code: `cmd.exe` at low integrity inside a job runs
  and a known string comes back. One test rather than four, because the four failure modes
  are indistinguishable from outside --- wrong flag, handle missing from the list, handle not
  inheritable, parent's copy of the write end left open --- and the last is a hang, not an
  error.

- **A parent can watch a contained child**: `try_wait`, `kill`, `wait_timeout`, `epitaph`,
  matching what `std::process::Child` offers on the other platform. Two findings, both in
  `docs/TRAPS.md`.

  `GetExitCodeProcess` reports `STILL_ACTIVE` for a live process, and `STILL_ACTIVE` **is
  259** --- an exit code any process may legitimately choose. Telling the two apart by value is
  wrong for exactly that one input, and a worker that really exited 259 would read as running
  forever while the pool waited on something already gone. Liveness comes from
  `WaitForSingleObject` with a zero timeout instead, and the code is read only once that has
  answered.

  The lifecycle test could not fail, and how that surfaced is the more useful half. Mutating
  `kill` into a no-op did not turn it red --- it made the run take **177 seconds** against a
  180-second harness timeout, and the harness printed `test result: ok` and `[HUNG]` in the
  same output without noticing those contradict. The assertion was "kill, then wait for the
  exit code", and an unbounded wait has two outcomes: pass, or block forever. A blocked test
  is not a failing test. With `wait_timeout` the same mutation fails in 10.02 seconds and
  names the test.

  Still nothing spawns a Windows worker: `Worker` holds a `std::process::Child`, and these
  are the pieces its Windows half will be built from.

- **`Shm` is real on Windows.** Every off-unix constructor previously returned "render
  workers are implemented on macOS only" --- which reads like a containment decision and was
  the absence of an implementation wearing the language of a policy. It is now a nameless
  section object: `CreateFileMappingW` with a null `lpName`, so it is reachable only through
  a handle, which is the same property the unlinked temp file buys on the POSIX side. A
  section holds its own reference to what backs it, so `map_file` closes the file handle and
  a child needs only the section --- there is no Windows analogue of passing a descriptor
  alongside.

  `raw_fd` is the one method not carried over: a `HANDLE` is pointer-sized and an `i32` is
  not, so returning one would truncate on 64-bit into a value that still looks like a
  plausible descriptor. `raw_handle` and `from_handle` replace it.

  Four mutations. Swapping the halves of the 32-bit length split turns three checks red;
  making the document mapping `PAGE_READWRITE` turns two red with `ERROR_ACCESS_DENIED`,
  because Windows refuses a writable section over a read-only file handle exactly as `mmap`
  refuses `PROT_WRITE` --- which is what makes an otherwise unprovable property provable
  without a faulting write. Stripping `FILE_MAP_WRITE` is caught too, but as a
  `0xC0000005` process death with **no** `test result:` lines at all, which is why the run
  was checked for positive evidence rather than grepped for `FAILED`. The fourth survives:
  reversing the drop order changes nothing, and the comment claiming it leaked was simply
  wrong. The order is kept and the comment now says no test pins it.

  `Worker::spawn` still refuses off macOS, which is the refusal that was always about the
  sandbox rather than about missing code. Its check needed a real fixture to keep meaning
  that: it passed `"nonexistent.pdf"`, which was fine while every constructor refused
  identically and would have gone red at "could not open" the moment `map_file` worked.

- **What Windows containment can be, measured** (`bin/win-sandbox-probe`). macOS gets its
  boundary from `sandbox_init`; Windows has no counterpart, so containment there is assembled
  from a job object, an integrity level and a restricted token, and which combination still
  lets PDFium render is not documented anywhere. Six rungs, each rendering the same tile in a
  re-exec'd child and compared **pixel for pixel** against an in-process render --- pixels
  because the macOS work already caught a sandboxed PDFium returning `ok` while silently
  substituting a typeface, and the default fixture is `text-base14.pdf` because base-14 faces
  are not embedded and must be found on the system.

  **A job object plus low integrity is the answer**: byte-identical output, and the child
  loses the authority to write `%USERPROFILE%` or `OpenProcess` the parent. It does not lose
  *reads* --- an integrity level governs writes --- which is why the child is handed its
  document and its output as inherited handles and never a path, the Windows analogue of the
  macOS `dup2`. A restricting SID is the stronger rung and dies in the loader at
  `STATUS_DLL_NOT_FOUND` before `main`, needing Chromium's initial-token / lockdown-token
  handover to reach.

  Two rungs are marked diagnostic and excluded from the verdict, because with `restricted`
  failing either ingredient was a plausible cause and one row cannot attribute it; the
  restricting SID turned out to be the whole cause. Excluding them was not cosmetic --- the
  verdict took the last row that worked, which was the one that denies nothing.

  Both mutations went red at the assertion aimed at: disabling handle inheritance broke the
  control and the probe said so in those words, and flipping one byte of the child's output
  turned `identical` to `NO` and reported a one-byte difference. **Nothing uses this yet** ---
  `RenderService` still selects in-process off macOS, so Windows fails open exactly as before.
  `windows-sys` is MIT/Apache and was already in the tree transitively, so the dependency adds
  no crate; checked with `cargo metadata` rather than assumed.

- **A `bins` gate** (`cargo build --locked --bins`), because none of the other gates links a
  binary. `scripts/gates.py` reported 7/7 while `npm run tauri build` failed on the same tree:
  clippy stops at metadata, and `cargo test` links each `[[bin]]` with `main` replaced by the
  harness's own, so `backend_probe.rs`'s two unguarded dyld symbols were dead code the linker
  dropped. Proved to fail before being trusted --- red in 5.7 s against the un-gated file, in
  the debug profile, checked separately from the release observation because the finding is
  precisely that linking depends on how the target was built.

- **Five checks on the tile origin** (`src/lib/tiles.test.ts`), asserting both platform
  spellings. Four mutations, each matching its prediction: hardcoding the macOS scheme in the
  URL turns one red, in the origin three, dropping the memo one, and encoding the whole path
  two. The mutation harness's own cross-check fired on its first run --- it parsed twice as
  many failures as vitest's summary, because `FAIL ` matches the file-level block as well as
  each test --- and reported a broken run rather than either number.

- **A print job is now checked against documents `lopdf` did not write.** Every other test
  of `print::build` feeds it a fixture the same serialiser produced, so the module was a
  writer tested against its own reader with only the read-back independent --- and printing
  is the one subsystem here whose output leaves the process. The new check builds a subset
  with a quarter turn from five generated corpora, takes both the page list and the expected
  rotations from PDFKit rather than from `lopdf`, and skips loudly per fixture, naming for
  each whether its rotations discriminate *which* pages survived or only the count.
  `rotated.pdf` is the one that does, carrying four rotations on four identical pages.

  Shown to fail by three mutations: a composed rotation that ignores what the page carries
  (`[90, 90]` where `[90, 0]` is right), a selection that keeps the first N pages instead of
  the ones asked for (`[90, 180]`), and a `/Count` left contradicting its `/Kids` --- which
  every `lopdf`-side check passes while PDFKit reports four pages for a two-page job, the
  two real ones and two it manufactures to satisfy the count.

- **`scripts/open_check.py` drives two overlapping opens** (`race`), the case `openPath`'s
  queue exists for, issued from inside the app because Launch Services hands over one
  document at a time. Four cold launches, since repeating the round *within* a launch was
  measured and is worse --- only the first round is cold, and the rest run against warmed
  workers and land in the same order every time.

- **A per-request deadline on worker calls** (`TPDF_CALL_MS`, default 30 000 ms; zero means
  zero, unreadable values fall back). A request that does not answer within the deadline now
  kills its worker and returns an error --- previously it held one of the pool's service
  threads forever, a handful of such requests wedged rendering for every open document, and
  `close` hung on its drain. The kill is announced (`[render] worker <pid>: no reply in
  <n> s; killing it`) and is not retried: a crash retry costs milliseconds, a deadline retry
  costs another deadline of a service thread. `docs/THREAT-MODEL.md` T3 is corrected to
  match what ships --- the deadline is wired, `RLIMIT_CPU` is measured and deliberately not
  set, and the footprint poll is measured and *not* wired, which the section previously
  stated as an operating mitigation.

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

  **Double-click selects a word and triple-click the line it is on**, and a drag begun with
  either extends by whole units instead of dropping back to characters --- which is what makes
  dragging out a sentence land on word boundaries the way it does everywhere else. A fourth
  click has no larger unit to reach for, so it wraps back to a caret rather than repeating the
  line.

  Word edges are runs of letters, digits and combining marks: correct wherever words are
  separated by something, and wrong for Chinese, Japanese and Thai, where a double-click takes
  a whole clause. `Intl.Segmenter` would know better and is deliberately not used --- it
  segments a *string*, and this layer works in code-point indices precisely because
  `FPDFText_GetText` drops characters and desynchronises the two spaces.

  The click counter is keyed on where the *document* was clicked rather than the screen, so a
  scroll, zoom, rotation or page jump between two clicks ends the run by construction instead
  of by a `reset()` call at each of those sites. Units are found from the character under the
  pointer rather than the caret beside it: the caret after a word's last glyph names the
  following space, and a word selection built on that selects the gap.
- **Recent documents in the command palette.** ⌘K, then type part of a name. The list itself
  is not new --- `session.rs` has kept every document read, most recent first, since session
  restore needed it --- but reaching the second entry has never been possible, so a reader
  who wanted yesterday's *other* document went through the file dialog for a file tpdf
  already knew about.

  They are commands rather than a menu, because §8 says every command is reachable in two
  keystrokes through the palette. The registry gained `replace(prefix, commands)` to swap the
  group when it changes, which also drops the replaced ids from the recently-run list --- inert
  today and wrong the moment an id is reused for a different document, which is what these
  ids do.

  Labels show the basename and lengthen **only where two collide**, one directory at a time.
  `report.pdf` in three client folders is the normal case, and three identical rows are worse
  than no list; a full path is unique and unreadable at a glance.

- **A search-results sidebar tab.** Every hit in the document, one row each: the page number
  and the words around the match, with the match emboldened. Picking a row moves the document
  to it. `12 of 5712` in the find bar says how much there is and nothing about what is in it.

  The snippets come from the backend, and that is not an optimisation: the words around a hit
  are on the *page*, and the frontend does not have the page --- Rust extracts the text,
  matches, and drops it again. Building them here would mean re-fetching every page a hit is
  on. A `Match` now carries `before`, `hit` and `after`, three strings rather than a string
  and two offsets, because an offset into a snippet would be a third index space beside the
  page's code points and JavaScript's UTF-16.

  Rows are appended rather than rebuilt --- a 775-page scan reports 775 times --- and capped
  at 2,000 with the cap *stated*, while the match count stays exact. Both mutation harnesses
  now refuse a mutation whose expected test does not exist: one of these named a functional
  check that vitest cannot run, and the pass reported SURVIVED, which reads as a gap in the
  suite rather than a mistake in the harness.

- **A bound on the front-end text cache.** Least-recently-used, 400,000 characters --- about
  16 MB --- with a floor of eight pages kept whatever they cost. It was unbounded, and search
  is what made that matter: a whole-document scan never touches it, but *stepping through* the
  results loads every page a hit lands on, so 5,712 matches over 775 pages is 775 pages of
  characters retained by somebody holding down ⌘G. `peek` counts as a use, so the pages on
  screen are always the youngest and are the last that could be dropped.

  Two of the eight tests written for it could not fail, and both are recorded. One covered a
  correction in `remember` that is unreachable --- `load` returns from the cache before it
  issues a request --- so the code went with the test. The other asserted that an evicted page
  reads as null, which passes whether or not the *rotated* copy of it was dropped too, because
  nothing reaches that map for a page `pages` has lost. A leak no behaviour can see needs an
  accounting observable, not a cleverer assertion.

- **Matching case and whole words, in find.** ⌥⌘C and ⌥⌘W, two toggles beside the find field,
  or the palette. Both default to off, so a reader who never touches them gets the search that
  was there before. A toggle rescans the current query rather than filtering what it found:
  deciding whether a hit is a whole word needs the characters *next to* it, and having those
  on the front end means shipping a 775-page document's text to answer a question about a
  dozen hits.

  Matching case turns off half the fold and nothing else --- whitespace still collapses and
  soft hyphens still disappear, because neither is about case. Whole word is `\b` over the
  folded sequence, so a soft hyphen does not break a word and a line break does. Its word
  class is letters, digits and underscore and deliberately **not** combining marks, which the
  front end's own word selection does count; the consequence is that a whole-word search for
  `cafe` still matches a decomposed `café`, which is what the unrestricted search does anyway.

  `scripts/mutate_rust.py` is new: the backend had no mutation harness, and `search.rs` is its
  densest logic. 16 mutations, each caught by the test named for it. Writing it exposed two
  defects elsewhere --- `keys.ts` rendered Shift before Option while its own comment said the
  reverse, unreachable because no binding held both; and the new harness reproduced, through
  `shutil.copy2`, the mtime-restore defect already recorded here as a `mv` problem. It was
  never a `mv` problem, and cargo served the last mutation to every run afterwards.

- **Go to page, and commands that take a value.** ⌥⌘G, or "Go to page…" in the palette, turns
  the palette's input into a value field with a placeholder, live validation and a preview of
  what Enter will do. A 775-page document previously had no way to reach page 400 at all:
  Home, End, and one page at a time.

  The mechanism is general. A command declares a `CommandArgument` --- `placeholder`,
  `problem`, `preview`, `run` --- and the palette does the typing; `Command` became a union so
  one can no longer be declared with neither `run` nor `argument`, a shape that would
  type-check, list in the palette and do nothing when chosen. Escape steps back to the command
  list rather than closing, so a mistyped number does not cost the palette as well.

  A page past the end is **refused, not clamped**: someone typing 900 into a 775-page document
  has made a mistake, and silently landing on the last page hides it. The registry re-checks
  the value the palette gives it, which is what makes it safe to call from a keybinding.

  Adding ⌥⌘G required fixing `matches`, which **never looked at `altKey`**: every binding
  matched with Option held as well as without, so ⌥⌘F opened find and ⌥⌘G ran find-next. The
  same both-directions bug the Shift check exists to prevent, one modifier over.

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
- **A worker is started, sandboxed and font-warmed before any document is chosen.** Opening a
  file then costs **0.3--1.1 ms** instead of 8--17 ms, because the process is already past its
  link, its `sandbox_init` and PDFium's system-font walk. The A0 sheet keeps 48 ms of its 56 ---
  that is page parse, which no pre-spawn can remove, and it is the row that says the
  measurement is not merely reporting zero.

  The document is handed over **after** the sandbox, as an `SCM_RIGHTS` descriptor: a
  pre-spawned worker has already dropped the authority to open a file, so a path would be
  useless to it even if it were trusted. `bin/fdpass_probe.rs` proves that crossing, with the
  control that the child cannot read `/etc/hosts` at the time.

  One spare, not a pool of them --- it is for the *first* worker of a document, which is what a
  reader waits on. A spare that dies falls back to an ordinary spawn rather than failing the
  open.

  A mutation pass afterwards found that **none of the three mechanisms this added was visible
  to any check**: deleting the font warm, skipping the readiness wait and dropping `FD_CLOEXEC`
  each left `backend-probe` green on every corpus. Two were real and are now pinned, and the
  third turned out to be unreachable defence:

  - `bin/prespawn_bench.rs` asserts and exits non-zero instead of printing a table. The
    comparison is between a base-14 fixture and an embedded-font one, because the gap between
    them *is* the system-font walk that warming pays early: 0.35 against 0.80 ms warm, 9.96
    against 0.84 ms with the warm deleted, over a 3.7 ms bound.
  - `backend-probe` gained "a spare does not outlive the service that started it", which runs
    this binary as a short-lived service and asserts the spare died with it. It needs a second
    process because the leak cannot be seen from inside the one that has the socket open.
  - `PreWorker::wait_warm` now *consumes* its receiver and returns a `WarmWorker`, the only
    type `adopt` accepts. The runtime check it replaces could not be made to fail, because a
    spare is only ever published warm --- but that was enforced in another module, so the
    ordering moved into the type rather than being deleted. Skipping the wait no longer
    compiles.

- **Tiles of one page render in several worker processes at once.** The worker backend is
  served by a pool of threads over one job queue, and each document has a pool of processes
  they draw from. A screenful of the A0 sheet goes from **3.46 s to 0.83 s, 4.2x**, measured
  through the service itself with interleaved rounds; a cheap page gains 2.7x. Six workers is
  where the curve flattens --- neither the core count nor the performance-core count.

  **Growth is lazy**: a document opens with one worker and gains another only under
  contention, so a reader turning one page at a time never pays for a second parse of it.
  A fully grown pool on the A0 sheet is about 290 MB, which is given back again once the
  scrolling stops --- see the retirement entry below.

  The in-process backend is deliberately *not* pooled: concurrent PDFium in one process is
  undefined behaviour whatever the handles are.

  Two of five mutations first survived, and both pointed at the design rather than the tests.
  With one thread per worker the pool's own capacity bound was unreachable --- the thread
  count was enforcing it --- which also meant six tiles of a slow document could occupy every
  thread and starve a second document whose workers were idle. Threads are now `pool + 2`.
- **An idle worker is retired, so a burst of scrolling no longer decides what the session
  keeps.** A worker untouched for 30 seconds is killed, down to one per document. On the A0
  sheet that returns **242.5 MB of a 289.9 MB pool** and charges the screenful after the
  pause **+65 ms on 811 ms**; on the text corpus, 56 MB and +15 ms. Both measured over two
  runs by `pool-bench --mode retire`, pairwise within interleaved rounds.

  **One worker is kept rather than zero.** Nothing breaks at zero --- the checkout path
  spawns from an empty pool and the close drain is trivially satisfied by it --- but the
  saving is one process against a spawn and a full re-parse charged to the next page turn,
  which is the moment someone is watching. Retiring to one already returns five sixths.

  The reaper thread holds a **weak** handle to the pool. A strong one would keep every worker
  and every document mapping alive after the last handle to the service was dropped, which is
  a larger leak than the one being fixed and is invisible to any check running against a live
  service --- so `backend-probe` now drops a service and asks the OS whether its processes
  went with it.

  Eight checks, and six mutations all caught. The one that matters is the *control*: a sample
  taken before the timeout expires, without which "the pool shrank to one" is equally
  satisfied by a reaper that kills everything on every sweep.
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

- **Document opens are serialised by `src/lib/serial.ts`** rather than by four lines inside
  `App.svelte`. The behaviour is unchanged --- one open at a time, in call order, a failure
  never stopping the next --- but the invariant now has tests that can fail, which it could
  not while it lived in a component with no harness. The end-to-end check that exercises it
  through the running app is a race, and a race is a smoke test rather than a gate: measured
  with the queue removed, it reports the defect in roughly two runs out of three.

  Writing it turned up unreachable code of its own. The chain was built with both `then`
  arms calling the body, copied from the original; a mutation reducing it to one survived
  the whole suite, because the tail is assigned a promise with both outcomes flattened and
  therefore can never reject. The arm is gone and the line that makes it impossible now says
  so. Three mutations of what remains --- no queue, no flattening, a tail that never advances
  --- each go red on the test aimed at them.

- **The tile-retry backoff moved into its own module** (`src/lib/backoff.ts`), with unit
  tests for the properties the scroller relies on: a failed request is not reissued before
  its wait, each further failure doubles the wait up to 8 s, an already-due entry reports no
  wake (the busy-loop guard), success forgets the entry. The clock is a parameter, which is
  also the fix for a dropped wake — the frame and the retry scheduler previously read the
  clock at two different moments, and an entry falling due between the two readings got no
  wake and stayed blank until unrelated input. Tile and thumbnail failures also now name
  their reason on the console, once per request rather than once per attempt.

- **`displayedSize` exists once** (exported from `scroller.ts`) instead of three times ---
  the odd-turn dimension swap was independently implemented in the scroller, the viewer and
  the page strip, and a rotation fix applied to one would not have reached the other two.

- **The eager startup open is only collected for the path it was started on.** A first open
  naming a different file than `TPDF_STARTUP` now falls through to a normal open instead of
  silently receiving the pre-opened document.

- **The worker pool moved out of `render.rs` into `workers.rs`.** That file held the service,
  both backends, the pool, the spare slot and the reaper at 1,958 lines. Nothing changed in
  the move — verified by asserting every top-level item HEAD defined exists in exactly one of
  the two files, that no test name was lost, and that the moved block diffs clean against
  `HEAD` apart from the accessors that replaced `RenderService` reaching into `SpareSlot`'s
  fields. `render.rs` keeps the service, the `Engine` trait and the in-process control;
  `pool_size` and friends are re-exported on the path the benchmarks already import.


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
  asking for that file. Checked by `scripts/open_check.py` across nine launches and 31
  checks, six of them controls --- and the checks themselves by eleven mutations, all of
  which behaved as predicted, including one predicted to survive.

### Added

- **A release workflow, firing on a CalVer tag and on nothing else.**
  `.github/workflows/release.yml` gates both platforms by invoking `scripts/gates.py` — not
  by re-listing its commands — then builds, signs, notarizes and publishes a draft release.
  macOS is Apple Silicon only: `fetch_pdfium.py` installs one architecture, and an x86_64
  slice carrying an arm64 engine would fail at bind time on a machine nothing here can test.

  The part with no precedent in the portfolio is signing the bundled `libpdfium.dylib` —
  neither `screenpick` nor `dblitz` ships a native library, and notarization requires every
  Mach-O in the bundle to be Developer ID signed with the hardened runtime. It is signed in
  `vendor/` before the bundler copies it, which holds whether or not Tauri re-signs nested
  resources.

  **Nothing in the macOS half has run yet.** Its verification step fails rather than warns,
  because a skipped notarization exits 0 and yields an app Gatekeeper rejects on any machine
  that has never seen it.

### Changed

- **The installers no longer ship the development spikes.** All 17 probe and benchmark
  harnesses were `[[bin]]` targets of the crate Tauri bundles, so every installer carried
  them — a sandbox prober and a hostile-document harness included. They are `[[example]]`
  targets now, which the bundler does not enumerate: the MSI payload is three files, and the
  MSI went 16.7 → 8.0 MB with the NSIS setup 8.8 → 5.8 MB. On macOS this is also 17 fewer
  binaries for the hardened runtime to sign and notarize.

  Invocations move with them — `--example <name>` rather than `--bin`, and artifacts land in
  `target/release/examples/`. Clear out any probe executables left in `target/release/`;
  nothing rebuilds them and an older documented path still resolves to a frozen copy.

  The `bins` gate takes `--examples` now, and that flag is not decoration: the file that
  motivated the gate is one of the moved ones, so without it the gate would link only the app
  and pass in under a second looking exactly as it did when it covered seventeen targets. An
  undefined symbol called from an example's `main` turns it red with `LNK2019`.

- **`backend-probe`'s Windows figures were a commit behind, and read as a missing check.**
  `BUILD.md` and `AGENTS.md` recorded `37/41 ... 40/41` there against 42 on macOS, and the
  gap was carried as an open question --- which check is macOS-only? --- with the parent's
  memory poll as the candidate, since `worker-probe` really does skip that one on Windows.
  None is. The 41s were taken at `df1ca61` and `9fb728f` added a check immediately after, so
  the two counts differed by a commit rather than by a platform. Re-measured on Windows:
  **38/42, 38/42, 39/42, 40/42** across `text-base14`, `text-cid`, `outline-hostile` and
  `vector-heavy`, no failures, and the name sets byte-identical across all four when diffed
  rather than counted. `BUILD.md`'s flat *"all 42 names appear"* was right as written and
  stays flat; the proposal to weaken it into a per-platform statement is what the
  mismeasurement would have cost. New trap: *Two counts from two commits are not a platform
  difference*.

- **`latency-bench`, closing the last measurable Windows gap.** `worker-bench --mode latency`
  decomposes what one tile costs --- render, encode, the parent reading it, and everything left
  over --- and cannot run off unix: it carries its own worker, `dup2` handover, socket pair and
  SBPL bisection. Its own refusal named that decomposition as the single thing a Windows spike
  would measure that nothing else does. This is that spike, and deliberately not a port: it
  drives the **production** worker, so it is portable by construction and macOS can cross-check
  it against an implementation sharing no worker code with it. **Every figure below is Windows,
  and it has never been run on macOS** --- that it compiles there is a claim about a compiler,
  not a result.

  There is no `pipe` variant, which is a finding rather than an omission --- production sends
  every tile payload through the shared mapping and never inline, so a pipe row would measure a
  route no tile takes. Differencing `raw` against `png` recovers the same quantity from two
  paths that are real.

  Windows, 1024² tile: crossing the boundary costs **0.263--0.283 ms**, a round trip carrying no
  tile **0.039--0.068 ms**, and moving bytes through the mapping **0.0051--0.0058 ms per 100 KB**.
  The boundary cost is a property of the boundary, so the number to read is that it varies by
  0.02 ms across fixtures whose render times differ by three orders of magnitude.

  Its own defects were found by running it rather than by reading it, and each is now a trap. It
  misparsed the outline reply as an array, so a defaulted zero printed *"the document has no
  outline"* for `outline-simple.pdf` while its own control timing four lines above said
  otherwise. It estimated the boundary cost by subtracting two ~2.7 s end-to-end figures, which
  on the A0 sheet reported **-265.822 ms**. It guarded payload differencing on ordering rather
  than materiality, so a page that barely compresses divided noise by a 68 KB gap. And the check
  added to stop the bad estimator returning was itself too weak twice over: requiring only that
  the figure be positive caught the defect just on the runs where the noise happened to fall
  that way, and the replacement compared a spread against a figure derived separately, so a
  mutation moved one and left the other sound. Both now come from one per-round vector.

  Verdicts go through a recorder that pads every label to seven at column 1 --- an indented
  `[SKIP]` had been invisible to the repository's own width recipe, which then passed by never
  examining it --- and the tag vocabulary is back to the four every other harness emits, a
  `[NOTE]` invented here having been dropped silently by anything grepping the set. The run ends
  with `N/M checks passed` and exits non-zero on a failure. 4/4 mutations caught, restored by
  bytes and verified by digest.

- **Every reference to a spike's path now resolves.** The 2026-07-31 move of 17 harnesses from
  `[[bin]]` to `[[example]]` left 34 `bin/<name>.rs` references across docs and doc comments
  naming a path that no longer exists. Repointed with each target verified to exist; the dated
  entries in this file and the trap describing the move itself keep their original paths, because
  a historical record naming a historical path is correct.

### Fixed

- **The installers shipped no PDF engine.** `tauri.conf.json` declared no `bundle.resources`,
  so nothing ever copied PDFium into a bundle, and the resource-directory fallback in
  `pdfium_library_dir` pointed at a directory the bundler never created. Every installer built
  before this produced an app that opens a window and cannot parse a document on any machine
  without this repository checked out at the same absolute path. `tauri.windows.conf.json` and
  `tauri.macos.conf.json` now carry the library, because the two archives disagree about where
  the loadable one lives.

  It survived because every check ran where the dev tree exists, so the bundled branch was
  never exercised — `viewer_check.py` against the bundle passed either way. Now proved against
  the extracted MSI with the dev library moved aside: **102/102 checks passed on the bundled
  library alone**, against a negative control with no PDFium reachable that fails and names the
  path it looked in. The lookup tries two bundled candidates, because Tauri's WiX template
  ignores a resource map's target directory and puts the DLL beside the executable.

- **The macOS half of that fix did not work, and the check found it** (2026-07-31, verified on
  a Mac). `"...libpdfium.dylib": "pdfium/"` produced neither bundled layout: the macOS bundler
  reads the value as a target *path* and renamed the dylib to the **file**
  `Contents/Resources/pdfium`. Both candidates missed, and a bundle with the dev tree hidden
  reported `0/1 checks passed` with three `could not load Pdfium` lines. `tauri.macos.conf.json`
  now names the file (`"pdfium/libpdfium.dylib"`); with the dev library hidden the same bundle
  reports **102/102 checks passed, 7 not applicable, 109 names**. `tauri.windows.conf.json` is
  unchanged on purpose — WiX ignores the target either way, and that platform cannot be re-run
  from a Mac.

  Every cheap observation agreed with the working case: the build exits 0, the bundle is the
  right size, `find` prints a path containing `pdfium`, and `viewer_check.py` from the repo
  passes. What discriminates is `-type f` against `-type d`. See the trap of that name.

- **`backend-probe` and `worker-probe` mis-aligned their own output on the rows that passed.**
  Both built the verdict as `"[{}] {name} ..."` with `OK` or `FAIL` interpolated, and `[OK]` is
  two characters shorter than `[FAIL]`/`[SKIP]` — so passing rows started two columns to the
  left, in the terminal and in anything parsing them. `BUILD.md`'s documented `cut -c8-47`
  recipe for extracting a check-name set then sliced those rows short, and diffing three
  `backend-probe` corpora reported **"the name sets diverge"** for three runs whose sets were
  identical. The count agreed throughout, which is what made it look like a real regression
  rather than a broken read. Both labels are padded to seven now, matching every other harness,
  so one recipe reads all of them; `prespawn-bench`'s summary line had the same shape and is
  fixed with them. `BUILD.md` says the `8` is a property of the harness and how to check it.

- **The Rust test gate printed a bare `error:` line while passing.** Two Windows checks spawn
  a worker whose child is the libtest harness, which has no `--render-worker` dispatch and
  says so on the stderr every worker inherits by design. That refusal is the checks' control,
  but it landed on the gate transcript as `error: Unrecognized option: 'render-worker'` above
  its own `ok` line, so a clean run of 205 tests was indistinguishable from one that failed
  and reported it badly. A test-only guard now points the process's stderr at the null device
  for the length of the spawn and restores it on drop --- the console changes, the child does
  not, and no `cfg(test)` branch enters the code under test. The gate transcript now contains
  no `error:` line at all.

  **The `#[cfg(unix)]` arm compiled and ran on macOS for the first time on 2026-07-31, and the
  claim it was written on turns out to be false there.** The noise does not occur on macOS:
  removing the `install()` call changed no output over 40 runs, while the same harness invoked
  directly prints `error: Unrecognized option: 'render-worker'` and exits 101. Holding the
  `PreWorker` for 400 ms before dropping it makes the line appear exactly once, which names the
  mechanism — `prespawn` returns as soon as `fork`/`exec` is issued, the test drops the child
  at once, and the kill lands while it is still in dyld, before libtest parses argv. Windows
  creates the process suspended and resumes it, and loses that race. The guard is kept rather
  than deleted, because the impossibility lives in another type's drop timing rather than in
  this arm, and the doc comment now says so.

  Proved by removing the guard, which puts the line back. `docs/TRAPS.md` records why that
  mutation has to be run single-threaded: the window is process-wide, so with the module's
  other checks running beside it the deletion printed nothing and read as a guard nothing
  needed.

- **Enter on a page thumbnail or an outline row could go to the wrong place.** Both the strip
  and the outline tree activated `focused` --- their own record of which row has focus, kept up
  to date by a `focusin` listener --- rather than the row the key actually reached. That record
  is a mirror, and `focusin` is not guaranteed to arrive: a document without system focus moves
  `activeElement` without delivering any focus event. A stale mirror sends the reader to
  whatever it still names, which is page 1, since it starts there. Both now read the row from
  the event itself and keep the mirror only for a key that landed on the container rather than
  on a row.

  Found from a single `viewer_check.py` failure on `vector-multi` that has never recurred, so
  it is an identification by mechanism rather than by catching it twice --- `docs/TRAPS.md` has
  what that does and does not establish. Each class has a unit test that was shown to fail
  first and a control on the fallback; `sidebar.ts` had no unit tests before this.

- **A harness phase could time out with no transcript at all, because render workers inherit the
  app's stdout.** `open_check.py`'s handover phase captured the app through a `PIPE` and waited on
  `communicate()`, which returns only at EOF --- and the workers are re-execs of the same binary
  holding the same descriptor, so one outliving the app by a moment produced `run timed out` with
  nothing printed before it. That reads as the app hanging on one phase while an adjacent phase
  passes 7/7. It now redirects to a file and waits on the *process*, so there is no EOF to wait
  for, and a timeout keeps whatever the run had already written.

  Two smaller fixes fell out of it. `scripts/stray.py` clears leftover instances of the binary
  under test before the first launch and prints a `[WARN]` naming the pids when it finds any: on
  Windows a stray instance **silently absorbs** every later launch through the single-instance
  plugin, so a run that needed clearing is a run whose earlier phases are suspect. And the
  handover's scratch directory tolerates cleanup errors, because the inherited handle can still
  hold the log file for a moment after every check has passed.

- **`session_check.py` reported a wrong page instead of a wrong fixture.** Its target page is 7
  and `Viewer.goToPage` clamps to the last page, so a document with fewer than eight pages gave
  *"it opens on the remembered page: page 0, wanted 7"* --- stably, on a session restore that was
  working perfectly (verified afterwards at 7/7 on twenty pages). There is a named check for the
  precondition now. The first version of it read the count immediately after the viewer appeared
  and reported "0 pages" for every document, because the status it comes from is published a frame
  later; it waits for the value and keeps "never became known" distinct from "too short".

  **The named check did not finish the job, and the rest of it is fixed here.** It fails inside
  the `record` phase, which returns --- but the Python driver cannot see that, accumulates with
  `ok &= report(...)` and launched the other three phases anyway. So a short fixture still
  produced eleven failures, ten of them describing a restore that was never attempted, and the
  transcript still *ended* on `it opens on the remembered page: page 0, wanted 7` and `session
  restore is not verified`. The correction was only visible above the noise it was meant to
  correct, and these harnesses are redirected to a file and read from the tail --- which is why
  `live_output.py` exists at all.

  The driver now reads that check's verdict out of the transcript, skips the recorded-file
  comparison and the remaining three phases *by name*, and ends with `[FAIL] session restore was
  not tested: <fixture> has too few pages to reach page 7`. On `text-base14.pdf`: eleven failures
  to one, three launches not made, exit code still 1. The good fixture is unchanged --- 19 checks,
  same name list, exit 0.

  The check's name is duplicated into `session_check.py` to make that possible, which is a
  coupling rather than an assertion, so its *absence* from the transcript is reported as a failure
  of the script rather than read as "the fixture is fine". Proved by mutation: renaming it turns a
  green run red with *"this script cannot find a check named ... it has been renamed in
  sessioncheck.ts, so the too-short-fixture path below is now dead code"*. Verdicts are matched by
  splitting on the `[FAIL]` label rather than by column, because `Report` pads names to a fixed
  width and a parser that encodes the padding breaks silently the day a name grows past it.

- **The render deadline was not a deadline on Windows, and said it was.** `workers::kill_pid` ---
  the only thing that bounds a request that never comes back --- was `#[cfg(not(unix))] fn
  kill_pid(_pid: u32) {}`, and its own comment had named the trigger in advance: *"if a worker
  ever starts on Windows this has to become `TerminateProcess`, or the deadline silently stops
  being one."* Workers started there the day before.

  It did not merely fail to enforce. `kill_overdue` counted the pid, set the killed flag, and
  printed *"worker killed for exceeding its deadline"* --- so the caller got a deadline error, the
  log recorded a kill, and the process went on holding a hung PDFium render forever: one leaked
  worker per hung document, with a line in the log saying otherwise. The three tests covering the
  mechanism were `#[cfg(unix)]` too, so the platform where the guard had stopped working was also
  the platform where nothing tested it, and the suite was green.

  Now `OpenProcess` + `TerminateProcess(sandbox_win::KILLED_EXIT)`, using a distinct exit code
  because Windows has no signal number to carry "did not choose to exit". The three tests are
  un-gated, with a `ping`-based sleeper --- Windows has no `sleep`, and `timeout.exe` exits
  immediately under a redirected stdin, which would have made every assertion pass for the wrong
  reason. Proved by mutation: restoring the no-op turns
  `the_supervisor_kills_the_process_holding_an_overdue_call` red with `ExitStatus(0)` and takes
  the suite from 0.06 s to 5.08 s, the sleeper's own lifetime.

- **No tile was ever painted on Windows, and nothing reported an error.** `tiles.ts` fetched
  `tile://localhost/...`, which WebView2 cannot resolve --- it registers no custom URI schemes,
  so Tauri serves them at `http://tile.localhost/...` there. PDFium bound, the document parsed,
  pages laid out, the frame loop ran and every coverage check read `sharp=0.0%`: everything
  that does not need a tile worked. The origin now comes from Tauri's own `convertFileSrc`,
  for the origin *only* --- handed a whole path it percent-encodes the separators the server
  splits on. `shell.html` carried a second copy of the URL and now derives it the same way.
  The CSP names `http://tile.localhost` beside `tile:`; it already named `http://ipc.localhost`
  beside `ipc:`, so the convention was known and had been applied to one scheme and not the
  other.

- **`progressive::bind` was public so probes would not copy it, and lived where they could
  not reach it.** The doc comment said as much --- *"public so a probe can exercise this
  binding rather than a copy of it"* --- while the function sat in `worker_child`, which is
  `#[cfg(unix)]`. So on Windows the one shared binding was the one unreachable, and three
  probes had already written their own. Moved to `progressive`, re-exported from
  `worker_child` so `fdpass_probe.rs` still imports it beside the genuinely macOS-only
  `apply_sandbox`.

- **`viewer_check.py` discarded a passing run's stderr**, so every warning the app prints was
  invisible to exactly the runs that succeed. Adding the uncontained-backend `[WARN]` and
  then seeing a full-marks Windows run show no trace of it is how this surfaced; the first
  reading was that the warning had not fired. `[WARN]` lines are now echoed on a passing run,
  and nothing else is, so the webview's ordinary teardown noise does not come back with them.

- **`backend-probe` did not link off macOS**, which is what broke `npm run tauri build` on
  Windows. It is now a thin entry point over `backend_probe/imp.rs` that refuses off macOS ---
  the shape `fdpass_probe.rs` already used, and the honest one: every claim the probe makes is
  about a worker backend that cannot exist there, so it exits 2 with a reason rather than
  printing a table nobody should read.

- **A tile or thumbnail that arrived after its owner was destroyed leaked its bitmap.**
  Three paths, all the same shape: teardown withdraws everything outstanding, but withdrawal
  races the renderer, so anything that had already finished still lands. The scroller pushed
  it onto an arrival queue drained by a frame loop that no longer runs; the page strip kept
  it in a map that had just been emptied, including a *copy* it makes of a bitmap the
  scroller owns. An `ImageBitmap` is GPU-backed and released only by `close()`, so each of
  those is memory held until the process exits, once per tile in the race window.

  The guard is `src/lib/lifetime.ts`, and the reason it is a class rather than the boolean
  the earlier fixes used is that a boolean would not have fixed these: a continuation that
  sees a dead owner and merely returns early leaks exactly as much as one that queues the
  bitmap. `Lifetime.claim(live, dispose)` makes the disposal a required argument, so the
  guard cannot be written without saying what happens to the value it declines. The viewer's
  own `destroyed` flag is now one of these, unchanged in behaviour.

  Nine mutations, each caught by the test aimed at it --- except one that survived and was
  the point of running them: the strip's borrow path was unreachable from its fixture, whose
  `placeholderFor` returned null, so the disposal there had no test at all until a fixture
  that actually borrows was written. Every disposal test is paired with a control asserting
  a live arrival is still kept, since an owner that closed everything would pass the first
  set perfectly while drawing nothing.

- **A document closed while a text extraction was outstanding left the old viewer's frame
  loop running.** `destroy()` set no flag and `wake()` restarted the loop unconditionally, so
  a text load landing after destroy --- guaranteed, since the loader never rejects ---
  resurrected the dead viewer: fresh tile requests for a closed document, re-woken by its own
  backoff every 8 s for the life of the process, and status callbacks overwriting the *new*
  document's header and sidebar. A `destroyed` flag is now set first in `destroy()` and
  checked at the single choke point every continuation reaches.

- **"Select all on page" issued an unbounded stream of extraction calls on a page whose text
  could not be read** --- the retry re-entered on every resolution and a failed load caches
  nothing, so each iteration was a fresh IPC invoke, surviving destroy and document close.
  The continuation now re-enters only when text actually arrived.

- **A file that failed to open tore down the reader's current document.** The error path
  cleared the title --- which unmounts the document body --- even when the failure happened
  before anything about the current document was touched, leaving a live viewer on detached
  DOM and a header describing a document with no body under it. The cleanup now runs only if
  the old document was already released, and the header state is cleared together with the
  title, never separately.

- **Copying a selection spanning a page whose text could not be read put a silently
  incomplete string on the clipboard** --- the exact bug the copy path documents itself as
  existing to prevent. Completeness is now re-checked after the loads; a copy that cannot be
  completed reports instead of writing, as does a clipboard that refuses the write.

- **A post-fork descriptor shuffle could close the descriptor it had just installed.** `dup`
  returns the lowest free descriptor, so the scratch copy could land on a target number
  (document on 3, tile on 5, hole at 4), where the second `dup2` overwrote it and the cleanup
  then closed the installed copy --- a worker dying on a closed fd, intermittently, as a
  function of the parent's fd-table holes. Scratch descriptors are now identified against the
  same table that drives the installs, so the two cannot drift.

- **The page strip kept fetching after the document closed; Cmd-O could stack file dialogs;
  a pending find-debounce could fire at the newly opened document; a `tile://` request posted
  after the render service stopped left the webview's fetch pending forever.** Four small
  teardown holes, each now closed where the state lives.

- **A tile that failed was re-requested every frame, forever.** `Scroller.request()` runs on
  each frame and issues any wanted tile that is neither resident nor in flight; the failure
  paths deleted the in-flight entry and recorded nothing, so the next frame asked again — and
  the frame loop could not idle out, because the re-issued requests kept `pendingWork` above
  zero. Under the worker backend each attempt costs a `kill`, a fresh `fork`/`exec` and a full
  re-parse, so a page that faults deterministically had the application spawning and killing
  sandboxed processes at display cadence for as long as the document stayed open, with nobody
  touching the machine.

  `docs/THREAT-MODEL.md` §7 stated this was "bounded by the reader's own requests". It was
  not: the reader makes one and the frame loop made the rest, which is a bound written in
  prose and enforced nowhere. Now a per-request exponential backoff (250 ms doubling to 8 s),
  cleared only by a reader's own zoom, rotation or inversion — nothing on the frame path
  clears it. `Viewer` schedules exactly one wake per backoff so a transient failure still
  recovers, and `nextRetryMs` deliberately reports nothing for a request already due, or that
  wake would rebuild the busy loop one level up. `thumbnails.ts` gets the same treatment
  through its own `failed` set, and `RunStats`/`ViewerStatus` now carry a `failed` count, so a
  renderer erroring on everything no longer looks identical to one that is merely slow.

- **Two document opens could interleave, closing a live document and leaving two viewers on
  one element.** `openPath` suspends three times while mutating `openDoc`, `viewer`, `sidebar`
  and `openPathName`, and two of its six callers fire it without awaiting anything — the drop
  handler and the `OPEN_EVENT` listener. Double-clicking a second PDF while a large one was
  still opening had each call read the *other's* freshly-set `openDoc` as its `outgoing` and
  release the document the other was about to build a viewer on, while the second
  `new Viewer` overwrote the first without destroying it: two sets of live `wheel`, `keydown`
  and `pointerdown` listeners on the same element, and two sidebars in the DOM, since
  `Sidebar` appends rather than replacing. Opens are now serialised through a promise chain.
  The body no longer awaits `firstPaint()`, so a queued open waits for the real work and not
  for a one-second grace period that has nothing to do with it.

- **A tile request was bounded by the wire format and not by the mapping it is delivered
  through.** `protocol::parse` accepted any size up to 65535², and the refusal happened in the
  worker *after* `progressive::render_tile` had allocated `width × height × 4` and drawn into
  it — about 17 GB at the maximum, inside the process holding the attacker's document. Now
  refused at parse time. `doc`, `x` and `y` are range-checked there too rather than `as`-cast:
  a negative document number silently became `u32::MAX` and an origin past `i32` wrapped to a
  plausible one, which is the quiet coercion that parser refuses everywhere else.

- **The recursive graph walks in the print path had no depth bound.** `sweep::references` and
  `print::forget_in_object` run on a document we did not write, in the **app process**, and
  recursed until the stack ran out. Both now stop at `sweep::MAX_NESTING` (256) and **refuse**
  rather than truncating: a mark-and-sweep that stops early has an incomplete reachable set,
  so it would delete live objects and hand back a document that still parses and has holes in
  it. `collect` and `drop_pages` propagate that as an error.

- **⌘O was advertised in the palette and reached no handler at all, and ⌘P turned the page as
  well as printing.** The palette's shortcut labels were hand-written strings sitting twenty
  lines from the handlers that implement them, with nothing checking the two agreed — which
  `App.svelte` said out loud and called "a real gap and a small one". Both defects were in
  that gap: no ⌘O branch was ever written, and the viewer's `p` arm tested the key without the
  modifier, so it sat below the ⌘-guarded arms and caught ⌘P on the way past. Bindings are now
  data in `src/lib/keys.ts`; the label is *rendered from* the same modifiers `matches` tests,
  so the two cannot drift, and the table is covered by `keys.test.ts`.

- **`Queue` tracked one in-flight request while the pool ran several.** `inflight` was an
  `Option<(u64, CancelToken)>`, correct when the render service was one FIFO thread and wrong
  once the worker backend served the same queue from `pool + 2`: a second claim evicted the
  first, so withdrawing the older of two concurrent renders matched nothing in either table
  and cancelled nothing. The worker's own copy of the queue still stopped the render, so
  nothing looked broken — a safety net that could not fire. Now a `HashMap`, with `release`
  keyed on the request.

- **Closing a document left its renders in the queue.** `Scroller.destroy()` withdrew only
  tier-2 requests, and only when the `cancel` variant flag was set — a flag that exists so the
  benchmark can measure what withdrawal is worth. A teardown is not a variant: everything
  outstanding is now withdrawn unconditionally, so the outgoing document's tiles stop sitting
  in front of the first page of the file the reader has just opened. The placeholder arrival
  queue is closed with it.

- **`copySelection` issued one extraction per selected page at once.** A selection dragged to
  the end of the 775-page corpus named 775 pages and `Promise.all` put all of them on the FIFO
  queue that also draws the page in front of the reader — the cost `prefetchText` and
  `TextCache` both go out of their way to avoid, re-entering through the copy path. Chunked at
  16, rather than capped: a copy has to be complete.

- **`backend-probe` had a vanishing check of its own**, the second found in a day and by the
  same method. "The page asked for is one a wrong page number would betray" disappeared on
  one-page documents rather than skipping, and the only trace was the name count moving from
  32 to 31 between corpora. All six now report 32.

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
