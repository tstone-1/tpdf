use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

pub(super) fn list() -> (Document, [ObjectId; 6], ObjectId, Vec<ObjectId>) {
    let (mut doc, ids) = fixture(CONTENT);
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    doc.get_dictionary_mut(ids[0]).unwrap().set(
        "Resources",
        dictionary! { "Font" => dictionary! { "F1" => font } },
    );
    let list = doc.add_object(dictionary! { "S" => "L", "P" => ids[2],
    "K" => vec![ids[3].into(), ids[4].into()],
    "A" => vec![Object::Dictionary(dictionary! { "O" => "List", "ListNumbering" => "Decimal" })] });
    doc.get_dictionary_mut(ids[2]).unwrap().set("K", list);
    let mut owners = Vec::new();
    for (index, item) in [ids[3], ids[4]].into_iter().enumerate() {
        let label = doc.add_object(
            dictionary! { "S" => "Lbl", "P" => item, "Pg" => ids[0], "K" => (index * 2) as i64 },
        );
        let body = doc.add_object(dictionary! { "S" => "LBody", "P" => item, "Pg" => ids[0], "K" => (index * 2 + 1) as i64 });
        doc.objects.insert(
            item,
            dictionary! { "S" => "LI", "P" => list, "K" => vec![label.into(), body.into()] }.into(),
        );
        owners.extend([label, body]);
    }
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(owners.iter().copied().map(Object::Reference).collect()),
        ],
    );
    let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), b"/Lbl <</MCID 0>> BDC BT /F1 12 Tf 40 180 Td (1.) Tj ET EMC\n/LBody <</MCID 1>> BDC BT /F1 12 Tf 60 180 Td (FIRST) Tj ET EMC\n/Lbl <</MCID 2>> BDC BT /F1 12 Tf 40 140 Td (2.) Tj ET EMC\n/LBody <</MCID 3>> BDC BT /F1 12 Tf 60 140 Td (SECOND) Tj ET EMC".to_vec()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    (doc, ids, list, owners)
}

pub(super) fn change(doc: &Document) -> Change {
    let runs = textedit::scan(doc, 0).unwrap();
    assert_eq!(
        runs.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>(),
        ["1.", "FIRST", "2.", "SECOND"]
    );
    Change {
        layout: None,
        page: 0,
        revision: runs.revision,
        operator: runs.runs[1].operator,
        original: "FIRST".into(),
        replacement: "IN".into(),
    }
}

pub(super) fn refused(mut doc: Document) {
    let before = doc.objects.clone();
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
    assert_eq!(doc.objects, before);
}

#[test]
fn textedit_lists_preserve_labels_numbering_structure_and_other_items() {
    for numbering in [
        "None",
        "Disc",
        "Circle",
        "Square",
        "Decimal",
        "UpperRoman",
        "LowerRoman",
        "UpperAlpha",
        "LowerAlpha",
    ] {
        for representation in 0..4 {
            let (mut doc, ids, list, _) = list();
            let attributes =
                Object::Dictionary(dictionary! { "O" => "List", "ListNumbering" => numbering });
            let attributes = if representation % 2 == 0 {
                attributes
            } else {
                Object::Array(vec![attributes])
            };
            let attributes = if representation < 2 {
                attributes
            } else {
                Object::Reference(doc.add_object(attributes))
            };
            doc.get_dictionary_mut(list).unwrap().set("A", attributes);
            let edit = change(&doc);
            let before = doc.objects.clone();
            textedit::write(&mut doc, &[edit]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(
                after
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<Vec<_>>(),
                ["1.", "IN", "2.", "SECOND"]
            );
            for (id, object) in before {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], object);
                }
            }
        }
    }
    let (mut doc, _, list, _) = list();
    doc.get_dictionary_mut(list).unwrap().remove(b"A");
    change(&doc);
}

#[test]
fn textedit_lists_accept_browser_neutral_bodies_and_referenced_attributes() {
    let (mut doc, ids, list, owners) = list();
    for index in [1, 3] {
        doc.get_dictionary_mut(owners[index])
            .unwrap()
            .set("S", "NonStruct");
    }
    let bytes = String::from_utf8(doc.get_page_content(ids[0]))
        .unwrap()
        .replace("/LBody", "/NonStruct");
    let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), bytes.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    let attributes = doc.add_object(dictionary! { "O" => "List", "ListNumbering" => "Decimal" });
    doc.get_dictionary_mut(list)
        .unwrap()
        .set("A", vec![Object::Reference(attributes)]);
    let edit = change(&doc);
    textedit::write(&mut doc, &[edit]).unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[1].text, "IN");
}

#[test]
fn textedit_lists_refuse_ambiguous_numbering_and_stale_attributes() {
    let attrs = dictionary! { "O" => "List", "ListNumbering" => "Decimal" };
    let bad = vec![
        Object::Null, Object::Array(vec![]), Object::Array(vec![attrs.clone().into(), attrs.clone().into()]),
        Object::Array(vec![attrs.clone().into(), 0.into()]),
        dictionary! { "O" => "Layout", "ListNumbering" => "Decimal" }.into(),
        dictionary! { "O" => "List", "ListNumbering" => "Unknown" }.into(),
        dictionary! { "O" => "List", "ListNumbering" => Object::string_literal("Decimal") }.into(),
        dictionary! { "O" => "List" }.into(),
        dictionary! { "O" => "List", "ListNumbering" => "Decimal", "BBox" => vec![0.into(),0.into(),10.into(),10.into()] }.into(),
        dictionary! { "O" => "List", "ListNumbering" => "Decimal", "R" => 0 }.into(),
    ];
    for value in bad {
        let (mut doc, _, list, _) = list();
        change(&doc);
        doc.get_dictionary_mut(list).unwrap().set("A", value);
        refused(doc);
    }
    for index in 0..3 {
        let (mut doc, ids, _, owners) = list();
        doc.get_dictionary_mut([ids[3], owners[0], owners[1]][index])
            .unwrap()
            .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
        refused(doc);
    }
}

#[test]
fn textedit_lists_refuse_wrong_roles_owners_cycles_and_semantic_overrides() {
    for mode in 0..14 {
        let (mut doc, ids, list, owners) = list();
        change(&doc);
        match mode {
            0 => {
                doc.get_dictionary_mut(list).unwrap().set("K", ids[2]);
            }
            1 => {
                doc.get_dictionary_mut(list)
                    .unwrap()
                    .set("K", vec![ids[3].into(), ids[3].into()]);
            }
            2 => {
                doc.get_dictionary_mut(ids[3]).unwrap().set("P", ids[2]);
            }
            3 => {
                doc.get_dictionary_mut(owners[1]).unwrap().set("P", ids[4]);
            }
            4 => {
                doc.get_dictionary_mut(ids[2])
                    .unwrap()
                    .set("K", vec![ids[3].into(), ids[4].into()]);
                for id in [ids[3], ids[4]] {
                    doc.get_dictionary_mut(id).unwrap().set("P", ids[2]);
                }
            }
            5 => {
                // This P and its neutral leaves would otherwise be valid; the
                // immediate L-child rule must be the guard that rejects it.
                doc.get_dictionary_mut(ids[3]).unwrap().set("S", "P");
                for owner in &owners[..2] {
                    doc.get_dictionary_mut(*owner)
                        .unwrap()
                        .set("S", "NonStruct");
                }
                let bytes = String::from_utf8(doc.get_page_content(ids[0]))
                    .unwrap()
                    .replacen("/Lbl", "/NonStruct", 1)
                    .replacen("/LBody", "/NonStruct", 1);
                let stream =
                    doc.add_object(Stream::new(lopdf::Dictionary::new(), bytes.into_bytes()));
                doc.get_dictionary_mut(ids[0])
                    .unwrap()
                    .set("Contents", stream);
            }
            // A content tag is descriptive, so a role change alone must be
            // refused by the role rules: Quote is not an editable list child.
            6 => {
                doc.get_dictionary_mut(owners[1]).unwrap().set("S", "Quote");
            }
            7 => {
                doc.get_dictionary_mut(owners[1]).unwrap().remove(b"Pg");
                doc.get_dictionary_mut(ids[3]).unwrap().set("Pg", ids[0]);
            }
            8 => {
                doc.get_dictionary_mut(owners[1])
                    .unwrap()
                    .set("ActualText", Object::string_literal("STALE"));
            }
            9 => {
                doc.get_dictionary_mut(ids[1])
                    .unwrap()
                    .set("RoleMap", dictionary! { "LBody" => "P" });
            }
            10 => {
                doc.get_dictionary_mut(ids[1])
                    .unwrap()
                    .set("RoleMap", dictionary! { "L" => "Sect" });
            }
            11 => {
                doc.get_dictionary_mut(ids[1])
                    .unwrap()
                    .set("RoleMap", dictionary! { "CustomList" => "L" });
                // A valid unused mapping must not disable unrelated content.
                assert!(textedit::scan(&doc, 0).is_ok());
                continue;
            }
            12 => {
                doc.get_dictionary_mut(owners[1])
                    .unwrap()
                    .set("K", owners[1]);
            }
            13 => {
                doc.get_dictionary_mut(ids[3])
                    .unwrap()
                    .set("K", vec![owners[0].into(), owners[3].into()]);
            }
            _ => unreachable!(),
        }
        refused(doc);
    }
}
