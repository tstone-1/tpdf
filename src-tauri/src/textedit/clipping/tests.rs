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
    // FIRST occupies x=40..76, y=177..192 in the conservative hit envelope.
    for rect in [
        "41 100 200 100",
        "0 178 300 50",
        "0 100 50 100",
        "0 100 300 91",
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
        "0 0 300 240 re n",
        "W n",
        "n",
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
    // A line-width setter does not enable any stroking or stroke text mode.
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
