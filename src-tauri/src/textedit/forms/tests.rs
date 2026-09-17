use super::*;
use crate::textedit::{self, Change, EditFont, Layout};
use lopdf::{dictionary, Stream};

fn fixture(text: bool) -> (Document, ObjectId, ObjectId) {
    let (mut doc, _, _, _) = textedit::fonts::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let resources = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .clone();
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 40.into(), 30.into()],
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 65.into(), 170.into()],
            "Resources" => resources,
        },
        if text {
            b"BT /F1 12 Tf 0 10 Td (SECOND) Tj ET".to_vec()
        } else {
            b"0 0 40 30 re f".to_vec()
        },
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("XObject", dictionary! { "Form" => form });
    let content = doc.add_object(Stream::new(
        Dictionary::new(),
        b"/Form Do BT /F1 12 Tf 40 180 Td (FIRST) Tj ET".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    (doc, page, form)
}

#[test]
fn textedit_preserved_forms_roundtrip_without_rewriting_or_offering_their_text() {
    for text in [false, true] {
        let (mut doc, page, _) = fixture(text);
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 1);
        assert_eq!(before.runs[0].text, "FIRST");
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
            if id != page {
                assert_eq!(doc.objects[&id], value);
            }
        }
    }
}

#[test]
fn textedit_layout_reserves_transformed_form_text_but_not_plain_graphics() {
    for text in [false, true] {
        let (mut doc, _, _) = fixture(text);
        let before = textedit::scan(&doc, 0).unwrap();
        let change = Change {
            page: 0,
            revision: before.revision,
            operator: before.runs[0].operator,
            original: "FIRST".into(),
            replacement: "FIRST FIRST".into(),
            layout: Some(Layout {
                width: 120.,
                height: 20.,
                size: 12.,
                wrap: false,
                font: EditFont::Original,
            }),
        };
        let objects = doc.objects.clone();
        let result = textedit::write(&mut doc, &[change]);
        if text {
            assert!(result.unwrap_err().contains("overlap"));
            assert_eq!(doc.objects, objects);
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn textedit_preserved_forms_validate_normal_graphics_states_without_hidden_carriers() {
    for state in [
        dictionary! { "Type" => "ExtGState", "BM" => "Normal", "CA" => 1 },
        dictionary! { "SMask" => dictionary! {} },
        dictionary! { "Font" => vec![Object::Null, 12.into()] },
        dictionary! { "CA" => 0.5 },
    ] {
        let accepted = state.has(b"BM");
        let (mut doc, _, form) = fixture(true);
        let stream = doc.get_object_mut(form).unwrap().as_stream_mut().unwrap();
        stream.content.splice(..0, b"/GS1 gs ".iter().copied());
        stream
            .dict
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("ExtGState", dictionary! { "GS1" => state });
        let original = doc.objects.clone();
        let result = textedit::scan(&doc, 0);
        if accepted {
            assert_eq!(result.unwrap().runs[0].text, "FIRST");
        } else {
            assert!(result.unwrap_err().contains("graphics state"));
        }
        assert_eq!(doc.objects, original);
    }
}

#[test]
fn textedit_preserved_forms_refuse_cycles_external_carriers_and_unbalanced_state() {
    for case in 0..9 {
        let (mut doc, _, form) = fixture(false);
        let stream = doc.get_object_mut(form).unwrap().as_stream_mut().unwrap();
        match case {
            0 => {
                stream.dict.set("Ref", dictionary! {});
            }
            1 => {
                stream.content = b"q".to_vec();
            }
            2 => {
                stream.content = b"BT".to_vec();
            }
            3 => {
                stream.content = b"BI /W 1 /H 1 ID x EI".to_vec();
            }
            4 => {
                stream.content = b"/Loop Do".to_vec();
                stream
                    .dict
                    .get_mut(b"Resources")
                    .unwrap()
                    .as_dict_mut()
                    .unwrap()
                    .set("XObject", dictionary! { "Loop" => form });
            }
            5 => {
                stream.content = vec![b' '; MAX_CONTENT + 1];
            }
            6 => {
                stream.content = b"0 g ".repeat(MAX_OPERATIONS + 1);
            }
            7 => stream.content = b"/HiddenText gs 0 0 10 10 re f".to_vec(),
            _ => stream.content = b"/Pattern cs /HiddenText scn 0 0 10 10 re f".to_vec(),
        }
        let before = doc.objects.clone();
        assert!(textedit::scan(&doc, 0).is_err(), "case {case}");
        assert_eq!(doc.objects, before);
    }
}
