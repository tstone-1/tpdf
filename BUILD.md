# BUILD.md — tpdf

Native UI probes now require a checks build:
`npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app`
(macOS), or
`npm run tauri build -- --config src-tauri/tauri.checks.conf.json --no-bundle`
(Windows; the executable is in `src-tauri/target/release/`). Add `--debug` for a
development build in `src-tauri/target/debug/`. It has a separate application identifier;
normal `npm run tauri build` excludes the frontend harness. Existing probe commands
below refer to this checks executable when they drive the UI. Run
`python3 scripts/check_bundle_share.py --checks` on its frontend output; the normal
gate requires zero harness code and deliberately refuses a checks build.
`scripts/mutate_viewer.py` also builds this explicit checks profile and launches
its matching bundle; a normal build cannot supply its mutation observer.
The content-completion mutation removes both strict decoding and the later
complete token walk: removing only strict decoding is masked by the second
validator. Separate stream-patch mutations cover that writer's boundary checks.
The build
gate verifies both profiles and leaves normal assets ready for packaging. Smoke-test
the normal bundle separately before release.

**PDFium 8066-tpdf.1, unpatched (2026-09-25).** PDFium fixed our RTL report,
[issue 561066233](https://issues.chromium.org/issues/561066233), by reverting the change
that caused it: [158290](https://pdfium-review.googlesource.com/c/pdfium/+/158290) on
`main`, backported to `chromium/8059`. It also added an `Arabic` regression test built
from our sample. So `scripts/pdfium_rtl.patch` went, and the build compiles one unpatched
engine where it compiled a control and a candidate. Before switching, bblanchon's
unpatched 8066 was measured against our patched 8044. It matched on the nine ordinary RTL
fixtures in text, indices, character boxes and pixels, and passed search on both corpora,
multilingual 68/68 and encoding 23/23. Upstream's `ActualTextRtl` expectations are
identical in 8044 and 8066 for every `/ActualText` case. The two known limitations are
wrong the other way round: the patched build kept the Latin word in place and reversed the
Hebrew words, and upstream keeps the Hebrew phrase and moves the Latin word to the far end.
`scripts/pdfium_verify.py` pins that exact output, so a change in either direction fails
the build. [Run 36098721810](https://github.com/tstone-1/tpdf/actions/runs/36098721810)
at `2b9f2f0` built both platforms and passed 63 upstream text tests on macOS and 62 on
Windows. On the installed macOS engine these all passed:

- all 26 gates;
- the probes above: `remove-probe c`, both search corpora, the engine scan with no V8
  or XFA, and `progressive-probe` on vector-heavy and the form;
- the form round trip, with PDFKit reading it back;
- the signature pixels, 32/32 by pypdfium2;
- the notices cross-check against the Windows archive, byte-identical.

`signature_pdfkit_check.swift` reads the first signature's red as `[0.918, 0.2, 0.137]`
and fails, identically on 8044. The PDFs are byte-identical under both engines, so it is
not the engine. The check read `PDFPage.thumbnail` bytes, which are in the main display's
colour space: sRGB red in this MacBook's P3 profile. It now reads sRGB and passes 48/48; see
the trap of that name. Both published sidecars of 8044 and 8066 end in
CRLF for Windows, so `shasum -c` fails elsewhere; the digests themselves are right, and
`build_pdfium.py` writes LF from now on. The Windows window phases and Windows probes were
not run on this engine; CI's `windows-2025` gates are the Windows evidence until they are.

**26.9.8 pre-tag verification (2026-09-16).** All 25 quality gates passed on
macOS (775.3 seconds; 1,559 Rust tests, three explicit ignores, and 1,687 frontend
tests). Windows passed all 25 gates across the full run and focused reruns after
fixture setup and cross-platform fixture-output corrections. The Windows
cross-compiler check passed. Selected mutations caught all 390 Rust, 228 frontend
and nine native cases; all 29 gate-runner mutations were caught. Four initial
Rust survivors led to three stronger regression fixtures and one combined
mutation for two guards enforcing the same bound.

Both platforms passed 314 text-heavy viewer checks (51 not applicable). The
vector-heavy checks passed 219 on macOS and 216 on Windows, with 146 and 149
not applicable respectively. Eight text-edit workflows passed on each platform,
followed by independent parser and PDFKit readback. Saved structures
were retained and no pixels outside the edited area changed. Normal-bundle
rendering with the development engine hidden, the visible missing-engine
control, macOS menu/save checks, Windows MSI extraction and NSIS upgrade from
the published 26.9.7 installer passed. The packaged workers mapped PDFium; the
coordinator did not. Original installations, registry exports and sessions were
restored. Windows printing passed all 10 real-spooler checks.

**26.9.7 release verification (2026-09-14).** The PDFium compatibility blocker
is closed by the verified `pdfium-8044-tpdf.1` dependency release. The final source
snapshot passed all 24 gates on macOS and Windows. The macOS run took 590.8 seconds
and passed 1,410 Rust tests, with three explicit ignores. Both platforms passed
the text-heavy and vector-heavy viewer checks and all 15 tagged-browser text-edit
checks, followed by independent saved-PDF readback and 17 corruption controls.
The selected Rust mutations passed (293 plus two preview cases), as did all 218
selected frontend mutations and the Windows cross-compiler check. All 93 native
mutations also passed; that full run was over-scoped and is not a release requirement.

Normal macOS bundle rendering, menu/save checks, Windows MSI extraction and NSIS
upgrade checks passed with the development engine hidden. Workers mapped the
packaged engine and the coordinator did not; hiding both engines produced the
expected visible refusal. The Mac refusal appears in the interface, not its
console log; the original smoke script's log assertion was corrected after
reviewing the owned-window capture. Original installations and sessions were
preserved. Windows printing passed 10/10. Its OCR sweep sampled 11,728 regions
across 133 PDFs: 7,556 were read back, with zero still-readable text, 3,337 verified
unreadable and 4,219 unverified. Unverified regions are not clean verdicts.
The [application release](https://github.com/tstone-1/tpdf/releases/tag/v26.9.7)
is published. Both [CI jobs](https://github.com/tstone-1/tpdf/actions/runs/34824420628)
and all five [release jobs](https://github.com/tstone-1/tpdf/actions/runs/34824435182)
passed. All eight downloaded assets matched their published SHA-256 digests;
all three updater signatures verified against the application's public key.
The signed macOS app and disk image passed notarization, stapling and Gatekeeper
checks. The packaged engine passed multilingual search (68/68, seven explicitly
not applicable). Anonymous Latest, manifest and asset downloads were verified.
The real macOS updater installed 26.9.7 from 26.9.6; every installed bundle file
and symlink matched the verified release payload. Relaunch, bundled-engine PDF
rendering and the explicit latest-version check passed, with the original session
restored byte for byte. The Windows live updater check remains pending.

The original failure was in the unmodified multilingual search baseline: PDFium
8044 reversed Arabic word order and lost the mixed Arabic/Latin phrase. Upstream
change [152910](https://pdfium-review.googlesource.com/c/pdfium/+/152910) disabled
automatic whole-line reversal to preserve mixed English/Hebrew `/ActualText`.
Our failing fixture has no `/ActualText`; PDFKit independently reads its authored
word order. The expected text was kept unchanged. The report and reproducer are
[PDFium issue 561066233](https://issues.chromium.org/issues/561066233).

`scripts/pdfium_rtl.patch`, against PDFium
`f91ca5a72358bb0b00b4da9481b21fe668157614` (8044), preserves `/ActualText` behavior
and restores ordinary predominantly RTL lines when their first and last strong
segments are RTL. The complete 11-fixture comparison restores seven regressions
with unchanged pixels and character geometry; two pre-existing Latin/Hebrew
limitations remain unchanged. This is compatibility evidence, not general bidi
correctness. Removing each ActualText/first-strong/last-strong guard was shown
to break its specific control in the source experiment.

The [verified hosted build](https://github.com/tstone-1/tpdf/actions/runs/34815333188)
used repository commit `e756da9ba83802f9a8d1b638f605a40623c2b419`. Both platforms
passed the full differential. Control and candidate each passed 62 upstream
text tests on macOS and 61 on Windows, where upstream disables
`TextSearchLatinExtended`. The downloaded macOS library also passed multilingual
search (68/68), encoding search (23/23), progressive vector/form rendering,
character alignment and tagged reading order. The Windows library imports only
KERNEL32, ADVAPI32, GDI32 and USER32; it requires no separate MSVC runtime DLL.

The [dependency release](https://github.com/tstone-1/tpdf/releases/tag/pdfium-8044-tpdf.1)
holds both engine archives, SHA-256 sidecars and the complete verification evidence.
It is a prerelease with Latest disabled, leaving the application updater on the
normal release channel. `scripts/fetch_pdfium.py` downloads these exact archives
and verifies their hashes; do not replace a vendored library manually.

The build is defined by `.github/workflows/pdfium.yml` and `scripts/build_pdfium.py`,
with source/tool revisions in `scripts/pdfium_build.json`. It builds native
mac-arm64 and win-x64 controls and candidates before emitting any archive.
The supplier's license collector omits Dragonbox and HarfBuzz; the wrapper adds
their permissive notices from the pinned sources, includes TPDF's patch licence,
and refuses any other unknown library. Windows selects Git Bash explicitly and
checks it before compiling: PATH can otherwise select the WSL launcher.
Run its safeguards with `python3 -m unittest discover -s scripts -p test_pdfium_build.py`.
Building locally needs full Xcode or VS with the pinned Windows SDK; use
`python3 scripts/build_pdfium.py --help` for the disposable-directory invocation.
Source/dependency revisions are pinned. Host SDK/CRT versions are recorded rather
than hermetically supplied; no byte-identical compiler-output claim is made.
A workflow artifact alone neither publishes a dependency release nor changes the pin.

Plain Cargo and Tauri builds share the macOS deployment-target default in
`.cargo/config.toml` and `src-tauri/tauri.conf.json`; the toolchain gate checks
that they agree. Run Cargo from within this checkout so it finds the root
configuration. A shell override still takes precedence and can invalidate the
cache. The value preserves Tauri's existing `10.13` setting; it is not evidence
that the application or bundled PDFium works on that OS version.

Protected signature storage can be exercised with synthetic maximum-size pixels:
`cargo test --manifest-path src-tauri/Cargo.toml --lib native_store_roundtrips_maximum_pixels_and_forgets -- --ignored --nocapture`.
It creates and removes its own OS storage entry; it needs an unlocked desktop.
The regular gates compile it but do not contact Keychain/DPAPI.
Run `uv run scripts/signed_save_check.py <checks-executable> testdata/incr-certified-2.pdf`
to exercise warning text, initial Cancel focus, cancellation and accepted saving.
The driver verifies unchanged file bytes after cancellation and changed bytes
after acceptance; the application drives its own modal without Accessibility.



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
| `qpdf` | Not needed to build or run tpdf, and **required** for the hostile corpus — `testdata/make_hostile_pdf.py` shells out to it, so without it there is no `hostile-manifest.json` and `sanitize-rewrite` cannot start. Also the structural oracle for spike 0.4. On Windows the winget package needs elevation; the release's `msvc64.zip` unpacks anywhere and needs none. |

---

## Clean clone

```
npm install
scripts/fetch_pdfium.py
```

`vendor/pdfium/` is gitignored — a 7.7 MB binary does not belong in the object store — so
**a fresh clone has no PDFium and every binary fails to bind at runtime until the fetch
script has run.** The script downloads the pinned source-built archive, verifies its SHA256
before extracting anything, and refuses a V8 asset.

Verify an existing install without touching the network:

```
scripts/fetch_pdfium.py --check
```

The pin is `pdfium-8066-tpdf.1`; Phase 0 measurements in `AGENTS.md` and `docs/PLAN.md`
used `chromium/7881`. Bumping it means editing `TAG` and the whole `PINS`
table in `scripts/fetch_pdfium.py` together, then re-running the checks that a digest
cannot stand in for:

Run these from the repository root, after generating the fixtures below. Each exits
non-zero on failure.

With pdfium-render 0.9.4, removal cases `a` and `c` both passed on Windows on
2026-09-10, against both 7881 and 8044. That does not establish whether the
earlier macOS ownership crash is fixed; retain the spike's workaround until
the destroy case is also rerun there.

```
# The FPDFPageObj_Destroy ownership segfault. Case `c` (leak) must pass; if case
# `a` (destroy) ever stops crashing, the upstream bug is fixed.
cargo run --release --manifest-path src-tauri/Cargo.toml --example remove-probe -- \
    testdata/text-truetype.pdf c

# Extraction order and search must agree with the independently authored text.
# The 7881 -> 8044 update changed Arabic word order; symbol and pixel probes
# cannot see that regression. Generate both fixtures before running these.
cargo run --release --manifest-path src-tauri/Cargo.toml --example search-probe -- \
    --file testdata/multilingual.pdf
cargo run --release --manifest-path src-tauri/Cargo.toml --example search-probe -- \
    --file testdata/encodings.pdf

# The V8 and XFA symbol scan. This mode reads the library rather than binding it,
# so --lib is required even though every other mode defaults it -- and the directory
# is platform-shaped: lib/ on macOS, bin/ on Windows, where the loadable DLL lives.
# macOS is the only platform where this can currently answer: the Windows DLL is
# stripped of local symbols and the check correctly reports [NOT VERIFIED]. Run both;
# the Windows one still reports the export surface, which stripping cannot hide.
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    testdata/text-heavy.pdf --mode engine --lib vendor/pdfium/lib   # macOS
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    --mode engine --lib vendor/pdfium/bin                           # Windows

# Progressive rendering still agrees with the safe path, byte for byte. Slow:
# roughly 20 s, because the point is the page that takes seconds to render.
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/vector-heavy.pdf --mode identity --slices 0

# Form widgets take a second PDFium pass after the progressive render. The
# fixture has a value and deliberately has no stored appearance stream, so
# omitting FPDF_FFLDraw changes 4,587 bytes rather than passing by construction.
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/form.pdf --mode identity --slices 0

# AcroForm editing: synthetic shared fields, saved appearances and the real UI.
# Use a scratch directory; the application check edits only its own copies.
TPDF_FORM_FIXTURE=/tmp/tpdf-form-fixture.pdf TPDF_FORM_PROBE=/tmp/tpdf-filled-form.pdf \
    cargo test --locked --manifest-path src-tauri/Cargo.toml --lib forms::tests::forms_round_trip_values_and_every_shared_widget_appearance
uv run scripts/tabs_check.py <checks-binary> /tmp/tpdf-form-fixture.pdf --phase forms
# macOS independent reader; the optional directory receives page PNGs.
swift scripts/form_pdfkit_check.swift /tmp/tpdf-filled-form.pdf /tmp/tpdf-form-render

# Visual signatures: synthetic colour/alpha quadrants on cropped, rotated pages.
TPDF_SIGNATURE_PROBE=/tmp/tpdf-signatures \
    cargo test --locked --manifest-path src-tauri/Cargo.toml --lib signature_pixels_alpha_and_placement
uv run --with pypdfium2 --with pillow scripts/signature_pdf_check.py /tmp/tpdf-signatures
swift scripts/signature_pdfkit_check.swift /tmp/tpdf-signatures
uv run scripts/tabs_check.py <checks-binary> /tmp/tpdf-signatures/signature-source-0.pdf --phase signatures
# On Windows, set TPDF_SIGNATURE_PROBE with $env:TPDF_SIGNATURE_PROBE and use a local scratch path.
# Use an isolated TAURI_CONFIG identifier if another instance of tpdf is running.

# Pages inserted from another file (`edit.insertPages`, past its dialog): the palette's
# page question dismissed (nothing placed, the file released), answered `2-N` (exactly
# those pages, in order), then left blank for every page --- drawn from the other file's
# own handle, its text read and searched there, one undo and one redo, then saved into
# the disposable copy. The other file needs three pages whose text differs from each
# other and from the opened file's, or the checks on *which* page answered cannot fail;
# links.pdf has eight. The roles were the other way round until the range question.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase import --other testdata/links.pdf

# The sentence a reader is shown after a redaction, read off the message area
# rather than out of the reply that produced it. `verify.rs` and `redact.rs`
# assert the report, and `redact_import_check.py` asserts it across the worker;
# between them and the screen sit `recovery.ts`'s `afterRedaction` and
# `App.svelte`, which no gate reaches. Three passes over three disposable
# copies, each marking two regions on page 1 and confirming at the warning:
# a word still on the page that was marked, one surviving only on a page that
# was not, and one in a form object both pages draw, which has no page at all.
# The fourth word is marked in every pass, occurs nowhere else, and must be
# named nowhere in any verdict -- the control that says the other three are
# about the fixture rather than about a scan that reports whatever it is given.
# `file.redactCopy` is the sibling command and cannot be driven: it opens a
# native save panel. Both report through the same sentence.
# `<checks-binary>`, not an ordinary build: every tabs_check phase is reached
# through `src/lib/harness.ts`, which a normal build strips -- so a release
# binary launches, ignores TPDF_OPENCHECK and is killed by the timeout, which
# reads as a hang rather than as the wrong binary.
python3 testdata/make_redact_pages_pdf.py
uv run scripts/tabs_check.py <checks-binary> testdata/redact-pages.pdf --phase redact-pages

# Character boxes still land on the ink they describe. Run it on a *small* text
# fixture: on testdata/text-heavy.pdf the wrong convention also scores 70%, so
# that page cannot discriminate and the probe fails rather than reporting a pass.
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-probe -- \
    testdata/text-marked.pdf --mode align

# Reading order taken from a document's own tags rather than from geometry. The
# assertion that carries the weight is not that an order came back but that it is
# the *tagged* one: page 1's margin note reads third by geometry and last by the
# tags, and the manifest states both. Page 2 is the control, tagged in the order
# geometry would infer anyway, and text-base14 is the other control -- an untagged
# page must report no runs rather than an order it inferred.
cargo run --release --example structure-probe -- \
    --library ../vendor/pdfium/lib --file ../testdata/tagged.pdf \
    --untagged ../testdata/text-base14.pdf

# The outline walk terminates, resolves and refuses. Run BOTH: the hostile
# fixture proves the bounds fire, and the ordinary one proves they do not fire
# when they should not, which is the half that catches a walk bounding
# everything.
cargo run --release --manifest-path src-tauri/Cargo.toml --example outline-probe -- \
    testdata/outline-simple.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example outline-probe -- \
    testdata/outline-hostile.pdf --mode check

# The comment scan reads what a reviewer wrote and refuses what it cannot. Run
# ALL THREE: the corpus proves the bodies, encodings, dates, replies and bounds
# (26/26); the rotated one-pager proves rectangles come back in display space
# (5/5, one skip); and the `clean` control on a document with no annotations
# proves the scan is not simply returning nothing for everything -- without which
# every "the hostile page was cut short" assertion passes on a scan that found
# nothing anywhere.
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/comments.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/comments-rotated.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/text-base14.pdf --mode clean

# Links: the rectangles a reader clicks. Run ALL FOUR, and `agree` is the one to
# read. It compares the two destination resolvers tpdf has -- `outline.rs` through
# PDFium, `links.rs` through lopdf -- on a fixture whose outline points at the same
# places its links do. That mode found a defect on its first run
# (`FPDFDest_GetLocationInPage` answers only for /XYZ, so every /FitH outline
# entry had been landing at the top of its page since outline.rs was written) and
# it is the only check here that can fail for a reason neither module's own tests
# can reach. `clean` is the control: without it, every "the hidden link is not
# listed" assertion passes on a scan that found nothing anywhere.
#   links.pdf --mode check   27/27
#   links.pdf --mode agree    9/9   (6 shared destinations, its control, and the
#                                    manifest-free outline differential)
#   links-rotated --mode check 7/7, 2 skipped
#   text-base14 --mode clean  2/2
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links.pdf --mode agree
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links-rotated.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/text-base14.pdf --mode clean

# A locked document: can a reader actually open one?
# Everything that decides the answer is on the other side of a process boundary
# -- the load in worker_child::serve, the retry in worker_child::unlock, the
# password on the worker's stdin, and the pool replaying it in
# Workers::spawn_into -- so no unit test in the app process can reach it. This
# drives a real RenderService in worker mode. It takes no arguments: the two
# fixtures it needs are named in the file, because the properties are about
# encryption rather than about content.
#
# Proved able to fail by ten mutations, each reddening exactly the check it
# belongs to. The first is the one worth reading, because its failure is the
# defect a naive implementation ships:
#
#   spawn_into skips the unlock       -> "8 served, then: This document is
#                                        locked" -- the first worker's tiles come
#                                        back and the pool's second worker
#                                        refuses. Every other check stayed green,
#                                        including "a tile renders with ink".
#   Response::locked never sets it    -> both "refused as locked" checks
#   unlock does not reword a retry    -> "the second refusal is worded
#                                        differently from the first"
#   serve never enters the unlock loop-> 4 failed, 2 skipped
#
# And six more for the password's onward hops, added 2026-08-23. Each is a
# one-line edit, restored afterwards, with the file digest checked before and
# after so a mutation that did not land cannot read as a survivor:
#
#   RawDocument::password -> None      -> 4 red: properties (locked=true), links
#                                        (2 pages unaccounted for), mapping (2
#                                        truncated), and the save refused
#   docinfo::scan drops it             -> the properties check, alone
#   links::scan drops it               -> the links check, alone
#   encoding::scan drops it            -> the mapping check, alone
#   annots::scan drops it              -> the comments check, alone -- and it
#                                        reddened NOTHING until that check was
#                                        written, because the fixture has no
#                                        comments and a count of them cannot tell
#                                        "none" from "could not look"
#   Workers::password -> None          -> the save: "the service holds None"
#
# WHAT IT NEEDS. testdata/incr-encrypted-pw.pdf, which pyhanko writes -- it was
# built with qpdf until 2026-08-23, and qpdf is not on a hosted runner, so this
# whole probe printed [SKIP]s there. It is in scripts/ci_fixtures.py's --signed
# group now and both workflows already install pyhanko. Without the fixture this
# still prints twelve [SKIP]s naming the reason rather than passing.
#
#   macOS arm64, 2026-08-23   12/12, 0 skipped
#                             the save check reports: 986 bytes appended to 2346,
#                             still AES-256, 2 pages
cargo run --release --manifest-path src-tauri/Cargo.toml --example password-probe

# Structural soundness: does an independent VALIDATOR accept what tpdf writes?
# PLAN.md section 6 step 5 asks for a parser that did not write a rewrite to
# re-check it, and measuring that on 2026-08-26 gave an uncomfortable answer.
# Given a rewrite whose /Size claims more objects than the file holds --
# spike 0.4's defect -- lopdf's loader says "OK, 8 pages", PDFKit says "OK, 8
# pages", and only `qpdf --check` objects:
#
#   reported number of objects (142) is not one plus the highest object number (101)
#
# So the shipped check (verify::structure) is deliberately narrow -- a header,
# one %%EOF, no trailing data, a startxref inside the file -- and this is where
# the missing half is exercised. qpdf is not a dependency and is not on a hosted
# runner; it is on a development machine, so run this by hand before a release
# and after anything that touches how a document is serialised.
#
# Every fixture goes through the REAL writer, save::write_copy -- the same call
# Save a copy, Extract pages and Split reach -- with two plans each: keep every
# page, and drop one, which is the plan that makes rewrite() run the sweep.
#
# BOTH DIRECTIONS ARE FAILURES, and the second is the one to expect:
#   * qpdf refuses what we passed  -> the rewrite shipped a broken file.
#   * we refuse what qpdf passed   -> over-refusal, which is worse than no
#     check: it would refuse to save a document a reader had just edited. The
#     first draft of a /Size rule did exactly that.
#
# TWO CONTROLS, and the probe is worthless without them, because a sweep that
# reports "nothing found" looks identical whether the oracle ran or never did:
#   * a planted stale /Size must be REFUSED BY QPDF. It also re-measures the
#     gap: verify::structure is expected to pass that file, and a run where it
#     suddenly catches it contradicts its own doc comment -- read that first.
#   * planted trailing bytes must be REFUSED BY US, so a run where
#     verify::structure was never called is distinguishable from a clean one.
#
# A finding is compared against the SOURCE's own verdict. A rewrite faithfully
# carries a defect the input already had, and the first run of this reported
# outline-hostile.pdf for a loop in its /Outlines tree -- which is what that
# fixture is for.
#
# WITHOUT QPDF it prints one [SKIP] and exits 2 rather than 0: the caller wanted
# a verdict and there is none. `brew install qpdf`.
#
#   macOS arm64, 2026-08-26   66 rewrites checked, 3 plans refused by the
#                             writer, 0 findings, both controls fired
cargo run --release --manifest-path src-tauri/Cargo.toml --example qpdf-probe

# Redaction: does removing a region remove the words, and ONLY those words?
# src/redact.rs is asserted against hand-built content streams, which is right
# for "which operator gets deleted" and says nothing about a real document -- a
# fixture agrees with whatever its author had in mind. This is the corpus
# control: the same two functions, real files, through PDFium, with verify::scan
# asked whether the words left the FILE rather than the page.
#
# Five checks, and the three that assert LIMITS are the valuable ones:
#
#   text-base14.pdf   the account number is removed, and "Sphinx of black
#                     quartz" on another line survives -- the over-redaction
#                     control, without which emptying the page would pass.
#   links.pdf         eight pages drawing the same words, so the needle names
#                     its page. The first run of this probe marked a word that
#                     lives on every page, removed it from one, and correctly
#                     reported it still in the file.
#   text-marked.pdf   the same line, held SIX times: as /ActualText on a
#                     marked-content span, on the structure element that span
#                     belongs to, in two annotations, in /Info, and in an
#                     outline entry whose title is a substring of it -- of which
#                     only the annotation away from the region survives a
#                     redaction, which is what redact-apply-probe measures. Its
#                     outline is FOUR entries with the carrier in the middle of
#                     the sibling chain, which is the shape that catches a
#                     removal that drops the object without splicing.
#                     Since 2026-08-27 the span's copy is cleared by the removal
#                     itself, so the check reads the carriers apart rather than
#                     asking whether the secret is anywhere in the file: the key
#                     must be gone from the page's content stream, with a control
#                     proving it was there, while the scan must still find the
#                     word -- which by then can only be the annotations and
#                     /Info. Asking one whole-file question could not say WHICH
#                     copy went, and that is why the check that promised to go
#                     red on this very day did not; see TRAPS.md.
#   hostile-scan.pdf  a region over a /DCTDecode image reports an INCOMPLETE
#                     plan naming each object it cannot remove. Deny by default:
#                     taking the words and leaving a picture of the words is the
#                     confident lie section 6 opens by forbidding.
#   text-cid.pdf      the blind spot, asserted in both directions -- PDFium
#                     extracts the account number and verify::scan cannot see
#                     it, because Identity-H stores glyph ids. A run where the
#                     scan DOES find it means the instrument grew a capability
#                     its own documentation denies.
#
# Route B eats the line: PLAN.md section 6 removes the whole text-showing
# operation containing any redacted glyph, so a word beside the target goes with
# it. Every control word is on a different line for that reason, and the run
# prints how many of the page's operators went.
#
# It needs the fonttools fixtures (text-base14, text-marked, text-cid), which a
# hosted runner does not have -- see scripts/ci_fixtures.py. Without them the
# cases print [SKIP] and the run reports that nothing was checked rather than
# passing.
#
#   macOS arm64, 2026-08-26   2 cases ran, 0 failures, all three limits asserted
#   Windows x64, 2026-09-02   3 cases ran, 0 failures, all three limits asserted
#
# The third case is hostile-ocg.pdf, added 2026-09-02, and unlike the two above
# it a hosted runner CAN build it -- make_hostile_pdf.py is dependency-free. It
# reaches the one branch of the marked-content handler nothing else here does:
# a span written `/OC /MC0 BDC`, whose property list is a NAME into the page's
# `/Properties` rather than an inline dictionary, resolved to a real `/Type /OCG`
# dictionary a parser produced. Shown to be the only reacher rather than assumed:
# forcing the shared-list refusal reddens that case and leaves the other two
# green, because both of them write inline dictionaries.
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-probe

# Redaction, end to end: does the whole path actually remove the words?
# redact-probe proves the primitive -- given ordinals, the operators go. This
# proves the PATH: a rectangle built from the character boxes becomes a plan
# against PDFium's own object list, becomes ordinals in a save plan, becomes a
# written file, and the words are not in it. Everything between the drag and the
# file except the dialog and the command's own glue.
#
# TWO READERS, and the control is the point. The needle must be gone and a word
# on another line must survive, asserted through verify::scan over the bytes AND
# through PDFium re-extracting the written file. A scan that finds nothing
# because it cannot look is the failure this repository has recorded from
# several directions; the survivor is what says it can see the file at all.
#
# The region deliberately overlaps a path -- make_text_pdf.py draws four
# unrelated non-text objects -- so the plan is INCOMPLETE and the probe asserts
# that too. A rule under a line of text is what almost every real document has,
# which is why the command writes the file and reports it as unproven rather
# than refusing; see PLAN.md section 6.
#
# THE ANNOTATION CARRIER is the second phase, on text-marked.pdf, added
# 2026-08-27. A comment about a passage quotes the passage, so an annotation over
# a redacted region goes with the words -- popup and replies included. Three
# assertions and the middle one is the control: ANNOT-OVER must go, ANNOT-AWAY
# must stay (a reader's other comments are not theirs to lose), and the secret
# itself must STILL be found, because /Info /Title and the surviving annotation
# both hold it and this command touches neither. If that last one flips, the
# document-level carriers are being cleared and this probe needs rewriting.
# THE STRUCTURE CARRIER is the same row of the carrier table in its other home,
# asserted the same way: STRUCT-CARRIER (the element owning the redacted line's
# /MCID) and STRUCT-ANCESTOR (the element above it, which restates what was
# removed) must go, while STRUCT-OTHER -- the element for a line nobody marked --
# must stay. A rule that stripped the whole tree would pass the first two.
#
# THE DOCUMENT'S OWN DESCRIPTION goes whole: /Info and the XMP packet, asserted
# through the fixture's /Info /Producer string, which appears nowhere else in the
# file. The title is not used for this -- the title IS the secret, so its going
# would be indistinguishable from the page's own copy going.
#
# A check ahead of all of them asserts every marker is in the fixture to begin
# with, without which no direction could fail.
#
# THE FORM is three checks on the same written file, and its fixture is built so
# that the VALUE rule is the only thing that can decide either field.
# FIELD-CARRIER holds the redacted line's own account number and its widget sits
# at the far corner of the page, so the annotation pass leaves the widget and the
# field can only be taken by what it says; WIDGET-UNDER-CARRIER then has to come
# with it, or the page keeps an annotation whose /Parent is gone. FIELD-KEEP
# holds somebody else's answer and is the over-removal control. Both widgets are
# HIDDEN (/F 2) -- not decoration: a visible widget would be drawn by PDFium's
# form-fill environment and move every pixel comparison this corpus makes, and a
# hidden field holding the answer is the more honest shape anyway, since it is
# exactly the leak this carrier is about.
#
# THE OUTLINE is read back through outline::read rather than out of the bytes,
# because a byte scan cannot answer this carrier's question: an entry spliced out
# of the chain but still an object is neither present nor absent by a grep. It is
# also the point -- outline::read is what feeds the sidebar, so a title it still
# returns is a title a reader still sees. Four checks: the carrier gone, its
# child gone, OUTLINE-BEFORE surviving, and OUTLINE-AFTER still REACHABLE. The
# last is what the fixture's shape is for -- see TRAPS.md on forgetting a node in
# a linked list, and note that deleting the splice leaves the first three green.
#
# IN PLACE is the last phase and it is the same removal pointed at the reader's
# own file: stage a sibling, check the source has not moved, rename over it, and
# read back the path rather than the buffer. It works on a COPY of
# text-base14.pdf made into a file of its own -- pointing it at the fixture
# would leave every later run of every other probe reading a redacted one. Four
# checks and two are controls: the needle gone from the reader's own path, KEEP
# still there so a scan that cannot look would fail the first, the file still
# opening in PDFium with every page it had, and the staged sibling gone -- which
# is not tidiness, since a temporary left beside a redacted document holds the
# unredacted bytes.
#
# Its last section is text inside a Form XObject, on form-xobject.pdf: PDFium
# enumerates a form as ONE page object, so remove_shows has no ordinal that names
# what is inside it -- 9,310 of 154,095 realistic regions across 41 real
# documents, the largest carrier a redaction could not take that is made of
# ordinary text. Nine checks, and the discriminating ones are the third and
# fourth: the marked line goes and the line BESIDE IT IN THE SAME FORM stays, or
# a removal that emptied the whole stream would pass everything else here. Then a
# form the document draws twice is refused and no file is written.
#
# Its image section is the same shape on image-region.pdf, 8 checks, and the one
# that matters greps the written bytes for the picture's OWN PIXELS rather than
# asking what the page draws. Those read almost the same and are not: deleting
# the `Do` stops the page drawing it and leaves every byte in the file. That is
# why the fixture stores its images uncompressed, and it is what caught the
# rewrite's sweep condition not listing image removals.
#
# It needs text-base14.pdf and text-marked.pdf, which a hosted runner does not
# have. Without them the run prints [SKIP] and says so rather than passing.
# form-xobject.pdf and image-region.pdf a runner CAN build -- both are pure
# Python with no system font -- so those sections run there.
#
#   macOS arm64, 2026-08-27   48 checks, 0 failures
#   Windows x64, 2026-09-02   52 checks, 0 failures, 0 skipped
#
# The count moved 48 -> 52 on 2026-09-02, when the structure-element section
# gained `/Alt` and `/E` beside its `/ActualText` pair. `redact.rs`'s SHADOW_TEXT
# names all three keys and only the first had a fixture any redaction probe
# opened, so the other two were exercised by hand-built Rust dictionaries alone
# and the loader had never produced them. Proved by A/B rather than asserted:
# truncating SHADOW_TEXT to `[b"ActualText"]` leaves the STRUCT-CARRIER check
# GREEN and turns exactly the two new ones red.
#
# The macOS line above predates those four checks and has not been re-run.
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-apply-probe

# --survey answers the one question that decides whether the feature works on
# real files: how often does the correspondence guard REFUSE? redact.rs removes
# by position and refuses when the show operators lopdf decodes disagree with
# the text objects PDFium counted, because nothing connects the two lists but
# order. Spike 0.3 measured 4:4 on four fixtures built for it and said a TJ
# split across objects, or a Form XObject contributing from another stream,
# breaks it -- without saying how often.
#
# It asserts nothing. A page that disagrees is a fact about the corpus rather
# than a defect, and the pages that disagree are printed because those are the
# ones worth reading.
#
#   macOS arm64, 2026-08-26   48 files, 1720 pages, 0 disagreements
#
# Read that with its limit: testdata/ is mostly fixtures this project generates,
# so it is not a sample of the wild. It does include the hostile set, the signed
# contracts and the multi-column and multilingual pages. What the number
# supports is "the guard did not fire once across everything here", not "it
# never fires".
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-apply-probe -- --survey

# Signatures: does PDFium agree with us about the same signatures?
# `docinfo.rs` walks /AcroForm /Fields with lopdf; PDFium implements that walk
# in C++ and exports the result. Neither knows about the other, which is what
# makes this the instrument links-probe --mode agree is, for the subsystem where
# being wrong means naming the wrong signer. Seven comparisons per signature:
# the count, /SubFilter, /Reason, /M digit for digit, the DocMDP level, the
# signed byte count, and -- the one that matters -- the certificate parsed out
# of EACH READER'S OWN /Contents blob, compared by subject and serial.
#
# `clean` is the control and is not optional: two readers that both find nothing
# agree perfectly. `agree` REFUSES an unsigned document (exit 1) and `clean`
# refuses a signed one, so neither can report a vacuous pass.
#
# Proved able to fail by five mutations of docinfo.rs, each reddening exactly
# the check it belongs to: summing the byte-range offsets (11357 against 3869),
# never reporting a DocMDP level (0 against 2), reading /Filter as the subfilter
# ("Adobe.PPKLite" against "adbe.pkcs7.detached"), misspelling the /Contents key
# (docinfo read none, PDFium's blob read one), and not recognising /FT /Sig at
# all (0 signed of 0 fields against 1). Restored and re-run green each time.
# What it CANNOT catch, measured rather than reasoned about: a bug inside
# parse_certificate is invisible, because both sides of the certificate
# comparison use it. PDFium hands over the /Contents bytes and no view of the
# certificate set, so replacing the signer match with `certificates[0]` leaves
# the probe at 13/13 on incr-two-signers.pdf -- both sides pick the same wrong
# element. This is a differential over WHICH BLOB, not over what the blob says;
# the unit tests own the second half.
#   all five incr-* signed fixtures --mode agree   7/7 each, 35 comparisons
#   incr-ber                        --mode agree   7/7, and every one of the 7 is
#                                                 a comparison over a BER blob
#   incr-two-signers                --mode agree  13/13, the only fixture where
#                                                 the per-signature pairing can
#                                                 fail (reversing docinfo's field
#                                                 order reddens 4 of the 13)
#   tagged / comments / links       --mode clean   3/3 each
#
# The certificate comparison's (None, None) arm was hard-coded to pass until
# 2026-08-21 -- literally `report.check(..., true, ...)` -- so a document neither
# reader could read a certificate from printed "7 passed, 0 failed". It is a
# FAILURE now: every signature reaching that loop is one both readers found, so
# two empty answers mean the blob defeated both parsers. Proved able to fail by
# stubbing parse_certificate to refuse everything: incr-signed.pdf goes 6/1.
#
# incr-timestamped.pdf is the only fixture whose signature carries an RFC 3161
# token. genTime is PINNED by the generator (2026-08-21 12:00:00 UTC) so tests
# assert the instant rather than its shape; the TSA is pyhanko's offline dummy,
# so the structure is real and the trust is nil.
#
# That fixture is no longer the only evidence. Until 2026-08-21 one real signed
# document to hand carried a timestamp and tpdf read NOTHING from it -- its
# /Contents is BER with indefinite lengths, which `der` refuses by design.
# ber::to_definite_length walks the blob first, and the same document now reads
# its certificate, the key usage it states, and a timestamp from a real TSA one
# second after the signing time it claims. The control was the change stashed
# and the probe rebuilt: cert="(no certificate)" before, named after.
#
# incr-ber.pdf is the fixture for that path, and it is incr-signed.pdf with
# every constructed value in its blob rewritten in indefinite form and NOTHING
# else changed -- same length, byte-identical outside the /Contents span. The
# pair is what makes the check discriminate: two blobs that come out of the walk
# equal can only have done so by the length form being normalised away. It is
# built by rewriting rather than by signing, because pyHanko emits DER and has
# no switch for this.
#
# --mode read also prints the timestamp, when a signature carries one, and what
# each certificate states its key is for. Nothing
# compares that against PDFium, which exposes no extension accessor at all --
# the oracle is `openssl x509 -text` on the same blob, which is what
# `the_usage_a_real_certificate_states_is_the_usage_openssl_reads` is written
# against. For incr-signed.pdf both read "Digital signature, Non-repudiation",
# no extended key usage, and CA:FALSE.
for f in incr-signed incr-certified-1 incr-certified-2 incr-certified-3 \
         incr-certified-3-indirect incr-two-signers incr-ber; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
      "testdata/$f.pdf" --mode agree
done
cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    testdata/tagged.pdf --mode clean

# --mode nested asserts a DISAGREEMENT, and is the odd one out here on purpose.
# /AcroForm /Fields is a tree; PDFium's signature enumeration reads the array and
# stops, while docinfo.rs recurses. So a field under /Kids gives PDFium 0 and us
# 1, and --mode agree on that fixture reports a count mismatch that reads like a
# defect in us. Established by control: the same document with the leaf flat
# instead of nested gives PDFium 1, same signature dictionary byte for byte, and
# qpdf --check passes both. The mode says in its own output "if this is 1, PDFium
# now recurses and this mode is obsolete", so the limitation expires loudly.
#   signed-nested-field --mode nested   3/3
cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    testdata/signed-nested-field.pdf --mode nested

# Marks: does a highlight a reader makes land on the words they made it from?
# Run ALL FOUR modes, and run them on BOTH geometry fixtures -- that is not
# thoroughness, it is the only way two of the checks can fail at all. Measured by
# mutation: dropping the crop-box origin from the write path reddens
# `links-cropped` and NOTHING else, and mapping with no rotation reddens
# `rotated-90` and nothing else. An upright, uncropped page cannot tell either
# mistake from correct behaviour, and `--mode roundtrip` says so in its output.
#
#   roundtrip  writes a mark, reads it back through `annots.rs` -- a separate
#              implementation of the inverse mapping -- and compares. Also pins
#              the `/QuadPoints` corner order against the bytes.  9/9
#              Since 2026-08-18 the note is *typed* through `renote` rather than
#              passed at creation, so this covers the route a reader takes: a
#              highlight made with nothing to say, and the words added after.
#              Both routes end in the same `/Contents`.
#   ink        renders the saved page and counts wash per quad, with the SOURCE
#              page as the control. 90-96% of each quad across the corpus.  3/3
#   noap       the same with the appearance stream stripped, so the wash is the
#              renderer's own, from `/QuadPoints`. Nothing else reads those
#              numbers: a mutation reordering every corner passed every other
#              mode.  3/3
#   legible    the glyphs survive the wash. Removing `/Multiply` leaves 0 of
#              2,744 ink pixels on `text-base14`.  2/2
#   rule       where a line kind's rule actually lands, in rendered pixels. The
#              check no file-level assertion can make: PDFium generates its own
#              appearance for a markup annotation that has none, so this is what
#              says it honours ours. Two assertions, and the second is what
#              tells the kinds apart -- an underline puts every pixel in the
#              bottom third of the quad and NONE in the middle, a strikeout the
#              other way round. Refuses `--kind highlight`, which fills its quad
#              and is what `--mode legible` measures.  4/4 per kind
#   outline    that a box is a frame and not a filled rectangle, in pixels. The
#              one measurement no file-level assertion can make: a stroked box
#              and a solid block of colour satisfy the subtype, the rectangle,
#              the absent quads and the presence of an /AP equally, and a solid
#              block hides the figure the box was drawn around. Three readings
#              -- the source page as control, the whole quad, the middle inset
#              clear of the stroke -- plus the thinner of the two horizontal
#              edges' thickness, which is what says the stroke was not clipped
#              in half by the /BBox. Renders at 4x whatever --scale says, and
#              prints that it did: at 2x a full stroke is 3 px against a
#              clipped 1.5 and antialiasing swallows the difference. Refuses
#              every kind but square.  4/4
#   refuse     the refusals, with a control proving a real mark is still taken.
#
# `--kind highlight|underline|strikeout|note|square` chooses what to write, and
# every mode that writes a mark takes it: `--mode roundtrip --kind strikeout`
# re-runs the whole file check against a `/StrikeOut`, whose subtype, appearance
# geometry and opacity all differ and whose quads do not. The last two are the
# kinds a reader places rather than selects, and they are what makes the quad
# count a real assertion rather than a formality: both expect ZERO, in the same
# run where the three markup kinds expect one, so a writer that stopped emitting
# quads for everything is not mistaken for one that correctly omits them.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/links-cropped.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/links-cropped.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode noap
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/columns.pdf --mode noap
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode legible
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode refuse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind strikeout
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind strikeout
# `--mode rule` on every turn. Which third of the quad is "under" has four
# answers and the mode reads them off a table; `rotated.pdf` carries /Rotate 0,
# 90, 180 and 270 on pages 0 to 3, so one sweep exercises all of it. Until
# 2026-08-20 this mode was only ever pointed at an upright page and split the
# quad down the screen regardless, which reported 330/330/332 and TWO FAILURES
# on a sideways underline that was drawn correctly. 4/4 on each of the eight.
for page in 0 1 2 3; do for kind in underline strikeout; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/rotated.pdf --page $page --mode rule --kind $kind
done; done
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind note
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind square
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode outline --kind square
# The ellipse, through the same mode and the same three readings -- plus a fourth
# that is the whole reason it takes this kind at all. `--kind square` above and
# `--kind ellipse` below are a PAIR: the corner check asserts emptiness for the
# ellipse and INK for the box, so running only one of them leaves the other
# direction untested, and an emptiness assertion whose control never runs cannot
# tell "the corner is clear" from "the renderer drew nothing".
#
# Everything else in this mode passes for a rectangle drawn in place of an
# ellipse -- measured, by mutating `Paint::Ellipse` to `Paint::Outline`: whole
# quad, inner half and edge thickness all stay green and only the corner fires.
# 5/5 on each of the two.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode outline --kind ellipse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind ellipse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind ellipse
# Whether a foreign renderer reads a comment icon's `/C` at all, which settles the
# unchecked half of `docs/PLAN.md` open question 8. Two files differing only in
# `/C`, rendered by both readers, compared byte for byte -- no hue, no threshold.
# 5/5, and the numbers are the finding: PDFKit moves 439 px and PDFium moves 0,
# so Preview shows the reader's colour and tpdf's own renderer does not.
#
# THE FIRST CHECK IS A CONTROL AND IS NOT OPTIONAL. It runs the same comparison
# on a HIGHLIGHT, whose appearance stream carries the colour, so both readers
# must move -- 3379 and 3546. Without it the PDFium reading is an emptiness
# assertion with nothing proving the instrument looked: sending one colour twice
# leaves that check GREEN while three others go red, which is what proves the
# pair has to be read together.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode iconcolor --kind note
# Reference run: PDFKit 439 px moved, PDFium 0, controls 3379/3546 on a highlight.
#
# `hidden-probe` is the sequel and asks the question the ranked overlay work
# turns on: does PDFium honour /F bit 2, Hidden, PER ANNOTATION? It does. The
# fixture wants a highlight and a comment a hundred points apart on one page,
# which `--mode preview --kind note --out` builds on top of an already-marked
# file; the note's /Rect is then moved by an equal-length byte edit so the xref
# stays valid.
#
#   src-tauri/target/release/examples/hidden-probe both.pdf hidden.pdf \
#       --source testdata/text-base14.pdf \
#       --note-rect 50,200,110,245 --quad-rect 65,112,300,124
#
# 4/4 on 2026-08-31: 3919 px for the fixture's two marks, 373 moved by the flag,
# 2815 still in the highlight's quad, 0 left in the comment's rectangle. Its
# control is to pass the VISIBLE file twice, which reddens the two live checks at
# 0 and 373 -- and 373 in the icon's rectangle is also what proves that rectangle
# is aimed at the icon rather than at blank paper.
#
# `--out <path>` keeps the four files it writes --- two notes and two highlights,
# each pair differing only in `/C` --- under the temporary directory as
# `tpdf-iconcolor-<pid>-<Kind>-<blue|red>.pdf`. That is how a reader this probe
# cannot drive gets measured: on 2026-08-31 those files went into Adobe Acrobat
# DC, whose window was captured at its own accessibility bounds and diffed by
# region. Acrobat honours `/C` --- 873 px in the icon, 0 beside it, 0 for the same
# file opened twice, 24,642 for the highlight. See docs/PLAN.md §10 q8; and note
# that a whole-window diff is wrong here, because the tab title carries the
# filename and differs for that reason alone.
# The stamp. `--mode stamp` exists because `--mode outline` CANNOT FAIL for this
# kind: a stamp is a box with a word in it, so every reading that mode takes of a
# box is satisfied by a stamp except the one it has backwards -- it requires an
# empty middle and a stamp's middle carries its word. The new mode reads three
# bands, and each is a different way of drawing a stamp wrong: the whole quad
# (nothing was drawn), the middle third (a box), and the top edge (a text box).
# 4/4; the reference run reads 11,309 px in the quad, 717 in the middle and 513
# on the top edge, against 0 on the source page.
#
# `--mode preview` is the strongest of the three and PDFKit is why: an
# independent parser reads the annotation as `Stamp` and an independent RENDERER
# draws 1,306 px across its rectangle, neither of them ours.
#
# `--stamp <name>` picks which of the four; it defaults to `approved`.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode stamp --kind stamp
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind stamp
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind stamp --stamp draft
# The squiggle and its control, and they are a PAIR for the corner check's exact
# reason: `--mode wave` asserts that a squiggle puts ink in the strip above where
# an underline's rule stops, and that an underline leaves that strip EMPTY.
# Running only the squiggle leaves the emptiness untested; running only the
# underline is an emptiness assertion that "the renderer drew nothing at all"
# satisfies just as well.
#
# `--mode rule` is run for the squiggle too and passes -- its ink is under the
# baseline -- but it CANNOT tell a squiggle from an underline, because thirds of
# a quad put both kinds in the same one. That is why `--mode wave` exists.
#
# Upright pages only: `--mode wave` refuses a turned page rather than repeating
# `--mode rule`'s four-row turn table for a second mode. 3/3 on each of the two.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode wave --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode wave --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind squiggly
# The text box, whose /AP holds words rather than a shape. `--mode rule` and
# `--mode outline` both refuse it and should: its ink is wherever its words fall,
# which depends on how many there are, so thirds of a quad do not describe it at
# all rather than describing it coarsely.
#
# `--mode preview` is where it is measured, and for this kind that check asserts
# something the others cannot: the drawn line is as wide as `textbox::advance`
# predicts (110.0 pt against 109.4). That is the Helvetica widths table checked
# through PDFKit, and `helvetica-probe` below checks it through PDFium.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind textbox
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind textbox
# The Helvetica widths, against what PDFium actually draws. **The only evidence
# that table is right**: it is 95 numbers written out by hand, a wrong entry
# still draws and still wraps, and any unit test would compare the table against
# itself. Needs no fixture -- it writes its own page.
#
# Every string must come in UNDER its predicted advance and none may exceed it:
# ink runs from the first glyph's left edge to the last one's right, and an
# advance includes the trailing side bearing. 8/8.
cargo run --release --manifest-path src-tauri/Cargo.toml --example helvetica-probe
# Freehand ink. `--mode strokes`, NOT `--mode ink` --- that name was taken nine
# months earlier by the coverage measurement above and means something else
# entirely, which is the collision `MarkKind::Ink` walked into.
#
# The fixture is two strokes along the run with a wide gap, and the gap must be
# EMPTY: a writer that flattened `/InkList` into one path joins the first stroke
# to the second with a diagonal straight through it. The two outer bands are
# read as well, because a writer that emitted only the first stroke also leaves
# the gap empty. Renders at 4x whatever `--scale` says, and says so.
#
# SEVEN checks, not five, since 2026-08-20: the two added are how LONG each
# stroke is. The other five passed on `rotated-90` while every stroke came out
# at a nineteenth of its length -- 545 px against 10200 -- because two stubs at
# the ends of a rectangle put ink in both outer thirds and none in the middle,
# exactly as two full-length strokes do. Expect `249.0 pt of 249.2` on the
# sideways page and `255.5 of 255.8` on the upright one; the bound is 80%, so a
# green run has about a fifth of the rectangle in hand rather than the two
# hundredths of a point this mode's band arithmetic once stood on.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode strokes --kind ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode strokes --kind ink

# `--out PATH` keeps the marked copy instead of removing it, for opening in
# Preview or Acrobat by hand. Worth doing once per release: the probe proves the
# geometry and the pixels PDFium draws, and what it cannot prove is that somebody
# else's reader shows the mark at all.
#
# `--mode preview` IS THIS STEP NOW, and the paragraph below is the by-hand run
# that came first. It opens the saved file with PDFKit and asks eight questions
# of it; run it over every kind before a release:
for kind in highlight underline strikeout note square ink; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode preview --kind $kind
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/rotated-90.pdf --mode preview --kind $kind
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/links-cropped.pdf --mode preview --kind $kind
done
# 18 runs, all green: 8/8 on an upright page, 7/8 plus one [SKIP] on the turned
# one, and 9/9 for a note, which asks two questions instead of one about its
# rectangle. macOS only -- Windows.Data.Pdf renders but exposes no annotation
# object model, so the mode refuses there rather than half-running.
#
# THE WINDOWS COUNTERPART IS `--mode winreader`, and it asks a strictly smaller
# question. The sentence above is about METADATA -- the subtype, the author, the
# note, the rectangle -- and it is right about those. It is not about the pixels:
# a renderer draws our mark or it does not, whether or not it will answer
# questions about it. So this is a before-and-after inside ONE reader rather than
# a differential between two. Windows only; on macOS it refuses and names
# `--mode preview` as the mode that answers there.
#
# Five checks per kind, and TWO of them are controls. The renderer must draw the
# same page identically -- in PIXELS, not in bytes, because WinRT's BMP encoder
# is not reproducible and a byte comparison would condemn a correct renderer.
# And the mark's rectangle must be a MINORITY of the page, or "the difference is
# inside the rectangle" is true by construction.
#
# All nine kinds, 5/5 each, on `text-base14.pdf` at one pixel per point.
# Reference run: highlight 2,973 px changed with 100% inside its own rectangle,
# ink 1,536, ellipse 1,205, square 802, squiggly 576, text box 448, underline and
# strikeout 254 each. A run whose px counts differ by a few is antialiasing; one
# whose INSIDE share drops below 100% for anything but a note is not.
#
# THE NOTE IS THE EXCEPTION AND IT IS NOT A DEFECT. `Windows.Data.Pdf` replaces a
# /Text rectangle with its own icon, as PDFKit does -- and CENTRES it where PDFKit
# anchors it to the top-left corner. A 254x14 rectangle at 60,111 changes an
# 18x19 box at 178,109. So 84.4% of its pixels are inside and that is correct;
# the mode asks whether the icon is small and sits on the rectangle instead.
# `docs/TRAPS.md` has it under "The second reader substitutes the same icon".
for kind in highlight underline strikeout squiggly note ink textbox square ellipse; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode winreader --kind $kind
done
#
# ACROBAT IS THE THIRD READER AND THE ONLY ONE THAT NEEDS A PERSON. It exposes no
# automation interface in the copy installed here -- Adobe Acrobat 26.001.21789,
# Reader mode, no `AcroExch.*` COM classes -- so the instrument is a folder of
# files and a pair of eyes. Done 2026-08-31; `docs/PLAN.md` has the results. Five
# minutes, and it is the only check that can see a repair dialog, which is what
# the strictest structural reader in circulation says about an incremental save
# written by `lopdf` over a file it did not create.
#
# DERIVE THE EXPECTATIONS FROM THE FILE, do not write them from memory. The
# handover for the first run described the mark as covering "about `...jumps
# ove`" when 40 characters of Helvetica end after `lazy `, so a byte-exact render
# was reported back as a possible defect. Wrong the other way it would have been
# reported as a confirmation. `docs/TRAPS.md`: "A handover telling a person what
# to expect is a second implementation".
DROP="$HOME/Desktop/acrobat-check-$(date +%F)"; mkdir -p "$DROP"
cp testdata/text-base14.pdf "$DROP/00-CONTROL-unmarked.pdf"
for kind in highlight underline strikeout squiggly note square ellipse textbox ink stamp; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode roundtrip --kind $kind --out "$DROP/$kind.pdf"
done
# The twelfth file is the one that found something: `square.pdf` with its /AP
# stripped, 15 bytes smaller and otherwise identical. Acrobat draws it.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode noap --kind square --out "$DROP/square-no-appearance.pdf"
#
# ASK FOUR THINGS of each file: a repair or damage dialog on open; the mark drawn
# on line 1 in the colour written; the Comments pane listing it as `annot-probe`
# with the body `written by annot-probe`; and NOTHING of either on the control.
#
# WHAT IT CANNOT CATCH is in the mode's own doc comment as a measured table, and
# is worth reading before trusting a green run: every check is between two
# READERS, so a writer that moves something legally moves it for both. A /Rect
# shifted three points sideways passes here and fails --mode roundtrip; a
# /Subtype written as the wrong one passes here and fails a unit test; a missing
# /AP passes here because PDFKit draws its own.
#
# DONE BY HAND FIRST, 2026-08-20, and this is that record -- a by-hand step that
# leaves no record is a step nobody can tell was skipped. All six kinds were written to
# `text-base14.pdf` and opened with PDFKit, which is what Preview is, through a
# throwaway Swift harness reporting the annotation list and diffing the render
# against the unmarked original. Every kind came back with the right /Subtype,
# author and note, at the /Rect it was written at, painting pixels the source
# page does not: highlight 81% of its own box, note 77%, ink 37%, box 27%,
# strikeout 9%, underline 8%. The control -- the original against itself -- is
# 0 annotations and 0 pixels changed.
#
# THIS IS PHASE 2's EXIT CRITERION and it has no standing check. Two things a
# repeat run must know, both measured the same day and both able to produce a
# confident wrong answer:
#
#   * On a /Rotate page PDFKit draws the content ROTATED into an UNROTATED
#     frame. `page.bounds(for: .mediaBox)` answers 612x792 for a page poppler
#     renders at 792x612, and six of rotated-90's twelve lines are clipped off
#     the side. Meanwhile `annotation.bounds` returns the raw /Rect, unrotated.
#     So the annotation layer and the content layer sit in different frames, and
#     a coverage figure "inside its own bounds" reads 0.0% for a mark that is
#     drawn correctly. Do the positional half on an upright page only.
#   * poppler's `pdftoppm` honours /Rotate properly and draws annotations, so it
#     is the better oracle for a turned page -- and it is how the transposed ink
#     above was found. Not a dependency and not on every machine: a spike tool,
#     not a gate.

# geometry-probe: the page PDFium lays out, against the page the document describes.
#
# `FPDFPage_GetMediaBox` does not walk `/Parent`, so a page inheriting its box
# from an ancestor gets no answer from PDFium -- and `FPDF_GetPageWidthF` then
# reports `width x width` for one that also carries a quarter turn. That is a
# document laid out square with its content clipped off the sheet, and nothing
# errors. `RawDocument::page` repairs it by handing PDFium the box
# `pagetree::displayed_boxes` derived; this is what says so.
#
#   size    the displayed width and height PDFium reports equal the page tree's.
#           `400.0 x 400.0 against 600.0 x 400.0` before the repair.
#   box     `crop_pt`, which every coordinate on the page is measured from, is
#           that rectangle in the page's OWN space. Not implied by the size
#           check: before the repair this page answered `[0 0 600 400]`, the
#           right size in the wrong convention.
#   ink     the page draws something -- the reader-visible half, and the one no
#           structural check makes. 1, 3 and 0 inked pixels of ~26,600 before,
#           1013, 1062 and 1317 after. The floor is 0.1%, and the margin is
#           printed either way.
#   cost    the page tree was parsed IFF some page needed it. Every number above
#           is identical whether the parse happened or not, so a repair that
#           parsed every document would be invisible here and would put a whole
#           `lopdf` pass on the path a reader waits on.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example geometry-probe -- \\
    testdata/inherited.pdf --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example geometry-probe -- \\
    testdata/links.pdf --lib vendor/pdfium/lib
#
# **Run both, and the second is not a formality.** `inherited.pdf` is the only
# fixture in the corpus PDFium has no `/MediaBox` for, so it is the only one
# where the repair does anything -- and therefore the only one where the cost
# check passes for the wrong reason. A document that states its own boxes is
# where "the page tree was not parsed" has to hold, and it is the one mutation
# (`geometry: parse the page tree for every document`) that nothing else can
# catch. Measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there):
# 10/10 and 25/25.
#
# Three mutations, all caught, in `scripts/mutate_viewer.py` under `geometry:`;
# four more against the unit tests in `scripts/mutate_rust.py`.
#
# **It runs on any fixture, and the ink check has a precondition it cannot
# assert.** Every other window corpus is green (4/4 to 37/37) except
# `encodings.pdf`, which is 9/10: its page 2 extracts fourteen Japanese
# characters and renders 0 of 56,600 pixels, being `/UniJIS-UCS2-H` over a
# non-embedded `KozMinPro-Regular` that needs a substituted font. Size and box
# pass on that page, so it is a font on this machine and not a box. The probe's
# own header says why no guard is written for it.
#
# **`inherited.pdf` became a window corpus on 2026-08-24**, at 272/272. This
# said it was deliberately not one, on the strength of a single red check the
# agree phase reported at 27x; that turned out to be the turned-mark defect
# `turned-probe` covers, not a text-box one. What is left is a skip rather than
# a failure: on a 400-point page the phase's synthetic text box has no room for
# a line and both renderers correctly draw nothing, so it is left out of the
# comparison with the measurement in its detail line.

# turned-probe: does a mark land where the reader put it, on a turned page?
#
# `save::user_quads` maps a mark out of the reader's frame and into the page's
# own, which is right for the rectangle and wrong for anything drawn inside it
# that has a direction. On `/Rotate 90` an underline came out as a rule down the
# LEFT edge of the words, a strikeout as a vertical line, a squiggle down the
# left, a text box as a column wrapped to the box's height, and a stamp
# sideways at the wrong size. `/Rotate 90` is what a scanner writes.
#
# One mark of each kind on each page of a document whose four pages carry
# `/Rotate 0`, `90`, `180`, `270` and are otherwise identical -- its generator
# says so in as many words, which is the whole design: page 0's reading is the
# reference and the other three must match it, so nothing is predicted and no
# expected number is written down. Each page is rendered before the mark and
# after it, and the pixels that moved are reduced to a coverage and an ink box,
# both as fractions of the box the reader dragged.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example turned-probe -- \\
    testdata/rotated.pdf --lib vendor/pdfium/lib
#
# 29/29 as measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there).
# Four mutations in `scripts/mutate_viewer.py` under `turned:`, all caught;
# seven more against the unit tests in `scripts/mutate_rust.py` under
# `turned marks:`.
#
# **It is the only check on the squiggle anywhere**, that kind being a stroked
# zigzag: there is no `re` operand for a unit test to read and no line count
# that moves, so what it looks like is a question about pixels.
#
# Two things in the output that are not defects. A highlight's COVERAGE differs
# across turns and is deliberately not compared -- `/BM /Multiply` leaves a
# pixel alone wherever the paper is already dark, so its coverage is a reading
# about the page's content, and this fixture's type is in a different part of
# the display at every turn. Its extent is compared instead. And the last check
# is about the whole set rather than one kind: two kinds drawn differently have
# to READ differently, or a run in which every kind drew the same thing would be
# entirely green.

# insert-text-check: a replacement typed on an inserted page, read back by two
# parsers that share no code with tpdf.
#
# `save::import_tests` proves the writer with lopdf on both sides, which cannot
# notice a file it agrees with itself about. This runs the same path through
# `insert-text-probe` and then asks `qpdf --check` about the structure and
# `pypdf` about the words.
#
# The assertion that bites is NOT "the replacement is there". Page n of the
# opened document and page n of the file its pages came from are two different
# pages, and the fixtures make that collision real: page 1 of `text-base14.pdf`
# is inserted after page 1 of `links.pdf`, which has a page 1 of its own with
# different words on it. A save that edited the wrong document would produce a
# valid file, with the right page count, containing the replacement -- on the
# wrong page. So the check reads the opened document's page 1 back too, and
# every other page of it, and counts the pages the replacement appears on.
#
# Proved by control 2026-09-20: routing every replacement to the opened document
# (the mutation `save: import: write every replacement into the opened document`)
# makes it exit 1. Seven checks, all passing on `main`.
#
cargo build --manifest-path src-tauri/Cargo.toml --example insert-text-probe
uv run --with pypdf scripts/insert_text_check.py
#
# The probe alone, for a different pair or a different page:
#
#   insert-text-probe <base.pdf> <other.pdf> <out.pdf> \
#       [--page N] [--after N] [--replacement TEXT]
#

# redact-import-check: a region removed from the reader's own page in a document
# that also holds pages of another file, read back by two parsers that share no
# code with tpdf.
#
# Unlike `insert-text-check` this goes through the SANDBOXED WORKER for both the
# write and the verification (`save::InWorker`, which is what the application
# uses); every other redaction probe passes `save::Here`. It is also the only
# harness that drives a removal and an import in one plan.
#
# Two runs, and the second is the one worth having:
#
#   plain   the other file carries neither word. The scan finds nothing, the
#           file verifies, no note is added -- which is what says the fix is not
#           the old document-wide refusal wearing a different coat.
#   --echo  the probe synthesises the other file so that its one page prints the
#           very word the region covered. `verify::scan` reads the whole file, so
#           it reports that word as still present -- and since 2026-09-21 it also
#           says WHICH PAGE, which is what this run now checks against pypdf
#           saying the same thing from outside: the hit is placed on the inserted
#           page exactly, the control word that survives on the marked page is
#           placed there instead, the reason a reader sees names that page number,
#           and `redact::inserted_pages_note` -- the sentence that existed to
#           disclaim an answer -- is asserted ABSENT.
#
# The `--keep` control must not reach either note. It survives on the marked page
# by construction, so handing it to `redact::marked_pages_note` reports a removal
# that did not take on EVERY run; the probe narrows the report to the removal's
# own needles first. The comment saying so had been written for the other note and
# was not applied to this one -- `docs/TRAPS.md`, *A warning written for one
# consumer of a control is not attached to the control*.
#
# The inserted page goes in FRONT of the marked one, so the marked page's number
# in the base file and its slot in the output are different numbers. A writer
# that addressed the removal by slot would strip the inserted page instead, and
# the file would still be valid, still have the right page count and still be
# missing the word.
#
# 22 checks over the two runs, all passing on `main` 2026-09-21 (17 until the
# attribution landed). Proved by control: the mutation `save: address a removal
# by output slot rather than by baseline page` reddens the unit test that owns
# the same property, and `verify: follow the dictionary keys that leave the page`
# reddens the walk's own.
#
cargo build --manifest-path src-tauri/Cargo.toml --example redact-import-probe
uv run --with pypdf scripts/redact_import_check.py
#
# The probe alone, for a different fixture or word:
#
#   redact-import-probe <base.pdf> <other.pdf> <out.pdf> \
#       [--library DIR] [--needle W] [--keep W] [--echo]
#
# `--echo` writes its own source file as `echo-source.pdf` beside the output; it
# refuses an output named the same, because that path is removed before the save
# and the refusal a reader would then see names a missing insert source.
#
# `--page` is the zero-based page of the OTHER file, `--after` the slot of the
# opened document it lands behind. It prints one JSON object on stdout and
# everything else on stderr. Without `--replacement` it shortens the original
# to its first word, which fits by construction -- a longer one is refused by
# the writer, and that refusal is a correct answer rather than a probe failure.

# merge-probe: a merged document against the two that went into it.
#
# `save::write_merged`'s unit tests are lopdf reading back what lopdf wrote, plus
# a page count from the OS parser. Both say the tree is right. Neither says
# PDFium -- the engine tpdf renders with -- draws page seven, or that the page it
# draws is the page that was merged in. So this compares the merge against its
# SOURCES, per page, three ways:
#
#   size    the page keeps the size it had in its own file. The oracle is
#           `pagetree::displayed_page` reading the source's object graph, NOT
#           PDFium's reading of it -- see the trap about a rotated page whose box
#           is inherited, which PDFium answers `width x width` for. PDFium's own
#           reading is printed beside it so a disagreement is visible.
#   ink     the merged page draws something at all, which is what a lost
#           resource dictionary looks like from outside. A second check compares
#           it against the source's render, and SKIPS with the reason where
#           PDFium reads that source page at the wrong size, since its render is
#           then not a baseline.
#   text    the same code points come back. The one check that needs the fonts
#           as well as the stream: a page whose /Font went missing still renders,
#           because PDFium substitutes, and extracts the wrong code points.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example merge-probe -- \
    testdata/rotated.pdf testdata/links.pdf --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example merge-probe -- \
    testdata/rotated.pdf testdata/inherited.pdf --lib vendor/pdfium/lib
#
# Measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there): 50/50 on the
# first, 30/30 on the second. Four mutations, and the two that survive are as
# informative as the two that do not:
#
#   merge::append shifting by 0                   7/23   caught
#   merge::append grafting nothing                16/21  caught
#   pagetree::detached_page materialising nothing 21/30  caught -- ONLY on the
#                                                 second run; the first stays at
#                                                 50/50, because no page of
#                                                 rotated.pdf or links.pdf
#                                                 inherits anything. That is why
#                                                 `testdata/inherited.pdf` exists.
#   detached_page keeping /Parent                 30/30  SURVIVES, correctly: it
#                                                 drags the source tree in as
#                                                 unreferenced objects and
#                                                 changes nothing a renderer can
#                                                 see. The unit test
#                                                 `the_walk_does_not_leave_the_page_it_started_from`
#                                                 is what covers it.
#
# **The second run was 27/27 with 3 skipped until the geometry repair the same
# day.** The three were the ink comparisons, skipped because PDFium read the
# source page at the wrong size; it does not any more, so they run. The two
# `detached_page` rows were re-measured against the repaired renderer and the
# two `merge::append` rows were not -- they are on the first corpus, which the
# repair cannot reach, and its clean total is unmoved at 50/50.
#
# One figure did NOT reproduce and is recorded rather than explained: the
# materialisation mutation's first-corpus total was written down as 38/38 before
# the repair and measures 50/50 after, so twelve checks that skipped under it no
# longer do. The verdict is the same either way -- it survives on a corpus that
# inherits nothing, which is the whole point of the row -- and chasing the
# denominator was not worth a run. If you are about to rely on that number,
# measure it rather than reading it.
#
# `--emit PATH` keeps the merged file instead of deleting it, which is how you
# hand one to `backend-probe` (40/43, 3 skipped, through the sandboxed pool) or
# open it in the viewer by hand.
#
# A merged document is NOT a window-sweep corpus, and that was measured rather
# than assumed: `viewer_check.py` over a merge of rotated.pdf and links.pdf is
# 297/300, and the three are the mixed-page-size checks -- "the page is laid out
# sideways: wanted 0.8312 for a 612x792 page" -- which derive what they expect
# from page 1's aspect ratio. That is the documented reason `links-rotated` and
# `comments-rotated` are excluded from the sweep. Merging two documents of ONE
# page size would pass, and would have removed the only property that makes a
# merged document different from either input.

# crop-probe: the crop the reader sets, against PDFium rather than against us.
#
# Four modes, and the first is the one the whole design rests on:
#   follows    setting a page's crop box moves EVERYTHING that reads it -- the
#              reported size, the origin every character box is measured from,
#              the render, and the text mapping. Its control is the restore:
#              asking for no crop must put every one of those back, or the page
#              cache turns one request's crop into everyone's. Its FIRST check
#              is the only one here not derived from `crop_pt`: PDFium's page
#              size against `pagetree::displayed_page`'s, read from the same
#              file through lopdf. Every other check would survive a corrupt
#              crop rule, because their before and their after corrupt
#              together.                                          6/6
#   content    the measured content box is inside the page and encloses
#              something.                                        2/2
#   geometry   the crop's rectangle inside the file's page and the cropped
#              page's own reported size must agree -- two derivations, one
#              through `text::to_device`'s rotation table and one from PDFium.
#              Its control is the uncropped case, where the rectangle has to be
#              the whole page at the origin.                      3/3
#   ink        cropping to the content box raises the ink per rendered pixel.
#              The reader-visible claim, and the one no structural check makes.
#              Skips with a stated reason on a page whose ink already reaches
#              its edges, which is the honest control for every other row.
#
# Green on all fourteen corpora. Two rows are worth running by hand after any
# change to the rotation table or the box arithmetic:
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/rotated-90.pdf --mode follows
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/rotated-90.pdf --mode geometry
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/links-cropped.pdf --mode content
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/columns.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/vector-heavy.pdf --mode ink

# `rotated-90.pdf` is not one fixture among fourteen here. It is the page that
# proves `FPDFPage_GetCropBox` and `FPDFPage_SetCropBox` are not inverses: with
# no `/CropBox` of its own the getter answers with the DISPLAYED rectangle, and
# writing that back shrinks a 612x792 page to 612x612. See `docs/TRAPS.md`.
# `vector-heavy.pdf` is the other end -- an A0 drawing with no margins, where the
# right answer is to crop nothing.

# `--mode agree` needs NO manifest since 2026-08-16, which is the point of it:
# it resolves the same outline through PDFium and through lopdf and compares the
# two lists, so any document with an outline is a test. Run it over real files --
# that is where it earns its keep, and the fixture offers 6 entries against 421:
#   find ~/Downloads ~/Desktop -name '*.pdf' -type f | while read -r f; do
#     cargo run -q --manifest-path src-tauri/Cargo.toml --example links-probe -- \
#       "$f" --mode agree 2>&1 | grep -E 'differ:|entries agree|no outline'
#   done
# Measured 2026-08-16 over 44 real documents: 10 agree, 0 differ, 34 have no
# outline, and 1 entry differs only in the reason PDFium cannot see -- a /Dest
# naming a destination that resolves nowhere, which PDFium reports as "no
# destination". That pair is allowed by name; any other difference fails.

# Both whole-document scans now report `pages_missed` -- pages PDFium has that
# `lopdf` could not account for. Worth knowing before reading a zero: swept over
# every fixture on 2026-08-16 it is 0 everywhere, so the two parsers agree about
# page count on every document PDFium will open. `incr-encrypted-pw.pdf` was the
# usual counter-example and was not one then, because PDFium would not open it
# at all -- and it IS one now, since tpdf can ask for a password. Without the key
# `lopdf` reads no objects and reports 0 pages for a document PDFium paginates as
# 2, which is why every one of these readers takes the password. password-probe
# asserts it is 0 once the key arrives and its mutation drives it to 2. To re-run
# the sweep:
#   for f in testdata/*.pdf; do
#     printf '%s: ' "$(basename "$f")"
#     cargo run -q --manifest-path src-tauri/Cargo.toml --example links-probe -- \
#       "$f" --mode read 2>&1 | tail -1
#   done

# The worker boundary is still transparent: the two backends must agree byte for
# byte on tiles, geometry, text, search, outlines and comments, and a worker killed out of
# the OS process table must be replaced by one serving the same document. Run it
# on vector-heavy as well as a text fixture -- it is the only corpus whose render
# is slow enough for the withdrawal and drain checks to apply, and on every other
# one they report [SKIP] with the reason. vector-heavy is the run to read: 44
# check names, 2 skipped on macOS (43 until 26.9.2, when the retirement phase
# gained `growing the pool adopts the warmed spare and re-arms the slot`; 42
# until 2026-08-16, when comments were added
# to the comparison; measured 2026-07-31 at 42, and this said 41/1 then and
# contradicted the "all 42 names" sentence below it -- the prose was right).
cargo run --release --manifest-path src-tauri/Cargo.toml --example backend-probe -- \
    testdata/vector-heavy.pdf
```

The count that matters there is the count of **names**, not the split between passed and
skipped: the split moves with the corpus and with a thumbnail's timing, and chasing a
documented split back to its value is how a condition that keeps a check honest gets
deleted. What holds on every corpus is that all **44** names appear (43 until 26.9.2, when the
retirement phase gained `growing the pool adopts the warmed spare and re-arms the slot`,
which skips with a stated reason on a platform that pre-spawns nothing; 42 until 2026-08-16,
when the comment comparison landed) — diff the name sets
across two fixtures rather than comparing their totals, which is what caught a check that
had stopped existing on one-page documents.

One of the checks **skips itself** rather than passing, and it is the pattern to copy. *"A
search option crosses the worker boundary"* compares a whole-word search on both backends
against an unrestricted one; where the option changes nothing — a page with no extractable
text — it says so and skips, because two backends agreeing on the same result is exactly
what a worker that *dropped* the option would also produce.

**Do not run it under `caffeinate`.** `caffeinate -d -u <utility>` `exec`s the utility in
its own process and leaves a helper behind as that process's *child*, and every observation
of a worker here comes from the process table. The probe filters on the worker's argv for
exactly this reason, so it is now correct either way — but the same trap is waiting for any
new check that counts children, and it presents as a stable, reproducible failure that reads
like a real defect. `AGENTS.md` has the incident.

The worker pool has its own measurement rather than a check, because what it is for is a
number. It is not part of the bump checklist above — run it when the pool, the thread
count, or the tile path changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --rounds 4 --sizes 1,2,4,6,8
```

It interleaves the sizes across rounds and compares pairwise within a round, discards round
0, and reports the cold regime (the pool growing) separately from the warm one. Quote two
runs, not one: the four-worker figure moves several percent between runs while six barely
moves, and one run would present that as a measurement.

The other half of the same subject — what a grown pool costs to hold and what retiring it
gives back — is a second mode. Run it when the idle timeout, the reaper, or the number of
workers kept changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --mode retire --rounds 4
```

It reports the pool's footprint at three points and, per round, a warm screenful against
the first one after a retirement. `--idle-ms` sets the timeout it runs at (4 s by default,
so a round does not take half a minute); the app's own default is 30 s. The wait for a
retirement is **bounded and fails the run** if it does not happen — without that, the
second column would quietly be a warm screenful wearing a cold label, which is a number
that looks entirely reasonable.

Three notes on why these are written out in full. The target names are **hyphenated**, and
`--example remove_probe` fails as "no such target", which reads like a missing harness
rather than a wrong name. They are `--example`, not `--bin`, since 2026-07-31 — as
`[[bin]]` they were shipped inside the installer, all seventeen of them — so an older
command fails the same way, and the built artifacts moved from `target/release/` down into
`target/release/examples/`. **Delete any probe executables still sitting in
`target/release/`**: they are left over from before the split, nothing rebuilds them, and a
path copied out of an older document silently runs a frozen binary. And `remove-probe` with
no case argument defaults to case `a`, whose whole purpose is to segfault — so the obvious
invocation of the regression check crashes by design and looks like the bump broke
something.

The progressive checks are why the raw path restates `FPDF_ANNOT`,
`FPDF_REVERSE_BYTE_ORDER` and `FPDFBitmap_BGRA` by value: `pdfium-render` does not
re-export them, and a bump that changed any of them would silently alter every tile. The
run compares progressive output byte-for-byte against the safe path, so it fails if one
does.

### Test fixtures

`testdata/*.pdf` is gitignored and generated. Nothing it produces may be committed or
redistributed — `make_text_pdf.py` embeds a system font.

```
uv run --with fonttools testdata/make_text_pdf.py testdata
python3 testdata/make_hostile_pdf.py testdata
python3 testdata/make_vector_pdf.py testdata/vector-heavy.pdf
python3 testdata/make_vector_pdf.py testdata/vector-multi.pdf 200000 12
uv run --with pyhanko --with pyhanko-certvalidator --with cryptography \
  testdata/make_incremental_pdf.py testdata
python3 testdata/make_outline_pdf.py testdata
python3 testdata/make_rotated_pdf.py testdata
python3 testdata/make_columns_pdf.py testdata
python3 testdata/make_mixed_pdf.py testdata
python3 testdata/make_tagged_pdf.py testdata
uv run --with fonttools testdata/make_multilingual_pdf.py testdata
uv run --with fonttools testdata/make_encodings_pdf.py testdata
python3 testdata/make_comments_pdf.py testdata
python3 testdata/make_links_pdf.py testdata
python3 testdata/make_form_pdf.py
```

The last two were missing from this list until 2026-08-02, while the corpus table
below told a reader to run the viewer check against both — so the instruction that
produces a fixture and the instruction that consumes it disagreed, and the failure is
an absent file reported as a broken bundle. `text-heavy.pdf` is deliberately not here:
it is a real document rather than a generated one, and a machine that does not have it
cannot make it.

**What that was quietly costing, found 2026-08-22.** This limitation had been written
down three times and every one of them discusses *corpora* — a viewer sweep that cannot
run all fourteen, a `prespawn-bench` check that skips, a 109-name re-run taken on six.
All true, and none of them is where it hurt. **Ten `cargo test` tests over the save
path's guards also asked for `text-heavy.pdf`**, and returned at their first line without
it: here, and on both runners, which cannot have it either. A test that returns early is
counted among the `753 passed`, and its `[SKIP]` goes to a stdout libtest discards for a
passing test, so nothing in a green run said so. Every mutation aimed at those guards
SURVIVED, which is the only instrument that could tell.

They use `comments.pdf` now — generated, appendable, and carrying `/Annots` of its own
so the array-bearing branch is exercised too — except the one whose control needs a page
listing *nothing*, which takes `rotated.pdf`. All twelve `append` mutations and all 62
`save` ones are now caught by the test named for them. The guards were correct throughout.

The general form is worth carrying to the next "this machine cannot have X" note: the
question is not whether it is true, it is **what else consumes X**. `docs/TRAPS.md` has
the entry.

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

**Its signed fixtures did not exist on a hosted runner until 2026-08-21**, so CI tested none
of the signature reader. Both workflows now install pyhanko and call
`scripts/ci_fixtures.py --signed`, which builds the nine of them — eleven since the two
encrypted fixtures joined the group on 2026-08-23. Two things had to change for
that to be possible, and the first is why it had never worked: `make_incremental_pdf.py` called
**qpdf** with `check=True` and nothing else, so a machine without qpdf died there with a
`FileNotFoundError` naming a program rather than a fixture — and died *before* every signed
fixture, none of which needs qpdf at all. It skipped that one fixture from then on. The
second is `--scan-pages` with no values, which is the existing switch for not writing 550 MB.

**It calls qpdf for nothing at all since 2026-08-23**, and the skipped fixture is a runner
fixture now. `encrypt_with` writes both encrypted documents with pyhanko — one behind
`swordfish`, one on an empty user password — so `incr-encrypted-open.pdf` and
`incr-encrypted-pw.pdf` are in the `--signed` group and CI builds them. That is what makes
`password-probe` run on a runner instead of printing twelve `[SKIP]`s, and it is not a
tidiness fix: the save path's encryption guard had been wrong for four weeks with every gate
green, and the fixture that catches it was the one no runner could build.

Proved both ways before the step was written: with the fixtures moved aside and pyhanko absent,
`ci_fixtures.py --signed` exits **1** with `exited 0 but testdata/incr-signed.pdf does not
exist`; with pyhanko present it exits 0 and writes all nine. **Green on both runners since
2026-08-21**, after three pushes — the first two failed on assertions that had pinned a value
out of a locally generated fixture, which is one trap and worth reading before adding a test
that reads one. That hard failure is what makes the
tests' own `[SKIP]`-when-absent safe — a runner that failed to build them goes red at the step
that built them, not green through a suite that skipped.

**The fixtures are not reproducible, and CI generates them fresh every run.** Two consecutive
runs *on one machine* produce nine files of identical size and differing bytes, because pyhanko
mints a new key pair and serial each time. **Across machines the size moves too**: both CI
runners build an `incr-signed.pdf` of **8,097** bytes where this laptop builds **8,128**, on the
same commit. So nothing absolute may be pinned out of one — not a digest, not a serial, not a
date, and **not a size**, which is the one that looked safe after the local pair agreed and went
red on both runners at the first push. What replaced the pinned numbers is a quantity derived
from the file at test time, by a route `docinfo` does not take.

Three tests reading them ended with `assert!(examined > 0)` until the same day, which is red on
exactly the machines that cannot have the files — measured by hiding `testdata/incr-*.pdf`:
three failures, each telling a runner to generate what the repository had written down as
deliberately absent. They now assert that **every** named fixture was examined, behind an early
return when none of them exists. Both directions proved: no signed fixture gives 702 passed, 0
failed; hiding exactly one gives two red.

`make_comments_pdf.py` is the only fixture carrying annotations, and it is also one of the
three `scripts/ci_fixtures.py` builds on a hosted runner — it needs nothing but the standard
library, since the PDF writer it borrows from `make_text_pdf.py` reaches for fonttools only
inside the function that embeds a font. Two things about it are deliberate and easy to undo by
accident: its rotated page's rectangle is **not square**, because a square one maps to itself
under a quarter turn and cannot tell a rotation from an identity; and its three malformed
`/Annots` entries are written **before** the 1,200 notes, because the per-page bound stops the
scan at 1,000 and anything after that is never read. Both are in `docs/TRAPS.md`, both were
found by the fixture failing to discriminate rather than by review.

`make_links_pdf.py` is the fourth `scripts/ci_fixtures.py` builds on a runner, and
dependency-free for the same reason. It writes **three** files, and the third is worth knowing
about before reading a green `text-probe`: `links-cropped.pdf` has a `/CropBox` inset 50 points
from its `/MediaBox`, which is the case PDFium lays out differently from the sheet and which the
scans got wrong until 2026-08-16. Run `text-probe` against it as well as `links-probe` — the
text half of that fix is covered by the probe rather than by `cargo test`, because it needs a
live PDFium page:

```sh
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-probe -- \
    testdata/links-cropped.pdf     # character boxes land on ink: 96.4%
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links-cropped.pdf --mode check    # 6/6, 2 skipped
```

**That `text-probe` run reports two of its four controls as `[SKIP]`, and it should.** Both
link fixtures are 36 rows of even text, and a dense page of uniform lines cannot detect a
y-flip — the un-flipped convention reaches 87% and the `/Rotate 180` control 77%, against
0--5% on the fixtures written for this probe. So what a green run proves on these two is
**placement, not orientation**, and the probe now says exactly that in a `[NOTE]` line rather
than reporting an undiscriminating control as a failure. Until 2026-08-16 it failed them and
exited 1, so this documented command was red and this page quoted only its passing line.

**The 96.4% is worth reading against its own control rather than against 100.** Removing the
origin shift in `text.rs` takes it to **74.8%** — a `[FAIL]`, since the threshold is 95% —
while `text-base14.pdf`, which has no crop box, stays at 100%. That is the measurement proving
this probe covers the text half of the crop-box fix at all, and it is closer to the threshold
than it looks: a 50 pt inset moves each box by less than a line's height, so on dense text most
still overlap some ink. The `0% before the fix` figure this page used to carry came from a
different and larger error — the scan then mixed PDFium's *cropped* size with page-space
boxes. A fixture with a bigger inset would give the probe more margin.

96.4% rather than 100% is correct and not a near-miss: the fixture's text runs past the crop
box's bottom edge, so the characters the crop hides have boxes outside the rendered page. A
fixture whose every glyph sat inside the crop would not exercise that. Two things about it carry the same kind of intent as
the comment fixture's. Its **outline points at the same destinations its links do**, which is
what makes `links-probe --mode agree` able to compare tpdf's two destination resolvers at all;
delete the outline and that mode still runs, still prints a count and can no longer fail.
And the rotated page is a **separate file** — `links-rotated.pdf` — because a document
that mixes page sizes reddens two of `viewer_check.py`'s rotation checks, which derive what
they expect from page 1's aspect ratio. That is the same split, for the same reason, that
`comments-rotated.pdf` exists.

`make_tagged_pdf.py` is the other side of that coin: the only fixture that carries a
`/StructTreeRoot`, so it says what its own reading order is. Page 1 puts a margin note beside
the first paragraph — geometry reads it third, the tags read it last — and page 2 is the
control, tagged in the order geometry would have inferred anyway. A tagged fixture whose tag
order matches geometry tests nothing, so the generator **asserts the discrimination itself**
and refuses to write a fixture that has lost it. Its manifest states both orders, which is what
lets a check say "the tagged answer was used" rather than "an answer was produced".

Its manifest also carries the three fields `viewer_check.py`'s reading-order check reads
(`page`, `name`, `lines`), so this is an ordinary corpus for that harness rather than one it has
to know about — and what that check then asserts is the **lines**, in tagged order, against a
file a different program wrote.

Worth knowing as external evidence that the fixture is not merely self-consistent: poppler's
`pdftotext` reads page 1 in **geometric** order — heading, margin note, body — which is the
wrong answer the tags exist to correct.

**It carries two heading levels, and that is not decoration.** A page with one heading cannot
tell a consumer that uses the document's level from one that announces every heading as `h1`:
the mutation doing exactly that survived against the first version, and the check passed. A
property with one value present is the same as none — see the trap, whose list of the usual
suspects (one page, one rotation, one font, one column) is worth reading before building any
fixture.

`make_columns_pdf.py` is the only fixture whose *content-stream order* is the point. Its
three pages are two columns emitted column by column, the same two columns emitted line by
line across the gutter, and a heading spanning both over the second of those. The first two
look identical and must read identically, which is an assertion neither page can satisfy by
agreeing with itself. It writes `columns-manifest.json` beside the PDF, and
`viewer_check.py` passes any `<stem>-manifest.json` it finds through to the check — so
what reading order is compared against is a file a different program wrote.

`make_mixed_pdf.py` is the only fixture whose pages are not all the same size. Every other
document in the corpus is uniform — `make_rotated_pdf.py` builds a second, uniform file for
exactly that reason — so until this existed no check could fail on the frontend's largest
layout assumption. It is A4 with an A3-landscape insert (wider, same height, so a failure is
the crop and not the offset), an A5 page (shorter, so a failure is the offset), and an A4
control before and after both. Each page carries a marker at every one of its own edges, and
page 3 carries one just past A4's width, so a cropped render loses a named string rather than
losing something unnamed.

It writes `mixed-geometry.json` rather than `mixed-manifest.json`, because the
`-manifest.json` suffix enrols a fixture in the reading-order check and this one makes no
claim about reading order. `viewer_check.py` binds that sidecar to `TPDF_GEOMETRY_MANIFEST`,
the `geometry_manifest` command hands its contents to the webview, and the three layout checks
assert against it — against a file a different program wrote, rather than against the backend
the viewer renders through. On every other fixture those three say `[SKIP] no geometry sidecar
for this fixture`.

---

## Quality gates

```
scripts/gates.py
```

That is the whole checklist. **`scripts/gates.py` is the definition of the gates, not a
description of them** — it holds the commands with their flags, and this file deliberately
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
  most of the backend — the request queue and the `tile://` parser, the worker protocol
  and the pool, rendering, text, search, outlines, printing, session and sweep — with
  `npm run test` doing the same for the front-end logic beside it. What it deliberately
  leaves to the harnesses under `scripts/` is everything that needs a live webview — and a
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
  its own lifetime, but the gaps between runs — and any headless bench running alongside
  it — are unprotected, and a session that locks mid-batch fails the next frame-rate run
  outright. A locked macOS session cannot be unlocked from a script by design, so this is
  preventable and not recoverable.

- **The `toolchain` gate runs first, and it is what makes the Rust pin real.**
  `rust-toolchain.toml` names the compiler; `RUSTUP_TOOLCHAIN` in the environment overrides
  that file completely and silently, so a pin with nothing asserting it is indistinguishable
  from no pin. The gate compares the running rustc against the file, checks clippy and
  rustfmt came from the same toolchain commit, and prints `RUSTUP_TOOLCHAIN` whether or not
  it is set. Neither workflow uses a toolchain-installing action any more — both run
  `rustup show`, which installs exactly what the file names.

  To move to a newer Rust: edit `rust-toolchain.toml`, run `scripts/gates.py`, and commit it
  on its own. Expect `-D warnings` to surface new lints; that is the pin working, and dealing
  with them in a dedicated commit is the whole reason it exists.

- **The `workflows` gate compares `ci.yml` and `release.yml`'s `gates` jobs, and only those.**
  They must be the same job: one says a commit is good, the other stops a tag on a broken
  commit producing artifacts, and if the release copy is weaker then every ordinary push is
  checked harder than the thing that actually ships. They had drifted exactly that way —
  see step 10 of the release checklist for what it cost. The gate compares every `uses:` with
  its pinned SHA and every `run:` body, in order; step *names* are not compared, since two
  identical commands under different labels are still the same job. It deliberately says
  nothing about the `release` job, the triggers or the permissions: those differ on purpose,
  and that difference is the fork threat model rather than drift.

- **The `pdfium` gate is a pin check, not a build step.** It fails if `vendor/pdfium` is
  missing or is not the pinned build — which is the difference between a benchmark that
  means something and one that does not.

- **The `notices` gate runs last because it reads the build's output.** It derives which
  npm packages ship from `dist/assets/*.js.map` — the bundler's own account of what it
  emitted — so it needs the `build` gate above it to have run. Two checks in one command:
  that `THIRD-PARTY-NOTICES.md` still matches the dependency tree, which is the
  binary-distribution obligation; and that no GPL, LGPL or AGPL licence has appeared. Its
  third population is the one nothing else can see — the C++ libraries inside
  libpdfium, read from `vendor/pdfium/licenses/`, which `cargo metadata` is structurally
  blind to. Regenerate with `scripts/third_party_notices.py` and commit the result; never
  hand-edit the file. On a mismatch it prints the **diff**, not the word "stale" — a gate
  that fails on a machine you are not sitting at is only actionable if its message carries
  the evidence.

  **After any PDFium pin bump, cross-check the two archives.** They ship the same fifteen
  licence files and nine of them differ — eight by line endings, and `pdfium.txt` by
  carrying a `//` comment prefix on macOS and none on Windows. A document generated from
  whichever archive is installed is then a function of the platform, which is how this gate
  came to be green on macOS and red on Windows with nothing wrong:

  ```
  scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
  scripts/third_party_notices.py --cross-check /tmp/pdfium-win
  ```

  Note this is doable **from either machine** — the other platform's archive is a download,
  not a machine you have to be sitting at. Worth reaching for before waiting on a CI round
  trip to diagnose a platform difference.

### CI runs per push and per pull request, and again on a tag

`.github/workflows/ci.yml` runs the gates on `macos-latest` and `windows-2025` for every
push to `main` and every pull request, since 2026-08-02.

This section said "CI runs on a tag, and on nothing else" until then, and the reason it
gave was half wrong in a way worth keeping. The objection was never runner minutes — it was
that a workflow would be **a second place for the gate list to live**. `ci.yml` does not
restate the commands, it invokes `scripts/gates.py`, so that objection never applied to the
workflow that was eventually written. What changed materially is that the repository went
public and macOS minutes stopped costing 10x against a private allowance. "One machine" was
a description of the circumstances, not an argument.

**What CI cannot cover, and why the harnesses below stay manual.** `viewer_check.py` and
`mutate_viewer.py` drive a real window and need an unlocked, unoccluded screen. On a
headless runner they do not fail, **they hang** — which is the failure shape this project
reads worst, since a hang and a pass both produce no red. Do not add them to a workflow.

`.github/workflows/release.yml` fires on a CalVer tag. It **invokes `scripts/gates.py`** on
both platforms rather than re-listing commands in YAML, so the checklist and the gate stay
one object instead of two that happen to agree today. Then it builds, signs, notarizes and
publishes a draft release.

It arrived for a reason this document did not predict — it expected the trigger to be the
repo going public or a second contributor. What actually forced it is notarization: it needs
a Mac, a Developer ID and Apple API credentials, and a signed macOS release should not
depend on which machine is free.

**Its macOS half ran green on 2026-08-03**, and this paragraph said it had never run until
that day. Ported from `screenpick`, whose version is proven; the part with no precedent is
signing the bundled `libpdfium.dylib`, since neither sibling ships a native library.
Notarization requires every Mach-O in the bundle to be Developer ID signed with the hardened
runtime, so the dylib is signed in `vendor/` before the bundler copies it — correct whether
or not Tauri re-signs nested resources. The verification step fails rather than warns,
because a skipped notarization exits 0.

It took four rehearsal tags and each failed one step later than the last; step 10 of the
release checklist has the sequence, and all three defects are in `docs/TRAPS.md`. What the
green run establishes, beyond that the path works:

- **Signing a bundled native library for notarization works.** The `.app` is `Accepted`, the
  DMG is notarized and stapled, and both the app and the dylib chain **Developer ID
  Application -> Developer ID Certification Authority -> Apple Root CA** with
  `flags=0x10000(runtime)`.
- **Verified from outside the workflow, not only by it.** rc3's DMG was downloaded from the
  draft and checked on a machine that had not built it: `spctl -a -t open` reports
  `source=Notarized Developer ID`, `stapler validate` passes on the DMG and on the `.app`,
  the payload holds exactly one `libpdfium.dylib` and `THIRD-PARTY-NOTICES.md`. Worth doing
  again on any release whose verification step has been touched — rc3 is the case where
  the artifact was perfect and the checker was broken.
- **The macOS bundle layout for a resource map is settled**, which `pdfium_library_dir`
  records as unverified and the code cannot answer: the engine lands at
  `Contents/Resources/pdfium/libpdfium.dylib`, and the verification step prints the path it
  found.

### Windows runs the viewer, and how it came to be contained

**Read this section as a timeline, not as a status.** It opens with the state before
2026-07-29 — uncontained, failing open — because the controls taken then are what make the
later evidence mean anything. The present state is at *Windows no longer fails open* below:
workers are selected there, proved from outside the process. `AGENTS.md` carried the
pre-flip wording in its own gates section for a day after the flip, in flat contradiction of
its own constraints section, which is the hazard this note exists to prevent here.

`scripts/gates.py` reported **8/8 on `x86_64-pc-windows-msvc`** on 2026-07-29 — a dated
count, and there are twelve gates now, so ask `--list` rather than this line — and the same
day a Windows build **opened documents and passed the full functional check**. A clean clone bootstraps
with no changes — `npm install` and `scripts/fetch_pdfium.py` both do the right thing, the
fetch script selects the `win-x64` asset and verifies its digest.

`viewer_check.py` runs unmodified: `webview_guard` already returns early off darwin, and
WebView2 needs no bundle identity, so a plain `target/release/tpdf.exe` is enough where macOS
needs an `.app`. Two things about the invocation, both of which present as something other
than what they are. The binary must come from `cargo build --release --features
tauri/custom-protocol` or the window shows *"localhost refused to connect"* (see the trap —
the profile is not what embeds the frontend). And **pass it as a backslash path**:
`CreateProcess` does not accept a relative forward-slash path, so
`src-tauri/target/release/tpdf.exe` raises `FileNotFoundError: [WinError 2] The system cannot
find the file specified` for a file that is plainly there, from inside Python's `subprocess`
rather than from anything in this repository.

Four corpora, every one reporting the **86 check names** that were the invariant then, with
splits inside the ranges the table above records. Word and line selection took that to **89**
on 2026-07-30, after this run; the splits below are left as measured rather than adjusted by
arithmetic. A Windows re-run should expect **109** names — 23 added since, and the macOS
table further down says which of them skip on which document:

| fixture | ran | skipped | failed |
|---|---|---|---|
| `outline-simple.pdf` | 81--82 | 4--5 | 0 |
| `outline-hostile.pdf` | 197 | 37 | 0 |
| `rotated-90.pdf` | 184 | 50 | 0 |
| `vector-heavy.pdf` | 104 | 130 | 0 |

Re-run 2026-07-30 with pre-spawning live, since that changes the app's own behaviour — every
open now consumes a warmed process and starts another. All four green, no `[WARN]`, 44 modules
at peak with no `pdfium` among them over 27--978 samples. `outline-simple` reported 82/4 that
time against 81/5 before: the **name set** is what is invariant, not the split, and one of
them stopped skipping. A split that moves is information; a name that disappears would not be.

#### The 109-name re-run, measured

Done 2026-07-30 after the reading-order work landed, on the **six** corpora this machine can
generate — `text-heavy.pdf` is a real document rather than a generated fixture and has never
been on this box, which is the same reason `prespawn-bench` skips one of its checks here.

Every corpus reports the same **109** names and, more usefully than the count, **the same
split as macOS on every single one**:

| fixture | Windows ran / skipped | macOS table | failed |
|---|---|---|---|
| `outline-simple.pdf` | 102 / 7 | 102 / 7 | 0 |
| `outline-hostile.pdf` | 102 / 7 | 102 / 7 | 0 |
| `rotated-90.pdf` | 95 / 14 | 95 / 14 | 0 |
| `vector-heavy.pdf` | 62 / 47 | 62 / 47 | 0 |
| `vector-multi.pdf` | 70 / 39 | 70 / 39 | 0 |
| `columns.pdf` | 93 / 16 | 93 / 16 | 0 |

The name sets were diffed pairwise with the `cut -c8-47` recipe above rather than compared by
count, and all six are byte-identical to each other. Each extracts **110** lines, not 109: the
`the app process never mapped the PDF parser` line is a Windows-only observation printed
outside the check set, exactly as intended, and it is the only difference. 43--45 modules at
peak, no `pdfium` among them, over 32--1324 samples.

**One run of `vector-multi` failed before this and is worth reading rather than discarding.**
`activating a thumbnail goes to its page` reported `from page 1 to 1, wanted 7`, and the three
withdrawal checks beside it skipped — which looks like two findings and is one, since nothing
navigated, so no new thumbnail was ever requested to be in flight.

It led to a real defect in two classes. The page strip and the outline tree both activated
`focused` — a **mirror** of the DOM's focus kept by a `focusin` listener — rather than the
row the key event reached, and a mirror that misses an update sends the reader to whatever it
still names, which is page 1 because it starts at 0. `focusin` is not guaranteed: a document
without system focus moves `activeElement` without delivering focus events. Both now take the
row from `event.target` and keep the mirror only as the fallback for a key that arrived on the
container. Each has a unit test that was shown to go red first, plus a control on the
fallback; `sidebar.ts` had no unit tests before this.

**The intermittent itself was never caught a second time** — five further corpus runs,
including a replay of the back-to-back loop it came from and one under deliberate concurrent
CPU load, are all green — so this is an identification by mechanism and symptom, not by
re-observation. Contention was the first guess and was wrong when tested. The check now prints
`activeElement`, whether the strip followed, and `document.hasFocus()`, so a recurrence
settles it. See the trap *A mirror of the DOM's focus goes stale*.

Rendering, scrolling, zoom, pinch, view rotation, text selection, search, the palette, the
accessibility tree, the outline sidebar, thumbnails, inversion and the print command's
refusals all behave as they do on macOS.

**What was missing then was containment, not function** (superseded 2026-07-29 — see below).
`sandbox_init` is SBPL and macOS-only, so `Worker::spawn` refused off macOS and
`Backend::default_here()` fell back to `Backend::InProcess`. A Windows build parsed
attacker-controlled PDF **in the app process**, which is exactly what `AGENTS.md` and
`docs/THREAT-MODEL.md` forbid. **It failed open**:
`Worker::spawn`'s refusal is asserted by tests, but only a caller that asks for
`TPDF_BACKEND=worker` ever reaches it — the default selects in-process and renders perfectly
happily, so nothing refuses. A port owes a real containment answer (job objects, a restricted
token, a separate desktop) before Windows can ship. That, and not the viewer, is now the whole
gap.

It is at least **visible**: the uncontained default records `render::UNSANDBOXED_MARK` on the
startup timeline and prints `[WARN] no sandbox on this platform ...` on stderr, and
`viewer_check.py` echoes `[WARN]` lines even on a passing run — it previously showed stderr
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
profile and `OpenProcess` on the parent. It does not deny *reads* — an integrity level
governs writes — so the child is handed its document and its output as inherited handles
rather than paths. A restricting SID is stronger and unreachable directly: the loader's own
reads are denied and the child dies before `main`, which needs Chromium's initial-token /
lockdown-token handover to get past.

**A worker uses it now** (2026-07-29). `Worker::spawn` builds a contained child on Windows, and
`worker-probe` is the standing proof:

```
cargo build --release --example worker-probe
./src-tauri/target/release/examples/worker-probe.exe testdata/text-base14.pdf
```

**Run it against `incr-scan-40p.pdf` too.** It reports what a save's preparation costs the
worker — on macOS, 362.7 MB before the request and 1029.8 MB after, so the append itself adds
667 MB on that document.

**Measured on Windows 2026-08-22, and the margin is 4.3% rather than the 35% that was
reasoned.** The first measurement was taken from outside the process, because `[INFO]` could
not print here at all: it was guarded on `Worker::footprint`, which is `phys_footprint`, which
is `None` off macOS — so a Windows run was told to read a line the build could not emit. That
is fixed, and the fix is the point rather than the convenience. The quantity a job object caps
is **commit**, and `Contained::peak_commit` reads it through the handle the parent already
holds, so `Worker::peak_commit` is to Windows what `footprint` is to macOS and the probe prints
whichever the platform has, named. The footprint check is no longer a `[SKIP]` here: it reads
*"the parent can read what bounds the worker's memory"* and passes on both, so the probe now
reports **17/17 with none not applicable** on either platform.

**Two checks were added on 2026-08-24, so the number is now 19** — measured **19/19 with none
not applicable on Windows**; the macOS figure was 17/17 before they existed and has not been
re-run. They cover a worker that cannot load PDFium at all: it must **answer** the request with
a reason, rather than exiting 1 the way the shipped 26.8.8 did, and the reason must name the
engine rather than being the parent's epitaph for a dead child. The fixture is a directory with
no PDFium in it, so it needs nothing generated.

**Four more on 2026-08-26, so the number is now 23** — measured **23/23 with none not
applicable on macOS**; the Windows figure was 19/19 before they existed and has not been
re-run. They put a worker on the save's *verification* side, which nothing exercised until
then: `save::InWorker` was reachable only from the command bodies (in `lib.rs` at the time,
now `commands/mod.rs`), so every test and every other probe passed `save::Here` and the
shipped verifier was proved by compiling.

What they assert is a differential — the worker and the coordinator asked the identical
question about identical bytes — plus the two things a differential cannot say on its own.
That the worker's refusal is **`lopdf`'s and not PDFium's at document-open**: the fixture is a
real document with a trailer pointing at offset 999999999, which PDFium reconstructs and opens
happily while `lopdf` names the cross-reference table, so the two messages differ and the
assertion pins the wording. And that a **worker was involved at all**, which no comparison of
answers can establish, since an `InWorker` delegating to `Here` would agree everywhere; that
one points the verifier at a directory with no PDFium in it, where `Here` still answers and
`InWorker` cannot start a child.

Both of those exist because the first draft got it wrong in the reassuring direction — it
planted a file that was not a PDF, the worker refused it at open, and the check reported `[OK]`
having never run `lopdf`. `docs/TRAPS.md` has both entries.

**Five more on 2026-08-28, so the number is now 28** — measured **28/28 with none not
applicable on macOS**; the Windows figure was 19/19 before any of the last nine existed and has
not been re-run. They put a worker on the save's *writing* side, which is the half
`docs/THREAT-MODEL.md` residual risk 18 was still disclosing: the same four shapes as the
verification checks above, plus one that only this path can make.

The differential here is **byte for byte** rather than a number, and it is affordable because a
rewrite of one document under one plan is deterministic — every date in the output comes from
the plan's own marks and not from the clock. On `testdata/comments.pdf` under a plan that turns
every page, `save::Here` and `save::InWorker` both write 222,667 identical bytes. A comparison
of lengths or page counts would have passed for a worker that dropped the turns.

The extra check is the one that says the **output channel** is real: an ordinary worker,
spawned with no output file, must refuse the rewrite in words rather than writing a document
into whichever descriptor happens to be open at that number. Every other check in the section
would pass just as well if the descriptor were handed over unconditionally and
`worker::OUT_ARGV` did nothing.

**What the move costs is printed beside them.** On `comments.pdf` (4 pages, 238 KB) the
rewrite is **2.4 ms in this process and 11.4 ms in a worker, +9.0 ms** — best of five
interleaved, minima rather than means, because the question is what the work costs and not
what the machine was doing while it ran. That delta is one process spawn plus PDFium's
initialisation, so it is **fixed rather than proportional**: on a document where the parse and
the serialisation are hundreds of milliseconds it is noise, and this fixture is close to the
worst case for it. Nothing else on the save path got slower — the parse moved, it did not
happen twice.

**Six more on 2026-09-01, then six more the same day, then two, then three, so the number is
45.** The last three are the redaction read-back — the differential, the needs-a-worker
control, and the one that says the compared reports are about a document that was actually
read: the report must name a needle every PDF contains, not name one no document contains, and
have reached objects. Without that last control "the two agree" is satisfied by two reports
that looked at nothing, which is exactly what a worker answering from an unparsed document
produces.

The earlier ones. The second six are the page-range print and the merge — three checks each: the differential,
the needs-a-worker control, and the scratch or page-count reading. The last two are
`xref-bomb.pdf`, whose `/W` widths aborted lopdf 0.44. With 0.45 it is refused normally;
the check accepts that parser refusal or a contained worker death and requires no output.
There is deliberately **no coordinator arm**, preserving containment if this regresses. Three of them put a
worker on **Save a copy** and three on the **print job**, which are the last two writing paths
`docs/THREAT-MODEL.md` residual risk 18 was disclosing.

The copy's three are the shapes above with one difference: they go through `save::write_copy`
rather than through `save::InWorker` directly, so they cover the staging, the length check and
the rename as well as the channel — a copy's destination is a name the reader chose in a
dialog, which the in-place path never has. The differential is byte for byte again.

The print job's three are the same shapes for the one path whose answer comes **back**:
`NSPrintOperation` and `Windows.Data.Pdf` take bytes rather than a pathname, so the worker
writes into a scratch file this process created and this process reads it. The third check is
the one only this path needs — that no `tpdf-print-job.*` file is left in the temporary
directory, on the refusal as well as on the answer, because what it holds for the length of a
print is the reader's document with its encryption off and its pages in the order they asked
for.

**What those two moves cost is printed beside them**, measured the same way as the rewrite's
and reported as minima of five interleaved runs. On `text-base14.pdf`, over three consecutive
runs: the copy is **4.0 ms here and 11.9 ms in a worker, +8.0**, and the print job **0.2 ms
here and 7.2 ms in a worker, +7.0**. The rewrite on the same runs reads **+7.1**, and
`text-wide.pdf` gives +7.2 for the copy, +7.0 for the print job and +6.9 for the rewrite. All
three are one number: a process spawn plus PDFium's initialisation, **fixed rather than
proportional** to the document.

⚠ **The copy's in-process baseline is 4.0 ms where the rewrite's and the print job's are 0.2,
and that is the check rather than the copy being slower.** A copy is written through
`save::stage`, which ends in `sync_data` — the bytes have to be on the platter before the
rename swaps them in. A print job is read straight back out of the handle that wrote it and
never has to reach the platter, and `worker-probe` measures the rewrite through `Rewriter`
directly rather than through `stage_in_place`. Comparing the three *deltas* is comparing the
same mechanism; comparing their baselines is comparing three different amounts of disk.

⚠ **The first readings taken for this paragraph were 1.5 to 2 times these, and the machine
was the variable.** They were measured under a load average of 8 to 12 with another job
saturating the cores, which is exactly what interleaving does *not* correct for: it controls
for drift between the two arms, not for a machine that is slow for both. Take a reading only
when the box is quiet, and say so when it was not.

⚠ **`26.9.0` shipped without a Windows run, and this paragraph is why.** The rewrite's output
channel there is a `DuplicateHandle` into the child rather than a `dup2` before `exec`, so the
five checks above were macOS evidence for a mechanism that has two implementations, and nothing
had exercised the Windows one — last measured **19/19 on 2026-08-24**, before these five, the
four verification-side checks and the six writing-path ones above existed. `AGENTS.md` records what a single sentence about
two platforms costs.

**The first Windows run was 2026-09-01, and it is the CI step below rather than a run somebody
made**: `34/34 checks passed, 0 not applicable to this platform` on `windows-2025`, run
33501693368. Nothing was skipped, so the copy, the split, the print job and the output channel
are each watched working there. **34 was the count at that run**; the six checks added later
the same day for the page-range print and the merge have not been through a Windows leg yet,
and the next push is what takes them there.

**The requirement was real and the placement was the defect.** This said *"Run it on Windows
before the next release"* until 2026-09-01, and a release then went out without it. An
imperative in a reference section is read by whoever is already in that section; a release
gate is read by whoever is cutting a release, and those are different readings of the same
file. That is the checklist-weaker-than-its-gate trap, found by an external review rather
than by anything here.

**So it is neither, now: both workflows run this probe on both legs, on every push**
(`.github/workflows/ci.yml` and the `gates` job of `release.yml`, which the `workflows` gate
holds equal). It costs under a second, needs no screen, and runs against
`testdata/text-wide.pdf`, since `scripts/ci_fixtures.py` cannot produce the `text-base14.pdf`
above — the macOS reading is the same against both fixtures (42/42 as of 2026-09-01, and
28/28 when it was 28), and was taken in **both profiles** (0.36 s release, and the debug one CI
actually builds), so neither the fixture nor the profile is load-bearing. The debt is paid continuously rather than once, and the reading below
stays as the last thing measured by hand.

Reverting `worker_child`'s bind arm to `bind(&library_dir)?` turns both red with
`worker stopped answering (exited with 1 (0x00000001))` — which is the string the reader who
reported it saw, reproduced from the other end. That is the mutation to re-run if either check
is ever in doubt.

The probe's own reading on `incr-scan-40p.pdf`, which is the strongest form of the result
because it is the same three numbers macOS printed:

```
[INFO] the append moved the worker's peak commit 359.5 -> 1027.8 MB (+668.3)
[INFO] that is 95.7% of the 1024 MiB the job object allows, leaving 43.8 MiB
[WARN] 43.8 MiB of headroom against the commit cap --- a larger document cannot
       have its save prepared in the worker
```

macOS reads 362.7 -> 1029.8 (+667.0) for the same fixture. **Baseline, total and delta all
agree**, which is what settles that the two metrics are measuring the same thing and that the
delta was never the term to compare.

The `[WARN]` fires whenever headroom falls under `THIN_HEADROOM_MIB` (128 MiB, roughly what a
42 MB scan costs to prepare). It fires today on the largest fixture in the repository, and that
is correct rather than noise — it goes quiet when the append stops carrying a discarded copy
of the previous revision, and not before.

**The probe appends a document the application would not.** `save::APPEND_MAX_BYTES` bounds the
production path at 256 MiB, so a 336.6 MB scan is reserialised rather than appended when a
reader saves it. `worker-probe` asks the worker for the append directly and therefore still
measures it, which is deliberate: the bound is a judgement placed under a measured ceiling, and
it can only stay under it if something keeps measuring where the ceiling is. A run whose `[WARN]`
disappears is the signal that the bound can rise.

The sweep that bracketed the ceiling was read from outside the process the first time, through
PSAPI's `PagefileUsage` / `PeakPagefileUsage` over the probe's children. On `MOTHERSHIP`
(x86_64):

```
fixture                    file   peak commit   of the 1 GiB cap   append built?
incr-scan-5p            42.1 MB    134.7 MiB    13.2%              yes, 16/16
incr-scan-20p          168.3 MB    496.9 MiB    48.5%              yes, 16/16
incr-scan-40p          336.6 MB    980.3 MiB    95.7%              yes, 16/16
(41 pages, scratch)    345.0 MB   1004.4 MiB    98.1%              yes, 16/16
(43 pages, scratch)    361.9 MB   1020.7 MiB    99.7%              NO,  12/16
(48 pages, scratch)    404.0 MB   1020.5 MiB    99.7%              NO,  12/16
```

**The reasoning was wrong about which term to compare, not about the mapping.** The mapping
really is file-backed and not commit — peak working set runs ~343 MB above peak commit on the
40-page scan, which is the document. But macOS `phys_footprint` excludes clean file-backed pages
too, so the mapping is absent from the 1029.8 as well, and the 362.7 MB baseline taken for it is
PDFium's own allocation, which is private commit here. The two metrics measure the same thing:
**980.3 MiB = 1027.9 MB against the macOS 1029.8 MB, 0.2% apart.** The last two rows are their
own control on the reading: commit stops at 1020 MiB and the allocator then fails, so the number
being read is the number the kernel is enforcing.

So `incr-scan-40p.pdf` — the largest fixture in the repository — sits **4.3% under the cap**,
and the ceiling is bracketed rather than extrapolated: 345.0 MB saves, 361.9 MB does not. Above
roughly **350 MB an append cannot be built on Windows.** The failure is the safe direction and
worth stating exactly: the allocation fails, the worker aborts with `0xC0000409`, and the append
is prepared *before* `save_document` closes the document — so it is a `refused`, nothing is
written, and the reader keeps their edits. What they are told is `worker stopped answering
(exited with 3221226505 (0xC0000409))`, which names neither the size nor the cap.

The asymmetry that makes this odd from a reader's chair: only the **append** runs in the worker.
`save::Mode::Rewrite` goes through `spawn_blocking` in the app process, which is under no job
object — so on a 400 MB scan, highlighting a line cannot be saved while highlighting a line
*and deleting a page* takes the uncapped path. `docs/PLAN.md` §3 carries the ranking this
measurement now speaks to.

**11/11 checks, 1 not applicable**, on `text-base14`, `text-cid`, `vector-heavy` and `rotated`
— tiles **pixel-identical** to the in-process render, plus text extraction, outlines and
search across the boundary. That is what the run measured on 2026-07-29 and is left as it was
read: the probe gained three checks on 2026-08-22 — a save's update section built across the
boundary, re-parsed after being appended, and compared against the length it was built for —
so a current Windows run reports **14 of 14 with one not applicable**, and nobody has taken one.
A count in prose is a dated statement about a dated run; the probe's own output is the
authority, and macOS measured 17/17 that day.

**There is no not-applicable one any more, as of 2026-08-22.** It was the parent's memory poll,
skipped here on the grounds that the job object caps commit in the kernel so there is nothing to
poll — true, and the wrong conclusion: a kernel bound makes the reading matter *more*, because
what a reader needs is how close the worker came to being refused. What was missing was a way to
look, not a reason. `Contained::peak_commit` is it, and both platforms now report **17/17 with
none not applicable**.

Two things that check does *not* cover, deliberately, because a `cargo test` child is the test
harness and never answers: pipe **direction** and content. Both are the probe's job, measured
by mutating the pipe pair and watching the probe go red — see the trap *A test whose child
never answers cannot see the pipes being crossed*.

**Windows no longer fails open** (2026-07-29). `Backend::default_here()` selects workers there,
and the evidence is external rather than a mark of our own:

```
python scripts/win_modules.py <pid>          # on its own
python scripts/viewer_check.py <exe> <pdf>   # samples it throughout a real run
```

`viewer_check.py` now launches the app rather than blocking on it, reads the loaded module list
from outside the process while a document is open, and takes the **union** of its samples —
the parser is mapped only while a document is open, so a single look could miss it in either
direction. The module count is printed beside the verdict, because an enumeration that read
*nothing* reports "not mapped" exactly as containment does; a peak of zero is reported as a
broken observation, never as a pass.

Run **before** the flip it reported `[FAIL] the app process mapped the PDF parser, 47 modules
at peak`. That control is why the pass afterwards means anything. After: four corpora green
with unchanged ran/skipped splits, no `[WARN]`, 44--45 modules at peak, no `pdfium` among them.

That line is printed *outside* the check names on purpose — those are `viewercheck.ts`'s
and are the cross-platform invariant, and adding a Windows-only name to that set would make the
two platforms look divergent when they are not.

**Outside** means on **stderr**, and the passing direction of it went to stdout until
2026-08-02. `mutate_viewer.py` reads check results from stdout alone for exactly this reason
and its own docstring says so, so on Windows every baseline silently carried a 
"check name" that no mutation could turn red — and a mutation whose expected name happened
to be a prefix of that line would have been matched against the wrapper rather than against a
check. Both `[FAIL]` forms had been on stderr from the start; only the `[OK]` was not, which is
the direction nobody reads. Same family as the repository's own trap about a wrapper's verdicts
sharing a check's shape, arriving in the harness written after that trap was recorded.

#### Pre-spawning, and what it is worth here

Implemented 2026-07-30, so both platforms start a worker before a file is chosen. Only the
handover differs. A macOS parent sends a descriptor as `SCM_RIGHTS`; a Windows parent
`DuplicateHandle`s the document section **into the running child's handle table** and then sends
a `Handover` line naming the number it wrote. Writing into a low-integrity child is the direction
integrity levels permit, so this crosses the boundary for the same structural reason the macOS
one does. `Handover` is deliberately not a `Request` variant — a handover is legal exactly once,
and keeping it out of the request vocabulary makes a second one unsayable rather than something
the child has to refuse.

```
cargo run --release --example prespawn-bench -- --rounds 6 \
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
— `text-base14`, which embeds nothing, costs 10.38 ms against 8.87/8.99 ms for the two that do.
So on Windows pre-spawning buys almost entirely the fixed floor: `CreateProcess`, the loader,
mapping `pdfium.dll`, the token and the job.

Read that 1.4 ms as a between-document comparison, not as the warm/no-warm control. The bench's
own `a warmed worker does not pay the font walk` check needs `text-heavy.pdf`, which this machine
has not generated, and it `[SKIP]`s with that reason rather than quietly not running.

### `backend-probe` on Windows, and the defect it found in itself

```
cargo build --release --example backend-probe
./src-tauri/target/release/examples/backend-probe.exe testdata/text-base14.pdf
./src-tauri/target/release/examples/backend-probe.exe testdata/vector-heavy.pdf
```

| fixture | passed | skipped | failed |
|---|---|---|---|
| `text-base14.pdf` | 38/42 | 4 | 0 |
| `text-cid.pdf` | 38/42 | 4 | 0 |
| `outline-hostile.pdf` | 39/42 | 3 | 0 |
| `vector-heavy.pdf` | 40/42 | 2 | 0 |

**The name total is 43 as of 2026-08-16** — *"comments return the same list on both"* was
added with the comment layer — and the four rows above are the Windows measurement at 42,
left as they were taken. macOS re-measured the same four the day the check landed and reports
each row's passed column one higher against the new total: `39/43`, `39/43`, `40/43`, `41/43`,
with the skip counts unchanged. Windows has not been re-run.

That check compares what the two backends *return*, not whether either is right: a defect in
`annots.rs` breaks both identically and it stays green. `comments-probe` is what says the
answer is correct; this says the worker boundary does not change it. Proved to bite before
being trusted — truncating the worker's reply to three comments turns it red, and restoring
it turns it green, on the same fixture in the same minute.

Re-measured 2026-07-31. **The earlier `41`s were not a missing check**, which is what they
looked like: this table read `37/41 ... 40/41` against macOS's 42, and a handover went out
asking which check was macOS-only and proposing that the flat "all 42 names appear" sentence
above become a per-platform one. Nothing is macOS-only. The 41s were taken at `df1ca61`, and
`9fb728f` — the very next commit to touch this file — added *"a search option crosses the
worker boundary"*. Windows has had all 42 ever since; the name sets are byte-identical across
all four corpora here, diffed rather than counted.

Worth keeping as the shape of the error rather than only its answer: **a count taken at one
commit and compared against a count taken at another is not a platform difference**, however
neatly the two platforms line up on either side of it. The cheap discriminator is the one that
settled it in a single command — grep the *name* out of the source at each commit, rather than
reasoning about which check a platform might lack. The plausible hypothesis on offer was the
parent's memory poll, since `worker-probe` really does skip that one here; it was wrong, and it
was wrong in the direction that would have put a false per-platform caveat into this file.

That added check is also why `vector-heavy` moved from 1 skip to 2 while its passed count stayed
at 40: it skips where the search option changes nothing, which is a corpus with no extractable
text. The other skips are a slow enough render for the three withdrawal checks (only
`vector-heavy` has one) and a second page to confuse a page number with. The boundary, the pixel comparisons, capacity,
crash restart, replacement, retirement, close, descriptor return **and the spare's lifetime** all
pass. Its Windows primitives are Toolhelp for the module list and the process table,
`GetProcessHandleCount` for descriptors, and `TerminateProcess` for a hostile kill from outside
the pool — deliberately not `Contained::kill`, since the pool has to notice a death it did not
cause.

This is also where the Windows spare is proved end to end, and the detail says more than the
count: `at open: pool [18840], children [2672, 18840], spares [2672]` — a warmed child exists,
is excluded from the pool rather than miscounted into it, and `opened with 1` beside it keeps the
laziness claim. `a spare does not outlive the service that started it` reports
`its 1 spare process(es) [58096] went with it`.

**It first reported 34/41, and the two failures were the probe's own.** They said a burst grew
the pool to six and 1.2 s into a 4.0 s idle timeout one was left, with **144 handles with one
worker, 144 grown, 144 retired** beside it — and five extra workers cannot cost zero handles.
Two independent observations agreeing, and the diagnosis drawn from them (created, used and
**destroyed rather than pooled**) was recorded here as an open defect for a day. It was wrong.

Both numbers were honest; neither could say *when* it was taken. `settled_descriptors` waits up
to five seconds for a pre-spawned spare to appear, Windows has none, and the verdict of that wait
was discarded — so it spent its whole bound on every call, which is longer than the idle timeout
the phase runs at. The instrument retired the pool and then measured it. One worker of six and a
lean handle count are precisely what a correct pool looks like five seconds after a burst. The
pid clause is now asked for only where a spare can exist, and a wait that expires says so with a
`[WARN]`. Nothing in `workers.rs` changed.

Do not "fix" a failure here by relaxing a check — but do check the clock before believing one.
The pre-fix run remains the red control for both: they were observed failing, are now observed
passing, and `an idle pool is retired down to one worker` is green on both sides, so retirement
was never the thing that broke.

**A second check went red the day pre-spawning landed, and it was the same shape.** `closing
gives back every descriptor opening took` reported *137 quiet, 145 with it open, 142 after
closing it* — five handles, one spare's worth. Nothing leaked: an `open` consumes the warmed
spare and starts a replacement on another thread, so a raw sample includes one spare or not
depending on how far that thread has got. macOS forks and wins that race; Windows creates a
process, a token, a job and a fresh map of `pdfium.dll`, and does not. Its three samples now go
through `settled_descriptors`, which exists for exactly this and predated them. See the trap —
the lesson is that passing on one platform was evidence about that platform's timing.

**Of the "four probe binaries that refuse to act as a worker off unix", one did.** That list was
in this file and in `AGENTS.md` for two days and was wrong about two of its four entries, in the
direction a list written by reading always errs — see the trap. What was actually true:

- `pool-bench`, `prespawn-bench` — a real `#[cfg(unix)]` gate on the `--render-worker` re-exec,
  dating from before `worker_child` compiled on Windows. Worth understanding before copying it:
  each binary re-execs *itself* as a worker, so gating that made the benchmark **unrunnable**
  rather than degraded. Ported 2026-07-30, along with the hardcoded library path.
- `tile-bench` — **never refused anything.** It ran on the first try and failed at
  `LoadLibraryExW` on the hardcoded path. Ported the same day; numbers below.
- `worker-bench` — seven of its eight modes genuinely refuse, and the reason is accurate: it
  carries its own POSIX worker implementation, fd passing and SBPL profile bisection included,
  and shares no mechanism with the job-object model. Those need a spike, not a port, and the
  refusal now says what such a spike would measure that nothing else does (the per-tile overhead
  decomposition of `latency` mode — parallel scaling is `pool-bench`, the authority rungs are
  `win-sandbox-probe`, crash and timeout are `backend-probe`, and `limits`/`footprint` are
  answered by the job object capping commit in the kernel).

  That last clause was an assertion when it was written and is a measurement as of 2026-07-30:
  `win-sandbox-probe` now probes the job's own two limits, which it had promised in its table
  and never tested. `bare` commits 1 GB and starts a second process; every rung with a job is
  refused with 1455 (commit charge) and 1816 (process quota). So `limits` and `footprint` are
  retired on Windows honestly rather than by hand-waving, and `latency` is the only mode left
  whose question nothing here answers.

  **The eighth mode ran here for the first time, and it does not say what the threat model does.**

```
./src-tauri/target/release/examples/worker-bench.exe --mode engine --lib vendor/pdfium/bin
```

  `--mode engine` spawns nothing — it reads the library file — and was unreachable off unix only
  because it sat inside a `#[cfg(unix)]` module. It is at file scope now, and on Windows it
  reports **`[NOT VERIFIED]`**: the shipped `pdfium.dll` carries no local C++ symbols
  (`CPDF_Document` is absent), so `v8::` and `CXFA_` being absent from it means nothing. That is
  the harness's second control working exactly as written — and it means
  `docs/THREAT-MODEL.md`'s promotion of "JavaScript is disabled" to "there is no engine to
  disable" is established on **macOS only**. On Windows it rests on the asset name and pinned
  digest `fetch_pdfium.py` asserts, which is a claim about which file was fetched rather than
  about what is in it. The threat model now says so.

  It also prints the one dimension that survives stripping, because exports are always named:
  **460 exported functions, four of them XFA-named** — `FPDF_LoadXFA` and
  `FPDF_GetXFAPacket{Count,Name,Content}`. Surface, not a contradiction: the three
  `GetXFAPacket*` calls read `/XFA` streams out of an AcroForm dictionary and need no XFA engine.
  Whether `FPDF_LoadXFA` is a stub there is open, and unlike JavaScript it is behaviourally
  decidable — a fixture carrying an `/XFA` packet makes `FPDF_GetXFAPacketCount > 0` a positive
  control, so `FPDF_LoadXFA` returning false on it would mean the implementation is absent rather
  than the document empty. Not written; that fixture does not exist.

  Both numbers were cross-checked against a throwaway Python PE parse before being written down
  — two independent parsers, same 460 and same four names. Every branch was exercised: a
  non-PDFium file `[FAIL]`s, a file that passes both controls but is not a PE reports "not a PE
  image" rather than a zero, a missing `--lib` exits 2, and another mode still refuses.

**Numbers are macOS arm64 unless a Windows one says so.** The pre-spawn table above and the
tile-bench section below are the sets taken on Windows and are labelled as such; everything else
in this file and in `AGENTS.md` still is not, and the platforms are far enough apart — a ~1.4 ms
font walk against ~7.4 ms — that carrying a figure over is a guess, not an estimate.

### `tile-bench` on Windows, and what the render constants cost here

```
cargo build --release --example tile-bench
./src-tauri/target/release/examples/tile-bench.exe testdata/vector-heavy.pdf --mode single --rounds 4
./src-tauri/target/release/examples/tile-bench.exe testdata/text-base14.pdf  --mode single --rounds 4
```

It needed two fixes and neither was a refusal: the hardcoded `vendor/pdfium/lib` (on Windows that
directory exists and holds the *import* library, so it fails at `LoadLibraryExW` rather than at a
missing path — see the trap), and `peak_rss_mb`, which returned `NaN` off unix. That is
`GetProcessMemoryInfo`/`PeakWorkingSetSize` now, keeping the `NaN`-on-failure contract because a
zero would read as "PDFium allocated nothing". A working set is trimmed under memory pressure, so
it can read below the peak *commit* the same run reached; it is still the right counterpart to
`ru_maxrss` for this question, since both are about pages actually held.

**§9's architectural conclusions hold on Windows, and every constant behind them is worse.**
`vector-heavy` is generated by the committed `make_vector_pdf.py` against the same PDFium pin, so
this row is a fair comparison of *constants* — across different machines, which is the useful
framing here rather than a CPU verdict:

| | macOS arm64 (`docs/PLAN.md`) | Windows (2026-07-30) |
|---|---|---|
| 256² tile of the A0 page | 0.98 s | **1.35 s** |
| that tile as a share of a full render | 4.3% | **3.8%** |
| full page, 1× | 22.8 s | **35.1 s** |
| full page, 2× | 48.4 s | **88.3 s** |
| fixed cost per render *call* | ~1 s | **~1.3 s** |

So the shape is the same on both — PDFium culls spatially, a tile is a few percent of a full
render, and there is a hard per-call floor that does not shrink with the request. The magnitudes
are **1.5--1.8× worse**, and the floor about a third worse. The practical consequence: a latency
budget written against the macOS floor is optimistic on Windows by roughly that much, and
`docs/PLAN.md` §4's four consequences now rest on two platforms rather than one.

Independently cross-checked on the same machine before being believed: `backend-probe` measured a
**1536 ms** 512² render of the same document through the worker, against tile-bench's 2203--3073 ms
for that tile size. Same order, differing about as much as a centred tile and a placed one should
— which is what says the numbers are the document's and not the harness's.

The cheap-page half confirms the asymmetry the plan bets on: `text-base14` is **flat**, 0.6--0.9
ms/Mpixel at every tile size and scale, with no per-call floor at all. Read that as a Windows
result on its own and **not** as a comparison — macOS measured `text-heavy.pdf`, which this
machine has not generated, so the two cheap-page numbers are different fixtures.

### `pool-bench` on Windows: what a pool buys a screenful

```
cargo run --release --example pool-bench -- testdata/vector-heavy.pdf --tiles 6 --rounds 4 --sizes 1,2,4,6,8
```

Six 1024² tiles of the A0 page, which is what one screenful is. Two runs, so the stable
conclusions can be told from the noisy ones:

| pool | run A | run B | macOS (spike 0.5) |
|---|---|---|---|
| 1 | 5105 ms, 1.00× | 5176 ms, 1.00× | 1.00× |
| 2 | 1.34× | 1.52× | — |
| 4 | 1.99× | 2.29× | 2.56× |
| 6 | **3.59×** | **3.60×** | **3.22×** |
| 8 | 3.60× | 3.54× | nothing further |

**The shape reproduces exactly**: monotone gains to six and nothing at eight, which is the
capacity ceiling doing its job. Six is stable to within 0.01× across runs and is slightly
*better* than macOS.

**Do not read the middle rows as a platform difference.** Pool 2 moved 1.34 → 1.52× and pool 4
1.99 → 2.29× between two identical runs, and the per-round warm figures span ±20% (pool 2: 2625
to 3894 ms). Only the pool-6 result and the flat pool-8 result are outside that spread. The
per-round table is printed for exactly this reason — a single speedup column would have made
the intermediate points look like measurements.

Cold and warm are indistinguishable here (5155 vs 5105 ms at pool 1, 1100 vs 1094 at pool 6),
because a ~9 ms spawn is noise against a 5 s screenful. That is a property of this corpus, not
a finding about spawn cost.

Still macOS-shaped, but less than it was: **`open_check.py` runs five of six phases** since the
single-instance plugin closed the document-handover gap. The one that stays macOS-only is the cold
double-click, and that is not a gap — an Explorer double-click arrives in `argv`, which the
`argv` phase already covers. It skips with its reason rather than disappearing.

`session_check.py` needed no porting at all. It does need a document of **at least eight pages**,
since its target page is 7 and `goToPage` clamps; on a shorter one it now says so as a named check
rather than reporting a wrong page, which is what it used to do. `incr-scan-20p.pdf` is the quick
fixture for it; `text-base14.pdf` (1 page) and `rotated.pdf` (4) are not long enough.

Both now call `clear_strays` before their first launch. That is not tidiness: on Windows a
leftover instance **silently absorbs** every later launch through the single-instance plugin, so
the next phase reports `run timed out` with no output — which reads as the app hanging. It prints
a `[WARN]` naming the pids when it finds any, because a run that needed it is a run whose earlier
phases are suspect.

`webview_guard` still checks nothing off darwin (see the trap — Chromium throttles occluded
windows too, so those runs are protected by nothing).

#### What the port changed, so it is not rediscovered

- `worker.rs` now compiles everywhere and refuses off macOS — which its own module doc had
  claimed since it was written, and which was not true. 38 error sites, all POSIX:
  `std::os::fd`, `mmap`/`munmap`, `File::from_raw_fd`, `ExitStatus::signal`. `Shm` off unix is
  a type with a private field and constructors that refuse.
- `worker_child.rs` was `#[cfg(unix)]`, with the `--render-worker` argv refusing off unix
  rather than falling through. **Both are gone as of 2026-07-29.** The module compiles
  everywhere; three functions know the platform (the two mapping handovers and
  `establish_boundary`) and the rest is shared. The refusal that replaced the `cfg` is
  `establish_boundary` itself, which fails where there is no boundary to establish and does
  so *before* a document is opened — the deleted one was never the load-bearing guard, and
  keeping it would have suggested otherwise.
- `pdfium_library_dir()` picks `bin/pdfium.dll` on Windows against `lib/libpdfium.dylib` on
  macOS, and now checks for the **library** rather than the directory. See the trap: on
  Windows `vendor/pdfium/lib` genuinely exists and holds the import library, so the old
  existence check passed and the bind failed later.
- `launch.rs`'s percent-decoding test takes a platform-shaped URL. `Url::to_file_path` wants a
  drive letter on Windows and refuses `file:///Users/...`, so the macOS fixture asserted only
  that refusal. Written that way rather than gated off Windows, deliberately — a check that
  silently stops existing on a platform is the thing this file warns about elsewhere.

#### What running it found, which no gate could

Three defects, none of which any amount of compiling would have surfaced.

- **`npm run tauri build` failed on a tree that gated 7/7.** `backend_probe.rs` called two
  dyld symbols unguarded; clippy never links, and `cargo test` links a `[[bin]]` with `main`
  replaced, which drops them as dead code. There is a **`bins` gate** now, and it was proved
  to fail (5.7 s, debug profile) against the un-gated file before being trusted. The probe
  itself is now a thin entry point over `backend_probe/imp.rs`, refusing off macOS the way
  `fdpass_probe.rs` does — every claim it makes is about a worker backend that cannot exist
  there.
- **Not one tile was ever painted.** `tiles.ts` fetched `tile://localhost/...`, which WebView2
  cannot resolve; Tauri serves custom protocols at `http://tile.localhost/...` on Windows. The
  origin now comes from Tauri's own `convertFileSrc`, and the CSP names
  `http://tile.localhost` beside `tile:` — it already named `http://ipc.localhost` beside
  `ipc:`, so the convention was known and applied to one scheme and not the other.
- **`cargo build --release` is not a production build.** It produced a window showing
  *"localhost refused to connect"*: `frontendDist` is embedded by the cargo feature
  `tauri/custom-protocol`, which the Tauri CLI passes and a bare cargo build does not, at any
  optimisation level. Build through `npm run tauri build`, or pass the feature.

**The old version of this section named the wrong blockers**, and the shape of the error is
worth keeping. It listed `sanitize_rewrite.rs` and `tile_bench.rs` as the compile errors;
both were real, but clippy never reached either, because the *library* failed first. A
blocker list assembled by reading code cannot know what fails first — that is a property of
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
not uniformly slow but *selectively* slow — PNG encoding of a tile measured 67 ms in debug
against 1.41 ms in release while the PDFium render beside it moved 1.39 -> 1.36 ms. Ratios
invert rather than merely inflate.

**Startup timing needs a bundle, not just `--release`.** Under `tauri dev` the frontend is
served by Vite over HTTP, so a startup measurement describes Vite's module graph:

```
npm run tauri build -- --bundles app
scripts/startup_bench.py target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf <file.pdf>
```

Run the executable inside the `.app` directly — that keeps stdout and the environment,
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

`TPDF_IDLE_MS` is a quantity and **zero means zero** — retire at the first sweep. There is
deliberately no spelling for "off": a "no value" marker taken from the value's own range is
how a sentinel collides with a real value the moment the timing is right, which this
repository has already paid for once. A caller that wants no retirement asks for a long
timeout. Unlike `TPDF_BACKEND`, an unreadable value here falls back to the default rather
than refusing, because it cannot make two measurements silently incomparable — every
harness that depends on the timeout is handed one explicitly.

Anything else is **refused before the window is created** — one line on stderr, exit 2. The
variable exists to say which of two implementations ran, so a value that quietly selected
the other one would make any comparison between them meaningless, and `in_process` for
`in-process` is one underscore away.

The refusal is read in `run()` rather than where the backend is used, and that placement is
the whole of its value. `RenderService::start` runs in the Tauri setup hook, which `App::run`
invokes from AppKit's frames — a panic there is non-unwinding, aborts through a backtrace
with no symbols, and races the watchdog's 30-second report about a page that never ran. A
misspelt variable would be diagnosed as an occluded window.

Two things read differently under the worker: the startup timeline has `worker spawned`
where the in-process one has `pdfium bound`, and a render can now fail because the worker
died rather than only because the document did. `backend-probe` is what says the two agree
about everything else.

A worker that dies is replaced and the request retried once, so a crash usually reaches the
reader as nothing at all — but it is never silent in the terminal: the parent prints
`[render] document N: worker killed by signal 11; starting a replacement` on stderr, and the
worker's own stderr is inherited. Seeing that line repeatedly on one document means the
document is faulting PDFium on a page the reader keeps asking for, which is the one case a
single retry cannot make cheap.

### `ocr-probe`: does the recogniser work, and is the flip right

macOS only — it is the Vision binding it exercises. Nothing in it is wired into the viewer;
OCR has interfaces, one engine and a control chooser, and no worker yet.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `text-truetype`, `text-cid`, `rotated` | 9/9 |
| `outline-simple` | 8/8, 1 skipped |
| `form` | 7/7, 2 skipped |
| `columns` | 2/2, 4 skipped — two columns leave no vertically isolated span to use as a control |
| `vector-heavy` | 1/1 against the *inverted* claim: the page has no text, so reading none is correct |
| `links` | **7/8**, 1 skipped — one expected red, below |
| `encodings` | **7/8**, 1 skipped — one expected red, below |
| `text-wide` | 9/9 — the wide-sheet fixture, below |

⚠ **Those counts were two behind on 2026-08-28 and are re-measured here.** The shape sweep's
control was added in the same commit that last touched this table and the row was not moved with
it, so every fixture the sweep runs on read one low before today and two after. `columns` and
`vector-heavy` are unchanged, which is the tell that the drift is the sweep: it is skipped on
both. A count in prose has no gate behind it — derive it from a run, and re-derive it whenever
a check is added.

**A shape sweep prints above the checks**, added 2026-08-28 and not a check — it passes and
fails nothing. It exists because the corpus probe's largest remaining bucket is the engine
*answering* and returning no spans at all, and a corpus measurement cannot look at the image it
was handed while a fixture can.

**It sweeps the region strip's height, which is the corpus's own variable**, and every row is a
real `ocr_gate::stack` output rather than a resized one — so the sweep and any padding change
are the same code path. The control strip is byte-identical in every row. Two earlier drafts got
the construction wrong and both were corrected the same day: the first used the page's tallest
blank band and measured a shape the gate never builds, and the second grew the image with
`Vec::resize`, which appends white *below the bottom margin* rather than widening the image the
way `stack` would.

⚠ **The fixed rows cap the aspect, so the band the corpus goes silent in cannot be built here at
all.** `stack` always writes two margins, the gap and the control strip, and on these fixtures
that is 104 to 117 px of a 1190-wide image — a ceiling of **10.1:1 to 11.3:1**. Every target at
12:1 and wider prints how many rows short it is instead of a reading, because a shape that was
never built must not read like a shape that was tried and said nothing. Reaching past 16:1 needs a
*shorter control*: the aspect is `width_pt / (tallest + control_pt + padding)` and the scale
cancels, so with `padding` fixed at 24 pt a 595 pt page needs the region and the control together
under about 13 pt.

**Two checks are the controls over the sweep.** The strip has to read at *some* shape, or every
row is a statement about that strip rather than about the proportions; and the token has to read
back at the gate's own shape, or a "no" further out is not evidence about the shape either. The
second was written from `height == real_h` inside the loop, which no swept aspect ever produces
— it failed on all four fixtures and was right to. It reads the gate's own image explicitly now.

**The `trailing` column separates the aspect from where the padding rows sit**, and it overturned
the reason padding was called a candidate rather than a win. At equal height and equal aspect,
`outline-simple` at 4.0:1 reads the token back when the white is in the region strip and **not**
when it is appended below the bottom margin. The earlier "padding loses the token at 1.9:1" was
about trailing whitespace after the control, not about proportions. Through `stack`'s own
construction the token reads back at every buildable shape on `text-base14`, `outline-simple` and
`encodings`.

**`testdata/text-wide.pdf` is the only fixture that reaches the band the corpus goes silent in**,
and it says the shape is innocent. A 1684 pt sheet with ordinary 14 pt text builds an **18.1:1**
probe image and sweeps to **28.1:1**, where A4 with the same text caps at 10.8:1 — the lever is
the page's width, so the control strip stays a comfortable 34.5 pt against A4's 30.5. Vision
returns a span and reads the token back at 28.1:1, 24.1:1, 20.0:1, 18.1:1, 16.0:1, 8.0:1 and
4.0:1, and loses only the token (not the span) at 2.0:1. `docs/PLAN.md` §6 has why that kills the
padding repair: on a page of ordinary width a wide probe image *requires* a small control, so the
corpus could never separate the two.

⚠ **The two existing fixtures that come closest cannot answer it, and the sweep's own control says
so.** `text-heavy` and `incr-xrefstream` reach 12.2:1, and on both *the token reads back at the
gate's own shape* fails — their controls are too small to be read reliably, so no column of
theirs is evidence about shape. Do not read their `no`s as the wide band starting early.

**The control-chooser check**, added 2026-08-27 — the ninth on a fixture where every check runs, and named here rather than numbered because appending a check renames a number, and it is the only place
`ocr::control_from_page`'s claim meets a real engine. The three gate checks above it take their
control strip out of **Vision's own output**, which is the engine agreeing with itself; this one
chooses from what the *document* says and then asks Vision to read it back. It runs in both
directions: where the engine's reading and the document's text agree the chosen control must
certify, and where they do not it must refuse.

**Two fixtures report one red each, both of them expected, and they are expected for opposite
reasons.** An expected red beside a green run is a bad thing to leave lying around — this
file has an entry about exactly that — so here is what each is and what it would take to
remove it.

`encodings.pdf` fails *what it read matches the embedded text*, 0 of 2 words. That is the
fixture doing its job: it has no usable `/ToUnicode`, PDFium returns plausible garbage, and the
check compares the engine's reading against that garbage. Making it green needs a way to tell a
broken engine from a broken text layer, and there is not one — the check *is* that
comparison. The chooser check reads the same disagreement and reports the refusal as a pass,
which is the honest verdict about the gate rather than about the fixture.

`links.pdf` fails *a blank strip adjudicates Illegible*: the strip control picked the token
`"Donn"` and Vision, handed the same rows inside a composite, read `"Dann 1"`. Nothing is wrong
with the page. It is the weakness the chooser exists for, showing up in the check that predates
it — a control taken from the engine's own earlier reading is not stable across a second call
on a different image. The same fixture passes the chooser check with `"lantern"`, chosen from
the document. **The fix is to give those three checks the same control source**, which is a
change to three verified checks and is deliberately not in the increment that added the fourth.

**The check that earns its keep is the ordering one.** `normalised_to_points` has unit tests
and they cannot catch the thing that matters, because they assert arithmetic against numbers
the same file wrote — Vision's `boundingBox` is normalized with the origin bottom-left, and
whether the conversion understands that is a question about a black box. So the probe asserts
content at a position: the word the *document* places highest must come back highest. Removing
the flip reports `read gap -119 pt against 123 pt in the document` and takes both gate checks
with it.

Two limits worth knowing before reading a run. The control band is a strip of the page's own
text rather than a drawn token, so a fixture whose lines are too close together produces no
usable strip and the gate checks `[SKIP]` rather than failing — `columns` is that case. And
`[SKIP]` here means the harness could not construct the input, never that the gate passed.

### `win-ocr-probe`: can `Windows.Media.Ocr` be the Windows engine at all

Windows only, and it runs **in CI** — the Windows leg of both `ci.yml` and `release.yml`, as a
step after the gates. That is the point of it: the question is what a machine nobody configured
carries, and the developer machines are all configured. Read the two `[verdict]` lines in the
job log.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example win-ocr-probe
```

**Not a gate.** It measures, and its exit code says whether it could *measure* — 0 for any
answer including "no language packs", 2 for a call that failed. A probe that reddened CI for
reporting an inconvenient truth is one somebody switches off, and the answer would go with it.
The cost of that choice is the one `AGENTS.md` records about `18/19 gates passed`: a step that
always exits 0 is a step nobody reads, so the verdict lines are written to be grepped.

Four readings, and the last two are why this is not an enumeration:

| reading | what it decides |
|---|---|
| `AvailableRecognizerLanguages` | whether the in-box engine ships at all, which is `docs/PLAN.md` §9.10's ranking |
| `TryCreateFromUserProfileLanguages` | whether the call an implementation would make comes back |
| `MaxImageDimension` | a real bound on `ocr::Pixels`, since the gate hands over a composited image whole |
| a word and a **non-word**, read back | whether `Options::language_correction` can be honoured here |

The last row is the one that reaches the interface rather than the ranking. That option is
documented as off for verification *always*, because a corrector turns marks it cannot read into
plausible words; Vision honours it (`ocr_vision.rs`'s `setUsesLanguageCorrection`) and
`Windows.Media.Ocr` exposes no such switch. A non-word coming back as something else means a
verdict from that engine means something different from a verdict from Vision.

**First reading, `windows-2025`, 2026-08-29** — the runner image as GitHub ships it, which is
the whole point of taking it there:

| reading | value |
|---|---|
| recogniser languages | **1**, `en-US` (English (United States)) |
| `TryCreateFromUserProfileLanguages` | an engine, `en-US` |
| `MaxImageDimension` | **10000** px |
| 44 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |
| 16 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |

So the gating question is answered and answered well: a stock Windows carries a pack, and the
in-box engine is a feature that ships rather than one that needs the machine set up first.

The 16 px row was added the same day because the 44 px one alone is a control easier than the
check: 44 is about 3x `ocr_gate::MIN_CONTROL_PX`, a corrector's effect is largest on marginal
input, and marginal is exactly what this gate hands an engine — a control sized from the
smallest box a redaction covered. 10000 px is a real ceiling on `ocr::Pixels`, worth knowing
before a page is composited at render scale.

**No correction was observed anywhere the probe looked**, which is the better of the two
answers `Options::language_correction` could have had. It is support rather than proof, and the
gap is specific: at 16 px this engine read clean synthetic text *exactly*, so it was never
operating near its limit, and a corrector only shows where a recogniser is struggling. What the
gate actually hands an engine is harder than this in a way size does not capture — a control
composited beside real page ink, at whatever contrast the document has. **The remaining risk
therefore moved rather than closed**: it is no longer "the API exposes no switch, so the
contract may be silently broken" but "we have not yet seen this engine read anything it found
difficult". The instrument for that is the corpus sweep the macOS side already has
(`redact-reach-probe`), not another synthetic string.

⚠ **A blank reading for *both* strings is a suspect probe before it is a suspect engine.** GDI
writes RGB into a 32-bit DIB and leaves the alpha byte alone, so the buffer forces alpha to 255
after drawing; if that were wrong every glyph would be transparent and the engine would honestly
report no text. The comment in `draw` says so. Vary the fixture — a larger entry in `SIZES_PX`, a
different face — before concluding anything about `Windows.Media.Ocr`.

**The containment rung, added 2026-08-29.** Everything above runs at whatever integrity the
shell gave the probe, and a real engine would run where the parser worker runs. So the probe
re-execs itself with `--contained-child` through **`sandbox_win::spawn_contained` with
`Containment::default()`** — the containment that ships, job object plus low integrity — and
takes the same four readings there. macOS answered the mirror of this with *no*: Vision is
killed by SIGTRAP under `SANDBOX_PROFILE` and needs general `file-read`, which is why OCR is a
separate process under `OCR_SANDBOX_PROFILE`. If the same holds here, an in-box Windows engine
needs a second containment story rather than a line in the worker.

Three things make that rung worth trusting:

- **The child proves it is contained before it measures.** `sandbox_win::assert_contained()`
  first, exiting 3 if not. A child that quietly ran uncontained would report that the engine
  survives containment, which is the direction that costs something.
- **The verdict is a comparison, not a survival check.** The uncontained readings are the
  control and the two lists are compared as data. The outcome to fear is not a child that
  died but one that read something *different* — a substituted font or a denied resource
  looks exactly like that, and `docs/TRAPS.md` records a sandboxed PDFium returning `ok`
  while silently swapping a typeface.
- **Dying is a result, not an error.** The child's exit code is read before its answer is
  parsed and passed through `sandbox_win::describe_exit`, because macOS's lesson is that this
  class of engine aborts its host rather than refusing.

It reuses `sandbox_win` rather than building a ladder of its own. `win-sandbox-probe` built six
rungs to find which one PDFium survives; that question is answered and the answer is what
`Containment::default()` implements, so a second ladder here would be a second copy of
security-critical code — and the copy that drifts is the one nobody ships.

**Since 2026-08-29 this drives the shipping engine**, `ocr_windows::WindowsOcr`, rather than
calling WinRT itself. So every CI run exercises `WindowsOcr::recognise` end to end — bitmap
construction, the word walk, the coordinate conversion — contained and uncontained, instead of
a parallel copy of the same calls agreeing with itself. One thing it still cannot see: the probe
draws black on white, and exchanging two channels leaves black and white unchanged, so a missing
RGBA-to-BGRA swap is invisible to any reading here. `ocr_windows`'s own unit test is the
instrument for that, and has to be.

**Measured `windows-2025`, 2026-08-29: `reads IDENTICALLY to uncontained`.** All four readings
come back the same under job object plus low integrity as outside it, so `Windows.Media.Ocr`
does **not** repeat what Vision does on macOS — the engine needs no separate containment story
and can run where the parser worker runs. That is the last thing standing between the interface
and a Windows implementation of `ocr::Recogniser`.

⚠ **That run also revealed a regression this probe had shipped one commit earlier**, and it was
found by diffing the CI output against the previous run rather than by any check: extracting
`languages()` and `make_engine()` replaced a region of `main` that two `say` calls sat in, so
`engine language` and `max image dimension` stopped being printed while `BUILD.md` went on
recording 10000 px as measured. Restored the same day. See the trap about a readings table
outliving the code that produced it, and note what it costs to write a measurement down: the
thing to protect is the call, not the number.

### `ocr-sandbox-probe`: what is left of a process under each profile

macOS only. Three rungs, each a re-exec'd child that renders a page **before** the profile
comes down — the parser worker maps PDFium first too, and sandboxing earlier would measure a
different program.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-sandbox-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| rung | writes a file | reaches the listener | runs Vision |
|---|---|---|---|
| `bare` — the control | ok | ok | 4 spans |
| `ocr` — `OCR_SANDBOX_PROFILE` | PermissionDenied | PermissionDenied | 4 spans |
| `parser` — `worker::SANDBOX_PROFILE` | — | — | killed by signal 5 |

7/7 on OS build 25G83, 2026-08-27. This makes executable the table `ocr.rs` has carried by
hand since 2026-07-31, and it measures something that one did not: the rung that worked there
allowed reads and said nothing about **writes**, while the constant that shipped denies
`file-write*` and `network*`.

**The parent holds a real listener open and passes its port**, and that is not a nicety:
`ConnectionRefused` and a sandbox denial are the same shape from a client's side, so without
something to connect to every rung reports a refusal and the row measures nothing. The `bare`
rung is the control for all three columns — a machine where nothing works reports a
perfectly contained ladder.

### `ocr-worker-probe`: does the engine work from a process of its own

**Both platforms since 2026-08-29**, and the binary is **its own worker**: `OcrWorker::spawn`
re-execs `current_exe`, so what is under test is the shipped child rather than a copy of it.
Same arrangement `pool-bench` uses.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-worker-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: the default joins `PDFIUM_SUBDIR` — `bin` on Windows,
where `lib` exists, holds the *import* library and binds to nothing. It hardcoded `lib` until
this became portable, and `only_the_macos_spikes_hardcode_the_library_directory` is the rule
that caught it, which is what that test is for.

**It was macOS-only for one line.** The in-process baseline named `ocr_vision::Vision`
directly; `WindowsOcr` is behind the same `ocr::Recogniser`, so only the engine's
*construction* is per-platform now and everything after it is the trait. That mattered more
than it sounds: this probe measures the **worker**, the Windows worker is the newest thing in
the subsystem, and Windows was the one platform that could not measure it. A spike is
macOS-only when its subject is, never when one line of its scaffolding is.

It runs on both CI legs, at **12.7 s** in the debug build the `bins` gate leaves behind.

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `rotated`, `links`, `columns`, `encodings` | 12/12 |
| `vector-heavy` | 0/0, 1 skipped — A0 at scale 2 is 128 MB against a 16 MB buffer |

The check **set** is the invariant, not the total: on Windows the *engine is mapped from launch*
row is absent, because it is a statement about static linkage — `objc2-vision` links Vision,
while `Windows.Media.Ocr` is WinRT activated through `combase` at the first call. What its
images are and when they arrive is a different question and **unmeasured**; a row asserting a
name nobody has measured would be a guess wearing a check's clothes.

**The baseline is the same program reading the same bytes in-process**, because a worker that
reads nothing and an engine that reads nothing produce identical output. Everything else is a
difference from that row, and the differential is the one that matters on every page: same
engine, same pixels, one process apart, so the text has to be *identical* and a mismatch is
the handover rather than the engine.

Two rows are there because a caller cannot recover from them. An image larger than the shared
mapping must be refused **and leave the worker usable**, or one oversized region costs a whole
document its verification. And a worker killed from outside must report inside its own
deadline rather than block on a pipe nobody will write to — the engine ignores the
`deadline_ms` it is handed, so the parent is the only place that bound can live.

**What this probe does *not* prove, and the first draft claimed it did:** that the process
which asks never maps the engine. `objc2-vision` links Vision, so every binary linking
`ocr_vision` maps it at launch — 2 images of 619, before a single call. `backend-probe` can
make that claim about `libpdfium` because `pdfium-render` `dlopen`s it. The check states the
measured fact instead, with an emptiness control beside it. See `docs/TRAPS.md`.

### `redact-reach-probe`: how much of a redaction can be proved, over a corpus

Not a check — it passes nothing and fails nothing. It is the instrument behind
`docs/PLAN.md` §6's *What a removal can take, re-measured*, and it exists because the 39.1%
that section had quoted since the beginning was measured before the form carrier, before the
image carrier and before there was an OCR gate at all. **A figure that decides which increment
comes next is worth exactly as much as the date on it.**

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- \
    ~/Downloads --pages 3 --regions 40 --no-gate
```

**Counts and shapes only.** Point it at a corpus of real documents: no page text, no
recognised string and no filename beyond the stem leaves it, because a measurement that prints
what it read is one nobody can run twice.

| flag | what it does |
|---|---|
| `--pages N` | pages sampled per document, spread through it rather than off the front |
| `--regions N` | regions sampled per page — one per word of four characters or more |
| `--max-mb N` | files above this are not opened; a rewrite copies the whole document |
| `--no-gate` | skip the write-and-read-back half, which is 40x the cost |
| `--full-width` | widen every region to the page. A **control** over the gate, not the removal |

The cheap half is 1.8 s over 40 documents and 2,893 regions; with the gate on it is about 12 s
for a twentieth of that sample, which is why the two halves are separable.

**`--full-width` is a control that failed to isolate what it was aimed at, and is kept for
what it found instead.** `ocr_gate::strip` renders the rows a rectangle covers as a
full-width tile, so widening a region leaves the row band identical and should move no
verdict. It moves them a great deal — 54 *still readable* became 9 on one sample — because
a wider region covers more words, which changes the control the gate may choose, which
changes the render scale. The region feeds two mechanisms, so varying it isolates neither;
what it establishes is that **the verdict turns heavily on the control choice**, which
nothing else here measures.

The gate half reads `ocr_gate::judge_all` rather than `run`, so it has the engine's own
rectangles and reports how many surviving reads were inside the region's own columns. Since
`ocr_gate::mask_columns` that has been all of them, on 104 regions and again on 448.

**Every *not verified* region is attributed to a step, and the buckets have to close.** Each
prints as its own row — twelve of them, including the ones that never fired, because an
absent row and a zero are different readings. `NotVerifiedCause` is a type rather than a
substring of the sentence: the version before 2026-08-28 bucketed by
`why.contains("control token")` and discarded the verdict of every page-wide refusal, so it
could attribute one cause of twelve. The `[WARN]` beneath them is the check — buckets plus
run-refusals must equal the unanswered total, so a region that reached it by a route carrying
no cause is subtracted and named rather than absorbed.

**Two extra axes print under *control not read back*, and only under that one.** It is the sole
cause where the gate got as far as showing the engine something, so it is the only one with a
rendered control to describe. The first bucket is how tall that control landed against
`ocr_gate::MIN_CONTROL_PX`, which is the bound the scale rule exists to clear — a row below the
floor is the rule missing what it aims at, and on 2026-08-28 that was 34 of 38. The second is how
many characters the token drew, because `ocr::adjudicate` matches by containment and one
recognised span has to hold the whole token. The first prints every bucket including the empty
ones, with a `[WARN]` if they do not sum to the cause's own count.

⚠ **The token axis prints `unread / all` and a rate, and the denominator is not decoration —
it is what stopped a wrong increment being built.** Read as a numerator alone the bucket says 29
of 33 unread controls drew eight characters or more, which reads as an indictment of
`control_from_page` picking the *longest* qualifying word. With the denominator it says 29 of
**128**: a rate of 22.7% against 33.3% for five-to-seven characters, so long tokens fail *less*
and the obvious repair moves the chooser toward the worse bucket. A count of failures bucketed
by a property is never evidence about that property until the same bucketing is applied to the
population.

⚠ **That reading is macOS's, and Windows reverses it.** Measured 2026-09-02 over 109 documents
and 372 unread controls, the rate climbs with token length rather than falling: 7.6% at four
characters, 10.9% at five to seven, **23.6%** at eight or more. The two engines agree almost
exactly on the long bucket — 23.6% against Vision's 22.7% — and disagree on the short ones,
where Vision is three times worse. So the paragraph above is arithmetically right and its
conclusion does not travel: on Windows the repair it argues against is the correct one. The
denominator saved the argument from being wrong on macOS; what it could not supply is the
platform label, and a rate is a fact about the engine that produced it. `docs/PLAN.md` §6 under
*The gate on Windows* carries both columns.

**A third axis prints under the same cause: what the engine had actually returned.**
`ocr::Unread` rides on the verdict and carries how many spans came back for the whole probe
image, how many fell in the control band, and how far outside the band the nearest span
*containing the token* sat. Three rows follow — *read nothing at all*, *read spans, none
holding it*, *read it, outside its band* — each split by the rendered-height bucket beneath
it. The split is the point: the height rows and the shape rows are two bucketings of one
population, and two marginals bound their overlap without measuring it. Measured 2026-08-28 over
197 refusals at three densities, the outside-the-band row is **0** at every one, and at
`--regions 40` exactly 40 of the 80 silent refusals had a control at or above `MIN_CONTROL_PX`
— which the marginals alone could only place between 40 and 80.

**A fourth axis, added 2026-08-28: the probe image's own proportions.**
`ocr_gate::geometry_for` reports the shape it planned, and the row prints `unread / all` and a
rate per aspect band, with the silent count beside it. It is a different axis from the control's
rendered height rather than another reading of it, because an aspect is a ratio and the render
scale cancels out of it — a probe image halved to fit the buffer keeps its shape. Measured over
40 documents at `--regions 12`: **12 / 36 up to 8:1, 28 / 294 between 8:1 and 16:1, 26 / 36 beyond
16:1**, and all 36 silent refusals are in the two tails with none in the middle band that holds
four fifths of the population.

⚠ **That row's denominator has to be counted in the per-region loop, not inside the
`ControlUnread` branch.** Written one scope too low it counts the failures, so every band prints
`N / N 100.0%` — which happened on 2026-08-28, one increment after the trap about denominators
was written. The `bad / all` form is what makes it visible; a bare percentage would have read as a
finding.

**A sixth and seventh axis, added 2026-08-28: the control's height in points, and which clamp
left it short.** Points is what the aspect turned out to be standing in for, and it is the sharper
single reading: **every control under 2 pt failed**, 24 of 24 and 40 of 40 at the two densities,
against 16.1% for 2 to 6 pt and 7.4% for 6 to 12 pt. That boundary is `MIN_CONTROL_PX /
MAX_SCALE` written as the division rather than as `2.0`, so raising either constant moves the
bucket with it. Crossed with the shape it gives the comparison neither axis could make alone: at a
control of 2 to 6 pt, 517 regions inside 8:1--16:1 are 0% silent and 104 beyond 16:1 are 50%
silent, so the shape matters at a fixed control size.

⚠ **The aspect axis is a description of the corpus, not a lever — established by building the
lever and measuring it.** Padding every probe image into the 8:1--16:1 band was implemented in
`ocr_gate` (one rule, two callers, six mutations all caught), and at `--regions 40` it moved 120 of
1,469 regions out of the wide band while changing **no verdict at all**: 79 still-readable, 96
unread, 80 silent, 404 provable, identical before and after. At `--regions 12` it left the silent
count at 36 and took *shown unreadable* from 276 to 264. The change was reverted; `docs/PLAN.md`
§6 and the trap entry have why. Read the aspect rows as a property of the population, and do not
rank work off them again.

**An eighth axis, added 2026-08-28: what a higher scale ceiling would do.** For every unread
control the probe computes the scale it would have needed — `ocr_gate::scale_wanted`, unclamped
— and whether the probe image fits at it, through `ocr_gate::bytes_at` against the worker's
capacity. Both went public for this; neither is a second copy of anything. Measured: **0 of 24 and
0 of 40 would fit**, worst case asking **31.1x** against a ceiling of 8. So raising `MAX_SCALE`
moves the refusal from *the ceiling could not reach it* to *probe image will not fit* and changes
nothing, which is why it was not written. Rendering the control alone at a generous scale does fit
and is unsound — a control read in its own kindly rendered image says nothing about the region
strip.

That measurement is what `NotVerifiedCause::ControlTooSmall` came out of: those regions now refuse
with *no scale renders the control legibly* and a message carrying what the page removed, the scale
it would have taken and the ceiling. **No region's outcome changes** — at `--regions 12`, *control
not read back* goes 66 to 42 with 24 stated, and *shown unreadable* stays at 276 — and the
evidence that it costs nothing was already printed: the points axis carries its denominator, and
every region with a control under 2 pt went unread.

The clamp row answers a question `ocr_gate.rs` recorded as open — *"no measurement has separated
them"*. A sub-floor control comes from the `MAX_SCALE` ceiling being unable to reach 16 px, or
from the image being halved to fit the buffer, and the two can hold together, so *both* is its own
row rather than an arm of an ordered chain. Measured: **24 and 40 from the ceiling, 0 from the
halving, 0 short for neither reason.** The `MIN_SCALE` clamp has never fired on real input; re-run
this if `capacity` or the region sampling changes.

**A fifth axis, added 2026-08-28: the two above, crossed.** The height row and the shape row are
marginals of one population, so equal counts on them are not evidence of one set of regions. The
crossing prints `silent / all` per cell, and populated cells only — an unpopulated cell is not a
zero rate, it is no measurement, and printing it as `0.0%` reads as the former. It answered the
question the marginals could not: at `--regions 12` both rows report a **12**, and the cell
carrying both properties has **no population at all**, so the overlap is 0 and the two tails are
separate defects. `docs/PLAN.md` §6 has the table.

Every row above is guarded, and the guards print nothing when they agree — deliberately not counted here, because a total in prose has nothing asserting it and the two loops each fire per bucket. The three shapes plus the no-evidence count must equal the
cause's own total; the cross-tabulated total must equal the height-bucket total, since both count
the regions that had a measurable control; and the crossing must reproduce **each** of the two
rows it was derived from, checked per axis rather than over the total. The per-axis split is what
makes a failure readable — keying the crossing on a constant aspect fires the shape control and
leaves the height control silent, and a constant height does the mirror, so the `[WARN]` names
which axis drifted. A single check over the total goes red for both and names neither. The points
crossing gets the same treatment — one loop per axis against that axis's own row — and the clamp
rows have to come to the same total as the height rows, since they partition the same regions. A
non-zero *carried no evidence* is a defect in `ocr::adjudicate` rather than a finding about the
gate: the type says that arm always records one.

⚠ **`--regions N` is not only the sample size; it changes what the gate can do, so every
percentage from this harness has to be quoted with its density.** The regions set `size_pt`
— the height of the smallest box any of them covers — and they consume the pool of
surviving words a control may come from, so sampling more of them makes `control_from_page`
harder to satisfy.

⚠ **That also moves which pages reach the shape axis at all, so aspect-band populations are
comparable within a run and not across runs.** A page whose control cannot be chosen contributes
no regions to any denominator here. Between `--regions 12` and `--regions 40`, *no surviving word
is long enough* goes from 60 to 594 and the squarest aspect band empties completely — which is
the opposite direction from more regions producing a taller image, and is not the capacity rule,
since *probe image will not fit* is 0 in both. Measured over the same 40 documents and the same
three pages each:

| `--regions` | gate regions | shown unreadable | not verified | control not read back | control-selection causes |
|---|---|---|---|---|---|
| 1 | 43 | 69.8% | 25.6% | 10 | 1 |
| 4 | 156 | 67.3% | 26.3% | 33 | 8 |
| 12 | 448 | 56.7% | 38.0% | 64 | 106 |
| 40 | 1,389 | 27.1% | 67.2% | 84 | 850 |

Measured 2026-08-28, after `geometry_for` began choosing the render scale from the control
word's own height rather than from the smallest box a region covered. The four rows before that
change read 65.1 / 64.1 / 55.8 / 26.4 in the third column.

A reader marks a name or a line. **Use `--regions 4` for a figure about the gate and
`--regions 40` for a stress of the control rule**, and say which in the sentence that quotes
it. The *still reads as text* rate is 4.7--6.4% at every density and is the one figure here
that does travel.

⚠ **The `--regions 12` row used to be quoted as reproducing `docs/PLAN.md` §6 to the digit, and
that is the wrong half of the row to make a control out of.** A change to the *gate* is supposed
to move the verdict columns, and this one did. What reproduces exactly across a gate change is
the **left** of the table — the region counts (43 / 156 / 448 / 1,389) and the
control-selection causes (1 / 8 / 106 / 850), neither of which any scale can touch. Those are
the control over the harness; the verdict columns are the measurement.

### `sanitize-rewrite`: does a collected rewrite sanitize without losing the document

The instrument `docs/PLAN.md` Phase 3 states its exit criterion in terms of. It runs six
routes over `testdata/hostile-*.pdf` — a byte copy and a plain `lopdf` round trip as
controls, then `lopdf`'s own collection, our mark-and-sweep, `qpdf`, and `qpdf` with object
streams — and checks each against `hostile-manifest.json`, which says per needle whether a
rewrite is supposed to remove it, keep it, or be unable to decide.

```
python3 testdata/make_hostile_pdf.py testdata
cargo run --manifest-path src-tauri/Cargo.toml --example sanitize-rewrite -- --strict
```

**`--strict` is not decoration, and the harness had none until 2026-09-02.** Without it every
run exits 0: a leak, a dropped carrier, a fixture that does not contain its own needles, all
printed and none of them fatal. A criterion nothing can refuse is not a criterion. What
`--strict` asserts, and each part was proved able to fire by a control manifest rather than
reasoned about:

| assertion | control that makes it fail |
|---|---|
| no **collecting** route keeps a needle marked `removed` | declare a surviving carrier `removed` — 4 failures |
| no collecting route drops one marked `survives` | unlink the nested chain from the page — 8 failures |
| a fixture carrying `unverifiable` or `needs-ocr` never reports clean | declare a clean fixture `unverifiable` — 4 failures |
| the **non-collecting** routes leak something | a manifest of only-surviving fixtures — 1 failure |
| a run covered at least one fixture | `--only no-such-fixture` — 1 failure |

The fourth is the one worth understanding, because it is what makes the first three mean
anything. Every collecting route reporting nothing is the same output whether the sweep works
or the corpus hides nothing at all, and only `copy` and `lopdf` — which are supposed to leak
— can tell those apart. On the real corpus they leak 14 carriers.

**It is not a `scripts/gates.py` gate and cannot be one**, because that list has to pass on a
fresh checkout where `testdata/` is empty and `hostile-manifest.json` therefore does not exist.
A gate that refuses on a precondition of running is red on every machine that is not running.

**It is a CI step on both legs since 2026-09-02**, which is a different thing and was worth the
one install line it costs. The paragraph here said *"not a gate and cannot be one"* for about
an hour, and the reason it gave — qpdf is on neither runner — was a fact about the runners
rather than about the work: both workflows install it now, `ci_fixtures.py --hostile` builds the
corpus, and the strict run follows. Homebrew on macOS, the project's own pinned release zip on
Windows, which is the asymmetry the prerequisites table at the top of this file already
describes for a person.

Run it by hand as well after anything that touches `sweep.rs`, `verify.rs` or the rewrite —
CI answers on push, and the loop while you are working is the instrument that owns the change.

Last run 2026-09-02 on macOS arm64: **15 fixtures, exit 0**, no collecting route leaking or
dropping, every unreadable file refusing certification, controls leaking 14.

### `encrypted-rewrite-probe`: does a rewrite keep an encrypted document's encryption

`docs/PLAN.md` §5 said for months that letting a reader delete a page from an encrypted
document needed QPDF. It needed `lopdf::Document::encrypt`, which `save.rs`'s `rewrite` now
calls with the state `checked` took off the document after a password load.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example encrypted-rewrite-probe
```

Seven checks over the two encrypted fixtures and the locked case; about a second. **The
verdict comes from `qpdf`, not from `lopdf`** — a reload with the writer's own reader is the
writer agreeing with itself, and here that is worse than usual, because a `lopdf` load
*without* the password parses no objects at all and reports zero pages. The spike this grew
from round-tripped an empty document and printed `[OK]` three times before that was caught, so
every page count here is read back with the password and the encryption comparison is
`qpdf --show-encryption` on the source against the output.

Without `qpdf` installed the three encryption checks `[SKIP]` with that reason and the page
counts still run. The last check is the control and is the one to read first: a rewrite that
dropped the encryption passes both of the others, so the probe scans the written bytes for
`/Encrypt`.

### `redact-gate-probe`: does the redaction gate certify a clean file and refuse a dirty one

`docs/PLAN.md` §6 step 4 is wired into `redact_copy` and `redact_document`, and neither is
reachable from a unit test — they are Tauri commands, and the join between one and
`ocr_gate::run` is the layer `docs/TRAPS.md` records as *a feature can be inert in the
application while three layers of tests pass*. This drives the real function against a real
render service, a real render worker, a real OCR worker and a real engine. The binary is both
workers, the way `ocr-worker-probe` and `pool-bench` are.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-gate-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: this runs on both platforms, so the default joins
`PDFIUM_SUBDIR` — `bin` on Windows, where `lib` exists, holds the *import* library and binds
to nothing.

**It runs on both CI legs since 2026-08-29, and it had to.** Until then it was a thing a human
ran by hand on a Mac — and it is the only instrument on either platform that drives the OCR
gate end to end through a worker process. The `child_main_if_asked` dispatch it needs was
widened to Windows in `lib.rs` alone, so on Windows this probe scored **5/8**: the child found
no marker, fell through into the *parent's* argument parser and exited, and every region came
back `the engine crashed`. Nothing in that sentence is about the engine. The step uses the
debug artifact the `bins` gate already built — **33.6 s on macOS against 1 s for the release
build**, which is the debug PDFium render and is the price of not building twice.

It is deliberately **not** a `scripts/gates.py` gate: the gate list has to pass on a fresh
checkout, where `testdata/` is empty because the fixtures are generated and gitignored. This
needs a real document, so it belongs after the step that writes one.

| fixture | result |
|---|---|
| `columns`, `text-base14`, `text-marked`, `rotated`, `links`, `text-cid`, `outline-simple` | 8/8 |
| `encodings` | 0/0, 1 skipped — one text object is every word on the page, so no control survives |

**`columns` ran 0/0 until 2026-08-27 and it is the fixture that matters most.** Its longest
word is `alpha`, five characters, and the target filter was six — so the one corpus that
puts a *second* text object on the region's own rows was the one this skipped. Every other
fixture draws a line as a single text object, so redacting a word in it takes the whole line
and there is no neighbour left to misread. Lowering the floor to five moves no other corpus,
because the choice is the longest word on the page.

Removing the `ocr_gate::mask_columns` call turns `columns.pdf` red on two checks — *the
redacted file is certified* and *a word beside the region on its own rows is not reported*
— and no other corpus on any. That is the control for the mask, and it is the only fixture
where the right rule and the wrong rule disagree.

**The control is the same gate run against the file that was not redacted.** A gate that
certifies everything passes *the redacted file has no reasons* perfectly, so that row on its
own is worth nothing; the source file, with the same regions and the same words, has to come
back **legible** and has to quote the word that is still there. One variable between the two
runs — which file — and it is the one under test.

The other three rows: a page the gate knows no words for must be *not verified* rather than
clean, since a page nothing was read on is also a page nothing survived on; the region's own
pixels must differ either side of the write; and on a platform with no engine the gate must
say so **once**, not once per region.

**That pixel row was a byte scan first and it was the wrong instrument.** `verify::scan` for
the removed words goes red on `text-marked.pdf`, where the same line appears four times and one
copy is an annotation the removal is right to keep — see the trap of that name. A gate about
a region is checked with an instrument about a region.

**Costs, measured on this machine at scale 2.** The gate renders strips rather than pages, and
these are why:

| what | cost |
|---|---|
| `OcrWorker::spawn`, once per save | 1.5 ms |
| one 1190 x 128 probe image through Vision | ~9 ms |
| a whole A4 page through Vision, for comparison | 195 ms |
| a whole page render, warm | 13--48 ms |
| the probe's end-to-end gate run, one region, open included | 200--260 ms |

### `latency-bench`: what one tile costs, decomposed

The last thing `worker-bench` measured that nothing else did. It is a **spike, not a port**:
`worker-bench` carries its own POSIX worker, `dup2` handover, socket pair and SBPL bisection and
cannot run off unix, so this drives the **production** `Worker` instead — which means it runs on
both platforms. **Measured on Windows 2026-07-30 and on macOS 2026-07-31**, and the point of it
being portable is that macOS can cross-check it against `worker-bench --mode latency`, an
implementation it shares no worker code with. That cross-check has now run, and it is the most
useful thing this harness has produced — see below.

```
cargo build --release --example latency-bench
./src-tauri/target/release/examples/latency-bench.exe testdata/text-base14.pdf
./src-tauri/target/release/examples/latency-bench.exe testdata/vector-heavy.pdf
```

Four variants, interleaved within each round, round 0 discarded as warm-up and printed anyway:
`inproc` (no boundary), `raw` (`Tile { png: false }`), `png` (`Tile { png: true }`), and
`control` (`Outline`, a round trip carrying no tile). Each row decomposes into render, encode,
parent fold and transport; every pixel-bearing variant folds its whole payload in the parent so
none can look cheap by never reading what it received. It ends with a `N/M checks passed`
summary and exits non-zero on a failure, so a scripted run can see one; a `[WARN]` does not fail
the run, because every warning here says a *derived* figure is untrustworthy rather than that the
measurement broke.

**There is no `pipe` row, and that is a finding.** `worker-bench` compares pixels down the pipe
against pixels through shared memory. Production never does the first — `Response` documents
that payloads travel through the mapping and never inline — so a pipe row would measure a route
no tile takes. The same quantity is recovered by differencing `raw` against `png`, two paths that
are both real.

Measured 2026-07-31, Windows, 1024² tile at scale 1:

| fixture | boundary cost | spread over rounds | round trip, no tile | per 100 KB moved |
|---|---|---|---|---|
| `text-base14.pdf` | 0.269 ms | 0.004 ms | 0.040 ms | 0.0055 ms |
| `outline-simple.pdf` | 0.309 ms | 0.016 ms | 0.070 ms | 0.0069 ms |
| `vector-heavy.pdf` | 0.294 ms | 0.150 ms | 0.052 ms | `[SKIP]` — see below |

And on macOS, 2026-07-31, same tile and scale, three interleaved passes per fixture rather than
one (the fixtures were run round-robin, not in blocks, because wall clock on these Macs drifts
several percent over minutes):

| fixture | boundary cost, 3 passes | within-run spread | round trip, no tile | inproc residual |
|---|---|---|---|---|
| `text-base14.pdf` | 0.103 / 0.079 / 0.071 ms | 0.048 / 0.024 / 0.002 ms | 0.012 ms | 0.001 ms |
| `outline-simple.pdf` | 0.100 / 0.085 / 0.079 ms | 0.003 / 0.007 / 0.010 ms | 0.022 ms | 0.001 ms |
| `vector-heavy.pdf` | 0.150 / 0.200 / 0.194 ms | 0.125 / 0.145 / 0.142 ms | 0.019 ms | 0.002 ms |

Expected shape reproduced exactly: 3/3, 3/3, and 3/4 with 1 skipped on `vector-heavy`, exit 0
throughout, and its `[SKIP]` for payload differencing appears for the documented reason — png
4027 KB against raw's 4096 KB, so the two variants move nearly the same bytes. That is a property
of the document and it held on both platforms.

**Read the invariance, not the numbers.** The boundary cost is a property of the boundary, so it
should not depend on the document, and across three fixtures that differ by three orders of
magnitude in render time it lands within 0.02 ms of itself. That agreement is the result; any one
of those figures alone would be a single sample.

**The invariance is looser on macOS, and the looseness is confined to one fixture.** The two
light fixtures agree tightly across six runs — 0.071 to 0.103 ms, overlapping completely — while
`vector-heavy` sits clear of both at 0.150 to 0.200 ms. Absolute spread across fixtures is
0.137 ms here against 0.040 ms on Windows. Before reading that as a defect, note that
`vector-heavy`'s *own* within-run spread is 0.125--0.145 ms, i.e. as large as its offset from the
others: it is the one fixture where the estimator is near the edge of what it can resolve, which
is exactly what the spread column exists to say. The check is `spread < boundary`, and there it
passes at 0.73--0.83 of its limit against Windows' 0.51 — so a macOS run of `vector-heavy` is
the plausible place for this to go red first. Three passes did not. Worth knowing rather than
worth acting on.

The absolutes are **~3.5x lower than Windows**, not the 1.5--1.8x the other render constants
differ by. A latency budget written from the Windows figures is conservative on a Mac by more
than the usual factor.

**The cross-check against `worker-bench --mode latency`.** Both harnesses were run on this
machine in one session, and both figures below use the *same* estimator — the tile variant's
transport column minus `inproc`'s, `inproc` being the variant that renders but crosses nothing:

| | `worker-bench` (private POSIX worker) | `latency-bench` (production `Worker`) |
|---|---|---|
| `text-base14` | 0.006, 0.008 ms | 0.071--0.103 ms |
| `outline-simple` | 0.008 ms | 0.079--0.100 ms |
| `vector-heavy` | **-0.087 ms** | 0.150--0.200 ms |
| in-process residual | 0.013--0.037 ms, and **46.7 ms** on `vector-heavy` | 0.001--0.002 ms |

Two conclusions, and the second is why the cross-check was worth doing:

- **The production worker's per-tile boundary cost is roughly 10x the prototype's** — ~0.08 ms
  against ~0.007 ms, non-overlapping across nine runs. Both are far below anything that matters
  (the same tile costs 3.0 ms to hand to the webview), so nothing architectural moves, but the
  production protocol is not free the way the spike suggested.
- **`worker-bench`'s latency mode cannot resolve its own answer**, and now says so. Its
  `transport` is a residual and it baselines on `ping`, which never renders, so the render-noise
  floor stays in the figure; on `vector-heavy` the residual is 46.7 ms against a printed 46.6 ms
  and the `inproc`-baselined value goes *negative*. It now prints the residual and the
  `inproc`-baselined figure beside the two `ping`-baselined ones and warns when the error is as
  large as the answer — which is on **every fixture measured so far**. Read its two headline
  transport figures as upper bounds. Trap: *"A baseline that skips the expensive step leaves its
  noise in the answer"*.

**No sandbox font substitution on macOS.** The handover flagged this as the second reason a Mac
run was worth more than a re-run, since a sandboxed PDFium has previously substituted fonts
silently while still returning `ok`. It did not happen: `inproc` and the worker agree on render
time to within 0.25% on all three fixtures (0.130 vs 0.133 ms, 0.523 vs 0.507 ms, 1670.5 vs
1666.3 ms).

Its four mutations were re-proved here rather than taken on trust — **4/4 caught**, control
green on all three fixtures first, file restored by bytes and verified by digest against `HEAD`.
Two are caught as `[WARN]` rather than `[FAIL]`, which is by design and worth knowing before
writing a harness around it: a parser that treats `passed = total - skipped` as the failure count
reports those two as broken runs. `passed = checks - failures - skipped - warnings`.

Two things the A0 fixture forced, both of which the small ones hid, and both now traps:

- **The boundary figure is differenced on the transport column, not on end-to-end.** The obvious
  estimator subtracts two ~2.7 s numbers to recover a ~0.3 ms one and reports render noise: it
  read **-265.822 ms** there. The run reports how far the same render varies between variants
  beside the figure, which is the error that estimator would have carried — on the A0 sheet a
  factor of several hundred.
- **Payload differencing is guarded by materiality, not by ordering.** A dense vector page barely
  compresses — png 4027 KB against raw 4096 — so `raw > png` passes on a 68 KB gap and divides
  noise by it. That fixture now reports `[SKIP]` naming both sizes.

The `control` variant subtracts the outline walk the reply reports in `walk_ms`, rather than
warning that the walk is inside the number. Whether that subtraction is sound is cross-checked
two ways — the entry count and the walk time must agree about whether any work happened — and
both disagreement branches were shown to fire under mutation. They exist because the first
version trusted the count alone, misparsed an object as an array, and printed *"the document has
no outline"* for `outline-simple.pdf`.

**The boundary check is on reproducibility, not on sign, and the difference was forced by a
mutation that survived.** The first version simply required the figure to be positive — a
boundary cannot be free — and restoring the wall-based estimator on the A0 fixture *passed* it,
because -265.822 ms had been one sample of a noisy quantity and the next run of the same broken
arithmetic landed positive. A check that fires only when noise falls one way is decoration on
every run where it does not. It now requires the figure to be positive **and** to repeat across
rounds, which the two estimators differ in enormously: 0.004--0.150 ms of spread on the sound
one, 48 ms on the broken one.

That fix exposed a second defect worth more than the first. The check compared a spread against
a figure that was computed by a *different route*, so the mutation moved the figure and left the
spread sound, and the comparison passed on an estimator that had been broken on purpose. Both
now come from one per-round vector. **Two derivations of one quantity have to be tied together
or their agreement means nothing** — which is the same lesson as the outline count above,
arriving from the other direction.

**Its checks are proved, not assumed.** Four mutations, four caught, each with the predicted
verdict, restored by bytes and verified by digest: forcing the outline count to zero, forcing it
non-zero, dropping the fold term from the transport formula, and estimating the boundary on
end-to-end again.

### Printing on Windows, and how to check it without paper

`print_win.rs` reads a job back with `Windows.Data.Pdf` — the OS's own PDF stack, the PDFKit
counterpart — then rasterises each page onto a printer DC. Windows has no in-box PDF print
API, so rasterising is what every Windows PDF viewer does; the output is **raster at 300 dpi**
where macOS is vector.

`present` opens a modal dialog, so that last step is the one thing no automatic check can
reach — and since 2026-08-23 there is something behind it worth stating rather than leaving
implied. The panel's **Pages** field was disabled until then (`nMinPage == nMaxPage`, both zero
by default) and now offers a range that `print::sheets` and `spool` honour. The arithmetic is
tested on every platform and the probe proves the spooler sends the named sheets; **whether the
field is enabled, and whether `PD_PAGENUMS` and `nFromPage`/`nToPage` come back holding what the
reader typed, is untested by anything here and cannot be.** The first person to print a range on
Windows is the instrument for that half. `nCopies` is a known second gap of the same shape: it
goes in as 1 and is never read back. Everything before it can be, because **"Microsoft Print to PDF" is a real driver with a
real spooler**, and naming an output file in `DOCINFOW.lpszOutput` stops it raising a save
dialog:

```
cargo run --release --example print-probe
cargo run --release --example print-probe -- testdata/rotated.pdf
cargo run --release --example print-probe -- testdata/vector-multi.pdf "Microsoft Print to PDF"
```

Ten checks, and four of them are the ones worth understanding:

- **Ink, not the page count.** A wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad
  `StretchDIBits` rectangle all produce the right number of perfectly blank sheets. Proved
  rather than assumed: mutating the blit away leaves *"the printed output has the pages that
  were sent"* green and only the ink red, at `[0, 0]` — and the output file drops from 598,694
  bytes to 1,183. The pages that were *sent* are the control, because "both zero" would
  otherwise pass on a completely broken path.
- **Ink extent against a predicted geometry, not an ink ratio.** This is the check that found a
  real defect, and the history is the useful part. It began as printed-ink-over-sent-ink with an
  order-of-magnitude band, which read `0.49` and **passed** while every page was going to paper
  at half physical size (a DIB rendered at 300 dpi placed unit-for-unit onto a 600 dpi printer
  DC). The same formula then failed at `0.01` on an A0 page for no reason but the paper being
  16× smaller in area. What holds for both is predicting where the ink should land — the source
  page's ink extent scaled by the page-to-sheet ratio — which reports 1% error on the reference
  run and **48%** against the reverted bug. See the trap of the same name.
- **Its own module table**, which is where the boundary claim gets its honest caveat: 80 modules
  mapped, none named pdfium, and `Windows.Data.Pdf.dll` printed beside it as what *is* mapped.
  Printing parses in the app process on both platforms; what the boundary buys is that the
  parser doing it is not ours.
- **A page range spools the sheet it names, not the first one.** Added 2026-08-23 with the range
  itself. A count of one sheet is equally satisfied by the *first* page, which is exactly what a
  loop ignoring its range produces, so the second check compares the printed ink's extent
  against the prediction for the sheet that was asked for. It asks for the **last** sheet for
  the same reason. On a fixture whose first and last sheets measure alike it prints a `[SKIP]`
  saying so, rather than passing on a comparison that cannot fail; `rotated.pdf` is a fixture
  where they differ.

Reference run, `testdata/rotated.pdf`, pages 1--2 with a quarter turn:

```
  pages    : 4 in the source, printing [1, 2] with a quarter turn

[OK]   the job we built is readable by the OS parser        2 pages
[OK]   control: the pages we sent have ink on them          non-white pixels per page: [5012, 5012]
[OK]   the spooler accepted every page                      2 pages spooled
[OK]   the printer produced a file                          598694 bytes
[OK]   the printed output has the pages that were sent      2 pages
[OK]   every printed page has ink ...                       non-white pixels per page: [4777, 6850]
[OK]   printed ink lands where the page geometry says ...   got 0.27x0.28, 0.45x0.27,
                                                            predicted 0.27x0.27, 0.45x0.27
                                                            (worst axis off by 1%, 0%)
       ... printed/sent ink, for information                [0.953, 1.367]
[OK]   the printing process never mapped our PDF parser     80 modules mapped, none named pdfium
       ... the OS PDF component it maps instead             Windows.Data.Pdf.dll
```

`outline-hostile.pdf` is the second shape worth running — A4-sized pages rather than small ones,
so the predicted extent is 0.72x0.47 instead of 0.27x0.27 and a wrong *fit* would show where a
wrong *scale* does not. Also 8/8.

**Do not run it on `vector-heavy.pdf` or `vector-multi.pdf` casually.** One A0 page of 200,000
vector operations takes **2m51s** end to end, essentially all of it inside
`Windows.Data.Pdf`'s rasteriser and largely independent of resolution. That is a real property of
a raster print path and not a defect — see the trap *"the OS's PDF rasteriser is not fast"* —
but it makes those fixtures unsuitable for a quick check.

Beyond the probe, `cargo test --lib print::` runs **18** checks on Windows where it ran 14, because
three of the four third-parser checks are no longer macOS-only. The fourth asserts the page count
and prints a `[SKIP]`: it needs per-page *text* to say which pages survived, and
`Windows.Data.Pdf` has no text API at all. `a_third_parser_checks_a_job_built_from_a_document_we_-`
`did_not_write` covers that property on both platforms instead, using per-page rotation —
`rotated.pdf` carries 0/90/180/270, so keeping the wrong two pages is a different rotation pair.

### The "reopen its windows" dialog

A development build is killed constantly — harness timeouts, the deliberate crash probes,
an aborted panic — and macOS answers each abnormal exit by offering, on the *next* launch,
*"the last time you opened tpdf, it unexpectedly quit while reopening windows. Do you want
to try to reopen its windows again?"* That dialog **blocks the launch** until someone clicks
it, in front of a run that has nothing to do with whatever produced it.

`src-tauri/Info.plist` sets `NSQuitAlwaysKeepsWindows` to false, which is merged into the
bundle by tauri-bundler — check it with
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
webview, dispatches real wheel and key events at it, and checks fit-width, fit-page, actual
size, scrolling, End and Home, the zoom ladder, a pinch, resize, text selection and copy,
find-in-document, the
command palette, the screen-reader text layer, the outline sidebar, the page-thumbnail
strip, page inversion, and that the frame loop idles when there is nothing to do:

```
scripts/viewer_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

It is **not** a `gates.py` gate: it needs a built bundle and a generated fixture, neither of
which a gate run has. Run it before a release, and after any change to `viewer.ts`,
`scroller.ts` or the tile protocol.

**One corpus while you are working; the sweep before a push.** The rule above names *files*,
and a file is the wrong unit: `viewer.ts` is 4,400 lines and most changes to it cannot vary
by document. What `viewer_sweep.py` buys over a single run is the name-set invariant across
fourteen corpora, so the question to ask is whether the change could make a check appear,
vanish or skip on *some* documents — layout, rotation, text extraction, the tile protocol,
anything reading a page's size. A change whose checks drive the DOM and the callbacks
directly cannot, and the sweep is then fourteen runs of the same answer. This is the
portfolio rule about running the owning gate while iterating and the whole suite once, at the
push, applied to the slowest instrument here.

**It requires a bundle, not merely a release build.** A raw `cargo build` binary opens a
window and never executes a line of JavaScript — WKWebView needs the bundle identity, and
the failure is silent: no error, no crash report, a blank window. Build one with
`npm run tauri build -- --bundles app` and run the executable inside it, which keeps stdout
and the environment that `open -a` does not. The *profile* genuinely does not matter — the
check asserts behaviour rather than timing it — so a debug bundle is only slower.

**On a zero exit it now asks whether the run happened**, which it could not until 2026-08-26.
Every guard in it was aimed at a run that failed, and a run that did *nothing* exits 0: on
Windows, single instance makes a second launch forward its argv to a window already open and
exit immediately, so an empty transcript came back as a pass, with the wrapper's own
containment `[OK]` as the only check-shaped line in it. A blank-window bundle failure produces
the same silence, so the paragraph above is no longer describing a failure with no report. The
observable is the `CHECK-NAMES-JSON` roll `checkreport.ts` prints before its summary, and the
refusal names no single cause — it prints the exit code, the byte count and the number of
`[FAIL]` lines, and lists the four things that look identical from here. Both callers already
guarded themselves (`viewer_sweep.py` on the same roll, `mutate_viewer.py` on the summary),
which is why the layer they share had nothing; neither is made dead by this, since the sweep
looks for the roll before it looks at the exit code.

That guard is a pure function of the transcript, so it can be proved without a screen, a
bundle or a document — six cases, one of them the acceptance, since a reader that refuses
everything passes all five refusals:

```
scripts/viewer_check.py --self-test
```

It also requires an unlocked screen, for the reason `scroll_bench.py` does: WebKit suspends
a page whose window is not visible, so behind a lock screen the check does not fail, it
stops. Both scripts share that guard (`scripts/webview_guard.py`).

**On a timeout it says which silence it hit**, which it could not until 2026-08-01. A page
WebKit has suspended and a page stuck in a loop both present as no output and a live process,
and they want opposite responses — one is an occluded window or a screen that locked
*mid-run*, the other a defect in whatever was last changed. `webview_guard.diagnose_silence`
samples the app's CPU **time** twice, two seconds apart, before the kill: suspended uses none,
waiting uses a little, spinning uses a core. The delta is load-bearing — a single
`ps -o %cpu` is a lifetime average on macOS, so a page that worked hard and then got suspended
reads as busy. `mutate_viewer.py` gets the line for free, since it already forwards
`[FAIL]` stderr lines into its own broken-run verdict.

`session_check.py` and `open_check.py` do **not** have it: they use `subprocess.run`, so the
process is already dead when the timeout is raised, and giving them the diagnosis means
converting four call sites to `Popen`. Worth knowing rather than assuming it is everywhere.

**It does not take focus.** The window appears and has to stay visible, but it will not raise
itself over what you are doing, so the run can sit in the background while you work.
`scroll_bench.py` is the exception and calls `set_focus()` on purpose — an unfocused window
is throttled, and a frame-rate benchmark would then be measuring the throttle.

**"Visible" is stricter than "unlocked", and the guard does not check it.** A window fully
covered by another — a full-screen terminal, a different Space — is *occluded*, and
WebKit suspends the page exactly as it does behind a lock screen. The run then produces no
output, uses no CPU, and stays alive, which reads as a hang in whatever was last changed.
`viewer_sweep.py` and `mutate_viewer.py` take **`--raise`**, and it is **off by default**
as of 2026-08-20. They used to force `TPDF_RAISE=1` on every launch, which on a fourteen-
corpus sweep takes the keyboard away fourteen times in a row — reported from the machine
as *"these tests are locking up this mac, as every window opens in foreground"*, and it
was never what the checks need. `lib.rs` says so at the call site and keeps a polite
default for exactly this reason: the checks drive behaviour rather than time it, so an
unfocused window costs them nothing. What costs them everything is an **occluded** window,
which is a different property — and one `webview_guard.py` already detects and names the
remedy for. Use `--raise` when a run produces nothing, not before.

Set `TPDF_RAISE=1` to raise the window when there is nowhere visible to put one:

```
TPDF_RAISE=1 scripts/viewer_check.py <binary> testdata/text-heavy.pdf
```

**An empty transcript file mid-run means nothing, and reading it as a stall is a mistake this
page invited.** `viewer_check.py` collects the app's output with `communicate()` and prints the
whole transcript when the process exits, so a redirected run shows **zero bytes** from the
first second to the last — on `vector-multi` that is minutes. The "results print as they are
produced" sentence below is about `viewercheck.ts` writing into the pipe, which is what makes a
*timeout's* partial transcript useful; it is not a promise that a live run's log grows. The
liveness signal is CPU time (`ps -o time= -p <pid>`), which is what `diagnose_silence` samples:
a page that never ran accumulates none, and a slow render accumulates seconds.

The watchdog identifies **any** page that never executed, whatever the reason — an
occluded window and a raw unbundled binary produce exactly the same silence. Every spike
entry point starts by asking Rust for its path, which records a `webview alive` mark; a run
that times out without one is told in full that the page never ran a line of JavaScript.
Confirm independently with `TPDF_STARTUP=<file> <binary>`, which fails the same way in 30 s
and settles "environmental or mine" in one command. Results otherwise print as they are
produced, so a run that stops partway names the last check it completed.

**That was false under a redirect until 2026-07-30, which is how these are always run.** Python
block-buffers stdout the moment it is not a tty, so `open_check.py > out.txt` held **zero bytes**
for a twelve-minute run — indistinguishable from a script that died at import, and exactly the
ambiguity printing-as-you-go exists to remove. `scripts/live_output.py` makes the three harnesses
line-buffered explicitly; A/B'd at the same four-second mark, 0 bytes against 38. Prefer that over
`python -u`, which is a property of the invocation that every future caller has to remember.

Two of its assertions carry the weight, and both tie a position to specific content rather
than checking that something happened. For **selection**, text dragged near the top of the
page must come from earlier in the page's text than text dragged further down — a substring
check was tried first and cannot fail, since a selection is a contiguous range of indices
whatever the boxes claim. For **search**, a match's index range must cover the characters
searched for, re-extracted independently; every other search assertion passes just as well
when the indices are off by one.

Run them with **`scripts/viewer_sweep.py <app-exe>`**, which is the list of corpora as well as
the way to run them:

```bash
scripts/viewer_sweep.py --list          # every window corpus, and every fixture excluded, with reasons
scripts/viewer_sweep.py src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
```

> **Not yet re-measured, and the shortfall has grown.** The properties dialog added five
> names to every corpus on 2026-08-21, and the comments panel's covered-words face added a
> sixth later the same day — so the totals in the table are **six** short of what the next
> run will print, and the invariant the sweep asserts is the *agreement* between corpora
> rather than any particular number. Written down rather than left to be noticed, because the
> table looks measured either way and a stale total reads exactly like a current one.
>
> The current figure, measured 2026-08-23: **342 names**, all distinct — 324 cutting
> `26.8.7`, plus five for the overlay-against-the-file phase, five for the stamp (its own
> overlay reading and its four commands), six for cropping by dragging (three backend
> checks, its one palette command, and two reading the scrim off the overlay), and two for
> the eraser taking a mark whole (the wash the nib crossed, and the wash beside it that is
> the control). Take the names from the harness's own
> `CHECK-NAMES-JSON` line, never by splitting the printed columns — this page records that a
> `\s{2,}` split matched 175 of 189 lines, and reaching for it again is what produced a diff
> full of per-corpus *detail* differences that looked like missing names.

#### The overlay against the file, and the one thing it needs from outside

Five of those names are a phase comparing what the overlay draws with what the *saved file*
renders — the one comparison nothing made, since `viewer_check.py` measured the overlay
against the model's numbers and `annot-probe` measured the file against the same numbers. It
makes nine marks, reads the overlay, saves a copy, opens it, renders the same page and reads
that; the file's ink is isolated by diffing that render against one taken before any mark was
made, so page content cancels and the classifier knows nothing about the colour it is about to
compare. `docs/PLAN.md` has the design and what it measured.

**It needs a writable path, which the webview has none of.** `viewer_check.py` makes one under
the system temp directory, binds it to `TPDF_VIEWERCHECK_SCRATCH`, and removes it at exit; the
`viewercheck_scratch` command hands it to the page. A run that gets nothing there skips all
five with that reason rather than passing. Running the harness by hand without the script is
therefore a run with those five skipped — which is correct, and worth knowing before reading
a hand-run transcript as a full one.

**The comment is excluded from the colour comparison**, with a measured reason: PDFium draws
its own `/Text` icon and ignores the `/C` we write, so blue reads 224 degrees on screen and 60
in the file, and red reads 0 and 60. See the trap of that name — the file is right and the
renderer is not ours.

Every run reports the same check names; what differs is how many are `[SKIP]` with a reason,
and a name that goes missing rather than skipping is the bug this arrangement exists to catch.
**The script asserts that**, as a set difference across the corpora, rather than leaving it to
whoever compares two totals — a check that stopped being printed and a check that started
skipping are the same number. It also prints the table below, so those numbers are measured
rather than transcribed.

> **The list is a gate (`corpora`) because it went wrong the moment it had no home.** On
> 2026-08-16 it lived in a hand-typed shell loop and `links-rotated.pdf` went into a sweep,
> producing eight red checks and three chased diagnoses, none of them a defect — against the
> paragraph on this page that already says that fixture is separate *because* it reddens two of
> these rotation checks. Every `testdata/*.pdf` is now either a window corpus with a stated
> purpose or excluded with a stated reason, and a fixture matching neither fails the gate.

**The sweep runs on Windows as of 2026-08-19, and did not before.** Two things had to be
fixed, and both are recorded in `docs/TRAPS.md`. It shelled out to `pkill` unconditionally,
which is not a program here: `check=False` swallows a non-zero exit and not a
`FileNotFoundError`, so it died on its first corpus with a traceback and **exit 0**. And
`subprocess.run(text=True)` decodes with the locale codec, which is cp1252 on this machine
— six corpora in, `multilingual.pdf` produced a byte it refuses, the decode raised inside
subprocess's own reader thread, and the failure arrived as a `TypeError` between a `None`
and a string with no mention of an encoding.

Two further things are worth knowing before running it here. A stray `tauri dev` makes
`tauri-plugin-single-instance` forward the launch and exit, so the check reports one line
about the module scan and nothing else — kill leftovers first, which the sweep now does.
And `text-heavy.pdf` is a real document rather than a generated fixture, so a machine that
does not have it cannot run the full sweep at all; `--only` over the other thirteen is the
honest substitute, and the sweep says which corpora it is missing rather than skipping them
quietly.

**`vector-multi` is timing-variable on this machine, and a single red run there is not a
finding.** Measured 2026-08-19 across four runs of the same corpus: **351 s, 496 s, 386 s**
on one build and **384 s** on another — a 41% spread on identical code. Two of those runs
failed, and they failed **different checks** (`the page already rendered is not rendered
twice`, reporting 11 borrows against 4 draws; and `covers the first screen`, timing out at
sharp=0.0%). Both are checks whose observable is a race between the thumbnail strip and the
viewer, on the only fixture where a thumbnail is slow enough for that race to be real —
which is precisely what the corpus is *for*.

So: **re-run it before treating a red vector-multi as a regression, and check whether the
same check fails twice.** Two different checks failing is variance; one check failing
repeatedly is a defect. The control that settled it was a `git worktree` at `HEAD` built and
run against the same fixture — cheap, and it leaves the working tree untouched, which
`git stash` does not.

Measured here on 2026-08-19: thirteen corpora, **276 check names each** (277 once
`app.about` became a driven probe), diffed as sets by the sweep rather than inferred from
the totals agreeing, and **no failing check on any of them**. 629 s in total, of which `vector-multi` is 341 s and `vector-heavy` 145 s; every
other corpus is under 40 s. The ran/skipped splits are the sweep's own output and differ
from the macOS table above only by the checks added since it was taken.

**Every row below was measured on macOS on 2026-08-17**, and printed by the script rather than
transcribed — the table is the sweep's own output, pasted. Zero failures
anywhere, and **all fourteen corpora report the same 234 check names** — diffed as sets by
the sweep, not inferred from the totals agreeing.

The link work took the total from 171 to 189, turning a page in the document took it to 204,
and deleting one took it to 218: ten in the viewer, three against the backend, and one command
probe. Of the ten, the one that carries the weight is about **identity** — the slot below the
gap must now hold the page that was under it, compared by its text — because a page count one
lower is equally true of a viewer that dropped the wrong page. On a corpus whose pages read
alike that check says so and skips rather than passing on a comparison that cannot fail.

**Moving one took it to 229**: nine checks and two command probes. The nine are where the
deletion ten do not transfer, and the reason is worth stating because it is what made the
frontend defect this phase found invisible. **Every deletion check is built on the page count,
and a move does not change it.** So the length is asserted to be *exactly what it was*, and
every statement that can fail is about identity: the moved page's text is in the slot it was
moved to, the page displaced by it is one slot lower rather than gone, the reader is still
looking at the page they were reading, and both pages keep the sizes they were measured at —
that last one on `mixed.pdf`, the only corpus where two pages have different sizes, so on every
other one it says so and skips.

**That size check asserted one slot until the mutation aimed at it survived**, which is what
the harness is for. The defect it was written against is a scroller re-indexing its learned
sizes by position, and one comparison catches that. The other way to lose a size is to lose
them all, and then every page falls back to one estimate — free to land within tolerance of
whichever single shape is being compared, which on this corpus it did. Asserting both slots
makes it arithmetic instead: a shared estimate would have to be within 0.02 of two shapes the
check's own precondition has just established are further apart than that. The same mutation
reddened a *deletion* check the whole time, which reads absolute boxes rather than shapes —
so the coverage existed and the check named for it was the one that could not fail.

> **The table above is one sweep of the tree as committed**, re-run after the last change to
> any check rather than carried over from before it. That is not free — this one was **728 s**,
> of which `vector-multi` alone is 349 and `vector-heavy` 205, so two of the fourteen are
> three quarters of the run; `scripts/viewer_sweep.py` prints that breakdown at the end so the
> question does not have to be answered by hand again — and it is worth it here for a reason this increment
> demonstrated: the run before it went red on four of the fourteen corpora, against ten that
> passed for no better reason than having strips too short to scroll. A table pasted from a
> sweep of a different tree would have been a claim about neither.

**Dragging one took it to 231**, and the two names are the narrowest pair that were worth
paying a window for. The slot arithmetic is two pure functions with unit tests, the gesture's
state machine has nine mutations against a fake DOM, and the edit a drop runs is already
covered by the three backend names below. What none of those can answer is whether a real
WKWebView captures the pointer, keeps delivering moves after it has left the row, and lays out
geometry the gap arithmetic can read — so the strip's handler here *records* rather than
edits, and the document is never touched. The control is the half that found something: its
first version could not fail either way it was read, and the mutation aimed at it survived
until both clauses were replaced. See the trap of that name.

**Extract took it to 232**, and one name is all it is worth: `file.extractPages runs from the
palette`, driven with a real argument. The arithmetic is 30 unit tests over `parsePageRange`
and `namePages`, the subset plan is ten Rust tests, and the command is four more — none of
which can say whether a typed value survives the palette's own input and arrives as the slots
the action is handed, which is the only thing the window is asked. The argument is `1-2`
rather than `1` on purpose: a single slot reads the same whether the parser produced one page
or dropped one, and every window corpus has at least two pages.

**The right-click menu took it to 234**, and both names are about the same gesture because the
report that asked for them was: right-clicking a page offered the web view's own menu, whose
one entry reloads the frontend. So one name asserts that menu is *suppressed* — read off
`defaultPrevented` on a real `contextmenu` event rather than inferred from a screenshot — and
the other that the slot under the pointer is the one reported. The menu's own behaviour is 18
unit tests over the real class in a fake DOM; what only a window can say is whether a
right-click on a row arrives at all. Three ways of posting a secondary click from *outside* the
process were tried first and every one of them failed silently; `docs/TRAPS.md` has them.

Putting the order back must restore the document exactly, which
is the check a viewer that reordered its own view but not its model passes and a viewer that
lost a page fails. Two of the eleven drive `page_move` for real, including the refusal of a
page moved behind itself, and one is undo.

The three that ask the backend are the `page_delete` round trip: the command is registered,
names a page by identity, refuses a second deletion of that id as *deleted* rather than as
unknown, and undo puts the page back. They leave the model as they found it, asserted rather
than assumed, because every phase after them reads the document.

Of the page-turn ten, three are negative — a page nobody turned keeps its proportions and its
upright text, and `viewer.rotation` does not move — because every positive statement about a
turned page is equally true of a view that rotated everything. The tenth is the half turn, and
it exists because a mutation deleting the invalidation that runs *before* the geometry survived
all nine others: a quarter turn changes the page box, so `applySizes` invalidates it either
way, and only 180 degrees leaves the box identical.

The Windows column is *not* carried forward: it was measured at 163 names on 2026-08-02 and
nothing has re-run there since, so it is absent rather than adjusted — which is what this
page has twice recorded arithmetic in a measurement column costing.

**Two rows are new**, and both earned their place the day they were added. `links.pdf` caught a
destination landing on the page before the one it named — the only corpus that could, being
the only fixture in the tree with a `/Fit` entry. `links-cropped.pdf` caught two checks whose
control could not be established on a document with a single link, because the check before
them follows it.

**Re-run 2026-08-24 on Windows**, with `inherited.pdf` promoted from an exclusion to a corpus:
**343** names, no failing check anywhere, 731 s. `text-heavy.pdf` is not on this machine —
no script writes it, it is a real document supplied by hand — so the run covered **14 of the
15** and its row below is the earlier macOS reading, marked as such. The sweep refuses a
missing fixture outright rather than skipping it, which is why the run had to name the other
fourteen.

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 292 | 50 | *(2026-08-23, macOS — not on the machine that ran the rest)* the dense case, and search across 775 pages |
| `outline-simple.pdf` | 299 | 44 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 299 | 44 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 198 | 145 | one page, no extractable text, and no white paper to invert |
| `vector-multi.pdf` | 238 | 105 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 278 | 65 | every page at `/Rotate 90`, which nothing else in the corpus has |
| `columns.pdf` | 288 | 55 | the only one whose content-stream order is not its reading order |
| `tagged.pdf` | 263 | 80 | the only one carrying a `/StructTreeRoot`, and the only two-page one |
| `multilingual.pdf` | 280 | 63 | the only one whose text is not Latin: CJK with no word separators, Arabic right-to-left, a decomposed accent, and a code point above the BMP |
| `encodings.pdf` | 281 | 62 | the only one whose character mappings are absent, broken or predefined — and the only fixture that reaches the replacement-character path at all |
| `mixed.pdf` | 282 | 61 | the only one whose pages are not all the same size, and the only one that exercises the three layout checks at all |
| `comments.pdf` | 306 | 37 | the only one carrying annotations: notes, a reply, a highlight, three text-string encodings, an indirect `/Annots` array and 1,200 marks on one page — the only corpus where all eight comment checks run |
| `links.pdf` | 308 | 35 | the only one with link annotations, and the only one whose outline is deliberately not in page order — which is what let it catch a destination landing on the page before the one it named |
| `links-cropped.pdf` | 245 | 98 | the only one whose `/CropBox` is not its `/MediaBox`, so a rectangle placed in media space lands visibly wrong |
| `inherited.pdf` | 272 | 71 | the only one whose pages take their `/MediaBox` from an ancestor while carrying a quarter turn, and the shortest displayed page in the corpus at 400 points |

**The 2026-08-23 sweep read 342 names on macOS, and the difference is not one number.** The
one new name is `file.mergeDocuments runs from the palette`, checked against the run's own
name list rather than inferred from the total. Every row's ran/skipped split also moved by one
or two, and **that is not attributed here**: this run is a different platform *and* four
commits later, so a per-row difference between the two is two variables at once — the trap of
that name. The invariant the sweep asserts is none of these totals, it is that all fourteen
agree on the *names*, and both runs satisfy it.

The 2026-08-23 run for the record: **342** names on fourteen (the same list with `inherited`
excluded), 721 s, no failing check anywhere, and `vector-multi` and `vector-heavy` 73% of the
time between them. Five of those names are the overlay-against-the-file phase, six are
cropping by dragging, and two are the eraser taking a mark whole.

**Re-run the same day** after the eraser and the crop drag: **329 -> 342**, every corpus
gaining thirteen *runs* and losing none. Two sweeps in one day are worth one note — the
totals move whenever a check is added, so a row here is a statement about the run that
produced it, and the invariant the sweep asserts is that all fourteen agree.

**Re-run 2026-08-18** with the crop: **267** names on all fourteen, seven more than the 260
below. Every corpus gained seven *runs* and lost none.
All seven are one backend phase, driving `page_content_box`, `page_geometry` and `page_crop`
against the real backend — which is the only place that can say the three commands are
*registered*, the failure every layer below passes through. Its last check is the control: a
crop whose corners are the wrong way round has to come back as the refusal the model names.

The two palette commands are deliberately **not** driven from the palette, and the reason is
in `viewercheck.ts` beside them: cropping to content is two IPC replies deep — measure the
ink, then ask what size the page becomes — and the probe framework's settle is a frame-loop
wait rather than a reply wait. Their wiring is covered by `appcommands.test.ts`'s sweep over
every registered command.

**Re-run 2026-08-18** with the keyboard route to a mark: **260** names on all fourteen,
six more than the 254 below, and the rows above are that sweep's. Every corpus gained six
*runs* and lost none — the four for the walk and the guard need no selection and no corpus
feature, since the marks are two the harness hands the viewer, and so do the two commands in
the sweep every registered command gets. The two command probes are the only pair in that
table with no `unless`: no fixture carries a mark, because these are the reader's own.

Four of the six are in a real webview for a reason a unit test cannot cover. The guard that
stops a key typed into a note from moving the page is about a key **bubbling** from the field
to the root handler; vitest dispatches at the root with a target of its own choosing, which is
a statement about the handler rather than about the tree it is installed in. Its control is in
the same check — the same key pressed on the page must still scroll — because a guard
tested only on its refusal is satisfied by a viewer that ignores everything.

**Re-measured 2026-08-18** with the note on a mark, on all fourteen corpora: every one
reports the same **254** names, and the rows above are that sweep's, pasted from what
`viewer_sweep.py` prints. Fourteen names on top of the 237 the morning's highlight work
left: **nine** for the note box, **four** for the three mark commands driven against the real
backend, and **one** for `edit.removeMark` in the sweep every registered command gets.

**Re-run 2026-08-18** with underline and strike out: **254** names on all fourteen, three
more than the 251 below. Two are the new commands in the sweep that every registered command
gets — aimed separately, carrying the kind in the expectation, since one action taking a
parameter is where a copy-and-paste gives a reader a Strike out that highlights. The third
drives each kind through `annot_mark` against the real backend and reads the kind back off
the state reply. Every corpus gained three *runs*: they need no selection and no corpus
feature, because the mark is one the harness hands the viewer.

**Re-run earlier the same day** after the page-turn placement fix, and every one of the
twenty-eight numbers then came back **byte-identical**, diffed rather than eyeballed. That is the honest
result and it is worth stating rather than quietly re-pasting the same table: the defect it
fixed — a comment or a link on a page an edit had turned, drawn in one place and found in
another — needs a page turn and an annotation *at the same time*, and no fixture in this
corpus has both. The window harness could not have caught it and still cannot. What it does
cover is the primitive underneath: a mutation that turns every rectangle a quarter too far
reddens three of the mark phase's checks, so the one implementation those three subsystems
now share is reached from here. The measurement that found the defect is a differential in
`viewerturns.test.ts`; see `docs/PLAN.md`.

**Every corpus gained runs and no skips**, which is the difference from the highlight
increment and is deliberate. Those checks needed a selection, so the two fixtures with no
extractable text skipped them; these are driven against a mark the harness hands the viewer
itself. The model is tested in `docmodel.rs`, the file in `annot-probe`, and this phase tests
what neither can reach — a rectangle on screen, a press landing on it, and the box that
opens — for which a synthetic rectangle is the right input and runs everywhere.

That earlier sweep is worth keeping for the shape of its failure: the first version of the
quad checks left them out of the no-text path entirely, so two corpora reported 235 names
against everything else's 237 — a name that had *vanished* rather than one that skipped,
which is exactly what the identical-name-sets rule is for.

**`tagged.pdf` runs three of these thirteen and skips ten**, which is the split worth knowing:
the ten that drive the viewer need a middle page to delete and it has two, while the three that
ask the backend need only a page to spare. They are the only checks of this phase that run
there, and skipping them along with everything else would have been a skip for a reason that is
not theirs.

**`vector-multi` is the one with no margin, and the sweep's timeout was raised for it.** It
takes **325 s** of the default 900 (420 until 2026-08-17, which the page-deletion phase went
past before that phase was cut from two delete-and-restore cycles to one). Every other corpus
is minutes. A tight timeout is worse than a generous one here: it fails as *"the run printed no
CHECK-NAMES-JSON line"*, which reads as a crash rather than as a bound.

**A `pkill -f "tpdf.app/Contents/MacOS/tpdf"` goes between runs**, and
`scripts/viewer_sweep.py` does it. A leftover window occludes the next one, WebKit suspends an
occluded page, and the run then produces nothing and uses no CPU — twice, before that went
into the sweep script. `TPDF_RAISE=1` covers the other half, a window with nowhere visible to
go, and the script sets it.

**`text-heavy.pdf` moved by one between two sweeps an hour apart** — 177/41 and 176/42, the
same 218 names — which is the race the notes below describe, not a regression. It is left as
the second reading rather than the flattering one. Those are the totals of that day, when the
name set was 218; the row in the table is a later sweep against a larger set, so read the
*one* it moved by, not the absolute figures.

**Every row above is one run, and the notes below say why that is a point estimate rather than
a bound.** Two of these fixtures have a check that lands on either side of a race, so their
split moves between runs while the name total does not. The table is not re-ranged here
because a range needs several runs per fixture and this sweep was one each — read a
disagreement of one as the documented variation, and a disagreement in the *name total* as the
bug this arrangement exists to catch.

The `text-heavy.pdf` row was `142 / 21` and marked derived until 2026-08-03, when running it
on the only machine that has the document made it `143 / 20`. The derivation was one check
out, which is the cost of carrying arithmetic in a column of measurements: it looks exactly
like the rows either side of it.

**Then the measurement replaced it with a point, and the true value is a range** — two runs
the same evening against the release bundle both reported `142 / 21`. Nothing regressed: the
one check that moves is the withdrawal race described below, and this row and
`outline-simple`'s are the two the note there says land on both sides. So the correction
above fixed the arithmetic and reintroduced the shape of the original error, which is worth
saying plainly: **a number measured once is still a point estimate of a quantity that varies**,
and for these two fixtures the invariant is the name total, not the split.

**`outline-simple`'s range was widened to `147--149 / 14--16` on 2026-08-08**, on six Windows
runs: three at `149 / 14`, then two at `148 / 15` and one at `149 / 14`. The total is 163 in
all six, which is the invariant the paragraph above says it is — and the widening is the
same lesson a third time, since `147--148` was itself two platforms' point estimates read as
a bound. Do not narrow it back on the strength of one green run. The three runs that came
first also carried the stale-focus-mirror failure described in `docs/TRAPS.md`, so treat any
row of this table as a statement about the split, never about whether the run passed.

**The two A0 corpora straddle the watchdog's old 300 s default, and their spread is far wider
than the bound.** Four runs on macOS on 2026-08-03: `vector-multi.pdf` at **275 s**, **387 s**
and once still going past **600 s**, and `vector-heavy.pdf` at **249 s** having been killed at
300 s on the run before. So the bound was a coin flip rather than a consistent failure, which
is why it survived — a corpus that fails every time gets fixed, and one that fails half the
time gets re-run. Do not read any single figure here as the cost; the spread is the
measurement, and it is roughly 2.2x on one document. Both are the fit-page setup on an A0
page, which is the operation
that defeats spatial culling: the whole page becomes visible, and PDFium charges its large
fixed cost per render call. The two bounds are one number now — `viewer_check.py` derives
`TPDF_VIEWERCHECK_TIMEOUT` from its own `--timeout` — so raising the one people edit raises
the one that decides. Before that they disagreed, and the app's was the tighter.

**Every measured row is its previous split with exactly three more skips**, which is a
stronger statement than a matching total: the three layout checks added on 2026-08-02 skip
on every fixture but `mixed.pdf`, for the stated reason that the fixture has no geometry
sidecar, and nothing else moved. `mixed.pdf` is the eleventh row and the only one where they
run.

The earlier note that eight of nine rows were the macOS split plus arithmetic still holds
underneath this one, and so does the exception: `multilingual.pdf`'s generator picks a font
per page from what the machine has, so the Windows fixture is a different document and one
check that skipped on macOS for want of text has text to work on here. A ran/skip split is a
property of the document, and that corpus is the only one whose document is not the same on
both platforms.

**Every measured row above is green as of 2026-08-02, and getting there took two rules rather
than one.** The check involved is `a page reads in the order its generator laid it out`, which
compares each page against what the generator *wrote*. It is downstream of `text.rs`,
`reading.ts` and the fixture's machine-local fonts and of nothing in the layout —
`readingChecks` builds its own `TextCache` and never touches the viewer or the scroller — so
it is the check that sees a font substitution, and both failures below were one.

**`multilingual.pdf` was red for a missing space, and that is fixed.** The folding page came
back `cafélatte` where the manifest says `café latte`. PDFium's extraction *does* contain the
space — `text-probe --mode order` shows `café`, a space run, then `latte` — so it was
dropped between extraction and the line's *ranges*. Measured through `FPDFText_GetCharBox`
against the vendored library: the space at index 4 comes back **placed**, 0.02 pt tall at
y 752.00--752.02, while every letter on the line sits at 752.14--766.08, the two bands missing
each other by 0.12 pt. `reading.ts` refuses a box that thin and re-attaches it by preceding
index. The page reads `café latte` here as of the run above.

**Fixing it broke `encodings.pdf`, and only running the whole corpus found that.** The rule as
it first landed was absolute — under `SLIVER_PT`, a tenth of a point — which is a claim
about glyphs and turns out to be a claim about *metrics*. Page 2 of `encodings.pdf` is set in a
predefined CMap with no embedded font, so PDFium has no metrics for it and reports **every**
character 0.018 pt tall. All of them were refused, nothing was placed, and the page came back
as a single fragment: its two lines, 632 pt apart, read as one. Established by reverting
`reading.ts` alone and rebuilding, which gives exactly complementary results:

| | `multilingual` | `encodings` |
|---|---|---|
| absolute rule absent | **129/130** `cafélatte` | 130/130 |
| absolute rule present | 130/130 `café latte` | **129/130** `日本語の符号\r\n日本語の符号` |

**The rule is a conjunction now**: `height < SLIVER_PT && height < SLIVER_OF_LINE * typical`,
where `typical` is the median height of the page's placed characters and `SLIVER_OF_LINE` is a
twentieth. The two measured samples are three orders of magnitude apart on the relative
quantity and adjacent on the absolute one — 0.02 pt against 13.94 pt letters is 0.0014 of
them, 0.018 pt against a page median of 0.018 is 1.0 of it — and `tagged.pdf`'s comma, at a
third of its letters, is well clear of both and stays `SHORT_MARK`'s business. Each half was
proved by a mutation turning exactly one test red; the median was proved against a maximum,
which survived the whole suite until a control was written for it. See the traps for the full
account.

**What this cost, and the discipline that paid for it:** the fix was verified on the corpus it
was written for and on macOS, where all eleven were green because that machine's substitute
font has real metrics. Nothing in either run could see the regression. It surfaced only from
re-running every corpus and diffing the name sets, which is what the standing instruction above
asks for and the reason it is worth its wall-clock.

**The two-page one is worth having for a reason unrelated to tags.** Adding it turned three
checks red that had been green on every corpus for a week — two nav probes guarded on "more
than one page" where the guard has to be "a page that can be reached", and a search check that
cannot tell "the scan restarted" from "there was nothing ahead to find". None was a defect in
the subject; all three were preconditions written as assertions, and the smallest multi-page
fixture until then had three pages. See the traps.

**The multilingual corpus paid for itself before it was green.** It found four things, and only
the first is in the viewer: a search picker written as `/[A-Za-z]{5,}/` matched nothing on a
Japanese page, so **seventeen** search checks skipped while printing *"page 1 has no extractable
text"* about a page with forty-nine characters on it — the checks did not run and the reason
printed was false. Twelve of them run now, on the same binary. The drag check had no precondition
for *"there is text where I dragged"* and reported a sparse page as a defect. In the backend,
`FPDFText_GetUnicode` turned out to be a UTF-16 API, so a code point above the BMP arrived as two
lone surrogates and was unfindable; and a combining accent on a word with no ascender opened a
line of its own. See the traps for each.

Its own harness is **`examples/search-probe`**, which is where the search claims live: 60/60 with
9 not applicable, against a manifest a different program wrote. Run it directly, since it needs no
webview:

```sh
cargo run --release --example search-probe -- --file ../testdata/multilingual.pdf
```

Twenty-one queries, and the manifest labels each count as **stated** (from what the generator
wrote), **measured** (a property of PDFium this corpus established — that the Alphabetic and
Arabic Presentation Forms come back normalised) or **decided** (a product decision). Conflating
the three is how a measurement comes to read as a specification, so a change to a `decided` count
has to be argued for rather than absorbed — and one of them has since been argued for and
changed: the fold case-folds rather than lowercasing since 2026-08-01, so `strasse` finds `Straße`
and its count went from 1 to 2. The `decided` prose records both the old answer and the new one.

**`encodings.pdf` is the other half of the multilingual work**, and a separate corpus because
the subject is different: those pages are correct documents in other scripts, and these are
documents whose own statement of what their bytes mean is missing or wrong. Three pages, and
it found a product defect on the first run.

| page | what it is | what it established |
|---|---|---|
| `no-mapping` | Identity-H, **no `/ToUnicode`** | PDFium does not fail — it returns eighteen characters of plausible garbage for eighteen drawn. The page is *not* textless, so nothing tells a reader that a search of it means nothing |
| `broken-map` | a `/ToUnicode` with lone surrogates | the only fixture reaching `text.rs`'s replacement path. Two of its broken entries also **pair into one astral character**, which nobody predicted |
| `predefined` | `/UniJIS-UCS2-H`, **non-embedded** KozMinPro | extracts correctly, so the `chromium/7881` build has the bundled Adobe-Japan1 CMaps. A fact about the pin, to re-establish if it moves |

```sh
cargo run --release --example search-probe -- --file ../testdata/encodings.pdf
```

23/23 with 7 not applicable. Six of its seven queries are `measured` rather than `stated`: what
a broken document extracts as is a property of PDFium, and writing it as a fact about the file
is how a measurement comes to read as a specification.

**The defect it found is in the regex path.** A pattern was compiled case-sensitively against a
haystack the fold had already lowercased, so with match-case off **any uppercase letter in a
pattern matched nothing at all**. It survived because `compile`'s own doc comment asserted the
invariant it was breaking, and because `viewer_check.py` builds its pattern from a word taken
from the page — so on every corpus with ordinary prose the pattern was lowercase and the two
sides agreed by accident. This corpus's garbage happens to be uppercase.

**`--mode scan` is the timing arm, and it is a different question from the manifest checks
above.** It walks a whole document for a query three ways, round by round and interleaved:
`extract` is what a scan did before 26.9.2 (a fresh `text::extract` and a freshly compiled
query per page), `cached` is what it does now (`OpenDocument::page_codes` and one
`search::Prepared` for the walk), and `recompile` is the cache without the compile-once, so
the two effects can be told apart. The three arms' hit counts are compared as well as their
times — a faster answer that is a different answer is not a measurement — and every round
is printed, because interleaving controls for drift between the arms and not for a machine
that is slow for both.

```sh
cargo run --release --example search-probe -- \
    --mode scan --file ../testdata/text-heavy.pdf --query the --rounds 5
```

The first round is reported apart from the rest, because on the `cached` arm it is the round
that fills the cache and therefore the one round that still measures the old cost.

**Compare the name *sets*, not the counts, and slice the name by column.** Every label is
exactly six characters — `[OK]  `, `[FAIL]`, `[SKIP]` — so the name begins at column 7
whatever the outcome, and consuming the label with a regex `\s` eats one space for `[OK]`
and none for `[SKIP]`. An ad-hoc comparison written that way reported five corpora
disagreeing when the only difference was which checks had skipped. The name column is padded
to 40 and a longer name is followed by a single space, so there is no reliable split at all:
key on the first 41 characters after the label, which are byte-identical for the same check
on any document. Two corpora legitimately differ in the *order* they record two checks;
compare sets.

**Diff the names mechanically, and not with a naive split.** `record` pads each name to 40
characters and then prints the detail, so a name *longer* than that is followed by a single
space and any pattern keyed on "two or more spaces" swallows the whole line — the padded-column
trap, walked into again on 2026-07-30 while checking this very invariant. The label is seven
characters wide and the padded name forty, so a fixed slice is exactly right:

```
grep -E "^\[(OK|FAIL|SKIP)\]" run.log | cut -c8-47 | sort > names.txt
```

**That `8` is a fact about this harness, not about the repository.** `backend-probe` and
`worker-probe` built their label by interpolating `OK`/`FAIL` into `[{}]`, so their passing
rows began at column 6 and their skipped rows at column 8 — and the recipe above, applied
there, sliced the `[OK]` rows two characters short and reported *"the name sets diverge"*
across three corpora that were in fact identical. Both now pad the label to seven like
everything else, so one recipe reads every harness; before copying it to a new one, check the
widths (`grep -hoE "^\[[A-Z]+\] *" run.log | awk '{print length($0)}' | sort -u` must print a
single value). See the trap of that name.

Six of those, diffed pairwise, is the invariant in one command. It also reports the count,
which must equal the number of unique lines — two checks whose first forty characters
coincide would otherwise merge silently.

**86 until 2026-07-30**, when word and line selection added three, the palette's argument
mode added five, the two find options added three, the results sidebar added four, the fit
modes added six, and reading order added two. The
results four skip together on a document with no extractable text, which is why the two
vector fixtures gained four skips and no runs. The selection three run on every
corpus with extractable text, rotated included — line grouping follows the page's own
reading axis, so there is nothing in them that assumes lines advance downwards — and skip
together on the two vector fixtures, which is why those gained three skips and no runs. The
find-option three are the same shape: they need a word taken from page 1, so they run
wherever search does. One of them skips on a fixture whose needle is already upper case,
there being no spelling of it that matching case would reject — and it says so rather than
passing on nothing.

The reading-order two run only where a manifest exists, which today is `columns.pdf` alone;
everywhere else they skip together, which is why every other corpus gained two skips and no
runs. `columns.pdf` in turn skips two that no other corpus does — the drag-ordering check,
whose premise is false on any multi-column layout, and the rotated-lines check, whose samples
are shorter than it can compare. Both say so.

The fit six run on every corpus but one, and the exception is the informative part:
`rotated-90` skips *"fitting the page shows less of it than fitting the width"*, because its
pages are landscape and already fit the window vertically at fit-width. That check is the
control on the one beside it — without it, "fit page shows the whole page" would be
satisfied there by doing nothing — so it prints the measurement that made it inapplicable
(`495px in 700px`) rather than passing.

**The single values in that table are one sample each**, not a claim that nothing moves. One
check races (see below) and can swing a run by one in either direction; a `78--79` style
range in an earlier revision of this table was that check being honest.

**`vector-multi` takes about 4m40s**, and everything else a fraction of that — twelve A0
pages is what it is for. The default timeout was 300 s, which sat close enough to that to
fail intermittently, and the timeout path *discarded the transcript* — so a slow machine
produced one line, `[FAIL] run timed out`, which is exactly what a page that never ran a
line of JavaScript produces. It now prints how far it got and the bound is 900 s, well
clear of the slowest corpus rather than beside it.

The two vector fixtures skip three of the six inversion checks, and that is the design
working rather than a gap: "the page went dark" cannot be shown on a document with no bright
paper, so it says so instead of passing on nothing.

**The ranges are all one check: "the strip withdraws its work when the viewer needs the
renderer".** A thumbnail on a cheap page takes about a millisecond, so whether one is still
in flight when the viewer asks for a tile is a race, and the check skips when it is not —
correctly, since nothing outstanding reads exactly like a successful withdrawal. Repeated
runs of `text-heavy` and `outline-simple` have each landed on both sides of it. It is
deterministic only on `vector-multi`, which exists for it.

Absolute counts are deliberately not quoted in this paragraph: they move whenever a check is
added, and a stale number here would send someone looking for a regression that is a
changelog entry. The table above is the one place they are written down.

**So the ran/skipped columns are not the invariant** — the **names** are, and how many there
are of them is in the table above rather than in this sentence, which said `109` for two days
after the number stopped being right.

**Measured 2026-08-20 — 279 names, on every corpus, byte-identical as sets:**

| fixture | ran | skipped | failed |
|---|---|---|---|
| `rotated-90` | 227 | 52 | 0 |
| `comments` | 244 | 35 | 0 |

Re-measured the same day at **281 names** after multi-stroke drawing added the
two preview checks: `comments` 246 ran / 35 skipped / 0 failed, 281 names, all
distinct. And again at **284** when the eraser landed: `comments` **249 ran / 35
skipped / 0 failed**, all distinct. The three it added are `edit.erase` in the
command sweep and the two that read the eraser's preview — *"a stroke the
eraser has taken stops being drawn at once"* at 38% of the band before the nib
and 0% after it, and its control *"and one the nib missed is still there"* at
44%. The control is not a formality: an overlay that stopped painting the whole
drawing satisfies the first check perfectly.

**The first of those runs went red, on the check written for exactly it.**
`edit.erase` was registered and unclassified, so *"every registered command is
classified, and every classification is registered"* failed with
`unclassified [edit.erase]` — which is the trap about a command deliberately
left out of the harness still having to be classified, firing on a command that
was not meant to be left out at all. That last clause is now checked by the harness itself — `Report.finish`
fails a run in which two checks share a name, because the roll above is compared
as a **set** and a set cannot see a repeat. It caught a real one within the hour
of being written; see `docs/TRAPS.md`.

Two names were added that day — `edit.draw` in the command sweep and *"a drawing follows
its strokes and does not fill its rectangle"* in the overlay phase — and *"the five kinds do
not all look the same"* was reworded to `six`.

**Measured 2026-08-20 at 310 names**, after the marks panel. Seven are new —
`view.showMarks` in the command sweep, the five that drive the panel, and *"every sidebar
tab fits inside the panel"*. Two of the panel's five are worth naming because they are the
ones no unit test can reach: *"activating a row opens that mark's note and goes to it"*,
which needs a real press on a real element and a document with pages to travel through,
and *"pressing a mark on the page selects its row"*, which is the `onMark` wiring end to
end — the popup reports, the viewer forwards, the panel marks the row, and nothing in
the phase told the panel which mark that was.

**The seventh went red on its first run, on a defect it was written to look for.** Five
labels want 293 px of content in a 260 px sidebar, so **Marks** was clipped by the host's
`overflow:hidden` — present in the DOM, `role="tab"`, and unreachable by a pointer. The
tab *count* check beside it passed throughout, because a clipped button is still a button.
The row wraps now, and the detail line prints every label's `scrollWidth/clientWidth` so a
failure says which one and by how much.

**And the sweep is what found two defects in the panel phase itself**, neither visible on
`comments.pdf`: on `links-cropped`, a one-page document, the phase's two synthetic marks
were at the same height on the same page, so the press meant for the first opened the
second; and on `rotated-90` the check asserted the viewer's page number after activating a
row, which the last page cannot satisfy — a scroll to the end clamps and leaves the page
before it at the top of the viewport. The trap is recorded under that name. It asserts the
mark is *visible* now, which is what "goes to it" means and is what a viewer that opened
the note without scrolling fails.

**One check was red on four corpora and is now fixed.** *"a text box draws its words and
not its rectangle"* failed on `vector-heavy`, `vector-multi`, `rotated-90` and
`links-cropped`; a `git worktree` control at the text-box commit reproduced it, so it
shipped there and was invisible because that increment was verified against `comments.pdf`
alone. **The painter was right on all four** — the predicate's every reading was a
fraction of a rectangle that scaled with the page, while a text box's type is a fixed
11 points, so it failed in both directions at once: on A0 the readings rounded to zero, and
on a 20-pixel-tall box the `edges` sample, which reads the middle tenth of the height,
landed on the second line. It is now two type-sized bands and three border strips measured
in points off the box's own corner, on a fixture rectangle of a fixed 260 x 90 points.
`docs/PLAN.md` has the full account, including why the 90 came from the sampler's
two-pixel floor rather than from the type. Three mutations prove it can fail.

**Measured 2026-08-20, all fourteen corpora, `--raise` off** (superseded by the 2026-08-21
table below, and kept because it is the run the text-box repair was proved against): the same
**310 check names**
on every one, diffed as sets. `text-heavy` 265/45, `outline-simple` 273/37,
`outline-hostile` 273/37, `vector-heavy` 172/138, `vector-multi` 212/98, `rotated-90`
258/52, `columns` 262/48, `tagged` 237/73, `multilingual` 254/56, `encodings` 255/55,
`mixed` 262/48, `comments` 275/35, `links` 282/28, `links-cropped` 217/93. 740 s in total,
of which `vector-multi` is 398 s and `vector-heavy` 164 s. The only failing check anywhere
was the one above. **Re-swept after the repair: all fourteen green** — every ran/skipped
split byte-identical to the run above, the same 310 names, and `no failing checks on any of
14 corpora`. The splits being unchanged is the useful half: repairing a check by making it
skip somewhere would have moved one.

**Measured 2026-08-21, all fourteen corpora, `--raise` off: the same 313 check names on
every one, no failing check anywhere, 686 s in total.** Three names were added that day —
*"a row's remove control asks for that mark and does not open it"* with the marks panel's
remove control, then *"and the words they cover are the words that are selected"* and *"a
mark nothing was typed on is listed by the words it covers"* with the covered-words row.

| corpus | ran | skipped | 2026-08-20 | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 45 | 265/45 | 25 |
| `outline-simple` | 276 | 37 | 273/37 | 10 |
| `outline-hostile` | 276 | 37 | 273/37 | 10 |
| `vector-heavy` | 174 | 139 | 172/138 | 152 |
| `vector-multi` | 214 | 99 | 212/98 | 341 |
| `rotated-90` | 261 | 52 | 258/52 | 9 |
| `columns` | 265 | 48 | 262/48 | 9 |
| `tagged` | 240 | 73 | 237/73 | 38 |
| `multilingual` | 257 | 56 | 254/56 | 39 |
| `encodings` | 258 | 55 | 255/55 | 9 |
| `mixed` | 265 | 48 | 262/48 | 9 |
| `comments` | 278 | 35 | 275/35 | 15 |
| `links` | 285 | 28 | 282/28 | 11 |
| `links-cropped` | 220 | 93 | 217/93 | 8 |

**Re-measured the same day after the comments panel: the same 317 check names on every one,
no failing check anywhere, 680 s.** Four names were added — *"a mark nobody wrote on is
listed by the words it covers"*, *"and they are the words the fixture's generator says are
there"*, *"and those words are really on the page it is on"*, and the control *"a comment
with a body is still listed by what its author wrote"*.

| corpus | ran | skipped | earlier that day | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 49 | 268/45 | 25 |
| `outline-simple` | 276 | 41 | 276/37 | 10 |
| `outline-hostile` | 276 | 41 | 276/37 | 10 |
| `vector-heavy` | 174 | 143 | 174/139 | 149 |
| `vector-multi` | 214 | 103 | 214/99 | 339 |
| `rotated-90` | 261 | 56 | 261/52 | 9 |
| `columns` | 265 | 52 | 265/48 | 9 |
| `tagged` | 240 | 77 | 240/73 | 38 |
| `multilingual` | 257 | 60 | 257/56 | 39 |
| `encodings` | 258 | 59 | 258/55 | 9 |
| `mixed` | 265 | 52 | 265/48 | 9 |
| `comments` | 282 | 35 | 278/35 | 15 |
| `links` | 285 | 32 | 285/28 | 11 |
| `links-cropped` | 220 | 97 | 220/93 | 8 |

**Twelve corpora are `+0` ran and `+4` skipped, and the two carrying annotations are `+4`
ran and `+0` skipped.** That is the whole check on the run: the covered-words checks need a
markup annotation nobody wrote on, so on every fixture without one they must stand down by
name rather than vanish, and `comments` and `links` are exactly the two that have one.

**The first attempt at this run was red, and it is the reason the split above is worth
reading.** `commentChecks` returns early on four paths — the comments could not be read,
the document has none, no comment has a rectangle on the page, the last one has no row —
and the new checks were called after all of them. So twelve corpora neither ran nor skipped
them: `comments` and `links` reported 317 names and everything else reported 313, and each
of those runs passed on its own. A single-corpus run said `282/282 checks passed` and looked
perfect. Only the cross-corpus name-set diff can see a check that is **absent** rather than
failing, which is what `viewer_sweep.py` is for. The names are a module constant with a
`skipCoveredWords(why)` helper now, called at each of the four returns.

**The previous column is there because the way the splits moved is the check on the run.**
Twelve corpora are `+3` ran and `+0` skipped; `vector-heavy` and `vector-multi` are `+2` and
`+1`, and those two are the documents with no text to select — so the check that compares
the selection's words is skipped there, exactly as its sibling *"a mark's rectangles come
from the page's own text"* already was. Three names arriving and every corpus accounting for
all three, in the two patterns its own contents predict, is a stronger statement than
fourteen green lines.

`vector-multi` is 50% of the wall clock and `vector-heavy` 22%, as before. The total fell
from 740 s to 686 s, which is machine noise rather than anything about the harness — these
are single samples, not a benchmark.

Two of the three are unreachable from a unit test, and for different reasons worth keeping
apart. The remove control's is that the fake DOM does not bubble, so `marklist.test.ts`
cannot tell `stopPropagation` from its absence. The covered-words row's is that the fake DOM
**resolves no styles at all**: the words a mark covers and the note a reader typed sit in the
same column, and the only thing separating them is that one is dimmed and italic, so a panel
drawing them alike passes every unit test there is. That check paints a noted row beside a
bare one and reads `getComputedStyle` on both — the noted row being the control, since a
panel calling every line the document's would satisfy half of it.

One check failed once and did not recur: *"a drag selects text from where it was dragged"*
on `outline-hostile`, in one of three sweeps that day. Two runs failing different checks is
variance; the same check twice is a defect — the trap is recorded under that name, and
this was the first shape.

⚠ **The first run of that measurement was against `text-base14`, which is not a window
corpus.** `viewer_sweep.py --list` classifies it as *"a backend-probe fixture: font coverage,
measured through the worker"*, and the sweep was pointed at it anyway — the trap recorded as
*"a probe fixture swept as a corpus, against the file that already said not to"*, walked into
by the person adding checks to the harness. It passed 177/279 with 102 skipped and **the same
279 names**, which is why nothing looked wrong: the name set belongs to the harness, so it is
identical whatever you open, and only the ran/skipped split is a fact about the document. A
split from a non-corpus is meaningless as a table row, and it was written into this table as
one. Take the fixture from `viewer_sweep.py --list`, not from `ls testdata`.

⚠ **The `109` above is of 2026-07-31 and is not the current count.** Between then and now the
harness gained marks, crops, print and the comment panel, and nothing moved that number. It
was left, and it then did exactly what a stale count does: an increment predicted the new
total as `109 + 2 = 111` and was wrong by 168. **Take the count from a run, never from this
file** — the sentence below about the ran/skipped columns not being the invariant is the
same warning, and it did not stop the arithmetic being done anyway. Read the names, and read
`CHECK-NAMES-JSON`, which the harness prints for exactly this purpose:

```sh
python3 -c 'import json,sys;print(len(json.loads([l for l in open(sys.argv[1]) if l.startswith("CHECK-NAMES-JSON")][0][16:])))' run.log
```

A count chased
back to a documented value is a defect introduced to satisfy a document, and the repair here
would be to delete the outstanding-request condition that makes the withdrawal observable at
all. Read a differing count by checking that the name is present and `[SKIP]`; a name that
has *vanished* is the bug this arrangement exists to catch.

This was written as a fixed `65 | 10` first, and a perfectly ordinary run then read as a
regression. **A table that records one sample of a race as an invariant makes the next honest
run look like a defect** — state the range and what varies, or the check that flips gets
"fixed" by someone chasing a number.

**Do not run all six while iterating.** Each run needs an `.app` bundle rebuilt and takes
the better part of a minute, and six transcripts of green is not evidence of anything — the
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
and the defect it found was total rather than subtle — see `docs/PLAN.md`. Its selection
ordering check skips, with the reason: on a page whose lines advance sideways a horizontal
drag crosses all of them, so the comparison is meaningless. What checks that mapping properly
is the probe, per rotation:

```
for page in 0 1 2 3; do
    for view in 0 1 2 3; do
        src-tauri/target/release/examples/text-probe testdata/rotated.pdf \
            --page $page --mode align --view-turns $view
    done
done
src-tauri/target/release/examples/outline-probe testdata/rotated-90.pdf --mode check \
    --manifest testdata/rotated-manifest.json
```

`--mode order` is the third mode and asserts nothing — there is no right answer for it to
check, because the order a page's characters arrive in is a property of whoever produced the
file. It prints them, which is the only way to see from outside the viewer that the file's
order is not the page's:

```
src-tauri/target/release/examples/text-probe testdata/columns.pdf --page 1 --mode order
```

On page 1 of that fixture it prints `alpha one beta one`, `alpha two beta two`, and so on —
two columns merged line by line, which is what `src/lib/reading.ts` exists to undo, and what
the clipboard used to get.

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
anything` — which is the honest answer, and is why they are not written as a pass.

What it does **not** cover: the command list `App.svelte` registers, and the Cmd-K that
opens the palette. The check builds its own registry, so it proves the palette works and
not that the application's commands are wired to it.

### Checking that a mark a reader makes reaches the document

The chain a mark travels — command, gesture on the viewer, callback, edit model,
overlay — had **nothing running over it end to end**, and a reader found the hole on
2026-08-22: a shape drawn on the last page of a document was dropped with no command sent
and no message shown, while all sixteen gates stayed green. Each half asserted its own side
and was right; the join is an object literal in `App.svelte`, which no unit test imports
and which `viewer_check.py` does not reach, because that harness builds its own `Viewer`
with no model behind it.

```
scripts/mark_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/links.pdf
```

It takes the **binary**, not the bundle — there is no Launch Services route here and the
document is handed over in `argv`. Inside a bundle it must still be the one under
`Contents/MacOS/`, because WKWebView needs the bundle identity or the page never runs.
`src/lib/markcheck.ts` holds the checks and the argument for each; the one it exists for is
*"and it is recorded on the page it was pressed on"*, which derives the expected page **id**
from the model's own page list at the viewer's own slot and compares it with the id the
model filed the mark under. Under the shipped defect those differed by one on every
document.

Every assertion reads the **model** — marks that came back over the IPC boundary from
Rust — and never the viewer that produced the gesture, which is what keeps it from being a
writer agreeing with its own reader. The single exception is the ink reading, whose job is
the last hop a model assertion cannot see.

⚠ **The launch half has never run.** It was written on a machine whose screen was locked,
and `webview_guard` refuses rather than hanging — correctly, since a suspended WebKit page
does not run the check slowly, it does not run it at all. So **this harness is in the state
`docs/TRAPS.md` warns about: one that has never executed produces no failures, and neither
does one that passes.** What *is* proved is the transcript reader, which needs no screen:

```
scripts/mark_check.py --self-test
```

Seven cases, six of them refusals — no summary line, a summary disagreeing with the exit
code, a failing summary, a run that never opened a document, a skipped keystone check, and
a name found by prefix rather than by column. Run the real thing on an unlocked screen
before trusting a green line from it, and prove it can go red the way every other harness
here was proved: reintroduce the slot lookup in `Edits.mark`, rebuild, and confirm the
page-identity check fails.

### Checking session restore

Reopening where the reader left off is a property of a *launch*, so it takes more than one:

```
scripts/session_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

**It runs unmodified on Windows** (2026-07-30), and did on the first attempt — another
harness this file listed as macOS-shaped that never was. `webview_guard` already returns early
off darwin, and the script takes a binary rather than a bundle, so nothing needed porting:

```
cargo build --release --features tauri/custom-protocol --bin tpdf
python scripts/session_check.py src-tauri/target/release/tpdf.exe testdata/outline-simple.pdf
```

All four phases green, both controls included — the default state differed in all five fields
from the remembered one, and nothing opened when nothing was remembered. Expect
`Failed to unregister class Chrome_WidgetWin_0. Error = 1412` on each shutdown: that is
WebView2 teardown noise on a *passing* run, not a failure.

**Open, and intermittent: the `default` control can hang instead of running** (Windows,
2026-08-08). It passed twice that day and then timed out three runs in a row, always the same
phase and never any other. What the hung launch looks like from outside is the useful part: the
process is alive, and `MainWindowHandle` is **0** — it never created a window, so it stopped
before any JavaScript could run and no frontend change can explain it. The shape fits a
single-instance secondary that forwarded its argv and failed to exit; the phase launches
immediately after the previous one's process goes away, and `tauri-plugin-single-instance` is
Windows-only, so this race does not exist on macOS.

It is **not** caused by the print or session threading changes in the same release: the run was
repeated with those stashed and the transcript is identical line for line, through the record
phase, the file inspection and the hang. Three of four phases pass either way, `verify` — the
one that actually tests restore — at 8/8. Two things to do before chasing it: kill any stray
`tpdf.exe` first, since one alive process changes what every later launch does, and check
`MainWindowHandle` rather than assuming the app got as far as its checks.

**The fixture must have at least eight pages.** The target page is 7 and `Viewer.goToPage` clamps
to the last page, so a shorter document reports a wrong page rather than a wrong fixture —
`text-base14.pdf` gave *"page 0, wanted 7"*, stably, on a restore that was working. There is a
named check for it now (*"the document is long enough to test page restore"*), so the run says
which it is. `outline-simple.pdf` above has 12 pages; `incr-scan-20p.pdf` has 20 and renders
faster than the A0 fixtures.

**And the run now stops there rather than colouring the rest of the transcript** (2026-07-30).
The named check alone did not settle it: it fails inside the `record` phase, and the driver
launched the other three regardless, so a short fixture still produced eleven failures of which
ten were `it opens on the remembered page: page 0, wanted 7` — the signature of a broken
restore, below the line that said otherwise, and these harnesses are read from the tail. The
driver reads that check's verdict out of the transcript, skips the remaining phases by name and
ends with `[FAIL] session restore was not tested: <fixture> has too few pages ...`. Measured on
`text-base14.pdf`: eleven failures to one, three launches not made, exit code still 1.

The check's name is duplicated into `session_check.py` to do that, which is a coupling rather
than an assertion — so a transcript that does not contain it is reported as a failure of the
script, not read as "the fixture is fine". Proved by renaming it: a green run turns red with
*"this script cannot find a check named ... it has been renamed in sessioncheck.ts"*.

**Start from a clean process table, and this is not advice.** A leftover `tpdf.exe` hangs the
next run outright: reproduced twice on 2026-07-30, where the launched app sat at **0.00 CPU**
for minutes and no phase produced a summary, and both times it passed immediately after
`Get-Process tpdf,python | Stop-Process -Force`. Same shape as the occlusion warning below for
`open_check.py`, and worse here because `webview_guard` returns early off darwin, so **nothing
guards it on Windows** — Chromium suspends an occluded page exactly as WebKit does. The tell is
the CPU figure, not the clock: a child holding 0.00 CPU is hung, and a run that is genuinely
working through four launches is not. Check that before extending a timeout.

Four launches, and the two labelled `control:` are what make the other two mean anything:

| phase | session | argument | asserts |
|---|---|---|---|
| `record` | fresh | a document | drives to page 7, one quarter turn, a fixed zoom, sidebar open — then writes it |
| `control: opening without a session` | empty | a document | that state is **not** where the app opens by itself |
| `verify` | recorded | none | the app came up in that state, told only by the file |
| `control: launching with nothing remembered` | empty | none | no document opens when nothing is remembered |

Without the first control, "restored to page 7" is satisfied by an app that happens to open
there — the same shape as a check whose precondition is already satisfied, which this
repository has paid for four times. It fails if *any* of the four fields already matches,
not only if all of them do: a restore that got only the rotation right would otherwise hide
behind a default that shared the page. Without the second, an app that reopened the last
file it could find by some other route would pass `verify` perfectly.

Between the phases the script reads the written `session.json` itself. Writing a place and
reading one back are different halves, and a run that only did the second would find nothing
to restore and report that somewhere else entirely.

Unlike every other harness here, **this one does not replace the application** — it boots
normally and observes itself, because restoring is part of the boot and a check that drove
`session.ts` directly would be a second implementation agreeing with the first. Same bundle
and unlocked-screen requirements as the viewer check.

Every launch gets its own `TPDF_SESSION_FILE` in a temporary directory, and **the two
controls get one each rather than sharing**. Shared first, and the second control failed:
the first control opens a document, which is what it is for, so by the time the second
launched there was something to restore and a document duly opened. A control is the thing
you assume is inert, which is why the standing rule about what one phase leaves behind for
the next did not fire.

Unlike the viewer check, **the exit code here is meaningful** — see the note below.

### Checking file associations

A PDF reaches tpdf three ways and they share almost no code, so this drives all of them:

```
scripts/open_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app testdata/text-heavy.pdf \
    --other testdata/outline-simple.pdf
```

Note it takes the **`.app` bundle** on macOS, not the executable inside it: two phases go
through Launch Services and there is nothing else to hand `open`. **On Windows it takes the
executable**, built with `--features tauri/custom-protocol`:

```
python scripts/open_check.py src-tauri/target/release/tpdf.exe \
    testdata/outline-simple.pdf --other testdata/rotated-90.pdf
```

Four of the six phases run there and pass — `argv`, `beats`, `control`, and all four launches
of `race`. The two that cannot print `[SKIP]` **with the reason**, so the phase-name list is the
same on both platforms and a reader can diff it:

- `double-click` has no second mechanism to test. An Explorer double-click hands the path over
  in argv, which `argv` already covers; there is no Launch Services layer to go through.
- `running` has no route at all. `RunEvent::Opened` is `#[cfg(target_os = "macos")]` and no
  single-instance plugin is linked, so **a second launch is a second process** — measured, not
  inferred: two launches leave two `tpdf.exe` processes with two windows and two worker pools,
  where macOS produces one app that swaps documents. Whether that is the behaviour to want is a
  product decision; what is certain is that the *emit* branch this phase exists to exercise is
  unreachable there, and that was previously unstated in either direction.

`HANDS_OVER_TO_RUNNING` is the single place that distinction lives, and each of the two
branching phase names is a constant rather than a literal at both call sites — a name written
twice eventually differs, and the diff then shows a check that vanished on one platform when
nothing had.

| phase | delivery | asserts |
|---|---|---|
| `argv` | the binary, with a path | the terminal and Windows double-click route |
| `double-click` | `open -a` on a cold app | the Apple Event, which is how macOS actually does it |
| `beats` | argv, with a different document remembered | a handed-over document wins |
| `control` | nothing handed over | the remembered one opens — without this, `beats` passes on an app that ignores the session |
| `running` | `open -a` on an app already up | the *emit* branch rather than the queue |

`running` is the only phase that would notice the frontend and the backend disagreeing about
the event's name, and it carries its own control: nothing may be open before the document
arrives, or "a document arrived" is satisfied by one that was already there.

**The environment does reach an app that Launch Services started** —
`TPDF_OPENCHECK=… open -a tpdf.app file.pdf` propagates — which is what makes the
double-click phase testable rather than merely argued. Both `open` phases capture the app's
stdout with `open --stdout`.

Same bundle and unlocked-screen requirements as the viewer check, and one extra: **leftover
tpdf windows occlude new ones**, and an occluded page never runs, so a phase produces no
output at all. `pkill -f "tpdf.app/Contents/MacOS/tpdf"` before a run, or `TPDF_RAISE=1`.
This cost real time once already — it looked exactly like the failure it was sitting next
to, which was genuine.

### Checking the recent documents the shell is told about

Two lists, unrelated despite the name: `src/lib/recents.ts` is tpdf's own, shown in
the command palette; the shell's is the Windows Jump List and macOS's *Open Recent* and
Dock menu, filled by `recentdocs.rs` and by nothing else the application does.

**Windows: look at the file the shell writes.** `SHAddToRecentDocs` drops a shortcut per
document, so open one and look.

```powershell
ls "$env:APPDATA\Microsoft\Windows\Recent\*.pdf.lnk"
$s = New-Object -ComObject WScript.Shell
$s.CreateShortcut("$env:APPDATA\Microsoft\Windows\Recent\x.pdf.lnk").TargetPath
```

Resolve one — an entry existing is not an entry that opens. Note a Jump List needs an
*installed* build (a Start Menu shortcut is what gives the app an AppUserModelID), so a
binary from `target\release` will look as though this does nothing.

**macOS: there is no file, and every place you would look says the feature is broken.**
Measured 2026-08-20: `defaults read com.timostein.tpdf NSRecentDocumentRecords` does not
exist and never will (pre-Sierra location); `sfltool list-info` hangs; and
`~/Library/Application Support/com.apple.sharedfilelist/` answers `Operation not
permitted`, so what is in it is unknown — do **not** run that `ls` with `2>/dev/null`,
which turns the refusal into a convincing `total 0`.

So the check is **two launches**, which is the feature rather than a proxy for it. It
needs a bundle — `npm run tauri build -- --bundles app` — because a bare binary has no
identifier to key a list to.

```bash
APP=src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
pkill -f "tpdf.app/Contents/MacOS/tpdf"
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/text-heavy.pdf" 2>&1 | grep recentdocs
# quit it, then:
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/rotated.pdf" 2>&1 | grep recentdocs
```

The first launch must print `before filing, AppKit holds 0` and the second
`before filing, AppKit holds 1` naming `text-heavy.pdf` — a document the second process
never filed. That carry-over is the whole assertion; a second launch holding 0 means the
call is being dropped. The probe is off unless the variable is set, so a shipped run does
not narrate its own menu bookkeeping into the one log a reader sends back.

### Checking that a Save reaches the disk

macOS only, and it needs a built bundle and an **unlocked** screen:

```bash
scripts/save_check.py                                  # the release bundle
scripts/save_check.py path/to/tpdf.app testdata/outline-simple.pdf
```

It copies the fixture to a temporary directory, opens it, and drives the real
menu — `Page > Rotate page clockwise`, then `File > Save`, then a highlight over
the page's own text and a second Save — reading the *file* back each time by
digest and through `qpdf --check`, which shares no code with anything here.
Since 2026-09-01 it also drives one Print, last, after appending to the file
underneath the open document: the job must be refused before any panel opens.
That phase reads the process's window count, not the prompt — the Save a copy
and Reload buttons the refusal carries are in the web view, which this harness
cannot reach.

**It exists because nothing else in the repository writes a file.**
`viewer_check.py` lists `file.save` as undriven with the reason (it would write
over the corpus fixture the rest of that run is reading), `save.rs`'s tests build
their plans directly, and `edits.test.ts` asserts the shape of the `invoke` call.
So when the 26.8.6 release commit recorded saving as "reported broken from the
running application", nothing here could test that claim — and put to its
author on 2026-08-21, it turned out he had never said it. The provenance is
recorded rather than quietly deleted, because the lesson is not about saving: an
unattributed sentence in a commit message became an open item, a paragraph in
this file and two harness docstrings, and none of it could be checked until there
was a check.

The control runs first and is the reason the rest means anything: Save must be
**withheld** on a document with no edits. It also asserts Save greys again after
the save — the reopen has to produce a clean document — and that nothing is
left in the directory, since staging writes a sibling and renames it, so a stray
is a commit that failed and said nothing.

**A locked screen is refused, not skipped and not survived.** The web view is
suspended while the session is locked, so the document never opens and every menu
item stays greyed, which reads exactly like an application ignoring its own menu.
The check reads `CGSSessionScreenIsLocked` first and exits 2 saying so.

First full run 2026-08-21, on 26.8.6 plus that day's commits, before phase 7 was
added on 2026-09-01: **10 checks, all green, 25 s** — Save withheld at rest, a rotation offering it, the file changing (10731 -> 10400
bytes), `qpdf` reading it back, Save withheld again after the reopen, a highlight saving too
(-> 14542 bytes), and nothing left beside the document. Its failure path was proved
separately by pointing a phase at a menu item that does not exist: `[FAIL] this check drives
a menu item that is not there`, exit 2, and no claim about saving either way.

So there is **no defect to reproduce here**: saving over the open document works from the
menu, twice in a row, for two different kinds of edit, in a scratch directory and in a
TCC-protected one (`~/Downloads`), on this machine. That is a statement about this route and
this fixture. Every refusal `save.rs` states needs a condition a clean local file does not
have — an encrypted document, a file changed under the open one, a missing baseline — and
each of those has tests of its own; since 2026-09-01 the changed-file one is also provoked
from the menu, by phase 7's second writer, though against a Print rather than a Save. What
none of them can tell you is whether the refusal
fires when it should not, which is what a real report of a spurious message would be for.

### Checking the menu bar

macOS only, and it needs a built bundle and an unlocked screen — but no document, no
fixture and no window of its own beyond the app's:

```bash
scripts/menu_check.py                                  # the release bundle
scripts/menu_check.py path/to/tpdf.app
scripts/menu_check.py --self-test                      # the rule, without a build
```

**Run it after touching `menu.rs`, `menubar.ts`, or any command's `title`.** It reads the
live menu bar through System Events and asserts three things: that the read returned menus at
all, that no two items in one menu carry the same name, and that the bar is exactly the
menus `menubar.ts` declares in that order, plus the predefined `Window` that `menu.rs`
appends.

It exists because on 2026-08-21 the application menu carried **two items named "About tpdf"**
and nothing in either language could have said so: the platform's items are never named in
our source, and ours arrive over IPC as data, so the only place both lists exist at once is
the bar. `docs/TRAPS.md` has the entry, including why our About was the one kept.

**It launches with `open`, deliberately** — not as a subprocess with pipes. A harness that
captures output supplies a stdout and a stderr that a double-clicked application does not
have, which is the trap that hid the Windows open defect for a month; this check has no
reason to differ from the reader's launch, so it does not.

Proved in both directions against real binaries rather than against a fixture: rebuilt from
`git checkout -- src-tauri/src/menu.rs` it reports the duplicate and exits 2, and with the fix
it exits 0. `--self-test` carries both measured menus so the rule can be shown to fire in a
second, and it is not a substitute for the run: it tests the predicate, not the menu.

It is **not** a gate, for the same reason `viewer_check.py` is not — an accessibility read
needs a real session, and on a headless runner it would not fail, it would hang.

### The exit code of a spike run

`AppHandle::exit(code)` does **not** set the process's exit code. It ends the event loop,
`App::run` returns normally, `main` returns unit, and the process exits 0 whatever was asked
for. Every automated run here therefore reported success through `$?` for its whole
existence, `viewer_check.py` included. Fixed 2026-07-27 in `spike_exit`, which now flushes
and calls `std::process::exit`.

If you add a harness, do not let the exit code be its only verdict — parse the transcript
too, and make the two agree. That is what caught this: a run printing `[OK] session restore
verified` directly beneath a phase whose own last line said `0/1 checks passed`.

### The four Phase 0 spikes nothing above invokes

`AGENTS.md` says this file has the invocations, and for four `[[example]]` targets it did
not — measured 2026-08-28 by diffing the `[[example]]` names in `src-tauri/Cargo.toml`
against this document: 40 targets, 4 unnamed. They are the oldest spikes, they answered
their question once, and their answers are load-bearing in `docs/PLAN.md` — which is
exactly why they should still be runnable rather than quietly rotting into files nobody
knows how to start.

```bash
# Spike 0.3, the gating one: can one text object be edited and the rest of the page
# reproduced faithfully? Two routes, PDFium and lopdf, measured against each other.
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-roundtrip -- \
    testdata/text-base14.pdf --strict-surgical

# Spike 0.6: does an appended update section satisfy a reader that is not ours?
cargo run --release --manifest-path src-tauri/Cargo.toml --example incremental-save

# Does `pdfium-render`'s `thread_safe` actually serialize PDFium? The whole
# worker-process architecture rests on the answer, so it is measured rather than cited.
cargo run --release --manifest-path src-tauri/Cargo.toml --example thread-probe

# Can a worker be handed its document *after* it is sandboxed, over a socket? This is
# what a pre-spawned worker would need, and it is why the ~6.6 ms floor sits where it does.
cargo run --release --manifest-path src-tauri/Cargo.toml --example fdpass-probe
```

`text-roundtrip --strict-surgical` exits non-zero if either surgical variant fails
to change the target, changes pixels outside it, cannot be reopened, or yields
the wrong text. Without that flag it is an exploratory report, including expected
PDFium failures; the process exit alone is not a verdict. `text-roundtrip` and
`incremental-save` need fixtures; `fdpass-probe` is macOS-only. The self-contained
Phase 5 font matrix, contained text-edit probe, strict writer controls and native
WebKit/PDFKit commands are
in `docs/PLAN.md` §7; they need no system font or private document.

---

### Fuzzing, and how to read what it leaves behind

`src-tauri/fuzz/run.py` **is** the invocation, for the reason `scripts/gates.py` is the gate
list: three things have to be right on every run — the toolchain, a linker flag without
which the build does not link at all, and a per-target input bound — and a command copied
into prose loses one and then measures something weaker.

```bash
src-tauri/fuzz/run.py --list                                     # targets and their bounds
src-tauri/fuzz/run.py --target save_rewrite_update --seconds 3600
src-tauri/fuzz/run.py --build-only
```

**Serially.** Nine `cargo fuzz run` invocations started together queue on one build lock and
print nothing, which is indistinguishable from nine fuzzers finding nothing — the trap index
has that one.

A short run can spend its whole time budget replaying the saved corpus. The
budget includes initialization: compare the execution counters on `INITED` and
`DONE`, and require the latter to be larger before reporting new-input fuzzing.
If they are equal, extend the budget. `new_units_added=0` alone does not answer
this question; it counts additions to coverage, not generated inputs.

#### A target that reaches a defect we cannot fix has to fork

libFuzzer stops at the **first** finding, which is the right default: a finding is the point,
and stopping puts it in front of you. It is the wrong default for a target that reaches a defect
nobody here can fix, because the run then ends in the same place every time and everything
behind it is unreachable.

`lopdf_load` and `encoding_scan` both reach `docs/THREAT-MODEL.md` residual risk 21 — `lopdf`'s
cross-reference parser multiplying out the `/W` field widths a document declares and asking for
the product, which aborts through `handle_alloc_error` where no guard of ours can sit. Measured
2026-09-02 on `lopdf_load`, same corpus and same binary:

| mode | executions | wall | outcome |
|---|---|---|---|
| default | 80,269 | 21 s | stopped on the abort |
| `-fork=1 -ignore_crashes=1` | 380,803 | 96 s | ran to time |

It had been stopping like that since 2026-09-01 and nothing said so, because no campaign had
been run to completion since. Both targets are in `run.py`'s `MUST_FORK` with that reason
written beside them, so they fork whether or not `--fork` is given; `--fork` forces it on for
everything.

**Fork mode redefines a clean run, and this is the part to carry rather than the flag.** With
`-ignore_crashes=1` libFuzzer answers **0** for a forked run that completed, however many
children it buried — so an exit code stops meaning what it means everywhere else here. A
harness still reading it would report a target aborting every few minutes as permanently green.

`run.py` therefore snapshots `artifacts/<target>/` before the run and diffs it after, in
**both** the foreground and background paths, and a new file is a failure in either mode. The
snapshot is taken before anything starts because the verdict is a difference and not a count:
the directory already holds files from earlier runs. Proved both ways — a 60 s `lopdf_load`
run exits 0 with no new artifacts, and a file appearing mid-run turns it red, names the file
and points here.

The foreground path is worth its own sentence: a bare `--target <name>` lands there, and when
the artifact check was first written it covered only the background branch, so the invocation
people actually use kept the exit code as its whole answer. That is *a check bound to one
caller covers only that caller*, and here the interpreter caught it because `run` had grown an
argument.

#### Reading an artifact: three shapes, and two of them look like the third

A run that stops leaves a file in `src-tauri/fuzz/artifacts/<target>/`. Both the directory and
`corpus/` are gitignored, so **an artifact is scratch and never a regression test** — what
makes a finding permanent is a test in the source tree, and for the one defect that is upstream
rather than ours, a generated fixture (`testdata/make_xref_bomb_pdf.py`).

Every artifact triaged here has fallen into one of three shapes, and the filename tells you
nothing about which:

1. **A real defect.** Re-runs alone, allocates or crashes on its own.
2. **A sampler misattribution.** libFuzzer runs without a sanitizer here, so its memory sampler
   fires on a timer and blames whatever is executing. The clearest instance triaged here was
   `oom-da39a3ee...`, which is the SHA-1 of the **empty string** — verified with
   `printf '' | shasum`, not inferred from the name. The empty input allocates nothing. It was
   deleted on 2026-09-02 along with five siblings, so do not go looking for it; a shape-(2)
   artifact carries no information and costs the next reader a triage.
3. **A real defect below the threshold.** The one that is easy to get wrong, because it reads
   exactly like (2): the input genuinely amplifies, just not past `-rss_limit_mb`, so libFuzzer
   only ever filed the largest instance of the same bug.

The method, and each step exists because skipping it produced a wrong answer at least once:

```bash
B=src-tauri/fuzz/target/aarch64-apple-darwin/release/<target>
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 <artifact>   # does it reproduce alone?
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 <corpus file of the same size>
: > /tmp/empty.bin                                                 # NOT /dev/null -- see below
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 /tmp/empty.bin
```

Read `stat::peak_rss_mb` against the other two, not against a number you remember. The floor is
32--55 MB depending on the target, measured 2026-09-02.

`/dev/null` does not work as the empty input and fails in a way that reads as a broken checkout:
libFuzzer treats any argument that is not a regular file as a **directory**, so it answers
`ERROR: The required directory "/dev/null" does not exist`. Use a real zero-byte file.

**When it comes back clean, that is a hypothesis too.** This is the half that is easy to skip,
and skipping it is how 2026-09-02 opened: an artifact that did not reproduce was read as stale,
and the actual cause was that the fuzz target had been changed to work around the very defect
it found, taking its own subject out of every later run. **The control is to revert the fix and
re-run** — if the artifact goes loud again, the fix is what silenced it; if it stays quiet,
something else did and you do not yet know what.

**Revert *every* guard added since the artifact was filed, not only the one you suspect.**
This is the half that was wrong until 2026-09-02, and getting it wrong nearly retired a real
defect as uninterpretable. `crash-732de3ab` came back clean; reverting the fix it was filed
against — the unreduced `page.turns + view % 4`, which overflows `u8` from 253 up — left it
clean too, which reads as "some third thing silenced this and we do not know what". It was
silent because a guard added a day *later*, the made-page bound of residual risk 22, refuses
that plan before the turns arithmetic runs. With all three reverted together it panics at
`save.rs:3097` with `attempt to add with overflow`, on the first run, every time.

That is *a control refused by a different guard than the one it was written for*, which the trap
index already carries twice — and neither previous instance was about a fuzz artifact. Guards
accumulate on a path, so an artifact's age is the thing to check first: everything merged since
is standing between the input and the defect it was filed for.

That control is what separated shapes (2) and (3) here. Three `save_rewrite_update` OOMs were
about to be written off as sampler noise; with the fix reverted they read **207 MB**, **291 MB**
and **6,201 MB** against a 54 MB floor. All three were the same defect at three magnitudes, and
only the largest had ever been filed as an OOM.

**Know what an ordinary parse of that size costs before calling anything an outlier.** This is
the measurement that separates (2) from (3) when there is no fix to revert, and it is cheap:
run three corpus files of the same size. Measured 2026-09-02, `lopdf_load` has a 32 MB floor,
and 2,879-byte corpus documents cost **32 MB, 100 MB and 165 MB** — so the three artifacts
sitting at 100 MB were squarely ordinary, and one perfectly healthy corpus file is the most
expensive input of the three. `links_scan`'s floor is 53 MB and its corpus reads 53--54 MB. A
single reading against a remembered floor would have made every one of those look like a
finding.

**A `crash-` whose message is `memory allocation of N bytes failed` is an abort, not a panic.**
`catch_unwind` cannot see it, and in this repository it is upstream: `lopdf`'s cross-reference
parser multiplies out the field widths a document declares in `/W` and asks for the product.
`docs/THREAT-MODEL.md` residual risk 21 has the full account, including why no guard in tpdf's
own code can sit in front of it. **Five artifacts across three targets carry it, at five
magnitudes** — 45555555555555555, 3333333333333333332, 1844674400000000000, 40000000000000000
and 6744073709551615 bytes, each reproduced rather than read off a filename. The numbers differ
and the defect does not, so a sixth adds nothing: check the message, then delete it.

---

## Windows signing onboarding

The maintainer submitted the SignPath Foundation application on 2026-09-12.
The Foundation declined it because the project does not yet have sufficient
public adoption and independent recognition. The decision was about public
visibility, not a technical assessment of the project.

Decision recorded 2026-09-16: continue development and unsigned Windows releases;
defer a paid SignPath subscription. Reapply to the Foundation after the project
has gained broader adoption and independent references. Account configuration,
a certificate and a signed Windows build have not yet been verified. The steps
below are retained for future onboarding, not an active release dependency.
The proposed [code signing policy](README.md#code-signing-policy) records the owner,
approval model and actual network behaviour. Do not claim that signing is provided
until onboarding succeeds.

The manual `SignPath onboarding samples` workflow builds the normal executable,
MSI and NSIS on `windows-2025`, checks the production frontend excludes the harness,
and records the source commit, executable metadata and SHA-256 digests. It uploads
separate executable and installer artifacts without release or signing credentials.
Run the workflow manually from the default branch:

```sh
gh workflow run signpath-onboarding.yml --ref main
gh run list --workflow signpath-onboarding.yml --limit 5
```

Use a commit whose regular CI is green. These are onboarding samples, not a
release verification. The first hosted run at `20d2e7a` passed on 2026-09-12:
[SignPath onboarding samples](https://github.com/tstone-1/tpdf/actions/runs/34704069850).
Both artifacts were downloaded and all three binaries matched the recorded SHA-256
digests, source commit and product version. Artifacts are retained for 14 days;
rerun the workflow if SignPath needs a fresh sample. No signing request was made.
`.signpath/tpdf-exe-v1.xml` proposes a narrowly scoped executable configuration
for the first signing test: only `tpdf.exe`, product `tpdf`, with a required
version parameter. Validate it in SignPath and register it as `tpdf-exe-v1`.

Account setup needs the maintainer's chosen email, two-factor authentication on
GitHub and SignPath, Foundation acceptance, and a project linked to the public
repository. Use SignPath's GitHub connector with a CI submitter and a separate
human approver. The first integration should use the assigned organization ID,
project slug, test signing policy, and a submitter token stored as a GitHub secret;
never put the token in this repository or pass it in a command line.

Before changing `release.yml`, demonstrate one signing request from a
GitHub-hosted build using its uploaded artifact ID. Confirm the signed executable's
identity and signature against the expected test certificate. Then resolve the
installer sequence with SignPath: sign the application before packaging, cover
the NSIS uninstaller as well as setup, and sign the MSI and NSIS outer containers.
Do not sign upstream `pdfium.dll` with the Foundation certificate.
Only after Authenticode signing is complete may the updater signatures and
`latest.json` be generated from the final installer bytes. Replacing an installer
after the current Tauri action publishes it would invalidate its updater signature.
Rehearse installation and updating before enabling release signing.

The [Foundation terms](https://signpath.org/terms) require manual release approval
and verifiable project reputation; acceptance is not automatic. The
[GitHub connector](https://docs.signpath.io/trusted-build-systems/github) requires
GitHub-hosted jobs leading to OSS signing, so local or self-hosted builds are not
substitutes. Also disclose the automatic update check: the Foundation's example
privacy sentence about network activity only on request does not describe tpdf.

## Cutting a release

**26.9.18 verification, macOS arm64 and Windows x64, 2026-09-24:** all 26 gates passed on
macOS (1,928 Rust tests with three expected skips, 1,786 frontend) and on Windows,
and `check_windows.py` type-checked the Windows tree. Every mutation selected `--since
v26.9.17` ran and was caught: 56 frontend, 373 Rust and 13 window mutations. The window
phases passed on both platforms: viewer sweeps 315 text-heavy on both and 220 vector-heavy
on macOS, 217 on Windows; `textedit` 23/23, `textedit-grow` 26/26, `textedit-push` 6/6,
`import` 28/28 and `textedit-wrapped` 28/28. That last phase was the gap 26.9.17 recorded,
and on Windows it passes on the macOS LibreOffice export only (see *Naturally wrapped
paragraphs and end indents*). `textedit-push` first failed 5/6 on both platforms: the
untagged wrap made its 1,000-character draft a wrap refusal, *"this paragraph cannot wrap:
its lines would move onto what is below it"*, where the check expected *"edge of the
page"*. That expectation was stale and was corrected; the draft was still refused with no
preview. On Windows, `print-probe` passed 10/10. `redact-reach-probe` opened 156
documents: of 8,846 regions read back, zero still read as text, 3,835 were shown
unreadable and 5,011 not verified. Both installers built.

**Not run before the tag:** the external smoke test of the normal macOS bundle (step 8),
the Windows installer upgrade and hidden-engine checks, and step 12's hand-applied update
on either platform.

Published 2026-09-24 from `11fc587`, after `ci.yml` passed both legs on that commit. The
release run passed all five jobs, and the draft carried 8 assets under the tag. The published
release is Latest. Fetched without an account, the `.dmg`, `.msi`, `-setup.exe` and
`latest.json` answered 200, and `latest.json` offers 26.9.18 for `darwin-aarch64` and
`windows-x86_64`.

**The MacBook clone's tags were stale after the 2026-09-24 history rewrite, and any
clone not made after it will be too.** `git fetch` does not overwrite an existing tag, so
`v26.9.17` still named a commit with no merge base with `main`. Every `--since` run then stopped at *"git
could not diff against 'v26.9.17'"*. `git fetch origin --tags --force` once per clone fixes it.

**26.9.17 verification, macOS arm64, 2026-09-23:** all 26 gates passed on the release tree
(1,886 Rust tests with three expected skips, 1,782 frontend) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--since v26.9.16` ran and was caught:
101 frontend in 50 s and 718 Rust in 36 min 54 s. The text-edit changes were checked on the
public sample through the worker, with round trips and independent readback, in *Wrapping onto
a new line of the paragraph* and *Which runs share a line*.

**Not run before the tag, and each is a gap rather than a pass.** The release was cut unattended
with the Mac's screen locked, so nothing needing a window ran: the 14 `mutate_viewer.py` window
mutations `--since` selected, every window phase — including `textedit-wrapped`, whose LibreOffice
fixture this Mac does not have and which is the one tagged phase fixture the wrap was not checked
against — the external smoke test of the normal bundle (step 8), and step 12's hand-applied
update, including the new *Restart to finish update*. Windows had none of its steps run either:
no window phases, `print-probe` or `redact-reach-probe`. The CI release run's gate job on
`windows-2025` is the only Windows evidence for this release until those are run.

Published 2026-09-24 from `703ce12`, after `ci.yml` passed both legs on that commit: the
release run passed all five jobs, the draft carried 8 assets under the tag, and the published
release is Latest. Unauthenticated fetches of the `.dmg`, the `.msi` and `latest.json` answered
200, and the public `latest.json` offers 26.9.17 for `darwin-aarch64` and `windows-x86_64`.

**26.9.16 verification, macOS arm64, 2026-09-20:** all 25 gates passed on macOS
and `check_windows.py` type-checked the Windows tree; the release run's own gate
job passed on `windows-2025`. Every mutation selected `--since v26.9.15` ran: 207
frontend and 570 Rust, the Rust set in 1,642 s. One Rust mutation did not compile
-- `edits: address a mark by its baseline page rather than its position`, whose
replacement rewrote a `match` that gained an arm when inserting pages added
`PageSource::Imported`. It was fixed and caught. That is the third stale mutation
in a week and all three were the same kind: the `anchors` gate asserts that a
search string is present, never that the mutated file compiles, so a mutation can
rot for months and read as covered.

Ten textedit and import window phases passed on macOS, the import phase at 28/28
after this cycle's three increments added ten checks to it. The redaction of an
own page beside an inserted one was checked end to end through the sandboxed
worker (`redact-import-probe`, 17/17) and read back with `qpdf --check` and pypdf.

**The Windows window phases, `print-probe` and `redact-reach-probe` were not run
before the tag: MOTHERSHIP was unreachable (asleep) for the whole release.** They
were run against the published tag the next morning, 2026-09-21, and all passed:
viewer sweeps 315 text-heavy and 217 vector-heavy checks, `textedit-push` 6/6,
the embedded textedit phase 23/23, the import phase **28/28** including every
inserted-page check this cycle added, `print-probe` 10/10 against the real
spooler, the OCR sweep with zero regions still reading as text, and both
installers built. So the published 26.9.16 has the same Windows evidence as the
releases before it -- after publication rather than before, which is the order
this record exists to state rather than smooth over.

**26.9.15 verification, macOS arm64 and Windows x64, 2026-09-20:** all 25 gates
passed on macOS and `check_windows.py` type-checked the Windows tree. Every
mutation selected `--since v26.9.14` ran: 38 frontend and 287 Rust, in 832 s. One
Rust mutation was caught by six tests and not by the one it named (`clip: omit
rectangle horizontal scale`), a stale expectation rather than lost coverage; it
now names the test whose subject it is. All ten textedit window phases passed on
macOS, including the new `textedit-push` at 6/6 and `textedit-grow` at 26/26. On
Windows the push phase passed 6/6, the embedded textedit phase 23/23, the import
phase 18/18, and the viewer sweeps 315 text-heavy and 217 vector-heavy checks;
`print-probe` passed 10/10 and the OCR sweep found zero regions still reading as
text. Both installers built. The installer upgrade and hidden-engine checks were
not run, and neither was the macOS updater.

**The measurements in this release were corrected twice before it was cut, both
times downwards.** `docs/PLAN.md` and `CHANGELOG.md` claimed 62/51/44 as-typed
acceptance at +10/+25/+50; those came from a tree whose push still refused some
runs their own unchanged text, and the fix for that costs a few points. The
finished code measures **58/48/41**, against 44/33/28 with the box growing alone
and 1/1/0 before either. A number in prose describes whichever tree produced it,
so the run that produces the release's numbers has to be the run on the code being
released.

**26.9.14 verification, macOS arm64 and Windows x64, 2026-09-19:** all 25 gates
passed on the release commit on macOS, and `check_windows.py` type-checked the
Windows tree. Every mutation selected `--since v26.9.13` was run: 4 frontend and
183 Rust, the Rust set in 537 s. 181 Rust mutations were caught at once; the
other two (`browser state: skip external state validation`, `image: skip image
validation`) did not compile, because both calls had gained a return value their
replacement text did not supply. They predate this release, and the `anchors`
gate cannot see it, since it checks that a search string exists rather than that
the mutated file compiles. Both were re-aimed and caught. On macOS, 13 textedit
window phases passed, including `textedit-overhang` at 24/24 after its stale
expectation was corrected (see *Keeping the source's own positioning in the
editor's box*). On Windows, the `textedit` phase passed 22/22 on
`testdata/textedit-embedded.pdf` (generated, not tracked; run
`testdata/make_textedit_embedded.py` first), the import phase 18/18, and the
viewer sweeps 315 text-heavy and 217 vector-heavy cases. The real-spooler probe
passed 10/10, and the OCR sweep opened 146 documents: zero regions still read as
text, 3,640 were shown unreadable and 4,670 could not be. The MSI and NSIS
installers built. The installer upgrade and hidden-engine checks were not run.
The published release carried 8 assets and the public `latest.json` offered
26.9.14 for both platforms.

**26.9.13 verification, macOS arm64 and Windows x64, 2026-09-19:** all 25 gates
passed on the release commit on macOS, and `check_windows.py` type-checked the
Windows tree. All 308 frontend mutations selected `--since v26.9.12` were caught.
The Rust selection was 303 at about 50 s each and was not run in full; the 51
Rust mutations added or re-aimed this cycle were caught when they were written.
The import window phase passed 18/18 on both platforms (dismissing the page
question, a range, every page, search, undo, redo and save). The first macOS run
of the earlier version of that phase found search filing an inserted page's hit
under slot 0, which also mis-filed hits after a page deletion or move in 26.9.12;
fixed before the release. The first Windows sweep found one missing entry in the
viewer harness's withheld-command list, a harness omission rather than a
product fault. Native checks passed 332 links and 315 text-heavy cases on macOS,
and 315 text-heavy and 217 vector-heavy cases on Windows. The Windows real-spooler
probe passed 10/10, and the OCR sweep opened 146 documents and asked about 12,608
regions: zero still read as text, 3,640 were shown unreadable and 4,670 could not
be; unverified is not a clean verdict. The Windows MSI and NSIS installers built
locally; the installer upgrade and hidden-engine checks were not run this time.
The published draft carried 8 assets, and the public `latest.json` offered 26.9.13
for both platforms. After publishing, the installed Windows app applied the update
by hand, and its uninstall entry then read 26.9.13. The macOS updater was not
exercised; the installed Mac app was 26.9.11.

**26.9.12 local verification, Windows x64, 2026-09-18:** all 25 gates passed on
the final code: 1,747 Rust tests with three expected skips and 1,695 frontend
tests. Mutations were selected as those added or re-aimed since the last commit,
96 of them, and all were caught on the final tree; the six README mutations
`--since v26.9.11` selected were caught too. A `--since` Rust run was stopped: it
selected 676 at about 50 s each here. The boxed-edit round trip then failed, a
regression from `0a8f1a8` (after the 26.9.11 checks) where the line breaker and
the ink check measured different strings; fixed with a test and a mutation, and
the round trip and every other check below were rerun on the fixed code.
Native checks passed 314 text-heavy and 216 vector-heavy cases, with 51 and 149
not applicable, plus 22 text-edit checks. The normal MSI rendered with the
development engine hidden and refused with both engines hidden, and its
PrintWindow capture was unchanged under an overlapping control. The released
26.9.11 NSIS installer upgraded to 26.9.12; the installation and registry were
restored. The packaged app applied and saved two edits in the Typst example, one
in its CID-keyed CFF title font and one in its TrueType body font without OS/2;
pypdf read both substitutions exactly and every font resource unchanged at `f32`
precision (a full save writes `353.51562` back as `353.51563`, as lopdf stores
reals as `f32`). The real-spooler probe passed 10/10. The OCR sweep opened 146
documents and read back 8,310 regions: zero still read as text, 3,640 were shown
unreadable and 4,670 were not, in 31.8 seconds without arithmetic warnings;
unverified is not a clean verdict. After publishing, the installed 26.9.11
offered *Update to 26.9.12*, installed it, and the relaunched application
reported *tpdf 26.9.12 is the latest version*; the installed executable matched
the published MSI's after installer-marker normalization, and the normal
session file was restored byte for byte.

**26.9.11 local verification, Windows x64, 2026-09-17:** the final run passed
all 25 gates in 157.4 seconds: 1,642 Rust tests passed with three expected skips,
and 1,695 frontend tests passed. Selected mutations caught 28 distinct Rust,
six frontend and one native keyboard fault; historical tables were not rerun.
The list-frontier regression was corrected after one mutation exposed that it
did not isolate pending siblings. Native checks passed 314 text-heavy and 216
vector-heavy cases, with 51 and 149 not applicable, plus 22 text-edit checks.
The normal MSI rendered with the development engine hidden and refused with
both engines hidden. Its PrintWindow capture was unchanged under an overlapping
control. The released 26.9.10 NSIS installer upgraded to 26.9.11; the original
installation and registry exports were restored. Packaged error copying,
preview, Apply and Save passed on a disposable compatibility input, including
signature confirmation and independent text, resource and structure readback.
The final rebuilt worker round trip matched preview/save pixels and preserved
adjacent content. The real-spooler probe passed 10/10. The OCR sweep opened 144
documents, sampled 12,368 regions and read back 8,155: zero still read as text
and 3,640 were shown unreadable, in 28.2 seconds without arithmetic warnings.
The remaining regions were unverified; that is not a clean verdict.

**26.9.10 local verification, Windows x64, 2026-09-17:** all 25 quality gates
passed in 335.4 seconds after the compatible dependency updates. Rust passed
1,627 tests with three expected skips; the frontend passed 1,692. Selected
mutations caught 29/29 Rust and 9/9 frontend faults covering CJK subsets, Type 3
glyphs, partial-page editing and layouts. The native mutation file selection
contained no changed targets; the complete historical tables were not rerun.
Native viewer checks passed 314 text-heavy and 216 vector-heavy cases, with
51 and 149 not applicable, plus 22 text-edit workflow checks. The normal MSI
rendered with the development engine hidden and refused with both engines
hidden. Its PrintWindow capture stayed unchanged under an overlapping control.
The released 26.9.9 NSIS installer upgraded to 26.9.10; the original installation
and registry exports were restored. Independent PDFKit readback verified CJK
Unicode and unchanged neighbouring geometry and pixels.
The normal app passed automatic CJK preview, apply, undo/redo and save, followed
by independent Unicode readback. Automation must wait for Save to become enabled
after Redo: sending Ctrl+S while it is still disabled performs no save. A control
with the same command sequence and that readiness wait passed. Independent
round trips passed two CJK, six Type 3 and seven partial-page cases; font outlines,
resources, logical text and neighbouring pixels were checked as applicable.
The real-spooler probe passed 10/10. The Windows OCR sweep opened 142 documents
and sampled 12,128 regions. Of 7,926 regions read back, zero still read as text,
3,492 were shown unreadable and 4,434 remained unverified, in 38.1 seconds,
without arithmetic warnings. Unverified is not a clean verdict.

**26.9.9 local verification, Windows x64, 2026-09-16:** all 25 quality gates
passed after refreshing the fuzz workspace lockfile. The full run took 258.9
seconds; its corrected fuzz gate took 41.6 seconds. Rust passed 1,603 tests
with three ignored; the frontend passed 1,691. Selected mutations caught
12/12 Rust, 13/13 frontend and 1/1 native UI faults; historical tables were
not run in full. Native viewer checks passed 314 text-heavy and 216 vector-heavy
cases, with 51 and 149 not applicable, plus 22 text-edit workflow checks.
The normal MSI app passed font fallback, box sizing, wrapping, live preview,
undo/redo and save, with independent parser readback. PDFKit separately checked
new glyphs, wrapping and unchanged neighbouring pixels. The worker round trip
matched preview/save pixels on both pages and preserved adjacent content.
The packaged engine worked with the development engine hidden; hiding both
produced an engine error. PrintWindow captured only the application and its
pixels stayed unchanged under an overlapping control window. The released
26.9.8 NSIS installer upgraded to 26.9.9; the original installation and registry
were restored. The real-spooler probe passed 10/10. The OCR sweep opened 141
documents and sampled 12,008 regions, reading back 7,820 on 267 pages: zero
still read as text, 3,492 were shown unreadable and 4,328 remained unverified.
It took 46.7 seconds with no arithmetic warnings. Unverified is not clean.

**26.9.5 local verification, Windows x64, 2026-09-11:** all 23 gates passed
in 309 seconds (1,319 Rust tests passed, two ignored; 1,631 frontend tests).
The selected mutations caught 14/14 Rust, 251/251 frontend and 2/2 native UI
faults. These cover forms, signatures, movement and changed frontend files;
the historical mutation tables were not run in full. The extracted MSI passed
313 text-heavy and 215 vector-heavy viewer checks, with 51 and 149 not applicable,
plus 34 form and 20 signature checks. The development PDFium was hidden; hiding
the bundled engine too failed as expected. The print probe passed 10/10.
The real-document OCR sweep opened 133 PDFs, sampled 11,728 regions and read back
7,556 on 260 pages: zero still read as text, 3,337 were reported unreadable,
and no arithmetic warnings appeared (27.5 seconds).
The released 26.9.4 NSIS setup upgraded to 26.9.5 in a disposable installation;
the prior installed release and all three registry exports were restored.

Version scheme is **CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO
starts at 0 and increments within the month.

1. `git fetch` and confirm the local branch is not behind — this repo is pushed from more
   than one machine, and a version bump on a stale clone has already cost a re-cut release
   elsewhere in the portfolio.
2. Bump **all four** version files so they agree:
   - `package.json`
   - `package-lock.json` (top-level *and* the root package entry — `npm version <v> --no-git-tag-version` does both)
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
3. `cargo check --manifest-path src-tauri/Cargo.toml` to refresh `Cargo.lock`.
   Also run `cargo check --manifest-path src-tauri/fuzz/Cargo.toml --bins`:
   the separate fuzz lockfile records the application version too, and the
   locked fuzz gate otherwise rejects a version bump.
4. In `CHANGELOG.md`, replace `Unreleased` with the release date.
5. `scripts/gates.py` — all gates pass.

   **Once, on the final tree.** If only version files, lockfiles and prose change after that
   run, step 3's `cargo check` plus `scripts/check_trap_index.py` and `scripts/check_dates.py`
   cover what changed, and CI then runs the full list on both platforms on the release commit.
   The 26.9.18 cycle ran the suite twice here, once on MOTHERSHIP and twice more in the cloud on
   one commit. **Do not run `gates.py` on the Windows desktop as part of the window checks**:
   CI's `windows-2025` leg runs the same gates on the same commit. The desktop is for what CI
   cannot do: the window phases, `print-probe`, `redact-reach-probe` and the installers.

   On a Windows host with many cores, cap Cargo concurrency if linking exhausts
   memory: `$env:CARGO_BUILD_JOBS='2'`. The gate suite defaults to two jobs on
   Windows. A 2026-09-10 run launched more than 30 linkers; a later allocation
   failure exhausted system commit memory and terminated the calling session.
   Keep this cap for release builds as well as the gates.

   **On a Mac, also `scripts/check_windows.py`, and it is not optional before a tag.** A
   green gate list on this platform says nothing about any `#[cfg(windows)]` line, because
   the compiler never parses one: `print_win.rs`, `examples/print_probe.rs`,
   `examples/win_sandbox_probe.rs` and the Windows halves of `worker*.rs` are all outside
   what the gate list covers — the two figures below are 15/15 because that is what the run
   was at the time, and it is 17/17 since 2026-08-22; the gap is the same one whatever the
   count, which is why the figures are left as the runs reported them. Cutting `26.8.3` proved the gap rather than predicted it — the
   page-move work changed `print::Pages::Only` from `Vec<u32>` to `Vec<PagePlan>` and missed
   the one Windows-only caller, and sixteen commits went by at 15/15 before a rehearsal tag
   turned both runner legs red. That leg reported *four* failures, since clippy, test and
   bins all stop at the same `error[E0308]`.

   ⚠ **If it does not return in about a minute, it is wedged rather than slow — kill it and
   run it again.** On 2026-08-27 it sat for 15 min 45 s with its log frozen at the banner,
   and the same command on the same tree finished in **21.83 s** a minute later. The
   instrument is CPU time, not elapsed: `ps -eo pid,etime,time,args | grep
   "[x]86_64-pc-windows-msvc"` showed 2 s of CPU across sixteen minutes. Add `--verbose`
   while diagnosing — output is captured and shown only on failure otherwise, which is
   exactly wrong for a run that never ends. See the trap of that name.

   ⚠ **That minute is the warm figure, and a change to a widely-included module makes the
   run minutes long rather than seconds — so the rule above will tell you to kill a healthy
   run.** Measured 2026-08-28 after editing `ocr_gate.rs`: **~4 minutes** cold against **1 s**
   warm on the very next invocation, both green. Two things follow. Judge by CPU, never by
   elapsed, exactly as the paragraph above says — but **use the `grep` form it prints and
   not `ps -p <pid>` on the process you happen to have**, because cargo and `cargo-clippy` are
   both near zero on a perfectly healthy run and all the work is in the `clippy-driver`
   children. Reading the parent is what nearly cost a good run here. And expect the cold cost
   whenever the edit was to a module the whole crate includes: the Windows target has its own
   `target/x86_64-pc-windows-msvc` tree, so it recompiles independently of everything the
   gates just did.

   It is `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` with
   the environment that command needs, and it does not link, so no MSVC linker is involved.
   **It was `cargo check` until 2026-08-20**, and the difference is a whole class of
   failure: dead code is not a type error, so a constant read only from a
   `#[cfg(target_os = "macos")]` function passed here and failed `windows-2025` as
   `constant TEXT_SIZE is never used`. 16/16 on the Mac, 15/16 on the runner, clippy the
   only red one — found by the `v26.8.6-rc1` rehearsal tag at a cost of a 25-minute round
   trip. `-D warnings` is exactly what the `clippy` gate denies, so the two legs now agree
   about what counts as a failure. See the trap of that name.

   **It has three costs, not one, and this file recorded only the middle one.** Measured
   2026-08-21:

   | tree state | cost |
   |------------|------|
   | nothing changed since the last run | **0.47 s** |
   | local Rust changed, dependencies did not | the **~8 s** this file used to quote |
   | after a dependency change | **2 min 58 s**, measured adding three crates |
   | first run for the triple, or after `cargo clean` | **minutes**, and not cleanly measured |

   The bottom two rows are where the surprise lives: clippy *compiles* the dependency tree
   rather than checking it, and a `--target x86_64-pc-windows-msvc` build shares nothing with
   the host one, so a fresh checkout, a `cargo clean` or a `Cargo.lock` bump pays for the whole
   tree again. The 2 min 58 s is real and was taken with nothing else running — it is what
   adding `cms`, `x509-cert` and `der` cost on 2026-08-21. **The last row is a ceiling rather
   than a measurement**: the run that reached fourteen minutes had a second copy of itself
   contending for cargo's build lock, which is the caveat that matters more than the number.
   Only ever run one at a time; the script captures cargo's output and prints at the end, so
   `Blocking waiting for file lock` is swallowed and two runs look exactly like one slow one.

   Schedule from the third row, not the first: start it when the Windows-relevant work is
   done and let it run while you do something else. Against a CI round trip of six minutes.
   One-time setup, which the script names in
   full if anything is missing rather than failing four times in a row:

   ```
   brew install xwin llvm
   xwin --accept-license --arch x86_64 --variant desktop splat --output ~/.xwin
   scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
   cp /tmp/pdfium-win/bin/pdfium.dll vendor/pdfium/bin/pdfium.dll
   ```

   The DLL is the one that reads as something else: Tauri resolves `bundle.resources` for
   the *target* platform, so without it the build script dies on `resource path ... doesn't
   exist`, which looks like a broken checkout. `vendor/` is gitignored and nothing on macOS
   loads it. The splat is 629 MB, which is why this is not a gate.

   **What it does not say**: only that the Windows tree type-checks and lints. A wrong
   *value* passes — proved, by changing a `PagePlan`'s turns and watching it stay green.
   Linking, loading and behaviour are still the runner's to find. **The general form is
   worth carrying to any stand-in for another platform: it is only as strong as the command
   it runs there, never as strong as the target it names**, and anything the real gate list
   does that the stand-in does not is a class of failure it cannot report while reading as
   coverage.
6. **Re-check `docs/THREAT-MODEL.md` against the code**, and correct the document before
   trusting anything else in this list — §3's boundary table, §5's sandbox policy and
   §6's macOS column especially. Every present-tense sentence there claims something is
   *wired*, and a mitigation stated in prose and enforced nowhere reads exactly like one
   that holds: three consecutive review rounds each found at least one claim that had
   quietly become a description of an earlier phase, and the third of them was the CPU and
   memory bounds in §T3. §8 lists the probes that answer the mechanical half
   (`worker-probe`, `backend-probe`, and `worker-bench --mode engine|authority` after any
   PDFium bump). The half no probe covers is reading each claim and naming the line that
   keeps it. Anything that turns out not to be wired gets wired or gets marked, never left.

   **And re-read `README.md`**, which is the same job on the document a stranger reads.
   Since 2026-08-24 `src/lib/readme.test.ts` does the mechanical half in both directions:
   nothing under *Not built yet* may be registered, and every registered command is either
   claimed by a `<!-- built: -->` marker in the prose or excluded there with a reason. So a
   new command can no longer arrive unmentioned, and this step no longer has to diff the
   registry by eye.

   **What it still cannot touch is the status paragraph, which is where the worst of it
   was**: on 2026-08-22 that paragraph said editing had just begun and that *the open file
   is never modified in place*, six weeks and one shipped Save-in-place after either was
   true. Nor does a `built:` marker say the prose beside it is accurate — only that the
   command is claimed somewhere a reader will look, so a bullet describing a command wrongly
   passes exactly like one describing it well. Read the first three paragraphs, then the two
   feature lists, against what you know shipped this cycle. Do not put a count in the prose:
   every one that was there had drifted, and the files they describe carry their own.
7. Run mutations for the changed behaviour, including new failure cases for a new
   capability. Start with `--since <last tag>` and add affected callers or runners
   when a shared contract changes. Always run the full quality gates once, and
   the relevant native checks. Record the selected mutation count and any gaps.

   **No full GUI mutation table is required for an ordinary release.** A dependency
   pin, version bump or checks-profile/bundle-path correction does not by itself
   justify every historical mutation. Check the affected behavior and prove the
   affected observer with representative controls; expand only when a shared
   behavior cannot be covered by a bounded selection.

   **Release scope, corrected 2026-09-10:** a new capability, a large diff, or a
   month elapsed does not by itself justify every historical mutation. Full
   tables are for a harness-wide change or a shared contract whose reach cannot
   be bounded. Run the affected table when its test runner changes (for example,
   the frontend table when upgrading Vitest). The historical timings below explain
   the cost; they are not an additional requirement to run those tables.

   **How much of them to run is a decision, and this step used to duck it.** It read *"if any
   of the code they cover changed"*, which is true of nearly every release and therefore meant
   the full 735 in practice: about 1 hour 40 minutes, of which the viewer table alone is 55.
   Measured over the `26.8.7` cut, that whole spend produced **one** finding.

   ⚠ **Both of those figures are stale, and the tables have roughly doubled since.** Measured
   on the `26.9.1` cut, **Windows x64, 2026-09-02**, running all three end to end:

   | table | mutations | wall | each |
   |---|---|---|---|
   | `mutate_rust.py` | 646 (9 skipped as macOS-only) | **61.1 min** | 5.7 s |
   | `mutate_frontend.py` | 593 | **15.9 min** | 1.6 s |
   | `mutate_viewer.py` | 92 | **89.9 min** | 58.6 s |
   | total | 1,331 run of 1,340 | **2 h 47 min** | — |

   So the full pass is **1,340** rather than 735, and about **2 h 47 min** rather than 1 h 40.
   The shape of the spend is also not what the sentence above implies: the viewer table is 7%
   of the mutations and **54%** of the time, because each one rebuilds and drives a window,
   while the front-end table is 45% of the mutations and 10% of the time. If only one table
   can be afforded, the two cheap ones are 77 minutes for 1,239 mutations.

   The older figures are left above rather than overwritten: they were taken on a different
   cut and their platform is not recorded, and a number whose platform is unstated is not one
   to replace with a Windows reading. What the two together support is the *trend* — the
   tables grow with the code, and this step's cost estimate goes stale silently, because
   nothing measures it but a person running it. Steps are only
   ever added to a checklist — this is the first one this file has ever narrowed — so the
   narrowing states its own trigger rather than leaving it to whoever is tired:

   **The default is the narrow pass**, which is minutes rather than hours:

   ```
   scripts/mutate_rust.py --since <last tag>        # only the mutations whose FILE moved
   scripts/mutate_frontend.py --since <last tag>    # same flag, same meaning
   scripts/mutate_viewer.py --since <last tag>      # and here, where it saves most
   scripts/mutate_viewer.py --runner structure      # the three that need no window,
   scripts/mutate_viewer.py --runner search         # about 24 s a mutation
   scripts/mutate_viewer.py --runner encodings
   ```

   **All three take `--since` as of 2026-08-25**, from one implementation in
   `scripts/mutation_since.py`. This paragraph used to say the flag existed on `mutate_rust.py`
   only and called giving it to the other two *the obvious next piece of work*; that is done.
   Measured against `HEAD~1` of the commit that closed it: **48 of 363** Rust mutations,
   **63 of 432** front-end, **1 of 92** window.

   ⚠ **It had never once worked on Windows, and the reason is one character.** The Rust
   harness built its key with `str(Path("src-tauri") / m.path)`, which is `src-tauri\src\...`
   there, while `git diff --name-only` reports forward slashes on every platform — so the set
   membership test matched nothing and `--since HEAD~1` over a commit that changed
   `docinfo.rs`, which **48** mutations aim at, selected **0**. What made that cost nothing is
   the guard beside it: an empty selection is refused with *"this run proved nothing, which is
   not the same as a green table"* and exit 1, so the flag was unusable here rather than
   quietly certifying an empty run.

   ⚠ **The narrow pass is still genuinely partial, and `--since`'s reach is shorter than its
   scope.** A mutation is selected by the file it edits; a change in one module can stop a
   mutation in another from being caught without that other file appearing in any diff. Each
   run prints what it left out — the count against the table's total, and the changed files
   no mutation aims at — and ends by saying it is not the full table. That is still the thing
   to run before a release that qualifies above.

   ⚠ **A `--runner` run validates only that runner's mutations.** So the narrow pass cannot
   report a mutation registered against the wrong runner — which is exactly what `26.8.7`
   shipped and what the full table refused to start over. If the narrow pass is what you ran,
   the table's own consistency is unverified, and `scripts/gates.py`'s `anchors` gate covers
   the anchors but not the runner assignment.

   **Nor does `anchors` cover whether a mutation still compiles, which is what the `types`
   gate was added for on 2026-09-21.** An anchor that still matches is not a mutation that
   still works: the `before` string can sit untouched while the code around it grows a return
   value, an enum arm or an argument the `after` does not account for, and the harness then
   gets a compile error where it wanted a red test. Three of those were found in one week,
   each by a `--since <tag>` run of a table measured in tens of minutes, and each invisible
   until then — `git status` clean, `anchors` green. `scripts/check_mutation_types.py` applies
   every mutation, type-checks, and puts the bytes back; it is in the gate list, so a stale
   replacement now goes red in seconds on the run that made it stale. Its own cost, measured
   on this Mac with a warm build tree: **0.4 s** when nothing under `src-tauri/src` or `src`
   has moved since the last run, **1.8 s** after a TypeScript edit, **~16 s** after a Rust
   one, and **22 s** for the whole 2,389-mutation table from an empty cache — thirteen
   compiles, because mutations with non-overlapping anchors are applied together and the
   compiler's own file:line is what names the culprit in a failing batch.

   Two things to know before relying on it. **It is the one gate that writes to the working
   tree**, so do not run it and a mutation harness against the same checkout at once;
   a killed run is recovered from `.mutations/types-inflight.json` on the next one, which
   says so loudly. And **the two groups are judged by different criteria**: Rust must
   compile, because cargo refuses to run a mutation that does not, while TypeScript is
   checked only for names that *resolve*, because vitest never type-checks and a good third
   of the front-end table breaks the types on purpose — passing a slot number where a branded
   `FilePage` is wanted is how you mutate a page-addressing bug into existence. Requiring the
   front end to type-check reported 40 mutations of which 36 were deliberate; requiring it to
   resolve reported 5, and all 5 were vacuous.

   ```
   scripts/check_mutation_types.py              # both groups, using the cache
   scripts/check_mutation_types.py --all        # ignore the cache
   scripts/check_mutation_types.py --group rust # one group
   scripts/check_mutation_types.py --list       # what would be checked, and what cannot be
   scripts/check_mutation_types.py --self-test  # the control
   ```

   `--self-test` is the control and takes about 10 s. It plants a replacement that cannot run
   beside real mutations and requires the sweep to name it, requires the real ones beside it
   to come back clean, and — the arm that is easy to leave out — requires a planted
   *type-only* TypeScript break to pass while being **counted**, so a deliberately narrow
   exemption stays distinguishable from a checker that saw nothing.

   **The full tables:**

   ```
   scripts/mutate_rust.py          # the modules in FILTERS, `cargo test --lib`
   scripts/mutate_frontend.py      # the modules under src/lib, `vitest`
   scripts/mutate_viewer.py        # every runner below, in one pass
   scripts/mutate_python.py        # the gates themselves, ~30 s for the table

   # What each costs, measured 2026-08-21 when 26.8.7 was cut. Scale by the
   # PER-MUTATION figure, never by the total -- the totals move whenever
   # somebody writes a mutation, and `--list` is the only authority on them:
   #
   #   mutate_rust.py       292 mutations, ~2.9 s each  -> about 14 minutes
   #   mutate_frontend.py   368 mutations, ~4.9 s each  -> about 30 minutes
   #   mutate_viewer.py      75 mutations               -> 50 min (3010 s measured)
   #                        8 of those need no screen at 23-24 s each; the
   #                        other 67 rebuild the bundle at 37 s each, plus
   #                        78 s per runner for its baseline and clean rebuild
   #
   # A whole-table viewer run validates ALL TEN baselines first, in
   # alphabetical order, before it mutates anything -- so the first seven to
   # ten minutes print only "Baseline: building and running the <name>
   # harness" and no [CAUGHT] line at all. That is the run working, not the
   # run stuck, and it is worth knowing before somebody kills it: a harness
   # that has not reached its first mutation looks exactly like one that
   # cannot.
   #
   # The Rust figure is wall clock over the whole run INCLUDING its one cold
   # build, and it disagrees with the 405 s / 229 measured below -- 2.9 s
   # against 1.8 s, unexplained, on the same machine two hours apart. The
   # front-end one is that section's per-mutation figure and was NOT
   # re-measured here. Both are said plainly rather than averaged into one
   # confident number: a timing whose provenance is a mixture is worth less
   # than either of the two it came from.
   #
   # See "What the Rust table costs" below, and read it before believing any
   # older figure in this file. A backgrounded run is still worth
   # waiting on by the signal the job emits rather than by asking the process
   # table whether it is alive:
   #
   #   scripts/mutate_rust.py > run.log 2>&1; echo "exit=$?" >> run.log
   #   until grep -q '^exit=' run.log; do sleep 60; done
   #
   # `until ! pgrep -f mutate_rust.py` is the wrong instrument twice over, and
   # `docs/TRAPS.md` has both halves.

   # All three take `--only <substring>`, matched against the mutation's name,
   # for the loop while a change is being made: `--only pagetree`, `--only
   # "page delete"`. Select release scope by the rule above --- the flag
   # exists because re-proving a hundred mutations that could not have moved is
   # somebody waiting, not because a subset is ever the gate.

   # All three also take `--since <ref>`, which needs no knowledge of the
   # mutation names: it runs the ones whose FILE the diff touched, working tree
   # included, prints how many it left out and which changed files no mutation
   # aims at, and exits 1 rather than looking green when it selected nothing.
   # Its reach is shorter than its scope --- a change in docmodel.rs can stop a
   # mutation in save.rs from being caught --- so include affected callers when
   # selecting release scope under the rule above.
   scripts/mutate_rust.py --since HEAD~3
   scripts/mutate_frontend.py --since HEAD~3
   scripts/mutate_viewer.py --since HEAD~3

   # And all three take `--resume`, which is about a run that DID NOT FINISH
   # rather than about narrowing one. Two backgrounded frontend runs were killed
   # at about twenty-five minutes on 2026-08-30 by something neither the operator
   # nor the harness can account for, and each cost four hundred proved verdicts
   # AND left its mutation in the tree. `scripts/mutation_resume.py` answers both.
   scripts/mutate_frontend.py --resume
   scripts/mutate_viewer.py --runner viewer --resume

   # Or one runner at a time. The three probe runners need no webview, no bundle
   # and no unlocked screen; the three viewer ones need all three.
   scripts/mutate_viewer.py --runner structure          # structure.rs, structure-probe
   scripts/mutate_viewer.py --runner search             # search.rs + text.rs, search-probe on multilingual.pdf
   scripts/mutate_viewer.py --runner encodings          # text.rs, search-probe on encodings.pdf
   scripts/mutate_viewer.py --runner viewer             # appcommands/search/results/viewercheck.ts + search.rs
   scripts/mutate_viewer.py --runner viewer-tagged      # a11y/reading/viewercheck.ts, viewer_check on tagged.pdf
   scripts/mutate_viewer.py --runner viewer-encodings   # a11y/search.ts, viewer_check on encodings.pdf
   ```

   **`--resume` is two halves, and only the second one is behind the flag.**

   *Recovery runs on every invocation.* Before the control run, before any baseline and
   before the fingerprint, each harness reads `.mutations/<harness>.json` and asks what the
   last run left behind. The record is written **before** the mutated bytes reach the file, so
   a kill in that window leaves a record and a clean file rather than a mutation nothing
   names. The answer is by digest and has three branches: the file is what the run started
   from (nothing to do, and it says so — silence there is indistinguishable from the check
   not having run), or it is the mutation that run wrote (restored from the backup beside the
   record, verified), or it is neither, in which case somebody has edited it since and the run
   **refuses and exits 1**. A refusal is right: clobbering a repair made by hand is worse than
   the mutation it would undo. Delete `.mutations/<harness>.json` to clear it.

   *Reuse needs the flag.* Verdicts already proved are reused only if the tracked tree
   fingerprints identically — `HEAD`, the full `git diff HEAD --binary`, and every
   untracked-but-unignored file's digest. Any edit at all discards all of them, and the run
   says which of the two happened. That is blunt on purpose: a mutation's verdict is a claim
   about the whole suite, so an edit anywhere can move it — and the practical consequence is
   worth knowing before it surprises you: editing a document, or one of the harnesses, throws
   the verdicts away exactly as editing `search.rs` does. Finish editing, then resume. A
   reused line is printed with `[reused]` on the end and the summary states how many came from
   an earlier process.

   `mutate_viewer.py` also skips the baseline for any runner whose every mutation is reused
   — that baseline was taken against this same tree when the verdicts were, so building and
   running it again costs about 78 s and establishes nothing. A fully reused
   `--runner structure --only ...` run measured **0.14 s against 75 s**.

   **Two things it does not cover, both said rather than papered over.** The fingerprint
   cannot see gitignored inputs — the generated corpus under `testdata/`, `vendor/pdfium/`,
   `node_modules/`. `mutate_viewer.py` closes the largest part of that by handing the
   fixtures its chosen runners open to the fingerprint explicitly; the other two do not, so
   regenerating the corpus between a kill and a resume is a reason to drop the state. And the
   verdicts are kept whatever the flag says, so a narrow `--only` run in the middle of a
   killed table adds one verdict rather than destroying the rest — the first draft wiped the
   file on every plain run, which made the feature useless in exactly the workflow it is for.

   ```
   python3 scripts/mutation_resume.py --self-test   # 24 checks, about 1.5 s
   ```

   Its 13 mutations were run on 2026-08-31 and all 13 were caught by the check named for
   them. Three of the findings are in `docs/TRAPS.md`: a check that read the state file
   directly **raised** under the mutation aimed at it and printed no named failure; the
   failures were collected and printed at the end, so that crash took ten already-found
   failures with it; and a check for "an edited anchor is a different mutation" passed under a
   mutation that broke the key, because a case two above it had emptied the store on purpose
   and both lookups therefore missed.

   **How many mutations each carries is `--list`, not this page.** It said 23, 85 and
   15 on 2026-08-03 against an actual 36, 98 and 31 — a tally in prose, in the one
   document whose job is to schedule the run, and nothing could go red about it. The
   module names above are the invariant; the counts are a property of the table and are
   printed by `--list` in the shape `<name> -> expects: <test>`.

   **Which modules each covers is `FILTERS` and the mutation table, not this page either.**
   The line above said `search/text/structure/encoding.rs` while the harness covered ten
   modules, which is the same defect as the counts below it and in the half the page calls
   the invariant. Read them out of the scripts:

   ```
   python3 -c "import re,pathlib; print(re.search(r'FILTERS = \[(.*?)\]', pathlib.Path('scripts/mutate_rust.py').read_text(), re.S).group(1))"
   ```

   `mutate_rust.py` filters on those module prefixes, and libtest takes several and ORs them
   — but only after `--`. `cargo test --lib a:: b::` is cargo's own argument error, which
   reads like the feature being unsupported.

   `mutate_viewer.py` drives **ten** runners, chosen per mutation and filterable with
   `--runner`. The `structure`, `search` and `encodings` ones need no webview and no bundle, so
   they neither wait for one nor require an unlocked screen; each rebuilds one example and runs
   it, at a measured **23--24 s** a mutation. Every runner prints the same `[FAIL] <name>` lines
   and the same summary, so the cross-check, the byte restore and the name validation are
   shared rather than copied. `RUNNERS` in the script is the list; the three probe runners
   share `search-probe` and differ only in the fixture they open.

   **That said seven and "all six" in one paragraph, and both were wrong** — corrected
   2026-08-21 by asking the script rather than by reading the page:

   ```
   python3 -c "import re,pathlib; print(re.findall(r'\"([a-z-]+)\"\s*:', re.search(r'RUNNERS\s*[:=].*?\n\}', pathlib.Path('scripts/mutate_viewer.py').read_text(), re.S).group(0))[::3])"
   scripts/mutate_viewer.py --runner <name> --list | grep -c 'expects:'
   ```

   Two numbers in one sentence disagreeing with each other is the cheapest possible tell that
   neither was measured, and it sat here through several increments that added runners. The
   split as of 2026-08-21: `viewer` 47, `viewer-tagged` 12, `viewer-mixed` 3, `viewer-encodings`
   2, `viewer-comments` 1, `crop-rotated` 1, `crop-content` 1 — 67 needing a window — against
   `structure` 4, `search` 2, `encodings` 2, which do not. Read it from `--list`, not from here.

   `viewer-tagged` is the viewer harness against `tagged.pdf`, and it exists because the two
   tagged-reading-order checks `[SKIP]` on every other corpus. `viewer-mixed` was added on
   2026-08-17 for the same reason on a different property: a page carrying its *measured*
   size to wherever it moved is only observable where the pages are different sizes, and
   `mixed.pdf` is the one corpus that qualifies — everywhere else the layout's estimate and
   the truth are the same number. A skipped check is in the name
   set and cannot go red, so a mutation aimed at one reported **SURVIVED** — the most
   misleading verdict this harness produces, since it reads as a gap in the checks rather than
   a fixture that does not exercise them. The baseline validation now refuses that case
   explicitly, alongside the zero-match and ambiguous-prefix ones.

   The viewer runner is different in kind and slower for it: it rebuilds the bundle and runs
   `viewer_check.py` per mutation — a measured **37 s**, with a further **78 s** per runner for
   its baseline build and the clean rebuild afterwards — a whole-table run measured **3010 s**,
   50 minutes for all 75 — because what it covers — the application's
   own command list, the window shortcuts, and the search behaviour that only shows up
   against a real document — is reachable from neither `cargo test` nor `vitest`. It needs
   an unlocked, unoccluded screen for the same reason `viewer_check.py` does. It reads check
   results from **stdout only**: `viewer_check.py` writes its own verdict on the run to
   stderr in the same `[FAIL] ` shape, and counting those as checks is what made its first
   run report all ten mutations as broken.

   Each mutation names the test expected to notice, and a mutation nothing caught is
   reported as a defect in the **suite**. Three properties keep that verdict honest: both
   cross-check the failure count two ways, both treat a run with no summary line as broken
   rather than as a survivor, and both **refuse to start** if a mutation names a test the
   suite does not define — derived from the runner's own listing, since a name that cannot
   go red reports SURVIVED and reads as a gap in the tests. `--list` prints the pairs without
   running anything.

   **A module absent from `FILTERS` is only half the failure, and the loud half.** The
   guard refuses a run whose mutation names a test it cannot see, which is what caught that
   list being forgotten five times. It cannot catch the sixth shape: a module in neither
   `FILTERS` **nor** the mutation table. `fingerprint.rs` was that on 2026-08-19 — nothing
   refused to start, because nothing was aimed at it, and its central comparison turned out
   to be provable by nothing. When a module lands, add it to `FILTERS` *and* write a
   mutation, and do not wait for the guard to ask.

   **All three run on Windows as of 2026-08-19. Two did as of 2026-07-30 — 22/22 and
   75/75 — and neither did before that.** Read the first sentence as dated too: it is the
   second one that expired without anything going red, because `mutate_rust.py`'s table grew
   two macOS-only mutations on 2026-08-17 and the guard that validates test names then
   refused the whole run here. The three defects behind 2026-08-19 are each in
   `docs/TRAPS.md`, and the shape they share is worth more than any of them: **a harness that
   has never run on a platform produces no failures there, and neither does one that passes.**

   `mutate_viewer.py` had never completed a run on Windows at all, for two independent
   reasons that had to be fixed in order. Its five probe runners named their binaries as
   relative forward-slash paths, which `CreateProcess` refuses — so the run died on the
   first baseline it reached, before any mutation, with a `FileNotFoundError` naming nothing
   in this repository. Underneath that, it read bytes without normalising newlines, so on a
   CRLF checkout every multi-line anchor in its table matched **zero** times; the `anchors`
   gate could not warn, because it reads with `read_text()` and that translation makes the
   same anchors match. Both fixed, and the whole table then ran here for the first time:
   **59/59 caught, 0 survived, 0 unreadable**, about an hour including the nine baselines.

   `mutate_rust.py` had never started here at all: it read each target with `read_text()`,
   whose locale codec on Windows is cp1252, and `search.rs` holds characters whose UTF-8
   encoding contains the byte `0x81`, which cp1252 leaves undefined, so it raised
   `UnicodeDecodeError` on the first mutation. `mutate_frontend.py` ran and reported three
   anchors it could not find, because the same mis-decoding hid the glyphs in them. Both now
   read bytes and decode UTF-8, normalise newlines **for matching only** against a CRLF
   checkout, and restore from the backup as bytes. Fixing the encoding alone took the
   front-end harness from three failures to twelve, because the discarded `read_text` had
   been quietly translating line endings for the anchors that span lines — the trap of that
   name has it, and it is also a correction to what an earlier entry prescribed.

   **`mutate_rust.py` then stopped running here again, and the guard that stopped it was
   right.** `menu.rs` and `keylayout.rs` are macOS-only, so `cargo test` never compiles them
   and the two mutations aimed at them name tests that do not exist on Windows — which the
   name validation reports exactly, and then refuses the whole table over. Correct and total
   are different properties: two mutations could not run, and 178 did not. `Mutation.only_on`
   declares the scope, those two print `[SKIP] ... macos only, and this is windows`, and the
   count rides on the final verdict so a partial run cannot read as a whole one. **A mutation
   with no `only_on` still refuses**, which is the property that had to survive; both
   directions were proved by control before the fix was trusted. Measured here on 2026-08-19:
   **all 176 caught by the test named for them, 2 skipped**, about ninety minutes.

   So a Windows run of that table reports 176 and a macOS run reports 178, and the two rows
   the difference names are printed rather than absent. Do not "fix" the 176 into a 178.
   (Those are counts of that date's table, which was 231 on 2026-08-21 and **292** later the
   same day, after the signature work. A parenthetical carrying a count is the shape this file
   keeps getting wrong; `--list` is the authority, and the three tables measured **292 Rust,
   368 front-end, 75 viewer** when `26.8.7` was cut.)

   **What the Rust table costs, and what it used to cost.** On 2026-08-21 a full run was
   measured at **69 s per mutation**, which for 231 of them is 4.4 hours — a figure nobody
   can pay per feature, and it was almost entirely two things that have nothing to do with
   the mutations:

   - **An editor holding the build lock.** Every mutation writes a file under
     `src-tauri/src`, and rust-analyzer answers each write with
     `cargo check --workspace --all-targets`, which takes the build directory's lock. Cargo
     says so — `Blocking waiting for file lock on build directory` — and a no-op
     `cargo test --lib --no-run` measured **28.2 s** against **0.2 s** with the editor idle.
     The harness now sets `CARGO_TARGET_DIR` to `src-tauri/target/mutations`, so it shares
     no lock with anything. One cold build (**42 s**, 2.4 GB, inside the already-ignored
     `target/`) and it is warm for every run after.
   - **607 tests to check one assertion.** Each mutation names the one test it expects to
     redden, and the harness ran the whole filtered suite anyway. Timing the modules
     separately says where that goes: `save::` 32.4 s, `print::` 32.3 s, `keylayout::`
     17.0 s, and **the other fifteen modules 0.1 s between them** — twelve tests that
     reach PDFKit or HIToolbox, one of which a `sample` shows sitting in
     `TISCopyCurrentKeyboardLayoutInputSource` for its whole run. It now runs the named test
     alone, and the full suite **only** when that test does not go red, which is the case
     where "nothing noticed" and "something else noticed" have to be told apart.

   Measured after both, on the same machine and the same table: **405 s for all 229 runnable
   mutations**, 0 survivors, 2 skipped, and **zero** fallbacks to the full suite. That is the
   number to plan against; every older figure in this file is from before the two changes and
   is left as a statement about its own date.

   **And this one is now such a statement too.** The table was 229 runnable when that was
   measured and is **292** as of `26.8.7`, which reported *all 290 caught, 2 skipped as not
   runnable on macos*. Per-mutation cost is what to carry forward from a timing, never the
   total — the total moves every time somebody writes a mutation, and nothing goes red when
   it does.

   **It expired without anybody changing the harness, which is the part worth carrying.** By
   `26.8.11` the same table cost **40.0 s** per mutation, measured twice over four minutes —
   5.6 hours for 508 mutations, against the 1.77 s above. Nothing had regressed in the
   harness: the *crate* had grown, and every mutation touches one file, so cargo re-codegens
   the crate and relinks a test binary that full debug info had taken to 33 MB. A cost that
   goes stale because the subject grew is invisible to every check here, exactly like the
   counts this file keeps getting wrong.

   Fixed the same day by building the mutation target with `CARGO_PROFILE_DEV_DEBUG=0`,
   measured interleaved against the same command in a second target directory — 22.6/30.1/27.0 s
   against 3.9/3.7/3.3 s, and the directory 1.7 GB rather than 14 GB. The whole table then ran
   in **1316 s including its 178 s cold build**: all 504 caught, 4 skipped as not runnable on
   macos. It is safe because `debug` is debug *information* only — `debug_assertions` and
   overflow checks are separate knobs and are untouched, so every test runs the program it ran
   before, and what is given up is line numbers in a panic backtrace that nothing here reads.
   The reasoning is in `scripts/mutate_rust.py` beside the constant.

   **Decompose before believing a per-mutation figure.** The no-op freshness check is 0.8 s
   warm; touching one source file costs 14--15 s of it; the named test itself runs in 0.03 s.
   Three measurements said the rebuild was the whole cost, which is what made the lever
   obvious — and the first theory, that a 33 MB binary was slow to *load*, was wrong and took
   one direct run of the test binary to refute.

   **What the front-end table costs, and why it did not fall as far.** `mutate_frontend.py`
   got the same narrowing — it runs the test *file* holding the mutation's own test, chosen
   from the file vitest prints beside every test in the control run's listing, with the same
   fallback to all twenty files whenever the narrow run finds nothing red. That took it from
   5.8 s to about 4.9 s per mutation, and no further, because the cost is vitest's own
   startup: **4.6 s for a single small file**, and measured the same through `npx` or `node`
   directly, forks pool or threads. The full table was **1570 s for 322 mutations**; it is
   **368** as of `26.8.7`, all caught, so scale that by the per-mutation figure rather than
   reading the total.

   Getting materially below that means keeping one vitest process warm in watch mode and
   attributing each re-run to the mutation that triggered it. That is worth roughly 26 minutes
   down to ten, against a harness that can mis-attribute a run — deliberately not built yet.

   One thing the narrowing did break, and it is recorded as a trap: with one file in the run,
   a summary line can read `Tests  2 failed (2)` with no `passed` segment, which the count
   regex required. One mutation of 322 reported `no summary line -- the run did not finish`
   for a mutation its test had caught. Fixed, and proved on five summary shapes.

8. `npm run tauri build` and smoke-test the normal bundle externally, including opening
   and rendering a PDF with the development engine hidden. Normal bundles exclude the
   in-app harness, so `scripts/viewer_check.py` must instead use a separate checks build
   (commands at the top of this file), against both `testdata/text-heavy.pdf` and
   `testdata/vector-heavy.pdf`. Keep the normal-bundle and checks-build results separate.
   On Windows also run `print-probe` (§8), which is the only check that reaches a real spooler.

   Capture only the test process's own window: use its CGWindowID on macOS and
   `PrintWindow` on Windows. For the Windows capture, verify that an overlapping
   control window does not change the captured application content. A desktop-rectangle screenshot can
   contain unrelated windows and is not valid application evidence.

   **One more on Windows, and it blocks the tag rather than decorating it.** This step is
   where a Windows-only mechanism gets exercised, and it lives here rather than in a step of
   its own on purpose: fifteen sentences across five files and `release.yml` name the steps of
   this list by number, so inserting one renames every reference — the trap of that name,
   arriving in the list that made it worth writing.

   ```powershell
   cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- `
       "$env:USERPROFILE\Downloads" --pages 3 --regions 40
   ```

   **That was written in `cmd.exe` syntax until 2026-09-02**, on machines this portfolio
   documents as PowerShell 7 — `^` is not a line continuation there and `%USERPROFILE%` does
   not expand, so the first line would have run with a literal `^` argument and the second as
   a command of its own. It was the only cmd-syntax block in this file; every other Windows
   example here is a ```powershell fence using `$env:`. Nothing caught it because nothing ran
   it, which the note below says in as many words.

   **`redact-reach-probe` against real scanned documents.** Windows OCR rests on
   `win-ocr-probe`'s synthetic validation, which reads clean text at two sizes and therefore
   never puts the engine near its limit; the corpus sweep is what would, and
   `docs/THREAT-MODEL.md` §20 says so about itself. Note the flags differ from the macOS
   invocation in §`redact-reach-probe`: **drop `--no-gate`**, because the gate is the half
   under test here. Counts and shapes only leave the process — no page text, no recognised
   string, no filename beyond the stem. Record the result where this file's macOS numbers
   live, **labelled Windows**.

   **It ran for the first time on 2026-09-02**, and the reading is in `docs/PLAN.md` §6
   under *The gate on Windows*. 109 documents, 8,940 regions, 5,254 read back: **68.1%**
   taken whole, against 67.7% and 66.9% on macOS, so the ratio travels. The gate cost
   **4.4x** the cheap half here (23.5 s against 5.3 s) rather than the 40x quoted for macOS
   above — that figure is Vision's, and Windows OCR is much faster. Every bucket closed
   and no `[WARN]` printed. Three axes read differently from the Mac's and one of them
   reverses the token-length argument in the ⚠ paragraph above; that paragraph now carries
   the platform label it lacked.

   **Windows x64, 26.9.3 release check, 2026-09-07:** 117 documents opened, none
   refused; 9,808 regions sampled, 6,399 taken whole (65.2%). The gate read back
   5,775 regions on 211 pages and showed 2,256 unreadable (39.06%), in 48.5 seconds.
   No arithmetic warnings were reported. This is the current local corpus, not a
   controlled comparison with the earlier run.

   **Windows x64, 26.9.6 release check, 2026-09-12:** 133 documents opened, none
   refused; 11,728 regions sampled, 8,062 taken whole (68.7%). The gate read back
   7,556 regions on 260 pages: zero still read as text, 3,337 were shown unreadable,
   and 4,219 remained unverified. The sweep took 29.2 seconds; no arithmetic
   warnings were reported. The same build passed all 10 real-spooler print checks.

   **Windows x64, 26.9.8 release check, 2026-09-16:** 141 documents opened, none
   refused; 12,008 regions sampled, 8,326 taken whole and 3,682 not wholly
   removable. The gate read back 7,820 regions on 267 pages: zero still read as
   text, 3,492 were shown unreadable, and 4,328 remained unverified. The sweep
   took 30.9 seconds; no arithmetic warnings were reported. Unverified is not a
   clean verdict. This is the current local corpus, not a controlled comparison
   with the earlier runs.

   **The state line stays, because the debt is now a different one.** The run separates
   nothing: the two corpora share no document, so a more permissive engine and a corpus of
   cleaner type explain the Windows figures equally well, and §6 records that as open.
   **This is the one that cannot be moved to CI**, and the reason is what the probe is for:
   it needs a corpus of *real* documents, which a hosted runner has none of and must not be
   given.

   **`worker-probe` used to be listed here and is a CI step instead, since 2026-09-01.** The
   rewrite's output channel is `dup2` before `exec` on macOS and `DuplicateHandle` into a
   suspended child on Windows, so one platform's result says nothing about the other; macOS
   was at 42/42 and the last Windows run was **19/19 on 2026-08-24**, before the four
   verification-side checks, the five writing-side ones and the six copy-and-print ones
   existed — and `26.9.0` shipped without a Windows run. The probe needs no screen and takes under a second, so both
   workflows' `gates` job now runs it on both legs against `testdata/text-wide.pdf`, and every
   push proves it rather than every release. `text-wide.pdf` rather than the `text-base14.pdf`
   this file's own invocation uses: `scripts/ci_fixtures.py` generates nothing from
   `make_text_pdf.py`, and the macOS reading is the same against both fixtures, so the probe
   is not fixture-specific. The general form is worth carrying — **a requirement that cannot go
   red belongs in a runner, not in a checklist**, and a checklist step is what you write when
   no runner can hold it, as with the sweep above.

   On macOS also `scripts/menu_check.py` and `scripts/save_check.py` against the bundle.
   **Both take a `.app` bundle.** `save_check.py` also takes a document;
   `menu_check.py` takes no document. Both launch through `open`, so the application
   starts through LaunchServices without a stdout supplied by the harness. Pass the
   bundle directory, not its executable:

   ```
   python3 scripts/menu_check.py --self-test    # the duplicate rule, both directions
   python3 scripts/menu_check.py                # the release bundle at the default path
   python3 scripts/save_check.py src-tauri/target/release/bundle/macos/tpdf.app \
       "$PWD/testdata/text-heavy.pdf"
   ```

   Both probes refuse an already running bundle identifier and target only the PID they
   launched. Use a separately identified test bundle when the installed app is open.

   Run the `--self-test` first. It replays the application menu as measured before and after
   the duplicate `About tpdf` was removed, so it shows the rule calling one a defect and the
   other clean — a check that has only ever passed is not known to be able to fail, and this
   one costs a second.
   Between them they read the two surfaces no test in either language can: the menu bar as
   the reader sees it — a duplicate label shipped in 26.8.6 and reached a reader before
   anything here noticed — and the file on disk after a Save, which nothing else in the
   repository writes. Both need an unlocked screen; `save_check.py` refuses a locked one
   rather than reporting an application that ignores its menu.

   **Windows produces an MSI and an NSIS installer**, since 2026-07-30. It did not until
   then, and the rule that came out of it is worth knowing before adding a probe:
   **`src/bin/` must contain only declared bin sources.** The bundler enumerates that
   directory and registers the first entry no `[[bin]]` `path =` claims — a `.rs` file is
   always claimed, a *subdirectory* never is — so `src/bin/backend_probe/`, which held only
   `imp.rs`, became a phantom binary and failed WiX. Those bodies now live in `src/probes/`.
   See the trap of that name for the four theories that were wrong first.

   **It no longer ships the probes.** Until 2026-07-31 the installer carried all 17 spike
   and benchmark executables, including a sandbox prober and a hostile-document harness,
   because they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets
   now: cargo still builds and links them, `scripts/gates.py`'s `bins` gate still covers
   them via `--examples`, and the bundler does not see them. The MSI payload was three files
   — `tpdf.exe`, `tpdf_lib.dll`, `pdfium.dll` — verified by extracting it, and the MSI went
   16.7 -> 8.0 MB with the NSIS setup 8.8 -> 5.8 MB.

   **It is four files as of 2026-08-02**, and the fourth is the point of the notices work:
   `THIRD-PARTY-NOTICES.md`, 469 KB, which a binary distribution owes and which nothing but
   an extraction can confirm actually shipped. Re-extract and list the payload after any
   change to the resource map — that is the only step here that reads the artifact rather
   than the configuration that was meant to produce it.

   **Measured against the shipped `26.8.0` MSI it is three, and `tpdf_lib.dll` is not one of
   them** (2026-08-04). Read from the released artifact rather than from a build tree, so
   this is what people download:

   | bytes | what it is |
   |---|---|
   | 14,072,832 | the executable |
   | 478,495 | `THIRD-PARTY-NOTICES.md` |
   | 7,211,520 | `pdfium.dll` |

   The middle row is certain rather than inferred: the committed notices file is 469,298
   bytes over 9,197 lines, and 469,298 + 9,197 = 478,495 exactly — the Windows runner
   checked out with CRLF, so the shipped copy carried one extra byte per line. The other two
   are identified by size and elimination.

   ⚠ **That arithmetic dates the measurement, and the next Windows build will not reproduce
   it.** `.gitattributes` pins `* text=auto eol=lf` as of 2026-08-26, so the runner now checks
   out LF and the shipped notices file is **469,298 bytes**, the same bytes the macOS bundle
   has always carried. The row is left as measured rather than edited to the new number,
   because it was measured and the new one is predicted; re-measure it at the next release.
   The change is an improvement and is worth stating as one: an artifact that differed between
   the two platforms for no reason now does not.

   **Settled 2026-08-19, and the answer is that a local MSI is not the MSI people get.** The
   released `26.8.4` payload is **three** files, `tpdf_lib.dll` not among them; a local `npm
   run tauri build` of `26.8.5` on this desktop produced **four**, the extra one being
   `tpdf_lib.dll` at 456,704 bytes, and the generated `target/release/wix/x64/main.wxs` names
   it as a `Source=` outright. The 2026-08-02 local `26.7.0` MSI has it too, at 137,728. So
   the earlier list was read off a build tree, and both readings were honest.

   **The runner and this desktop disagree about the same commit, which is the sharpest form
   of it.** `26.8.5-rc1` was built by CI from `5e0bb20`; its MSI holds **three** files, and
   the MSI this machine built from that same commit holds four. So the difference is the
   build environment and not the version, the configuration or the tag. The Windows leg's log
   never mentions `tpdf_lib` at all, so the runner's `target/release/` did not have the
   `cdylib` at bundle time; whether it was never built there or never harvested is still
   **open**, and it does not affect the shipped application, which links the `rlib` and never
   loads the DLL.

   What follows operationally: **read a payload count off a released artifact, never off a
   local build.** `gh release download <tag> --pattern '*.msi'` and extract that — it needs
   no Windows, takes a minute, and works on a draft, so a rehearsal tag can answer it before
   the real one is cut.

   Its absence is not a defect on its face, since the binary links the `rlib` and does not
   load it — but it is one more reason the installed app has to be *run* on Windows and not
   only unpacked.

   **This does not need Windows**, which is why it happened at all. An MSI is an OLE
   compound file with a cabinet inside it, and both are readable anywhere:

   ```
   uv run --with olefile python -c "
   import olefile, struct
   ole = olefile.OleFileIO('tpdf_26.8.0_x64_en-US.msi')
   cab = next(ole.openstream(s).read() for s in ole.listdir()
              if ole.openstream(s).read(4) == b'MSCF')
   off, = struct.unpack_from('<I', cab, 16)
   n, = struct.unpack_from('<H', cab, 28)
   for _ in range(n):
       size, = struct.unpack_from('<I', cab, off)
       end = cab.index(b'\x00', off + 16)
       print(f'{size:>12,}  {cab[off+16:end].decode()}')
       off = end + 1
   "
   ```

   The names it prints are WiX **File table keys** (`Path`, `PathFile_I<guid>`), not
   destination filenames, so identify the rows by size — the count and the sizes are the
   facts here, and the count is what disagreed with this document.

   **Build before hiding the development library, not after.** The bundler copies
   `../vendor/pdfium/bin/pdfium.dll` as a resource, so a build with it already moved aside
   fails at `resource path ... doesn't exist` — which reads like a broken checkout rather
   than like the sequence being wrong. Build, extract, *then* hide.

   **Run the bundle check with the development library moved aside.** This is not optional
   and it is not paranoia: until 2026-07-31 no bundle contained PDFium at all, and every
   check passed anyway, because `pdfium_library_dir` tries the dev tree first and a check run
   from the repo never reaches the bundled branch. A check on a distributable that can see the
   development tree is a check on the development tree.

   ```
   # Windows. macOS is the same shape with lib/libpdfium.dylib and the .app.
   msiexec /a <the msi> /qn TARGETDIR=<somewhere>
   mv vendor/pdfium/bin/pdfium.dll vendor/pdfium/bin/pdfium.dll.hidden
   python scripts/viewer_check.py <somewhere>/PFiles/tpdf/tpdf.exe <an absolute path>/testdata/outline-simple.pdf
   mv vendor/pdfium/bin/pdfium.dll.hidden vendor/pdfium/bin/pdfium.dll
   ```

   Two things that cost a run each, both worth knowing before starting. Pass the PDF as an
   **absolute** path — the app resolves a relative one against its own working directory, and
   the failure is a plain "could not find the file" that reads like a broken bundle. And make
   sure the fixture has been **generated**: `testdata/*.pdf` is gitignored, an absent one
   produces the same red, and the first two attempts here died on `text-heavy.pdf`, which this
   machine had never built.

   Move the *bundled* library aside as well, once, and confirm the run fails. A pass on its own
   cannot say which of the candidate paths resolved; the failure names it.

   **Run on macOS 2026-07-31, and it failed — the Windows fix did not carry over.** The
   `.app` built cleanly and `find` reported the dylib present, which is exactly how this
   stays hidden: `Contents/Resources/pdfium` existed, and it was a **file**, not a directory.
   The bundler read `"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/"` as a target *path* and
   renamed the dylib to `pdfium`, so both bundled candidates missed and the app died on
   `Contents/Resources/libpdfium.dylib` — `0/1 checks passed`, three `could not load Pdfium`
   lines naming the path. The trailing slash is not a directory marker on this bundler.

   Fixed by naming the file in `tauri.macos.conf.json`
   (`"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/libpdfium.dylib"`), which lands it where
   the second candidate already looked. `tauri.windows.conf.json` is deliberately **not**
   changed: WiX ignores the target directory either way and the resource-root candidate
   catches it there, and that platform cannot be re-verified from a Mac. After the fix, with
   the dev library hidden: **102/102 checks passed, 7 not applicable, 109 names**.

   The failing run before the fix is the negative control, and it is what makes the pass mean
   anything — the same `.app`, the same command, the only difference being where the library
   sits. Keep both halves when repeating this.
   **On Windows, verify the UPGRADE and not only the install — with the released
   installer as the failing leg.** A first install is the case every local build exercises
   by accident; an upgrade is the one nobody sees until a reader has it. 26.8.9 could not
   install over 26.8.8 at all (`docs/TRAPS.md`, *A silent installer skips the file it cannot
   write, and exits 0*), and no check here could have said so, because every check started
   from an empty directory.

   The shape, and each leg takes about ten seconds:

   ```
   # The control is the artifact that is actually out there, not a rebuild.
   gh release download v<previous> --repo tstone-1/tpdf --pattern '*_x64-setup.exe' --dir <scratch>

   # Reproduce whatever the previous release leaves behind, in a scratch directory,
   # then run BOTH installers over it with /S /D=<dir> -- last argument, unquoted, no spaces.
   ```

   Read the answer off the *filesystem* (is the payload where `pdfium_library_dir` looks?),
   never off the exit code: the failing leg exits **0**, writes every other file, registers
   itself and creates the shortcut. Silent mode turns the Abort/Retry/Ignore box into Ignore,
   and Ignore reports success.

   The packaged executable differs from the loose `target/release/tpdf.exe` by
   Tauri's installer-kind marker: `__TAURI_BUNDLE_TYPE_VAR_UNK` becomes
   `__TAURI_BUNDLE_TYPE_VAR_NSS` for NSIS. Verify exactly one marker replacement
   and byte identity everywhere else, or compare against an extracted matching
   installer. A raw digest comparison against the loose binary falsely rejected
   the correct 26.9.5 upgrade; the difference was exactly those three bytes.

   Measured 2026-08-24, planting 26.8.8's stray `pdfium` file in both legs:

   ```
   shipped 26.8.9 setup, /S      exit 0   pdfium\pdfium.dll  ABSENT
   26.8.10 setup, /S             exit 0   pdfium\pdfium.dll  present, digest matches vendor/
   26.8.10 setup, /S, clean dir  exit 0   pdfium\pdfium.dll  present
   26.8.10 setup, /S, pdfium/    exit 0   pdfium\pdfium.dll  present, replaced
   ```

   The last two are the hook's other branches — a first install and an ordinary upgrade —
   and they are what says the fix costs nothing on a machine that never ran the broken build.

   **Installing writes to the machine you are testing on.** Three keys: `Uninstall\tpdf`,
   `Software\Timo Stein\tpdf`, and `Classes\.pdf`, whose `..._backup` value holds whatever
   handled PDFs before tpdf did. `reg export` all three first; afterwards put the machine
   back by re-running the **shipped** installer into the real location and diffing the
   exports, since re-running the new build would leave an unreleased version installed.

   **If the release adds or changes an NSIS hook, prove it was wired.** A mistyped key and a
   path naming a missing file are both refused — by the build script's schema and by the
   bundler — but a file that exists and defines the macro under another name is skipped in
   silence by the generated script's `!ifmacrodef` guard, and the bundle builds green:

   ```
   grep -n 'installer-hooks' src-tauri/target/release/nsis/x64/installer.nsi
   ```

   That is a source-level assertion and does not replace the A/B above; it is what tells you
   *why* the A/B failed when it does.

9. Commit as `Release vYY.M.MICRO: <summary>` and push it.

10. **Rehearse changed release mechanics, then tag for real.** This list ended at step 9 until
    2026-08-03, which left the single riskiest action in the process written down nowhere
    but a comment in `release.yml` — and it is the action that runs unreviewed code paths
    beside the signing key.

    ```
    git tag v26.8.0-rc1 && git push origin v26.8.0-rc1     # rehearsal
    # ... watch it, fix what it finds, delete the tag and the draft, repeat ...
    git tag v26.8.0     && git push origin v26.8.0         # the real one
    ```

    **The `Release` run skips its own gates when CI already passed them on the tagged SHA**, so
    tag only after `ci.yml` is green on both legs for that commit. A tag on a commit CI never saw runs the full gates, about 25 minutes longer on Windows.

    **Rehearse changes to build, signing, notarization or publication mechanics.**
    Release-note wording and dependency pins alone do not require a second build
    under a throwaway tag. When those mechanics are unchanged, verify the real
    release draft before publishing it. The tag glob
    `v[0-9][0-9].[0-9]*.[0-9]*` matches an `-rcN` suffix on purpose so it can be done at all.
    Cutting `26.8.0` took **three** rehearsal tags, and each found a real defect that no
    amount of reading had:

    - `rc1` — both gate legs red. `release.yml`'s `gates` job had been written from
      `ci.yml` and the copy lost the fixture-generation step, so a unit test needing
      `rotated.pdf` failed on both runners while passing in CI and locally.
    - `rc2` — gates green, Windows published, macOS died on `***: no identity found`.
      Nothing had imported the certificate into a keychain yet; the step that signs the
      vendored dylib has to run *before* the bundler copies it, and the Tauri CLI's own
      import happens two steps later.
    - `rc3` — the notarization path itself.

    Clean up between rehearsals, and note the two are separate: `git push --delete origin
    <tag>` **does not remove the draft release**, which persists without its tag and will
    sit in the release list looking like a real one.

    **Delete a draft by id, not by tag.** `gh release delete <tag> --yes` answers *"release
    not found"* for a draft that plainly exists — it resolves the tag through the same REST
    endpoint that does not return drafts, which is the behaviour the `draft` job in
    `release.yml` was written to route around. Measured on `26.8.3-rc5`: the command reported
    not-found and the draft was still there afterwards.

    ```
    gh api graphql -f query='{ repository(owner: "tstone-1", name: "tpdf") {
      releases(first: 5, orderBy: {field: CREATED_AT, direction: DESC}) { nodes {
        databaseId tagName isDraft } } } }'
    gh api -X DELETE repos/tstone-1/tpdf/releases/<databaseId>
    git push --delete origin <tag>
    ```

    A failed run publishes nothing — `release` needs `gates`, and both legs create the
    release as a **draft**. That draft is the last chance to edit the release body, and
    publishing it is step 11 rather than a clause here: see that step for what describing
    it in this sentence cost.

    **One `draft` job creates the release, and the build legs upload into it by id.** That
    is new on 2026-08-17, and it replaced a failure worth knowing about: `26.8.3-rc2` and
    `-rc3` each produced **two drafts under one tag** with the artifacts split, one holding no
    macOS updater bundle and the other no Windows installers. `tauri-action` used to resolve
    the release itself, which for a draft means paging `listReleases` for the tag — its own
    source says *"you can't get an existing draft by tag"* — and that lookup silently came
    back empty. `v26.8.2` logged `Found draft release ...` and was whole; neither leg of rc2 or
    rc3 logged it. **Why the lookup failed is still open**; `releaseId` means nothing looks
    anything up, so it no longer decides whether a release is whole.

    Step 11's asset count stays anyway, and not as ceremony: it is the only check that can
    tell a whole release from half of one, and it would have caught this before publishing.

11. **Publish the draft, and check it from outside the account.** A green `Release` run
    produces four artifacts and shows them to nobody — GitHub hides a draft from everyone
    but repository owners, and its assets sit under `releases/download/untagged-<hash>/`
    rather than under the tag. Meanwhile the *tag* is public, so from outside the repository
    the state reads as a tag pushed by mistake.

    **Count the assets before publishing, and count them with GraphQL.** A complete release
    is **8** files: the `.dmg`, `tpdf_aarch64.app.tar.gz` and its `.sig`, the `.msi` and
    `-setup.exe` with their two `.sig`s, and `latest.json`. Fewer than that is half a
    release, and the two instruments that look right for this both fail: `gh release view
    <tag>` returns *a* release for the tag with no way to say which, so with two drafts it
    reports one of them as though it were the release; and `gh api
    repos/tstone-1/tpdf/releases` answers **HTTP 200 with `[]`** under the keychain token,
    because the REST endpoint wants a scope it lacks and reports that by returning nothing.

    ```
    gh api graphql -f query='{ repository(owner: "tstone-1", name: "tpdf") {
      releases(first: 5, orderBy: {field: CREATED_AT, direction: DESC}) { nodes {
        databaseId tagName isDraft releaseAssets(first: 20) { nodes { name } } } } } }'
    ```

    ```
    gh release list --repo tstone-1/tpdf                      # second column: Draft
    gh release edit vYY.M.MICRO --repo tstone-1/tpdf --draft=false
    gh release list --repo tstone-1/tpdf                      # second column: Latest
    curl -sIL -o /dev/null -w '%{http_code}\n' \
      https://github.com/tstone-1/tpdf/releases/download/vYY.M.MICRO/<asset>   # expect 200
    ```

    **Read the body before publishing, and if you correct it, send `tag_name` with the
    correction.** It is a literal in `release.yml`, so nothing can make it go stale except
    nobody reading it, and on 2026-08-23 it listed *stamps* under "what it is not, yet" one
    release after they shipped.

    **Read `SECURITY.md` in the same pass**, for the same reason and with no mechanical half
    at all: no gate reads it, and it is the file a stranger is sent to from `README.md` when
    they have something to report. It said *"Nothing has shipped yet — there are no tags and
    no released binaries"* for thirteen releases, corrected on 2026-08-31. The supported-version
    statement is written so that it does not name a version and therefore cannot go stale by
    shipping — what can go stale is the **scope** list, so read that against what this release
    added: a new parser, a new format read, or a new place a document's bytes reach is a line
    in *In scope* or a disclosure that it is not.

    **Half of that is mechanical since 2026-08-28, and the half that is not is named.** The
    *"What it is not, yet"* sentence carries a `<!-- not-built: -->` marker, and
    `src/lib/readme.test.ts` — which already imports the registry for the README's own
    list — asserts that nothing it names is registered, and that every id it names is also
    called unbuilt in the README. So the two copies of that list are provably the same claim
    rather than merely both plausible. It was built after the block was wrong in **three of
    four** releases: stamps in `26.8.8`, *Merge documents* in `26.8.10`, and **true
    redaction** in `26.8.11`, which was published as the release that ships it.

    What it does **not** own is the `26.8.10` direction — a capability that shipped and is
    simply not mentioned. A release body is prose, and requiring it to name all 84 registered
    commands would make it the palette transcribed, so that stays with this step and a person
    reading it. Nor is every phrase covered: *"Signature verification"* names no command,
    because verifying a signature is a behaviour rather than something in the palette. Read
    the feature paragraphs against what you know shipped this cycle; the marker only stops
    the notes calling a shipped command unbuilt. A `PATCH` carrying only `body` resets the draft's `tag_name`
    to `untagged-<hash>`, and publishing in that state attaches the release to no tag —
    while `gh release list` still shows it by name and `gh release view <tag>` cannot see a
    draft at all, so neither of the two obvious instruments reports it. The GraphQL query
    above prints `tagName` beside the asset count, which is why it is the one to use.

    **Put `tag_name` inside the JSON, because `--input` discards every `-f` beside it.**
    This block carried `-f tag_name=vYY.M.MICRO --input body.json` until 2026-08-24, and
    that command does exactly what the paragraph above warns against: `--input` supplies
    the *whole* request body, so the `-f` never reaches GitHub and the PATCH is a body-only
    one. Measured on `26.8.9` — the reply came back `"tag_name":
    "untagged-bb1e54625d56b97bbd57"` from a command written to prevent that. The repair is
    one line of `json` and a second PATCH, and it is only cheap because the GraphQL query
    above is run either side of the edit; the two commands `gh release view` and `gh release
    list` both report the release by name in that state.

    ```
    python -c "import json,sys; d=json.load(open('body.json')); d['tag_name']='vYY.M.MICRO'; \
      json.dump(d, open('body.json','w'))"
    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> --input body.json    # tag_name is IN the file
    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> -f tag_name=vYY.M.MICRO -F draft=false
    ```

    The `latest.json` asset carries its own copy of that prose — `tauri-action` fills its
    `notes` from the body at build time — and correcting the release page does not correct
    it. Left alone deliberately: nothing in tpdf reads that field, so replacing an asset on a
    published release to fix text no reader sees is the worse trade. See the trap.

    **The `curl` is the point, not ceremony.** `gh release list` reporting `Latest` is our
    own authenticated view; an unauthenticated fetch of a download URL is the reader's, and
    it is the only one of the two that can tell a published release from a draft we happen
    to be able to see.

    This list ended at step 10 until 2026-08-12, with publishing named only in that step's
    closing sentence — and `26.8.0` sat as a draft for **nine days** after a green run that
    had signed, notarized and uploaded everything. A step described inside the prose of
    another step is not a step anybody executes; nothing can go red for it, since no runner
    runs it and no gate covers it. The trap is *A draft release is invisible, and the tag
    beside it says the work shipped*.

12. **Apply the update from the previous release, by hand.** This is the only end-to-end
    proof the updater works, and no gate, harness or unit test can stand in for it:
    `update.test.ts` fakes the plugin, so what it covers is the state machine and not
    signature verification, TLS, or the shape of the real `latest.json`. Nothing in this
    repository has ever fetched that file.

    Needs two published releases, so it starts from the second one ever cut with the
    updater — first opportunity is applying `26.8.2` from an installed `26.8.2`+1.

    **Carried out for the first time on 2026-08-31, and it passes.** 26.8.11 installed from
    its own `.dmg` over the 26.8.12 that was there, launched normally: the toolbar showed
    `Update to 26.8.12`, pressing it reached `Update ready — restart to finish` in **two
    seconds**, and after quit-and-reopen the toolbar read `tpdf 26.8.12` with no update
    offered — which is the negative direction in the same observation. The bundle on disk
    afterwards is 26.8.12, `spctl -a -vv` says `source=Notarized Developer ID`, and the team
    identifier is unchanged, so the payload the updater installed carries the same signature
    the `.dmg` does. The sentence above about this step never having been carried out is
    therefore spent; keep it, because it is the reason to run this rather than trust it.

    **Two instrument failures on the way, both worth knowing before repeating this.** The
    accessibility tree is *not* a usable observer here: an `entire contents` walk of the
    window reported the toolbar without the update button, and a 70-second polling loop over
    it printed six clean absences while the button was on screen the whole time. The walk
    then began returning nothing at all, silently. **A screenshot of the window is the
    instrument** — `screencapture -x -o -R<x>,<y>,<w>,<h>` with the window's own AX bounds.
    And **a synthetic `click at` from System Events does not reach the WKWebView**; `cliclick`
    posts a real event and does. Verify the pointer landed before believing a click did
    nothing: `cliclick m:<x>,<y>` then `screencapture -C`, which draws the cursor.

    ```
    # With the PREVIOUS release installed in /Applications, launched normally:
    #   1. the toolbar offers "Update to <new version>" within a second or two
    #   2. clicking it shows progress, then "Restart to finish update"
    #   3. clicking THAT restarts tpdf -- no quit and reopen by hand -- and
    #      "About tpdf" (palette or the tpdf menu) reports the new version
    #   4. the document and the page you were on come back, as after any restart
    #
    # Step 2's label read "Update ready — restart to finish" over a DISABLED
    # button until 26.9.17, and step 3 was "quit, reopen" for the same reason:
    # nothing in the application could relaunch it. Both are the action now.
    #
    # And run step 3 twice, the second time with an edit outstanding: with a
    # mark drawn and not saved, pressing Restart must ask before discarding it,
    # and answering "Keep open" must leave the update still offered and the
    # document still open. A relaunch does not go through the window's close
    # handler, so that question is asked by `finishUpdate` and by nothing else
    # -- see the trap of that name.
    #
    # That third line named a Help/About that did not exist until 2026-08-19, so
    # this step could never have been carried out. See the trap; the short of it
    # is that a checklist has no failing case, and one nobody has executed reads
    # exactly like one that keeps passing.
    curl -s https://github.com/tstone-1/tpdf/releases/latest/download/latest.json | head -20
    ```

    **On Windows steps 2 and 3 are one step, and that is the plugin rather than
    a difference in tpdf.** `Update::install_inner` there hands the installer to
    `ShellExecuteW` and calls `exit(0)`, so the application closes while
    installing and the installer starts it again; "Restart to finish update"
    never appears, because no process is left to show it. What to check instead
    is the question in front of it: with an unsaved mark, pressing **Install
    update** must ask before closing, and "Keep open" must leave the update
    still offered. Not verified on Windows as of 2026-09-21.

    **Windows live update verified 2026-09-12, from 26.9.5 to 26.9.6.** The old
    installation offered the published update; applying it installed the executable
    extracted from the verified CI MSI, after normalizing Tauri's installer-kind
    marker. The new application explicitly reported itself as current. A synthetic
    PDF rendered with the packaged engine and no workers survived exit.

    **An updater relaunch can drop `TPDF_SESSION_FILE`.** The first Windows run
    reopened the normal last document. Automated updater checks must also back up,
    temporarily clear and finally restore
    `%APPDATA%\com.timostein.tpdf\session.json`, with the application closed.
    Repeating the check this way kept the relaunched window empty and restored the
    normal session byte for byte, alongside the original executable and registry
    exports. Require the explicit latest-version message: an absent update button
    can also mean that the network check failed.

    **Check the negative direction too, and it is the cheaper half:** launch the *newest*
    release and confirm the toolbar stays empty. An updater that offers an update to the
    version already running looks identical to a working one right up to the moment somebody
    installs the same build twice.

    If `latest.json` 404s, the release is still a draft — step 11 was skipped, and the
    endpoint resolves only to published releases. That is by design: publishing is what
    offers an update to anybody.

Verify the bump landed everywhere:

```
grep -n '"version"' package.json src-tauri/tauri.conf.json
grep -n '^version' src-tauri/Cargo.toml
```

**The updater needs two secrets and neither can be read back.**
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are set on the
repository, and their only other copy is in KeePass under *"tpdf updater signing key
(minisign)"*. Lose both and no installed copy of tpdf can ever be updated again — the public
half is compiled into every binary, so the only route back is a new key, a new build, and
every user installing it by hand. The private key is deliberately **not** on any development
machine: see the trap *Turning on updater artifacts makes every build demand the signing
key* for why `createUpdaterArtifacts` lives in a CI-only overlay rather than in
`tauri.conf.json`.

### Existing-text workflow (unreleased)

Use synthetic inputs and the isolated checks application. On macOS:

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/text-edit/worker
npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app
python3 scripts/tabs_check.py "src-tauri/target/release/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf" scratch/text-edit/worker/synthetic-before.pdf --phase textedit --saved-copy scratch/text-edit/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/text-edit/worker
```

PDFKit independently reads text and compares pixels from the file saved through
the UI. Windows uses the isolated checks executable with the same
`tabs_check.py --phase textedit` arguments. The native check covers draft draining
across tabs, unsaved rendering, selection, search, undo/redo, refusal and saving.
Fuzzing includes `textedit_scan`, seeded with an editable synthetic document, plus
pending text changes in `save_rewrite_update`; the regular fuzz gate builds both.

The independent ReportLab producer exercises font setup outside the visible text
block, line leading, saved graphics states, page translations/scaling, and compressed filter arrays. It writes
both basic layouts with Flate alone and with ReportLab's usual ASCII85 wrapper,
plus saved-state, translated-origin, scaled, explicit-default and accented variants.
Generate them with:

```bash
uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-reportlab
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reportlab/separate-result scratch/textedit-reportlab/separate.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reportlab/multiline-result scratch/textedit-reportlab/multiline.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/separate-result
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/multiline-result
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reportlab/separate-ascii85-result scratch/textedit-reportlab/separate-ascii85.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reportlab/multiline-ascii85-result scratch/textedit-reportlab/multiline-ascii85.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/separate-ascii85-result
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/multiline-ascii85-result
```

The four ASCII inputs can also replace the fixture argument to `tabs_check.py --phase textedit`.
The seed generator includes a multiline document so fuzzing reaches the new state
transitions as well as their refusal paths.

Verified on macOS on 2026-09-12: both Flate-only layouts passed all 15 native
text-editing checks. PDFKit read both UI-saved files and measured 2,394 changed
pixels within the edited line and zero outside it. The independent parser also
confirmed that the worker outputs changed only the target `Tj` operand and kept
the font dictionary unchanged. All 19 targeted Rust tests and all-target Clippy
passed. A 21-second instrumented `textedit_scan` run executed 31,884 inputs with
85 MiB peak RSS and no finding. These are independent producer fixtures, not
coverage of arbitrary ReportLab output or embedded fonts.

The ASCII85 follow-up also passed both worker round trips and PDFKit comparisons.
The multiline ASCII85 input passed all 15 native application checks; PDFKit then
verified the UI-saved output with the same zero-outside-change result. All 24
focused Rust tests and all-target Clippy passed. A 21-second instrumented fuzz run
executed 34,091 inputs with 86 MiB peak RSS and no finding. Decoder tests cover
independent byte vectors, malformed markers and groups, trailing data, truncation,
and separate intermediate/final expansion bounds. Encoded input is capped at
2 MiB per stream; intermediate output and total decoded page content at 1 MiB.

The Latin-1 increment uses `latin1.pdf`, containing `SYNTHETIC ÄÖÜ ß` and the same
untouched second line. The generator also compares all 191 supported Helvetica
advances against ReportLab's independent glyph metrics. The worker and UI replace
the first line with `GEPRÜFT ß`:

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reportlab/latin1-result scratch/textedit-reportlab/latin1.pdf --latin1
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/latin1-result --latin1
python3 scripts/tabs_check.py "src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf" scratch/textedit-reportlab/latin1.pdf --phase textedit-latin1 --saved-copy scratch/textedit-reportlab/latin1-result/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-reportlab/latin1-result --latin1
```

The UI command requires the isolated checks build described above. Latin-1
characters occupy one PDF byte each; UTF-8 bytes must never be copied into the
operand. The 4,096-character limit applies equally to ASCII and accented text in
the UI, worker and journal. Other scripts and WinAnsi punctuation outside Latin-1
remain refused. Shared Helvetica metrics now include the true widths of sharp s,
accented lowercase i, slashed o and Latin-1 symbols; this also corrects wrapping
in text boxes and form appearances.

Verified on macOS on 2026-09-12: 28 focused text-editing tests, 10 shared text-layout
tests, 1,667 frontend tests and all 15 native Latin-1 workflow checks passed.
Type checking, all-target Clippy and both frontend build profiles also passed.
PDFKit read the worker and UI outputs, measuring 2,332 changed pixels inside the
edited line and zero outside. The independent parser found only the target operand
changed and the original font dictionary preserved. The instrumented fuzz run
executed 38,711 inputs in 21 seconds with 85 MiB peak RSS and no finding. The
independent metrics check rejects a deliberately wrong width; the frontend test
rejects removal of accent support. The production build excludes all harness code.

Windows x64 follow-up on 2026-09-13 at `efbec7b`: all 28 text-editing tests and
10 shared text-layout tests passed. Both ASCII85 layouts and the Latin-1 fixture
passed the contained worker probe and all 15 native UI checks each (45 total).
The external worker-exit observer passed its live/dead control and every UI run;
no check process remained. All six worker/UI outputs passed independent PDFKit
readback on macOS: 2,394 changed pixels for each ASCII case, 2,332 for Latin-1,
and zero outside the edited line. The independent parser found only the target
operand changed, with the original font preserved. Transfer sizes and SHA-256
digests matched. The remote checkout stayed clean; temporary scheduled tasks were
removed and normal frontend assets restored.

For remote Windows checks, use the logged-in interactive session. In this run the
SSH service session could not access GitHub credentials, while the interactive
memory check passed. Capture native stdout/stderr explicitly: a hidden PowerShell
transcript recorded command exit statuses but omitted native output. Piping each
command through `Tee-Object` captured the verdicts; inspect `$LASTEXITCODE` immediately
after that pipeline, without a subsequent native command replacing it.


The first embedded TrueType increment (2026-09-13, unreleased) uses an original,
MIT-licensed geometric subset generated by `testdata/make_textedit_embedded.py`.
It exercises the same native UI workflow with an embedded font rather than the
standard Helvetica resource. Reproduce the worker and independent readback:

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-embedded-worker testdata/textedit-embedded.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-embedded-worker
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-embedded-worker/synthetic-before.pdf scratch/textedit-embedded-worker/synthetic-after.pdf
```

For the native workflow, use the isolated checks build above and
`tabs_check.py <checks-binary> testdata/textedit-embedded.pdf --phase textedit`.
The macOS worker probe and all 15 native UI checks passed. Both saved outputs
passed independent PDFKit readback: 898 changed pixels inside the edited line,
zero outside. The independent parser found exactly one changed `Tj` operand
and byte-identical font programs with unchanged font dictionaries. Its negative
control refuses the unedited input. The generated font is deterministic and
contains no third-party font material.

The text-edit fuzz target includes this font as a built-in seed, independent of
locally generated PDFs. A final 21-second run exercised 33,914 inputs without a finding,
with 86 MiB peak RSS. The target now skips already-empty runs: deleting one is an
unchanged edit, which the production writer correctly refuses. An empty-run seed
keeps that case in every future run.

All 35 focused text-edit tests passed, including seven embedded-font tests.
Removing either missing-glyph rejection or cmap agreement made its targeted test
fail. A further regression test reproduced acceptance of an out-of-range space
glyph before the fix and passed afterwards: no-outline results now require equal,
in-bounds glyph offsets before a space is considered blank. The permanent mutation
registry covers all three guards.

Final verification passed all 24 repository gates after classifying the new fixture
and rerunning the seven affected gates. The full sweep took 681.6 seconds; the
final Rust/inventory rerun took 314.9 seconds. The final suite passed 1,331 Rust
tests (three ignored) and 1,667 frontend tests. A fresh worker probe after the
space-glyph fix again passed both independent readers with 898 changed pixels
inside the target line and zero outside. Normal frontend assets were restored.

Windows x64 verification on 2026-09-13 passed this embedded-font increment from
an isolated, uncommitted source snapshot over `efbec7b`. The transferred archive's
SHA-256 was `56398dbc98c60c85dad46cb09dc04b962582820cf0607251fd8e5d6d89237f83`;
all 18 overlaid files matched their manifest before and after the checks, including
the regenerated font. All 35 text-edit tests, 10 shared text-layout tests, the
worker preview/save probe and all 15 native UI checks passed. The worker-exit
observer passed its control and found no surviving test workers.

Both Windows-saved PDFs passed independent parser checks on Windows and macOS:
only the target `Tj` operand changed, with font dictionaries and decoded font
programs preserved. PDFKit read both outputs and measured 898 changed pixels inside
the edited line, zero outside. Retrieved PDF sizes and SHA-256 digests matched the
Windows artifacts. The remote job took 140 seconds, restored normal frontend
assets with zero harness code, and left the normal checkout clean at its original
commit. The temporary scheduled task was removed.

The saved-state increment (2026-09-13, unreleased) adds bounded `q`/`Q` support
outside text blocks. The independent producer's `saved-state-ascii85.pdf` is
generated by the ReportLab command above. Before this change the worker refused
it; afterwards all 38 text-edit tests and all 15 native checks passed on both
platforms. Windows also passed the 10 shared text-layout tests. All four worker/UI
outputs passed independent parser and PDFKit checks: only the target operand
changed, fonts were preserved, and 2,394 pixels changed inside the edited line
with zero outside. Three targeted mutations were caught by their named tests:
discarding restored state, accepting unclosed saves and selecting the last `Tf`
instead of the restored font.

The Windows job used an isolated snapshot over `efbec7b`, archive SHA-256
`dee6aca45c765332a29543a0412752a05ff4874e5a002a4be7df5fd43ac38e7f`.
Source and output digests matched, no test workers survived, normal frontend
assets were restored, and the temporary scheduled task was removed. The job
took 63 seconds using the previous build cache.

The built-in `editable-saved-state` seed exercises restoration in future fuzz
runs. Use the repository wrapper, which supplies the macOS linker flags and
per-target input bounds:

```sh
uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20
```

The final instrumented run completed 36,925 inputs in 21 seconds with 86 MiB peak
RSS and no finding. This uses the wrapper's coverage instrumentation and Rust
bounds/overflow checks; AddressSanitizer remains disabled as documented there.

Final verification passed all 24 repository gates after removing redundant test-pattern
labels and applying rustfmt. The full sweep took 511.5 seconds; the focused
Clippy rerun took 93.0 seconds. The final suite passed 1,334 Rust tests (three
ignored) and 1,667 frontend tests. Normal frontend assets exclude all harness code.

The translated-origin increment (2026-09-13, unreleased) accepts pure page
translations outside text blocks. ReportLab's `translated-ascii85.pdf` places the
first line through two nested translations, including a negative offset, then
restores the original coordinates for the second line. The preceding editor
refused this input; the updated worker passes preview, extraction, search,
undo and save checks. All 41 focused text-edit tests pass. Tests also compare
translated and absolute hit boxes under all four page rotations with a crop,
and refuse accumulated or composed positions outside the coordinate bound.

Windows passed those 41 tests, the 10 shared text-layout tests and all 15 native
checks from an isolated snapshot over `efbec7b`, archive SHA-256
`3670db8816ca7d4b911a2c0d27e5a140feed891c0448d7b61744bde57931548d`.
Both Windows saves passed independent PDFKit and parser readback on macOS:
2,394 changed pixels inside the edited line, zero outside, with only the target
operand changed and fonts preserved. Source/output digests matched. The 51-second
remote job left no test workers, restored normal frontend assets and removed its
temporary scheduled task.

Three targeted mutations were caught: omitting the page translation from the run,
forgetting a saved translation and accepting a nontranslation matrix. The
`editable-translated` fuzz seed is built in; the instrumented run completed 37,682
inputs in 21 seconds, with 86 MiB peak RSS and no finding.

The macOS native run also passed all 15 checks. Both Mac worker/UI saves passed
the same independent readback, bringing this increment to four verified outputs
with zero changed pixels outside the edited line. All 24 repository gates passed
in one 351.6-second sweep: 1,337 Rust tests (three ignored), 1,667 frontend tests,
and the locked fuzz/example builds. Normal frontend assets were restored and
contain zero harness code. The separate Mac checks application build took 7m11s
and rebuilt dependencies just used by the plain Cargo gates. The following
investigation resolved that repeated build work.

On 2026-09-13 Cargo's fingerprint log identified two direct environment
invalidations: `ring` and `objc2-exception-helper` saw
`MACOSX_DEPLOYMENT_TARGET` change from `Some("10.13")` to `None`. The ensuing
plain `cargo build --locked --manifest-path src-tauri/Cargo.toml --lib` rebuilt
37 dependencies and took 2m05s. The root Cargo configuration now supplies the
same default as Tauri's explicitly recorded minimum system version. Two mutation
controls prove that the toolchain gate rejects drift on either side.

The first full sweep with that configuration passed all 24 gates in 584.0 seconds,
including 1,337 Rust tests (three ignored), 1,667 frontend tests and the locked
fuzz/example builds. This sweep includes refreshing the separate compiler/test
caches. The subsequent checks application build took 37.51s, compiling four Tauri
dependencies whose build variant had not yet been refreshed, and passed all 15
native text-edit checks. Switching back to the same plain Cargo command took
9.35s and rebuilt zero dependencies. A normal application build then took 17.54s,
and another plain Cargo build took 8.39s, each with zero dependency rebuilds and
no deployment-target invalidation. The application itself still rebuilds when
its embedded assets or Tauri configuration change. Normal frontend assets contain
zero harness code. These timings describe this debug-build sequence on macOS;
the dependency invalidations establish the mechanism independently of timing.

The positive page-scaling increment (2026-09-13, unreleased) accepts horizontal
and vertical scaling outside text blocks, composed with translations and saved
graphics states. ReportLab's `scaled-ascii85.pdf` independently produces a first
line scaled by 1.25 horizontally and 0.75 vertically, with the second line outside
the saved state. The preceding worker refused it. The updated worker passes
preview, extraction, search, undo, refusal and save checks. All 44 focused
text-edit tests pass, including transform order/restoration, equivalent hit boxes
after crop and all four page rotations, bounded composed scales/positions and
underflow refusal. Rotation, reflection, skew and transforms inside text blocks
remain refused.

Windows passed those 44 tests, 10 shared text-layout tests and all 15 native
checks from an isolated snapshot over `efbec7b`, archive SHA-256
`59404f4a4ab4a29b09f10f68a9b8578755c0473ed9fc5641391d2a5b459f85d1`.
The current implementation matches that snapshot. Both Windows and both Mac
worker/UI saves passed independent parser and PDFKit readback: exactly the target
text operand changed, fonts/transforms stayed intact, 2,299 pixels changed inside
the first line and zero outside. Transfer sizes/digests matched, the Windows
worker-exit observer passed and the temporary task was removed.

Seven targeted mutations were caught, covering transform order, saved state,
hit-box width/height, unsupported matrices and composed bounds. The
`editable-scaled` seed was added to the corpus; the fuzz run completed 29,036
executions in 21 seconds, peaking at 86 MiB RSS without a finding. All 24
repository gates passed in 360.1 seconds:
1,340 Rust tests (three ignored), 1,667 frontend tests and locked fuzz/example
builds. The Mac native run passed 15/15; its separate checks application compiled
only `tpdf`, finishing the Rust build in 17.53 seconds. Normal frontend assets
were restored and contain zero harness code.

Explicit default text state (2026-09-13, unreleased) accepts `0 Tc`, `0 Tw`,
`100 Tz`, `0 Ts` and integer `0 Tr`, inside or outside text blocks. Nondefault
values, malformed operands and shows without independent positioning remain
refused. ReportLab's `defaults-ascii85.pdf` emits the four spacing/scale/rise
defaults through its public API; the preceding worker refused that fixture.
All 47 focused tests pass, including unchanged geometry, preserved non-target
operators and numeric serialization normalization (`0.0` becomes `0`).

Windows passed those 47 tests, 10 shared layout tests and 15 native checks from
an isolated snapshot over `efbec7b`, archive SHA-256
`1aefd1991f60bc1d5298a32ebfe0ed2663ac737e3bb86b80579ccc4444a8ff8a`.
The implementation matches that snapshot. All four Mac/Windows worker/UI saves
passed independent parser and PDFKit readback: only the target text operand
changed, with 2,394 changed pixels inside the line and zero outside. Transfer
digests matched and the temporary Windows task was removed.

Four targeted mutations were caught. With `editable-defaults` added to the
corpus, fuzzing completed 30,061 inputs in 21 seconds without a finding, peaking
at 84 MiB RSS. All 24 repository gates passed in 346.7 seconds: 1,343 Rust tests
(three ignored), 1,667 frontend tests and locked fuzz/example builds. The Mac
native run passed 15/15; its Rust checks build took 16.18 seconds and rebuilt no
dependencies. Normal frontend assets were restored with zero harness code.

### Independent text producer survey

`text-edit-probe --inspect <fixture.pdf>` asks the contained worker about page zero
without saving or printing document text. It emits JSON with `status` equal to
`editable`, `no_runs` or `refused` (including the first refusal reason). Exit 0
means inspection completed; inspect `status` to determine compatibility.
Infrastructure failures and invalid arguments exit 1. This is discovery evidence,
not a save/readback check or an inventory of every unsupported construct.

Add `--all-pages` to inspect every page, with a 128-page bound. The reply includes
`page_count` and one result per page; later refusals and pages with no runs remain
visible. Worker failure emits no partial report. The original page-zero mode is
unchanged. `scripts/textedit_survey.py` collects these reports for explicitly chosen
inputs, verifies their SHA-256 digests before and after inspection, and refuses
missing, repeated or reordered page results. Existing output paths are refused so
a failed rerun cannot be confused with a freshly written report.

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
uv run --with pypdf scripts/textedit_survey.py src-tauri/target/debug/examples/text-edit-probe --self-test
python3 scripts/textedit_survey.py src-tauri/target/debug/examples/text-edit-probe testdata/comments.pdf testdata/inherited.pdf testdata/rotated.pdf --output scratch/textedit-survey.json
```

The self-test exercises the real contained worker on four pages: editable,
unsupported font, no text, then editable again. It also checks the 128-page
boundary, refuses 129 pages without a partial report, preserves source bytes,
and checks missing files, invalid arguments and incomplete-report controls.

Generate synthetic exports on macOS (LibreOffice is an external producer only,
not an application dependency):

```bash
mkdir -p scratch/textedit-producers/tagged scratch/textedit-producers/untagged
swift testdata/make_textedit_quartz.swift scratch/textedit-producers/quartz.pdf
/Applications/LibreOffice.app/Contents/MacOS/soffice -env:UserInstallation=file:///tmp/tpdf-producer-lo --headless --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"true"}}' --outdir scratch/textedit-producers/tagged testdata/textedit-producer.rtf
/Applications/LibreOffice.app/Contents/MacOS/soffice -env:UserInstallation=file:///tmp/tpdf-producer-lo --headless --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"false"}}' --outdir scratch/textedit-producers/untagged testdata/textedit-producer.rtf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- --inspect scratch/textedit-producers/quartz.pdf
```

The isolated LibreOffice profile avoids reusing the reader's export preferences.
Its explicit tagging parameter is documented in the
[LibreOffice PDF CLI reference](https://help.libreoffice.org/latest/en-US/text/shared/guide/pdf_params.html).
Repeat inspection with each exported PDF. `pypdf` independently confirmed one page
and the two synthetic lines in all three outputs on 2026-09-13:

| Producer measured | Worker result | Additional constructs observed in the original output |
| --- | --- | --- |
| macOS 26.6.2 Quartz/CoreText | Refused: unsupported state/positioning | `cs`/`sc`, MacRoman TrueType subset, `TJ` kerning arrays |
| LibreOffice 26.2.3.2, tagged | Refused: tagged text | Clipping, custom character codes/ToUnicode, `TJ`, marked content |
| LibreOffice 26.2.3.2, untagged | Refused: unsupported state/positioning | Clipping, custom character codes/ToUnicode, `TJ` |

The ReportLab explicit-default fixture remained an editable two-run positive
control. A blank page reported `no_runs`; missing files and invalid arguments
failed. Every inspected source retained its SHA-256 digest. Disabling LibreOffice
tags alone did not make its export editable. These are compatibility baselines;
the editor's accepted grammar was not expanded by this survey.

All 24 local gates passed (283.5 seconds summed gate time). The existing editing
probe also passed its ReportLab round trip; independent parser/PDFKit readback
found only the target operand changed and zero pixel changes outside its line.

### Bounded kerning-array editing

The synthetic `TJ` fixture isolates kerning-array support from the remaining
Quartz colour-space/font-mapping work. It uses fractional offsets of both signs
and a second, independently positioned array. Reproduce worker and independent
readback checks with:

```bash
uv run --with pypdf testdata/make_textedit_kerning.py scratch/textedit-kerning/fixture.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-kerning/worker scratch/textedit-kerning/fixture.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-kerning/worker/synthetic-before.pdf scratch/textedit-kerning/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-kerning/worker
```

The same fixture works with `tabs_check.py --phase textedit --saved-copy`, using
the separate checks build. Arrays are replaced as a whole with normal font spacing;
the replacement must fit their original adjusted advance. `docs/PLAN.md` records
the accepted shapes and bounds. The independent parser requires exactly one
changed operand and rejects an unchanged input supplied as the supposed output.

On 2026-09-13 the preceding worker refused this fixture. The new implementation
passed 52 focused tests per platform, including embedded glyph validation,
adjustment sign/scale, array/character limits, retreating bounds and overflow.
Seven targeted mutations were caught. Fuzzing with the new `editable-kerning`
seed completed 27,573 inputs in 21 seconds without a finding (84 MiB peak RSS).
Windows passed 15 native checks on isolated source archive SHA-256
`0c0ee3352b50ac3efa9f116af5be28ea0a5a69566ca92c1dafd258c037e88bda`;
both worker and UI saves passed independent parser/PDFKit readback, with 2,387
changed pixels inside the edited line and zero outside. Transfer digests matched,
the implementation matched the snapshot and the temporary task was removed.

The Mac passed the same worker round trip and 15 native checks; both saves passed
the independent readers with the same pixel counts. All 24 local gates passed
in 378.4 seconds: 1,348 Rust tests (three ignored), 1,667 frontend tests, and the
locked fuzz/example builds. The checks application's Rust build took 18.73 seconds.
Normal frontend assets were restored with zero harness code.

### Quartz/CoreText text-edit round trip

The original synthetic Quartz export from the producer survey now supports
preview, shorter replacement and save without modifying its font or colour
resources. This covers its Apple TrueType `true` program, explicit MacRoman map,
ICCBased grey colour and kerning arrays. Only validated ASCII glyphs already in
the subset are offered. The accepted grammar and remaining exclusions are in
`docs/PLAN.md`; this is evidence for these exports, not arbitrary Quartz PDFs.

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-quartz/worker scratch/textedit-producers/quartz.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-quartz/worker/synthetic-before.pdf scratch/textedit-quartz/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-quartz/worker
swift testdata/make_textedit_quartz.swift scratch/textedit-quartz/colour.pdf --colour
```

Repeat the worker and independent-reader commands with the coloured export.
`--colour` asks CoreText for two different foreground colours; it does not patch
PDF operators after export. The parser now compares all page resources, including
decoded font and ICC streams, and rejects a deliberately changed ICC profile.
On 2026-09-13 the grey and coloured worker saves changed respectively 2,394 and
2,471 pixels inside the edited line, with zero outside.

Both platforms passed 58 focused text-editor tests. Nine targeted mutations were
caught; fuzzing with the additional MacRoman/colour seed completed 30,509 inputs
in 21 seconds without a finding, peaking at 84 MiB RSS. The worker and native
checks now repeat `S` for the overflowing draft: it exists in the original line,
whereas `A` is absent from Quartz's subset and could make the overflow check pass
for the wrong reason.

Windows passed the corrected 15-case native workflow on isolated source archive
SHA-256 `7aa79fe9c70e94365049dd014d2980d8e46470b569ee80128c5dab54902369ea`.
The exact original Quartz input was transferred with SHA-256
`754757537738043fa04e050187b1398422703e5d66a22984099ae3ea20de4638`.
Both worker and UI output passed independent parser and PDFKit readback, with
2,394 changed pixels inside the line and zero outside. Retrieved digests and
source manifests matched; temporary tasks were removed and the normal checkout
remained clean. Normal frontend assets were restored with zero harness code.

The Mac passed the corrected 15 native checks and both independent readers, with
2,394 changed pixels inside the edited line and zero outside. All 24 local gates
passed in 487.4 seconds summed gate time: 1,354 Rust tests (three ignored), 1,667
frontend tests and locked fuzz/example builds. The checks application's Rust build
took 21.07 seconds. Normal frontend assets were restored with zero harness code.
Implementation files still match the corrected Windows snapshot; the subsequent
plan and verification-record updates do not change that tested implementation.

### Single-byte symbolic font editing

The mapped-font path keeps PDF bytes separate from Unicode text. It accepts the
measured LibreOffice-style symbolic TrueType arrangement: no `/Encoding`, one
Macintosh cmap for glyph selection, and a strict one-to-one ASCII `ToUnicode`
map. Existing glyph width, outline and embedding checks still apply. Both `Tj`
and `TJ` decode through the map; replacements reuse the original font codes.
The exact limits and exclusions are in `docs/PLAN.md`.

Generate the original geometric-font control and run independent readback:

```bash
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-symbolic/fixture.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-symbolic/worker scratch/textedit-symbolic/fixture.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-symbolic/worker/synthetic-before.pdf scratch/textedit-symbolic/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-symbolic/worker
```

On 2026-09-13 this worker round trip changed 935 pixels inside the edited line
and zero outside. Its font and mapping remained unchanged. A separate control
removed only the initial `w`, `re`, `W*`, `n` operations from the untagged
LibreOffice survey export, preserving its `q`/`Q` and all text/resource data.
PDFKit found identical text and pixels before editing that control. Its worker
save then passed both independent readers, with 2,403 changed pixels inside the
line and zero outside. The original, unmodified export is still refused; the
control isolates character mapping and does not establish clipping support.

The parser readback now requires both exactly one changed text operand and the
expected decoded text. For symbolic fixtures it also reads pypdf's parsed mapping
explicitly and requires every operand byte to be mapped. `extract_text()` alone
silently falls back to ASCII for unknown codes: a deliberately misencoded output
passed it. The stricter check rejects that output, while the mapped-font,
LibreOffice control, Quartz and kerning outputs pass. The unchanged-output control
is also rejected.

All 64 focused text-editor tests passed. Nine targeted mutations were caught,
including duplicate source/Unicode entries, competing font maps, incorrect raw
output codes, declared entry counts and decoding limits. The mapped-text length
test now checks its specific refusal reason: a generic error assertion remained
green when that early bound was removed because later width calculation applied
the shared text-length limit again.

Fuzzing with the new `editable-symbolic` seed completed 28,176 inputs in 21 seconds
without a finding, peaking at 85 MiB RSS. The seed itself reports `editable` through
the contained worker inspection command.

Windows passed 64 focused editor tests, 10 shared layout tests and all 15 native
editing checks on isolated source archive SHA-256
`2f4b480d8bcb0b98f70dac49324496258fc7d848c5ca0efb698b6dcd7d62005d`.
The transferred synthetic PDF had SHA-256
`a3af5cf7a4c670732b2942b984c68338e4a0ca5a266a7dc80967e394f9c49574`.
Both worker and native UI output passed independent parser and PDFKit readback:
935 changed pixels inside the edited line, zero outside, and unchanged resources.
Transfer digests and source manifests matched. The temporary task was removed,
the normal checkout stayed clean and normal frontend assets contain zero harness
code.

The Mac passed the same 15 native editing checks and independent readers, again
with 935 changed pixels inside the line and zero outside. All 24 local gates
passed in 380.3 seconds summed gate time: 1,360 Rust tests (three ignored),
1,667 frontend tests and locked fuzz/example builds. The checks application's
Rust build took 20.06 seconds. Normal frontend assets were restored with zero
harness code. Implementation files match the Windows snapshot; only this later
verification record differs.

### Unmodified LibreOffice export and rectangular clips

The clipping path accepts only complete `re W n` or `re W* n` sequences outside
text blocks. Positive rectangles are transformed into original page coordinates,
intersected with existing clips and restored with `q`/`Q`. Every affected run must
fit its entire editing envelope inside the clip. This requires validated embedded
glyph outlines; standard Helvetica's advance-only metrics are insufficient here.
The writer preserves the path and its position among the other operators. A
bounded nonnegative line-width setter is also accepted because all text remains
filled and every stroking operator is refused. Detailed limits are in `docs/PLAN.md`.

Run the unchanged untagged LibreOffice fixture from the producer survey:

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-libreoffice/worker scratch/textedit-producers/untagged/textedit-producer.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-libreoffice/worker/synthetic-before.pdf scratch/textedit-libreoffice/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-libreoffice/worker
```

On 2026-09-13 the original export, SHA-256
`3c531a18f460179e9daa000435e9962af30b464b1f816f6facad77e549e0dbd0`, passed the
worker round trip without removing or modifying its clipping operators first.
The input and the probe's before-copy have the same digest. Independent parser
and PDFKit readback found one changed text operand, unchanged resources, 2,403
changed pixels inside the edited line and zero outside. At this stage the tagged
survey export was still refused; the tagged paragraph increment below follows it.

Eight targeted clipping mutations were caught. Coverage includes every edge of
the text envelope, intersections, saved clip state, transforms fixed at path
creation, crop/rotation, malformed and painted paths, coordinate limits and the
embedded-outline requirement. The new `editable-clipped` seed reports `editable`
through worker inspection. Fuzzing completed 24,711 inputs in 21 seconds without
a finding, peaking at 84 MiB RSS.

Windows passed 70 focused editor tests, 10 shared layout tests and all 15 native
editing checks using the unchanged producer PDF. Source archive SHA-256:
`57f18316d78a53b9e20d1663faef3f218c95e10a651a2c388273632f1ff0c78c`.
Both worker and native UI output passed independent parser and PDFKit readback,
with 2,403 changed pixels inside the line and zero outside. Transfer digests and
source manifests matched, the temporary task was removed, and the normal checkout
remained clean. Normal Windows frontend assets contain zero harness code.

The Mac native UI passed the same 15 checks against the unchanged export. Its
saved PDF also passed independent parser and PDFKit readback: 2,403 changed pixels
inside the edited line and zero outside. All 24 local gates passed in 487.5 seconds
summed gate time, including 1,366 Rust tests (three ignored), 1,667 frontend tests
in 70 suites, and locked fuzz-target and example builds. The checks application
built in 21.71 seconds of Rust build time; normal frontend assets were then
restored and verified to contain zero harness code.

Final diff review corrected an overly broad anchor in the existing graphics-state
restoration mutation. All five existing mutations whose anchors changed with this
implementation were then run and caught by their named tests; compilation errors
were not counted as catches. Application sources still match the Windows snapshot.
Only the verification notes, plan and mutation script changed after that snapshot.

### Tagged paragraph text editing

The first tagged grammar is a bounded single-page Document/paragraph tree; its
limits and remaining refusals are in `docs/PLAN.md`. It preserves structure and
marked-content operators while replacing only the target text operand. The
parent-tree check follows ISO 32000-1 section 14.7.4.4; both forward and reverse
references must agree. Alternate text and potentially stale layout attributes are
refused before any document mutation.

Referenced metadata and omitted structure-element types use a synthetic fixture:

```bash
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/tag-metadata/source.pdf --tagged-indirect
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/tag-metadata/worker scratch/tag-metadata/source.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/tag-metadata/worker/synthetic-before.pdf scratch/tag-metadata/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/tag-metadata/worker
```

Use the ordinary `tabs_check.py --phase textedit` workflow on that source.
The fixture shares an indirect layout dictionary, references its role map and
parent-tree number array, and omits `/Type` on its structure elements. The
independent graph comparison must preserve those references and omissions.
Wrong types, unresolved/cyclic references, semantic overrides and inconsistent
parent mappings remain refused. The public factsheet, mouse guide and W-9 use
some of these representations but also exceed the supported tree grammar;
this fixture does not establish that those documents are editable.

Grouping containers and headings use two complementary fixtures:

```bash
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/tag-containers/source.pdf --tagged-containers
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py <browser-executable> scratch/tag-containers/browser --headings
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/tag-containers/browser-worker scratch/tag-containers/browser/browser-tagged.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/tag-containers/browser-worker/synthetic-before.pdf scratch/tag-containers/browser-worker/synthetic-after.pdf --float32
swift scripts/text_edit_pdfkit.swift scratch/tag-containers/browser-worker --browser
```

Both sources use the ordinary native `textedit` phase. The symbolic fixture
combines Part/Art/Sect/Div/NonStruct containers with aliased heading and section
roles; its independent parser check needs no float32 opt-in and PDFKit needs no
variant flag. The unchanged browser export carries Document/Art/NonStruct above
H1/P, each with a NonStruct content leaf. Keep the browser's output unchanged;
its tag names and font streams are part of this control.
Container depth/count tests have accepting boundary controls at 8/128 and reject
9/129. A separate frontier test requires rejection before oversized child arrays
enter the work list. Standard-role remapping is tested on an unused name so a
later shape refusal cannot mask a missing role-map check.

Numbered browser lists use `L/LI` with separate `Lbl` and neutral `NonStruct`
content. Literal `LBody` content leaves are supported as well. The existing
container depth/count and total-content bounds apply; parent links and every
MCID still have to agree in both directions. Only one `O=List` attribute object
with a standard `ListNumbering` name is admitted, directly or in a singleton
array, including references. List-role aliases, blocks below LBody, revision
arrays and leaf attributes remain refused. Nested L containers may be direct LI
children, with each item retaining its own content. Lists at every level share
the existing container bounds; LI does not add a container level.

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py <browser-executable> scratch/tag-lists/browser --list
uv run scripts/tabs_check.py <checks-binary> scratch/tag-lists/browser/browser-tagged.pdf --phase textedit --saved-copy scratch/tag-lists/native/synthetic-after.pdf
cp scratch/tag-lists/browser/browser-tagged.pdf scratch/tag-lists/native/synthetic-before.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/tag-lists/native/synthetic-before.pdf scratch/tag-lists/native/synthetic-after.pdf --float32 --list
swift scripts/text_edit_pdfkit.swift scratch/tag-lists/native --list
```

Measured on macOS: an unchanged Edge 153 numbered-list export passes all 15
native checks. Independent readback preserves the complete structure and both
list numbers; PDFKit finds 2,423 changed pixels inside the first item's body and
zero outside. Its comparison region excludes the label. Moving the first label
by three source units produces 170 changed pixels outside and fails; changing
the numbering metadata or reversing the list items fails the independent graph
comparison. The native harness selects the expected text by its accessible name
because the first editable run can now be the list number. The earlier browser
heading fixture remains a separate regression control.

Nested list verification uses an unchanged browser export with the second item
inside a list owned by the first item:

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py <browser-executable> scratch/tag-nested-lists/browser --nested-list
uv run scripts/tabs_check.py <checks-binary> scratch/tag-nested-lists/browser/browser-tagged.pdf --phase textedit --saved-copy scratch/tag-nested-lists/native-parent/synthetic-after.pdf
uv run scripts/tabs_check.py <checks-binary> scratch/tag-nested-lists/browser/browser-tagged.pdf --phase textedit-list-child --saved-copy scratch/tag-nested-lists/native-child/synthetic-after.pdf
cp scratch/tag-nested-lists/browser/browser-tagged.pdf scratch/tag-nested-lists/native-parent/synthetic-before.pdf
cp scratch/tag-nested-lists/browser/browser-tagged.pdf scratch/tag-nested-lists/native-child/synthetic-before.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/tag-nested-lists/native-parent/synthetic-before.pdf scratch/tag-nested-lists/native-parent/synthetic-after.pdf --float32 --nested-list
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/tag-nested-lists/native-child/synthetic-before.pdf scratch/tag-nested-lists/native-child/synthetic-after.pdf --float32 --nested-list-child
swift scripts/text_edit_pdfkit.swift scratch/tag-nested-lists/native-parent --nested-list
swift scripts/text_edit_pdfkit.swift scratch/tag-nested-lists/native-child --nested-list-child
```

Both native phases pass 15 checks on macOS. Independent readback preserves all
structure objects and labels, with 2,423 changed pixels inside the parent edit
and 2,906 inside the child edit, zero outside in both. Flattening the list or
changing its numbering fails graph readback. Moving the child label produces
170 pixels outside the child edit; moving the parent text produces 2,657.
The depth test edits a valid eight-level chain and refuses nine levels. Separate
queue controls cover a pending sibling as well as child count; cross-page tests
edit the nested body without changing the parent page. The original public
survey remains at 1 editable page of 45.

Use the unchanged tagged LibreOffice fixture from the producer survey:

```bash
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-tagged/worker scratch/textedit-producers/tagged/textedit-producer.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls scratch/textedit-tagged/worker/synthetic-before.pdf scratch/textedit-tagged/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-tagged/worker
```

On 2026-09-13 the original input, SHA-256
`166b15ccaf6526d6d4ef4994dad54ed17f5f62d77555d5681aeec5721b754a77`, passed the
worker preview, undo and save round trip. Independent parser readback compares the
complete cyclic structure graph independent of object numbering, with page
references as explicit leaves. The source and output agree, and five deliberately
corrupted copies fail: deleted structure root, wrong parent reference, changed
MCID, stale ActualText and changed StructParents. PDFKit reports 2,403 changed
pixels inside the edited line and zero outside.

Eight targeted structure mutations were caught by their named tests, including
the paragraph count limit, both parent-reference directions, page key, semantic
field whitelist, artifact text, duplicate IDs and incomplete markers. A valid
128-paragraph fixture passes while its 129-paragraph counterpart is refused.
The `editable-tagged` fuzz seed also reports editable through worker inspection.

Windows passed 75 editor tests, 10 shared layout tests and all 15 native editing
checks on the unchanged tagged export. Its source archive SHA-256 is
`58416c37d51dbaf2b6c74d54b9aee8dc6032135035dc8249696f83c78989ba7d`.
Both worker and native UI output passed independent structure/resource readback
and PDFKit: 2,403 changed pixels inside the edited line and zero outside. Retrieved
PDF digests and source manifests matched. The normal Windows frontend contains
zero harness code, the temporary task was removed, and the normal checkout stayed
clean. The local bounded fuzz run completed 25,453 inputs in 21 seconds
without a finding, peaking at 83 MiB RSS.

The Mac native UI passed all 15 checks on the same unchanged export. Its output
passed independent structure/resource readback, all five corruption controls and
PDFKit (2,403 changed pixels inside the line, zero outside). All 24 local gates
passed in 468.8 seconds summed gate time: 1,371 Rust tests (three ignored), 1,667
frontend tests in 70 suites, and locked fuzz-target and example builds. The Mac
checks app built in 1 minute 49 seconds of Rust build time. Normal frontend assets
were restored and verified to contain zero harness code. Implementation sources
match the Windows snapshot; subsequent changes only record verification and the
completed plan milestone.

### Multi-page tagged text editing

This extends the bounded Document/paragraph grammar to a flat parent number tree
with one array per page. MCIDs may repeat across pages. Page keys need not be
consecutive, but must be unique and sorted in the number tree. The global limit
remains 128 paragraphs, with no recursive number-tree or structure traversal.

Generate the independent synthetic export and edit page two (zero-based index 1):

```bash
mkdir -p scratch/textedit-producers/multipage
/Applications/LibreOffice.app/Contents/MacOS/soffice -env:UserInstallation=file:///tmp/tpdf-producer-lo --headless --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"true"}}' --outdir scratch/textedit-producers/multipage testdata/textedit-producer-multipage.rtf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-multipage/worker-page2 scratch/textedit-producers/multipage/textedit-producer-multipage.pdf --page=1
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls scratch/textedit-multipage/worker-page2/synthetic-before.pdf scratch/textedit-multipage/worker-page2/synthetic-after.pdf --page=1
swift scripts/text_edit_pdfkit.swift scratch/textedit-multipage/worker-page2 --page=1
```

On 2026-09-13 the unchanged producer input, SHA-256
`4e52fa7a70750a3f877a0adfde9e41060140a92094041926f3c2290447803341`, passed worker
edits to each page separately. Every unedited page retains its original runs and
decoded stream bytes. Independent parser readback preserves the full structure
graph, distinguishing references to each page; PDFKit sees 2,403 changed pixels
inside the target line and zero elsewhere, including the entire other page.
Geometry is compared at the writer's f32 precision: the existing writer rounds
299.990551 points to 299.99054, so exact decimal equality would reject a valid
control. Pixel comparisons remain exact.

All 13 targeted tagged-structure mutations were caught. The new tests include
shared streams, reused MCIDs, interleaved reading order, nonconsecutive page keys,
distinct tag names per page and atomic multi-page batches. Seven independent
corruption controls fail, including a paragraph assigned to the wrong page and
a byte change on the untouched page. The native phase is `textedit-multipage`;
it edits page two and checks that page one stays unchanged before and after save.

Mac and Windows native runs each passed all 19 checks on the unchanged export.
Windows also passed 78 editor tests and 10 shared layout tests. Both Windows
saved outputs (worker and native UI) passed independent parser readback and
PDFKit: page one is pixel-identical, and page two has 2,403 changed pixels inside
the target line and zero outside. All seven corruption controls also rejected
the damaged native outputs on both platforms.

The Windows source archive SHA-256 is
`31f1b89d34d1217362ab9e58427fa27dbee29caa5f37b40283d27be5f828dd98`.
Retrieved PDF digests and source manifests matched. The temporary task was
removed, the normal checkout stayed clean, and normal Windows frontend assets
were restored with zero harness code. The local bounded fuzz run completed
23,247 inputs in 21 seconds without a finding, peaking at 83 MiB RSS.

All 24 local gates passed in 431.6 seconds summed gate time: 1,374 Rust tests
(three ignored), 1,667 frontend tests in 70 suites, and locked fuzz-target and
example builds. Frontend diagnostics reported no errors or warnings. Normal Mac
frontend assets were restored and verified to contain zero harness code.
Implementation sources match the Windows snapshot; subsequent changes only
record verification and the completed plan milestone.

### Line-broken and page-spanning paragraph editing

A paragraph can now own several content items: integers on its own page, or
inline MCR dictionaries with explicit Type, Pg and MCID on another page. Every
item must agree with its reverse parent-tree entry. There are at most 128 items
in total, rather than 128 items per paragraph. Empty paragraphs, duplicate item
ownership, indirect MCRs, missing page references and external-stream references
are refused. Alternate text and unsupported layout attributes remain refused.
The page-reference interpretation follows ISO 32000-1 section 14.7.4.2; the
[PDF Association clarification](https://pdf-issues.pdfa.org/32000-2-2020/clause14.html#14.7.5.2-marked-content-sequences-as-content-items)
also describes the relationship between Pg and page content streams.

Generate unchanged synthetic producer exports:

```bash
mkdir -p scratch/textedit-producers/flow
/Applications/LibreOffice.app/Contents/MacOS/soffice -env:UserInstallation=file:///tmp/tpdf-producer-flow-lo --headless --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"true"}}' --outdir scratch/textedit-producers/flow testdata/textedit-producer-wrapped.rtf testdata/textedit-producer-flow.rtf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-flow/worker-page2 scratch/textedit-producers/flow/textedit-producer-flow.pdf --page=1
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls scratch/textedit-flow/worker-page2/synthetic-before.pdf scratch/textedit-flow/worker-page2/synthetic-after.pdf --page=1
swift scripts/text_edit_pdfkit.swift scratch/textedit-flow/worker-page2 --page=1
```

On 2026-09-13 LibreOffice 26.2.3.2 exported a line-broken paragraph with K=[0,1].
With a larger bottom margin, four lines overflow naturally into two pages while
remaining one paragraph: two integer items followed by two MCR dictionaries for
page two. An explicit page-break control instead produced separate paragraphs;
it is not evidence for a paragraph spanning pages. No exported PDF was patched.
The measured single-page input SHA-256 is
`3ce717c92c3cc8a13fd84b03053407be9eb0c5164c35b2bf99a3e19b7f5147ec`;
the page-spanning input is
`a7e6c1e73501b589f075cd9eb93f6233e0139e2c0354a06fcd3a2e8fec0f0563`.

The first page-two worker round trip passed complete structure/resource readback
and PDFKit: page one is pixel-identical, and page two changes 2,403 pixels only
inside the edited line. Eleven independent corruption controls reject changes
to paragraph or MCR page ownership, item IDs, item order, missing items, stale
alternate text, the parent tree, and untouched page bytes. The native phase
remains `textedit-multipage`, since the visible two-page workflow is the same.

A separate naturally word-wrapped sample with a right paragraph indent adds
EndIndent and trailing spaces to the text-show operands. It remains outside this
milestone; these examples use explicit line breaks within the paragraph and
natural overflow between pages. Preserving tags does not make this a reflowing
paragraph editor or establish PDF/UA conformance.

The focused suite passes 83 editor tests, including atomic two-page batches,
128/129-item boundary controls and malformed MCR refusals. All 20 targeted
structure mutations were caught. A new failing control exposed empty role names
colliding with the validator's unclaimed-slot marker; empty role names are now
refused and the control passes. The bounded fuzz run completed 32,065 inputs in
21 seconds without a finding, peaking at 85 MiB RSS.

Mac and Windows native runs each pass all 19 checks on the unchanged page-spanning
export. The initial Mac run caught a harness timing defect: its idle callback
drains pending edits but does not wait for the viewer to publish the page number.
The corrected harness waits for both the displayed target page and an idle viewer
before invoking the edit command. Both platforms were rerun with that correction.
Their native outputs pass all eleven corruption controls and independent PDFKit
readback: page one is pixel-identical; page two changes 2,403 pixels only inside
the target line. The single-page line-break export and a separate page-one edit
also pass worker, parser and pixel checks.

Windows additionally passes 83 editor tests and 10 shared layout tests. The final
source archive SHA-256 is
`bb13a1e456b04eb65af862dc92223f92597335ece5c8ef4016d10d9ad3eef842`.
Retrieved PDF digests and source manifests match the snapshot. Both Windows
worker and native outputs pass independent PDFKit readback. Normal Windows
frontend assets contain zero harness code; the normal checkout remains clean.

All 24 local gates passed in 394.6 seconds summed gate time: 1,379 Rust tests
(three ignored), 1,667 frontend tests in 70 suites, and locked fuzz-target and
example builds. Frontend diagnostics reported no errors or warnings, and normal
Mac assets were restored with zero harness code. The final Windows task was
removed. Implementation sources match the verified Windows snapshot; subsequent
changes only record these results and the completed plan milestone.

### Naturally wrapped paragraphs and end indents

The measured LibreOffice export uses an authored EndIndent on a block paragraph.
It describes an allocation constraint, rather than the current text's ink bounds;
see the [PDF Association structure-attribute reference](https://pdfa.org/download-area/cheat-sheets/StructureAttributes.pdf).
The reader now accepts and preserves a finite numeric EndIndent on paragraphs,
bounded to an absolute value of 1,000,000 like text coordinates. Document-level
EndIndent, unknown attributes, ink bounds and alternate text remain refused.

Literal trailing spaces were already supported by the editor. The worker and
native probes previously assumed fixture lines had none; their wrapped variant
now requires the exact source space and verifies it survives in the journal.
The independent parser also checks the exact original operand through ToUnicode,
so whitespace normalization in text extraction cannot hide a changed source.
A deliberately trimmed original is refused as stale before any object changes.

Reproduce the unchanged single-page and page-spanning synthetic exports:

```bash
mkdir -p scratch/textedit-producers/natural
/Applications/LibreOffice.app/Contents/MacOS/soffice -env:UserInstallation=file:///tmp/tpdf-producer-natural-lo --headless --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"true"}}' --outdir scratch/textedit-producers/natural testdata/textedit-producer-natural.rtf testdata/textedit-producer-natural-flow.rtf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-natural/worker-page2 scratch/textedit-producers/natural/textedit-producer-natural-flow.pdf --page=1 --wrapped
uv run --with pypdf testdata/make_textedit_embedded.py --tagged-controls scratch/textedit-natural/worker-page2/synthetic-before.pdf scratch/textedit-natural/worker-page2/synthetic-after.pdf --page=1 --wrapped
swift scripts/text_edit_pdfkit.swift scratch/textedit-natural/worker-page2 --page=1
```

The source RTF has ordinary spaces, a right paragraph indent and no explicit line
or page breaks. A larger bottom margin makes the longer paragraph flow across
pages. On 2026-09-13 LibreOffice 26.2.3.2 produced the single-page input SHA-256
`6ed82049345584fed015453bc7278cd04552c008908f32ca10911eeb2ed94ee0`
and page-spanning input
`311d3189bbdca04890c243111d122ce46a956f9e05fc88c52e8bf7ff2f2d85f6`.
The native phase is `textedit-wrapped`; it edits page two and checks the journal's
literal source space as well as the existing multi-page workflow.

**The phase needs an export that keeps the trailing space inside its line's show, and
not every LibreOffice writes one.** Measured 2026-09-24: a fresh export from LibreOffice
26.2.3.2 on macOS (SHA-256 `f8818e08…`, which differs from the hash above only by its
metadata) keeps the space inside the `TJ` and passes 28/28. LibreOffice 26.8.0.3 on
Windows writes the space as a text object of its own (`BT 146.1 180.759 Td /F1 12 Tf<09>Tj
ET`), which becomes a separate run labelled `Edit:  `. The phase then finds no
`SYNTHETIC FIRST ` target and fails at *fresh editable text targets did not appear*.
That is a difference in the fixture, not a product defect: the same Windows build passed
28/28 on the macOS export copied across. On Windows, run the phase against a copy of the
macOS export. The independent
corruption controls now also remove EndIndent and require that to fail, even
though no rendered pixel changes.

Worker edits to either page of the unchanged two-page export, and to its
single-page counterpart, pass preview, undo, search and save checks. Independent
parser readback compares the complete tagged graph and exact mapped operand;
assuming a trimmed original deliberately fails. PDFKit reports 2,403 changed
pixels inside the edited line and zero outside, including the entire other page.
All twelve page-spanning corruption controls fail, including removal of EndIndent;
the single-page output passes its eight applicable controls.

Mac and Windows native runs each pass all 20 checks, including the exact source
space retained in the journal. Their outputs pass all twelve independent
corruption controls and the same pixel comparison. Windows also passes 85 editor
tests and 10 shared layout tests. All 23 targeted structure mutations were caught,
including skipped indent validation, document-level indent acceptance and trimmed
stale-source comparison. The bounded fuzz run completed 31,940 inputs in
21 seconds without a finding, peaking at 85 MiB RSS.

The Windows source archive SHA-256 is
`cacd9ed077c5deb6ed38704415754471883544a6b6027d1c032492149f028a48`.
Retrieved PDF digests and source manifests matched. Both Windows worker and
native outputs passed independent PDFKit readback; normal Windows frontend
assets contain zero harness code and its normal checkout stayed clean.

All 24 local gates passed in 602.9 seconds summed gate time: 1,381 Rust tests
(three ignored), 1,667 frontend tests in 70 suites, and locked fuzz-target and
example builds. Frontend diagnostics reported no errors or warnings; normal
Mac assets were restored with zero harness code. The Windows task was removed.
Implementation sources match the verified Windows snapshot; subsequent changes
only record these results and the completed plan milestone.

### Word and browser text producer coverage

On 2026-09-13, Word 16.112.3 on macOS exported the existing synthetic RTF
without changing the resulting PDF. The export uses a simple embedded Helvetica
subset with MacRomanEncoding and four text shows: the two lines, each followed
by a separate single-space show. The application already supports this grammar.
`text-edit-probe --spacers` checks that exact source sequence and compares every
untargeted run after saving. Omitting the flag deliberately fails discovery.

Export through Word's local PDF save command, retaining the source RTF:

```bash
mkdir -p scratch/textedit-word
osascript - "$PWD/testdata/textedit-producer.rtf" "$PWD/scratch/textedit-word/word.pdf" <<'APPLESCRIPT'
on run argv
  tell application id "com.microsoft.Word"
    open file name (item 1 of argv) read only true add to recent files false
    set fixtureDocument to active document
    save as fixtureDocument file name (item 2 of argv) file format format PDF add to recent files false
    close fixtureDocument saving no
  end tell
end run
APPLESCRIPT
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-word/worker scratch/textedit-word/word.pdf --spacers
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-word/worker/synthetic-before.pdf scratch/textedit-word/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-word/worker
```

The measured Word input SHA-256 was
`dce5859a8857241c5e6df0092b8878f464b1ffe6ccfd3685323188d760411dba`.
The worker passes preview, search, undo, save and refusal checks. All 15 Mac native
`tabs_check.py --phase textedit` checks pass on the same unchanged input. Worker
and native outputs pass independent parser readback; PDFKit measures 2,413 changed
pixels inside the target line and zero outside. Four corruption controls fail
for their intended reasons: deleting a spacer changes the operator count;
emptying a spacer or moving the second line adds a changed operand; changing a
font width changes resources. A first native attempt selected an old release
build; the recorded passing run uses the current debug checks application.
No Windows native run was performed for this Word sample.

Generate tagged and untagged browser exports through an isolated browser profile:

```bash
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge' scratch/textedit-browser
uv run --with pypdf scripts/text_edit_producers.py --probe src-tauri/target/debug/examples/text-edit-probe scratch/textedit-word/word.pdf scratch/textedit-browser/browser-tagged.pdf scratch/textedit-browser/browser-untagged.pdf
```

The exporter uses the browser's
[Page.printToPDF API](https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-printToPDF)
and independently checks both tag presence and extracted synthetic text. A
command-line attempt with `--disable-features=PrintTaggedPDF` left tags present
in Edge 153.0.4234.32; it is not an untagged control. The explicit API produced
these unchanged inputs on that version:

| Sample | SHA-256 | First worker refusal |
| --- | --- | --- |
| Tagged | `1318bcf26d788bf653f88863731527c77e53b5a2da1397e87b3e86a366647c95` | Unsupported or inconsistent tagged text structure |
| Untagged | `9b0d3000b912919175d2b777347956c78435caea3ae79c7f6077b323f84dfae1` | Rotated, reflected or skewed page content |

Independent inventory finds Type0/Identity-H with CIDFontType2 and an identity
CIDToGIDMap in both. ToUnicode uses two-byte codes and bfchar/bfrange mappings.
The page has a negative vertical scale and each text matrix reflects it back;
ExtGState carries `/ca 1 /BM /Normal`. Tagged structure adds Document/P/NonStruct
nesting. These are additional unsupported shapes read from the original PDF,
not successful worker traversal past its first refusal. Start with the untagged
case before broadening structure attributes further.

The survey requires synthetic text, reports no document text or metadata, and
checks each source digest before and after inspection. Word supplies its editable
control and a blank fixture supplies `no_runs`; both browser variants supply real
refusals. Seven injected instrument faults are rejected: empty output, an unknown
status, a refusal without a reason, editable with zero runs, negative runs,
boolean runs and a probe that changes its input. These measurements concern the
local Mac exports; they are not a claim about every producer version or platform.

The final browser exporter connects directly to the endpoint recorded in its
isolated profile. An extra HTTP version lookup timed out on repeat runs; the
direct connection succeeds and a fresh export reproduces the same inventory
and worker refusals. A failed-browser control also proves both old output files
are cleared before a rerun can fail.

All 24 local gates passed in 267.6 seconds summed gate time: 1,381 Rust tests
(three ignored), 1,667 frontend tests in 70 suites, locked fuzz-target and example
builds, and normal frontend assets restored with zero harness code. Changes are
confined to development probes, a synthetic HTML source and the plan/evidence
record; the application grammar is unchanged.

### Existing-glyph composite TrueType editing

The worker now reads and writes two-byte Identity-H codes for a single
CIDFontType2 descendant with an explicit identity CIDToGIDMap. It validates the
existing font program, embedding rights, glyph indices, widths and outline
bounds. ToUnicode supports the measured standard wrapper with bfchar and scalar
bfrange data, bounded to 16 KiB and unique printable ASCII. Both W array forms
and DW are checked; explicit width entries are sorted, non-overlapping and
limited to 4,096 CIDs. No font or mapping stream is rewritten. These semantics
follow [ISO 32000-1, sections 9.7 and 9.10](https://developer.adobe.com/document-services/docs/assets/35e4369068f86065372c18787171a17e/PDF_ISO_32000-1.pdf).

Generate a fixture using only the repository's original geometric font:

```bash
uv run --with fonttools --with pypdf testdata/make_textedit_composite.py scratch/textedit-cid/source.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-cid/worker scratch/textedit-cid/source.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-cid/worker/synthetic-before.pdf scratch/textedit-cid/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-cid/worker
```

On 2026-09-13 the generated input SHA-256 was
`7c3144f57da025f08aba1f1635042c240cd87c9729b551098c3824e6b596e956`.
Worker preview, extraction, search, undo, rewrite and refusal checks pass. All
15 native `textedit` checks pass on both Mac and Windows. Independent parser
readback confirms the one changed operand and unchanged resources. PDFKit reads
both platforms' worker and native saves: 898 changed pixels inside the target
line and zero outside. Five independent corruption controls reject glyph zero,
reversed code byte order, an altered glyph map, a changed width and changed font
bytes for their intended reasons.

All 93 focused editor tests pass on both platforms; Windows also passes 10
shared layout tests. All 22 targeted `cid:` and `mapped:` mutations are caught.
The width-limit mutation was rerun after fixing its positive control to compare
against the fixture's literal count, independently of the implementation limit.
The composite fuzz seed is independently recognized as editable; the bounded
fuzz run completed 24,959 inputs in 21 seconds without a finding, at 85 MiB peak
RSS. It uses this platform's existing sanitizer-free fuzz configuration.

A separate diagnostic page reuses the original Edge export's complete font
resource and text-show bytes, with upright positioning and no tags or graphics
state. Its worker round trip and PDFKit readback pass: 2,361 changed pixels in
the target line, zero outside, and byte-identical embedded font data. The strict
resource comparator deliberately remains red on six decimal spellings normalized
by lopdf: CapHeight, two FontBBox values and three W values. Every pair has the
same f32 representation; none is a font-stream change. This diagnostic proves
the font case only. At that milestone the unchanged untagged browser export
was refused at its reflected page transform. Later subsections record the
coordinate, clipping and graphics-state work; tagged structure remains separate.

The Windows source archive SHA-256 is
`2d51737ebb9d9898b361b4f645941742c466bcd8bb3740a117d47fadca1e5447`.
Retrieved output digests and the source manifest match. The normal Windows
checkout stayed clean and its frontend was restored with zero harness code.

All 24 local gates passed in 552.1 seconds summed gate time: 1,389 Rust tests
(three ignored), 1,667 frontend tests in 70 suites, all locked fuzz targets and
examples, and both frontend build profiles. Normal Mac assets contain zero
harness code. The Windows task was removed. Every implementation file matches
the verified Windows snapshot; only this evidence record and the plan changed
afterward.

### Paired reflected coordinates for text editing

The editor accepts nonzero diagonal page/text reflections when both combined
text axes remain positive. Rectangular clips normalize their transformed corners
before intersection. Mirrored final text, rotations, skew, collapsed matrices,
out-of-range coordinates and partly clipped glyphs remain refused. This covers
the coordinate layer used by browser exports; their stroke-colour setters and
external graphics state remain a separate compatibility step.

The synthetic composite-font generator has a `--reflected` mode. It retains the
existing two lines and their page positions through paired reflections and an
explicit clip, using exactly representable scales. No installed font is included.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_composite.py scratch/textedit-reflected/source.pdf --reflected
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-reflected/worker scratch/textedit-reflected/source.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-reflected/worker/synthetic-before.pdf scratch/textedit-reflected/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-reflected/worker
```

The input SHA-256 on 2026-09-13 is
`59b198514e0c4c6878781027f49a714a8c1b95546140fda51aef060b039c4ab6`.
PDFKit renders it identically to the upright composite fixture: zero changed
channels with 10,608 nonwhite source channels. Removing the first text reflection
changes 11,838 channels and the same comparison rejects that control.

All 96 focused editor tests pass on Mac and Windows; Windows additionally passes
10 shared layout tests. All 17 targeted reflection, clipping and page-transform
mutations are caught. The tests cover both reflected axes, nested page transforms,
line moves, crop/rotation, clipping intersections, restoration, unchanged non-target
operators/resources and refusals without document mutation.

Worker and native saves pass independent pypdf readback on both platforms. All
15 native checks pass on both platforms, including saved-output retention.
PDFKit reads all four saved outputs with 898 changed pixels inside the target
line and zero outside. The first Mac invocation passed its UI assertions but
failed to retain the output because its destination directory did not exist;
a rerun with the directory created supplies the independently checked artifact.

The reflected fuzz seed is recognized as editable with one run. The bounded fuzz
run executed 31,161 inputs in 21 seconds without a finding, with 86 MiB peak RSS,
under the existing sanitizer-free macOS configuration.

The Windows archive SHA-256 is
`7773e4c5386ce2dd9f7085136a1359ddf4ad8f27209174dc8bec06dc814c1cd5`.
Retrieved PDF digests and the source manifest match. The temporary scheduled task
was removed; the normal checkout remains clean at its original commit, and its
frontend contains zero harness code.

At the coordinate milestone, the unchanged untagged Edge export passed its
page-transform/clip setup and was refused at unsupported graphics state. A
diagnostic copy removing only `RG` and `gs` was refused as partly clipped: the
first line's conservative full-em envelope extended above the browser's clip.
The following milestones add proven glyph bounds and graphics-state support. This diagnostic
retains the original transforms and font and is not an unchanged browser export.

Independent fontTools inspection of the first browser line finds a maximum glyph
height of 0.72802734375 em. At nominal browser coordinates its ink top is
198.486328125 pt, below the clip top at 200.25 pt; the full-em estimate reaches
201.75 pt. This measures the original line only: a future tighter envelope must
also prove containment for every accepted replacement.

All 24 local gates passed in 572.2 seconds summed gate time: 1,392 Rust
tests (three ignored), 1,667 frontend tests, locked fuzz targets and examples,
and both frontend build profiles. Normal Mac assets contain zero harness code.
Only this evidence record and the plan differ from the verified Windows source
snapshot; implementation and fixtures are unchanged.

### Glyph-based vertical clipping envelopes

Embedded simple and composite fonts now retain the vertical union of every
validated, offered glyph. Clip checks use that union and the original text
advance, while editing hit boxes keep their full-em geometry. This covers
replacement glyphs absent from the source string; a clip that excludes any of
them remains unsupported. Standard-font substitution still cannot supply this
proof. Overhanging glyphs and unsupported font formats remain refused.

`fonts/outlines.rs` measures transformed outline points, including curve controls.
Their convex hull encloses the curves. It preserves fractional component
coordinates that the font parser's public integer rectangle truncates: a test
glyph reaches 800.189208984375 font units while the integer rectangle says 800.
The test's false glyph-header boxes also demonstrate that the measurement comes
from actual outlines. Font-wide bounds include the baseline for blank glyphs.

The generator's `--tight-clip` variant adds a taller fractional B and a descending
D to the original synthetic font. B is absent from the source lines; D appears
in the edited first line. Its clip ends at 190 pt, below the old full-em estimate
of 192 pt. The previous worker refuses this fixture as partly clipped.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_composite.py scratch/textedit-ink/source.pdf --reflected --tight-clip
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-ink/worker scratch/textedit-ink/source.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-ink/worker/synthetic-before.pdf scratch/textedit-ink/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-ink/worker
```

The input SHA-256 on 2026-09-13 is
`46c320ec72459027eddec5cc7d52af81f33ab821758e5a1e59d868211aa938c9`.
All 100 focused editor tests pass on Mac and Windows; Windows also passes 10
shared layout tests. All 26 targeted glyph-envelope, clipping and reflection
mutations are caught. The new cases include unused taller/descending glyphs,
fractional component bounds, curve controls, page scaling, preserved hit boxes
and refusals without document mutation.

All 15 native checks pass on both platforms. Independent pypdf readback confirms
one changed text operand and unchanged font/clip data. Corruption controls reject
a shifted clip, removed clipping operators and an altered font width. PDFKit reads
both platforms' worker and native saves with 1,081 changed pixels inside the target
line and zero outside. The glyph-clip fuzz seed is editable; the bounded fuzz run
executed 30,544 inputs in 21 seconds without a finding, at 86 MiB peak RSS, using
the existing sanitizer-free macOS configuration.

At the glyph-envelope milestone, the unchanged untagged browser export was
refused at graphics state. Its diagnostic copy removing only `RG` and `gs`
yielded two editable runs, retaining
the original font, transforms and clips. This is discovery evidence, not an
unchanged-browser round trip; the outstanding decimal resource normalization
checks still apply before claiming that broader compatibility.

The Windows archive SHA-256 is
`b8665c349544f95dcdef8c478577235a509569fa3784c04c48372d72ea83e3ec`.
Retrieved PDF digests and the source manifest match. The temporary task was removed,
the normal checkout remains clean, and its frontend contains zero harness code.

Discovery regression checks on the unchanged Word export (four runs), the
naturally wrapped LibreOffice export (two runs on page zero) and its tagged
single-page export (two runs) remain editable under the new outline measurement.
These are discovery checks; their earlier round-trip evidence is recorded above.

All 24 local gates passed in 327.6 seconds summed gate time: 1,396 Rust
tests (three ignored), 1,667 frontend tests, locked fuzz targets and examples,
and both frontend build profiles. Normal Mac assets contain zero harness code.
Only this evidence record differs from the verified Windows source snapshot.
### Unchanged untagged browser export and precise content patches

The unchanged single-page Edge export now passes contained preview and saving.
The fixture is generated by the browser procedure above and remains ignored;
its SHA-256 is
`9b0d3000b912919175d2b777347956c78435caea3ae79c7f6077b323f84dfae1`.
The supported ExtGState subset permits only optional `/Type /ExtGState`,
`/BM /Normal`, and alpha `/ca` or `/CA` equal to one. Unknown keys, masks,
transparency, alternate blend modes and font overrides are refused. `G`, `RG`
and `K` validate their operands but leave fill colour unchanged; stroke painting
remains unsupported. At most 32 distinct external state names are admitted.

The first independent raster check exposed a writer defect: re-encoding every
operation rounded `.23999999` to `.24`, changing 116 antialiased pixels in the
untouched second line. `textedit/streams.rs` now replaces only selected text-show
operations and preserves the surrounding content bytes, including number
spellings, comments and whitespace. Its bounded scanner handles strings, escapes,
hex strings, arrays and dictionaries; lopdf must agree with each operation
boundary. Discovery also checks these limits, so a discovered run can be deleted.
The existing operator-preservation test now compares the original operands,
including real zero, instead of comparing against a normalized serialization.

```sh
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-browser-state/worker scratch/textedit-producers/browser-api/browser-untagged.pdf
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-state/worker/synthetic-before.pdf scratch/textedit-browser-state/worker/synthetic-after.pdf --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-browser-state/worker --browser
```

The browser checker opts into exact float32 representation comparisons for
resource numbers only. All content operands and decoded resource stream bytes
remain exact. The default checker still rejects the browser's resource decimal
normalization; it was not weakened for other fixtures. Nine corruption controls
reject changed alpha, blend mode, mask, adjacent float32 font/transform values,
font bytes, shifted/removed clipping and movement of the untouched second line.
PDFKit reads the worker output with 2,421 changed pixels inside the first line
and zero outside. Its fixed target rectangle is independent of editor hit boxes.

All 108 focused editor tests pass on macOS and Windows. Seven graphics-state mutations and
seven stream-patching mutations are caught by their named tests. The new
`editable-browser-state` fuzz seed reaches discovery and deletion with a
composite font, reflected transform, tight clip, graphics state and a decimal
coordinate requiring preservation.

All 15 native editing checks pass on both platforms, including unsaved selection,
search, tab isolation, overflow refusal, undo/redo and save/reopen. Independent
pypdf readback passes for both worker and UI saves. PDFKit reads all four outputs
with 2,421 changed pixels inside the first line and zero outside. Discovery also
remains editable for the unchanged Word export (four runs), naturally wrapped
LibreOffice export (two runs) and tagged single-page export (two runs).

The bounded fuzz run executed 29,094 inputs in 21 seconds without a finding,
at 86 MiB peak RSS, using the existing sanitizer-free macOS configuration.
The Windows archive SHA-256 is
`5d1c3552ec49fb3e3182e723003c90790c0957940d779fd5343a27d75a03a083`.
Retrieved PDF digests and the source manifest match. The temporary task was
removed; the normal Windows checkout remains clean and the restored normal
frontend contains zero harness code.

The final macOS sweep passes all 24 gates (455.9 seconds summed gate time),
including 1,404 Rust tests with three existing ignores and 1,667 frontend tests.
Only this build record and the plan changed after the verified Windows snapshot.

### Tagged browser exports with a bounded NonStruct level

The original tagged Edge export now passes worker preview and saving without
normalizing the source. Its SHA-256 is
`1318bcf26d788bf653f88863731527c77e53b5a2da1397e87b3e86a366647c95`.
The structure grammar is Document/P with one optional NonStruct level, bounded
by the existing 128 content-item limit. It accepts scalar children and indirect
arrays, preserves bounded ASCII language identifiers and checks an optional
ParentTreeNextKey against the actual parent-tree keys. The browser's optional
`/Type /ParentTree` is admitted with that exact name.

ISO 32000-1 tables 322-324 and 333 define the relevant structure entries,
content ownership and NonStruct semantics. Containers may omit Pg, but Pg is
not inherited by a leaf from a structure ancestor: a local integer MCID needs
its own element's Pg, while an explicit MCR identifies its own page. Tests
reject an absent leaf Pg even when its paragraph or Document supplies one.
ActualText, Alt, expansion text, titles, classes, additional child levels and
NonStruct layout attributes remain refused. The authored tree is retained,
including its reading order and parent links.

```sh
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-browser-tagged/worker scratch/textedit-producers/browser-api/browser-tagged.pdf
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-tagged/worker/synthetic-before.pdf scratch/textedit-browser-tagged/worker/synthetic-after.pdf --tagged --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-browser-tagged/worker --browser
```

The independent checker compares the complete structure graph, page parent
keys, content operands and font stream bytes. Resource numbers retain the
explicit float32 comparison established for the untagged browser fixture.
Its eight new negative controls corrupt the forward/reverse parent links,
MCID, role, language, ActualText, next parent key or whole tree; all are rejected,
alongside the nine existing graphics/font/content controls. PDFKit reads the
worker output with 2,421 changed pixels inside the target line and zero outside.

All 114 focused editor tests pass on macOS and Windows, including multi-page ownership,
mixed direct/nested paragraph items, scalar/array variants, the 128-item bound,
missing ownership, cycles, duplicates, metadata and atomic refusal. The new
`editable-nested` fuzz seed reaches discovery and deletion through a symbolic
font, scalar NonStruct leaf and indirect parent array.

All 33 targeted tag mutations are caught. The unsupported-role control changes
both the structure role and the corresponding marked-content name together;
changing only one had allowed the name-mismatch check to hide a removed role
guard. The strengthened case also passes in the Windows follow-up run.

All 15 native checks pass on macOS and Windows. Independent parser and PDFKit
readback verify both platforms' worker and UI saves: the complete structure is
preserved and each output changes 2,421 pixels inside the target, zero outside.
Discovery still finds the expected runs in the unchanged Word, naturally wrapped
LibreOffice, tagged LibreOffice and untagged Edge fixtures.

The bounded fuzz run executed 30,519 inputs in 21 seconds without a finding,
at 87 MiB peak RSS, using the existing sanitizer-free macOS configuration.
The Windows archive SHA-256 is
`baf066e978612c6483712e2edb1da3e79b2562e4cdc034e062a8fa7c3b6d097a`.
Retrieved PDFs and manifests match. A follow-up updated only the isolated-role
test; its final manifest matches all current source and tests. Both temporary
tasks were removed, and the normal Windows checkout remains clean. The restored
normal frontend contains zero harness code.

The final full run passed all 24 gates in 622.2 seconds of summed gate time:
1,410 Rust tests passed with three existing ignored tests, and all 1,667
frontend tests passed. The normal bundle again contains zero harness code.

For remote follow-up checks, transfer source files separately rather than
embedding their base64 data in an EncodedCommand. Run Cargo through the same
interactive scheduled-task environment as the main checks; the direct SSH/WSL
invocation could not access this build directory.

### Naturally wrapped browser text across pages

Verified on macOS, 2026-09-14, with Edge `153.0.4234.32`. The new
`testdata/textedit-producer-browser-flow.html` contains one paragraph and ordinary
spaces, with no line breaks, explicit page breaks or PDF post-processing. Fixed
page size, paragraph width and line height make it wrap naturally into two lines
on each of two pages. The exporter verifies both pages' text and, for tagged
output, one Document/P/NonStruct chain with an integer MCID on page one and an
explicit MCR on page two. Both directions of page ownership must agree.

No application grammar change was needed. Unchanged tagged and untagged exports
pass worker preview and saving on either page. All 19 native multi-page checks
pass on the tagged export, including undo/redo, tab isolation, overflow refusal
and save/reopen. Independent parser and PDFKit readback pass on all four worker
outputs and the native save: 2,421 changed pixels inside the edited line, zero
outside it, and zero changes on the untouched page. The embedded font stream,
content operands and complete tagged structure remain preserved; resource numbers
use the existing explicit float32 comparison.

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge' scratch/textedit-browser-flow --flow
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-browser-flow/worker-page2 scratch/textedit-browser-flow/browser-tagged.pdf --page=1
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-flow/worker-page2/synthetic-before.pdf scratch/textedit-browser-flow/worker-page2/synthetic-after.pdf --flow --tagged --page=1 --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-browser-flow/worker-page2 --browser-flow --page=1
uv run scripts/tabs_check.py 'src-tauri/target/release/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-browser-flow/browser-tagged.pdf --phase textedit-multipage --saved-copy scratch/textedit-browser-flow/ui-page2.pdf
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-flow/browser-tagged.pdf scratch/textedit-browser-flow/ui-page2.pdf --flow --tagged --page=1
```

Build the checks app using the explicit profile at the top of this file when
needed. Use a separate output directory and omit `--page=1` to edit page one.
For the untagged control, choose `browser-untagged.pdf` and omit `--tagged` and
`--controls`. Five new negative controls change the continuation page, remove its
content item, change a page parent key, break reverse ownership or change the
untouched page's content bytes; each fails for its named reason on both edited
page choices. All 17 existing single-page browser corruption controls still fail,
and the original exporter and PDFKit browser mode remain green.

Windows x64 follow-up at `c2e975b` on 2026-09-14 also passes the worker round trip,
all 19 native checks and five cross-page corruption controls for editing page two
of the tagged fixture. Both saved PDFs pass independent parser and PDFKit readback;
page one is unchanged. The combined Windows record is below the overhang checks.

### Existing accented glyphs in composite fonts

Measured on macOS, 2026-09-14, with Edge `153.0.4234.32`. Before the change,
the unchanged accented export was refused at its two-byte character map.
The reader now permits unique printable Latin-1 mappings, bounded to 191 values;
the 16 KiB CMap limit, glyph/width checks and outline bounds are unchanged.
Single-byte mapped and ordinary simple embedded fonts retain their ASCII scope.

At this mapping-only step, the Arial fixture still failed because Ä extends
1.465 font units past each side of its advance. The overhang increment below
separately proves placement. Independently measured Verdana outlines
fit their advances; the unchanged browser export using that font passes the worker
round trip and all 15 native editing checks. Independent parser and PDFKit readback
of both worker and UI saves preserve the font, structure and surrounding content:
2,984 changed pixels inside the edited line, zero outside. All 17 corruption
controls fail. The language control now selects a different language dynamically;
writing `de` into this already German fixture had changed nothing.

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge' scratch/textedit-browser-latin1/verdana --latin1 --latin1-font Verdana
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-browser-latin1/verdana/worker scratch/textedit-browser-latin1/verdana/browser-tagged.pdf --cid-latin1
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-latin1/verdana/worker/synthetic-before.pdf scratch/textedit-browser-latin1/verdana/worker/synthetic-after.pdf --tagged --latin1 --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-browser-latin1/verdana/worker --browser-latin1
uv run scripts/tabs_check.py 'src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-browser-latin1/verdana/browser-tagged.pdf --phase textedit-cid-latin1 --saved-copy scratch/textedit-browser-latin1/verdana/ui-after.pdf
```

Build the debug checks app using the explicit profile at the top of this file.
Generate the Arial counterpart in a separate directory with `--latin1` alone.
All 104 editor-module tests pass, and all three targeted mapping mutations are
caught. The new `editable-composite-latin1` fuzz seed reaches discovery with one
run; the bounded fuzz campaign executed 30,272 inputs in 21 seconds without a
finding, at 87 MiB peak RSS, using the existing sanitizer-free macOS setup.
Type checking, Clippy and all 1,667 frontend tests passed on macOS. Windows x64
follow-up at `c2e975b` on 2026-09-14 passes the tagged Verdana worker round trip,
all 15 native checks and independent readback, detailed below.


### Bounded horizontal overhangs in composite fonts

The unchanged Edge/Arial accented export now has a separate placement check.
Ä extends 1.465 font units beyond both sides of its advance; the original line
contains it internally and fits its clip. `--overhang` replaces that line with
`ÖÄÜ äöü ß`, while also requiring a leading-Ä replacement to fail without output.
Neither the source clip nor the embedded font is modified. Composite outlines
may extend at most a quarter em per side; source hit boxes and clipping use the
measured excursions, and replacement ink must stay inside the original unrounded
horizontal envelope as well as its advance. Simple-font overhangs remain refused.

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge' scratch/textedit-browser-latin1 --latin1
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-browser-latin1/arial-overhang/worker scratch/textedit-browser-latin1/browser-tagged.pdf --overhang
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-browser-latin1/arial-overhang/worker/synthetic-before.pdf scratch/textedit-browser-latin1/arial-overhang/worker/synthetic-after.pdf --tagged --overhang --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-browser-latin1/arial-overhang/worker --browser-overhang
uv run scripts/tabs_check.py 'src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-browser-latin1/browser-tagged.pdf --phase textedit-overhang --saved-copy scratch/textedit-browser-latin1/arial-overhang/ui-after.pdf
```

Measured on macOS, 2026-09-14: tagged and untagged worker saves pass independent
parser and PDFKit readback, with 2,900 changed pixels inside the target line and
zero outside. All 17 parser corruption controls fail. All 107 editor-module tests
pass, and all seven selected Rust mutations are caught (five new overhang checks
and two existing checks affected by the geometry change). The mutation runner's
full Rust control also passes. Type checking, Clippy, all 1,667 frontend tests and
the production build pass; the normal bundle still contains no check harness.
All 16 native editing checks pass, including rejection of a shorter draft whose
ink crosses the left boundary. The UI-saved PDF passes the same independent
parser/PDFKit checks with 2,900 changed pixels inside the line and zero outside.
The native harness waits for newly created editor targets: the old targets can
remain visible while discovery awaits the worker, so existence alone raced the
replacement editor. The refusal check requires the specific ink-boundary error.
Windows x64 verification at `c2e975b` on 2026-09-14 passes all 107 editor tests
and 50 native checks: Arial overhangs (16), Verdana Latin-1 (15), and naturally
wrapped browser text on page two (19). All three worker previews/saves pass,
including refusal without output, as do 17 Arial and five cross-page parser
corruption controls. Each worker and native save passes independent parsing on
Windows and macOS, then PDFKit rendering:

| Fixture | Changed pixels inside the edited line, worker / UI | Outside the line |
| --- | ---: | ---: |
| Arial overhangs | 2,900 / 2,900 | 0 / 0 |
| Verdana Latin-1 | 2,984 / 2,984 | 0 / 0 |
| Naturally wrapped, page two | 2,421 / 2,421 | 0 / 0 |

The wrapped fixture's first page remains pixel-identical. All 15 transferred PDFs
match their Windows sizes and SHA-256 digests. The external worker-exit observer
passes its live/dead control and all three native runs. The job completes in about
3.5 minutes including a fresh build, restores normal frontend assets with zero
harness code, and leaves both source checkouts clean. The temporary scheduled
task is removed; the isolated build cache is retained for subsequent checks.


### Text editing around painted rectangles

Fresh, unchanged Edge exports from `textedit-producer-rectangles.html` contain
coloured backgrounds before and after text. The tagged export gives the first
background its own MCID under the paragraph, alongside a NonStruct text item.
ReportLab's `painted-rectangles.pdf` adds a filled background, a stroked border,
and a filled/stroked rectangle, with ASCII85/Flate stream encoding. These cases
previously failed before text discovery. Paths are now consumed as complete
rectangle/paint pairs; clipping retains its separate validation. Tags need actual
text or painting, so an empty or discarded path cannot satisfy a marked item.

```sh
uv run --with websocket-client --with pypdf testdata/make_textedit_browser.py '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge' scratch/textedit-rectangles/browser --rectangles
uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-rectangles/reportlab
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-rectangles/browser-worker scratch/textedit-rectangles/browser/browser-tagged.pdf
uv run --with pypdf scripts/text_edit_browser_check.py scratch/textedit-rectangles/browser-worker/synthetic-before.pdf scratch/textedit-rectangles/browser-worker/synthetic-after.pdf --tagged --rectangles --controls
swift scripts/text_edit_pdfkit.swift scratch/textedit-rectangles/browser-worker --browser
uv run scripts/tabs_check.py 'src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-rectangles/browser/browser-tagged.pdf --phase textedit --saved-copy scratch/textedit-rectangles/browser-ui.pdf
python3 scripts/mutate_rust.py --only 'painted rectangles:'
```

Measured on macOS, 2026-09-14: tagged and untagged Edge worker saves and the
ReportLab worker save pass independent parsing and PDFKit rendering. Only the
edited text operand changes; fonts, tags and painted operators are preserved.
PDFKit measures 2,421 changed pixels for each Edge save and 2,394 for ReportLab,
all inside the edited line, with zero outside. All 20 tagged Edge corruption
controls and 12 untagged controls are rejected, including moved, altered and
removed rectangle painting. All six targeted Rust mutations are caught by their
named tests, with a green full-suite control. A clipping mutation initially
survived because `Q` restored the clip before the test drew text; the corrected
test also draws text before restoration and without a saved graphics state.

All 123 tests selected by `cargo test --lib textedit` pass. Both native workflows
(tagged Edge and ReportLab) pass all 15 checks; their saved PDFs pass the same
independent parser/PDFKit checks and pixel totals above. Type checking, Clippy
for the library/tests, formatting and mutation anchors pass. The production build
contains zero harness code. The new rectangle fuzz seed is independently offered
as editable by the contained probe; the 20-second `textedit_scan` campaign completes
25,152 inputs in 21 seconds without a finding, at 87 MiB peak RSS. This macOS run
uses `--sanitizer=none` and is not address-sanitizer evidence.

These are synthetic compatibility examples, not a success rate for arbitrary
PDFs. Curves, compound paths and clipping combined with painting remain refused.
Windows x64 verification at `ecf4e51` on 2026-09-14 passes all 123 selected Rust
tests, all three worker preview/save cases and 30 native checks (tagged Edge and
ReportLab). Independent parsing on Windows rejects all 20 tagged and 12 untagged
corruption controls. All five worker/native saves then pass parsing and PDFKit
rendering on macOS with the same pixel totals above and zero changes outside the
edited line. All 13 transferred PDFs match their Windows sizes and SHA-256 digests.
The external worker-exit observer passes its live/dead control and both native runs.

The cached Windows build completes the job in 67 seconds, including dependency
installation and the checks-app rebuild. Normal frontend assets are restored with
zero harness code; both source checkouts remain clean and the temporary task is
removed. The isolated source/build cache is retained. If a reused checkout warns
that commit-graph files are missing, `git -c core.commitGraph=true commit-graph
write --reachable --split=replace` rebuilds that cache; follow with `git commit-graph
verify` and require no warning output. Setting `core.commitGraph=false` also disables
writing: that command returned zero while doing no repair in this run.


### Text editing around straight-line strokes

The unchanged `columns.pdf` fixture previously refused all text because of its
column divider. It now offers 20 text runs; `text-base14.pdf`, with backgrounds
and a polyline, offers four. The fresh ReportLab `stroked-lines.pdf` export has a
horizontal divider plus open and closed polylines around the two text blocks.
Only complete, bounded straight-line subpaths are accepted; curves, compound
paths, nonrectangular fills and line-based clips remain refused.

```sh
uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-strokes/reportlab
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-strokes/worker scratch/textedit-strokes/reportlab/stroked-lines.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-strokes/worker/synthetic-before.pdf scratch/textedit-strokes/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-strokes/worker
uv run scripts/tabs_check.py 'src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-strokes/reportlab/stroked-lines.pdf --phase textedit --saved-copy scratch/textedit-strokes/ui-after.pdf
python3 scripts/mutate_rust.py --only 'stroked lines:'
```

Measured on macOS, 2026-09-14: all 126 text-editing tests pass. The ReportLab worker
round trip passes preview, extraction, search, undo and refusal without output.
Independent parsing confirms only the target text operand changed, preserving
font/colour resources and every stroke operator. PDFKit measures 2,394 changed
pixels inside the edited line and zero outside. Corrupting a line point, changing
its paint operator or removing a segment each fails independent readback.
All 15 native editing checks pass, and the UI-saved PDF passes the same independent
parser and PDFKit checks with the same pixel totals. All seven targeted mutations
are caught by their named tests; the full Rust control is green. Type checking,
all-target Clippy, formatting, mutation anchors and the production build pass, with zero shipped
harness code. The new stroke seed reaches editable discovery, and a 20-second
`textedit_scan` campaign completes 23,577 inputs in 21 seconds without a finding,
at 87 MiB peak RSS. This macOS run uses `--sanitizer=none`.
Windows x64 verification at `66c8350` on 2026-09-14 passes all 126 text-editing
tests and 15 native checks. The unchanged column-divider and background/polyline
fixtures expose 20 and four text runs respectively. Both ReportLab worker and UI
saves pass independent parsing on Windows and macOS, then PDFKit rendering with
2,394 changed pixels inside the edited line and zero outside. All five transferred
PDFs match their Windows sizes and SHA-256 digests. The worker-exit observer passes
its live/dead control and confirms no surviving test workers after the native run.
The cached job takes 61 seconds, restores normal frontend assets with zero harness
code, and leaves both source checkouts clean. The temporary task is removed and
the isolated source/build cache retained.


### Text editing with default Helvetica encoding

Unembedded standard Helvetica without an Encoding entry uses Adobe's default
mapping. Only its 117 characters inside the existing printable Latin-1 domain
are offered; codes for curly quotes and ligatures remain refused. Custom metrics,
encoding dictionaries and an explicit StandardEncoding name remain refused.
The font resource is preserved without adding an Encoding entry.

```sh
mkdir -p scratch/textedit-default
TPDF_DEFAULT_ENCODING_PROBE="$PWD/scratch/textedit-default/mapping.json" cargo test --locked --manifest-path src-tauri/Cargo.toml --lib textedit -- --quiet
uv run --with reportlab --with pypdf testdata/make_textedit_default.py scratch/textedit-default --mapping scratch/textedit-default/mapping.json
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-default/mapped-worker scratch/textedit-default/mapped.pdf --default-encoding
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-default/mapped-worker/synthetic-before.pdf scratch/textedit-default/mapped-worker/synthetic-after.pdf --default-encoding
swift scripts/text_edit_pdfkit.swift scratch/textedit-default/mapped-worker --default-encoding
uv run scripts/tabs_check.py 'src-tauri/target/debug/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf' scratch/textedit-default/ascii.pdf --phase textedit --saved-copy scratch/textedit-default/ui-after.pdf
python3 scripts/mutate_rust.py --only 'default encoding:'
python3 src-tauri/fuzz/run.py --target textedit_scan --seconds 20
```

Measured on macOS, 2026-09-14: all 129 text-editing tests pass; ReportLab's
independent tables agree with all 256 decoding decisions and all 117 supported
character widths. ASCII and mapped-punctuation worker saves pass preview,
extraction, search, undo and refusal checks. Independent parsing confirms only
the target text operand changes; PDFKit reads the intended text and measures
2,394 changed pixels for ASCII and 1,634 for mapped punctuation, all inside the
edited line with zero outside. All four targeted mutations are caught by their
named tests, with a green full Rust control. The unchanged comments, inherited
and rotated fixtures now expose 36, six and 12 text runs respectively; these are
synthetic compatibility examples, not a real-document success rate.
All 15 native checks pass on the ASCII fixture. Its UI-saved output passes the
independent parser and PDFKit checks with 2,394 changed pixels inside the edited
line and zero outside. The mapped-punctuation fixture is covered through the
worker; the native phase types ASCII. Clippy across all targets, type checking,
formatting and mutation anchors pass. The normal production build contains zero
harness code. The independent mapping oracle also rejects a deliberately wrong
character width. The new fuzz seed reaches editable discovery; a 20-second
`textedit_scan` campaign completes 27,745 inputs in 21 seconds with no finding,
at 88 MiB peak RSS. This macOS run uses `--sanitizer=none`.
Windows x64 verification at `4e48576` on 2026-09-14 passes all 129 text-editing
tests, all 256 independent mapping decisions and 117 widths, both worker round
trips, and all 15 native checks. The three unchanged fixtures expose the same
36, six and 12 runs. All eight transferred PDFs match their Windows sizes and
SHA-256 digests. Both worker saves and the UI save pass independent parsing on
Windows and macOS, then PDFKit rendering: ASCII changes 2,394 pixels, mapped
punctuation 1,634, with zero changes outside the edited line in every case.
The worker-exit observer passes its live/dead control and reports no surviving
test workers after enumerating 506 processes. The cached job takes 49 seconds,
restores normal frontend assets with zero harness code, and leaves both source
checkouts clean. The temporary task is removed; the isolated build cache is retained.


### Public-document text editing baseline

The broader check on 2026-09-14 changes the development priority. A local survey
of 42 selected synthetic and producer-export PDFs found 72 editable pages out of
88. All selected Word, LibreOffice, browser and Quartz exports passed discovery.
The large `text-heavy.pdf` stress fixture exceeded the 128-page inspection bound
and was explicitly excluded. Those controlled inputs do not establish practical
compatibility: five unchanged public documents had **zero editable pages out of
45**, with no worker crash or incomplete inspection. Independent pypdf page counts
agree with all five worker reports; every source digest is unchanged.

| Public source | Pages | First refusal reported |
|---|---:|---|
| HM Passport Office application guidance | 16 | External text graphics state |
| European Commission consumer conditions factsheet, Lithuania | 6 | Tagged structure |
| Logitech M185 quick-start guide | 2 | Tagged structure |
| IRS Form W-9 and instructions | 6 | Tagged structure |
| Attention Is All You Need, arXiv 1706.03762 | 15 | Unsupported fonts (12), text state/positioning (3) |

The selected sources, URLs, byte counts and SHA-256 digests are recorded in
`testdata/textedit-public-corpus.json`. PDFs are downloaded into scratch only;
they are not rewritten to make the editor accept them. This is a small selected
sample, not a population success rate. Refused means text editing is unavailable,
not that viewing, annotations or form filling fail. The survey checks discovery;
it does not claim that arbitrary replacements fit or that saving was verified.

Reproduce the public sample (network access needed only for missing downloads):

```sh
python3 - <<'PYCODE'
import hashlib, json, subprocess, urllib.request
from pathlib import Path
manifest = json.loads(Path('testdata/textedit-public-corpus.json').read_text())
root = Path('scratch/textedit-public'); root.mkdir(parents=True, exist_ok=True)
paths = []
for entry in manifest['files']:
    path = root / entry['filename']
    if not path.exists():
        with urllib.request.urlopen(entry['url'], timeout=30) as response:
            data = response.read(16 * 1024 * 1024 + 1)
        assert len(data) <= 16 * 1024 * 1024 and data.startswith(b'%PDF-')
        path.write_bytes(data)
    assert path.stat().st_size == entry['bytes']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == entry['sha256']
    paths.append(str(path))
subprocess.run(['python3', 'scripts/textedit_survey.py',
    'src-tauri/target/debug/examples/text-edit-probe', *paths,
    '--output', str(root / 'report.json')], check=True)
PYCODE
```

A newer publisher revision fails the digest comparison; record it as a new sample
instead of treating different bytes as a regression. Two attempted NHS leaflet
URLs returned HTTP 404 and 403 and were not counted as inspected documents.

The first refusal is not the full blocker list. Independent resource inspection
finds CFF font programs in the passport, mouse and W-9 documents, Type1 programs
in the research paper, and unembedded Arial plus tables/figures in the factsheet.
Relaxing graphics-state or tag checks alone would not demonstrate those documents
are editable. The next increment should take one unchanged public page through
discovery, replacement and independent saved-PDF readback, documenting every
blocking construct first and reproducing it in small synthetic tests. The initial
W-9 acceptance target was wrong: all four embedded CFF fonts on instruction page
index 1 declare `/FSType 4 def`. Under the editor's existing embedding policy,
these print/preview-only fonts are refused. Keep this document as a refusal
control; do not bypass its font restrictions to meet a compatibility target.

Validation of the survey tool on macOS: its mixed-page/boundary controls pass,
Clippy for `text-edit-probe` and formatting pass, and the public input page counts
agree with the independent parser. Windows execution at `c43df81` also passes the
mixed-page and boundary controls; all 45 public page verdicts and input digests
agree with macOS. No production editing rules were relaxed by the survey increment.

### Text editing with embedded CFF fonts

Simple Type1 fonts carrying a Type1C program now support existing printable ASCII
glyphs under explicit WinAnsi encoding. Glyph names select the outlines, independently
of the program's internal encoding. PDF widths must agree with program advances,
and complete outlines, including spaces, must validate within bounded ink limits.
The existing replacement clip and line-width checks apply. Fonts and adjacent
text are preserved; no glyphs are added or substituted.

The worker bounds decoded font data, glyph counts and CFF metadata. Only the
standard font matrix and filled Type 2 outlines are supported. The later
*Custom CFF glyph encodings* increment adds bounded ASCII Differences and matching
ToUnicode maps. CID CFF, other custom encodings, conflicting Unicode mappings,
arbitrary embedded PostScript, nonstandard paint semantics and restricted
embedding permissions remain refused. Literal FSType
declarations follow the same permission mask as the TrueType path. The CFF
metadata check covers fields that the outline library intentionally skips.

The fixtures contain original geometric outlines, generated without installed
fonts. Regenerate their programs and the disposable PDF with:

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_cff.py scratch/textedit-cff --rust-fixtures
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
src-tauri/target/debug/examples/text-edit-probe scratch/textedit-cff/worker scratch/textedit-cff/synthetic.pdf
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-cff/worker/synthetic-before.pdf scratch/textedit-cff/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-cff/worker
```

On macOS, 136 focused Rust tests and all 15 native text-editing checks pass.
Independent pypdf readback confirms only the target operand changed; PDFKit
confirms text agreement and 988 changed pixels inside the target, zero outside.
The native saved copy also passes independent readback. The unchanged public
sample still has 0 editable pages out of 45; CFF support alone does not close its
other layout and font blockers.

All eight targeted `CFF:` mutations are caught by their named tests, after correcting
one mutation that initially failed to compile. Their clean control passes 1,435
Rust tests. The instrumented `textedit_scan` campaign executes 23,076 inputs in
21 seconds without a finding, including original CFF and refusal seeds; macOS
uses `--sanitizer=none`, so this is not an AddressSanitizer result. Clippy for all
targets, formatting and notices pass. All 11 font programs regenerate byte-for-byte,
and the final normal frontend bundle contains zero harness units.

Windows x64 verification at `c43df81` on 2026-09-14 passes all 136 text-editor
Rust tests and all 15 native checks. The contained worker and native saved copy
both pass independent pypdf readback on Windows and macOS. PDFKit renders each
Windows output with 988 changed pixels inside the target and zero outside.
All five retrieved PDF sizes and SHA-256 digests match the Windows manifest.
The run also passes the survey's all-pages controls and reproduces every verdict
in the original 45-page public sample. Normal frontend assets are restored with
zero harness units, the temporary task is removed, and the ordinary checkout
remains clean. The isolated exact-commit source and build cache are retained.

### Public target follow-up

Two additional unchanged public documents were inspected on macOS, separately
from the original five-document baseline. Their URLs and digests are in
`testdata/textedit-public-corpus.json` under `followup_files`; use that list in
the download recipe above to reproduce this separate sample. All three pages
are refused, and their digests remain unchanged.

- Wellington Parish Council's two-page April 2026 draft agenda is a Quartz
  export. Both pages first refuse the single-byte character map. Its seven
  embedded TrueType resources omit OS/2, a legacy case the existing font policy
  can support, but also require additional mapping proof. The document also has
  character spacing, a rendering intent, curves and an image invocation. It is
  not a one-guard compatibility fix.
- Adobe's one-page resignation letter template first refuses tagged structure.
  Its regular fonts declare `/FSType 8 def`, but its bold heading font declares
  `/FSType 4 def`. It also has CID CFF, custom encoding with a ligature, ToUnicode
  overrides and additional text/graphics state. Its appearance as an editable
  template does not make it a suitable whole-page acceptance control for the
  current font policy.

The next practical-document acceptance target remains open. Choose a document by its complete
resource and content requirements before adding more grammar support; a first
refusal alone repeatedly understated the work and the font-policy constraints.

### Simple TrueType overhangs and an unchanged W3C fixture

W3C's public `dummy.pdf` provides a small external acceptance control. Its
OpenOffice 2.1 output contains a simple symbolic Arial Bold subset; the existing
legacy-font policy accepts its omitted OS/2 table. The only admission blocker
was the `f` outline extending about 29.3 units past its PDF advance, measured in
thousandths of an em. Simple TrueType now carries measured horizontal overhangs
through the same source-clip and replacement-ink checks used by the composite
and CFF paths, with the same quarter-em bounds. The metrics are indexed by
decoded characters, not symbolic PDF byte values.

The unchanged download now exposes its six text fragments. The worker and native
application both replace the final `le` with `ll`, changing `Dummy PDF file` to
`Dummy PDF fill`. No source repair, font substitution or normalization is used
to gain admission. The five preceding fragments and every font byte are preserved.
URLs and digests for this file and the still-refused five-page W3C headers/footers
example are recorded under `external_test_files` in the public-corpus manifest.
These are external test fixtures, separate from the seven practical documents
whose 48 pages remain the outstanding compatibility sample.

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
src-tauri/target/debug/examples/text-edit-probe --w3c-dummy scratch/textedit-target/w3c-dummy.pdf scratch/textedit-target/worker
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-target/worker/synthetic-before.pdf scratch/textedit-target/worker/synthetic-after.pdf --w3c-dummy
swift scripts/text_edit_pdfkit.swift scratch/textedit-target/worker --w3c-dummy
```

Download the manifest's `external_test_files` with the earlier digest-checked
recipe. The probe requires a new output directory and refuses to overwrite a
previous result. Its `synthetic-before/after.pdf` output names follow the existing
readback tools; in this mode the before file is a byte-for-byte downloaded copy.
Use `tabs_check.py --phase textedit-w3c --saved-copy <path>` with the checks
application and the original download for the native workflow.

Verified on macOS: 138 focused Rust tests, all 15 native checks on the W3C file,
and the existing 15-check synthetic workflow control pass. Independent pypdf
readback accepts both saved outputs and refuses an unedited output or altered
font resources. PDFKit confirms the expected text and 243 changed pixels within
the final fragment, zero outside, for both saves. Four targeted `simple overhang:`
mutations are caught by their named tests; the clean Rust control passes 1,437
tests. The instrumented fuzz run executes 26,509 inputs in 21 seconds without a
finding, at 87 MiB peak RSS (`--sanitizer=none` on macOS).

Windows x64 verification at `ac82af5` on 2026-09-14 passes all 138 text-editor
Rust tests, the contained worker probes, and both 15-check native workflows
(unchanged W3C input and synthetic CFF control). All four saved outputs pass
independent pypdf readback on Windows and macOS. PDFKit renders each Windows
W3C output with 243 changed pixels inside the final fragment and zero outside;
each CFF control has 988 inside and zero outside. All 11 retrieved PDF sizes and
SHA-256 digests match the Windows manifest. The external all-pages survey agrees
with macOS for both documents and all six pages: one editable and five refused.
Normal frontend assets are restored with zero harness units, the temporary task
is removed, and the ordinary checkout remains clean. The isolated exact-commit
source and build cache are retained.


### Single-byte character-map ranges

Symbolic TrueType editing now accepts scalar `bfrange` entries as well as
`bfchar`, using the existing strict wrapper and 16 KiB decoded limit. Entire
expansions must stay in printable ASCII, with unique source and Unicode values
across all blocks. Reversed endpoints, mismatched counts, array targets and
multibyte codes remain refused. No mapping or font program is rewritten.

This addresses a measured blocker in the unchanged Wellington agenda: its
Quartz fonts use single-byte ranges. Both pages now pass their first font-map
check and stop at unsupported page operators. This does not establish page
editability: the document also needs nonzero character spacing, an en dash,
curves and an image invocation. The practical-document target remains open.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-ranges/source.pdf --ranges
src-tauri/target/debug/examples/text-edit-probe scratch/textedit-ranges/worker scratch/textedit-ranges/source.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-ranges/worker/synthetic-before.pdf scratch/textedit-ranges/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-ranges/worker
```

The generator uses original geometric outlines and gives the edited letters
codes inside multi-character ranges. Use this generated PDF for native and
independent readback; the hand-built Rust font fixture is for unit assertions
and did not pass PDFKit readback. On macOS, all 141 focused tests and the 15 native
workflow checks pass. Worker and native saves preserve all resources under
pypdf readback; PDFKit measures 935 changed pixels inside the line and zero
outside for each. Unchanged-output and altered-mapping controls are rejected.
All seven selected mapping mutations are caught, with 1,440 Rust tests passing
in the clean control. Clippy and mutation-anchor checks pass.

The instrumented `textedit_scan` run, including the new range-mapping seed,
executes 27,962 inputs in 21 seconds without a finding, peaking at 88 MiB RSS.
It uses `--sanitizer=none` on macOS. The existing `bfchar` generator output remains
byte-identical. Normal frontend assets are restored with zero harness units.
Windows x64 verification at `04c8fd7` on 2026-09-14 passes all 141 text-editor
Rust tests and both 15-check native workflows (range-mapped geometric fixture
and unchanged W3C control). All four worker/native outputs pass independent
pypdf readback on Windows and macOS. PDFKit finds 935 changed pixels inside the
range fixture's target and 243 inside the W3C target, zero outside, for both
writing routes. All 11 retrieved PDF sizes and SHA-256 digests match the Windows
manifest. Discovery agrees with macOS for the W3C document and both agenda pages.
Normal assets are restored with zero harness units, the temporary task is removed,
and the ordinary checkout remains clean; the isolated source/build cache is retained.

The next acceptance candidate is page 1 of the unchanged Wellington agenda,
whose URL and digest are in `testdata/textedit-public-corpus.json`. Its complete
operator inventory has no curves, so page 2 need not be admitted to prove an
edit on page 1. Page 1 has 107 `Tc` setters, ranging from -0.0076 to 0.0017 text
space units, a `/Perceptual` rendering intent, and one 841x141, eight-bit,
ICCBased Flate image without a mask. Its `/TT8` font maps an en dash (U+2013).
These are admission requirements, not evidence that all other font/ink checks
will pass. Page 2 adds 147 cubic-curve operators and remains a separate target.

Character spacing needs one shared measurement path for discovery and writing:
apply it once per decoded character (not per byte of a CID), include it in `TJ`
fragment positions and advances, preserve it across `BT`/`ET` and `q`/`Q`, and
validate replacement ink with the same state. Supporting the setter alone would
mis-size edits. Keep the original image and font bytes, mapping, clipping and
unrelated content unchanged throughout the eventual page-1 round trip.


### Character spacing in text edits

Nonzero `Tc` is retained during discovery, preview and writing. A shared layout
calculation measures its per-character advance and glyph envelope; it applies
once per decoded character, including two-byte CID fonts, and carries through
`TJ` fragments and `q`/`Q` restoration. Each show limits spacing to a quarter of
the font size, requires positive character steps and bounds the accumulated
advance. The final spacing step affects advance without enlarging glyph ink.
Other text-state exclusions remain in place.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-spacing/positive.pdf --ranges --spacing 1
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-spacing/negative.pdf --ranges --spacing -1
```

Run each source through `text-edit-probe` and the native `textedit` phase using
the existing commands above. Both existing no-spacing generator modes remain
byte-identical. Independent pypdf and `text_edit_pdfkit.swift` readback preserve
resources and find 935 changed pixels inside the edited line, zero outside, for
both worker and native saves in both spacing directions.

`scripts/text_spacing_pdfkit.swift` takes the positive source, negative source,
positive saved PDF and negative saved PDF as four arguments. It independently
measures the rendered position of every painted glyph: the +/-1 Tc pair must
differ by 2pt per character, including spaces in the character count. The
identical-source control must fail. PDFKit's selection rectangles are unsuitable
for this measurement; the check reads rendered columns instead.

On macOS, 147 focused Rust tests and both 15-check native workflows pass. All
13 selected mutations are caught; the clean controls pass 1,446 Rust tests.
Clippy and mutation-anchor checks pass. The unchanged agenda is still refused:
page 1 reaches an unsupported operator and page 2 the single-byte map refusal.
This completes the spacing prerequisite, not the practical-page milestone.

The seeded `textedit_scan` fuzz run completes 27,176 executions in 21 seconds,
with 88 MiB peak RSS and no finding. This macOS run uses `--sanitizer=none`;
it is not AddressSanitizer coverage.
Normal frontend assets are restored; the bundle contains zero harness units,
and the third-party notices check passes.

Windows x64 verification at `7c15d42` on 2026-09-14 passes all 147 text-editor
tests and both 15-check native workflows. Both worker and native saves pass
independent pypdf readback on Windows and macOS. PDFKit finds 935 changed pixels
inside the target, zero outside, for all four outputs; the glyph-position check
also passes for both writing routes. All ten retrieved PDFs match their Windows
sizes and SHA-256 digests. The first task was terminated with status `0xC000013A`
after the tests; the retry completes with exit zero. Normal assets are restored,
the temporary task is removed, and the ordinary checkout remains clean.

### Rendering intents in text edits

The four standard intents are accepted through `ri` and graphics-state `/RI`,
with original operators and resource bytes retained. Unknown names, malformed
values and implicit positioning between shows remain refused. Generate the
combined spacing/intent fixture with:

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-intents/source.pdf --ranges --spacing -1 --intent Perceptual
```

On macOS, 149 focused Rust tests and the 15-check native workflow pass. Worker
and native saves pass independent pypdf and PDFKit readback: only the target
text operand changes, resources agree, and 935 pixels change inside the target
with zero outside. All three targeted mutations are caught by their named tests;
the clean control passes 1,448 Rust tests. Clippy, mutation anchors, normal bundle
and notices checks pass. Windows verification of this increment is pending.
The unchanged agenda still refuses both pages; image and en-dash support remain
before its practical-page milestone can be demonstrated.

Windows x64 verification at `b779185` on 2026-09-14 passes all 149 text-editor
tests and the 15-check native workflow. Worker and native outputs pass independent
pypdf readback on both platforms and PDFKit rendering: 935 changed pixels inside
the target, zero outside. All five retrieved PDFs match their Windows sizes and
SHA-256 digests. Normal assets are restored, the temporary task is removed, and
the ordinary checkout remains clean.

### Images alongside editable text

Opaque eight-bit image XObjects now remain intact during text edits. Dimensions,
colour spaces, filters and samples are validated under the per-page limits in
`docs/PLAN.md`; masks, forms, external data and custom decode mappings are refused.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-images/source.pdf --ranges --spacing -1 --intent Perceptual --image
```

macOS verification passes 153 focused Rust tests and the 15-check native workflow.
Worker readback covers the generated RGB image and the unchanged `/Im1` image
and ICC resource from the Wellington agenda, placed in the synthetic fixture.
Native readback uses the latter. All three outputs preserve image/resource bytes
and change 935 pixels inside the text target, zero outside. Pass `--image` to
`text_edit_pdfkit.swift`: it also requires painted pixels in the fixed image area
(18,816 for RGB, 1,826 for the agenda image); the no-image control must fail.
All 14 selected mutations are caught, including seven new image-admission cases;
the clean control passes 1,452 Rust tests. The worker probe confirms the new image
fuzz seed reaches editable text. Both unchanged agenda pages now reach a character-map refusal;
the practical-page milestone remains open, and Windows image verification is pending.
The seeded fuzz run completes 27,742 executions in 21 seconds with 88 MiB peak
RSS and no finding (`--sanitizer=none` on macOS). Clippy, mutation anchors,
formatting, bundle and notices checks pass; normal frontend assets are restored.


Windows x64 image verification at `11fe094` passes all 153 focused Rust tests,
worker RGB/ICC cases and both 15-check native workflows. All ten retrieved PDFs
match the Windows artifact sizes and SHA-256 digests. Independent pypdf and
PDFKit checks of the four edited outputs preserve every resource and find 935
changed pixels inside the text target, zero outside. Both image variants remain
visibly painted. The temporary task is removed and the ordinary checkout is clean.

### Mapped en dash and an unchanged practical page

U+2013 is supported when an admitted single-byte symbolic TrueType or two-byte
Identity-H font already maps it to a valid glyph. The font's original codes,
widths, outlines and resources remain authoritative. No subset is extended.
Unmapped fonts retain their prior character repertoire; U+0096 and neighboring
unsupported punctuation remain refused. The 4,096-character limit still counts
characters, including three-byte UTF-8 en dashes.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py scratch/textedit-dash/source.pdf --ranges --dash --spacing -1 --intent Perceptual
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-dash/worker scratch/textedit-dash/source.pdf --dash
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-dash/worker/synthetic-before.pdf scratch/textedit-dash/worker/synthetic-after.pdf --dash
swift scripts/text_edit_pdfkit.swift scratch/textedit-dash/worker --dash
```

Build the separate checks application as described above. Use
`tabs_check.py <checks-binary> <source.pdf> --phase textedit-dash --saved-copy <saved.pdf>`
for the synthetic fixture, and `--phase textedit-agenda` for the unchanged Wellington
agenda identified by `testdata/textedit-public-corpus.json`. The latter edits
`REGULAR` to `ANNUAL` in the first heading, only in disposable copies. Preserve
source and saved files as `synthetic-before.pdf` and `synthetic-after.pdf` in one
readback directory (the existing tool filenames also serve external fixtures).
Pass `--agenda` to both independent readers; the Python reader additionally pins
the original download's SHA-256. Never normalize the input to gain admission.

macOS verification on 2026-09-14 passes 157 focused Rust tests and ten frontend
text-editor tests. Both native workflows pass 15 checks: draft draining on tab
switch, extraction, selection, search, undo/redo and pixel restoration, overflow
refusal, save/reopen and document isolation. The synthetic worker and native
outputs have 939 changed pixels inside the first text region, zero outside.
The unchanged agenda has 236 discovered runs on page 1. Its native output changes
one text operand, with exact font/image/colour resource preservation and exact
untouched-page content. PDFKit reads the replacement and adjacent text correctly:
914 pixels change inside the heading region, zero outside; page 2 has zero changed
pixels. Before/after Poppler renders were also inspected. The shorter replacement
leaves subsequent text at its original position; this does not implement reflow.
Negative controls reject the unedited input, changed font resource and changed
second-page content for their respective reasons.

The same seven unchanged practical PDFs now report 1 editable page and 47 refused
pages. The agenda's second page still refuses curved graphics. This is a small
selected compatibility sample, not a representative success rate. Windows en-dash
and unchanged-agenda verification subsequently passed at `61239a2`, as recorded below.

All eleven selected Rust mutations are caught by their named tests, with 1,456
passing tests in the clean control. The frontend en-dash mutation is caught too,
with 1,494 passing tests in its control. The new `editable-dash` fuzz seed reaches
editable text through the contained probe. The seeded run completes 26,298
executions in 21 seconds, with 88 MiB peak RSS and no finding
(`--sanitizer=none` on macOS). Type checking, Clippy, formatting,
mutation anchors, notices and the normal bundle check pass; normal frontend
assets are restored and contain zero harness code.


Windows x64 verification at `61239a2` passes 157 focused Rust tests, the en-dash
worker probe and both 15-check native workflows (en dash and unchanged agenda
page 1). All seven retrieved PDFs match their Windows sizes and SHA-256 digests.
pypdf and PDFKit independently confirm the Windows outputs: 939 changed pixels
inside the synthetic target, 914 inside the agenda target, zero outside, and
zero changes on agenda page 2. The checks task is removed, normal frontend assets
are restored, and the ordinary checkout remains clean.

### Text editing around curved paths

Complete line/Bezier paths retain their original operators and coordinates.
Multiple nonempty subpaths, cubic `c`/`v`/`y` segments and fill/stroke endings are
supported within the existing stream and coordinate bounds. Every explicit
control point is validated after transformation; no flattening or reconstruction
is performed. Path clipping, mixed rectangle subpaths and interleaved graphics
state/text operators remain refused. `docs/PLAN.md` records the precise grammar.

The acceptance input is the unchanged Wellington agenda in
`testdata/textedit-public-corpus.json`. Its second-page graphic has seven subpaths,
147 cubic segments and 24 line segments before a single fill. The contained
probe now discovers 236 text runs on page 1 and 85 on page 2.

After building the separate checks application:

```sh
uv run scripts/tabs_check.py <checks-binary> <unchanged-agenda.pdf> --phase textedit-agenda-page2 --saved-copy <saved.pdf>
uv run --with pypdf testdata/make_textedit_embedded.py --check <unchanged-agenda.pdf> <saved.pdf> --agenda --page=1
swift scripts/text_edit_pdfkit.swift <readback-directory> --agenda --page=1
```

The Swift reader uses `synthetic-before.pdf` and `synthetic-after.pdf` in the
readback directory, as for the previous external fixture checks. Page indices
are zero based. Only disposable copies are edited.

macOS verification on 2026-09-14 passes 160 focused Rust tests and all 19 native
workflow checks. The application changes `Community Hub` to `Community` on page 2.
pypdf confirms the exact mapped replacement, unchanged font/image/colour resources,
identical other operands (including the whole curved graphic) and byte-identical
page-1 content. PDFKit confirms adjacent text and geometry: 283 pixels change inside
the page-2 heading, zero outside, and page 1 remains pixel-identical. The rendered
second page was also inspected. Negative controls reject an unchanged input,
a deleted curve, modified first-page content and an actual added space in the
replacement operand.

pypdf infers an extra trailing space in the shortened heading before the original,
separately positioned space. Its page-2 text comparison therefore normalizes
extracted whitespace only after checking the exact font-coded replacement and
all other operands. The added-space control must fail that exact operand check;
normalizing extracted text alone would conceal a real change.

All eleven targeted mutations are caught by their named tests; the clean control
passes 1,459 Rust tests. The `editable-curves` fuzz seed reaches text discovery
through the contained probe. The seven unchanged practical PDFs now report two
editable pages and 46 refused pages. Windows verification at `33998a1` passes
160 focused tests and both agenda native workflows. Hash-verified Windows outputs
pass independent pypdf and macOS PDFKit readback: 914 changed heading pixels on
page 1 or 283 on page 2, zero outside the edited heading and zero on the untouched
page. The temporary scheduled task was removed; the normal checkout was unchanged.
The seeded fuzz run completes 27,574 executions in 21 seconds, with 88 MiB
peak RSS and no finding (`--sanitizer=none` on macOS). Clippy, frontend type
checking, formatting, mutation anchors, notices and the normal-bundle check pass.
Normal frontend assets are restored, with zero harness code.


### Word spacing and the next practical target

`Tw` joins `Tc` in the saved graphics state and per-run replacement geometry.
`Tc` and negative `Tw` are bounded to one quarter of the active font size.
Positive `Tw` accepts up to one million text-space units; combined glyph steps
must remain positive and total advances stay within one million. Original
operators, font mappings and resources survive.
ISO 32000-1 section 9.3.3 applies word spacing to single-byte PDF code 32,
regardless of its mapped character. A mapped Unicode space at another code and
all Identity-H two-byte codes receive no word spacing.

For a tab-sized positive gap under `Tf=1` and a scaled text matrix:
`uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py <source.pdf>
--unit-font --word-code space --word-spacing 12.112`. Use ordinary worker and
native `tabs_check.py --phase textedit-wide-spacing` readback, then
`swift scripts/text_edit_pdfkit.swift <readback-directory>
--wide-spacing`; it checks the absolute position of FIRST and requires zero
pixel changes outside the first-line region. Setting `Tw` to zero in the before
and after files must fail that check. The native phase asserts the geometric
column order caused by this untagged gap, rather than assuming line order.
`textedit_continued_discovery_requires_representable_deletion` covers the fuzz
regression where discovery offered a continued run that deletion could not
compensate within one millionth of a page point. The writer retains its separate
check because shortening can require a less representable value than deletion.

Generate the native fixture in both directions (`1` and `-1`):

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py <source.pdf> --ranges --word-spacing 1 --word-code space
uv run scripts/tabs_check.py <checks-binary> <source.pdf> --phase textedit --saved-copy <saved.pdf>
uv run --with pypdf testdata/make_textedit_embedded.py --check <source.pdf> <saved.pdf>
swift scripts/text_spacing_pdfkit.swift <positive-source.pdf> <negative-source.pdf> <positive-saved.pdf> <negative-saved.pdf> --word-char=space
```

`--word-code letter` maps `S` to byte 32; verify it with `--word-char=S`.
Omitting `--word-code` leaves byte 32 absent; `--word-char=none` checks that changing
Tw moves no glyph. The Swift reader's original four-argument Tc mode is unchanged.
On macOS, 2026-09-14: all six worker cases pass independent operand/resource
readback and measured PDFKit glyph positions. Both space-code native workflows
pass 15 checks and the same independent readback. Seven targeted mutations are
caught by their named tests; the clean Rust control passes 1,463 tests (164 focused
text-editor tests). Both native saves change 1,115 pixels inside the target and
zero outside it. Negative controls reject missing word-spacing movement and an
unchanged input. The two new word-spacing fuzz seeds reach discovery; the seeded
run completes 23,734 executions in 21 seconds with 88 MiB peak RSS and no finding
(`--sanitizer=none` on macOS). Clippy, type checking, formatting, mutation anchors,
notices and the normal bundle check pass; the normal bundle contains no harness.
Windows word-spacing verification remains outstanding.

The next unchanged practical input is passport-guide page 16, identified by the
existing manifest digest. Across its 415 operators, every text show has an explicit
position; there are no images or soft masks. Word spacing ranges from -0.125 to
0.01. External graphics state, stroke-state operators and custom Myriad CFF
mappings (including ligatures and curly quotes) still prevent admission. This
increment does not increase the practical corpus's editable-page count.


### Print graphics state during text editing

The editor preserves boolean `OP`, `op` and `SA`, integer `OPM` 0/1,
`SMask /None` and `AIS false` (ISO 32000-1, Table 58). It keeps the entire
ExtGState dictionary and every `gs`/`q`/`Q` operand unchanged, including an omitted
`op`: `OP` sets both overprint parameters when `op` is absent. These print settings
are retained, not simulated. Nondefault alpha sources, active soft masks, malformed
entries, transparency, transfer functions and unknown state remain refused.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_symbolic.py <source.pdf> --ranges --spacing -1 --word-spacing 1 --word-code space --print-state --intent Perceptual --image
uv run scripts/tabs_check.py <checks-binary> <source.pdf> --phase textedit --saved-copy <saved.pdf>
uv run --with pypdf testdata/make_textedit_embedded.py --check <source.pdf> <saved.pdf>
swift scripts/text_edit_pdfkit.swift <readback-directory> --image
```

The generator places opposing overprint states around a graphics-state save;
word/character spacing, an intent and an opaque image exercise preservation
together. The readback directory uses `synthetic-before.pdf` and
`synthetic-after.pdf`. On macOS, 2026-09-14: 166 focused tests, all 15 native
workflow checks, and independent pypdf/PDFKit readback pass. Both worker and
native saves change 1,115 pixels inside the target and zero outside, including
18,816 unchanged painted image pixels. Controls changing `OP`, `op` or `SA` in
a saved resource are each rejected by the independent reader. This measures
saved settings and screen rendering, not a physical overprinting press.
Five targeted mutations are caught; the clean Rust control passes 1,465 tests.
The new `editable-print-state` seed reaches discovery; the seeded fuzz run
completes 25,385 executions in 21 seconds with 89 MiB peak RSS and no finding
(`--sanitizer=none` on macOS). Clippy, formatting, mutation anchors, notices and
the normal-bundle check pass; normal assets are restored with zero harness code.

The unchanged passport guide retains its manifest SHA-256 and all 16 pages now
reach `unsupported embedded CFF font`, past the initial external-state refusal.
This does not establish that later states are supported or make any page editable;
page 16 remains the next practical target. Windows verification of this increment
remains outstanding.


### Custom CFF glyph encodings

CFF fonts may use an explicit WinAnsi-based Encoding dictionary with bounded
Differences: single-byte codes select existing standard ASCII glyph names, or
`.notdef` removes an offer. No implicit base encoding, unknown dictionary keys,
repeated code assignments, dangling range starts or out-of-range codes are accepted.
Valid glyphs must have unique offered codes, matching PDF/program widths and
validated outlines. Width and ink metrics follow glyph identity, not PDF byte
position; word spacing still follows byte 32 even when it represents a letter.

Optional ToUnicode maps use the existing bounded single-byte grammar: bfchar and
scalar bfrange, a 16 KiB decoded limit and unique targets. Every declared target
must agree with the glyph selected by Encoding. Missing entries are not inferred.
The next increment below adds six non-ASCII names; ligatures remain unsupported.
Font programs and all mapping resources remain byte-identical when saving.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_cff.py <fixture-directory>
uv run scripts/tabs_check.py <checks-binary> <fixture-directory>/remapped-unicode.pdf --phase textedit --saved-copy <saved.pdf>
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --check <fixture-directory>/remapped-unicode.pdf <saved.pdf>
swift scripts/text_edit_pdfkit.swift <readback-directory>
```

The generator also writes `remapped.pdf` without ToUnicode. Both swap space and
`S` codes and set nonzero word spacing; the original `synthetic.pdf` is unchanged.
Readback uses `synthetic-before.pdf` and `synthetic-after.pdf` in one directory.
CFF readback now requires fontTools: pypdf otherwise warns about incomplete CFF
decoding and continues. A control independently lacking fontTools is refused.

On macOS, 2026-09-14: the clean Rust mutation control passes, with 1,469 tests
registered in its selection; the 170 focused text-editor tests pass. All seven
targeted mapping mutations are caught. Both worker variants and all 15 native checks pass; independent pypdf and PDFKit readback
confirm only the edited text operand changes, resources stay identical, and
1,852 changed pixels lie inside the target with zero outside. A control adding a
real remapped space is rejected by exact operand decoding, including without
ToUnicode. All 12 pre-existing generator outputs remain byte-identical.
The `editable-cff-mapping` seed reaches discovery; the seeded fuzz run completes
23,584 executions in 21 seconds with 88 MiB peak RSS and no finding
(`--sanitizer=none` on macOS). Clippy, formatting, mutation anchors, notices and
the normal-bundle check pass; normal assets are restored with zero harness code.

The unchanged passport guide reaches `unsupported CFF glyph name` on all 16 pages.
It still contains unsupported non-ASCII names and multi-character ligature targets.
Its ToUnicode declares a two-byte code space while bfchar sources have one byte;
the current strict wrapper does not accept that shape. Further support needs
independent reader comparisons, not a skipped mapping check. The practical-corpus
editable-page count is unchanged; Windows verification remains outstanding.

### Non-ASCII CFF glyphs

The same bounded CFF path now accepts the exact names `minus`, `uni00A0`,
`quoteleft`, `quoteright`, `endash` and `sterling`. The last four also use their
WinAnsi codes. Differences retain the actual glyph name alongside its Unicode
metric slot; the writer never substitutes a space for `uni00A0` or a hyphen for
minus. Widths and ink bounds still come from the selected embedded outline.
NBSP does not receive word spacing unless the PDF assigns it byte 32.

`uni00A0` requires matching ToUnicode: pypdf without that map extracts the literal
name instead of a character. Other offered names may use their standard mappings.
A present ToUnicode must agree and may narrow the repertoire. This increment does
not widen the TrueType or CID map repertoire, accept control characters, or change
the existing single-byte CMap grammar. Ligatures remain refused.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_cff.py <fixtures>
cargo run --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- <worker-output> <fixtures>/unicode-mapped.pdf --cff-unicode
uv run scripts/tabs_check.py <checks-binary> <fixtures>/unicode-mapped.pdf --phase textedit-cff-unicode --saved-copy <saved.pdf>
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --check <fixtures>/unicode-mapped.pdf <saved.pdf> --cff-unicode
swift scripts/text_edit_pdfkit.swift <readback-directory> --cff-unicode
uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20
```

`unicode.cff` is another original geometric font, including a blank NBSP and a
minus with ink on both sides of its advance. `unicode-mapped.pdf` exercises all
six additions; `unicode.pdf` uses an en dash without ToUnicode (`--dash` in the
worker and independent readers). All 14 pre-existing generator outputs remain
byte-identical. PDFKit normalizes NBSP to space in extraction; the exact pypdf
operand check separately rejects a control that replaces NBSP with a plain space.

On macOS, 2026-09-14: 174 focused text-editor tests and both mutation controls
pass. The harnesses register 1,473 Rust and 1,494 frontend tests; those are
registration counts, not pass totals. The full pre-push Rust gate passes 1,470
tests with 3 ignored (two benchmarks and the explicit native-storage check).
All 11 selected Rust mutations and both frontend mutations are caught. Both
worker variants and all 15 native checks pass. Independent pypdf readback preserves all font resources and other operands;
PDFKit reports 1,455 changed pixels within the target and zero outside for both
worker and native output, and 593 within the target for the no-ToUnicode variant.
The new `editable-cff-unicode` fuzz seed reaches one editable run. The seeded
fuzz run completes 25,154 executions in 21 seconds, with 89 MiB peak RSS and no
finding (`--sanitizer=none` on macOS). Clippy, type checking, formatting, mutation
anchors, notices and the normal-bundle check pass; normal assets are restored
with zero harness code.

The unchanged passport guide remains refused on all 16 pages at its unsupported
ligature names. No additional practical page is editable yet. Its CMap code-space
mismatch and later stroke-state operators remain separate work. Windows
verification of this increment is outstanding.

### Line stroke styles during text editing

The editor preserves `J`, `j`, `M` and `d` without rebuilding the surrounding
paths. Caps and joins must be integers 0..2; miter limits must be at least 1.
Dash arrays are limited to 32 entries, with nonnegative lengths and phase, and
at least one positive length in a nonempty array. The existing finite-number
bound of 1,000,000 applies. Empty arrays restore solid lines; zero-length dashes
in an advancing pattern preserve dotted lines. Stroked and clipping text remain
refused. These settings are retained through their original `q`/`Q` scopes.

```sh
uv run --with reportlab testdata/make_textedit_reportlab.py scratch/textedit-stroke-styles/reportlab
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-stroke-styles/worker scratch/textedit-stroke-styles/reportlab/stroke-styles.pdf
uv run --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-stroke-styles/worker/synthetic-before.pdf scratch/textedit-stroke-styles/worker/synthetic-after.pdf
swift scripts/text_edit_pdfkit.swift scratch/textedit-stroke-styles/worker
uv run scripts/tabs_check.py <checks-binary> scratch/textedit-stroke-styles/reportlab/stroke-styles.pdf --phase textedit --saved-copy <saved.pdf>
python3 scripts/mutate_rust.py --only 'stroke styles:'
uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20
```

Measured on macOS, 2026-09-14: three new Rust tests and all nine targeted
mutations pass. The worker round trip and all 15 native checks pass. Independent
pypdf readback finds only the selected text operand changed; PDFKit measures
2,395 changed pixels inside the target and zero outside for both worker and
native output. Changing a saved dash pattern to `[20 10]` is rejected by both
readers; PDFKit detects 3,000 changed pixels outside the target. All 11 existing
ReportLab outputs remain byte-identical. The new `editable-stroke-styles` fuzz
seed reaches one editable run. The seeded fuzz run completes 13,968 executions
in 21 seconds with 86 MiB peak RSS and no finding (`--sanitizer=none` on macOS).
The seven-document practical survey remains at 2 editable and 46 refused pages.
All 24 gates pass (263.4s total): 1,473 Rust tests pass with 3 ignored,
1,668 frontend tests pass, and the normal bundle contains zero harness code.

The unchanged passport guide's page 16 uses caps/joins 0 and 1, miter limit 4,
and dotted patterns with a zero dash and a positive gap. Its source digest
matches the public corpus manifest. Font ligatures and its CMap code-space
mismatch remain separate blockers; this increment does not establish another
editable practical page. Windows native verification remains outstanding.

### Ligatures and matching simple-font maps

CFF text editing supports existing `f_f`, `f_i`, `f_l` and `f_f_i` glyphs only
when their ToUnicode entries exactly spell `ff`, `fi`, `fl` and `ffi`.
Discovery measures original PDF glyph codes, including each `TJ` fragment;
expanding a ligature for extraction does not add character-spacing steps.
Replacement encoding uses the longest available validated sequence. Both source
expansion and replacement text retain the 4,096-character limit. Missing glyphs,
ambiguous maps, compatibility characters and other sequences remain refused.

Simple CFF and WinAnsi TrueType fonts may carry the exact `<0000> <FFFF>`
code-space header with one-byte source entries. The entries themselves remain
one byte; Identity-H and symbolic TrueType retain their separate rules. WinAnsi
ToUnicode entries must be ASCII identity mappings, and all accepted Unicode and
Macintosh cmaps must agree on each offered glyph. No font is repaired or extended.
Named stroke colours retain independent `CS`/`SC`/`SCN` state through `q`/`Q`.
Complete groups of painted rectangles may have negative dimensions; every
rectangle is bounded and preserved, while clipping keeps its stricter subset.

```sh
uv run --with fonttools --with pypdf testdata/make_textedit_cff.py scratch/textedit-ligatures/fixtures --rust-fixtures
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --named-mapping scratch/textedit-ligatures/fixtures/named-mapping.pdf
cargo run --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe -- scratch/textedit-ligatures/worker scratch/textedit-ligatures/fixtures/ligatures.pdf --cff-ligatures
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --check scratch/textedit-ligatures/worker/synthetic-before.pdf scratch/textedit-ligatures/worker/synthetic-after.pdf --cff-ligatures
swift scripts/text_edit_pdfkit.swift scratch/textedit-ligatures/worker --cff-ligatures
uv run scripts/tabs_check.py <checks-binary> scratch/textedit-ligatures/fixtures/ligatures.pdf --phase textedit-cff-ligatures --saved-copy <saved.pdf>
python3 scripts/mutate_rust.py --only 'ligature:'
uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20
```

Use the named-mapping fixture with the ordinary worker, native `textedit` phase
and readback commands, omitting `--cff-ligatures`. For native PDFKit readback,
place the original and saved files in a directory as `synthetic-before.pdf` and
`synthetic-after.pdf`.

Measured on macOS, 2026-09-14: 185 focused editor tests pass; all 25 new or
re-aimed mutations are caught by their named test. Both worker round trips and
both native workflows pass (15/15 checks each). Independent pypdf readback
confirms the exact ligature codes, unchanged resources and untouched operands.
PDFKit measures 2,261 changed pixels inside the ligature target and 898 inside
the named-map target, with zero outside, for both worker and native saves.
Replacing one ligature by its separate letters leaves extracted text unchanged
but fails the exact-code check. Altering a stroke colour fails both readers,
including 864 changed pixels outside the target. The exact wider CFF header
control preserves independently extracted text and pixel-identical rendering.

The new `editable-ligatures` fuzz seed reaches one editable run. The seeded fuzz
run completes 24,730 executions in 21 seconds with 88 MiB peak RSS and no finding
(`--sanitizer=none` on macOS). The seven unchanged, digest-verified public
samples remain at 2 editable and 46 refused pages. Passport guide page 16 now
reaches its final, 90-degree text matrix; it remains refused pending support for
orthogonal text rotation. This is verified grammar coverage, not another
editable practical page. Windows native verification remains outstanding.

All 24 final gates pass (242.9s summed gate time): 1,481 Rust tests pass
with 3 ignored, 1,668 frontend tests pass, and normal assets contain zero
check-harness code.

### Signed-fixture padding regression

`fixturebytes` runs `python3 testdata/test_incremental_pdf.py` without optional
dependencies. The three tests cover all 256 final payload bytes through the
actual BER fixture writer in both hexadecimal cases, short/long outer lengths,
reserved offsets, truncated payloads and nonzero padding. The generator must
consume the encoded outer length; stripping trailing zeros corrupts valid
signatures and caused Windows CI run 34892610186 to fail before the gates.
Both `python3 scripts/mutate_python.py --only fixturebytes` controls turn red
when trimming is restored or nonzero padding is admitted.

On macOS, 2026-09-14, the three tests and two mutations pass. A fresh generation
with the pinned fixture tools writes all 11 signed/encrypted fixtures; OpenSSL
parses both the original CMS and its BER conversion. All 25 final gates pass
(43.1s summed gate time), including 1,481 Rust tests with 3 ignored and
1,668 frontend tests. No application signing behavior changed.

### Prototype producer sample

Seventeen unchanged public PDFs were surveyed on Windows x64, 2026-09-17, to rank
which constructs block ordinary documents. The fifteen new files, with URLs,
producers and digests, are under `prototype_files` in
`testdata/textedit-public-corpus.json`; the W3C files are the existing
`external_test_files`. Use the download recipe above with that key. The sample
is chosen to cover producers, not to estimate a population rate.

Before this increment 3 of 268 pages were editable (the Google Docs invoice and
the W3C dummy file). After it, 123 are, and one more carries no text to edit:

| Producer | Pages | Editable before | Editable after | First refusal now |
|---|---:|---:|---:|---|
| Word via PDFMaker 20 (Coatesville minutes) | 21 | 0 | 21 | none |
| Word via PDFMaker 26 (Hugo minutes) | 7 | 0 | 7 | none |
| Word via PDFMaker 22 (Illinois resumes) | 2 | 0 | 0 | External graphics state, missing glyph |
| Word 2016 (Mercer Island minutes) | 4 | 0 | 0 | Non-embedded TrueType fonts |
| Acrobat 25 (Arcadia agenda) | 127 | 0 | 92 | Content stream filter |
| LiveCycle Designer (Canada Post invoice) | 2 | 0 | 0 | Alt text on paragraphs |
| Designer 6.5 (IRS W-4) | 5 | 0 | 0 | Unrecognized content operator |
| InDesign (three documents) | 54 | 0 | 0 | ClassMap, CFF glyph name, restricted CFF |
| Distiller (council schedule) | 2 | 0 | 0 | External graphics state |
| pdfTeX (two arXiv papers) | 35 | 0 | 0 | Type 1 font programs |
| LibreOffice (W3C headers) | 5 | 0 | 0 | TextAlign layout attribute |
| Google Docs, W3C dummy, Sliced invoice | 4 | 3 | 3 | Incomplete painted path |

The survey reports the first refusal per page only; each widening below was
driven by re-surveying after the previous one. What changed, and why each is
safe for a text edit:

- Parent trees split into `/Kids` subtrees are flattened after checking every
  node's `/Limits`, key order, depth (8) and node count (256).
- `Link` and `Form` elements may own an annotation through `OBJR`. The
  annotation must be listed on its page, have the matching subtype, and carry a
  `/StructParent` whose parent-tree entry names that element; every annotation
  entry must be claimed exactly once. Link and field text stays read-only.
- Pages without `/StructParents`, `null` parent-tree slots, and slots naming
  elements no longer reachable from the root are accepted. Content on unowned or
  orphaned slots is read-only.
- Artifact property lists (Table 330 keys only) are accepted inside and outside
  text objects. `/Artifact` on an owned MCID is refused.
- A content tag no longer has to repeat the owning element's type; the element
  supplies the semantics. Word writes `Span` and `P`, LiveCycle `Content`.
- `THead`/`TBody`/`TFoot`, lists nested directly in lists, lists and figures in
  table cells, elements without `/K`, figure `Width`/`Height`, and the PDF 1.7
  and 2.0 standard namespaces (for types common to both) are accepted.
- `/Alt` is accepted only on read-only owners (Figure, Link, Form). A non-empty
  `/T` is accepted only on elements whose text cannot be edited.
- WinAnsi TrueType fonts with a ToUnicode map may use WinAnsi punctuation
  (0x82-0x9F except the euro sign, plus Latin-1). Glyphs are selected through
  the (3,1) cmap by Unicode value (ISO 32000-1 9.6.6.4); a Macintosh cmap need
  only agree for ASCII. 0x80, 0xA0 and 0xAD stay refused.
- A continued run's TJ compensation is written as an exact integer plus an f32
  remainder, so a whole Word line at `Tf 1` keeps following text within 1e-6
  page points.
- Bounds: 4,096 parent-tree slots per page, 16,384 per document, 1,024
  grouping containers.
- An image may carry a `/SMask`. It is validated as its own DeviceGray image
  against the same rules and charged to the same 8 MiB page budget, so a mask
  cannot smuggle in a second budget; a mask naming a mask of its own is refused
  rather than followed. What the alpha hides is irrelevant to an edit, and the
  image's placement rectangle already bounds where text may not go.
- `/Decode` is accepted only when it equals the colour space's own default
  (ISO 32000-1 Table 89): `[0 255]` for an eight-bit indexed image, `[0 1]` per
  component otherwise. Word and PDFMaker write that default out in full. A
  non-default mapping would mean the samples are not what they appear to be, and
  stays refused.
- `/DecodeParms` is accepted only with `Predictor` absent or 1, which is the
  default and leaves the filter's output as the sample data; the remaining
  entries then describe nothing. `filters::decode_unpredicted` is the entry
  point for that one caller, and `filters::decode` — every page content stream,
  every font program, every ICC profile — still refuses parameters outright.
- An image's `/Metadata` XMP packet is kept unchanged and must declare
  `/Type /Metadata`. It describes the image; nothing in it maps a sample.
- Figure, Link, Form and Table may carry layout attributes, with `/BBox`
  optional and `/Placement` any of the five standard names. Acrobat writes a
  bare `/O /Layout` on a tagged hyperlink and a full bounding box on a table.
- `/BBox` is the element's own ink (Table 344), not an authored allocation, so
  keeping one is only sound while the content it describes cannot move. A
  figure, link and field are read-only already. A table that declares bounds
  makes its own cells read-only — `Tags::bounded` carries those MCIDs — while
  text beside the table stays editable; a table that declares only a placement
  changes nothing. This is the one widening in this increment that takes
  something away, and it takes it from pages that refused entirely before.
- A list may be nested inside an `LBody` as well as beside it. The sublist is a
  container, so `groups` hands it back to the walk that owns the depth and
  container bounds rather than recursing; claiming its id in `groups` would make
  the walk refuse it as a second visit.
- `Note` joins the grouping containers: a footnote or endnote holds ordinary
  blocks exactly as `Div` does, and Acrobat's `Footnote` role maps onto it. One
  that owns marked content directly is still refused.
- A preserved Form XObject may name the layer it belongs to (`/OC`, an OCG or
  OCMD dictionary) and carry `/PieceInfo` and `/LastModified`. None of the three
  is painted. The editor never resolves the layer state and treats the form as
  painted either way, because reserving the box of a form that turns out to be
  hidden refuses a layout that would have fitted, while the reverse would let
  new text land on visible graphics.
- Text state (`Tc Tw Tz TL Tf Tr Ts`) is accepted outside a text object as well
  as inside it, which ISO 32000-1 Table 51 permits and which is where Acrobat's
  page-number stamps set it; positioning and showing still need the text object.
  This is the one that unlocked the 92 pages: every other change above moved the
  agenda's first refusal without making a page editable.

The largest remaining refusal is not a gap. The 50 InDesign pages report
`embedded CFF font does not permit this editable use` because all four Sofia Pro
subsets carry `/FSType 4`, which is Preview & Print embedding: the OpenType
specification says such a document may be viewed and printed but not edited.
Refusing is the correct behaviour and must stay.

Round trips on the unchanged public files, all through the contained worker:

```sh
printf '%s' '[{"page":18,"contains":"possessions","replacement":"items — §","replace_match":true},{"page":1,"contains":"Accounts","replacement":"Acct","replace_match":true}]' > scratch/prototype/rt/final.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/coatesville-minutes.pdf scratch/prototype/rt/final.json scratch/prototype/rt/final
printf '%s' '[{"page":2,"contains":"Playground","replacement":"Park","replace_match":true}]' > scratch/prototype/rt/h.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/hugo-minutes.pdf scratch/prototype/rt/h.json scratch/prototype/rt/h
```

Both report preview/save pixel agreement and unchanged adjacent pixels. The
saved copies were then checked with tools independent of PDFium and lopdf:
`qpdf --check` finds no errors; Poppler's `pdftoppm` at 100 dpi changes only
one text line on each edited page (1,512 and 1,830 pixels on Coatesville pages
2 and 19, 1,390 on Hugo page 3) and no pixel on neighbouring pages; pypdf
extracts the replacements, extracts every other page identically, and keeps
Hugo's namespace dictionary. Replacements that need a glyph the embedded subset
lacks are refused, as before.

The image increment was round-tripped the same way, on the pages that carry the
images: Coatesville page 1 (indexed image with an explicit default `/Decode`) and
Hugo pages 1 and 7 (a soft-masked DeviceRGB image with inert `/DecodeParms`, and
a second indexed image with a `/Decode`). Both pass, and pypdf then reads back
all three image streams and the soft mask byte-identical, with `/SMask`,
`/Decode` and `/DecodeParms` still present, and the replaced text on the edited
page; `qpdf --check` finds no errors in either saved copy.

The agenda was round-tripped the same way, on two of its 92 editable pages
(page 3's heading and page 9's body text, both shortened). It reports
preview/save pixel agreement and unchanged adjacent pixels; `qpdf --check` finds
no errors; and pypdf then reads back all **127** form XObjects byte-identical
with all 127 still on a layer, the whole **2,318**-element structure tree
identical including the five tables that carry attributes, both replacements
present, and every untouched page extracting exactly as before.

Committed fixtures for the image shapes, so this does not depend on the
downloaded corpus: `testdata/make_textedit_alpha.py` writes three PDFs (a
soft-masked RGB image with parameters and a metadata packet, an indexed image
with an explicit default mapping, and both together), and
`scripts/text_image_check.py <text-edit-probe> <new-ignored-directory>` edits
text beside each, checks the streams and entries survive through a second
parser, and then damages one entry per fixture and requires a refusal — without
that last step a generator that stopped writing an entry would pass by producing
an ordinary opaque image.

Verification on Windows x64, 2026-09-17: all 25 gates pass, including 1,675 Rust
tests with 3 ignored and 1,695 frontend tests. `scripts/mutate_rust.py --since HEAD`
selected 274 mutations in the four changed source files. Every one is now caught
by the test it names. Getting there removed one guard that could no longer fail
(a claimed-versus-total comparison the orphan pass had made unreachable), added
tests where the new fixtures had bypassed a guard (artifact-tagged orphans never
reach the orphan path; header links across `THead`/`TBody`; a PDF 2.0-namespaced
`Form`), and repaired three mutations that already failed on the previous
commit: two did not compile and one named a test that could not catch it. macOS
was not run for this increment; CI covers it.

The image widening adds 5 tests and 11 mutations. Ten were caught by the test
named for them at once; the eleventh survived, and it was the mutation that was
wrong rather than the test. It replaced the first half of
`decode.len() != default.len() || <values differ>`, and comparing two `Vec<f32>`
already answers the length, so that conjunct could not change any outcome. The
redundant half is gone and the mutation now disables the comparison itself.

The tagging and form widenings add 11 tests and 23 mutations, and re-aim 6 that
pointed at lines this increment changed. Five survived the first run, and each
was a gap rather than a false alarm. One was caught by the other test of its
pair and only needed its expected name corrected. Two refusal tests did not
discriminate — deferring a child that is not a sublist still ended in a refusal,
just from somewhere else — so a list body holding a `Span` leaf and a list
inside a paragraph were added, both of which change answer under the mutation.
One layout test used a box too small to overlap the form it was about, so
"nothing was reserved" and "something was reserved and missed" looked alike; it
now asserts both directions with one layout. The fifth is the one worth reading
the trap entry for: two frontier bounds shared the message `tagged list frontier
exceeds its limit`, so a `contains` assertion could not tell them apart and the
deferred bound was never exercised by the test written for it. It has its own
message now, and the test pins all three outcomes by exact string.

### Producer sample: tagged metadata, Type 1 fonts and word gaps

The same seventeen files, re-surveyed on Windows x64, 2026-09-18, after each change below.
Editable pages go from 123 to 154 of 268, and no page that was editable before is lost:

| Producer | Pages | Before | After | First refusal now |
|---|---:|---:|---:|---|
| pdfTeX, 2025 arXiv paper | 24 | 0 | 22 | Figure form content, image budget |
| pdfTeX, 2020 arXiv paper | 11 | 0 | 3 | Oversized figure streams, math codes, a non-embedded font |
| LibreOffice (W3C headers) | 5 | 0 | 3 | Read-only title page, inline `BMC` |
| Acrobat 25 (Arcadia agenda) | 127 | 92 | 94 | Content stream filter (scanned pages) |
| Word via PDFMaker 22 (Illinois resumes) | 2 | 0 | 1 | Missing glyph |
| All other producers | 99 | 31 | 31 | unchanged |

Measured before choosing, and worth knowing because each looked like the obvious next step:
the 19 Arcadia pages refused on `CCITTFaxDecode` are scans whose only text is a read-only
page-number form, so a CCITT decoder would move them to "no text" and make none editable;
Canada Post's table cells name a header ID (`...Cell1[0]`) that is absent from the IDTree,
a dangling reference that stays refused; the W-4's CFF fonts carry `/FSType` 4 and are
correctly refused like the InDesign ones.

What changed, and why each is safe for a text edit:

- **Metadata that describes content pins it.** `/Alt` on any element (it stands in for every
  descendant), a non-empty `/T` on an element that owns text, and a non-`Start` `TextAlign`
  keep that element's text read-only instead of refusing the page. They join the table
  `/BBox` rule through the same `Tags::bounded` set. An edit could otherwise leave them
  describing wording or alignment that is gone.
- **`/ClassMap` classes** named by `/C` (at most 8, each optionally followed by a revision
  number) are validated by the same function as the element's own `/A`. Layout attributes
  may omit `Placement` and carry `LineHeight`; inline leaves accept only `LineHeight`.
- **`TOC`/`TOCI`** group ordinary blocks (LibreOffice and Word export one per index), and a
  `Form` may carry `PrintField` attributes (LiveCycle, on every check box).
- **Flatness and smoothness** (`i`, `/FL`, `/SM`) are device tolerances and are preserved.
  Constant alpha (`ca`/`CA` in 0-1) is accepted: an edit keeps the state, so a replacement
  is painted with the alpha of the text it replaces. Blend modes and soft masks stay refused.
- **A Mac Roman `(1,0)` cmap beside `(3,1)`** is accepted without a ToUnicode map under the
  same ASCII agreement check that already applied with one.
- **Embedded Type 1 programs** are parsed and interpreted without executing PostScript (see
  `AGENTS.md`). Codes are offered only where glyph, width and ToUnicode agree with the name.
- **Word gaps and leading offsets.** In a font that cannot write a space, a `TJ`
  displacement of at least 0.18 em reads as a space and is written back as the run's mean
  gap; one leading `TJ` number moves the run's origin and is kept. `docs/TRAPS.md` has why.
- **Opaque glyphs.** A validated glyph the editor cannot write keeps its run read-only with
  its ink reserved, instead of refusing the page.
- **pdfTeX figures.** Forms carrying `PTEX.FileName`, `PTEX.PageNumber` and `PTEX.InfoDict`
  are preserved like any other form.

Round trips on the unchanged public files, through the contained worker. Build the probe
first; `--roundtrip` needs a new output directory each time:

```sh
printf '%s' '[{"page":2,"contains":"reasoning.","replacement":"thinking.","replace_match":true},{"page":2,"contains":"These complementary strengths","replacement":"These strengths","replace_match":true},{"page":4,"contains":"Criterion","replacement":"Criteria","replace_match":true}]' > scratch/prototype/rt/tex2.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/arxiv-recent.pdf scratch/prototype/rt/tex2.json scratch/prototype/rt/tex2
printf '%s' '[{"page":2,"contains":"Header Two","replacement":"Head Two","replace_match":true},{"page":2,"contains":"Header Three","replacement":"Head Three","replace_match":true}]' > scratch/prototype/rt/w3c.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/w3c-headers.pdf scratch/prototype/rt/w3c.json scratch/prototype/rt/w3c
```

Both report preview/save pixel agreement and unchanged adjacent pixels. The saved copies were
then checked with tools independent of PDFium and lopdf: `qpdf --check` finds no errors;
Poppler's `pdftoppm` at 100 dpi changes only the edited lines (arXiv page 3 rows 197-208 and
227-238, page 5 rows 122-130; W3C page 3 two heading bands) and no pixel on neighbouring
pages; Poppler's `pdftotext` reads the rewritten TeX line as "These strengths suggest
potential for hybrid", so the written gaps are spaces to another reader too; pypdf extracts
changed text only on the edited pages and keeps the W3C structure tree at 66 elements.
Replacements that need a glyph the subset lacks (digits in the W3C heading font) are refused.

### Producer sample, continued: kerning, paths, render modes and scans

The same seventeen files, re-surveyed on Windows x64, 2026-09-18. Editable pages go from
154 to 170 of 268, and again no editable page is lost:

| Producer | Pages | Before | After |
|---|---:|---:|---:|
| Acrobat 25 (Arcadia agenda) | 127 | 94 | 99 |
| pdfTeX, 2020 arXiv paper | 11 | 3 | 7 |
| Word (Mercer minutes) | 4 | 0 | 4 |
| pdfTeX, 2025 arXiv paper | 24 | 22 | 23 |
| LibreOffice (W3C headers) | 5 | 3 | 4 |
| Word via PDFMaker 22 (Illinois resumes) | 2 | 1 | 2 |
| All other producers | 95 | 31 | 31 |

What changed, each described in `AGENTS.md`: a `TJ` replacement keeps the source's kerning
and gaps around its changed middle; painted paths under any CTM and movetos that draw
nothing; a 32 MiB image budget; non-embedded WinAnsi TrueType and Type 1 fonts measured by
their widths; text render modes 0-3 with the stroke counted as ink; `/Artifact BMC` inside
a text object; single-point glyphs as empty; and a Type0 name that differs from its
descendant's.

The scanned Arcadia pages were then taken past their image refusals, as the previous
section predicted they would behave: `[/FlateDecode /DCTDecode]` backgrounds, NUL padding
after EOI, CCITT Group 4 stencil masks and an untagged page-number artifact are all
accepted, and the 18 pages move from refused to "no text" (19 in all, with one before).
None becomes editable, because the only text on them is the read-only page stamp. The
support is for OCR'd scans, which pair exactly those images with a mode-3 text layer; this
sample has none, so it is covered by synthetic tests only.

Refused now, 79 pages: 55 are CFF fonts with `/FSType` 4 (Preview & Print), which the
OpenType specification restricts to read-only use, and 24 are spread over seventeen reasons
with no more than four pages each.

Mutations, on Windows x64: the render-mode, artifact, glyph and Type0 changes add 14 and
re-aim 8 (22 run); the image, stencil and untagged-artifact changes add 30 and re-aim 3
(43 run, including their neighbours). Every one is caught by the test named for it. Five
needed a fix first: a compound clip is the only place the stroke margin on ink shows, so
`textedit_compound_clips_contain_the_stroke_around_text` was added; `/Artifact BMC` inside
a text object and the untagged-page tree guard are refused by later checks too, so their
tests now assert the message the survey reports; and a moveto that starts a new subpath
before `h` needed its own refusal case. `stencils` got an explicit type, because a
mutation that never inserts into it otherwise failed to compile rather than run.

### Producer sample, continued: the standard fonts

The twelve Latin standard fonts take the arXiv 2020 paper's first page from refused to
editable (171 of 268; nothing else moves). It was refused only for its side stamp,
`arXiv:2003.00976v2 [cs.SE] 5 Mar 2020`, set in unembedded Times-Roman at 20 pt and
turned a quarter, which every arXiv paper carries. Regenerate or check the width table
with:

```sh
uv run --with reportlab --with pdfminer.six --with matplotlib python scripts/standard_font_widths.py --check
```

(`--with matplotlib` since the table gained each font's FontBBox; see *the newer arXiv
stamp* below. That section also records that this check failed from the day it was
written until then.)

The round trip edits the stamp's date and a line of the title block:

```sh
printf '%s' '[{"page":0,"contains":"5 Mar 2020","replacement":"6 Mar 2020","replace_match":true},{"page":0,"contains":"Kristopher Ambrose","replacement":"Kristopher Ambros","replace_match":true}]' > scratch/prototype/rt/stamp.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/arxiv-2003.pdf scratch/prototype/rt/stamp.json scratch/prototype/rt/stamp
```

It passes with preview/save pixel agreement and unchanged adjacent pixels. It first
failed on the stamp: a same-width digit summed to 337.74 against the scan's
337.73999999999995, and the ink check had no rounding allowance (`docs/TRAPS.md`). The
kerning benchmark from the previous section still gives 69 of 80 after the fix; its 11
refusals are real overruns of the advance.

Mutations: 3 new for the standard fonts and 1 for the rounding allowance, with 2
re-aimed; all caught by the test named for them. A second allowance, on the
kept-kerning path, survived its mutation because that path sums in the scan's own
order, so it was removed rather than tested.

### Producer sample, continued: letter ligatures

Arcadia pages 39 and 40 (indices 38, 39; about 300 words each) are Word exports whose
Calibri composite font maps one glyph to `ft`. The Unicode path now admits any unique run
of two or three letters, which takes the survey to 173 of 268. The AutoCAD brochure page
was also measured: its CFF fonts name ligatures `fi`/`fl`, now accepted, but its TrueType
font carries `/FSType` 4, so it stays refused like the other Preview & Print pages.

```sh
printf '%s' '[{"page":39,"contains":"Ignition software required.","replacement":"Ignition software needed.","replace_match":true},{"page":38,"contains":"SCADA Software Integration","replacement":"SCADA Software","replace_match":true}]' > scratch/prototype/rt/ft.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/arcadia-agenda.pdf scratch/prototype/rt/ft.json scratch/prototype/rt/ft
```

It passes with pixel agreement; `qpdf --check` finds no errors, pypdf extracts changed
text on those two pages only, and both pypdf and Poppler's `pdftotext` read "software"
through the `ft` glyph. Transposition edits on these lines were refused as wider than the
source. They use the same glyphs, so the difference is in the kerning around the swapped
pair; that was inferred, not traced.

Mutations: 3 new (both directions of the letter rule, and the CFF alias), all caught.

### Producer sample, continued: plotted figures

arXiv 2003.00976 pages 2, 4 and 5 (indices 1, 3, 4) hold pdfTeX-included figures, and
were the last refused pages of that paper. Measured with pypdf before changing anything:

| Page | Figure forms (decoded content, operators) | Images drawn by a form | First refusal before |
|---:|---|---|---|
| 1 | 669 KB / 14,768 and 1.5 MB / 38,298 | none | Flate content over the 1 MiB form bound |
| 3 | 390 KB / 11,261 across three | 29 rasters, 12.3 MB decoded | images charged to the form's 1 MiB bound |
| 4 | 4.7 MB / 288,594, plus two smaller | none | Flate content over the 1 MiB form bound |

Page 3 was an accounting defect: images a form draws were charged to the form's content
bound instead of the 32 MiB page image budget (`docs/TRAPS.md`). Pages 1 and 4 needed a
larger form bound. A form is never rewritten, so it now gets its own: 8 MiB of content and
524,288 operators per top-level form tree. Cost, measured on Windows x64 with release
probes run alternately three times: `--inspect --all-pages` on the paper went from about
195 ms to 352 ms, and the worker's peak working set from 41 MB to 181 MB, against the
1 GiB commit cap. Top-level forms are parsed one at a time, so the peak follows the
operator bound, about 330 MB at the limit by the same ratio.

```sh
printf '%s' '[{"page":1,"contains":"Ambrose, Huntsman, Robinson, and Yutin","replacement":"Ambrose, Huntsman, and Yutin","replace_match":true},{"page":3,"contains":"Ambrose, Huntsman, Robinson, and Yutin","replacement":"Ambrose, Robinson, and Yutin","replace_match":true},{"page":4,"contains":"Topological Differential Testing","replacement":"Topological Testing","replace_match":true}]' > scratch/prototype/rt/figures.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/arxiv-2003.pdf scratch/prototype/rt/figures.json scratch/prototype/rt/figures
```

It passes with pixel agreement. `qpdf --check` finds no errors, pypdf reads changed text on
those three pages only, xpdf's `pdftotext` reads the new header, and all 52 XObjects on the
three pages hash the same before and after. The survey goes from 173 to 176 of 268.

Mutations: 5 new (image charging, content charging, both sides of the content bound and
the operator bound) and 1 re-aimed; all caught.

### Producer sample, continued: the newer arXiv stamp

The 2025 arXiv paper's first page (index 0) was the last refused page of that file. Its
figure, the Creative Commons badge pdfTeX includes, is a form with an isolated
transparency group (`/Group << /S /Transparency /CS /DeviceRGB /I true >>`, Inkscape's);
a validated group is now accepted on a preserved form, which is never composited by the
editor. Behind it the stamp itself is `q BT 0 1 -1 0 0 0 cm 1 0 0 1 x y Tm /Times-Roman
20 Tf ... TJ ET Q`: arXiv now turns the CTM inside the text block instead of using a
rotated `Tm` as in 2020. A `cm` before the block's first show is accepted, and the stamp
is read-only because its CTM is not diagonal. Read-only text needs bounded glyphs, and
unembedded Times has no outlines, so each standard font's FontBBox is now in the table.

Two things surfaced on the way. `standard_font_widths.py --check` had failed ever since it
was written, because it compared its own layout against the file after `cargo fmt`; it
now formats its output through `rustfmt` first. And the two FontBBox sources disagree
for the oblique Helvetica styles by one unit and for all four Courier styles by up to
120 units (two revisions of Adobe's Courier), so the table holds their union, which is
sound for a bound.

```sh
uv run --with reportlab --with pdfminer.six --with matplotlib python scripts/standard_font_widths.py --check
printf '%s' '[{"page":0,"contains":"Benchmarking PDF Accessibility Evaluation","replacement":"Benchmarking PDF Accessibility","replace_match":true},{"page":0,"contains":"there is no standardized methodology to evaluate how different","replacement":"there is no standard methodology to evaluate how different","replace_match":true}]' > scratch/prototype/rt/stamp2.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype/arxiv-recent.pdf scratch/prototype/rt/stamp2.json scratch/prototype/rt/stamp2
```

It passes with pixel agreement. `qpdf --check` finds no errors, pypdf reads changed text
on page 0 only, pypdf and xpdf's `pdftotext` read both edits and the stamp, the stamp's
operators are unchanged, and the badge form is byte-identical. The survey goes from 176 to
177 of 268, and a page-by-page diff against the previous report changes no other page.

Mutations: 9 new (the group's four checks, the cm rule both ways, the box lookup, its
reach and the per-font row), all caught.

Letting standard-font text be read-only exposed one existing test that had been passing
for an unrelated reason: a nested-list case asserted as a refusal was accepted by the tag
walk (its content is orphaned, so read-only) and refused only because read-only
Helvetica had no bounds. It now asserts the acceptance.

### Producer sample, second batch

Eight unchanged public PDFs from producers the first sample lacked, surveyed on Windows x64,
2026-09-18. URLs, producers and digests are under `expansion_files` in
`testdata/textedit-public-corpus.json`; they live in `scratch/prototype2/`. The survey
inspects at most 128 pages, so the three longer files are surveyed through an extract that
keeps every stream byte for byte:

```sh
qpdf --stream-data=preserve --empty --pages full/luatex-manual.pdf 1-128 -- luatex-manual-p1-128.pdf
python scripts/textedit_survey.py src-tauri/target/debug/examples/text-edit-probe scratch/prototype2/*.pdf --output <new report>
```

| File (producer) | Pages | Editable before | Editable after | First refusal now |
|---|---:|---:|---:|---|
| Wikipedia export (Chrome, Skia) | 25 | 0 | 0 | Tag nesting (see below) |
| Healdsburg slides (PowerPoint via PDFMaker) | 29 | 0 | 13 | Single-byte character map, read-only-only slides |
| fontspec manual (XeLaTeX, xdvipdfmx) | 71 | 0 | 0 | CID-keyed CFF font |
| LuaTeX manual (LuaTeX/ConTeXt), pages 1-128 | 128 | 0 | 91 | CID-keyed CFF font |
| Typst example | 1 | 0 | 0 | CID-keyed CFF font |
| ReportLab user guide, pages 1-128 | 128 | 88 | 88 | Non-standard fonts, Latin-1, empty rectangles |
| Union County budget (IBM afp2pdf), pages 1-128 | 128 | 128 | 128 | none |
| Pottawattamie County form (Microsoft Print to PDF) | 2 | 0 | 0 | Partly clipped text |

216 of 512 pages were editable; 320 are. The first sample is unchanged by every change
below (same verdicts and run counts on all 268 pages). What changed, each safe because the
editor keeps the bytes and only reads the value:

- PowerPoint: attribute arrays, default `WritingMode`, inline list labels, a label's
  `BBox` (which pins it read-only), indents and spacing on figures, figures under a
  `Diagram`/`Chart` role, figures inside figures, layout attributes on list labels and
  bodies, and each slide's background layer (`/OC` marked content; text inside a layer is
  read-only).
- Typst: `Tf` and `TL` before `BT`. Its font is CID-keyed CFF, the next blocker.
- ConTeXt: ToUnicode CMaps named after the font, and footers whose `TJ` draws the title
  left of the page number (a backtracking run, now read-only rather than refusing the
  page).
- Microsoft Print to PDF: indirect `CIDSystemInfo` strings.

Round trips, each passing with pixel agreement and read back independently (`qpdf --check`
without errors; pypdf and xpdf's `pdftotext` read the edits; only the edited pages change):

```sh
printf '%s' '[{"page":0,"contains":"Healdsburg City Council","replacement":"Healdsburg Council","replace_match":true},{"page":3,"contains":"Dry Creek Commons","replacement":"Dry Creek","replace_match":true},{"page":16,"contains":"Preposed Sequencing:","replacement":"Proposed Sequencing:","replace_match":true}]' > scratch/prototype2/slides.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/healdsburg-slides.pdf scratch/prototype2/slides.json scratch/prototype2/rt-slides
printf '%s' '[{"page":19,"contains":"We currently use Lua","replacement":"We now use Lua","replace_match":true},{"page":41,"contains":"The subtypes 2 and 3","replacement":"Subtypes 2 and 3","replace_match":true}]' > scratch/prototype2/luatex.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/luatex-manual-p1-128.pdf scratch/prototype2/luatex.json scratch/prototype2/rt-luatex
```

Slide 16's title ("Redistricting Partners") is editable but drawn under a full-slide
picture, so an edit to it changes no pixel and the round trip reports `preview changed the
wrong pages or no pixels`. That is the probe's check, which needs a visible change, not a
fault in the edit.

What is left, largest first:

- **CID-keyed CFF composite fonts** (`CIDFontType0` / `CIDFontType0C`): all of fontspec,
  32 LuaTeX pages and Typst. Read since the same day; see *CID-keyed CFF fonts* below.
- **Chrome's tag tree**: containers nest up to 13 deep (the bound is 8), inline elements up
  to 7 deep below a block (`P > NonStruct > Link > NonStruct > NonStruct`), links own
  `NonStruct` children, and some containers own content directly. Supporting it means a
  bounded recursive inline walk rather than the single leaf level.

Mutations: 22 new and 12 re-aimed across tagging, layers, text state, backtracking and
fonts, all caught. Two were removed because the rule they tested was deliberately
relaxed: attributes on list labels refused, and retreating kerning refused.

### CID-keyed CFF fonts

The blocker the second batch left largest: xdvipdfmx (XeLaTeX), LuaTeX and Typst embed
their OpenType CFF fonts as `CIDFontType0` with a bare `FontFile3 /CIDFontType0C`, all
three under Identity-H and Adobe-Identity-0 with a single font dict. Surveyed on Windows
x64, 2026-09-18, against the unchanged files of the second batch:

| File | Pages | Editable before | Editable after | First refusal now |
|---|---:|---:|---:|---|
| fontspec manual (xdvipdfmx) | 71 | 0 | 71 | none |
| LuaTeX manual, pages 1-128 | 128 | 91 | 123 | read-only-only pages (3) |
| Typst example | 1 | 0 | 1 | none |

The whole second batch went from 320 to 424 of 512 pages; the 104 pages that changed are
all refused-to-editable, and every other page of both samples has the same verdict and run
count as before (the first sample stays at 177 of 268). What it took, beyond the CFF
reader itself (`fonts/cff/cid.rs`, described in `AGENTS.md`):

- **ToUnicode headers.** xdvipdfmx writes `CMapName`, `CMapType` and `CIDSystemInfo` in its
  own order; Typst adds DSC comments, `CMapVersion`, `WMode` and a system info built as
  `3 dict dup begin ... end def`, and ends the stream with `%%EOF` and no end of line, which
  lopdf's content parser refuses. The labels are now a small closed grammar.
- **Shared ToUnicode targets.** Pagella's small capitals read as capitals, and a math font
  has several sizes of each parenthesis. Refusing the font for that refused every page; each
  now reads as its text, and a replacement writes it with the glyph its run shows. The
  first rule written, "never write a shared text", passed the survey and failed the first
  round trip: the writer rewrites a whole `Tj` run, so any edit in a run with a capital
  failed.
- **Math widths.** LuaTeX writes TeX's italic correction into the PDF widths of math glyphs
  (879 in the program, 877.9 in `/W`). Such a glyph is read-only at its PDF width, as the
  Type 1 path already kept one.
- **A shown `.notdef`.** fontspec's manual demonstrates a missing glyph six times in a row.
- **Typst's TrueType subsets carry no OS/2 table**, which held the embedding rights. A
  program that declares no rights is now edited as declaring no restriction, as Type 1 and
  CFF programs without an FSType already were (`docs/THREAT-MODEL.md` residual risk 23); a
  present table's restrictions still refuse.
- **A zero-width mark.** Typst writes `DW 0` and leaves its combining macron out of `/W`;
  a zero width is now a read-only mark rather than a refusal of the font.
- **TeX math in simple Type1C.** xdvipdfmx writes CMSY, CMMI and CMR as symbolic Type1C
  fonts with no PDF `/Encoding`; they are read through the Type 1 rules with the program's
  own encoding (format 1 in every font here).

fontTools' `T2WidthExtractor` was the independent check on the width reader: it agrees with
every `/W` mismatch the reader found, on the same glyphs. Round trips, each passing with
pixel agreement and read back independently (`qpdf --check` without errors, `pdftotext`
reads the edits, pypdf reads them apart from LuaTeX's gap spaces, only the edited page's
content changes):

```sh
printf '%s' '[{"page":5,"contains":"behaviour is now obsolete","replacement":"behaviour is obsolete","replace_match":true},{"page":5,"contains":"are required","replacement":"are needed","replace_match":true}]' > scratch/prototype2/fontspec-req.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/fontspec.pdf scratch/prototype2/fontspec-req.json scratch/prototype2/rt-fontspec
printf '%s' '[{"page":6,"contains":"to get activate","replacement":"to activate","replace_match":true}]' > scratch/prototype2/fontspec-math-req.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/fontspec.pdf scratch/prototype2/fontspec-math-req.json scratch/prototype2/rt-fontspec-math
printf '%s' '[{"page":22,"contains":"linked lists","replacement":"linked list","replace_match":true}]' > scratch/prototype2/luatex-cid-req.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/luatex-manual-p1-128.pdf scratch/prototype2/luatex-cid-req.json scratch/prototype2/rt-luatex-cid
printf '%s' '[{"page":113,"contains":"advantages","replacement":"benefits","replace_match":true}]' > scratch/prototype2/luatex-math-req.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/luatex-manual-p1-128.pdf scratch/prototype2/luatex-math-req.json scratch/prototype2/rt-luatex-math
printf '%s' '[{"page":0,"contains":"document","replacement":"text","replace_match":true},{"page":0,"contains":"adipisicing","replacement":"adipiscing","replace_match":true}]' > scratch/prototype2/typst-req.json
src-tauri/target/debug/examples/text-edit-probe --roundtrip scratch/prototype2/typst-example.pdf scratch/prototype2/typst-req.json scratch/prototype2/rt-typst
```

Two of them fix real typos ("to get activate", "a linked lists"). Page 6 is set beside
CMSY10, page 113 beside the italic-correction math font. The Typst edits land in its CID
CFF title font and in Georgia, a TrueType subset without OS/2. "setup" to "set up" on page 5 is
refused as wider than the run, which is the ordinary no-layout rule.

Mutations: 46 new, 4 re-aimed, all caught. Two guards were deleted rather than covered,
because nothing could reach them: a CID-0 check the charset format already guarantees and
the duplicate check makes redundant, and a repeated-key check in the CMap dictionary form
that the three-entry count already refuses. One pre-existing mutation was removed with the
rule it tested (OpenType TrueType without OS/2 refused). A first run also showed one test
case that could not fail (49 operands with no operator after them ends the charstring
before the stack bound is reached), and one mutation that did not compile.

### Edit length and refusals across the public sample — measured 2026-09-19

The question was whether paragraph reflow should be the next text-editing feature: how often
is a replacement refused only because it is longer than the space it has, against refused for
another reason, against accepted, as a function of how much longer it is. Measured on macOS
arm64 at `79d5f53` plus this instrument, over every editable run with visible text on the
first 128 pages of the corpus in `testdata/textedit-public-corpus.json`: 31 of its 32 files
(803 pages, 629 editable, 44,282 runs). `wiki-pdf.pdf` was left out because the URL now serves
different bytes (1,098,738 against the recorded 1,098,693); a regenerated export is a new sample,
not this one.

```sh
python3 scripts/textedit_growth.py src-tauri/target/release/examples/text-edit-probe \
  scratch/reflow-corpus/*.pdf --manifest testdata/textedit-public-corpus.json --jobs 6 \
  --output <new report.json>
uv run --with pypdf scripts/textedit_growth.py src-tauri/target/release/examples/text-edit-probe --self-test
```

`text-edit-probe --growth` (`src/probes/text_edit_growth.rs`) builds, per run, the unchanged
text, a same-length change (two adjacent letters swapped), a quarter shorter, and 10, 25 and
50% longer. The longer ones append only characters the run already has, so a missing glyph
cannot be the reason. Each goes through `textedit::write` in three modes: `app`, the layout the
editor sends when a reader types (`defaultTextLayout`: the box is the run's own advance);
`patch`, no layout, the byte-patch writer; and, for the longer ones, `widened`, the app layout
with the box grown in 5% steps until its own width is no longer the objection, which is what a
reader resizing the box reaches. Refusals are sorted by message into the categories in the
driver, with anything unmatched counted verbatim. Use a release build: the debug probe took 8
minutes for one 7-page file. The whole run took 1,003 s wall on six processes, 1,002 s of it
the one pdfTeX paper whose figure forms every trial re-inspects.

Before any number was used:

- **Synthetic fixture** (the driver's `--self-test`, generated with pypdf): a short word at the
  start of a free line is accepted when widened; a line ending 3 pt from the right edge is refused
  as off the page; a line with another run 2 pt after it is refused as an overlap and attributed
  to that neighbour; a run shown as `[(KER) 80 (NED) 80 (RUN)] TJ` is refused unchanged in the
  editor's box, as box width.
- **Mutations of `textedit::write`** on three files (922 runs): refusing everything moved the
  report to 0% accepted in every column and every refusal into `other`; accepting everything
  moved it to 100%, and the worker comparison below reported 818 disagreements, so it can fail.
  Restored, the same files read 78% unchanged, 28 / 72 / 0 at +10% widened.
- **The in-process verdict is the worker's.** Every 97th trial also went through the contained
  worker's `TextRuns` request, the application's own validation path: 9,601 checked, 0
  verdicts or messages different. Each page's runs are rescanned after its trials, so an
  accepted trial that was not undone would stop the run.
- **An accepted trial is a real edit.** Four accepted +25% widened trials were saved with
  `--roundtrip` (via `--growth-request`) and read back: Word via PDFMaker 20, PDFMaker 26 and
  Typst pass the round trip; all four pass `qpdf --check` and `pdftotext` finds the
  replacement. The LibreOffice one saves and previews identically, but the round trip refuses
  it: PDFium renders 139 pixels of the next line (rows 318-328, only under the edited span,
  largest channel difference 90) differently, while poppler at the same resolution shows the
  following line unmoved to within 0.00002 pt. Unexplained.
- Two full runs gave identical counts for every trial they shared.

Rates are of the runs a trial applies to (a run too short or too uniform for a same-length
change has none). Widened cells are accepted / no room (overlap, off the page, clipped) / other.

| Producer | Runs | Unchanged ok % | Same length ok % | 25% shorter ok % | +25% as typed ok % | +10% widened | +25% widened | +50% widened |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Acrobat 25 (Arcadia agenda) | 7461 | 70 | 67 | 100 | 0 | 38 / 62 / 0 | 29 / 71 / 0 | 23 / 77 / 0 |
| pdfTeX (arXiv 2003.00976) | 3018 | 27 | 16 | 100 | 0 | 27 / 73 / 0 | 17 / 82 / 0 | 13 / 87 / 0 |
| arXiv GenPDF (arXiv 2509.18965) | 3019 | 22 | 18 | 100 | 0 | 56 / 44 / 0 | 34 / 66 / 0 | 24 / 76 / 0 |
| Word via PDFMaker 20 (Coatesville) | 1681 | 53 | 57 | 100 | 0 | 33 / 67 / 0 | 15 / 85 / 0 | 10 / 90 / 0 |
| XeLaTeX / xdvipdfmx (fontspec) | 6190 | 22 | 11 | 100 | 0 | 49 / 51 / 0 | 40 / 60 / 0 | 33 / 67 / 0 |
| PowerPoint via PDFMaker (Healdsburg) | 60 | 40 | 33 | 100 | 0 | 75 / 25 / 0 | 73 / 27 / 0 | 70 / 30 / 0 |
| Word via PDFMaker 26 (Hugo) | 626 | 77 | 77 | 100 | 0 | 31 / 69 / 0 | 24 / 76 / 0 | 18 / 82 / 0 |
| Word via PDFMaker 22 (Illinois) | 241 | 61 | 50 | 100 | 0 | 55 / 45 / 0 | 49 / 51 / 0 | 46 / 54 / 0 |
| LuaTeX / ConTeXt, pages 1-128 | 10283 | 27 | 19 | 99 | 2 | 53 / 45 / 2 | 46 / 53 / 2 | 41 / 57 / 2 |
| Word 2016 (Mercer Island) | 487 | 2 | 1 | 43 | 0 | 15 / 61 / 24 | 15 / 65 / 20 | 13 / 69 / 18 |
| HM Passport Office guidance | 37 | 46 | 46 | 100 | 0 | 84 / 16 / 0 | 78 / 22 / 0 | 54 / 46 / 0 |
| ReportLab, pages 1-128 | 7335 | 100 | 100 | 100 | 0 | 37 / 63 / 0 | 33 / 67 / 0 | 29 / 71 / 0 |
| pdfTeX (arXiv 1706.03762) | 189 | 5 | 3 | 100 | 0 | 63 / 37 / 0 | 49 / 51 / 0 | 28 / 72 / 0 |
| Google Docs (SampleForms invoice) | 97 | 61 | 72 | 100 | 0 | 58 / 42 / 0 | 49 / 51 / 0 | 48 / 52 / 0 |
| Typst | 228 | 83 | 92 | 100 | 0 | 15 / 85 / 0 | 14 / 86 / 0 | 10 / 90 / 0 |
| IBM afp2pdf, pages 1-128 | 3104 | 92 | 96 | 92 | 0 | 10 / 90 / 0 | 10 / 90 / 0 | 7 / 93 / 0 |
| W3C dummy | 1 | 100 | 100 | 100 | 0 | 100 / 0 / 0 | 100 / 0 / 0 | 100 / 0 / 0 |
| LibreOffice (W3C headers) | 68 | 78 | 84 | 100 | 0 | 44 / 56 / 0 | 35 / 65 / 0 | 35 / 65 / 0 |
| Wellington agenda | 157 | 62 | 57 | 100 | 0 | 50 / 50 / 0 | 50 / 50 / 0 | 44 / 56 / 0 |
| **All runs** | 44282 | 52 | 49 | 99 | 1 | 41 / 58 / 1 | 33 / 67 / 1 | 28 / 72 / 1 |
| **Runs of 20+ characters** | 19632 | 52 | 52 | 99 | 1 | 43 / 56 / 0 | 27 / 73 / 0 | 18 / 82 / 0 |

The other twelve files have no editable run with visible text (ten have no editable page).

The same totals as counts, with the refusal categories:

| Trial | Mode | Tried | ok | box width | overlap | off page | clip | box height | glyph | other |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| unchanged | app | 44282 | 22992 | 20873 | 7 | 259 | 0 | 140 | 11 | 0 |
| same length | app | 38108 | 18518 | 19432 | 7 | 130 | 0 | 10 | 11 | 0 |
| same length | patch | 38108 | 32309 | 5733 | 0 | 0 | 0 | 0 | 64 | 2 |
| 25% shorter | app | 39013 | 38463 | 218 | 0 | 259 | 7 | 27 | 9 | 30 |
| +10% | app | 44282 | 390 | 43881 | 0 | 0 | 0 | 0 | 11 | 0 |
| +10% | widened | 44282 | 18246 | 119 | 21680 | 3999 | 48 | 179 | 11 | 0 |
| +25% | app | 44282 | 233 | 44038 | 0 | 0 | 0 | 0 | 11 | 0 |
| +25% | widened | 44282 | 14523 | 103 | 21603 | 7814 | 49 | 179 | 11 | 0 |
| +50% | widened | 44282 | 12190 | 93 | 21667 | 10099 | 44 | 179 | 10 | 0 |

`patch` refuses every longer edit as wider than the original advance, as `app` does. Its two
`other` are "cannot preserve following text at PDF number precision"; the 30 under `app` are
"the selected fallback font does not contain a required character".

Where the +25% widened trials of axis-aligned runs end up, judged against the hit rectangles of
the other discovered runs on the page (read-only text, graphics and form fields are not in
them, so this is a lower bound on what is in the way): accepted within the page's existing text
extent 11,352; accepted only by running past the rightmost existing text, i.e. into the margin,
3,161; overlap with another run on the same line to the right 17,716; overlap with other content
3,887; off the page 7,814. For runs of 20 or more characters the same split is 3,660 / 1,643 /
5,002 / 1,586 / 7,704: two in five would leave the page, and fewer than one in five fits within
the page's existing text extent.

**Verdict.** Length is the refusal that matters once a replacement gets as far as the page: with
the box widened, 58% of +10% and 67% of +25% edits have no room, against at most 1% refused for
any other reason. But reflow is not the next step, because most edits never get that far. In the
box the editor opens, 48% of runs refuse their own unchanged text and 51% refuse a same-length
change, all but a few hundred as wider than the box; the byte-patch writer, which the editor
never sends, accepts 85% of the same same-length changes. The mechanism is shown by the kerned
fixture and by reading `layout::prepare`: it lays the run out again from glyph widths alone,
without the source's `TJ` kerning, so a run the producer tightened no longer fits its own advance.
ReportLab, which writes no kerning, accepts 100% of its runs unchanged, the TeX producers 5 to 27%. The first
increment is therefore the editor's own box: keep the source kerning (or take the patch path) for
text that fits, and size the box to the typed text as far as the free space allows. The second
already has its number: 41% of +10% and 33% of +25% edits fit when the box follows the text,
none of which needs reflow. After that, reflow is the right feature, and its first half is moving
the rest of a line rather than wrapping: 82% of the overlaps (17,716 of 21,603 at +25%) are the
next run on the same line, while wrapping onto a new line is what the 18% leaving the page need.

### Keeping the source's own positioning in the editor's box — measured 2026-09-19

The fix for the verdict above, measured with the same instrument on the same 31 files, macOS
arm64, release probes built from `49cd191` (before) and from the change (after). The corpus
digests and the trials are unchanged; `--records` kept each file's raw verdicts, and
`--compare` diffed them run by run.

```sh
python3 scripts/textedit_growth.py <text-edit-probe> scratch/reflow-corpus/*.pdf \
  --manifest testdata/textedit-public-corpus.json --jobs 6 \
  --output <new report.json> --records <new directory>
python3 scripts/textedit_growth.py <text-edit-probe> --compare <before records> <after records>
```

**Cause, as confirmed.** Two, not one. The layout set the run again from glyph widths and
dropped its `TJ` kerns and word gaps, as the verdict said. But it also set it at the size the
box sends, which `defaultTextLayout` rounds **up** to a thousandth of a point: pdfTeX's
9.96264 became 9.963, so an unkerned `Tj` run was refused unchanged too. A synthetic
`(PLAIN UNKERNED LINE OF TEXT) Tj` at `9.96264 Tf`, and at `Tf 1` under an 11.0417 scale, were
both refused as box width before the change. Horizontal scaling is not a cause (only `100 Tz`
is editable); character and word spacing were already carried over. Two smaller causes showed
up once those were gone: a run whose glyphs reach past its advance or before its origin was
held to the box (and a left overhang shifted the text right), and a run the document already
clips (Word 2016 draws a clip around many lines) was refused for that clip.

**Fix.** `textedit::own_items` is now the one place both writers get their items from: the
source's own items around an unchanged start and end (`kerning.rs`), or where that version does
not fit, the run written afresh. The layout (`layout::source_items`) uses it for a one-line
replacement at the run's own font and size, placed at its own origin, with the box width as
the advance limit and the box or the source's own ink as the ink limit; ink within the source's
own is not held to a clip the source already had. A requested size within 0.001 pt of the
source's is the source's (`layout::own_size`). The default box stays the run's own advance,
kerning included (reason in `defaultTextLayout`'s comment). Anything else, and any
source-positioned edit that then collides with a line or a clip, is laid out as before; that
second try is what makes the run-by-run comparison come out at zero.

| Producer | Runs | Unchanged ok % | Same length ok % | 25% shorter ok % | +10% widened ok % | +25% widened ok % | +50% widened ok % |
|---|---:|---:|---:|---:|---:|---:|---:|
| Acrobat 25 (Arcadia agenda) | 7461 | 70 -> 100 | 67 -> 77 | 100 -> 100 | 38 -> 38 | 29 -> 29 | 23 -> 23 |
| pdfTeX (arXiv 2003.00976) | 3018 | 27 -> 100 | 16 -> 88 | 100 -> 100 | 27 -> 27 | 17 -> 17 | 13 -> 13 |
| arXiv GenPDF (arXiv 2509.18965) | 3019 | 22 -> 100 | 18 -> 86 | 100 -> 100 | 56 -> 56 | 34 -> 35 | 24 -> 24 |
| Word via PDFMaker 20 (Coatesville) | 1681 | 53 -> 98 | 57 -> 64 | 100 -> 100 | 33 -> 35 | 15 -> 15 | 10 -> 10 |
| XeLaTeX / xdvipdfmx (fontspec) | 6190 | 22 -> 100 | 11 -> 77 | 100 -> 100 | 49 -> 49 | 40 -> 40 | 33 -> 33 |
| PowerPoint via PDFMaker (Healdsburg) | 60 | 40 -> 100 | 33 -> 46 | 100 -> 100 | 75 -> 75 | 73 -> 73 | 70 -> 70 |
| Word via PDFMaker 26 (Hugo) | 626 | 77 -> 98 | 77 -> 84 | 100 -> 100 | 31 -> 31 | 24 -> 24 | 18 -> 18 |
| Word via PDFMaker 22 (Illinois) | 241 | 61 -> 100 | 50 -> 59 | 100 -> 100 | 55 -> 55 | 49 -> 49 | 46 -> 46 |
| LuaTeX / ConTeXt, pages 1-128 | 10283 | 27 -> 98 | 19 -> 97 | 99 -> 99 | 53 -> 53 | 46 -> 46 | 41 -> 41 |
| Word 2016 (Mercer Island) | 487 | 2 -> 100 | 1 -> 95 | 43 -> 100 | 15 -> 16 | 15 -> 15 | 13 -> 14 |
| HM Passport Office guidance | 37 | 46 -> 100 | 46 -> 65 | 100 -> 100 | 84 -> 84 | 78 -> 78 | 54 -> 54 |
| ReportLab, pages 1-128 | 7335 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 37 -> 37 | 33 -> 33 | 29 -> 29 |
| pdfTeX (arXiv 1706.03762) | 189 | 5 -> 100 | 3 -> 91 | 100 -> 100 | 63 -> 63 | 49 -> 49 | 28 -> 28 |
| Google Docs (SampleForms invoice) | 97 | 61 -> 87 | 72 -> 72 | 100 -> 100 | 58 -> 58 | 49 -> 49 | 48 -> 48 |
| Typst | 228 | 83 -> 100 | 92 -> 100 | 100 -> 100 | 15 -> 15 | 14 -> 14 | 10 -> 10 |
| IBM afp2pdf, pages 1-128 | 3104 | 92 -> 92 | 96 -> 96 | 92 -> 92 | 10 -> 10 | 10 -> 10 | 7 -> 7 |
| W3C dummy | 1 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 100 -> 100 |
| LibreOffice (W3C headers) | 68 | 78 -> 100 | 84 -> 97 | 100 -> 100 | 44 -> 44 | 35 -> 35 | 35 -> 35 |
| Wellington agenda | 157 | 62 -> 100 | 57 -> 94 | 100 -> 100 | 50 -> 50 | 50 -> 50 | 44 -> 44 |
| **All runs** | 44282 | 52 -> 99 | 49 -> 88 | 99 -> 99 | 41 -> 41 | 33 -> 33 | 28 -> 28 |
| **Runs of 20+ characters** | 19632 | 52 -> 98 | 52 -> 86 | 99 -> 99 | 43 -> 44 | 27 -> 27 | 18 -> 18 |

In counts: unchanged 22,992 -> 43,760 of 44,282; same length 18,518 -> 33,534 of 38,108 (the
byte-patch writer, unchanged, 32,309); 25% shorter 38,463 -> 38,693; widened 18,246 -> 18,342,
14,523 -> 14,557 and 12,190 -> 12,227 at +10/+25/+50%. "As typed" longer edits stay at 1%: the
box is still the run's own advance, which the next increment (sizing the box to the text) is for.

- **Run by run: 36,181 verdicts refused before and accepted now, 0 accepted before and refused
  now**, over all 597,062 (trial, mode) verdicts. A first version without the second try had
  140 regressions, every one a widened longer edit (Word 2016 70 clip, LuaTeX 38, Acrobat 30
  and XeLaTeX 2 overlap): kept at the source's origin, a longer text collided where the layout
  inset by an overhang did not. That is what the comparison is for.
- **What is still refused unchanged** (522 runs): 260 whose default box already leaves the page
  (258 in the afp2pdf file), 179 box height, 83 box width. The same-length remainder is mostly a
  swap next to a kern: `kerning.rs` drops the kern of a changed pair, the text gets wider, and
  the byte-patch writer refuses those too.
- **One refusal changed message, not verdict**: for those 260 runs every longer trial now says
  the box leaves the page rather than that the text is too wide, because the page-edge check
  runs before the layout is tried twice.
- **Worker agreement**: 9,309 trials also sent through the contained worker, 0 disagreements.
  The before sweep reproduced the previous section's table exactly. The driver's `--self-test`
  now requires the kerned fixture to be accepted unchanged and swapped, and fails on the
  probe built before the change.
- Wall time 1,030 s on six processes, as before.

**Round trips with the application's layout** (`--growth-request <file> <page> <op> control
<default width>`, then `--roundtrip`), on same-length edits refused before by every writer the
application sends:

| Producer | Before | After | `qpdf --check` | Readback |
|---|---|---|---|---|
| pdfTeX (arXiv 2003.00976) | box width | pass | clean | `pdftotext` and pypdf find the replacement |
| XeLaTeX (fontspec) | box width | pass | clean | both find it |
| LuaTeX (manual, pages 1-6 extracted with qpdf) | box width | pass | clean | only the two swapped characters differ in `pdftotext` output |
| Acrobat 25 (Arcadia) | box width | pass | clean | both find it |
| Word 2016 (Mercer Island) | ink exceeds the box | pass | clean | both find it |
| arXiv GenPDF | box width | pass | clean | both find it |
| Word via PDFMaker 20 (Coatesville) | box width | **fails**: pixels outside the edit | | |

"Pass" is the probe's own verdict: the preview and the saved file render identically, and no
pixel outside the edited run's box changed. The PDFMaker 20 failure is not this change: the
probe built before it fails the same way on the same run with any box wide enough to be
accepted (365 and 380 pt tried). The changed pixels are on the following lines of the same
text block, x 53-71, y 412-576. That block positions every line with a relative `Td`, so the
layout's closing `Tm`, which restores the line matrix computed in `f64` and written as `f32`,
is the likely cause: PDFium accumulates the same `Td` chain in single precision, and a
difference below the 0.0001 pt the writer checks flips anti-aliased pixels further down.
Unverified; it is the same symptom the LibreOffice round trip above showed.

**The `textedit-overhang` window phase had a stale expectation, found running this change.**
Two of its 23 checks failed: *"a shorter draft whose ink escapes the left edge is refused"*,
and *"saved and reopened text matches the unsaved revision"* as a consequence. The first
required the message *"replacement ink would exceed the original text bounds"*, which only
the byte-patch writer produces, and since 26.9.9 every edit the editor makes carries a
layout. The layout path accepts the draft `ÄÖÜ äöü ß` and insets the line by the leading
Ä's overhang, 0.0234 text units (0.0176 pt), so its ink begins at the box's left edge and
within the source's own ink. Measured with `--roundtrip` on the same fixture and layout
(`--growth-request … 0 14 shrink25 131.362`, replacement changed): the probe built from
`49cd191` and the one built from this change both pass and write **byte-identical** files,
so this change does not touch that edit; the phase was already failing at `49cd191`. The
accepted draft then replaced the journal's revision, which is why the save check read other
text. The phase now expects the draft to be accepted, and undoes it before saving.

### Restoring the line matrix by replaying the source — measured 2026-09-19

The round-trip failure the previous section left unexplained, on Word via PDFMaker 20 and
LibreOffice, measured on macOS arm64 with release probes built from `5f67964` (before) and
from the change (after).

**Mechanism.** After drawing a replacement, the layout restored the line matrix with one `Tm`
carrying the origin the scanner had accumulated in `f64`, rounded to `f32`. The hypothesis that
the `f32` rounding itself was the cause is wrong in the case measured: the value written is the
value PDFium held. What differs is where it is held. PDFium (`CPDF_AllStates`, checked at
`chromium/7881`) keeps `text_matrix_` and `text_line_pos_` apart as floats, adds each `Td`/`TD`
to the position, subtracts `text_leading_` for `T*`, resets the position at `BT` and `Tm`, and
places a show at `text_matrix_.Transform(pos)`. The restore moved the accumulated origin from
the position into the matrix, and float addition is not associative. In the LibreOffice export
(`w3c-headers.pdf`, page 3) the lines hop right and back with `451 0 Td ... -451 -23 Td`, so the
source's following lines start at `fl(fl(56.8 + 451) - 451)` = 56.799988 and the restored ones
at 56.8 + 0 = 56.8. Replaying both streams in `f32` (a script following those members) gives
exactly that 1.1e-5 pt difference on all 23 lines after the hop and on no other, and the changed
pixels lie within those lines (x 68-86, rows 330-541). poppler and the `f64` scanner see no move.

A synthetic block, `19.999992 200 Td (…) Tj 200 0 Td (…) Tj -200 -30 Td …`, reproduces it: 16
starting points a few ulps under a boundary were tried, and only the one crossing a whole point
changed pixels (324), so most lines absorb the shift and a few move a column of pixels.

**Fix.** `layout::restore_line` replays what set the line: the source's own `Tm`, or the
identity a `BT` sets, then every `Td`, `TD`, `T*` and `TL` between it and the run with the same
operands, preceded by the leading in effect at that `Tm` whenever a `T*` is replayed. The
scanner records that origin and leading (`Context::line_origin`). The old `f32` precision check
on the restored origin is gone with the `Tm` it guarded; the `TJ` compensation for a continued
line is unchanged. `'` and `"` are refused by the scanner, and form text is read-only, so
neither reaches the layout.

**Evidence.**

- `a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it` replays the saved
  stream in PDFium's arithmetic and requires every following line start to be bit-identical
  to the source's: across `TD`, `T*`, a leading set before the block and changed after the
  edit, a line continued past the edit, a block `Tm`, quarter-turned text, and edits after the
  hop. It fails on the code before the change (`SECOND` at bits 1101004796 against
  1101004800).
- `uv run --with pypdf scripts/text_continuation_check.py --generate <path> --line-drift`
  writes the synthetic block; `--growth-request <path> 0 3 control 108` then `--roundtrip`
  fails on the before probe (pixels outside the edit) and passes on the after probe.
- Seven mutations in `scripts/mutate_rust.py` (`--only "line restore"` and `--only "boxed
  edit: lose original line origin"`), each red on its named test. Two survived a first run,
  because the fixture set its leading inside the block where replay reproduced it anyway; the
  fixture now sets it before `BT`.

Same-length edits in the default box (`--growth-request … control <default width>`, then
`--roundtrip`), before -> after:

| Producer | Trials | Pass before | Pass after |
|---|---:|---:|---:|
| LibreOffice (W3C headers), every run | 59 | 33 | 59 |
| Word via PDFMaker 20 (Coatesville), every 10th | 97 | 72 | 97 |
| Acrobat 25 (Arcadia) | 25 | 19 | 25 |
| Word via PDFMaker 26 (Hugo) | 25 | 16 | 25 |
| Word via PDFMaker 22 (Illinois) | 25 | 20 | 25 |
| Typst, Word 2016 (Mercer), XeLaTeX (fontspec), pdfTeX (arXiv 2003), PowerPoint (Healdsburg), Google Docs (SampleForms), Wellington | 25 each | 25 each | 25 each |

Every before-failure was "pixels changed outside edited text envelopes". The ReportLab guide
cannot be round-tripped whole (over 128 pages) and was left out. One saved edit per producer
from the table (nine) passes the probe, `qpdf --check` clean, and `pdftotext` and pypdf both
find the replacement on its page.

**No regression.** The growth instrument over the same 31 files with `--records`, compared
with the records of the previous section's after run: 597,062 verdicts unchanged in kind, 0
refused before and accepted now, 0 accepted before and refused now; 9,309 worker agreement
checks, 0 disagreements; 1,089 s on six processes.

### Moving the rest of the line along — measured 2026-09-20

When a replacement needs more room than the line has free, the runs after it on that
line are pushed along by exactly the distance it overran, and the edit is accepted
instead of refused. `layout::reach` answers how far that set may go and what stops it;
`layout::drag` decides which shows need a displacement of their own; `layout::push`
writes it. The rule, and why it is a rule rather than a consequence, is in
`docs/PLAN.md` §7.

**The push moves the text cursor, never the line matrix.** A displacement inside a
`TJ` array moves the cursor; `Td`, `TD`, `T*` and `Tm` move the line matrix that every
following line accumulates in single precision. Rewriting one of those would
reintroduce the drift *Restoring the line matrix by replaying the source* was written
to remove, so the push rewrites each moved show's own array instead.

**Measured on the 31-file public sample, 44,282 runs, macOS arm64, release probe:**

| trial, as typed | before the push | after |
|---|---:|---:|
| +10% longer | 44% (19,389) | **58% (25,847)** |
| +25% longer | 33% (14,756) | **48% (21,195)** |
| +50% longer | 28% (12,316) | **41% (18,323)** |
| same length | 97% | 97% |
| unchanged | 99% | 99% |

`--compare` against the pre-push records: 578,093 verdicts unchanged in kind, 18,968
refused before and accepted now, **1 accepted before and refused now**, and that one is
the defect below. 9,309 worker-agreement checks, 0 disagreements, 1,018 s on six
processes.

**The one regression, and what it cost to find.** On page 104 of the ReportLab guide a
run of one character stopped being able to save its own **unchanged** text: *"There is
no room for more text on this line: the text after it cannot be moved."* The push's
origin — the point text has to pass before anything moves — was held to the run's own
far edge but not to the box that arrived. The editor's default box is the run's advance
rounded **up** to the thousandth, so text that merely fills the box measures a hair past
that edge, which the push read as growth and then refused wherever the line could not
move. `from` is now also held to the box's own width, and the run saves its own text
again.

⚠ **That fix is covered by the corpus comparison and not by a unit test.** Three
synthetic fixtures were built to reproduce it — a rounded box against a neighbour that
cannot move, the same with the neighbour inside the run's own ink, and the same again at
a coordinate where single-precision steps are an order of magnitude coarser — and in all
three the identity edit writes the same bytes with the fix and without it, so a test over
them could not fail. What the real page has and the fixtures do not was not established;
the honest statement is that the instrument that found this defect is the run-by-run
comparison over real documents, and that is what has to be run when this code changes.
An assertion that cannot go red was deleted rather than kept for the look of it.

### The editing box follows the typed text into the room on its line — measured 2026-09-20

The second increment the length survey asked for. Until now the box the editor opens was
exactly the run's own advance, so a replacement longer than the text already there was
refused whatever the rest of the line held: 1% of +10% edits and 1% of +25% edits were
accepted as typed. macOS arm64, release probes built from `760e013` (before) and from this
change (after), over the same 31 files as the two sections above; the corpus digests, the
trials and the probe's arithmetic are unchanged.

```sh
python3 scripts/textedit_growth.py <text-edit-probe> scratch/reflow-corpus/*.pdf \
  --manifest testdata/textedit-public-corpus.json --jobs 6 \
  --output <new report.json> --records <new directory>
python3 scripts/textedit_growth.py <probe> --compare <before records> <after records>
uv run --with pypdf scripts/textedit_growth.py <text-edit-probe> --self-test
```

**The rule, and where it is.** `layout::room` answers how wide the box may be and what stops
it there, as pure geometry over displayed-page rectangles so it can be stated in numbers
without a document; `layout::free_width` is the adapter that builds those rectangles for a
discovered run. The box grows along the run's own text axis until it meets the first thing on
its line, and no further than the page's edge or the clip in force.

- **The direction** is read off the box's own display rectangle at two widths, not derived
  from the page's quarter turns. A second hand-written table of turns is what `text::to_device`
  warns about, and reading it costs one extra mapping.
- **The probe width** those two rectangles are taken at is about one page point of box, not a
  fixed unit of text space: a display rectangle is `f32`, and Word's `Tf 1` under a matrix of
  11 and a `0.001` page transform are both real, so a fixed unit can be a few ulps at a page
  coordinate.
- **A neighbour** is any hit rectangle the collision check in `prepare` would compare against
  — another run, preserved read-only text, a form field — and both read **one** list
  (`layout::obstacles`), so the box cannot be grown into something that check would then
  refuse it for. It counts as on this line when it overlaps the box's cross-axis span, taken
  together with the run's own hit rectangle, by more than the 0.1 pt that check ignores. The
  box stops at its near edge.
- **A compound clip** is a set of rectangles with holes rather than an edge, so the box growth
  would produce is handed to the region instead of being reduced to a coordinate; growth is
  given up when the region refuses it.
- **The box does not grow to the left.** The writer places a replacement at the run's own
  origin and replays the source's own positioning to get there, so starting a line further left
  is a move of the line rather than a wider box for it. A line set flush against the right edge
  has a whole empty page to its left and does not grow.

**Which box is the reader's.** `Layout.grow` (`TextLayout.grow` in the frontend) says the
reader has not sized this box. `defaultTextLayout` sets it; `TextLayoutControls` clears it from
the **width** control's own input event and takes the answer back from whatever layout `set`
is given, so picking a font or ticking wrapping — neither of which says anything about the
width — leaves the box free to follow the text, and a stored change that carried a sized box
reopens sized. It defaults to off in the request, so a saved journal, a probe's request file
and every test keep the box they name. The box reported back to the editor is the size of the
**text**, not of the room it had, so the dashed outline a reader sees follows what they typed.

**When the text no longer fits**, the refusal names what filled the line rather than a box the
reader never set: *"there is no room for more text on this line: other text follows it"*, *"…
it reaches the edge of the page"*, *"… the document clips the space after it"*. The driver
sorts these under a `line_full` category of their own rather than as more needles under
`box_width`, because widening such a box by hand cannot help and the probe's 5% ladder would
climb straight past the neighbour that stopped it.

Rates are of the runs a trial applies to. "As typed" is the `app` mode: the layout the editor
sends when a reader types, which since this change carries `grow`.

| Producer | Runs | Unchanged ok % | Same length ok % | 25% shorter ok % | +10% as typed | +25% as typed | +50% as typed |
|---|---:|---:|---:|---:|---:|---:|---:|
| Acrobat 25 (Arcadia agenda) | 7461 | 100 -> 100 | 77 -> 98 | 100 -> 100 | 0 -> 46 | 0 -> 29 | 0 -> 23 |
| pdfTeX (arXiv 2003.00976) | 3018 | 100 -> 100 | 88 -> 97 | 100 -> 100 | 0 -> 27 | 0 -> 18 | 0 -> 13 |
| arXiv GenPDF (arXiv 2509.18965) | 3019 | 100 -> 100 | 86 -> 99 | 100 -> 100 | 0 -> 54 | 0 -> 33 | 0 -> 24 |
| Word via PDFMaker 20 (Coatesville) | 1681 | 98 -> 100 | 64 -> 85 | 100 -> 100 | 0 -> 40 | 0 -> 16 | 0 -> 10 |
| XeLaTeX / xdvipdfmx (fontspec) | 6190 | 100 -> 100 | 77 -> 93 | 100 -> 100 | 0 -> 48 | 0 -> 40 | 0 -> 33 |
| PowerPoint via PDFMaker (Healdsburg) | 60 | 100 -> 100 | 46 -> 100 | 100 -> 100 | 0 -> 75 | 0 -> 73 | 0 -> 70 |
| Word via PDFMaker 26 (Hugo) | 626 | 98 -> 99 | 84 -> 99 | 100 -> 100 | 0 -> 32 | 0 -> 24 | 0 -> 18 |
| Word via PDFMaker 22 (Illinois) | 241 | 100 -> 100 | 59 -> 99 | 100 -> 100 | 0 -> 57 | 0 -> 49 | 0 -> 46 |
| LuaTeX / ConTeXt, pages 1-128 | 10283 | 98 -> 98 | 97 -> 99 | 99 -> 99 | 4 -> 58 | 2 -> 46 | 1 -> 42 |
| Word 2016 (Mercer Island) | 487 | 100 -> 100 | 95 -> 97 | 100 -> 100 | 0 -> 15 | 0 -> 15 | 0 -> 13 |
| HM Passport Office guidance | 37 | 100 -> 100 | 65 -> 100 | 100 -> 100 | 0 -> 81 | 0 -> 76 | 0 -> 57 |
| ReportLab, pages 1-128 | 7335 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 0 -> 38 | 0 -> 33 | 0 -> 30 |
| pdfTeX (arXiv 1706.03762) | 189 | 100 -> 100 | 91 -> 97 | 100 -> 100 | 0 -> 63 | 0 -> 61 | 0 -> 28 |
| Google Docs (SampleForms invoice) | 97 | 87 -> 100 | 72 -> 100 | 100 -> 100 | 0 -> 57 | 0 -> 49 | 0 -> 48 |
| Typst | 228 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 0 -> 14 | 0 -> 13 | 0 -> 8 |
| IBM afp2pdf, pages 1-128 | 3104 | 92 -> 92 | 96 -> 96 | 92 -> 92 | 0 -> 10 | 0 -> 10 | 0 -> 7 |
| W3C dummy | 1 | 100 -> 100 | 100 -> 100 | 100 -> 100 | 0 -> 100 | 0 -> 100 | 0 -> 100 |
| LibreOffice (W3C headers) | 68 | 100 -> 100 | 97 -> 97 | 100 -> 100 | 0 -> 62 | 0 -> 35 | 0 -> 35 |
| Wellington agenda | 157 | 100 -> 100 | 94 -> 100 | 100 -> 100 | 0 -> 49 | 0 -> 49 | 0 -> 44 |
| **All runs** | 44282 | 99 -> 99 | 88 -> 97 | 99 -> 99 | 1 -> 44 | 1 -> 33 | 0 -> 28 |
| **Runs of 20+ characters** | 19632 | 98 -> 99 | 86 -> 97 | 99 -> 99 | 1 -> 50 | 1 -> 29 | 0 -> 19 |

In counts, "as typed": +10% 390 -> 19,389 of 44,282; +25% 233 -> 14,756; +50% 104 -> 12,316.
Same-length edits went 33,534 -> 37,090 of 38,108 and unchanged text 43,760 -> 43,822 of
44,282, because a box that may grow also lets a producer's own kerning fit where the run's own
advance did not.

**The as-typed rate now passes the ceiling a reader could reach by hand**, which the survey
put at 41 / 33 / 28%: 19,389 against 18,342 at +10%, 14,756 against 14,557 at +25% and 12,316
against 12,227 at +50%. Two reasons, and neither is that the ceiling was wrong. The ladder
steps the box by 5% until the objection stops being the box's own width, so it stops at a
width the room may not have and at one the room had more than; and it clears `grow`, so it
reaches the run through the general layout rather than through the source's own positioning.
The `widened` column is deliberately unchanged by this work — `grow` cleared, same ladder —
and it reads 18,342 / 14,557 / 12,227 before and after, to the unit.

**No regression, and it took two tries to get there.** Run by run over all 597,062 (trial,
mode) verdicts, against the records of the *Restoring the line matrix* run: **49,354 refused
before and accepted now, 0 accepted before and refused now**; 9,309 worker agreement checks, 0
disagreements; 985.6 s on six processes.

A first version grew the ceiling and gave the writer only the grown box to work in — the
source's own positioning at the ceiling, then the general layout at the ceiling. That turned
**14 verdicts from accepted into refused**: 13 same-length edits in Word 2016 (eleven as an
overlap with the next line, two as the line being full) and one in XeLaTeX where the general
layout has no glyph for a character the source's own items carry. The mechanism is that
growing the ceiling changes *which version* `own_items` picks: a producer's kerning that did
not fit the run's own advance fits a wider box, and the kept items are wider than the rewrite
they displace, so an edit that used to be written at the source's origin was handed to the
general layout, which insets the line by an overhang and put ink where the source had none.
`prepare` now runs four writers and takes the first that succeeds — the source's positioning
in the grown box, then in the box that arrived, then the general layout in each — so the last
two are the writer exactly as it was before the box could grow. **Growth may add room; it may
never take a writer away.**

⚠ **Those last two are covered by this comparison and not by a unit test**, which is the
honest state of it. Reaching them needs a page where the grown attempt fails and the ungrown
one succeeds, and that needs an embedded font with *varying glyph widths* (so that keeping the
source's kerns changes the advance) *together with* right ink overhang or a partly clipped
run. A sweep of 5 kerned fixtures x 7 replacements x 11 neighbour positions, run against a
copy of `layout.rs` with the ungrown pair deleted, found none: every fixture in the tree with
ink overhang has uniform 600-unit widths, and every one with varying widths is Helvetica,
whose ink does not pass its advance and which discovery refuses to clip at all. Building that
font is the work a unit test here would take.

**Round trips**, one per producer, on `grow25` trials the app layout accepted — every one a
case where the text genuinely grew into the room after the line, since the box the request
carries is the run's own advance and `grow` does the rest:

```sh
<probe> --growth-request <file> <page> <operator> grow25 <default width> grow > <req.json>
<probe> --roundtrip <file> <req.json> <new directory>
```

| Producer | Page | Chars | Probe | `qpdf --check` | `pdftotext` | pypdf |
|---|---:|---:|---|---|---|---|
| Acrobat 25 (Arcadia agenda) | 2 | 40 | pass | clean | finds it | finds it |
| pdfTeX (arXiv 2003.00976) | 1 | 32 | pass | clean | finds it | finds it |
| arXiv GenPDF (arXiv 2509.18965) | 1 | 41 | pass | clean | finds it | finds it |
| Word via PDFMaker 20 (Coatesville) | 1 | 35 | pass | clean | finds it | finds it |
| XeLaTeX / xdvipdfmx (fontspec) | 1 | 14 | pass | clean | finds it | finds it |
| PowerPoint via PDFMaker (Healdsburg) | 1 | 24 | pass | clean | finds it | finds it |
| Word via PDFMaker 26 (Hugo) | 1 | 13 | pass | clean | finds it | finds it |
| Word via PDFMaker 22 (Illinois) | 1 | 39 | pass | clean | finds it | finds it |
| Word 2016 (Mercer Island) | 1 | 25 | pass | clean | finds it | finds it |
| HM Passport Office guidance | 16 | 47 | pass | clean | finds it | finds it |
| pdfTeX (arXiv 1706.03762) | 11 | 85 | pass | clean | finds it | finds it |
| Google Docs (SampleForms invoice) | 1 | 27 | pass | clean | finds it | finds it |
| Typst | 1 | 22 | pass | clean | finds it | finds it |
| W3C dummy | 1 | 14 | pass | clean | finds it | finds it |
| LibreOffice (W3C headers) | 3 | 30 | pass | clean | finds it | finds it |
| Wellington agenda | 1 | 21 | pass | clean | finds it | finds it |

"Pass" is the probe's own verdict: the preview and the saved file render identically and no
pixel outside the edited run's box changed. `qpdf --check` exits 0 with *"No syntax or stream
encoding errors"* on all sixteen. The pypdf readback asserts the replacement **is** on the
saved page and **is not** on the same page of the source, so a reader that simply failed to
find anything cannot pass it. LuaTeX/ConTeXt, ReportLab and the Union County budget are over
128 pages and `--roundtrip` refuses them (`unsupported page count`), as before.

**Unit tests.** Six over `layout::room` alone, in the displayed page's own space and with the
numbers written out: the room reaching the page edge and stopping 40 pt short of a neighbour
in **each of the four growth directions**, a neighbour 2 pt away, a clip with a nearer
neighbour winning, a neighbour off the line and one overlapping it by less than 0.1 pt, a
neighbour the run's own glyphs reach above or below the box, and a box already wider than the
page keeping its width. Seven more through the writer on real pages: free space, a neighbour 2
pt on (with the same edit accepted once that neighbour moves away, so the refusal is the
neighbour rather than the length), the page edge, a line flush against the right edge that does
not grow leftwards, a rectangular clip, a compound clip that gives growth up, a kerned run that
keeps its kerns because the box grew, the reported box being the size of the text, and a
fallback-font replacement growing too.

**Two window phases had drafts that had stopped overflowing** (found by the lead running
them on a checks build of this tree, macOS). One site in `src/lib/opencheck.ts` builds a
draft to be refused — `(passport ? "I" : agendaPage2 ? "C" : agenda ? "R" : w3c ? "l" :
"S").repeat(80)` — and a draft only has to be wider than the *room* now, not wider than the
run's own advance. Every one of the five was measured through the worker, with
`--growth-request … grow` and `--roundtrip`:

| phase group | draft | verdict | |
|---|---|---|---|
| `textedit-passport` (passport guide p16, op 413) | `"I" x 80` | **accepted, round-trips clean** | stale |
| `textedit-w3c` (W3C dummy, op 10) | `"l" x 80` | **accepted, round-trips clean** | stale |
| `textedit-agenda` (Wellington p1, op 65) | `"R" x 80` | refused: other text follows it | still valid |
| `textedit-agenda-page2` (Wellington p2, op 62) | `"C" x 80` | refused: reaches the edge of the page | still valid |
| every other textedit phase (embedded fixture, op 3) | `"S" x 80` | refused: reaches the edge of the page | still valid |

The passport one is not a defect in the free-space rule and was checked as one before being
called stale: `"I" x 80` **round-trips** — preview and saved file render identically and no
pixel outside the edited box changes — the same draft with `grow` cleared is still refused as
box width, and the line does fill up, at `"I" x 200`, refused as *other text follows it*. That
label is vertical and runs up the right margin, which is why it holds 150 of them. The count
is now **1,000**, measured to be refused for the room on all five, and short of both the
4,096-character bound and any missing glyph. The check now also asserts **why** it refused
(*"no room for more text on this line"*): it asserted only that the apply threw and the journal
did not move, which a missing glyph or a character bound satisfies just as well — which is how
two of five could stop testing anything without going red, and why only the one phase that was
run showed up. `"x".repeat(21)` at the other site is a glyph refusal and is unaffected.

**A window phase was added and deliberately not run here**: `tabs_check.py --phase
textedit-grow`, on the same fixture as `--phase textedit`. Its first run by the lead was
24/25: *"a longer draft previews in a line with room after it"* failed on
`testdata/textedit-embedded.pdf`, and the phase was wrong rather than the product — that
line has 260 pt of room and takes **sixteen** more of its own characters, while the nine of
`" AND MORE"` are wider than that. The draft is four of the run's own characters now, which
is how the growth instrument builds its longer trials and for the same reason. Everything this increment does
that a reader can see happens in the running application — the preview and its dashed
outline follow the typed text, and the status line names what filled the line — and none of
it is reachable from a gate. The phase types a longer draft and requires a preview, types a
much longer one and requires *"no room for more text on this line"* with no mention of a box,
then dispatches an input on the width control and requires the refusal to go back to naming
the box the reader now owns. It leaves the draft as it found it and applies nothing, so it
does not disturb the rest of that phase's flow.

**Mutations.** Eighteen in `scripts/mutate_rust.py` (`--only "room: " --only "free width: "
--only "grown box: "`) and five in `scripts/mutate_frontend.py` (`--only "grown box: "`), each
red on the test named for it. Three survived a first run and all three were findings rather
than variants: the cross-axis span's union with the run's own hit rectangle was a no-op in
every fixture (a new test gives the run a descender that reaches past the box); handing the
source's items the grown ceiling changed only *which* items were written, not the verdict (a
new test asserts the kerns survive an edit that needed the room); and checking the page
rectangle at the grown box rather than at the one that arrived could not fail at all, because
`room` already holds a grown box inside the page — that second guard was deleted rather than
given a test.


### What a paragraph model would have to work with — measured 2026-09-20

Wrapping an edit onto a new line is the half of reflow the line push does not reach, and
`docs/PLAN.md` §7 says it needs a paragraph model the scanner does not have: which runs are
lines of one block, the leading between them, where the block ends, and what may be pushed
down. This measures how much of the still-refused population wrapping could serve, where that
model would come from, and what a page has room for. Nothing was built.

macOS arm64 at `dbb4bee` plus this instrument, over the same 31 files as the three sections
above (`testdata/textedit-public-corpus.json`, 803 pages, 629 editable, 44,282 runs).

```sh
python3 scripts/textedit_blocks.py <text-edit-probe> scratch/reflow-corpus/*.pdf \
  --manifest testdata/textedit-public-corpus.json --jobs 6 \
  --records <new directory> --output <new report.json>
python3 scripts/textedit_blocks.py <probe> --report <records> --rule all|none --output <new>
uv run --with pypdf scripts/textedit_blocks.py <text-edit-probe> --self-test
```

`text-edit-probe --blocks` (`src/probes/text_edit_blocks.rs`) runs the three longer trials of
`--growth` in the one mode the editor sends (`app`, with `grow`), and adds three things that
instrument does not carry: the MCID and owning structure element of every run, read from the
parent tree **by the probe rather than by `textedit::tagging`**; and `ink_below`, the clear
distance from a run's hit rectangle straight down to the first pixel that is not the page's
own background, from one render per page. The block rule lives in the driver, not the probe,
so the instrument cannot be the evidence for its own rule. 120.8 s on six processes — an
eighth of `--growth`'s, because the 5% widening ladder is most of that instrument's cost.

Before any number was used:

- **The refusal population is the release's own.** Every one of the **132,846** (page,
  operator, trial) verdicts this probe produced is byte-identical to the corresponding verdict
  in the 26.9.15 records, with no key in one set and not the other. Two probes, two runs, one
  answer.
- **1,362 worker-agreement checks, 0 disagreements**, and each page's runs are rescanned after
  its trials, so an accepted trial that was not undone would stop the run.
- **A synthetic fixture with a known answer for every question** (the driver's `--self-test`):
  a tagged page whose two paragraphs are geometrically identical and stated apart only by the
  tags; an untagged page whose chain is ended by leading alone and whose next join is stopped
  by a painted rule alone; and a third whose lines carry a gutter, three fragments that are
  not a gutter, a pair differing only in left edge, one differing only in font and one
  differing only in size.
- **Mutations, both halves.** Four in the probe — the structure read always answering
  untagged, the MCID walk finding nothing, the ink scan always reaching the page edge, the
  ink test never firing — and fifteen in the driver, one per rule constant and one per signal.
  Every one of the nineteen turned the self-test red. Three earlier survivors were findings
  rather than variants: the fixture had no inline `Span` to climb past, no chain whose pitch
  steps, and no gutter, and each got a case. A fourth survivor was a **guard that could not
  fail** — the rule tested horizontal overlap, which `pairs` already requires of every pair it
  hands over — and it was deleted rather than given a fixture.
- **`--rule all` and `--rule none` move the corpus numbers, not only the fixture's**: false
  joins go 577 -> 1,355 -> 0 and blocks 13,452 -> 3,970 -> 20,569.

⚠ **`scripts/textedit_growth.py --self-test` was red on the tree that shipped 26.9.15**, found
by running it here as the pre-flight its own docstring says it is. Its fixture asserts that a
run 2 pt short of a neighbour is refused; the line push made that case an acceptance, correctly,
and the assertion was left behind. No gate runs that harness, and the push increment measured
itself with the corpus comparison instead, which cannot fail on a stale expectation. Corrected
in the same session: the assertion now reads `ok`, and the fixture gained a second neighbour
with the room behind it spent, so eight more characters are still refused and at the page rather
than at the neighbour. Both halves were mutated in the fixture and both went red. The trap index
has the entry.

One defect the fixture found in the driver, worth recording because it is a shape rather than a
typo: block membership was decided from the edge set, so a line the leading check *dropped* from
a chain still carried its incoming edge, was therefore not a chain start, and appeared in no
block at all. Five blocks reported as four. Chain starts are now decided by walking in reading
order, where a line that is neither claimed nor claimed-from is a start by construction.

**1. What is still refused, and whether wrapping is the answer.** The five refusals the editor
can give for a full line are `layout::Room`, and they are exactly the split the question asks
for: `page edge` is what wrapping exists for, the three `neighbour` reasons are what the line
push met and could not move, `clip` is the document having cut the space off. Counts of 44,282
runs, `app` mode, the layout a reader types into:

| Trial | accepted | page edge | neighbour | clip | other |
|---|---:|---:|---:|---:|---:|
| +10% longer | 25,848 | 7,957 | 9,974 | 240 | 263 |
| +25% longer | 21,196 | 12,201 | 10,355 | 265 | 265 |
| +50% longer | 18,324 | 14,646 | 10,775 | 273 | 264 |

As a share of the refusals rather than of the runs, the page edge takes over as the edit grows:
43% of refusals at +10%, **53% at +25%**, 56% at +50%, against 54 / 45 / 41% for the neighbour
family. At +25% that is **12,201 runs, 28% of every editable run in the corpus**, whose only
obstacle is the edge of the page. The `other` column is 260 afp2pdf runs whose font cannot
write the character the trial appends and five with several glyphs for one character; nothing
in it is about length.

| Producer | Runs | accepted | page edge | neighbour | clip | other | tagged pages |
|---|---:|---:|---:|---:|---:|---:|---:|
| LuaTeX / ConTeXt | 10283 | 7303 | 2498 | 482 | 0 | 0 | 0 of 125 |
| Acrobat 25 (Arcadia agenda) | 7461 | 4471 | 1858 | 1118 | 14 | 0 | 101 of 119 |
| ReportLab | 7335 | 2999 | 2049 | 2287 | 0 | 0 | 0 of 88 |
| XeLaTeX / xdvipdfmx (fontspec) | 6190 | 2957 | 1036 | 2190 | 0 | 7 | 0 of 71 |
| IBM afp2pdf | 3104 | 323 | 2523 | 0 | 0 | 258 | 0 of 128 |
| arXiv GenPDF (arXiv 2509.18965) | 3019 | 1356 | 724 | 939 | 0 | 0 | 0 of 24 |
| pdfTeX (arXiv 2003.00976) | 3018 | 651 | 381 | 1986 | 0 | 0 | 0 of 11 |
| Word via PDFMaker 20 (Coatesville) | 1681 | 278 | 859 | 544 | 0 | 0 | 21 of 21 |
| Word via PDFMaker 26 (Hugo) | 626 | 194 | 160 | 272 | 0 | 0 | 7 of 7 |
| Word 2016 (Mercer Island) | 487 | 72 | 0 | 207 | 208 | 0 | 4 of 4 |
| Word via PDFMaker 22 (Illinois) | 241 | 118 | 36 | 86 | 1 | 0 | 2 of 2 |
| Typst | 228 | 98 | 12 | 118 | 0 | 0 | 0 of 1 |
| pdfTeX (arXiv 1706.03762) | 189 | 125 | 26 | 38 | 0 | 0 | 0 of 2 |
| Wellington agenda | 157 | 106 | 24 | 27 | 0 | 0 | 0 of 2 |
| Google Docs (SampleForms invoice) | 97 | 48 | 3 | 42 | 4 | 0 | 0 of 2 |
| LibreOffice (W3C headers) | 68 | 24 | 0 | 6 | 38 | 0 | 4 of 4 |
| PowerPoint via PDFMaker (Healdsburg) | 60 | 44 | 5 | 11 | 0 | 0 | 14 of 14 |
| HM Passport Office guidance | 37 | 28 | 7 | 2 | 0 | 0 | 0 of 1 |
| W3C dummy | 1 | 1 | 0 | 0 | 0 | 0 | 0 of 1 |
| **All runs** | 44282 | 21196 | 12201 | 10355 | 265 | 265 | 153 of 629 |

**2. Where the model would come from, and for how much of that population.** 153 of the 629
editable pages carry a structure tree that reaches them, and 10,624 of the 44,282 runs are on
one: **24% either way**. The refused population is split in the same proportion — 2,918 of the
12,201 page-edge refusals are on a tagged page, which is 23.9% against a base of 24.0% — so
tagging neither concentrates nor avoids the edits wrapping would serve. The numerator alone
would have read as a finding; it is the denominator that says there is none.

Two facts about the tagged quarter that are better than expected, and one that is worse:

- **Every tagged page's parent tree was readable, on all 153.** No MCID map failed, and the
  probe's own decode addressed the same operators the scanner does on every one of them.
- **Every run the editor offers on a tagged page is tagged** — zero runs with no owning
  element. That is not luck: `Tags::read_only` withholds text outside a marked-content
  sequence on a page that has a tree at all, so on a tagged page "tagged page" and "tagged run"
  are one population. It also means a fixture cannot mix the two halves on one page; the trap
  index has that entry.
- **`textedit::tagging` cannot answer the question anyway.** It keeps one tag *name* per MCID
  and drops the element that owns it, so it can say *this text is in a paragraph* and cannot
  say *these two runs are in the same paragraph*. The owner is read and discarded three lines
  from where it is checked (`names[mcid] = tag.to_vec()`, beside `reference(&entries[mcid])? !=
  id`), so carrying it is a field and not a walk — but nothing today carries it, and the
  probe had to read the parent tree itself.

**3. What geometry has to work with on the other three quarters.** Over every pair of
consecutive lines, the signals a block rule would read, tagged pages against untagged:

| Signal | tagged pairs (4,532) | untagged pairs (17,053) |
|---|---:|---:|
| same font resource | 85% | 65% |
| same size | 99% | 90% |
| nothing painted between the two lines | 99% | 92% |
| pitch at most 1.5 em | 78% | 74% |
| left edges within 0.1 pt | 76% | 61% |
| left edges more than 36 pt apart | 12% | 17% |

The left-edge distribution is **bimodal, which is the useful part**: either within a tenth of a
point or more than twelve points away, with 122 tagged pairs and 552 untagged pairs in between.
So the tolerance is not a knife edge — moving it from 1 pt to 3 pt moves about half a percent of
pairs either way — and a rule can use it without its answer depending on the constant. The line
pitch is not so kind: 3,102 untagged pairs sit at or below 1.0 em, which is not a line pitch at
all but stacked fragments, superscripts and table rows, and they are indistinguishable by pitch
from tight leading.

**The candidate rule, and what it is wrong about.** Two consecutive lines are one block when the
pitch is between zero and three ems, the font resource and size agree, the left edges agree to 1
pt (or the earlier line is indented by up to four ems and has not already been joined from
above), and the render shows nothing painted between them; chains are then cut wherever the
pitch steps by more than half a point from the chain's first. Measured against the tagged pages,
**where the answer is known**, over the 4,508 pairs whose page is tagged throughout:

| Producer | Pairs | joined, one block | joined, two blocks | split, one block | split, two blocks | false joins |
|---|---:|---:|---:|---:|---:|---:|
| Acrobat 25 (Arcadia agenda) | 3016 | 1609 | 425 | 417 | 565 | 21% |
| Word via PDFMaker 20 (Coatesville) | 897 | 781 | 64 | 7 | 45 | 8% |
| Word via PDFMaker 26 (Hugo) | 234 | 128 | 37 | 16 | 53 | 22% |
| Word 2016 (Mercer Island) | 157 | 34 | 25 | 33 | 65 | 42% |
| Word via PDFMaker 22 (Illinois) | 114 | 10 | 3 | 35 | 66 | 23% |
| LibreOffice (W3C headers) | 58 | 36 | 9 | 0 | 13 | 20% |
| PowerPoint via PDFMaker (Healdsburg) | 32 | 4 | 14 | 0 | 14 | 78% |
| **All tagged pairs** | 4508 | 2602 | 577 | 508 | 821 | 18% |

**About one pair in six that the rule joins is not one block**, and it misses 16% of the pairs
that are. The false-positive *shape* is the finding, and it is one shape: **468 of the 577 false
joins, 81%, are one paragraph followed by another paragraph** — 238 where both are a single line
and 230 where they are not — with 64 more involving a list body. Only 23 involve two different
roles. So the rule is good at telling a heading from body text, which differ in font or size,
and blind to the boundary between two paragraphs set solid in one style, which differ in nothing
it can see. The leading check does not save it: those are the pairs where the producer left no
extra space, and a break with extra space is already caught.

The per-producer spread matters as much as the total. Word through PDFMaker 20 is 8% wrong and
PowerPoint slides are 78% wrong, because a slide is a stack of separate text boxes at one pitch
in one style — which is exactly the false-join shape, as the whole content of the page. And
**the total is one document**: 3,016 of the 4,508 tagged pairs are the Arcadia agenda, so the
18% is that agenda plus a little. A second heavily tagged producer would move it.

**4. What is below, and whether one more line fits.** From the render, a block's last line has
clear space below it, and one more line needs the block's own pitch plus the gap an existing
following line leaves below a hit rectangle — which is measured from the block's own interior
lines where it has them and from the page's otherwise, so the criterion reproduces a line that
is already there rather than assuming what one would need.

| Blocks | count | one more line fits | what is directly below |
|---|---:|---:|---|
| From the tags, on tagged pages | 1,481 | 471 (32%) | another line of text 88%, unaccounted ink 10%, page edge 2% |
| From the tags, blocks whose own text is refused for the page edge | 755 | 287 (38%) | |
| From the rule, on untagged pages | 13,452 | 2,494 (19%) | another line of text 81%, unaccounted ink 15%, page edge 4% |
| From the rule, blocks whose own text is refused for the page edge | 5,768 | 1,039 (18%) | |

**Two thirds of the blocks that need wrapping have no room to put the extra line.** Below a
paragraph is another paragraph, nearly nine times in ten. The tagged number is the one to trust:
the untagged one depends on the block model, and the same corpus reads 14 / 19 / 28% under
`--rule none` / `signals` / `all`, which is the circularity to name rather than to average away.

Two honest limits on this table. `ink_below` starts at the hit rectangle, which is a full em box
rather than the ink in it, so the clearance is a lower bound and the "fits" counts are
conservative. And two pages of the 629 are more than half non-background, where "the page's own
background" is a weaker idea than elsewhere; they are reported rather than excluded.

**Verdict.** Length is what refuses an edit, the page edge is now the majority of it, and
wrapping is the right next feature. What the evidence changes is its **scope**:

- **Tagged pages first, and on their own.** They are 24% of the corpus and 2,918 of the
  page-edge refusals, the parent tree was readable on every one of them, every offered run
  carries an element, and the answer is stated rather than inferred. The work is a field on
  `Tags` and a reader for it, not a heuristic.
- **Untagged pages out of the first increment.** A geometric rule is wrong about one join in
  six, its errors are concentrated in the one case wrapping would damage — the next paragraph
  gets pushed down as though it were this one — and the error rate is 8% for one producer and
  78% for another, so there is no threshold at which it is uniformly safe. It is also the
  three quarters of the corpus, so this is a real cost and not a cheap deferral.
- **Wrapping into the space that is there, before wrapping that moves the page.** Only about a
  third of the blocks that need it can take another line where they sit. The rest need what is
  below to move, which is a second capability an order of magnitude larger, and the first
  increment should refuse those rather than grow into them.

### Wrapping onto a new line of the paragraph — measured 2026-09-23

The first wrapping increment, scoped by the section above: tagged pages only, into room that is
already below the paragraph. `textedit/layout/wrap.rs` plans it, `layout::prepare` lays it out
and checks where the moved lines land (`left_behind`, `wrap_room`), `write` applies it.
`docs/PLAN.md` §7, *Wrapping into the room below*, has the rule and the decisions in it.

macOS arm64, the same 31 files as the sections above (`testdata/textedit-public-corpus.json`,
803 pages, 629 editable, 44,282 runs), the growth instrument run twice, once with a release
probe built from `1907d70` in a separate worktree and once with this tree:

```sh
python3 scripts/textedit_growth.py <text-edit-probe> scratch/reflow-corpus/*.pdf \
  --manifest testdata/textedit-public-corpus.json --jobs 6 \
  --output <new report.json> --records <new directory>
python3 scripts/textedit_growth.py <probe> --compare <before records> <after records>
```

`--compare`: **594,391 verdicts unchanged in kind, 2,671 refused before and accepted now, 0
accepted before and refused now.** 9,309 worker-agreement checks, 0 disagreements, 1,073.5 s on
six processes (1,021.7 s for the baseline). The growth driver has no category for the new
refusals; they were counted from the records, with the tagged pages taken from the block
records of the section above.

Edits as typed, in the app's own box, on the 10,624 runs of tagged pages:

| trial | page-edge refusals before | now wrap | now name the paragraph | still the page edge |
|---|---:|---:|---:|---:|
| +10% longer | 1,144 | 271 | 103 | 770 |
| +25% longer | 2,918 | **1,069** | 940 | 909 |
| +50% longer | 3,671 | 1,302 | 1,193 | 1,176 |

"Name the paragraph" is 916 *"its lines would move onto what is below it"* and 24 for something
below that cannot move or a drawing over it, at +25%. **904 of the 909** still refused at the
page edge have more of their own paragraph after them on the line, which is reflow of the line's
remainder and not this increment. Across the whole sample, +25% goes from 21,196 accepted to
22,265 (47.9% to 50.3%); untagged pages do not move, by construction.

All of it is in four files: the Acrobat 25 agenda (29 / 440 / 579 at +10 / +25 / +50%),
Coatesville via PDFMaker 20 (195 / 549 / 624), Hugo via PDFMaker 26 (47 / 79 / 97) and Illinois
via PDFMaker 22 (0 / 1 / 2). Mercer Island (Word 2016) and the LibreOffice export have no
page-edge refusals to begin with, and the PowerPoint slides five.

⚠ **Two changes in kind that are not a longer edit.** In Coatesville, 27 same-length edits (two
letters swapped) and 2 unchanged runs were refused at the page edge and now wrap. Their own text,
laid out again from glyph widths, is wider than the room the line has; the wrap is the first
writer that fits it, so the reader gets two lines for an edit that added nothing. It is 29
verdicts of 597,062 and better than a refusal, but it is not what a same-length edit looks
like, and it is where to look if a reader reports a line that wrapped for no reason.

**Round trips, three per file** (one in Illinois, which has one), each a +25% edit refused at the
page edge before and accepted now, through the probe, `qpdf --check`, and an independent reading:

```sh
<probe> --growth-request <file> <page> <operator> grow25 app grow > <req.json>
<probe> --roundtrip <file> <req.json> <new directory>
uv run --with pypdf --with pdfplumber scripts/text_wrap_check.py --compare <file> <dir>/edited.pdf <page>
```

| Producer | page | glyphs moved | distance | probe | `qpdf` | `--compare` |
|---|---:|---:|---:|---|---|---|
| Acrobat 25 (Arcadia agenda) | 2 | 0 (its last line) | — | pass | clean | pass |
| Acrobat 25 | 30 | 80 | 15.0 pt | pass | clean | pass |
| Acrobat 25 | 70 | 8 | 13.4 pt | pass | clean | pass |
| Word via PDFMaker 20 (Coatesville) | 1 | 114 | 13.2 pt | pass | clean | pass |
| Word via PDFMaker 20 | 8 | 362 | 13.2 pt | pass | clean | pass |
| Word via PDFMaker 20 | 15 | 115 | 13.2 pt | pass | clean | pass |
| Word via PDFMaker 26 (Hugo) | 1 | 136 | 13.8 pt | pass | clean | pass |
| Word via PDFMaker 26 | 4 | 82 | 13.8 pt | pass | clean | pass |
| Word via PDFMaker 26 | 5 | 175 | 13.8 pt | pass | clean | pass |
| Word via PDFMaker 22 (Illinois) | 2 | 13 | 8.6 pt | pass | clean | pass |

`--compare` reads both files with pdfplumber and pairs every glyph on the page: each is where it
was, or straight down by one distance every moved glyph shares, and what is left over from the
source lies on the edited line alone. It also fails when the saved page has more pairs of
overlapping glyphs than the source, and overlapping pairs were 0 -> 0 on all ten. The glyph and
distance columns are its report.

⚠ **The probe's own pass cannot see a wrap that moved nothing,** and the table above is not
redundant with it. With the moved lines deleted from the writer, `--roundtrip` passed the
synthetic paragraph — the preview and the save come from one writer and agree, and the new line
printed over the unmoved one is inside the envelope, because the envelope is where that line
was meant to leave from. `text_wrap_check.py` failed it both ways: `--check` read `'THIRD
LINE' at (20.0, 172.0), expected (20.0, 158.0)`, and `--compare` counted 10 overlapping pairs
against 0. The trap index has the entry, and the two ways `--compare` itself was wrong first:

- It paired pypdf's **text chunks**, which a reader cuts at every `Tm`, and every moved show has
  one of its own — the same glyphs came back in different pieces. It pairs glyphs now.
- It paired glyphs to a hundredth of a point, and pdfminer places a cursor-continued show 0.024
  pt from where poppler does **in the untouched source**, while both agree on the saved file
  where the show has a `Tm`. `pdftotext -bbox` put the Hugo word at 302.94 before and after.
  The tolerance is 0.05 pt.

The synthetic case, generated and read by pypdf rather than by the editor's own lopdf:

```sh
uv run --with pypdf scripts/text_wrap_check.py --generate <new directory>
<probe> --growth-request <dir>/source.pdf 0 8 grow25 app grow > <dir>/requests.json
<probe> --roundtrip <dir>/source.pdf <dir>/requests.json <new result directory>
uv run --with pypdf scripts/text_wrap_check.py --check <dir>/source.pdf <result>/edited.pdf
```

`--check` asserts the two lines below the edit moved exactly one 14 pt pitch at the same x, the
continuation starts at the paragraph's left edge, and the next paragraph — placed by a `Td`
chained through every moved line — is where it was to a thousandth of a point. `app` is new in
`--growth-request`: the width of the box the editor opens, measured by the probe.

**Unit tests.** 25 in `textedit/wrap_tests.rs`, on a synthetic tagged page: the move itself with
the next paragraph's start compared bit for bit; no room below, and exactly one pitch of room; an
untagged page and text after the run on its line keeping the old refusal; the last line wrapping
into space and moving nothing; a continuation under a first-line indent; a rule under a moved
line refusing and a page background not; a highlight refusing and a popup not; a batch conflict
in either order; a single-line paragraph at the default pitch; read-only text of the paragraph
below; a link in the paragraph's last line; pushes and wraps meeting in a show, from either side
of the stream and through a show that only rides the cursor; a quarter-turned page at each of
90, 180 and 270; the outlines the editor draws; three ems as the largest pitch; a moved line
opening with a `TJ` displacement; a show of another block continuing a moved line from the
cursor; the foot of the page; another block's text inside a moved line; another size and a run
already pushed; a clip over the paragraph; the preview's extent; an unreadable annotation list.

**Mutations: 34 in `mutate_rust.py` under `wrap`, and 3 in `mutate_frontend.py` under `text
outlines`, all caught by the test each names.** The first run had three survivors, and all three
were findings about the code rather than the tests:

- `wrap: the preview crop leaves out where the lines were` could not be caught because the
  property always holds: the extent is one rectangle from the box to the lowest moved line, and
  every place a line left lies between them. The code that added the old places was deleted.
- `wrap tags: a link is a block of its own` exercised a branch that never runs for a link inside
  a paragraph: the paragraph's walk takes the link in as one of its own leaves and gives it the
  paragraph's block already. The branch was deleted; the test stays, because it is what proves
  the leaf path.
- `wrap write: over an earlier push` was shadowed by the check at the end of `write`, which sees
  every show a push *displaced*. It is the only check that sees a show that only rides a pushed
  line's cursor, and that got a test of its own.

**What the window phases meet.** Eight text-edit phase fixtures are tagged. The seven Edge
exports among them were checked through the writer with the phase's 1,000-character overflow
draft, and each is refused *"the document clips the space after it"*: Edge clips its page, a clip
ends those lines rather than the page edge, and a clip is not a trigger. The eighth, the
LibreOffice export `textedit-wrapped` runs on, is not on this machine and was **not** checked;
run that phase before the next release. A paragraph whose refusal does now come from a wrap still begins *"There is
no room for more text on this line"*, which is what those phases assert.

**Found while building it, and not fixed here:** the line push takes runs of the next line for
its own. Hit rectangles are em boxes; at 12 pt on a 14 pt pitch adjacent lines overlap by a
point, and the push's "same line" test allows a tenth. Growing a run on one line pushed the line
above it 18 pt to the right in a synthetic fixture (a run earlier in the stream, so the line
above was the one after it). It is ranked first in `docs/PLAN.md` §7.

### Which runs share a line — measured 2026-09-23

`layout::Axis::beside` decides it for the growing box (`room`), for what the push may move
(`free_width`) and for what may stop a pushed run (`reach`). It counted anything overlapping
the line by more than 0.1 pt. Hit rectangles are em boxes — a quarter em below the baseline,
a whole em above — so 12 pt text on a 14 pt pitch has adjacent boxes overlapping by 1 pt, and
the push moved runs of the next line with its own. Now a run shares the line when it shares
more than half of the shorter of the two heights: a superscript, a larger word and a run's own
descender reach all do, the next line's sliver does not. Whether new ink meets the next line's
glyphs is the collision check's question, unchanged.

Growth instrument, release probes built from `09c8c03` in a worktree and from this tree,
31 files, 44,282 runs, `app` mode, as typed:

| trial | before | after |
|---|---:|---:|
| unchanged | 43,828 (99.0%) | 43,833 (99.0%) |
| same length | 37,177 (97.6%) | 37,267 (97.8%) |
| +10% | 26,119 (59.0%) | **28,153 (63.6%)** |
| +25% | 22,265 (50.3%) | **24,216 (54.7%)** |
| +50% | 19,626 (44.3%) | **21,466 (48.5%)** |

`--compare`: 589,898 unchanged in kind, **6,542 refused before and accepted now, 622 accepted
before and refused now.** 9,309 worker-agreement checks, 0 disagreements. The 622 are in eleven
files, most in arXiv 2509.18965 (169), the ReportLab guide (157) and Typst (104).

**What the 622 were.** 24 were sampled at random and saved with the old probe; 5 of them are
beyond the round trip's 128 pages. Of the 19 that saved, every one shifted glyphs sideways on
a line other than the edited one — 16 on two to six lines, and 3 on the line 8.4 pt below
the edit in the pdfTeX paper, whose appended characters landed on the edited line and whose
next line moved whole. They were accepted because the defect made the room.

**What the 6,542 are.** 30 were sampled (outside the three files over 128 pages) and saved
with the new probe: every one shifted glyphs sideways on one line only, counting tops within
2 pt as one line (a pdfTeX line with sub- and superscripts spans 3.4 pt), and none added a
pair of overlapping glyphs, counted through pdfplumber as `text_wrap_check.py` counts them.

Tests: `push_tests::the_next_line_is_not_pushed_along_with_this_one` was red before the fix
(the run below moved from 100 to 119.2), and `text_on_the_next_line_does_not_stop_a_push`
covers `reach`. Two `room` unit tests encoded the old tenth-of-a-point rule and were rewritten
to the new boundary: 7.5 pt of a 15 pt box is off the line, 7.6 pt on it. Six mutations under
`same line:`, one per call site restoring the old rule plus the boundary itself, all caught;
the box's own call site is caught by the `room` unit tests rather than the push test, because
a box stopped at the next line is rescued by the push path around it.

### The rest of the line flows after the edit — measured 2026-09-24

The second wrapping increment, ranked second in `docs/PLAN.md` §7. Until now a wrap was not
offered when the paragraph had more text after the edit on the same line. Now that text flows
after the edit, a run at a time: each run keeps its bytes and its gap to what came before it,
stays on the edit's last line while it fits the paragraph's measure, and otherwise starts the
next line at the paragraph's left edge. `wrap::flow` places the runs, `layout::prepare` moves
them with `wrap::lowered`, and `wrap_room` checks where each one lands, now with an offset of
its own per run. The gap is measured from the farther of the replacement's advance and its ink,
as the push measures a line.

Refused, keeping the refusal the edit already had: another block's text among what the push
along the line would move; a run the writer cannot move (no layout context, inside an
ActualText span, under a compound clip); and a run wider than a whole continuation line, which
only a hanging indent produces.

Growth instrument, the release probe built from `HEAD` (`1ae9089` code) and one built from this
tree, 31 files, 44,282 runs, `app` mode, as typed. The baseline reproduces the figures of
*Which runs share a line* exactly:

| trial | before | after |
|---|---:|---:|
| unchanged | 43,833 (99.0%) | 43,838 (99.0%) |
| same length | 37,267 (97.8%) | 37,422 (98.2%) |
| +10% | 28,153 (63.6%) | **29,313 (66.2%)** |
| +25% | 24,216 (54.7%) | **25,469 (57.5%)** |
| +50% | 21,466 (48.5%) | **22,932 (51.8%)** |

`--compare`: **593,023 verdicts unchanged in kind, 4,039 refused before and accepted now, 0
accepted before and refused now.** 9,309 worker-agreement checks, 0 disagreements.

At +25%, of the 16,270 edits refused at the page edge before (every page, tagged or not), 1,253
are accepted, 569 now say *"its lines would move onto what is below it"*, 51 name a drawing or
an annotation over the lines that would move, and 14,397 are still at the page edge. The
accepted ones are in five files: Coatesville 562, the Arcadia agenda 446, Hugo 237, Illinois 4
and the Healdsburg slides 4.

Three things in those numbers are not what the feature's name suggests:

- **About half the flows move a run the push along the line never saw.** Counted with a
  temporary print over the five files: of 2,083 distinct cases where text flowed, 1,022 had
  nothing in the push's line, 235 of them because the run after the edit starts inside the box
  the editor opens (the rounding `Free::from` describes). These edits were refused before
  because the replacement was laid out over that run; now the run moves. 345 of the 2,083 add
  no line at all, and 5 of the 11 round trips below are such edits. Untagged pages still refuse
  them, which is why `docs/PLAN.md` ranks fixing the push above splitting runs. **Fixed the same
  day; the cause was not the suspect named in `docs/PLAN.md`** -- see *The push finds a run set
  flush against the edit*.
- **155 same-length edits and 5 unchanged ones are accepted now**, 139 and 4 of them in
  Coatesville. Laid out again from glyph widths their own text is wider than the line has, so
  they wrap: the reader gets a changed line break for an edit that added nothing. It is the
  same case *Wrapping onto a new line of the paragraph* found 29 of, better than a refusal and
  still not what a same-length edit looks like.
- **A run is not split.** Across the four files it applies to, 1,164 of 1,268 distinct cases
  had a run after the edit holding several words, so the usual result is a line that ends right
  after the edit and a new line holding the rest. A flowed run that starts a line keeps a
  leading space if it had one; that is 28 of 2,415 placements (1%).

**Round trips, three per file** (one in Illinois and in Healdsburg), each a +25% edit refused
before and accepted now, chosen at random with seed 7 from the flipped verdicts:

```sh
<probe> --growth-request <file> <page> <operator> grow25 app grow > <req.json>
<probe> --roundtrip <file> <req.json> <new directory>
qpdf --check <dir>/edited.pdf
uv run --with pypdf --with pdfplumber scripts/text_wrap_check.py --compare <file> <dir>/edited.pdf <page> <req.json>
```

| Producer | page | lines added | moved down | probe | `qpdf` | `--compare` |
|---|---:|---:|---:|---|---|---|
| Acrobat 25 (Arcadia agenda) | 53 | 1 | 222 glyphs, 16.3 pt | pass | clean | pass |
| Acrobat 25 | 1 | 1 | 366, 15.0 pt | pass | clean | pass |
| Acrobat 25 | 59 | 0 | — | pass | clean | pass |
| Word via PDFMaker 20 (Coatesville) | 16 | 1 | 925, 13.2 pt | pass | clean | pass |
| Word via PDFMaker 20 | 2 | 1 | 17, 13.2 pt | pass | clean | pass |
| Word via PDFMaker 20 | 0 | 0 | — | pass | clean | pass |
| PowerPoint via PDFMaker 24 (Healdsburg) | 23 | 0 | — | pass | clean | pass |
| Word via PDFMaker 26 (Hugo) | 6 | 1 | 79, 13.8 pt | pass | clean | pass |
| Word via PDFMaker 26 | 4 | 0 | — | pass | clean | pass |
| Word via PDFMaker 26 | 1 | 1 | 522, 13.8 pt | pass | clean | pass |
| Word via PDFMaker 22 (Illinois) | 0 | 0 | — | pass | clean | pass |

Overlapping glyph pairs were 0 -> 0 on all eleven.

**`--compare` counts now, when it is given the request.** Pairing glyphs cannot tell a flowed
run from the replacement: both are left over on the edited line in the source and turn up as
new glyphs on the saved page. `--growth-request` now writes the run's `original` beside the
replacement (`--roundtrip` ignores the key), and `--compare` asserts that the saved page gained
exactly as many visible glyphs over the source's leftovers as the replacement adds to the
original, so a flowed glyph lost or drawn twice fails. Control: the Hugo page 1 file with one
character added to the request's `original` fails with *"gained 9 glyphs ... the replacement
adds 8"*.

⚠ **The first run of these round trips failed two of eleven, and the probe was right.** The
preview's extent is one rectangle, the box together with where each moved run lands, and a run
leaving the right end of the edit's line for the start of the next one leaves from outside
both. `--roundtrip` requires every changed pixel inside the extent the editor reports, and
said *"pixels changed outside edited text envelopes"*. The extent now includes each flowed
run's source rectangle. The trap index has the entry.

Tests: `wrap_tests` gained ten, among them `the_text_after_the_edit_on_its_line_flows_after_it`
(positions to 0.0001 pt, the moved show's own bytes, and the next paragraph's line start to the
bit), the new-line and hanging-indent cases, the in-box neighbour, an ActualText run that is
refused, another block's text, a batch conflict, the outlines, the preview extent and an
overhanging last glyph. The old `text_after_the_run_on_its_line_keeps_the_page_edge_refusal`
pinned the refusal this removes and was replaced. The mutations are under `wrap` and `flow:`
in `scripts/mutate_rust.py`, one per decision: `mutate_rust.py --only wrap --only flow: --only
'grown box: measure'` ran 66, all caught by the test named for each, on the final tree.

### The push finds a run set flush against the edit — measured 2026-09-24

Ranked above splitting runs in `docs/PLAN.md` §7: in about half the lines whose rest flowed
after a wrap, the push along the line had moved nothing. `docs/PLAN.md` suspected
`free_width`'s `near + 0.1 < own_far`, a tenth of a text-space unit. **It is not that.** A
temporary print over the five tagged files recorded, for every flow with an empty push, which
condition dropped the run after the edit: all 804 distinct cases (448 edited runs) left through
one early return, `if free >= hard { return nothing(stop) }`, and in every one the run after the
edit started at the box's edge or up to 0.1 units inside it.

The mechanism: `room` skips a run starting inside the box, because the box may never shrink,
and the box is the run's advance rounded up, so a run set flush against the edited one is always
skipped. With nothing movable after that run, the box's limit with every run counted (`free`)
and with only the fixed ones (`hard`) are the same page edge, and the early return read
agreement as "nothing movable is in the way". `Free::from` already handled the flush run when a
third run made the walk reachable (`the_push_is_measured_from_the_run_it_moves_not_from_the_room`);
without one it was never reached. The early return is gone, and the walk decides.

Removing it exposed a second defect in the same place. The ceiling was `(from + shift).max(free)`,
a floor meant to keep a replacement that fitted the old room accepted. With the flush run
skipped, `free` runs past it to the next obstacle and the floor replaces `reach`'s limit: the
push moved runs further than `reach` allowed. It is `(from + shift).max(free.min(from))` now.
The trap index has the general form.

Tightening that exposed a third, which the loose floor had been hiding: `reach` counted any
drawing overlapping a pushed run as leaving it no room, so a page border or background around
the whole text block (Word's, on the Illinois résumés and the Healdsburg slides) refused every
push on the page. A drawing that starts behind the run now holds it, and the run may move as far
as the drawing's far edge; one that starts partway along the run still stops it. The wrap
already asked the same of the lines it moves.

Growth instrument, the release probe built from `HEAD` (`5b85103`) and one from this tree, 31
files, 44,282 runs, `app` mode, as typed:

| trial | before | after |
|---|---:|---:|
| unchanged | 43,838 | 43,838 |
| same length | 37,422 | 37,434 |
| +10% | 29,313 (66.2%) | **30,327 (68.5%)** |
| +25% | 25,469 (57.5%) | **26,444 (59.7%)** |
| +50% | 22,932 (51.8%) | **23,836 (53.8%)** |

`--compare`: **594,041 verdicts unchanged in kind, 2,963 refused before and accepted now, 58
accepted before and refused now.** 9,309 worker-agreement checks, 0 disagreements. At +25%, 995
edits flipped to accepted: 701 on untagged pages (LuaTeX manual 239, fontspec 104, arXiv 101 and
98, ReportLab guide 87, the SampleForms invoice 42, the research paper 27, Wellington 3) and 294
in the five tagged files (Arcadia 207, Illinois 59, Hugo 14, Healdsburg 11, Coatesville 3).

**The 58 were rendered from the old probe's output, one per refusal message**, since each is an
edit the old code accepted:

| refusal now | cases | what the old code wrote |
|---|---:|---|
| other text follows it | 21 | Typst: `𝜎` pushed 2.6 pt onto its own superscript `2`, which stayed |
| the text after it cannot be moved | 15 | arXiv: a line of the right-hand column pushed 4.5 pt along with the left one |
| this paragraph cannot wrap | 7 | Arcadia: `ONE MILLION, FIFTY-SEVEN` pushed to x 620.1 on a 612 pt page |
| a picture or a drawing follows it | 12 | Arcadia p.53: a run pushed 18 pt into a picture starting 0.6 pt after it |
| it reaches the edge of the page | 3 | ReportLab: a line pushed to x 598.8 on a 595.3 pt page |

One of the twelve drawing cases is a same-length edit on an underlined line (Arcadia p.93): the
underline ends 0.002 pt past the text, so the text has no room, and the old code moved it a
fraction of a point off its underline. That refusal is a real, small loss; it is the push's
existing rule for any drawing after a run. `--roundtrip` passed every one of the old outputs,
because the damage is inside the edit's own envelope.

**Round trips, two per file with flips** (fourteen), each a +25% edit refused before and accepted
now, chosen with seed 7 from the flipped verdicts; the LuaTeX and ReportLab pages were first
extracted with `qpdf --empty --pages <file> <n> --`, since `--roundtrip` refuses their page count.
All fourteen pass the probe (preview and save agree, nothing outside the envelope changes) and
`qpdf --check`. `text_wrap_check.py --compare` passes twelve; the two arXiv picks (page 8,
operators 1711 and 1975) are edits inside a displayed formula, where the glyphs sit on five
baselines and the checker's one-line model reports *"source glyphs neither stayed nor moved by
the shared None pt, on 6 lines"*. Rendered, `{A, B, C}` became `{A,, B, C}` with the rest of the
formula moved along intact.

⚠ Found while reading those, and older than this change: poppler reports *"Syntax Error: Invalid
XRef entry 0"* on every arXiv 2003 output, including one written by the `HEAD` probe, and on no
other file. `qpdf --check` is clean on all of them. Not investigated.

Tests: `push_tests` gained `a_flush_neighbour_with_nothing_after_it_is_still_pushed` (the move,
and the page limit past the skipped run) and
`a_drawing_holding_the_run_that_moves_allows_it_to_its_far_edge` (a frame, and a drawing
starting inside the run); `wrap_tests` gained
`text_kerned_into_the_edit_flows_although_the_push_leaves_it`, because the mutation `wrap: flow
text only the push saw` survived once the push found the flush run and needed a run the push
still leaves out. Four new mutations in `scripts/mutate_rust.py`; `--only push: --only wrap
--only flow --only 'grown box' --only 'boxed edit' --only layout:` ran 112, one survivor
re-aimed and re-run, then all caught.

### A run after the edit is cut at a space — measured 2026-09-24

Ranked next in `docs/PLAN.md` §7. Until now the text after a wrapped edit moved a run at a time,
so a run too wide for what was left of a line moved down whole and the line ended right after
the edit. Now such a run is cut at a space: the words that fit stay on the line, the rest start
the next line at the paragraph's left edge, and the space at the break is written nowhere, which
also ends the 1% of flowed runs that started a line with a space.

`kerning::words` cuts a show's items at its spaces -- a space glyph, or a word gap in a font that
writes none -- keeping each word's own glyph bytes and the kerns inside it, and the source's own
items between two words (`Word::before`) so a piece of several words is joined exactly as the
source drew it. `wrap::flow` places pieces rather than runs, and `wrap::drawn` writes one `Tm` and
one `TJ` per piece, followed by the single line-and-cursor restoration a moved show always ends
with. A piece's rectangle for the room check is the run's cut to the piece's own stretch along
the line, and the editor outlines a cut run as one rectangle holding every piece. Only a run
that is one plain `Tj` or `TJ` is cut; a grouped run, whose members each carry a position, and
one that does not cut cleanly (two spaces in a row, a word opening with a displacement) moves
whole as before.

**The corpus found a false refusal the cut made more common, in `wrap_room`.** Words that stay
on the edit's own line slide along it, and the room check compares a moved box with the lines
around it, allowing the overlap one line pitch leaves. The pitch is the paragraph's to its *next*
line, and lines are not evenly pitched: on Coatesville page 19 the line above is 13.2 pt away
against a pitch of 13.4 below, so the two lines' boxes overlap by 1.3 pt in the source against
an allowance of 1.2, and every slide along that line was refused as "would move onto what is
below it". A moved box may now keep the overlap its source already had with a line, when that
overlap is a sliver -- less than half the shorter box, so never text on the same line. The half
that withholds it from text on the same line has no test that can reach it: `wrap::plan` refuses
a line shared with another block's text before the room check runs.

Growth instrument, the release probe built from `HEAD` and one from this tree, 31 files, 44,282
runs, `app` mode:

| trial | before | after |
|---|---:|---:|
| unchanged | 43,838 | 43,838 |
| same length | 37,434 | 37,449 |
| +10% | 30,327 (68.49%) | 30,358 (68.56%) |
| +25% | 26,444 (59.72%) | 26,473 (59.78%) |
| +50% | 23,836 (53.83%) | 23,866 (53.90%) |

`--compare`: **596,957 verdicts unchanged in kind, 105 refused before and accepted now, 0
accepted before and refused now.** 9,309 worker-agreement checks, 0 disagreements. The gains
are small by design: a cut changes where flowed text lands, not whether the edit fits. Every one
of the 105 was refused before as *"its lines would move onto what is below it"* (at +25%:
Coatesville 19, Arcadia 7, Hugo 3); which of the two changes -- a line fewer, or the overlap
allowance -- accepted each was not split out. A first run of this measurement, before the
allowance, found 8 verdicts the other way, all that `wrap_room` refusal; they are accepted now.

**Round trips**: the eleven flow edits of *The rest of the line flows after the edit* and two per
file with flips (six), with seed 7. Sixteen pass the probe and `qpdf --check`; the seventeenth,
Arcadia page 53 operator 131, is refused since *The push finds a run set flush against the edit*,
which found its old output pushed a run into a picture. `text_wrap_check.py --compare` passes
fourteen of the sixteen, overlapping pairs 0 -> 0 on all; the two Hugo page 5 edits fail it on a
superscript `th` after `140`, which sits on its own baseline and which the checker counts as a
second line -- in the output `140` and `th` both moved the same 7 pt.

Tests: `wrap_tests` gained six (the cut, the leading space, a kern inside a cut word, the space a
cut run ended with, a grouped run moving whole, both pieces outlined) and the uneven-pitch slide;
three existing ones now pin the cut instead of a whole run moving down. `fonts/type1/tests.rs`
tests `kerning::words` on word gaps, a kern beside a space glyph and two spaces in a row. The
mutation table gained twelve under `flow:`, `cut:` and `wrap room:`, and eight existing ones were
re-aimed at the new code; `--only flow --only cut: --only wrap --only push:` ran 107 with three
survivors, each answered by a new test and re-run, and the two `wrap room` mutations touched last
were run on the final tree.


### The blocks below a wrapped paragraph move down with it — measured 2026-09-24

Ranked next in `docs/PLAN.md` §7 (item 3). A wrap moved its own paragraph's lines down and
refused when they would land on anything else, which below a paragraph is another paragraph
most of the time: at +25% as typed, 1,342 edits were refused as *"its lines would move onto
what is below it"*. Now the block they would land on moves down by the same distance, and so
does whatever that block would land on in turn, until a gap below is deep enough for the added
lines; nothing after that gap moves.

`wrap::Plan::beneath` lists the candidates: every other structure-tree block whose runs are all
below the edited line, set in its direction, and movable by the writer (a layout context, not
inside an ActualText span, not under a clip drawn as a path). A block with one run above the
line -- a column beside the paragraph, a heading in the margin -- is not offered, so it stays and
refuses the wrap as before. `layout::cascade` decides which candidates move: a block moves when
a run already moving would land on it by `wrap_room`'s own rule (`lands`, factored out of it), or
when the ink of one of the edit's lines is over it by `lay_out`'s own rule (`strikes`, likewise
factored out, which exempts the edited run's own box). A moved block's runs join the moving set,
so the next block down is judged against where it went. At most 32 blocks move (`MAX_CASCADE`);
the corpus never came near it, and no test reaches the bound. Each moved show is written with
`wrap::lowered`, exactly as the paragraph's own lines are, and `wrap_room` checks the whole moving
set against the page, clips, text that stays, and drawings or annotations over the area swept.

`lay_out` runs once, with every candidate flagged as moving, and now also returns each line's
ink on the displayed page. The first version seeded the cascade with a plain 0.1 pt overlap
test instead, and the corpus caught it: on Arcadia pages 115 and 116 the edited line's ink
overlapped the next paragraph's box by 0.7 pt inside the run's own box, which `lay_out` allows,
so the cascade moved a paragraph that did not need to move, into an underline. Those two
verdicts went from accepted to refused; with `strikes` shared they are accepted again and pass
the round trip, and `a_block_the_edited_line_only_grazes_stays` pins it.

Growth instrument, the release probe built from `HEAD` and one from this tree, 31 files, 44,282
runs, `app` mode:

| trial | before | after |
|---|---:|---:|
| unchanged | 43,838 | 43,838 |
| same length | 37,449 | 37,456 |
| +10% | 30,358 (68.56%) | 30,608 (69.12%) |
| +25% | 26,473 (59.78%) | 26,953 (60.87%) |
| +50% | 23,866 (53.90%) | 24,459 (55.23%) |

`--compare`: **595,732 verdicts unchanged in kind, 1,330 refused before and accepted now, 0
accepted before and refused now.** 9,309 worker-agreement checks, 0 disagreements. Every one of
the 1,330 was refused before as *"its lines would move onto what is below it"*; at +25% they are
Arcadia 309, Coatesville 121, Hugo 31, Illinois 18, Healdsburg 1 -- the five tagged files. At
+25% the refusals under that message went from 1,342 to 515, and *"a drawing or an annotation is
placed over the lines that would move"* from 54 to 401: moving more of the page sweeps more of
it. Rendered, the pages with the most of those (Arcadia 76, Coatesville 10) underline their
headings and item titles, and moving the text would leave the underline behind. Two verdicts
refused before and still refused now give a new reason, *"edited page exceeds the text operator
limit"*: each moved show costs at least four operators, and a dense page has less room for many.

**What it did to the page: a moved block could use up a paragraph break.** The gap that took
the added lines was usually the space between two paragraphs further down, and a moved line
could come as close to what stays as a paragraph's own lines are to each other -- the rule the
wrap had for its own lines since *Wrapping onto a new line of the paragraph*. On Coatesville
page 14 the paragraph below the edit moved down a line and then sat directly on the one after
it. Nothing overlapped and the glyph check passed; the break was gone. The next section changes
that.

**Round trips**: two grow25 flips per file with seed 7, nine edits (Healdsburg has one).
All nine pass the probe (preview and save agree, adjacent pixels unchanged), `qpdf --check` and
`text_wrap_check.py --compare`, which requires every glyph to stay or move straight down by one
distance shared by all.

Tests: `wrap_tests` gained seven (the next paragraph moving under a moved line and under the
edit's own new line, a chain stopped by the first deep gap, a block leaving the page, a block
reaching above the line, an unmovable block below in three forms, a pending edit of a moved
block, and the grazed block); three existing ones used a movable paragraph as the thing below
and now use an untagged line, which nothing can move. The mutation table gained eleven under
`cascade:` and `beneath:`, and six existing ones were re-aimed at `landing`, `lands` and
`strikes`. Two survived their first run: one showed a missing test (a block mixing an editable
line with a read-only one), and the other was a check that could never fire (skipping the edited
block, which its own run already excludes), and was deleted.

### A wrap keeps the paragraph breaks below it — measured 2026-09-24

Decided after the section above: a wrap may no longer close a paragraph break. `lands` gained a
second test for a rectangle moving towards text ahead of it (below it, for a line moving down)
that shares its extent along the line: the clear space between them may shrink, but not below
one blank line of the paragraph, which is two pitches less a line's box (13 pt at 12 pt type
and a 14 pt pitch). A move is whole pitches, so a gap already narrower than a blank line is not
allowed to close at all. Text clipped away entirely has a hit rectangle of no height and is
skipped, which `a_clip_over_the_paragraph_stops_the_lines_it_would_move` caught on the first run.

The same test drives the cascade, so a block whose break would close moves down too, and the
first gap with a blank line to spare takes the added lines. When the edited line is the
paragraph's last, nothing of the paragraph moves, and the edit's new last line is its bottom:
`Edge` is the edited line's box, as wide as the edit's lines, moved down by the lines added, and
both `cascade` and `wrap_room` hold it to the same test. `landing` counts its height, which the
first run missed (with nothing moving, a blank line came out a whole pitch).

**It costs more than the cascade gained.** Same instrument, against the records of the two runs
before it:

| +25% as typed | before the cascade | cascade | cascade, breaks kept |
|---|---:|---:|---:|
| accepted | 26,473 (59.78%) | 26,953 (60.87%) | 25,360 (57.27%) |
| refused: what is below it | 1,342 | 515 | 1,975 |
| refused: a drawing or annotation | 54 | 401 | 533 |

| trial | before the cascade | breaks kept |
|---|---:|---:|
| same length | 37,449 | 37,405 |
| +10% | 30,358 | 29,852 |
| +50% | 23,866 | 22,496 |

`--compare` against the cascade: 4,366 verdicts accepted before and refused now, none the other
way; against the state before the cascade, 906 refused before and accepted now, and 3,942 the
other way. Those are edits the wrap accepted by closing a paragraph break, its own since
2026-09-23 or, since the cascade, one further down: Coatesville 2,714, Arcadia 1,164, Hugo 469,
Illinois 19. Coatesville's pages are full to the footer, so keeping every break runs the cascade
into the footer, which is untagged, and nothing on those pages wraps: the page 14 edit above is
refused now. What would win some of it back is spreading the added lines over several breaks,
each giving up part of its space, where today every moved block moves the whole distance.

**Round trips**: two per file of the edits refused before the cascade and accepted now, seed 7,
nine edits. All nine pass the probe, `qpdf --check` and `text_wrap_check.py --compare`. Rendered,
Arcadia page 101 gains a line in its fourth paragraph and every paragraph below it moves down
one line with its blank line kept, down to the page number.

Tests: the three fixtures that expected one blank line to be used up now expect a refusal, and
wrap with 42 pt; the cut-at-a-space test's next paragraph moves down two lines with it; the
cascade test gained a chain started by a closing break; a new test holds the edited last line
to the break below it, including a word beyond the old run but under the new line; the grazed
test gained a word beside where a moved line goes; and a word of another block beside the
paragraph's short last line, where the edit's new line reaches, moves down (it keeps the ink
seed of the cascade tested now that the edge covers the last-line case). Ten mutations under
`keep breaks:`, one re-aimed; all caught, as are the cascade and room mutations beside them.
`before.min(blank)` and a same-line skip were written first and deleted: neither could change a
verdict, for the reasons given above and because text on the same line is never ahead.

### Spreading the added lines, and a break is between blocks — measured 2026-09-24

Ranked next after *A wrap keeps the paragraph breaks below it*: every block the cascade moved went
the whole distance, so one break had to take all of it. Now each block moves only as far as it
needs to (`layout::cascade`, `need`): the distance the block above it moved, less what the break
between them has beyond a blank line. A break of a blank line or less passes the whole distance
on; the first block that need not move ends the cascade, and `wrap::lowered` writes each block
at its own distance. A need under a hundredth of a point (`NO_MOVE`) is no move: the corpus found
it on Arcadia page 101, where subtracting a break's spare from the distance it took exactly left
0.00003 pt, and moving a paragraph by that toward the page number under it refused 21 edits.

The measurement predicted little, and got it: Coatesville's breaks are exactly one blank line, so
they have nothing to spare. Alone, spreading moved 87 verdicts from refused to accepted and none
the other way.

**Its round trips found that the break rule was measured between the wrong things.** On Arcadia
page 66 a paragraph's short last line (*therefrom.*, 72 to about 120) sits above the next
paragraph's first line, indented to 184. The break rule compared lines that share extent along
the line, so it measured from the wide line above *therefrom.*, found 12 pt to spare, moved the
next paragraph 1.4 pt, and the break was gone -- and the version before spreading had the same
hole, hidden only because every block moved the whole distance. A break is between two blocks,
so the rule now compares blocks: each moved rectangle carries its block's extent along the line
(`Moving::reach`; the paragraph's is its moved lines, the edit's lines and its bottom, and a
carried block's is its own lines), and both `need` and `lands` ask whether those extents overlap.
A block level with a moving line or above it is not ahead of it and is not moved by it. The ink
widening of the paragraph's bottom (`Edge`) is gone with it: the paragraph's reach covers it, and
its mutation survived.

Against the records of *A wrap keeps the paragraph breaks below it*, 31 files, 44,282 runs:

| trial | before | after |
|---|---:|---:|
| same length | 37,405 | 37,408 |
| +10% | 29,852 | 29,860 |
| +25% | 25,360 (57.27%) | 25,360 (57.27%) |
| +50% | 22,496 | 22,495 |

`--compare`: 76 refused before and accepted now, **66 accepted before and refused now**, all on
Arcadia. 57 are *"its lines would move onto what is below it"*: six of them round-tripped with the
previous probe and five had closed a paragraph break on page 101 (a gap wider than 1.5 pitches in
pdfplumber's line tops: 8 before, 7 after) -- that page's render in the previous section showed
the lines moving and missed that one break closed. The sixth, and the other 9, are *"part of it
below cannot be moved"* on page 100, a notary form: the blocks below now count as below the
paragraph, and one shares its line with the form's read-only text.

**`text_wrap_check.py --compare` had a hole of its own**, older than today. It took whatever
source glyphs were left over on one line for the edited line, so a whole line of other text moved
sideways, moved up, or split between two distances passed, and the count check still added up.
The left-over glyphs must now sit on a line that holds the original's characters. It also
accepts several downward distances, a whole line at a time, each supported by at least three
glyphs (`SUPPORT`). `--self-test` runs six synthetic controls, both ways; without the new
assertion three of them fail. It does not count paragraph breaks; the break counts above were
taken by hand.

**Round trips**: four newly accepted edits (two per file with flips, seed 7) pass the probe,
`qpdf --check`, `text_wrap_check.py --compare`, and keep every break (6 -> 6 and 1 -> 1). Both
Arcadia page 66 edits are refused now.

Tests: `wrap_tests` gained three (the lines spread over two breaks, a block left a few
thousandths to go, a short last line above an indented paragraph) and three changed: the cut
test's next paragraph moves 4 pt rather than two lines; the grazed test's word under the
paragraph's end moves to keep its break, and it gained a tagged and an untagged column beside the
paragraph; and a clipped-away paragraph stays. The mutation table gained eleven under `spread:`
and `keep breaks:`, eleven were re-aimed, and two went: a duplicate, and one of deleted code. All
40 under `spread:`, `cascade:`, `keep breaks:`, `wrap room:` and `beneath:` are caught. Three conditions were written and deleted as
unreachable: a cap on a block's distance (it cannot exceed what moved above it), a branch moving a
block the whole way when a line lands on it from beside (`lands` needs the extent the other
branch already covers), and the ink widening of the bottom.

### Wrapping on pages without tags — measured 2026-09-24

`docs/PLAN.md` §7 item 4. Until now the wrap ran only where the structure tree names each run's
paragraph, which is a quarter of the corpus. On a page without tags the blocks are now read off the
lines (`textedit/blocks.rs`), and the wrap and the cascade below it run unchanged on them.

**How the rule was judged.** *What a paragraph model would have to work with* counted line pairs:
one pair in six that a geometric rule joined was two paragraphs, and on that count the rule was
not safe. That count predates the cascade. Now that a wrap moves the blocks below its paragraph,
a paragraph set solid under another moves the same distance whether or not it is part of the same
block, so the pair count no longer says what a wrap does. The measure used here is the wrap itself:
the rule forced onto the seven tagged files (`TPDF_PROBE_GEOMETRIC=1`), every trial's written page
content hashed (`TPDF_PROBE_DIGESTS=1`), and each result compared against the tags' result for
the same edit, then rendered where the bytes differ.

```sh
TPDF_PROBE_DIGESTS=1 python3 scripts/textedit_growth.py <probe> <tagged files> --agree-every 0 --records <tags>
TPDF_PROBE_DIGESTS=1 TPDF_PROBE_GEOMETRIC=1 python3 scripts/textedit_growth.py <probe> <tagged files> --agree-every 0 --records <rule>
```

The first rule, the measured Python candidate ported as it was, looked fine by that measure:
70% of its wraps on tagged pages were byte-identical to the tags', and with no wrap refused it took
+25% as typed across the corpus from 57.27% to 70.39%. Rendering what differed, and rounds of
untagged samples through `--roundtrip`, `qpdf --check`, `text_wrap_check.py --compare` and a render,
found what the numbers hid. Every change below was prompted by a render:

- **A paragraph whose first line starts at a deep tab stop became two blocks side by side**
  (Arcadia page 27). The wrap moved one and left the other, and two lines ended up on one line.
  **Rows of a two-column résumé came apart the same way** (Illinois). Both are now refused
  (`BESIDE`): on a page without tags, a moved line must be level with exactly the text that
  stays that it was level with before. On a tagged page the tags say what belongs together,
  and the check does not apply. **The same holds for drawings**, found on the forced run: on
  Arcadia page 98 a wrap the tags refuse moved *By* four points below the signature line drawn
  beside it.
- **A block of one line wrapped into the margin**: with no second line there is no pitch and no
  measure, and the room ran to the page edge (Arcadia page 33, a pdfTeX reference). On a page
  without tags a one-line block does not wrap; a tagged one-line paragraph still does.
- **A numbered item's continuation was read as the next item's indented first line** (Hugo
  minutes), so an item's second line and the next item's first line became one block. A line
  opening with a list label (`7.`, `(a)`, `iv)`, a bullet) now starts a block, and only its first
  line may hang out to the left of the rest.
- **One italic word split a paragraph**: fonts were compared run by run. Lines are now compared by
  the font and size most of their characters are set in, and a tie compares as nothing.
- **A table row set as one run, its columns aligned with spaces** (Union County budget, page 22), wrapped
  at a space and put its last cells on a new line. A line holding three spaces in a row, or a
  show with a displacement of an em or more after its first item (a table in the recent arXiv
  paper), is a table row and joins nothing. This took the budget's gains from 1,045 to 56.
- **Two columns a narrow gutter apart were read as one line** where a justified line reached the
  gutter (the passport guidance, page 16), and the wrap flowed the other column's text. The gutter is now
  one em, down from two: a word space is a third of one, and a tab stop is a boundary too.
- **Something drawn between two lines is measured from the baselines**, a quarter em below the upper
  one (an underline sits above that) to three quarters of an em above the lower one; lines set 14 pt
  apart have overlapping glyph boxes and no gap between them to look in.

**Against the tags, on their own pages.** Forced onto the seven tagged files, at +25% as typed the
tags wrap 928 edits and the rule 699; 692 are wrapped by both and 566 of those write byte-identical
page content. Of the 126 that differ, 106 render identically, and the other 20, all on Arcadia, were
looked at one by one: the rule's result is sound in each and better in some (on page 47 the tags
break *MorrowMo-* inside the word; on page 102 they move *By* below its line). The rule refuses 236
edits the tags accept, which is the safeguards above doing what they are for, and wraps 7 that the
tags refuse; on the build before the drawing check, all seven passed `--roundtrip` and
`text_wrap_check.py --compare`.

**Across the 31-file public sample**, 44,282 runs, against the records of *Spreading the added
lines*, edits as typed:

| trial | before | after |
|---|---:|---:|
| +10% | 67.43% | 69.85% |
| +25% | 57.27% | 61.33% (+1,800) |
| +50% | 50.80% | 55.20% |

`--compare` over every trial: 4,909 verdicts moved from refused to accepted and none the other
way, with 9,309 worker agreement checks and no disagreement. At +25% the gains are LuaTeX 616,
ReportLab 604, xdvipdfmx (fontspec) 402, the two arXiv papers 106, Union County 56, Wellington 13
and the SampleForms invoice 3.

**The round trips found a writer defect that had nothing to do with the rule.** xdvipdfmx writes a
show straight after the operator before it, `10.211 0 Td[<0035>...]TJ`, which is a boundary only
because the array opens with a delimiter. A moved show is written starting with its `Tm`, a
number, so the splice produced `Td1 0 0 1 ...`, and the saved page no longer parsed (*"couldn't
parse input"*). `streams::rewrite_expanded` now puts a newline between the two when neither side is
a delimiter. The tagged documents never showed it, because Word and Acrobat always write the space.
The test for it reads the spliced page back through the scan: lopdf's own `decode_strict` accepts
`Td1` without complaint, so a test that decoded the bytes passed with the fix removed.

**The instruments needed two corrections for fonts without a space glyph.** Where the font has no
space the writer sets word gaps as kerns, and the space a line breaks at is written nowhere. The
round trip looked for the replacement in the reopened runs' concatenated text, and failed for that
reason; it now compares without whitespace. `text_wrap_check.py --compare` still fails on such a
glyph count, on text pushed along its line and on cascades that move many blocks by many different
distances. Every one of those failures in these samples was rendered and was right on the page.

**Samples**: three rounds of untagged wraps (seeds 11, 23 and 37), 89 edits across nine producers,
all through `--roundtrip` and `qpdf --check` and every one rendered. The last round, on the final
rule, passed 25 of 31 on `text_wrap_check.py`; the six others are the checker cases above.

**What is left.** A line of a code listing wraps at a space like prose (ReportLab page 104): the
layout holds, but a reader may prefer a refusal. Two-column pages lose most of their wraps to
`BESIDE`, because each column's lines are level with the other's. Telling a column apart from a
row beside it is the next refinement.

Tests: `blocks_tests.rs` has eleven for the rule, one per signal and each with its boundary;
`wrap_tests` replaced *an untagged page keeps the page edge refusal* with one that wraps an
untagged paragraph exactly as the tagged one, and gained the `BESIDE` test with its tagged control;
`streams` gained the splice test. The mutation table gained 32 under `geometric blocks:`,
`labels:`, `wrap room:` and `splice:`, and two `wrap tags:` mutations were re-aimed: with the tags
removed the geometry now gives the same blocks, so the test they named stayed green.

### Two columns on pages without tags — measured 2026-09-25

The previous section ended on this: two-column pages lost most of their wraps. Measured first, on
the same 31 files at +25% as typed, the refusal it named, *out of line with the text beside them*,
was 1,374 edits (3.1%) and *other text follows it* 611 (1.4%), both concentrated on the two arXiv
papers, ReportLab, LuaTeX and fontspec. The largest refusal is something else: *it reaches the
edge of the page*, 7,709 (17.4%), of which 2,390 are the Union County budget's table rows, which do
not wrap by design.

**Two halves, one for each column.**

- **The right-hand column.** Its lines end at the page edge, so the wrap ran, and every line it
  moved came level with a different line of the other column. The check that keeps a label beside
  its entry refused them all. Text in a column beside the paragraph is now exempt from that check
  (`column_runs`): a geometric block of at least three lines and at least half as wide as the
  paragraph. A label, a date beside an entry and a line's far end at a tab stop are a line or two,
  or narrow, and still refuse.
- **The left-hand column.** Its lines end where the column across the gutter starts: that was
  *other text follows it*, which the wrap does not answer. A column's text now ends a line as the
  page edge does, `Room::Column` (*it reaches the next column*), and is never pushed along its line.

**Before and after, +25% as typed:** accepted 61.34% to 62.05% (+317). *Out of line* 1,374 to
1,008; *next column* is new at 88. Over every trial, `--compare` moved 1,280 verdicts from refused to
accepted and 302 the other way.

**Of the 302, the four rendered were the old push damaging the other column.** They were rebuilt
with the previous probe; the other 298 were not looked at. In two arXiv pages the edit had pushed the neighbouring column's line
sideways, into the gutter and past the page edge, colliding with the edited line; in ReportLab it
pushed a table cell's line across the rule between cells. The fourth, a LuaTeX table of contents,
pushed an entry's title against its number (*10.7.1010in_name_ok*) and its page number a few points
out of line: intact, and not good. All four round-tripped, so none of this was visible to
`--roundtrip`; only the render showed it.

**The gains were checked the same way:** twelve, two per file, through `--roundtrip` and
`qpdf --check`, all passing, and five rendered, a wrap in each column among them. Each is sound.

**A condition was removed because it could not be reached.** The first version also asked that the
column lie wholly to one side of the paragraph. Removing it changed no outcome, including for a
wide block beside the short last line: a block across the paragraph's width that is beside or below
a line the wrap moves is moved down with it (`cascade`), so the text left level with a moved line is
beside the paragraph already. The doc comment records why the condition is absent.

Tests: `wrap_tests` gained one per half, each with row and label controls that still refuse, and
a push that carries the rest of a line into the column. The mutation table gained seven under
`columns:`; the existing `wrap room: a drawing beside may come apart` was re-aimed at rustfmt's new
line. What is left is the page-edge refusal above, which is four times the size of this one.


### Two columns with staggered baselines — measured 2026-09-25

The largest refusal left after the section above was *it reaches the edge of the page*: 7,567
edits of 44,282 (17.1%) at +25% as typed. A temporary `eprintln!` at each of the seven places
`wrap::plan` gives up without a stated reason, over the six files that hold nearly all of them,
put **7,575 of 7,692 at one**: a one-line block on a page without tags, which has neither a pitch
nor a measure to wrap to. The remaining 117 were a pitch outside `PITCH_EM`.

**Why so many lines were blocks of one.** `blocks.rs` numbered the page's lines top down and
paired each line with the page's *next* line that overlapped it. In ACM's two-column layout, both
arXiv papers here, the columns' baselines are offset by about 2.7 pt, so on the page the lines
alternate between the columns. Each line's next line was the other column's, which does not
overlap it, so every line of both columns was a block of its own. The measurement driver,
`scripts/textedit_blocks.py pairs`, paired the same way, and its comparison against the tags
scores only the pairs it forms, so the section *What a paragraph model would have to work with*
could not have shown it. The trap index has the entry.

**The change.** A line is paired with the nearest line below it that overlaps it, in both the
rule and the driver. Across another line of the page the pitch must be at most two ems
(`SKIP_PITCH_EM`); a pair on the page's very next line keeps the three-em limit. The two ems are
read off the driver's pitch distribution on tagged pages: 43 pairs between 1.5 and 2 ems, 883
between 2 and 3, which is the gap between paragraphs. Without the limit, the nearest-line pairing
added 11 false joins on tagged pages, ten of them a paragraph gap of 2.3 to 2.7 ems across a line
set elsewhere on the page. With it, one (`mercer-minutes`, 1.2 ems apart with left edges 15 pt apart), not looked at further.

**Against the tags**, the stored records of 2026-09-20 re-scored (`--report`): joined within one
element 2,602 → 2,602, joined across two 577 → 578, split within one 508 → 512. Tagged pages
hold almost no staggered columns, so this says the change costs nothing there; it is no evidence
about the untagged two-column pages it is for.

**Before and after, +25% as typed, the 31 files:** accepted 62.05% → 62.41% (+161). The page-edge
refusal fell by 1,349, to 6,218. Most of those edits now reach the wrap and are refused by it:
*out of line with the text beside them* +651 (to 1,659), *a drawing or an annotation is placed
over the lines* +231, *onto what is below it* +152, *part of it below cannot be moved* +122,
*it reaches the next column* +71. Over every trial `--compare` moved 609 verdicts from refused to
accepted and 179 the other way. Of the 179, all but three are in the two arXiv papers and
fontspec.

**The 179 are the other column being pushed.** Twelve, two per refusal kind, were replayed with
the previous probe from a worktree at `627c190`: all twelve were accepted there, and each of the
seven renders looked at pushed the rest of a left-column line along into the right column's text
(*aged bywhat we**somewhat***, *LNUM Numbers=UppercaseLNUM Number* over *Lining Figures*). With the
columns now read as blocks of several lines, the column rule of the section above stops those
pushes here too.

**The gains:** ten, two per file, through `--roundtrip` and `qpdf --check`, all passing; four
rendered, two of them wraps in arXiv's left and right columns, each moving the rest of its
column down and leaving the other column alone.

Tests: two in `blocks_tests`, staggered columns and the two-em limit with its no-skip control;
four mutations under `geometric blocks:`, each red on its test. The driver's `--self-test` gained
the same two cases as plain data, and three mutations of its pairing (the page's next line only,
no limit, the limit on every pair) each turned it red. What is left: the page edge is still the
largest refusal at 14.0%, now followed by *out of line with the text beside them* at 3.7%, which
this change more than doubled and whose cause on these pages has not been looked at.


### A column's headings and short lines — measured 2026-09-25

After the section above, *its lines would move out of line with the text beside them* was 1,659
edits (3.7%) at +25% as typed. A temporary `eprintln!` at that refusal, naming the text that came
apart, over the five files that hold nearly all of it: most were **short blocks in the other
column**. The column rule of *Two columns on pages without tags* exempts a block of at least three
lines at least half as wide as the paragraph; a column's headings (*2 Background and Related
work*, *3.3 A test corpus*), a paragraph's last line (*inputs.*) and a first line split off for
an italic phrase are one or two lines, so a moved line coming level with one refused the wrap.
Drawings were the other large source, LuaTeX's the largest share (361 of 858).

**The change.** A column that lies wholly to one side of the paragraph owns every shorter block
set within its measure, to `COLUMN_SLACK` (1 pt) past either edge. Those runs are exempt from the
level check and end a line as the column does (`Room::Column`). A mark in the gutter is within no
column; a block that spans the paragraph is to neither side of it, so a label under one still
refuses. The slack matters: at 0 pt, over arxiv-recent, fontspec and ReportLab, 62 verdicts it
accepts are refused and 15 it refuses are accepted.

**Before and after, +25% as typed, the 31 files:** accepted 62.41% → 62.73% (+138). *Out of line*
1,659 → 1,112. Most of those edits now reach the next check: *a drawing or an annotation is placed
over the lines* 1,277 → 1,684, *onto what is below it* +91, *it reaches the next column* +76. The
page edge fell by 119, to 6,099 (13.8%). Over every trial `--compare` moved 505 verdicts from
refused to accepted and 134 the other way.

**The 134.** Ten, two per file, replayed with the previous probe at `23af1c1`: all ten accepted
there. Of the nine renders looked at, eight were a push into other text: a figure caption into the
next caption (*(LefFigure 19*), a table cell into the next (*eColorlinesColor*), a reference line
into the other column, a line into a heading, a LuaTeX option over its description, and fontspec
feature names over their neighbours.
**One was not damage**: in the LuaTeX manual a superscript footnote mark grew from *6* to *66* and
pushed its table cell a few points along. It is now refused with *it reaches the next column*,
because a table column of three lines or more is at least half as wide as a one-character
paragraph. That is the column rule's width test meeting a paragraph that is a mark, and it is not
fixed here.

**The gains.** Twenty, through `--roundtrip` and `qpdf --check`, all passing; nine renders looked at,
all sound: wraps in both arXiv columns and in a bulleted list, pushes inside ReportLab's code
listings, and two wraps in fontspec's tables of language and script names. Those two moved one
column of the table down a line and left the others, which looked like a table coming apart until
the whole width was rendered: each is a list set in six or four independent columns, and fontspec
wraps its own long entries in them the same way (*Malayalam Traditional*).

Tests: `wrap_tests` gained one, a heading half a point outside a column's edge that no longer
refuses the wrap, with three controls that still do (no column, a mark in the gutter, a block across
the paragraph). The mutation table gained five under `columns:`. Next by size: *a drawing or an
annotation is placed over the lines* at 3.8%, and the page edge, still first at 13.8%.


### Links over lines a wrap moves — measured 2026-09-25

After the section above, *a drawing or an annotation is placed over the lines that would move*
was 1,684 edits (3.8%) at +25% as typed. A temporary `eprintln!` at that refusal, over the seven
files that hold nearly all of it, named what was in the way: across every trial, 3,244
annotations, 1,174 rules under two points tall (underlines and fill-in lines in the Hugo and
Arcadia minutes), 188 boxes and 23 small shapes. Every annotation on those pages is a `/Link`
held by reference, with no `/AP` and no `/QuadPoints`: 1,288 of them over the five files, counted
with `pypdf`. A link is a rectangle over words and nothing drawn, so moving the words and leaving
the rectangle sends a reader who clicks them somewhere else.

**The change.** A wrap moves such a link with the run it lies over: the run's hit rectangle holds
the link to `LINK_SLACK` (2 pt) on every side, and the link's `/Rect` moves as far as the run on
the displayed page, carried into the page's own space from the rectangle before and after
(`textedit::prepare_batch`). A link with an appearance, with quadrilaterals, written into
`/Annots` directly, over more than one run, or any other annotation, stays where it is and refuses
the wrap as before. This is the first edit that changes an object the document already had: the
save is the rewrite, which serialises the whole document, and comment edits already change
annotation dictionaries there (`save::rewrite_note_edits`).

The 2 pt is measured, as how far a link reached past the moved run's rectangle, logged at every
check over arxiv-recent and ReportLab: not at all 1,107 and 1,384 times, by up to 1 pt 4,170 times
(all arxiv-recent), by 1 to 2 pt 7 and 111 times, and by more 264 times, which stay refused.

**A prototype that moved nothing first**, treating a link inside a moved run as absent, measured
the ceiling on the five files with links: 901 more edits accepted at +25%, and every one of them
reached *accepted*, not a later refusal.

**The measurement then went wrong, and the instrument was the cause.** The first corpus run of the
real change showed 236 edits newly accepted, 248 newly refused and a net of -13. `text-edit-probe
--growth` undid each accepted trial by restoring the page dictionary and dropping new objects; a
moved link is neither, so every later trial on the page met the links where the last one had left
them. Its proof of the restore rescanned text runs, which a link is not. Both probes now restore
the page's annotation objects and compare every object with a copy taken before the page's trials;
with the annotation restore removed that comparison stops the run on the first page with a moved
link. The trap index has the entry.

**Before and after, +25% as typed, the 31 files:** accepted 62.73% → 64.79% (+914), the annotation
refusal 1,684 → 770. Over every trial `--compare` moved 2,545 verdicts from refused to accepted
and **none** the other way.

**The gains:** twelve, two per file, through `--roundtrip` and `qpdf --check`, all passing. The
round trip compares pixels, and a link draws none, so the saved links were checked separately: for
each link whose `/Rect` changed, the words under the old rectangle in the original and under the
new one in the saved file, read by `pdftotext`. All 23 moved links read the same words. As a
control, the old rectangles read against the saved page gave different words for 4 of 6 on one
ReportLab page, the other two being right-aligned page numbers that line up anyway. One fontspec
gain moved no link at all: the words under all four of its links are unchanged, so the link was
over text the wrap left in place, which the refusal had not told apart. Two renders showed the
moved lines sound; one also shows the wrap's existing habit of leaving a hyphenated word part alone
on the line it flowed to.

Tests: two in `wrap_tests`, a link that moves with its line with five controls that refuse (a
highlight in the same place, an appearance, quadrilaterals, a link written into the list, one
reaching 2.5 pt past the line), and the same move on pages turned 90°, 180° and 270°. The mutation
table gained nine under `wrap links:`, among them applying the displayed move to the page's own
space unturned, which only the turned-page test catches. Next by size: the page edge at 13.8%,
*onto what is below it* at 10.0%, and the rules under text in the minutes.


### A wrap's lines against the lines beside them — measured 2026-09-25

After the section above, *its lines would move onto what is below it* was 4,448 edits (10.0%) at
+25% as typed. It is one message for seven places a wrap gives up; a temporary `eprintln!` at each,
over the five files that hold most of it, put two of them far ahead.

- **Landing on text that stays, in the Coatesville minutes (2,841).** 2,826 of them land on the
  page's running footer, *1 Minutes*, 21 pt below the last line. Those pages are full transcript:
  every block below the edit moves down and the last line meets the footer. The refusal is right,
  and the only remedy is flowing onto the next page, which the editor does not do.
- **The laid-out text overlapping another line, in ReportLab (4,996) and LuaTeX (704).** Logged
  with the rectangles: 4,548 were the wrap's *first* line and 448 its next, each overlapping a line
  above or on the edited line by 0.5 pt (3,018) or 1.2 pt (1,291). ReportLab sets 10 pt text on a
  12 pt pitch in boxes 12.5 pt tall, and its code listings 8 pt on 8.8 pt in boxes 10 pt tall, so
  every line's box overlaps its neighbours' by that much. The edited run's own box overlaps them
  the same way, and `strikes` excused only the part of an overlap inside that box. A wrap's first
  line is the edited line made longer, and the part past the run was refused for the sliver the
  run itself has.

**The change.** An overlap between a laid-out line and another line's box is a graze, not a strike,
when it is no deeper than `GRAZE`, an eighth of the shorter of the two heights. Logged on five
files wherever only that allowance let an edit through: 37,211 overlaps at an eighth of a line or
less, 2 between that and 0.14, 198 above, which stay refused. It holds for every laid-out line, a
box the reader sized included, so a box may sit as close to its neighbours as the document's own
lines do and no closer.

**Two rules came before it, and both are recorded because the second one's failure is the point.**
The first excused a graze only between two lines that are not level ([`level`], half a line) and
only inside the edited line's own band. Three wrap tests could not make the band condition matter,
because the landing check refuses or moves anything below the paragraph first, so it was removed
as unreachable. The corpus run of that first rule, which still had the band, then showed 9,248 of
the gains were **boxes the reader sized**, where nothing below moves: without the band, a box's
second line would be free to overlap the text under it by up to half a line. That was read off the
code, not measured, and it was enough: the condition was reachable, only not by a wrap. The depth
limit replaced both, and a box and a wrap are held to the same thing.

The test measures what `strikes` measures, the new line's ink against the other line's box. The
fixture's lines are capitals, whose ink sits well inside a 15 pt box, so a box overlap of 30%
still grazes and 35% does not.

**Before and after, +25% as typed, the 31 files:** accepted 64.79% → **76.95%** (+5,383). Every
*no room* refusal fell: *onto what is below it* 4,448 → 2,305, the page edge 6,099 → 4,238, *a
drawing or an annotation* 770 → 447, *out of line with the text beside them* 1,112 → 635, *part of
it below cannot be moved* 681 → 393, *a picture or a drawing follows it* 493 → 301. Over every
trial `--compare` moved 24,039 verdicts from refused to accepted and **none** the other way;
13,823 of them in ReportLab.

**The gains:** twenty, sixteen as the editor opens the box across ten files and four at a width
the reader chose, replayed on the probe's 5% ladder. All passed `--roundtrip` and `qpdf --check`.
A tagged page does not survive extraction with `qpdf --pages`, empty base or not, so the five
tagged documents were replayed whole. Thirteen renders looked at, all sound: each grown line or box
meets the lines above and below as the lines beside it do, and in none does its ink reach another
line's.

Tests: `wrap_tests` gained one, a wrap on lines set at 90% of their box height, with a control at
65% that refuses and one where text that stays below is reached; three mutations under `wrap
graze:`. What is left, at +25%: the page edge 9.6%, *onto what is below it* 5.2%, and the running
footers of full pages among it.

### A run set off its line's baseline — measured 2026-09-25

After the section above, *it reaches the edge of the page* was still the largest refusal at +25%
as typed: 4,238 of 44,282 edits (9.6%). **2,390 of them are `unioncounty-budget` and correct**:
a mainframe printout whose every row is one Courier line the width of the page, so a row a
quarter longer has nowhere to go and wrapping it would break the table. Nearly all of the rest
are in the LuaTeX manual (1,158) and fontspec (515), and most of those are one-character runs in
the middle of a line: the lowered E of the TeX logo.

**Why they did not wrap.** A temporary `eprintln!` at each place `wrap::plan` gives up without a
stated reason, replayed on one of them (`luatex-manual` page 23, the E of *LuaTEX*), fired at the
one-line-block check. Over seven files, in a run stopped part-way, it fired there and at the check
that every show pushed along the line is the block's own, and at nowhere else but three pitches
out of range. Both are one cause. `blocks.rs` made a line of every set of baselines within 0.5 pt,
so the E, about 2 pt below its line, was a line of its own. The line above it then paired with the E
as its nearest overlapping line below, not with its paragraph's next line, and the paragraph was
cut in two there. Superscripts and footnote marks did the same.

**The change.** A group of runs on one baseline is *set off* a neighbouring group when it has
fewer characters, its baseline is within half an em of the neighbour's (`SHIFT_EM`, the same
half em `wrap.rs` already reads a line by, `SAME_LINE_EM`), and none of its runs is a gutter or
more clear of the neighbour's extent along the line. It is then on that line, and the line keeps
the neighbour's baseline, so a mark opening a line does not change its pitch. The extent limit
is what keeps a second column apart: its lines are close to the first column's baselines but a
gutter away from them. The wrap already moves a flowed run by a translation, so the E keeps its
drop on the line it lands on. `scripts/textedit_blocks.py` applies the same rule, with
`chars`, spaces included, in place of the characters other than spaces the rule counts.

**Against the tags**, the stored records of 2026-09-20 re-scored (`--report`): joined within one
element 2,602 → 2,604, joined across two 578 → 581, split within one 512 → 483, split across two
822 → 827. Most of the 29 fewer splits within one element are pairs that no longer exist, a line
paired with a mark set off it.

**Before and after, +25% as typed, the 31 files:** accepted 76.95% → **78.93%** (+879). The page
edge fell by 1,018, to 3,220, and *it reaches the next column* by 51. Some of those edits now
reach the wrap and are refused by it: *out of line with the text beside them* +109, *part of it
below cannot be moved* +55, *a drawing or an annotation* +33. Over every trial `--compare` moved
2,655 verdicts from refused to accepted (`luatex-manual` 2,073, fontspec 560, arXiv 2003 22)
and 21 the other way.

**The 21 are 8 runs**, replayed with the probe at `da2e0fe` on pages extracted with `qpdf --pages`:
all eight were accepted there, and six of the renders were broken. Four pushed a cell's text into
the next cell or column (*scriptcramb*, *dynCH₂*, *oaxisexact n*), one moved a *b* off its sub-
and superscripts below a table's rule, one wrapped over the text beside it. The two sound ones
are a mark growing, the arXiv superscript *m* → *mm* and a fontspec footnote mark *8* → *88*,
both now refused with *it reaches the next column*: the same width test of the column rule as the
LuaTeX footnote mark recorded in *A column's headings and short lines*.

**The gains:** ten, two per file and four more from the LuaTeX manual, through `--roundtrip` and
`qpdf --check`, all passing, and all ten renders looked at. Each grown logo, superscript or
citation pushes the rest of its line along, the line's last word wraps onto a new line, and the
lines below move down one; the lowered E keeps its drop wherever it lands.

Tests: `blocks_tests` gained one, a run raised and lowered by 2 and 5.9 pt that is on its line,
6.1 pt that is not, a mark opening a line, a run between two lines it could be set off that goes
to the nearer, and a second column whose baselines are 2 and 5 pt off the first's, which stays a
column; `wrap_tests` gained one, a lowered run that flows after the edit 2 pt below the line it
lands on. Eight mutations under `geometric blocks:`, all caught by the test named for them. What
is left, at +25%: the page edge 7.3%, 5.4 of it the budget printout's rows, and *onto what is
below it* 5.2%.

**The line need not be the next baseline.** The first version looked for the line a group is set
off only one baseline up and one down. Replaying the two sound regressions showed why that was
too near: the fontspec footnote mark *8* had the raised A of the LaTeX logo between it and its
line, and the arXiv superscript *m* had a line of the other column, whose baselines are staggered
against its own. Any group within half an em may now keep it, the nearest by baseline. Against
the first version, +25% as typed: accepted 78.93% → **78.96%** (+12), and over every trial 36
verdicts from refused to accepted and none the other way. Against `da2e0fe` the two together move
2,688 from refused to accepted and 18, in 7 runs, the other way: the six broken pushes above and
the arXiv superscript, now refused because `wrap::plan` reads its line in the superscript's own
size (half an em of 5.5 pt), so its own line, a few points below, is a line below it. The
fontspec mark now wraps. `blocks_tests` gained both shapes and one mutation.

**What the page edge and the next column still refuse.** A replay of 80 of the 817 page-edge
refusals outside the budget printout, and of all 137 *it reaches the next column*, with the
temporary `eprintln!` back in `wrap::plan`: 70 and 111 stop at the one-line-block check, 3 and 18
at the pitch check (the superscript's own size, as above), 7 and 5 at a show on the line with no
block. The one-line blocks, read off `blocks.rs` segment by segment on four pages, are:
paragraphs that really are one line, with a table or a display after them (*All traditional
TEX…* on LuaTeX page 27); list items of one line; table-of-contents entries and table rows; a
reference list's lines; and continuation lines whose neighbour is mostly set in another font, a
list item whose first line is mostly primitive names in monospace. Wrapping the first two would
need a pitch and a measure the block does not have, taken from the page's other paragraphs in
its font; the face rule is what keeps table rows and entries apart. Neither is changed here.

### What a wrap lands on below it — measured 2026-09-25

*Its lines would move onto what is below it* was the largest refusal the wrap itself gives, 2,315
of 44,282 edits at +25% as typed. A temporary `eprintln!` at each of the seven places that return
it, naming what was landed on, over 150 of them drawn at random and replayed with
`--growth-request` and `--roundtrip` (whole documents up to 128 pages, the page extracted with
`qpdf --pages` beyond that):

- **115 are a full page**, all in Arcadia and Coatesville: the moved lines would land on
  something 42 to 52 pt above the page's bottom edge and 10 to 15 pt tall, the running footer.
  Two renders looked at, one of each, have their last line of text directly above it. Those
  refusals are right: the text has nowhere to go on the page, and flowing it onto the next one
  is a feature, not a fix.
- **34 are mid-page, in the LaTeX documents.** Replayed with the overlap measured, 11 land on
  text that stays by 0.2 to 0.8 of the shorter box, 7 by 0.09 or less, 12 not at all: those are
  refused by the rule that a moved line may not close the space to text below it to less than a
  blank line. Three were the edit's own lines overlapping as laid out, and one the paragraph's
  moved bottom landing.

**The rule counted text on the moved run's own line as below it.** It asks whether the other
text is ahead of the run by comparing their centres, and the lowered E of the TeX logo, on the
edited line itself before the edit, has its centre a couple of points lower than the text after
the edit. When that text flowed onto a new line it moved towards the E, closer than a blank line,
and the wrap was refused. The fontspec page 11 case: the T of *XƎTEX* growing, the Ǝ before it
2.3 pt below its line. Text sharing half the shorter box with the run, the rule `lands` already
uses for a sliver the source had, is now on the run's line and never ahead of it. A second
change, allowing a moved line the overlap the source's own moving lines had with the text that
stays, gained one of the 34 more and was left out.

**Before and after, +25% as typed, the 31 files:** accepted 78.96% → **79.30%** (+153). *Onto
what is below it* fell by 196, to 2,119; 38 of those edits now reach the wrap's text-beside check
and are refused by it, and 5 the drawing check. Over every trial `--compare` moved 491 verdicts
from refused to accepted (fontspec 266, `luatex-manual` 224, Arcadia 1) and **none** the other
way.

**The gains:** fifteen, ten of the 34 samples and five from the corpus run, through `--roundtrip`
and `qpdf --check`, all passing; seven renders looked at, all sound. In each the text after the
edit flows onto a new line past the lowered E before it, and the lines below move down one.

Tests: `wrap_tests` gained one, a lowered run before the edit and text after it that flows onto
the next line; it fails without the change with the corpus's refusal. Two mutations under `keep
breaks:`, and three there re-aimed at the changed line, all caught by the test named for them.
What is left, at +25%: *onto what is below it* 4.8%, three quarters of it full pages.
