# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

The one thing this file does *not* carry in full is the trap list --- 220 entries
in [`docs/TRAPS.md`](docs/TRAPS.md), indexed by title below. That file is **not**
auto-loaded, on purpose, and the index exists so that the decision to read an entry is an
informed one rather than a guess.

## What tpdf is

A desktop PDF viewer and editor for macOS and Windows. Built because nothing on the
market fits: Adobe Acrobat is slow, buggy, and hides its tools behind endless menus;
Foxit is the same shape with a different skin; SumatraPDF is fast and lightweight but
cannot edit.

**The thesis, in one line:** SumatraPDF's speed with Acrobat's capability, and a UI where
you never hunt for a tool.

Three non-negotiable properties, in priority order:

1. **Fast.** Cold start to first page painted under 300 ms. Scrolling never stutters.
2. **Discoverable.** Every command reachable in two keystrokes via the command palette.
3. **Capable.** Annotations, page operations, forms, signatures, true redaction, and
   eventually in-place text editing.

Sibling projects built on the same reasoning: `screenpick` (screenshot tools were
bloated), `dblitz` (DB Browser for SQLite was missing things).

---

## HANDOVER --- open on macOS, written 2026-08-02 from Windows

**Delete this whole section once the five items below are done.** It is a transient note in
an auto-loaded file, so it costs every agent on every machine until it goes.

Everything after `7dd6170` --- a full-tree review, fixes for its sixteen warnings, and the
work it recommended --- was **written and verified on Windows only**. CI covers the twelve
gates on both platforms; it structurally cannot cover `viewer_check.py`, `session_check.py`
or the mutation harnesses. The surface is unusually broad: the worker split, the per-page
layout, the reporting migration and the logging all cross platforms.

### 1. Build first, and expect the worker split to be where it breaks

`worker.rs` was split into five files as a proven pure move --- proven on a machine where
`#[cfg(target_os = "macos")]` code **does not compile**. `worker_handover.rs` (the fd-passing
document handover) and the macOS halves of `worker.rs`/`worker_shm.rs` have never been
through a compiler since they moved, so the first `scripts/gates.py` run here is the actual
verification of that split. A failure in those files is the move (a `use` that did not
follow, a visibility too narrow under the macOS cfg), not whatever was being worked on.

### 2. Re-run the window harnesses; the invariant is 163 names now

Regenerate fixtures first (`testdata/make_mixed_pdf.py` is new; its `mixed-geometry.json`
sidecar is gitignored and wired through `TPDF_GEOMETRY_MANIFEST`). Then `viewer_check.py`
on all corpora --- `mixed.pdf` is the eleventh, 138/25 on Windows --- and `session_check.py`.
Diff name sets against `BUILD.md`'s table per the standing discipline; the wrapper's
module-audit verdict moved to stderr, so `mutate_viewer.py`'s baseline must say 163 here
too. `viewercheck.ts` now reports through `checkreport.Report`: pad width 46, ASCII skip
join, `not applicable` summary tail --- the three deltas every parser was checked against.

### 3. The pdfium stamp will warn once, and the warning is the upgrade path

`fetch_pdfium.py --check` now hashes the installed library against a digest the installer
records. A stamp from July predates that line, so `--check` prints `[WARN]` and falls back
to an existence check rather than going red. Run `python scripts/fetch_pdfium.py` once; it
re-fetches, records the digest, and `--check` is strong from then on.

### 4. Expect `multilingual.pdf` green here, and do not read that as the bug being gone

The folding page fails on Windows --- `cafélatte` for `café latte` --- and the mechanism is
measured in `BUILD.md`: `msgothic.ttc` gives the space a box 0.02 pt tall floating 0.12 pt
below the line's band, so `reading.ts` bands it with nothing and drops it. Arial Unicode
puts its space in-band, so macOS stays green on a different document. The fix is open work
for whichever machine reaches it first: treat a box under some epsilon height as unplaced
and re-attach it by preceding index, with two controls --- every macOS-green corpus stays
green, and the Windows folding line goes green.

### 5. Confirm the log file lands where macOS puts logs

Nine coordinator diagnostics now also append to `tpdf.log` via `diag::note` --- stderr is
untouched, and a test re-runs the binary to prove it. What no Windows machine can check:
that a real macOS run writes under `app_log_dir` (`~/Library/Logs/...`). Open a document,
kill nothing, and look for the file; then `TPDF_LOG_FILE` for the override path.

---


## Hard constraints

### Licensing: permissive dependencies only

**No AGPL or GPL dependencies. Ever.** This is a deliberate, load-bearing decision, not
an accident of what was convenient.

MuPDF (what SumatraPDF uses) is the obvious engine and was rejected. It is dual-licensed
AGPL / commercial, and the AGPL path costs three things that matter here:

- **It is viral across all of tpdf.** Every line of Rust and Svelte becomes AGPL.
- **It would forbid reusing tpdf code in private or work repositories.** Lifting tpdf's
  text extraction or page-splitting into an internal tool that processes documents at
  work would require AGPL-ing that tool, which is not something an employer's codebase
  can absorb. This is the cost that actually bites, given the surrounding portfolio.
- **It would make relicensing later impossible** without an Artifex commercial licence
  (quoted case by case, $1,500 to $50,000+).

It would also rule out the Mac App Store, whose terms conflict with the GPL family.
Direct notarized distribution (what `screenpick` does) is unaffected.

The repository is **public** and MIT-licensed, which is what that decision was protecting.
The option is now spent rather than held: a copyleft dependency added today would not
merely close a door, it would contradict the licence already granted to everyone who has
cloned this. Do not introduce one. If a copyleft library ever looks necessary, raise it as
a decision rather than adding it.

Two obligations follow from shipping binaries rather than only source, and both are now
executable rather than written down. `THIRD-PARTY-NOTICES.md` reproduces the notices a
binary distribution requires, `scripts/third_party_notices.py` generates it, it ships inside
both installers, and the `notices` gate fails on a stale file or a forbidden licence. Do not
edit it by hand; a hand-maintained notices file is wrong the first time a dependency changes
and nothing says so.

**The `cargo metadata` sweep this file has recommended since the beginning is real and
structurally incomplete, and the gap is the whole product.** It sees 531 cargo packages. It
is blind to the **fourteen C++ libraries compiled into libpdfium** --- FreeType, ICU,
libjpeg-turbo, libpng, libtiff, Little CMS, OpenJPEG, zlib, Abseil, AGG, fast_float,
simdutf, llvm-libc --- because no cargo command can see inside a prebuilt blob, and that
blob is the thing that actually parses PDFs. A sweep complete over cargo and silent about
everything else passes exactly like one that covered everything, which is the
consistency-versus-completeness trap arriving in the licensing constraint that the entire
project rests on. The gate now enumerates `vendor/pdfium/licenses/` as a third population,
so a new file appearing there is a finding.

Two GPL strings live in there and both are benign; they are allowlisted **by file and by
mechanism** in the script, never inferred, and an entry naming a file that has gone produces
a warning rather than silently excusing nothing. `icu.txt` covers ICU4C's autotools scripts
under the Autoconf exception, which are build-time files of a library we consume prebuilt;
`llvm-libc.txt` is Apache-2.0 WITH LLVM-exception, whose GPLv2 clause *waives* Apache terms
rather than imposing GPL ones. All three of the gate's failure modes were proved by mutation
before it was trusted.

### Redaction must be genuine

Redaction removes content. It does not draw a black rectangle over it. Any implementation
that leaves the underlying bytes recoverable is a defect, not a limitation --- see
`docs/PLAN.md` §6 for the full subsystem design.

Corollary, and the harder half: **tpdf must never claim a redaction is clean unless it
can prove it.** A verification that cannot decode a carrier has not verified anything. If
any check cannot complete, the result is "not verified", never "clean".

### Every PDF is hostile input

PDFium is native C++ parsing attacker-controlled files, and PDF is a format with
JavaScript, launch actions, embedded executables, recursive object graphs and
decompression bombs in it. Chrome sandboxes PDFium in a separate process for exactly this
reason, and so must tpdf. **In place for the viewer's own render path since 2026-07-28** —
`RenderService` defaults to worker processes on macOS, and `examples/backend_probe.rs` proves the
app process never maps libpdfium by reading the dynamic linker's image table.

**On both platforms since 2026-07-29.** macOS gets its boundary from `sandbox_init` SBPL,
which the child applies to itself after `exec`; Windows has no counterpart, so the *parent*
builds one --- a low-integrity token inside a job object, applied while the child is still
suspended. `Backend::default_here()` selects workers on both, and a platform with neither
still falls back to in-process and records `render::UNSANDBOXED_MARK` with a `[WARN]`, so an
uncontained run stays distinguishable from a contained one. A mark rather than a refusal is
deliberate: refusing would make a platform useless rather than uncontained.

**The Windows evidence is external, which is the part that matters.** A milestone we record
says what our code believes it did. `scripts/win_modules.py` reads the app process's loaded
module list from *outside* it, through Toolhelp, while a document is open, and asserts
`pdfium.dll` is absent --- with the module count printed beside it, so a failed enumeration
cannot read as containment. `viewer_check.py` samples it throughout the run and takes the
union, since the parser is mapped only while a document is open.

It was run **before** the flip and reported the parser mapped: 47 modules at peak, `[FAIL]`.
That control is the reason the pass afterwards means anything. After: all four corpora green
with the same ran/skipped splits as before (81/5, 81/5, 75/11, 52/34), the `[WARN]` gone, and
44--45 modules at peak with no `pdfium` among them --- including `outline-hostile`, which is
the corpus that most wants a boundary.

**What Windows containment can actually be is now measured, not guessed** (2026-07-29,
`examples/win_sandbox_probe.rs`). Six rungs, each rendering the same tile from the same document
in a re-exec'd child and compared **pixel for pixel** against an in-process render, with an
uncontained child as the control over the harness itself:

| rung | renders | identical | denies |
|------|---------|-----------|--------|
| `bare` (control) | yes | yes | nothing |
| `job` object | yes | yes | runaway memory, extra processes, orphans |
| `lowil` (job + low integrity) | **yes** | **yes** | writing the user profile, opening the parent process |
| restricting SID (`S-1-5-12`) | **no** | --- | everything, including the loader |

So the answer is **low integrity plus a job object**: PDFium renders byte-identically under
it --- the font-substitution risk that the macOS work already caught did *not* materialise ---
while losing the authority to write anything or reach into the app process. A restricting SID
is the stronger rung and is not reachable directly: the child dies at `STATUS_DLL_NOT_FOUND`
before `main`, because the loader's own reads are denied. Reaching it needs Chromium's
initial-token / lockdown-token handover, which is a real piece of work rather than a flag.

One honest limit on that: low integrity **does not stop reads**, so a contained worker could
still read any file the user can --- which is why the document and the output are handed over
as inherited handles rather than paths, the Windows analogue of the macOS `dup2`.

**The `job` row's denials are measured as of 2026-07-30, and were not before.** That row
promised "runaway memory, extra processes, orphans" from the day it was written and the probe
tested none of the three: its three authority probes are all *integrity level* properties, so
every rung reported on `lowil` and above while the job's own two limits went unexercised. Two
more probes close it, and the control earns its keep --- `bare` commits 1 GB and starts a
process; every rung with a job is refused with **1455** (commit charge) and **1816** (process
quota). The third, an orphan outliving the parent, is `KILL_ON_JOB_CLOSE` and is still only
claimed: testing it means killing the probe itself.

Worth knowing rather than inferring, because it is a real asymmetry with macOS: the Windows
bound is on **committed** memory, which the kernel charges at `VirtualAlloc` time, so a
decompression bomb is refused *before* a byte of it exists. macOS bounds *resident* memory, so
its balloon has to write to every page it takes. That is why `Worker::footprint` returning
`None` on Windows is not the gap it looks like --- there is a kernel bound there instead of a
poll, and it is now the measured kind. (Nothing in production reads `footprint` on either
platform; only `pool-bench` does.)

**And the render deadline is real on Windows as of the same day, having silently not been.**
`kill_pid` was a `#[cfg(not(unix))]` no-op whose own comment predicted exactly this, and the
three tests that would have caught it were `#[cfg(unix)]`, so the platform where the guard had
stopped working was also the platform where nothing tested it. It did not merely fail to
enforce: `kill_overdue` set the killed flag and logged a kill, so the caller got a deadline
error while the process went on holding a hung render. The trap index entry *"a guard that
degrades to a no-op off its platform stops being a guard"* carries the detail and the mutation
that proves the fix.

**A Windows worker now exists and works** (2026-07-29). `Worker::spawn` builds one on Windows:
the child is created suspended, dropped to low integrity, assigned to the job object before it
executes an instruction, and given two pipes and the document and tile sections as inherited
handles named in argv. `worker-probe` is the evidence --- 11/11 checks on `text-base14`,
`text-cid`, `vector-heavy` and `rotated`, including **pixel-identical** tiles against the
in-process render, text extraction, outlines and search across the boundary. The font
substitution that the macOS sandbox caused, and that `win_sandbox_probe` predicted would not
recur here, did not.

`Worker` carries the two platforms as per-platform type aliases rather than an enum, so the
macOS *types* are what they were: `WorkerProcess`, `WorkerStdin` and `WorkerStdout` resolve
there to `Child`, `ChildStdin` and `ChildStdout` exactly as before. The reasoning was that none
of this can be re-verified on macOS from a Windows machine, so a diff touching only Windows code
is the strongest statement available about what cannot have regressed.

**It is not literally a Windows-only diff, and this said it was** (corrected 2026-07-30, from
the macOS side). The struct fields, the `use`, `WorkerSender`'s inner type and three accessors
were all renamed onto the aliases, and two `#[cfg(not(target_os = "macos"))]` refusal arms were
deleted --- macOS lines, changed. The behaviour is identical because the aliases resolve to the
same types, but that is a claim about what a compiler does, not the "nothing on that platform
was touched" the sentence promised, and the two are only the same thing until one of them is
wrong. What actually stands behind macOS here is that the harnesses were re-run there: `gates`
8/8 with 168 tests, `backend-probe` 41 names across four corpora with identical name sets,
`worker-probe` 12/12 on four, `viewer_check` 86 names across all six, `session_check` and
`open_check` green. Worth stating because the original phrasing invites the next session to
skip exactly that.

Those two counts are **of their date and are no longer current** --- 168 tests and 86 names were
what that run measured, and macOS reports **182 tests and 109 check names** as of 2026-07-31.
They are left rather than overwritten because the paragraph is about what was verified when;
the number to work from is the one in `BUILD.md`'s table, which is the single place these are
written down.

**Windows no longer fails open.** `Backend::default_here()` selects workers there, proved by
the external module check above rather than by the absence of our own warning.

**`backend-probe` runs on Windows too, and passes** (re-measured 2026-07-31): **38/42** on
`text-base14` and `text-cid`, **39/42** on `outline-hostile`, **40/42** on `vector-heavy`, which
is where a render is slow enough for the withdrawal checks to run rather than skip. Name sets
byte-identical across all four, diffed rather than counted. This paragraph read `37/41 ... 40/41`
until then, which is the same count one commit earlier and **not** a platform difference --- see
`BUILD.md`, which carries the table and why the "one check is macOS-only" reading was wrong.
No failures on any. The
boundary, the pixel comparisons, capacity, crash restart, replacement, retirement, close,
descriptor return **and the spare's lifetime** all pass. Its Windows primitives are Toolhelp for
the module list and the process table, `GetProcessHandleCount` for descriptors, and
`TerminateProcess` for a hostile kill from outside the pool.

That run is also the end-to-end evidence for the Windows spare, and it is worth reading the
detail rather than the count: `at open: pool [18840], children [2672, 18840], spares [2672]` ---
a warmed child exists, is correctly *excluded* from the pool, and the laziness claim beside it
still says `opened with 1`.

**The two failures it first reported were the probe's, not the pool's**, and the correction is
worth more than the result. They read as a pool that grows to six and keeps one, with a handle
count that never moved --- two independent observations agreeing on "created, used and destroyed
rather than pooled", which is what was recorded here for a day. Both readings were honest and
neither could say *when* it was taken: the sample sat behind a five-second wait for a
pre-spawned spare, and Windows has none, so it spent its whole bound on every call --- longer
than the phase's own four-second idle timeout. The instrument retired the pool and then measured
it. Nothing in `workers.rs` was touched. See the trap, which is now about the wait rather than
about the pool.

`pool-bench` and `prespawn-bench` act as their own worker on Windows now --- their `#[cfg(unix)]`
gate on the re-exec dated from before `worker_child` compiled there, and left each binary unable
to be the thing it measures.

**`tile-bench` was never blocked at all**, and this file said it was for two days: the list of
four refusing binaries had it in, and running it showed no refusal --- only a hardcoded
`vendor/pdfium/lib` and a `NaN` where a peak should be. Both fixed, so it now measures on Windows.
That is the trap of the same name arriving in this file rather than in the code: a blocker list
is written by reading, and reading over-reports. `worker-bench` is the one genuine refusal left,
and its reason is real --- it carries its own POSIX worker implementation, fd passing and SBPL
profiles included, and shares no mechanism with the Windows model. Seven of its eight modes; the
eighth needs no worker, was trapped behind the module's `cfg`, and now runs --- see the symbol-scan
trap for what it found.

**The two viewer harnesses run there too** (2026-07-30). `session_check.py` needed no porting at
all and passes its four phases with both controls --- note it needs a document of **at least eight
pages**, since its target page is 7; on a shorter one it now says so rather than reporting a wrong
page, which is what it used to do.

`open_check.py` runs **five of six**. It ran four until the last gap was closed: a second launch on
Windows was a second process, two windows and two worker pools, where macOS hands the document to
the running app. `tauri-plugin-single-instance` closes it --- the second process forwards its argv
to the first and exits --- and the callback feeds the same `Launch` queue and emits the same
`OPEN_EVENT` as every other route in, so there is one path for "open this document" rather than
two that can drift. Proved by mutation: disabling the plugin turns the phase red with *"nothing
ever arrived"* while its control still passes.

The one phase that stays macOS-only is the cold double-click, and that is not a gap: an Explorer
double-click arrives in `argv`, which the `argv` phase already covers, so there is no second
mechanism there to test.

So the tally on documented blockers is **four lists wrong this week, always by over-reporting**:
of six benchmarks and harnesses called macOS-only, two were genuinely gated, one was trapped
behind a `cfg` it never needed, one had only a hardcoded path, one needed nothing, and one was
two-thirds portable. Run it before writing it down as blocked.

**The error has a second direction, found 2026-07-30, and it is the quieter one.** The two
mutation harnesses were on nobody's blocked list --- and `scripts/mutate_rust.py` had never
executed a single mutation on Windows, dying on `read_text()` before its first one, while
`scripts/mutate_frontend.py` silently could not find three of its anchors. Both are fixed and
now report 22/22 and 75/75. Over-reporting a blocker costs a capability nobody uses;
**under-reporting one costs a check everybody believes ran**, which is the more expensive of
the two. A harness that has never run on a platform produces no failures there, and neither
does one that passes.

**Pre-spawning works on Windows too** (2026-07-30), so both platforms now start a worker before
a file is chosen. The handover is the only part that differs and it had to: a macOS parent
*sends* a descriptor over a socket, and a Windows parent **writes into the child's handle table**
with `DuplicateHandle` and then names the number it wrote. That direction is the one integrity
levels permit --- medium may reach into low, never the reverse --- so the handover survives the
containment structurally rather than by luck. The message is a `Handover` of its own rather than
a `Request` variant, which is what makes "adopt a second document" unsayable instead of something
the child must refuse.

Measured, not assumed, by `prespawn-bench`: **8.4--9.6 ms saved per open**, on a spawn-to-first-reply
of 8.9--10.4 ms for small documents. The saving is nearly constant, and that is the difference
from macOS worth knowing --- there the system-font walk is ~7.4 ms of it, here it is **~1.4 ms**,
so on Windows what pre-spawning buys is almost entirely the fixed floor (`CreateProcess`, the
loader, mapping `pdfium.dll`, the token and the job) rather than font enumeration.

**Printing works on Windows** (2026-07-30), which was the last user-facing capability the
platform did not have --- `present_job` returned `Err("printing is implemented on macOS only")`,
and its comment still justified that with "everything in this repository is macOS-only until a
Windows build has actually run", which had stopped being true two days earlier.

The half that corresponds exactly is the **readback**. macOS refuses to open a panel for a job
PDFKit cannot read; Windows now refuses for one `Windows.Data.Pdf` cannot read. Both are the
platform's own PDF stack, so both are independent of the `lopdf` that wrote the job and the
PDFium that drew what the reader saw --- which is the property the whole print subsystem is built
on, and the same standard `docs/PLAN.md` §6 sets for a redaction.

The half that does **not** correspond is the printing itself, and it is not a shortcut. macOS
hands PDF bytes to `NSPrintOperation` and the OS paginates and prints them as vectors. Windows
has no in-box "print this PDF" API at any layer --- not Win32, not WinRT --- so every Windows PDF
viewer, SumatraPDF included, rasterises each page onto a printer device context itself, and that
is what `print_win.rs` does. Two consequences to state rather than discover: Windows output is
**raster at 300 dpi** where macOS is vector, so text is not selectable in a print-to-PDF result;
and `Windows.Data.Pdf` reports page sizes in **DIPs at 96 to the inch**, not PDF points, which is
a trap with an entry because getting it wrong renders every page 1.33x too large and still looks
fine.

Three things came free with it, and the third is the one worth noticing:

- **`examples/print_probe.rs` verifies the whole path without paper.** "Microsoft Print to PDF" is a
  real driver and a real spooler, and naming an output file in `DOCINFOW.lpszOutput` stops it
  raising a save dialog --- so everything except the panel is driven end to end and the result is
  re-read by the OS parser. 8/8, including **ink per page** rather than a page count, because a
  broken blit produces the right number of blank sheets (proved: mutating the blit away leaves
  the count green and only the ink red).
- **Three of `print.rs`'s four third-parser checks now run on Windows**, where they were
  `#[cfg(target_os = "macos")]` because PDFKit used to be the only independent parser available.
  Proved to buy real coverage rather than merely existing: breaking `effective_rotation` turns
  both rotation checks red here, including `rotated.pdf`'s *which-pages-survived* case. The
  fourth needs text, which `Windows.Data.Pdf` has none of, so it asserts the page count and
  prints a `[SKIP]` naming what it could not check.
- **Printing maps a PDF parser into the app process, on both platforms.** That is the honest
  complication in "the app process never maps the PDF parser", and it is now measured instead of
  glossed: `print-probe` reads its own module table and reports 80 modules with none named
  pdfium, and `Windows.Data.Pdf.dll` beside it as what it mapped instead. The boundary's real
  guarantee is narrower than the sentence sounds --- no *our* PDFium, and the parser that is
  there is patched by Windows Update rather than pinned in `Cargo.lock`.

The `windows` crate this needs adds no crate to the tree: it is already there transitively
through Tauri's WebView2 stack, and it is `MIT OR Apache-2.0`, checked rather than assumed.

**A Windows distributable builds** (2026-07-30): an MSI and an NSIS installer, from
`npm run tauri build`. It did not, and the cause is worth knowing because it is a rule about
this repository's layout rather than a Tauri bug: **`src/bin/` must contain only declared bin
sources.** The bundler enumerates that directory and registers the first entry no `[[bin]]`
`path =` claims; a `.rs` file is always claimed, a *subdirectory* never is. So
`src/bin/backend_probe/`, which existed only to hold `imp.rs`, became a phantom binary named
`backend_probe`, colliding with the component id WiX derives from the real `backend-probe.exe`
and failing `light.exe`. The two `imp.rs` bodies now live in `src/probes/`, reached by
`#[path]`, which leaves module parentage and every `super::` in them unchanged.

It had never been caught because Windows packaging had never been attempted --- `BUILD.md`
mentioned neither MSI nor WiX. The trap entry records the four theories that were wrong first,
including an experiment whose control was placed where it could not fire.

**And it no longer ships the spikes.** Until 2026-07-31 the installer carried all 17 probe and
benchmark executables --- a sandbox prober and a hostile-document harness among them --- because
they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets now: cargo
builds and links them exactly as before, the `bins` gate keeps covering them through
`--examples`, and the bundler does not see them. Extracting the MSI shows a payload of three
files (`tpdf.exe`, `tpdf_lib.dll`, `pdfium.dll`); the MSI went 16.7 -> 8.0 MB and the NSIS setup
8.8 -> 5.8 MB. The invocations moved with them: `--example <name>`, and built artifacts now sit
in `target/release/examples/`.

**That gate flag is load-bearing, and was proved so rather than assumed.** Dropping
`--examples` would narrow the `bins` gate back to the one thing it was added to catch ---
`backend_probe.rs`'s dyld symbols, which is now an example --- leaving only the app under
`--bins`. An undefined extern called from one example's `main` turns the gate red with
`LNK2019`, which is the check that says the flag does something.

**The JavaScript harness does ship, and as of 2026-08-02 that is a decision rather than the
unexamined half of the same hygiene.** `App.svelte` statically imports all six webview entry
points --- `viewercheck`, `scrollbench`, `sessioncheck`, `opencheck`, `autobench`, `startup`
--- so the functional check and its five siblings sit in `dist/assets/index-*.js`, which
`frontendDist` embeds whole into the binary. Read out of the shipped file rather than off the
import list: check names such as *"a bare k does not open the palette"* and the timeline's
`TIMELINE-JSON` marker are literals in the minified bundle,
and `dist/shell.html` (9.0 kB), the framework-free page `ShellMode::Blank` loads, ships
beside it. The weight is **77.1 kB of a 221.2 kB bundle, 34.9%**, measured two ways that
agree: attributing the bundle's own sourcemap back to its sources, and separately minifying
each of the six with every import external, which lands at 77.8 kB --- 0.9% apart.

**It stays, and the first reason is the one the checks are built on.** Their whole design is
that they observe the artifact that ships; the frame loop, the input handlers and the layout
they assert against exist nowhere else, which is why they need a real window at all.
Excluding them at build time would run the 109-name invariant against a bundle nobody
installs --- a checked artifact and a shipped artifact that agree about everything except the
difference between them, which is the writer-and-its-own-reader failure this repository has
already recorded twice from other directions.

**The second reason is measured, and not marginally.** Priority 1 is a cold start under
300 ms, and the frontend payload is not what decides it: the `blank` variant deletes the
*entire* payload --- no module graph, no Svelte, no `@tauri-apps/api` --- and moved warm
start by -8.4, +9.9 and -0.2 ms across three interleaved runs (`docs/PLAN.md` §0). The reason
is the trap *"The shell floor is ~250 ms"*: the webview's first request over a Tauri custom
protocol costs ~45 ms and whichever request is first pays it, so a smaller payload only moves
which line of the table wears it. 77 kB inside a floor built from a WKWebView and a protocol
toll is not a lever. The six launch-time probes are the same shape --- each entry point asks
Rust for its variable through `spike_env` and returns on `None` --- and the baseline's first
IPC costs 0.0 ms, because the module fetch has already paid the toll.

**The 2026-07-31 removal does not transfer, and the difference is authority rather than
size.** The 17 that left were *executables*: independently launchable, each with its own
hostile-input surface, sitting in the install directory where anything that can run a file
can run them. They were also 8.7 MB of a 16.7 MB MSI, over a hundred times this harness.
Dead JS in an embedded bundle is launchable by nothing: it holds no authority the bundle does
not already have, and it cannot start itself, since every entry point is inert unless its
variable is set in the app process's own environment. That environment surface is the
binary's, not the bundle's --- the 32 `TPDF_*` levers are read in `src-tauri/src`, and **no
`TPDF_` string occurs in the shipped JS at all**. Read the *"payload of three files"* above as
the statement about executables that it is; the frontend rides inside `tpdf.exe`.

**The honest cost is `spike_print` and `spike_exit`**, registered in `generate_handler` and
therefore callable by any script the webview runs: one prints to stdout, the other calls
`process::exit` with the code it is handed. Two things bound that, and neither is a promise
about the harness. The CSP is `default-src 'self'` with no `'unsafe-inline'`, so the only
script that runs is the one that shipped --- residual risk 7 in `docs/THREAT-MODEL.md` carries
that, the T8 invariant that keeps document text from becoming script or navigation, and the
seam it leaves, since a grep over TypeScript cannot see the Rust half. The marginal authority
is nil: a caller able to reach `spike_exit` can already reach `open_document` and the print
path, so what these two add is a denial of service, not an escalation. **What would reopen
the decision**: a spike command with authority past print-and-exit, or a harness grown to
where bundle size moves the shell floor. The second is 45 ms of protocol toll away --- but
this is a decision about the numbers above, to be re-measured rather than inherited.

Non-negotiable: parsing and rendering happen in **worker processes** with no filesystem or
network authority, under resource and time limits, restartable on crash. Document
JavaScript and launch actions are **disabled by default**. All `lopdf` stream decoding is
bounded. This is a Phase 0 concern, not a hardening pass to be done later --- retrofitting
a process boundary is an architectural rewrite.

This constraint is load-bearing in a second way: because concurrent in-process PDFium calls
are undefined behaviour and crash in practice (see Known traps), worker processes are also
the *only* route to parallel rendering. Security and performance want the same
architecture.

`docs/THREAT-MODEL.md` is the worked-out version: what is being defended, the trust
boundaries, each threat against the evidence that it is handled, the sandbox profile in
full, and the residual risks in one list. Every claim there is either measured with the
spike named, or marked untested --- keep it that way when adding to it.

---

## Stack

The **shell is settled**, and since 2026-07-27 so is the **PDF layer** --- Phase 0 proved
each provisional choice and the verdict is recorded per row (see `docs/PLAN.md` §9).

| Layer | Choice | Status |
|-------|--------|--------|
| Shell | Tauri 2 | Settled |
| Frontend | Svelte 5 (runes), TypeScript `strict: true`, Vite | Settled |
| Backend | Rust | Settled |
| Platforms | macOS + Windows | Settled |
| Rendering + text extraction | PDFium via [`pdfium-render`](https://docs.rs/pdfium-render) (BSD-3-Clause) | **Settled** --- renders, extracts and sandboxes correctly; not usable for redaction (spikes 0.1, 0.3, 0.5) |
| Object graph + content streams | [`lopdf`](https://docs.rs/lopdf) (MIT) | **Settled** --- surgical rewriting and sanitation both work, with our own mark-and-sweep and an encryption guard (spikes 0.3, 0.4, 0.6) |
| Hardened structural rewrite | [QPDF](https://qpdf.readthedocs.io/) (Apache-2.0) | Candidate --- not required for the rewrite; still wanted for preserving encryption and for object streams |
| macOS print dialog | PDFKit + AppKit via [`objc2`](https://docs.rs/objc2) (Zlib OR Apache-2.0 OR MIT) | **Settled** --- paginates and runs the panel; also the independent parser every print job is read back with |
| Windows print dialog | `Windows.Data.Pdf` + GDI via [`windows`](https://docs.rs/windows) (MIT OR Apache-2.0) | **Settled** --- reads the job back, rasterises each page onto a printer DC, `PrintDlgW` for the panel. Raster where macOS is vector; see below |

The PDFium pin is `chromium/7881`, installed by `scripts/fetch_pdfium.py` and verified by
digest. Every measurement in this file was taken against that build, so bumping it
invalidates them until the two checks in `BUILD.md` are re-run.

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI.

Two crates carry the search, and this sentence said **three** while naming one until
2026-08-01 --- which is the counting-in-prose failure this file records elsewhere about the trap
list, arriving in the section where a missing dependency matters most.

`regex` (MIT OR Apache-2.0) reads a reader's pattern, and it was already in the tree
transitively through the toolchain, so declaring it added no package. `caseless` (MIT) does
Unicode case folding, which is what makes `strasse` find `Straße`: `char::to_lowercase` is
defined for *displaying* text and leaves a sharp s alone, and folding is the operation defined
for caseless *matching*. It brings `unicode-normalization` (MIT OR Apache-2.0) with
`tinyvec`/`tinyvec_macros` (permissive) --- the only genuinely new packages either of them adds.

Both checked with `cargo metadata` over the whole tree rather than from a README, which is the
standing rule for anything the licensing constraint above touches. The sweep covers all 531
packages and looks for the copyleft families by name; the only hits are MPL-2.0 (file-level, in
Servo's CSS crates via Tauri) and a triple-licensed `r-efi` whose `MIT OR Apache-2.0` arm
applies, so the option of making this repository public is intact.

Two plugins are linked. `tauri-plugin-dialog` (Apache-2.0 OR MIT) for the file-open dialog,
which pulls `tauri-plugin-fs` (Apache-2.0 OR MIT) and `rfd` (MIT); and, on Windows only,
`tauri-plugin-single-instance` (Apache-2.0 OR MIT), which is what gives that platform the
document handover macOS gets from `RunEvent::Opened`. Checked against the licensing constraint
above rather than assumed --- every dependency added has to be, because one copyleft crate
anywhere in the tree removes the option of making this repository public. The check is
`cargo metadata` over the whole tree, not a glance at the crate's own README.

### What each library is, and is not

Be precise about this. The 2026-07-26 audit found the earlier framing implied a complete
editing stack where there is none.

- **PDFium is a renderer and text extractor** with a limited object-mutation API. It is
  what Chrome ships, so it is correct on the long tail of malformed real-world PDFs in a
  way no younger library is. It does **not** provide semantic content-stream editing,
  structural sanitation, signature creation, paragraph layout, or font-subset extension.
- **lopdf is a low-level syntax layer.** It gives the object graph and decoded content
  operators; PDF *semantics* are left entirely to us.
- **QPDF** is the strongest candidate for the redaction rewrite path specifically, because
  it does hardened structural rewriting with garbage collection of unreachable objects ---
  precisely the guarantee a full rewrite needs and neither of the other two offers.

The honest consequence: **tpdf is building most of an editor engine itself.** These
libraries remove the rendering and parsing problem, not the editing problem. Plan
schedules accordingly.

Apache PDFBox was evaluated and rejected --- it is the best reference implementation for
forms and signing, but it is Java, and a JVM in a Tauri app defeats the entire premise.
It remains useful as a *behavioural oracle* to test against.

Pure-Rust renderers were considered and are not yet ready to be the primary engine.

---

## Versioning

**CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO starts at 0 and
increments per release within the month. Same scheme as `screenpick`, `atr-viewer`,
`snowscreen`, `sitm-explorer`, `ticket-creator2`, `ddf`.

Following `screenpick`, **four files must agree** on every version bump:

1. `package.json`
2. `package-lock.json` (top-level *and* the root package entry --- `npm version <v> --no-git-tag-version` does both)
3. `src-tauri/Cargo.toml`
4. `src-tauri/tauri.conf.json`

Then run `cargo check` to refresh `Cargo.lock`.

Each release is a `Release vYY.M.MICRO: ...` commit. Unreleased work sits under
`## [YY.M.MICRO] - Unreleased` in `CHANGELOG.md`; the date replaces `Unreleased` only at
release time.

---

## Quality gates

`scripts/gates.py` runs them all, and **is** the gate list rather than a description of
one. `BUILD.md` names that one command and deliberately does not repeat the commands
underneath it.

That is a deviation from the portfolio rule, which says a release checklist must state
every gating command verbatim with its flags. The rule exists because a hand-copied
command quietly loses a `--locked` or an `--all-targets` and then tests something weaker
than the real gate. Keeping the commands in exactly one executable place satisfies the
intent without the copy that has to be re-verified. Ask the script, not a document:

```
scripts/gates.py --list
```

Currently twelve: a toolchain-pin check, a PDFium pin check, a trap-index check, `cargo fmt
--check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo build
--locked --bins --examples`, a webview-sink check, `npm run check`, `npm run test`, `npm run
build`, and a third-party-notices check. Two of them are ordered rather than merely present:
`toolchain` runs **first**, because every result after it is a statement about whichever
compiler actually ran, and `notices` runs **last**, because it reads the build's own
sourcemaps to see which npm packages shipped.

**`pdfium` verifies the library, not the stamp beside it**, as of 2026-08-02. It compared the
pin against a digest the installer itself had written, and its only fact about the tree was
that *something* matching `*pdfium*` sat in `lib/` or `bin/` --- which on Windows the import
library `lib/pdfium.dll.lib` satisfies alone, so deleting `bin/pdfium.dll`, the blob that
parses every hostile document, left the gate green. `SHA256.txt` now carries a second line
recording the extracted library's own digest, and `--check` asks for `library_path(key)` by
name and re-hashes it. An install predating that line is not refused --- it was admitted by
the archive check and the machines holding one are fine --- but the run prints a `[WARN]`
saying which of the two checks it actually ran. The trap *"A directory that exists is not the
library you need"* had arrived inside the script whose docstring names that same mistake.

**`traps` compares `docs/TRAPS.md`'s titles against this file's index as sets.** The count
here has an authority (`grep -c '^### '`) and stopped drifting; the index did not, and was
three entries short on 2026-08-02 --- added by the commit that had updated the number. So the
tally was right while the list nobody counts was wrong, which is the doctrine one level up:
the invariant is the set of titles, and a set diff needs no number. The rule it enforces is
the file's own --- a bullet is the title verbatim, optionally with a parenthetical the index
adds where a title misleads on its own. It refuses an empty scan on either side and a
duplicate on either side, since two bullets covering one title can hide a third going
missing. Proved by removing a bullet, adding one naming nothing, duplicating one, and by
disabling the parenthetical rule inside the checker; all four red.

**`sinks` enforces `docs/THREAT-MODEL.md` T8**, which until 2026-08-02 was the one mitigation
in that document held by convention rather than by a line. Document text --- outline titles,
search results --- is attacker-controlled and reaches the DOM as data; the gate pins the
narrow invariant that makes that checkable at all, **no markup-parsing sink anywhere in the
frontend**, which is sufficient rather than merely necessary because without a sink the only
routes left do not parse markup.

Five further rules close the routes by which a string that cannot become *markup* can still
become a *navigation or a script*: a computed `setAttribute` name, a dangerous literal one
(`href`, `src`, `on*`), an assignment to a navigating property, and --- the blunt ones that
make the others nearly moot --- **creating a URL-bearing element at all**, by a literal name
or a computed one. It also refuses a scan that found no files, no `setAttribute` calls or no
`createElement` calls, since a pattern that stops occurring passes exactly like a clean one.
Every rule proved to fire by mutation, with a control (`this.onChange`, an ordinary field)
proved *not* to.

**Each rule reads the namespaced spelling too**, added 2026-08-02 with the computed-element
rule. `.setAttribute(` does not match `.setAttributeNS(`, and the control for that is worth
keeping: the gate as it stood reported `[OK]` on a planted
`element.setAttributeNS(null, "href", <document text>)`, which is the sufficiency claim
falsified by two letters. A namespaced call whose arguments the pattern cannot parse is
flagged rather than skipped, on the principle the rest of the file is about.

The **one exemption** in the tree is `a11y.ts`'s `createElement(elementFor(block.tag))` ---
the tag is the document's, the element name is not, because `elementFor` is total and
answers `p` or `h1`..`h6` for every input. The marker (`webview-sink-ok:`) is honoured on
the flagged line or the one immediately above it, since a justification that has to fit on
the end of the line it justifies gets written as "safe", which is not a reason. A marker
that ends up beside no finding is printed as a `[WARN]`: an allowlist entry naming something
that no longer exists is how an allowlist rots into a blanket permission.

**The backend half is enforced by the type**, and the two halves cannot see each other.
`outline.rs` refuses `/URI`, `/Launch` and `/GoToR` into `Target::Refused { action }`, whose
string is one of five literals chosen there rather than anything the document said, and
`no_target_variant_may_carry_a_url` matches `Target` exhaustively --- so adding a URL-bearing
variant is `error[E0004]`, not a red test. Read the two together: a grep over TypeScript
cannot see Rust, so a Rust change cannot turn the gate red, and that seam is residual risk 7.

The gate's own first version, shipped hours earlier the same day, is why this is spelled out:
it enforced only that an attribute *name* be a literal, while the threat model claimed
sufficiency from "every `setAttribute` passes a constant name, so there is no URL-bearing
attribute to poison" --- and `setAttribute("href", row.title)` satisfies both. Correct about
the tree in front of it, wrong about what it guaranteed.

**The Rust toolchain is pinned in `rust-toolchain.toml`** as of 2026-08-02, and the pin is
enforced by `scripts/check_toolchain.py` rather than assumed. `RUSTUP_TOOLCHAIN` overrides
that file silently, which is exactly what a CI action installing its own toolchain may set,
so both workflows use `rustup show` instead of one --- and the gate asserts the result. See
the trap of that name. Bumping the pin is a deliberate commit of its own; the cost of
pinning is that new lints and diagnostics wait for it, which is the point.

`--all-targets` covers test code, `-D warnings` makes lints
fatal, and `--locked` catches a `Cargo.lock` that was not committed after a `cargo update`;
dropping any of them silently weakens the gate. `--bins` is there because **none of the
others links a binary** --- clippy stops at metadata and `cargo test` links each `[[bin]]`
with `main` replaced by the harness's own, so a symbol reachable only from `main` is dropped
as dead code. That gap let a 7/7 sweep sit beside a failing `npm run tauri build`.

One honest note. The earlier plan listed `npm run lint` and `npm run test`, neither of
which existed; adding an ESLint config and a test runner with nothing to lint or test is
scaffolding, and the rule was that they land when there is something for them to check.
`npm run test` (vitest) landed on 2026-07-27, when command ranking gave it something ---
front-end logic with an answer that can be wrong rather than merely ugly. `npm run lint`
still does not exist, for the same reason as before.

**There is CI for ordinary commits as of 2026-08-02, and a release workflow since
2026-07-31.** This paragraph said the opposite for two days and the reasoning it gave was
half right, which is the more interesting half: the objection was never cost, it was that a
workflow restating the gate commands in YAML would be *a second place for the gate list to
live*. `ci.yml` does not restate them --- it invokes `scripts/gates.py`, exactly as
`release.yml` does --- so that objection never applied to the workflow that was eventually
written. What changed materially is that the repository went public, and macOS runner
minutes bill at 10x against a private allowance and are free here. The stated reason and the
operative reason were different, which is worth noticing: "one machine" was a description of
the circumstances, not an argument.

It runs on `pull_request` rather than `pull_request_target`, asks for `contents: read`, and
**references no secret** --- see the fork threat model under Repository facts, and the header
comment in the file, which is the copy that has to stay right.

What CI structurally cannot cover, and the reason `BUILD.md` still schedules them by hand:
`viewer_check.py` and `mutate_viewer.py` drive a real window and need an unlocked,
unoccluded screen, so on a headless runner they do not fail, **they hang** --- which is the
failure shape this repository is least able to read, since a hang and a pass both produce no
red. The mutation harnesses rebuild per mutation and take minutes.

`.github/workflows/release.yml` fires only on a CalVer tag, and it exists because **the
predicted trigger was the wrong one**. This file expected CI to arrive with the repo going
public or a second contributor; what actually forced it is that notarization needs a Mac,
an Apple Developer ID and API credentials, and cutting a signed macOS release by hand from
whichever machine happens to be free is precisely the step that should not depend on who is
sitting where. Worth recording because the prediction was confident and specific.

It **invokes `scripts/gates.py`** rather than re-listing commands in YAML, which was the
one instruction this file did get right --- a hand-copied command quietly loses a `--locked`
and then gates something weaker than the real gate.

**Nothing in its macOS half has ever run.** It is ported from `screenpick`'s working
workflow, and the one part with no precedent anywhere in the portfolio is signing the
bundled `libpdfium.dylib`: neither sibling ships a native library, and notarization requires
every Mach-O in the bundle to carry a Developer ID signature and the hardened runtime. The
dylib is therefore signed in `vendor/` *before* the bundler copies it, which is correct
whether or not Tauri re-signs nested resources. Its verification step is written to fail
rather than warn --- a skipped notarization exits 0 and produces an app Gatekeeper rejects on
any machine that has never seen it.

**Windows runs the viewer, and is contained.** On 2026-07-29 a Windows build opened documents
and passed `viewer_check.py` on four corpora --- **86 check names** on each, which was the
invariant *then*, with ran/skipped splits inside the ranges `BUILD.md` records; re-run on
2026-07-30 with pre-spawning live and still green.

The invariant is **109 names** now, and both platforms hold to it: Windows measured 109 across
six corpora on 2026-07-30, and macOS confirmed 109 on **all seven** on 2026-07-31 --- name sets
diffed pairwise and byte-identical, with every ran/skipped split matching `BUILD.md`'s table
exactly, from the fixed bundle. So the table needed no correction and the only stale numbers
were the ones in this file. A count written into prose goes stale the next time a check is
added, which is why the table is the one place it belongs; these two paragraphs are dated
statements about dated runs.

This paragraph said the opposite until 2026-07-30 --- "the platform is unsandboxed", "it fails
open" --- which had been true and was fixed the day before by the commit that selects workers
there. It is left recorded rather than quietly deleted, because it is the file contradicting
itself: the constraints section above had the corrected version the whole time, and a reader
who happened to start here would have concluded that hostile input is parsed in the app
process. **A document with two accounts of the same fact is worse than one with none**, and
the failure mode is that whichever section a reader reaches first wins.

**Nothing measurable is missing here as of 2026-07-31.** This sentence named `worker-bench`'s
seven POSIX modes, and only one of them --- `latency`'s per-tile overhead decomposition ---
measured anything no other harness covers. `latency-bench` covers it now, on both platforms,
through the production worker rather than a private POSIX one; see `docs/PLAN.md` §0 and
`BUILD.md`. `worker-bench` still refuses to run here, which is correct: a POSIX harness not
running on Windows was never the gap, only the measurement it held exclusively.

**The cross-check that portability was for has now run, and it paid.** `latency-bench`
executed on macOS 2026-07-31 (3/3, 3/3, 3/4+1 skip, exit 0; four mutations re-proved 4/4;
no sandbox font substitution) and was compared against `worker-bench --mode latency`, which
shares no worker code with it. They disagreed by an order of magnitude on the same quantity,
and the older harness was the wrong one: it baselines on a variant that never renders, so its
residual --- 46.7 ms on `vector-heavy`, against a printed 46.6 ms --- stays in the answer.
`worker-bench` now prints that residual and warns when it dominates, which is on every fixture
measured. The production worker's per-tile cost is **0.071--0.103 ms** on macOS, ~10x the
prototype's and still ~30x under the webview hand-off, so no conclusion moves. Two agreeing
harnesses would have proved less than these two disagreeing did.

The same sentence also claimed *"two `open_check.py`
phases whose route does not exist here"*, and both halves were wrong by 2026-07-31 --- the count
is **one**, and it is a decision rather than a gap, since Explorer hands the path over in `argv`
and the `argv` phase already covers it. The paragraph 290 lines above says exactly that (*"runs
five of six"*, *"the one phase that stays macOS-only"*), and `docs/PLAN.md` §0 agrees with it ---
so this was the same document contradicting itself that the paragraph immediately above warns
about, three paragraphs later, on a different fact.

Two things a green sweep still does not say, both learned the same day. `scripts/gates.py`
reported 7/7 while `npm run tauri build` failed, because nothing in the list linked a
binary --- there is a `bins` gate now, and it was proved to fail before being trusted. And a
`cargo build --release` binary is *not* a production build: the frontend is embedded by a
cargo **feature**, not by the profile. Both are in `docs/TRAPS.md`.

Every *measurement* in this file is macOS arm64 unless it says otherwise --- the pre-spawn
figures above are the first Windows ones, and they are labelled. The two platforms differ enough
on that measurement that carrying a macOS number over is a guess rather than an estimate.

**And the render constants are now measured on both.** `tile-bench` runs on Windows since
2026-07-30, and `docs/PLAN.md` §4's four architectural consequences reproduce there against the
same generated A0 fixture: spatial culling intact (a 256² tile is 3.8% of a full render, against
4.3% on macOS), a real per-render floor of **~1.3 s** against ~1 s, and a full page at **35.1 s /
88.3 s** for 1× / 2× against 22.8 s / 48.4 s. The ratios that drove the architecture hold; every
absolute number is **1.5--1.8× worse**, so a latency budget written against the macOS figures is
optimistic here by about a third. `BUILD.md` has the table, the caveats and the independent
cross-check that says the numbers are the document's rather than the harness's.

**So does the reason to have a pool.** `pool-bench` on the same page: **3.6× on six workers** and
nothing at eight, against 3.22× and nothing on macOS --- the same shape, the ceiling doing its job,
and six stable to 0.01× across two runs. The intermediate sizes are *not* stable enough to read
(pool 4 moved 1.99× → 2.29× between identical runs, and the per-round warm figures span ±20%), so
only the six and the flat eight are conclusions. `BUILD.md` says which is which.

---

## Known traps

Things already paid for once, or verified before writing code. Add to the list rather
than rediscovering.

**The entries themselves are in [`docs/TRAPS.md`](docs/TRAPS.md)**, under these exact
titles. Only the titles are here, because there are 220 of them and the full text
was 93% of this file --- an instruction budget spent on the 214 traps that are not
the one in front of you. Keep both numbers in this section current when adding an entry;
they have been two and then six behind before now, on 2026-07-28 and 2026-07-31 ---
which is how a count in prose fails, and why the authority is
`grep -c '^### ' docs/TRAPS.md` rather than this sentence. The *titles* have their own
authority now, and it is mechanical rather than prose: `scripts/check_trap_index.py`
diffs the set both ways and is one of the gates, so an entry added to one file and not
the other goes red instead of going unnoticed.
What the index has to preserve is knowing that a trap *exists*;
the paragraphs matter once you are in that area.

So: **before working in any area named below, read its entry.** A title is a claim, not
the lesson --- several of them are the opposite of what they sound like, which is why they
were written down. Grep the title in `docs/TRAPS.md`.

New traps go in `docs/TRAPS.md` with a line added here, in the same commit. That is a rule
with a gate behind it since 2026-08-02: `traps` in `scripts/gates.py` diffs the two as
**sets** and fails on either side having something the other lacks. The prose count above
still has to be moved by hand --- a number in a sentence is exactly what the gate does not
depend on.

**Code comments and the other documents say "`AGENTS.md` records ..." in about a hundred
places, and those references are still good** --- they were written when the entries lived
here, and they were left alone rather than rewritten, because a hundred-file mechanical
diff over prose carries more risk than the one hop it saves. Read them as naming the trap
index; the paragraph is in `docs/TRAPS.md` under the title.

### PDFium: rendering, mutation and page state
- PDFium: removed objects come back unless you regenerate the content stream
- Destroying an object removed from a page segfaults
- PDFium mutations regenerate page content wholesale
- `set_text()` silently draws `.notdef` when a glyph is outside the subset
- PDFium pays a large fixed cost *per render call*, not per page open
- PDFium parses a document lazily --- but enumerating pages is not lazy
- PDFium rendering *is* interruptible --- via the progressive API
- PDFium decides how often it can be interrupted, and the slice does not change it
- `FPDF_LoadPage` re-parses every time, and on a complex page that is 44 ms
- PDFium's render rotation composes with `/Rotate`, and wants the turned size
- PDFium accepting a file is not evidence the file is well formed

### PDFium: text, coordinates and outlines
- A byte scan cannot verify a document with a Type0 font
- The page break is whitespace, and concatenating two pages loses it
- A pattern over folded text has no lines, so `^` means the page
- `FPDFText_GetText` drops characters, so it cannot be indexed alongside boxes
- A page carries `/Rotate`, and PDFium answers in two coordinate systems at once
- A line-grouping rule assumes an axis, and the axis is not always vertical
- Two rotation tables, disagreeing at every turn but zero
- PDFium's character order is not the page's line order
- A dense page of uniform lines cannot detect a y-flip
- A comma opens a line of its own, and every space on the line joins it
- A paragraph is one mark and several text objects, and the gap between them belongs to neither
- `FPDFBookmark_GetDest` follows the bookmark's action without checking its type
- An outline can be infinite, and PDFium says so in its own documentation
- PDFium cannot create digital signatures

### Text matching, and scripts that are not English
- `FPDFText_GetUnicode` is a UTF-16 API, so an astral character is two characters
- A content stream has no bidi, so logical order draws right-to-left text backwards
- PDFium maps Arabic presentation forms to base letters, which was assumed to be false
- `ß` does not lowercase to `ss`, and the doc comment saying so stood for days
- A combining mark does not touch its own line, and a word with an ascender hides it
- With no `/ToUnicode`, PDFium returns plausible garbage rather than nothing
- A pattern was compiled case-sensitively against a haystack the fold had lowercased
- Two broken `/ToUnicode` entries can decode to one valid astral character
- A change predicted to fix three things fixed two, and the third was never the same problem
- PDFium normalises ligatures too, so the cost of case folding was smaller than stated

### The worker boundary, the sandbox and the pool
- macOS Vision cannot run in the parser worker's sandbox, and it aborts rather than refusing
- Printing maps a PDF parser into the app process, on both platforms
- `thread_safe` does not serialize PDFium --- there is no mutex, and threads crash
- A worker process is nearly free; the webview boundary is not
- macOS has no memory rlimit, and `RLIMIT_CPU` is a lifetime budget
- Polling a child's footprint bounds a leak, not a burst
- `proc_pid_rusage` takes the struct's address, not a pointer to it
- The vendored PDFium has no JavaScript engine and no XFA --- verify it, do not assume it
- The no-V8 property is one word in a URL, so the fetch asserts it
- A symbol scan needs symbols, and the Windows PDFium has none
- PDFium ships its loadable library in a different directory on Windows
- A sandboxed PDFium substitutes fonts silently --- and the obvious fix does not work
- The linker's image table is an observable; a milestone of ours is a claim
- A Rust process absorbs the first SIGSEGV you send it
- A released id must leave a hole, because removing it renumbers the rest
- Two copies of a distinction drift, and a mutation of one survives
- Dropping the owner does not close a pipe something else has cloned
- A descriptor without `FD_CLOEXEC` leaks into every later child, and keeps it alive
- Two mechanisms with the same limit make one of them untestable
- FIFO dequeue is not FIFO completion
- A worker killed a moment ago still says it is running
- The cleanup after an fd shuffle can close what it just installed
- A per-page invalidation counter is not the same as a generation

### The document model: saving, structure, signatures
- Redaction conflicts with incremental save --- and a full rewrite is not sufficient either
- Digital signatures constrain what may be edited at all
- Whether `/Annots` is an indirect array decides how large an annotation edit is
- Embedded fonts are subsetted
- `lopdf`'s object collection is quadratic, but the algorithm is not
- `lopdf` silently drops encryption on save
- An incremental save is cheap on disk, not in memory --- and its cost is the parse
- An object a prior revision overwrote is reachable by no parser
- A decompression bomb costs QPDF CPU, not memory — and `lopdf` neither

### Tauri, the webview and startup
- `AppHandle::exit` does not set the process's exit code
- `RunEvent::Opened` fires before the setup hook, so managed state is not there yet
- A raw `cargo build` binary runs no webview content at all
- A page that never ran looks exactly like one that ran slowly
- WKWebView presents at 59 Hz on a 120 Hz display
- `performance.now()` is clamped to 1 ms — average, do not take a median
- Never benchmark through `tauri dev` without `--release`
- Startup has three regimes, and two of them are the OS, not us
- The shell floor is ~250 ms, and no lever on our side moves it
- A webview's first custom-protocol request costs ~45 ms, whichever request it is
- Tauri creates config windows *before* the setup hook, hiding the webview's cost
- A page whose window is not visible is suspended --- so a JS watchdog cannot fire either
- A refusal in the setup hook cannot speak, so it must happen before the event loop

### Rust and macOS
- A locked macOS session cannot be unlocked from a script, so it must be prevented
- `Instant` on Apple Silicon ticks at 41.67 ns, so "elapsed == 0" is reachable
- `evict_page` can dangle a live `RawPage`, and the borrow checker allows it

### Measuring: what a number can and cannot say
- A documented count that is one sample of a race makes an honest run look like a defect
- Two counts from two commits are not a platform difference
- A baseline that skips the expensive step leaves its noise in the answer
- A difference is only a measurement when the operands make it one
- A check on the sign of a noisy quantity fires only when the noise falls one way
- A mean cannot test a claim about a minimum
- A frame-rate pass means nothing without a coverage number beside it
- Interleaving controls for drift, not for what the last variant left behind
- Three similarity metrics in a row, each unable to see its own failure
- A timer that starts after the setup measures the wrong thing, and reports it
- `cargo test` is a debug build, and a debug number in a doc comment is a lie

### Writing a check that can fail
- Break the code on purpose, or the test suite is decoration
- A control that is easier than the check certifies nothing
- An OCR engine's bounding box is a detection, not a measurement
- A property that holds by construction cannot test the thing it resembles
- A fixture the library itself wrote cannot tell a passthrough from a rewrite
- An oracle more forgiving than the thing it stands in for cannot fail
- A writer and its own reader agree about a document that is wrong
- A reply parsed as the wrong shape reads as absence, and absence is the reassuring branch
- A canvas round trip cannot read back what a renderer produced
- A dependency that refuses your test input makes your own guard look redundant
- A defect that switches off a check's precondition is not caught by that check
- An "already have it" cache needs an in-flight set, not just the cache
- A text comparison cannot see a property that is not about text
- A selector naming one element stops reading the page when the layer gains another
- A test whose precondition is already satisfied never runs
- A crash test that compiles away proves containment of a crash that never happened
- A test for an atomic write must plant the intermediate it is meant to prove
- A control can be contaminated by the phase that ran before it
- A check that derives its inputs from the thing it is testing cannot fail
- A closure and a direct read of the same variable disagreed, and it is unexplained
- A mirror of the DOM's focus goes stale, and Enter activates the row nobody is on
- A page fitted to the element's own width is measured under the scrollbar
- A synthetic heading that does not reach the second column tests nothing
- Whatever a fixture is meant to discriminate, it needs two of
- A leak no behaviour can see needs an accounting observable, not a cleverer assertion
- An outcome two mechanisms can produce cannot test either one
- A length bound cannot be tested by the verdict it produces
- A check nested inside a lookup for the thing under test disappears with it
- A check whose failure mode is a wait cannot fail
- A test whose failure is a hang reports a pass and a timeout in one breath
- An unreachable guard is worth keeping if the type can carry it instead
- A label rendered only from real ids cannot be tested on a combination none of them uses
- A post-destroy guard that returns early leaks what it declined to take
- A print check that counts pages cannot see a blank page
- A page count read too early is 0, and 0 is not a count
- A DIB pixel is not a device unit, and every page printed at half size while a check passed
- A tolerated gap in the input becomes a hole in the output
- A test cannot see the direction of an attachment it puts in index order
- A guard for "more than one page" is not a guard for "a page that can be reached"
- A wrap is correct when there is nothing ahead, so the check cannot fire
- A check with no precondition reports a sparse fixture as a defect
- A test that refuses an empty fixture set is what makes CI's absence visible
- A feature made a standing check false, and the only corpus that could tell had never been opened
- A negative assertion needs an observable saying the question was asked

### Harnesses: running checks and reading what they print
- A mutation harness needs the same control as the thing it is testing
- A timeout that discards the transcript recreates the failure it was added to diagnose
- Restoring a mutated file by *moving* a backup over it tests the mutated binary (the title names the wrong mechanism --- see the entry below it)
- A harness that prints only at the end cannot say where it stopped
- A harness that prints as it goes writes nothing until it exits, under a redirect
- A mutation harness that dies leaves the mutation in the tree
- A verification chained after a failed edit reports success for work that is not there
- A restored file with its original timestamp leaves the build serving the mutation
- Three mechanisms, no checks: measure what a commit's tests can actually see
- A verdict that reads a timeout as "no result" throws away the finding
- A mutation naming a test the harness cannot run reports SURVIVED
- A mutation that survives may be a variant, not a gap --- check before strengthening
- A leaner data structure turned a wrong edit into a no-op
- A harness that prints stderr only on failure hides what a passing run said
- A wrapper's own verdicts are on the other stream, in the same shape as a check's
- A mutation aimed at a check that skips reports SURVIVED
- A mutation caught by an access violation produces no test results at all
- A guard that also guarantees termination fails as a hang, not as a red test
- A comment claimed an ordering mattered, and the mutation that should have hurt did not
- `caffeinate <utility>` becomes a child of the utility, so a child count counts it
- Repeating a race inside one process re-runs the first round, not the race
- A precondition that names the cause still lets the symptom print
- A text-mode restore is not a byte restore, and the locale codec cannot even read the file
- A gate's static reason turned a crash into a wrong diagnosis, twice over
- A decoder told to replace what it cannot read does, and the result ships
- A harness that synthesises input must reset the input's own state machine
- The last page cannot reach the top of the viewport
- An expected error line beside a passing suite makes a green run unreadable
- A harness that cannot read a script skips, and blames the fixture
- A check name that is a prefix of another cannot be aimed at
- A mutation aimed at code no fixture reaches survives, and the fix is not a new corpus
- A harness sliced a code-point index with `String.prototype.slice`
- A measured string transcribed off a terminal loses what the terminal does not draw
- A mutation aimed at one branch when the fixture only reaches the other
- A snapshot taken after the first mutation restores the mutation, and verifies itself clean
- A rewritten line leaves a mutation aimed at nothing, and only the harness says so
- A stream split done for the failing direction leaves the passing one where it was

### Windows and portability
- The gates had never run on the platform where they fail
- A document meant to cover both platforms was generated from platform-specific inputs
- A crate-root `#![cfg]` empties a `[[bin]]`, and cargo reports a missing `main`
- An uninhabited type carries its impossibility into every caller
- A `null` that means "inferred" is not a `null` that means "unknown"
- A directory that exists is not the library you need
- A list of documented blockers can be wrong in the direction that looks thorough
- A gate list that never links a binary cannot see a link error
- A pin that nothing verifies is indistinguishable from no pin
- A toolchain pin can match on version and still be the wrong ABI
- A custom URI scheme is not spelled the same way on every platform
- One constant standing for two platform distinctions breaks the moment they diverge
- A release build is not a production build; a cargo *feature* decides that
- A guard that degrades to a no-op off its platform stops being a guard
- `CreateProcessAsUser` waives a privilege only for a token it still recognises
- A restricting SID stops the loader, and the code never runs
- One failing rung cannot say which ingredient failed
- A verdict that takes the last row that worked recommends the weakest one
- A refusal that exists because nobody wrote the code is not a guarantee
- The kernel refuses a writable mapping of a read-only file, on both platforms
- "Inherit nothing" cannot be spelled as an empty handle list
- A safe function taking a raw `HANDLE` has an unstated contract, and clippy says so
- `GetExitCodeProcess` reports 259 for a live process, and 259 is a legal exit code
- A pipe reaches EOF before the process it belonged to is signalled
- `eprintln!` is not one write, and every worker shares the parent's stderr
- A test whose child never answers cannot see the pipes being crossed
- A wait for a condition that cannot hold spends its whole bound, and retires the pool it was about to measure
- A check that wins a race on one platform has not been shown to pass on it
- Single-instance turns a stray process into a launch that succeeds and does nothing
- A `DataWriter` closes the stream it was created over, so a helper that returns the stream returns a closed one
- WinRT reports a PDF page's size in DIPs, not points
- A BMP's DIB header is never 4-byte aligned, so reading it in place is undefined behaviour
- A print DPI relative to the page is the wrong quantity, and A4 is the example that hides it
- The OS's PDF rasteriser is not fast, and a raster print path inherits that
- A directory under `src/bin/` becomes a phantom binary in the Windows installer
- A trailing slash in a Tauri resource map is a rename on macOS, not a directory
- An interpolated status label is two columns narrower when it passes
- A green gate list can sit beside a distributable that cannot be built
- A bundled app that finds its library in the dev tree proves nothing about the bundle
- Moving a binary out of the installer moves it out of the gate that links it
- `cargo fmt` was blamed for mangling a string, and it was innocent

### Fixtures
- The test fixtures are generated, not committed
- A stand-in glyph with a degenerate box measures the wrong rule
- A fixture's self-check forbade its own finding

### Documents as controls
- A mitigation present and disclaimed is quieter than one claimed and absent

## Repository facts

- GitHub: `tstone-1/tpdf`, **public**, MIT (`LICENSE`).
- Public since 2026-08-02, and it needed no history scrub: all 108 commits across every
  ref were authored and committed as `48162401+tstone-1@users.noreply.github.com`, there
  were no tags, no `refs/pull/*`, no forks and no workflow run logs to become visible.
  That is the cheap case, and it held only because the clone was made with a repo-local
  identity --- a fresh clone on the Windows flat layout has no `includeIf` rule and would
  silently commit under a work address. Set `user.email` / `user.name` repo-locally there.
- **The `APPLE_*` secrets survive the flip; a workflow that reads them must not.**
  Repository secrets are not exposed by making a repository public, but fork pull requests
  now exist. `release.yml` is tag-push-only and therefore unreachable from a fork; `ci.yml`
  references no secret, runs on `pull_request` rather than `pull_request_target`, and asks
  for `contents: read`. Keep that split --- it is the whole of the fork threat model.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` --- anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
