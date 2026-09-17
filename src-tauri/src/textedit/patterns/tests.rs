use super::*;
use crate::textedit::{self, Change};
use lopdf::{dictionary, ObjectId, Stream};

const TEXT: &str = "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 140 Td (SECOND) Tj ET";
const ART: &str = "q /Pattern CS /Pattern cs /Gradient SCN /Gradient scn 10 10 m 30 10 l 30 30 20 40 10 30 c h B Q";

fn fixture(prefix: &str) -> (Document, [ObjectId; 4]) {
    let (mut doc, _, _, _) = textedit::fonts::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let function = doc.add_object(dictionary! {
        "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()],
        "C0" => vec![1.into(), 0.into(), 0.into()],
        "C1" => vec![0.into(), 0.into(), 1.into()], "N" => 1
    });
    let shading = doc.add_object(dictionary! {
        "ShadingType" => 2, "ColorSpace" => "DeviceRGB",
        "Coords" => vec![0.into(), 0.into(), 100.into(), 0.into()],
        "Extend" => vec![true.into(), true.into()], "Function" => function
    });
    let pattern = doc.add_object(dictionary! {
        "Type" => "Pattern", "PatternType" => 2, "Shading" => shading,
        "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()]
    });
    let mut resources = textedit::resources(&doc, page).unwrap().clone();
    resources.set("Pattern", dictionary! { "Gradient" => pattern });
    let stream = doc.add_object(Stream::new(
        Dictionary::new(),
        format!("{prefix} {TEXT}").into_bytes(),
    ));
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set("Resources", resources);
    dict.set("Contents", stream);
    (doc, [page, pattern, shading, function])
}

#[test]
fn textedit_shading_artwork_and_state_survive_edit_and_deletion() {
    for art in [
        ART.to_string(),
        ART.replace(
            "10 10 m 30 10 l 30 30 20 40 10 30 c h B",
            "10 10 20 20 re B*",
        ),
        format!("{ART} /Pattern CS /Gradient SCN"),
    ] {
        for replacement in ["IN", ""] {
            let (mut doc, ids) = fixture(&art);
            let runs = textedit::scan(&doc, 0).unwrap();
            let before = doc.objects.clone();
            textedit::write(
                &mut doc,
                &[Change {
                    page: 0,
                    layout: None,
                    revision: runs.revision,
                    operator: runs.runs[0].operator,
                    original: "FIRST".into(),
                    replacement: replacement.into(),
                }],
            )
            .unwrap();
            let after = textedit::scan(&doc, 0).unwrap();
            assert_eq!(after.runs[0].text, replacement);
            assert_eq!(after.runs[1], runs.runs[1]);
            assert!(doc.get_page_content(ids[0]).starts_with(art.as_bytes()));
            for (id, object) in before {
                if id != ids[0] {
                    assert_eq!(doc.objects[&id], object);
                }
            }
        }
    }
}

#[test]
fn textedit_shading_refuses_unselected_patterns_and_pattern_text() {
    for prefix in [
        "/Pattern cs /Gradient scn",
        "/Pattern cs",
        "/Pattern cs /Gradient sc",
        "/Pattern cs 0 scn",
        "/Pattern cs /Missing scn",
        "/Pattern cs 0 0 10 10 re f",
        "/Pattern CS 0 0 m 10 10 l S",
        "/Pattern cs q 0 g Q 0 0 10 10 re f",
        "/Pattern cs /Gradient scn q 0 g Q",
        "q /Pattern cs /Gradient scn Q /Pattern cs 0 0 10 10 re f",
    ] {
        assert!(
            textedit::scan(&fixture(prefix).0, 0).is_err(),
            "accepted {prefix}"
        );
    }
    for prefix in [
        "/Pattern cs 0 g",
        "/Pattern cs q /Gradient scn Q 0 g",
        "/Pattern CS 0 0 10 10 re f",
        "/Pattern cs 0 0 10 10 re n 0 g",
    ] {
        assert!(
            textedit::scan(&fixture(prefix).0, 0).is_ok(),
            "refused {prefix}"
        );
    }
}

#[test]
fn textedit_shading_resource_refusals_are_atomic() {
    for (index, key, value) in [
        (1, "PatternType", 1.into()),
        (1, "ExtGState", dictionary! { "ca" => 0.5 }.into()),
        (1, "Matrix", vec![0.into(); 6].into()),
        (2, "ShadingType", 3.into()),
        (2, "ColorSpace", "Pattern".into()),
        (2, "Coords", vec![0.into(); 4].into()),
        (
            2,
            "Coords",
            vec![0.into(), 0.into(), 1_000_001.into(), 0.into()].into(),
        ),
        (2, "Domain", vec![0.into(), 2.into()].into()),
        (2, "Extend", vec![true.into(), 1.into()].into()),
        (2, "Function", Object::Reference((9999, 0))),
        (3, "FunctionType", 4.into()),
        (3, "Domain", vec![0.into(), 2.into()].into()),
        (3, "C0", vec![0.into()].into()),
        (3, "C1", vec![0.into(), 0.into(), 2.into()].into()),
        (3, "N", 0.into()),
        (3, "N", 129.into()),
        (3, "SYNTHETIC_SECRET", Object::string_literal("SECRET")),
    ] {
        let (mut doc, ids) = fixture(ART);
        let runs = textedit::scan(&doc, 0).unwrap();
        let change = Change {
            page: 0,
            layout: None,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
        };
        doc.get_dictionary_mut(ids[index]).unwrap().set(key, value);
        let before = doc.objects.clone();
        let error = textedit::scan(&doc, 0).unwrap_err();
        assert!(!error.contains("SECRET"));
        assert!(textedit::write(&mut doc, &[change]).is_err());
        assert_eq!(doc.objects, before);
    }
}

#[test]
fn textedit_pattern_text_is_preserved_beside_editable_solid_text() {
    let prefix = "q /Pattern cs /Gradient scn BT /F1 12 Tf 40 70 Td (FIRST) Tj ET Q";
    let (mut doc, ids) = fixture(prefix);
    let runs = textedit::scan(&doc, 0).unwrap();
    assert_eq!(runs.runs.len(), 2);
    let before = doc.objects.clone();
    textedit::write(
        &mut doc,
        &[Change {
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: "FIRST".into(),
            replacement: "IN".into(),
            layout: None,
        }],
    )
    .unwrap();
    assert!(doc.get_page_content(ids[0]).starts_with(prefix.as_bytes()));
    assert_eq!(textedit::scan(&doc, 0).unwrap().runs[0].text, "IN");
    for (id, object) in before {
        if id != ids[0] {
            assert_eq!(doc.objects[&id], object);
        }
    }
}
