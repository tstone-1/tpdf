//! Strict single-byte ToUnicode subset for symbolic TrueType text. This is a
//! data grammar, never a PostScript interpreter. Reject every extra operation.

use lopdf::{content::Content, Object, Stream};

#[cfg(test)]
mod tests;

// Standard wrapper emitted by the measured LibreOffice export. Only bfchar
// blocks vary. Other wrappers, ranges, inheritance and multi-character mappings
// remain unsupported until their semantics have independent readback evidence.
const PREFIX: &[u8] = br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange";
const SUFFIX: &[u8] = b"endcmap CMapName currentdict /CMap defineresource pop end end";
const MAX_MAP: usize = 16 * 1024;

pub(super) fn parse(stream: &Stream) -> Result<Box<[Option<u8>; 256]>, String> {
    if stream.dict.has(b"UseCMap") {
        return Err("inherited character maps are not editable yet".into());
    }
    let invalid = || "unsupported or ambiguous single-byte character map".to_string();
    let bytes = super::filters::decode(stream, MAX_MAP)?;
    let content = Content::decode_strict(&bytes).map_err(|_| invalid())?;
    let prefix = Content::decode_strict(PREFIX).map_err(|_| invalid())?;
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
    let mut result = Box::new([None; 256]);
    let mut unicode = [false; 128];
    for block in ops[start..end].chunks_exact(2) {
        let [Object::Integer(count)] = block[0].operands.as_slice() else {
            return Err(invalid());
        };
        if block[0].operator != "beginbfchar"
            || !(1..=100).contains(count)
            || block[1].operator != "endbfchar"
            || block[1].operands.len() != *count as usize * 2
        {
            return Err(invalid());
        }
        for pair in block[1].operands.chunks_exact(2) {
            let (Object::String(code, _), Object::String(text, _)) = (&pair[0], &pair[1]) else {
                return Err(invalid());
            };
            let ([code], [0, ch]) = (code.as_slice(), text.as_slice()) else {
                return Err(invalid());
            };
            if !(32..=126).contains(ch) || result[*code as usize].is_some() || unicode[*ch as usize]
            {
                return Err(invalid());
            }
            result[*code as usize] = Some(*ch);
            unicode[*ch as usize] = true;
        }
    }
    Ok(result)
}
