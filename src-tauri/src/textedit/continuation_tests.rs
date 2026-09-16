use super::*;

#[test]
fn textedit_continued_precision_and_accumulated_bounds_fail_atomically() {
    let mut doc =
        tests::with_content(b"BT /F1 12 Tf 0.125 Tc 1000 0 0 1000 0 0 Tm (AAA) Tj (SECOND) Tj ET");
    let runs = scan(&doc, 0).unwrap();
    let original = doc.objects.clone();
    let edit = Change {
        layout: None,
        page: 0,
        revision: runs.revision,
        operator: runs.runs[0].operator,
        original: "AAA".into(),
        replacement: "A".into(),
    };
    assert!(write(&mut doc, &[edit])
        .unwrap_err()
        .contains("PDF number precision"));
    assert_eq!(doc.objects, original);
    let huge = format!(
        "BT /F1 1000 Tf 0 0 Td ({}) Tj ({}) Tj ET",
        "W".repeat(600),
        "W".repeat(600)
    );
    assert!(scan(&tests::with_content(huge.as_bytes()), 0)
        .unwrap_err()
        .contains("continued text advance"));
    assert!(scan(
        &tests::with_content(b"BT /F1 12 Tf 999999 0 Td (FIRST) Tj (SECOND) Tj ET"),
        0
    )
    .unwrap_err()
    .contains("position exceeds"));
}

#[test]
fn textedit_continued_discovery_requires_representable_deletion() {
    for body in [
        "BT /F1 12 Tf 0.1234567 Tc 1000 0 0 1000 0 0 Tm (FIRST) Tj (SECOND) Tj ET",
        "q 12.112 Tw BT /F1 12 Tf 30 TL 40 180 Td (ACME SYNTHETIC TEXT) Tj [( SECOND) -125] TJ ( THIRD) Tj T* (NEXT LINE) Tj ET Q",
    ] {
        let doc = tests::with_content(body.as_bytes());
        assert!(scan(&doc, 0).unwrap_err().contains("PDF number precision"));
    }
    for body in [
        "BT /F1 12 Tf 0.125 Tc 1000 0 0 1000 0 0 Tm (AAA) Tj (SECOND) Tj ET",
        "q 12 Tw BT /F1 12 Tf 40 180 Td (ACME SYNTHETIC TEXT) Tj ( SECOND) Tj ET Q",
    ] {
        let mut doc = tests::with_content(body.as_bytes());
        let runs = scan(&doc, 0).unwrap();
        let change = Change {
            layout: None,
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: runs.runs[0].text.clone(),
            replacement: String::new(),
        };
        write(&mut doc, &[change]).unwrap();
        let after = scan(&doc, 0).unwrap();
        assert!(after.runs[0].text.is_empty());
        for (actual, expected) in after.runs[1].matrix.iter().zip(runs.runs[1].matrix) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn textedit_continued_shows_keep_followers_fixed_when_shortened_or_deleted() {
    for first in ["(FIRST) Tj", "[(FI) -20 (RST)] TJ"] {
        for replacement in ["FI", ""] {
            let mut doc = tests::with_content(format!("BT /F1 12 Tf 30 TL 40 180 Td {first} 0.1 Tc 0.2 Tw (SECOND WORD) Tj T* (THIRD) Tj ET").as_bytes());
            let before = scan(&doc, 0).unwrap();
            assert_eq!(before.runs.len(), 3);
            assert!((before.runs[1].matrix[4] - 40. - before.runs[0].advance).abs() < 1e-6);
            assert_eq!(before.runs[2].matrix[4..], [40., 150.]);
            let change = Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: replacement.into(),
            };
            write(&mut doc, &[change]).unwrap();
            let after = scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, replacement);
            for index in 1..3 {
                assert_eq!(after.runs[index].text, before.runs[index].text);
                for (a, b) in after.runs[index]
                    .matrix
                    .iter()
                    .zip(before.runs[index].matrix)
                {
                    assert!((a - b).abs() < 1e-6, "follower moved: {a} != {b}");
                }
            }
        }
    }
}

#[test]
fn textedit_continued_shows_follow_transformed_axes_and_reset_at_line_moves() {
    for matrix in [
        "2 0 0 3 100 100",
        "0 2 -3 0 100 100",
        "-2 0 0 -3 100 100",
        "0 -2 3 0 100 100",
    ] {
        let doc = tests::with_content(format!("BT /F1 12 Tf 10 TL {matrix} Tm (FIRST) Tj /F1 8 Tf (SECOND) Tj 4 -5 Td (THIRD) Tj T* (FOURTH) Tj ET").as_bytes());
        let runs = scan(&doc, 0).unwrap().runs;
        let m = runs[0].matrix;
        assert!((runs[1].matrix[4] - m[4] - m[0] * runs[0].advance).abs() < 1e-6);
        assert!((runs[1].matrix[5] - m[5] - m[1] * runs[0].advance).abs() < 1e-6);
        assert_eq!(runs[2].matrix[4], m[4] + 4. * m[0] - 5. * m[2]);
        assert_eq!(runs[3].matrix[5], m[5] + 4. * m[1] - 15. * m[3]);
    }
}

#[test]
fn textedit_continued_batch_preserves_every_advance_and_stream_boundary() {
    let source = b"% retain this\nBT /F1 12 Tf 40 180 Td (FIRST) Tj % between\n[(SECOND) -100] TJ (THIRD) Tj ET";
    let mut doc = tests::with_content(source);
    let before = scan(&doc, 0).unwrap();
    let changes: Vec<_> = before
        .runs
        .iter()
        .map(|run| Change {
            layout: None,
            page: 0,
            revision: before.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: run.text[..1].into(),
        })
        .collect();
    write(&mut doc, &changes).unwrap();
    let after = scan(&doc, 0).unwrap();
    for (a, b) in after.runs.iter().zip(&before.runs) {
        assert_eq!(a.text, b.text[..1]);
        assert!((a.matrix[4] - b.matrix[4]).abs() < 1e-6);
    }
    let bytes = page_content(&doc, crate::pagetree::ordered_pages(&doc)[0]).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("% retain this\nBT /F1 12 Tf 40 180 Td "));
    assert!(text.contains("% between\n"));
    // A new text block must not inherit the previous block's advanced cursor.
    assert!(scan(
        &tests::with_content(b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT (SECOND) Tj ET"),
        0
    )
    .is_err());
}
