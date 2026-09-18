//! Worker-only validation of embedded TrueType and simple CFF fonts. No font bytes are
//! exported, substituted, repaired or extended. PDF widths control positioning.

use super::{dictionary, filters, number};
use lopdf::{Dictionary, Document, Object};
use ttf_parser::{Face, GlyphId, PlatformId, Tag};

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(super) mod ink_tests;

#[cfg(test)]
mod winansi_tests;

#[cfg(test)]
mod unembedded_tests;

mod cff;
mod composite;
mod ligatures;
mod mapping;
mod outlines;
mod standard;
mod type1;
mod type3;
mod unicode;
pub(super) use composite::embedded as composite;
pub(super) use type3::embedded as type3;

pub(super) fn type1(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let descriptor = dictionary(
        doc,
        font.get(b"FontDescriptor")
            .map_err(|_| "invalid Type1 font descriptor")?,
    )?;
    // PDF Type1 is a font dictionary subtype, not the embedded program format.
    // ISO 32000-1 Table 126 distinguishes FontFile (PostScript) from Type1C in
    // FontFile3 (CFF). Diagnose the declared carrier without decoding its bytes.
    let mut carriers = [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
        .into_iter()
        .filter(|key| descriptor.has(key));
    let Some(carrier) = carriers.next() else {
        return unembedded(doc, font);
    };
    if carriers.next().is_some() {
        return Err("Type1 font has conflicting embedded font programs".into());
    }
    let program = crate::encoding::resolve(
        doc,
        descriptor
            .get(carrier)
            .map_err(|_| "invalid embedded program in Type1 font")?,
    )
    .as_stream()
    .map_err(|_| "invalid embedded program in Type1 font")?;
    match carrier {
        b"FontFile" => return type1::embedded(doc, font),
        b"FontFile2" => return Err("FontFile2 is not supported in Type1 fonts".into()),
        _ => {}
    }
    match program.dict.get(b"Subtype").and_then(Object::as_name).ok() {
        Some(b"Type1C") => cff::embedded(doc, font),
        Some(b"OpenType") => Err("OpenType programs in Type1 fonts are not editable yet".into()),
        _ => Err("unsupported embedded program subtype in Type1 font".into()),
    }
}

enum Codes {
    Single(Box<[Option<u8>; 256]>),
    Double(std::collections::BTreeMap<u16, u8>),
}

pub(super) const GAP_SPACES: &str =
    "This font shows spaces as gaps between words; use single spaces between words";

// Read-only text marks each opaque glyph with this character. The writable
// repertoire never contains it, so it cannot collide with offered text.
pub(super) const OPAQUE: char = '\u{FFFD}';

#[derive(Clone, Copy)]
pub(super) struct Opaque {
    // Advance and excursions beyond it, in thousandths of an em.
    pub width: f64,
    pub overhang: [f64; 2],
}

pub(super) struct Metrics {
    unicode: Option<unicode::Metrics>,
    // Union of all offered glyphs, in thousandths of an em, including baseline.
    // Any width-fitting replacement is therefore covered by the same envelope.
    pub(super) vertical_bounds: Option<[f64; 2]>,
    widths: Box<[Option<f64>; 256]>,
    // Measured excursions beyond each advance, in thousandths of an em.
    // Embedded fonts admit them; standard Helvetica retains zero slack.
    horizontal_overhangs: Option<Box<[[f64; 2]; 256]>>,
    // Single-byte codes whose glyph is validated (advance and ink) but whose
    // text the editor cannot write: math symbols, letters outside Latin-1.
    // They measure source text so that a run using them stays read-only with
    // its ink reserved, instead of refusing the page. Never used to encode.
    opaque: Option<Box<[Option<Opaque>; 256]>>,
    // PDF codes to metric slots. Each font path validates its offered repertoire.
    // None retains the WinAnsi/Latin-1 path.
    codes: Option<Codes>,
}

pub(super) mod fallback;
mod fallback_subset;

impl Metrics {
    fn text_slots(&self, text: &str) -> Result<Vec<u8>, String> {
        let bytes = super::encode_text(text)?;
        if !ligatures::GLYPHS
            .iter()
            .any(|(_, _, slot)| self.widths[*slot as usize].is_some())
        {
            return Ok(bytes);
        }
        let mut result = Vec::with_capacity(bytes.len());
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            if let Some((_, sequence, slot)) = ligatures::GLYPHS.iter().find(|(_, text, slot)| {
                self.widths[*slot as usize].is_some() && rest.starts_with(text.as_bytes())
            }) {
                result.push(*slot);
                rest = &rest[sequence.len()..];
            } else {
                result.push(rest[0]);
                rest = &rest[1..];
            }
        }
        Ok(result)
    }

    pub(super) fn helvetica() -> Self {
        let mut widths = Box::new([None; 256]);
        for byte in (32..=126).chain(160..=255) {
            widths[byte as usize] = Some(crate::textbox::advance(
                &char::from(byte).to_string(),
                1000.,
            ));
        }
        Self {
            opaque: None,
            unicode: None,
            vertical_bounds: None,
            widths,
            horizontal_overhangs: None,
            codes: None,
        }
    }

    // Unembedded nonsymbolic Helvetica without Encoding uses Adobe's default,
    // not WinAnsi: ISO 32000-1, 9.6.6 and Annex D.1. A name literally called
    // StandardEncoding is not a predefined PDF encoding and stays refused.
    // Only characters in our existing printable Latin-1 domain are offered.
    #[cfg(test)]
    pub(super) fn helvetica_default() -> Self {
        Self::helvetica().standard_encoding()
    }

    /// One of the twelve Latin standard fonts (ISO 32000-1 9.6.2.2), measured
    /// by Adobe's metrics over the same printable WinAnsi codes as Helvetica.
    /// Like Helvetica it has no outlines here, so its text cannot be kept
    /// read-only; every reader is required to have the font.
    pub(super) fn standard(base: &[u8]) -> Option<Self> {
        let (_, table) = standard::FONTS.iter().find(|(name, _)| *name == base)?;
        let mut metrics = Self::helvetica();
        for (slot, width) in (32..=126).chain(160..=255).zip(table) {
            metrics.widths[slot] = Some(f64::from(*width));
        }
        Some(metrics)
    }

    // The same default encoding for every Latin standard font: its built-in
    // encoding is StandardEncoding.
    pub(super) fn standard_encoding(self) -> Self {
        let mut codes = Box::new([None; 256]);
        for code in 32..=126 {
            // These codes mean curly quotes, outside the supported domain.
            if !matches!(code, 39 | 96) {
                codes[code as usize] = Some(code);
            }
        }
        for (code, ch) in [
            (161, 161),
            (162, 162),
            (163, 163),
            (165, 165),
            (167, 167),
            (168, 164),
            (169, 39),
            (171, 171),
            (180, 183),
            (182, 182),
            (187, 187),
            (191, 191),
            (193, 96),
            (194, 180),
            (197, 175),
            (200, 168),
            (203, 184),
            (225, 198),
            (227, 170),
            (233, 216),
            (235, 186),
            (241, 230),
            (249, 248),
            (251, 223),
        ] {
            codes[code] = Some(ch);
        }
        let mut metrics = self;
        for (ch, width) in metrics.widths.iter_mut().enumerate() {
            if !codes.contains(&Some(ch as u8)) {
                *width = None;
            }
        }
        metrics.codes = Some(Codes::Single(codes));
        metrics
    }

    pub(super) fn decode(&self, bytes: &[u8]) -> Result<String, String> {
        if let Some(metrics) = &self.unicode {
            return metrics.source(bytes, 1., 0., 0.).map(|(text, _, _)| text);
        }
        let Some(codes) = &self.codes else {
            return super::decode_text(bytes);
        };
        if let Codes::Double(codes) = codes {
            if bytes.len() % 2 != 0 || bytes.len() / 2 > super::MAX_TEXT {
                return Err("invalid or oversized two-byte text".into());
            }
            let mut text = String::new();
            let mut characters = 0;
            for pair in bytes.chunks_exact(2) {
                let slot = *codes
                    .get(&u16::from_be_bytes([pair[0], pair[1]]))
                    .ok_or("text contains an unmapped font code")?;
                if let Some(sequence) = ligatures::text(slot) {
                    text.push_str(sequence);
                    characters += sequence.len();
                } else {
                    text.push(super::slot_character(slot));
                    characters += 1;
                }
                if characters > super::MAX_TEXT {
                    return Err("expanded two-byte text exceeds its limit".into());
                }
            }
            return Ok(text);
        }
        let Codes::Single(codes) = codes else {
            unreachable!()
        };
        if bytes.len() > super::MAX_TEXT {
            return Err("mapped text exceeds its limit".into());
        }
        let mut text = String::new();
        let mut characters = 0;
        for &code in bytes {
            let Some(slot) = codes[code as usize] else {
                if self.opaque(code).is_none() {
                    return Err("text contains an unmapped font code".into());
                }
                text.push(OPAQUE);
                characters += 1;
                continue;
            };
            if let Some(sequence) = ligatures::text(slot) {
                text.push_str(sequence);
                characters += sequence.len();
            } else {
                text.push(super::slot_character(slot));
                characters += 1;
            }
            if characters > super::MAX_TEXT {
                return Err("expanded text exceeds its limit".into());
            }
        }
        Ok(text)
    }

    /// Bytes per character code in a shown string.
    pub(super) fn code_len(&self) -> usize {
        match (&self.unicode, &self.codes) {
            (Some(metrics), _) => metrics.code_len(),
            (None, Some(Codes::Double(_))) => 2,
            _ => 1,
        }
    }

    /// Whether a space can be written as a glyph. TeX fonts and some subsets
    /// have none; their producers show word gaps as TJ displacements instead.
    pub(super) fn writes_space(&self) -> bool {
        self.encode(" ").is_ok()
    }

    /// The TJ items that show `text`: one string, or, in a font that cannot
    /// write a space, its words separated by displacements of `gap` thousandths
    /// of an em. A leading, trailing or repeated space has no word to separate
    /// and is refused rather than written as an invisible shift.
    pub(super) fn items(&self, text: &str, gap: f64) -> Result<Vec<lopdf::Object>, String> {
        if !text.contains(' ') || self.writes_space() {
            return Ok(vec![lopdf::Object::string_literal(self.encode(text)?)]);
        }
        let mut items = Vec::new();
        for (index, word) in text.split(' ').enumerate() {
            if word.is_empty() {
                return Err(GAP_SPACES.into());
            }
            if index > 0 {
                items.push(lopdf::Object::Real(-gap as f32));
            }
            items.push(lopdf::Object::string_literal(self.encode(word)?));
        }
        Ok(items)
    }

    /// Advance and ink of `text` as `items` shows it, in text space units.
    pub(super) fn gapped_layout(
        &self,
        text: &str,
        size: f64,
        spacing: f64,
        word_spacing: f64,
        gap: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        if !text.contains(' ') || self.writes_space() {
            return self.spaced_layout(text, size, spacing, word_spacing);
        }
        let shift = gap * size / 1000.;
        let mut cursor = 0.;
        let mut bounds = [0_f64; 2];
        for (index, word) in text.split(' ').enumerate() {
            if word.is_empty() {
                return Err(GAP_SPACES.into());
            }
            if index > 0 {
                cursor += shift;
            }
            let (advance, [left, right]) = self.spaced_layout(word, size, spacing, word_spacing)?;
            if index == 0 {
                bounds = [left, right];
            } else {
                bounds = [bounds[0].min(cursor + left), bounds[1].max(cursor + right)];
            }
            cursor += advance;
        }
        Ok((cursor, bounds))
    }

    pub(super) fn encode(&self, text: &str) -> Result<Vec<u8>, String> {
        if let Some(metrics) = &self.unicode {
            return metrics.encode(text);
        }
        let bytes = self.text_slots(text)?;
        let Some(codes) = &self.codes else {
            // The unmapped font paths retain their existing Latin-1 repertoire.
            // A mapped-only metric slot must never escape as a literal PDF code.
            super::decode_text(&bytes)?;
            return Ok(bytes);
        };
        if let Codes::Double(codes) = codes {
            let mut result = Vec::with_capacity(bytes.len() * 2);
            for byte in bytes {
                let (&code, _) =
                    codes
                        .iter()
                        .find(|(_, value)| **value == byte)
                        .ok_or_else(|| {
                            "the font has no validated glyph for this character".to_string()
                        })?;
                result.extend(code.to_be_bytes());
            }
            return Ok(result);
        }
        let Codes::Single(codes) = codes else {
            unreachable!()
        };
        bytes
            .iter()
            .map(|byte| {
                codes
                    .iter()
                    .position(|value| value.as_ref() == Some(byte))
                    .map(|code| code as u8)
                    .ok_or_else(|| "the font has no validated glyph for this character".to_string())
            })
            .collect()
    }

    fn width(&self, byte: u8) -> Result<f64, String> {
        Ok(self.widths[byte as usize]
            .ok_or("the font has no validated glyph for this character")?)
    }

    pub(super) fn advance(&self, text: &str, size: f64) -> Result<f64, String> {
        if let Some(metrics) = &self.unicode {
            return metrics
                .replacement(text, size, 0., 0.)
                .map(|(advance, _)| advance);
        }
        let mut width = 0.;
        for byte in self.text_slots(text)? {
            width += self.width(byte)?;
        }
        Ok(width * size / 1000.)
    }

    pub(super) fn horizontal_bounds(&self, text: &str, size: f64) -> Result<[f64; 2], String> {
        if let Some(metrics) = &self.unicode {
            return metrics
                .replacement(text, size, 0., 0.)
                .map(|(_, bounds)| bounds);
        }
        let mut bounds = [0_f64; 2];
        let mut cursor = 0.;
        for byte in self.text_slots(text)? {
            let width = self.width(byte)?;
            let [left, right] = self
                .horizontal_overhangs
                .as_ref()
                .map_or([0.; 2], |values| values[byte as usize]);
            bounds[0] = bounds[0].min(cursor + left);
            cursor += width;
            bounds[1] = bounds[1].max(cursor + right);
        }
        Ok(bounds.map(|value| value * size / 1000.))
    }

    // Tc is in unscaled text space and applies after every character, including
    // the last. It changes the advance, but trailing spacing is not glyph ink.
    // Work on decoded characters: an Identity-H code consumes two PDF bytes.
    pub(super) fn spaced_layout(
        &self,
        text: &str,
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        if let Some(metrics) = &self.unicode {
            return metrics.replacement(text, size, spacing, word_spacing);
        }
        if spacing == 0. && word_spacing == 0. {
            return Ok((
                self.advance(text, size)?,
                self.horizontal_bounds(text, size)?,
            ));
        }
        self.spaced_slots(&self.text_slots(text)?, size, spacing, word_spacing)
    }

    // A ligature and its separate letters can coexist in one font and extract
    // identically. Measuring re-encoded Unicode would change the source width.
    pub(super) fn source_layout(
        &self,
        bytes: &[u8],
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(String, f64, [f64; 2]), String> {
        if let Some(metrics) = &self.unicode {
            return metrics.source(bytes, size, spacing, word_spacing);
        }
        let text = self.decode(bytes)?;
        let (advance, bounds) = if let Some(Codes::Single(codes)) = &self.codes {
            let word_slot = codes[32];
            let glyphs = bytes
                .iter()
                .map(|&code| match codes[code as usize] {
                    Some(slot) => self.glyph(slot, word_slot == Some(slot)),
                    None => self
                        .opaque(code)
                        .map(|glyph| (glyph.width, glyph.overhang, code == 32))
                        .ok_or_else(|| "text contains an unmapped font code".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            self.spaced_glyphs(&glyphs, size, spacing, word_spacing)?
        } else if let Some(Codes::Double(codes)) = &self.codes {
            // Keep original glyph boundaries even when their Unicode text could
            // be re-encoded as a different combination of letters and ligatures.
            let slots = bytes
                .chunks_exact(2)
                .map(|pair| codes[&u16::from_be_bytes([pair[0], pair[1]])])
                .collect::<Vec<_>>();
            self.spaced_slots(&slots, size, spacing, word_spacing)?
        } else {
            self.spaced_layout(&text, size, spacing, word_spacing)?
        };
        Ok((text, advance, bounds))
    }

    fn opaque(&self, code: u8) -> Option<Opaque> {
        self.opaque
            .as_ref()
            .and_then(|opaque| opaque[code as usize])
    }

    // One offered glyph: advance and excursions in thousandths of an em, and
    // whether word spacing applies to it.
    fn glyph(&self, slot: u8, word: bool) -> Result<(f64, [f64; 2], bool), String> {
        let overhang = self
            .horizontal_overhangs
            .as_ref()
            .map_or([0.; 2], |values| values[slot as usize]);
        Ok((self.width(slot)?, overhang, word))
    }

    fn spaced_slots(
        &self,
        slots: &[u8],
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        // ISO 32000-1, 9.3.3: Tw applies to single-byte PDF code 32,
        // regardless of its Unicode mapping. Identity-H has no such code.
        let word_slot = match &self.codes {
            None => Some(32),
            Some(Codes::Single(codes)) => codes[32],
            Some(Codes::Double(_)) => None,
        };
        let glyphs = slots
            .iter()
            .map(|&slot| self.glyph(slot, word_slot == Some(slot)))
            .collect::<Result<Vec<_>, _>>()?;
        self.spaced_glyphs(&glyphs, size, spacing, word_spacing)
    }

    fn spaced_glyphs(
        &self,
        glyphs: &[(f64, [f64; 2], bool)],
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        if !spacing.is_finite() || spacing.abs() > size * 0.25 {
            return Err("character spacing exceeds a quarter of the font size".into());
        }
        // Positive Tw can encode a tab-sized gap with Tf=1 and a scaled Tm.
        // Bound its value and the resulting cursor separately. Keep the
        // existing negative-spacing subset and positive per-glyph steps.
        if !word_spacing.is_finite() || word_spacing < -size * 0.25 || word_spacing > 1_000_000. {
            return Err("word spacing exceeds editable limits".into());
        }
        let mut cursor = 0.;
        let mut bounds = [0_f64; 2];
        for &(width, [left, right], word) in glyphs {
            let width = width * size / 1000.;
            let step = width + spacing + if word { word_spacing } else { 0. };
            if step <= 0. {
                return Err("backtracking character spacing is not editable yet".into());
            }
            bounds[0] = bounds[0].min(cursor + left * size / 1000.);
            bounds[1] = bounds[1].max(cursor + width + right * size / 1000.);
            cursor += step;
            if !cursor.is_finite() || cursor > 1_000_000. {
                return Err("spaced text advance exceeds its limit".into());
            }
        }
        Ok((cursor, bounds))
    }
}

// outline_glyph returns None for both empty and malformed glyphs. Only equal,
// in-bounds loca offsets prove the no-data case; an outline failure does not.
// A simple glyph of one contour with one point paints nothing either, and is
// how YuGothic subsets carry their space: read from the glyf header itself
// (numberOfContours, then after the bounding box endPtsOfContours[0]).
fn empty_glyph(face: &Face<'_>, glyph: GlyphId) -> Option<bool> {
    let raw = face.raw_face();
    let loca = ttf_parser::loca::Table::parse(
        face.tables().maxp.number_of_glyphs,
        face.tables().head.index_to_location_format,
        raw.table(Tag::from_bytes(b"loca"))?,
    )?;
    let next = glyph.0.checked_add(1)?;
    let (start, end) = match loca {
        ttf_parser::loca::Table::Short(offsets) => (
            u32::from(offsets.get(glyph.0)?) * 2,
            u32::from(offsets.get(next)?) * 2,
        ),
        ttf_parser::loca::Table::Long(offsets) => (offsets.get(glyph.0)?, offsets.get(next)?),
    };
    let glyf = raw.table(Tag::from_bytes(b"glyf"))?;
    let data = glyf.get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)?;
    Some(data.is_empty() || (data.get(0..2)? == [0, 1] && data.get(10..12)? == [0, 0]))
}

/// A simple TrueType or Type 1 font the document names but does not embed,
/// as Word does for Arial and Times New Roman. Every reader positions its
/// glyphs by the PDF `Widths` and draws them with a substitute, so the widths
/// are the metrics, exactly as the built-in ones are for standard Helvetica.
/// Only nonsymbolic WinAnsi fonts are read, over the printable Latin-1 range;
/// a code with no width is not offered. There are no outlines, so like
/// Helvetica its text cannot be kept read-only beside an edit.
pub(super) fn unembedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let invalid = || "unsupported unembedded font".to_string();
    if font.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font")
        || !matches!(
            font.get(b"Subtype").and_then(Object::as_name).ok(),
            Some(b"TrueType" | b"Type1")
        )
        || font.get(b"BaseFont").and_then(Object::as_name).is_err()
        || font.get(b"Encoding").and_then(Object::as_name).ok() != Some(b"WinAnsiEncoding")
        || font.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type"
                    | b"Subtype"
                    | b"BaseFont"
                    | b"Encoding"
                    | b"Name"
                    | b"FirstChar"
                    | b"LastChar"
                    | b"Widths"
                    | b"FontDescriptor"
            )
        })
    {
        return Err(invalid());
    }
    let descriptor = dictionary(doc, font.get(b"FontDescriptor").map_err(|_| invalid())?)?;
    if descriptor.get(b"Type").and_then(Object::as_name).ok() != Some(b"FontDescriptor")
        || descriptor.get(b"FontName").ok() != font.get(b"BaseFont").ok()
        || descriptor
            .get(b"Flags")
            .and_then(Object::as_i64)
            .map_err(|_| invalid())?
            & (4 | 32)
            != 32
    {
        // Both callers reach this only for a descriptor without a program.
        return Err(invalid());
    }
    let first = font
        .get(b"FirstChar")
        .and_then(Object::as_i64)
        .map_err(|_| invalid())?;
    let last = font
        .get(b"LastChar")
        .and_then(Object::as_i64)
        .map_err(|_| invalid())?;
    if first < 0 || last > 255 || first > last {
        return Err(invalid());
    }
    let widths = crate::encoding::resolve(doc, font.get(b"Widths").map_err(|_| invalid())?)
        .as_array()
        .map_err(|_| invalid())?;
    if widths.len() != (last - first + 1) as usize {
        return Err(invalid());
    }
    // FontBBox bounds every glyph the font has, so it stands in for the
    // outlines this font does not carry: text in it can then be kept read-only
    // with that box reserved, and nothing drawn is ever outside it.
    let bbox = crate::encoding::resolve(doc, descriptor.get(b"FontBBox").map_err(|_| invalid())?)
        .as_array()
        .map_err(|_| invalid())?
        .iter()
        .map(number)
        .collect::<Result<Vec<_>, _>>()?;
    let [left, bottom, right, top] = bbox[..] else {
        return Err(invalid());
    };
    if left > right || bottom > top || [left, bottom, right, top].iter().any(|v| v.abs() > 4000.) {
        return Err(invalid());
    }
    let mut result = Box::new([None; 256]);
    let mut overhangs = Box::new([[0_f64; 2]; 256]);
    for code in (32..=126).chain(160..=255) {
        if code < first || code > last {
            continue;
        }
        let width = number(&widths[(code - first) as usize])?;
        if !(0. ..=2000.).contains(&width) {
            return Err(invalid());
        }
        if width > 0. {
            result[code as usize] = Some(width);
            overhangs[code as usize] = [left.min(0.), (right - width).max(0.)];
        }
    }
    Ok(Metrics {
        opaque: None,
        unicode: None,
        vertical_bounds: Some([bottom.min(0.), top.max(0.)]),
        widths: result,
        horizontal_overhangs: Some(overhangs),
        codes: None,
    })
}

/// Whether a simple font's descriptor names no embedded program.
pub(super) fn is_unembedded(doc: &Document, font: &Dictionary) -> bool {
    font.get(b"FontDescriptor")
        .ok()
        .and_then(|value| dictionary(doc, value).ok())
        .is_some_and(|descriptor| {
            ![b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                .iter()
                .any(|key| descriptor.has(key))
        })
}

pub(super) fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let invalid = || "unsupported embedded TrueType font or character mapping".to_string();
    let custom = !font.has(b"Encoding") && font.has(b"ToUnicode");
    let mac_roman =
        font.get(b"Encoding").and_then(Object::as_name).ok() == Some(b"MacRomanEncoding");
    let named_unicode = !custom && !mac_roman && font.has(b"ToUnicode");
    if font.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font")
        || (!mac_roman
            && !custom
            && font.get(b"Encoding").and_then(Object::as_name).ok() != Some(b"WinAnsiEncoding"))
        || font.get(b"BaseFont").and_then(Object::as_name).is_err()
        || font.iter().any(|(key, _)| {
            !((custom || named_unicode) && key == b"ToUnicode")
                && !matches!(
                    key.as_slice(),
                    b"Type"
                        | b"Subtype"
                        | b"BaseFont"
                        | b"Encoding"
                        | b"Name"
                        | b"FirstChar"
                        | b"LastChar"
                        | b"Widths"
                        | b"FontDescriptor"
                )
        })
    {
        return Err(invalid());
    }
    let descriptor = dictionary(doc, font.get(b"FontDescriptor").map_err(|_| invalid())?)?;
    let flags = descriptor
        .get(b"Flags")
        .and_then(Object::as_i64)
        .map_err(|_| invalid())?;
    // ISO 32000-1, 9.6.6.4: standard mappings require nonsymbolic flags.
    // The custom path requires symbolic byte lookup through one Macintosh cmap.
    // WinAnsi may retain a Mac cmap only when every offered ASCII glyph agrees
    // with the Windows cmap, which Word and Office write together. No name
    // fallback is used.
    if flags & (4 | 32 | 262144) != if custom { 4 } else { 32 }
        || descriptor.get(b"Type").and_then(Object::as_name).ok() != Some(b"FontDescriptor")
        || descriptor.get(b"FontName").ok() != font.get(b"BaseFont").ok()
        || descriptor.has(b"FontFile")
        || descriptor.has(b"FontFile3")
    {
        return Err(invalid());
    }
    let bytes = program(doc, descriptor)?;
    let face = face(&bytes, mac_roman || custom)?;
    let cmap = face.tables().cmap.ok_or_else(invalid)?;
    if cmap.subtables.len() > 8
        || cmap.subtables.into_iter().count() != usize::from(cmap.subtables.len())
    {
        return Err(invalid());
    }
    let primary = cmap
        .subtables
        .into_iter()
        .find(|table| {
            if mac_roman || custom {
                table.platform_id == PlatformId::Macintosh && table.encoding_id == 0
            } else {
                table.platform_id == PlatformId::Windows && table.encoding_id == 1
            }
        })
        .ok_or_else(invalid)?;
    // Refuse other legacy mappings rather than guessing which renderer selects
    // them. All accepted maps must agree on each offered ASCII glyph.
    if cmap.subtables.into_iter().any(|table| {
        !table.is_unicode()
            && !(table.platform_id == PlatformId::Macintosh && table.encoding_id == 0)
    }) {
        return Err(invalid());
    }
    // PDF 1.6 section 5.5.5: a symbolic font without a (3,0) map uses
    // the string bytes directly in (1,0). ToUnicode supplies text semantics,
    // not glyph selection. Never infer Unicode from a symbolic glyph number.
    if custom && cmap.subtables.len() != 1 {
        return Err(invalid());
    }
    let codes = if custom || named_unicode {
        let stream = crate::encoding::resolve(doc, font.get(b"ToUnicode").map_err(|_| invalid())?)
            .as_stream()
            .map_err(|_| invalid())?;
        Some(if custom {
            mapping::parse(stream)?
        } else {
            mapping::parse_named(stream)?
        })
    } else {
        None
    };
    let first = font
        .get(b"FirstChar")
        .and_then(Object::as_i64)
        .map_err(|_| invalid())?;
    let last = font
        .get(b"LastChar")
        .and_then(Object::as_i64)
        .map_err(|_| invalid())?;
    if first < 0 || last > 255 || first > last {
        return Err(invalid());
    }
    let widths = crate::encoding::resolve(doc, font.get(b"Widths").map_err(|_| invalid())?)
        .as_array()
        .map_err(|_| invalid())?;
    if widths.len() != (last - first + 1) as usize {
        return Err(invalid());
    }
    let unit = 1000. / f64::from(face.units_per_em());
    let mut result = Box::new([None; 256]);
    let mut vertical_bounds = [0_f64; 2];
    let mut horizontal_overhangs = Box::new([[0_f64; 2]; 256]);
    // Standard maps share ASCII codes; symbolic maps select the PDF code and
    // its Unicode value separately. WinAnsi's nonbreaking-space/soft-hyphen aliases,
    // extended glyph names and custom ToUnicode maps require separate proof.
    for code_byte in 0_u8..=255 {
        let byte = if let Some(codes) = &codes {
            let Some(byte) = codes[code_byte as usize] else {
                continue;
            };
            byte
        } else if (32..=126).contains(&code_byte) {
            code_byte
        } else {
            continue;
        };
        // ISO 32000-1 9.6.6.4: a nonsymbolic WinAnsi font selects glyphs in the
        // (3,1) cmap by the Unicode value of the code's WinAnsi glyph name. For
        // ASCII and Latin-1 that value is the code itself.
        let code = if named_unicode {
            u32::from(super::slot_character(byte))
        } else {
            u32::from(code_byte)
        };
        let Some(glyph) = primary.glyph_index(code) else {
            continue;
        };
        // Format 6 returns glyph zero for holes; it is .notdef, not a usable
        // character (unlike the None returned by other cmap formats).
        if glyph.0 == 0 {
            continue;
        }
        // A Macintosh (1,0) map numbers non-ASCII characters differently, so
        // agreement beyond ASCII is required only of the Unicode maps.
        if glyph.0 >= face.number_of_glyphs()
            || cmap
                .subtables
                .into_iter()
                .filter(|table| code < 128 || table.is_unicode())
                .any(|table| table.glyph_index(code) != Some(glyph))
        {
            return Err(invalid());
        }
        if i64::from(code_byte) < first || i64::from(code_byte) > last {
            continue;
        }
        let width = number(&widths[(i64::from(code_byte) - first) as usize])?;
        // Subset exporters leave zero-width holes even when the original cmap
        // and glyph remain embedded. Such a code is unavailable for editing;
        // it must not prevent using the other, independently validated codes.
        if width == 0. {
            continue;
        }
        let advance = f64::from(face.glyph_hor_advance(glyph).ok_or_else(invalid)?) * unit;
        if width <= 0. || width > 2000. || (width - advance).abs() > 1. {
            return Err("embedded font widths disagree with its glyph metrics".into());
        }
        // Match the other embedded-font paths: bounded excursions are allowed,
        // then each source/replacement is checked against its actual ink bounds.
        match outlines::bounds(&face, glyph) {
            Some([left, bottom, right, top])
                if left * unit >= -250.
                    && right * unit <= width + 250.
                    && bottom * unit >= -500.
                    && top * unit <= 1000. =>
            {
                vertical_bounds[0] = vertical_bounds[0].min(bottom * unit);
                vertical_bounds[1] = vertical_bounds[1].max(top * unit);
                horizontal_overhangs[byte as usize] =
                    [(left * unit).min(0.), (right * unit - width).max(0.)];
            }
            None if byte == b' ' && empty_glyph(&face, glyph) == Some(true) => {}
            _ => continue,
        }
        result[byte as usize] = Some(width);
    }
    Ok(Metrics {
        opaque: None,
        unicode: None,
        vertical_bounds: Some(vertical_bounds),
        widths: result,
        horizontal_overhangs: Some(horizontal_overhangs),
        codes: codes.map(Codes::Single),
    })
}

fn program(doc: &Document, descriptor: &Dictionary) -> Result<Vec<u8>, String> {
    let invalid = || "unsupported embedded TrueType program".to_string();
    if descriptor.has(b"FontFile") || descriptor.has(b"FontFile3") {
        return Err(invalid());
    }
    let stream =
        crate::encoding::resolve(doc, descriptor.get(b"FontFile2").map_err(|_| invalid())?)
            .as_stream()
            .map_err(|_| invalid())?;
    // Same strict decoder as page content: 2 MiB encoded, 1 MiB decoded.
    let bytes = filters::decode(stream, super::MAX_CONTENT)?;
    if stream.dict.has(b"Length1")
        && stream.dict.get(b"Length1").and_then(Object::as_i64).ok() != Some(bytes.len() as i64)
    {
        return Err(invalid());
    }
    Ok(bytes)
}

fn face(bytes: &[u8], allow_apple: bool) -> Result<Face<'_>, String> {
    let invalid = || "unsupported embedded TrueType program".to_string();
    let apple_true = allow_apple && bytes.get(..4) == Some(b"true");
    if !apple_true && bytes.get(..4) != Some(&[0, 1, 0, 0]) {
        return Err(invalid()); // No collections, CFF or alternate sfnt flavours.
    }
    let face = Face::parse(bytes, 0).map_err(|_| invalid())?;
    if face.tables().glyf.is_none()
        || [b"fvar", b"COLR", b"CBDT", b"sbix", b"SVG "]
            .iter()
            .any(|tag| face.raw_face().table(Tag::from_bytes(tag)).is_some())
    {
        return Err(invalid());
    }
    // Document fonts are preserved; only the bundled OFL fonts are subsetted.
    // The no-subsetting bit is therefore compatible with document-font reuse.
    // Apple's TrueType format makes OS/2 optional. Preserve an already embedded
    // legacy program without manufacturing a permissions table. If present, its
    // restrictions still apply; OpenType-style programs still require the table.
    // https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html
    if let Some(os2) = face.raw_face().table(Tag::from_bytes(b"OS/2")) {
        let rights = os2.get(8..10).ok_or_else(invalid)?;
        let rights = u16::from_be_bytes([rights[0], rights[1]]);
        if rights & !0x108 != 0 {
            return Err("embedded font does not permit this editable use".into());
        }
    } else if !apple_true {
        return Err(invalid());
    }
    Ok(face)
}
