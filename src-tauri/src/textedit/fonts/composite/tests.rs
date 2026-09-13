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
fn textedit_composite_refusals_leave_every_object_unchanged() {
    for (scope, key, value) in [
        (0, "Encoding", Object::Name(b"Identity-V".to_vec())),
        (0, "ToUnicode", Object::Null),
        (0, "Extra", true.into()),
        (0, "DescendantFonts", Vec::<Object>::new().into()),
        (1, "Subtype", Object::Name(b"CIDFontType0".to_vec())),
        (1, "BaseFont", Object::Name(b"Other".to_vec())),
        (1, "CIDToGIDMap", Object::Name(b"Other".to_vec())),
        (1, "W2", Vec::<Object>::new().into()),
        (1, "DW", 601.01.into()),
        (1, "DW", 0.into()),
        (1, "CIDSystemInfo", dictionary! {"Registry" => Object::string_literal("Other"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0}.into()),
        (2, "Flags", 32.into()),
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

#[test]
fn textedit_composite_width_table_bounds_and_defaults() {
    assert_eq!(widths(&dictionary! {}).unwrap().0, 1000.);
    let good = dictionary! { "W" => vec![0.into(), vec![0.into(), 600.into()].into(), 2.into(), 4095.into(), 600.into()] };
    let parsed = widths(&good).unwrap();
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
        assert!(widths(&dictionary! { "W" => values }).is_err());
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
