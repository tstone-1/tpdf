//! Existing named glyphs in embedded Adobe Type 1 programs (FontFile), as pdfTeX,
//! dvipdfm and older Distiller and Ghostscript exports write them, and in the
//! symbolic or built-in-encoded Type1C programs (FontFile3) xdvipdfmx writes for
//! the same TeX fonts. PDF glyph names select charstrings; a ToUnicode map may
//! narrow the offered repertoire but never contradict those names. No font
//! bytes are changed.

use super::{dictionary, filters, number, Codes, Metrics};
use lopdf::{Dictionary, Document, Object};

pub(super) mod program;
#[cfg(test)]
mod tests;

const INVALID: &str = "unsupported embedded Type 1 font";
// How far a read-only glyph's ink may reach from its origin, in thousandths.
pub(super) const OPAQUE_REACH: f64 = 4000.;

// ISO 32000-1 Annex D.2 glyph names for WinAnsi's codes above ASCII, with the
// metric slot each one occupies. 0x80 is the internal minus slot; 0xA0 and 0xAD
// (space and hyphen aliases) are omitted because their Unicode is ambiguous.
const WINANSI_NAMES: [(&str, u8); 121] = [
    ("minus", 0x80),
    ("quotesinglbase", 0x82),
    ("florin", 0x83),
    ("quotedblbase", 0x84),
    ("ellipsis", 0x85),
    ("dagger", 0x86),
    ("daggerdbl", 0x87),
    ("circumflex", 0x88),
    ("perthousand", 0x89),
    ("Scaron", 0x8A),
    ("guilsinglleft", 0x8B),
    ("OE", 0x8C),
    ("Zcaron", 0x8E),
    ("quoteleft", 0x91),
    ("quoteright", 0x92),
    ("quotedblleft", 0x93),
    ("quotedblright", 0x94),
    ("bullet", 0x95),
    ("endash", 0x96),
    ("emdash", 0x97),
    ("tilde", 0x98),
    ("trademark", 0x99),
    ("scaron", 0x9A),
    ("guilsinglright", 0x9B),
    ("oe", 0x9C),
    ("zcaron", 0x9E),
    ("Ydieresis", 0x9F),
    ("exclamdown", 0xA1),
    ("cent", 0xA2),
    ("sterling", 0xA3),
    ("currency", 0xA4),
    ("yen", 0xA5),
    ("brokenbar", 0xA6),
    ("section", 0xA7),
    ("dieresis", 0xA8),
    ("copyright", 0xA9),
    ("ordfeminine", 0xAA),
    ("guillemotleft", 0xAB),
    ("logicalnot", 0xAC),
    ("registered", 0xAE),
    ("macron", 0xAF),
    ("degree", 0xB0),
    ("plusminus", 0xB1),
    ("twosuperior", 0xB2),
    ("threesuperior", 0xB3),
    ("acute", 0xB4),
    ("mu", 0xB5),
    ("paragraph", 0xB6),
    ("periodcentered", 0xB7),
    ("cedilla", 0xB8),
    ("onesuperior", 0xB9),
    ("ordmasculine", 0xBA),
    ("guillemotright", 0xBB),
    ("onequarter", 0xBC),
    ("onehalf", 0xBD),
    ("threequarters", 0xBE),
    ("questiondown", 0xBF),
    ("Agrave", 0xC0),
    ("Aacute", 0xC1),
    ("Acircumflex", 0xC2),
    ("Atilde", 0xC3),
    ("Adieresis", 0xC4),
    ("Aring", 0xC5),
    ("AE", 0xC6),
    ("Ccedilla", 0xC7),
    ("Egrave", 0xC8),
    ("Eacute", 0xC9),
    ("Ecircumflex", 0xCA),
    ("Edieresis", 0xCB),
    ("Igrave", 0xCC),
    ("Iacute", 0xCD),
    ("Icircumflex", 0xCE),
    ("Idieresis", 0xCF),
    ("Eth", 0xD0),
    ("Ntilde", 0xD1),
    ("Ograve", 0xD2),
    ("Oacute", 0xD3),
    ("Ocircumflex", 0xD4),
    ("Otilde", 0xD5),
    ("Odieresis", 0xD6),
    ("multiply", 0xD7),
    ("Oslash", 0xD8),
    ("Ugrave", 0xD9),
    ("Uacute", 0xDA),
    ("Ucircumflex", 0xDB),
    ("Udieresis", 0xDC),
    ("Yacute", 0xDD),
    ("Thorn", 0xDE),
    ("germandbls", 0xDF),
    ("agrave", 0xE0),
    ("aacute", 0xE1),
    ("acircumflex", 0xE2),
    ("atilde", 0xE3),
    ("adieresis", 0xE4),
    ("aring", 0xE5),
    ("ae", 0xE6),
    ("ccedilla", 0xE7),
    ("egrave", 0xE8),
    ("eacute", 0xE9),
    ("ecircumflex", 0xEA),
    ("edieresis", 0xEB),
    ("igrave", 0xEC),
    ("iacute", 0xED),
    ("icircumflex", 0xEE),
    ("idieresis", 0xEF),
    ("eth", 0xF0),
    ("ntilde", 0xF1),
    ("ograve", 0xF2),
    ("oacute", 0xF3),
    ("ocircumflex", 0xF4),
    ("otilde", 0xF5),
    ("odieresis", 0xF6),
    ("divide", 0xF7),
    ("oslash", 0xF8),
    ("ugrave", 0xF9),
    ("uacute", 0xFA),
    ("ucircumflex", 0xFB),
    ("udieresis", 0xFC),
    ("yacute", 0xFD),
    ("thorn", 0xFE),
    ("ydieresis", 0xFF),
];

// Adobe Glyph List names for the ligatures the editor can offer, in both the
// underscore form (AGL specification 2.0) and the Standard Encoding form.
const LIGATURE_NAMES: [(&str, &str); 5] = [
    ("fi", "f_i"),
    ("fl", "f_l"),
    ("ff", "f_f"),
    ("ffi", "f_f_i"),
    ("ffl", "f_f_l"),
];

// Adobe StandardEncoding above ASCII, where it differs from WinAnsi (PLRM 3,
// Appendix E.6). Below 0x80 it differs only at 0x27 and 0x60.
const STANDARD_UPPER: [(u8, &str); 57] = [
    (0xA1, "exclamdown"),
    (0xA2, "cent"),
    (0xA3, "sterling"),
    (0xA4, "fraction"),
    (0xA5, "yen"),
    (0xA6, "florin"),
    (0xA7, "section"),
    (0xA8, "currency"),
    (0xA9, "quotesingle"),
    (0xAA, "quotedblleft"),
    (0xAB, "guillemotleft"),
    (0xAC, "guilsinglleft"),
    (0xAD, "guilsinglright"),
    (0xAE, "fi"),
    (0xAF, "fl"),
    (0xB1, "endash"),
    (0xB2, "dagger"),
    (0xB3, "daggerdbl"),
    (0xB4, "periodcentered"),
    (0xB6, "paragraph"),
    (0xB7, "bullet"),
    (0xB8, "quotesinglbase"),
    (0xB9, "quotedblbase"),
    (0xBA, "quotedblright"),
    (0xBB, "guillemotright"),
    (0xBC, "ellipsis"),
    (0xBD, "perthousand"),
    (0xBF, "questiondown"),
    (0xC1, "grave"),
    (0xC2, "acute"),
    (0xC3, "circumflex"),
    (0xC4, "tilde"),
    (0xC5, "macron"),
    (0xC6, "breve"),
    (0xC7, "dotaccent"),
    (0xC8, "dieresis"),
    (0xCA, "ring"),
    (0xCB, "cedilla"),
    (0xCD, "hungarumlaut"),
    (0xCE, "ogonek"),
    (0xCF, "caron"),
    (0xD0, "emdash"),
    (0xE1, "AE"),
    (0xE3, "ordfeminine"),
    (0xE8, "Lslash"),
    (0xE9, "Oslash"),
    (0xEA, "OE"),
    (0xEB, "ordmasculine"),
    (0xF1, "ae"),
    (0xF5, "dotlessi"),
    (0xF8, "lslash"),
    (0xF9, "oslash"),
    (0xFA, "oe"),
    (0xFB, "germandbls"),
    (0x27, "quoteright"),
    (0x60, "quoteleft"),
    (0x20, "space"),
];

// The metric slot a glyph name denotes, if the editor can offer it.
fn slot(name: &[u8]) -> Option<u8> {
    let name = std::str::from_utf8(name).ok()?;
    if let Some(index) = super::cff::ASCII_NAMES.iter().position(|&n| n == name) {
        return Some(index as u8 + 32);
    }
    if let Some(&(_, slot)) = WINANSI_NAMES.iter().find(|(n, _)| *n == name) {
        return Some(slot);
    }
    let canonical = LIGATURE_NAMES
        .iter()
        .find(|(short, long)| *short == name || *long == name)
        .map(|(_, long)| *long)?;
    super::ligatures::GLYPHS
        .iter()
        .find(|(glyph, _, _)| *glyph == canonical)
        .map(|(_, _, slot)| *slot)
}

// Code to glyph name. ISO 32000-1 Table 114: with no BaseEncoding, or no
// Encoding at all, an embedded font's base is its program's built-in encoding.
fn names(
    doc: &Document,
    font: &Dictionary,
    builtin: &program::Builtin,
) -> Result<Box<[Option<Vec<u8>>; 256]>, String> {
    let invalid = || "unsupported or ambiguous Type 1 encoding".to_string();
    let mut names: Box<[Option<Vec<u8>>; 256]> = Box::new(std::array::from_fn(|_| None));
    let winansi = |names: &mut [Option<Vec<u8>>; 256]| {
        for (index, name) in super::cff::ASCII_NAMES.iter().enumerate() {
            names[index + 32] = Some(name.as_bytes().to_vec());
        }
        for (name, code) in WINANSI_NAMES {
            if code != 0x80 {
                names[code as usize] = Some(name.as_bytes().to_vec());
            }
        }
    };
    let standard = |names: &mut [Option<Vec<u8>>; 256]| {
        for (index, name) in super::cff::ASCII_NAMES.iter().enumerate() {
            names[index + 32] = Some(name.as_bytes().to_vec());
        }
        for (code, name) in STANDARD_UPPER {
            names[code as usize] = Some(name.as_bytes().to_vec());
        }
    };
    let base = |names: &mut [Option<Vec<u8>>; 256], encoding: Option<&[u8]>| match encoding {
        Some(b"WinAnsiEncoding") => {
            winansi(names);
            Ok(())
        }
        Some(b"StandardEncoding") => {
            standard(names);
            Ok(())
        }
        Some(_) => Err(invalid()),
        None => {
            match builtin {
                program::Builtin::Standard => standard(names),
                program::Builtin::Custom(custom) => names.clone_from_slice(&custom[..]),
            }
            Ok(())
        }
    };
    let Ok(encoding) = font.get(b"Encoding") else {
        base(&mut names, None)?;
        return Ok(names);
    };
    let encoding = crate::encoding::resolve(doc, encoding);
    if let Ok(name) = encoding.as_name() {
        base(&mut names, Some(name))?;
        return Ok(names);
    }
    let encoding = dictionary(doc, encoding)?;
    for (key, value) in encoding {
        match key.as_slice() {
            b"Type" if value.as_name().ok() == Some(b"Encoding") => {}
            b"BaseEncoding" | b"Differences" => {}
            _ => return Err(invalid()),
        }
    }
    base(
        &mut names,
        match encoding.get(b"BaseEncoding") {
            Ok(value) => Some(value.as_name().map_err(|_| invalid())?),
            Err(_) => None,
        },
    )?;
    let Ok(differences) = encoding.get(b"Differences") else {
        return Ok(names);
    };
    let differences = crate::encoding::resolve(doc, differences)
        .as_array()
        .map_err(|_| invalid())?;
    if differences.len() > 512 {
        return Err(invalid());
    }
    let mut next = None;
    let mut changed = [false; 256];
    for entry in differences {
        match entry {
            Object::Integer(code) if (0..=255).contains(code) => next = Some(*code as usize),
            Object::Name(name) => {
                let code = next.filter(|&code| code < 256).ok_or_else(invalid)?;
                if std::mem::replace(&mut changed[code], true) || name.len() > 127 {
                    return Err(invalid());
                }
                names[code] = (name != b".notdef").then(|| name.clone());
                next = Some(code + 1);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(names)
}

// The font and descriptor checks both program carriers share.
fn descriptor<'a>(doc: &'a Document, font: &'a Dictionary) -> Result<&'a Dictionary, String> {
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
    // Exactly one of symbolic and nonsymbolic. Glyph names select outlines
    // either way, so the encoding rules below are the same for both.
    if !matches!(flags & (4 | 32), 4 | 32) {
        return Err(INVALID.into());
    }
    if descriptor.get(b"Type").and_then(Object::as_name).ok() != Some(b"FontDescriptor")
        || descriptor.get(b"FontName").ok() != font.get(b"BaseFont").ok()
        || descriptor.has(b"FontFile2")
    {
        return Err(INVALID.into());
    }
    Ok(descriptor)
}

pub(super) fn embedded(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let descriptor = descriptor(doc, font)?;
    if descriptor.has(b"FontFile3") {
        return Err(INVALID.into());
    }
    let stream = crate::encoding::resolve(doc, descriptor.get(b"FontFile").map_err(|_| INVALID)?)
        .as_stream()
        .map_err(|_| INVALID)?;
    let length = |key: &[u8]| -> Result<usize, String> {
        match stream.dict.get(key) {
            Ok(value) => {
                let value = crate::encoding::resolve(doc, value)
                    .as_i64()
                    .map_err(|_| INVALID)?;
                usize::try_from(value).map_err(|_| INVALID.into())
            }
            Err(_) if key == b"Length3" => Ok(0),
            Err(_) => Err(INVALID.into()),
        }
    };
    let (length1, length2, length3) = (
        length(b"Length1")?,
        length(b"Length2")?,
        length(b"Length3")?,
    );
    let bytes = filters::decode(stream, super::super::MAX_CONTENT)?;
    let program = program::parse(&bytes, length1, length2, length3)?;
    // The same embedding permissions as an OpenType OS/2 fsType, when present.
    if program.rights.is_some_and(|rights| rights & !0x108 != 0) {
        return Err("embedded font does not permit this editable use".into());
    }
    measured(doc, font, &program)
}

// A Type1C program read as a Type 1 one (`cff::named`): glyph names, their
// advances and outlines, and the built-in encoding a PDF encoding starts from.
pub(super) fn compact(doc: &Document, font: &Dictionary) -> Result<Metrics, String> {
    let descriptor = descriptor(doc, font)?;
    if descriptor.has(b"FontFile") {
        return Err(INVALID.into());
    }
    let stream = crate::encoding::resolve(doc, descriptor.get(b"FontFile3").map_err(|_| INVALID)?)
        .as_stream()
        .map_err(|_| INVALID)?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1C") {
        return Err(INVALID.into());
    }
    let bytes = filters::decode(stream, super::super::MAX_CONTENT)?;
    measured(doc, font, &super::cff::named(&bytes)?)
}

fn measured(
    doc: &Document,
    font: &Dictionary,
    program: &program::Program,
) -> Result<Metrics, String> {
    let names = names(doc, font, &program.encoding)?;
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
    let unicode = match font.get(b"ToUnicode") {
        Ok(mapping) => {
            let stream = crate::encoding::resolve(doc, mapping)
                .as_stream()
                .map_err(|_| INVALID)?;
            Some(super::mapping::parse_names(stream)?)
        }
        Err(_) => None,
    };
    let mut result = Box::new([None; 256]);
    let mut slot_names: [Option<&[u8]>; 256] = [None; 256];
    let mut codes = Box::new([None; 256]);
    let mut vertical_bounds = [0_f64; 2];
    let mut overhangs = Box::new([[0_f64; 2]; 256]);
    let mut opaque = Box::new([None; 256]);
    for code in 0..256 {
        let Some(name) = names[code].as_deref() else {
            continue;
        };
        // A code outside the widths, with a zero width, or naming a glyph the
        // subset does not carry cannot be shown, so nothing below applies to it.
        // pdfTeX writes zero widths for the encoding's unused codes.
        if (code as i64) < first || (code as i64) > last {
            continue;
        }
        let Some(glyph) = program.glyphs.get(name) else {
            continue;
        };
        let width = number(&widths[code - first as usize])?;
        if width == 0. {
            continue;
        }
        // A present map is the text a reader extracts. A code is offered only
        // where it agrees with the glyph name; without a map the ligatures carry
        // no evidence of their text. pdfTeX maps the whole TeX encoding, so
        // codes the Differences left to the built-in encoding routinely
        // disagree without ever being shown.
        let offered = slot(name).filter(|&slot| match &unicode {
            Some(unicode) => unicode[code] == Some(Some(slot)),
            None => super::ligatures::text(slot).is_none(),
        });
        if !(0. ..=2000.).contains(&width) {
            continue;
        }
        // A reader positions every glyph by the PDF width. An offered glyph is
        // also measured for replacements, so the program has to agree with it;
        // one that does not is kept read-only at the PDF width, as TeX math
        // italic widths (which carry an italic correction) routinely are.
        let offered = offered.filter(|_| (width - glyph.width).abs() <= 1.);
        if let Some(slot) = offered {
            let fits = match glyph.bounds {
                Some([left, bottom, right, top])
                    if left >= -250.
                        && right <= width + 250.
                        && bottom >= -250.
                        && top <= 1000. =>
                {
                    vertical_bounds[0] = vertical_bounds[0].min(bottom);
                    vertical_bounds[1] = vertical_bounds[1].max(top);
                    overhangs[slot as usize] = [left.min(0.), (right - width).max(0.)];
                    true
                }
                None => slot == b' ',
                _ => false,
            };
            if fits {
                // Two codes may name one glyph (T1 encoding has two hyphens).
                // Two glyphs for one character would make the choice arbitrary.
                match slot_names[slot as usize] {
                    Some(existing) if existing != name => {
                        return Err("ambiguous duplicate Type 1 glyph encoding".into())
                    }
                    _ => {}
                }
                slot_names[slot as usize] = Some(name);
                result[slot as usize] = Some(width);
                codes[code] = Some(slot);
                continue;
            }
        }
        // Validated but not writable: measures read-only text. It reserves ink
        // rather than bounding a replacement, so taller excursions (large
        // delimiters, accents) are accepted up to four ems: TeX's extension
        // font hangs its largest parentheses 2.4 em below the baseline.
        match glyph.bounds {
            Some([left, bottom, right, top])
                if left >= -OPAQUE_REACH
                    && right <= width + OPAQUE_REACH
                    && bottom >= -OPAQUE_REACH
                    && top <= OPAQUE_REACH =>
            {
                vertical_bounds[0] = vertical_bounds[0].min(bottom);
                vertical_bounds[1] = vertical_bounds[1].max(top);
                opaque[code] = Some(super::Opaque {
                    width,
                    overhang: [left.min(0.), (right - width).max(0.)],
                });
            }
            None => {
                opaque[code] = Some(super::Opaque {
                    width,
                    overhang: [0.; 2],
                })
            }
            _ => {}
        }
    }
    Ok(Metrics {
        opaque: Some(opaque),
        unicode: None,
        widths: result,
        codes: Some(Codes::Single(codes)),
        vertical_bounds: Some(vertical_bounds),
        horizontal_overhangs: Some(overhangs),
    })
}
