//! Bounded simple-font encoding. Glyph names determine rendering; ToUnicode
//! may narrow the offered repertoire, but may never contradict those names.
use super::{dictionary, Document, Object, ASCII_NAMES};
use lopdf::Dictionary;

pub(super) fn slots(doc: &Document, font: &Dictionary) -> Result<Box<[Option<u8>; 256]>, String> {
    let invalid = || "unsupported or ambiguous CFF encoding".to_string();
    let mut slots = Box::new([None; 256]);
    for code in 32..=126 {
        slots[code] = Some(code as u8);
    }
    let encoding = crate::encoding::resolve(doc, font.get(b"Encoding").map_err(|_| invalid())?);
    if encoding.as_name().ok() != Some(b"WinAnsiEncoding") {
        let encoding = dictionary(doc, encoding)?;
        if encoding.get(b"BaseEncoding").and_then(Object::as_name).ok() != Some(b"WinAnsiEncoding")
        {
            return Err(invalid());
        }
        for (key, value) in encoding {
            match key.as_slice() {
                b"Type" if value.as_name().ok() == Some(b"Encoding") => {}
                b"BaseEncoding" | b"Differences" => {}
                _ => return Err(invalid()),
            }
        }
        if let Ok(differences) = encoding.get(b"Differences") {
            let differences = crate::encoding::resolve(doc, differences)
                .as_array()
                .map_err(|_| invalid())?;
            if differences.len() > 512 {
                return Err(invalid());
            }
            let mut next = None;
            let mut needs_name = false;
            let mut changed = [false; 256];
            for entry in differences {
                match entry {
                    Object::Integer(code) if (0..=255).contains(code) && !needs_name => {
                        next = Some(*code as usize);
                        needs_name = true;
                    }
                    Object::Name(name) => {
                        let code = next.filter(|&code| code < 256).ok_or_else(invalid)?;
                        if changed[code] {
                            return Err(invalid());
                        }
                        let slot = if name == b".notdef" {
                            None
                        } else {
                            Some(
                                (ASCII_NAMES
                                    .iter()
                                    .position(|candidate| candidate.as_bytes() == name)
                                    .ok_or("unsupported CFF glyph name")?
                                    + 32) as u8,
                            )
                        };
                        slots[code] = slot;
                        changed[code] = true;
                        next = Some(code + 1);
                        needs_name = false;
                    }
                    _ => return Err(invalid()),
                }
            }
            if needs_name {
                return Err(invalid());
            }
        }
    }
    if let Ok(mapping) = font.get(b"ToUnicode") {
        let stream = crate::encoding::resolve(doc, mapping)
            .as_stream()
            .map_err(|_| invalid())?;
        let unicode = super::super::mapping::parse(stream)?;
        for (slot, target) in slots.iter_mut().zip(unicode.iter()) {
            if target.is_some() && target != slot {
                return Err("CFF ToUnicode disagrees with its glyph encoding".into());
            }
            // A present but partial ToUnicode map supplies no evidence for
            // omitted codes. Refuse their text instead of guessing a fallback.
            *slot = *target;
        }
    }
    Ok(slots)
}
