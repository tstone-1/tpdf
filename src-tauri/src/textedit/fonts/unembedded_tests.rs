// Word leaves Arial and Times New Roman unembedded. Such a font is read by its
// PDF widths, as standard Helvetica is by its built-in ones.
use super::tests::fixture;
use crate::textedit::{self, Change};
use lopdf::{Document, Object, ObjectId};

fn unembedded() -> (Document, ObjectId, ObjectId) {
    let (mut doc, font, descriptor, _) = fixture();
    doc.get_dictionary_mut(descriptor)
        .unwrap()
        .remove(b"FontFile2");
    (doc, font, descriptor)
}

#[test]
fn textedit_unembedded_fonts_edit_by_their_pdf_widths() {
    let (mut doc, font, _) = unembedded();
    // Distinct widths, so the advance can only come from the Widths array.
    let widths = (32..=89)
        .map(|code| Object::Integer(500 + code))
        .collect::<Vec<_>>();
    doc.get_dictionary_mut(font).unwrap().set("Widths", widths);
    let scan = textedit::scan(&doc, 0).unwrap();
    assert_eq!(scan.runs[0].text, "SYNTHETIC FIRST");
    let expected: i64 = "SYNTHETIC FIRST"
        .bytes()
        .map(|code| 500 + i64::from(code))
        .sum();
    assert!((scan.runs[0].advance - expected as f64 * 12. / 1000.).abs() < 1e-9);
    let before = doc.objects.clone();
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: "SYNTHETIC FIRST".into(),
            replacement: "SYNTHETIC FIR".into(),
        }],
    )
    .unwrap();
    assert_eq!(
        textedit::scan(&doc, 0).unwrap().runs[0].text,
        "SYNTHETIC FIR"
    );
    assert_eq!(doc.objects[&font], before[&font]);
    // A code with no width is not offered, and neither is one past LastChar.
    let (mut doc, font, _) = unembedded();
    let mut widths = vec![Object::Integer(600); 58];
    widths[usize::from(b'X' - 32)] = 0.into();
    doc.get_dictionary_mut(font).unwrap().set("Widths", widths);
    let scan = textedit::scan(&doc, 0).unwrap();
    for replacement in ["SYNTHETIC X", "SYNTHETIC ~"] {
        let result = textedit::write(
            &mut doc.clone(),
            &[Change {
                layout: None,
                page: 0,
                revision: scan.revision.clone(),
                operator: scan.runs[0].operator,
                original: "SYNTHETIC FIRST".into(),
                replacement: replacement.into(),
            }],
        );
        assert!(result.is_err(), "{replacement}");
    }
}

// With no outlines, the descriptor's FontBBox is the ink every glyph stays
// inside, so text in the font can still be kept read-only beside an edit.
#[test]
fn textedit_unembedded_font_box_bounds_read_only_text() {
    let (mut doc, _, descriptor) = unembedded();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        b"BT /F1 12 Tf 1 0.5 0 1 40 100 Tm (SYNTHETIC SECOND) Tj ET BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    let runs = textedit::scan(&doc, 0).unwrap().runs;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "SYNTHETIC FIRST");
    // The box sets the run's hit area: from its bottom to its top at 12 pt.
    doc.get_dictionary_mut(descriptor).unwrap().set(
        "FontBBox",
        vec![0.into(), (-500).into(), 400.into(), 1500.into()],
    );
    let tall = textedit::scan(&doc, 0).unwrap().runs[0].display_rect;
    let rect = runs[0].display_rect;
    assert!(tall[3] - tall[1] > rect[3] - rect[1], "{tall:?} {rect:?}");
}

#[test]
fn textedit_unembedded_fonts_require_a_plain_winansi_contract() {
    for case in 0..11 {
        let (mut doc, font, descriptor) = unembedded();
        match case {
            0 => doc.get_dictionary_mut(descriptor).unwrap().set("Flags", 4),
            1 => doc
                .get_dictionary_mut(font)
                .unwrap()
                .set("Encoding", "MacRomanEncoding"),
            2 => {
                doc.get_dictionary_mut(font).unwrap().remove(b"Encoding");
            }
            3 => doc
                .get_dictionary_mut(font)
                .unwrap()
                .set("ToUnicode", Object::Null),
            4 => doc
                .get_dictionary_mut(descriptor)
                .unwrap()
                .set("FontName", "Other"),
            5 => doc
                .get_dictionary_mut(font)
                .unwrap()
                .set("Widths", vec![Object::Integer(600); 57]),
            6 => doc
                .get_dictionary_mut(font)
                .unwrap()
                .set("Widths", vec![Object::Integer(2001); 58]),
            7 => doc.get_dictionary_mut(font).unwrap().set("FirstChar", 300),
            8 => doc
                .get_dictionary_mut(descriptor)
                .unwrap()
                .set("FontBBox", vec![0.into(), 0.into(), 400.into()]),
            9 => doc
                .get_dictionary_mut(descriptor)
                .unwrap()
                .set("FontBBox", vec![400.into(), 0.into(), 0.into(), 700.into()]),
            _ => doc.get_dictionary_mut(descriptor).unwrap().set(
                "FontBBox",
                vec![0.into(), 0.into(), 400.into(), 4001.into()],
            ),
        }
        assert!(textedit::scan(&doc, 0).is_err(), "case {case}");
    }
    // Control: the unchanged fixture is accepted.
    assert!(textedit::scan(&unembedded().0, 0).is_ok());
}
