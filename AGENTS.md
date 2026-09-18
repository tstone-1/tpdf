# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

Two things this file does *not* carry in full. The trap list lives in
[`docs/TRAPS.md`](docs/TRAPS.md) and is indexed by title below; the worked-out account behind
each rule — the measurements, what they cost, and which earlier sentence they corrected —
lives in [`docs/RATIONALE.md`](docs/RATIONALE.md), which the three long sections here point at.
Neither is auto-loaded, on purpose, and the indexes exist so that the decision to read an entry
is an informed one rather than a guess. Code comments and the other documents say "`AGENTS.md`
records ..." in about a hundred places; those references were written when all of it lived here
and are still good in one hop — read them as naming whichever of the two files carries the
paragraph.

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

Existing-text editing admits exact 90/180/270-degree text matrices with positive
orientation after composition; editable runs require diagonal page CTMs. Decoded page content
is bounded to 1 MiB and 16,384 operators, allowing character-positioned exports
past the former 4,096-operator limit. Adjacent positioned text fragments are grouped
within a line, without crossing graphics/text state changes. Saving a group
replaces its first show and empties its other shows, preserving every authored
positioning operation. Rectangular clips remain active during preview and saving;
partly clipped text no longer blocks the page, and hit targets respect that clip.
Compound rectangular paths retain their winding/hole semantics. Identity-H fonts
also admit bounded Unicode mappings, with glyph indices, widths and outlines
validated against their embedded TrueType program. Without an explicit layout,
replacements need existing validated glyphs and must fit the original width.
`text-edit-probe --roundtrip <input.pdf> <requests.json> <new-output-directory>`
checks contained preview/save pixel agreement and unchanged adjacent pixels;
request examples are in `src-tauri/src/probes/text_edit_roundtrip.rs`. Keep private inputs,
request files and outputs in ignored directories, never in fixtures or assertions.
Line movement and both hit/ink envelopes use the transformed text axes. Skew and
mirrored final text are not offered for editing. `textedit/rotation_tests.rs` checks crop/page turns, clipping on
every edge, bounded replacements and preserved positioning.
`tabs_check.py --phase textedit-passport` edits the vertical label on page 16 of
the unchanged passport guide in `testdata/textedit-public-corpus.json`.
Independent readers accept `--passport --page=15` with the usual before/after
filenames (`testdata/make_textedit_embedded.py --check` and
`scripts/text_edit_pdfkit.swift`). Selection reads the edited vertical label and
retains its order through every view turn. `PageText.char_turns` carries PDFium's
per-scalar clockwise directions relative to the unrotated page, omitted when all
are zero. Reading groups directions separately before ordering their lines;
caret placement and selection highlights use those same character directions.
Arbitrary skew retains the existing upright fallback. Mixed untagged directions
are treated as separate regions; author-defined tags still decide block order.
`uv run --with pypdf scripts/text_direction_check.py <text-probe>` verifies all
16 text/page rotation combinations through real extraction (`--mode json`).
`TextEditor` focuses with `preventScroll: true`: ordinary
focus can scroll an overflow-hidden overlay and displace every hit target.

Explicit text layouts support width/height, font size and wrapping within one
editing area. `textedit/layout.rs` restores the original font, spacing, line
matrix and cursor after drawing each replacement; later text remains fixed.
`vendor/fonts/manifest.json` pins four OFL Noto Sans styles and upright regular/
bold Noto Sans CJK SC for automatic or explicit fallback. Automatic mode keeps
the original font when it covers the replacement, then tries Noto Sans and CJK.
CJK uses Simplified Chinese glyph forms, including its mapped traditional Han,
kana and Hangul; it does not choose regional forms by language or synthesize
italics. Latin font programs are shared by style within a write. CJK programs
are subsetted by the permissively licensed `subsetter` crate and shared only
when style and source glyph sets match. CIDToGIDMap preserves remapped indices;
the original OS/2 table is restored so saved subsets retain embedding rights
and pass normal validation. Subsets retain the 1 MiB decoded-font bound.
`scripts/build_cjk_fonts.py` reproducibly builds pinned static instances with
FontTools, retaining all Unicode mappings and dropping unused shaping/vertical
alternates. This avoids ttf-parser's loca-count overflow for 65,535 glyphs.
`scripts/text_cjk_check.py` checks worker preview/save pixels and independently
compares saved Unicode, outlines, widths, rights and sfnt checksums with the
bundled fonts. Layout tests cover adding new characters after reopening a subset.
No system fonts, complex-script shaping or automatic table-row growth are used.
New text must fit the page, active clips and the selected box without colliding
with other source text. Draft previews use the worker's save path and a bounded
PNG crop; cancelling a preview creates no journal entry. Independent synthetic
generation/readback commands are in `scripts/text_layout_check.py`, and
`textedit/layout_tests.rs` covers orthogonal matrices, continued shows, clipping,
deletion and overflow. Layout changes participate in the normal undo journal.

Consecutive `Tj`/`TJ` text shows keep a separate text cursor and line matrix.
The following byte-patching rules describe changes without an explicit layout.
When an edit precedes another show without a position reset, the writer adds a
trailing `TJ` adjustment to preserve the original advance. It refuses an edit
whose PDF number precision would move following text by more than 0.000001 page
points. `Tm`, `Td`, `TD` and `T*` reset the cursor; a new `BT` still requires explicit
positioning. Only edited `Tj` operators may become `TJ`; all untouched stream
bytes remain unchanged. `textedit/continuation_tests.rs` covers deletion,
spacing/font changes, orthogonal axes and line resets. Generate fixtures and
check independent saved geometry with `scripts/text_continuation_check.py`;
`text-edit-probe` and `text_edit_pdfkit.swift` accept `--continued` for them.
`TD` also sets leading to minus its vertical operand, with the same coordinate
bounds as `Td`. Leading persists across text blocks and follows `q`/`Q` saves.
The generator's `--leading` fixture uses ordinary worker/native/PDFKit checks
without `--continued`; `textedit/leading_tests.rs` covers signed spacing,
transformed axes, line resets and preserved positioning bytes.

Inline `/Span` sequences carrying only `ActualText` tab or U+0007 separators
are preserved, with one optional `Tm`/`Td` and one space-only `Tj`/`TJ`. They
remain subject to normal font, coordinate and clipping validation; their shows
are excluded from editable runs. Nested sequences, state changes, visible text,
extra semantic properties and more than 32 separators are refused. The byte
patcher preserves the entire sequence, and preceding edits retain its origin.
`textedit/spacers.rs` owns these checks; generate independent fixtures with
`scripts/text_continuation_check.py --generate <path> --inline tab|bell|tabs`.
Use `text-edit-probe --continued` and `text_edit_pdfkit.swift --inline` for readback.

Other bounded inline or outer `/Span` ActualText sequences are validated without
blocking unrelated runs. A single visible fragment whose logical text agrees can
be edited; the writer patches both its show and ActualText, including deletion and
single-line layout previews. Empty cursor-restoration shows stay read-only.
Different logical text or multiple visible fragments remain read-only with embedded
glyph bounds used for collision checks. Nested spans, extra semantic properties,
malformed encodings and multiline ActualText layout edits remain refused.
`textedit/actual.rs` owns this path. `uv run --with pypdf scripts/text_partial_check.py
<text-edit-probe> <new-ignored-directory>` generates synthetic inputs and verifies
contained preview/save pixels plus independent text, resource and structure readback.

MCID-bearing `BDC`/`EMC` markers also work inside text objects, using the same
bounded structure, role and ownership validation as markers outside `BT`/`ET`.
Text objects and marked-content sequences balance independently; tag boundaries
never reset text position or continuation tracking. Extra semantic properties,
nested MCIDs and untagged MCIDs remain refused. The symbolic generator's
`--inline-tags` option runs through ordinary worker/native textedit checks and
independent parser/PDFKit readback. `tagging/inline_tests.rs` also checks shorter
edits and deletion across adjacent tags, malformed sequences and tab spacers.

Single rectangular clips accept nonzero signed width and height, normalizing
transformed corners before intersection. The saved `re W/W* n` bytes remain
unchanged. Partly clipped text retains its clip and exposes only its visible hit
area. Bounded compound rectangular clips preserve winding and holes; their text
ink must remain fully contained. Unsupported clip shapes remain refused.
Generate equivalent clip fixtures with `make_textedit_composite.py <path> --clip-direction
positive|x|y|both --clip-rule W|W*`; the background makes a missing clip visible.

Opaque eight-bit grayscale/RGB JPEG image XObjects survive text edits unchanged,
including progressive and ICCBased images. `textedit/images/jpeg.rs` bounds
encoded bytes, marker framing, scan count and decoded dimensions before pixel
allocation; the shared page image budget still applies. Decoder success does not
certify every entropy sample: even strict mode recovers some malformed input.
The explicit framing check rejects empty scans, missing ends and trailing images,
but admits NUL padding after EOI (Acrobat writes a few bytes of it); the decoder
is then given the bytes up to EOI. `[/FlateDecode /DCTDecode]` (Acrobat's
recompressed scans) is inflated by `filters::inflate`, bounded like encoded
content, and the result checked as a JPEG. No image sample is rewritten.
Four-component JPEGs, colour-key masks and decode parameters selecting a
predictor remain refused.

Stencil masks (`/ImageMask true`, 1 bit) are `images/stencil.rs`: unfiltered,
Flate, or pure CCITT Group 4 (`K < 0`, `Columns` = `Width`, `Rows` absent, 0 or
`Height`, no byte-aligned rows or EOL codes). The G4 stream must decode to
exactly `Height` rows through `fax::decoder::Group4Decoder`, driven row by row
because `decode_g4` pads missing rows with white. A stencil paints the fill
colour, so `Do` checks that colour at every use; a preserved form, whose colour
is not tracked, refuses one. Tests build their G4 data with the crate's encoder.
Generate synthetic JPEG cases with
`uv run --with pillow --with fonttools --with pypdf testdata/make_textedit_jpeg.py <directory>`;
use ordinary `text-edit-probe`/native textedit checks and PDFKit `--image` readback.

An image may also carry a `/SMask`, which is validated as its own DeviceGray
image and charged to the same page budget; alpha for alpha is refused rather
than followed. `/Decode` is admitted only as the colour space's own default —
`[0 255]` for an indexed image at eight bits, `[0 1]` per component otherwise —
so no image the editor keeps needs its samples remapped. `/DecodeParms` is
admitted only with `Predictor` absent or 1, which leaves the filter's output as
the sample data; `filters::decode_unpredicted` exists for that single caller and
every other one still refuses parameters outright. An image's `/Metadata` packet
is kept unchanged and must declare `/Type /Metadata`. Ordinary Word, LiveCycle
and Illustrator exports write all four around a logo with a transparent
background. Generate the fixtures with
`uv run --with fonttools --with pypdf testdata/make_textedit_alpha.py <directory>`
and check them with `scripts/text_image_check.py <text-edit-probe> <new-ignored-directory>`,
which damages one entry per fixture and requires a refusal, so a generator that
stopped writing an entry cannot pass by doing nothing.

Preserved Form XObjects use an eight-level, 32-call traversal with cycle detection,
a shared 1 MiB decoded form budget and 16,384 operators. Their text stays read-only
and the transformed BBox reserves space against layout expansion. A form may name
the layer it belongs to (`/OC`, an OCG or OCMD dictionary), carry an application's
`/PieceInfo` and its `/LastModified` date; none of the three is painted, and the
editor never resolves the layer state. It treats a form as painted either way,
because reserving the box of a form that turns out to be hidden refuses a layout
that would have fitted, while the reverse would let new text land on visible
graphics. Text state (`Tc Tw Tz TL Tf Tr Ts`) is accepted outside a text object
as well as inside, which is where Acrobat's page-number stamps set it (ISO
32000-1 Table 51); positioning and showing still require the text object.
External forms, soft-mask graphics states and pattern colours remain refused. Indexed eight-bit
images validate palette length and every sample. Page images and preserved forms
share a 32 MiB byte budget (`MAX_IMAGES`, checked before decoding; a screenshot
with its soft mask is ~13 MB). Figure MCIDs may use P stream markers for preserved
graphics; direct figure text remains refused. Artifacts and unmarked additions on
tagged pages keep their bytes and glyph collision bounds without becoming editable.
Embedded fonts accept zero-width holes only when unused, half-em descenders,
nonsymbolic Identity-H descriptors, indirect width arrays and verified empty
ideographic-space glyphs. `empty_glyph` also counts a simple glyph of one
contour with one point as empty, read from the glyf header (YuGothic subsets
carry their space that way). A Type0 font's `/BaseFont` need not repeat its
descendant's: merged documents give it another subset tag, and the name selects
nothing. The descendant and its descriptor must still agree. Runs with deeper descenders carry a minimum physical
box height, used by the default layout before rounding its font size and height.
ToUnicode dictionary capacities from 1 through 256 are
allocation hints; the remaining CMap grammar is unchanged. Synthetic regressions
live beside `forms`, `images`, `tagging` and `fonts`; private round-trip inputs and
outputs belong in ignored directories.
Invisible signature widgets with zero-area rectangles remain read-only and no
longer block form discovery or unrelated saves. Finite-coordinate, ordering and
field-ownership checks still apply; signature-save confirmation is unchanged.
For editing compatibility fixes, verify native Apply and Save as well as the
worker round trip: desktop Save also scans form widgets, which a text-only probe
does not exercise. Check the default layout, signature confirmation and saved
copy through an independent reader; always use disposable private copies.
Run the document round trip on freshly rebuilt final code before release gates,
then repeat Apply and Save with the packaged application. A pass taken before
a later parser restriction does not establish compatibility of the release.

Tagged editing accepts direct or referenced `/RoleMap`, layout `/A` dictionaries
and parent-tree `/Nums` arrays. Structure-element `/Type` may be absent; supplied
values must be `/StructElem`. Reference resolution retains the existing bound,
and every resolved value still passes the same grammar and ownership checks.
`make_textedit_symbolic.py --tagged-indirect` generates the combined fixture for
ordinary worker/native textedit checks and independent structure-graph readback.
Block layout attributes admit bounded numeric StartIndent/EndIndent and
SpaceBefore/SpaceAfter. These authored allocation constraints are retained. Bounded numeric TextIndent
also works on paragraph-like blocks, preserving the first-line origin through
shorter edits. TextAlign Start is accepted; Center/End/Justify pin the block
read-only (see below), and ink bounds remain refused. The unchanged LibreOffice
exports of `textedit-producer-hanging-indent.rtf` and
`textedit-producer-first-indent.rtf` exercise both signs. PDFKit readback uses
`--hanging-indent` / `--first-indent` and checks the authored 12pt line offsets;
the ordinary parser readback also preserves the complete tag graph. The unchanged LibreOffice
export of `textedit-producer-spacing.rtf` exercises the combined attributes;
`scripts/text_edit_producers.py` records its export and readback commands.
Below the structure root, an iterative walk admits Document, Part/Art/Sect/Div and
neutral NonStruct containers, then P/H/H1-H6 blocks with the existing optional
NonStruct or literal Span content leaves. Span leaves retain optional language
tags, accept no layout attributes or semantic overrides, and cannot nest further.
`textedit-producer-language.rtf` exports an unchanged LibreOffice language-span
fixture for ordinary worker/native checks and independent structure readback.
The tree allows at most eight container levels and 1,024
containers, with 4,096 parent-tree slots per page and 16,384 per document. The independent
4,096-node bound includes empty blocks/cells and nameless empty placeholders.
Document elements may have root siblings; all parent ownership remains checked.
Grouping containers own child
elements, never marked content or layout attributes; their Pg is not inherited.
Used role aliases may target Document and supported grouping/heading types.
Unused mappings to other standard roles are preserved; unsupported used roles still refuse editing.
standard PDF 1.7 names cannot be remapped, including types the editor does not
support. The RoleMap guard covers all 49 standard types; valid custom names
remain case-sensitive. `tagging/role_tests.rs` covers unused definitions and
disguised content, with atomic write refusal and preserved custom aliases.
The survey self-test independently generates complete tagged pages to check
both accepted aliases and refused standard-name remaps through the worker.
`--tagged-containers` on the
symbolic generator covers aliases and metadata references. The browser generator's
`--headings` exports an unchanged Document/Art/NonStruct tree with H1/P blocks;
ordinary native textedit checks and PDFKit `--browser` read it back.
Lists admit literal L/LI with Lbl, LBody or neutral NonStruct content leaves;
LBody children that are themselves blocks remain refused. Nested L containers
may occur directly inside LI; each item must retain its own content. The same
iterative queue, depth/count bounds and explicit page ownership apply at every
level. The browser generator uses `--nested-list`; native `textedit` edits the
parent and `textedit-list-child` edits the child. Independent readers use
`--nested-list` / `--nested-list-child` respectively.
List containers share the grouping bounds and may carry one List attribute
object with a standard ListNumbering name, directly or in a singleton array;
references are resolved without changing the saved graph. List-role aliases,
leaf attributes and semantic overrides remain refused. The browser generator's
`--list` fixture runs through the ordinary native textedit phase; independent
parser/PDFKit readback both use `--list` (parser also `--float32`).

Tables admit literal Table/TR/TD/TH with the existing single NonStruct/Span
content-leaf level. Table and row containers share the grouping limits; a table
may own painted borders, but text must belong to a cell. Cell Table attributes
admit integer RowSpan/ColSpan from 1 through 128, Headers and header-only Row/Column/Both Scope, in at
most four dictionaries. IDs are bounded byte strings; the IDTree must enumerate
exactly the visited identified headers. Data-cell links must be unique and point
to headers in the same table. Header-to-header links, cell sizing,
nested tables and table-role aliases remain refused. Cells may contain paragraph
or heading blocks with the same optional Span/NonStruct leaf level; each content
owner retains its explicit page and parent-tree mapping. Span attributes describe
the existing structure; editing does not merge cells or change table geometry.
`tagging/tables.rs` checks name-tree ordering, exact Limits and ownership, with
independent limits of eight child levels, 128 nodes and 128 identifiers. Nothing in
that graph is rewritten. The browser generator's `--table` and `--table-header`
export unchanged tables with rules; ordinary worker/native textedit checks,
parser `--float32` and PDFKit `--table` verify the complete graph, following-cell
position and pixels outside the edit.

Ordinary Word, Acrobat PDFMaker and LiveCycle output needs the following shapes;
each was found by surveying unchanged public documents, and BUILD.md *Prototype
producer sample* has the table, the reasons each is safe and the round trips.
Parent trees split into `/Kids` are flattened after checking Limits, order,
depth and node count. `Link`/`Form` elements own annotations through `OBJR`; the
page's `/Annots`, subtype and `/StructParent` entry must all agree, each entry
is claimed once, and their text stays read-only so link areas stay accurate.
Pages without StructParents, `null` slots and slots naming unreachable elements
are accepted; unowned and orphaned content is read-only, and a reachable
element that skips its slot is still refused. The owning element, not the
content tag, supplies semantics, except that `/Artifact` on an owned MCID is
refused; artifact property lists (Table 330 keys) work inside and outside BT,
as does `/Artifact BMC` (LibreOffice's TOC dot leaders). An artifact without an
MCID needs no structure tree, so an untagged page may carry one (Acrobat's page
stamps on scans); its text is read-only there too (`Tags::read_only` answers
`Some(None)` with true on every page, not only tagged ones).
THead/TBody/TFoot, lists directly in lists or cells, figures in cells, missing
`/K`, figure Width/Height, and the PDF 1.7/2.0 standard namespaces (types common
to both) are accepted. `tagging/producer_tests.rs`,
`annotation_tests.rs` and `tree_tests.rs` hold the synthetic fixtures.

An element *pins* its content (read-only, through `Tags::bounded`) when it keeps
metadata describing that content as it stands: `/Alt` on any element (it
covers every descendant), a non-empty `/T` on an element owning text, a
non-Start `TextAlign`, or a table `/BBox`. `element()` returns the pin with the
page; the walk ORs it into `bounded` and `Group::pinned` carries it through
`groups` and deferred sublists. Pinned text still needs validated glyph outlines
(`read-only text requires validated glyph outlines`), so a standard-14 font
cannot be pinned. `/ClassMap` classes named by `/C` (name or array of at most 8,
each optionally followed by a non-negative revision) are validated by the same
`attributes()` as `/A`; at most 256 classes; TD/TH may not use `/C`. Layout
attributes may omit `Placement` and carry `LineHeight` (number >= 0, Normal,
Auto); inline Span/NonStruct leaves accept only `O` + `LineHeight`. `TOC`/`TOCI`
group blocks (TOC holds TOCI or TOC; TOCI sits in a TOC). `Form` may carry
`O /PrintField` with standard Role/checked/Desc. `tagging/pinned_tests.rs` covers
these. Flatness (`i`, ExtGState `/FL`, 0-100) and smoothness (`/SM`, 0-1) are
preserved rendering tolerances (`graphics::tolerance`).

Text render modes 0-3 are accepted (fill, stroke, both, invisible; OCR text
layers are mode 3). Mode and line width are graphics state, saved by `q` and
restored by `Q`; `w` and an ExtGState `/LW` set the width. A stroked run (1, 2)
needs a solid stroke colour, and its hit box and ink grow by half the width
times the CTM scale, which clips, compound clips and the layout editor
(`layout::Context.stroke`) all see. Modes 4-7 add to the clipping path and are
refused.

Layout attributes are also accepted on Figure, Link, Form and Table, where
`/BBox` is optional and `/Placement` may be any of the five standard names.
ISO 32000-1 Table 344 makes `/BBox` the element's own ink rather than an
authored allocation, so keeping one is only sound while the content it
describes cannot move: a figure, link and field are read-only already, and a
table that declares bounds makes its own cells read-only too. `Tags::bounded`
carries those MCIDs, and text beside such a table stays editable. A table that
declares only a placement changes nothing. A list may be nested inside an
`LBody` as well as beside it, which is what Word and Acrobat export; the
sublist goes back to the walk that owns the depth and container bounds rather
than recursing in `groups`. `Note` joins the grouping containers, so a footnote
or endnote holds ordinary blocks and a producer role mapped onto it is accepted;
one that owns marked content directly is still refused.

WinAnsi TrueType fonts with ToUnicode may map WinAnsi punctuation at its own
code (0x82-0x9F except 0x80, plus Latin-1 except 0xA0/0xAD); glyphs come from
the (3,1) cmap by Unicode value, and a Macintosh cmap need only agree for ASCII.
The same (1,0) agreement rule admits a Mac cmap beside (3,1) without ToUnicode.
Metric slots reuse the WinAnsi codes, so the minus keeps 0x80 and the euro sign
stays unsupported. Two-byte fonts keep their Latin-1 and en dash repertoire.
A continued run's TJ compensation is an exact integer plus an f32 remainder,
because lopdf stores reals as f32 and Word shows whole lines at `Tf 1`.
`fonts/winansi_tests.rs` builds its cmaps in the test.

Positive word spacing accepts values up to one million text-space units, with
the combined advance bounded separately. Negative spacing retains its quarter-
font-size limit. Single-byte PDF code 32 receives `Tw`; mapped spaces at other
codes and two-byte codes do not. Replacement ink, advance and continuation checks
still apply. `make_textedit_symbolic.py --unit-font --word-code space --word-spacing
12.112` generates the `Tf=1`, scaled-matrix case; PDFKit `--wide-spacing` checks
the gap's absolute position as well as unchanged pixels outside the edit.
The native `textedit-wide-spacing` phase checks the geometric column order that
this untagged gap produces, including selection refresh and undo. Discovery also
requires deletion compensation to fit PDF number precision for every continued
run; replacement-specific compensation is checked again by the writer.

Composite font discovery resolves an indirect `/DescendantFonts` array through
`encoding::resolve`, retaining the original array and font resources on save.
The composite-font regression covers matching direct/indirect geometry, saved
text and cyclic or non-array references.
Identity-H TrueType fonts also admit the exact `ff`, `fi`, `fl` and `ffi`
ToUnicode sequences already supported by CFF. Source measurements retain original
CID boundaries; replacement encoding chooses the longest available ligature.
That slot path (`mapping::parse_cid`) still refuses any other sequence. The
Unicode path (`mapping::unicode_codes`, composite and Type3 fonts) admits any
unique run of two or three letters (Calibri's `ft`, `st`, `Th`), since its
encoder matches the longest mapped sequence; a digit, space or control in a
sequence, duplicate targets and sequence ranges remain refused. A CFF font's
`Differences` may also name ligatures by Adobe's original `fi`/`fl`/`ffi`
(`fixtures/legacy-ligatures.cff`, from `testdata/make_cff_legacy_ligatures.py`).
The Unicode path also admits individual compatibility
characters when their embedded glyphs validate. Expanded text retains the normal character bound.
Generate with `testdata/make_textedit_composite.py <path> --ligatures`;
`text-edit-probe`, `make_textedit_embedded.py --check` and
`text_edit_pdfkit.swift` accept `--cid-ligatures`. The native check reuses
`tabs_check.py --phase textedit-cff-ligatures` because its text workflow is shared.

`textedit/refusal.rs` explains unsupported operation contexts using fixed PDF
keywords; it never echoes operand values or unknown document tokens.
`scripts/textedit_survey.py` reports `refusal_totals`, counting only the first
refusal per page. Its contained `--self-test` covers specific inline-tag errors,
continued discovery after refusals, report completeness and aggregation.
Tagged-structure refusals distinguish unsupported metadata, role mappings,
parent-tree ownership and marked-content context. Only fixed PDF keywords may
be named; unknown keys, role names and all values stay out of errors. The survey
self-test checks this through the worker. These reports identify first blockers;
removing one refusal does not imply that a page becomes editable. Re-survey
unchanged source bytes after widening the supported profile.

Type3 fonts admit bounded, uncoloured `d1` glyph programs composed only of
filled straight/Bezier outlines, with diagonal FontMatrix, consistent Widths
and an unambiguous one-byte ToUnicode map. Glyph control-point hulls bound ink;
FontBBox alone is not trusted. Original glyph programs remain unchanged.
Original-font replacements use validated glyphs already in that subset;
automatic layout can use the bundled CJK fallback for new characters.
External glyph resources, coloured/stroked glyphs,
recursive programs and other operators remain refused. Each font shares a
1 MiB/16,384-operation budget, with a 64 KiB per-glyph bound. The `d1` header
is validated and normalized only in a scratch buffer because lopdf's strict
reader splits that operator into `d` and `1`. `fonts/type3/tests.rs` covers
the grammar; `scripts/text_type3_check.py` independently generates and reads
positive/negative font matrices, word spacing, replacements, deletion and layout
through the contained worker, including unchanged font programs and pixels.

Font refusals distinguish the Type1 resource subtype from its program carrier:
FontFile declares PostScript Type 1; FontFile3/Type1C declares CFF. Parent-tree
dictionary entries index annotations; an entry no Link or Form element claims is
reported separately from a missing page entry.

Embedded Adobe Type 1 programs (`FontFile`, what pdfTeX, dvipdfm and older
Distiller write) are read by `fonts/type1/program.rs` without executing
PostScript: the cleartext for `FontMatrix` (must be 0.001), `FontType` 1,
`PaintType` 0, `FSType` and a `StandardEncoding` or literal `dup n /name put`
built-in encoding; the eexec part (binary only, `lenIV` -1 to 4) for `Subrs` and
`CharStrings`, RD/ND/NP spelled either way. Each charstring runs in a bounded
interpreter (24-deep stack, 10-deep subroutines, 65,536 operations, flex and hint
replacement through OtherSubrs 0-3) for its `hsbw`/`sbw` advance and control-point
hull; `seac`, other OtherSubrs and anything unknown leave that glyph unoffered.
`fonts/type1.rs` maps codes to names through `Differences` over the built-in,
WinAnsi or Standard base and to slots by AGL name (ASCII, WinAnsi 0x82-0xFF,
minus, `fi`/`f_i`-style ligatures including `ffl`). A code is offered only where
its glyph exists, its width is non-zero and agrees, and a present ToUnicode agrees
with the name (`mapping::parse_names` accepts pdfTeX's own CMap and collection
names and narrows rather than refuses). Any other validated glyph (math symbols,
letters outside Latin-1, a TeX math-italic width that includes its italic
correction) is *opaque*: it measures source text at its PDF width, marks it
with U+FFFD, and its run stays read-only with its ink reserved, up to 4 em from
the origin (`OPAQUE_REACH`; TeX's largest delimiters hang 2.4 em down). `type1/tests.rs` builds
its fonts in the test, charstrings and eexec encryption included.

In a font that cannot write a space, a `TJ` displacement of at least 0.18 em
between two strings reads as a space and a replacement writes each space as a
displacement of the run's mean gap (`Metrics::items`/`gapped_layout`, shared with
the layout editor; spaces with no word on one side are refused). One leading `TJ`
number is where the run starts: it moves the origin and is written back unchanged.
`docs/TRAPS.md` has why for both. Constant alpha (`ca`/`CA` in 0-1) is accepted in
ExtGStates, since an edit keeps the state; blend modes and soft masks stay refused.
Preserved forms accept pdfTeX's `PTEX.FileName`/`PageNumber`/`InfoDict`.

A replacement in a `TJ` run keeps the source's own items for its unchanged start
and end (`kerning.rs`): glyph bytes, kerns and word gaps are copied, only the
changed middle is encoded, and a kern between a kept and a changed glyph is
dropped. The candidate is read back through `array_text` and used only if it
reads as the replacement and fits; otherwise the run is rewritten whole. On the
arXiv sample this takes transposition edits from 24 to 69 of 80 lines.

A non-embedded simple TrueType or Type 1 font (Word leaves Arial and Times New
Roman out) is read by `fonts::unembedded`: nonsymbolic WinAnsi only, measured by
its PDF `Widths` over printable Latin-1, with the descriptor's `FontBBox` as the
ink of every glyph so its text can be kept read-only. Readers substitute the
shapes and position by those widths, as they do for standard Helvetica.

Every one of the twelve Latin standard fonts (Helvetica, Times, Courier, each in
four styles) is edited the way Helvetica always was: no descriptor, WinAnsi or
the built-in StandardEncoding, measured by Adobe's metrics from
`fonts/standard.rs`. That table is generated by `scripts/standard_font_widths.py`
from ReportLab's and pdfminer.six's transcriptions, which must agree on all
12 x 191 widths; its Helvetica row is also checked against `textbox.rs`, which
`annot-probe` checks against PDFium. Every arXiv paper's side stamp is set in
unembedded Times-Roman. Symbol and ZapfDingbats stay refused.

A replacement's right ink edge may exceed the source's by 1e-6, the same
allowance as the advance: the scan and the layout sum a run's widths in
different orders, so an equal-width edit landed at 337.74 against
337.73999999999995 and was refused. The left edge and the kept-kerning path
sum nothing differently and get no allowance.

Untagged pages may retain a bounded integer StructParents index with no
StructTreeRoot. Preserve that unused index; actual MCIDs without a tree remain
refused. Axial shading patterns with bounded type-2 interpolation functions can
paint preserved paths; tiling patterns remain refused. Pattern-filled text,
skewed or mirrored text matrices and text under a non-diagonal page CTM stay
read-only, with validated embedded-font
bounds retained for layout collision checks. Its text, positioning and resource
bytes remain unchanged when ordinary text elsewhere on the page is edited.
Painted paths and rectangles are accepted under any affine CTM (only their
points' range is checked, TikZ rotates drawings with `cm`), and a moveto may
start a subpath that draws nothing (TikZ's `m m ... h m S`) as long as the path
draws a segment. Clips under a non-diagonal CTM remain refused. See `textedit/patterns.rs` and
`textedit/preserved_tests.rs`; private-document checks stay in ignored directories.


Visual signatures use a bounded RGBA raster (`signature.rs`, `signature.ts`) on
`MarkKind::Signature`; PNG/JPEG decoding stays in the webview. Pixels are shared by
`Arc` in the journal, limited to 512x256 pixels (including rotated equivalents)
per image and 4 MiB across retained marks. The worker writes a PDF Stamp appearance
with an RGB image and alpha soft mask. Placement stores inverse-rotated pixels in
the mark's original display space, so later page turns rotate the image with it.
`SignatureDialog` remembers pixels only on explicit opt-in, in macOS Keychain or
user-scoped Windows DPAPI storage. `signaturestore.ts` migrates the legacy
localStorage entry only after protected readback matches. PNG/JPEG headers are
validated before decoding: at most 10 MiB compressed, 8 megapixels and 8192 pixels
per dimension; animations and changing dimensions are refused.
`tabs_check.py --phase signatures` checks the real application; the independent
PDFium and PDFKit pixel readers and fixture commands are in `BUILD.md`.
This creates visual marks only; it does not create certificate-based signatures.

Before any save/copy writing command, the application reads digital-signature
metadata from the worker and asks for consent if signatures/certification exist
or enumeration was incomplete. DocMDP permission to fill forms does not bypass
the warning: the current rewrite cannot preserve that cryptographic history.
Cancellation reaches no writing IPC. This is a warning, not signature validation.

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
Restoration keeps the saved absolute point until the target page's lazy geometry
arrives; ordinary scrolling over estimated pages still keeps its relative position.
Fit uses the visible sheet, which can differ from the page at the viewport's top.
`tabs_check.py --phase tabs-position` checks repeated tab switches at nonzero offsets
and at the final page under fit-page, fit-width and fixed zoom.
View/page rotations and page reordering retain the fit target across layout changes;
`tabs_check.py --phase tabs-rotation` checks the native view-rotation commands
against an explicit return to the same sheet and refit.

Windows workers join their cleanup job during process creation through
`PROC_THREAD_ATTRIBUTE_JOB_LIST`. Do not restore a separate post-creation
assignment: parent termination in that interval leaves a suspended orphan.
The sandbox regression kills a helper parent at that boundary. Native tab,
form, signature and signed-save checks also verify worker exit externally with
`scripts/win_worker_exit.py`; its controls run in Windows CI and release validation.

Windows orphaned test workers can hold the release executable open while CIM
returns no executable path for them. Restart Manager with that exact executable
registered as a resource identifies its holders. Never clean up by image name:
the installed application may be running alongside the test build.

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

The PDFium pin is `pdfium-8044-tpdf.1`, installed by `scripts/fetch_pdfium.py` and
verified by digest. TPDF builds the 8044 source with `scripts/pdfium_rtl.patch`
through `.github/workflows/pdfium.yml`; archives carry source/toolchain provenance,
the patch and licensing notices. The correction restores seven ordinary RTL
extraction regressions while preserving upstream ActualText behavior; two known
mixed-direction limitations remain. The report is PDFium issue 561066233.
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

**Since 2026-08-22 the worker also *writes* with `lopdf`.** `Request::Append` builds the update
section for a save that only adds marks, because doing so is a pure function of the document's
bytes and the plan — and those bytes are the attacker's. It runs where every other parse of
them runs, which narrowed `docs/THREAT-MODEL.md` residual risk 18 from every writing path to
the rewriting ones. The split is by authority: `save::append_ready` asks the coordinator's
questions about a path, `save::append_update` asks none.

**Since 2026-08-28 the in-place *rewrite* runs there too**, which is what took *delete a page
and press ⌘S* off that risk. The obstacle was never the input: it was that a rewrite's answer
is the whole document, against a 32 MB reply limit and files ten times that. So the worker is
handed an **output channel** — the staging file's own descriptor, on `worker::OUT_FD`, given
at `exec` — and writes down it. `save::rewrite_update` is the pure half, `save::Rewriter` the
seam, and `save::Outside` names the one choice both seams read. The coordinator holds neither
the document's bytes nor the new file's; what crosses back is a length, which it compares
against the staged file's own size.

That the channel survives the sandbox was measured rather than assumed: the profile says
`(deny file-write*)`, which denies *opening* a path and not a descriptor handed over before
it. `worker-probe` writes one document both ways and compares byte for byte.

**The copy paths, the split and the print job followed on 2026-09-01**, on the same seam —
`Job` carries what a print does not share with a save, so `staged_rewrite` is one function.
**The page-range print and the merge followed on 2026-09-01**, through
`Request::PrintRange` and `Request::Merge`. The merge is the widest of them — it parses files
the reader picked in a dialog that tpdf never opened — and the obstacle recorded for it was
wrong in a way worth keeping: the threat model said each incoming file's object graph would
have to come *back*, when nothing comes back per file. They go **in**, as one read-only
mapping on `worker::IN_FD` with `save::Incoming` naming each, and the merged document goes out
down the channel a rewrite already had.

⚠ **The last one was a *reader* residual risk 18 never listed: `verify::scan`.** The
redaction verification parses the file it just wrote, and it was invisible to that risk, to
`docs/THREAT-MODEL.md` §3 and to `scripts/check_writers.py` alike, because all three enumerate
what **writes** — the same blind spot that hid `print::build`, twice in two days. The index
has the trap.

**It moved on 2026-09-01, through `save::Verifier` and `Request::Verify`, so on both shipped
platforms no `lopdf` parse of a document happens in the coordinator at all.** `save::Here`
remains the exception and is what a platform with no sandbox gets. Two properties of that move
are not guessable from the feature: the scan needs the reader's password, because a redacted
copy of an encrypted document is re-encrypted and a worker without the key parses no objects
and finds nothing — an absence that reads exactly like a clean file; and a report is now a
reply read under `MAX_REPLY_BYTES`, so `verify::MAX_OBJECT_REASONS` bounds its per-object lists
and counts the rest. The index has that second one too.

**Windows is wired the same way and is measured**: `worker-probe` is a step of both CI legs,
and it reported **45/45 with 0 not applicable to this platform** on run 33626718480, which is
the current `main`. That supersedes the 34/34 recorded here from run 33501693368, and closes
the caveat that stood beside it — the checks added since have now been through a Windows leg.
Read the count from the run rather than from this line: it is a fact about a build, and the
sentence it sits in has been stale once already.

**Since 2026-08-23 a reader can open a document behind a password.** Until then an encrypted
PDF could be chosen from the file dialog and then not opened by any route — `open_failure`
said so, in a sentence ending *"and tpdf cannot ask for one yet"*, which is a to-do that
reads as a decision. The worker asks and retries the load **in place**, which is legal
because a failed load poisons nothing: measured on `testdata/incr-encrypted-pw.pdf`, four
loads of one buffer in one process open on both correct passwords and refuse on both others.

Two consequences are not guessable from the feature. **PDFium answers the same error for a
document given no password and one given the wrong password**, so the sentence a reader sees
on a retry is chosen in `worker_child::unlock`, the only place that knows one was tried. And
**the password is held for the document's lifetime on `Held::password`**, because every
worker after the first — pool growth and crash replacement alike — maps the same bytes and
meets the same encryption; without it a locked document renders the page a reader is looking
at and refuses the next. `docs/THREAT-MODEL.md` §T6.9 states what holding it costs.

**Since 2026-08-23 a reader can also save a mark onto one, and since 2026-08-28 a rewrite
too.** An append never touches the previous revision, and `IncrementalDocument::save_to`
encrypts each appended object with the key the load recorded; a rewrite goes through
`lopdf`'s full serialiser, which writes every object in the clear and drops the `/Encrypt`
dictionary with it — so `save::rewrite` takes the encryption state off the document before
it touches anything, and calls `Document::encrypt` back on as its **last** step, after the
sweep and after everything that adds an object. `examples/password_probe.rs` runs the append
end to end (986 bytes appended to a 2,346-byte AES-256 document, reopened afterwards with the
same password and refused without it); `examples/encrypted_rewrite_probe.rs` is the rewrite's,
through `qpdf` rather than through the writer that produced it.

⚠ **This paragraph said the opposite for a day, and the stack table above said the truth —
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
comments, links, properties and character mapping all came back empty — and empty is the
reassuring answer. `links.rs` and `annots.rs` already carried a `pages_missed` count for
exactly that, which is what `password-probe` asserts against; the comments check exists
because taking the password away from `annots::scan` reddened nothing without it.

**Comments, links and a document's own properties are read through `lopdf`, not through
PDFium, and that is a measurement rather than a preference.** `FPDFPage_GetAnnot` and friends work — checked on a fixture before
anything was written — but every one of them needs an `FPDF_PAGE`, and `FPDF_LoadPage`
re-parses each time at up to 44 ms on a complex page. The panel's question is about the whole
document, so through PDFium it is a page load per page; through the object graph it is one
parse the file already needs for `encoding.rs`, at 0.1 ms on a small document and 11.9 ms on
the 337 MB scan. Since 26.9.2 it is one parse in fact as well as in cost:
`docgraph::DocumentGraph` holds it and six readers share it, and `DocumentGraph::parses` is
the observable that says so. `pdfium-render` also does not expose `/IRT` at all, so a reply arrives there
as an unrelated second note by another author.

**Links take the same route, and it costs a second destination resolver** —
`outline.rs` asks PDFium because a bookmark is a PDFium object, `links.rs` reads the
destination array itself. That is the drift trap this file's index names, so `links.pdf`
gives its outline entries the same destinations as its links and `links-probe --mode agree`
compares them, both against the manifest rather than against each other. The properties
readout in `docinfo.rs` takes the `lopdf` route too, and since 2026-08-21 also parses the signer's
certificate — a second ASN.1 parser on attacker-chosen bytes, bounded and sandboxed
accordingly (`docs/THREAT-MODEL.md` §T6.8). `examples/signature_probe.rs` is the differential
against PDFium's own reading of the same file; `BUILD.md` has the invocations.

Two things in that module are traps rather than choices, both in the index: `lopdf::decrypt`
removes the `/Encrypt` trailer entry, so the encryption has to be read **before** it; and the
permission bits do not mean the same thing at every revision, because bits 9 to 12 are
reserved under revision 2 and a negative `/P` sets all four.

**What PDFium does supply is the marks themselves**, and that is why no drawing was added.
`progressive.rs` renders with `FPDF_ANNOT`, so a sticky note's icon and a highlight's wash are
painted inside the tiles — measured on a fixture carrying no appearance streams at all,
where PDFium generates them: the note icon fills 637 of the 756 pixels in its own rectangle,
the highlight 6,690 of 9,436, and a `/Popup` correctly draws nothing. What no reader could
reach before `annots.rs` was the *text*.

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

**`fax` (MIT, pdf-rs project) was added 2026-09-18 and brings exactly one package** — its
derive crate is behind a feature that is not enabled. It decodes the CCITT Group 4 stencil
masks of scanned pages inside the worker, as one more parser of attacker-chosen bytes;
every mode it reads consumes input bits, so its work is bounded by the encoded length,
which `images/stencil.rs` caps like any encoded stream. Both lockfiles carry it: the fuzz
package resolves the application by path.

Three plugins are linked. `tauri-plugin-dialog` (Apache-2.0 OR MIT) for the file-open and
file-save dialogs, which pulls `tauri-plugin-fs` (Apache-2.0 OR MIT) and `rfd` (MIT) — the
capability list in `src-tauri/capabilities/default.json` names `dialog:allow-open` and, since
2026-08-16, `dialog:allow-save`; that second one opens a panel and writes nothing, and what
actually writes is `save_copy` and, since 2026-08-19, `save_document`, whose authority `docs/THREAT-MODEL.md` §T6.1 states; on Windows only,
`tauri-plugin-single-instance` (Apache-2.0 OR MIT), which is what gives that platform the
document handover macOS gets from `RunEvent::Opened`; and `tauri-plugin-updater` (MIT OR
Apache-2.0), which is the largest single addition the tree has taken — **48 crates,
325 to 373**, because it brings a TLS stack (`rustls`) and archive extraction (`zip`, `tar`).
All permissive, swept as below.

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

Currently, in the order `--list` prints them: a toolchain-pin check, a PDFium pin check, a trap-index check, a
future-date check, a
workflow-parity check, a workflow-fixture check, a mutation-anchor check, a mutation-suite check, a
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

`.github/workflows/release.yml` fires only on a CalVer tag and **invokes `scripts/gates.py`**
rather than re-listing commands in YAML. The one part with no precedent in the portfolio is
signing the bundled `libpdfium.dylib`: notarization requires every Mach-O in the bundle to carry
a Developer ID signature and the hardened runtime, so the dylib is signed in `vendor/` *before*
the bundler copies it. Its verification step is written to fail rather than warn — a skipped
notarization exits 0 and produces an app Gatekeeper rejects. The tag glob matches an `-rcN`
suffix so a rehearsal is possible, and a failed run publishes nothing, since `release` needs
`gates` and the release is created as a **draft**. It took four rehearsal tags to get there, each
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

Short fuzz runs must advance beyond corpus initialization; compare `INITED` and
`DONE` execution counts as described in `BUILD.md`, *Fuzzing*.

Every *measurement* in this file is macOS arm64 unless it says otherwise. The two
platforms differ enough — on pre-spawn cost, on render constants — that carrying a macOS
number over is a guess rather than an estimate, so a Windows figure is always labelled.

**The render constants are measured on both platforms.** `tile-bench` and `pool-bench` run
on Windows, and `docs/PLAN.md` §4's four architectural consequences reproduce there: the ratios
that drove the architecture hold, and every absolute number is **1.5--1.8x worse** than macOS,
so a latency budget written against the macOS figures is optimistic here by about a third.
`BUILD.md` has both tables and the caveats.

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
