//! Wrapping an edit onto a new line of its own paragraph, on a tagged page.
//!
//! The fixtures are the synthetic font `layout_tests` uses, whose every glyph is
//! 600/1000 wide: at 12 pt a character is exactly 7.2 pt. The page is 300 x 240,
//! so a run starting at x 20 has 280 pt of room -- 38 characters -- before the
//! page edge stops it, and anything longer is what a wrap is for.
//!
//! The standard paragraph is three lines at a 14 pt pitch, the second the
//! widest at 24 characters, so the paragraph's measure is 172.8 pt from x 20;
//! and a second paragraph 52 pt below the last line. Everything is one text
//! object positioned by relative `Td`, which is the case a moved line has to be
//! most careful with: every line after it, in the paragraph or not, is placed
//! from the line matrix the moved one leaves behind.
use super::layout_tests::{in_default_box, reader_line_starts, shows};
use super::*;
use lopdf::dictionary;

pub(super) const FIRST: &str = "FIRST ONE SECOND THIRD";
pub(super) const WIDEST: &str = "FIFTY NINE ONCE AND DONE";
pub(super) const LAST: &str = "TEN";
pub(super) const NEXT: &str = "SECOND BRANCH";
/// 47 characters: 338.4 pt, past the 280 pt the line has, and 24 + 21 at the
/// paragraph's measure, breaking at the space after DONE.
pub(super) const LONGER: &str = "FIFTY NINE ONCE AND DONE THEN FIRST AND SECOND";

/// A page whose structure tree gives `blocks[i]` the MCIDs of paragraph `i`.
pub(super) fn tagged(content: &str, blocks: &[&[i64]]) -> Document {
    let (mut doc, _, _, _) = fonts::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let pages = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    let pages = doc.get_dictionary_mut(pages).unwrap();
    pages.set("Kids", vec![Object::Reference(page)]);
    pages.set("Count", 1);
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
    let root = doc.new_object_id();
    let document = doc.new_object_id();
    let paragraphs: Vec<ObjectId> = blocks.iter().map(|_| doc.new_object_id()).collect();
    let mut slots = vec![Object::Null; blocks.iter().map(|b| b.len()).sum()];
    for (id, mcids) in paragraphs.iter().zip(blocks) {
        for mcid in *mcids {
            slots[*mcid as usize] = Object::Reference(*id);
        }
        doc.objects.insert(
            *id,
            dictionary! {
                "Type" => "StructElem", "S" => "P", "P" => document, "Pg" => page,
                "K" => mcids.iter().map(|m| Object::Integer(*m)).collect::<Vec<_>>(),
            }
            .into(),
        );
    }
    let parents =
        doc.add_object(dictionary! { "Nums" => vec![Object::Integer(0), Object::Array(slots)] });
    doc.objects.insert(
        root,
        dictionary! { "Type" => "StructTreeRoot", "K" => vec![Object::Reference(document)], "ParentTree" => parents }
            .into(),
    );
    doc.objects.insert(
        document,
        dictionary! { "Type" => "StructElem", "S" => "Document", "P" => root, "Pg" => page,
        "K" => paragraphs.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>() }
        .into(),
    );
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set("Contents", stream);
    dict.set("StructParents", 0);
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog)
        .unwrap()
        .set("StructTreeRoot", root);
    doc
}

/// The standard paragraph's lines, then whatever `after` adds to the text
/// object, with the next paragraph `gap` points below the last line.
pub(super) fn content(gap: f64, after: &str) -> String {
    format!(
        "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC ({FIRST}) Tj EMC \
         0 -14 Td /P <</MCID 1>> BDC ({WIDEST}) Tj EMC \
         0 -14 Td /P <</MCID 2>> BDC ({LAST}) Tj EMC \
         0 -{gap} Td /P <</MCID 3>> BDC ({NEXT}) Tj EMC {after}ET"
    )
}

pub(super) fn paragraph(gap: f64) -> Document {
    tagged(&content(gap, ""), &[&[0, 1, 2], &[3]])
}

fn index_of(doc: &Document, text: &str) -> usize {
    scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .position(|run| run.text == text)
        .unwrap_or_else(|| panic!("no run {text}"))
}

fn run(doc: &Document, text: &str) -> Run {
    scan(doc, 0).unwrap().runs[index_of(doc, text)].clone()
}

fn refusal(doc: &Document, text: &str, replacement: &str) -> String {
    let mut copy = doc.clone();
    let before = copy.objects.clone();
    let error = write(
        &mut copy,
        &[in_default_box(doc, index_of(doc, text), replacement)],
    )
    .expect_err("the edit was accepted");
    assert_eq!(copy.objects, before, "{replacement} changed the document");
    error
}

fn wrapped(doc: &Document, text: &str, replacement: &str) -> Document {
    let mut copy = doc.clone();
    write(
        &mut copy,
        &[in_default_box(doc, index_of(doc, text), replacement)],
    )
    .unwrap_or_else(|error| panic!("{replacement}: {error}"));
    copy
}

// The edit is laid out at the paragraph's measure, its second line goes where
// the paragraph's next line was, and that line moves down one pitch. Nothing
// else on the page moves, down to the bit: the next paragraph is placed by a
// Td chained through every moved line, and it lands exactly where it did.
#[test]
fn an_edit_past_the_page_edge_wraps_and_moves_the_rest_of_its_paragraph_down() {
    let doc = paragraph(52.);
    // Unwrapped, the text needs 338.4 pt and the line has 280.
    let saved = wrapped(&doc, WIDEST, LONGER);
    let runs = scan(&saved, 0).unwrap().runs;
    // The blank runs are the cursor restorations every replacement ends with.
    let texts: Vec<&str> = runs
        .iter()
        .map(|run| run.text.trim_end())
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        texts,
        [
            FIRST,
            "FIFTY NINE ONCE AND DONE",
            "THEN FIRST AND SECOND",
            LAST,
            NEXT
        ]
    );
    let at = |text: &str| {
        let run = runs.iter().find(|run| run.text.trim_end() == text).unwrap();
        (run.matrix[4], run.matrix[5])
    };
    assert_eq!(at(FIRST), (20., 200.));
    assert_eq!(at("FIFTY NINE ONCE AND DONE"), (20., 186.));
    // The continuation starts at the paragraph's left edge, one pitch down.
    let (x, y) = at("THEN FIRST AND SECOND");
    assert!(
        (x - 20.).abs() < 0.0001 && (y - 172.).abs() < 0.0001,
        "{x} {y}"
    );
    // The last line moved down by the one line the edit added.
    let (x, y) = at(LAST);
    assert!(
        (x - 20.).abs() < 0.0001 && (y - 158.).abs() < 0.0001,
        "{x} {y}"
    );
    assert_eq!(at(NEXT), (20., 120.));
    // Its own bytes are the source's; only where it is drawn changed.
    assert!(shows(&saved).contains(&Object::string_literal(LAST)));
    // And every line a reader places from the line matrix -- the one after
    // the moved line above all -- starts at the bits it always did.
    let source = reader_line_starts(&doc);
    let after = reader_line_starts(&saved);
    let next = |starts: &[(Object, Option<[u32; 2]>)]| {
        starts
            .iter()
            .find(|(show, _)| *show == Object::string_literal(NEXT))
            .unwrap()
            .1
    };
    assert_eq!(next(&after), next(&source));
    // The structure is untouched: the tags still own the same content.
    assert!(scan(&saved, 0).is_ok());
}

// A paragraph with no room below it is refused, and says why: the lines it
// would move have nowhere to go.
#[test]
fn a_paragraph_with_no_room_below_is_refused_with_the_reason() {
    // The next paragraph 14 pt below: it is the paragraph's own pitch, so the
    // last line would land on it.
    let error = refusal(&paragraph(14.), WIDEST, LONGER);
    assert!(
        error.contains("its lines would move onto what is below it"),
        "{error}"
    );
    // 28 pt below leaves exactly one line of the paragraph's own pitch.
    wrapped(&paragraph(28.), WIDEST, LONGER);
}

// An untagged page has no answer to which lines are one paragraph, so its
// refusal is the one it always had.
#[test]
fn an_untagged_page_keeps_the_page_edge_refusal() {
    let bare = content(52., "")
        .replace(" /P <</MCID 0>> BDC", "")
        .replace(" /P <</MCID 1>> BDC", "")
        .replace(" /P <</MCID 2>> BDC", "")
        .replace(" /P <</MCID 3>> BDC", "")
        .replace(" EMC", "");
    let doc = super::layout_tests::synthetic(&bare);
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(error.contains("it reaches the edge of the page"), "{error}");
}

/// The standard paragraph with its widest line set as two runs, the second
/// placed by a displacement of one character from the cursor the first leaves:
/// `FIFTY NINE` from x 20 to 92, and `ONCE AND DONE` from x 99.2 to 192.8.
fn split_line(gap: f64, after: &str) -> Document {
    tagged(
        &content(gap, after).replace(
            &format!("({WIDEST}) Tj EMC"),
            "(FIFTY NINE) Tj [-600 (ONCE AND DONE)] TJ EMC",
        ),
        &[&[0, 1, 2], &[3]],
    )
}

/// Where every run with text is drawn, in reading order.
fn placed_runs(doc: &Document) -> Vec<(String, f64, f64)> {
    scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.trim().is_empty())
        .map(|run| (run.text.clone(), run.matrix[4], run.matrix[5]))
        .collect()
}

fn at(runs: &[(String, f64, f64)], text: &str) -> (f64, f64) {
    let (_, x, y) = runs
        .iter()
        .find(|(run, ..)| run.trim_end() == text)
        .unwrap_or_else(|| panic!("no run {text} in {runs:?}"));
    (*x, *y)
}

fn near((x, y): (f64, f64), (ex, ey): (f64, f64)) -> bool {
    (x - ex).abs() < 0.0001 && (y - ey).abs() < 0.0001
}

// The rest of the edited line flows after the edit: here the edit wraps at
// the paragraph's measure, 172.8 pt, and `ONCE AND DONE` follows its second
// line with the gap it had, one character. Pushed along instead, the line
// would need 43 characters of the 38 the page has. The line below moves down the one line that was added, and the next
// paragraph stays where it was, down to the bits a reader places it from.
#[test]
fn the_text_after_the_edit_on_its_line_flows_after_it() {
    let doc = split_line(52., "");
    let saved = wrapped(&doc, "FIFTY NINE", "FIFTY NINE THEN FIRST AND SEC");
    let runs = placed_runs(&saved);
    let texts: Vec<&str> = runs.iter().map(|(text, ..)| text.trim_end()).collect();
    assert_eq!(
        texts,
        [
            FIRST,
            "FIFTY NINE THEN FIRST",
            "AND SEC",
            "ONCE AND DONE",
            LAST,
            NEXT
        ]
    );
    assert!(near(at(&runs, "AND SEC"), (20., 172.)), "{runs:?}");
    // One character after `AND SEC`, which is 7 characters long.
    assert!(near(at(&runs, "ONCE AND DONE"), (77.6, 172.)), "{runs:?}");
    assert!(near(at(&runs, LAST), (20., 158.)), "{runs:?}");
    assert_eq!(at(&runs, NEXT), (20., 120.));
    // It is the source's own show, moved: its leading displacement, which
    // said where it started, is what the move replaced.
    assert!(
        shows(&saved).contains(&Object::Array(vec![Object::string_literal(
            "ONCE AND DONE"
        )]))
    );
    let next = |starts: &[(Object, Option<[u32; 2]>)]| {
        starts
            .iter()
            .find(|(show, _)| *show == Object::string_literal(NEXT))
            .unwrap()
            .1
    };
    assert_eq!(
        next(&reader_line_starts(&saved)),
        next(&reader_line_starts(&doc))
    );
}

// The text after the edit follows the edit's last glyph where its ink ends,
// when that is past its advance: here `D` is reshaped to reach 0.6 pt past
// its own, and the gap of one character is kept from there rather than from
// the advance, so the overhang is not drawn into the run that follows.
#[test]
fn text_after_the_edit_keeps_its_gap_from_an_overhanging_last_glyph() {
    let mut doc = split_line(52., "");
    let (_, _, _, program) = fonts::tests::fixture();
    let stream = doc
        .get_object_mut(program)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = fonts::ink_tests::with_components(
        stream.content.clone(),
        [('B', 0, 0, 16384), ('D', 250, 0, 16384)],
    );
    let saved = wrapped(&doc, "FIFTY NINE", "FIFTY NINE THEN FIRST AND SED");
    let runs = placed_runs(&saved);
    assert!(near(at(&runs, "AND SED"), (20., 172.)), "{runs:?}");
    assert!(near(at(&runs, "ONCE AND DONE"), (78.2, 172.)), "{runs:?}");
}

// When the text after the edit does not fit what is left of the edit's last
// line, it starts a line of its own at the paragraph's left edge, with no gap
// in front of it, and the lines below move down by both lines.
#[test]
fn text_after_the_edit_that_does_not_fit_its_last_line_starts_the_next() {
    let doc = split_line(52., "");
    // The second line is `AND SECOND AND`, 100.8 pt; the gap and
    // `ONCE AND DONE` are another 100.8, and the measure is 172.8.
    let saved = wrapped(&doc, "FIFTY NINE", "FIFTY NINE THEN FIRST AND SECOND AND");
    let runs = placed_runs(&saved);
    assert!(near(at(&runs, "AND SECOND AND"), (20., 172.)), "{runs:?}");
    assert!(near(at(&runs, "ONCE AND DONE"), (20., 158.)), "{runs:?}");
    assert!(near(at(&runs, LAST), (20., 144.)), "{runs:?}");
    assert_eq!(at(&runs, NEXT), (20., 120.));
    // Two lines is more than 24 pt of room below leaves.
    let error = refusal(
        &split_line(38., ""),
        "FIFTY NINE",
        "FIFTY NINE THEN FIRST AND SECOND AND",
    );
    assert!(
        error.contains("its lines would move onto what is below it"),
        "{error}"
    );
    // One line fits in the same room.
    wrapped(
        &split_line(38., ""),
        "FIFTY NINE",
        "FIFTY NINE THEN FIRST AND SEC",
    );
}

// Under a hanging indent the lines after the first start further right, so
// the text after a short first word can be wider than a whole continuation
// line. It has nowhere to flow to, and the edit keeps the refusal it had
// rather than setting that text past the paragraph's measure.
#[test]
fn text_after_the_edit_wider_than_a_continuation_line_keeps_the_refusal() {
    let hanging = |first: &str| {
        tagged(
            &format!(
                "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC ({first}) Tj \
                 ( NINE ONCE AND DONE THEN FIRST) Tj EMC \
                 20 -14 Td /P <</MCID 1>> BDC ({LAST}) Tj EMC \
                 -20 -52 Td /P <</MCID 2>> BDC ({NEXT}) Tj EMC ET"
            ),
            &[&[0, 1], &[2]],
        )
    };
    // `FI` ends at 34.4, before the continuation's 40: the 216 pt after it
    // is wider than the 210.4 pt from 40 to the measure.
    let error = refusal(&hanging("FI"), "FI", "FIFTY NINE");
    assert!(error.contains("it reaches the edge of the page"), "{error}");
    // After `FIFTY` it starts at 56 and fits a continuation line.
    let saved = wrapped(&hanging("FIFTY"), "FIFTY", "FIFTY NINE ");
    let runs = placed_runs(&saved);
    assert!(
        near(at(&runs, " NINE ONCE AND DONE THEN FIRST"), (40., 186.)),
        "{runs:?}"
    );
    // After `FIF` it starts at 41.6. The box the editor opens is 21.6 pt
    // rounded up, so the run after it starts inside the box, which `room`
    // skips; the push along the line still finds it, and it flows the same.
    let saved = wrapped(&hanging("FIF"), "FIF", "FIFTY NINE ");
    let runs = placed_runs(&saved);
    assert!(
        near(at(&runs, " NINE ONCE AND DONE THEN FIRST"), (40., 186.)),
        "{runs:?}"
    );
}

// A run kerned back into the edit by more than the push's tenth of a unit is
// not on the push's line -- the source already overlaps there -- but it is the
// paragraph's, and movable, so it flows like any other.
#[test]
fn text_kerned_into_the_edit_flows_although_the_push_leaves_it() {
    let doc = tagged(
        &format!(
            "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC (FIFTY) Tj \
             [50 ( NINE ONCE AND DONE THEN FIRST)] TJ EMC \
             20 -14 Td /P <</MCID 1>> BDC ({LAST}) Tj EMC \
             -20 -52 Td /P <</MCID 2>> BDC ({NEXT}) Tj EMC ET"
        ),
        &[&[0, 1], &[2]],
    );
    let saved = wrapped(&doc, "FIFTY", "FIFTY NINE ");
    let runs = placed_runs(&saved);
    assert!(
        near(at(&runs, " NINE ONCE AND DONE THEN FIRST"), (40., 186.)),
        "{runs:?}"
    );
}

// Text after the edit that the writer cannot move -- here inside an
// ActualText span, whose grammar the writer owns -- keeps the refusal the
// edit had. The span starts inside the box, so the push along the line never
// looked at it and only the flow's own check stands in the way.
#[test]
fn text_after_the_edit_that_cannot_be_moved_keeps_the_refusal() {
    let doc = tagged(
        &format!(
            "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC (FIF) Tj \
             /Span <</ActualText ( NINE ONCE AND DONE THEN FIRST)>> BDC \
             ( NINE ONCE AND DONE THEN FIRST) Tj EMC EMC \
             20 -14 Td /P <</MCID 1>> BDC ({LAST}) Tj EMC \
             -20 -52 Td /P <</MCID 2>> BDC ({NEXT}) Tj EMC ET"
        ),
        &[&[0, 1], &[2]],
    );
    let error = refusal(&doc, "FIF", "FIFTY NINE ");
    assert!(!error.contains("paragraph"), "{error}");
}

// Text of another block sharing the edited line -- a second column set close
// to the first -- is not this paragraph's to move onto a new line, so the edit
// keeps the refusal it had.
#[test]
fn another_blocks_text_after_the_edit_on_its_line_keeps_the_page_edge_refusal() {
    let doc = tagged(
        &content(52., "").replace(
            &format!("({WIDEST}) Tj EMC"),
            "(FIFTY NINE) Tj ( ONCE) Tj EMC /P <</MCID 4>> BDC ( AND DONE) Tj EMC",
        ),
        &[&[0, 1, 2], &[3], &[4]],
    );
    let error = refusal(&doc, "FIFTY NINE", "FIFTY NINE THEN FIRST AND SEC");
    assert!(error.contains("it reaches the edge of the page"), "{error}");
}

// A batch that wraps a line and also edits the text the wrap moves along it
// is refused, in either order: the second edit is written where its source
// was, which is where the wrapped edit now is.
#[test]
fn a_wrap_that_moves_the_text_after_it_refuses_an_edit_of_that_text() {
    let doc = split_line(52., "");
    let wrap = in_default_box(
        &doc,
        index_of(&doc, "FIFTY NINE"),
        "FIFTY NINE THEN FIRST AND SEC",
    );
    let other = in_default_box(&doc, index_of(&doc, "ONCE AND DONE"), "ONCE AND");
    for batch in [[wrap.clone(), other.clone()], [other, wrap]] {
        let error = write(&mut doc.clone(), &batch).unwrap_err();
        assert!(error.contains("another pending edit"), "{error}");
    }
}

// The last line of a paragraph wraps into the space below it and moves
// nothing, because nothing of its paragraph is below it.
#[test]
fn the_last_line_wraps_into_the_space_below_and_moves_nothing() {
    let doc = paragraph(52.);
    let saved = wrapped(&doc, LAST, LONGER);
    let texts: Vec<String> = scan(&saved, 0)
        .unwrap()
        .runs
        .iter()
        .map(|run| run.text.trim_end().to_owned())
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(
        texts,
        [
            FIRST,
            WIDEST,
            "FIFTY NINE ONCE AND DONE",
            "THEN FIRST AND SECOND",
            NEXT
        ]
    );
    let next = run(&saved, NEXT);
    assert_eq!((next.matrix[4], next.matrix[5]), (20., 120.));
}

// The continuation starts where the paragraph's lines do, not where the run
// does: under a first-line indent, the first line's run starts further right.
#[test]
fn a_continuation_starts_at_the_paragraphs_left_edge_under_a_first_line_indent() {
    let doc = tagged(
        &content(52., "")
            .replace("20 200 Td", "40 200 Td")
            .replace("0 -14 Td /P <</MCID 1>>", "-20 -14 Td /P <</MCID 1>>"),
        &[&[0, 1, 2], &[3]],
    );
    // FIRST starts at 40 and is the paragraph's furthest-reaching line, so
    // the measure is its own end at 198.4: 158.4 pt of room, 22 characters.
    let saved = wrapped(&doc, FIRST, LONGER);
    let runs = scan(&saved, 0).unwrap().runs;
    let second = &runs[1];
    assert!(
        (second.matrix[4] - 20.).abs() < 0.0001 && (second.matrix[5] - 186.).abs() < 0.0001,
        "{:?}",
        second.matrix
    );
    // The first line broke at the measure, not at the page: DONE would take
    // the line to 23 characters.
    assert_eq!(runs[0].text.trim_end(), "FIFTY NINE ONCE AND");
}

// A rule drawn under a line that would move stays where it is while the text
// leaves it; a background holding the whole paragraph still holds it.
#[test]
fn a_drawing_partly_over_the_moved_lines_refuses_and_one_holding_them_does_not() {
    let rule = tagged(
        &format!("0 0 0 rg 20 155 30 1 re f {}", content(52., "")),
        &[&[0, 1, 2], &[3]],
    );
    let error = refusal(&rule, WIDEST, LONGER);
    assert!(error.contains("a drawing or an annotation"), "{error}");
    let background = tagged(
        &format!("0.9 g 0 0 300 240 re f 0 g {}", content(52., "")),
        &[&[0, 1, 2], &[3]],
    );
    wrapped(&background, WIDEST, LONGER);
}

// A highlight over the line that would move stays where it is too.
#[test]
fn an_annotation_over_the_moved_lines_refuses() {
    let mut doc = paragraph(52.);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let note = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Highlight",
        "Rect" => vec![18.into(), 168.into(), 50.into(), 184.into()],
    });
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::Reference(note)]);
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(error.contains("a drawing or an annotation"), "{error}");
    // A popup is where a note opens, not a mark on the page.
    doc.get_dictionary_mut(note)
        .unwrap()
        .set("Subtype", "Popup");
    wrapped(&doc, WIDEST, LONGER);
}

// Two edits in one batch that a wrap puts in each other's way are refused
// whichever order the batch lists them in; an edit above the wrap is not in
// its way.
#[test]
fn a_wrap_that_moves_another_pending_edit_is_refused_in_either_order() {
    let doc = paragraph(52.);
    let wrap = in_default_box(&doc, index_of(&doc, WIDEST), LONGER);
    let other = in_default_box(&doc, index_of(&doc, LAST), "NINE");
    for batch in [[wrap.clone(), other.clone()], [other.clone(), wrap.clone()]] {
        let mut copy = doc.clone();
        let error = write(&mut copy, &batch).unwrap_err();
        assert!(error.contains("another pending edit"), "{error}");
    }
    // Above the wrap is not in its way.
    let above = in_default_box(&doc, index_of(&doc, FIRST), "FIRST");
    write(&mut doc.clone(), &[wrap, above]).unwrap();
}

// A paragraph of one line has no measure, so its first line keeps the room it
// had and its continuation starts under it.
#[test]
fn a_single_line_paragraph_wraps_at_the_room_it_had_at_the_default_pitch() {
    let doc = tagged(
        "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC (FIRST) Tj EMC \
         0 -80 Td /P <</MCID 1>> BDC (SECOND) Tj EMC ET",
        &[&[0], &[1]],
    );
    let text = format!("{LONGER} {LONGER}");
    let saved = wrapped(&doc, "FIRST", &text);
    let runs = scan(&saved, 0).unwrap().runs;
    // 280 pt of room: 38 characters, broken at the last space inside it.
    assert_eq!(
        runs[0].text.trim_end(),
        "FIFTY NINE ONCE AND DONE THEN FIRST"
    );
    assert!(
        (runs[1].matrix[5] - 185.).abs() < 0.0001,
        "{:?}",
        runs[1].matrix
    );
}

// Text of the paragraph the editor may not move -- here a run that draws back
// over itself and is kept read-only -- on a line the wrap would move: the wrap
// is refused rather than leaving that text behind on its own.
#[test]
fn read_only_text_of_the_paragraph_below_the_edit_refuses_the_wrap() {
    let doc = tagged(
        &content(52., "").replace(&format!("({LAST}) Tj"), "[(TEN) 3000 (T)] TJ"),
        &[&[0, 1, 2], &[3]],
    );
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(
        error.contains("part of it below cannot be moved"),
        "{error}"
    );
}

// Each line in its own text object, so that another block's run can sit on the
// paragraph's last line and come before or after the paragraph in the stream.
// The pitch is 16 pt, a little over the 15 pt em box, so that no line's hit
// rectangle reaches the next.
fn beside(neighbour_first: bool) -> Document {
    let neighbour = "BT /F1 12 Tf 2 168 Td /P <</MCID 4>> BDC (A) Tj EMC ET ";
    let lines = format!(
        "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC ({FIRST}) Tj EMC ET \
         BT /F1 12 Tf 20 184 Td /P <</MCID 1>> BDC ({WIDEST}) Tj EMC ET "
    );
    let last = format!(
        "BT /F1 12 Tf 20 168 Td /P <</MCID 2>> BDC ({LAST}) Tj EMC ET \
         BT /F1 12 Tf 20 120 Td /P <</MCID 3>> BDC ({NEXT}) Tj EMC ET"
    );
    let body = if neighbour_first {
        format!("{neighbour}{lines}{last}")
    } else {
        format!("{lines}{neighbour}{last}")
    };
    tagged(&body, &[&[0, 1, 2], &[3], &[4]])
}

// A run of another block, set just left of the paragraph's last line, grows and
// pushes that line along it while the paragraph wraps and moves it down. The
// two are written from separate copies of the line's bytes, so the batch is
// refused, whichever of the two the stream reaches first.
#[test]
fn a_line_pushed_by_one_edit_and_moved_down_by_a_wrap_is_refused() {
    for neighbour_first in [true, false] {
        let doc = beside(neighbour_first);
        let push = in_default_box(&doc, index_of(&doc, "A"), "ABCDE");
        let wrap = in_default_box(&doc, index_of(&doc, WIDEST), LONGER);
        // Each alone is accepted: the push moves TEN along, the wrap moves it
        // down.
        for alone in [&push, &wrap] {
            write(&mut doc.clone(), std::slice::from_ref(alone))
                .unwrap_or_else(|error| panic!("{neighbour_first} {}: {error}", alone.replacement));
        }
        let error = write(&mut doc.clone(), &[push, wrap]).unwrap_err();
        assert!(
            error.contains("another pending edit"),
            "{neighbour_first}: {error}"
        );
    }
}

// On a page turned a quarter, the paragraph's "down" is along the displayed
// page's x axis. The page-space result is the same as unturned, and so is the
// refusal when there is no room, which is the cross axis being read correctly.
#[test]
fn a_wrap_on_a_quarter_turned_page_moves_the_same_lines_the_same_way() {
    for turns in [90, 180, 270] {
        let turned = |gap: f64| {
            let mut doc = paragraph(gap);
            let page = crate::pagetree::ordered_pages(&doc)[0];
            doc.get_dictionary_mut(page).unwrap().set("Rotate", turns);
            doc
        };
        let saved = wrapped(&turned(52.), WIDEST, LONGER);
        let last = run(&saved, LAST);
        assert!(
            (last.matrix[4] - 20.).abs() < 0.0001 && (last.matrix[5] - 158.).abs() < 0.0001,
            "Rotate {turns}: {:?}",
            last.matrix
        );
        let error = refusal(&turned(14.), WIDEST, LONGER);
        assert!(
            error.contains("what is below it"),
            "Rotate {turns}: {error}"
        );
    }
}

// The editor outlines runs where the pending batch leaves them: the wrapped
// run's box is both of its lines, and the line it moved is one pitch lower on
// the displayed page, which runs downwards.
#[test]
fn placements_outline_the_wrapped_box_and_the_line_it_moved() {
    let doc = paragraph(52.);
    let source = scan(&doc, 0).unwrap();
    let edited = &source.runs[index_of(&doc, WIDEST)];
    let last = &source.runs[index_of(&doc, LAST)];
    let placed = placements(
        &doc,
        0,
        &[in_default_box(&doc, index_of(&doc, WIDEST), LONGER)],
    )
    .unwrap();
    let moved = placed[&last.operator];
    assert!(
        (moved[1] - last.display_rect[1] - 14.).abs() < 0.001,
        "{moved:?}"
    );
    assert!(
        (moved[3] - last.display_rect[3] - 14.).abs() < 0.001,
        "{moved:?}"
    );
    assert_eq!(
        (moved[0], moved[2]),
        (last.display_rect[0], last.display_rect[2])
    );
    let boxed = placed[&edited.operator];
    // Two lines deep: from the top of the edited line to one pitch further
    // down than its bottom.
    assert!(
        (boxed[1] - edited.display_rect[1]).abs() < 0.001,
        "{boxed:?}"
    );
    assert!(
        (boxed[3] - edited.display_rect[3] - 14.).abs() < 0.001,
        "{boxed:?}"
    );
    // Nothing else moved.
    assert_eq!(placed.len(), 2, "{placed:?}");
}

// The run after the edit is outlined where it flowed to: one line down, and
// from 20 + 50.4 + 7.2 along the displayed page, which runs downwards, so the
// line below it moved down one pitch too.
#[test]
fn placements_outline_the_text_that_flowed_after_the_edit() {
    let doc = split_line(52., "");
    let source = scan(&doc, 0).unwrap();
    let after = &source.runs[index_of(&doc, "ONCE AND DONE")];
    let last = &source.runs[index_of(&doc, LAST)];
    let placed = placements(
        &doc,
        0,
        &[in_default_box(
            &doc,
            index_of(&doc, "FIFTY NINE"),
            "FIFTY NINE THEN FIRST AND SEC",
        )],
    )
    .unwrap();
    let flowed = placed[&after.operator];
    assert!((flowed[0] - 77.6).abs() < 0.001, "{flowed:?}");
    assert!(
        (flowed[1] - after.display_rect[1] - 14.).abs() < 0.001,
        "{flowed:?}"
    );
    assert!(
        (flowed[2] - flowed[0] - (after.display_rect[2] - after.display_rect[0])).abs() < 0.001,
        "{flowed:?}"
    );
    let moved = placed[&last.operator];
    assert!(
        (moved[1] - last.display_rect[1] - 14.).abs() < 0.001,
        "{moved:?}"
    );
    assert_eq!(placed.len(), 3, "{placed:?}");
}

// A link set in the paragraph's last line is the paragraph's text as far as a
// wrap is concerned, though the structure tree gives it an element of its own:
// the paragraph's walk takes it in as one of its leaves. The wrap is refused
// because that line cannot move. Were the link text of no block, the wrap would
// move nothing, leave the link where it is, and set the new line on top of it.
// The link's rectangle is put elsewhere on purpose, so that it is the text and
// not the annotation that refuses.
#[test]
fn a_link_in_a_line_below_the_edit_is_part_of_the_paragraph_and_refuses_the_wrap() {
    let mut doc = paragraph(52.);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let root = doc
        .get_dictionary(catalog)
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let parents = doc
        .get_dictionary(root)
        .unwrap()
        .get(b"ParentTree")
        .unwrap()
        .as_reference()
        .unwrap();
    let slots = doc
        .get_dictionary(parents)
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap()[1]
        .as_array()
        .unwrap()
        .clone();
    let (first, second) = (
        slots[0].as_reference().unwrap(),
        slots[3].as_reference().unwrap(),
    );
    let annotation = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "P" => page, "StructParent" => 1,
        "Rect" => vec![200.into(), 20.into(), 240.into(), 40.into()],
    });
    let link = doc.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Link", "P" => first, "Pg" => page,
        "K" => vec![
            Object::Integer(2),
            dictionary! { "Type" => "OBJR", "Obj" => annotation, "Pg" => page }.into(),
        ],
    });
    doc.get_dictionary_mut(first).unwrap().set(
        "K",
        vec![
            Object::Integer(0),
            Object::Integer(1),
            Object::Reference(link),
        ],
    );
    doc.get_dictionary_mut(parents).unwrap().set(
        "Nums",
        vec![
            0.into(),
            Object::Array(vec![first.into(), first.into(), link.into(), second.into()]),
            1.into(),
            link.into(),
        ],
    );
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::Reference(annotation)]);
    assert!(
        scan(&doc, 0)
            .unwrap()
            .runs
            .iter()
            .all(|run| run.text != LAST),
        "the link's text is offered for editing"
    );
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(
        error.contains("part of it below cannot be moved"),
        "{error}"
    );
}

// Lines more than three ems apart are not a paragraph's pitch -- a block that
// resumes past a figure, say -- and the wrap is not offered.
#[test]
fn a_line_three_ems_below_is_not_a_pitch_to_wrap_at() {
    let doc = tagged(
        &content(52., "").replacen("0 -14 Td /P <</MCID 2>>", "0 -40 Td /P <</MCID 2>>", 1),
        &[&[0, 1, 2], &[3]],
    );
    // The edit of the line above the gap keeps its page-edge refusal.
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(error.contains("it reaches the edge of the page"), "{error}");
    // 36 pt is three ems exactly, and is a pitch; TEN moves 36 pt down.
    let doc = tagged(
        &content(80., "").replacen("0 -14 Td /P <</MCID 2>>", "0 -36 Td /P <</MCID 2>>", 1),
        &[&[0, 1, 2], &[3]],
    );
    let saved = wrapped(&doc, WIDEST, LONGER);
    let last = run(&saved, LAST);
    assert!((last.matrix[5] - 114.).abs() < 0.0001, "{:?}", last.matrix);
}

// A moved line that opens with a displacement keeps it: the matrix it is
// placed at already stands past that number, so writing it again would move
// the line twice.
#[test]
fn a_moved_line_that_opens_with_a_displacement_is_moved_once() {
    let doc = tagged(
        &content(52., "").replace(&format!("({LAST}) Tj"), "[-500 (TEN)] TJ"),
        &[&[0, 1, 2], &[3]],
    );
    // -500 thousandths of 12 pt is 6 pt to the right.
    assert_eq!(run(&doc, LAST).matrix[4], 26.);
    let last = run(&wrapped(&doc, WIDEST, LONGER), LAST);
    assert!(
        (last.matrix[4] - 26.).abs() < 0.0001 && (last.matrix[5] - 158.).abs() < 0.0001,
        "{:?}",
        last.matrix
    );
}

// A show of another block that continues a moved line from the text cursor,
// far enough along to be outside the paragraph, stays exactly where it was:
// each moved show puts the cursor back where its source left it.
#[test]
fn a_show_continuing_a_moved_line_from_the_cursor_stays_where_it_was() {
    let doc = tagged(
        &content(52., "").replace(
            &format!("({LAST}) Tj EMC"),
            &format!("({LAST}) Tj EMC /P <</MCID 4>> BDC [-20000 (A)] TJ EMC"),
        ),
        &[&[0, 1, 2], &[3], &[4]],
    );
    // TEN is 21.6 pt and the displacement 240 pt: A starts at 281.6.
    let before = run(&doc, "A");
    assert!(
        (before.matrix[4] - 281.6).abs() < 0.0001,
        "{:?}",
        before.matrix
    );
    let after = run(&wrapped(&doc, WIDEST, LONGER), "A");
    assert_eq!(after.matrix, before.matrix);
}

// At the foot of the page there is nowhere to move a line to, and nowhere to
// set a new last line either.
#[test]
fn at_the_foot_of_the_page_there_is_no_room_below() {
    let foot = tagged(
        &content(52., "")
            .replace("20 200 Td", "20 40 Td")
            .replace("0 -52 Td /P <</MCID 3>> BDC", "0 90 Td /P <</MCID 3>> BDC"),
        &[&[0, 1, 2], &[3]],
    );
    // TEN's baseline is 12 and its hit rectangle reaches 3 pt below it; one
    // more line down is 2 pt below the page.
    assert_eq!(run(&foot, LAST).matrix[5], 12.);
    for text in [WIDEST, LAST] {
        let error = refusal(&foot, text, LONGER);
        assert!(error.contains("what is below it"), "{text}: {error}");
    }
}

// Read-only text of another block inside a line that would move -- between two
// of the paragraph's own runs -- would be left behind on its own.
#[test]
fn another_blocks_text_inside_a_moved_line_refuses_the_wrap() {
    let doc = tagged(
        &content(52., "").replace(
            &format!("({LAST}) Tj EMC"),
            &format!(
                "({LAST}) Tj EMC /P <</MCID 4>> BDC [(A) 3000 (A)] TJ EMC \
                 /P <</MCID 5>> BDC [-3000 ({LAST})] TJ EMC"
            ),
        ),
        &[&[0, 1, 2, 5], &[3], &[4]],
    );
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(
        error.contains("part of it below cannot be moved"),
        "{error}"
    );
}

// The paragraph's pitch is the pitch of its own size: an edit at another size
// keeps the refusal it had, and so does a run an earlier edit has already
// pushed along its line.
#[test]
fn a_different_size_or_a_run_already_pushed_keeps_the_page_edge_refusal() {
    let doc = paragraph(52.);
    let mut smaller = in_default_box(&doc, index_of(&doc, WIDEST), &format!("{LONGER} {LONGER}"));
    smaller.layout.as_mut().unwrap().size = 11.;
    let error = write(&mut doc.clone(), &[smaller]).unwrap_err();
    assert!(error.contains("it reaches the edge of the page"), "{error}");
    // FIFTY and the rest of its line as two runs: growing the first pushes
    // the second, which then has less room than it needs.
    let split = tagged(
        &content(52., "").replace(
            &format!("({WIDEST}) Tj"),
            "(FIFTY) Tj ( NINE ONCE AND DONE) Tj",
        ),
        &[&[0, 1, 2], &[3]],
    );
    let push = in_default_box(&split, index_of(&split, "FIFTY"), "FIFTY FIFTY");
    let wrap = in_default_box(&split, index_of(&split, " NINE ONCE AND DONE"), LONGER);
    write(&mut split.clone(), std::slice::from_ref(&push)).unwrap();
    let error = write(&mut split.clone(), &[push, wrap]).unwrap_err();
    assert!(error.contains("it reaches the edge of the page"), "{error}");
}

// A page whose annotation list cannot be read may have one over the lines a
// wrap would move, so the wrap is refused; nothing else on the page reads that
// list, and every other edit is accepted exactly as before.
#[test]
fn an_unreadable_annotation_list_refuses_a_wrap_and_nothing_else() {
    let mut doc = paragraph(52.);
    let page = crate::pagetree::ordered_pages(&doc)[0];
    doc.get_dictionary_mut(page).unwrap().set("Annots", 7);
    let error = refusal(&doc, WIDEST, LONGER);
    assert!(error.contains("a drawing or an annotation"), "{error}");
    wrapped(&doc, WIDEST, "FIFTY NINE");
}

// A clip over the paragraph holds its lines where they are; a line the wrap
// would move out of it is refused rather than moved into the clip's shadow.
#[test]
fn a_clip_over_the_paragraph_stops_the_lines_it_would_move() {
    // TEN's hit rectangle is 169..184 and would be 155..170 one line down.
    let clipped = |bottom: f64| {
        tagged(
            &format!(
                "q 0 {bottom} 300 {} re W n {} Q",
                240. - bottom,
                content(52., "")
            ),
            &[&[0, 1, 2], &[3]],
        )
    };
    let error = refusal(&clipped(160.), WIDEST, LONGER);
    assert!(error.contains("what is below it"), "{error}");
    wrapped(&clipped(150.), WIDEST, LONGER);
}

// The preview crop has to show every pixel the wrap changes, and a moved line
// changes the pixels where it was as well as where it went. With a short third
// line and a wide fourth, the third moves into the fourth's place and covers
// only a sliver of what the fourth drew there.
#[test]
fn the_preview_covers_where_the_moved_lines_were_as_well_as_where_they_went() {
    let doc = tagged(
        &content(52., "").replace(
            &format!("({LAST}) Tj EMC"),
            &format!("({LAST}) Tj EMC 0 -14 Td /P <</MCID 4>> BDC ({WIDEST}) Tj EMC"),
        ),
        &[&[0, 1, 2, 4], &[3]],
    );
    let source = scan(&doc, 0).unwrap();
    let fourth = source
        .runs
        .iter()
        .filter(|run| run.text == WIDEST)
        .nth(1)
        .unwrap();
    let preview =
        preview_layout(&doc, &in_default_box(&doc, index_of(&doc, WIDEST), LONGER)).unwrap();
    // Where the fourth line was: 20..192.8 across, 67..82 down the page.
    let old = fourth.display_rect;
    let extent = preview.extent;
    assert!(
        extent[0] <= old[0] && extent[1] <= old[1] && extent[2] >= old[2] && extent[3] >= old[3],
        "{extent:?} does not cover {old:?}"
    );
}

// A run that flows from the end of the edit's line to the start of the next
// leaves a place right of the box and of where it lands, and the preview has
// to show that place emptied too.
#[test]
fn the_preview_covers_where_the_text_that_flowed_was() {
    let doc = split_line(52., "");
    let old = scan(&doc, 0).unwrap().runs[index_of(&doc, "ONCE AND DONE")].display_rect;
    let preview = preview_layout(
        &doc,
        &in_default_box(
            &doc,
            index_of(&doc, "FIFTY NINE"),
            "FIFTY NINE THEN FIRST AND SECOND AND",
        ),
    )
    .unwrap();
    let extent = preview.extent;
    assert!(
        extent[0] <= old[0] && extent[1] <= old[1] && extent[2] >= old[2] && extent[3] >= old[3],
        "{extent:?} does not cover {old:?}"
    );
}

// A line push gives a displacement to the first show after the edit and lets
// the rest of the line ride the cursor. Here the show that rides is the
// paragraph's last line, and the one given the displacement belongs to another
// block, so the wrap moving the paragraph down and the push moving that line
// along meet in a show the push never names on its own: only the record of the
// whole pushed line can refuse the pair.
#[test]
fn a_show_riding_a_pushed_line_is_not_also_moved_down_by_a_wrap() {
    let doc = tagged(
        &format!(
            "BT /F1 12 Tf 2 168 Td /P <</MCID 4>> BDC (A) Tj EMC /P <</MCID 5>> BDC (B) Tj EMC \
             /P <</MCID 2>> BDC ({LAST}) Tj EMC ET \
             BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC ({FIRST}) Tj EMC ET \
             BT /F1 12 Tf 20 184 Td /P <</MCID 1>> BDC ({WIDEST}) Tj EMC ET \
             BT /F1 12 Tf 20 120 Td /P <</MCID 3>> BDC ({NEXT}) Tj EMC ET"
        ),
        &[&[0, 1, 2], &[3], &[4, 5]],
    );
    let push = in_default_box(&doc, index_of(&doc, "A"), "ABCDE");
    let wrap = in_default_box(&doc, index_of(&doc, WIDEST), LONGER);
    for alone in [&push, &wrap] {
        write(&mut doc.clone(), std::slice::from_ref(alone))
            .unwrap_or_else(|error| panic!("{}: {error}", alone.replacement));
    }
    let error = write(&mut doc.clone(), &[push, wrap]).unwrap_err();
    assert!(error.contains("another pending edit"), "{error}");
}
