//! Existing named glyphs in single-font Type1C programs with bounded WinAnsi encodings.
//! PDF glyph names select outlines; the CFF's own byte encoding is irrelevant.

use super::{dictionary, filters, number, Codes, Metrics};
use lopdf::{Dictionary, Document, Object};
use std::collections::BTreeMap;
use ttf_parser::{cff::Table, GlyphId};

mod encoding;
mod profile;
#[cfg(test)]
mod tests;

const INVALID: &str = "unsupported embedded CFF font";
// Adobe glyph names for WinAnsi's printable ASCII range, ISO 32000-1 Annex D.
pub(super) const ASCII_NAMES: [&str; 95] = [
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quotesingle",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
];

pub(in crate::textedit) fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    if font.get(b"Type").and_then(Object::as_name).ok() != Some(b"Font")
        || font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1")
        || font.get(b"BaseFont").and_then(Object::as_name).is_err()
        || font.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type"
                    | b"Subtype"
                    | b"BaseFont"
                    | b"Encoding"
                    | b"ToUnicode"
                    | b"Name"
                    | b"FirstChar"
                    | b"LastChar"
                    | b"Widths"
                    | b"FontDescriptor"
            )
        })
    {
        return Err(INVALID.into());
    }
    let descriptor = dictionary(doc, font.get(b"FontDescriptor").map_err(|_| INVALID)?)?;
    let flags = descriptor
        .get(b"Flags")
        .and_then(Object::as_i64)
        .map_err(|_| INVALID)?;
    if flags & (4 | 32 | 262144) != 32
        || descriptor.get(b"Type").and_then(Object::as_name).ok() != Some(b"FontDescriptor")
        || descriptor.get(b"FontName").ok() != font.get(b"BaseFont").ok()
        || descriptor.has(b"FontFile")
        || descriptor.has(b"FontFile2")
    {
        return Err(INVALID.into());
    }
    let stream = crate::encoding::resolve(doc, descriptor.get(b"FontFile3").map_err(|_| INVALID)?)
        .as_stream()
        .map_err(|_| INVALID)?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1C") {
        return Err(INVALID.into());
    }
    let bytes = filters::decode(stream, super::super::MAX_CONTENT)?;
    profile::validate(&bytes)?;
    let face = Table::parse(&bytes).ok_or(INVALID)?;
    let matrix = face.matrix();
    if [
        matrix.sx, matrix.ky, matrix.kx, matrix.sy, matrix.tx, matrix.ty,
    ] != [0.001, 0., 0., 0.001, 0., 0.]
        || face.number_of_glyphs() > 4096
    {
        return Err(INVALID.into());
    }
    let mut names = BTreeMap::new();
    for index in 0..face.number_of_glyphs() {
        let glyph = GlyphId(index);
        let name = face.glyph_name(glyph).ok_or(INVALID)?; // CID fonts have no glyph names.
        if name.is_empty() || name.len() > 127 || names.insert(name, glyph).is_some() {
            return Err(INVALID.into());
        }
    }
    if names.get(".notdef") != Some(&GlyphId(0)) {
        return Err(INVALID.into());
    }
    let first = font
        .get(b"FirstChar")
        .and_then(Object::as_i64)
        .map_err(|_| INVALID)?;
    let last = font
        .get(b"LastChar")
        .and_then(Object::as_i64)
        .map_err(|_| INVALID)?;
    if first < 0 || last > 255 || first > last {
        return Err(INVALID.into());
    }
    let widths = crate::encoding::resolve(doc, font.get(b"Widths").map_err(|_| INVALID)?)
        .as_array()
        .map_err(|_| INVALID)?;
    if widths.len() != (last - first + 1) as usize {
        return Err(INVALID.into());
    }
    let encoding = encoding::slots(doc, font)?;
    let mut result = Box::new([None; 256]);
    let mut codes = Box::new([None; 256]);
    let mut vertical_bounds = [0_f64; 2];
    let mut overhangs = Box::new([[0_f64; 2]; 256]);
    for (code, slot) in encoding.slots.iter().enumerate() {
        let Some(slot) = slot.map(usize::from) else {
            continue;
        };
        let name = encoding.names[code];
        if (code as i64) < first || (code as i64) > last {
            continue;
        }
        let Some(&glyph) = names.get(name) else {
            continue;
        };
        if glyph.0 == 0 {
            continue;
        }
        let width = number(&widths[code - first as usize])?;
        let advance = f64::from(face.glyph_width(glyph).ok_or(INVALID)?);
        if width <= 0. || width > 2000. || (width - advance).abs() > 1. {
            return Err("CFF font widths disagree with its glyph metrics".into());
        }
        match super::outlines::cff_bounds(&face, glyph) {
            Ok(Some([left, bottom, right, top]))
                if left >= -250. && right <= width + 250. && bottom >= -250. && top <= 1000. =>
            {
                vertical_bounds[0] = vertical_bounds[0].min(bottom);
                vertical_bounds[1] = vertical_bounds[1].max(top);
                overhangs[slot] = [left.min(0.), (right - width).max(0.)];
            }
            Ok(None) if matches!(slot, 32 | 160) => {}
            _ => continue,
        }
        if result[slot].is_some() {
            return Err("ambiguous duplicate CFF glyph encoding".into());
        }
        result[slot] = Some(width);
        codes[code] = Some(slot as u8);
    }
    Ok(Metrics {
        opaque: None,
        unicode: None,
        widths: result,
        codes: Some(Codes::Single(codes)),
        vertical_bounds: Some(vertical_bounds),
        horizontal_overhangs: Some(overhangs),
    })
}
