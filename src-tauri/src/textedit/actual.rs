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
        let logical = logical(dict.get(b"ActualText").map_err(|_| INVALID)?)?;
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

/// An ActualText string as text, bounded like every run: UTF-16 or UTF-8 with
/// their byte-order marks, PDFDocEncoding otherwise, and no control character.
pub(super) fn logical(value: &Object) -> Result<String, String> {
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
    Ok(logical)
}

fn utf16(text: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    Object::string_literal(bytes)
}

/// How a Span's ActualText differs from the text its one run paints, when it
/// differs by nothing but spaces at either end: the ActualText's and the run's
/// counts of leading and trailing U+0020, around a core that is the same.
/// PowerPoint writes the two equal, or with the word's space in only one of
/// them (`docs/TEXTEDIT.md`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Edges {
    actual: [usize; 2],
    painted: [usize; 2],
}

fn edges(text: &str) -> (&str, [usize; 2]) {
    let core = text.trim_matches(' ');
    let lead = text.len() - text.trim_start_matches(' ').len();
    let trail = text.len() - text.trim_end_matches(' ').len();
    (core, [lead, trail])
}

impl Edges {
    pub(super) fn of(actual: &str, painted: &str) -> Option<Self> {
        let (core, actual) = edges(actual);
        let (shown, painted) = edges(painted);
        // Both empty is a match too: a deleted run paints nothing and its
        // Span says nothing, and the run is still there to type into.
        (core == shown).then_some(Self { actual, painted })
    }

    /// The ActualText for a replacement of the run: the replacement, with
    /// each end's spaces moved by the difference the source had there. All
    /// spaces, or nothing, is one end.
    pub(super) fn apply(self, replacement: &str) -> String {
        let shift = |count: usize, side: usize| {
            (count + self.actual[side]).saturating_sub(self.painted[side])
        };
        let (core, [lead, trail]) = edges(replacement);
        if core.is_empty() {
            let count = replacement.len() + self.actual[0] + self.actual[1];
            return " ".repeat(count.saturating_sub(self.painted[0] + self.painted[1]));
        }
        format!(
            "{}{core}{}",
            " ".repeat(shift(lead, 0)),
            " ".repeat(shift(trail, 1))
        )
    }
}

/// A structure Span whose ActualText is its run's text, as `Edges` allows.
#[derive(Clone, Copy, Debug)]
pub(super) struct Element {
    pub span: lopdf::ObjectId,
    pub edges: Edges,
}

/// The structure Spans a batch rewrites, each with its new ActualText.
pub(super) fn rewrite(page: &mut Inspection, operator: u32, replacement: &str) {
    if let Some(element) = page.structure_actual.get(&operator).copied() {
        page.span_texts
            .insert(element.span, utf16(&element.edges.apply(replacement)));
    }
}

pub(super) fn patch(page: &mut Inspection, operator: u32, replacement: &str) -> Result<(), String> {
    if let Some(&index) = page.actual_text.get(&operator) {
        let dict = page.content.operations[index].operands[1]
            .as_dict_mut()
            .map_err(|_| INVALID)?;
        dict.set("ActualText", utf16(replacement));
        page.patched.insert(index);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
