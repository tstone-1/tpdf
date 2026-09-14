use super::*;
use crate::textedit::{self, Change};
use lopdf::{content::Content, dictionary, Stream};

fn fixture(state: Object, body: &str) -> Document {
    let mut doc = textedit::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let resources = textedit::resources(&doc, id).unwrap().clone();
    doc.get_dictionary_mut(id)
        .unwrap()
        .set("Resources", resources);
    let entry = doc.add_object(state);
    doc.get_dictionary_mut(id)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("ExtGState", dictionary! { "G3" => entry });
    let content = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", content);
    doc
}

#[test]
fn textedit_normal_graphics_state_preserves_fill_geometry_and_saved_operators() {
    for state in [
        dictionary! {},
        dictionary! { "ca" => 1, "BM" => "Normal" },
        dictionary! { "Type" => "ExtGState", "ca" => 1.0, "CA" => 1, "BM" => "Normal" },
    ] {
        let mut doc = fixture(state.into(), "/DeviceGray cs 0 0 0 RG 0.3 sc /G3 gs q /DeviceRGB cs 0.8 G 0 0 0 1 K 1 0 0 sc BT /F1 12 Tf /G3 gs 40 180 Td (FIRST) Tj ET Q 0.2 sc BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        assert_eq!(before.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let old = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        let objects = doc.objects.clone();
        let change = Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        textedit::write(&mut doc, std::slice::from_ref(&change)).unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        let new = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        assert_eq!(old.operations.len(), new.operations.len());
        for (i, (a, b)) in old.operations.iter().zip(new.operations).enumerate() {
            assert_eq!(a.operator, b.operator);
            if i != change.operator as usize {
                assert_eq!(a.operands, b.operands);
            }
        }
        for (key, value) in objects {
            if key != id {
                assert_eq!(doc.objects[&key], value);
            }
        }
    }
}

#[test]
fn textedit_graphics_state_refuses_effects_bad_types_and_later_resets_atomically() {
    let mut bad = vec![
        Object::Null,
        Object::Array(vec![]),
        Object::Reference((9999, 0)),
    ];
    for (key, values) in [
        (
            "ca",
            vec![
                0.into(),
                0.5.into(),
                1.01.into(),
                (-1).into(),
                "One".into(),
                Object::Boolean(true),
            ],
        ),
        (
            "CA",
            vec![
                0.into(),
                0.99.into(),
                2.into(),
                Object::Reference((9999, 0)),
            ],
        ),
        (
            "BM",
            vec![
                "Multiply".into(),
                vec![Object::Name(b"Normal".to_vec())].into(),
                1.into(),
            ],
        ),
        (
            "Type",
            vec!["Font".into(), Object::string_literal("ExtGState")],
        ),
    ] {
        for value in values {
            let mut state = dictionary! { "ca" => 1, "BM" => "Normal" };
            state.set(key, value);
            bad.push(state.into());
        }
    }
    for key in [
        "Font", "SMask", "TR", "TR2", "OP", "op", "OPM", "AIS", "TK", "LW", "HT", "Unknown",
    ] {
        let mut state = dictionary! { "ca" => 1, "BM" => "Normal" };
        state.set(key, Object::Null);
        bad.push(state.into());
    }
    for state in bad {
        // A later reset does not excuse an unsupported state that came before it.
        let mut doc = fixture(
            state,
            "/G3 gs /Reset gs BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
        );
        let id = crate::pagetree::ordered_pages(&doc)[0];
        doc.get_dictionary_mut(id)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"ExtGState")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Reset", dictionary! {});
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err());
        assert!(textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: vec![],
                operator: 0,
                original: "FIRST".into(),
                replacement: "IN".into()
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
}

#[test]
fn textedit_graphics_state_limits_names_and_refuses_missing_or_malformed_resources() {
    for count in [32, 33] {
        let body = (0..count).map(|i| format!("/G{i} gs ")).collect::<String>()
            + "/G0 gs BT /F1 12 Tf 40 180 Td (FIRST) Tj ET";
        let mut doc = fixture(dictionary! {}.into(), &body);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let mut entries = Dictionary::new();
        for i in 0..count {
            entries.set(format!("G{i}"), dictionary! {});
        }
        doc.get_dictionary_mut(id)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("ExtGState", entries);
        assert_eq!(textedit::scan(&doc, 0).is_ok(), count == 32);
    }
    for body in [
        "/Absent gs",
        "gs",
        "1 gs",
        "(G3) gs",
        "/G3 /G3 gs",
        "/G3 gs BT /F1 12 Tf (FIRST) Tj ET",
    ] {
        assert!(
            textedit::scan(&fixture(dictionary! {}.into(), body), 0).is_err(),
            "{body}"
        );
    }
    let mut doc = fixture(dictionary! {}.into(), "/G3 gs");
    let id = crate::pagetree::ordered_pages(&doc)[0];
    doc.get_dictionary_mut(id)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("ExtGState", Object::Null);
    assert!(textedit::scan(&doc, 0).is_err());
}

#[test]
fn textedit_stroke_setters_validate_values_without_enabling_stroke_text() {
    for setter in ["G", "RG", "K"] {
        for values in [
            "",
            "-1",
            "1.01",
            "(0)",
            "null",
            "0 0 0 0 0",
            "0 0 2",
            "0 0 -1 0",
        ] {
            let body = format!("{values} {setter} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
            assert!(
                textedit::scan(&fixture(dictionary! {}.into(), &body), 0).is_err(),
                "{body}"
            );
        }
    }
    for body in [
        "0 G BT /F1 12 Tf 1 Tr 40 180 Td (FIRST) Tj ET",
        "0 0 0 RG 0 0 m 100 100 l W S",
        "0 0 0 1 K /DeviceRGB CS",
        "0 G BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 G (SECOND) Tj ET",
    ] {
        assert!(
            textedit::scan(&fixture(dictionary! {}.into(), body), 0).is_err(),
            "{body}"
        );
    }
}
