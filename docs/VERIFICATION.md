# Verification records

What was run to verify each feature: the commands, the platform, the date and the result, one `###` section per feature.
The sections stood at the end of `BUILD.md`'s *Cutting a release* until 2026-10-05 and were moved here unchanged.
Where a section says "this file", "above" or "the top of this file", it means [`BUILD.md`](../BUILD.md).

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

Windows x64 verification on 2026-09-30 at `67d7d5b` covers this section and the
five below it, plus the Phase 5 passport and position work in `docs/PLAN.md`.
After fast-forwarding, the Windows checkout needed
`python scripts\fetch_pdfium.py --force`: its `vendor/pdfium` was still
`pdfium-8044-tpdf.1` against the `8066` pin. Running that fetch in the elevated SSH
session left `vendor\pdfium` with a protected, non-inherited ACL that grants only
SYSTEM, Administrators and the owner (Administrators). The interactive,
non-elevated session then got `LoadLibraryExW` error 5 for `pdfium.dll`, so every
worker probe failed with `could not read source page count`. The application was
unaffected because it loads its copy from `target\debug\pdfium`.
`icacls vendor\pdfium /reset /T` and `/setowner` fixed it, and `--check`
still verified the library. Fetch in the session that will run the checks.
All 562 tests selected by `cargo test --locked --lib textedit` pass on Windows
(1,817 filtered out). Fourteen Mac-generated fixtures arrived with matching
SHA-256 digests, including archive
`29d6e72dc9ddd6f475ae47f2a33943cd69fe884b9ae4c8e6047f3e5d451d15aa`.
The 33 retrieved PDFs matched their Windows digests, archive
`54395eb7941789a717a31dd783f06e077b75983879f23e4cbd3d569b6d9599ec`.
The native checks ran inside the logged-in console session through a temporary
`/IT` scheduled task. The worker-exit observer passed its self-test and reported no
surviving test workers after each of the 14 native runs.

Three independent readers now reject correct output on both platforms, so they
are recorded rather than counted as passes:
- **Refusal wording.** The default `text-edit-probe` mode still expects the refusal
  wording from before `59d25b6`, so its last step fails after a successful save.
- **Explicit layout.** Native saves go through the explicit-layout writer, which
  restates text state and adds a trailing `TJ` offset, so
  `make_textedit_embedded.py --check` reports `operator count changed`.
- **Kept adjustment.** The writer now keeps the authored `TJ` adjustment after an
  edited fragment, so pypdf reports `wrong replacement array` for the
  symbolic-TrueType worker saves. The same kept 20/1000 em (0.24 pt) puts `FIRST`
  at 235.504 rather than 235.744 under `--wide-spacing`.

All 11 Windows worker outputs are byte-identical to a macOS worker built from a
clean `67d7d5b` worktree, and the macOS probe fails at the same wording step.
In place of the stale operator check, a structural pypdf readback accepts all 11
native saves. Each has one replaced show, only text-state, positioning and show
operators added, resources and other pages unchanged, and the same replacement
bytes as the worker. It crashes rather than passing on an unchanged input or a
foreign document.
For this section, both space-code native workflows pass 23/23 checks. For worker
and native saves alike:
- `text_spacing_pdfkit.swift --word-char=space` matches all 14 and 11 painted glyphs.
- PDFKit counts 1,040 (`+1`) and 1,112 (`-1`) changed pixels inside the target, zero outside.

The unedited inputs used as saved files are rejected (glyph populations 14, 14).
The `textedit-wide-spacing` phase passes 23/23. The temporary scheduled tasks and
`C:\tmp` work folder were removed, and `npm run build` restored normal assets
(`check_bundle_share.py`: harness excluded). The checkout is clean at `67d7d5b`.
Remote work took about 21 minutes.

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
page 16 remains the next practical target. Windows x64 verification on 2026-09-30
at `67d7d5b` (setup and reader notes under *Word spacing*) passes the native
workflow, 23/23. Worker and native saves each change 1,040 pixels inside the
target and zero outside, with all 18,816 image pixels still painted. The worker
save is byte-identical to macOS.


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
editable-page count is unchanged. Windows x64 verification on 2026-09-30 at
`67d7d5b` passes both native workflows, `remapped-unicode.pdf` and
`remapped.pdf`, at 23/23 each. The independent pypdf reader accepts both worker
saves: only target operands change and resources are preserved. PDFKit measures
1,852 changed pixels inside the target and zero outside for all four saves.
Worker outputs are byte-identical to macOS.

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
mismatch and later stroke-state operators remain separate work. Windows x64
verification on 2026-09-30 at `67d7d5b` passes the `textedit-cff-unicode` workflow
23/23. The pypdf reader accepts the `--cff-unicode` and `--dash` worker saves.
PDFKit measures 1,455 changed pixels inside the target for the worker and native
`unicode-mapped.pdf` saves, and 593 for `unicode.pdf`, each with zero outside.
Worker outputs are byte-identical to macOS.

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
editable practical page. Windows x64 verification on 2026-09-30 at `67d7d5b`
passes the native workflow 23/23. The pypdf reader accepts the worker save, and
PDFKit measures 2,395 changed pixels inside the target and zero outside for the
worker and native saves. The native save restates `TL` with the other text state.

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
editable practical page. Windows x64 verification on 2026-09-30 at `67d7d5b`
passes both native workflows, `textedit-cff-ligatures` and `textedit` on
`named-mapping.pdf`, at 23/23 each. The pypdf reader accepts both worker saves
with exact ligature codes. PDFKit measures 2,261 and 898 changed pixels inside the
targets and zero outside for worker and native saves.

The same Windows run covers the passport work in `docs/PLAN.md` Phase 5. The
passport guide's digest matches the manifest. `text-edit-probe --inspect --all-pages`
finds page 16 editable with 37 runs and the other 15 pages refused, the same JSON as
on macOS. `textedit-passport` passes 31/31, including mixed-direction selection
through every view turn. PDFKit measures 404 changed pixels inside the label, zero
outside, and zero changed pixels on each of the other 15 pages. `tabs-position`
passes 12/12 on the 600x800/1200x1600/600x400 document and 12/12 on the passport
guide, and `tabs-rotation` passes 10/10 on 600x800/1200x400/600x800.
`text_direction_check.py` passes all 16 text/page direction combinations.

All 24 final gates pass (242.9s summed gate time): 1,481 Rust tests pass
with 3 ignored, 1,668 frontend tests pass, and normal assets contain zero
check-harness code.

### An installed copy of the document's font — measured 2026-09-30

Decided 2026-09-30: when a replacement needs a character the document's embedded subset lacks,
automatic font mode tries the subset, then **an installed copy of the same font, matched by
exact PostScript name**, then the bundled Noto. The preview names the font and marks it
*(installed)*; a copy that was found and not used is named beside the Noto that was, with the
reason. The same edit comes out in Noto on a computer without the font, which the decision
accepts. What the editor admits is in `docs/TEXTEDIT.md`; what crosses the process boundary is
`docs/THREAT-MODEL.md` §T6.26.

**Why, measured.** A structure-only scan of one reader's own PDFs on macOS (aggregate counts
only; no document, font name or content was kept): 58 readable PDFs, 34 embedding subset
fonts; in 18 every subset font was installed on that Mac, and 77 of 154 subset-font occurrences
were. For 43 installed WinAnsi TrueType subsets the PDF `/Widths` agreed with the installed
advances on every glyph within 1/1000 em for 38, on at least 95% of glyphs for 5, and nowhere
wholesale disagreed. That is the tolerance: one unit (`installed::TOLERANCE`), the rounding a
producer's integer widths introduce and the unit the document's Unicode path already allows.

**How it runs.** The worker names the font (`Preview::wants`); the app process looks it up
(`sysfont.rs`: CoreText descriptor match on macOS, which names the file and not the face;
DirectWrite's system collection on Windows, which names both, simulated faces skipped), reads at
most 32 MiB, and asks again with the bytes in `Layout::installed`. The worker accepts the copy
only if the face carries the name in every name record that decodes, has TrueType or CFF
outlines and is neither CFF2, variable nor colour, has `fsType` 0 or 0x8, and agrees with every
width the document's subset declares. It embeds a subset (the replacement's and the document
subset's glyphs, OS/2 and a format 12 character map put back) as a Type0/Identity-H font, and
after an Apply the journal keeps that subset instead of the file. A TrueType subset is embedded
as it is under a CIDFontType2; a CFF one as the bare CID-keyed CFF inside it, FontFile3
`/CIDFontType0C` under a CIDFontType0, with its `fsType` written into the program (below). The command-line tool's replacements carry no
layout, so they use neither Noto nor an installed font; nothing changed there.

**Also fixed on the way.** Automatic mode took *encodes* as *covers*: a simple font's Latin-1
slot encodes whether or not the subset has the glyph, so a WinAnsi TrueType subset lacking a
letter refused the automatic preview with *the font has no validated glyph for this character*
instead of reaching Noto. Coverage now also measures the line.

**Reproduce** (from the repository root; the fonts are original, generated by
`testdata/make_installed_fonts.py`, which `--check` compares byte for byte):

```bash
uv run --with fonttools testdata/make_installed_fonts.py --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib installed
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib sysfont
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
uv run --with pypdf --with fonttools python scripts/text_installed_check.py src-tauri/target/debug/examples/text-edit-probe scratch/installed-check
swift scripts/text_edit_pdfkit.swift scratch/installed-check --installed
python3 scripts/mutate_rust.py --only 'installed font:'
python3 scripts/mutate_frontend.py --only 'installed font:'
# CFF outlines (the check above writes scratch/installed-check/cff as well)
swift scripts/text_edit_pdfkit.swift scratch/installed-check/cff --installed
qpdf --check scratch/installed-check/cff/synthetic-after.pdf
qpdf --check scratch/installed-check/cff/again/edited.pdf
python3 scripts/mutate_rust.py --only 'installed-cff:'
```

**Measured on macOS arm64, 2026-09-30:**

- `fonts::installed::tests`: 18 tests (one macOS-gated: `ArialMT` found by CoreText and
  `Helvetica` found as a face of `Helvetica.ttc`, whose name records are Macintosh Roman only;
  an unknown name answers nothing although CoreText matches a substitute). The Windows-gated
  counterpart (`ArialMT` with a DirectWrite face index) passes on Windows x64 (below).
- `sysfont::tests`: 7 tests, the lookup seam, the bound (32 MiB accepted, one byte more
  refused), and the request round trip against the real writer: asked, supplied once, asked
  again, the subset journalled, and the journalled subset reused with no lookup.
- `text_installed_check.py`: the probe's preview/save pixels agree and adjacent pixels are
  unchanged; 1,620 of the 5,032 font bytes are embedded; Unicode, every saved outline and
  advance, `/W`, `fsType` and the PostScript name agree with the installed font; the
  document's own font program is byte-identical. A copy three font units wide on one glyph of
  the document's subset, and one with the no-subsetting bit, are both set in Noto Sans.
- PDFKit `--installed`: 1,541 changed pixels inside the target, zero outside; text agrees.
- The same round trip with a producer-like subset of this Mac's own Arial (integer widths,
  scratch only, nothing committed) set the new characters in `XXXXXX+ArialMT`.
- Mutations: 33 new Rust (`installed font:`, one macOS-only) and two re-aimed (*CJK fallback:
  omit embedding rights* now names the test that asserts the OS/2 table, see `docs/TRAPS.md`;
  *layout: fall back for a space* follows the new coverage check), 35/35 caught by the named
  test after two survivors were fixed (a second, redundant file-length bound in `sysfont::find`
  removed and the mutation aimed at the bound that remains; a stripped `wants` field asserted
  on a reply that carries one). Frontend: 4/4 (`textlayout.ts`'s explanation of the automatic
  order on its option).
- `scripts/gates.py`: 28/29 on the first run, the fuzz gate red on a stale
  `src-tauri/fuzz/Cargo.lock` (the two new direct dependencies); the lockfile was refreshed
  offline and `fuzz`, `untracked` and `notices` re-run green. 2,366 Rust tests passed with 9
  ignored; 1,976 frontend tests passed.
- `scripts/check_windows.py`: the Windows tree type-checks and passes clippy, examples
  included.

**CFF outlines, measured 2026-09-30.** `subsetter` 0.2.6 subsets CFF (not CFF2) and returns an
OpenType file whose `CFF ` table it has made CID-keyed (`Adobe-Identity-0`), desubroutinized,
with each glyph its own CID and every absolute offset a five-byte integer. Two carriers were
possible for it: the whole OpenType file as FontFile3 `/OpenType`, which keeps the OS/2 table,
or the bare table as FontFile3 `/CIDFontType0C`. The bare table was chosen, and only it was
built: it is PDF 1.3 where `/OpenType` is PDF 1.6 and outside PDF/A-1, it is what xdvipdfmx,
LuaTeX and Typst embed, and the editor already reads it (`fonts/cff/cid.rs`), so a reopened
document is re-edited through existing code rather than a second CFF path. PDFium, PDFKit,
fontTools and poppler all read what was written (below). The cost is the OS/2 `fsType`, which a bare CFF has nowhere to carry:
`fonts/cff/rights.rs` adds Distiller's Top DICT `PostScript` string `/FSType N def
/OrigFontType /OpenType def` and moves every offset by what it adds, and the result is parsed
by `cff::cid::parse` with every charstring width compared against the `/W` about to be written
before anything is. Codes are the subset's glyph ids, so there is no CIDToGIDMap, and two
characters one glyph draws (U+00A0 and the space, in many fonts) cannot both be written: the copy
is refused with *draws two of the characters with one glyph*.

- `fonts::installed::tests`: 28 on macOS (10 new, one macOS-gated new; the old CFF refusal test
  replaced by a CFF2 one). A TrueType document subset set through `cff.otf`, saved, reopened,
  its CID-keyed CFF read back as the document's own font and used with no installed copy, then
  a character in neither subset added through the copy again; `fsType` 8 carried and read back,
  0x2/0x4/0x100/0x200 refused; widths three units off refused; a copy whose `hmtx` and
  charstrings disagree, and one drawn at 2048 units per em, refused before writing; a Type1C
  document subset compared with the CFF and the TrueType build of the font. The macOS test uses
  fonts in the sealed system volume: Hiragino Sans W3 (CID-keyed, Adobe-Japan1, `fsType` 8,
  7.8 MB, a face of a collection CoreText does not name) became a 3,648-byte subset for
  "Tokyo 東京", Kohinoor Devanagari (name-keyed, `fsType` 0) a 2,556-byte one, each installed and
  read back through the composite reader with its widths and rights; Tamil Sangam MN is refused.
  No Windows counterpart: no CFF font is guaranteed on Windows (one present is measured below).
- `text_installed_check.py`, CFF part: 451 of 3,600 font bytes embedded as `/CIDFontType0C`
  under a CIDFontType0 with no CIDToGIDMap; fontTools' CFF parser finds `Adobe-Identity-0`, the
  rights string for `fsType` 0 and for a copy with 8, and every embedded charstring's outline and
  width equal to the installed font's, and `/W` equal to its advances; the saved file edited
  again through the probe, with the embedded program read back and `z` from the copy. The probe's
  own preview/save agreement is PDFium's rendering of each.
- PDFKit `scratch/installed-check/cff --installed`: 1,544 changed pixels inside the target, zero
  outside; text agrees. `qpdf --check` is clean on the three CFF outputs. As a third reader, not
  committed: poppler's `pdftoppm` draws the TrueType and the CFF result with the same glyphs.
- Mutations: 16 new Rust (`installed-cff:`, one macOS-only), 16/16 caught by the named test after
  two findings. A separate check in the writer that two characters did not share a code
  survived: the layout step that follows already refuses such a text, so the check was removed
  and its test kept. And with the fixture's Private DICT empty, as fontTools builds it, the
  mutation that leaves the Private offset unmoved passed the test named for it, caught only by
  the Hiragino test; the fixture now has hint values (`docs/TRAPS.md`, *An empty Private DICT
  reads the same wherever its offset points*). *installed font: embed CFF outlines* became
  *installed-cff: embed CFF2 outlines*. All 48 `installed` mutations were run and caught; so
  were 128 of the 129 aimed at the files this touched (`--since HEAD`), the one survivor, *gaps:
  accept an empty word*, surviving identically on the commit before this one.
- `scripts/gates.py`: 29/29, 2,392 Rust tests passed with 9 ignored, 1,976 frontend tests.
  `scripts/check_windows.py`: the Windows tree type-checks and passes clippy, examples included.
  Neither lookup needed a change, because neither filtered CFF files; the worker did.
  CoreText finding Hiragino and Kohinoor is measured above; DirectWrite finding a CFF file is
  measured on Windows below.

**Not done:** CFF2 installed fonts are refused (*has CFF2 outlines, which cannot be embedded*;
`subsetter` subsets CFF2 only by instancing it to TrueType, behind a feature tpdf does not
enable), and so is a CFF font whose FontMatrix is not 1/1000 (drawn at other than 1000 units per
em, Tamil Sangam MN on macOS), as *could not be read*, because the CID-keyed CFF reader a
reopened document goes through accepts only that matrix. No fuzz seed was
added: the fuzz targets take documents and edit without a layout, so neither reaches
`installed::accept`, whose input is the reader's own font file.

**Windows x64, 2026-09-30, at `072f3ea`.** On MOTHERSHIP (Windows 11), in the console session
at medium integrity through a one-shot `/IT` scheduled task, with `CARGO_BUILD_JOBS=2`:
`cargo test --locked --lib textedit` passes 604 (1,824 filtered out), `--lib sysfont` 7, and
`--lib installed` 27: the macOS 28 less its two macOS-gated tests, plus
`windows_finds_installed_fonts_by_exact_postscript_name`, which ran and passed, so
DirectWrite found `ArialMT` and named its face index. `fetch_pdfium.py --check` verified the
8066 library. The commands above only supply generated fonts, so the lookup itself was run
through the application, which is the only caller of `sysfont::find`. Scratch-only
documents, built on the Mac from copies of the two installed Windows files (digests compared
on both sides, nothing committed), embed producer-like subsets with integer widths: `SYNTHETIC
FIRST` in one subset and `SYNTHETIC SECOND` in another, so `EDITED FIRST` needs a `D` the first
subset lacks. Windows' Arial is version 7.06, `fsType` 8, 1,045,720 bytes (SHA-256 `b3658ead...`).
The native `textedit` phase passes 23/23 on the Arial document, whose subsets are WinAnsi
TrueType fonts named `ABCDEF+ArialMT` and `GHIJKL+ArialMT`. In the saved copy fontTools and pypdf find
one added Type0/Identity-H font, `XGDDDA+ArialMT`, a 23,552-byte FontFile2 subset of the
installed file. Its 8 codes have outlines, advances and `/W` equal to `arial.ttf`, and it
carries `fsType` 8. Both document subsets are byte-identical. PDFKit counts 2,362 changed pixels
inside the target and none outside, and the text agrees. The probe's `--roundtrip`, handed the
same file, passes its preview/save pixel agreement. It writes a font of the same size and tag,
which the same readers accept. The control is the same document with its font renamed
`TPDFNoSuchFontAnywhere`: the phase still passes 23/23, the lookup finds nothing, and the save
is set in `NotoSans`. The readback then fails, and PDFKit counts 2,212 different pixels. `qpdf
--check` is clean on every save.

**CFF on Windows.** No CFF font is guaranteed on Windows. MOTHERSHIP has 18 CFF faces among
486 font files, all from Windows' optional Hebrew supplemental fonts: David, Frank Ruehl,
Miriam and Nachlieli CLM, Frank Ruhl Hofshi and Miriam Libre. All are name-keyed, `fsType` 0 and
not CFF2. Only the four Frank Ruhl Hofshi and Miriam Libre faces are drawn at 1000 units per
em; the fourteen CLM faces are at 1090 or 1200, which the editor refuses as it refuses Tamil
Sangam MN (from the code; none was tried here). `MiriamLibre-Regular.otf` (64,252 bytes, SHA-256 `6c366f3f...`) was exercised the
same way, with its document subsets embedded as FontFile3 `/Type1C`. The native phase passes
23/23. DirectWrite finds the `.otf`, and the save embeds `CCHZVU+MiriamLibre-Regular` as
a 1,440-byte bare CID-keyed CFF (`/CIDFontType0C`, `Adobe-Identity-0`, no CIDToGIDMap). The
program carries `/FSType 0 def /OrigFontType /OpenType def`, and its 8 charstrings' outlines,
widths and `/W` equal the installed file's. PDFKit counts 1,884 changed pixels inside and none
outside. The probe's round trip passes too (1,434 bytes, `FCDPYN+`). Remote work took about 15
minutes; both scheduled tasks and the work folder were removed, `npm run build` restored the normal assets (`check_bundle_share.py`: harness excluded),
and the checkout is clean at `072f3ea`.

**Settled 2026-09-30, as built:** when the document's own font forbids editing, no installed
copy is tried, because the document's restriction decides. An installed file over 32 MiB falls
back to Noto; the fonts that large are mostly CJK, which go to Noto CJK regardless. With two
installed versions of one font, CoreText may pick the one whose widths disagree, and the reader
then sees Noto with that reason. CFF installed fonts were the next increment on this path,
because Adobe's Minion Pro and Myriad Pro families, common in the scanned documents, are CFF;
they are embedded since the same day (*CFF outlines*, below). Neither family is installed on the
Mac this was measured on, so neither has been tried. Also settled: an installed CFF font is
embedded as bare `FontFile3 /CIDFontType0C`, not `/OpenType`, which keeps PDF 1.3 and PDF/A-1
readers and lets re-editing use the existing CID-keyed CFF reader, at the cost of writing the
rights into the program's Top DICT. A replacement that needs two characters one glyph draws
(a space and a no-break space, say) falls back to Noto rather than splitting across two fonts.

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

### Documents the editor offers nothing in — measured 2026-09-26

The +25% acceptance rate the sections above report counts only text the editor offers. Counted
per document it hid the larger gap: **12 of the 31 files offered no editable text at all**, among
them the letter, all three invoices, both brochures and both IRS forms, and 174 of 803 pages were
refused outright. `scripts/textedit_growth.py` now prints that first (*Editable documents: N of
31*, the pages, and every page refusal by reason), and `--compare` counts text that becomes
offered, rather than stopping at records that tried different runs; a run no longer offered is a
regression whatever its verdict was.

**Most refused pages were refused over one font.** 120 of the 174 named a font: a program
the editor does not validate, a character map it cannot read, or embedding rights that forbid
editing. Any one such font refused the whole page. Now a simple font with `Widths` and a bounded
`FontBBox` is measured by those alone (`fonts::read_only`): every code opaque, its ink held to the
box, the program never read. Its text is kept byte for byte and the rest of the page edits; a page
left with nothing to edit is refused with the font's own reason. Symbol and ZapfDingbats named
without a program, ReportLab's bullets, are measured from generated Adobe widths
(`fonts::symbolic`).

**The tagged output of InDesign and PowerPoint, one shape at a time.** A temporary `eprintln!` at
each of `tagging.rs`'s generic refusals, replayed on the InDesign letter and the PowerPoint fact
sheet, named each blocker in turn: ActualText on a structure element (now pins, as Alt does), a
language beside the MCID, a tab spacer showing one space for several tabs, an empty spacer or an
empty paragraph between text objects, and a Span inside a Figure (pinned). Each is kept rather
than refused, and what each describes stays read-only. The letter is then refused for artifact
properties the editor does not read (corrected 2026-09-26: this said its font's licence, which the
records of this very run do not show), and the fact sheet for text boxes inside table cells;
neither is changed here.

**Before and after, the 31 files:** editable pages 629 → **667**, runs offered 44,282 → 46,449,
documents with editable text 19 → 20, and over every trial `--compare` found no verdict
accepted before and refused now. The twentieth document is the IRS W-4, and only for one `▲`
glyph: its words are all in a font whose licence forbids editing. At +25% as typed, 36,862 edits
are accepted, where 35,117 were.

**Checked by hand:** twelve newly offered edits, in the Healdsburg slides, the research paper
and the ReportLab guide, through `--roundtrip` and `qpdf --check`, all passing; six renders
looked at, each changing only the edited text, which the crop to every changed pixel shows.

**What is left, and why.** 48 pages are text wholly in a licence-restricted CFF font (the
Highmark brochure, the W-9, the W-4): editable only by setting new text in another font, the
bundled Noto, which is a decision rather than a fix (taken the same day, next section). The Canada Post invoice's table
cells name a header no element carries, and stays refused as the broken structure it is. The
rest are single documents: the passport guidance's Unicode map, the fact sheet's cells, the
Latin-1 limit.

### Replacing text set in a font that forbids editing — measured 2026-09-26

The decision the previous section left open was taken: text in a font whose embedding rights
forbid editing may be replaced, with the replacement set in the bundled Noto Sans, and the editor
says so. The font is now read like any other, so its runs are offered, and `Metrics::restricted`
keeps new text out of it: `encode` refuses anything but an empty show. Automatic layout, which
the editor always sends, then chooses Noto by the font's name (bold, italic), and the preview line
reads *Noto Sans (the document's font does not permit editing)*. Asking for the original font is
refused with the two choices that work; an edit with no layout is refused with the licence
reason; deleting the text writes nothing in the font and goes ahead. Text not edited stays in the
original font byte for byte. The rights test is one function, `fonts::restricts`, used by the
OS/2 `fsType`, Type 1 `FSType` and CFF `/FSType` readers alike. A CID-keyed CFF program with such
rights is still refused (its text read-only), because nothing in the sample needs it.

**Before and after, the 31 files:** documents with editable text 20 → **22** (the Highmark
brochure and the W-9), pages 667 → **680**, runs offered 46,449 → 47,435, and at +25% as typed
36,862 → **37,727** accepted. `--compare` found no verdict accepted before and refused now, 3
refused before and accepted now, and 13,706 verdicts newly offered, 7,637 of them accepted. The
W-4 went from one offered `▲` to 822 runs.

**Fourteen unchanged edits are refused, and that is expected.** Retyping a run's own text in a
restricted font sets it in Noto, which is wider, and on 14 lines of the W-4 and the brochure it
does not fit. The editor submits nothing for unchanged text, so no reader meets this; it shows
only in the probe's identity trial.

**The next refusal in those documents** is not the font: 31 of the brochure's pages stop at
*unsupported CFF glyph name*, and the IRS forms at *text contains an unmapped font code* (6
pages). The letter was never refused for its font: its one page stops at *unsupported artifact
properties*, as the previous section now says.

**Checked by hand:** nine newly offered edits, three each in the W-9, the W-4 and the brochure,
through `--roundtrip` and `qpdf --check`, all passing, and all nine renders looked at. Five are
right: the replacement is Noto Sans at the run's size and colour and nothing else moved. One of
them shows the style rule's limit: a run in PlantinMTPro-Semibold is set in Noto Sans Bold,
because the style is read from the font's name and *Semibold* contains *bold*.

**Three exposed older defects in the push, not this change.** Page 6 of the W-9 and page 2 of the
W-4 are tagged and set in two columns.

- In two W-9 edits, growing a line in the left column pushed the level line of the right-hand
  column along by the same distance. The column exemption of *Two columns on pages without tags*
  applies only to untagged pages, so on a tagged page every run level with the line is pushed.
- In the same two, the edited run's neighbour lost the space before it (*emails CHANGEDwebsites*).
  The push starts only once the text passes the next run's near edge (`from` in `free_width`), so
  the gap between them is used up first, whatever the page.
- In the W-4, a line grew across the gutter until it touched the next column's text, for the
  same reason as the first: on a tagged page nothing ends a line at the next column.

All three are 26.9.19 behaviour, now reachable in documents that were refused outright before.
They are fixed in the next section.

### Columns on tagged pages, the gutter, and the space before a pushed word — measured 2026-09-26

The three push defects the previous section found, fixed together, since two share a cause.

- **The other column was pushed.** `free_width` asked `column_runs` only on pages without
  tags. A tagged page's blocks are its structure elements, which tell a column from a row as
  well as the geometry does, so the question is now asked on every page: a column's lines are
  never pushed and are where a line ends. The wrap's level check (`wrap_room`) stays untagged
  only, because there a tagged page's structure already says which lines belong together.
- **A line filled the gutter.** A column's text was the limit, so a line could grow across
  the gutter until it touched the next column: the W-4. `layout::gutter` puts a limit in the
  gutter, measured from the widest line on this side of the column that is level with it (a
  heading above both columns does not count), and a line may take `GUTTER_SHARE` of it.
- **The pushed word lost its space.** The push started only once the text reached the next
  run's near edge, so the gap between them was spent first and the neighbour followed flush:
  *emails CHANGEDwebsites*. It now keeps the smaller of the gap it had and one word space (the
  run's own word gap, or a quarter em). A wider gap, a tab stop or the next cell, is still spent
  down to that space. `docs/PLAN.md` §7 records the changed rule.

**How much of the gutter, measured rather than chosen.** On the 31 files at +25% as typed:

| rule | accepted |
|---|---:|
| before this section (lines ran into the next column) | 37,727 |
| none of the gutter | 36,283 |
| **half of the gutter** | **37,333** |

With none of it, justified two-column papers (the arXiv pair, LuaTeX, fontspec) have no line
short of the measure, so every growing line has to wrap, and many of those wraps are refused
for something else (*part of it below cannot be moved*, *out of line with the text beside
them*). Half keeps 1,050 of those 1,444 edits and still leaves space between the columns; two
of them were rendered: an arXiv line running a few points past its column's edge with the gutter
still clear, and a figure label *C* made *CC*, which touches its circle's outline: the box grows over a
drawing by design (`obstacles`), so that is not the gutter's doing. The remaining 394 are lines that would take more than half
the gutter, which is the defect.

**A zero push at the column no longer shrinks the wrap.** With the gutter in the way, a run
already at its column's measure has no push left; returning *nothing* there handed the wrap the
room before that run as its measure, and a paragraph came back two words a line. A column stop
now keeps the whole line for the wrap.

**Checked by hand:** the nine edits of the previous section again, all through `--roundtrip` and
`qpdf --check`, and the three that were damaged rendered: the W-9's *a CHANGED falsely* keeps
its space and the right-hand column does not move, and the W-4's *Tax Withholding CHANGED*
wraps inside its column. The wrap breaks only the edited line, so *established* sits alone on a
line and the paragraph below it moves down one: correct, not typeset.

**Tests:** `wrap_tests` gained a tagged two-column page (`tagged_columns_at`), with the column
at a gutter and flush, a line stopping in the gutter, a push into its allowed half and past it;
the first push test gained a gap narrower than a space. Seventeen push tests were re-measured
for the kept space, each by hand before the run. Two wrap fixtures split a three-line paragraph,
which is now a column beside a one-character label and so no longer pushed by it. Ten mutations
were added under `columns:`, `gutter:` and `push:`; two older `columns:` ones were re-aimed at
the flush-column case, where the column and not the gutter stops the push, and *push: measure
the move from the room instead* at the new `from`.

### A letter outside Latin in a CFF font's encoding — measured 2026-09-26

The Highmark brochure prints a Russian edition, and every one of its Type1C fonts names the
Cyrillic letters in its `/Differences` (`uni0410` to `uni044F`). The WinAnsi CFF path writes only
the Latin names it lists, and refused the font for any other one, *unsupported CFF glyph name*,
which refused the page: 31 of the brochure's 52. Such a font is now read by glyph name, the Type 1
rules `type1::compact` already applies to symbolic and unencoded Type1C fonts, where a letter the
editor cannot write measures read-only text and the rest stays editable (`docs/TEXTEDIT.md`). The
route is taken only on that one refusal (`cff::UNNAMED`), so no font the WinAnsi path accepted or
refused for anything else changes.

**Before and after, the 31 files,** against the 26.9.20 records: pages with editable text 680 →
**711**, runs offered 47,435 → 48,292, and at +25% as typed 37,333 → **38,047** accepted.
`--compare` found 640,308 verdicts unchanged in kind, none moved either way, and 11,780 newly
offered, 7,003 of them accepted. Every one of the brochure's 31 pages opened; its remaining nine
refusals are other causes (ActualText, painted paths, `MP`, a TrueType program).

**Checked by hand:** two accepted +25% edits on newly opened pages (10 and 38) through
`--roundtrip`, both passing, and page 10 rendered: the grown heading is set in Noto Sans, since
the brochure's fonts forbid editing, and nothing else on the page moved.

**Tests:** `textedit_cff_letter_outside_latin_keeps_its_text_read_only_and_the_page_editable`,
over a new synthetic program with one Cyrillic glyph (`fixtures/cyrillic.cff`, from
`testdata/make_textedit_cff.py`, which reproduces the other fourteen byte for byte): the WinAnsi
path still refuses the font, the glyph-name path reads its Latin and marks the Cyrillic opaque,
the Latin run is edited, the Cyrillic run is kept byte for byte, and a font refused for widths
that disagree stays refused. Two mutations under `dispatch:`, both caught. `textedit_growth.py`
now counts the licence refusal as `restricted` rather than unclassified; only its `patch` mode
meets it.

### Half a paragraph break when the page is full — measured 2026-09-26

The decision *Spreading the added lines* left open was taken: of keeping the refusal, halving the
breaks, and letting text move into the bottom margin, **a break may give up half its blank
line**. A wrap is laid out as before, every break kept whole; only when that is refused as
*its lines would move onto what is below it* is it laid out again with each break allowed to
give up `BREAK_GIVE` (half) of a pitch (`layout.rs`, the `settle` closure). The allowance is
one number in `landing`'s answer, so `lands` (may a moved line come this close) and `cascade`
(how far each block below moves) read the same rule. An edit accepted before is laid out
exactly as it was, and a break given up is the nearest first: the cascade takes each break's
spare in page order.

**Before and after, the 31 files,** against the records of the previous section: accepted as
typed at +10/+25/+50% went 41,865/38,047/35,013 → **42,066/38,905/36,168**. `--compare` found
2,219 verdicts refused before and accepted now and none the other way. *Onto what is below it*
at +25% went from 2,213 to 817, and Coatesville's 770 are gone: its breaks are exactly one blank
line, which is why *Spreading the added lines* could not use them. What is left is mostly the
Arcadia agenda (394), whose breaks sit above text the wrap cannot move.

**Checked by hand:** four newly accepted +25% edits, two each in Coatesville and Arcadia,
through `--roundtrip`, all passing, and all four rendered: the breaks below the edit are
narrower and still read as breaks, and no line touches another.

**Tests:** `a_full_page_takes_a_wrap_in_half_of_two_paragraph_breaks` (two breaks of one blank
line each give 7 pt apiece to a 14 pt line; 1 pt less and the last paragraph would leave the
page). Two tests that asserted a refusal next to text that cannot move now assert the edit is
taken by half the break, and a refusal where even half is not enough. The retry can rescue a
defect in the whole-break rule, and did for three older mutations (*a blank line is a whole
pitch*, *the bottom has no height*, *a thousandth of a point is a move*); their tests gained a
case at exactly half a break, or with nothing below to refuse the move, and catch them again.
Five mutations under `half breaks:`; two older ones re-aimed at `landing` and `cascade`, where
the blank line is now computed. All 167 mutations of the wrap path (wrap, columns, flow,
cascade, cut, beneath, gutter, room) are caught.

### Underlines move with their lines — measured 2026-09-26

After *Half a paragraph break when the page is full*, the largest refusal left at +25% that the
editor can do something about was *a drawing or an annotation is placed over the lines that
would move*: 1,087, of which Arcadia 692 and Coatesville 113. Measured before building: a
temporary log of the first drawing in the way, on the six files with the most of them and over
every trial, found 2,668 refusals, **1,809 of them a thin horizontal path inside a moving line's
box** -- an underline, which Word draws as a filled rectangle of its own under the words --, 296
more thin rules not inside any single run, 382 annotations (arXiv, LuaTeX) and 181 boxes.

The scan now remembers, for each painted rectangle or path outside a text object, its operators
from the first construction operator to the painting one and the transform they are drawn under
(`Inspection::paths`). A wrap moves such a path with a moving line when it is no deeper across
the lines than a quarter of the line (`UNDERLINE`) and lies within the line's hit rectangles to
`LINK_SLACK`, a line being the moved runs that go the same distance and share their height, so a
rule under several runs goes with all of them (`layout::underlines`). It is written under a
translation, `q 1 0 0 1 dx dy cm` before its first operator and `Q` after its last, both kept
byte for byte (`wrap::translated`); the rewrite accepts exactly that bracket around an unchanged
path operator and nothing else (`streams::rewrite_expanded`). A path cannot contain a clip, so
the bracket changes nothing after it. An underline under text the wrap cuts into pieces that go
different distances is not moved, and still refuses; so do an image in the same place, a box as
deep as a highlight, and a rule running past the line.

**Before and after, the 31 files,** against the records of the previous section: accepted as
typed at +10/+25/+50% went 42,066/38,905/36,168 → **42,095/39,683/37,351**. `--compare`
found 1,990 verdicts refused before and accepted now and none the other way. The drawing
refusal at +25% went from 1,087 to 309; what is left is mostly annotations in LuaTeX and the
arXiv papers (a citation link not over one moved run) and Arcadia's boxes.

**Checked by hand:** nine newly accepted +25% edits, five in Arcadia and two each in Coatesville
and Hugo, all through `--roundtrip` and six through `qpdf --check`, all passing, and three
rendered: Arcadia's *Cyber Liability Insurance.*, a two-line underlined heading in Hugo and a
justified one in Coatesville each move down with their underline under the same words.

**Tests:** `an_underline_moves_with_its_line`: the underline moves 14 pt with its line on a
tagged page and on one without tags, and with two runs of one line; the deep box, the rule past
the line, the underline under cut text and the image still refuse. Ten mutations under
`underline:`, all caught; two older `wrap room:` ones re-aimed at the reshaped loops.

### Three readers brought back to the writer — measured 2026-09-30

The Windows verification of 2026-09-30 (*Word spacing and the next practical target*) found
three independent readers rejecting correct output on both platforms. They had gone stale
behind the writer, not the other way round; each is now fixed and still refuses what its
section says it refuses.

- **`text-edit-probe`, default mode.** Its last step expected the refusal wording from
  before `59d25b6` (2026-09-17), which added WinAnsi punctuation. It now expects
  *printable Latin-1, WinAnsi punctuation and minus only*. Stale from `59d25b6`.
- **`make_textedit_embedded.py --check`.** It required the edited page to keep its operator
  count and a replaced `TJ` to be a single string. Both stopped being true of correct output:
  `ec5a696` (2026-09-18) keeps the source's own `TJ` items around the changed middle
  (`kerning.rs`), which broke worker saves of a `TJ`; and the application has sent every
  edit with a layout since 26.9.9 (`812601d`, 2026-09-16), whose save restates
  `Tf Tc Tw Tm` around the new show, replays the source's line operators and ends with
  `TJ [() n]` to put the cursor back (its present form is from `b5a47f4` and `eab6dee`,
  2026-09-19), which broke every native save. The dates come from the history, not from
  re-running each commit. The reader now pairs each
  edited show through a diff of the two operator lists and accepts exactly two shapes: shows
  replaced one for one, or one show replaced by that layout sequence. Every restated state
  must equal the source's at that point, the replayed line matrix must equal the source's,
  and the cursor number must equal the source show's own advance (measured from the font's
  `Widths`, `W` or, for a standard font, Adobe's metrics) to 1e-4 text units. A number left
  in a replacement `TJ` must be one the source array had, in its order. It also decodes both
  edited operands exactly through a simple font's own encoding: the page-text comparison it
  had folds whitespace, so an added trailing space passed it, which the new controls found.
- **`text_edit_pdfkit.swift --wide-spacing`.** `FIRST` now lands at 235.504 pt, not 235.744:
  the space before it is unchanged, so the source's 20/1000 em (0.24 pt at 12 pt) between
  them is kept. Stale from `ec5a696` for the worker save, and for the native one since the
  layout default.

`--layout-controls before.pdf after.pdf [--check options]` damages a copy of a passing output
seven ways and requires a refusal for the named reason each time: the input left unedited, a
space added to the replacement, a font resource changed, a restated `Tc` differing from the
source, the cursor restoration one unit short, a kerning number the source never had, and a
painting operator inside the edited region. A control that cannot apply to an output (a byte
patch has no restoration) prints `[SKIP]` with its name rather than passing.

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
src-tauri/target/debug/examples/text-edit-probe scratch/readers/default
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --layout-controls <dir>/synthetic-before.pdf <dir>/synthetic-after.pdf [--cff-unicode|--cff-ligatures|--dash|--w3c-dummy|--passport --page=15]
swift scripts/text_edit_pdfkit.swift <dir> --wide-spacing
```

Measured on macOS against the 22 outputs retrieved from the Windows run (11 worker, 11
native; the macOS worker's 11 are byte-identical to them, and so is a fresh macOS native
`textedit-wide-spacing` save, 23/23): the default probe and all 11 fixture probes pass; the
pypdf reader accepts all 22, and its controls refuse every applicable damage (7 of 7 on the
four symbolic-TrueType native saves, 6 on the other native saves, which have no kept number,
4 or 5 on worker saves) with the named reason; the W3C worker save passes with 4 refusals.
PDFKit places `FIRST` correctly in both `--wide-spacing` saves and refuses an unedited copy
(`wide word gap has the wrong position for after`).

**What relied on them.** Between 26.9.9 and this fix no section of this file cites a pass
by any of the three. The 26.9.9 release notes report a packaged fallback-font save *with
independent parser readback* without naming the reader; `--check` refuses any save that adds
a font resource, so it cannot have been this one. The sections of 2026-09-17 to 2026-09-29 verify through
`--roundtrip`, `text_wrap_check.py`, `qpdf --check` and `pdftotext`, and the Windows section
of 2026-09-30 already recorded all three as failing and substituted a structural readback.
Its claims that *the pypdf reader accepts the worker save* (remapped and Unicode CFF, stroke
styles, ligatures, named maps) were true when written, because those worker saves are byte
patches of `Tj` strings with no kept number; they were not re-run here beyond the 22 outputs
above. The earlier sections, before `59d25b6`, were measured against the writer of their day
and are not claims about the current one.

### PowerPoint factsheet: an unchanged practical page — measured 2026-09-30

The EC consumer conditions factsheet for Lithuania (`consumer-factsheet.pdf` in
`testdata/textedit-public-corpus.json`, SHA-256 `cbd18ff8...a53b`) is a Microsoft PowerPoint for
Microsoft 365 export, tagged. It had 0 of 6 editable pages. Its first refusal was a tagged
structure, and as *Public target follow-up* warned, the first refusal understated the work: the
whole inventory was found by walking one refusal at a time with scratch-only bypasses, never
by rewriting the source, and it is eight constructs. Each is now admitted, with a synthetic test,
`pptx:` mutations and the grammar in `docs/TEXTEDIT.md`, *PowerPoint for Microsoft 365*:

| Construct | Where it refused | Now |
|---|---|---|
| `TD > Textbox > P` (RoleMap `Textbox` to `Sect`) | a container inside a cell | the bare wrapper is lifted into the cell, one level |
| `Link > Span > MCID`, and an `OBJR` that is an indirect object | an element inside a link | the Span is the link's pinned words; the OBJR is resolved |
| a layout `/BBox` with its top first (the mouse guide) | inverted corners | either pair of opposite corners (ISO 32000-1 7.9.5) |
| `m l l l W* n`, corners off the axis by up to 0.0002 pt | an open, skewed clip | closed by the clip; within 0.001 it is its inner rectangle |
| a type 0 sampled function in every gradient | a stream where a dictionary was expected | checked, never evaluated |
| `/Matte [0 0 0]` on every picture's soft mask | an unknown image key | one number in `[0, 1]` per owner colour component |
| ActualText on all 305 text Spans, 292 in paragraphs and 13 in links (137 equal to the words, 168 equal but for an end space, none different) | every run pinned read-only | rewritten with the words; any other Span pinned as before |
| a turned square clipping an icon, page 3 | a non-rectangular clip | its text read-only; the scope holds an image and no text |

Two of these carry a decision worth knowing. The turned clip keeps text under it read-only
rather than testing containment against the polygon, because measuring first showed no text in
that scope. The ActualText rule strips only U+0020 at either end, and it has to admit the empty
text a deletion leaves: the `textedit_scan` fuzz target found that on the new seed's first
execution (`docs/TRAPS.md`, *A text's own ActualText rule has to admit the empty text a deletion
leaves*).

Reproduce, with the public PDFs downloaded and digest-checked by the recipe in *Public-document
text editing baseline*, and the checks application built as in *Existing-text workflow*:

```sh
cargo build --locked --manifest-path src-tauri/Cargo.toml --example text-edit-probe
src-tauri/target/debug/examples/text-edit-probe --inspect scratch/textedit-public/consumer-factsheet.pdf --all-pages
python3 scripts/tabs_check.py <checks-binary> scratch/textedit-public/consumer-factsheet.pdf --phase textedit-factsheet --saved-copy scratch/factsheet/cell/synthetic-after.pdf
python3 scripts/tabs_check.py <checks-binary> scratch/textedit-public/consumer-factsheet.pdf --phase textedit-factsheet-body --saved-copy scratch/factsheet/body/synthetic-after.pdf
# with the unchanged download copied in as <dir>/synthetic-before.pdf:
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --layout-controls scratch/factsheet/cell/synthetic-before.pdf scratch/factsheet/cell/synthetic-after.pdf --factsheet --page=1
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --layout-controls scratch/factsheet/body/synthetic-before.pdf scratch/factsheet/body/synthetic-after.pdf --factsheet-body --page=1
swift scripts/text_edit_pdfkit.swift scratch/factsheet/cell --factsheet --page=1
swift scripts/text_edit_pdfkit.swift scratch/factsheet/body --factsheet-body --page=1
# the same shapes on one original page:
uv run --with fonttools --with pypdf testdata/make_textedit_pptx.py scratch/pptx
src-tauri/target/debug/examples/text-edit-probe scratch/pptx/worker scratch/pptx/pptx.pdf
uv run --with fonttools --with pypdf testdata/make_textedit_embedded.py --layout-controls scratch/pptx/worker/synthetic-before.pdf scratch/pptx/worker/synthetic-after.pdf --pptx
swift scripts/text_edit_pdfkit.swift scratch/pptx/worker --pptx
python3 scripts/tabs_check.py <checks-binary> scratch/pptx/pptx.pdf --phase textedit --saved-copy <dir>/synthetic-after.pdf
python3 scripts/mutate_rust.py --only 'pptx:'
```

**Measured on macOS.** The worker discovers 97, 35, 33, 42, 32 and 53 runs on the six pages.
On page 2 the native application replaces `Action taken` in a table cell (`TD > Textbox > P >
Span`) with `Action done`, and in a second run `Detailed results` in the page header with
`Detailed data`: 27/27 checks each, run twice, the second time on the final code and byte-identical, including undo and redo pixels, the refused overflow,
save, reopen, page 1 unchanged and the other tab untouched. In both saved files pypdf finds one
show replaced by the layout sequence with every restated state equal to the source's, the kept
kerning numbers the source's own, the cursor restored to the source show's advance, the operand
decoding exactly to the new words, every page's resources equal, the other five pages'
content byte-identical, and exactly one structure element changed: the edited Span, whose
ActualText went from the old words to the new. Nine controls are refused for their named
reasons each, among them the old ActualText put back. PDFKit finds 695 and 781 changed pixels
inside the edited words, none outside them and none on the other five pages, and reads the
page's text with only the words replaced. A worker round trip of both edits in one save passes
its preview/save pixel agreement and `qpdf --check`, and both Spans read the new words.

The synthetic page passes the worker probe and the native `textedit` phase 23/23. pypdf accepts
both saves, with 7 controls refused on the worker's byte patch and 8 on the native layout save
(the rest do not apply: a byte patch restates nothing, and the file has one page), and PDFKit
finds 968 changed pixels inside the edit and none outside, for both.

The survey of the unchanged downloads, before and after (`python3 scripts/textedit_survey.py`,
the same eight files; the Adobe letter could not be downloaded, timeouts, and was not measured):

| Document | Before | After | First refusal left |
|---|---:|---:|---|
| Passport guide | 1/16 | 1/16 | ActualText sequence, inline spacing, Unicode map |
| Consumer factsheet | 0/6 | **6/6** | none |
| Mouse guide | 0/2 | 0/2 | text operator count (was: tagged structure) |
| W-9 | 1/6 | 1/6 | marked content against its tag, unmapped font code |
| Research paper | 12/15 | 12/15 | preserved Form XObject |
| Wellington agenda | 2/2 | 2/2 | none |
| W3C dummy | 1/1 | 1/1 | none |
| W3C headers | 4/5 | 4/5 | page with only read-only text |
| **All eight** | **21/53** | **27/53** | |

The six practical documents among them went from 16 to 22 editable pages of 47.

**The mouse guide** passes its tagged structure now (the reversed BBox), and stops at its content:
27,313 and 39,479 operators on its two pages against the 16,384 the scan bounds, and past that
12 `sh` shadings and `BX`/`EX` compatibility sections. The operator bound is a performance
policy (*Existing-text editing*: 1 MiB, 16,384 operators) and was not raised; whether an InDesign
page of 40,000 operators is worth the scan cost is a decision to make on a measurement of what
the scan costs at that size, not a side effect of this increment.

**Windows x64, 2026-09-30, at `072f3ea`.** The downloaded factsheet's digest matched the manifest
on both sides. `text-edit-probe --inspect --all-pages` finds all six pages editable with 97, 35,
33, 42, 32 and 53 runs, as on macOS. In the console session through a one-shot `/IT` scheduled
task, `textedit-factsheet` and `textedit-factsheet-body` pass 27/27 each. The worker-exit
observer passed its self-test and found no surviving test workers. On both saves
`make_textedit_embedded.py --layout-controls` accepts the tagged structure graph, the rewritten
ActualText and the operands, and refuses all nine controls. PDFKit counts 695 and 781 changed
pixels inside the edited words, none outside and none on the other five pages, the macOS counts
exactly. The synthetic page passes the worker probe and the native `textedit` phase 23/23. pypdf
accepts both saves with 7 and 8 controls refused, and PDFKit counts 968 changed pixels inside
and none outside for both. `qpdf --check` is clean on all four saves.

**Not done.** Text under a turned clip is read-only, never
editable. A Span whose ActualText spans several runs, differs from its words by more than end
spaces, or sits over an inline ActualText span stays read-only. A replacement that wraps inside
a rewritten Span is refused. The mouse guide's operator bound is the user's call. The full
mutation table was not run; the run below covered every mutation in the files these commits
touched.

**Mutations.** `python3 scripts/mutate_rust.py --only 'pptx:'`: 61 mutations, each caught by the
test named for it. Four survived their first run and each was a test that could not fail, fixed
in the test: a nested text box refused by the parent check inside `element` rather than by the
one-level rule (now the only mechanism), turned-clip refusals that passed because a page left
with only read-only text is refused too (a line after the scope now separates them), a range
bound only reachable under a scaling CTM, and a sample-width case refused for its length first.
`--since 195286e` then ran all 459 mutations in the files these commits touched, after 15 of
them were re-aimed at code the increment reshaped: all 459 caught. One old mutation was removed
because the check it broke was removed on purpose (*bounded attributes: accept an inverted
box*), and one of the new ones because it duplicated a re-aimed one.

**Fuzz.** `uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20`, with the new
`editable-pptx` seed: the first run failed on that seed's first execution (the deletion finding
above); after the fix, 24,186 executions from 131 at `INITED`, 100 MB peak RSS, no finding
(`--sanitizer=none` on macOS, so not an AddressSanitizer result).

### Passport guide: a list bullet mapped to a control character — measured 2026-10-01

HM Passport Office's application guidance (`passport-guidance.pdf` in
`testdata/textedit-public-corpus.json`, SHA-256 `0c70c5f7...4f21`, an InDesign export) had 1 of
16 editable pages. Twelve of the fifteen refused pages reported the same first refusal,
*unsupported or ambiguous Unicode character map*, and it was one glyph: every list item on
pages 3 to 14 begins with a Wingdings bullet (`/C2_0 1 Tf <0079> Tj`, a Type0 Identity-H
TrueType font holding that single glyph) whose ToUnicode entry is `<0079> <009F>`, a C1 control
character. `mapping::unicode_codes` refused any control target.

A scratch build with that one check removed made ten of the twelve pages editable, which is
what settled the rule before it was written: the map is admitted, and the glyph is read-only
(`docs/TEXTEDIT.md`). The run counts say only the bullets were withdrawn: against the scratch
build, each page has exactly as many fewer runs as it has bullets (page 3, 170 and 160 with
ten bullets; page 9, 216 and 159 with fifty-seven).

| Passport guide page | Before | Now |
|---|---|---|
| 1, 13, 15 | unsupported ActualText marked-content sequence | the same |
| 2 | unsupported inline spacing sequence | the same |
| 3--7, 9--12, 14 | unsupported or ambiguous Unicode character map | editable, 133 to 191 runs |
| 8 | unsupported or ambiguous Unicode character map | unsupported external text graphics state |
| 16 | editable, 37 runs | the same |

The five public documents go from 20 to 30 editable pages of 45, and the six practical
documents from 22 to 32 of 47. The others are unchanged: factsheet 6/6, mouse guide 0/2,
research paper 12/15, W-9 1/6.

**One unchanged page, through the path a reader's edit takes.** `tpdf-cli text-runs --page 3`
lists *The Child box is for under 16s.*, the first bulleted line; `tpdf-cli edit` replaces it
with *The Child box is for under 18s.* and saves. `qpdf --check` finds no errors, `pdftotext`
reads the new sentence on page 3, and Poppler renders of all sixteen pages at 100 dpi differ
in 52 pixels, every one inside the edited run's rectangle; the bullets and the other fifteen
pages are pixel-identical. The source's digest is unchanged.

```sh
src-tauri/target/debug/tpdf-cli text-runs scratch/textedit-public/passport-guidance.pdf --page 3 --json
src-tauri/target/debug/tpdf-cli edit scratch/textedit-public/passport-guidance.pdf --plan plan.json -o edited.pdf
```

**Mutations.** `python3 scripts/mutate_rust.py --only 'control target'`: three, each caught by
the test named for it. *unicode: a zero-width mark is written* was re-aimed, since the line it
edits gained a term, and is still caught.

**Fuzz.** `uv run src-tauri/fuzz/run.py --target textedit_scan --seconds 20`: 22,973 executions
from 14,314 at `INITED`, 95 MB peak RSS, no finding (`--sanitizer=none` on macOS).

**Not done.** The five pages still refused, each by a construct of its own. A native-window
edit of the page, PDFKit readback and a Windows run were not made (`scripts/check_windows.py`
compiles it); the full mutation table was not run. The Wellington agenda was not downloaded
for this run, so its 2/2 in the six-document total is the earlier measurement.

### Wider public sample, and the IRS forms' punctuation — measured 2026-10-01

**The sample.** Every file in `testdata/textedit-public-corpus.json` that could be downloaded
with its recorded digest and fits the survey's 128-page bound: 26 documents, 416 pages, of
which 21 carry no text to edit. Left out: three documents past the bound (LuaTeX manual 328
pages, ReportLab guide 135, Union County budget 145), and three whose download failed or no
longer matches its digest (Wellington agenda, Adobe letter, the Wikipedia PDF). It is the
recipe under *Reproduce the public sample*, looped over every list in the manifest, with
`qpdf --show-npages` to apply the bound.

| Code | Editable | Refused |
|---|---:|---:|
| `33c7cbd`, before the bullet rule above | 335 | 60 |
| `25e4429`, with it | 345 | 50 |
| with the punctuation below | 350 | 45 |

The bullet rule moved the passport guide and nothing else, so InDesign's control-character
bullet is not a general pattern in this sample.

**The forms.** *text contains an unmapped font code* was the first refusal on four W-9 pages,
two W-4 pages and the Harvest invoice. With the refusal temporarily naming the code, the IRS
pages gave 147, 149 and 151: WinAnsi's left curly double quote, bullet and em dash, set in
Helvetica Neue CFF subsets with `/WinAnsiEncoding` and no ToUnicode map. The Type 1 path names
all of WinAnsi; the CFF path named four characters beyond ASCII. It now names those three and
the right double quote (`docs/TEXTEDIT.md`).

| Page | Before | Now |
|---|---|---|
| W-9 pages 2–5 | text contains an unmapped font code | editable, 190 to 217 runs |
| W-4 pages 3–4 | text contains an unmapped font code | unsupported image on an editable page |
| Passport guide page 2 | unsupported inline spacing sequence | editable, 143 runs |
| Harvest invoice | text contains an unmapped font code (code 105) | the same |

The five-document sample is 35 of 45 editable (W-9 5/6, passport guide 12/16), and the six
practical documents 37 of 47, the agenda's 2/2 being the earlier measurement.

**One unchanged W-9 page.** Its fonts declare `/FSType 4`, so a replacement in the original
font is refused, as before, and automatic mode sets the new text in Noto Sans. On page 2 the
line *• Form 1099-INT (interest earned or paid).* was replaced with *• Form 1099-INT
(interest paid).* through `textedit::write` with an automatic layout, from a scratch test that
is not in the tree. `qpdf --check` finds no errors, `pdftotext` reads the new line,
`pdffonts` lists the four original subsets and one added NotoSans, and Poppler renders of the
six pages at 100 dpi differ in 1,006 pixels on page 2, inside the run's rectangle to within
one pixel; the other five pages are identical. The source's digest is unchanged. `tpdf-cli
edit` cannot make this edit: it sends no layout, so it asks for the original font and is
refused with *embedded font does not permit this editable use*.

**What is left.** 45 refused pages under 18 first refusals, none above seven pages:
ActualText marked content (7, three producers), a page with only read-only text (7), a Type3
outline font (4), and nothing else above three.

**Tests and mutations.** `punctuation.cff` is a new synthetic program from
`make_textedit_cff.py`, whose em dash reaches past its advance so a test can tell which outline
a code selected; the other fifteen fixtures regenerate byte for byte.
`python3 scripts/mutate_rust.py --only 'CFF punctuation'`: four. One survived its first run,
*name the bullet at the em dash code*, because every synthetic glyph had the same shape; the
distinct em dash is the fix. *CFF Unicode: extend unverified TrueType mappings* was re-aimed at
the reformatted line and is still caught.

**Not done.** PDFKit readback, a Windows run, the full mutation table. (The W-9 edit in the
application window is under *The W-9 and a centred title in the application window*.) The W-4's image refusal and the Harvest invoice's code 105 were not looked at.

### A fallback font from the command line — measured 2026-10-01

`replace_text` in an edit plan takes an optional `font`, one of the editor's eight choices.
With it the command builds the box the editor opens on that run (`textedit::Layout::opened`,
the arithmetic of `defaultTextLayout`, which the two text-edit probes now call too) and the
change goes down the same layout path as an edit made in the window. It reads the page's runs
once more, unedited, to get the run's matrix and advance. Without `font` a plan sends no
layout, as before. This corrects the last sentence of *One unchanged W-9 page* above.

**The W-9 again, through `tpdf-cli edit`.** Page 2, operator 17, *Form W-9 (Rev. 3-2024)*
replaced by *Edited W-9 (Rev. 3-2024)*, three plans:

- no `font`: exit 3, *embedded font does not permit this editable use*;
- `"font":"original"`: exit 3, *This text's font does not permit editing. Choose automatic
  fallback or a Noto font.*;
- `"font":"auto"`: exit 0. `qpdf --check` finds no errors, `pdftotext` reads the new line,
  `pdffonts` lists the six original subsets and one NotoSans. Poppler renders at 100 dpi are
  identical on pages 1 and 3 to 6; page 2 differs in 693 pixels, rows 52 to 62 inside the
  run's 50.8 to 62.9, columns 50 to 160 against a run that ended at 154.3. Noto Sans is wider
  than the Helvetica Neue it replaces, and the box is allowed to grow into the empty margin.

**Not the editor's automatic mode in one respect.** The app process looks for an installed
copy of the document's font before falling back to Noto; the command sends none, so `auto`
there means the document's font, then Noto Sans.

**Tests and mutations.** `a_named_font_sends_the_box_the_editor_opens_on_that_run` in
`cli/edit.rs`. `python3 scripts/mutate_rust.py --only 'cli edit'` and `--only 'opened
layout'`: seven new, all caught on the first run.

**The refusal names the option.** A plan without `font` that is refused because the font
forbids editing, or because the replacement has a character outside the Latin set, ends its
message with *add "font":"auto" to this operation to allow Noto Sans*. Both seen on real
files: the W-9 plan above, and *Ωж* typed into the passport guide's page 3, which `"auto"`
then writes. No other refusal carries the hint, and none does once a font is named. Four
more mutations under `--only 'cli edit'`, caught on the first run.

**The report names the font** (added the same day). The worker lays a change with a box out
for its reply and that layout has a label; `cli/edit.rs` dropped it. It is now the report's
`fonts` list, one entry for each operation that named a font, and a reply without the label
is an internal failure. Checked in the `cli` integration suite on a real edit: `noto_sans_bold`
reports `Noto Sans Bold` against operation 2 of a two-operation plan, `auto` on a font that
has the characters reports the document's own font, a plan with no `font` reports an empty
list, and the plain output has the line. `--only 'cli edit: the font'` and `--only 'cli edit:
a reply'` are its three mutations.

**Not done.** Width, height, size and wrapping are not in the plan.

### Placed artwork — measured 2026-10-01

**The refusal named the wrong thing.** Seven pages of the 26-document survey refused with
*unsupported ActualText marked-content sequence*: the AutoCAD brochure's page 1, the Highmark
brochure's 25, 27 and 52, the passport guide's 1, 13 and 15. Two of them have no ActualText in
their content at all, and the ActualText the others have is the tab spacer already supported.
What all seven share is `/PlacedPDF /MCn BDC` or `/PlacedGraphic /MCn BDC`, InDesign's mark
around artwork placed from another file, on two pages with `/Metadata /MCn BDC` directly
inside. Every `BDC` without an MCID that is not a spacer went to `actual::Span::new`, which
refuses a tag other than `/Span`. The property lists are `<< /Metadata n 0 R >>`, the nested
one with `/Type /Metadata /Subtype /XML` added.

**The rule** is in `docs/TEXTEDIT.md`: the two tags, outside a text object, a property list
holding only the packet, one metadata sequence inside at most, text inside read-only.

**Result, 26 documents, 416 pages:** editable 350 to 352, refused 45 to 43, no text 21. No
page lost and no editable page's run count changed. Passport guide page 1 (30 runs) and
Highmark page 25 (8 runs) became editable. The other five moved to a later refusal, about the
artwork itself: *only complete bounded painted paths are editable* (AutoCAD 1, passport 13),
*only complete unpainted rectangular clips are editable* (Highmark 27 and 52), *empty text
clipping intersection* (passport 15). The first refusals are still 18; the largest is *page
contains only read-only text* at seven, and painted paths and rectangular clips are now four
each.

**Two edits through `tpdf-cli edit` with `"font":"auto"`.** Passport page 1, *We want to help
you ...* with its first word reversed: exit 0, `qpdf --check` clean, `pdftotext` reads *eW
want to help*, 1,763 changed pixels at 100 dpi in columns 248 to 537 and rows 92 to 105,
inside the run's 248.7 to 539.2 and 88.8 to 106.2. Highmark page 25, *To see if your provider
is in network, visit* replaced by *Is your provider in network? Visit*: exit 0, clean,
readable, 2,105 pixels in 85 to 364 by 988 to 1004 inside the run's 85.3 to 367.8 by 984.7 to
1003.8. That font forbids editing, so the text is in Noto Sans, and the same-length reversal
was refused first because the wider Noto line reached the page edge.

**Tests and mutations.** `textedit_placed_artwork_keeps_its_text_read_only`.
`python3 scripts/mutate_rust.py --only 'placed artwork'`: twelve. One survived its first run,
*opens inside a text object*, because the test asserted only that the page was refused and it
is refused either way; the test now pins the reason. A check that the packet is written as a
reference was removed before it was committed: the stream check after it refuses the same
inputs, so no test could tell it was there.

**Not done.** A run in the application window, PDFKit readback, a Windows run, the full
mutation table. Text inside placed artwork is read-only by caution, not by a measured need.

### Centred headings: what *page contains only read-only text* is — measured 2026-10-01

**Seven pages, one rule.** The Healdsburg slides' pages 3, 5, 8, 9, 10 and 29 and the W3C
headers document's page 1 refuse with *page contains only read-only text*. A temporary print
at the read-only decision gave the reason for every run on them: the slide number and the
background text are artifacts, and every heading is tagged content pinned by its element's
`/A << /O /Layout /TextAlign /Center ... >>` (`H1`, `H2`, the `Span`s under them, one `P`, one
`Title`). The rule is `attributes()` in `tagging.rs`: Center, End and Justify pin the block,
because an edit that changes the line's width leaves it where it started and it is no longer
centred.

**How much the rule holds back.** With that one arm returning the unpinned value, measured and
then reverted: 352 to 359 editable pages of 416, refused 43 to 36, and 35,223 to 35,252
editable runs. All 29 runs are in those two documents, 28 of them slide titles on eight of the
Healdsburg file's 29 pages. No other document in the sample declares a non-Start alignment.

**Why it was not simply switched off.** The writer starts a replacement at the run's own
origin (`layout::room`'s comment: starting a line further left is a move of the line, *the
next increment*). An edited centred title would keep its left edge and sit off centre under a
tag that still says Center. Keeping it centred means moving the line's start by half the
change in width, and the shows before the edited one with it. The next section does the first
half of that.

### Centred lines are edited about their centre — measured 2026-10-01

**The slice.** Of the 26 runs the rule locked on the six Healdsburg pages, 20 are alone on
their line, so the first increment is exactly that case: a centred run with no other non-blank
text on its line. Nothing before it has to move, and the writer already restores the cursor
after a replacement, so the whole change is where the replacement starts. `docs/TEXTEDIT.md`,
*Centred lines*, has the rule.

**Result, 26 documents, 416 pages:** editable 352 to 357, refused 43 to 38, editable runs
35,223 to 35,244. Healdsburg pages 3, 9, 10 and 29 and the W3C headers page 1 became editable;
Healdsburg pages 11 and 27 gained one run each. Pages 5 and 8 stay refused: their two heading
lines each have an artifact with the same words on the same line. No page was lost.

**Edits through `tpdf-cli edit` with `"font":"auto"`, Poppler at 100 dpi.** Each passes
`qpdf --check`, `pdftotext` reads the new text, and the changed rows are inside the run's own.

| page | edit | old columns, centre | changed columns, centre |
|---|---|---|---|
| Healdsburg 3 | *Items* to *Items of note* | 140 to 240, 190.3 | 75 to 306, 190.5 |
| Healdsburg 3 | *Items* to *It* | 140 to 240, 190.3 | 143 to 239, 191.0 |
| Healdsburg 29 | *del público* to *público* | 66 to 332, 199.0 | 68 to 329, 198.5 |
| W3C headers 1 | *Test Document* to *A much longer title for headers* | 336 to 514, 425.1 | 231 to 620, 425.5 |
| W3C headers 1 | *Test Document* to *Short* | 336 to 514, 425.1 | 336 to 514, 425.0 |

For a shorter replacement the changed columns are the old text's, which the new text sits
inside, so those rows show that nothing outside the old line changed, not where the new text
is. Healdsburg 9, *Questions* to *Questions and answers*: columns 40 to 932, centre 486 against
485.5, and the same text written back differs in no pixel. Healdsburg 10, *Comment* to
*Comments welcome*, is refused with *There is no room to keep this line centred*: the panel is
about 250 pixels wide.

**Tests and mutations.** Four tests in `tagging/centred_tests.rs`; two in `pinned_tests.rs`
changed to say a centred block is offered. `python3 scripts/mutate_rust.py --only 'centred:'`:
sixteen, all caught on the first run. *alignment: let a centred block stay editable* became
*let a right-aligned block stay editable*, on the arm that still pins.

**Not done.** (The application window is under *The W-9 and a centred title in the application
window*, which found the dashed box off the text's middle.) A page displayed a quarter turn
(`centred::settle`'s `turned`) has no test. The room is still the room to the right of the
run's origin, so a centred line near the right edge is refused although it would grow only half
as far that way. A centred line with other runs on it, right-aligned text and justified text
are unchanged. PDFKit readback, a Windows run, the full mutation table.

### Content on an empty parent-tree slot — measured 2026-10-01

Three pages of the 26-document sample were refused as *marked content repeats or disagrees
with its structure tag*: the first page of the IRS W-9 and W-4 (Designer 6.5) and page 4 of
the Arcadia agenda (Acrobat). None repeats an MCID. Each has marked content whose parent-tree
slot is `null`, under a tag other than `/Artifact`: 20 and 17 `/Content` sequences on the two
forms, holding no text, and three `/Span` sequences on the agenda, holding three lines of a
paragraph. `Tags::begin` took a `null` slot as read-only only under `/Artifact`, although this
file and `docs/TEXTEDIT.md` already said unowned content is read-only.

A `null` slot is now read-only under any tag. An MCID past the page's slots, a repeated MCID
and `/Artifact` on an owned MCID are refused as before. The three pages are editable, with
176, 239 and 136 runs; no other page of the 416 changed, and 360 are editable
(`scratch/textedit-public/w26-nullslot.json` against `w26-centred.json`). The agenda's three
unowned lines are not among its runs.

**On the W-9.** `tpdf-cli edit` with `"font":"auto"` replaced *Before you begin.* on page 1
with *Before you start.*; the report says `set in Noto Sans (the document's font does not
permit editing)`. Poppler at 72 dpi: the two renders of page 1 differ only inside
36 to 103 by 85 to 94, which is the run's rectangle, and pages 2 to 6 are identical.

**Mutations.** `python3 scripts/mutate_rust.py --only 'null slots:'` (3): the page refused
again, a slot the page lacks kept read-only, and an empty tag adopting the slot as its own.

**Not done.** The remaining 35 refused pages have 17 first refusals, none above four pages.

### The W-9 and a centred title in the application window — measured 2026-10-01

Two phases of `tabs_check.py`, each the whole text-editing workflow (24 checks) on an unchanged
public document, with the checks application built as in *Existing-text workflow*:

```sh
python3 scripts/tabs_check.py <checks-binary> scratch/textedit-public/w9.pdf --phase textedit-w9 --saved-copy <dir>/w9-after.pdf
python3 scripts/tabs_check.py <checks-binary> scratch/textedit-public/healdsburg-slides.pdf --phase textedit-centred --saved-copy <dir>/centred-after.pdf
```

`textedit-w9` replaces *Before you begin.* on page 1 with *Before you start.*; the font does not
permit editing, so the text is set in Noto Sans. `textedit-centred` replaces *Items*, one line
of the centred title on page 3, with *Item*. Each adds one check of its own, on where the
editor puts the replacement's target against where the source's was: a left-aligned run keeps
its left edge, a centred one its middle.

**The centred phase found the dashed box off the text.** The text was centred, as the engine
measurements said. Its box was not: the source's target was 143.9 to 229.5 px and the
replacement's 151.3 to 236.2, the same width moved 7 px right. `layout::prepare` reported a box
of the opened width starting where the text now starts, so with a shorter text the box hung
past it on one side by the whole loss. A centred line's box now starts half the difference
before the text (`lead`), which leaves the box the editor opened where it was. Longer text
grows the box with it as before, and other alignments are unchanged. Three mutations,
`--only 'centred: the box'` and `--only 'centred: every box'`.

A second failure in the first run was the phase's own: it expected a centring sentence for a
1,000-character draft, and the application answers *no room for more text on this line: it
reaches the edge of the page*, the same as for any line. The check now expects that.

**After the fix**, both phases pass 24 of 24. In the saved copy `tpdf-cli text-runs` puts
*Items* at 100.98 to 173.07 and *Item* at 107.16 to 166.95, middles 137.02 and 137.06, in the
document's own font.

**Not done.** Looking at the window: these are the harness's readings of targets, text and
pixels, not a person's. Typing a longer centred title in the window. A Windows run.

### `sign --image` and `sign --hide` — measured 2026-10-01

**Why there is a Rust decoder.** The request was for the file to go through the path and
limits of an image imported in the signature chooser, with no second decoder. The chooser's
decoder is the webview's (`createImageBitmap` in `signature.ts`), which the command-line tool
does not have. So the limits and the header grammar are the chooser's, ported rule for rule,
and the decoding is `png` and `zune-jpeg`, both already linked. It runs in a worker.
`docs/SUBSYSTEMS.md` lists what is shared and what differs.

**A worker that reads a second file and writes nothing.** `Worker::spawn_mapped` refused an
inputs mapping without an output file on both platforms, because on macOS the descriptor
shuffle filled its two optional slots in order. The slots are now filled independently and the
refusal is gone. The Windows arm already handed the two over independently; only its guard
was removed, and it has not been run on Windows.

**The end-to-end checks** are `tests/cli/sign_image.rs`, in the `cli` integration suite (371
checks, real workers, a software key, a store that counts): the file's pixels are read back
from the signed document's image XObject with and without a saved image, and the saved image
is read zero times; the control without `--image` draws the saved image and reads it once;
`--lines ""` leaves an appearance with no text; a 1200 by 300 picture arrives as 512 by 128;
seven unusable files (missing, text, empty, cut short, over 10 MB, a PDF, transparent) exit 3
naming the file and the reason, write nothing, and leave the certificate list, the key and
the saved image untouched; a document that is not a PDF is refused as the document. With
`--reason "Document approved" --hide reason`, `/Reason` is set and the appearance has no text;
without `--hide` the same line draws it.

**Rendered with Poppler at 72 dpi**, rectangle `40,40,200,80` on a 400 by 300 page: image
alone, the stamp fills 64 to 216 by 42 to 118 and nothing else on the page is dark but the
page's own stroke; with the default lines, the stamp is in the left half, 42 to 138, with the
text beside it; with the hidden reason, as image alone. `tpdf-cli verify` reads the
signature intact.

**Mutations.** `python3 scripts/mutate_rust.py --only 'signature image:'` (15), `--only
'sign: --'` (5) and `--only 'sign: a hidden'` (3). Three were not caught at first and each
was a redundant line rather than a missing test: `Transformations::EXPAND`, which `ALPHA`
implies, and two guards for an image that shows nothing where one does the work. All three
lines are gone. The harness cannot name a check in the `cli` suite, so eight more were applied
by hand, one at a time, and the suite run against each: the saved image drawn for a file, no
image drawn, the store asked before the image, the worker decoding nothing, every worker
refusal reported as the image's, no size check before the worker, `--hide` not passed on, the
image descriptor not handed over. Each turned at least one check red.

**The item-2 acceptance line needs `--hide reason`.** As written it adds only `--reason` to
`--image stamp.png --lines ""` and expects no text, but `--lines "" --reason X` is a command
line that works today and draws the reason, which the same request says must not change.

**On Windows, with a key in the certificate store** (MOTHERSHIP, Windows 11, pwsh 7.6.6,
`tpdf-cli` built from `458878f`, 2026-10-01). A one-day self-signed RSA certificate made by
`New-SelfSignedCertificate` in `Cert:\CurrentUser\My`, software key storage provider, key
usage `DigitalSignature,NonRepudiation`, and
`-TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.4,1.3.6.1.5.5.7.3.36')`; without that
extension the certificate is for logging in and web servers and `sign` refuses it by name,
exit 3. Removed afterwards with `Remove-Item -Path Cert:\CurrentUser\My\<thumbprint>
-DeleteKey`, and the store listed none left. On a 400 by 300 page, `--rect 40,40,200,80
--image stamp.png`: with `--lines ""`, with `--lines label,date`, with `--lines "" --reason
"Document approved" --hide reason`, and the last without `--hide`. All four exit 0 and
`verify` reads each intact, trust `untrusted` / `root`. Rendered with Poppler at 72 dpi from
the files copied back (digests equal on both sides): image alone, the page changes only inside
64 to 216 by 42 to 119, within the rectangle; with lines, 42 to 237 by 55 to 105. With
`--hide reason`, `/Reason` is `Document approved` and the appearance has no text operator;
without it the reason is drawn. `--image in.pdf` exits 3 naming the file and writes nothing.
This ran over ssh, where the key needed no consent; a key that asks at the desktop was not
tried.

**Not done.** A signing with a real keychain key on macOS, or with a certificate a CA issued
on either platform. `api/python/check_signing.py` with `image=`. EXIF orientation.

**The two header readers are held to one file of cases** (added the same day).
`src-tauri/testdata/signature/headers.json` has 46 headers, 10 read and 36 refused, and each
reader has one test that runs all of them; the counts are asserted on both sides, so a file
that lost its cases fails. The limits sit on both sides of each boundary: 8192 and 8193 pixels
a side, 4096 by 2048 and 4096 by 2049, exactly 10 MiB and one byte more, for PNG and for JPEG.
`python3 scripts/mutate_rust.py --only 'signature headers:'` (15) and
`python3 scripts/mutate_frontend.py --only 'signature headers:'` (14) change one limit or one
rule on one side, and each is caught. Ten of the fourteen frontend mutations are caught by
this test alone: before it, nothing in the frontend suite checked a side limit of 8191, either
byte limit, a lossless, progressive or twelve-bit JPEG, a scan before the frame, a second PNG
header, a PNG end chunk with data, or a PNG without image data. To add a case, edit
`src-tauri/testdata/signature/headers.py`, run it from the repository root, and raise the two
counts.

### `sign`: own wording, placement beside text, and where it was drawn — measured 2026-10-01

Six additions to `tpdf sign`, all on the command line and in the Python client; the signing
panel in the window is unchanged.

- `--identity` takes the certificate's SHA-1 in hex, which is the thumbprint `certmgr` and
  `Get-ChildItem Cert:` show. `identities` prints it and reports it as `sha1`.
- `--text` draws the caller's own lines in place of the standard three, with `{name}`,
  `{date}`, `{reason}` and `{location}` filled in. Braces, because a batch file rewrites a
  percent sign inside an argument and a POSIX shell rewrites a dollar.
- `--date-format` writes the date from `YYYY`, `MM`, `DD`, `HH`, `mm` and `ss`. The time is
  UTC: the date drawn is `/M` read back.
- `--reason` and `--location` no longer need `--visible`, and `--contact` writes
  `/ContactInfo`. They reach the worker as `sign_prepare::Notes` in
  `Request::PrepareSignature`, which the application sends empty.
- `--anchor TEXT --size w,h [--offset dx,dy] [--anchor-match N]` measures the rectangle from
  text on the page, found by the search `redact --text` uses. A second worker reads the page's
  text before the store is asked for anything.
- `sign --json` reports `appearance`: the rectangle, the image's rectangle, the type size and
  each line's box and baseline, in the space `--rect` is given in. It is the worker's layout
  computed a second time in the tool's process from the same rectangle, image size and lines.

**What holds it.** 50 mutations in `scripts/mutate_rust.py` under `identity:`, `sign text:`,
`sign date:`, `sign notes:` and `sign anchor:`, each caught by the test named for it, and three
older ones re-aimed after their anchors moved. `tests/cli.rs` gained two groups,
`sign_text::draws_the_text` and `sign_anchor::places_beside_text`, which sign through the tool
and read the drawn strings, the text and image matrices, the signature dictionary and the
anchored rectangle back out of the written file. That binary is outside the mutation table, so
eight mutations were run by hand against it, each rebuilt and each red: the reported baseline,
line box and image moved by a point; the anchor's offset dropped on either axis; the match
number taken off by one; a second match accepted without `--anchor-match`; and the page-count
check removed.

**Not covered.** The `appearance` comparison signs an upright page; no rotated page is
compared. Nothing here ran on Windows beyond `scripts/check_windows.py`.

### The command-line tool on the Windows `PATH` — measured 2026-10-01

The per-user installer (`-setup.exe`) runs `tpdf-cli path --add` after installing and
`tpdf-cli path --remove` before uninstalling, from `src-tauri/installer-hooks.nsh`; removal is
skipped when the uninstaller runs as part of an update. *Install command-line tool…* and
*Uninstall command-line tool…* do the same on Windows. The edit is `userpath.rs`: the user's own
`HKCU\Environment\Path`, read whole through the registry API. It is not done in the
installer's script, because an NSIS string is cut at a fixed length and the desktop's user
`PATH` measured here is 1,899 characters.

**Measured on MOTHERSHIP**, with a debug `tpdf-cli` built from this tree and the value exported
first: `path` said the folder was not there; `--add` appended it as the last entry and nothing
else changed; a second `--add` wrote nothing; `--remove` left the value identical to the one
exported, character for character, with its registry type (`REG_SZ`) unchanged; a second
`--remove` wrote nothing; `--add --remove` exits 2. `npm run tauri build -- --bundles nsis` then
built `tpdf_26.10.0_x64-setup.exe` with the hooks included, so both macros compile.
`python3 scripts/mutate_rust.py --only 'user path:'` runs 12 mutations of the list editing and
the argument parsing, each caught.

**The installer, run on that desktop**, silently, with tpdf not running and the user `PATH`
exported first. Install: exit 0, and the value is the exported one with the install folder
appended, once. The same installer again with `/UPDATE`, and again without: exit 0 both times,
the value unchanged and the folder still there once. Uninstall (`uninstall.exe /S`): exit 0,
the value identical to the export, the tool gone, and the same eleven data folders as before.
The released 26.10.0 installer was then put back.

**Not done.** The menu command in a window on Windows. An update driven by the updater itself,
which passes its own arguments. A `PATH` stored as `REG_EXPAND_SZ`; the code keeps whatever type
it reads. `tpdf-cli path` without an option was reworded after these runs to say what is stored
as well as what the terminal holds, and that wording has only been type-checked for Windows.
The `.msi` installer does not change `PATH`.

### A LibreOffice list in paragraph styles of its own — measured 2026-10-06

`docs/TEXTEDIT.md`, *Lists in paragraph styles of their own*, has the rules. This is what
was measured, on macOS arm64 with a debug build.

**The document that was refused.** One page, LibreOffice 26.2 Writer, tagged, a numbered
list in a paragraph style of its own, then a signature picture from Acrobat's Fill & Sign
and a document timestamp. It is not public and nothing of it is in this repository; only
its refusals, tag names, operator names and counts were read. `tpdf-cli text-runs` exited 3.
With a throwaway build that names the refusing line, the refusals came in this order, each
found only once the one before was gone:

| | Refusal | What it was |
| --- | --- | --- |
| 1 | *unsupported or inconsistent tagged text structure* | a paragraph element under `LBody`, named after its style, which the RoleMap makes a `P` |
| 2 | *marked content without MCID must be an Artifact* | `/ADBE_FillSign BMC`, once, at the end of the content |
| 3 | *unsupported preserved Form XObject* | the key `/ADBE_FillSign` on each of three nested forms |
| 4 | *unsupported preserved Form XObject* | the innermost form's `/BBox [0 1 1 0]` |
| 5 | none: 18 runs offered of a page of text | the body font's map names U+2022, used in the footer, so the font was read-only |

After the change: exit 0, 83 runs offered, which are 108 of the page's 125 shown strings; 17
are read-only. One same-length change through `tpdf-cli edit` to a scratch copy: exit 0, the
copy re-read with 83 runs, one of them the replacement, still tagged. The copy was deleted.
The timestamp's revision and its signature field played no part in any refusal.

**The fixture.** `testdata/textedit-producer-list-styles.fodt` is a synthetic Writer document,
every name in it invented, set in Liberation Sans (SIL Open Font License 1.1, shipped with
LibreOffice). Its export is committed, because a hosted runner has no LibreOffice:
`src-tauri/src/textedit/tagging/fixtures/libreoffice-list.pdf`, 52,116 bytes, SHA-256
`558ff52a4bc29f8f4f07b320af70a699ff302a966f15b4a812f517e007ae5930`, made by LibreOffice
26.8.0.3 on macOS with

```
soffice --headless \
  --convert-to 'pdf:writer_pdf_Export:{"UseTaggedPDF":{"type":"boolean","value":"true"}}' \
  --outdir <directory> testdata/textedit-producer-list-styles.fodt
```

`tpdf-cli info` says `Tagged: yes`. An export repeats neither its creation date nor its file
identifier, so a new export has another digest; replace the committed file, the digest in
`tagging/libreoffice_tests.rs` and in `scripts/text_list_check.py`, and this paragraph
together. That was done once, later the same day: the source gained a fifth list item and
36 pt of space above the table for *A wrap on a LibreOffice page* below, and the export
named here is that second one. The first was 50,926 bytes, SHA-256
`8f7e3f4514c568989892003a3170af06ff9134c7d826347067777e95a5fe62fb`, and the counts in the
rest of this section are its counts: 27 runs, 10 read-only shows, 930 structure values,
where the second export has 32, 11 and 1,033.
Before the change the export was refused at the same line as the document above,
for the same pair (a style-named paragraph under `LBody`), and with the font change undone
it offers 5 runs, the bold ones, for the same reason that document offered 18.

What the export holds that the document did, by structure: `LI > LBody > <style> > Span(Lang)`
with a `Span` carrying `/ActualText` U+00AD for each of two hyphens; centred `Standard`
paragraphs; a justified one; a table with a `BBox`, block rows and inline cells; a bullet
(U+2022) in the body font's map. What it holds that the document did not: a sublist inside
a body beside its paragraph (the document's bulleted list was a list of its own between two
numbered ones); a header picture drawn as paths in a `Figure` sequence whose element nothing
in the tree reaches, where LibreOffice 26.2 wrote an artifact; a footer in an artifact with
a property list, where 26.2 wrote `/Artifact BMC`. What it lacks: anything from
Fill & Sign, which no tool here writes; those shapes are built by hand in `forms/tests.rs`
and appended to the export in `tagging/libreoffice_tests.rs`.

**In the suite.** `tagging/libreoffice_tests.rs` (4 tests) loads the export: the 27 runs
offered, by text; the 10 read-only shows with text (4 bullets, 2 hyphens, 2 lines of the
justified paragraph, 2 of the footer); the item as one block; four edits in one write (a list
paragraph, bold first words, a sublist bullet, a centred line with the editor's layout),
after which every other run is where it was, every structure object, the role map and the
parent tree are the objects that were read, and no font program changed.

**Round trip, read back three ways.** `uv run --with pypdf scripts/text_list_check.py
<text-edit-probe> <tpdf-cli> <new-directory>`: the four edits go through
`text-edit-probe --roundtrip` (preview and save agree pixel for pixel, and so does everything
outside the edits), and the saved copy is read by pypdf (the page's text; 930 values of the
structure graph, role map and parent tree compared one by one; 4 font programs by digest;
the annotations), by PDFium (`tpdf-cli text`) and by PDFKit. All three show the four
replacements and no other change. The script first runs its comparisons where they must
fail, the unedited source and a copy whose list paragraph is renamed `P`, and both are
refused. The same four changes through `tpdf-cli edit --plan`, the centred one with
`"font":"auto"`: exit 0, 27 runs re-read, `verify` exit 0.

One thing the readers showed that the tests had not: a shorter word before a hyphenated line
end leaves a gap before the hyphen, which is a read-only run of its own and does not move.
The script edits a line that ends its paragraph.

**Mutations.** `python3 scripts/mutate_rust.py --only 'list paragraph:' --only 'fill and
sign:' --only 'kept code:'`: 31, each red in the test named for it. Two guards went out
instead of being covered. A filter refusing a kept code whose glyph is `.notdef` could not
be reached through a format 0 character map, and is not needed: whatever glyph the program
draws for a code nobody writes is the one to measure. A draft mutation that took any element
in a body for its paragraph was caught, but by the test for a body's own inline leaf and not
by the one written for it; it is listed under that test.

**Not done.** No window was opened: the check that needs a screen is *Edit existing text* on
the export, for the list paragraph's box, a wrap of a list paragraph (the item's label should
stay on the first line, the continuation lines start under the paragraph's text, and the
items below move down whole), and the centred line. Nothing ran on Windows beyond
`scripts/check_windows.py`. LibreOffice 26.2's own export of the fixture was not made; 26.8
is the version installed here. No document with a two-paragraph list item or a composite
no-break space was measured, and both are still refused or read-only as before.

### A wrap on a LibreOffice page — measured 2026-10-06

`docs/TEXTEDIT.md`, *What moves with a wrap on such a page*, has the rules. On the page of
the section above, and on the export, a line that was full could be given no more words:
the wrap that works on a Word export was never tried, and when it was tried it was refused.
Measured on macOS arm64; the counts are `text-edit-probe --growth` in `app` mode, which
types 10, 25 and 50% more characters at the end of every run with visible text, in the box
the editor opens.

**Two causes, in order.** (1) LibreOffice draws the page under a clip the size of the sheet
less a rounding (`0 0.028 594.964 841.975 re W* n` on the export), so every line ended at a
clip 0.028 pt before the page's edge, and only a line the page or a column ends is offered
a wrap. With that alone changed, every one of the text editor's unit tests still passed,
which is how it was known that none covered it. (2) The wrap was then refused: its moved
lines landed on text that stays, or a line of the paragraph could not move. Only text the
writer may rewrite moved, and below a list item there are bullets, added hyphens and a
justified paragraph.

**The source and its export.** `testdata/textedit-producer-list-styles.fodt` gained a fifth
numbered item, a paragraph of three lines whose first ends at a space and whose second
ends at an added hyphen, and 36 pt of space under the justified paragraph. The space is
there because the table below states its bounds: without it every wrap on the page was
refused, rightly, with *The table states its bounds, and this text would leave them*, and
no wrap could be measured. The export was made again with the command of the section
above (same LibreOffice, 26.8.0.3) and replaces the first: 52,116 bytes, 32 runs offered,
11 read-only shows with text (4 bullets, 3 hyphens, 2 lines of the justified paragraph, 2
of the footer).

**Counts, before (commit `68d351cd`) and after.** The export, 31 runs tried:

| | +10% | +25% | +50% |
| --- | ---: | ---: | ---: |
| accepted, before | 27 | 26 | 26 |
| accepted, after | 27 | 27 | 27 |
| *the document clips the space after it*, before | 0 | 1 | 1 |
| the same, after | 0 | 0 | 0 |
| *other text follows it*, before and after | 3 | 3 | 3 |
| *the text after it cannot be moved*, before and after | 1 | 1 | 1 |

A LibreOffice export that was signed afterwards, the one page of the section above, not
public and read here for counts and the tool's own messages only, 81 runs tried:

| | +10% | +25% | +50% |
| --- | ---: | ---: | ---: |
| accepted, before | 69 | 60 | 51 |
| accepted, after | 69 | 69 | 68 |
| *the document clips the space after it*, before | 0 | 9 | 18 |
| the same, after | 0 | 0 | 0 |
| *it reaches the edge of the page*, after | 0 | 0 | 1 |
| *other text follows it*, before and after | 7 | 7 | 7 |
| *the text after it cannot be moved*, before and after | 5 | 5 | 5 |

The untagged export of the same source (37 runs tried, blocks read off the lines): 32
accepted at +25% and at +50% before, 5 refused for the clip; 35 after, and 2 refused as
*part of it below cannot be moved*.

The growth instrument types a quarter or a half more, which fills a long line and leaves a
short list item short. A one-line item is wrapped by `scripts/text_list_check.py` instead,
with a sentence.

**Every newly accepted edit, saved and read back.** Each of the 26 on the signed document
(9 at +25%, 17 at +50%), the 2 on the export and the 6 on its untagged twin went through
`text-edit-probe --roundtrip`, which holds the worker's preview and the saved page to the
same pixels, and then through `scripts/text_wrap_check.py --compare`, which pairs every
glyph before and after as pdfplumber reads them: 34 of 34 passed. In each, every glyph of
the page either stayed or moved straight down with its line, the page gained exactly the
glyphs the request adds, and no pair of glyphs overlaps (0 before, 0 after, every time). On
the signed document between 151 and 2,084 glyphs moved, by 14.0 pt in 23 of the 26, by
11.0 and 11.3 pt in 2 and by 14.8 pt in 1.

**The public sample, so that nothing accepted before is refused now.** The change is not
about LibreOffice alone: read-only text and text in ActualText spans move on every page,
and a clip near the page's edge is the page on every page. `scripts/textedit_growth.py`
over the 31 files of `testdata/textedit-public-corpus.json`, 52,894 runs, release probes
built from `68d351cd` and from this tree, `app` mode, with `--records` and `--compare`:

| trial | before | after |
| --- | ---: | ---: |
| +10% | 45,773 | 45,920 |
| +25% | 42,893 | 43,251 |
| +50% | 40,251 | 40,740 |

1,006 verdicts went from refused to accepted, on eleven files, and none the other way. The
first run of the comparison had 46 the other way, on three files, nearly all on an
untagged XeLaTeX manual: wraps that had been accepted with a read-only block left where it
was were now refused, because that block was offered to the cascade, moved as soon as the
break above it would close, and its move was refused for text beside it that stays. A wrap
refused with every movable block offered is therefore tried once more with only the blocks
the writer may rewrite, which is the wrap as it was (`layout::prepare`, `plain`); with that
the 46 are accepted again and the 1,006 are unchanged.

Of the 1,006, 54 were taken evenly from the eleven files and saved. Six are on files of
more than 128 pages, which `--roundtrip` refuses to open. The other 48 all pass the round
trip (preview and saved page agree, nothing outside the edit's envelope changes); 29 of them
pass `text_wrap_check.py --compare` and 19 fail it, in the shapes the earlier wrap records
list for this checker: a replacement set in a fallback font, glyphs of the edited run on two
baselines, a cascade over many distances. Two of the 19 were rendered before and after and
are right on the page, each line below one pitch lower with its formula glyphs and its link
text in place. The other 17 were not looked at.

**Three wraps on the export, read by three readers.** `uv run --with pypdf --with pdfplumber
scripts/text_list_check.py <text-edit-probe> <tpdf-cli> <new-directory>` now also types a
sentence after a one-line item, after an item above a bulleted sublist, and after the first
line of the fifth item, one round trip each. pypdf, PDFium (`tpdf-cli text`) and PDFKit
each show the item's text with the sentence after it and every other line of the page as
it was and in the order it was; the structure (1,033 values), the 4 font programs and the
annotations are unchanged; and the glyph comparison passes: 597, 533 and 322 glyphs moved
down, by 11.3, 11.3 and 10.4 pt, with 99, 70 and 67 glyphs added and no overlap. Each
comparison is first run where it must fail: on the unedited source, and on a reading with
two lines exchanged. Two readers changed their reading of text that had only moved, and
the comparison says what it holds each to (`docs/TRAPS.md`, *Two readers each changed their
reading of text that had only moved, and comparing whole readings blamed the edit*). The
three saved pages were rendered with `tpdf-cli render` and looked at: the new line hangs
under the item's words, the sublist's bullets, the hyphen at the end of the fifth item's
second line and the justified paragraph are each one line lower beside their text, and the
table and the footer are where they were. The first line of each of the two one-line items
runs on through the right margin to the edge of the sheet, which is the room its line had
(*Not done*).

**In the suite.** `textedit/carried_tests.rs` (7 tests): a pinned block below moves with
its bytes and what follows it starts at the bits it did; a show in an inline ActualText span
moves inside its span and is edited afterwards; a read-only line of the edited paragraph
moves and is held to the page; text that stays in the middle of a read-only line refuses
the wrap by name; a line under a clip the size of the page wraps and one under a clip 0.6 pt
in does not; the rule as geometry, in the four directions a line runs; and a wrap a
read-only block cannot follow, made without it.
`tagging/libreoffice_tests.rs` (6 tests, 2 of them new): the three wraps above with every
other show where it was or one line lower, and what is still refused, with its wording.
`tagging/list_tests.rs` (2 new): where a one-line item and an item of several lines start
a new line. Two tests in `wrap_tests.rs` changed, because what they pinned is the refusal
this change removes: a block below whose text is inside an ActualText span now moves (its
place there is taken by a spacer, which does not), and a link's words in a line of the
paragraph move with the line where the wrap was refused.

**Mutations.** `python3 scripts/mutate_rust.py --only 'carried:' --only 'page clip:' --only
'item:'`: 22, each red in the test named for it. The first run had 2 of them red in another
test than the one named: a constant moved by a tenth of a point, which a float comparison
decided differently in each direction, and a mutation of the several-line case that no
test of that case existed for; the first is now a larger step and the second has a test.
18 older entries were re-aimed at the same guards where the lines moved, and each is red
again. `beneath: text in an ActualText span may move` left the table: that text moves now,
and `carried: a show in an ActualText span stays where it is` is its reverse.

**The measure of a one-line block.** A one-line item wrapped only where its line ran out
of room, which on these pages is the edge of the sheet, through the right margin. It now
wraps at the furthest any line reaches in the page's other tagged blocks of several lines
(`wrap::borrowed`). In the fixture item 2 with a sentence added broke two words earlier
than before, at the measure of the items beside it. One test in
`tagging/libreoffice_tests.rs` is new: item 3 is first grown on its line into the margin,
then item 2 still breaks where it did (a block of one line is no measure), and item 3,
grown again until the page ends it, keeps its first line (a line past the measure is not
held to it). Three mutations under `measure:`, each applied by hand and red in the test
named for it. The growth probe's accepted counts on the fixture and on the measured
document are the same with and without this.

**In a window, and what it found.** A debug build against the dev server, on a scratch
copy of the measured document, a sentence typed onto the end of a one-line list item.
Three things came out of it that no probe had shown:

- The edit was refused as reaching the edge of the page, although the writer accepted the
  same sentence when the probe appended it. The difference was one character: the reader
  replaced the item's full stop, so the run's own bytes were not kept whole and the line
  was laid out afresh. `line_breaks` broke by the advance, the last glyph's ink reached
  0.1 pt past the borrowed measure (433.26 against 433.16), and the ink check refused the
  line. The break now measures the ink where a line may break. A search of 15,625
  replacements on the fixture found none that differ, since Liberation Sans has no such
  overhang at 9 pt; the test uses the overhang font of `fonts/ink_tests.rs` in a box the
  reader wraps, and the mutation `ink break:` is red in it.
- The item's two lines stood 11.25 pt apart among paragraphs set wider. The pitch is now
  borrowed from the nearest block of several lines; the mutation `measure: a one-line
  block wraps at the editor's own pitch` is red in the fixture's wrap test, whose two
  one-line cases moved from 11.25 to 10.35. `wrap: the single-block pitch` was re-aimed
  and is red again.
- After both, the item wrapped at the right margin at the document's own pitch, the items
  below moved down, the signature block and the footer stayed, the save warned about the
  digital signature, `pdftotext` read the new words once and `qpdf --check` passed.

**A few words past the measure.** After that check, fewer words than fill the line to the
edge of the sheet were still set on one line, through the right margin. They now wrap at
the measure, on a tagged page, where the page ends the line's room and nothing follows the
run on its line; where no wrap can be planned or made they stay on their line, so no edit
accepted before is refused. A first version without those limits turned five tests red,
and each was a decision already made: a push moves text along its line, a line beside a
column may take half the gutter, and a wrap after a push is refused. On the fixture three
cases are pinned (a one-line item, the last line of an item of several, and the item with
the room above the table spent, which stays), and the growth probe's accepted counts are
unchanged on the fixture (27 at each step) and on the measured document (69, 69, 68).
`scripts/text_list_check.py` passes as before. Eight mutations under `margin:`, each
applied by hand and red in the test named for it; `wrap: at another size` and `wrap: after
a push` were re-aimed at the same guards and are red again. A guard for a box that does
not grow was written and removed: no such box reaches that code, and nothing went red
without it. In a window, on a scratch copy of the measured document: thirty characters
added to a one-line item, which ran to the edge of the sheet on one line before, previewed
as two lines and were applied as two, broken at the right margin at the document's pitch,
with the items below one line lower; ten characters added to the last line of an item of
several stayed on that line, inside the measure; the copy saved, `pdftotext` read the new
words and `qpdf --check` passed. The last line of an item of several growing past its
measure was not reached in the window: the typing automation lost characters in that
field after an arrow key, three times. The fixture test covers it. Not measured: the
public sample.

**Still open.** A wrap leaves an empty run behind for each
line it moved (32 runs became 53 on the fixture, the 21 new ones empty). The size test in
`borrowed_pitch` has no test that fails without it: the fixture has no block of several
lines at another size.

**Not done.** No window was opened. Nothing ran on Windows beyond `scripts/check_windows.py`.
A line that ends at an added hyphen still takes no more words: that is the 7 and the 5 of
the signed document and the 3 and the 1 of the export, unchanged. The one edit still refused
at +50% on the signed document, for the page's edge, was not looked into. A paragraph of one
line wraps at the room its line had, which on these pages is the edge of the sheet and not
the right margin. No LibreOffice 26.2 export was made. Of the 1,006 edits newly accepted on
the public sample, 48 were saved and 31 were confirmed, by the glyph comparison or by eye.

### The command-line tool's two commands are greyed by what is installed — measured 2026-10-06

*Install command-line tool…* is greyed once this copy's tool is what a terminal gets, and
*Uninstall command-line tool…* when nothing is at the tool's paths. The reading is
`clitool::state_of` (macOS, over `clitool::plan`) and `userpath::stored` (Windows), asked
through `command_line_tool_state`; what is known and when it is asked again is
`src/lib/clitoolstate.ts`. The commands themselves are unchanged and still read the
filesystem or the `PATH` when they run.

**macOS.** `cargo test --locked --lib clitool::` runs 8 tests, all passing; the state tests
build every arrangement of the two names in a scratch folder --- nothing, one name, both,
another copy of tpdf, somebody else's file, a path that cannot be read --- and list the
folder before and after each read. As a control on the real paths, a test added for the
purpose and removed again read `/usr/local/bin/tpdf` and `/usr/local/bin/tpdf-cli` on a
machine where both link to the released application: asked with that application's tool
it answered installed, in 247 µs, and asked with a tool path of another copy it answered
not installed with something there. `python3 scripts/mutate_rust.py --only 'cli tool state'`
runs 6 mutations, each red in the test named for it.

**The window's side.** `npx vitest run src/lib/clitoolstate.test.ts src/lib/appcommands.test.ts`
passes; the first holds the rule, a failed read, two reads that cross and the look taken
after a command, and reads `App.svelte`'s source for the join. The second holds that each
command is withheld by its own answer in the palette and in the map the menu bar is greyed
from. `python3 scripts/mutate_frontend.py --only 'cli tool'` runs 23 mutations, each red.
The two booleans cross as `ToolState`, held to the window's mirror by the committed sample
`src-tauri/testdata/replies/ToolState.json`.

**Windows**, on a Windows 11 desktop through `scripts/run_on_windows.py --suite all`: the
unit tests pass, 2,891 of them, among them
`userpath::tests::the_handle_a_reading_holds_cannot_write` --- a write through the handle a
reading opens is refused with error 5 --- and
`asking_whether_a_folder_is_stored_writes_nothing`, which compares the last-write time of
`HKCU\Environment` across three reads. That test's control failed on its first run: a
`RegSetValueExW` storing the bytes already there does not move a key's last-write time, so
the control now writes a changed value and the handle test is the one that rules a write
out. `scripts/check_windows.py` passes. The command-line suite passed 517 of 517 in three
runs of four; in the other, `-o --force replaces an existing file` failed once with an
access refusal on a temporary file, in code this change does not touch.

**Not done.** No window was opened on either platform: the greying of the two items in the
macOS tpdf menu, the palette leaving a greyed one out, and the second look after a command
and on coming back to the window have been tested as modules and read in the source, not
seen. Neither command was run, because installing asks for an administrator password and
changes the machine. On Windows the command has not been asked from a window; the
test holds the reading to the stored value for three folders and does not say which of
them were there.
