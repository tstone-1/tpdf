//! Stencil masks (ISO 32000-1 8.9.6.2): 1-bit images that paint the current
//! fill colour where a sample selects it. Scanners and Acrobat's OCR output
//! store them as CCITT Group 4, which is decoded here line by line and kept
//! byte for byte. Nothing in a stencil maps a colour, so only its shape is
//! validated: the dictionary, the encoding and the number of complete rows.

use super::super::filters;
use lopdf::{Document, Object, Stream};

fn invalid() -> String {
    "unsupported stencil mask on an editable page".into()
}

pub(super) fn check(doc: &Document, image: &Stream, remaining: usize) -> Result<usize, String> {
    let mut ccitt = false;
    for (key, value) in &image.dict {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"XObject" => {}
            (b"Subtype", Object::Name(name)) if name == b"Image" => {}
            (b"ImageMask", Object::Boolean(true)) => {}
            (b"BitsPerComponent", Object::Integer(1)) => {}
            (b"Interpolate", Object::Boolean(_)) => {}
            (b"Name", Object::Name(name)) if !name.is_empty() && name.len() <= 127 => {}
            (b"Width" | b"Height" | b"Length", _) => {}
            // Either polarity: [1 0] only chooses which samples paint.
            (b"Decode", value) => {
                let mapping = crate::encoding::resolve(doc, value)
                    .as_array()
                    .map_err(|_| invalid())?
                    .iter()
                    .map(super::super::number)
                    .collect::<Result<Vec<_>, _>>()?;
                if mapping != [0., 1.] && mapping != [1., 0.] {
                    return Err(invalid());
                }
            }
            (b"Filter", value) => {
                ccitt = match crate::encoding::resolve(doc, value) {
                    Object::Name(name) if name == b"CCITTFaxDecode" => true,
                    Object::Array(names) => match names.as_slice() {
                        [Object::Name(name)] if name == b"CCITTFaxDecode" => true,
                        [Object::Name(name)] if name == b"FlateDecode" => false,
                        _ => return Err(invalid()),
                    },
                    Object::Name(name) if name == b"FlateDecode" => false,
                    _ => return Err(invalid()),
                }
            }
            (b"DecodeParms", _) => {}
            _ => return Err(invalid()),
        }
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
    let bytes = width.div_ceil(8) * height;
    if bytes > remaining {
        return Err("decoded images exceed the editable page budget".into());
    }
    if ccitt {
        parameters(doc, image, width, height)?;
        if image.content.len() > 2 * super::super::MAX_CONTENT {
            return Err(invalid());
        }
        group4(&image.content, width, height)?;
    } else {
        if image.dict.has(b"DecodeParms") {
            return Err(invalid());
        }
        if filters::decode(image, bytes)?.len() != bytes {
            return Err("image samples do not match dimensions and colour components".into());
        }
    }
    Ok(bytes)
}

// ISO 32000-1 Table 11. Only pure two-dimensional Group 4 (K < 0) is read, on
// its default row framing; the other entries describe the same bits.
fn parameters(doc: &Document, image: &Stream, width: usize, height: usize) -> Result<(), String> {
    let value = image.dict.get(b"DecodeParms").map_err(|_| invalid())?;
    let value = match crate::encoding::resolve(doc, value) {
        Object::Array(values) if values.len() == 1 => crate::encoding::resolve(doc, &values[0]),
        value => value,
    };
    let parameters = value.as_dict().map_err(|_| invalid())?;
    // Absent entries take their defaults; a present one must be an integer.
    let integer = |key: &[u8], default: i64| match parameters.get(key) {
        Err(_) => Some(default),
        Ok(value) => value.as_i64().ok(),
    };
    let rows = integer(b"Rows", 0);
    if !integer(b"K", 0).is_some_and(|k| k < 0)
        || integer(b"Columns", 1728) != Some(width as i64)
        // Zero means the rows are not stated; Height then counts them.
        || (rows != Some(0) && rows != Some(height as i64))
        || integer(b"DamagedRowsBeforeError", 0) != Some(0)
    {
        return Err(invalid());
    }
    for (key, value) in parameters {
        match (key.as_slice(), value) {
            (b"K" | b"Columns" | b"Rows" | b"DamagedRowsBeforeError", _) => {}
            (b"BlackIs1" | b"EndOfBlock", Object::Boolean(_)) => {}
            // Byte-aligned rows and end-of-line codes frame rows differently.
            (b"EncodedByteAlign" | b"EndOfLine", Object::Boolean(false)) => {}
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

// Every row must decode completely. A stream that ends early, or an
// end-of-block before the last row, is refused rather than padded with white.
fn group4(data: &[u8], width: usize, height: usize) -> Result<(), String> {
    use fax::decoder::{DecodeStatus, Group4Decoder};
    let failed = || "incomplete or unsupported CCITT image".to_string();
    let reader = data.iter().copied().map(Ok::<u8, std::convert::Infallible>);
    let mut decoder = Group4Decoder::new(reader, width as u32).map_err(|_| failed())?;
    for _ in 0..height {
        if !matches!(decoder.advance(), Ok(DecodeStatus::Incomplete)) {
            return Err(failed());
        }
    }
    Ok(())
}
