use crate::textedit::{self, tests::with_content, Change};

#[test]
fn textedit_inline_separators_preserve_bytes_and_positions_without_edit_targets() {
    for (actual, show) in [
        ("<09>", "( ) Tj"),
        ("<FEFF0007>", "( ) Tj"),
        ("<FEFF00090009>", "[( ) -125 ( )] TJ"),
    ] {
        for position in ["", "40 140 Td", "0 1 -1 0 100 100 Tm"] {
            let span = format!("/Span << /ActualText {actual} >> BDC {position} {show} EMC");
            let source = format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj {span} (SECOND) Tj ET");
            let mut doc = with_content(source.as_bytes());
            let before = textedit::scan(&doc, 0).unwrap();
            assert_eq!(
                before
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>(),
                ["FIRST", "SECOND"]
            );
            let mut invalid = Change {
                layout: None,
                page: 0,
                revision: before.revision.clone(),
                operator: before.runs[1].operator - 2,
                original: " ".into(),
                replacement: "".into(),
            };
            let original = doc.objects.clone();
            assert!(textedit::write(&mut doc, &[invalid.clone()])
                .unwrap_err()
                .contains("no longer exists"));
            assert_eq!(doc.objects, original);
            invalid.operator = before.runs[0].operator;
            invalid.original = "FIRST".into();
            invalid.replacement = "FI".into();
            textedit::write(&mut doc, &[invalid]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, "FI");
            assert_eq!(after.runs[1].text, "SECOND");
            for (a, b) in before.runs[1].matrix.iter().zip(after.runs[1].matrix) {
                assert!((a - b).abs() < 1e-6);
            }
            let bytes =
                textedit::page_content(&doc, crate::pagetree::ordered_pages(&doc)[0]).unwrap();
            assert!(String::from_utf8(bytes).unwrap().contains(&span));
        }
    }
}

#[test]
fn textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences() {
    for span in [
        "/Span << /ActualText (WORDS) >> BDC ( ) Tj EMC",
        "/Span << /ActualText <09> /MCID 0 >> BDC ( ) Tj EMC",
        // An MCID takes the tagging branch; these reach the separator guard.
        "/Span << /ActualText <09> /Lang (en-US) >> BDC ( ) Tj EMC",
        "/Span << /ActualText <09> /Alt (SYNTHETIC) >> BDC ( ) Tj EMC",
        "/Span << /ActualText <09> /E (SYNTHETIC) >> BDC ( ) Tj EMC",
        "/Other << /ActualText <09> >> BDC ( ) Tj EMC",
        "/Span /Named BDC ( ) Tj EMC",
        "/Span << /ActualText <> >> BDC ( ) Tj EMC",
        "/Span << /ActualText <FEFF00> >> BDC ( ) Tj EMC",
        "/Span << /ActualText <FEFF000A> >> BDC ( ) Tj EMC",
        "/Span << /ActualText <09> >> BDC (X) Tj EMC",
        "/Span << /ActualText <09> >> BDC (  ) Tj EMC",
        "/Span << /ActualText <09> >> BDC EMC",
        "/Span << /ActualText <09> >> BDC ( ) Tj",
        "/Span << /ActualText <09> >> BDC ( ) Tj ( ) Tj EMC",
        "/Span << /ActualText <09> >> BDC 0 0 Td 0 0 Td ( ) Tj EMC",
        "/Span << /ActualText <09> >> BDC ( ) Tj 0 0 Td EMC",
        "/Span << /ActualText <09> >> BDC /F1 10 Tf ( ) Tj EMC",
        "/Span << /ActualText <09> >> BDC /Span << /ActualText <09> >> BDC ( ) Tj EMC EMC",
        "/Span << /ActualText <09> >> BDC ET BT ( ) Tj EMC",
        "/Span << /ActualText <09> >> BDC 1000001 0 Td ( ) Tj EMC",
    ] {
        let source = format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj {span} (SECOND) Tj ET");
        assert!(
            textedit::scan(&with_content(source.as_bytes()), 0).is_err(),
            "{span}"
        );
    }
    for count in [32, 33] {
        let source = format!(
            "BT /F1 12 Tf 40 180 Td /Span << /ActualText <{}> >> BDC ({}) Tj EMC (SECOND) Tj ET",
            "09".repeat(count),
            " ".repeat(count)
        );
        assert_eq!(
            textedit::scan(&with_content(source.as_bytes()), 0).is_ok(),
            count == 32
        );
    }
}
