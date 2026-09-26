use super::tests::{fixture, CONTENT};
use crate::textedit;
use lopdf::{dictionary, Object};

const UNTAGGED: &[u8] =
    b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";

fn without_tree(content: &[u8]) -> (lopdf::Document, lopdf::ObjectId) {
    let (mut doc, ids) = fixture(content);
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog)
        .unwrap()
        .remove(b"StructTreeRoot");
    (doc, ids[0])
}

#[test]
fn textedit_unused_parent_index_preserves_plain_text_edits_and_metadata() {
    for key in [0, 7, 1_000_000] {
        for replacement in ["IN", ""] {
            let (mut doc, page) = without_tree(UNTAGGED);
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("StructParents", key);
            let before = textedit::scan(&doc, 0).unwrap();
            let objects = doc.objects.clone();
            let change = textedit::Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: replacement.into(),
            };
            textedit::write(&mut doc, &[change]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, replacement);
            assert_eq!(after.runs[1], before.runs[1]);
            let mut page_before = objects[&page].as_dict().unwrap().clone();
            let mut page_after = doc.get_dictionary(page).unwrap().clone();
            page_before.remove(b"Contents");
            page_after.remove(b"Contents");
            assert_eq!(page_after, page_before);
            for (id, object) in objects {
                if id != page {
                    assert_eq!(doc.objects[&id], object);
                }
            }
        }
    }
}

#[test]
fn textedit_unused_parent_index_does_not_admit_structural_or_semantic_content() {
    for content in [
        b"/P <</MCID 0>> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC".as_slice(),
        b"BT /F1 12 Tf 40 180 Td /P <</MCID 0>> BDC (FIRST) Tj EMC ET",
        b"BT /F1 12 Tf 40 180 Td /Span <</ActualText (SYNTHETIC SECRET)>> BDC (FIRST) Tj EMC ET",
    ] {
        let (mut doc, _) = without_tree(UNTAGGED);
        let before = textedit::scan(&doc, 0).unwrap();
        let change = textedit::Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        let (changed, _) = without_tree(content);
        doc = changed;
        let objects = doc.objects.clone();
        let error = textedit::scan(&doc, 0).unwrap_err();
        assert!(!error.contains("SECRET"));
        assert!(textedit::write(&mut doc, &[change]).is_err());
        assert_eq!(doc.objects, objects);
    }
    for value in [
        Object::Null,
        (-1).into(),
        1_000_001.into(),
        Object::Real(0.0),
    ] {
        let (mut doc, page) = without_tree(UNTAGGED);
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("StructParents", value);
        assert!(textedit::scan(&doc, 0).is_err());
    }
    let (mut doc, page) = without_tree(UNTAGGED);
    doc.get_dictionary_mut(page).unwrap().set("StructParent", 0);
    assert!(textedit::scan(&doc, 0).is_err());
    let (mut doc, _) = fixture(UNTAGGED);
    assert!(
        textedit::scan(&doc, 0).is_err(),
        "a present tree must still be validated"
    );
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog)
        .unwrap()
        .set("StructTreeRoot", Object::Null);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_parent_tree_refusals_identify_unclaimed_annotation_entries() {
    for direct in [false, true] {
        for extra in [false, true] {
            let (mut doc, ids) = fixture(CONTENT);
            let scan = textedit::scan(&doc, 0).unwrap();
            let object: Object =
                dictionary! { "Type" => "StructElem", "S" => "Form", "P" => ids[2],
                "K" => dictionary! { "Type" => "OBJR", "Obj" => (9999, 0) } }
                .into();
            let object = if direct {
                object
            } else {
                doc.add_object(object).into()
            };
            let nums = if extra {
                vec![
                    0.into(),
                    Object::Array(vec![ids[3].into(), ids[4].into()]),
                    7.into(),
                    object,
                ]
            } else {
                vec![0.into(), object]
            };
            doc.get_dictionary_mut(ids[5]).unwrap().set("Nums", nums);
            let before = doc.objects.clone();
            // An annotation entry must be an indirect element, must not take
            // the page's own key, and must be claimed by an element's OBJR.
            let expected = match (direct, extra) {
                (true, _) => "unsupported or inconsistent tagged text structure",
                (false, false) => "tagged parent tree must contain one entry per page",
                (false, true) => "tagged parent tree has annotation entries no element claims",
            };
            assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected);
            let change = textedit::Change {
                layout: None,
                page: 0,
                revision: scan.revision,
                operator: scan.runs[0].operator,
                original: scan.runs[0].text.clone(),
                replacement: "IN".into(),
            };
            assert_eq!(textedit::write(&mut doc, &[change]).unwrap_err(), expected);
            assert_eq!(doc.objects, before);
        }
    }
    // A missing page entry, malformed pair, or bad reference must not be
    // diagnosed as an object entry merely because the lengths disagree.
    for entries in [
        vec![],
        vec![Object::Integer(0)],
        vec![
            0.into(),
            Object::Null,
            7.into(),
            Object::Reference((9999, 0)),
        ],
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[5]).unwrap().set("Nums", entries);
        assert_eq!(
            textedit::scan(&doc, 0).unwrap_err(),
            "tagged parent tree must contain one entry per page"
        );
    }
}

#[test]
fn textedit_tagged_refusals_identify_metadata_without_echoing_document_data() {
    for (index, key, expected) in [
        (
            1,
            "IDTree",
            "unsupported IDTree metadata in tagged structure root",
        ),
        (3, "IDTree", "unsupported IDTree metadata in tagged element"),
        (
            3,
            "ClassMap",
            "unsupported ClassMap metadata in tagged element",
        ),
        (3, "E", "unsupported E metadata in tagged element"),
        (
            3,
            "SYNTHETIC_SECRET\n",
            "unsupported unrecognized metadata in tagged element",
        ),
        (
            5,
            "Kids",
            "unsupported or inconsistent tagged text structure",
        ),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        let change = textedit::Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        doc.get_dictionary_mut(ids[index])
            .unwrap()
            .set(key, Object::string_literal("SYNTHETIC SECRET"));
        let objects = doc.objects.clone();
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected);
        assert_eq!(textedit::write(&mut doc, &[change]).unwrap_err(), expected);
        assert_eq!(doc.objects, objects, "refusal changed the document");
    }
    // Layout keys have their own context; arbitrary keys never get echoed.
    for key in ["BBox", "SYNTHETIC_SECRET"] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"A")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(key, Object::string_literal("SYNTHETIC SECRET"));
        let error = textedit::scan(&doc, 0).unwrap_err();
        assert_eq!(
            error,
            format!(
                "unsupported {} metadata in tagged layout attributes",
                if key == "BBox" {
                    "BBox"
                } else {
                    "unrecognized"
                }
            )
        );
    }
}

#[test]
fn textedit_tagged_refusals_separate_roles_parent_tree_and_ownership() {
    for case in 0..6 {
        let (mut doc, ids) = fixture(CONTENT);
        let expected = match case {
            0 => {
                doc.get_dictionary_mut(ids[1])
                    .unwrap()
                    .set("RoleMap", dictionary! { "Standard" => "SYNTHETIC_SECRET" });
                "tagged RoleMap contains unsupported or conflicting roles"
            }
            1 => {
                doc.get_dictionary_mut(ids[2])
                    .unwrap()
                    .set("S", "SYNTHETIC_SECRET");
                "tagged element role is not editable yet"
            }
            2 => {
                doc.get_dictionary_mut(ids[5])
                    .unwrap()
                    .set("Nums", Vec::<Object>::new());
                "tagged parent tree must contain one entry per page"
            }
            3 => {
                doc.get_dictionary_mut(ids[5])
                    .unwrap()
                    .set("Nums", vec![0.into(), Object::Array(vec![])]);
                "tagged parent content is empty or exceeds its limit"
            }
            4 => {
                doc.get_dictionary_mut(ids[3])
                    .unwrap()
                    .set("S", "SYNTHETIC_SECRET");
                "tagged element role is not editable yet"
            }
            _ => {
                doc.get_dictionary_mut(ids[5]).unwrap().set(
                    "Nums",
                    vec![0.into(), Object::Array(vec![ids[4].into(), ids[3].into()])],
                );
                "tagged content and parent tree disagree on ownership"
            }
        };
        assert_eq!(
            textedit::scan(&doc, 0).unwrap_err(),
            expected,
            "case {case}"
        );
    }
}

#[test]
fn textedit_tagged_refusals_distinguish_marked_content_context() {
    let source = std::str::from_utf8(CONTENT).unwrap();
    for (content, expected) in [
        (
            source.replace(
                "/Artifact BMC q EMC",
                "/Artifact BMC /Artifact BMC q EMC EMC",
            ),
            "nested marked content is not editable yet",
        ),
        (
            source.replace("/MCID 0", "/MCID 0 /ActualText (SYNTHETIC SECRET)"),
            "unsupported ActualText metadata in tagged marked-content properties",
        ),
        (
            source.replace("/Standard << /MCID 0", "/Artifact << /MCID 0"),
            "marked content repeats or disagrees with its structure tag",
        ),
        (
            source.replace("/MCID 1", "/MCID 9"),
            "marked content repeats or disagrees with its structure tag",
        ),
        (
            source.replace("/MCID 1", "/MCID 0"),
            "marked content repeats or disagrees with its structure tag",
        ),
        (
            source.replace("/Artifact BMC", "/SYNTHETIC_SECRET BMC"),
            "marked content without MCID must be an Artifact",
        ),
    ] {
        let (doc, _) = fixture(content.as_bytes());
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected);
    }
    // The owning element supplies the semantics, whatever the stream calls it.
    for tag in ["/SYNTHETIC_SECRET", "/Span", "/P", "/Content"] {
        let (doc, _) = fixture(
            source
                .replace("/Standard << /MCID 0", &format!("{tag} << /MCID 0"))
                .as_bytes(),
        );
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2, "{tag}");
    }
    let doc = textedit::tests::with_content(b"/SYNTHETIC_SECRET BMC EMC");
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "marked content has no supported structure tree"
    );
    assert_eq!(
        textedit::scan(&fixture(CONTENT).0, 0).unwrap().runs.len(),
        2
    );
}

// A marked-content sequence may name its language beside its MCID, as
// InDesign writes every paragraph; the edit is accepted and the saved stream
// keeps the language. A value that is not a language tag is still refused.
#[test]
fn textedit_marked_content_may_name_its_language() {
    let source = std::str::from_utf8(CONTENT).unwrap();
    let tagged = source.replace("/MCID 0", "/Lang (en-US) /MCID 0");
    let (mut doc, _) = fixture(tagged.as_bytes());
    let scan = textedit::scan(&doc, 0).unwrap();
    let first = scan.runs.iter().find(|run| run.text == "FIRST").unwrap();
    textedit::write(
        &mut doc,
        &[textedit::Change {
            layout: None,
            page: 0,
            revision: scan.revision.clone(),
            operator: first.operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.get_page_content(page);
    let written = String::from_utf8_lossy(&content);
    assert!(written.contains("/Lang (en-US)"), "{written}");
    assert!(written.contains("(IN)"), "{written}");
    for value in ["()", "(en--US)", "(-en)", "/en"] {
        let bad = source.replace("/MCID 0", &format!("/Lang {value} /MCID 0"));
        let (doc, _) = fixture(bad.as_bytes());
        assert_eq!(
            textedit::scan(&doc, 0).unwrap_err(),
            "unsupported or inconsistent tagged text structure",
            "{value}"
        );
    }
}
