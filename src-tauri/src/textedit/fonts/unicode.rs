//! Validated font glyphs with Unicode semantics, including CJK and symbols.
use std::collections::{BTreeMap, BTreeSet};
use ttf_parser::{Face, GlyphId};

// One glyph as a font program states it, in thousandths of an em: which
// glyph it is, its advance, its outline's control-point box, whether it draws
// nothing, and whether it may only be measured (a shown `.notdef`, which no
// text names).
pub(super) struct Glyph {
    pub id: u16,
    pub advance: f64,
    pub ink: Option<[f64; 4]>,
    pub empty: bool,
    pub read_only: bool,
}

pub(super) struct Metrics {
    codes: BTreeMap<u16, String>,
    glyphs: BTreeMap<u16, (f64, [f64; 2])>,
    reverse: BTreeMap<String, u16>,
    // Texts that more than one offered glyph shows. They read, but a
    // replacement cannot choose between the glyphs, so none is written.
    ambiguous: BTreeSet<String>,
    // What each offered code draws: its glyph and its PDF width. Two codes
    // with the same identity are the same choice (the fallback fonts give
    // every character occurrence its own code).
    identities: BTreeMap<u16, (u16, u64)>,
    // Codes measured for read-only text only; they read as `OPAQUE`.
    opaque: BTreeSet<u16>,
    single_byte: bool,
}

// Text to code for the offered glyphs, without the texts that codes drawing
// different glyphs (or one glyph at different widths) share.
fn reverse(
    codes: &BTreeMap<u16, String>,
    identities: &BTreeMap<u16, (u16, u64)>,
) -> (BTreeMap<String, u16>, BTreeSet<String>) {
    let mut reverse: BTreeMap<String, u16> = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (&code, text) in codes {
        let Some(identity) = identities.get(&code) else {
            continue;
        };
        match reverse.get(text) {
            Some(first) if identities.get(first) != Some(identity) => {
                ambiguous.insert(text.clone());
            }
            Some(_) => {}
            None => {
                reverse.insert(text.clone(), code);
            }
        }
    }
    for text in &ambiguous {
        reverse.remove(text);
    }
    (reverse, ambiguous)
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
        Self::from_glyphs(codes, default, widths, |code| {
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
            let advance = f64::from(
                face.glyph_hor_advance(glyph)
                    .ok_or("missing glyph advance")?,
            ) * unit;
            let ink = super::outlines::bounds(face, glyph).map(|ink| ink.map(|value| value * unit));
            Ok(Some(Glyph {
                id: glyph_index,
                advance,
                ink,
                empty: ink.is_none() && super::empty_glyph(face, glyph) == Some(true),
                read_only: false,
            }))
        })
    }

    // Any program's glyphs, in thousandths of an em. `glyph` answers `None`
    // for a glyph that exists but cannot be measured: it is not offered.
    pub(super) fn from_glyphs(
        codes: BTreeMap<u16, String>,
        default: f64,
        widths: &BTreeMap<u16, f64>,
        mut glyph: impl FnMut(u16) -> Result<Option<Glyph>, String>,
    ) -> Result<(Self, [f64; 2]), String> {
        let mut glyphs = BTreeMap::new();
        let mut identities = BTreeMap::new();
        let mut opaque = BTreeSet::new();
        let mut vertical = [0_f64; 2];
        let reach = super::type1::OPAQUE_REACH;
        for (&code, text) in &codes {
            let Some(Glyph {
                id,
                advance,
                ink,
                empty,
                read_only,
            }) = glyph(code)?
            else {
                continue;
            };
            let width = widths.get(&code).copied().unwrap_or(default);
            if width < 0. {
                return Err("composite font widths disagree with its glyph metrics".into());
            }
            // A zero width is a combining mark set over the glyph before it
            // (Typst's macron, under `DW 0`): measured, never written.
            let agrees = !read_only && width > 0. && (width - advance).abs() <= 1.;
            let bounds = match ink {
                Some([left, bottom, right, top])
                    if agrees
                        && left >= -1000.
                        && right <= width + 1000.
                        && bottom >= -1000.
                        && top <= 2000. =>
                {
                    vertical[0] = vertical[0].min(bottom);
                    vertical[1] = vertical[1].max(top);
                    [left.min(0.), (right - width).max(0.)]
                }
                None if agrees && matches!(text.as_str(), " " | "\u{a0}" | "\u{3000}") && empty => {
                    [0.; 2]
                }
                // A reader positions every glyph by its PDF width. One whose
                // program disagrees (LuaTeX writes TeX's italic correction into
                // math widths) is measured there and kept read-only with its
                // ink reserved, as `type1` keeps such a glyph. A glyph that
                // agrees but reaches too far is left out, as it always was;
                // no document has needed it kept read-only.
                Some([left, bottom, right, top])
                    if !agrees
                        && left >= -reach
                        && right <= width + reach
                        && bottom >= -reach
                        && top <= reach =>
                {
                    vertical[0] = vertical[0].min(bottom);
                    vertical[1] = vertical[1].max(top);
                    opaque.insert(code);
                    [left.min(0.), (right - width).max(0.)]
                }
                None if !agrees && empty => {
                    opaque.insert(code);
                    [0.; 2]
                }
                _ => continue,
            };
            glyphs.insert(code, (width, bounds));
            if !opaque.contains(&code) {
                identities.insert(code, (id, width.to_bits()));
            }
        }
        let (reverse, ambiguous) = reverse(&codes, &identities);
        Ok((
            Self {
                codes,
                glyphs,
                reverse,
                ambiguous,
                identities,
                opaque,
                single_byte: false,
            },
            vertical,
        ))
    }

    // A replacement writes a text several glyphs share with the glyph its run
    // already shows for it, when the run shows exactly one: a small-caps word
    // keeps its small capitals. Other shared texts stay unwritable.
    pub(super) fn prefer(&mut self, shown: &[u8]) {
        let size = self.code_len();
        let mut chosen: BTreeMap<String, Option<u16>> = BTreeMap::new();
        for chunk in shown.chunks_exact(size) {
            let code = match chunk {
                [byte] => u16::from(*byte),
                pair => u16::from_be_bytes([pair[0], pair[1]]),
            };
            let Some(text) = self.codes.get(&code) else {
                continue;
            };
            let Some(identity) = self.identities.get(&code) else {
                continue;
            };
            if self.ambiguous.contains(text) {
                let choice = chosen.entry(text.clone()).or_insert(Some(code));
                if choice.is_some_and(|first| self.identities.get(&first) != Some(identity)) {
                    *choice = None;
                }
            }
        }
        for (text, code) in chosen {
            if let Some(code) = code {
                self.ambiguous.remove(&text);
                self.reverse.insert(text, code);
            }
        }
    }

    // Type3 validates its PDF glyph programs instead of a TrueType face.
    pub(super) fn single(
        codes: BTreeMap<u16, String>,
        glyphs: BTreeMap<u16, (f64, [f64; 2])>,
    ) -> Self {
        // Each Type3 code has its own glyph program.
        let identities = codes.keys().map(|&code| (code, (code, 0))).collect();
        let (reverse, ambiguous) = reverse(&codes, &identities);
        Self {
            codes,
            glyphs,
            reverse,
            ambiguous,
            identities,
            opaque: BTreeSet::new(),
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
                .ok_or_else(|| {
                    let first = rest.chars().next().map(String::from).unwrap_or_default();
                    if self.ambiguous.contains(&first) {
                        "the font has several glyphs for this character and cannot choose one"
                    } else {
                        "the font has no validated glyph for this character"
                    }
                })?;
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

    pub(super) fn code_len(&self) -> usize {
        if self.single_byte {
            1
        } else {
            2
        }
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
            let mapped = self.codes.get(code).ok_or("unmapped font code")?;
            if self.opaque.contains(code) {
                text.push(super::OPAQUE);
            } else {
                text.push_str(mapped);
            }
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
            // Only a read-only mark may stand still; writable text advances.
            if step < 0. || (step == 0. && !self.opaque.contains(code)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Codes 1 and 2 read "S"; `glyphs` names what each draws and at what width.
    fn metrics(glyphs: [(u16, f64); 3]) -> Metrics {
        let codes: BTreeMap<u16, String> = [(1, "S"), (2, "S"), (3, "A")]
            .into_iter()
            .map(|(code, text)| (code, text.to_string()))
            .collect();
        let widths = (1..=3).zip(glyphs.map(|(_, width)| width)).collect();
        Metrics::from_glyphs(codes, 600., &widths, |code| {
            let (id, width) = glyphs[usize::from(code) - 1];
            Ok(Some(Glyph {
                id,
                advance: width,
                ink: Some([0., 0., 400., 700.]),
                empty: false,
                read_only: false,
            }))
        })
        .unwrap()
        .0
    }

    // The fallback fonts give each character occurrence a code of its own, so
    // one glyph under two codes is one choice; two glyphs, or one glyph at two
    // widths, are not.
    #[test]
    fn shared_texts_are_ambiguous_only_between_different_glyphs() {
        let same = metrics([(7, 600.), (7, 600.), (8, 600.)]);
        assert_eq!(same.encode("SA").unwrap(), [0, 1, 0, 3]);
        for glyphs in [
            [(7, 600.), (9, 600.), (8, 600.)],
            [(7, 600.), (7, 500.), (8, 600.)],
        ] {
            assert_eq!(
                metrics(glyphs).encode("SA").unwrap_err(),
                "the font has several glyphs for this character and cannot choose one"
            );
        }
    }
}
