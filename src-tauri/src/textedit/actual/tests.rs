use crate::textedit::*;

fn fixture(body: &str) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    doc
}

#[test]
fn actual_text_single_line_layout_survives_reopen_and_a_second_edit() {
    for replacement in ["IN", ""] {
        let mut doc=fixture("BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC (SECOND) Tj ET");
        let before = scan(&doc, 0).unwrap();
        write(
            &mut doc,
            &[Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: replacement.into(),
                layout: Some(Layout {
                    width: 36.,
                    height: 20.,
                    size: 12.,
                    wrap: false,
                    font: EditFont::Original,
                }),
            }],
        )
        .unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.runs.runs.len(), 2);
        assert_eq!(after.runs.runs[0].text, replacement);
        let at = after.actual_text[&after.runs.runs[0].operator];
        assert_eq!(
            lopdf::decode_text_string(
                after.content.operations[at].operands[1]
                    .as_dict()
                    .unwrap()
                    .get(b"ActualText")
                    .unwrap()
            )
            .unwrap(),
            replacement
        );
        let second = if replacement.is_empty() { "IN" } else { "I" };
        write(
            &mut doc,
            &[Change {
                page: 0,
                revision: after.runs.revision,
                operator: after.runs.runs[0].operator,
                original: replacement.into(),
                replacement: second.into(),
                layout: Some(Layout {
                    width: 36.,
                    height: 20.,
                    size: 12.,
                    wrap: false,
                    font: EditFont::Original,
                }),
            }],
        )
        .unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, second);
    }
}

#[test]
fn actual_text_edits_update_logical_text_and_preserve_following_positions() {
    for actual in [
        "(FIRST)",
        "<FEFF00460049005200530054>",
        "<EFBBBF4649525354>",
    ] {
        for outer in [false, true] {
            for replacement in ["IN", ""] {
                let span = format!("/Span << /ActualText {actual} >> BDC");
                let body = if outer {
                    format!("{span} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET")
                } else {
                    format!("BT /F1 12 Tf 40 180 Td {span} (FIRST) Tj EMC (SECOND) Tj ET")
                };
                let mut doc = fixture(&body);
                let before = inspect(&doc, 0).unwrap();
                let run = &before.runs.runs[0];
                let at = before.actual_text[&run.operator];
                write(
                    &mut doc,
                    &[Change {
                        page: 0,
                        revision: before.runs.revision.clone(),
                        operator: run.operator,
                        original: run.text.clone(),
                        replacement: replacement.into(),
                        layout: None,
                    }],
                )
                .unwrap();
                let after = inspect(&doc, 0).unwrap();
                assert_eq!(after.runs.runs[0].text, replacement);
                assert_eq!(after.runs.runs[1].text, "SECOND");
                for (a, b) in before.runs.runs[1]
                    .matrix
                    .iter()
                    .zip(after.runs.runs[1].matrix)
                {
                    assert!((a - b).abs() < 1e-6);
                }
                let logical = after.content.operations[at].operands[1]
                    .as_dict()
                    .unwrap()
                    .get(b"ActualText")
                    .unwrap();
                assert_eq!(lopdf::decode_text_string(logical).unwrap(), replacement);
                assert_eq!(after.actual_text.len(), 1);
            }
        }
    }
}

#[test]
fn alternate_actual_text_preserves_span_and_refuses_forged_edits_and_overlap() {
    for inner in ["(FIRST) Tj", "(FI) Tj (RST) Tj"] {
        let span = format!(
            "/Span << /ActualText <FEFF00A0> >> BDC /F1 12 Tf 1 0 0 1 90 180 Tm {inner} EMC"
        );
        let mut doc = fixture(&format!("BT {span} 1 0 0 1 40 180 Tm (FIRST) Tj ET"));
        let before = inspect(&doc, 0).unwrap();
        assert_eq!(before.runs.runs.len(), 1);
        assert_eq!(
            before.preserved.len(),
            if inner == "(FIRST) Tj" { 1 } else { 2 }
        );
        let run = &before.runs.runs[0];
        let mut change = Change {
            page: 0,
            revision: before.runs.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: "IN".into(),
            layout: None,
        };
        let mut forged = change.clone();
        forged.operator = before.preserved[0].operator;
        forged.original = before.preserved[0].text.clone();
        let original = doc.objects.clone();
        assert!(write(&mut doc, &[forged]).is_err());
        assert_eq!(doc.objects, original);
        change.replacement = "FIRST FIRST FIRST".into();
        change.layout = Some(Layout {
            width: 200.,
            height: 20.,
            size: 12.,
            wrap: false,
            font: EditFont::Original,
        });
        assert!(write(&mut doc, &[change.clone()])
            .unwrap_err()
            .contains("overlap"));
        assert_eq!(doc.objects, original);
        change.replacement = "IN".into();
        change.layout = None;
        write(&mut doc, &[change]).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(before.preserved, after.preserved);
        assert!(doc
            .get_page_content(before.id)
            .windows(span.len())
            .any(|bytes| bytes == span.as_bytes()));
    }
}

#[test]
fn actual_text_rejects_invalid_encodings_nesting_metadata_and_unclosed_spans() {
    for body in [
        "/Span << /ActualText <FEFFD800> >> BDC (FIRST) Tj EMC",
        "/Span << /ActualText <FEFF00> >> BDC (FIRST) Tj EMC",
        "/Span << /ActualText <EFBBBFFF> >> BDC (FIRST) Tj EMC",
        "/Span << /ActualText (FIRST) /Alt (SYNTHETIC) >> BDC (FIRST) Tj EMC",
        "/Span << /ActualText (FIRST) >> BDC /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC EMC",
        "/Span << /ActualText (FIRST) >> BDC EMC",
        "/Span << /ActualText (FIRST) >> BDC (FIRST) Tj",
    ] {
        let mut doc = fixture(&format!("BT /F1 12 Tf 40 180 Td {body} ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET"));
        assert!(scan(&doc,0).is_err(), "accepted {body}");
        let original = doc.objects.clone();
        assert!(write(&mut doc, &[Change {page:0, revision:vec![], operator:0, original:"FIRST".into(), replacement:"IN".into(), layout:None}]).is_err());
        assert_eq!(doc.objects,original);
    }
}

#[test]
fn actual_text_standard_font_matching_text_stays_editable() {
    let mut doc = tests::with_content(
        b"BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC ET",
    );
    let before = scan(&doc, 0).unwrap();
    write(
        &mut doc,
        &[Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "".into(),
            layout: None,
        }],
    )
    .unwrap();
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
}
