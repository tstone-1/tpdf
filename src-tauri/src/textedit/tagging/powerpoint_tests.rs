// Shapes PowerPoint (through PDFMaker) writes on every slide: attribute arrays,
// figures under a custom role, figures inside figures.
use super::tests::{fixture, CONTENT};
use crate::textedit;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

fn layout() -> Dictionary {
    dictionary! { "O" => "Layout", "Placement" => "Block" }
}

fn texts(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .map(|run| run.text)
        .collect()
}

// ISO 32000-1 14.7.5: /A may hold several attribute objects; each is checked.
// Revision numbers stay refused.
#[test]
fn textedit_attribute_arrays_hold_each_object_to_the_same_rules() {
    let bad = dictionary! { "O" => "Layout", "Placement" => "Middle" };
    for (value, accepted) in [
        (Object::Array(vec![layout().into(), layout().into()]), true),
        (Object::Array(vec![layout().into()]), true),
        (Object::Array(vec![]), false),
        (Object::Array(vec![layout().into(); 9]), false),
        (Object::Array(vec![layout().into(), 0.into()]), false),
        (Object::Array(vec![layout().into(), bad.into()]), false),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[3]).unwrap().set("A", value);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted);
    }
}

// The second paragraph becomes a figure named through the RoleMap, drawing a
// rectangle; `text` puts the paragraph's text inside it instead.
fn figure(text: bool) -> (Document, [ObjectId; 6]) {
    let second = if text {
        "/Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC"
    } else {
        "/Standard << /MCID 1 >> BDC 40 130 20 20 re f EMC"
    };
    let content =
        format!("/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC {second}");
    let (mut doc, ids) = fixture(content.as_bytes());
    doc.get_dictionary_mut(ids[1]).unwrap().set(
        "RoleMap",
        dictionary! { "Standard" => "P", "Diagram" => "Figure" },
    );
    let element = doc.get_dictionary_mut(ids[4]).unwrap();
    element.set("S", "Diagram");
    element.set(
        "A",
        dictionary! { "O" => "Layout", "BBox" => vec![40.into(), 130.into(), 60.into(), 150.into()] },
    );
    (doc, ids)
}

// PowerPoint maps Diagram and Chart to Figure. The alias is read-only like
// any figure, its attributes are a figure's (BBox), and text under it is
// refused as figure text, because its group carries the mapped role.
#[test]
fn textedit_figure_aliases_are_figures() {
    let (doc, _) = figure(false);
    assert_eq!(texts(&doc), ["FIRST"]);
    let (doc, _) = figure(true);
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "text outside a tagged table cell is not editable"
    );
    // A paragraph alias still may not carry a figure's bounds.
    let (mut doc, ids) = figure(false);
    doc.get_dictionary_mut(ids[1]).unwrap().set(
        "RoleMap",
        dictionary! { "Standard" => "P", "Diagram" => "P" },
    );
    assert!(textedit::scan(&doc, 0).is_err());
}

// A grouped drawing: a figure whose pictures are figures of their own.
#[test]
fn textedit_figures_may_hold_figures() {
    let (mut doc, ids) = figure(false);
    let inner = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Figure", "P" => ids[4], "Pg" => ids[0],
        "K" => vec![Object::Integer(1)],
    });
    doc.get_dictionary_mut(ids[4])
        .unwrap()
        .set("K", vec![Object::Reference(inner)]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![0.into(), Object::Array(vec![ids[3].into(), inner.into()])],
    );
    assert_eq!(texts(&doc), ["FIRST"]);
    // Only a figure goes back to the walk; a paragraph inside one does not.
    doc.get_dictionary_mut(inner).unwrap().set("S", "P");
    assert!(textedit::scan(&doc, 0).is_err());
}

// ids: page, root, document, first cell, second cell, parents, table, rows.
// PowerPoint (Microsoft 365, measured on the EC consumer factsheet) wraps the
// paragraphs of every table cell in a `Textbox` that its RoleMap maps to Sect:
// TD -> Textbox -> P -> MCID. `change` edits the textbox before the scan.
fn textbox_cell(change: impl FnOnce(&mut Document, ObjectId, ObjectId)) -> Document {
    let (mut doc, ids) = super::table_tests::table();
    let paragraph = doc.new_object_id();
    let textbox = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Textbox", "P" => ids[3], "Pg" => ids[0],
        "K" => vec![Object::Reference(paragraph)],
    });
    doc.objects.insert(
        paragraph,
        dictionary! {
            "Type" => "StructElem", "S" => "P", "P" => textbox, "Pg" => ids[0],
            "K" => vec![Object::Integer(0)],
        }
        .into(),
    );
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("K", vec![Object::Reference(textbox)]);
    doc.get_dictionary_mut(ids[1]).unwrap().set(
        "RoleMap",
        dictionary! { "Standard" => "P", "Textbox" => "Sect" },
    );
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![paragraph.into(), ids[4].into(), ids[6].into()]),
        ],
    );
    change(&mut doc, textbox, paragraph);
    doc
}

#[test]
fn textedit_a_cell_text_box_is_lifted_into_its_cell() {
    let mut doc = textbox_cell(|_, _, _| {});
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    // The paragraph is the cell's: edited in place, with the tree unchanged.
    let runs = textedit::scan(&doc, 0).unwrap();
    let structure: Vec<_> = doc
        .objects
        .iter()
        .filter(|(_, object)| object.as_dict().is_ok_and(|d| d.has(b"S")))
        .map(|(id, object)| (*id, object.clone()))
        .collect();
    textedit::write(
        &mut doc,
        &[textedit::Change {
            layout: None,
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(texts(&doc), ["IN", "SECOND"]);
    for (id, object) in structure {
        assert_eq!(doc.objects[&id], object);
    }
    // Alternate text on the text box stands in for what it holds: pinned.
    let doc = textbox_cell(|doc, textbox, _| {
        doc.get_dictionary_mut(textbox)
            .unwrap()
            .set("Alt", Object::string_literal("cell"));
    });
    assert_eq!(texts(&doc), ["SECOND"]);
}

#[test]
fn textedit_a_cell_text_box_is_only_a_bare_wrapper() {
    type Change = fn(&mut Document, ObjectId, ObjectId);
    let cases: [(&str, Change); 6] = [
        // It is a grouping element: no layout attributes, which would be
        // about the box rather than the cell PowerPoint gives them to.
        ("attributes", |doc, textbox, _| {
            doc.get_dictionary_mut(textbox)
                .unwrap()
                .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" });
        }),
        ("empty", |doc, textbox, paragraph| {
            doc.get_dictionary_mut(textbox)
                .unwrap()
                .set("K", Vec::<Object>::new());
            doc.get_dictionary_mut(paragraph).unwrap().set("P", textbox);
        }),
        // A grouping element owns elements, never content of its own, even
        // content whose slot names the cell it would be lifted into.
        ("content", |doc, textbox, _| {
            let cell = doc
                .get_dictionary(textbox)
                .unwrap()
                .get(b"P")
                .unwrap()
                .clone();
            doc.get_dictionary_mut(textbox)
                .unwrap()
                .set("K", vec![Object::Integer(0)]);
            let root = doc
                .catalog()
                .unwrap()
                .get(b"StructTreeRoot")
                .unwrap()
                .as_reference()
                .unwrap();
            let parents = doc
                .get_dictionary(root)
                .unwrap()
                .get(b"ParentTree")
                .unwrap()
                .as_reference()
                .unwrap();
            let nums = doc
                .get_dictionary_mut(parents)
                .unwrap()
                .get_mut(b"Nums")
                .unwrap()
                .as_array_mut()
                .unwrap();
            nums[1].as_array_mut().unwrap()[0] = cell;
        }),
        // One level: a text box inside a lifted text box is not lifted.
        ("nested", |doc, textbox, paragraph| {
            let inner = doc.add_object(dictionary! {
                "Type" => "StructElem", "S" => "Textbox", "P" => textbox,
                "K" => vec![Object::Reference(paragraph)],
            });
            doc.get_dictionary_mut(paragraph).unwrap().set("P", inner);
            doc.get_dictionary_mut(textbox)
                .unwrap()
                .set("K", vec![Object::Reference(inner)]);
        }),
        // Only a grouping role is lifted; a text box mapped to a paragraph is
        // a paragraph in a paragraph.
        ("paragraph role", |doc, _, _| {
            let root = doc
                .catalog()
                .unwrap()
                .get(b"StructTreeRoot")
                .unwrap()
                .as_reference()
                .unwrap();
            doc.get_dictionary_mut(root).unwrap().set(
                "RoleMap",
                dictionary! { "Standard" => "P", "Textbox" => "P" },
            );
        }),
        // The paragraph must name the text box as its parent.
        ("parent", |doc, textbox, paragraph| {
            let cell = doc
                .get_dictionary(textbox)
                .unwrap()
                .get(b"P")
                .unwrap()
                .clone();
            doc.get_dictionary_mut(paragraph).unwrap().set("P", cell);
        }),
    ];
    for (case, change) in cases {
        let doc = textbox_cell(change);
        assert!(textedit::scan(&doc, 0).is_err(), "{case}");
    }
}

// ids: page, root, document, first, second, parents, link, span, annotation.
// PowerPoint tags a link's words as a Span inside the Link, and names the
// annotation through an OBJR that is itself an indirect object.
fn linked_span(subtype: &str, tag: &str) -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = fixture(CONTENT);
    let annotation = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => subtype, "P" => ids[0], "StructParent" => 1,
        "Rect" => vec![38.into(), 176.into(), 90.into(), 192.into()],
    });
    let link = doc.new_object_id();
    let span = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Span", "P" => link, "Pg" => ids[0],
        "K" => vec![Object::Integer(0)],
    });
    let objr =
        doc.add_object(dictionary! { "Type" => "OBJR", "Obj" => annotation, "Pg" => ids[0] });
    doc.objects.insert(
        link,
        dictionary! {
            "Type" => "StructElem", "S" => tag, "P" => ids[3], "Pg" => ids[0],
            "K" => vec![Object::Reference(span), Object::Reference(objr)],
        }
        .into(),
    );
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("K", vec![Object::Reference(link)]);
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Annots", vec![Object::Reference(annotation)]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![span.into(), ids[4].into()]),
            1.into(),
            link.into(),
        ],
    );
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], link, span, annotation,
        ],
    )
}

#[test]
fn textedit_a_links_words_in_a_span_stay_read_only() {
    let (doc, _) = linked_span("Link", "Link");
    // The link's words are kept and reserved, the paragraph beside is offered.
    assert_eq!(texts(&doc), ["SECOND"]);
    // The indirect OBJR is resolved and held to the ownership rules: an
    // annotation that names another parent-tree entry is refused.
    let (mut doc, ids) = linked_span("Link", "Link");
    doc.get_dictionary_mut(ids[8])
        .unwrap()
        .set("StructParent", 2);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_a_span_in_a_link_holds_only_its_words() {
    type Change = fn(&mut Document, [ObjectId; 9]);
    let cases: [(&str, Change); 4] = [
        // A second, empty Span beside the one with the words.
        ("empty", |doc, ids| {
            let empty = doc.add_object(dictionary! {
                "Type" => "StructElem", "S" => "Span", "P" => ids[6], "Pg" => ids[0],
            });
            doc.get_dictionary_mut(ids[6])
                .unwrap()
                .get_mut(b"K")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(empty.into());
        }),
        ("element", |doc, ids| {
            let inner = doc.add_object(dictionary! {
                "Type" => "StructElem", "S" => "Span", "P" => ids[7], "Pg" => ids[0], "K" => 0,
            });
            doc.get_dictionary_mut(ids[7])
                .unwrap()
                .set("K", vec![Object::Reference(inner)]);
        }),
        ("not a span", |doc, ids| {
            doc.get_dictionary_mut(ids[7]).unwrap().set("S", "P");
        }),
        // The slot names the Span that claims the words, not the Link.
        ("owner", |doc, ids| {
            doc.get_dictionary_mut(ids[5]).unwrap().set(
                "Nums",
                vec![
                    0.into(),
                    Object::Array(vec![ids[6].into(), ids[4].into()]),
                    1.into(),
                    ids[6].into(),
                ],
            );
        }),
    ];
    for (case, change) in cases {
        let (mut doc, ids) = linked_span("Link", "Link");
        change(&mut doc, ids);
        assert!(textedit::scan(&doc, 0).is_err(), "{case}");
    }
    // A form field owns its widget's text directly; PowerPoint's shape is a link's.
    let (doc, _) = linked_span("Widget", "Form");
    assert!(textedit::scan(&doc, 0).is_err());
}
