//! Nonstroking colour state and image colour spaces. Preserve authored colour data;
//! PDFium in the worker renders profiles. These checks bound their envelope and
//! component count, not the colour transforms inside an ICC profile.

use super::{dictionary, filters, number};
use lopdf::{Dictionary, Document, Object};

#[cfg(test)]
mod tests;

fn device(name: &[u8]) -> Option<usize> {
    match name {
        b"DeviceGray" => Some(1),
        b"DeviceRGB" => Some(3),
        b"DeviceCMYK" => Some(4),
        _ => None,
    }
}

// ISO 32000-1, 8.6.5.8 / Table 70. These affect colour conversion, not
// glyph geometry. The writer retains the authored ri/gs operators and state
// dictionaries; PDFium applies the intent during preview and rendering.
pub(super) fn intent(name: &[u8]) -> Result<(), String> {
    match name {
        b"AbsoluteColorimetric" | b"RelativeColorimetric" | b"Saturation" | b"Perceptual" => Ok(()),
        _ => Err("unsupported text rendering intent".into()),
    }
}

pub(super) fn named(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<usize, String> {
    let spaces = resources
        .get(b"ColorSpace")
        .ok()
        .map(|value| dictionary(doc, value))
        .transpose()?;
    if let Some(spaces) = spaces {
        if [b"DefaultGray".as_slice(), b"DefaultRGB", b"DefaultCMYK"]
            .iter()
            .any(|key| spaces.has(key))
        {
            return Err("default colour-space substitution is not editable yet".into());
        }
    }
    if let Some(components) = device(name) {
        return Ok(components);
    }
    let invalid = || "unsupported text colour space or ICC profile header".to_string();
    space(
        doc,
        spaces
            .ok_or_else(invalid)?
            .get(name)
            .map_err(|_| invalid())?,
    )
}

pub(super) fn space(doc: &Document, value: &Object) -> Result<usize, String> {
    let invalid = || "unsupported text colour space or ICC profile header".to_string();
    let value = crate::encoding::resolve(doc, value);
    if let Object::Name(name) = value {
        return device(name).ok_or_else(invalid);
    }
    let values = value.as_array().map_err(|_| invalid())?;
    let [Object::Name(kind), profile] = values.as_slice() else {
        return Err(invalid());
    };
    if kind != b"ICCBased" {
        return Err(invalid());
    }
    let profile = crate::encoding::resolve(doc, profile)
        .as_stream()
        .map_err(|_| invalid())?;
    let components = match profile.dict.get(b"N").and_then(Object::as_i64) {
        Ok(1) => 1,
        Ok(3) => 3,
        Ok(4) => 4,
        _ => return Err(invalid()),
    };
    if let Ok(alternate) = profile.dict.get(b"Alternate") {
        if alternate.as_name().ok().and_then(device) != Some(components) {
            return Err(invalid());
        }
    }
    if let Ok(range) = profile.dict.get(b"Range") {
        let range = range.as_array().map_err(|_| invalid())?;
        if range.len() != components * 2 {
            return Err(invalid());
        }
        for pair in range.chunks_exact(2) {
            if number(&pair[0])? != 0. || number(&pair[1])? != 1. {
                return Err(invalid());
            }
        }
    }
    let bytes = filters::decode(profile, super::MAX_CONTENT)?;
    let signature = match components {
        1 => b"GRAY",
        3 => b"RGB ",
        _ => b"CMYK",
    };
    if bytes.len() < 132
        || u32::from_be_bytes(bytes[..4].try_into().map_err(|_| invalid())?) as usize != bytes.len()
        || &bytes[16..20] != signature
        || &bytes[36..40] != b"acsp"
    {
        return Err(invalid());
    }
    Ok(components)
}

pub(super) fn values(values: &[Object], components: usize) -> Result<(), String> {
    if values.len() != components {
        return Err("wrong text colour component count".into());
    }
    for value in values {
        if !(0.0..=1.0).contains(&number(value)?) {
            return Err("text colour component is out of range".into());
        }
    }
    Ok(())
}
