"""Rust mutation rows: text added on every page, comparing two documents, and
text set at an angle, all from 26.10.23.

Added 2026-10-18. The commit that brought these three in had unit tests for
each and no mutation of any, so none of its tests had been seen to fail.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- What `check` refuses --------------------------------------------------------
    Mutation(
        "added text: a control character written",
        "src/addedtext.rs",
        "    if text.chars().any(char::is_control) {",
        "    if false && text.chars().any(char::is_control) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a script with no font written",
        "src/addedtext.rs",
        "    if !crate::textbox::encodable(text) && !crate::textedit::AddedFont::covers(text) {",
        "    if false && !crate::textbox::encodable(text) && !crate::textedit::AddedFont::covers(text) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a field other than page and pages written",
        "src/addedtext.rs",
        '        if field != "{page}" && field != "{pages}" {',
        '        if false && field != "{page}" && field != "{pages}" {',
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a size out of range written",
        "src/addedtext.rs",
        "    if !SIZES.contains(&added.size) {",
        "    if false && !SIZES.contains(&added.size) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: an opacity of 0 written",
        "src/addedtext.rs",
        "    if !(added.opacity > 0.0 && added.opacity <= 1.0) {",
        "    if !(added.opacity >= 0.0 && added.opacity <= 1.0) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a colour past 1 written",
        "src/addedtext.rs",
        "    if !added.color.iter().all(|c| (0.0..=1.0).contains(c)) {",
        "    if !added.color.iter().all(|c| (0.0..=2.0).contains(c)) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: an angle past a whole turn written",
        "src/addedtext.rs",
        "    if !(added.angle.is_finite() && added.angle.abs() <= 360.0) {",
        "    if !(added.angle.is_finite() && added.angle.abs() <= 720.0) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a margin past the bound written",
        "src/addedtext.rs",
        "    if !(0.0..=MAX_MARGIN).contains(&added.margin) {",
        "    if !(0.0..=MAX_MARGIN * 2.0).contains(&added.margin) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: a first number past the bound written",
        "src/addedtext.rs",
        "    if added.first_number > MAX_FIRST {",
        "    if added.first_number > MAX_FIRST * 2 {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: text past the length bound written",
        "src/addedtext.rs",
        "    if text.chars().count() > MAX_CHARS {",
        "    if text.chars().count() > MAX_CHARS * 2 {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    # --- Which pages, and which numbers ---------------------------------------------
    Mutation(
        "added text: the total not moved by the first number",
        "src/addedtext.rs",
        "        let total = line.first_number.saturating_sub(1) + u32::try_from(chosen.len()).unwrap_or(0);",
        "        let total = u32::try_from(chosen.len()).unwrap_or(0);",
        "a_page_left_out_is_not_counted_and_the_first_number_is_taken",
    ),
    Mutation(
        "added text: a page asked for twice not merged",
        "src/addedtext.rs",
        "                sorted.sort_unstable();\n                sorted.dedup();\n",
        "                sorted.sort_unstable();\n",
        "a_page_left_out_is_not_counted_and_the_first_number_is_taken",
    ),
    Mutation(
        "added text: the page past the last one let through",
        "src/addedtext.rs",
        "                if let Some(past) = asked.iter().find(|p| **p >= count) {",
        "                if let Some(past) = asked.iter().find(|p| **p > count) {",
        "what_cannot_be_written_is_refused_before_anything_is",
    ),
    Mutation(
        "added text: one page object listed twice written twice",
        "src/addedtext.rs",
        "            if let Some(first) = seen.insert(pages[*page as usize], *page) {",
        "            if let Some(first) = seen.insert(pages[*page as usize], *page).filter(|_| false) {",
        "a_page_listed_twice_is_refused_rather_than_numbered_twice",
    ),
    # --- How a line is drawn --------------------------------------------------------
    Mutation(
        "added text: the content's open q left open",
        "src/addedtext.rs",
        '        let mut body = "Q\\n".repeat(open + 1);',
        '        let mut body = "Q\\n".repeat(1);',
        "the_text_is_drawn_after_the_content_with_every_open_q_closed",
    ),
    Mutation(
        "added text: a Latin line set in the embedded font",
        "src/addedtext.rs",
        "                Some(font) if !crate::textbox::encodable(text) => Face::Embedded(font),",
        "                Some(font) => Face::Embedded(font),",
        "a_latin_line_stays_in_helvetica_beside_a_greek_one",
    ),
    Mutation(
        "added text: the fill opaque",
        "src/addedtext.rs",
        '                "ca" => Object::Real(line.opacity),',
        '                "ca" => Object::Real(1.0),',
        "a_watermark_is_turned_about_the_middle_and_translucent",
    ),
    Mutation(
        "added text: the angle turned the other way",
        "src/addedtext.rs",
        "    let along_dir = (cos * right.0 - sin * down.0, cos * right.1 - sin * down.1);",
        "    let along_dir = (cos * right.0 + sin * down.0, cos * right.1 + sin * down.1);",
        "a_watermark_is_turned_about_the_middle_and_translucent",
    ),
    Mutation(
        "added text: the left column at the page's edge",
        "src/addedtext.rs",
        "    let x = match line.position.column() {\n        0 => margin,",
        "    let x = match line.position.column() {\n        0 => 0.0,",
        "the_other_corners_and_sides_sit_inside_the_margin",
    ),
    Mutation(
        "added text: the right column starting at the margin",
        "src/addedtext.rs",
        "        1 => -advance / 2.0,\n        _ => -advance,",
        "        1 => -advance / 2.0,\n        _ => 0.0,",
        "the_other_corners_and_sides_sit_inside_the_margin",
    ),
    Mutation(
        "added text: the top row's capitals above the margin",
        "src/addedtext.rs",
        "        0 => size * cap_height,\n        1 => size * cap_height / 2.0,",
        "        0 => 0.0,\n        1 => size * cap_height / 2.0,",
        "the_other_corners_and_sides_sit_inside_the_margin",
    ),
    Mutation(
        "added text: a header marked as a footer",
        "src/addedtext.rs",
        '        0 => "<</Type /Pagination /Subtype /Header>>",',
        '        0 => "<</Type /Pagination /Subtype /Footer>>",',
        "each_line_is_a_pagination_artifact_with_its_row_as_subtype",
    ),
    # --- Comparing two documents ----------------------------------------------------
    Mutation(
        "compare: a soft hyphen kept in the word",
        "src/compare.rs",
        "                Some(SOFT_HYPHEN) => {",
        "                Some('\\u{0}') => {",
        "a_soft_hyphen_is_not_part_of_the_word_but_is_inside_its_span",
    ),
    Mutation(
        "compare: a repeated word taken as an anchor",
        "src/compare.rs",
        "            .filter(|(ca, _, cb, _)| *ca == 1 && *cb == 1)",
        "            .filter(|(ca, _, cb, _)| *ca >= 1 && *cb >= 1)",
        "a_word_repeated_on_the_older_side_is_not_an_anchor",
    ),
    Mutation(
        "compare: anchors out of order kept",
        "src/compare.rs",
        "        unique.sort_unstable();\n        longest_increasing(&unique)",
        "        unique.sort_unstable();\n        unique",
        "a_moved_paragraph_is_a_removal_and_an_addition",
    ),
    Mutation(
        "compare: the table used past its bound",
        "src/compare.rs",
        "        if a.len().saturating_mul(b.len()) <= MAX_TABLE_CELLS {",
        "        if true {",
        "a_stretch_too_large_for_the_table_is_reported_whole_and_counted",
    ),
    Mutation(
        "compare: the table never used",
        "src/compare.rs",
        "        if a.len().saturating_mul(b.len()) <= MAX_TABLE_CELLS {",
        "        if false {",
        "a_stretch_with_no_unique_word_in_common_is_aligned_by_the_table",
    ),
    Mutation(
        "compare: the list not cut at its bound",
        "src/compare.rs",
        "            if changes.len() < MAX_CHANGES {",
        "            if true {",
        "the_list_is_cut_at_its_bound_and_the_total_is_not",
    ),
    Mutation(
        "compare: the total counts only the listed changes",
        "src/compare.rs",
        "            total += 1;\n            if changes.len() < MAX_CHANGES {",
        "            if changes.len() < MAX_CHANGES {\n                total += 1;",
        "the_list_is_cut_at_its_bound_and_the_total_is_not",
    ),
    Mutation(
        "compare: a removal called a replacement",
        "src/compare.rs",
        "        (false, true) => Kind::Removed,",
        "        (false, true) => Kind::Replaced,",
        "a_removal_at_the_end_is_placed_after_the_last_right_word",
    ),
    Mutation(
        "compare: the end placed before the last word",
        "src/compare.rs",
        "            index: last.end,",
        "            index: last.start,",
        "a_removal_at_the_end_is_placed_after_the_last_right_word",
    ),
    Mutation(
        "compare: one span across a page break",
        "src/compare.rs",
        "            Some(span) if span.page == word.page => span.end = word.end,",
        "            Some(span) => span.end = word.end,",
        "a_change_over_a_page_break_has_one_span_per_page",
    ),
    Mutation(
        "compare: a long quote not cut",
        "src/compare.rs",
        "            if chars >= limit {",
        "            if chars >= limit + 100_000 {",
        "a_quote_is_cut_with_an_ellipsis",
    ),
    # --- Text set at an angle -------------------------------------------------------
    Mutation(
        "oblique: no character counted as oblique",
        "src/text.rs",
        "    if (turns - turns.round()).abs() * 90.0 > OBLIQUE_DEGREES {",
        "    if false && (turns - turns.round()).abs() * 90.0 > OBLIQUE_DEGREES {",
        "text_character_turns_accept_float_quarters_without_promoting_skew",
    ),
    Mutation(
        "oblique: nine degrees counted as oblique",
        "src/text.rs",
        "const OBLIQUE_DEGREES: f32 = 10.0;",
        "const OBLIQUE_DEGREES: f32 = 8.0;",
        "text_character_turns_accept_float_quarters_without_promoting_skew",
    ),
    Mutation(
        "oblique: eleven degrees counted as upright",
        "src/text.rs",
        "const OBLIQUE_DEGREES: f32 = 10.0;",
        "const OBLIQUE_DEGREES: f32 = 12.0;",
        "text_character_turns_accept_float_quarters_without_promoting_skew",
    ),
    Mutation(
        "oblique: read in its quarter turn's group",
        "src/reading.rs",
        "    if view.char_turns.get(index) == Some(&crate::text::OBLIQUE_TURN) {",
        "    if false && view.char_turns.get(index) == Some(&crate::text::OBLIQUE_TURN) {",
        "an_oblique_watermark_is_read_after_the_lines_it_crosses",
    ),
    Mutation(
        "oblique: not read last",
        "src/reading.rs",
        "        (a.0 == OBLIQUE)\n            .cmp(&(b.0 == OBLIQUE))\n            .then(by(ea.cross_start, eb.cross_start))",
        "        by(ea.cross_start, eb.cross_start)",
        "an_oblique_watermark_is_read_after_the_lines_it_crosses",
    ),
]
