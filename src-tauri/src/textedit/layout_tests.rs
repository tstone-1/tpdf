use super::*;

fn edit(doc: &Document, replacement: &str, width: f64, height: f64, wrap: bool) -> Change {
    let page = scan(doc, 0).unwrap();
    Change {
        page: 0,
        revision: page.revision,
        operator: page.runs[0].operator,
        original: page.runs[0].text.clone(),
        replacement: replacement.into(),
        layout: Some(Layout {
            width,
            height,
            size: 12.,
            wrap,
            font: EditFont::Auto,
        }),
    }
}

#[test]
fn resized_text_preserves_later_cursor_line_origin_and_resources() {
    let mut doc = tests::with_content(
        b"BT /F1 12 Tf 40 180 Td (A) Tj 100 0 Td (NEIGHBOUR) Tj 0 -60 Td (SECOND) Tj ET",
    );
    let baseline = scan(&doc, 0).unwrap();
    let change = edit(&doc, "LONGER TITLE", 96., 15., false);
    write(&mut doc, &[change]).unwrap();
    let after = scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, "LONGER TITLE");
    for text in ["NEIGHBOUR", "SECOND"] {
        let before = baseline.runs.iter().find(|r| r.text == text).unwrap();
        let saved = after.runs.iter().find(|r| r.text == text).unwrap();
        assert_eq!(saved.matrix, before.matrix);
        assert_eq!(saved.display_rect, before.display_rect);
        assert_eq!(saved.font, before.font);
    }
}

#[test]
fn fallback_embeds_new_characters_and_reopens_for_another_edit() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 0 -80 Td (SECOND) Tj ET");
    let change = edit(&doc, "ACME \u{3a9} Al\u{2082}O\u{2083}", 200., 20., false);
    let report = preview_layout(&doc, &change).unwrap();
    assert_eq!(report.font, "Noto Sans");
    write(&mut doc, std::slice::from_ref(&change)).unwrap();
    let after = scan(&doc, 0).unwrap();
    assert_eq!(after.runs[0].text, change.replacement);
    assert_eq!(
        after
            .runs
            .iter()
            .find(|r| r.text == "SECOND")
            .unwrap()
            .matrix[5],
        100.
    );
    let again = edit(&doc, "ACME \u{3a9}", 200., 20., false);
    write(&mut doc, &[again]).unwrap();
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "ACME \u{3a9}");
}

#[test]
fn wrapping_keeps_spaces_line_breaks_and_following_text_in_place() {
    let mut doc =
        tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 0 -100 Td (SECOND) Tj ET");
    let change = edit(&doc, "ACME FIRST SECOND\nTHIRD", 85., 65., true);
    let preview = preview_layout(&doc, &change).unwrap();
    assert!(preview.lines >= 3);
    write(&mut doc, &[change]).unwrap();
    let after = scan(&doc, 0).unwrap();
    let lines: Vec<_> = after
        .runs
        .iter()
        .filter(|r| !r.text.is_empty() && r.matrix[5] > 80.)
        .collect();
    assert_eq!(
        lines.iter().map(|r| r.text.as_str()).collect::<String>(),
        "ACME FIRST SECONDTHIRD"
    );
    assert!(lines
        .windows(2)
        .all(|pair| pair[0].matrix[5] > pair[1].matrix[5]));
    assert_eq!(
        after
            .runs
            .iter()
            .find(|r| r.text == "SECOND" && r.matrix[5] == 80.)
            .unwrap()
            .matrix[4],
        40.
    );
}

#[test]
fn overflowing_clipped_colliding_and_invalid_layouts_refuse_atomically() {
    let doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 70 0 Td (NEIGHBOUR) Tj ET");
    for (width, height, size, text, wrap) in [
        (20., 20., 12., "TOO LONG", false),
        (20., 15., 12., "LONG LONG LONG", true),
        (14400., 20., 12., "TITLE", false),
        (150., 20., 12., "ACME TITLE OVERLAPS", false),
        (100., 20., f64::NAN, "TITLE", false),
        (100., -1., 12., "TITLE", false),
    ] {
        let mut copy = doc.clone();
        let mut change = edit(&doc, text, width, height, wrap);
        change.layout.as_mut().unwrap().size = size;
        assert!(
            write(&mut copy, &[change]).is_err(),
            "accepted {width} x {height}, {size}, {text}, {wrap}"
        );
        assert_eq!(copy.objects, doc.objects);
    }
    let mut clipped =
        tests::with_content(b"q 40 170 50 30 re W n BT /F1 12 Tf 40 180 Td (TITLE) Tj ET Q");
    // Standard-font clipping is independently refused at discovery.
    assert!(scan(&clipped, 0).is_err());
    assert!(write(&mut clipped, &[]).is_ok());
}

#[test]
fn layout_only_changes_and_all_bundled_styles_are_supported() {
    for font in [
        EditFont::NotoSans,
        EditFont::NotoSansBold,
        EditFont::NotoSansItalic,
        EditFont::NotoSansBoldItalic,
    ] {
        let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (ACME) Tj ET");
        let mut change = edit(&doc, "ACME", 90., 20., false);
        change.layout.as_mut().unwrap().font = font;
        write(&mut doc, &[change]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "ACME");
    }
}

#[test]
fn layout_rotations_and_continued_shows_preserve_later_geometry() {
    for axes in ["1 0 0 1", "0 1 -1 0", "-1 0 0 -1", "0 -1 1 0", "2 0 0 2"] {
        for turn in [0, 90, 180, 270] {
            let bytes = format!(
                "BT /F1 12 Tf {axes} 150 150 Tm (TITLE) Tj (NEXT) Tj 0 -40 Td (SECOND) Tj ET"
            );
            let mut doc = tests::with_content(bytes.as_bytes());
            let id = crate::pagetree::ordered_pages(&doc)[0];
            doc.get_dictionary_mut(id).unwrap().set("Rotate", turn);
            let before = scan(&doc, 0).unwrap();
            for target in ["TITLE", "NEXT"] {
                let mut copy = doc.clone();
                let mut change = edit(&doc, "\u{3a9}", 30., 20., false);
                let source = before.runs.iter().find(|run| run.text == target).unwrap();
                change.operator = source.operator;
                change.original = source.text.clone();
                change.layout.as_mut().unwrap().size = 10.;
                write(&mut copy, &[change])
                    .unwrap_or_else(|error| panic!("{axes}, {turn}, {target}: {error}"));
                let after = scan(&copy, 0).unwrap();
                for text in ["TITLE", "NEXT", "SECOND"]
                    .into_iter()
                    .filter(|text| *text != target)
                {
                    let old = before.runs.iter().find(|run| run.text == text).unwrap();
                    let new = after.runs.iter().find(|run| run.text == text).unwrap();
                    for (a, b) in old.matrix.iter().zip(new.matrix) {
                        assert!((a - b).abs() < 0.00001, "{axes}, {turn}");
                    }
                }
            }
        }
    }
}

#[test]
fn fallback_deletion_and_clipped_growth_remain_editable_or_refuse_atomically() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj ET");
    let mut first = edit(&doc, "ACME", 80., 20., false);
    first.layout.as_mut().unwrap().font = EditFont::NotoSans;
    write(&mut doc, &[first]).unwrap();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = page_content(&doc, id).unwrap();
    let clipped = [b"q 40 170 40 30 re W n\n".as_slice(), &content, b"\nQ"].concat();
    let stream = doc.add_object(Stream::new(Dictionary::new(), clipped));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    let mut change = edit(&doc, "ACME ACME ACME", 200., 20., false);
    let before = doc.objects.clone();
    assert!(write(&mut doc, &[change.clone()])
        .unwrap_err()
        .contains("clips"));
    assert_eq!(doc.objects, before);
    change.replacement.clear();
    change.layout.as_mut().unwrap().font = EditFont::NotoSansBold;
    write(&mut doc, &[change]).unwrap();
    assert!(scan(&doc, 0)
        .unwrap()
        .runs
        .iter()
        .all(|run| run.text.is_empty()));
}

#[test]
fn replacement_fonts_keep_discovery_limits_and_share_programs() {
    let bytes = (0..32)
        .map(|index| format!("BT /F1 1 Tf 40 {} Td (T) Tj ET\n", 200 - index * 3))
        .collect::<String>();
    let doc = tests::with_content(bytes.as_bytes());
    let before = scan(&doc, 0).unwrap();
    let changes: Vec<_> = before
        .runs
        .iter()
        .map(|run| Change {
            page: 0,
            revision: before.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: "\u{3a9}".into(),
            layout: Some(Layout {
                width: 20.,
                height: 1.5,
                size: 1.,
                wrap: false,
                font: EditFont::Auto,
            }),
        })
        .collect();
    let mut copy = doc.clone();
    write(&mut copy, &changes[..31]).unwrap();
    assert_eq!(
        scan(&copy, 0)
            .unwrap()
            .runs
            .iter()
            .filter(|run| run.text == "\u{3a9}")
            .count(),
        31
    );
    let programs: BTreeSet<_> = copy
        .objects
        .values()
        .filter_map(|object| object.as_dict().ok())
        .filter_map(|dict| dict.get(b"FontFile2").ok()?.as_reference().ok())
        .collect();
    assert_eq!(programs.len(), 1);
    let mut rejected = doc.clone();
    assert!(write(&mut rejected, &changes)
        .unwrap_err()
        .contains("32-font"));
    assert_eq!(rejected.objects, doc.objects);
}
