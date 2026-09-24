//! Pushing the rest of a line along when the text typed into it grows past the
//! room the producer left.
//!
//! The fixtures are the synthetic font `layout_tests` uses, whose every glyph is
//! 600/1000 wide: a character at 12 pt is exactly 7.2 pt, so the arithmetic in
//! each test is written out rather than measured. The page is 300 x 240.
use super::layout_tests::{in_default_box, reader_line_starts, shows, synthetic};
use super::*;

// Two runs on one line, the first at x 40 and 36 pt wide, the second starting
// where the caller puts it. The room after the first run is therefore `at - 40`.
fn pair(at: f64) -> Document {
    synthetic(&format!(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf {at} 180 Td (FIRST) Tj ET"
    ))
}

// The run whose text is `text`, in discovery order.
fn find(doc: &Document, text: &str) -> Run {
    scan(doc, 0)
        .unwrap()
        .runs
        .into_iter()
        .find(|run| run.text == text)
        .unwrap_or_else(|| panic!("no run {text}"))
}

// The displacement a show is opened with, in thousandths of an em. The saved
// bytes are decoded again, so a whole number comes back as an integer.
fn leading(show: &Object) -> f64 {
    let Object::Array(items) = show else {
        panic!("not a TJ array: {show:?}")
    };
    match items.first() {
        Some(Object::Integer(value)) => *value as f64,
        Some(Object::Real(value)) => f64::from(*value),
        other => panic!("no displacement: {other:?}"),
    }
}

fn refusal(doc: &Document, index: usize, replacement: &str) -> String {
    let mut copy = doc.clone();
    let before = copy.objects.clone();
    let error = write(&mut copy, &[in_default_box(doc, index, replacement)])
        .expect_err("the edit was accepted");
    // Every refusal below is atomic: a line half pushed is worse than one not
    // pushed, so nothing may be left behind when any part of it is refused.
    assert_eq!(copy.objects, before, "{replacement} changed the document");
    error
}

// The text after the edit moves by exactly the distance the text ran past the
// room it had, and by nothing at all while it still fits that room.
#[test]
fn the_next_run_on_the_line_moves_by_what_the_text_ran_past_the_room() {
    // FIRST occupies 40..76 and the next run starts at 100: 60 pt of room from
    // the first run's own origin.
    let doc = pair(100.);
    let source = shows(&doc);
    // Eight of the run's own characters are 57.6 pt, inside that 60, so the box
    // grows and nothing is pushed: the gap the producer left is spent first.
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, "FIRSTFIR")]).unwrap();
    assert_eq!(
        shows(&fits).last(),
        source.last(),
        "an edit inside the room moved it"
    );
    assert_eq!(find(&fits, "FIRST").matrix[4], 100.);
    // Eleven are 79.2 pt, which is 19.2 pt past the room, so the next run goes
    // from 100 to 119.2.
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    let moved = find(&pushed, "FIRST");
    assert!(
        (moved.matrix[4] - 119.2).abs() < 0.0001,
        "{}",
        moved.matrix[4]
    );
    assert!((f64::from(moved.display_rect[0]) - 119.2).abs() < 0.0001);
    assert_eq!(find(&pushed, "FIRSTFIRSTF").matrix[4], 40.);
    // It moved by being opened with a displacement, and by nothing else: its
    // own glyphs and its own Td are the bytes the source wrote.
    let saved = shows(&pushed);
    assert_eq!(leading(saved.last().unwrap()), -1600.);
    let Object::Array(items) = saved.last().unwrap() else {
        panic!()
    };
    assert_eq!(items[1..], [Object::string_literal("FIRST")]);
}

// A run the editor may not rewrite does not move, and says so rather than
// reporting the box that the reader never set.
#[test]
fn text_the_editor_cannot_move_stops_the_push_and_names_itself() {
    // The second run draws back over itself (advance 36, then -36, then 7.2),
    // so it is not in reading order and is kept read-only.
    let fixed = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td [(FIRST) 3000 (F)] TJ ET",
    );
    assert_eq!(scan(&fixed, 0).unwrap().runs.len(), 1);
    let error = refusal(&fixed, 0, "FIRSTFIRSTF");
    assert!(error.contains("other text follows it"), "{error}");
    // The same edit against a run the editor may rewrite is accepted, so the
    // refusal is the neighbour and not the length.
    let source = pair(100.);
    let mut movable = source.clone();
    write(&mut movable, &[in_default_box(&source, 0, "FIRSTFIRSTF")]).unwrap();
    assert_eq!(find(&movable, "FIRSTFIRSTF").text, "FIRSTFIRSTF");
}

// The push stops where the run it is pushing would leave the page.
#[test]
fn the_page_edge_stops_the_push_beyond_the_run_that_moves() {
    // The next run occupies 240..276 on a 300 pt page, so it has 24 pt to give;
    // the first run has 200 pt of room, and 224 pt of text in all.
    let doc = pair(240.);
    // Thirty-one characters are 223.2 pt and fit; thirty-two are 230.4 and do not.
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, &"F".repeat(31))]).unwrap();
    assert!((find(&fits, "FIRST").matrix[4] - 263.2).abs() < 0.0001);
    let error = refusal(&doc, 0, &"F".repeat(32));
    assert!(error.contains("reaches the edge of the page"), "{error}");
}

// A clip over the run that would move is its own limit, measured where that run
// is rather than where the edited one is.
#[test]
fn a_clip_over_the_run_that_moves_stops_the_push_there() {
    // The clip reaches x 290 and covers both runs; the second occupies 240..276,
    // so it has 14 pt to give against the page's 24.
    let doc = synthetic(
        "q 30 170 260 30 re W n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 240 180 Td (FIRST) Tj ET Q",
    );
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, &"F".repeat(29))]).unwrap();
    assert!((find(&fits, "FIRST").matrix[4] - 248.8).abs() < 0.0001);
    let error = refusal(&doc, 0, &"F".repeat(30));
    assert!(error.contains("clips the space after it"), "{error}");
}

// A drawing is not in the hit list the box grows against, deliberately, and it
// is in the one the push measures: growing a box puts the reader's own text
// where they are watching it, while a push puts somebody else's somewhere they
// never asked for it to go.
#[test]
fn a_drawing_after_the_run_that_moves_stops_the_push_but_not_the_box() {
    // A filled rule at x 260..270, and the next run at 200..236 with 24 pt to
    // give before it; the first run has 160 pt of room.
    let doc = synthetic(
        "0 g 260 170 10 30 re f BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 200 180 Td (FIRST) Tj ET",
    );
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, &"F".repeat(25))]).unwrap();
    assert!((find(&fits, "FIRST").matrix[4] - 220.).abs() < 0.0001);
    let error = refusal(&doc, 0, &"F".repeat(26));
    assert!(
        error.contains("a picture or a drawing follows it"),
        "{error}"
    );
    // The same rule does not stop the box itself: with nothing to push, 26 of
    // the run's own characters grow straight over it.
    let alone = synthetic("0 g 260 170 10 30 re f BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let mut grown = alone.clone();
    write(&mut grown, &[in_default_box(&alone, 0, &"F".repeat(26))]).unwrap();
    assert_eq!(find(&grown, &"F".repeat(26)).matrix[4], 40.);
}

// A drawing that already holds the run -- a frame, a background -- lets it
// move as far as it still holds it; one that starts partway along the run
// stops it dead. Word draws a page border around the whole text block, and
// counting that as "a drawing follows it" refused every push on the page.
#[test]
fn a_drawing_holding_the_run_that_moves_allows_it_to_its_far_edge() {
    // The frame spans 30..280; the next run is at 200..236, so it may move 44
    // pt, and the first run has 160 pt of room: 204 in all.
    let framed = synthetic(
        "0 g 30 170 250 30 re f BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 200 180 Td (FIRST) Tj ET",
    );
    let mut fits = framed.clone();
    write(&mut fits, &[in_default_box(&framed, 0, &"F".repeat(28))]).unwrap();
    assert!((find(&fits, "FIRST").matrix[4] - 241.6).abs() < 0.0001);
    let error = refusal(&framed, 0, &"F".repeat(29));
    assert!(
        error.contains("a picture or a drawing follows it"),
        "{error}"
    );
    // Starting at 210, inside the run at 200..236, it holds nothing and the
    // run cannot move at all.
    let over = synthetic(
        "0 g 210 170 70 30 re f BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 200 180 Td (FIRST) Tj ET",
    );
    let error = refusal(&over, 0, &"F".repeat(23));
    assert!(
        error.contains("a picture or a drawing follows it"),
        "{error}"
    );
}

// An unpainted rectangle is a clip or a nothing, never a drawing, so it must not
// stop a push the way the filled one above does.
#[test]
fn a_path_that_paints_nothing_is_not_a_drawing() {
    let doc = synthetic(
        "260 170 10 30 re n BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 200 180 Td (FIRST) Tj ET",
    );
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, &"F".repeat(26))]).unwrap();
    assert!((find(&pushed, "FIRST").matrix[4] - 227.2).abs() < 0.0001);
}

// Everything after the edit on its own line moves; nothing on any other line
// does, and "nothing" is bit-identical rather than close.
//
// A displacement moves the text cursor and a `Td` moves the line matrix, which
// is the whole reason this holds: replay the saved stream in the reader's own
// single-precision arithmetic (`reader_line_starts`) and every following line
// starts on exactly the bits the source started it on.
#[test]
fn a_push_leaves_every_other_line_on_exactly_the_bits_the_source_had() {
    let doc = synthetic(
        "BT /F1 12 Tf 19.999992 200 Td (FIRST) Tj 100 0 Td (FIRST) Tj \
         -100 -30 Td (FIRST) Tj T* (FIRST) Tj 20 TL T* (FIRST) Tj ET",
    );
    let before = reader_line_starts(&doc);
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRSTFIRST")]).unwrap();
    let after = reader_line_starts(&pushed);
    // The first line's two shows are the edit and the run it pushed; the three
    // after it must be bit-identical.
    assert_eq!(before.len(), 5);
    assert_eq!(
        after.len(),
        6,
        "the edit writes one closing show of its own"
    );
    for (index, offset) in [(2_usize, 3_usize), (3, 4), (4, 5)] {
        assert_eq!(
            after[offset].1, before[index].1,
            "line {index} moved: {:?} against {:?}",
            after[offset].1, before[index].1
        );
    }
}

// A show that continues the line rides the cursor of the show before it, so it
// is pushed by that one having been pushed and must not be given a second
// displacement of its own.
#[test]
fn a_continued_show_is_carried_by_the_cursor_and_not_pushed_twice() {
    let doc = synthetic("BT /F1 12 Tf 40 180 Td (FIRST) Tj 60 0 Td (FIRST) Tj (FIRST) Tj ET");
    let source = shows(&doc);
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    let saved = shows(&pushed);
    // 60 pt of room, 79.2 pt of text: 19.2 pt of push. The positioned show is
    // opened with a displacement; the continued one is untouched, and both
    // end up 19.2 pt further along.
    assert_eq!(leading(&saved[2]), -1600.);
    assert_eq!(saved[3], source[2], "the continued show was rewritten");
    let runs = scan(&pushed, 0).unwrap().runs;
    let moved: Vec<f64> = runs
        .iter()
        .filter(|run| run.text == "FIRST")
        .map(|run| run.matrix[4])
        .collect();
    assert_eq!(moved.len(), 2);
    assert!((moved[0] - 119.2).abs() < 0.0001, "{moved:?}");
    assert!((moved[1] - 155.2).abs() < 0.0001, "{moved:?}");
}

// A read-only show continuing a pushed line would be dragged by that cursor
// without the editor ever deciding to move it, which is refused outright.
#[test]
fn a_read_only_show_on_the_pushed_cursor_refuses_the_whole_push() {
    let doc =
        synthetic("BT /F1 12 Tf 40 180 Td (FIRST) Tj 60 0 Td (FIRST) Tj [(FIRST) 3000 (F)] TJ ET");
    // The backtracking show is preserved, so only two runs are offered.
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 2);
    let error = refusal(&doc, 0, "FIRSTFIRSTF");
    assert!(error.contains("cannot be moved"), "{error}");
}

// Characters placed one at a time are one run with several shows, each carrying
// its own Td, so each of them needs a displacement of its own.
#[test]
fn a_grouped_run_moves_with_every_show_it_was_built_from() {
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj 60 0 Td (F) Tj 8 0 Td (I) Tj 8 0 Td (R) Tj ET",
    );
    assert_eq!(
        scan(&doc, 0)
            .unwrap()
            .runs
            .iter()
            .map(|run| run.text.clone())
            .collect::<Vec<_>>(),
        ["FIRST", "FIR"]
    );
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    let saved = shows(&pushed);
    // 19.2 pt of push at 12 pt is 1600 thousandths of an em, on every one of
    // the three shows: a member left behind would tear the word in half.
    for index in [2_usize, 3, 4] {
        assert_eq!(leading(&saved[index]), -1600., "show {index}");
    }
    assert!((find(&pushed, "FIR").matrix[4] - 119.2).abs() < 0.0001);
}

// Two edits on one line: the second is written where the first put it, and the
// run after both of them carries the sum.
#[test]
fn two_edits_on_one_line_compose() {
    // 40..76, 100..136 and 200..236: 60 pt of room for the first run and, once
    // it has been pushed, 100 for the second.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 200 180 Td (FIRST) Tj ET",
    );
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 3);
    let changes: Vec<Change> = [(0_usize, "FIRSTFIRSTF"), (1, "FIRSTFIRSTFIRST")]
        .iter()
        .map(|(index, text)| in_default_box(&doc, *index, text))
        .collect();
    // Sent the way a journal replays them, which is not stream order.
    let mut reversed = changes.clone();
    reversed.reverse();
    let mut both = doc.clone();
    write(&mut both, &reversed).unwrap();
    // The first edit is 79.2 pt against 60 of room, so it pushes everything
    // after it by 19.2. The second is then written at 119.2 with 100 pt of room
    // in front of it and 108 pt of text, so it pushes the third by another 8:
    // 200 + 19.2 + 8.
    assert_eq!(find(&both, "FIRSTFIRSTF").matrix[4], 40.);
    let second = find(&both, "FIRSTFIRSTFIRST").matrix[4];
    assert!((second - 119.2).abs() < 0.0001, "{second}");
    let third = find(&both, "FIRST").matrix[4];
    assert!((third - 227.2).abs() < 0.0001, "{third}");
    // One displacement each, measured from the source's own array rather than
    // added to the one the first edit wrote.
    let written = leading(shows(&both).last().unwrap());
    assert!(
        (written + 27.2 * 1000. / 12.).abs() < 0.01,
        "{written} against {}",
        -27.2 * 1000. / 12.
    );
}

// Undoing to the text that was there is the journal replaying a shorter set of
// changes against the source, so the run that was pushed has to come back to
// exactly where its own bytes put it -- which is why the displacement is always
// measured from the source's array rather than added to whatever is there.
#[test]
fn the_run_that_was_pushed_comes_back_to_its_own_bytes() {
    let doc = pair(100.);
    let source = shows(&doc);
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    assert_ne!(shows(&pushed)[2], source[1]);
    // The journal now holds the original text again, in the box the editor
    // opens, and is replayed against the source document.
    let mut undone = doc.clone();
    write(&mut undone, &[in_default_box(&doc, 0, "FIRST")]).unwrap();
    assert_eq!(shows(&undone)[2], source[1]);
    assert_eq!(
        find(&undone, "FIRST").display_rect,
        find(&doc, "FIRST").display_rect
    );
    assert_eq!(find(&undone, "FIRST").matrix, find(&doc, "FIRST").matrix);
}

// A box the reader sized is theirs, and a width they typed is not a licence to
// move anybody else's text.
#[test]
fn a_box_the_reader_sized_pushes_nothing() {
    let doc = pair(100.);
    let mut change = in_default_box(&doc, 0, "FIRSTFIRSTF");
    change.layout.as_mut().unwrap().grow = false;
    change.layout.as_mut().unwrap().width = 200.;
    let mut copy = doc.clone();
    let error = write(&mut copy, &[change]).expect_err("a sized box pushed the line");
    assert!(error.contains("overlap another line"), "{error}");
    assert_eq!(copy.objects, doc.objects);
}

// The preview's crop covers the run that moved as well as the box, or the
// reader watches their own text grow while the word it displaced is cut in half.
#[test]
fn the_preview_extent_covers_the_run_the_draft_pushes() {
    let doc = pair(100.);
    let grown = preview_layout(&doc, &in_default_box(&doc, 0, "FIRSTFIRSTF")).unwrap();
    // The box is the text, 40..119.2; the pushed run ends at 155.2.
    assert!(
        (f64::from(grown.rect[2]) - 119.2).abs() < 0.01,
        "{:?}",
        grown.rect
    );
    assert!(
        (f64::from(grown.extent[2]) - 155.2).abs() < 0.01,
        "{:?}",
        grown.extent
    );
    // A draft that pushes nothing reports the two the same.
    let same = preview_layout(&doc, &in_default_box(&doc, 0, "FIRSTFIR")).unwrap();
    assert_eq!(same.rect, same.extent);
}

// Something the editor cannot move separates the runs on either side of it: the
// one beyond it never has to give way, so it is not pushed and not counted as
// room either.
#[test]
fn a_run_beyond_text_that_cannot_move_is_left_alone() {
    // 40..76, a movable run at 100, a read-only one at 160, and another movable
    // one at 220. The push owns the first of those and nothing past the wall.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td (FIRST) Tj ET \
         BT /F1 12 Tf 160 180 Td [(FIRST) 3000 (F)] TJ ET \
         BT /F1 12 Tf 220 180 Td (FIRST) Tj ET",
    );
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 3);
    let at = |doc: &Document| {
        let mut xs: Vec<f64> = scan(doc, 0)
            .unwrap()
            .runs
            .iter()
            .filter(|run| !run.text.is_empty())
            .map(|run| run.matrix[4])
            .collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    assert_eq!(at(&doc), [40., 100., 220.]);
    // The run at 100 ends at 136 and the wall starts at 160: 24 pt of push, so
    // the first run may reach 84 pt of text. Eleven characters are 79.2.
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    let moved = at(&fits);
    assert!((moved[1] - 119.2).abs() < 0.0001, "{moved:?}");
    assert_eq!(moved[2], 220., "the run past the wall moved: {moved:?}");
    // Twelve are 86.4, which is more room than the wall leaves.
    let error = refusal(&doc, 0, "FIRSTFIRSTFI");
    assert!(error.contains("cannot be moved"), "{error}");
}

// Drawing order is not reading order: a run written earlier in the stream sits
// wherever its own matrix puts it, and this edit has already gone past its
// cursor, so it is something to stop at rather than something to move.
#[test]
fn a_run_drawn_before_the_edit_is_not_pushed() {
    let doc =
        synthetic("BT /F1 12 Tf 100 180 Td (FIRST) Tj ET BT /F1 12 Tf 40 180 Td (FIRST) Tj ET");
    let page = scan(&doc, 0).unwrap();
    let index = page
        .runs
        .iter()
        .position(|run| run.matrix[4] == 40.)
        .unwrap();
    let mut copy = doc.clone();
    let error = write(&mut copy, &[in_default_box(&doc, index, "FIRSTFIRSTF")])
        .expect_err("a run drawn earlier was pushed");
    assert!(error.contains("other text follows it"), "{error}");
    assert_eq!(copy.objects, doc.objects);
}

// A run on the same line but behind the edit does not move either, however late
// in the stream it is drawn.
#[test]
fn a_run_behind_the_edit_on_its_own_line_is_not_pushed() {
    // The single glyph at 10 is drawn after the edited run and sits behind it;
    // the run at 100 is what the edit actually pushes.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 10 180 Td (F) Tj ET \
         BT /F1 12 Tf 100 180 Td (FIRST) Tj ET",
    );
    let page = scan(&doc, 0).unwrap();
    let index = page
        .runs
        .iter()
        .position(|run| run.matrix[4] == 40.)
        .unwrap();
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, index, &"F".repeat(30))]).unwrap();
    // Thirty characters are 216 pt against 60 of room: 156 pt of push.
    assert!((find(&pushed, "FIRST").matrix[4] - 256.).abs() < 0.0001);
    assert_eq!(find(&pushed, "F").matrix[4], 10.);
    assert_eq!(
        find(&pushed, "F").display_rect,
        find(&doc, "F").display_rect
    );
}

// A run whose array already opens with a displacement -- an indented or
// justified line, which is how pdfTeX writes most of them -- has that number
// replaced rather than a second one added, and the replacement is measured from
// the source's own value so the run ends up where both together put it.
#[test]
fn a_neighbour_that_already_opens_with_a_displacement_keeps_it() {
    // -1000 thousandths at 12 pt is 12 pt of indent, so the second run's text
    // starts at 112 with 72 pt of room in front of it.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td [-1000 (FIRST)] TJ ET",
    );
    // Both runs read FIRST, so the neighbour is the further along of the two.
    let at = |doc: &Document| {
        scan(doc, 0)
            .unwrap()
            .runs
            .iter()
            .map(|run| run.matrix[4])
            .fold(f64::MIN, f64::max)
    };
    assert_eq!(at(&doc), 112.);
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, &"F".repeat(12))]).unwrap();
    // Twelve characters are 86.4 pt against 72 of room: 14.4 pt of push, which
    // is 1200 thousandths on top of the 1000 the source already wrote.
    assert_eq!(leading(shows(&pushed).last().unwrap()), -2200.);
    assert!((at(&pushed) - 126.4).abs() < 0.0001, "{}", at(&pushed));
}

// A displacement large enough that a PDF number cannot carry it is refused
// rather than written: the saved file would be one no reader could discover,
// because `number` refuses the same magnitude on the way back in.
#[test]
fn a_displacement_beyond_a_pdf_number_refuses_the_push() {
    // The neighbour is set at Tf 0.1 under a matrix scaled 100 in y, so it is
    // tall enough to be on the line while a point of movement costs it ten
    // thousand thousandths of an em.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         BT /F1 0.1 Tf 1 0 0 100 100 180 Tm (FIRST) Tj ET",
    );
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 2);
    // 25 characters are 180 pt against 60 of room: 120 pt of push, and
    // 120 / (0.1 / 1000) is 1.2 million.
    let error = refusal(&doc, 0, &"F".repeat(25));
    assert!(error.contains("PDF number precision"), "{error}");
    // Sixteen characters are 115.2 pt, so 55.2 pt of push, and that number is
    // inside the range and is written.
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, &"F".repeat(16))]).unwrap();
    assert_eq!(leading(shows(&fits).last().unwrap()), -552000.);
}

// A read-only show that draws nothing is in no hit list -- `reach` cannot stop
// the push at it, because it has no rectangle worth stopping at -- and it still
// rides the cursor of the show before it. Dragging it is moving text the editor
// never decided to move, so the whole push is refused.
#[test]
fn an_invisible_read_only_show_on_the_cursor_refuses_the_push() {
    // Helvetica, which has a space; the trailing show draws two of them and
    // then draws back over itself, which keeps it read-only and blank.
    let doc = tests::with_content(
        b"BT /F1 12 Tf 40 180 Td (TITLE) Tj 60 0 Td (TITLE) Tj [( ) 3000 ( )] TJ ET",
    );
    let page = scan(&doc, 0).unwrap();
    assert_eq!(
        page.runs
            .iter()
            .map(|run| run.text.clone())
            .collect::<Vec<_>>(),
        ["TITLE", "TITLE"]
    );
    let error = refusal(&doc, 0, "TITLE TITLE TITLE");
    assert!(error.contains("cannot be moved"), "{error}");
}

// An edit in the middle of a pushed line ends by putting the cursor back where
// the source left it, so the show after it does not inherit the push and has to
// be given a displacement of its own -- even when that edit moves nothing.
#[test]
fn an_edit_in_the_middle_of_a_pushed_line_does_not_carry_the_push() {
    // 40..76, then a run at 100 whose own continued show follows it at 136.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td (FIRST) Tj (FIRST) Tj ET",
    );
    let page = scan(&doc, 0).unwrap();
    assert_eq!(page.runs.len(), 3);
    let changes = [
        // Eleven characters are 79.2 pt against 60 of room: 19.2 pt of push.
        in_default_box(&doc, 0, "FIRSTFIRSTF"),
        // The middle run keeps its own length, so it pushes nothing itself.
        in_default_box(&doc, 1, "FIRSTFIRSTF"),
    ];
    let mut both = doc.clone();
    let same = Change {
        replacement: "TSRIF".into(),
        ..changes[1].clone()
    };
    write(&mut both, &[changes[0].clone(), same]).unwrap();
    let mut xs: Vec<f64> = scan(&both, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| run.matrix[4])
        .collect();
    xs.sort_by(f64::total_cmp);
    // The edit, the same-length edit 19.2 on, and its continued show 19.2 on.
    assert_eq!(xs.len(), 3);
    assert_eq!(xs[0], 40.);
    assert!((xs[1] - 119.2).abs() < 0.0001, "{xs:?}");
    assert!((xs[2] - 155.2).abs() < 0.0001, "{xs:?}");
}

// The push is measured from the nearest run it moves, which is not the room the
// box had: `room` skips a neighbour that begins inside the box the reader
// already has, because the box may never shrink, and the box the editor opens
// is the run's own advance rounded **up**. A run set flush against the end of
// this one therefore starts a rounding inside it and is skipped every time --
// and measuring the push from what `room` returned instead moved it by the
// distance the run after it needed, which is a whole word too little.
//
// Found on a Word minute where `7:00 p.m.` is three runs: the saved line read
// back through poppler as `7:0called 0 thp.m.`, with the pushed `0` sitting
// between the last two glyphs of the replacement.
#[test]
fn the_push_is_measured_from_the_run_it_moves_not_from_the_room() {
    // At 11.1 pt each glyph is 6.66 pt and FIRST is 33.3 minus a rounding, so
    // the box the editor opens is a hair wider than the run's own advance and
    // the neighbour set flush against it begins inside that box.
    let doc = synthetic(
        "BT /F1 11.1 Tf 40 180 Td (FIRST) Tj ET BT /F1 11.1 Tf 73.3 180 Td (FIRST) Tj ET \
         BT /F1 11.1 Tf 140 180 Td (FIRST) Tj ET",
    );
    let page = scan(&doc, 0).unwrap();
    assert_eq!(page.runs.len(), 3);
    // The advance is a hair over 33.3 because lopdf stores 11.1 as an f32, and
    // the box rounds that up again to the next thousandth, so the neighbour set
    // at 73.3 starts inside it.
    let advance = page.runs[0].advance;
    assert!((33.3..33.301).contains(&advance), "{advance}");
    let mut pushed = doc.clone();
    // Ten of the run's own characters are 66.6 pt, so the text ends at 106.6 and
    // the run that was flush against it has to start there.
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRST")]).unwrap();
    let mut xs: Vec<f64> = scan(&pushed, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| run.matrix[4])
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs.len(), 3);
    assert_eq!(xs[0], 40.);
    assert!((xs[1] - 106.6).abs() < 0.01, "{xs:?}");
    // The run behind it keeps the gap it had: 140 - 73.3 = 66.7 pt.
    assert!((xs[2] - xs[1] - 66.7).abs() < 0.01, "{xs:?}");
}

// The same flush neighbour with nothing after it on the line. `room` skips it,
// so the box's own limit and the limit with every movable run gone are the same
// page edge, and the push used to conclude from that alone that nothing movable
// was in the way -- leaving the run where it was, under the new text. Across the
// public sample that was half of every line whose rest flowed after a wrap.
#[test]
fn a_flush_neighbour_with_nothing_after_it_is_still_pushed() {
    let doc = synthetic(
        "BT /F1 11.1 Tf 40 180 Td (FIRST) Tj ET BT /F1 11.1 Tf 73.3 180 Td (FIRST) Tj ET",
    );
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 2);
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, "FIRSTFIRST")]).unwrap();
    let mut xs: Vec<f64> = scan(&pushed, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| run.matrix[4])
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs.len(), 2);
    assert_eq!(xs[0], 40.);
    assert!((xs[1] - 106.6).abs() < 0.01, "{xs:?}");
    // The run can go as far as the page's 300: 193.4 pt, so the box holds
    // 33.3 + 193.4 = 226.7 pt, thirty-four glyphs of 6.66 and not thirty-five.
    // The page edge `room` saw past the skipped run is not the box's limit.
    let mut fits = doc.clone();
    write(&mut fits, &[in_default_box(&doc, 0, &"F".repeat(34))]).unwrap();
    let error = refusal(&doc, 0, &"F".repeat(35));
    assert!(error.contains("reaches the edge of the page"), "{error}");
}

// A push may be as long as the text grew and no longer.
//
// The run the push starts from is admitted when its near edge is within the
// tenth of a point every other check here ignores, because the box the editor
// opens is the run's advance rounded up and a run set flush against it lands
// just inside. Measuring the push from that edge then moves the line by that
// tenth even when the text did not grow at all -- which refused a ReportLab
// line its own unchanged text, for a wall the reader was nowhere near.
#[test]
fn an_unchanged_edit_pushes_nothing_even_where_the_next_run_starts_a_hair_early() {
    // FIRST runs 40..76, so the box opens at 36 and the second run at 75.95 is
    // five hundredths inside it. Its clip ends two hundredths past it, which is
    // all the room the push has; the third run at 130 is what makes a push
    // reachable at all.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET \
         q 70 170 41.97 30 re W n BT /F1 12 Tf 75.95 180 Td (FIRST) Tj ET Q \
         BT /F1 12 Tf 130 180 Td (FIRST) Tj ET",
    );
    let before = scan(&doc, 0).unwrap();
    assert_eq!(before.runs.len(), 3);
    assert_eq!(before.runs[0].display_rect[2], 76.);
    let mut same = doc.clone();
    write(&mut same, &[in_default_box(&doc, 0, "FIRST")]).unwrap();
    // The edit writes one empty show of its own, so compare what is visible.
    let visible = |page: &PageRuns| {
        page.runs
            .iter()
            .filter(|run| !run.text.is_empty())
            .skip(1)
            .map(|run| (run.display_rect, run.matrix))
            .collect::<Vec<_>>()
    };
    let after = scan(&same, 0).unwrap();
    assert_eq!(visible(&after).len(), 2);
    assert_eq!(visible(&after), visible(&before));
}

// Hit rectangles are em boxes, and at 12 pt on a 14 pt pitch the next line's
// overlaps this one's by a point. That is not the same line: a run on the line
// below, starting past the end of the edited one, stays where it is. The push
// used to count anything overlapping by more than a tenth of a point, and moved
// it along with its own line.
#[test]
fn the_next_line_is_not_pushed_along_with_this_one() {
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 166 Td (SECOND) Tj ET \
         BT /F1 12 Tf 200 180 Td (THIRD) Tj ET",
    );
    // FIRST ends at 76; SECOND starts at 100 on the line below, THIRD at 200
    // on this one. Eleven characters end at 119.2, past SECOND's start and
    // short of THIRD's.
    let mut saved = doc.clone();
    write(&mut saved, &[in_default_box(&doc, 0, "FIRSTFIRSTF")]).unwrap();
    let second = find(&saved, "SECOND");
    assert_eq!((second.matrix[4], second.matrix[5]), (100., 166.));
    assert_eq!(find(&saved, "THIRD").matrix[4], 200.);
    // Twenty-three reach 205.6 and push THIRD, on this line, by 5.6 pt; SECOND
    // still stays.
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, &"F".repeat(23))]).unwrap();
    let third = find(&pushed, "THIRD");
    assert!(
        (third.matrix[4] - 205.6).abs() < 0.0001,
        "{}",
        third.matrix[4]
    );
    assert_eq!(find(&pushed, "SECOND").matrix[4], 100.);
}

// The mirror: what stops a pushed run is what is on *its* line. Text the editor
// cannot move, on the line below and ahead of the run being pushed, is a point
// into that run's em box at a 14 pt pitch and is not in its way.
#[test]
fn text_on_the_next_line_does_not_stop_a_push() {
    // THIRD at 100..136 on this line; a read-only run (it draws back over
    // itself) at 150 on the line below.
    let doc = synthetic(
        "BT /F1 12 Tf 40 180 Td (FIRST) Tj ET BT /F1 12 Tf 100 180 Td (THIRD) Tj ET \
         BT /F1 12 Tf 150 166 Td [(FIRST) 3000 (F)] TJ ET",
    );
    assert_eq!(scan(&doc, 0).unwrap().runs.len(), 2);
    // Twelve characters end at 126.4: THIRD goes to 126.4..162.4, over the
    // read-only run's start at 150, but on its own line.
    let mut pushed = doc.clone();
    write(&mut pushed, &[in_default_box(&doc, 0, &"F".repeat(12))])
        .unwrap_or_else(|error| panic!("{error}"));
    let third = find(&pushed, "THIRD");
    assert!(
        (third.matrix[4] - 126.4).abs() < 0.0001,
        "{}",
        third.matrix[4]
    );
}
