use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

const CONTENT: &[u8] = b"/Artifact BMC q EMC /Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC Q";

fn multipage() -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = fixture(CONTENT);
    let page = doc.add_object(doc.objects[&ids[0]].clone());
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("StructParents", 7);
    let mut extra = Vec::new();
    for old in [ids[3], ids[4]] {
        let id = doc.add_object(doc.objects[&old].clone());
        doc.get_dictionary_mut(id).unwrap().set("Pg", page);
        extra.push(id);
    }
    let pages = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(pages).unwrap().set(
        "Kids",
        vec![Object::Reference(ids[0]), Object::Reference(page)],
    );
    doc.get_dictionary_mut(pages).unwrap().set("Count", 2);
    // Deliberately interleaved reading order, nonconsecutive page keys, reused
    // MCIDs, and a shared content stream. None of these may retarget an edit.
    doc.get_dictionary_mut(ids[2]).unwrap().set(
        "K",
        vec![
            Object::Reference(ids[3]),
            Object::Reference(extra[0]),
            Object::Reference(ids[4]),
            Object::Reference(extra[1]),
        ],
    );
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            Object::Integer(0),
            Object::Array(vec![ids[3].into(), ids[4].into()]),
            Object::Integer(7),
            Object::Array(extra.iter().copied().map(Object::Reference).collect()),
        ],
    );
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], page, extra[0], extra[1],
        ],
    )
}

fn flowing() -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = multipage();
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![Object::Reference(ids[3])]);
    doc.get_dictionary_mut(ids[3]).unwrap().set(
        "K",
        vec![
            Object::Integer(0),
            Object::Integer(1),
            dictionary! { "Type" => "MCR", "Pg" => ids[6], "MCID" => 0 }.into(),
            dictionary! { "Type" => "MCR", "Pg" => ids[6], "MCID" => 1 }.into(),
        ],
    );
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            Object::Integer(0),
            Object::Array(vec![ids[3].into(); 2]),
            Object::Integer(7),
            Object::Array(vec![ids[3].into(); 2]),
        ],
    );
    (doc, ids)
}

#[test]
fn textedit_tagged_end_indent_and_source_spaces_survive_a_fitting_edit() {
    for indent in [
        Object::Integer(-1_000_000),
        Object::Integer(0),
        Object::Real(1.6),
        Object::Real(1_000_000.0),
    ] {
        let content = std::str::from_utf8(CONTENT)
            .unwrap()
            .replace("(FIRST)", "(FIRST )");
        let (mut doc, ids) = fixture(content.as_bytes());
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"A")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("EndIndent", indent.clone());
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs[0].text, "FIRST ");
        let original = doc.objects.clone();
        let edit = Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST ".into(),
            replacement: "IN".into(),
        };
        let mut stale = edit.clone();
        stale.original.pop();
        assert!(textedit::write(&mut doc, &[stale]).is_err());
        assert_eq!(doc.objects, original);
        textedit::write(&mut doc, &[edit]).unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        for (id, object) in original {
            if id != ids[0] {
                assert_eq!(doc.objects[&id], object);
            }
        }
        assert_eq!(
            doc.get_dictionary(ids[3])
                .unwrap()
                .get(b"A")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"EndIndent")
                .unwrap(),
            &indent
        );
    }
}

#[test]
fn textedit_tagged_end_indent_rejects_invalid_values_and_document_scope() {
    for indent in [
        Object::Integer(1_000_001),
        Object::Integer(-1_000_001),
        Object::Real(f32::INFINITY),
        Object::Real(f32::NEG_INFINITY),
        Object::Real(f32::NAN),
        Object::Null,
        Object::Boolean(true),
        Object::string_literal("1.6"),
        Object::Array(vec![Object::Integer(1)]),
        Object::Reference((999, 0)),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"A")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("EndIndent", indent);
        assert!(textedit::scan(&doc, 0).is_err());
    }
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[2]).unwrap().set(
        "A",
        dictionary! { "O" => "Layout", "Placement" => "Block", "EndIndent" => 1 },
    );
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_tagged_flowing_paragraph_preserves_every_item_and_page() {
    for page in [0, 1] {
        let (mut doc, ids) = flowing();
        let before = textedit::scan(&doc, page).unwrap();
        let other = textedit::scan(&doc, 1 - page).unwrap();
        let original = doc.objects.clone();
        let edit = Change {
            page,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        textedit::write(&mut doc, &[edit]).unwrap();
        let after = textedit::scan(&doc, page).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        assert_eq!(textedit::scan(&doc, 1 - page).unwrap().runs, other.runs);
        for (id, object) in original {
            if id != ids[if page == 0 { 0 } else { 6 }] {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
    // Item order is reading order, not paint order. The element's page can be
    // either page; local integer IDs follow that page and MCRs name the other.
    let (mut doc, ids) = flowing();
    doc.get_dictionary_mut(ids[3]).unwrap().set("Pg", ids[6]);
    let items = doc
        .get_dictionary_mut(ids[3])
        .unwrap()
        .get_mut(b"K")
        .unwrap()
        .as_array_mut()
        .unwrap();
    for item in &mut items[2..] {
        item.as_dict_mut().unwrap().set("Pg", ids[0]);
    }
    items.reverse();
    for page in [0, 1] {
        assert_eq!(textedit::scan(&doc, page).unwrap().runs.len(), 2);
    }
}

#[test]
fn textedit_tagged_flowing_items_refuse_bad_ownership_without_mutation() {
    for mode in 0..15 {
        let (mut doc, ids) = flowing();
        let before = textedit::scan(&doc, 1).unwrap();
        let edit = Change {
            page: 1,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        let items = doc
            .get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"K")
            .unwrap()
            .as_array_mut()
            .unwrap();
        match mode {
            0 => items[2].as_dict_mut().unwrap().set("Stm", ids[0]),
            1 => items[2].as_dict_mut().unwrap().set("StmOwn", ids[0]),
            2 => items[2]
                .as_dict_mut()
                .unwrap()
                .set("ActualText", Object::string_literal("STALE")),
            3 => items[2].as_dict_mut().unwrap().set("Type", "OBJR"),
            4 => {
                items[2].as_dict_mut().unwrap().remove(b"Pg");
            }
            5 => items[2].as_dict_mut().unwrap().set("Pg", ids[1]),
            6 => items[2].as_dict_mut().unwrap().set("MCID", -1),
            7 => items[2]
                .as_dict_mut()
                .unwrap()
                .set("MCID", Object::Real(0.0)),
            8 => items[2].as_dict_mut().unwrap().set("Pg", ids[0]),
            9 => {
                items.pop();
            }
            10 => items.push(items[2].clone()),
            11 => items.clear(),
            12 => items[2] = Object::Reference(ids[3]),
            13 => items[2].as_dict_mut().unwrap().set("MCID", 128),
            14 => items[3] = items[2].clone(),
            _ => unreachable!(),
        }
        let original = doc.objects.clone();
        for page in [0, 1] {
            assert!(
                textedit::scan(&doc, page).is_err(),
                "mode {mode}, page {page}"
            );
        }
        assert!(textedit::write(&mut doc, &[edit]).is_err(), "mode {mode}");
        assert_eq!(doc.objects, original);
    }
    // Empty paragraph alongside fully claimed content is a distinct failure.
    let (mut doc, ids) = flowing();
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", Vec::<Object>::new());
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![ids[3].into(), ids[4].into()]);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_tagged_one_paragraph_still_bounds_total_content_items() {
    for count in [128, 129] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[2])
            .unwrap()
            .set("K", vec![Object::Reference(ids[3])]);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set("K", (0..count).map(Object::Integer).collect::<Vec<_>>());
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                Object::Integer(0),
                Object::Array(vec![ids[3].into(); count as usize]),
            ],
        );
        let content = (0..count)
            .map(|mcid| {
                format!(
                    "/Standard << /MCID {mcid} >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC "
                )
            })
            .collect::<String>();
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        let result = textedit::scan(&doc, 0);
        if count == 128 {
            assert_eq!(result.unwrap().runs.len(), 128);
        } else {
            assert!(result.is_err());
        }
    }
}

#[test]
fn textedit_tagged_empty_role_name_cannot_hide_duplicate_items() {
    let (mut doc, ids) = fixture(CONTENT);
    let mut roles = Dictionary::new();
    roles.set(Vec::<u8>::new(), Object::Name(b"P".to_vec()));
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("RoleMap", roles);
    for id in [ids[3], ids[4]] {
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("S", Object::Name(Vec::new()));
        doc.get_dictionary_mut(id)
            .unwrap()
            .set("K", vec![Object::Integer(0)]);
    }
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![Object::Integer(0), Object::Array(vec![ids[4].into(); 2])],
    );
    // Keep the first reverse pointer valid while combining two content items in
    // one element. An empty tag must not double as an unclaimed-slot sentinel.
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![Object::Reference(ids[4])]);
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", vec![Object::Integer(0); 2]);
    assert!(super::Tags::read(&doc, ids[0], &[ids[0]]).is_err());
}

#[test]
fn textedit_tagged_flowing_batch_is_atomic_across_pages() {
    let (mut doc, _) = flowing();
    let edits = [0, 1].map(|page| {
        let before = textedit::scan(&doc, page).unwrap();
        Change {
            page,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: if page == 0 { "IN".into() } else { "IS".into() },
        }
    });
    let original = doc.objects.clone();
    let mut bad = edits.clone();
    bad[1].replacement = "S".repeat(80);
    assert!(textedit::write(&mut doc, &bad).is_err());
    assert_eq!(doc.objects, original);
    textedit::write(&mut doc, &edits).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IS");
}

#[test]
fn textedit_tagged_pages_keep_local_ids_and_shared_streams_isolated() {
    for page in [0, 1] {
        let (mut doc, ids) = multipage();
        let before = textedit::scan(&doc, page).unwrap();
        let other = textedit::scan(&doc, 1 - page).unwrap();
        let objects = doc.objects.clone();
        let edit = Change {
            page,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        textedit::write(&mut doc, &[edit]).unwrap();
        assert_eq!(textedit::scan(&doc, page).unwrap().runs[0].text, "IN");
        assert_eq!(textedit::scan(&doc, 1 - page).unwrap().runs, other.runs);
        let changed_page = if page == 0 { ids[0] } else { ids[6] };
        for (id, object) in objects {
            if id != changed_page {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
    // Identical names and MCIDs alone cannot detect returning the first page's
    // map for every request. Give page two a different valid paragraph tag.
    let (mut doc, ids) = multipage();
    for id in [ids[7], ids[8]] {
        doc.get_dictionary_mut(id).unwrap().set("S", "P");
    }
    let content = std::str::from_utf8(CONTENT)
        .unwrap()
        .replace("/Standard", "/P");
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("Contents", stream);
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs.len(), 2);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
}

#[test]
fn textedit_tagged_multi_page_batches_validate_before_writing() {
    let (mut doc, ids) = multipage();
    let edits = [0, 1].map(|page| {
        let before = textedit::scan(&doc, page).unwrap();
        Change {
            page,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: if page == 0 { "IN".into() } else { "IS".into() },
        }
    });
    let original = doc.objects.clone();
    let mut bad = edits.clone();
    bad[1].replacement = "S".repeat(80);
    assert!(textedit::write(&mut doc, &bad).is_err());
    assert_eq!(doc.objects, original);
    textedit::write(&mut doc, &edits).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    assert_eq!(textedit::scan(&doc, 1).unwrap().runs[0].text, "IS");
    for id in [ids[1], ids[2], ids[3], ids[4], ids[5], ids[7], ids[8]] {
        assert_eq!(doc.objects[&id], original[&id]);
    }
}

#[test]
fn textedit_tagged_multi_page_parent_keys_and_owners_must_agree() {
    for mode in 0..11 {
        let (mut doc, ids) = multipage();
        match mode {
            0 => doc
                .get_dictionary_mut(ids[6])
                .unwrap()
                .set("StructParents", 0),
            1 => doc
                .get_dictionary_mut(ids[6])
                .unwrap()
                .set("StructParents", 9),
            2 => doc.get_dictionary_mut(ids[7]).unwrap().set("Pg", ids[0]),
            3 => doc
                .get_dictionary_mut(ids[7])
                .unwrap()
                .set("ActualText", Object::string_literal("STALE")),
            4 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    Object::Integer(7),
                    Object::Array(vec![ids[7].into(), ids[8].into()]),
                    Object::Integer(0),
                    Object::Array(vec![ids[3].into(), ids[4].into()]),
                ],
            ),
            5 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    Object::Integer(0),
                    Object::Array(vec![ids[3].into(), ids[4].into()]),
                ],
            ),
            6 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    Object::Integer(0),
                    Object::Array(vec![ids[3].into(), ids[4].into()]),
                    Object::Integer(7),
                    Object::Array(vec![ids[3].into(), ids[4].into()]),
                ],
            ),
            7 => doc
                .get_dictionary_mut(ids[6])
                .unwrap()
                .set("StructParent", 7),
            8 => doc
                .get_dictionary_mut(ids[7])
                .unwrap()
                .set("K", vec![Object::Integer(2)]),
            9 => {
                doc.get_dictionary_mut(ids[5])
                    .unwrap()
                    .get_mut(b"Nums")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(Object::Integer(99));
            }
            10 => {
                let nums = doc
                    .get_dictionary_mut(ids[5])
                    .unwrap()
                    .get_mut(b"Nums")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                nums[3]
                    .as_array_mut()
                    .unwrap()
                    .push(Object::Reference(ids[7]));
            }
            _ => unreachable!(),
        }
        for page in [0, 1] {
            assert!(
                textedit::scan(&doc, page).is_err(),
                "accepted mode {mode}, page {page}"
            );
        }
    }
    let (mut doc, ids) = multipage();
    doc.get_dictionary_mut(ids[2]).unwrap().set("Pg", ids[6]);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
}

fn fixture(content: &[u8]) -> (Document, [ObjectId; 6]) {
    let (mut doc, _, _, _) = textedit::fonts::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let pages = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    let pages = doc.get_dictionary_mut(pages).unwrap();
    pages.set("Kids", vec![Object::Reference(page)]);
    pages.set("Count", 1);
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
    let root = doc.new_object_id();
    let document = doc.new_object_id();
    let first = doc.new_object_id();
    let second = doc.new_object_id();
    let parents = doc.add_object(dictionary! { "Nums" => vec![Object::Integer(0), Object::Array(vec![first.into(), second.into()])] });
    doc.objects.insert(root, dictionary! { "Type" => "StructTreeRoot", "K" => vec![Object::Reference(document)], "ParentTree" => parents, "RoleMap" => dictionary! { "Standard" => "P" } }.into());
    doc.objects.insert(document, dictionary! { "Type" => "StructElem", "S" => "Document", "P" => root, "Pg" => page, "K" => vec![Object::Reference(first), Object::Reference(second)] }.into());
    for (id, mcid) in [(first, 0), (second, 1)] {
        doc.objects.insert(id, dictionary! { "Type" => "StructElem", "S" => "Standard", "P" => document, "Pg" => page, "K" => vec![Object::Integer(mcid)], "A" => dictionary! { "O" => "Layout", "Placement" => "Block" } }.into());
    }
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set("Contents", stream);
    dict.set("StructParents", 0);
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog)
        .unwrap()
        .set("StructTreeRoot", root);
    (doc, [page, root, document, first, second, parents])
}

#[test]
fn textedit_tagged_limits_have_valid_boundary_controls() {
    for count in [128, 129] {
        let (mut doc, ids) = fixture(CONTENT);
        let mut children = Vec::new();
        let mut content = String::new();
        for mcid in 0..count {
            let child = doc.add_object(dictionary! { "Type" => "StructElem", "S" => "P", "P" => ids[2], "Pg" => ids[0], "K" => vec![Object::Integer(mcid)] });
            children.push(Object::Reference(child));
            content.push_str(&format!(
                "/P << /MCID {mcid} >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC "
            ));
        }
        doc.get_dictionary_mut(ids[2])
            .unwrap()
            .set("K", children.clone());
        doc.get_dictionary_mut(ids[1]).unwrap().remove(b"RoleMap");
        doc.get_dictionary_mut(ids[5])
            .unwrap()
            .set("Nums", vec![Object::Integer(0), Object::Array(children)]);
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        doc.get_dictionary_mut(ids[0])
            .unwrap()
            .set("Contents", stream);
        let result = textedit::scan(&doc, 0);
        if count == 128 {
            assert_eq!(result.unwrap().runs.len(), 128);
        } else {
            assert!(result.is_err());
        }
    }
    let (mut doc, ids) = fixture(CONTENT);
    let second = doc.add_object(doc.objects[&ids[0]].clone());
    let pages = doc
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(pages).unwrap().set(
        "Kids",
        vec![Object::Reference(ids[0]), Object::Reference(second)],
    );
    doc.get_dictionary_mut(pages).unwrap().set("Count", 2);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_tagged_round_trip_preserves_structure_and_marked_content() {
    let (mut doc, ids) = fixture(CONTENT);
    let before = textedit::scan(&doc, 0).unwrap();
    let objects = doc.objects.clone();
    let edit = Change {
        page: 0,
        revision: before.revision,
        operator: before.runs[0].operator,
        original: "FIRST".into(),
        replacement: "IN".into(),
    };
    textedit::write(&mut doc, &[edit]).unwrap();
    let after = textedit::scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "IN");
    assert_eq!(after.runs[1], before.runs[1]);
    for (id, object) in objects {
        if id != ids[0] {
            assert_eq!(doc.objects[&id], object);
        }
    }
    let content = lopdf::content::Content::decode_strict(&doc.get_page_content(ids[0])).unwrap();
    let original = lopdf::content::Content::decode_strict(CONTENT).unwrap();
    assert_eq!(content.operations.len(), original.operations.len());
    for (a, b) in content.operations.iter().zip(original.operations) {
        if a.operator != "Tj" {
            assert_eq!(a.operator, b.operator);
            assert_eq!(a.operands, b.operands);
        }
    }
    // Structure order need not be drawing order; preserve the authored order.
    doc.get_dictionary_mut(ids[2]).unwrap().set(
        "K",
        vec![Object::Reference(ids[4]), Object::Reference(ids[3])],
    );
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
}

#[test]
fn textedit_tagged_refuses_semantic_overrides_and_stale_layout_attributes() {
    for index in [2, 3, 4] {
        for key in ["ActualText", "Alt", "E", "T", "C"] {
            let (mut doc, ids) = fixture(CONTENT);
            let before = textedit::scan(&doc, 0).unwrap();
            let edit = Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            doc.get_dictionary_mut(ids[index])
                .unwrap()
                .set(key, Object::string_literal("OLD TEXT"));
            let objects = doc.objects.clone();
            assert!(
                textedit::write(&mut doc, &[edit]).is_err(),
                "accepted {key} on {index}"
            );
            assert_eq!(doc.objects, objects);
        }
    }
    for key in ["BBox", "Width", "Height", "TextIndent"] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .get_mut(b"A")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(key, 1);
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {key}");
    }
}

#[test]
fn textedit_tagged_requires_both_parent_directions_and_bounded_unique_ids() {
    for mode in 0..14 {
        let (mut doc, ids) = fixture(CONTENT);
        match mode {
            0 => doc.get_dictionary_mut(ids[3]).unwrap().set("P", ids[1]),
            1 => doc.get_dictionary_mut(ids[3]).unwrap().set("Pg", ids[1]),
            2 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("K", vec![Object::Integer(1)]),
            3 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("K", vec![Object::Integer(-1)]),
            4 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("K", vec![Object::Integer(128)]),
            5 => doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    Object::Integer(0),
                    Object::Array(vec![ids[4].into(), ids[3].into()]),
                ],
            ),
            6 => doc
                .get_dictionary_mut(ids[0])
                .unwrap()
                .set("StructParents", 1),
            7 => doc
                .get_dictionary_mut(ids[2])
                .unwrap()
                .set("K", vec![Object::Reference(ids[3]); 129]),
            8 => doc
                .get_dictionary_mut(ids[2])
                .unwrap()
                .set("K", vec![Object::Reference(ids[3]); 2]),
            9 => doc
                .get_dictionary_mut(ids[2])
                .unwrap()
                .set("K", vec![Object::Reference(ids[2])]),
            10 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("K", vec![Object::Reference(ids[2])]),
            11 => doc
                .get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", dictionary! { "Standard" => "Figure" }),
            12 => doc
                .get_dictionary_mut(ids[5])
                .unwrap()
                .set("Kids", vec![Object::Reference(ids[5])]),
            13 => doc.get_dictionary_mut(ids[3]).unwrap().set("Type", "Other"),
            _ => unreachable!(),
        }
        assert!(textedit::scan(&doc, 0).is_err(), "accepted mode {mode}");
    }
}

#[test]
fn textedit_tagged_requires_balanced_unique_markers_and_paragraph_text() {
    let source = std::str::from_utf8(CONTENT).unwrap();
    for text in [
        source.replace(
            "/Artifact BMC q EMC",
            "/Artifact BMC q BT /F1 12 Tf 10 10 Td (FIRST) Tj ET EMC",
        ),
        format!("/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 210 Td (FIRST) Tj ET EMC {source}"),
        source.replace("/MCID 0", "/MCID 1"),
        source.replace("/MCID 0", "/MCID -1"),
        source.replace("/MCID 0", "/MCID 1000000"),
        source.replace("/MCID 0", "/MCID 0 /ActualText (OLD)"),
        source.replace("/Standard << /MCID 0 >> BDC", "/Artifact BMC"),
        source.replace("/Standard << /MCID 0 >> BDC", ""),
        source.replace("/Artifact BMC q EMC", "/Artifact BMC q"),
        source.replace("/Standard << /MCID 0 >> BDC", "/Other << /MCID 0 >> BDC"),
        source.replace(
            "/Standard << /MCID 0 >> BDC",
            "/Standard /NamedProperties BDC",
        ),
        source.replace("(FIRST) Tj", ""),
        source.replace("ET EMC Q", "ET Q"),
        source.replace("BT /F1", "BT EMC /F1"),
    ] {
        let (doc, _) = fixture(text.as_bytes());
        assert!(textedit::scan(&doc, 0).is_err(), "accepted {text}");
    }
}
