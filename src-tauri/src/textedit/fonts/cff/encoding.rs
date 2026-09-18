//! Bounded simple-font encoding. Glyph names determine rendering; ToUnicode
//! may narrow the offered repertoire, but may never contradict those names.
use super::{dictionary, Document, Object, ASCII_NAMES};
use lopdf::Dictionary;

pub(super) struct Encoding {
    pub slots: Box<[Option<u8>; 256]>,
    pub names: Box<[&'static str; 256]>,
}

// Exact names, not name normalization: these select the embedded outlines.
const EXTRA: [(&str, u8); 6] = [
    ("minus", 0x80),
    ("uni00A0", 0xa0),
    ("sterling", 0xa3),
    ("quoteleft", 0x91),
    ("quoteright", 0x92),
    ("endash", 0x96),
];

pub(super) fn slots(doc: &Document, font: &Dictionary) -> Result<Encoding, String> {
    let invalid = || "unsupported or ambiguous CFF encoding".to_string();
    let mut slots = Box::new([None; 256]);
    let mut names = Box::new([""; 256]);
    for code in 32..=126 {
        slots[code] = Some(code as u8);
        names[code] = ASCII_NAMES[code - 32];
    }
    // WinAnsi names for the supported non-ASCII codes. Its code 160 names
    // space, not uni00A0; do not invent a NBSP glyph or a duplicate space code.
    for (name, slot) in EXTRA {
        if matches!(slot, 0x91 | 0x92 | 0x96 | 0xa3) {
            slots[slot as usize] = Some(slot);
            names[slot as usize] = name;
        }
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
                            names[code] = "";
                            None
                        } else {
                            let (glyph_name, slot) = ASCII_NAMES
                                .iter()
                                .enumerate()
                                .map(|(index, &name)| (name, (index + 32) as u8))
                                .chain(EXTRA)
                                .chain(
                                    super::super::ligatures::GLYPHS
                                        .map(|(name, _, slot)| (name, slot)),
                                )
                                // Adobe's original names (`fi`, `fl`, `ffi`),
                                // which spell the ligature's own text and
                                // which older CFF fonts still use.
                                .chain(
                                    super::super::ligatures::GLYPHS
                                        .map(|(_, text, slot)| (text, slot)),
                                )
                                .find(|(candidate, _)| candidate.as_bytes() == name)
                                .ok_or("unsupported CFF glyph name")?;
                            names[code] = glyph_name;
                            Some(slot)
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
        let unicode = super::super::mapping::parse_cff(stream)?;
        for (slot, target) in slots.iter_mut().zip(unicode.iter()) {
            if target.is_some() && target != slot {
                return Err("CFF ToUnicode disagrees with its glyph encoding".into());
            }
            // A present but partial ToUnicode map supplies no evidence for
            // omitted codes. Refuse their text instead of guessing a fallback.
            *slot = *target;
        }
    }
    if !font.has(b"ToUnicode") {
        // Reader agreement for uni00A0 requires an explicit Unicode map.
        for slot in slots.iter_mut() {
            if slot
                .is_some_and(|slot| slot == 0xa0 || super::super::ligatures::text(slot).is_some())
            {
                *slot = None;
            }
        }
    }
    Ok(Encoding { slots, names })
}
