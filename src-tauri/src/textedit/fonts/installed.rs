//! An installed copy of a document's own font, for the characters its embedded
//! subset lacks. Automatic font mode's second choice, after the subset itself
//! and before the bundled Noto fonts (`layout::prepare`).
//!
//! **The app process finds the file and this module decides whether to use
//! it.** `crate::sysfont` asks the operating system for the font by its exact
//! PostScript name and hands the bytes over in the request; the worker parses
//! them here and trusts none of it:
//!
//! * the face must carry the requested PostScript name in its own name table,
//!   because an OS lookup that misses can answer with a substitute, and a
//!   collection holds several faces;
//! * its OS/2 rights must permit editing *and* subsetting (`permits`), stricter
//!   than the rule for a document's own font, which is never subsetted;
//! * every glyph width the document's subset declares must agree with the
//!   installed advance within [`TOLERANCE`], or the two are not the same font
//!   whatever their names say;
//! * TrueType outlines only: a CFF-outline (`.otf`) copy is refused with its
//!   own reason, and so is a variable or colour font.
//!
//! What is embedded is a subset, never the file: the glyphs of the replacement
//! and of the document's own subset, with the OS/2 table and a character map
//! put back (`fallback_subset::with_tables`). That same subset is what the
//! journal keeps after an Apply, and it passes every check here again on each
//! later request, since a subset of the font is still the font.
use super::{fallback, Metrics};
use crate::textedit::Installed;
use ttf_parser::{name_id, Face, GlyphId, Tag};

/// The largest installed font file the app process reads and the worker
/// accepts. Ordinary system TrueType fonts are 0.2--2 MB; the large pan-Unicode
/// and CJK ones reach 20--25 MB, and a font past this is left to Noto.
pub(crate) const MAX_BYTES: usize = 32 * 1024 * 1024;

/// How far a document's PDF width and the installed advance may differ, in
/// thousandths of an em, and still be the same glyph.
///
/// A producer writes widths as integers, rounded or truncated from the font's
/// units, so a faithful copy differs by up to one unit. Measured on 2026-09-30
/// over 43 installed WinAnsi TrueType subsets in a reader's own documents: 38
/// agreed on every glyph within one unit and none of the other five did, each
/// with some glyph further off -- a different release of the font. The
/// document's own Unicode path uses the same unit for a program's advance
/// against its PDF width (`unicode::Metrics::from_glyphs`).
pub(in crate::textedit) const TOLERANCE: f64 = 1.;

/// Faces a collection may hold. The largest system collections hold a few
/// dozen.
const MAX_FACES: u32 = 256;

/// Why an installed copy was not used; the preview names it beside the Noto
/// font it fell to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::textedit) enum Refusal {
    /// Not a readable font, too large, or no face carrying the name.
    Unreadable,
    /// PostScript (CFF) outlines, which this writer cannot embed yet.
    Cff,
    /// A variable or colour font, whose outlines the editor does not validate.
    Variable,
    /// Rights that forbid editing or subsetting.
    Rights,
    /// A width that disagrees with the document's subset, or a character of
    /// that subset the installed copy lacks.
    Widths,
    /// A character of the replacement the installed copy lacks.
    Missing,
}

impl Refusal {
    pub(in crate::textedit) fn reason(self, name: &str) -> String {
        let why = match self {
            Refusal::Unreadable => "could not be read",
            Refusal::Cff => "has CFF outlines, which cannot be embedded yet",
            Refusal::Variable => "is a variable or colour font",
            Refusal::Rights => "does not permit editing",
            Refusal::Widths => "differs from the document's copy",
            Refusal::Missing => "lacks a character",
        };
        format!("the installed {name} {why}")
    }
}

/// An installed copy accepted for one replacement: the writer set up for it,
/// the preview's label, and the subset the journal keeps.
pub(in crate::textedit) struct Accepted {
    pub font: fallback::Font,
    pub metrics: Metrics,
    pub label: String,
    pub program: Installed,
}

/// Whether `name` can be a PostScript font name: 1 to 63 printable ASCII
/// characters without the PDF and PostScript delimiters. The worker derives
/// the name from the document; the app process checks it again before it
/// asks the operating system for anything.
pub(crate) fn valid_postscript_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && !b"[](){}<>/%".contains(&b))
}

/// The PostScript name of an embedded font, without its subset tag, from its
/// descriptor's `FontName` (a Type0 font's descendant's), or `None` for a font
/// that embeds no program or has no usable name. That is the name the
/// document's producer took from the font it had installed.
pub(in crate::textedit) fn postscript_name(
    doc: &lopdf::Document,
    font: &lopdf::Dictionary,
) -> Option<String> {
    use lopdf::Object;
    fn resolve<'a>(doc: &'a lopdf::Document, value: &'a Object) -> Option<&'a lopdf::Dictionary> {
        crate::encoding::resolve_dict(doc, value).ok()
    }
    let font = match font.get(b"Subtype").and_then(Object::as_name).ok()? {
        b"TrueType" | b"Type1" | b"MMType1" => font,
        b"Type0" => {
            let descendants = crate::encoding::resolve(doc, font.get(b"DescendantFonts").ok()?)
                .as_array()
                .ok()?;
            resolve(doc, descendants.first()?)?
        }
        _ => return None,
    };
    let descriptor = resolve(doc, font.get(b"FontDescriptor").ok()?)?;
    if ![b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
        .iter()
        .any(|key| descriptor.has(key))
    {
        return None;
    }
    let name = descriptor
        .get(b"FontName")
        .or_else(|_| font.get(b"BaseFont"))
        .and_then(Object::as_name)
        .ok()?;
    // ISO 32000-1 9.6.4: a subset's name is six upper-case letters and a plus
    // sign before the font's own.
    let name = if name.len() > 7 && name[..6].iter().all(u8::is_ascii_uppercase) && name[6] == b'+'
    {
        &name[7..]
    } else {
        name
    };
    let name = std::str::from_utf8(name).ok()?;
    valid_postscript_name(name).then(|| name.to_owned())
}

/// Whether an installed font's OS/2 `fsType` lets tpdf embed a subset of it
/// in a document the reader then edits: installable (0) or editable (0x8)
/// embedding, and nothing else. Restricted (0x2) and preview-and-print (0x4)
/// forbid the use, as they do for a document's own font (`restricts`); the
/// no-subsetting (0x100) and bitmap-only (0x200) bits forbid what is done here,
/// where a document's font, never subsetted, may carry the first.
pub(in crate::textedit) fn permits(rights: u16) -> bool {
    rights & !0x8 == 0
}

/// Decide whether `supplied` is an installed copy of the font the document
/// names `wanted`, whose subset declares `declared` widths, and if it is, set
/// it up to write `text`.
pub(in crate::textedit) fn accept(
    supplied: &Installed,
    wanted: &str,
    declared: &[(char, f64)],
    text: &str,
) -> Result<Accepted, Refusal> {
    if supplied.name != wanted || supplied.program.len() > MAX_BYTES {
        return Err(Refusal::Unreadable);
    }
    let bytes = supplied.program.as_slice();
    let index = face_index(bytes, supplied.index, wanted)?;
    // `face_index` has checked that this face carries the name.
    let face = Face::parse(bytes, index).map_err(|_| Refusal::Unreadable)?;
    let tables = face.tables();
    if tables.cff.is_some() || face.raw_face().table(Tag::from_bytes(b"CFF2")).is_some() {
        return Err(Refusal::Cff);
    }
    if tables.glyf.is_none() {
        return Err(Refusal::Unreadable);
    }
    if [b"fvar", b"COLR", b"CBDT", b"sbix", b"SVG "]
        .iter()
        .any(|tag| face.raw_face().table(Tag::from_bytes(tag)).is_some())
    {
        return Err(Refusal::Variable);
    }
    let os2 = face.raw_face().table(Tag::from_bytes(b"OS/2"));
    if let Some(os2) = os2 {
        let rights = os2.get(8..10).ok_or(Refusal::Unreadable)?;
        if !permits(u16::from_be_bytes([rights[0], rights[1]])) {
            return Err(Refusal::Rights);
        }
    }
    let unit = 1000. / f64::from(face.units_per_em());
    let glyph = |ch: char| face.glyph_index(ch).filter(|id| id.0 != 0);
    if declared.is_empty() {
        return Err(Refusal::Widths);
    }
    let mut mapped = std::collections::BTreeMap::new();
    for &(ch, width) in declared {
        let id = glyph(ch).ok_or(Refusal::Widths)?;
        let advance = f64::from(face.glyph_hor_advance(id).ok_or(Refusal::Widths)?) * unit;
        if (advance - width).abs() > TOLERANCE {
            return Err(Refusal::Widths);
        }
        mapped.insert(ch, id);
    }
    for ch in text.chars().filter(|ch| *ch != '\n') {
        mapped.insert(ch, glyph(ch).ok_or(Refusal::Missing)?);
    }
    let program = subset(bytes, index, &mapped, os2).ok_or(Refusal::Unreadable)?;
    let (font, metrics) = fallback::Font::installed(program.clone(), wanted, text)
        .map_err(|_| Refusal::Unreadable)?;
    Ok(Accepted {
        font,
        metrics,
        label: format!("{} (installed)", display_name(&face, wanted)),
        program: Installed {
            name: wanted.to_owned(),
            index: None,
            program,
        },
    })
}

/// The face in `bytes` that carries `wanted`: the one `index` names, or for a
/// collection the app process could not name a face in, the first that
/// carries it.
fn face_index(bytes: &[u8], index: Option<u32>, wanted: &str) -> Result<u32, Refusal> {
    let count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    if count == 0 || count > MAX_FACES {
        return Err(Refusal::Unreadable);
    }
    let named = |at: u32| Face::parse(bytes, at).is_ok_and(|face| carries_name(&face, wanted));
    match index {
        Some(at) if at < count && named(at) => Ok(at),
        Some(_) => Err(Refusal::Unreadable),
        None => (0..count).find(|&at| named(at)).ok_or(Refusal::Unreadable),
    }
}

/// Whether every PostScript name record the face has that decodes reads
/// `wanted`, and at least one does. A Macintosh Roman record is read as the
/// ASCII it must be (OpenType `name` ID 6): Apple's own collections, Helvetica
/// among them, carry no other kind, and ttf-parser decodes Unicode records only.
fn carries_name(face: &Face<'_>, wanted: &str) -> bool {
    let mut found = false;
    for record in face.names() {
        if record.name_id != name_id::POST_SCRIPT_NAME {
            continue;
        }
        let roman = record.platform_id == ttf_parser::PlatformId::Macintosh
            && record.encoding_id == 0
            && record.name.is_ascii();
        let decoded = if roman {
            std::str::from_utf8(record.name).ok().map(str::to_owned)
        } else {
            record.to_string()
        };
        match decoded {
            Some(name) if name == wanted => found = true,
            Some(_) => return false,
            None => {}
        }
    }
    found
}

/// The face's full name for the preview, or the PostScript name when it has
/// none worth showing.
fn display_name(face: &Face<'_>, fallback: &str) -> String {
    face.names()
        .into_iter()
        .filter(|record| record.name_id == name_id::FULL_NAME)
        .find_map(|record| record.to_string())
        .filter(|name| {
            (1..=64).contains(&name.chars().count()) && !name.chars().any(char::is_control)
        })
        .unwrap_or_else(|| fallback.to_owned())
}

/// A subset holding the glyphs `mapped` names, with the source's OS/2 table and
/// a character map from each character to its glyph in the subset.
fn subset(
    bytes: &[u8],
    index: u32,
    mapped: &std::collections::BTreeMap<char, GlyphId>,
    os2: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let mut remapper = subsetter::GlyphRemapper::new();
    for id in mapped.values() {
        remapper.remap(id.0);
    }
    let program = subsetter::subset(bytes, index, &remapper).ok()?;
    let pairs = mapped
        .iter()
        .map(|(ch, id)| Some((u32::from(*ch), remapper.get(id.0)?)))
        .collect::<Option<Vec<_>>>()?;
    let cmap = character_map(&pairs);
    let mut extra = vec![(*b"cmap", cmap.as_slice())];
    if let Some(os2) = os2 {
        extra.push((*b"OS/2", os2));
    }
    let program = super::fallback_subset::with_tables(&program, &extra).ok()?;
    (program.len() <= crate::textedit::MAX_CONTENT).then_some(program)
}

/// A `cmap` with one Windows Unicode full-repertoire subtable (3, 10) in
/// format 12, one group per character. `pairs` is in character order.
fn character_map(pairs: &[(u32, u16)]) -> Vec<u8> {
    let groups = u32::try_from(pairs.len()).unwrap_or(u32::MAX);
    let mut table = Vec::with_capacity(28 + 12 * pairs.len());
    for value in [0_u16, 1, 3, 10] {
        table.extend(value.to_be_bytes());
    }
    table.extend(12_u32.to_be_bytes());
    table.extend(12_u16.to_be_bytes());
    table.extend(0_u16.to_be_bytes());
    table.extend((16 + 12 * groups).to_be_bytes());
    table.extend(0_u32.to_be_bytes());
    table.extend(groups.to_be_bytes());
    for &(character, id) in pairs {
        table.extend(character.to_be_bytes());
        table.extend(character.to_be_bytes());
        table.extend(u32::from(id).to_be_bytes());
    }
    table
}

#[cfg(test)]
mod tests;
