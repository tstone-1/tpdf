use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Stream};

const NORMAL: &[u8] = include_bytes!("fixtures/normal.cff");

#[test]
fn textedit_font_refusals_identify_program_carriers_without_echoing_values() {
    for (carrier, subtype, expected) in [
        (
            // A FontFile is read as a Type 1 program now; this one has no
            // Length1/Length2, and the refusal still names no value.
            "FontFile",
            "SYNTHETIC_SECRET",
            "unsupported embedded Type 1 font",
        ),
        (
            "FontFile2",
            "SYNTHETIC_SECRET",
            "FontFile2 is not supported in Type1 fonts",
        ),
        (
            "FontFile3",
            "OpenType",
            "OpenType programs in Type1 fonts are not editable yet",
        ),
        (
            "FontFile3",
            "SYNTHETIC_SECRET",
            "unsupported embedded program subtype in Type1 font",
        ),
    ] {
        let (mut doc, _, descriptor, program) = fixture(NORMAL);
        let before = textedit::scan(&doc, 0).unwrap();
        let fd = doc.get_dictionary_mut(descriptor).unwrap();
        fd.remove(b"FontFile3");
        fd.set(carrier, program);
        doc.get_object_mut(program)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Subtype", subtype);
        let unchanged = doc.objects.clone();
        let edit = Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: before.runs[0].text.clone(),
            replacement: "EDITED FIRST".into(),
        };
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected);
        assert_eq!(textedit::write(&mut doc, &[edit]).unwrap_err(), expected);
        assert_eq!(doc.objects, unchanged);
    }
}

#[test]
fn textedit_font_refusals_separate_missing_conflicting_and_invalid_programs() {
    // Without a program the font is read by its PDF widths, as an unembedded
    // font (see fonts::unembedded).
    let (mut doc, _, descriptor, _) = fixture(NORMAL);
    doc.get_dictionary_mut(descriptor)
        .unwrap()
        .remove(b"FontFile3");
    assert_eq!(
        textedit::scan(&doc, 0).unwrap().runs[0].text,
        "SYNTHETIC FIRST"
    );
    for case in 1..5 {
        let (mut doc, _, descriptor, program) = fixture(NORMAL);
        let expected = match case {
            1 => {
                doc.get_dictionary_mut(descriptor)
                    .unwrap()
                    .set("FontFile", program);
                "Type1 font has conflicting embedded font programs"
            }
            2 => {
                doc.get_dictionary_mut(descriptor)
                    .unwrap()
                    .set("FontFile2", Object::Null);
                "Type1 font has conflicting embedded font programs"
            }
            3 => {
                doc.get_dictionary_mut(descriptor)
                    .unwrap()
                    .set("FontFile3", Object::Null);
                "invalid embedded program in Type1 font"
            }
            _ => {
                doc.get_object_mut(program)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap()
                    .dict
                    .remove(b"Subtype");
                "unsupported embedded program subtype in Type1 font"
            }
        };
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected);
    }
    // A genuinely CFF program still reaches the CFF validator.
    let (doc, _, _, _) = fixture(b"SYNTHETIC INVALID CFF");
    assert!(textedit::scan(&doc, 0).unwrap_err().contains("CFF"));
}

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
                layout: None,
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
        include_bytes!("fixtures/unknown-postscript.cff"),
    ] {
        let (doc, font, _, _) = fixture(bytes);
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
    // A program whose rights forbid editing is read and measured, and never
    // written in.
    let (doc, font, _, _) = fixture(include_bytes!("fixtures/preview-only.cff"));
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.is_restricted());
    assert!(metrics.advance("A", 12.).is_ok());
    assert!(metrics.writes_space());
    assert!(metrics.encode("A").unwrap_err().contains("does not permit"));
    let (doc, font, _, _) = fixture(NORMAL);
    assert!(!embedded(&doc, doc.get_dictionary(font).unwrap())
        .unwrap()
        .is_restricted());
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
                layout: None,
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
                layout: None,
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
                layout: None,
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

fn unicode_fixture() -> (Document, lopdf::ObjectId) {
    let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/unicode.cff"));
    let f = doc.get_dictionary_mut(font).unwrap();
    f.set("FirstChar", 26);
    f.set("LastChar", 163);
    f.set("Widths", vec![Object::Integer(600); 138]);
    f.set(
        "Encoding",
        dictionary! { "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![26.into(), "minus".into(), "uni00A0".into()] },
    );
    let mapping = doc.add_object(unicode("<1a> <2212> <1b> <00a0> <91> <2018> <92> <2019> <96> <2013> <a3> <00a3> <20> <0020> <41> <0041> <42> <0042>", 9));
    doc.get_dictionary_mut(font)
        .unwrap()
        .set("ToUnicode", mapping);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.add_object(Stream::new(
        Dictionary::new(),
        b"1 Tw BT /F1 12 Tf 40 180 Td <1a1b919296a3> Tj ET BT /F1 12 Tf 40 140 Td (AB) Tj ET"
            .to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    (doc, font)
}

#[test]
fn textedit_cff_unicode_roundtrip_preserves_codes_ink_and_resources() {
    let (mut doc, font) = unicode_fixture();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let original = "\u{2212}\u{a0}\u{2018}\u{2019}\u{2013}£";
    assert_eq!(
        metrics.decode(&[26, 27, 145, 146, 150, 163]).unwrap(),
        original
    );
    assert_eq!(
        metrics.encode(original).unwrap(),
        [26, 27, 145, 146, 150, 163]
    );
    assert_eq!(
        metrics.horizontal_bounds("\u{2212}", 1000.).unwrap(),
        [-20., 620.]
    );
    assert_eq!(
        metrics.horizontal_bounds("\u{a0}", 1000.).unwrap(),
        [0., 600.]
    );
    assert_eq!(
        metrics.spaced_layout(" \u{a0}", 1000., 1., 2.).unwrap().0,
        1204.
    );
    let before = textedit::scan(&doc, 0).unwrap();
    assert_eq!(before.runs[0].text, original);
    assert!((before.runs[0].advance - 43.2).abs() < 0.00001); // NBSP is not byte 32.
    let objects = doc.objects.clone();
    let change = Change {
        layout: None,
        page: 0,
        revision: before.revision,
        operator: before.runs[0].operator,
        original: original.into(),
        replacement: "£\u{2013}\u{2019}\u{a0}\u{2212}".into(),
    };
    textedit::write(&mut doc, std::slice::from_ref(&change)).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, change.replacement);
    assert_eq!(after.runs[1], before.runs[1]);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let ops = lopdf::content::Content::decode_strict(&doc.get_page_content(page)).unwrap();
    assert_eq!(
        ops.operations[change.operator as usize].operands[0]
            .as_str()
            .unwrap(),
        [163, 150, 146, 27, 26]
    );
    for (id, object) in objects {
        if id != page {
            assert_eq!(doc.objects[&id], object);
        }
    }
}

#[test]
fn textedit_cff_unicode_requires_exact_glyph_names_widths_and_maps() {
    let (mut doc, font) = unicode_fixture();
    let descriptor = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"FontDescriptor")
        .unwrap()
        .as_reference()
        .unwrap();
    let program = doc
        .get_dictionary(descriptor)
        .unwrap()
        .get(b"FontFile3")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = NORMAL.to_vec();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.encode("\u{a0}").is_err()); // Existing space is not a substitute.
    for replacement in ["space", "uni00a0", "uni2212", "nonbreakingspace", "f_i"] {
        let (mut doc, font) = unicode_fixture();
        doc.get_dictionary_mut(font).unwrap().set("Encoding", dictionary! {
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![26.into(), "minus".into(), 27.into(), Object::Name(replacement.as_bytes().to_vec())]
        });
        assert!(
            embedded(&doc, doc.get_dictionary(font).unwrap()).is_err(),
            "{replacement}"
        );
    }
    let (mut doc, font) = unicode_fixture();
    doc.get_dictionary_mut(font)
        .unwrap()
        .get_mut(b"Widths")
        .unwrap()
        .as_array_mut()
        .unwrap()[0] = 599.into();
    // One font-unit tolerance is intentional; the next unit must be refused.
    assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_ok());
    doc.get_dictionary_mut(font)
        .unwrap()
        .get_mut(b"Widths")
        .unwrap()
        .as_array_mut()
        .unwrap()[0] = 598.into();
    assert!(embedded(&doc, doc.get_dictionary(font).unwrap())
        .err()
        .unwrap()
        .contains("widths disagree"));
}

#[test]
fn textedit_cff_unicode_missing_maps_glyphs_and_unmapped_fonts_never_guess() {
    let (mut doc, font) = unicode_fixture();
    doc.get_dictionary_mut(font).unwrap().remove(b"ToUnicode");
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    for (text, code) in [
        ("\u{2212}", 26),
        ("\u{2018}", 145),
        ("\u{2019}", 146),
        ("\u{2013}", 150),
        ("£", 163),
    ] {
        assert_eq!(metrics.encode(text).unwrap(), [code]);
        assert_eq!(metrics.decode(&[code]).unwrap(), text);
    }
    assert!(metrics.encode("\u{a0}").is_err());
    assert!(metrics.decode(&[27]).is_err());
    let program = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"FontDescriptor")
        .unwrap()
        .as_reference()
        .unwrap();
    let program = doc
        .get_dictionary(program)
        .unwrap()
        .get(b"FontFile3")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = NORMAL.to_vec();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    for text in [
        "\u{2212}", "\u{a0}", "\u{2018}", "\u{2019}", "\u{2013}", "£",
    ] {
        assert!(metrics.encode(text).is_err());
    }
    for text in ["\u{2212}", "\u{2018}", "\u{2019}"] {
        assert!(super::super::Metrics::helvetica().encode(text).is_err());
        assert!(super::super::Metrics::helvetica_default()
            .encode(text)
            .is_err());
    }
}

fn ligature_fixture(body: &[u8]) -> (Document, lopdf::ObjectId) {
    let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/ligatures.cff"));
    let f = doc.get_dictionary_mut(font).unwrap();
    f.set("FirstChar", 28);
    f.set("Widths", vec![Object::Integer(600); 99]);
    f.set("Encoding", dictionary! {
        "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![28.into(), "f_l".into(), "f_f".into(), "f_i".into(), "f_f_i".into()]
    });
    let mut entries = (32..=126)
        .map(|code| format!("<{code:02x}> <{code:04x}> "))
        .collect::<String>();
    entries += "<1c> <0066006c> <1d> <00660066> <1e> <00660069> <1f> <006600660069>";
    let map = doc.add_object(unicode(&entries, 99));
    doc.get_dictionary_mut(font).unwrap().set("ToUnicode", map);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.add_object(Stream::new(Dictionary::new(), body.to_vec()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    (doc, font)
}

#[test]
fn textedit_ligatures_measure_original_glyphs_and_encode_longest_existing_match() {
    let (doc, font) = ligature_fixture(b"");
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(
        metrics.decode(&[31, 32, 29, 32, 30, 32, 28]).unwrap(),
        "ffi ff fi fl"
    );
    assert_eq!(
        metrics.encode("ffi ff fi fl").unwrap(),
        [31, 32, 29, 32, 30, 32, 28]
    );
    assert_eq!(metrics.encode("fffi").unwrap(), [29, 30]);
    assert_eq!(metrics.advance("ffi", 1000.).unwrap(), 600.);
    assert_eq!(metrics.horizontal_bounds("ffi", 1000.).unwrap(), [0., 600.]);
    assert_eq!(
        metrics.spaced_layout("ffi", 1000., 2., 3.).unwrap(),
        (602., [0., 600.])
    );
    assert_eq!(
        metrics.source_layout(b"ffi", 1000., 2., 3.).unwrap(),
        ("ffi".into(), 1806., [0., 1804.])
    );
    assert_eq!(
        metrics.source_layout(&[31], 1000., 2., 3.).unwrap(),
        ("ffi".into(), 602., [0., 600.])
    );
    assert_eq!(
        metrics.source_layout(b"ffi", 1000., 0., 0.).unwrap().1,
        1800.
    );
    for ch in ["\u{1}", "\u{2}", "\u{3}", "\u{4}", "\u{fb01}", "\u{fb03}"] {
        assert!(metrics.encode(ch).is_err());
    }
    assert_eq!(
        super::super::Metrics::helvetica().encode("ffi").unwrap(),
        b"ffi"
    );
}

// Older CFF fonts (the AutoCAD brochure's Artifakt subsets) name their
// ligatures by Adobe's original `fi`/`fl`, not `f_i`/`f_l`. Same glyphs,
// same slots; generated by testdata/make_cff_legacy_ligatures.py.
#[test]
fn textedit_cff_ligatures_accept_adobe_original_names() {
    let (mut doc, font) = ligature_fixture(b"");
    let program = doc
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_stream().is_ok_and(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type1C")
            })
        })
        .map(|(id, _)| *id)
        .unwrap();
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = include_bytes!("fixtures/legacy-ligatures.cff").to_vec();
    let differences = |names: [&str; 4]| {
        dictionary! {
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![28.into(), names[0].into(), names[1].into(), names[2].into(), names[3].into()]
        }
    };
    doc.get_dictionary_mut(font)
        .unwrap()
        .set("Encoding", differences(["fl", "ff", "fi", "ffi"]));
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(
        metrics.decode(&[31, 32, 29, 32, 30, 32, 28]).unwrap(),
        "ffi ff fi fl"
    );
    assert_eq!(
        metrics.encode("ffi ff fi fl").unwrap(),
        [31, 32, 29, 32, 30, 32, 28]
    );
    // Against this program the newer spelling names no glyph, so those codes
    // are not offered, as for any glyph a subset lacks; `fi` is then written
    // as two letters.
    doc.get_dictionary_mut(font)
        .unwrap()
        .set("Encoding", differences(["f_l", "f_f", "f_i", "f_f_i"]));
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.decode(&[30]).is_err());
    assert_eq!(metrics.encode("fi").unwrap(), b"fi");
}

#[test]
fn textedit_ligatures_preserve_source_fragment_boundaries_and_other_operators() {
    let body = b"1 Tc 2 Tw BT /F1 12 Tf 40 180 Td [(f) 0 (fi) 0 <1f>] TJ ET BT /F1 12 Tf 40 140 Td (ffi) Tj ET";
    let (mut doc, _) = ligature_fixture(body);
    let before = textedit::scan(&doc, 0).unwrap();
    assert_eq!(before.runs[0].text, "ffiffi");
    assert!((before.runs[0].advance - 32.8).abs() < 1e-6); // Four glyphs, not six letters or two ligatures.
    assert!((before.runs[1].advance - 24.6).abs() < 1e-6); // Three separate glyphs.
    let objects = doc.objects.clone();
    let change = Change {
        layout: None,
        page: 0,
        revision: before.revision,
        operator: before.runs[0].operator,
        original: "ffiffi".into(),
        replacement: "ffi fi".into(),
    };
    textedit::write(&mut doc, std::slice::from_ref(&change)).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "ffi fi");
    assert!((after.runs[0].advance - 26.6).abs() < 1e-6); // Three glyphs, one Tw space.
    assert_eq!(after.runs[1], before.runs[1]);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let ops = lopdf::content::Content::decode_strict(&doc.get_page_content(page)).unwrap();
    assert_eq!(
        ops.operations[change.operator as usize].operands[0],
        Object::Array(vec![Object::string_literal(vec![31, 32, 30])])
    );
    for (id, object) in objects {
        if id != page {
            assert_eq!(doc.objects[&id], object);
        }
    }
}

#[test]
fn textedit_ligatures_refuse_overflow_expansion_and_stale_edits_atomically() {
    let (mut doc, font) = ligature_fixture(b"BT /F1 12 Tf 40 180 Td <1f> Tj ET");
    let before = textedit::scan(&doc, 0).unwrap();
    let objects = doc.objects.clone();
    for (original, replacement) in [("ffi", "ffl"), ("ffi", "f f"), ("ff", "fi")] {
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[0].operator,
                original: original.into(),
                replacement: replacement.into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let count = textedit::MAX_TEXT / 3;
    assert_eq!(metrics.decode(&vec![31; count]).unwrap().len(), count * 3);
    assert!(metrics.decode(&vec![31; count + 1]).is_err());
    assert!(metrics.decode(&vec![b'A'; textedit::MAX_TEXT + 1]).is_err());
}

#[test]
fn textedit_ligatures_require_matching_maps_and_existing_outlines() {
    let (doc, font) = ligature_fixture(b"");
    let map = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"ToUnicode")
        .unwrap()
        .as_reference()
        .unwrap();
    for (from, to) in [
        ("<00660069>", "<0066006c>"), // Different glyph, duplicate target.
        ("<00660069>", "<fb01>"),     // Compatibility character is not the declared sequence.
        ("<00660069>", "<00660066006c>"),
        ("<1f>", "<1e>"),
        ("<00660069>", "<006600>"),
    ] {
        let mut bad = doc.clone();
        let stream = bad.get_object_mut(map).unwrap().as_stream_mut().unwrap();
        let text = String::from_utf8(stream.content.clone()).unwrap();
        assert!(text.contains(from));
        stream.content = text.replace(from, to).into_bytes();
        assert!(
            embedded(&bad, bad.get_dictionary(font).unwrap()).is_err(),
            "{from} -> {to}"
        );
    }
    // An identical duplicate would otherwise be overwritten without changing
    // the later glyph-name agreement check.
    let mut duplicate = doc.clone();
    let stream = duplicate
        .get_object_mut(map)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let text = String::from_utf8(stream.content.clone()).unwrap();
    stream.content = text
        .replace("99 beginbfchar", "100 beginbfchar")
        .replace("endbfchar", "<1f> <006600660069> endbfchar")
        .into_bytes();
    assert!(embedded(&duplicate, duplicate.get_dictionary(font).unwrap()).is_err());
    let mut missing = doc.clone();
    missing
        .get_dictionary_mut(font)
        .unwrap()
        .remove(b"ToUnicode");
    let metrics = embedded(&missing, missing.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.decode(&[31]).is_err());
    assert_eq!(metrics.encode("ffi").unwrap(), b"ffi");
    let descriptor = doc
        .get_dictionary(font)
        .unwrap()
        .get(b"FontDescriptor")
        .unwrap()
        .as_reference()
        .unwrap();
    let program = doc
        .get_dictionary(descriptor)
        .unwrap()
        .get(b"FontFile3")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut absent = doc;
    absent
        .get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = NORMAL.to_vec();
    let metrics = embedded(&absent, absent.get_dictionary(font).unwrap()).unwrap();
    assert!(metrics.decode(&[31]).is_err());
    assert_eq!(metrics.encode("ffi").unwrap(), b"ffi");
}

#[test]
fn textedit_ligatures_count_word_spacing_by_original_pdf_code() {
    let (mut doc, font) = ligature_fixture(b"");
    let f = doc.get_dictionary_mut(font).unwrap();
    f.set(
        "Encoding",
        dictionary! { "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![31.into(), "space".into(), "f_f_i".into()] },
    );
    let map = doc.add_object(unicode("<1f> <0020> <20> <006600660069>", 2));
    doc.get_dictionary_mut(font).unwrap().set("ToUnicode", map);
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(metrics.encode("ffi ").unwrap(), [32, 31]);
    assert_eq!(
        metrics.spaced_layout("ffi ", 1000., 1., 2.).unwrap().0,
        1204.
    );
    assert_eq!(metrics.source_layout(b" ", 1000., 1., 2.).unwrap().1, 603.);
    assert_eq!(metrics.source_layout(&[31], 1000., 1., 2.).unwrap().1, 601.);
}

// xdvipdfmx's TeX fonts: symbolic, and often with no PDF Encoding, so the
// program's own encoding names each code's glyph (read through `type1`). This
// fixture swaps A and B in that encoding.
#[test]
fn textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules() {
    let content = |doc: &mut Document| {
        let page = crate::pagetree::ordered_pages(doc)[0];
        let stream = doc.add_object(Stream::new(
            Dictionary::new(),
            b"BT /F1 12 Tf 40 180 Td (SYNTHETIC AB) Tj ET BT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET".to_vec(),
        ));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
    };
    let first = |doc: &Document| textedit::scan(doc, 0).unwrap().runs[0].text.clone();
    let builtin = include_bytes!("fixtures/builtin-encoding.cff");
    for (flags, encoding, expected) in [
        (32, true, "SYNTHETIC AB"),
        (4, true, "SYNTHETIC AB"),
        (4, false, "SYNTHETIC BA"),
        (32, false, "SYNTHETIC BA"),
    ] {
        let (mut doc, font, descriptor, _) = fixture(builtin);
        content(&mut doc);
        doc.get_dictionary_mut(descriptor)
            .unwrap()
            .set("Flags", flags);
        if !encoding {
            doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
        }
        assert_eq!(first(&doc), expected, "{flags} {encoding}");
    }
    let (mut doc, font, _, _) = fixture(builtin);
    content(&mut doc);
    doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
    let runs = textedit::scan(&doc, 0).unwrap();
    let edit = Change {
        layout: None,
        page: 0,
        revision: runs.revision,
        operator: runs.runs[0].operator,
        original: runs.runs[0].text.clone(),
        replacement: "SYNTHETIC B".into(),
    };
    textedit::write(&mut doc, &[edit]).unwrap();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let saved = lopdf::content::Content::decode(&doc.get_page_content(page)).unwrap();
    assert!(saved
        .operations
        .iter()
        .any(|op| op.operands.first().and_then(|o| o.as_str().ok()) == Some(b"SYNTHETIC A")));
    // StandardEncoding as the built-in encoding reads the ordinary letters.
    let (mut doc, font, _, _) = fixture(NORMAL);
    doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
    assert_eq!(first(&doc), "SYNTHETIC FIRST");
    // The Expert encoding has no Latin letters to offer.
    let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/expert-encoding.cff"));
    doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "unsupported CFF built-in encoding"
    );
    // A space whose outline does not validate is left out, so text using it
    // cannot be measured.
    let (mut doc, font, _, _) = fixture(include_bytes!("fixtures/broken-space.cff"));
    doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
    assert!(textedit::scan(&doc, 0).is_err());
}
