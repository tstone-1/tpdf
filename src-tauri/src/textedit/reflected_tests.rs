use super::*;

fn page(bytes: &[u8]) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    doc
}

#[test]
fn textedit_reflections_cancel_before_hitboxes_and_preserve_other_operators() {
    let absolute = b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";
    // Exercise either axis separately and both together. Inner page translation
    // and text leading must retain their signs under the reflected outer CTM.
    for (outer, inner, matrix, leading) in [
        (
            ".25 0 0 -.25 0 240",
            "4 0 0 4 40 40",
            "1 0 0 -1 30 50",
            "40",
        ),
        (
            "-.25 0 0 .25 300 0",
            "4 0 0 4 40 40",
            "-1 0 0 1 250 170",
            "40",
        ),
        (
            "-.25 0 0 -.25 300 240",
            "4 0 0 4 40 40",
            "-1 0 0 -1 250 50",
            "40",
        ),
    ] {
        let bytes = format!("q {outer} cm 0 0 1200 960 re W* n q {inner} cm BT /F1 12 Tf {leading} TL {matrix} Tm (FIRST) Tj T* (SECOND) Tj ET Q Q");
        for rotation in [0, 90, 180, 270] {
            let mut doc = page(bytes.as_bytes());
            let mut baseline = page(absolute);
            for source in [&mut doc, &mut baseline] {
                let id = crate::pagetree::ordered_pages(source)[0];
                let dict = source.get_dictionary_mut(id).unwrap();
                dict.set("Rotate", rotation);
                dict.set(
                    "CropBox",
                    vec![10.into(), 20.into(), 290.into(), 220.into()],
                );
            }
            let before = inspect(&doc, 0).unwrap();
            let expected = scan(&baseline, 0).unwrap();
            for (actual, expected) in before.runs.runs.iter().zip(&expected.runs) {
                assert_eq!(
                    actual.matrix, expected.matrix,
                    "{outer}, rotation {rotation}"
                );
                assert_eq!(actual.display_rect, expected.display_rect);
                assert_eq!(actual.advance, expected.advance);
            }
            assert_eq!(before.runs.runs.len(), 2);
            let original_objects = doc.objects.clone();
            let change = Change {
                layout: None,
                page: 0,
                revision: before.runs.revision,
                operator: before.runs.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            write(&mut doc, std::slice::from_ref(&change)).unwrap();
            let after = inspect(&doc, 0).unwrap();
            assert_eq!(after.runs.runs[0].text, "IN");
            assert_eq!(after.runs.runs[1], before.runs.runs[1]);
            assert_eq!(
                after.content.operations.len(),
                before.content.operations.len()
            );
            for (index, (old, new)) in before
                .content
                .operations
                .iter()
                .zip(&after.content.operations)
                .enumerate()
            {
                if index != change.operator as usize {
                    assert_eq!(old.operator, new.operator);
                    assert_eq!(old.operands, new.operands);
                }
            }
            for (id, object) in original_objects {
                if id != before.id {
                    assert_eq!(doc.objects[&id], object);
                }
            }
        }
    }
}

#[test]
fn textedit_reflections_restore_page_transform_and_keep_fixed_clip_intersections() {
    let doc = page(b"q 1 0 0 -1 0 240 cm 40 48 80 15 re W n q 1 0 0 -1 0 240 cm BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
    let runs = scan(&doc, 0).unwrap().runs;
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
    assert_eq!(runs[1].matrix, runs[0].matrix);
    assert_eq!(runs[2].matrix, [1., 0., 0., 1., 40., 140.]);
    // A normalized reflected clip still excludes each partly hidden side.
    for rect in [
        "41 40 200 100",
        "0 40 300 19",
        "0 40 50 100",
        "0 52 300 100",
    ] {
        let bytes = format!("0 0 300 240 re W n 1 0 0 -1 0 240 cm {rect} re W* n BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET");
        let runs = super::tests::clipped_roundtrip(&page(bytes.as_bytes()));
        assert_ne!(runs.runs[0].display_rect, [40., 48., 76., 63.]);
    }
    let doc = page(b"0 0 10 10 re W n 1 0 0 -1 0 240 cm 20 210 10 10 re W n");
    assert!(scan(&doc, 0)
        .unwrap_err()
        .contains("empty text clipping intersection"));
}

#[test]
fn textedit_reflections_refuse_mirrored_collapsed_skewed_and_unbounded_text_atomically() {
    for body in [
        "1 0 0 -1 0 240 cm BT /F1 12 Tf 40 60 Td (FIRST) Tj ET",
        "-1 0 0 1 300 0 cm BT /F1 12 Tf 260 180 Td (FIRST) Tj ET",
        "1 0 0 -1 0 240 cm BT /F1 12 Tf 1 0.1 0 -1 40 60 Tm (FIRST) Tj ET",
        "1 0 0 -1 0 240 cm BT /F1 12 Tf 1 0 0.1 -1 40 60 Tm (FIRST) Tj ET",
        "1 0.1 0 -1 0 240 cm BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET",
        "1 0 0.1 -1 0 240 cm BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET",
        "0 0 0 -1 0 240 cm BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET",
        "1 0 0 0 0 240 cm BT /F1 12 Tf 1 0 0 -1 40 60 Tm (FIRST) Tj ET",
        "1 0 0 -1 0 240 cm BT /F1 12 Tf 0 0 0 -1 40 60 Tm (FIRST) Tj ET",
        "1 0 0 -1 0 240 cm BT /F1 12 Tf 1 0 0 0 40 60 Tm (FIRST) Tj ET",
        "-1000 0 0 1 0 0 cm BT /F1 12 Tf -1001 0 0 1 0 0 Tm (FIRST) Tj ET",
        "1 0 0 -1000 0 0 cm BT /F1 12 Tf 1 0 0 -1001 0 0 Tm (FIRST) Tj ET",
        "1 0 0 -1 0 -1000000 cm 1 0 0 1 0 1 cm BT /F1 12 Tf 1 0 0 -1 0 0 Tm (FIRST) Tj ET",
        "1 0 0 -1000 0 0 cm 0 0 300 2000 re W n",
    ] {
        let mut doc = page(body.as_bytes());
        let before = doc.objects.clone();
        assert!(scan(&doc, 0).is_err(), "accepted {body}");
        assert!(write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: Vec::new(),
                operator: 0,
                original: "FIRST".into(),
                replacement: "IN".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, before);
    }
    for axis in [0, 3] {
        let mut outer = [1., 0., 0., 1., 0., 0.];
        let mut inner = outer;
        outer[axis] = -1e-300;
        inner[axis] = -1e-300;
        assert!(
            compose_orthogonal(outer, inner).is_err(),
            "underflow axis {axis}"
        );
        inner[axis] = f64::INFINITY;
        assert!(compose_orthogonal(outer, inner).is_err());
    }
}
