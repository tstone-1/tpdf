//! Preserve bounded, opaque image XObjects while editing surrounding text.
//! Images never change text state. Forms, masks, external data and custom
//! sample mappings are excluded; the original stream and dictionary stay intact.

use super::{colors, dictionary, filters};
use lopdf::{Dictionary, Document, Object};

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
    for (key, value) in &image.dict {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"XObject" => {}
            (b"Subtype", Object::Name(name)) if name == b"Image" => {}
            (b"ImageMask", Object::Boolean(false)) => {}
            (b"Interpolate", Object::Boolean(_)) => {}
            (b"Intent", Object::Name(name)) => colors::intent(name)?,
            (
                b"Width" | b"Height" | b"BitsPerComponent" | b"ColorSpace" | b"Length" | b"Filter",
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
    let components = match space {
        Object::Name(name) => colors::named(doc, resources, name)?,
        value => colors::space(doc, value)?,
    };
    // Dimensions are bounded before multiplication or decompression. The budget
    // is shared across distinct image names on the page, not reset per image.
    let bytes = width * height * components;
    if bytes > remaining {
        return Err("decoded images exceed the editable page budget".into());
    }
    let decoded = filters::decode(image, bytes)?;
    if decoded.len() != bytes {
        return Err("image samples do not match dimensions and colour components".into());
    }
    Ok(bytes)
}
