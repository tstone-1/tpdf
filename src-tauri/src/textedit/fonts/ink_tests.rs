use super::*;
use crate::textedit::{self, Change};
use lopdf::{content::Content, ObjectId, Stream};

// Turn B into a fractionally scaled A and D into a descending A. Keep the
// original table allocations; the component ends before their padding. Zero
// header bboxes deliberately disagree with the actual component outlines.
pub(in crate::textedit) fn program() -> Vec<u8> {
    component_program([('B', 0, 0, 18729), ('D', 0, -100, 16384)])
}

pub(in crate::textedit) fn component_program(components: [(char, i16, i16, i16); 2]) -> Vec<u8> {
    with_components(include_bytes!("../synthetic.ttf").to_vec(), components)
}

fn with_components(mut bytes: Vec<u8>, components: [(char, i16, i16, i16); 2]) -> Vec<u8> {
    let table = |tag: &[u8]| {
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let record = bytes[12..12 + count * 16]
            .chunks_exact(16)
            .find(|r| &r[..4] == tag)
            .unwrap();
        u32::from_be_bytes(record[8..12].try_into().unwrap()) as usize
    };
    let glyf = table(b"glyf");
    let loca = table(b"loca");
    // Fixture mappings may use symbolic bytes, but glyph order is shared with
    // the original geometric program. Do not infer glyph IDs from PDF codes.
    let face = Face::parse(include_bytes!("../synthetic.ttf"), 0).unwrap();
    assert_eq!(
        face.tables().head.index_to_location_format,
        ttf_parser::head::IndexToLocationFormat::Short
    );
    let a = face.glyph_index('A').unwrap().0;
    let ids =
        components.map(|(ch, x, y, scale)| (face.glyph_index(ch).unwrap().0 as usize, x, y, scale));
    for (id, x, y, scale) in ids {
        let start = glyf
            + usize::from(u16::from_be_bytes(
                bytes[loca + id * 2..loca + id * 2 + 2].try_into().unwrap(),
            )) * 2;
        let end = glyf
            + usize::from(u16::from_be_bytes(
                bytes[loca + id * 2 + 2..loca + id * 2 + 4]
                    .try_into()
                    .unwrap(),
            )) * 2;
        // Composite, false bbox, word XY arguments, independent X/Y scale.
        let words = [-1_i16, 0, 0, 0, 0, 0x43, a as i16, x, y, 16384, scale];
        let replacement: Vec<_> = words.iter().flat_map(|n| n.to_be_bytes()).collect();
        assert!(replacement.len() <= end - start);
        bytes[start..end].fill(0);
        bytes[start..start + replacement.len()].copy_from_slice(&replacement);
    }
    bytes
}

fn set_content(doc: &mut Document, prefix: &str, encoded: &[u8]) {
    let hex = encoded
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let body = format!("{prefix} BT /F1 12 Tf 40 180 Td <{hex}> Tj ET");
    let id = crate::pagetree::ordered_pages(doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.into_bytes()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
}

pub(in crate::textedit) fn exercise(mut doc: Document, font: ObjectId, program_id: ObjectId) {
    doc.get_object_mut(program_id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content = program();
    let dict = doc.get_dictionary(font).unwrap();
    let metrics = if dict.get(b"Subtype").unwrap().as_name().unwrap() == b"Type0" {
        composite(&doc, dict).unwrap()
    } else {
        embedded(&doc, dict).unwrap()
    };
    let top = 700. * 18729. / 16384.;
    assert_eq!(metrics.vertical_bounds, Some([-100., top]));
    let encoded = metrics.encode("A").unwrap();
    set_content(&mut doc, "", &encoded);
    let plain = textedit::scan(&doc, 0).unwrap().runs.remove(0);
    let control = doc.clone();
    // Full-em upper bound is 192; the complete offered glyph set fits below 190.
    for replacement in ["B", "D", " "] {
        let mut doc = control.clone();
        set_content(&mut doc, "0 178 300 12 re W n", &encoded);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs[0].display_rect, plain.display_rect);
        assert_eq!(before.runs[0].matrix, plain.matrix);
        let objects = doc.objects.clone();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let old = Content::decode_strict(&doc.get_page_content(page)).unwrap();
        let change = Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "A".into(),
            replacement: replacement.into(),
        };
        textedit::write(&mut doc, std::slice::from_ref(&change)).unwrap();
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, replacement);
        let new = Content::decode_strict(&doc.get_page_content(page)).unwrap();
        assert_eq!(old.operations.len(), new.operations.len());
        for (i, (old, new)) in old.operations.iter().zip(new.operations).enumerate() {
            assert_eq!(old.operator, new.operator);
            if i != change.operator as usize {
                assert_eq!(old.operands, new.operands);
            }
        }
        for (id, object) in objects {
            if id != page {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
    // A fits each rectangle, but an offered replacement would escape: B at the
    // top, D at the bottom. The last rectangle exposes integer truncation (800
    // versus 800.1892 font units), rather than a missing whole-glyph union.
    for rect in ["0 178 300 11", "0 179 300 11", "0 178 300 11.601"] {
        let mut doc = control.clone();
        set_content(&mut doc, &format!("{rect} re W n"), &encoded);
        let objects = doc.objects.clone();
        textedit::tests::clipped_roundtrip(&doc);
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: Vec::new(),
                operator: 0,
                original: "A".into(),
                replacement: "B".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
    // The established clip is in page coordinates. Scale the glyphs twofold
    // about the same baseline: missing that scale would accept each bad clip.
    for (rect, accepted) in [
        ("0 177 300 23", true),
        ("0 170 300 29", false),
        ("0 178 300 32", false),
    ] {
        let mut doc = control.clone();
        set_content(
            &mut doc,
            &format!("{rect} re W n 1 0 0 2 0 -180 cm"),
            &encoded,
        );
        let clipped = textedit::tests::clipped_roundtrip(&doc).runs.remove(0);
        let mut plain = control.clone();
        set_content(&mut plain, "1 0 0 2 0 -180 cm", &encoded);
        assert_eq!(
            clipped.display_rect == textedit::scan(&plain, 0).unwrap().runs[0].display_rect,
            accepted,
            "scaled {rect}"
        );
    }
}

#[test]
fn textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements() {
    let (doc, font, _, program) = tests::fixture();
    exercise(doc, font, program);
}

#[test]
fn textedit_outline_bounds_preserve_fractional_components_and_ignore_header_boxes() {
    let bytes = program();
    let face = Face::parse(&bytes, 0).unwrap();
    let glyph = face.glyph_index('B').unwrap();
    assert_eq!(face.glyph_bounding_box(glyph).unwrap().y_max, 800);
    assert_eq!(
        outlines::bounds(&face, glyph),
        Some([0., 0., 400., 700. * 18729. / 16384.])
    );
}

fn simple_overhang_fixture(kind: u8, left: i16, right: i16) -> (Document, ObjectId) {
    let (mut doc, font, program) = match kind {
        0 => {
            let (doc, font, _, program) = tests::fixture();
            (doc, font, program)
        }
        1 => tests::mac_fixture(true),
        _ => tests::custom_fixture(),
    };
    let stream = doc
        .get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = with_components(
        stream.content.clone(),
        [('B', left, 0, 16384), ('D', right, 0, 16384)],
    );
    (doc, font)
}

#[test]
fn textedit_simple_overhang_tracks_unicode_ink_and_preserves_resources() {
    for kind in 0..3 {
        let (source, font) = simple_overhang_fixture(kind, -10, 250);
        let metrics = embedded(&source, source.get_dictionary(font).unwrap()).unwrap();
        assert_eq!(metrics.horizontal_bounds("B", 1000.).unwrap(), [-10., 600.]);
        assert_eq!(metrics.horizontal_bounds("D", 1000.).unwrap(), [0., 650.]);
        for (text, prefix, accepted) in [
            ("BA", "40 170 30 20 re W n", false),
            ("BA", "39 170 30 20 re W n", true),
            ("AD", "40 170 14.4 20 re W n", false),
            ("AD", "40 170 15.1 20 re W n", true),
            ("AD", "80 170 28.8 20 re W n 2 0 0 1 0 0 cm", false),
            ("AD", "80 170 30.1 20 re W n 2 0 0 1 0 0 cm", true),
        ] {
            let mut doc = source.clone();
            set_content(&mut doc, prefix, &metrics.encode(text).unwrap());
            let clipped = textedit::tests::clipped_roundtrip(&doc).runs.remove(0);
            let mut plain = source.clone();
            set_content(&mut plain, "", &metrics.encode(text).unwrap());
            let plain = textedit::scan(&plain, 0).unwrap().runs.remove(0);
            if !prefix.contains("cm") {
                assert_eq!(clipped.display_rect == plain.display_rect, accepted);
            }
        }
        for (replacement, accepted) in [("BA", false), ("AAD", false), ("AD", true), ("ABA", true)]
        {
            let mut doc = source.clone();
            set_content(
                &mut doc,
                "40 170 21.6 20 re W n",
                &metrics.encode("AAA").unwrap(),
            );
            let scanned = textedit::scan(&doc, 0).unwrap();
            let before = doc.objects.clone();
            let result = textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: scanned.revision,
                    operator: scanned.runs[0].operator,
                    original: "AAA".into(),
                    replacement: replacement.into(),
                }],
            );
            assert_eq!(
                result.is_ok(),
                accepted,
                "kind={kind} {replacement}: {result:?}"
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
            }
        }
    }
}

#[test]
fn textedit_simple_overhang_quarter_em_boundaries() {
    for kind in 0..3 {
        for (left, right, valid_b, valid_d) in [
            (-250, 450, true, true),
            (-251, 450, false, true),
            (-250, 451, true, false),
        ] {
            let (doc, font) = simple_overhang_fixture(kind, left, right);
            let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
            assert_eq!(metrics.advance("B", 10.).is_ok(), valid_b);
            assert_eq!(metrics.advance("D", 10.).is_ok(), valid_d);
        }
    }
}

#[test]
fn textedit_truetype_descenders_extend_hit_bounds_and_keep_a_finite_limit() {
    for bottom in [-300, -500, -501] {
        let (mut doc, font, _, program) = super::tests::fixture();
        doc.get_object_mut(program)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content = component_program([('B', 0, bottom, 16384), ('D', 0, -100, 16384)]);
        let metrics = embedded(&doc, doc.get_dictionary(font).unwrap()).unwrap();
        assert_eq!(metrics.advance("B", 12.).is_ok(), bottom >= -500);
        if bottom < -500 {
            continue;
        }
        set_content(&mut doc, "", b"B");
        let before = textedit::scan(&doc, 0).unwrap();
        // 240pt page, 180pt baseline: a 12pt glyph descends 3.6 or 6 points.
        let expected_bottom = if bottom == -300 { 63.6 } else { 66. };
        assert!((before.runs[0].display_rect[3] - expected_bottom).abs() < 0.001);
        let height = before.runs[0].minimum_height.unwrap();
        assert!((height - (expected_bottom as f64 - 48.)).abs() < 0.001);
        textedit::write(
            &mut doc,
            &[Change {
                layout: Some(textedit::Layout {
                    width: before.runs[0].advance,
                    height,
                    size: 12.,
                    wrap: false,
                    font: textedit::EditFont::Original,
                }),
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "B".into(),
                replacement: "D".into(),
            }],
        )
        .unwrap();
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "D");
    }
}
