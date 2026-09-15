use super::tests::{fixture, multipage, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

fn replace_content(doc: &mut Document, page: ObjectId, bytes: Vec<u8>) {
    let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), bytes));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
}

// Wrap all existing children without changing their order or content owners.
fn wrap(doc: &mut Document, parent: ObjectId, tag: &str) -> ObjectId {
    let children = doc
        .get_dictionary(parent)
        .unwrap()
        .get(b"K")
        .unwrap()
        .clone();
    let items = match &children {
        Object::Array(items) => items.clone(),
        item => vec![item.clone()],
    };
    let group = doc.add_object(dictionary! { "S" => tag, "P" => parent, "K" => children });
    for child in items {
        doc.get_dictionary_mut(child.as_reference().unwrap())
            .unwrap()
            .set("P", group);
    }
    doc.get_dictionary_mut(parent).unwrap().set("K", group);
    group
}

fn edit(doc: &Document, page: u32) -> Change {
    let runs = textedit::scan(doc, page).unwrap();
    Change {
        page,
        revision: runs.revision,
        operator: runs.runs[0].operator,
        original: "FIRST".into(),
        replacement: "IN".into(),
    }
}

fn refused(mut doc: Document) {
    let before = doc.objects.clone();
    assert!(textedit::scan(&doc, 0).is_err());
    assert!(textedit::write(
        &mut doc,
        &[Change {
            page: 0,
            revision: vec![],
            operator: 0,
            original: "FIRST".into(),
            replacement: "IN".into()
        }]
    )
    .is_err());
    assert_eq!(doc.objects, before);
}

#[test]
fn textedit_containers_and_headings_preserve_roles_order_and_graph() {
    for grouping in ["Part", "Art", "Sect", "Div", "NonStruct"] {
        for block in ["P", "H", "H1", "H2", "H3", "H4", "H5", "H6"] {
            for aliases in [false, true] {
                // NonStruct is structural only; role aliases cannot target it.
                if grouping == "NonStruct" && aliases {
                    continue;
                }
                let (mut doc, ids) = fixture(CONTENT);
                let tag = if aliases { "HeadingStyle" } else { block };
                for id in [ids[3], ids[4]] {
                    doc.get_dictionary_mut(id).unwrap().set("S", tag);
                }
                replace_content(
                    &mut doc,
                    ids[0],
                    String::from_utf8(CONTENT.to_vec())
                        .unwrap()
                        .replace("/Standard <<", &format!("/{tag} <<"))
                        .into_bytes(),
                );
                let group = if aliases { "Story" } else { grouping };
                doc.get_dictionary_mut(ids[1]).unwrap().set(
                    "RoleMap",
                    if aliases {
                        dictionary! { "HeadingStyle" => block, "Story" => grouping }
                    } else {
                        lopdf::Dictionary::new()
                    },
                );
                wrap(&mut doc, ids[2], group);
                let original = doc.objects.clone();
                let change = edit(&doc, 0);
                textedit::write(&mut doc, &[change]).unwrap();
                let after = textedit::scan(&doc, 0).unwrap();
                assert_eq!(after.runs[0].text, "IN");
                assert_eq!(after.runs[1].text, "SECOND");
                for (id, object) in original {
                    if id != ids[0] {
                        assert_eq!(doc.objects[&id], object);
                    }
                }
            }
        }
    }
}

#[test]
fn textedit_containers_keep_interleaved_page_owners_and_nested_leaves() {
    let (mut doc, ids) = multipage();
    let before = [
        textedit::scan(&doc, 0).unwrap(),
        textedit::scan(&doc, 1).unwrap(),
    ];
    for tag in ["Sect", "Art", "Part", "Div"] {
        wrap(&mut doc, ids[2], tag);
    }
    // A grouping Pg can differ from the pages owned by its descendants.
    let outer = doc
        .get_dictionary(ids[2])
        .unwrap()
        .get(b"K")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(outer).unwrap().set("Pg", ids[6]);
    for page in 0..2 {
        assert_eq!(
            textedit::scan(&doc, page).unwrap().runs,
            before[page as usize].runs
        );
    }
    let original = doc.objects.clone();
    let change = edit(&doc, 1);
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, before[0].runs);
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IN");
    for (id, object) in original {
        if id != ids[6] {
            assert_eq!(doc.objects[&id], object);
        }
    }

    let (mut doc, ids) = fixture(CONTENT);
    let paragraph = doc.get_dictionary(ids[3]).unwrap().clone();
    let leaf = doc.add_object(dictionary! { "S" => "NonStruct", "P" => ids[3], "Pg" => ids[0], "K" => paragraph.get(b"K").unwrap().clone() });
    doc.get_dictionary_mut(ids[3]).unwrap().set("K", leaf);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![0.into(), Object::Array(vec![leaf.into(), ids[4].into()])],
    );
    replace_content(
        &mut doc,
        ids[0],
        String::from_utf8(CONTENT.to_vec())
            .unwrap()
            .replacen("/Standard <<", "/NonStruct <<", 1)
            .into_bytes(),
    );
    wrap(&mut doc, ids[2], "Sect");
    let change = edit(&doc, 0);
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
}

#[test]
fn textedit_container_depth_is_bounded_with_a_valid_boundary() {
    for depth in [8, 9] {
        let (mut doc, ids) = fixture(CONTENT);
        for _ in 0..depth {
            wrap(&mut doc, ids[2], "Sect");
        }
        if depth == 8 {
            let change = edit(&doc, 0);
            textedit::write(&mut doc, &[change]).unwrap();
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
        } else {
            refused(doc);
        }
    }
}

#[test]
fn textedit_container_count_is_bounded_independently_of_depth_and_content() {
    for extra in [false, true] {
        let (mut doc, ids) = fixture(CONTENT);
        let mut groups = Vec::new();
        let mut owners = Vec::new();
        let mut bytes = String::new();
        for mcid in 0..128 {
            let group = doc.add_object(dictionary! { "S" => "Sect", "P" => ids[2] });
            let block = doc
                .add_object(dictionary! { "S" => "P", "P" => group, "Pg" => ids[0], "K" => mcid });
            doc.get_dictionary_mut(group).unwrap().set("K", block);
            groups.push(Object::Reference(group));
            owners.push(Object::Reference(block));
            bytes.push_str(&format!(
                "/P << /MCID {mcid} >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC "
            ));
        }
        if extra {
            wrap(&mut doc, groups[0].as_reference().unwrap(), "Div");
        }
        doc.get_dictionary_mut(ids[2]).unwrap().set("K", groups);
        doc.get_dictionary_mut(ids[5])
            .unwrap()
            .set("Nums", vec![0.into(), Object::Array(owners)]);
        replace_content(&mut doc, ids[0], bytes.into_bytes());
        if extra {
            refused(doc);
        } else {
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 128);
        }
    }
}

#[test]
fn textedit_containers_refuse_cycles_aliases_attributes_and_wrong_owners_atomically() {
    for mode in 0..15 {
        let (mut doc, ids) = fixture(CONTENT);
        let group = wrap(&mut doc, ids[2], "Sect");
        assert!(textedit::scan(&doc, 0).is_ok());
        match mode {
            0 => doc.get_dictionary_mut(group).unwrap().set("K", group),
            1 => doc.get_dictionary_mut(group).unwrap().set("K", ids[2]),
            2 => doc
                .get_dictionary_mut(group)
                .unwrap()
                .set("K", vec![ids[3].into(), ids[3].into()]),
            3 => doc.get_dictionary_mut(ids[3]).unwrap().set("P", ids[2]),
            4 => doc.get_dictionary_mut(group).unwrap().set("P", ids[1]),
            5 => doc
                .get_dictionary_mut(group)
                .unwrap()
                .set("K", Vec::<Object>::new()),
            6 => doc.get_dictionary_mut(group).unwrap().set("K", 0),
            7 => doc.get_dictionary_mut(group).unwrap().set(
                "K",
                dictionary! { "Type" => "MCR", "Pg" => ids[0], "MCID" => 0 },
            ),
            8 => doc
                .get_dictionary_mut(group)
                .unwrap()
                .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" }),
            9 => doc
                .get_dictionary_mut(group)
                .unwrap()
                .set("ActualText", Object::string_literal("STALE")),
            10 => doc
                .get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", dictionary! { "Standard" => "P", "Div" => "P" }),
            11 => doc
                .get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", dictionary! { "Standard" => "P", "H1" => "P" }),
            12 => doc.get_dictionary_mut(group).unwrap().set("S", "Figure"),
            13 => {
                doc.get_dictionary_mut(group).unwrap().set("Pg", ids[0]);
                doc.get_dictionary_mut(ids[3]).unwrap().remove(b"Pg");
            }
            14 => doc.get_dictionary_mut(ids[1]).unwrap().set(
                "RoleMap",
                dictionary! { "Standard" => "Other", "Other" => "P" },
            ),
            _ => unreachable!(),
        }
        refused(doc);
    }
}

#[test]
fn textedit_container_frontier_is_bounded_before_children_are_visited() {
    for pending_sibling in [false, true] {
        let (mut doc, ids) = fixture(CONTENT);
        let group = wrap(&mut doc, ids[2], "Sect");
        if pending_sibling {
            doc.get_dictionary_mut(ids[2])
                .unwrap()
                .set("K", vec![group.into(), ids[4].into()]);
            doc.get_dictionary_mut(ids[4]).unwrap().set("P", ids[2]);
            doc.get_dictionary_mut(group)
                .unwrap()
                .set("K", vec![Object::Reference(ids[3])]);
        }
        assert!(textedit::scan(&doc, 0).is_ok());
        // One extra pending branch exceeds the two-item document budget.
        // It must be rejected before visiting this deliberately invalid child.
        doc.get_dictionary_mut(group)
            .unwrap()
            .get_mut(b"K")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(Object::Null);
        assert!(textedit::scan(&doc, 0)
            .unwrap_err()
            .contains("grouping frontier"));
    }
}
