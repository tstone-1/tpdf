//! Shapes that Word, Acrobat PDFMaker and LiveCycle write by default, each with
//! the nearest malformed variant that must still be refused.
use super::table_tests::table;
use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

fn content(doc: &mut Document, page: ObjectId, bytes: String) {
    let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.into_bytes()));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
}

fn edit(doc: &Document) -> Change {
    let runs = textedit::scan(doc, 0).unwrap();
    Change {
        layout: None,
        page: 0,
        revision: runs.revision,
        operator: runs.runs[0].operator,
        original: runs.runs[0].text.clone(),
        replacement: "IN".into(),
    }
}

fn refused(mut doc: Document, change: &Change, case: &str) {
    let objects = doc.objects.clone();
    assert!(textedit::scan(&doc, 0).is_err(), "accepted {case}");
    assert!(
        textedit::write(&mut doc, std::slice::from_ref(change)).is_err(),
        "{case}"
    );
    assert_eq!(doc.objects, objects, "{case}");
}

fn texts(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .map(|run| run.text)
        .collect()
}

const HEADER: &str = "BT /F1 12 Tf 40 220 Td (FIRST) Tj ET";

#[test]
fn textedit_artifact_property_lists_keep_headers_read_only_inside_and_outside_text() {
    let source = std::str::from_utf8(CONTENT).unwrap();
    for (inside, properties) in [
        (
            false,
            "<< /Type /Pagination /Subtype /Header /Attached [/Top] /BBox [0 0 300.5 20] >>",
        ),
        (
            true,
            "<< /Type /Pagination /Subtype /Footer /Attached [/Bottom /Left] >>",
        ),
        (false, "<< /Type /Layout /Contents (SYNTHETIC) >>"),
        (false, "<< >>"),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        let header = if inside {
            format!("BT /Artifact {properties} BDC /F1 12 Tf 40 220 Td (FIRST) Tj EMC ET")
        } else {
            format!("/Artifact {properties} BDC {HEADER} EMC")
        };
        content(&mut doc, ids[0], format!("{header} {source}"));
        // The artifact's own text is preserved, never offered for editing.
        assert_eq!(texts(&doc), ["FIRST", "SECOND"], "{properties}");
        let objects = doc.objects.clone();
        let change = edit(&doc);
        textedit::write(&mut doc, &[change]).unwrap();
        assert_eq!(texts(&doc), ["IN", "SECOND"]);
        for id in [ids[1], ids[2], ids[3], ids[4], ids[5]] {
            assert_eq!(doc.objects[&id], objects[&id]);
        }
        let saved = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
        assert!(saved.contains("/Artifact"), "{properties}");
    }
    let (doc, _) = fixture(CONTENT);
    let change = edit(&doc);
    for properties in [
        "<< /Type /Unknown >>",
        "<< /Subtype /Title >>",
        "<< /Attached [/Middle] >>",
        "<< /Attached [/Top /Top /Top /Top /Top] >>",
        "<< /BBox [0 0 1] >>",
        "<< /BBox [0 0 1 (x)] >>",
        "<< /Contents /Name >>",
        "<< /SYNTHETIC_SECRET 1 >>",
        "/Named",
        "<< /MCID 0 >>",
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        content(
            &mut doc,
            ids[0],
            format!("/Artifact {properties} BDC {HEADER} EMC {source}"),
        );
        let error = textedit::scan(&doc, 0).unwrap_err();
        assert!(!error.contains("SYNTHETIC"), "{error}");
        refused(doc, &change, properties);
    }
}

#[test]
fn textedit_word_structure_without_k_and_with_table_sections_stays_editable() {
    // Word writes empty text boxes with no K at all.
    let (mut doc, ids) = fixture(CONTENT);
    let empty = doc.add_object(dictionary! { "S" => "Sect", "P" => ids[2] });
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![ids[3].into(), empty.into(), ids[4].into()]);
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();

    // Table -> TBody -> TR -> TD, as Word and Acrobat export tables.
    let build = || {
        let (mut doc, ids) = table();
        let body = doc.add_object(dictionary! {
            "S" => "TBody", "P" => ids[6], "K" => vec![ids[7].into(), ids[8].into()],
        });
        doc.get_dictionary_mut(ids[6])
            .unwrap()
            .set("K", vec![2.into(), body.into()]);
        for row in [ids[7], ids[8]] {
            doc.get_dictionary_mut(row).unwrap().set("P", body);
        }
        (doc, ids, body)
    };
    let (mut doc, ids, body) = build();
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    let objects = doc.objects.clone();
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    for id in [ids[6], ids[7], ids[8], body] {
        assert_eq!(doc.objects[&id], objects[&id]);
    }
    let change = edit(&build().0);
    for case in 0..4 {
        let (mut doc, ids, body) = build();
        match case {
            // A section outside a table.
            0 => {
                doc.get_dictionary_mut(body).unwrap().set("P", ids[2]);
                doc.get_dictionary_mut(ids[2])
                    .unwrap()
                    .set("K", vec![ids[6].into(), body.into()]);
                doc.get_dictionary_mut(ids[6])
                    .unwrap()
                    .set("K", vec![2.into()]);
            }
            // A cell directly inside a section.
            1 => {
                doc.get_dictionary_mut(body)
                    .unwrap()
                    .set("K", vec![ids[3].into(), ids[8].into()]);
                doc.get_dictionary_mut(ids[3]).unwrap().set("P", body);
            }
            // A section inside a row.
            2 => {
                let head = doc.add_object(dictionary! {
                    "S" => "THead", "P" => ids[7], "K" => Vec::<Object>::new(),
                });
                doc.get_dictionary_mut(ids[7])
                    .unwrap()
                    .set("K", vec![ids[3].into(), head.into()]);
            }
            // Sections carry no attributes.
            _ => doc
                .get_dictionary_mut(body)
                .unwrap()
                .set("A", dictionary! { "O" => "Layout", "Placement" => "Block" }),
        }
        refused(doc, &change, &format!("section case {case}"));
    }
}

#[test]
fn textedit_word_lists_nest_directly_and_cells_hold_lists_and_figures() {
    // L -> L -> LI -> LBody: Word's nested list, then P after it.
    let (mut doc, ids) = fixture(CONTENT);
    let outer = doc.add_object(dictionary! { "S" => "L", "P" => ids[2] });
    let inner = doc.add_object(dictionary! { "S" => "L", "P" => outer });
    let item = doc.add_object(dictionary! { "S" => "LI", "P" => inner, "K" => ids[3] });
    doc.get_dictionary_mut(outer).unwrap().set("K", inner);
    doc.get_dictionary_mut(inner).unwrap().set("K", item);
    let body = doc.get_dictionary_mut(ids[3]).unwrap();
    body.set("S", "LBody");
    body.set("P", item);
    body.remove(b"A");
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![outer.into(), ids[4].into()]);
    let source = std::str::from_utf8(CONTENT).unwrap().replacen(
        "/Standard << /MCID 0",
        "/LBody << /MCID 0",
        1,
    );
    content(&mut doc, ids[0], source.clone());
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    // A list may hold lists and items only.
    let mut broken = doc.clone();
    let stray =
        broken.add_object(dictionary! { "S" => "P", "P" => outer, "K" => Vec::<Object>::new() });
    broken
        .get_dictionary_mut(outer)
        .unwrap()
        .set("K", vec![inner.into(), stray.into()]);
    assert!(textedit::scan(&broken, 0).is_err());

    // A table cell that owns text, a figure and a list.
    let build = || {
        let (mut doc, ids) = table();
        let figure = doc.add_object(dictionary! {
            "S" => "Figure", "P" => ids[3], "Pg" => ids[0], "K" => 3,
            "A" => dictionary! { "O" => "Layout", "Placement" => "Block",
                "BBox" => vec![40.into(), 100.into(), 50.into(), 110.into()],
                "Width" => 10, "Height" => "Auto" },
        });
        let list = doc.add_object(dictionary! { "S" => "L", "P" => ids[4] });
        let item = doc.add_object(dictionary! { "S" => "LI", "P" => list });
        let body =
            doc.add_object(dictionary! { "S" => "LBody", "P" => item, "Pg" => ids[0], "K" => 4 });
        doc.get_dictionary_mut(list).unwrap().set("K", item);
        doc.get_dictionary_mut(item).unwrap().set("K", body);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set("K", vec![0.into(), figure.into()]);
        doc.get_dictionary_mut(ids[4])
            .unwrap()
            .set("K", vec![1.into(), list.into()]);
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![
                    ids[3].into(),
                    ids[4].into(),
                    ids[6].into(),
                    figure.into(),
                    body.into(),
                ]),
            ],
        );
        let source = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
        content(
            &mut doc,
            ids[0],
            format!(
                "{source} /Figure <</MCID 3>> BDC 40 100 10 10 re f EMC /LBody <</MCID 4>> BDC BT /F1 12 Tf 40 90 Td (FIRST) Tj ET EMC"
            ),
        );
        (doc, ids, figure)
    };
    let (mut doc, _, _) = build();
    assert_eq!(texts(&doc), ["FIRST", "SECOND", "FIRST"]);
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    let change = edit(&build().0);
    for (key, value) in [
        ("Width", Object::Integer(-1)),
        ("Height", Object::string_literal("10")),
        ("Width", Object::Name(b"Fixed".to_vec())),
    ] {
        let (mut doc, _, figure) = build();
        doc.get_dictionary_mut(figure)
            .unwrap()
            .get_mut(b"A")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(key, value);
        refused(doc, &change, key);
    }
    // Figure text inside a cell stays refused.
    let (mut doc, ids, _) = build();
    let source = String::from_utf8(doc.get_page_content(ids[0]))
        .unwrap()
        .replace("40 100 10 10 re f", "BT /F1 12 Tf 40 100 Td (FIRST) Tj ET");
    content(&mut doc, ids[0], source);
    refused(doc, &change, "figure text");
}

#[test]
fn textedit_orphaned_parent_tree_slots_stay_read_only_and_cannot_hide_reachable_owners() {
    let build_tagged = |slot: Option<Object>, tag: &str| {
        let (mut doc, ids) = fixture(CONTENT);
        // Acrobat's retagging leaves an element with no parent in the tree.
        let orphan = doc.add_object(dictionary! { "S" => tag, "K" => 2 });
        let slot = slot.unwrap_or(Object::Reference(orphan));
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![ids[3].into(), ids[4].into(), slot]),
            ],
        );
        let source = std::str::from_utf8(CONTENT).unwrap();
        content(
            &mut doc,
            ids[0],
            format!("/{tag} <</MCID 2>> BDC {HEADER} EMC {source}"),
        );
        (doc, ids, orphan)
    };
    let build = |slot: Option<Object>| build_tagged(slot, "Artifact");
    // A deleted paragraph keeps its own tag in the content. Its text stays
    // read-only, and its MCID is not counted against the owned content.
    let (mut doc, _, _) = build_tagged(None, "P");
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(texts(&doc), ["IN", "SECOND"]);
    let (mut doc, ids, orphan) = build(None);
    // The orphan's text is preserved; the owned paragraphs remain editable.
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    let objects = doc.objects.clone();
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(texts(&doc), ["IN", "SECOND"]);
    assert_eq!(doc.objects[&orphan], objects[&orphan]);
    assert_eq!(doc.objects[&ids[5]], objects[&ids[5]]);
    let saved = String::from_utf8(doc.get_page_content(ids[0])).unwrap();
    assert!(saved.starts_with("/Artifact <</MCID 2>> BDC BT /F1 12 Tf 40 220 Td (FIRST) Tj"));
    // An orphaned slot need not be used by the content at all.
    let (mut doc, ids, _) = build(None);
    content(
        &mut doc,
        ids[0],
        String::from_utf8(CONTENT.to_vec()).unwrap(),
    );
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);

    let change = edit(&build(None).0);
    for case in 0..3 {
        let (mut doc, ids, _) = build(None);
        let slot = match case {
            // A reachable element that does not claim the slot is inconsistent.
            0 => Object::Reference(ids[2]),
            // A slot must name a structure element dictionary.
            1 => doc
                .add_object(Stream::new(Dictionary::new(), Vec::new()))
                .into(),
            _ => Object::Integer(7),
        };
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![ids[3].into(), ids[4].into(), slot]),
            ],
        );
        refused(doc, &change, &format!("orphan case {case}"));
    }
}

#[test]
fn textedit_artifact_on_an_unowned_slot_is_read_only_and_owned_artifacts_are_refused() {
    let build = |slot: Object| {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![ids[3].into(), ids[4].into(), slot]),
            ],
        );
        let source = std::str::from_utf8(CONTENT).unwrap();
        content(
            &mut doc,
            ids[0],
            format!("/Artifact <</MCID 2>> BDC {HEADER} EMC {source}"),
        );
        (doc, ids)
    };
    // Acrobat marks retagged headers this way; the null slot owns nothing.
    let (mut doc, _) = build(Object::Null);
    assert_eq!(texts(&doc), ["FIRST", "SECOND"]);
    let change = edit(&doc);
    textedit::write(&mut doc, &[change]).unwrap();
    assert_eq!(texts(&doc), ["IN", "SECOND"]);
    let change = edit(&build(Object::Null).0);
    // An MCID beyond the page's slots, and a slot naming a reachable element
    // that does not claim it, stay refused. Owned artifacts: refusal_tests.
    let (mut doc, ids) = build(Object::Null);
    let source = std::str::from_utf8(CONTENT).unwrap();
    content(
        &mut doc,
        ids[0],
        format!("/Artifact <</MCID 3>> BDC {HEADER} EMC {source}"),
    );
    refused(doc, &change, "MCID beyond the slots");
    let (doc, ids) = build(Object::Null);
    let (doc, _) = {
        let mut doc = doc;
        doc.get_dictionary_mut(ids[5]).unwrap().set(
            "Nums",
            vec![
                0.into(),
                Object::Array(vec![ids[3].into(), ids[4].into(), ids[3].into()]),
            ],
        );
        (doc, ())
    };
    refused(
        doc,
        &change,
        "slot names a reachable element that does not claim it",
    );
}

#[test]
fn textedit_standard_namespaces_are_accepted_and_others_refused() {
    let build = |uri: &str, extra: Option<(&str, Object)>, tag: &str| {
        let (mut doc, ids) = fixture(CONTENT);
        let namespace = doc.add_object(dictionary! {
            "Type" => "Namespace", "NS" => Object::string_literal(uri),
        });
        if let Some((key, value)) = extra {
            doc.get_dictionary_mut(namespace).unwrap().set(key, value);
        }
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("Namespaces", vec![Object::Reference(namespace)]);
        let document = doc.get_dictionary_mut(ids[2]).unwrap();
        document.set("NS", namespace);
        document.set("S", tag);
        (doc, ids, namespace)
    };
    for uri in ["http://iso.org/pdf2/ssn", "http://iso.org/pdf/ssn"] {
        let (mut doc, ids, namespace) = build(uri, None, "Document");
        let objects = doc.objects.clone();
        let change = edit(&doc);
        textedit::write(&mut doc, &[change]).unwrap();
        for id in [ids[1], ids[2], namespace] {
            assert_eq!(doc.objects[&id], objects[&id]);
        }
    }
    let change = edit(&build("http://iso.org/pdf2/ssn", None, "Document").0);
    for (case, (uri, extra, tag)) in [
        ("http://example.com/SYNTHETIC", None, "Document"),
        (
            "http://iso.org/pdf2/ssn",
            Some(("RoleMapNS", Object::Dictionary(Dictionary::new()))),
            "Document",
        ),
        (
            "http://iso.org/pdf2/ssn",
            Some(("Schema", Object::Null)),
            "Document",
        ),
        (
            "http://iso.org/pdf2/ssn",
            Some(("Type", Object::Name(b"Other".to_vec()))),
            "Document",
        ),
        // A PDF 2.0 type whose meaning PDF 1.7 does not share.
        ("http://iso.org/pdf2/ssn", None, "DocumentFragment"),
    ]
    .into_iter()
    .enumerate()
    {
        let (doc, _, _) = build(uri, extra, tag);
        let error = textedit::scan(&doc, 0).unwrap_err();
        assert!(!error.contains("SYNTHETIC"), "{error}");
        refused(doc, &change, &format!("namespace case {case}"));
    }
    // An element may only name a namespace the root declares.
    let (mut doc, ids, _) = build("http://iso.org/pdf2/ssn", None, "Document");
    let other =
        doc.add_object(dictionary! { "NS" => Object::string_literal("http://iso.org/pdf2/ssn") });
    doc.get_dictionary_mut(ids[2]).unwrap().set("NS", other);
    refused(doc, &change, "undeclared namespace");
}
