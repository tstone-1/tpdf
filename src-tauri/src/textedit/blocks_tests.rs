//! Blocks read off the geometry of an untagged page (`blocks.rs`).
//!
//! The fixtures set text in the base fixture's Helvetica, which has every
//! character a list label needs; the synthetic font `wrap_tests` uses has no
//! digits and no full stop. At 12 pt an `A` is 8.004 pt wide.
use super::*;

fn synthetic(body: &str) -> Document {
    let mut doc = tests::fixture();
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let stream = doc.add_object(Stream::new(Dictionary::new(), body.as_bytes().to_vec()));
    doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
    doc
}

/// The page's runs grouped by block, top down, each block as its runs' text.
fn grouped(doc: &Document) -> Vec<Vec<String>> {
    let page = inspect(doc, 0).unwrap();
    let mut order: Vec<ObjectId> = Vec::new();
    let mut groups: BTreeMap<ObjectId, Vec<String>> = BTreeMap::new();
    for run in &page.runs.runs {
        let Some(block) = page.blocks.get(&run.operator) else {
            continue;
        };
        if !order.contains(block) {
            order.push(*block);
        }
        groups.entry(*block).or_default().push(run.text.clone());
    }
    order
        .into_iter()
        .map(|block| groups.remove(&block).unwrap())
        .collect()
}

fn page(lines: &str) -> Document {
    synthetic(&format!("BT /F1 12 Tf {lines} ET"))
}

/// A page with a second name for Helvetica, so a line can be set in a font
/// resource of its own.
fn two_fonts(body: &str) -> Document {
    let mut doc = synthetic(body);
    let font = doc.add_object(lopdf::dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding"
    });
    let id = crate::pagetree::ordered_pages(&doc)[0];
    doc.get_dictionary_mut(id).unwrap().set(
        "Resources",
        lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => font, "F2" => font } },
    );
    doc
}

fn strings(blocks: &[&[&str]]) -> Vec<Vec<String>> {
    blocks
        .iter()
        .map(|block| block.iter().map(|text| (*text).to_owned()).collect())
        .collect()
}

#[test]
fn lines_at_one_pitch_one_font_and_one_left_edge_are_one_block() {
    let doc = page("20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj 0 -14 Td (THREE) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO", "THREE"]]));
}

#[test]
fn a_line_in_another_font_or_size_or_too_far_below_starts_a_block() {
    let doc = two_fonts(
        "BT /F1 12 Tf 20 200 Td (HEAD) Tj /F2 12 Tf 0 -14 Td (BODY) Tj 0 -14 Td (MORE) Tj ET",
    );
    assert_eq!(grouped(&doc), strings(&[&["HEAD"], &["BODY", "MORE"]]));
    // What counts is the font most of a line is set in: one word in another
    // leaves the line in its paragraph, and a line mostly in it does not.
    let doc = two_fonts(
        "BT /F1 12 Tf 20 200 Td (ONE LINE) Tj 0 -14 Td (SECOND ) Tj /F2 12 Tf (WORD) Tj ET",
    );
    assert_eq!(grouped(&doc), strings(&[&["ONE LINE", "SECOND ", "WORD"]]));
    let doc = two_fonts(
        "BT /F1 12 Tf 20 200 Td (ONE LINE) Tj 0 -14 Td (AN ) Tj /F2 12 Tf (ITALIC LINE) Tj ET",
    );
    assert_eq!(
        grouped(&doc),
        strings(&[&["ONE LINE"], &["AN ", "ITALIC LINE"]])
    );
    // Half and half is no font at all.
    let doc =
        two_fonts("BT /F1 12 Tf 20 200 Td (ONE LINE) Tj 0 -14 Td (AB ) Tj /F2 12 Tf (CD) Tj ET");
    assert_eq!(grouped(&doc), strings(&[&["ONE LINE"], &["AB ", "CD"]]));
    let doc = page("20 200 Td (ONE) Tj /F1 10 Tf 0 -14 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE"], &["TWO"]]));
    // Three ems at 12 pt is 36 pt.
    let doc = page("20 200 Td (ONE) Tj 0 -37 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE"], &["TWO"]]));
    let doc = page("20 200 Td (ONE) Tj 0 -36 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO"]]));
}

#[test]
fn a_run_a_fraction_of_a_point_off_the_baseline_is_on_its_line() {
    let doc = page("20 200 Td (ONE) Tj 30 0.4 Td (TWO) Tj -30 -14.4 Td (THREE) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO", "THREE"]]));
    let doc = page("20 200 Td (ONE) Tj 30 0.6 Td (TWO) Tj -30 -14.6 Td (THREE) Tj");
    assert_ne!(grouped(&doc), strings(&[&["ONE", "TWO", "THREE"]]));
}

#[test]
fn a_block_ends_where_its_pitch_steps() {
    let doc = page("20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj 0 -14.6 Td (THREE) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO"], &["THREE"]]));
    let doc = page("20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj 0 -14.4 Td (THREE) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO", "THREE"]]));
}

#[test]
fn something_drawn_between_two_lines_ends_the_block_and_a_background_does_not() {
    let body = |drawing: &str| {
        synthetic(&format!(
            "{drawing} BT /F1 12 Tf 20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj ET"
        ))
    };
    // Between the bottom of one line's box and the top of the next: an
    // underline, inside the upper line's own box, is not between them.
    assert_eq!(
        grouped(&body("20 195.5 60 1 re f")),
        strings(&[&["ONE"], &["TWO"]])
    );
    assert_eq!(
        grouped(&body("20 198.5 60 1 re f")),
        strings(&[&["ONE", "TWO"]])
    );
    assert_eq!(
        grouped(&body("10 150 200 80 re f")),
        strings(&[&["ONE", "TWO"]])
    );
    // Beside the lines rather than between them.
    assert_eq!(
        grouped(&body("150 193 60 1 re f")),
        strings(&[&["ONE", "TWO"]])
    );
}

#[test]
fn left_edges_agree_or_the_first_line_is_indented_by_at_most_four_ems() {
    let doc = page("20 200 Td (ONE) Tj 1.5 -14 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE"], &["TWO"]]));
    let doc = page("20 200 Td (ONE) Tj 0.9 -14 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO"]]));
    // Four ems at 12 pt is 48 pt.
    let doc = page("60 200 Td (ONE ONE) Tj -40 -14 Td (TWO TWO TWO) Tj 0 -14 Td (THREE) Tj");
    assert_eq!(
        grouped(&doc),
        strings(&[&["ONE ONE", "TWO TWO TWO", "THREE"]])
    );
    let doc = page("70 200 Td (ONE ONE) Tj -50 -14 Td (TWO TWO TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE ONE"], &["TWO TWO TWO"]]));
    // Only a first line: an indented line after the first starts a block.
    let doc = page("20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj 0 -14 Td (THREE) Tj 20 -14 Td (FOUR) Tj -20 -14 Td (FIVE) Tj");
    assert_eq!(
        grouped(&doc),
        strings(&[&["ONE", "TWO", "THREE"], &["FOUR", "FIVE"]])
    );
    // Nor is a line below a joined one that starts further left.
    let doc = page("20 200 Td (ONE) Tj 0 -14 Td (TWO) Tj -10 -14 Td (THREE) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE", "TWO"], &["THREE"]]));
    // And a line hanging out to the left of the rest is not a first line.
    let doc = page("20 200 Td (ONE) Tj 20 -14 Td (TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE"], &["TWO"]]));
}

#[test]
fn a_list_label_starts_an_item_whose_first_line_hangs() {
    let doc = page(
        "20 200 Td (A. ONE) Tj 21.6 -14 Td (TWO) Tj -21.6 -14 Td (B. THREE) Tj 21.6 -14 Td (FOUR) Tj",
    );
    assert_eq!(
        grouped(&doc),
        strings(&[&["A. ONE", "TWO"], &["B. THREE", "FOUR"]])
    );
    // A label set as a show of its own.
    let doc = page(
        "20 200 Td (A.) Tj 21.6 0 Td (ONE) Tj 0 -14 Td (TWO) Tj -21.6 -14 Td (B.) Tj 21.6 0 Td (THREE) Tj",
    );
    assert_eq!(
        grouped(&doc),
        strings(&[&["A.", "ONE", "TWO"], &["B.", "THREE"]])
    );
    // Items set flush, one line each, are items and not one paragraph.
    let doc = page("20 200 Td (A. ONE) Tj 0 -14 Td (B. TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["A. ONE"], &["B. TWO"]]));
}

#[test]
fn a_line_aligned_by_runs_of_spaces_is_a_table_row_and_not_prose() {
    let doc = page("20 200 Td (ONE   TWO) Tj 0 -14 Td (ONE   TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE   TWO"], &["ONE   TWO"]]));
    let doc = page("20 200 Td (ONE  TWO) Tj 0 -14 Td (ONE  TWO) Tj");
    assert_eq!(grouped(&doc), strings(&[&["ONE  TWO", "ONE  TWO"]]));
    // The same columns set with a displacement inside one show; one that opens
    // the show only says where it starts.
    let rows = |gap: i64| {
        page(&format!(
            "20 200 Td [(ONE) {gap} (TWO)] TJ 0 -14 Td [(ONE) {gap} (TWO)] TJ"
        ))
    };
    assert_eq!(grouped(&rows(-1000)).len(), 2);
    assert_eq!(grouped(&rows(-999)).len(), 1);
    let doc = page("20 200 Td [-2000 (ONE TWO)] TJ 0 -14 Td [-2000 (ONE TWO)] TJ");
    assert_eq!(grouped(&doc).len(), 1);
}

#[test]
fn a_gutter_along_a_line_separates_two_blocks_side_by_side() {
    // One em at 12 pt is 12 pt: A ends at 28.004.
    let doc = page("20 200 Td (A) Tj 20.1 0 Td (B) Tj -20.1 -14 Td (A) Tj 20.1 0 Td (B) Tj");
    assert_eq!(grouped(&doc), strings(&[&["A", "A"], &["B", "B"]]));
    let doc = page("20 200 Td (A) Tj 19.9 0 Td (B) Tj -19.9 -14 Td (A) Tj 19.9 0 Td (B) Tj");
    assert_eq!(grouped(&doc), strings(&[&["A", "B", "A", "B"]]));
}

#[test]
fn the_label_rule_reads_numbers_letters_numerals_and_bullets() {
    for text in [
        "1. A",
        "12) A",
        "(a) A",
        "iv. A",
        "\u{2022} A",
        "- A",
        "\u{f0b7} A",
    ] {
        assert!(blocks::label([text].into_iter()), "{text:?}");
    }
    for text in ["1.", "1234. A", "AB. A", "A A", "WORD. A", "1.5 A", ""] {
        assert!(!blocks::label([text].into_iter()), "{text:?}");
    }
    assert!(blocks::label(["7.", "A"].into_iter()));
}

#[test]
fn geometry_answers_only_where_the_tags_do_not() {
    let doc = super::wrap_tests::paragraph(52.);
    let page = inspect(&doc, 0).unwrap();
    assert!(!page.blocks.is_empty());
    assert!(page.blocks.values().all(|block| block.0 != 0));
    let id = crate::pagetree::ordered_pages(&doc)[0];
    let sheet = crate::pagetree::displayed_page(&doc, id);
    let geometric = blocks::geometric(&page, &sheet);
    assert_eq!(
        geometric.keys().collect::<Vec<_>>(),
        page.blocks.keys().collect::<Vec<_>>()
    );
    assert!(geometric.values().all(|block| block.0 == 0));
}
