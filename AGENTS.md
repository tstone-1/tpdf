# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

---

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

## Hard constraints

### Licensing: permissive dependencies only

**No AGPL or GPL dependencies. Ever.** This is a deliberate, load-bearing decision, not
an accident of what was convenient.

MuPDF (what SumatraPDF uses) is the obvious engine and was rejected. It is dual-licensed
AGPL / commercial, and the AGPL path costs three things that matter here:

- **It is viral across all of tpdf.** Every line of Rust and Svelte becomes AGPL.
- **It would forbid reusing tpdf code in private or work repositories.** Lifting tpdf's
  text extraction or page-splitting into a Nexperia tool that processes customer
  declarations or IMDS documents would require AGPL-ing that tool, which is impossible.
  This is the cost that actually bites, given the surrounding portfolio.
- **It would make relicensing later impossible** without an Artifex commercial licence
  (quoted case by case, $1,500 to $50,000+).

It would also rule out the Mac App Store, whose terms conflict with the GPL family.
Direct notarized distribution (what `screenpick` does) is unaffected.

The repository is currently **private**. Because every dependency is permissive, it can
be flipped to public at any time with no licensing work. Do not introduce a dependency
that removes that option. If a copyleft library ever looks necessary, raise it as a
decision rather than adding it.

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
reason, and so must tpdf.

Non-negotiable: parsing and rendering happen in **worker processes** with no filesystem or
network authority, under resource and time limits, restartable on crash. Document
JavaScript and launch actions are **disabled by default**. All `lopdf` stream decoding is
bounded. This is a Phase 0 concern, not a hardening pass to be done later --- retrofitting
a process boundary is an architectural rewrite.

This constraint is load-bearing in a second way: because in-process PDFium is serialized
behind a global mutex (see Known traps), worker processes are also the *only* route to
parallel rendering. Security and performance want the same architecture.

`docs/THREAT-MODEL.md` is the worked-out version: what is being defended, the trust
boundaries, each threat against the evidence that it is handled, the sandbox profile in
full, and the residual risks in one list. Every claim there is either measured with the
spike named, or marked untested --- keep it that way when adding to it.

---

## Stack

The **shell is settled**; the **PDF layer is provisional until Phase 0 proves it** (see
`docs/PLAN.md` §9).

| Layer | Choice | Status |
|-------|--------|--------|
| Shell | Tauri 2 | Settled |
| Frontend | Svelte 5 (runes), TypeScript `strict: true`, Vite | Settled |
| Backend | Rust | Settled |
| Platforms | macOS + Windows | Settled |
| Rendering + text extraction | PDFium via [`pdfium-render`](https://docs.rs/pdfium-render) (BSD-3-Clause) | Provisional |
| Object graph + content streams | [`lopdf`](https://docs.rs/lopdf) (MIT) | Provisional |
| Hardened structural rewrite | [QPDF](https://qpdf.readthedocs.io/) (Apache-2.0) | Candidate |

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI.

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

`BUILD.md` will carry the release checklist. It must state every CI gating command
**verbatim, with flags** --- a checklist weaker than the gate it exists to satisfy buys
false confidence and goes red after the release is cut.

Planned gates (to be mirrored exactly in `.github/workflows/ci.yml`):

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
npm run check          # svelte-check + tsc
npm run lint
npm run test
```

Note `--all-targets` (covers test code), `-D warnings`, and `--locked` (catches an
uncommitted `Cargo.lock` after a `cargo update`). Dropping any of those silently tests
something weaker than CI does.

---

## Known traps

Things already paid for once, or verified before writing code. Add to this list rather
than rediscovering.

### PDFium: removed objects come back unless you regenerate the content stream

After `FPDFPage_RemoveObject` (or any page-object mutation), you **must** call
`FPDFPage_GenerateContent()` before saving. Otherwise the original content stream is
written out unchanged and the removed object is still in the saved file --- it looks like
the edit worked, because the in-memory page reports the object gone. This is
[pdfium issue 1051](https://groups.google.com/g/pdfium-bugs/c/RBwhmdbejRk).

For redaction this is not a cosmetic bug, it is a data leak. It is the single strongest
argument for the mandatory post-save verification pass described in `docs/PLAN.md`.

Note also that `FPDFPage_RemoveObject` is marked Experimental API upstream. Pin the
PDFium build and re-test object removal after any bump.

Confirmed still true on chromium/7881 by spike 0.3: saving after `set_text()` without
regeneration changes exactly zero pixels and leaves the original string in the stream.

### Destroying an object removed from a page segfaults

`fpdf_edit.h` says `FPDFPage_RemoveObject` transfers ownership to the caller and that
`FPDFPageObj_Destroy()` frees it. `pdfium-render`'s `Drop` does exactly that, and it
**segfaults inside `FPDFPageObj_Destroy`** --- for text and path objects alike, and whether
the destroy happens immediately, after `regenerate_content()`, or after the save. The fault
is a bad vtable dereference, i.e. the handle is already dead.

`std::mem::forget` on the returned object is the only safe route through this binding; the
memory is reclaimed when the document closes. Removal is redaction's primitive, so this is
not an edge case --- it is on the main path.

`src-tauri/src/bin/remove_probe.rs` is the minimal repro, kept as a standing regression:
case `c` (leak) must pass, and if case `a` (destroy) ever starts passing, the upstream bug
is fixed and the `forget` can go. Re-run it after any `pdfium-render` or PDFium bump.

Beware the shape of this bug when diagnosing it: piping the run through `tail` reports
`tail`'s exit status, so a segfaulting binary looks like a clean exit. Check `$?` on the
program itself.

### PDFium mutations regenerate page content wholesale

PDFium's page-object edit path does not splice content streams; it regenerates them.
Consequence: **any page-object edit reflows the entire content stream**, so byte-level
diffs of a page are meaningless, round-tripping a page through PDFium is not lossless,
and "the file changed" is not a usable edit-detection signal.

Measured in spike 0.3, editing one text object on a four-line page: `Td` became `Tm`, `Tj`
became `TJ`, every run was wrapped in `q`/`Q` with explicit `rg`, `RG` and `0 Tr`, an
ExtGState `/FXE1 gs` was introduced, `/F1` was renamed `/FXF1` --- and the marked-content
span around the target was **discarded, `/ActualText` with it**. Every other text object on
the page was rewritten too, though none had been touched.

The page rendered **pixel-identical** through all of that. So the rule worth carrying:
**a clean visual diff is not evidence of a faithful edit.** Tagged structure, accessibility
text, optional-content membership and marked-content property lists can all be gone while
every pixel matches. Anything that must survive an edit needs its own assertion.

This is a property of PDFium, **not of PDF**. Surgical operator-level rewriting is
entirely possible --- tokenize the stream, remove or replace selected operators, re-encode
--- and `lopdf` exposes decoded page content as a sequence of operations for exactly this.
Spike 0.3 did it, and it preserved everything PDFium's regeneration destroyed.
What is genuinely hard is mapping a PDFium page object back to the exact operator range
that produced it while preserving graphics and text state. Where surgical precision is
required (redaction), go through a content-stream interpreter, not through PDFium.

### `set_text()` silently draws `.notdef` when a glyph is outside the subset

`PdfPageTextObject::set_text()` takes a Unicode string and re-encodes it into the object's
font. When the font is embedded and subsetted --- i.e. almost always --- characters absent
from the subset have no glyph, and PDFium **returns success anyway**. Measured in spike
0.3, replacing text with `QUARZ ÜBERPRÜFT` where seven of its characters are absent:

- **Type0 / Identity-H:** every missing character encoded as **glyph 0**, `.notdef`.
  Renders as boxes; text extraction returns neither the old string nor the new one.
- **TrueType simple font:** the correct WinAnsi *codes* were written for glyphs that do not
  exist. Renders as jammed-together fragments (the missing codes carry zero advance) while
  **text extraction returns the requested string in full**. Displayed text and extracted
  text disagree --- a search hit on text nobody can see.

Both are silent. Neither is detectable from the return value, and the second is not
detectable from a pixel diff either.

Working in **code space** rather than Unicode makes the condition visible: build the
character-to-code table from the object's existing operand and its extracted text, and a
character with no entry is exactly a character with no glyph. Refuse and report it (see
`docs/PLAN.md` §7 point 6) rather than emitting something. Silent substitution is what
makes the competition untrustworthy; this is where tpdf would acquire the same habit.

### A byte scan cannot verify a document with a Type0 font

Under Identity-H the content stream carries **glyph ids, not text**, so a secret drawn on
the page is never present in the file as its own bytes. A `strings`- or `grep`-shaped leak
check therefore reports *clean* on exactly the documents most modern producers emit.

This is not hypothetical: spike 0.3's own leak scanner did it, calling the CID fixture
clean while text extraction proved the needle was still there. It now reports **not
verified** whenever the document contains a Type0 font.

The general rule, and it is the same one `docs/PLAN.md` §6 states for redaction: a verifier
must decode each carrier **in that carrier's own encoding**, and a carrier it cannot decode
makes the result "not verified", never "clean". "Grep found nothing" is not evidence.

### PDFium is serialized behind a global mutex --- threads buy nothing

Upstream PDFium makes **no thread-safety guarantee at all**, and its authors recommend
parallel *processing*, not multi-threading. `pdfium-render`'s `thread_safe` feature is
**on by default** and achieves safety by locking every single PDFium call behind one
mutex. Multiple `FPDF_Document` handles in one process therefore render strictly
sequentially --- opening more handles buys crash-safety, not parallelism.

Consequences, which drive the whole architecture:

- In-process parallel tile rendering is not achievable. Parallelism requires **separate
  worker processes**.
- One pathological page (huge CAD drawing, transparency groups, Type 3 fonts) holds the
  mutex and starves every other render in the process.

An earlier version of this file claimed PDFium was unsafe only "per document handle" and
that multiple handles would render in parallel. That was wrong; it was caught in the
2026-07-26 plan audit before any code depended on it.

Spike 0.5 measured what worker processes actually buy: near-linear speedup to the
**performance**-core count (3.89x on four, on a 4P+6E machine), then about 0.4x per further
worker. Size the pool from performance cores, not `hw.ncpu`.

### A worker process is nearly free; the webview boundary is not

Measured 2026-07-26 (`worker-bench --mode latency`). A control round trip to a worker ---
write a JSON line, wake it, read a JSON line --- is **6 µs**. Moving a 4 MB tile out of it
costs **0.11 ms through shared memory** and 0.61 ms down a pipe. The shared-memory figure
is indistinguishable from the in-process residual, and one worker matches the in-process
baseline for throughput exactly.

Two things make it that cheap, and both are worth keeping:

- **PDFium renders straight into the shared mapping** via `PdfBitmap::from_bytes`, so
  there is no copy on the worker side at all. `as_rgba_bytes()` would allocate and copy a
  second 4 MB; the BGRA-to-RGBA swizzle is 0.27 ms and is better done in place.
- **The mapping is an unlinked temp file passed by descriptor**, `dup2`'d to a fixed fd
  before `exec`, not a `shm_open` name. A descriptor has no name to guess and survives a
  policy that denies opening files --- which is what lets the worker be sandboxed at all.
  Note `dup` both sources to fresh descriptors before `dup2`ing them down: the parent's own
  mapping files typically land on exactly the fd numbers being targeted.

Put next to §3's other measurement --- 3.0 ms to hand the same 4 MB to the webview --- the
process boundary costs about 1/27th of the UI boundary. Isolation is not where the time
goes.

### macOS has no memory rlimit, and `RLIMIT_CPU` is a lifetime budget

Measured 2026-07-26, and confirmed independently through Python's `resource` module:
`setrlimit` on macOS refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright with
`EINVAL`. There is no address-space or heap bound available this way at all. `RLIMIT_CPU`,
`RLIMIT_NOFILE` and `RLIMIT_FSIZE` are accepted and do fire.

The subtler half: `RLIMIT_CPU` counts CPU consumed over the **process lifetime**, not per
request. Under a 3 s limit a 1.72 s render succeeds and the next one dies 1.30 s in, at a
cumulative 3.0 s (SIGXCPU, signal 24). So it bounds how long a worker may live, and a
per-render bound has to come from the parent's own deadline and kill --- measured at 1.2 ms
to kill and reap, 4.8 ms to respawn.

Always read a limit back after setting it. A limit the kernel accepts and never enforces is
worse than none, because it reads in the source as a bound that exists.

### Polling a child's footprint bounds a leak, not a burst

The substitute for the missing memory rlimit is supervision: the parent samples the worker's
`ri_phys_footprint` through `proc_pid_rusage` and kills it over budget. A sample costs
0.33 µs, so the poll interval trades overshoot against essentially nothing, and the obvious
reading is that a tight interval solves it. Measured 2026-07-26
(`worker-bench --mode footprint`), against a child allocating at ~22 GB/s and a 128 MB
budget: 1 ms polling overshoots 16 MB, 5 ms overshoots 22 MB.

The result that matters is the one that is not an overshoot at all. **At 20 ms and above,
most runs never saw the event** --- the child took its whole 512 MB burst and exited between
two samples, so supervision never engaged and reported nothing. A burst smaller than
interval × growth rate is invisible to any polling scheme, whatever the interval. Bounding
the *inputs* --- decompressed stream size, tile dimensions, pages per request --- is the
layer that catches those, and it is not optional.

Two smaller traps in measuring this. Neither the median nor the worst *observed* overshoot
is the bound; both depend on where the crossing falls between two samples, so a budget must
be set from interval × rate. And a zero interval is not free supervision --- it burns a core,
and the low overshoot it appears to buy is partly the child being starved of the CPU it was
allocating with.

Use footprint, not RSS: a footprint excludes clean file-backed pages, so a worker with a
337 MB document mapped is not charged for it, and an RSS bound would kill a worker for
reading its own input.

### `proc_pid_rusage` takes the struct's address, not a pointer to it

`rusage_info_t` is itself `void *`, so the declared `int proc_pid_rusage(int, int,
rusage_info_t *)` reads as taking a pointer to a pointer and does not. Every caller in the
SDK writes `(rusage_info_t *)&info`, passing the struct's own address.

Passing the address of a pointer instead **type-checks cleanly in Rust, returns 0, and has
the kernel write the whole struct over whatever follows that pointer on the stack.** The
only symptom was a footprint that read as zero for every child, which looks exactly like a
permissions problem and sends you off to check entitlements. Note also that `RUSAGE_INFO_V0`
is the oldest flavour carrying `ri_phys_footprint`, so it is the one least likely to shift
under a macOS update.

### The vendored PDFium has no JavaScript engine and no XFA --- verify it, do not assume it

`bblanchon/pdfium-binaries` ships a build with **zero `v8::` symbols**, no real
`CJS_Runtime` (only `CJS_RuntimeStub`, whose `ExecuteScript` disassembles to three
instructions that zero the output and return), and **zero `CXFA_` symbols**. So "document
JavaScript is disabled by default" understates the position: there is no engine to disable,
and the XFA refusal is a property of the binary rather than a policy that can be forgotten.

Both are properties of *that build*, not of PDFium, so they must be re-checked after every
bump --- `worker-bench --mode engine` does it. Note it cannot be tested behaviourally: a
document whose JavaScript does nothing looks exactly like one whose JavaScript never ran, so
the absence of an effect is not evidence of the absence of an engine. The symbol table is
the only thing that discriminates, which is why the check needs its own controls --- one
that the file really is PDFium, one that its local symbols survived stripping. Without the
second, every absence is "not verified", not "not present".

What this does *not* buy: `pdfium-render` calls `FPDFDOC_InitFormFillEnvironment` on every
document open, so the form-fill machinery is reachable parsing surface even with nothing
behind it.

### A sandboxed PDFium substitutes fonts silently --- and the obvious fix does not work

A worker under `sandbox_init` with files and network denied still opens the document (it
arrives as a mapped descriptor, never a path) and still renders. On a document with
**embedded** fonts the output is pixel-identical to an unsandboxed render. On a **base-14**
document it is not: PDFium returns success and draws a different face, with almost exactly
the same amount of ink --- so it is a substitution, not a blank page, and nothing in the
return value says so.

The repair that looks right is wrong. Denying `file-read*` and allowing `file-read*` back
on `/System/Library/Fonts` still renders differently, because what the font lookup needs is
**metadata** reads across the filesystem, not data reads from the font directories. What
works, verified pixel-identical on base-14, TrueType, CID and a 775-page corpus:

```
(version 1)
(allow default)
(deny network*)
(deny file-write*)
(deny file-read*)
(allow file-read-metadata)
(allow file-read-data (subpath "/System/Library/Fonts") (subpath "/Library/Fonts"))
```

The residual is that a hostile document can learn which paths exist; it cannot read one,
write one, or open a socket.

The general rule, and it is the third time this shape has appeared: **verify a sandbox by
comparing pixels, never by checking that the render returned `ok`.** Same silent
substitution as `set_text()` drawing `.notdef`, arriving from a completely different
direction.

Bisect an SBPL profile rather than reasoning about it --- `worker-bench --profiles` accepts
raw SBPL so a policy can be narrowed from the shell. Note also that `sandbox_init` denials
do not appear in the unified log without an explicit report clause, so "no log entries" is
not evidence that nothing was denied.

### A crash test that compiles away proves containment of a crash that never happened

`worker-bench --mode crash` originally faulted with `null_mut::<u8>().write(1)`. That is UB
the optimizer is entitled to delete, and in release it did: the process exited normally
through the fallthrough arm, the parent reported clean containment, and the run looked like
a pass. The tell was the epitaph --- "exited with code 9" where a segfault should have said
"killed by signal 11".

Route the address through `std::hint::black_box` and use `write_volatile`. More generally:
a test whose failure mode is *not failing* needs its own assertion on how it failed, not
just on the outcome.

### Never benchmark through `tauri dev` without `--release`

`tauri dev` shells out to `cargo run` in the **dev profile**. PDFium arrives as a prebuilt
optimized dylib, so it is barely affected --- but every line of our own Rust is
unoptimized. The result is not uniformly slow, it is *selectively* slow, which is worse:
ratios between our code and PDFium's are inverted rather than merely inflated.

Concretely, 2026-07-26: PNG encoding of a 1024² tile measured **67 ms** under `tauri dev`
and **1.41 ms** under `tauri dev -- --release`, a 48x difference, while the PDFium render
alongside it moved only 1.39 -> 1.36 ms. Read naively, the debug run said encoding cost
4813% of rendering; the truth was 91%. A conclusion had already been drafted from the
first table.

Also note `tauri dev` runs a bare `cargo run`, which fails with "could not determine which
binary to run" as soon as the crate has more than one bin. `default-run = "tpdf"` in
`[package]` fixes it.

**Startup timing is worse still: `--release` is not enough, it needs a bundle.** Under
`tauri dev` the frontend is served by a Vite dev server over HTTP, so a startup measurement
describes Vite's module graph, not the app. Build with
`npm run tauri build -- --bundles app` and run the executable inside the `.app` directly
(`target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`) --- that keeps stdout and the
environment, which `open -a` does not.

### PDFium pays a large fixed cost *per render call*, not per page open

Measured 2026-07-26 (`src-tauri/src/bin/tile_bench.rs --mode single`). On a complex page
--- an A0 sheet with ~200k path segments --- every render call pays roughly **1 second**
before any area-proportional work, and this is charged per *call* even when the same
`PdfPage` object is held across all of them. Rendering that page to a 150 px-wide
thumbnail (1/270th of its 1x pixels) still costs **1.52 s**; a single 256x256 tile costs
0.98 s; the full page costs 22.8 s at 1x and 48.4 s at 2x.

PDFium does cull spatially --- a 256² tile costs 4.3% of the full page while covering 0.8%
of its area --- so tiling is not futile. But the cost does not approach zero as the
request shrinks, which has three consequences worth stating plainly:

- **Small tiles are a trap.** Tile count multiplies the constant. Covering a 1920x1080
  viewport of that page takes 39 s in 256² tiles and 18 s in one 2048² tile. Prefer the
  fewest, largest tiles the memory budget allows; 1024²--2048², not 512².
- **A cheap low-resolution placeholder is not cheap** on the pages that most need one.
  Render it once at document open, off the critical path.
- **Any "just re-render it smaller" fallback does not help.** Scale is the wrong lever;
  spatial extent is the only one that moves the number.

Measure with interleaved variants (A,B,A,B across rounds, compared pairwise within a
round). Round 0 is a consistent warm-up outlier on cheap pages --- 0.895 against a 0.207
steady state in one text-page series --- and vanishes once renders take seconds.

### Startup has three regimes, and two of them are the OS, not us

Measured 2026-07-26 on the M5 MacBook Pro, release bundle, 775-page document, first page
presented (`scripts/startup_bench.py`):

| regime | to first page | pre-`main` |
|---|---|---|
| warm | 374 ms | 4.2 ms |
| cold page cache (`--purge`) | 1562 ms | 217 ms |
| cold + never-seen binary | ~1.8 s | 444 ms |

Two separate OS costs are hiding in there, and they are additive:

**Paging the frameworks.** `main` to the Tauri setup callback is 142 ms warm and **951 ms
cold**. That single interval is the whole cold penalty. Nothing of ours runs in it; it is
Tauri and WebKit being read off disk. Cold start is therefore mostly a function of how much
framework code is *linked*, not of what the app does.

**Validating an unseen code signature, ~300 ms, charged per binary identity.** First launch
of a fresh build spends 444 ms before `main`; relaunching the same binary spends 4.2 ms.
Copying the bundle to a new path --- same bytes, same warm page cache, only the file
identity is new --- reproduces it at 299 ms, and relaunching *that* copy costs 4.8 ms.

Consequences worth keeping:

- **A cold-start budget cannot cover the signature cost,** because it is spent before any of
  our code runs. State startup targets as warm and report the other two regimes separately.
- **It recurs on every update,** not just on install --- a new build is a new identity.
- **It makes "first run after build" a useless benchmark sample.** Every rebuild pays it, so
  the first run of a series is systematically ~300 ms slow for reasons unrelated to the
  change being measured. Discard it or report it separately.
- **Do not read a cold number as an I/O problem to cache away.** Page geometry enumeration
  costs 86.5 ms cold against 85.9 ms warm --- identical, i.e. pure CPU. Compare the two
  columns before concluding anything about where time goes.

Note `--purge` needs root, so an unattended cold run needs sudo credentials arranged for
`purge` on the machine doing the measuring.

### The shell floor is ~250 ms, and no lever on our side moves it

Measured 2026-07-26 with interleaved variants (`scripts/startup_bench.py --variant`), three
runs of 8 rounds. Of a 368 ms warm start, the first line of application code runs at
~247 ms, and that interval is seven Tauri/WebKit intervals of which **none is ours**: 4 ms
dyld, 30 ms `Builder::build()`, 25 ms `App::run` prologue, 78 ms
`WebviewWindowBuilder::build()`, 58 ms HTML load, 48 ms to the first completed IPC.

The two obvious levers were tested and neither is one:

- **Deleting the entire frontend framework is worth nothing.** A variant doing identical
  work in one inline script --- no module graph, no Svelte, no `@tauri-apps/api` --- measured
  −8.4, +9.9 and −0.2 ms across the three runs. See the next entry for why.
- **Creating the window in the setup hook rather than from `tauri.conf.json` is ±1 ms.**
  The cost is the WKWebView, not when it is asked for.

Two things *were* reducible, and both were defaults rather than requirements:

- **The 86 ms page-geometry walk**, which is ours. Two routes exist --- collect lazily, or
  collect during the shell's boot before the webview can ask --- and they are
  **alternatives, not complements**: both delete the same 86 ms, so doing both measures the
  same as doing either. Lazy is the better one; it needs no path known at launch.
- **Tauri's default macOS menu, ~16 ms.** Split across `Builder::build()` (39.7 → 32.9) and
  the `App::run` prologue (94.0 → 87.8), so looking in one place would have missed most of
  it. An empty menu is not shippable --- Cmd-Q, Cmd-W and clipboard shortcuts live there ---
  so the lesson is to build the menu the app needs instead of accepting the default.

Together they take 368 ms to **276 ms**. Note the menu effect is clean against the
low-variance `lazy` variant (negative in 7/7 rounds) and reads as −9 ms with a range
spanning zero against the noisier baseline: when two pairings disagree, believe the one
with the tighter distribution rather than averaging them.

The consequence to carry: tpdf's own startup budget is about **50 ms**, of which ~45 is
already spent. There is no headroom elsewhere to absorb a regression, and anything below
the floor requires presenting the first page without the webview at all.

### A webview's first custom-protocol request costs ~45 ms, whichever request it is

The reason the payload experiment above came out flat, and the more useful half of it. The
framework build spends 48 ms between its first script and its mount --- and its first IPC
then costs **0.0 ms**. The no-framework build has nothing to fetch, is fully loaded 39 ms
earlier, and its first IPC costs **43.9 ms**.

Same toll, different door. It is charged once, to whichever request over a Tauri custom
scheme happens to be first --- an asset fetch or an `invoke`, it does not matter. So a
smaller bundle does not avoid it, it only moves which line of the timeline shows it, and
"module load and framework mount: 47 ms" is a misreading of an interval that is mostly not
either of those things.

Corollary for reading any startup table: an interval named after what the application is
doing may be dominated by a fixed cost the platform charges inside it. Attribute by
substituting the work, not by naming the interval.

### Tauri creates config windows *before* the setup hook, hiding the webview's cost

`tauri::Builder::build()` does not run the setup hook; `App::run` does, and the internal
`setup` creates every window listed in `tauri.conf.json` *before* calling ours. A mark at
the top of the setup hook is therefore already after WKWebView creation, and no mark can be
placed between the two --- which is why 142 ms sat unattributed for a full spike.

To time it: take `tauri::generate_context!()` as `mut`, `context.config_mut().app.windows.clear()`,
and build the window inside the hook with `WebviewWindowBuilder`. Note `build()` returns
once the webview exists and has been told what to load, not once it has loaded it.

### A page whose window is not visible is suspended --- so a JS watchdog cannot fire either

WebKit suspends a page when its window is not visible, and behind a **lock screen** or on a
display that has gone dark, every window qualifies. The suspension stops
`requestAnimationFrame` **and `setTimeout`**. So a startup run that ends on a presentation
callback does not time out, does not error, and produces no output at all --- it stops dead
after the last milestone before presentation and looks exactly like a slow machine. A
JavaScript watchdog is useless here: it is suspended alongside the thing it was watching.

An hour went into this, on a machine that had simply idled and locked mid-benchmark. Three
guards now, and the third is the one that diagnosed it:

- `startup_bench.py` checks `CGSSessionScreenIsLocked` (via `ioreg -n Root -d1 -a`) and
  refuses to start.
- It holds `caffeinate -du` for its own lifetime. **`-d` alone is not enough** --- it
  prevents a display going idle and will not turn one back on that is already off.
- The app carries a **Rust-side** watchdog that prints the marks it did reach and exits 2.

The general rule is the same one the crash-test entry states from the other direction: a
harness must be able to fail. If the only mechanism that could report a failure lives on
the same side as the failure, it will not report it.

### PDFium parses a document lazily --- but enumerating pages is not lazy

Opening the 775-page text corpus with `FPDF_LoadDocument` takes **0.6 ms**. Collecting every
page's size afterwards takes **86 ms**. On a *one-page* document it still takes 52 ms, so
this is not per-page geometry arithmetic --- it is a fixed cost plus a real page load each
time.

The trap is that "open the document and read the page table" looks like one cheap operation
and is two very different ones. Anything that wants full document geometry up front ---
a virtual scroller sizing its scrollbar is the obvious one --- is putting 50-90 ms on the
critical path to buy exactness it could have estimated. Load geometry lazily and correct.

### PDFium rendering *is* interruptible --- via the progressive API

`FPDF_RenderPageBitmap()` cannot be cancelled once entered. But
`FPDF_RenderPageBitmap_Start()` takes an `IFSDK_PAUSE` callback whose `NeedToPauseNow()`
is polled during rendering; returning non-zero suspends the render, and
`FPDF_RenderPage_Continue()` resumes it. This is the mechanism for both cancellation and
for not holding the mutex through a long render. Use the progressive API for anything
that is not a small, bounded tile.

### PDFium cannot create digital signatures

`fpdf_signature.h` is an **inspection** API --- it reads existing signatures. Applying a
cryptographic signature requires a separate crypto stack, trust store, certificate
selection, timestamping and revocation handling. Placing a signature *image* and
*cryptographically signing* are unrelated problems; do not let the roadmap conflate them.

### Redaction conflicts with incremental save --- and a full rewrite is not sufficient either

Incremental save appends an update section and leaves the original bytes intact, which is
exactly what redaction must not do. So applying a redaction is a **full-rewrite barrier**.

But a non-incremental save is still not proof of sanitation. A serializer can happily
rewrite a file while carrying over unreachable objects, unused resources, embedded
originals and copied streams; and overwriting a file in place can leave trailing bytes
past the new `%%EOF`. Redaction must write a **fresh file from a garbage-collected
reachable object graph**, then atomically replace the target. See `docs/PLAN.md` §6.

Scope note to state honestly in the UI: this sanitizes the PDF, not previous copies,
backups, or recoverable filesystem sectors.

### Digital signatures constrain what may be edited at all

Incremental save preserves a prior revision's cryptographic integrity, but that is not the
same as the signature remaining *valid and trusted* --- the document is still reported as
modified after signing. Worse, a certification signature with a DocMDP permission entry
can **forbid** page, annotation or form changes outright. Every edit command must be
classified as incrementally representable, full-rewrite-required, or forbidden on a signed
document. Do not claim "signatures survive".

Spike 0.6 measured how far that goes, and it is further than "some edits are forbidden".
Against pyhanko, on an approval signature and on DocMDP levels 1, 2 and 3, an appended
update leaves the signature `intact=yes valid=yes` every time --- and the difference
analysis rejects the change every time, at every level, including an **annotation-only**
edit to a level-3 certified document, which the specification explicitly permits. Cutting
the edit down to its irreducible minimum does not clear it: extending an `/Annots` array
that is its own object, so the page dictionary is never touched, narrows the complaint to
two objects and still fails. pyhanko says why in its own log --- *"StandardDiffPolicy was
not designed to support DocMDP level 3 (MDPPerm.ANNOTATE)"*.

So DocMDP is not the discriminator it looks like. **"The spec permits this edit" and "a
validator will accept it" are different claims**, and only the second one is what a user
sees. Treat any signed document as edit-hostile regardless of its permission level, offer
to save a copy, and never let the UI imply the signature will be fine.

### Whether `/Annots` is an indirect array decides how large an annotation edit is

A page can carry `/Annots [5 0 R]` written inline in its dictionary, or `/Annots 12 0 R`
pointing at an array object. Both are common. They are not interchangeable for incremental
editing: extending the array *object* touches one object and leaves the page dictionary
untouched, while extending an inline array means rewriting the page dictionary itself.

On a signed document that difference is the whole game --- the page dictionary is a signed
structural object, so rewriting it is a change no difference analysis can justify, and
`.Root.Pages.Kids[0]` shows up by name in the rejection. Prefer the array object; when the
producer inlined it, know the edit is bigger than it looks.

The same shape applies to `/Contents`, and there it has a second trap: `/Resources` is an
*inheritable* attribute. A page that does not carry one takes its parent's, so "create an
empty `/Resources` on the page" does not add a resource, it removes every inherited one.
The page then renders blank while every check that counts pages still passes.

### Embedded fonts are subsetted

An embedded font contains only the glyphs already used. Typing a character that is not in
the subset has no glyph to draw. This is the root cause of every mangled Acrobat text
edit, and it constrains the entire text-editing design. What that looks like in practice,
and how PDFium handles it, is measured above under `set_text()`.

### `lopdf`'s object collection is quadratic, but the algorithm is not

`prune_objects()` and `renumber_objects()` both walk the graph through
`traverse_objects()`, which accumulates seen ids in a `Vec` and calls `contains` before
every push. Measured 2026-07-26, cost of collection over a plain save: 3.7 ms at 2,445
objects, 83 ms at 7,758, **1,414 ms at 25,583** — a 3.3x larger graph costs 17x more.
25,583 objects is a medium document.

A mark-and-sweep over a `HashSet` (`route_lopdf_mark` in `sanitize_rewrite.rs`, about
thirty lines) produces byte-identical output at a cost indistinguishable from not
collecting at all. Renumbering is dropped with it — contiguous object numbers are cosmetic
and cost a second quadratic pass — but then **`max_id` must be lowered by hand**, because
`/Size` is written from it and sweeping does not touch it. Skip that and the file claims
more objects than it has; `qpdf --check` rejects it and PDFium does not notice.

So: use `lopdf` for the rewrite, write the sweep yourself. Do not reach for QPDF to solve
a `Vec::contains`.

### `lopdf` silently drops encryption on save

Given an AES-256 file with an empty user password — one that opens in any reader with no
prompt — `lopdf` decrypts on load and writes **plaintext** on save. No error, no warning,
no flag. QPDF re-encrypts with the original parameters.

Quietly removing a document's protection is its own security failure, and it is invisible
in every check that looks at content rather than structure. Any save path must preserve
encryption or refuse; `qpdf --is-encrypted` (exit 0 when encrypted, 2 when not) is the
cheap assertion.

### An incremental save is cheap on disk, not in memory --- and its cost is the parse

Measured 2026-07-26 across a 0.9 KB to 337 MB sweep (`incremental-save --mode speed`). In
memory, a full `lopdf` rewrite of a 337 MB scan costs **12.4 ms** against an incremental
append's **12.3 ms** --- no advantage at all, because the rewrite is essentially a large
`memcpy` and this machine has the bandwidth. Reading only that table would kill incremental
save as pointless.

Landing the save on disk reverses it: **29 ms against 239 ms, 8.2×**, because the append
writes 723 bytes to a file that already exists and the rewrite writes 336,623,496 and
renames. Below a few megabytes the ratio is 1.0× --- both are dominated by a single
`F_FULLFSYNC`, about 3 ms. The rewrite also needs room for two copies of the document,
which no timing shows.

Two things worth carrying:

- **Benchmark a save by writing it, not by serialising it.** The interesting cost of a
  save is I/O, and an in-memory harness measures precisely the part that does not matter.
- **`sync_all` is `F_FULLFSYNC` on macOS, a device-wide barrier.** Staging a 337 MB fixture
  with `fs::write` and then timing an append's flush charges the append for the staging: it
  measured *slower* than the full rewrite until the setup was made durable outside the
  timer. Any fsync benchmark on macOS must account for what else is dirty.

What remains of the append's cost is almost entirely **parsing** --- 5.7 ms of the 29 ms at
337 MB --- which every edit must pay to know what it is editing. Note also that
`IncrementalDocument::create_from` takes the previous bytes by value and `save_to` writes
them through before appending, so the obvious use of the API copies the whole document
twice for no reason. Sink the prefix, or append to the file in place.

### An object a prior revision overwrote is reachable by no parser

Incremental update replaces object 5; the old object 5 stays at its old offset and the new
cross-reference table points past it. Every parser resolves through the newest table, so
the old bytes are handed to nothing — not to a graph walk, not to `qpdf --check`, not to
PDFium. A byte scan finds them only if they were uncompressed, which for real content they
will not be.

The consequence for any verifier: **a file with more than one revision cannot be certified,
only rewritten and then certified.** `%%EOF` count is the cheap test. Note this is a
different failure from an object the update merely *dropped from the page* — that one is
still in the cross-reference table and a graph walk does see it, as an orphan.

### A decompression bomb costs QPDF CPU, not memory — and `lopdf` neither

`qpdf in out` re-encodes stream data by default, so it fully decodes a stream that inflates
to 1 GiB: **1.92 s for a 2,879-byte input**, at only 8.4 MB resident, because it streams
rather than buffers. `lopdf` with `LoadOptions::max_decompressed_size` refuses in 0.3 ms.
`qpdf --stream-data=preserve` finishes in under 10 ms — by copying bytes it never looked
at, which is precisely what a sanitizing rewrite must not do.

Two things follow. The bound belongs on the *rewriter*, not only on the verifier. And a
resource limit expressed in memory would have caught none of this: the attack here is
600x amplification in time.

### PDFium accepting a file is not evidence the file is well formed

PDFium is deliberately lenient — that is why it is the right renderer for real-world PDFs —
so it will happily render a file whose `/Size` is wrong, and render it pixel-identically to
a correct one. `qpdf --check` named the same defect immediately.

Corollary for verification: a render comparison proves the *content* survived and nothing
about the *structure*. `docs/PLAN.md` §6 requires re-parsing with an independent parser for
this reason, and the requirement paid for itself the first time it ran, on a bug in tpdf's
own sweep.

### The test fixtures are generated, not committed

`testdata/*.pdf` is gitignored. Regenerate with:

```
uv run --with fonttools testdata/make_text_pdf.py testdata   # text-*.pdf, spike 0.3
python3 testdata/make_hostile_pdf.py testdata                 # hostile-*.pdf, spike 0.4
python3 testdata/make_vector_pdf.py testdata/vector-heavy.pdf # spike 0.1
uv run --with pyhanko --with cryptography \
    testdata/make_incremental_pdf.py testdata                 # incr-*.pdf, spike 0.6
```

`make_incremental_pdf.py` writes about **550 MB** — the scan fixtures exist so that
"appending to a 300 MB file is near-instant" can be tested at 300 MB, and they are
uncompressed on purpose so nothing downstream can make them cheap. An existing scan of the
right name is reused rather than rewritten. The signed fixtures need `pyhanko`, which is a
*test oracle* and not a dependency of tpdf: it both writes the signatures and is the only
implementation here that can validate them (`testdata/check_signature.py`). Without it
those five fixtures are skipped and the rest still build.

`make_hostile_pdf.py` also writes `hostile-manifest.json`, which records where each
fixture's needle is and whether a collected rewrite is expected to remove it; the Rust
harness reads that rather than hardcoding expectations. It shells out to `qpdf` for the
encrypted fixture, and needs `--preserve-unreferenced` there — without it qpdf collects the
orphan on the way in and the fixture arrives already sanitized.

A fixture whose needle cannot be found in the fixture *itself* proves nothing about any
rewrite, so `sanitize-rewrite` checks that first and says so. Two fixtures were vacuous on
their first run and had to be redesigned.

`make_text_pdf.py` embeds a **system** font, which is fine only because the output stays on
the machine. Nothing it generates may be committed or redistributed.

Its default font is deliberately a **serif** face, and changing that breaks the fixture in
a way that is invisible. PDFium's font mapper aliases Helvetica to Arial, so embedding
Arial made the substituted `base14` render **bit-identical** to the embedded ones --- three
fixtures that cannot distinguish "used the embedded subset" from "silently substituted",
which is the one thing they exist to test. Caught only because three different fixtures
hashed the same. If a fixture's baselines ever match across fonts, suspect that first.

---

## Repository facts

- GitHub: `tstone-1/tpdf`, **private**.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` --- anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
