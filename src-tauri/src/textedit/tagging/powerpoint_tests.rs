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

// ids: page, root, document, first, second, parents, first span, second span.
// PowerPoint (the EC consumer factsheet) sets ActualText on every Span of a
// paragraph, equal to the words it paints, or to them with the word's space at
// one end. `body` replaces the page's content; its MCIDs 0 and 1 are the Spans'.
fn spoken(first: &str, second: &str, body: Option<&str>) -> (Document, [ObjectId; 8]) {
    let (mut doc, ids) = fixture(
        body.unwrap_or(std::str::from_utf8(CONTENT).unwrap())
            .as_bytes(),
    );
    let mut spans = Vec::new();
    for (paragraph, mcid, text) in [(ids[3], 0, first), (ids[4], 1, second)] {
        let span = doc.add_object(dictionary! {
            "Type" => "StructElem", "S" => "Span", "P" => paragraph, "Pg" => ids[0],
            "K" => vec![Object::Integer(mcid)], "ActualText" => Object::string_literal(text),
        });
        doc.get_dictionary_mut(paragraph)
            .unwrap()
            .set("K", vec![Object::Reference(span)]);
        spans.push(span);
    }
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![spans[0].into(), spans[1].into()]),
        ],
    );
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], spans[0], spans[1],
        ],
    )
}

fn spoken_text(doc: &Document, span: ObjectId) -> String {
    let value = doc
        .get_dictionary(span)
        .unwrap()
        .get(b"ActualText")
        .unwrap();
    crate::textedit::actual::logical(value).unwrap()
}

fn replace(
    doc: &mut Document,
    original: &str,
    replacement: &str,
    layout: Option<textedit::Layout>,
) -> Result<(), String> {
    let runs = textedit::scan(doc, 0).unwrap();
    let run = runs.runs.iter().find(|run| run.text == original).unwrap();
    textedit::write(
        doc,
        &[textedit::Change {
            layout,
            page: 0,
            revision: runs.revision,
            operator: run.operator,
            original: original.into(),
            replacement: replacement.into(),
        }],
    )
}

#[test]
fn textedit_a_spans_actual_text_is_rewritten_with_its_words() {
    // Equal, and equal but for a trailing space only the ActualText has.
    let (mut doc, ids) = spoken("FIRST", "SECOND ", None);
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    let before = doc.objects.clone();
    replace(&mut doc, "FIRST", "IN", None).unwrap();
    assert_eq!(spoken_text(&doc, ids[6]), "IN");
    assert_eq!(spoken_text(&doc, ids[7]), "SECOND ");
    // Nothing else in the structure moved: only the page and the one Span.
    for (id, object) in &before {
        if *id != ids[0] && *id != ids[6] {
            assert_eq!(&doc.objects[id], object);
        }
    }
    // Reopened, the rewritten Span is offered again and edits again; the
    // other keeps the space its ActualText had over its words.
    assert_eq!(texts(&doc), ["IN", "SECOND"]);
    replace(&mut doc, "SECOND", "NOTED", None).unwrap();
    assert_eq!(spoken_text(&doc, ids[7]), "NOTED ");
    replace(&mut doc, "IN", "", None).unwrap();
    assert_eq!(spoken_text(&doc, ids[6]), "");
    // Deleted, the run is still offered, to be typed into again.
    assert_eq!(texts(&doc), ["", "NOTED"]);
    // The editor's own edits come with a layout: rewritten the same way.
    let (mut doc, ids) = spoken("FIRST", "SECOND", None);
    let layout = textedit::Layout {
        width: 60.,
        height: 16.,
        size: 12.,
        wrap: false,
        font: textedit::EditFont::Original,
        grow: false,
    };
    replace(&mut doc, "FIRST", "IN", Some(layout.clone())).unwrap();
    assert_eq!(spoken_text(&doc, ids[6]), "IN");
    // One line only: a replacement that wraps would need its ActualText to
    // say where the line broke.
    let (mut doc, _) = spoken("FIRST", "SECOND", None);
    let wrapped = textedit::Layout {
        width: 30.,
        height: 40.,
        wrap: true,
        ..layout
    };
    assert_eq!(
        replace(&mut doc, "FIRST", "IN IN", Some(wrapped)).unwrap_err(),
        "ActualText editing currently requires a single line"
    );
}

#[test]
fn textedit_a_span_whose_actual_text_is_not_its_words_stays_read_only() {
    for (first, why) in [
        ("FIRST!", "different text"),
        ("FIR ST", "a space inside"),
        ("  ", "only spaces"),
        ("", "nothing"),
    ] {
        let (doc, _) = spoken(first, "SECOND", None);
        assert_eq!(texts(&doc), ["SECOND"], "{why}");
    }
    // Alternate text, or a title, still pins the Span as before.
    for key in ["Alt", "T"] {
        let (mut doc, ids) = spoken("FIRST", "SECOND", None);
        doc.get_dictionary_mut(ids[6])
            .unwrap()
            .set(key, Object::string_literal("first"));
        assert_eq!(texts(&doc), ["SECOND"], "{key}");
    }
    // One sequence painting two runs: the ActualText covers both.
    let two = "/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 160 Td (DOCS) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC";
    for first in ["FIRST", "FIRST DOCS"] {
        let (doc, _) = spoken(first, "SECOND", Some(two));
        assert_eq!(texts(&doc), ["SECOND"], "{first}");
    }
    // One run, and beside it a show kept read-only (under a turned clip): the
    // ActualText covers that show's words too.
    let beside = "/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET q 110 170 m 128 167 l 125 149 l 107 152 l h W n BT /F1 12 Tf 110 155 Td (DOCS) Tj ET Q EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC";
    let (doc, _) = spoken("FIRST", "SECOND", Some(beside));
    assert_eq!(texts(&doc), ["SECOND"]);
    // An ActualText span of its own inside the Span's sequence.
    let inline = "/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td /Span << /ActualText (FIRST) >> BDC (FIRST) Tj EMC ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 40 140 Td (SECOND) Tj ET EMC";
    let (doc, _) = spoken("FIRST", "SECOND", Some(inline));
    assert_eq!(texts(&doc), ["SECOND"]);
    // The controls: without the Span's ActualText, those same pages offer FIRST.
    for body in [two, beside, inline] {
        let (mut doc, ids) = spoken("FIRST", "SECOND", Some(body));
        doc.get_dictionary_mut(ids[6])
            .unwrap()
            .remove(b"ActualText");
        assert!(texts(&doc).contains(&"FIRST".to_string()), "{body}");
    }
}

// A Span holding a sequence on each of two pages: the ActualText is the text of
// both, and neither page alone may rewrite it.
#[test]
fn textedit_a_span_across_two_pages_stays_read_only() {
    let (mut doc, ids) = super::tests::multipage();
    let span = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Span", "P" => ids[3], "Pg" => ids[0],
        "K" => vec![
            Object::Integer(0),
            dictionary! { "Type" => "MCR", "Pg" => ids[6], "MCID" => 0 }.into(),
        ],
        "ActualText" => Object::string_literal("FIRST"),
    });
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("K", vec![Object::Reference(span)]);
    // The second page's first paragraph now owns nothing; drop it.
    let kids = doc
        .get_dictionary_mut(ids[2])
        .unwrap()
        .get_mut(b"K")
        .unwrap()
        .as_array_mut()
        .unwrap();
    kids.retain(|kid| kid.as_reference().ok() != Some(ids[7]));
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![span.into(), ids[4].into()]),
            7.into(),
            Object::Array(vec![span.into(), ids[8].into()]),
        ],
    );
    assert_eq!(texts(&doc), ["SECOND"]);
    // The control: on one page only, the same Span is rewritable.
    doc.get_dictionary_mut(span)
        .unwrap()
        .set("K", vec![Object::Integer(0)]);
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![span.into(), ids[4].into()]),
            7.into(),
            Object::Array(vec![Object::Null, ids[8].into()]),
        ],
    );
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
}
