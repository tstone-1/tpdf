"""Rust mutation rows: a smaller copy's pictures and preview, page boxes too
large for a number, pictures too long for their page, the content's open `q`,
and named destinations read once.

Added 2026-11-19, each row seen red on the day it was written.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- Which pictures a smaller copy reads, and which it leaves alone ----------------
    Mutation(
        "compress: a picture behind a PNG predictor read to the plain size",
        "src/compress.rs",
        "wanted.checked_add(picture.height)?",
        "wanted",
        "a_picture_behind_a_png_predictor_is_read_and_shrunk_again",
    ),
    Mutation(
        "compress: a JPEG with decode parameters re-encoded",
        "src/compress.rs",
        'if stored == Stored::Jpeg && dict.has(b"DecodeParms") {',
        "if false {",
        "a_jpeg_with_decode_parameters_is_left_as_it_is",
    ),
    Mutation(
        "compress: a picture on a page whose content will not decode scaled",
        "src/compress.rs",
        "        let Ok(content) = Content::decode(data) else {\n            self.unfollowed_in(resources);\n",
        "        let Ok(content) = Content::decode(data) else {\n",
        "a_picture_drawn_where_content_could_not_be_followed_is_not_scaled",
    ),
    Mutation(
        "compress: a picture drawn by a deeper form than is followed scaled",
        "src/compress.rs",
        "                if depth >= MAX_NESTING {\n                    self.unfollowed_in(&nearest);\n",
        "                if depth >= MAX_NESTING {\n",
        "a_picture_drawn_where_content_could_not_be_followed_is_not_scaled",
    ),
    Mutation(
        "compress: a picture in a form whose content will not decode scaled",
        "src/compress.rs",
        "MAX_CONTENT_BYTES) else {\n                    self.unfollowed_in(&nearest);\n",
        "MAX_CONTENT_BYTES) else {\n",
        "a_picture_drawn_where_content_could_not_be_followed_is_not_scaled",
    ),
    Mutation(
        "compress: a page drawing past the bound on draws measured",
        "src/compress.rs",
        "                        self.lost = true;\n                        return;",
        "                        return;",
        "a_picture_drawn_where_content_could_not_be_followed_is_not_scaled",
    ),
    Mutation(
        "compress: a picture of unknown size given a size",
        "src/compress.rs",
        "            if let Some(shown) = walk.shown.get_mut(id) {\n                unknown(shown);",
        "            if let Some(shown) = walk.shown.get_mut(id) {\n                let _ = shown;",
        "a_picture_drawn_where_content_could_not_be_followed_is_not_scaled",
    ),
    Mutation(
        "compress: a picture whose stated size overflows compared by multiplying",
        "src/compress.rs",
        "let (before, after) = (old.0.saturating_mul(old.1), new.0.saturating_mul(new.1));\n            (before.saturating_sub(after), before)",
        "(old.0 * old.1 - new.0 * new.1, old.0 * old.1)",
        "a_picture_whose_stated_size_overflows_is_compared_without_overflowing",
    ),
    # --- The preview, and the size it says it has --------------------------------------
    Mutation(
        "compress: a sample drawn side by side at a size it does not have",
        "src/compress.rs",
        "        self.checked()?;\n",
        "",
        "a_sample_whose_size_is_not_its_pixels_is_not_drawn",
    ),
    # --- A page box too large for a number ---------------------------------------------
    Mutation(
        "page box: an infinite extent kept by the shared reader",
        "src/pagetree.rs",
        "        .filter(|b| (b[2] - b[0]).is_finite() && (b[3] - b[1]).is_finite())\n}",
        "}",
        "a_page_box_too_large_for_a_number_falls_back_like_no_box",
    ),
    Mutation(
        "page box: an infinite extent kept by the comments reader",
        "src/annots.rs",
        "            .filter(|b| (b[2] - b[0]).is_finite() && (b[3] - b[1]).is_finite())\n    };",
        "    };",
        "a_page_box_too_large_for_a_number_falls_back_like_no_box",
    ),
    # --- A page for a picture too long for its shape -----------------------------------
    Mutation(
        "picture pages: a page side past the bounds",
        "src/imagepages.rs",
        "(length * scale).clamp(MIN_PAGE_PT, MAX_PAGE_PT)",
        "(length * scale)",
        "a_page_for_a_picture_too_long_for_its_shape_is_still_within_bounds",
    ),
    # --- Added text: the content's open q ----------------------------------------------
    Mutation(
        "added text: every open q closed however many",
        "src/addedtext.rs",
        "    usize::try_from(depth.max(0)).unwrap_or(0).min(MAX_UNCLOSED)\n",
        "    usize::try_from(depth.max(0)).unwrap_or(0)\n",
        "the_q_closed_for_the_content_are_bounded",
    ),
    # --- Named destinations, read once -------------------------------------------------
    Mutation(
        "named destinations: the last value a name is given kept",
        "src/links.rs",
        "values.entry(key).or_insert(&pair[1]);",
        "values.insert(key, &pair[1]);",
        "named_destinations_resolve_as_a_walk_for_each_name_would",
    ),
    Mutation(
        "named destinations: a tree cut at its bound read as whole",
        "src/links.rs",
        "    if depth == 0 || *budget == 0 {\n        return false;\n    }",
        "    if depth == 0 || *budget == 0 {\n        return true;\n    }",
        "named_destinations_resolve_as_a_walk_for_each_name_would",
    ),
]
