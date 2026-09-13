//! Bounded existing-glyph Identity-H editing. PDF code == CID == TrueType glyph
//! index only when CIDToGIDMap explicitly says Identity; ToUnicode supplies text
//! semantics, not glyph selection. No cmap inference, new glyphs or font writes.

use super::{dictionary, mapping, number, Codes, Metrics};
use lopdf::{Dictionary, Document, Object};
use std::collections::BTreeMap;
use ttf_parser::GlyphId;

#[cfg(test)]
mod tests;

const INVALID: &str = "unsupported composite TrueType font or character mapping";
const MAX_WIDTHS: usize = 4096;

fn keys(dict: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> {
    if dict
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn name(dict: &Dictionary, key: &[u8], expected: &[u8]) -> Result<(), String> {
    if dict.get(key).and_then(Object::as_name).ok() != Some(expected) {
        return Err(INVALID.into());
    }
    Ok(())
}

// Parse both W forms completely, including entries that are not offered for
// editing. Reject duplicate/overlapping or reversed ranges before expansion.
fn widths(font: &Dictionary) -> Result<(f64, BTreeMap<u16, f64>), String> {
    let width = |value: &Object| -> Result<f64, String> {
        let value = number(value)?;
        if !(0.0..=2000.0).contains(&value) {
            return Err(INVALID.into());
        }
        Ok(value)
    };
    let cid = |value: &Object| -> Result<u16, String> {
        u16::try_from(value.as_i64().map_err(|_| INVALID)?).map_err(|_| INVALID.into())
    };
    let default = font.get(b"DW").map_or(Ok(1000.), width)?;
    let mut result = BTreeMap::new();
    let Some(values) = font.get(b"W").ok() else {
        return Ok((default, result));
    };
    let mut values = values.as_array().map_err(|_| INVALID)?.as_slice();
    while let [first, rest @ ..] = values {
        let first = cid(first)?;
        let Some(next) = rest.first() else {
            return Err(INVALID.into());
        };
        let (last, widths, remaining) = if let Object::Array(array) = next {
            if array.is_empty() || array.len() > MAX_WIDTHS {
                return Err(INVALID.into());
            }
            let last = u16::try_from(usize::from(first) + array.len() - 1).map_err(|_| INVALID)?;
            (
                last,
                array.iter().map(width).collect::<Result<Vec<_>, _>>()?,
                &rest[1..],
            )
        } else {
            let [last, value, tail @ ..] = rest else {
                return Err(INVALID.into());
            };
            let last = cid(last)?;
            if last < first || usize::from(last - first) + 1 > MAX_WIDTHS {
                return Err(INVALID.into());
            }
            (
                last,
                vec![width(value)?; usize::from(last - first) + 1],
                tail,
            )
        };
        if result.len() + widths.len() > MAX_WIDTHS
            || result
                .last_key_value()
                .is_some_and(|(previous, _)| *previous >= first)
        {
            return Err(INVALID.into());
        }
        result.extend((first..=last).zip(widths));
        values = remaining;
    }
    Ok((default, result))
}

pub(in crate::textedit) fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    keys(
        font,
        &[
            b"Type",
            b"Subtype",
            b"BaseFont",
            b"Encoding",
            b"DescendantFonts",
            b"ToUnicode",
        ],
    )?;
    name(font, b"Type", b"Font")?;
    name(font, b"Subtype", b"Type0")?;
    name(font, b"Encoding", b"Identity-H")?;
    let base = font
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .map_err(|_| INVALID)?;
    let children = font
        .get(b"DescendantFonts")
        .and_then(Object::as_array)
        .map_err(|_| INVALID)?;
    let [child] = children.as_slice() else {
        return Err(INVALID.into());
    };
    let child = dictionary(doc, child)?;
    keys(
        child,
        &[
            b"Type",
            b"Subtype",
            b"BaseFont",
            b"CIDSystemInfo",
            b"FontDescriptor",
            b"CIDToGIDMap",
            b"W",
            b"DW",
        ],
    )?;
    name(child, b"Type", b"Font")?;
    name(child, b"Subtype", b"CIDFontType2")?;
    name(child, b"BaseFont", base)?;
    name(child, b"CIDToGIDMap", b"Identity")?;
    let info = dictionary(doc, child.get(b"CIDSystemInfo").map_err(|_| INVALID)?)?;
    keys(info, &[b"Registry", b"Ordering", b"Supplement"])?;
    if info.get(b"Registry").and_then(Object::as_str).ok() != Some(b"Adobe")
        || info.get(b"Ordering").and_then(Object::as_str).ok() != Some(b"Identity")
        || info.get(b"Supplement").and_then(Object::as_i64).ok() != Some(0)
    {
        return Err(INVALID.into());
    }
    let descriptor = dictionary(doc, child.get(b"FontDescriptor").map_err(|_| INVALID)?)?;
    name(descriptor, b"Type", b"FontDescriptor")?;
    name(descriptor, b"FontName", base)?;
    let flags = descriptor
        .get(b"Flags")
        .and_then(Object::as_i64)
        .map_err(|_| INVALID)?;
    if flags & (4 | 32 | 262144) != 4 {
        return Err(INVALID.into());
    }
    let bytes = super::program(doc, descriptor)?;
    let face = super::face(&bytes, false)?;
    let stream = crate::encoding::resolve(doc, font.get(b"ToUnicode").map_err(|_| INVALID)?)
        .as_stream()
        .map_err(|_| INVALID)?;
    let codes = mapping::parse_cid(stream)?;
    let (default, widths) = widths(child)?;
    let unit = 1000. / f64::from(face.units_per_em());
    let mut result = Box::new([None; 256]);
    let mut vertical_bounds = [0_f64; 2];
    for (&code, &ch) in &codes {
        // Identity-H and an explicit Identity CIDToGIDMap make cmap irrelevant.
        let glyph = GlyphId(code);
        if code == 0 || code >= face.number_of_glyphs() {
            return Err(INVALID.into());
        }
        let width = widths.get(&code).copied().unwrap_or(default);
        let advance = f64::from(face.glyph_hor_advance(glyph).ok_or(INVALID)?) * unit;
        if width <= 0. || (width - advance).abs() > 1. {
            return Err("composite font widths disagree with its glyph metrics".into());
        }
        match super::outlines::bounds(&face, glyph) {
            Some([left, bottom, right, top])
                if left * unit >= 0.
                    && right * unit <= width
                    && bottom * unit >= -250.
                    && top * unit <= 1000. =>
            {
                vertical_bounds[0] = vertical_bounds[0].min(bottom * unit);
                vertical_bounds[1] = vertical_bounds[1].max(top * unit);
            }
            None if ch == b' ' && super::empty_glyph(&face, glyph) == Some(true) => {}
            _ => continue,
        }
        result[ch as usize] = Some(width);
    }
    Ok(Metrics {
        vertical_bounds: Some(vertical_bounds),
        widths: result,
        codes: Some(Codes::Double(codes)),
    })
}
