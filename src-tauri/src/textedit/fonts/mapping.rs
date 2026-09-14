//! Strict single-byte ToUnicode subset for symbolic TrueType text. This is a
//! data grammar, never a PostScript interpreter. Reject every extra operation.

use lopdf::{
    content::{Content, Operation},
    Object, Stream,
};

#[cfg(test)]
mod tests;

// Standard wrapper emitted by the measured LibreOffice and Quartz exports.
// Only bfchar and scalar bfrange blocks vary. Other wrappers, array targets,
// inheritance and multi-character mappings remain unsupported.
const PREFIX: &[u8] = br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange";
const SUFFIX: &[u8] = b"endcmap CMapName currentdict /CMap defineresource pop end end";
const MAX_MAP: usize = 16 * 1024;

fn blocks(stream: &Stream, wide: bool) -> Result<Vec<Operation>, String> {
    if stream.dict.has(b"UseCMap") {
        return Err("inherited character maps are not editable yet".into());
    }
    let invalid = || "unsupported or ambiguous character map".to_string();
    let bytes = super::filters::decode(stream, MAX_MAP)?;
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
    let ops = &content.operations;
    let start = prefix.operations.len();
    let end = ops
        .len()
        .checked_sub(suffix.operations.len())
        .ok_or_else(invalid)?;
    if end <= start || (end - start) % 2 != 0 {
        return Err(invalid());
    }
    for (actual, expected) in ops[..start]
        .iter()
        .zip(&prefix.operations)
        .chain(ops[end..].iter().zip(&suffix.operations))
    {
        if actual.operator != expected.operator || actual.operands != expected.operands {
            return Err(invalid());
        }
    }
    Ok(ops[start..end].to_vec())
}

pub(super) fn parse(stream: &Stream) -> Result<Box<[Option<u8>; 256]>, String> {
    let ops = blocks(stream, false)?;
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
            let [high, low] = text.as_slice() else {
                return Err(invalid());
            };
            let target = u16::from_be_bytes([*high, *low]);
            let end_target = u32::from(target) + u32::from(last.saturating_sub(first));
            // A source range has at most 256 entries. Validate Unicode before
            // narrowing to metric slots; a range cannot wrap into allowed text.
            let allowed = |ch| (32..=126).contains(&ch) || ch == 0x2013;
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

// Identity-H codes and UTF-16BE targets are exactly two bytes. A range expands
// only into unique printable Latin-1 or en dash, at most 192 distinct characters.
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
            let target = word(&entry[stride - 1])?;
            let last_target = u32::from(target) + u32::from(last.saturating_sub(first));
            let slot = |ch| char::from_u32(ch).and_then(super::super::character_slot);
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
