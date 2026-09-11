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
and failing `light.exe`. The two bodies now live in `src/probes/` under their own names, reached by
`#[path]`, which leaves module parentage and every `super::` in them unchanged.

**And it no longer ships the spikes.** Until 2026-07-31 the installer carried all 17 probe and
benchmark executables --- a sandbox prober and a hostile-document harness among them --- because
they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets now: cargo builds
and links them exactly as before, the `bins` gate keeps covering them through `--examples`, and
the bundler does not see them, so the MSI payload is three files --- listed in `BUILD.md`'s *Measured against the
shipped MSI* table, since a local build tree emits a fourth --- and about half the size it was. The invocations moved with them: `--example <name>`,
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

Visual signatures use a bounded RGBA raster (`signature.rs`, `signature.ts`) on
`MarkKind::Signature`; PNG/JPEG decoding stays in the webview. Pixels are shared by
`Arc` in the journal, limited to 512x256 pixels (including rotated equivalents)
per image and 4 MiB across retained marks. The worker writes a PDF Stamp appearance
with an RGB image and alpha soft mask. Placement stores inverse-rotated pixels in
the mark's original display space, so later page turns rotate the image with it.
`SignatureDialog` remembers pixels in localStorage only on explicit opt-in.
`tabs_check.py --phase signatures` checks the real application; the independent
PDFium and PDFKit pixel readers and fixture commands are in `BUILD.md`.
This creates visual marks only; it does not create certificate-based signatures.

AcroForm filling uses `forms.rs` inside the document worker, with shared field
answers in the edit journal and `Plan.forms`. Every save carrying answers takes
an explicit-appearance rewrite; ordinary save, copy, print and raster redaction
share that writer. Text fields, checkboxes, radio groups, dropdowns (including
editable choices), and single/multiple-selection lists are supported. XFA,
read-only, password, file-select, comb and rich-text controls are not editable.
Text uses Helvetica with the same supported character set as `textbox.rs`.
`FormLayer` commits and drains validation before a tab transition or save.
`tabs_check.py --phase forms` drives the application on disposable synthetic forms;
`form_pdfkit_check.swift` independently reads and renders the saved result.
When writing inherited fields, materialise `/FT` and `/Ff` on the terminal field:
PDFKit reads `/V` through the parent chain but did not render our fixture's text
when `/FT` existed only on its grandparent. The unit test pins this compatibility
requirement. See `BUILD.md` for the fixture-generation and check commands.

Choice answers use option indices in the journal, preserving distinct labels
that share an export value. The writer saves `/V`, `/I` and explicit appearances
together; editable custom text removes stale `/I`. Radio indices use widget
object order, so page moves cannot retarget a pending answer, and `/AS` is updated
on every sibling while preserving its authored artwork. `TPDF_CHOICE_FIXTURE`
and `TPDF_CHOICE_PROBE` on the mixed-form Rust test generate synthetic inputs
and saved output for `tabs_check.py --phase forms` and `choice_pdf_check.py`.
The latter uses the independent `pypdf` parser and must reject the unedited input.
PDFKit displays the first label for duplicate dropdown exports despite a saved
`/I` selecting the second. `TPDF_CHOICE_UNIQUE_PROBE` generates the distinct-export
control for `choice_pdfkit_check.swift`, which asserts the selected dropdown label
and radio states. PDFKit's thumbnail also omits list-selection shading; the
independent parser checks the saved list values, indices and shaded appearances.

Document tabs retain backend handles and edit journals in `src/lib/documenttabs.ts`.
Only the active tab mounts a Viewer and Sidebar; switching commits open note fields,
drains pending edits, and keeps the reading position, search scope and sidebar choice.
Save/reload replaces the handle in the same tab. File writes block tab transitions;
tab closure releases its handle, and window closure checks all tabs for unsaved work.
`scripts/tabs_check.py <binary> <fixture.pdf>` exercises the application on disposable
copies. On Windows, run an isolated build (`TAURI_CONFIG` with a distinct `identifier`)
when the installed app is running, since single-instance forwarding otherwise absorbs it.
The restart session still restores the most recent document, not the full tab list.

Windows orphaned test workers can hold the release executable open while CIM
returns no executable path for them. Restart Manager with that exact executable
registered as a resource identifies its holders. Never clean up by image name:
the installed application may be running alongside the test build.

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

The PDFium pin is `chromium/8044`, installed by `scripts/fetch_pdfium.py` and verified by
digest. Phase 0 measurements used `chromium/7881`; they remain historical evidence.
On a pin change, re-run the compatibility probes listed near the top of `BUILD.md`.

`pdfium-render` 0.9.4 hides its bindings accessor. `progressive::bind` and
`bind_library` retain both a safe wrapper and a second public raw binding table
for the same library; only the safe wrapper initializes PDFium. Raw-interface
callers must use this bridge, before applying containment. The tables live for
the process lifetime; this does not make PDFium calls safe to run concurrently.

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
and it reported **45/45 with 0 not applicable to this platform** on run 33626718480, which is
the current `main`. That supersedes the 34/34 recorded here from run 33501693368, and closes
the caveat that stood beside it --- the checks added since have now been through a Windows leg.
Read the count from the run rather than from this line: it is a fact about a build, and the
sentence it sits in has been stale once already.

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
the 337 MB scan. Since 26.9.2 it is one parse in fact as well as in cost:
`docgraph::DocumentGraph` holds it and six readers share it, and `DocumentGraph::parses` is
the observable that says so. `pdfium-render` also does not expose `/IRT` at all, so a reply arrives there
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
- **QPDF** was the candidate for the redaction rewrite path because it does hardened
  structural rewriting with garbage collection of unreachable objects. That garbage collection
  is now `sweep.rs`, so what remains for QPDF is object streams, as the stack table says.

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

On Windows the gate runner defaults `CARGO_BUILD_JOBS` to 2, respecting an explicit
override. Concurrent example builds exhausted commit memory with OS error 1455
and allocation aborts; use the same bound for local Cargo verification outside
the runner. This limits compilation concurrency, not the Rust test threads.

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
`cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`, which does not link
and so needs headers rather than a linker. **It costs 1 s warm and over ten minutes cold**, measured
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
- `traps` --- `docs/TRAPS.md`'s table of contents against its own `### ` entries, diffed as
  sets both ways, because a tally can be right while the index is three entries short. Since
  2026-08-31 it also holds a bullet to its title, since 2026-09-06 the thirteen group names
  here to the `## ` groups there, and throughout this file to a size ceiling: the diff cannot
  see a bullet's tail, and 323 of them took the file past the limit at which it stops being
  loaded at all.
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
  `ViewerOptions` is every optional `on*` callback the frontend declares; the script prints the count.
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
editing it is `scripts/mutate_rust.py`: dozens of mutation anchors name `src/save/marks.rs` rather than
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

**Reply shapes are checked too, since 2026-09-06**, by `src-tauri/src/replies.rs` --- which
writes a committed sample of each reply under `src-tauri/testdata/replies/` --- and
`src/lib/replyshapes.test.ts`, which holds the TypeScript mirrors against those bytes. What
it covers is the seventeen named `Ok` payloads, compared key set for key set at the top
level, so a field the mirror has lost and a field no mirror declares are both findings. What
it does not cover is the `Err` payloads, and the key sets of the shapes nested inside a
payload.

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
> and `session_check.py` all hand the app a stderr (`PIPE` or `capture_output`), because the transcript
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

**The traps and the index of them both live in [`docs/TRAPS.md`](docs/TRAPS.md).** The table
of contents at the top of that file names every entry by title, grouped by area; the entries
themselves follow, in the order they were written. Open that table of contents before working
in any area named below, and then read the entry --- **a title is a claim, not the lesson**.
Several of them are the opposite of what they sound like, which is why they were written down.

The thirteen groups, and when each is worth opening:

- **PDFium: rendering, mutation and page state** --- calling the render engine, or editing the
  objects on a page.
- **PDFium: text, coordinates and outlines** --- extracting text, converting between a page's
  coordinate systems, or resolving a destination.
- **Text matching, and scripts that are not English** --- search, case folding, and any
  document whose text is not plain ASCII.
- **The worker boundary, the sandbox and the pool** --- anything crossing into a worker
  process, or deciding what one is allowed to do.
- **The document model: saving, structure, signatures** --- writing a document: appends,
  rewrites, encryption, the page tree, annotations, signatures.
- **Tauri, the webview and startup** --- the shell, the window, the menu, and anything about
  cold start.
- **Rust and macOS** --- language and platform behaviour that surprised us, with no PDF in it.
- **Measuring: what a number can and cannot say** --- before quoting any benchmark, delta,
  rate or coverage figure, including one already written down.
- **Writing a check that can fail** --- adding or changing any test, control or assertion.
  The largest group, and the one most often the real answer.
- **Harnesses: running checks and reading what they print** --- running the mutation
  harnesses, the window checks or the gate runner, and reading what comes back.
- **Windows and portability** --- anything under `#[cfg(windows)]`, and anything a gate
  running on a Mac structurally cannot see.
- **Fixtures** --- generating or extending a corpus under `testdata/`.
- **Documents as controls** --- editing this file, `docs/PLAN.md`, `BUILD.md`, `CHANGELOG.md`
  or the threat model, where the prose is itself a control something else is checked against.

New traps go in `docs/TRAPS.md` in one commit: the entry under a `### ` heading, and its title
verbatim as a bullet under the matching `## ` group in that file's table of contents. That rule
has a gate behind it: `traps` in `scripts/gates.py` diffs the two as **sets**, both ways, and
fails on either side having something the other lacks. It also refuses a bullet that carries a
parenthetical gloss --- a bullet is the title and nothing else, unless the title is named in
the checker's `ALLOWED_PARENTHETICAL`, which holds one, the title that is actively wrong about
its own subject. A gloss that restates the entry does not qualify: the warning that a title can
mislead is three paragraphs up, where it covers every entry at no cost per entry. And it holds
the thirteen group names above against the `## ` groups over there, both ways, so a group
cannot be added on one side alone.

**The index moved out of this file on 2026-09-06, and the arithmetic is the whole reason.** It
had reached 639 bullets --- about 51 KB of a file that is loaded whole before every task, spent
on the several hundred traps that are not the one in front of you. `AGENTS.md` was 112,084
characters against the 130,000-character ceiling the same gate enforces, and the corpus had
been growing about 130 entries a week for three weeks, which put the ceiling two to three weeks
away. Nothing else on the table bought more than a fortnight. The index costs thirteen lines
here now and grows by nothing when a trap is added, and it sits in the file it describes, so
adding an entry and listing it are one edit in one place rather than two files that drift. The
ceiling stays, because the other sections grow too; when it fires again the fix is the same one
it was this time --- move a section out to a file this one points at, which is what
`docs/TRAPS.md` and `docs/RATIONALE.md` already are.

**Code comments and the other documents say "`AGENTS.md` records ..." in about a hundred
places, and those references are still good** --- they were written when the entries lived
here, and they were left alone rather than rewritten, because a hundred-file mechanical diff
over prose carries more risk than the one hop it saves. Read them as naming a trap entry; the
paragraph is in `docs/TRAPS.md`, findable through the table of contents at the top of it.

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
