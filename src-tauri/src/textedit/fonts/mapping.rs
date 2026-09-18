//! Strict bounded ToUnicode subset for symbolic TrueType text. This is a
//! data grammar, never a PostScript interpreter. Reject every extra operation.

use lopdf::{
    content::{Content, Operation},
    Object, Stream,
};

#[cfg(test)]
mod tests;

// Standard wrapper emitted by the measured LibreOffice and Quartz exports.
// Only bfchar and scalar bfrange blocks vary. Other wrappers, array targets,
// inheritance and general multi-character mappings remain unsupported. CFF and
// Identity-H admit only the exact sequences enumerated in ligatures.rs.
const PREFIX: &[u8] = br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange";
const SUFFIX: &[u8] = b"endcmap CMapName currentdict /CMap defineresource pop end end";
const MAX_MAP: usize = 128 * 1024;

// The general Identity-H path retains Unicode scalars instead of narrowing
// them into the legacy Latin-1 metric slots. Glyph validation remains separate.
pub(super) fn unicode_cid(
    stream: &Stream,
) -> Result<std::collections::BTreeMap<u16, String>, String> {
    unicode_codes(stream, true)
}

pub(super) fn unicode_single(
    stream: &Stream,
) -> Result<std::collections::BTreeMap<u16, String>, String> {
    unicode_codes(stream, false)
}

fn unicode_codes(
    stream: &Stream,
    wide: bool,
) -> Result<std::collections::BTreeMap<u16, String>, String> {
    let invalid = || "unsupported or ambiguous Unicode character map".to_string();
    let word = |object: &Object| -> Result<u16, String> {
        let bytes = object.as_str().map_err(|_| invalid())?;
        if !wide {
            let [byte] = bytes else {
                return Err(invalid());
            };
            return Ok(u16::from(*byte));
        }
        let [a, b] = bytes else {
            return Err(invalid());
        };
        Ok(u16::from_be_bytes([*a, *b]))
    };
    let mut result = std::collections::BTreeMap::new();
    for block in blocks(stream, wide)?.chunks_exact(2) {
        let [Object::Integer(count)] = block[0].operands.as_slice() else {
            return Err(invalid());
        };
        let stride = match (block[0].operator.as_str(), block[1].operator.as_str()) {
            ("beginbfchar", "endbfchar") => 2,
            ("beginbfrange", "endbfrange") => 3,
            _ => return Err(invalid()),
        };
        if !(1..=100).contains(count) || block[1].operands.len() != *count as usize * stride {
            return Err(invalid());
        }
        for entry in block[1].operands.chunks_exact(stride) {
            let first = word(&entry[0])?;
            let last = if stride == 3 { word(&entry[1])? } else { first };
            if last < first || result.len() + usize::from(last - first) + 1 > 4096 {
                return Err(invalid());
            }
            let bytes = entry[stride - 1].as_str().map_err(|_| invalid())?;
            if bytes.is_empty() || bytes.len() % 2 != 0 || bytes.len() > 6 {
                return Err(invalid());
            }
            let units = bytes
                .chunks_exact(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect::<Vec<_>>();
            for code in first..=last {
                let text = if stride == 3 {
                    let [start] = units.as_slice() else {
                        return Err(invalid());
                    };
                    let target = u32::from(*start) + u32::from(code - first);
                    if target > 0xffff {
                        return Err(invalid());
                    }
                    char::from_u32(target).ok_or_else(invalid)?.to_string()
                } else {
                    String::from_utf16(&units).map_err(|_| invalid())?
                };
                // A glyph may stand for up to three letters: a ligature (ff,
                // fi, and Calibri's ft, st, Th). The encoder matches the
                // longest mapped sequence. Several glyphs may share a text (a
                // small capital and its capital, a delimiter's sizes); each
                // reads as that text and none is written for it.
                if (text.chars().count() != 1 && !text.chars().all(char::is_alphabetic))
                    || text.chars().any(char::is_control)
                    || result.insert(code, text).is_some()
                {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(result)
}

// The CMap's own name and CIDSystemInfo label the map and change no entry;
// ConTeXt names them after the font (`/Registry (TeX)`), like pdfTeX.
fn blocks(stream: &Stream, wide: bool) -> Result<Vec<Operation>, String> {
    blocks_with_header(stream, wide, false, true)
}

// The CMap's own name, type, version, writing mode and CIDSystemInfo label
// it; they map no code. pdfTeX and dvipdfm name the map after the TeX
// encoding, xdvipdfmx writes the three standard ones in its own order, and
// Typst builds the system info as a PostScript dictionary (`3 dict dup begin
// ... end def`). Each may appear once, with the shape the CMap specification
// (Adobe TN 5099) gives it. Horizontal writing only.
fn labels(mut ops: &[Operation]) -> bool {
    let short = |object: &Object| object.as_str().is_ok_and(|text| text.len() <= 127);
    let info = |info: &lopdf::Dictionary| {
        info.len() == 3
            && info.get(b"Registry").is_ok_and(short)
            && info.get(b"Ordering").is_ok_and(short)
            && info
                .get(b"Supplement")
                .is_ok_and(|value| value.as_i64().is_ok_and(|n| (0..=1000).contains(&n)))
    };
    let mut seen: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
    while let [op, tail @ ..] = ops {
        let (key, valid) = match (op.operator.as_str(), op.operands.as_slice()) {
            ("def", [Object::Name(key), value]) => {
                ops = tail;
                let valid = match (key.as_slice(), value) {
                    (b"CMapName", Object::Name(name)) => name.len() <= 127,
                    (b"CMapType", Object::Integer(kind)) => (0..=2).contains(kind),
                    (b"CMapVersion", Object::Integer(_) | Object::Real(_)) => true,
                    (b"WMode", Object::Integer(0)) => true,
                    (b"CIDSystemInfo", Object::Dictionary(dict)) => info(dict),
                    _ => false,
                };
                (key, valid)
            }
            ("dict", [Object::Name(key), Object::Integer(1..=8)]) if key == b"CIDSystemInfo" => {
                let [dup, begin, entries @ .., end, def] = tail.get(..7).unwrap_or_default() else {
                    return false;
                };
                ops = &tail[7..];
                let mut dict = lopdf::Dictionary::new();
                for entry in entries {
                    match (entry.operator.as_str(), entry.operands.as_slice()) {
                        // A repeated key leaves fewer than the three `info` needs.
                        ("def", [Object::Name(name), value]) => {
                            dict.set(name.clone(), value.clone())
                        }
                        _ => return false,
                    }
                }
                let bare =
                    |op: &Operation, name: &str| op.operator == name && op.operands.is_empty();
                let valid = bare(dup, "dup")
                    && bare(begin, "begin")
                    && bare(end, "end")
                    && bare(def, "def")
                    && info(&dict);
                (key, valid)
            }
            _ => return false,
        };
        if !valid || !seen.insert(key.clone()) {
            return false;
        }
    }
    true
}

fn blocks_with_header(
    stream: &Stream,
    wide: bool,
    padded_single: bool,
    labelled: bool,
) -> Result<Vec<Operation>, String> {
    if stream.dict.has(b"UseCMap") {
        return Err("inherited character maps are not editable yet".into());
    }
    let invalid = || "unsupported or ambiguous character map".to_string();
    let mut bytes = super::filters::decode(stream, MAX_MAP)?;
    // Typst ends its map with a `%%EOF` comment and no end of line, which
    // lopdf's content parser refuses; the end of the data ends a comment too.
    bytes.push(b'\n');
    let content = Content::decode_strict(&bytes).map_err(|_| invalid())?;
    let prefix_bytes = if wide {
        String::from_utf8_lossy(PREFIX)
            .replace("<00> <FF>", "<0000> <FFFF>")
            .into_bytes()
    } else {
        PREFIX.to_vec()
    };
    let prefix = Content::decode_strict(&prefix_bytes).map_err(|_| invalid())?;
    let suffix = Content::decode_strict(SUFFIX).map_err(|_| invalid())?;
    let mut ops = content.operations;
    let mut prefix = prefix.operations;
    if labelled {
        // Compare the fixed operators around the labels, not the labels.
        let begin = ops.iter().position(|op| op.operator == "begincmap");
        let end = ops
            .iter()
            .position(|op| op.operator == "begincodespacerange");
        let (Some(begin), Some(end)) = (begin, end) else {
            return Err(invalid());
        };
        if end <= begin || !labels(&ops[begin + 1..end]) {
            return Err(invalid());
        }
        ops.drain(begin + 1..end);
        prefix.retain(|op| op.operator != "def");
    }
    let start = prefix.len();
    let end = ops
        .len()
        .checked_sub(suffix.operations.len())
        .ok_or_else(invalid)?;
    if end <= start || (end - start) % 2 != 0 {
        return Err(invalid());
    }
    for (actual, expected) in ops[..start]
        .iter()
        .zip(&prefix)
        .chain(ops[end..].iter().zip(&suffix.operations))
    {
        // Some simple-font exports declare a two-byte code space while every
        // source entry remains one byte. Admit only that exact header variant;
        // parsing below still validates all source lengths, targets and counts.
        let padded_range = padded_single
            && expected.operator == "endcodespacerange"
            && matches!(actual.operands.as_slice(), [Object::String(low, _), Object::String(high, _)]
                if low == &[0, 0] && high == &[255, 255]);
        // PostScript's dict operand is an allocation hint, not part of the
        // character mapping. Producers use several initial capacities.
        let dictionary_capacity = expected.operator == "dict"
            && matches!(actual.operands.as_slice(), [Object::Integer(size)] if (1..=256).contains(size));
        if actual.operator != expected.operator
            || (actual.operands != expected.operands && !padded_range && !dictionary_capacity)
        {
            return Err(invalid());
        }
    }
    Ok(ops[start..end].to_vec())
}

pub(super) fn parse(stream: &Stream) -> Result<Box<[Option<u8>; 256]>, String> {
    parse_single(stream, false, false, false)
}

// CFF glyph-name agreement is checked by the caller. Other font paths keep
// their independently verified repertoire and do not inherit these additions.
pub(super) fn parse_cff(stream: &Stream) -> Result<Box<[Option<u8>; 256]>, String> {
    parse_single(stream, true, true, false)
}

pub(super) fn parse_named(stream: &Stream) -> Result<Box<[Option<u8>; 256]>, String> {
    let codes = parse_single(stream, false, true, true)?;
    // A WinAnsi font's ToUnicode may only restate WinAnsi itself: each code's
    // slot is its own code. 0xA0 and 0xAD are the space and hyphen aliases,
    // whose Unicode meaning is ambiguous, and 0x80 is the minus slot.
    if codes.iter().enumerate().any(|(code, slot)| {
        slot.is_some_and(|slot| {
            usize::from(slot) != code || matches!(code, 0x80 | 0xA0 | 0xAD) || code < 32
        })
    }) {
        return Err("named TrueType ToUnicode disagrees with its ASCII encoding".into());
    }
    Ok(codes)
}

fn parse_single(
    stream: &Stream,
    cff: bool,
    padded: bool,
    winansi: bool,
) -> Result<Box<[Option<u8>; 256]>, String> {
    let ops = blocks_with_header(stream, false, padded, false)?;
    let invalid = || "unsupported or ambiguous single-byte character map".to_string();
    let mut result = Box::new([None; 256]);
    let mut unicode = [false; 256];
    for block in ops.chunks_exact(2) {
        let [Object::Integer(count)] = block[0].operands.as_slice() else {
            return Err(invalid());
        };
        let stride = match (block[0].operator.as_str(), block[1].operator.as_str()) {
            ("beginbfchar", "endbfchar") => 2,
            ("beginbfrange", "endbfrange") => 3,
            _ => return Err(invalid()),
        };
        if !(1..=100).contains(count) || block[1].operands.len() != *count as usize * stride {
            return Err(invalid());
        }
        let byte = |object: &Object| -> Result<u8, String> {
            match object {
                Object::String(bytes, _) if bytes.len() == 1 => Ok(bytes[0]),
                _ => Err(invalid()),
            }
        };
        for entry in block[1].operands.chunks_exact(stride) {
            let first = byte(&entry[0])?;
            let last = if stride == 3 { byte(&entry[1])? } else { first };
            let Object::String(text, _) = &entry[stride - 1] else {
                return Err(invalid());
            };
            if cff && stride == 2 && text.len() > 2 {
                let slot = super::ligatures::target(text).ok_or_else(invalid)?;
                if result[first as usize].is_some() || unicode[slot as usize] {
                    return Err(invalid());
                }
                result[first as usize] = Some(slot);
                unicode[slot as usize] = true;
                continue;
            }
            let [high, low] = text.as_slice() else {
                return Err(invalid());
            };
            let target = u16::from_be_bytes([*high, *low]);
            let end_target = u32::from(target) + u32::from(last.saturating_sub(first));
            // A source range has at most 256 entries. Validate Unicode before
            // narrowing to metric slots; a range cannot wrap into allowed text.
            let allowed = |ch| {
                (32..=126).contains(&ch)
                    || ch == 0x2013
                    || (cff && matches!(ch, 0x00a0 | 0x00a3 | 0x2018 | 0x2019 | 0x2212))
                    || (winansi
                        && ch != 0x2212
                        && char::from_u32(ch)
                            .and_then(super::super::character_slot)
                            .is_some())
            };
            if last < first || !(u32::from(target)..=end_target).all(allowed) {
                return Err(invalid());
            }
            for code in first..=last {
                let ch = super::super::character_slot(
                    char::from_u32(u32::from(target) + u32::from(code - first))
                        .ok_or_else(invalid)?,
                )
                .ok_or_else(invalid)?;
                if result[code as usize].is_some() || unicode[ch as usize] {
                    return Err(invalid());
                }
                result[code as usize] = Some(ch);
                unicode[ch as usize] = true;
            }
        }
    }
    Ok(result)
}

// Single-byte Type 1 maps as pdfTeX writes them: the whole TeX encoding, much
// of it outside the editor's repertoire. Each code is None when unmapped,
// Some(None) when its text cannot be offered, and Some(Some(slot)) otherwise.
// The caller requires agreement with the glyph names; nothing here is trusted
// to choose a glyph.
pub(super) fn parse_names(stream: &Stream) -> Result<Box<[Option<Option<u8>>; 256]>, String> {
    let invalid = || "unsupported or ambiguous single-byte character map".to_string();
    let mut result = Box::new([None; 256]);
    for block in blocks_with_header(stream, false, true, true)?.chunks_exact(2) {
        let [Object::Integer(count)] = block[0].operands.as_slice() else {
            return Err(invalid());
        };
        let stride = match (block[0].operator.as_str(), block[1].operator.as_str()) {
            ("beginbfchar", "endbfchar") => 2,
            ("beginbfrange", "endbfrange") => 3,
            _ => return Err(invalid()),
        };
        if !(1..=100).contains(count) || *count as usize * stride != block[1].operands.len() {
            return Err(invalid());
        }
        let byte = |object: &Object| -> Result<u8, String> {
            match object {
                Object::String(bytes, _) if bytes.len() == 1 => Ok(bytes[0]),
                _ => Err(invalid()),
            }
        };
        for entry in block[1].operands.chunks_exact(stride) {
            let first = byte(&entry[0])?;
            let last = if stride == 3 { byte(&entry[1])? } else { first };
            let Object::String(bytes, _) = &entry[stride - 1] else {
                return Err(invalid());
            };
            if last < first || bytes.is_empty() || bytes.len() % 2 != 0 || bytes.len() > 16 {
                return Err(invalid());
            }
            let units = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            for code in first..=last {
                let offset = u16::from(code - first);
                let text = if stride == 3 {
                    // A range increments its target's last unit (TN 5099 1.4).
                    let [start] = units.as_slice() else {
                        return Err(invalid());
                    };
                    let unit = start.checked_add(offset).ok_or_else(invalid)?;
                    String::from_utf16(&[unit]).map_err(|_| invalid())?
                } else {
                    String::from_utf16(&units).map_err(|_| invalid())?
                };
                let mut chars = text.chars();
                let slot = match (chars.next(), chars.next()) {
                    // U+2212 takes the internal minus slot; the caller's glyph
                    // name must say `minus` for it to be offered. Control
                    // characters have no slot.
                    (Some(ch), None) => super::super::character_slot(ch)
                        .filter(|&slot| !matches!(slot, 0xA0 | 0xAD)),
                    _ => super::ligatures::GLYPHS
                        .iter()
                        .find(|(_, sequence, _)| *sequence == text)
                        .map(|(_, _, slot)| *slot),
                };
                if result[code as usize].replace(slot).is_some() {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(result)
}

// Identity-H codes and scalar UTF-16BE targets are two bytes. A range expands
// only into unique printable Latin-1 or en dash. Individual bfchar entries may
// also name the four exact ligature sequences; ranges cannot expand sequences.
pub(super) fn parse_cid(stream: &Stream) -> Result<std::collections::BTreeMap<u16, u8>, String> {
    let invalid = || "unsupported or ambiguous two-byte character map".to_string();
    let word = |object: &Object| -> Result<u16, String> {
        let Object::String(bytes, _) = object else {
            return Err(invalid());
        };
        let [a, b] = bytes.as_slice() else {
            return Err(invalid());
        };
        Ok(u16::from_be_bytes([*a, *b]))
    };
    let mut result = std::collections::BTreeMap::new();
    let mut unicode = std::collections::BTreeSet::new();
    for block in blocks(stream, true)?.chunks_exact(2) {
        let [Object::Integer(count)] = block[0].operands.as_slice() else {
            return Err(invalid());
        };
        let stride = match (block[0].operator.as_str(), block[1].operator.as_str()) {
            ("beginbfchar", "endbfchar") => 2,
            ("beginbfrange", "endbfrange") => 3,
            _ => return Err(invalid()),
        };
        if !(1..=100).contains(count) || block[1].operands.len() != *count as usize * stride {
            return Err(invalid());
        }
        for entry in block[1].operands.chunks_exact(stride) {
            let first = word(&entry[0])?;
            let last = if stride == 3 { word(&entry[1])? } else { first };
            if stride == 2 {
                let bytes = entry[1].as_str().map_err(|_| invalid())?;
                if bytes.len() > 2 {
                    let slot = super::ligatures::target(bytes).ok_or_else(invalid)?;
                    if result.insert(first, slot).is_some() || !unicode.insert(slot) {
                        return Err(invalid());
                    }
                    continue;
                }
            }
            let target = word(&entry[stride - 1])?;
            let last_target = u32::from(target) + u32::from(last.saturating_sub(first));
            // Two-byte fonts keep their proven Latin-1 and en dash repertoire.
            let slot = |ch| {
                let target = char::from_u32(ch).and_then(super::super::character_slot);
                target.filter(|_| ch <= 0xff || ch == 0x2013)
            };
            if last < first || !(u32::from(target)..=last_target).all(|ch| slot(ch).is_some()) {
                return Err(invalid());
            }
            for code in first..=last {
                let ch = slot(u32::from(target) + u32::from(code - first)).ok_or_else(invalid)?;
                if result.insert(code, ch).is_some() || !unicode.insert(ch) {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(result)
}
