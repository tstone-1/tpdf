//! Locate already-validated operators without rounding untouched numeric tokens.
//! The scanner only supplies boundaries: lopdf must agree on each whole operation.
use super::{Content, MAX_CONTENT};
use std::{collections::BTreeSet, ops::Range};

fn delimiter(byte: u8) -> bool {
    matches!(
        byte,
        0 | 9 | 10 | 12 | 13 | 32 | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'/' | b'%'
    )
}

fn skip(bytes: &[u8], pos: &mut usize) {
    loop {
        while bytes
            .get(*pos)
            .is_some_and(|b| matches!(b, 0 | 9 | 10 | 12 | 13 | 32))
        {
            *pos += 1;
        }
        if bytes.get(*pos) != Some(&b'%') {
            break;
        }
        while bytes.get(*pos).is_some_and(|b| !matches!(b, b'\r' | b'\n')) {
            *pos += 1;
        }
    }
}

// Iteration is bounded by the content limit; recursive containers by 32 levels.
fn token(bytes: &[u8], pos: &mut usize, depth: usize) -> Result<bool, String> {
    let invalid = || "cannot preserve text stream tokens".to_string();
    if depth > 32 {
        return Err(invalid());
    }
    let start = *pos;
    let first = *bytes.get(*pos).ok_or_else(invalid)?;
    *pos += 1;
    match first {
        b'(' => {
            let mut nesting = 1;
            while let Some(&byte) = bytes.get(*pos) {
                *pos += 1;
                match byte {
                    b'\\' => {
                        if *pos < bytes.len() {
                            *pos += 1;
                        }
                    }
                    b'(' => {
                        nesting += 1;
                        if nesting > 32 {
                            return Err(invalid());
                        }
                    }
                    b')' => {
                        nesting -= 1;
                        if nesting == 0 {
                            return Ok(false);
                        }
                    }
                    _ => {}
                }
            }
            Err(invalid())
        }
        b'[' | b'<' if first == b'[' || bytes.get(*pos) == Some(&b'<') => {
            let dictionary = first == b'<';
            if dictionary {
                *pos += 1;
            }
            loop {
                skip(bytes, pos);
                if dictionary && bytes.get(*pos..*pos + 2) == Some(b">>") {
                    *pos += 2;
                    return Ok(false);
                }
                if !dictionary && bytes.get(*pos) == Some(&b']') {
                    *pos += 1;
                    return Ok(false);
                }
                if token(bytes, pos, depth + 1)? {
                    return Err(invalid());
                }
            }
        }
        b'<' => {
            while let Some(&byte) = bytes.get(*pos) {
                *pos += 1;
                if byte == b'>' {
                    return Ok(false);
                }
            }
            Err(invalid())
        }
        b')' | b'>' | b']' | b'%' => Err(invalid()),
        _ => {
            while bytes.get(*pos).is_some_and(|&b| !delimiter(b)) {
                *pos += 1;
            }
            let word = &bytes[start..*pos];
            Ok(first != b'/'
                && !matches!(first, b'0'..=b'9' | b'+' | b'-' | b'.')
                && !matches!(word, b"true" | b"false" | b"null"))
        }
    }
}

pub(super) fn rewrite(
    bytes: &[u8],
    changed: &Content,
    edits: &BTreeSet<usize>,
) -> Result<Vec<u8>, String> {
    rewrite_expanded(bytes, changed, edits, &std::collections::BTreeMap::new())
}

pub(super) fn rewrite_expanded(
    bytes: &[u8],
    changed: &Content,
    edits: &BTreeSet<usize>,
    expansions: &std::collections::BTreeMap<usize, Vec<lopdf::content::Operation>>,
) -> Result<Vec<u8>, String> {
    if changed.operations.len() + expansions.values().map(Vec::len).sum::<usize>()
        > super::MAX_OPERATIONS
    {
        return Err("edited page exceeds the text operator limit".into());
    }
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut pos = 0;
    let mut start = None;
    while pos < bytes.len() {
        skip(bytes, &mut pos);
        if pos == bytes.len() {
            break;
        }
        start.get_or_insert(pos);
        if token(bytes, &mut pos, 0)? {
            spans.push(start.take().ok_or("missing operator start")?..pos);
            if spans.len() > changed.operations.len() {
                return Err("text operator boundaries disagree".into());
            }
        }
    }
    if start.is_some() || spans.len() != changed.operations.len() {
        return Err("text operator boundaries disagree".into());
    }
    let mut output = Vec::new();
    let mut copied = 0;
    for (index, span) in spans.into_iter().enumerate() {
        let original = Content::decode_strict(&bytes[span.clone()]).map_err(|e| e.to_string())?;
        let next = &changed.operations[index];
        if original.operations.len() != 1 {
            return Err("text operator boundaries disagree".into());
        }
        let same_operator = original.operations[0].operator == next.operator;
        let compensated_show = edits.contains(&index)
            && original.operations[0].operator == "Tj"
            && next.operator == "TJ";
        if !same_operator && !compensated_show {
            return Err("text operator boundaries disagree".into());
        }
        if edits.contains(&index) {
            let actual_text = next.operator == "BDC"
                && next.operands.len() == 2
                && original.operations[0].operands.len() == 2
                && !expansions.contains_key(&index)
                && super::actual::Span::new(
                    &original.operations[0].operands[0],
                    &original.operations[0].operands[1],
                    index,
                )
                .is_ok()
                && super::actual::Span::new(&next.operands[0], &next.operands[1], index).is_ok();
            // A painted path moved with the line it underlines
            // (`wrap::translated`): its first operator after a saved state and
            // a translation, or its last before the restore, both unchanged.
            let moved_path = original.operations[0].operands == next.operands
                && expansions.get(&index).is_some_and(|operations| {
                    let kept = |op: &lopdf::content::Operation| {
                        op.operator == next.operator && op.operands == next.operands
                    };
                    match operations.as_slice() {
                        [save, translate, path] => {
                            save.operator == "q" && translate.operator == "cm" && kept(path)
                        }
                        [path, restore] => restore.operator == "Q" && kept(path),
                        _ => false,
                    }
                });
            if !actual_text
                && !moved_path
                && (!matches!(next.operator.as_str(), "Tj" | "TJ") || next.operands.len() != 1)
            {
                return Err("invalid text patch".into());
            }
            output.extend_from_slice(&bytes[copied..span.start]);
            let encoded = Content {
                operations: expansions
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| vec![next.clone()]),
            }
            .encode()
            .map_err(|e| e.to_string())?;
            // A show's operand may follow the operator before it with no
            // space (`0 Td[(A)]TJ`, as xdvipdfmx writes), which is only a
            // boundary because the operand opens with a delimiter. What
            // replaces it may open with a number -- a moved show's `Tm` --
            // and would then run into that operator.
            if output.last().is_some_and(|&byte| !delimiter(byte))
                && encoded.first().is_some_and(|&byte| !delimiter(byte))
            {
                output.push(b'\n');
            }
            output.extend(encoded);
            copied = span.end;
        } else if original.operations[0].operands != next.operands {
            return Err("untouched text operator changed".into());
        }
    }
    output.extend_from_slice(&bytes[copied..]);
    if output.len() > MAX_CONTENT {
        return Err("text page content exceeds its limit".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
