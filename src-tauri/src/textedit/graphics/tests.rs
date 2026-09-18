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
        dictionary! { "ca" => 1, "BM" => "Normal", "LW" => 0.5, "LC" => 0, "LJ" => 2, "ML" => 4, "SA" => true },
        dictionary! { "Type" => "ExtGState", "ca" => 1.0, "CA" => 1, "BM" => "Normal" },
        // Constant alpha is kept with the text it applies to (TikZ, Apache FOP).
        dictionary! { "ca" => 0.25, "CA" => 0 },
        dictionary! { "Type" => "ExtGState", "ca" => 0, "CA" => 0.99 },
    ] {
        let mut doc = fixture(state.into(), "/DeviceGray cs 0 0 0 RG 0.3 sc /G3 gs q /DeviceRGB cs 0.8 G 0 0 0 1 K 1 0 0 sc BT /F1 12 Tf /G3 gs 40 180 Td (FIRST) Tj ET Q 0.2 sc BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        assert_eq!(before.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let old = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        let objects = doc.objects.clone();
        let change = Change {
            layout: None,
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
                1.01.into(),
                (-1).into(),
                "One".into(),
                Object::Boolean(true),
            ],
        ),
        (
            "CA",
            vec![(-0.01).into(), 2.into(), Object::Reference((9999, 0))],
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
                layout: None,
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
        "0 G BT /F1 12 Tf 5 Tr 40 180 Td (FIRST) Tj ET",
        "0 0 0 RG 0 0 m 100 100 l W S",
        "0 0 0 1 K /DeviceRGB CS 0 SC",
        "0 G BT /F1 12 Tf 0 G (SECOND) Tj ET",
    ] {
        assert!(
            textedit::scan(&fixture(dictionary! {}.into(), body), 0).is_err(),
            "{body}"
        );
    }
}

#[test]
fn textedit_rendering_intents_preserve_operators_resources_and_geometry() {
    for intent in [
        "AbsoluteColorimetric",
        "RelativeColorimetric",
        "Saturation",
        "Perceptual",
    ] {
        let body = format!("/{intent} ri q /G3 gs BT /F1 12 Tf /{intent} ri 40 180 Td (FIRST) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
        let mut doc = fixture(dictionary! { "RI" => intent }.into(), &body);
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        assert_eq!(before.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let bytes = doc.get_page_content(page);
        let objects = doc.objects.clone();
        let change = Change {
            layout: None,
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        textedit::write(&mut doc, &[change]).unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        assert_eq!(
            crate::encoding::resolve(
                &doc,
                doc.get_dictionary(page).unwrap().get(b"Contents").unwrap()
            )
            .as_stream()
            .unwrap()
            .content,
            String::from_utf8(bytes)
                .unwrap()
                .replace("(FIRST)", "(IN)")
                .into_bytes()
        );
        for (id, object) in objects {
            if id != page {
                assert_eq!(doc.objects[&id], object);
            }
        }
    }
}

#[test]
fn textedit_rendering_intents_refuse_unknown_names_types_and_implicit_positions() {
    for body in [
        "ri",
        "1 ri",
        "(Perceptual) ri",
        "[ /Perceptual ] ri",
        "null ri",
        "/Perceptual /Saturation ri",
        "/Unknown ri",
        "/perceptual ri",
        "/PerceptualExtra ri",
    ] {
        let body = format!("{body} /Perceptual ri BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
        let mut doc = fixture(dictionary! {}.into(), &body);
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "{body}");
        assert!(textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: vec![],
                operator: 0,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }]
        )
        .is_err());
        assert_eq!(doc.objects, objects);
    }
    for value in [
        Object::Name(b"Unknown".to_vec()),
        Object::string_literal("Perceptual"),
        Object::Integer(0),
        Object::Null,
        Object::Reference((9999, 0)),
    ] {
        let doc = fixture(
            dictionary! { "RI" => value }.into(),
            "/G3 gs /Perceptual ri BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
        );
        assert!(textedit::scan(&doc, 0).is_err());
    }
    for setter in ["/Perceptual ri", "/G3 gs"] {
        let body = format!("BT /F1 12 Tf {setter} (SECOND) Tj ET");
        let doc = fixture(dictionary! { "RI" => "Perceptual" }.into(), &body);
        assert!(textedit::scan(&doc, 0).is_err(), "{setter}");
    }
}

#[test]
fn textedit_print_graphics_state_preserves_flags_resources_and_scoped_operators() {
    // Both overprint modes and stroke-adjustment values; OP alone also sets op.
    for (overprint, stroke_adjust, mode) in [(false, false, 0), (true, true, 1)] {
        for fill in [None, Some(false), Some(true)] {
            let mut state = dictionary! {
                "Type" => "ExtGState", "BM" => "Normal", "ca" => 1, "CA" => 1,
                "OP" => overprint, "OPM" => mode, "SA" => stroke_adjust,
                "SMask" => "None", "AIS" => false,
            };
            if let Some(fill) = fill {
                state.set("op", fill);
            }
            let mut doc = fixture(state.into(), "/G3 gs q BT /F1 12 Tf /G3 gs 40 180 Td (FIRST) Tj ET Q BT /F1 12 Tf 40 140 Td (SECOND) Tj ET");
            let page = crate::pagetree::ordered_pages(&doc)[0];
            let objects = doc.objects.clone();
            let before = textedit::scan(&doc, 0).unwrap();
            let old = Content::decode_strict(&doc.get_page_content(page)).unwrap();
            let update = Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            textedit::write(&mut doc, std::slice::from_ref(&update)).unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, "IN");
            assert_eq!(after.runs[0].matrix, before.runs[0].matrix);
            assert_eq!(after.runs[1], before.runs[1]);
            let new = Content::decode_strict(&doc.get_page_content(page)).unwrap();
            assert_eq!(new.operations.len(), old.operations.len());
            for (index, (old, new)) in old.operations.iter().zip(&new.operations).enumerate() {
                assert_eq!(old.operator, new.operator);
                if index != update.operator as usize {
                    assert_eq!(old.operands, new.operands);
                }
            }
            // The only original object changed by a rewrite is the page's
            // Contents reference. In particular OP/op presence stays exact.
            for (id, value) in objects {
                if id != page {
                    assert_eq!(doc.objects[&id], value);
                }
            }
        }
    }
}

#[test]
fn textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically() {
    let cases = [
        (
            "OP",
            vec![
                0.into(),
                1.into(),
                Object::string_literal("true"),
                Object::Null,
            ],
        ),
        (
            "op",
            vec![0.into(), "True".into(), Object::Reference((9999, 0))],
        ),
        (
            "SA",
            vec![0.into(), 1.into(), Object::Null, vec![true.into()].into()],
        ),
        (
            "OPM",
            vec![
                (-1).into(),
                2.into(),
                0.0.into(),
                1.0.into(),
                true.into(),
                "One".into(),
            ],
        ),
        (
            "AIS",
            vec![true.into(), 0.into(), Object::Null, "False".into()],
        ),
        (
            "SMask",
            vec![
                Object::Null,
                "Alpha".into(),
                Object::string_literal("None"),
                dictionary! { "S" => "Alpha" }.into(),
                Object::Reference((9999, 0)),
            ],
        ),
    ];
    for (key, values) in cases {
        for value in values {
            let mut state = dictionary! { "ca" => 1, "CA" => 1, "BM" => "Normal" };
            state.set(key, value);
            // A later Q restores the valid initial state but cannot excuse
            // an unsupported entry earlier in the stream.
            let mut doc = fixture(
                state.into(),
                "q /G3 gs Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
            );
            let objects = doc.objects.clone();
            assert!(
                textedit::scan(&doc, 0)
                    .unwrap_err()
                    .contains("unsupported external text graphics state"),
                "{key}"
            );
            assert!(textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: vec![],
                    operator: 5,
                    original: "FIRST".into(),
                    replacement: "IN".into()
                }]
            )
            .is_err());
            assert_eq!(doc.objects, objects, "{key}");
        }
    }
}

#[test]
fn textedit_stroke_styles_preserve_scoped_operators_and_other_text() {
    for style in [
        "0 J 0 j 1 M [] 0 d",
        "1 J 1 j 4 M [0 2.972] 0 d",
        "2 J 2 j 10.5 M [3 2 1] 2.5 d",
        "1 J 2 j 1000000 M [1000000 0] 1000000 d",
        &format!("[{}] 0 d", "1 ".repeat(32)),
    ] {
        let body = format!("{style} 1.5 w 30 160 m 260 160 l S q 0 J 0 j 4 M [] 0 d BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q 30 30 m 80 50 l 130 30 l S BT {style} /F1 12 Tf 40 140 Td (SECOND) Tj ET");
        let mut doc = fixture(dictionary! {}.into(), &body);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let old = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        let resources = textedit::resources(&doc, id).unwrap().clone();
        let change = Change {
            layout: None,
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
        assert_eq!(textedit::resources(&doc, id).unwrap(), &resources);
        let new = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        assert_eq!(old.operations.len(), new.operations.len());
        for (index, (a, b)) in old.operations.iter().zip(new.operations).enumerate() {
            assert_eq!(a.operator, b.operator);
            if index != change.operator as usize {
                assert_eq!(a.operands, b.operands);
            }
        }
    }
}

#[test]
fn textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically() {
    for style in [
        "-1 J",
        "3 J",
        "1.0 J",
        "true J",
        "J",
        "0 1 J",
        "-1 j",
        "3 j",
        "1.0 j",
        "/Round j",
        "j",
        "0 1 j",
        "0 M",
        "0.99 M",
        "-1 M",
        "1000001 M",
        "null M",
        "M",
        "1 2 M",
        "[-1 2] 0 d",
        "[1 -2] 0 d",
        "[0] 0 d",
        "[0 0] 1 d",
        "[1] -1 d",
        "[1000001] 0 d",
        "[1] 1000001 d",
        "[1 null] 0 d",
        "[[]] 0 d",
        "[] null d",
        "1 0 d",
        "[1] d",
        "[] 0 1 d",
        &format!("[{}] 0 d", "1 ".repeat(33)),
        // Admitting stroke state must never admit clipping text.
        "1 J 1 j [0 3] 0 d 5 Tr",
        "6 Tr",
        "4 Tr",
        "7 Tr",
    ] {
        let mut doc = fixture(
            dictionary! {}.into(),
            &format!("{style} 0 J 0 j 10 M [] 0 d 0 Tr BT /F1 12 Tf 40 180 Td (FIRST) Tj ET"),
        );
        let objects = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "{style}");
        assert!(
            textedit::write(
                &mut doc,
                &[Change {
                    layout: None,
                    page: 0,
                    revision: vec![],
                    operator: 0,
                    original: "FIRST".into(),
                    replacement: "IN".into(),
                }]
            )
            .is_err(),
            "{style}"
        );
        assert_eq!(doc.objects, objects, "{style}");
    }
}

#[test]
fn textedit_stroke_styles_refuse_nonfinite_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(stroke("M", &[Object::Real(value)]).is_err());
        assert!(stroke("d", &[vec![Object::Real(value)].into(), 0.into()]).is_err());
        assert!(stroke("d", &[vec![Object::Integer(1)].into(), Object::Real(value)]).is_err());
    }
}

// ISO 32000-1 Table 58, 10.6.2 and 10.6.3: flatness and smoothness are device
// tolerances. Distiller, PDFMaker and Designer write them into every page; the
// operators and the state are kept byte for byte beside an edit.
#[test]
fn textedit_rendering_tolerances_are_preserved_within_their_ranges() {
    for (state, body) in [
        (dictionary! { "SM" => 0.02 }, "/G3 gs 1 i"),
        (dictionary! { "SM" => 0, "FL" => 100 }, "/G3 gs 0 i"),
        (dictionary! { "SM" => 1.0, "FL" => 0.5 }, "/G3 gs 100 i"),
    ] {
        let mut doc = fixture(
            state.into(),
            &format!(
                "{body} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET"
            ),
        );
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2, "{body}");
        let id = crate::pagetree::ordered_pages(&doc)[0];
        let old = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        let objects = doc.objects.clone();
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
        let new = Content::decode_strict(&doc.get_page_content(id)).unwrap();
        for (i, (a, b)) in old.operations.iter().zip(&new.operations).enumerate() {
            if i != before.runs[0].operator as usize {
                assert_eq!((&a.operator, &a.operands), (&b.operator, &b.operands));
            }
        }
        for (key, value) in objects {
            if key != id {
                assert_eq!(doc.objects[&key], value);
            }
        }
    }
    for (state, body) in [
        (dictionary! { "SM" => 1.01 }, "/G3 gs"),
        (dictionary! { "SM" => -0.1 }, "/G3 gs"),
        (dictionary! { "SM" => "Low" }, "/G3 gs"),
        (dictionary! { "FL" => 101 }, "/G3 gs"),
        (dictionary! { "FL" => -1 }, "/G3 gs"),
        (dictionary! {}, "101 i"),
        (dictionary! {}, "-1 i"),
        (dictionary! {}, "/Low i"),
        (dictionary! {}, "1 2 i"),
        (dictionary! {}, "i"),
    ] {
        let doc = fixture(
            state.into(),
            &format!("{body} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET"),
        );
        assert!(textedit::scan(&doc, 0).is_err(), "{body}");
    }
}

// Word draws simulated bold as fill-then-stroke (Tr 2), and scanned documents
// carry their OCR text invisibly (Tr 3). Both are edited in place: the mode is
// kept, so the replacement paints the same way, and stroked text's hit area
// reaches half the line width beyond the glyphs.
#[test]
fn textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink() {
    let rect = |body: &str, state: Dictionary| {
        let doc = fixture(state.into(), body);
        textedit::scan(&doc, 0).unwrap().runs[0].display_rect
    };
    let plain = rect("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET", dictionary! {});
    for (body, state, half) in [
        (
            "4 w BT /F1 12 Tf 2 Tr 40 180 Td (FIRST) Tj ET",
            dictionary! {},
            2.,
        ),
        (
            "4 w BT /F1 12 Tf 1 Tr 40 180 Td (FIRST) Tj ET",
            dictionary! {},
            2.,
        ),
        (
            "/G3 gs BT /F1 12 Tf 2 Tr 40 180 Td (FIRST) Tj ET",
            dictionary! { "LW" => 6 },
            3.,
        ),
        (
            "2 0 0 2 0 0 cm 4 w BT /F1 6 Tf 2 Tr 20 90 Td (FIRST) Tj ET",
            dictionary! {},
            4.,
        ),
        // Invisible text paints nothing and needs no margin.
        (
            "4 w BT /F1 12 Tf 3 Tr 40 180 Td (FIRST) Tj ET",
            dictionary! {},
            0.,
        ),
        // Q restores both the mode and the width.
        (
            "q 9 w 2 Tr Q BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
            dictionary! {},
            0.,
        ),
        (
            "9 w q 1 w Q BT /F1 12 Tf 2 Tr 40 180 Td (FIRST) Tj ET",
            dictionary! {},
            4.5,
        ),
    ] {
        let stroked = rect(body, state);
        assert_eq!(
            stroked,
            [
                plain[0] - half,
                plain[1] - half,
                plain[2] + half,
                plain[3] + half,
            ],
            "{body}"
        );
    }
    // The edit keeps the mode and the width, byte for byte.
    let mut doc = fixture(
        dictionary! {}.into(),
        "4 w BT /F1 12 Tf 3 Tr 40 180 Td (FIRST) Tj ET",
    );
    let scan = textedit::scan(&doc, 0).unwrap();
    textedit::write(
        &mut doc,
        &[Change {
            layout: None,
            page: 0,
            revision: scan.revision,
            operator: scan.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        }],
    )
    .unwrap();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = Content::decode_strict(&doc.get_page_content(id)).unwrap();
    let operators = content
        .operations
        .iter()
        .map(|op| op.operator.as_str())
        .collect::<Vec<_>>();
    assert_eq!(operators, ["w", "BT", "Tf", "Tr", "Td", "Tj", "ET"]);
    assert_eq!(content.operations[3].operands, [Object::Integer(3)]);
}
