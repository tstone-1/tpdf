//! Bundled OFL fonts, embedded once per style. No system-font or filesystem access.
//! Sources and byte digests are recorded in vendor/fonts/manifest.json.
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::collections::BTreeMap;

pub(in crate::textedit) struct Font {
    pub style: u8,
    codes: BTreeMap<u16, String>,
    widths: BTreeMap<u16, f64>,
    glyphs: Vec<u8>,
}

pub(in crate::textedit) fn data(style: u8) -> &'static [u8] {
    match style {
        1 => include_bytes!("../../../../vendor/fonts/NotoSans-Bold.ttf"),
        2 => include_bytes!("../../../../vendor/fonts/NotoSans-Italic.ttf"),
        3 => include_bytes!("../../../../vendor/fonts/NotoSans-BoldItalic.ttf"),
        _ => include_bytes!("../../../../vendor/fonts/NotoSans-Regular.ttf"),
    }
}

pub(in crate::textedit) fn label(style: u8) -> &'static str {
    match style {
        1 => "Noto Sans Bold",
        2 => "Noto Sans Italic",
        3 => "Noto Sans Bold Italic",
        _ => "Noto Sans",
    }
}

impl Font {
    pub(in crate::textedit) fn new(
        style: u8,
        text: &str,
    ) -> Result<(Self, super::Metrics), String> {
        let face = super::face(data(style), false)?;
        let characters: std::collections::BTreeSet<_> =
            text.chars().filter(|ch| *ch != '\n').collect();
        if characters.len() > crate::textedit::MAX_TEXT {
            return Err("too many replacement characters".into());
        }
        let mut font = Self {
            style,
            codes: BTreeMap::new(),
            widths: BTreeMap::new(),
            glyphs: vec![0, 0],
        };
        for (index, ch) in characters.into_iter().enumerate() {
            let glyph = face.glyph_index(ch).filter(|id| id.0 != 0)
                .ok_or("Noto Sans does not contain a required character; choose the original font or another replacement")?;
            let code = (index + 1) as u16;
            let width = f64::from(
                face.glyph_hor_advance(glyph)
                    .ok_or("missing fallback glyph width")?,
            ) * 1000.
                / f64::from(face.units_per_em());
            font.codes.insert(code, ch.to_string());
            font.widths.insert(code, width);
            font.glyphs.extend(glyph.0.to_be_bytes());
        }
        let (unicode, vertical) = super::unicode::Metrics::with_glyphs(
            &face,
            font.codes.clone(),
            1000.,
            &font.widths,
            Some(&font.glyphs),
        )?;
        let metrics = super::Metrics {
            unicode: Some(unicode),
            vertical_bounds: Some(vertical),
            widths: Box::new([None; 256]),
            horizontal_overhangs: None,
            codes: None,
        };
        // Measurement and encoding must both work before any PDF object is added.
        for line in text.split('\n') {
            metrics.spaced_layout(line, 12., 0., 0.)?;
        }
        Ok((font, metrics))
    }

    pub(in crate::textedit) fn install(
        &self,
        doc: &mut Document,
        programs: &mut BTreeMap<u8, ObjectId>,
    ) -> Result<ObjectId, String> {
        let program = *programs.entry(self.style).or_insert_with(|| {
            doc.add_object(Stream::new(
                dictionary! { "Length1" => data(self.style).len() as i64 },
                data(self.style).to_vec(),
            ))
        });
        let name = label(self.style).replace(' ', "");
        let face = super::face(data(self.style), false)?;
        let scale = 1000. / f64::from(face.units_per_em());
        let bounds = face.global_bounding_box();
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => name.as_str(), "Flags" => 4,
            "FontBBox" => vec![Object::Real((f64::from(bounds.x_min)*scale) as f32),
                Object::Real((f64::from(bounds.y_min)*scale) as f32), Object::Real((f64::from(bounds.x_max)*scale) as f32),
                Object::Real((f64::from(bounds.y_max)*scale) as f32)],
            "ItalicAngle" => if self.style >= 2 { -12 } else { 0 },
            "Ascent" => (f64::from(face.ascender())*scale) as i64,
            "Descent" => (f64::from(face.descender())*scale) as i64,
            "CapHeight" => 714, "StemV" => 80, "FontFile2" => program
        });
        let glyphs = doc.add_object(Stream::new(Dictionary::new(), self.glyphs.clone()));
        let widths = self
            .widths
            .iter()
            .flat_map(|(code, width)| {
                [
                    Object::Integer(i64::from(*code)),
                    Object::Array(vec![Object::Real(*width as f32)]),
                ]
            })
            .collect::<Vec<_>>();
        let child = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => name.as_str(),
            "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
            "FontDescriptor" => descriptor, "CIDToGIDMap" => glyphs, "DW" => 1000, "W" => widths
        });
        let mut map = String::from("/CIDInit /ProcSet findresource begin\n12 dict begin begincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n");
        let entries: Vec<_> = self.codes.iter().collect();
        for block in entries.chunks(100) {
            map.push_str(&format!("{} beginbfchar\n", block.len()));
            for (code, text) in block {
                let target = text
                    .encode_utf16()
                    .map(|unit| format!("{unit:04X}"))
                    .collect::<String>();
                map.push_str(&format!("<{code:04X}> <{target}>\n"));
            }
            map.push_str("endbfchar\n");
        }
        map.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
        let map = doc.add_object(Stream::new(Dictionary::new(), map.into_bytes()));
        Ok(doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type0", "BaseFont" => name.as_str(),
            "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(child)], "ToUnicode" => map }))
    }
}
