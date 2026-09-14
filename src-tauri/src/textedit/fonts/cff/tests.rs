use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Stream};

const NORMAL: &[u8] = include_bytes!("fixtures/normal.cff");

fn fixture(bytes: &[u8]) -> (Document, lopdf::ObjectId, lopdf::ObjectId, lopdf::ObjectId) {
    let (mut doc, font, descriptor, program) = super::super::tests::fixture();
    let f = doc.get_dictionary_mut(font).unwrap();
    f.set("Subtype", "Type1");
    f.set("BaseFont", "TPDFSyntheticCFF");
    f.set("LastChar", 126);
    f.set("Widths", vec![Object::Integer(600); 95]);
    let fd = doc.get_dictionary_mut(descriptor).unwrap();
    fd.set("FontName", "TPDFSyntheticCFF");
    fd.remove(b"FontFile2");
    fd.set("FontFile3", program);
    doc.objects.insert(
        program,
        Object::Stream(Stream::new(
            dictionary! {"Subtype" => "Type1C"},
            bytes.to_vec(),
        )),
    );
    (doc, font, descriptor, program)
}

#[test]
fn textedit_cff_maps_ascii_by_glyph_name_and_preserves_resources() {
    for bytes in [
        NORMAL,
        include_bytes!("fixtures/expert-encoding.cff"),
        include_bytes!("fixtures/editable-rights.cff"),
    ] {
        let (mut doc, font, _, _) = fixture(bytes);
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        let ascii: String = (32..=126).map(char::from).collect();
        assert_eq!(metrics.decode(ascii.as_bytes()).unwrap(), ascii);
        assert_eq!(metrics.encode(&ascii).unwrap(), ascii.as_bytes());
        assert_eq!(metrics.advance(&ascii, 1000.).unwrap(), 95. * 600.);
        assert!(metrics.encode("é").is_err());
        assert!(metrics.decode(&[127]).is_err());
        let runs = textedit::scan(&doc, 0).unwrap();
        let before = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: runs.revision,
                operator: runs.runs[0].operator,
                original: runs.runs[0].text.clone(),
                replacement: "EDITED FIRST".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "EDITED FIRST");
        assert_eq!(after.runs[1], runs.runs[1]);
        let page = crate::pagetree::ordered_pages(&doc)[0];
        for (id, value) in before {
            if id != page {
                assert_eq!(doc.objects[&id], value);
            }
        }
    }
}

#[test]
fn textedit_cff_refuses_unvalidated_program_semantics_and_permissions() {
    for bytes in [
        include_bytes!("fixtures/matrix.cff").as_slice(),
        include_bytes!("fixtures/paint.cff"),
        include_bytes!("fixtures/charstring.cff"),
        include_bytes!("fixtures/preview-only.cff"),
        include_bytes!("fixtures/unknown-postscript.cff"),
    ] {
        let (doc, font, _, _) = fixture(bytes);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
    let (doc, font, _, _) = fixture(include_bytes!("fixtures/preview-only.cff"));
    assert!(embedded(&doc, doc.get_dictionary(font).unwrap())
        .err()
        .unwrap()
        .contains("does not permit"));
    for length in [0, 3, 8, NORMAL.len() / 2] {
        let (doc, font, _, _) = fixture(&NORMAL[..length]);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
}

#[test]
fn textedit_cff_missing_glyph_and_malformed_space_are_not_offered() {
    let (doc, font, _, _) = fixture(include_bytes!("fixtures/missing-A.cff"));
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.advance("B", 12.).is_ok());
    assert!(metrics.encode("A").is_err());
    assert!(metrics.advance("A", 12.).is_err());
    let bytes = include_bytes!("fixtures/broken-space.cff");
    let face = Table::parse(bytes).unwrap();
    let glyph = face.glyph_index_by_name("space").unwrap();
    assert_eq!(face.glyph_width(glyph), Some(600)); // This alone cannot prove a blank glyph.
    assert!(super::super::outlines::cff_bounds(&face, glyph).is_err());
    let (doc, font, _, _) = fixture(bytes);
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.advance("B", 12.).is_ok());
    assert!(metrics.encode(" ").is_err());
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_cff_bounds_replacement_ink_and_preserves_failed_document() {
    let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/overhang.cff"));
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(metrics.horizontal_bounds("A", 1000.).unwrap(), [-20., 600.]);
    let runs = textedit::scan(&doc, 0).unwrap();
    let before = doc.objects.clone();
    for replacement in ["A", "é", "SYNTHETIC FIRST FIRST FIRST"] {
        assert!(textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: runs.revision.clone(),
                operator: runs.runs[0].operator,
                original: runs.runs[0].text.clone(),
                replacement: replacement.into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, before);
    }
}

#[test]
fn textedit_cff_requires_matching_pdf_font_contract() {
    for (key, value) in [
        ("Encoding", Object::Name(b"MacRomanEncoding".to_vec())),
        ("ToUnicode", Object::Null),
        ("Widths", Object::Array(vec![])),
        ("FirstChar", Object::Integer(-1)),
        ("LastChar", Object::Integer(256)),
    ] {
        let (mut doc, font, _, _) = fixture(NORMAL);
        doc.get_dictionary_mut(font).unwrap().set(key, value);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
    for (key, value) in [
        ("Flags", Object::Integer(4)),
        ("FontName", Object::Name(b"OTHER".to_vec())),
        ("FontFile2", Object::Null),
    ] {
        let (mut doc, font, descriptor, _) = fixture(NORMAL);
        doc.get_dictionary_mut(descriptor).unwrap().set(key, value);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
    let (mut doc, font, _, program) = fixture(NORMAL);
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Subtype", "OpenType");
    assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    let (mut doc, font, _, _) = fixture(NORMAL);
    doc.get_dictionary_mut(font)
        .unwrap()
        .get_mut(b"Widths")
        .unwrap()
        .as_array_mut()
        .unwrap()[b'A' as usize - 32] = Object::Integer(1000);
    assert!(embedded(&doc, doc.get_dictionary(font).unwrap())
        .err()
        .unwrap()
        .contains("widths disagree"));
}

fn remapped(with_unicode: bool) -> (Document, lopdf::ObjectId) {
    let (mut doc, font, _, _) = fixture(NORMAL);
    let encoding = doc.add_object(dictionary! {
        "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![32.into(), "S".into(), 83.into(), "space".into()]
    });
    doc.get_dictionary_mut(font)
        .unwrap()
        .set("Encoding", encoding);
    if with_unicode {
        let entries = (32..=126)
            .map(|code| {
                let target = match code {
                    32 => 83,
                    83 => 32,
                    other => other,
                };
                format!("<{code:02X}> <{target:04X}>\n")
            })
            .collect::<String>();
        let mapping = unicode(&entries, 95);
        let id = doc.add_object(mapping);
        doc.get_dictionary_mut(font).unwrap().set("ToUnicode", id);
    }
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = b"1 Tw BT /F1 12 Tf 40 180 Td ( YNTHETICSFIR T) Tj ET BT /F1 12 Tf 40 140 Td ( YNTHETICS ECOND) Tj ET";
    let id = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
    doc.get_dictionary_mut(page).unwrap().set("Contents", id);
    (doc, font)
}

fn unicode(entries: &str, count: usize) -> Stream {
    Stream::new(Dictionary::new(), format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange {count} beginbfchar {entries} endbfchar endcmap CMapName currentdict /CMap defineresource pop end end").into_bytes())
}

#[test]
fn textedit_cff_custom_encoding_roundtrips_original_codes_and_word_spacing() {
    for with_unicode in [false, true] {
        let (mut doc, font) = remapped(with_unicode);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs[0].text, "SYNTHETIC FIRST");
        // Byte 32 is S, so Tw applies twice, not once at the Unicode space.
        assert!((before.runs[0].advance - (15. * 7.2 + 2.)).abs() < 0.0001);
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        assert_eq!(metrics.encode("S ").unwrap(), b" S");
        assert_eq!(metrics.horizontal_bounds(" ", 1000.).unwrap(), [0., 600.]);
        let objects = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: before.runs[0].text.clone(),
                replacement: "EDITED FIRST".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "EDITED FIRST");
        assert_eq!(after.runs[1], before.runs[1]);
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let ops = lopdf::content::Content::decode_strict(&doc.get_page_content(page)).unwrap();
        assert_eq!(
            ops.operations[before.runs[0].operator as usize].operands[0]
                .as_str()
                .unwrap(),
            b"EDITEDSFIR T"
        );
        for (id, value) in objects {
            if id != page {
                assert_eq!(doc.objects[&id], value);
            }
        }
    }
}

#[test]
fn textedit_cff_custom_encoding_refuses_malformed_or_ambiguous_differences() {
    let cases = [
        vec!["A".into()],
        vec![32.into()],
        vec![32.into(), 33.into(), "A".into()],
        vec![(-1).into(), "A".into()],
        vec![256.into(), "A".into()],
        vec![255.into(), "A".into(), "B".into()],
        vec![32.into(), Object::string_literal("space")],
        vec![32.into(), "space".into(), 32.into(), "space".into()],
        vec![32.into(), "f_i".into()],
        vec![32.into(), "uni0041".into()],
        vec![32.into(), "B".into()], // B would have two codes with one metric slot.
        vec![Object::Null; 513],
    ];
    for differences in cases {
        let (mut doc, font, _, _) = fixture(NORMAL);
        doc.get_dictionary_mut(font).unwrap().set(
            "Encoding",
            dictionary! {
                "BaseEncoding" => "WinAnsiEncoding", "Differences" => differences
            },
        );
        let before = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err());
        assert!(textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: vec![],
                operator: 3,
                original: "SYNTHETIC FIRST".into(),
                replacement: "EDITED FIRST".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, before);
    }
    for encoding in [
        dictionary! {},
        dictionary! {"BaseEncoding" => "MacRomanEncoding"},
        dictionary! {"BaseEncoding" => "WinAnsiEncoding", "Type" => "Font"},
        dictionary! {"BaseEncoding" => "WinAnsiEncoding", "Unknown" => 1},
        dictionary! {"BaseEncoding" => "WinAnsiEncoding", "Differences" => Object::Null},
    ] {
        let (mut doc, font, _, _) = fixture(NORMAL);
        doc.get_dictionary_mut(font)
            .unwrap()
            .set("Encoding", encoding);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
}

#[test]
fn textedit_cff_tounicode_must_match_glyphs_and_never_guesses_missing_entries() {
    for entries in [
        "<20> <0020>",
        "<41> <0042>",
        "<01> <0041>",
        "<20> <00660069>",
    ] {
        let (mut doc, font) = remapped(false);
        let id = doc.add_object(unicode(entries, 1));
        doc.get_dictionary_mut(font).unwrap().set("ToUnicode", id);
        assert!(
            embedded(&doc, doc.get_dictionary(font).unwrap()).is_err(),
            "{entries}"
        );
    }
    let (mut doc, font) = remapped(false);
    let id = doc.add_object(unicode("<20> <0053>", 1));
    doc.get_dictionary_mut(font).unwrap().set("ToUnicode", id);
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(metrics.decode(b" ").unwrap(), "S");
    assert_eq!(metrics.encode("S").unwrap(), b" ");
    assert!(metrics.encode("A").is_err());
    assert!(metrics.decode(b"A").is_err());
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_cff_remapped_metrics_follow_glyphs_at_both_code_boundaries() {
    for code in [0_u8, 255] {
        let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/overhang.cff"));
        let font = doc.get_dictionary_mut(font).unwrap();
        font.set("FirstChar", 0);
        font.set("LastChar", 255);
        font.set("Widths", vec![Object::Integer(600); 256]);
        font.set("Encoding", dictionary! {
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![Object::Integer(i64::from(code)), "A".into(), 65.into(), ".notdef".into()]
        });
        let font = font.clone();
        let metrics = embedded(&doc, &font).unwrap();
        assert_eq!(metrics.encode("A").unwrap(), [code]);
        assert_eq!(metrics.decode(&[code]).unwrap(), "A");
        assert!(metrics.decode(b"A").is_err());
        assert_eq!(metrics.horizontal_bounds("A", 1000.).unwrap(), [-20., 600.]);
        assert_eq!(metrics.horizontal_bounds("B", 1000.).unwrap(), [0., 600.]);
        let mut font = font;
        font.get_mut(b"Widths").unwrap().as_array_mut().unwrap()[usize::from(code)] =
            Object::Integer(700);
        assert!(embedded(&doc, &font)
            .err()
            .unwrap()
            .contains("widths disagree"));
    }
}
