use super::nested_tests::nested;
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

fn spans(multi: bool) -> (Document, Vec<ObjectId>, Vec<ObjectId>) {
    let (mut doc, ids, leaves) = nested(multi);
    for &id in &leaves {
        let leaf = doc.get_dictionary_mut(id).unwrap();
        leaf.set("S", "Span");
        leaf.set("Lang", Object::string_literal("de-DE"));
    }
    for page in crate::pagetree::ordered_pages(&doc) {
        let source = String::from_utf8(doc.get_page_content(page))
            .unwrap()
            .replace("/NonStruct <<", "/Span <<");
        let stream = doc.add_object(Stream::new(Dictionary::new(), source.into_bytes()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
    }
    (doc, ids, leaves)
}

#[test]
fn textedit_span_languages_and_structure_survive_edits_across_pages() {
    for multi in [false, true] {
        let (mut doc, _, leaves) = spans(multi);
        // Language is optional; retain the distinction rather than supplying
        // an inherited or default language when writing the second span.
        doc.get_dictionary_mut(leaves[1]).unwrap().remove(b"Lang");
        let pages = crate::pagetree::ordered_pages(&doc);
        let original = doc.objects.clone();
        for page in 0..pages.len() {
            let before = textedit::scan(&doc, page as u32).unwrap();
            assert_eq!(before.runs.len(), 2);
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: page as u32,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, page as u32).unwrap();
            assert_eq!(after.runs[0].text, "IN");
            assert_eq!(after.runs[0].matrix, before.runs[0].matrix);
            assert_eq!(after.runs[1], before.runs[1]);
        }
        for (id, value) in original {
            if !pages.contains(&id) {
                assert_eq!(doc.objects[&id], value);
            }
        }
    }
}

#[test]
fn textedit_spans_refuse_overrides_layout_nesting_and_wrong_owners_atomically() {
    for mode in 0..12 {
        let (mut doc, ids, leaves) = spans(false);
        let before = textedit::scan(&doc, 0).unwrap();
        let change = Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        match mode {
            // Alt, ActualText and T pin the leaf instead of refusing it
            // (pinned_tests); C needs a ClassMap, ID belongs to a table
            // header, and an unknown key is refused either way.
            0..=3 => doc.get_dictionary_mut(leaves[0]).unwrap().set(
                ["ID", "C", "E", "Unknown"][mode],
                Object::string_literal("STALE SYNTHETIC TEXT"),
            ),
            4 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" }),
            5 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("Lang", Object::string_literal("de--DE")),
            6 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("K", leaves[1]),
            7 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("K", leaves[0]),
            8 => doc.get_dictionary_mut(leaves[0]).unwrap().set("P", ids[2]),
            9 => doc.get_dictionary_mut(leaves[0]).unwrap().set("Pg", ids[1]),
            10 => doc
                .get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", dictionary! { "Span" => "P" }),
            _ => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    0.into(),
                    Object::Array(vec![ids[3].into(), leaves[1].into()]),
                ],
            ),
        }
        let original = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "case {mode}");
        assert!(textedit::write(&mut doc, &[change]).is_err(), "case {mode}");
        assert_eq!(doc.objects, original);
    }
}
