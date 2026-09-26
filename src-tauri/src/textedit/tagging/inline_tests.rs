use super::tests::fixture;
use crate::textedit::{self, Change};
use lopdf::{content::Content, Dictionary, Stream};

const FIRST: &str = "/Standard << /MCID 0 >> BDC";
const SECOND: &str = "/Standard << /MCID 1 >> BDC";

#[test]
fn textedit_inline_tags_preserve_continuations_and_every_unedited_operator() {
    for position in ["", "0 -40 Td", "1 0 0 1 40 140 Tm"] {
        for replacement in ["FI", ""] {
            let source = format!("BT /F1 12 Tf 40 180 Td {FIRST} (FIRST) Tj EMC % keep boundary\n{SECOND} {position} (SECOND) Tj EMC ET");
            let (mut doc, ids) = fixture(source.as_bytes());
            let before = textedit::scan(&doc, 0).unwrap();
            assert_eq!(before.runs.len(), 2);
            let expected = if position.is_empty() {
                [40. + before.runs[0].advance, 180.]
            } else {
                [40., 140.]
            };
            assert_eq!(before.runs[1].matrix[4..], expected);
            let original = doc.objects.clone();
            let target = before.runs[0].operator as usize;
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: replacement.into(),
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, replacement);
            assert_eq!(after.runs[1].text, "SECOND");
            for (a, b) in after.runs[1].matrix.iter().zip(before.runs[1].matrix) {
                assert!((a - b).abs() < 1e-6);
            }
            let saved = doc.get_page_content(ids[0]);
            let ops = Content::decode(&saved).unwrap().operations;
            let previous = Content::decode(source.as_bytes()).unwrap().operations;
            assert_eq!(ops.len(), previous.len());
            for (index, (a, b)) in ops.iter().zip(previous).enumerate() {
                if index != target {
                    assert_eq!(a.operator, b.operator);
                    assert_eq!(a.operands, b.operands);
                }
            }
            assert!(String::from_utf8(saved)
                .unwrap()
                .contains("EMC % keep boundary\n"));
            for (id, value) in original {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], value);
                }
            }
        }
    }
}

#[test]
fn textedit_inline_tags_balance_independently_of_text_objects_and_keep_spacers() {
    // PDF text objects and marked-content sequences are separately balanced.
    // Opening and closing a tag never resets either text matrix.
    for (begin, end) in [(true, true), (false, true), (true, false), (false, false)] {
        for spacer in ["", "/Span << /ActualText <09> >> BDC ( ) Tj EMC"] {
            let start = if begin {
                format!("BT {FIRST}")
            } else {
                format!("{FIRST} BT")
            };
            let end = if end { "EMC ET" } else { "ET EMC" };
            let source = format!("{start} /F1 12 Tf 40 180 Td (FIRST) Tj {spacer} {end} BT /F1 12 Tf 40 140 Td {SECOND} (SECOND) Tj EMC ET");
            let (mut doc, _) = fixture(source.as_bytes());
            let before = textedit::scan(&doc, 0).unwrap();
            assert_eq!(before.runs.len(), 2);
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "FI".into(),
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, "FI");
            assert_eq!(after.runs[1], before.runs[1]);
        }
    }
}

#[test]
fn textedit_inline_tags_refuse_invalid_semantics_and_balance_atomically() {
    let source = format!(
        "BT /F1 12 Tf 40 180 Td {FIRST} (FIRST) Tj EMC {SECOND} 0 -40 Td (SECOND) Tj EMC ET"
    );
    let invalid = [
        source.replacen("/MCID 0", "/MCID 0 /ActualText <09>", 1),
        source.replacen("/MCID 0", "/MCID 0 /Alt (STALE)", 1),
        source.replacen("/MCID 0", "/MCID 0 /Private (SECRET)", 1),
        source.replacen("/MCID 0", "/MCID -1", 1),
        source.replacen("/MCID 0", "/MCID 0.5", 1),
        source.replacen("/MCID 0", "/MCID 2", 1),
        source.replacen("/MCID 1", "/MCID 0", 1),
        source.replacen("/Standard", "/Artifact", 1),
        source.replacen("(FIRST) Tj", &format!("{FIRST} (FIRST) Tj EMC"), 1),
        source.replacen("(FIRST) Tj", "0 0 Td", 1),
        source.replacen("EMC", "", 1),
        format!("{source} EMC"),
        source.replace(" EMC ET", " ET"),
        source.replacen(FIRST, "", 1).replacen("EMC", "", 1),
        source.replace(" EMC ET", " EMC"),
        source.replacen("BT", "BT BT", 1),
        source.replacen("EMC", "EMC ET", 1),
    ];
    for (case, content) in invalid.iter().enumerate() {
        let (mut doc, ids) = fixture(source.as_bytes());
        let before = textedit::scan(&doc, 0).unwrap();
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        let original = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "case {case}");
        assert!(
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "FI".into(),
                }]
            )
            .is_err(),
            "case {case}"
        );
        assert_eq!(doc.objects, original);
    }
    let untagged = textedit::tests::with_content(source.as_bytes());
    assert!(textedit::scan(&untagged, 0).is_err());
}

// LibreOffice marks a table of contents' dot leaders `/Artifact BMC` inside the
// text object. They stay read-only; the entries around them stay editable.
#[test]
fn textedit_artifact_bmc_inside_a_text_object_is_read_only() {
    let content = b"BT /F1 12 Tf /Standard <</MCID 0>> BDC 40 180 Td (FIRST) Tj EMC /Standard <</MCID 1>> BDC 0 -40 Td (SECOND) Tj EMC /Artifact BMC 0 -40 Td (DOTS) Tj EMC ET";
    let (mut doc, _) = super::tests::fixture(content);
    let scan = crate::textedit::scan(&doc, 0).unwrap();
    assert_eq!(
        scan.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["FIRST", "SECOND"]
    );
    crate::textedit::write(
        &mut doc,
        &[crate::textedit::Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    // Any other tag without properties still has nothing to own it.
    let span = String::from_utf8_lossy(content).replace("/Artifact BMC", "/Span BMC");
    let (doc, _) = super::tests::fixture(span.as_bytes());
    assert!(crate::textedit::scan(&doc, 0).is_err());
}

// Acrobat stamps page numbers on untagged scans as an artifact. It names no
// structure element, so no tree is needed; its text stays read-only and the
// page's other text stays editable. Any other marked content still needs one.
#[test]
fn textedit_artifacts_on_untagged_pages_are_read_only() {
    let untagged = |content: &str| {
        let (mut doc, _, _, _) = crate::textedit::fonts::tests::fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        doc
    };
    let stamp = "/Artifact <</Type /Pagination /Subtype /Footer /Contents (2)>> BDC BT /F1 12 Tf 40 100 Td (SECOND) Tj ET EMC";
    for artifact in [
        stamp,
        "/Artifact BMC BT /F1 12 Tf 40 100 Td (SECOND) Tj ET EMC",
    ] {
        let mut doc = untagged(&format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {artifact}"));
        let scan = textedit::scan(&doc, 0).unwrap();
        assert_eq!(
            scan.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["FIRST"],
            "{artifact}"
        );
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
    }
    for refused in [
        "/Span BMC BT /F1 12 Tf 40 100 Td (SECOND) Tj ET EMC",
        "/Artifact <</MCID 0>> BDC BT /F1 12 Tf 40 100 Td (SECOND) Tj ET EMC",
        "/P <</MCID 0>> BDC BT /F1 12 Tf 40 100 Td (SECOND) Tj ET EMC",
    ] {
        let doc = untagged(&format!("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET {refused}"));
        // Later checks would refuse these too; the survey reports this reason.
        assert!(
            textedit::scan(&doc, 0)
                .unwrap_err()
                .contains("no supported structure tree"),
            "{refused}"
        );
    }
}
