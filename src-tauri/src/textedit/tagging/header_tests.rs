use super::{list_tests::refused, table_tests::table};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

fn key() -> Object {
    Object::string_literal("SYNTHETIC_HEADER")
}
fn limits() -> Object {
    Object::Array(vec![key(), key()])
}

fn header() -> (Document, [ObjectId; 9], ObjectId) {
    let (mut doc, ids) = table();
    let cell = doc.get_dictionary_mut(ids[3]).unwrap();
    cell.set("S", "TH");
    cell.set("ID", key());
    cell.set(
        "A",
        vec![
            Object::Dictionary(dictionary! { "O" => "Table", "Scope" => "Column" }),
            dictionary! { "O" => "Table", "RowSpan" => 1 }.into(),
            dictionary! { "O" => "Table", "ColSpan" => 1 }.into(),
        ],
    );
    doc.get_dictionary_mut(ids[4]).unwrap().set(
        "A",
        dictionary! { "O" => "Table", "Headers" => vec![key()] },
    );
    let leaf =
        doc.add_object(dictionary! { "Names" => vec![key(), ids[3].into()], "Limits" => limits() });
    let tree = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(leaf)] });
    doc.get_dictionary_mut(ids[1]).unwrap().set("IDTree", tree);
    let bytes = String::from_utf8(doc.get_page_content(ids[0]))
        .unwrap()
        .replacen("/TD", "/TH", 1);
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    (doc, ids, leaf)
}

#[test]
fn textedit_headers_preserve_identity_links_and_both_cells() {
    for scope in ["Row", "Column", "Both"] {
        for target in 0..2 {
            for replacement in ["IN", ""] {
                let (mut doc, ids, _) = header();
                doc.get_dictionary_mut(ids[3])
                    .unwrap()
                    .set("A", dictionary! { "O" => "Table", "Scope" => scope });
                let before = doc.objects.clone();
                let scan = textedit::scan(&doc, 0).unwrap();
                assert_eq!(scan.runs.len(), 2);
                let original = &scan.runs[target];
                textedit::write(
                    &mut doc,
                    &[Change {
                        layout: None,
                        page: 0,
                        revision: scan.revision,
                        operator: original.operator,
                        original: original.text.clone(),
                        replacement: replacement.into(),
                    }],
                )
                .unwrap();
                let after = textedit::scan(&doc, 0).unwrap();
                assert_eq!(after.runs[target].text, replacement);
                assert_eq!(after.runs[1 - target].text, scan.runs[1 - target].text);
                assert_eq!(
                    after.runs[1 - target].display_rect,
                    scan.runs[1 - target].display_rect
                );
                for (id, object) in before {
                    if id != ids[0] {
                        assert_eq!(doc.objects[&id], object);
                    }
                }
            }
        }
    }
}

#[test]
fn textedit_headers_validate_idtree_in_both_directions() {
    for case in 0..7 {
        let (mut doc, ids, leaf) = header();
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
        match case {
            0 => {
                doc.get_dictionary_mut(ids[1]).unwrap().remove(b"IDTree");
            }
            1 => {
                doc.get_dictionary_mut(ids[3]).unwrap().remove(b"ID");
            }
            2 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", vec![key(), ids[4].into()]),
            3 => doc
                .get_dictionary_mut(ids[3])
                .unwrap()
                .set("ID", Object::string_literal("OTHER_SYNTHETIC_ID")),
            4 => doc.get_dictionary_mut(ids[4]).unwrap().set("ID", key()),
            5 => {
                // Index entry with no identifier on the structure element.
                let extra = Object::string_literal("ZZ_SYNTHETIC");
                doc.get_dictionary_mut(leaf).unwrap().set(
                    "Names",
                    vec![key(), ids[3].into(), extra.clone(), ids[3].into()],
                );
                doc.get_dictionary_mut(leaf)
                    .unwrap()
                    .set("Limits", vec![key(), extra]);
            }
            _ => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", vec![key(), Object::Reference((99999, 0))]),
        }
        refused(doc);
    }
}

#[test]
fn textedit_headers_refuse_unknown_foreign_duplicate_and_recursive_links() {
    for values in [
        vec![Object::string_literal("UNKNOWN_SYNTHETIC")],
        vec![key(), key()],
        vec![Object::Null],
        vec![key(); 17],
    ] {
        let (mut doc, ids, _) = header();
        doc.get_dictionary_mut(ids[4])
            .unwrap()
            .set("A", dictionary! { "O" => "Table", "Headers" => values });
        refused(doc);
    }
    let (mut doc, ids, _) = header();
    let other = doc.add_object(dictionary! { "S" => "Table", "P" => ids[2], "K" => ids[8] });
    doc.get_dictionary_mut(ids[8]).unwrap().set("P", other);
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![Object::Integer(2), ids[7].into()]);
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![Object::Reference(ids[6]), other.into()]);
    // With no link, both tables are valid. Reinstating it must fail solely
    // because the target belongs to the other table, not because it is missing.
    let mut control = doc.clone();
    control.get_dictionary_mut(ids[4]).unwrap().remove(b"A");
    assert_eq!(textedit::scan(&control, 0).unwrap().runs.len(), 2);
    refused(doc);
    let (mut doc, ids, _) = header();
    doc.get_dictionary_mut(ids[3]).unwrap().set(
        "A",
        dictionary! { "O" => "Table", "Headers" => vec![key()] },
    );
    refused(doc);
}

#[test]
fn textedit_headers_bound_identifiers_scope_and_attributes() {
    for length in [127, 128] {
        let (mut doc, ids, leaf) = header();
        let key = Object::string_literal(vec![b'x'; length]);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set("ID", key.clone());
        doc.get_dictionary_mut(ids[4]).unwrap().set(
            "A",
            dictionary! { "O" => "Table", "Headers" => vec![key.clone()] },
        );
        doc.get_dictionary_mut(leaf)
            .unwrap()
            .set("Names", vec![key.clone(), ids[3].into()]);
        doc.get_dictionary_mut(leaf)
            .unwrap()
            .set("Limits", vec![key.clone(), key]);
        if length == 127 {
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
        } else {
            refused(doc);
        }
    }
    for value in [
        Object::Null,
        Object::string_literal(""),
        Object::string_literal(vec![b'x'; 128]),
    ] {
        let (mut doc, ids, _) = header();
        doc.get_dictionary_mut(ids[3]).unwrap().set("ID", value);
        refused(doc);
    }
    for value in [Object::Null, "Unknown".into(), 1.into()] {
        let (mut doc, ids, _) = header();
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set("A", dictionary! { "O" => "Table", "Scope" => value });
        refused(doc);
    }
    let (mut doc, ids, _) = header();
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("A", dictionary! { "O" => "Table", "Scope" => "Column" });
    refused(doc);
}

#[test]
fn textedit_headers_validate_name_tree_shape_order_limits_and_depth() {
    for direct in [false, true] {
        let (mut doc, ids, _) = header();
        let tree: Object = dictionary! { "Names" => vec![key(), ids[3].into()] }.into();
        let tree = if direct {
            tree
        } else {
            doc.add_object(tree).into()
        };
        doc.get_dictionary_mut(ids[1]).unwrap().set("IDTree", tree);
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    }
    for depth in [8, 9] {
        let (mut doc, ids, leaf) = header();
        let mut child = leaf;
        for _ in 1..depth {
            child = doc.add_object(
                dictionary! { "Kids" => vec![Object::Reference(child)], "Limits" => limits() },
            );
        }
        let root = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(child)] });
        doc.get_dictionary_mut(ids[1]).unwrap().set("IDTree", root);
        if depth == 8 {
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
        } else {
            refused(doc);
        }
    }
    for case in 0..8 {
        let (mut doc, ids, leaf) = header();
        match case {
            0 => {
                doc.get_dictionary_mut(leaf).unwrap().remove(b"Limits");
            }
            1 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Limits", vec![key(), Object::string_literal("WRONG")]),
            2 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", vec![key()]),
            3 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", vec![key(), ids[3].into(), key(), ids[3].into()]),
            4 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Kids", vec![Object::Reference(leaf)]),
            5 => {
                doc.get_dictionary_mut(leaf).unwrap().remove(b"Names");
                doc.get_dictionary_mut(leaf)
                    .unwrap()
                    .set("Kids", vec![Object::Reference(leaf)]);
            }
            6 => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", Vec::<Object>::new()),
            _ => doc
                .get_dictionary_mut(leaf)
                .unwrap()
                .set("Names", vec![key(), dictionary! { "S" => "TH" }.into()]),
        }
        refused(doc);
    }
}

#[test]
fn textedit_headers_link_across_table_sections_of_one_table() {
    let (mut doc, ids, _) = header();
    let head = doc.add_object(dictionary! { "S" => "THead", "P" => ids[6], "K" => ids[7] });
    let body = doc.add_object(dictionary! { "S" => "TBody", "P" => ids[6], "K" => ids[8] });
    doc.get_dictionary_mut(ids[7]).unwrap().set("P", head);
    doc.get_dictionary_mut(ids[8]).unwrap().set("P", body);
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![2.into(), head.into(), body.into()]);
    let before = doc.objects.clone();
    let scan = textedit::scan(&doc, 0).unwrap();
    assert_eq!(scan.runs.len(), 2);
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[1].operator,
            original: scan.runs[1].text.clone(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    for id in [ids[3], ids[4], ids[6], head, body] {
        assert_eq!(doc.objects[&id], before[&id]);
    }
}
