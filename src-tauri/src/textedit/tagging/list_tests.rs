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
    // An item itself carries no attributes; its label and body are laid out
    // like paragraphs (PowerPoint writes both), held to the same rules.
    let (mut doc, ids, _, _) = list();
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
    refused(doc);
    let bbox = || vec![Object::from(38), 176.into(), 50.into(), 192.into()];
    for (index, attributes, editable) in [
        (
            1,
            dictionary! { "O" => "Layout", "Placement" => "Block", "WritingMode" => "LrTb", "SpaceAfter" => 12 },
            Some(["1.", "FIRST", "2.", "SECOND"].as_slice()),
        ),
        (
            0,
            dictionary! { "O" => "Layout", "Placement" => "Inline" },
            Some(["1.", "FIRST", "2.", "SECOND"].as_slice()),
        ),
        // A label's recorded ink keeps its text where it is.
        (
            0,
            dictionary! { "O" => "Layout", "Placement" => "Inline", "BBox" => bbox() },
            Some(["FIRST", "2.", "SECOND"].as_slice()),
        ),
        (
            1,
            dictionary! { "O" => "Layout", "Placement" => "Inline" },
            None,
        ),
        (1, dictionary! { "O" => "Layout", "BBox" => bbox() }, None),
        (
            0,
            dictionary! { "O" => "Layout", "WritingMode" => "RlTb" },
            None,
        ),
    ] {
        let (mut doc, _, _, owners) = list();
        doc.get_dictionary_mut(owners[index])
            .unwrap()
            .set("A", attributes);
        match editable {
            Some(texts) => {
                let runs = textedit::scan(&doc, 0).unwrap();
                assert_eq!(
                    runs.runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<Vec<_>>(),
                    texts
                );
            }
            None => refused(doc),
        }
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
                    .set("E", Object::string_literal("STALE"));
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

/// [`list`] with other content: the first item's body is `body`, drawn inside
/// one text object that starts at x 60 on the label's line.
fn item(body: &str) -> Document {
    let (mut doc, ids, ..) = list();
    let content = format!(
        "/Lbl <</MCID 0>> BDC BT /F1 12 Tf 40 180 Td (1.) Tj ET EMC\n\
         /LBody <</MCID 1>> BDC BT /F1 12 Tf 60 180 Td {body} ET EMC\n\
         /Lbl <</MCID 2>> BDC BT /F1 12 Tf 40 100 Td (2.) Tj ET EMC\n\
         /LBody <</MCID 3>> BDC BT /F1 12 Tf 60 100 Td (SECOND) Tj ET EMC"
    );
    let stream = doc.add_object(Stream::new(lopdf::Dictionary::new(), content.into_bytes()));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    doc
}

/// Where each run with text starts after `text` has been given more words
/// than its line has room for, in the box the editor opens.
fn wrapped(doc: &Document, text: &str) -> Vec<(String, f64, f64)> {
    let mut doc = doc.clone();
    let page = textedit::scan(&doc, 0).unwrap();
    let run = page.runs.iter().find(|run| run.text == text).unwrap();
    textedit::write(
        &mut doc,
        &[Change {
            layout: Some(textedit::Layout::opened(run, textedit::EditFont::Auto)),
            page: 0,
            revision: page.revision.clone(),
            operator: run.operator,
            original: text.into(),
            replacement: format!("{text} AND MORE WORDS THAN THE LINE HAS ROOM FOR NOW"),
        }],
    )
    .unwrap();
    textedit::scan(&doc, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.trim().is_empty())
        .map(|run| (run.text.clone(), run.matrix[4], run.matrix[5]))
        .collect()
}

// An item is one block with its label, so the left edge of an item of one line
// is its label's. Its new line starts under its words all the same, as its
// producer would hang it: the first text after the label.
#[test]
fn textedit_a_one_line_list_item_wraps_under_its_words_not_its_label() {
    let runs = wrapped(&item("(FIRST) Tj"), "FIRST");
    let below: Vec<_> = runs
        .iter()
        .filter(|(_, _, y)| *y < 180. && *y > 100.)
        .collect();
    assert_eq!(below.len(), 1, "{runs:?}");
    assert!((below[0].1 - 60.).abs() < 0.001, "{runs:?}");
}

// An item of several lines says itself where its lines start, and that is
// where the new ones go: here at the label's own left edge, which is how a
// list without a hanging indent is set. The label decides nothing there.
#[test]
fn textedit_a_list_item_of_several_lines_wraps_where_its_own_lines_start() {
    const SECOND: &str = "AND A SECOND LINE THAT IS";
    let doc = item(&format!("(FIRST) Tj -20 -14 Td ({SECOND}) Tj"));
    let runs = wrapped(&doc, "FIRST");
    let new: Vec<_> = runs
        .iter()
        .filter(|(text, _, y)| *y < 180. && *y > 100. && text != SECOND)
        .collect();
    assert!(!new.is_empty(), "{runs:?}");
    for (text, x, _) in &new {
        assert!((x - 40.).abs() < 0.001, "{text} in {runs:?}");
    }
    // The item's own second line went down by the lines that were added.
    let second = runs.iter().find(|(text, ..)| text == SECOND).unwrap();
    assert!(
        (second.1 - 40.).abs() < 0.001
            && (second.2 - (166. - 14. * new.len() as f64)).abs() < 0.001,
        "{runs:?}"
    );
}
