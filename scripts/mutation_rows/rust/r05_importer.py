"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

#: The join between the redaction and the engine: which words a page has, how big
#: to render them, what image the engine is shown, and what a verdict is called.
#: Every one of these decides whether a region can be *certified*, which is the
#: one answer `docs/PLAN.md` §6 says must never be given by default.
ROWS += [
    Mutation(
        # Drop the flush after the loop. A page's last word never leaves the
        # accumulator, so a one-word page yields nothing and the chooser refuses
        # a page that had a perfectly good control on it.
        "gate: forget the word the page ends on",
        "src/ocr_gate.rs",
        "    flush(&mut out, &mut text, &mut rect);\n    out\n}",
        "    let _ = (&mut text, &mut rect);\n    out\n}",
        "a_word_at_the_very_end_of_the_page_is_not_lost",
    ),
    Mutation(
        # Let a character PDFium gave no box for into the geometry. Its four
        # zeroes then pull the word's box to the page corner, and the control is
        # cropped from somewhere the word is not.
        "gate: place a word from a character that has no box",
        "src/ocr_gate.rs",
        "        if box_ == [0.0; 4] || !box_.iter().all(|v| v.is_finite()) {\n            continue;\n        }",
        "        if !box_.iter().all(|v| v.is_finite()) {\n            continue;\n        }",
        "a_character_with_no_box_stays_in_the_text_and_out_of_the_geometry",
    ),
    Mutation(
        # Drop every word the removal names, covered ones included. The covered
        # words are what set the size the control must be no easier than, so the
        # chooser then measures against a bigger box and picks a control that
        # proves less --- `docs/TRAPS.md`'s *a control that is easier than the
        # check certifies nothing*, arriving through the filter in front of it.
        "gate: let the survivor filter take the covered words too",
        "src/ocr_gate.rs",
        "            regions.iter().any(|r| crate::objects::overlaps(w.rect, *r))\n                || !gone.iter().any(|g| *g == w.text)",
        "            !gone.iter().any(|g| *g == w.text)",
        "a_word_a_region_covers_is_kept_even_when_the_removal_names_it",
    ),
    Mutation(
        # Take whatever scale the control's size asks for. A 40 pt heading then
        # renders below 1x and a 1 pt one at 16x, and the probe image is either
        # unreadable or too big to hand over.
        "gate: render at whatever scale the control asks for",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28: the unclamped half of this rule became
        # `scale_wanted`, so a measurement could ask what a control would have
        # needed. The defect is the same one -- drop the clamp.
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE);",
        "    let mut scale = scale_wanted(size_pt);",
        "the_scale_never_leaves_its_bounds",
    ),
    Mutation(
        # Never reduce. A probe image past the mapping is then refused outright
        # rather than rendered at a scale that fits, so a large page loses its
        # verification for a reason that had a remedy.
        "gate: refuse a big probe image rather than shrinking it",
        "src/ocr_gate.rs",
        "    while scale > MIN_SCALE && bytes_at(width_pt, probe_height_pt, scale) > capacity {\n        scale = (scale * 0.5).max(MIN_SCALE);\n    }",
        "    while false {\n        scale = (scale * 0.5).max(MIN_SCALE);\n    }",
        "a_probe_image_that_will_not_fit_at_the_chosen_scale_is_rendered_smaller",
    ),
    Mutation(
        # Butt the two strips together. This is the defect that shipped for an
        # afternoon: with no gap the engine read `quartz,` as `auartz,` on
        # `text-base14` and the gate refused a redaction that was fine.
        "gate: stack the strips with nothing between them",
        "src/ocr_gate.rs",
        "    let gap = (SEPARATION_PT * scale).round().max(2.0) as u32;",
        "    let gap = 0u32;",
        "the_two_strips_never_touch",
    ),
    Mutation(
        # Put the band's edge at the control's first row instead of in the middle
        # of the gap. `Control::contains` tests a centre and an engine's box is a
        # detection rather than a measurement, so a control reported a point high
        # then counts as a survivor and a clean region is reported legible.
        "gate: put the band edge at the control rather than in the gap",
        "src/ocr_gate.rs",
        "    let edge = margin + under + gap / 2;",
        "    let edge = margin + under + gap;",
        "the_band_edge_sits_in_the_middle_of_the_gap_not_at_the_control",
    ),
    Mutation(
        # Accept a strip that is not whole rows. The row count is then wrong, so
        # the band lands somewhere else in the image and the partition decides the
        # verdict: a survivor counted as the control certifies the region.
        "gate: accept a strip that is not whole rows",
        "src/ocr_gate.rs",
        "        if strip.is_empty() || strip.len() % stride != 0 {",
        "        if false {",
        "a_strip_that_is_not_whole_rows_is_refused_before_it_is_stacked",
    ),
    Mutation(
        # Say nothing about a verdict that did not certify. `Applied::why` is
        # empty exactly when `verified`, so a region the gate could not check is
        # then reported to the reader as a clean redaction.
        "gate: stay silent about a region that could not be checked",
        "src/ocr_gate.rs",
        '        Legibility::NotVerified { why, .. } => Some(said!(',
        '        Legibility::NotVerified { why, .. } => (false).then(|| said!(',
        "a_not_verified_verdict_carries_its_own_reason_through",
    ),
    Mutation(
        # Let the error path drift from the rule it duplicates. `judge` builds its
        # own `NotVerified` when no engine answered, rather than fabricating an
        # `EngineId` for `adjudicate` to ignore --- which is a second *caller* of
        # rule 1 and must not become a second copy of it.
        "gate: let the engine-failure reason drift from adjudicate's",
        "src/ocr_gate.rs",
        "    Legibility::NotVerified {\n        why: format!(\"{e}\"),\n        cause: NotVerifiedCause::EngineError,\n        evidence: None,\n    }\n}",
        "    Legibility::NotVerified {\n        why: format!(\"the engine failed: {e}\"),\n        cause: NotVerifiedCause::EngineError,\n        evidence: None,\n    }\n}",
        "the_error_path_says_what_adjudicate_would",
    ),
    Mutation(
        # Choose the render scale from the smallest box a region covered rather
        # than from the control word the engine is actually shown. This is the
        # code as it stood until 2026-08-28, and it is invisible from every
        # verdict: the gate still runs, still refuses, and still gives its
        # reason. What changes is that 34 of 38 unverifiable regions in a
        # 40-document corpus were shown a control below `MIN_CONTROL_PX` -- the
        # floor this very call exists to clear.
        "gate: scale the probe image from the covered box, not the control",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28 when this call gained a `map_err` to carry its
        # own cause. The defect is unchanged -- size the render from the box
        # rather than from the control the engine is shown.
        "let scale = scale_for(control_pt, page.width_pt, height_pt, capacity)",
        "let scale = scale_for(choice.size_pt, page.width_pt, height_pt, capacity)",
        "the_scale_clears_the_floor_for_the_control_and_not_for_the_box",
    ),
    Mutation(
        # Raise the scale by dropping the floor's clamp instead of by measuring
        # the right height. The test above passes either way; only the control
        # beside it, which requires an 8 pt control to stay at exactly 2x, can
        # tell a fix from a blanket magnification.
        "gate: reach the floor by raising every scale rather than the right one",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28, same split as above.
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE);",
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE) * 2.0;",
        "a_control_no_smaller_than_its_box_is_scaled_the_same_as_before",
    ),
    Mutation(
        # Report every way of failing to choose a control as the same cause. The
        # sentence a reader sees is unchanged, so nothing in the application
        # differs -- and the measurement that decides the next increment loses
        # the split it was built to make: 90% of the gate's refusals are this
        # one refusal, and which of its four reasons fired is the whole question.
        "ocr: collapse the four control refusals onto one cause",
        "src/ocr.rs",
        "            NotVerifiedCause::ControlAllLarger,",
        "            NotVerifiedCause::ControlTooShort,",
        "a_word_one_point_taller_than_what_went_does_not",
    ),
    Mutation(
        # Give two causes the same label. A report keyed by it then shows one row
        # where there were two, its total still adds up, and the step that
        # stopped being reported reads exactly like a step that stopped failing.
        "ocr: label two causes the same",
        "src/ocr.rs",
        "            Self::Stack => \"strips would not stack\",",
        "            Self::Stack => \"region is not on the page\",",
        "every_cause_has_its_own_label",
    ),
    Mutation(
        # The copy-paste a hand-written list invites: one cause listed twice and
        # another not at all. The array is fixed-length, so the count still
        # compiles -- and the missing cause's row is never printed, which is not
        # the same as being printed as a zero.
        "ocr: list one cause twice in ALL and drop another",
        "src/ocr.rs",
        "        Self::Stack,\n        Self::EngineError,",
        "        Self::Stack,\n        Self::Stack,",
        "all_lists_every_cause_once",
    ),
    Mutation(
        # Return a row range for a rectangle that is off the page. It is empty, so
        # the engine is handed no rows, reads nothing, and reading nothing is the
        # answer that certifies.
        "gate: hand back an empty row range for an area off the page",
        "src/ocr_gate.rs",
        "    (bottom > top).then_some((top, bottom - top))",
        "    Some((top, bottom.saturating_sub(top)))",
        "a_rectangle_entirely_off_the_page_covers_no_rows_at_all",
    ),
]

#: The OCR worker's own guards. Its input is a pipe and a shared mapping, so
#: every one of these is about numbers that arrived from somewhere else.
ROWS += [
    Mutation(
        # Do the size arithmetic in u32, where 65536 x 65536 x 4 is ZERO. Every
        # length comparison below it then passes and the engine is handed an
        # empty slice described as four gigapixels.
        "ocrworker: size a frame with u32 arithmetic",
        "src/ocr_worker.rs",
        "    let pixels = usize::try_from(width)\n        .ok()?\n        .checked_mul(usize::try_from(height).ok()?)?;\n    pixels.checked_mul(4)",
        "    Some(width.wrapping_mul(height).wrapping_mul(4) as usize)",
        "a_four_gigapixel_frame_is_a_number_too_big_rather_than_zero",
    ),
    Mutation(
        # Resolve any engine name to the first one this build knows. A child from
        # a different build then reports an identity that is not its own, and
        # nothing downstream can invalidate a recognition against an engine that
        # never ran.
        "ocrworker: resolve any engine name to a known one",
        "src/ocr_worker.rs",
        "            .find(|known| **known == self.name)",
        "            .find(|known| !known.is_empty())",
        "an_unknown_engine_name_is_refused_and_named_in_the_refusal",
    ),
    Mutation(
        # Make the reply untagged. `docs/TRAPS.md` records untagged silently
        # swapping two variants of the same shape; here a bare payload becomes a
        # legal reply, which is the encoding this one is chosen against.
        "ocrworker: make the reply untagged",
        "src/ocr_worker.rs",
        "pub enum Said {",
        "#[serde(untagged)]\npub enum Said {",
        "a_bare_payload_is_not_a_reply",
    ),
    Mutation(
        # Refuse an image exactly as large as the buffer. Nothing is unsafe and
        # the largest region a reader can redact stops being verifiable, silently
        # -- the reader is told the file could not be proved clean.
        "ocrworker: refuse an image exactly the size of the buffer",
        "src/ocr_worker.rs",
        "    if pixels.rgba.len() > capacity {",
        "    if pixels.rgba.len() >= capacity {",
        "an_image_larger_than_the_buffer_is_refused_before_it_is_copied",
    ),
    Mutation(
        # Copy an image in without checking it is its own dimensions. The child
        # then reads a frame whose length its own numbers imply and whose content
        # is somebody else's previous request.
        "ocrworker: copy in an image that is not its own dimensions",
        "src/ocr_worker.rs",
        "    if !pixels.is_consistent() {",
        "    if false {",
        "an_image_that_is_not_its_own_dimensions_is_refused_before_it_is_copied",
    ),
    Mutation(
        # Refuse a frame exactly as large as the mapping, in the child this time.
        # The two guards are separate on purpose and so are the mutations: one is
        # about what the parent copies in and one about what the child reads out.
        "ocrworker: refuse a frame exactly the size of the mapping",
        "src/ocr_worker.rs",
        "    if len > mapping.len() {",
        "    if len >= mapping.len() {",
        "a_frame_exactly_as_large_as_the_mapping_fits",
    ),
]

# The guards added on 2026-08-28, when an outside review found that the increment
# which taught the rewrite to preserve encryption had not been carried to four of
# its neighbours. Each of these was run before it was written down; none is here
# on the strength of an argument.
ROWS += [
    Mutation(
        # Take the password off the reload of the merge's own base. `lopdf`
        # answers `Ok` with nothing parsed for a document it cannot authenticate,
        # so the merge fails at the catalog and blames this module's writer.
        "save: reload the merge base without the key that opened it",
        "src/save.rs",
        "            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    // Not a refusal a reader can act on",
        "            ..Default::default()\n        },\n    )\n    // Not a refusal a reader can act on",
        "a_merge_whose_base_is_password_protected_keeps_its_encryption",
    ),
    Mutation(
        # Leave the merged document in the clear. Same class as the rewrite's own
        # encryption mutation and a different site: the incoming-file refusal
        # exists to stop exactly this, and the base can do it too.
        "save: write a merge of an encrypted base in the clear",
        "src/save.rs",
        "    if let Some(state) = &encryption {\n        merged.encrypt(state)",
        "    if let Some(state) = &encryption.filter(|_| false) {\n        merged.encrypt(state)",
        "a_merge_whose_base_is_password_protected_keeps_its_encryption",
    ),
    Mutation(
        # Print without the reader's key. The parse in front of the encryption
        # refusal then fails first, and the reader is told to open the document
        # with the password it is already open with.
        "save: check a print job without the password that opened the document",
        "src/save.rs",
        "    let checked = checked(original, plan, job.view(), password, inputs)?;",
        "    let checked = checked(original, plan, job.view(), None, inputs)?;",
        "a_print_job_from_a_locked_document_names_the_escape_that_exists",
    ),
    Mutation(
        # Let the verifier call a file clean when it decoded none of it. The
        # needle is compared against an empty graph, `found` stays empty, and the
        # verdict is `Verified` about bytes nothing read.
        "verify: verify a file whose encryption was never opened",
        "src/verify.rs",
        "            if doc.is_encrypted() {",
        "            if false {",
        "a_scan_that_decoded_no_object_is_not_verified",
    ),
    Mutation(
        # The same claim for the other subject: a document that parses cleanly
        # and holds nothing. Two rules, two fixtures --- the encrypted one leaves
        # ONE object behind, so it never reaches this arm.
        "verify: verify a file that parsed to no objects at all",
        "src/verify.rs",
        "            } else if doc.objects.is_empty() {",
        "            } else if false {",
        "a_scan_that_decoded_no_object_is_not_verified",
    ),
    Mutation(
        # Stage beside the name rather than beside the document. On a link whose
        # target is in another directory the temporary file lands on a filesystem
        # the rename may not be able to cross.
        "save: stage a rewrite beside the link rather than the document",
        "src/save.rs",
        "    let target = resolved(source);",
        "    let target = source.to_path_buf();",
        "saving_in_place_through_a_symlink_edits_the_document_the_link_names",
        only_on="macos",
    ),
    Mutation(
        # Rename onto the link. It becomes an ordinary file holding the new
        # bytes, and the document it named keeps the old ones --- so a page turn
        # and a highlight, on one file, end up in two different places.
        "save: commit a rewrite onto the link rather than the document",
        "src/save.rs",
        "    commit(staged, &resolved(source))",
        "    commit(staged, source)",
        "saving_in_place_through_a_symlink_edits_the_document_the_link_names",
        only_on="macos",
    ),
    Mutation(
        # Let the replacement take the umask's mode. A document kept at 0600 in a
        # shared directory comes back readable by everyone, with correct bytes and
        # the right page count.
        "save: give the replacement the umask's mode rather than the document's",
        "src/save.rs",
        "        #[cfg(unix)]\n        if let Ok(existing) = std::fs::metadata(out) {\n            use std::os::unix::fs::PermissionsExt;\n            let mut permissions = existing.permissions();\n            permissions.set_mode(permissions.mode() & !0o111);\n            let _ = file.set_permissions(permissions);\n        }\n",
        "",
        "a_rewrite_keeps_the_documents_mode",
        only_on="macos",
    ),
    Mutation(
        # Time the read-back out without ending the worker. The refusal is still
        # correct; the process and the thread blocked on its pipe are leaked.
        "save: let a timed-out read-back leave its worker running",
        # `awaited` moved to `save_outside.rs` on 2026-08-31 with the rest of
        # `InWorker`'s behaviour, and the call it names lost its `crate::workers::`
        # prefix in the move, because naming that module in the import header is
        # the whole point of the move. Both halves of the anchor had to follow.
        # Re-aimed 2026-10-09: the kill became the argument `awaited` hands to
        # `awaited_then`, so that a test can stand in for it.
        "src/save_outside.rs",
        "    awaited_then(rx, within, RELEASED_WITHIN, || kill_pid(pid))",
        "    awaited_then(rx, within, RELEASED_WITHIN, || ())",
        "a_read_back_that_never_answers_ends_the_worker",
        only_on="macos",
    ),
    Mutation(
        # Make the bound a thousand times longer, which is what having no bound
        # looks like from inside a test. Killed by the *upper* assertion only ---
        # a lower bound is satisfied by any longer wait, and this survived until
        # that assertion existed.
        "save: bound the read-back at a thousand times the deadline",
        # Moved with `awaited`. The line itself is unchanged --- it names nothing
        # outside the function --- so only the file it lives in moved.
        "src/save_outside.rs",
        "    match rx.recv_timeout(within) {",
        "    match rx.recv_timeout(within * 1000) {",
        "a_read_back_that_never_answers_ends_the_worker",
        only_on="macos",
    ),
]

# `redact::aggregate`, extracted from the Tauri command layer on 2026-08-28. The
# arithmetic did not change; what changed is that a test can reach it. Every one
# of these five was unreachable by any mutation while the code was a loop body
# inside a `#[tauri::command]`'s private helper.
ROWS += [
    Mutation(
        # Image-only selections used to report zero despite removing image draws.
        "redact: omit image draws from the removal count",
        "src/redact.rs",
        "    total += images.len();",
        "    total += 0;",
        "image_draws_are_merged_and_counted_without_a_text_layer",
    ),
    Mutation(
        # Stop merging the operators two overlapping regions both name. The
        # reader is then told a page has more removals in it than it has.
        "redact: count one show operator once per region that names it",
        "src/redact.rs",
        # Re-aimed 2026-10-07 when the cuts were merged between these lines.
        "    shows.sort_unstable();\n    shows.dedup();\n    // A show one region",
        "    shows.sort_unstable();\n    // A show one region",
        "two_regions_over_one_line_are_one_removal",
    ),
    Mutation(
        # The same, one level down, inside a form. Its own mutation because it is
        # its own statement -- a fixture exercising both at once could not say
        # which of them a survivor had broken.
        "redact: count a form's show operator once per region that names it",
        "src/redact.rs",
        "    form_shows.sort_unstable();\n    form_shows.dedup();\n    total",
        "    form_shows.sort_unstable();\n    total",
        "text_inside_a_form_is_merged_and_counted_too",
    ),
    Mutation(
        # Report the model's page number rather than the reader's, which sends
        # somebody to look at the wrong sheet for an object that could not go.
        "redact: name a concern's page zero-based",
        "src/redact.rs",
        'format!("page {}: {}", page + 1, object.sentence())',
        'format!("page {}: {}", page, object.sentence())',
        "an_object_that_cannot_be_removed_names_the_readers_page_number",
    ),
    Mutation(
        # Let a region that takes no text contribute an empty needle. An empty
        # string is present in every file ever written, so the verification then
        # reports a leak on a redaction that is perfectly clean.
        "redact: take an empty string as something to verify",
        "src/redact.rs",
        "        if !taking.is_empty() {",
        "        if true {",
        "a_region_that_takes_no_text_adds_no_needle",
    ),
    Mutation(
        # Hand the OCR gate the plan's rectangles, which are in the page's own
        # space, instead of the reader's, which are in display space. Every
        # control then gets looked for somewhere else on the page.
        "redact: give the gate the page-space regions rather than the display ones",
        "src/redact.rs",
        "        regions: displayed,",
        "        regions: areas.clone(),",
        "the_gate_gets_the_display_space_regions_rather_than_the_plans",
    ),
]

# `pagetree::materialise`, the one page-selection sequence both writers use,
# extracted on 2026-08-28. `save::rewrite` and `print::build` each had their own
# copy, spelled differently, and `docs/TRAPS.md` records two shipped defects that
# came from the pair drifting.
ROWS += [
    # "Leave the outline in place after a deletion" stood here until 2026-10-07.
    # The outline is repaired now rather than dropped, and the same edit is
    # `bookmarks: do not repair the outline at all` at the end of this file.
    Mutation(
        # Drop the outline unconditionally, which takes every bookmark out of a
        # document whose pages were merely rearranged --- and they would all still
        # have pointed at a page.
        "pagetree: drop the outline for a move as well as a deletion",
        "src/pagetree.rs",
        "    if !dropped.is_empty() {\n        // Before the pages go:",
        "    drop_outline(doc)?;\n    if !dropped.is_empty() {\n        // Before the pages go:",
        "materialising_a_move_keeps_the_outline",
    ),
    Mutation(
        # Reorder even when nothing moved. `reorder_pages` flattens the tree, so
        # every page loses what it inherited from the node it hung under --- for a
        # job that changed nothing.
        "pagetree: flatten the page tree for a plan that moved nothing",
        "src/pagetree.rs",
        "    if let Some(order) = reorder_to {\n        reorder_pages(doc, order)?;\n    }",
        "    reorder_pages(doc, reorder_to.unwrap_or(&doc.get_pages().into_values().collect::<Vec<_>>()))?;",
        "materialising_nothing_leaves_the_tree_alone",
    ),
    Mutation(
        # Ignore the anchor. Every inserted page lands at the front, which on a
        # one-page document is the same place it belonged and on any other is
        # not --- and a count of pages agrees either way.
        "docmodel: put an inserted page at the front whatever it was anchored to",
        "src/docmodel.rs",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let at = 0;",
        "an_inserted_page_lands_behind_its_anchor_and_shows_nothing",
    ),
    Mutation(
        # Off by one the other way: the page lands in front of the one it was
        # anchored to. `Command::Move` has the identical mutation for the
        # identical reason, and both look like a drag landing one row out.
        "docmodel: put an inserted page in front of its anchor rather than behind it",
        "src/docmodel.rs",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor),\n                };",
        "an_inserted_page_lands_behind_its_anchor_and_shows_nothing",
    ),
    Mutation(
        # Accept a page of no area at the entry point. The apply below still
        # refuses it, so the *refusal* is unchanged --- what changes is that the
        # id was spent on the way, which only the "spends no id" half can see.
        "docmodel: issue a page's id before checking that it has any area",
        "src/docmodel.rs",
        "        if !size.is_proper() {\n            return Err(Refusal::DegeneratePage(size));\n        }\n        if let Some(anchor) = after {",
        "        if let Some(anchor) = after {",
        "a_page_enclosing_no_area_is_refused_and_spends_no_id",
    ),
    Mutation(
        # And in the apply, which is the guard the public `Command::Insert`
        # meets. Its neighbour in `Doc::insert` refuses the same input first, so
        # only a test that goes through `Doc::apply` can see this one.
        "docmodel: let the apply take a page of no area",
        "src/docmodel.rs",
        "                if !size.is_proper() {\n                    return Err(Refusal::DegeneratePage(size));\n                }",
        "                if false {\n                    return Err(Refusal::DegeneratePage(size));\n                }",
        "a_page_of_no_area_is_refused_by_the_apply_as_well_as_by_the_entry_point",
    ),
    Mutation(
        # Number a made page from 1, as the baseline pages are. The second page
        # of the file and the first page tpdf makes then share an id, and every
        # command naming either reaches whichever the table happens to hold.
        "docmodel: issue a made page's id from the same run as the file's own",
        "src/docmodel.rs",
        "            next_page: u64::from(pages) + 1,",
        "            next_page: 1,",
        "a_made_page_is_never_given_a_baseline_page_s_id",
    ),
    # **Deleted 2026-08-30: "docmodel: let a mark onto a page tpdf made".** It
    # watched a guard that has been removed on purpose --- a mark on a page tpdf
    # made is written now, and the addressing that made it impossible is gone.
    # Deleted rather than re-aimed, for the reason `docs/TRAPS.md` records about
    # a seam that removes a class of defect: there is no guard left for a
    # mutation to delete. What replaced its coverage is `save: resolve a mark
    # against the file's pages rather than the plan's`, below, which is the
    # defect the old guard existed to make unreachable.
    Mutation(
        # And a redaction, which puts a row in the review list certifying the
        # removal of nothing from a page that has nothing on it.
        "docmodel: let a redaction onto a page tpdf made",
        "src/docmodel.rs",
        "                PageSource::Blank(_) => {\n                    return Err(Refusal::RedactionOnMadePage(redaction.page));\n                }",
        "                PageSource::Blank(_) => {}",
        "a_page_tpdf_made_takes_a_mark_and_no_redaction",
    ),
    Mutation(
        # Resolve a mark against the file's own pages rather than the plan's.
        # This is the addressing that made a mark on an inserted page impossible
        # --- a made page is in no file, so it has no number here at all. On a
        # plan that inserts, the mark's position names a page the baseline list
        # does not have and the save is refused; on one that only moves, it
        # names a different page and the mark is written onto it.
        "save: resolve a mark against the file's pages rather than the plan's",
        "src/save.rs",
        "    let mut sites = mark_sites(&doc, &order, &plan.marks)?;",
        "    let mut sites = mark_sites(&doc, &pages, &plan.marks)?;",
        "a_mark_on_a_page_tpdf_made_is_written_onto_that_page",
    ),
    Mutation(
        # Put the baseline page number in the plan instead of the position, which
        # is what this field held until the addressing was widened. Every plan
        # that is the file agrees with itself, so the only thing that can see it
        # is a plan whose pages have moved --- and the 0 for a page tpdf made is
        # the other half: it would put every mark on an inserted page onto the
        # first page of the document.
        "edits: address a mark by its baseline page rather than its position",
        "src/edits.rs",
        "            let at = u32::try_from(at).unwrap_or(u32::MAX);",
        "            let at = match view.source {\n                PageSource::Baseline(number) => number,\n                PageSource::Blank(_) | PageSource::Imported { .. } => 0,\n            };",
        "a_marks_reply_names_the_page_by_identity_and_the_plan_by_position",
    ),
    Mutation(
        # And a crop, which the frontend cannot even measure for such a page ---
        # so the model would be holding a box nothing could have produced.
        "docmodel: let a crop onto a page tpdf made",
        "src/docmodel.rs",
        "                    if matches!(page_now.source, PageSource::Blank(_)) && to.is_some() {",
        "                    if false {",
        "a_page_tpdf_made_takes_no_crop_and_clearing_one_is_not_an_error",
    ),
    Mutation(
        # Refuse clearing one as well. The asymmetry is deliberate: a made page
        # has no crop to clear, so this refuses an operation with no effect ---
        # and the refusal is what a reader sees.
        "docmodel: refuse clearing a crop a made page never had",
        "src/docmodel.rs",
        "                    if matches!(page_now.source, PageSource::Blank(_)) && to.is_some() {",
        "                    if matches!(page_now.source, PageSource::Blank(_)) {",
        "a_page_tpdf_made_takes_no_crop_and_clearing_one_is_not_an_error",
    ),
    Mutation(
        # Read a page tpdf made as the file's own page at that slot. The plan
        # then claims to be the document on disk, and a save takes the append
        # path --- which cannot create a page, so the insert is silently absent
        # from a file the reader was told was written.
        "edits: read a page tpdf made as the file's own page at that slot",
        "src/edits.rs",
        "                    PageSource::Blank(_) => false,\n                    PageSource::Imported { .. } => false,",
        "                    PageSource::Blank(_) => true,\n                    PageSource::Imported { .. } => false,",
        "a_deletion_and_an_insert_together_are_still_not_the_file_on_disk",
    ),
    Mutation(
        # Leave an inserted page out of the reason to rebuild the tree. The page
        # object is created and never linked into `/Kids`, so the file comes out
        # with the pages it had and an orphan the sweep may or may not take.
        "save: do not rebuild the page tree for an inserted page",
        "src/save.rs",
        "    let moved =\n        baselines.len() != plan.pages.len() || baselines.windows(2).any(|two| two[0] >= two[1]);",
        "    let moved = baselines.windows(2).any(|two| two[0] >= two[1]);",
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Make the page with no media box. It has no size, so every reader
        # guesses one --- and they do not all guess the same.
        "save: make a blank page with no media box",
        "src/save.rs",
        '                (\n                    "MediaBox",',
        '                (\n                    "NotABox",',
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Give it the size of nothing. A zero media box is a page every reader
        # renders as empty and several refuse, and the dictionary still has
        # every key a check counting them would look for.
        "save: make a blank page of no size",
        "src/save.rs",
        "                        Object::Real(size.width as f32),\n                        Object::Real(size.height as f32),",
        "                        Object::Real(0.0),\n                        Object::Real(0.0),",
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Spell a plan carrying a made page as the file pages it does have. The
        # list is one entry short, and every consumer of it reads a complete
        # selection of the wrong document.
        "print: list a plan with a page tpdf made as the file pages it does have",
        "src/print.rs",
        "        let PageSource::Baseline(number) = page.source else {\n            return Pages::Unlistable;\n        };",
        "        let PageSource::Baseline(number) = page.source else {\n            continue;\n        };",
        "a_plan_with_a_page_tpdf_made_cannot_be_listed_and_goes_to_the_working_writer",
    ),
]

# --- The importer taking a selection (2026-08-30) ---------------------------
#
# `merge::import` is the piece `docs/PLAN.md` names as carrying over into
# inserting pages from another file. What is new here is the *selection*: the
# whole-document path is `append`, unchanged, and its own tests are the control
# for it. So every mutation below is aimed at a clause that exists only because
# a caller may ask for some of a document rather than all of it.
#
# One clause deliberately has no mutation, and it is worth saying which rather
# than leaving a gap: the `!detached.contains_key(id)` filter that builds
# `left`. A page being taken is in `seen` and in the queue before the walk
# starts, so putting it in the barrier as well changes nothing any test could
# observe --- the filter is there so the set means what its name says, not so
# the walk behaves differently. `docs/TRAPS.md` records an unreachable guard as
# worth keeping and not worth a mutation.
ROWS += [
    Mutation(
        # Detach every page rather than the ones asked for. `detached` is what
        # seeds the walk, so all three pages come across as objects while the
        # answer still names only the one the caller wanted --- a file reporting
        # one page and carrying the text of two others, which is the leak
        # `docs/TRAPS.md` records a shipped extract having.
        "merge: import every page rather than the ones asked for",
        "src/merge.rs",
        "    for &page in &taking {",
        "    for &page in &pages {",
        "only_the_pages_asked_for_come_across",
    ),
    Mutation(
        # Take the wall away. A `/Dest` from a page somebody asked for to a page
        # they did not then imports that page, its resources and its content
        # stream --- the same leak as above, arriving by reference rather than
        # through the seed list, which is why it needs its own mutation.
        "merge: follow a reference into a page nobody asked for",
        "src/merge.rs",
        "            if left.contains(&id) {\n                continue;\n            }\n",
        "",
        "a_page_left_behind_is_not_followed_into",
    ),
    Mutation(
        # Answer in the document's own page order. Every other assertion in the
        # module passes --- the ids are right, the count is right, nothing leaks
        # --- and a caller placing them puts page 3 where it asked for page 1.
        "merge: answer in the document's order rather than the order asked for",
        "src/merge.rs",
        "    taking\n        .iter()\n        .map(|&id| shifted_id(id, by))",
        "    pages\n        .iter()\n        .filter(|id| taking.contains(id))\n        .map(|&id| shifted_id(id, by))",
        "the_answer_is_in_the_order_asked_for",
    ),
    Mutation(
        # Serve a page asked for twice. Two positions then hold one page object,
        # which is the hazard `save::mark_sites` refuses a mark on and
        # `pagetree` refuses a deletion of --- reached here by building such a
        # document rather than by finding one.
        "merge: let one page be asked for twice",
        "src/merge.rs",
        "        if taking.contains(&id) {\n            return Err(said!(\"page {page} was asked for twice\", page = at + 1));\n        }\n",
        "",
        "the_same_page_asked_for_twice_is_refused",
    ),
    Mutation(
        # Take the first page for a position the document does not have. The
        # import succeeds, the answer is the right length, and it names a page
        # nobody asked for.
        "merge: fall back to the first page for a position that is not there",
        "src/merge.rs",
        "        let id = *pages.get(at).ok_or_else(|| {",
        "        let id = *pages.get(at).or(pages.first()).ok_or_else(|| {",
        "a_position_the_document_does_not_have_is_refused",
    ),
]

# The nib a reader picks: the door it arrives through, the two derivations that
# pad by half of it, and the two readers that carry it out.
ROWS += [
    Mutation(
        # Answer the default to everything. Every width a reader picks is thrown
        # away silently: the mark is taken, the file is valid, and the line is
        # the weight it always was. The control in the test is what catches it --
        # every other assertion there is satisfied by a door that clamps
        # correctly and by one that ignores its argument.
        "edits: answer the default nib to every width sent",
        "src/edits.rs",
        "    if value.is_finite() {\n        value.clamp(NIB_MIN, NIB_MAX)",
        "    if true {\n        INK_WIDTH",
        "a_nib_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
    Mutation(
        # Take a non-finite width as sent, which is the fourth door standing
        # open: `1e40` is valid JSON, is `INFINITY` here, and `save.rs` writes it
        # as the three letters `inf` in the middle of a content stream. tpdf
        # would write a file no reader can parse and sign its name to it.
        "edits: let a non-finite nib through the door",
        "src/edits.rs",
        "fn nib(value: f64) -> f64 {\n    if value.is_finite() {",
        "fn nib(value: f64) -> f64 {\n    if true {",
        "a_nib_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
    Mutation(
        # Derive the rectangle from the constant rather than from the mark. At
        # the default nib the two agree exactly, which is why the fixture has to
        # be a second width -- the sibling test that proves the pad exists at all
        # cannot see this.
        "edits: pad a drawing's rectangle by the default nib",
        "src/edits.rs",
        "        let width = nib(want.width);",
        "        let width = INK_WIDTH;",
        "the_rectangle_is_padded_by_half_the_nib_the_reader_chose",
    ),
    Mutation(
        # The same reach for the constant, on the second derivation: `reink` runs
        # after an eraser sweep, so a broad drawing would keep its ink and lose
        # the box around it the first time a reader rubbed one stroke out.
        "docmodel: pad an erased drawing by the default nib",
        "src/docmodel.rs",
        "        let width = self.mark(mark).map_or(INK_WIDTH, |m| m.width);",
        "        let width = INK_WIDTH;",
        "erasing_a_stroke_keeps_the_nib_the_drawing_was_made_with",
    ),
    Mutation(
        # The overlay's copy. PDFium draws the mark inside the tile at the width
        # the file carries and the overlay redraws ink on top from this; a
        # constant here puts the default weight over the reader's, which reads as
        # a rendering fault rather than as a wrong number.
        "edits: report the default nib to the overlay",
        "src/edits.rs",
        "                width: mark.width,\n                note: model.note_of(id).to_string(),",
        "                width: INK_WIDTH,\n                note: model.note_of(id).to_string(),",
        "the_reply_and_the_plan_both_carry_the_nib",
    ),
    Mutation(
        # The writer's copy, and the one that reaches a file. Together with the
        # mutation above this says the two readers are fed separately: a mark
        # that reached one with the chosen nib and the other with the default is
        # a drawing that changes weight the moment it is saved.
        "edits: plan the default nib for the writer",
        "src/edits.rs",
        "                    width: body.width,",
        "                    width: INK_WIDTH,",
        "the_reply_and_the_plan_both_carry_the_nib",
    ),
    Mutation(
        # Write the constant into the appearance stream. Every reader on earth
        # then draws the default, whatever the reader picked and whatever the
        # overlay showed them while they drew it.
        "save: write the default nib rather than the mark's",
        "src/save/marks.rs",
        '        Paint::Path | Paint::Segments => (mark.width, "1 J 1 j "),',
        '        Paint::Path | Paint::Segments => (crate::docmodel::INK_WIDTH, "1 J 1 j "),',
        "the_stroke_width_is_the_nib_the_reader_chose",
    ),
]

# Deleting a comment the file came with: the one edit that cannot be an append,
# the sweep clause that keeps its bytes out of the file, and the refusal replying
# made it need.
ROWS += [
    Mutation(
        # Let a deletion append. An append only adds objects, so the highlight
        # beside it is written, the deletion is dropped, and the save reports
        # success -- and a fixture holding only the deletion cannot see it,
        # because with no marks and no note edits the first clause makes the
        # answer `Rewrite` either way.
        "edits: let a deleted comment be written as an append",
        "src/edits.rs",
        "            && self.discards.is_empty()\n",
        "",
        "a_plan_that_only_adds_marks_is_appended_and_anything_else_is_rewritten",
    ),
    Mutation(
        # Stop sweeping after a deletion. `pagetree::forget` removes the
        # annotation and every reference to it, which leaves its appearance
        # stream -- a drawing of the words the reader deleted -- reachable from
        # nothing and written out regardless. The page draws nothing and every
        # byte is still there, which is the picture's trap in the one place the
        # leftover is text somebody asked to be rid of.
        "save: leave a deleted comment's bytes in the file",
        "src/save.rs",
        "        || discarded > 0\n",
        "",
        "a_deleted_comment_leaves_the_page_and_leaves_no_bytes_behind",
    ),
    Mutation(
        # Take the deletion off the page without removing the object, which is
        # the smaller-looking implementation and is what the redaction path's own
        # comment argues against: a structure element's `/OBJR` or an AcroForm's
        # `/Fields` names the annotation too, so pruning the one list a caller
        # has in mind leaves it alive.
        "save: prune the page's list instead of forgetting the object",
        "src/save/marks.rs",
        "    let taken = doomed.len();\n    crate::pagetree::forget(doc, &doomed).map_err(Refusal::from)?;",
        "    let taken = doomed.len();\n    for id in &doomed {\n        doc.objects.remove(id);\n    }",
        "a_deleted_comment_leaves_the_page_and_leaves_no_bytes_behind",
    ),
    Mutation(
        # Accept a plan naming an object that is not an annotation, which would
        # let a caller delete a font, a page or the catalog out of the file.
        "save: delete whatever object a plan names",
        "src/save/marks.rs",
        "            return Err(said!(\"that comment is not an annotation\").into());\n        }\n        doomed.insert(id);",
        "        }\n        doomed.insert(id);",
        "a_deletion_naming_something_that_is_not_an_annotation_is_refused",
    ),
    Mutation(
        # Let a comment one of the reader's own replies answers be deleted. The
        # reply's `/IRT` then points at nothing, which is a malformed thread in
        # every reader that draws one -- and no reader would see it until the
        # file was reopened somewhere else.
        "docmodel: delete a comment a reply of the reader's own answers",
        "src/docmodel.rs",
        "        if self.answered_by_a_reply(object) {\n            return Err(Refusal::ReplyAnswersIt(object));\n        }\n",
        "",
        "a_comment_a_reply_answers_cannot_be_discarded",
    ),
    Mutation(
        # Count every mark ever made rather than the live ones. A reader who
        # removes their reply is then still refused, for ever, and the refusal's
        # own advice -- delete the reply first -- stops working.
        "docmodel: count removed replies as replies",
        "src/docmodel.rs",
        "        self.now\n            .all_marks()\n            .into_iter()\n            .filter_map(|(_, id)| self.mark(id))",
        "        self.marks\n            .keys()\n            .copied()\n            .filter_map(|id| self.mark(id))",
        "removing_the_reply_makes_the_comment_deletable_again",
    ),
    Mutation(
        # Let a deleted page's discards outlive it. The model then hands the
        # writer a deletion for a comment on a page the plan does not carry.
        "docmodel: keep a deleted page's discards",
        "src/docmodel.rs",
        "                self.discards.retain(|_, on| *on != page);\n",
        "",
        "deleting_a_page_takes_the_discards_on_it_and_no_others",
    ),
    Mutation(
        # Throw the rewrite away when the comment is deleted, which is the
        # tempting single-map implementation. Undoing the deletion then brings
        # the comment back reading what the *file* said rather than what the
        # reader typed.
        "docmodel: drop a comment's edit when it is deleted",
        "src/docmodel.rs",
        "                self.discards.insert(object, page);",
        "                self.rewrites.remove(&object);\n                self.discards.insert(object, page);",
        "a_comment_can_be_rewritten_and_then_discarded_and_undo_keeps_the_edit",
    ),
]

ROWS += [
    Mutation(
        # Step back one entry whatever gesture it belonged to, which is what
        # undo did until 2026-08-30. A sweep of the eraser across five drawings
        # is five commands, so the reader who let go once presses undo five
        # times -- and four comments in four files said otherwise, because a
        # sweep that stays inside one drawing really is one command and is the
        # sweep anyone writing a test reaches for.
        "docmodel: undo one entry whatever gesture it belonged to",
        "src/docmodel.rs",
        """        while let Some(with) = self.grouped(self.cursor) {
            if self.cursor == 0 || self.grouped(self.cursor - 1) != Some(with) {
                break;
            }
            self.cursor -= 1;
        }
""",
        "",
        "one_press_of_undo_puts_back_a_sweep_that_crossed_several_drawings",
    ),
    Mutation(
        # Redo one entry at a time while undo takes a whole gesture. The two
        # counts then disagree about how many presses the reader is from where
        # they were, which is worse than either behaviour on its own.
        "docmodel: redo one entry whatever gesture it belonged to",
        "src/docmodel.rs",
        """        while let Some(with) = self.grouped(self.cursor - 1) {
            if !self.can_redo() || self.grouped(self.cursor) != Some(with) {
                break;
            }
            self.step();
        }
""",
        "",
        "redo_returns_a_whole_sweep_in_one_press",
    ),
    Mutation(
        # Give every ungrouped entry one shared gesture, so that two commands
        # which belong to nothing group with each other. `Option` equality is
        # what makes this reachable: `None == None`, and the property that a
        # command outside a gesture stands alone is carried by the `while let`
        # rather than by any comparison.
        "docmodel: let two commands outside a gesture join each other",
        "src/docmodel.rs",
        "        self.journal[at].sweep\n",
        "        self.journal[at].sweep.or(SweepId::new(u64::MAX))\n",
        "a_command_belonging_to_no_gesture_joins_neither_neighbour",
    ),
    Mutation(
        # Drop the gesture between the door and the model. Every erasure then
        # stands alone, which is the state this whole increment replaced -- and
        # nothing about the reply changes, so only a check that presses undo
        # once and counts what came back can see it.
        "edits: forget which gesture an erasure belonged to",
        "src/edits.rs",
        "        model.apply_in(cmd, sweep).map_err(describe)?;\n",
        "        model.apply(cmd).map_err(describe)?;\n",
        "a_sweep_that_took_two_marks_whole_is_one_undo",
    ),
    Mutation(
        # Read the wire's zero as a gesture. Zero is what every caller but the
        # eraser sends, so this groups unrelated commands into one press of
        # undo -- the failure the non-zero type exists to make unspellable.
        "edits: treat the wire's no-gesture as gesture zero",
        "src/edits.rs",
        "    NonZeroU64::new(value)\n",
        "    Some(NonZeroU64::new(value).unwrap_or(NonZeroU64::MIN))\n",
        "two_erasures_outside_a_gesture_are_two_undos",
    ),
    Mutation(
        # Re-ink outside the gesture while the whole-mark removal stays inside
        # it. A sweep that clipped a drawing and took a highlight then costs two
        # presses, and only a check that crosses both callbacks can see it.
        "edits: re-ink outside the gesture the sweep named",
        "src/edits.rs",
        "            model.reink_in(id, keep, joins).map_err(describe)?;\n",
        "            model.reink(id, keep).map_err(describe)?;\n",
        "a_sweep_that_crossed_two_drawings_is_still_one_undo",
    ),
]

ROWS += [
    Mutation(
        # Read a bottom-up BMP as top-down and the other way round. The image is
        # still the right size, still the right colours and still plausible --- it
        # is upside down, so a mark's rectangle is read from the wrong end of the
        # page and a coverage figure comes back small rather than wrong-looking.
        # A symmetric fixture cannot tell this from an identity, which is why
        # `three_rows` is not symmetric.
        "raster: read the rows in the other direction",
        "src/print_win.rs",
        "        let row = if self.top_down {\n            y\n        } else {\n            self.height - 1 - y\n        };",
        "        let row = if self.top_down {\n            self.height - 1 - y\n        } else {\n            y\n        };",
        "the_top_row_is_the_top_row_whichever_way_the_bmp_stores_it",
        only_on="windows",
    ),
    Mutation(
        # Walk the pixels without the row padding. Three 24-bit pixels are 9 bytes
        # in a 12-byte row, so this drifts three bytes further into the image on
        # every row and reads padding as colour -- which on a mostly-white page is
        # a small plausible count rather than an obvious error.
        "raster: ignore the four-byte row padding",
        "src/print_win.rs",
        "        let stride = (width * bytes_per_pixel).div_ceil(4) * 4;",
        "        let stride = width * bytes_per_pixel;",
        "a_row_is_read_past_the_padding_that_follows_it",
        only_on="windows",
    ),
    Mutation(
        # Take the bounds check off `pixel`. A rectangle that overhangs the page is
        # a question a check may legally ask, and this turns the answer into a
        # panic -- so a check's failure becomes a crash rather than a reading.
        "raster: let a pixel outside the image be read anyway",
        "src/print_win.rs",
        "        if x >= self.width || y >= self.height {\n            return [0xFF, 0xFF, 0xFF];\n        }",
        "        if false {\n            return [0xFF, 0xFF, 0xFF];\n        }",
        "a_pixel_outside_the_image_reads_white_rather_than_panicking",
        only_on="windows",
    ),
    Mutation(
        # Accept an indexed image. Its bytes are palette *indices*, so every
        # comparison downstream would be comparing index numbers as if they were
        # colours -- and two different colours with adjacent indices would read as
        # nearly the same pixel.
        "raster: compare an indexed image without its palette",
        "src/print_win.rs",
        "        if !(bpp == 24 || bpp == 32) {",
        "        if false {",
        "an_indexed_bmp_is_refused_rather_than_compared_without_its_palette",
        only_on="windows",
    ),
]

ROWS += [
    Mutation(
        # Take the changed-file guard off the print path. The job is then built
        # from whatever is on disk now, with the reader's marks placed at
        # coordinates measured on the file still on their screen -- exactly the
        # defect the guard was written for, and the paper does not say so.
        "save: print a file that changed since it was opened",
        "src/save.rs",
        "    print_ready(source, plan)?;\n",
        "    let _ = print_ready(source, plan);\n",
        "a_print_job_over_a_changed_file_is_refused_or_says_so",
    ),
    Mutation(
        # The over-refusal direction: make `print_ready` refuse every job,
        # changed file or not. The guard letting an unchanged file through is
        # the half of it a reader relies on every day, and only the control can
        # see it go.
        "save: refuse a print job whether or not the file changed",
        "src/save.rs",
        "        .map(|_ready| ())\n",
        "        .and_then(|_ready| Err(Refusal::changed(\"changed on disk since you opened it\")))\n",
        "a_print_job_over_a_changed_file_is_refused_or_says_so",
    ),
    Mutation(
        # Let the umask choose the tile mapping's backing-file permissions
        # again. On a machine with the default umask 022 the file is briefly
        # 0644 in the shared temp directory; the test reads the mode through
        # the descriptor, because the path is unlinked on the next statement.
        "worker_shm: let the umask choose the backing file's permissions",
        "src/worker_shm.rs",
        "            .mode(0o600)\n",
        "",
        "the_backing_file_of_a_new_mapping_is_not_readable_by_anyone_else",
        only_on="macos",
    ),
]

# --- the print refusal, as the window receives it ------------------------
ROWS += [
    Mutation(
        # Flatten the changed-file refusal back to its sentence, which is what
        # `print_document` did until 2026-09-01. It compiles, it refuses the
        # print, and the message the reader sees is word for word the right one
        # -- only the flag that lets the window put Save a copy and Reload
        # beside it is gone. Nothing but an assertion on the flag can see this:
        # `docs/TRAPS.md`, "A refusal flattened to a string across a process
        # boundary loses the action that answers it".
        "print: flatten the print refusal to its message again",
        "src/commands/print.rs",
        "            save::print_ready(source, plan)?;",
        "            save::print_ready(source, plan)\n"
        "                .map_err(|refused| save::Refusal::from(refused.message))?;",
        "a_print_of_a_replaced_file_rejects_with_the_flag_that_offers_reload",
    ),
    Mutation(
        # Rename the wire field the window branches on. The rejection is still
        # a two-field object carrying the right sentence and the right value,
        # and the frontend reads a name that is not there as absent -- so the
        # reader is told their file moved and offered nothing to do about it.
        # A rename is what a refactor of this struct looks like, which is why
        # the assertion is against a literal rather than a round trip.
        "save: rename the refusal field the window reads",
        "src/save.rs",
        "    /// The source changed on disk since the reader opened it.\n    pub changed: bool,",
        "    /// The source changed on disk since the reader opened it.\n"
        '    #[serde(rename = "fileChanged")]\n'
        "    pub changed: bool,",
        "the_wire_shape_of_a_refusal_is_a_message_and_a_changed_flag",
    ),
]

# --- the redaction read-back, the last parse to leave the coordinator -----
ROWS += [
    Mutation(
        # Scan in the coordinator, which is what every redaction did until
        # 2026-09-01. The report is identical for any file that parses, so no
        # comparison of reports can see this -- only an assertion on WHO was
        # asked, over bytes this process could not have produced that answer
        # from.
        "verify: scan the written file in the coordinator instead of the verifier",
        "src/commands/redact.rs",
        "    scanning.scan(&mut file, len, needles, password)",
        "    let _ = scanning;\n"
        "    let mut bytes = Vec::new();\n"
        "    std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|e| e.to_string())?;\n"
        "    Ok(verify::scan(&bytes, needles, password))",
        "the_redaction_read_back_does_not_parse_the_file_it_wrote",
    ),
    Mutation(
        # Answer an empty report for a file that could not be opened. Every
        # field is zero, `found` is empty, and `Verdict::Verified` follows --
        # so a redaction whose output vanished between the rename and the read
        # would be certified clean. This is the exact shape `docs/PLAN.md` 6
        # forbids, and it is one `?` away at all times.
        "verify: certify a file that could not be opened at all",
        "src/commands/redact.rs",
        '    let mut file = std::fs::File::open(at)\n        .map_err(|why| said!("the redacted file could not be read back: {why}"))?;',
        '    let Ok(mut file) = std::fs::File::open(at) else {\n        return Ok(verify::Report::default());\n    };',
        "a_read_back_of_a_file_that_is_not_there_is_an_error",
    ),
    Mutation(
        # Take the cap off one arm. The report then carries a reason per object
        # and the reply it travels in stops fitting -- which the reader meets as
        # a verification that FAILED rather than as a file with a lot wrong with
        # it. Only the arm the fixture reaches is mutated: the three are
        # deliberately separate anchors, since a single one would be ambiguous.
        "verify: list every unrecognised object rather than the bounded number",
        "src/verify.rs",
        "                    Carrier::Unrecognised { filter } => {\n"
        "                        blind_objects += 1;\n"
        "                        if blind_objects <= MAX_OBJECT_REASONS {",
        "                    Carrier::Unrecognised { filter } => {\n"
        "                        blind_objects += 1;\n"
        "                        if blind_objects <= usize::MAX {",
        "a_report_lists_a_bounded_number_of_objects_and_counts_the_rest",
    ),
    Mutation(
        # Say nothing was left over. The list is still bounded and the verdict
        # is still a refusal, so every check but the arithmetic passes -- and a
        # reader is told a thousand objects are unaccounted for when the number
        # is twelve hundred. A cap that does not report what it dropped is a cap
        # that hides the worst files best.
        "verify: report nothing left over when the object list is shortened",
        "src/verify.rs",
        '                    count = blind_objects - MAX_OBJECT_REASONS',
        '                    count = 0',
        "a_report_lists_a_bounded_number_of_objects_and_counts_the_rest",
    ),
]

# --- the ordering, the bounds and the handle a save's guards rest on ---------
ROWS += [
    Mutation(
        # Send the answer and release the worker afterwards. Every path through
        # `InWorker` hands its worker a mapping of the file the coordinator is
        # about to resize: on Windows a file with a section open on it cannot be
        # shortened at all, so the roll-back after a refused read-back fails with
        # ERROR_USER_MAPPED_FILE and the reader keeps the update that was just
        # refused. Nothing about that is visible on macOS, where ftruncate
        # succeeds either way -- which is why the check is on the ordering.
        "save_outside: send the worker's answer before releasing its mapping",
        "src/save_outside.rs",
        "        let answer = ask(&mut resource);\n"
        "        // Before the send, never after. See the note above.\n"
        "        drop(resource);\n"
        "        let _ = tx.send(answer);",
        "        let answer = ask(&mut resource);\n"
        "        let _ = tx.send(answer);\n"
        "        drop(resource);",
        "a_worker_is_released_before_its_answer_is_sent",
    ),
    Mutation(
        # Signal outside the table's lock, which is where it was until an
        # overdue reply arriving in the gap was traced through: the blocked
        # thread reads `killed`, takes the entry and hands its worker to
        # `discard`, which reaps the child -- and the pid is free before the
        # signal is sent. Windows reissues one immediately.
        "workers: signal an overdue worker after releasing the in-flight table",
        # The first spelling of this mutation kept the second loop *inside* the
        # block that holds `calls`, so the lock was still held when it signalled
        # and the test rightly saw nothing: a variant, not the defect. The kill
        # has to land after the block's closing brace to be outside the lock.
        "src/workers.rs",
        "            for call in calls.iter_mut().filter(|call| overdue.contains(&call.pid)) {\n"
        "                call.killed = true;\n"
        "                kill(call.pid);\n"
        "            }\n"
        "            overdue\n"
        "        };\n",
        "            for call in calls.iter_mut().filter(|call| overdue.contains(&call.pid)) {\n"
        "                call.killed = true;\n"
        "            }\n"
        "            overdue\n"
        "        };\n"
        "        for pid in &overdue {\n"
        "            kill(*pid);\n"
        "        }\n",
        "an_overdue_worker_is_signalled_while_its_entry_is_still_held",
    ),
    Mutation(
        # Keep the claim after a spare that never warmed. `prewarm` returns
        # early whenever `warming` is set, so this is not one lost spare: no
        # spare is ever started again for the life of the process, and every
        # open from then on pays the link and the font walk on the reader's
        # thread. The only symptom is opens that are quietly slower.
        "workers: keep the spare slot claimed after a warm that timed out",
        "src/workers.rs",
        "    let answered = rx.recv_timeout(within).ok();\n"
        "    slot.lock().unwrap_or_else(|e| e.into_inner()).warming = None;",
        "    let answered = rx.recv_timeout(within).ok();\n"
        "    if answered.is_some() {\n"
        "        slot.lock().unwrap_or_else(|e| e.into_inner()).warming = None;\n"
        "    }",
        "a_spare_that_never_warms_gives_the_slot_back",
        # The stand-in for a silent spare is a real process, so the test is
        # `#[cfg(unix)]` and this mutation reports SURVIVED on Windows rather
        # than blocking the run there.
        only_on="macos",
    ),
    Mutation(
        # Take the ceiling off a merge's inputs, which is how it was: every
        # incoming file read into one buffer with nothing in the way, in a
        # process that then copies it. A file dialog takes as many files as
        # somebody cares to shift-click.
        "save: merge as many incoming bytes as the reader picked",
        "src/save.rs",
        "        if total > ceiling {",
        "        if false {",
        "more_incoming_bytes_than_the_ceiling_are_refused_with_something_to_do",
    ),
    Mutation(
        # Add the byte-range lengths plainly. Three of `i64::MAX` is a legal
        # array and a five-line file: the sum panics under debug assertions and
        # wraps in the shipped build, where it becomes a coverage figure the
        # panel prints as fact.
        "docinfo: add a signature's byte-range lengths without saturating",
        "src/docinfo.rs",
        "            .fold(0_u64, u64::saturating_add);",
        "            .sum();",
        "byte_range_lengths_that_cannot_be_added_saturate_rather_than_wrap",
    ),
    Mutation(
        # Walk the `/Parent` chain with the trip count as the only guard. A loop
        # in it then yields the same ancestors until the bound runs out, and
        # every lap decrements a `/Count`: one deleted page took a three-page
        # tree to -29.
        "pagetree: walk a page's ancestors without a visited set",
        "src/pagetree.rs",
        "            if !seen.insert(parent) {\n"
        "                break;\n"
        "            }\n"
        "            decrements.push(parent);",
        "            decrements.push(parent);",
        "a_cyclic_parent_chain_decrements_each_ancestor_once",
    ),
    Mutation(
        # Decrement the count plainly. `/Count -9223372036854775808` is as
        # writable as any other number, and this panics on it under debug
        # assertions and wraps to the largest count there is in release.
        "pagetree: decrement a node's count without saturating",
        "src/pagetree.rs",
        '                tree.set("Count", count.saturating_sub(1));',
        '                tree.set("Count", count - 1);',
        "a_count_at_the_bottom_of_its_range_saturates_rather_than_wrapping",
    ),
    Mutation(
        # Fingerprint the name rather than the handle it was handed. The
        # document is mapped for the workers on one thread while this hashes on
        # another, so this is a second lookup of one name -- and a file replaced
        # in that window by a revision with the same page count leaves the model
        # describing bytes nobody has seen, with every other guard agreeing.
        "fingerprint: hash the pathname again instead of the handle handed over",
        "src/fingerprint.rs",
        "    pub fn of_open(file: &File, what: &Path) -> Result<Fingerprint, String> {\n"
        "        let meta = file",
        "    pub fn of_open(_handed: &File, what: &Path) -> Result<Fingerprint, String> {\n"
        "        let file = &File::open(what)\n"
        '            .map_err(|e| format!("could not open {} to fingerprint it: {e}", what.display()))?;\n'
        "        let meta = file",
        "a_fingerprint_through_a_handle_is_of_the_file_that_was_opened",
    ),
]

# Web links, added 2026-09-07 with the feature. The four in `weburl.rs` are the
# allowlist and the three refusals beside it; the three in `webopen.rs` are the
# app process keeping the addresses away from the webview, which is the property
# `docs/THREAT-MODEL.md` T8 now rests on and which no behaviour a reader can see
# would report if it stopped holding.
ROWS += [
    Mutation(
        # Open every scheme. The allowlist is the whole of the difference
        # between "tpdf opens web links" and "tpdf opens whatever a document
        # names", and on Windows the interesting schemes are the ones nobody
        # has heard of.
        "weburl: accept every scheme rather than http and https",
        "src/weburl.rs",
        '        if !matches!(url.scheme(), "http" | "https") {\n'
        "            return None;\n"
        "        }",
        "",
        "every_other_scheme_is_refused",
    ),
    Mutation(
        # Allow an embedded credential through. `https://bank.example@evil/` is
        # the one case where the host a reader picks out of the raw text is not
        # the host the request reaches.
        "weburl: allow a URL carrying a username or password",
        "src/weburl.rs",
        "        if !url.username().is_empty() || url.password().is_some() {\n"
        "            return None;\n"
        "        }",
        "",
        "an_embedded_credential_is_refused",
    ),
    Mutation(
        # Check for control characters only *after* the parse. `Url::parse`
        # strips tabs and newlines rather than refusing them, so the survivor of
        # this edit is a URL whose displayed form differs from the file's with
        # nothing reporting it.
        "weburl: look for control characters only after the parser has run",
        "src/weburl.rs",
        "        if raw.chars().any(is_unsafe_display) {\n"
        "            return None;\n"
        "        }\n\n"
        "        let url = Url::parse(raw).ok()?;",
        "        let url = Url::parse(raw).ok()?;",
        "a_control_character_is_refused_even_though_the_parser_strips_it",
    ),
    Mutation(
        # Show the whole path. The truncation is what keeps a reader's eye on
        # the host rather than on a reassuring string a stranger wrote.
        "weburl: show the path in full rather than cutting it",
        "src/weburl.rs",
        "    let mut out: String = rest.chars().take(MAX_REST_CHARS).collect();\n"
        "    if rest.chars().nth(MAX_REST_CHARS).is_some() {\n"
        "        out.push(ELLIPSIS);\n"
        "    }\n"
        "    out",
        "    rest.to_string()",
        "a_long_path_is_cut_and_says_so",
    ),
    Mutation(
        # Copy the addresses instead of taking them, so every URL in the
        # document continues to the webview. Nothing a reader can see changes:
        # links open, the dialog says the same thing, and the property T8 rests
        # on is gone.
        "webopen: copy the addresses out of the reply instead of draining them",
        "src/webopen.rs",
        "        let parsed: Addresses = std::mem::take(urls)\n            .iter()",
        "        let parsed: Addresses = urls\n            .iter()",
        "adopting_empties_the_list_the_frontend_would_receive",
    ),
    Mutation(
        # Drop the entries that do not parse rather than leaving a hole. The
        # list shortens, so every token after the hole names somebody else's
        # address -- the one failure here that opens the wrong page rather than
        # none.
        "webopen: drop an unparseable address instead of leaving a hole",
        "src/webopen.rs",
        "            .map(|raw| Web::parse(raw))\n            .collect();",
        "            .filter_map(|raw| Web::parse(raw))\n            .map(Some)\n            .collect();",
        "an_address_that_does_not_parse_leaves_a_hole_and_does_not_shift_the_rest",
    ),
    Mutation(
        # Forget only the links, leaving the outline's addresses behind under a
        # document number the service is about to reuse.
        "webopen: forget a document's links but not its outline",
        "src/webopen.rs",
        "            held.remove(&(*id, Source::Links));\n"
        "            held.remove(&(*id, Source::Outline));",
        "            held.remove(&(*id, Source::Links));",
        "forgetting_a_document_makes_its_tokens_name_nothing",
    ),
    Mutation(
        # Forget the document and none of the files it imported from. Since
        # 26.9.16 a web link on an inserted page is opened through the imported
        # file's own handle, so its list is reachable rather than merely held --
        # and a list left under a handle the service is about to hand to another
        # file answers that file's clicks with this one's addresses.
        "webopen: forget a document but not the files it imported from",
        "src/webopen.rs",
        "        for id in std::iter::once(&document).chain(sources) {",
        "        for id in std::iter::once(&document).chain(&sources[..0]) {",
        "a_document_takes_the_files_it_imported_from_with_it",
    ),
    Mutation(
        # Give every web target token 0. One link works and every other one
        # opens the first link's address.
        "links: number every web target zero rather than by position",
        "src/links.rs",
        "    let token = urls.len() as u32;\n    urls.push(web.url);",
        "    let token = 0;\n    urls.push(web.url);",
        "each_web_target_indexes_its_own_address",
    ),
]

# Added 2026-09-07 with the shared-XObject fix. A letterhead drawn on every page
# used to make the *writer* refuse a redaction, which took the region's words
# with it -- so the arithmetic that decides which drawings are shared, and the
# translation from an image ordinal to the object a reader is told about, are
# where a silent wrong answer costs a redaction rather than a warning.
ROWS += [
    Mutation(
        # Leave the picture and say nothing, which is the file that reads as
        # redacted and is not: the words go, the logo stays, and the verdict
        # calls it clean because nothing was reported.
        "shared: leave a repeated picture in the plan without reporting it",
        "src/redact.rs",
        "        if let Some(times) = shared.images.get(*ordinal).copied().flatten() {",
        "        if let Some(times) = None::<usize> {",
        "a_picture_drawn_elsewhere_too_is_taken_off_this_page_and_said_to_stay",
    ),
    Mutation(
        # Call every picture shared. The control, and the direction that costs a
        # capability rather than a promise: no region would remove any picture
        # from any document, and every check about a repeated one still passes.
        "shared: treat an unrepeated picture as repeated",
        "src/redact.rs",
        "        if let Some(times) = shared.images.get(*ordinal).copied().flatten() {",
        "        if let Some(times) = shared.images.get(*ordinal).copied().flatten().or(Some(1)) {",
        "a_picture_that_is_not_shared_is_left_in_the_plan",
    ),
    Mutation(
        # Answer positionally when the positions are in doubt. `remove_images`
        # refuses on the same disagreement, so this is the edit that turns a
        # refusal a reader can act on into a picture silently left behind.
        "shared: answer from a draw count that disagrees with PDFium",
        "src/redact.rs",
        "    if names.len() != expected {\n        return vec![None; expected];\n    }",
        "    if false {\n        return vec![None; expected];\n    }",
        "an_image_count_that_disagrees_answers_nothing_at_all",
    ),
    Mutation(
        # Report nothing shared, ever. The control over `shared_draws` itself:
        # without it every check here could be satisfied by a function that
        # always says *drawn once*, which is what the module did before.
        "shared: report every drawing as repeated",
        "src/redact.rs",
        "                .and_then(|id| drawn_more_than_once(doc, page, id, names, name))",
        "                .and_then(|id| drawn_more_than_once(doc, page, id, names, name).or(Some(2)))",
        "a_picture_this_page_alone_draws_is_not_reported_as_shared",
    ),
    Mutation(
        # Ask about the page's forms with an empty name list. Every form comes
        # back unrepeated, so a shared header's text is planned for removal and
        # the writer refuses the whole redaction -- the defect, restored on the
        # half of it that is about forms.
        "shared: ask nothing about the page's forms",
        "src/redact.rs",
        "            &form_draws(doc, page, &content).unwrap_or_default(),",
        "            &[],",
        "a_form_a_second_page_also_names_is_counted",
    ),
    Mutation(
        # Name the picture by its ordinal among images rather than by its
        # position in the page's object list. Both are small integers and they
        # agree on a page whose only object is a picture, which is what most
        # fixtures are.
        "shared: report the image ordinal as the object a reader sees",
        "src/redact.rs",
        "            at: image_at.get(*ordinal).copied().unwrap_or(*ordinal),",
        "            at: *ordinal,",
        "a_note_names_the_page_object_rather_than_the_image_ordinal",
    ),
    Mutation(
        # Report a shared form once per line of its text the region covers. The
        # count is right for a region over one line and wrong for every larger
        # one, which is the shape `Unhandled::at` exists to prevent.
        "shared: report a repeated form once per covered line",
        "src/redact.rs",
        '        if !left\n'
        '            .iter()\n'
        '            .any(|other| other.at == *at && other.kind == "form")\n'
        "        {",
        "        if true {",
        "a_shared_form_is_one_finding_however_many_of_its_lines_are_covered",
    ),
    Mutation(
        # Look up the first form for every one of them. One shared form on a
        # page then takes every other form's text out of the plan with it, and
        # a page with one form -- which is most of them -- cannot tell.
        "shared: read one form's repeat count for all of them",
        "src/redact.rs",
        "            .position(|form| form.at == *at)",
        "            .position(|_| true)",
        "a_shared_form_does_not_take_another_forms_lines_with_it",
    ),
    Mutation(
        # Append the findings in the order they were computed. A reader checks a
        # page from the top, and a shared picture reported after a shading
        # further down the sheet reads as a second page's finding.
        "shared: leave the findings in the order they were appended",
        "src/redact.rs",
        "    plan.unhandled.sort_by_key(|object| object.at);",
        "    let _ = &plan.unhandled;",
        "findings_stay_in_page_order_when_a_shared_one_joins_them",
    ),
    Mutation(
        # Say the one sentence for both reasons. A reader whose letterhead was
        # left is told tpdf cannot handle their picture, which sends them to
        # look for a different tool rather than at their own document.
        "shared: explain a repeated object as one tpdf cannot handle",
        "src/redact.rs",
        "        let Unhandled { at, kind, drawn } = self;",
        "        let Unhandled { at, kind, drawn: _ } = self;\n        let drawn = &None::<usize>;",
        "a_repeated_object_says_why_it_stayed_and_an_ordinary_one_does_not",
    ),
]

ROWS += [
    Mutation(
        "raster: accept a snapshot whose digest differs from the opened source",
        "src/save_outside.rs",
        "    if digest != expected.digest {",
        "    if false && digest != expected.digest {",
        "raster_snapshot_binds_bytes_and_survives_source_changes",
    ),
    Mutation(
        "raster: allow overwriting the source with an image-only copy",
        "src/save.rs",
        '    if same_file(source, out) {\n        return Err(said!(\n'
        '            "Choose a different name for the image-only copy; the original must be kept"\n',
        '    if false {\n        return Err(said!(\n'
        '            "Choose a different name for the image-only copy; the original must be kept"\n',
        "raster_copy_never_overwrites_its_source",
    ),
]

ROWS += [
    Mutation(
        "strict parsing: recover a damaged signed prefix",
        "src/docinfo.rs",
        "                // which objects belonged to the signed revision.\n"
        "                strict: true,",
        "                // which objects belonged to the signed revision.\n"
        "                strict: false,",
        "an_appendix_whose_signed_prefix_is_unparseable_is_reported_as_unread",
    ),
    Mutation(
        "strict parsing: repair a saved file instead of rejecting its broken table",
        "src/save.rs",
        "pub fn reread_pages(bytes: &[u8], password: Option<&str>) -> Result<usize, String> {\n"
        "    Document::load_mem_with_options(\n"
        "        bytes,\n"
        "        lopdf::LoadOptions {\n"
        "            strict: true,",
        "pub fn reread_pages(bytes: &[u8], password: Option<&str>) -> Result<usize, String> {\n"
        "    Document::load_mem_with_options(\n"
        "        bytes,\n"
        "        lopdf::LoadOptions {\n"
        "            strict: false,",
        "an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was",
    ),
]

ROWS += [
    Mutation(
        "visual signature: discard transparency when writing its mask",
        "src/signature.rs",
        "            alpha.push(pixel[3]);",
        "            alpha.push(255);",
        "signature_pixels_alpha_and_placement_survive_append_and_rewrite",
    ),
    Mutation(
        "visual signature: accept an invisible raster",
        "src/signature.rs",
        "            && self.rgba.chunks_exact(4).any(|p| p[3] != 0)",
        "            && !self.rgba.is_empty()",
        "signature_rasters_reject_bad_lengths_limits_and_invisible_pixels",
    ),
]

ROWS += [
    Mutation("boxed edit: lose following cursor", "src/textedit/layout.rs",
        "let adjustment = (-context.cursor_after * 1000. / run.size) as f32;",
        "let adjustment = 0_f32;",
        "layout_rotations_and_continued_shows_preserve_later_geometry"),
    Mutation("boxed edit: lose original line origin", "src/textedit/layout.rs",
        "    operations.extend(restore_line(",
        "    drop(restore_line(",
        "layout_rotations_and_continued_shows_preserve_later_geometry"),
    Mutation("line restore: skip the line moves", "src/textedit/layout.rs",
        '"Td" | "TD" | "T*" | "TL" => restored.push(op.clone()),',
        '"Td" | "TD" | "T*" | "TL" => {}',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: replay T* with the run's leading", "src/textedit/layout.rs",
        'restored.push(numeric("TL", &[leading]));',
        '',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: identity at a different origin", "src/textedit/layout.rs",
        '"BT" => Operation::new("Tm", [1, 0, 0, 1, 0, 0]',
        '"BT" => Operation::new("Tm", [1, 0, 0, 1, 0, 1]',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: identity for a Tm too", "src/textedit/layout.rs",
        '"Tm" => start.clone(),',
        '"Tm" => Operation::new("Tm", [1, 0, 0, 1, 0, 0].map(Object::Integer).to_vec()),',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: no leading recorded at BT", 'src/textedit/inspect.rs',
        'self.matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];\n        self.line_origin = (index, leading);',
        'self.matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];\n        self.line_origin = (index, 0.0);',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: start at the block, not the Tm", 'src/textedit/inspect.rs',
        'self.cursor = 0.0;\n        self.line_origin = (index, leading);',
        'self.cursor = 0.0;',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("boxed edit: omit wrapped line movement", "src/textedit/layout.rs",
        "let dy = first_baseline - index as f64 * line_height;",
        "let dy = first_baseline;",
        "wrapping_keeps_spaces_line_breaks_and_following_text_in_place"),
    Mutation("boxed edit: bypass document clipping", "src/textedit/layout.rs",
        'clipping::contains(context.clip, ink).map_err',
        'clipping::contains(None, ink).map_err',
        "fallback_deletion_and_clipped_growth_remain_editable_or_refuse_atomically"),
    Mutation("boxed edit: allow collisions with other lines", "src/textedit/layout.rs",
        "                    if strikes(shown, *other, free.own) {",
        "                    if false && strikes(shown, *other, free.own) {",
        "overflowing_clipped_colliding_and_invalid_layouts_refuse_atomically"),
    Mutation("boxed edit: suppress automatic font fallback", "src/textedit/layout.rs",
        "EditFont::Auto if encodable => None,",
        "EditFont::Auto if encodable || true => None,",
        "fallback_embeds_new_characters_and_reopens_for_another_edit"),
]

# Pages of another file, 2026-09-19: the model's import command and the rewrite
# that places the pages. Each is aimed at the one test whose job it is, and the
# three `pages_are_the_file` mutations above were re-aimed the same day, when
# that predicate became an exhaustive `match`.
ROWS += [
    Mutation(
        # Accept an empty selection at the entry point. The apply takes it, so
        # a source record and a selection id are spent on a command that places
        # nothing --- the refusal this replaces is what keeps the tables empty.
        "docmodel: import: accept an empty selection",
        "src/docmodel.rs",
        "        if pages.is_empty() {\n            return Err(Refusal::EmptyImport);\n        }",
        "",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Place a page the other file does not have. The model has no other way
        # to know the count, and the writer would refuse the whole save later.
        "docmodel: import: accept a page past the end of the file",
        "src/docmodel.rs",
        "        if let Some(&past) = pages.iter().find(|&&page| page >= source.pages) {",
        "        if let Some(&past) = pages.iter().find(|&&page| page > source.pages) {",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Accept a page named twice in one selection. The writer imports a
        # selection in one walk and refuses the repeat, so the save fails for a
        # command the model took.
        "docmodel: import: accept the same page twice in one selection",
        "src/docmodel.rs",
        "        if let Some(&twice) = pages.iter().find(|&&page| !seen.insert(page)) {",
        "        if let Some(&twice) = pages.iter().find(|_| false) {",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Put the imported pages at the front whatever they were anchored to.
        "docmodel: import: ignore the anchor",
        "src/docmodel.rs",
        "                let start = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let start = 0;",
        "an_import_places_its_pages_behind_the_anchor_in_the_order_asked",
    ),
    Mutation(
        # Keep a discarded import's selection. Nothing reads it, so no working
        # document differs; the accounting observable is what can say so.
        "docmodel: import: keep a discarded selection body",
        "src/docmodel.rs",
        "                Command::Import { selection, .. } => {\n                    self.selections.remove(&selection);\n                }",
        "                Command::Import { .. } => {}",
        "a_discarded_import_takes_its_selection_and_its_file_record_with_it",
    ),
    Mutation(
        # Keep a file record no selection names.
        "docmodel: import: keep a file record nothing names",
        "src/docmodel.rs",
        "        self.sources.retain(|id, _| named.contains(id));",
        "        let _ = named;",
        "a_discarded_import_takes_its_selection_and_its_file_record_with_it",
    ),
    Mutation(
        # Let a region be marked on a page of another file. Every step that
        # proves a removal clean is addressed to the opened document, and the
        # removal reaches the file, so it would strike every position showing
        # that page.
        "docmodel: import: redact a page of another file",
        "src/docmodel.rs",
        "                PageSource::Imported { .. } => {\n                    return Err(Refusal::RedactionOnImportedPage(redaction.page));\n                }",
        "                PageSource::Imported { .. } => {}",
        "a_region_on_a_page_from_another_file_is_refused",
    ),
    Mutation(
        # Refuse the region for the document rather than for the page --- the
        # over-broad half that 2026-09-20 removed, put back. The accept is what
        # goes red, which is what says the narrowing is load-bearing and not
        # merely a rename.
        "docmodel: import: refuse a region anywhere while a page came from another file",
        "src/docmodel.rs",
        "        if let Some(page) = self.now.page(redaction.page) {\n            match page.source {",
        "        if self.now.pages.values().any(|page| matches!(page.source, PageSource::Imported { .. })) {\n            return Err(Refusal::RedactionOnImportedPage(redaction.page));\n        }\n        if let Some(page) = self.now.page(redaction.page) {\n            match page.source {",
        "a_region_on_the_reader_s_own_page_is_accepted_beside_an_inserted_one",
    ),
    Mutation(
        # Leave the gate page at its number in the source file. The gate
        # reopens the written file and renders a page of it by number, and the
        # two are the same only when nothing moved.
        "redact: gate the page at its number in the source file",
        "src/redact.rs",
        "            one.page = u32::try_from(slot).ok()?;",
        "            let _ = slot;",
        "a_gate_page_follows_its_page_to_where_the_written_file_puts_it",
    ),
    Mutation(
        # Gate a page the plan does not place, at slot zero --- the failure path
        # that acts hardest where it knows least.
        "redact: gate a page the plan does not place, at slot zero",
        "src/redact.rs",
        "            let slot = pages\n                .iter()\n                .position(|page| page.source == docmodel::PageSource::Baseline(one.page))?;",
        "            let slot = pages\n                .iter()\n                .position(|page| page.source == docmodel::PageSource::Baseline(one.page))\n                .unwrap_or(0);",
        "a_gate_page_the_plan_does_not_place_is_dropped",
    ),
    Mutation(
        # Add the inserted-pages note to every redaction in such a document,
        # whether the scan found anything or not. `verified` is `why.is_empty()`,
        # so that is the document-wide refusal back under another name.
        "redact: note the inserted pages even when the scan found nothing",
        "src/redact.rs",
        "    if inserted == 0 || found.is_empty() || placed {\n        return None;\n    }",
        "    if inserted == 0 || placed {\n        return None;\n    }",
        "the_inserted_pages_note_is_added_only_when_the_scan_found_something",
    ),
    Mutation(
        # Let the black fill re-read every file the insert came from. A source
        # touched since the removal then refuses the fill, after the words are
        # already gone.
        "redaction_fill: keep the other files in the fill pass's plan",
        "src/redaction_fill.rs",
        "        sources: Vec::new(),\n        // Addressed by",
        "        sources: original.sources.clone(),\n        // Addressed by",
        "the_fill_pass_names_no_other_file_and_no_text_edit",
    ),
    Mutation(
        # Edit the reader's own page by its slot in the output rather than by
        # its number in the file. With pages inserted in front of it, that is
        # the inserted page's content stream.
        "save: address a removal by output slot rather than by baseline page",
        "src/save.rs",
        "        apply_redactions(&mut doc, &pages, &plan.redactions)?",
        "        apply_redactions(&mut doc, &order, &plan.redactions)?",
        "a_rewrite_removes_from_the_reader_s_page_and_carries_the_inserted_ones_whole",
    ),
    Mutation(
        # Edit a page of another file that this document shows twice. The
        # writer edits the file and imports it once per position, so the edit
        # would appear in the position nobody edited.
        "docmodel: import: edit a page of another file placed twice",
        "src/docmodel.rs",
        "                if self.now.placements_of(source, number) > 1 {\n                    return Err(Refusal::TextOnRepeatedImport(page));\n                }",
        "",
        "text_on_a_page_this_document_shows_twice_is_refused",
    ),
    Mutation(
        # Accept a replacement addressed at another page of the same file: the
        # stale-address case, on the other file's side of it.
        "docmodel: import: take any page number as this imported page's",
        "src/docmodel.rs",
        "            } if number == change.page => {",
        "            } if true => {",
        "a_replacement_addressed_at_another_page_of_the_same_file_is_refused",
    ),
    Mutation(
        # Insert a page that already carries a replacement. Two positions, one
        # edit, and the writer would show it in both.
        "docmodel: import: insert a page that already has edited text",
        "src/docmodel.rs",
        "        if let Some(id) = known {\n            let edited = self.now.edited_pages_of(id);\n            if let Some(&again) = pages.iter().find(|page| edited.contains(page)) {\n                return Err(Refusal::ImportOfEditedPage(again));\n            }\n        }",
        "",
        "inserting_a_page_that_already_has_edited_text_is_refused",
    ),
    Mutation(
        # Report every replacement as the opened document's. The other file's
        # would then be applied to whatever the opened file has at the number.
        "docmodel: import: forget which file a replacement is addressed in",
        "src/docmodel.rs",
        "                    PageSource::Imported { source, .. } => {\n                        Some(textedit::Edit::imported(source.get(), change))\n                    }",
        "                    PageSource::Imported { .. } => Some(textedit::Edit::opened(change)),",
        "a_replacement_on_an_imported_page_names_the_file_it_came_from",
    ),
    Mutation(
        # Let a comment of the opened file be deleted off a page of another. The
        # writer would delete whatever the opened file has at that number, on
        # some other page.
        "docmodel: import: accept a foreign comment on an imported page",
        "src/docmodel.rs",
        "            Some(PageSource::Imported { .. }) => Err(Refusal::ForeignCommentOnImportedPage(id)),",
        "            Some(PageSource::Imported { .. }) => Ok(()),",
        "a_foreign_comment_cannot_be_said_to_be_on_an_imported_page",
    ),
    Mutation(
        # Keep a replacement in the plan on the page number alone, whatever
        # file it names. An extract then carries the other file's edit with a
        # page of the opened one.
        "edits: filter a plan's replacements by page number alone",
        "src/edits.rs",
        "            source.get() == file && page == edit.change.page",
        "            page == edit.change.page",
        "a_source_handle_answers_with_that_files_replacements",
    ),
    Mutation(
        # Hand the opened document's worker every replacement, the other
        # files' included. Each would be applied to whatever the opened file
        # has at the same page number.
        "edits: render the opened document with every file's replacements",
        "src/edits.rs",
        "                .filter(|edit| edit.source.is_none())",
        "",
        "a_source_handle_answers_with_that_files_replacements",
    ),
    Mutation(
        # And the mirror: hand an imported file's worker every replacement.
        "edits: render an imported file with every file's replacements",
        "src/edits.rs",
        "                .filter(|edit| edit.source == Some(file.get()))",
        "",
        "a_source_handle_answers_with_that_files_replacements",
    ),
    Mutation(
        # Answer a source handle as though it named nothing. Every inserted
        # page would draw without the reader's own edit on it, silently.
        "edits: stop looking for an imported file's handle",
        "src/edits.rs",
        "        for open in docs.values() {\n            let Some((&file, _)) = open.sources.iter().find(|(_, &held)| held == handle) else {\n                continue;\n            };",
        "        for open in docs.values() {\n            let Some((&file, _)) = open.sources.iter().find(|(_, &held)| held != handle) else {\n                continue;\n            };",
        "a_source_handle_answers_with_that_files_replacements",
    ),
    Mutation(
        # Read an inserted page's text through the opened document's handle,
        # which holds a different page of the same number.
        "commands: read an inserted page's text from the opened document",
        "src/commands/read.rs",
        "            Ok(TextAddress {\n                doc: held,",
        "            Ok(TextAddress {\n                doc,",
        "an_imported_page_is_read_through_its_own_files_handle",
    ),
    Mutation(
        # Fall back to the opened document for a file with no handle, which is
        # the wrong-page picture `PageAddress` exists to refuse.
        "commands: fall back to the opened document for an unopened file",
        "src/commands/read.rs",
        '                None => return Err(said!("The file this page came from is no longer open")),',
        '                None => doc,',
        "an_imported_page_whose_file_is_not_open_is_refused",
    ),
    Mutation(
        # Hand a worker every pending replacement rather than the ones
        # addressed in the document it holds.
        "commands: preflight a page against every file's replacements",
        "src/commands/read.rs",
        "            .filter(|edit| edit.source == self.source)",
        "",
        "a_worker_is_handed_only_the_replacements_addressed_in_its_own_document",
    ),
    Mutation(
        # Call a plan with a page of another file the file on disk. With a mark
        # beside it the save takes the append, which cannot write the page.
        "edits: read a page of another file as the file's own page",
        "src/edits.rs",
        "                    PageSource::Imported { .. } => false,\n                };",
        "                    PageSource::Imported { .. } => true,\n                };",
        "a_plan_with_a_page_of_another_file_is_never_appendable",
    ),
    Mutation(
        # Hand the rewriter nothing. The pages cannot be imported and every save
        # of such a document is refused.
        "save: import: rewrite without the other files",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, None)?;",
        "a_rewrite_places_pages_of_another_file_where_the_plan_puts_them",
    ),
    Mutation(
        # Import from a file that changed since the reader chose its pages.
        "save: import: skip the digest check",
        "src/save.rs",
        "        if digest != expected.digest {",
        "        if false {",
        "a_source_changed_since_its_pages_were_inserted_is_refused",
    ),
    Mutation(
        # Ask one walk for a page placed twice, which `merge::import` refuses.
        "save: import: put a repeated page in the same round",
        "src/save.rs",
        "                .find(|round| round.iter().all(|&(_, taken)| taken != page))",
        "                .find(|_| true)",
        "the_same_page_placed_twice_is_two_pages",
    ),
    Mutation(
        # Drop an imported page's crop: `agreed_crops` keys by baseline page.
        "save: import: forget an imported page's crop",
        "src/save.rs",
        "    crops.extend(",
        "    let _ = (",
        "a_mark_a_turn_and_a_crop_on_an_imported_page_land_on_it",
    ),
    Mutation(
        # Ask only the trailer whether the other file is encrypted, which
        # `lopdf` empties the moment the empty password opens it.
        "save: import: miss encryption the empty password opened",
        "src/save.rs",
        '        if document.was_encrypted() || document.is_encrypted() {\n            return Err(said!(\n                "{label} is encrypted, and inserting',
        '        if document.is_encrypted() {\n            return Err(said!(\n                "{label} is encrypted, and inserting',
        "a_source_an_empty_password_opens_is_refused_rather_than_decrypted",
    ),
    Mutation(
        # Leave the page count to `merge::import`, in the half that writes.
        "save: import: skip the other file's page count",
        "src/save.rs",
        "                    if number as usize >= has {",
        "                    if false {",
        "a_page_the_other_file_does_not_have_is_refused",
    ),
    Mutation(
        # Let a merge reach the rewrite's refusal about a file it was not given.
        "save: import: merge a document holding imported pages",
        "src/save.rs",
        "    if !plan.sources.is_empty() {\n        return Err(said!(\n            \"save this document before merging it",
        "    if false {\n        return Err(said!(\n            \"save this document before merging it",
        "a_merge_of_a_document_holding_imported_pages_says_why_it_is_refused",
    ),
    Mutation(
        # Edit the opened document with an inserted page's replacement. It is
        # addressed in the other file, so the digest would either refuse it or
        # -- worse, on a page that happens to match -- change the wrong page.
        "save: import: write every replacement into the opened document",
        "src/save.rs",
        "        let Some(file) = edit.source else {\n            base.push(edit.change.clone());\n            continue;\n        };",
        "        let file = 0;\n        if edit.source.is_none() || true {\n            base.push(edit.change.clone());\n            continue;\n        }\n        #[allow(unreachable_code)]\n        let _: u32 = file;",
        "a_rewrite_writes_a_replacement_into_the_page_it_imports",
    ),
    Mutation(
        # Drop an inserted page's replacement instead of writing it. The save
        # reports success and the reader's words are not in the file.
        "save: import: drop an inserted page's replacement",
        "src/save.rs",
        "        textedit::write(into, &changes)?;",
        "        let _ = (into, &changes);",
        "a_rewrite_writes_a_replacement_into_the_page_it_imports",
    ),
    Mutation(
        # Take a replacement naming a file the plan does not list. It would be
        # written into whichever document happens to be at that position.
        "save: import: accept a replacement naming a file the plan lacks",
        "src/save.rs",
        "        let from = plan\n            .sources\n            .iter()\n            .position(|one| one.id == file)",
        "        let from = plan\n            .sources\n            .iter()\n            .position(|_| true)",
        "replacements_a_save_cannot_place_are_refused_with_the_reason",
    ),
    Mutation(
        # Accept a replacement on a page this save does not place. The other
        # file is edited, a different page of it is imported, and nothing
        # carries the reader's words -- a silent half-write reported as a save.
        "save: import: accept a replacement on a page nothing places",
        "src/save.rs",
        "        match placed {\n            0 => {",
        "        match placed {\n            usize::MAX => {",
        "replacements_a_save_cannot_place_are_refused_with_the_reason",
    ),
    Mutation(
        # Accept a replacement on a page the plan places twice, which would
        # change the position nobody edited as well as the one they did.
        "save: import: accept a replacement on a page placed twice",
        "src/save.rs",
        '            1 => {}\n            _ => {\n                return Err(Refusal::from(said!(',
        '            _ => {}\n            #[allow(unreachable_patterns)]\n            _ => {\n                return Err(Refusal::from(said!(',
        "replacements_a_save_cannot_place_are_refused_with_the_reason",
    ),
]

# The render handles of the other files a document imports from, and the
# refusals of the command that opens them. What each of these costs is a
# sandboxed worker pool leaked, or a page drawn from nowhere --- neither of
# which a reader can see until the machine has a dozen of them.
ROWS += [
    Mutation(
        # Close a document and hand back nothing, leaking a pool per file.
        "edits: sources: close without handing the handles back",
        "src/edits.rs",
        "        let mut held: Vec<u32> = open.sources.into_values().collect();",
        "        let mut held: Vec<u32> = Vec::new();",
        "closing_a_document_hands_back_every_file_it_imported_from",
    ),
    Mutation(
        # Release on undo, which the redo tail still needs.
        "edits: sources: release the handles when the import is undone",
        "src/edits.rs",
        "        let model = &mut open.model;\n        model.undo();",
        "        let model = &mut open.model;\n        model.undo();\n        open.sources.clear();",
        "undoing_an_import_keeps_the_handle_and_closing_releases_it",
    ),
    Mutation(
        # Hand back the first handle of a file imported again, which releases
        # the pool the pages on screen are drawn from.
        "edits: sources: hand back the handle in use for a file imported again",
        "src/edits.rs",
        "            Some(_) => Some(handle),",
        "            Some(&kept) => Some(kept),",
        "importing_the_same_file_again_hands_back_the_second_handle",
    ),
    Mutation(
        # Answer without the handles, so the frontend has nowhere to draw from.
        "edits: sources: leave the handles out of the reply",
        "src/edits.rs",
        "    state.sources = sources;\n",
        "",
        "an_import_answers_the_handle_its_pages_are_drawn_from",
    ),
    Mutation(
        # A guard dropped without being kept releases nothing: every refusal
        # after the open leaks the pool it opened.
        "imports: release nothing when the guard is dropped",
        "src/imports.rs",
        "        if let Some(release) = self.release.take() {\n            release(self.id);\n        }",
        "        let _ = self.release.take();",
        "a_refusal_after_the_open_releases_the_handle",
    ),
    Mutation(
        # Keep and release anyway, which closes the pool the model just
        # recorded as the one its pages are drawn from.
        "imports: release a handle that was kept",
        "src/imports.rs",
        "        self.release = None;\n        self.id",
        "        self.id",
        "a_kept_handle_is_the_caller_s_and_is_not_released",
    ),
    Mutation(
        # Pass a locked file on as the open's own sentence, which offers a
        # password for a file the save would refuse anyway.
        "imports: word a locked file like any other failure",
        "src/imports.rs",
        "    if refusal.locked {\n        return locked(name);\n    }",
        "",
        "a_locked_file_is_refused_as_encrypted",
    ),
]

# The file an insert holds open while the reader names its pages, 2026-09-19.
# Every path out of the wait has to end it --- placed, replaced, cancelled,
# refused or closed --- and a stale answer must not end a newer one.
ROWS += [
    Mutation(
        # Close a document and forget the file it was waiting on.
        "edits: pending: close without handing back the waiting file",
        "src/edits.rs",
        "        held.extend(open.pending.map(|pending| pending.handle));\n",
        "",
        "closing_a_document_releases_the_file_waiting_for_its_pages",
    ),
    Mutation(
        # Replace the waiting file and leave its pool to nobody.
        "edits: pending: replace without handing back the first file",
        "src/edits.rs",
        "            .map(|previous| previous.handle);",
        "            .map(|_| None).unwrap_or(None);",
        "a_second_prepare_replaces_the_first_and_hands_it_back",
    ),
    Mutation(
        # End whatever is waiting, whatever id the cancel names.
        "edits: pending: cancel a newer import than the one named",
        "src/edits.rs",
        "        let open = docs.get_mut(&doc)?;\n        if open.pending.as_ref().map(|waiting| waiting.id) != Some(pending) {\n            return None;\n        }",
        "        let open = docs.get_mut(&doc)?;",
        "a_cancel_hands_back_the_file_it_names_and_only_that_one",
    ),
    Mutation(
        # Commit whatever is waiting, whatever id the answer names.
        "edits: pending: commit an answer to another question",
        "src/edits.rs",
        '            if open.pending.as_ref().map(|waiting| waiting.id) != Some(pending) {\n                return Err(said!(',
        '            if open.pending.is_none() {\n                return Err(said!(',
        "a_commit_naming_an_import_nobody_is_waiting_on_is_refused",
    ),
    Mutation(
        # Take the file for a commit the model then refuses, and release nothing.
        "edits: pending: leak the file on a refused commit",
        "src/edits.rs",
        "        let held = imports::Held::new(taken.handle, release.clone());",
        "        let held = imports::Held::new(taken.handle, |_| {});",
        "a_refused_commit_releases_the_file",
    ),
    Mutation(
        # Keep a second pool for a file the document already draws from.
        "edits: pending: keep the spare handle of a file already held",
        "src/edits.rs",
        "            debug_assert_eq!(spare, kept);\n            release(spare);",
        "            debug_assert_eq!(spare, kept);",
        "committing_a_file_already_held_releases_the_second_handle",
    ),
]

# --- whether a signature still covers what it was made over ----------------
#
# `integrity.rs`, 2026-09-26. Each mutation removes one check the verdict
# rests on, and names the test built so that only that check can fail it ---
# the fixtures are rewritten one field at a time for exactly that reason.
ROWS += [
    Mutation(
        # Never test the signature value. Every fixture whose signature holds
        # still passes; `signed-broken.pdf`, whose one flipped bit is in the
        # value, reads as intact --- the forgery case, reported as the good one.
        "integrity: skip the signature mathematics",
        "src/integrity.rs",
        "        if !verify(&key, method, hash, &to_verify, &self.signature) {",
        "        if false {",
        "a_flipped_bit_in_the_signature_value_is_broken",
    ),
    Mutation(
        # Never compare the digest. The tampered-document case: a byte inside
        # the signed range changed and the signature still reports intact.
        "integrity: skip the digest comparison",
        "src/integrity.rs",
        "        if !digest_matches {",
        "        if false {",
        "a_changed_byte_in_the_signed_range_is_altered",
    ),
    Mutation(
        # Believe the digest before the signature. A document both altered and
        # carrying a broken value then reads as merely altered --- trusting a
        # digest nothing vouches for.
        "integrity: judge the digest before the signature",
        "src/integrity.rs",
        """        if !verify(&key, method, hash, &to_verify, &self.signature) {
            return named(Verdict::Broken);
        }
        if !digest_matches {
            return named(Verdict::Altered);
        }""",
        """        if !digest_matches {
            return named(Verdict::Altered);
        }
        if !verify(&key, method, hash, &to_verify, &self.signature) {
            return named(Verdict::Broken);
        }""",
        "a_broken_signature_is_broken_even_when_the_bytes_also_changed",
    ),
    Mutation(
        # Call a SHA-1 match intact.
        "integrity: treat SHA-1 as strong",
        "src/integrity.rs",
        "        if hash == Hash::Sha1 {",
        "        if false {",
        "a_sha1_match_is_weak_and_never_intact",
    ),
    Mutation(
        # Verify under the only certificate even when the signature names
        # another. The dialog's reading keeps that certificate for a set of
        # one; the verdict must not.
        "integrity: verify under a certificate the signature does not name",
        "src/integrity.rs",
        "            .filter(|(_, matched)| *matched)",
        "            .filter(|_| true)",
        "a_signer_the_signature_does_not_name_is_not_used_to_check_it",
    ),
    Mutation(
        # Accept any hex string in the hole. The signature-wrapping shape: the
        # range leaves some other value uncovered and the digest still matches.
        "integrity: accept a hole that is not this signature's value",
        "src/integrity.rs",
        "    if decode_hex(inner)? != contents {",
        "    if decode_hex(inner).is_none() {",
        "a_range_whose_hole_holds_anything_but_this_signature_is_refused",
    ),
    Mutation(
        # Accept a range that skips the head of the file.
        "integrity: accept a range that does not start at zero",
        "src/integrity.rs",
        "    if start != 0 || first == 0 || second <= first || end > size {",
        "    if first == 0 || second <= first || end > size {",
        "a_range_not_starting_at_zero_or_not_in_two_pieces_is_refused",
    ),
    Mutation(
        # Accept a range reaching past the end of the file --- which then
        # panics on the slice, inside the worker.
        "integrity: accept a range past the end of the file",
        "src/integrity.rs",
        "    if start != 0 || first == 0 || second <= first || end > size {",
        "    if start != 0 || first == 0 || second <= first {",
        "a_range_the_file_cannot_hold_is_refused_without_arithmetic_overflow",
    ),
    Mutation(
        # Hash whatever the document asks for.
        "integrity: ignore the hashing budget",
        "src/integrity.rs",
        "        if cost > *budget {",
        "        if false {",
        "a_budget_smaller_than_the_range_refuses_before_hashing",
    ),
    Mutation(
        # Never charge the budget, so it bounds each signature and not the
        # document: thirty-two signatures over a large file hash it 32 times.
        "integrity: never spend the budget",
        "src/integrity.rs",
        "        *budget -= cost;",
        "",
        "a_budget_smaller_than_the_range_refuses_before_hashing",
    ),
    Mutation(
        # Check every subfilter with the detached computation.
        "integrity: check a subfilter it does not implement",
        "src/integrity.rs",
        '    if !document_timestamp && !matches!(kind, "adbe.pkcs7.detached" | "ETSI.CAdES.detached") {',
        "    if kind.is_empty() && false {",
        "a_subfilter_this_does_not_check_is_not_checked",
    ),
    Mutation(
        # Ignore the content-type attribute. The signature and the digest
        # both still hold on the relabelled fixture, which is why only this
        # comparison can refuse it.
        "integrity: ignore the content type the attributes state",
        "src/integrity.rs",
        "    if &stated_type != content_type {",
        "    if false {",
        "a_content_type_the_attributes_disagree_with_is_not_checked",
    ),
    Mutation(
        # Check the first of several signers.
        "integrity: attribute a two-signer blob to its first signer",
        "src/integrity.rs",
        "        let [info] = infos else {",
        "        let [info, ..] = infos else {",
        "a_blob_with_two_signers_is_not_attributed_to_either",
    ),
    Mutation(
        # Let a signature algorithm name a different hash from the digest.
        "integrity: accept a signature algorithm naming another hash",
        "src/integrity.rs",
        "        if named == hash {",
        "        if named == hash || true {",
        "a_signature_algorithm_naming_another_hash_is_not_checked",
    ),
    Mutation(
        # Ignore the salt length PSS states and assume the default of 20.
        # pyHanko salts with the digest's own length, so the mathematics fails.
        "integrity: assume PSS's default salt length",
        "src/integrity.rs",
        "        salt: usize::from(params.salt_len),",
        "        salt: 20,",
        "each_signature_scheme_is_checked_by_its_own_mathematics",
    ),
    Mutation(
        # Hash the signed attributes under their `[0]` tag, as written. RFC
        # 5652 says SET OF; every real signature then reads as broken.
        "integrity: hash the signed attributes under their implicit tag",
        "src/integrity.rs",
        "            *encoded.first_mut()? = 0x31;",
        "",
        "an_untouched_signature_is_intact",
    ),
    Mutation(
        # An unchecked verdict with no reason.
        "integrity: refuse without saying why",
        "src/integrity.rs",
        "            why: Some(why),",
        "            why: None,",
        "an_unchecked_verdict_always_says_why",
    ),
    Mutation(
        # Compute the verdict and never attach it. Every assertion inside
        # `integrity.rs` about a fixture reads it back through `docinfo::scan`,
        # which is the path the dialog takes --- so this is caught there.
        "docinfo: compute the integrity verdict and drop it",
        "src/docinfo.rs",
        "    out.integrity = Some(integrity);",
        "",
        "an_untouched_signature_is_intact",
    ),
    Mutation(
        # Hand the verdict the lenient range, from which a non-integer was
        # silently dropped.
        "docinfo: check a byte range a non-integer was dropped from",
        "src/docinfo.rs",
        "        if numbers.len() == range.len() {",
        "        if !numbers.is_empty() {",
        "a_byte_range_with_a_non_integer_in_it_is_not_checked",
    ),
    Mutation(
        # A fresh budget per signature rather than one per document.
        "docinfo: give each signature its own hashing budget",
        "src/docinfo.rs",
        "            limits,\n            budget,\n            judging,\n            archive,\n",
        "            limits,\n            &mut budget.clone(),\n            judging,\n            archive,\n",
        "the_hashing_budget_is_shared_by_every_signature_of_a_document",
    ),
]

# --- whether a timestamp token is sound and covers what it is attached to ---
#
# `integrity/token.rs`, `docinfo.rs` and `trust.rs`, 2026-09-28. Each removes
# one check the token verdict, the attested time or the authority's standing
# rests on, and names the test built so only that check can fail it --- the
# tokens come from `integrity/test_tsa.rs`, one fault apiece.
ROWS += [
    Mutation(
        # Never compare the imprint: a sound token of different data reads as
        # a timestamp of this signature.
        "token: never compare the imprint",
        "src/integrity/token.rs",
        "    if hash.digest(&pieces) != imprint.hashed_message.as_bytes() {",
        "    if false {",
        "a_token_of_different_data_is_altered",
    ),
    Mutation(
        # Believe a `TSTInfo` whatever it hashes to: the statement in hand is
        # not the one the authority signed, and reads as the one it did.
        "token: believe a TSTInfo that is not the one signed",
        "src/integrity/token.rs",
        "    let own = signer.judge(&[content.as_slice()], budget);",
        "    let own = { let o = signer.judge(&[content.as_slice()], budget); if o.verdict == Verdict::Altered { Integrity { verdict: Verdict::Intact, ..o } } else { o } };",
        "a_tst_info_changed_after_signing_is_broken_not_altered",
    ),
    Mutation(
        # Report a changed `TSTInfo` as a timestamp of different data, which
        # says the token is the authority's when it is not.
        "token: call a changed TSTInfo altered rather than broken",
        "src/integrity/token.rs",
        """            return Integrity {
                verdict: Verdict::Broken,
                ..own
            };""",
        """            return own;""",
        "a_tst_info_changed_after_signing_is_broken_not_altered",
    ),
    Mutation(
        # Never test the token's own signature.
        "token: ignore the token's own signature",
        "src/integrity/token.rs",
        "    let own = signer.judge(&[content.as_slice()], budget);",
        "    let own = { let o = signer.judge(&[content.as_slice()], budget); if o.verdict == Verdict::Broken { Integrity { verdict: Verdict::Intact, ..o } } else { o } };",
        "a_corrupted_token_signature_is_broken",
    ),
    Mutation(
        # Read the imprint before the signature is known to hold: a forged
        # token of other data then reads as merely altered.
        "token: judge the imprint before the token's signature",
        "src/integrity/token.rs",
        "    let own = signer.judge(&[content.as_slice()], budget);",
        "    if imprint_of(&content).is_some_and(|i| match target { Target::Signature(v) => Hash::from_oid(&i.hash_algorithm.oid.to_string()).is_some_and(|h| h.digest(&[v]) != i.hashed_message.as_bytes()), Target::Range(_) => false }) { return Integrity { verdict: Verdict::Altered, ..Integrity::default() }; }\n    let own = signer.judge(&[content.as_slice()], budget);",
        "a_broken_token_is_broken_even_when_its_imprint_is_also_wrong",
    ),
    Mutation(
        # Skip the ESS binding altogether.
        "token: skip the ESS binding",
        "src/integrity/token.rs",
        "    if let Err(why) = binds(attributes.as_ref(), certificate.as_deref()) {",
        "    if let Err(why) = Ok::<(), Why>(()).and(Ok(drop((attributes, certificate)))) {",
        "a_token_with_no_ess_binding_is_not_checked",
    ),
    Mutation(
        # Accept a token that carries neither form of the binding.
        "token: accept a token with no binding at all",
        "src/integrity/token.rs",
        "    if v1.is_none() && v2.is_none() {",
        "    if false {",
        "a_token_with_no_ess_binding_is_not_checked",
    ),
    Mutation(
        # Accept a binding whose hash names another certificate.
        "token: accept a binding naming another certificate",
        "src/integrity/token.rs",
        "    if hash.digest(&[certificate]) == stated {",
        "    if true {",
        "a_binding_naming_another_certificate_is_not_checked",
    ),
    Mutation(
        # Call an SHA-1 imprint intact.
        "token: treat an SHA-1 imprint as strong",
        "src/integrity/token.rs",
        "    let weak = sha1_signed || hash == Hash::Sha1;",
        "    let weak = sha1_signed;",
        "an_sha1_imprint_is_weak_and_never_intact",
    ),
    Mutation(
        # Call a token signed over SHA-1 intact.
        "token: treat an SHA-1 token signature as strong",
        "src/integrity/token.rs",
        "    let weak = sha1_signed || hash == Hash::Sha1;",
        "    let weak = hash == Hash::Sha1;",
        "an_sha1_token_signature_is_weak_and_names_sha1",
    ),
    Mutation(
        # Read any CMS as a token, whatever its content type says it carries.
        "token: read a CMS that is not a token as one",
        "src/integrity/token.rs",
        "    if content_type(token).as_deref() != Some(TST_INFO) {",
        "    if content_type(token).is_none() {",
        "a_cms_that_is_not_a_token_is_not_read_as_one",
    ),
    Mutation(
        # Hash a document timestamp's whole range whatever the budget says.
        "token: ignore the hashing budget",
        "src/integrity/token.rs",
        "    if cost > *budget {",
        "    if false {",
        "a_budget_smaller_than_the_target_refuses_before_hashing",
    ),
    Mutation(
        # Check a document timestamp over whatever range it states, without
        # the rule that the hole is exactly its own value.
        "integrity: check a document timestamp over a range nobody vouched for",
        "src/integrity.rs",
        """    let Some(pieces) = covered(bytes, range, contents) else {
        return Integrity::unchecked(Why::Range);
    };""",
        """    let Some(pieces) = covered(bytes, range, contents).or_else(|| {
        let [_, first, second, last] = range else { return None };
        let (first, second, last) = (usize::try_from(*first).ok()?, usize::try_from(*second).ok()?, usize::try_from(*last).ok()?);
        (document_timestamp && second + last <= bytes.len()).then(|| [&bytes[..first], &bytes[second..second + last]])
    }) else {
        return Integrity::unchecked(Why::Range);
    };""",
        "a_document_timestamp_whose_range_does_not_frame_its_value_is_not_checked",
    ),
    Mutation(
        # Check a document timestamp with the detached computation.
        "integrity: check a document timestamp as a detached signature",
        "src/integrity.rs",
        "    if document_timestamp {\n        return token::check(",
        "    if false {\n        return token::check(",
        "a_document_timestamp_over_its_range_is_intact",
    ),
    Mutation(
        # Call every token's time attested: the verdict is computed and then
        # not consulted before the time is believed.
        "docinfo: call a broken token's time attested",
        "src/docinfo.rs",
        "    let attested = attributable(&integrity);",
        "    let attested = true;",
        "a_broken_tokens_time_is_not_attested_and_its_authority_not_asked",
    ),
    Mutation(
        # Ask about the authority of a token nothing vouches for.
        "docinfo: ask about a broken token's authority",
        "src/docinfo.rs",
        "    let trust = attested.then(|| match archive {",
        "    let trust = Some(()).map(|()| match archive {",
        "a_broken_tokens_time_is_not_attested_and_its_authority_not_asked",
    ),
    Mutation(
        # The same rule on the document timestamp's path, which copies the
        # field's verdict rather than computing it twice.
        "docinfo: call a document timestamp's time attested whatever its verdict",
        "src/docinfo.rs",
        "                    attested: attributable(&integrity),",
        "                    attested: true,",
        "a_document_timestamp_over_changed_bytes_attests_nothing",
    ),
    Mutation(
        # Ask about a document timestamp's authority as a document signer.
        "docinfo: judge a document timestamp's authority for signing documents",
        "src/docinfo.rs",
        '    if kind == "ETSI.RFC3161" {',
        "    if false {",
        "a_document_timestamps_signer_is_trusted_for_timestamping",
    ),
    Mutation(
        # Check the token against the signature's DER, tag and length and all,
        # rather than the value octets RFC 3161 Appendix A names.
        "docinfo: imprint the signature's DER rather than its value",
        "src/docinfo.rs",
        "        crate::integrity::token::Target::Signature(signer.signature.as_bytes()),",
        "        crate::integrity::token::Target::Signature(&signer.signature.to_der().unwrap_or_default()),",
        "a_sound_token_over_the_signature_value_attests_its_time",
    ),
    Mutation(
        # Trust an authority for timestamping whatever purpose it names.
        "trust: accept any purpose for a timestamp authority",
        "src/trust.rs",
        "                && matches!(usage.0.as_slice(), [only] if only.to_string() == TIMESTAMPING)",
        "                && !usage.0.is_empty()",
        "a_timestamp_authority_must_name_timestamping_alone_and_critical",
    ),
    Mutation(
        # Accept timestamping named among other purposes, as pyHanko does.
        "trust: accept timestamping among other purposes",
        "src/trust.rs",
        "                && matches!(usage.0.as_slice(), [only] if only.to_string() == TIMESTAMPING)",
        "                && usage.0.iter().any(|named| named.to_string() == TIMESTAMPING)",
        "a_timestamp_authority_must_name_timestamping_alone_and_critical",
    ),
    Mutation(
        # Accept a timestamping purpose the certificate does not mark critical.
        "trust: accept a timestamping purpose not marked critical",
        "src/trust.rs",
        "            extension.critical\n",
        "            true\n",
        "a_timestamp_authority_must_name_timestamping_alone_and_critical",
    ),
    Mutation(
        # Let a timestamp authority's certificate state no purpose, as a
        # document signer's may.
        "trust: let a timestamp authority state no purpose",
        "src/trust.rs",
        "        return Some(purpose == Purpose::Documents);",
        "        return Some(true);",
        "a_timestamp_authority_must_name_timestamping_alone_and_critical",
    ),
    Mutation(
        # Give an authority's refusal the document signer's reason.
        "trust: say an authority is not a document signer",
        "src/trust.rs",
        """            Purpose::Timestamping => Doubt::Timestamping,
        };
        return answer(Standing::Untrusted, Some(doubt));
    }
    answer(standing, None)""",
        """            Purpose::Timestamping => Doubt::Purpose,
        };
        return answer(Standing::Untrusted, Some(doubt));
    }
    answer(standing, None)""",
        "a_timestamp_authority_must_name_timestamping_alone_and_critical",
    ),
]
