# Existing-text editing: what the editor admits

Moved verbatim from `AGENTS.md` on 2026-09-24, where it sat under *Stack* and was loaded
before every task. `AGENTS.md` keeps one index line per topic. Code comments and other
documents that say "`AGENTS.md` records ..." about text editing mean this file.

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
are preserved, with one optional `Tm`/`Td` and one space-only `Tj`/`TJ` of one to
as many spaces as separators (InDesign shows a single space for a run of tabs). The
same span with nothing inside it, between text objects, is kept as it is: InDesign
writes a tab stop that way. They
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
and each top-level form's tree shares 8 MiB of decoded content and 524,288
operators (`forms::MAX_FORM_CONTENT`, `MAX_FORM_OPERATIONS`). A form is only read,
so it may exceed the 1 MiB a patched page stream gets: pdfTeX includes a plotted
figure as one form, and an arXiv figure of 4.7 MB and 288,594 operators took the
worker's peak from 41 MB to 181 MB on Windows. Their text stays read-only
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
images validate palette length and every sample. Page images, preserved forms and
the images those forms draw share a 32 MiB byte budget (`MAX_IMAGES`, checked
before decoding; a screenshot with its soft mask is ~13 MB). An image inside a
form was charged to the form's content bound until 2026-09-18, which refused a
figure of 29 small rasters (12.3 MB decoded) that the page budget holds. Figure MCIDs may use P stream markers for preserved
graphics; direct figure text remains refused, and a Span inside a figure, PowerPoint's
text in a shape, is pinned. Artifacts and unmarked additions on
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
and parent-tree `/Nums` arrays. `Tags` also keeps each MCID's owning block element
(a paragraph, heading, item or cell, above its Span leaves; a link inside a
paragraph is the paragraph's), which is how a wrap finds its paragraph's lines on
a tagged page (`textedit/layout/wrap.rs`, `docs/PLAN.md` §7). Structure-element `/Type` may be absent; supplied
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
Used role aliases may target Document and supported grouping/heading types, and
Figure (PowerPoint's Diagram and Chart); a figure alias's group carries the mapped
role, so its attributes are a figure's and text under it is refused as figure text.
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
references are resolved without changing the saved graph. A label or body may
carry layout attributes, checked like a paragraph's (PowerPoint writes them on
every item); an item itself may not. List-role aliases and semantic overrides
remain refused. The browser generator's
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
owned sequences may carry a validated `/Lang` beside the MCID (InDesign's paragraphs),
an owned sequence with no operator at all is kept (InDesign's empty paragraph) while
one holding only operators that paint nothing is refused,
as does `/Artifact BMC` (LibreOffice's TOC dot leaders). An artifact without an
MCID needs no structure tree, so an untagged page may carry one (Acrobat's page
stamps on scans); its text is read-only there too (`Tags::read_only` answers
`Some(None)` with true on every page, not only tagged ones).
THead/TBody/TFoot, lists directly in lists or cells, figures in cells, missing
`/K`, figure Width/Height, and the PDF 1.7/2.0 standard namespaces (types common
to both) are accepted. `tagging/producer_tests.rs`,
`annotation_tests.rs` and `tree_tests.rs` hold the synthetic fixtures.

An element *pins* its content (read-only, through `Tags::bounded`) when it keeps
metadata describing that content as it stands: `/Alt` or `/ActualText` on any
element (each covers every descendant; InDesign sets ActualText on the Span of a
forced line break), a non-empty `/T` on an element owning text, being a child of a
Figure, a
non-Start `TextAlign`, or a table `/BBox`. `element()` returns the pin with the
page; the walk ORs it into `bounded` and `Group::pinned` carries it through
`groups` and deferred sublists. Pinned text still needs bounded glyphs
(`read-only text requires validated glyph outlines`): outlines, a descriptor's
FontBBox, or for a Latin standard font the Adobe FontBBox in `fonts/standard.rs`. `/ClassMap` classes named by `/C` (name or array of at most 8,
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
sequence and sequence ranges remain refused. Several codes may share a text (a
small capital and its capital, a delimiter's sizes): each reads as that text, and
a replacement writes it only with the one glyph its own run already shows for it
(`unicode::Metrics::prefer`, fed by `textedit::shown`); otherwise the edit is
refused with "several glyphs for this character". A CFF font's
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
Both writers get their items from `textedit::own_items`, and the editor's layout
(which every edit in the application sends) uses them for a one-line replacement
at the run's own font and size, placed at its own origin and held to the box and
the source's own ink (`layout::source_items`); ink within the source's is not
held to a clip the source already had. A requested size within 0.001 pt of the
source's is the source's (`layout::own_size`), since `defaultTextLayout` rounds
it up. Anything else is laid out from glyph widths as before.

A non-embedded simple TrueType or Type 1 font (Word leaves Arial and Times New
Roman out) is read by `fonts::unembedded`: nonsymbolic WinAnsi only, measured by
its PDF `Widths` over printable Latin-1, with the descriptor's `FontBBox` as the
ink of every glyph so its text can be kept read-only. Readers substitute the
shapes and position by those widths, as they do for standard Helvetica.

A simple font the editor cannot write with -- a program it does not validate, a
character map it cannot read, embedding rights that forbid editing -- no longer
refuses the page when `fonts::read_only` can measure it: Type 1, MMType1 or
TrueType with `FirstChar`/`LastChar`/`Widths` and a descriptor whose `FontBBox`
(`fonts::font_box`, shared with `unembedded`) is ordered and within 4000 units.
Every code is then opaque, advanced by its `Widths` entry or `MissingWidth` and
inked to the box, so its text is read-only with that ink reserved and the other
text on the page edits. The program is never read. A page left with nothing to
edit is refused with the first such font's own reason, not *page contains only
read-only text*. Composite and Type 3 fonts keep their own refusals.
Symbol and ZapfDingbats named with no program and no key but `Type`, `Subtype`,
`BaseFont` and `Name`, as ReportLab sets bullets, are measured the same way by
`fonts::symbolic` from `standard::SYMBOLIC`: each code of the built-in encoding with
Adobe's width, and a code the encoding leaves empty refuses as unmapped.
`scripts/standard_font_widths.py` generates that table only when ReportLab's width
and encoding tables and matplotlib's AFM files name the same glyph with the same
width for every encoded code.

Every one of the twelve Latin standard fonts (Helvetica, Times, Courier, each in
four styles) is edited the way Helvetica always was: no descriptor, WinAnsi or
the built-in StandardEncoding, measured by Adobe's metrics from
`fonts/standard.rs`. That table is generated by `scripts/standard_font_widths.py`
from ReportLab's and pdfminer.six's transcriptions, which must agree on all
12 x 191 widths; its Helvetica row is also checked against `textbox.rs`, which
`annot-probe` checks against PDFium. Every arXiv paper's side stamp is set in
unembedded Times-Roman. Symbol and ZapfDingbats stay refused. The same table
holds each font's FontBBox, the union of pdfminer.six's and the AFM files
matplotlib ships (they differ for oblique Helvetica and every Courier). It is used
only for read-only text, which reserves the box, widened by its larger side at
both ends of the advance; editable text keeps the width-only checks, because a
box edge past every narrow glyph would refuse ordinary edits.

Shapes from the second producer sample (Chrome, PowerPoint, XeLaTeX, ConTeXt,
Typst, ReportLab, afp2pdf, Microsoft Print to PDF; `BUILD.md` *Producer sample,
second batch*):
- `/A` may be an array of up to 8 attribute objects, each checked alone; revision
  numbers stay refused. `WritingMode` is accepted only as the default `LrTb`.
  A list label may be `Placement /Inline`, and a `BBox` on one pins its text
  (`tagging::bounding_box`). Figures, links, fields and tables accept the block
  indents and spacing.
- A figure may hold figures (`goes_back_to_the_walk`), like a list body a list.
- `/OC /name BDC ... EMC` in page content (a layer; PowerPoint puts each slide's
  background in one) is accepted when the name resolves to an OCG or OCMD in
  the page's `/Properties`. Text inside is read-only, since the layer may be
  off, and no marked content may open inside it.
- `Tf` and `TL` are text state (ISO 32000-1 Table 51) and are accepted before
  `BT`, as Typst writes them; `q`/`Q` save both.
- The Unicode ToUnicode path accepts a CMap's own name and `CIDSystemInfo`
  labels, as the Type 1 path does for pdfTeX; ConTeXt writes `/Registry (TeX)`.
- A `TJ` whose string ends before an earlier one did, or whose trailing number
  pulls the cursor back behind the last string, backtracks: that run is kept
  read-only instead of refusing the page (ConTeXt sets footers this way). The
  cursor may pass behind the origin within the same 1,000,000-unit bound.
- A composite font's `CIDSystemInfo` strings may be indirect (Microsoft Print to
  PDF).

CID-keyed CFF (`CIDFontType0` with `FontFile3 /CIDFontType0C`, what xdvipdfmx,
LuaTeX and Typst embed) is read by `fonts/cff/cid.rs` under Identity-H and
Adobe-Identity-0 only. The PDF code is the CID; the program's charset maps it to
a glyph (xdvipdfmx and LuaTeX subsets keep the original CIDs, so CID is not the
glyph index), and a CIDToGIDMap is refused. ttf-parser reads the charset and
outlines of such a font but returns no widths, so `cid::width` reads the operand
before a charstring's first stack-clearing operator against FDSelect's Private
dict (`nominalWidthX`, `defaultWidthX`); a subroutine call before it leaves the
glyph unoffered. Top, font and Private dicts are closed key lists; the top
FontMatrix must be absent or 0.001 and a font dict's absent or identity. ForceBold
is accepted here, a hint for the whole glyph set. xdvipdfmx maps CID 0 to U+FFFF
and fontspec's manual shows it: `.notdef` is measured for read-only text only.
In the whole Unicode path (TrueType Identity-H, Type3 and CID CFF), a glyph whose
program width disagrees with its PDF width, or whose ink reaches past the editable
box, is kept read-only at its PDF width with its ink reserved (up to
`type1::OPAQUE_REACH`), where it used to refuse the font; LuaTeX writes TeX's
italic correction into math widths. A PDF width of zero (Typst's combining macron
under `DW 0`) is such a read-only mark too, the only glyph whose layout step may be zero.
The ToUnicode label grammar
(`mapping::labels`) takes `CMapName`, `CMapType` 0-2, `CMapVersion`, `WMode 0`
and `CIDSystemInfo` in any order, each once, the last also as Typst's
`3 dict dup begin ... end def`; lopdf refuses a comment at the very end of a
stream, so `blocks_with_header` appends a newline (Typst ends with `%%EOF`).
xdvipdfmx's simple Type1C fonts that are symbolic or have no PDF `/Encoding` (TeX
math) go through the Type 1 rules (`type1::compact`, `cff::named`), reading the
program's built-in encoding in format 0 or 1 (`cff/encoding.rs::builtin`;
Expert and supplements refused); a nonsymbolic one with an `/Encoding` keeps
the WinAnsi CFF path. Typst's TrueType subsets carry no OS/2 table; `fonts::face`
accepts a program without one as unrestricted, like a Type 1 or CFF program with no
`FSType`, and a present table's restrictions still refuse (`docs/THREAT-MODEL.md`
residual risk 23).

A `cm` inside a text block is accepted before the block's first show. ISO
32000-1 Figure 9 does not list it there, but arXiv's newer stamp is
`BT 0 1 -1 0 0 0 cm ... Tm ... TJ ET`, which every reader applies; geometry is
taken from the CTM at each show, so nothing measured moves. After a show it is
refused. Such a stamp is read-only (its CTM is not diagonal).

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
