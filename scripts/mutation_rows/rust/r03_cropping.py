"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

ROWS += [
    Mutation(
        # Drop a body's paragraph breaks in the flattener itself, which is the
        # rule rather than the routing. Judged by the pure test, and the two
        # together are what say both halves work.
        "annots: drop a body's paragraph breaks",
        "src/annots.rs",
        "            pending_breaks = (pending_breaks + 1).min(2);\n            pending_space = false;\n            continue;\n        }\n        if ch.is_whitespace()",
        "            pending_breaks = 0;\n            pending_space = true;\n            continue;\n        }\n        if ch.is_whitespace()",
        "a_body_keeps_its_newlines_where_a_title_would_not",
    ),
    Mutation(
        # Take `/Rect` as written. A producer may write either corner first, and
        # the specification says a consumer shall normalise it --- so this is
        # invisible on every fixture whose rectangles happen to be written the
        # usual way round.
        "annots: trust /Rect's corner order",
        "src/annots.rs",
        "    let left = values[0].min(values[2]);\n    let right = values[0].max(values[2]);",
        "    let left = values[0];\n    let right = values[2];",
        "a_rectangle_written_backwards_is_normalised",
    ),
    Mutation(
        # Ignore the page's own rotation. Every rectangle is still on its page,
        # still the right size and still in the right order --- only in the
        # wrong place, which no count can see.
        "annots: place a rectangle without the page's /Rotate",
        "src/annots.rs",
        "    let placed = crate::text::to_device(\n        turns,",
        "    let placed = crate::text::to_device(\n        0,",
        "a_rotated_page_places_a_rectangle_in_display_space",
    ),
    Mutation(
        # Clamp nothing. A rectangle at 1e10 then reaches the viewer, which
        # places a marker somewhere it can never scroll to.
        "annots: leave a rectangle wherever the file put it",
        "src/annots.rs",
        "        placed[0].clamp(0.0, width),",
        "        placed[0],",
        "a_rectangle_off_the_page_is_clamped_to_it",
    ),
    Mutation(
        # Report a `/Link` or a `/Widget` as a kind nobody knows, which puts a
        # permanent "some comments were dropped" notice on every document that
        # has a hyperlink in it.
        "annots: count a link and a form field as unreadable kinds",
        "src/annots.rs",
        "            if !Kind::is_not_a_comment(subtype) {",
        "            if true {",
        "a_link_and_a_widget_are_not_comments_and_are_not_counted_as_unknown",
    ),
    Mutation(
        # Accept a reply link without checking that walking up from it
        # terminates. The panel then walks a cycle with no visited set of its
        # own, which is a hang rather than a wrong row.
        "annots: accept a reply link that closes a loop",
        "src/annots.rs",
        "        if looped {\n            limits.cycles += 1;",
        "        if false {\n            limits.cycles += 1;",
        "a_reply_cycle_is_broken_and_counted",
    ),
    Mutation(
        # Accept any string of digits as a date. Month 13 and hour 99 then reach
        # the panel, which shows them.
        "annots: accept a date outside the calendar",
        "src/annots.rs",
        "    if !(1000..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {",
        "    if false {",
        "a_string_that_is_not_a_date_produces_no_date",
    ),
    Mutation(
        # Keep the components that parsed and drop the one that did not. The
        # array is then *shorter*, so `[/Red 0 0 0]` is read as a legal
        # DeviceRGB triple and the comment comes out black -- which is why the
        # test's fixture is four entries long rather than three.
        "annots: skip an unreadable colour component instead of declining the array",
        "src/annots.rs",
        "    if values.len() != array.len() {",
        "    if false {",
        "a_colour_with_a_value_that_is_not_a_number_is_declined_whole",
    ),
    Mutation(
        # Let an infinity or a `NaN` through to the clamp. An infinity clamps to
        # a bound and looks like a colour; a `NaN` clamps to `NaN` and reaches
        # serde, which refuses it -- so this one is a wrong colour on one input
        # and a failed reply on the other.
        "annots: clamp a colour component without checking it is finite",
        "src/annots.rs",
        "    if values.iter().any(|value| !value.is_finite()) {\n        return None;\n    }\n    let rgb = match values[..] {",
        "    if false {\n        return None;\n    }\n    let rgb = match values[..] {",
        "a_colour_with_a_value_that_is_not_a_number_is_declined_whole",
    ),
    Mutation(
        # Read DeviceGray into the red channel alone. Every gray then comes out
        # a shade of red, and 0.0 -- black -- is the one value that still looks
        # right, which is why the test uses 0.25.
        "annots: read a one-number colour into one channel",
        "src/annots.rs",
        "        [gray] => [gray, gray, gray],",
        "        [gray] => [gray, 0.0, 0.0],",
        "a_colour_names_its_space_by_how_many_numbers_it_has",
    ),
    Mutation(
        # Drop the black component from the CMYK conversion. Correct for every
        # colour with `k` of zero, which is most of a palette.
        "annots: convert a CMYK colour without its black component",
        "src/annots.rs",
        "            (1.0 - c) * (1.0 - k),",
        "            1.0 - c,",
        "a_colour_names_its_space_by_how_many_numbers_it_has",
    ),
    Mutation(
        # Read an array of any other length as black rather than as no colour.
        # An empty `/C` -- which §12.5.2 spells *no colour* -- then paints a
        # black comment, and so does a malformed two-number array.
        "annots: read a colour of an illegal length as black",
        "src/annots.rs",
        "        _ => return None,\n    };\n    // Legal values",
        "        _ => [0.0, 0.0, 0.0],\n    };\n    // Legal values",
        "an_array_that_is_not_one_of_the_four_legal_lengths_is_not_a_colour",
    ),
    Mutation(
        # Pass a component outside 0..1 through. Harmless for a value a producer
        # rounded past 1.0 and not for one a document chose, and either way it
        # is a number no consumer of this field expects.
        "annots: do not clamp a colour component to its legal range",
        "src/annots.rs",
        "    Some(rgb.map(|component| component.clamp(0.0, 1.0)))",
        "    Some(rgb)",
        "a_component_outside_the_legal_range_is_clamped_rather_than_declined",
    ),
    Mutation(
        # Never read `/C` at all. Every unit test above still passes, because
        # they call `color_of` directly -- this is the one that says the reader
        # is *wired in*, and the only test that can see it is the one that goes
        # through `scan`.
        "annots: do not read a colour onto the comment",
        "src/annots.rs",
        "        color: color_of(annot, document),",
        "        color: None,",
        "the_scan_reads_the_colour_onto_the_comment",
    ),
    Mutation(
        # Rebuild from the newest snapshot rather than the newest one at or below
        # the target. Correct whenever undo has not crossed a snapshot, which is
        # most of the time.
        #
        # `a_journal_replays_to_the_state_it_was_applied_to` was named here first
        # and is the wrong test: it walks a mixed journal and every prefix of it,
        # but applies eight commands where SNAPSHOT_EVERY is 32, so it never has
        # a snapshot to pick the wrong one of. The harness reported it caught by
        # something else, which is the cross-check earning its keep --- the
        # mutation was aimed at code that test does not reach.
        "docmodel: rebuild from the newest snapshot, wherever it is",
        "src/docmodel.rs",
        "            .filter(|&at| at <= upto)",
        "            .filter(|&at| at <= upto.max(usize::MAX))",
        "a_rebuild_never_starts_from_a_snapshot_ahead_of_its_target",
    ),
    # --- links.rs ----------------------------------------------------------
    Mutation(
        # Read `/Dest` before `/A`, which is the ordering `outline.rs` had to
        # learn the hard way: a `/GoToR` carries a `/D` that resolves perfectly
        # against *this* document, so the link jumps to a plausible page of
        # another file's numbering instead of being refused.
        "links: take /Dest before the action that overrides it",
        "src/links.rs",
        '    if let Ok(action) = annot.get(b"A") {',
        '    if let Ok(dest) = annot.get(b"Dest") {\n        return destination(dest, document, numbers, geometry, limits);\n    }\n    if let Ok(action) = annot.get(b"A") {',
        "an_action_beats_a_dest_sitting_beside_it",
    ),
    Mutation(
        # Follow an action tpdf does not know instead of declining it. The arm
        # still refuses the four named ones, so every other refusal assertion
        # stays green --- which is why the unknown case needs its own test.
        "links: follow an unknown action's destination",
        "src/links.rs",
        '            _ => refused("unsupported"),',
        "            _ => match action.get(b\"D\") {\n                Ok(dest) => destination(dest, document, numbers, geometry, limits),\n                Err(_) => Target::Broken,\n            },",
        "an_unknown_action_is_refused_rather_than_followed",
    ),
    Mutation(
        # Read every fit's top from `/XYZ`'s position. `/FitH top` then reads the
        # element after the top, which on a real destination is absent --- so the
        # link lands at the page's top and looks like it works.
        "links: read every fit's top from XYZ's position",
        "src/links.rs",
        '        b"XYZ" => 3,\n        b"FitH" | b"FitBH" => 2,\n        b"FitR" => 5,',
        '        b"XYZ" | b"FitH" | b"FitBH" | b"FitR" => 3,',
        "each_fit_takes_its_top_from_its_own_position",
    ),
    Mutation(
        # Flip the destination offset against the page the *link* is on rather
        # than the page it lands on. Invisible on any document of uniform pages,
        # which is every fixture here but the one written for it.
        "links: flip the offset against the wrong page",
        "src/links.rs",
        "    let shown = *geometry.get(page as usize)?;",
        "    let shown = *geometry.first()?;",
        "the_offset_is_flipped_against_the_page_it_lands_on",
    ),
    Mutation(
        # Treat `/F` as a boolean rather than testing bit 2. Every real link sets
        # `/F 4` (Print), so this drops all of them --- which is why the control
        # in that test is a printing link rather than an unflagged one.
        "links: treat any /F as hidden",
        "src/links.rs",
        "            .is_some_and(|flags| flags & 0b10 != 0);",
        "            .is_some_and(|flags| flags != 0);",
        "a_hidden_link_is_not_clickable_and_a_printing_one_is",
    ),
    Mutation(
        # Give up on a name tree by reporting the name missing rather than the
        # bound firing. The link is `Broken` either way, so only the limit can
        # tell a hostile tree from an honest typo.
        "links: report an exhausted name tree as a missing name",
        "src/links.rs",
        "        Found::Exhausted => {\n            limits.unresolved_names += 1;\n            Target::Broken\n        }",
        "        Found::Exhausted => Target::Broken,",
        "a_cyclic_name_tree_is_given_up_on_and_counted",
    ),
    Mutation(
        # Charge a limit for an ordinary missing name too, which makes every
        # broken link in a healthy document look like a truncated scan. The
        # control for the mutation above, failing in the other direction.
        "links: charge a limit for a name that is simply absent",
        "src/links.rs",
        "        Found::Missing => Target::Broken,",
        "        Found::Missing => {\n            limits.unresolved_names += 1;\n            Target::Broken\n        }",
        "a_missing_name_is_broken_without_charging_a_limit",
    ),
    Mutation(
        # Look names up only in the name tree, dropping the PDF 1.1 dictionary.
        # A reader that knows one mechanism silently fails to follow every link
        # in whichever half of the corpus uses the other.
        "links: forget the PDF 1.1 /Dests dictionary",
        "src/links.rs",
        '    if let Ok(dests) = catalog.get(b"Dests") {',
        '    if let (false, Ok(dests)) = (true, catalog.get(b"Dests")) {',
        "a_named_destination_resolves_through_the_flat_dictionary",
    ),
    Mutation(
        # Trust `/Rect`'s corner order, which §12.5.2 says a consumer shall
        # normalise. Invisible on every rectangle written the usual way round.
        "links: trust /Rect's corner order",
        "src/links.rs",
        "            values[0].min(values[2]) as f64 - ox,\n            values[1].min(values[3]) as f64 - oy,\n            values[0].max(values[2]) as f64 - ox,\n            values[1].max(values[3]) as f64 - oy,",
        "            values[0] as f64 - ox,\n            values[1] as f64 - oy,\n            values[2] as f64 - ox,\n            values[3] as f64 - oy,",
        "a_rectangle_written_backwards_is_normalised",
    ),
    Mutation(
        # List zero-area links, which puts a target in the list no reader can
        # reach and every hit test walks past.
        "links: list a rectangle with no area",
        "src/links.rs",
        "        if rect[2] - rect[0] <= 0.0 || rect[3] - rect[1] <= 0.0 {\n            continue;\n        }",
        "        if false {\n            continue;\n        }",
        "a_zero_area_rectangle_is_left_out",
    ),
    Mutation(
        # Count a comment sharing the `/Annots` array as an unreadable entry,
        # which makes every reviewed document report a truncated link scan.
        "links: count a non-link annotation as unreadable",
        "src/links.rs",
        "            Ok(_) => continue,",
        "            Ok(_) => {\n                limits.unreadable += 1;\n                continue;\n            }",
        "annotations_that_are_not_links_are_skipped_without_complaint",
    ),
    Mutation(
        # Answer "no links" for a document whose pages could not be read. The
        # scan returns an empty list either way; only the limit can tell a
        # document with no links from one nothing could look at.
        "links: report pages lopdf could not read as nothing missing",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = 0;",
        "a_page_lopdf_cannot_account_for_is_reported",
    ),
    Mutation(
        # Charge a deficit for every document, which puts a warning on every file
        # tpdf opens and trains a reader to ignore the one that matters.
        "links: charge a page deficit even when the parsers agree",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.saturating_sub(pages.len()) + 1;",
        "a_document_both_parsers_agree_about_reports_nothing_missing",
    ),
    Mutation(
        # Underflow when lopdf reads further than PDFium paginates, reporting the
        # largest number the type can hold as a page deficit.
        "links: subtract page counts without saturating",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.wrapping_sub(pages.len());",
        "seeing_more_pages_than_claimed_is_not_a_deficit",
    ),
    Mutation(
        # The same silence in the comment scan, which is where the shape was
        # copied from --- so the two must be broken separately to prove each.
        "annots: report pages lopdf could not read as nothing missing",
        "src/annots.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = 0;",
        "a_page_lopdf_cannot_account_for_is_reported",
    ),
    Mutation(
        # And its control, in the other direction.
        "annots: charge a page deficit even when the parsers agree",
        "src/annots.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.saturating_sub(pages.len()) + 1;",
        "a_document_both_parsers_agree_about_reports_nothing_missing",
    ),
    Mutation(
        # One sentence for every failure, which is what stood here before: a
        # document that is well formed and merely locked is then announced as
        # damaged, and the reader goes looking for another copy of a good file.
        "progressive: give every open failure the same reason",
        "src/progressive.rs",
        '        err::PASSWORD => "This document is locked, and needs a password.".into(),',
        '        err::PASSWORD => "This file is not a PDF, or it is damaged beyond reading.".into(),',
        "each_reason_says_something_different",
    ),
    Mutation(
        # Keep the flag out of the reply. The reason still crosses and still
        # reads correctly, so what a reader loses is only the prompt --- they are
        # told, accurately, about a document they can no longer open.
        "worker_proto: stop sending whether a refusal is one a reader can answer",
        "src/worker_proto.rs",
        "    #[serde(default)]\n    pub locked: bool,",
        "    #[serde(default, skip_serializing)]\n    pub locked: bool,",
        "a_reply_is_locked_only_when_it_says_so",
    ),
    Mutation(
        # The parent's half of the same distinction, which is the copy that
        # drifts: `docs/TRAPS.md` records two copies of one distinction, and a
        # mutation of one surviving.
        "workers: drop the answerability when a worker's refusal reaches the engine",
        "src/workers.rs",
        "        locked: response.locked,",
        "        locked: false,",
        "a_locked_reply_reaches_the_engine_as_a_locked_refusal",
    ),
    Mutation(
        # Widen the one refusal a reader can answer to nearly every one. A
        # document that is not a PDF then asks for a password it has none for,
        # and the reader retypes into a dialog that can never accept anything.
        "progressive: treat almost every open failure as answerable",
        "src/progressive.rs",
        "            locked: code == err::PASSWORD,",
        "            locked: code != err::FORMAT,",
        "only_a_password_refusal_is_one_a_reader_can_answer",
    ),
    Mutation(
        # The other direction, through the widening `?` uses. Every ordinary
        # failure inside an open --- a page with no size, a path that is not
        # UTF-8 --- would then be shown as a locked document.
        "progressive: let a failure that arrived as prose claim it is locked",
        "src/progressive.rs",
        """        Self {
            reason,
            locked: false,
        }""",
        """        Self {
            reason,
            locked: true,
        }""",
        "a_refusal_widened_from_prose_is_not_locked",
    ),
    Mutation(
        # Report success as the reason. Reachable --- PDFium can return a null
        # handle with no error set --- and it reads as though the open worked.
        "progressive: report no-error as the reason a document did not open",
        "src/progressive.rs",
        '        _ => "This document could not be opened, and PDFium did not say why.".into(),',
        '        0 => "No error.".into(),\n        _ => "This document could not be opened, and PDFium did not say why.".into(),',
        "success_is_not_reported_as_a_reason",
    ),
    Mutation(
        # Walk the outline breadth-first: siblings before children. Both lists
        # still hold every entry, so only an order-sensitive comparison notices
        # -- and `links-probe --mode agree` compares two lists positionally.
        "links: walk the outline siblings-first rather than pre-order",
        "src/links.rs",
        "        if let Ok(Object::Reference(child)) = dict.get(b\"First\") {\n            walk_outline(\n                document,\n                *child,\n                numbers,\n                geometry,\n                seen,\n                out,\n                limits,\n                urls,\n                depth - 1,\n            );\n        }\n        node = match dict.get(b\"Next\") {",
        "        node = match dict.get(b\"Next\") {",
        "the_outline_walk_is_pre_order",
    ),
    Mutation(
        # Drop the visited set. A /Next chain that loops then does not return a
        # wrong answer -- it does not return, which is why the test asserts a
        # length rather than a value.
        "links: follow an outline chain that loops",
        "src/links.rs",
        "        if out.len() >= MAX_OUTLINE_ITEMS || !seen.insert(id) {",
        "        if out.len() >= MAX_OUTLINE_ITEMS {",
        "a_looping_outline_chain_terminates",
    ),
    Mutation(
        # Answer "no destination" for an entry whose /Dest does not resolve,
        # which is the PDFium behaviour this oracle exists to be independent of.
        # An oracle that reproduced the defect would agree perfectly and say
        # nothing.
        "links: call an unresolvable outline destination absent rather than broken",
        "src/links.rs",
        "    match annot.get(b\"Dest\") {\n        Ok(dest) => destination(dest, document, numbers, geometry, limits),",
        "    match annot.get(b\"Dest\") {\n        Ok(dest) => match destination(dest, document, numbers, geometry, limits) {\n            Target::Broken => Target::None,\n            other => other,\n        },",
        "a_destination_that_resolves_nowhere_is_broken_not_absent",
    ),
    Mutation(
        # Lay the page out from /MediaBox, which is what stood here before.
        # PDFium lays it out from /CropBox, so every rectangle on a cropped page
        # is out by the difference -- silently, on a page that looks normal.
        #
        # Aimed at `pagetree.rs` since the arithmetic moved there to serve the
        # mark writer as well. The test it expects is still `links.rs`'s: that
        # module is the caller that has a cropped fixture, and one mutation
        # reddening a test in another module is the point of sharing the code.
        "pagetree: place rectangles against the media box rather than the crop box",
        "src/pagetree.rs",
        "    let shown = match box_of(b\"CropBox\") {",
        "    let shown = match None::<[f32; 4]> {",
        "a_cropped_page_places_a_rectangle_in_the_crop_box_s_space",
    ),
    Mutation(
        # Take the crop box's *size* and ignore its origin. Subtler than the one
        # above and wrong in the same way: the page is the right shape and every
        # rectangle on it is shifted.
        "pagetree: use the crop box's size but not its origin",
        "src/pagetree.rs",
        "        origin: (shown[0], shown[1]),",
        "        origin: (0.0, 0.0),",
        "a_cropped_page_places_a_rectangle_in_the_crop_box_s_space",
    ),
    Mutation(
        # Trust a crop box larger than the sheet, so the page is displayed bigger
        # than its own paper and every rectangle is scaled against a size the
        # renderer never uses.
        "pagetree: trust a crop box larger than the media box",
        "src/pagetree.rs",
        "    let shown = match box_of(b\"CropBox\") {\n        Some(crop) => [\n            crop[0].max(media[0]),\n            crop[1].max(media[1]),\n            crop[2].min(media[2]),\n            crop[3].min(media[3]),\n        ],",
        "    let shown = match box_of(b\"CropBox\") {\n        Some(crop) => crop,",
        "a_crop_box_larger_than_the_page_is_intersected_with_it",
    ),
    Mutation(
        # The same blindness in the comment scan, which computes its geometry
        # separately -- so the two must be broken separately to prove each.
        "annots: place rectangles against the media box rather than the crop box",
        "src/annots.rs",
        "    let crop = box_of(b\"CropBox\").map(|crop| {",
        "    let crop = None::<[f32; 4]>.map(|crop: [f32; 4]| {",
        "a_cropped_page_places_a_comment_in_the_crop_box_s_space",
    ),
    Mutation(
        # `links.rs` carried three crop-box mutations and `annots.rs` one, for
        # code the two modules compute independently. The origin half of the
        # comment scan was reachable by no mutation, and the intersection half
        # by no *test* -- see the mirror added to annots.rs the same day.
        "annots: use the crop box's size but not its origin",
        "src/annots.rs",
        "        (width, height, turns, shown[0], shown[1])",
        "        (width, height, turns, 0.0, 0.0)",
        "a_cropped_page_places_a_comment_in_the_crop_box_s_space",
    ),
    Mutation(
        # Trust a crop box larger than the sheet. The twin of the links mutation
        # of the same name, against separate code with its own arithmetic.
        "annots: trust a crop box larger than the media box",
        "src/annots.rs",
        "    let crop = box_of(b\"CropBox\").map(|crop| {\n"
        "        [\n"
        "            crop[0].max(media[0]),\n"
        "            crop[1].max(media[1]),\n"
        "            crop[2].min(media[2]),\n"
        "            crop[3].min(media[3]),\n"
        "        ]\n"
        "    });",
        "    let crop = box_of(b\"CropBox\");",
        "a_crop_box_larger_than_the_page_is_intersected_with_it",
    ),
    Mutation(
        # `corner_of` was inline in `origin_pt` until 2026-08-16, where it needed
        # a live PDFium page and no unit test could reach it -- so both of its
        # rules were guarded by nothing, in the function every character, link
        # and comment position is measured from.
        # Re-aimed 2026-08-18: `corner_of` was folded into `normalised`, which
        # answers with the whole rectangle rather than one corner, because the
        # crop a reader sets needs all four numbers. The rule and its test are
        # the same; only where they live moved.
        "progressive: take a crop box's corners in the order they were written",
        "src/progressive.rs",
        "        box_pt[0].min(box_pt[2]),\n        box_pt[1].min(box_pt[3]),\n        box_pt[0].max(box_pt[2]),\n        box_pt[1].max(box_pt[3]),",
        "        box_pt[0],\n        box_pt[1],\n        box_pt[2],\n        box_pt[3],",
        "a_crop_box_written_backwards_still_yields_its_lower_left",
    ),
    Mutation(
        # One NaN here is a whole page of NaN boxes, and a NaN comparison fails
        # silently rather than loudly -- so it reads as an empty text layer.
        "progressive: let a non-finite crop box through",
        "src/progressive.rs",
        "    if !ok || !box_pt.iter().all(|value| value.is_finite()) {",
        "    if !ok {",
        "a_crop_box_with_a_non_finite_corner_is_refused",
    ),
    Mutation(
        # Trust the out-parameters of a call PDFium refused, which leaves them
        # holding whatever they held.
        "progressive: trust a crop box PDFium would not answer for",
        "src/progressive.rs",
        "    if !ok || !box_pt.iter().all(|value| value.is_finite()) {",
        "    if false || !box_pt.iter().all(|value| value.is_finite()) {",
        "a_crop_box_pdfium_would_not_answer_for_is_the_origin",
    ),
    Mutation(
        # Accept an update section built against a different number of bytes than
        # the caller measured. The two used to be one number by construction, and
        # stopped being one when the parse moved into the worker: a stale mapping
        # or a file that moved between the check and the build now produces a
        # cross-reference pointing into a document nobody has, in a file that
        # still opens.
        "append: accept an update built against a different length",
        "src/save.rs",
        "    if update.built_against as u64 != ready.was {",
        "    if false {",
        "an_update_built_against_a_different_length_is_refused",
    ),
    Mutation(
        # Keep the call and let it pass whenever the length matches, which is
        # what `append_in_place` compared until 2026-08-22 while
        # `Appended::verified` held a full fingerprint nothing read. A file
        # replaced by a distinct revision of the same size is accepted, and this
        # update's byte offsets go into an object graph they were never computed
        # against.
        "save: accept any file of the right length before appending",
        "src/save.rs",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)\n        .or_else(|e| if meta.len() == appended.was { Ok(()) } else { Err(e) })",
        "an_append_refuses_a_replacement_that_kept_the_length",
    ),
    Mutation(
        # Drop the guard entirely, so nothing is compared before the write. Its
        # sibling above keeps the call and weakens it; this one proves the call
        # is reached at all, which is the difference between a covered guard and
        # a guard whose caller nothing tests.
        "save: append without checking the file at all",
        "src/save.rs",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)\n        .map_err(|why| {",
        "    Ok::<(), String>(())\n        .map_err(|why: String| {",
        "an_append_refuses_a_replacement_that_kept_the_length",
    ),
    Mutation(
        # Ask the pathname which file this is instead of asking the handle, so
        # the identity check compares the replacement with itself and always
        # agrees. The save then reports success for edits that went somewhere
        # the reader cannot reach.
        "save: identify the file by name rather than by the handle written to",
        "src/save.rs",
        "    let writing_to = FileId::of(file).ok_or_else(|| {",
        "    let writing_to = FileId::at(source).ok_or_else(|| {",
        "an_append_writes_through_its_handle_and_says_so_when_the_name_moves",
    ),
    Mutation(
        # Write the update at the file offset the handle opens with, which is
        # zero, rather than seeking to the end. It overwrites the previous
        # revision's first bytes instead of adding to it -- and the reason this
        # line exists at all is that the handle is no longer in append mode:
        # append mode on Windows drops the access right `set_len` needs, so
        # every roll-back there would have failed with access denied.
        "save: write the update at the start of the file rather than the end",
        "src/save.rs",
        "    file.seek(std::io::SeekFrom::End(0))?;\n    file.write_all(body)?;",
        "    file.write_all(body)?;",
        "an_append_leaves_every_byte_of_the_previous_revision_where_it_was",
    ),
    Mutation(
        # Truncate whatever is at the staging name, which is what
        # `std::fs::write` did: it destroyed an unrelated file beside the
        # destination and followed a symlink planted at that path.
        "save: stage over whatever is already at the temporary name",
        "src/save.rs",
        "            .create_new(true)",
        "            .create(true)\n            .truncate(true)",
        "staging_never_writes_over_a_file_that_is_already_there",
    ),
    Mutation(
        # Go back to one staging name per destination. Two saves aimed at one
        # file share a temporary again, so the second truncates the first's bytes
        # and either can rename or delete the other's work.
        "save: give every save of a destination the same staging name",
        "src/save.rs",
        "    name.push(format!(\".{PARTIAL}-{}-{attempt}\", std::process::id()));",
        "    name.push(format!(\".{PARTIAL}\"));",
        "two_saves_to_one_destination_do_not_share_a_staging_file",
    ),
    Mutation(
        # Replace the page's `/Annots` rather than extending it. A page that had
        # no comments is unaffected, which is most pages and every fixture
        # written by hand -- and a page that had one loses it the moment a reader
        # highlights anything.
        "save: replace a page's /Annots instead of extending it",
        "src/save/marks.rs",
        "            array.push(Object::Reference(annotation));\n            doc.get_object_mut(page)",
        "            array.clear();\n            array.push(Object::Reference(annotation));\n            doc.get_object_mut(page)",
        "a_mark_is_written_whatever_shape_the_page_s_annots_is_in",
    ),
    Mutation(
        # Write the annotation object and leave it off the page. The file grows
        # by a perfectly well-formed annotation that is on no page, which every
        # reader reports as a document with no comments -- and which any check
        # counting objects passes.
        "save: write the mark object without listing it on the page",
        "src/save/marks.rs",
        "        let annotation = doc.add_object(dictionary);\n        attach(doc, page, annots, annotation)?;",
        "        let _annotation = doc.add_object(dictionary);",
        "a_marked_page_lists_the_mark_in_its_own_annots",
    ),
    Mutation(
        # Let a mark be written onto a page object that two page numbers share.
        # It appears on both, which is not what the reader asked for and not
        # something the file records as a mistake.
        "save: allow a mark on a page two numbers share",
        "src/save/marks.rs",
        # Re-aimed 2026-08-30: a mark is addressed by its position in the plan
        # now, so the count is over the plan's own page objects rather than over
        # the kept baseline numbers.
        "        if sheet.iter().filter(|id| **id == page).count() > 1 {",
        "        if false {",
        "a_mark_on_a_page_two_numbers_share_is_refused",
    ),
    Mutation(
        # Scope the shared-page refusal to the whole file rather than to this
        # mark's page. Every refusal test still passes; what breaks is the
        # ordinary case, where one malformed page makes the rest unmarkable.
        "save: refuse a mark anywhere in a file that has a shared page",
        "src/save/marks.rs",
        # Re-aimed 2026-08-30 with its sibling above, onto the same line and for
        # the same reason. The over-scoping is spelled differently and means
        # exactly what it did: refuse if *anything* in this document is shared.
        "        if sheet.iter().filter(|id| **id == page).count() > 1 {",
        "        if sheet.iter().any(|id| sheet.iter().filter(|other| *other == id).count() > 1) {",
        "a_mark_on_an_unshared_page_of_a_document_that_has_a_shared_one_is_written",
    ),
    Mutation(
        # Keep a quad that covers nothing. A `/QuadPoints` entry of zero area is
        # one some readers draw as nothing at all and others draw as a hairline,
        # so the mark's appearance stops being the document's.
        "save: keep quads that cover no area",
        "src/save/marks.rs",
        "        .filter(|quad| quad.covers_area())",
        "        .filter(|_| true)",
        "a_mark_whose_quads_all_collapse_is_refused_rather_than_written_empty",
    ),
    Mutation(
        # Let a plan carrying a mark call itself the file on disk. The print path
        # then hands over the original bytes, and a reader who highlighted a
        # document prints one without the highlights -- with nothing failing,
        # because what it printed is a perfectly good file.
        # Re-aimed 2026-08-26, when a redaction clause joined the predicate.
        "edits: call a plan with marks in it the file itself",
        "src/edits.rs",
        "        self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()",
        "        self.redactions.is_empty() && self.pages_are_the_file()",
        "a_plan_carrying_a_mark_is_not_the_file_on_disk",
    ),
    Mutation(
        # Drop the marks a subset plan should carry. An extract of pages a reader
        # highlighted comes out unmarked, which looks like a feature nobody
        # implemented rather than one that was lost on the way.
        "edits: leave the marks out of a plan",
        "src/edits.rs",
        # Re-aimed 2026-08-30: the `filter_map` that dropped the pages tpdf made
        # is an `enumerate` now, because every page carries marks.
        "    pages\n        .iter()\n        .enumerate()",
        "    pages\n        .iter()\n        .take(0)\n        .enumerate()",
        "a_mark_is_carried_into_the_plan_for_the_page_it_is_on",
    ),
    Mutation(
        # Write out the note the mark was made with rather than the one it says
        # now. A reader who types on a highlight and saves gets a file with the
        # highlight in it and an empty note, and everything on screen still says
        # what they typed -- so nothing is wrong until the file is reopened.
        "edits: write the note a mark was made with rather than what it says",
        "src/edits.rs",
        "                    note: model.note_of(*mark).to_string(),",
        "                    note: String::new(),",
        "a_note_reaches_the_reader_and_the_writer_as_the_same_words",
    ),
    Mutation(
        # The same on the other side of the boundary. The model has the note,
        # the file gets the note, and the box the reader types in opens empty
        # every time they come back to it.
        "edits: leave the note out of the state the frontend redraws from",
        "src/edits.rs",
        "                note: model.note_of(id).to_string(),",
        "                note: String::new(),",
        "a_note_reaches_the_reader_and_the_writer_as_the_same_words",
    ),
    Mutation(
        # Rewind the allocator with the cursor. This is the failure `docmodel`'s
        # module note names and deferred until something issued an id: the mark
        # a reader undid gives its number back, and the next mark is created
        # wearing it -- so a redo restores "the" mark and gets somebody else's.
        "docmodel: give an undone mark's id back to the allocator",
        "src/docmodel.rs",
        "        self.now = self.rebuild(self.cursor);\n        true",
        "        self.next_mark = self.next_mark.saturating_sub(1);\n        self.now = self.rebuild(self.cursor);\n        true",
        "an_id_spent_by_an_undone_mark_is_never_issued_again",
    ),
    Mutation(
        # Spend the id before the preconditions are checked. Nothing visible
        # changes -- the mark is still refused -- which is the point: the ids
        # then run ahead of the marks, and the only thing that can see it is the
        # accounting observable.
        "docmodel: issue a mark's id before checking that it covers anything",
        "src/docmodel.rs",
        "        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;",
        "        let id = MarkId(self.next_mark);\n        self.next_mark += 1;\n        let _ = id;\n        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;",
        "a_mark_covering_nothing_is_refused_and_spends_no_id",
    ),
    Mutation(
        # Demand that every quad have area. A selection running to the end of a
        # line yields a real rectangle followed by an empty one, so this refuses
        # ordinary highlights -- and passes every case in the refusal test,
        # which only ever hands it marks where nothing has area.
        "docmodel: require every quad to cover area rather than any",
        "src/docmodel.rs",
        "            !mark.quads.iter().any(|quad| quad.covers_area())",
        "            !mark.quads.iter().all(|quad| quad.covers_area())",
        "one_quad_with_area_is_enough",
    ),
    Mutation(
        # Drop a deleted page's marks without tombstoning their ids. The marks
        # vanish correctly and a command naming one afterwards is then told it
        # never existed, which is the wrong diagnosis rather than a coarse one.
        "docmodel: let a deleted page's marks go without tombstoning them",
        "src/docmodel.rs",
        "                for mark in self.marks.remove(&page).unwrap_or_default() {\n                    self.forget_mark(mark);",
        "                for mark in self.marks.remove(&page).unwrap_or_default() {\n                    let _ = mark;",
        "deleting_a_page_takes_its_marks_and_undo_brings_both_back",
    ),
    Mutation(
        # Leave an empty vector behind. Every behaviour is identical; what
        # changes is that a working document compares unequal to one that was
        # never annotated, which is what a snapshot rebuild is checked against.
        "docmodel: leave an empty mark list under a page key",
        "src/docmodel.rs",
        "                if list.is_empty() {\n                    self.marks.remove(&page);\n                }",
        "                let _ = list.is_empty();",
        "a_document_annotated_and_cleared_compares_equal_to_one_that_never_was",
    ),
    Mutation(
        # Keep the bodies of commands the redo tail discarded. Nothing behaves
        # differently and no document is wrong -- the table simply grows for as
        # long as a reader keeps annotating and undoing.
        "docmodel: keep mark bodies whose commands were discarded",
        "src/docmodel.rs",
        "                Command::Annotate { mark, note, .. } => {\n                    self.marks.remove(&mark);\n                    self.notes.remove(&note);\n                }",
        "                Command::Annotate { .. } => {}",
        "an_id_spent_by_an_undone_mark_is_never_issued_again",
    ),
    Mutation(
        # Keep the note when the mark's page is deleted. The mark is gone from
        # every list, so nothing on screen or in a written file differs -- and
        # the note is still reachable through a mark this document no longer
        # has, which an undo then restores twice over.
        # **Shares an anchor with the mutation below it, and that is a fact about
        # the code rather than a duplicate entry.** Deleting a page and removing
        # a mark were two cleanups doing different work; they are one
        # `forget_mark` since 2026-08-24, so there is one line to break and it
        # reddens both routes. Both entries are kept because each names a
        # *different* test, and what a mutation certifies is that its named test
        # can catch the loss --- folding them into one would leave one of those
        # two tests pinned by nothing. The cost is one duplicated run.
        "docmodel: keep a note when the page it is on is deleted",
        "src/docmodel.rs",
        "        self.mark_graves.insert(mark);\n        self.notes.remove(&mark);",
        "        self.mark_graves.insert(mark);",
        "a_marks_note_goes_with_it_and_comes_back_with_it",
    ),
    Mutation(
        # Keep the note when the mark is taken off the page. Same shape as the
        # entry above and a different arm: the map's keys are meant to be
        # exactly the live marks, and a leftover makes a document that was
        # annotated and un-annotated compare unequal to one that never was --
        # which is what a snapshot rebuild is checked against.
        # The other half of the pair above: same anchor, different test.
        "docmodel: keep a note when its mark is taken off the page",
        "src/docmodel.rs",
        "        self.mark_graves.insert(mark);\n        self.notes.remove(&mark);",
        "        self.mark_graves.insert(mark);",
        "a_mark_that_is_removed_says_nothing",
    ),
    Mutation(
        # Keep note versions whose commands went with the discarded redo tail.
        # No behaviour differs at all: the versions are unreachable, the ids are
        # never re-issued, and a reader who types and undoes in a loop grows the
        # table forever. `note_bodies` is the only observable there is.
        "docmodel: keep note versions whose commands were discarded",
        "src/docmodel.rs",
        "                Command::Renote { note, .. } => {\n                    self.notes.remove(&note);\n                }",
        "                Command::Renote { .. } => {}",
        "a_note_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # Issue the note's id before checking the mark is live. A refused note
        # then spends a version that nothing can ever read, which is the same
        # accounting `annotate` states for mark ids and the same reason.
        "docmodel: issue a note's id before checking the mark",
        "src/docmodel.rs",
        "        self.now.live_mark(mark)?;\n        let note = self.issue_note(note);",
        "        let note = self.issue_note(note);\n        self.now.live_mark(mark)?;",
        "a_refused_note_spends_nothing",
    ),
    Mutation(
        # Answer "no such mark" for one that was removed. The coarse diagnosis,
        # and the wrong one: it says the mark never existed to a caller whose
        # own undo took it off a moment ago.
        "docmodel: report a removed mark as one that never existed",
        "src/docmodel.rs",
        "            .ok_or(if self.mark_graves.contains(&mark) {",
        "            .ok_or(if false {",
        "a_note_names_a_mark_and_is_refused_by_name",
    ),
    Mutation(
        # Report the marks in whatever order the map iterates. The overlay and
        # the writer both take this as reading order, and a map's order is not
        # one -- it is stable within a run and unrelated to the document.
        "docmodel: report marks in map order rather than page order",
        "src/docmodel.rs",
        "        self.order\n            .iter()\n            .flat_map(|page| self.marks_on(*page).iter().map(|mark| (*page, *mark)))\n            .collect()",
        "        self.marks\n            .iter()\n            .flat_map(|(page, list)| list.iter().map(|mark| (*page, *mark)))\n            .collect()",
        "marks_come_back_in_page_order_after_the_pages_move",
    ),
    Mutation(
        # Undo the flip regardless of the turn. Correct at /Rotate 0 -- which is
        # most documents and every fixture written by hand -- and a quarter turn
        # out on the scanned pages that carry a rotation, which is exactly where
        # a highlight would land beside the words it was made from.
        "text: map a display box back with the arm for no rotation",
        "src/text.rs",
        "        1 => [top, left, bottom, right],",
        "        1 => [left, h0 - bottom, right, h0 - top],",
        "a_display_box_maps_back_to_the_page_box_it_came_from",
    ),
    Mutation(
        # Emit one arm's corners the wrong way round. `/QuadPoints` built from
        # an improper rectangle is one PDF 32000-1 tells consumers to normalise,
        # and the ones that do not draw nothing at all.
        "text: emit a mapped-back rectangle with its corners swapped",
        "src/text.rs",
        "        _ => [w0 - bottom, h0 - right, w0 - top, h0 - left],",
        "        _ => [w0 - top, h0 - left, w0 - bottom, h0 - right],",
        "a_mapped_back_rectangle_is_proper",
    ),
    Mutation(
        # Write an underline as a highlight. It draws correctly, because the
        # appearance stream is ours and is unaffected -- and Acrobat, Preview
        # and this application's own sidebar all report it as a highlight. The
        # failure that looks like nothing is wrong.
        "save: write an underline under the highlight's subtype",
        "src/save/marks.rs",
        '        MarkKind::Underline => b"Underline",',
        '        MarkKind::Underline => b"Highlight",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Treat a line as a wash: it then fills its whole quad, multiplied, at
        # 40%. One predicate deciding four things, which is why one mutation
        # reddens three tests.
        # Re-aimed when `is_wash` and `is_note` were replaced by one `ink`
        # question: the arm it named said `=> false` and now says `=> Ink::Line`.
        "save: draw the two line kinds as washes",
        "src/save/marks.rs",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Line,",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Wash,",
        "a_line_is_opaque_and_a_wash_is_not",
    ),
    Mutation(
        # Centre the underline on the quad's bottom edge, which is the obvious
        # reading of "under" and puts half the rule outside the `/BBox`. Every
        # reader clips it, and the result looks like a thinner line rather than
        # like a defect -- which is why the assertion is about the bound and not
        # about how it looks.
        "save: centre an underline on the edge it should sit on",
        "src/save/marks.rs",
        "        MarkKind::Underline => (bottom, thickness),",
        "        MarkKind::Underline => (bottom - thickness / 2.0, thickness),",
        "a_line_stays_inside_the_quad_it_marks",
    ),
    Mutation(
        # And a strikeout drawn at the bottom, which is an underline with the
        # wrong subtype. Every check keyed on the subtype passes; only where the
        # rule sits tells them apart.
        "save: draw a strikeout where an underline goes",
        "src/save/marks.rs",
        "        MarkKind::StrikeOut => (bottom + full / 2.0 - thickness / 2.0, thickness),",
        "        MarkKind::StrikeOut => (bottom, thickness),",
        "a_strikeout_crosses_the_text_and_an_underline_sits_under_it",
    ),
    Mutation(
        # Take the colour off the wire as it arrives. JSON refuses `NaN` and
        # `Infinity` as literals, which is what makes this look safe -- and
        # `1e40` is valid JSON and is `f32::INFINITY` once it is an `f32`, which
        # `format!` writes into a content stream as `inf`.
        "edits: take a colour off the wire without bringing it into range",
        "src/edits.rs",
        "                    color: want.color.map(channel),",
        "                    color: want.color,",
        "a_colour_off_the_wire_is_brought_into_the_range_the_model_promises",
    ),
    Mutation(
        # The boundary rather than the writer: ignore what the caller asked for.
        # Every mark is then a highlight however it was chosen, and the file is
        # correct for whatever the mark claims to be -- so `annot-probe` agrees
        # with it and only a test that names the kind on both sides can see it.
        "edits: write every mark as a highlight whatever was asked for",
        "src/edits.rs",
        "                    kind: want.kind,",
        "                    kind: MarkKind::Highlight,",
        "the_kind_the_caller_asked_for_reaches_the_plan_and_the_reply",
    ),
    Mutation(
        # The eraser's whole reason for a `quads_of` accessor: read the body
        # instead, which still holds the rectangle the drawing had before a
        # stroke was rubbed out. Every field the reply carries is otherwise
        # correct, so only a test that erases and then reads the rectangle can
        # see it.
        "edits: send the rectangle the drawing was made with, not the one it has",
        "src/edits.rs",
        "                quads: model\n                    .quads_of(id)",
        "                quads: mark\n                    .quads",
        "the_reply_carries_the_rectangle_the_drawing_has_now",
    ),
    Mutation(
        # The same read on the path that reaches a FILE rather than the window.
        # A document could then look right on screen and save the erased stroke,
        # which is the worse direction of the two.
        "edits: write the drawing the file was opened with, not the one on screen",
        "src/edits.rs",
        "                    strokes: model.strokes_of(*mark).to_vec(),",
        "                    strokes: body.strokes.clone(),",
        "a_saved_file_is_written_from_what_survived_the_eraser",
    ),
    Mutation(
        # Refuse instead of removing. The model is right to refuse a drawing of
        # nothing; this is the layer that knows the sweep meant "get rid of it",
        # and without it a reader who rubs out the last stroke gets an error
        # message and a drawing that is still there.
        "edits: refuse a sweep that takes the last stroke instead of removing the drawing",
        "src/edits.rs",
        "        if keep.iter().any(Stroke::is_drawable) {",
        "        if true {",
        "erasing_the_last_stroke_takes_the_drawing_with_it",
    ),
    Mutation(
        # Act on the half of the gesture that made sense. The stroke it did
        # understand is erased on the strength of a sweep aimed at a drawing
        # this is not -- and the reply looks entirely normal.
        "edits: let a sweep name a stroke that is not there",
        "src/edits.rs",
        "        if let Some(past) = remove.iter().find(|&&at| at >= held) {",
        "        if let Some(past) = None::<usize>.as_ref() {",
        "a_gesture_aimed_at_a_stroke_that_is_not_there_is_refused_whole",
    ),
    Mutation(
        # Keep the rectangle the strokes used to occupy. `Stroke::bounds` is
        # called in two places -- when a drawing is made and when it is erased --
        # and this is the second one.
        "docmodel: derive an erased drawing's rectangle from nothing",
        "src/docmodel.rs",
        "        let quads = Stroke::bounds(&strokes, (width / 2.0) as f32)",
        "        let quads = Stroke::bounds(&strokes, 1000.0)",
        "erasing_a_stroke_leaves_the_others_and_shrinks_the_rectangle",
    ),
    Mutation(
        # Leave the version behind when the mark goes. A mark removed and
        # restored by undo then comes back at whatever version an erasure left
        # it on rather than the one the journal says.
        "docmodel: let a removed drawing keep the version it was erased to",
        "src/docmodel.rs",
        "        self.inks.remove(&mark);\n        self.colors.remove(&mark);",
        "        self.colors.remove(&mark);",
        "a_removed_drawing_forgets_which_version_it_was_on",
    ),
    Mutation(
        # Read this process's counters instead of the child's. The mistake the
        # test exists for, and it is not hypothetical: `GetProcessMemoryInfo`
        # takes a handle, `GetCurrentProcess()` is the pseudo-handle for self,
        # and `tile_bench` passes exactly that on purpose two files away. A
        # worker's headroom would then be reported as the app's.
        "sandbox: read the parent's commit charge rather than the worker's",
        "src/sandbox_win.rs",
        "        let ok = unsafe { GetProcessMemoryInfo(self.process, &raw mut counters, counters.cb) };",
        "        let ok = unsafe { GetProcessMemoryInfo(windows_sys::Win32::System::Threading::GetCurrentProcess(), &raw mut counters, counters.cb) };",
        "a_contained_childs_peak_commit_is_readable_and_is_the_childs",
        only_on="windows",
    ),
    Mutation(
        # Lie about the struct's size. Win32 reads `cb` to decide which version
        # of the counters it was handed, so a zero is refused -- and the refusal
        # is the branch that produces `None`, which is what a caller reads as
        # "this platform cannot measure that" rather than as a broken call.
        "sandbox: hand GetProcessMemoryInfo a zero-length struct",
        "src/sandbox_win.rs",
        "        counters.cb = size_of_u32::<PROCESS_MEMORY_COUNTERS>();",
        "        counters.cb = 0;",
        "a_contained_childs_peak_commit_is_readable_and_is_the_childs",
        only_on="windows",
    ),
    Mutation(
        # Take the bound off. The plan condition still holds, so every marks-only
        # save goes back to being prepared in the worker whatever the document
        # weighs -- which is the state that shipped on 2026-08-22 and aborts a
        # worker above roughly 350 MB.
        "append: let a document of any size be prepared in the worker",
        "src/save.rs",
        "    if plan.is_appendable() && source_bytes <= APPEND_MAX_BYTES {",
        "    if plan.is_appendable() && source_bytes <= u64::MAX {",
        "a_marks_only_plan_is_rewritten_once_the_file_is_too_large_to_parse_twice",
    ),
    Mutation(
        # Move the bound to 256 MB from 256 MiB. A 5% change, in the direction
        # that is safe, and the point is that it is not silent: the value is
        # pinned so that it moves in the test as well or not at all.
        "append: move the size bound without saying so",
        "src/save.rs",
        "pub const APPEND_MAX_BYTES: u64 = 256 * 1024 * 1024;",
        "pub const APPEND_MAX_BYTES: u64 = 256 * 1000 * 1000;",
        "a_marks_only_plan_is_rewritten_once_the_file_is_too_large_to_parse_twice",
    ),
    Mutation(
        # Read an unmeasurable file as empty rather than as unbounded. It then
        # takes the append -- the arm with a memory bound over it -- on the one
        # input nothing knows the size of, which is the "could not check" and
        # "checked, fine" collapse this function exists to keep apart.
        "append: treat a file that cannot be measured as small",
        "src/save.rs",
        "        std::fs::metadata(source).map_or(u64::MAX, |m| m.len()),",
        "        std::fs::metadata(source).map_or(0, |m| m.len()),",
        "a_document_whose_size_cannot_be_read_is_rewritten",
    ),
    Mutation(
        # Point the hook at a file that is not there, which is what a rename
        # that forgets the config looks like. NOT spelled as a mistyped key:
        # `installerHooksTypo` is rejected by the build script's own schema
        # ("unknown field ... expected one of ... installerHooks"), so that
        # mutation never reaches a test and reports as a build failure.
        #
        # The bundler refuses this one too -- "failed to resolve `bundle >
        # windows > nsis > installerHooks`" -- but only when a bundle is
        # actually built, which is a CI leg and not a gate. The test answers in
        # seconds, on every machine.
        "installer: point the NSIS hook at a file that is not there",
        "tauri.windows.conf.json",
        '"installerHooks": "installer-hooks.nsh"',
        '"installerHooks": "installer-hooks-gone.nsh"',
        "the_windows_installer_clears_the_way_for_the_pdfium_directory",
    ),
    Mutation(
        # Leave the hook wired and take out the only thing it does. Everything
        # that can be checked from outside the installer -- the config, the
        # include, the macro definition -- still looks right.
        "installer: keep the hook but remove the deletion",
        "installer-hooks.nsh",
        '    Delete "$INSTDIR\\pdfium"',
        '    DetailPrint "$INSTDIR\\pdfium"',
        "the_windows_installer_clears_the_way_for_the_pdfium_directory",
    ),
    Mutation(
        # Prefer the page tree even where PDFium answered. Nothing visible moves
        # on any fixture -- the two agree wherever both answer -- so this is here
        # for the rule rather than for a symptom: the renderer's own reading is
        # what every downstream number is consistent with, and a second opinion
        # can only introduce a disagreement between the size a page reports and
        # the pixels it produces.
        "geometry: let the page tree override PDFium's own reading",
        "src/progressive.rs",
        "        (None, Some(from_tree)) => from_tree,",
        "        (_, Some(from_tree)) => from_tree,",
        "the_page_tree_decides_only_where_pdfium_has_no_media_box",
    ),
    Mutation(
        # Refuse to answer where PDFium could not. The page then keeps the box
        # PDFium computed for an inheriting page, which is the defect -- and a
        # document whose bytes cannot be re-read is meant to end up here, so the
        # arm has to stay reachable rather than be deleted.
        "geometry: decline the page tree's box even when it is the only one",
        "src/progressive.rs",
        "        (None, Some(from_tree)) => from_tree,",
        "        (None, Some(_)) => crop,",
        "the_page_tree_decides_only_where_pdfium_has_no_media_box",
    ),
    Mutation(
        # Return a short answer instead of refusing when the two parsers count
        # pages differently. The caller indexes positionally, so page 5's box
        # lands on page 4 -- a plausible page at a plausible size, with nothing
        # for a reader to notice.
        "geometry: return what the page tree found rather than refusing",
        "src/pagetree.rs",
        "    if pages.len() != page_count {",
        "    if false {",
        "a_page_count_the_two_parsers_disagree_about_is_refused",
    ),
    Mutation(
        # Stop undoing the display transpose. Every dimension is right and the
        # rectangle is on its side, which is the shape of error that survives a
        # size comparison and only a box comparison can see.
        "geometry: hand back the displayed rectangle as the page's own",
        "src/pagetree.rs",
        "        let (width, height) = if self.turns % 2 == 1 {\n            (self.height, self.width)\n        } else {\n            (self.width, self.height)\n        };",
        "        let (width, height) = (self.width, self.height);",
        "the_pages_own_box_is_the_same_rectangle_at_every_turn",
    ),
    # --------------------------------------------------------------
    # A mark on a page the document says is turned
    # --------------------------------------------------------------
    Mutation(
        # Stop swapping the box's sides at an odd quarter. This is the defect
        # itself, one level up from where it shipped: `wrap` is then handed the
        # box's height, which is what broke four words into eighteen lines.
        "turned marks: the reader's box keeps the page's own sides",
        "src/save/marks.rs",
        "            1 => Self {\n                width: h,\n                height: w,\n                origin: (quad[0], quad[1]),",
        "            1 => Self {\n                width: w,\n                height: h,\n                origin: (quad[0], quad[1]),",
        "an_upright_box_is_the_rectangle_the_reader_dragged",
    ),
    Mutation(
        # Anchor the reader's top-left at the wrong corner of the page-space
        # quad. Every size stays right and every mark moves by its own width,
        # which no size assertion can see -- the reason that test reads the
        # corners as well.
        "turned marks: the reader's corner is the page's",
        "src/save/marks.rs",
        "                origin: (quad[0], quad[1]),\n                right: (0.0, 1.0),",
        "                origin: (quad[0], quad[3]),\n                right: (0.0, 1.0),",
        "an_upright_box_is_the_rectangle_the_reader_dragged",
    ),
    Mutation(
        # Wrap to the page's width again, which is what shipped.
        "turned marks: a text box wraps to the page's width",
        "src/save/marks.rs",
        "        let width = seen.width - textbox::INSET * 2.0;",
        "        let width = (quad[2] - quad[0]) - textbox::INSET * 2.0;",
        "a_text_box_wraps_to_the_width_the_reader_dragged_however_the_page_is_turned",
    ),
    Mutation(
        # Keep the right lines and set them along the page's axis. The other
        # half of the same defect, and the half a line count cannot reach.
        "turned marks: type is set on the identity whatever the turn",
        "src/save/marks.rs",
        '            "{} {} {} {} {x} {y} Tm",\n            flat(self.right.0),\n            flat(self.right.1),\n            flat(-self.down.0),\n            flat(-self.down.1)',
        '            "{} {} {} {} {x} {y} Tm",\n            flat(1.0),\n            flat(0.0),\n            flat(0.0),\n            flat(1.0)',
        "type_runs_the_readers_way_on_a_turned_page",
    ),
    Mutation(
        # Stop normalising `-0.0`. The stream is still correct arithmetic and
        # the identity now prints as `1 0 -0 1`, which is the operator a reader
        # of the file has to squint at -- caught by the upright control rather
        # than by the turned assertion, which is why that control reads the
        # coefficients rather than merely counting them.
        "turned marks: a negated zero reaches the content stream",
        "src/save/marks.rs",
        "        let flat = |value: f64| if value == 0.0 { 0.0 } else { value };",
        "        let flat = |value: f64| value;",
        "type_runs_the_readers_way_on_a_turned_page",
    ),
    Mutation(
        # Put the rule back on the page's bottom edge. On a turned page that is
        # the left edge of the words.
        "turned marks: a rule sits on the page's bottom edge",
        "src/save/marks.rs",
        "        let (base, band) = line_rect(kind, 0.0, seen.height);\n        let [x, y, width, height] = seen.rect(",
        "        let (base, band) = line_rect(kind, 0.0, quad[3] - quad[1]);\n        let [x, y, width, height] = seen.rect(",
        "a_rule_sits_under_the_words_however_the_page_is_turned",
    ),
    Mutation(
        # Size a stamp's word from the page's box. The word is then set for a
        # rectangle of the other shape, which is a size error rather than a
        # placement one.
        "turned marks: a stamp is sized by the page's box",
        "src/save/marks.rs",
        "        let inner_w = seen.width - STAMP_INSET * 2.0;\n        let inner_h = seen.height - STAMP_INSET * 2.0;",
        "        let inner_w = (quad[2] - quad[0]) - STAMP_INSET * 2.0;\n        let inner_h = (quad[3] - quad[1]) - STAMP_INSET * 2.0;",
        "a_stamps_word_is_sized_by_the_box_the_reader_dragged",
    ),
    Mutation(
        # Rewrite in the coordinator, which is the code this replaced. Every
        # number a caller can see is identical on a good document -- the two
        # processes produce the same bytes -- so the only thing that can notice
        # is a test whose source does not parse here.
        #
        # It sits in `staged_rewrite` now rather than inline in
        # `stage_in_place`, because the copy and the split take the same step;
        # three tests can catch it and the one named below is the oldest.
        "save: rewrite in the coordinator instead of the worker",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = Here.write(source, len, out, plan, job, password, inputs)?;",
        "the_coordinator_does_not_parse_the_document_it_rewrites",
    ),
    Mutation(
        # Build the print job in the coordinator, which is what every print of
        # an edited document did until 2026-09-01. The bytes are identical
        # either way on a document that parses.
        "save: build the print job in the coordinator instead of the worker",
        "src/save.rs",
        # **Re-aimed 2026-09-01**, when the scratch dance both print routes do
        # moved into `into_scratch` and this call became the closure's body.
        "        staged_rewrite(\n            rewriter,\n            reading,",
        "        staged_rewrite(\n            &Here,\n            reading,",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Tell the writer a print job is a save. The reader's own rotation is
        # dropped, so the pages come out the way the file holds them rather than
        # the way they are looking at them -- and every byte-level assertion
        # about the job still passes, because the writer is asked for bytes and
        # answers with them.
        "save: build a print job as if it were a save",
        "src/save.rs",
        "        Job::Print { view },",
        "        Job::Save,",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Keep the scratch file the job was built in. It holds the reader's
        # document decrypted and reordered, in a shared directory, under a name
        # they never chose.
        "save: leave the print job's scratch file in the temporary directory",
        "src/save.rs",
        "    let _ = std::fs::remove_file(&at);\n    built",
        "    built",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Parse a copy in the coordinator, which is what Save a copy, Extract
        # and Redact to a copy did until 2026-09-01. The bytes are identical
        # either way on a document that parses, so the only thing that can
        # notice is a source this process cannot read.
        "save: copy in the coordinator instead of the worker",
        "src/save.rs",
        "    let staged = stage(out, |writing| {\n        staged_rewrite(\n            rewriter,\n"
        "            &mut reading,\n            len,\n            writing,\n            plan,\n            Job::Save,",
        "    let staged = stage(out, |writing| {\n        staged_rewrite(\n            &Here,\n"
        "            &mut reading,\n            len,\n            writing,\n            plan,\n            Job::Save,",
        "the_coordinator_does_not_parse_the_document_it_copies",
    ),
    Mutation(
        # The same edit on the split, which is a separate call site and needs a
        # separate mutation: one worker per part, and nothing about the copy's
        # call site says anything about this one.
        "save: split in the coordinator instead of the worker",
        "src/save.rs",
        "            staged_rewrite(rewriter, &mut opened, len, into, plan, Job::Save, password)",
        "            staged_rewrite(&Here, &mut opened, len, into, plan, Job::Save, password)",
        "the_coordinator_does_not_parse_the_document_it_splits",
    ),
    Mutation(
        # Merge in the coordinator, which is what every merge did until
        # 2026-09-01 -- and this one parses more than the reader's own document:
        # every file they picked in a dialog is loaded too. The bytes are
        # identical either way on documents that parse, so only a source this
        # process cannot read can notice.
        "merge: merge in the coordinator instead of the worker",
        "src/save.rs",
        "        staged_merge(\n            merger,",
        "        staged_merge(\n            &Here,",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Start every incoming document at the beginning of the buffer. Each one
        # then reads the first file's bytes under its own name and length -- the
        # page count is plausible, every span is inside the buffer, and the
        # result is a document. Only an assertion that reads the BYTES each span
        # points at can see it.
        #
        # `at: 0` rather than something cleverer, and that is a lesson: the first
        # version read `incoming.first()` inside the `push` that gives `incoming`
        # its element type, so the mutated file did not compile and the harness
        # reported `no summary line` -- which reads exactly like a drifted
        # anchor. A mutation has to be a program.
        "merge: start every incoming document at the beginning of the buffer",
        "src/save.rs",
        "        incoming.push(Incoming {\n            at,",
        "        incoming.push(Incoming {\n            at: 0,",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Report the page count this process guessed rather than the one the
        # writer answered. Counting it here would mean parsing the merged
        # document, which is the parse that moved -- so the only honest source is
        # the reply.
        "merge: report a page count of this process's own",
        "src/save.rs",
        "        .map(|counted| pages = counted)",
        "        .map(|_| pages = 0)",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Build the page range in the coordinator, which is what every print of
        # a typed range did until 2026-09-01 -- and which residual risk 18 never
        # listed, because it names the paths that *write* a document and this
        # one writes nothing. The bytes are identical either way on a document
        # that parses, so only a source this process cannot read can notice.
        "save: build a page-range print job in the coordinator instead of the worker",
        "src/save.rs",
        "        staged_range(rewriter, reading, len, into, job)",
        "        staged_range(&Here, reading, len, into, job)",
        "the_coordinator_does_not_parse_the_document_it_prints_a_range_of",
    ),
    Mutation(
        # Send the writer every page instead of the range the reader typed. The
        # job is well formed, the scratch file is cleaned up, the length checks
        # out and the reader gets a document -- the whole of it. Only an
        # assertion that reads what CROSSED can see this, which is why the test
        # named below records the job the writer was handed rather than only the
        # bytes it gave back.
        "save: print every page instead of the range that was asked for",
        "src/save.rs",
        "        staged_range(rewriter, reading, len, into, job)",
        "        staged_range(\n            rewriter,\n            reading,\n            len,\n            into,\n            &crate::print::Job { pages: crate::print::Pages::All, turns: job.turns },\n        )",
        "the_coordinator_does_not_parse_the_document_it_prints_a_range_of",
    ),
    Mutation(
        # Write every part of a split through one plan. The count of parts is
        # right, the files all exist, and each of them is the first part -- so a
        # check that only counts files cannot see it.
        "save: write every part of a split from the first plan",
        "src/save.rs",
        "    for (plan, target) in plans.iter().zip(&targets) {",
        "    for (plan, target) in std::iter::repeat(&plans[0]).zip(&targets) {",
        "a_split_writes_each_page_once_and_in_order",
    ),
    Mutation(
        # Trust the length the writer reported. The bytes never reach this
        # process, so this comparison is the whole of what stands between a
        # short write in another process and a rename over the reader's only
        # copy.
        "save: trust the rewrite's own byte count",
        "src/save.rs",
        "    if landed != wrote as u64 {",
        "    if false {",
        "a_rewriter_that_overstates_what_it_wrote_is_refused",
    ),
    Mutation(
        # Ask the writer about the staged file rather than the source. Under
        # `Here` the number is a capacity hint that changes no answer; a worker
        # maps exactly this many bytes of the document it is rewriting.
        "save: ask the rewrite about the wrong file's length",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, 0, out, plan, job, password, inputs)?;",
        "the_rewrite_is_asked_for_the_length_and_the_password",
    ),
    Mutation(
        # Drop the password on the way to the rewrite. `lopdf` parses no objects
        # at all for a document it cannot authenticate, so an encrypted save
        # would rewrite to an empty document rather than refusing.
        "save: rewrite without the reader's password",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, len, out, plan, job, None, inputs)?;",
        "the_rewrite_is_asked_for_the_length_and_the_password",
    ),
    Mutation(
        # Keep the partial file when the write refuses. It is a copy of the
        # reader's document, possibly a truncated one, left in their directory
        # under a name they did not choose.
        "save: leave the staging file behind on a refusal",
        "src/save.rs",
        "            let _ = std::fs::remove_file(&partial);\n            return Err(why);",
        "            return Err(why);",
        "a_rewriter_that_refuses_says_so_without_a_disk_error_in_front_of_it",
    ),
    Mutation(
        # Flatten the refusal to "this cannot be reloaded". The sentence still
        # reaches the reader and the one action that answers it does not.
        "proto: drop whether a refusal is answerable by reloading",
        "src/worker_proto.rs",
        "            changed: why.changed,",
        "            changed: false,",
        "a_refusal_carries_whether_reloading_would_answer_it",
    ),
    Mutation(
        # Parse the written file in the coordinator again, which is the code
        # this replaced. Nothing about the save's outcome changes on a good
        # file -- both readers agree wherever both answer -- so the only thing
        # that can notice is a test asserting WHO was asked.
        "save: verify the append in the coordinator instead of the worker",
        "src/save.rs",
        "    match reread.pages(file, expected, password) {",
        "    match Here.pages(file, expected, password) {",
        "the_coordinator_does_not_parse_the_file_it_wrote",
    ),
    Mutation(
        # Ask about the update alone rather than the whole file. Harmless under
        # `Here`, where the number is a capacity hint that costs an allocation
        # and changes no answer; a worker maps exactly this many bytes, so it
        # would verify a prefix of the file it is meant to be checking.
        "save: ask the re-read about the update rather than the file",
        "src/save.rs",
        "    let expected = usize::try_from(appended.was).unwrap_or(0) + appended.update.len();",
        "    let expected = appended.update.len();",
        "the_re_read_is_asked_for_the_length_the_save_produced",
    ),
    Mutation(
        # Drop the password on the way to the re-read. `lopdf` parses no objects
        # at all for a document it cannot authenticate, so an encrypted append
        # is verified as having zero pages and a correct save is rolled back --
        # a refusal rather than a corruption, which is the safe direction and
        # still wrong.
        "save: re-read an encrypted append without its password",
        "src/save.rs",
        "            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map(|after| after.get_pages().len())",
        "            password: None,\n            ..Default::default()\n        },\n    )\n    .map(|after| after.get_pages().len())",
        "an_encrypted_document_can_be_appended_to_and_stays_encrypted",
    ),
    Mutation(
        # Classify a filter chain by its FIRST entry. `/Filter` is applied in
        # decoding order, so the last entry is the one that produces the
        # content: `[/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG.
        # Reading the first calls it scannable, and the byte scan is then handed
        # a decoded JPEG to look for words in.
        "verify: classify a filter chain by its first entry",
        "src/verify.rs",
        "    let Some(last) = filters.last() else {",
        "    let Some(last) = filters.first() else {",
        "an_armoured_image_is_still_an_image",
    ),
    Mutation(
        # Stop recognising raster carriers at all. They then fall through to the
        # unrecognised arm, which is blind rather than deferred -- so the report
        # says no instrument can help where OCR would, and every scanned
        # document becomes uncertifiable for the wrong reason.
        "verify: stop recognising a raster carrier",
        "src/verify.rs",
        "    if IMAGE.contains(last) {\n        return Carrier::Image { filter: name() };\n    }",
        "    if false {\n        return Carrier::Image { filter: name() };\n    }",
        "the_raster_filters_are_deferred_to_a_different_instrument",
    ),
    Mutation(
        # Let a picture nobody read certify. THE one that would ship a lie: a
        # scanned document is nothing but image carriers, so a `deferred` list
        # that did not withhold the verdict would hand a reader the word
        # "verified" for a file where nothing read the only thing in it.
        "verify: certify a document whose pictures nobody read",
        "src/verify.rs",
        "        why.extend(self.deferred.iter().cloned());",
        "        // why.extend(self.deferred.iter().cloned());",
        "an_image_carrier_does_not_certify",
    ),
    Mutation(
        # Forget which filters are merely missing a decoder here. They become
        # "unrecognised", which is the deny-by-default arm and still withholds
        # certification -- so the verdict is unchanged and only the REASON is
        # wrong, telling a reader nobody has looked at a filter the format has
        # always had.
        "verify: conflate a missing decoder with an unknown filter",
        "src/verify.rs",
        '    const KNOWN: &[&[u8]] = &[b"ASCIIHexDecode", b"RunLengthDecode", b"Crypt"];',
        "    const KNOWN: &[&[u8]] = &[];",
        "a_filter_we_cannot_decode_is_blind_rather_than_deferred",
    ),
    Mutation(
        # Take the wrap away. Rust's `%` keeps the dividend's sign, so a page
        # written `/Rotate -90` comes back as a quarter-turn count of 255 -- and
        # the cast is what makes it silent, since `as u8` does not panic on a
        # negative in either profile.
        "pagetree: drop the wrap that makes a negative rotation a turn count",
        "src/pagetree.rs",
        "    (((degrees / 90) % 4 + 4) % 4) as u8",
        "    ((degrees / 90) % 4) as u8",
        "a_rotation_in_degrees_becomes_quarter_turns_clockwise",
    ),
    Mutation(
        # Round to the nearest quarter turn rather than truncating towards zero.
        # Every document that writes a multiple of 90 -- which is every document
        # that follows the specification -- is unaffected, and a page written
        # `/Rotate 45` is shown sideways.
        "pagetree: round a rotation to the nearest quarter turn",
        "src/pagetree.rs",
        "    (((degrees / 90) % 4 + 4) % 4) as u8",
        "    ((((degrees + 45) / 90) % 4 + 4) % 4) as u8",
        "a_rotation_that_is_not_a_multiple_of_ninety_truncates_towards_zero",
    ),
]

# --- cropping a page -----------------------------------------------------
ROWS += [
    Mutation(
        # Count a transparent pixel as ink. The renderer leaves the overhang
        # beyond the page as it found the buffer, so every page comes back
        # uncroppable.
        "content: read the paper beyond the page as ink",
        "src/content.rs",
        "            if px[3] == 0 || (px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE) {",
        "            if px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE {",
        "transparent_pixels_are_paper_too",
    ),
    Mutation(
        # Require every channel to be dark. Red text on white paper is then
        # blank, and a page of coloured type has no content box.
        "content: read only black as ink",
        "src/content.rs",
        "            if px[3] == 0 || (px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE) {",
        "            if px[3] == 0 || px[0] >= WHITE || px[1] >= WHITE || px[2] >= WHITE {",
        "a_single_channel_of_colour_is_ink",
    ),
    Mutation(
        # Treat the far edges as inclusive. Every crop is a pixel short on two
        # sides, which at the scan's resolution is three points of ink.
        "content: take the ink bounds as a closed rectangle",
        "src/content.rs",
        "            right = right.max(x + 1);\n            bottom = bottom.max(y + 1);",
        "            right = right.max(x);\n            bottom = bottom.max(y);",
        "one_inked_pixel_bounds_itself_and_nothing_else",
    ),
    Mutation(
        # Read past the end of a buffer smaller than it claims, in the process
        # holding the reader's document.
        "content: trust a buffer's claimed size",
        "src/content.rs",
        "    if pixels.len() < width * height * 4 {\n        return None;\n    }",
        "",
        "a_buffer_smaller_than_it_claims_is_refused",
    ),
    Mutation(
        # A threshold at pure white. Antialiasing puts a near-white fringe around
        # every glyph, so the box is found one pixel outside the ink.
        "content: find ink one pixel outside the ink",
        "src/content.rs",
        "const WHITE: u8 = 240;",
        "const WHITE: u8 = 255;",
        "a_near_white_fringe_is_paper_and_a_grey_is_not",
    ),
    Mutation(
        # No margin. The scan under-reports, so cropping to exactly the measured
        # box shaves descenders.
        "content: crop to exactly the measured ink",
        "src/content.rs",
        "pub const MARGIN_PT: f64 = 2.0;",
        "pub const MARGIN_PT: f64 = 0.0;",
        "the_margin_is_larger_than_a_pixel_of_the_scan",
    ),
    Mutation(
        # Write the crop onto the page's parent, which is inheritable -- so
        # cropping one page crops every page hanging under that node.
        "pagetree: write a crop where every page inherits it",
        "src/pagetree.rs",
        "        doc.get_object_mut(*id)\n            .and_then(Object::as_dict_mut)\n            .map_err(|e| format!(\"page {id:?} is not a dictionary: {e}\"))?\n            .set(\n                \"CropBox\",",
        "        let parent = doc\n            .get_object(*id)\n            .and_then(Object::as_dict)\n            .and_then(|d| d.get(b\"Parent\"))\n            .and_then(Object::as_reference)\n            .unwrap_or(*id);\n        doc.get_object_mut(parent)\n            .and_then(Object::as_dict_mut)\n            .map_err(|e| format!(\"page {id:?} is not a dictionary: {e}\"))?\n            .set(\n                \"CropBox\",",
        "a_crop_lands_on_the_page_and_not_on_its_parent",
    ),
    Mutation(
        # Write a crop that shares no area with the sheet. The page renders as
        # nothing, in whatever reader opens the file.
        "pagetree: write a crop that misses the page",
        "src/pagetree.rs",
        "        if box_pt[2] <= box_pt[0] || box_pt[3] <= box_pt[1] {",
        "        if false {",
        "a_crop_that_misses_the_page_is_refused",
    ),
    Mutation(
        # Skip the intersection with the sheet. A producer's oversized crop box
        # is written straight through.
        "pagetree: write a crop without clamping it to the sheet",
        "src/pagetree.rs",
        "        let box_pt = [\n            want[0].max(media[0]),\n            want[1].max(media[1]),\n            want[2].min(media[2]),\n            want[3].min(media[3]),\n        ];",
        "        let box_pt = *want;",
        "a_crop_outside_the_page_is_brought_back_onto_it",
    ),
    Mutation(
        # Let the last of two crops for one page win, silently, for both.
        "save: let one page be cropped two ways",
        "src/save.rs",
        "        match chosen.get(&source) {\n            None => {",
        "        match None::<&([f64; 4], usize)> {\n            None => {",
        "one_page_cropped_two_ways_is_refused_and_cropped_one_way_is_not",
    ),
    Mutation(
        # Write a crop box onto every page of every document tpdf saves.
        "save: plan a crop for a page nobody cropped",
        "src/save.rs",
        "        let Some(want) = page.crop else { continue };",
        "        let want = page.crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);",
        "a_plan_with_no_crop_writes_no_crop_box",
    ),
    Mutation(
        # Drop the crop between the model and the writer. The reader sees a
        # cropped page and every file they save is uncropped.
        "edits: leave the crop out of the plan",
        "src/edits.rs",
        "                crop: page.crop.map(|r| [r.llx, r.lly, r.urx, r.ury]),",
        "                crop: None,",
        "a_crop_reaches_the_reply_and_the_plan_and_clearing_it_removes_it",
    ),
    Mutation(
        # Fill the box instead of stroking it. Every file-level assertion about a
        # /Square -- subtype, rectangle, no quads, an /AP exists -- is satisfied
        # equally by a solid block of colour, and a solid block hides the figure
        # the box was drawn around.
        "save: fill a box rather than stroking its edge",
        "src/save/marks.rs",
        # **The trailing `}` is what makes this unambiguous, and it was not
        # needed until the stamp arrived**: a stamp is bordered by the same
        # `re S` line, so the bare string now matches twice. The box's is the one
        # that ends its arm; the stamp's is followed by the word it draws.
        'out.push_str(&format!("{x} {y} {width} {height} re S\\n"));\n    }',
        'out.push_str(&format!("{x} {y} {width} {height} re f\\n"));\n    }',
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Stroke every kind. The check written for the box passes unchanged --
        # which is what its control is for.
        # Anchored on the wash's arm rather than on `re f`, which occurs twice:
        # the anchor gate requires exactly one occurrence, and a mutation that
        # could land in either of two places is one nobody can reason about.
        "save: stroke a wash rather than filling it",
        "src/save/marks.rs",
        '        let (x, y) = (quad[0], quad[1]);\n        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n        out.push_str(&format!("{x} {y} {width} {height} re f',
        '        let (x, y) = (quad[0], quad[1]);\n        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n        out.push_str(&format!("{x} {y} {width} {height} re S',
        "the_wash_and_the_rules_fill_rather_than_stroke",
    ),
    Mutation(
        # Write the text box's words as a UTF-8 literal instead of WinAnsi hex.
        # Every ASCII text box stays perfect and every German one draws `Ã¼` --
        # the encoding defect this would have shipped with, and the reason the
        # writer uses hex at all.
        "save: write a text box's words as UTF-8 rather than WinAnsi",
        "src/save/marks.rs",
        '        let byte = if code <= 0xff { code as u8 } else { b\' \' };\n'
        '        out.push_str(&format!("{byte:02X}"));',
        '        for byte in ch.to_string().into_bytes() {\n'
        '            out.push_str(&format!("{byte:02X}"));\n'
        '        }',
        "a_text_box_draws_its_words_as_winansi_hex_rather_than_a_literal",
    ),
    Mutation(
        # Stop writing /DA. The mark still displays from its /AP everywhere, and
        # becomes uneditable in every reader but this one -- which no rendering
        # check can see, because nothing about the picture changes.
        "save: leave /DA off a text box",
        "src/save/marks.rs",
        "    if mark.kind == MarkKind::TextBox {",
        "    if false {",
        "a_text_box_carries_the_da_the_specification_requires_and_nothing_else_does",
    ),
    Mutation(
        # Give every mark a font in its appearance resources. Harmless-looking,
        # and it is the control for the `/Font` assertion: a check that only ever
        # says "the text box has one" passes equally if everything does.
        "save: put a font in every mark's appearance resources",
        "src/save/marks.rs",
        "    if style == Paint::Text {",
        "    if true {",
        "a_text_box_draws_its_words_as_winansi_hex_rather_than_a_literal",
    ),
    Mutation(
        # Push the empty leftover as a line. The trailing-blank defect a test
        # found while being written for termination: one spurious line in a
        # one-paragraph box, and a line of displacement for every paragraph after
        # the first in a longer one.
        "textbox: push an empty line after a word that was broken",
        "src/textbox.rs",
        "        if !line.is_empty() {\n            lines.push(line);\n        } else if paragraph.is_empty() {",
        "        if !line.is_empty() || !paragraph.is_empty() {\n            lines.push(line);\n        } else if paragraph.is_empty() {",
        "a_width_too_small_for_any_character_still_terminates",
    ),
    Mutation(
        # Never break a word that does not fit. It emits one line wider than the
        # box, the /BBox clips it, and the text disappears at the edge.
        "textbox: let a word wider than the box overflow rather than breaking it",
        "src/textbox.rs",
        "            let mut rest = word;\n            while advance(rest, size) > width {",
        "            let mut rest = word;\n            while false {",
        "a_word_wider_than_the_box_is_broken_rather_than_overflowing",
    ),
    Mutation(
        # Accept anything. A reader pastes Greek, tpdf draws it with a system
        # font on the overlay, and the saved file has substituted glyphs.
        "textbox: accept text Helvetica cannot write",
        "src/textbox.rs",
        # Re-aimed after `cargo fmt` collapsed the closure onto one line -- the
        # anchor did not drift, the formatter moved it, which the `anchors` gate
        # caught in 0.1 s. Run that gate after any `cargo fmt` that touches a
        # file a mutation points into.
        "        .all(|ch| ch == '\\n' || (' '..='~').contains(&ch) || ('\\u{a0}'..='\\u{ff}').contains(&ch))",
        "        .all(|ch| {\n            let _ = ch;\n            true\n        })",
        "what_helvetica_cannot_write_is_refused_and_what_it_can_is_not",
    ),
    Mutation(
        # Draw the squiggle as a flat rule. /Squiggly still goes in the file, so
        # every reader files it as a squiggle and draws an underline -- and the
        # subtype test passes it. The mirror of the mutation below it.
        "save: draw a squiggle as a flat rule",
        "src/save/marks.rs",
        "        MarkKind::Squiggly => Paint::Wave,",
        "        MarkKind::Squiggly => Paint::Line,",
        "a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule",
    ),
    Mutation(
        # Write the underline's subtype for a squiggle. Our own /AP draws the
        # right wave, so the mark looks correct in tpdf and is an underline to
        # everything else.
        "save: write /Underline for a squiggle",
        "src/save/marks.rs",
        '        MarkKind::Squiggly => b"Squiggly",',
        '        MarkKind::Squiggly => b"Underline",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Give the squiggle the underline's band. The wave is then drawn inside a
        # rule's height -- a 7%-tall zigzag, which at body size is a fuzzy line
        # and reads as bad antialiasing rather than as the wrong mark. It also
        # closes the strip every discriminating check reads.
        "save: fit a squiggle into an underline's band",
        "src/save/marks.rs",
        "        MarkKind::Squiggly => (bottom, full * SQUIGGLE_HEIGHT),",
        "        MarkKind::Squiggly => (bottom, thickness),",
        "a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule",
    ),
    Mutation(
        # Drop the squiggle out of the quad-carrying kinds. It is a text-markup
        # subtype and the specification lists /QuadPoints on it, so a reader that
        # positions from quads finds none and falls back to /Rect -- which is the
        # union of the run and is wrong the moment a mark spans two lines.
        "save: stop writing quads for a squiggle",
        "src/save/marks.rs",
        "        MarkKind::Highlight | MarkKind::Underline | MarkKind::Squiggly | MarkKind::StrikeOut",
        "        MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Draw the ellipse as a rectangle. `/Circle` still goes in the file, so
        # every reader files it under "ellipse" and every one of them draws a
        # box -- the subtype and the appearance disagreeing, which is the one
        # thing neither the subtype test nor a pixel count of the whole quad can
        # see on its own.
        "save: draw an ellipse with the box's rectangle",
        "src/save/marks.rs",
        "        MarkKind::Ellipse => Paint::Ellipse,",
        "        MarkKind::Ellipse => Paint::Outline,",
        "an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle",
    ),
    Mutation(
        # Write the box's subtype for an ellipse. Our own /AP draws the right
        # curve, so the mark looks correct in tpdf and is a rectangle to
        # everything else -- the mirror of the mutation above, and the reason
        # the two tests are separate.
        "save: write /Square for an ellipse",
        "src/save/marks.rs",
        '        MarkKind::Ellipse => b"Circle",',
        '        MarkKind::Ellipse => b"Square",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Leave the ellipse's path open. The fourth arc ends where the first
        # began, so the shape is right and the join at three o'clock is a cap
        # instead -- a nick in a thick stroke rather than a missing curve.
        "save: leave an ellipse's path open rather than closing it",
        "src/save/marks.rs",
        '        out.push_str("h S',
        '        out.push_str("S',
        "an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle",
    ),
    Mutation(
        # Stroke on the quad's own edge. Half of every side falls outside the
        # appearance stream's /BBox, which clips it -- so the box comes out with
        # hairline edges, which looks like a thin border and not like a bug.
        "save: stroke a box on its edge rather than inset by half the stroke",
        "src/save/marks.rs",
        "    let inset = OUTLINE_WIDTH / 2.0;",
        "    let inset = 0.0;",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Never set the stroke colour. `rg` sets the fill's and does not imply
        # `RG`, so the box comes out black -- which reads as a colour that was
        # ignored rather than one that was never set.
        "save: set only the fill colour, so the stroke comes out black",
        "src/save/marks.rs",
        "{r} {g} {b} rg {r} {g} {b} RG {width} w {joins}",
        "{r} {g} {b} rg {width} w {joins}",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Write /QuadPoints on every kind, as the writer did before a mark
        # existed that must not carry them. Most readers ignore an unlisted key
        # and one day something does not.
        "save: write text-markup quads on a box and a comment too",
        "src/save/marks.rs",
        "    if is_text_markup(mark.kind) {",
        "    if true {",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Give a box no appearance stream, as a comment correctly gets none.
        # Nothing synthesises a rectangle, so the annotation is in the file,
        # findable, removable -- and invisible.
        "save: leave a box's appearance to the reader, as a comment's is",
        "src/save/marks.rs",
        "        let appearance = if paint(mark.kind) == Paint::None {",
        "        let appearance = if !is_text_markup(mark.kind) {",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Keep a colour version whose command was discarded with the redo tail.
        # Invisible in every document the model can produce -- which is exactly
        # why the accounting observable exists, and exactly how its twin below
        # leaked for a week with nothing reading it.
        "docmodel: keep a colour after the command naming it is discarded",
        "src/docmodel.rs",
        "                Command::Recolor { color, .. } => {\n"
        "                    self.colors.remove(&color);\n"
        "                }",
        "                Command::Recolor { .. } => {}",
        "a_colour_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # The same for the eraser's table, and this one is not hypothetical: the
        # arm did not exist until the colour work went in, and a reader erasing
        # and undoing in a loop grew `inks` forever.
        "docmodel: keep a drawing after the command naming it is discarded",
        "src/docmodel.rs",
        "                Command::Reink { ink, .. } => {\n"
        "                    self.inks.remove(&ink);\n"
        "                }",
        "                Command::Reink { .. } => {}",
        "a_drawing_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # Leave a removed mark pointing at a colour version. Undo then brings the
        # mark back in whatever colour it had been recoloured to rather than the
        # one the journal says it was made in.
        #
        # Aimed at `forget_mark` since 2026-08-24, where it used to name the
        # `Unannotate` arm. The two arms that end a mark --- removing the mark,
        # and deleting the page it sits on --- were separate cleanups doing
        # different amounts of work, and this mutation could only reach the one
        # that did more. One helper is one anchor, and it now reaches both.
        "docmodel: leave a removed mark's colour behind",
        "src/docmodel.rs",
        "        self.inks.remove(&mark);\n        self.colors.remove(&mark);",
        "        self.inks.remove(&mark);",
        "a_removed_mark_forgets_which_colour_it_was_on",
    ),
    Mutation(
        # Answer a mark's colour out of its body. Everything still works and
        # every recolour is silently ignored -- on screen and in the file, since
        # both readers go through this one accessor.
        "docmodel: answer a mark's colour out of its body",
        "src/docmodel.rs",
        "        self.now\n            .color_of(mark)\n            .and_then(|color| self.colors.get(&color))",
        "        None.and_then(|color: ColorId| self.colors.get(&color))",
        "recolouring_changes_what_a_mark_is_drawn_in_and_undo_puts_it_back",
    ),
    Mutation(
        # Spend the id before checking the mark. A refused recolour then leaks a
        # colour version per press, which no document can show.
        "docmodel: spend a colour id before checking the mark is there",
        "src/docmodel.rs",
        "        self.now.live_mark(mark)?;\n        let color = self.issue_color(color);",
        "        let color = self.issue_color(color);\n        self.now.live_mark(mark)?;",
        "recolouring_a_mark_that_is_not_there_is_refused_before_an_id_is_spent",
    ),
    Mutation(
        # Reply with the colour the mark was made in. The overlay paints from
        # this field, so a recolour would be accepted, journalled, saved -- and
        # invisible until the file was reopened, which is the shape of wrong the
        # overlay work was done to end.
        "edits: reply with the colour the mark was made in",
        "src/edits.rs",
        "                color: model.color_of(id),",
        "                color: mark.color,",
        "the_reply_carries_the_colour_the_mark_has_now",
    ),
    Mutation(
        # And the writer's half, which the reply's test cannot see: the mark
        # looks right on screen and saves in its first colour.
        "edits: write the colour the mark was made in",
        "src/edits.rs",
        "                    color: model.color_of(*mark),",
        "                    color: body.color,",
        "a_saved_file_is_written_in_the_colour_the_mark_has_now",
    ),
    Mutation(
        # Let a recolour past the clamp. `1e40` is valid JSON, becomes `inf` in
        # an `f32`, and `format!` writes the three letters into a content stream
        # -- a file no reader can parse, through the command that only changes an
        # appearance.
        "edits: let a recolour past the clamp a new mark goes through",
        "src/edits.rs",
        "            .recolor(MarkId::from_raw(mark), color.map(channel))",
        "            .recolor(MarkId::from_raw(mark), color)",
        "a_colour_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
]

# --- printing an encrypted document ---------------------------------------
ROWS += [
    Mutation(
        # Reserialise an encrypted document to print a selection of it. `lopdf`
        # writes every object in the clear, so the job -- which reaches a
        # printer, the platform's own PDF reader, and Print to PDF -- is a
        # decrypted copy of the reader's document.
        "print: build a selection from an encrypted document",
        "src/print.rs",
        "    if doc.was_encrypted() || doc.is_encrypted() {",
        "    if false {",
        "an_encrypted_document_is_printed_whole_or_refused",
    ),
    Mutation(
        # Refuse every document. The control: without it, a guard that never let
        # anything through would satisfy every refusal the test above asserts.
        "print: refuse every document as encrypted",
        "src/print.rs",
        "    if doc.was_encrypted() || doc.is_encrypted() {",
        "    if true {",
        "an_unencrypted_document_still_prints_a_selection",
    ),
]

# --- what a document says about itself ------------------------------------
ROWS += [
    Mutation(
        # Read the encryption *after* decrypting, which reading the code alone
        # suggests is fine. `lopdf::decrypt` removes the trailer entry and the
        # object it points at, so the same call then answers "not encrypted" for
        # a document that plainly is, and every permission goes with it.
        "docinfo: ask about the encryption after decrypting rather than before",
        "src/docinfo.rs",
        "    let encryption = read_encryption(document).or_else(|| encryption_from_state(document));",
        "    let encryption: Option<Encryption> = None;",
        "a_document_that_needs_a_password_says_so_rather_than_reporting_nothing",
    ),
    Mutation(
        # Drop the second route and keep the first. The trailer answers only for
        # a document nothing unlocked, so this reports every unprompted
        # permission-restricted file -- the commonest encrypted PDF there is --
        # as carrying no encryption at all.
        "docinfo: read the encryption only from the trailer",
        "src/docinfo.rs",
        "    let encryption = read_encryption(document).or_else(|| encryption_from_state(document));",
        "    let encryption = read_encryption(document);",
        "an_encrypted_document_reports_its_encryption_either_way",
    ),
    Mutation(
        # Report every document as encrypted. The control for the mutation
        # above: without it, a route that answered `Some` unconditionally would
        # satisfy every assertion the encrypted fixtures make.
        "docinfo: report an unencrypted document as encrypted",
        "src/docinfo.rs",
        "    let state = document.encryption_state.as_ref()?;",
        "    let Some(state) = document.encryption_state.as_ref() else {\n        return Some(Encryption::default());\n    };",
        "a_document_with_no_encryption_reports_none",
    ),
    Mutation(
        # Report a locked document's structure tree as absent. "No" and "could
        # not look" are different claims and only one is true here; the `Option`
        # is what carries the difference.
        "docinfo: report a locked document as untagged rather than unknown",
        "src/docinfo.rs",
        '        tagged: catalog.map(|c| c.has(b"StructTreeRoot")),',
        '        tagged: Some(catalog.is_some_and(|c| c.has(b"StructTreeRoot"))),',
        "a_document_that_needs_a_password_says_so_rather_than_reporting_nothing",
    ),
    Mutation(
        # Ignore the revision, so bits 9 to 12 are read as permissions on a
        # revision-2 document. All four are set in `P = -60` because the number
        # is negative, so this reports a document forbidding accessibility
        # extraction as permitting it.
        "docinfo: read the reserved permission bits whatever the revision",
        "src/docinfo.rs",
        "    let old = revision < 3;",
        "    let old = false;",
        "revision_2_reads_the_reserved_bits_as_the_coarser_ones_they_stand_for",
    ),
    Mutation(
        # Take any `/Reference` entry's level. A `/FieldMDP` transform's `/P` is
        # a field-locking mode, not a certification level, so this reports a
        # signature locking one form field as certifying the whole document.
        "docinfo: take any transform's level, not the DocMDP one",
        "src/docinfo.rs",
        '        if name_of(document, reference, b"TransformMethod") != method {',
        "        if false {",
        "the_certification_level_comes_from_the_docmdp_reference_and_no_other",
    ),
    Mutation(
        # Check only where the signed range ends. A range starting past zero
        # leaves the file's head unsigned and still ends where the file does ---
        # the half of the check that reading the code calls redundant.
        "docinfo: check only where the signed range ends, not where it starts",
        "src/docinfo.rs",
        "        out.covers_whole_file = end == Some(size) && out.starts_at_zero;",
        "        out.covers_whole_file = end == Some(size);",
        "a_range_that_skips_the_start_of_the_file_is_not_whole_coverage",
    ),
    Mutation(
        # Report every range as beginning at the first byte, which is what let
        # "everything up to the signature" be said of a range that skips the head.
        "docinfo: report every signed range as beginning at the first byte",
        "src/docinfo.rs",
        "        out.starts_at_zero = numbers.first() == Some(&0);",
        "        out.starts_at_zero = true;",
        "a_range_that_skips_the_start_of_the_file_is_not_whole_coverage",
    ),
    Mutation(
        # Count the container as an append. This is the defect the field was
        # added to fix, wearing the shape it had before: `size -
        # covered_bytes` is the uncovered total, and on a real DocuSign
        # contract 65,536 of its 74,637 bytes are the `/Contents` hex string
        # that no signature can cover. The panel then leads with 73 KB where
        # the honest number is 9 KB.
        # Tag the reply enum internally rather than adjacently. This is not a
        # style choice being reverted: serde cannot merge a tag into a payload
        # that is not a map, so `Reply::Content(Option<[f64; 4]>)` fails to
        # serialise AT RUNTIME with "cannot serialize tagged newtype variant
        # containing an optional". Content is the crop tool's ink bounds, so a
        # reader would meet it as a worker that cannot answer. Found this way,
        # before the code ran once.
        "proto: tag the reply enum internally, which cannot carry a bare payload",
        "src/worker_proto.rs",
        '#[serde(tag = "reply", content = "value", rename_all = "kebab-case")]',
        '#[serde(tag = "reply", rename_all = "kebab-case")]',
        "every_reply_variant_survives_the_wire_as_itself",
    ),
    Mutation(
        # Drop the tag entirely. `Content(Option<[f64; 4]>)` and
        # `CropBox([f32; 4])` are then the same four numbers on the wire, so serde
        # takes the first that fits and a crop box arrives as ink bounds --- the
        # right numbers carrying the wrong meaning, which is the failure mode the
        # untyped `serde_json::Value` had in a different costume.
        "proto: leave the reply enum untagged, so two payloads of four numbers collide",
        "src/worker_proto.rs",
        '#[serde(tag = "reply", content = "value", rename_all = "kebab-case")]',
        "#[serde(untagged)]",
        "every_reply_variant_survives_the_wire_as_itself",
    ),
    Mutation(
        # Send a payload on a reply that says it failed. Every caller checks `ok`
        # before reading the payload, so this is the reply that carries an answer
        # nobody will look at -- and the parent reports the worker's `error`
        # string, which is empty.
        "proto: mark a payload-bearing reply as a failure",
        "src/worker_proto.rs",
        "    pub fn reply(reply: Reply) -> Self {\n        Self {\n            ok: true,",
        "    pub fn reply(reply: Reply) -> Self {\n        Self {\n            ok: false,",
        "a_response_carries_its_reply_through_the_framing",
    ),
    Mutation(
        # Put a portable spike back on the macOS library directory. `lib/` exists
        # on Windows and holds the IMPORT library, so the path resolves, the
        # directory check passes and the bind fails much later pointing at
        # something that is right there. PDFIUM_SUBDIR's own note says this had
        # been rediscovered three times and named the grep that checks it; on
        # 2026-08-25 four probes had drifted back, which is what a rule with
        # nothing enforcing it does.
        # Count every id swept rather than every document actually released.
        # `close` leaves holes rather than removing entries, so a table of holes
        # is the ordinary shape here -- and this makes a fresh start claim it
        # released documents belonging to a webview that never existed. The one
        # line this count produces means "a page reloaded", which is exactly what
        # the miscount turns into a lie.
        #
        # A sibling mutation belongs here and is deliberately absent: dropping the
        # `slot.is_some()` filter that builds the id list. Running it showed it
        # SURVIVES, and correctly -- the filter keeps `close` from being asked
        # about holes, and `close` refuses a hole anyway, so the count is held
        # here rather than there. A variant, not a gap; recorded so the next
        # person to notice finds out it was measured.
        "workers: sweep the documents without closing any of them",
        "src/workers.rs",
        "            if self.close(doc).is_ok() {",
        "            if false {",
        "releasing_everything_empties_every_slot_and_reports_how_many",
    ),
    Mutation(
        "lib: hardcode the macOS library directory in a portable spike",
        "examples/crop_probe.rs",
        'PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR)',
        'PathBuf::from("vendor/pdfium/lib")',
        "only_the_macos_spikes_hardcode_the_library_directory",
    ),
    Mutation(
        # Read the appendix without requiring the signed prefix to parse. With
        # no prefix to compare against, every object in the document reads as
        # ADDED -- so an ordinary validation-data append is reported as the
        # document having been rewritten from nothing, which is the alarming
        # direction to be wrong in. The guard is "both parses or neither".
        "docinfo: describe an appendix whose signed prefix would not parse",
        "src/docinfo.rs",
        "    let (Ok(whole), Ok(signed)) = (load(bytes), load(&bytes[..end])) else {",
        "    let signed = load(&bytes[..end]).unwrap_or_default();\n    let Ok(whole) = load(bytes) else {",
        "an_appendix_whose_signed_prefix_is_unparseable_is_reported_as_unread",
    ),
    Mutation(
        # Count a page like any other object. `pages_touched` is the one number
        # separating an append that rewrote something a reader sees from one that
        # did not, and without it the two rows read identically.
        "docinfo: stop counting the pages an appendix rewrote",
        "src/docinfo.rs",
        "    out.pages_touched = touched.len() + moved;",
        "    out.pages_touched = moved;",
        "a_second_signature_is_reported_as_a_signature_rather_than_as_a_size",
    ),
    Mutation(
        # Slice past the end of the file. A `/ByteRange` written for bytes a
        # truncation removed gives an `end` beyond the buffer, and this bound is
        # what stands between that and a panic inside the worker.
        "docinfo: slice an appendix on an end past the file",
        "src/docinfo.rs",
        "    if end == 0 || end >= bytes.len() {",
        "    if end == 0 {",
        "an_appendix_outside_the_file_is_refused_rather_than_sliced",
    ),
    Mutation(
        "docinfo: report every uncovered byte as an append",
        "src/docinfo.rs",
        "        out.appended_bytes = end.map_or(0, |end| size.saturating_sub(end));",
        "        out.appended_bytes = size.saturating_sub(out.covered_bytes);",
        "what_was_appended_after_signing_is_counted_apart_from_the_container",
    ),
    Mutation(
        # Subtract without saturating. A range written for bytes a truncation
        # removed claims to end past the file, and an unsigned wrap then tells
        # the reader 18 exabytes were appended -- which is not a smaller error
        # than the one above, it is a different one.
        "docinfo: let a range past the end of the file wrap the append count",
        "src/docinfo.rs",
        "        out.appended_bytes = end.map_or(0, |end| size.saturating_sub(end));",
        "        out.appended_bytes = end.map_or(0, |end| size.wrapping_sub(end));",
        "a_range_reaching_past_the_end_of_the_file_reports_no_append",
    ),
    Mutation(
        # Sum every number in `/ByteRange`. Offsets are not lengths, so this
        # reports a signature as covering far more than it does --- wrong in the
        # direction that reassures.
        "docinfo: add up every byte-range number, offsets included",
        "src/docinfo.rs",
        "            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())",
        "            .map(|pair| u64::try_from(pair[0] + pair[1]).unwrap_or_default())",
        "a_signature_covering_the_file_says_so_and_one_that_does_not_says_that",
    ),
    Mutation(
        # Let the catalog's `/Version` win whatever it says. It exists so an
        # incremental update can *raise* the version, never lower it.
        "docinfo: let the catalog's version win even when it is earlier",
        "src/docinfo.rs",
        "    if catalog > header {",
        "    if !catalog.is_empty() {",
        "the_later_of_the_header_and_the_catalog_version_wins",
    ),
    Mutation(
        # Count a name tree's entries rather than its pairs. `/Names` interleaves
        # key and value, so this reports twice as many attachments as there are.
        "docinfo: count a name tree's entries rather than its pairs",
        "src/docinfo.rs",
        "        .map_or(0, |array| array.len() / 2)",
        "        .map_or(0, std::vec::Vec::len)",
        "attachments_are_counted_as_pairs_because_a_name_tree_interleaves_them",
    ),
    Mutation(
        # Shorten a value and say nothing. The readout then looks whole while a
        # property is silently truncated, which is the failure every `Limits`
        # field here exists to prevent.
        "docinfo: clip a value without counting that anything was clipped",
        "src/docinfo.rs",
        "    limits.values_clipped += 1;",
        "    limits.values_clipped += 0;",
        "a_value_that_would_fill_the_dialog_is_clipped_and_counted",
    ),
    Mutation(
        # Read the four claimed strings off the *field* rather than off the
        # signature. They live in `/V`, so every one comes back empty and the
        # dialog shows a signature nobody signed with no reason and no date.
        "docinfo: read a signature's claims off the field, not the signature",
        "src/docinfo.rs",
        '    out.reason = text(sig, b"Reason");',
        '    out.reason = text(field, b"Reason");',
        "what_the_signer_says_is_carried_through_unchanged",
    ),
]
