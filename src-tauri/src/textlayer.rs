//! Invisible text over a page that is a picture of text.
//!
//! A scanned page has no text objects, so search, selection, copy and
//! text redaction all have nothing to work on. This module writes what an OCR
//! engine read back into the page as text drawn in render mode 3 --- neither
//! filled nor stroked --- so the page looks the same and reads as text.
//!
//! It is the writer only. Which pages need a layer and what the words are is
//! decided by the caller, which has the recogniser; this module is handed words
//! and rectangles and never looks at pixels.
//!
//! ## One box per word, and why not per line
//!
//! Measured 2026-10-03 on `testdata/text-base14.pdf`, rendered to an image and
//! given a layer back: with one box per **line**, a search hit for a word in the
//! middle of the line was drawn up to 33 pt from the word, because the layer
//! spreads a line's characters evenly and the type on the page is proportional.
//! With one box per **word** the hit was within 1.5 pt on every word tried. So a
//! [`Word`] is a word, and a caller holding lines splits them first.
//!
//! ## The font draws a box, and why it is not empty
//!
//! The text is set in a font made here, [`font_program`]: one glyph, a
//! rectangle from the descender to the ascender, which every character code
//! maps to. Nothing paints it --- render mode 3 --- but PDFium takes a
//! character's box from its glyph outline, and the same measurement with an
//! **empty** glyph read the words back and gave every search hit no rectangle
//! at all. A font that is not embedded read back correctly in PDFium too, but
//! then the result depends on which substitute each reader picks.
//!
//! Every word is followed by a space of its own, placed just past the word's
//! box, so that two words whose boxes touch still read as two.
//!
//! Character codes are UTF-16 code units under `/Identity-H`, and the
//! `/ToUnicode` map is the identity, so no per-document encoding is built and a
//! word in any script is written the same way.
//!
//! ## Why the layer goes first in the page's content
//!
//! A page's own content may end with a transformed matrix or an unclosed `q`.
//! Text appended after it would inherit that; text placed before it starts in
//! the default graphics state, and its own `q`/`Q` keeps the font, the render
//! mode and the horizontal scaling from reaching the page's content.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::pagetree::{self, DisplayedPage};
use crate::save::Upright;

/// One recognised word and where it is on the page as displayed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// What the engine read.
    pub text: String,
    /// `left, top, right, bottom` in points, y down, origin at the displayed
    /// page's top-left --- [`crate::ocr::RecognisedItem::rect`]'s convention.
    pub rect: [f32; 4],
}

/// The words to write on one page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// The page, zero-based, in the file's own page order.
    pub page: u32,
    /// Its words, in reading order.
    pub words: Vec<Word>,
}

/// The most words one page's layer may carry.
///
/// A dense A4 page of 6 pt type is about 2,500 words. This bound is there so a
/// request cannot make the writer build an unbounded content stream.
pub const MAX_WORDS: usize = 20_000;

/// The most UTF-16 code units one word may carry.
pub const MAX_UNITS: usize = 512;

/// The em the glyph is measured in.
///
/// 1000, because a PDF's own font metrics --- `/DW`, `/FontBBox`, `/Ascent` ---
/// are in thousandths of the font size whatever the program's em is. With one
/// number for both there is no conversion to forget. An em of 1024 was tried on
/// 2026-10-03 and `/DW` was written as 512: every word then ran 2.4% long.
const EM: u16 = 1000;
/// The glyph's advance: half an em.
const ADVANCE: i16 = 500;
/// The glyph's vertical extent, which is also the font's. One em in total.
///
/// **PDFium reports the box slightly larger than this**, measured the same
/// day with `textlayer-probe`: the top comes back higher by 1/64 of the ascent
/// (0.15 pt on 12 pt type) and the last character's right edge further by 1/64
/// of the advance. The left and bottom edges come back exactly. Nothing here
/// compensates, because other readers take a character's box from the advance
/// and the ascent, where these numbers are already exact.
const ASCENT: i16 = 800;
const DESCENT: i16 = -200;

/// The name the font is registered under in a page's resources, before any
/// suffix needed to avoid one the page already uses.
const FONT_NAME: &str = "TpdfOcr";

/// Writes each layer into its page.
///
/// A layer whose words are all unusable --- empty, or with a rectangle that is
/// not finite or encloses nothing --- leaves its page untouched.
///
/// # Errors
///
/// A page number the document does not have, the same page named twice, a layer
/// over [`MAX_WORDS`], a word over [`MAX_UNITS`], or a page that is not a
/// dictionary. Nothing is written when any layer is refused.
pub fn write(doc: &mut Document, layers: &[Layer]) -> Result<(), String> {
    if layers.is_empty() {
        return Ok(());
    }
    let pages = pagetree::ordered_pages(doc);
    let mut seen = std::collections::BTreeSet::new();
    let mut planned = Vec::new();
    for layer in layers {
        let id = *pages.get(layer.page as usize).ok_or_else(|| {
            format!(
                "the text layer names page {}, and the document has {}",
                layer.page + 1,
                pages.len()
            )
        })?;
        if !seen.insert(layer.page) {
            return Err(format!(
                "the text layer names page {} twice",
                layer.page + 1
            ));
        }
        if layer.words.len() > MAX_WORDS {
            return Err(format!(
                "the text layer for page {} has {} words, over the limit of {MAX_WORDS}",
                layer.page + 1,
                layer.words.len()
            ));
        }
        let shown = pagetree::displayed_page(doc, id);
        if let Some(lines) = content(shown, &layer.words)? {
            planned.push((id, lines));
        }
    }
    if planned.is_empty() {
        return Ok(());
    }
    let font = add_font(doc)?;
    for (page, lines) in planned {
        attach(doc, page, font, &lines)?;
    }
    Ok(())
}

/// The operators that set one page's words, without the font's resource name.
///
/// `None` when no word is usable. The name is filled in by [`attach`], which is
/// the first place that knows which names the page already uses.
fn content(shown: DisplayedPage, words: &[Word]) -> Result<Option<Vec<String>>, String> {
    let mut lines = Vec::new();
    for word in words {
        if let Some(line) = show(shown, word)? {
            lines.push(line);
        }
    }
    Ok((!lines.is_empty()).then_some(lines))
}

/// One word as `Tf Tz Tm Tj` operands, with `{}` left for the font's name.
fn show(shown: DisplayedPage, word: &Word) -> Result<Option<String>, String> {
    let units: Vec<u16> = word
        .text
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .encode_utf16()
        .collect();
    if units.len() > MAX_UNITS {
        return Err(format!(
            "a word in the text layer is {} characters long, over the limit of {MAX_UNITS}",
            units.len()
        ));
    }
    let [left, top, right, bottom] = word.rect;
    let usable = word.rect.iter().all(|v| v.is_finite()) && right > left && bottom > top;
    if units.is_empty() || !usable {
        return Ok(None);
    }

    let (ox, oy) = (f64::from(shown.origin.0), f64::from(shown.origin.1));
    let quad = crate::text::from_device(shown.turns, shown.width, shown.height, word.rect);
    let upright = Upright::of(
        shown.turns,
        [quad[0] + ox, quad[1] + oy, quad[2] + ox, quad[3] + oy],
    );

    // The glyph is one em tall, descender to ascender, so a font size equal to
    // the box's height makes the glyph exactly as tall as the box.
    let size = upright.height;
    let em = f64::from(EM);
    let natural = units.len() as f64 * f64::from(ADVANCE) / em * size;
    let scaling = upright.width / natural * 100.0;
    // The baseline sits above the box's bottom by the descender's share.
    let baseline = size * f64::from(ASCENT) / em;
    // **A space after the word, outside its box.** The scaling above is
    // computed from the word alone, so its characters fill the box exactly and
    // the space falls just past the right edge. A reader decides where one word
    // ends from the gap before the next, and an engine's boxes touch: measured
    // 2026-10-03 with Vision on a rendered page, "REDACT ME" read back as
    // "REDACTME" and "my vow" as "myvow" until the space was written.
    let hex: String = units
        .iter()
        .chain(std::iter::once(&0x0020))
        .map(|unit| format!("{unit:04X}"))
        .collect();

    Ok(Some(format!(
        "/{{}} {size:.3} Tf {scaling:.3} Tz {} <{hex}> Tj",
        upright.text_matrix(0.0, baseline)
    )))
}

/// Puts the layer first in the page's content and its font in the resources.
fn attach(
    doc: &mut Document,
    page: ObjectId,
    font: ObjectId,
    lines: &[String],
) -> Result<(), String> {
    // The effective resources, which a page may inherit. They are copied onto
    // the page so that adding a font to them changes this page and no other.
    let mut resources = match pagetree::inherited(doc, page, b"Resources") {
        Some(found) => dictionary_of(doc, &found),
        None => Dictionary::new(),
    };
    let mut fonts = match resources.get(b"Font") {
        Ok(found) => dictionary_of(doc, found),
        Err(_) => Dictionary::new(),
    };
    let mut name = FONT_NAME.to_string();
    let mut suffix = 0_u32;
    while fonts.has(name.as_bytes()) {
        suffix += 1;
        name = format!("{FONT_NAME}{suffix}");
    }
    fonts.set(name.as_bytes().to_vec(), Object::Reference(font));
    resources.set("Font", Object::Dictionary(fonts));

    let mut body = String::from("q\nBT\n3 Tr\n");
    for line in lines {
        body.push_str(&line.replacen("{}", &name, 1));
        body.push('\n');
    }
    body.push_str("ET\nQ\n");
    let mut stream = Stream::new(Dictionary::new(), body.into_bytes());
    let _ = stream.compress();
    let layer = doc.add_object(stream);

    let existing = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?
        .get(b"Contents")
        .ok()
        .cloned();
    let mut contents = vec![Object::Reference(layer)];
    match existing {
        Some(Object::Array(parts)) => contents.extend(parts),
        Some(Object::Reference(id)) => match doc.get_object(id) {
            // A reference to an array of streams is the array's entries; a
            // reference to anything else is one stream.
            Ok(Object::Array(parts)) => contents.extend(parts.iter().cloned()),
            _ => contents.push(Object::Reference(id)),
        },
        _ => {}
    }

    let dict = doc
        .get_object_mut(page)
        .and_then(Object::as_dict_mut)
        .map_err(|e| format!("page {page:?} is not a dictionary: {e}"))?;
    dict.set("Contents", Object::Array(contents));
    dict.set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// A dictionary, read through one reference, or an empty one.
fn dictionary_of(doc: &Document, object: &Object) -> Dictionary {
    let direct = match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        other => Some(other),
    };
    direct
        .and_then(|found| found.as_dict().ok())
        .cloned()
        .unwrap_or_default()
}

/// Adds the font's four objects and returns the Type 0 font.
fn add_font(doc: &mut Document) -> Result<ObjectId, String> {
    let program = font_program();
    let length = i64::try_from(program.len()).map_err(|e| e.to_string())?;
    let mut file = Stream::new(dictionary! { "Length1" => length }, program);
    let _ = file.compress();
    let file = doc.add_object(file);

    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "TpdfOcrBox",
        // Symbolic: the font has no Latin character set to claim.
        "Flags" => 4,
        "FontBBox" => vec![
            0.into(),
            i64::from(DESCENT).into(),
            i64::from(ADVANCE).into(),
            i64::from(ASCENT).into(),
        ],
        "ItalicAngle" => 0,
        "Ascent" => i64::from(ASCENT),
        "Descent" => i64::from(DESCENT),
        "CapHeight" => i64::from(ASCENT),
        "StemV" => 80,
        "FontFile2" => file,
    });

    // Every character code draws glyph 0: two zero bytes per code.
    let mut map = Stream::new(Dictionary::new(), vec![0_u8; 2 * 65_536]);
    let _ = map.compress();
    let map = doc.add_object(map);

    let descendant = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "TpdfOcrBox",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor,
        "DW" => i64::from(ADVANCE),
        "CIDToGIDMap" => map,
    });

    let mut to_unicode = Stream::new(Dictionary::new(), to_unicode().into_bytes());
    let _ = to_unicode.compress();
    let to_unicode = doc.add_object(to_unicode);

    Ok(doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "TpdfOcrBox",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(descendant)],
        "ToUnicode" => to_unicode,
    }))
}

/// The identity map from a two-byte code to the UTF-16 unit of the same value.
///
/// One range per high byte, because a `bfrange` may not cross one, and at most
/// 100 ranges per block, which is the format's limit.
fn to_unicode() -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let highs: Vec<u32> = (0..256).collect();
    for block in highs.chunks(100) {
        out.push_str(&format!("{} beginbfrange\n", block.len()));
        for high in block {
            out.push_str(&format!("<{high:02X}00> <{high:02X}FF> <{high:02X}00>\n"));
        }
        out.push_str("endbfrange\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

/// A TrueType program with one glyph: a rectangle [`ADVANCE`] wide, from
/// [`DESCENT`] to [`ASCENT`].
///
/// Built here rather than shipped as a file, because it is six tables of a few
/// numbers each and every one of them is stated above. It carries the tables a
/// font embedded in a PDF as `CIDFontType2` needs and no others.
#[must_use]
pub fn font_program() -> Vec<u8> {
    fn be16(out: &mut Vec<u8>, values: &[i32]) {
        for value in values {
            out.extend_from_slice(&(*value as i16).to_be_bytes());
        }
    }
    let (advance, ascent, descent) = (i32::from(ADVANCE), i32::from(ASCENT), i32::from(DESCENT));

    // One contour of four on-curve points, each coordinate a 16-bit delta.
    let mut glyf = Vec::new();
    be16(&mut glyf, &[1, 0, descent, advance, ascent]);
    be16(&mut glyf, &[3, 0]); // last point of the contour; no instructions
    glyf.extend_from_slice(&[1, 1, 1, 1]); // on-curve, long coordinates
    be16(&mut glyf, &[0, 0, advance, 0]); // x deltas
    be16(&mut glyf, &[descent, ascent - descent, 0, descent - ascent]); // y deltas
    let glyph_length = glyf.len();

    let mut head = Vec::new();
    head.extend_from_slice(&0x0001_0000_u32.to_be_bytes()); // version
    head.extend_from_slice(&0x0001_0000_u32.to_be_bytes()); // revision
    head.extend_from_slice(&0_u32.to_be_bytes()); // checksum adjustment, below
    head.extend_from_slice(&0x5F0F_3CF5_u32.to_be_bytes()); // magic
    be16(&mut head, &[0x0003, i32::from(EM)]); // flags, units per em
    head.extend_from_slice(&[0; 16]); // created, modified
    be16(&mut head, &[0, descent, advance, ascent]); // bounding box
    be16(&mut head, &[0, 8, 2, 0, 0]); // style, smallest size, direction, short loca, format

    let mut hhea = Vec::new();
    hhea.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    be16(&mut hhea, &[ascent, descent, 0, advance, 0, 0, advance]);
    be16(&mut hhea, &[1, 0, 0, 0, 0, 0, 0, 0, 1]); // caret, reserved, format, one metric

    let mut maxp = Vec::new();
    maxp.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    be16(&mut maxp, &[1, 4, 1, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0]);

    let mut hmtx = Vec::new();
    be16(&mut hmtx, &[advance, 0]);

    // Short offsets count in units of two bytes.
    let mut loca = Vec::new();
    be16(&mut loca, &[0, (glyph_length / 2) as i32]);

    // Table records are sorted by tag.
    let tables: [(&[u8; 4], Vec<u8>); 6] = [
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let checksum = |bytes: &[u8]| {
        bytes.chunks(4).fold(0_u32, |sum, chunk| {
            let mut word = [0_u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    };

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    // Six tables: the largest power of two not above it is four.
    for value in [6_u16, 64, 2, 32] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut head_at = 0;
    for (tag, bytes) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&checksum(bytes).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        if *tag == b"head" {
            head_at = offset;
        }
        offset += bytes.len().div_ceil(4) * 4;
    }
    for (_, bytes) in &tables {
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(4) * 4, 0);
    }
    let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&out));
    out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One page, `width` by `height`, with the given `/Rotate` and one content
    /// stream that draws nothing.
    fn document(rotate: i64) -> (Document, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let content = doc.add_object(Stream::new(Dictionary::new(), b"q Q".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Rotate" => rotate,
            "Contents" => content,
        });
        doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page)],
                "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => dictionary! {
                    "Font" => dictionary! { "TpdfOcr" => Object::Null },
                },
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        (doc, page)
    }

    fn word(text: &str, rect: [f32; 4]) -> Word {
        Word {
            text: text.into(),
            rect,
        }
    }

    fn layer_text(doc: &Document, page: ObjectId) -> String {
        let contents = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Contents"))
            .and_then(Object::as_array)
            .expect("contents array");
        let first = contents[0].as_reference().expect("a reference");
        let stream = doc
            .get_object(first)
            .and_then(Object::as_stream)
            .expect("stream");
        let bytes = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        String::from_utf8(bytes).expect("ascii operators")
    }

    #[test]
    fn the_font_program_is_one_box_glyph_of_the_stated_size() {
        let program = font_program();
        let face = ttf_parser::Face::parse(&program, 0).expect("a TrueType program");
        assert_eq!(face.number_of_glyphs(), 1);
        assert_eq!(face.units_per_em(), EM);
        let glyph = ttf_parser::GlyphId(0);
        assert_eq!(face.glyph_hor_advance(glyph), Some(ADVANCE as u16));
        let bounds = face
            .glyph_bounding_box(glyph)
            .expect("an outline, not an empty glyph");
        assert_eq!(
            (bounds.x_min, bounds.y_min, bounds.x_max, bounds.y_max),
            (0, DESCENT, ADVANCE, ASCENT)
        );
    }

    #[test]
    fn the_glyph_is_one_em_tall_and_half_an_em_wide() {
        assert_eq!(i32::from(ASCENT) - i32::from(DESCENT), i32::from(EM));
        assert_eq!(i32::from(ADVANCE) * 2, i32::from(EM));
    }

    #[test]
    fn the_font_program_checksums_to_the_format_s_constant() {
        let program = font_program();
        // A header, six table records and the six tables, each padded to four.
        assert_eq!(program.len(), 276);
        let sum = program.chunks(4).fold(0_u32, |sum, chunk| {
            sum.wrapping_add(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        });
        assert_eq!(sum, 0xB1B0_AFBA);
    }

    #[test]
    fn a_word_is_set_at_its_box_and_scaled_to_its_width() {
        let (mut doc, page) = document(0);
        // Two characters in a box 30 wide and 10 tall, 100 from the left and
        // 50 from the top of an 800-tall page.
        let layers = [Layer {
            page: 0,
            words: vec![word("Hi", [100.0, 50.0, 130.0, 60.0])],
        }];
        write(&mut doc, &layers).expect("written");
        let text = layer_text(&doc, page);
        // Natural width is 2 x 0.5 x 10 = 10, so 30 wide is 300%. The baseline
        // is 0.8 x 10 below the top: 800 - 50 - 8 = 742.
        assert!(
            text.contains("10.000 Tf 300.000 Tz 1 0 0 1 100 742 Tm <004800690020> Tj"),
            "{text}"
        );
        assert!(text.starts_with("q\nBT\n3 Tr\n"), "{text}");
        assert!(text.ends_with("ET\nQ\n"), "{text}");
    }

    #[test]
    fn on_a_turned_page_the_type_runs_the_way_it_is_read() {
        let (mut doc, page) = document(90);
        // The page is displayed 800 wide and 600 tall.
        let layers = [Layer {
            page: 0,
            words: vec![word("Hi", [100.0, 50.0, 130.0, 60.0])],
        }];
        write(&mut doc, &layers).expect("written");
        let text = layer_text(&doc, page);
        // A quarter turn clockwise: the reader's right is the page's up, and
        // the displayed point (100, 58) is the page's (58, 100).
        assert!(
            text.contains("10.000 Tf 300.000 Tz 0 1 -1 0 58 100 Tm <004800690020> Tj"),
            "{text}"
        );
    }

    #[test]
    fn the_layer_goes_before_the_page_s_own_content_and_keeps_it() {
        let (mut doc, page) = document(0);
        let before = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Contents"))
            .cloned()
            .expect("contents");
        write(
            &mut doc,
            &[Layer {
                page: 0,
                words: vec![word("a", [0.0, 0.0, 5.0, 5.0])],
            }],
        )
        .expect("written");
        let contents = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Contents"))
            .and_then(Object::as_array)
            .expect("contents array")
            .clone();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[1], before);
    }

    #[test]
    fn inherited_resources_are_copied_and_a_taken_name_is_not_reused() {
        let (mut doc, page) = document(0);
        write(
            &mut doc,
            &[Layer {
                page: 0,
                words: vec![word("a", [0.0, 0.0, 5.0, 5.0])],
            }],
        )
        .expect("written");
        let fonts = doc
            .get_object(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Resources"))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Font"))
            .and_then(Object::as_dict)
            .expect("the page's own font resources")
            .clone();
        // The inherited entry is still there, and ours is beside it.
        assert_eq!(fonts.get(b"TpdfOcr").ok(), Some(&Object::Null));
        assert!(fonts
            .get(b"TpdfOcr1")
            .and_then(Object::as_reference)
            .is_ok());
        assert!(layer_text(&doc, page).contains("/TpdfOcr1 "));
    }

    #[test]
    fn a_character_outside_the_basic_plane_is_two_code_units() {
        let (mut doc, page) = document(0);
        write(
            &mut doc,
            &[Layer {
                page: 0,
                words: vec![word("\u{1D49C}", [0.0, 0.0, 10.0, 10.0])],
            }],
        )
        .expect("written");
        assert!(layer_text(&doc, page).contains("<D835DC9C0020> Tj"));
    }

    #[test]
    fn unusable_words_are_left_out_and_a_page_with_none_is_untouched() {
        let (mut doc, page) = document(0);
        let before = doc.objects.clone();
        let words = vec![
            word("", [0.0, 0.0, 5.0, 5.0]),
            word(" \u{7} ", [0.0, 0.0, 5.0, 5.0]),
            word("flat", [0.0, 5.0, 5.0, 5.0]),
            word("thin", [5.0, 0.0, 5.0, 5.0]),
            word("nan", [f32::NAN, 0.0, 5.0, 5.0]),
        ];
        write(&mut doc, &[Layer { page: 0, words }]).expect("nothing to write is not an error");
        assert_eq!(doc.objects, before);
        let _ = page;
    }

    #[test]
    fn a_page_the_document_lacks_a_page_named_twice_and_an_oversized_layer_are_refused() {
        let one = |page| Layer {
            page,
            words: vec![word("a", [0.0, 0.0, 5.0, 5.0])],
        };
        for (layers, expected) in [
            (vec![one(1)], "names page 2"),
            (vec![one(0), one(0)], "twice"),
            (
                vec![Layer {
                    page: 0,
                    words: vec![word("a", [0.0, 0.0, 5.0, 5.0]); MAX_WORDS + 1],
                }],
                "over the limit",
            ),
            (
                vec![Layer {
                    page: 0,
                    words: vec![word(&"a".repeat(MAX_UNITS + 1), [0.0, 0.0, 5.0, 5.0])],
                }],
                "characters long",
            ),
        ] {
            let (mut doc, _) = document(0);
            let before = doc.objects.clone();
            let refused = write(&mut doc, &layers).expect_err("refused");
            assert!(refused.contains(expected), "{refused}");
            assert_eq!(doc.objects, before, "a refusal writes nothing");
        }
    }

    #[test]
    fn a_cropped_page_s_corner_is_where_the_layer_measures_from() {
        let (mut doc, page) = document(0);
        // The page shows 500 x 700 of the sheet, starting 40 right and 60 up.
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .expect("the page")
            .set(
                "CropBox",
                vec![40.into(), 60.into(), 540.into(), 760.into()],
            );
        write(
            &mut doc,
            &[Layer {
                page: 0,
                words: vec![word("Hi", [100.0, 50.0, 130.0, 60.0])],
            }],
        )
        .expect("written");
        // 100 from the shown left is 140 on the sheet; 58 down from the shown
        // top, which is at 760, is 702.
        let text = layer_text(&doc, page);
        assert!(text.contains(" 1 0 0 1 140 702 Tm "), "{text}");
    }

    #[test]
    fn a_page_whose_content_is_an_array_keeps_every_part_after_the_layer() {
        for by_reference in [false, true] {
            let (mut doc, page) = document(0);
            let parts: Vec<Object> = (0..2)
                .map(|_| {
                    Object::Reference(
                        doc.add_object(Stream::new(Dictionary::new(), b"q Q".to_vec())),
                    )
                })
                .collect();
            let held = if by_reference {
                Object::Reference(doc.add_object(Object::Array(parts.clone())))
            } else {
                Object::Array(parts.clone())
            };
            doc.get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .expect("the page")
                .set("Contents", held);
            write(
                &mut doc,
                &[Layer {
                    page: 0,
                    words: vec![word("a", [0.0, 0.0, 5.0, 5.0])],
                }],
            )
            .expect("written");
            let contents = doc
                .get_object(page)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"Contents"))
                .and_then(Object::as_array)
                .expect("contents array")
                .clone();
            assert_eq!(contents.len(), 3, "by reference: {by_reference}");
            assert_eq!(contents[1..], parts[..], "by reference: {by_reference}");
        }
    }

    #[test]
    fn the_unicode_map_covers_every_high_byte_once() {
        let map = to_unicode();
        assert_eq!(map.matches("beginbfrange").count(), 3);
        assert_eq!(map.matches("> <").count(), 1 + 2 * 256);
        assert!(map.contains("<4100> <41FF> <4100>\n"));
        assert!(map.contains("<FF00> <FFFF> <FF00>\n"));
    }
}
