use lopdf::content::Content;
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

use super::{Rect, Verdict};

/// A one-page document drawing `stream`, with one named graphics state.
fn one_page(stream: &str) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let content = doc.add_object(Stream::new(dictionary! {}, stream.as_bytes().to_vec()));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "ExtGState" => dictionary! {
                "Thick" => dictionary! { "LW" => 4 },
                "Round" => dictionary! { "LC" => 1 },
                "Dashed" => dictionary! {
                    "D" => vec![Object::Array(vec![3.into()]), 0.into()],
                },
            },
        },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    (doc, page)
}

/// What cutting the last path `stream` paints against `regions` comes to, as
/// text: the operators left, or the verdict's name.
fn cut(stream: &str, regions: &[Rect]) -> String {
    let (doc, page) = one_page(stream);
    let content = Content::decode(stream.as_bytes()).expect("decodes");
    let paints = ["S", "s", "f", "F", "f*", "B", "B*", "b", "b*"];
    let last = content
        .operations
        .iter()
        .rposition(|operation| paints.contains(&operation.operator.as_str()))
        .expect("the stream paints a path");
    let builds = ["m", "l", "c", "v", "y", "re", "h", "W", "W*"];
    let mut first = last;
    while first > 0 && builds.contains(&content.operations[first - 1].operator.as_str()) {
        first -= 1;
    }
    let path: Vec<usize> = (first..=last).collect();
    let drawings = super::drawings(&doc, page, &content, &[path.as_slice()]);
    let Some(drawing) = &drawings[0] else {
        return "not cuttable".to_string();
    };
    match drawing.cut(regions) {
        Verdict::Outside => "outside".to_string(),
        Verdict::Gone => "gone".to_string(),
        Verdict::Unsupported => "unsupported".to_string(),
        Verdict::Cut(operations) => {
            let bytes = Content { operations }.encode().expect("encodes");
            String::from_utf8(bytes)
                .expect("text")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

#[test]
fn a_rule_is_cut_at_both_edges_of_the_region_that_crosses_it() {
    assert_eq!(
        cut(
            "0.5 w 72 200 m 539 200 l S",
            &[[100.0, 190.0, 150.0, 210.0]]
        ),
        "72 200 m 100 200 l 150 200 m 539 200 l S"
    );
    // Drawn from right to left, it is still drawn from right to left.
    assert_eq!(
        cut(
            "0.5 w 539 200 m 72 200 l S",
            &[[100.0, 190.0, 150.0, 210.0]]
        ),
        "539 200 m 150 200 l 100 200 m 72 200 l S"
    );
    // And a vertical one.
    assert_eq!(
        cut(
            "0.5 w 300 72 m 300 700 l S",
            &[[290.0, 100.0, 310.0, 150.0]]
        ),
        "300 72 m 300 100 l 300 150 m 300 700 l S"
    );
}

#[test]
fn a_region_over_one_end_of_a_rule_leaves_the_other() {
    assert_eq!(
        cut("1 w 72 200 m 539 200 l S", &[[50.0, 190.0, 150.0, 210.0]]),
        "150 200 m 539 200 l S"
    );
    assert_eq!(
        cut("1 w 72 200 m 539 200 l S", &[[400.0, 190.0, 600.0, 210.0]]),
        "72 200 m 400 200 l S"
    );
}

#[test]
fn two_regions_cut_one_rule_twice() {
    assert_eq!(
        cut(
            "1 w 72 200 m 539 200 l S",
            &[[100.0, 190.0, 150.0, 210.0], [300.0, 190.0, 320.0, 210.0]]
        ),
        "72 200 m 100 200 l 150 200 m 300 200 l 320 200 m 539 200 l S"
    );
}

#[test]
fn a_region_that_takes_only_part_of_a_rule_s_thickness_is_not_cut() {
    // A 4 pt rule inks 198 to 202. A region from 200 up covers half of it.
    assert_eq!(
        cut("4 w 72 200 m 539 200 l S", &[[100.0, 200.0, 150.0, 210.0]]),
        "unsupported"
    );
    // From 198 up it covers all of it, and one hundredth short of that it does not.
    assert_eq!(
        cut("4 w 72 200 m 539 200 l S", &[[100.0, 198.0, 150.0, 210.0]]),
        "72 200 m 100 200 l 150 200 m 539 200 l S"
    );
    assert_eq!(
        cut("4 w 72 200 m 539 200 l S", &[[100.0, 198.01, 150.0, 210.0]]),
        "unsupported"
    );
}

#[test]
fn a_cap_that_reaches_past_its_end_is_pulled_back_by_that_reach() {
    // Round and projecting caps add half the width, here 2, past a new end.
    for cap in ["1 J", "2 J", "/Round gs"] {
        assert_eq!(
            cut(
                &format!("4 w {cap} 72 200 m 539 200 l S"),
                &[[100.0, 190.0, 150.0, 210.0]]
            ),
            "72 200 m 98 200 l 152 200 m 539 200 l S",
            "{cap}"
        );
    }
    // A butt cap stops at its end, so the cut is at the edge.
    assert_eq!(
        cut(
            "4 w 0 J 72 200 m 539 200 l S",
            &[[100.0, 190.0, 150.0, 210.0]]
        ),
        "72 200 m 100 200 l 150 200 m 539 200 l S"
    );
}

#[test]
fn a_rule_that_only_touches_the_region_is_outside_it() {
    // It ends where the region begins. PDFium's bounds reach a whole width on.
    assert_eq!(
        cut("4 w 72 200 m 300 200 l S", &[[300.0, 190.0, 400.0, 210.0]]),
        "outside"
    );
    // With a round cap its ink does reach in, and it is shortened.
    assert_eq!(
        cut(
            "4 w 1 J 72 200 m 300 200 l S",
            &[[300.0, 190.0, 400.0, 210.0]]
        ),
        "72 200 m 298 200 l S"
    );
    // Beside the region, not in it.
    assert_eq!(
        cut("4 w 72 200 m 300 200 l S", &[[100.0, 202.0, 150.0, 210.0]]),
        "outside"
    );
}

#[test]
fn a_rule_whose_ink_is_all_inside_the_region_goes() {
    assert_eq!(
        cut("1 w 100 100 m 200 100 l S", &[[100.0, 99.5, 200.0, 100.5]]),
        "gone"
    );
}

#[test]
fn a_stroked_rectangle_keeps_its_corners_joined_around_a_cut() {
    // The bottom side is cut; the run starts after the cut, goes round all
    // four corners and ends before it.
    assert_eq!(
        cut("1 w 100 100 200 100 re S", &[[150.0, 90.0, 170.0, 110.0]]),
        "170 100 m 300 100 l 300 200 l 100 200 l 100 100 l 150 100 l S"
    );
    // Spelt with lines and closed by `s`, it is the same rectangle.
    assert_eq!(
        cut(
            "1 w 100 100 m 300 100 l 300 200 l 100 200 l s",
            &[[150.0, 90.0, 170.0, 110.0]]
        ),
        "170 100 m 300 100 l 300 200 l 100 200 l 100 100 l 150 100 l S"
    );
}

#[test]
fn a_region_over_a_corner_cuts_both_sides_that_meet_there() {
    assert_eq!(
        cut("1 w 100 100 200 100 re S", &[[90.0, 90.0, 110.0, 110.0]]),
        "110 100 m 300 100 l 300 200 l 100 200 l 100 110 l S"
    );
    // The region stops inside the left side's thickness, 99.5 to 100.5.
    assert_eq!(
        cut("1 w 100 100 200 100 re S", &[[100.2, 90.0, 120.0, 110.0]]),
        "unsupported"
    );
}

#[test]
fn a_line_that_goes_whole_does_not_join_the_two_either_side_of_it() {
    // Down, across, up. The region takes the line across and both corners.
    assert_eq!(
        cut(
            "1 w 100 200 m 100 100 l 200 100 l 200 200 l S",
            &[[90.0, 90.0, 210.0, 110.0]]
        ),
        "100 200 m 100 110 l 200 110 m 200 200 l S"
    );
}

#[test]
fn a_join_that_still_reaches_into_the_region_is_not_cut() {
    // The line folds back on itself at 100, so its join reaches to 102, and
    // the region begins past the fold. No line can be shortened to clear it.
    assert_eq!(
        cut(
            "4 w 0 100 m 100 100 l 50 100 l S",
            &[[100.5, 97.0, 103.0, 103.0]]
        ),
        "unsupported"
    );
    // The control: one unit further out the region is past the join's reach.
    assert_eq!(
        cut(
            "4 w 0 100 m 100 100 l 50 100 l S",
            &[[102.0, 97.0, 103.0, 103.0]]
        ),
        "outside"
    );
}

#[test]
fn a_filled_rectangle_loses_what_the_region_covers() {
    // Across a thin one: a piece either side.
    assert_eq!(
        cut("100 100 200 2 re f", &[[150.0, 90.0, 170.0, 110.0]]),
        "100 100 50 2 re 170 100 130 2 re f"
    );
    // Inside a large one: left, right, below, above.
    assert_eq!(
        cut("100 100 200 100 re f*", &[[150.0, 120.0, 170.0, 140.0]]),
        "100 100 50 100 re 170 100 130 100 re 150 100 20 20 re 150 140 20 60 re f*"
    );
    // Over a corner: two pieces.
    assert_eq!(
        cut("100 100 200 100 re f", &[[90.0, 90.0, 120.0, 130.0]]),
        "120 100 180 100 re 100 130 20 70 re f"
    );
    // Four straight sides are a rectangle too, and this one runs clockwise.
    assert_eq!(
        cut(
            "100 100 m 100 102 l 300 102 l 300 100 l h f",
            &[[150.0, 90.0, 170.0, 110.0]]
        ),
        "150 100 -50 2 re 300 100 -130 2 re f"
    );
    assert_eq!(
        cut("100 100 200 2 re f", &[[100.0, 100.0, 300.0, 102.0]]),
        "gone"
    );
    assert_eq!(
        cut("100 100 200 2 re f", &[[100.0, 102.0, 300.0, 110.0]]),
        "outside"
    );
}

#[test]
fn several_filled_rectangles_are_cut_only_when_their_fill_is_their_union() {
    assert_eq!(
        cut(
            "100 100 200 2 re 100 300 200 2 re f",
            &[[150.0, 90.0, 170.0, 110.0]]
        ),
        "100 100 50 2 re 170 100 130 2 re 100 300 200 2 re f"
    );
    // Under even-odd two rectangles that overlap leave a hole.
    assert_eq!(
        cut(
            "100 100 200 2 re 100 300 200 2 re f*",
            &[[150.0, 90.0, 170.0, 110.0]]
        ),
        "not cuttable"
    );
    // And under nonzero when they run opposite ways.
    assert_eq!(
        cut(
            "100 100 200 2 re 300 300 -200 2 re f",
            &[[150.0, 90.0, 170.0, 110.0]]
        ),
        "not cuttable"
    );
}

#[test]
fn the_region_is_carried_into_the_space_the_path_is_drawn_in() {
    // A quarter turn: (x, y) lands at (300 - y, x), so this rule runs up the
    // page at x = 250, and the region is given where the page shows it.
    assert_eq!(
        cut(
            "q 0 1 -1 0 300 0 cm 1 w 10 50 m 200 50 l S Q",
            &[[240.0, 100.0, 260.0, 120.0]]
        ),
        "10 50 m 100 50 l 120 50 m 200 50 l S"
    );
    // Doubled: the rule is 2 pt thick on the page, 19 to 21.
    assert_eq!(
        cut(
            "2 0 0 2 0 0 cm 1 w 10 10 m 100 10 l S",
            &[[50.0, 19.0, 60.0, 21.0]]
        ),
        "10 10 m 25 10 l 30 10 m 100 10 l S"
    );
    assert_eq!(
        cut(
            "2 0 0 2 0 0 cm 1 w 10 10 m 100 10 l S",
            &[[50.0, 19.5, 60.0, 21.0]]
        ),
        "unsupported"
    );
    // Two matrices multiply, the later one applied first.
    assert_eq!(
        cut(
            "1 0 0 1 100 0 cm 2 0 0 2 0 0 cm 1 w 10 10 m 100 10 l S",
            &[[150.0, 19.0, 160.0, 21.0]]
        ),
        "10 10 m 25 10 l 30 10 m 100 10 l S"
    );
    // A skew keeps no axis.
    assert_eq!(
        cut(
            "1 0.2 0 1 0 0 cm 1 w 10 10 m 100 10 l S",
            &[[50.0, 0.0, 60.0, 40.0]]
        ),
        "not cuttable"
    );
}

#[test]
fn the_line_width_is_the_one_in_effect_where_the_path_is_painted() {
    let region = [[100.0, 198.0, 150.0, 202.0]];
    let cut_there = "72 200 m 100 200 l 150 200 m 539 200 l S";
    // 4 pt fits the region exactly and 6 pt does not.
    assert_eq!(cut("4 w 72 200 m 539 200 l S", &region), cut_there);
    assert_eq!(cut("6 w 72 200 m 539 200 l S", &region), "unsupported");
    // `Q` puts back the width `q` saved.
    assert_eq!(cut("4 w q 6 w Q 72 200 m 539 200 l S", &region), cut_there);
    assert_eq!(
        cut("4 w q 6 w 72 200 m 539 200 l S Q", &region),
        "unsupported"
    );
    // A named graphics state sets it as `w` does.
    assert_eq!(cut("/Thick gs 72 200 m 539 200 l S", &region), cut_there);
    assert_eq!(
        cut("6 w /Thick gs 72 200 m 539 200 l S", &region),
        cut_there
    );
    // One that cannot be found leaves the width unknown, until `w` sets it.
    assert_eq!(
        cut("4 w /Missing gs 72 200 m 539 200 l S", &region),
        "not cuttable"
    );
    assert_eq!(
        cut("/Missing gs 4 w 0 J [] 0 d 72 200 m 539 200 l S", &region),
        cut_there
    );
}

#[test]
fn what_cannot_be_split_exactly_is_not_cut() {
    let region = [[100.0, 190.0, 150.0, 210.0]];
    for stream in [
        // A curve.
        "1 w 72 200 m 100 250 200 250 539 200 c S",
        // A line off the axes.
        "1 w 72 200 m 539 205 l S",
        // Dashed, by the operator and by a named state.
        "1 w [3] 0 d 72 200 m 539 200 l S",
        "1 w /Dashed gs 72 200 m 539 200 l S",
        // A hairline: as thin as the device draws, which is not a width.
        "0 w 72 200 m 539 200 l S",
        // Filled and stroked together.
        "1 w 100 100 200 2 re B",
        // Also the clip.
        "1 w 72 200 m 539 200 l W S",
        // A filled shape that is not a rectangle.
        "72 200 m 539 200 l 300 300 l f",
        // An operand that is not a number.
        "1 w 72 (200) m 539 200 l S",
        // A cap that is none of the three.
        "1 w 7 J 72 200 m 539 200 l S",
    ] {
        assert_eq!(cut(stream, &region), "not cuttable", "{stream}");
    }
    // The control: the same rule with nothing in the way is cut.
    assert_eq!(
        cut("1 w [] 0 d 0 J 72 200 m 539 200 l S", &region),
        "72 200 m 100 200 l 150 200 m 539 200 l S"
    );
}

#[test]
fn a_path_whose_operators_are_not_next_to_each_other_is_not_cut() {
    let stream = "1 w 72 200 m 2 0 0 2 0 0 cm 539 200 l S";
    let (doc, page) = one_page(stream);
    let content = Content::decode(stream.as_bytes()).expect("decodes");
    // `w m cm l S`: the path is operations 1, 3 and 4.
    let drawings = super::drawings(&doc, page, &content, &[&[1, 3, 4]]);
    assert!(drawings[0].is_none());
    let stream = "1 w 72 200 m 539 200 l S";
    let (doc, page) = one_page(stream);
    let content = Content::decode(stream.as_bytes()).expect("decodes");
    let drawings = super::drawings(&doc, page, &content, &[&[1, 2, 3]]);
    assert!(drawings[0].is_some(), "the control");
}
