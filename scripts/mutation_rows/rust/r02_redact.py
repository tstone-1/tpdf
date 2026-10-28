"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

ROWS += [
    Mutation(
        # Decrement `/Count` once per doomed OBJECT rather than once per page
        # NUMBER, leaving a tree that claims a page it does not have.
        "pagetree: charge a shared page's deletion to /Count once, not once per number",
        "src/pagetree.rs",
        "    for number in numbers {",
        "    for number in numbers.iter().take(1) {",
        "a_shared_page_costs_the_tree_one_count_per_number_it_answered_to",
    ),
    Mutation(
        # Compose each page's turn without reconciling the objects, which is the
        # loop that was there before `agreed_turns` existed: a shared page's
        # second visit reads the 90 the first wrote and leaves 180.
        "save: turn a shared page once per page number rather than once per object",
        "src/save.rs",
        "    turn_pages(&mut doc, &agreed_turns(&turns)?, &written)?;",
        "    turn_pages(&mut doc, &turns, &written)?;",
        "a_page_reached_twice_is_turned_once",
    ),
    Mutation(
        # Accept a plan that asks one shared page for two different turns, and
        # silently apply whichever came first.
        "save: accept two different turns for one shared page",
        "src/pagetree.rs",
        "            Some(&(first, first_at)) if first != extra => {",
        "            Some(&(first, first_at)) if first != extra && false => {",
        "a_page_reached_twice_cannot_be_turned_two_ways",
    ),
    Mutation(
        # The over-refusal direction: refuse a shared page even when nothing
        # conflicts, which would deny saving an unedited document whose page tree
        # happens to be malformed.
        "save: refuse a shared page even when its turns agree",
        "src/pagetree.rs",
        "            Some(&(first, first_at)) if first != extra => {",
        "            Some(&(first, first_at)) if first == extra || first != extra => {",
        "a_page_reached_twice_is_saved_normally_when_nothing_conflicts",
    ),
    Mutation(
        # Write a rotation onto a page nobody turned. Invisible on every fixture
        # whose pages state their own rotation; on one that inherits it, the value
        # written is whatever `effective_rotation` answered, which is 0 whenever
        # its 64-hop walk gives up. It survived until the test stopped asserting
        # the effective rotation --- which is 90 either way --- and started
        # asserting that the untouched page states no `/Rotate` of its own.
        "pagetree: write a rotation onto every page, turned or not",
        "src/pagetree.rs",
        "        if extra == 0 {",
        "        if false {",
        "a_page_that_was_not_turned_keeps_an_inherited_rotation",
    ),
    Mutation(
        # Reparent every page and take nothing with it. `/MediaBox`, `/Rotate`,
        # `/Resources` and `/CropBox` are inheritable, so a page that hung under
        # a tree node stating one loses it the moment it hangs off the root ---
        # producing a document that opens, looks plausible, and has a page at
        # the wrong size or the wrong angle.
        "pagetree: reorder without carrying what a page inherited",
        "src/pagetree.rs",
        "                    pushes.push((page, key, value));",
        "                    let _ = value;",
        "a_reordered_page_takes_what_it_inherited_with_it",
    ),
    Mutation(
        # The other direction: write every inherited attribute onto every page.
        # It costs pages nobody moved their byte-for-byte identity, and for
        # `/Rotate` it is the flattening `apply_turns` deliberately avoids --- a
        # page whose `/Parent` chain outruns the bound reads back as upright.
        "pagetree: push an inherited value down even when the root already supplies it",
        "src/pagetree.rs",
        "                Some(value) if Some(&value) != from_root[at].as_ref() => {",
        "                Some(value) => {",
        "a_page_that_inherits_from_the_root_is_not_written_to",
    ),
    Mutation(
        # Leave `/Count` at whatever the file declared. The tree then claims
        # pages a reader walking `/Kids` will not find, which is a document
        # every parser is entitled to reject.
        "pagetree: leave the rebuilt tree claiming the page count the file had",
        "src/pagetree.rs",
        "    tree.set(\"Count\", order.len() as i64);",
        "",
        "the_flattened_tree_counts_what_it_holds_and_owns_every_page",
    ),
    Mutation(
        # Rebuild `/Kids` and leave every page pointing at the node it used to
        # hang under. The document reads correctly --- `get_pages` walks down
        # from the root --- and every walk *up* a `/Parent` chain, this module's
        # included, resolves against an ancestry that is no longer there.
        "pagetree: rebuild the tree without telling the pages who their parent is",
        "src/pagetree.rs",
        "            .set(\"Parent\", Object::Reference(root));",
        "            .set(\"NotParent\", Object::Reference(root));",
        "the_flattened_tree_counts_what_it_holds_and_owns_every_page",
    ),
    Mutation(
        # Rebuild the tree for every save. The document is identical either way,
        # so nothing a reader sees can tell --- and every page of every copy has
        # been reparented for a plan that moved nothing.
        "save: rebuild the page tree even when nothing moved",
        "src/save.rs",
        "    let moved =\n        baselines.len() != plan.pages.len() || baselines.windows(2).any(|two| two[0] >= two[1]);",
        "    let moved = true;",
        "a_plan_in_document_order_leaves_the_page_tree_as_it_found_it",
    ),
    Mutation(
        # The print half of the same defect, and it was live in shipped code as
        # a documented property: a subset came out in document order whatever
        # order it was asked for.
        "print: hand the printer the pages in the file's order rather than the job's",
        "src/print.rs",
        "    let moved = wanted.windows(2).any(|two| two[0].number >= two[1].number);",
        "    let moved = false;",
        "a_job_prints_its_pages_in_the_order_it_lists_them",
    ),
    Mutation(
        # And its over-application, which no page a reader sees can distinguish.
        "print: rebuild the page tree for a job whose pages never moved",
        "src/print.rs",
        "    let moved = wanted.windows(2).any(|two| two[0].number >= two[1].number);",
        "    let moved = true;",
        "a_job_in_document_order_keeps_the_page_tree_the_file_had",
    ),
    Mutation(
        # Drop the anchor and put every moved page at the front. Half of all
        # moves then land where they were asked to, which is the shape that
        # survives a test suite reading only the page count.
        "edits: move every page to the front whatever anchor was named",
        "src/edits.rs",
        "                after: after.map(PageId::from_raw),",
        "                after: None,",
        "a_moved_page_lands_behind_the_page_it_named_and_keeps_its_identity",
    ),
    Mutation(
        # Call a plan the file on disk whenever it has every page unturned,
        # which a reordered document does. The print path then hands the file
        # over byte for byte and the reader's rearrangement never reaches paper.
        "edits: read a plan's length and turns as meaning it is the file on disk",
        "src/edits.rs",
        "                let theirs = match source {\n                    PageSource::Baseline(n) => *n as usize == at,\n                    PageSource::Blank(_) => false,\n                    PageSource::Imported { .. } => false,\n                };",
        "                let theirs = {\n                    let _ = (source, at);\n                    true\n                };",
        "a_plan_after_a_move_is_out_of_document_order",
    ),
    Mutation(
        # Keep every page whatever the plan says. The copy then comes out with
        # the page the reader deleted still in it, which is the whole feature
        # silently doing nothing.
        "save: keep a page the plan does not name",
        "src/save.rs",
        "            .filter(|number| !kept.contains(number))",
        "            .filter(|_number| false)",
        "a_third_parser_sees_the_pages_that_were_kept_and_not_the_one_that_was_not",
    ),
    Mutation(
        # Resolve each plan entry one page late. Every turn then lands on the
        # page after the one it was aimed at --- which on an unedited document is
        # invisible, because every turn is zero.
        "save: aim each turn at the page after the one the plan named",
        "src/save.rs",
        "                    Slot::Kept(*pages.get(number as usize).ok_or_else(|| {",
        "                    Slot::Kept(*pages.get(number as usize + 1).ok_or_else(|| {",
        "a_turn_on_a_page_after_the_deleted_one_lands_where_it_was_aimed",
    ),
    Mutation(
        # Let a shared page be half-deleted. `drop_pages` keeps any object a
        # surviving number names, so the deletion silently does nothing and the
        # copy has the page in it.
        "save: accept a deletion that removes one of two numbers naming one page",
        "src/save.rs",
        "        unshared(&pages, &kept, &dropped)?;",
        "        let _ = unshared(&pages, &kept, &dropped);",
        "deleting_one_of_two_numbers_that_are_one_page_is_refused",
    ),
    Mutation(
        # The over-refusal direction of the same guard: refuse a deletion whose
        # doomed page nothing else names, which is every ordinary deletion.
        "save: refuse a deletion that no kept page shares",
        "src/save.rs",
        "        let Some(shared) = kept.iter().find(|keep| at(keep) == Some(id)) else {",
        "        let Some(shared) = kept.iter().find(|_keep| true) else {",
        "deleting_both_numbers_of_a_shared_page_is_not_refused",
    ),
    Mutation(
        # Write a reordered plan in the order the file already has. The pages
        # come out in the wrong order, in a file that opens and prints.
        #
        # It named the refusal this replaced --- and when that test went, the
        # harness refused to start rather than reporting the mutation survived.
        # Same edit, same defect, and now a test that asserts the pages came out
        # where the reader put them instead of that the save declined to try.
        "save: write a reordered plan in file order rather than in the reader's",
        "src/save.rs",
        "|| baselines.windows(2).any(|two| two[0] >= two[1]);",
        "|| false;",
        "a_plan_whose_pages_have_moved_comes_out_in_the_order_the_reader_put_them",
    ),
    Mutation(
        # Number the ordinals by every object rather than by text objects. An
        # image between two lines then shifts the numbering, and the redaction
        # removes a line the reader did not mark while reporting success.
        "redact: count every page object when numbering the text ones",
        "src/redact.rs",
        "        let is_text = object.kind == \"text\";\n        let ordinal = text_ordinal;\n        if is_text {\n            text_ordinal += 1;\n        }",
        "        let is_text = object.kind == \"text\";\n        let ordinal = text_ordinal;\n        text_ordinal += 1;",
        "an_image_between_two_lines_does_not_shift_the_text_ordinals",
    ),
    Mutation(
        # Delete front-first. The second removal then lands on whatever moved
        # into the slot -- invisible with two lines, wrong with three.
        "redact: remove show operators front-first instead of back-first",
        "src/redact.rs",
        # Widened when `remove_images` arrived carrying the same three-line loop.
        # Third sibling in one session, and `docs/TRAPS.md` records the shape:
        # a near-copy makes an existing anchor ambiguous without the anchor
        # moving, and an ambiguous anchor is refused. The operand is what tells
        # the two apart -- one removes from the page's content, the other from a
        # decoded copy of it that is written back whole.
        "    for at in positions.into_iter().rev() {\n"
        "        let stays = without_drawing(&content.operations[at]);",
        "    for at in positions.into_iter() {\n"
        "        let stays = without_drawing(&content.operations[at]);",
        "removing_two_operators_removes_the_two_that_were_named",
    ),
    Mutation(
        # Proceed when PDFium's object count and the operator count disagree,
        # which is a redaction that removes the wrong words and says it worked.
        "redact: remove by position even when the two counts disagree",
        "src/redact.rs",
        # Widened when `remove_form_shows` arrived carrying the same guard one
        # level down. `docs/TRAPS.md` records the shape: a near-copy of a
        # function makes an existing anchor ambiguous without the anchor moving,
        # and an ambiguous anchor is refused, so the mutation stops being able to
        # fail. The page's message is what distinguishes them.
        '    if shows.len() != text_objects {\n        return Err(said!(\n            "the page has',
        '    if false {\n        return Err(said!(\n            "the page has',
        "a_count_that_disagrees_with_pdfium_refuses_and_removes_nothing",
    ),
    # The colour of a redaction's boxes, 2026-10-07.
    Mutation(
        "redaction fill: draw white as black",
        "src/redaction_fill.rs",
        "            Self::White => [1.0, 1.0, 1.0],",
        "            Self::White => [0.0, 0.0, 0.0],",
        "the_boxes_are_the_colour_that_was_asked_for_in_all_three_places",
    ),
    Mutation(
        "redaction fill: fill the appearance black whatever was asked",
        "src/redaction_fill.rs",
        "lopdf::content::Operation::new(\"rg\", colour.clone()),",
        "lopdf::content::Operation::new(\"rg\", vec![0.into(), 0.into(), 0.into()]),",
        "the_boxes_are_the_colour_that_was_asked_for_in_all_three_places",
    ),
    Mutation(
        "redaction fill: name black as the interior whatever was asked",
        "src/redaction_fill.rs",
        "                \"IC\" => colour.clone(),",
        "                \"IC\" => vec![0.into(), 0.into(), 0.into()],",
        "the_boxes_are_the_colour_that_was_asked_for_in_all_three_places",
    ),
    Mutation(
        "redaction fill: read the name white as black",
        "src/redaction_fill.rs",
        "            \"white\" => Some(Self::White),",
        "            \"white\" => Some(Self::Black),",
        "a_fill_is_black_unless_another_is_named",
    ),
    Mutation(
        "redaction fill: make white the fill nobody chose",
        "src/cli/redact.rs",
        "        fill: crate::redaction_fill::Fill::default(),",
        "        fill: crate::redaction_fill::Fill::White,",
        "the_fill_is_black_unless_one_of_the_three_is_named",
    ),
    Mutation(
        "redaction fill: take any word for a colour",
        "src/cli/redact.rs",
        "                command.fill = crate::redaction_fill::Fill::named(name).ok_or_else(|| {",
        "                command.fill = crate::redaction_fill::Fill::named(name).or(Some(Default::default())).ok_or_else(|| {",
        "the_fill_is_black_unless_one_of_the_three_is_named",
    ),
    # The image-only copy in the colour chosen, 2026-10-08. What reads the
    # painted page back and refuses another colour needs PDFium and is held by
    # `raster-redact-probe`, which no mutation here runs.
    Mutation(
        "redaction fill: give every colour the pixel of black",
        "src/redaction_fill.rs",
        "        self.rgb().map(|part| (part * 255.0).round() as u8)",
        "        self.rgb().map(|part| (part * 0.0).round() as u8)",
        "a_fill_is_one_pixel_colour",
    ),
    Mutation(
        "image-only copy: paint the marked pixels black whatever was chosen",
        "src/raster_redact.rs",
        "                pixel.copy_from_slice(&colour);",
        "                pixel.copy_from_slice(&[0, 0, 0]);",
        "strip_mask_removes_only_intersecting_rgb_pixels",
    ),
    # A document path split at its spaces by the restart after an update,
    # 2026-10-08 (`launch.rs`).
    Mutation(
        "launch: join the arguments whether or not they are a file together",
        "src/launch.rs",
        "        if !one_is_a_file && is_file(&whole) {",
        "        if !one_is_a_file {",
        "pieces_that_are_no_file_together_stay_apart",
    ),
    Mutation(
        "launch: join two documents that each exist",
        "src/launch.rs",
        "        if !one_is_a_file && is_file(&whole) {",
        "        if is_file(&whole) {",
        "two_documents_that_exist_are_two_documents",
    ),
    Mutation(
        "launch: leave a split path in its pieces",
        "src/launch.rs",
        "    if pieces.len() > 1 {",
        "    if pieces.len() > usize::MAX - 1 {",
        "a_path_split_at_its_spaces_is_put_back_together",
    ),
    Mutation(
        "launch: leave the hyphen out of the path that is put back",
        "src/launch.rs",
        '        let whole = PathBuf::from(pieces.join(" "));',
        '        let whole = PathBuf::from(pieces.iter().filter(|piece| !piece.starts_with(\'-\')).cloned().collect::<Vec<_>>().join(" "));',
        "a_path_split_at_its_spaces_is_put_back_together",
    ),
    # Taking part of a show operator, 2026-10-07: the glyphs under a region
    # and no others, with a gap as wide as they were (`redact/glyph_cut.rs`).
    Mutation(
        "glyph cut: take a glyph the region only grazes",
        "src/redact/glyph_cut.rs",
        "                under.push(Some(share > UNDER));",
        "                under.push(Some(share > 0.0));",
        "a_glyph_goes_when_more_than_a_tenth_of_it_is_under_the_region",
    ),
    Mutation(
        "glyph cut: take a space when either side of it goes",
        "src/redact/glyph_cut.rs",
        "                    (Some(a), Some(b)) => a && b,",
        "                    (Some(a), Some(b)) => a || b,",
        "a_space_goes_with_the_glyphs_on_both_sides_of_it",
    ),
    Mutation(
        "glyph cut: treat a letter with no box as a space",
        "src/redact/glyph_cut.rs",
        "            None => return None,\n        }\n    }\n    let mut take",
        "            None => under.push(None),\n        }\n    }\n    let mut take",
        "a_glyph_with_no_box_that_is_not_white_space_cannot_be_cut_around",
    ),
    Mutation(
        "glyph cut: cut a show whose codes are not as many as its glyphs",
        "src/redact/glyph_cut.rs",
        "            .filter(|text| fact.codes == Some(text.glyphs.len()))",
        "            .filter(|_| true)",
        "a_show_that_cannot_be_described_goes_whole",
    ),
    Mutation(
        "glyph cut: take a show whole when none of its glyphs is under the region",
        "src/redact/glyph_cut.rs",
        "            if take.is_empty() {\n                continue;\n            }",
        "            if take.is_empty() {\n                whole.push(ordinal);\n                continue;\n            }",
        "a_show_with_no_glyph_under_the_region_leaves_the_plan",
    ),
    Mutation(
        "glyph cut: cut a show the region covers all of",
        "src/redact/glyph_cut.rs",
        "            if !all && !(to_the_end",
        "            if !(to_the_end",
        "a_show_wholly_under_a_region_still_goes_whole",
    ),
    Mutation(
        "glyph cut: cut to the end with no place for the pen",
        "src/redact/glyph_cut.rs",
        "(to_the_end && fact.carries_on && text.tail.is_none())",
        "(to_the_end && fact.carries_on && false)",
        "a_cut_to_the_end_needs_the_next_start_only_when_the_pen_carries_on",
    ),
    Mutation(
        "glyph cut: also cut a show another region takes whole",
        "src/redact/glyph_cut.rs",
        "        if shows.binary_search(&cut.ordinal).is_ok() {\n            continue;\n        }",
        "",
        "a_show_another_region_takes_whole_is_not_also_cut",
    ),
    Mutation(
        "glyph cut: keep a cut that takes every glyph",
        "src/redact/glyph_cut.rs",
        "        let whole = cut.takes_all() && cut.tail.is_none();",
        "        let whole = false;",
        "cuts_that_add_up_to_the_whole_show_take_it_whole",
    ),
    Mutation(
        "glyph cut: replace the second region's cut with the first's",
        "src/redact/glyph_cut.rs",
        "                have.take.extend(cut.take);",
        "",
        "two_regions_over_one_show_cut_it_once_for_both",
    ),
    Mutation(
        "glyph cut: move the pen back where it should go forwards",
        "src/redact/glyph_cut.rs",
        "                            shown.push(Object::Real(cut.pens[from] - pen));",
        "                            shown.push(Object::Real(pen - cut.pens[from]));",
        "a_run_in_the_middle_becomes_a_gap_as_wide_as_it_was",
    ),
    Mutation(
        "glyph cut: keep a kern inside what goes",
        "src/redact/glyph_cut.rs",
        "                if open.is_none() {\n                    shown.push(number);\n                }",
        "                shown.push(number);",
        "a_kern_inside_what_goes_is_dropped_and_one_outside_it_stays",
    ),
    Mutation(
        "glyph cut: leave the pen short of the show that follows",
        "src/redact/glyph_cut.rs",
        "            shown.push(Object::Real(cut.pens[from] - tail));",
        "            let _ = (from, tail);",
        "a_run_to_the_end_leaves_a_gap_up_to_a_show_that_starts_at_the_pen",
    ),
    Mutation(
        "glyph cut: move the pen for a show that is placed anyway",
        "src/redact/glyph_cut.rs",
        "        if continues {\n            let tail",
        "        if true {\n            let tail",
        "a_run_to_the_end_needs_no_gap_when_the_next_show_is_placed",
    ),
    Mutation(
        "glyph cut: forget that a quote moves to the next line",
        "src/redact/glyph_cut.rs",
        "        (\"'\", [text]) => {\n            out.push(Operation::new(\"T*\", Vec::new()));",
        "        (\"'\", [text]) => {",
        "a_quote_operator_keeps_what_it_did_before_showing",
    ),
    Mutation(
        "glyph cut: forget the spacing a double quote sets",
        "src/redact/glyph_cut.rs",
        "            out.push(Operation::new(\"Tw\", vec![word.clone()]));\n",
        "",
        "a_quote_operator_keeps_what_it_did_before_showing",
    ),
    Mutation(
        "glyph cut: keep the font set inside a saved state",
        "src/redact/glyph_cut.rs",
        "                \"Q\" => now = saved.pop().flatten(),",
        "                \"Q\" => {}",
        "the_font_in_force_follows_save_and_restore",
    ),
    Mutation(
        "glyph cut: read a composite font a byte at a time",
        "src/redact/glyph_cut.rs",
        "            Object::Name(name) if name == b\"Identity-H\" => Some(2),",
        "            Object::Name(name) if name == b\"Identity-H\" => Some(1),",
        "a_composite_font_is_cut_between_its_two_byte_codes",
    ),
    Mutation(
        "glyph cut: count the codes of a CMap nobody has read",
        "src/redact/glyph_cut.rs",
        "            Object::Name(name) if name == b\"Identity-H\" => Some(2),\n            _ => None,",
        "            Object::Name(name) if name == b\"Identity-H\" => Some(2),\n            _ => Some(2),",
        "the_facts_of_each_show_are_its_codes_and_whether_the_pen_carries_on",
    ),
    Mutation(
        "glyph cut: let the pen carry on across a Td",
        "src/redact/glyph_cut.rs",
        "            \"Td\" | \"TD\" | \"Tm\" | \"T*\" | \"'\" | \"\\\"\" | \"ET\" | \"BT\" => return false,",
        "            \"TD\" | \"Tm\" | \"T*\" | \"'\" | \"\\\"\" | \"ET\" | \"BT\" => return false,",
        "the_facts_of_each_show_are_its_codes_and_whether_the_pen_carries_on",
    ),
    Mutation(
        "glyph cut: answer for a page whose shows are not as many as expected",
        "src/redact/glyph_cut.rs",
        "    (facts.len() == expected).then_some(facts)",
        "    Some(facts)",
        "the_facts_of_each_show_are_its_codes_and_whether_the_pen_carries_on",
    ),
    Mutation(
        "glyph cut: accept a cut with more pens than the show has codes",
        "src/redact/glyph_cut.rs",
        "    if glyph != cut.pens.len() {\n        return Err(wrong());\n    }",
        "",
        "a_cut_that_does_not_fit_its_show_is_refused_and_writes_nothing",
    ),
    Mutation(
        "glyph cut: cut by position when the two counts disagree",
        "src/redact/glyph_cut.rs",
        "    if shows.len() != text_objects {\n        return Err(said!(\n            \"the page has {count} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Cutting",
        "    if false {\n        return Err(said!(\n            \"the page has {count} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Cutting",
        "a_cut_that_does_not_fit_its_show_is_refused_and_writes_nothing",
    ),
    Mutation(
        "glyph cut: cut one show twice",
        "src/redact/glyph_cut.rs",
        "        if work.iter().any(|(have, _)| *have == at) {",
        "        if false {",
        "a_cut_that_does_not_fit_its_show_is_refused_and_writes_nothing",
    ),
    Mutation(
        "glyph cut: leave the alternate text around a cut show",
        "src/redact/glyph_cut.rs",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;\n\n    // Backwards",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &[])?;\n    let _ = &positions;\n\n    // Backwards",
        "the_alternate_text_around_a_cut_show_is_cleared",
    ),
    Mutation(
        "glyph cut: cut the shows of a page front to back",
        "src/redact/glyph_cut.rs",
        "    for (at, cut) in work.into_iter().rev() {",
        "    for (at, cut) in work.into_iter() {",
        "two_cuts_on_one_page_each_land_on_their_own_show",
    ),
    Mutation(
        "glyph cut: cut a show other than the one named",
        "src/redact/glyph_cut.rs",
        "        let at = *shows.get(cut.ordinal).ok_or_else(|| {",
        "        let at = *shows.get(cut.ordinal + 1).ok_or_else(|| {",
        "only_the_named_show_is_cut_and_empty_shows_are_passed_over",
    ),
    Mutation(
        "glyphs: accept a pen that is off the baseline",
        "src/objects.rs",
        "            if !pen.is_finite() || (off - off_line).abs() > BASELINE_SLACK {",
        "            if !pen.is_finite() {",
        "an_object_that_is_not_a_row_of_glyphs_running_forwards_has_no_glyphs",
    ),
    Mutation(
        "glyphs: accept pens that run backwards",
        "src/objects.rs",
        "                .is_some_and(|before| pen < before.pen - PEN_SLACK)",
        "                .is_some_and(|_| false)",
        "an_object_that_is_not_a_row_of_glyphs_running_forwards_has_no_glyphs",
    ),
    Mutation(
        "glyphs: measure a pen without the matrix's scale",
        "src/objects.rs",
        "    let em = f64::from(size) * along;",
        "    let em = f64::from(size);",
        "the_matrix_and_the_size_take_the_scale_out_of_a_pen",
    ),
    Mutation(
        "glyphs: measure a pen along the page and not the baseline",
        "src/objects.rs",
        "    let (ux, uy) = (a / along, b / along);",
        "    let (ux, uy) = (1.0, 0.0);",
        "a_turned_baseline_is_measured_along_itself",
    ),
    Mutation(
        "glyphs: make a glyph of every character",
        "src/objects.rs",
        "        if last != Some(one.origin) {",
        "        if true {",
        "characters_at_one_pen_are_one_glyph",
    ),
    Mutation(
        "glyphs: take a start behind the last pen for the tail",
        "src/objects.rs",
        "        .filter(|tail| tail.is_finite() && *tail >= end - PEN_SLACK);",
        "        .filter(|tail| tail.is_finite());",
        "the_tail_is_where_the_next_object_starts_along_this_baseline",
    ),
    Mutation(
        "redact: do not count a cut as a removal",
        "src/redact.rs",
        "    let mut total = shows.len() + show_cuts.len();",
        "    let mut total = shows.len();",
        "a_cut_is_counted_and_carries_its_line_beside_what_it_takes",
    ),
    Mutation(
        "redact: drop the line a cut came out of",
        "src/redact.rs",
        "        if !line.is_empty() {\n            lines.push(line.to_string());\n        }",
        "",
        "a_cut_is_counted_and_carries_its_line_beside_what_it_takes",
    ),
    Mutation(
        "redact: do not merge the cuts of two regions",
        "src/redact.rs",
        "    let show_cuts = merge_cuts(show_cuts, &mut shows);",
        "    let _ = merge_cuts(Vec::new(), &mut shows);",
        "a_cut_is_counted_and_carries_its_line_beside_what_it_takes",
    ),
    # A show operator that draws nothing, 2026-10-07. PDFium makes no text
    # object of one, so counting it puts the two lists out of step.
    Mutation(
        "redact: count every show on the page as a text object",
        "src/redact.rs",
        "        .filter(|(_, operation)| makes_text_object(operation))\n        .map(|(at, _)| at)\n        .collect();\n\n    // The correspondence guard.",
        "        .filter(|(_, operation)| matches!(operation.operator.as_str(), \"Tj\" | \"TJ\" | \"'\" | \"\\\"\"))\n        .map(|(at, _)| at)\n        .collect();\n\n    // The correspondence guard.",
        "empty_shows_are_passed_over_and_left_where_they_are",
    ),
    Mutation(
        "redact: count every show in a form as a text object",
        "src/redact.rs",
        "        .filter(|(_, operation)| makes_text_object(operation))\n        .map(|(at, _)| at)\n        .collect();\n    if shows.len() != text_objects {",
        "        .filter(|(_, operation)| matches!(operation.operator.as_str(), \"Tj\" | \"TJ\" | \"'\" | \"\\\"\"))\n        .map(|(at, _)| at)\n        .collect();\n    if shows.len() != text_objects {",
        "empty_shows_in_a_form_are_passed_over_and_left_where_they_are",
    ),
    Mutation(
        "redact: count an empty string as text",
        "src/redact.rs",
        "    let text = |object: &Object| matches!(object, Object::String(bytes, _) if !bytes.is_empty());",
        "    let text = |object: &Object| matches!(object, Object::String(..));",
        "a_show_without_text_is_not_a_text_object",
    ),
    Mutation(
        "redact: do not count a name shown as text",
        "src/redact.rs",
        "            text(last) || matches!(last, Object::Name(name) if !name.is_empty())",
        "            text(last)",
        "a_show_without_text_is_not_a_text_object",
    ),
    Mutation(
        "redact: read a show's first operand in place of its last",
        "src/redact.rs",
        "        \"Tj\" | \"'\" | \"\\\"\" => operation.operands.last().is_some_and(|last| {",
        "        \"Tj\" | \"'\" | \"\\\"\" => operation.operands.first().is_some_and(|last| {",
        "a_show_without_text_is_not_a_text_object",
    ),
    Mutation(
        "redact: count any array shown as text",
        "src/redact.rs",
        "matches!(last, Object::Array(parts) if parts.iter().any(text))",
        "matches!(last, Object::Array(_))",
        "a_show_without_text_is_not_a_text_object",
    ),
    Mutation(
        "redact: count no array shown as text",
        "src/redact.rs",
        "        \"TJ\" => operation\n            .operands\n            .last()\n",
        "        \"TJ\" => operation\n            .operands\n            .last()\n            .filter(|_| false)\n",
        "the_two_quote_operators_are_show_operators",
    ),
    Mutation(
        # Treat a shared edge as an overlap, so a region drawn flush against a
        # line silently eats it.
        # Re-aimed 2026-09-06 with the page-object vocabulary: `overlaps` and
        # `normalised` moved to `objects.rs`, beside the code that produces the
        # boxes they compare. The test stays in `redact.rs`, which still reaches
        # both through the re-export, so only the file this names moved.
        "objects: treat two rectangles that only touch as overlapping",
        "src/objects.rs",
        "    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]",
        "    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]",
        "a_region_flush_against_a_line_does_not_eat_it",
    ),
    Mutation(
        # Say nothing about an object that cannot be removed. PLAN.md section 6's
        # deny-by-default rule inverted: the words go and the picture of the
        # words stays, and the plan calls itself complete.
        # Re-aimed 2026-08-26, when the finding became a kind and a position
        # rather than a sentence, and widened 2026-08-27, when a form's children
        # gained a push of their own and the shorter anchor matched both -- the
        # `} else {` is what makes this the page level's.
        "redact: pass over an image in the region without reporting it",
        "src/redact.rs",
        "        } else {\n            plan.unhandled.push(Unhandled {",
        "        } else {\n            drop(Unhandled {",
        "an_object_this_cannot_remove_makes_the_plan_incomplete",
    ),
    Mutation(
        # Forget the two quote forms, so a redaction passes over any line drawn
        # with one.
        "redact: stop recognising the quote text-showing operators",
        "src/redact.rs",
        "    matches!(operator, \"Tj\" | \"TJ\" | \"'\" | \"\\\"\")",
        "    matches!(operator, \"Tj\" | \"TJ\")",
        "the_two_quote_operators_are_show_operators",
    ),
    Mutation(
        # Leave the shadow text where it is. The glyphs go and `/ActualText`
        # keeps a verbatim copy of the words beside the hole they left --- which
        # is the redaction that reports success and removes nothing a reader
        # cares about.
        "redact: leave the shadow text on a span the removal emptied",
        "src/redact.rs",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &[])?;",
        "a_span_the_removal_touched_loses_its_copy_of_the_words",
    ),
    Mutation(
        # Clear the carriers after the deletion has renumbered the operations.
        # One span cannot tell the two orders apart; with two, the second span's
        # `EMC` has moved by the time the walk reaches it and its copy survives.
        "redact: clear the carriers after the removal has renumbered the operations",
        "src/redact.rs",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;\n"
        "\n"
        "    for at in positions.into_iter().rev() {\n"
        "        let stays = without_drawing(&content.operations[at]);\n"
        "        content.operations.splice(at..=at, stays);\n"
        "    }",
        "    for at in positions.clone().into_iter().rev() {\n"
        "        let stays = without_drawing(&content.operations[at]);\n"
        "        content.operations.splice(at..=at, stays);\n"
        "    }\n"
        "\n"
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;",
        "two_spans_each_holding_a_removed_line_both_lose_their_shadow_text",
    ),
    Mutation(
        # Mark only the innermost open span. An outer `/ActualText` restates
        # everything inside it, so the removed line stays in the file one level
        # up.
        "redact: mark only the innermost span rather than every open one",
        "src/redact.rs",
        "            for frame in &mut open {\n                frame.1 = true;\n            }",
        "            if let Some(frame) = open.last_mut() {\n                frame.1 = true;\n            }",
        "an_enclosing_span_loses_its_shadow_text_as_well",
    ),
    Mutation(
        # Drop a span the stream never closed. Malformed input, and the carrier
        # then survives in exactly the file least likely to be looked at twice.
        "redact: drop a marked-content span the stream never closed",
        "src/redact.rs",
        "    for (start, inside) in open {",
        "    for (start, inside) in open.into_iter().take(0) {",
        "a_span_that_was_never_closed_still_loses_its_shadow_text",
    ),
    Mutation(
        # Never follow an /MCID into the structure tree. The words go from the
        # page and from its property list, and the element that owns that same
        # marked content keeps a verbatim copy nothing on the page mentions.
        "redact: leave the structure tree alone after a removal",
        "src/redact.rs",
        "    let struct_carriers = clear_struct_shadow_text(doc, page, &carriers.mcids);",
        "    let struct_carriers = clear_struct_shadow_text(doc, page, &[]);",
        "the_structure_element_a_removed_span_belongs_to_loses_its_shadow_text",
    ),
    Mutation(
        # Clear the element and stop. An ancestor restates everything beneath it,
        # so the removed line survives one level up.
        "redact: clear the element a span owns but not the ones above it",
        "src/redact.rs",
        "        for _ in 0..MAX_ANCESTORS {",
        "        for _ in 0..1 {",
        "an_ancestor_of_that_element_loses_its_shadow_text_as_well",
    ),
    Mutation(
        # Take every element the page's entry lists rather than the ones the
        # removal reached, which is the alternate text of every line on the page.
        "redact: strip the whole parent-tree entry rather than the spans reached",
        "src/redact.rs",
        "    for mcid in mcids {",
        "    for mcid in &(0..entries.len() as i64).collect::<Vec<i64>>() {",
        "the_element_for_a_line_nobody_redacted_keeps_its_shadow_text",
    ),
    Mutation(
        # Read the parent tree as one flat /Nums. Every document large enough for
        # a producer to balance the tree then reports no structure carrier at
        # all -- silently, because a miss and an untagged page look the same.
        "redact: read a parent tree's /Nums and never its /Kids",
        "src/redact.rs",
        "    let kids = node\n        .get(b\"Kids\")",
        "    let kids = node\n        .get(b\"NoKids\")",
        "a_parent_tree_written_as_kids_is_followed",
    ),
    Mutation(
        # Step the wave at the period the quad's aspect ratio asks for. The
        # trip count is width/half with both from a plan the writer did not
        # build, and the fuzzer's was two hundred million -- 6.2 GB of `String`
        # from a 2,937-byte input, in one pass. Fails fast rather than by
        # exhausting memory: the test's fixture asks for 200,049 segments,
        # which is large enough to cross the cap and small enough to finish.
        "marks: step a squiggle at whatever period the quad asks for",
        "src/save/marks.rs",
        "        if seen.width / half > MAX_WAVE_SEGMENTS {",
        "        if false {",
        "a_squiggle_across_an_extreme_quad_emits_a_bounded_number_of_segments",
    ),
    Mutation(
        # Accept a made page of any size that is a number. `make_blank_pages`
        # casts to `f32` for the /MediaBox, so anything past `f32::MAX` is
        # written as `inf` and the file is malformed with nothing saying so.
        "save: accept a made page wider than PDF permits",
        "src/save.rs",
        "                        || size.width > MAX_PAGE_POINTS",
        "                        || false",
        "a_made_page_of_a_size_no_reader_could_have_asked_for_is_refused",
    ),
    Mutation(
        # Trust the app-process guard. `edits.rs` refuses a non-finite page
        # size where the insert command receives it -- in the app process,
        # while this runs in the worker against a plan that may have crossed
        # the boundary or come out of a restored session.
        "save: trust that a made page's size is a finite number",
        "src/save.rs",
        "                    if !size.width.is_finite()",
        "                    if false",
        "a_made_page_of_a_size_no_reader_could_have_asked_for_is_refused",
    ),
    Mutation(
        # Leave the document's own description of itself. The words go from the
        # page and /Info /Title still says what the document is about.
        "save: leave /Info and the XMP packet on a redaction",
        "src/save.rs",
        "        done.metadata = strip_metadata(doc)?;",
        "        done.metadata = 0;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Strip on every rewrite rather than on a redaction. Every copy,
        # extract, split and merge silently loses its title and author, and the
        # two checks above stay green -- this is the SCOPE, and without a
        # control for it the condition is decoration.
        #
        # Since 2026-08-27 this guard covers the OUTLINE too, so the mutation
        # reddens `a_copy_that_is_not_a_redaction_keeps_its_outline` as well.
        # It names one of the two because a mutation names one test, and the
        # other says so in its own doc comment. A second entry with the same
        # anchor and an equivalent replacement would be padding: there is one
        # condition here, not two.
        "save: strip metadata on every save rather than on a redaction",
        "src/save.rs",
        "    if done.shows > 0 || done.annots > 0 || !redactions.is_empty() {",
        "    if true {",
        "a_copy_that_is_not_a_redaction_keeps_its_metadata",
    ),
    Mutation(
        # Take /Info and leave the XMP packet, which holds the same title in the
        # form PDF 2.0 prefers.
        "save: strip /Info and leave the XMP packet beside it",
        "src/save.rs",
        "        doomed.insert(metadata);",
        "        let _ = metadata;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Take the XMP packet and leave /Info, which is the half every reader
        # still shows in its properties panel.
        "save: strip the XMP packet and leave /Info beside it",
        "src/save.rs",
        "        doomed.insert(info);",
        "        let _ = info;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Take no annotation at all. The words go off the page and the comment
        # about them, which quotes them, stays -- displayed by every reader.
        "redact: leave every annotation where it is",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) || true => {}",
        "an_annotation_over_the_region_is_taken",
    ),
    Mutation(
        # Take every annotation on the page. Passes the check above perfectly and
        # destroys every other comment the reader has.
        "redact: take every annotation on a page that has a region",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) && false => {}",
        "an_annotation_away_from_the_region_is_left",
    ),
    Mutation(
        # Treat a shared edge as an overlap here rather than through `overlaps`,
        # which is the rule a second inline comparison would get wrong.
        "redact: test an annotation against the region without the strict rule",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect)\n                if !areas.iter().any(|area| {\n                    !(rect[2] < area[0] || area[2] < rect[0] || rect[3] < area[1] || area[3] < rect[1])\n                }) => {}",
        "an_annotation_flush_against_the_region_is_left",
    ),
    Mutation(
        # Default an unreadable rectangle to somewhere far away instead of taking
        # the annotation. The plausible mistake, and it keeps what it cannot see.
        "redact: keep an annotation whose rectangle cannot be read",
        "src/redact.rs",
        "        match rect_of(doc, annot) {",
        "        match rect_of(doc, annot).or(Some([f32::MAX, f32::MAX, f32::MAX, f32::MAX])) {",
        "an_annotation_with_no_readable_rectangle_is_taken",
    ),
    Mutation(
        # Leave the popup. It is a separate object with its own `/Contents` in
        # most producers' output, so the note goes and the words stay.
        "redact: leave the popup of an annotation that is taken",
        "src/redact.rs",
        "                if let Ok(popup) = annot.get(b\"Popup\").and_then(Object::as_reference) {",
        "                if let Ok(popup) = annot.get(b\"Popup_\").and_then(Object::as_reference) {",
        "a_popup_goes_with_the_annotation_that_owns_it",
    ),
    Mutation(
        # One pass instead of a fixed point. A reply to a reply survives, and a
        # reply is a copy of the conversation about the words that went.
        "redact: collect replies in one pass rather than to a fixed point",
        "src/redact.rs",
        "        if !grew {\n            break;\n        }",
        "        let _ = grew;\n        break;",
        "a_chain_of_replies_goes_with_the_note_it_answers",
    ),
    Mutation(
        # Read `/Annots` only when it is written inline. `/Annots 12 0 R` is as
        # common, and a page written that way keeps every annotation on it.
        "redact: read /Annots only in its inline spelling",
        "src/redact.rs",
        "        .get(b\"Annots\")\n        .and_then(|object| doc.dereference(object).map(|(_, object)| object))\n        .and_then(Object::as_array)",
        "        .get(b\"Annots\")\n        .and_then(Object::as_array)",
        "an_annots_array_that_is_its_own_object_is_read",
    ),
    Mutation(
        # Remove the objects and leave every reference to them. The annotation is
        # gone and the AcroForm still names it, which is a dangling reference
        # where there was a comment -- and on a document where the second name is
        # a structure element, a reader that walks the tree still finds it.
        "save: remove a redacted annotation without dropping the references to it",
        "src/save.rs",
        "            pagetree::forget(doc, &taken).map_err(Refusal::from)?;",
        "            for id in \u0026taken {\n                doc.objects.remove(id);\n            }",
        "a_redacted_annotation_loses_the_references_that_are_not_on_the_page",
    ),
    Mutation(
        # Prune the page's `/Annots` and leave the object. It is then reachable
        # from a structure element's `/OBJR` or an AcroForm's `/Fields`, written
        # out, and still carrying the comment about the words.
        "save: unlink a redacted annotation without removing the object",
        "src/save.rs",
        "            pagetree::forget(doc, &taken).map_err(Refusal::from)?;",
        "            let _ = &taken;",
        "an_annotation_over_a_redacted_region_is_removed_and_its_neighbour_is_not",
    ),
    Mutation(
        # Count the key and leave it where it is. `Removed::carriers` then reports
        # the work as done while every word is still in the stream -- which is the
        # difference between an accounting observable and the thing it accounts
        # for, and the probe's own two assertions separate the same pair.
        "redact: count a shadow-text key without removing it",
        "src/redact.rs",
        "                    if properties.remove(key).is_some() {\n                        cleared.keys += 1;\n                    }",
        "                    if properties.has(key) {\n                        cleared.keys += 1;\n                    }",
        "a_span_the_removal_touched_loses_its_copy_of_the_words",
    ),
    Mutation(
        # Clear the famous key and leave its two siblings, which hold the same
        # words in the same dictionary for the same reason.
        "redact: clear only /ActualText and leave /Alt and /E",
        "src/redact.rs",
        "const SHADOW_TEXT: [&[u8]; 3] = [b\"ActualText\", b\"Alt\", b\"E\"];",
        "const SHADOW_TEXT: [&[u8]; 1] = [b\"ActualText\"];",
        "every_shadow_text_key_goes_not_just_the_famous_one",
    ),
    Mutation(
        # Slip the comparison so no subtree is ever skipped. `text-marked.pdf`'s
        # first kid says `/Limits [5 9]` while its `/Nums` claims key 0, so a
        # walk that stops honouring the limits finds the decoy's answer first --
        # a different, wrong element rather than none, which is what gives this
        # a failing case at all.
        "redact: search every kid of a number tree, ignoring /Limits",
        "src/redact.rs",
        "                if key < low || key > high {",
        "                if key < low && key > high {",
        "a_balanced_parent_tree_is_walked_through_its_kids",
    ),
    Mutation(
        # Read the flat `/Nums` and never descend. The function's own doc comment
        # says this "would find nothing on every document large enough to matter
        # -- silently, since a miss and an untagged page are the same answer",
        # and until the fixture carried a balanced tree nothing could say so.
        "redact: never descend a number tree's /Kids",
        "src/redact.rs",
        "    for kid in kids {",
        "    for kid in Vec::<Object>::new() {",
        "a_balanced_parent_tree_is_walked_through_its_kids",
    ),
    Mutation(
        # Never refuse a shared property list, so a file is written with the
        # words still in it -- or, worse, a later edit strips a dictionary other
        # pages share.
        "redact: pass over a shared property list that carries the words",
        "src/redact.rs",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|key| shared.has(key)) {",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|_key| false) {",
        "a_shared_property_list_carrying_the_words_refuses_and_removes_nothing",
    ),
    Mutation(
        # THE mutation this increment exists for: drop the outline entry
        # without splicing the chain it was in. `pagetree::forget` then takes
        # /Next off the entry before it, so a reader walks /First, /Next and
        # stops one entry early -- every later sibling unreachable, the file
        # valid, no parser complaining.
        "redact: remove an outline entry without joining its neighbours",
        "src/redact.rs",
        "        if let Some(at) = prev {\n            set_or_clear(doc, at, b\"Next\", next);\n        }",
        "        if let Some(at) = prev {\n            let _ = at;\n        }",
        "an_outline_removal_leaves_the_entries_around_it_reachable",
    ),
    Mutation(
        # The other half of the splice. A /Prev naming a deleted object is what
        # a reader walking BACKWARDS from /Last meets, and PDFium's own
        # FPDFBookmark_GetFirstChild/GetNextSibling never look at it -- so this
        # is the half no forward walk can see.
        "redact: leave an outline entry's successor pointing back at what went",
        "src/redact.rs",
        "        if let Some(at) = next {\n            set_or_clear(doc, at, b\"Prev\", prev);\n        }",
        "        if let Some(at) = next {\n            let _ = at;\n        }",
        "an_outline_removal_leaves_the_entries_around_it_reachable",
    ),
    Mutation(
        # Leave /Count saying what the outline used to hold. The /Size shape
        # from spike 0.4 one subsystem along: renders identically, structurally
        # wrong.
        "redact: leave the outline counting entries that are gone",
        "src/redact.rs",
        "            recount(doc, root, 0);",
        "            let _ = root;",
        "a_removal_leaves_the_outline_counting_what_is_left",
    ),
    Mutation(
        # Take every entry rather than the ones naming what went -- which is
        # what a page deletion correctly does and what this must not.
        "redact: take the whole outline rather than the entries that name it",
        "src/redact.rs",
        "        if folded.iter().any(|line| line.contains(&title)) {",
        "        if folded.iter().any(|line| !line.is_empty() || line.contains(&title)) {",
        "an_outline_entry_naming_something_else_survives_a_redaction",
    ),
    Mutation(
        # Match a title against nothing, so the carrier survives. The direction
        # that leaves the words on screen in tpdf's own sidebar.
        "redact: leave every outline entry whatever it names",
        "src/redact.rs",
        "        if folded.iter().any(|line| line.contains(&title)) {",
        "        if folded.iter().any(|line| line.contains(&title) && line.is_empty()) {",
        "a_redaction_takes_the_outline_entry_naming_what_it_removed",
    ),
    Mutation(
        # Take the matched entry and leave what hangs under it. A section's
        # subsections belong to the section, and a child's own title matches
        # nothing -- so only the subtree walk can reach it.
        "redact: take a matched outline entry without its subtree",
        "src/redact.rs",
        "        collect_subtree(doc, id, &mut all);",
        "        if !all.contains(&id) {\n            all.push(id);\n        }",
        "a_redaction_takes_the_outline_entry_naming_what_it_removed",
    ),
    Mutation(
        # Act on a one-character title. A bookmark called `1` is a substring of
        # almost any line, so this takes the outline off a document for the sake
        # of a chapter number.
        "redact: match an outline title too short to be distinctive",
        "src/redact.rs",
        "        if title.len() < MIN_OUTLINE_TITLE {",
        "        if title.is_empty() {",
        "a_very_short_outline_title_is_not_matched",
    ),
    Mutation(
        # Redact an XFA form rather than refusing it. Section 6 has said to
        # refuse since before any of this was written and nothing read it until
        # 2026-08-27: an XFA packet is a complete second copy of every answer,
        # so a redaction that takes the field values and leaves it has removed
        # nothing a reader could not recover.
        "redact: redact an XFA field form rather than refusing it",
        "src/save.rs",
        "    if !redactions.is_empty() && redact::has_xfa(doc) {",
        "    if !redactions.is_empty() && false {",
        "a_redaction_of_an_xfa_form_is_refused_rather_than_half_done",
    ),
    Mutation(
        # Refuse a plain copy of an XFA form too. A serialisation makes no claim
        # about what it removed, so there is nothing for XFA to falsify -- and
        # refusing would make tpdf unable to open-and-save a whole class of
        # document for a promise it is not making.
        "redact: refuse a field form's copy as well as its redaction",
        "src/save.rs",
        "    if !redactions.is_empty() && redact::has_xfa(doc) {",
        "    if redact::has_xfa(doc) {",
        "a_copy_of_an_xfa_form_is_not_refused",
    ),
    Mutation(
        # Leave the field dictionary whose widgets all went. The gap measured
        # before this was built: the kid goes as an annotation and the parent
        # keeps /V, so nothing draws the value and every search finds it.
        "redact: keep a field whose widgets have all been removed",
        "src/redact.rs",
        "        let orphaned = lost.contains(&id) || (field.has(b\"Kids\") && kids.is_empty());",
        "        let orphaned = false && (lost.contains(&id) || (field.has(b\"Kids\") && kids.is_empty()));",
        "a_field_whose_widgets_all_went_does_not_keep_its_value",
    ),
    Mutation(
        # Treat a merged field -- no /Kids at all -- as orphaned. Every field
        # that is its own widget then goes on the first redaction, which is the
        # over-removal direction and empties the form.
        "redact: call a field with no kids at all an orphaned one",
        "src/redact.rs",
        "        let orphaned = lost.contains(&id) || (field.has(b\"Kids\") && kids.is_empty());",
        "        let orphaned = lost.contains(&id) || (!field.has(b\"Kids\") && kids.is_empty());",
        "a_field_naming_nothing_that_went_survives_a_redaction",
    ),
    Mutation(
        # Take a field only when every widget under it went, which is what
        # this rule asked until 2026-09-26: a field with a second widget
        # outside the region kept its /V, drew it through that widget, and the
        # redaction reported itself verified. `split` is the only subject this
        # decides alone -- its value is under the length guard.
        "redact: take a field only when every widget under it went",
        "src/redact.rs",
        "        let orphaned = lost.contains(&id) || (field.has(b\"Kids\") && kids.is_empty());",
        "        let orphaned = field.has(b\"Kids\") && kids.is_empty();",
        "a_field_with_one_widget_covered_and_one_elsewhere_loses_its_value",
    ),
    Mutation(
        # Match fields against the page text alone. A widget's answer is drawn
        # by its appearance stream, never reported as page text, so a second
        # copy of an answer the reader covered survives -- `away` is the only
        # subject the removed widgets' answers decide.
        "save: match form fields against the page text alone",
        "src/save.rs",
        "            answers.extend(redact::widget_answers(doc, &taken));",
        "            let _ = &answers;",
        "a_field_holding_what_went_goes_even_with_its_widget_elsewhere",
    ),
    Mutation(
        # Name no covered form answer as a verification needle: a copy that
        # survived the removal is then invisible to the scan, and the
        # redaction reports itself verified.
        "redact: name no covered form answer as a needle",
        "src/redact.rs",
        "            answers.push(answer.to_string());",
        "            let _ = answer;",
        "a_covered_answer_is_a_needle_and_a_surviving_copy_is_not_verified",
    ),
    Mutation(
        # Let the fill pass write the form answers a second time. A region over
        # a field the reader filled takes the field, so the second write finds
        # no widget and refuses the fill after the words are gone.
        "redaction_fill: write the form answers again in the fill pass",
        "src/redaction_fill.rs",
        "        // answer discarded.\n        forms: Vec::new(),",
        "        // answer discarded.\n        forms: original.forms.clone(),",
        "the_fill_pass_writes_no_form_answer_a_second_time",
    ),
    Mutation(
        # Match a field value against nothing, so a field holding what went
        # survives wherever its widget sits -- which is section 6's "widgets
        # outside the redacted rectangle" left open.
        "redact: leave a field holding what went when its widget is elsewhere",
        "src/redact.rs",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.contains(&text))",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.is_empty())",
        "a_field_holding_what_went_goes_even_with_its_widget_elsewhere",
    ),
    Mutation(
        # Read only /V and not /DV. A default value is the same string in the
        # same dictionary, and a redaction that took the answer and left the
        # default it was pre-filled from has removed nothing.
        "redact: read a field's value and not the default beside it",
        "src/redact.rs",
        "        let carries = [b\"V\".as_slice(), b\"DV\".as_slice()].into_iter().any(|key| {",
        "        let carries = [b\"V\".as_slice()].into_iter().any(|key| {",
        "a_field_whose_default_holds_what_went_is_taken_too",
    ),
    Mutation(
        # Act on a field value too short to be distinctive. A form is full of
        # short answers, and a field holding `Yes` is a substring of almost any
        # line.
        "redact: match a field value too short to be distinctive",
        "src/redact.rs",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.contains(&text))",
        "            folded.iter().any(|line| line.contains(&text))",
        "a_field_value_too_short_to_be_distinctive_is_not_matched",
    ),
    Mutation(
        # Take a matched field and leave its widgets. They are annotations on a
        # page, so the drawing stays where the reader can see it while the value
        # behind it has gone.
        "redact: take a matched field without the widgets under it",
        # Re-aimed 2026-09-06: `collect_field_subtree` is `fields::descend`,
        # which is the same loop `covered_fields` itself runs. The edit is the
        # same one -- take the field and leave everything under it.
        "src/redact.rs",
        """        fields::descend(doc, &[id], &bounds, |node| {
            let Some(at) = node.id else {
                return fields::Flow::Leaf;
            };
            if all.contains(&at) {
                return fields::Flow::Leaf;
            }
            all.push(at);
            fields::Flow::Descend
        });""",
        "        if !all.contains(&id) {\n            all.push(id);\n        }",
        "a_matched_field_takes_the_widgets_under_it",
    ),
    Mutation(
        # Keep an /AcroForm whose fields have all gone. It then reads as a
        # document that never had a form, while /DA, /DR and /NeedAppearances go
        # on describing fields that are not there.
        "redact: keep a form dictionary with no fields left in it",
        "src/redact.rs",
        "    if empty {\n        if let Ok(catalog) = doc.catalog_mut() {",
        "    if false {\n        if let Ok(catalog) = doc.catalog_mut() {",
        "a_form_with_nothing_left_in_it_goes_as_well",
    ),
    Mutation(
        # Refuse every named property list rather than only the ones carrying a
        # key. `/OC /MC0 BDC` is optional content on nearly every layered
        # drawing and carries no text, so this makes those pages unredactable.
        "redact: refuse every named property list, carrier or not",
        "src/redact.rs",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|key| shared.has(key)) {",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|_key| true) {",
        "a_shared_property_list_with_no_shadow_text_does_not_refuse",
    ),
    Mutation(
        # Trust the corners' order. A region arriving with either pair the other
        # way round then overlaps nothing at all, and the redaction quietly
        # removes nothing.
        # Re-aimed 2026-09-06 with the move to `objects.rs`.
        "objects: assume a region's corners arrive in the order PDFium uses",
        "src/objects.rs",
        "        rect[0].min(rect[2]),\n        rect[1].min(rect[3]),",
        "        rect[0],\n        rect[1],",
        "a_region_with_its_corners_the_other_way_round_still_overlaps",
    ),
    Mutation(
        # Normalise the region and not the object. The mirror, and the reason
        # there is one function rather than two copies: the first version of
        # this mutation survived because a test reversing both axes let the
        # other copy rescue it.
        # Re-aimed 2026-09-06 with the move to `objects.rs`.
        "objects: normalise the region a reader drew and not the object it covers",
        "src/objects.rs",
        "    let a = normalised(a);\n    let b = normalised(b);",
        "    let b = normalised(b);",
        "an_object_with_its_corners_the_other_way_round_is_still_found",
    ),
    Mutation(
        # Act on a repeated ordinal twice, so the second pass deletes whatever
        # moved into the slot.
        "redact: act on a repeated ordinal twice",
        "src/redact.rs",
        # Widened for the reason above: `remove_form_shows` deduplicates the same
        # way, and the line after it is what tells the two apart.
        "    positions.dedup();\n    let removed = positions.len();\n\n    // **Before the removal",
        "    positions.reverse();\n    positions.sort_unstable();\n    let removed = positions.len();\n\n    // **Before the removal",
        "the_same_ordinal_twice_removes_one_operator",
    ),
    Mutation(
        # Leave max_id where the graph left it, which is spike 0.4's defect:
        # /Size then claims objects the file does not contain. Nothing in this
        # process reads that -- only qpdf does -- so the assertion has to be on
        # the number rather than on any reader's verdict.
        "save: serialise a document whose /Size claims objects it does not hold",
        "src/save.rs",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);",
        "    doc.max_id = doc.max_id.max(doc.objects.keys().map(|id| id.0).max().unwrap_or(0));",
        "a_serialised_document_reports_the_size_its_objects_justify",
    ),
    Mutation(
        # Lower it one too far. The over-correction direction: /Size then fails
        # to cover the highest object written, which is the same defect
        # mirrored and equally invisible to every reader here.
        "save: declare a /Size that does not cover the highest object written",
        "src/save.rs",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0).saturating_sub(1);",
        "no_object_is_written_at_or_past_the_size_that_was_declared",
    ),
    Mutation(
        # Accept a file that does not begin with a PDF header.
        "verify: let a file with no PDF header pass the structural check",
        "src/verify.rs",
        "    if !bytes.starts_with(b\"%PDF-\") {",
        "    if false {",
        "a_file_that_does_not_begin_with_a_pdf_header_is_refused",
    ),
    Mutation(
        # Accept a file with no %%EOF at all.
        "verify: let a file with no %%EOF pass the structural check",
        "src/verify.rs",
        '            wrong.push(said!("the file has no %%EOF marker"));\n',
        '',
        "a_file_with_no_eof_marker_is_refused",
    ),
    Mutation(
        # Accept several revisions, which PLAN.md section 6 forbids for a
        # rewrite: an object an earlier revision held sits at its old offset,
        # addressable by nothing and invisible to a graph walk.
        "verify: let a rewrite hold more than one revision",
        "src/verify.rs",
        "    match revision_ends(bytes) {\n        0 if",
        "    match revision_ends(bytes).min(1) {\n        0 if",
        "a_second_revision_is_refused_and_the_count_is_reported",
    ),
    Mutation(
        # Ignore bytes past the last %%EOF -- content belonging to no object,
        # which is the other half of section 6's rule.
        "verify: ignore bytes trailing the last %%EOF",
        "src/verify.rs",
        "        if trailing > 0 {",
        "        if false {",
        "bytes_after_the_last_eof_are_refused_and_whitespace_is_not",
    ),
    Mutation(
        # Treat whitespace after %%EOF as trailing content. The over-refusal
        # direction, and the one that would fire on every file every writer
        # produces -- which is how the first draft of a /Size rule was caught.
        "verify: count the newline every writer ends a file with as trailing content",
        "src/verify.rs",
        "        let trailing = bytes[last + 5..]\n            .iter()\n            .filter(|byte| !byte.is_ascii_whitespace())",
        "        let trailing = bytes[last + 5..]\n            .iter()\n            .filter(|_byte| true)",
        "bytes_after_the_last_eof_are_refused_and_whitespace_is_not",
    ),
    Mutation(
        # Accept a startxref pointing past the end of the file.
        "verify: accept a startxref offset past the end of the file",
        "src/verify.rs",
        '            Some(offset) if offset >= bytes.len() => wrong.push(said!(',
        '            Some(offset) if false && offset >= bytes.len() => wrong.push(said!(',
        "a_startxref_pointing_past_the_end_of_the_file_is_refused",
    ),
    Mutation(
        # Accept a startxref with no number after it.
        "verify: accept a startxref with no offset after it",
        "src/verify.rs",
        '            None => wrong.push(said!("the file\'s startxref has no offset after it")),',
        '            None => {}',
        "a_startxref_with_no_number_after_it_is_refused",
    ),
    Mutation(
        # Accept a file with no startxref at all.
        "verify: accept a file with no startxref",
        "src/verify.rs",
        '        None => wrong.push(said!("the file has no startxref")),',
        '        None => {}',
        "a_file_with_no_startxref_is_refused",
    ),
    Mutation(
        # Complain about everything. The control's own control: without this the
        # corpus sweep passes for a check that cannot fail, which is the shape
        # the first /Size rule had in reverse.
        "verify: report every file as malformed",
        "src/verify.rs",
        "pub fn structure(bytes: &[u8]) -> Vec<String> {\n    let mut wrong = Vec::new();",
        "pub fn structure(bytes: &[u8]) -> Vec<String> {\n    let mut wrong = vec![String::from(\"planted\")];",
        "every_rewritten_fixture_is_structurally_sound",
    ),
    # --- attribution: which page a surviving word is on (2026-09-21) ---------
    Mutation(
        # Follow the keys that leave the page. /Parent reaches /Pages and /Kids
        # reaches every other page, so every object becomes reachable from every
        # page and the walk answers "both" for a word on one of them -- a wrong
        # attribution, which is worse than no attribution.
        "verify: follow the dictionary keys that leave the page",
        "src/verify.rs",
        "        if NOT_CONTENT.contains(&key.as_slice()) {\n            continue;\n        }",
        "        if false && NOT_CONTENT.contains(&key.as_slice()) {\n            continue;\n        }",
        "a_needle_on_the_page_it_was_removed_from_is_placed_there",
    ),
    Mutation(
        # Place a word in a file holding more than one revision. An object an
        # earlier revision overwrote sits at its old offset addressable by
        # nothing, so the live graph's answer is not an answer about the file.
        "verify: attribute a page in a file with more than one revision",
        "src/verify.rs",
        "            if !report.found.is_empty() && report.eofs <= 1 {",
        "            if !report.found.is_empty() {",
        "a_file_with_more_than_one_revision_places_nothing",
    ),
    Mutation(
        # Walk on past the per-page object bound. A truncated walk does not lose
        # an answer, it invents a wrong one.
        "verify: keep walking past the per-page object bound",
        "src/verify.rs",
        "            if seen.len() > MAX_REACH_OBJECTS {\n                return None;\n            }",
        "            if seen.len() > MAX_REACH_OBJECTS {}",
        "a_page_reaching_past_the_object_bound_stops_the_walk",
    ),
    Mutation(
        # Follow a reference chain past the depth bound.
        "verify: follow a reference chain past the depth bound",
        "src/verify.rs",
        "                if depth + 1 > MAX_REACH_DEPTH {\n                    return false;\n                }",
        "                if depth + 1 > MAX_REACH_DEPTH {}",
        "a_reference_chain_past_the_depth_bound_stops_the_walk",
    ),
    Mutation(
        # Spend past the total work bound inside one object. Direct values are
        # not references, so the per-page object bound cannot see this at all.
        "verify: spend past the total work bound on direct objects",
        "src/verify.rs",
        "        *steps += 1;\n        if *steps > MAX_REACH_STEPS {\n            return false;\n        }",
        "        *steps += 1;",
        "work_past_the_step_bound_stops_the_walk",
    ),
    Mutation(
        # Call a carrier that two pages draw a page. The first page that reached
        # it is not the page it is on, and a reader comparing that against the
        # page they marked would act on a fabrication.
        "verify: attribute a shared carrier to the pages that share it",
        "src/verify.rs",
        "                    if slots.len() > 1 {\n                        shared.extend(slots.iter().copied());\n                    }",
        "                    if false {\n                        shared.extend(slots.iter().copied());\n                    }",
        "a_needle_in_something_two_pages_draw_is_not_placed_on_either",
    ),
    Mutation(
        # Ignore a carrier no page reaches. The word is then reported as on the
        # pages its other carriers gave, with the unreachable one silent.
        "verify: ignore a carrier no page reaches",
        "src/verify.rs",
        "                None => unplaced = true,",
        "                None => {}",
        "a_needle_in_the_file_s_own_metadata_is_not_placed",
    ),
    Mutation(
        # Say nothing at all about a needle only the byte scan found, instead of
        # saying it cannot be placed. Absent reads as "not asked", which is the
        # sentence a reader gets for a file nobody walked.
        "verify: leave a byte-only hit out of the attribution",
        "src/verify.rs",
        "        let Some(carried) = carriers.get(needle) else {\n            out.insert(needle.clone(), Located::Unplaced);\n            continue;\n        };",
        "        let Some(carried) = carriers.get(needle) else {\n            continue;\n        };",
        "a_needle_no_object_carries_is_not_placed",
    ),
    Mutation(
        # Place a word carried by more objects than the cap. The list stopped
        # being an answer long before it stopped being affordable.
        "verify: place a word with more carriers than the cap",
        "src/verify.rs",
        "        if carried.overflowed || carried.objects.is_empty() {",
        "        if carried.objects.is_empty() {",
        "a_needle_in_more_objects_than_the_cap_is_not_placed",
    ),
    Mutation(
        # Shorten the page list and lose the count with it -- the cap-and-stop
        # shape MAX_OBJECT_REASONS' own test was written against.
        "verify: shorten the page list without counting what it left out",
        "src/verify.rs",
        "            more: pages.len().saturating_sub(MAX_LOCATED_PAGES),",
        "            more: 0,",
        "a_placed_needle_names_a_bounded_number_of_pages_and_counts_the_rest",
    ),
    Mutation(
        # Report the walk as complete when any one finding was placed. The
        # unplaced one could be the word sitting on the marked page.
        "verify: call the walk complete when any finding was placed",
        "src/verify.rs",
        "                .all(|needle| matches!(self.located.get(needle), Some(Located::Pages(_))))",
        "                .any(|needle| matches!(self.located.get(needle), Some(Located::Pages(_))))",
        "a_single_unplaced_finding_silences_the_comparison",
    ),
    Mutation(
        # Ignore the attribution when writing the reasons, which is the sentence
        # a reader actually sees.
        "verify: leave the page out of the reason a reader is shown",
        "src/verify.rs",
        "            why.push(match self.located.get(needle) {",
        "            why.push(match None::<&Located> {",
        "a_needle_on_another_page_reads_as_still_in_the_file",
    ),
    Mutation(
        # Keep the could-not-tell disclaimer beside a report that did tell. Its
        # first clause is then a memory rather than a fact, and `verified` stays
        # false on a blindness that is gone.
        "redact: disclaim an answer the walk gave",
        "src/redact.rs",
        "    if inserted == 0 || found.is_empty() || placed {",
        "    if inserted == 0 || found.is_empty() {",
        "the_inserted_pages_note_is_added_only_when_the_scan_found_something",
    ),
    Mutation(
        # Compare against the marked pages without the walk having placed every
        # finding, which is a claim about a partial set. The test of the empty
        # report named this until 26.10.18: the rule for pieces too short to
        # mean anything answers for an empty report too, so only a report with
        # one finding not placed still shows this line gone.
        "redact: compare against the marked pages without a complete walk",
        "src/redact.rs",
        "    if !report.placed() {\n        return None;\n    }",
        "    if false {\n        return None;\n    }",
        "a_single_unplaced_finding_silences_the_comparison",
    ),
    Mutation(
        # Report a word still on the marked page as though it were somewhere
        # else -- the one direction of this sentence that reads as success.
        "redact: read a hit on a marked page as a hit somewhere else",
        "src/redact.rs",
        "    if !hit.is_empty() {",
        "    if false {",
        "a_word_still_on_a_marked_page_is_named_as_a_removal_that_did_not_take",
    ),
    Mutation(
        # Never collect, which is what the save path did until 2026-08-26.
        # Extracting one page of eight then produces a one-page file carrying all
        # eight content streams --- `docs/THREAT-MODEL.md` residual risk 16, which
        # named the deletion and missed the extract.
        "save: leave a dropped page's content in the file",
        "src/save.rs",
        "    if !dropped.is_empty()\n        || moved\n        || discarded > 0",
        "    if false\n        \u0026\u0026 moved\n        || discarded > 0",
        "extracting_a_page_leaves_the_other_pages_out_of_the_file",
    ),
    Mutation(
        # Collect on every save, including one that dropped nothing. The two
        # leak checks stay green -- this is the *scope* that has to be able to
        # fail, and without a control for it the condition above is decoration.
        "save: sanitize a plain copy as well as a save that removed something",
        "src/save.rs",
        "    if !dropped.is_empty()\n        || moved\n        || discarded > 0",
        "    if dropped.is_empty()\n        || !moved\n        || true\n        || discarded > 0",
        "a_copy_that_drops_nothing_keeps_the_orphans_it_was_given",
    ),
    Mutation(
        # **Re-aimed on 2026-08-28, when the sequence moved to
        # `pagetree::materialise`.** The rule it used to mutate --- drop the
        # outline when pages go --- now lives in one place and has its own
        # mutation there, aimed at `pagetree`'s own test. Two mutations on one
        # line would make the anchor ambiguous, and an ambiguous anchor is
        # refused, so this one is aimed at what is still *this* caller's: whether
        # `save` tells the shared function anything was dropped at all. The rule
        # is covered once; each caller's wiring to it is covered where it is.
        "save: tell the page-tree writer that nothing was dropped",
        "src/save.rs",
        "    pagetree::materialise(&mut doc, &dropped, moved.then_some(order.as_slice()))?;",
        "    pagetree::materialise(&mut doc, &[], moved.then_some(order.as_slice()))?;",
        "deleting_a_page_keeps_the_bookmarks_of_the_pages_that_stay",
    ),
    Mutation(
        # Delete the page in the slot rather than the one that was named.
        # Identical on an unedited document and wrong the moment one moves.
        "edits: delete the page in the slot rather than the page that was named",
        "src/edits.rs",
        "            Command::Delete {\n                page: PageId::from_raw(page),",
        "            Command::Delete {\n                page: PageId::from_raw(page + 1),",
        "a_deleted_page_leaves_the_order_and_the_ones_after_it_move_up",
    ),
    Mutation(
        # Call every plan the file on disk. A print job then hands over the
        # original bytes, and the reader's deletions and turns are not on paper.
        "edits: report an edited document as the file on disk",
        "src/edits.rs",
        "                theirs && turns % 4 == 0 && crop.is_none()",
        "                (theirs || turns % 4 == 0) && crop.is_none()",
        "only_an_unedited_document_is_the_file_on_disk",
    ),
    Mutation(
        # Print the file rather than the working document. The pages the reader
        # deleted come out of the printer.
        "print: print the file even when the model says it has been edited",
        "src/print.rs",
        "    if plan.is_identity() {",
        "    if true {",
        "an_edited_document_prints_the_pages_the_model_kept",
    ),
    Mutation(
        # Apply the view rotation and drop each page's own edit, which is what
        # the print path did until a page could be turned in the document.
        "print: apply the view rotation and not the page's own edit",
        "src/print.rs",
        "            Some((id, ((i16::from(page.turns) + view).rem_euclid(4)) as u8))",
        "            Some((id, (view.rem_euclid(4)) as u8))",
        "each_page_takes_its_own_edit_and_the_view_rotation_on_top",
    ),
    Mutation(
        # Let a print job rewrite an encrypted document. This is the regression
        # the encryption increment introduced and did not notice: `checked` used
        # to refuse every decrypted document, `print::route`'s `Working` arm
        # calls `print_bytes` directly and never reaches `print::build`'s own
        # guard, so removing that refusal removed the only one on this route.
        # The whole suite stayed green.
        # It moved into `rewrite_update` on 2026-09-01 with the parse it guards.
        "save: let a print job rewrite an encrypted document",
        "src/save.rs",
        "    if job.is_print() && checked.encryption.is_some() {",
        "    if false {",
        "a_print_job_from_an_encrypted_document_is_refused_whatever_the_rewrite_can_do",
    ),
    Mutation(
        # Refuse an encrypted *save* as well, which is the other operand of the
        # conjunction above and the half a single mutation cannot reach. A save
        # of an encrypted document keeps its encryption and must not be refused;
        # the print refusal reads as if it were about encryption alone.
        "save: refuse an encrypted save the way a print job is refused",
        "src/save.rs",
        "    if job.is_print() && checked.encryption.is_some() {",
        "    if checked.encryption.is_some() {",
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # Write a document `lopdf` decrypted on the way in back in the clear.
        # **Re-aimed rather than deleted on 2026-08-28**: the guard this used to
        # mutate -- a refusal keyed on `was_encrypted` -- is gone, because a
        # rewrite now re-encrypts instead of refusing. The class of defect did
        # not go with it. Dropping a document's encryption on save is still one
        # line away, and this is that line.
        #
        # Aimed at the fixture test rather than the synthetic one on purpose:
        # the synthetic document's encryption is fake, authentication fails on
        # it, and it therefore takes the `is_encrypted` arm. Two arms, two
        # mutations, because one fixture cannot reach both.
        "save: write a decrypted document back in the clear",
        "src/save.rs",
        "    if let Some(state) = &encryption {\n        doc.encrypt(state)",
        "    if let Some(state) = &encryption.filter(|_| false) {\n        doc.encrypt(state)",
        "a_really_encrypted_document_keeps_its_encryption_or_names_its_lock",
    ),
    Mutation(
        # Never take the state off the document, so `rewrite` has nothing to put
        # back. The mirror of the mutation above, at the other end of the value's
        # journey: one deletes the write, this one deletes the read, and a fix
        # for either alone leaves the other's defect intact.
        "save: keep no record of the encryption a password opened",
        "src/save.rs",
        "    let encryption = doc.encryption_state.take();",
        "    let encryption = None;\n    let _ = doc.encryption_state.take();",
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # Load without the reader's password. Every object then fails to parse,
        # and the document that comes back is EMPTY rather than refused -- which
        # is the failure shape that made the first two runs of this increment's
        # own spike report three passes over a document with nothing in it.
        "save: ignore the password the reader supplied",
        "src/save.rs",
        '            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| said!("could not parse the document: {e}"))?;',
        '            password: None,\n            ..Default::default()\n        },\n    )\n    .map_err(|e| said!("could not parse the document: {e}"))?;',
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # The other arm: a document nothing could unlock. `lopdf` parses no
        # objects for it, so a rewrite would write out an empty document.
        "save: rewrite a document nothing unlocked",
        "src/save.rs",
        "    if doc.is_encrypted() {\n        return Err(said!(\n            \"This document is encrypted and tpdf could not unlock it, so it cannot be \\\n             rewritten. Open it with its password first.\"",
        "    if false {\n        return Err(said!(\n            \"This document is encrypted and tpdf could not unlock it, so it cannot be \\\n             rewritten. Open it with its password first.\"",
        "an_encrypted_document_is_refused_rather_than_quietly_decrypted",
    ),
    Mutation(
        # Append to a document nothing unlocked. `lopdf` refuses this itself in
        # `check_incremental_save_supported`, so what this proves is that the
        # refusal a reader sees is ours and names a password rather than an
        # upstream issue number.
        "save: append to a document nothing unlocked",
        "src/save.rs",
        "    if prev.is_encrypted() {",
        "    if false {",
        "an_append_to_a_document_nobody_unlocked_is_refused",
    ),
    Mutation(
        # Build the update section without the reader's key. `lopdf` then parses
        # no objects, and the page walk below sees an empty document -- so the
        # refusal is about the baseline rather than about the password.
        "save: build an append without the password",
        "src/save.rs",
        '            max_decompressed_size: Some(MAX_DECODE),\n            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| said!("this document could not be parsed: {e}"))?;',
        '            max_decompressed_size: Some(MAX_DECODE),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| said!("this document could not be parsed: {e}"))?;',
        "an_encrypted_document_can_be_appended_to_and_stays_encrypted",
    ),
    Mutation(
        # Accept a plan that does not describe the file on disk. The turns then
        # land on whichever pages happen to be in those positions.
        "save: accept a plan of the wrong length",
        "src/save.rs",
        "    let pages = ordered_pages(&doc);\n    if pages.len() != plan.baseline as usize {",
        "    let pages = ordered_pages(&doc);\n    if false {",
        "a_plan_that_does_not_match_the_file_on_disk_is_refused",
    ),
    Mutation(
        # Overwrite the open document. The journal then replays against a
        # baseline that no longer exists.
        "save: allow writing over the source",
        "src/save.rs",
        '    if same_file(source, out) {\n        return Err(said!(\n'
        '            "tpdf cannot save over the document it is reading --- choose another name"\n',
        '    if false {\n        return Err(said!(\n'
        '            "tpdf cannot save over the document it is reading --- choose another name"\n',
        "saving_over_the_open_document_is_refused",
    ),
    Mutation(
        # Put the save in place during the staging, which is the shape the write
        # had before it was split. The document is replaced while a worker still
        # has it mapped -- which succeeds on macOS, and leaves that worker serving
        # the file that used to be there.
        #
        # Written as a copy-and-remove rather than as an edit to the `stage` call
        # because the staging is a closure now: this reproduces both halves of the
        # old defect -- the source changes, and the path handed back is gone.
        # An earlier attempt handed the source back as the staged path and was
        # NOT caught: `resolved()` canonicalizes, so `/private/var/...` and
        # `/var/...` are unequal `PathBuf`s and the test's `assert_ne!` passed.
        "save: put a save in place during the staging rather than after the close",
        "src/save.rs",
        "    Ok(Staged { path, verified })",
        "    std::fs::copy(&path, &target).map_err(|e| e.to_string())?;\n    let _ = std::fs::remove_file(&path);\n    Ok(Staged { path, verified })",
        "staging_a_save_in_place_writes_beside_the_source_and_leaves_it_alone",
    ),
    Mutation(
        # Report the commit as done without renaming anything. The reader is told
        # their document was saved, the file on disk is the one they opened, and
        # the staged copy of their work sits beside it under a name nothing reads.
        "save: report a commit that never renamed anything",
        "src/save.rs",
        "    commit(staged, &resolved(source))\n}",
        "    let _ = (staged, source);\n    Ok(())\n}",
        "committing_a_staged_save_puts_the_edits_in_the_file_the_reader_opened",
    ),
    Mutation(
        # Stage before the guards run, which is the ordering the `reopen: false`
        # half of `SaveFailure` rests on. Every refusal then leaves a partial
        # file next to the reader's document under a name they never chose.
        "save: stage a save in place before its guards have run",
        "src/save.rs",
        "    if plan.opened_as.is_none() {",
        "    let early = stage(source, |_: &mut std::fs::File| Ok(()))?;\n    let _ = early;\n    if plan.opened_as.is_none() {",
        "a_refused_save_in_place_stages_nothing",
    ),
    Mutation(
        # Compare the paths as strings. Two spellings of one file then read as
        # two files, and the guard above passes while the file is overwritten.
        "save: two spellings of one path are two files",
        "src/save.rs",
        "    canonical_parent(a) == canonical_parent(b) && a.file_name() == b.file_name()",
        "    a == b",
        "a_hard_link_to_the_source_is_the_same_file_and_a_copy_is_not",
    ),
    Mutation(
        # Copy the staged file into place instead of renaming it. An interrupted
        # save then leaves a truncated PDF where the reader's file was.
        #
        # **Re-aimed 2026-09-01, and it had stopped being able to fail before
        # that.** It was aimed at `write_atomically`, which had three callers --
        # the copy, the split and the merge -- until the copy and the split moved
        # to the seam on 2026-09-01 and the merge followed. The test named below
        # goes through the COPY, so from the moment that path stopped calling
        # `write_atomically` the mutation could redden nothing, while the anchors
        # gate went on reporting the anchor present exactly once. That gate reads
        # the file; only a run reads the coverage. `docs/TRAPS.md` records the
        # shape under *A refactor orphans mutations nobody can see*.
        #
        # `commit` is the mechanism now, and it is stronger: four writing paths
        # end there rather than one.
        "save: write straight to the destination rather than renaming into it",
        "src/save.rs",
        "        match std::fs::rename(staged, out) {",
        "        match std::fs::copy(staged, out).map(|_| ()) {",
        "the_destination_is_replaced_whole_rather_than_written_through",
    ),
    Mutation(
        # A command that names a page by its position rather than its identity.
        # Identical on an unedited document, and wrong the moment a page moves.
        "edits: rotate the page in the slot rather than the page that was named",
        "src/edits.rs",
        "            Command::Rotate {\n                page: PageId::from_raw(page),",
        "            Command::Rotate {\n                page: PageId::from_raw(page + 1),",
        "a_turn_lands_on_the_page_it_named_and_nowhere_else",
    ),
    Mutation(
        # Report a document as unedited whenever it looks like the file on disk.
        # A rotate-and-rotate-back then reads as clean, and the reader is not
        # told there is an unsaved journal.
        "edits: read dirty off the working document instead of the journal",
        "src/edits.rs",
        "    let (applied, _) = model.depth();",
        "    let (applied, _) = (0usize, 0usize);",
        "a_turn_lands_on_the_page_it_named_and_nowhere_else",
    ),
    Mutation(
        # Keep the previous document's journal under a reused handle. Neither
        # backend gives a handle out twice today; the replacement is held
        # because the other outcome is one document's edits applied to another.
        "edits: keep the model already under a reopened handle",
        "src/edits.rs",
        "        self.docs.lock().expect(\"edits lock\").insert(\n            doc,\n            Open {",
        "        self.docs.lock().expect(\"edits lock\").entry(doc).or_insert(\n            Open {",
        "reopening_under_a_reused_handle_does_not_inherit_the_previous_journal",
    ),
    Mutation(
        # Collapse the two refusals. An id that never existed and an id that was
        # deleted are different diagnoses, and the tombstone exists to keep them
        # apart.
        "edits: report every unknown page as deleted",
        "src/edits.rs",
        '        Refusal::NoSuchPage(_) => said!("no such page"),',
        '        Refusal::NoSuchPage(_) => said!("that page has been deleted"),',
        "an_id_no_document_ever_had_is_refused_by_name",
    ),
    Mutation(
        # Back to lowercasing, which is what stood here until 2026-08-01 and is the
        # whole subject of the change: `ß` is already lowercase, so it survives the
        # fold and `strasse` cannot find `Straße`.
        "fold: lowercase instead of case-folding",
        "src/search.rs",
        "            for folded in std::iter::once(ch).default_case_fold() {",
        "            for folded in ch.to_lowercase() {",
        "a_sharp_s_folds_to_two_letters",
    ),
    # There is deliberately no second mutation for the Greek half. Written and run:
    # the one above already turns `both_greek_sigmas_fold_together` red, because
    # lowercasing gets Greek wrong in its own way --- it maps `Σ` to `σ` and leaves
    # `ς` alone, so one word's two spellings land in different buckets. A second
    # mutation that pre-lowercased the input before the loop broke six *other* tests
    # and not that one, because it also defeated `match_case`.
    Mutation(
        # A pattern compiled case-sensitively against a haystack the fold has
        # already lowercased: any uppercase letter in a pattern then matches
        # nothing. It shipped that way, and the doc comment above `compile`
        # asserted the invariant it was breaking.
        "regex: compile a pattern case-sensitively whatever the option says",
        "src/search.rs",
        "        .case_insensitive(!match_case)",
        "        .case_insensitive(false)",
        "an_upper_case_pattern_matches_lower_case_text",
    ),
    Mutation(
        # The other direction: ignore case even when asked not to. Only a test
        # that turns the option *on* can see it.
        "regex: always ignore case, whatever the option says",
        "src/search.rs",
        "        .case_insensitive(!match_case)",
        "        .case_insensitive(true)",
        "an_upper_case_pattern_still_distinguishes_case_when_asked",
    ),
    Mutation(
        # No corpus reaches this: it fires only on a page that is both tagged and
        # carries a character above the BMP, and neither fixture is both. A
        # mutation switching the whole translation off passed `search-probe` and
        # `structure-probe` alike, which is why the arithmetic was split out and is
        # judged here instead.
        "text: leave the tagged runs in PDFium's index space",
        "src/text.rs",
        "    if ours.len() == len + 1 {\n        // No pair anywhere, so the two spaces are the same one.\n        return;\n    }",
        "    return;\n    #[allow(unreachable_code)]",
        "a_run_after_a_pair_moves_back_by_the_units_it_saved",
    ),
    Mutation(
        # The opposite: translate even when there is nothing to translate. It is
        # the identity, so only a fixture with no pair can tell.
        "text: round a run's end outwards to include a half-covered pair",
        "src/text.rs",
        "    let at = |index: u32| ours.get(index as usize).copied().unwrap_or(len as u32);",
        "    let at = |index: u32| ours.get(index as usize + 1).copied().unwrap_or(len as u32);",
        "a_run_ending_inside_a_pair_comes_back_empty",
    ),
    Mutation(
        # The defect the multilingual corpus was built to look for, and it was
        # there: `FPDFText_GetUnicode` is a UTF-16 API, so an astral code point
        # arrives as two lone surrogates. `char::from_u32` refuses both, the fold
        # drops them, and a CJK Extension B ideograph is unfindable while being
        # perfectly visible on the page.
        "text: report a surrogate pair as two characters",
        "src/text.rs",
        "    match next {\n        Some(low) if (0xDC00..0xE000).contains(&low) => {\n            (0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00), 2)\n        }\n        _ => (REPLACEMENT, 1),\n    }",
        "    let _ = next;\n    (REPLACEMENT, 1)",
        "a_surrogate_pair_becomes_one_scalar_over_two_units",
    ),
    Mutation(
        # The other direction: pair anything that follows a high surrogate. This
        # consumes a real character, so the page comes back one short and every
        # box after it shifts.
        "text: pair a high surrogate with whatever follows it",
        "src/text.rs",
        "        Some(low) if (0xDC00..0xE000).contains(&low) => {",
        "        Some(low) => {",
        "a_high_surrogate_followed_by_anything_else_is_replaced",
    ),
    Mutation(
        # A lone surrogate dropped rather than replaced. It looks tidier and it
        # shortens the page silently, which is the one thing an index space may
        # not do.
        "text: treat a lone low surrogate as two units wide",
        "src/text.rs",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (REPLACEMENT, 1);\n    }",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (REPLACEMENT, 2);\n    }",
        "a_lone_low_surrogate_is_replaced_and_never_paired_backwards",
    ),
    Mutation(
        "fold: keep every whitespace character instead of collapsing runs",
        "src/search.rs",
        "                if chars.last() == Some(&' ') {\n                    continue;\n                }",
        "",
        "a_run_of_spaces_matches_one_space",
    ),
    Mutation(
        "fold: treat a soft hyphen as a character",
        "src/search.rs",
        "            if ch == SOFT_HYPHEN {\n                continue;\n            }",
        "",
        "a_soft_hyphen_is_not_a_character",
    ),
    Mutation(
        # Written first as `to_ascii_lowercase().to_lowercase()`, which SURVIVED
        # -- and it should have: `to_ascii_lowercase` is the identity on every
        # non-ASCII character and agrees with `to_lowercase` on the rest, so the
        # composition is exactly `to_lowercase` and the edit changed nothing. A
        # mutation that changes nothing looks precisely like a test that cannot
        # fail, and the entry of that name in docs/TRAPS.md is about the minute
        # spent strengthening a test that was already fine.
        "fold: fold ASCII only, dropping the Unicode mapping",
        "src/search.rs",
        "            for folded in std::iter::once(ch).default_case_fold() {",
        "            for folded in [ch.to_ascii_lowercase()] {",
        "a_multi_character_lowercase_still_maps_back",
    ),
    Mutation(
        "fold: ignore the option and always lower-case",
        "src/search.rs",
        "            if match_case {\n                chars.push(ch);\n                source.push(index);\n                continue;\n            }",
        "",
        "matching_case_distinguishes_what_ignoring_it_conflated",
    ),
    Mutation(
        # The other direction of the same switch. Together they say the flag is
        # read, rather than that one of its two values happens to work.
        "fold: ignore the option and never lower-case",
        "src/search.rs",
        "            if match_case {",
        "            if true {",
        "case_is_ignored_in_both_directions",
    ),
    Mutation(
        "match: end the span by arithmetic instead of through the source map",
        "src/search.rs",
        "        let stop = hay.source[end - 1] as usize + 1;",
        "        let stop = start + needle.chars.len();",
        "a_multi_character_lowercase_still_maps_back",
    ),
    Mutation(
        "match: let occurrences overlap",
        "src/search.rs",
        "            // `aa` occurs once in `aaa`, not twice.\n            at += needle.chars.len();",
        "            // `aa` occurs once in `aaa`, not twice.\n            at += 1;",
        "matches_do_not_overlap",
    ),
    Mutation(
        # Re-aimed when the guard moved into `Prepared::new`, which is where a
        # query is now read once for a whole walk rather than once per page. The
        # edit is the same one: stop calling a whitespace-only query barren, so
        # the walk runs it and answers with every gap in the document.
        "match: run a query of only whitespace",
        "src/search.rs",
        "            needle.chars.iter().all(|ch| *ch == ' ')",
        "            false",
        "an_empty_query_matches_nothing",
    ),
    Mutation(
        "whole word: check the left boundary only",
        "src/search.rs",
        "            || (boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))\n                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))",
        "            || boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))",
        "a_whole_word_search_bounds_both_ends_independently",
    ),
    Mutation(
        "whole word: check the right boundary only",
        "src/search.rs",
        "            || (boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))\n                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))",
        "            || boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied())",
        "a_whole_word_search_bounds_both_ends_independently",
    ),
    Mutation(
        "whole word: treat the end of the page as not a boundary",
        "src/search.rs",
        "        _ => true,",
        "        _ => false,",
        "a_word_may_end_at_the_page_rather_than_at_a_boundary",
    ),
    Mutation(
        "whole word: require both neighbours to be non-word, not a boundary",
        "src/search.rs",
        "        (Some(left), Some(right)) => !(is_word(left) && is_word(right)),",
        "        (Some(left), Some(right)) => !is_word(left) && !is_word(right),",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "whole word: count punctuation as part of a word",
        "src/search.rs",
        "    ch.is_alphanumeric() || ch == '_'",
        "    !ch.is_whitespace()",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "whole word: skip the whole span after rejecting a candidate",
        "src/search.rs",
        "                // caught the restructure that introduced the regex path.\n                at += 1;",
        "                // caught the restructure that introduced the regex path.\n                at += needle.chars.len();",
        "a_rejected_candidate_does_not_hide_the_one_overlapping_it",
    ),
    Mutation(
        # Predicted against `a_match_is_found_where_it_is` first, and that was
        # simply wrong: its needle is a word with spaces on both sides, so it
        # matches identically whether or not the boundary test runs. What
        # notices is the *count* on the mixed fixture, where two of the four
        # occurrences are inside longer words -- so the discriminating assertion
        # is the one that pins what the plain search finds, not the one that
        # finds anything at all.
        "whole word: apply the boundary test whether or not it was asked for",
        "src/search.rs",
        "        !options.whole_word",
        "        false",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "context: take the words after the hit from before it",
        "src/search.rs",
        "            before: slice_of(codes, start.saturating_sub(CONTEXT_CHARS)..start),",
        "            before: slice_of(codes, start..(start + CONTEXT_CHARS).min(codes.len())),",
        "a_hit_carries_the_words_on_either_side_of_it",
    ),
    Mutation(
        "context: run off the end of the page instead of clamping",
        "src/search.rs",
        "            after: slice_of(codes, stop..(stop + CONTEXT_CHARS).min(codes.len())),",
        "            after: slice_of(codes, stop..(stop + CONTEXT_CHARS)),",
        "context_stops_at_the_ends_of_the_page",
    ),
    Mutation(
        "context: take everything before the hit, not a bounded window",
        "src/search.rs",
        "start.saturating_sub(CONTEXT_CHARS)..start",
        "0..start",
        "context_is_bounded_and_the_hit_is_not",
    ),
    Mutation(
        # The replacement is the *folded* needle rather than the query as typed,
        # because the walk no longer has the raw query in scope --- it holds a
        # `Prepared`. It is the same mutation: show what was searched for rather
        # than what the page says.
        "context: show the query instead of what the page says",
        "src/search.rs",
        "            hit: exact_of(codes, start..stop),",
        "            hit: needle.chars.iter().collect::<String>(),",
        "the_hit_is_the_page_text_and_not_the_query",
    ),
    Mutation(
        "context: collapse the whitespace inside the hit as well",
        "src/search.rs",
        "            hit: exact_of(codes, start..stop),",
        "            hit: slice_of(codes, start..stop),",
        "context_collapses_line_breaks_but_the_hit_keeps_them",
    ),
    Mutation(
        "context: leave the line breaks in the words around the hit",
        "src/search.rs",
        "        if ch.is_whitespace() {\n            if !out.ends_with(' ') {\n                out.push(' ');\n            }\n            continue;\n        }",
        "",
        "context_collapses_line_breaks_but_the_hit_keeps_them",
    ),
    Mutation(
        # Same edit, aimed at `Prepared::search_page`, where the answer is now
        # built. The needle's length stands in for the query's, which is what
        # this module has left of it by then.
        "page: report the query's length rather than the page's",
        "src/search.rs",
        "            chars: codes.len() as u32,\n            problem: None,",
        "            chars: self.needle.chars.len() as u32,\n            problem: None,",
        "a_page_with_no_text_reports_it_rather_than_no_matches",
    ),
    Mutation(
        # A run's answers are indexed positionally by the caller, which continues
        # from the end of what came back. Out of order, it re-asks about pages it
        # already has and skips the ones it does not.
        "range: answer a run out of walk order",
        "src/render.rs",
        "        answers.push(answer);",
        "        answers.insert(0, answer);",
        "a_run_answers_the_pages_it_was_asked_about",
    ),
    Mutation(
        # Carry the tail between any two pages of a run rather than between
        # neighbours only. An unscoped scan wraps, so the page after the last is
        # the first, and this reports a phrase spanning the end of the document.
        "range: carry a tail across a gap in the pages",
        "src/render.rs",
        "        let adjacent = step > 0 && pages[step - 1] + 1 == page;",
        "        let adjacent = step > 0;",
        "a_run_does_not_carry_across_pages_that_are_not_neighbours",
    ),
    Mutation(
        # Answer every page asked about, whatever the reply weighs. A reply is
        # read under `MAX_REPLY_BYTES` and one that exceeds it leaves the stream
        # mid-line, which kills the worker -- so a document with a hit on every
        # line turns a search into a lost pool.
        "range: answer every page whatever the reply weighs",
        "src/render.rs",
        "        if spent >= budget {\n            break;\n        }",
        "",
        "a_run_stops_at_the_budget_rather_than_answering_every_page",
    ),
    Mutation(
        # Lose a run because a later page will not extract. One damaged page in
        # sixteen would take the hits on the fifteen before it with it.
        "range: lose the run when a later page will not extract",
        "src/render.rs",
        "            Err(_) => break,",
        "            Err(e) => return Err(e),",
        "a_damaged_page_ends_a_run_rather_than_losing_it",
    ),
    Mutation(
        # Answer a run that produced nothing with a page that has no hits, which
        # is the reassuring branch: a document that could not be read at all
        # would report itself as containing no matches.
        "range: answer an empty run with a page that has no hits",
        "src/render.rs",
        '        return Err("the search answered no pages at all".into());',
        "        return Ok(PageMatches { page: 0, matches: Vec::new(), chars: 0, problem: None, tail: None, more: Vec::new() });",
        "packing_no_pages_at_all_is_refused",
    ),
    Mutation(
        # Evict rather than decline. A scan touches every page once in the same
        # order, so eviction empties the cache of exactly the pages the next scan
        # asks for first -- a hit rate of zero rather than a partial one.
        "cache: evict what fits to make room for what does not",
        "src/textcache.rs",
        "        if self.chars + held.len() <= budget {\n            self.chars += held.len();\n            self.entries.insert(key, Arc::clone(&held));\n        }",
        "        while self.chars + held.len() > budget {\n            let Some(victim) = self.entries.keys().next().copied() else { break };\n            if let Some(gone) = self.entries.remove(&victim) { self.chars -= gone.len(); }\n        }\n        self.chars += held.len();\n        self.entries.insert(key, Arc::clone(&held));",
        "what_does_not_fit_is_declined_rather_than_evicting_what_does",
    ),
    Mutation(
        # Key on the page alone. The two extractions carry the same codes today,
        # so a caller that starts asking for cropped text would be served the
        # uncropped page's answer with nothing anywhere disagreeing.
        "cache: key on the page and ignore the crop",
        "src/textcache.rs",
        "    (page, crop.map(|c| c.map(f32::to_bits)))",
        "    let _ = crop;\n    (page, None)",
        "a_crop_is_part_of_the_key",
    ),
    Mutation(
        # Count the *asks* rather than the parses, by moving the counter outside
        # the cell that memoises it. Five parses and one give identical answers
        # and differ by 5.8 ms each on the 775-page document, so the count is the
        # only thing that can see the difference -- and it only can while it is
        # inside the closure that does the work.
        #
        # An earlier version of this mutation replaced the cell with a fresh
        # `OnceCell` per call, which is what "parse again for every question"
        # would literally be. It does not compile: the reference would outlive
        # the temporary. Recorded because the shape recurs -- a memo returning a
        # borrow cannot be un-memoised by an expression.
        "graph: count the questions rather than the parses",
        "src/docgraph.rs",
        "        self.parsed\n            .get_or_init(|| {",
        "        self.parses.set(self.parses.get() + 1);\n        self.parsed\n            .get_or_init(|| {",
        "one_parse_answers_every_question",
    ),
    Mutation(
        # Start the fingerprint at open, as it was. Hashing reads the file end to
        # end -- 452 ms cold on the 337 MB scan fixture -- and this puts that read
        # back on the wire while the first page is being drawn.
        "hash: start the fingerprint at open rather than at the first edit",
        "src/edits.rs",
        "                sources: HashMap::new(),\n                pending: None,\n            },\n        );\n    }",
        "                sources: HashMap::new(),\n                pending: None,\n            },\n        );\n        self.wake(doc);\n    }",
        "opening_a_document_does_not_start_the_hash",
    ),
    Mutation(
        # Leave the edit path to somebody else. `edits.rs` has no chokepoint, so
        # a trigger missing from one mutator is a document whose hash does not
        # start until the save that waits for it.
        "hash: do not start the fingerprint on an edit",
        "src/edits.rs",
        '        self.wake(doc);\n        let mut docs = self.docs.lock().expect("edits lock");\n        let open = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;\n        let model = &mut open.model;\n        model.apply_in(cmd, sweep).map_err(describe)?;',
        '        let mut docs = self.docs.lock().expect("edits lock");\n        let open = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;\n        let model = &mut open.model;\n        model.apply_in(cmd, sweep).map_err(describe)?;',
        "every_edit_starts_the_hash",
    ),
    Mutation(
        # One of the twelve that lock for themselves, so it has a wake of its
        # own and no chokepoint to inherit one from. Aimed here rather than only
        # at `command_in` because the two halves fail differently: this is the
        # half a test written against a delegating method cannot see.
        "hash: do not start the fingerprint on an annotation",
        "src/edits.rs",
        "        sweep: u64,\n    ) -> Result<EditState, String> {\n        // The reader is here: start the fingerprint if nothing has. See `wake`.\n        self.wake(doc);\n        if want.quads.len() % 4 != 0 {",
        "        sweep: u64,\n    ) -> Result<EditState, String> {\n        if want.quads.len() % 4 != 0 {",
        "every_edit_starts_the_hash",
    ),
    Mutation(
        # The other direction: wake on a read of the state as well. The frontend
        # asks for it the instant a document opens, so this is the deferral
        # undone while every test about the fingerprint still passes.
        "hash: start the fingerprint when the state is read",
        "src/edits.rs",
        "    pub fn state(&self, doc: u32) -> Result<EditState, String> {",
        "    pub fn state(&self, doc: u32) -> Result<EditState, String> {\n        self.wake(doc);",
        "reading_the_state_does_not_start_the_hash",
    ),
    Mutation(
        # The invariant the wire carries: runs present means runs complete. A
        # truncated walk is a reading order missing an unknown part of the page,
        # and a consumer cannot tell which part.
        "structure: offer a truncated walk's runs anyway",
        "src/structure.rs",
        "        if self.truncated {\n            return Vec::new();\n        }",
        "",
        "a_truncated_walk_offers_nothing",
    ),
    Mutation(
        # The one that matters, and the one no fixture can catch. `/Encoding`
        # decides code -> CID and says nothing about CID -> Unicode; the
        # descendant's `/Ordering` is what supplies it. Both fields covary on
        # every page of `encodings.pdf` --- Identity-H with Identity, UniJIS with
        # Japan1 --- so this rule passes the corpus completely. Only the two
        # synthetic diagonals in `encoding.rs` reach it, and they are why the
        # module carries unit tests at all.
        "encoding: key on the font's /Encoding name instead of the ordering",
        "src/encoding.rs",
        "    let info = descendant.get(b\"CIDSystemInfo\").ok()?;\n"
        "    let info = resolve_dict(document, info).ok()?;",
        "    let info = descendant.get(b\"CIDSystemInfo\").ok()?;\n"
        "    let info = resolve_dict(document, info).ok()?;\n"
        "    let _ = info;\n"
        "    return Some(\n"
        "        font.get(b\"Encoding\")\n"
        "            .ok()\n"
        "            .and_then(|object| object.as_name().ok())\n"
        "            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())\n"
        "            .unwrap_or_default()\n"
        "            .starts_with(\"Identity\"),\n"
        "    );\n"
        "    #[allow(unreachable_code)]",
        "identity_encoding_over_a_known_ordering_is_not_a_guess",
    ),
    Mutation(
        # A `/ToUnicode` states the mapping whatever else the font says, so
        # ignoring it reports a document that answers the question as one that
        # does not. Only the fixture carrying one can tell.
        "encoding: ignore a /ToUnicode entirely",
        "src/encoding.rs",
        "    if font.get(b\"ToUnicode\").is_ok() {\n        return Some(false);\n    }",
        "",
        "a_tounicode_settles_it_even_over_identity_ordering",
    ),
    Mutation(
        # The control on the composite rule, inverted. A Type1 font with no
        # `/ToUnicode` is most PDFs ever made, and judging one reports the world
        # as broken while every test above still passes.
        "encoding: consider simple fonts as well as composite ones",
        "src/encoding.rs",
        "    font.get(b\"Subtype\")\n"
        "        .and_then(Object::as_name)\n"
        "        .map(|name| name == b\"Type0\")\n"
        "        .unwrap_or(false)",
        "    let _ = font;\n    true",
        "a_simple_font_is_not_considered",
    ),
    Mutation(
        # `None` is "this font cannot be judged", and dropping it silently makes
        # the page's answer clean on evidence nobody has --- which is the exact
        # lie the module was written one level up to stop.
        "encoding: treat a font that cannot be judged as clean",
        "src/encoding.rs",
        "            None => mapping.truncated = true,",
        "            None => {}",
        "a_font_that_cannot_be_judged_is_reported_as_unknown",
    ),
    Mutation(
        # `Identity` means "these numbers are glyph indices in this font", so
        # there is no table to consult and PDFium is guessing. Listing it as
        # mappable is the plausible mistake, and it turns the whole module off.
        # The handover that specified this mutation recorded
        # `a_page_lopdf_cannot_account_for_is_unknown` as its catcher, which also
        # goes red; the test named for the rule is the one aimed at here.
        "encoding: list Identity as an ordering PDFium can map",
        "src/encoding.rs",
        'const MAPPABLE_ORDERINGS: [&str; 5] = ["Japan1", "GB1", "CNS1", "Korea1", "KR"];',
        'const MAPPABLE_ORDERINGS: [&str; 6] =\n'
        '    ["Japan1", "GB1", "CNS1", "Korea1", "KR", "Identity"];',
        "identity_ordering_without_a_tounicode_is_a_guess",
    ),
    # ------------------------------------------------------------------
    # docmodel.rs --- the working document and its journal.
    #
    # Every test in that module drives the model directly, so none of them is
    # over a fixture anyone else wrote and all of them could in principle be
    # tautologies. Two of the mutations below are aimed at claims made only in a
    # comment: the statement ordering inside `Move`, and the stale-snapshot
    # `retain`. `docs/TRAPS.md` records a comment that claimed an ordering
    # mattered where no mutation could show it, which is what these are for.
    # ------------------------------------------------------------------
    Mutation(
        # The ordering the comment above these two statements claims is
        # load-bearing. Reading the anchor's position first overshoots by one
        # whenever the moved page sits ahead of the anchor -- and leaves the
        # other direction correct, which is why only one of the two move tests
        # is named.
        "docmodel: read the move anchor's position before removing the page",
        "src/docmodel.rs",
        "                let from = self.position(page);\n"
        "                self.order.remove(from);\n"
        "                let to = match after {\n"
        "                    None => 0,\n"
        "                    Some(anchor) => self.position(anchor) + 1,\n"
        "                };",
        "                let from = self.position(page);\n"
        "                let to = match after {\n"
        "                    None => 0,\n"
        "                    Some(anchor) => self.position(anchor) + 1,\n"
        "                };\n"
        "                self.order.remove(from);",
        "a_page_moved_after_one_that_follows_it_lands_immediately_after_it",
    ),
    Mutation(
        # Insert before the anchor rather than after it. The off-by-one in the
        # other direction, and the likelier of the two to be written.
        "docmodel: move a page in front of its anchor instead of behind it",
        "src/docmodel.rs",
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor),\n                };\n                self.order.insert(to, page);",
        "a_page_moved_after_one_that_follows_it_lands_immediately_after_it",
    ),
    Mutation(
        # "No anchor" means the front. Sending it to the back instead is what a
        # reader would see as the drag going to the wrong end of the document.
        "docmodel: send an unanchored move to the back",
        "src/docmodel.rs",
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
        "                let to = match after {\n                    None => self.order.len(),\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
        "a_page_moved_with_no_anchor_goes_to_the_front",
    ),
    Mutation(
        # Collapse the two refusals into one. The model still refuses, the
        # document is still correct, and the only thing lost is the distinction
        # between an id that was deleted and one that never existed -- which is
        # the whole reason `docs/PLAN.md` asks for tombstones.
        "docmodel: report a deleted page as one that never existed",
        "src/docmodel.rs",
        "            Err(Refusal::PageDeleted(id))",
        "            Err(Refusal::NoSuchPage(id))",
        "a_command_naming_a_deleted_page_is_refused_as_deleted",
    ),
    Mutation(
        # Delete without tombstoning. Indistinguishable from a correct delete in
        # the document itself: the page is gone from the order and from the
        # table, and only a later command naming it can tell.
        "docmodel: delete a page without tombstoning its id",
        "src/docmodel.rs",
        "                self.graves.insert(page);",
        "                let _ = &mut self.graves;",
        "a_command_naming_a_deleted_page_is_refused_as_deleted",
    ),
    Mutation(
        # Take a snapshot that has forgotten the pending redactions. Every short
        # journal is unaffected -- SNAPSHOT_EVERY is 32, so most documents never
        # take one -- and a rebuild through such a snapshot silently drops every
        # region marked before it. A redaction missing from the review list is
        # the one failure this subsystem must never have, and it would arrive
        # with the document looking entirely normal.
        "docmodel: snapshot a document without its pending redactions",
        "src/docmodel.rs",
        "            self.snapshots.insert(self.cursor, self.now.clone());",
        "            let mut snap = self.now.clone();\n"
        "            snap.redactions.clear();\n"
        "            self.snapshots.insert(self.cursor, snap);",
        "a_pending_redaction_survives_a_rebuild_across_a_snapshot_boundary",
    ),
    Mutation(
        # Accept a region that covers nothing. It reaches the review list, the
        # overlay draws a line, and applying it removes nothing -- so a reader
        # is shown a row that certifies the removal of nothing at all.
        "docmodel: accept a redaction covering no area",
        "src/docmodel.rs",
        "        if !redaction.area.covers_area() {",
        "        if false {",
        "a_region_covering_nothing_is_refused_rather_than_listed",
    ),
    Mutation(
        # Mark a region without checking the page is live. The redaction lands
        # in the table under a page nobody can see, and every refusal the
        # frontend depends on for a stale panel row stops happening.
        "docmodel: mark a region without checking the page exists",
        "src/docmodel.rs",
        "        self.now.live(redaction.page)?;",
        "        let _ = self.now.live(redaction.page);",
        "a_region_on_a_page_that_is_gone_says_which_of_the_two_it_is",
    ),
    Mutation(
        # Collapse the two redaction refusals, exactly as the page pair above is
        # collapsed. The model still refuses; what is lost is the distinction
        # between a row a reader has already removed and an id nobody issued,
        # which is what a stale review panel needs told apart.
        "docmodel: report a removed redaction as one that never existed",
        "src/docmodel.rs",
        "                Refusal::RedactionRemoved(redaction)",
        "                Refusal::NoSuchRedaction(redaction)",
        "removing_a_pending_redaction_twice_is_told_which_answer_it_is_getting",
    ),
    Mutation(
        # Delete a page and leave its pending redactions behind. The document is
        # identical -- the page is gone from the order either way -- and the
        # instruction survives, pointing at a page nobody can see. Only a
        # command naming it afterwards can tell.
        "docmodel: delete a page without tombstoning its redactions",
        "src/docmodel.rs",
        "                for redaction in self.redactions.remove(&page).unwrap_or_default() {\n"
        "                    self.redaction_graves.insert(redaction);\n"
        "                }",
        "                let _ = self.redactions.remove(&page);",
        "deleting_a_page_takes_its_pending_redactions_with_it",
    ),
    Mutation(
        # Keep the body of a redaction whose command was discarded. Nothing
        # about the working document differs -- the redaction is gone from the
        # page either way -- so only the accounting observable can see it.
        "docmodel: keep redaction bodies a discarded redo tail named",
        "src/docmodel.rs",
        "                Command::Redact { redaction, .. } => {\n"
        "                    self.redactions.remove(&redaction);\n"
        "                }",
        "                Command::Redact { .. } => {}",
        "a_discarded_redo_tail_drops_the_redaction_bodies_it_named",
    ),
    Mutation(
        # Answer with every page's redactions rather than the named page's. A
        # single-page document cannot tell, and neither can a review list read
        # whole -- only an assertion about which page a region is on.
        "docmodel: list every page's redactions for whichever page is asked",
        "src/docmodel.rs",
        "    pub fn redactions_on(&self, page: PageId) -> &[RedactionId] {\n"
        "        self.redactions\n"
        "            .get(&page)",
        "    pub fn redactions_on(&self, page: PageId) -> &[RedactionId] {\n"
        "        self.redactions\n"
        "            .values()\n"
        "            .next()",
        "a_region_marked_for_removal_lands_on_the_page_it_names",
    ),
    Mutation(
        # A document with no pages is not a document.
        "docmodel: allow the last page to be deleted",
        "src/docmodel.rs",
        "                if self.order.len() == 1 {",
        "                if false {",
        "the_last_page_cannot_be_deleted",
    ),
    Mutation(
        # Let rotation accumulate past three. Every value below four is right, so
        # only a test that turns a page four times can see it -- and a viewer
        # that never turns a page more than twice would never show it either.
        "docmodel: let quarter turns accumulate without wrapping",
        "src/docmodel.rs",
        "                p.extra_turns = (i16::from(p.extra_turns) + i16::from(turns)).rem_euclid(4) as u8;",
        "                p.extra_turns = (i16::from(p.extra_turns) + i16::from(turns)).max(0) as u8;",
        "a_rotation_accumulates_and_wraps_at_four",
    ),
    Mutation(
        # Accept a crop box of zero width or height. It is the boundary, so every
        # inverted box is still refused and only the degenerate one gets through.
        "docmodel: accept a crop box enclosing no area",
        "src/docmodel.rs",
        "        self.urx > self.llx && self.ury > self.lly",
        "        self.urx >= self.llx && self.ury >= self.lly",
        "a_crop_enclosing_no_area_is_refused",
    ),
    Mutation(
        # Keep snapshots the redo tail's discard has invalidated. Nothing looks
        # wrong until a rebuild passes through one, and then every page after it
        # is built from commands that were thrown away.
        "docmodel: keep snapshots that the discarded redo tail produced",
        "src/docmodel.rs",
        "        self.snapshots.retain(|&at, _| at <= self.cursor);",
        "        self.snapshots.retain(|_, _| true);",
        "a_stale_snapshot_is_dropped_when_the_redo_tail_is_discarded",
    ),
    Mutation(
        # Do not discard the redo tail on a new command, which turns the journal
        # into a tree whose branches share a cursor.
        "docmodel: keep the redo tail when a new command is applied",
        "src/docmodel.rs",
        "        self.journal.truncate(self.cursor);",
        "        self.journal.truncate(self.journal.len());",
        "applying_after_an_undo_discards_the_redo_tail",
    ),
    Mutation(
        # Decode a PDF text string as Latin-1, which is what every "it is nearly
        # ASCII" implementation does and what this one deliberately does not.
        # The two agree on the accented range and disagree over 0x80--0x9F,
        # where PDFDocEncoding has punctuation.
        # Aimed inside `pdf_doc_encoded` rather than at one of its two call
        # sites: the first draft mutated the flush inside the loop, which a body
        # with no control characters never reaches, and it survived. That is the
        # trap about a mutation aimed at code no fixture reaches, met here.
        "annots: decode a text string as Latin-1 rather than PDFDocEncoding",
        "src/annots.rs",
        "        Ok(text) => text.chars().skip(1).collect(),",
        "        Ok(_) => run.iter().map(|&byte| byte as char).collect(),",
        "pdfdocencoding_is_not_latin1",
    ),
    Mutation(
        # Flatten a comment's body the way a title is flattened. Every visible
        # character survives; only the paragraphs are lost, which is why a
        # fixture asserting the words would pass.
        # Judged by `a_documents_body_keeps_its_paragraphs`, which reads a body
        # out of a document. The obvious candidate --- the test that calls
        # `sanitize_body` directly --- cannot see this at all, because what is
        # broken here is which flattener a body is *routed* to.
        "annots: collapse a body's newlines, as a one-line title would",
        "src/annots.rs",
        "    if keep_paragraphs {\n        sanitize_body(&decoded, limit)\n    } else {",
        "    if false {\n        sanitize_body(&decoded, limit)\n    } else {",
        "a_documents_body_keeps_its_paragraphs",
    ),
    Mutation(
        # The mirror: route every field through the body flattener, so an author
        # carrying a newline reaches a one-line byline with the newline in it.
        "annots: keep an author's newlines, as a body would",
        "src/annots.rs",
        "        crate::outline::sanitize_title(&decoded, limit)",
        "        sanitize_body(&decoded, limit)",
        "an_author_is_flattened_to_one_line",
    ),
]
