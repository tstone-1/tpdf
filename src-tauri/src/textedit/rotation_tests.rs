use super::*;

fn page(body: &str) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    doc
}

#[test]
fn textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn() {
    // Independent explicit positions and extrema for a nonsquare envelope.
    for (axes, origin, expected, envelope) in [
        (
            "2 0 0 3",
            [80., 100.],
            [100., 160.],
            [100., 151., 172., 196.],
        ),
        (
            "0 2 -3 0",
            [160., 80.],
            [100., 100.],
            [64., 100., 109., 172.],
        ),
        (
            "-2 0 0 -3",
            [160., 180.],
            [140., 120.],
            [68., 84., 140., 129.],
        ),
        (
            "0 -2 3 0",
            [80., 180.],
            [140., 160.],
            [131., 88., 176., 160.],
        ),
    ] {
        let body = format!("BT /F1 12 Tf 10 TL {axes} {} {} Tm 10 20 Td (FIRST) Tj T* (SECOND) Tj 3 -5 Td (THIRD) Tj ET", origin[0], origin[1]);
        let doc = page(&body);
        let runs = scan(&doc, 0).unwrap().runs;
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].matrix[4..], expected);
        // The original geometric fixture has 600-unit advances, FIRST = 36pt.
        assert!((runs[0].advance - 36.).abs() < 1e-6);
        let expected_display = [
            envelope[0] as f32,
            (240. - envelope[3]) as f32,
            envelope[2] as f32,
            (240. - envelope[1]) as f32,
        ];
        assert_eq!(runs[0].display_rect, expected_display);
        let a = runs[0].matrix[0];
        let b = runs[0].matrix[1];
        let c = runs[0].matrix[2];
        let d = runs[0].matrix[3];
        assert_eq!(
            runs[1].matrix[4..],
            [expected[0] - 10. * c, expected[1] - 10. * d]
        );
        assert_eq!(
            runs[2].matrix[4..],
            [
                expected[0] - 15. * c + 3. * a,
                expected[1] - 15. * d + 3. * b
            ]
        );
        for turns in [0, 90, 180, 270] {
            let mut cropped = doc.clone();
            let id = crate::pagetree::ordered_pages(&cropped)[0];
            let dict = cropped.get_dictionary_mut(id).unwrap();
            dict.set(
                "CropBox",
                vec![10.into(), 20.into(), 290.into(), 220.into()],
            );
            dict.set("Rotate", turns);
            let run = scan(&cropped, 0).unwrap().runs.remove(0);
            let [l, bot, r, top] = envelope;
            let expected = match turns {
                0 => [l - 10., 220. - top, r - 10., 220. - bot],
                90 => [bot - 20., l - 10., top - 20., r - 10.],
                180 => [290. - r, bot - 20., 290. - l, top - 20.],
                _ => [220. - top, 290. - r, 220. - bot, 290. - l],
            }
            .map(|v| v as f32);
            assert_eq!(run.display_rect, expected, "{axes} Rotate {turns}");
        }
    }
}

#[test]
fn textedit_rotation_composes_page_scales_and_preserves_replacement_placement() {
    let mut doc = page("q 2 0 0 3 10 20 cm BT /F1 12 Tf 0 2 -3 0 60 40 Tm [(FI) 0 (RST)] TJ 0 -20 Td (SECOND) Tj ET Q");
    let before = inspect(&doc, 0).unwrap();
    let run = &before.runs.runs[0];
    assert_eq!(run.matrix, [0., 6., -6., 0., 130., 140.]);
    let change = Change {
        page: 0,
        revision: before.runs.revision.clone(),
        operator: run.operator,
        original: run.text.clone(),
        replacement: "IN".into(),
    };
    write(&mut doc, std::slice::from_ref(&change)).unwrap();
    let after = inspect(&doc, 0).unwrap();
    assert_eq!(after.runs.runs[0].text, "IN");
    assert_eq!(after.runs.runs[0].matrix, run.matrix);
    assert_eq!(after.runs.runs[1], before.runs.runs[1]);
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
    let snapshot = doc.objects.clone();
    let change = Change {
        replacement: "FIRST FIRST FIRST".into(),
        ..tests::change(&doc)
    };
    assert!(write(&mut doc, &[change])
        .unwrap_err()
        .contains("exceed the original"));
    assert_eq!(doc.objects, snapshot);
}

#[test]
fn textedit_rotation_clips_use_transformed_glyph_envelopes_on_every_side() {
    for axes in ["0 1 -1 0", "0 -1 1 0", "-1 0 0 -1"] {
        let body = format!("BT /F1 12 Tf {axes} 140 120 Tm (FIRST) Tj ET");
        let doc = page(&body);
        let metrics = font(
            &doc,
            resources(&doc, crate::pagetree::ordered_pages(&doc)[0]).unwrap(),
            b"F1",
        )
        .unwrap();
        let [bottom, top] = metrics.vertical_bounds.unwrap();
        let m = scan(&doc, 0).unwrap().runs[0].matrix;
        // Explicit formula for the three rotations, using actual fixture ink.
        let lo = bottom * 12. / 1000.;
        let hi = top * 12. / 1000.;
        let bounds = if m[1] > 0. {
            [140. - hi, 120., 140. - lo, 156.]
        } else if m[1] < 0. {
            [140. + lo, 84., 140. + hi, 120.]
        } else {
            [104., 120. - hi, 140., 120. - lo]
        };
        let clipped = |r: [f64; 4]| {
            page(&format!(
                "{} {} {} {} re W n {body}",
                r[0],
                r[1],
                r[2] - r[0],
                r[3] - r[1]
            ))
        };
        let roomy = [
            bounds[0] - 1.,
            bounds[1] - 1.,
            bounds[2] + 1.,
            bounds[3] + 1.,
        ];
        assert!(scan(&clipped(roomy), 0).is_ok());
        for side in 0..4 {
            let mut cut = roomy;
            cut[side] = bounds[side] + if side < 2 { 0.5 } else { -0.5 };
            assert!(
                scan(&clipped(cut), 0)
                    .unwrap_err()
                    .contains("partly clipped"),
                "{axes}, side {side}"
            );
        }
    }
}

#[test]
fn textedit_rotation_refuses_skew_mirrors_collapse_and_accumulated_overflow() {
    for axes in [
        "0 0 0 0",
        "0 1 0 0",
        "0 0 -1 0",
        "0 1 1 0",
        "0 -1 -1 0",
        "0.000001 1 -1 0",
        "0 1 -1 0.000001",
        "1 1 -1 1",
    ] {
        assert!(
            scan(
                &page(&format!("BT /F1 12 Tf {axes} 100 100 Tm (FIRST) Tj ET")),
                0
            )
            .is_err(),
            "{axes}"
        );
    }
    for body in [
        "BT /F1 12 Tf 0 1000 -1000 0 0 0 Tm 2000 0 Td (FIRST) Tj ET",
        "BT /F1 12 Tf 0 1000 -1000 0 0 0 Tm 0 2000 Td (FIRST) Tj ET",
        "2000 0 0 2000 0 0 cm BT /F1 12 Tf 0 1000 -1000 0 0 0 Tm (FIRST) Tj ET",
    ] {
        assert!(scan(&page(body), 0).is_err(), "{body}");
    }
}
