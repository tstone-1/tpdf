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
