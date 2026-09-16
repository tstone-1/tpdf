use super::*;

#[test]
fn textedit_td_sets_signed_leading_and_resets_both_text_origins() {
    for matrix in [
        "2 0 0 3 100 100",
        "0 2 -3 0 100 100",
        "-2 0 0 -3 100 100",
        "0 -2 3 0 100 100",
    ] {
        for y in [-5., 0., 5.] {
            let source = format!("q 2 0 0 2 10 20 cm BT /F1 12 Tf 13 TL {matrix} Tm (FIRST) Tj (FOLLOWER) Tj 4 {y} TD (SECOND) Tj T* (THIRD) Tj 1 -2 Td (FOURTH) Tj T* (FIFTH) Tj ET Q");
            let mut doc = tests::with_content(source.as_bytes());
            let before = scan(&doc, 0).unwrap();
            let m = before.runs[0].matrix;
            for (index, x, y) in [
                (2, 4., y),
                (3, 4., y * 2.),
                (4, 5., y * 2. - 2.),
                (5, 5., y * 3. - 2.),
            ] {
                assert_eq!(before.runs[index].matrix[4], m[4] + x * m[0] + y * m[2]);
                assert_eq!(before.runs[index].matrix[5], m[5] + x * m[1] + y * m[3]);
            }
            let changes = [0, 1, 2].map(|index| Change {
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[index].operator,
                original: before.runs[index].text.clone(),
                replacement: if index == 1 { "" } else { "F" }.into(),
            });
            write(&mut doc, &changes).unwrap();
            let after = scan(&doc, 0).unwrap();
            for (old, new) in before.runs.iter().zip(&after.runs) {
                for (a, b) in old.matrix.iter().zip(new.matrix) {
                    assert!((a - b).abs() < 1e-6);
                }
            }
            assert_eq!(after.runs[0].text, "F");
            assert!(after.runs[1].text.is_empty());
            assert_eq!(after.runs[2].text, "F");
            assert_eq!(after.runs[3..], before.runs[3..]);
            let content = Content::decode(
                &page_content(&doc, crate::pagetree::ordered_pages(&doc)[0]).unwrap(),
            )
            .unwrap();
            // Only FIRST has a continued follower. TD starts a fresh line,
            // so deleting FOLLOWER must not introduce a compensation array.
            assert_eq!(
                content.operations[after.runs[0].operator as usize].operator,
                "TJ"
            );
            assert_eq!(
                content.operations[after.runs[1].operator as usize].operator,
                "Tj"
            );
        }
    }
}

#[test]
fn textedit_td_leading_persists_across_blocks_and_restores_with_graphics_state() {
    let doc = tests::with_content(b"BT /F1 12 Tf 40 180 TD (FIRST) Tj ET q BT 0 -20 TD ET Q BT 40 40 Td (SECOND) Tj T* (THIRD) Tj 13 TL T* (FOURTH) Tj ET");
    let runs = scan(&doc, 0).unwrap().runs;
    assert_eq!(
        runs.iter()
            .map(|r| [r.matrix[4], r.matrix[5]])
            .collect::<Vec<_>>(),
        [[40., 180.], [40., 40.], [40., 220.], [40., 207.]]
    );
}

#[test]
fn textedit_td_preserves_authored_operators_and_comments_on_save() {
    let source =
        b"% SYNTHETIC\nBT /F1 12 Tf 13 TL 20 220 Td % keep\n20 -40 TD (FIRST) Tj T* (SECOND) Tj ET";
    let mut doc = tests::with_content(source);
    let before = scan(&doc, 0).unwrap();
    write(
        &mut doc,
        &[Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "F".into(),
        }],
    )
    .unwrap();
    let bytes = page_content(&doc, crate::pagetree::ordered_pages(&doc)[0]).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap().trim_end_matches('\n'),
        "% SYNTHETIC\nBT /F1 12 Tf 13 TL 20 220 Td % keep\n20 -40 TD (F) Tj T* (SECOND) Tj ET"
    );
}

#[test]
fn textedit_td_refuses_malformed_unbounded_and_inline_state_atomically() {
    for source in [
        "0 -12 TD BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 -12 1 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj /SYNTHETIC -12 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 1000001 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 1000001 0 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 999999 0 TD ET",
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj /Span << /ActualText <FEFF0009> >> BDC 0 -12 TD ( ) Tj EMC ET",
    ] {
        let mut doc = tests::with_content(source.as_bytes());
        let original = doc.objects.clone();
        assert!(scan(&doc, 0).is_err(), "{source}");
        assert!(write(&mut doc, &[Change {
            page: 0,
            revision: Vec::new(),
            operator: 3,
            original: "FIRST".into(),
            replacement: "F".into(),
        }]).is_err(), "{source}");
        assert_eq!(doc.objects, original);
    }
}
