# DETAIL.md — tpdf

Paragraphs moved verbatim out of `AGENTS.md` on 2026-09-24, when that file (58.9k characters)
and the global instruction files together were over the 140k session load budget. `AGENTS.md`
keeps a pointer where each block stood. Not auto-loaded; read the section before working in its
area. A reference elsewhere that says "`AGENTS.md` records ..." may mean a paragraph here.

## Every PDF is hostile input: both platforms, printing, packaging

**On both platforms since 2026-07-29.** macOS gets its boundary from `sandbox_init` SBPL,
which the child applies to itself after `exec`; Windows has no counterpart, so the *parent*
builds one — a low-integrity token inside a job object, applied while the child is still
suspended. `Backend::default_here()` selects workers on both, and a platform with neither
still falls back to in-process and records `render::UNSANDBOXED_MARK` with a `[WARN]`, so an
uncontained run stays distinguishable from a contained one. A mark rather than a refusal is
deliberate: refusing would make a platform useless rather than uncontained.

**The Windows evidence is external, which is the part that matters.** A milestone we record
says what our code believes it did. `scripts/win_modules.py` reads the app process's loaded
module list from *outside* it, through Toolhelp, while a document is open, and asserts
`pdfium.dll` is absent — with the module count printed beside it, so a failed enumeration
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
| restricting SID (`S-1-5-12`) | **no** | — | everything, including the loader |

So the answer is **low integrity plus a job object**: PDFium renders byte-identically under
it — the font-substitution risk that the macOS work already caught did *not* materialise —
while losing the authority to write anything or reach into the app process. A restricting SID
is the stronger rung and is not reachable directly: the child dies at `STATUS_DLL_NOT_FOUND`
before `main`, because the loader's own reads are denied. Reaching it needs Chromium's
initial-token / lockdown-token handover, which is a real piece of work rather than a flag.

One honest limit on that: low integrity **does not stop reads**, so a contained worker could
still read any file the user can — which is why the document and the output are handed over
as inherited handles rather than paths, the Windows analogue of the macOS `dup2`.

**Windows no longer fails open.** `Backend::default_here()` selects workers there, proved by
the external module check above rather than by the absence of our own warning.

Worth knowing rather than inferring, because it is a real asymmetry with macOS: the Windows
bound is on **committed** memory, which the kernel charges at `VirtualAlloc` time, so a
decompression bomb is refused *before* a byte of it exists. macOS bounds *resident* memory, so
its balloon has to write to every page it takes. That is why `Worker::footprint` returning
`None` on Windows is not the gap it looks like — there is a kernel bound there instead of a
poll, and it is now the measured kind. (Nothing in production reads `footprint` on either
platform; only `pool-bench` does.)

**Printing works on both platforms, and only the readback corresponds.** macOS refuses to
open a panel for a job PDFKit cannot read; Windows refuses for one `Windows.Data.Pdf` cannot
read. Both are the platform's own PDF stack, so both are independent of the `lopdf` that wrote
the job and the PDFium that drew what the reader saw — which is the property the whole print
subsystem is built on, and the same standard `docs/PLAN.md` §6 sets for a redaction.

The half that does **not** correspond is the printing itself, and it is not a shortcut. macOS
hands PDF bytes to `NSPrintOperation` and the OS paginates and prints them as vectors. Windows
has no in-box "print this PDF" API at any layer — not Win32, not WinRT — so every Windows PDF
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
narrower than the sentence sounds — no *our* PDFium, and the parser that is there is patched
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
benchmark executables — a sandbox prober and a hostile-document harness among them — because
they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets now: cargo builds
and links them exactly as before, the `bins` gate keeps covering them through `--examples`, and
the bundler does not see them, so the MSI payload is three files — listed in `BUILD.md`'s *Measured against the
shipped MSI* table, since a local build tree emits a fourth — and about half the size it was. The invocations moved with them: `--example <name>`,
and built artifacts sit in `target/release/examples/`. **That gate flag is load-bearing, and was
proved so rather than assumed** — without `--examples` the `bins` gate covers only the app, and
an undefined extern called from one example's `main` is what turns it red with `LNK2019`.

**Normal builds exclude the JavaScript test harness.** `src/lib/harness.ts` uses
compile-time guards around dynamic imports; `npm run build` removes them, while
`npm run build:checks` retains them for native UI checks. Use
`npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app`
on macOS, or replace `--bundles app` with `--no-bundle` on Windows, for a separate
application identifier with the checks enabled. This supersedes
the earlier decision to ship all harness code. The production modules are shared;
a check build is not byte-identical to the released artifact, so release smoke
tests must also exercise the normal bundle. `scripts/check_bundle_share.py` checks
all emitted JavaScript chunks and refuses any harness implementation in a normal
build; `--checks` instead requires every entry and bounds its size.

## Stack: dependencies beyond the table

Two crates carry the search.

`regex` (MIT OR Apache-2.0) reads a reader's pattern, and it was already in the tree
transitively through the toolchain, so declaring it added no package. `caseless` (MIT) does
Unicode case folding, which is what makes `strasse` find `Straße`: `char::to_lowercase` is
defined for *displaying* text and leaves a sharp s alone, and folding is the operation defined
for caseless *matching*. It brings `unicode-normalization` (MIT OR Apache-2.0) with
`tinyvec`/`tinyvec_macros` (permissive) — the only genuinely new packages either of them adds.

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
as it arrives: `src-tauri/src/ber.rs` — about 150 lines, no dependency at all — walks it
first and hands the parsers a definite-length value, because RFC 5652 requires DER and real
signers emit the indefinite form that `der` refuses outright.

**Six crates check signatures, added 2026-09-26, and none of them holds a key.** `rsa` 0.9
(PKCS#1 v1.5 and PSS), `p256` and `p384` 0.13 through `ecdsa` 0.16 (ECDSA over the two NIST
curves), `sha1` 0.10, and `sha2` 0.10 --- the package already in the tree through
`tauri-codegen`, declared as `sha2_10` because the direct `sha2` is 0.11. They are the
RustCrypto generation `cms`, `x509-cert`, `spki` and `der` already use, which is what lets the
signer's `SubjectPublicKeyInfo` reach them without a conversion. Twenty-nine packages between
them, every one `Apache-2.0 OR MIT` (`ff`, `group`, `num-bigint-dig` spell it with a slash),
bar `libm` and `spin` (MIT) and `zerocopy` (`BSD-2-Clause OR Apache-2.0 OR MIT`): swept with
`cargo metadata`, and the tree's only copyleft string is still `r-efi`'s. `rand` 0.8 comes in
through `rsa`, which uses it only to generate keys --- never called here.

What they read is the document's: the covered bytes, the signed attributes, the signature
value and the public key in the signer's certificate. **They run in the worker only**, from
`integrity.rs` through `docinfo::scan_from`, and `docs/THREAT-MODEL.md` §T6.8 records the
bounds --- the range validated before hashing, one gigabyte of hashing per document, RSA
moduli up to 8,192 bits. **`rsa` carries RUSTSEC-2023-0071**, the Marvin timing side channel
in private-key operations, with no fixed release. Verification is a public-key operation and
tpdf holds no private key, so `.cargo/audit.toml` accepts it with that reason, proved both
ways: `cargo audit --file src-tauri/Cargo.lock` exits 0 with the entry and 1 without. The
entry becomes wrong the day anything here signs with `rsa`; Phase 6 step 2 signs inside the
OS key store instead, which is part of why.

**`fax` (MIT, pdf-rs project) was added 2026-09-18 and brings exactly one package** — its
derive crate is behind a feature that is not enabled. It decodes the CCITT Group 4 stencil
masks of scanned pages inside the worker, as one more parser of attacker-chosen bytes;
every mode it reads consumes input bits, so its work is bounded by the encoded length,
which `images/stencil.rs` caps like any encoded stream. Both lockfiles carry it: the fuzz
package resolves the application by path.

**Three more crates arrived with the text editor, and two of them read attacker-chosen bytes.**
All three run in the document worker (`textedit.rs`'s header: "executed in the document
worker"), and none of them is reached before the stream holding its input has
been decoded under a stated bound.

- **`ttf-parser` (MIT OR Apache-2.0), added 2026-09-13** in `f1d5e5f`, reads the embedded
  TrueType (`/FontFile2`) and CFF (`/FontFile3`) programs of a document's fonts. The program is
  decoded with `filters::decode` under `MAX_CONTENT`, 1 MiB (`textedit/fonts.rs:1096`,
  `textedit/fonts/cff.rs:208`), from an encoded stream capped at twice that. Before a face is
  accepted, `rights_face` refuses font collections and every sfnt flavour but `00 01 00 00`
  (and Apple's `true` for a MacRoman or custom encoding), and refuses any face carrying `fvar`,
  `COLR`, `CBDT`, `sbix` or `SVG `. The `glyph-names` feature is enabled for CFF, whose
  glyphs are addressed by name (`textedit/fonts/cff.rs`). **RustSec declared it unmaintained
  on 2026-06-28** (RUSTSEC-2026-0192; the author's statement is harfbuzz/ttf-parser#217), with
  no patched version and `skrifa` named as the replacement. It is accepted, not ignored:
  `.cargo/audit.toml` lists it with that reason, so a *vulnerability* reported against it
  would still be a new advisory and a red `audit.yml` run. Moving to `skrifa` is the remedy
  if one is.
- **`zune-jpeg` and `zune-core` (MIT OR Apache-2.0 OR Zlib), added 2026-09-15** in `e2cbaca`,
  and already in the tree through the image stack. They decode the DCT images a text edit
  preserves, only to prove the JPEG is whole; no decoded pixel reaches the saved file
  (`textedit/images/jpeg.rs`). The bounds, in the order they apply: the page's shared
  `MAX_IMAGES` budget of 32 MiB of samples is charged before the decoder is reached
  (`textedit/images.rs`); `jpeg::framing` walks the marker structure itself and refuses input
  over 2 MiB or more than 64 scans; the decoder runs in strict mode with its maximum width and
  height set to the image dictionary's, so a header claiming a larger image is refused before
  any buffer is allocated; and the output buffer must be exactly `width × height × components`.
- **`subsetter` (MIT OR Apache-2.0), added 2026-09-17** in `5dabd27`, reads no document bytes.
  It subsets only the bundled Noto fallback programs (`textedit/fonts/fallback_subset.rs`),
  and its output is parsed back with `ttf-parser` and held to `MAX_CONTENT` like any other
  program.

What checks these crates against the advisory databases is `.github/workflows/audit.yml`, on
every push, every pull request and weekly, since 2026-09-26. Before that nothing did, which is
how the `ttf-parser` notice sat unread for three months.

Four plugins are linked. `tauri-plugin-dialog` (Apache-2.0 OR MIT) for the file-open and
file-save dialogs, which pulls `tauri-plugin-fs` (Apache-2.0 OR MIT) and `rfd` (MIT) — the
capability list in `src-tauri/capabilities/default.json` names `dialog:allow-open` and, since
2026-08-16, `dialog:allow-save`; that second one opens a panel and writes nothing, and what
actually writes is `save_copy` and, since 2026-08-19, `save_document`, whose authority `docs/THREAT-MODEL.md` §T6.1 states; on Windows only,
`tauri-plugin-single-instance` (Apache-2.0 OR MIT), which is what gives that platform the
document handover macOS gets from `RunEvent::Opened`; and `tauri-plugin-updater` (MIT OR
Apache-2.0), which is the largest single addition the tree has taken — **48 crates,
325 to 373**, because it brings a TLS stack (`rustls`) and archive extraction (`zip`, `tar`).
And, since 2026-09-21, `tauri-plugin-process` (Apache-2.0 OR MIT), which is the opposite
extreme — **one crate**, two commands over `AppHandle`, and no transitive of its own. It is
there because finishing an update on macOS needs a relaunch: the updater replaces the `.app`
on disk and leaves the process running the old code, so without it the reader has to quit and
reopen by hand. The capability list names `process:allow-restart` alone rather than
`process:default`, which would also hand the webview `exit`. All permissive, swept as below.

**That plugin is also the only network authority in the application, and it changed a property
that had held until 26.8.2: tpdf made no request at all.** It is spent narrowly — one check per
launch, issued after every spike and check entry point has returned, so every harness here still
runs offline; nothing downloads or installs without a click; and the payload's signature is
verified against a compiled-in public key before anything is unpacked, which is what keeps those
two new archive parsers from ever seeing attacker-chosen bytes. `docs/THREAT-MODEL.md` §T9 is the
worked-out version, residual risks included. Every dependency added has to be checked against the
licensing constraint above rather than assumed, because one copyleft crate anywhere in the tree
removes the option of making this repository public. The check is `cargo metadata` over the whole
tree, not a glance at the crate's own README.

## Versioning: the CHANGELOG heading

**That heading form is measured safe here, and it is not safe everywhere** — checked
2026-08-16, because the cross-repo notes flag tpdf as a repo where it had been assumed and never
verified. It is dangerous wherever the release tooling selects a CHANGELOG section by matching
the version heading: a prefix match accepts `## [1.1.2] - Unreleased` exactly as it accepts a
dated one, so a forgotten rename publishes a release whose notes say *Unreleased* with nothing
going red. `xlsxturbo` is such a repo and uses a bare `## [Unreleased]` for that reason.
`release.yml` here reads **nothing** from `CHANGELOG.md` — its `releaseBody` is a literal block
in the workflow — so no tag can pick up a heading of any shape. The cost is the opposite
failure and it is real: that body cannot go stale by tooling, only by nobody reading it, and it
shipped a **"Nothing here edits a document"** paragraph that a later release made false. Re-read
it on every release.

## Quality gates: the list, the Windows check, and each gate

Currently, in the order `--list` prints them: a toolchain-pin check, a PDFium pin check, a trap-index check, a
future-date check, a
workflow-parity check, a workflow-fixture check, a mutation-anchor check, a mutation-compiles check, a mutation-suite check, a
corpus-classification check, `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`,
a locked build of all fuzz targets, `cargo build --locked --bins --examples`, a webview-sink check, a viewer-wiring check, a
doc-comment check, a command-classification check, a file-writer check, `npm run check`,
`npm run test`, both frontend build profiles (leaving normal assets), a bundle-share check, and a third-party-notices check. Three of them are
ordered rather than merely present: `toolchain` runs **first**, because every result after it
is a statement about whichever compiler actually ran, and `notices` runs **last**, because it
reads the build's own sourcemaps to see which npm packages shipped — with `bundleshare`
between `build` and it, reading the same sourcemaps for a different question.

**Every one of them can be green on a Mac while the Windows tree does not compile**, and that is not
a hypothetical: it was true for sixteen commits until a rehearsal tag for `26.8.3` turned both
runner legs red on `examples/print_probe.rs`. A Mac compiler never parses a `#[cfg(windows)]`
line, so `print_win.rs`, the two Windows probes and the Windows halves of `worker*.rs` sit
outside everything the list covers. `scripts/check_windows.py` closes it —
`cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`, which does not link
and so needs headers rather than a linker. **It costs 1 s warm and over ten minutes cold**, measured
2026-09-02 on the same tree an hour apart; this said "about 8 s" flat, which is the warm figure
for the `check` alone and describes neither run anyone actually makes. The whole cost is
building every dependency for a second target, so the number you get is decided by whether
`target/x86_64-pc-windows-msvc/` already exists — and a fresh checkout, which is the case the
next sentence is about, always pays the cold one. Budget for it rather than being surprised by
it: a ten-minute timeout on what a document calls an eight-second check reads as a hang. **Deliberately not a gate**: it needs a 629 MB SDK splat a fresh
checkout does not have, and CI runs a real `windows-2025` runner, which is better evidence. Run
it before pushing anything that touches a Windows-only file, and before a tag; `BUILD.md` step
5 has the one-time setup and the reason the missing PDFium DLL reads as a broken checkout.
Its honest limit is that a type-check is not a test — a wrong *value* passes it.

**Each gate exists because something specific went wrong, and one line each is the index
rather than the account.** `docs/RATIONALE.md` has the full version of every one:

- `toolchain` — `RUSTUP_TOOLCHAIN` silently overrides `rust-toolchain.toml`, which is what a
  CI action installing its own toolchain may set.
- `pdfium` — the pin was checked against a digest the installer wrote, and the only fact it
  had about the tree was that *something* named `*pdfium*` existed.
- `traps` — `docs/TRAPS.md`'s table of contents against its own `### ` entries, diffed as
  sets both ways, because a tally can be right while the index is three entries short. Since
  2026-08-31 it also holds a bullet to its title, since 2026-09-06 the thirteen group names
  here to the `## ` groups there, and throughout this file to a size ceiling: the diff cannot
  see a bullet's tail, and 323 of them took the file past the limit at which it stops being
  loaded at all.
- `workflows` — `release.yml`'s `gates` job was copied from `ci.yml` and dropped a whole step,
  so the release gate was weaker than the gate it exists to satisfy. It also asserts what
  authority that job holds, which comparing steps was blind to.
- `fixturebytes` — deterministic signed-fixture conversion checks cover every final
  payload byte, reserved offsets and malformed padding, without generating random keys.
- `anchors` — every mutation's search string occurs exactly once in the file it names, the
  test it names exists, and that test can go red on this platform. A killed harness's leftover
  edit and a drifted anchor are both invisible in `git status`.
- `types` — and the other half, because an anchor that still matches is not a mutation that
  still works: the *replacement* has to still compile. Three that no longer did were found in
  one week, each by a harness run measured in tens of minutes. The only gate that writes to the
  working tree; 0.4 s cached, ~16 s after a Rust edit, 22 s for all 2,389 from an empty cache.
- `mutations` — every suite `vitest list --json` collects is either mutated or excluded with a
  reason, so a harness omission is caught in twelve seconds rather than after a control pass.
- `corpora` — every `testdata/*.pdf` is a window corpus with a stated purpose or an exclusion
  with a stated reason; the list used to live in whatever shell loop somebody typed.
- `sinks` — `docs/THREAT-MODEL.md` T8: no markup-parsing sink anywhere in the frontend, plus
  five rules closing the routes by which document text becomes a navigation or a script. The
  backend half is enforced by the type (`Target::Refused`), and the two halves cannot see each
  other — that seam is residual risk 7.
- `wiring` — `Viewer`'s optional callbacks against `App.svelte`'s object literal, both ways.
  The box shipped inert with three layers of tests green, because nothing looks at the literal
  that joins them.
- `docs` — a doc comment must be followed by code. Two `/** */` blocks in a row bind only the
  second, silently; the first scan found 31 orphans across twelve files. Since 2026-08-28 it
  also has a **Rust** arm, for the mirror failure: two `///` runs with no blank line between
  them are *one* comment, so nothing is lost and the whole thing documents the wrong item —
  three live instances, one of them introduced while fixing the other two.
- `wiring` also covers `ScrollerOptions` and `ThumbnailOptions` as of 2026-08-28, which with
  `ViewerOptions` is every optional `on*` callback the frontend declares; the script prints the count.
  `AppActions`' 51 members are deliberately **not** here: they are required, so `npm run check`
  refuses a missing one, and a gate over them would have no reachable subject.
- `classified` — every registered command is in the window harness's `probes` or its
  `undriven` table. That harness asserts it already; it needs a screen and is run by hand,
  so two commands shipped unclassified on 2026-08-29 and the check was red for a day with
  every gate green. This one reads the source text and buys the day, not the certainty.
- `writers` — every registered command that reaches one of the `save` module's terminal
  writers is named in `docs/THREAT-MODEL.md` §3's list, and the row's count agrees. That row is
  the one place answering *how many ways can the webview cause a write*, and it was wrong three
  times in two weeks, always under-claiming: it said six against a list of five when the answer
  was eight. The section's own rule — *the list is the claim and the number follows it* — had
  nobody applying it. Its control reads `save.rs` **and everything under `src/save/`**: it read
  the one file until 2026-09-01, when a split moved code out of it, and a control that keeps
  passing on a smaller file is the failure this gate exists to refuse.
- `dates` — no date in a tracked file may be later than today. Provenance here is written as
  dated measurements, and on 2026-08-28 there were **70** stamps reading a day or two ahead,
  every one written by a commit dated 2026-08-28. A stamp in the future does not merely
  mislead about one measurement; it makes every stamp written in the same sitting unreliable,
  and nothing else notices.
- `fuzz` — build every target in the separate fuzz package, whose lockfile and
  plan generator can drift independently of the application. `forms_scan` covers
  AcroForm traversal; rewrite inputs include form answers and signature rasters.
- `bundleshare` — require zero harness implementation across all normal-build
  chunks; `--checks` verifies the separate native-check build includes every entry.


**`save.rs` is a directory module since 2026-09-01**, and the reason to know that before
editing it is `scripts/mutate_rust.py`: dozens of mutation anchors name `src/save/marks.rs` rather than
`src/save.rs`, and both new files are submodules of `save` on purpose, so the harness's
`save::` filter still reaches their tests. The file went from 14,844 lines to 3,988 — 61% of it
was its own test module — with `save/tests.rs` and `save/marks.rs` beside it.
`docs/RATIONALE.md` *Splitting `save.rs`* has what was measured before anything moved; the rest
of the split by concern (the append path, the staging, the worker seam, the rewrite engine) is
not done and is a design question rather than a file operation.

## Quality gates: toolchain pin, flags, CI

The README is checked against the command registry by `src/lib/readme.test.ts` rather than by a
gate of its own, in both directions: a `<!-- not-built: id -->` bullet may name no registered
command, and every registered command is named in a `<!-- built: -->` marker or excluded in the
test's `UNLISTED` table with a reason. What it does not check is the prose beside the markers —
`BUILD.md`'s release checklist carries that half, and is a checklist rather than a check on
purpose.

**Reply shapes are checked too, since 2026-09-06**, by `src-tauri/src/replies.rs` — which
writes a committed sample of each reply under `src-tauri/testdata/replies/` — and
`src/lib/replyshapes.test.ts`, which holds the TypeScript mirrors against those bytes. What
it covers is the seventeen named `Ok` payloads, compared key set for key set at the top
level, so a field the mirror has lost and a field no mirror declares are both findings. What
it does not cover is the `Err` payloads, and the key sets of the shapes nested inside a
payload.

**The Rust toolchain is pinned in `rust-toolchain.toml`** as of 2026-08-02, and the pin is
enforced by `scripts/check_toolchain.py` rather than assumed. `RUSTUP_TOOLCHAIN` overrides
that file silently, which is exactly what a CI action installing its own toolchain may set,
so both workflows use `rustup show` instead of one — and the gate asserts the result. See
the trap of that name. Bumping the pin is a deliberate commit of its own; the cost of
pinning is that new lints and diagnostics wait for it, which is the point.

`--all-targets` covers test code, `-D warnings` makes lints
fatal, and `--locked` catches a `Cargo.lock` that was not committed after a `cargo update`;
dropping any of them silently weakens the gate.

**`--locked` has to be on the *first* resolving command, and until 2026-08-05 it was not.**
clippy carried `--all-targets` alone, and clippy is the earliest cargo command in the list
that resolves dependencies — so an edited `Cargo.toml` beside a stale committed
`Cargo.lock` had the lockfile rewritten to match by the gate directly above `cargo test
--locked`, and the lockfile gate then passed on a file that had just been corrected under
it. Both carry `--locked` now. The general shape, which is the same one the release-checklist
rule above is about: a gate is only as strong as the earliest command in the run that can
undo what it checks, whatever flags the later ones carry. `--bins` is there because **none of the
others links a binary** — clippy stops at metadata and `cargo test` links each `[[bin]]`
with `main` replaced by the harness's own, so a symbol reachable only from `main` is dropped
as dead code. That gap let a 7/7 sweep sit beside a failing `npm run tauri build`.

One honest note. The earlier plan listed `npm run lint` and `npm run test`, neither of
which existed; adding an ESLint config and a test runner with nothing to lint or test is
scaffolding, and the rule was that they land when there is something for them to check.
`npm run test` (vitest) landed on 2026-07-27, when command ranking gave it something —
front-end logic with an answer that can be wrong rather than merely ugly. `npm run lint`
still does not exist, for the same reason as before.

**There is CI for ordinary commits as of 2026-08-02, and a release workflow since 2026-07-31.**
The objection that delayed it was never cost — it was that a workflow restating the gate
commands in YAML would be *a second place for the gate list to live*, and neither workflow does
that: both invoke `scripts/gates.py`. What changed materially is that the repository went public,
and macOS runner minutes bill at 10x against a private allowance and are free here. The stated
reason and the operative reason were different: "one machine" was a description of the
circumstances, not an argument.

It runs on `pull_request` rather than `pull_request_target`, asks for `contents: read`, and
**references no secret** — see the fork threat model under Repository facts, and the header
comment in the file, which is the copy that has to stay right.

## Quality gates: release workflow and Windows measurements

`.github/workflows/release.yml` fires only on a CalVer tag and **invokes `scripts/gates.py`**
rather than re-listing commands in YAML. The one part with no precedent in the portfolio is
signing the bundled `libpdfium.dylib`: notarization requires every Mach-O in the bundle to carry
a Developer ID signature and the hardened runtime, so the dylib is signed in `vendor/` *before*
the bundler copies it. Its verification step is written to fail rather than warn — a skipped
notarization exits 0 and produces an app Gatekeeper rejects. The tag glob matches an `-rcN`
suffix so a rehearsal is possible, and a failed run publishes nothing, since `release` needs
`gates` and the release is created as a **draft**. Since 26.9.18 its `gates` job is skipped when
`ci.yml` has already passed both gate legs on the tagged SHA: the `proven` job asks the API, and
`draft` accepts a skipped `gates` only with that answer. Any other answer, an API error included,
runs the gates in full. It took four rehearsal tags to get there, each
failing one step later than the last; `docs/RATIONALE.md` has the sequence and `BUILD.md`'s
checklist has the habit as step 10.

> ⚠ **Every Windows measurement below was taken from a process the harness gave a stderr to,
> and on 2026-08-19 that turned out to hide a defect that made the installed application
> unable to open any document at all — by any route.** `viewer_check.py`, `open_check.py`
> and `session_check.py` all hand the app a stderr (`PIPE` or `capture_output`), because the transcript
> they read *is* the app's output; Python implements that with `STARTF_USESTDHANDLES`, so the
> app always had a valid stderr. A GUI-subsystem binary started by a person has none, and the
> worker spawn treated that as an error and refused. A terminal does not help — measured, by
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

This section said the opposite until 2026-07-30 — "the platform is unsandboxed", "it fails
open" — while the constraints section above had the corrected version the whole time, so a
reader who happened to start here would have concluded that hostile input is parsed in the app
process. **A document with two accounts of the same fact is worse than one with none**, and the
failure mode is that whichever section a reader reaches first wins.

Two things a green sweep still does not say, both learned the same day. `scripts/gates.py`
reported 7/7 while `npm run tauri build` failed, because nothing in the list linked a
binary — there is a `bins` gate now, and it was proved to fail before being trusted. And a
`cargo build --release` binary is *not* a production build: the frontend is embedded by a
cargo **feature**, not by the profile. Both are in `docs/TRAPS.md`.

## Quality gates: render constants on Windows

**The render constants are measured on both platforms.** `tile-bench` and `pool-bench` run
on Windows, and `docs/PLAN.md` §4's four architectural consequences reproduce there: the ratios
that drove the architecture hold, and every absolute number is **1.5--1.8x worse** than macOS,
so a latency budget written against the macOS figures is optimistic here by about a third.
`BUILD.md` has both tables and the caveats.

## Known traps: why the index left this file

**The index moved out of this file on 2026-09-06, and the arithmetic is the whole reason.** It
had reached 639 bullets — about 51 KB of a file that is loaded whole before every task, spent
on the several hundred traps that are not the one in front of you. `AGENTS.md` was 112,084
characters against the 130,000-character ceiling the same gate enforces, and the corpus had
been growing about 130 entries a week for three weeks, which put the ceiling two to three weeks
away. Nothing else on the table bought more than a fortnight. The index costs thirteen lines
here now and grows by nothing when a trap is added, and it sits in the file it describes, so
adding an entry and listing it are one edit in one place rather than two files that drift. The
ceiling stays, because the other sections grow too; when it fires again the fix is the same one
it was this time — move a section out to a file this one points at, which is what
`docs/TRAPS.md` and `docs/RATIONALE.md` already are.

**Code comments and the other documents say "`AGENTS.md` records ..." in about a hundred
places, and those references are still good** — they were written when the entries lived
here, and they were left alone rather than rewritten, because a hundred-file mechanical diff
over prose carries more risk than the one hop it saves. Read them as naming a trap entry; the
paragraph is in `docs/TRAPS.md`, findable through the table of contents at the top of it.
