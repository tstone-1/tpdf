//! Preserve bounded inline ActualText separators without offering them for editing.
use lopdf::Object;

const INVALID: &str = "unsupported inline spacing sequence";

pub(super) struct Spacer {
    count: usize,
    positioned: bool,
    shown: bool,
}

impl Spacer {
    pub(super) fn new(tag: &Object, properties: &Object) -> Result<Self, String> {
        let invalid = "inline BDC marked content is not editable yet";
        let dict = properties.as_dict().map_err(|_| invalid)?;
        if tag.as_name().ok() != Some(b"Span") || dict.len() != 1 {
            return Err(invalid.into());
        }
        let bytes = dict
            .get(b"ActualText")
            .and_then(Object::as_str)
            .map_err(|_| invalid)?;
        let count = if let Some(units) = bytes.strip_prefix(&[0xfe, 0xff]) {
            if units.len() % 2 != 0
                || !units
                    .chunks_exact(2)
                    .all(|unit| unit[0] == 0 && matches!(unit[1], 7 | 9))
            {
                return Err(invalid.into());
            }
            units.len() / 2
        } else {
            if !bytes.iter().all(|byte| matches!(byte, 7 | 9)) {
                return Err(invalid.into());
            }
            bytes.len()
        };
        if !(1..=32).contains(&count) {
            return Err(invalid.into());
        }
        Ok(Self {
            count,
            positioned: false,
            shown: false,
        })
    }

    // One optional position and one show; no nesting, state changes, or crossing ET.
    // The ordinary inspector still validates coordinates, glyphs, advances and clips.
    pub(super) fn step(&mut self, operator: &str) -> Result<(), String> {
        match operator {
            "Tm" | "Td" if !self.positioned && !self.shown => self.positioned = true,
            "Tj" | "TJ" if !self.shown => self.shown = true,
            "EMC" if self.shown => {}
            _ => return Err(INVALID.into()),
        }
        Ok(())
    }

    // One space per tab, as Word writes it, or fewer: InDesign shows a single
    // space for a run of tabs and places it with the matrix. Either way the
    // show is spacing only, which is what keeps it out of the editable text.
    // In a font the editor keeps read-only (`fonts::read_only`) the glyph
    // cannot be read, and needs not be: nothing in that font is editable.
    pub(super) fn text(&self, text: &str) -> Result<(), String> {
        if !(1..=self.count).contains(&text.chars().count())
            || !text
                .chars()
                .all(|character| character == ' ' || character == super::fonts::OPAQUE)
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
