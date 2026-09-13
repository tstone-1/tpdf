use super::*;
use crate::textedit::{self, Change};
use lopdf::{content::Content, ObjectId, Stream};

// Turn B into a fractionally scaled A and D into a descending A. Keep the
// original table allocations; the component ends before their padding. Zero
// header bboxes deliberately disagree with the actual component outlines.
pub(in crate::textedit) fn program() -> Vec<u8> {
    let mut bytes = include_bytes!("../synthetic.ttf").to_vec();
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
    let face = Face::parse(&bytes, 0).unwrap();
    assert_eq!(
        face.tables().head.index_to_location_format,
        ttf_parser::head::IndexToLocationFormat::Short
    );
    let a = face.glyph_index('A').unwrap().0;
    let ids = [('B', 0_i16, 18729_i16), ('D', -100, 16384)]
        .map(|(ch, y, scale)| (face.glyph_index(ch).unwrap().0 as usize, y, scale));
    for (id, y, scale) in ids {
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
        let words = [-1_i16, 0, 0, 0, 0, 0x43, a as i16, 0, y, 16384, scale];
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
        assert!(
            textedit::scan(&doc, 0)
                .unwrap_err()
                .contains("partly clipped"),
            "{rect}"
        );
        assert!(textedit::write(
            &mut doc,
            &[Change {
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
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "scaled {rect}");
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
