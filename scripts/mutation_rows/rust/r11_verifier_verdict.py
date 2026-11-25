"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.

Whether the verifier read a stream the way a viewer reads it (`verify::as_drawn`
and the trailer), and the one function every redaction command reaches its
verdict through (`redaction_steps::judge`). Named by area like the neighbours,
so `--only 'verify:'` runs these with the verdict's other guards.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- verify: read the way a viewer reads it, or not counted as read ----------
ROWS += [
    Mutation(
        # A picture stored with /FlateDecode or no filter is inflated and
        # searched as if its samples were text, and the file passes where the
        # same picture as a JPEG would not.
        "verify: a lossless picture read as content",
        "src/verify.rs",
        '    if subtype_is(doc, &stream.dict, b"Image") {',
        "    if false {",
        "a_deflated_image_is_deferred_like_a_jpeg",
    ),
    Mutation(
        "verify: an inline image read as the words around it",
        "src/verify.rs",
        "    let inline_picture = draws_inline_picture(&decoded);",
        "    let inline_picture = false;",
        "an_inline_image_in_a_content_stream_is_deferred",
    ),
    Mutation(
        # `lopdf` takes an indirect /DecodeParms as none, so the predictor is
        # not undone and the words are not in what the search reads.
        "verify: decoding parameters lopdf passes over read as none",
        "src/verify.rs",
        "    if let Some(why) = parameters_unread(object, stream) {",
        "    if let Some(why) = None::<String> {",
        "a_stream_whose_decode_parms_is_a_reference_withholds_the_verdict",
    ),
    Mutation(
        # PDFium applies PNG for every predictor from 10 up, `lopdf` only to 15.
        "verify: a predictor above 15 read as none",
        "src/verify.rs",
        "            16.. => return false,",
        "            16.. => {}",
        "a_stream_whose_decode_parms_is_a_reference_withholds_the_verdict",
    ),
    Mutation(
        "verify: a composite font named by reference read as no font",
        "src/verify.rs",
        '        if subtype_is(doc, dict, b"Type0") {',
        '        if dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Type0") {',
        "a_composite_font_named_by_reference_is_still_composite",
    ),
    Mutation(
        # `lopdf` returns what a failed inflate produced, as `Ok`.
        "verify: a decode cut short read as whole",
        "src/verify.rs",
        "    if stopped_part_way(stream, filters) {",
        "    if false {",
        "a_flate_stream_cut_short_withholds_the_verdict",
    ),
    Mutation(
        # The over-refusal direction: a stream whose only fault is its checksum
        # was read to its end by `lopdf` too.
        "verify: a checksum fault called a stream cut short",
        "src/verify.rs",
        "        || (input.len() > 2 && whole(&mut flate2::read::DeflateDecoder::new(&input[2..])))",
        "        || (false && whole(&mut flate2::read::DeflateDecoder::new(&input[2..])))",
        "a_flate_stream_cut_short_withholds_the_verdict",
    ),
    Mutation(
        "verify: the trailer's strings left unread",
        "src/verify.rs",
        "                if find(&trailer, needle.as_bytes()) {",
        "                if false {",
        "a_title_in_an_info_dictionary_written_into_the_trailer_is_found",
    ),
    Mutation(
        "verify: a copy in the trailer placed on the page of another copy",
        "src/verify.rs",
        "                    carriers.entry(needle.clone()).or_default().unreached = true;",
        "                    carriers.entry(needle.clone()).or_default().unreached = false;",
        "a_title_in_an_info_dictionary_written_into_the_trailer_is_found",
    ),
]

# --- redaction: one verdict for the three commands ---------------------------
ROWS += [
    Mutation(
        # The code reader is the one reader the image-only copy shares with the
        # rewrite, and the one it lacked.
        "redaction: the code reader left out of the verdict",
        "src/redaction_steps.rs",
        "    why.extend(codes.why);",
        "    let _ = codes.why;",
        "an_image_only_copy_reports_a_code_that_still_carries_the_name",
    ),
    Mutation(
        "redaction: an image-only copy forgets a form it could not read",
        "src/redaction_steps.rs",
        "                &mut asked.needles_short.iter().cloned(),",
        "                &mut std::iter::empty(),",
        "an_image_only_copy_is_judged_on_what_applies_to_pictures",
    ),
    Mutation(
        # A picture copy keeps no object of the source, so a sentence about an
        # object the removal left is not true of it.
        "redaction: an image-only copy reports the objects a rewrite left",
        "src/redaction_steps.rs",
        "                &mut asked.needles_short.iter().cloned(),",
        "                &mut asked.concerns.iter().map(|(_, why)| why.clone()),",
        "an_image_only_copy_is_judged_on_what_applies_to_pictures",
    ),
    Mutation(
        "redaction: the extra reader's findings left out of a rewrite's verdict",
        "src/redaction_steps.rs",
        "            add(ReasonKind::Found, &mut also.into_iter());",
        "            let _ = also;",
        "a_rewrite_is_judged_on_every_reader_in_order",
    ),
    Mutation(
        # The window groups the reasons by this kind (`redactreview.ts`); a
        # word the scan found shown as a part it could not read tells the
        # reader there is nothing to fix.
        "redaction: a word the scan found given the kind of an unread part",
        "src/redaction_steps.rs",
        "                    let kind = if at < found {",
        "                    let kind = if false && at < found {",
        "a_rewrite_is_judged_on_every_reader_in_order",
    ),
    Mutation(
        "redaction: the picture left in the file unsaid",
        "src/redaction_steps.rs",
        "            taken_here = asked.taken_here.to_vec();",
        "            let _ = asked.taken_here;",
        "a_rewrite_is_judged_on_every_reader_in_order",
    ),
]
