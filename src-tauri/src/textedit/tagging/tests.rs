use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

const CONTENT: &[u8] = b"/Artifact BMC q EMC /Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC Q";

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
