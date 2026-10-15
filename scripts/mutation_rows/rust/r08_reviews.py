"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- the 2026-10-09 review's Rust findings ------------------------------------
# What changed after a signature (`read_appendix`): a stream replaced at its
# old length, and an object a later cross-reference section frees. Then a page
# that shares its content stream under a redaction, the release a timed-out
# read-back waits for, and spacing alone after a glyph cut. Each is named by
# its area like its neighbours, so `--only 'freed:'` runs the reader of the
# free entries and `--only 'appendix:'` what `read_appendix` makes of them.
ROWS += [
    Mutation(
        # `lopdf` prints a stream without its bytes, so the comparison by
        # `Debug` alone calls a content stream written again at its old
        # length unchanged, and `verify --strict` passes it.
        "appendix: compare two streams by what lopdf prints of them",
        "src/docinfo.rs",
        "        if before.content != after.content {",
        "        if false {",
        "a_content_stream_replaced_with_the_same_length_touches_its_page",
    ),
    Mutation(
        # A listing needs nothing the page draws from to have changed, and
        # its content stream is the first of those. Left out of what the page
        # refers to, a stream replaced beside a new signature field reads
        # "the page's content is unchanged".
        "appendix: call a page a listing whatever became of its content stream",
        "src/docinfo.rs",
        '        if key.as_slice() != b"Annots" {',
        '        if key.as_slice() != b"Annots" && key.as_slice() != b"Contents" {',
        "a_same_length_replacement_beside_a_new_signature_field_is_not_a_listing",
    ),
    Mutation(
        # Until 2026-10-09 that stream was all a listing was checked for: a
        # form, a font or a graphics state replaced in the page's resources
        # beside a new signature field passed `verify --strict`.
        "appendix: call a page a listing whatever became of what it draws with",
        "src/docinfo.rs",
        "        listings.retain(|(page, _)| !redrawn.contains(page));",
        "        drop(redrawn);",
        "what_a_page_draws_replaced_beside_a_new_signature_field_is_not_a_listing",
    ),
    Mutation(
        # The annotations a page already had are the page's: a field's value
        # written again beside the new signature changes what its widget shows.
        "appendix: follow none of the annotations a page already had",
        "src/docinfo.rs",
        "        Some((timestamp, self.apart_from_signing(after, &kept).0))",
        "        Some((timestamp, self.apart_from_signing(after, &[]).0))",
        "an_annotation_changed_beside_a_new_signature_field_is_not_a_listing",
    ),
    Mutation(
        # The field a page gained arrives with its appearance stream, and the
        # page reaches both. Followed like any other entry, every visible
        # second signature would read as a rewritten page.
        "appendix: follow the field a page gained to what it brings",
        "src/docinfo.rs",
        "        Some((timestamp, self.apart_from_signing(after, &kept).0))",
        "        Some((timestamp, self.apart_from_signing(after, &new.iter().collect::<Vec<_>>()).0))",
        "a_visible_second_signature_is_a_listing_whatever_its_widget_brings",
    ),
    Mutation(
        # The page's own object is one of the changes, and `PageRead::listing`
        # has already compared it entry by entry.
        "appendix: count a page's own rewrite as something it draws from",
        "src/docinfo.rs",
        "            .filter(|id| !apart_from_signing.contains_key(id))",
        "            .filter(|_| true)",
        "a_visible_second_signature_is_a_listing_whatever_its_widget_brings",
    ),
    Mutation(
        # What unmakes a listing is that the page draws the object, and not
        # that the revision replaced one.
        "appendix: unmake a listing whenever another object was replaced",
        "src/docinfo.rs",
        "        listings.retain(|(page, _)| !redrawn.contains(page));",
        "        listings.retain(|_| out.replaced == 1 && redrawn.is_empty());",
        "an_object_no_page_draws_replaced_beside_a_new_signature_field_leaves_the_listing",
    ),
    Mutation(
        # The signature's widget, written under a number the signed page's
        # resources already named: the page draws with it and lists it.
        "appendix: leave a page's own resources out of what it draws from",
        "src/docinfo.rs",
        '        if key.as_slice() != b"Annots" {',
        '        if key.as_slice() != b"Annots" && key.as_slice() != b"Resources" {',
        "a_new_field_the_page_also_draws_with_is_not_a_listing",
    ),
    Mutation(
        # `lopdf` keeps no free entry, so an object a later section frees is
        # still in the whole parse as it was. Only the sections say it went.
        "appendix: take what lopdf still holds for what a reader still finds",
        "src/docinfo.rs",
        "        .filter(|id| freed.contains(&id.0) || !whole.objects.contains_key(id))",
        "        .filter(|id| !whole.objects.contains_key(id))",
        "an_object_freed_after_signing_touches_the_page_that_drew_it",
    ),
    Mutation(
        "appendix: miss an object the whole document no longer has",
        "src/docinfo.rs",
        "        .filter(|id| freed.contains(&id.0) || !whole.objects.contains_key(id))",
        "        .filter(|id| freed.contains(&id.0))",
        "an_object_the_whole_document_no_longer_has_is_removed",
    ),
    Mutation(
        # A section that cannot be followed may be the one that frees a
        # page's content. "Nothing was freed" is the reassuring answer.
        "appendix: read sections that cannot be followed as freeing nothing",
        "src/docinfo.rs",
        "    let Some(freed) = freed::after(bytes, end, whole.xref_start, MAX_DECODE) else {\n"
        "        return unread;\n"
        "    };",
        "    let freed = freed::after(bytes, end, whole.xref_start, MAX_DECODE).unwrap_or_default();",
        "a_section_that_cannot_be_followed_makes_the_appendix_unread",
    ),
    Mutation(
        "appendix: report a page that lost what it drew as only listing a field",
        "src/docinfo.rs",
        "        listings.retain(|(page, _)| !lost.contains(page));\n",
        "",
        "a_page_that_lost_what_it_drew_is_not_reported_as_only_listing_a_field",
    ),
    Mutation(
        "appendix: count what was removed and touch no page for it",
        "src/docinfo.rs",
        "        touched.extend(lost);",
        "        drop(lost);",
        "strict_fails_a_signature_followed_by_a_freed_content_stream",
    ),
    Mutation(
        # poppler honours a free entry in a classic table where PDFium 8066
        # does not, so it is counted in either form.
        "freed: read a free entry of a classic table as an object in use",
        "src/docinfo/freed.rs",
        '                b"f" => false,',
        '                b"f" => true,',
        "an_object_freed_in_a_classic_table_is_counted_too",
    ),
    Mutation(
        "freed: read every entry of a stream section as an object in use",
        "src/docinfo/freed.rs",
        "            entries.insert(id, kind == 1 || kind == 2);",
        "            entries.insert(id, true);",
        "a_freed_object_is_followed_over_the_signed_document",
    ),
    Mutation(
        "freed: read an object in an object stream as free",
        "src/docinfo/freed.rs",
        "            entries.insert(id, kind == 1 || kind == 2);",
        "            entries.insert(id, kind == 1);",
        "a_free_entry_is_read_in_either_form",
    ),
    Mutation(
        "freed: let the oldest section that lists an object decide",
        "src/docinfo/freed.rs",
        "            decided.entry(id).or_insert(in_use);",
        "            decided.insert(id, in_use);",
        "the_newest_section_decides_and_a_signed_one_is_left_alone",
    ),
    Mutation(
        # A section before the signed range's end was signed. What it frees
        # is the signed revision's and is not something an append did.
        "freed: follow the chain into the signed part",
        "src/docinfo/freed.rs",
        "    while at >= end {",
        "    loop {",
        "the_newest_section_decides_and_a_signed_one_is_left_alone",
    ),
    Mutation(
        # A hybrid table lists as free what its own stream holds in object
        # streams, so within one revision an object in use by either is in use.
        "freed: let a hybrid table's free entry stand over its own stream",
        "src/docinfo/freed.rs",
        "            let entry = entries.entry(id).or_insert(in_use);\n            *entry |= in_use;",
        "            entries.entry(id).or_insert(in_use);",
        "an_object_in_use_by_a_tables_own_stream_is_in_use",
    ),
    Mutation(
        "freed: stop the chain at a previous section it cannot read the place of",
        "src/docinfo/freed.rs",
        "        Ok(_) => None,",
        "        Ok(_) => Some(None),",
        "a_section_that_cannot_be_read_is_no_answer",
    ),
    Mutation(
        "freed: take the last of two equal keys in a trailer",
        "src/docinfo/freed.rs",
        "                    if dict.has(&key) {\n                        return None;\n                    }\n",
        "",
        "a_section_that_cannot_be_read_is_no_answer",
    ),
    Mutation(
        "freed: read a name's escapes as the characters they are written with",
        "src/docinfo/freed.rs",
        "            if byte != b'#' {",
        "            if true {",
        "an_escaped_key_is_the_key_it_spells",
    ),
    Mutation(
        "freed: read a deflated section's bytes as its entries",
        "src/docinfo/freed.rs",
        "        Ok(filters) if !filters.is_empty() => stream.decompressed_content_with_limit(limit).ok()?,",
        "        Ok(filters) if filters.is_empty() => stream.decompressed_content_with_limit(limit).ok()?,",
        "a_deflated_stream_is_decoded",
    ),
    Mutation(
        "verify: leave what was removed out of the report",
        "src/cli/verify.rs",
        "        removed: appendix.removed,",
        "        removed: 0,",
        "a_signature_reports_what_was_removed_after_it",
    ),
    Mutation(
        "verify: say 0 objects in front of a removal that is all there is",
        "src/cli/verify.rs",
        "    } else if arrived == 0 && appendix.removed > 0 {",
        "    } else if false {",
        "a_signature_reports_what_was_removed_after_it",
    ),
    Mutation(
        "verify: leave what was removed out of the sentence",
        "src/cli/verify.rs",
        "    match appendix.removed {\n        0 =>",
        "    match 0 {\n        0 =>",
        "a_signature_reports_what_was_removed_after_it",
    ),
    Mutation(
        # A repeated page shares its content stream with its copy. Rewriting
        # the object where it stands changes a page nobody marked, with no
        # fill drawn there and no line in the report.
        "redact: rewrite a content stream another page also names",
        "src/redact.rs",
        "        [only] if !named_by_another_page(doc, page, *only) => {",
        "        [only] => {",
        "a_redaction_on_one_of_two_pages_sharing_a_stream_leaves_the_other_page_as_it_was",
    ),
    Mutation(
        "redact: give every page a new content stream",
        "src/redact.rs",
        "        .any(|other| other != page && doc.get_page_contents(other).contains(&stream))",
        "        .any(|other| doc.get_page_contents(other).contains(&stream))",
        "a_redaction_on_one_of_two_pages_sharing_a_stream_leaves_the_other_page_as_it_was",
    ),
    Mutation(
        # The roll-back that follows a timeout is a `set_len`, which Windows
        # refuses while the file is mapped, and the thread still holds the
        # worker and its mapping when the process has only just been ended.
        "save_outside: return from a timed-out wait before the worker is released",
        "src/save_outside.rs",
        "            let _ = rx.recv_timeout(released_within);\n",
        "",
        "a_timed_out_wait_returns_only_after_the_worker_is_released",
    ),
    Mutation(
        # A thousand times the bound, which is what no bound looks like from
        # inside a test, for the reason the read-back's own bound gives.
        "save_outside: wait for a release a thousand times its bound",
        "src/save_outside.rs",
        "            let _ = rx.recv_timeout(released_within);",
        "            let _ = rx.recv_timeout(released_within * 1000);",
        "a_release_that_never_comes_is_not_waited_for_past_its_bound",
    ),
    Mutation(
        # `[-500] TJ` draws nothing and moves the pen. The gap a cut to the
        # end leaves goes to where PDFium placed the next show, which is after
        # that move: left in, the `TJ` moves the next show a second time.
        "glyph cut: leave spacing alone after a cut whose gap already holds it",
        "src/redact/glyph_cut.rs",
        "            drop_spacing_after(&mut content.operations, at);\n",
        "",
        "spacing_alone_after_a_cut_to_the_end_goes_into_the_gap",
    ),
    Mutation(
        "glyph cut: take spacing out after a cut that stops short of the end",
        "src/redact/glyph_cut.rs",
        "        if continues && to_the_end {",
        "        if continues {",
        "spacing_alone_after_a_cut_to_the_end_goes_into_the_gap",
    ),
    Mutation(
        "glyph cut: take spacing out before a show that is placed",
        "src/redact/glyph_cut.rs",
        "        if continues && to_the_end {",
        "        if to_the_end {",
        "spacing_alone_after_a_cut_to_the_end_goes_into_the_gap",
    ),
    Mutation(
        "glyph cut: take out the spacing after the next show too",
        "src/redact/glyph_cut.rs",
        '        !(here > at && here < next && operation.operator == "TJ")',
        '        !(here > at && operation.operator == "TJ")',
        "spacing_alone_after_a_cut_to_the_end_goes_into_the_gap",
    ),
]

# --- a place prepared for a signature, and a list of annotations held apart ---
# `PageRead` in `docinfo.rs`: a signature put in an empty signature field the
# signed revision already had is not a change to its page, and a new field
# listed in an `/Annots` that is an object of its own is the listing a page
# written again is. Each mutation was seen red by hand on 2026-10-09 before
# it was written here.
ROWS += [
    Mutation(
        # The append writes the field and its widget again and no page object.
        # The page reaches the changed widget through the `/Annots` entry it
        # always had, and until 2026-10-09 that was a rewritten page.
        "appendix: count a page for a signature put in a place it had empty",
        "src/docinfo.rs",
        "        } else if let Some(refers_to) = read.signed_places(after) {",
        "        } else if let Some(refers_to) = read.signed_places(after).filter(|_| false) {",
        "a_prepared_signature_field_signed_touches_no_page",
    ),
    Mutation(
        # A field's widgets are each on a page by that page's own `/Annots`.
        # Followed from the field, the signed widget is reached again.
        "appendix: reach a signed place's own field through the field's kids",
        "src/docinfo.rs",
        '                    key.as_slice() == b"Parent" || (depth > 0 && key.as_slice() == b"Kids");',
        '                    key.as_slice() == b"Parent";',
        "a_prepared_signature_field_signed_touches_no_page",
    ),
    Mutation(
        "appendix: reach a signed place's field as the widget's parent",
        "src/docinfo.rs",
        '                    key.as_slice() == b"Parent" || (depth > 0 && key.as_slice() == b"Kids");',
        '                    depth > 0 && key.as_slice() == b"Kids";',
        "a_prepared_signature_field_signed_touches_no_page",
    ),
    Mutation(
        # Where the place is and how large is the signed revision's word.
        "appendix: let a signer move the place it signs",
        "src/docinfo.rs",
        'const SIGNING_WRITES: [&[u8]; 4] = [b"V", b"AP", b"F", b"M"];',
        'const SIGNING_WRITES: [&[u8]; 5] = [b"V", b"AP", b"F", b"M", b"Rect"];',
        "a_prepared_signature_field_changed_beyond_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: compare nothing of a place that was signed",
        "src/docinfo.rs",
        "                && !same_apart_from(self.signed.get_dictionary(id).ok()?, now, writes)",
        "                && false",
        "a_prepared_signature_field_changed_beyond_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: let a signer lock the field above its widget",
        "src/docinfo.rs",
        'const SIGNING_WRITES_ABOVE: [&[u8]; 1] = [b"V"];',
        'const SIGNING_WRITES_ABOVE: [&[u8]; 2] = [b"V", b"Lock"];',
        "a_prepared_signature_field_changed_beyond_signing_is_a_rewritten_page",
    ),
    Mutation(
        # Only the signature arrives on a field above the widget.
        "appendix: let a signer write on the field above what it writes on the widget",
        "src/docinfo.rs",
        "            writes = &SIGNING_WRITES_ABOVE;\n",
        "",
        "a_prepared_signature_field_changed_beyond_signing_is_a_rewritten_page",
    ),
    Mutation(
        # The place has to have been empty when the document was signed.
        "appendix: take a signature replaced for a place signed",
        "src/docinfo.rs",
        '            || inherited(self.signed, was, b"V").is_some()\n',
        "",
        "a_signature_replaced_after_signing_is_a_rewritten_page",
    ),
    Mutation(
        # A text field answered after signing is a form answer changed.
        "appendix: take any field filled for a signature place",
        "src/docinfo.rs",
        "            || !is_signature_field(self.signed, was)\n",
        "",
        "a_prepared_text_field_answered_after_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: take any annotation for a signature's widget",
        "src/docinfo.rs",
        "        if !is_widget(self.signed, was)\n"
        "            || !is_signature_field(self.signed, was)",
        "        if !is_signature_field(self.signed, was)",
        "a_signature_put_in_what_is_no_widget_is_a_rewritten_page",
    ),
    Mutation(
        # A signature in a field has a `/ByteRange`; a dictionary without one
        # signs nothing.
        "appendix: take a dictionary that signs nothing for a signature",
        "src/docinfo.rs",
        '            && self.whole.get_dictionary(signature).ok()?.has(b"ByteRange"))',
        '            && self.whole.get_dictionary(signature).is_ok())',
        "a_prepared_signature_field_given_something_else_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: take a signature the signed revision had for a new one",
        "src/docinfo.rs",
        "        (!self.signed.objects.contains_key(&signature)\n",
        "        (true\n",
        "a_prepared_signature_field_given_something_else_is_a_rewritten_page",
    ),
    # The same two, asked of a field the page gained: a signature field with
    # no signature in it is not a listing, so its appearance counts.
    Mutation(
        "appendix: list a gained signature field whatever its value",
        "src/docinfo.rs",
        "                || self.new_signature(widget).is_none()\n",
        "",
        "an_unsigned_signature_field_with_an_appearance_after_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: list a gained field whose value signs nothing",
        "src/docinfo.rs",
        '            && self.whole.get_dictionary(signature).ok()?.has(b"ByteRange"))',
        '            && self.whole.get_dictionary(signature).is_ok())',
        "an_unsigned_signature_field_with_an_appearance_after_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: list a gained field holding a signature the signed revision had",
        "src/docinfo.rs",
        "        (!self.signed.objects.contains_key(&signature)\n",
        "        (true\n",
        "an_unsigned_signature_field_with_an_appearance_after_signing_is_a_rewritten_page",
    ),
    Mutation(
        "appendix: follow nothing else from a page with a signed place",
        "src/docinfo.rs",
        "                None => references_in(entry, &mut refers_to),",
        "                None => {}",
        "what_a_page_draws_replaced_beside_a_signed_place_is_a_rewritten_page",
    ),
    Mutation(
        # The edge left out is the signature's. What the widget names through
        # any other entry is the page's as it was.
        "appendix: follow nothing a signed place refers to besides its signature",
        "src/docinfo.rs",
        "                if !followed_here && !writes.contains(&key.as_slice()) {",
        "                if false {",
        "what_a_page_draws_replaced_beside_a_signed_place_is_a_rewritten_page",
    ),
    Mutation(
        # An edge is left out and never an object: a widget the page also
        # names in its resources is reached through the resources.
        "appendix: leave a page's resources out beside a signed place",
        "src/docinfo.rs",
        '        if key.as_slice() != b"Annots" {',
        '        if key.as_slice() != b"Annots" && key.as_slice() != b"Resources" {',
        "a_signed_place_the_page_also_draws_with_is_a_rewritten_page",
    ),
    Mutation(
        # `/Parent` is the document's word and can point in a circle.
        "appendix: take a field that leads back to itself for a signed place",
        "src/docinfo.rs",
        "            writes = &SIGNING_WRITES_ABOVE;\n"
        "        }\n"
        "        None\n",
        "            writes = &SIGNING_WRITES_ABOVE;\n"
        "        }\n"
        "        Some(refers_to)\n",
        "a_place_whose_fields_point_in_a_circle_is_a_rewritten_page",
    ),
    Mutation(
        # A list of annotations held as an object of its own is written again
        # without the page object.
        "appendix: ask only a rewritten page object whether it lists a field",
        "src/docinfo.rs",
        "        if !self.changed.contains(&id) && !self.list_rewritten(after) {",
        "        if !self.changed.contains(&id) {",
        "a_new_signature_field_listed_in_annotations_held_apart_is_a_listing",
    ),
    Mutation(
        "appendix: count no page for a field listed in a list held apart",
        "src/docinfo.rs",
        "    touched.extend(listings.iter().map(|(page, _)| *page));\n",
        "",
        "a_new_signature_field_listed_in_annotations_held_apart_is_a_listing",
    ),
    Mutation(
        # A page with a signed place names its list's entries in place of the
        # list, which is right only while the list stands as it was.
        "appendix: read a rewritten list as it stood beside a signed place",
        "src/docinfo.rs",
        "        if self.list_rewritten(after) {\n"
        "            return None;\n"
        "        }\n",
        "",
        "a_new_signature_field_listed_in_annotations_held_apart_is_a_listing",
    ),
    Mutation(
        "appendix: follow a signed place like any other annotation a page kept",
        "src/docinfo.rs",
        "            match self.signed_place(entry) {",
        "            match None::<Vec<lopdf::ObjectId>> {",
        "prepared_places_left_empty_change_no_other_reading",
    ),
    Mutation(
        # A page object is counted by what the file calls it, in the tree or
        # not.
        "appendix: count a page object only when the tree holds it",
        "src/docinfo.rs",
        '            .filter(|id| whole.objects.get(id).is_some_and(|o| kind_of(o) == "Page")),',
        '            .filter(|id| whole.objects.get(id).is_some_and(|o| kind_of(o) == "Pages")),',
        "a_page_object_written_outside_the_tree_is_counted",
    ),
]

# --- a show that goes whole, and what follows it ------------------------------
# A show removed whole took what it did to the pen and to the line with it.
# `glyph_cut.rs` now takes one with a show at its pen as a cut of every glyph,
# which leaves the distance, and `redact.rs` leaves what a `'` or a `"` did
# besides drawing. Each mutation was seen red by hand on 2026-10-09.
ROWS += [
    Mutation(
        # Until 2026-10-09 this was what happened: the operator went, the pen it
        # moved went with it, and the next show was drawn where it had started.
        "glyph cut: delete a whole show that has a show at its pen",
        "src/redact/glyph_cut.rs",
        "        if !fact.carries_on {\n"
        "            whole.push(ordinal);",
        "        if true {\n"
        "            whole.push(ordinal);",
        "a_whole_show_with_a_show_at_its_pen_becomes_a_cut_of_every_glyph",
    ),
    Mutation(
        # The control's direction: with nothing at its pen a show is deleted.
        "glyph cut: delete a show at whose pen another starts when it has no show there",
        "src/redact/glyph_cut.rs",
        "        if !fact.carries_on {\n"
        "            whole.push(ordinal);",
        "        if false {\n"
        "            whole.push(ordinal);",
        "a_whole_show_with_a_show_at_its_pen_becomes_a_cut_of_every_glyph",
    ),
    Mutation(
        # No place for the next show is no distance. The show is left and the
        # region says so.
        "glyph cut: take a show whole with no place for the show at its pen",
        "src/redact/glyph_cut.rs",
        "        match text.filter(|text| text.tail.is_some()) {",
        "        match text {",
        "a_whole_show_that_cannot_be_measured_is_left_and_reported",
    ),
    Mutation(
        "glyph cut: name an unmeasured show by the first text on the page",
        "src/redact/glyph_cut.rs",
        "                    .nth(ordinal)\n",
        "                    .nth(0)\n",
        "a_whole_show_that_cannot_be_measured_is_left_and_reported",
    ),
    Mutation(
        "glyph cut: say of an unmeasured show what is said of a picture",
        "src/redact.rs",
        "            (UNMEASURED_TEXT, None) => {",
        '            ("synthetic", None) => {',
        "a_whole_show_that_cannot_be_measured_is_left_and_reported",
    ),
    Mutation(
        "glyph cut: leave no distance where a whole show was",
        "src/redact/glyph_cut.rs",
        "    shown.push(Object::Real(first - tail));",
        "    shown.push(Object::Real(0.0));",
        "a_whole_show_leaves_the_distance_to_a_show_that_starts_at_its_pen",
    ),
    Mutation(
        # The first pen is after the numbers a `TJ` holds before its first string.
        "glyph cut: drop the numbers before a whole show's first string",
        "src/redact/glyph_cut.rs",
        "            .take_while(|part| matches!(part, Object::Integer(_) | Object::Real(_)))",
        "            .take_while(|_| false)",
        "a_whole_show_leaves_the_distance_to_a_show_that_starts_at_its_pen",
    ),
    Mutation(
        # The distance is measured to where the next show was placed, which is
        # after a `TJ` that draws nothing. Left in, it moves that show twice.
        "glyph cut: leave spacing in after a whole show",
        "src/redact/glyph_cut.rs",
        "        if continues && to_the_end {",
        "        if continues && to_the_end && !cut.takes_all() {",
        "spacing_alone_after_a_whole_show_goes_into_the_distance",
    ),
    Mutation(
        # `Td`, `Tm`, `T*`, `'` and `"` place what follows from the line's start.
        "glyph cut: leave a distance after a whole show with nothing at its pen",
        "src/redact/glyph_cut.rs",
        "    if !continues {\n"
        "        return Ok(out);\n"
        "    }\n",
        "",
        "a_whole_show_with_nothing_at_its_pen_leaves_nothing",
    ),
    Mutation(
        "glyph cut: take what a quote operator did with a whole show",
        "src/redact/glyph_cut.rs",
        "    let mut out = without_drawing(operation);",
        "    let mut out: Vec<Operation> = Vec::new();",
        "a_whole_quote_operator_keeps_what_it_did_before_showing",
    ),
    Mutation(
        # Nothing of the show stays that a wrong count could misplace.
        "glyph cut: count the codes of a show that goes whole",
        "src/redact/glyph_cut.rs",
        "        let replacement = if cut.takes_all() {",
        "        let replacement = if false {",
        "a_whole_show_counts_no_codes",
    ),
    Mutation(
        "glyph cut: write a whole show's distance to nowhere",
        "src/redact/glyph_cut.rs",
        "    let (first, tail) = cut.pens.first().copied().zip(cut.tail).ok_or_else(wrong)?;",
        "    let (first, tail) = cut\n"
        "        .pens\n"
        "        .first()\n"
        "        .copied()\n"
        "        .zip(cut.tail.or(Some(0.0)))\n"
        "        .ok_or_else(wrong)?;",
        "a_whole_show_not_measured_to_the_show_at_its_pen_is_refused",
    ),
    Mutation(
        # A cut of every glyph leaves no text object, so every show after it is
        # one earlier than its ordinal says.
        "glyph cut: delete the shows after a cut of every glyph by their planned ordinals",
        "src/redact/glyph_cut.rs",
        "        .map(|show| show - emptied.iter().filter(|gone| *gone < show).count())",
        "        .map(|show| *show)",
        "the_shows_after_a_cut_of_every_glyph_are_counted_from_what_is_left",
    ),
    Mutation(
        "glyph cut: count the shows a cut of every glyph took as still there",
        "src/redact/glyph_cut.rs",
        "    (shows, text_objects.saturating_sub(emptied.len()))",
        "    (shows, text_objects)",
        "the_shows_after_a_cut_of_every_glyph_are_counted_from_what_is_left",
    ),
    Mutation(
        "glyph cut: delete a show whose cuts add up to all of it though it has a tail",
        "src/redact/glyph_cut.rs",
        "        let whole = cut.takes_all() && cut.tail.is_none();",
        "        let whole = cut.takes_all();",
        "cuts_that_add_up_to_a_whole_show_with_a_tail_stay_a_cut",
    ),
    Mutation(
        # `'` moves to the next line before it shows. Deleted whole, the line
        # after it was drawn where the removed line had been.
        "redact: take a removed quote operator's line move with it",
        "src/redact.rs",
        '        ("\'" | "\\"", _) => vec![next_line()],',
        '        ("\'" | "\\"", _) => Vec::new(),',
        "a_removed_quote_operator_still_moves_to_its_line",
    ),
    Mutation(
        "redact: take the word spacing a removed double quote set with it",
        "src/redact.rs",
        '            Operation::new("Tw", vec![word.clone()]),\n',
        "",
        "a_removed_quote_operator_still_moves_to_its_line",
    ),
    Mutation(
        "redact: delete a removed show and what it did besides drawing",
        "src/redact.rs",
        "        let stays = without_drawing(&content.operations[at]);",
        "        let stays: Vec<Operation> = Vec::new();",
        "a_removed_quote_operator_still_moves_to_its_line",
    ),
    Mutation(
        "form: delete a removed show and what it did besides drawing",
        "src/redact.rs",
        "        let stays = without_drawing(&inside.operations[where_]);",
        "        let stays: Vec<Operation> = Vec::new();",
        "a_removed_quote_operator_in_a_form_still_moves_to_its_line",
    ),
]

# Added 2026-10-09 with the fixes to two read-only reviews' findings, in the
# same edit as the tests they name. Every one was seen red by hand before it
# was written here --- the anchor once in its file, the edit applied, the named
# test failing, the file restored --- because this table was not run that day:
# another writer was in the checkout, and a harness that edits the tree must
# not be.
ROWS += [
    Mutation(
        "reading: read a line's halves in the order of their top edges",
        "src/reading.rs",
        "    if fragments.len() < 2 {\n"
        "        return fragments;\n"
        "    }\n"
        "    let mut rows",
        "    if fragments.len() < usize::MAX {\n"
        "        return fragments;\n"
        "    }\n"
        "    let mut rows",
        "the_halves_of_a_split_line_are_read_along_it_whichever_stands_higher",
    ),
    Mutation(
        "reading: read every row from its left end",
        "src/reading.rs",
        "            let backwards = written_backwards(view, &row, axes);",
        "            let backwards = false;",
        "a_right_to_left_line_in_two_halves_is_read_from_its_right_end",
    ),
    Mutation(
        "reading: read every row from its right end",
        "src/reading.rs",
        "            let backwards = written_backwards(view, &row, axes);",
        "            let backwards = true;",
        "the_halves_of_a_split_line_are_read_along_it_whichever_stands_higher",
    ),
    Mutation(
        "reading: let one step back turn a row round",
        "src/reading.rs",
        "    backwards > forwards\n"
        "}",
        "    backwards > 0\n"
        "}",
        "one_word_written_the_other_way_does_not_turn_its_line_round",
    ),
    Mutation(
        "reading: count the step back to a combining mark",
        "src/reading.rs",
        "            if !placed(q) || combining(view.codes.get(index).copied().unwrap_or(0)) {",
        "            if !placed(q) {",
        "a_combining_mark_does_not_turn_its_line_round",
    ),
    Mutation(
        "reading: take whatever touches a line for a half of its row",
        "src/reading.rs",
        "        match rows.iter_mut().find(|(first, _)| same_row(*first, extents)) {",
        "        match rows.iter_mut().find(|(first, _)| same_band(*first, extents)) {",
        "type_of_another_size_touching_a_line_is_not_a_half_of_it",
    ),
    Mutation(
        "reading: measure a row against the shorter fragment",
        "src/reading.rs",
        "    let taller = (a.cross_end - a.cross_start).max(b.cross_end - b.cross_start);",
        "    let taller = (a.cross_end - a.cross_start).min(b.cross_end - b.cross_start);",
        "type_of_another_size_touching_a_line_is_not_a_half_of_it",
    ),
    Mutation(
        "sign: compare a field's name as bytes",
        "src/fields.rs",
        "        .map(crate::annots::decode_text_string)\n"
        "        .unwrap_or_default()",
        "        .map(|raw| String::from_utf8_lossy(raw).into_owned())\n"
        "        .unwrap_or_default()",
        "a_name_taken_in_another_encoding_is_taken",
    ),
    Mutation(
        "sign: take any signature of the name for the new one",
        "src/sign_cms.rs",
        "        signature.signed && signature.field == field && signature.appended_bytes == appended",
        "        signature.signed && signature.field == field",
        "an_older_intact_signature_of_the_same_name_does_not_answer_for_the_new_one",
    ),
    Mutation(
        "sign: take the first field that fits for the new one, intact or not",
        "src/sign_cms.rs",
        "        .position(|signature| fits(signature) && intact(signature))\n"
        "        .or_else(|| found.iter().position(fits))",
        "        .position(fits)",
        "the_new_signature_is_the_one_whose_range_ends_where_its_revision_does",
    ),
    Mutation(
        "sign: take an unsigned field for the new signature",
        "src/sign_cms.rs",
        "        signature.signed && signature.field == field && signature.appended_bytes == appended",
        "        signature.field == field && signature.appended_bytes == appended",
        "the_new_signature_is_the_one_whose_range_ends_where_its_revision_does",
    ),
    Mutation(
        "sign: look for the new signature as if nothing followed it",
        "src/commands/sign.rs",
        "    let ours = sign_cms::ours(&signatures, &field, (bytes.len() - sealed) as u64);",
        "    let ours = sign_cms::ours(&signatures, &field, 0);",
        "long_term_data_that_came_is_written_and_read_back_good",
    ),
    Mutation(
        "longterm: look for the new signature as if nothing followed it",
        "src/longterm.rs",
        "    let ours = crate::sign_cms::ours(&extended.signatures, field, extended.update.len() as u64);",
        "    let ours = crate::sign_cms::ours(&extended.signatures, field, 0);",
        "a_b_lt_signature_is_written_and_reads_back_good_for_signer_and_authority",
    ),
    Mutation(
        "sign: call a signature the read-back had no budget for a failed copy",
        "src/commands/sign.rs",
        "            } else {\n                ReadBack::Unchecked\n",
        "            } else {\n                ReadBack::Signature\n",
        "a_new_signature_the_read_back_had_no_budget_for_is_not_a_failed_copy",
    ),
    Mutation(
        "sign: call any unchecked new signature one not checked again",
        "src/commands/sign.rs",
        "        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {\n",
        "        Some(found) if found.verdict == Verdict::Unchecked => {\n",
        "a_copy_that_does_not_read_back_as_written_is_refused_in_the_window_too",
    ),
    Mutation(
        "cli sign: exit 4 for a signature not checked again",
        "src/cli/sign.rs",
        "        ReadBack::Unchecked => Exit::Strict,",
        "        ReadBack::Unchecked => Exit::Internal,",
        "a_signature_not_checked_again_ends_sign_with_1_and_says_which_check_happened",
    ),
    Mutation(
        "cli sign: exit 0 for a signature not checked again",
        "src/cli/sign.rs",
        "        ReadBack::Unchecked => Exit::Strict,",
        "        ReadBack::Unchecked => Exit::Ok,",
        "a_signature_not_checked_again_ends_sign_with_1_and_says_which_check_happened",
    ),
    Mutation(
        "words: say do not rely of a signature not checked again",
        "src/words.rs",
        "    let unchecked = verdict_is(Verdict::Unchecked)\n"
        "        && ",
        "    let unchecked = false\n"
        "        && ",
        "a_signature_not_checked_again_ends_sign_with_1_and_says_which_check_happened",
    ),
    Mutation(
        "words: say not checked again of any unchecked signature",
        "src/words.rs",
        "            integrity.as_ref().and_then(|i| i.why) == Some(Why::Budget)",
        "            integrity.as_ref().and_then(|i| i.why).is_some()",
        "a_signature_not_checked_again_ends_sign_with_1_and_says_which_check_happened",
    ),
    Mutation(
        "words: report the timestamp of a signature not checked again",
        "src/words.rs",
        "    if let Some((timestamp, authority)) = timestamp.filter(|_| !unchecked) {",
        "    if let Some((timestamp, authority)) = timestamp {",
        "a_signature_not_checked_again_ends_sign_with_1_and_says_which_check_happened",
    ),
    Mutation(
        "longterm: call a signature the check had no budget for tpdf's own failure",
        "src/longterm.rs",
        "        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {\n"
        "            return Err(Refusal::Bound(",
        "        Some(found) if false && found.why == Some(Why::Budget) => {\n"
        "            return Err(Refusal::Bound(",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "longterm: call any unchecked new signature a bound",
        "src/longterm.rs",
        "        Some(found) if found.verdict == Verdict::Unchecked && found.why == Some(Why::Budget) => {\n"
        "            return Err(Refusal::Bound(",
        "        Some(found) if found.verdict == Verdict::Unchecked => {\n"
        "            return Err(Refusal::Bound(",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "sign: write a revision lopdf would leave an object out of",
        "src/sign_prepare.rs",
        "    crate::save::written_whole(&incremental)?;\n",
        "",
        "a_form_the_writer_would_leave_out_is_a_refusal_and_not_a_revision_without_it",
    ),
    Mutation(
        "dss: write a revision lopdf would leave an object out of",
        "src/sign_dss.rs",
        "    crate::save::written_whole(&incremental)?;\n",
        "",
        "a_dss_the_writer_would_leave_out_is_a_refusal",
    ),
    Mutation(
        "append: write an update lopdf would leave an object out of",
        "src/save.rs",
        "    written_whole(&incremental)?;\n"
        "    incremental",
        "    incremental",
        "an_edit_the_writer_would_leave_out_is_a_refusal_and_not_a_save_that_changed_nothing",
    ),
    Mutation(
        "append: do not ask about a /Linearized key",
        "src/save.rs",
        'const LEFT_OUT: [&[u8]; 3] = [b"ObjStm", b"XRef", b"Linearized"];',
        'const LEFT_OUT: [&[u8]; 2] = [b"ObjStm", b"XRef"];',
        "lopdf_still_leaves_such_an_object_out_of_an_update_without_an_error",
    ),
    Mutation(
        "rewrite: do not ask about a /Linearized key",
        "src/save.rs",
        'const LEFT_OUT: [&[u8]; 3] = [b"ObjStm", b"XRef", b"Linearized"];',
        'const LEFT_OUT: [&[u8]; 2] = [b"XRef", b"ObjStm"];',
        "lopdf_still_leaves_such_an_object_out_of_a_whole_copy_without_an_error",
    ),
    Mutation(
        "rewrite: write a copy lopdf would leave an object out of",
        "src/save.rs",
        "    reached_whole(doc, packed)?;\n",
        "",
        "a_copy_the_writer_would_leave_a_comment_out_of_is_a_refusal_and_not_a_copy_without_it",
    ),
    Mutation(
        "merge: write a merged document lopdf would leave an object out of",
        "src/save.rs",
        "    reached_whole(doc, packed)?;\n",
        "    let _ = reached_whole(doc, packed);\n",
        "a_merge_the_writer_would_leave_a_comment_out_of_is_a_refusal",
    ),
    Mutation(
        "print: build a job lopdf would leave an object out of",
        "src/save.rs",
        "    reached_whole(doc, packed)?;\n",
        "    reached_whole(doc, packed).ok();\n",
        "a_print_job_that_would_lose_a_comment_is_a_refusal_and_an_estimate_has_it",
    ),
    Mutation(
        "preview: draw a page without a comment lopdf would leave out",
        "src/textview.rs",
        "    crate::save::reached_whole(&document, false)?;\n",
        "",
        "a_preview_that_would_be_drawn_without_a_comment_of_the_file_is_refused",
    ),
    Mutation(
        "rewrite: refuse an object of such a type that nothing reaches",
        "src/save.rs",
        "    match left_out.iter().find(|(id, _)| reachable.contains(id)) {",
        "    match left_out.first() {",
        "a_linearized_document_and_one_with_object_streams_are_still_rewritten_whole",
    ),
    Mutation(
        "rewrite: refuse an object nothing reaches, asked of the guard alone",
        "src/save.rs",
        "    match left_out.iter().find(|(id, _)| reachable.contains(id)) {",
        "    match left_out.iter().find(|_| true) {",
        "an_object_of_such_a_type_that_nothing_reaches_is_not_a_refusal",
    ),
    Mutation(
        "rewrite: ask an encrypted smaller copy the question of the writer it does not get",
        "src/save.rs",
        "    let plain = !packed || doc.is_encrypted();",
        "    let plain = !packed;",
        "a_copy_the_writer_would_leave_a_comment_out_of_is_a_refusal_and_not_a_copy_without_it",
    ),
    Mutation(
        "rewrite: refuse a smaller copy for what a plain one would lose",
        "src/save.rs",
        "    let plain = !packed || doc.is_encrypted();",
        "    let plain = true;",
        "a_print_job_that_would_lose_a_comment_is_a_refusal_and_an_estimate_has_it",
    ),
    Mutation(
        "rewrite: refuse a smaller copy for a dictionary typed as an object stream",
        "src/save.rs",
        '                matches!(object, Object::Stream(_)) && kind == b"ObjStm"',
        '                kind == b"ObjStm"',
        "lopdf_still_leaves_such_an_object_out_of_a_whole_copy_without_an_error",
    ),
    Mutation(
        "rewrite: write a smaller copy without a stream typed as an object stream",
        "src/save.rs",
        '                matches!(object, Object::Stream(_)) && kind == b"ObjStm"',
        "                false",
        "lopdf_still_leaves_such_an_object_out_of_a_whole_copy_without_an_error",
    ),
    Mutation(
        "field names: stop at a group with no name",
        "src/fields.rs",
        "        if name.is_empty() {\n            return Flow::Descend;",
        "        if name.is_empty() {\n            return Flow::Leaf;",
        "a_name_taken_under_a_group_with_no_name_is_taken",
    ),
    Mutation(
        "field names: stop at a group with no name, for a new form field",
        "src/fields.rs",
        "        if name.is_empty() {\n            return Flow::Descend;",
        "        if true {\n            return Flow::Leaf;",
        "a_name_a_field_under_a_group_with_no_name_has_is_taken",
    ),
    Mutation(
        "field names: collect what is under a field that has a name",
        "src/fields.rs",
        "        names.insert(name);\n        Flow::Leaf",
        "        names.insert(name);\n        Flow::Descend",
        "the_names_of_one_part_are_read_through_groups_with_no_name",
    ),
    Mutation(
        "field names: hand over the names of a tree cut for its depth",
        "src/fields.rs",
        "    if cut.dropped > 0 || cut.too_deep > 0 {\n        return Err(\n",
        "    if cut.dropped > 0 {\n        return Err(\n",
        "a_form_too_deep_to_read_every_name_of_is_not_given_a_guessed_one",
    ),
    Mutation(
        "field names: hand over the names of a tree cut for its size",
        "src/fields.rs",
        "    if cut.dropped > 0 || cut.too_deep > 0 {\n        return Err(\n",
        "    if cut.too_deep > 0 {\n        return Err(\n",
        "the_names_of_a_tree_past_the_bounds_are_refused_and_not_cut_short",
    ),
    Mutation(
        "field names: refuse a document with no form",
        "src/fields.rs",
        "    if cut.dropped > 0 || cut.too_deep > 0 {\n        return Err(\n",
        "    if true {\n        return Err(\n",
        "a_document_with_no_form_has_no_names_and_is_not_refused",
    ),
    Mutation(
        "field names: visit a group that holds itself until the budget is gone",
        "src/fields.rs",
        "        depth: Some(32),\n        dedup: true,\n        names: false,",
        "        depth: Some(32),\n        dedup: false,\n        names: false,",
        "the_names_of_a_tree_past_the_bounds_are_refused_and_not_cut_short",
    ),
    Mutation(
        "append: stop asking at the first object that is not a dictionary",
        "src/save.rs",
        "        let Ok(kind) = object.type_name() else {\n"
        "            continue;\n"
        "        };",
        "        let Ok(kind) = object.type_name() else {\n"
        "            return Ok(());\n"
        "        };",
        "a_dss_the_writer_would_leave_out_is_a_refusal",
    ),
    Mutation(
        "sign: take a held signature before judging the answer",
        "src/commands/sign.rs",
        "            Some(found) if found.number == number => judge(&found.stage)?,",
        "            Some(found) if found.number == number => {\n"
        '                let taken = held.take().expect("held");\n'
        "                judge(&taken.stage)?;\n"
        "                return Ok(taken);\n"
        "            }",
        "an_answer_that_is_refused_leaves_the_signature_held",
    ),
    Mutation(
        "sign: resume long-term signing with no timestamp authority",
        "src/commands/sign.rs",
        "            long_term_of(long_term, authority)?;\n"
        "        }\n"
        "        Ok(())\n"
        "    })\n"
        "}",
        "        }\n"
        "        Ok(())\n"
        "    })\n"
        "}",
        "an_answer_that_is_refused_leaves_the_signature_held",
    ),
    Mutation(
        "sign: hand a held signature to any number",
        "src/commands/sign.rs",
        "            Some(found) if found.number == number => judge(&found.stage)?,",
        "            Some(found) => judge(&found.stage)?,",
        "an_answer_that_is_refused_leaves_the_signature_held",
    ),
    Mutation(
        "workers: repeat a mismatched reply whole",
        "src/workers.rs",
        '        "a worker answered {} to a request for {asked}",\n'
        "        got.described()",
        '        "a worker answered {:?} to a request for {asked}",\n'
        "        got",
        "a_mismatched_reply_is_named_and_not_repeated",
    ),
    Mutation(
        "worker proto: describe a reply with its payload",
        "src/worker_proto.rs",
        '        format!("{name} ({})", about_bytes(bytes))',
        '        format!("{name} ({}) {self:?}", about_bytes(bytes))',
        "a_reply_is_described_by_its_name_and_size_and_never_its_payload",
    ),
    Mutation(
        "worker proto: show an unreadable reply line whole",
        "src/worker_proto.rs",
        "    let start: String = line.chars().take(SHOWN).collect();",
        "    let start: String = line.chars().collect();",
        "a_reply_is_described_by_its_name_and_size_and_never_its_payload",
    ),
]

# --- long-term validation data for a document that is already signed ---------
#
# `longterm/existing.rs`, `sign_dss::survey`, `docinfo::signature_values`,
# `commands/validation.rs`, `cli/long_term.rs`, 2026-10-10. What can go wrong
# here is a refusal that is skipped --- data fetched for a signer nobody
# vouched for, a copy written with one signature left out of the check --- so
# nearly every entry removes one refusal and names the test that holds it.
# Every one was seen red by hand before it was written here: the anchor once
# in its file, the edit applied, the named test failing, the file restored.
# This table was not run that day, because another writer was in the checkout.
ROWS += [
    Mutation(
        "existing: add data to a document with no signature",
        "src/longterm/existing.rs",
        "    if signed.is_empty() {",
        "    if false && signed.is_empty() {",
        "a_document_with_no_signature_is_refused",
    ),
    Mutation(
        "existing: take a survey that says it is not complete",
        "src/longterm/existing.rs",
        "    if !survey.complete || !agree {",
        "    if !agree {",
        "a_survey_that_is_not_complete_is_refused",
    ),
    Mutation(
        "existing: take values that do not match the signatures",
        "src/longterm/existing.rs",
        "    if !survey.complete || !agree {",
        "    if !survey.complete {",
        "a_survey_that_is_not_complete_is_refused",
    ),
    Mutation(
        "existing: append to a document certified against any change",
        "src/longterm/existing.rs",
        "        if signature.certification == 1 {",
        "        if false && signature.certification == 1 {",
        "a_certification_with_no_changes_permitted_is_refused_and_the_others_are_not",
    ),
    Mutation(
        "existing: gather for a signature that does not verify",
        "src/longterm/existing.rs",
        "        match signature.integrity.as_ref() {\n"
        "            Some(found) if found.verdict == Verdict::Intact => {}",
        "        match signature.integrity.as_ref() {\n"
        "            Some(_) => {}",
        "a_signature_that_does_not_verify_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: gather for a signature whose timestamp does not verify",
        "src/longterm/existing.rs",
        "            if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {\n"
        "                return Err(Refusal::NotIntact {",
        "            if false {\n"
        "                return Err(Refusal::NotIntact {",
        "a_timestamp_that_does_not_verify_is_refused_by_name",
    ),
    Mutation(
        "existing: take any standing as a chain vouched for",
        "src/longterm/existing.rs",
        "    if trust.standing == Standing::Trusted {",
        "    if true || trust.standing == Standing::Trusted {",
        "a_signer_this_computer_does_not_trust_is_refused_and_nothing_is_fetched",
    ),
    Mutation(
        "existing: judge a document timestamp's authority as a signer of documents",
        "src/longterm/existing.rs",
        "    let document_timestamp = kind == DOCUMENT_TIMESTAMP;",
        "    let document_timestamp = false;",
        "a_document_already_archived_gets_a_further_archive_and_keeps_its_dss",
    ),
    Mutation(
        "existing: ask about one certificate once for every signature that names it",
        "src/longterm/existing.rs",
        "            if !subjects.iter().any(|s| der(&s.certificate) == this) {",
        "            if true {",
        "every_signature_of_a_document_is_covered_and_each_certificate_asked_once",
    ),
    Mutation(
        "existing: ask about any number of certificates",
        "src/longterm/existing.rs",
        "    if subjects.len() > most {",
        "    if false {",
        "more_certificates_than_one_run_asks_about_is_a_refusal",
    ),
    Mutation(
        "existing: find an earlier signature by its name alone",
        "src/longterm/existing.rs",
        "                && s.appended_bytes == was.appended_bytes.saturating_add(appended)\n",
        "",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: write a result in which an earlier signature is not intact",
        "src/longterm/existing.rs",
        "        match now.integrity.as_ref() {\n"
        "            Some(found) if found.verdict == Verdict::Intact => {}",
        "        match now.integrity.as_ref() {\n"
        "            Some(_) => {}",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: take an appendix tpdf could not read",
        "src/longterm/existing.rs",
        "            Some(appendix) if !appendix.unread => {}",
        "            Some(_) => {}",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: write a result with a page changed after a signature",
        "src/longterm/existing.rs",
        "        if pages > pages_were || removed > removed_were {",
        "        if removed > removed_were {",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: write a result with an object removed after a signature",
        "src/longterm/existing.rs",
        "        if pages > pages_were || removed > removed_were {",
        "        if pages > pages_were {",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: count a page that only lists a new field as changed",
        "src/longterm/existing.rs",
        "                .saturating_sub(appendix.pages_listing.len()),",
        "                .saturating_sub(0),",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: write a result in which a signature lost its timestamp",
        "src/longterm/existing.rs",
        "        } else if was.timestamp.is_some() {",
        "        } else if false {",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: write a result whose timestamp no longer verifies",
        "src/longterm/existing.rs",
        "            if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {\n"
        "                return Err(changed(",
        "            if false {\n"
        "                return Err(changed(",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: leave the certificates' own answers unread",
        "src/longterm/existing.rs",
        "        leaves_good(&leaves).map_err(about)?;",
        "        let _ = &leaves;",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: leave the chains above the certificates unread",
        "src/longterm/existing.rs",
        "        chains_good(&chains).map_err(about)?;",
        "        let _ = &chains;",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: leave a signature's timestamp authority unread",
        "src/longterm/existing.rs",
        "            leaves.push((\n"
        "                stamp.revocation.as_ref(),",
        "            leaves.push((\n"
        "                now.revocation.as_ref(),",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: leave the chain above a timestamp authority unread",
        "src/longterm/existing.rs",
        "            chains.push((stamp.revocation_chain.as_ref(), \"the timestamp authority's\"));",
        "            chains.push((now.revocation_chain.as_ref(), \"the timestamp authority's\"));",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: name a document timestamp's certificate as a signer's",
        "src/longterm/existing.rs",
        "        let document_timestamp = now.kind == DOCUMENT_TIMESTAMP;",
        "        let document_timestamp = false;",
        "every_earlier_signature_must_read_good_with_the_data_added",
    ),
    Mutation(
        "existing: take any signature ending at the last byte for the archive",
        "src/longterm/existing.rs",
        "        .find(|s| s.signed && s.kind == DOCUMENT_TIMESTAMP && s.appended_bytes == 0)",
        "        .find(|s| s.signed && s.appended_bytes == 0)",
        "the_written_file_must_hold_the_archive_over_the_whole_file",
    ),
    Mutation(
        "existing: take an earlier timestamp for the archive",
        "src/longterm/existing.rs",
        "        .find(|s| s.signed && s.kind == DOCUMENT_TIMESTAMP && s.appended_bytes == 0)",
        "        .find(|s| s.signed && s.kind == DOCUMENT_TIMESTAMP)",
        "the_written_file_must_hold_the_archive_over_the_whole_file",
    ),
    Mutation(
        "existing: pass an archive that does not cover the whole file",
        "src/longterm/existing.rs",
        "    if !archive.covers_whole_file {",
        "    if false {",
        "the_written_file_must_hold_the_archive_over_the_whole_file",
    ),
    Mutation(
        "existing: pass an archive that is not intact",
        "src/longterm/existing.rs",
        "        Some(found) if found.verdict == Verdict::Intact => Ok(()),",
        "        Some(_) => Ok(()),",
        "the_written_file_must_hold_the_archive_over_the_whole_file",
    ),
    Mutation(
        "existing: read the written file without the earlier signatures",
        "src/longterm/existing.rs",
        "    covered(before, after, appended)?;\n"
        "    let Some(archive) = archive_of(after) else {",
        "    let Some(archive) = archive_of(after) else {",
        "the_written_file_must_hold_the_archive_over_the_whole_file",
    ),
    Mutation(
        "existing: go on with a document its worker refused",
        "src/longterm/existing.rs",
        "    if let Some(why) = &survey.refused {",
        "    if let Some(why) = survey.refused.as_ref().filter(|_| false) {",
        "a_worker_that_died_is_tpdfs_failure_and_a_document_it_refuses_is_not",
    ),
    Mutation(
        "existing: fetch before the budget is known to hold",
        "src/longterm/existing.rs",
        "    if past_budget(&held, original.len()) {",
        "    if false {",
        "signatures_past_the_hashing_budget_are_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: write a result past the budget to check it",
        "src/longterm/existing.rs",
        "    if past_budget(&held, bytes.len()) {",
        "    if false {",
        "signatures_past_the_hashing_budget_are_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: take a revision built against other bytes",
        "src/longterm/existing.rs",
        "    if extended.built_against != original.len() {",
        "    if false {",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: write a result the worker's reading was not held to",
        "src/longterm/existing.rs",
        "    covered(\n"
        "        &survey.signatures,\n"
        "        &extended.signatures,\n"
        "        extended.update.len() as u64,\n"
        "    )?;",
        "",
        "a_result_in_which_an_earlier_signature_reads_differently_is_not_written",
    ),
    Mutation(
        "existing: seal an archive timestamp from an authority nobody vouched for",
        "src/longterm/existing.rs",
        "        let token = archive(pieces)?;\n",
        "        let token = archive(pieces)?;\n"
        "        if !token.is_empty() {\n"
        "            return Ok(token);\n"
        "        }\n",
        "an_archive_timestamp_from_an_authority_nobody_trusts_is_refused",
    ),
    Mutation(
        "existing: call a worker's death the document's refusal",
        "src/longterm/existing.rs",
        "            Refusal::Failed(_) | Refusal::Changed { .. } | Refusal::Archive(_) => true,",
        "            Refusal::Changed { .. } | Refusal::Archive(_) => true,",
        "a_worker_that_died_is_tpdfs_failure_and_a_document_it_refuses_is_not",
    ),
    Mutation(
        "existing: say of a revoked certificate what a signing says",
        "src/longterm/existing.rs",
        "        super::Refusal::Revoked { name, at, by } => format!(",
        "        super::Refusal::Revoked { name, at, by } if false => format!(",
        "revocation_data_that_cannot_be_had_writes_nothing",
    ),
    Mutation(
        "existing: say of a certificate publishing nothing what a signing says",
        "src/longterm/existing.rs",
        "        super::Refusal::NotPublished(name) => format!(",
        "        super::Refusal::NotPublished(name) if false => format!(",
        "revocation_data_that_cannot_be_had_writes_nothing",
    ),
    Mutation(
        "dss: survey a document that was encrypted and opened",
        "src/sign_dss.rs",
        "    if document.is_encrypted() || document.was_encrypted() {\n"
        "        return Survey::encrypted();",
        "    if document.is_encrypted() {\n"
        "        return Survey::encrypted();",
        "an_encrypted_document_is_refused_whether_or_not_it_opens",
    ),
    Mutation(
        "dss: survey a document no password opened",
        "src/sign_dss.rs",
        "    if document.is_encrypted() || document.was_encrypted() {\n"
        "        return Survey::encrypted();",
        "    if document.was_encrypted() {\n"
        "        return Survey::encrypted();",
        "an_encrypted_document_is_refused_whether_or_not_it_opens",
    ),
    Mutation(
        "dss: answer a survey of any size",
        "src/sign_dss.rs",
        "    if survey.bytes() > bound {",
        "    if false {",
        "a_document_that_can_take_no_revision_is_refused_in_the_answer",
    ),
    Mutation(
        "dss: call a survey complete whatever the walk said",
        "src/sign_dss.rs",
        "        complete: values.complete,",
        "        complete: true,",
        "a_value_that_cannot_be_read_makes_the_survey_incomplete",
    ),
    Mutation(
        "dss: hand over no store",
        "src/sign_dss.rs",
        "        store: values.store,",
        "        store: Vec::new(),",
        "the_survey_hands_over_each_signature_s_value_and_what_the_dss_carries",
    ),
    Mutation(
        "values: call an unread value read",
        "src/docinfo.rs",
        "                complete &= value.is_some();",
        "                complete &= true;",
        "a_value_that_cannot_be_read_makes_the_survey_incomplete",
    ),
    Mutation(
        "values: stop at the signature bound without saying so",
        "src/docinfo.rs",
        "                if values.len() >= MAX_SIGNATURES {\n"
        "                    complete = false;",
        "                if values.len() >= MAX_SIGNATURES {",
        "values_cut_short_by_a_bound_of_the_walk_are_not_complete",
    ),
    Mutation(
        "values: pass over an entry that is no field without saying so",
        "src/docinfo.rs",
        "        let Some(field) = node.dict else {\n"
        "            complete = false;\n"
        "            return fields::Flow::Leaf;\n"
        "        };\n"
        '        if name_of(document, field, b"FT") == "Sig" {\n'
        "            if let Some(sig) = field",
        "        let Some(field) = node.dict else {\n"
        "            return fields::Flow::Leaf;\n"
        "        };\n"
        '        if name_of(document, field, b"FT") == "Sig" {\n'
        "            if let Some(sig) = field",
        "values_cut_short_by_a_bound_of_the_walk_are_not_complete",
    ),
    Mutation(
        "values: stop at the depth bound without saying so",
        "src/docinfo.rs",
        "    complete &= cut.dropped == 0 && cut.too_deep == 0;",
        "    complete &= cut.dropped == 0;",
        "values_cut_short_by_a_bound_of_the_walk_are_not_complete",
    ),
    Mutation(
        "validation: report a copy nobody read back",
        "src/commands/validation.rs",
        "    existing::read_back(&added.before, &signatures, appended).map_err(Stopped::ReadBack)?;",
        "    let _ = appended;",
        "a_copy_that_does_not_read_back_as_built_is_named_and_is_tpdfs_failure",
    ),
    Mutation(
        "validation: go on after a copy that could not be written",
        "src/commands/validation.rs",
        "    write(&added.bytes).map_err(Stopped::Unwritten)?;",
        "    let _ = write(&added.bytes);",
        "a_copy_that_could_not_be_written_is_not_tpdfs_failure",
    ),
    Mutation(
        "validation: read the written file as if nothing had been appended",
        "src/commands/validation.rs",
        "    let appended = (added.bytes.len() - original.len()) as u64;",
        "    let appended = 0u64;",
        "the_copy_is_written_read_back_and_said_in_one_sentence",
    ),
    Mutation(
        "validation: call a copy that did not read back the document's refusal",
        "src/commands/validation.rs",
        "            Stopped::Unread(_) | Stopped::ReadBack(_) => true,",
        "            Stopped::Unread(_) | Stopped::ReadBack(_) => false,",
        "a_copy_that_does_not_read_back_as_built_is_named_and_is_tpdfs_failure",
    ),
    Mutation(
        "validation: say nothing was written about a copy that was",
        "src/commands/validation.rs",
        '            Stopped::Refused(why) => format!("{} --- nothing was written", why.sentence()),\n'
        "            Stopped::Unwritten(why) | Stopped::Unread(why) => why.clone(),\n"
        "            Stopped::ReadBack(why) => format!(\n"
        '                "{name} was written, but reading it back did not find what was added: {}. Do \\\n'
        '                 not rely on that copy.",\n'
        "                why.sentence()\n"
        "            ),",
        "            Stopped::Refused(why) | Stopped::ReadBack(why) => {\n"
        '                format!("{} --- nothing was written{}", why.sentence(), &name[..0])\n'
        "            }\n"
        "            Stopped::Unwritten(why) | Stopped::Unread(why) => why.clone(),",
        "a_copy_that_does_not_read_back_as_built_is_named_and_is_tpdfs_failure",
    ),
    Mutation(
        "validation: add to a document with unsaved edits",
        "src/commands/validation.rs",
        "    if dirty {\n"
        "        return Err(\n"
        '            "Save your changes first: validation data',
        "    if false && dirty {\n"
        "        return Err(\n"
        '            "Save your changes first: validation data',
        "unsaved_edits_and_a_document_too_large_are_refused",
    ),
    Mutation(
        "validation: add to a document of any size",
        "src/commands/validation.rs",
        "    if len > save::APPEND_MAX_BYTES {\n"
        "        return Err(format!(\n"
        '            "This document is {} MB, and tpdf adds validation data',
        "    if false {\n"
        "        return Err(format!(\n"
        '            "This document is {} MB, and tpdf adds validation data',
        "unsaved_edits_and_a_document_too_large_are_refused",
    ),
    Mutation(
        "cli: long-term over its own input",
        "src/cli/long_term.rs",
        "    if lexically_same(&input, &output) {\n"
        "        return Err(\n"
        '            "the output names the input --- the document with the data added',
        "    if false {\n"
        "        return Err(\n"
        '            "the output names the input --- the document with the data added',
        "long_term_needs_its_output_and_an_authority_it_was_told_to_ask",
    ),
    Mutation(
        "cli: long-term asks an address as typed",
        "src/cli/long_term.rs",
        "                    crate::tsa::authority(value(arg, &mut rest)?)\n"
        '                        .map_err(|why| why.sentence(""))?,\n'
        "                );\n"
        "            }\n"
        '            (false, "--json") => json = true,\n'
        '            (false, "--force") => force = true,\n'
        "            (false, flag) if flag.starts_with('-') && flag != \"-\" => {\n"
        '                return Err(unknown("long-term", flag))',
        "                    url::Url::parse(value(arg, &mut rest)?).map_err(|e| e.to_string())?,\n"
        "                );\n"
        "            }\n"
        '            (false, "--json") => json = true,\n'
        '            (false, "--force") => force = true,\n'
        "            (false, flag) if flag.starts_with('-') && flag != \"-\" => {\n"
        '                return Err(unknown("long-term", flag))',
        "long_term_needs_its_output_and_an_authority_it_was_told_to_ask",
    ),
    Mutation(
        "cli: long-term over an output that exists",
        "src/cli/long_term.rs",
        "        if !self.force && self.output.exists() {\n"
        "            return Err(Failure::new(\n"
        "                Exit::Refused,\n"
        "                format!(\n"
        '                    "{} already exists --- choose another name, or give --force to replace it",\n'
        "                    self.output.display()\n"
        "                ),\n"
        "            ));\n"
        "        }\n"
        "        // Read once",
        "        // Read once",
        "long_term_refuses_an_existing_output_and_a_missing_input_before_any_worker",
    ),
    Mutation(
        "cli: long-term takes an empty file to a worker",
        "src/cli/long_term.rs",
        "        if original.is_empty() {",
        "        if false {",
        "long_term_refuses_an_existing_output_and_a_missing_input_before_any_worker",
    ),
    Mutation(
        "cli: long-term exits 3 for tpdf's own failure",
        "src/cli/long_term.rs",
        "    let exit = if stopped.tpdf_failed() {",
        "    let exit = if false {",
        "a_long_term_run_that_stopped_is_4_only_when_the_failure_is_tpdfs",
    ),
    Mutation(
        "words: say one signature in the plural",
        "src/words.rs",
        '        [(_, false)] => "that signature is",',
        '        [(_, false)] => "each of them is",',
        "the_copy_is_written_read_back_and_said_in_one_sentence",
    ),
]

# --- the same, after an independent read-only review (2026-10-10) -------------
#
# The survey's bounds held in the app process, a `/DSS` with no room left
# refused before fetching and no entry written twice, the catalog's
# certification and a level spelt as a real number, a later run after a
# certificate has expired, and a look-alike issuer. Each seen red by hand
# against the test it names, as the block above. One survived its first run:
# the fixture wrote the level `1.0` through `lopdf`, which spells it `1`, so
# the real-number branch was never read (`docs/TRAPS.md`); the fixture now
# respells the bytes. **Not here, because `tests/cli.rs` is a binary this
# table cannot select**: `save_outside.rs`'s `if answered.locked {` made
# `if false {` turns *long-term refuses a document that needs a password*
# red (exit 4 for 3), by hand.
ROWS += [
    Mutation(
        "existing: take a survey of any size from the worker",
        "src/longterm/existing.rs",
        "    bounded(&survey)?;\n",
        "",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take any number of signatures from the worker",
        "src/longterm/existing.rs",
        "    let over = survey.signatures.len() > crate::docinfo::MAX_SIGNATURES\n",
        "    let over = false\n",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take any number of values from the worker",
        "src/longterm/existing.rs",
        "        || survey.values.len() > crate::docinfo::MAX_SIGNATURES\n",
        "",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take a value of any size from the worker",
        "src/longterm/existing.rs",
        "            .any(|(_, value)| value.len() > crate::docinfo::MAX_SIG_BLOB)",
        "            .any(|(_, value)| value.len() > usize::MAX)",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take any number of stored certificates from the worker",
        "src/longterm/existing.rs",
        "        || survey.store.len() > crate::revocation::MAX_DSS_CERTIFICATES\n",
        "",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take a stored certificate of any size from the worker",
        "src/longterm/existing.rs",
        "            .any(|der| der.len() > crate::trust::MAX_CERTIFICATE_BYTES)",
        "            .any(|der| der.len() > usize::MAX)",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: take any amount of DER from the worker",
        "src/longterm/existing.rs",
        "        || survey.bytes() > crate::sign_dss::MAX_BYTES;",
        "        || false;",
        "a_survey_past_a_workers_bounds_is_refused_before_any_of_it_is_parsed",
    ),
    Mutation(
        "existing: fetch for a document whose store has no room",
        "src/longterm/existing.rs",
        "    room(&survey, &subjects, &certificates)?;\n",
        "",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: fetch for a store the reader already cuts",
        "src/longterm/existing.rs",
        "    let fits = !survey.store_cut\n",
        "    let fits = true\n",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: add certificates past what the reader takes",
        "src/longterm/existing.rs",
        "        && certificates + new <= crate::revocation::MAX_DSS_CERTIFICATES\n",
        "",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: add responses past what the reader takes",
        "src/longterm/existing.rs",
        "        && responses + subjects.len() <= crate::revocation::MAX_RESPONSES\n",
        "",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: add lists past what the reader takes",
        "src/longterm/existing.rs",
        "        && lists + subjects.len() <= crate::revocation::MAX_LISTS;",
        "        && true;",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: count a certificate the store holds as one more",
        "src/longterm/existing.rs",
        "        .filter(|der| !survey.store.contains(der))",
        "        .filter(|der| !der.is_empty())",
        "a_dss_with_no_room_left_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        "existing: go on with a catalog certified against any change",
        "src/longterm/existing.rs",
        "    if survey.certified == 1 {",
        "    if false {",
        "a_document_certified_in_its_catalog_is_judged_by_the_level_it_states",
    ),
    Mutation(
        "existing: walk into a certificate the store did not vouch for",
        "src/longterm.rs",
        "            if at > 0 && !admitted.contains(&der) {",
        "            if false {",
        "a_look_alike_issuer_is_never_asked_about_wherever_the_document_offers_it",
    ),
    Mutation(
        "existing: gather for a signer whose certificate has expired",
        "src/longterm/existing.rs",
        "    if trust.standing == Standing::Trusted {",
        "    if trust.standing == Standing::Trusted || trust.standing == Standing::Expired {",
        "a_later_run_is_refused_once_a_certificate_has_expired",
    ),
    Mutation(
        "dss: write an entry the store already holds",
        "src/sign_dss.rs",
        "            if there.contains(item) {\n"
        "                continue;\n"
        "            }\n",
        "",
        "an_entry_the_dss_already_holds_is_not_written_again",
    ),
    Mutation(
        "dss: forget what this revision has just written",
        "src/sign_dss.rs",
        "            there.push(item.clone());\n",
        "",
        "an_entry_the_dss_already_holds_is_not_written_again",
    ),
    Mutation(
        "dss: survey a /DSS the writer cannot extend",
        "src/sign_dss.rs",
        "        Err(why) => return Survey::refusing(why),\n"
        "    };\n"
        "    let found",
        "        Err(_) => [0; 3],\n"
        "    };\n"
        "    let found",
        "a_dss_that_cannot_be_extended_or_read_whole_is_said_in_the_survey",
    ),
    Mutation(
        "dss: say the store holds nothing",
        "src/sign_dss.rs",
        "        Ok(found) => found.arrays.map(|array| array.len()),",
        "        Ok(found) => found.arrays.map(|_| 0),",
        "an_entry_the_dss_already_holds_is_not_written_again",
    ),
    Mutation(
        "dss: leave the catalog's certification out of the survey",
        "src/sign_dss.rs",
        "        certified: crate::sign_prepare::certification(&document),",
        "        certified: 0,",
        "a_document_certified_in_its_catalog_is_judged_by_the_level_it_states",
    ),
    Mutation(
        "values: call a store the reader cut whole",
        "src/docinfo.rs",
        "        store_cut: store.dropped > 0 || store.unread > 0,",
        "        store_cut: store.unread > 0,",
        "a_dss_that_cannot_be_extended_or_read_whole_is_said_in_the_survey",
    ),
    Mutation(
        "values: call a store with an unread entry whole",
        "src/docinfo.rs",
        "        store_cut: store.dropped > 0 || store.unread > 0,",
        "        store_cut: store.dropped > 0,",
        "a_dss_that_cannot_be_extended_or_read_whole_is_said_in_the_survey",
    ),
    Mutation(
        "values: call any field after the bound a signature left out",
        "src/docinfo.rs",
        "        let Some(field) = node.dict else {\n"
        "            complete = false;\n"
        "            return fields::Flow::Leaf;\n"
        "        };\n"
        '        if name_of(document, field, b"FT") == "Sig" {\n'
        "            if let Some(sig) = field",
        "        if values.len() >= MAX_SIGNATURES {\n"
        "            complete = false;\n"
        "            return fields::Flow::Leaf;\n"
        "        }\n"
        "        let Some(field) = node.dict else {\n"
        "            complete = false;\n"
        "            return fields::Flow::Leaf;\n"
        "        };\n"
        '        if name_of(document, field, b"FT") == "Sig" {\n'
        "            if let Some(sig) = field",
        "values_cut_short_by_a_bound_of_the_walk_are_not_complete",
    ),
    Mutation(
        "certification: read a level written as a real number as none",
        "src/docinfo.rs",
        "            Object::Real(level) if level.fract() == 0.0 => Some(*level as i64),",
        "            Object::Real(level) if level.is_nan() => Some(*level as i64),",
        "a_document_certified_in_its_catalog_is_judged_by_the_level_it_states",
    ),
    Mutation(
        "certification: read a level that is no whole number as one",
        "src/docinfo.rs",
        "            Object::Real(level) if level.fract() == 0.0 => Some(*level as i64),",
        "            Object::Real(level) if level.is_finite() => Some(*level as i64),",
        "a_document_certified_in_its_catalog_is_judged_by_the_level_it_states",
    ),
    Mutation(
        "validation: take the original's own name for the copy",
        "src/commands/validation.rs",
        "    if save::same_file(source, out) {\n"
        "        return Err(\n"
        '            "The document with the data added',
        "    if false {\n"
        "        return Err(\n"
        '            "The document with the data added',
        "the_originals_own_name_is_refused_before_anybody_is_asked",
    ),
    Mutation(
        "words: call an earlier timestamp a signature",
        "src/words.rs",
        "            .filter(|(_, timestamp)| *timestamp == timestamps)",
        "            .filter(|_| !timestamps)",
        "the_sentence_names_a_timestamp_as_one",
    ),
    Mutation(
        "words: call a lone timestamp that signature",
        "src/words.rs",
        '        [(_, true)] => "that timestamp is",',
        '        [(_, true)] => "that signature is",',
        "the_sentence_names_a_timestamp_as_one",
    ),
    # --- strategic recommendation 1 of the 2026-10-15 review: the platform set
    Mutation(
        "reveal: leave a quote in the URI that gdbus parses (Linux)",
        "src/commands/document.rs",
        """.replace('\\'', "%27")""",
        "",
        "a_file_name_with_quotes_and_specials_cannot_leave_the_uri_string",
        only_on="linux",
    ),
    Mutation(
        "reveal: no bound on how long a file manager may take (Linux)",
        "src/commands/document.rs",
        '        "--timeout",\n        SHOW_ITEMS_TIMEOUT,\n',
        "",
        "show_items_is_one_array_of_one_uri_and_an_empty_startup_id",
        only_on="linux",
    ),
    Mutation(
        "path: send Linux to the macOS menu command (Linux)",
        "src/cli/path.rs",
        "on Linux, the .deb package puts tpdf-cli on",
        "on macOS, the .deb package puts tpdf-cli on",
        "the_refusal_names_this_platform_and_not_the_other",
        only_on="linux",
    ),
    Mutation(
        "path: name Linux in the macOS sentence",
        "src/cli/path.rs",
        "on macOS, choose Install command-line tool",
        "on macOS or Linux, choose Install command-line tool",
        "the_refusal_names_this_platform_and_not_the_other",
        only_on="macos",
    ),
    Mutation(
        "tesseract: open only the Debian soname, as 26.10.19 did (Fedora finds none)",
        "src/ocr_tesseract.rs",
        '    "libtesseract.so.5.5",\n',
        # A duplicate rather than nothing: the array's length is in its type.
        '    "libtesseract.so.5",\n',
        "fedoras_library_is_found_by_its_major_and_minor_name",
        only_on="linux",
    ),
    Mutation(
        "ocr worker: a child with no engine reports a crash instead of why",
        "src/ocr_worker.rs",
        "        Err(self.0.clone())\n",
        '        Err(RecogniseError::Crashed("broken pipe".into()))\n',
        "a_child_with_no_engine_answers_why",
        only_on="linux",
    ),
    Mutation(
        "folder: offer every file, not only PDFs",
        "src/commands/document.rs",
        "        if pdf && candidate.is_file() {",
        "        if candidate.is_file() {",
        "lists_the_pdfs_beside_a_document_and_nothing_else",
    ),
    Mutation(
        "folder: offer a folder named like a PDF",
        "src/commands/document.rs",
        "        if pdf && candidate.is_file() {",
        "        if pdf {",
        "lists_the_pdfs_beside_a_document_and_nothing_else",
    ),
    Mutation(
        "folder: match the extension's case",
        "src/commands/document.rs",
        '            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));',
        '            .is_some_and(|extension| extension == "pdf");',
        "lists_the_pdfs_beside_a_document_and_nothing_else",
    ),
    Mutation(
        "line: take three points",
        "src/docmodel.rs",
        "            Self::Line | Self::Arrow => one(2, 2),",
        "            Self::Line | Self::Arrow => one(2, 3),",
        "a_line_or_a_shape_takes_its_own_number_of_corners",
    ),
    Mutation(
        "polygon: take two corners",
        "src/docmodel.rs",
        "            Self::Polygon => one(3, MAX_CORNERS),",
        "            Self::Polygon => one(2, MAX_CORNERS),",
        "a_line_or_a_shape_takes_its_own_number_of_corners",
    ),
    Mutation(
        "arrow: no room for the head",
        "src/docmodel.rs",
        "        let head = if self == Self::Arrow {",
        "        let head = if self == Self::Polygon {",
        "an_arrows_head_is_inside_its_rectangle",
    ),
    Mutation(
        "line: no rectangle round its points",
        "src/edits.rs",
        "        if want.kind.has_strokes() {",
        "        if want.kind == MarkKind::Ink {",
        "a_line_or_a_shape_gets_the_rectangle_round_its_points",
    ),
    Mutation(
        "polyline: written as a polygon",
        "src/save/marks.rs",
        '        MarkKind::Polyline => b"PolyLine",',
        '        MarkKind::Polyline => b"Polygon",',
        "a_line_and_the_shapes_of_straight_sides_write_their_corners",
    ),
    Mutation(
        "arrow: written with no line ending",
        "src/save/marks.rs",
        "            let end: &[u8] = if mark.kind == MarkKind::Arrow {",
        "            let end: &[u8] = if mark.kind == MarkKind::Line {",
        "a_line_and_the_shapes_of_straight_sides_write_their_corners",
    ),
    Mutation(
        "polygon: left open",
        "src/save/marks.rs",
        "        out.push_str(if kind == MarkKind::Polygon {",
        "        out.push_str(if kind == MarkKind::Polyline {",
        "a_polygon_closes_and_an_arrow_has_a_head",
    ),
]
