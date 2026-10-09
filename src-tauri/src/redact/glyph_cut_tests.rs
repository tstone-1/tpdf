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

/// A show of `codes` codes with nothing after it that starts at its pen.
fn fact(codes: usize) -> ShowFacts {
    ShowFacts {
        codes: Some(codes),
        carries_on: false,
    }
}

/// The same with the next show starting at its pen.
fn carried(codes: usize) -> ShowFacts {
    ShowFacts {
        codes: Some(codes),
        carries_on: true,
    }
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
        &[],
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
        &[],
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
        &[],
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
        &[],
        std::slice::from_ref(&cuttable),
        Some(&[fact(6)]),
        region,
    );
    assert_eq!(plan.show_cuts.len(), 1);

    let mut unboxed = glyphs("abcdef");
    unboxed.glyphs[5].bounds = None;
    type Case = (&'static str, Option<TextGlyphs>, Option<Vec<ShowFacts>>);
    let cases: [Case; 5] = [
        ("PDFium could not place it", None, Some(vec![fact(6)])),
        ("the content could not be read", cuttable.clone(), None),
        (
            "its font's codes cannot be counted",
            cuttable.clone(),
            Some(vec![ShowFacts {
                codes: None,
                carries_on: false,
            }]),
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
        let said = cut_within(&mut plan, &[], &[text], facts.as_deref(), region);
        assert!(said.is_empty(), "{why}");
        assert_eq!(plan.shows, vec![0], "{why}");
        assert!(plan.show_cuts.is_empty(), "{why}");
        assert!(plan.unhandled.is_empty(), "{why}");
    }
}

/// The last glyph goes and the next show starts at this one's pen: the gap
/// needs the place PDFium put that next show. Without it the show cannot be
/// cut, and it cannot go whole either, because that would move the next
/// show: it is left, and the region says so.
#[test]
fn a_cut_to_the_end_needs_the_next_start_only_when_the_pen_carries_on() {
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[],
        &[Some(glyphs("abcd"))],
        Some(&[carried(4)]),
        over(2, 4),
    );
    assert!(
        plan.shows.is_empty() && plan.show_cuts.is_empty(),
        "no tail, and the pen carries on"
    );
    assert_eq!(plan.unhandled.len(), 1, "it is reported");

    let mut with_tail = glyphs("abcd");
    with_tail.tail = Some(2000.0);
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[],
        &[Some(with_tail)],
        Some(&[carried(4)]),
        over(2, 4),
    );
    assert_eq!(plan.show_cuts.len(), 1, "a tail");
    assert_eq!(plan.show_cuts[0].take, vec![2, 3]);

    let mut plan = whole();
    cut_within(
        &mut plan,
        &[],
        &[Some(glyphs("abcd"))],
        Some(&[fact(4)]),
        over(2, 4),
    );
    assert_eq!(plan.show_cuts.len(), 1, "the pen does not carry on");

    // And a cut that stops short of the end never needs it.
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[],
        &[Some(glyphs("abcd"))],
        Some(&[carried(4)]),
        over(1, 3),
    );
    assert_eq!(plan.show_cuts.len(), 1, "the last glyph stays");
    assert!(plan.unhandled.is_empty());
}

/// A page object of `kind`, which is all [`cut_within`] reads of one.
fn object(kind: &str) -> PageObject {
    PageObject {
        bounds: [0.0, 0.0, 1.0, 1.0],
        kind: kind.to_string(),
    }
}

/// **A show that goes whole takes the pen it moved with it**, so one with a
/// show after it at its pen is not deleted: it becomes a cut of every glyph,
/// which leaves the distance to where PDFium placed that next show.
#[test]
fn a_whole_show_with_a_show_at_its_pen_becomes_a_cut_of_every_glyph() {
    let mut text = glyphs("abc");
    text.tail = Some(1500.0);
    let all = ShowCut {
        ordinal: 0,
        pens: vec![0.0, 500.0, 1000.0],
        tail: Some(1500.0),
        take: vec![0, 1, 2],
    };
    assert!(all.takes_all());

    let mut plan = whole();
    let said = cut_within(
        &mut plan,
        &[],
        &[Some(text.clone())],
        Some(&[carried(3)]),
        over(0, 3),
    );
    // Nothing is said of it here: what a whole show takes is its text, which
    // the caller has.
    assert!(said.is_empty());
    assert!(plan.shows.is_empty() && plan.unhandled.is_empty());
    assert_eq!(plan.show_cuts, vec![all.clone()]);

    // The control: with nothing at its pen it is deleted, as it was.
    let mut plan = whole();
    cut_within(
        &mut plan,
        &[],
        &[Some(text.clone())],
        Some(&[fact(3)]),
        over(0, 3),
    );
    assert_eq!(plan.shows, vec![0]);
    assert!(plan.show_cuts.is_empty() && plan.unhandled.is_empty());

    // A show that cannot be cut in part goes whole the same way, whatever
    // the region covers of it: its codes cannot be counted, or do not match.
    for codes in [None, Some(7)] {
        let mut plan = whole();
        let said = cut_within(
            &mut plan,
            &[],
            &[Some(text.clone())],
            Some(&[ShowFacts {
                codes,
                carries_on: true,
            }]),
            over(0, 1),
        );
        assert!(said.is_empty(), "{codes:?}");
        assert_eq!(plan.show_cuts, vec![all.clone()], "{codes:?}");
        assert!(plan.shows.is_empty(), "{codes:?}");
    }
}

/// The distance cannot be measured: PDFium placed no glyphs for the show
/// along one line, which is what vertical writing and right-to-left text
/// are, or placed the next show behind it. It is left, and reported by its
/// place among the page's objects.
#[test]
fn a_whole_show_that_cannot_be_measured_is_left_and_reported() {
    let objects = [
        object("path"),
        object("text"),
        object("image"),
        object("text"),
    ];
    let reported = |plan: &Plan| -> Vec<(usize, String)> {
        plan.unhandled
            .iter()
            .map(|left| (left.at, left.kind.clone()))
            .collect()
    };
    let second = || Plan {
        shows: vec![1],
        ..Plan::default()
    };

    // No glyphs at all.
    let mut plan = second();
    cut_within(
        &mut plan,
        &objects,
        &[None, None],
        Some(&[fact(3), carried(3)]),
        over(0, 3),
    );
    assert!(plan.shows.is_empty() && plan.show_cuts.is_empty());
    assert_eq!(reported(&plan), vec![(3, UNMEASURED_TEXT.to_string())]);
    assert!(!plan.is_complete());

    // Glyphs, and no place for the next show.
    let mut plan = second();
    cut_within(
        &mut plan,
        &objects,
        &[None, Some(glyphs("abc"))],
        Some(&[fact(3), carried(3)]),
        over(0, 3),
    );
    assert_eq!(reported(&plan), vec![(3, UNMEASURED_TEXT.to_string())]);

    // The control: the same show with nothing at its pen is deleted.
    let mut plan = second();
    cut_within(
        &mut plan,
        &objects,
        &[None, None],
        Some(&[fact(3), fact(3)]),
        over(0, 3),
    );
    assert_eq!(plan.shows, vec![1]);
    assert!(plan.is_complete());

    // And what a reader is told.
    let said = Unhandled {
        at: 3,
        kind: UNMEASURED_TEXT.to_string(),
        drawn: None,
    }
    .sentence();
    assert!(
        said.contains("object 3 is text whose width could not be measured")
            && said.contains("would move that text"),
        "{said}"
    );
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

/// With a tail the show may have another at its pen, and deleting it would
/// lose the distance: it stays a cut, of all its glyphs.
#[test]
fn cuts_that_add_up_to_a_whole_show_with_a_tail_stay_a_cut() {
    let with_tail = |take: &[usize]| ShowCut {
        tail: Some(2000.0),
        ..cut(3, 4, take)
    };
    let mut shows = vec![9];
    let merged = merge_cuts(vec![with_tail(&[0, 1]), with_tail(&[2, 3])], &mut shows);
    assert_eq!(merged, vec![with_tail(&[0, 1, 2, 3])]);
    assert_eq!(shows, vec![9]);
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

/// A `TJ` that draws nothing stands between the cut show and the next one,
/// and moves the pen. The tail is where PDFium put the next show, which is
/// after that move, so the gap up to it holds the move and the `TJ` goes:
/// left in, it would move the next show a second time.
#[test]
fn spacing_alone_after_a_cut_to_the_end_goes_into_the_gap() {
    // `abcd` ends at 2000 here and the spacing is 500 more: the next show is
    // at 2500, and `c` is at 1000.
    let stream =
        "BT /F1 12 Tf (abcd) Tj 0 Tc [-500] TJ [()] TJ () Tj (next) Tj [-70] TJ (last) Tj ET";
    let (mut doc, page) = one_page(stream);
    let mut one = cut(0, 4, &[2, 3]);
    one.tail = Some(2500.0);
    cut_shows(&mut doc, page, &[one], 3).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        // What is not a `TJ` stays, and so does the spacing after the next
        // show, which is not between the two.
        "BT /F1 12 Tf [(ab) -1500] TJ 0 Tc () Tj (next) Tj [-70] TJ (last) Tj ET"
    );

    // A cut that stops short of the end leaves the pen where the last glyph
    // left it, and the spacing with it.
    let (mut doc, page) = one_page(stream);
    let mut short = cut(0, 4, &[1, 2]);
    short.tail = Some(2500.0);
    cut_shows(&mut doc, page, &[short], 3).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf [(a) -1000 (d)] TJ 0 Tc [-500] TJ [()] TJ () Tj (next) Tj [-70] TJ (last) Tj ET"
    );

    // And so does a cut to the end of a show whose next one is placed: no
    // gap is written, so nothing holds the move.
    let (mut doc, page) = one_page("BT /F1 12 Tf (abcd) Tj [-500] TJ 0 -14 Td (next) Tj ET");
    cut_shows(&mut doc, page, &[cut(0, 4, &[2, 3])], 2).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 12 Tf [(ab)] TJ [-500] TJ 0 -14 Td (next) Tj ET"
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

/// A cut of every glyph of `SECRET`, whose pens are 500 apart from 0, with
/// the next show placed at `tail`.
fn all_of_secret(ordinal: usize, tail: Option<f32>) -> ShowCut {
    ShowCut {
        tail,
        ..cut(ordinal, 6, &[0, 1, 2, 3, 4, 5])
    }
}

/// What `stream` is after the show at `ordinal` of its `shows` goes whole as
/// a cut of every glyph, the next show having been placed at 3400.
fn without_secret(stream: &str, ordinal: usize, shows: usize) -> String {
    let (mut doc, page) = one_page(stream);
    cut_shows(
        &mut doc,
        page,
        &[all_of_secret(ordinal, Some(3400.0))],
        shows,
    )
    .expect("cut");
    content_of(&doc, page)
}

/// **The show after one that goes whole stays where it was.** Until
/// 2026-10-09 the operator was deleted and `NEXT` was drawn where `SECRET`
/// had started. The gap is from the first pen to where PDFium placed `NEXT`.
#[test]
fn a_whole_show_leaves_the_distance_to_a_show_that_starts_at_its_pen() {
    assert_eq!(
        without_secret("BT /F1 20 Tf 100 700 Td (SECRET) Tj (NEXT) Tj ET", 0, 2),
        "BT /F1 20 Tf 100 700 Td [-3400] TJ (NEXT) Tj ET"
    );
    // What sets spacing or the font between the two moves no pen.
    assert_eq!(
        without_secret("BT /F1 20 Tf (SECRET) Tj 2 Tc /F1 9 Tf (NEXT) Tj ET", 0, 2),
        "BT /F1 20 Tf [-3400] TJ 2 Tc /F1 9 Tf (NEXT) Tj ET"
    );
    // The numbers a `TJ` holds before its first string are before the first
    // pen, and stay; those after it are inside the distance.
    assert_eq!(
        without_secret(
            "BT /F1 20 Tf [-120 (SEC) 30 (RET) -40] TJ (NEXT) Tj ET",
            0,
            2
        ),
        "BT /F1 20 Tf [-120 -3400] TJ (NEXT) Tj ET"
    );
}

/// A `TJ` that draws nothing between the two is in the distance already,
/// which is measured to where the next show was placed: left in, it would
/// move that show a second time.
#[test]
fn spacing_alone_after_a_whole_show_goes_into_the_distance() {
    assert_eq!(
        without_secret(
            "BT /F1 20 Tf (SECRET) Tj [-500] TJ (NEXT) Tj [-70] TJ ET",
            0,
            2
        ),
        "BT /F1 20 Tf [-3400] TJ (NEXT) Tj [-70] TJ ET"
    );
}

/// Nothing starts at the pen of the last show of a text object, and `Td`,
/// `Tm`, `T*`, `'` and `"` place what follows from the start of the line: no
/// distance is written, and spacing that follows stays where it was.
#[test]
fn a_whole_show_with_nothing_at_its_pen_leaves_nothing() {
    for (stream, shows, want) in [
        (
            "BT /F1 20 Tf (lead) Tj (SECRET) Tj ET",
            2,
            "BT /F1 20 Tf (lead) Tj ET",
        ),
        (
            "BT /F1 20 Tf (lead) Tj (SECRET) Tj ET BT (NEXT) Tj ET",
            3,
            "BT /F1 20 Tf (lead) Tj ET BT (NEXT) Tj ET",
        ),
        (
            "BT /F1 20 Tf (lead) Tj (SECRET) Tj [-500] TJ 0 -14 Td (NEXT) Tj ET",
            3,
            "BT /F1 20 Tf (lead) Tj [-500] TJ 0 -14 Td (NEXT) Tj ET",
        ),
        (
            "BT /F1 20 Tf (lead) Tj (SECRET) Tj 1 0 0 1 50 60 Tm (NEXT) Tj ET",
            3,
            "BT /F1 20 Tf (lead) Tj 1 0 0 1 50 60 Tm (NEXT) Tj ET",
        ),
        (
            "BT /F1 20 Tf 14 TL (lead) Tj (SECRET) Tj T* (NEXT) Tj ET",
            3,
            "BT /F1 20 Tf 14 TL (lead) Tj T* (NEXT) Tj ET",
        ),
        (
            "BT /F1 20 Tf 14 TL (lead) Tj (SECRET) Tj (NEXT) ' ET",
            3,
            "BT /F1 20 Tf 14 TL (lead) Tj (NEXT) ' ET",
        ),
    ] {
        assert_eq!(without_secret(stream, 1, shows), want, "{stream}");
    }
}

/// `'` and `"` do more than show, and a whole one still does it: the line
/// moves, and `"` sets the two spacings the next show is drawn with.
#[test]
fn a_whole_quote_operator_keeps_what_it_did_before_showing() {
    assert_eq!(
        without_secret("BT /F1 20 Tf 14 TL (SECRET) ' (NEXT) Tj ET", 0, 2),
        "BT /F1 20 Tf 14 TL T* [-3400] TJ (NEXT) Tj ET"
    );
    assert_eq!(
        without_secret("BT /F1 20 Tf 14 TL 2 1 (SECRET) \" (NEXT) Tj ET", 0, 2),
        "BT /F1 20 Tf 14 TL 2 Tw 1 Tc T* [-3400] TJ (NEXT) Tj ET"
    );
    // With another `'` after it, nothing starts at its pen and the line
    // still moves.
    assert_eq!(
        without_secret("BT /F1 20 Tf 14 TL (SECRET) ' (NEXT) ' ET", 0, 2),
        "BT /F1 20 Tf 14 TL T* (NEXT) ' ET"
    );
}

/// No code is counted for a show that goes whole, so its font's codes need
/// not be countable and the pens need not be as many as its codes.
#[test]
fn a_whole_show_counts_no_codes() {
    assert_eq!(
        without_secret("BT /F3 20 Tf (SECRET) Tj (NEXT) Tj ET", 0, 2),
        "BT /F3 20 Tf [-3400] TJ (NEXT) Tj ET"
    );
    let (mut doc, page) = one_page("BT /F1 20 Tf (SECRETS) Tj (NEXT) Tj ET");
    cut_shows(&mut doc, page, &[all_of_secret(0, Some(3900.0))], 2).expect("cut");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 20 Tf [-3900] TJ (NEXT) Tj ET"
    );
}

/// A show at its pen and no place for it: nothing says how far the pen
/// moved, so nothing is written.
#[test]
fn a_whole_show_not_measured_to_the_show_at_its_pen_is_refused() {
    let stream = "BT /F1 20 Tf (SECRET) Tj (NEXT) Tj ET";
    let (mut doc, page) = one_page(stream);
    let before = content_of(&doc, page);
    let why = cut_shows(&mut doc, page, &[all_of_secret(0, None)], 2).expect_err("no tail");
    assert!(why.contains("not measured to the show after it"), "{why}");
    assert_eq!(content_of(&doc, page), before);
    // The control: with nothing at its pen the same cut needs no tail.
    let (mut doc, page) = one_page("BT /F1 20 Tf (SECRET) Tj 0 -14 Td (NEXT) Tj ET");
    cut_shows(&mut doc, page, &[all_of_secret(0, None)], 2).expect("cut");
    assert_eq!(content_of(&doc, page), "BT /F1 20 Tf 0 -14 Td (NEXT) Tj ET");
}

/// A cut of every glyph leaves no show, so the shows deleted after it are
/// counted from what is left.
#[test]
fn the_shows_after_a_cut_of_every_glyph_are_counted_from_what_is_left() {
    let cuts = [
        all_of_secret(1, Some(3400.0)),
        cut(3, 6, &[2]),
        all_of_secret(4, Some(3400.0)),
    ];
    assert_eq!(after_cuts(&cuts, &[0, 2, 5, 6], 7), (vec![0, 1, 3, 4], 5));
    // The control: cuts of a part change nothing.
    assert_eq!(after_cuts(&cuts[1..2], &[0, 2, 5], 7), (vec![0, 2, 5], 7));

    // And on a page: the second show goes as a cut, the fourth is deleted.
    let stream = "BT /F1 20 Tf (keep) Tj (SECRET) Tj (NEXT) Tj 0 -14 Td (gone) Tj (last) Tj ET";
    let (mut doc, page) = one_page(stream);
    let cuts = [all_of_secret(1, Some(3400.0))];
    cut_shows(&mut doc, page, &cuts, 5).expect("cut");
    let (shows, text_objects) = after_cuts(&cuts, &[3], 5);
    super::super::remove_shows(&mut doc, page, &shows, text_objects).expect("removed");
    assert_eq!(
        content_of(&doc, page),
        "BT /F1 20 Tf (keep) Tj [-3400] TJ (NEXT) Tj 0 -14 Td (last) Tj ET"
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
    let known = |codes, carries_on| ShowFacts {
        codes: Some(codes),
        carries_on,
    };
    let unknown = |carries_on| ShowFacts {
        codes: None,
        carries_on,
    };
    assert_eq!(
        show_facts(&doc, page, 6),
        Some(vec![
            // Three codes, and the `TJ` after the empty show starts at its pen.
            known(3, true),
            // Three codes in two strings, and a `Td` places what follows.
            known(3, false),
            // Two codes of two bytes.
            known(2, true),
            // A CMap whose codes this cannot count. Whether the next show
            // starts at its pen is known all the same.
            unknown(true),
            // Three bytes do not divide into two-byte codes, and a `'` places
            // what follows.
            unknown(false),
            // `F2` is still in force, so `ab` is one code of two bytes; and
            // nothing follows it.
            known(1, false),
        ])
    );
    // The count is the guard for the whole answer.
    assert_eq!(show_facts(&doc, page, 5), None);
}
