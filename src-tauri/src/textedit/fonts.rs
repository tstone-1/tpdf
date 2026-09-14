//! Worker-only validation of embedded TrueType and simple CFF fonts. No font bytes are
//! exported, substituted, repaired or extended. PDF widths control positioning.

use super::{dictionary, filters, number};
use lopdf::{Dictionary, Document, Object};
use ttf_parser::{Face, GlyphId, PlatformId, Tag};

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(super) mod ink_tests;

mod cff;
mod composite;
pub(super) use cff::embedded as cff;
mod mapping;
mod outlines;
pub(super) use composite::embedded as composite;

enum Codes {
    Single(Box<[Option<u8>; 256]>),
    Double(std::collections::BTreeMap<u16, u8>),
}

pub(super) struct Metrics {
    // Union of all offered glyphs, in thousandths of an em, including baseline.
    // Any width-fitting replacement is therefore covered by the same envelope.
    pub(super) vertical_bounds: Option<[f64; 2]>,
    widths: Box<[Option<f64>; 256]>,
    // Measured excursions beyond each advance, in thousandths of an em.
    // Embedded fonts admit them; standard Helvetica retains zero slack.
    horizontal_overhangs: Option<Box<[[f64; 2]; 256]>>,
    // PDF codes to metric slots. Embedded single-byte maps admit ASCII and en dash.
    // None retains the WinAnsi/Latin-1 path.
    codes: Option<Codes>,
}

impl Metrics {
    pub(super) fn helvetica() -> Self {
        let mut widths = Box::new([None; 256]);
        for byte in (32..=126).chain(160..=255) {
            widths[byte as usize] = Some(crate::textbox::advance(
                &char::from(byte).to_string(),
                1000.,
            ));
        }
        Self {
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
    pub(super) fn helvetica_default() -> Self {
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
        let mut metrics = Self::helvetica();
        for (ch, width) in metrics.widths.iter_mut().enumerate() {
            if !codes.contains(&Some(ch as u8)) {
                *width = None;
            }
        }
        metrics.codes = Some(Codes::Single(codes));
        metrics
    }

    pub(super) fn decode(&self, bytes: &[u8]) -> Result<String, String> {
        let Some(codes) = &self.codes else {
            return super::decode_text(bytes);
        };
        if let Codes::Double(codes) = codes {
            if bytes.len() % 2 != 0 || bytes.len() / 2 > super::MAX_TEXT {
                return Err("invalid or oversized two-byte text".into());
            }
            return bytes
                .chunks_exact(2)
                .map(|pair| {
                    codes
                        .get(&u16::from_be_bytes([pair[0], pair[1]]))
                        .copied()
                        .map(super::slot_character)
                        .ok_or_else(|| "text contains an unmapped font code".to_string())
                })
                .collect();
        }
        let Codes::Single(codes) = codes else {
            unreachable!()
        };
        if bytes.len() > super::MAX_TEXT {
            return Err("mapped text exceeds its limit".into());
        }
        bytes
            .iter()
            .map(|&code| {
                codes[code as usize]
                    .map(super::slot_character)
                    .ok_or_else(|| "text contains an unmapped font code".to_string())
            })
            .collect()
    }

    pub(super) fn encode(&self, text: &str) -> Result<Vec<u8>, String> {
        let bytes = super::encode_text(text)?;
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
        let mut width = 0.;
        for byte in super::encode_text(text)? {
            width += self.width(byte)?;
        }
        Ok(width * size / 1000.)
    }

    pub(super) fn horizontal_bounds(&self, text: &str, size: f64) -> Result<[f64; 2], String> {
        let mut bounds = [0_f64; 2];
        let mut cursor = 0.;
        for byte in super::encode_text(text)? {
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
        if spacing == 0. && word_spacing == 0. {
            return Ok((
                self.advance(text, size)?,
                self.horizontal_bounds(text, size)?,
            ));
        }
        if !spacing.is_finite() || spacing.abs() > size * 0.25 {
            return Err("character spacing exceeds a quarter of the font size".into());
        }
        if !word_spacing.is_finite() || word_spacing.abs() > size * 0.25 {
            return Err("word spacing exceeds a quarter of the font size".into());
        }
        // ISO 32000-1, 9.3.3: Tw applies to single-byte PDF code 32,
        // regardless of its Unicode mapping. Identity-H has no such code.
        let word_slot = match &self.codes {
            None => Some(32),
            Some(Codes::Single(codes)) => codes[32],
            Some(Codes::Double(_)) => None,
        };
        let mut cursor = 0.;
        let mut bounds = [0_f64; 2];
        for byte in super::encode_text(text)? {
            let width = self.width(byte)? * size / 1000.;
            let [left, right] = self
                .horizontal_overhangs
                .as_ref()
                .map_or([0.; 2], |values| values[byte as usize]);
            let step = width
                + spacing
                + if word_slot == Some(byte) {
                    word_spacing
                } else {
                    0.
                };
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
    Some(start == end && usize::try_from(end).ok()? <= raw.table(Tag::from_bytes(b"glyf"))?.len())
}

pub(super) fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let invalid = || "unsupported embedded TrueType font or character mapping".to_string();
    let custom = !font.has(b"Encoding") && font.has(b"ToUnicode");
    let mac_roman =
        font.get(b"Encoding").and_then(Object::as_name).ok() == Some(b"MacRomanEncoding");
    if font.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font")
        || (!mac_roman
            && !custom
            && font.get(b"Encoding").and_then(Object::as_name).ok() != Some(b"WinAnsiEncoding"))
        || font.get(b"BaseFont").and_then(Object::as_name).is_err()
        || font.iter().any(|(key, _)| {
            !(custom && key == b"ToUnicode")
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
    // No name-based fallback or competing platform map is accepted.
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
            && !((mac_roman || custom)
                && table.platform_id == PlatformId::Macintosh
                && table.encoding_id == 0)
    }) {
        return Err(invalid());
    }
    // PDF 1.6 section 5.5.5: a symbolic font without a (3,0) map uses
    // the string bytes directly in (1,0). ToUnicode supplies text semantics,
    // not glyph selection. Never infer Unicode from a symbolic glyph number.
    if custom && cmap.subtables.len() != 1 {
        return Err(invalid());
    }
    let codes = if custom {
        let stream = crate::encoding::resolve(doc, font.get(b"ToUnicode").map_err(|_| invalid())?)
            .as_stream()
            .map_err(|_| invalid())?;
        Some(mapping::parse(stream)?)
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
        let code = u32::from(code_byte);
        let Some(glyph) = primary.glyph_index(code) else {
            continue;
        };
        // Format 6 returns glyph zero for holes; it is .notdef, not a usable
        // character (unlike the None returned by other cmap formats).
        if glyph.0 == 0 {
            continue;
        }
        if glyph.0 >= face.number_of_glyphs()
            || cmap
                .subtables
                .into_iter()
                .any(|table| table.glyph_index(code) != Some(glyph))
        {
            return Err(invalid());
        }
        if i64::from(code_byte) < first || i64::from(code_byte) > last {
            continue;
        }
        let width = number(&widths[(i64::from(code_byte) - first) as usize])?;
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
                    && bottom * unit >= -250.
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
    // Match the preview spike's conservative embedding policy. No subsetting
    // takes place, so the no-subsetting bit is compatible with this writer.
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
