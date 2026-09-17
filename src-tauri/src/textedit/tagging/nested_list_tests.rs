use super::list_tests::{change, list, refused};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

fn nested() -> (Document, [ObjectId; 6], ObjectId, Vec<ObjectId>) {
    let (mut doc, ids, outer, owners) = list();
    let inner = doc.add_object(dictionary! { "S" => "L", "P" => ids[3], "K" => ids[4],
    "A" => dictionary! { "O" => "List", "ListNumbering" => "Decimal" } });
    doc.get_dictionary_mut(outer).unwrap().set("K", ids[3]);
    doc.get_dictionary_mut(ids[4]).unwrap().set("P", inner);
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .get_mut(b"K")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(inner.into());
    (doc, ids, inner, owners)
}

#[test]
fn textedit_nested_lists_keep_each_body_label_and_structure_object() {
    for edited in [1, 3] {
        let (mut doc, ids, _, _) = nested();
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(
            before
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>(),
            ["1.", "FIRST", "2.", "SECOND"]
        );
        let original = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[edited].operator,
                original: before.runs[edited].text.clone(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[edited].text, "IN");
        for index in 0..4 {
            if index != edited {
                assert_eq!(before.runs[index], after.runs[index]);
            }
        }
        for (id, object) in original {
            if id != ids[0] {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
}

// Every level owns a body, so the depth control remains a valid list tree.
fn deep(depth: usize) -> Document {
    let (mut doc, ids, _, _) = list();
    let mut parent = ids[2];
    let mut owners = Vec::new();
    let mut content = String::new();
    for index in 0..depth {
        let group = doc.add_object(dictionary! { "S" => "L", "P" => parent });
        if index == 0 {
            doc.get_dictionary_mut(parent).unwrap().set("K", group);
        } else {
            doc.get_dictionary_mut(parent)
                .unwrap()
                .get_mut(b"K")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(group.into());
        }
        let item = doc.add_object(dictionary! { "S" => "LI", "P" => group });
        let body = doc.add_object(
            dictionary! { "S" => "LBody", "P" => item, "Pg" => ids[0], "K" => index as i64 },
        );
        doc.get_dictionary_mut(group).unwrap().set("K", item);
        doc.get_dictionary_mut(item)
            .unwrap()
            .set("K", vec![Object::Reference(body)]);
        owners.push(Object::Reference(body));
        content.push_str(&format!(
            "/LBody <</MCID {index}>> BDC BT /F1 12 Tf 60 {} Td (FIRST) Tj ET EMC\n",
            180 - index * 12
        ));
        parent = item;
    }
    doc.get_dictionary_mut(ids[5])
        .unwrap()
        .set("Nums", vec![0.into(), Object::Array(owners)]);
    let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    doc
}

#[test]
fn textedit_nested_lists_share_the_grouping_depth_bound() {
    let mut doc = deep(8);
    let before = textedit::scan(&doc, 0).unwrap();
    assert_eq!(before.runs.len(), 8);
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[7].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[7].text, "IN");
    refused(deep(9));
}

#[test]
fn textedit_nested_lists_bound_pending_children_before_visiting_them() {
    for sibling in [false, true] {
        let (mut doc, ids, inner, owners) = nested();
        // Move the nested item's label to the parent item. Every MCID still
        // has one owner, and the parent now has four pending children.
        doc.get_dictionary_mut(owners[2]).unwrap().set("P", ids[3]);
        doc.get_dictionary_mut(ids[4]).unwrap().set("K", owners[3]);
        doc.get_dictionary_mut(ids[3]).unwrap().set(
            "K",
            vec![
                owners[0].into(),
                owners[1].into(),
                owners[2].into(),
                inner.into(),
            ],
        );
        if sibling {
            // Move one owned item into a valid root sibling, trading one
            // pending child for one pending sibling at the same total bound.
            let outer = doc
                .get_dictionary(ids[3])
                .unwrap()
                .get(b"P")
                .unwrap()
                .clone();
            doc.get_dictionary_mut(owners[0]).unwrap().set("S", "P");
            doc.get_dictionary_mut(owners[0]).unwrap().set("P", ids[2]);
            doc.get_dictionary_mut(ids[2])
                .unwrap()
                .set("K", vec![outer, owners[0].into()]);
            doc.get_dictionary_mut(ids[3])
                .unwrap()
                .get_mut(b"K")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .remove(0);
            let bytes = String::from_utf8(doc.get_page_content(ids[0]))
                .unwrap()
                .replacen("/Lbl", "/P", 1);
            let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), bytes.into_bytes()));
            doc.get_dictionary_mut(ids[0])
                .unwrap()
                .set("Contents", stream);
        }
        change(&doc);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"K")
            .unwrap()
            .as_array_mut()
            .unwrap()
            // With a sibling, the child list itself is exactly at the limit;
            // only counting the pending sibling must trigger this refusal.
            .extend(vec![Object::Null; super::MAX_NODES - 3]);
        assert!(textedit::scan(&doc, 0)
            .unwrap_err()
            .contains("list frontier"));
    }
}

#[test]
fn textedit_nested_lists_refuse_cycles_duplicate_owners_and_empty_items() {
    for mode in 0..9 {
        let (mut doc, ids, inner, owners) = nested();
        change(&doc);
        match mode {
            0 => {
                doc.get_dictionary_mut(inner).unwrap().set("P", ids[2]);
            }
            1 => {
                doc.get_dictionary_mut(inner).unwrap().set("K", ids[3]);
            }
            2 => {
                doc.get_dictionary_mut(ids[4])
                    .unwrap()
                    .get_mut(b"K")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(inner.into());
            }
            3 => {
                doc.get_dictionary_mut(ids[3])
                    .unwrap()
                    .get_mut(b"K")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(inner.into());
            }
            4 => {
                doc.get_dictionary_mut(ids[3])
                    .unwrap()
                    .set("K", vec![Object::Reference(inner)]);
            }
            5 => {
                doc.get_dictionary_mut(inner)
                    .unwrap()
                    .set("ActualText", Object::string_literal("STALE"));
            }
            6 => {
                doc.get_dictionary_mut(owners[3]).unwrap().remove(b"Pg");
                doc.get_dictionary_mut(inner).unwrap().set("Pg", ids[0]);
            }
            7 => {
                doc.get_dictionary_mut(ids[4])
                    .unwrap()
                    .set("K", vec![owners[0].into(), owners[3].into()]);
            }
            8 => {
                doc.get_dictionary_mut(inner).unwrap().set("S", "LBody");
            }
            _ => unreachable!(),
        }
        refused(doc);
    }
}

#[test]
fn textedit_nested_lists_keep_cross_page_content_ownership() {
    let (mut doc, ids, inner, owners) = nested();
    let page = doc.add_object(doc.objects[&ids[0]].clone());
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("StructParents", 7);
    let pages = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(pages)
        .unwrap()
        .set("Kids", vec![ids[0].into(), page.into()]);
    doc.get_dictionary_mut(pages).unwrap().set("Count", 2);
    let content = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
    let (first, rest) = content.split_once("/Lbl <</MCID 2>>").unwrap();
    let second = format!("/Lbl <</MCID 0>>{rest}").replace("/MCID 3", "/MCID 1");
    for (page, bytes) in [(ids[0], first.as_bytes()), (page, second.as_bytes())] {
        let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), bytes.to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
    }
    for (index, owner) in owners[2..].iter().enumerate() {
        doc.get_dictionary_mut(*owner).unwrap().set("Pg", page);
        doc.get_dictionary_mut(*owner)
            .unwrap()
            .set("K", index as i64);
    }
    // A list container's Pg cannot replace the explicit content-owner page.
    doc.get_dictionary_mut(inner).unwrap().set("Pg", ids[0]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(owners[..2].iter().copied().map(Object::Reference).collect()),
            7.into(),
            Object::Array(owners[2..].iter().copied().map(Object::Reference).collect()),
        ],
    );
    let first = textedit::scan(&doc, 0).unwrap();
    let before = textedit::scan(&doc, 1).unwrap();
    assert_eq!(before.runs[1].text, "SECOND");
    let original = doc.objects.clone();
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 1,
            revision: before.revision,
            operator: before.runs[1].operator,
            original: "SECOND".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[1].text, "IN");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, first.runs);
    for (id, object) in original {
        if id != page {
            assert_eq!(doc.objects[&id], object);
        }
    }
}
