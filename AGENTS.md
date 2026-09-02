# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

Two things this file does *not* carry in full. The trap list lives in
[`docs/TRAPS.md`](docs/TRAPS.md) and is indexed by title below; the worked-out account behind
each rule --- the measurements, what they cost, and which earlier sentence they corrected ---
lives in [`docs/RATIONALE.md`](docs/RATIONALE.md), which the three long sections here point at.
Neither is auto-loaded, on purpose, and the indexes exist so that the decision to read an entry
is an informed one rather than a guess. Code comments and the other documents say "`AGENTS.md`
records ..." in about a hundred places; those references were written when all of it lived here
and are still good in one hop --- read them as naming whichever of the two files carries the
paragraph.

No count of the entries is written here. The authority is `grep -c '^### ' docs/TRAPS.md`, the
*titles* have a gate behind them (`traps` in `scripts/gates.py`, which diffs the two sets), and
a count in prose has none --- which is the whole reason the gate compares sets rather than
totals, and why three copies of that number here once said 275 and 282 at once.

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
structurally incomplete, and the gap is the whole product.** It sees every cargo package, and it
is blind to the **C++ libraries compiled into libpdfium** --- FreeType, ICU, libjpeg-turbo,
libpng, libtiff, Little CMS, OpenJPEG, zlib, Abseil, AGG, fast_float, simdutf, llvm-libc ---
because no cargo command can see inside a prebuilt blob, and that blob is the thing that
actually parses PDFs. A sweep complete over cargo and silent about everything else passes
exactly like one that covered everything, which is the consistency-versus-completeness trap
arriving in the licensing constraint the entire project rests on. The gate enumerates
`vendor/pdfium/licenses/` as a third population, so a new file appearing there is a finding.

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

**Windows no longer fails open.** `Backend::default_here()` selects workers there, proved by
the external module check above rather than by the absence of our own warning.

Worth knowing rather than inferring, because it is a real asymmetry with macOS: the Windows
bound is on **committed** memory, which the kernel charges at `VirtualAlloc` time, so a
decompression bomb is refused *before* a byte of it exists. macOS bounds *resident* memory, so
its balloon has to write to every page it takes. That is why `Worker::footprint` returning
`None` on Windows is not the gap it looks like --- there is a kernel bound there instead of a
poll, and it is now the measured kind. (Nothing in production reads `footprint` on either
platform; only `pool-bench` does.)

**Printing works on both platforms, and only the readback corresponds.** macOS refuses to
open a panel for a job PDFKit cannot read; Windows refuses for one `Windows.Data.Pdf` cannot
read. Both are the platform's own PDF stack, so both are independent of the `lopdf` that wrote
the job and the PDFium that drew what the reader saw --- which is the property the whole print
subsystem is built on, and the same standard `docs/PLAN.md` §6 sets for a redaction.

The half that does **not** correspond is the printing itself, and it is not a shortcut. macOS
hands PDF bytes to `NSPrintOperation` and the OS paginates and prints them as vectors. Windows
has no in-box "print this PDF" API at any layer --- not Win32, not WinRT --- so every Windows PDF
viewer, SumatraPDF included, rasterises each page onto a printer device context itself, and that
is what `print_win.rs` does. Two consequences to state rather than discover: Windows output is
**raster at 300 dpi** where macOS is vector, so text is not selectable in a print-to-PDF result;
and `Windows.Data.Pdf` reports page sizes in **DIPs at 96 to the inch**, not PDF points, which is
a trap with an entry because getting it wrong renders every page 1.33x too large and still looks
fine.

**Printing maps a PDF parser into the app process, on both platforms**, which is the
honest complication in "the app process never maps the PDF parser". It is measured rather
than glossed: `print-probe` reads its own module table and finds none named pdfium, with
`Windows.Data.Pdf.dll` beside it as what it mapped instead. The boundary's real guarantee is
narrower than the sentence sounds --- no *our* PDFium, and the parser that is there is patched
by Windows Update rather than pinned in `Cargo.lock`.

**A Windows distributable builds** (2026-07-30): an MSI and an NSIS installer, from
`npm run tauri build`. It did not, and the cause is worth knowing because it is a rule about
this repository's layout rather than a Tauri bug: **`src/bin/` must contain only declared bin
sources.** The bundler enumerates that directory and registers the first entry no `[[bin]]`
`path =` claims; a `.rs` file is always claimed, a *subdirectory* never is. So
`src/bin/backend_probe/`, which existed only to hold `imp.rs`, became a phantom binary named
`backend_probe`, colliding with the component id WiX derives from the real `backend-probe.exe`
and failing `light.exe`. The two `imp.rs` bodies now live in `src/probes/`, reached by
`#[path]`, which leaves module parentage and every `super::` in them unchanged.

**And it no longer ships the spikes.** Until 2026-07-31 the installer carried all 17 probe and
benchmark executables --- a sandbox prober and a hostile-document harness among them --- because
they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets now: cargo builds
and links them exactly as before, the `bins` gate keeps covering them through `--examples`, and
the bundler does not see them, so the MSI payload is three files (`tpdf.exe`, `tpdf_lib.dll`,
`pdfium.dll`) and about half the size it was. The invocations moved with them: `--example <name>`,
and built artifacts sit in `target/release/examples/`. **That gate flag is load-bearing, and was
proved so rather than assumed** --- without `--examples` the `bins` gate covers only the app, and
an undefined extern called from one example's `main` is what turns it red with `LNK2019`.

**The JavaScript harness does ship, and that is a decision.** `App.svelte` statically
imports every webview entry point, so the functional checks and the benchmarks sit in the
bundle `frontendDist` embeds whole into the binary --- about a third of it. They stay
because the checks observe the artifact that ships, and because the payload is not what decides
cold start. No share is written here, for the reason no trap count is: the one that was
(`77.1 kB of a 221.2 kB bundle`, 2026-08-02) sat in two documents for a month while the bundle
doubled, and nobody could compute the current one from either. The authority is
`scripts/check_bundle_share.py`, which attributes the built sourcemap per module and fails on
two ceilings, and it is the `bundleshare` gate. The honest cost is `spike_print` and
`spike_exit`, registered commands callable by
any script the webview runs; the CSP (`default-src 'self'`, no `'unsafe-inline'`) is what bounds
that, and residual risk 7 in `docs/THREAT-MODEL.md` carries the seam.

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

The account behind this section --- what was measured, what it cost, and which earlier sentence it corrected --- is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The process boundary, rung by rung*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

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
| Hardened structural rewrite | [QPDF](https://qpdf.readthedocs.io/) (Apache-2.0) | Candidate --- not required for the rewrite, and **no longer wanted for encryption either**: `lopdf`'s own `Document::encrypt` preserves it, measured against `qpdf` field for field (2026-08-28). Object streams remain |
| macOS print dialog | PDFKit + AppKit via [`objc2`](https://docs.rs/objc2) (Zlib OR Apache-2.0 OR MIT) | **Settled** --- paginates and runs the panel; also the independent parser every print job is read back with |
| Windows print dialog | `Windows.Data.Pdf` + GDI via [`windows`](https://docs.rs/windows) (MIT OR Apache-2.0) | **Settled** --- reads the job back, rasterises each page onto a printer DC, `PrintDlgW` for the panel. Raster where macOS is vector; see below |
| XMP metadata | [`quick-xml`](https://docs.rs/quick-xml) (MIT) | **Settled** --- reads the catalog's `/Metadata` packet for conformance claims. Already in the tree through Tauri's `plist`, so it adds no package; namespace-aware, and expands no entity |
| Certificates in a signature | [`cms`](https://docs.rs/cms) + [`x509-cert`](https://docs.rs/x509-cert) + [`der`](https://docs.rs/der) (Apache-2.0 OR MIT) | **Settled** --- reads the signer's certificate out of `/Contents`: subject, issuer, serial, validity. Parsing only; there is no trust store and no chain building. PDFium's read-only signature API is not a second implementation but *is* the differential, through `signature-probe` |

The PDFium pin is `chromium/7881`, installed by `scripts/fetch_pdfium.py` and verified by
digest. Every measurement in this file was taken against that build, so bumping it
invalidates them until the two checks in `BUILD.md` are re-run.

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI.

**Since 2026-08-22 the worker also *writes* with `lopdf`.** `Request::Append` builds the update
section for a save that only adds marks, because doing so is a pure function of the document's
bytes and the plan --- and those bytes are the attacker's. It runs where every other parse of
them runs, which narrowed `docs/THREAT-MODEL.md` residual risk 18 from every writing path to
the rewriting ones. The split is by authority: `save::append_ready` asks the coordinator's
questions about a path, `save::append_update` asks none.

**Since 2026-08-28 the in-place *rewrite* runs there too**, which is what took *delete a page
and press ⌘S* off that risk. The obstacle was never the input: it was that a rewrite's answer
is the whole document, against a 32 MB reply limit and files ten times that. So the worker is
handed an **output channel** --- the staging file's own descriptor, on `worker::OUT_FD`, given
at `exec` --- and writes down it. `save::rewrite_update` is the pure half, `save::Rewriter` the
seam, and `save::Outside` names the one choice both seams read. The coordinator holds neither
the document's bytes nor the new file's; what crosses back is a length, which it compares
against the staged file's own size.

That the channel survives the sandbox was measured rather than assumed: the profile says
`(deny file-write*)`, which denies *opening* a path and not a descriptor handed over before
it. `worker-probe` writes one document both ways and compares byte for byte.

**The copy paths, the split and the print job followed on 2026-09-01**, on the same seam ---
`Job` carries what a print does not share with a save, so `staged_rewrite` is one function.
**The page-range print and the merge followed on 2026-09-01**, through
`Request::PrintRange` and `Request::Merge`. The merge is the widest of them --- it parses files
the reader picked in a dialog that tpdf never opened --- and the obstacle recorded for it was
wrong in a way worth keeping: the threat model said each incoming file's object graph would
have to come *back*, when nothing comes back per file. They go **in**, as one read-only
mapping on `worker::IN_FD` with `save::Incoming` naming each, and the merged document goes out
down the channel a rewrite already had.

⚠ **The last one was a *reader* residual risk 18 never listed: `verify::scan`.** The
redaction verification parses the file it just wrote, and it was invisible to that risk, to
`docs/THREAT-MODEL.md` §3 and to `scripts/check_writers.py` alike, because all three enumerate
what **writes** --- the same blind spot that hid `print::build`, twice in two days. The index
has the trap.

**It moved on 2026-09-01, through `save::Verifier` and `Request::Verify`, so on both shipped
platforms no `lopdf` parse of a document happens in the coordinator at all.** `save::Here`
remains the exception and is what a platform with no sandbox gets. Two properties of that move
are not guessable from the feature: the scan needs the reader's password, because a redacted
copy of an encrypted document is re-encrypted and a worker without the key parses no objects
and finds nothing --- an absence that reads exactly like a clean file; and a report is now a
reply read under `MAX_REPLY_BYTES`, so `verify::MAX_OBJECT_REASONS` bounds its per-object lists
and counts the rest. The index has that second one too.

**Windows is wired the same way and is measured**: `worker-probe` is a step of both CI legs,
and it reported 34/34 with nothing skipped on run 33501693368 --- the count at that run;
checks added since have not all been through a Windows leg.

**Since 2026-08-23 a reader can open a document behind a password.** Until then an encrypted
PDF could be chosen from the file dialog and then not opened by any route --- `open_failure`
said so, in a sentence ending *"and tpdf cannot ask for one yet"*, which is a to-do that
reads as a decision. The worker asks and retries the load **in place**, which is legal
because a failed load poisons nothing: measured on `testdata/incr-encrypted-pw.pdf`, four
loads of one buffer in one process open on both correct passwords and refuse on both others.

Two consequences are not guessable from the feature. **PDFium answers the same error for a
document given no password and one given the wrong password**, so the sentence a reader sees
on a retry is chosen in `worker_child::unlock`, the only place that knows one was tried. And
**the password is held for the document's lifetime on `Held::password`**, because every
worker after the first --- pool growth and crash replacement alike --- maps the same bytes and
meets the same encryption; without it a locked document renders the page a reader is looking
at and refuses the next. `docs/THREAT-MODEL.md` §T6.9 states what holding it costs.

**Since 2026-08-23 a reader can also save a mark onto one, and since 2026-08-28 a rewrite
too.** An append never touches the previous revision, and `IncrementalDocument::save_to`
encrypts each appended object with the key the load recorded; a rewrite goes through
`lopdf`'s full serialiser, which writes every object in the clear and drops the `/Encrypt`
dictionary with it --- so `save::rewrite` takes the encryption state off the document before
it touches anything, and calls `Document::encrypt` back on as its **last** step, after the
sweep and after everything that adds an object. `examples/password_probe.rs` runs the append
end to end (986 bytes appended to a 2,346-byte AES-256 document, reopened afterwards with the
same password and refused without it); `examples/encrypted_rewrite_probe.rs` is the rewrite's,
through `qpdf` rather than through the writer that produced it.

⚠ **This paragraph said the opposite for a day, and the stack table above said the truth ---
one file with two accounts of one fact, which is the failure this file's own Quality-gates
section names.** It read *"a rewrite is refused and always will be through that writer"*. The
*always* was the load-bearing word and it was wrong: `Document::encrypt` is public, and the
capability sat closed for months behind a sentence that read as a decision rather than as a
to-do. What is genuinely refused is narrower and worth stating exactly: a document **nobody
unlocked** cannot be rewritten, because there is no state to put back; and a **print job over
part of** an encrypted document is refused, because re-encrypting gives the printer something
it cannot read and not re-encrypting gives it a decrypted copy of a document somebody
encrypted deliberately (`save::print_bytes`, which takes the reader's password precisely so
that *that* refusal is the one they meet).

The password reaches `save::append_update` because the worker holds it, and reaches
`save::append_in_place` because the app process does. That second hop is not optional: the
append re-reads the file it wrote to check the cross-reference chained, and `lopdf` parses no
objects at all without the key, so the check would count zero pages against the two it
expects and roll a correct save back.

**Every one of those `lopdf` parses takes the reader's password too, since 2026-08-23.** It
is one field on `RawDocument` and five call sites, and it is not a nicety: without the key
`lopdf` parses **no objects at all** and returns a `Document` that loads cleanly and reports
zero pages, so a document behind a real password would open, render and search while its
comments, links, properties and character mapping all came back empty --- and empty is the
reassuring answer. `links.rs` and `annots.rs` already carried a `pages_missed` count for
exactly that, which is what `password-probe` asserts against; the comments check exists
because taking the password away from `annots::scan` reddened nothing without it.

**Comments, links and a document's own properties are read through `lopdf`, not through
PDFium, and that is a measurement rather than a preference.** `FPDFPage_GetAnnot` and friends work --- checked on a fixture before
anything was written --- but every one of them needs an `FPDF_PAGE`, and `FPDF_LoadPage`
re-parses each time at up to 44 ms on a complex page. The panel's question is about the whole
document, so through PDFium it is a page load per page; through the object graph it is one
parse the file already needs for `encoding.rs`, at 0.1 ms on a small document and 11.9 ms on
the 337 MB scan. `pdfium-render` also does not expose `/IRT` at all, so a reply arrives there
as an unrelated second note by another author.

**Links take the same route, and it costs a second destination resolver** ---
`outline.rs` asks PDFium because a bookmark is a PDFium object, `links.rs` reads the
destination array itself. That is the drift trap this file's index names, so `links.pdf`
gives its outline entries the same destinations as its links and `links-probe --mode agree`
compares them, both against the manifest rather than against each other. The properties
readout in `docinfo.rs` takes the `lopdf` route too, and since 2026-08-21 also parses the signer's
certificate --- a second ASN.1 parser on attacker-chosen bytes, bounded and sandboxed
accordingly (`docs/THREAT-MODEL.md` §T6.8). `examples/signature_probe.rs` is the differential
against PDFium's own reading of the same file; `BUILD.md` has the invocations.

Two things in that module are traps rather than choices, both in the index: `lopdf::decrypt`
removes the `/Encrypt` trailer entry, so the encryption has to be read **before** it; and the
permission bits do not mean the same thing at every revision, because bits 9 to 12 are
reserved under revision 2 and a negative `/P` sets all four.

**What PDFium does supply is the marks themselves**, and that is why no drawing was added.
`progressive.rs` renders with `FPDF_ANNOT`, so a sticky note's icon and a highlight's wash are
painted inside the tiles --- measured on a fixture carrying no appearance streams at all,
where PDFium generates them: the note icon fills 637 of the 756 pixels in its own rectangle,
the highlight 6,690 of 9,436, and a `/Popup` correctly draws nothing. What no reader could
reach before `annots.rs` was the *text*.

Two crates carry the search.

`regex` (MIT OR Apache-2.0) reads a reader's pattern, and it was already in the tree
transitively through the toolchain, so declaring it added no package. `caseless` (MIT) does
Unicode case folding, which is what makes `strasse` find `Straße`: `char::to_lowercase` is
defined for *displaying* text and leaves a sharp s alone, and folding is the operation defined
for caseless *matching*. It brings `unicode-normalization` (MIT OR Apache-2.0) with
`tinyvec`/`tinyvec_macros` (permissive) --- the only genuinely new packages either of them adds.

Both checked with `cargo metadata` over the whole tree rather than from a README, which is the
standing rule for anything the licensing constraint above touches. The sweep looks for the
copyleft families by name across the whole tree; the only hits are MPL-2.0 (file-level, in
Servo's CSS crates via Tauri) and a triple-licensed `r-efi` whose `MIT OR Apache-2.0` arm
applies, so the licence the repository already grants is intact.

No package count is written here either, for the reason the trap count is not: one in prose
read **531** while the tree held 572, left behind by the updater plugin's 48 crates and again by
the certificate reader's 9. The authority is the command: `cargo metadata --format-version 1 |
python3 -c 'import json,sys; print(len(json.load(sys.stdin)["packages"]))'`.

**Three crates read certificates and one reads XMP, all added 2026-08-21, and the XMP one
adds no package.** `cms`, `x509-cert` and `der` bring nine packages, every one
`Apache-2.0 OR MIT` except `flagset` (`Apache-2.0`); `quick-xml` was already in the tree
through Tauri's `plist`. Both matter to the threat model as much as to the licence, and
`docs/THREAT-MODEL.md` §T6.8 records what bounds them. Nothing reads a signature's `/Contents`
as it arrives: `src-tauri/src/ber.rs` --- about 150 lines, no dependency at all --- walks it
first and hands the parsers a definite-length value, because RFC 5652 requires DER and real
signers emit the indefinite form that `der` refuses outright.

Three plugins are linked. `tauri-plugin-dialog` (Apache-2.0 OR MIT) for the file-open and
file-save dialogs, which pulls `tauri-plugin-fs` (Apache-2.0 OR MIT) and `rfd` (MIT) --- the
capability list in `src-tauri/capabilities/default.json` names `dialog:allow-open` and, since
2026-08-16, `dialog:allow-save`; that second one opens a panel and writes nothing, and what
actually writes is `save_copy` and, since 2026-08-19, `save_document`, whose authority `docs/THREAT-MODEL.md` §T6.1 states; on Windows only,
`tauri-plugin-single-instance` (Apache-2.0 OR MIT), which is what gives that platform the
document handover macOS gets from `RunEvent::Opened`; and `tauri-plugin-updater` (MIT OR
Apache-2.0), which is the largest single addition the tree has taken --- **48 crates,
325 to 373**, because it brings a TLS stack (`rustls`) and archive extraction (`zip`, `tar`).
All permissive, swept as below.

**That plugin is also the only network authority in the application, and it changed a property
that had held until 26.8.2: tpdf made no request at all.** It is spent narrowly --- one check per
launch, issued after every spike and check entry point has returned, so every harness here still
runs offline; nothing downloads or installs without a click; and the payload's signature is
verified against a compiled-in public key before anything is unpacked, which is what keeps those
two new archive parsers from ever seeing attacker-chosen bytes. `docs/THREAT-MODEL.md` §T9 is the
worked-out version, residual risks included. Every dependency added has to be checked against the
licensing constraint above rather than assumed, because one copyleft crate anywhere in the tree
removes the option of making this repository public. The check is `cargo metadata` over the whole
tree, not a glance at the crate's own README.

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

The account behind this section --- what was measured, what it cost, and which earlier sentence it corrected --- is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The PDF layer: what each dependency cost to settle*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

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

**That heading form is measured safe here, and it is not safe everywhere** --- checked
2026-08-16, because the cross-repo notes flag tpdf as a repo where it had been assumed and never
verified. It is dangerous wherever the release tooling selects a CHANGELOG section by matching
the version heading: a prefix match accepts `## [1.1.2] - Unreleased` exactly as it accepts a
dated one, so a forgotten rename publishes a release whose notes say *Unreleased* with nothing
going red. `xlsxturbo` is such a repo and uses a bare `## [Unreleased]` for that reason.
`release.yml` here reads **nothing** from `CHANGELOG.md` --- its `releaseBody` is a literal block
in the workflow --- so no tag can pick up a heading of any shape. The cost is the opposite
failure and it is real: that body cannot go stale by tooling, only by nobody reading it, and it
shipped a **"Nothing here edits a document"** paragraph that a later release made false. Re-read
it on every release.

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

Currently, in the order `--list` prints them: a toolchain-pin check, a PDFium pin check, a trap-index check, a
future-date check, a
workflow-parity check, a workflow-fixture check, a mutation-anchor check, a mutation-suite check, a
corpus-classification check, `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`,
`cargo build --locked --bins --examples`, a webview-sink check, a viewer-wiring check, a
doc-comment check, a command-classification check, a file-writer check, `npm run check`,
`npm run test`, `npm run build`, a bundle-share check, and a third-party-notices check. Three of them are
ordered rather than merely present: `toolchain` runs **first**, because every result after it
is a statement about whichever compiler actually ran, and `notices` runs **last**, because it
reads the build's own sourcemaps to see which npm packages shipped --- with `bundleshare`
between `build` and it, reading the same sourcemaps for a different question.

**Every one of them can be green on a Mac while the Windows tree does not compile**, and that is not
a hypothetical: it was true for sixteen commits until a rehearsal tag for `26.8.3` turned both
runner legs red on `examples/print_probe.rs`. A Mac compiler never parses a `#[cfg(windows)]`
line, so `print_win.rs`, the two Windows probes and the Windows halves of `worker*.rs` sit
outside everything the list covers. `scripts/check_windows.py` closes it ---
`cargo check --target x86_64-pc-windows-msvc --all-targets`, which does not link and so needs
headers rather than a linker. **It costs 1 s warm and over ten minutes cold**, measured
2026-09-02 on the same tree an hour apart; this said "about 8 s" flat, which is the warm figure
for the `check` alone and describes neither run anyone actually makes. The whole cost is
building every dependency for a second target, so the number you get is decided by whether
`target/x86_64-pc-windows-msvc/` already exists --- and a fresh checkout, which is the case the
next sentence is about, always pays the cold one. Budget for it rather than being surprised by
it: a ten-minute timeout on what a document calls an eight-second check reads as a hang. **Deliberately not a gate**: it needs a 629 MB SDK splat a fresh
checkout does not have, and CI runs a real `windows-2025` runner, which is better evidence. Run
it before pushing anything that touches a Windows-only file, and before a tag; `BUILD.md` step
5 has the one-time setup and the reason the missing PDFium DLL reads as a broken checkout.
Its honest limit is that a type-check is not a test --- a wrong *value* passes it.

**Each gate exists because something specific went wrong, and one line each is the index
rather than the account.** `docs/RATIONALE.md` has the full version of every one:

- `toolchain` --- `RUSTUP_TOOLCHAIN` silently overrides `rust-toolchain.toml`, which is what a
  CI action installing its own toolchain may set.
- `pdfium` --- the pin was checked against a digest the installer wrote, and the only fact it
  had about the tree was that *something* named `*pdfium*` existed.
- `traps` --- `docs/TRAPS.md` and this file's index, diffed as sets both ways, because a tally
  can be right while the index is three entries short. Since 2026-08-31 it also holds a bullet
  to its title and this file to a size ceiling: the diff cannot see a bullet's tail, and 323 of
  them took the file past the limit at which it stops being loaded at all.
- `workflows` --- `release.yml`'s `gates` job was copied from `ci.yml` and dropped a whole step,
  so the release gate was weaker than the gate it exists to satisfy. It also asserts what
  authority that job holds, which comparing steps was blind to.
- `anchors` --- every mutation's search string occurs exactly once in the file it names, the
  test it names exists, and that test can go red on this platform. A killed harness's leftover
  edit and a drifted anchor are both invisible in `git status`.
- `mutations` --- every suite `vitest list --json` collects is either mutated or excluded with a
  reason, so a harness omission is caught in twelve seconds rather than after a control pass.
- `corpora` --- every `testdata/*.pdf` is a window corpus with a stated purpose or an exclusion
  with a stated reason; the list used to live in whatever shell loop somebody typed.
- `sinks` --- `docs/THREAT-MODEL.md` T8: no markup-parsing sink anywhere in the frontend, plus
  five rules closing the routes by which document text becomes a navigation or a script. The
  backend half is enforced by the type (`Target::Refused`), and the two halves cannot see each
  other --- that seam is residual risk 7.
- `wiring` --- `Viewer`'s optional callbacks against `App.svelte`'s object literal, both ways.
  The box shipped inert with three layers of tests green, because nothing looks at the literal
  that joins them.
- `docs` --- a doc comment must be followed by code. Two `/** */` blocks in a row bind only the
  second, silently; the first scan found 31 orphans across twelve files. Since 2026-08-28 it
  also has a **Rust** arm, for the mirror failure: two `///` runs with no blank line between
  them are *one* comment, so nothing is lost and the whole thing documents the wrong item ---
  three live instances, one of them introduced while fixing the other two.
- `wiring` also covers `ScrollerOptions` and `ThumbnailOptions` as of 2026-08-28, which with
  `ViewerOptions` is every optional `on*` callback the frontend declares --- 19 of them.
  `AppActions`' 51 members are deliberately **not** here: they are required, so `npm run check`
  refuses a missing one, and a gate over them would have no reachable subject.
- `classified` --- every registered command is in the window harness's `probes` or its
  `undriven` table. That harness asserts it already; it needs a screen and is run by hand,
  so two commands shipped unclassified on 2026-08-29 and the check was red for a day with
  every gate green. This one reads the source text and buys the day, not the certainty.
- `writers` --- every registered command that reaches one of the `save` module's terminal
  writers is named in `docs/THREAT-MODEL.md` §3's list, and the row's count agrees. That row is
  the one place answering *how many ways can the webview cause a write*, and it was wrong three
  times in two weeks, always under-claiming: it said six against a list of five when the answer
  was eight. The section's own rule --- *the list is the claim and the number follows it* --- had
  nobody applying it. Its control reads `save.rs` **and everything under `src/save/`**: it read
  the one file until 2026-09-01, when a split moved code out of it, and a control that keeps
  passing on a smaller file is the failure this gate exists to refuse.
- `dates` --- no date in a tracked file may be later than today. Provenance here is written as
  dated measurements, and on 2026-08-28 there were **70** stamps reading a day or two ahead,
  every one written by a commit dated 2026-08-28. A stamp in the future does not merely
  mislead about one measurement; it makes every stamp written in the same sitting unreliable,
  and nothing else notices.
- `bundleshare` --- the unattended harness ships inside the bundle on purpose, and the argument
  for it was written against a number half the current size, in two documents, unread for a
  month. It attributes the built sourcemap per module and bounds the harness family by share
  *and* by absolute, because the share alone missed a month in which both halves grew together.
- `notices` --- runs last, because it reads the build's own sourcemaps to see which npm packages
  shipped.

**`save.rs` is a directory module since 2026-09-01**, and the reason to know that before
editing it is `scripts/mutate_rust.py`: 54 mutation anchors name `src/save/marks.rs` rather than
`src/save.rs`, and both new files are submodules of `save` on purpose, so the harness's
`save::` filter still reaches their tests. The file went from 14,844 lines to 3,988 --- 61% of it
was its own test module --- with `save/tests.rs` and `save/marks.rs` beside it.
`docs/RATIONALE.md` *Splitting `save.rs`* has what was measured before anything moved; the rest
of the split by concern (the append path, the staging, the worker seam, the rewrite engine) is
not done and is a design question rather than a file operation.

**`App.svelte` is the layer no gate reaches, so state is born outside it rather than extracted
from it later.** Anything shaped like a walk, a set, a cache or a map --- anything holding state
past the wiring and the markup --- starts life as a `src/lib` module with its own unit tests; the
component keeps the object literals that join things and the markup. This is a rule and not a
taste, because three trap entries locate shipped defects at exactly this join and every
extraction so far happened after one: *An id and a slot are both `number`, so a mark drawn on the
last page vanished*, *An "already asked" set keyed by a slot is renumbered by the next deletion*,
and *A feature can be inert in the application while three layers of tests pass*, which is the
`wiring` gate's own founding defect. No test imports `App.svelte`; its net is that gate plus
harnesses that need a screen.

The README is checked against the command registry by `src/lib/readme.test.ts` rather than by a
gate of its own, in both directions: a `<!-- not-built: id -->` bullet may name no registered
command, and every registered command is named in a `<!-- built: -->` marker or excluded in the
test's `UNLISTED` table with a reason. What it does not check is the prose beside the markers ---
`BUILD.md`'s release checklist carries that half, and is a checklist rather than a check on
purpose.

**The Rust toolchain is pinned in `rust-toolchain.toml`** as of 2026-08-02, and the pin is
enforced by `scripts/check_toolchain.py` rather than assumed. `RUSTUP_TOOLCHAIN` overrides
that file silently, which is exactly what a CI action installing its own toolchain may set,
so both workflows use `rustup show` instead of one --- and the gate asserts the result. See
the trap of that name. Bumping the pin is a deliberate commit of its own; the cost of
pinning is that new lints and diagnostics wait for it, which is the point.

`--all-targets` covers test code, `-D warnings` makes lints
fatal, and `--locked` catches a `Cargo.lock` that was not committed after a `cargo update`;
dropping any of them silently weakens the gate.

**`--locked` has to be on the *first* resolving command, and until 2026-08-05 it was not.**
clippy carried `--all-targets` alone, and clippy is the earliest cargo command in the list
that resolves dependencies --- so an edited `Cargo.toml` beside a stale committed
`Cargo.lock` had the lockfile rewritten to match by the gate directly above `cargo test
--locked`, and the lockfile gate then passed on a file that had just been corrected under
it. Both carry `--locked` now. The general shape, which is the same one the release-checklist
rule above is about: a gate is only as strong as the earliest command in the run that can
undo what it checks, whatever flags the later ones carry. `--bins` is there because **none of the
others links a binary** --- clippy stops at metadata and `cargo test` links each `[[bin]]`
with `main` replaced by the harness's own, so a symbol reachable only from `main` is dropped
as dead code. That gap let a 7/7 sweep sit beside a failing `npm run tauri build`.

One honest note. The earlier plan listed `npm run lint` and `npm run test`, neither of
which existed; adding an ESLint config and a test runner with nothing to lint or test is
scaffolding, and the rule was that they land when there is something for them to check.
`npm run test` (vitest) landed on 2026-07-27, when command ranking gave it something ---
front-end logic with an answer that can be wrong rather than merely ugly. `npm run lint`
still does not exist, for the same reason as before.

**There is CI for ordinary commits as of 2026-08-02, and a release workflow since 2026-07-31.**
The objection that delayed it was never cost --- it was that a workflow restating the gate
commands in YAML would be *a second place for the gate list to live*, and neither workflow does
that: both invoke `scripts/gates.py`. What changed materially is that the repository went public,
and macOS runner minutes bill at 10x against a private allowance and are free here. The stated
reason and the operative reason were different: "one machine" was a description of the
circumstances, not an argument.

It runs on `pull_request` rather than `pull_request_target`, asks for `contents: read`, and
**references no secret** --- see the fork threat model under Repository facts, and the header
comment in the file, which is the copy that has to stay right.

**So `gh run list --workflow=ci.yml --branch main --limit 1` is now the cheapest first thing to
do in a session, and it answers a question a handover cannot.** A handover is written before the
run it triggers finishes, so it is authoritative about the code and structurally stale about the
build; and on a two-platform repository the machine that files it is the one that cannot compile
half of what it moved. Establishing green first costs one command, and it converts every later
failure into a statement about your own change. Select the workflow rather than taking the newest
run, and read the job count beside the conclusion --- two jobs, not one.

What CI structurally cannot cover, and the reason `BUILD.md` still schedules them by hand:
`viewer_check.py` and `mutate_viewer.py` drive a real window and need an unlocked,
unoccluded screen, so on a headless runner they do not fail, **they hang** --- which is the
failure shape this repository is least able to read, since a hang and a pass both produce no
red. The mutation harnesses rebuild per mutation and take minutes.

`.github/workflows/release.yml` fires only on a CalVer tag and **invokes `scripts/gates.py`**
rather than re-listing commands in YAML. The one part with no precedent in the portfolio is
signing the bundled `libpdfium.dylib`: notarization requires every Mach-O in the bundle to carry
a Developer ID signature and the hardened runtime, so the dylib is signed in `vendor/` *before*
the bundler copies it. Its verification step is written to fail rather than warn --- a skipped
notarization exits 0 and produces an app Gatekeeper rejects. The tag glob matches an `-rcN`
suffix so a rehearsal is possible, and a failed run publishes nothing, since `release` needs
`gates` and the release is created as a **draft**. It took four rehearsal tags to get there, each
failing one step later than the last; `docs/RATIONALE.md` has the sequence and `BUILD.md`'s
checklist has the habit as step 10.

> ⚠ **Every Windows measurement below was taken from a process the harness gave a stderr to,
> and on 2026-08-19 that turned out to hide a defect that made the installed application
> unable to open any document at all --- by any route.** `viewer_check.py`, `open_check.py`
> and `session_check.py` all launch with `stderr=subprocess.PIPE`, because the transcript
> they read *is* the app's output; Python implements that with `STARTF_USESTDHANDLES`, so the
> app always had a valid stderr. A GUI-subsystem binary started by a person has none, and the
> worker spawn treated that as an error and refused. A terminal does not help --- measured, by
> the reporter, against the first explanation recorded here, which said it would.
>
> So the results below are true of the binary **and** of an instrument that supplies a
> precondition no user supplies. No automated check here can reach that case, because any
> harness that captures output has by that act created a stdout and a stderr. Nothing has
> been re-measured from Explorer. The trap index has the entry.

**Windows runs the viewer, and is contained.** A Windows build opens documents and passes
`viewer_check.py`, and the invariant is the check-name **set** rather than any total: name sets
diffed pairwise are byte-identical across corpora and across both platforms, with every
ran/skipped split matching `BUILD.md`'s table, which is where those numbers belong. A count
written into prose goes stale the next time a check is added.

This section said the opposite until 2026-07-30 --- "the platform is unsandboxed", "it fails
open" --- while the constraints section above had the corrected version the whole time, so a
reader who happened to start here would have concluded that hostile input is parsed in the app
process. **A document with two accounts of the same fact is worse than one with none**, and the
failure mode is that whichever section a reader reaches first wins.

Two things a green sweep still does not say, both learned the same day. `scripts/gates.py`
reported 7/7 while `npm run tauri build` failed, because nothing in the list linked a
binary --- there is a `bins` gate now, and it was proved to fail before being trusted. And a
`cargo build --release` binary is *not* a production build: the frontend is embedded by a
cargo **feature**, not by the profile. Both are in `docs/TRAPS.md`.

Every *measurement* in this file is macOS arm64 unless it says otherwise. The two
platforms differ enough --- on pre-spawn cost, on render constants --- that carrying a macOS
number over is a guess rather than an estimate, so a Windows figure is always labelled.

**The render constants are measured on both platforms.** `tile-bench` and `pool-bench` run
on Windows, and `docs/PLAN.md` §4's four architectural consequences reproduce there: the ratios
that drove the architecture hold, and every absolute number is **1.5--1.8x worse** than macOS,
so a latency budget written against the macOS figures is optimistic here by about a third.
`BUILD.md` has both tables and the caveats.

The account behind this section --- what was measured, what it cost, and which earlier sentence it corrected --- is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The gates, one at a time*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

---

## Known traps

Things already paid for once, or verified before writing code. Add to the list rather
than rediscovering.

**The entries themselves are in [`docs/TRAPS.md`](docs/TRAPS.md)**, under these exact titles.
Only the titles are here, because the full text was 93% of this file --- an instruction budget
spent on the several hundred traps that are not the one in front of you. What the index has to
preserve is knowing that a trap *exists*; the paragraphs matter once you are in that area.

So: **before working in any area named below, read its entry.** A title is a claim, not the
lesson --- several of them are the opposite of what they sound like, which is why they were
written down. Grep the title in `docs/TRAPS.md`.

New traps go in `docs/TRAPS.md` with a line added here, in the same commit. That rule has a gate
behind it: `traps` in `scripts/gates.py` diffs the two as **sets**, both ways, and fails on
either side having something the other lacks.

**A bullet is the entry's title and nothing else, and that half now has a gate too.** It was a
rule with nothing enforcing it, and the set diff is structurally blind to a bullet's tail: by
2026-08-31, **323** of 588 bullets carried a parenthetical gloss, 62,440 characters, and this
file went over the 150,000-character limit at which it stops being loaded whole. Every one of
those tails was audited against the entry it points at; exactly one carried a fact the entry did
not, and that fact was merged into the entry. So a parenthetical now needs a named exemption in
`ALLOWED_PARENTHETICAL`, which holds one title --- the one that is actively wrong about its own
subject. A gloss that restates the entry does not qualify: the warning that a title can mislead
is two paragraphs up, where it covers every entry at no cost per entry.

The same gate holds this file to a **130,000-character ceiling**, because the bullet rule bounds
what an entry may cost and not how many there are: titles average 76 characters and the corpus
went from 116 traps to 588 between 2026-07-29 and 2026-08-31, so the index floor climbs about
1.3 KB a day on its own.
When the ceiling fires, the fix is to move a section out to a file this one points at --- what
`docs/TRAPS.md` and `docs/RATIONALE.md` already are.

**Code comments and the other documents say "`AGENTS.md` records ..." in about a hundred places,
and those references are still good** --- they were written when the entries lived here, and they
were left alone rather than rewritten, because a hundred-file mechanical diff over prose carries
more risk than the one hop it saves. Read them as naming the trap index; the paragraph is in
`docs/TRAPS.md` under the title.

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
- `PdfiumLibraryBindingsAlreadyInitialized` — a helper that binds its own library works alone and fails in company
- A wash that reads as zero everywhere: PDFium's buffer is RGBA, not BGRA
- PDFium's render rotation composes with `/Rotate`, and wants the turned size
- A rotated page whose box it inherited comes back `width x width`
- PDFium accepting a file is not evidence the file is well formed
- An error message that names no cause is not vague, it is a wrong diagnosis
- A fallback is in the coordinate system of whoever wrote it
- Two handles to one cached page are aliases, and a reading taken after a change describes the change
- PDFium answers the same error for no password and for the wrong one


### PDFium: text, coordinates and outlines
- A byte scan cannot verify a document with a Type0 font
- The page break is whitespace, and concatenating two pages loses it
- A pattern over folded text has no lines, so `^` means the page
- `FPDFText_GetText` drops characters, so it cannot be indexed alongside boxes
- A page carries `/Rotate`, and PDFium answers in two coordinate systems at once
- PDFium lays a page out from its `/CropBox`, and everything else here read `/MediaBox`
- A line-grouping rule assumes an axis, and the axis is not always vertical
- Two rotation tables, disagreeing at every turn but zero
- PDFium's character order is not the page's line order
- A dense page of uniform lines cannot detect a y-flip
- A comma opens a line of its own, and every space on the line joins it
- A loop that re-attaches to the previous item drops a leading orphan
- A font can float a space's box clear of its own line, and overlap banding drops it
- An absolute epsilon refuses a page whose every glyph is that thin
- A paragraph is one mark and several text objects, and the gap between them belongs to neither
- `FPDFBookmark_GetDest` follows the bookmark's action without checking its type
- `FPDFDest_GetLocationInPage` answers only for `/XYZ`, so every other fit lands at the page top
- Two resolvers agreeing with themselves is not two resolvers agreeing
- `FPDFBookmark_GetDest` cannot tell a heading from a damaged link
- A differential that needs a manifest is a differential over one document
- A destination's offset belongs to the page it lands on, not the page it left
- An outline can be infinite, and PDFium says so in its own documentation
- PDFium cannot create digital signatures
- PDFium's signature enumeration does not walk the field tree, and ours does
- PDFium draws a comment's icon in its own colour, and the file is not wrong
- PDFium synthesises an appearance for `/Text` and not for `/Stamp`
- A form's text is on the page's text layer, and the page's object list cannot reach it
- A form drawn twice on one page is one reference in the object graph
- Removing the `Do` stops the page drawing the picture, and leaves every byte of it in the file
- A comment saying two rules are the same is not a check that they are
- Filtering the engine's answer is weaker than not showing it the pixels
- The gate reads a band of rows, so a region narrower than its line is judged with its neighbours

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
- A body's newlines live below the table that decodes it

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
- Where the parse runs is not observable from a unit test
- A Rust process absorbs the first SIGSEGV you send it
- A released id must leave a hole, because removing it renumbers the rest
- Forgetting a node in a linked list is not removing it from the list
- A resource whose only owner is on the other side of a boundary is leaked whenever that side forgets
- Two copies of a distinction drift, and a mutation of one survives
- Dropping the owner does not close a pipe something else has cloned
- A descriptor without `FD_CLOEXEC` leaks into every later child, and keeps it alive
- Two mechanisms with the same limit make one of them untestable
- FIFO dequeue is not FIFO completion
- A worker killed a moment ago still says it is running
- A pre-spawned worker outlived its parent, and the claim that it cannot is untested by design
- The cleanup after an fd shuffle can close what it just installed
- A per-page invalidation counter is not the same as a generation
- State keyed by a slot belongs to whatever moves into that slot
- `(deny file-write*)` does not deny a write through a descriptor you were handed
- A child cannot tell a descriptor it was handed from whatever is open at that number
- A refusal flattened to a string across a process boundary loses the action that answers it
- A MAP_SHARED document does not pin the file, so a truncation is a SIGBUS
- A rename over a mapped file succeeds, and the mapping goes on serving the file that is gone
- A pool that replaces a dead worker with the same bytes faults again, forever
- A diagnosis placed after a liveness check inherits that check's race
- A valid in-place rewrite is served silently, and a length check cannot see it
- The check that could not exist while one function did both halves
- A field documented as the caller's last look, and read by nobody
- A guard that looks a pathname up again is not a guard on the file you are writing
- One temporary name for every save, written with a call that truncates
- Writing a page's rotation "for completeness" flattens what a bounded walk could not read
- Two page numbers can be one page object, and the second turn composes on the first
- A page number is a position, and deleting a page renumbers every one after it
- Removing one of two page numbers that name one page cannot be done by removing objects
- Dropping a reference out of a destination array leaves a destination with no page
- Flattening a page tree loses what a page inherited from the node it hung under
- A permutation and a subset are the same document to every reader, and not the same file
- A quirk documented as harmless becomes a defect the day its precondition is wired
- The order a model inserts into is not the order its caller is looking at
- An id and a slot are both `number`, so a mark drawn on the last page vanished
- Moving a mark is a re-inking of it, and reusing the command beat adding one
- A password that unlocks the first worker unlocks nothing else
- Wrapping stdin in a `BufReader` eats the first request of the session
- One untyped reply carrier, and the two ways serde refuses to replace it


### The document model: saving, structure, signatures
- Redaction conflicts with incremental save --- and a full rewrite is not sufficient either
- Digital signatures constrain what may be edited at all
- Whether `/Annots` is an indirect array decides how large an annotation edit is
- Embedded fonts are subsetted
- `lopdf`'s object collection is quadratic, but the algorithm is not
- Removing a refusal removes it for every caller, including the one that never had a guard of its own
- `lopdf` silently drops encryption on save
- An incremental save is cheap on disk, not in memory --- and its cost is the parse
- An object a prior revision overwrote is reachable by no parser
- A signature blob is trimmed by trailing zero, and BER ends in zeros
- Asking for fewer pages made the walk reach more, because the bound was a property of taking all of them
- A decompression bomb costs QPDF CPU, not memory — and `lopdf` neither
- A shortcut can produce the right answer and lose the report
- An empty answer from a whole-document scan cannot say whether it looked
- A cited instance can be half right, and the wrong half is the one doing the work
- JSON refuses `NaN`, which is what made an unchecked `f32` look safe
- A mutation that survives every check because nothing reads the field
- A panel that lists a hidden comment must not let the page open it
- `/F` is a bit field, and the flag every real link sets is not the one you are testing
- One predicate answering three questions is right until a second kind makes them disagree
- Padding a rectangle to make one refusal legal disables the check that refusal was doing
- A byte grep cannot see inside an object stream, and it returns enough hits to look like it worked
- `lopdf::decrypt` removes the entry that says the document is encrypted
- The guard that could not fire, because the library removes the evidence first
- A field with no reachable `true`, guarded by a comment about the wrong call
- The same silent decryption, on the path whose output a reader keeps
- A mark's rectangle survives a quarter turn and everything drawn inside it does not
- Twelve tests for marks and not one of them turned a page
- The uncovered bytes are mostly the signature's own container, and reporting the total reads as an accusation
- A field added to a shared plan is read by one writer, and nothing says which
- A guard checked after the surgery is a true sentence about the wrong document
- A workflow step is the one source no local gate reads, and mine named a file that cannot exist
- An emptiness control written as a threshold is a measurement of the platform it was written on
- The operation that "cannot lose anything" was the one nobody guarded, and its own doc comment said why
- Two nested `Result`s because the outer one is the pool
- A cross-reference stream states the width of its own fields, and `lopdf` believes it
- The bound is on what the loader expands, not on what a caller asks for afterwards
- A guard about the arithmetic is not a guard on the trip count, and the loop between them was unbounded
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
- Turning on updater artifacts makes every build demand the signing key
- A status element that comes and goes rearranges the toolbar it sits beside
- A menu item is a global key claim, not a label
- A menu item's greying is a snapshot, so a guard that moves without an edit is stale for ever
- A one-shot tool armed from the palette says nothing, and the reader is not stuck but lost
- A page's own turn is not the view's, and a rectangle drawn by one was found by the other
- A size is learned once, so a page turned before it was seen keeps a transposed one
- A framework can abort your whole test binary, and 470 passing tests report nothing
- A synthetic right-click posted to the window server never reaches the web view
- A Control+click is the primary button, so a guard on the button number missed the commonest right-click on macOS
- A key handler is only as safe as the newest element inside it
- A label the platform writes is compared against a label we write by nothing
- The second copy of a gated list is the one that drifts, and only it
- A title that is a strict prefix of another ties, and registration order decides

### Rust and macOS
- A locked macOS session cannot be unlocked from a script, so it must be prevented
- `Instant` on Apple Silicon ticks at 41.67 ns, so "elapsed == 0" is reachable
- `evict_page` can dangle a live `RawPage`, and the borrow checker allows it
- A mechanical insert before a declaration can land between an attribute and its item
- A `Decode<'static>` bound is satisfiable by leaking, and nothing goes red
- `trim_text` trims each event, and a value with an entity in it arrives as several
- A stale binary answered for a source file that was never written
- A guard whose neighbour refuses the same input cannot be tested by it
- Putting a guard in front of a parser disarms the parser's own guard, and the test still passes
- macOS has no `setsid`, so a detached restart never starts
### Measuring: what a number can and cannot say
- A documented count that is one sample of a race makes an honest run look like a defect
- The harness prints the count so nobody has to derive it, and it was derived anyway
- Two counts from two commits are not a platform difference
- A baseline that skips the expensive step leaves its noise in the answer
- A difference is only a measurement when the operands make it one
- A clamped delta turned "the baseline moved" into "this cost nothing"
- The edit that moved a copy and reported it as removing one
- A difference assertion is satisfied by any difference, including the one the defect produces
- A probe reading one edge of a box cannot see a mutation that clips the other three
- A check on the sign of a noisy quantity fires only when the noise falls one way
- The append was 8.2x in the spike and 1.1x in the application, and the difference is a hash
- A round trip is a composition, so it is blind to a symmetric error
- A mean cannot test a claim about a minimum
- A guard that reads the whole file does not belong on the path a reader waits on
- A check that defers to a cheaper one it supersedes cannot be tested, and refuses what it should forgive
- A guard's last look should compare against the moment of the first look, not the moment of the open
- One refusal message, two moments, and it told the reader to do something they no longer could
- A poll for something to appear has no control, so six clean absences meant the walk was broken
- A synthetic click from System Events does not reach the web view, and the pointer was 120 points off
- A wait built on a program the machine does not have returns instantly, and every check after it reads as a pass
- Two runs failing different checks is variance; the same check twice is a defect
- A test that changes the working directory silences every other test that reads a relative path
- A refusal that names a fallback has to keep the fallback open, and this one closed it
- A message set before the operation that clears the message area is a message nobody sees
- A frame-rate pass means nothing without a coverage number beside it
- A rate whose sample size is also an input to the mechanism does not travel, and 40 regions a page is not a reader
- Interleaving controls for drift, not for what the last variant left behind
- Three similarity metrics in a row, each unable to see its own failure
- A timer that starts after the setup measures the wrong thing, and reports it
- `cargo test` is a debug build, and a debug number in a doc comment is a lie
- PDFKit reports an annotation's bounds rotated and renders the page unrotated
- Reading the code predicted four call sites, and there were eleven
- The delta was the wrong term, because the mapping was already absent from both numbers
- A multiplied mark's coverage is a reading about the page, not only about the mark
- Interleaving controls for drift between the arms, not for a machine that is slow for both
- Process RSS is a high-water mark, so two oversized seeds look exactly like a leak
### Writing a check that can fail
- Break the code on purpose, or the test suite is decoration
- There was no check on the overlay at all, and that is why a reader found the underline defect
- A feature can be inert in the application while three layers of tests pass
- A control that is easier than the check certifies nothing
- A bound enforced against an upper bound on the quantity is enforced against nothing, and the shortfall reads as the engine's fault
- A count of failures bucketed by a property is not evidence about that property, and the numerator alone reads as a finding
- Two marginals bound an overlap and cannot measure it, and the bound reads like a finding
- When the remedy is a constant, compute what it would take before you write it
- An intervention outranks a stratified observation, and a bucket four points wide did not break the tie
- Two variables a corpus cannot separate, because on an ordinary page one forces the other
- A sweep that pads with `resize` is not the change it stands in for, and the difference set the next increment
- An OCR engine's bounding box is a detection, not a measurement
- A property that holds by construction cannot test the thing it resembles
- Four assertions became unfalsifiable without being touched
- A fixture the library itself wrote cannot tell a passthrough from a rewrite
- An oracle more forgiving than the thing it stands in for cannot fail
- A writer and its own reader agree about a document that is wrong
- A reply parsed as the wrong shape reads as absence, and absence is the reassuring branch
- A canvas round trip cannot read back what a renderer produced
- A dependency that refuses your test input makes your own guard look redundant
- Two constants in different units, and the comment comparing them was false at every zoom a reader uses
- PDFKit synthesises an appearance for an annotation that has none
- A defect that switches off a check's precondition is not caught by that check
- An "already have it" cache needs an in-flight set, not just the cache
- A text comparison cannot see a property that is not about text
- A selector naming one element stops reading the page when the layer gains another
- A test whose precondition is already satisfied never runs
- A check that borrows a neighbour's precondition passes wherever the neighbour ran
- A catch-all arm that was right for two variants is wrong for the third
- A crash test that compiles away proves containment of a crash that never happened
- A test for an atomic write must plant the intermediate it is meant to prove
- A control can be contaminated by the phase that ran before it
- A check that derives its inputs from the thing it is testing cannot fail
- A closure and a direct read of the same variable disagreed, and it is unexplained
- A hit-test slack that rescues a small target hands the click to its neighbour
- The nib was tested where it was, not where it had been
- Recording a jump at the call sites is a rule; recording it inside the primitive is a mechanism
- A mirror of the DOM's focus goes stale, and Enter activates the row nobody is on
- A synchroniser is not a fix, and the entry above called the arrows fixed anyway
- The fourth copy carried the explanation and not the fix
- A page fitted to the element's own width is measured under the scrollbar
- Fit-width rescales every page when one of them becomes the widest
- A synthetic heading that does not reach the second column tests nothing
- Two tests naming their scratch directory the same string delete each other's
- Whatever a fixture is meant to discriminate, it needs two of
- A fixture where the right rule and the wrong rule agree cannot tell them apart
- `NSURL` hands a path back decomposed, and the fixture that shows it is not the ASCII one
- Reading a decision back out of the DOM makes the test double part of the logic
- The fake DOM supports what has been needed before, and nothing else
- A leak no behaviour can see needs an accounting observable, not a cleverer assertion
- The window reads the status and the tests read the viewer, so the copy between them is untested
- A bound stops discriminating when the behaviour around it changes, and its test keeps passing
- Four checks that say where the ink is, and none that says how long it is
- A check that measures along the axis it is policing shrinks its expectation with its measurement
- A bound no correct input can reach makes a check that cannot pass, and a manual-only harness is where that survives
- Two mechanisms for one rule, and it took two survivors to see it
- Two mechanisms, one outcome, and deleting either one would have been the defect
- A check no gate runs is a check nobody runs, and two commands shipped past it
- A harness printed `[FAIL]` and exited 0, under a criterion that names it
- A scanner over every tracked file scans its own exemption table, and a CI gate born red still ships
- A readings table outlived the code that produced it, and every document still agreed
- An accounting observable nobody reads is the same as not having one
- The same assumption, quiet in one mode and loud in its neighbour
- Borrowing the writer's own table to avoid drift made the check unable to fail
- Two readers of one file cannot catch the writer that moved it
- An outcome two mechanisms can produce cannot test either one
- A length bound cannot be tested by the verdict it produces
- A check nested inside a lookup for the thing under test disappears with it
- A lower bound on a wait is satisfied by any longer wait, including a broken one
- A check whose failure mode is a wait cannot fail
- A test whose failure is a hang reports a pass and a timeout in one breath
- A check that cannot run is not a check, and a locked screen is enough to stop one
- An unreachable guard is worth keeping if the type can carry it instead
- A fixture that aborts its parser cannot live in a directory something sweeps
- A risk and a gate both keyed on writing cannot see the path that only reads
- A guard the type system already makes unexpressible has no mutation to write
- A guard whose only reachable input is one the model forbids
- An Escape ordering that no reachable input can distinguish
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
- A class used with `instanceof` must not live in a module the tests mock wholesale
- A command deliberately left out of the window harness still has to be classified
- A refusal that carries a `NaN` is not equal to itself, and both sides print the same
- Testing a rule is not testing that the rule is used
- A margin above a destination lands on the previous page, and the tolerance that compensates for it can only reach within a page
- A guard asking how long the document is cannot answer how far the jump went
- A size-driven invalidation cannot see a half turn
- Every statement about a turned page is also true of a rotated view
- An exclusion keyed on a prefix grows on its own
- `instanceof` against a constructor the runner does not have throws, it does not answer no
- A page count cannot see a move, and every deletion check is built on the page count
- A duplicate key in an object literal is legal JavaScript, so the suite stayed green
- A tolerance around one value is satisfied by an estimate that replaced every value
- The natural place to press is the one place the defect has no effect
- A feature reached only through an optional callback is invisible to a harness that omits it
- Pressing a row navigates, and navigating scrolls the list out from under the drag
- A break recorded as a position in a list the callee does not own
- A caller that validates first cannot reach the guard beneath it
- A coverage figure over the union of several quads measures the line spacing
- A control refused by a different guard than the one it was written for
- A count taken from the input, asserted against the output --- red on a clean tree
- A control refused by a different guard than the one it was written for, again --- and the verdict was green
- A differential between two readers cannot tell you which mechanisms ran
- A denominator that is constant in one dimension cannot compare areas
- A band check can pass by two hundredths of a point, and a passing run does not say so
- A probe that writes one colour cannot measure a mark drawn in another
- A single-entry cache is evicted by the grid scan that was about to test it
- A cross-check that type-checks the other platform does not lint it
- A reading in fractions of a rectangle cannot test something that is a fixed size
- A count of the tabs cannot see that one of them is clipped out of the panel
- Two synthetic marks addressed by page land on top of each other on a one-page corpus
- A getter that answers from the rows it was handed cannot see a panel that drew one
- Two writers for one document, and the printer got the older one
- Removing the second copy is what made the differential unable to fail
- A differential's most important check was hard-coded to pass when both readers failed
- A test helper that builds its fixture with the encoder under test
- A mock's default return value decides whether a mutation fails or hangs
- A check reported `[OK]` with the reason it should have failed printed beside it
- A check read the palette's rendered rows, which are capped at 64
- A correction that changed the direction of a movement that was never happening
- Before widening a check to another language, ask whether that language admits the defect
- A bound written against its own constant cannot see the constant move
- A control token spanning a whole line is read back only when the engine returns the line in one piece
- A control chosen from the document's own text is worth nothing when the text layer does not say what the page draws
- A framework you link is mapped whether you call it or not, so an absent image cannot be the evidence
- Two strips butted together make the engine misread both, and the control is what pays
- The words are still in the file, and the redaction is correct
- An ordering asserted over a `HashMap` fails a third of the time, which reads as flake
- A widened type enumerates its readers, and is blind to the ones that already had a fallback
- A skip in a reply-driven queue stops the queue, and "nothing was requested" is what a stall looks like
- A cache keyed by the page of the file, and every caller holding the slot
- A record of what has been asked for, kept where it cannot see the answer being thrown away
- The status line showed the selection and the copy refused it, over a page with nothing on it
- One refusal, three callers, one string --- and the string named one of them
- A helper named after the local it replaces shadows it, and `expect(fn.length)` passes
- A gate that reads prose as code can pass for the wrong reason, and rewording a comment turns it red
- An unbounded report crossing a bounded pipe turns a bad file into a failed check
- A `NaN` in one field is not a test of the other, and the clause that looks redundant is the one it needs
### Harnesses: running checks and reading what they print
- A mutation harness needs the same control as the thing it is testing
- A timeout that discards the transcript recreates the failure it was added to diagnose
- Restoring a mutated file by *moving* a backup over it tests the mutated binary (the title names the wrong mechanism --- see the entry below it)
- Piping the gate runner through `tail` ate the exit code and the evidence, about fifteen times
- A harness that prints only at the end cannot say where it stopped
- A harness that prints as it goes writes nothing until it exits, under a redirect
- A `pgrep -f` wait loop is defeated by the command that checks on it
- A wait built on `pgrep -f` outlives the job, and every later check agrees with it
- A mutation harness that dies leaves the mutation in the tree
- A design that wiped the state on every ordinary run was useless in the workflow it was for
- The check aimed at the mutation is the one that raised, and a traceback names no check
- An earlier case emptied the store the later case reads, so neither lookup ever happened
- A filtered test run is only as good as the names, and mine excluded the check I wrote
- An over-removal control cannot be proved by a mutation that under-removes
- Running a repo's formatter over files it does not format
- A mutation aimed at deleted code is refused far too late to matter
- A refactor orphans mutations nobody can see, and the gate is what finds them
- A cross-check that counts names against a count of tests is wrong wherever two tests share a name
- A refactor moved three callers away, and the mutation kept its anchor and lost its meaning
- A mutation harness knows only the tests it was told to run
- A verification chained after a failed edit reports success for work that is not there
- A restored file with its original timestamp leaves the build serving the mutation
- Three mechanisms, no checks: measure what a commit's tests can actually see
- A verdict that reads a timeout as "no result" throws away the finding
- A mutation naming a test the harness cannot run reports SURVIVED
- A mutation that survives may be a variant, not a gap --- check before strengthening
- A mutation that survived, a comment that claimed a behaviour, and no test to add
- A check written because a mutation survived has to inherit that mutation's expectation
- A leaner data structure turned a wrong edit into a no-op
- A harness that prints stderr only on failure hides what a passing run said
- A wrapper's own verdicts are on the other stream, in the same shape as a check's
- A mutation aimed at a check that skips reports SURVIVED
- A timeout whose failure path has no timeout is not a bound
- A mutation caught by an access violation produces no test results at all
- An unguarded `invoke` for a command that is not registered ends the run, and the harness calls it SURVIVED
- A guard that also guarantees termination fails as a hang, not as a red test
- A comment claimed an ordering mattered, and the mutation that should have hurt did not
- Three ways to be wrong about whether your own build is still running
- `caffeinate <utility>` becomes a child of the utility, so a child count counts it
- Repeating a race inside one process re-runs the first round, not the race
- A precondition that names the cause still lets the symptom print
- A text-mode restore is not a byte restore, and the locale codec cannot even read the file
- A gate's static reason turned a crash into a wrong diagnosis, twice over
- A sweep that names one cause for a symptom several produce sends you to rebuild what is current
- A decoder told to replace what it cannot read does, and the result ships
- A harness that synthesises input must reset the input's own state machine
- The last page cannot reach the top of the viewport
- An expected error line beside a passing suite makes a green run unreadable
- A harness that cannot read a script skips, and blames the fixture
- A check name that is a prefix of another cannot be aimed at
- A check named by its position in a list is renamed by whatever is appended to that list
- A global text replace with a "one or more" assertion rewrote four unrelated checks
- A mutation aimed at code no fixture reaches survives, and the fix is not a new corpus
- A harness sliced a code-point index with `String.prototype.slice`
- A measured string transcribed off a terminal loses what the terminal does not draw
- A mutation aimed at one branch when the fixture only reaches the other
- A delivery counter cannot say WHICH delivery, and the guard was satisfied by the event it excluded
- A snapshot taken after the first mutation restores the mutation, and verifies itself clean
- A `|` in the data split my own mutation in half, and the run reported a pass
- Three near-copies of a command made an existing mutation's anchor ambiguous
- The mutation that proves a guard is the one that performs the write it prevents
- `--only "text: "` runs every `context:` mutation too
- A rewritten line leaves a mutation aimed at nothing, and only the harness says so
- A stream split done for the failing direction leaves the passing one where it was
- Two budgets for one run, and the one that was raised is not the one that decides
- A workflow copied from CI can lose a whole step, and then the release gate is the weaker one
- A parity check that compares steps is blind to the authority they run with
- A step that signs before anything imports the certificate fails with the masked secret as its error
- The verification step failed after everything it verifies had succeeded, because `mapfile` is bash 4
- A mirrored value read after "idle" is the previous operation's, and it flaked on a release artifact
- A PATCH that sets only the body clears the draft's tag, and publishing then attaches it to nothing
- A draft release is invisible, and the tag beside it says the work shipped
- A test that walks every prefix of a journal still could not see the snapshot rule
- Two tests sharing a name make a mutation harness's two counts disagree
- A mutation that inserts rather than moves runs the code twice, and the second run overwrites the first
- The sweep shelled out to `pkill`, which is not a program on Windows
- `subprocess.run(text=True)` decodes with the locale codec, and the multilingual corpus is the one that breaks it
- An escape sequence written into a mutation table through a shell never arrives as an escape
- An event without the modifier fields a matcher tests reads as no match at all
- A probe copied from its neighbour inherits a starting point that may not apply
- The gate guarding the anchors reads the file differently from the harness that uses them
- A mutation written on one platform names a test the other platform does not compile
- Adding a third drag made five existing mutations aim at nothing, or at two things
- A new test can make an existing mutation's anchor ambiguous, and the anchor never moved
- A new command turns the mutation harness's control red, one layer from where it reads
- A new kind that is a near-twin inherits a predicate written when it had no twin
- A test named for the population it covers is renamed by every kind you add
- A predicate named after the population it covers is renamed by every kind you add
- A mutation that ANDs with true has changed nothing, and SURVIVED is then correct
- A mechanical edit keyed on a field name hits every occurrence of that name
- An AppleScript loop over a property list iterates a reference, and every menu reads as empty
- A harness that edits source files pays for the editor watching them
- A harness that prints its first three failures reads exactly like one that prints all of them
- A harness's cost expired because the code grew, and nothing goes red about that
- A `tauri dev` watcher recompiles the crate you are gating, and rustc's own OOM reads as a failed test
- A harness written on a locked screen is a harness that has never run
- A documented cost measured warm is the wrong number for the run you are about to make
- A mutation block below the `__main__` guard is counted by the gate and run by nothing
- Narrowing a run made a shape the output parser had assumed away
- A capability nobody could use is invisible to every check, including the mutation harness
- An option whose value is optional swallows the next argument, and `vitest list --json` overwrote a test file
- A wrapper that exits zero on a run that ran nothing, while both its callers guard themselves
- A rule written for the job a review named stops at that job, and the job next door had the most to lose
- A module split renames the harness's filter without moving one anchor
- A security gate keyed on where a call is written decides where that call may move
- A mutation that leaks a file poisons the absolute-count check written to catch that leak
- A count over a directory the whole suite shares is not an observable while the suite runs in parallel
- A filtered `cargo test --exact` that matches nothing prints `test result: ok`
- A link flag that makes the build succeed and the binary unable to start
- Without a sanitizer, libFuzzer blames whichever input was current when its sampler fired
- Nine `cargo fuzz run` invocations queue on one build lock and print nothing
- A generator that works around the defect it found stops looking behind it
- A fuzz artifact that comes back clean is a hypothesis, and the control is to revert the fix
- Repeating a flag argparse did not declare repeatable proves what the last one says
### Windows and portability
- The gates had never run on the platform where they fail
- A document meant to cover both platforms was generated from platform-specific inputs
- A crate-root `#![cfg]` empties a `[[bin]]`, and cargo reports a missing `main`
- An uninhabited type carries its impossibility into every caller
- A `null` that means "inferred" is not a `null` that means "unknown"
- A directory that exists is not the library you need
- A wedged compile and a slow one look identical, and CPU time is the only thing that separates them
- A stale Windows resource artifact disables the cross-check and reads as a broken checkout
- The comment naming the grep that would have caught it, four times running
- A list of documented blockers can be wrong in the direction that looks thorough
- A gate list that never links a binary cannot see a link error
- A pin that nothing verifies is indistinguishable from no pin
- A toolchain pin can match on version and still be the wrong ABI
- A custom URI scheme is not spelled the same way on every platform
- One constant standing for two platform distinctions breaks the moment they diverge
- A release build is not a production build; a cargo *feature* decides that
- A test cannot see a change to a profile it does not run under
- A guard that degrades to a no-op off its platform stops being a guard
- Reaching for a constant *because* it is portable, and picking the one that is absent
- The same platform refusal, a result in one scenario and a failure in the next
- `CreateProcessAsUser` waives a privilege only for a token it still recognises
- A restricting SID stops the loader, and the code never runs
- One failing rung cannot say which ingredient failed
- A verdict that takes the last row that worked recommends the weakest one
- A refusal that exists because nobody wrote the code is not a guarantee
- The kernel refuses a writable mapping of a read-only file, on both platforms
- "Inherit nothing" cannot be spelled as an empty handle list
- A safe function taking a raw `HANDLE` has an unstated contract, and clippy says so
- `GetExitCodeProcess` reports 259 for a live process, and 259 is a legal exit code
- A bound documented as "not load-bearing", measured under the only load it will meet
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
- A GUI process has no stderr, and every Windows check launched the app from a shell
- A refusal the reader needs, reported on a channel that does not exist
- Three ways to look for a macOS recent-documents list, and all three say nothing is there
- Moving a binary out of the installer moves it out of the gate that links it
- The same trailing slash on the other platform, left there by a prediction that it was survivable
- A silent installer skips the file it cannot write, and exits 0
- `cargo fmt` was blamed for mangling a string, and it was innocent
- A Windows-only file is invisible to every gate on a Mac, and cargo can cross-check it
- An unused-import warning on one platform is not an unused import
- A gate that refuses on a precondition of running is red on every machine that is not running
- One unguarded call to an external program made eleven fixtures that need nothing unbuildable
- Two drafts under one tag, with the artifacts split, and the first cause I recorded was wrong
- `$?` read in the same word as a command substitution is the substitution's status
- A relative forward-slash path is not an executable, and `cwd` makes every other argument in the list work
- `git` reports forward slashes on every platform, so a path key built with `Path` matches nothing on Windows
- A guard that answers by refusing the whole run turns two blocked mutations into 178
- An `[INFO]` line guarded on a macOS-only reading cannot print on Windows, and the instruction was to read it
- A `[SKIP]` whose stated reason is true can be the check you most need
- A capability absent through a struct default has no defect to find
- A PDF with no NUL in its first 8000 bytes is text to git, and autocrlf shipped a damaged one inside the binary
- A platform gate widened in one of three copies, and the two left behind blamed the engine
- A test module whose every test is platform-gated makes its own `use super::*` an error on the other platform

### Fixtures
- The test fixtures are generated, not committed
- A fixture whose origin is zero makes an offset term unfalsifiable
- A stand-in glyph with a degenerate box measures the wrong rule
- A fixture's self-check forbade its own finding
- A square fixture cannot tell a rotation from an identity
- A bound in the code hides everything after it in the fixture
- A test pinned a random value out of a generated fixture, and both places it runs hid that
- A test whose oracle is the heuristic the code replaced fails once in 256 runs, on correct code
- A `-manifest.json` sidecar enrols a fixture in a check it never claimed
- A `/Text` annotation's rectangle is advisory, and PDFKit replaces it
- The second reader substitutes the same icon and centres it, where the first anchored it
- A rotated page makes a document mixed-size, and two checks assume it is not
- A new corpus has to satisfy the sample points every existing check hardcodes
- An empty transcript is what a *running* viewer check looks like
- A probe fixture swept as a corpus, against the file that already said not to
- Three crop-box mutations in one module and one in its twin, for code written twice
- A rule about names, enforced by the one harness that discovers it last
- The tool written to catch a missing check reported agreement about the wrong set
- A control that cannot discriminate is not a failure, and calling it one made a documented command red
- A guard written inline with an FFI call is reachable by nothing
- Two predicates decide whether a save removes anything, and neither mentions removal
- An empty warning line and no warning line are the same string
- The containment rule that makes a highlight readable makes a redaction dishonest
- An "already asked" set keyed by a slot is renumbered by the next deletion
- Four answers, because a review panel may not say "nothing here" when it means "not yet"
- A request still in flight is not re-issued, so a mid-flight invalidation looks broken
- A fixture no script writes gated ten guards, and the tests that skipped passed
- A test helper that reads through a parser that could not read
- A control that turns the page in the plan turns nothing the writer reads
- Two correct rules deciding every subject make each other unfalsifiable
- The guard the type checker asked for made the loop bound untestable

### Documents as controls
- A mitigation present and disclaimed is quieter than one claimed and absent
- A mitigation that moved half a path reads exactly like one that moved the path
- A checklist step nothing can perform, and a comment promising a mechanism that does not exist
- The plan said the words had to be extracted, and the model had never let them be lost
- A *Not done* note outlives the work that closes it, and it is the recommendation nobody re-checks
- The only document nobody re-reads is the one strangers read
- The half of a check that could only ever agree with its own guess
- A gate over claimed absences only catches the name the claim guessed
- A regex over the source could not see eleven of the seventy-seven commands, four of them the ones the check was written for
- A refusal a reader could answer, reported on a channel with no answer in it
- An insertion between a doc comment and its declaration orphans it, and TypeScript says nothing
- Two `///` runs with no blank line are one comment, and it documents the wrong item
- A comment defending a name can become an argument for the opposite name, with no word of it changing
- A comment defending a design, with every number in it stale
- A comment argued for an ordering the code did not have, and the file's own measurement had made it pointless
- A *Not done* note can describe a route with no reader in it
- Two open questions joined by a word, and the smaller one had no choice in it
- A module header that says "and nothing more", under a `use` block that says otherwise
- The obvious name for the new type was already taken, and the error count climbed instead of falling
- A document's spelling of an em dash is not a string's, and the comment above the line legitimises it
- A disclosed risk names the operation you had in mind, and the path it describes keeps acquiring callers
- Nothing in this process catches the defect step 5 exists for, and the obvious repair condemned a healthy file
- PDFKit's first call costs 39 seconds in a debug build and 51 milliseconds in release, and the file it gets looks guilty
- Three structural rules, three over-refusals on correct files, and the third would have been unreachable anyway
- A comment's stated reason was checkable and false, and the next feature copied it
- 486 lines of the viewer never run under vitest, and the fix is a seam rather than a harness
- A tripwire that promised to go red could not, because three carriers answer one needle
- A fixed point is invisible when the fixture is written in dependency order
- A refusal promised in the plan and never built emits nothing to grep for
- A paragraph that names its own failure mode reads as coverage, and the count went stale three more times
- A sweep that stays inside one drawing really is one undo, and four files said that meant every sweep was
- The unchecked clause of a two-sided decision was the one carrying the cost, and checking it inverted the answer
- The checker tolerated the thing the rule forbade, and the index grew until nothing loaded it
- A claim about somebody else's program has no gate here, and this one was false for months
- A handover telling a person what to expect is a second implementation, and mine was wrong
- "The one copy" acquired a second copy the day after the sentence, and its comment cited an import as the definition
- An allowlist for a tool that is not installed reads exactly like a control
- The decision was right, its cost basis had doubled, and the share was the half that had not moved
- A YAML comment binds to nothing, so an inserted step can steal the one below it

## Repository facts

- GitHub: `tstone-1/tpdf`, **public**, MIT (`LICENSE`).
- **Line endings are pinned by `.gitattributes`, not by anyone's `core.autocrlf`.**
  `* text=auto eol=lf`, plus `binary` for the image, font and PDF extensions. Added
  2026-08-26; before it, every blob in git was LF while a Windows working tree held 236
  files as CRLF and 52 as LF, and `src-tauri/src/warm.pdf` --- a tracked PDF that
  `include_bytes!` puts inside the shipped executable --- was converted on checkout and
  compiled in damaged. That entry in the trap index is worth reading before adding any
  mostly-ASCII binary format to the tree, because `eol=lf` alone would not have caught it.
  Do not set `core.autocrlf` per clone: the attributes override it, so a per-machine
  setting is both unnecessary and a thing only one machine would have.
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
