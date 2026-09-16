use super::tests::{fixture, multipage, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId};

pub(super) fn nested(multi: bool) -> (Document, Vec<ObjectId>, Vec<ObjectId>) {
    let (mut doc, ids) = if multi {
        let (doc, ids) = multipage();
        (doc, ids.to_vec())
    } else {
        let (doc, ids) = fixture(CONTENT);
        (doc, ids.to_vec())
    };
    let paragraphs = if multi {
        vec![ids[3], ids[4], ids[7], ids[8]]
    } else {
        vec![ids[3], ids[4]]
    };
    let mut leaves = Vec::new();
    for &id in &paragraphs {
        let old = doc.get_dictionary(id).unwrap().clone();
        let items = old.get(b"K").unwrap().as_array().unwrap();
        let leaf = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "NonStruct", "P" => id, "Pg" => old.get(b"Pg").unwrap().clone(), "K" => items[0].clone() });
        let paragraph = doc.get_dictionary_mut(id).unwrap();
        paragraph.set("K", leaf);
        paragraph.remove(b"Pg");
        paragraph.remove(b"A");
        leaves.push(leaf);
    }
    for page in crate::pagetree::ordered_pages(&doc) {
        let content = doc.get_page_content(page);
        let text = String::from_utf8(content)
            .unwrap()
            .replace("/Standard <<", "/NonStruct <<");
        let stream = doc.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            text.into_bytes(),
        ));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
    }
    let document = doc.get_dictionary_mut(ids[2]).unwrap();
    document.remove(b"Pg");
    document.set("Lang", Object::string_literal("en"));
    let root = doc.get_dictionary_mut(ids[1]).unwrap();
    root.set("K", ids[2]);
    root.set("ParentTreeNextKey", if multi { 8 } else { 1 });
    let first = doc.add_object(Object::Array(vec![leaves[0].into(), leaves[1].into()]));
    let mut nums = vec![0.into(), first.into()];
    if multi {
        let second = doc.add_object(Object::Array(vec![leaves[2].into(), leaves[3].into()]));
        nums.extend([7.into(), second.into()]);
    }
    let parents = doc.get_dictionary_mut(ids[5]).unwrap();
    parents.set("Type", "ParentTree");
    parents.set("Nums", nums);
    (doc, ids, leaves)
}

fn update(doc: &Document, page: u32) -> Change {
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

fn refused(mut doc: Document) {
    let original = doc.objects.clone();
    assert!(textedit::scan(&doc, 0).is_err());
    assert!(textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: vec![],
            operator: 0,
            original: "FIRST".into(),
            replacement: "IN".into()
        }]
    )
    .is_err());
    assert_eq!(doc.objects, original);
}

#[test]
fn textedit_nested_browser_tree_roundtrips_pages_without_changing_structure() {
    for multi in [false, true] {
        let (mut doc, _, _) = nested(multi);
        let pages = crate::pagetree::ordered_pages(&doc);
        let before = doc.objects.clone();
        let edits: Vec<_> = (0..pages.len())
            .map(|page| update(&doc, page as u32))
            .collect();
        textedit::write(&mut doc, &edits).unwrap();
        for page in 0..pages.len() {
            let runs = textedit::scan(&doc, page as u32).unwrap();
            assert_eq!(runs.runs[0].text, "IN");
            assert_eq!(runs.runs[1].text, "SECOND");
        }
        for (id, object) in before {
            if !pages.contains(&id) {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
    let (mut doc, _, _) = nested(true);
    let other = textedit::scan(&doc, 1).unwrap();
    let edit = update(&doc, 0);
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 1).unwrap();
    assert_eq!(after.runs, other.runs);
    assert_eq!(after.revision, other.revision);
}

#[test]
fn textedit_nested_optional_container_pages_and_scalar_children_are_explicit() {
    for owner in [2, 3] {
        let (mut doc, ids, leaves) = nested(false);
        doc.get_dictionary_mut(ids[owner])
            .unwrap()
            .set("Pg", ids[0]);
        doc.get_dictionary_mut(leaves[0]).unwrap().remove(b"Pg");
        // Pg is not inherited from an ancestor structure element.
        refused(doc);
    }
    let (mut doc, ids, leaves) = nested(false);
    let child_array = doc.add_object(Object::Array(vec![leaves[0].into()]));
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("K", child_array);
    let root_array = doc.add_object(Object::Array(vec![ids[2].into()]));
    doc.get_dictionary_mut(ids[1]).unwrap().set("K", root_array);
    let item = dictionary! { "Type" => "MCR", "Pg" => ids[0], "MCID" => 0 };
    let leaf = doc.get_dictionary_mut(leaves[0]).unwrap();
    leaf.set("K", item);
    leaf.remove(b"Pg");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    let (mut doc, _, leaves) = nested(false);
    doc.get_dictionary_mut(leaves[0]).unwrap().remove(b"Pg");
    refused(doc);
}

#[test]
fn textedit_nested_ownership_cycles_and_extra_levels_are_refused_atomically() {
    for mode in 0..15 {
        let (mut doc, ids, leaves) = nested(false);
        match mode {
            0 => doc.get_dictionary_mut(leaves[0]).unwrap().set("P", ids[2]),
            1 => doc.get_dictionary_mut(leaves[0]).unwrap().set("Pg", ids[1]),
            2 => doc.get_dictionary_mut(leaves[0]).unwrap().set("K", 1),
            3 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("K", leaves[1]),
            4 => doc.get_dictionary_mut(ids[3]).unwrap().set("K", ids[2]),
            5 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("K", vec![leaves[0].into(), leaves[0].into()]),
            6 => doc.get_dictionary_mut(ids[4]).unwrap().set("K", leaves[0]),
            7 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("K", Vec::<Object>::new()),
            8 => doc.get_dictionary_mut(leaves[0]).unwrap().set("S", "Span"),
            9 => doc
                .get_dictionary_mut(leaves[0])
                .unwrap()
                .set("Type", "Other"),
            10 => doc.get_dictionary_mut(leaves[0]).unwrap().set("K", -1),
            11 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    0.into(),
                    Object::Array(vec![ids[3].into(), leaves[1].into()]),
                ],
            ),
            12 => {
                let alias = doc.add_object(Object::Integer(0));
                doc.get_dictionary_mut(leaves[0]).unwrap().set("K", alias);
            }
            13 => doc
                .get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", dictionary! { "NonStruct" => "P" }),
            14 => {
                // Both names agree, so the supported-role guard must refuse it.
                doc.get_dictionary_mut(leaves[0]).unwrap().set("S", "Quote");
                let source = String::from_utf8(doc.get_page_content(ids[0]))
                    .unwrap()
                    .replacen("/NonStruct <<", "/Quote <<", 1);
                let stream = doc.add_object(lopdf::Stream::new(
                    lopdf::Dictionary::new(),
                    source.into_bytes(),
                ));
                doc.get_dictionary_mut(ids[0])
                    .unwrap()
                    .set("Contents", stream);
            }
            _ => unreachable!(),
        }
        refused(doc);
    }
}

#[test]
fn textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text() {
    for index in 0..3 {
        for key in ["ActualText", "Alt", "E", "T", "C", "Unknown"] {
            let (mut doc, ids, leaves) = nested(false);
            let id = [ids[2], ids[3], leaves[0]][index];
            doc.get_dictionary_mut(id)
                .unwrap()
                .set(key, Object::string_literal("OLD"));
            refused(doc);
        }
    }
    for value in [
        Object::Null,
        Object::Name(b"en".to_vec()),
        Object::string_literal(""),
        Object::string_literal("en--GB"),
        Object::string_literal("-en"),
        Object::string_literal("en_XX"),
        Object::string_literal("en\n"),
        Object::string_literal("1en"),
        Object::string_literal(format!("abcdefgh-{}aaaaaaa", "aaaaaaa-".repeat(6))),
        Object::string_literal("abcdefghi"),
    ] {
        let (mut doc, ids, _) = nested(false);
        doc.get_dictionary_mut(ids[2]).unwrap().set("Lang", value);
        refused(doc);
    }
    for language in [
        "en",
        "en-GB",
        "i-default",
        "de-1996",
        &format!("{}aaaaaaa", "aaaaaaa-".repeat(7)),
    ] {
        let (mut doc, _, leaves) = nested(false);
        doc.get_dictionary_mut(leaves[0])
            .unwrap()
            .set("Lang", Object::string_literal(language));
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    }
    let (mut doc, _, leaves) = nested(false);
    doc.get_dictionary_mut(leaves[0])
        .unwrap()
        .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
    refused(doc);
    for next in [
        Object::Integer(-1),
        0.into(),
        1_000_002.into(),
        Object::Real(1.0),
        Object::string_literal("1"),
    ] {
        let (mut doc, ids, _) = nested(false);
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("ParentTreeNextKey", next);
        refused(doc);
    }
    let (mut doc, ids, _) = nested(false);
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("ParentTreeNextKey", 1_000_001);
    assert!(textedit::scan(&doc, 0).is_ok());
    let (mut doc, ids, _) = nested(false);
    doc.get_dictionary_mut(ids[5]).unwrap().set("Type", "Other");
    refused(doc);
}

#[test]
fn textedit_nested_mixed_leaf_ownership_keeps_each_authored_tag() {
    let (mut doc, ids, leaves) = nested(false);
    doc.get_dictionary_mut(ids[2]).unwrap().set("K", ids[3]);
    let paragraph = doc.get_dictionary_mut(ids[3]).unwrap();
    paragraph.set("Pg", ids[0]);
    paragraph.set("K", vec![Object::Integer(0), leaves[1].into()]);
    doc.get_dictionary_mut(leaves[1]).unwrap().set("P", ids[3]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), leaves[1].into()]),
        ],
    );
    let text = String::from_utf8(doc.get_page_content(ids[0]))
        .unwrap()
        .replacen("/NonStruct <<", "/Standard <<", 1);
    let stream = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        text.into_bytes(),
    ));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    let original = doc.objects.clone();
    let edit = update(&doc, 0);
    textedit::write(&mut doc, &[edit]).unwrap();
    for (id, value) in original {
        if id != ids[0] {
            assert_eq!(doc.objects[&id], value);
        }
    }
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[1].text, "SECOND");
}

#[test]
fn textedit_nested_total_content_limit_covers_all_leaf_groups() {
    for count in [128, 129] {
        let (mut doc, ids, _) = nested(false);
        let mut children = Vec::new();
        let mut content = String::new();
        for mcid in 0..count {
            let leaf = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "NonStruct", "P" => ids[3], "Pg" => ids[0], "K" => mcid });
            children.push(Object::Reference(leaf));
            content.push_str(&format!(
                "/NonStruct << /MCID {mcid} >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC "
            ));
        }
        doc.get_dictionary_mut(ids[2]).unwrap().set("K", ids[3]);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set("K", children.clone());
        doc.get_dictionary_mut(ids[5])
            .unwrap()
            .set("Nums", vec![0.into(), Object::Array(children)]);
        let stream = doc.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            content.into_bytes(),
        ));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), count == 128);
        if count == 128 {
            let edit = update(&doc, 0);
            textedit::write(&mut doc, &[edit]).unwrap();
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 128);
        }
    }
}

#[test]
fn textedit_nested_implicit_element_types_preserve_all_page_owners() {
    let (mut doc, ids, leaves) = nested(true);
    let before = [
        textedit::scan(&doc, 0).unwrap(),
        textedit::scan(&doc, 1).unwrap(),
    ];
    for id in [ids[2], ids[3], ids[4], ids[7], ids[8]]
        .iter()
        .chain(&leaves)
    {
        doc.get_dictionary_mut(*id).unwrap().remove(b"Type");
    }
    for page in 0..2 {
        assert_eq!(
            textedit::scan(&doc, page).unwrap().runs,
            before[page as usize].runs
        );
    }
    let objects = doc.objects.clone();
    let edit = update(&doc, 1);
    textedit::write(&mut doc, &[edit]).unwrap();
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IN");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs, before[0].runs);
    for (id, object) in objects {
        if id != ids[6] {
            assert_eq!(doc.objects[&id], object);
        }
    }
}
