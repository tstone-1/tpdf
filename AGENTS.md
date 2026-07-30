# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

The one thing this file does *not* carry in full is the trap list --- 129 entries
in [`docs/TRAPS.md`](docs/TRAPS.md), indexed by title below. That file is **not**
auto-loaded, on purpose, and the index exists so that the decision to read an entry is an
informed one rather than a guess.

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
reason, and so must tpdf. **In place for the viewer's own render path since 2026-07-28** —
`RenderService` defaults to worker processes on macOS, and `bin/backend_probe.rs` proves the
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
`bin/win_sandbox_probe.rs`). Six rungs, each rendering the same tile from the same document
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

**A Windows worker now exists and works** (2026-07-29). `Worker::spawn` builds one on Windows:
the child is created suspended, dropped to low integrity, assigned to the job object before it
executes an instruction, and given two pipes and the document and tile sections as inherited
handles named in argv. `worker-probe` is the evidence --- 11/11 checks on `text-base14`,
`text-cid`, `vector-heavy` and `rotated`, including **pixel-identical** tiles against the
in-process render, text extraction, outlines and search across the boundary. The font
substitution that the macOS sandbox caused, and that `win_sandbox_probe` predicted would not
recur here, did not.

`Worker` carries the two platforms as per-platform type aliases rather than an enum, so every
macOS line in `worker.rs` is byte-identical to what it was --- deliberate, because none of this
can be re-verified on macOS from a Windows machine and a diff that touches only Windows code is
the strongest statement available about what cannot have regressed.

**Windows no longer fails open.** `Backend::default_here()` selects workers there, proved by
the external module check above rather than by the absence of our own warning.

**`backend-probe` runs on Windows too, and passes** (2026-07-30): **37/41** on `text-base14` and
`text-cid`, **38/41** on `outline-hostile`, **40/41** on `vector-heavy`, which is where a render
is slow enough for the withdrawal checks to run rather than skip. No failures on any. The
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
profiles included, and shares no mechanism with the Windows model.

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

One smaller gap remains: the pool's memory poll (`Worker::footprint`) returns `None` there, which
is not a gap in the same sense --- the job object caps commit in the kernel, which is the bound
macOS cannot have and polls for instead.

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
| macOS print dialog | PDFKit + AppKit via [`objc2`](https://docs.rs/objc2) (Zlib OR Apache-2.0 OR MIT) | **Settled on macOS** --- paginates and runs the panel; also the independent parser every print job is read back with. Windows not written |

The PDFium pin is `chromium/7881`, installed by `scripts/fetch_pdfium.py` and verified by
digest. Every measurement in this file was taken against that build, so bumping it
invalidates them until the two checks in `BUILD.md` are re-run.

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI.

`tauri-plugin-dialog` (Apache-2.0 OR MIT) is the only plugin linked, for the file-open
dialog; it pulls `tauri-plugin-fs` (Apache-2.0 OR MIT) and `rfd` (MIT). Checked against the
licensing constraint above rather than assumed --- every dependency added has to be, because
one copyleft crate anywhere in the tree removes the option of making this repository public.

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

Currently: a PDFium pin check, `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings`, `cargo test --locked`, `cargo build --locked --bins`, `npm run check`,
`npm run test`, `npm run build`. `--all-targets` covers test code, `-D warnings` makes lints
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

**There is no remote CI, deliberately** --- pre-release, one machine, and a workflow would
add a second place for the gate list to live while catching nothing `scripts/gates.py`
does not catch first. When it is added (the natural trigger is the repo going public, or a
second contributor) the workflow should *invoke* `scripts/gates.py` rather than re-list the
commands in YAML.

**Windows runs the viewer, and is still not supported.** On 2026-07-29 a Windows build opened
documents and passed `viewer_check.py` on four corpora --- **86 check names** on each, the same
invariant macOS holds to, with ran/skipped splits inside the ranges `BUILD.md` records. So the
viewer works there; what is missing is not function.

What is missing is **containment**. The platform is unsandboxed: `Backend::default_here()`
selects in-process off macOS, so hostile input is parsed in the app process, and it **fails
open** --- the refusal guards `TPDF_BACKEND=worker`, a path the default never takes, so there
is no error to notice. A port owes a real answer (job objects, a restricted token, a separate
desktop) before Windows can ship. `BUILD.md` has the detail.

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
titles. Only the titles are here, because there are 129 of them and the full text
was 93% of this file --- an instruction budget spent on the 126 traps that are not
the one in front of you. Keep both numbers in this section current when adding an entry;
they were already two behind when this one was written, which is how a count in prose
fails. What the index has to preserve is knowing that a trap *exists*;
the paragraphs matter once you are in that area.

So: **before working in any area named below, read its entry.** A title is a claim, not
the lesson --- several of them are the opposite of what they sound like, which is why they
were written down. Grep the title in `docs/TRAPS.md`.

New traps go in `docs/TRAPS.md` with a line added here, in the same commit.

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
- `FPDFText_GetText` drops characters, so it cannot be indexed alongside boxes
- A page carries `/Rotate`, and PDFium answers in two coordinate systems at once
- A line-grouping rule assumes an axis, and the axis is not always vertical
- Two rotation tables, disagreeing at every turn but zero
- PDFium's character order is not the page's line order
- A dense page of uniform lines cannot detect a y-flip
- `FPDFBookmark_GetDest` follows the bookmark's action without checking its type
- An outline can be infinite, and PDFium says so in its own documentation
- PDFium cannot create digital signatures

### The worker boundary, the sandbox and the pool
- `thread_safe` does not serialize PDFium --- there is no mutex, and threads crash
- A worker process is nearly free; the webview boundary is not
- macOS has no memory rlimit, and `RLIMIT_CPU` is a lifetime budget
- Polling a child's footprint bounds a leak, not a burst
- `proc_pid_rusage` takes the struct's address, not a pointer to it
- The vendored PDFium has no JavaScript engine and no XFA --- verify it, do not assume it
- The no-V8 property is one word in a URL, so the fetch asserts it
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
- A mean cannot test a claim about a minimum
- A frame-rate pass means nothing without a coverage number beside it
- Interleaving controls for drift, not for what the last variant left behind
- Three similarity metrics in a row, each unable to see its own failure
- A timer that starts after the setup measures the wrong thing, and reports it
- `cargo test` is a debug build, and a debug number in a doc comment is a lie

### Writing a check that can fail
- Break the code on purpose, or the test suite is decoration
- A property that holds by construction cannot test the thing it resembles
- A fixture the library itself wrote cannot tell a passthrough from a rewrite
- An oracle more forgiving than the thing it stands in for cannot fail
- A writer and its own reader agree about a document that is wrong
- A canvas round trip cannot read back what a renderer produced
- A dependency that refuses your test input makes your own guard look redundant
- A defect that switches off a check's precondition is not caught by that check
- An "already have it" cache needs an in-flight set, not just the cache
- A text comparison cannot see a property that is not about text
- A test whose precondition is already satisfied never runs
- A crash test that compiles away proves containment of a crash that never happened
- A test for an atomic write must plant the intermediate it is meant to prove
- A control can be contaminated by the phase that ran before it
- A check that derives its inputs from the thing it is testing cannot fail
- An outcome two mechanisms can produce cannot test either one
- A length bound cannot be tested by the verdict it produces
- A check nested inside a lookup for the thing under test disappears with it
- A check whose failure mode is a wait cannot fail
- A test whose failure is a hang reports a pass and a timeout in one breath
- An unreachable guard is worth keeping if the type can carry it instead
- A post-destroy guard that returns early leaks what it declined to take

### Harnesses: running checks and reading what they print
- A mutation harness needs the same control as the thing it is testing
- A timeout that discards the transcript recreates the failure it was added to diagnose
- Restoring a mutated file by *moving* a backup over it tests the mutated binary
- A harness that prints only at the end cannot say where it stopped
- A mutation harness that dies leaves the mutation in the tree
- Three mechanisms, no checks: measure what a commit's tests can actually see
- A verdict that reads a timeout as "no result" throws away the finding
- A harness that prints stderr only on failure hides what a passing run said
- A mutation caught by an access violation produces no test results at all
- A comment claimed an ordering mattered, and the mutation that should have hurt did not
- `caffeinate <utility>` becomes a child of the utility, so a child count counts it
- Repeating a race inside one process re-runs the first round, not the race

### Windows and portability
- A crate-root `#![cfg]` empties a `[[bin]]`, and cargo reports a missing `main`
- An uninhabited type carries its impossibility into every caller
- A directory that exists is not the library you need
- A list of documented blockers can be wrong in the direction that looks thorough
- A gate list that never links a binary cannot see a link error
- A custom URI scheme is not spelled the same way on every platform
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

### Fixtures
- The test fixtures are generated, not committed

## Repository facts

- GitHub: `tstone-1/tpdf`, **private**.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` --- anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
