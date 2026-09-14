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
