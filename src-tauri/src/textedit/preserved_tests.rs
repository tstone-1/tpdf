use super::*;

#[test]
fn preserved_transform_bounds_cover_both_axes_and_singular_composition() {
    let identity = [1., 0., 0., 1., 0., 0.];
    assert_eq!(compose_affine(identity, identity).unwrap(), identity);
    for matrix in [
        [0., 0., 0., 1., 0., 0.],
        [1., 0., 0., 0., 0., 0.],
        [1., 2., 2., 4., 0., 0.],
    ] {
        assert!(compose_affine(identity, matrix).is_err());
    }
    assert_eq!(
        compose_affine([2., 3., 4., 5., 6., 7.], [1., 2., 3., 4., 5., 6.]).unwrap(),
        [10., 13., 22., 29., 40., 52.]
    );
    for axis in [0, 3] {
        let mut tiny = identity;
        tiny[axis] = 1e-200;
        assert!(compose_affine(identity, tiny).is_ok());
        assert!(compose_affine(tiny, tiny).is_err());
    }
}

fn fixture(prefix: &str) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(
        Dictionary::new(),
        format!("{prefix} BT /F1 12 Tf 40 180 Td (FIRST) Tj ET").into_bytes(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    doc
}

#[test]
fn preserved_skew_text_keeps_bytes_geometry_and_has_no_edit_address() {
    for matrix in ["1 0.2 0.3 1 40 60", "0 -1 1 0 40 60", "1 0.2 0 -1 40 60"] {
        let prefix =
            format!("q {matrix} cm BT /F1 12 Tf 1 0 0 1 0 0 Tm (SECOND) Tj (FIRST) Tj ET Q");
        for replacement in ["IN", ""] {
            let mut doc = fixture(&prefix);
            let before = inspect(&doc, 0).unwrap();
            assert_eq!(before.preserved.len(), 2);
            assert_eq!(before.runs.runs.len(), 1);
            let run = &before.runs.runs[0];
            let change = Change {
                page: 0,
                layout: None,
                revision: before.runs.revision.clone(),
                operator: run.operator,
                original: run.text.clone(),
                replacement: replacement.into(),
            };
            let mut forged = change.clone();
            forged.operator = before.preserved[0].operator;
            forged.original = before.preserved[0].text.clone();
            let objects = doc.objects.clone();
            assert!(write(&mut doc, &[forged]).is_err());
            assert_eq!(doc.objects, objects);
            write(&mut doc, &[change]).unwrap();
            let after = inspect(&doc, 0).unwrap();
            assert_eq!(after.preserved, before.preserved);
            assert_eq!(after.runs.runs[0].text, replacement);
            assert!(doc
                .get_page_content(before.id)
                .starts_with(prefix.as_bytes()));
        }
    }
}

#[test]
fn preserved_skew_text_blocks_layout_collisions() {
    let prefix = "q 1 0.2 0 1 90 180 cm BT /F1 12 Tf 1 0 0 1 0 0 Tm (SECOND) Tj ET Q";
    let mut doc = fixture(prefix);
    let before = scan(&doc, 0).unwrap();
    let change = Change {
        page: 0,
        revision: before.revision,
        operator: before.runs[0].operator,
        original: "FIRST".into(),
        replacement: "FIRST FIRST FIRST".into(),
        layout: Some(Layout {
            width: 200.,
            height: 20.,
            size: 12.,
            wrap: false,
            font: EditFont::Original,
        }),
    };
    let objects = doc.objects.clone();
    assert!(write(&mut doc, &[change]).unwrap_err().contains("overlap"));
    assert_eq!(doc.objects, objects);
}

#[test]
fn preserved_text_matrices_allow_neighbour_edits_without_offering_skew_or_mirrors() {
    for matrix in [
        "1 0.2 0 1 90 180",
        "1 0 0.2 1 90 180",
        "-1 0 0 1 150 180",
        "1 0 0 -1 90 190",
    ] {
        let prefix = format!("BT /F1 12 Tf {matrix} Tm (SECOND) Tj ET");
        let mut doc = fixture(&prefix);
        let before = inspect(&doc, 0).unwrap();
        assert_eq!(before.runs.runs.len(), 1);
        assert_eq!(before.preserved.len(), 1);
        let run = &before.runs.runs[0];
        let change = Change {
            page: 0,
            revision: before.runs.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: "IN".into(),
            layout: None,
        };
        let mut forged = change.clone();
        forged.operator = before.preserved[0].operator;
        forged.original = "SECOND".into();
        assert!(write(&mut doc, &[forged]).is_err());
        write(&mut doc, &[change]).unwrap();
        let after = inspect(&doc, 0).unwrap();
        assert_eq!(after.preserved, before.preserved);
        assert!(doc
            .get_page_content(before.id)
            .starts_with(prefix.as_bytes()));
    }
}

#[test]
fn preserved_skew_text_does_not_hide_invalid_content_or_clip_changes() {
    for content in [
        "BT /F1 12 Tf 40 60 Td (SECOND) Tj ET",
        "1 0 0 0 0 0 cm",
        "0 0 20 20 re W n",
        "0 0 m 20 0 l 20 20 l h W n",
        "BT /F1 12 Tf 0 0 Td (SECOND) Tj",
        "BT /F1 12 Tf 0 0 Td (SECOND) Tj /Bogus Do ET",
    ] {
        let prefix = format!("q 1 0.2 0 1 1000000 0 cm {content} Q");
        assert!(scan(&fixture(&prefix), 0).is_err(), "accepted {content}");
    }
}
