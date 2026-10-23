# PDFium pins and their records

Each engine pin since 8044: what moved, the candidate run that built it, the probes that cleared it, and the release verifications that turned on the engine. Newest first; the record of a new pin goes at the top. Moved here verbatim from `BUILD.md` on 2026-10-23, which indexes this file.

**PDFium 8103-tpdf.1 (2026-10-23).** `Audit` went red on the 26.10.25 release commit: Chrome
stable 157 had moved to `chromium/8103`, and `chromium/8078` had gained commits after the pin.
`pdfium-bump` started the build and opened issue #5, and the build failed on all three
platforms, for two reasons. On macOS, the builder's `patches/mac/build.patch` at `f2e9a1c` no
longer applied to `toolchain/apple/toolchain.gni`. Its next commit, `976ce1f`, changes only that
patch. Checked with `git apply --check` against the `chromium/src/build` revision that 8103's
DEPS names (`ebdaeefa`): `976ce1f` applies, `f2e9a1c` fails as on the runner, and
`clang_rt.patch` and `win/build.patch` apply too. On Linux and Windows, the engine passed its
text tests and the RTL observation, and then the licence inventory refused libyuv, which 8103
compiles in. Its `LICENSE` at the DEPS revision (`eb8eda9c`) is BSD-3-Clause, and
`build_pdfium.py` copies it as it does Dragonbox's and HarfBuzz's. With both changes,
[run 39936822326](https://github.com/tstone-1/tpdf/actions/runs/39936822326) at `8ab2d086`
built all three platforms: 63 upstream text tests on macOS and Linux, 62 on Windows, and the
two mixed-direction limitations unchanged. **The code moved.** Every library differs from
`8078-tpdf.2`, by about 1.1 to 1.8% in size, so the probes were run. On Windows 11: `remove-probe c`;
multilingual search 68/68 and encoding search 23/23, which need `--lib vendor/pdfium/bin`
when started from the repository root; `progressive-probe` identical on vector-heavy and the
form; `backend-probe` 42/44 on `vector-heavy.pdf` with two skipped and 41/44 on
`text-heavy.pdf` with three skipped; `worker-probe` 48/48. On macOS the engine scan found no
V8 and no XFA, and `remove-probe c` passed. The notices cross-check is byte-identical against
the macOS and Linux archives, and libyuv's notice is the new one. All 33 gates passed on the
installed engine. `window_checks.py` passed 16 of 16 on a second Mac through `run_on_mac.py` and
16 of 16 on Windows 11 on a `--no-bundle` checks build, and `scripts/run_on_windows.py --suite
all` passed 3,284 unit tests and 622 command-line checks, with 4 skipped.

**PDFium 8078-tpdf.2 (2026-10-18).** The weekly check found one commit on `chromium/8078`
after the pin, `2f22dfe` *[M156] Guard against re-entrant destruction of the CJS_Runtime*,
and `Audit` went red. The automatic path worked as written: `pdfium-bump` started
[run 39150711792](https://github.com/tstone-1/tpdf/actions/runs/39150711792) at `61a474a6`
and opened issue #4; all three platforms built and passed 63 upstream text tests (62 on
Windows). The fix is in the JavaScript runtime, which this build does not compile: `nm` on
the macOS library finds only `CJS_RuntimeStub`. **The compiled code did not move.** Against
`8078-tpdf.1`, the macOS and Linux libraries are byte-identical, and the Windows DLL has the
same `.text` section and differs in 12 bytes of `.rdata` and `.rsrc`. So the compatibility
probes were not re-run: a probe of identical code measures nothing new. The gates passed on
the installed engine. This is the first bump to settle the probe question by comparing
binaries, and it is worth doing first on the next one too: when the code sections differ,
run the full list below.

**PDFium 8078-tpdf.1 (2026-10-10).** The pin moved from `chromium/8066` to the head of
`chromium/8078` (`bdbf237`), the branch Chrome 156 ships. Nobody had looked since the pin
was set: Chrome stable had moved on, and the day before, upstream had merged *Correctly
initialize current cell in agg::outline_aa* into 8078, a fix in the rasterizer this build
compiles in. [Run 38073135904](https://github.com/tstone-1/tpdf/actions/runs/38073135904)
at `b18adac` built both platforms in 17 minutes and passed 63 upstream text tests on macOS
and 62 on Windows. The builder and depot_tools pins did not move. On the installed macOS
engine: all 31 gates; `remove-probe c`; both search corpora, multilingual 68/68 and
encoding 23/23; the engine scan with no V8 or XFA; `progressive-probe` identical on
vector-heavy and the form; `backend-probe` 42/44 on `vector-heavy.pdf` with two skipped
and 41/44 on `text-heavy.pdf` with three skipped; `worker-probe` 48/48. `window_checks.py`
passed 11 of 11 on a second Mac and 11 of 11 on Windows 11, and on Windows 11
`scripts/run_on_windows.py --suite all` passed 3,106 unit tests and 567 command-line
checks, each run printing the engine it installed. `scripts/run_on_mac.py` copied
`vendor/pdfium` from the other Mac's clone until this pin, which would have checked the
new tree against the old engine; it installs the pinned one now.

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
colour space: sRGB red in the development Mac's P3 profile. It now reads sRGB and passes 48/48; see
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

*History: this paragraph and the two after it describe the 8044 engine. Since
`pdfium-8066-tpdf.1` (2026-09-25, above) the patch is gone and the build compiles one
unpatched engine.* `scripts/pdfium_rtl.patch`, against PDFium
`f91ca5a72358bb0b00b4da9481b21fe668157614` (8044), preserved `/ActualText` behavior
and restored ordinary predominantly RTL lines when their first and last strong
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
normal release channel. `scripts/fetch_pdfium.py` downloaded these archives while 8044
was the pin; it now names `pdfium-8066-tpdf.1`. Either way it verifies the hashes of what it
downloads, so do not replace a vendored library manually.
