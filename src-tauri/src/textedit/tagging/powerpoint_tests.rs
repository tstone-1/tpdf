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
