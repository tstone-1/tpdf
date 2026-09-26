# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

Five things this file does *not* carry in full. The trap list lives in
[`docs/TRAPS.md`](docs/TRAPS.md) and is indexed by title below; the worked-out account behind
each rule — the measurements, what they cost, and which earlier sentence they corrected —
lives in [`docs/RATIONALE.md`](docs/RATIONALE.md), which the three long sections here point at.
What the text editor admits is [`docs/TEXTEDIT.md`](docs/TEXTEDIT.md), and signatures, forms,
tabs and the worker's writers and readers are [`docs/SUBSYSTEMS.md`](docs/SUBSYSTEMS.md); both
moved out of *Stack* on 2026-09-24, when this file and the global instructions together were
twice the load budget, and *Stack* indexes both. The long-form paragraphs of *Every PDF is
hostile input*, *Stack*, *Quality gates* and *Known traps* are in
[`docs/DETAIL.md`](docs/DETAIL.md), moved the same day for the same reason, with a pointer left
where each stood. None is auto-loaded, on purpose, and the
indexes exist so that the decision to read an entry is an informed one rather than a guess.
Code comments and the other documents say "`AGENTS.md` records ..." in about a hundred places;
those references were written when all of it lived here and are still good in one hop — read
them as naming whichever of these files carries the paragraph.

No count of the entries is written here. The authority is `grep -c '^### ' docs/TRAPS.md`, the
*titles* have a gate behind them (`traps` in `scripts/gates.py`, which diffs the two sets), and
a count in prose has none — which is the whole reason the gate compares sets rather than
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
is blind to the **C++ libraries compiled into libpdfium** — FreeType, ICU, libjpeg-turbo,
libpng, libtiff, Little CMS, OpenJPEG, zlib, Abseil, AGG, fast_float, simdutf, llvm-libc,
Dragonbox and HarfBuzz. Cargo enumerates the Rust dependency graph, leaving the engine's
C++ dependencies outside that inventory. A sweep complete over cargo and silent about everything else passes
exactly like one that covered everything, which is the consistency-versus-completeness trap
arriving in the licensing constraint the entire project rests on. The gate enumerates
`vendor/pdfium/licenses/` as a third population, so a new file appearing there is a finding.

Two GPL strings live in there and both are benign; they are allowlisted **by file and by
mechanism** in the script, never inferred, and an entry naming a file that has gone produces
a warning rather than silently excusing nothing. `icu.txt` covers ICU4C's autotools scripts
under the Autoconf exception; those build-time scripts are not compiled into libpdfium.
`llvm-libc.txt` is Apache-2.0 WITH LLVM-exception, whose GPLv2 clause *waives* Apache terms
rather than imposing GPL ones. All three of the gate's failure modes were proved by mutation
before it was trusted.

### Redaction must be genuine

Redaction removes content. It does not draw a black rectangle over it. Any implementation
that leaves the underlying bytes recoverable is a defect, not a limitation — see
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

**Both platforms are contained, and the detail is in [`docs/DETAIL.md`](docs/DETAIL.md)**
*Every PDF is hostile input*: macOS by `sandbox_init` SBPL, Windows by a low-integrity token
inside a job object, proved from outside the process by `scripts/win_modules.py`; why printing
maps a platform PDF parser into the app process; why `src/bin/` must contain only declared bin
sources; and why normal builds exclude the JavaScript test harness (`npm run build:checks` for
a check build).

Non-negotiable: parsing and rendering happen in **worker processes** with no filesystem or
network authority, under resource and time limits, restartable on crash. Document
JavaScript and launch actions are **disabled by default**. All `lopdf` stream decoding is
bounded. This is a Phase 0 concern, not a hardening pass to be done later — retrofitting
a process boundary is an architectural rewrite.

This constraint is load-bearing in a second way: because concurrent in-process PDFium calls
are undefined behaviour and crash in practice (see Known traps), worker processes are also
the *only* route to parallel rendering. Security and performance want the same
architecture.

`docs/THREAT-MODEL.md` is the worked-out version: what is being defended, the trust
boundaries, each threat against the evidence that it is handled, the sandbox profile in
full, and the residual risks in one list. Every claim there is either measured with the
spike named, or marked untested — keep it that way when adding to it.

The account behind this section — what was measured, what it cost, and which earlier sentence it corrected — is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The process boundary, rung by rung*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

---

The SignPath Foundation application submitted on 2026-09-12 was declined for
insufficient public adoption and independent recognition, not a technical finding.
Decision recorded 2026-09-16: continue development with unsigned Windows releases,
defer paid SignPath, and reconsider a Foundation application after broader adoption.
`BUILD.md`'s *Windows signing onboarding* retains the sample workflow, account
prerequisites and signing order for future use. The public policy is in `README.md`;
signing is not available until approval and a verified signing rehearsal.

## Stack

TypeScript 7 is installed as `@typescript/native` (an npm alias); it supplies `tsc`.
`typescript` aliases `@typescript/typescript6` because `svelte-check` 4.7.6 still
requires the TypeScript 5/6 compiler API. `npm run check` runs both the native
compiler and Svelte diagnostics. Remove the compatibility alias only when the
Svelte checker supports the new compiler API; a plain TypeScript 7 replacement
cannot supply the API that checker imports. See Microsoft's
[side-by-side migration guidance](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/).

**Existing-text editing is specified in [`docs/TEXTEDIT.md`](docs/TEXTEDIT.md)**; read it before
touching `src-tauri/src/textedit/`. Its topics, in order:

- Text matrices, page CTMs, content bounds, grouping, clips, Identity-H, `text-edit-probe --roundtrip`, text direction.
- Explicit layouts, bundled Noto fallback fonts, CJK subsetting, previews.
- Consecutive shows: cursor, line matrix, `TJ` compensation, `TD` leading.
- ActualText tab/bell spacers (`spacers.rs`) and other ActualText spans (`actual.rs`).
- Marked content inside text objects; signed and compound rectangular clips.
- Images: JPEG, CCITT G4 stencil masks, `/SMask`, `/Decode`, `/DecodeParms`, `/Metadata`.
- Preserved Form XObjects, their bounds, the 32 MiB image budget, empty glyphs.
- Tagged structure: RoleMap, block attributes, containers, lists, tables, producer shapes, pinned content.
- Render modes, layout attributes on figures and tables, WinAnsi TrueType, word spacing.
- Composite fonts and ligatures, refusal wording and the survey, Type3 fonts.
- Embedded Type 1 programs, word gaps in fonts without a space, kept kerning (`kerning.rs`).
- Non-embedded and standard-14 Latin fonts, the second producer sample, CID-keyed CFF.
- `cm` inside a text block, the 1e-6 ink allowance, untagged StructParents, patterns, skewed text.

**Signatures, forms, tabs and the worker's writers and readers are in
[`docs/SUBSYSTEMS.md`](docs/SUBSYSTEMS.md).** Its topics, in order:

- Visual signatures and their Keychain/DPAPI store; the digital-signature warning before any write.
- AcroForm filling (`forms.rs`) and choice answers by option index.
- Document tabs (`documenttabs.ts`), and two Windows worker-cleanup rules.
- The worker writes with `lopdf`: append, rewrite, copies, print, merge, and `verify::scan`.
- Password-protected documents: opening, holding the password, saving, every `lopdf` parse taking it.
- Comments, links and properties are read through `lopdf`; PDFium paints the marks.

The **shell is settled**, and since 2026-07-27 so is the **PDF layer** — Phase 0 proved
each provisional choice and the verdict is recorded per row (see `docs/PLAN.md` §9).

| Layer | Choice | Status |
|-------|--------|--------|
| Shell | Tauri 2 | Settled |
| Frontend | Svelte 5 (runes), TypeScript `strict: true`, Vite | Settled |
| Backend | Rust | Settled |
| Platforms | macOS + Windows | Settled |
| Rendering + text extraction | PDFium via [`pdfium-render`](https://docs.rs/pdfium-render) (BSD-3-Clause) | **Settled** — renders, extracts and sandboxes correctly; not usable for redaction (spikes 0.1, 0.3, 0.5) |
| Object graph + content streams | [`lopdf`](https://docs.rs/lopdf) (MIT) | **Settled** — surgical rewriting and sanitation both work, with our own mark-and-sweep and an encryption guard (spikes 0.3, 0.4, 0.6) |
| Hardened structural rewrite | [QPDF](https://qpdf.readthedocs.io/) (Apache-2.0) | Candidate — not required for the rewrite, and **no longer wanted for encryption either**: `lopdf`'s own `Document::encrypt` preserves it, measured against `qpdf` field for field (2026-08-28). Object streams remain |
| macOS print dialog | PDFKit + AppKit via [`objc2`](https://docs.rs/objc2) (Zlib OR Apache-2.0 OR MIT) | **Settled** — paginates and runs the panel; also the independent parser every print job is read back with |
| Windows print dialog | `Windows.Data.Pdf` + GDI via [`windows`](https://docs.rs/windows) (MIT OR Apache-2.0) | **Settled** — reads the job back, rasterises each page onto a printer DC, `PrintDlgW` for the panel. Raster where macOS is vector; see below |
| XMP metadata | [`quick-xml`](https://docs.rs/quick-xml) (MIT) | **Settled** — reads the catalog's `/Metadata` packet for conformance claims. Already in the tree through Tauri's `plist`, so it adds no package; namespace-aware, and expands no entity |
| Certificates in a signature | [`cms`](https://docs.rs/cms) + [`x509-cert`](https://docs.rs/x509-cert) + [`der`](https://docs.rs/der) (Apache-2.0 OR MIT) | **Settled** — reads the signer's certificate out of `/Contents`: subject, issuer, serial, validity. Parsing only; there is no trust store and no chain building. PDFium's read-only signature API is not a second implementation but *is* the differential, through `signature-probe` |

The PDFium pin is `pdfium-8066-tpdf.1`, installed by `scripts/fetch_pdfium.py` and
verified by digest. TPDF builds the unpatched 8066 source through
`.github/workflows/pdfium.yml`; archives carry source/toolchain provenance and
licensing notices. Our RTL report (PDFium issue 561066233) was fixed upstream by a
revert, so 8044's patch went; `scripts/pdfium_verify.py` pins the two known
mixed-direction limitations to upstream's exact output.
Only mac-arm64 and win-x64 are published. Engine releases are prereleases with
Latest disabled so they do not replace the application updater's release.
Phase 0 measurements used `chromium/7881`; they remain historical evidence.
On a pin change, re-run the compatibility probes listed near the top of `BUILD.md`.

`pdfium-render` 0.9.4 hides its bindings accessor. `progressive::bind` and
`bind_library` retain both a safe wrapper and a second public raw binding table
for the same library; only the safe wrapper initializes PDFium. Raw-interface
callers must use this bridge, before applying containment. The tables live for
the process lifetime; this does not make PDFium calls safe to run concurrently.

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI.

**Dependencies beyond the table** — the two search crates, the certificate and XMP readers, the signature checkers,
`fax`, and the four Tauri plugins — are listed with their licences and package costs in
[`docs/DETAIL.md`](docs/DETAIL.md) *Stack*. `tauri-plugin-updater` is the application's only
network authority (`docs/THREAT-MODEL.md` §T9). Check every new dependency against the
licensing constraint with `cargo metadata` over the whole tree, never from its README.

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

Apache PDFBox was evaluated and rejected — it is the best reference implementation for
forms and signing, but it is Java, and a JVM in a Tauri app defeats the entire premise.
It remains useful as a *behavioural oracle* to test against.

Pure-Rust renderers were considered and are not yet ready to be the primary engine.

The account behind this section — what was measured, what it cost, and which earlier sentence it corrected — is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The PDF layer: what each dependency cost to settle*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

---

## Versioning

**CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO starts at 0 and
increments per release within the month. Same scheme as `screenpick`.

Following `screenpick`, **four files must agree** on every version bump:

1. `package.json`
2. `package-lock.json` (top-level *and* the root package entry — `npm version <v> --no-git-tag-version` does both)
3. `src-tauri/Cargo.toml`
4. `src-tauri/tauri.conf.json`

Then run `cargo check` to refresh `Cargo.lock`.

Each release is a `Release vYY.M.MICRO: ...` commit. Unreleased work sits under
`## [YY.M.MICRO] - Unreleased` in `CHANGELOG.md`; the date replaces `Unreleased` only at
release time.

That heading form is safe here because `release.yml` reads nothing from `CHANGELOG.md`; its
`releaseBody` is a literal block in the workflow, so re-read it on every release
([`docs/DETAIL.md`](docs/DETAIL.md) *Versioning*).

---

## Quality gates

`scripts/gates.py` runs them all, and **is** the gate list rather than a description of
one. `BUILD.md` names that one command and deliberately does not repeat the commands
underneath it.

On Windows the gate runner defaults `CARGO_BUILD_JOBS` to 2, respecting an explicit
override. Concurrent example builds exhausted commit memory with OS error 1455
and allocation aborts; use the same bound for local Cargo verification outside
the runner. This limits compilation concurrency, not the Rust test threads.

The root `.cargo/config.toml` supplies the same macOS deployment target as
`bundle.macOS.minimumSystemVersion` in `src-tauri/tauri.conf.json`. Keep them
together; the toolchain gate checks agreement. An unset value in plain Cargo
versus Tauri's `10.13` invalidated `ring` and Objective-C dependency build scripts
on every switch between gates and application builds. Cargo reads configuration
from the invocation directory's ancestors, so run from this checkout, including
when using `--manifest-path`. Explicit environment overrides remain possible.

That is a deviation from the portfolio rule, which says a release checklist must state
every gating command verbatim with its flags. The rule exists because a hand-copied
command quietly loses a `--locked` or an `--all-targets` and then tests something weaker
than the real gate. Keeping the commands in exactly one executable place satisfies the
intent without the copy that has to be re-verified. Ask the script, not a document:

```
scripts/gates.py --list
```

**What each gate guards, and why it exists, is indexed in [`docs/DETAIL.md`](docs/DETAIL.md)**
*Quality gates*, together with the ordering (`toolchain` first, `notices` last), the `save.rs`
directory split, and the README and reply-shape checks. Every gate can be green on a Mac while
the Windows tree does not compile: run `scripts/check_windows.py` before pushing anything that
touches a Windows-only file and before a tag (warm 1 s, cold over ten minutes; `BUILD.md` step
5 has the setup).

**`App.svelte` is the layer no gate reaches, so state is born outside it rather than extracted
from it later.** Anything shaped like a walk, a set, a cache or a map — anything holding state
past the wiring and the markup — starts life as a `src/lib` module with its own unit tests; the
component keeps the object literals that join things and the markup. This is a rule and not a
taste, because three trap entries locate shipped defects at exactly this join and every
extraction so far happened after one: *An id and a slot are both `number`, so a mark drawn on the
last page vanished*, *An "already asked" set keyed by a slot is renumbered by the next deletion*,
and *A feature can be inert in the application while three layers of tests pass*, which is the
`wiring` gate's own founding defect. No test imports `App.svelte`; its net is that gate plus
harnesses that need a screen.

The toolchain pin (`rust-toolchain.toml`, enforced because `RUSTUP_TOOLCHAIN` overrides it),
why `--locked` must sit on the first resolving command, and how CI and `ci.yml`'s fork threat
model came about: [`docs/DETAIL.md`](docs/DETAIL.md) *Quality gates*.

**So `gh run list --workflow=ci.yml --branch main --limit 1` is now the cheapest first thing to
do in a session, and it answers a question a handover cannot.** A handover is written before the
run it triggers finishes, so it is authoritative about the code and structurally stale about the
build; and on a two-platform repository the machine that files it is the one that cannot compile
half of what it moved. Establishing green first costs one command, and it converts every later
failure into a statement about your own change. Select the workflow rather than taking the newest
run, and read the job count beside the conclusion — two jobs, not one.

What CI structurally cannot cover, and the reason `BUILD.md` still schedules them by hand:
`viewer_check.py` and `mutate_viewer.py` drive a real window and need an unlocked,
unoccluded screen, so on a headless runner they do not fail, **they hang** — which is the
failure shape this repository is least able to read, since a hang and a pass both produce no
red. The mutation harnesses rebuild per mutation and take minutes.

`release.yml` (tag-only, invokes `scripts/gates.py`, signs `libpdfium.dylib` before bundling,
creates a draft) and the Windows viewer measurements — including the warning that every harness
supplies a stderr no Explorer-launched app has — are in [`docs/DETAIL.md`](docs/DETAIL.md)
*Quality gates*.

Short fuzz runs must advance beyond corpus initialization; compare `INITED` and
`DONE` execution counts as described in `BUILD.md`, *Fuzzing*.

Every *measurement* in this file is macOS arm64 unless it says otherwise. The two
platforms differ enough — on pre-spawn cost, on render constants — that carrying a macOS
number over is a guess rather than an estimate, so a Windows figure is always labelled.

The account behind this section — what was measured, what it cost, and which earlier sentence it corrected — is [`docs/RATIONALE.md`](docs/RATIONALE.md) *The gates, one at a time*. That file is not auto-loaded, on the same reasoning as `docs/TRAPS.md`.

---

## Known traps

Things already paid for once, or verified before writing code. Add to the list rather
than rediscovering.

**The traps and the index of them both live in [`docs/TRAPS.md`](docs/TRAPS.md).** The table
of contents at the top of that file names every entry by title, grouped by area; the entries
themselves follow, in the order they were written. Open that table of contents before working
in any area named below, and then read the entry — **a title is a claim, not the lesson**.
Several of them are the opposite of what they sound like, which is why they were written down.

The thirteen groups, and when each is worth opening:

- **PDFium: rendering, mutation and page state** — calling the render engine, or editing the
  objects on a page.
- **PDFium: text, coordinates and outlines** — extracting text, converting between a page's
  coordinate systems, or resolving a destination.
- **Text matching, and scripts that are not English** — search, case folding, and any
  document whose text is not plain ASCII.
- **The worker boundary, the sandbox and the pool** — anything crossing into a worker
  process, or deciding what one is allowed to do.
- **The document model: saving, structure, signatures** — writing a document: appends,
  rewrites, encryption, the page tree, annotations, signatures.
- **Tauri, the webview and startup** — the shell, the window, the menu, and anything about
  cold start.
- **Rust and macOS** — language and platform behaviour that surprised us, with no PDF in it.
- **Measuring: what a number can and cannot say** — before quoting any benchmark, delta,
  rate or coverage figure, including one already written down.
- **Writing a check that can fail** — adding or changing any test, control or assertion.
  The largest group, and the one most often the real answer.
- **Harnesses: running checks and reading what they print** — running the mutation
  harnesses, the window checks or the gate runner, and reading what comes back.
- **Windows and portability** — anything under `#[cfg(windows)]`, and anything a gate
  running on a Mac structurally cannot see.
- **Fixtures** — generating or extending a corpus under `testdata/`.
- **Documents as controls** — editing this file, `docs/PLAN.md`, `BUILD.md`, `CHANGELOG.md`
  or the threat model, where the prose is itself a control something else is checked against.

New traps go in `docs/TRAPS.md` in one commit: the entry under a `### ` heading, and its title
verbatim as a bullet under the matching `## ` group in that file's table of contents. That rule
has a gate behind it: `traps` in `scripts/gates.py` diffs the two as **sets**, both ways, and
fails on either side having something the other lacks. It also refuses a bullet that carries a
parenthetical gloss — a bullet is the title and nothing else, unless the title is named in
the checker's `ALLOWED_PARENTHETICAL`, which holds one, the title that is actively wrong about
its own subject. A gloss that restates the entry does not qualify: the warning that a title can
mislead is three paragraphs up, where it covers every entry at no cost per entry. And it holds
the thirteen group names above against the `## ` groups over there, both ways, so a group
cannot be added on one side alone.

## Repository facts

- GitHub: `tstone-1/tpdf`, **public**, MIT (`LICENSE`).
- **Line endings are pinned by `.gitattributes`, not by anyone's `core.autocrlf`.**
  `* text=auto eol=lf`, plus `binary` for the image, font and PDF extensions. Added
  2026-08-26; before it, every blob in git was LF while a Windows working tree held 236
  files as CRLF and 52 as LF, and `src-tauri/src/warm.pdf` — a tracked PDF that
  `include_bytes!` puts inside the shipped executable — was converted on checkout and
  compiled in damaged. That entry in the trap index is worth reading before adding any
  mostly-ASCII binary format to the tree, because `eol=lf` alone would not have caught it.
  Do not set `core.autocrlf` per clone: the attributes override it, so a per-machine
  setting is both unnecessary and a thing only one machine would have.
- Public since 2026-08-02, and it needed no history scrub: all 108 commits across every
  ref were authored and committed as `48162401+tstone-1@users.noreply.github.com`, there
  were no tags, no `refs/pull/*`, no forks and no workflow run logs to become visible.
  That is the cheap case, and it held only because the clone was made with a repo-local
  identity — a fresh clone on the Windows flat layout has no `includeIf` rule and would
  silently commit under a work address. Set `user.email` / `user.name` repo-locally there.
- **The `APPLE_*` secrets survive the flip; a workflow that reads them must not.**
  Repository secrets are not exposed by making a repository public, but fork pull requests
  now exist. `release.yml` is tag-push-only and therefore unreachable from a fork; `ci.yml`
  references no secret, runs on `pull_request` rather than `pull_request_target`, and asks
  for `contents: read`. Keep that split — it is the whole of the fork threat model.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` — anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
