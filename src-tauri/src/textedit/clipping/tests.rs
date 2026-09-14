use crate::textedit::{self, Change};
use lopdf::{content::Content, Dictionary, Document, Stream};

fn page(bytes: &[u8]) -> Document {
    let (mut doc, _, _, _) = textedit::fonts::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", content);
    doc
}

#[test]
fn textedit_rectangular_clips_preserve_operators_and_following_text() {
    for rule in ["W", "W*"] {
        let mut doc = page(format!("0.1 w q 0 0 300 240 re {rule} n q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET Q").as_bytes());
        let before = textedit::scan(&doc, 0).unwrap();
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let ops = Content::decode_strict(&doc.get_page_content(id))
            .unwrap()
            .operations;
        let change = Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        textedit::write(&mut doc, &[change]).unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        let saved = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        assert_eq!(saved.operations.len(), ops.len());
        for (index, (a, b)) in saved.operations.iter().zip(ops).enumerate() {
            assert_eq!(a.operator, b.operator);
            if index != before.runs[0].operator as usize {
                assert_eq!(a.operands, b.operands);
            }
        }
    }
}

#[test]
fn textedit_clip_intersections_contain_every_side_of_text() {
    // FIRST occupies x=40..76, y=180..188.4 in the validated glyph envelope.
    for rect in [
        "41 100 200 100",
        "0 181 300 50",
        "0 100 50 100",
        "0 100 300 88",
    ] {
        let doc = page(format!("{rect} re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert!(
            textedit::scan(&doc, 0)
                .unwrap_err()
                .contains("partly clipped"),
            "accepted {rect}"
        );
    }
    // W/W* intersect; a later larger rectangle cannot reopen a smaller clip.
    let doc = page(b"0 0 50 240 re W n 0 0 300 240 re W* n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("partly clipped"));
    let doc = page(b"0 0 10 10 re W n 20 20 10 10 re W n");
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("empty text clipping intersection"));
}

#[test]
fn textedit_clip_transform_is_fixed_at_creation_and_restored_by_q() {
    // The clip is made under a translated/scaled CTM; later text has a new CTM.
    let doc = page(
        b"2 0 0 3 30 50 cm 0 0 100 60 re W n 1 0 0 1 10 10 cm BT /F1 12 Tf 10 30 Td (FIRST) Tj ET",
    );
    assert_eq!(
        textedit::scan(&doc, 0).unwrap().runs[0].matrix,
        [2., 0., 0., 3., 70., 170.]
    );
    // Tighten inside q; Q restores the outer clip before drawing elsewhere.
    let doc = page(b"0 0 300 240 re W n q 40 177 80 15 re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    // Q must not discard an outer clip either.
    let doc =
        page(b"40 177 80 15 re W n q 40 177 80 15 re W n Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("partly clipped"));
    // Translating text does not move an already established clip with it.
    let doc = page(b"0 0 100 240 re W n 1 0 0 1 150 0 cm BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("partly clipped"));
}

#[test]
fn textedit_clips_refuse_partial_compound_painted_and_unbounded_paths() {
    for prefix in [
        "0 0 300 240 re",
        "0 0 300 240 re W",
        "W n",
        "0 0 300 240 re W S",
        "0 0 300 240 re W f",
        "0 0 300 240 re 1 W n",
        "0 0 300 240 re W 1 n",
        "0 0 300 240 re 0 0 100 100 re W n",
        "0 0 300 240 re W q n Q",
        "0 0 m 300 240 l W n",
        "0 0 -300 240 re W n",
        "0 0 300 0 re W n",
        "0 0 1000001 240 re W n",
        "1000000 0 1 240 re W n",
        "1000 0 0 1000 0 0 cm 0 0 2000 240 re W n",
        "-1 w",
        "1000001 w",
        "/bad w",
        "1 2 w",
    ] {
        let doc = page(format!("{prefix} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {prefix}");
    }
    assert!(textedit::scan(
        &page(b"BT 0 0 300 240 re W n /F1 12 Tf 40 180 Td (FIRST) Tj ET"),
        0
    )
    .is_err());
    // A line-width setter does not enable arbitrary paths or stroke text.
    for body in [
        "0 w BT /F1 12 Tf 1 Tr 40 180 Td (FIRST) Tj ET",
        "1 w 0 0 m 100 100 l S",
    ] {
        assert!(textedit::scan(&page(body.as_bytes()), 0).is_err());
    }
}

#[test]
fn textedit_clips_require_proven_outline_bounds_not_only_advances() {
    let mut doc = textedit::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(
        Dictionary::new(),
        b"0 0 300 240 re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    assert!(textedit::scan(&doc, 0)
        .unwrap_err()
        .contains("validated embedded glyph outlines"));
}

#[test]
fn textedit_clips_compare_original_page_coordinates_before_crop_and_rotation() {
    for rotation in [0, 90, 180, 270] {
        let mut clipped = page(b"40 177 80 15 re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        let mut plain = page(b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        for doc in [&mut clipped, &mut plain] {
            let id = crate::pagetree::ordered_pages(doc)[0];
            let page = doc.get_dictionary_mut(id).unwrap();
            page.set(
                "CropBox",
                vec![20.into(), 30.into(), 300.into(), 240.into()],
            );
            page.set("Rotate", rotation);
        }
        let clipped = textedit::scan(&clipped, 0).unwrap().runs.remove(0);
        let plain = textedit::scan(&plain, 0).unwrap().runs.remove(0);
        assert_eq!(clipped.matrix, plain.matrix);
        assert_eq!(clipped.display_rect, plain.display_rect);
        assert_eq!(clipped.advance, plain.advance);
    }
}

#[test]
fn textedit_painted_rectangles_preserve_paint_state_and_surrounding_operators() {
    let plain = page(b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
    let geometry = textedit::scan(&plain, 0).unwrap();
    for paint in ["S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n"] {
        // This decoration is far from the text: treating it as a clip must fail
        // the positive control. q/Q restores its colour and transform.
        let body = format!("n q 0.3 0.6 0.9 rg 0.1 0.2 0.3 RG 2 w -2 0 0 2 100 0 cm 0 0 20 20 re {paint} Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET 0.1 g 20 20 10 10 re {paint} BT /F1 12 Tf 40 140 Td (SECOND) Tj ET n");
        let mut doc = page(body.as_bytes());
        let before = textedit::scan(&doc, 0).unwrap();
        for (actual, expected) in before.runs.iter().zip(&geometry.runs) {
            assert_eq!(actual.display_rect, expected.display_rect);
            assert_eq!(actual.matrix, expected.matrix);
        }
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let objects = doc.objects.clone();
        let operations = Content::decode_strict(&doc.get_page_content(id))
            .unwrap()
            .operations;
        textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        let saved = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        assert_eq!(saved.operations.len(), operations.len());
        for (index, (old, new)) in operations.iter().zip(saved.operations).enumerate() {
            assert_eq!(old.operator, new.operator);
            if index != before.runs[0].operator as usize {
                assert_eq!(old.operands, new.operands);
            }
        }
        for (object_id, object) in objects {
            if object_id != id {
                assert_eq!(doc.objects[&object_id], object);
            }
        }
    }
}

#[test]
fn textedit_painted_rectangles_do_not_replace_or_discard_clipping() {
    for paint in ["S", "f", "B*", "n"] {
        for (clip, accepted) in [("0 0 300 240", true), ("41 0 259 240", false)] {
            for (open, before_text, after_text) in [("", "", ""), ("q", "Q", ""), ("q", "", "Q")] {
                let body = format!(
                    "{clip} re W n {open} 0 0 300 240 re {paint} {before_text} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {after_text}"
                );
                let doc = page(body.as_bytes());
                let result = textedit::scan(&doc, 0);
                assert_eq!(result.is_ok(), accepted, "{body}: {result:?}");
                if !accepted {
                    assert!(result.unwrap_err().contains("partly clipped"));
                }
            }
        }
        // Clipping combined with painting still needs its own implementation.
        let doc = page(
            format!("0 0 300 240 re W {paint} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes(),
        );
        assert_eq!(textedit::scan(&doc, 0).is_ok(), paint == "n");
    }
}

#[test]
fn textedit_painted_rectangles_refuse_invalid_geometry_and_incomplete_paths_atomically() {
    for path in [
        "0 0 20 re f",
        "0 0 20 20 20 re f",
        "0 0 /bad 20 re f",
        "0 0 -20 20 re f",
        "0 0 20 0 re f",
        "0 0 1000001 20 re f",
        "1000000 0 20 20 re f",
        "2 0 0 1 0 0 cm 0 0 600000 20 re f",
        "0 0 20 20 re 1 f",
        "0 0 20 20 re W f",
        "0 0 20 20 re W S",
        "0 0 20 20 re 0 0 10 10 re f",
        "0 0 20 20 re q f Q",
        "0 0 20 20 re h f",
        "0 0 20 20 re 10 10 l f",
        "0 0 20 20 re",
        "f",
        "S",
        "1 n",
        "BT 0 0 20 20 re f ET",
        "BT n ET",
    ] {
        let bytes = format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {path}");
        let mut doc = page(bytes.as_bytes());
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {path}");
        assert!(textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: vec![],
                operator: 3,
                original: "FIRST".into(),
                replacement: "IN".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
}
