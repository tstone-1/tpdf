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
warnings`, `cargo test --locked`, `npm run check`, `npm run test`, `npm run build`.
`--all-targets` covers test code, `-D warnings` makes lints fatal, and `--locked` catches a
`Cargo.lock` that was not committed after a `cargo update`; dropping any of them silently
weakens the gate.

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

**Windows is unverified.** Every gate run and every measurement here is macOS arm64.

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

### `thread_safe` does not serialize PDFium --- there is no mutex, and threads crash

Upstream PDFium makes **no thread-safety guarantee at all**, and its authors recommend
parallel *processing*, not multi-threading. That much has always been right here. The
mechanism this file gave for it was not.

**`pdfium-render`'s `thread_safe` feature does not lock anything.** Measured on 0.9.3,
2026-07-27, by `src/bin/thread_probe.rs`. What the feature actually does is store the
bindings in a global `OnceCell` that is *awaited* rather than unwrapped, plus
`unsafe impl Send for Pdfium` and `unsafe impl Sync for Pdfium`. The only `Mutex` in the
crate guards a page-index cache; the only `RwLock` is in the WASM bindings. A native call
dispatches straight through a function pointer:

```rust
unsafe fn FPDF_LoadPage(&self, document: FPDF_DOCUMENT, page_index: c_int) -> FPDF_PAGE {
    (self.extern_FPDF_LoadPage)(document, page_index)   // no lock, anywhere
}
```

The crate's README still says the feature "wraps access to Pdfium behind a mutex". The
implementation and its documentation disagree, and this file believed the documentation.

What actually happens when two threads render at once, each owning its own
`FPDF_DOCUMENT` --- the exact scenario the old text said was serialized:

| fixture | threads | outcome |
|---|---|---|
| `vector-heavy` (A0) | 2 | **SIGSEGV** |
| `vector-heavy` (A0) | 4 | **SIGSEGV** |
| `text-heavy` | 4 | survives, 3.85x speedup, pixel-correct --- 6 runs out of 6 |
| `text-heavy` | 8 | **SIGABRT** |
| `text-heavy` | 4, five rounds | survives round 0 at 3.85x, then **crashes on round 1** |

So both halves of the old claim were wrong, in opposite directions. They do **not** render
sequentially --- 3.85x on four threads is near-linear, which no global mutex permits. And
extra handles buy **no crash-safety whatsoever**; they buy a segfault.

The architectural conclusion is unchanged and now rests on something true: in-process
parallel tile rendering is not achievable, and parallelism requires **separate worker
processes**. But the reason is that threads are *undefined behaviour*, not that they are
pointless. Those are different arguments, and only one of them is a safety argument.

Two corrections that follow, both previously stated as consequences of the mutex:

- **Nothing "holds the mutex and starves" anything.** tpdf's renders are serialized today
  because `src/render.rs` deliberately uses one render thread, which is our design choice
  and reversible, not a property of the library.
- **The progressive API's value is cancellation, not lock release.** There is no lock to
  release between `FPDF_RenderPage_Continue` calls.

The trap worth carrying is the middle row of that table. **Concurrent PDFium often works.**
On a simple document at four threads it returned pixel-perfect tiles six times out of six,
and a developer who tested exactly that would have concluded threads were fine. The same
configuration crashed on the second round of a longer run. A race that usually wins is
indistinguishable from correct code until it is in front of a user with a CAD drawing ---
the same shape as the sandbox that rendered `ok` with a substituted font, and the crash
test that compiled away.

This is the **second** time this entry has been wrong. An earlier version claimed PDFium
was unsafe only "per document handle" and that multiple handles would render in parallel;
that was caught in the 2026-07-26 audit. Both errors came from trusting a dependency's
prose over its behaviour. The rule that would have caught either: **a claim about a
library's concurrency is a measurement, not a citation.**

What worker processes buy depends on the *shape* of the work, and there is no single
scaling factor to quote. One tile from each of many pages of the text corpus: **3.89x on
four** workers, then about 0.4x per further worker (spike 0.5). Six tiles of one A0 page,
which is what a viewport actually asks for: **2.56x on four, 3.22x on six, and nothing at
eight** (2026-07-27, `worker-bench --mode parallel --grid 3`). Same machine, same tile size.

So "size the pool from performance cores, not `hw.ncpu`" was drawn from the first workload
and does not survive the second --- the plateau there is at six on a 4P+6E machine. Measure
the workload you have; a speedup measured across documents does not predict one measured
across tiles, and the difference is large enough to change an architectural decision.

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

### The no-V8 property is one word in a URL, so the fetch asserts it

`bblanchon/pdfium-binaries` publishes **both** variants of every release:
`pdfium-mac-arm64.tgz` (3.5 MB) and `pdfium-v8-mac-arm64.tgz` (13.4 MB), and the same pair
for every platform. The entry above --- and `docs/THREAT-MODEL.md`'s promotion of "document
JavaScript is disabled" to "there is no engine to disable" --- is a property of the *asset
that was fetched*, not of PDFium. Downloading the other one silently reinstates a
JavaScript engine, and nothing downstream would notice: the symbol scan is the only check
that discriminates, and it is not run on every build.

So `scripts/fetch_pdfium.py` refuses a V8 asset by name and verifies a pinned SHA256
before extracting anything. A digest alone would not have caught it --- a V8 build pinned
to its own digest passes a digest check perfectly.

The pin is `chromium/7881`. The fetched mac-arm64 archive is byte-identical to the install
every Phase 0 measurement was taken on (dylib sha256 `1bc45b15…`), which is what makes the
script a reproduction of the tested binary rather than merely a download of a similar one.

### PDFium ships its loadable library in a different directory on Windows

macOS gets `lib/libpdfium.dylib`. Windows gets the runtime DLL at **`bin/pdfium.dll`**,
and `lib/` holds only the import library `pdfium.dll.lib`. Found 2026-07-27 by a
cross-platform install test that was only meant to check a digest.

`pdfium_library_dir()` in `src-tauri/src/lib.rs` joins `vendor/pdfium/lib` unconditionally,
so on Windows it resolves to a directory containing nothing loadable. **This is an open
defect**, not a fixed one --- it is recorded here rather than repaired because no Windows
build has ever run, and a blind fix would be another untested claim. `scripts/fetch_pdfium.py`
already knows both layouts.

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

### Break the code on purpose, or the test suite is decoration

The first tests in this repo (the request queue and the `tile://` parser) were written, run,
and passed. That proves nothing on its own --- a test that cannot fail passes exactly like
one that can. Each was then checked by deliberately mutating the code it covers and
confirming the *expected* test went red. Two of the twenty-six were wrong, and neither would
have been found any other way:

- **A guard no mutation could fail.** `Queue::withdraw` opened with `if rid == 0 { return }`.
  Deleting it broke nothing, because `enqueue` and `claim` already keep zero out of both
  tables, so the early return was unreachable defence. It was deleted rather than kept:
  a check nothing pins is a check that can silently become wrong. The two guards that *are*
  load-bearing were each confirmed to fail a test on their own.
- **A test that probed the wrong direction.** "A query key is matched whole" asserted that
  `fmt` is not found in `xfmt`. Mutating `==` to `starts_with` **passed** --- the hazard for
  a prefix match is a key that *extends* the sought one (`fmtx`), not one that precedes it.
  The test asserted the mistake it happened to think of, not the mistake the code could
  make.

The procedure is cheap --- copy the file, apply one edit, run, restore --- and it is the
only thing that distinguishes a suite from a comfort blanket. **Write down which mutation
each test is supposed to catch, then make that mutation.** If nothing goes red, either the
test or the code is wrong, and both are worth knowing about.

Two smaller notes from doing it. Mutate one thing at a time: a double mutation showed a
test failing that a single one did not, which is how the redundant guard was found rather
than misread as covered. And expect a mutation to trip *more* tests than the one aimed at
--- that is a coverage overlap, not a problem, but the aimed-at test must be among them.

**A mutation that changes nothing looks exactly like a test that cannot fail.** Doing this
again on the viewer (2026-07-27, six mutations), one of them replaced
`this.scroller.setZoom(next)` with `setZoom(this.zoom)` --- and the line above had already
assigned `this.zoom = next`, so it was an identity. Nothing went red, which reads precisely
like a missing assertion, and the first instinct was to go and strengthen a check that was
already fine. Before concluding a test is decoration, confirm the mutation was one: print
the diff, or pick a mutation whose effect is impossible to miss.

**Aim at the control, not only at the assertion.** A second mutation there deleted the zoom
invalidation entirely. The check it was aimed at --- "recovers coverage after a zoom" ---
stayed green, correctly: coverage never dropped, so there was nothing to recover. What went
red was the *control* beside it, "a zoom step discards what it invalidates". Both outcomes
are the suite working; predicting the wrong one is a fact about the prediction. Write down
which check should notice, and treat a different check noticing as a result worth reading
rather than a miss to be tidied away.

### A property that holds by construction cannot test the thing it resembles

The viewer check asserted that text dragged out of the page appears in the page's own text:
`whole.includes(dragged)`. That reads like a check on the *geometry* --- if the character
boxes and the character indices disagreed, surely the drag would return the wrong words.

It cannot fail. A selection is a contiguous range of character **indices**, and the string it
produces is built from those indices, so it is a substring of the page's text whatever the
boxes claim. Inverting the y-flip in `text.rs` --- the exact defect the check was written to
catch --- passed all twenty checks, and the drag returned real words. They were simply the
wrong words, from the wrong part of the page.

What discriminates is an assertion that ties a **screen position** to **specific characters**:
text dragged near the top of the page must come from earlier in the page's text than text
dragged further down. Under the flip that inverts, and the check goes red.

The general form, and it is subtler than the usual "the test never ran": **before trusting an
assertion, ask what would have to be true for it to fail.** If the answer is "nothing the
code under test controls", it is decoration however relevant it sounds. Related to the query-key
test above that probed the wrong direction, but worse --- that one could fail on some input,
this one on none.

### A defect that switches off a check's precondition is not caught by that check

The sharpest form yet of "a test whose precondition is already satisfied never runs", and
worse, because here it is the **defect itself** that arranges the precondition.

The page strip builds only the rows its panel can show. The check for it read: if the strip
built every row, skip --- *all the rows fit in the panel* --- otherwise assert it built some.
Perfectly sensible, and then the mutation that deletes windowing entirely, so that `layout`
builds every row in the document, makes the check report `[SKIP]`. It does not go red.
Nothing goes red.

The fault is that the skip condition was derived from the same quantity being asserted. The
repair is to derive it from something the defect does not control --- here the panel height
and the row height, which give the most rows that *could* be on screen --- and then assert
against that bound rather than against the strip's own output.

**Ask what a check does when the thing it checks is broken, not only when it is absent.** A
skip is a third outcome, and a defect that reaches it is as invisible as one that passes.

Worth recording how it surfaced, because it is the cheap half of the procedure: it was found
while **writing down which check each mutation should turn red**, before running any of them.
The check had already been written, reviewed and run green on five corpora. Stating the
prediction is what exposed that there was no prediction to make.

Two earlier versions of the same check were wrong in the other direction, which is worth the
sentence because the pair brackets the mistake. The first estimated the row height at 60 px
and skipped on the twelve-page document whose rows are 186 px --- so the only fixture that
could exercise windowing declared itself inapplicable. The second asked the strip how many
rows it had built, which is exactly the quantity the defect controls.

### An "already have it" cache needs an in-flight set, not just the cache

The strip borrows a page's bitmap from the viewer's tier-1 cache when it has one, and copies
it with `createImageBitmap` --- which completes in a microtask, not immediately. The guard
was "is this page in `bitmaps`", and between starting the copy and finishing it the page is
in neither the cache nor the outstanding-request slot. Every scroll, resize and position
change in that window starts the *same* borrow again.

It read as twelve borrows on a twelve-page document with seven rows on screen. Every one of
them succeeded and the pictures were correct, so the only symptom was a number in a detail
column that looked slightly too round. What pins it is `borrowCount <= renderedCount` --- the
*upper* bound. The check said `borrowCount > 0` first, which is the obvious way round, and
under this defect that passes **harder**: more duplicates make it more true.

**A "do we already have it" test is not a "is one already on the way" test**, and any cache
filled asynchronously needs both. Same shape as the request queue's `queued`/`inflight`
split, arriving from a direction that did not look like a queue.

### A mutation harness needs the same control as the thing it is testing

The script driving those mutations rebuilt the app, ran the check, and looked for `[FAIL]`
lines. A run that never produced a report --- a crash, a timeout, a lock screen --- has no
`[FAIL]` lines either, so it printed *"nothing went red at all"*, which is exactly what a
mutation nothing noticed looks like. Two of five results were unreadable for that reason.

It now requires the summary line (`N/M checks passed`) to be present and reports a missing
one as a broken run, distinct from a surviving mutation. **A harness that reads absence as
evidence needs to distinguish absence from silence** --- the same failure as the leak scanner
that could not decode a Type0 font, one level up.

### A text comparison cannot see a property that is not about text

Two defects in the *checks* for the screen-reader layer, found within minutes of each other,
same shape both times.

**`textContent` concatenates block elements with nothing between them.** Comparing a page's
accessible text against an independent extraction failed at 2,562 characters against 2,618
--- exactly one missing separator per line on a 56-line page. The content was identical; the
comparison had flattened away the block structure that carried the line breaks, and then
reported the absence of that structure as a content mismatch. Join the blocks (or read
`innerText`, which respects layout) when the thing being compared is what a reader hears.

**`display:none` and `visibility:hidden` remove an element from the accessibility tree, and
every text assertion still passes.** A screen-reader layer hidden either way is completely
inert while reading perfectly correct through `textContent`. The visually-hidden idiom has
to be the clipping form (`width:1px;height:1px;clip-path:inset(50%)`) precisely because
those two are not equivalent to it, and that is worth its own assertion: the container must
be 1x1 *and* neither `display:none` nor `visibility:hidden`. Without it the whole layer is
one CSS edit away from doing nothing, with no check able to notice.

### Restoring a mutated file by *moving* a backup over it tests the mutated binary

The harness driving the `search.rs` mutations copied the file, mutated it, ran the tests,
and put the backup back with `shutil.move`. A move carries the backup's metadata --- so the
restored file's mtime was the moment the *copy* was taken, older than the artifact cargo had
just built from the mutated source. Cargo compared timestamps, concluded nothing had
changed, and ran the suite against the mutation. The confirmation run afterwards reported a
failing test that was not in the tree, on a file whose contents were provably correct.

The mutations themselves were unaffected --- each was written with `open(path, "w")`, which
stamps the current time, so every one of those runs did rebuild. It is only the restore that
went backwards in time, which is the half nobody thinks to check.

Two rules, and the second matters more: **restore by writing the bytes back, never by moving
a file over them**, and **verify the restore by re-running the suite and requiring green**.
A harness that can leave the tree in a state its own results do not describe is worse than
no harness, because the next real failure reads as another restore artifact.

The same harness then failed a second way, which is worth the sentence because the mechanism
is so ordinary. It found failure lines with a regex expecting **two** spaces between the
check's name and its detail — and the names are printed with `padEnd(40)`, so a name of
exactly 41 characters is followed by one space and matched nothing. That mutation was
reported as a **survivor** while the summary line in the same output said 28 of 29 checks
passed. Two numbers in one buffer disagreeing, and nothing comparing them. **When a harness
can derive the same fact two ways — a parsed detail and a printed total — make it check that
they agree**, because the parse is the half that breaks silently.

### A test whose precondition is already satisfied never runs

The sharpest instance of the shape above, and the fourth in this project. `viewercheck.ts`
pressed **End** on a 775-page document and then waited for full sharp coverage before
declaring "covers the last page". It passed instantly --- because the *first* screen was
already fully covered, so the predicate was true before the jump had rendered a single tile.
Its own detail line said so: `sharp=100.0% on page 1/775`.

The fix is a control that asserts the **drop** before the recovery: one frame after the jump,
coverage must be *below* threshold. With that in place the same check reads
`sharp=100.0% on page 775/775`.

The general rule: **an assertion that something recovered is worth nothing unless something
was first shown to have broken.** Any test of the form "do X, then wait for the good state"
needs to establish that the good state was not already there --- and the seam is invisible,
because a check that returns immediately and a check that waited both print `[OK]`.

Note the control is not always applicable, and that has to be visible too. On a one-page A0
sheet **End** moves 488 px and the tiles on screen stay valid, which is correct behaviour;
the check reports `[SKIP]` with the reason rather than silently vanishing, because a control
that disappears on some inputs is indistinguishable from one that ran.

### A crash test that compiles away proves containment of a crash that never happened

`worker-bench --mode crash` originally faulted with `null_mut::<u8>().write(1)`. That is UB
the optimizer is entitled to delete, and in release it did: the process exited normally
through the fallthrough arm, the parent reported clean containment, and the run looked like
a pass. The tell was the epitaph --- "exited with code 9" where a segfault should have said
"killed by signal 11".

Route the address through `std::hint::black_box` and use `write_volatile`. More generally:
a test whose failure mode is *not failing* needs its own assertion on how it failed, not
just on the outcome.

### `AppHandle::exit` does not set the process's exit code

`app.exit(1)` ends the event loop; `App::run` then returns normally, `run()` returns, `main`
returns unit, and **the process exits 0**. So every automated run in this repository
reported success for its entire existence, whatever it printed --- including
`scripts/viewer_check.py`, whose closing `return completed.returncode` could not fail.

Nothing caught it because nothing looked. The mutation harnesses parse `[FAIL]` lines out
of the transcript rather than reading `$?`, which is why the mutation results they produced
were nonetheless correct; and a human reading a transcript sees the failures directly. The
exit code was the one consumer with no second opinion, and it was wrong for months.

What surfaced it was a run printing `[FAIL]` and `0/1 checks passed` immediately above its
own harness verdict of `[OK] session restore verified` --- two numbers in one buffer
disagreeing, with nothing comparing them. That is the same defect as the `search.rs`
harness whose regex silently matched nothing, one level up, and the same repair applies:
**when a harness can derive the same fact two ways, make it check that they agree**, and
treat a disagreement as a broken run rather than as either answer.

`spike_exit` now flushes and calls `std::process::exit(code)` directly. Verify a fix like
this in **both** directions --- a failing run must exit non-zero *and* a passing one must
exit zero. Only one of those was ever in doubt, and checking just it would have been happy
with `exit(1)` unconditionally.

### `RunEvent::Opened` fires before the setup hook, so managed state is not there yet

A macOS double-click does not put anything in `argv`. Launch Services sends an Apple Event,
Tauri surfaces it as `RunEvent::Opened`, and **it arrives before the setup hook runs**. So a
handler that reaches for state registered in `setup` --- the obvious place --- calls
`state::<T>()` on unmanaged state, which **panics**, on precisely the path it was written to
serve.

The symptom is not a crash dialog at the time. The window appears, nothing is in it, no
error reaches stdout or stderr, and the last startup mark is `app built`. macOS records it
and offers to reopen windows on the *next* launch, which is how it surfaced --- a day later,
in a dialog. `~/Library/Logs/DiagnosticReports/<app>-*.ips` has the truth: `EXC_CRASH
SIGABRT`, `Abort trap: 6`, main thread.

Register anything the run callback touches with `Builder::manage`, before the event loop
exists, and read it with `try_state` --- a panic here is invisible, where a `None` costs one
document not opening.

Note what this does to the ordering advice already in this file. `Builder::build()` does not
run the setup hook and `App::run` does; `Opened` is dispatched by that same `run` *before*
setup. So "before the setup hook" is a strictly larger set of moments than it looks.

**Two causes were producing this one symptom, and the second nearly buried the first.**
Windows left over from testing were occluding new ones, so unrelated runs *also* produced no
output --- and `TPDF_RAISE=1` "fixed" those, which read as the whole problem being
environmental. A missing `tauri setup` mark was then read as "the setup hook never ran",
which was true and not the cause. Two mechanisms, one silence: fix the one you can prove and
re-measure, rather than accepting the first explanation that covers the evidence.

**The harness was the last thing to doubt, and should have been.** Four phases passed and one
produced nothing, so `open --stdout` looked like the culprit --- plausible, since `open`
detaches. What settled it was abandoning stdout and asking a different question: does the app
write its session file after a double-click? It did not. That turned "the capture is broken"
into "the feature is broken" in one command. **When a check reports a failure it alone can
see, find a second channel that does not share its machinery.**

### A test for an atomic write must plant the intermediate it is meant to prove

`Session::save` writes a scratch file and renames it over the target, so that a crash
mid-write leaves the previous session rather than a truncated one. The obvious test --- save,
then assert no scratch file is left behind --- **passes identically for a save that writes
the target directly**, because a direct write produces the right bytes and leaves no scratch
file of its own to find. Every other test in the module passes too: they all read the
result, and the result is the same.

What discriminates is planting a stale scratch file *first*, as a write that died would have
left, and asserting it is gone afterwards. Only a save that renames over it removes it.

The general form, and it is the one this repository keeps rediscovering: **when the property
is "how the result was produced", no assertion about the result can test it.** Find the
intermediate state the mechanism must pass through and assert on that. Same shape as the
crash test whose failure mode was not failing, and as the leak scanner that could not decode
its own carrier.

It was found by writing the mutation down before running it --- "the save writes the target
directly, with no rename" had no check to name, which is the cheap half of the procedure
doing its job for the second time in this project.

### A control can be contaminated by the phase that ran before it

The session check launches the app four times, two of them controls: "the app does not open
in the remembered state by itself", and "nothing opens when nothing is remembered". Both
were pointed at one scratch session file, on the reasoning that both wanted it empty.

It is not empty by the time the second one runs. The *first* control opens a document ---
that is what it is for --- and the app dutifully remembers it. So the second control
launched with a document to restore, restored it, and failed.

The standing rule about interleaving covers this ("before trusting an A/B, ask what each
variant leaves behind that the next one can find"), and it did not fire, because this did
not look like an A/B at all --- it is a control, and a control is exactly the thing assumed
to be inert. **A phase that writes state needs its own copy of that state, not a shared one
that happens to start empty.** The two controls now get a file each.

Worth noting the failure was legible only because the control existed: a check that had
merely asserted "the remembered state comes back" would have passed the whole run.

### A locked macOS session cannot be unlocked from a script, so it must be prevented

WebKit suspends a page whose window is not visible, and a locked screen occludes every
window --- so a frame-rate benchmark behind one does not run slowly, it does not run at all.
`scripts/scroll_bench.py` already refuses to start in that state rather than hanging.

There is no supported way to unlock the session programmatically; the only mechanisms are a
typed password, Touch ID or a paired watch, all of which need a person. The workaround that
exists in the wild --- storing the login password and having `osascript` type it at the lock
screen --- puts that password in a file readable by anything running as the user, and is not
worth the convenience.

So the only lever is prevention, and the trap is that per-run prevention is not enough.
`scroll_bench.py` holds `caffeinate -du` for **its own lifetime**: the gaps between runs are
unprotected, and a long headless bench running alongside it holds nothing at all. That is
how a session locked mid-batch here. Wrap the whole batch:

```sh
caffeinate -du bash -c '<run> ; <run> ; <run>'
```

`-u` as well as `-d`, because `-d` only stops a display going idle and will not turn one
back on that is already off.

### A raw `cargo build` binary runs no webview content at all

**`src-tauri/target/release/tpdf` opens a window and never executes a line of
JavaScript.** No error, no crash report, no console output --- a blank window and a page
that never loads. The same code inside the bundle
(`target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`) works perfectly. WKWebView
needs the bundle identity; a bare Mach-O has none.

This cost most of an afternoon on 2026-07-27, and the reason it was not obvious is that
every other harness in this repository runs a raw binary happily --- `outline-probe`,
`text-probe`, `worker-bench` and the rest are all plain executables with no webview in
them. Only the four `TPDF_*` spike entry points need a page to run, and `BUILD.md` gives
the bundle path in its example while the prose beside it says the check "does not require
a release bundle". Both statements are true and together they are misleading: it does not
require a *release* build, it requires a *bundle*.

So: **any run that needs the webview needs the `.app`.** `npm run tauri build -- --bundles
app`, then the executable inside it --- which keeps stdout and the environment, unlike
`open -a`.

The diagnosis that led there is worth as much as the fact. Chasing it produced two wrong
answers first, and the thing that settled it was building **HEAD itself** and watching it
fail identically. An earlier control that reverted only the frontend was not enough: the
Rust changes were still in the binary, so "it is not my frontend" had been established and
silently generalised to "it is not my code". **A control has to cover everything that
changed, not the part currently under suspicion.**

### A page that never ran looks exactly like one that ran slowly

The symptom shared by the entry above and the lock-screen one: **no output, 0% CPU, the
process alive.** It reads as a hang in whatever was most recently changed, and on
2026-07-27 that reading was confidently wrong three times in a row --- first blaming the
new code, then a broken frontend bundle, then window occlusion. The actual cause was the
raw binary, above.

Note especially that the occlusion theory was *stated as a conclusion* while it was still a
guess, and it survived longer than it should have because it is a real WebKit behaviour
that produces the identical symptom. **A plausible mechanism that explains the evidence is
not the same as the mechanism, and neither is one that has an entry in this file.**

Two repairs came out of it, both worth keeping whatever the cause turns out to be next
time:

- **The watchdog names the condition.** Every spike entry point begins by asking Rust for
  its path or config, so the first of those calls proves the page ran; `spike_env` records
  it as a `webview alive` mark. When the watchdog fires without that mark it says the page
  never ran a line of JavaScript, instead of printing a mark list that has to be
  interpreted. Both halves are now verified: it fires on a raw binary and stays quiet on a
  bundled one.
- **`viewercheck.ts` prints each result as it is recorded.** Buffering the report until the
  end meant a run that stopped midway printed nothing at all --- indistinguishable from one
  that never started, which is precisely the state being diagnosed.

`TPDF_RAISE=1` also exists now, and raises the window for a run that has nowhere visible to
put one. It did **not** fix this, and is kept because occlusion is nonetheless real. Opt-in
rather than the default, because raising a window over whatever someone is doing,
every time a check runs, is its own bug --- the scroll benchmark raises unconditionally
only because an unfocused window would falsify its numbers.

### A harness that prints only at the end cannot say where it stopped

Found by the entry above, and the reason it took an afternoon. `viewercheck.ts` collected
every result and printed them in one block at the very end, so a run that did not reach the
end printed **nothing at all** --- identical to a run that never started, and identical to
a run whose first line of code never executed. The only fact available was that the process
was alive.

It now prints each result as it is recorded. The lines are chained through one promise
rather than awaited at the call site, so `check()` stays synchronous and the transcript
cannot arrive shuffled --- `invoke` resolves out of order under load, and out-of-order
results are worse than late ones.

The general form, and it is the same shape as the crash test that compiled away one level
up: **buffering a report until the end makes every partial failure look like the same
failure.** If a harness can stop midway, it has to be able to say where.

### A mean cannot test a claim about a minimum

`docs/PLAN.md` §9 requires that the visible page area is **never** below its tier-1
placeholder. The scroll benchmark reported the mean coverage across frames, it printed
`100%`, and that was very nearly written up as the criterion being met.

It is not evidence. A mean that rounds to 100% over 300 frames is entirely consistent with
one frame that showed nothing --- which is precisely and only the thing the criterion
forbids. The statistic could not express the failure, so it could not test for it, and a
pass read off it would have been an assertion wearing a number.

The fix is a `floor` column: the worst single frame, of the worst round, not the mean of the
per-round minima --- averaging minima hides the bad round behind the good ones exactly as
averaging frames hid the bad frame. It reads 100% on both corpora, so the conclusion did not
change; what changed is that there is now something behind it.

**Match the statistic to the quantifier.** "Never" and "always" need a min or a max; "95% of
the area" needs a mean; a tail requirement needs a percentile. A number that cannot go bad
when the property does is decoration, and it is the same failure as the crash test that
compiled away and the leak scanner that could not decode its carrier --- three different
subjects, one shape.

### A frame-rate pass means nothing without a coverage number beside it

Spike 0.8 scrolled the A0 vector page at a flawless **60 fps, zero dropped frames, in every
variant** — over a screen that was **0–4% sharp**, and at 400% zoom showed nothing at all
for about a fifth of its frames. Both statements are true of the same run. The frame loop
is decoupled from the renderer on purpose, so the thing a frame-rate test measures is the
compositor, and a scroller that has given up entirely posts the best numbers in the table.

So any smoothness measurement needs a second metric asserting something was *on screen* —
here, the fraction of visible page area backed by a sharp tile, plus a second fraction
counting the tier-1 placeholder, since "blurry" and "blank" are different failures. Same
lesson as the crash test that compiled away and the sandbox that rendered `ok` with the
wrong font: **assert on what was produced, never on the absence of a complaint.**

The harness itself failed this way first, and more sharply. Clearing tier 2 *after* the
warm-up — to guarantee the timed section scrolled over unrendered content — left the
warm-up's four requests outstanding but invalidated, so against a one-second-per-tile page
the in-flight limit was never released and the timed section could not issue a single
request. It reported a perfect 60 fps over a document it had not asked for. The tell was
`requested: 0`, which is why the counter is in the output at all.

### Interleaving controls for drift, not for what the last variant left behind

The standing rule for a performance comparison is to interleave A,B,A,B and compare
pairwise, because wall clock drifts over minutes. That defends against the machine changing
underneath the measurement. It does **not** defend against variant A leaving work in a
shared, stateful component that variant B then inherits — and when the component is a
single FIFO render thread whose backlog outlives a round, that is the larger effect by far.

Measured 2026-07-27 comparing withdrawal of stale tiles against leaving them to render. On
the A0 sheet, whichever variant ran **first** reached 100% tier-1 coverage and the better
sharp figure; whichever ran second reached 75% and the worse one. Swapping the order swapped
the result exactly, so both orderings "showed" that the first variant was better. Reading
either run alone gives a confident, reproducible, wrong answer — and the pairwise comparison
the interleaving rule prescribes is what produces it, because the pairs are always
first-then-second.

The fix is to drain between variants, not to average across orderings: `scrollbench.ts`
waits for a variant's outstanding requests to settle before the next one starts, bounded,
and shouts if the bound is hit. With that in place the two variants measured identically at
a slow scroll and reproduced within a tile at a fast one, in both orderings.

The general form: **before trusting an A/B, ask what each variant leaves behind that the
next one can find.** Caches, queues, background threads, page-cache state, a warmed JIT. If
the answer is "something", run the orderings both ways as a control — if they disagree, the
harness is measuring the order.

### WKWebView presents at 59 Hz on a 120 Hz display

Measured 2026-07-26 on the M5 MacBook Pro, whose panel reports a `120.00Hz` mode: an idle
`requestAnimationFrame` loop returns a **17.0 ms median interval**. Ruled out, each with
its own control in the benchmark's header — mains power (re-measured on AC), a
non-visible page (`document.visibilityState`), and an unfocused window
(`document.hasFocus()`, with the app raising itself via `set_focus()` before the run).

Two consequences. tpdf's scroll budget is **16.7 ms, not 8.3 ms**, so frame numbers here are
against the easier of the two cadences this hardware can present at — and tpdf cannot scroll
as smoothly as a native app on the same machine, whatever it does. It is the shell floor
again, in the time domain.

Never assume 60 Hz when deciding what a dropped frame is. Time an idle loop first and state
every threshold as a multiple of what it returned; a threshold derived from an assumed
cadence reports drops that are not drops, or misses the ones that are.

### `performance.now()` is clamped to 1 ms — average, do not take a median

The webview clamps the clock, which is visible in the integer-valued series spike 0.1 saw.
For anything smaller than a millisecond this is fatal to the obvious statistic: our
per-frame scroller work costs ~0.1–0.6 ms, so *every individual sample* reads as 0 or 1, and
the median is only ever one of those two values. The **mean over hundreds of samples**
recovers a usable figure from a clock that cannot resolve one, and a rate taken across the
whole run (frames ÷ elapsed) is immune to the clamp entirely.

Probe the resolution rather than assuming it — spin until the value changes — and print it,
so no claim finer than the clock's step is made by accident.

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
for abandoning a tile the viewport has already left. (An earlier version of this entry also
credited it with releasing the global mutex between `Continue` calls. There is no global
mutex --- see the `thread_safe` entry above.) Use the progressive API for anything that is
not a small, bounded tile.

Reaching it is not free, though: `PdfDocument::handle`, `PdfPage::page_handle` and
`PdfBitmap::handle` are all `pub(crate)` in `pdfium-render`, so the progressive functions
--- which are public on `PdfiumLibraryBindings` and take raw handles --- **cannot be called
on anything the safe API produced.** Cancellable rendering therefore means owning
`FPDF_DOCUMENT`, `FPDF_PAGE` and `FPDF_BITMAP` ourselves and driving the render through the
bindings trait. The safe wrapper is all-or-nothing: use it and you cannot cancel. This
points the same way as the worker design, whose processes want raw handles regardless.

`src/progressive.rs` is that ownership, and `bin/progressive_probe.rs` measured it. An
uncancelled progressive render is **byte-identical** to `render_into_bitmap_with_config`
on every fixture tried, sliced or not --- which is also what asserts the flags, clear
colour and placement, since `pdfium-render` re-exports the handle *types* out of its
private `bindgen` module but not the `FPDF_ANNOT` / `FPDF_REVERSE_BYTE_ORDER` /
`FPDFBitmap_BGRA` constants, so those had to be restated by value. One documented gap: the
safe path also calls `FPDF_FFLDraw` to overlay interactive form-field appearances, and the
progressive path does not.

### PDFium decides how often it can be interrupted, and the slice does not change it

Measured 2026-07-27 on the A0 sheet, one 1024² tile at 1x, a 6.4 s render
(`progressive-probe --mode poll`). PDFium polled `NeedToPauseNow` **268 times, and exactly
268 times in every variant** --- with no slice, with a 16 ms slice, and with a 0 ms slice
that asks it to stop at the first opportunity. The polls are wherever PDFium's own work
divides; a slice only chooses *which* of them we pause at.

So the granularity is not a tuning parameter. Mean gap 24 ms, worst observed **66 ms**, and
asking for a 1 ms slice cannot beat that --- the number says how long PDFium goes without
asking, not how long we are willing to wait. Any latency budget has to be written against
the poll spacing, and any claim of finer control is a claim about a knob that does nothing.

What it buys is nonetheless decisive: cancellation from another thread returned in
**0.25--24 ms against a 6.3 s render**, and slicing costs about 1--2%, which is inside the
round-to-round noise. Note the cancelling thread sets an `AtomicBool` and touches no PDFium
state --- that is the only reason a second thread is involved at all.

Two traps in testing this. A **cheap page offers no interruption points**: the text corpus
renders a tile in 1.5 ms with `polls: 1`, so a cancellation test there passes by never
being exercised, which is why the probe fails a run whose sliced variant never actually
paused. And a cancelled render leaves a **genuine partial composite** in the caller's
bitmap rather than an untouched buffer --- PDFium composites as it goes --- so the buffer
is not safe to reuse on the assumption that a cancelled render wrote nothing.

### `FPDF_LoadPage` re-parses every time, and on a complex page that is 44 ms

PDFium does not cache a loaded page. Measured 2026-07-27 (`progressive-probe --mode
pageload`), loading page 0 of an already-opened document, repeatedly:

| corpus | per load |
|---|---|
| `text-heavy` | 0.18 ms |
| `vector-heavy` (A0) | **44.3 ms** |

The cost tracks page complexity, because what is being repeated is the content-stream
parse. So "load the page, render a tile, drop the page" --- which is what `render.rs` does
today, once per tile request --- charges a screenful of six tiles **266 ms of pure
re-parsing** on the one document where latency is already the problem, and nothing at all
on the corpus most likely to be used for testing. That asymmetry is why it went unnoticed:
the cheap page hides it perfectly.

`RawDocument` now keeps a small bounded cache of page *handles*. Handles rather than
`RawPage` values because a `RawPage` borrows its document, so storing one inside the
document would be self-referential --- whereas a handle is a plain pointer, copied out
under a short borrow, with the document closing every one it holds on drop. Note the cache
bound (4) is untuned; it was picked to be obviously safe.

Two things to keep: **evict outside the `RefCell` borrow**, or a PDFium call can re-enter
it and panic. And keep `evict_page` even though nothing in the viewer needs it yet --- it
is what lets the probe measure the uncached path, and a cache whose value cannot be
re-measured after a bump is a cache taken on faith.

### `Instant` on Apple Silicon ticks at 41.67 ns, so "elapsed == 0" is reachable

A latent bug, and worth the entry because the shape generalises. A pause deadline was
stored as nanoseconds-since-origin in an `AtomicU64`, with **0 meaning "no deadline"**.
Arming a *zero-length* slice a few nanoseconds after taking the origin should give a
deadline of a few nanoseconds --- but `Instant` here is backed by the 24 MHz timebase, so
its resolution is about **41.67 ns** and two reads that close together return the same
value. `origin.elapsed()` was genuinely 0, collided with the sentinel, and turned "pause at
the first opportunity" into "run to completion".

It was intermittent, because whether the two reads land in the same tick depends on what
the caller did between them: adding the page cache changed the timing enough to expose it,
having been silent until then. The fix is `u64::MAX` as the sentinel --- a nanosecond count
that cannot occur, rather than one that merely seems unlikely.

**A "no value" sentinel drawn from the value's own range is a bug waiting for the right
timing.** Zero is the worst possible choice for an elapsed-time field, because zero elapsed
time is exactly what a fast path produces.

What caught it was not a test of the sentinel. It was the probe's rule that a sliced render
reporting `resumes: 0` **fails** --- the control that exists so a "pausing is lossless"
result cannot be produced by never pausing. Without it, slicing would have silently stopped
working and every identity check would still have passed, because a render that never
pauses is byte-identical to one that never had to.

### Three similarity metrics in a row, each unable to see its own failure

Characterising a cancelled tile went through three metrics before one behaved, and all
three failures are the same shape as the crash test that compiled away:

- **"Fraction of pixels with ink"** --- defined as *not opaque white* --- reports an
  untouched, all-zero buffer as **100% ink**. It cannot distinguish "PDFium drew the whole
  tile" from "nothing ever wrote here", which is precisely the distinction it was added to
  make. Fixed by reporting the all-white and all-zero fractions separately.
- **Exact pixel match** reports **0.00%** for a partial that is visibly most of the way
  there, because the A0 fixture is antialiased random linework covering every pixel and no
  pixel is ever exactly right until the last stroke lands.
- **Mean absolute channel error** reads **45.1, 44.4, 44.2, 45.3** for cancellations at
  0.5, 1.5, 3.0 and 5.0 s of a 6.3 s render. It does not converge, because the distance
  between two dense random-colour images is roughly constant whatever fraction is drawn.

The fixture saturates all three, which is a property of a fixture built to be a
pathological *renderer* stress test and never intended as a *perceptual* one. What can
still be proven on it: the partial is not the untouched buffer, not the cleared buffer, and
not the finished tile, and an early cancellation differs from a late one. Whether a partial
tile is worth putting on screen is **not measured** and needs a realistic drawing.

The rule that survives all three: **a similarity number is not evidence until you have
shown what it reads on the failure you are trying to exclude.** Dumping the tiles and
looking at them took two minutes and settled what an hour of metrics could not.

### `FPDFText_GetText` drops characters, so it cannot be indexed alongside boxes

It extracts UCS-2 and, in its own documentation, "ignores characters without UCS-2
representations". Every other text API --- `FPDFText_GetCharBox`, `GetUnicode`,
`GetCharIndexAtPos`, the search functions --- is keyed by *character index*. So the string it
returns and the indices everything else speaks are two different sequences, and they diverge
exactly on the documents nobody tests with: CJK, symbol fonts, anything astral.

The symptom is not a crash or an error. It is a selection that highlights the right
rectangles and copies text from a few characters further along, on one document out of
twenty. `src/text.rs` therefore sends **one Unicode scalar per index** and no string at all,
and the frontend builds a string from the range it selected. Same rule as `set_text()`
drawing `.notdef`: work in the code space the document uses, never in a re-encoding of it.

**This is also why search does not call `FPDFText_FindStart`.** PDFium's search API is what
Chrome's Ctrl-F uses and would have been far shorter --- but it matches against that same
extracted string and answers in positions into it: a second index space carrying the same
divergence, which then has to be mapped back onto the boxes. Matching over the codes instead
(`src/search.rs`) makes a hit a range of the indices the boxes are already keyed by, so there
is no mapping left to get wrong. The shorter route is not cheaper; it is the same work with
the failure moved somewhere no test would look.

### A page carries `/Rotate`, and PDFium answers in two coordinate systems at once

Nothing in the corpus had one until 2026-07-27, and `/Rotate 90` is what a scanner emits.
For such a page PDFium reports:

* `FPDF_GetPageWidthF` / `GetPageHeightF` --- and `FPDF_GetPageSizeByIndexF`, measured, which
  is not obvious since it reads the page dictionary rather than the loaded page --- give the
  size **after** rotation, and a render comes out rotated to match. Layout and tiles were
  already right.
* `FPDFText_GetCharBox` and `FPDFDest_GetLocationInPage` give coordinates in the page's own
  **unrotated** space.

So the obvious flip, `height_pt - y` against the reported height, is correct at `/Rotate 0`
and wrong at every other value. Measured with `text-probe --mode align` on
`testdata/rotated.pdf`, per page: **100% of character boxes landed on ink at 0 and 0.0% at
90, 180 and 270.** Not approximately wrong --- every selection, every search highlight and
the whole screen-reader reading order was somewhere else entirely, in tidy rectangles, on
exactly the documents a scanner produces.

Three things worth carrying:

- **One mapping, two callers.** `text::to_device` turns a box; `outline.rs` places a
  destination by handing it a degenerate one. A second implementation of the turn is a
  second place to get it wrong, and the destination case is the one nobody would test.
- **A quarter turn makes the display's vertical axis the page's horizontal one**, so a
  destination that names no `x` cannot be placed at all. That path returns "no coordinate",
  which is what `/Fit` means anyway --- and it needed a fixture entry (`/XYZ null 600 0`) built
  for it, because without one the guard could be deleted and nothing would notice.
- **Reading the rotation costs a page load.** `FPDFPage_GetRotation` needs an `FPDF_PAGE`,
  while everything else in the outline walk reads the page dictionary. Measured on
  `outline-simple`: the walk went **0.17 ms -> 7.5 ms** (45.7 ms on a cold first run), about
  1 ms per distinct page named with coordinates. A three-hundred-entry table of contents is
  therefore a third of a second of the render thread, which is why `App.svelte` now waits for
  the first screen before asking for the outline at all.

### A line-grouping rule assumes an axis, and the axis is not always vertical

The consequence of the entry above, found the same day and worth its own note because the
first fix did not imply the second. Characters are grouped into lines by **vertical**
overlap --- which is right for a page read left to right, and on a `/Rotate 90` page puts
every character on a line of its own. The screen-reader layer then reads the page **letter
by letter**, and the selection highlight becomes one rectangle per glyph.

Every text assertion still passed. `textContent` is identical either way; only the block
structure differs, and the check that noticed compared a page's accessible text against an
independent extraction and got 877 characters against 534 --- the difference being the spaces
introduced by joining one block per character.

So the grouping takes the axis from the page's rotation. The honest limit, stated because it
is easy to believe otherwise: this fixes the *whole-page* case only. A rotated **run** inside
an upright page --- a sideways table header --- is still split character by character, as it
was before.

### PDFium's render rotation composes with `/Rotate`, and wants the turned size

`FPDF_RenderPageBitmap_Start` takes a rotation argument, and it is the right mechanism for
rotating the *view*: PDFium's display matrix applies the page's own `/Rotate` first and this
on top, so "turn it a quarter clockwise" means the same thing on a scanned page as on an
upright one. Two things about it are easy to get wrong in the same direction.

The `size_x`/`size_y` arguments are the **displayed** dimensions, so a quarter turn swaps
them. PDFium fits the page into the rect it is given and rotates inside it; passing the
upright size squeezes a landscape page into a portrait box rather than turning it. The
symptom is a page that is recognisably the right content at the wrong proportions, which
reads as a tiling bug.

And the text side does *not* go through the same call. `FPDFText_GetCharBox` knows nothing
about a view rotation --- it is not a property of the document --- so the boxes have to be
turned in our own code. That gives two implementations of one idea, which is what the
composition rule is for: turning a device-space box by `v` after `to_device(p, …)` must equal
`to_device((p + v) % 4, …)` of the raw box. `text.rs` asserts it for all sixteen
combinations, and it is the only thing tying the frontend's turn to the mapping that was
verified against pixels.

Note the two are separately capable of being wrong, in ways that hide each other. A view
that turns its boxes and not its render, or its render and not its boxes, still selects text
in tidy rectangles --- the wrong ones. `text-probe --mode align --view-turns N` renders with
the rotation and maps with it, so it catches either half; mutating each was worth doing,
because dropping the dimension swap goes red only on odd turns and reading the rotation as
zero goes red on all three.

### Two rotation tables, disagreeing at every turn but zero

A rotated page has two reading directions and they are not the same table. **Across** the
lines, the first line is at the low end of the axis at no rotation and at three quarter
turns --- because one quarter turn sends the page's y to the display's x *decreasing*, and
three send it to x increasing. **Along** a line, reading runs in the increasing direction at
no rotation and at *one* quarter turn.

Written as one table --- which is the obvious mistake, since both are "is it rotated" --- a
check that drags two lines out of a rotated page went red on two corpora and passed on
neither. Derive each from where the page's top-left corner ends up; do not derive the second
from the first.

### PDFium's character order is not the page's line order

A check asserted that text dragged from nearer the start of a page has a lower character
index. On `text-heavy` that is true. On `rotated-90.pdf` it is not: dragging out `Line 03`
gives index 405 and `Line 10` gives 90, so the extraction runs the other way --- and the
check went red against a rotation that `text-probe --mode align` had already confirmed
correct at 100% of character boxes.

That is a claim about PDFium and the document, not about the code under test. What is ours,
and what the check now says, is that **the same two lines come back**: sample two lines
before the rotation and the same two after, from wherever the rotation should have put them.
It assumes nothing about extraction order, and a rotation applied backwards still returns the
other line.

Compare exactly, though, and it fails for a reason that is not a defect: rotating refits the
page to the window, so the zoom changes and the drag endpoints land a character further in or
out. `"ine 03 charlie delta ech"` against `"Line 03 charlie delta ec"` is the same line.
Compare the core of the shorter string --- that tolerates an edge and not a line.

### A check that derives its inputs from the thing it is testing cannot fail

The fourth instance of this shape in this project, and the most disguised. "The same lines
come back out of a rotated page" computed its drag positions from the viewer's own turned
character boxes, then asked the viewer's caret which characters were there. Delete the line
that tells the text layer about the rotation and it is wrong **consistently**: the sample and
the caret agree, the same lines come back, and the check passes over a selection ninety
degrees out from the page on screen. It survived that mutation.

What catches it is a direct assertion on the wiring rather than on the geometry --- the text
layer must *report* the turned rotation and the swapped page width, checked against a second
fetch from the backend. Cheap, and unsatisfiable by self-consistency.

Two smaller ones found the same way, both gaps rather than wrong answers:

- **Nothing in a viewer check looks at a pixel.** Drop the rotation on its way into the tile
  URL and the boxes still turn, the layout still turns, and the page underneath is upright.
  The check for it fetches one tile at two rotations and asserts they differ --- with the
  control beside it that the *same* rotation twice is byte-identical, or "they differ" is
  satisfied by a renderer that is merely non-deterministic.
- **The viewer and the scroller each keep a rotation, and either can be wrong alone.**
  Checking the zoom covers the viewer's; a scroller laying every page out upright survived
  it completely, producing a narrow page inside a correctly refitted window. The scroller's
  own laid-out page box has to be asserted separately, and its aspect is the thing to assert:
  the mutation's detail line read `aspect 0.773 then 0.773`.

### A dense page of uniform lines cannot detect a y-flip

Character boxes come in page space (y up, origin bottom-left) and everything downstream wants
device space (y down, origin top-left). Getting that flip wrong is the classic failure here,
and it does not look like a bug --- the highlight is still made of tidy rectangles, just on
the wrong lines.

`bin/text-probe --mode align` checks it against pixels: render the page, and for each drawable
character ask whether its mapped box covers ink. On the four small fixtures the correct
convention scores **100%** and the flipped one **4.1--4.8%**. On `text-heavy.pdf` the correct
one still scores 100% --- and **the flipped one scores 69.9%**, because a page of evenly
spaced identical lines has ink almost everywhere a mirrored box could land.

So the corpus most likely to be reached for is the one where the check is blind, and the
probe fails the run and says so rather than printing the 100%. The general form: **a control
that discriminates on one input may not on another, and "the check passed" is only meaningful
alongside "the check could have failed here".**

Two smaller notes from building it. A whole-page ink bounding box is not an oracle --- the
text fixtures draw a frame, so the ink box is far bigger than the characters and *neither*
convention matched it; per-character is both stricter and indifferent to other content. And
whitespace has a box and no ink, so it has to be excluded or it puts a floor on the failure
rate that has nothing to do with the mapping.

### `evict_page` can dangle a live `RawPage`, and the borrow checker allows it

`RawDocument::evict_page` takes `&self` --- it has to, since the cache is behind a `RefCell`
--- and closes the `FPDF_PAGE`. `RawPage` also borrows `&self`. Two shared borrows coexist
happily, so this compiles:

```rust
let page = document.page(0)?;
document.evict_page(0);       // closes the handle `page` holds
let _ = page.width_pt();      // use after close, and rustc is fine with it
```

`RawPage` has no `Drop`, so `drop(page)` does not end anything either --- and clippy rejects
it as a no-op, which is how this surfaced. Scope the borrow instead. It bit while writing
`text-probe --mode extract`, whose whole method is load, extract, evict, load again.

### A timer that starts after the setup measures the wrong thing, and reports it

`text-probe --mode extract` compares extracting from a cached page against an uncached one.
Written with `document.page()` before `Instant::now()`, both columns measured extraction
alone: they came out identical on the text corpus (1.43 vs 1.43 ms, which reads as "the page
cache does nothing") and reported the A0 sheet's *uncached* case at **0.116 ms**, against a
page load independently measured at 44 ms.

Moving the load inside the timer gives 1.42 / 1.64 ms and 0.12 / **43.2 ms**. The tell was
available before the fix: a column named for a cost that is 44 ms cannot read 0.1 ms. **When
an A/B shows no difference, check that the variable is inside the measurement before
concluding it does not matter.**

### `FPDFBookmark_GetDest` follows the bookmark's action without checking its type

The narrow-sounding accessor is not narrow. When a bookmark has no `/Dest`, PDFium's own
implementation falls back to `FPDFBookmark_GetAction` and returns **that action's `/D`
array**, with no check on the action's `/S`. So an entry meaning *"open other.pdf at page
1"* --- a `/GoToR` --- comes back as an ordinary destination, and
`FPDFDest_GetDestPageIndex` then resolves it against **this** document.

Measured 2026-07-27 on `outline-hostile.pdf`: the entry titled "Remote goto" reported
`page 1`. Not an error, not a refusal --- a plausible page of the file the reader already
has open. The same fallback reaches `/Launch` and `/URI` actions, which happen to carry no
`/D` and so come back null; nothing about the API guarantees that, and a `/Launch` with a
`/D` would be followed just as silently.

The fix is an ordering, not a filter: **read the action first**, obey its type, and consult
`FPDFBookmark_GetDest` only when there is no action at all --- which is precisely the case
where its fallback has nothing to reach. It also happens to be what PDF 32000-1 §12.3.3
says, `/Dest` being forbidden alongside `/A`.

What makes this worth an entry is that the wrong version *works*. Every ordinary outline
resolves identically, because ordinary entries have a `/Dest` or a `/GoTo`. Only a document
carrying a remote destination behaves differently, and it behaves plausibly. It was caught
by a fixture built to contain one, not by reading the header.

### An outline can be infinite, and PDFium says so in its own documentation

`FPDFBookmark_GetNextSibling`: *"the caller is responsible for handling circular bookmark
references, as may arise from malformed documents."* That is not a caveat to note, it is
the library declining to bound a walk over attacker-controlled data --- and the obvious
loop hangs the render thread forever, with no output and no error.

`src-tauri/src/outline.rs` carries three bounds, and the point is that each catches
something the others cannot:

- **A visited set** stops a cycle at its first repeat. Note the alternative that looks
  equivalent --- abandoning the sibling list on a repeat --- loses every entry *after* the
  loop, which on the fixture is nine of the ten top-level items.
- **A depth bound** stops a chain that is deep without ever repeating. 200 distinct nested
  nodes put nothing in the visited set twice.
- **An item budget** stops everything else, including a visited set defeated by PDFium
  handing back a fresh pointer for a node already seen. The set is the mechanism expected
  to work; the budget is what makes termination not depend on that expectation.

Deleting the visited set does **not** hang --- the budget catches it --- which is exactly
why `outline-probe` asserts that the budget was *not* what stopped the walk. Without that
control the run still says "18 checks", the cycle is unnoticed, and 14 of them go red for
reasons nobody would connect to a loop. Measured: removing it drops the hostile fixture
from 18/18 to 4/18.

Whatever any bound cuts is counted and reported rather than dropped. An outline shown as if
it were complete when it is not is the same failure as a leak scanner reporting clean on a
carrier it could not decode.

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
python3 testdata/make_vector_pdf.py testdata/vector-multi.pdf 200000 12  # Phase 1
uv run --with pyhanko --with cryptography \
    testdata/make_incremental_pdf.py testdata                 # incr-*.pdf, spike 0.6
python3 testdata/make_outline_pdf.py testdata                 # outline-*.pdf, Phase 1
python3 testdata/make_rotated_pdf.py testdata                 # rotated*.pdf, Phase 1
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

`make_outline_pdf.py` writes `outline-manifest.json` the same way, and one of its
expectations is deliberately **weaker** than the rest. The unpaired-surrogate title is
marked `observed`, not `required`: PDFium may repair or drop it while parsing the document
string, in which case the fixture proves nothing about our decoder and asserting on it
would be a check that cannot fail. What pins the decoder is a unit test in `outline.rs`
that hands it the bytes directly, where the input is ours. **When a fixture cannot
guarantee it delivers the input a check needs, move the check to where the input is
controlled and say so in the manifest** --- rather than writing an assertion whose meaning
depends on a library's parsing.

Its hostile fixture is also verified by an independent oracle: `qpdf --check` reports
*"loop detected in /Outlines tree"* against it. A fixture built to be cyclic that no other
implementation agrees is cyclic is a fixture that might simply be wrong.

`make_rotated_pdf.py` writes two files and they are not interchangeable. `rotated.pdf` has
one page per `/Rotate` value, for `text-probe --mode align`, which takes a `--page` and needs
all four. `rotated-90.pdf` has every page at 90 because the viewer cannot use the first one:
its scroller lays every page out at page 1's size, so a document alternating portrait and
landscape is drawn wrong for reasons that have nothing to do with rotation, and a check run
on it would be measuring that instead. The second also carries an outline, which is what pins
the *destination* half of the same conversion --- including one entry with `/XYZ null 600 0`,
which exists so that the "this destination names no horizontal coordinate" path has an input
that reaches it.

`vector-multi.pdf` is twelve A0 pages sharing **one** content stream object, which is the
point rather than a shortcut: PDFium re-parses a page on every `FPDF_LoadPage` and rebuilds
per-page state on every render call, so twelve pages pointing at one stream cost twelve
times as much to draw while the file stays under 3 MB. It exists because the page strip's
yield needs a document whose *background* work outlives a moment, and the single-page
`vector-heavy` cannot provide one.

Its page count is load-bearing and was raised from three. The check suite visits page 1 and
the last page, so on a short document the viewer has already made a tier-1 placeholder for
every page and the strip borrows all of them --- rendering nothing, and giving the yield
check nothing to catch. **A fixture built to exercise a slow path has to be long enough that
the rest of the run does not warm it.**

---

## Repository facts

- GitHub: `tstone-1/tpdf`, **private**.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` --- anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
