# BUILD.md --- tpdf

How to get a clean clone building, what the quality gates are, and how a release is cut.

Durable project knowledge lives in [`AGENTS.md`](AGENTS.md); the architecture and roadmap
are in [`docs/PLAN.md`](docs/PLAN.md). This file is only the mechanics.

---

## Prerequisites

| Tool | Notes |
|------|-------|
| Rust (stable, via rustup) | `rustup update`. Do not install a second toolchain through Homebrew. |
| Node 20+ and npm | |
| Python 3.9+ | Only for `scripts/`; not a runtime dependency of tpdf. |
| `uv` | Only for the test fixtures that need `fontTools` or `pyhanko`. |
| `qpdf` | Optional. A structural oracle for spike 0.4; not needed to build or run. |

---

## Clean clone

```
npm install
scripts/fetch_pdfium.py
```

`vendor/pdfium/` is gitignored --- a 7.7 MB binary does not belong in the object store --- so
**a fresh clone has no PDFium and every binary fails to bind at runtime until the fetch
script has run.** The script downloads the pinned upstream build, verifies its SHA256
before extracting anything, and refuses a V8 asset.

Verify an existing install without touching the network:

```
scripts/fetch_pdfium.py --check
```

The pin is `chromium/7881`, which is the build every Phase 0 measurement in `AGENTS.md`
and `docs/PLAN.md` was taken against. Bumping it means editing `TAG` and the whole `PINS`
table in `scripts/fetch_pdfium.py` together, then re-running the two checks that a digest
cannot stand in for:

Run these from the repository root, after generating the fixtures below. Each exits
non-zero on failure.

```
# The FPDFPageObj_Destroy ownership segfault. Case `c` (leak) must pass; if case
# `a` (destroy) ever stops crashing, the upstream bug is fixed.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin remove-probe -- \
    testdata/text-truetype.pdf c

# The V8 and XFA symbol scan. This mode reads the library rather than binding it,
# so --lib is required even though every other mode defaults it.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin worker-bench -- \
    testdata/text-heavy.pdf --mode engine --lib vendor/pdfium/lib

# Progressive rendering still agrees with the safe path, byte for byte. Slow:
# roughly 20 s, because the point is the page that takes seconds to render.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin progressive-probe -- \
    testdata/vector-heavy.pdf --mode identity --slices 0

# Character boxes still land on the ink they describe. Run it on a *small* text
# fixture: on testdata/text-heavy.pdf the wrong convention also scores 70%, so
# that page cannot discriminate and the probe fails rather than reporting a pass.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin text-probe -- \
    testdata/text-marked.pdf --mode align

# The outline walk terminates, resolves and refuses. Run BOTH: the hostile
# fixture proves the bounds fire, and the ordinary one proves they do not fire
# when they should not, which is the half that catches a walk bounding
# everything.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin outline-probe -- \
    testdata/outline-simple.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --bin outline-probe -- \
    testdata/outline-hostile.pdf --mode check

# The worker boundary is still transparent: the two backends must agree byte for
# byte on tiles, geometry, text, search and outlines, and a worker killed out of
# the OS process table must be replaced by one serving the same document. Run it
# on vector-heavy as well as a text fixture -- it is the only corpus whose render
# is slow enough for the withdrawal and drain checks to apply, and on every other
# one they report [SKIP] with the reason. vector-heavy is the run to read: 41
# check names, 1 skipped.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin backend-probe -- \
    testdata/vector-heavy.pdf
```

The count that matters there is the count of **names**, not the split between passed and
skipped: the split moves with the corpus and with a thumbnail's timing, and chasing a
documented split back to its value is how a condition that keeps a check honest gets
deleted. What holds on every corpus is that all 41 names appear --- diff the name sets
across two fixtures rather than comparing their totals, which is what caught a check that
had stopped existing on one-page documents.

**Do not run it under `caffeinate`.** `caffeinate -d -u <utility>` `exec`s the utility in
its own process and leaves a helper behind as that process's *child*, and every observation
of a worker here comes from the process table. The probe filters on the worker's argv for
exactly this reason, so it is now correct either way --- but the same trap is waiting for any
new check that counts children, and it presents as a stable, reproducible failure that reads
like a real defect. `AGENTS.md` has the incident.

The worker pool has its own measurement rather than a check, because what it is for is a
number. It is not part of the bump checklist above --- run it when the pool, the thread
count, or the tile path changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --bin pool-bench -- \
    testdata/vector-heavy.pdf --rounds 4 --sizes 1,2,4,6,8
```

It interleaves the sizes across rounds and compares pairwise within a round, discards round
0, and reports the cold regime (the pool growing) separately from the warm one. Quote two
runs, not one: the four-worker figure moves several percent between runs while six barely
moves, and one run would present that as a measurement.

The other half of the same subject --- what a grown pool costs to hold and what retiring it
gives back --- is a second mode. Run it when the idle timeout, the reaper, or the number of
workers kept changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --bin pool-bench -- \
    testdata/vector-heavy.pdf --mode retire --rounds 4
```

It reports the pool's footprint at three points and, per round, a warm screenful against
the first one after a retirement. `--idle-ms` sets the timeout it runs at (4 s by default,
so a round does not take half a minute); the app's own default is 30 s. The wait for a
retirement is **bounded and fails the run** if it does not happen --- without that, the
second column would quietly be a warm screenful wearing a cold label, which is a number
that looks entirely reasonable.

Two notes on why these are written out in full. The binary names are **hyphenated**, and
`--bin remove_probe` fails as "no such target", which reads like a missing binary rather
than a wrong name. And `remove-probe` with no case argument defaults to case `a`, whose
whole purpose is to segfault --- so the obvious invocation of the regression check crashes
by design and looks like the bump broke something.

The third check is why the progressive path restates `FPDF_ANNOT`,
`FPDF_REVERSE_BYTE_ORDER` and `FPDFBitmap_BGRA` by value: `pdfium-render` does not
re-export them, and a bump that changed any of them would silently alter every tile. The
run compares progressive output byte-for-byte against the safe path, so it fails if one
does.

### Test fixtures

`testdata/*.pdf` is gitignored and generated. Nothing it produces may be committed or
redistributed --- `make_text_pdf.py` embeds a system font.

```
uv run --with fonttools testdata/make_text_pdf.py testdata
python3 testdata/make_hostile_pdf.py testdata
python3 testdata/make_vector_pdf.py testdata/vector-heavy.pdf
python3 testdata/make_vector_pdf.py testdata/vector-multi.pdf 200000 12
uv run --with pyhanko --with cryptography testdata/make_incremental_pdf.py testdata
python3 testdata/make_outline_pdf.py testdata
python3 testdata/make_rotated_pdf.py testdata
```

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

---

## Quality gates

```
scripts/gates.py
```

That is the whole checklist. **`scripts/gates.py` is the definition of the gates, not a
description of them** --- it holds the commands with their flags, and this file deliberately
does not repeat them. `AGENTS.md` records why: a checklist weaker than the gate it exists
to satisfy is worse than no checklist, and the usual failure is a hand-copied command that
quietly loses a flag. Removing the copy removes the drift.

To see what will run, ask the script rather than this document:

```
scripts/gates.py --list
scripts/gates.py --gate clippy      # run one, repeatable
```

Every gate runs even after an earlier one fails, so one pass reports everything that is
wrong. The exit code is non-zero if any failed.

Two of them are worth understanding rather than just running:

- **`cargo test --locked` is two gates in one.** Besides the unit tests it fails on a
  `Cargo.lock` that was not committed after a `cargo update`, and it compiles the test
  targets, which is where `--all-targets` clippy findings surface. Coverage now reaches
  most of the backend --- the request queue and the `tile://` parser, the worker protocol
  and the pool, rendering, text, search, outlines, printing, session and sweep --- with
  `npm run test` doing the same for the front-end logic beside it. What it deliberately
  leaves to the harnesses under `scripts/` is everything that needs a live webview --- and a
  Windows run is now one of those, `viewer_check.py` having passed there on 2026-07-29,
  rather than something nothing covers at all. What no gate covers is paper: a print job is
  checked by reading its bytes back with PDFKit, a parser independent of the writer but still
  not a printer.
- **`cargo build --locked --bins` is the only gate that links anything.** clippy stops at
  metadata, and `cargo test` links each `[[bin]]` with its `main` replaced by the test
  harness's, so a symbol reachable only from `main` is dropped as dead code rather than
  reported as missing. Without this gate a 7/7 sweep sat beside a failing
  `npm run tauri build`; see the trap.
- **Wrap a batch of benchmark runs in `caffeinate -du`.** `scroll_bench.py` holds one for
  its own lifetime, but the gaps between runs --- and any headless bench running alongside
  it --- are unprotected, and a session that locks mid-batch fails the next frame-rate run
  outright. A locked macOS session cannot be unlocked from a script by design, so this is
  preventable and not recoverable.

- **The `pdfium` gate is a pin check, not a build step.** It fails if `vendor/pdfium` is
  missing or is not the pinned build --- which is the difference between a benchmark that
  means something and one that does not.

### There is no remote CI, deliberately

The project is pre-release and developed on one machine. A GitHub Actions workflow would
add macOS-runner minutes and a second place for the gate list to live, in exchange for
catching nothing that `scripts/gates.py` does not catch locally first.

When CI is added --- the natural trigger is the repo going public, or a second contributor
--- **the workflow should invoke `scripts/gates.py`**, not re-list the commands in YAML. That
keeps the checklist and the gate the same object rather than two things that happen to
agree today.

### Windows runs the viewer. It is still not supported.

`scripts/gates.py` reports **8/8 on `x86_64-pc-windows-msvc`**, and on 2026-07-29 a Windows
build **opened documents and passed the full functional check**. A clean clone bootstraps
with no changes --- `npm install` and `scripts/fetch_pdfium.py` both do the right thing, the
fetch script selects the `win-x64` asset and verifies its digest.

`viewer_check.py` runs unmodified: `webview_guard` already returns early off darwin, and
WebView2 needs no bundle identity, so a plain `target/release/tpdf.exe` is enough where macOS
needs an `.app`. Two things about the invocation, both of which present as something other
than what they are. The binary must come from `cargo build --release --features
tauri/custom-protocol` or the window shows *"localhost refused to connect"* (see the trap ---
the profile is not what embeds the frontend). And **pass it as a backslash path**:
`CreateProcess` does not accept a relative forward-slash path, so
`src-tauri/target/release/tpdf.exe` raises `FileNotFoundError: [WinError 2] The system cannot
find the file specified` for a file that is plainly there, from inside Python's `subprocess`
rather than from anything in this repository.

Four corpora, every one reporting the **86 check names** that are the invariant, with splits
inside the ranges the table above records:

| fixture | ran | skipped | failed |
|---|---|---|---|
| `outline-simple.pdf` | 81--82 | 4--5 | 0 |
| `outline-hostile.pdf` | 81 | 5 | 0 |
| `rotated-90.pdf` | 75 | 11 | 0 |
| `vector-heavy.pdf` | 52 | 34 | 0 |

Re-run 2026-07-30 with pre-spawning live, since that changes the app's own behaviour --- every
open now consumes a warmed process and starts another. All four green, no `[WARN]`, 44 modules
at peak with no `pdfium` among them over 27--978 samples. `outline-simple` reported 82/4 that
time against 81/5 before: the **86 names** are what is invariant, and one of them stopped
skipping. A split that moves is information; a name that disappears would not be.

Rendering, scrolling, zoom, pinch, view rotation, text selection, search, the palette, the
accessibility tree, the outline sidebar, thumbnails, inversion and the print command's
refusals all behave as they do on macOS.

**What is missing is containment, not function.** `sandbox_init` is SBPL and macOS-only, so
`Worker::spawn` refuses off macOS and `Backend::default_here()` falls back to
`Backend::InProcess`. A Windows build parses attacker-controlled PDF **in the app process**,
which is exactly what `AGENTS.md` and `docs/THREAT-MODEL.md` forbid. **It fails open**:
`Worker::spawn`'s refusal is asserted by tests, but only a caller that asks for
`TPDF_BACKEND=worker` ever reaches it --- the default selects in-process and renders perfectly
happily, so nothing refuses. A port owes a real containment answer (job objects, a restricted
token, a separate desktop) before Windows can ship. That, and not the viewer, is now the whole
gap.

It is at least **visible**: the uncontained default records `render::UNSANDBOXED_MARK` on the
startup timeline and prints `[WARN] no sandbox on this platform ...` on stderr, and
`viewer_check.py` echoes `[WARN]` lines even on a passing run --- it previously showed stderr
only on failure, which hid the warning from exactly the runs that succeed. Visibility is not
containment, and a mark is deliberately not a refusal: refusing would make Windows useless
rather than uncontained, which is a decision rather than a defect.

**And the fix is now measured rather than guessed.** `cargo run --release --bin
win-sandbox-probe` runs six containment rungs, each rendering the same tile in a re-exec'd
child and compared pixel for pixel against an in-process render, with an uncontained child as
the control over the harness itself:

```
bare        yes   yes   0                       control: what Windows does today
job         yes   yes   0                       memory cap, one process, kill-on-close
lowil       yes   yes   0                       job + low integrity level
noprivs     yes   yes   0                       diagnostic: privileges dropped only
sidonly     no    -     STATUS_DLL_NOT_FOUND    diagnostic: restricting SID only
restricted  no    -     STATUS_DLL_NOT_FOUND    job + restricted token
```

A **job object plus low integrity** renders byte-identically while denying writes to the user
profile and `OpenProcess` on the parent. It does not deny *reads* --- an integrity level
governs writes --- so the child is handed its document and its output as inherited handles
rather than paths. A restricting SID is stronger and unreachable directly: the loader's own
reads are denied and the child dies before `main`, which needs Chromium's initial-token /
lockdown-token handover to get past.

**A worker uses it now** (2026-07-29). `Worker::spawn` builds a contained child on Windows, and
`worker-probe` is the standing proof:

```
cargo build --release --bin worker-probe
./src-tauri/target/release/worker-probe.exe testdata/text-base14.pdf
```

**11/11 checks, 1 not applicable**, on `text-base14`, `text-cid`, `vector-heavy` and `rotated`
--- tiles **pixel-identical** to the in-process render, plus text extraction, outlines and
search across the boundary. The not-applicable one is the parent's memory poll: macOS has no
rlimit and polls as a substitute, while here the job object caps commit in the kernel, so there
is nothing to poll. It prints `[SKIP]` with that reason rather than vanishing.

Two things that check does *not* cover, deliberately, because a `cargo test` child is the test
harness and never answers: pipe **direction** and content. Both are the probe's job, measured
by mutating the pipe pair and watching the probe go red --- see the trap *A test whose child
never answers cannot see the pipes being crossed*.

**Windows no longer fails open** (2026-07-29). `Backend::default_here()` selects workers there,
and the evidence is external rather than a mark of our own:

```
python scripts/win_modules.py <pid>          # on its own
python scripts/viewer_check.py <exe> <pdf>   # samples it throughout a real run
```

`viewer_check.py` now launches the app rather than blocking on it, reads the loaded module list
from outside the process while a document is open, and takes the **union** of its samples ---
the parser is mapped only while a document is open, so a single look could miss it in either
direction. The module count is printed beside the verdict, because an enumeration that read
*nothing* reports "not mapped" exactly as containment does; a peak of zero is reported as a
broken observation, never as a pass.

Run **before** the flip it reported `[FAIL] the app process mapped the PDF parser, 47 modules
at peak`. That control is why the pass afterwards means anything. After: four corpora green
with unchanged ran/skipped splits, no `[WARN]`, 44--45 modules at peak, no `pdfium` among them.

That line is printed *outside* the 86 check names on purpose --- those are `viewercheck.ts`'s
and are the cross-platform invariant, and adding a Windows-only name to that set would make the
two platforms look divergent when they are not.

#### Pre-spawning, and what it is worth here

Implemented 2026-07-30, so both platforms start a worker before a file is chosen. Only the
handover differs. A macOS parent sends a descriptor as `SCM_RIGHTS`; a Windows parent
`DuplicateHandle`s the document section **into the running child's handle table** and then sends
a `Handover` line naming the number it wrote. Writing into a low-integrity child is the direction
integrity levels permit, so this crosses the boundary for the same structural reason the macOS
one does. `Handover` is deliberately not a `Request` variant --- a handover is legal exactly once,
and keeping it out of the request vocabulary makes a second one unsayable rather than something
the child has to refuse.

```
cargo run --release --bin prespawn-bench -- --rounds 6 \
    text-base14.pdf text-truetype.pdf text-cid.pdf vector-heavy.pdf
```

| fixture | size | spawn now (min/med/max) | pre-spawned | saved |
|---|---|---|---|---|
| `text-base14.pdf` | 888 B | 10.10 / 10.38 / 10.62 ms | 0.69 ms | **+9.64** |
| `text-truetype.pdf` | 20 KB | 8.70 / 8.87 / 9.75 ms | 0.44 ms | **+8.42** |
| `text-cid.pdf` | 22 KB | 8.51 / 8.99 / 9.46 ms | 0.45 ms | **+8.55** |
| `vector-heavy.pdf` | 2 MB | 75.09 / 75.78 / 76.54 ms | 66.77 ms | **+9.15** |

**The shape of the saving is not the macOS one, and that is the finding.** There the interval
splits into a ~6.6 ms floor plus ~7.4 ms of system-font enumeration paid only by documents that
embed nothing. Here the saving is nearly constant at ~9 ms and the font component is **~1.4 ms**
--- `text-base14`, which embeds nothing, costs 10.38 ms against 8.87/8.99 ms for the two that do.
So on Windows pre-spawning buys almost entirely the fixed floor: `CreateProcess`, the loader,
mapping `pdfium.dll`, the token and the job.

Read that 1.4 ms as a between-document comparison, not as the warm/no-warm control. The bench's
own `a warmed worker does not pay the font walk` check needs `text-heavy.pdf`, which this machine
has not generated, and it `[SKIP]`s with that reason rather than quietly not running.

### `backend-probe` on Windows, and the defect it found in itself

```
cargo build --release --bin backend-probe
./src-tauri/target/release/backend-probe.exe testdata/text-base14.pdf
./src-tauri/target/release/backend-probe.exe testdata/vector-heavy.pdf
```

| fixture | passed | skipped | failed |
|---|---|---|---|
| `text-base14.pdf` | 37/41 | 4 | 0 |
| `text-cid.pdf` | 37/41 | 4 | 0 |
| `outline-hostile.pdf` | 38/41 | 3 | 0 |
| `vector-heavy.pdf` | 40/41 | 1 | 0 |

The skips are a slow enough render for the three withdrawal checks (only `vector-heavy` has one)
and a second page to confuse a page number with. The boundary, the pixel comparisons, capacity,
crash restart, replacement, retirement, close, descriptor return **and the spare's lifetime** all
pass. Its Windows primitives are Toolhelp for the module list and the process table,
`GetProcessHandleCount` for descriptors, and `TerminateProcess` for a hostile kill from outside
the pool --- deliberately not `Contained::kill`, since the pool has to notice a death it did not
cause.

This is also where the Windows spare is proved end to end, and the detail says more than the
count: `at open: pool [18840], children [2672, 18840], spares [2672]` --- a warmed child exists,
is excluded from the pool rather than miscounted into it, and `opened with 1` beside it keeps the
laziness claim. `a spare does not outlive the service that started it` reports
`its 1 spare process(es) [58096] went with it`.

**It first reported 34/41, and the two failures were the probe's own.** They said a burst grew
the pool to six and 1.2 s into a 4.0 s idle timeout one was left, with **144 handles with one
worker, 144 grown, 144 retired** beside it --- and five extra workers cannot cost zero handles.
Two independent observations agreeing, and the diagnosis drawn from them (created, used and
**destroyed rather than pooled**) was recorded here as an open defect for a day. It was wrong.

Both numbers were honest; neither could say *when* it was taken. `settled_descriptors` waits up
to five seconds for a pre-spawned spare to appear, Windows has none, and the verdict of that wait
was discarded --- so it spent its whole bound on every call, which is longer than the idle timeout
the phase runs at. The instrument retired the pool and then measured it. One worker of six and a
lean handle count are precisely what a correct pool looks like five seconds after a burst. The
pid clause is now asked for only where a spare can exist, and a wait that expires says so with a
`[WARN]`. Nothing in `workers.rs` changed.

Do not "fix" a failure here by relaxing a check --- but do check the clock before believing one.
The pre-fix run remains the red control for both: they were observed failing, are now observed
passing, and `an idle pool is retired down to one worker` is green on both sides, so retirement
was never the thing that broke.

**A second check went red the day pre-spawning landed, and it was the same shape.** `closing
gives back every descriptor opening took` reported *137 quiet, 145 with it open, 142 after
closing it* --- five handles, one spare's worth. Nothing leaked: an `open` consumes the warmed
spare and starts a replacement on another thread, so a raw sample includes one spare or not
depending on how far that thread has got. macOS forks and wins that race; Windows creates a
process, a token, a job and a fresh map of `pdfium.dll`, and does not. Its three samples now go
through `settled_descriptors`, which exists for exactly this and predated them. See the trap ---
the lesson is that passing on one platform was evidence about that platform's timing.

**Of the "four probe binaries that refuse to act as a worker off unix", one did.** That list was
in this file and in `AGENTS.md` for two days and was wrong about two of its four entries, in the
direction a list written by reading always errs --- see the trap. What was actually true:

- `pool-bench`, `prespawn-bench` --- a real `#[cfg(unix)]` gate on the `--render-worker` re-exec,
  dating from before `worker_child` compiled on Windows. Worth understanding before copying it:
  each binary re-execs *itself* as a worker, so gating that made the benchmark **unrunnable**
  rather than degraded. Ported 2026-07-30, along with the hardcoded library path.
- `tile-bench` --- **never refused anything.** It ran on the first try and failed at
  `LoadLibraryExW` on the hardcoded path. Ported the same day; numbers below.
- `worker-bench` --- the one genuine refusal, and its stated reason is accurate: it carries its
  own POSIX worker implementation, fd passing and SBPL profile bisection included, and shares no
  mechanism with the job-object model. That one needs its own spike, not a port.

**Numbers are macOS arm64 unless a Windows one says so.** The pre-spawn table above and the
tile-bench section below are the sets taken on Windows and are labelled as such; everything else
in this file and in `AGENTS.md` still is not, and the platforms are far enough apart --- a ~1.4 ms
font walk against ~7.4 ms --- that carrying a figure over is a guess, not an estimate.

### `tile-bench` on Windows, and what the render constants cost here

```
cargo build --release --bin tile-bench
./src-tauri/target/release/tile-bench.exe testdata/vector-heavy.pdf --mode single --rounds 4
./src-tauri/target/release/tile-bench.exe testdata/text-base14.pdf  --mode single --rounds 4
```

It needed two fixes and neither was a refusal: the hardcoded `vendor/pdfium/lib` (on Windows that
directory exists and holds the *import* library, so it fails at `LoadLibraryExW` rather than at a
missing path --- see the trap), and `peak_rss_mb`, which returned `NaN` off unix. That is
`GetProcessMemoryInfo`/`PeakWorkingSetSize` now, keeping the `NaN`-on-failure contract because a
zero would read as "PDFium allocated nothing". A working set is trimmed under memory pressure, so
it can read below the peak *commit* the same run reached; it is still the right counterpart to
`ru_maxrss` for this question, since both are about pages actually held.

**§9's architectural conclusions hold on Windows, and every constant behind them is worse.**
`vector-heavy` is generated by the committed `make_vector_pdf.py` against the same PDFium pin, so
this row is a fair comparison of *constants* --- across different machines, which is the useful
framing here rather than a CPU verdict:

| | macOS arm64 (`docs/PLAN.md`) | Windows (2026-07-30) |
|---|---|---|
| 256² tile of the A0 page | 0.98 s | **1.35 s** |
| that tile as a share of a full render | 4.3% | **3.8%** |
| full page, 1× | 22.8 s | **35.1 s** |
| full page, 2× | 48.4 s | **88.3 s** |
| fixed cost per render *call* | ~1 s | **~1.3 s** |

So the shape is the same on both --- PDFium culls spatially, a tile is a few percent of a full
render, and there is a hard per-call floor that does not shrink with the request. The magnitudes
are **1.5--1.8× worse**, and the floor about a third worse. The practical consequence: a latency
budget written against the macOS floor is optimistic on Windows by roughly that much, and
`docs/PLAN.md` §4's four consequences now rest on two platforms rather than one.

Independently cross-checked on the same machine before being believed: `backend-probe` measured a
**1536 ms** 512² render of the same document through the worker, against tile-bench's 2203--3073 ms
for that tile size. Same order, differing about as much as a centred tile and a placed one should
--- which is what says the numbers are the document's and not the harness's.

The cheap-page half confirms the asymmetry the plan bets on: `text-base14` is **flat**, 0.6--0.9
ms/Mpixel at every tile size and scale, with no per-call floor at all. Read that as a Windows
result on its own and **not** as a comparison --- macOS measured `text-heavy.pdf`, which this
machine has not generated, so the two cheap-page numbers are different fixtures.

Still macOS-shaped: `session_check.py` and `open_check.py` want `open -a` and an `.app`, and
`webview_guard` checks nothing off darwin (see the trap --- Chromium throttles occluded
windows too, so those runs were protected by nothing).

#### What the port changed, so it is not rediscovered

- `worker.rs` now compiles everywhere and refuses off macOS --- which its own module doc had
  claimed since it was written, and which was not true. 38 error sites, all POSIX:
  `std::os::fd`, `mmap`/`munmap`, `File::from_raw_fd`, `ExitStatus::signal`. `Shm` off unix is
  a type with a private field and constructors that refuse.
- `worker_child.rs` was `#[cfg(unix)]`, with the `--render-worker` argv refusing off unix
  rather than falling through. **Both are gone as of 2026-07-29.** The module compiles
  everywhere; three functions know the platform (the two mapping handovers and
  `establish_boundary`) and the rest is shared. The refusal that replaced the `cfg` is
  `establish_boundary` itself, which fails where there is no boundary to establish and does
  so *before* a document is opened --- the deleted one was never the load-bearing guard, and
  keeping it would have suggested otherwise.
- `pdfium_library_dir()` picks `bin/pdfium.dll` on Windows against `lib/libpdfium.dylib` on
  macOS, and now checks for the **library** rather than the directory. See the trap: on
  Windows `vendor/pdfium/lib` genuinely exists and holds the import library, so the old
  existence check passed and the bind failed later.
- `launch.rs`'s percent-decoding test takes a platform-shaped URL. `Url::to_file_path` wants a
  drive letter on Windows and refuses `file:///Users/...`, so the macOS fixture asserted only
  that refusal. Written that way rather than gated off Windows, deliberately --- a check that
  silently stops existing on a platform is the thing this file warns about elsewhere.

#### What running it found, which no gate could

Three defects, none of which any amount of compiling would have surfaced.

- **`npm run tauri build` failed on a tree that gated 7/7.** `backend_probe.rs` called two
  dyld symbols unguarded; clippy never links, and `cargo test` links a `[[bin]]` with `main`
  replaced, which drops them as dead code. There is a **`bins` gate** now, and it was proved
  to fail (5.7 s, debug profile) against the un-gated file before being trusted. The probe
  itself is now a thin entry point over `backend_probe/imp.rs`, refusing off macOS the way
  `fdpass_probe.rs` does --- every claim it makes is about a worker backend that cannot exist
  there.
- **Not one tile was ever painted.** `tiles.ts` fetched `tile://localhost/...`, which WebView2
  cannot resolve; Tauri serves custom protocols at `http://tile.localhost/...` on Windows. The
  origin now comes from Tauri's own `convertFileSrc`, and the CSP names
  `http://tile.localhost` beside `tile:` --- it already named `http://ipc.localhost` beside
  `ipc:`, so the convention was known and applied to one scheme and not the other.
- **`cargo build --release` is not a production build.** It produced a window showing
  *"localhost refused to connect"*: `frontendDist` is embedded by the cargo feature
  `tauri/custom-protocol`, which the Tauri CLI passes and a bare cargo build does not, at any
  optimisation level. Build through `npm run tauri build`, or pass the feature.

**The old version of this section named the wrong blockers**, and the shape of the error is
worth keeping. It listed `sanitize_rewrite.rs` and `tile_bench.rs` as the compile errors;
both were real, but clippy never reached either, because the *library* failed first. A
blocker list assembled by reading code cannot know what fails first --- that is a property of
the build graph. It also said `TPDF_BACKEND=in-process` was "the only thing that runs off
macOS", which was false: `pub mod worker;` was unconditional, so the crate carrying that
control did not compile and nothing ran off macOS at all.

---

## Running it

```
npm run tauri dev -- --release
```

**Never benchmark through `tauri dev` without `--release`.** It shells out to `cargo run`
in the dev profile, and because PDFium arrives as a prebuilt optimized dylib the result is
not uniformly slow but *selectively* slow --- PNG encoding of a tile measured 67 ms in debug
against 1.41 ms in release while the PDFium render beside it moved 1.39 -> 1.36 ms. Ratios
invert rather than merely inflate.

**Startup timing needs a bundle, not just `--release`.** Under `tauri dev` the frontend is
served by Vite over HTTP, so a startup measurement describes Vite's module graph:

```
npm run tauri build -- --bundles app
scripts/startup_bench.py target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf <file.pdf>
```

Run the executable inside the `.app` directly --- that keeps stdout and the environment,
which `open -a` does not. `--purge` gives a genuinely cold page cache and needs a sudoers
entry for `/usr/sbin/purge`.

### Which backend parses the document

Documents are parsed in a sandboxed worker process, one per document. `TPDF_BACKEND`
overrides that:

```
TPDF_BACKEND=worker      # the default on macOS
TPDF_BACKEND=in-process  # the control, and the only thing that runs off macOS
TPDF_POOL=6              # workers one document may have
TPDF_IDLE_MS=30000       # how long one may idle before it is killed
```

`TPDF_IDLE_MS` is a quantity and **zero means zero** --- retire at the first sweep. There is
deliberately no spelling for "off": a "no value" marker taken from the value's own range is
how a sentinel collides with a real value the moment the timing is right, which this
repository has already paid for once. A caller that wants no retirement asks for a long
timeout. Unlike `TPDF_BACKEND`, an unreadable value here falls back to the default rather
than refusing, because it cannot make two measurements silently incomparable --- every
harness that depends on the timeout is handed one explicitly.

Anything else is **refused before the window is created** --- one line on stderr, exit 2. The
variable exists to say which of two implementations ran, so a value that quietly selected
the other one would make any comparison between them meaningless, and `in_process` for
`in-process` is one underscore away.

The refusal is read in `run()` rather than where the backend is used, and that placement is
the whole of its value. `RenderService::start` runs in the Tauri setup hook, which `App::run`
invokes from AppKit's frames --- a panic there is non-unwinding, aborts through a backtrace
with no symbols, and races the watchdog's 30-second report about a page that never ran. A
misspelt variable would be diagnosed as an occluded window.

Two things read differently under the worker: the startup timeline has `worker spawned`
where the in-process one has `pdfium bound`, and a render can now fail because the worker
died rather than only because the document did. `backend-probe` is what says the two agree
about everything else.

A worker that dies is replaced and the request retried once, so a crash usually reaches the
reader as nothing at all --- but it is never silent in the terminal: the parent prints
`[render] document N: worker killed by signal 11; starting a replacement` on stderr, and the
worker's own stderr is inherited. Seeing that line repeatedly on one document means the
document is faulting PDFium on a page the reader keeps asking for, which is the one case a
single retry cannot make cheap.

### The "reopen its windows" dialog

A development build is killed constantly --- harness timeouts, the deliberate crash probes,
an aborted panic --- and macOS answers each abnormal exit by offering, on the *next* launch,
*"the last time you opened tpdf, it unexpectedly quit while reopening windows. Do you want
to try to reopen its windows again?"* That dialog **blocks the launch** until someone clicks
it, in front of a run that has nothing to do with whatever produced it.

`src-tauri/Info.plist` sets `NSQuitAlwaysKeepsWindows` to false, which is merged into the
bundle by tauri-bundler --- check it with
`plutil -p .../tpdf.app/Contents/Info.plist | grep Quit` after a build rather than assuming
the merge happened. An app that saves no window state cannot be asked to restore it, and
the observable is the mechanism rather than the symptom: hard-kill a running bundle and
`~/Library/Saved Application State/com.timostein.tpdf.savedState` must not appear.

This is also the right *product* behaviour, not only a developer convenience. tpdf reopens
the document you were reading, on the page you were on, through its own session file
(`session.rs`); Cocoa's restoration would be a second mechanism doing the same job, and two
mechanisms agree until they do not.

An existing machine that has already been prompted also wants the user-domain switch, since
the plist only governs bundles built after it:

```
defaults write com.timostein.tpdf ApplePersistenceIgnoreState -bool true
defaults write com.timostein.tpdf NSQuitAlwaysKeepsWindows -bool false
rm -rf ~/Library/"Saved Application State"/com.timostein.tpdf.savedState
```

### Checking the viewer

The reading surface is asserted rather than eyeballed. This opens a document in a real
webview, dispatches real wheel and key events at it, and checks fit-width, scrolling, End
and Home, the zoom ladder, a pinch, resize, text selection and copy, find-in-document, the
command palette, the screen-reader text layer, the outline sidebar, the page-thumbnail
strip, page inversion, and that the frame loop idles when there is nothing to do:

```
scripts/viewer_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

It is **not** a `gates.py` gate: it needs a built bundle and a generated fixture, neither of
which a gate run has. Run it before a release, and after any change to `viewer.ts`,
`scroller.ts` or the tile protocol.

**It requires a bundle, not merely a release build.** A raw `cargo build` binary opens a
window and never executes a line of JavaScript --- WKWebView needs the bundle identity, and
the failure is silent: no error, no crash report, a blank window. Build one with
`npm run tauri build -- --bundles app` and run the executable inside it, which keeps stdout
and the environment that `open -a` does not. The *profile* genuinely does not matter --- the
check asserts behaviour rather than timing it --- so a debug bundle is only slower.

It also requires an unlocked screen, for the reason `scroll_bench.py` does: WebKit suspends
a page whose window is not visible, so behind a lock screen the check does not fail, it
stops. Both scripts share that guard (`scripts/webview_guard.py`).

**It does not take focus.** The window appears and has to stay visible, but it will not raise
itself over what you are doing, so the run can sit in the background while you work.
`scroll_bench.py` is the exception and calls `set_focus()` on purpose --- an unfocused window
is throttled, and a frame-rate benchmark would then be measuring the throttle.

**"Visible" is stricter than "unlocked", and the guard does not check it.** A window fully
covered by another --- a full-screen terminal, a different Space --- is *occluded*, and
WebKit suspends the page exactly as it does behind a lock screen. The run then produces no
output, uses no CPU, and stays alive, which reads as a hang in whatever was last changed.
Set `TPDF_RAISE=1` to raise the window when there is nowhere visible to put one:

```
TPDF_RAISE=1 scripts/viewer_check.py <binary> testdata/text-heavy.pdf
```

The watchdog identifies **any** page that never executed, whatever the reason --- an
occluded window and a raw unbundled binary produce exactly the same silence. Every spike
entry point starts by asking Rust for its path, which records a `webview alive` mark; a run
that times out without one is told in full that the page never ran a line of JavaScript.
Confirm independently with `TPDF_STARTUP=<file> <binary>`, which fails the same way in 30 s
and settles "environmental or mine" in one command. Results otherwise print as they are
produced, so a run that stops partway names the last check it completed.

Two of its assertions carry the weight, and both tie a position to specific content rather
than checking that something happened. For **selection**, text dragged near the top of the
page must come from earlier in the page's text than text dragged further down --- a substring
check was tried first and cannot fail, since a selection is a contiguous range of indices
whatever the boxes claim. For **search**, a match's index range must cover the characters
searched for, re-extracted independently; every other search assertion passes just as well
when the indices are off by one.

Run all six corpora. Every run reports the same **86 check names**; what differs is how
many are `[SKIP]` with a reason, and a name that goes missing rather than skipping is the
bug this arrangement exists to catch:

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 75--76 | 10--11 | the dense case, and search across 775 pages |
| `outline-simple.pdf` | 81--82 | 4--5 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 81 | 5 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 52 | 34 | one page, no extractable text, and no white paper to invert |
| `vector-multi.pdf` | 59 | 27 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 75 | 11 | every page at `/Rotate 90`, which nothing else in the corpus has |

**`vector-multi` takes about 4m40s**, and everything else a fraction of that --- twelve A0
pages is what it is for. The default timeout was 300 s, which sat close enough to that to
fail intermittently, and the timeout path *discarded the transcript* --- so a slow machine
produced one line, `[FAIL] run timed out`, which is exactly what a page that never ran a
line of JavaScript produces. It now prints how far it got and the bound is 900 s, well
clear of the slowest corpus rather than beside it.

The two vector fixtures skip three of the six inversion checks, and that is the design
working rather than a gap: "the page went dark" cannot be shown on a document with no bright
paper, so it says so instead of passing on nothing.

**The ranges are all one check: "the strip withdraws its work when the viewer needs the
renderer".** A thumbnail on a cheap page takes about a millisecond, so whether one is still
in flight when the viewer asks for a tile is a race, and the check skips when it is not ---
correctly, since nothing outstanding reads exactly like a successful withdrawal. Repeated
runs of `text-heavy` and `outline-simple` have each landed on both sides of it. It is
deterministic only on `vector-multi`, which exists for it.

Absolute counts are deliberately not quoted in this paragraph: they move whenever a check is
added, and a stale number here would send someone looking for a regression that is a
changelog entry. The table above is the one place they are written down.

**So the ran/skipped columns are not the invariant** --- the **86 names** are. A count chased
back to a documented value is a defect introduced to satisfy a document, and the repair here
would be to delete the outstanding-request condition that makes the withdrawal observable at
all. Read a differing count by checking that the name is present and `[SKIP]`; a name that
has *vanished* is the bug this arrangement exists to catch.

This was written as a fixed `65 | 10` first, and a perfectly ordinary run then read as a
regression. **A table that records one sample of a race as an invariant makes the next honest
run look like a defect** --- state the range and what varies, or the check that flips gets
"fixed" by someone chasing a number.

**Do not run all six while iterating.** Each run needs an `.app` bundle rebuilt and takes
the better part of a minute, and six transcripts of green is not evidence of anything --- the
value of a regression check is in the run that goes red, and nothing about running the same
one repeatedly makes that more likely. Use **one** corpus while a change is in progress,
picked for what it can exercise, and the full sweep **once before a commit**, where "did I
break something elsewhere" is the actual question. What the sweep is for is the corpora's
*differences*: `vector-heavy` skips 31 of the 75, and those are the ones a single corpus
cannot tell you about.

`vector-heavy` skipping most of them is the expected output there, not a problem. The one
search check it does run is the useful one for that document: that the viewer says there is
no text to search rather than reporting no matches.

`rotated-90` is the only document where the text layer's coordinate turn is exercised at all,
and the defect it found was total rather than subtle --- see `docs/PLAN.md`. Its selection
ordering check skips, with the reason: on a page whose lines advance sideways a horizontal
drag crosses all of them, so the comparison is meaningless. What checks that mapping properly
is the probe, per rotation:

```
for page in 0 1 2 3; do
    for view in 0 1 2 3; do
        src-tauri/target/release/text-probe testdata/rotated.pdf \
            --page $page --mode align --view-turns $view
    done
done
src-tauri/target/release/outline-probe testdata/rotated-90.pdf --mode check \
    --manifest testdata/rotated-manifest.json
```

`--view-turns` rotates the *view* on top of the page's own `/Rotate`, which is what Cmd-R
does. All sixteen combinations should report 100% of character boxes on ink with every wrong
turn under the control ceiling; anything else means the render and the boxes have stopped
agreeing, and the pattern of which combinations go red says which half. Dropping the
placement's dimension swap fails only the odd view turns; ignoring the rotation in the render
fails all twelve rotated ones.

`vector-multi` earns its place with three checks and nothing else: a thumbnail costs about a
millisecond on a text page and a second and a half on an A0 sheet, so it is the only corpus
where the page strip can still be rendering when the viewer asks for a tile. On every other
document those three report `[SKIP] the thumbnail finished before the viewer asked for
anything` --- which is the honest answer, and is why they are not written as a pass.

What it does **not** cover: the command list `App.svelte` registers, and the Cmd-K that
opens the palette. The check builds its own registry, so it proves the palette works and
not that the application's commands are wired to it.

### Checking session restore

Reopening where the reader left off is a property of a *launch*, so it takes more than one:

```
scripts/session_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

Four launches, and the two labelled `control:` are what make the other two mean anything:

| phase | session | argument | asserts |
|---|---|---|---|
| `record` | fresh | a document | drives to page 7, one quarter turn, a fixed zoom, sidebar open --- then writes it |
| `control: opening without a session` | empty | a document | that state is **not** where the app opens by itself |
| `verify` | recorded | none | the app came up in that state, told only by the file |
| `control: launching with nothing remembered` | empty | none | no document opens when nothing is remembered |

Without the first control, "restored to page 7" is satisfied by an app that happens to open
there --- the same shape as a check whose precondition is already satisfied, which this
repository has paid for four times. It fails if *any* of the four fields already matches,
not only if all of them do: a restore that got only the rotation right would otherwise hide
behind a default that shared the page. Without the second, an app that reopened the last
file it could find by some other route would pass `verify` perfectly.

Between the phases the script reads the written `session.json` itself. Writing a place and
reading one back are different halves, and a run that only did the second would find nothing
to restore and report that somewhere else entirely.

Unlike every other harness here, **this one does not replace the application** --- it boots
normally and observes itself, because restoring is part of the boot and a check that drove
`session.ts` directly would be a second implementation agreeing with the first. Same bundle
and unlocked-screen requirements as the viewer check.

Every launch gets its own `TPDF_SESSION_FILE` in a temporary directory, and **the two
controls get one each rather than sharing**. Shared first, and the second control failed:
the first control opens a document, which is what it is for, so by the time the second
launched there was something to restore and a document duly opened. A control is the thing
you assume is inert, which is why the standing rule about what one phase leaves behind for
the next did not fire.

Unlike the viewer check, **the exit code here is meaningful** --- see the note below.

### Checking file associations

A PDF reaches tpdf three ways and they share almost no code, so this drives all of them:

```
scripts/open_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app testdata/text-heavy.pdf \
    --other testdata/outline-simple.pdf
```

Note it takes the **`.app` bundle**, not the executable inside it: two phases go through
Launch Services and there is nothing else to hand `open`.

| phase | delivery | asserts |
|---|---|---|
| `argv` | the binary, with a path | the terminal and Windows double-click route |
| `double-click` | `open -a` on a cold app | the Apple Event, which is how macOS actually does it |
| `beats` | argv, with a different document remembered | a handed-over document wins |
| `control` | nothing handed over | the remembered one opens --- without this, `beats` passes on an app that ignores the session |
| `running` | `open -a` on an app already up | the *emit* branch rather than the queue |

`running` is the only phase that would notice the frontend and the backend disagreeing about
the event's name, and it carries its own control: nothing may be open before the document
arrives, or "a document arrived" is satisfied by one that was already there.

**The environment does reach an app that Launch Services started** ---
`TPDF_OPENCHECK=… open -a tpdf.app file.pdf` propagates --- which is what makes the
double-click phase testable rather than merely argued. Both `open` phases capture the app's
stdout with `open --stdout`.

Same bundle and unlocked-screen requirements as the viewer check, and one extra: **leftover
tpdf windows occlude new ones**, and an occluded page never runs, so a phase produces no
output at all. `pkill -f "tpdf.app/Contents/MacOS/tpdf"` before a run, or `TPDF_RAISE=1`.
This cost real time once already --- it looked exactly like the failure it was sitting next
to, which was genuine.

### The exit code of a spike run

`AppHandle::exit(code)` does **not** set the process's exit code. It ends the event loop,
`App::run` returns normally, `main` returns unit, and the process exits 0 whatever was asked
for. Every automated run here therefore reported success through `$?` for its whole
existence, `viewer_check.py` included. Fixed 2026-07-27 in `spike_exit`, which now flushes
and calls `std::process::exit`.

If you add a harness, do not let the exit code be its only verdict --- parse the transcript
too, and make the two agree. That is what caught this: a run printing `[OK] session restore
verified` directly beneath a phase whose own last line said `0/1 checks passed`.

---

## Cutting a release

Version scheme is **CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO
starts at 0 and increments within the month.

1. `git fetch` and confirm the local branch is not behind --- this repo is pushed from more
   than one machine, and a version bump on a stale clone has already cost a re-cut release
   elsewhere in the portfolio.
2. Bump **all four** version files so they agree:
   - `package.json`
   - `package-lock.json` (top-level *and* the root package entry --- `npm version <v> --no-git-tag-version` does both)
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
3. `cargo check --manifest-path src-tauri/Cargo.toml` to refresh `Cargo.lock`.
4. In `CHANGELOG.md`, replace `Unreleased` with the release date.
5. `scripts/gates.py` --- all gates pass.
6. **Re-check `docs/THREAT-MODEL.md` against the code**, and correct the document before
   trusting anything else in this list --- §3's boundary table, §5's sandbox policy and
   §6's macOS column especially. Every present-tense sentence there claims something is
   *wired*, and a mitigation stated in prose and enforced nowhere reads exactly like one
   that holds: three consecutive review rounds each found at least one claim that had
   quietly become a description of an earlier phase, and the third of them was the CPU and
   memory bounds in §T3. §8 lists the probes that answer the mechanical half
   (`worker-probe`, `backend-probe`, and `worker-bench --mode engine|authority` after any
   PDFium bump). The half no probe covers is reading each claim and naming the line that
   keeps it. Anything that turns out not to be wired gets wired or gets marked, never left.
7. `npm run tauri build` and smoke-test the bundle, then `scripts/viewer_check.py` against
   it on both `testdata/text-heavy.pdf` and `testdata/vector-heavy.pdf`.
8. Commit as `Release vYY.M.MICRO: <summary>`.

Verify the bump landed everywhere:

```
grep -n '"version"' package.json src-tauri/tauri.conf.json
grep -n '^version' src-tauri/Cargo.toml
```
