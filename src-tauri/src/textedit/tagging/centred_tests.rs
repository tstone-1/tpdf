// A centred line is edited about its own centre: the replacement starts half
// the change in width earlier, and only a run that is its whole line is offered.
use super::tests::fixture;
use crate::textedit::{self, Change, EditFont, Layout, Run};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};

const LINES: &[u8] = b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 80 140 Td (SECOND) Tj ET EMC";

fn align(doc: &mut Document, id: ObjectId, value: &str) {
    doc.get_dictionary_mut(id).unwrap().set(
        "A",
        dictionary! { "O" => "Layout", "Placement" => "Block", "TextAlign" => value },
    );
}

fn runs(doc: &Document) -> Vec<Run> {
    textedit::scan(doc, 0).unwrap().runs
}

fn change(doc: &Document, original: &str, replacement: &str, layout: bool) -> Change {
    let scan = textedit::scan(doc, 0).unwrap();
    let run = scan.runs.iter().find(|run| run.text == original).unwrap();
    Change {
        layout: layout.then(|| Layout::opened(run, EditFont::Original)),
        page: 0,
        revision: scan.revision,
        operator: run.operator,
        original: original.into(),
        replacement: replacement.into(),
    }
}

// Where the run with this text starts and how far it runs, after the edit.
fn span(doc: &Document, text: &str) -> (f64, f64) {
    let run = runs(doc).into_iter().find(|run| run.text == text).unwrap();
    (run.matrix[4], run.advance * run.matrix[0])
}

fn edited(alignment: &str, replacement: &str) -> Document {
    let (mut doc, ids) = fixture(LINES);
    align(&mut doc, ids[3], alignment);
    let change = change(&doc, "FIRST", replacement, true);
    textedit::write(&mut doc, &[change]).unwrap();
    doc
}

#[test]
fn textedit_a_centred_line_is_edited_about_its_centre() {
    let (doc, _) = fixture(LINES);
    let (start, width) = span(&doc, "FIRST");
    for replacement in ["FIRSTFIRST", "IF"] {
        // Control: a start-aligned line keeps its origin.
        let (kept, wide) = span(&edited("Start", replacement), replacement);
        assert_eq!(kept, start, "{replacement}");
        assert!((wide - width).abs() > 5., "{replacement}");
        // Centred: the middle of the line is where it was.
        let (moved, wide) = span(&edited("Center", replacement), replacement);
        assert!(
            (moved + wide / 2. - (start + width / 2.)).abs() < 0.01,
            "{replacement}: {moved} {wide}"
        );
        assert!((moved - start).abs() > 2., "{replacement}");
    }
    // The other line and the tag are untouched.
    let doc = edited("Center", "FIRSTFIRST");
    assert_eq!(span(&doc, "SECOND").0, 80.);
    // The box the editor outlines: a centred line's box has the line's middle,
    // also when the text is shorter than the box, and a start-aligned line's
    // box starts where the line does.
    for replacement in ["FIRSTFIRST", "IF"] {
        let outline = |alignment: &str| {
            let (mut doc, ids) = fixture(LINES);
            align(&mut doc, ids[3], alignment);
            let source = runs(&doc)
                .into_iter()
                .find(|run| run.text == "FIRST")
                .unwrap();
            let change = change(&doc, "FIRST", replacement, true);
            let placed = textedit::placements(&doc, 0, &[change]).unwrap();
            (source.display_rect, placed[&source.operator])
        };
        let middle = |rect: [f32; 4]| (rect[0] + rect[2]) / 2.;
        let (source, boxed) = outline("Center");
        assert!(
            (middle(boxed) - middle(source)).abs() < 0.01,
            "{replacement}: {source:?} {boxed:?}"
        );
        assert!(
            boxed[2] - boxed[0] >= source[2] - source[0] - 0.01,
            "{replacement}"
        );
        let (source, boxed) = outline("Start");
        assert!((boxed[0] - source[0]).abs() < 0.01, "{replacement}");
    }
}

// The source line is measured as it is drawn, kerns and tracking included, and
// so is the replacement: its own text, set again with the source's own
// positioning, stays exactly where it was. Measured afresh from glyph widths,
// the source was the kerns narrower than the replacement that kept them, and
// the unchanged line moved back by half of that.
#[test]
fn textedit_a_kerned_centred_line_keeps_its_centre() {
    for line in [
        // Plain: the control, which never moved.
        &b"(FIRST) Tj"[..],
        // Kerned: 2.4 pt wider than its glyphs at 12 pt.
        b"[(FI) -200 (RST)] TJ",
        // Tracked: every glyph 0.6 pt apart.
        b"[(F) -50 (I) -50 (R) -50 (S) -50 (T)] TJ",
    ] {
        let content = [
            &b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td "[..],
            line,
            b" ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 80 140 Td (SECOND) Tj ET EMC",
        ]
        .concat();
        let (mut doc, ids) = fixture(&content);
        align(&mut doc, ids[3], "Center");
        let label = String::from_utf8_lossy(line).into_owned();
        let before = runs(&doc)
            .into_iter()
            .find(|run| run.text == "FIRST")
            .unwrap_or_else(|| panic!("{label}: not offered"));
        let same = change(&doc, "FIRST", "FIRST", true);
        textedit::write(&mut doc, &[same]).unwrap_or_else(|error| panic!("{label}: {error}"));
        let after = runs(&doc)
            .into_iter()
            .find(|run| run.text == "FIRST")
            .unwrap();
        assert!(
            (after.matrix[4] - before.matrix[4]).abs() < 0.0001,
            "{label}: {} against {}",
            after.matrix[4],
            before.matrix[4]
        );
        assert!(
            (after.advance - before.advance).abs() < 0.0001,
            "{label}: the kerns were not kept"
        );
        // A longer text is centred on the middle of the line as it was drawn.
        let (mut doc, ids) = fixture(&content);
        align(&mut doc, ids[3], "Center");
        let longer = change(&doc, "FIRST", "FIRSTFIRST", true);
        textedit::write(&mut doc, &[longer]).unwrap_or_else(|error| panic!("{label}: {error}"));
        let (start, width) = (before.matrix[4], before.advance * before.matrix[0]);
        let (moved, wide) = span(&doc, "FIRSTFIRST");
        assert!(
            (moved + wide / 2. - (start + width / 2.)).abs() < 0.01,
            "{label}: {moved} {wide} against {start} {width}"
        );
    }
    // A line ending in a space: the space draws nothing, in the source and in
    // the replacement alike, and its own text set again does not move; a
    // longer text is centred on the line's words.
    let spaced = b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td (FIRST ) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 80 140 Td (SECOND) Tj ET EMC";
    let (mut doc, ids) = fixture(spaced);
    align(&mut doc, ids[3], "Center");
    let before = span(&doc, "FIRST ");
    let same = change(&doc, "FIRST ", "FIRST ", true);
    textedit::write(&mut doc, &[same]).unwrap_or_else(|error| panic!("{error}"));
    let after = span(&doc, "FIRST ");
    assert!(
        (after.0 - before.0).abs() < 0.0001,
        "{after:?} against {before:?}"
    );
    // FIRST is 80..116, so its middle is 98, and FIRSTFIRST is 72 pt wide;
    // counted with its space, the line's middle was 101.6.
    let (mut doc, ids) = fixture(spaced);
    align(&mut doc, ids[3], "Center");
    let longer = change(&doc, "FIRST ", "FIRSTFIRST", true);
    textedit::write(&mut doc, &[longer]).unwrap_or_else(|error| panic!("{error}"));
    let (moved, _) = span(&doc, "FIRSTFIRST");
    assert!((moved - 62.).abs() < 0.01, "{moved}");
    // A kerned line whose last glyph reaches past its advance: `D` is
    // reshaped to reach 0.6 pt past its own. The kerns move that glyph 2.4 pt
    // along, so the line's ink ends at 39, not where the kerned advance does.
    // DFIRS shares neither end with it, so it is laid out afresh, 36 pt wide
    // with no overhang, and is centred on 80..119: it starts at 81.5.
    let (mut doc, ids) = fixture(b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td [(FI) -200 (RSD)] TJ ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 80 140 Td (SECOND) Tj ET EMC");
    let (_, _, _, program) = crate::textedit::fonts::tests::fixture();
    let stream = doc
        .get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = crate::textedit::fonts::ink_tests::with_components(
        stream.content.clone(),
        [('B', 0, 0, 16384), ('D', 250, 0, 16384)],
    );
    align(&mut doc, ids[3], "Center");
    let afresh = change(&doc, "FIRSD", "DFIRS", true);
    textedit::write(&mut doc, &[afresh]).unwrap_or_else(|error| panic!("{error}"));
    let (moved, _) = span(&doc, "DFIRS");
    assert!((moved - 81.5).abs() < 0.01, "{moved}");
}

#[test]
fn textedit_a_centred_line_is_offered_only_when_it_is_the_whole_line() {
    let offered = |doc: &Document| {
        runs(doc)
            .into_iter()
            .map(|run| run.text)
            .collect::<Vec<_>>()
    };
    let (mut doc, ids) = fixture(LINES);
    align(&mut doc, ids[3], "Center");
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    // Two runs on one line: the centred one is kept, the other is not its concern.
    let shared = b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 140 180 Td (SECOND) Tj ET EMC";
    let (mut doc, ids) = fixture(shared);
    assert_eq!(offered(&doc), ["FIRST", "SECOND"]);
    align(&mut doc, ids[3], "Center");
    assert_eq!(offered(&doc), ["SECOND"]);
    align(&mut doc, ids[4], "Center");
    assert!(textedit::scan(&doc, 0).is_err());
    // A blank run beside it is not text on the line.
    let blank = b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 80 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 140 180 Td ( ) Tj ET EMC";
    let (mut doc, ids) = fixture(blank);
    align(&mut doc, ids[3], "Center");
    assert_eq!(offered(&doc), ["FIRST", " "]);
    // End and Justify still pin.
    for alignment in ["End", "Justify"] {
        let (mut doc, ids) = fixture(LINES);
        align(&mut doc, ids[3], alignment);
        assert_eq!(offered(&doc), ["SECOND"], "{alignment}");
    }
}

#[test]
fn textedit_the_nearest_declared_alignment_decides() {
    let centre = |doc: &Document| {
        let (start, width) = span(doc, "FIRSTFIRST");
        start + width / 2.
    };
    let (plain, _) = fixture(LINES);
    let (start, width) = span(&plain, "FIRST");
    let middle = start + width / 2.;
    let write = |mut doc: Document| {
        let change = change(&doc, "FIRST", "FIRSTFIRST", true);
        textedit::write(&mut doc, &[change]).unwrap();
        doc
    };
    // Inherited from the Document element.
    let (mut doc, ids) = fixture(LINES);
    doc.get_dictionary_mut(ids[2]).unwrap().set(
        "A",
        dictionary! { "O" => "Layout", "TextAlign" => "Center" },
    );
    for id in [ids[3], ids[4]] {
        doc.get_dictionary_mut(id).unwrap().remove(b"A");
    }
    assert!((centre(&write(doc.clone())) - middle).abs() < 0.01);
    // Overridden by the paragraph's own Start.
    align(&mut doc, ids[3], "Start");
    assert_eq!(span(&write(doc), "FIRSTFIRST").0, start);
    // Declared through a class, and overridden by the element's own attributes.
    let classes: Dictionary =
        dictionary! { "Centred" => dictionary! { "O" => "Layout", "TextAlign" => "Center" } };
    let (mut doc, ids) = fixture(LINES);
    doc.get_dictionary_mut(ids[1])
        .unwrap()
        .set("ClassMap", classes);
    let first = doc.get_dictionary_mut(ids[3]).unwrap();
    first.remove(b"A");
    first.set("C", Object::Name(b"Centred".to_vec()));
    assert!((centre(&write(doc.clone())) - middle).abs() < 0.01);
    align(&mut doc, ids[3], "Start");
    assert_eq!(span(&write(doc), "FIRSTFIRST").0, start);
}

#[test]
fn textedit_a_centred_line_refuses_what_would_leave_it_off_centre() {
    let (mut doc, ids) = fixture(LINES);
    align(&mut doc, ids[3], "Center");
    let attempt = |doc: &Document, change: Change| {
        let mut copy = doc.clone();
        let result = textedit::write(&mut copy, &[change]);
        if result.is_err() {
            assert_eq!(copy.objects, doc.objects);
        }
        result
    };
    // No layout: written at the origin.
    assert_eq!(
        attempt(&doc, change(&doc, "FIRST", "IF", false)).unwrap_err(),
        textedit::centred::NEEDS_LAYOUT
    );
    assert!(attempt(&doc, change(&doc, "SECOND", "IF", false)).is_ok());
    // More than one line.
    let mut wrapped = change(&doc, "FIRST", "FIRST FIRST", true);
    wrapped.layout.as_mut().unwrap().wrap = true;
    assert_eq!(
        attempt(&doc, wrapped).unwrap_err(),
        "A centred line cannot be wrapped yet. Keep the text on one line."
    );
    // No room before the line: it starts 6 pt from the page's edge.
    let edge = b"/Standard << /MCID 0 >> BDC BT /F1 12 Tf 6 180 Td (FIRST) Tj ET EMC /Standard << /MCID 1 >> BDC BT /F1 12 Tf 80 140 Td (SECOND) Tj ET EMC";
    let (mut doc, ids) = fixture(edge);
    align(&mut doc, ids[3], "Center");
    assert_eq!(
        attempt(&doc, change(&doc, "FIRST", "FIRSTFIR", true)).unwrap_err(),
        "There is no room to keep this line centred. Shorten the text or reduce the font size."
    );
    assert!(attempt(&doc, change(&doc, "FIRST", "IF", true)).is_ok());
}
