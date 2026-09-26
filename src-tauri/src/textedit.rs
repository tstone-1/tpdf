//! Conservative content-stream text editing, executed in the document worker.
//!
//! Supported text uses Helvetica with WinAnsi/default encoding or validated
//! embedded TrueType/CFF glyphs. Continued shows retain their original advances.
//! Font/leading setup may precede a text block.
//! Complete painted rectangles, straight-line strokes and bounded opaque images are preserved, and
//! bounded character spacing is retained. Other graphics and custom text state are refused.
//! Addresses refer to decoded operators, never PDFium's text-object ordinals.

mod actual;
#[doc(hidden)]
pub mod blocks;
mod clipping;
mod colors;
mod filters;
mod fonts;
mod forms;
mod graphics;
mod grouping;
mod images;
mod kerning;
mod layout;
mod patterns;
#[cfg(test)]
mod preserved_tests;
mod refusal;
mod spacers;
mod streams;
mod tagging;

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CONTENT: usize = 1024 * 1024;
// Image samples have their own bounded budget; ordinary photographs are much
// larger than page operator streams. Charge preserved forms against it too.
// 32 MiB holds a full-page screenshot with its soft mask (a 2264 x 1440 RGB
// figure is 13 MB) and is checked before any decoding, inside a worker whose
// commit is capped at 1 GiB (sandbox_win::WORKER_MEMORY_CAP).
const MAX_IMAGES: usize = 32 * 1024 * 1024;
// Character-positioned exports use two operators per glyph. Keep a finite
// work bound without rejecting ordinary dense pages; decoded bytes stay at 1 MiB.
const MAX_OPERATIONS: usize = 16_384;
pub(crate) const MAX_TEXT: usize = 4096;
pub(crate) const MAX_CHANGES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub operator: u32,
    pub text: String,
    pub font: String,
    pub size: f64,
    /// Text matrix in the page's original user space, before crop and rotation.
    pub matrix: [f64; 6],
    pub advance: f64,
    /// Hit rectangle in the original displayed page, before journal crop and turns.
    pub display_rect: [f32; 4],
    /// Required single-line box height in page points when deeper than 1.25 em.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_height: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PageRuns {
    pub page: u32,
    /// Which document `page` is a page of: the file an inserted page came
    /// from, by its `crate::docmodel::SourceId`, and absent for the document
    /// the reader opened.
    ///
    /// **Filled in by the command rather than by the scan**, because the scan
    /// is the worker's and a worker knows no source id --- it holds one
    /// document and every page number it answers is a page of that one. The
    /// reply carries it so the editor can tell its own pending replacements
    /// from those on the opened file's page of the same number, which is a
    /// pair that collides exactly when a reader inserts pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
    /// Binds operator addresses to the exact decoded content that was inspected.
    pub revision: Vec<u8>,
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
}

/// A bounded crop rendered by the same worker/writer used for the saved PDF.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub png: Vec<u8>,
    pub font: String,
    /// The dashed outline the reader sees: the box their text needed.
    pub rect: [f32; 4],
    /// What the crop has to cover: that box together with every run this draft
    /// pushes along its line, where the draft puts them. The two differ only
    /// while a draft is moving text, and a crop taken from `rect` alone would
    /// cut the moved text in half.
    #[serde(default)]
    pub extent: [f32; 4],
    pub lines: usize,
}

pub(crate) fn preview_layout(doc: &Document, change: &Change) -> Result<Preview, String> {
    // One change, so no other edit on its line has pushed it and none of the
    // shows it pushes is being replaced beside it. A batch that does both is
    // written by `write`; what the reader sees while typing is this one draft.
    let prepared = layout::prepare(
        doc,
        &inspect(doc, change.page)?,
        change,
        &layout::Placement::default(),
    )?;
    Ok(Preview {
        png: Vec::new(),
        font: prepared.label,
        rect: prepared.rect,
        extent: prepared.extent,
        lines: prepared.lines,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub page: u32,
    pub revision: Vec<u8>,
    pub operator: u32,
    pub original: String,
    pub replacement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
}

/// One replacement, and which document its `page` is a page of.
///
/// **The file is beside the change rather than in it**, which is the same
/// split `crate::edits::Plan` makes between `PlannedSource` and a page's
/// `PageSource`: a [`Change`] is a replacement addressed in *one* document ---
/// a page number, an operator ordinal and the digest of that page's decoded
/// content --- and every function in this module takes one together with the
/// `Document` it is addressed in. Which document that is, is a second fact,
/// and putting it inside [`Change`] would have made every validator, fixture
/// and probe in the module carry a field none of them can act on.
///
/// `source` is a [`crate::docmodel::SourceId`] as its bare number, absent for
/// a page of the document the reader opened. It is the same spelling
/// [`crate::docmodel::PageSource::Imported`] and `PlannedSource::id` use, and
/// for the same reason: both sides only ever compare it.
///
/// **`#[serde(flatten)]` on the change**, so the wire shape is what it always
/// was with one optional key more. A journal, a plan or a reply written before
/// this existed therefore reads back as the replacement on the opened
/// document it meant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
    #[serde(flatten)]
    pub change: Change,
}

impl Edit {
    /// A replacement on a page of the document the reader opened.
    pub fn opened(change: Change) -> Edit {
        Edit {
            source: None,
            change,
        }
    }

    /// A replacement on a page inserted from the file `source` names.
    pub fn imported(source: u32, change: Change) -> Edit {
        Edit {
            source: Some(source),
            change,
        }
    }
}

/// User-selected editing area and font size, in page points along the text axes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    pub size: f64,
    #[serde(default)]
    pub wrap: bool,
    #[serde(default)]
    pub font: EditFont,
    /// The reader has not sized this box, so it may follow the typed text as
    /// far as the room after the run allows (`layout::free_width`).
    ///
    /// The editor sets it while the width control still holds the box it was
    /// opened with, and clears it the moment a reader types a width of their
    /// own. It defaults to off, so every request that predates it -- a saved
    /// journal, a probe's request file, a test -- keeps the box it names.
    #[serde(default)]
    pub grow: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditFont {
    #[default]
    Auto,
    Original,
    NotoSans,
    NotoSansBold,
    NotoSansItalic,
    NotoSansBoldItalic,
    NotoSansCjkSc,
    NotoSansCjkScBold,
}

fn dictionary<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Dictionary, String> {
    crate::encoding::resolve_dict(doc, value).map_err(|()| "invalid text resources".into())
}

fn resources(doc: &Document, page: ObjectId) -> Result<&Dictionary, String> {
    let mut id = page;
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        if !seen.insert(id) {
            break;
        }
        let node = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = node.get(b"Resources") {
            return dictionary(doc, value);
        }
        id = node
            .get(b"Parent")
            .and_then(Object::as_reference)
            .map_err(|e| e.to_string())?;
    }
    Err("text resource inheritance exceeds its limit".into())
}

// A layer named in the page's Properties resource: an optional content group
// or membership dictionary, as a form's /OC must be.
fn optional_content(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let invalid = || "unsupported optional content".to_string();
    let properties = dictionary(doc, resources.get(b"Properties").map_err(|_| invalid())?)?;
    let group = dictionary(doc, properties.get(name).map_err(|_| invalid())?)?;
    match group.get(b"Type").and_then(Object::as_name).ok() {
        Some(b"OCG" | b"OCMD") => Ok(()),
        _ => Err(invalid()),
    }
}

// The font named `name` measured for read-only text only, if it is a simple
// font with the widths and bounding box that takes, or Symbol or ZapfDingbats.
fn read_only_font(doc: &Document, resources: &Dictionary, name: &[u8]) -> Option<fonts::Metrics> {
    let fonts = dictionary(doc, resources.get(b"Font").ok()?).ok()?;
    let font = dictionary(doc, fonts.get(name).ok()?).ok()?;
    fonts::read_only(doc, font).or_else(|| fonts::symbolic(font))
}

// The FontBBox named by a font's BaseFont, if it is a Latin standard font's.
// It is read only where the metrics carry no vertical bounds, which only the
// standard-font dispatch at the end of `font` produces.
fn standard_box(doc: &Document, resources: &Dictionary, name: &[u8]) -> Option<[f64; 4]> {
    let fonts = dictionary(doc, resources.get(b"Font").ok()?).ok()?;
    let font = dictionary(doc, fonts.get(name).ok()?).ok()?;
    fonts::Metrics::standard_box(font.get(b"BaseFont").and_then(Object::as_name).ok()?)
}

// The codes a run shows: its own show and the shows grouped into it.
fn shown(content: &Content, groups: &BTreeMap<u32, Vec<u32>>, operator: u32) -> Vec<u8> {
    let members = groups.get(&operator).map_or(&[][..], Vec::as_slice);
    let mut bytes = Vec::new();
    for &index in std::iter::once(&operator).chain(members) {
        let Some(show) = content.operations.get(index as usize) else {
            continue;
        };
        for operand in &show.operands {
            match operand {
                Object::String(string, _) => bytes.extend_from_slice(string),
                Object::Array(items) => {
                    for item in items {
                        if let Object::String(string, _) = item {
                            bytes.extend_from_slice(string);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    bytes
}

fn font(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<fonts::Metrics, String> {
    let fonts = dictionary(doc, resources.get(b"Font").map_err(|e| e.to_string())?)?;
    let font = dictionary(doc, fonts.get(name).map_err(|e| e.to_string())?)?;
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type3") {
        return fonts::type3(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0") {
        return fonts::composite(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"TrueType")
        && fonts::is_unembedded(doc, font)
    {
        return fonts::unembedded(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"TrueType") {
        return fonts::embedded(doc, font);
    }
    if font.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type1")
        && font.has(b"FontDescriptor")
    {
        return fonts::type1(doc, font);
    }
    let unsupported = || "text editing requires a standard font or a supported embedded font";
    for (key, expected) in [
        (b"Type".as_slice(), b"Font".as_slice()),
        (b"Subtype", b"Type1"),
    ] {
        if font.get(key).and_then(Object::as_name).ok() != Some(expected) {
            return Err(unsupported().into());
        }
    }
    // Any of the twelve Latin standard fonts: every arXiv paper's side stamp
    // is set in unembedded Times-Roman. Symbol and ZapfDingbats are not Latin.
    let base = font
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .map_err(|_| unsupported())?;
    let metrics = fonts::Metrics::standard(base).ok_or_else(unsupported)?;
    if font.iter().any(|(key, _)| {
        !matches!(
            key.as_slice(),
            b"Type" | b"Subtype" | b"BaseFont" | b"Encoding" | b"Name"
        )
    }) {
        return Err("custom font metrics or character mappings are not editable yet".into());
    }
    match font.get(b"Encoding").ok() {
        None => Ok(metrics.standard_encoding()),
        Some(Object::Name(name)) if name == b"WinAnsiEncoding" => Ok(metrics),
        _ => Err("unsupported standard font encoding".into()),
    }
}

fn number(value: &Object) -> Result<f64, String> {
    let value = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err("invalid text position or size".into()),
    };
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err("text position or size exceeds its limit".into());
    }
    Ok(value)
}

// WinAnsi agrees with Latin-1 in these ranges. Bytes 127..159 have different
// glyph mappings and must not be interpreted as Unicode control characters.
fn text_byte(byte: u8) -> bool {
    (32..=126).contains(&byte) || byte >= 160
}

// ISO 32000-1 Annex D.2: WinAnsi punctuation and letters in 0x82..0x9F. Each
// keeps its WinAnsi code as its metric slot. The euro sign (0x80) is omitted
// because that slot is the internal minus. 0x81, 0x8D, 0x8F, 0x90 and 0x9D
// are undefined in WinAnsi.
const WINANSI_EXTRA: [(u8, char); 26] = [
    (0x82, '\u{201A}'),
    (0x83, '\u{0192}'),
    (0x84, '\u{201E}'),
    (0x85, '\u{2026}'),
    (0x86, '\u{2020}'),
    (0x87, '\u{2021}'),
    (0x88, '\u{02C6}'),
    (0x89, '\u{2030}'),
    (0x8A, '\u{0160}'),
    (0x8B, '\u{2039}'),
    (0x8C, '\u{0152}'),
    (0x8E, '\u{017D}'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201C}'),
    (0x94, '\u{201D}'),
    (0x95, '\u{2022}'),
    (0x96, '\u{2013}'),
    (0x97, '\u{2014}'),
    (0x98, '\u{02DC}'),
    (0x99, '\u{2122}'),
    (0x9A, '\u{0161}'),
    (0x9B, '\u{203A}'),
    (0x9C, '\u{0153}'),
    (0x9E, '\u{017E}'),
    (0x9F, '\u{0178}'),
];

// Metric slots use Latin-1 indices, WinAnsi's extra characters at their own
// codes, and one minus slot. A slot is not a PDF code in a custom font; its
// ToUnicode map supplies that. U+0096 is a control character, never an alias.
// Each font path still decides which of these characters it can prove.
fn character_slot(ch: char) -> Option<u8> {
    if ch == '\u{2212}' {
        Some(0x80) // Internal metric slot only; never a literal WinAnsi byte.
    } else if let Some(&(slot, _)) = WINANSI_EXTRA.iter().find(|(_, extra)| *extra == ch) {
        Some(slot)
    } else {
        u8::try_from(ch as u32).ok().filter(|&byte| text_byte(byte))
    }
}

fn slot_character(slot: u8) -> char {
    if slot == 0x80 {
        '\u{2212}'
    } else if let Some(&(_, ch)) = WINANSI_EXTRA.iter().find(|(extra, _)| *extra == slot) {
        ch
    } else {
        char::from(slot)
    }
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_TEXT || !bytes.iter().all(|&byte| text_byte(byte)) {
        return Err("text editing currently supports printable Latin-1 only".into());
    }
    Ok(bytes.iter().map(|&byte| char::from(byte)).collect())
}

fn encode_text(text: &str) -> Result<Vec<u8>, String> {
    // The en dash uses three UTF-8 bytes. This returns metric slots; a custom
    // font maps them back to its own one- or two-byte PDF codes when writing.
    if text.len() > MAX_TEXT * 3 {
        return Err("text replacement exceeds its limit".into());
    }
    let bytes = text
        .chars()
        .map(|ch| {
            character_slot(ch)
                .ok_or("text editing currently supports printable Latin-1, WinAnsi punctuation and minus only")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() > MAX_TEXT {
        return Err("text replacement exceeds its limit".into());
    }
    Ok(bytes)
}

fn page_content(doc: &Document, id: ObjectId) -> Result<Vec<u8>, String> {
    let page = doc.get_dictionary(id).map_err(|e| e.to_string())?;
    let contents = page.get(b"Contents").map_err(|e| e.to_string())?;
    let contents = crate::encoding::resolve(doc, contents);
    let streams = match contents {
        Object::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    if streams.len() > 128 {
        return Err("too many page content streams".into());
    }
    let mut bytes = Vec::new();
    for value in streams {
        let stream = crate::encoding::resolve(doc, value)
            .as_stream()
            .map_err(|e| e.to_string())?;
        let remaining = MAX_CONTENT.saturating_sub(bytes.len() + 1);
        let decoded = filters::decode(stream, remaining)?;
        bytes.extend(decoded);
        bytes.push(b'\n');
        if bytes.len() > MAX_CONTENT {
            return Err("page content exceeds its limit".into());
        }
    }
    Ok(bytes)
}

// Td/T* translate the line matrix, not the text matrix advanced by Tj.
// Orthogonal text axes reach here; reject accumulated positions outside the
// same bound used for authored coordinates.
fn move_line(matrix: &mut [f64; 6], x: f64, y: f64) -> Result<(), String> {
    shift_position(
        matrix,
        x * matrix[0] + y * matrix[2],
        x * matrix[1] + y * matrix[3],
    )
}

// The same position bound applies to authored and accumulated line positions.
fn shift_position(matrix: &mut [f64; 6], x: f64, y: f64) -> Result<(), String> {
    matrix[4] += x;
    matrix[5] += y;
    if matrix[4..]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err("text position exceeds its limit".into());
    }
    Ok(())
}

// Exactly axis-aligned text axes, including quarter turns. No epsilon admits
// near-orthogonal skew: bounding and replacement containment rely on this shape.
fn orthogonal(matrix: [f64; 6]) -> bool {
    (matrix[1] == 0. && matrix[2] == 0. && matrix[0] != 0. && matrix[3] != 0.)
        || (matrix[0] == 0. && matrix[3] == 0. && matrix[1] != 0. && matrix[2] != 0.)
}

// ISO 32000-1, 8.3.4: a new matrix acts before the existing CTM. The
// page CTM stays diagonal; only text matrices may exchange the two axes.
// Reflections may cancel; orientation is checked after composing at each show.
fn compose_orthogonal(outer: [f64; 6], inner: [f64; 6]) -> Result<[f64; 6], String> {
    let result = compose_affine(outer, inner)?;
    if !orthogonal(result) {
        return Err("composed text transform exceeds its limit".into());
    }
    Ok(result)
}

fn diagonal(matrix: [f64; 6]) -> bool {
    matrix[1] == 0. && matrix[2] == 0. && matrix[0] != 0. && matrix[3] != 0.
}

fn compose_affine(outer: [f64; 6], inner: [f64; 6]) -> Result<[f64; 6], String> {
    let result = [
        inner[0] * outer[0] + inner[1] * outer[2],
        inner[0] * outer[1] + inner[1] * outer[3],
        inner[2] * outer[0] + inner[3] * outer[2],
        inner[2] * outer[1] + inner[3] * outer[3],
        inner[4] * outer[0] + inner[5] * outer[2] + outer[4],
        inner[4] * outer[1] + inner[5] * outer[3] + outer[5],
    ];
    if result
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        || result[0] * result[3] - result[1] * result[2] == 0.
    {
        return Err("composed text transform exceeds its limit".into());
    }
    Ok(result)
}

// Transform the entire text-space envelope, not just its baseline/advance.
// Corner extrema also cover the negative axes of 90/180/270-degree text.
fn text_bounds(matrix: [f64; 6], rect: [f64; 4]) -> [f64; 4] {
    let [a, b, c, d, e, f] = matrix;
    let [left, bottom, right, top] = rect;
    let xs = [left * a, right * a];
    let ys = [bottom * c, top * c];
    let xb = [left * b, right * b];
    let yd = [bottom * d, top * d];
    [
        e + xs[0].min(xs[1]) + ys[0].min(ys[1]),
        f + xb[0].min(xb[1]) + yd[0].min(yd[1]),
        e + xs[0].max(xs[1]) + ys[0].max(ys[1]),
        f + xb[0].max(xb[1]) + yd[0].max(yd[1]),
    ]
}

struct Inspection {
    id: ObjectId,
    content: Content,
    bytes: Vec<u8>,
    patched: BTreeSet<usize>,
    runs: PageRuns,
    // Validated non-diagonal text is preserved byte-for-byte. It supplies
    // collision bounds but never an editable operator address.
    preserved: Vec<Run>,
    form_text_bounds: Vec<[f32; 4]>,
    /// Everything the page paints that is not text: a painted rectangle, a
    /// painted path, an image, and a preserved form's whole BBox, in the
    /// original displayed page.
    ///
    /// Nothing reads this when the editing box grows, and that asymmetry is
    /// deliberate: growing a box puts the *reader's own* text where they are
    /// watching it, while pushing the line along puts somebody else's text
    /// somewhere they never asked for it to go, so the push is held to what the
    /// editor can see and the box is not. The list is not complete -- an
    /// annotation's rectangle is not in it, because the editor never reads the
    /// page's /Annots -- which is why it can only ever refuse a push.
    graphics: Vec<[f32; 4]>,
    /// Which of `graphics` are painted paths the writer can move whole, by
    /// index: the operators from the first construction operator to the
    /// painting one, and the transform they are drawn under. A wrap moves an
    /// underline with its line (`layout::wrap_room`).
    paths: BTreeMap<usize, DrawnPath>,
    actual_text: BTreeMap<u32, usize>,
    // The active Tf can precede a restored state, not just the last Tf in the
    // stream. Keep its address privately; display font names can be lossy UTF-8.
    font_operators: BTreeMap<u32, usize>,
    // Unrounded horizontal bounds relative to the authored text origin.
    // A replacement must stay within these as well as the original advance.
    horizontal_bounds: BTreeMap<u32, [f64; 2]>,
    text_spacing: BTreeMap<u32, (f64, f64)>,
    // A TJ's leading adjustment, kept verbatim in front of whatever replaces
    // the show so the run and everything after it keep their positions.
    leads: BTreeMap<u32, Object>,
    // The mean word-gap displacement of a run whose font cannot write a space,
    // in thousandths of an em; a replacement's spaces reuse it.
    gaps: BTreeMap<u32, f64>,
    continued: BTreeSet<u32>,
    groups: BTreeMap<u32, Vec<u32>>,
    compound_run_clips: BTreeMap<u32, (Vec<clipping::Region>, [f64; 4])>,
    contexts: BTreeMap<u32, layout::Context>,
    expanded: BTreeMap<usize, layout::Prepared>,
    /// The block element owning every show on a tagged page -- editable,
    /// read-only and spacer shows alike -- so that a wrap can find all of its
    /// paragraph's text, including what it may not move (`layout::wrap`).
    blocks: BTreeMap<u32, ObjectId>,
    /// The shows a wrap moves down its paragraph, each with the operations
    /// that draw it there (`layout::Prepared::lowered`), gathered across the
    /// batch so that `write` can refuse two edits that move one show.
    lowered: BTreeMap<usize, Vec<lopdf::content::Operation>>,
    /// Every annotation's rectangle except a popup's, in the original
    /// displayed page. Only a wrap reads it: moving a paragraph's lines down
    /// would leave a highlight or a link over the text that used to be there.
    /// `None` when the page's list could not be read, which refuses a wrap and
    /// nothing else: no other edit ever read annotations.
    annotations: Option<Vec<Annotation>>,
    /// The links the batch's wraps move with their text, each with its new
    /// `/Rect` in default user space (`layout::Prepared::links`).
    links: BTreeMap<ObjectId, [f64; 4]>,
}

/// One annotation as a wrap sees it: where it is on the displayed page, and,
/// for the one kind a wrap may move with the text under it, which object it is
/// and its rectangle in default user space.
///
/// That kind is a `/Link` held by reference, with no appearance stream and no
/// `/QuadPoints`: a rectangle and nothing drawn, so moving the rectangle moves
/// all of it. Every link on the pages that measured this has that shape
/// (`BUILD.md`, *Links over lines a wrap moves*). Anything else stays where it
/// is and refuses a wrap that moves text from under it.
pub(super) struct Annotation {
    pub rect: [f32; 4],
    pub link: Option<(ObjectId, [f64; 4])>,
}

// TJ offsets are subtracted in thousandths of text space, before the text/page
// matrices. Keep a single left-to-right envelope: no negative cursor or
// retreating fragment ends. Trailing forward padding is allowed.
// A replacement drops kerning within this run and must fit the resulting
// original advance.
//
// One leading adjustment is not kerning but where the run starts: pdfTeX opens
// indented and justified lines with one. It is returned separately, moves the
// run's origin, and is written back unchanged in front of any replacement.
//
// In a font that cannot write a space, a displacement of at least GAP_EM
// between two strings is the space itself (pdfTeX sets every interword space
// this way). It becomes ' ' in the text; the total of those displacements is
// returned so a replacement can reuse their mean width.
fn array_text<'a>(
    values: &'a [Object],
    metrics: &fonts::Metrics,
    size: f64,
    spacing: f64,
    word_spacing: f64,
) -> Result<ArrayText<'a>, String> {
    let (lead, values) = match values {
        [lead @ (Object::Integer(_) | Object::Real(_)), rest @ ..] => (Some(lead), rest),
        values => (None, values),
    };
    if values.is_empty()
        || values.len() > MAX_TEXT
        || !matches!(values.first(), Some(Object::String(..)))
    {
        return Err("unsupported kerning array shape or size".into());
    }
    let mut text = String::new();
    let mut characters = 0;
    let mut advance = 0.0;
    let mut furthest = 0.0_f64;
    let mut backtracks = false;
    let mut bounds = [0_f64; 2];
    let gap_spaces = !metrics.writes_space();
    let mut gaps = Gaps::default();
    // The displacement since the last string, in thousandths of an em.
    let mut pending = 0.0;
    for value in values {
        if let Object::String(bytes, _) = value {
            let (fragment, width, [left, right]) =
                metrics.source_layout(bytes, size, spacing, word_spacing)?;
            // The run's leading adjustment is split off above, so a gap here
            // always follows a string.
            if gap_spaces && pending >= GAP_EM {
                text.push(' ');
                characters += 1;
                gaps.total += pending;
                gaps.count += 1;
            }
            pending = 0.0;
            characters += fragment.chars().count();
            if characters > MAX_TEXT {
                return Err("kerning array text exceeds its limit".into());
            }
            bounds[0] = bounds[0].min(advance + left);
            bounds[1] = bounds[1].max(advance + right);
            advance += width;
            // A string that ends before an earlier one did draws back over it
            // (ConTeXt's footers put the title left of the page number). The
            // run's text is then not in reading order, so it is kept
            // read-only; the bounds above already cover every string.
            backtracks |= advance < furthest;
            furthest = furthest.max(advance);
            text.push_str(&fragment);
        } else {
            let value = number(value)?;
            pending -= value;
            advance -= value * size / 1000.0;
        }
        if !advance.is_finite() || advance.abs() > 1_000_000.0 {
            return Err("kerning position exceeds its limit".into());
        }
    }
    // A trailing number that pulls the cursor back behind the last string's
    // end moves whatever follows over it, so that run stays read-only too.
    backtracks |= advance < furthest;
    Ok((text, advance, bounds, lead, gaps, backtracks))
}

// The smallest TJ displacement read as a word space, in thousandths of an em:
// the same 0.18 em grouping.rs uses between separately positioned fragments.
// Kerns stay well below it; TeX's shrunk interword spaces stay above it.
const GAP_EM: f64 = 180.0;
// The space a replacement writes where its run had none to measure.
const DEFAULT_GAP: f64 = 250.0;

// A TJ array's text, advance, ink, leading adjustment, word gaps and whether
// any string starts before the end of an earlier one.
type ArrayText<'a> = (String, f64, [f64; 2], Option<&'a Object>, Gaps, bool);

#[derive(Default)]
struct Gaps {
    total: f64,
    count: u32,
}

fn inspect(doc: &Document, page: u32) -> Result<Inspection, String> {
    let pages = crate::pagetree::ordered_pages(doc);
    let id = *pages
        .get(page as usize)
        .ok_or("text page is not in this document")?;
    if pages.iter().filter(|&&other| other == id).count() != 1 {
        return Err("a repeated page object is not editable".into());
    }
    let mut tags = tagging::Tags::read(doc, id, &pages)?;
    let bytes = page_content(doc, id)?;
    let content = Content::decode_strict(&bytes).map_err(|e| e.to_string())?;
    if content.operations.len() > MAX_OPERATIONS {
        return Err("text operator count exceeds its limit".into());
    }
    // Discovery promises that deletion can use the byte-preserving writer too.
    streams::rewrite(&bytes, &content, &BTreeSet::new())?;
    let resources = resources(doc, id)?;
    let mut fill_components =
        patterns::Colour::Solid(colors::named(doc, resources, b"DeviceGray")?);
    let mut colour_spaces = BTreeMap::new();
    let mut shading_patterns = BTreeSet::new();
    // Each validated state and the line width it sets, if any.
    let mut graphics_states = BTreeMap::new();
    let mut image_names = BTreeSet::new();
    let mut stencils: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut form_bounds = BTreeMap::new();
    let mut image_bytes = 0;
    let mut result = PageRuns {
        page,
        // The scan holds one document and cannot name it; the command that
        // chose which document to ask fills this in. See the field.
        source: None,
        revision: Sha256::digest(&bytes).to_vec(),
        runs: Vec::new(),
        preview: None,
    };
    // Never skip unknown operators: graphics and text state can change a Tj's
    // meaning without changing its string. Tf and TL persist across BT/ET;
    // the text/line matrices reset at BT. Keep the line matrix separate from
    // the cursor advanced by each show. A shorter edit must compensate that
    // advance whenever another show depends on it.
    let mut inside = false;
    let mut positioned = false;
    let mut cursor = 0.0;
    let mut previous_show = None;
    // Where the current text line matrix was last set (the BT or Tm), and the
    // leading then in effect: a layout restores the line by replaying from here.
    let mut line_origin = (0_usize, 0.0_f64);
    let mut spacer: Option<spacers::Spacer> = None;
    // An empty spacer outside a text object: open until its EMC.
    let mut empty_spacer = false;
    let mut actual: Option<actual::Span> = None;
    // Inside an optional-content (layer) sequence: its text may be hidden.
    let mut layer = false;
    let mut actual_spans = Vec::new();
    let mut continued = BTreeSet::new();
    let mut selected_font = None;
    let mut font_metrics = BTreeMap::new();
    let mut font_boxes = BTreeMap::new();
    // Why the first font the editor cannot write with was kept read-only: the
    // refusal a page gets when that leaves it nothing to edit.
    let mut unusable_font: Option<String> = None;
    let mut leading = 0.0;
    let mut spacing = 0.0;
    let mut word_spacing = 0.0;
    // ISO 32000-1 9.3.6 and 8.4.3.2: the text render mode and the line width
    // are graphics state, saved and restored with it. Modes 1 and 2 stroke the
    // glyphs, so their ink reaches half the line width beyond the outlines.
    let mut render = 0_i64;
    let mut line_width = 1.0_f64;
    let mut stroke_components = patterns::Colour::Solid(1);
    let mut states = Vec::new();
    let mut clip = None;
    let mut compound_clips: Vec<clipping::Region> = Vec::new();
    let mut path_until = 0;
    let mut page_transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut font_operators = BTreeMap::new();
    let mut horizontal_bounds = BTreeMap::new();
    let mut blocks = BTreeMap::new();
    let mut text_spacing = BTreeMap::new();
    let mut leads = BTreeMap::new();
    let mut gaps = BTreeMap::new();
    let mut compound_run_clips = BTreeMap::new();
    let mut contexts = BTreeMap::new();
    let mut preserved = Vec::new();
    let mut form_text_bounds = Vec::new();
    let mut graphics: Vec<[f32; 4]> = Vec::new();
    let mut paths = BTreeMap::new();
    // What each named XObject paints, in its own space: the unit square for an
    // image (ISO 32000-1 8.9.5.2 maps every image onto it) and the BBox for a
    // preserved form. A name is checked once and drawn many times, each time
    // under its own CTM, so the rectangle is kept and transformed per use.
    let mut drawn_bounds: BTreeMap<Vec<u8>, [f64; 4]> = BTreeMap::new();
    let sheet = crate::pagetree::displayed_page(doc, id);
    let (sox, soy) = (f64::from(sheet.origin.0), f64::from(sheet.origin.1));
    let to_display = |bounds: [f64; 4]| {
        crate::text::to_device(
            sheet.turns,
            sheet.width,
            sheet.height,
            [
                bounds[0] - sox,
                bounds[1] - soy,
                bounds[2] - sox,
                bounds[3] - soy,
            ],
        )
    };
    let mut matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    // Whether the marked-content sequence opened at `index` closes at once,
    // with no operator inside it.
    let closes = |index: usize| {
        content
            .operations
            .get(index + 1)
            .is_some_and(|next| next.operator == "EMC" && next.operands.is_empty())
    };
    for (index, op) in content.operations.iter().enumerate() {
        if index < path_until {
            continue;
        }
        if let Some(spacer) = &mut spacer {
            spacer.step(&op.operator)?;
        }
        if actual.is_some() && matches!(op.operator.as_str(), "BDC" | "BMC") {
            return Err("nested ActualText marked content is not editable yet".into());
        }
        if layer && matches!(op.operator.as_str(), "BDC" | "BMC") {
            return Err("marked content inside optional content is not editable yet".into());
        }
        match (op.operator.as_str(), op.operands.as_slice()) {
            // Marked content and text objects are independently balanced (ISO
            // 32000-1, 14.6.1). MCIDs use the same ownership checks inside BT;
            // only the narrow ActualText spacer grammar has a separate path.
            // Artifacts balance independently of BT/ET too; PDFMaker opens
            // running headers inside the text object.
            // ISO 32000-1 8.11.3.2: content that belongs to a layer. PowerPoint
            // puts each slide's background in one. The editor never resolves
            // the layer state, so text inside is kept read-only, and nothing
            // may open inside it, which lets the next EMC close it.
            ("BDC", [tag, Object::Name(resource)]) if tag.as_name().ok() == Some(b"OC") => {
                optional_content(doc, resources, resource)?;
                layer = true;
            }
            ("EMC", []) if layer => layer = false,
            ("BDC", [tag, properties])
                if tag.as_name().ok() == Some(b"Artifact")
                    && !properties.as_dict().is_ok_and(|dict| dict.has(b"MCID")) =>
            {
                tags.artifact(properties)?
            }
            ("BDC", [tag, properties])
                if !properties.as_dict().is_ok_and(|dict| dict.has(b"MCID")) =>
            {
                let separator = spacers::Spacer::new(tag, properties).ok();
                // InDesign writes a tab stop as an empty span of this kind
                // between text objects, where nothing can be shown: it holds
                // only its ActualText, and there is nothing in it to edit or
                // move.
                let empty = !inside && closes(index);
                if let Some(value) = separator.filter(|_| inside || empty) {
                    if inside {
                        spacer = Some(value);
                    } else {
                        empty_spacer = true;
                    }
                } else {
                    actual = Some(actual::Span::new(tag, properties, index)?);
                }
            }
            ("EMC", []) if actual.is_some() => {
                if actual_spans.len() >= 128 {
                    return Err("too many ActualText spans".into());
                }
                actual_spans.push(actual.take().unwrap());
            }
            ("EMC", []) if inside && spacer.is_some() => {
                spacer = None;
            }
            ("EMC", []) if empty_spacer => empty_spacer = false,
            // An artifact needs no properties, inside a text object or out:
            // LibreOffice marks table-of-contents dot leaders `/Artifact BMC`.
            ("BMC", [tag]) if !inside || tag.as_name().ok() == Some(b"Artifact") => {
                tags.begin(tag, None)?
            }
            ("BDC", [tag, properties]) => {
                tags.begin(tag, Some(properties))?;
                if closes(index) {
                    tags.empty();
                }
            }
            ("EMC", []) => tags.end()?,
            // ISO 32000-1, 8.4.2: font, size and leading are graphics state.
            // Only accept saves outside BT/ET. Preserve every accepted state
            // component; the next BT resets both text matrices.
            ("q", []) if !inside => {
                if states.len() >= 64 {
                    return Err("text graphics-state stack exceeds its limit".into());
                }
                states.push((
                    selected_font,
                    leading,
                    page_transform,
                    fill_components,
                    clip,
                    spacing,
                    word_spacing,
                    stroke_components,
                    compound_clips.clone(),
                    (render, line_width),
                ));
            }
            ("Q", []) if !inside => {
                (
                    selected_font,
                    leading,
                    page_transform,
                    fill_components,
                    clip,
                    spacing,
                    word_spacing,
                    stroke_components,
                    compound_clips,
                    (render, line_width),
                ) = states.pop().ok_or("unmatched graphics-state restore")?;
            }
            ("re", _) if !inside => {
                if let Some(rectangles_consumed) =
                    clipping::painted(&content.operations[index..], page_transform)?
                {
                    patterns::paint(
                        &content.operations[index + rectangles_consumed - 1].operator,
                        fill_components,
                        stroke_components,
                    )?;
                    if content.operations[index + rectangles_consumed - 1].operator != "n" {
                        tags.paint();
                    }
                    if let Some(bounds) = clipping::drawn(
                        &content.operations[index..],
                        rectangles_consumed,
                        page_transform,
                    )? {
                        paths.insert(
                            graphics.len(),
                            DrawnPath {
                                operations: (index, index + rectangles_consumed - 1),
                                transform: page_transform,
                            },
                        );
                        graphics.push(to_display(bounds));
                    }
                    path_until = index + rectangles_consumed;
                } else if !diagonal(page_transform) {
                    return Err("non-diagonal clips are not editable yet".into());
                } else {
                    clip = Some(clipping::apply(
                        clip,
                        &content.operations[index..],
                        page_transform,
                    )?);
                    path_until = index + 3;
                }
            }
            ("m", _) if !inside => {
                // A rotated or skewed path may be painted; clipping with one
                // stays refused, because the clip model is axis-aligned.
                if let Some((consumed, region)) = diagonal(page_transform)
                    .then(|| clipping::compound(&content.operations[index..], page_transform))
                    .transpose()?
                    .flatten()
                {
                    if compound_clips.len() >= 32 {
                        return Err("too many compound clipping intersections".into());
                    }
                    compound_clips.push(region);
                    path_until = index + consumed;
                    continue;
                }
                let consumed = clipping::path(&content.operations[index..], page_transform)?;
                patterns::paint(
                    &content.operations[index + consumed - 1].operator,
                    fill_components,
                    stroke_components,
                )?;
                if content.operations[index + consumed - 1].operator != "n" {
                    tags.paint();
                }
                if let Some(bounds) =
                    clipping::drawn(&content.operations[index..], consumed, page_transform)?
                {
                    paths.insert(
                        graphics.len(),
                        DrawnPath {
                            operations: (index, index + consumed - 1),
                            transform: page_transform,
                        },
                    );
                    graphics.push(to_display(bounds));
                }
                path_until = index + consumed;
            }
            // Every accepted path is consumed as a complete sequence, so an
            // isolated n outside BT has no pending path or clip to apply.
            ("n", []) if !inside => {}
            ("Do", [Object::Name(name)]) if !inside => {
                if !image_names.contains(name) {
                    if image_names.len() >= 32 {
                        return Err("too many images on an editable page".into());
                    }
                    if let Some(form) =
                        forms::check(doc, resources, name, MAX_IMAGES - image_bytes)?
                    {
                        image_bytes += form.bytes;
                        form_bounds.insert(name.clone(), form.text_bounds);
                        drawn_bounds.insert(name.clone(), form.bounds);
                    } else {
                        let image = images::check(doc, resources, name, MAX_IMAGES - image_bytes)?;
                        image_bytes += image.bytes;
                        if image.stencil {
                            stencils.insert(name.clone());
                        }
                        drawn_bounds.insert(name.clone(), [0., 0., 1., 1.]);
                    }
                    image_names.insert(name.clone());
                }
                // Where this use of it lands, clipped as its text bounds are.
                if let Some(&painted) = drawn_bounds.get(name) {
                    let mut bounds = text_bounds(page_transform, painted);
                    if let Some(clip) = clip {
                        bounds = [
                            bounds[0].max(clip[0]),
                            bounds[1].max(clip[1]),
                            bounds[2].min(clip[2]),
                            bounds[3].min(clip[3]),
                        ];
                    }
                    if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                        graphics.push(to_display(bounds));
                    }
                }
                // A stencil mask paints the fill colour current at each use.
                if stencils.contains(name) {
                    patterns::paint("f", fill_components, stroke_components)?;
                }
                if let Some(Some(bounds)) = form_bounds.get(name) {
                    let mut bounds = text_bounds(page_transform, *bounds);
                    if let Some(clip) = clip {
                        bounds = [
                            bounds[0].max(clip[0]),
                            bounds[1].max(clip[1]),
                            bounds[2].min(clip[2]),
                            bounds[3].min(clip[3]),
                        ];
                    }
                    if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                        let geometry = crate::pagetree::displayed_page(doc, id);
                        let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
                        form_text_bounds.push(crate::text::to_device(
                            geometry.turns,
                            geometry.width,
                            geometry.height,
                            [
                                bounds[0] - ox,
                                bounds[1] - oy,
                                bounds[2] - ox,
                                bounds[3] - oy,
                            ],
                        ));
                    }
                }
                tags.paint();
            }
            ("w", [value]) => {
                clipping::line_width(value)?;
                line_width = number(value)?;
            }
            ("J" | "j" | "M" | "d", values) => graphics::stroke(&op.operator, values)?,
            ("i", [value]) => graphics::tolerance(b"FL", value)?,
            // ISO 32000-1 Figure 9 does not list cm inside a text object, but
            // arXiv's stamp (`BT 0 1 -1 0 0 0 cm ... Tm ... TJ ET`) puts it
            // first in the block and every reader applies it. Geometry is
            // taken from the CTM at each show, so a cm before the block's first
            // show changes nothing already measured; one between shows would.
            ("cm", values) if (!inside || previous_show.is_none()) && values.len() == 6 => {
                let mut next = [0.0; 6];
                for (dest, value) in next.iter_mut().zip(values) {
                    *dest = number(value)?;
                }
                page_transform = compose_affine(page_transform, next)?;
            }
            ("BT", []) if !inside => {
                inside = true;
                positioned = false;
                cursor = 0.0;
                previous_show = None;
                matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                line_origin = (index, leading);
            }
            ("ET", []) if inside => inside = false,
            // Explicit defaults have the same semantics as an omitted setting.
            // Text state can be set outside BT/ET and persists across blocks.
            // Text spacing is saved by q/Q and checked against the active
            // font at each show. Other nondefault text state remains refused.
            ("Tc", [value]) => spacing = number(value)?,
            ("Tw", [value]) => word_spacing = number(value)?,
            ("Ts", [value]) if number(value)? == 0.0 => {}
            ("Tz", [value]) if number(value)? == 100.0 => {}
            // Fill, stroke, both, or neither. Modes 4 to 7 add the glyphs to
            // the clipping path and change what later content shows.
            ("Tr", [Object::Integer(mode @ 0..=3)]) => render = *mode,
            ("ri", [Object::Name(name)]) => colors::intent(name)?,
            ("gs", [Object::Name(name)]) => {
                if !graphics_states.contains_key(name) {
                    if graphics_states.len() >= 32 {
                        return Err("too many external text graphics states".into());
                    }
                    let width = graphics::normal(doc, resources, name)?;
                    graphics_states.insert(name.clone(), width);
                }
                if let Some(width) = graphics_states[name] {
                    line_width = width;
                }
            }
            ("cs" | "CS", [Object::Name(name)]) => {
                if !colour_spaces.contains_key(name) {
                    if colour_spaces.len() >= 32 {
                        return Err("too many text colour spaces".into());
                    }
                    colour_spaces.insert(
                        name.clone(),
                        if name == b"Pattern" {
                            patterns::Colour::Pattern { selected: false }
                        } else {
                            patterns::Colour::Solid(colors::named(doc, resources, name)?)
                        },
                    );
                }
                if op.operator == "cs" {
                    fill_components = colour_spaces[name];
                } else {
                    stroke_components = colour_spaces[name];
                }
            }
            ("sc" | "scn", values) => {
                fill_components.set(doc, resources, &op.operator, values, &mut shading_patterns)?
            }
            ("SC" | "SCN", values) => stroke_components.set(
                doc,
                resources,
                &op.operator,
                values,
                &mut shading_patterns,
            )?,
            ("g" | "rg" | "k", values) => {
                let components = match op.operator.as_str() {
                    "g" => 1,
                    "rg" => 3,
                    _ => 4,
                };
                fill_components = patterns::Colour::Solid(components);
                colors::values(values, components)?;
            }
            ("G" | "RG" | "K", values) => {
                // Filled text cannot use stroke colour; preserve the validated
                // setter without changing the independently tracked fill space.
                let components = match op.operator.as_str() {
                    "G" => 1,
                    "RG" => 3,
                    _ => 4,
                };
                stroke_components = patterns::Colour::Solid(components);
                colors::values(values, components)?;
            }
            // ISO 32000-1 Table 51: font and leading are text state, which the
            // page may set before BT (Typst does); q/Q save both.
            ("Tf", [name, size]) => {
                let name = name.as_name().map_err(|e| e.to_string())?;
                if !font_metrics.contains_key(name) {
                    if font_metrics.len() >= 32 {
                        return Err("too many fonts on an editable page".into());
                    }
                    // A simple font the editor cannot write with keeps its
                    // text read-only (`fonts::read_only`) rather than refusing
                    // the page; when even that cannot measure it, the page is
                    // refused with the font's own reason.
                    let metrics = match font(doc, resources, name) {
                        Ok(metrics) => metrics,
                        Err(error) => {
                            let metrics = read_only_font(doc, resources, name).ok_or(&error)?;
                            unusable_font.get_or_insert(error);
                            metrics
                        }
                    };
                    font_metrics.insert(name.to_vec(), metrics);
                    if let Some(bounds) = standard_box(doc, resources, name) {
                        font_boxes.insert(name.to_vec(), bounds);
                    }
                }
                let size = number(size)?;
                if !(0.0..=1000.0).contains(&size) || size == 0.0 {
                    return Err("unsupported text size".into());
                }
                selected_font = Some((name, size, index));
            }
            ("TL", [value]) => leading = number(value)?,
            ("Tm", values) if inside && values.len() == 6 => {
                for (dest, value) in matrix.iter_mut().zip(values) {
                    *dest = number(value)?;
                }
                compose_affine([1., 0., 0., 1., 0., 0.], matrix)?;
                positioned = true;
                cursor = 0.0;
                line_origin = (index, leading);
            }
            ("Td" | "TD", [x, y]) if inside => {
                let (x, y) = (number(x)?, number(y)?);
                // TD is exactly -ty TL followed by tx ty Td. Both move the
                // line matrix, independently of the preceding show's advance.
                if op.operator == "TD" {
                    leading = -y;
                }
                move_line(&mut matrix, x, y)?;
                positioned = true;
                cursor = 0.0;
            }
            ("T*", []) if inside => {
                move_line(&mut matrix, 0.0, -leading)?;
                positioned = true;
                cursor = 0.0;
            }
            ("Tj", [_]) | ("TJ", [Object::Array(_)])
                if inside && (positioned || previous_show.is_some()) =>
            {
                tags.text()?;
                if let Some(block) = tags.block() {
                    blocks.insert(index as u32, block);
                }
            }
            _ => {
                return Err(refusal::operation(
                    &op.operator,
                    &op.operands,
                    inside,
                    positioned || previous_show.is_some(),
                ))
            }
        }
        if !matches!(op.operator.as_str(), "Tj" | "TJ") {
            continue;
        }
        let (name, size, font_operator) = selected_font.ok_or("text has no explicit font")?;
        if !positioned {
            continued.insert(previous_show.ok_or("text has no preceding position")?);
        }
        positioned = false;
        previous_show = Some(index as u32);
        let geometry = crate::pagetree::displayed_page(doc, id);
        let metrics = font_metrics.get(name).ok_or("missing text font")?;
        let mut backtracks = false;
        let (text, advance, horizontal) = if op.operator == "TJ" {
            let (text, advance, horizontal, lead, found, back) = array_text(
                op.operands[0].as_array().map_err(|e| e.to_string())?,
                metrics,
                size,
                spacing,
                word_spacing,
            )?;
            backtracks = back;
            if found.count > 0 {
                gaps.insert(index as u32, found.total / f64::from(found.count));
            }
            if let Some(lead) = lead {
                cursor -= number(lead)? * size / 1000.0;
                if !cursor.is_finite() || cursor.abs() > 1_000_000.0 {
                    return Err("kerning position exceeds its limit".into());
                }
                leads.insert(index as u32, lead.clone());
            }
            (text, advance, horizontal)
        } else {
            metrics.source_layout(
                op.operands[0].as_str().map_err(|e| e.to_string())?,
                size,
                spacing,
                word_spacing,
            )?
        };
        let mut shown_matrix = matrix;
        shift_position(&mut shown_matrix, cursor * matrix[0], cursor * matrix[1])?;
        let page_matrix = if diagonal(page_transform) && orthogonal(shown_matrix) {
            compose_orthogonal(page_transform, shown_matrix)?
        } else {
            compose_affine(page_transform, shown_matrix)?
        };
        // A glyph whose text the editor cannot write keeps its whole run.
        let read_only = tags.read_only()
            || layer
            || backtracks
            || text.contains(fonts::OPAQUE)
            || !diagonal(page_transform)
            || !orthogonal(page_matrix)
            || page_matrix[0] * page_matrix[3] - page_matrix[1] * page_matrix[2] <= 0.0
            || !matches!(fill_components, patterns::Colour::Solid(_))
            || (matches!(render, 1 | 2)
                && !matches!(stroke_components, patterns::Colour::Solid(_)));
        // The colours the mode paints with; an invisible run paints nothing.
        if let Some(paint) = [Some("f"), Some("S"), Some("B"), None][render as usize] {
            patterns::paint(paint, fill_components, stroke_components)?;
        }
        // Half the line width, in page units, around stroked glyphs.
        let stroke = if matches!(render, 1 | 2) {
            line_width / 2.
                * (page_transform[0].abs() + page_transform[2].abs())
                    .max(page_transform[1].abs() + page_transform[3].abs())
        } else {
            0.
        };
        cursor += advance;
        if !cursor.is_finite() || cursor > 1_000_000.0 {
            return Err("continued text advance exceeds its limit".into());
        }
        let mut bounds = text_bounds(
            page_matrix,
            [horizontal[0], -size * 0.25, horizontal[1], size],
        );
        if (read_only || metrics.vertical_bounds.is_some()) && !text.is_empty() {
            let (reach, bottom, top) = match (metrics.vertical_bounds, font_boxes.get(name)) {
                (Some([bottom, top]), _) => (0., bottom, top),
                // A standard font has no outlines here, but its FontBBox holds
                // every glyph: read-only text in it (arXiv's rotated stamp)
                // reserves that box, widened by its larger side at both ends.
                (None, Some(&[left, bottom, right, top])) => {
                    (left.abs().max(right.abs()), bottom, top)
                }
                (None, None) => {
                    return Err("read-only text requires validated glyph outlines".into())
                }
            };
            let reach = reach * size / 1000.;
            let ink = text_bounds(
                page_matrix,
                [
                    horizontal[0].min(horizontal[1]) - reach,
                    bottom * size / 1000.,
                    horizontal[0].max(horizontal[1]) + reach,
                    top * size / 1000.,
                ],
            );
            bounds = [
                bounds[0].min(ink[0]),
                bounds[1].min(ink[1]),
                bounds[2].max(ink[2]),
                bounds[3].max(ink[3]),
            ];
        }
        bounds = [
            bounds[0] - stroke,
            bounds[1] - stroke,
            bounds[2] + stroke,
            bounds[3] + stroke,
        ];
        // Include actual horizontal overhang in hit boxes and clipping. The
        // vertical union covers every offered glyph. Writing additionally keeps
        // replacement ink inside these unrounded original horizontal bounds.
        // Standard-font widths cannot prove substituted glyph ink bounds.
        if !text.is_empty() && (clip.is_some() || !compound_clips.is_empty()) {
            let [bottom, top] = metrics
                .vertical_bounds
                .ok_or("clipped text requires validated embedded glyph outlines")?;
            let ink_bounds = text_bounds(
                page_matrix,
                [
                    horizontal[0],
                    bottom * size / 1000.,
                    horizontal[1],
                    top * size / 1000.,
                ],
            );
            let ink_bounds = [
                ink_bounds[0] - stroke,
                ink_bounds[1] - stroke,
                ink_bounds[2] + stroke,
                ink_bounds[3] + stroke,
            ];
            // A rectangular clip remains in the saved stream and in the worker
            // preview. Partly clipped source text is still editable; rejecting
            // it here would disable every other text object on the page.
            if clipping::contains(clip, ink_bounds).is_err() {
                let rect = clip.unwrap();
                bounds = [
                    bounds[0].clamp(rect[0], rect[2]),
                    bounds[1].clamp(rect[1], rect[3]),
                    bounds[2].clamp(rect[0], rect[2]),
                    bounds[3].clamp(rect[1], rect[3]),
                ];
            }
            for region in &compound_clips {
                region.contains(ink_bounds)?;
            }
            if !compound_clips.is_empty() {
                compound_run_clips.insert(index as u32, (compound_clips.clone(), ink_bounds));
            }
        }
        let [left, bottom, right, top] = bounds;
        let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
        let display_rect = crate::text::to_device(
            geometry.turns,
            geometry.width,
            geometry.height,
            [left - ox, bottom - oy, right - ox, top - oy],
        );
        if display_rect.iter().any(|v| !v.is_finite()) {
            return Err("text bounds exceed the display range".into());
        }
        if let Some(spacer) = &spacer {
            spacer.text(&text)?;
            continue;
        }
        if let Some(span) = &mut actual {
            span.show(index as u32, &text, metrics.vertical_bounds.is_some())?;
        }
        if read_only {
            preserved.push(Run {
                display_rect,
                minimum_height: None,
                operator: index as u32,
                text,
                font: String::from_utf8_lossy(name).into_owned(),
                size,
                matrix: page_matrix,
                advance,
            });
            continue;
        }
        font_operators.insert(index as u32, font_operator);
        horizontal_bounds.insert(index as u32, horizontal);
        text_spacing.insert(index as u32, (spacing, word_spacing));
        contexts.insert(
            index as u32,
            layout::Context {
                line_origin,
                shown: shown_matrix,
                cursor_after: cursor,
                clip,
                regions: compound_clips.clone(),
                stroke,
                size,
                scale: page_matrix[0].hypot(page_matrix[1]),
                transform: page_transform,
            },
        );
        result.runs.push(Run {
            display_rect,
            minimum_height: metrics
                .vertical_bounds
                .filter(|bounds| bounds[0] < -250.)
                .map(|bounds| {
                    size * page_matrix[2].hypot(page_matrix[3]) * (1. - bounds[0] / 1000.)
                }),
            operator: index as u32,
            text,
            font: String::from_utf8_lossy(name).into_owned(),
            size,
            matrix: page_matrix,
            advance,
        });
    }
    if actual.is_some() || spacer.is_some() {
        return Err("unterminated ActualText marked content".into());
    }
    if inside {
        return Err("unterminated text block".into());
    }
    if !states.is_empty() {
        return Err("unterminated graphics-state save".into());
    }
    tags.finish()?;
    if result.runs.is_empty() && !preserved.is_empty() {
        // Transformed, pattern-filled and tagged read-only text all land here.
        return Err(unusable_font.unwrap_or_else(|| "page contains only read-only text".into()));
    }
    // Discovery promises that every offered run can be deleted. Check the
    // actual f32 TJ compensation before offering implicit-advance text.
    for run in &result.runs {
        if continued.contains(&run.operator) {
            continuation_adjustment(run, 0.)?;
        }
    }
    let mut inspection = Inspection {
        id,
        content,
        bytes,
        patched: BTreeSet::new(),
        runs: result,
        preserved,
        form_text_bounds,
        graphics,
        paths,
        actual_text: BTreeMap::new(),
        font_operators,
        horizontal_bounds,
        text_spacing,
        leads,
        gaps,
        continued,
        groups: BTreeMap::new(),
        compound_run_clips,
        contexts,
        expanded: BTreeMap::new(),
        blocks,
        lowered: BTreeMap::new(),
        annotations: annotation_rects(doc, id).ok(),
        links: BTreeMap::new(),
    };
    for span in actual_spans {
        span.finish(&mut inspection)?;
    }
    if inspection.runs.runs.is_empty() && !inspection.preserved.is_empty() {
        return Err(unusable_font.unwrap_or_else(|| "page contains only read-only text".into()));
    }
    grouping::collect(&mut inspection);
    if inspection.blocks.is_empty() || blocks::FORCE.load(std::sync::atomic::Ordering::Relaxed) {
        inspection.blocks = blocks::geometric(&inspection, &sheet);
    }
    Ok(inspection)
}

/// The page's annotation rectangles in the original displayed page, popups
/// left out: a popup is the window a note opens in, not a mark on the page.
///
/// Bounded like every other list the scan reads, and strict about shape: an
/// annotation whose rectangle cannot be read is one the editor cannot see, and
/// a wrap that moved text under it would be moving it blind. The scan keeps
/// the failure rather than raising it (`Inspection::annotations`).
fn annotation_rects(doc: &Document, page: ObjectId) -> Result<Vec<Annotation>, String> {
    const MAX_ANNOTATIONS: usize = 4096;
    let dict = doc.get_dictionary(page).map_err(|e| e.to_string())?;
    let Ok(list) = dict.get(b"Annots") else {
        return Ok(Vec::new());
    };
    let list = crate::encoding::resolve(doc, list)
        .as_array()
        .map_err(|_| "page annotations are not an array")?;
    if list.len() > MAX_ANNOTATIONS {
        return Err("page annotation count exceeds its limit".into());
    }
    let geometry = crate::pagetree::displayed_page(doc, page);
    let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
    let mut rects = Vec::new();
    for entry in list {
        let annotation = crate::encoding::resolve(doc, entry)
            .as_dict()
            .map_err(|_| "page annotation is not a dictionary")?;
        if annotation
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|kind| kind == b"Popup")
        {
            continue;
        }
        let rect = crate::encoding::resolve(
            doc,
            annotation
                .get(b"Rect")
                .map_err(|_| "page annotation has no rectangle")?,
        )
        .as_array()
        .map_err(|_| "page annotation rectangle is not an array")?;
        let [a, b, c, d] = rect.as_slice() else {
            return Err("page annotation rectangle is not four numbers".into());
        };
        let [a, b, c, d] = [
            number(crate::encoding::resolve(doc, a))?,
            number(crate::encoding::resolve(doc, b))?,
            number(crate::encoding::resolve(doc, c))?,
            number(crate::encoding::resolve(doc, d))?,
        ];
        if ![a, b, c, d]
            .iter()
            .all(|n| n.is_finite() && n.abs() <= 1_000_000.)
        {
            return Err("page annotation rectangle exceeds its limit".into());
        }
        let user = [a.min(c), b.min(d), a.max(c), b.max(d)];
        let link = match entry {
            Object::Reference(object)
                if annotation
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|kind| kind == b"Link")
                    && !annotation.has(b"AP")
                    && !annotation.has(b"QuadPoints") =>
            {
                Some((*object, user))
            }
            _ => None,
        };
        rects.push(Annotation {
            rect: crate::text::to_device(
                geometry.turns,
                geometry.width,
                geometry.height,
                [user[0] - ox, user[1] - oy, user[2] - ox, user[3] - oy],
            ),
            link,
        });
    }
    Ok(rects)
}

// lopdf stores reals as f32, about seven significant digits. A whole line
// shown at a small Tf (Word writes Tf 1 with a scaled Tm) needs a compensation
// in the tens or hundreds of thousands, where f32 rounds by hundredths. TJ sums
// consecutive numbers (ISO 32000-1 9.4.3), so emit the exact integer part and
// the small remainder separately; both share the sign of the whole value.
// A TJ operand with the show's original leading adjustment, if it had one.
fn led(lead: Option<&Object>, items: Vec<Object>) -> Object {
    Object::Array(lead.cloned().into_iter().chain(items).collect())
}

fn continuation_adjustment(run: &Run, replacement_advance: f64) -> Result<Vec<Object>, String> {
    let wanted = (replacement_advance - run.advance) * 1000. / run.size;
    if !wanted.is_finite() || wanted.abs() > 1_000_000.0 {
        return Err("cannot preserve following text at PDF number precision".into());
    }
    let whole = wanted.trunc();
    let fraction = (wanted - whole) as f32;
    let mut parts = Vec::new();
    if whole != 0. {
        parts.push(Object::Integer(whole as i64));
    }
    if fraction != 0. || parts.is_empty() {
        parts.push(Object::Real(fraction));
    }
    let emitted = whole + f64::from(fraction);
    let saved_advance = replacement_advance - emitted * run.size / 1000.;
    let drift = (saved_advance - run.advance).abs() * run.matrix[0].abs().max(run.matrix[1].abs());
    if emitted > 0. || drift > 0.000_001 {
        return Err("cannot preserve following text at PDF number precision".into());
    }
    Ok(parts)
}

/// A replacement in its run's own font and size, positioned as its source is:
/// the source's kerns and word gaps kept around an unchanged start and end
/// (kerning.rs) when that version `fits`, and otherwise the run written afresh
/// from glyph widths, which the caller still has to check. Kept kerns can widen
/// as well as narrow, which is why a kept version that does not fit gives way.
/// Both writers use it: the byte patch with the source's own advance and ink as
/// the limit, the layout (layout.rs) with the reader's box.
///
/// Returns the TJ items (without the source's leading adjustment), their
/// advance and their horizontal ink, in text space.
#[allow(clippy::too_many_arguments)]
fn own_items(
    source: &lopdf::content::Operation,
    grouped: bool,
    led: bool,
    replacement: &str,
    metrics: &fonts::Metrics,
    gap: f64,
    (size, spacing, word_spacing): (f64, f64, f64),
    fits: impl Fn(f64, [f64; 2]) -> bool,
) -> Result<(Vec<Object>, f64, [f64; 2]), String> {
    let kept = if source.operator == "TJ" && !grouped {
        let values = source.operands[0].as_array().map_err(|e| e.to_string())?;
        kerning::kept(
            &values[usize::from(led)..],
            replacement,
            metrics,
            gap,
            (size, spacing, word_spacing),
        )
    } else {
        None
    };
    if let Some((items, advance, bounds)) =
        kept.filter(|(_, advance, bounds)| fits(*advance, *bounds))
    {
        return Ok((items, advance, bounds));
    }
    let (advance, bounds) = metrics.gapped_layout(replacement, size, spacing, word_spacing, gap)?;
    Ok((metrics.items(replacement, gap)?, advance, bounds))
}

/// Discover a complete supported page, or explain why it cannot be edited yet.
///
/// # Errors
/// Unsupported content, invalid resources, or exhausted parsing limits.
pub fn scan(doc: &Document, page: u32) -> Result<PageRuns, String> {
    inspect(doc, page).map(|page| page.runs)
}

/// Validate the entire batch before changing any page. Shared streams are cloned.
///
/// # Errors
/// Unsupported, stale, duplicate, unchanged, or overflowing replacements.
pub fn write(doc: &mut Document, changes: &[Change]) -> Result<(), String> {
    let prepared = prepare_batch(doc, changes)?;
    commit_batch(doc, prepared)
}

/// Where a batch puts what it moves, on one page: each edited run's box, and
/// every run it pushes along a line or moves down a paragraph, as hit
/// rectangles in the original displayed page, keyed by operator.
///
/// The editor outlines runs by these, so a line an earlier edit moved is
/// outlined where the reader sees it rather than where the source drew it. It
/// prepares the batch exactly as [`write`] does and writes nothing.
///
/// # Errors
/// Whatever [`write`] would refuse the batch for.
pub(crate) fn placements(
    doc: &Document,
    page: u32,
    changes: &[Change],
) -> Result<BTreeMap<u32, [f32; 4]>, String> {
    let mut prepared = prepare_batch(doc, changes)?;
    let mut placed = BTreeMap::new();
    if let Some(inspection) = prepared.remove(&page) {
        // Operator order is the order `write` prepared them in, so a run two
        // edits pushed ends up where the later one, carrying the running
        // total, put it.
        for (operator, value) in inspection.expanded {
            placed.insert(operator as u32, value.rect);
            placed.extend(value.placed.iter().copied());
        }
    }
    Ok(placed)
}

/// Everything [`write`] decides before it changes the document: each page's
/// inspection with its replacements, pushes and moved lines applied to the
/// decoded content. It reads the document and nothing else.
fn prepare_batch(doc: &Document, changes: &[Change]) -> Result<BTreeMap<u32, Inspection>, String> {
    if changes.len() > MAX_CHANGES {
        return Err("too many text replacements".into());
    }
    let mut prepared = BTreeMap::new();
    let mut seen = BTreeSet::new();
    // Left to right along each page, because an edit that pushes its line has
    // to be written before the edits it pushes: each of those is placed where
    // the one before it left it (`layout::Placement::inherited`) rather than
    // being given a displacement its own expansion would throw away.
    let mut ordered: Vec<&Change> = changes.iter().collect();
    ordered.sort_by_key(|change| (change.page, change.operator));
    let mut edited: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for change in &ordered {
        edited
            .entry(change.page)
            .or_default()
            .insert(change.operator);
    }
    // Where each show on a pushed line has ended up, page points along its text
    // axis, as a running total: the operand is built once at the end from the
    // source's own bytes, so the last edit to push a show is the one that says
    // how far it went.
    let mut shifts: BTreeMap<(u32, u32), f64> = BTreeMap::new();
    // The subset of those that are given a displacement of their own. The rest
    // of a pushed line rides the text cursor from the show before it, and
    // writing a second displacement there would push it twice; `layout::drag`
    // is what tells the two apart.
    let mut pushes: BTreeMap<(u32, u32), f64> = BTreeMap::new();
    for change in ordered {
        if !seen.insert((change.page, change.operator)) {
            return Err("duplicate text replacement".into());
        }
        if change.replacement.chars().count() > MAX_TEXT
            || change.replacement.chars().any(|ch| {
                (ch.is_control() && !(ch == '\n' && change.layout.as_ref().is_some_and(|l| l.wrap)))
                    || matches!(ch, '\u{2028}' | '\u{2029}')
            })
        {
            return Err("invalid or oversized replacement text".into());
        }
        if change.original == change.replacement && change.layout.is_none() {
            return Err("text replacement is unchanged".into());
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = prepared.entry(change.page) {
            entry.insert(inspect(doc, change.page)?);
        }
        if change.layout.is_some() {
            let page = prepared.get_mut(&change.page).ok_or("missing text page")?;
            let placement = layout::Placement {
                inherited: shifts
                    .get(&(change.page, change.operator))
                    .copied()
                    .unwrap_or_default(),
                edited: edited.get(&change.page).cloned().unwrap_or_default(),
            };
            let replacement = layout::prepare(doc, page, change, &placement)?;
            if page.actual_text.contains_key(&change.operator) && replacement.lines > 1 {
                return Err("ActualText editing currently requires a single line".into());
            }
            // Every show this edit pushes, told where it ended up. The ones
            // this batch also replaces are in here without being in `moved`:
            // they move because their own `prepare` places them there, not
            // because anything gave their show a displacement.
            for show in &replacement.line {
                shifts.insert((change.page, *show), replacement.shift);
            }
            for (show, shift) in &replacement.moved {
                pushes.insert((change.page, *show), *shift);
            }
            // A wrap moves each of these down; an earlier edit on the same
            // visual line -- another block's run set before this paragraph --
            // may already have pushed one along, and the two are written from
            // separate copies of its bytes. Every show belongs to one block, so
            // two wraps never move the same one.
            for (show, operations) in &replacement.lowered {
                if shifts.contains_key(&(change.page, *show)) {
                    return Err(layout::WRAP_CONFLICT.into());
                }
                page.lowered.insert(*show as usize, operations.clone());
                page.patched.insert(*show as usize);
            }
            if !replacement.links.is_empty() {
                let geometry = crate::pagetree::displayed_page(doc, page.id);
                let (ox, oy) = (f64::from(geometry.origin.0), f64::from(geometry.origin.1));
                let user = |rect: [f32; 4]| {
                    let [l, b, r, t] = crate::text::from_device(
                        geometry.turns,
                        geometry.width,
                        geometry.height,
                        rect,
                    );
                    [l + ox, b + oy, r + ox, t + oy]
                };
                for (link, by) in &replacement.links {
                    let Some((rect, source)) = page.annotations.iter().flatten().find_map(|a| {
                        a.link
                            .filter(|(id, _)| id == link)
                            .map(|(_, u)| (a.rect, u))
                    }) else {
                        return Err("missing link annotation".into());
                    };
                    // The move in user space, from the displayed rectangle
                    // before and after, applied to the source's own numbers.
                    let moved = rect.map(f64::from);
                    let moved = [
                        moved[0] + by[0],
                        moved[1] + by[1],
                        moved[2] + by[0],
                        moved[3] + by[1],
                    ]
                    .map(|v| v as f32);
                    let (was, now) = (user(rect), user(moved));
                    let new = [0, 1, 2, 3].map(|i| source[i] + now[i] - was[i]);
                    page.links.insert(*link, new);
                }
            }
            page.expanded.insert(change.operator as usize, replacement);
            page.patched.insert(change.operator as usize);
            let members = page
                .groups
                .get(&change.operator)
                .cloned()
                .unwrap_or_default();
            for member in members {
                let show = &mut page.content.operations[member as usize];
                show.operands[0] = if show.operator == "TJ" {
                    Object::Array(vec![Object::string_literal(Vec::new())])
                } else {
                    Object::string_literal(Vec::new())
                };
                page.patched.insert(member as usize);
            }
            continue;
        }
        let Inspection {
            id,
            content,
            runs,
            font_operators,
            horizontal_bounds,
            text_spacing,
            leads,
            gaps,
            continued,
            patched,
            groups,
            ..
        } = prepared.get_mut(&change.page).ok_or("missing text page")?;
        let run = runs
            .runs
            .iter()
            .find(|run| run.operator == change.operator)
            .ok_or("text run no longer exists")?;
        if change.revision != runs.revision || change.original != run.text {
            return Err("text changed since this run was inspected".into());
        }
        // Use the original operand bytes, not the lossy display name, to resolve
        // a resource. Font names are PDF names and need not be valid UTF-8.
        let font_operator = font_operators
            .get(&change.operator)
            .ok_or("missing text font")?;
        let name = content.operations[*font_operator].operands[0]
            .as_name()
            .map_err(|e| e.to_string())?;
        let metrics = font(doc, resources(doc, *id)?, name)?.preferring(&shown(
            content,
            groups,
            change.operator,
        ));
        let (spacing, word_spacing) = *text_spacing
            .get(&change.operator)
            .ok_or("missing text spacing")?;
        let gap = gaps.get(&change.operator).copied().unwrap_or(DEFAULT_GAP);
        let original = *horizontal_bounds
            .get(&change.operator)
            .ok_or("missing text ink bounds")?;
        let (mut items, replacement_advance, replacement_bounds) = own_items(
            &content.operations[change.operator as usize],
            groups.contains_key(&change.operator),
            leads.contains_key(&change.operator),
            &change.replacement,
            &metrics,
            gap,
            (run.size, spacing, word_spacing),
            |advance, bounds| {
                advance <= run.advance + 0.000_001
                    && bounds[0] >= original[0]
                    && bounds[1] <= original[1]
            },
        )?;
        if replacement_advance > run.advance + 0.000_001 {
            return Err("replacement would exceed the original text advance".into());
        }
        // The right edge gets the advance's rounding allowance: the scan sums a
        // run's widths glyph by glyph, the layout in its own order, so an
        // equal-width replacement can land a few ulps past the source (337.74
        // against 337.73999999999995 on the arXiv stamp). The left edge is one
        // glyph's overhang, summed from nothing, and needs none.
        if replacement_bounds[0] < original[0] || replacement_bounds[1] > original[1] + 0.000_001 {
            return Err("replacement ink would exceed the original text bounds".into());
        }
        patched.insert(change.operator as usize);
        let show = &mut content.operations[change.operator as usize];
        let lead = leads.get(&change.operator);
        show.operands[0] = if continued.contains(&change.operator) {
            // TJ offsets are subtracted in thousandths of text space. Keep
            // following shows fixed, including after deletion of this string.
            items.extend(continuation_adjustment(run, replacement_advance)?);
            show.operator = "TJ".into();
            led(lead, items)
        } else if show.operator == "TJ" || items.len() > 1 {
            // Word gaps need an array even where the source was a Tj.
            show.operator = "TJ".into();
            led(lead, items)
        } else {
            items.remove(0)
        };
        if let Some(members) = groups.get(&change.operator) {
            for &member in members {
                let show = &mut content.operations[member as usize];
                show.operands[0] = if show.operator == "TJ" {
                    // A member's own leading adjustment moves nothing: grouping
                    // requires a Td before every member.
                    Object::Array(vec![Object::string_literal(Vec::new())])
                } else {
                    Object::string_literal(Vec::new())
                };
                patched.insert(member as usize);
            }
        }
    }
    // The line's own push, applied once the whole batch is prepared and always
    // from the source's own array, so that two edits on one line do not add
    // their displacements to each other's output.
    for ((page, operator), shift) in &pushes {
        let page = prepared.get_mut(page).ok_or("missing text page")?;
        // A later edit pushed a line an earlier one moved down: the push would
        // be written into the source's array while the wrap draws that show
        // from its own copy of it.
        if page.lowered.contains_key(&(*operator as usize)) {
            return Err(layout::WRAP_CONFLICT.into());
        }
        let moved = layout::push(page, *operator, *shift)?;
        let show = &mut page.content.operations[*operator as usize];
        show.operator = "TJ".into();
        show.operands[0] = moved;
        page.patched.insert(*operator as usize);
    }
    // Logical replacement strings are patched with their owning show.
    for change in changes {
        actual::patch(
            prepared.get_mut(&change.page).ok_or("missing text page")?,
            change.operator,
            &change.replacement,
        )?;
    }
    Ok(prepared)
}

/// A painted path outside any text object, as `Inspection::paths` records it:
/// the first and last of its operators, and the transform in force at them.
#[derive(Clone, Copy, Debug)]
struct DrawnPath {
    operations: (usize, usize),
    transform: [f64; 6],
}

/// Writes a prepared batch into the document: one new content stream per page,
/// and the fallback fonts its replacements installed.
fn commit_batch(doc: &mut Document, prepared: BTreeMap<u32, Inspection>) -> Result<(), String> {
    let ready = prepared
        .into_values()
        .map(
            |Inspection {
                 id,
                 content,
                 bytes,
                 patched,
                 expanded,
                 lowered,
                 links,
                 ..
             }| {
                let mut font_names = BTreeSet::new();
                for operation in content
                    .operations
                    .iter()
                    .chain(expanded.values().flat_map(|value| &value.operations))
                {
                    if operation.operator == "Tf" {
                        font_names
                            .insert(operation.operands[0].as_name().map_err(|e| e.to_string())?);
                    }
                }
                if font_names.len() > 32 {
                    return Err("Text edits would exceed the page's 32-font limit".into());
                }
                let expansions = expanded
                    .iter()
                    .map(|(index, value)| (*index, value.operations.clone()))
                    .chain(lowered)
                    .collect();
                streams::rewrite_expanded(&bytes, &content, &patched, &expansions)
                    .map(|bytes| (id, bytes, expanded, links))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    let mut programs = BTreeMap::new();
    for (page, bytes, expanded, links) in ready {
        for (link, rect) in links {
            doc.get_dictionary_mut(link)
                .map_err(|e| e.to_string())?
                .set("Rect", rect.map(|v| Object::Real(v as f32)).to_vec());
        }
        if expanded.values().any(|value| value.fallback.is_some()) {
            let mut resources = resources(doc, page)?.clone();
            let mut fonts =
                dictionary(doc, resources.get(b"Font").map_err(|e| e.to_string())?)?.clone();
            for value in expanded.into_values() {
                if let Some(font) = value.fallback {
                    fonts.set(value.name, font.install(doc, &mut programs)?);
                }
            }
            resources.set("Font", fonts);
            doc.get_dictionary_mut(page)
                .map_err(|e| e.to_string())?
                .set("Resources", resources);
        }
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes));
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?
            .set("Contents", stream);
    }
    Ok(())
}

#[cfg(test)]
mod continuation_tests;

#[cfg(test)]
mod leading_tests;

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod push_tests;

#[cfg(test)]
mod blocks_tests;
#[cfg(test)]
mod wrap_tests;

#[cfg(test)]
mod rotation_tests;

#[cfg(test)]
mod reflected_tests;

#[cfg(test)]
mod spacing_tests;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::dictionary;

    /// An `Edit` is a `Change` with one optional key, on the wire as in the
    /// type --- and a body written before the key existed reads back as the
    /// replacement on the opened document it meant.
    ///
    /// **Flattened rather than nested**, which is what makes the second half
    /// true: a nested `change` object would have made every stored journal,
    /// plan and reply from before today unreadable, and the failure would
    /// have been a replacement silently missing rather than an error.
    #[test]
    fn an_edit_is_a_change_with_one_optional_key() {
        let change = Change {
            layout: None,
            page: 2,
            revision: vec![1; 32],
            operator: 3,
            original: "SYNTHETIC ORIGINAL".into(),
            replacement: "SYNTHETIC EDIT".into(),
        };
        let opened = serde_json::to_value(Edit::opened(change.clone())).expect("serialise");
        assert_eq!(
            opened,
            serde_json::to_value(&change).expect("serialise the change"),
            "a replacement on the opened document is byte-identical to the change it wraps"
        );
        let imported = serde_json::to_value(Edit::imported(4, change.clone())).expect("serialise");
        assert_eq!(
            imported.get("source").and_then(serde_json::Value::as_u64),
            Some(4)
        );
        assert_eq!(
            imported.get("page").and_then(serde_json::Value::as_u64),
            Some(2),
            "the change's own keys stay at the top level"
        );
        // And back, including the shape that predates the key.
        let legacy: Edit = serde_json::from_value(opened).expect("a body with no source");
        assert_eq!(legacy, Edit::opened(change.clone()));
        let back: Edit = serde_json::from_value(imported).expect("a body with one");
        assert_eq!(back, Edit::imported(4, change));
    }

    // Clipping is authored artwork, not a reason to reject unrelated text.
    // Exercise a real replacement and prove every non-text operator survives.
    pub(crate) fn clipped_roundtrip(source: &Document) -> PageRuns {
        let before = inspect(source, 0).unwrap();
        let run = &before.runs.runs[0];
        let mut doc = source.clone();
        write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.runs.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: String::new(),
            }],
        )
        .unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(
            after.content.operations.len(),
            before.content.operations.len()
        );
        for (index, (old, new)) in before
            .content
            .operations
            .iter()
            .zip(&after.content.operations)
            .enumerate()
        {
            if index != run.operator as usize {
                assert_eq!(old.operator, new.operator);
                assert_eq!(old.operands, new.operands);
            }
        }
        for (id, object) in &source.objects {
            if *id != before.id {
                assert_eq!(&doc.objects[id], object);
            }
        }
        assert!(after.runs.runs[0].text.is_empty());
        before.runs
    }

    #[test]
    fn textedit_dense_pages_preserve_operators_and_enforce_work_limit() {
        let body = format!(
            "{}BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET",
            "0 g\n".repeat(6000)
        );
        let mut doc = with_content(body.as_bytes());
        let before = scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 1);
        write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "SYNTHETIC FIRST".into(),
                replacement: "FIRST".into(),
            }],
        )
        .unwrap();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let bytes = page_content(&doc, page).unwrap();
        assert!(bytes.starts_with("0 g\n".repeat(6000).as_bytes()));
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "FIRST");
        assert!(scan(&with_content("0 g\n".repeat(MAX_OPERATIONS).as_bytes()), 0).is_ok());
        assert!(scan(
            &with_content("0 g\n".repeat(MAX_OPERATIONS + 1).as_bytes()),
            0
        )
        .unwrap_err()
        .contains("operator count"));
    }

    #[test]
    fn textedit_kerning_geometry_and_rewrite_preserve_other_shows() {
        let mut doc = with_content(b"q 2 0 0 3 0 0 cm BT /F1 20 Tf 40 TL 10 50 Td [(A) 120 (W) -50 (AY)] TJ T* [(SECOND) -20 ( LINE)] TJ ET Q");
        let before = scan(&doc, 0).unwrap();
        let run = &before.runs[0];
        let expected = crate::textbox::advance("AWAY", 20.) - 1.4;
        assert_eq!(run.text, "AWAY");
        assert!((run.advance - expected).abs() < 1e-6);
        assert!(
            (f64::from(run.display_rect[2] - run.display_rect[0]) - expected * 2.).abs() < 1e-4
        );
        assert_eq!(run.matrix, [2., 0., 0., 3., 20., 150.]);
        let operations = inspect(&doc, 0).unwrap().content.operations;
        let update = Change {
            replacement: "A".into(),
            ..change(&doc)
        };
        write(&mut doc, std::slice::from_ref(&update)).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.runs.runs[0].text, "A");
        assert_eq!(after.runs.runs[1], before.runs[1]);
        for (index, (actual, original)) in
            after.content.operations.iter().zip(&operations).enumerate()
        {
            assert_eq!(actual.operator, original.operator);
            if index == update.operator as usize {
                assert_eq!(
                    actual.operands,
                    vec![Object::Array(vec![Object::string_literal("A")])]
                );
            } else {
                assert_eq!(actual.operands, original.operands);
            }
        }
        let empty = Change {
            replacement: String::new(),
            ..change(&doc)
        };
        write(&mut doc, &[empty]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
    }

    #[test]
    fn textedit_kerning_overflow_is_checked_against_adjusted_width() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 180 Td [(W) 500 (W)] TJ ET");
        // WA fits the unkerned WW, but exceeds WW with its authored adjustment.
        let update = Change {
            replacement: "WA".into(),
            ..change(&doc)
        };
        let before = doc.objects.clone();
        assert!(write(&mut doc, &[update])
            .unwrap_err()
            .contains("exceed the original"));
        assert_eq!(doc.objects, before);
    }

    #[test]
    fn textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays() {
        // A bounded final cursor must not hide an out-of-bounds intermediate one.
        // Scale the font into the page so later ink bounds cannot mask this guard.
        for (adjustments, accepted) in [
            ("-500000 -400000 500000 400000", true),
            ("-600000 -600000 600000 600000", false),
            // Behind the origin and back, drawing nothing on the way.
            ("9999 -9999", true),
        ] {
            let content =
                format!("BT /F1 1000 Tf 0.001 0 0 0.001 40 180 Tm [(A) {adjustments}] TJ ET");
            assert_eq!(
                scan(&with_content(content.as_bytes()), 0).is_ok(),
                accepted,
                "{adjustments}"
            );
        }
        // One leading number is where the run starts (see array_text); two,
        // or one with nothing after it, is still refused.
        for array in [
            "[]",
            "[1]",
            "[1 2 (TEXT)]",
            "[1000001 (TEXT)]",
            "[(A) [0] (B)]",
            "[(A) /Name (B)]",
            "[(A) null (B)]",
            "[(A) true (B)]",
            "[(A) 1000001 (B)]",
            "[(A) -1000001 (B)]",
            "[(A) -600000 -600000 (B)]",
        ] {
            let content = format!("BT /F1 1000 Tf 40 180 Td {array} TJ ET");
            assert!(
                scan(&with_content(content.as_bytes()), 0).is_err(),
                "accepted {array}"
            );
        }
        // A string that ends before an earlier one draws back over it: its run
        // is not in reading order and stays read-only, and the page stays
        // editable. ConTeXt sets its footers this way.
        for array in [
            "[(A) 9999 (B)]",
            "[(WWW) 2000 (i)]",
            "[(19) 9239 (TEXT)]",
            "[(19) 9239 (TEXT) -20000 (X)]",
            "[(TEXT) 1]",
        ] {
            let content = format!(
                "BT /F1 12 Tf 40 180 Td {array} TJ ET BT /F1 12 Tf 40 140 Td (FIRST) Tj ET"
            );
            let runs = scan(&with_content(content.as_bytes()), 0).unwrap().runs;
            assert_eq!(
                runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(),
                ["FIRST"],
                "{array}"
            );
        }
        // After a continued show, an opening shift can carry the cursor past
        // its bound; that is refused before the run is laid out.
        assert_eq!(
            scan(
                &with_content(b"BT /F1 1000 Tf 40 180 Td (A) Tj [-1000000 (B)] TJ ET"),
                0
            )
            .unwrap_err(),
            "kerning position exceeds its limit"
        );
        for content in [
            "BT /F1 12 Tf 40 180 Td [(TEXT)] Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) TJ ET",
            "BT /F1 12 Tf 40 180 Td [(TEXT)] [(MORE)] TJ ET",
            "BT /F1 12 Tf [(TEXT)] TJ ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET [(MORE)] TJ",
        ] {
            assert!(
                scan(&with_content(content.as_bytes()), 0).is_err(),
                "accepted {content}"
            );
        }
    }

    #[test]
    fn textedit_kerning_bounds_total_characters_and_array_items() {
        for (body, accepted) in [
            (
                format!(
                    "({}) ({})",
                    "A".repeat(MAX_TEXT / 2),
                    "A".repeat(MAX_TEXT / 2)
                ),
                true,
            ),
            (format!("({}) (B)", "A".repeat(MAX_TEXT)), false),
            ("() ".repeat(MAX_TEXT), true),
            ("() ".repeat(MAX_TEXT + 1), false),
        ] {
            let content = format!("BT /F1 12 Tf 40 180 Td [{body}] TJ ET");
            assert_eq!(scan(&with_content(content.as_bytes()), 0).is_ok(), accepted);
        }
        let mut content = b"BT /F1 12 Tf 40 180 Td [(".to_vec();
        content.extend(vec![0xe4; MAX_TEXT / 2]);
        content.extend(b") (".to_vec());
        content.extend(vec![0xdf; MAX_TEXT / 2]);
        content.extend(b")] TJ ET");
        assert_eq!(
            scan(&with_content(&content), 0).unwrap().runs[0]
                .text
                .chars()
                .count(),
            MAX_TEXT
        );
        for byte in [0, 31, 127, 159] {
            let content = [
                b"BT /F1 12 Tf 40 180 Td [(A) (".as_slice(),
                &[byte],
                b")] TJ ET",
            ]
            .concat();
            assert!(scan(&with_content(&content), 0).is_err());
        }
    }

    #[test]
    fn textedit_latin1_uses_single_pdf_bytes_and_bounds_characters() {
        assert_eq!(
            encode_text("ÄÖÜ äöü ß î ø").unwrap(),
            b"\xC4\xD6\xDC \xE4\xF6\xFC \xDF \xEE \xF8"
        );
        let bytes: Vec<u8> = (32..=126).chain(160..=255).collect();
        assert_eq!(encode_text(&decode_text(&bytes).unwrap()).unwrap(), bytes);
        for byte in (0..32).chain(127..160) {
            assert!(decode_text(&[byte]).is_err());
            assert!(encode_text(&char::from(byte).to_string()).is_err());
        }
        for text in ["α", "€", "a\u{308}", "日本語"] {
            assert!(encode_text(text).is_err());
        }
        assert_eq!(encode_text(&"ä".repeat(MAX_TEXT)).unwrap().len(), MAX_TEXT);
        assert!(encode_text(&"ä".repeat(MAX_TEXT + 1)).is_err());
        assert!(decode_text(&vec![0xE4; MAX_TEXT + 1]).is_err());
    }

    #[test]
    fn textedit_dash_slots_are_bijective_and_count_characters() {
        let mut accepted = 0;
        for value in 0..=0x10ffff {
            let Some(ch) = char::from_u32(value) else {
                continue;
            };
            let expected = (32..=126).contains(&value)
                || (160..=255).contains(&value)
                || value == 0x2212
                || WINANSI_EXTRA
                    .iter()
                    .any(|(_, extra)| *extra as u32 == value);
            let slot = character_slot(ch);
            assert_eq!(slot.is_some(), expected, "U+{value:04X}");
            if let Some(slot) = slot {
                accepted += 1;
                assert_eq!(slot_character(slot), ch);
            }
        }
        assert_eq!(accepted, 218);
        assert_eq!(
            encode_text(&"\u{2013}".repeat(MAX_TEXT)).unwrap(),
            vec![0x96; MAX_TEXT]
        );
        assert!(encode_text(&"\u{2013}".repeat(MAX_TEXT + 1)).is_err());
        assert!(decode_text(&[0x96]).is_err());
    }

    #[test]
    fn textedit_latin1_width_refuses_sharp_s_and_accented_i_overflow() {
        for (original, replacement) in [("s", "ß"), ("i", "î"), ("o", "ø")] {
            let raw = format!("BT /F1 12 Tf 40 180 Td ({original}) Tj ET");
            let mut doc = with_content(raw.as_bytes());
            let change = Change {
                replacement: replacement.into(),
                ..change(&doc)
            };
            let before = doc.objects.clone();
            assert!(write(&mut doc, &[change])
                .unwrap_err()
                .contains("exceed the original"));
            assert_eq!(doc.objects, before);
        }
    }

    #[test]
    fn textedit_latin1_rewrites_and_rediscovers_without_utf8_in_the_operand() {
        let mut doc = fixture();
        let update = Change {
            replacement: "GEPRÜFT ß".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let Inspection { content, runs, .. } = inspect(&doc, 0).unwrap();
        assert_eq!(runs.runs[0].text, "GEPRÜFT ß");
        assert_eq!(
            content.operations[runs.runs[0].operator as usize].operands[0]
                .as_str()
                .unwrap(),
            b"GEPR\xDCFT \xDF"
        );
        assert_eq!(runs.runs[1].text, "SYNTHETIC SECOND");
        let update = Change {
            replacement: "ASCII".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "ASCII");
    }

    pub(crate) fn fixture() -> Document {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        let contents = doc.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\nBT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET".to_vec()));
        let pages: Vec<Object> = (0..2)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page", "Parent" => root, "Contents" => contents,
                })
                .into()
            })
            .collect();
        doc.objects.insert(
            root,
            dictionary! { "Type" => "Pages", "Kids" => pages, "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 240.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
            }
            .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", catalog);
        doc
    }

    pub(crate) fn change(doc: &Document) -> Change {
        let runs = scan(doc, 0).unwrap();
        Change {
            layout: None,
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: runs.runs[0].text.clone(),
            replacement: "EDITED FIRST".into(),
        }
    }

    pub(super) fn with_content(bytes: &[u8]) -> Document {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        doc
    }

    #[test]
    fn textedit_reportlab_font_setup_and_multiline_positions_survive_shorter_edits() {
        // Produced independently by testdata/make_textedit_reportlab.py.
        let mut doc = with_content(b"1 0 0 1 0 0 cm BT /F1 12 Tf 14.4 TL ET\nBT 1 0 0 1 40 180 Tm 40 TL (SYNTHETIC FIRST) Tj T* (SYNTHETIC SECOND) Tj T* ET");
        let Inspection {
            content: before,
            runs: mapped,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(
            mapped.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [8, 10]
        );
        assert_eq!(mapped.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        assert_eq!(mapped.runs[1].matrix, [1., 0., 0., 1., 40., 140.]);
        assert_eq!(mapped.runs[1].size, 12.);
        let mut edit = change(&doc);
        edit.replacement = "X".into();
        write(&mut doc, &[edit]).unwrap();
        let Inspection {
            content: after,
            runs: saved,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "X");
        assert_eq!(saved.runs[1], mapped.runs[1]);
        assert_eq!(before.operations.len(), after.operations.len());
        for (index, (a, b)) in before.operations.iter().zip(&after.operations).enumerate() {
            if index != 8 {
                assert_eq!(a.operator, b.operator, "changed operator {index}");
                assert_eq!(a.operands, b.operands, "changed operands {index}");
            }
        }
    }

    #[test]
    fn textedit_line_positions_use_scaled_line_matrix_and_reset_on_bt() {
        let doc = with_content(b"BT /F1 12 Tf 20 TL 2 0 0 3 40 180 Tm (FIRST) Tj 10 -10 Td (SECOND) Tj T* (THIRD) Tj ET BT 5 200 Td (FOURTH) Tj T* (FIFTH) Tj ET");
        let runs = scan(&doc, 0).unwrap().runs;
        assert_eq!(
            runs.iter()
                .map(|r| [r.matrix[4], r.matrix[5]])
                .collect::<Vec<_>>(),
            [[40., 180.], [60., 150.], [60., 90.], [5., 200.], [5., 180.]]
        );
        assert_eq!(runs[3].matrix[..4], [1., 0., 0., 1.]);
        assert!(runs.iter().all(|r| r.size == 12.));
    }

    #[test]
    fn textedit_graphics_stack_restores_font_size_and_leading() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 TL ET q BT /F1 8 Tf 10 TL ET q BT /F1 6 Tf 5 TL ET Q BT 40 180 Td (INNER) Tj T* (LINE) Tj ET Q BT 40 140 Td (OUTER) Tj T* (LINE) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before.runs.iter().map(|r| r.size).collect::<Vec<_>>(),
            [8., 8., 12., 12.]
        );
        assert_eq!(
            before.runs.iter().map(|r| r.matrix[5]).collect::<Vec<_>>(),
            [180., 170., 140., 100.]
        );
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
    }

    #[test]
    fn textedit_graphics_stack_requires_balanced_bounded_outer_saves() {
        for bytes in [
            "Q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET",
            "q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf q 40 180 Td (TEXT) Tj Q ET",
            "q BT /F1 12 Tf Q 40 180 Td (TEXT) Tj ET",
            "q BT /F1 12 Tf ET Q BT 40 180 Td (TEXT) Tj ET",
            "1 q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET Q",
        ] {
            let doc = with_content(bytes.as_bytes());
            assert!(scan(&doc, 0).is_err(), "accepted {bytes}");
        }
        for depth in [64, 65] {
            let bytes = format!(
                "{}BT /F1 12 Tf 40 180 Td (TEXT) Tj ET {}",
                "q ".repeat(depth),
                "Q ".repeat(depth)
            );
            assert_eq!(
                scan(&with_content(bytes.as_bytes()), 0).is_ok(),
                depth == 64
            );
        }
    }

    #[test]
    fn textedit_page_translations_compose_restore_and_preserve_following_runs() {
        let mut doc = with_content(b"BT /F1 12 Tf 40 TL ET q 1 0 0 1 20 100 cm q 1 0 0 1 10 20 cm BT 2 0 0 3 10 60 Tm (FIRST) Tj T* (SECOND) Tj ET Q BT 1 0 0 1 20 40 Tm (THIRD) Tj ET Q BT 40 100 Td (FOURTH) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before
                .runs
                .iter()
                .map(|r| [r.matrix[4], r.matrix[5]])
                .collect::<Vec<_>>(),
            [[40., 180.], [40., 60.], [40., 140.], [40., 100.]]
        );
        assert_eq!(before.runs[0].matrix[..4], [2., 0., 0., 3.]);
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
    }

    #[test]
    fn textedit_translated_hitboxes_match_absolute_positions_after_crop_and_rotation() {
        let translated = b"q 1 0 0 1 30 150 cm BT /F1 12 Tf 2 0 0 3 10 30 Tm (TEXT) Tj ET Q";
        let absolute = b"BT /F1 12 Tf 2 0 0 3 40 180 Tm (TEXT) Tj ET";
        for rotation in [0, 90, 180, 270] {
            let mut results = Vec::new();
            for bytes in [translated.as_slice(), absolute.as_slice()] {
                let mut doc = with_content(bytes);
                let page = crate::pagetree::ordered_pages(&doc)[0];
                let page = doc.get_dictionary_mut(page).unwrap();
                page.set("Rotate", rotation);
                page.set(
                    "CropBox",
                    vec![10.into(), 20.into(), 290.into(), 220.into()],
                );
                let run = scan(&doc, 0).unwrap().runs.remove(0);
                results.push((run.matrix, run.display_rect));
            }
            assert_eq!(results[0], results[1], "rotation {rotation}");
        }
    }

    #[test]
    fn textedit_explicit_defaults_preserve_geometry_and_saved_operators() {
        let baseline = with_content(b"BT /F1 12 Tf 40 TL 40 180 Td (FIRST) Tj T* (SECOND) Tj ET");
        let mut doc = with_content(b"0 Tc 0 Tw 100 Tz 0 Ts 0 Tr q BT /F1 12 Tf 40 TL 40 180 Td 0.0 Tc -0.0 Tw 100.0 Tz 0.0 Ts 0 Tr (FIRST) Tj 0 Tc 0 Tw 100 Tz 0 Ts 0 Tr T* (SECOND) Tj ET Q");
        let expected = scan(&baseline, 0).unwrap();
        let before = scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), expected.runs.len());
        for (actual, expected) in before.runs.iter().zip(expected.runs) {
            let mut actual = actual.clone();
            actual.operator = expected.operator;
            assert_eq!(actual, expected);
        }
        // Untouched operands retain their original spelling, including real zero.
        let operations = inspect(&doc, 0).unwrap().content.operations;
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, std::slice::from_ref(&update)).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.runs.runs[0].text, "IN");
        assert_eq!(after.runs.runs[1], before.runs[1]);
        assert_eq!(after.content.operations.len(), operations.len());
        for (index, (actual, expected)) in
            after.content.operations.iter().zip(operations).enumerate()
        {
            if index != update.operator as usize {
                assert_eq!(actual.operator, expected.operator);
                assert_eq!(actual.operands, expected.operands);
            }
        }
    }

    #[test]
    fn textedit_default_setters_refuse_nondefault_and_malformed_operands() {
        for (operator, default) in [("Ts", "0"), ("Tz", "100"), ("Tr", "0")] {
            for operand in [
                "", "0 0", "(0)", "/Zero", "[0]", "true", "null", "1", "-1", "0.01", "99.99",
                "100.01", "1000001",
            ] {
                if operator == "Tr" && operand == "1" {
                    continue; // Stroked text is a supported mode.
                }
                // A later default setter must not hide an earlier unsupported one.
                let bytes = format!(
                    "{operand} {operator} {default} {operator} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET"
                );
                assert!(
                    scan(&with_content(bytes.as_bytes()), 0).is_err(),
                    "accepted {operand} {operator}"
                );
            }
        }
        // Rendering mode is an integer. Fill, stroke, both and invisible are
        // editable; modes 4..7 add the glyphs to the clipping path.
        for (mode, accepted) in [
            ("0.0", false),
            ("1", true),
            ("2", true),
            ("3", true),
            ("4", false),
            ("5", false),
            ("6", false),
            ("7", false),
            ("8", false),
        ] {
            let bytes = format!("BT /F1 12 Tf 40 180 Td {mode} Tr (TEXT) Tj ET");
            assert_eq!(
                scan(&with_content(bytes.as_bytes()), 0).is_ok(),
                accepted,
                "{mode} Tr"
            );
        }
    }

    #[test]
    fn textedit_default_setters_do_not_position_a_following_show() {
        for setting in ["0 Tc", "0 Tw", "100 Tz", "0 Ts", "0 Tr"] {
            let content = format!("BT /F1 12 Tf {setting} (TEXT) Tj ET");
            assert!(
                scan(&with_content(content.as_bytes()), 0).is_err(),
                "accepted {content}"
            );
        }
    }

    #[test]
    fn textedit_page_scales_compose_restore_and_preserve_following_runs() {
        let mut doc = with_content(b"BT /F1 12 Tf 4 TL ET q 2 0 0 3 20 30 cm q .5 0 0 2 -5 10 cm BT 2 0 0 3 10 20 Tm (FIRST) Tj T* (SECOND) Tj ET Q BT 1 0 0 1 10 20 Tm (THIRD) Tj ET Q BT 40 140 Td (FOURTH) Tj ET");
        let before = scan(&doc, 0).unwrap();
        assert_eq!(
            before.runs.iter().map(|r| r.matrix).collect::<Vec<_>>(),
            [
                [2., 0., 0., 18., 20., 180.],
                [2., 0., 0., 18., 20., 108.],
                [2., 0., 0., 3., 40., 90.],
                [1., 0., 0., 1., 40., 140.],
            ]
        );
        let update = Change {
            replacement: "IN".into(),
            ..change(&doc)
        };
        write(&mut doc, &[update]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1..], before.runs[1..]);
        let unchanged = doc.clone();
        let overflow = Change {
            replacement: "TOO LONG".into(),
            ..change(&doc)
        };
        assert!(write(&mut doc, &[overflow]).is_err());
        assert_eq!(doc.objects, unchanged.objects);
    }

    #[test]
    fn textedit_scaled_hitboxes_match_absolute_matrices_after_crop_and_rotation() {
        let scaled =
            b"q 2 0 0 .5 20 150 cm 1 0 0 1 10 20 cm BT /F1 12 Tf 3 0 0 4 5 40 Tm (TEXT) Tj ET Q";
        let absolute = b"BT /F1 12 Tf 6 0 0 2 50 180 Tm (TEXT) Tj ET";
        for rotation in [0, 90, 180, 270] {
            let mut results = Vec::new();
            for bytes in [scaled.as_slice(), absolute.as_slice()] {
                let mut doc = with_content(bytes);
                let page = crate::pagetree::ordered_pages(&doc)[0];
                let page = doc.get_dictionary_mut(page).unwrap();
                page.set("Rotate", rotation);
                page.set(
                    "CropBox",
                    vec![10.into(), 20.into(), 290.into(), 220.into()],
                );
                let run = scan(&doc, 0).unwrap().runs.remove(0);
                results.push((run.matrix, run.display_rect, run.advance));
            }
            assert_eq!(results[0], results[1], "rotation {rotation}");
        }
    }

    #[test]
    fn textedit_page_scales_bound_composed_scales_and_positions() {
        for prefix in [
            "1000000 0 0 1 0 0 cm 2 0 0 1 0 0 cm",
            "1 0 0 1000000 0 0 cm 1 0 0 2 0 0 cm",
            "1000000 0 0 1 0 0 cm 1 0 0 1 2 0 cm",
            "1 0 0 1000000 0 0 cm 1 0 0 1 0 -2 cm",
        ] {
            let bytes = format!("{prefix} BT /F1 12 Tf 0 0 Td (TEXT) Tj ET");
            assert!(
                scan(&with_content(bytes.as_bytes()), 0).is_err(),
                "accepted {prefix}"
            );
        }
        // Each authored value is bounded, but the composed text scale is not.
        let doc = with_content(b"1000 0 0 1000 0 0 cm BT /F1 12 Tf 1001 0 0 1 0 0 Tm (TEXT) Tj ET");
        assert!(scan(&doc, 0).is_err());
        // Repeated positive scales must not underflow to a collapsed matrix.
        let bytes = format!(
            "{}BT /F1 12 Tf 0 0 Td (TEXT) Tj ET",
            "0.000001 0 0 1 0 0 cm ".repeat(60)
        );
        assert!(scan(&with_content(bytes.as_bytes()), 0).is_err());
        let at_limit =
            with_content(b"1000 0 0 1000 0 0 cm BT /F1 12 Tf 1000 0 0 1000 0 0 Tm (TEXT) Tj ET");
        assert_eq!(scan(&at_limit, 0).unwrap().runs[0].matrix[0], 1_000_000.);
    }

    #[test]
    fn textedit_page_transforms_refuse_unbounded_matrices_and_preserve_restored_state() {
        // Restoring the matrix before text prevents its later geometry check
        // from hiding a page-transform admission failure.
        for (matrix, accepted) in [
            ("1 0 0 1 0 0", true),
            ("0 1 -1 0 0 0", true),
            ("0 -1 1 0 0 0", true),
            ("0 0 0 1 0 0", false),
        ] {
            let bytes = format!("q {matrix} cm Q BT /F1 12 Tf 40 180 Td (TEXT) Tj ET");
            assert_eq!(scan(&with_content(bytes.as_bytes()), 0).is_ok(), accepted);
        }
        for prefix in [
            "1 0 0 1 1000000 0 cm 1 0 0 1 1 0 cm",
            "1 0 0 1 0 -1000000 cm 1 0 0 1 0 -1 cm",
            "1 0 0 1 1000000 0 cm",
            "1 0 0 1 (bad) 0 cm",
            "1 0 0 1 0 cm",
            "1 0 0 1 0 0 0 cm",
            "1 0 0 1 20 30 cm 0 0 0 1 0 0 cm",
            "1 0.1 0 1 0 0 cm",
            "1 0 0.1 1 0 0 cm",
            "1 0 0 -1 0 0 cm",
        ] {
            let bytes = format!("{prefix} BT /F1 12 Tf 40 180 Td (TEXT) Tj ET");
            assert!(
                scan(&with_content(bytes.as_bytes()), 0).is_err(),
                "accepted {prefix}"
            );
        }
        // ISO 32000-1 does not list cm inside a text object, but arXiv's stamp
        // puts one before the block's first show, where it moves what follows;
        // after a show it would move text already measured, and is refused.
        let doc = with_content(b"BT /F1 12 Tf 1 0 0 1 20 30 cm 40 180 Td (TEXT) Tj ET");
        assert_eq!(
            scan(&doc, 0).unwrap().runs[0].matrix,
            [1., 0., 0., 1., 60., 210.]
        );
        let doc = with_content(b"BT /F1 12 Tf 40 180 Td (TEXT) Tj 1 0 0 1 20 30 cm (MORE) Tj ET");
        assert!(scan(&doc, 0)
            .unwrap_err()
            .contains("graphics operation cm inside a text block"));
    }

    // Typst sets the font before BT. Font and leading are text state (ISO
    // 32000-1 Table 51) and q/Q restore them, so the run uses the saved font.
    #[test]
    fn textedit_font_and_leading_may_be_set_before_the_text_block() {
        let doc = with_content(b"/F1 12 Tf 14 TL BT 40 180 Td (FIRST) Tj T* (SECOND) Tj ET");
        let runs = scan(&doc, 0).unwrap().runs;
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(),
            ["FIRST", "SECOND"]
        );
        assert_eq!(runs[1].matrix[5], 166.);
        let doc = with_content(b"q /F1 12 Tf Q BT 40 180 Td (FIRST) Tj ET");
        assert_eq!(scan(&doc, 0).unwrap_err(), "text has no explicit font");
    }

    // ISO 32000-1 8.11.3.2: PowerPoint puts each slide's background in a
    // layer. The layer state is never resolved, so text inside one is kept
    // read-only; nothing may open inside it, and its resource must be a layer.
    #[test]
    fn textedit_optional_content_keeps_its_text_read_only() {
        let layered = |content: &[u8], group: Dictionary| {
            let mut doc = with_content(content);
            let page = crate::pagetree::ordered_pages(&doc)[0];
            let group = doc.add_object(group);
            let mut own = resources(&doc, page).unwrap().clone();
            own.set("Properties", dictionary! { "L1" => group });
            doc.get_dictionary_mut(page).unwrap().set("Resources", own);
            doc
        };
        let ocg =
            || dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Background") };
        let text = |doc: &Document| {
            scan(doc, 0)
                .unwrap()
                .runs
                .into_iter()
                .map(|run| run.text)
                .collect::<Vec<_>>()
        };
        let body = "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET";
        let doc = layered(
            format!("/OC /L1 BDC 0 0 10 10 re f EMC {body}").as_bytes(),
            ocg(),
        );
        assert_eq!(text(&doc), ["FIRST"]);
        let doc = layered(
            format!("/OC /L1 BDC BT /F1 12 Tf 40 140 Td (HIDDEN) Tj ET EMC {body}").as_bytes(),
            ocg(),
        );
        assert_eq!(text(&doc), ["FIRST"]);
        let doc = layered(
            b"/OC /L1 BDC 0 0 10 10 re f EMC",
            dictionary! { "Type" => "OCMD" },
        );
        assert!(scan(&doc, 0).is_ok());
        for (content, group) in [
            ("/OC /L1 BDC /Artifact BMC 0 0 10 10 re f EMC EMC", ocg()),
            ("/OC /L2 BDC 0 0 10 10 re f EMC", ocg()),
            (
                "/OC /L1 BDC 0 0 10 10 re f EMC",
                dictionary! { "Type" => "Pattern" },
            ),
        ] {
            let doc = layered(format!("{content} {body}").as_bytes(), group);
            assert!(scan(&doc, 0).is_err(), "{content}");
        }
    }

    #[test]
    fn textedit_accepts_single_flate_array_and_rejects_invalid_encodings() {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(stream)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .compress()
            .unwrap();
        for (filters, accepted) in [
            (vec![Object::Name(b"FlateDecode".to_vec())], true),
            (vec![], false),
            (vec![Object::Name(b"ASCII85Decode".to_vec())], false),
            (vec![Object::Name(b"FlateDecode".to_vec()); 2], false),
        ] {
            doc.get_object_mut(stream)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set("Filter", Object::Array(filters));
            assert_eq!(scan(&doc, 0).is_ok(), accepted);
        }
    }

    #[test]
    fn textedit_maps_operators_and_clones_shared_streams() {
        let mut doc = fixture();
        let Inspection {
            content: before,
            runs,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(
            runs.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [3, 8]
        );
        assert_eq!(runs.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let first = change(&doc);
        let second = Change {
            operator: 8,
            original: "SYNTHETIC SECOND".into(),
            replacement: "EDITED SECOND".into(),
            ..first.clone()
        };
        write(&mut doc, &[first, second]).unwrap();
        let Inspection {
            content: after,
            runs: saved,
            ..
        } = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "EDITED FIRST");
        assert_eq!(saved.runs[1].text, "EDITED SECOND");
        assert_eq!(scan(&doc, 1).unwrap().runs[0].text, "SYNTHETIC FIRST");
        for (a, b) in before.operations.iter().zip(after.operations.iter()) {
            if a.operator != "Tj" {
                assert_eq!(a.operator, b.operator);
                assert_eq!(a.operands, b.operands);
            }
        }
    }

    #[test]
    fn textedit_rejects_invalid_batches_without_mutating_the_document() {
        let doc = fixture();
        let valid = change(&doc);
        let variants = [
            (
                Change {
                    revision: vec![0; 32],
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    original: "WRONG".into(),
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    operator: 4,
                    ..valid.clone()
                },
                "no longer exists",
            ),
            (
                Change {
                    page: 9,
                    ..valid.clone()
                },
                "not in this document",
            ),
            (
                Change {
                    replacement: "Z".repeat(80),
                    ..valid.clone()
                },
                "exceed the original",
            ),
            (
                Change {
                    replacement: "\u{03b1}".into(),
                    ..valid.clone()
                },
                "Latin-1, WinAnsi punctuation and minus only",
            ),
            (
                Change {
                    replacement: valid.original.clone(),
                    ..valid.clone()
                },
                "unchanged",
            ),
        ];
        for (bad, expected) in variants {
            let mut copy = doc.clone();
            // Put a valid edit on the other page first, so a late refusal also
            // proves validation does not partially apply a batch.
            let prior = Change {
                page: 1,
                ..valid.clone()
            };
            assert!(write(&mut copy, &[prior, bad])
                .unwrap_err()
                .contains(expected));
            assert_eq!(copy.objects, doc.objects);
            assert_eq!(copy.max_id, doc.max_id);
        }
        let mut copy = doc.clone();
        assert!(write(&mut copy, &[valid.clone(), valid])
            .unwrap_err()
            .contains("duplicate"));
        assert_eq!(copy.objects, doc.objects);
    }

    #[test]
    fn textedit_refuses_unsupported_state_and_font_semantics() {
        for bytes in [
            "7 Tr BT /F1 12 Tf 40 180 Td (HIDDEN) Tj ET",
            "-2 0 0 2 0 0 cm BT /F1 12 Tf 40 180 Td (REFLECTED) Tj ET",
            "/Span << /ActualText (OTHER) >> BDC BT /F1 12 Tf 40 180 Td (TEXT) Tj ET EMC",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj",
            "BT BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 4 Tc T* (MORE) Tj ET",
            "BT /F1 12 Tf (TEXT) Tj ET",
            "BT /F1 12 Tf 1000000 0 0 1 40 180 Tm 2 0 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 1 T* ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET (OUTSIDE) Tj",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.as_bytes().to_vec()));
            doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
            assert!(scan(&doc, 0).is_err(), "accepted {bytes}");
        }
        for key in ["Widths", "ToUnicode", "FontDescriptor"] {
            let mut doc = fixture();
            let font = doc
                .objects
                .values_mut()
                .find_map(|o| o.as_dict_mut().ok().filter(|d| d.has(b"BaseFont")))
                .unwrap();
            font.set(key, Object::Null);
            assert!(scan(&doc, 0)
                .unwrap_err()
                .contains(if key == "FontDescriptor" {
                    "invalid text resources"
                } else {
                    "custom font"
                }));
        }
    }

    #[test]
    fn textedit_validates_empty_replacement_positioning_and_limits() {
        let mut doc = fixture();
        let mut edit = change(&doc);
        edit.replacement.clear();
        write(&mut doc, &[edit]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
        assert_eq!(scan(&doc, 0).unwrap().runs[1].matrix[5], 140.);
        assert!(
            write(&mut fixture(), &vec![change(&fixture()); MAX_CHANGES + 1])
                .unwrap_err()
                .contains("too many")
        );
        assert!(decode_text(&vec![b'A'; MAX_TEXT + 1]).is_err());
    }

    #[test]
    fn textedit_rejects_partial_or_undecodable_content() {
        for kind in [
            "bad-reference",
            "bad-filter",
            "invalid-flate",
            "trailing-junk",
            "too-large",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let original = doc
                .get_dictionary(id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .clone();
            match kind {
                "bad-reference" => doc
                    .get_dictionary_mut(id)
                    .unwrap()
                    .set("Contents", vec![original, Object::Reference((999999, 0))]),
                _ => {
                    let stream = doc
                        .get_object_mut(original.as_reference().unwrap())
                        .unwrap()
                        .as_stream_mut()
                        .unwrap();
                    match kind {
                        "bad-filter" => stream.dict.set("Filter", 7),
                        "invalid-flate" => stream.dict.set("Filter", "FlateDecode"),
                        "trailing-junk" => stream.content.extend(b"\n(unfinished"),
                        "too-large" => stream.content = vec![b' '; MAX_CONTENT + 1],
                        _ => unreachable!(),
                    }
                }
            }
            assert!(scan(&doc, 0).is_err(), "accepted {kind}");
        }
        let mut compressed = fixture();
        compressed.compress();
        assert_eq!(
            scan(&compressed, 0).unwrap().runs[0].text,
            "SYNTHETIC FIRST"
        );
    }
}
