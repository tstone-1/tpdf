use super::list_tests::refused;
use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

pub(super) fn table() -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = fixture(CONTENT);
    let table = doc.add_object(dictionary! { "S" => "Table", "P" => ids[2], "Pg" => ids[0] });
    let mut rows = Vec::new();
    for cell in [ids[3], ids[4]] {
        let row = doc.add_object(dictionary! { "S" => "TR", "P" => table, "K" => cell });
        let cell = doc.get_dictionary_mut(cell).unwrap();
        cell.set("S", "TD");
        cell.set("P", row);
        cell.set(
            "A",
            vec![
                Object::Dictionary(
                    dictionary! { "O" => "Table", "Headers" => Vec::<Object>::new() },
                ),
                Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1 }),
                Object::Dictionary(dictionary! { "O" => "Table", "ColSpan" => 1 }),
            ],
        );
        rows.push(row);
    }
    doc.get_dictionary_mut(table)
        .unwrap()
        .set("K", vec![2.into(), rows[0].into(), rows[1].into()]);
    doc.get_dictionary_mut(ids[2]).unwrap().set("K", table);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![ids[3].into(), ids[4].into(), table.into()]),
        ],
    );
    let content = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/TD");
    let content = format!("/Table <</MCID 2>> BDC 40 120 100 1 re f EMC {content}");
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], table, rows[0], rows[1],
        ],
    )
}

#[test]
fn textedit_tables_preserve_cells_borders_and_structure() {
    for target in 0..2 {
        for replacement in ["IN", ""] {
            let (mut doc, ids) = table();
            let runs = textedit::scan(&doc, 0).unwrap();
            assert_eq!(runs.runs.len(), 2);
            let edit = Change {
                layout: None,
                page: 0,
                revision: runs.revision,
                operator: runs.runs[target].operator,
                original: runs.runs[target].text.clone(),
                replacement: replacement.into(),
            };
            let before = doc.objects.clone();
            textedit::write(&mut doc, &[edit]).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs.len(), 2);
            assert_eq!(after.runs[target].text, replacement);
            let other = after
                .runs
                .iter()
                .find(|run| run.text == runs.runs[1 - target].text)
                .unwrap();
            assert_eq!(other.display_rect, runs.runs[1 - target].display_rect);
            for (id, object) in before {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], object);
                }
            }
            assert!(String::from_utf8(doc.get_page_content(ids[0]))
                .unwrap()
                .contains("40 120 100 1 re f"));
        }
    }
}

#[test]
fn textedit_tables_accept_bounded_cell_attribute_representations() {
    for representation in 0..5 {
        let (mut doc, ids) = table();
        let attrs =
            Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1, "ColSpan" => 1 });
        let attrs = match representation {
            0 => attrs,
            1 => Object::Array(vec![attrs]),
            2 => doc.add_object(attrs).into(),
            3 => Object::Array(vec![doc.add_object(attrs).into()]),
            _ => doc.add_object(Object::Array(vec![attrs])).into(),
        };
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", attrs);
        assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    }
    let (mut doc, ids) = table();
    doc.get_dictionary_mut(ids[3]).unwrap().remove(b"A");
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
}

#[test]
fn textedit_tables_refuse_spans_metadata_and_attribute_ambiguity() {
    for key in ["RowSpan", "ColSpan"] {
        for value in [
            0.into(),
            2.into(),
            (-1).into(),
            Object::Real(1.0),
            "1".into(),
            Object::Null,
        ] {
            let (mut doc, ids) = table();
            let mut attrs = dictionary! { "O" => "Table" };
            attrs.set(key, value);
            doc.get_dictionary_mut(ids[3]).unwrap().set("A", attrs);
            refused(doc);
        }
    }
    let attrs = Object::Dictionary(dictionary! { "O" => "Table", "RowSpan" => 1 });
    for value in [
        Object::Array(vec![]),
        Object::Array(vec![attrs.clone(); 4]),
        Object::Array(vec![attrs.clone(); 2]),
        Object::Array(vec![attrs, 0.into()]),
        Object::Array(
            vec![dictionary! { "O" => "Table", "Headers" => Vec::<Object>::new() }.into(); 2],
        ),
        dictionary! { "O" => "Table", "Headers" => Object::Null }.into(),
        dictionary! { "O" => "Layout", "RowSpan" => 1 }.into(),
        dictionary! { "O" => "Table", "Scope" => "Column" }.into(),
        dictionary! { "O" => "Table", "Headers" => vec![Object::string_literal("SYNTHETIC")] }
            .into(),
        dictionary! { "O" => "Table", "Width" => 100 }.into(),
        Object::Null,
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", value);
        refused(doc);
    }
    let (mut doc, ids) = table();
    let cyclic = doc.new_object_id();
    doc.objects.insert(cyclic, cyclic.into());
    doc.get_dictionary_mut(ids[3]).unwrap().set("A", cyclic);
    refused(doc);
}

#[test]
fn textedit_tables_refuse_invalid_hierarchy_and_container_text() {
    for (index, role) in [
        (6, "Div"),
        (7, "Div"),
        (3, "P"),
        (3, "THead"),
        (7, "Table"),
        (6, "TR"),
    ] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[index]).unwrap().set("S", role);
        refused(doc);
    }
    for index in [6, 7] {
        let (mut doc, ids) = table();
        doc.get_dictionary_mut(ids[index])
            .unwrap()
            .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
        refused(doc);
    }
    let (mut doc, ids) = table();
    let text = doc.get_page_content(ids[0]);
    let text = String::from_utf8(text)
        .unwrap()
        .replace("40 120 100 1 re f", "BT /F1 12 Tf 40 100 Td (FIRST) Tj ET");
    let stream = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    refused(doc);
}

fn without_border(doc: &mut Document, ids: &[ObjectId; 9]) {
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![Object::Reference(ids[7]), ids[8].into()]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![0.into(), Object::Array(vec![ids[3].into(), ids[4].into()])],
    );
    let content = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/TD");
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
}

#[test]
fn textedit_tables_require_rows_and_cells_in_their_own_containers() {
    let (mut doc, ids) = table();
    without_border(&mut doc, &ids);
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2);
    let mut orphan_rows = doc.clone();
    orphan_rows
        .get_dictionary_mut(ids[6])
        .unwrap()
        .set("S", "Div");
    refused(orphan_rows);
    let mut orphan_cells = doc.clone();
    orphan_cells
        .get_dictionary_mut(ids[7])
        .unwrap()
        .set("S", "Div");
    orphan_cells
        .get_dictionary_mut(ids[8])
        .unwrap()
        .set("S", "Div");
    orphan_cells
        .get_dictionary_mut(ids[6])
        .unwrap()
        .set("S", "Div");
    refused(orphan_cells);
    // Removing cell attributes leaves P otherwise supported: only the row's
    // child-role check can reject a paragraph disguised as a cell.
    for cell in [ids[3], ids[4]] {
        doc.get_dictionary_mut(cell).unwrap().set("S", "P");
        doc.get_dictionary_mut(cell).unwrap().remove(b"A");
    }
    // Keep the stream tags consistent with the replacement paragraph roles.
    let bytes = String::from_utf8(CONTENT.to_vec())
        .unwrap()
        .replace("/Standard", "/P");
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    refused(doc);
}
