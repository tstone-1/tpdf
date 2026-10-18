//! The bundled Noto fonts for text tpdf adds to a page rather than edits:
//! page numbers and watermarks (`addedtext.rs`) whose characters Helvetica
//! with `/WinAnsiEncoding` cannot draw.
//!
//! The same writer the text editor embeds a fallback font with
//! ([`fallback::Font`]): a Type0 font, `/Identity-H`, a `/ToUnicode` map so the
//! words extract as they were typed, and the program always a subset.
//!
//! **The font is chosen per character, not per line.** Noto Sans has Latin,
//! Greek and Cyrillic and no ideographs; Noto Sans CJK SC has the ideographs
//! and lacks accented Greek (`ά`, `ί`), so `Σελίδα 1 · 第1页` fits neither
//! alone. Each character is set in Noto Sans when it has a glyph there and in
//! the CJK font otherwise, and a line switches font where its characters do.
//! Neither font shapes text, and neither has Arabic, Hebrew, the Indic scripts
//! or Thai, so text in those is refused by [`AddedFont::covers`] rather than
//! drawn as the wrong glyphs.

use lopdf::{Document, ObjectId};
use ttf_parser::Face;

use super::fallback::{self, Font};
use super::Metrics;

/// Noto Sans, then Noto Sans CJK SC: `fallback::data`'s styles 0 and 4, in
/// the order a character is offered to them.
const STYLES: [u8; 2] = [0, 4];

/// One part of a line: the font it is set in, an index into [`STYLES`], and
/// its codes as the hex string a `Tj` shows.
pub(crate) struct Piece {
    pub(crate) font: usize,
    pub(crate) hex: String,
}

/// One subset per bundled font a write needs, each holding the characters
/// set in it.
pub(crate) struct AddedFont {
    parts: [Option<(Font, Metrics)>; 2],
    cap_height: f64,
    descent: f64,
}

/// The faces of [`STYLES`], parsed.
fn faces() -> Result<[Face<'static>; 2], String> {
    Ok([
        super::face(fallback::data(STYLES[0]), false)?,
        super::face(fallback::data(STYLES[1]), false)?,
    ])
}

fn has(face: &Face<'_>, ch: char) -> bool {
    face.glyph_index(ch).is_some_and(|g| g.0 != 0)
}

/// Which of [`STYLES`] sets `ch`, if either has it.
fn font_for(faces: &[Face<'_>; 2], ch: char) -> Option<usize> {
    faces.iter().position(|face| has(face, ch))
}

impl AddedFont {
    /// Whether every character of `text`, line breaks aside, is in one of the
    /// bundled fonts.
    pub(crate) fn covers(text: &str) -> bool {
        faces().is_ok_and(|faces| {
            text.chars()
                .filter(|ch| *ch != '\n')
                .all(|ch| font_for(&faces, ch).is_some())
        })
    }

    /// The subsets holding every character of `texts`.
    ///
    /// # Errors
    ///
    /// A character neither bundled font has, more distinct characters than the
    /// editor takes, or a subset that does not read back.
    pub(crate) fn for_texts(texts: &[&str]) -> Result<AddedFont, String> {
        let faces = faces()?;
        let mut chars: [String; 2] = [String::new(), String::new()];
        for ch in texts.iter().flat_map(|text| text.chars()) {
            let at = font_for(&faces, ch).ok_or_else(|| {
                format!("no bundled font has a glyph for U+{:04X}", u32::from(ch))
            })?;
            if !chars[at].contains(ch) {
                chars[at].push(ch);
            }
        }
        let mut parts = [None, None];
        for (at, wanted) in chars.iter().enumerate() {
            if !wanted.is_empty() {
                parts[at] = Some(Font::subset(STYLES[at], wanted)?);
            }
        }
        // Placed by Noto Sans's measures when it sets any of the text, since
        // its capitals are the ones a centred line is centred on.
        let face = &faces[usize::from(parts[0].is_none())];
        let em = f64::from(face.units_per_em());
        Ok(AddedFont {
            parts,
            cap_height: face
                .capital_height()
                .map_or(0.714, |height| f64::from(height) / em),
            descent: -f64::from(face.descender()) / em,
        })
    }

    /// `text` as pieces, one per change of font.
    ///
    /// # Errors
    ///
    /// A character the subsets were not made with.
    pub(crate) fn pieces(&self, text: &str) -> Result<Vec<Piece>, String> {
        let faces = faces()?;
        let mut runs: Vec<(usize, String)> = Vec::new();
        for ch in text.chars() {
            let at = font_for(&faces, ch).ok_or("a character no bundled font has")?;
            match runs.last_mut() {
                Some((font, run)) if *font == at => run.push(ch),
                _ => runs.push((at, ch.to_string())),
            }
        }
        runs.into_iter()
            .map(|(font, run)| {
                let (_, metrics) = self.parts[font]
                    .as_ref()
                    .ok_or("a line's character is in no subset made for the write")?;
                let hex = metrics
                    .encode(&run)?
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect();
                Ok(Piece { font, hex })
            })
            .collect()
    }

    /// The width of `text` set at `size` points, in points.
    ///
    /// # Errors
    ///
    /// A character the subsets were not made with.
    pub(crate) fn advance(&self, text: &str, size: f64) -> Result<f64, String> {
        let faces = faces()?;
        let mut width = 0.0;
        for ch in text.chars() {
            let at = font_for(&faces, ch).ok_or("a character no bundled font has")?;
            let (_, metrics) = self.parts[at]
                .as_ref()
                .ok_or("a line's character is in no subset made for the write")?;
            width += metrics
                .spaced_layout(ch.encode_utf8(&mut [0; 4]), size, 0.0, 0.0)?
                .0;
        }
        Ok(width)
    }

    /// Adds each subset to `doc` and returns its font dictionary, by the
    /// index [`Piece::font`] uses; `None` for a font no text is set in.
    ///
    /// # Errors
    ///
    /// A font program that does not read back.
    pub(crate) fn install(&self, doc: &mut Document) -> Result<[Option<ObjectId>; 2], String> {
        let mut programs = fallback::Programs::new();
        let mut ids = [None, None];
        for (at, part) in self.parts.iter().enumerate() {
            if let Some((font, _)) = part {
                ids[at] = Some(font.install(doc, &mut programs)?);
            }
        }
        Ok(ids)
    }

    /// The capital height as a fraction of the size.
    pub(crate) fn cap_height(&self) -> f64 {
        self.cap_height
    }

    /// The descender, positive, as a fraction of the size.
    pub(crate) fn descent(&self) -> f64 {
        self.descent
    }
}
