//! Tests of `glyph_cut.rs`.
//!
//! The glyphs are written by hand: ten units wide, on a baseline at y = 100,
//! with pens a round number of thousandths apart. What PDFium reports for a
//! real page is `objects.rs`'s to read and `tests/cli/redact.rs`'s to hold to
//! the pinned engine; everything here is the arithmetic after that.

use lopdf::content::Content;
use lopdf::{dictionary, Document, ObjectId, Stream};

use super::*;
use crate::objects::{Glyph, TextGlyphs};

/// `text` as glyphs 10 wide and 10 high from x = 0, pens 500 apart. A space
/// has no box.
fn glyphs(text: &str) -> TextGlyphs {
    TextGlyphs {
        glyphs: text
            .chars()
            .enumerate()
            .map(|(at, c)| Glyph {
                draws: c.to_string(),
                bounds: (c != ' ').then(|| {
                    let left = at as f32 * 10.0;
                    [left, 100.0, left + 10.0, 110.0]
                }),
                pen: at as f32 * 500.0,
            })
            .collect(),
        tail: None,
    }
}

/// A region over glyphs `from..to` of [`glyphs`], and all of their height.
fn over(from: usize, to: usize) -> Rect {
    [from as f32 * 10.0, 90.0, to as f32 * 10.0, 120.0]
}

fn fact(codes: usize) -> Option<ShowFacts> {
    Some(ShowFacts {
        codes,
        carries_on: false,
    })
}

/// A plan that takes show 0 whole, as `covered` leaves it.
fn whole() -> Plan {
    Plan {
        shows: vec![0],
        ..Plan::default()
    }
}

#[test]
fn a_region_over_some_glyphs_takes_those_and_no_others() {
    let text = glyphs("abcdef");
    assert_eq!(taken(&text.glyphs, over(2, 4)), Some(vec![2, 3]));
}

#[test]
fn a_glyph_goes_when_more_than_a_tenth_of_it_is_under_the_region() {
    let text = glyphs("abc");
    // From x = 8.5: the last 1.5 of `a`'s 10, which is more than a tenth.
    assert_eq!(
        taken(&text.glyphs, [8.5, 90.0, 20.0, 120.0]),
        Some(vec![0, 1])
    );
    // From x = 9.5: half a unit of `a`, which is less.
    assert_eq!(taken(&text.glyphs, [9.5, 90.0, 20.0, 120.0]), Some(vec![1]));
    // And the same rule across the line: a region over the top 0.5 of every
    // glyph takes none of them.
    assert_eq!(
        taken(&text.glyphs, [0.0, 109.5, 30.0, 130.0]),
        Some(Vec::new())
    );
}

#[test]
fn a_region_drawn_from_its_other_corner_takes_the_same_glyphs() {
    let text = glyphs("abcdef");
    assert_eq!(
        taken(&text.glyphs, [40.0, 120.0, 20.0, 90.0]),
        Some(vec![2, 3])
    );
}

/// The space inside what goes, goes; the space beside it stays.
#[test]
fn a_space_goes_with_the_glyphs_on_both_sides_of_it() {
    let text = glyphs("ab cd ef");
    // `cd`, and neither space: each has a neighbour that stays.
    assert_eq!(taken(&text.glyphs, over(3, 5)), Some(vec![3, 4]));
    // `cd ef`: the space between them goes, the one before `c` stays.
    assert_eq!(taken(&text.glyphs, over(3, 8)), Some(vec![3, 4, 5, 6, 7]));
    // A space at the end goes with the word before it.
    let trailing = glyphs("ab ");
    assert_eq!(taken(&trailing.glyphs, over(0, 2)), Some(vec![0, 1, 2]));
    // A show of spaces alone has nothing a region can be over.
    assert_eq!(taken(&glyphs("  ").glyphs, over(0, 2)), Some(Vec::new()));
}

/// **A letter with no box cannot be said to be outside the region.**
#[test]
fn a_glyph_with_no_box_that_is_not_white_space_cannot_be_cut_around() {
    let mut text = glyphs("abc");
    text.glyphs[2].bounds = None;
    assert_eq!(taken(&text.glyphs, over(0, 1)), None);
    // A box of no area is no box.
    text.glyphs[2].bounds = Some([20.0, 100.0, 20.0, 110.0]);
    assert_eq!(taken(&text.glyphs, over(0, 1)), None);
}

#[test]
fn a_show_partly_under_a_region_becomes_a_cut_and_says_what_it_takes() {
    let mut plan = whole();
    let said = cut_within(
        &mut plan,
        &[Some(glyphs("abcdef"))],
        Some(&[fact(6)]),
        over(2, 4),
    );
    assert_eq!(said, vec![(0, "cd".to_string())]);
    assert!(plan.shows.is_empty());
    assert_eq!(
        plan.show_cuts,
        vec![ShowCut {
            ordinal: 0,
            pens: vec![0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0],
            tail: None,
            take: vec![2, 3],
        }]
    );
}

#[test]
fn a_show_wholly_under_a_region_still_goes_whole() {
    let mut plan = whole();
    let said = cut_within(
        &mut plan,
        &[Some(glyphs("abc"))],
        Some(&[fact(3)]),
        over(0, 3),
    );
    assert!(said.is_empty());
    assert_eq!(plan.shows, vec![0]);
    assert!(plan.show_cuts.is_empty());
}

/// Its box overlaps the region and none of its ink does: it is left alone.
#[test]
fn a_show_with_no_glyph_under_the_region_leaves_the_plan() {
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[Some(glyphs("abc"))],
        Some(&[fact(3)]),
        [0.0, 109.5, 30.0, 130.0],
    );
    assert!(plan.shows.is_empty() && plan.show_cuts.is_empty());
}

/// Each reason a show cannot be cut leaves it going whole, as it did before.
#[test]
fn a_show_that_cannot_be_described_goes_whole() {
    let region = over(2, 4);
    let cuttable = Some(glyphs("abcdef"));
    // The control: this one is cut.
    let mut plan = whole();
    cut_within(
        &mut plan,
        std::slice::from_ref(&cuttable),
        Some(&[fact(6)]),
        region,
    );
    assert_eq!(plan.show_cuts.len(), 1);

    let mut unboxed = glyphs("abcdef");
    unboxed.glyphs[5].bounds = None;
    type Case = (
        &'static str,
        Option<TextGlyphs>,
        Option<Vec<Option<ShowFacts>>>,
    );
    let cases: [Case; 5] = [
        ("PDFium could not place it", None, Some(vec![fact(6)])),
        ("the content could not be read", cuttable.clone(), None),
        (
            "its font's codes cannot be counted",
            cuttable.clone(),
            Some(vec![None]),
        ),
        (
            "it holds more codes than glyphs",
            cuttable.clone(),
            Some(vec![fact(7)]),
        ),
        ("a letter has no box", Some(unboxed), Some(vec![fact(6)])),
    ];
    for (why, text, facts) in cases {
        let mut plan = whole();
        let said = cut_within(&mut plan, &[text], facts.as_deref(), region);
        assert!(said.is_empty(), "{why}");
        assert_eq!(plan.shows, vec![0], "{why}");
        assert!(plan.show_cuts.is_empty(), "{why}");
    }
}

/// The last glyph goes and the next show starts at this one's pen: the gap
/// needs the place PDFium put that next show, and without it the show goes
/// whole.
#[test]
fn a_cut_to_the_end_needs_the_next_start_only_when_the_pen_carries_on() {
    let carried = Some(ShowFacts {
        codes: 4,
        carries_on: true,
    });
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[Some(glyphs("abcd"))],
        Some(&[carried]),
        over(2, 4),
    );
    assert_eq!(plan.shows, vec![0], "no tail, and the pen carries on");

    let mut with_tail = glyphs("abcd");
    with_tail.tail = Some(2000.0);
    let mut plan = whole();
    cut_within(&mut plan, &[Some(with_tail)], Some(&[carried]), over(2, 4));
    assert_eq!(plan.show_cuts.len(), 1, "a tail");

    let mut plan = whole();
    cut_within(
        &mut plan,
        &[Some(glyphs("abcd"))],
        Some(&[fact(4)]),
        over(2, 4),
    );
    assert_eq!(plan.show_cuts.len(), 1, "the pen does not carry on");

    // And a cut that stops short of the end never needs it.
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[Some(glyphs("abcd"))],
        Some(&[carried]),
        over(1, 3),
    );
    assert_eq!(plan.show_cuts.len(), 1, "the last glyph stays");
}

fn cut(ordinal: usize, glyphs: usize, take: &[usize]) -> ShowCut {
    ShowCut {
        ordinal,
        pens: (0..glyphs).map(|at| at as f32 * 500.0).collect(),
        tail: None,
        take: take.to_vec(),
    }
}

#[test]
fn two_regions_over_one_show_cut_it_once_for_both() {
    let mut shows = vec![7];
    let merged = merge_cuts(vec![cut(3, 6, &[4, 5]), cut(3, 6, &[0, 1])], &mut shows);
    assert_eq!(merged, vec![cut(3, 6, &[0, 1, 4, 5])]);
    assert_eq!(shows, vec![7]);
}

#[test]
fn a_show_another_region_takes_whole_is_not_also_cut() {
    let mut shows = vec![3];
    assert!(merge_cuts(vec![cut(3, 6, &[0])], &mut shows).is_empty());
    assert_eq!(shows, vec![3]);
}

#[test]
fn cuts_that_add_up_to_the_whole_show_take_it_whole() {
    let mut shows = vec![9];
    let merged = merge_cuts(
        vec![cut(3, 4, &[0, 1]), cut(5, 4, &[0]), cut(3, 4, &[2, 3])],
        &mut shows,
    );
    assert_eq!(merged, vec![cut(5, 4, &[0])]);
    assert_eq!(shows, vec![3, 9], "sorted, for `remove_shows`");
}

/// One page whose content is `stream`, with a simple font `F1` and a
/// composite font `F2` under `Identity-H`, `F3` under another CMap.
fn one_page(stream: &str) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let simple = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let wide = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Wide",
        "Encoding" => "Identity-H",
    });
    let other = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Other",
        "Encoding" => "UniJIS-UTF16-H",
    });
    let content = doc.add_object(Stream::new(dictionary! {}, stream.as_bytes().to_vec()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => simple, "F2" => wide, "F3" => other },
        },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    (doc, page)
}

/// The page's content after a change, as text, one space between tokens.
fn content_of(doc: &Document, page: ObjectId) -> String {
    let data = doc.get_page_content(page);
    let encoded = Content::decode(&data)
        .expect("decodes")
        .encode()
        .expect("encodes");
    // `lopdf` writes a number and the string after it with nothing between,
    // which is legal and hard to read: a space is put back.
    let mut spaced = String::new();
    for c in String::from_utf8_lossy(&encoded).chars() {
        if matches!(c, '(' | '<') && spaced.ends_with(|last: char| last.is_ascii_digit()) {
            spaced.push(' ');
        }
        spaced.push(c);
    }
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts `take` out of the one show of `stream` and returns the content.
fn after(stream: &str, glyphs: usize, take: &[usize], tail: Option<f32>) -> String {
    let (mut doc, page) = one_page(stream);
    let mut one = cut(0, glyphs, take);
    one.tail = tail;
    cut_shows(&mut doc, page, &[one], 1).expect("cut");
    content_of(&doc, page)
}

#[test]
fn a_run_in_the_middle_becomes_a_gap_as_wide_as_it_was() {
    // `c` and `d` stood at 1000 and 1500 and `e` stands at 2000, so the pen
    // has to move 1000 forwards, which a `TJ` writes as -1000.
    assert_eq!(
        after("BT /F1 12 Tf (abcdef) Tj ET", 6, &[2, 3], None),
        "BT /F1 12 Tf [(ab) -1000 (ef)] TJ ET"
    );
}

#[test]
fn a_run_at_the_start_leaves_its_gap_before_what_stays() {
    assert_eq!(
        after("BT /F1 12 Tf (abcdef) Tj ET", 6, &[0, 1], None),
        "BT /F1 12 Tf [-1000 (cdef)] TJ ET"
    );
}

#[test]
fn two_runs_leave_two_gaps() {
    assert_eq!(
        after("BT /F1 12 Tf (abcdef) Tj ET", 6, &[1, 4], None),
        "BT /F1 12 Tf [(a) -500 (cd) -500 (f)] TJ ET"
    );
}

/// The numbers of a `TJ`: one inside a run is part of the distance between the
/// two pens, and one outside it stays.
#[test]
fn a_kern_inside_what_goes_is_dropped_and_one_outside_it_stays() {
    assert_eq!(
        after(
            "BT /F1 12 Tf [(ab) -50 (c) 30 (d) -20 (ef)] TJ ET",
            6,
            &[2, 3],
            None
        ),
        "BT /F1 12 Tf [(ab) -50 -1000 (ef)] TJ ET"
    );
}

/// Nothing follows that starts at the pen, so the end needs no gap.
#[test]
fn a_run_to_the_end_needs_no_gap_when_the_next_show_is_placed() {
    let (mut doc, page) = one_page("BT /F1 12 Tf (abcd) Tj 0 -14 Td (next) Tj ET");
    let mut one = cut(0, 4, &[2, 3]);
    one.tail = Some(9000.0);
    cut_shows(&mut doc, page, &[one], 2).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf [(ab)] TJ 0 -14 Td (next) Tj ET"
    );
}

/// The next show starts at the pen, so the pen is moved to where PDFium put
/// that show: from `c` at 1000 to the tail at 2600.
#[test]
fn a_run_to_the_end_leaves_a_gap_up_to_a_show_that_starts_at_the_pen() {
    let (mut doc, page) = one_page("BT /F1 12 Tf (abcd) Tj 0 Tc (next) Tj ET");
    let mut one = cut(0, 4, &[2, 3]);
    one.tail = Some(2600.0);
    cut_shows(&mut doc, page, &[one], 2).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf [(ab) -1600] TJ 0 Tc (next) Tj ET"
    );
}

/// `'` and `"` do more than show: the line moves, and `"` sets two spacings.
#[test]
fn a_quote_operator_keeps_what_it_did_before_showing() {
    assert_eq!(
        after("BT /F1 12 Tf 14 TL (abcd) ' ET", 4, &[0], None),
        "BT /F1 12 Tf 14 TL T* [-500 (bcd)] TJ ET"
    );
    assert_eq!(
        after("BT /F1 12 Tf 14 TL 2 1 (abcd) \" ET", 4, &[3], None),
        "BT /F1 12 Tf 14 TL 2 Tw 1 Tc T* [(abc)] TJ ET"
    );
}

/// Two bytes a code under `Identity-H`: glyph 1 is the second pair.
#[test]
fn a_composite_font_is_cut_between_its_two_byte_codes() {
    assert_eq!(
        after("BT /F2 12 Tf <000100020003> Tj ET", 3, &[1], None),
        "BT /F2 12 Tf [<0001> -500 <0003>] TJ ET"
    );
}

#[test]
fn the_font_in_force_follows_save_and_restore() {
    // `F2` is set inside `q ... Q`, so the show after `Q` is in `F1` again and
    // its six bytes are six codes.
    assert_eq!(
        after("/F1 12 Tf q /F2 12 Tf Q BT (abcdef) Tj ET", 6, &[5], None),
        "/F1 12 Tf q /F2 12 Tf Q BT [(abcde)] TJ ET"
    );
}

#[test]
fn only_the_named_show_is_cut_and_empty_shows_are_passed_over() {
    let (mut doc, page) =
        one_page("BT /F1 12 Tf () Tj (one) Tj 0 -14 Td (two) Tj 0 -14 Td (three) Tj ET");
    cut_shows(&mut doc, page, &[cut(1, 3, &[0])], 3).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf () Tj (one) Tj 0 -14 Td [-500 (wo)] TJ 0 -14 Td (three) Tj ET"
    );
}

/// **Backwards, and two `'` are what show it.** Each becomes two operations,
/// so cutting the first one first moves the second, and the second cut then
/// lands on the `T*` the first one wrote.
#[test]
fn two_cuts_on_one_page_each_land_on_their_own_show() {
    let (mut doc, page) = one_page("BT /F1 12 Tf 14 TL (abcd) ' (efgh) ' ET");
    cut_shows(&mut doc, page, &[cut(0, 4, &[0]), cut(1, 4, &[3])], 2).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf 14 TL T* [-500 (bcd)] TJ T* [(efg)] TJ ET"
    );
}

/// Each refusal, and the content untouched by it.
#[test]
fn a_cut_that_does_not_fit_its_show_is_refused_and_writes_nothing() {
    let stream = "BT /F1 12 Tf (abcdef) Tj ET";
    let cases: [(&str, Vec<ShowCut>, usize, &str); 5] = [
        (
            "more glyphs than codes",
            vec![cut(0, 7, &[1])],
            1,
            "not shaped",
        ),
        (
            "fewer glyphs than codes",
            vec![cut(0, 5, &[1])],
            1,
            "not shaped",
        ),
        (
            "a show that is not there",
            vec![cut(4, 6, &[1])],
            1,
            "no show operator 4",
        ),
        (
            "a count that disagrees",
            vec![cut(0, 6, &[1])],
            2,
            "text-showing operator(s)",
        ),
        (
            "one show cut twice",
            vec![cut(0, 6, &[1]), cut(0, 6, &[2])],
            1,
            "cut twice",
        ),
    ];
    for (what, cuts, objects, says) in cases {
        let (mut doc, page) = one_page(stream);
        let before = content_of(&doc, page);
        let why = cut_shows(&mut doc, page, &cuts, objects).expect_err(what);
        assert!(why.contains(says), "{what}: {why}");
        assert_eq!(content_of(&doc, page), before, "{what}");
    }

    // A font whose codes cannot be counted.
    let (mut doc, page) = one_page("BT /F3 12 Tf (abcdef) Tj ET");
    let why = cut_shows(&mut doc, page, &[cut(0, 6, &[1])], 1).expect_err("unknown CMap");
    assert!(why.contains("codes this cannot count"), "{why}");
}

/// The alternate text of the span around a cut show restates what went.
#[test]
fn the_alternate_text_around_a_cut_show_is_cleared() {
    let (mut doc, page) =
        one_page("/Span <</ActualText (abcdef)>> BDC BT /F1 12 Tf (abcdef) Tj ET EMC");
    let done = cut_shows(&mut doc, page, &[cut(0, 6, &[2, 3])], 1).expect("cut");
    assert_eq!((done.removed, done.carriers), (1, 1));
    assert!(!content_of(&doc, page).contains("ActualText"));
}

#[test]
fn the_facts_of_each_show_are_its_codes_and_whether_the_pen_carries_on() {
    let (doc, page) = one_page(
        "BT /F1 12 Tf (abc) Tj () Tj [(de) -20 (f)] TJ 0 -14 Td \
         /F2 12 Tf <00010002> Tj /F3 12 Tf (zz) Tj /F2 12 Tf <000102> Tj (ab) ' ET",
    );
    let known = |codes, carries_on| Some(ShowFacts { codes, carries_on });
    assert_eq!(
        show_facts(&doc, page, 6),
        Some(vec![
            // Three codes, and the `TJ` after the empty show starts at its pen.
            known(3, true),
            // Three codes in two strings, and a `Td` places what follows.
            known(3, false),
            // Two codes of two bytes.
            known(2, true),
            // A CMap whose codes this cannot count.
            None,
            // Three bytes do not divide into two-byte codes.
            None,
            // `F2` is still in force, so `ab` is one code of two bytes; and
            // nothing follows it.
            known(1, false),
        ])
    );
    // The count is the guard for the whole answer.
    assert_eq!(show_facts(&doc, page, 5), None);
}
