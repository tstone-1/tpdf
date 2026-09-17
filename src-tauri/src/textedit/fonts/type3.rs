//! Bounded, uncoloured Type3 outline fonts. Glyphs remain PDF programs; only
//! their measured metrics and one-byte Unicode encoding enter the editor.
//! ISO 32000-2 9.6.4: d1 widths are in glyph space, transformed by FontMatrix.
use super::{dictionary, filters, mapping, number, unicode, Metrics};
use crate::encoding::resolve;
use lopdf::{content::Content, Dictionary, Document, Object};
use std::collections::BTreeMap;

const INVALID: &str = "unsupported or inconsistent Type3 outline font";

fn get<'a>(dict: &'a Dictionary, key: &[u8]) -> Result<&'a Object, String> {
    dict.get(key).map_err(|_| INVALID.into())
}

fn numbers<const N: usize>(values: &[Object]) -> Result<[f64; N], String> {
    if values.len() != N {
        return Err(INVALID.into());
    }
    let mut result = [0.; N];
    for (target, value) in result.iter_mut().zip(values) {
        *target = number(value)?;
    }
    Ok(result)
}

fn keys(dict: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> {
    if dict
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn glyph_content(bytes: &[u8]) -> Result<Content, String> {
    // lopdf 0.45 tokenises d1 as operator d followed by number 1, even in
    // decode_strict. Locate exactly six numeric tokens and the literal d1,
    // then substitute an alphabetic token in this scratch buffer. Separating
    // the header tokens also handles comments immediately after PDF numbers.
    // No font or saved document bytes are changed. Every other operator still
    // goes through strict decoding and the closed outline grammar below.
    let whitespace = |b: u8| matches!(b, 0 | 9 | 10 | 12 | 13 | 32);
    let mut at = 0;
    let mut normalized = Vec::with_capacity(bytes.len() + 8);
    for index in 0..7 {
        loop {
            while bytes.get(at).is_some_and(|b| whitespace(*b)) {
                at += 1;
            }
            if bytes.get(at) != Some(&b'%') {
                break;
            }
            while bytes.get(at).is_some_and(|b| !matches!(b, b'\r' | b'\n')) {
                at += 1;
            }
        }
        let start = at;
        while bytes.get(at).is_some_and(|b| !whitespace(*b) && *b != b'%') {
            at += 1;
        }
        let token = &bytes[start..at];
        if index < 6 {
            if token.is_empty()
                || !token
                    .iter()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
            {
                return Err("invalid Type3 glyph header".into());
            }
            normalized.extend_from_slice(token);
            normalized.push(b' ');
        } else {
            if token != b"d1" {
                return Err("Type3 glyph must start with d1".into());
            }
            normalized.extend_from_slice(b"dX ");
            normalized.extend_from_slice(&bytes[at..]);
            return Content::decode_strict(&normalized)
                .map_err(|_| "cannot parse Type3 glyph program".into());
        }
    }
    unreachable!()
}

// Control-point hulls enclose Bezier curves. They can overestimate ink but
// never underestimate it, unlike trusting FontBBox or the d1 declaration.
fn outline(
    doc: &Document,
    value: &Object,
    bytes_left: &mut usize,
    ops_left: &mut usize,
) -> Result<(f64, [f64; 4]), String> {
    let stream = resolve(doc, value).as_stream().map_err(|_| INVALID)?;
    keys(&stream.dict, &[b"Length", b"Filter"])
        .map_err(|_| "unsupported Type3 glyph stream dictionary")?;
    let bytes = filters::decode(stream, (*bytes_left).min(64 * 1024))?;
    *bytes_left -= bytes.len();
    let content = glyph_content(&bytes)?;
    if content.operations.is_empty() || content.operations.len() > *ops_left {
        return Err("Type3 outline operations exceed their limit".into());
    }
    *ops_left -= content.operations.len();
    let first = &content.operations[0];
    if first.operator != "dX" {
        return Err("Type3 glyph must start with d1".into());
    }
    let [width, dy, x0, y0, x1, y1] = numbers(&first.operands)?;
    if width <= 0. || dy != 0. || x0 > x1 || y0 > y1 {
        return Err("invalid Type3 d1 metrics".into());
    }
    let mut bounds = [x0, y0, x1, y1];
    let mut open = false;
    let mut painted = false;
    for operation in &content.operations[1..] {
        if painted {
            return Err(INVALID.into());
        }
        let points: &[Object] = match (operation.operator.as_str(), operation.operands.as_slice()) {
            ("m", points) if points.len() == 2 => {
                open = true;
                points
            }
            ("l", points) if open && points.len() == 2 => points,
            ("c", points) if open && points.len() == 6 => points,
            ("h", []) if open => {
                open = false;
                &[]
            }
            ("f" | "f*", []) => {
                painted = true;
                &[]
            }
            _ => return Err("Type3 glyph uses unsupported painting operations".into()),
        };
        for point in points.chunks_exact(2) {
            let [x, y] = numbers(point)?;
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
    }
    if !painted && (content.operations.len() != 1 || bounds != [0.; 4]) {
        return Err(INVALID.into());
    }
    Ok((width, bounds))
}

pub fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    keys(
        font,
        &[
            b"Type",
            b"Subtype",
            b"Name",
            b"FontBBox",
            b"FontMatrix",
            b"CharProcs",
            b"Encoding",
            b"FirstChar",
            b"LastChar",
            b"Widths",
            b"Resources",
            b"ToUnicode",
            b"FontDescriptor",
            b"CIDToGIDMap",
        ],
    )?;
    if get(font, b"Type")?.as_name().ok() != Some(b"Font") {
        return Err(INVALID.into());
    }
    if let Ok(value) = font.get(b"CIDToGIDMap") {
        // Some producers attach this irrelevant CID-font entry to Type3.
        // Only the inert name is retained; never interpret an external map.
        if value.as_name().ok() != Some(b"Identity") {
            return Err(INVALID.into());
        }
    }
    if let Ok(value) = font.get(b"Resources") {
        if !dictionary(doc, value)?.is_empty() {
            return Err(INVALID.into());
        }
    }
    if let Ok(value) = font.get(b"FontDescriptor") {
        let descriptor = dictionary(doc, value)?;
        keys(
            descriptor,
            &[
                b"Type",
                b"FontName",
                b"FontFamily",
                b"FontStretch",
                b"FontWeight",
                b"Flags",
                b"FontBBox",
                b"ItalicAngle",
                b"Ascent",
                b"Descent",
                b"Leading",
                b"CapHeight",
                b"XHeight",
                b"StemV",
                b"StemH",
                b"AvgWidth",
                b"MaxWidth",
                b"MissingWidth",
                b"CharSet",
            ],
        )?;
        if descriptor
            .get(b"Type")
            .is_ok_and(|value| value.as_name().ok() != Some(b"FontDescriptor"))
        {
            return Err(INVALID.into());
        }
    }
    let matrix = resolve(doc, get(font, b"FontMatrix")?)
        .as_array()
        .map_err(|_| INVALID)?;
    let [sx, b, c, sy, x, y] = numbers(matrix)?;
    if sx <= 0. || sy == 0. || b != 0. || c != 0. || x != 0. || y != 0. {
        return Err("unsupported Type3 font matrix".into());
    }
    // FontBBox is metadata, never the authority for ink. Some exporters invert
    // its y corners when sy is negative. Validate numbers and measure the paths.
    let _: [f64; 4] = numbers(
        resolve(doc, get(font, b"FontBBox")?)
            .as_array()
            .map_err(|_| INVALID)?,
    )?;
    let first = get(font, b"FirstChar")?.as_i64().map_err(|_| INVALID)?;
    let last = get(font, b"LastChar")?.as_i64().map_err(|_| INVALID)?;
    if !(0..=255).contains(&first) || !(first..=255).contains(&last) {
        return Err(INVALID.into());
    }
    let values = resolve(doc, get(font, b"Widths")?)
        .as_array()
        .map_err(|_| INVALID)?;
    if values.len() != (last - first + 1) as usize {
        return Err(INVALID.into());
    }
    let widths = values.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    if widths.iter().any(|width| *width < 0.) {
        return Err(INVALID.into());
    }

    let encoding = dictionary(doc, get(font, b"Encoding")?)?;
    keys(encoding, &[b"Type", b"Differences"])?;
    if encoding
        .get(b"Type")
        .is_ok_and(|value| value.as_name().ok() != Some(b"Encoding"))
    {
        return Err(INVALID.into());
    }
    let differences = resolve(doc, get(encoding, b"Differences")?)
        .as_array()
        .map_err(|_| INVALID)?;
    if differences.len() > 512 {
        return Err(INVALID.into());
    }
    let mut names = BTreeMap::new();
    let mut next = None;
    for value in differences {
        match value {
            Object::Integer(code) if (first..=last).contains(code) => next = Some(*code),
            Object::Name(name) if !name.is_empty() && name.len() <= 127 => {
                let code = next.ok_or(INVALID)?;
                if code > last || names.insert(code as u16, name.as_slice()).is_some() {
                    return Err(INVALID.into());
                }
                next = Some(code + 1);
            }
            _ => return Err(INVALID.into()),
        }
    }
    let procs = dictionary(doc, get(font, b"CharProcs")?)?;
    if procs.is_empty() || procs.len() > 256 {
        return Err(INVALID.into());
    }
    let mut decoded = super::super::MAX_CONTENT;
    let mut operations = super::super::MAX_OPERATIONS;
    let mut outlines = BTreeMap::new();
    for (name, value) in procs {
        outlines.insert(
            name.as_slice(),
            outline(doc, value, &mut decoded, &mut operations)?,
        );
    }
    let cmap = resolve(doc, get(font, b"ToUnicode")?)
        .as_stream()
        .map_err(|_| INVALID)?;
    let codes = mapping::unicode_single(cmap)?;
    if codes.is_empty() {
        return Err(INVALID.into());
    }
    let mut glyphs = BTreeMap::new();
    let mut vertical = [0_f64; 2];
    for &code in codes.keys() {
        let name = names.get(&code).ok_or(INVALID)?;
        let &(advance, bounds) = outlines.get(name).ok_or(INVALID)?;
        let width = *widths
            .get(usize::from(code) - first as usize)
            .ok_or(INVALID)?;
        if width != advance {
            return Err("Type3 width disagrees with glyph program".into());
        }
        let width = width * sx * 1000.;
        let [left, bottom, right, top] = [
            bounds[0] * sx * 1000.,
            (bounds[1] * sy).min(bounds[3] * sy) * 1000.,
            bounds[2] * sx * 1000.,
            (bounds[1] * sy).max(bounds[3] * sy) * 1000.,
        ];
        if width <= 0.
            || width > 10000.
            || left < -1000.
            || right > width + 1000.
            || bottom < -1000.
            || top > 2000.
        {
            return Err("Type3 glyph bounds exceed editable limits".into());
        }
        vertical = [vertical[0].min(bottom), vertical[1].max(top)];
        glyphs.insert(code, (width, [left.min(0.), (right - width).max(0.)]));
    }
    Ok(Metrics {
        unicode: Some(unicode::Metrics::single(codes, glyphs)),
        vertical_bounds: Some(vertical),
        widths: Box::new([None; 256]),
        horizontal_overhangs: None,
        codes: None,
    })
}

#[cfg(test)]
mod tests;
