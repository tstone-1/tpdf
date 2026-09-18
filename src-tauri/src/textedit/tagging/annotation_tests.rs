use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Document, Object, ObjectId};

// ids: page, root, document, first, second, parents, link element, annotation.
// The first paragraph's only content is a Link, as Word and Acrobat export an
// inline hyperlink; its annotation is indexed through StructParent 1.
fn linked(subtype: &str, tag: &str) -> (Document, [ObjectId; 8]) {
    let (mut doc, ids) = fixture(CONTENT);
    let annotation = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => subtype, "P" => ids[0], "StructParent" => 1,
        "Rect" => vec![38.into(), 176.into(), 90.into(), 192.into()],
    });
    let link = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => tag, "P" => ids[3], "Pg" => ids[0],
        "K" => vec![
            Object::Integer(0),
            dictionary! { "Type" => "OBJR", "Obj" => annotation, "Pg" => ids[0] }.into(),
        ],
    });
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
            Object::Array(vec![link.into(), ids[4].into()]),
            1.into(),
            link.into(),
        ],
    );
    let content = std::str::from_utf8(CONTENT).unwrap().replacen(
        "/Standard << /MCID 0",
        &format!("/{tag} << /MCID 0"),
        1,
    );
    let stream = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        content.into_bytes(),
    ));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    (
        doc,
        [
            ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], link, annotation,
        ],
    )
}

fn second(doc: &Document) -> Change {
    let runs = textedit::scan(doc, 0).unwrap();
    let run = runs.runs.iter().find(|run| run.text == "SECOND").unwrap();
    Change {
        layout: None,
        page: 0,
        revision: runs.revision,
        operator: run.operator,
        original: "SECOND".into(),
        replacement: "IN".into(),
    }
}

#[test]
fn textedit_link_text_stays_read_only_while_neighbouring_text_is_edited() {
    for (subtype, tag) in [("Link", "Link"), ("Widget", "Form")] {
        let (mut doc, ids) = linked(subtype, tag);
        let before = textedit::scan(&doc, 0).unwrap();
        // The link's text is preserved, not offered: its rectangle is placed
        // over those glyphs and would point at stale text after an edit.
        assert_eq!(
            before
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["SECOND"],
            "{tag}"
        );
        let objects = doc.objects.clone();
        let mut stale = second(&doc);
        stale.operator = textedit::scan(&fixture(CONTENT).0, 0).unwrap().runs[0].operator;
        stale.original = "FIRST".into();
        assert!(textedit::write(&mut doc, &[stale]).is_err());
        assert_eq!(doc.objects, objects);
        {
            let changes = [second(&doc)];
            textedit::write(&mut doc, &changes)
        }
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        for id in [ids[1], ids[3], ids[5], ids[6], ids[7]] {
            assert_eq!(doc.objects[&id], objects[&id], "{tag}");
        }
    }
}

#[test]
fn textedit_link_titles_and_alternate_text_are_preserved_on_read_only_owners() {
    let (mut doc, ids) = linked("Link", "Link");
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("Alt", Object::string_literal("SYNTHETIC LINK"));
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("T", Object::string_literal("SYNTHETIC TITLE"));
    {
        let changes = [second(&doc)];
        textedit::write(&mut doc, &changes)
    }
    .unwrap();
    // A paragraph keeps Word's empty title and stays editable; a title or
    // alternate text that could repeat its wording keeps it read-only.
    for (key, value, offered) in [
        ("T", "", vec!["FIRST", "SECOND"]),
        ("T", "SYNTHETIC TITLE", vec!["SECOND"]),
        ("Alt", "SYNTHETIC", vec!["SECOND"]),
    ] {
        let (mut doc, ids) = fixture(CONTENT);
        doc.get_dictionary_mut(ids[3])
            .unwrap()
            .set(key, Object::string_literal(value));
        let runs = textedit::scan(&doc, 0).unwrap().runs;
        assert_eq!(
            runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(),
            offered,
            "{key} {value}"
        );
    }
}

#[test]
fn textedit_link_annotation_ownership_must_agree_in_both_directions() {
    const OWNERSHIP: &str = "tagged annotation and parent tree disagree on ownership";
    const UNCLAIMED: &str = "tagged parent tree has annotation entries no element claims";
    for case in 0..11 {
        let (mut doc, ids) = linked("Link", "Link");
        let change = second(&doc);
        let expected = match case {
            0 => {
                doc.get_dictionary_mut(ids[7])
                    .unwrap()
                    .set("StructParent", 2);
                OWNERSHIP
            }
            1 => {
                doc.get_dictionary_mut(ids[0])
                    .unwrap()
                    .set("Annots", Vec::<Object>::new());
                OWNERSHIP
            }
            2 => {
                doc.get_dictionary_mut(ids[7])
                    .unwrap()
                    .set("Subtype", "Widget");
                OWNERSHIP
            }
            3 => {
                doc.get_dictionary_mut(ids[7]).unwrap().set("P", ids[1]);
                OWNERSHIP
            }
            4 => {
                // The entry names a different element than the one claiming it.
                let nums = doc
                    .get_dictionary_mut(ids[5])
                    .unwrap()
                    .get_mut(b"Nums")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                nums[3] = ids[4].into();
                OWNERSHIP
            }
            5 => {
                // Remove the OBJR: the annotation entry is left unclaimed.
                doc.get_dictionary_mut(ids[6])
                    .unwrap()
                    .set("K", vec![Object::Integer(0)]);
                UNCLAIMED
            }
            6 => {
                // Two references to one annotation cannot both claim its entry.
                let items = doc
                    .get_dictionary_mut(ids[6])
                    .unwrap()
                    .get_mut(b"K")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                items.push(items[1].clone());
                OWNERSHIP
            }
            7 => {
                // OBJR belongs only to Link or Form owners.
                doc.get_dictionary_mut(ids[6]).unwrap().set("S", "Span");
                let content = std::str::from_utf8(CONTENT).unwrap().to_owned();
                let stream = doc.add_object(lopdf::Stream::new(
                    lopdf::Dictionary::new(),
                    content
                        .replacen("/Standard << /MCID 0", "/Span << /MCID 0", 1)
                        .into_bytes(),
                ));
                doc.get_dictionary_mut(ids[0])
                    .unwrap()
                    .set("Contents", stream);
                "unsupported or inconsistent tagged text structure"
            }
            8 => {
                let items = doc
                    .get_dictionary_mut(ids[6])
                    .unwrap()
                    .get_mut(b"K")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                items[1].as_dict_mut().unwrap().set("SYNTHETIC_SECRET", 1);
                "unsupported unrecognized metadata in tagged object reference"
            }
            9 => {
                // A Link owns no child structure elements.
                let child = doc.add_object(dictionary! {
                    "Type" => "StructElem", "S" => "Span", "P" => ids[6], "Pg" => ids[0], "K" => Vec::<Object>::new(),
                });
                let items = doc
                    .get_dictionary_mut(ids[6])
                    .unwrap()
                    .get_mut(b"K")
                    .unwrap()
                    .as_array_mut()
                    .unwrap();
                items.push(child.into());
                "unsupported or inconsistent tagged text structure"
            }
            _ => {
                // A role alias for Link is not trusted to own annotations.
                doc.get_dictionary_mut(ids[6])
                    .unwrap()
                    .set("S", "Hyperlink");
                doc.get_dictionary_mut(ids[1]).unwrap().set(
                    "RoleMap",
                    dictionary! { "Standard" => "P", "Hyperlink" => "Link" },
                );
                "unsupported or inconsistent tagged text structure"
            }
        };
        let objects = doc.objects.clone();
        assert_eq!(
            textedit::scan(&doc, 0).unwrap_err(),
            expected,
            "case {case}"
        );
        assert!(textedit::write(&mut doc, &[change]).is_err(), "case {case}");
        assert_eq!(doc.objects, objects, "case {case}");
    }
}

#[test]
fn textedit_pdf2_namespace_only_admits_types_whose_meaning_is_shared() {
    for (tag, subtype, accepted) in [("Link", "Link", true), ("Form", "Widget", false)] {
        let (mut doc, ids) = linked(subtype, tag);
        let namespace = doc.add_object(dictionary! {
            "Type" => "Namespace", "NS" => Object::string_literal("http://iso.org/pdf2/ssn"),
        });
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("Namespaces", vec![Object::Reference(namespace)]);
        doc.get_dictionary_mut(ids[6]).unwrap().set("NS", namespace);
        match textedit::scan(&doc, 0) {
            Ok(runs) => assert!(accepted, "{tag}: {:?}", runs.runs.len()),
            Err(error) => {
                assert!(!accepted, "{tag}: {error}");
                assert_eq!(error, "unsupported NS metadata in tagged element");
            }
        }
    }
}

// ISO 32000-1 Table 344: a link's own layout attributes. Acrobat writes a bare
// owner beside a tagged hyperlink, and a figure or field may declare its ink
// bounds. All three are read-only content, so no edit can make them stale.
#[test]
fn textedit_read_only_owners_keep_their_layout_attributes() {
    for (attributes, accepted) in [
        (dictionary! { "O" => "Layout" }, true),
        (
            dictionary! { "O" => "Layout", "Placement" => "Inline" },
            true,
        ),
        (
            dictionary! { "O" => "Layout", "Placement" => "Inline",
            "BBox" => vec![38.into(), 176.into(), 90.into(), 192.into()] },
            true,
        ),
        (
            dictionary! { "O" => "Layout", "Width" => 52, "Height" => "Auto" },
            true,
        ),
        (dictionary! { "O" => "Link" }, false),
        (
            dictionary! { "O" => "Layout", "Placement" => "Middle" },
            false,
        ),
        (
            dictionary! { "O" => "Layout", "BBox" => vec![90.into(), 176.into(), 38.into(), 192.into()] },
            false,
        ),
        // PowerPoint writes block spacing and the default writing mode on
        // figures; they place read-only content and cannot go stale.
        (
            dictionary! { "O" => "Layout", "SpaceBefore" => 12, "StartIndent" => 138.4, "WritingMode" => "LrTb" },
            true,
        ),
        (
            dictionary! { "O" => "Layout", "SpaceBefore" => "Wide" },
            false,
        ),
        (
            dictionary! { "O" => "Layout", "WritingMode" => "TbRl" },
            false,
        ),
        (
            dictionary! { "O" => "Layout", "TextAlign" => "Center" },
            false,
        ),
        (dictionary! { "O" => "Layout", "Width" => "Wide" }, false),
    ] {
        let (mut doc, ids) = linked("Link", "Link");
        doc.get_dictionary_mut(ids[6]).unwrap().set("A", attributes);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted);
    }
    // ISO 32000-1 Table 348: LiveCycle describes every check box's role and
    // state. Only a field may carry one, and only with the standard values.
    for (tag, attributes, accepted) in [
        (
            "Form",
            dictionary! { "O" => "PrintField", "Role" => "cb", "checked" => "off" },
            true,
        ),
        (
            "Form",
            dictionary! { "O" => "PrintField", "Role" => "tv", "Checked" => "on",
            "Desc" => Object::string_literal("SYNTHETIC FIELD") },
            true,
        ),
        ("Form", dictionary! { "O" => "PrintField" }, true),
        (
            "Form",
            dictionary! { "O" => "PrintField", "Role" => "xx" },
            false,
        ),
        (
            "Form",
            dictionary! { "O" => "PrintField", "checked" => "maybe" },
            false,
        ),
        (
            "Form",
            dictionary! { "O" => "PrintField", "Checked" => Object::Boolean(true) },
            false,
        ),
        (
            "Form",
            dictionary! { "O" => "PrintField", "Desc" => 1 },
            false,
        ),
        (
            "Form",
            dictionary! { "O" => "PrintField", "Placement" => "Block" },
            false,
        ),
        (
            "Link",
            dictionary! { "O" => "PrintField", "Role" => "cb" },
            false,
        ),
    ] {
        let subtype = if tag == "Form" { "Widget" } else { "Link" };
        let (mut doc, ids) = linked(subtype, tag);
        doc.get_dictionary_mut(ids[6]).unwrap().set("A", attributes);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), accepted, "{tag}");
    }
    // The attributes survive an edit to the ordinary paragraph beside the link.
    let (mut doc, ids) = linked("Link", "Link");
    doc.get_dictionary_mut(ids[6])
        .unwrap()
        .set("A", dictionary! { "O" => "Layout" });
    let before = doc.objects.clone();
    let scan = textedit::scan(&doc, 0).unwrap();
    assert_eq!(scan.runs.len(), 1);
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: scan.runs[0].text.clone(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(doc.objects[&ids[6]], before[&ids[6]]);
}
