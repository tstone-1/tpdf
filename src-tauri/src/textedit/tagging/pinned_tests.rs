// Metadata that describes an element's content as it stands keeps that content
// read-only instead of refusing the page: alternate text, a title, and an
// alignment an edit would falsify. Classes from the root's ClassMap are held to
// the same rules as the element's own attributes, and a table of contents
// groups ordinary blocks.
use super::nested_tests::nested;
use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

fn offered(doc: &Document) -> Vec<String> {
    textedit::scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .map(|run| run.text.clone())
        .collect()
}

fn edit(doc: &mut Document, original: &str) -> Result<(), String> {
    let scan = textedit::scan(doc, 0)?;
    let operator = scan
        .runs
        .iter()
        .find(|run| run.text == original)
        .map_or(0, |run| run.operator);
    textedit::write(
        doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator,
            original: original.into(),
            replacement: "IN".into(),
        }],
    )
}

fn refused(mut doc: Document) {
    let original = doc.objects.clone();
    assert!(textedit::scan(&doc, 0).is_err());
    assert!(edit(&mut doc, "FIRST").is_err());
    assert_eq!(doc.objects, original);
}

fn layout(doc: &mut Document, id: ObjectId, entries: Dictionary) {
    doc.get_dictionary_mut(id).unwrap().set("A", entries);
}

#[test]
fn textedit_alternate_text_and_titles_pin_the_content_they_describe() {
    // Control: the fixture offers both paragraphs.
    let (doc, _) = fixture(CONTENT);
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    // On a paragraph, only that paragraph is kept; the other stays editable,
    // and the edit leaves the pinned element's metadata untouched.
    for key in ["Alt", "T"] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[4])
            .unwrap()
            .set(key, Object::string_literal("SECOND"));
        assert_eq!(offered(&doc), ["FIRST"], "{key}");
        let kept = doc.objects[&ids[4]].clone();
        edit(&mut doc, "FIRST").unwrap();
        assert_eq!(offered(&doc), ["IN"]);
        assert_eq!(doc.objects[&ids[4]], kept);
        // A change aimed at the pinned paragraph is refused atomically.
        let before = doc.objects.clone();
        assert!(edit(&mut doc, "SECOND").is_err());
        assert_eq!(doc.objects, before);
    }
    // Alt stands in for everything a grouping element holds, so it pins every
    // descendant. A grouping element's title names it and pins nothing.
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("Alt", Object::string_literal("SYNTHETIC"));
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "page contains only read-only text"
    );
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("T", Object::string_literal("SYNTHETIC"));
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    // An inline leaf pins only its own content.
    let (mut doc, _, leaves) = nested(false);
    doc.get_dictionary_mut(leaves[1])
        .unwrap()
        .set("Alt", Object::string_literal("SECOND"));
    assert_eq!(offered(&doc), ["FIRST"]);
    // Both are text strings with bounds.
    for (key, value) in [
        ("Alt", Object::Name(b"SECOND".to_vec())),
        ("T", Object::Integer(1)),
        ("T", Object::string_literal("x".repeat(4097))),
        ("Alt", Object::string_literal("x".repeat(65537))),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[4]).unwrap().set(key, value);
        refused(doc);
    }
}

#[test]
fn textedit_alignment_pins_a_block_and_line_height_is_kept() {
    for (align, offered_texts) in [
        ("Start", vec!["FIRST", "SECOND"]),
        ("Center", vec!["SECOND"]),
        ("End", vec!["SECOND"]),
        ("Justify", vec!["SECOND"]),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        layout(
            &mut doc,
            ids[3],
            dictionary! { "O" => "Layout", "Placement" => "Block", "TextAlign" => align },
        );
        assert_eq!(offered(&doc), offered_texts, "{align}");
    }
    // Placement may be omitted, and LineHeight is kept whatever the edit.
    for entries in [
        dictionary! { "O" => "Layout" },
        dictionary! { "O" => "Layout", "LineHeight" => 12 },
        dictionary! { "O" => "Layout", "LineHeight" => 0.0 },
        dictionary! { "O" => "Layout", "LineHeight" => "Normal" },
        dictionary! { "O" => "Layout", "LineHeight" => "Auto", "StartIndent" => 3 },
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        layout(&mut doc, ids[3], entries.clone());
        assert_eq!(offered(&doc), ["FIRST", "SECOND"], "{entries:?}");
        edit(&mut doc, "FIRST").unwrap();
        assert_eq!(
            doc.get_dictionary(ids[3]).unwrap().get(b"A").unwrap(),
            &Object::Dictionary(entries)
        );
    }
    for entries in [
        dictionary! { "O" => "Layout", "TextAlign" => "Middle" },
        dictionary! { "O" => "Layout", "TextAlign" => 1 },
        dictionary! { "O" => "Layout", "LineHeight" => -1 },
        dictionary! { "O" => "Layout", "LineHeight" => "Tall" },
        dictionary! { "O" => "Layout", "Placement" => "Inline" },
        dictionary! { "Placement" => "Block" },
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        layout(&mut doc, ids[3], entries);
        refused(doc);
    }
}

fn classed(classes: Dictionary, class: Object) -> (Document, [ObjectId; 6]) {
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("ClassMap", classes);
    doc.get_dictionary_mut(ids[3]).unwrap().set("C", class);
    (doc, ids)
}

fn classes() -> Dictionary {
    dictionary! {
        "Indent" => dictionary! { "O" => "Layout", "StartIndent" => 2, "LineHeight" => 12 },
        "Centred" => dictionary! { "O" => "Layout", "TextAlign" => "Center" },
        "Pair" => vec![
            dictionary! { "O" => "Layout", "SpaceBefore" => 1 }.into(),
            dictionary! { "O" => "Layout", "TextAlign" => "End" }.into(),
        ],
        "Bounded" => dictionary! { "O" => "Layout", "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()] },
        "Inline" => dictionary! { "O" => "Layout", "LineHeight" => 9 },
    }
}

#[test]
fn textedit_classes_apply_the_rules_of_the_elements_own_attributes() {
    let name = |text: &str| Object::Name(text.as_bytes().to_vec());
    for (class, offered_texts) in [
        (name("Indent"), vec!["FIRST", "SECOND"]),
        (
            vec![name("Indent"), 0.into()].into(),
            vec!["FIRST", "SECOND"],
        ),
        (
            vec![name("Indent"), 3.into(), name("Inline")].into(),
            vec!["FIRST", "SECOND"],
        ),
        (name("Centred"), vec!["SECOND"]),
        (name("Pair"), vec!["SECOND"]),
        (vec![name("Indent"), name("Centred")].into(), vec!["SECOND"]),
    ] {
        let (mut doc, ids) = classed(classes(), class.clone());
        assert_eq!(offered(&doc), offered_texts, "{class:?}");
        let root = doc.objects[&ids[1]].clone();
        edit(&mut doc, "SECOND").unwrap();
        assert_eq!(doc.objects[&ids[1]], root);
    }
    for class in [
        name("Missing"),
        name("Bounded"),
        vec![].into(),
        vec![0.into()].into(),
        vec![name("Indent"), (-1).into()].into(),
        vec![name("Indent"), 0.into(), 1.into()].into(),
        Object::string_literal("Indent"),
        vec![name("Indent"); 9].into(),
    ] {
        let (doc, _) = classed(classes(), class);
        refused(doc);
    }
    // A class needs a ClassMap to name, and a ClassMap has to be a dictionary
    // whether or not an element names one of its classes.
    let (mut doc, ids) = fixture(CONTENT);
    doc.get_dictionary_mut(ids[3])
        .unwrap()
        .set("C", name("Indent"));
    assert_eq!(
        textedit::scan(&doc, 0).unwrap_err(),
        "unsupported C metadata in tagged element"
    );
    refused(doc);
    for named in [true, false] {
        let (mut doc, ids) = classed(classes(), name("Indent"));
        if !named {
            doc.get_dictionary_mut(ids[3]).unwrap().remove(b"C");
        }
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("ClassMap", vec![Object::from(classes())]);
        refused(doc);
    }
    // A ClassMap nobody names changes nothing, but its size is bounded.
    for (count, accepted) in [(256, true), (257, false)] {
        let (mut doc, ids) = fixture(CONTENT);
        let mut map = Dictionary::new();
        for index in 0..count {
            map.set(format!("K{index}"), dictionary! { "O" => "Layout" });
        }
        doc.get_dictionary_mut(ids[1]).unwrap().set("ClassMap", map);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{count}");
    }
}

#[test]
fn textedit_inline_leaves_accept_only_a_line_height_class() {
    for (entries, accepted) in [
        (dictionary! { "O" => "Layout", "LineHeight" => 9 }, true),
        (
            dictionary! { "O" => "Layout", "LineHeight" => "Normal" },
            true,
        ),
        (dictionary! { "O" => "Layout" }, false),
        (
            dictionary! { "O" => "Layout", "LineHeight" => 9, "StartIndent" => 2 },
            false,
        ),
        (
            dictionary! { "O" => "Layout", "LineHeight" => 9, "TextAlign" => "Start" },
            false,
        ),
        (
            dictionary! { "O" => "Layout", "LineHeight" => 9, "Placement" => "Block" },
            false,
        ),
    ] {
        let (mut doc, ids, leaves) = nested(false);
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("ClassMap", dictionary! { "Leaf" => entries.clone() });
        doc.get_dictionary_mut(leaves[0])
            .unwrap()
            .set("C", Object::Name(b"Leaf".to_vec()));
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{entries:?}");
        // The same rules hold for the leaf's own attributes.
        let (mut doc, _, leaves) = nested(false);
        doc.get_dictionary_mut(leaves[0])
            .unwrap()
            .set("A", entries.clone());
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "A {entries:?}");
    }
}

// ids: page, root, document, first, second, parents, contents, entry, entry.
fn contents() -> (Document, [ObjectId; 9]) {
    let (mut doc, ids) = fixture(CONTENT);
    let toc = doc.new_object_id();
    let mut entries = Vec::new();
    for paragraph in [ids[3], ids[4]] {
        let entry = doc.add_object(dictionary! {
            "Type" => "StructElem", "S" => "TOCI", "P" => toc, "K" => vec![Object::Reference(paragraph)],
        });
        doc.get_dictionary_mut(paragraph).unwrap().set("P", entry);
        entries.push(entry);
    }
    doc.objects.insert(
        toc,
        dictionary! {
            "Type" => "StructElem", "S" => "TOC", "P" => ids[2],
            "K" => entries.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
        }
        .into(),
    );
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("K", vec![Object::Reference(toc)]);
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], toc, entries[0], entries[1],
        ],
    )
}

#[test]
fn textedit_table_of_contents_groups_editable_entries() {
    let (mut doc, ids) = contents();
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    let before = doc.objects.clone();
    edit(&mut doc, "FIRST").unwrap();
    assert_eq!(offered(&doc), ["IN", "SECOND"]);
    for id in [ids[6], ids[7], ids[8], ids[3], ids[4]] {
        assert_eq!(doc.objects[&id], before[&id]);
    }
    // A table of contents may nest another inside itself or inside an entry.
    let (mut doc, ids) = contents();
    let inner = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "TOC", "P" => ids[6], "K" => vec![Object::Reference(ids[8])],
    });
    doc.get_dictionary_mut(ids[8]).unwrap().set("P", inner);
    doc.get_dictionary_mut(ids[6]).unwrap().set(
        "K",
        vec![Object::Reference(ids[7]), Object::Reference(inner)],
    );
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    let (mut doc, ids) = contents();
    let inner = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "TOC", "P" => ids[7], "K" => vec![Object::Reference(ids[8])],
    });
    doc.get_dictionary_mut(ids[8]).unwrap().set("P", inner);
    doc.get_dictionary_mut(ids[7]).unwrap().set(
        "K",
        vec![Object::Reference(ids[3]), Object::Reference(inner)],
    );
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("K", vec![Object::Reference(ids[7])]);
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    for mode in 0..5 {
        let (mut doc, ids) = contents();
        match mode {
            // An entry outside a table of contents.
            0 => {
                doc.get_dictionary_mut(ids[7]).unwrap().set("P", ids[2]);
                doc.get_dictionary_mut(ids[2]).unwrap().set(
                    "K",
                    vec![Object::Reference(ids[6]), Object::Reference(ids[7])],
                );
                doc.get_dictionary_mut(ids[6])
                    .unwrap()
                    .set("K", vec![Object::Reference(ids[8])]);
            }
            // A table of contents holding a paragraph directly.
            1 => {
                doc.get_dictionary_mut(ids[3]).unwrap().set("P", ids[6]);
                doc.get_dictionary_mut(ids[6]).unwrap().set(
                    "K",
                    vec![Object::Reference(ids[3]), Object::Reference(ids[8])],
                );
                doc.get_dictionary_mut(ids[7])
                    .unwrap()
                    .set("K", Vec::<Object>::new());
            }
            // An entry owning marked content itself.
            2 => {
                doc.get_dictionary_mut(ids[7])
                    .unwrap()
                    .set("K", vec![Object::Integer(0)]);
            }
            // Layout attributes on the grouping element.
            3 => layout(&mut doc, ids[6], dictionary! { "O" => "Layout" }),
            // A role mapped onto a table of contents.
            _ => {
                doc.get_dictionary_mut(ids[6]).unwrap().set("S", "Index");
                doc.get_dictionary_mut(ids[1]).unwrap().set(
                    "RoleMap",
                    dictionary! { "Standard" => "P", "Index" => "TOC" },
                );
            }
        }
        refused(doc);
    }
}
