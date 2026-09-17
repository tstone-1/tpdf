use super::tests::{fixture, multipage, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId};

fn first(doc: &Document, page: u32) -> Change {
    let runs = textedit::scan(doc, page).unwrap();
    Change {
        layout: None,
        page,
        revision: runs.revision,
        operator: runs.runs[0].operator,
        original: runs.runs[0].text.clone(),
        replacement: "IN".into(),
    }
}

// Splits the two-page parent tree into two Limits-bearing leaves below a Kids
// root, the shape Acrobat writes for large documents. ids[5] becomes the root.
fn split(doc: &mut Document, ids: &[ObjectId; 9]) -> [ObjectId; 2] {
    let nums = doc
        .get_dictionary(ids[5])
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let left = doc.add_object(dictionary! {
        "Nums" => nums[0..2].to_vec(), "Limits" => vec![0.into(), 0.into()],
    });
    let right = doc.add_object(dictionary! {
        "Nums" => nums[2..4].to_vec(), "Limits" => vec![7.into(), 7.into()],
    });
    let root = doc.get_dictionary_mut(ids[5]).unwrap();
    root.remove(b"Nums");
    root.set(
        "Kids",
        vec![Object::Reference(left), Object::Reference(right)],
    );
    [left, right]
}

#[test]
fn textedit_parent_number_tree_kids_are_flattened_in_key_order() {
    let (mut doc, ids) = multipage();
    let flat = [
        textedit::scan(&doc, 0).unwrap(),
        textedit::scan(&doc, 1).unwrap(),
    ];
    let leaves = split(&mut doc, &ids);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, flat[0].runs);
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs, flat[1].runs);
    // An intermediate level with its own Limits spanning both leaves.
    let middle = doc.add_object(dictionary! {
        "Kids" => vec![Object::Reference(leaves[0]), Object::Reference(leaves[1])],
        "Limits" => vec![0.into(), 7.into()],
    });
    doc.get_dictionary_mut(ids[5])
        .unwrap()
        .set("Kids", vec![Object::Reference(middle)]);
    let objects = doc.objects.clone();
    {
        let changes = [first(&doc, 1)];
        textedit::write(&mut doc, &changes)
    }
    .unwrap();
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IN");
    for (id, object) in objects {
        if id != ids[6] {
            assert_eq!(doc.objects[&id], object);
        }
    }
}

#[test]
fn textedit_parent_number_tree_refuses_inconsistent_limits_order_and_cycles() {
    for case in 0..9 {
        let (mut doc, ids) = multipage();
        let change = first(&doc, 0);
        let leaves = split(&mut doc, &ids);
        match case {
            0 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("Limits", vec![0.into(), 1.into()]),
            1 => {
                doc.get_dictionary_mut(leaves[1]).unwrap().remove(b"Limits");
            }
            2 => doc
                .get_dictionary_mut(ids[5])
                .unwrap()
                .set("Limits", vec![0.into(), 7.into()]),
            // Leaves out of key order.
            3 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Kids",
                vec![Object::Reference(leaves[1]), Object::Reference(leaves[0])],
            ),
            // A leaf reached twice, and a node that is its own child.
            4 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Kids",
                vec![Object::Reference(leaves[0]), Object::Reference(leaves[0])],
            ),
            5 => doc
                .get_dictionary_mut(ids[5])
                .unwrap()
                .set("Kids", vec![Object::Reference(ids[5])]),
            6 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("Kids", vec![Object::Reference(leaves[1])]),
            7 => doc
                .get_dictionary_mut(ids[5])
                .unwrap()
                .set("Kids", Vec::<Object>::new()),
            _ => doc
                .get_dictionary_mut(leaves[1])
                .unwrap()
                .set("Type", "Other"),
        }
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "case {case}");
        assert!(textedit::write(&mut doc, &[change]).is_err(), "case {case}");
        assert_eq!(doc.objects, objects, "case {case}");
    }
    // Depth is bounded independently of the node count: leaves at depth 8
    // (seven single-child levels above the leaf pair) pass, depth 9 does not.
    for levels in [7, 8] {
        let (mut doc, ids) = multipage();
        let leaves = split(&mut doc, &ids);
        let mut below = vec![Object::Reference(leaves[0]), Object::Reference(leaves[1])];
        for _ in 0..levels {
            let node = doc.add_object(dictionary! {
                "Kids" => below, "Limits" => vec![0.into(), 7.into()],
            });
            below = vec![Object::Reference(node)];
        }
        doc.get_dictionary_mut(ids[5]).unwrap().set("Kids", below);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), levels == 7, "{levels}");
    }
}

#[test]
fn textedit_untagged_page_in_a_tagged_document_edits_as_plain_text() {
    let (mut doc, ids) = multipage();
    // Page two becomes an inserted untagged page: no StructParents, no
    // parent-tree entry, and no elements naming it.
    let untagged = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        b"BT /F1 12 Tf 40 180 Td (FIRST) Tj ET".to_vec(),
    ));
    let page = doc.get_dictionary_mut(ids[6]).unwrap();
    page.remove(b"StructParents");
    page.set("Contents", untagged);
    doc.get_dictionary_mut(ids[2]).unwrap().set(
        "K",
        vec![Object::Reference(ids[3]), Object::Reference(ids[4])],
    );
    let nums = doc
        .get_dictionary(ids[5])
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap()[0..2]
        .to_vec();
    doc.get_dictionary_mut(ids[5]).unwrap().set("Nums", nums);
    let objects = doc.objects.clone();
    {
        let changes = [first(&doc, 1), first(&doc, 0)];
        textedit::write(&mut doc, &changes)
    }
    .unwrap();
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IN");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    for id in [ids[1], ids[2], ids[3], ids[4], ids[5]] {
        assert_eq!(doc.objects[&id], objects[&id]);
    }

    // Marked content still needs a tree entry, and elements may not name the page.
    for case in 0..2 {
        let mut changed = doc.clone();
        match case {
            0 => {
                let stream = changed.add_object(lopdf::Stream::new(
                    lopdf::Dictionary::new(),
                    b"/P << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC".to_vec(),
                ));
                changed
                    .get_dictionary_mut(ids[6])
                    .unwrap()
                    .set("Contents", stream);
                assert_eq!(
                    textedit::scan(&changed, 1).unwrap_err(),
                    "marked content has no supported structure tree"
                );
            }
            _ => {
                changed
                    .get_dictionary_mut(ids[4])
                    .unwrap()
                    .set("Pg", ids[6]);
                assert!(textedit::scan(&changed, 0).is_err());
                assert!(textedit::scan(&changed, 1).is_err());
            }
        }
    }
}

#[test]
fn textedit_null_parent_tree_slots_are_unowned_and_cannot_be_used() {
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), Object::Null, ids[4].into()]),
        ],
    );
    let content = std::str::from_utf8(CONTENT)
        .unwrap()
        .replace("/MCID 1", "/MCID 2");
    let stream = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        content.clone().into_bytes(),
    ));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", vec![Object::Integer(2)]);
    {
        let changes = [first(&doc, 0)];
        textedit::write(&mut doc, &changes)
    }
    .unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");

    // A content sequence that uses the unowned slot is refused, not adopted.
    let used = content.replacen("/MCID 0", "/MCID 1", 1);
    let (mut doc, ids) = fixture(used.as_bytes());
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), Object::Null, ids[4].into()]),
        ],
    );
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", vec![Object::Integer(2)]);
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "marked content repeats or disagrees with its structure tag"
    );
    // An empty tag name cannot match the empty name of an unowned slot.
    let (mut doc, ids) = fixture(
        used.replacen("/Standard << /MCID 1", "/ << /MCID 1", 1)
            .as_bytes(),
    );
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), Object::Null, ids[4].into()]),
        ],
    );
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", vec![Object::Integer(2)]);
    assert!(textedit::scan(&doc, 0).is_err());
}
