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
fn cjk_fallback_subsets_and_reopens_with_new_characters() {
    for chosen in [
        EditFont::Auto,
        EditFont::NotoSansCjkSc,
        EditFont::NotoSansCjkScBold,
    ] {
        let mut doc =
            tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 0 -80 Td (SECOND) Tj ET");
        let baseline = scan(&doc, 0).unwrap();
        let mut change = edit(
            &doc,
            "ACME \u{65b0}\u{589e}\u{6c49}\u{5b57} \u{65e5}\u{672c}\u{8a9e} \u{d55c}\u{ae00}",
            210.,
            22.,
            false,
        );
        change.layout.as_mut().unwrap().font = chosen;
        let report = preview_layout(&doc, &change).unwrap();
        assert_eq!(
            report.font,
            if chosen == EditFont::NotoSansCjkScBold {
                "Noto Sans CJK SC Bold"
            } else {
                "Noto Sans CJK SC"
            }
        );
        write(&mut doc, std::slice::from_ref(&change)).unwrap();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(
            bytes.len() < 64 * 1024,
            "a short edit must not embed a full CJK font"
        );
        let mut reopened = Document::load_mem(&bytes).unwrap();
        let after = scan(&reopened, 0).unwrap();
        assert_eq!(after.runs[0].text, change.replacement);
        assert_eq!(
            after
                .runs
                .iter()
                .find(|r| r.text == "SECOND")
                .unwrap()
                .matrix,
            baseline.runs[1].matrix
        );
        // These characters did not exist in the first saved subset.
        let next = edit(
            &reopened,
            "\u{66f4}\u{6362}\u{5185}\u{5bb9}",
            210.,
            22.,
            false,
        );
        write(&mut reopened, std::slice::from_ref(&next)).unwrap();
        assert_eq!(scan(&reopened, 0).unwrap().runs[0].text, next.replacement);
    }
}

#[test]
fn cjk_program_cache_distinguishes_glyph_sets_and_reuses_identical_sets() {
    let mut doc = tests::with_content(
        b"BT /F1 12 Tf 40 180 Td (FIRST) Tj 0 -60 Td (SECOND) Tj 0 -60 Td (THIRD) Tj ET",
    );
    let page = scan(&doc, 0).unwrap();
    let changes: Vec<_> = page
        .runs
        .iter()
        .zip(["\u{4e2d}\u{6587}", "\u{65e5}\u{672c}", "\u{6587}\u{4e2d}"])
        .map(|(run, replacement)| Change {
            page: 0,
            revision: page.revision.clone(),
            operator: run.operator,
            original: run.text.clone(),
            replacement: replacement.into(),
            layout: Some(Layout {
                width: 60.,
                height: 20.,
                size: 12.,
                wrap: false,
                font: EditFont::Auto,
            }),
        })
        .collect();
    write(&mut doc, &changes).unwrap();
    let after = scan(&doc, 0).unwrap();
    assert_eq!(
        after
            .runs
            .iter()
            .filter(|r| !r.text.is_empty())
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        changes
            .iter()
            .map(|c| c.replacement.as_str())
            .collect::<Vec<_>>()
    );
    let programs: BTreeSet<_> = doc
        .objects
        .values()
        .filter_map(|v| v.as_dict().ok())
        .filter_map(|d| d.get(b"FontFile2").ok()?.as_reference().ok())
        .collect();
    assert_eq!(programs.len(), 2);
    for id in programs {
        let stream = doc.get_object(id).unwrap().as_stream().unwrap();
        let face = ttf_parser::Face::parse(&stream.content, 0).unwrap();
        assert!(face.number_of_glyphs() < 16);
        assert_eq!(
            face.raw_face()
                .table(ttf_parser::Tag::from_bytes(b"OS/2"))
                .unwrap()[8..10],
            [0, 0]
        );
    }
}

#[test]
fn cjk_missing_glyphs_refuse_atomically_and_latin_style_stays_selected() {
    assert_eq!(fonts::fallback::automatic(3, "ACME \u{3a9}").unwrap(), 3);
    assert_eq!(fonts::fallback::automatic(3, "\u{4e2d}").unwrap(), 5);
    let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj ET");
    let objects = doc.objects.clone();
    for text in ["\u{10ffff}", "\u{1f9ea}", "\u{4e2d}\u{fe0f}"] {
        let change = edit(&doc, text, 100., 20., false);
        assert!(write(&mut doc, &[change]).is_err());
        assert_eq!(doc.objects, objects);
    }
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

// A replacement keeps the source's render mode, so stroked text reaches half
// the line width beyond its outlines; a clip at the text origin cuts that off.
#[test]
fn stroked_layouts_reserve_half_the_line_width() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj ET");
    let mut first = edit(&doc, "ACME", 80., 20., false);
    first.layout.as_mut().unwrap().font = EditFont::NotoSans;
    write(&mut doc, &[first]).unwrap();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let content = page_content(&doc, id).unwrap();
    for (state, accepted) in [("0 Tr", true), ("4 w 2 Tr", false), ("4 w 3 Tr", true)] {
        let mut copy = doc.clone();
        let clipped = [
            format!("q 40 100 200 130 re W n {state}\n").as_bytes(),
            &content,
            b"\nQ",
        ]
        .concat();
        let stream = copy.add_object(Stream::new(Dictionary::new(), clipped));
        copy.get_dictionary_mut(id).unwrap().set("Contents", stream);
        // Longer than the source: its own text stays within the source's ink,
        // which is clipped exactly as before, and is not held to the clip.
        let change = edit(&copy, "ACMEE", 80., 20., false);
        let before = copy.objects.clone();
        let result = write(&mut copy, &[change]);
        assert_eq!(result.is_ok(), accepted, "{state}: {result:?}");
        if !accepted {
            assert!(result.unwrap_err().contains("clips"));
            assert_eq!(copy.objects, before);
        }
    }
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

// A wrapped line keeps the space it broke at. `line_breaks` fits the line by its
// words, so the ink check must not count that space either: here the box is half
// a point wider than "ACME FIRST", less than a space.
#[test]
fn wrapping_fits_a_line_by_its_words_not_the_space_it_broke_at() {
    let mut doc =
        tests::with_content(b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 0 -100 Td (SECOND) Tj ET");
    let words = fonts::Metrics::helvetica()
        .advance("ACME FIRST", 12.)
        .unwrap();
    let change = edit(&doc, "ACME FIRST SECOND", words + 0.5, 65., true);
    assert_eq!(preview_layout(&doc, &change).unwrap().lines, 2);
    write(&mut doc, &[change]).unwrap();
    let texts: String = scan(&doc, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|r| r.matrix[5] > 80.)
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(texts, "ACME FIRST SECOND");
}

// The layout `defaultTextLayout` (src/lib/textlayout.ts) sends when a reader
// starts typing, in the same arithmetic: the run's own advance, its size
// rounded up to the next thousandth of a point.
fn default_layout(run: &Run) -> Layout {
    let x = run.matrix[0].hypot(run.matrix[1]);
    let y = run.matrix[2].hypot(run.matrix[3]);
    let round = |value: f64| (value * 1000.).ceil() / 1000.;
    let source = run.size * y;
    let size = round(source);
    let height = (source * 1.25).max(run.minimum_height.unwrap_or(0.)) * size / source;
    Layout {
        width: round(run.advance * x).max(0.1),
        height: round(height).max(0.1),
        size,
        wrap: false,
        font: EditFont::Auto,
    }
}

fn in_default_box(doc: &Document, index: usize, replacement: &str) -> Change {
    let page = scan(doc, 0).unwrap();
    let run = &page.runs[index];
    Change {
        page: 0,
        revision: page.revision.clone(),
        operator: run.operator,
        original: run.text.clone(),
        replacement: replacement.into(),
        layout: Some(default_layout(run)),
    }
}

// Every show the saved page makes, in order, with its operand.
fn shows(doc: &Document) -> Vec<Object> {
    let page = crate::pagetree::ordered_pages(doc)[0];
    Content::decode_strict(&doc.get_page_content(page))
        .unwrap()
        .operations
        .into_iter()
        .filter(|op| op.operator == "TJ" || op.operator == "Tj")
        .map(|op| op.operands[0].clone())
        .collect()
}

fn kerned(items: &[(&str, i64)]) -> Object {
    let mut array = Vec::new();
    for (text, kern) in items {
        array.push(Object::string_literal(*text));
        if *kern != 0 {
            array.push(Object::Integer(*kern));
        }
    }
    Object::Array(array)
}

// A producer that tightened a run with kerning (pdfTeX, Word, Acrobat) gives it
// an advance shorter than its glyph widths. The editor's box is that advance, so
// laying the text out again from plain widths refused the run's own text.
#[test]
fn a_kerned_run_fits_its_own_default_box_unchanged_and_keeps_its_kerns() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 40 Td [(KER) 80 (NED) 80 (RUN)] TJ ET");
    let change = in_default_box(&doc, 0, "KERNEDRUN");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(
        shows(&doc)[0],
        kerned(&[("KER", 80), ("NED", 80), ("RUN", 0)])
    );
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "KERNEDRUN");
}

// Two letters swapped inside a kern-free stretch: the replacement is exactly as
// wide as the source only if both kerns survive, and nothing after it moves.
#[test]
fn a_same_length_edit_in_the_default_box_keeps_kerns_and_leaves_neighbours() {
    let mut doc = tests::with_content(
        b"BT /F1 12 Tf 20 100 Td [(ABCDEF) 80 (GH) 80 (IJ)] TJ (NEXT) Tj 0 -40 Td (BELOW) Tj ET",
    );
    let before = scan(&doc, 0).unwrap();
    let change = in_default_box(&doc, 0, "ABDCEFGHIJ");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(
        shows(&doc)[0],
        kerned(&[("ABDCEF", 80), ("GH", 80), ("IJ", 0)])
    );
    let after = scan(&doc, 0).unwrap();
    let edited = after.runs.iter().find(|r| r.text == "ABDCEFGHIJ").unwrap();
    assert_eq!(edited.matrix, before.runs[0].matrix);
    assert!((edited.advance - before.runs[0].advance).abs() < 1e-9);
    for text in ["NEXT", "BELOW"] {
        let old = before.runs.iter().find(|r| r.text == text).unwrap();
        let new = after.runs.iter().find(|r| r.text == text).unwrap();
        assert_eq!(new.matrix, old.matrix, "{text}");
        assert_eq!(new.display_rect, old.display_rect, "{text}");
    }
}

// The box's size control shows a thousandth of a point, rounded up, so a TeX
// 9.96264 pt run arrives as 9.963 and a Word run at Tf 1 under an 11.0417 scale as
// 11.042. Laid out at that size the run's own text is wider than its own
// advance, kerned or not.
#[test]
fn a_run_keeps_its_own_font_size_through_the_rounded_default_box() {
    for (content, size) in [
        (
            &b"BT /F1 9.96264 Tf 20 100 Td (PLAIN UNKERNED LINE OF TEXT) Tj ET"[..],
            9.96264_f32,
        ),
        (
            &b"BT /F1 1 Tf 11.0417 0 0 11.0417 20 100 Tm (PLAIN UNKERNED LINE OF TEXT) Tj ET"[..],
            1.,
        ),
    ] {
        let mut doc = tests::with_content(content);
        let before = scan(&doc, 0).unwrap();
        let change = in_default_box(&doc, 0, "PLAIN UNKERNED LINE OF TEXT");
        write(&mut doc, &[change]).unwrap();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let written = Content::decode_strict(&doc.get_page_content(page)).unwrap();
        // The source's own Tf comes first; the edit's is the second.
        let font = written
            .operations
            .iter()
            .filter(|op| op.operator == "Tf")
            .nth(1)
            .unwrap();
        assert_eq!(font.operands[1].as_float().unwrap(), size);
        let after = scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].matrix, before.runs[0].matrix);
        assert_eq!(after.runs[0].size, before.runs[0].size);
    }
}

// A size the reader chose is not the source's, so the run is laid out again
// from glyph widths; the kerns belonged to the old size.
#[test]
fn a_kerned_run_at_another_size_is_laid_out_again_without_its_kerns() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td [(KER) 80 (NED) 80 (RUN)] TJ ET");
    let mut change = in_default_box(&doc, 0, "KERNEDRUN");
    let layout = change.layout.as_mut().unwrap();
    layout.size = 14.;
    layout.width = 200.;
    layout.height = 20.;
    write(&mut doc, &[change]).unwrap();
    assert_eq!(shows(&doc)[0], Object::string_literal("KERNEDRUN"));
}

// A bundled font cannot show the source's character codes, so choosing one
// always lays the run out again in that font.
#[test]
fn a_kerned_run_in_a_chosen_bundled_font_does_not_keep_source_codes() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td [(KER) 80 (NED) 80 (RUN)] TJ ET");
    let mut change = in_default_box(&doc, 0, "KERNEDRUN");
    let layout = change.layout.as_mut().unwrap();
    layout.font = EditFont::NotoSans;
    layout.width = 200.;
    assert_eq!(preview_layout(&doc, &change).unwrap().font, "Noto Sans");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "KERNEDRUN");
    assert_ne!(
        shows(&doc)[0],
        kerned(&[("KER", 80), ("NED", 80), ("RUN", 0)])
    );
}

// A kept kern can widen: the 1000-unit gap after A leaves no room in the box
// for W, while the rewrite without it fits, so the rewrite is written.
#[test]
fn kept_kerns_that_do_not_fit_the_box_give_way_to_the_rewrite() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td [(A) -1000 (BC)] TJ ET");
    let source = doc.clone();
    let change = in_default_box(&doc, 0, "ABW");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(shows(&doc)[0], Object::string_literal("ABW"));
    // A box narrowed below the source's advance, though still wider than its
    // ink, cannot hold the source's own gap: the run is set without it.
    let mut doc = source;
    let mut change = in_default_box(&doc, 0, "ABC");
    change.layout.as_mut().unwrap().width -= 1.;
    write(&mut doc, &[change]).unwrap();
    assert_eq!(shows(&doc)[0], Object::string_literal("ABC"));
}

// The size control's step is a thousandth of a point, either way; beyond it the
// reader asked for another size.
#[test]
fn a_requested_size_within_one_step_of_the_source_is_the_source_size() {
    for (requested, written) in [
        (12.001_000_000_000_5, 12.),
        (11.999_5, 12.),
        (12.001_1, 12.001_1_f32),
        (11.998_9, 11.998_9),
    ] {
        let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td (SIZE) Tj ET");
        let mut change = in_default_box(&doc, 0, "SIZE");
        let layout = change.layout.as_mut().unwrap();
        layout.size = requested;
        layout.width = 100.;
        layout.height = 20.;
        write(&mut doc, &[change]).unwrap();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let content = Content::decode_strict(&doc.get_page_content(page)).unwrap();
        let font = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tf")
            .nth(1)
            .unwrap();
        assert_eq!(font.operands[1].as_float().unwrap(), written, "{requested}");
    }
}
