//! Preserve bounded, opaque image XObjects while editing surrounding text.
//! Images never change text state. Forms, stencil masks, external data and
//! custom sample mappings are excluded; the original stream and dictionary stay
//! intact. A soft mask supplies its owner's alpha and is bounded as its own
//! image; it is never painted alone.

use super::{colors, dictionary, filters};
use lopdf::{Dictionary, Document, Object, Stream};

mod jpeg;
#[cfg(test)]
mod tests;

pub(super) fn check(
    doc: &Document,
    resources: &Dictionary,
    name: &[u8],
    remaining: usize,
) -> Result<usize, String> {
    let invalid = || "unsupported image on an editable page".to_string();
    let entries = dictionary(doc, resources.get(b"XObject").map_err(|_| invalid())?)?;
    let image = crate::encoding::resolve(doc, entries.get(name).map_err(|_| invalid())?)
        .as_stream()
        .map_err(|_| invalid())?;
    let bytes = samples(doc, Some(resources), image, remaining)?;
    match image.dict.get(b"SMask") {
        // ISO 32000-1 11.6.5.3: the soft mask carries this image's alpha. It is
        // painted only through the image naming it, so it has no mask of its
        // own, and its samples are charged to the same page budget.
        Ok(mask) => {
            let mask = crate::encoding::resolve(doc, mask)
                .as_stream()
                .map_err(|_| invalid())?;
            if mask.dict.has(b"SMask") {
                return Err(invalid());
            }
            Ok(bytes + samples(doc, None, mask, remaining - bytes)?)
        }
        Err(_) => Ok(bytes),
    }
}

// A painted image resolves named colour spaces through the page resources. A
// soft mask reaches none, which is also what distinguishes the two here.
fn samples(
    doc: &Document,
    resources: Option<&Dictionary>,
    image: &Stream,
    remaining: usize,
) -> Result<usize, String> {
    let invalid = || "unsupported image on an editable page".to_string();
    for (key, value) in &image.dict {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"XObject" => {}
            (b"Subtype", Object::Name(name)) if name == b"Image" => {}
            (b"ImageMask", Object::Boolean(false)) => {}
            (b"Interpolate", Object::Boolean(_)) => {}
            (b"Intent", Object::Name(name)) => colors::intent(name)?,
            // Obsolete image identifier; the resource dictionary resolves Do.
            (b"Name", Object::Name(name)) if !name.is_empty() && name.len() <= 127 => {}
            // The image's own XMP packet, preserved unchanged. It describes the
            // image; nothing in it maps a sample.
            (b"Metadata", _) => {
                let packet = crate::encoding::resolve(doc, value)
                    .as_stream()
                    .map_err(|_| invalid())?;
                if packet.dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Metadata") {
                    return Err(invalid());
                }
            }
            (b"DecodeParms", _) => parameters(doc, value)?,
            // Validated by the caller, which owns the shared budget.
            (b"SMask", _) => {}
            (
                b"Width" | b"Height" | b"BitsPerComponent" | b"ColorSpace" | b"Length" | b"Filter"
                | b"Decode",
                _,
            ) => {}
            _ => return Err(invalid()),
        }
    }
    if image.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Image")
        || image
            .dict
            .get(b"BitsPerComponent")
            .and_then(Object::as_i64)
            .ok()
            != Some(8)
    {
        return Err(invalid());
    }
    let dimension = |key: &[u8]| -> Result<usize, String> {
        let value = image
            .dict
            .get(key)
            .and_then(Object::as_i64)
            .map_err(|_| invalid())?;
        if !(1..=8192).contains(&value) {
            return Err(invalid());
        }
        Ok(value as usize)
    };
    let width = dimension(b"Width")?;
    let height = dimension(b"Height")?;
    let space =
        crate::encoding::resolve(doc, image.dict.get(b"ColorSpace").map_err(|_| invalid())?);
    let mut palette_bytes = 0;
    let mut high_index = None;
    let components = match resources {
        // ISO 32000-1 11.6.5.3: a soft mask holds alpha rather than colour, and
        // its samples are read as DeviceGray whatever its owner paints with.
        None if space.as_name().ok() == Some(b"DeviceGray") => 1,
        None => return Err(invalid()),
        Some(resources) => match space {
            Object::Array(values) if matches!(values.first(), Some(Object::Name(name)) if name == b"Indexed") =>
            {
                let [_, base, high, lookup] = values.as_slice() else {
                    return Err(invalid());
                };
                let components = colors::space(doc, crate::encoding::resolve(doc, base))?;
                let high = high.as_i64().map_err(|_| invalid())?;
                if !(0..=255).contains(&high) {
                    return Err(invalid());
                }
                palette_bytes = (high as usize + 1) * components;
                let lookup = crate::encoding::resolve(doc, lookup);
                let bytes = match lookup {
                    Object::String(bytes, _) if bytes.len() == palette_bytes => bytes.clone(),
                    Object::Stream(stream) => filters::decode(stream, palette_bytes)?,
                    _ => return Err(invalid()),
                };
                if bytes.len() != palette_bytes {
                    return Err(invalid());
                }
                high_index = Some(high as u8);
                1
            }
            Object::Name(name) => colors::named(doc, resources, name)?,
            value => colors::space(doc, value)?,
        },
    };
    // ISO 32000-1 Table 89: only this colour space's default sample mapping is
    // accepted, so no image the editor keeps needs its samples remapped. An
    // explicit copy of that default is written by several ordinary producers.
    if let Ok(decode) = image.dict.get(b"Decode") {
        let default = if high_index.is_some() {
            vec![0., 255.]
        } else {
            [0., 1.].repeat(components)
        };
        let mapping = crate::encoding::resolve(doc, decode)
            .as_array()
            .map_err(|_| invalid())?
            .iter()
            .map(super::number)
            .collect::<Result<Vec<_>, _>>()?;
        if mapping != default {
            return Err(invalid());
        }
    }
    // Dimensions are bounded before multiplication or decompression. The budget
    // is shared across distinct image names on the page, not reset per image.
    let samples = width * height * components;
    let bytes = samples + palette_bytes;
    if bytes > remaining {
        return Err("decoded images exceed the editable page budget".into());
    }
    let dct = match image.dict.get(b"Filter") {
        Ok(Object::Name(name)) => name == b"DCTDecode",
        Ok(Object::Array(names)) => {
            matches!(names.as_slice(), [Object::Name(name)] if name == b"DCTDecode")
        }
        _ => false,
    };
    if dct {
        if high_index.is_some() {
            return Err(invalid());
        }
        jpeg::check(&image.content, width, height, components)?;
        return Ok(bytes);
    }
    let decoded = filters::decode_unpredicted(image, samples)?;
    if decoded.len() != samples
        || high_index.is_some_and(|high| decoded.iter().any(|&sample| sample > high))
    {
        return Err("image samples do not match dimensions and colour components".into());
    }
    Ok(bytes)
}

// ISO 32000-1 Table 10. Predictor 1, the default, leaves the filter's output as
// the sample data, and the remaining entries then describe nothing. Any other
// predictor would have to be undone before those samples could be read.
fn parameters(doc: &Document, value: &Object) -> Result<(), String> {
    let invalid = || "unsupported image on an editable page".to_string();
    let value = crate::encoding::resolve(doc, value);
    let value = match value {
        Object::Array(values) if values.len() == 1 => crate::encoding::resolve(doc, &values[0]),
        value => value,
    };
    for (key, value) in value.as_dict().map_err(|_| invalid())? {
        match key.as_slice() {
            b"Predictor" if value.as_i64().ok() == Some(1) => {}
            b"Colors" | b"Columns" | b"BitsPerComponent" => {
                value.as_i64().map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
    }
    Ok(())
}
