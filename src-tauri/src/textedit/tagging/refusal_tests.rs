use super::tests::{fixture, CONTENT};
use crate::textedit;
use lopdf::{dictionary, Object};

#[test]
fn textedit_parent_tree_refusals_identify_non_page_entries() {
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
            let expected = "non-page parent-tree entries are not editable yet";
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
            1,
            "ClassMap",
            "unsupported ClassMap metadata in tagged structure root",
        ),
        (
            3,
            "ActualText",
            "unsupported ActualText metadata in tagged element",
        ),
        (
            3,
            "SYNTHETIC_SECRET\n",
            "unsupported unrecognized metadata in tagged element",
        ),
        (5, "Kids", "unsupported Kids metadata in tagged parent tree"),
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
                "tagged structure requires one Document root element"
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
            source.replace("/Standard << /MCID 0", "/SYNTHETIC_SECRET << /MCID 0"),
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
