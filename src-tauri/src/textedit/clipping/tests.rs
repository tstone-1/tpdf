use crate::textedit::{self, Change};
use lopdf::{content::Content, Dictionary, Document, Stream};

#[test]
fn compound_clip_holes_winding_and_saved_state() {
    let outer = "0 0 m 300 0 l 300 240 l 0 240 l 0 0 l h";
    let hole = "50 170 m 60 170 l 60 200 l 50 200 l h";
    let reversed = "50 170 m 50 200 l 60 200 l 60 170 l h";
    for (inner, rule, accepted) in [
        (hole, "W*", false),
        (hole, "W", true),
        (reversed, "W", false),
    ] {
        let doc = page(
            format!("{outer} {inner} {rule} n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes(),
        );
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted);
    }
    let mut doc =
        page(format!("q {outer} {hole} W* n Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
    let source = textedit::scan(&doc, 0).unwrap();
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: source.revision,
            operator: source.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    let id = crate::pagetree::ordered_pages(&doc)[0];
    assert!(doc
        .get_page_content(id)
        .starts_with(format!("q {outer} {hole} W* n Q").as_bytes()));
}

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
            layout: None,
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
        let runs = textedit::tests::clipped_roundtrip(&doc);
        assert_ne!(runs.runs[0].display_rect, [40., 48., 76., 63.]);
    }
    // W/W* intersect; a later larger rectangle cannot reopen a smaller clip.
    let doc = page(b"0 0 50 240 re W n 0 0 300 240 re W* n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    assert_eq!(
        textedit::tests::clipped_roundtrip(&doc).runs[0].display_rect[2],
        50.
    );
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
    let hidden = textedit::tests::clipped_roundtrip(&doc).runs[0].display_rect;
    assert_eq!(hidden[1], hidden[3]);
    // Translating text does not move an already established clip with it.
    let doc = page(b"0 0 100 240 re W n 1 0 0 1 150 0 cm BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let hidden = textedit::tests::clipped_roundtrip(&doc).runs[0].display_rect;
    assert_eq!(hidden[0], hidden[2]);
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
        "0 w BT /F1 12 Tf 5 Tr 40 180 Td (FIRST) Tj ET",
        "1 w 0 0 m 100 100 l W S",
    ] {
        assert!(textedit::scan(&page(body.as_bytes()), 0).is_err());
    }
}

// FIRST's ink starts at x=40. A compound clip from x=39 holds the fill but not
// a 2-point stroke (4 w, Tr 2), and the stroke is what it would cut.
#[test]
fn textedit_compound_clips_contain_the_stroke_around_text() {
    let clip = "39 100 m 300 100 l 300 230 l 39 230 l h W n";
    for (mode, accepted) in [("0 Tr", true), ("4 w 2 Tr", false), ("4 w 3 Tr", true)] {
        let doc = page(format!("{clip} {mode} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{mode}");
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
        let body = format!("n q 0.3 0.6 0.9 rg 0.1 0.2 0.3 RG 2 w -2 0 0 2 100 0 cm 0 0 -20 20 re 10 10 20 -20 re -10 -10 -5 -5 re {paint} Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET 0.1 g 20 20 10 10 re {paint} BT /F1 12 Tf 40 140 Td (SECOND) Tj ET n");
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
                layout: None,
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
                assert!(result.is_ok(), "{body}: {result:?}");
                let runs = textedit::tests::clipped_roundtrip(&doc);
                assert_eq!(
                    runs.runs[0].display_rect[0],
                    if accepted { 40. } else { 41. }
                );
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
        "0 0 20 0 re f",
        "0 0 1000001 20 re f",
        "1000000 0 20 20 re f",
        "2 0 0 1 0 0 cm 0 0 600000 20 re f",
        "0 0 20 20 re 1 f",
        "0 0 20 20 re W f",
        "0 0 20 20 re W S",
        "0 0 20 20 re 0 0 10 0 re f",
        "0 0 20 20 re 1000000 0 10 10 re f",
        "0 0 20 20 re 0 /bad 10 10 re f",
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
                layout: None,
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

#[test]
fn textedit_stroked_lines_preserve_graphics_and_following_text() {
    for ending in ["S", "s", "h S", "h s", "n", "h n"] {
        let body = format!("q .1 .2 .3 RG 2 w -2 0 0 2 100 0 cm 0 0 m 20 20 l 40 0 l {ending} Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET 0 0 m 20 20 l {ending} BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
        let mut doc = page(body.as_bytes());
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        assert_eq!(before.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let objects = doc.objects.clone();
        let id = crate::pagetree::ordered_pages(&doc)[0];
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
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
        let original = Content::decode_strict(body.as_bytes()).unwrap();
        let saved = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        assert_eq!(original.operations.len(), saved.operations.len());
        for (index, (a, b)) in original.operations.iter().zip(saved.operations).enumerate() {
            assert_eq!(a.operator, b.operator);
            if index != before.runs[0].operator as usize {
                assert_eq!(a.operands, b.operands);
            }
        }
        for (old_id, object) in objects {
            if old_id != id {
                assert_eq!(doc.objects[&old_id], object);
            }
        }
    }
}

#[test]
fn textedit_stroked_lines_cannot_replace_or_discard_a_clip() {
    for ending in ["S", "s", "h S", "n"] {
        for (clip, accepted) in [("0 0 300 240", true), ("41 0 259 240", false)] {
            for (open, before, after) in [("", "", ""), ("q", "", "Q"), ("q", "Q", "")] {
                let body=format!("{clip} re W n {open} 0 0 m 20 20 l {ending} {before} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {after}");
                let runs = textedit::tests::clipped_roundtrip(&page(body.as_bytes()));
                assert_eq!(
                    runs.runs[0].display_rect[0],
                    if accepted { 40. } else { 41. }
                );
            }
        }
    }
}

#[test]
fn textedit_stroked_lines_refuse_invalid_and_incomplete_paths_atomically() {
    for path in [
        "0 m 20 20 l S",
        "0 0 0 m 20 20 l S",
        "0 /bad m 20 20 l S",
        "0 0 m 20 l S",
        "0 0 m /bad 20 l S",
        "0 0 m 20 20 20 l S",
        "1000001 0 m 20 20 l S",
        "0 0 m 1000001 20 l S",
        "2 0 0 2 0 0 cm 600000 0 m 20 20 l S",
        "2 0 0 2 0 0 cm 0 0 m 20 600000 l S",
        "0 0 m S",
        "0 0 m n",
        "0 0 m 20 20 l",
        "0 0 m 20 20 l h",
        "0 0 m 20 20 l 1 h S",
        "0 0 m 20 20 l h 1 S",
        "0 0 m 20 20 l 1 n",
        "0 0 m 20 20 l 1 s",
        "0 0 m 20 20 l h 40 0 l S",
        "0 0 m 20 20 l W n",
        "0 0 m 20 20 l W S",
        "0 0 m 20 20 l q S Q",
        "0 0 m 20 20 l 2 w S",
        "0 0 m 20 20 l 0 0 10 10 re S",
        "BT 0 0 m 20 20 l S ET",
        "20 20 l S",
        "h S",
    ] {
        let body = format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {path}");
        let mut doc = page(body.as_bytes());
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {path}");
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
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

#[test]
fn textedit_curves_preserve_complete_subpaths_and_following_text() {
    for end in ["S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n"] {
        for close in ["", "h"] {
            let path = format!("0 0 m 10 30 30 30 40 0 c 50 -10 60 0 v 70 10 80 0 y 90 0 l {close} 100 0 m 110 0 l {end}");
            let body = format!("0 0 300 240 re W n q -1 0 0 2 200 40 cm {path} Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {path} BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
            let mut doc = page(body.as_bytes());
            let before = textedit::scan(&doc, 0).unwrap();
            assert_eq!(before.runs.len(), 2);
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let objects = doc.objects.clone();
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
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
            let old = Content::decode_strict(body.as_bytes()).unwrap();
            let new = Content::decode_strict(&doc.get_page_content(id)).unwrap();
            assert_eq!(old.operations.len(), new.operations.len());
            for (index, (a, b)) in old.operations.iter().zip(new.operations).enumerate() {
                assert_eq!(a.operator, b.operator);
                if index != before.runs[0].operator as usize {
                    assert_eq!(a.operands, b.operands);
                }
            }
            for (key, object) in objects {
                if key != id {
                    assert_eq!(doc.objects[&key], object);
                }
            }
        }
    }
}

#[test]
fn textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically() {
    let mut paths = vec![
        // A path has to draw a segment; movetos alone draw nothing.
        "0 0 m 20 20 m f".to_string(),
        // A moveto starts a new subpath; closing it before a segment is refused.
        "0 0 m 10 10 l 20 20 m h S".to_string(),
        "0 0 m f".to_string(),
        "0 0 m 1 2 3 4 5 6 c h h f".to_string(),
        "0 0 m 1 2 3 4 5 6 c W n".to_string(),
        "0 0 m 1 2 3 4 5 6 c W* f".to_string(),
        "0 0 m 1 2 3 4 5 6 c q f Q".to_string(),
        "0 0 m 1 2 3 4 5 6 c BT /F1 12 Tf ET f".to_string(),
        "0 0 m 1 2 3 4 5 6 c 1 f".to_string(),
        "0 0 m 1 2 3 4 5 6 c".to_string(),
    ];
    for (op, count) in [("c", 6), ("v", 4), ("y", 4)] {
        for size in [count - 1, count + 1] {
            paths.push(format!("0 0 m {} {op} f", vec!["1"; size].join(" ")));
        }
        for index in 0..count {
            for invalid in ["/bad", "1000001", "-1000001"] {
                let mut points = vec!["1"; count];
                points[index] = invalid;
                paths.push(format!("0 0 m {} {op} f", points.join(" ")));
            }
            let mut points = vec!["1"; count];
            points[index] = "500001";
            paths.push(format!("2 0 0 2 0 0 cm 0 0 m {} {op} f", points.join(" ")));
        }
    }
    for path in paths {
        let mut doc = page(format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {path}").as_bytes());
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {path}");
        assert!(
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: vec![],
                    operator: 3,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                }]
            )
            .is_err(),
            "wrote {path}"
        );
        assert_eq!(doc.objects, objects);
    }
}

#[test]
fn textedit_curves_preserve_clip_and_validate_transformed_boundary() {
    let curve = "0 0 m 1 2 3 4 5 6 c 7 8 9 10 v 11 12 13 14 y f";
    for (rect, accepted) in [("0 0 300 240", true), ("41 0 259 240", false)] {
        let body = format!("{rect} re W n q {curve} Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        let runs = textedit::tests::clipped_roundtrip(&page(body.as_bytes()));
        assert_eq!(
            runs.runs[0].display_rect[0],
            if accepted { 40. } else { 41. }
        );
    }
    for (op, count) in [("c", 6), ("v", 4), ("y", 4)] {
        for index in 0..count {
            for value in ["500000", "-500000"] {
                let mut points = vec!["1"; count];
                points[index] = value;
                let body = format!(
                    "q 2 0 0 2 0 0 cm 0 0 m {} {op} f Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
                    points.join(" ")
                );
                assert!(textedit::scan(&page(body.as_bytes()), 0).is_ok(), "{body}");
            }
        }
    }
}

#[test]
fn textedit_reversed_clips_preserve_geometry_and_authored_bytes() {
    let text = "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";
    for rule in ["W", "W*"] {
        for transform in ["", "-1 0 0 -1 300 240 cm"] {
            let control =
                page(format!("{transform} q 30 120 200 80 re {rule} n {text} Q").as_bytes());
            let expected = textedit::scan(&control, 0).unwrap();
            for rect in ["230 120 -200 80", "30 200 200 -80", "230 200 -200 -80"] {
                let prefix = format!("{transform} q {rect} re {rule} n ");
                let mut doc = page(format!("{prefix}{text} Q").as_bytes());
                let before = textedit::scan(&doc, 0).unwrap();
                assert_eq!(before.runs, expected.runs);
                let id = crate::pagetree::ordered_pages(&doc)[0];
                let objects = doc.objects.clone();
                textedit::write(
                    &mut doc,
                    &[Change {
                        layout: None,
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
                assert!(doc.get_page_content(id).starts_with(prefix.as_bytes()));
                for (object_id, object) in objects {
                    if object_id != id {
                        assert_eq!(doc.objects[&object_id], object);
                    }
                }
            }
        }
    }
}

#[test]
fn textedit_reversed_clips_still_reject_partial_empty_and_unbounded_regions() {
    for rule in ["W", "W*"] {
        for rect in [
            "241 200 -200 -100",
            "300 231 -300 -50",
            "50 200 -50 -100",
            "300 188 -300 -88",
        ] {
            let body = format!("{rect} re {rule} n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
            let runs = textedit::tests::clipped_roundtrip(&page(body.as_bytes()));
            assert_ne!(runs.runs[0].display_rect, [40., 48., 76., 63.]);
        }
        for rect in [
            "0 0 0 -240",
            "0 0 -300 0",
            "-1000000 0 -1 240",
            "0 -1000000 300 -1",
            "2 0 0 2 0 0 cm 0 0 -500001 240",
        ] {
            let body = format!("{rect} re {rule} n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
            assert!(textedit::scan(&page(body.as_bytes()), 0).is_err(), "{body}");
        }
        let body = format!("50 240 -50 -240 re {rule} n 300 240 -300 -240 re {rule} n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        assert_eq!(
            textedit::tests::clipped_roundtrip(&page(body.as_bytes())).runs[0].display_rect[2],
            50.
        );
        let body = format!("10 10 -10 -10 re {rule} n 30 30 -10 -10 re {rule} n");
        assert!(textedit::scan(&page(body.as_bytes()), 0)
            .unwrap_err()
            .contains("empty text clipping intersection"));
    }
}

#[test]
fn textedit_reversed_clip_bounds_normalize_every_corner_after_transform() {
    for (ctm, expected) in [
        ([1., 0., 0., 1., 0., 0.], [30., 120., 230., 200.]),
        ([-1., 0., 0., 1., 300., 0.], [70., 120., 270., 200.]),
        ([1., 0., 0., -1., 0., 240.], [30., 40., 230., 120.]),
        ([-2., 0., 0., -2., 600., 480.], [140., 80., 540., 240.]),
    ] {
        for rect in [
            "30 120 200 80",
            "230 120 -200 80",
            "30 200 200 -80",
            "230 200 -200 -80",
        ] {
            for rule in ["W", "W*"] {
                let ops = Content::decode_strict(format!("{rect} re {rule} n").as_bytes()).unwrap();
                assert_eq!(super::apply(None, &ops.operations, ctm).unwrap(), expected);
            }
        }
    }
}

// TikZ opens paths with two movetos and closes them with `h m`. A moveto that
// starts no segment draws nothing, so these paint exactly what their segments
// do; the edit beside them keeps every path operator.
#[test]
fn textedit_paths_accept_movetos_that_start_nothing() {
    for path in [
        "0 0 m 0 0 m 10 10 l f",
        "0 0 m 10 10 l 20 20 m f",
        "0 0 m 10 0 l 10 10 l h 0 0 m S",
        "5 5 m 5 5 m 1 2 3 4 5 6 c 7 8 9 10 11 12 c h 5 5 m f",
    ] {
        let mut doc = page(format!("{path} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        let scan = textedit::scan(&doc, 0).unwrap();
        let before = lopdf::content::Content::decode_strict(
            &doc.get_page_content(crate::pagetree::ordered_pages(&doc)[0]),
        )
        .unwrap();
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: scan.revision,
                operator: scan.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = lopdf::content::Content::decode_strict(
            &doc.get_page_content(crate::pagetree::ordered_pages(&doc)[0]),
        )
        .unwrap();
        let count = path
            .split(' ')
            .filter(|t| t.chars().all(|c| c.is_ascii_alphabetic()))
            .count();
        for (a, b) in before.operations[..count]
            .iter()
            .zip(&after.operations[..count])
        {
            assert_eq!(
                (&a.operator, &a.operands),
                (&b.operator, &b.operands),
                "{path}"
            );
        }
    }
}

// TikZ rotates drawings with cm. A painted path or rectangle under any affine
// CTM is preserved and only range-checked; a clip under one stays refused,
// because the clip model is axis-aligned.
#[test]
fn textedit_rotated_paint_is_preserved_and_rotated_clips_are_refused() {
    for path in [
        "q 0 1 -1 0 200 0 cm 0 0 m 10 0 l 10 10 l h f Q",
        "q 0.7 0.7 -0.7 0.7 100 20 cm 0 0 10 10 re S Q",
        "q 1 0.2 0 1 0 0 cm 5 5 m 1 2 3 4 5 6 c S Q",
    ] {
        let doc = page(format!("{path} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert_eq!(
            textedit::scan(&doc, 0).unwrap().runs[0].text,
            "FIRST",
            "{path}"
        );
    }
    for (path, message) in [
        (
            "q 0 1 -1 0 200 0 cm 0 0 10 10 re W n Q",
            "non-diagonal clips are not editable yet",
        ),
        (
            "q 0 1 -1 0 200 0 cm 0 0 m 10 0 l 10 10 l h W n Q",
            "only complete bounded painted paths are editable",
        ),
        (
            "q 0 1000 -1000 0 0 0 cm 0 0 m 1001 0 l S Q",
            "path coordinates exceed their limit",
        ),
        (
            "q 0 1000 -1000 0 0 0 cm 0 0 1001 1 re f Q",
            "rectangle coordinates exceed their limit",
        ),
        (
            "q 0 1 -1 0 0 0 cm 0 0 0 5 re f Q",
            "empty rectangle is not editable",
        ),
    ] {
        let doc = page(format!("{path} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").as_bytes());
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), message, "{path}");
    }
}
