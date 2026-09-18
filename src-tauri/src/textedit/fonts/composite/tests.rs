use super::*;
use crate::textedit::{self, Change};
use lopdf::{content::Content, dictionary, ObjectId, Stream};

fn fixture() -> (Document, [ObjectId; 4]) {
    let (mut doc, font, descriptor, program) = super::super::tests::fixture();
    let face = ttf_parser::Face::parse(
        doc.get_object(program)
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .as_slice(),
        0,
    )
    .unwrap();
    let codes: BTreeMap<u16, u8> = (32..=126)
        .filter_map(|ch| face.glyph_index(char::from(ch)).map(|g| (g.0, ch)))
        .collect();
    let map = format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange {} beginbfchar {} endbfchar endcmap CMapName currentdict /CMap defineresource pop end end", codes.len(), codes.iter().map(|(code, ch)| format!("<{code:04x}> <00{ch:02x}>")).collect::<Vec<_>>().join(" "));
    let encode = |text: &[u8]| -> Vec<u8> {
        text.iter()
            .flat_map(|ch| {
                codes
                    .iter()
                    .find(|(_, v)| *v == ch)
                    .unwrap()
                    .0
                    .to_be_bytes()
            })
            .collect()
    };
    let pages = crate::pagetree::ordered_pages(&doc);
    let mut content = Content::decode(&doc.get_page_content(pages[0])).unwrap();
    for op in &mut content.operations {
        if op.operator == "Tj" {
            op.operands[0] = Object::string_literal(encode(op.operands[0].as_str().unwrap()));
        }
    }
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
    for page in pages {
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
    }
    doc.get_dictionary_mut(descriptor).unwrap().set("Flags", 4);
    let child = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "TPDFSynthetic",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
        "FontDescriptor" => descriptor, "CIDToGIDMap" => "Identity", "DW" => 600
    });
    let mapping = doc.add_object(Stream::new(Dictionary::new(), map.into_bytes()));
    doc.objects.insert(font, dictionary! { "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "TPDFSynthetic", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(child)], "ToUnicode" => mapping }.into());
    (doc, [font, child, descriptor, mapping])
}

fn update(doc: &Document, replacement: &str) -> Change {
    Change {
        replacement: replacement.into(),
        ..textedit::tests::change(doc)
    }
}

#[test]
fn ideographic_space_requires_a_verified_empty_glyph() {
    for (mapped, accepted) in [("3000", true), ("4e00", false)] {
        let (mut doc, [font, _, _, mapping]) = fixture();
        let stream = doc
            .get_object_mut(mapping)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        stream.content = String::from_utf8(stream.content.clone())
            .unwrap()
            .replace("> <0020>", &format!("> <{mapped}>"))
            .into_bytes();
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        let text = char::from_u32(u32::from_str_radix(mapped, 16).unwrap())
            .unwrap()
            .to_string();
        assert_eq!(metrics.encode(&text).is_ok(), accepted);
        if accepted {
            let before = textedit::scan(&doc, 0).unwrap();
            assert!(before.runs[0].text.contains('\u{3000}'));
            let change = update(&doc, "IN\u{3000}IN");
            let fonts = doc.objects[&font].clone();
            textedit::write(&mut doc, &[change]).unwrap();
            assert_eq!(
                textedit::scan(&doc, 0).unwrap().runs[0].text,
                "IN\u{3000}IN"
            );
            assert_eq!(doc.objects[&font], fonts);
        }
    }
}

#[test]
fn textedit_composite_nonsymbolic_flags_and_indirect_widths_preserve_glyphs() {
    let (mut doc, ids) = fixture();
    let expected = textedit::scan(&doc, 0).unwrap();
    doc.get_dictionary_mut(ids[2]).unwrap().set("Flags", 32);
    let list = doc.add_object(Object::Array(vec![600.into(); 32]));
    let widths = doc.add_object(Object::Array(vec![1.into(), list.into()]));
    doc.get_dictionary_mut(ids[1]).unwrap().set("W", widths);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, expected.runs);
    let change = update(&doc, "IN");
    let objects = doc.objects.clone();
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    for id in [ids[0], ids[1], ids[2], list, widths] {
        assert_eq!(doc.objects[&id], objects[&id]);
    }
    doc.objects.insert(widths, Object::Reference(widths));
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn unicode_cid_replacements_preserve_glyph_identity_and_other_runs() {
    let (mut doc, [font, _, _, mapping]) = fixture();
    let stream = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    // Relabel a synthetic geometric glyph; no real document or installed font.
    let map = String::from_utf8(stream.content.clone())
        .unwrap()
        .replace("> <0053>", "> <4e00>");
    stream.content = map.into_bytes();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let source = textedit::scan(&doc, 0).unwrap();
    assert!(source.runs[0].text.contains('\u{4e00}'));
    let bytes = metrics.encode("\u{4e00}YN").unwrap();
    assert_eq!(
        metrics.source_layout(&bytes, 12., 0., 0.).unwrap().0,
        "\u{4e00}YN"
    );
    let before_font = doc.objects[&font].clone();
    let change = Change {
        layout: None,
        replacement: "\u{4e00}YN".into(),
        original: source.runs[0].text.clone(),
        page: 0,
        revision: source.revision,
        operator: source.runs[0].operator,
    };
    textedit::write(&mut doc, &[change]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "\u{4e00}YN");
    assert_eq!(after.runs[1], source.runs[1]);
    assert_eq!(doc.objects[&font], before_font);
    assert!(metrics.encode("\u{4e01}").is_err());
}

#[test]
fn textedit_spacing_counts_decoded_cids_and_keeps_font_resources() {
    let (mut doc, ids) = fixture();
    let original = textedit::scan(&doc, 0).unwrap();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let mut content = b"0.5 Tc ".to_vec();
    content.extend(doc.get_page_content(page));
    let stream = doc.add_object(Stream::new(Dictionary::new(), content));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let before = doc.objects.clone();
    let spaced = textedit::scan(&doc, 0).unwrap();
    for (old, new) in original.runs.iter().zip(&spaced.runs) {
        assert!((new.advance - old.advance - old.text.chars().count() as f64 * 0.5).abs() < 1e-6);
    }
    let edit = update(&doc, "EDITED FIRST");
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "EDITED FIRST");
    assert!((after.runs[0].advance - 12. * (7.2 + 0.5)).abs() < 1e-6);
    assert_eq!(after.runs[1], spaced.runs[1]);
    for id in ids {
        assert_eq!(doc.objects[&id], before[&id]);
    }
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs, original.runs);
}

#[test]
fn textedit_composite_glyph_envelope_covers_fractional_and_unused_replacements() {
    let (doc, [font, _, descriptor, _]) = fixture();
    let program = doc
        .get_dictionary(descriptor)
        .unwrap()
        .get(b"FontFile2")
        .unwrap()
        .as_reference()
        .unwrap();
    super::super::ink_tests::exercise(doc, font, program);
}

#[test]
fn textedit_composite_roundtrip_preserves_program_mapping_and_other_page() {
    for kerning in [false, true] {
        let (mut doc, [font, child, _, _]) = fixture();
        // Exercise explicit array and range W entries as well as DW below.
        doc.get_dictionary_mut(child).unwrap().set(
            "W",
            vec![
                1.into(),
                vec![600.into()].into(),
                2.into(),
                20.into(),
                600.into(),
            ],
        );
        let page = crate::pagetree::ordered_pages(&doc)[0];
        if kerning {
            let mut content = Content::decode(&doc.get_page_content(page)).unwrap();
            let op = &mut content.operations[3];
            assert_eq!(op.operator, "Tj");
            let bytes = op.operands[0].as_str().unwrap();
            op.operands = vec![vec![
                Object::string_literal(&bytes[..2]),
                0.into(),
                Object::string_literal(&bytes[2..]),
            ]
            .into()];
            op.operator = "TJ".into();
            let stream = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Contents", stream);
        }
        let before = doc.objects.clone();
        let old = textedit::scan(&doc, 0).unwrap();
        assert_eq!(old.runs[0].text, "SYNTHETIC FIRST");
        assert!((old.runs[0].advance - 108.).abs() < 0.0001);
        let other = textedit::scan(&doc, 1).unwrap();
        let change = update(&doc, "EDITED FIRST");
        textedit::write(&mut doc, &[change]).unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "EDITED FIRST");
        assert_eq!(after.runs[1], old.runs[1]);
        assert_eq!(textedit::scan(&doc, 1).unwrap().runs, other.runs);
        for (id, value) in before {
            if id != page {
                assert_eq!(doc.objects[&id], value);
            }
        }
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        assert_eq!(metrics.encode("A").unwrap().len(), 2);
        assert_eq!(
            metrics.decode(&metrics.encode("AB").unwrap()).unwrap(),
            "AB"
        );
        let empty = update(&doc, "");
        textedit::write(&mut doc, &[empty]).unwrap();
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "");
    }
}

#[test]
fn textedit_composite_latin1_uses_existing_codes_and_preserves_resources() {
    let (mut doc, [font, _, _, mapping]) = fixture();
    // Assign two unused synthetic glyphs non-ASCII semantics. Real accented
    // outlines are checked independently with the unchanged browser export.
    let map = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let text = String::from_utf8(map.content.clone()).unwrap();
    assert_eq!(text.matches("<0041>").count(), 1);
    assert_eq!(text.matches("<0042>").count(), 1);
    map.content = text
        .replace("<0041>", "<00e4>")
        .replace("<0042>", "<00df>")
        .into_bytes();
    let before = doc.objects.clone();
    let pages = crate::pagetree::ordered_pages(&doc);
    let other = textedit::scan(&doc, 1).unwrap();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    assert_eq!(metrics.encode("äß").unwrap().len(), 4);
    assert_eq!(
        metrics.decode(&metrics.encode("äß").unwrap()).unwrap(),
        "äß"
    );
    assert!((metrics.advance("ä ß", 12.).unwrap() - 21.6).abs() < 0.0001);
    let limit = metrics.encode(&"ä".repeat(4096)).unwrap();
    assert_eq!(limit.len(), 8192);
    assert_eq!(metrics.decode(&limit).unwrap().chars().count(), 4096);
    assert!(metrics.encode(&"ä".repeat(4097)).is_err());
    for replacement in ["ö", "A", "ä".repeat(40).as_str()] {
        let change = update(&doc, replacement);
        assert!(textedit::write(&mut doc, &[change]).is_err());
        assert_eq!(doc.objects, before);
    }
    let change = update(&doc, "ä ß");
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "ä ß");
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs, other.runs);
    for (id, value) in before {
        if id != pages[0] {
            assert_eq!(doc.objects[&id], value);
        }
    }
}

#[test]
fn textedit_composite_refusals_leave_every_object_unchanged() {
    for (scope, key, value) in [
        (0, "Encoding", Object::Name(b"Identity-V".to_vec())),
        (0, "ToUnicode", Object::Null),
        (0, "Extra", true.into()),
        (0, "BaseFont", 1.into()),
        (0, "DescendantFonts", Vec::<Object>::new().into()),
        (1, "Subtype", Object::Name(b"CIDFontType0".to_vec())),
        (1, "BaseFont", Object::Name(b"Other".to_vec())),
        (1, "CIDToGIDMap", Object::Name(b"Other".to_vec())),
        (1, "W2", Vec::<Object>::new().into()),
        (1, "DW", 601.01.into()),
        (1, "DW", 0.into()),
        (1, "CIDSystemInfo", dictionary! {"Registry" => Object::string_literal("Other"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0}.into()),
        (2, "Flags", 36.into()),
        (2, "Flags", (32 | 262144).into()),
        (2, "FontName", Object::Name(b"Other".to_vec())),
        (2, "FontFile3", Object::Null),
    ] {
        let (mut doc, ids) = fixture();
        let change = update(&doc, "EDITED FIRST");
        doc.get_dictionary_mut(ids[scope]).unwrap().set(key, value);
        let before = doc.objects.clone();
        assert!(textedit::write(&mut doc, &[change]).is_err(), "{scope}/{key}");
        assert_eq!(doc.objects, before);
    }
    let (mut doc, _) = fixture();
    for replacement in ["Z", "ä", "SYNTHETIC FIRST FIRST"] {
        let change = update(&doc, replacement);
        let before = doc.objects.clone();
        assert!(textedit::write(&mut doc, &[change]).is_err());
        assert_eq!(doc.objects, before);
    }
}

// Merged documents keep one descendant under a Type0 font whose subset tag
// differs (UYTNFZ+SymbolMT over MQUKPZ+SymbolMT); the name selects nothing.
#[test]
fn textedit_composite_type0_name_may_differ_from_its_descendant() {
    let (mut doc, ids) = fixture();
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("BaseFont", Object::Name(b"ABCDEF+Other".to_vec()));
    let change = update(&doc, "EDITED FIRST");
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(
        doc.get_dictionary(ids[0])
            .unwrap()
            .get(b"BaseFont")
            .unwrap(),
        &Object::Name(b"ABCDEF+Other".to_vec())
    );
}

#[test]
fn textedit_composite_width_table_bounds_and_defaults() {
    assert_eq!(widths(&Document::new(), &dictionary! {}).unwrap().0, 1000.);
    let good = dictionary! { "W" => vec![0.into(), vec![0.into(), 600.into()].into(), 2.into(), 4095.into(), 600.into()] };
    let parsed = widths(&Document::new(), &good).unwrap();
    assert_eq!(parsed.1.len(), 4096);
    assert_eq!(parsed.1[&0], 0.);
    assert_eq!(parsed.1[&4095], 600.);
    for values in [
        vec![1.into()],
        vec![1.into(), 2.into()],
        vec![2.into(), 1.into(), 600.into()],
        vec![0.into(), 4096.into(), 600.into()],
        vec![65536.into(), 65536.into(), 600.into()],
        vec![65535.into(), vec![600.into(), 600.into()].into()],
        vec![1.into(), Vec::<Object>::new().into()],
        vec![1.into(), vec![600.into(); 4097].into()],
        vec![
            1.into(),
            3.into(),
            600.into(),
            3.into(),
            vec![600.into()].into(),
        ],
        vec![1.into(), vec![Object::Real(f32::NAN)].into()],
        vec![1.into(), vec![2001.into()].into()],
        vec![1.into(), vec![(-1).into()].into()],
        vec![
            0.into(),
            4095.into(),
            600.into(),
            4096.into(),
            vec![600.into()].into(),
        ],
    ] {
        assert!(widths(&Document::new(), &dictionary! { "W" => values }).is_err());
    }
}

#[test]
fn textedit_composite_code_lengths_notdef_and_glyph_bounds() {
    let (mut doc, [font, _, _, mapping]) = fixture();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    for bytes in [
        vec![1],
        vec![0, 0],
        vec![255, 255],
        metrics.encode("A").unwrap().repeat(4097),
    ] {
        assert!(metrics.decode(&bytes).is_err());
    }
    assert_eq!(
        metrics
            .decode(&metrics.encode(&"A".repeat(4096)).unwrap())
            .unwrap()
            .len(),
        4096
    );
    let source = doc
        .get_object(mapping)
        .unwrap()
        .as_stream()
        .unwrap()
        .content
        .clone();
    for code in ["0000", "ffff"] {
        let text = String::from_utf8(source.clone())
            .unwrap()
            .replace("<0001>", &format!("<{code}>"));
        doc.get_object_mut(mapping)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content = text.into_bytes();
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
}

#[test]
fn textedit_composite_checks_embedding_rights_and_program_format() {
    for rights in [0_u16, 8, 0x108, 2, 4, 0x200, 0xffff] {
        let (mut doc, [font, _, descriptor, _]) = fixture();
        let program = doc
            .get_dictionary(descriptor)
            .unwrap()
            .get(b"FontFile2")
            .unwrap()
            .as_reference()
            .unwrap();
        let bytes = &mut doc
            .get_object_mut(program)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content;
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let record = bytes[12..12 + count * 16]
            .chunks_exact(16)
            .find(|r| &r[..4] == b"OS/2")
            .unwrap();
        let offset = u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize;
        bytes[offset + 8..offset + 10].copy_from_slice(&rights.to_be_bytes());
        assert_eq!(
            embedded(&doc, doc.get_dictionary(font).unwrap()).is_ok(),
            [0, 8, 0x108].contains(&rights)
        );
    }
    for remove_os2 in [false, true] {
        let (mut doc, [font, _, descriptor, _]) = fixture();
        let program = doc
            .get_dictionary(descriptor)
            .unwrap()
            .get(b"FontFile2")
            .unwrap()
            .as_reference()
            .unwrap();
        let bytes = &mut doc
            .get_object_mut(program)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content;
        if remove_os2 {
            let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
            let record = bytes[12..12 + count * 16]
                .chunks_exact_mut(16)
                .find(|r| &r[..4] == b"OS/2")
                .unwrap();
            record[..4].copy_from_slice(b"NONE");
        } else {
            bytes[..4].copy_from_slice(b"true");
        }
        assert!(embedded(&doc, doc.get_dictionary(font).unwrap()).is_err());
    }
}

#[test]
fn textedit_composite_kerning_limit_counts_characters_across_strings() {
    for count in [4096, 4097] {
        let (mut doc, [font, _, _, _]) = fixture();
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        let code = metrics.encode("A").unwrap();
        let mut content = Content::decode(b"BT /F1 12 Tf 40 180 Td (A) Tj ET").unwrap();
        content.operations[3].operator = "TJ".into();
        content.operations[3].operands = vec![vec![
            Object::string_literal(code.repeat(2048)),
            0.into(),
            Object::string_literal(code.repeat(count - 2048)),
        ]
        .into()];
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
        let page = crate::pagetree::ordered_pages(&doc)[0];
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        let result = textedit::scan(&doc, 0);
        if count == 4096 {
            assert_eq!(result.unwrap().runs[0].text.len(), count);
        } else {
            assert!(result.unwrap_err().contains("kerning array text exceeds"));
        }
    }
}

// A is 400 units wide with a 600-unit advance. Shift B left and D right;
// neither header bbox describes the component's actual horizontal excursion.
fn overhang_fixture(left: i16, right: i16) -> (Document, ObjectId) {
    let (mut doc, [font, _, descriptor, _]) = fixture();
    let program = doc
        .get_dictionary(descriptor)
        .unwrap()
        .get(b"FontFile2")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content =
        super::super::ink_tests::component_program([('B', left, 0, 16384), ('D', right, 0, 16384)]);
    (doc, font)
}

fn overhang_content(doc: &mut Document, font: ObjectId, text: &str, prefix: &str, kerning: bool) {
    let metrics = embedded(doc, doc.get_dictionary(font).unwrap()).unwrap();
    let hex = |text: &str| {
        metrics
            .encode(text)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let show = if kerning {
        format!("[<{}> 100 <{}>] TJ", hex(&text[..1]), hex(&text[1..]))
    } else {
        format!("<{}> Tj", hex(text))
    };
    let body = format!("{prefix} BT /F1 10 Tf 40 180 Td {show} ET");
    let page = crate::pagetree::ordered_pages(doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.into_bytes()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
}

#[test]
fn textedit_composite_overhang_replacements_stay_inside_original_ink_atomically() {
    for kerning in [false, true] {
        let (mut source, font) = overhang_fixture(-10, 250);
        overhang_content(&mut source, font, "AAA", "40 170 18 20 re W n", kerning);
        for (replacement, accepted) in [
            ("ABA", !kerning),
            ("ADA", !kerning),
            ("BA", false),
            ("AAD", false),
            ("AD", true),
            ("", true),
        ] {
            let mut doc = source.clone();
            let before = doc.objects.clone();
            let scanned = textedit::scan(&doc, 0).unwrap();
            let edit = Change {
                layout: None,
                page: 0,
                revision: scanned.revision,
                operator: scanned.runs[0].operator,
                original: "AAA".into(),
                replacement: replacement.into(),
            };
            let result = textedit::write(&mut doc, &[edit]);
            // With TJ tightening, three unkerned letters exceed the advance first.
            assert_eq!(
                result.is_ok(),
                accepted,
                "{replacement} kerning={kerning}: {result:?}"
            );
            if accepted {
                assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, replacement);
                let page = crate::pagetree::ordered_pages(&doc)[0];
                for (id, value) in before {
                    if id != page {
                        assert_eq!(doc.objects[&id], value);
                    }
                }
            } else {
                assert_eq!(doc.objects, before);
                if !kerning {
                    assert!(result.unwrap_err().contains("replacement ink"));
                }
            }
        }
    }
    // Original overhang is legal without a clip and can be retained by an edit.
    let (mut doc, font) = overhang_fixture(-10, 250);
    overhang_content(&mut doc, font, "BAD", "", false);
    let scan = textedit::scan(&doc, 0).unwrap();
    assert!((scan.runs[0].display_rect[0] - 39.9).abs() < 0.00001);
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: "BAD".into(),
            replacement: "BD".into(),
        }],
    )
    .unwrap();
}

#[test]
fn textedit_composite_overhang_source_clips_follow_kerning_and_page_scale() {
    for (text, kerning, prefix, accepted) in [
        ("BA", false, "40 170 30 20 re W n", false),
        ("BA", false, "39 170 30 20 re W n", true),
        ("AD", false, "40 170 12 20 re W n", false),
        ("AD", false, "40 170 13 20 re W n", true),
        ("AD", true, "40 170 11 20 re W n", false),
        ("AD", true, "40 170 12 20 re W n", true),
        ("AD", false, "80 170 24 20 re W n 2 0 0 1 0 0 cm", false),
        ("AD", false, "80 170 25 20 re W n 2 0 0 1 0 0 cm", true),
        (
            "AD",
            false,
            "40 170 12 20 re W n -1 0 0 1 0 0 cm -1 0 0 1 0 0 cm",
            false,
        ),
    ] {
        let (mut doc, font) = overhang_fixture(-10, 250);
        overhang_content(&mut doc, font, text, prefix, kerning);
        let clipped = textedit::tests::clipped_roundtrip(&doc).runs.remove(0);
        let (mut plain, font) = overhang_fixture(-10, 250);
        overhang_content(&mut plain, font, text, "", kerning);
        let plain = textedit::scan(&plain, 0).unwrap().runs.remove(0);
        if !prefix.contains("cm") {
            assert_eq!(clipped.display_rect == plain.display_rect, accepted);
        }
    }
}

#[test]
fn textedit_composite_overhang_quarter_em_limits_have_boundary_controls() {
    for (left, right, valid_b, valid_d) in [
        (-250, 450, true, true),
        (-251, 450, false, true),
        (-250, 451, true, false),
    ] {
        let (doc, font) = overhang_fixture(left, right);
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        assert_eq!(metrics.advance("B", 10.).is_ok(), valid_b);
        assert_eq!(metrics.advance("D", 10.).is_ok(), valid_d);
        if valid_b {
            assert_eq!(metrics.horizontal_bounds("B", 10.).unwrap(), [-2.5, 6.]);
        }
        if valid_d {
            assert_eq!(metrics.horizontal_bounds("D", 10.).unwrap(), [0., 8.5]);
        }
    }
}

#[test]
fn textedit_cid_dash_keeps_two_byte_codes_and_font_data() {
    let (mut doc, [font, child, descriptor, mapping]) = fixture();
    let map = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let text = String::from_utf8(map.content.clone()).unwrap();
    assert_eq!(text.matches("<0042>").count(), 1);
    map.content = text.replace("<0042>", "<2013>").into_bytes();
    let objects = doc.objects.clone();
    let edit = update(&doc, "EDITED\u{2013}FIRST");
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "EDITED\u{2013}FIRST");
    assert_eq!(after.runs[1].text, "SYNTHETIC SECOND");
    for id in [font, child, descriptor, mapping] {
        assert_eq!(doc.objects[&id], objects[&id]);
    }
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = Content::decode_strict(&doc.get_page_content(page)).unwrap();
    let bytes = content
        .operations
        .iter()
        .find(|op| op.operator == "Tj")
        .unwrap()
        .operands[0]
        .as_str()
        .unwrap();
    assert_eq!(bytes.len(), 24);
    assert_ne!(&bytes[12..14], &[0x20, 0x13]);
}

#[test]
fn textedit_composite_indirect_descendants_preserve_resources_and_refuse_cycles() {
    let (mut doc, ids) = fixture();
    let before = textedit::scan(&doc, 0).unwrap();
    let children = doc
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"DescendantFonts")
        .unwrap()
        .clone();
    let array = doc.add_object(children);
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("DescendantFonts", array);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, before.runs);
    let original = doc.objects.clone();
    let edit = update(&doc, "EDITED FIRST");
    textedit::write(&mut doc, &[edit]).unwrap();
    assert_eq!(
        textedit::scan(&doc, 0).unwrap().runs[0].text,
        "EDITED FIRST"
    );
    for id in ids.into_iter().chain([array]) {
        assert_eq!(doc.objects[&id], original[&id]);
    }
    doc.objects.insert(array, Object::Reference(array));
    assert!(textedit::scan(&doc, 0).is_err());
    doc.objects.insert(array, Object::Null);
    assert!(textedit::scan(&doc, 0).is_err());
}

// Re-label our original geometric glyphs, not an installed font. Identity CID
// mapping uses the original glyph IDs and metrics, independently of cmap names.
fn ligature_fixture() -> (Document, ObjectId, BTreeMap<char, u16>) {
    let (mut doc, [font, _, _, mapping]) = fixture();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let Some(Codes::Double(codes)) = metrics.codes else {
        panic!("expected CID codes")
    };
    let source = codes
        .iter()
        .map(|(&code, &ch)| (char::from(ch), code))
        .collect();
    let stream = doc
        .get_object_mut(mapping)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    let mut map = String::from_utf8(stream.content.clone()).unwrap();
    for (from, to) in [
        ("0041", "006600660069"),
        ("0042", "00660066"),
        ("0043", "00660069"),
        ("0044", "0066006c"),
        ("0046", "0066"),
        ("0049", "0069"),
        ("004c", "006c"),
    ] {
        map = map.replace(&format!("> <{from}>"), &format!("> <{to}>"));
    }
    stream.content = map.into_bytes();
    (doc, font, source)
}

#[test]
fn textedit_cid_ligatures_measure_source_glyphs_and_bound_expansion() {
    let (doc, font, codes) = ligature_fixture();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let raw = |text: &str| {
        text.chars()
            .flat_map(|ch| codes[&ch].to_be_bytes())
            .collect::<Vec<_>>()
    };
    assert_eq!(metrics.decode(&raw("A B C D")).unwrap(), "ffi ff fi fl");
    assert_eq!(metrics.encode("ffi ff fi fl").unwrap(), raw("A B C D"));
    assert_eq!(metrics.encode("fffi").unwrap(), raw("BC"));
    assert_eq!(
        metrics.spaced_layout("ffi", 1000., 2., 3.).unwrap(),
        (602., [0., 600.])
    );
    assert_eq!(
        metrics.source_layout(&raw("FFI"), 1000., 2., 3.).unwrap(),
        ("ffi".into(), 1806., [0., 1804.])
    );
    assert_eq!(
        metrics.source_layout(&raw("A"), 1000., 2., 3.).unwrap(),
        ("ffi".into(), 602., [0., 600.])
    );
    assert_eq!(
        metrics.source_layout(&raw("A A"), 1000., 0., 3.).unwrap().1,
        1800.
    ); // Tw never applies to two-byte code 32.
    let count = textedit::MAX_TEXT / 3;
    assert_eq!(
        metrics.decode(&raw(&"A".repeat(count))).unwrap().len(),
        count * 3
    );
    assert!(metrics.decode(&raw(&"A".repeat(count + 1))).is_err());
    assert!(metrics.decode(&[0]).is_err());
    assert!(metrics.decode(&[255, 255]).is_err());
    for text in ["\u{1}", "\u{9f}", "\u{fb01}", "\u{fb03}"] {
        assert!(metrics.encode(text).is_err());
    }
}

#[test]
fn textedit_cid_ligatures_preserve_fragments_followers_and_resources() {
    let (mut doc, _, codes) = ligature_fixture();
    let raw = |text: &str| {
        text.chars()
            .flat_map(|ch| codes[&ch].to_be_bytes())
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let source = format!(
        "1 Tc 2 Tw BT /F1 12 Tf 40 180 Td [<{}> 0 <{}> 0 <{}>] TJ (<INVALID>) Tj ET",
        raw("F"),
        raw("FI"),
        raw("A")
    );
    let source = source.replace("(<INVALID>)", &format!("<{}>", raw("SECOND")));
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), source.into_bytes()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let before = textedit::scan(&doc, 0).unwrap();
    assert_eq!(before.runs[0].text, "ffiffi");
    assert!((before.runs[0].advance - 32.8).abs() < 1e-6);
    let objects = doc.objects.clone();
    let edit = Change {
        layout: None,
        page: 0,
        revision: before.revision.clone(),
        operator: before.runs[0].operator,
        original: "ffiffi".into(),
        replacement: "ffi fi".into(),
    };
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "ffi fi");
    assert_eq!(after.runs[1].text, before.runs[1].text);
    assert_eq!(after.runs[1].advance, before.runs[1].advance);
    assert_eq!(after.runs[1].display_rect, before.runs[1].display_rect);
    for (old, new) in before.runs[1].matrix.iter().zip(after.runs[1].matrix) {
        assert!((old - new).abs() < 1e-6);
    }
    for (id, object) in objects {
        if id != page {
            assert_eq!(doc.objects[&id], object);
        }
    }
    let snapshot = doc.objects.clone();
    let edit = Change {
        layout: None,
        page: 0,
        revision: after.revision,
        operator: after.runs[0].operator,
        original: "ffi fi".into(),
        replacement: "ffi fi ffi fi".into(),
    };
    assert!(textedit::write(&mut doc, &[edit]).is_err());
    assert_eq!(doc.objects, snapshot);
}
