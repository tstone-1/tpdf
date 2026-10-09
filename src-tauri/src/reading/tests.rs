//! `reading.rs` against what `reading.ts` says, and the cases both are asked.
//!
//! [`cases`] is the question list: every route and every rule `reading.ts`
//! names, each on a page small enough to read. `cli::tests` writes the answers
//! this file computes to `testdata/cli/reading.json`, and
//! `src/lib/clireading.test.ts` asks the original the same questions --- that
//! is the check that the two agree. The tests below pin the answers that matter
//! on their own, so a rule that drifts on *both* sides at once is still red.

use super::{read, Range, Route};
use crate::structure::TaggedRun;
use crate::text::PageText;

/// One question for both implementations.
pub(crate) struct Case {
    pub name: &'static str,
    pub text: PageText,
}

type Glyph = (u32, [f32; 4]);

/// Ten-point characters laid left to right from `x`, on the band at `top`.
///
/// Widths of 5.5 and heights of 11.3, so every coordinate is a value an `f32`
/// cannot hold exactly --- which is the route the numbers take to the viewer,
/// and the one `webview_number` exists for.
fn word(text: &str, x: f32, top: f32) -> Vec<Glyph> {
    text.chars()
        .enumerate()
        .map(|(at, c)| {
            let left = x + at as f32 * 5.5;
            (c as u32, [left, top, left + 5.5, top + 11.3])
        })
        .collect()
}

/// Ten-point characters laid right to left, the first ending at `right`.
fn written_leftwards(text: &str, right: f32, top: f32) -> Vec<Glyph> {
    text.chars()
        .enumerate()
        .map(|(at, c)| {
            let edge = right - at as f32 * 5.5;
            (c as u32, [edge - 5.5, top, edge, top + 11.3])
        })
        .collect()
}

/// PDFium's synthesised line break: two characters it placed nowhere.
fn crlf() -> Vec<Glyph> {
    vec![(13, [0.0; 4]), (10, [0.0; 4])]
}

fn page(parts: Vec<Vec<Glyph>>) -> PageText {
    let mut text = PageText {
        width_pt: 595.2756,
        height_pt: 841.8898,
        ..PageText::default()
    };
    for (code, quad) in parts.into_iter().flatten() {
        text.codes.push(code);
        text.boxes.extend_from_slice(&quad);
    }
    text
}

fn run(tag: &str, start: u32, end: u32) -> TaggedRun {
    TaggedRun {
        tag: tag.into(),
        path: vec![tag.into()],
        start,
        end,
    }
}

/// Two columns of three lines, `alpha` at x 72 and `beta` at x 320.
fn column(name: &str, x: f32, line: usize) -> Vec<Glyph> {
    let mut out = word(&format!("{name} {line}"), x, 100.0 + line as f32 * 14.1);
    out.extend(crlf());
    out
}

/// The same page turned `turns` quarter-turns, as PDFium would report it.
fn turned(upright: &PageText, turns: u8) -> PageText {
    let (w, h) = (upright.width_pt, upright.height_pt);
    let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = upright.clone();
    out.quarter_turns = turns;
    out.width_pt = dw;
    out.height_pt = dh;
    for quad in out.boxes.chunks_mut(4) {
        if quad.iter().all(|v| *v == 0.0) {
            continue;
        }
        // Device space upright back to the page's own, then out again turned.
        let own = [
            f64::from(quad[0]),
            f64::from(h - quad[3]),
            f64::from(quad[2]),
            f64::from(h - quad[1]),
        ];
        quad.copy_from_slice(&crate::text::to_device(turns, dw, dh, own));
    }
    out
}

/// Every case, in a fixed order. The names are the sample's keys.
#[allow(clippy::too_many_lines)]
pub(crate) fn cases() -> Vec<Case> {
    let mut out = Vec::new();

    // Columns written column by column, and interleaved line by line: the
    // second is `alpha one beta one` in index order.
    let natural = page(
        (0..3)
            .map(|l| column("alpha", 72.0, l))
            .chain((0..3).map(|l| column("beta", 320.0, l)))
            .collect(),
    );
    let interleaved = page(
        (0..3)
            .flat_map(|l| [column("alpha", 72.0, l), column("beta", 320.0, l)])
            .collect(),
    );
    out.push(Case {
        name: "columns-natural",
        text: natural,
    });
    out.push(Case {
        name: "columns-interleaved",
        text: interleaved.clone(),
    });

    // A heading across both columns, with more air under it than between the
    // body's lines, emitted last.
    let mut heading = (0..3)
        .flat_map(|l| [column("alpha", 72.0, l + 3), column("beta", 320.0, l + 3)])
        .collect::<Vec<_>>();
    heading.push(word(
        "A heading across the whole width of the page",
        72.0,
        60.0,
    ));
    out.push(Case {
        name: "heading",
        text: page(heading),
    });

    // A margin note left of the body and tagged last, emitted between the two
    // body lines --- so index order, geometric order and tagged order are three
    // different orders.
    let body_one = word("body one", 200.0, 100.0);
    let note = word("note", 40.0, 90.0);
    let body_two = word("body two", 200.0, 114.1);
    let (a, b, c) = (
        body_one.len() as u32,
        note.len() as u32,
        body_two.len() as u32,
    );
    let mut tagged = page(vec![body_one.clone(), note.clone(), body_two.clone()]);
    tagged.runs = vec![
        run("P", 0, a),
        run("P", a + b, a + b + c),
        run("Note", a, a + b),
    ];
    out.push(Case {
        name: "tagged",
        text: tagged.clone(),
    });
    let mut untagged = tagged.clone();
    untagged.runs.clear();
    out.push(Case {
        name: "tagged-stripped",
        text: untagged,
    });
    // One visible character left unclaimed: the tags do not cover the page, so
    // the geometry decides.
    let mut incomplete = tagged.clone();
    incomplete.runs = vec![
        run("P", 0, a),
        run("P", a + b, a + b + c),
        run("Note", a, a + b - 1),
    ];
    out.push(Case {
        name: "tagged-incomplete",
        text: incomplete,
    });
    // Unclaimed separators between the elements do not disqualify the tags,
    // and are kept, attached to the run before them.
    let mut separated = page(vec![body_one, crlf(), note, crlf(), body_two]);
    separated.runs = vec![
        run("P", 0, a),
        run("P", a + b + 4, a + b + c + 4),
        run("Note", a + 2, a + b + 2),
    ];
    out.push(Case {
        name: "tagged-separators",
        text: separated,
    });

    // Rotation: the same interleaved page at each turn.
    for turns in 1..=3u8 {
        out.push(Case {
            name: ["rotated-1", "rotated-2", "rotated-3"][usize::from(turns - 1)],
            text: turned(&interleaved, turns),
        });
    }

    // A vertical label beside upright body text: characters turned a quarter.
    let mut mixed = page(vec![
        word("upright body", 120.0, 100.0),
        word("upright more", 120.0, 114.1),
    ]);
    let label: Vec<Glyph> = "LABEL"
        .chars()
        .enumerate()
        .map(|(at, ch)| {
            let top = 100.0 + at as f32 * 5.5;
            (ch as u32, [60.0, top, 71.3, top + 5.5])
        })
        .collect();
    let before = mixed.codes.len();
    for (code, quad) in label {
        mixed.codes.push(code);
        mixed.boxes.extend_from_slice(&quad);
    }
    mixed.char_turns = vec![0; before];
    mixed
        .char_turns
        .extend(std::iter::repeat_n(1u8, mixed.codes.len() - before));
    out.push(Case {
        name: "mixed-directions",
        text: mixed,
    });

    // A combining acute above the x-height, touching no band: it joins the `e`.
    let mut accent = word("resume", 72.0, 100.0);
    accent.push((0x301, [94.0, 96.0, 97.5, 98.6]));
    accent.extend(word(" next", 99.5, 100.0));
    out.push(Case {
        name: "combining-mark",
        text: page(vec![accent, crlf(), word("second line", 72.0, 114.1)]),
    });

    // A space the font parked 0.02pt tall just below its line.
    let mut floated = word("cafe", 72.0, 100.0);
    floated.push((32, [94.0, 111.4, 96.5, 111.42]));
    floated.extend(word("latte", 96.5, 100.0));
    out.push(Case {
        name: "sliver-space",
        text: page(vec![floated, crlf(), word("below", 72.0, 114.1)]),
    });

    // A comma dipping below the baseline, and spaces PDFium reports 0.01 tall.
    // It overlaps the letters by 2.0 of its 4.6 --- under half, as
    // `tagged.pdf`'s measured 46% is --- so only the short-mark rule keeps it.
    let mut comma = word("one", 72.0, 100.0);
    comma.push((44, [88.5, 109.3, 90.0, 113.9]));
    comma.push((32, [90.0, 111.29, 92.5, 111.3]));
    comma.extend(word("two", 92.5, 100.0));
    out.push(Case {
        name: "comma",
        text: page(vec![comma, crlf(), word("three", 72.0, 114.1)]),
    });

    // Two characters whose gap is the cut width exactly as the viewer holds the
    // numbers, and wider than it as a widening cast holds them: in decimal
    // 104.5 - 101.2 = 3.3 against a cut of 3.3000000000000256, so one line;
    // widened, 3.3000031 against 3.2999954, so two. Found by search, and it is
    // what makes `webview_number` a rule a test can see.
    out.push(Case {
        name: "f32-tie",
        text: page(vec![vec![
            (u32::from('A'), [100.1, 100.0, 101.2, 111.3]),
            (u32::from('B'), [104.5, 100.0, 105.6, 111.3]),
        ]]),
    });

    // One line in two halves, a wide gap between them, and the right half
    // standing 0.8pt higher: ascenders beside x-height letters. The halves are
    // found top edge first, and read left to right all the same. Tagged, so
    // the gap is not a column cut and both halves reach one block.
    let label = word("Date:", 72.0, 100.0);
    let value = word("12 March", 200.0, 99.2);
    let mut split = page(vec![label.clone(), value.clone()]);
    split.runs = vec![run("P", 0, (label.len() + value.len()) as u32)];
    out.push(Case {
        name: "split-line-tagged",
        text: split,
    });
    // The same line by the geometry: a full line under it with no free band
    // between the two, so neither a column cut nor a row cut parts the halves.
    out.push(Case {
        name: "split-line-tight",
        text: page(vec![
            label,
            value,
            word("a full line under both halves", 72.0, 110.9),
        ]),
    });
    // A right-to-left line the same way: PDFium hands its characters back in
    // the order they are read, so their indices rise as their positions fall.
    // The half read first is on the right, and here it is the lower one, so
    // neither the top edges nor ascending position puts it first.
    let first = written_leftwards("\u{5d0}\u{5d1}\u{5d2}\u{5d3}", 300.0, 100.0);
    let second = written_leftwards("\u{5d4}\u{5d5}\u{5d6}", 150.0, 99.2);
    let mut leftwards = page(vec![first.clone(), second.clone()]);
    leftwards.runs = vec![run("P", 0, (first.len() + second.len()) as u32)];
    out.push(Case {
        name: "split-line-right-to-left",
        text: leftwards,
    });

    // One word written right to left between two written left to right: three
    // steps forwards in each Latin half against two backwards, so the line is
    // a left-to-right one and the Hebrew word keeps its place in it.
    let mut mixed_line = word("Date", 72.0, 100.0);
    mixed_line.extend(written_leftwards("\u{5d0}\u{5d1}\u{5d2}", 216.5, 99.2));
    mixed_line.extend(word("then", 300.0, 100.0));
    let mut mixed_line = page(vec![mixed_line]);
    mixed_line.runs = vec![run("P", 0, 11)];
    out.push(Case {
        name: "split-line-mixed",
        text: mixed_line,
    });

    // A combining mark drawn a point left of where its base starts, in a line
    // of two one-letter halves: the only step between neighbouring characters
    // is from the base back to its mark, and it must not make the line a
    // right-to-left one.
    let mut marked = page(vec![vec![
        (u32::from('e'), [72.0, 100.0, 77.5, 111.3]),
        (0x301, [71.0, 96.0, 74.5, 98.6]),
        (u32::from('x'), [200.0, 99.2, 205.5, 110.5]),
    ]]);
    marked.runs = vec![run("P", 0, 3)];
    out.push(Case {
        name: "split-line-mark",
        text: marked,
    });

    // Small type whose band reaches into large type lower down and further
    // left: one line to the gathering, which lets a short box join what it
    // touches, and not two halves of one row. The overlap is 7.3 of the small
    // type's 11.3 and of the large type's 30, so it is the taller one that
    // says no. It stays in the order of the top edges --- as a row above a
    // paragraph gathered into one tall fragment must.
    let mut unequal = word("above", 200.0, 100.0);
    unequal.extend("BIG".chars().enumerate().map(|(at, c)| {
        let left = 72.0 + at as f32 * 15.0;
        (c as u32, [left, 104.0, left + 15.0, 134.0])
    }));
    let mut unequal = page(vec![unequal]);
    unequal.runs = vec![run("P", 0, 8)];
    out.push(Case {
        name: "split-line-unequal",
        text: unequal,
    });

    // Degenerate pages.
    out.push(Case {
        name: "empty",
        text: page(Vec::new()),
    });
    out.push(Case {
        name: "all-unplaced",
        text: page(vec!["no boxes"
            .chars()
            .map(|c| (c as u32, [0.0; 4]))
            .collect()]),
    });
    out.push(Case {
        name: "leading-unplaced",
        text: page(vec![vec![(32, [0.0; 4])], word("after", 72.0, 100.0)]),
    });

    out
}

fn text_of(text: &PageText, lines: &[Vec<Range>]) -> Vec<String> {
    lines
        .iter()
        .map(|ranges| {
            ranges
                .iter()
                .flat_map(|r| r.from..r.to)
                .filter_map(|i| char::from_u32(text.codes[i]))
                .filter(|c| *c != '\r' && *c != '\n')
                .collect()
        })
        .collect()
}

fn case(name: &str) -> PageText {
    cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no case {name}"))
        .text
}

#[test]
fn interleaved_columns_read_one_column_after_the_other() {
    let text = case("columns-interleaved");
    let reading = read(&text);
    assert_eq!(reading.route, Route::Geometric);
    assert_eq!(
        text_of(&text, &reading.lines),
        ["alpha 0", "alpha 1", "alpha 2", "beta 0", "beta 1", "beta 2"]
    );
}

#[test]
fn a_spanning_heading_is_read_before_both_columns() {
    let text = case("heading");
    let lines = text_of(&text, &read(&text).lines);
    assert_eq!(lines[0], "A heading across the whole width of the page");
    assert_eq!(lines[1..4], ["alpha 3", "alpha 4", "alpha 5"]);
}

#[test]
fn tags_that_cover_the_page_decide_its_order_and_the_geometry_does_not() {
    let text = case("tagged");
    let reading = read(&text);
    assert_eq!(reading.route, Route::Tagged);
    assert_eq!(
        text_of(&text, &reading.lines),
        ["body one", "body two", "note"]
    );
    // The control: without the tags, the same page reads the note first.
    let stripped = case("tagged-stripped");
    let geometric = read(&stripped);
    assert_eq!(geometric.route, Route::Geometric);
    assert_eq!(
        text_of(&stripped, &geometric.lines),
        ["note", "body one", "body two"]
    );
}

#[test]
fn tags_that_leave_a_visible_character_unclaimed_are_not_used() {
    assert_eq!(read(&case("tagged-incomplete")).route, Route::Geometric);
    // Unclaimed separators are not visible, so they disqualify nothing.
    let text = case("tagged-separators");
    let reading = read(&text);
    assert_eq!(reading.route, Route::Tagged);
    assert_eq!(
        text_of(&text, &reading.lines),
        ["body one", "body two", "note"]
    );
}

/// Every case, `leading-unplaced` included: until 2026-09-27 both sides
/// dropped a first character PDFium placed nowhere, and this test carried that
/// as an exception so the two would agree. It is now fixed on both sides.
#[test]
fn every_order_is_a_permutation_of_the_page() {
    for case in cases() {
        let mut order = read(&case.text).order();
        order.sort_unstable();
        let all: Vec<usize> = (0..case.text.codes.len()).collect();
        assert_eq!(order, all, "{}", case.name);
    }
}

#[test]
fn a_turned_page_reads_as_the_upright_one_does() {
    let upright = read(&case("columns-interleaved")).order();
    for name in ["rotated-1", "rotated-2", "rotated-3"] {
        assert_eq!(read(&case(name)).order(), upright, "{name}");
    }
}

#[test]
fn a_mark_a_sliver_and_a_comma_stay_on_their_line() {
    for (name, first) in [
        ("combining-mark", "resume\u{301} next"),
        ("sliver-space", "cafe latte"),
        ("comma", "one, two"),
    ] {
        let text = case(name);
        let lines = text_of(&text, &read(&text).lines);
        assert_eq!(lines.len(), 2, "{name}: {lines:?}");
        assert_eq!(lines[0], first, "{name}");
    }
}

/// Which fragments share a line is found from their top edges; the order they
/// are read in is along the line. Until 2026-10-09 one sort answered both, and
/// the first of these read `12 MarchDate:`.
#[test]
fn the_halves_of_a_split_line_are_read_along_it_whichever_stands_higher() {
    let text = case("split-line-tagged");
    let reading = read(&text);
    assert_eq!(reading.route, Route::Tagged);
    assert_eq!(text_of(&text, &reading.lines), ["Date:12 March"]);

    let text = case("split-line-tight");
    let reading = read(&text);
    assert_eq!(reading.route, Route::Geometric);
    assert_eq!(
        text_of(&text, &reading.lines),
        ["Date:12 March", "a full line under both halves"]
    );
}

/// The direction is the line's own: a right-to-left line is read from its
/// right end, where ascending position would read its second half first.
#[test]
fn a_right_to_left_line_in_two_halves_is_read_from_its_right_end() {
    let text = case("split-line-right-to-left");
    assert_eq!(
        text_of(&text, &read(&text).lines),
        ["\u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4}\u{5d5}\u{5d6}"]
    );
}

/// Only halves of one row are put in order along it. Type of another size
/// that merely touches the line keeps the order of the top edges: measured
/// 2026-10-09, ordering those too moved a row behind the paragraph under it.
#[test]
fn type_of_another_size_touching_a_line_is_not_a_half_of_it() {
    let text = case("split-line-unequal");
    assert_eq!(text_of(&text, &read(&text).lines), ["aboveBIG"]);
}

/// A majority of the steps decides, so one word written the other way does
/// not turn the sentence round.
#[test]
fn one_word_written_the_other_way_does_not_turn_its_line_round() {
    let text = case("split-line-mixed");
    assert_eq!(
        text_of(&text, &read(&text).lines),
        ["Date\u{5d0}\u{5d1}\u{5d2}then"]
    );
}

/// A mark is drawn over its base, wherever the producer put its box: the step
/// back to it says nothing about which way the line is written.
#[test]
fn a_combining_mark_does_not_turn_its_line_round() {
    let text = case("split-line-mark");
    assert_eq!(text_of(&text, &read(&text).lines), ["e\u{301}x"]);
}

#[test]
fn text_turned_a_quarter_is_read_in_its_own_direction() {
    let text = case("mixed-directions");
    let lines = text_of(&text, &read(&text).lines);
    assert!(lines.contains(&"LABEL".to_string()), "{lines:?}");
    assert!(lines.contains(&"upright body".to_string()), "{lines:?}");
}

#[test]
fn a_gap_is_measured_in_the_numbers_the_viewer_holds() {
    let text = case("f32-tie");
    assert_eq!(text_of(&text, &read(&text).lines), ["AB"]);
}

#[test]
fn a_number_is_the_one_the_viewer_holds() {
    assert_eq!(super::webview_number(595.2756), 595.2756);
    assert_ne!(f64::from(595.2756f32), 595.2756);
    assert_eq!(super::webview_number(f32::NAN), 0.0);
}
