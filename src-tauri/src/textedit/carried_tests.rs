//! Read-only text a wrap carries with its line or its block.
//!
//! The editor keeps some text byte for byte: what the tags pin (a paragraph
//! with alternate text, a justified one, a hyphen under a Span of its own, a
//! link's words), a run with a glyph it cannot write, and what an ActualText
//! span describes. None of that is rewritten, and all of it can be drawn
//! somewhere else from the bytes it has. So when a wrap moves a line or a
//! block down, such text goes with it; until 2026-10-06 it stayed, and the
//! wrap was refused for landing on it or for leaving it behind.
//!
//! The page and the paragraph are `wrap_tests`': 300 x 240, three lines at a
//! 14 pt pitch from x 20, the next paragraph `gap` below. The synthetic font
//! has no glyph the editor cannot write, so that shape is tested on the
//! LibreOffice export (`tagging/libreoffice_tests.rs`), whose bullets are one.
use super::layout_tests::{reader_line_starts, shows};
use super::wrap_tests::{content, refusal, tagged, wrapped, FIRST, LAST, LONGER, NEXT, WIDEST};
use super::*;

/// `doc` with the paragraph that owns `mcid` given alternate text, which
/// keeps its content read-only (`tagging`, *What pins a paragraph*).
fn pinned(mut doc: Document, mcid: usize) -> Document {
    let reference = |doc: &Document, id: ObjectId, key: &[u8]| {
        doc.get_dictionary(id)
            .unwrap()
            .get(key)
            .unwrap()
            .as_reference()
            .unwrap()
    };
    let catalog = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let root = reference(&doc, catalog, b"StructTreeRoot");
    let parents = reference(&doc, root, b"ParentTree");
    let element = doc
        .get_dictionary(parents)
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap()[1]
        .as_array()
        .unwrap()[mcid]
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(element)
        .unwrap()
        .set("Alt", Object::string_literal("DESCRIBED"));
    doc
}

/// Where every read-only show with text is drawn.
fn kept(doc: &Document) -> Vec<(String, f64, f64)> {
    inspect(doc, 0)
        .unwrap()
        .preserved
        .iter()
        .filter(|run| !run.text.trim().is_empty())
        .map(|run| (run.text.clone(), run.matrix[4], run.matrix[5]))
        .collect()
}

fn offered(doc: &Document) -> Vec<(String, f64, f64)> {
    scan(doc, 0)
        .unwrap()
        .runs
        .iter()
        .filter(|run| !run.text.trim().is_empty())
        .map(|run| (run.text.clone(), run.matrix[4], run.matrix[5]))
        .collect()
}

fn at(runs: &[(String, f64, f64)], text: &str) -> (f64, f64) {
    let found: Vec<_> = runs.iter().filter(|(run, ..)| run == text).collect();
    assert_eq!(found.len(), 1, "{text} in {runs:?}");
    (found[0].1, found[0].2)
}

fn near((x, y): (f64, f64), (ex, ey): (f64, f64)) -> bool {
    (x - ex).abs() < 0.0001 && (y - ey).abs() < 0.0001
}

// A paragraph the tags keep read-only, one pitch below the edited one: the
// wrap's moved line lands where it is, so it goes down by the same line, its
// bytes the ones it had. A block after it, placed from the line matrix the
// moved show leaves behind, starts at the bits it always did: the restoration
// a moved show ends with is the read-only show's own.
#[test]
fn a_read_only_block_below_moves_down_with_the_wrap() {
    let doc = pinned(
        tagged(
            &content(14., "0 -60 Td /P <</MCID 4>> BDC (BY) Tj EMC "),
            &[&[0, 1, 2], &[3], &[4]],
        ),
        3,
    );
    // The control: it is read-only, and where the wrap would put a line.
    assert_eq!(kept(&doc), [(NEXT.to_string(), 20., 158.)]);
    let saved = wrapped(&doc, WIDEST, LONGER);
    assert!(near(at(&offered(&saved), LAST), (20., 158.)));
    let moved = kept(&saved);
    assert!(near(at(&moved, NEXT), (20., 144.)), "{moved:?}");
    // Still read-only, still drawn from its own bytes, once.
    assert!(offered(&saved).iter().all(|(text, ..)| text != NEXT));
    let literal = Object::string_literal(NEXT);
    assert_eq!(
        shows(&saved)
            .iter()
            .filter(|show| **show == literal)
            .count(),
        1
    );
    // The block below it has a blank line to spare and stays, to the bit.
    let start = |doc: &Document| {
        reader_line_starts(doc)
            .into_iter()
            .find(|(show, _)| *show == Object::string_literal("BY"))
            .unwrap()
            .1
    };
    assert!(start(&doc).is_some());
    assert_eq!(start(&saved), start(&doc));
}

// A show inside an inline ActualText span moves inside its span: the position
// is written between the span's own BDC and EMC, the span still says what it
// said, and the text is still offered and still edited with its ActualText.
#[test]
fn a_show_in_an_actualtext_span_moves_with_its_block_inside_its_span() {
    let doc = tagged(
        &content(14., "").replace(
            &format!("/P <</MCID 3>> BDC ({NEXT}) Tj EMC"),
            &format!("/P <</MCID 3>> BDC /Span <</ActualText ({NEXT})>> BDC ({NEXT}) Tj EMC EMC"),
        ),
        &[&[0, 1, 2], &[3]],
    );
    assert!(near(at(&offered(&doc), NEXT), (20., 158.)));
    let saved = wrapped(&doc, WIDEST, LONGER);
    assert!(near(at(&offered(&saved), NEXT), (20., 144.)));
    let page = crate::pagetree::ordered_pages(&saved)[0];
    let operations = Content::decode_strict(&saved.get_page_content(page))
        .unwrap()
        .operations;
    let open = operations
        .iter()
        .position(|op| {
            op.operator == "BDC" && op.operands[1].as_dict().is_ok_and(|d| d.has(b"ActualText"))
        })
        .unwrap();
    let close = open
        + operations[open..]
            .iter()
            .position(|op| op.operator == "EMC")
            .unwrap();
    let inside = &operations[open + 1..close];
    assert_eq!(
        operations[open].operands[1]
            .as_dict()
            .unwrap()
            .get(b"ActualText")
            .unwrap(),
        &Object::string_literal(NEXT)
    );
    assert!(inside.iter().any(|op| op.operator == "Tm"));
    assert_eq!(
        inside
            .iter()
            .filter(|op| op.operator == "Tj" && op.operands[0] == Object::string_literal(NEXT))
            .count(),
        1
    );
    // And it is edited afterwards as any such span is.
    let mut again = saved.clone();
    let page = scan(&again, 0).unwrap();
    let run = page.runs.iter().find(|run| run.text == NEXT).unwrap();
    write(
        &mut again,
        &[Change {
            page: 0,
            revision: page.revision.clone(),
            operator: run.operator,
            original: NEXT.into(),
            replacement: "SECOND BRANDS".into(),
            layout: None,
        }],
    )
    .unwrap();
    assert!(near(at(&offered(&again), "SECOND BRANDS"), (20., 144.)));
}

/// A paragraph whose last line is read-only text of its own, a span whose
/// ActualText is not what it shows, with the first line at `top`; the next
/// paragraph is set above it, out of the way.
fn read_only_last_line(top: f64) -> Document {
    tagged(
        &format!(
            "BT /F1 12 Tf 20 {top} Td /P <</MCID 0>> BDC ({FIRST}) Tj EMC \
             0 -14 Td /P <</MCID 1>> BDC ({WIDEST}) Tj EMC \
             0 -14 Td /P <</MCID 2>> BDC /Span <</ActualText (TENTH)>> BDC ({LAST}) Tj EMC EMC \
             0 150 Td /P <</MCID 3>> BDC ({NEXT}) Tj EMC ET"
        ),
        &[&[0, 1, 2], &[3]],
    )
}

// A read-only line of the edited paragraph moves down with the lines the edit
// adds, and is held to the page like any line that moves: one that would
// leave it refuses the wrap. Its rectangle is the read-only run's, which the
// page keeps apart from the runs it offers.
#[test]
fn a_read_only_line_of_the_paragraph_moves_and_is_held_to_the_page() {
    let doc = read_only_last_line(60.);
    assert_eq!(kept(&doc), [(LAST.to_string(), 20., 32.)]);
    let saved = wrapped(&doc, WIDEST, LONGER);
    assert!(near(at(&kept(&saved), LAST), (20., 18.)));
    // Three points up the page: the line's box reaches the bottom edge where
    // it is, and a line lower it would be off the page.
    let low = read_only_last_line(31.);
    assert_eq!(kept(&low), [(LAST.to_string(), 20., 3.)]);
    let error = refusal(&low, WIDEST, LONGER);
    assert!(error.contains("what is below it"), "{error}");
}

// Text that stays, set in the middle of a read-only line that would move, is
// left behind by the move: the wrap is refused for it by name, before any
// text is laid out, as it is under a line the editor may rewrite.
#[test]
fn text_that_stays_in_the_middle_of_a_read_only_line_refuses_the_wrap() {
    let line = |between: &str| {
        tagged(
            &format!(
                "BT /F1 12 Tf 20 200 Td /P <</MCID 0>> BDC ({FIRST}) Tj EMC \
                 0 -14 Td /P <</MCID 1>> BDC ({WIDEST}) Tj EMC \
                 0 -14 Td /P <</MCID 2>> BDC /Span <</ActualText (TENTH)>> BDC ({LAST}) Tj EMC \
                 130 0 Td /Span <</ActualText (ENDED)>> BDC (END) Tj EMC EMC \
                 -60 0 Td {between} \
                 -70 -52 Td /P <</MCID 3>> BDC ({NEXT}) Tj EMC ET"
            ),
            &[&[0, 1, 2], &[3]],
        )
    };
    // The control: with nothing between its two pieces the line moves.
    let saved = wrapped(&line(""), WIDEST, LONGER);
    let moved = kept(&saved);
    assert!(near(at(&moved, LAST), (20., 158.)), "{moved:?}");
    assert!(near(at(&moved, "END"), (150., 158.)), "{moved:?}");
    // A word no element owns is read-only and no block's, so nothing moves it.
    let error = refusal(&line("(BY) Tj"), WIDEST, LONGER);
    assert!(
        error.contains("part of it below cannot be moved"),
        "{error}"
    );
    // That refusal is for a line the page has filled. Five letters that take
    // the line past its paragraph's measure and not to the page's edge wrap
    // where they can, and stay on their line where they cannot, as they did
    // before a line wrapped at its measure at all.
    let few = "FIFTY NINE ONCE AND DONE THEN";
    let texts = |doc: &Document| -> Vec<String> {
        scan(doc, 0)
            .unwrap()
            .runs
            .into_iter()
            .map(|run| run.text)
            .collect()
    };
    let broken = texts(&wrapped(&line(""), WIDEST, few));
    assert!(broken.contains(&"THEN".to_string()), "{broken:?}");
    let stays = texts(&wrapped(&line("(BY) Tj"), WIDEST, few));
    assert!(stays.contains(&few.to_string()), "{stays:?}");
    // And where no wrap can be planned at all: a tab's spacer in a line of
    // the paragraph below, which has one position and nothing can move.
    let spacer = tagged(
        &content(52., "").replace(
            &format!("({LAST}) Tj"),
            &format!("({LAST}) Tj /Span <</ActualText (\t)>> BDC ( ) Tj EMC (BY) Tj"),
        ),
        &[&[0, 1, 2], &[3]],
    );
    let error = refusal(&spacer, WIDEST, LONGER);
    assert!(
        error.contains("part of it below cannot be moved"),
        "{error}"
    );
    let stays = texts(&wrapped(&spacer, WIDEST, few));
    assert!(stays.contains(&few.to_string()), "{stays:?}");
}

// LibreOffice Writer draws a page under a clip of the page's own size less a
// rounding. A line that clip ends has reached the page edge: it is refused in
// those words, and it wraps as a line at the page edge does. A clip that ends
// the line further in is a box its producer cut the text to, and keeps the
// refusal it had.
#[test]
fn a_line_that_a_clip_the_size_of_the_page_ends_wraps_at_the_page_edge() {
    let under = |right: f64| {
        tagged(
            &format!("q 0 0.03 {right} 239.94 re W* n {} Q", content(52., "")),
            &[&[0, 1, 2], &[3]],
        )
    };
    let saved = wrapped(&under(299.97), WIDEST, LONGER);
    let runs = offered(&saved);
    assert!(near(at(&runs, "THEN FIRST AND SECOND"), (20., 172.)));
    assert!(near(at(&runs, LAST), (20., 158.)));
    // A line that cannot wrap -- the next line is three ems and more below,
    // which is no pitch -- keeps its refusal, and that names the page, which
    // is what the reader sees at the end of the line.
    let apart = tagged(
        &format!(
            "q 0 0.03 299.97 239.94 re W* n {} Q",
            content(52., "").replacen("0 -14 Td /P <</MCID 2>>", "0 -40 Td /P <</MCID 2>>", 1)
        ),
        &[&[0, 1, 2], &[3]],
    );
    let error = refusal(&apart, WIDEST, LONGER);
    assert!(error.contains("it reaches the edge of the page"), "{error}");
    // Six tenths of a point in is past the allowance: a clip, as before.
    let error = refusal(&under(299.4), WIDEST, LONGER);
    assert!(
        error.contains("the document clips the space after it"),
        "{error}"
    );
}

// The rule itself, as geometry: the nearest of page, clip and table bounds
// ends the line, and a clip within half a point of the page's edge is named
// as the page. Asked along each direction a line can run, of the edge it runs
// into and of no other; the bounds a table states are never the page.
#[test]
fn a_clip_within_half_a_point_of_the_page_edge_is_the_page() {
    // (the box at width 0 and 1, the page, a clip `inset` short of the edge
    // the line runs into, and otherwise far inside the page)
    let cases = |inset: f64| {
        [
            (
                ([100., 48., 100., 63.], [100., 48., 101., 63.]),
                [300., 240.],
                [50., 40., 300. - inset, 70.],
            ),
            (
                ([200., 48., 200., 63.], [199., 48., 200., 63.]),
                [300., 240.],
                [inset, 40., 250., 70.],
            ),
            (
                ([48., 100., 63., 100.], [48., 100., 63., 101.]),
                [240., 300.],
                [40., 50., 70., 300. - inset],
            ),
            (
                ([48., 200., 63., 200.], [48., 199., 63., 200.]),
                [240., 300.],
                [40., inset, 70., 250.],
            ),
        ]
    };
    let own = [0., 0., 0., 0.];
    for (inset, stop) in [
        (0.03, layout::Room::Page),
        (0.4, layout::Room::Page),
        (0.6, layout::Room::Clip),
        (20., layout::Room::Clip),
    ] {
        for (direction, (edges, page, clip)) in cases(inset).into_iter().enumerate() {
            let (free, named) =
                layout::room(edges, 1., page, (Some(clip), None), own, [].into_iter(), 0.);
            assert_eq!(named, stop, "direction {direction}, {inset} pt in");
            // Named as the page or not, the clip is where the line ends.
            assert!(
                (free - (200. - inset)).abs() < 0.0001,
                "direction {direction}, {inset} pt in: {free}"
            );
            // The same rectangle as the bounds a table states is a table's.
            let (_, named) =
                layout::room(edges, 1., page, (None, Some(clip)), own, [].into_iter(), 0.);
            assert_eq!(named, layout::Room::Table, "direction {direction}");
        }
    }
}

// A read-only block offered to the wrap moves when a line moving beside it
// would close on it, as any block does, and here that move has nowhere to go:
// text no element owns is one pitch under its far end, past the paragraph. Left where it is, the block is
// in nobody's way, which is how this wrap was accepted while read-only text
// could not move at all. So a wrap refused with every movable block offered
// is tried once more with only the blocks the writer may rewrite.
#[test]
fn a_wrap_a_read_only_block_cannot_follow_is_made_without_it() {
    let doc = pinned(
        tagged(
            &content(
                52.,
                "130 50 Td /P <</MCID 4>> BDC (BY BY BY BY) Tj EMC 65 -14 Td (YY) Tj ",
            ),
            &[&[0, 1, 2], &[3], &[4]],
        ),
        4,
    );
    assert!(near(at(&kept(&doc), "BY BY BY BY"), (150., 170.)));
    assert!(near(at(&kept(&doc), "YY"), (215., 156.)));
    let saved = wrapped(&doc, WIDEST, "FIFTY NINE ONCE AND DONE THEN FIRST AND");
    assert!(near(at(&offered(&saved), "THEN FIRST AND"), (20., 172.)));
    assert!(near(at(&offered(&saved), LAST), (20., 158.)));
    assert!(near(at(&kept(&saved), "BY BY BY BY"), (150., 170.)));
    assert!(near(at(&kept(&saved), "YY"), (215., 156.)));
}
