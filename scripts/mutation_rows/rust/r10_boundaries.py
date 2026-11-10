"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.

The guards at the edges of what tpdf accepts and writes: what `verify` will
not certify, what a merge, a sweep or a redaction leaves behind, what a move or
a rename refuses, what a print, a compression sample or a SyncTeX file may
claim, the order a tagged page is read in, the pushed runs of the text editor,
and the worker boundary on each platform. Each is named by its area like its
neighbours, so `--only 'verify:'` runs the verdict's guards and
`--only 'worker:'` the boundary's.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- verify: what withholds the verdict ---------------------------------------
ROWS += [
    Mutation(
        # A `/Filter` that is an indirect reference makes `filters()` fail, and
        # the guard told that apart from a stream with no filter at all. Read as
        # "no filter", the encoded bytes are scanned as text and pass.
        "verify: an indirect filter read as no filter",
        "src/verify.rs",
        '                    Err(_) if !stream.dict.has(b"Filter") => Vec::new(),',
        "                    Err(_) if true => Vec::new(),",
        "a_stream_whose_filter_is_an_indirect_reference_withholds_the_verdict",
    ),
    Mutation(
        # A composite font draws glyph ids, not the words, so a byte scan for
        # the redacted words cannot find them and proves nothing.
        "verify: a composite font certified by the byte scan",
        "src/verify.rs",
        "            if doc.objects.values().any(names_a_composite_font) {",
        "            if false && doc.objects.values().any(names_a_composite_font) {",
        "a_file_with_a_composite_font_is_not_certified_by_the_byte_scan",
    ),
    Mutation(
        "verify: an embedded file let through",
        "src/verify.rs",
        "            if held.embedded {",
        "            if false && held.embedded {",
        "an_embedded_file_withholds_the_verdict",
    ),
    Mutation(
        "verify: an application's private data let through",
        "src/verify.rs",
        "            if held.private {",
        "            if false && held.private {",
        "an_applications_private_data_withholds_the_verdict",
    ),
]

# --- merge, sweep and redaction: what is left behind ---------------------------
ROWS += [
    Mutation(
        # The widget keeps its `/Parent`, so the imported page carries the
        # whole field tree, and with it every other page's answers.
        "merge: an imported widget keeps its parent field",
        "src/merge.rs",
        '    out.remove(b"Parent");\n',
        "",
        "importing_one_page_of_a_form_does_not_bring_the_other_pages_answers",
    ),
    Mutation(
        # A form XObject that draws through the page's own resources object
        # names what it draws there, so pruning that object by the page's use
        # alone takes the form's fonts and images away.
        "unused: prune resources a form shares with its page",
        "src/unused.rs",
        "        if group\n            .via\n",
        "        if false\n            && group\n            .via\n",
        "a_form_that_shares_the_pages_resources_object_keeps_what_it_draws",
    ),
    Mutation(
        # An annotation written inline in `/Annots` has no object to forget, so
        # it stays over the region with its contents unless this takes it.
        "redaction: an inline annotation over the region left in place",
        "src/save.rs",
        "        done.annots += redact::take_inline_annots(doc, page, &redaction.areas);",
        "        let _ = &redaction.areas;",
        "a_redacted_page_does_not_keep_its_thumbnail",
    ),
    Mutation(
        # The thumbnail is a picture of the page before the redaction, and a
        # viewer may show it in place of the page.
        "redaction: the page's thumbnail kept",
        "src/save.rs",
        '            dictionary.remove(b"Thumb");',
        "            let _ = dictionary;",
        "a_redacted_page_does_not_keep_its_thumbnail",
    ),
    Mutation(
        "redaction: every inline annotation taken, over the region or not",
        "src/redact.rs",
        "                matches!(rect_of(doc, annot), Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)))",
        "                true || matches!(rect_of(doc, annot), Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)))",
        "an_annotation_written_inline_in_annots_over_a_region_is_taken",
    ),
    Mutation(
        # The read-back must read the file just written. A handle on another
        # file reads a document that was never written as if it had been.
        "save: a read-back handle on another file accepted",
        "src/save.rs",
        "    if FileId::of(&reading) != Some(writing_to) {",
        "    if false && FileId::of(&reading) != Some(writing_to) {",
        "a_read_back_handle_on_another_file_is_refused_before_anything_is_written",
    ),
    Mutation(
        # `/RC` is the rich-text copy of a comment's body, and a viewer that
        # prefers it shows the old words after an edit.
        "comments: an edited comment keeps its rich-text body",
        "src/save/marks.rs",
        '    dictionary.remove(b"RC");\n',
        "",
        "an_edited_comment_keeps_no_rich_text_copy_of_its_old_body",
    ),
    Mutation(
        # A replaced answer leaves its old appearance stream unreachable, and
        # only a plan that says it may leave orphans is swept.
        "sweep: a form answer said to leave no orphans",
        "src/edits.rs",
        "            || !forms.is_empty()\n",
        "",
        "an_answer_that_replaces_another_leaves_no_stream_drawing_the_first",
    ),
    Mutation(
        "sweep: an edited comment said to leave no orphans",
        "src/edits.rs",
        "            || !notes.is_empty()\n",
        "",
        "an_edited_comments_old_rich_text_stream_leaves_the_file",
    ),
]

# --- moves, glyph rows, signing and renames --------------------------------------
ROWS += [
    Mutation(
        "docmodel: a move to a coordinate that is not finite accepted",
        "src/docmodel.rs",
        "        if !finite {\n            return Err(Refusal::EmptyMark);",
        "        if false && !finite {\n            return Err(Refusal::EmptyMark);",
        "a_move_that_would_leave_a_coordinate_not_finite_is_refused",
    ),
    Mutation(
        "objects: a glyph whose offset is not finite counted in the row",
        "src/objects.rs",
        "            if !pen.is_finite() || !off.is_finite() || ",
        "            if !pen.is_finite() || ",
        "an_object_that_is_not_a_row_of_glyphs_running_forwards_has_no_glyphs",
    ),
    Mutation(
        # Long-term signing asks the authority anyway, so excusing it from
        # the vouching seals a token nobody vouches for.
        "sign: a long-term signing excused from vouching for its token",
        "src/sign_finish.rs",
        "if let (Some(cms), false) = (&stamped, asking.any_authority) {",
        "if let (Some(cms), false) = (&stamped, long_term || asking.any_authority) {",
        "a_long_term_signing_does_not_seal_a_token_from_an_authority_nobody_vouches_for",
    ),
    Mutation(
        # A field under a group with no name of its own has the same full name
        # as a top-level field, so taking the name makes two fields one.
        "form rename: a name taken from a field under an unnamed group",
        "src/formedit.rs",
        "        if !taken && !named_above(doc, *field) {",
        "        if !taken && false {",
        "a_rename_cannot_take_the_name_of_a_field_under_an_unnamed_group",
    ),
    Mutation(
        "form rename: every group above a field read as unnamed",
        "src/formedit.rs",
        "        if named {\n            return true;\n        }",
        "        if named {\n            return false;\n        }",
        "a_field_under_an_unnamed_group_cannot_take_a_top_level_name",
    ),
]

# --- printing, compression, SyncTeX, reading order and the command line --------
ROWS += [
    Mutation(
        # The clock runs through a call that waits on the reader, so a Save
        # dialog left open reads as a stuck queue.
        "print: the clock left running while the print system asks",
        "src/print.rs",
        "    pub fn asking<R>(&self, work: impl FnOnce() -> R) -> R {\n        self.paused(work)",
        "    pub fn asking<R>(&self, work: impl FnOnce() -> R) -> R {\n        work()",
        "a_print_that_stops_moving_is_reported_once_and_one_that_moves_is_not",
    ),
    Mutation(
        "print: a printer the webview names taken without the installed list",
        "src/commands/print.rs",
        "        return if installed.iter().any(|name| name == asked) {",
        "        return if true {",
        "a_printer_the_webview_names_is_held_to_the_installed_list",
    ),
    Mutation(
        "compress: a sample whose size is not its pixels accepted",
        "src/commands/compress.rs",
        "    sized(sample)?;\n",
        "",
        "a_sample_whose_size_is_not_its_pixels_is_refused",
    ),
    Mutation(
        "synctex: inputs past the count read on",
        "src/synctex.rs",
        "                    if found.len() >= MAX_INPUTS {",
        "                    if false && found.len() >= MAX_INPUTS {",
        "inputs_past_the_bounds_are_refused_and_not_cut_short",
    ),
    Mutation(
        "synctex: an input name past the text bound read on",
        "src/synctex.rs",
        "                    if text > MAX_INPUT_TEXT {",
        "                    if false && text > MAX_INPUT_TEXT {",
        "inputs_past_the_bounds_are_refused_and_not_cut_short",
    ),
    Mutation(
        "synctex: a header scale that is not a number accepted",
        "src/synctex.rs",
        "header.filter(|(_, n)| !n.is_finite())",
        "header.filter(|(_, n)| false && !n.is_finite())",
        "a_scale_that_is_not_a_number_and_a_size_that_overflows_place_no_box",
    ),
    Mutation(
        "synctex: a box height that overflows added unchecked",
        "src/synctex.rs",
        "        let tall = h.checked_add(d)?;",
        "        let tall = h + d;",
        "a_scale_that_is_not_a_number_and_a_size_that_overflows_place_no_box",
    ),
    Mutation(
        "reading: every tagged fragment put in the first tag's place",
        "src/reading.rs",
        "            if let Some(list) = out.get_mut(at) {",
        "            if let Some(list) = out.get_mut(0) {",
        "tags_that_cover_the_page_decide_its_order_and_the_geometry_does_not",
    ),
    Mutation(
        # The usage text is one of three lists of the commands that exit 1,
        # and naming no command in place of `comments` is the drift the test
        # holds the three lists against.
        "cli: the usage text names no command where comments exits 1",
        "src/cli.rs",
        "left some uncompared, or comments could",
        "left some uncompared, or the scan could",
        "the_three_lists_of_exit_code_1_name_the_same_commands",
    ),
]

# --- text editor: pushed runs and kerned centred lines ---------------------------
ROWS += [
    Mutation(
        "textedit: a pushed run written without a layout",
        "src/textedit.rs",
        "        if change.layout.is_none() && shifts.contains_key(&(change.page, change.operator)) {",
        "        if false && change.layout.is_none() && shifts.contains_key(&(change.page, change.operator)) {",
        "a_pushed_run_edited_without_a_layout_is_refused_or_moved",
    ),
    Mutation(
        "textedit push: annotations do not stop the push",
        "src/textedit/layout.rs",
        "            None => fixed.push((rect, Room::Drawn)),",
        "            None => {}",
        "a_push_stops_at_text_under_an_annotation",
    ),
    Mutation(
        "textedit push: an unreadable annotation list read as an empty one",
        "src/textedit/layout.rs",
        "    let Some(annotations) = page.annotations.as_deref() else {",
        "    let Some(annotations) = page.annotations.as_deref().or(Some(&[])) else {",
        "a_push_stops_at_text_under_an_annotation",
    ),
    Mutation(
        "textedit push: no link held by a pushed run",
        "src/textedit/layout.rs",
        "        let under = annotation.link.filter(|_| {",
        "        let under = annotation.link.filter(|_| false).filter(|_| {",
        "a_link_over_a_pushed_run_moves_with_it",
    ),
    Mutation(
        "textedit push: any link counted as held",
        "src/textedit/layout.rs",
        "        let under = annotation.link.filter(|_| {",
        "        let under = annotation.link.filter(|_| true || {",
        "a_link_over_a_pushed_run_moves_with_it",
    ),
    Mutation(
        "textedit push: a held link not moved",
        "src/textedit/layout.rs",
        "        carried.extend(\n            free.links",
        "        carried.extend(\n            free.links.iter().take(0).copied().collect::<Vec<_>>()",
        "a_link_over_a_pushed_run_moves_with_it",
    ),
    Mutation(
        # The old measure: the source laid out afresh from glyph widths, which
        # leaves out the kerns the line was drawn with.
        "textedit centred: the source measured from its glyph widths",
        "src/textedit/layout.rs",
        "    let original_width = as_drawn(\n        &original_metrics,\n        &run.text,\n        run.advance,\n        (run.size, source_spacing, source_word_spacing),\n        gap,\n    );",
        "    let original_width = original_metrics.gapped_layout(run.text.trim_end_matches(' '), run.size, source_spacing, source_word_spacing, gap).map_or(run.advance, |(advance, ink)| advance.max(ink[1] + (-ink[0]).max(0.)));",
        "textedit_a_kerned_centred_line_keeps_its_centre",
    ),
    Mutation(
        "textedit centred: trailing spaces counted as drawn",
        "src/textedit/layout.rs",
        "            let advance = advance - (whole - words);",
        "            let advance = advance - 0. * (whole - words);",
        "textedit_a_kerned_centred_line_keeps_its_centre",
    ),
    Mutation(
        "textedit centred: a kept plan measured by what it used",
        "src/textedit/layout.rs",
        "                Some((_, advance, ..)) => as_drawn(",
        "                Some(_) if true => *used,\n                Some((_, advance, ..)) => as_drawn(",
        "textedit_a_kerned_centred_line_keeps_its_centre",
    ),
    Mutation(
        "textedit centred: the ink not moved by the kerns",
        "src/textedit/layout.rs",
        "            advance.max(ink[1] + (advance - words) + (-ink[0]).max(0.))",
        "            advance.max(ink[1] + (-ink[0]).max(0.))",
        "textedit_a_kerned_centred_line_keeps_its_centre",
    ),
]

# --- the worker boundary -----------------------------------------------------------
# What a worker's reply may claim, what its process may do, and the handles it
# is given. A row whose code compiles on one platform only names it, though its
# test compiles on more: elsewhere the edit lands in code that is not built and
# the row would report SURVIVED.
ROWS += [
    Mutation(
        "worker: a copy past the mapping cut short rather than refused",
        "src/worker_shm.rs",
        "            .get(..length)\n            .map(<[u8]>::to_vec)",
        "            .get(..length.min(self.len))\n            .map(<[u8]>::to_vec)",
        "copying_past_the_mapping_is_refused",
        only_on="windows",
    ),
    Mutation(
        "worker: an open reply past the page bound accepted",
        "src/workers.rs",
        "    if page_count > MAX_PAGES {",
        "    if false && page_count > MAX_PAGES {",
        "an_open_reply_claiming_more_pages_than_any_document_has_is_refused",
    ),
    Mutation(
        "worker: an eager open reply with too few pages accepted",
        "src/workers.rs",
        "    } else {\n        pages.len() == page_count\n    };",
        "    } else {\n        true\n    };",
        "an_open_reply_claiming_more_pages_than_any_document_has_is_refused",
    ),
    Mutation(
        "worker: a lazy open reply accepted when none was asked for",
        "src/workers.rs",
        "        asked_lazy && pages.len() == page_count.min(1)",
        "        pages.len() == page_count.min(1)",
        "an_open_reply_claiming_more_pages_than_any_document_has_is_refused",
    ),
    Mutation(
        "worker: a reply's shown host left as the worker wrote it",
        "src/webopen.rs",
        "        for target in shown {",
        "        for target in shown.into_iter().take(0) {",
        "a_reply_whose_shown_host_is_not_its_addresss_host_is_shown_the_real_one",
    ),
    Mutation(
        "worker: the children of an outline entry not reached",
        "src/outline.rs",
        "            walk(&mut item.children, out);\n",
        "",
        "every_target_of_a_nested_outline_is_reached",
    ),
    Mutation(
        "worker: a contained worker allowed to change a file's mode",
        "src/sandbox_linux.rs",
        "    // Mode.\n    libc::SYS_fchmod,\n",
        "    // Mode.\n",
        "the_filter_refuses_changing_a_files_mode_owner_attributes_and_times",
        only_on="linux",
    ),
    Mutation(
        "worker: a contained worker allowed to change a file's times",
        "src/sandbox_linux.rs",
        "    // Timestamps.\n    libc::SYS_utimensat,\n",
        "    // Timestamps.\n",
        "the_filter_refuses_changing_a_files_mode_owner_attributes_and_times",
        only_on="linux",
    ),
    Mutation(
        "worker: a contained worker allowed to set an attribute by directory",
        "src/sandbox_linux.rs",
        "    SYS_SETXATTRAT,\n    SYS_REMOVEXATTRAT,\n    // Timestamps.",
        "    SYS_REMOVEXATTRAT,\n    // Timestamps.",
        "the_filter_refuses_changing_a_files_mode_owner_attributes_and_times",
        only_on="linux",
    ),
    Mutation(
        "worker: an OCR reply read without its limit",
        "src/ocr_worker.rs",
        "            match crate::worker_proto::read_reply_line(&mut reader, limit) {",
        "            match crate::worker_proto::read_reply_line(&mut reader, u64::MAX) {",
        "an_ocr_reply_longer_than_the_limit_ends_the_worker",
    ),
    Mutation(
        # A withdrawal posted with a blocking send waits on a worker that is
        # not reading, which is the wait the bounded queue exists to refuse.
        "worker: a withdrawal waits for room in the queue",
        "src/workers.rs",
        "        self.tx.try_send(item).is_ok()",
        "        self.tx.send(item).is_ok()",
        "a_withdrawal_never_waits_on_a_worker_that_is_not_reading",
    ),
    Mutation(
        "worker: a worker's job allowed to read the clipboard",
        "src/sandbox_win.rs",
        "            UIRestrictionsClass: UI_LIMITS,\n",
        "            UIRestrictionsClass: UI_LIMITS & !JOB_OBJECT_UILIMIT_READCLIPBOARD,\n",
        "a_workers_job_refuses_it_the_clipboard",
        only_on="windows",
    ),
    Mutation(
        "worker: a tile section born inheritable",
        "src/worker_shm.rs",
        '        if mapping.is_null() {\n            return Err(format!(\n                "shm create failed: {}",',
        '        let _ = unsafe { crate::sandbox_win::make_inheritable(mapping as _) };\n        if mapping.is_null() {\n            return Err(format!(\n                "shm create failed: {}",',
        "a_section_handle_is_not_inheritable_outside_a_spawn",
        only_on="windows",
    ),
    Mutation(
        "worker: a read-only document section duplicated inheritable",
        "src/worker_shm.rs",
        "                &raw mut reader,\n                FILE_MAP_READ,\n                0,\n                0,",
        "                &raw mut reader,\n                FILE_MAP_READ,\n                1,\n                0,",
        "a_section_handle_is_not_inheritable_outside_a_spawn",
        only_on="windows",
    ),
    Mutation(
        "worker: a backend that never started answers as locked",
        "src/render.rs",
        "                        reason: error.to_string(),\n                        locked: false,\n                    })),\n                    $(",
        "                        reason: error.to_string(),\n                        locked: true,\n                    })),\n                    $(",
        "a_backend_that_never_started_answers_every_job_with_its_reason",
    ),
    Mutation(
        "worker: a backend that never started answers without its reason",
        "src/render.rs",
        "$(Job::$variant { reply, .. } => reply(Err(error.to_string())),)+",
        "$(Job::$variant { reply, .. } => reply(Err(String::new())),)+",
        "a_backend_that_never_started_answers_every_job_with_its_reason",
    ),
    # --- save.rs: the rename that puts a staged file in place --------------------
    # The first takes about a minute: the test lets go of the file after 30 s,
    # and the rename that then lands is what makes it red.
    Mutation(
        "commit: a refused rename tried again for ever",
        "src/save.rs",
        "                    && attempt < COMMIT_RETRIES =>",
        "                    && attempt < u32::MAX =>",
        "a_target_held_open_briefly_is_still_replaced",
        only_on="windows",
    ),
    Mutation(
        "commit: a refused rename never tried again",
        "src/save.rs",
        "                    && attempt < COMMIT_RETRIES =>",
        "                    && attempt > COMMIT_RETRIES =>",
        "a_target_held_open_briefly_is_still_replaced",
        only_on="windows",
    ),
]
