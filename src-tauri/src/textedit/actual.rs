//! Bounded ActualText spans: edit identical logical/glyph text together, otherwise
//! keep the complete span read-only. Never infer a mapping from alternate words.
use super::{Inspection, MAX_TEXT};
use lopdf::Object;

const INVALID: &str = "unsupported ActualText marked-content sequence";

pub(super) struct Span {
    pub operator: usize,
    logical: String,
    shows: Vec<u32>,
    visible: String,
    bounded_ink: bool,
}

impl Span {
    pub(super) fn new(tag: &Object, properties: &Object, operator: usize) -> Result<Self, String> {
        let dict = properties.as_dict().map_err(|_| INVALID)?;
        if tag.as_name().ok() != Some(b"Span") || dict.len() != 1 {
            return Err(INVALID.into());
        }
        let value = dict.get(b"ActualText").map_err(|_| INVALID)?;
        let bytes = value.as_str().map_err(|_| INVALID)?;
        if bytes.len() > MAX_TEXT * 4 + 3 {
            return Err(INVALID.into());
        }
        let logical = if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
            if bytes.len() % 2 != 0 {
                return Err(INVALID.into());
            }
            let units: Vec<_> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| INVALID)?
        } else if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
            std::str::from_utf8(bytes).map_err(|_| INVALID)?.to_owned()
        } else {
            lopdf::decode_text_string(value).map_err(|_| INVALID)?
        };
        if logical.chars().count() > MAX_TEXT
            || logical
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{fffd}' | '\u{2028}' | '\u{2029}'))
        {
            return Err(INVALID.into());
        }
        Ok(Self {
            operator,
            logical,
            shows: Vec::new(),
            visible: String::new(),
            bounded_ink: true,
        })
    }

    pub(super) fn show(&mut self, index: u32, text: &str, bounded_ink: bool) -> Result<(), String> {
        if self.shows.len() >= 128 || self.visible.chars().count() + text.chars().count() > MAX_TEXT
        {
            return Err(INVALID.into());
        }
        self.shows.push(index);
        self.visible.push_str(text);
        self.bounded_ink &= bounded_ink || text.is_empty();
        Ok(())
    }

    pub(super) fn finish(self, page: &mut Inspection) -> Result<(), String> {
        if self.shows.is_empty() {
            return Err(INVALID.into());
        }
        let nonempty: Vec<_> = page
            .runs
            .runs
            .iter()
            .chain(&page.preserved)
            .filter(|run| self.shows.contains(&run.operator) && !run.text.is_empty())
            .map(|run| run.operator)
            .collect();
        // Layout restoration emits an empty TJ to restore the source cursor.
        // It is not a second logical text fragment and must never be editable.
        let candidate = match nonempty.as_slice() {
            [] => Some(self.shows[0]),
            [operator] => Some(*operator),
            _ => None,
        }
        .filter(|_| self.visible == self.logical);
        if candidate.is_none() && !self.bounded_ink {
            return Err("read-only ActualText requires validated glyph outlines".into());
        }
        if let Some(operator) = candidate {
            page.actual_text.insert(operator, self.operator);
        }
        page.runs.runs.retain(|run| {
            if self.shows.contains(&run.operator) && Some(run.operator) != candidate {
                page.preserved.push(run.clone());
                false
            } else {
                true
            }
        });
        Ok(())
    }
}

pub(super) fn patch(page: &mut Inspection, operator: u32, replacement: &str) -> Result<(), String> {
    if let Some(&index) = page.actual_text.get(&operator) {
        let dict = page.content.operations[index].operands[1]
            .as_dict_mut()
            .map_err(|_| INVALID)?;
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend(replacement.encode_utf16().flat_map(u16::to_be_bytes));
        dict.set("ActualText", Object::string_literal(bytes));
        page.patched.insert(index);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
