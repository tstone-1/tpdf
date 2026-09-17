use super::list_tests::{change, list, refused};
use super::tests::{fixture, CONTENT};
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

// Word and Acrobat put a sublist inside the list body rather than beside it.
// The body keeps its own content; the sublist is a container and goes back to
// the walk, which owns the depth and container bounds.
fn inside_body() -> (Document, [ObjectId; 6], ObjectId, Vec<ObjectId>) {
    let (mut doc, ids, outer, owners) = list();
    let inner = doc.add_object(dictionary! { "S" => "L", "P" => owners[1], "K" => ids[4],
    "A" => dictionary! { "O" => "List", "ListNumbering" => "Decimal" } });
    doc.get_dictionary_mut(outer).unwrap().set("K", ids[3]);
    doc.get_dictionary_mut(ids[4]).unwrap().set("P", inner);
    doc.get_dictionary_mut(owners[1])
        .unwrap()
        .set("K", vec![Object::Integer(1), inner.into()]);
    (doc, ids, inner, owners)
}

#[test]
fn textedit_lists_nested_in_a_list_body_keep_every_level_editable() {
    for edited in [1, 3] {
        let (mut doc, ids, inner, owners) = inside_body();
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(
            before
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["1.", "FIRST", "2.", "SECOND"]
        );
        let objects = doc.objects.clone();
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
        // The label, both bodies, the sublist and its numbering all survive.
        for id in owners.iter().copied().chain([inner, ids[3], ids[4]]) {
            assert_eq!(doc.objects[&id], objects[&id], "{id:?}");
        }
    }
}

#[test]
fn textedit_list_bodies_refuse_children_that_are_not_a_nested_list() {
    // Control: the same fixture with a list in that position is accepted.
    assert!(textedit::scan(&inside_body().0, 0).is_ok());
    for role in ["P", "Table", "Div", "LI", "Lbl"] {
        let (mut doc, _, inner, _) = inside_body();
        doc.get_dictionary_mut(inner).unwrap().set("S", role);
        refused(doc);
    }
    // A sublist still has to say which body owns it.
    let (mut doc, ids, inner, _) = inside_body();
    doc.get_dictionary_mut(inner).unwrap().set("P", ids[2]);
    refused(doc);
}

// A list body may also hold an ordinary inline leaf, which `groups` validates
// itself. Only a sublist goes back to the walk, so this is what separates
// "defer a nested list" from "defer whatever a list body happens to hold".
#[test]
fn textedit_list_bodies_still_validate_their_own_inline_leaves() {
    let (mut doc, ids, _, owners) = list();
    let span = doc.add_object(
        dictionary! { "S" => "Span", "P" => owners[1], "Pg" => ids[0], "K" => vec![Object::Integer(1)] },
    );
    doc.get_dictionary_mut(owners[1])
        .unwrap()
        .set("K", vec![Object::Reference(span)]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![
                owners[0].into(),
                span.into(),
                owners[2].into(),
                owners[3].into(),
            ]),
        ],
    );
    let scan = textedit::scan(&doc, 0).unwrap();
    assert_eq!(
        scan.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["1.", "FIRST", "2.", "SECOND"]
    );
}

// The same reference in a paragraph is not the nested-list shape. Deferring it
// would hand the walk a container the paragraph may not own.
#[test]
fn textedit_paragraphs_refuse_a_list_among_their_content() {
    let (mut doc, ids) = fixture(CONTENT);
    let inner =
        doc.add_object(dictionary! { "S" => "L", "P" => ids[3], "K" => Vec::<Object>::new() });
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("K", vec![Object::Integer(0), inner.into()]);
    refused(doc);
}

// The deferred sublists are counted against the same frontier as every other
// pending child, before any of them is visited. Reaching that bound needs a
// frontier that is already loaded, which is why the document carries siblings
// the walk has not reached yet. The bound has its own message because the list
// branch below it shares the limit: while both said "list frontier", a test
// could sit on one of them believing it had covered the other.
#[test]
fn textedit_deferred_sublists_are_bounded_before_the_walk_visits_them() {
    const SIBLINGS: usize = super::MAX_NODES - 96;
    for (sublists, expected) in [
        (95, "unsupported or inconsistent tagged text structure"),
        (96, "tagged list frontier exceeds its limit"),
        (97, "tagged sublist frontier exceeds its limit"),
    ] {
        let (mut doc, ids, inner, owners) = inside_body();
        // One placeholder named many times. The walk refuses a second visit to
        // any of them, so none may be popped before the frontier is counted.
        let placeholder = doc.add_object(dictionary! { "S" => "", "P" => ids[2] });
        let mut children = vec![Object::Reference(
            doc.get_dictionary(ids[3])
                .unwrap()
                .get(b"P")
                .unwrap()
                .as_reference()
                .unwrap(),
        )];
        children.extend(vec![Object::Reference(placeholder); SIBLINGS]);
        doc.get_dictionary_mut(ids[2]).unwrap().set("K", children);
        // One sublist named many times, for the same reason.
        let mut body = vec![Object::Integer(1)];
        body.extend(vec![Object::Reference(inner); sublists]);
        doc.get_dictionary_mut(owners[1]).unwrap().set("K", body);
        assert_eq!(textedit::scan(&doc, 0).unwrap_err(), expected, "{sublists}");
    }
}
