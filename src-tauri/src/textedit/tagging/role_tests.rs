use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Object};

#[test]
fn textedit_figure_ownership_preserves_paragraph_markers_without_admitting_text() {
    for (paint, accepted) in [
        ("40 20 10 10 re f", true),
        ("BT /F1 12 Tf 40 20 Td (SECOND) Tj ET", false),
    ] {
        let content = format!("/Standard << /MCID 0 >> BDC BT /F1 12 Tf 40 180 Td (FIRST) Tj ET EMC /P << /MCID 1 >> BDC {paint} EMC");
        let (mut doc, ids) = fixture(content.as_bytes());
        let bounds = doc.add_object(Object::Array(vec![
            40.into(),
            20.into(),
            50.into(),
            30.into(),
        ]));
        let figure = doc.get_dictionary_mut(ids[4]).unwrap();
        figure.set("S", "Figure");
        figure.set(
            "A",
            dictionary! { "O" => "Layout", "Placement" => "Block", "BBox" => bounds },
        );
        let result = textedit::scan(&doc, 0);
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            let before = result.unwrap();
            let objects = doc.objects.clone();
            textedit::write(
                &mut doc,
                &[Change {
                    page: 0,
                    layout: None,
                    revision: before.revision,
                    operator: before.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                }],
            )
            .unwrap();
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
            for (id, value) in objects {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], value);
                }
            }
        }
    }
}

#[test]
fn textedit_figure_cannot_expose_text_through_a_span_child() {
    let (mut doc, ids) = fixture(CONTENT);
    let span =
        doc.add_object(dictionary! { "S" => "Span", "P" => ids[4], "Pg" => ids[0], "K" => 1 });
    let figure = doc.get_dictionary_mut(ids[4]).unwrap();
    figure.set("S", "Figure");
    figure.set("K", span);
    figure.remove(b"A");
    doc.get_dictionary_mut(ids[5]).unwrap().set(
        "Nums",
        vec![0.into(), Object::Array(vec![ids[3].into(), span.into()])],
    );
    let stream = doc.add_object(lopdf::Stream::new(
        Dictionary::new(),
        String::from_utf8(CONTENT.to_vec())
            .unwrap()
            .replace("/Standard << /MCID 1", "/Span << /MCID 1")
            .into_bytes(),
    ));
    doc.get_dictionary_mut(ids[0])
        .unwrap()
        .set("Contents", stream);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_exporter_roles_empty_cells_and_root_siblings_preserve_structure() {
    let (mut doc, ids) = fixture(CONTENT);
    let mut roles = dictionary! { "Standard" => "P", "SyntheticDocument" => "Document" };
    for i in 0..20 {
        roles.set(format!("Unused{i}"), Object::Name(b"Note".to_vec()));
    }
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("RoleMap", roles);
    doc.get_dictionary_mut(ids[2])
        .unwrap()
        .set("S", "SyntheticDocument");
    let table = doc.add_object(dictionary! { "S" => "Table", "P" => ids[1] });
    let row = doc.add_object(dictionary! { "S" => "TR", "P" => table });
    let cells: Vec<_> = (0..300)
        .map(|_| {
            Object::Reference(doc.add_object(dictionary! {
                "S" => "TD", "P" => row, "Pg" => ids[0], "K" => Vec::<Object>::new()
            }))
        })
        .collect();
    doc.get_dictionary_mut(row).unwrap().set("K", cells);
    doc.get_dictionary_mut(table).unwrap().set("K", row);
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("K", vec![Object::Reference(ids[2]), table.into()]);
    let before = textedit::scan(&doc, 0).unwrap();
    let original = doc.objects.clone();
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    for (id, value) in original {
        if id != ids[0] {
            assert_eq!(doc.objects[&id], value);
        }
    }
    // An empty node may not masquerade as the owner of existing marked content.
    doc.get_dictionary_mut(ids[5])
        .unwrap()
        .get_mut(b"Nums")
        .unwrap()
        .as_array_mut()
        .unwrap()[1]
        .as_array_mut()
        .unwrap()[0] = row.into();
    assert!(textedit::scan(&doc, 0).is_err());
}

// Independent census from ISO 32000-1 Tables 333-340, including types that
// discovery does not yet admit. Test the scan/write paths, not the predicate.
const STANDARD: &[&str] = &[
    "Document",
    "Part",
    "Art",
    "Sect",
    "Div",
    "BlockQuote",
    "Caption",
    "TOC",
    "TOCI",
    "Index",
    "NonStruct",
    "Private",
    "P",
    "H",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    "L",
    "LI",
    "Lbl",
    "LBody",
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    "Span",
    "Quote",
    "Note",
    "Reference",
    "BibEntry",
    "Code",
    "Link",
    "Annot",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    "Figure",
    "Formula",
    "Form",
];

#[test]
fn textedit_role_map_cannot_redefine_any_standard_type_even_when_unused() {
    for &role in STANDARD {
        for target in ["P", "Sect", role] {
            let (mut doc, ids) = fixture(CONTENT);
            let before = textedit::scan(&doc, 0).unwrap();
            let change = Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            let mut roles = dictionary! { "Standard" => "P" };
            roles.set(role, Object::Name(target.as_bytes().to_vec()));
            doc.get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", roles);
            let objects = doc.objects.clone();
            // The original content never uses this new entry. A later grammar
            // refusal therefore cannot conceal a missing RoleMap guard.
            assert_eq!(
                textedit::scan(&doc, 0).unwrap_err(),
                "tagged RoleMap contains unsupported or conflicting roles",
                "{role} -> {target}"
            );
            assert!(textedit::write(&mut doc, &[change]).is_err());
            assert_eq!(doc.objects, objects);
        }
    }
}

#[test]
fn textedit_role_map_blocks_disguised_standard_content_but_preserves_custom_aliases() {
    for role in [
        "Figure",
        "Table",
        "Span",
        "Link",
        "SyntheticParagraph",
        "FigureCustom",
        "figure",
        "H7",
    ] {
        let content = std::str::from_utf8(CONTENT)
            .unwrap()
            .replace("/Standard", &format!("/{role}"));
        let (mut doc, ids) = fixture(content.as_bytes());
        for id in [ids[3], ids[4]] {
            doc.get_dictionary_mut(id)
                .unwrap()
                .set("S", Object::Name(role.as_bytes().to_vec()));
        }
        let mut roles = Dictionary::new();
        roles.set(role, Object::Name(b"P".to_vec()));
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("RoleMap", roles);
        if ["Figure", "Table", "Span", "Link"].contains(&role) {
            assert_eq!(
                textedit::scan(&doc, 0).unwrap_err(),
                "tagged RoleMap contains unsupported or conflicting roles",
                "{role}"
            );
            continue;
        }
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        let original = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        for (id, object) in original {
            if id != ids[0] {
                assert_eq!(doc.objects[&id], object);
            }
        }
        // The saved stream still names the producer's custom role.
        let stream = doc.get_page_content(ids[0]);
        assert!(String::from_utf8(stream)
            .unwrap()
            .contains(&format!("/{role}")));
    }
}
