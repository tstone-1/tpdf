//! Word and Acrobat PDFMaker embed WinAnsi TrueType subsets whose ToUnicode
//! restates WinAnsi punctuation such as U+2019 at code 0x92. Glyphs for those
//! codes are found through the (3,1) cmap by Unicode value, not by the code.
use super::tests::{fixture, SYNTHETIC};
use super::*;
use crate::textedit::{self, Change};
use lopdf::Stream;

// (WinAnsi code, Unicode, existing outline used for it)
const EXTRA: [(u8, u32, char); 3] = [
    (0x92, 0x2019, 'A'),
    (0x97, 0x2014, 'B'),
    (0xA7, 0x00A7, 'S'),
];

fn glyph(ch: char) -> u16 {
    Face::parse(SYNTHETIC, 0)
        .unwrap()
        .glyph_index(ch)
        .unwrap()
        .0
}

fn ascii() -> Vec<(u32, u16)> {
    let face = Face::parse(SYNTHETIC, 0).unwrap();
    (32_u8..=89)
        .filter_map(|code| {
            face.glyph_index(char::from(code))
                .map(|g| (u32::from(code), g.0))
        })
        .collect()
}

fn format6(entries: &[(u32, u16)]) -> Vec<u8> {
    let first = entries.iter().map(|(code, _)| *code).min().unwrap();
    let last = entries.iter().map(|(code, _)| *code).max().unwrap();
    let count = (last - first + 1) as u16;
    let mut table = Vec::new();
    for value in [6_u16, 10 + 2 * count, 0, first as u16, count] {
        table.extend(value.to_be_bytes());
    }
    for code in first..=last {
        let glyph = entries
            .iter()
            .find(|(value, _)| *value == code)
            .map_or(0, |(_, glyph)| *glyph);
        table.extend(glyph.to_be_bytes());
    }
    table
}

fn format0(entries: &[(u32, u16)]) -> Vec<u8> {
    let mut table = Vec::new();
    for value in [0_u16, 262, 0] {
        table.extend(value.to_be_bytes());
    }
    let mut glyphs = [0_u8; 256];
    for (code, glyph) in entries {
        glyphs[*code as usize] = u8::try_from(*glyph).unwrap();
    }
    table.extend(glyphs);
    table
}

// Records must be given in (platform, encoding) order.
fn cmap(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut header = vec![0, 0];
    header.extend((records.len() as u16).to_be_bytes());
    let mut offset = 4 + 8 * records.len();
    let mut bodies: Vec<u8> = Vec::new();
    for (platform, encoding, table) in records {
        header.extend(platform.to_be_bytes());
        header.extend(encoding.to_be_bytes());
        header.extend((offset as u32).to_be_bytes());
        offset += table.len();
        bodies.extend(table);
    }
    header.extend(bodies);
    header
}

fn program(cmap: Vec<u8>) -> Vec<u8> {
    let mut bytes = SYNTHETIC.to_vec();
    let offset = bytes.len() as u32;
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    for record in bytes[12..12 + 16 * count].chunks_exact_mut(16) {
        if &record[..4] == b"cmap" {
            record[8..12].copy_from_slice(&offset.to_be_bytes());
            record[12..16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
        }
    }
    bytes.extend(cmap);
    bytes
}

fn unicode_entries() -> Vec<(u32, u16)> {
    let mut entries = ascii();
    entries.extend(EXTRA.iter().map(|(_, unicode, ch)| (*unicode, glyph(*ch))));
    entries
}

// The Mac table agrees for ASCII and numbers codes above it differently, as a
// MacRoman legacy map does. Only the Unicode table may select those glyphs.
fn mac_entries() -> Vec<(u32, u16)> {
    let mut entries = ascii();
    entries.extend([(0x97, glyph('Y'))]);
    entries
}

fn mapping(extra: &str) -> String {
    format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfrange <20> <59> <0020> endbfrange {extra} endcmap CMapName currentdict /CMap defineresource pop end end")
}

const EXTRA_MAP: &str = "3 beginbfchar <92> <2019> <97> <2014> <A7> <00A7> endbfchar";

// ids: font, program, ToUnicode stream.
fn word_font(tables: Vec<u8>, map: &str) -> (Document, [lopdf::ObjectId; 3]) {
    let (mut doc, font, _, font_program) = fixture();
    doc.get_object_mut(font_program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = program(tables);
    let stream = doc.add_object(Stream::new(Dictionary::new(), mapping(map).into_bytes()));
    let dict = doc.get_dictionary_mut(font).unwrap();
    dict.set("LastChar", 255);
    dict.set("Widths", vec![Object::Integer(600); 224]);
    dict.set("ToUnicode", stream);
    (doc, [font, font_program, stream])
}

fn standard() -> (Document, [lopdf::ObjectId; 3]) {
    word_font(
        cmap(&[
            (1, 0, format0(&mac_entries())),
            (3, 1, format6(&unicode_entries())),
        ]),
        EXTRA_MAP,
    )
}

fn change(doc: &Document, replacement: &str) -> Change {
    let runs = textedit::scan(doc, 0).unwrap();
    Change {
        layout: None,
        page: 0,
        operator: runs.runs[0].operator,
        revision: runs.revision,
        original: runs.runs[0].text.clone(),
        replacement: replacement.into(),
    }
}

#[test]
fn textedit_winansi_punctuation_uses_unicode_glyphs_and_writes_winansi_codes() {
    let (mut doc, ids) = standard();
    let objects = doc.objects.clone();
    let edit = change(&doc, "FIRST\u{2019}S \u{2014} \u{a7}");
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "FIRST\u{2019}S \u{2014} \u{a7}");
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.get_page_content(page);
    let written = b"FIRST\x92S \x97 \xa7";
    assert!(content.windows(written.len()).any(|w| w == written));
    for id in ids {
        assert_eq!(doc.objects[&id], objects[&id]);
    }
    // A character with a slot but no glyph in this subset is not offered.
    let (mut doc, _) = standard();
    let edit = change(&doc, "FIRST\u{201c}");
    assert_eq!(
        textedit::write(&mut doc, &[edit]).unwrap_err(),
        "the font has no validated glyph for this character"
    );
}

#[test]
fn textedit_winansi_mapping_must_restate_winansi_and_unicode_cmaps_must_agree() {
    let (doc, _) = standard();
    let update = change(&doc, "IN");
    let mut disagreeing = unicode_entries();
    disagreeing.retain(|(code, _)| *code != 0x2019);
    disagreeing.push((0x2019, glyph('B')));
    for defect in 0..6 {
        let (mut doc, _) = match defect {
            // ToUnicode names a different character than WinAnsi's 0x92.
            0 => word_font(
                cmap(&[
                    (1, 0, format0(&mac_entries())),
                    (3, 1, format6(&unicode_entries())),
                ]),
                &EXTRA_MAP.replace("<2019>", "<201D>"),
            ),
            // 0x80 is the internal minus slot, never WinAnsi's euro.
            1 => word_font(
                cmap(&[(3, 1, format6(&unicode_entries()))]),
                "1 beginbfchar <80> <2212> endbfchar",
            ),
            // The space and hyphen aliases stay unproven.
            2 => word_font(
                cmap(&[(3, 1, format6(&unicode_entries()))]),
                "1 beginbfchar <A0> <00A0> endbfchar",
            ),
            3 => word_font(
                cmap(&[(3, 1, format6(&unicode_entries()))]),
                "1 beginbfchar <AD> <00AD> endbfchar",
            ),
            // A second Unicode table that selects another glyph for U+2019.
            4 => word_font(
                cmap(&[
                    (0, 3, format6(&disagreeing)),
                    (3, 1, format6(&unicode_entries())),
                ]),
                EXTRA_MAP,
            ),
            // Only a Mac table: WinAnsi glyphs cannot be selected by Unicode.
            _ => word_font(cmap(&[(1, 0, format0(&mac_entries()))]), EXTRA_MAP),
        };
        assert!(textedit::scan(&doc, 0).is_err(), "accepted defect {defect}");
        let objects = doc.objects.clone();
        assert!(
            textedit::write(&mut doc, std::slice::from_ref(&update)).is_err(),
            "defect {defect}"
        );
        assert_eq!(doc.objects, objects, "defect {defect}");
    }
    // Control: the same Unicode table alone, without the Mac map, is accepted,
    // so defect 5 fails for the missing Unicode table and not for the map.
    let (doc, _) = word_font(cmap(&[(3, 1, format6(&unicode_entries()))]), EXTRA_MAP);
    assert!(textedit::scan(&doc, 0).is_ok());
}
