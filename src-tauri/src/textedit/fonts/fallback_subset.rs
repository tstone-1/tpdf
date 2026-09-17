//! Subset trusted bundled fonts only; preserve embedding rights on reopen.
//! subsetter intentionally drops OS/2. Restore the original table, rather
//! than relaxing the editor's validation for arbitrary document fonts.
use std::collections::BTreeMap;
use ttf_parser::Tag;

pub(super) fn build(source: &[u8], codes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut mapper = subsetter::GlyphRemapper::new();
    for pair in codes.chunks_exact(2) {
        mapper.remap(u16::from_be_bytes([pair[0], pair[1]]));
    }
    let subset = subsetter::subset(source, 0, &mapper)
        .map_err(|_| "Could not subset the bundled editing font")?;
    let face = super::face(source, false)?;
    let rights = face
        .raw_face()
        .table(Tag::from_bytes(b"OS/2"))
        .ok_or("Missing bundled font embedding rights")?;
    let program = restore_rights(&subset, rights)?;
    if program.len() > crate::textedit::MAX_CONTENT {
        return Err("The replacement needs too many font outlines; shorten the text".into());
    }
    super::face(&program, false)?;
    let glyphs = codes
        .chunks_exact(2)
        .map(|pair| {
            mapper
                .get(u16::from_be_bytes([pair[0], pair[1]]))
                .ok_or("Missing subset glyph")
                .map(u16::to_be_bytes)
        })
        .collect::<Result<Vec<_>, _>>()?
        .concat();
    Ok((program, glyphs))
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn restore_rights(subset: &[u8], rights: &[u8]) -> Result<Vec<u8>, String> {
    let invalid = "Invalid bundled font subset";
    let face = ttf_parser::RawFace::parse(subset, 0).map_err(|_| invalid)?;
    let mut tables = BTreeMap::new();
    for record in face.table_records {
        tables.insert(
            record.tag.to_bytes(),
            face.table(record.tag).ok_or(invalid)?.to_vec(),
        );
    }
    tables.insert(*b"OS/2", rights.to_vec());
    tables
        .get_mut(b"head")
        .and_then(|head| head.get_mut(8..12))
        .ok_or(invalid)?
        .fill(0);
    let count = u16::try_from(tables.len()).map_err(|_| invalid)?;
    if count == 0 || count > 64 {
        return Err(invalid.into());
    }
    let power = count.ilog2() as u16;
    let search = (1u16 << power) * 16;
    let mut result = vec![0, 1, 0, 0];
    for value in [count, search, power, count * 16 - search] {
        result.extend(value.to_be_bytes());
    }
    let mut offset = 12 + tables.len() * 16;
    let mut head_at = 0;
    for (tag, bytes) in &tables {
        result.extend(tag);
        result.extend(checksum(bytes).to_be_bytes());
        result.extend((offset as u32).to_be_bytes());
        result.extend((bytes.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = offset + 8;
        }
        offset += (bytes.len() + 3) & !3;
    }
    for bytes in tables.values() {
        result.extend(bytes);
        while result.len() % 4 != 0 {
            result.push(0);
        }
    }
    let adjustment = 0xB1B0AFBAu32.wrapping_sub(checksum(&result));
    result[head_at..head_at + 4].copy_from_slice(&adjustment.to_be_bytes());
    Ok(result)
}
