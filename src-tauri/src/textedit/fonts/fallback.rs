//! Bundled OFL fonts, embedded once per style. No system-font or filesystem access.
//! Sources and byte digests are recorded in vendor/fonts/manifest.json.
//!
//! The same writer embeds a subset of an installed copy of a document's font
//! (`installed.rs`), which reaches the worker as bytes in the request: this
//! module still opens no file.
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::collections::BTreeMap;

pub(in crate::textedit) struct Font {
    pub style: u8,
    /// The PostScript name of an installed font this program is a subset of;
    /// `None` for the bundled Noto fonts.
    installed: Option<String>,
    codes: BTreeMap<u16, String>,
    widths: BTreeMap<u16, f64>,
    glyphs: Vec<u8>,
    subset: Option<Vec<u8>>,
    /// The bare CID-keyed CFF embedded for an installed font with CFF
    /// outlines, in place of `subset`, which stays what the journal keeps.
    compact: Option<Vec<u8>>,
    program_key: Vec<u8>,
}

pub(in crate::textedit) type Programs = BTreeMap<(u8, Vec<u8>), ObjectId>;

pub(in crate::textedit) fn data(style: u8) -> &'static [u8] {
    match style {
        4 => include_bytes!("../../../../vendor/fonts/NotoSansCJKsc-Regular.ttf"),
        5 => include_bytes!("../../../../vendor/fonts/NotoSansCJKsc-Bold.ttf"),
        1 => include_bytes!("../../../../vendor/fonts/NotoSans-Bold.ttf"),
        2 => include_bytes!("../../../../vendor/fonts/NotoSans-Italic.ttf"),
        3 => include_bytes!("../../../../vendor/fonts/NotoSans-BoldItalic.ttf"),
        _ => include_bytes!("../../../../vendor/fonts/NotoSans-Regular.ttf"),
    }
}

pub(in crate::textedit) fn label(style: u8) -> &'static str {
    match style {
        4 => "Noto Sans CJK SC",
        5 => "Noto Sans CJK SC Bold",
        1 => "Noto Sans Bold",
        2 => "Noto Sans Italic",
        3 => "Noto Sans Bold Italic",
        _ => "Noto Sans",
    }
}

pub(in crate::textedit) fn automatic(style: u8, text: &str) -> Result<u8, String> {
    let face = super::face(data(style), false)?;
    if text
        .chars()
        .filter(|ch| *ch != '\n')
        .all(|ch| face.glyph_index(ch).is_some_and(|g| g.0 != 0))
    {
        Ok(style)
    } else {
        // CJK has upright regular/bold styles; never claim a synthetic italic.
        Ok(4 + (style & 1))
    }
}

/// The style number of a program that is an installed font's subset rather
/// than a bundled one: its own key in [`Programs`], beside the six styles.
const INSTALLED: u8 = 6;

impl Font {
    pub(in crate::textedit) fn new(
        style: u8,
        text: &str,
    ) -> Result<(Self, super::Metrics), String> {
        let face = super::face(data(style), false).map_err(|e| format!("Bundled font: {e}"))?;
        let (mut font, metrics) = Self::measure(&face, style, text, false)?;
        if style >= 4 {
            font.program_key = font.glyphs.clone();
            let (program, glyphs) = super::fallback_subset::build(data(style), &font.glyphs)
                .map_err(|e| format!("Bundled font subset: {e}"))?;
            // Revalidate the remapped outlines and advances, not just the text map.
            let face = super::face(&program, false)?;
            let (remapped, _) = super::unicode::Metrics::with_glyphs(
                &face,
                font.codes.clone(),
                1000.,
                &font.widths,
                Some(&glyphs),
            )?;
            for line in text.split('\n') {
                remapped.replacement(line, 12., 0., 0.)?;
            }
            font.subset = Some(program);
            font.glyphs = glyphs;
        }
        Ok((font, metrics))
    }

    /// A subset of an installed font, already built and checked by
    /// `installed::accept`, set up to write `text` under `name`, read here
    /// through its own character map.
    ///
    /// A TrueType subset is embedded as it is. A CFF one is embedded as the
    /// bare CID-keyed program inside it (FontFile3 /CIDFontType0C, PDF 1.3),
    /// the carrier xdvipdfmx, LuaTeX and Typst write and `cff::cid` reads;
    /// `subsetter` has made it CID-keyed with each glyph its own CID, so the
    /// codes written are the subset's glyph ids. The OS/2
    /// rights go into the program (`cff::rights`), and the program is read
    /// back through the parser a reopened document is read with, every width
    /// against the one `/W` will declare, before anything is written.
    pub(in crate::textedit) fn installed(
        program: Vec<u8>,
        name: &str,
        text: &str,
    ) -> Result<(Self, super::Metrics), String> {
        use sha2::{Digest, Sha256};
        let face = super::installed_face(&program)?;
        let cff = face.tables().cff.is_some();
        let (mut font, metrics) = Self::measure(&face, INSTALLED, text, cff)?;
        if cff {
            let rights = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"OS/2"))
                .map(|os2| {
                    os2.get(8..10)
                        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                        .ok_or("invalid installed font rights")
                })
                .transpose()?;
            let table = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"CFF "))
                .ok_or("missing installed CFF program")?;
            let compact = super::cff::rights::with_rights(table, rights)?;
            let read = super::cff::cid::parse(&compact)?;
            for (&code, &width) in &font.widths {
                let glyph = read.glyph(code)?.ok_or("unreadable installed CFF glyph")?;
                if (glyph.advance - width).abs() > super::installed::TOLERANCE {
                    return Err("installed CFF widths disagree with its metrics".into());
                }
            }
            font.compact = Some(compact);
        }
        font.installed = Some(name.to_owned());
        font.program_key = Sha256::digest(&program).to_vec();
        font.subset = Some(program);
        Ok((font, metrics))
    }

    /// One code per distinct character of `text`, with its glyph in `face`
    /// and that glyph's advance as its width, and the metrics that write them.
    /// With `identity` the code is the glyph id, for a CID-keyed CFF program,
    /// which has no CIDToGIDMap; otherwise codes count up from 1.
    fn measure(
        face: &ttf_parser::Face<'_>,
        style: u8,
        text: &str,
        identity: bool,
    ) -> Result<(Self, super::Metrics), String> {
        let characters: std::collections::BTreeSet<_> =
            text.chars().filter(|ch| *ch != '\n').collect();
        if characters.len() > crate::textedit::MAX_TEXT {
            return Err("too many replacement characters".into());
        }
        let mut font = Self {
            style,
            installed: None,
            codes: BTreeMap::new(),
            widths: BTreeMap::new(),
            glyphs: vec![0, 0],
            subset: None,
            compact: None,
            program_key: Vec::new(),
        };
        for (index, ch) in characters.into_iter().enumerate() {
            let glyph = face
                .glyph_index(ch)
                .filter(|id| id.0 != 0)
                .ok_or("The selected fallback font does not contain a required character")?;
            // Two characters drawn by one glyph would share an identity code;
            // the later would take the code's text, and the layout below, which
            // must encode every character, refuses the earlier.
            let code = if identity {
                glyph.0
            } else {
                (index + 1) as u16
            };
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
            face,
            font.codes.clone(),
            1000.,
            &font.widths,
            (!identity).then_some(font.glyphs.as_slice()),
        )?;
        let metrics = super::Metrics {
            restricted: false,
            opaque: None,
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
        programs: &mut Programs,
    ) -> Result<ObjectId, String> {
        let bytes = self.subset.as_deref().unwrap_or_else(|| data(self.style));
        let program = *programs
            .entry((self.style, self.program_key.clone()))
            .or_insert_with(|| {
                doc.add_object(match &self.compact {
                    Some(compact) => Stream::new(
                        dictionary! { "Subtype" => "CIDFontType0C" },
                        compact.clone(),
                    ),
                    None => Stream::new(
                        dictionary! { "Length1" => bytes.len() as i64 },
                        bytes.to_vec(),
                    ),
                })
            });
        let mut name = self
            .installed
            .clone()
            .unwrap_or_else(|| label(self.style).replace(' ', ""));
        if self.subset.is_some() {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(bytes);
            let tag: String = hash[..6]
                .iter()
                .map(|b| char::from(b'A' + b % 26))
                .collect();
            name = format!("{tag}+{name}");
        }
        let face = if self.installed.is_some() {
            super::installed_face(bytes)?
        } else {
            super::face(data(self.style), false)?
        };
        let (subtype, carrier) = if self.compact.is_some() {
            ("CIDFontType0", "FontFile3")
        } else {
            ("CIDFontType2", "FontFile2")
        };
        let scale = 1000. / f64::from(face.units_per_em());
        let bounds = face.global_bounding_box();
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => name.as_str(), "Flags" => 4,
            "FontBBox" => vec![Object::Real((f64::from(bounds.x_min)*scale) as f32),
                Object::Real((f64::from(bounds.y_min)*scale) as f32), Object::Real((f64::from(bounds.x_max)*scale) as f32),
                Object::Real((f64::from(bounds.y_max)*scale) as f32)],
            "ItalicAngle" => if self.installed.is_some() { Object::Real(face.italic_angle()) }
                else if matches!(self.style, 2 | 3) { Object::Integer(-12) } else { Object::Integer(0) },
            "Ascent" => (f64::from(face.ascender())*scale) as i64,
            "Descent" => (f64::from(face.descender())*scale) as i64,
            "CapHeight" => 714, "StemV" => 80, carrier => program
        });
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
        let mut child = dictionary! {
            "Type" => "Font", "Subtype" => subtype, "BaseFont" => name.as_str(),
            "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
            "FontDescriptor" => descriptor, "DW" => 1000, "W" => widths
        };
        // A CID-keyed CFF maps CIDs to glyphs through its own charset, and
        // ISO 32000-1 Table 117 gives a CIDToGIDMap to CIDFontType2 only.
        if self.compact.is_none() {
            let glyphs = doc.add_object(Stream::new(Dictionary::new(), self.glyphs.clone()));
            child.set("CIDToGIDMap", glyphs);
        }
        let child = doc.add_object(child);
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
