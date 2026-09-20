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
            grow: false,
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
                grow: false,
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
                grow: false,
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
// rounded up to the next thousandth of a point, and `grow` set, because the
// reader has not touched the width control yet.
pub(super) fn default_layout(run: &Run) -> Layout {
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
        grow: true,
    }
}

pub(super) fn in_default_box(doc: &Document, index: usize, replacement: &str) -> Change {
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
pub(super) fn shows(doc: &Document) -> Vec<Object> {
    let page = crate::pagetree::ordered_pages(doc)[0];
    Content::decode_strict(&doc.get_page_content(page))
        .unwrap()
        .operations
        .into_iter()
        .filter(|op| op.operator == "TJ" || op.operator == "Tj")
        .map(|op| op.operands[0].clone())
        .collect()
}

pub(super) fn kerned(items: &[(&str, i64)]) -> Object {
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
//
// The box has to be one a reader sized (`grow` cleared), because that is what
// makes the box the limit. A box the editor opens follows the typed text into
// the room after the line, and on this page there is 280 pt of it, so the kept
// gap fits and is kept -- which is the point of growing it and is covered by
// `a_box_the_reader_has_not_sized_grows_into_the_free_space_after_the_line`.
#[test]
fn kept_kerns_that_do_not_fit_the_box_give_way_to_the_rewrite() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td [(A) -1000 (BC)] TJ ET");
    let source = doc.clone();
    let mut change = in_default_box(&doc, 0, "ABW");
    change.layout.as_mut().unwrap().grow = false;
    write(&mut doc, &[change]).unwrap();
    assert_eq!(shows(&doc)[0], Object::string_literal("ABW"));
    // A box narrowed below the source's advance, though still wider than its
    // ink, cannot hold the source's own gap: the run is set without it.
    let mut doc = source;
    let mut change = in_default_box(&doc, 0, "ABC");
    change.layout.as_mut().unwrap().width -= 1.;
    change.layout.as_mut().unwrap().grow = false;
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

// Where a reader puts each line, in its own arithmetic. PDFium keeps the text
// matrix and the line position apart, both in `float` (`CPDF_AllStates`:
// `text_matrix_`, `text_line_pos_`, `text_leading_`), adds each Td or TD to the
// position (`MoveTextPoint`), subtracts the leading for T* (`MoveTextToNextLine`),
// resets the position at BT and Tm, and places a show at
// `text_matrix_.Transform(pos)`. Each show that follows a line move is listed
// with the bits of the point its line starts at; a show that continues a line
// gets `None`, since its position also depends on the glyph advances before it.
pub(super) fn reader_line_starts(doc: &Document) -> Vec<(Object, Option<[u32; 2]>)> {
    let page = crate::pagetree::ordered_pages(doc)[0];
    let content = Content::decode_strict(&doc.get_page_content(page)).unwrap();
    let identity = [1_f32, 0., 0., 1., 0., 0.];
    let (mut matrix, mut line, mut leading) = (identity, [0_f32; 2], 0_f32);
    let mut moved = false;
    let mut starts = Vec::new();
    for op in content.operations {
        let n: Vec<f32> = op
            .operands
            .iter()
            .filter_map(|o| o.as_float().ok())
            .collect();
        match op.operator.as_str() {
            "BT" => (matrix, line, moved) = (identity, [0., 0.], true),
            "Tm" => {
                matrix.copy_from_slice(&n);
                (line, moved) = ([0., 0.], true);
            }
            "Td" | "TD" => {
                (line[0], line[1], moved) = (line[0] + n[0], line[1] + n[1], true);
                if op.operator == "TD" {
                    leading = -n[1];
                }
            }
            "T*" => (line[1], moved) = (line[1] - leading, true),
            "TL" => leading = n[0],
            "Tj" | "TJ" => {
                let start = [
                    matrix[0] * line[0] + matrix[2] * line[1] + matrix[4],
                    matrix[1] * line[0] + matrix[3] * line[1] + matrix[5],
                ];
                starts.push((
                    op.operands[0].clone(),
                    moved.then(|| start.map(f32::to_bits)),
                ));
                moved = false;
            }
            _ => {}
        }
    }
    starts
}

// Edits `target` in the editor's default box, swapping its first two letters,
// and requires every show after it that starts a line to start it on exactly
// the same bits as in the source.
fn following_lines_are_identical(content: &str, target: &str, followers: &[&str]) {
    let mut doc = tests::with_content(content.as_bytes());
    let before = reader_line_starts(&doc);
    let index = scan(&doc, 0)
        .unwrap()
        .runs
        .iter()
        .position(|run| run.text == target)
        .unwrap_or_else(|| panic!("{content}: no run {target}"));
    let mut swapped: Vec<char> = target.chars().collect();
    swapped.swap(0, 1);
    let replacement: String = swapped.into_iter().collect();
    let change = in_default_box(&doc, index, &replacement);
    write(&mut doc, &[change]).unwrap_or_else(|error| panic!("{content}: {error}"));
    let after = reader_line_starts(&doc);
    assert_eq!(scan(&doc, 0).unwrap().runs[index].text, replacement);
    for text in followers {
        let find = |starts: &[(Object, Option<[u32; 2]>)]| {
            let found: Vec<_> = starts
                .iter()
                .filter(|(operand, _)| *operand == Object::string_literal(*text))
                .map(|(_, start)| start.expect("a follower starts a line"))
                .collect();
            assert_eq!(found.len(), 1, "{content}: {text}");
            found[0]
        };
        assert_eq!(find(&after), find(&before), "{content}: {text}");
    }
}

// A line restored with one Tm is the same point in exact arithmetic and a
// different one in floats: the source's lines after `200 0 Td ... -200 -30 Td`
// start at 20, where the restored matrix put them at 19.999992, a pixel to the
// left wherever that crosses a rounding boundary. The restore replays the
// source's own Tm and line moves, so every reader repeats the same additions:
// across TD, T*, a leading changed before and after the edit, a line continued
// past it, a Tm in the block, and quarter-turned text. The leading a T* before
// the edit read was set before the block, so only the restore can put it back.
#[test]
fn a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it() {
    let tail = "200 0 Td (AWAY) Tj -200 -30 Td (SECOND) Tj 0 -14 TD (THIRD) Tj \
                T* (FOURTH) Tj 20 TL T* (FIFTH) Tj ET";
    for content in [
        format!("BT /F1 12 Tf 19.999992 200 Td (FIRST LINE) Tj {tail}"),
        format!("BT /F1 12 Tf 19.999992 200 Td (FIRST LINE) Tj (MORE) Tj {tail}"),
        format!("14 TL BT /F1 12 Tf 19.999992 214 Td T* 9 TL (FIRST LINE) Tj {tail}"),
        format!("BT /F1 12 Tf 1 0 0 1 9.999992 150 Tm 10 50 Td (FIRST LINE) Tj {tail}"),
    ] {
        following_lines_are_identical(
            &content,
            "FIRST LINE",
            &["AWAY", "SECOND", "THIRD", "FOURTH", "FIFTH"],
        );
    }
    let turned = "BT /F1 12 Tf 0 1 -1 0 250 20 Tm 19.999992 0 Td (FIRST LINE) Tj \
                  200 0 Td (AWAY) Tj -200 -30 Td (SECOND) Tj 14 TL T* (THIRD) Tj ET";
    following_lines_are_identical(turned, "FIRST LINE", &["AWAY", "SECOND", "THIRD"]);
    // An edit after the hop, and one on a later line of the chain.
    let chain = format!("BT /F1 12 Tf 19.999992 200 Td (FIRST LINE) Tj {tail}");
    following_lines_are_identical(&chain, "AWAY", &["SECOND", "THIRD", "FOURTH", "FIFTH"]);
    following_lines_are_identical(&chain, "THIRD", &["FOURTH", "FIFTH"]);
}

// ---------------------------------------------------------------------------
// The room after a run, and the box that follows the text into it.
//
// `layout::room` is pure geometry over displayed-page rectangles, so the rule
// it implements is stated here in numbers rather than inferred from a document:
// the box grows along the run's own text axis until it meets the first thing on
// its line, and no further than the page's edge or the clip in force. The tests
// below it run the same rule through the writer on real pages.
// ---------------------------------------------------------------------------

// The box of a run whose leading edge sits `lead` along the growth axis, and
// one unit wider, in each of the four directions a quarter-turned run grows in.
// Written out four times rather than derived from the first, because a second
// hand-written table of turns is the thing `text::to_device` warns about: these
// are four independent statements the one implementation has to satisfy.
fn box_edges(direction: usize, lead: f64) -> ([f64; 4], [f64; 4], [f64; 2]) {
    match direction {
        0 => (
            [lead, 48., lead, 63.],
            [lead, 48., lead + 1., 63.],
            [300., 240.],
        ),
        1 => (
            [lead, 48., lead, 63.],
            [lead - 1., 48., lead, 63.],
            [300., 240.],
        ),
        2 => (
            [48., lead, 63., lead],
            [48., lead, 63., lead + 1.],
            [240., 300.],
        ),
        _ => (
            [48., lead, 63., lead],
            [48., lead - 1., 63., lead],
            [240., 300.],
        ),
    }
}

// The run's own hit rectangle, reaching 80 pt back from its leading edge, and a
// neighbour whose near edge is `gap` further on, both in the same direction.
fn own_and_neighbour(direction: usize, lead: f64, gap: f64) -> ([f64; 4], [f64; 4]) {
    match direction {
        0 => (
            [lead - 80., 48., lead, 63.],
            [lead + gap, 48., lead + gap + 40., 63.],
        ),
        1 => (
            [lead, 48., lead + 80., 63.],
            [lead - gap - 40., 48., lead - gap, 63.],
        ),
        2 => (
            [48., lead - 80., 63., lead],
            [48., lead + gap, 63., lead + gap + 40.],
        ),
        _ => (
            [48., lead, 63., lead + 80.],
            [48., lead - gap - 40., 63., lead - gap],
        ),
    }
}

#[test]
fn the_room_after_a_run_reaches_the_page_edge_and_stops_short_of_a_neighbour() {
    for direction in 0..4 {
        // Directions 1 and 3 grow towards zero, so their leading edge starts at
        // the far end and has the same 200 pt of page in front of it.
        let lead = if direction % 2 == 1 { 200. } else { 100. };
        let (zero, one, page) = box_edges(direction, lead);
        let (own, neighbour) = own_and_neighbour(direction, lead, 40.);
        assert_eq!(
            layout::room((zero, one), 1., page, None, own, [].into_iter(), 0.),
            (200., layout::Room::Page),
            "direction {direction}, nothing in the way"
        );
        assert_eq!(
            layout::room(
                (zero, one),
                1.,
                page,
                None,
                own,
                [neighbour].into_iter(),
                0.
            ),
            (40., layout::Room::Line),
            "direction {direction}, a neighbour 40 pt on"
        );
    }
}

#[test]
fn the_room_after_a_run_stops_two_points_short_of_a_close_neighbour() {
    let (zero, one, page) = box_edges(0, 100.);
    let (own, neighbour) = own_and_neighbour(0, 100., 2.);
    assert_eq!(
        layout::room(
            (zero, one),
            1.,
            page,
            None,
            own,
            [neighbour].into_iter(),
            0.
        ),
        (2., layout::Room::Line)
    );
}

#[test]
fn the_room_after_a_run_stops_at_a_clip_and_a_nearer_neighbour_wins() {
    let (zero, one, page) = box_edges(0, 100.);
    let (own, neighbour) = own_and_neighbour(0, 100., 40.);
    let clip = Some([0., 40., 180., 70.]);
    assert_eq!(
        layout::room((zero, one), 1., page, clip, own, [].into_iter(), 0.),
        (80., layout::Room::Clip)
    );
    assert_eq!(
        layout::room(
            (zero, one),
            1.,
            page,
            clip,
            own,
            [neighbour].into_iter(),
            0.
        ),
        (40., layout::Room::Line)
    );
}

#[test]
fn the_room_after_a_run_ignores_what_is_not_on_its_line() {
    let (zero, one, page) = box_edges(0, 100.);
    let (own, _) = own_and_neighbour(0, 100., 40.);
    // Below the line entirely, and overlapping it by less than the tenth of a
    // point the collision check ignores.
    for other in [[140., 70., 180., 90.], [140., 62.95, 180., 90.]] {
        assert_eq!(
            layout::room((zero, one), 1., page, None, own, [other].into_iter(), 0.),
            (200., layout::Room::Page),
            "{other:?}"
        );
    }
    assert_eq!(
        layout::room(
            (zero, one),
            1.,
            page,
            None,
            own,
            [[140., 62.5, 180., 90.]].into_iter(),
            0.
        ),
        (40., layout::Room::Line)
    );
}

#[test]
fn the_room_after_a_run_is_never_less_than_the_box_it_was_given() {
    let (zero, one, page) = box_edges(0, 100.);
    let (own, neighbour) = own_and_neighbour(0, 100., 40.);
    // A box already past the page edge keeps its width; growth only ever adds.
    assert_eq!(
        layout::room((zero, one), 1., page, None, own, [].into_iter(), 250.).0,
        250.
    );
    // A neighbour already inside the box does not pull the box back over it.
    assert_eq!(
        layout::room(
            (zero, one),
            1.,
            page,
            None,
            own,
            [neighbour].into_iter(),
            60.
        ),
        (200., layout::Room::Page)
    );
}

// Every glyph of the synthetic font is 600/1000 wide, so a character at 12 pt
// is exactly 7.2 pt and the arithmetic below is readable. The page is 300 x 240.
pub(super) fn synthetic(body: &str) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    doc
}

// Built from the run's own characters, as the growth instrument builds its
// longer trials, so a missing glyph can never be the reason for a refusal.
const LONGER: &str = "FIRSTFIRSTFIRSTFIRS"; // 19 glyphs, 136.8 pt

#[test]
fn a_box_the_reader_has_not_sized_grows_into_the_free_space_after_the_line() {
    // FIRST occupies 36 pt from x 40; the page leaves 260 pt after its origin.
    let mut doc = synthetic("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let change = in_default_box(&doc, 0, LONGER);
    let mut sized = change.clone();
    sized.layout.as_mut().unwrap().grow = false;
    let refusal = write(&mut doc.clone(), &[sized]).unwrap_err();
    assert!(refusal.contains("exceeds the box width"), "{refusal}");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, LONGER);
}

// The neighbour here is read-only, and since 26.9.15 that is what makes this a
// statement about the box at all: a neighbour the writer may rewrite is pushed
// along the line instead of stopping the box, which `push_tests` covers. What
// is left for the box is the text it cannot move.
#[test]
fn a_grown_box_stops_two_points_short_of_text_it_cannot_move() {
    // The first FIRST ends at 76 and the second starts at 78: two points of
    // room. Both runs are the same word because the synthetic font has no
    // validated glyph for every code it declares, and a run it cannot show is
    // not discovered at all; the second draws back over itself, which keeps it
    // read-only and so immovable.
    let crowded = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 78 180 Td [(FIRST) 3000 (F)] TJ ET",
    );
    assert_eq!(scan(&crowded, 0).unwrap().runs.len(), 1);
    let refusal = write(&mut crowded.clone(), &[in_default_box(&crowded, 0, LONGER)]).unwrap_err();
    assert!(refusal.contains("other text follows it"), "{refusal}");
    // The run's own text still fits the box it always had.
    write(
        &mut crowded.clone(),
        &[in_default_box(&crowded, 0, "FIRST")],
    )
    .unwrap();
    // Moving that neighbour away accepts the same edit, so the refusal above is
    // the neighbour rather than the length.
    let mut roomy = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 240 180 Td [(FIRST) 3000 (F)] TJ ET",
    );
    let change = in_default_box(&roomy, 0, LONGER);
    write(&mut roomy, &[change]).unwrap();
    assert_eq!(scan(&roomy, 0).unwrap().runs[0].text, LONGER);
}

#[test]
fn a_grown_box_stops_at_the_edge_of_the_page() {
    // FIRST ends at 296 on a 300 pt page: four points of room.
    let edge = synthetic("BT /F1 12 Tf 260 180 Td (FIRST) Tj ET");
    let refusal = write(&mut edge.clone(), &[in_default_box(&edge, 0, LONGER)]).unwrap_err();
    assert!(
        refusal.contains("reaches the edge of the page"),
        "{refusal}"
    );
    let mut inland = synthetic("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let change = in_default_box(&inland, 0, LONGER);
    write(&mut inland, &[change]).unwrap();
}

// Growth is to the right of the run's own origin only: the writer places a
// replacement where the source put it and replays the source's own positioning
// to get there, so starting the line further left is reflow rather than a wider
// box. A line set flush against the right edge has a whole empty page to its
// left and still does not grow.
#[test]
fn a_line_against_the_right_edge_does_not_grow_into_the_space_on_its_left() {
    let right = synthetic("BT /F1 12 Tf 264 180 Td (FIRST) Tj ET");
    let before = scan(&right, 0).unwrap();
    assert_eq!(before.runs[0].display_rect[2], 300.);
    let refusal = write(&mut right.clone(), &[in_default_box(&right, 0, "FIRSTF")]).unwrap_err();
    assert!(
        refusal.contains("reaches the edge of the page"),
        "{refusal}"
    );
    // One glyph shorter is accepted, so the run itself is editable.
    let mut shorter = right.clone();
    let change = in_default_box(&shorter, 0, "FIRS");
    write(&mut shorter, &[change]).unwrap();
}

#[test]
fn a_grown_box_stops_at_a_clip_the_document_has_in_force() {
    // The clip reaches x 138, so the line has 98 pt of room from x 40.
    let clipped = synthetic("q 38 170 100 30 re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET Q");
    let refusal = write(&mut clipped.clone(), &[in_default_box(&clipped, 0, LONGER)]).unwrap_err();
    assert!(refusal.contains("clips the space after it"), "{refusal}");
    // Twelve glyphs are 86.4 pt and fit inside the same clip.
    let mut fits = clipped.clone();
    let change = in_default_box(&fits, 0, "FIRSTFIRSTFI");
    write(&mut fits, &[change]).unwrap();
    assert_eq!(scan(&fits, 0).unwrap().runs[0].text, "FIRSTFIRSTFI");
}

#[test]
fn the_box_reported_back_is_the_size_of_the_text_not_of_the_room_it_had() {
    let doc = synthetic("BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let grown = preview_layout(&doc, &in_default_box(&doc, 0, LONGER)).unwrap();
    let width = f64::from(grown.rect[2] - grown.rect[0]);
    assert!((width - 136.8).abs() < 0.01, "{width} for {LONGER}");
    // Text that needs no room keeps the box it was opened with, so the outline
    // a reader sees is their text and not the whole line.
    let same = preview_layout(&doc, &in_default_box(&doc, 0, "FIRST")).unwrap();
    assert!(
        (f64::from(same.rect[2] - same.rect[0]) - 36.).abs() < 0.01,
        "{:?}",
        same.rect
    );
}

// A replacement the original font cannot encode goes through the bundled
// fallback, which lays the line out from glyph widths rather than keeping the
// source's items. That path grows too.
#[test]
fn a_fallback_replacement_grows_into_the_free_space_as_well() {
    // Helvetica at x 40 in a 300 pt page: 106.008 pt of text, 260 pt of room.
    let mut doc = tests::with_content(b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET");
    let longer = "SYNTHETIC FIRST AND \u{3a9}";
    let change = in_default_box(&doc, 0, longer);
    let mut sized = change.clone();
    sized.layout.as_mut().unwrap().grow = false;
    let refusal = write(&mut doc.clone(), &[sized]).unwrap_err();
    assert!(refusal.contains("the box"), "{refusal}");
    assert_eq!(preview_layout(&doc, &change).unwrap().font, "Noto Sans");
    write(&mut doc, &[change]).unwrap();
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, longer);
}

// A compound clip is a set of rectangles with holes rather than one edge, so
// the box growth would produce is handed to the region instead of being reduced
// to a coordinate. The region here ends at x 240 while the page runs to 300, so
// the box the page alone would allow is refused and growth is given up.
#[test]
fn a_run_under_a_compound_clip_gives_growth_up_rather_than_guessing_an_edge() {
    let clipped = synthetic(
        "40 170 m 240 170 l 240 200 l 40 200 l h W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET",
    );
    assert_eq!(scan(&clipped, 0).unwrap().runs.len(), 1);
    let refusal = write(&mut clipped.clone(), &[in_default_box(&clipped, 0, LONGER)]).unwrap_err();
    assert!(refusal.contains("clips the space after it"), "{refusal}");
    // The run is still editable inside the box it always had.
    let mut shorter = clipped.clone();
    let change = in_default_box(&shorter, 0, "FIRS");
    write(&mut shorter, &[change]).unwrap();
    assert_eq!(scan(&shorter, 0).unwrap().runs[0].text, "FIRS");
}

// The cross-axis span growth is measured over is the box's own together with
// the run's hit rectangle: a descender or an accent takes a run's glyphs past
// the box the editor opened, and something beside them is still on this line.
#[test]
fn the_room_after_a_run_counts_a_neighbour_its_own_glyphs_reach() {
    let (zero, one, page) = box_edges(0, 100.);
    for (own, other) in [
        // Reaching below the box, and a neighbour only beside that reach.
        ([20., 48., 100., 70.], [140., 64., 180., 90.]),
        // And above it.
        ([20., 30., 100., 63.], [140., 20., 180., 47.]),
    ] {
        assert_eq!(
            layout::room((zero, one), 1., page, None, own, [other].into_iter(), 0.),
            (40., layout::Room::Line),
            "{own:?} beside {other:?}"
        );
        // The same neighbour beside a run that keeps to its box is not on the line.
        assert_eq!(
            layout::room(
                (zero, one),
                1.,
                page,
                None,
                [20., 48., 100., 63.],
                [other].into_iter(),
                0.
            ),
            (200., layout::Room::Page),
            "{other:?}"
        );
    }
}

// Growing the box hands the wider ceiling to the source's own items as well, so
// a producer's kerning survives an edit that needed the room -- laying the run
// out again from glyph widths is what the box was widened to avoid.
#[test]
fn a_kerned_run_grown_into_the_free_space_keeps_the_kerns_it_had() {
    let mut doc = tests::with_content(b"BT /F1 12 Tf 20 100 Td [(KER) 80 (NED) 80 (RUN)] TJ ET");
    let change = in_default_box(&doc, 0, "KERNEDRUNNING");
    let mut sized = change.clone();
    sized.layout.as_mut().unwrap().grow = false;
    assert!(write(&mut doc.clone(), &[sized]).is_err());
    write(&mut doc, &[change]).unwrap();
    assert_eq!(
        shows(&doc)[0],
        kerned(&[("KER", 80), ("NED", 80), ("RUNNING", 0)])
    );
    assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "KERNEDRUNNING");
}
