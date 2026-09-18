use super::*;
use crate::textedit::{self, Change, EditFont, Layout};
use lopdf::{dictionary, ObjectId, Stream};

const MAP: &str = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 3 beginbfchar <01> <0041> <20> <0042> <7F> <4E2D> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end";

fn fixture() -> (Document, ObjectId, ObjectId, ObjectId) {
    let mut doc = textedit::tests::fixture();
    let glyph = doc.add_object(Stream::new(
        Dictionary::new(),
        b"600 0 0 -700 500 0 d1 0 0 m 500 0 l 500 -700 l 0 -700 l h f".to_vec(),
    ));
    let cmap = doc.add_object(Stream::new(Dictionary::new(), MAP.as_bytes().to_vec()));
    let mut widths = vec![Object::Integer(0); 127];
    for code in [1, 32, 127] {
        widths[code - 1] = 600.into();
    }
    let font=doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type3", "FirstChar"=>1, "LastChar"=>127,
        "FontMatrix"=>vec![0.001.into(),0.into(),0.into(),(-0.001).into(),0.into(),0.into()],
        "FontBBox"=>vec![0.into(),0.into(),500.into(),(-700).into()],
        "Widths"=>widths, "Encoding"=>dictionary! {"Type"=>"Encoding", "Differences"=>vec![1.into(),Object::Name(b"A".to_vec()),32.into(),Object::Name(b"B".to_vec()),127.into(),Object::Name(b"C".to_vec())]},
        "CharProcs"=>dictionary! {"A"=>glyph,"B"=>glyph,"C"=>glyph}, "ToUnicode"=>cmap,
    });
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let mut resources = textedit::resources(&doc, page).unwrap().clone();
    let mut fonts = dictionary(&doc, resources.get(b"Font").unwrap())
        .unwrap()
        .clone();
    fonts.set("T3", font);
    resources.set("Font", fonts);
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    let stream = doc.add_object(Stream::new(
        Dictionary::new(),
        b"BT /T3 12 Tf 40 180 Td <01207F> Tj <01> Tj ET BT /F1 12 Tf 40 120 Td (FIRST) Tj ET"
            .to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    (doc, font, glyph, cmap)
}

#[test]
fn type3_edit_delete_and_layout_preserve_fonts_and_following_text() {
    for replacement in ["A", "\u{4e2d}", ""] {
        for layout in [
            None,
            Some(Layout {
                width: 21.6,
                height: 15.,
                size: 12.,
                wrap: false,
                font: EditFont::Original,
            }),
        ] {
            let (mut doc, _, _, _) = fixture();
            let before = textedit::scan(&doc, 0).unwrap();
            assert_eq!(before.runs[0].text, "AB\u{4e2d}");
            assert!((before.runs[0].advance - 21.6).abs() < 1e-5);
            let objects = doc.objects.clone();
            textedit::write(
                &mut doc,
                &[Change {
                    page: 0,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: before.runs[0].text.clone(),
                    replacement: replacement.into(),
                    layout,
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, replacement);
            let follower = after
                .runs
                .iter()
                .find(|r| r.text == "A" && r.matrix[4] > 60.)
                .unwrap();
            for (a, b) in follower.matrix.iter().zip(before.runs[1].matrix) {
                assert!((a - b).abs() < 1e-5);
            }
            let page = crate::pagetree::ordered_pages(&doc)[0];
            for (id, object) in objects {
                if id != page {
                    assert_eq!(doc.objects[&id], object);
                }
            }
        }
    }
}

#[test]
fn type3_single_byte_word_spacing_and_curve_bounds_are_measured() {
    let (mut doc, font, glyph, _) = fixture();
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let (text, advance, _) = metrics.source_layout(&[1, 32, 127], 10., 0., 7.).unwrap();
    assert_eq!(text, "AB\u{4e2d}");
    assert!((advance - 25.).abs() < 1e-5);
    assert_eq!(metrics.encode(&text).unwrap(), vec![1, 32, 127]);
    assert!(metrics.encode("UNAVAILABLE").is_err());
    // The declared bbox deliberately understates both the control-point hull
    // and the actual curve. Bounds must come from the program too.
    doc.objects.insert(
        glyph,
        Stream::new(
            Dictionary::new(),
            b"600 0 0 -700 500 0 d1 0 0 m -40 -1000 600 -1000 500 0 c h f".to_vec(),
        )
        .into(),
    );
    let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
    let (advance, bounds) = metrics.spaced_layout("A", 10., 0., 0.).unwrap();
    assert!((advance - 6.).abs() < 1e-5);
    assert!(bounds[0] < -0.39);
    assert!(metrics.vertical_bounds.unwrap()[1] > 999.);
}

#[test]
fn type3_header_is_literal_bounded_and_preserves_comments() {
    let (mut doc, _, glyph, _) = fixture();
    for bytes in [
        "600 0 0 -700 500 0 d1 0 0 m 500 -700 l f",
        "% d1 in a comment\r\n+600% width\n0 0 -700.0 500 0\td1% paint\n0 0 m 500 -700 l f",
    ] {
        doc.objects.insert(
            glyph,
            Stream::new(Dictionary::new(), bytes.as_bytes().to_vec()).into(),
        );
        let result = outline(&doc, &Object::Reference(glyph), &mut 65536, &mut 16);
        assert!(result.is_ok(), "{bytes}: {result:?}");
        assert!(outline(&doc, &Object::Reference(glyph), &mut 65536, &mut 3).is_err());
    }
    for header in [
        "600 0 0 -700 500 0 dX",
        "600 0 0 -700 500 0 d10",
        "600 0 0 -700 500 0 /d1",
        "600 0 0 -700 500 d1",
        "600 0 0 -700 500 0 1 d1",
        "600 0 0 -700 500 0 d1 600 0 0 -700 500 0 d1",
        "6e2 0 0 -700 500 0 d1",
        "600 0 0 -700 500 ..0 d1",
    ] {
        let bytes = format!("{header} 0 0 m 500 -700 l f").into_bytes();
        doc.objects
            .insert(glyph, Stream::new(Dictionary::new(), bytes).into());
        assert!(
            outline(&doc, &Object::Reference(glyph), &mut 65536, &mut 64).is_err(),
            "{header}"
        );
    }
}

#[test]
fn type3_rejects_program_side_effects_bad_metrics_maps_and_limits_atomically() {
    let (original, font, glyph, cmap) = fixture();
    let good = textedit::scan(&original, 0).unwrap();
    let edit = Change {
        page: 0,
        revision: good.revision,
        operator: good.runs[0].operator,
        original: good.runs[0].text.clone(),
        replacement: "A".into(),
        layout: None,
    };
    let mut cases = Vec::new();
    for bytes in [
        "600 0 d0 0 0 m 10 10 l f",
        "601 0 0 -700 500 0 d1 0 0 m 10 10 l f",
        "600 1 0 -700 500 0 d1 0 0 m 10 10 l f",
        "600 0 0 -700 500 0 d1 /X Do",
        "600 0 0 -700 500 0 d1 BT (HIDDEN) Tj ET",
        "600 0 0 -700 500 0 d1 1 0 0 rg 0 0 m 10 10 l f",
        "600 0 0 -700 500 0 d1 0 0 m 10 10 l S",
        "600 0 0 -700 500 0 d1 0 0 m 10 10 l h",
        "600 0 0 -700 500 0 d1 0 0 m 10 10 l f q",
        "600 0 0 -700 500 0 d1 0 0 m 10 10 l f 1",
        "600 0 0 -700 500 0 d1 0 0 m 1000001 0 l f",
    ] {
        let mut doc = original.clone();
        doc.objects.insert(
            glyph,
            Stream::new(Dictionary::new(), bytes.as_bytes().to_vec()).into(),
        );
        cases.push(doc);
    }
    for key in ["FontMatrix", "Widths", "Encoding", "ToUnicode", "CharProcs"] {
        let mut doc = original.clone();
        doc.get_dictionary_mut(font).unwrap().set(key, Object::Null);
        cases.push(doc);
    }
    // Two codes may read as one text; the page still scans.
    let mut doc = original.clone();
    doc.objects.insert(
        cmap,
        Stream::new(
            Dictionary::new(),
            MAP.replace("<0042>", "<0041>").into_bytes(),
        )
        .into(),
    );
    assert!(textedit::scan(&doc, 0).is_ok());
    for text in [
        MAP.replace("<20>", "<21>"),
        MAP.replace("<01>", "<0001>"),
        MAP.replace("<0042>", "<D800>"),
    ] {
        let mut doc = original.clone();
        doc.objects.insert(
            cmap,
            Stream::new(Dictionary::new(), text.into_bytes()).into(),
        );
        cases.push(doc);
    }
    let mut doc = original.clone();
    doc.objects.insert(
        glyph,
        Stream::new(Dictionary::new(), vec![b' '; 65537]).into(),
    );
    cases.push(doc);
    for mut doc in cases {
        let before = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err());
        assert!(textedit::write(&mut doc, std::slice::from_ref(&edit)).is_err());
        assert_eq!(doc.objects, before);
    }
}
