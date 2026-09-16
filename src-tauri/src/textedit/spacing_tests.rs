use super::*;

fn content(mut doc: Document, bytes: &[u8]) -> Document {
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    doc
}

fn embedded(bytes: &[u8]) -> Document {
    content(fonts::tests::fixture().0, bytes)
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn textedit_spacing_measures_character_steps_and_tj_fragments() {
    for spacing in [-2, 0, 2] {
        for (show, adjustment) in [("(AB) Tj", 0.), ("[(A) 100 (B)] TJ", 1.)] {
            let bytes = format!("q 2 0 0 3 0 0 cm {spacing} Tc BT /F1 10 Tf 20 60 Td {show} ET Q");
            let doc = embedded(bytes.as_bytes());
            let page = inspect(&doc, 0).unwrap();
            let run = &page.runs.runs[0];
            close(run.advance, 12. + 2. * f64::from(spacing) - adjustment);
            let right = 12. + f64::from(spacing) - adjustment;
            assert_eq!(page.horizontal_bounds[&run.operator], [0., right]);
            close(
                f64::from(run.display_rect[2] - run.display_rect[0]),
                2. * right,
            );
            assert_eq!(run.matrix, [2., 0., 0., 3., 40., 180.]);
        }
    }
}

#[test]
fn textedit_spacing_persists_across_blocks_and_restores_for_writing() {
    let mut doc = embedded(b"-2 Tc BT /F1 10 Tf 40 180 Td (BA) Tj ET q 2 Tc BT 40 140 Td (AB) Tj ET Q BT 40 100 Td (BA) Tj ET");
    let before = inspect(&doc, 0).unwrap();
    assert_eq!(
        before
            .runs
            .runs
            .iter()
            .map(|r| r.advance)
            .collect::<Vec<_>>(),
        [8., 16., 8.]
    );
    let other = scan(&doc, 1).unwrap();
    let resources = resources(&doc, before.id).unwrap().clone();
    let updates: Vec<_> = [0, 2]
        .map(|index| Change {
            layout: None,
            page: 0,
            revision: before.runs.revision.clone(),
            operator: before.runs.runs[index].operator,
            original: "BA".into(),
            replacement: "AB".into(),
        })
        .into();
    write(&mut doc, &updates).unwrap();
    let after = inspect(&doc, 0).unwrap();
    assert_eq!(after.runs.runs[1], before.runs.runs[1]);
    assert_eq!(after.runs.runs[0].text, "AB");
    assert_eq!(after.runs.runs[2].text, "AB");
    assert_eq!(after.runs.runs[2].advance, 8.);
    assert_eq!(scan(&doc, 1).unwrap().runs, other.runs);
    assert_eq!(*super::resources(&doc, before.id).unwrap(), resources);
    for (index, (old, new)) in before
        .content
        .operations
        .iter()
        .zip(after.content.operations)
        .enumerate()
    {
        if !updates.iter().any(|u| u.operator as usize == index) {
            assert_eq!(
                (old.operator.as_str(), &old.operands),
                (new.operator.as_str(), &new.operands)
            );
        }
    }
}

#[test]
fn textedit_spacing_replacement_advance_and_ink_are_both_checked_atomically() {
    // iii fits A without spacing, but with 2 Tc its three steps exceed A's one.
    let mut doc = content(tests::fixture(), b"BT /F1 10 Tf 2 Tc 40 180 Td (A) Tj ET");
    let before = doc.objects.clone();
    let update = Change {
        replacement: "iii".into(),
        ..tests::change(&doc)
    };
    assert!(write(&mut doc, &[update])
        .unwrap_err()
        .contains("original text advance"));
    assert_eq!(doc.objects, before);

    // With negative spacing the final glyph extends past the final cursor.
    // WW -> Ai fits the advance, but i would retreat with -2.5 Tc.
    let mut doc = content(
        tests::fixture(),
        b"BT /F1 10 Tf -2.5 Tc 40 180 Td (WW) Tj ET",
    );
    let before = doc.objects.clone();
    let update = Change {
        replacement: "Ai".into(),
        ..tests::change(&doc)
    };
    assert!(write(&mut doc, &[update])
        .unwrap_err()
        .contains("backtracking character"));
    assert_eq!(doc.objects, before);

    // Equal advances still do not permit a replacement's left/right overhang.
    let (mut source, _, _, program) = fonts::tests::fixture();
    source
        .get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .content =
        fonts::ink_tests::component_program([('B', -10, 0, 16384), ('D', 250, 0, 16384)]);
    let source = content(source, b"BT /F1 10 Tf 2 Tc 40 180 Td (A) Tj ET");
    for replacement in ["B", "D"] {
        let mut doc = source.clone();
        let before = doc.objects.clone();
        let update = Change {
            replacement: replacement.into(),
            ..tests::change(&doc)
        };
        assert!(write(&mut doc, &[update])
            .unwrap_err()
            .contains("replacement ink"));
        assert_eq!(doc.objects, before);
    }
}

#[test]
fn textedit_spacing_source_clips_use_ink_instead_of_final_cursor() {
    for (spacing, right) in [(-2, 50.), (2, 54.)] {
        for (edge, accepted) in [(right, true), (right - 0.01, false)] {
            let bytes =
                format!("q 0 0 {edge} 240 re W n BT /F1 10 Tf {spacing} Tc 40 180 Td (AB) Tj ET Q");
            let runs = super::tests::clipped_roundtrip(&embedded(bytes.as_bytes()));
            assert!((f64::from(runs.runs[0].display_rect[2]) - edge).abs() < 0.0001);
            assert_eq!(edge == right, accepted);
        }
    }
}

#[test]
fn textedit_spacing_bounds_and_malformed_setters_remain_refused() {
    for (value, accepted) in [
        ("2.5", true),
        ("-2.5", true),
        ("2.5001", false),
        ("-2.5001", false),
    ] {
        let bytes = format!("{value} Tc BT /F1 10 Tf 40 180 Td (AB) Tj ET");
        assert_eq!(
            scan(&embedded(bytes.as_bytes()), 0).is_ok(),
            accepted,
            "{value}"
        );
    }
    for value in [
        "", "0 0", "(0)", "/Zero", "[0]", "true", "null", "1000001", "-1000001",
    ] {
        let bytes = format!("{value} Tc 0 Tc BT /F1 10 Tf 40 180 Td (AB) Tj ET");
        assert!(scan(&embedded(bytes.as_bytes()), 0).is_err(), "{value}");
    }
    let doc = content(
        tests::fixture(),
        b"BT /F1 1000 Tf -222 Tc 40 180 Td (i) Tj ET",
    );
    assert!(scan(&doc, 0)
        .unwrap_err()
        .contains("backtracking character"));
    let bytes = format!(
        "BT /F1 1000 Tf 250 Tc 40 180 Td ({}) Tj ET",
        "W".repeat(1000)
    );
    let doc = content(tests::fixture(), bytes.as_bytes());
    assert!(scan(&doc, 0)
        .unwrap_err()
        .contains("spaced text advance exceeds"));
    for bytes in [
        b"2 Tc BT /F1 10 Tf (AB) Tj ET".as_slice(),
        b"BT /F1 10 Tf 2 Tc (AB) Tj ET",
    ] {
        assert!(scan(&embedded(bytes), 0).is_err());
    }
}

#[test]
fn textedit_large_word_spacing_preserves_scaled_continuations_and_ink_bounds() {
    let mut doc = embedded(b"12 Tw BT /F1 1 Tf 8 0 0 8 40 180 Tm (A B ) Tj (C) Tj ET");
    let before = inspect(&doc, 0).unwrap();
    let first = &before.runs.runs[0];
    close(first.advance, 26.4);
    close(before.horizontal_bounds[&first.operator][1], 14.4);
    close(before.runs.runs[1].matrix[4], 251.2);
    let resources = resources(&doc, before.id).unwrap().clone();
    let change = Change {
        replacement: "B ".into(),
        ..tests::change(&doc)
    };
    write(&mut doc, &[change]).unwrap();
    let after = inspect(&doc, 0).unwrap();
    assert_eq!(after.runs.runs[0].text, "B ");
    assert_eq!(after.runs.runs[1], before.runs.runs[1]);
    assert_eq!(*super::resources(&doc, before.id).unwrap(), resources);
    assert_eq!(
        after.content.operations[0].operator,
        before.content.operations[0].operator
    );
    assert_eq!(
        after.content.operations[0].operands,
        before.content.operations[0].operands
    );

    // A second space has little ink but exceeds the source advance. Removing
    // a space fits that advance yet can move replacement ink past its envelope.
    for (source, replacement, reason) in [
        ("A B", "A  B", "original text advance"),
        ("A ", "ABC", "original text bounds"),
    ] {
        let mut doc =
            embedded(format!("12 Tw BT /F1 1 Tf 8 0 0 8 40 180 Tm ({source}) Tj ET").as_bytes());
        let objects = doc.objects.clone();
        let change = Change {
            replacement: replacement.into(),
            ..tests::change(&doc)
        };
        assert!(write(&mut doc, &[change]).unwrap_err().contains(reason));
        assert_eq!(doc.objects, objects);
    }
}

#[test]
fn textedit_large_word_spacing_bounds_cursor_and_restores_inactive_spacing() {
    for (value, source, accepted) in [
        ("999999", " ", true),
        ("1000000", " ", false),
        ("1000000", "AB", true),
        ("500000", "  ", false),
    ] {
        let body = format!("{value} Tw BT /F1 1 Tf 40 180 Td ({source}) Tj ET");
        let result = scan(&embedded(body.as_bytes()), 0);
        assert_eq!(result.is_ok(), accepted, "{value} {source:?}: {result:?}");
    }
    let doc = embedded(b"12 Tw BT /F1 1 Tf 40 180 Td (A B) Tj ET q 0 Tw BT 40 140 Td (A B) Tj ET Q BT 40 100 Td (A B) Tj ET");
    let runs = scan(&doc, 0).unwrap().runs;
    close(runs[0].advance, 13.8);
    close(runs[1].advance, 1.8);
    close(runs[2].advance, 13.8);
}

#[test]
fn textedit_word_spacing_measures_combined_steps_and_kerning_fragments() {
    for word in [-2, 0, 2] {
        for (show, adjustment) in [("(A B ) Tj", 0.), ("[(A ) 100 (B )] TJ", 1.)] {
            let bytes = format!("1 Tc {word} Tw BT /F1 10 Tf 40 180 Td {show} ET");
            let page = inspect(&embedded(bytes.as_bytes()), 0).unwrap();
            let run = &page.runs.runs[0];
            close(run.advance, 28. + 2. * f64::from(word) - adjustment);
            close(
                page.horizontal_bounds[&run.operator][1],
                27. + f64::from(word) - adjustment,
            );
        }
    }
}

#[test]
fn textedit_word_spacing_restores_state_and_checks_replacements_atomically() {
    let mut doc = embedded(b"-2 Tw BT /F1 10 Tf 40 180 Td (A B) Tj ET q 2 Tw BT 40 140 Td (A B) Tj ET Q BT 40 100 Td (A B) Tj ET");
    let before = inspect(&doc, 0).unwrap();
    assert_eq!(
        before
            .runs
            .runs
            .iter()
            .map(|r| r.advance)
            .collect::<Vec<_>>(),
        [16., 20., 16.]
    );
    let unchanged = doc.objects.clone();
    // Removing the space would fit if the writer forgot the source's negative Tw.
    let update = Change {
        replacement: "ABC".into(),
        ..tests::change(&doc)
    };
    assert!(write(&mut doc, &[update])
        .unwrap_err()
        .contains("original text advance"));
    assert_eq!(doc.objects, unchanged);
    let updates = [0, 2].map(|index| Change {
        layout: None,
        page: 0,
        revision: before.runs.revision.clone(),
        operator: before.runs.runs[index].operator,
        original: "A B".into(),
        replacement: "B A".into(),
    });
    write(&mut doc, &updates).unwrap();
    let after = inspect(&doc, 0).unwrap();
    assert_eq!(after.runs.runs[1], before.runs.runs[1]);
    for index in [0, 2] {
        assert_eq!(after.runs.runs[index].text, "B A");
        close(after.runs.runs[index].advance, 16.);
    }
    for (index, (old, new)) in before
        .content
        .operations
        .iter()
        .zip(&after.content.operations)
        .enumerate()
    {
        if !updates.iter().any(|u| u.operator as usize == index) {
            assert_eq!(
                (&old.operator, &old.operands),
                (&new.operator, &new.operands)
            );
        }
    }
    assert_eq!(
        scan(&doc, 1).unwrap().runs,
        scan(&embedded(b"BT /F1 10 Tf 40 180 Td (AB) Tj ET"), 1)
            .unwrap()
            .runs
    );

    // An added space fits without Tw, but positive Tw can make it overflow.
    let mut doc = content(tests::fixture(), b"2 Tw BT /F1 10 Tf 40 180 Td (A) Tj ET");
    let unchanged = doc.objects.clone();
    let update = Change {
        replacement: "i ".into(),
        ..tests::change(&doc)
    };
    assert!(write(&mut doc, &[update])
        .unwrap_err()
        .contains("original text advance"));
    assert_eq!(doc.objects, unchanged);
}

#[test]
fn textedit_word_spacing_bounds_combined_backtracking_and_positioning() {
    for (value, accepted) in [
        ("2.5", true),
        ("-2.5", true),
        ("2.5001", true),
        ("12.112", true),
        ("-2.5001", false),
    ] {
        let bytes = format!("{value} Tw BT /F1 10 Tf 40 180 Td (A B) Tj ET");
        assert_eq!(
            scan(&embedded(bytes.as_bytes()), 0).is_ok(),
            accepted,
            "{value}"
        );
    }
    for value in [
        "", "0 0", "(0)", "/Zero", "[0]", "true", "null", "1000001", "-1000001",
    ] {
        let bytes = format!("{value} Tw 0 Tw BT /F1 10 Tf 40 180 Td (A B) Tj ET");
        assert!(scan(&embedded(bytes.as_bytes()), 0).is_err(), "{value}");
    }
    // Each spacing alone is legal, but their sum reverses a Helvetica space.
    let doc = content(
        tests::fixture(),
        b"-2 Tc -2 Tw BT /F1 10 Tf 40 180 Td (A B) Tj ET",
    );
    assert!(scan(&doc, 0)
        .unwrap_err()
        .contains("backtracking character"));
    for bytes in [
        b"2 Tw BT /F1 10 Tf (A B) Tj ET".as_slice(),
        b"BT /F1 10 Tf 2 Tw (A B) Tj ET",
    ] {
        assert!(scan(&embedded(bytes), 0).is_err());
    }
}
