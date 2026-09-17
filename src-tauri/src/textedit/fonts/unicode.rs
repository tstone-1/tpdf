//! Validated font glyphs with Unicode semantics, including CJK and symbols.
use std::collections::BTreeMap;
use ttf_parser::{Face, GlyphId};

pub(super) struct Metrics {
    codes: BTreeMap<u16, String>,
    glyphs: BTreeMap<u16, (f64, [f64; 2])>,
    reverse: BTreeMap<String, u16>,
    single_byte: bool,
}

impl Metrics {
    pub(super) fn new(
        face: &Face<'_>,
        codes: BTreeMap<u16, String>,
        default: f64,
        widths: &BTreeMap<u16, f64>,
    ) -> Result<(Self, [f64; 2]), String> {
        Self::with_glyphs(face, codes, default, widths, None)
    }

    pub(super) fn with_glyphs(
        face: &Face<'_>,
        codes: BTreeMap<u16, String>,
        default: f64,
        widths: &BTreeMap<u16, f64>,
        mapping: Option<&[u8]>,
    ) -> Result<(Self, [f64; 2]), String> {
        let unit = 1000. / f64::from(face.units_per_em());
        let mut glyphs = BTreeMap::new();
        let mut vertical = [0_f64; 2];
        for (&code, text) in &codes {
            let glyph_index = if let Some(mapping) = mapping {
                let at = usize::from(code) * 2;
                let pair = mapping
                    .get(at..at + 2)
                    .ok_or("missing composite glyph mapping")?;
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                code
            };
            let glyph = GlyphId(glyph_index);
            if glyph_index == 0 || glyph_index >= face.number_of_glyphs() {
                return Err("invalid composite glyph index".into());
            }
            let width = widths.get(&code).copied().unwrap_or(default);
            let advance = f64::from(
                face.glyph_hor_advance(glyph)
                    .ok_or("missing glyph advance")?,
            ) * unit;
            if width <= 0. || (width - advance).abs() > 1. {
                return Err("composite font widths disagree with its glyph metrics".into());
            }
            let bounds = match super::outlines::bounds(face, glyph) {
                Some([left, bottom, right, top])
                    if left * unit >= -1000.
                        && right * unit <= width + 1000.
                        && bottom * unit >= -1000.
                        && top * unit <= 2000. =>
                {
                    vertical[0] = vertical[0].min(bottom * unit);
                    vertical[1] = vertical[1].max(top * unit);
                    [(left * unit).min(0.), (right * unit - width).max(0.)]
                }
                None if matches!(text.as_str(), " " | "\u{a0}")
                    && super::empty_glyph(face, glyph) == Some(true) =>
                {
                    [0.; 2]
                }
                _ => continue,
            };
            glyphs.insert(code, (width, bounds));
        }
        let reverse = codes
            .iter()
            .filter(|(code, _)| glyphs.contains_key(code))
            .map(|(code, text)| (text.clone(), *code))
            .collect();
        Ok((
            Self {
                codes,
                glyphs,
                reverse,
                single_byte: false,
            },
            vertical,
        ))
    }

    // Type3 validates its PDF glyph programs instead of a TrueType face.
    pub(super) fn single(
        codes: BTreeMap<u16, String>,
        glyphs: BTreeMap<u16, (f64, [f64; 2])>,
    ) -> Self {
        let reverse = codes
            .iter()
            .map(|(code, text)| (text.clone(), *code))
            .collect();
        Self {
            codes,
            glyphs,
            reverse,
            single_byte: true,
        }
    }

    fn slots(&self, text: &str) -> Result<Vec<u16>, String> {
        if text.chars().count() > crate::textedit::MAX_TEXT || text.chars().any(char::is_control) {
            return Err("invalid replacement text".into());
        }
        let mut rest = text;
        let mut result = Vec::new();
        while !rest.is_empty() {
            let (end, code) = rest
                .char_indices()
                .take(3)
                .map(|(index, ch)| index + ch.len_utf8())
                .filter_map(|end| self.reverse.get(&rest[..end]).map(|code| (end, *code)))
                .last()
                .ok_or("the font has no validated glyph for this character")?;
            result.push(code);
            rest = &rest[end..];
        }
        Ok(result)
    }

    pub(super) fn encode(&self, text: &str) -> Result<Vec<u8>, String> {
        if self.single_byte {
            return self
                .slots(text)?
                .into_iter()
                .map(|code| u8::try_from(code).map_err(|_| "invalid single-byte glyph code".into()))
                .collect();
        }
        Ok(self
            .slots(text)?
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect())
    }

    pub(super) fn source(
        &self,
        bytes: &[u8],
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(String, f64, [f64; 2]), String> {
        let code_size = if self.single_byte { 1 } else { 2 };
        if bytes.len() % code_size != 0 || bytes.len() / code_size > crate::textedit::MAX_TEXT {
            return Err("invalid or oversized encoded text".into());
        }
        let slots: Vec<_> = bytes
            .chunks_exact(code_size)
            .map(|p| {
                if self.single_byte {
                    u16::from(p[0])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        let mut text = String::new();
        for code in &slots {
            text.push_str(self.codes.get(code).ok_or("unmapped font code")?);
        }
        if text.chars().count() > crate::textedit::MAX_TEXT {
            return Err("expanded text exceeds its limit".into());
        }
        let (advance, bounds) = self.layout(&slots, size, spacing, word_spacing)?;
        Ok((text, advance, bounds))
    }

    pub(super) fn replacement(
        &self,
        text: &str,
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        self.layout(&self.slots(text)?, size, spacing, word_spacing)
    }

    fn layout(
        &self,
        slots: &[u16],
        size: f64,
        spacing: f64,
        word_spacing: f64,
    ) -> Result<(f64, [f64; 2]), String> {
        if !spacing.is_finite()
            || spacing.abs() > size * 0.25
            || !word_spacing.is_finite()
            || word_spacing < -size * 0.25
            || word_spacing > 1_000_000.
        {
            return Err("text spacing exceeds editable limits".into());
        }
        let mut cursor = 0_f64;
        let mut bounds = [0_f64; 2];
        for code in slots {
            let (width, [left, right]) = self
                .glyphs
                .get(code)
                .ok_or("the font has no validated glyph for this character")?;
            let step = width * size / 1000.
                + spacing
                + if self.single_byte && *code == 32 {
                    word_spacing
                } else {
                    0.
                };
            if step <= 0. {
                return Err("backtracking character spacing is not editable yet".into());
            }
            bounds[0] = bounds[0].min(cursor + left * size / 1000.);
            bounds[1] = bounds[1].max(cursor + (width + right) * size / 1000.);
            cursor += step;
            if !cursor.is_finite() || cursor > 1_000_000. {
                return Err("spaced text advance exceeds its limit".into());
            }
        }
        Ok((cursor, bounds))
    }
}
